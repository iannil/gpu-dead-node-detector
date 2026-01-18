//! Device interface trait and common types
//!
//! Defines the core abstraction for GPU/NPU device operations.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Device type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    /// Auto-detect device type
    #[default]
    Auto,
    /// NVIDIA GPU
    Nvidia,
    /// Huawei Ascend NPU
    Ascend,
}

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceType::Auto => write!(f, "auto"),
            DeviceType::Nvidia => write!(f, "nvidia"),
            DeviceType::Ascend => write!(f, "ascend"),
        }
    }
}

/// Unique identifier for a device
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId {
    /// Device index (0-based)
    pub index: u32,
    /// Device UUID (if available)
    pub uuid: Option<String>,
    /// Device name/model
    pub name: String,
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GPU{}", self.index)
    }
}

/// Device metrics collected during passive detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetrics {
    /// GPU temperature in Celsius
    pub temperature: u32,
    /// GPU utilization percentage (0-100)
    pub gpu_utilization: u32,
    /// Memory utilization percentage (0-100)
    pub memory_utilization: u32,
    /// Power usage in Watts
    pub power_usage: u32,
    /// Power limit in Watts
    pub power_limit: u32,
    /// Total memory in bytes
    pub memory_total: u64,
    /// Used memory in bytes
    pub memory_used: u64,
    /// Free memory in bytes
    pub memory_free: u64,
    /// PCIe throughput TX in KB/s (if available)
    pub pcie_tx: Option<u32>,
    /// PCIe throughput RX in KB/s (if available)
    pub pcie_rx: Option<u32>,
    /// ECC error counts
    pub ecc_errors: EccErrors,
    /// Timestamp when metrics were collected
    pub timestamp: DateTime<Utc>,
}

/// ECC error counts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EccErrors {
    /// Single-bit correctable errors
    pub single_bit: u64,
    /// Double-bit uncorrectable errors
    pub double_bit: u64,
}

/// XID error from GPU
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XidError {
    /// XID error code
    pub code: u32,
    /// Error message
    pub message: String,
    /// Timestamp when error occurred
    pub timestamp: DateTime<Utc>,
    /// Device index that generated the error
    pub device_index: u32,
}

impl XidError {
    /// Check if this is a fatal XID error
    pub fn is_fatal(&self, fatal_xids: &[u32]) -> bool {
        fatal_xids.contains(&self.code)
    }
}

/// Result of an active check operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Whether the check passed
    pub passed: bool,
    /// Duration of the check
    pub duration: Duration,
    /// Error message if check failed
    pub error: Option<String>,
    /// Exit code from check binary (if applicable)
    pub exit_code: Option<i32>,
}

impl CheckResult {
    /// Create a successful check result
    pub fn success(duration: Duration) -> Self {
        Self {
            passed: true,
            duration,
            error: None,
            exit_code: Some(0),
        }
    }

    /// Create a failed check result
    pub fn failure(duration: Duration, error: String, exit_code: Option<i32>) -> Self {
        Self {
            passed: false,
            duration,
            error: Some(error),
            exit_code,
        }
    }

    /// Create a timeout check result
    pub fn timeout(timeout: Duration) -> Self {
        Self {
            passed: false,
            duration: timeout,
            error: Some("Check timed out".to_string()),
            exit_code: None,
        }
    }
}

/// Errors that can occur during device operations
#[derive(Debug, Error)]
pub enum DeviceError {
    /// NVML initialization failed
    #[error("Failed to initialize NVML: {0}")]
    NvmlInitError(String),

    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Failed to query device
    #[error("Failed to query device: {0}")]
    QueryError(String),

    /// Active check failed
    #[error("Active check failed: {0}")]
    CheckError(String),

    /// Operation timed out
    #[error("Operation timed out after {0:?}")]
    Timeout(Duration),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Other errors
    #[error("{0}")]
    Other(String),
}

/// Device interface trait
///
/// This trait provides a unified interface for different GPU/NPU devices.
/// Implementations should be thread-safe and async-compatible.
#[async_trait]
pub trait DeviceInterface: Send + Sync {
    /// List all available devices
    async fn list_devices(&self) -> Result<Vec<DeviceId>, DeviceError>;

    /// Get current metrics for a device
    async fn get_metrics(&self, device: &DeviceId) -> Result<DeviceMetrics, DeviceError>;

    /// Get recent XID errors for a device
    async fn get_xid_errors(&self, device: &DeviceId) -> Result<Vec<XidError>, DeviceError>;

    /// Check for zombie GPU processes (processes in D state)
    async fn check_zombie_processes(&self, device: &DeviceId) -> Result<Vec<u32>, DeviceError>;

    /// Run an active check on the device
    ///
    /// This typically involves running a small computation to verify
    /// the device is responsive and functioning correctly.
    async fn run_active_check(
        &self,
        device: &DeviceId,
        timeout: Duration,
    ) -> Result<CheckResult, DeviceError>;

    /// Get the device type
    fn device_type(&self) -> DeviceType;

    /// Check if PCIe bandwidth test is supported
    fn supports_pcie_test(&self) -> bool {
        false
    }

    /// Run PCIe bandwidth test (L3 detection)
    async fn run_pcie_test(&self, _device: &DeviceId) -> Result<CheckResult, DeviceError> {
        Err(DeviceError::Other("PCIe test not supported".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id_display() {
        let id = DeviceId {
            index: 0,
            uuid: Some("GPU-12345".to_string()),
            name: "Tesla V100".to_string(),
        };
        assert_eq!(format!("{}", id), "GPU0");
    }

    #[test]
    fn test_xid_is_fatal() {
        let fatal_xids = vec![31, 43, 48, 79];
        let xid = XidError {
            code: 31,
            message: "MMU Fault".to_string(),
            timestamp: Utc::now(),
            device_index: 0,
        };
        assert!(xid.is_fatal(&fatal_xids));

        let non_fatal = XidError {
            code: 13,
            message: "Some warning".to_string(),
            timestamp: Utc::now(),
            device_index: 0,
        };
        assert!(!non_fatal.is_fatal(&fatal_xids));
    }

    #[test]
    fn test_check_result() {
        let success = CheckResult::success(Duration::from_millis(100));
        assert!(success.passed);
        assert!(success.error.is_none());

        let failure = CheckResult::failure(
            Duration::from_millis(50),
            "GPU hung".to_string(),
            Some(1),
        );
        assert!(!failure.passed);
        assert!(failure.error.is_some());

        let timeout = CheckResult::timeout(Duration::from_secs(5));
        assert!(!timeout.passed);
        assert!(timeout.error.as_ref().unwrap().contains("timed out"));
    }
}
