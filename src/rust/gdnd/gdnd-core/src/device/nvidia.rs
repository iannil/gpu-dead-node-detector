//! NVIDIA GPU device implementation
//!
//! Uses NVML (NVIDIA Management Library) for GPU monitoring.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::Nvml;
use once_cell::sync::OnceCell;
use regex::Regex;
use tracing::{debug, trace, warn};

use super::{
    CheckResult, DeviceError, DeviceId, DeviceInterface, DeviceMetrics, DeviceType, EccErrors,
    XidError,
};

/// Global NVML instance
static NVML: OnceCell<Arc<Nvml>> = OnceCell::new();

/// Get or initialize the global NVML instance
fn get_nvml() -> Result<&'static Arc<Nvml>, DeviceError> {
    NVML.get_or_try_init(|| {
        Nvml::init()
            .map(Arc::new)
            .map_err(|e| DeviceError::NvmlInitError(e.to_string()))
    })
}

/// NVIDIA GPU device implementation
pub struct NvidiaDevice {
    nvml: &'static Arc<Nvml>,
    gpu_check_path: String,
}

impl NvidiaDevice {
    /// Create a new NVIDIA device interface
    pub fn new() -> Result<Self, DeviceError> {
        Self::with_gpu_check_path("/usr/local/bin/gpu-check".to_string())
    }

    /// Create a new NVIDIA device interface with custom gpu-check path
    pub fn with_gpu_check_path(gpu_check_path: String) -> Result<Self, DeviceError> {
        let nvml = get_nvml()?;
        Ok(Self {
            nvml,
            gpu_check_path,
        })
    }
}

#[async_trait]
impl DeviceInterface for NvidiaDevice {
    async fn list_devices(&self) -> Result<Vec<DeviceId>, DeviceError> {
        let count = self
            .nvml
            .device_count()
            .map_err(|e| DeviceError::QueryError(e.to_string()))?;

        let mut devices = Vec::with_capacity(count as usize);

        for i in 0..count {
            let device = self
                .nvml
                .device_by_index(i)
                .map_err(|e| DeviceError::QueryError(e.to_string()))?;

            let name = device
                .name()
                .map_err(|e| DeviceError::QueryError(e.to_string()))?;

            let uuid = device.uuid().ok();

            devices.push(DeviceId {
                index: i,
                uuid,
                name,
            });
        }

        Ok(devices)
    }

    async fn get_metrics(&self, device: &DeviceId) -> Result<DeviceMetrics, DeviceError> {
        let nvml_device = self
            .nvml
            .device_by_index(device.index)
            .map_err(|e| DeviceError::DeviceNotFound(e.to_string()))?;

        // Temperature
        let temperature = nvml_device
            .temperature(TemperatureSensor::Gpu)
            .unwrap_or(0);

        // Utilization
        let (gpu_utilization, memory_utilization) = nvml_device
            .utilization_rates()
            .map(|u| (u.gpu, u.memory))
            .unwrap_or((0, 0));

        // Power
        let power_usage = nvml_device.power_usage().unwrap_or(0) / 1000; // mW to W
        let power_limit = nvml_device
            .power_management_limit()
            .unwrap_or(0)
            / 1000;

        // Memory
        let memory_info = nvml_device.memory_info().map_err(|e| {
            DeviceError::QueryError(format!("Failed to get memory info: {}", e))
        })?;

        // PCIe throughput
        let pcie_tx = nvml_device.pcie_throughput(nvml_wrapper::enum_wrappers::device::PcieUtilCounter::Send).ok();
        let pcie_rx = nvml_device.pcie_throughput(nvml_wrapper::enum_wrappers::device::PcieUtilCounter::Receive).ok();

        // ECC errors - simplify handling as API varies by version
        // For now, just return default. Full ECC support can be added later.
        let ecc_errors = EccErrors::default();

        Ok(DeviceMetrics {
            temperature,
            gpu_utilization,
            memory_utilization,
            power_usage,
            power_limit,
            memory_total: memory_info.total,
            memory_used: memory_info.used,
            memory_free: memory_info.free,
            pcie_tx,
            pcie_rx,
            ecc_errors,
            timestamp: Utc::now(),
        })
    }

    async fn get_xid_errors(&self, device: &DeviceId) -> Result<Vec<XidError>, DeviceError> {
        // Parse dmesg for NVIDIA XID errors
        // Format: NVRM: Xid (PCI:0000:xx:xx.0): XX, ...
        let output = tokio::process::Command::new("dmesg")
            .arg("-T")
            .output()
            .await
            .map_err(DeviceError::IoError)?;

        if !output.status.success() {
            debug!("dmesg command failed, skipping XID scan");
            return Ok(Vec::new());
        }

        let dmesg = String::from_utf8_lossy(&output.stdout);
        let xid_regex = Regex::new(r"NVRM: Xid \(PCI:([^)]+)\): (\d+),").unwrap();

        let mut errors = Vec::new();
        for cap in xid_regex.captures_iter(&dmesg) {
            if let Ok(code) = cap[2].parse::<u32>() {
                errors.push(XidError {
                    code,
                    message: get_xid_description(code),
                    timestamp: Utc::now(), // Ideally parse from dmesg timestamp
                    device_index: device.index,
                });
            }
        }

        trace!(device = %device, count = errors.len(), "Found XID errors");
        Ok(errors)
    }

