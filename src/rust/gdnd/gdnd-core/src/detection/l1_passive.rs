//! L1 Passive Detection
//!
//! High-frequency, low-overhead passive detection:
//! - NVML metrics query (temperature, power, utilization)
//! - XID error scanning (from dmesg)
//! - Zombie process detection (D state processes)
//! - ECC error monitoring

use std::sync::Arc;

use tracing::{debug, trace, warn};

use super::{DetectionLevel, DetectionResult, Finding, FindingType};
use crate::device::{DeviceError, DeviceId, DeviceInterface};

/// L1 Passive Detector
pub struct L1PassiveDetector {
    device: Arc<dyn DeviceInterface>,
    temperature_threshold: u32,
    fatal_xids: Vec<u32>,
}

impl L1PassiveDetector {
    /// Create a new L1 passive detector
    pub fn new(
        device: Arc<dyn DeviceInterface>,
        temperature_threshold: u32,
        fatal_xids: Vec<u32>,
    ) -> Self {
        Self {
            device,
            temperature_threshold,
            fatal_xids,
        }
    }

    /// Run passive detection on a single device
    pub async fn detect(&self, device: &DeviceId) -> Result<DetectionResult, DeviceError> {
        let mut findings = Vec::new();

        // 1. Check metrics (temperature, ECC errors)
        match self.device.get_metrics(device).await {
            Ok(metrics) => {
                trace!(
                    device = %device,
                    temp = metrics.temperature,
                    gpu_util = metrics.gpu_utilization,
                    mem_util = metrics.memory_utilization,
                    "Device metrics"
                );

                // Check temperature
                if metrics.temperature > self.temperature_threshold {
                    warn!(
                        device = %device,
                        temp = metrics.temperature,
                        threshold = self.temperature_threshold,
                        "High temperature detected"
                    );
                    findings.push(Finding::high_temperature(
                        metrics.temperature,
                        self.temperature_threshold,
                    ));
                }

                // Check double-bit ECC errors
                if metrics.ecc_errors.double_bit > 0 {
                    warn!(
                        device = %device,
                        count = metrics.ecc_errors.double_bit,
                        "Double-bit ECC errors detected"
                    );
                    findings.push(Finding::double_bit_ecc(metrics.ecc_errors.double_bit));
                }
            }
            Err(e) => {
                warn!(device = %device, error = %e, "Failed to get device metrics");
                // Don't fail the entire detection if metrics query fails
            }
        }

        // 2. Check XID errors
        match self.device.get_xid_errors(device).await {
            Ok(xid_errors) => {
                for xid in xid_errors {
                    if xid.is_fatal(&self.fatal_xids) {
                        warn!(
                            device = %device,
                            xid = xid.code,
                            message = %xid.message,
                            "Fatal XID error detected"
                        );
                        findings.push(Finding::fatal_xid(xid.code, &xid.message));
                    } else {
                        debug!(
                            device = %device,
                            xid = xid.code,
                            "Non-fatal XID error"
                        );
                        findings.push(Finding::new(
                            FindingType::NonFatalXid(xid.code),
                            xid.message.clone(),
                            false,
                        ));
                    }
                }
            }
            Err(e) => {
                debug!(device = %device, error = %e, "Failed to scan XID errors");
            }
        }

        // 3. Check zombie processes
        match self.device.check_zombie_processes(device).await {
            Ok(zombie_pids) => {
                for pid in zombie_pids {
                    warn!(device = %device, pid = pid, "Zombie GPU process detected");
                    findings.push(Finding::zombie_process(pid));
                }
            }
            Err(e) => {
                debug!(device = %device, error = %e, "Failed to check zombie processes");
            }
        }

        // Determine overall result
        if findings.is_empty() {
            Ok(DetectionResult::pass(device.clone(), DetectionLevel::L1Passive))
        } else {
            Ok(DetectionResult::fail(
                device.clone(),
                DetectionLevel::L1Passive,
                findings,
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
    async fn test_l1_detect_healthy() {
        let mock = Arc::new(MockDevice::new());
        let detector = L1PassiveDetector::new(mock.clone(), 85, vec![31, 43, 48, 79]);

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(result.passed);
        assert!(result.findings.is_empty());
    }

    #[tokio::test]
    async fn test_l1_detect_high_temp() {
        let mock = Arc::new(MockDevice::new());
        mock.set_temperature(90);
        let detector = L1PassiveDetector::new(mock.clone(), 85, vec![31, 43, 48, 79]);

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(!result.passed);
        assert!(result
            .findings
            .iter()
            .any(|f| matches!(f.finding_type, FindingType::HighTemperature)));
    }

    #[tokio::test]
    async fn test_l1_detect_fatal_xid() {
        let mock = Arc::new(MockDevice::new());
        mock.add_xid_error(31, 0).await; // Fatal XID
        let detector = L1PassiveDetector::new(mock.clone(), 85, vec![31, 43, 48, 79]);

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(!result.passed);
        assert!(result.has_fatal_finding());
    }

    #[tokio::test]
    async fn test_l1_detect_zombie() {
        let mock = Arc::new(MockDevice::new());
        mock.add_zombie_pid(12345).await;
        let detector = L1PassiveDetector::new(mock.clone(), 85, vec![31, 43, 48, 79]);

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(!result.passed);
        assert!(result
            .findings
            .iter()
            .any(|f| matches!(f.finding_type, FindingType::ZombieProcess)));
    }
}
