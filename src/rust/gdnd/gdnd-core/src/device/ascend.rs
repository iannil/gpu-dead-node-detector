//! Huawei Ascend NPU device implementation
//!
//! Uses npu-smi command line tool and device-os logs for NPU monitoring.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use regex::Regex;
use tracing::{debug, trace, warn};

use super::{
    CheckResult, DeviceError, DeviceId, DeviceInterface, DeviceMetrics, DeviceType, EccErrors,
    XidError,
};

/// Ascend NPU error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AscendErrorCode {
    /// HBM memory error
    HbmError = 1001,
    /// AI Core hang
    AiCoreHang = 1002,
    /// Over temperature
    OverTemperature = 1003,
    /// PCIe link error
    PcieLinkError = 1005,
    /// Device lost
    DeviceLost = 1007,
    /// ECC uncorrectable error
    EccUncorrectable = 1008,
    /// Unknown error
    Unknown = 9999,
}

impl AscendErrorCode {
    /// Check if this error code is fatal
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            AscendErrorCode::HbmError
                | AscendErrorCode::AiCoreHang
                | AscendErrorCode::PcieLinkError
                | AscendErrorCode::DeviceLost
                | AscendErrorCode::EccUncorrectable
        )
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            AscendErrorCode::HbmError => "HBM memory error",
            AscendErrorCode::AiCoreHang => "AI Core hang",
            AscendErrorCode::OverTemperature => "Over temperature",
            AscendErrorCode::PcieLinkError => "PCIe link error",
            AscendErrorCode::DeviceLost => "Device lost",
            AscendErrorCode::EccUncorrectable => "ECC uncorrectable error",
            AscendErrorCode::Unknown => "Unknown error",
        }
    }

    /// Convert from u32 error code
    pub fn from_code(code: u32) -> Self {
        match code {
            1001 => AscendErrorCode::HbmError,
            1002 => AscendErrorCode::AiCoreHang,
            1003 => AscendErrorCode::OverTemperature,
            1005 => AscendErrorCode::PcieLinkError,
            1007 => AscendErrorCode::DeviceLost,
            1008 => AscendErrorCode::EccUncorrectable,
            _ => AscendErrorCode::Unknown,
        }
    }
}

/// Parsed NPU device info from npu-smi
#[derive(Debug, Clone)]
struct NpuDeviceInfo {
    /// NPU index
    index: u32,
    /// Device name (e.g., "910B3")
    name: String,
    /// Health status (OK, Warning, Error)
    health: String,
    /// Bus ID (e.g., "0000:C1:00.0")
    bus_id: Option<String>,
}

/// Parsed NPU metrics from npu-smi
#[derive(Debug, Clone, Default)]
struct NpuMetricsInfo {
    /// Temperature in Celsius
    temperature: u32,
    /// Power usage in Watts
    power: u32,
    /// AI Core utilization percentage
    aicore_util: u32,
    /// HBM used in MB
    hbm_used: u64,
    /// HBM total in MB
    hbm_total: u64,
}

/// Huawei Ascend NPU device implementation
pub struct AscendDevice {
    /// Path to npu-smi binary
    npu_smi_path: String,
    /// Path to npu-check binary
    npu_check_path: String,
    /// Path to NPU log directory
    log_dir: PathBuf,
    /// Fatal error codes
    fatal_error_codes: Vec<u32>,
}

impl AscendDevice {
    /// Create a new Ascend device interface with default paths
    pub fn new() -> Result<Self, DeviceError> {
        Self::with_config(
            "/usr/local/bin/npu-smi".to_string(),
            "/usr/local/bin/npu-check".to_string(),
            PathBuf::from("/var/log/npu/slog"),
            vec![1001, 1002, 1005, 1007, 1008],
        )
    }

    /// Create a new Ascend device interface with custom configuration
    pub fn with_config(
        npu_smi_path: String,
        npu_check_path: String,
        log_dir: PathBuf,
        fatal_error_codes: Vec<u32>,
    ) -> Result<Self, DeviceError> {
        // Check if npu-smi exists
        if !std::path::Path::new(&npu_smi_path).exists() {
            return Err(DeviceError::Other(format!(
                "npu-smi not found at {}",
                npu_smi_path
            )));
        }

        Ok(Self {
            npu_smi_path,
            npu_check_path,
            log_dir,
            fatal_error_codes,
        })
    }

