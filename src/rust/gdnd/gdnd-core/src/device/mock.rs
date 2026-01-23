//! Mock device implementation for testing

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use super::{
    CheckResult, DeviceError, DeviceId, DeviceInterface, DeviceMetrics, DeviceType, EccErrors,
    XidError,
};

/// Mock device for testing
pub struct MockDevice {
    devices: Vec<DeviceId>,
    /// Configurable failure simulation
    pub fail_active_check: AtomicBool,
    /// Configurable PCIe test failure simulation
    pub fail_pcie_test: AtomicBool,
    /// Simulated XID errors
    xid_errors: RwLock<Vec<XidError>>,
    /// Simulated temperature
    pub temperature: AtomicU32,
    /// Simulated zombie PIDs
    zombie_pids: RwLock<Vec<u32>>,
}

impl MockDevice {
    /// Create a new mock device with default 2 GPUs
    pub fn new() -> Self {
        Self::with_device_count(2)
    }

    /// Create a mock device with specified number of GPUs
    pub fn with_device_count(count: u32) -> Self {
        let devices = (0..count)
            .map(|i| DeviceId {
                index: i,
                uuid: Some(format!("GPU-MOCK-{:04}", i)),
                name: format!("Mock GPU {}", i),
            })
            .collect();

        Self {
            devices,
            fail_active_check: AtomicBool::new(false),
            fail_pcie_test: AtomicBool::new(false),
            xid_errors: RwLock::new(Vec::new()),
            temperature: AtomicU32::new(45),
            zombie_pids: RwLock::new(Vec::new()),
        }
    }

    /// Set whether active check should fail
    pub fn set_fail_active_check(&self, fail: bool) {
        self.fail_active_check.store(fail, Ordering::SeqCst);
    }

    /// Set whether PCIe test should fail
    pub fn set_fail_pcie_test(&self, fail: bool) {
        self.fail_pcie_test.store(fail, Ordering::SeqCst);
    }

    /// Add a simulated XID error
    pub async fn add_xid_error(&self, code: u32, device_index: u32) {
        let mut errors = self.xid_errors.write().await;
        errors.push(XidError {
            code,
            message: format!("Mock XID error {}", code),
            timestamp: Utc::now(),
            device_index,
        });
    }

    /// Clear XID errors
    pub async fn clear_xid_errors(&self) {
        let mut errors = self.xid_errors.write().await;
        errors.clear();
    }

    /// Set simulated temperature
    pub fn set_temperature(&self, temp: u32) {
        self.temperature.store(temp, Ordering::SeqCst);
    }

    /// Add simulated zombie PID
    pub async fn add_zombie_pid(&self, pid: u32) {
        let mut pids = self.zombie_pids.write().await;
        pids.push(pid);
    }

    /// Clear zombie PIDs
    pub async fn clear_zombie_pids(&self) {
        let mut pids = self.zombie_pids.write().await;
        pids.clear();
    }
}

impl Default for MockDevice {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DeviceInterface for MockDevice {
    async fn list_devices(&self) -> Result<Vec<DeviceId>, DeviceError> {
        Ok(self.devices.clone())
    }

    async fn get_metrics(&self, device: &DeviceId) -> Result<DeviceMetrics, DeviceError> {
        if device.index >= self.devices.len() as u32 {
            return Err(DeviceError::DeviceNotFound(format!(
                "Device {} not found",
                device.index
            )));
        }

        Ok(DeviceMetrics {
            temperature: self.temperature.load(Ordering::SeqCst),
            gpu_utilization: 25,
            memory_utilization: 30,
            power_usage: 150,
            power_limit: 300,
            memory_total: 16 * 1024 * 1024 * 1024, // 16GB
            memory_used: 4 * 1024 * 1024 * 1024,   // 4GB
            memory_free: 12 * 1024 * 1024 * 1024,  // 12GB
            pcie_tx: Some(1000),
            pcie_rx: Some(1000),
            ecc_errors: EccErrors::default(),
            timestamp: Utc::now(),
        })
    }

    async fn get_xid_errors(&self, device: &DeviceId) -> Result<Vec<XidError>, DeviceError> {
        let errors = self.xid_errors.read().await;
        Ok(errors
            .iter()
            .filter(|e| e.device_index == device.index)
            .cloned()
            .collect())
    }

    async fn check_zombie_processes(&self, _device: &DeviceId) -> Result<Vec<u32>, DeviceError> {
        let pids = self.zombie_pids.read().await;
        Ok(pids.clone())
    }

    async fn run_active_check(
        &self,
        _device: &DeviceId,
        _timeout: Duration,
    ) -> Result<CheckResult, DeviceError> {
        // Simulate some processing time
        tokio::time::sleep(Duration::from_millis(10)).await;

        if self.fail_active_check.load(Ordering::SeqCst) {
            Ok(CheckResult::failure(
                Duration::from_millis(10),
                "Mock active check failure".to_string(),
                Some(1),
            ))
        } else {
            Ok(CheckResult::success(Duration::from_millis(10)))
        }
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Nvidia // Mock as NVIDIA for testing
    }

    fn supports_pcie_test(&self) -> bool {
        true
    }

    async fn run_pcie_test(&self, _device: &DeviceId) -> Result<CheckResult, DeviceError> {
        // Simulate PCIe test
        tokio::time::sleep(Duration::from_millis(100)).await;

        if self.fail_pcie_test.load(Ordering::SeqCst) {
            Ok(CheckResult::failure(
                Duration::from_millis(100),
                "PCIe bandwidth degradation detected: 4.5 GB/s (expected 12+ GB/s)".to_string(),
                Some(1),
            ))
        } else {
            Ok(CheckResult::success(Duration::from_millis(100)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_device_list() {
        let mock = MockDevice::with_device_count(4);
        let devices = mock.list_devices().await.unwrap();
        assert_eq!(devices.len(), 4);
    }

    #[tokio::test]
    async fn test_mock_device_metrics() {
        let mock = MockDevice::new();
        mock.set_temperature(75);
        let devices = mock.list_devices().await.unwrap();
        let metrics = mock.get_metrics(&devices[0]).await.unwrap();
        assert_eq!(metrics.temperature, 75);
    }

    #[tokio::test]
    async fn test_mock_xid_errors() {
        let mock = MockDevice::new();
        mock.add_xid_error(31, 0).await;
        mock.add_xid_error(43, 0).await;
        mock.add_xid_error(31, 1).await;

        let devices = mock.list_devices().await.unwrap();
        let errors = mock.get_xid_errors(&devices[0]).await.unwrap();
        assert_eq!(errors.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_active_check_pass() {
        let mock = MockDevice::new();
        let devices = mock.list_devices().await.unwrap();
        let result = mock
            .run_active_check(&devices[0], Duration::from_secs(5))
            .await
            .unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn test_mock_active_check_fail() {
        let mock = MockDevice::new();
        mock.set_fail_active_check(true);
        let devices = mock.list_devices().await.unwrap();
        let result = mock
            .run_active_check(&devices[0], Duration::from_secs(5))
            .await
            .unwrap();
        assert!(!result.passed);
    }
}
