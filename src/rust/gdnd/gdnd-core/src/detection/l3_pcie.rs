//! L3 PCIe Bandwidth Detection
//!
//! Low-frequency PCIe bandwidth testing to detect link degradation.
//! This is typically run daily or on-demand.
//!
//! Detects:
//! - PCIe link degradation (e.g., x16 -> x8)
//! - Bandwidth falling below expected thresholds
//! - NVLink/NVSwitch issues (on supported hardware)

use std::sync::Arc;

use tracing::{debug, info, warn};

use super::{DetectionLevel, DetectionResult, Finding, FindingType};
use crate::device::{DeviceError, DeviceId, DeviceInterface};

/// L3 PCIe bandwidth test configuration
#[derive(Debug, Clone)]
pub struct L3PcieConfig {
    /// Minimum expected bandwidth in GB/s
    pub min_bandwidth_gbps: f64,
    /// Whether to skip test if device doesn't support it
    pub skip_if_unsupported: bool,
}

impl Default for L3PcieConfig {
    fn default() -> Self {
        Self {
            // PCIe 3.0 x16 theoretical: ~16 GB/s, practical ~12 GB/s
            // PCIe 4.0 x16 theoretical: ~32 GB/s, practical ~24 GB/s
            // Set conservative threshold to catch major degradation
            min_bandwidth_gbps: 8.0,
            skip_if_unsupported: true,
        }
    }
}

/// L3 PCIe Bandwidth Detector
pub struct L3PcieDetector {
    device: Arc<dyn DeviceInterface>,
    config: L3PcieConfig,
}

impl L3PcieDetector {
    /// Create a new L3 PCIe detector with default config
    pub fn new(device: Arc<dyn DeviceInterface>) -> Self {
        Self {
            device,
            config: L3PcieConfig::default(),
        }
    }

    /// Create a new L3 PCIe detector with custom config
    pub fn with_config(device: Arc<dyn DeviceInterface>, config: L3PcieConfig) -> Self {
        Self { device, config }
    }

    /// Check if PCIe testing is supported
    pub fn is_supported(&self) -> bool {
        self.device.supports_pcie_test()
    }

    /// Run PCIe bandwidth test on a single device
    pub async fn detect(&self, device: &DeviceId) -> Result<DetectionResult, DeviceError> {
        // Check if PCIe test is supported
        if !self.device.supports_pcie_test() {
            if self.config.skip_if_unsupported {
                debug!(
                    device = %device,
                    "PCIe test not supported, skipping"
                );
                return Ok(DetectionResult::pass(device.clone(), DetectionLevel::L3Pcie));
            } else {
                return Ok(DetectionResult::fail(
                    device.clone(),
                    DetectionLevel::L3Pcie,
                    vec![Finding::new(
                        FindingType::PcieDegradation,
                        "PCIe bandwidth test not supported on this device".to_string(),
                        false,
                    )],
                ));
            }
        }

        info!(device = %device, "Running L3 PCIe bandwidth test");

        let result = self.device.run_pcie_test(device).await?;

        if result.passed {
            info!(
                device = %device,
                duration = ?result.duration,
                "L3 PCIe bandwidth test passed"
            );
            Ok(DetectionResult::pass(device.clone(), DetectionLevel::L3Pcie))
        } else {
            let error_msg = result
                .error
                .unwrap_or_else(|| "PCIe bandwidth test failed".to_string());

            warn!(
                device = %device,
                error = %error_msg,
                "L3 PCIe bandwidth test failed - possible link degradation"
            );

            Ok(DetectionResult::fail(
                device.clone(),
                DetectionLevel::L3Pcie,
                vec![Finding::new(
                    FindingType::PcieDegradation,
                    error_msg,
                    false, // PCIe degradation is not immediately fatal
                )],
            ))
        }
    }

    /// Run detection on all devices
    pub async fn detect_all(&self) -> Result<Vec<DetectionResult>, DeviceError> {
        if !self.is_supported() && self.config.skip_if_unsupported {
            debug!("PCIe test not supported, skipping all devices");
            return Ok(Vec::new());
        }

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
    async fn test_l3_pcie_supported() {
        let mock = Arc::new(MockDevice::new());
        let detector = L3PcieDetector::new(mock);

        assert!(detector.is_supported());
    }

    #[tokio::test]
    async fn test_l3_pcie_detect_pass() {
        let mock = Arc::new(MockDevice::new());
        let detector = L3PcieDetector::new(mock.clone());

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(result.passed);
        assert!(result.findings.is_empty());
        assert_eq!(result.level, DetectionLevel::L3Pcie);
    }

    #[tokio::test]
    async fn test_l3_pcie_detect_all() {
        let mock = Arc::new(MockDevice::new());
        let detector = L3PcieDetector::new(mock);

        let results = detector.detect_all().await.unwrap();

        assert_eq!(results.len(), 2); // MockDevice has 2 GPUs by default
        assert!(results.iter().all(|r| r.passed));
    }

    #[tokio::test]
    async fn test_l3_pcie_config() {
        let mock = Arc::new(MockDevice::new());
        let config = L3PcieConfig {
            min_bandwidth_gbps: 12.0,
            skip_if_unsupported: false,
        };
        let detector = L3PcieDetector::with_config(mock, config);

        assert!(detector.is_supported());
    }

    #[tokio::test]
    async fn test_l3_pcie_detect_fail() {
        let mock = Arc::new(MockDevice::new());
        mock.set_fail_pcie_test(true);
        let detector = L3PcieDetector::new(mock.clone());

        let devices = mock.list_devices().await.unwrap();
        let result = detector.detect(&devices[0]).await.unwrap();

        assert!(!result.passed);
        assert!(result
            .findings
            .iter()
            .any(|f| matches!(f.finding_type, FindingType::PcieDegradation)));
    }
}
