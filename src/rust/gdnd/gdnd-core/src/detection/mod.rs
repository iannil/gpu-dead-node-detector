//! Detection modules
//!
//! Implements the three-tier detection pipeline:
//! - L1: Passive detection (NVML queries, XID scans)
//! - L2: Active micro-detection (CUDA matrix multiply)
//! - L3: PCIe bandwidth testing (optional)

mod l1_passive;
mod l2_active;
mod l3_pcie;

pub use l1_passive::L1PassiveDetector;
pub use l2_active::L2ActiveDetector;
pub use l3_pcie::{L3PcieConfig, L3PcieDetector};

use serde::{Deserialize, Serialize};

use crate::device::DeviceId;

/// Result from a detection check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    /// Device that was checked
    pub device: DeviceId,
    /// Detection level (L1, L2, L3)
    pub level: DetectionLevel,
    /// Whether the check passed
    pub passed: bool,
    /// Detailed findings
    pub findings: Vec<Finding>,
}

impl DetectionResult {
    /// Create a passing result
    pub fn pass(device: DeviceId, level: DetectionLevel) -> Self {
        Self {
            device,
            level,
            passed: true,
            findings: Vec::new(),
        }
    }

    /// Create a failing result with findings
    pub fn fail(device: DeviceId, level: DetectionLevel, findings: Vec<Finding>) -> Self {
        Self {
            device,
            level,
            passed: false,
            findings,
        }
    }

    /// Check if any finding is fatal
    pub fn has_fatal_finding(&self) -> bool {
        self.findings.iter().any(|f| f.is_fatal)
    }
}

/// Detection level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionLevel {
    /// L1: Passive detection
    L1Passive,
    /// L2: Active micro-detection
    L2Active,
    /// L3: PCIe bandwidth test
    L3Pcie,
}

impl std::fmt::Display for DetectionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetectionLevel::L1Passive => write!(f, "L1"),
            DetectionLevel::L2Active => write!(f, "L2"),
            DetectionLevel::L3Pcie => write!(f, "L3"),
        }
    }
}

/// A finding from detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Type of finding
    pub finding_type: FindingType,
    /// Human-readable message
    pub message: String,
    /// Whether this finding is fatal (requires immediate isolation)
    pub is_fatal: bool,
}

impl Finding {
    /// Create a new finding
    pub fn new(finding_type: FindingType, message: String, is_fatal: bool) -> Self {
        Self {
            finding_type,
            message,
            is_fatal,
        }
    }

    /// Create a fatal XID error finding
    pub fn fatal_xid(code: u32, message: &str) -> Self {
        Self {
            finding_type: FindingType::FatalXid(code),
            message: message.to_string(),
            is_fatal: true,
        }
    }

    /// Create a high temperature finding
    pub fn high_temperature(temp: u32, threshold: u32) -> Self {
        Self {
            finding_type: FindingType::HighTemperature,
            message: format!("Temperature {}C exceeds threshold {}C", temp, threshold),
            is_fatal: false,
        }
    }

    /// Create a zombie process finding
    pub fn zombie_process(pid: u32) -> Self {
        Self {
            finding_type: FindingType::ZombieProcess,
            message: format!("Zombie GPU process detected: PID {}", pid),
            is_fatal: false,
        }
    }

    /// Create an active check failure finding
    pub fn active_check_failure(error: &str) -> Self {
        Self {
            finding_type: FindingType::ActiveCheckFailure,
            message: error.to_string(),
            is_fatal: false,
        }
    }

    /// Create a double-bit ECC error finding
    pub fn double_bit_ecc(count: u64) -> Self {
        Self {
            finding_type: FindingType::DoubleBitEcc,
            message: format!("Double-bit ECC errors detected: {}", count),
            is_fatal: true,
        }
    }
}

/// Types of findings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FindingType {
    /// Fatal XID error
    FatalXid(u32),
    /// Non-fatal XID error
    NonFatalXid(u32),
    /// High temperature
    HighTemperature,
    /// Zombie process
    ZombieProcess,
    /// Active check failed
    ActiveCheckFailure,
    /// Active check timeout
    ActiveCheckTimeout,
    /// Double-bit ECC error
    DoubleBitEcc,
    /// PCIe degradation
    PcieDegradation,
}
