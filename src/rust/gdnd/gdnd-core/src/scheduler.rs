//! Detection Scheduler
//!
//! Coordinates L1 and L2 detection loops and handles state transitions.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::{watch, RwLock};
use tracing::{debug, error, info};

use crate::detection::{DetectionLevel, DetectionResult, L1PassiveDetector, L2ActiveDetector};
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
    health_manager: Arc<RwLock<GpuHealthManager>>,
    isolation_executor: Arc<E>,
    metrics: Arc<MetricsRegistry>,
    l1_interval: Duration,
    l2_interval: Duration,
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
            health_manager,
            isolation_executor,
            metrics,
            l1_interval,
            l2_interval,
        }
    }

    /// Run the detection loop
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        info!(
            l1_interval = ?self.l1_interval,
            l2_interval = ?self.l2_interval,
            "Starting detection scheduler"
        );

        let mut l1_ticker = tokio::time::interval(self.l1_interval);
        let mut l2_ticker = tokio::time::interval(self.l2_interval);

        // Skip immediate first tick
        l1_ticker.tick().await;
        l2_ticker.tick().await;

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
                "Executing isolation actions"
            );

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

        // Run both L1 and L2
        self.run_l1_detection().await?;
        self.run_l2_detection().await?;

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
