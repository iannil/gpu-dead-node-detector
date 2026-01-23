//! GPU Dead Node Detector (GDND)
//!
//! Kubernetes-based GPU node fault detection and isolation system.
//! Runs as a DaemonSet on GPU nodes to detect unhealthy GPUs and automatically
//! isolate faulty nodes via Taint/Cordon.

mod cli;
mod config;

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::signal;
use tokio::sync::{watch, RwLock};
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use cli::Cli;
use config::{Config, HealingStrategy as ConfigHealingStrategy};
use gdnd_core::detection::{L1PassiveDetector, L2ActiveDetector, L3PcieDetector};
use gdnd_core::device::{create_device_interface, DeviceType as CoreDeviceType};
use gdnd_core::healing::{HealingConfig as CoreHealingConfig, HealingStrategy as CoreHealingStrategy, SelfHealer};
use gdnd_core::metrics::MetricsRegistry;
use gdnd_core::scheduler::{DetectionScheduler, IsolationExecutor};
use gdnd_core::state_machine::{GpuHealthManager, StateTransition};
use gdnd_k8s::client::K8sClient;
use gdnd_k8s::node_ops::{IsolationConfig, NodeOperator};

/// Initialize the tracing/logging subsystem
fn init_logging(log_level: &str, json_format: bool) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    if json_format {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer())
            .init();
    }
}

/// Convert CLI device type to core device type
fn to_core_device_type(dt: config::DeviceType) -> CoreDeviceType {
    match dt {
        config::DeviceType::Auto => CoreDeviceType::Auto,
        config::DeviceType::Nvidia => CoreDeviceType::Nvidia,
        config::DeviceType::Ascend => CoreDeviceType::Ascend,
    }
}

/// Convert config isolation to k8s isolation config
fn to_k8s_isolation_config(config: &config::IsolationConfig) -> IsolationConfig {
    IsolationConfig {
        cordon: config.cordon,
        evict_pods: config.evict_pods,
        taint_key: config.taint_key.clone(),
        taint_value: config.taint_value.clone(),
        taint_effect: config.taint_effect.clone(),
    }
}

/// Convert config healing strategy to core healing strategy
fn to_core_healing_strategy(strategy: ConfigHealingStrategy) -> CoreHealingStrategy {
    match strategy {
        ConfigHealingStrategy::Conservative => CoreHealingStrategy::Conservative,
        ConfigHealingStrategy::Moderate => CoreHealingStrategy::Moderate,
        ConfigHealingStrategy::Aggressive => CoreHealingStrategy::Aggressive,
    }
}

/// Node operator wrapper implementing IsolationExecutor
struct NodeOperatorExecutor {
    operator: NodeOperator,
}

impl NodeOperatorExecutor {
    fn new(operator: NodeOperator) -> Self {
        Self { operator }
    }
}

#[async_trait::async_trait]
impl IsolationExecutor for NodeOperatorExecutor {
    async fn execute(&self, transition: &StateTransition) -> Result<()> {
        self.operator.execute_actions(&transition.actions).await
    }
}

