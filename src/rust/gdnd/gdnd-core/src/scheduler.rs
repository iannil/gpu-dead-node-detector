//! Detection Scheduler
//!
//! Coordinates L1 and L2 detection loops and handles state transitions.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::{watch, RwLock};
use tracing::{debug, error, info, warn};

use crate::detection::{DetectionLevel, DetectionResult, L1PassiveDetector, L2ActiveDetector, L3PcieDetector};
use crate::healing::SelfHealer;
use crate::metrics::MetricsRegistry;
use crate::state_machine::{GpuHealthManager, HealthEvent, HealthState, StateTransition};

/// Trait for executing isolation actions
#[async_trait::async_trait]
pub trait IsolationExecutor: Send + Sync {
    /// Execute isolation actions from state transition
    async fn execute(&self, transition: &StateTransition) -> Result<()>;
}

/// Detection scheduler
pub struct DetectionScheduler<E: IsolationExecutor> {
    l1_detector: L1PassiveDetector,
    l2_detector: L2ActiveDetector,
    l3_detector: Option<L3PcieDetector>,
    health_manager: Arc<RwLock<GpuHealthManager>>,
    isolation_executor: Arc<E>,
    healer: Option<Arc<SelfHealer>>,
    metrics: Arc<MetricsRegistry>,
    l1_interval: Duration,
    l2_interval: Duration,
    l3_interval: Option<Duration>,
}