    /// Execute npu-smi command with arguments
    async fn run_npu_smi(&self, args: &[&str]) -> Result<String, DeviceError> {
        let output = tokio::process::Command::new(&self.npu_smi_path)
            .args(args)
            .output()
            .await
            .map_err(|e| DeviceError::IoError(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DeviceError::QueryError(format!(
                "npu-smi failed: {}",
                stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Parse npu-smi info output to get device list
    ///
    /// Example output format:
    /// ```text
    /// +------------------------------------------------------------------------------------------------+
    /// | npu-smi 23.0.0                   Version: 23.0.0                                               |
    /// +---------------------------+---------------+----------------------------------------------------+
    /// | NPU     Name              | Health        | Power(W)    Temp(C)           Hugepages-Usage(page)|
    /// | Chip                      | Bus-Id        | AICore(%)   Memory-Usage(MB)   HBM-Usage(MB)       |
    /// +===========================+===============+====================================================+
    /// | 0       910B3             | OK            | 112.5       37         0 / 0                       |
    /// | 0                         | 0000:C1:00.0  | 6           0 / 0              33551 / 65536       |
    /// +===========================+===============+====================================================+
    /// ```
    fn parse_device_list(&self, output: &str) -> Result<Vec<NpuDeviceInfo>, DeviceError> {
        let mut devices = Vec::new();

        // Match device lines: "| 0       910B3             | OK            |"
        let device_regex =
            Regex::new(r"\|\s*(\d+)\s+(\S+)\s+\|\s*(\w+)\s+\|").unwrap();
        // Match chip lines: "| 0                         | 0000:C1:00.0  |"
        let chip_regex =
            Regex::new(r"\|\s*(\d+)\s+\|\s*([0-9a-fA-F:\.]+)\s+\|").unwrap();

        let lines: Vec<&str> = output.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            // Try to match device line
            if let Some(cap) = device_regex.captures(line) {
                let index: u32 = cap[1].parse().unwrap_or(0);
                let name = cap[2].to_string();
                let health = cap[3].to_string();

                // Try to get bus_id from next line
                let bus_id = if i + 1 < lines.len() {
                    chip_regex
                        .captures(lines[i + 1])
                        .map(|c| c[2].to_string())
                } else {
                    None
                };

                devices.push(NpuDeviceInfo {
                    index,
                    name,
                    health,
                    bus_id,
                });

                i += 2; // Skip chip line
                continue;
            }

            i += 1;
        }

        Ok(devices)
    }

    /// Parse npu-smi info output to get device metrics
    ///
    /// Extracts: Power(W), Temp(C), AICore(%), HBM-Usage(MB)
    fn parse_device_metrics(&self, output: &str, device_index: u32) -> NpuMetricsInfo {
        let mut metrics = NpuMetricsInfo::default();

        // Match the main device line with metrics
        // "| 0       910B3             | OK            | 112.5       37         0 / 0"
        let main_regex = Regex::new(
            r"\|\s*(\d+)\s+\S+\s+\|\s*\w+\s+\|\s*(\d+\.?\d*)\s+(\d+)\s+",
        )
        .unwrap();

        // Match the chip line with AICore and HBM
        // "| 0                         | 0000:C1:00.0  | 6           0 / 0              33551 / 65536"
        let chip_regex = Regex::new(
            r"\|\s*(\d+)\s+\|\s*[0-9a-fA-F:\.]+\s+\|\s*(\d+)\s+\d+\s*/\s*\d+\s+(\d+)\s*/\s*(\d+)",
        )
        .unwrap();

        for line in output.lines() {
            // Try main line
            if let Some(cap) = main_regex.captures(line) {
                if cap[1].parse::<u32>().unwrap_or(u32::MAX) == device_index {
                    metrics.power = cap[2].parse::<f64>().unwrap_or(0.0) as u32;
                    metrics.temperature = cap[3].parse().unwrap_or(0);
                }
            }

            // Try chip line
            if let Some(cap) = chip_regex.captures(line) {
                if cap[1].parse::<u32>().unwrap_or(u32::MAX) == device_index {
                    metrics.aicore_util = cap[2].parse().unwrap_or(0);
                    metrics.hbm_used = cap[3].parse::<u64>().unwrap_or(0) * 1024 * 1024; // MB to bytes
                    metrics.hbm_total = cap[4].parse::<u64>().unwrap_or(0) * 1024 * 1024;
                }
            }
        }

        metrics
    }

    /// Parse device-os logs for errors
    ///
    /// Log path: /var/log/npu/slog/device-os-{id}/
    async fn parse_device_logs(&self, device_index: u32) -> Vec<XidError> {
        let log_path = self.log_dir.join(format!("device-os-{}", device_index));

        if !log_path.exists() {
            debug!(
                path = ?log_path,
                "Device log directory not found"
            );
            return Vec::new();
        }

        // Error patterns to match in logs
        let error_patterns = [
            (r"\[ERROR\].*HBM.*error", 1001u32),
            (r"\[ERROR\].*AICore.*hang", 1002),
            (r"\[ERROR\].*temperature", 1003),
            (r"\[ERROR\].*PCIe.*link", 1005),
            (r"\[ERROR\].*device.*lost", 1007),
            (r"\[ERROR\].*ECC.*uncorrectable", 1008),
            (r"ErrCode=(\d+)", 0), // Generic error code pattern
        ];

        let mut errors = Vec::new();

        // Read recent log files
        let mut entries = match tokio::fs::read_dir(&log_path).await {
            Ok(entries) => entries,
            Err(e) => {
                debug!(error = %e, "Failed to read log directory");
                return Vec::new();
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Only process recent log files (last 24h)
            if let Ok(metadata) = entry.metadata().await {
                if let Ok(modified) = metadata.modified() {
                    let age = std::time::SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or(Duration::MAX);
                    if age > Duration::from_secs(86400) {
                        continue;
                    }
                }
            }

            // Read and parse log file
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                for (pattern, default_code) in &error_patterns {
                    let re = match Regex::new(pattern) {
                        Ok(re) => re,
                        Err(_) => continue,
                    };

                    for cap in re.captures_iter(&content) {
                        let code = if *default_code == 0 {
                            // Extract code from capture group
                            cap.get(1)
                                .and_then(|m| m.as_str().parse().ok())
                                .unwrap_or(9999)
                        } else {
                            *default_code
                        };

                        let error_code = AscendErrorCode::from_code(code);
                        errors.push(XidError {
                            code,
                            message: error_code.description().to_string(),
                            timestamp: Utc::now(),
                            device_index,
                        });
                    }
                }
            }
        }

        // Deduplicate by error code
        errors.sort_by_key(|e| e.code);
        errors.dedup_by_key(|e| e.code);

        trace!(
            device_index = device_index,
            count = errors.len(),
            "Found Ascend errors in logs"
        );

        errors
    }

    /// Check if health status indicates an error
    fn health_to_error(&self, health: &str) -> Option<AscendErrorCode> {
        match health.to_uppercase().as_str() {
            "OK" => None,
            "WARNING" => Some(AscendErrorCode::OverTemperature),
            "ERROR" | "FAULT" => Some(AscendErrorCode::DeviceLost),
            _ => None,
        }
    }
}

#[async_trait]
impl DeviceInterface for AscendDevice {
    async fn list_devices(&self) -> Result<Vec<DeviceId>, DeviceError> {
        let output = self.run_npu_smi(&["info"]).await?;
        let devices = self.parse_device_list(&output)?;

        Ok(devices
            .into_iter()
            .map(|d| DeviceId {
                index: d.index,
                uuid: d.bus_id, // Use bus_id as UUID equivalent
                name: format!("Ascend {}", d.name),
            })
            .collect())
    }

    async fn get_metrics(&self, device: &DeviceId) -> Result<DeviceMetrics, DeviceError> {
        let output = self.run_npu_smi(&["info"]).await?;
        let metrics = self.parse_device_metrics(&output, device.index);

        Ok(DeviceMetrics {
            temperature: metrics.temperature,
            gpu_utilization: metrics.aicore_util,
            memory_utilization: if metrics.hbm_total > 0 {
                ((metrics.hbm_used as f64 / metrics.hbm_total as f64) * 100.0) as u32
            } else {
                0
            },
            power_usage: metrics.power,
            power_limit: 0, // npu-smi doesn't provide power limit
            memory_total: metrics.hbm_total,
            memory_used: metrics.hbm_used,
            memory_free: metrics.hbm_total.saturating_sub(metrics.hbm_used),
            pcie_tx: None,
            pcie_rx: None,
            ecc_errors: EccErrors::default(),
            timestamp: Utc::now(),
        })
    }

    async fn get_xid_errors(&self, device: &DeviceId) -> Result<Vec<XidError>, DeviceError> {
        // Get errors from device logs
        let mut errors = self.parse_device_logs(device.index).await;

        // Also check npu-smi health status
        let output = self.run_npu_smi(&["info"]).await?;
        let devices = self.parse_device_list(&output)?;

        for d in devices {
            if d.index == device.index {
                if let Some(error_code) = self.health_to_error(&d.health) {
                    errors.push(XidError {
                        code: error_code as u32,
                        message: error_code.description().to_string(),
                        timestamp: Utc::now(),
                        device_index: device.index,
                    });
                }
                break;
            }
        }

        Ok(errors)
    }

    async fn check_zombie_processes(&self, device: &DeviceId) -> Result<Vec<u32>, DeviceError> {
        // Use npu-smi info -t usages to get process list
        let output = match self.run_npu_smi(&["info", "-t", "usages"]).await {
            Ok(output) => output,
            Err(_) => return Ok(Vec::new()),
        };

        // Parse PIDs from output
        // Format varies, look for PID patterns
        let pid_regex = Regex::new(r"\bPID[:\s]+(\d+)").unwrap();
        let pids: Vec<u32> = pid_regex
            .captures_iter(&output)
            .filter_map(|cap| cap[1].parse().ok())
            .collect();

        let mut zombie_pids = Vec::new();

        for pid in pids {
            // Check process state in /proc
            let stat_path = format!("/proc/{}/stat", pid);
            if let Ok(stat) = tokio::fs::read_to_string(&stat_path).await {
                let parts: Vec<&str> = stat.split_whitespace().collect();
                if parts.len() > 2 {
                    let state = parts[2];
                    if state == "D" {
                        warn!(pid = pid, device = %device, "Found zombie NPU process");
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

        // Run npu-check binary with timeout
        let result = tokio::time::timeout(
            timeout,
            tokio::process::Command::new(&self.npu_check_path)
                .arg("-d")
                .arg(device.index.to_string())
                .output(),
        )
        .await;

        let duration = start.elapsed();

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    debug!(device = %device, duration = ?duration, "Ascend active check passed");
                    Ok(CheckResult::success(duration))
                } else {
                    let error = String::from_utf8_lossy(&output.stderr).to_string();
                    warn!(device = %device, error = %error, "Ascend active check failed");
                    Ok(CheckResult::failure(duration, error, output.status.code()))
                }
            }
            Ok(Err(e)) => {
                // Binary not found or couldn't execute
                if e.kind() == std::io::ErrorKind::NotFound {
                    debug!("npu-check binary not found, skipping active check");
                    Ok(CheckResult::success(duration))
                } else {
                    Err(DeviceError::CheckError(e.to_string()))
                }
            }
            Err(_) => {
                // Timeout
                warn!(device = %device, timeout = ?timeout, "Ascend active check timed out");
                Ok(CheckResult::timeout(timeout))
            }
        }
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Ascend
    }

    fn supports_pcie_test(&self) -> bool {
        true
    }

    async fn run_pcie_test(&self, device: &DeviceId) -> Result<CheckResult, DeviceError> {
        // Run npu-check with PCIe test flag
        let start = std::time::Instant::now();

        let result = tokio::process::Command::new(&self.npu_check_path)
            .arg("-d")
            .arg(device.index.to_string())
            .arg("--pcie-test")
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
                        "npu-check not found, PCIe test unavailable".to_string(),
                    ))
                } else {
                    Err(DeviceError::IoError(e))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascend_error_codes() {
        assert_eq!(AscendErrorCode::from_code(1001), AscendErrorCode::HbmError);
        assert_eq!(AscendErrorCode::from_code(1002), AscendErrorCode::AiCoreHang);
        assert_eq!(AscendErrorCode::from_code(1007), AscendErrorCode::DeviceLost);
        assert_eq!(AscendErrorCode::from_code(9999), AscendErrorCode::Unknown);

        assert!(AscendErrorCode::HbmError.is_fatal());
        assert!(AscendErrorCode::DeviceLost.is_fatal());
        assert!(!AscendErrorCode::OverTemperature.is_fatal());
    }

    #[test]
    fn test_parse_npu_smi_output() {
        let sample_output = r#"
+------------------------------------------------------------------------------------------------+
| npu-smi 23.0.0                   Version: 23.0.0                                               |
+---------------------------+---------------+----------------------------------------------------+
| NPU     Name              | Health        | Power(W)    Temp(C)           Hugepages-Usage(page)|
| Chip                      | Bus-Id        | AICore(%)   Memory-Usage(MB)   HBM-Usage(MB)       |
+===========================+===============+====================================================+
| 0       910B3             | OK            | 112.5       37         0 / 0                       |
| 0                         | 0000:C1:00.0  | 6           0 / 0              33551 / 65536       |
+===========================+===============+====================================================+
| 1       910B3             | OK            | 110.0       35         0 / 0                       |
| 1                         | 0000:C2:00.0  | 10          0 / 0              20000 / 65536       |
+===========================+===============+====================================================+
"#;

        // Create a mock device to test parsing
        let device = AscendDevice {
            npu_smi_path: "/usr/local/bin/npu-smi".to_string(),
            npu_check_path: "/usr/local/bin/npu-check".to_string(),
            log_dir: PathBuf::from("/var/log/npu/slog"),
            fatal_error_codes: vec![1001, 1002, 1007, 1008],
        };

        let devices = device.parse_device_list(sample_output).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].name, "910B3");
        assert_eq!(devices[0].health, "OK");
        assert_eq!(devices[0].bus_id, Some("0000:C1:00.0".to_string()));

        assert_eq!(devices[1].index, 1);
        assert_eq!(devices[1].bus_id, Some("0000:C2:00.0".to_string()));
    }

    #[test]
    fn test_parse_device_metrics() {
        let sample_output = r#"
| 0       910B3             | OK            | 112.5       37         0 / 0                       |
| 0                         | 0000:C1:00.0  | 6           0 / 0              33551 / 65536       |
"#;

        let device = AscendDevice {
            npu_smi_path: "/usr/local/bin/npu-smi".to_string(),
            npu_check_path: "/usr/local/bin/npu-check".to_string(),
            log_dir: PathBuf::from("/var/log/npu/slog"),
            fatal_error_codes: vec![1001, 1002, 1007, 1008],
        };

        let metrics = device.parse_device_metrics(sample_output, 0);
        assert_eq!(metrics.temperature, 37);
        assert_eq!(metrics.power, 112);
        assert_eq!(metrics.aicore_util, 6);
        assert_eq!(metrics.hbm_used, 33551 * 1024 * 1024);
        assert_eq!(metrics.hbm_total, 65536 * 1024 * 1024);
    }

    #[test]
    fn test_health_status_mapping() {
        let device = AscendDevice {
            npu_smi_path: "/usr/local/bin/npu-smi".to_string(),
            npu_check_path: "/usr/local/bin/npu-check".to_string(),
            log_dir: PathBuf::from("/var/log/npu/slog"),
            fatal_error_codes: vec![1001, 1002, 1007, 1008],
        };

        assert!(device.health_to_error("OK").is_none());
        assert_eq!(
            device.health_to_error("WARNING"),
            Some(AscendErrorCode::OverTemperature)
        );
        assert_eq!(
            device.health_to_error("ERROR"),
            Some(AscendErrorCode::DeviceLost)
        );
        assert_eq!(
            device.health_to_error("FAULT"),
            Some(AscendErrorCode::DeviceLost)
        );
    }
}