/// Run the main detection loop
async fn run(config: Config, shutdown_rx: watch::Receiver<bool>) -> Result<()> {
    let node_name = config
        .node_name
        .clone()
        .context("Node name must be specified via config, --node-name, or NODE_NAME env")?;

    info!(node = %node_name, "Starting GDND on node");

    // Create device interface based on device type
    let device = create_device_interface(to_core_device_type(config.device_type))
        .await
        .context("Failed to create device interface")?;

    // List available devices
    let devices = device.list_devices().await?;
    info!(count = devices.len(), "Discovered GPU devices");

    if devices.is_empty() {
        warn!("No GPU devices found on this node");
    }

    // Initialize metrics registry
    let metrics = Arc::new(MetricsRegistry::new());
    metrics.set_gpu_count(devices.len() as i64);

    // Initialize K8s client and node operator
    let k8s_client = K8sClient::new().await?;
    let node_operator = NodeOperator::new(
        k8s_client,
        node_name.clone(),
        to_k8s_isolation_config(&config.isolation),
        config.dry_run,
    );
    let executor = Arc::new(NodeOperatorExecutor::new(node_operator));

    // Initialize health state manager
    let health_manager = if config.recovery.enabled {
        info!(
            threshold = config.recovery.threshold,
            "Recovery detection enabled"
        );
        Arc::new(RwLock::new(GpuHealthManager::with_recovery(
            config.health.failure_threshold,
            config.recovery.threshold,
            config.health.fatal_xids.clone(),
        )))
    } else {
        Arc::new(RwLock::new(GpuHealthManager::new(
            config.health.failure_threshold,
            config.health.fatal_xids.clone(),
        )))
    };

    // Initialize detectors
    let l1_detector = L1PassiveDetector::new(
        device.clone(),
        config.health.temperature_threshold,
        config.health.fatal_xids.clone(),
    );

    let l2_detector = L2ActiveDetector::new(
        device.clone(),
        config.gpu_check_path.clone(),
        config.health.active_check_timeout,
    );

    // Create scheduler
    let mut scheduler = DetectionScheduler::new(
        l1_detector,
        l2_detector,
        health_manager.clone(),
        executor,
        metrics.clone(),
        config.l1_interval,
        config.l2_interval,
    );

    // Attach self-healer if healing is enabled
    if config.healing.enabled {
        info!(
            strategy = ?config.healing.strategy,
            dry_run = config.healing.dry_run,
            "Self-healing enabled"
        );

        let core_device_type = to_core_device_type(config.device_type);
        let healing_config = CoreHealingConfig {
            enabled: config.healing.enabled,
            strategy: to_core_healing_strategy(config.healing.strategy),
            timeout: config.healing.timeout,
            dry_run: config.healing.dry_run,
        };

        let healer = SelfHealer::new(healing_config, core_device_type);
        scheduler = scheduler.with_healer(healer);
    }

    // Attach L3 PCIe detector if enabled
    if config.l3_enabled {
        info!(
            interval = ?config.l3_interval,
            "L3 PCIe detection enabled"
        );

        let l3_detector = L3PcieDetector::new(device.clone());
        scheduler = scheduler.with_l3(l3_detector, config.l3_interval);
    }

    // Start metrics server if enabled
    if config.metrics.enabled {
        let port = config.metrics.port;
        tokio::spawn(async move {
            if let Err(e) = start_metrics_server(port).await {
                error!(error = %e, "Metrics server failed");
            }
        });
    }

    // Run the main detection loop
    scheduler.run(shutdown_rx).await?;

    info!("GDND shutdown complete");
    Ok(())
}

/// Start the Prometheus metrics HTTP server
async fn start_metrics_server(port: u16) -> Result<()> {
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!(port = port, "Metrics server listening");

    loop {
        let (mut socket, _) = listener.accept().await?;

        tokio::spawn(async move {
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await;

            let metrics_output = prometheus::TextEncoder::new()
                .encode_to_string(&prometheus::gather())
                .unwrap_or_default();

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
                metrics_output.len(),
                metrics_output
            );

            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse_args();

    // Initialize logging
    init_logging(&cli.log_level, cli.log_json);

    info!(version = env!("CARGO_PKG_VERSION"), "GDND starting");

    // Load configuration
    let mut config = if cli.config.exists() {
        Config::from_file(&cli.config)
            .with_context(|| format!("Failed to load config from {:?}", cli.config))?
    } else {
        warn!(path = ?cli.config, "Config file not found, using defaults");
        Config::default()
    };

    // Apply CLI overrides
    if cli.dry_run {
        config.dry_run = true;
    }
    if cli.node_name.is_some() {
        config.node_name = cli.node_name;
    }

    // Load node name from environment if not set
    config = config.with_node_name_from_env();

    // Validate configuration
    config.validate().context("Invalid configuration")?;

    info!(dry_run = config.dry_run, "Configuration loaded");

    // Setup shutdown signal handler
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install SIGTERM handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {
                info!("Received Ctrl+C, initiating shutdown");
            }
            _ = terminate => {
                info!("Received SIGTERM, initiating shutdown");
            }
        }

        let _ = shutdown_tx.send(true);
    });

    // Run single pass if --once flag is set
    if cli.once {
        info!("Running single detection pass (--once mode)");
        // Create minimal setup for single pass
        let device = create_device_interface(to_core_device_type(config.device_type))
            .await
            .context("Failed to create device interface")?;

        let l1_detector = L1PassiveDetector::new(
            device.clone(),
            config.health.temperature_threshold,
            config.health.fatal_xids.clone(),
        );

        let results = l1_detector.detect_all().await?;
        for result in &results {
            if result.passed {
                info!(device = %result.device, "Device healthy");
            } else {
                warn!(
                    device = %result.device,
                    findings = ?result.findings,
                    "Device has issues"
                );
            }
        }

        return Ok(());
    }

    // Run main loop
    run(config, shutdown_rx).await
}