impl<E: IsolationExecutor + 'static> DetectionScheduler<E> {
    /// Create a new detection scheduler
    pub fn new(
        l1_detector: L1PassiveDetector,
        l2_detector: L2ActiveDetector,
        health_manager: Arc<RwLock<GpuHealthManager>>,
        isolation_executor: Arc<E>,
        metrics: Arc<MetricsRegistry>,
        l1_interval: Duration,
        l2_interval: Duration,
    ) -> Self {
        Self {
            l1_detector,
            l2_detector,
            l3_detector: None,
            health_manager,
            isolation_executor,
            healer: None,
            metrics,
            l1_interval,
            l2_interval,
            l3_interval: None,
        }
    }

    /// Set the self-healer for automatic recovery attempts
    pub fn with_healer(mut self, healer: SelfHealer) -> Self {
        self.healer = Some(Arc::new(healer));
        self
    }

    /// Set the L3 PCIe detector for bandwidth testing
    pub fn with_l3(mut self, detector: L3PcieDetector, interval: Duration) -> Self {
        self.l3_detector = Some(detector);
        self.l3_interval = Some(interval);
        self
    }

    /// Run the detection loop
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        info!(
            l1_interval = ?self.l1_interval,
            l2_interval = ?self.l2_interval,
            l3_interval = ?self.l3_interval,
            l3_enabled = self.l3_detector.is_some(),
            "Starting detection scheduler"
        );

        let mut l1_ticker = tokio::time::interval(self.l1_interval);
        let mut l2_ticker = tokio::time::interval(self.l2_interval);

        // Skip immediate first tick
        l1_ticker.tick().await;
        l2_ticker.tick().await;

        // L3 ticker only if enabled
        let mut l3_ticker = self.l3_interval.map(tokio::time::interval);
        if let Some(ref mut ticker) = l3_ticker {
            ticker.tick().await;
        }

        loop {
            tokio::select! {
                _ = l1_ticker.tick() => {
                    if let Err(e) = self.run_l1_detection().await {
                        error!(error = %e, "L1 detection failed");
                    }
                }
                _ = l2_ticker.tick() => {
                    if let Err(e) = self.run_l2_detection().await {
                        error!(error = %e, "L2 detection failed");
                    }
                }
                _ = async {
                    if let Some(ref mut ticker) = l3_ticker {
                        ticker.tick().await
                    } else {
                        std::future::pending::<tokio::time::Instant>().await
                    }
                } => {
                    if let Err(e) = self.run_l3_detection().await {
                        error!(error = %e, "L3 detection failed");
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Shutdown signal received, stopping scheduler");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Run L1 passive detection on all devices
    async fn run_l1_detection(&self) -> Result<()> {
        debug!("Running L1 passive detection");
        let start = Instant::now();

        let results = self.l1_detector.detect_all().await?;

        for result in results {
            self.process_result(&result).await?;
            self.update_metrics(&result, start.elapsed());
        }

        debug!(duration = ?start.elapsed(), "L1 detection complete");
        Ok(())
    }

    /// Run L2 active detection on all devices
    async fn run_l2_detection(&self) -> Result<()> {
        debug!("Running L2 active detection");
        let start = Instant::now();

        let results = self.l2_detector.detect_all().await?;

        for result in results {
            self.process_result(&result).await?;
            self.update_metrics(&result, start.elapsed());
        }

        debug!(duration = ?start.elapsed(), "L2 detection complete");
        Ok(())
    }

    /// Run L3 PCIe detection on all devices
    async fn run_l3_detection(&self) -> Result<()> {
        let Some(detector) = &self.l3_detector else {
            return Ok(());
        };

        debug!("Running L3 PCIe detection");
        let start = Instant::now();

        let results = detector.detect_all().await?;

        for result in results {
            self.process_result(&result).await?;
            self.update_metrics(&result, start.elapsed());
        }

        debug!(duration = ?start.elapsed(), "L3 detection complete");
        Ok(())
    }

    /// Process a detection result
    async fn process_result(&self, result: &DetectionResult) -> Result<()> {
        let mut manager = self.health_manager.write().await;
        let transition = manager.process_result(result);

        // Update health status metric
        if let Some(health) = manager.get(&result.device) {
            self.metrics.set_gpu_status(&result.device, health.state);
        }

        drop(manager); // Release lock before executing actions

        // Execute isolation actions if state changed to UNHEALTHY
        if transition.changed && transition.to == HealthState::Unhealthy {
            info!(
                device = %result.device,
                from = %transition.from,
                to = %transition.to,
                "Device became unhealthy"
            );

            // Attempt self-healing if enabled
            if let Some(healer) = &self.healer {
                if healer.is_enabled() {
                    info!(device = %result.device, "Attempting self-healing before isolation");
                    let device = result.device.clone();
                    let healer = Arc::clone(healer);

                    // Run healing in blocking task since it uses std::process::Command
                    let heal_results = tokio::task::spawn_blocking(move || {
                        healer.heal(&device)
                    })
                    .await;

                    match heal_results {
                        Ok(Ok(results)) => {
                            for heal_result in &results {
                                if heal_result.success {
                                    info!(
                                        device = %result.device,
                                        action = ?heal_result.action,
                                        message = ?heal_result.message,
                                        "Healing action succeeded"
                                    );
                                } else {
                                    warn!(
                                        device = %result.device,
                                        action = ?heal_result.action,
                                        message = ?heal_result.message,
                                        "Healing action failed"
                                    );
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            warn!(device = %result.device, error = %e, "Healing failed");
                        }
                        Err(e) => {
                            error!(device = %result.device, error = %e, "Healing task panicked");
                        }
                    }
                }
            }

            // Proceed with isolation regardless of healing outcome
            info!(device = %result.device, "Executing isolation actions");

            if let Err(e) = self.isolation_executor.execute(&transition).await {
                error!(error = %e, "Failed to execute isolation actions");
                return Err(e);
            }

            // Mark isolation as completed
            let mut manager = self.health_manager.write().await;
            manager.transition(&result.device, HealthEvent::IsolationCompleted);
        }

        Ok(())
    }

    /// Update metrics for a detection result
    fn update_metrics(&self, result: &DetectionResult, duration: Duration) {
        let level = match result.level {
            DetectionLevel::L1Passive => "L1",
            DetectionLevel::L2Active => "L2",
            DetectionLevel::L3Pcie => "L3",
        };

        self.metrics
            .observe_check_duration(level, &result.device, duration.as_secs_f64());

        if !result.passed {
            for finding in &result.findings {
                let reason = format!("{:?}", finding.finding_type);
                self.metrics.inc_check_failure(level, &result.device, &reason);
            }
        }
    }

    /// Run a single detection pass (for --once mode)
    pub async fn run_once(&self) -> Result<()> {
        info!("Running single detection pass");

        // Run L1, L2, and L3 (if enabled)
        self.run_l1_detection().await?;
        self.run_l2_detection().await?;
        self.run_l3_detection().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MockDevice;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockExecutor {
        call_count: AtomicU32,
    }

    impl MockExecutor {
        fn new() -> Self {
            Self {
                call_count: AtomicU32::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl IsolationExecutor for MockExecutor {
        async fn execute(&self, _transition: &StateTransition) -> Result<()> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_scheduler_run_once() {
        let device = Arc::new(MockDevice::new());
        let l1_detector =
            L1PassiveDetector::new(device.clone(), 85, vec![31, 43, 48, 79]);
        let l2_detector = L2ActiveDetector::new(
            device.clone(),
            "/usr/local/bin/gpu-check".to_string(),
            Duration::from_secs(5),
        );

        let health_manager = Arc::new(RwLock::new(GpuHealthManager::new(
            3,
            vec![31, 43, 48, 79],
        )));

        let executor = Arc::new(MockExecutor::new());
        let metrics = Arc::new(MetricsRegistry::new());

        let scheduler = DetectionScheduler::new(
            l1_detector,
            l2_detector,
            health_manager,
            executor,
            metrics,
            Duration::from_secs(30),
            Duration::from_secs(300),
        );

        scheduler.run_once().await.unwrap();
    }
}