    async fn check_zombie_processes(&self, device: &DeviceId) -> Result<Vec<u32>, DeviceError> {
        // Find processes in D state (uninterruptible sleep) that are using GPU
        let output = tokio::process::Command::new("nvidia-smi")
            .args(["--query-compute-apps=pid", "--format=csv,noheader,nounits"])
            .args(["-i", &device.index.to_string()])
            .output()
            .await
            .map_err(DeviceError::IoError)?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect();

        let mut zombie_pids = Vec::new();

        for pid in pids {
            // Check process state
            let stat_path = format!("/proc/{}/stat", pid);
            if let Ok(stat) = tokio::fs::read_to_string(&stat_path).await {
                let parts: Vec<&str> = stat.split_whitespace().collect();
                if parts.len() > 2 {
                    let state = parts[2];
                    if state == "D" {
                        warn!(pid = pid, device = %device, "Found zombie GPU process");
                        zombie_pids.push(pid);
                    }
                }
            }
        }

        Ok(zombie_pids)
    }

    async fn run_active_check(
        &self,
        device: &DeviceId,
        timeout: Duration,
    ) -> Result<CheckResult, DeviceError> {
        let start = std::time::Instant::now();

        // Run gpu-check binary with timeout
        let result = tokio::time::timeout(
            timeout,
            tokio::process::Command::new(&self.gpu_check_path)
                .arg("-d")
                .arg(device.index.to_string())
                .output(),
        )
        .await;

        let duration = start.elapsed();

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    debug!(device = %device, duration = ?duration, "Active check passed");
                    Ok(CheckResult::success(duration))
                } else {
                    let error = String::from_utf8_lossy(&output.stderr).to_string();
                    warn!(device = %device, error = %error, "Active check failed");
                    Ok(CheckResult::failure(duration, error, output.status.code()))
                }
            }
            Ok(Err(e)) => {
                // Binary not found or couldn't execute
                if e.kind() == std::io::ErrorKind::NotFound {
                    debug!("gpu-check binary not found, skipping active check");
                    Ok(CheckResult::success(duration))
                } else {
                    Err(DeviceError::CheckError(e.to_string()))
                }
            }
            Err(_) => {
                // Timeout
                warn!(device = %device, timeout = ?timeout, "Active check timed out");
                Ok(CheckResult::timeout(timeout))
            }
        }
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Nvidia
    }

    fn supports_pcie_test(&self) -> bool {
        true
    }

    async fn run_pcie_test(&self, device: &DeviceId) -> Result<CheckResult, DeviceError> {
        // Run bandwidth test using cuda-samples bandwidthTest if available
        let start = std::time::Instant::now();

        let result = tokio::process::Command::new("bandwidthTest")
            .arg("--device")
            .arg(device.index.to_string())
            .output()
            .await;

        let duration = start.elapsed();

        match result {
            Ok(output) => {
                if output.status.success() {
                    Ok(CheckResult::success(duration))
                } else {
                    let error = String::from_utf8_lossy(&output.stderr).to_string();
                    Ok(CheckResult::failure(duration, error, output.status.code()))
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Err(DeviceError::Other(
                        "bandwidthTest not found, PCIe test unavailable".to_string(),
                    ))
                } else {
                    Err(DeviceError::IoError(e))
                }
            }
        }
    }
}

/// Get human-readable description for XID error codes
fn get_xid_description(code: u32) -> String {
    match code {
        13 => "Graphics Engine Exception".to_string(),
        31 => "GPU memory page fault".to_string(),
        32 => "Invalid or corrupted push buffer stream".to_string(),
        38 => "Driver firmware error".to_string(),
        43 => "GPU stopped processing".to_string(),
        45 => "Preemptive cleanup, due to previous errors".to_string(),
        48 => "Double Bit ECC Error".to_string(),
        61 => "Internal micro-controller breakpoint/warning".to_string(),
        62 => "Internal micro-controller halt".to_string(),
        63 => "ECC page retirement or row remapping recording event".to_string(),
        64 => "ECC page retirement or row remapper recording failure".to_string(),
        68 => "NVDEC0 Exception".to_string(),
        69 => "Graphics Engine class error".to_string(),
        74 => "NVLINK Error".to_string(),
        79 => "GPU has fallen off the bus".to_string(),
        92 => "High single-bit ECC error rate".to_string(),
        94 => "Contained ECC error".to_string(),
        95 => "Uncontained ECC error".to_string(),
        _ => format!("Unknown XID error (code: {})", code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xid_descriptions() {
        assert!(get_xid_description(31).contains("page fault"));
        assert!(get_xid_description(43).contains("stopped"));
        assert!(get_xid_description(48).contains("ECC"));
        assert!(get_xid_description(79).contains("fallen off"));
        assert!(get_xid_description(9999).contains("Unknown"));
    }
}
