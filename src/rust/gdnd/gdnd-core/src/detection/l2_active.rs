//! L2 Active Detection
//!
//! Medium-frequency active detection that runs small GPU computations
//! to detect issues that passive monitoring cannot catch:
//! - Driver deadlocks
//! - GPU hangs
//! - Compute capability issues

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, warn};

use super::{DetectionLevel, DetectionResult, Finding, FindingType};
use crate::device::{DeviceError, DeviceId, DeviceInterface};

/// L2 Active Detector
pub struct L2ActiveDetector {
    device: Arc<dyn DeviceInterface>,
    #[allow(dead_code)]
    gpu_check_path: String,
    timeout: Duration,
}

impl L2ActiveDetector {
    /// Create a new L2 active detector
    pub fn new(
        device: Arc<dyn DeviceInterface>,
        gpu_check_path: String,
        timeout: Duration,
    ) -> Self {
        Self {
            device,
            gpu_check_path,
            timeout,
        }
    }

    /// Run active detection on a single device
    pub async fn detect(&self, device: &DeviceId) -> Result<DetectionResult, DeviceError> {
        debug!(device = %device, timeout = ?self.timeout, "Running L2 active check");

        let result = self.device.run_active_check(device, self.timeout).await?;

        if result.passed {
            debug!(
                device = %device,
                duration = ?result.duration,
                "L2 active check passed"
            );
            Ok(DetectionResult::pass(device.clone(), DetectionLevel::L2Active))
        } else {
            let finding = if result.error.as_ref().map_or(false, |e| e.contains("timed out")) {
                warn!(device = %device, timeout = ?self.timeout, "L2 active check timed out");
                Finding::new(
                    FindingType::ActiveCheckTimeout,
                    format!("Active check timed out after {:?}", self.timeout),
                    false,
                )
            } else {
                let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
                warn!(device = %device, error = %error_msg, "L2 active check failed");
                Finding::active_check_failure(&error_msg)
            };

            Ok(DetectionResult::fail(
                device.clone(),
                DetectionLevel::L2Active,
                vec![finding],
            ))
        }
    }

    /// Run detection on all devices
    pub async fn detect_all(&self) -> Result<Vec<DetectionResult>, DeviceError> {
        let devices = self.device.list_devices().await?;
        let mut results = Vec::with_capacity(devices.len());

        for device in &devices {
            let result = self.detect(device).await?;
            results.push(result);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::MockDevice;

    #[tokio::test]
    async fn test_l2_detect_pass() {
        let mock = Arc::new(MockDevice::new());
        let detector = L2ActiveDetector::new(
            mock.clone(),
            "/usr/local/bin/gpu-check".to_string(),
            Duration::from_secs(5),
        );

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(result.passed);
        assert!(result.findings.is_empty());
    }

    #[tokio::test]
    async fn test_l2_detect_fail() {
        let mock = Arc::new(MockDevice::new());
        mock.set_fail_active_check(true);
        let detector = L2ActiveDetector::new(
            mock.clone(),
            "/usr/local/bin/gpu-check".to_string(),
            Duration::from_secs(5),
        );

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(!result.passed);
        assert!(result
            .findings
            .iter()
            .any(|f| matches!(f.finding_type, FindingType::ActiveCheckFailure)));
    }
}
