//! Self-Healing Module
//!
//! Provides automated recovery actions for GPU/NPU issues.
//! This module is optional and disabled by default due to the
//! potentially disruptive nature of healing operations.
//!
//! WARNING: Healing operations may interrupt running workloads.
//! Use with caution in production environments.

use std::process::Command;
use std::time::Duration;

use thiserror::Error;
use tracing::{info, warn};

use crate::device::{DeviceId, DeviceType};

/// Healing strategy determines how aggressive the recovery attempts are
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealingStrategy {
    /// Conservative: Only kill zombie processes, no hardware resets
    #[default]
    Conservative,
    /// Moderate: Kill zombies + attempt GPU soft reset
    Moderate,
    /// Aggressive: All recovery options including driver reload
    /// WARNING: This will interrupt ALL GPU workloads on the node
    Aggressive,
}

/// Healing action types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealingAction {
    /// Kill a specific process
    KillProcess { pid: u32 },
    /// Kill all zombie GPU processes
    KillZombieProcesses,
    /// Soft reset a specific GPU (nvidia-smi -r)
    GpuSoftReset { device_index: u32 },
    /// Reload GPU driver (rmmod + modprobe)
    /// WARNING: This affects ALL GPUs on the node
    DriverReload,
}

/// Result of a healing attempt
#[derive(Debug, Clone)]
pub struct HealingResult {
    /// The action that was attempted
    pub action: HealingAction,
    /// Whether the action succeeded
    pub success: bool,
    /// Optional message with details
    pub message: Option<String>,
}

impl HealingResult {
    /// Create a successful result
    pub fn success(action: HealingAction) -> Self {
        Self {
            action,
            success: true,
            message: None,
        }
    }

    /// Create a successful result with message
    pub fn success_with_message(action: HealingAction, message: String) -> Self {
        Self {
            action,
            success: true,
            message: Some(message),
        }
    }

    /// Create a failed result
    pub fn failure(action: HealingAction, message: String) -> Self {
        Self {
            action,
            success: false,
            message: Some(message),
        }
    }
}

/// Errors that can occur during healing
#[derive(Debug, Error)]
pub enum HealingError {
    /// Healing is disabled
    #[error("Healing is disabled")]
    Disabled,

    /// Device type not supported for this healing action
    #[error("Device type {0} does not support this healing action")]
    UnsupportedDevice(DeviceType),

    /// Command execution failed
    #[error("Command execution failed: {0}")]
    CommandError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Self-healing configuration
#[derive(Debug, Clone)]
pub struct HealingConfig {
    /// Whether healing is enabled
    pub enabled: bool,
    /// Healing strategy to use
    pub strategy: HealingStrategy,
    /// Timeout for healing operations
    pub timeout: Duration,
    /// Whether to run in dry-run mode (log but don't execute)
    pub dry_run: bool,
}

impl Default for HealingConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default for safety
            strategy: HealingStrategy::Conservative,
            timeout: Duration::from_secs(30),
            dry_run: false,
        }
    }
}

/// Self-healer for GPU/NPU recovery
pub struct SelfHealer {
    config: HealingConfig,
    device_type: DeviceType,
}

impl SelfHealer {
    /// Create a new self-healer with the given configuration
    pub fn new(config: HealingConfig, device_type: DeviceType) -> Self {
        Self {
            config,
            device_type,
        }
    }

    /// Check if healing is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the current healing strategy
    pub fn strategy(&self) -> HealingStrategy {
        self.config.strategy
    }

    /// Get available healing actions for the current strategy and device type
    pub fn available_actions(&self) -> Vec<HealingAction> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut actions = Vec::new();

        // Conservative: Only zombie process cleanup
        actions.push(HealingAction::KillZombieProcesses);

        if self.config.strategy == HealingStrategy::Conservative {
            return actions;
        }

        // Moderate: Add GPU soft reset (NVIDIA only)
        if self.device_type == DeviceType::Nvidia {
            actions.push(HealingAction::GpuSoftReset { device_index: 0 });
        }

        if self.config.strategy == HealingStrategy::Moderate {
            return actions;
        }

        // Aggressive: Add driver reload
        if self.device_type == DeviceType::Nvidia {
            actions.push(HealingAction::DriverReload);
        }

        actions
    }

    /// Attempt to heal a device
    pub fn heal(&self, device: &DeviceId) -> Result<Vec<HealingResult>, HealingError> {
        if !self.config.enabled {
            return Err(HealingError::Disabled);
        }

        let mut results = Vec::new();

        // Step 1: Kill zombie processes (all strategies)
        let zombie_result = self.kill_zombie_processes(device)?;
        results.push(zombie_result);

        // Step 2: GPU soft reset (moderate and aggressive)
        if self.config.strategy != HealingStrategy::Conservative
            && self.device_type == DeviceType::Nvidia
        {
            let reset_result = self.gpu_soft_reset(device)?;
            results.push(reset_result);
        }

        // Step 3: Driver reload (aggressive only)
        if self.config.strategy == HealingStrategy::Aggressive
            && self.device_type == DeviceType::Nvidia
        {
            let reload_result = self.driver_reload()?;
            results.push(reload_result);
        }

        Ok(results)
    }

    /// Kill zombie GPU processes
    pub fn kill_zombie_processes(&self, device: &DeviceId) -> Result<HealingResult, HealingError> {
        let action = HealingAction::KillZombieProcesses;

        if self.config.dry_run {
            info!(device = %device, "[DRY-RUN] Would kill zombie GPU processes");
            return Ok(HealingResult::success_with_message(
                action,
                "Dry run - no processes killed".to_string(),
            ));
        }

        info!(device = %device, "Attempting to kill zombie GPU processes");

        // Find GPU processes in D (uninterruptible sleep) state
        let output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E 'D.*nvidia|D.*cuda|D.*gpu' | grep -v grep | awk '{print $2}'")
            .output()?;

        if !output.status.success() {
            return Ok(HealingResult::failure(
                action,
                "Failed to list zombie processes".to_string(),
            ));
        }

        let pids: Vec<u32> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect();

        if pids.is_empty() {
            return Ok(HealingResult::success_with_message(
                action,
                "No zombie processes found".to_string(),
            ));
        }

        let mut killed = 0;
        for pid in &pids {
            // Send SIGKILL to zombie processes
            let kill_result = Command::new("kill").arg("-9").arg(pid.to_string()).output();

            match kill_result {
                Ok(output) if output.status.success() => {
                    info!(pid = pid, "Killed zombie process");
                    killed += 1;
                }
                Ok(_) => {
                    warn!(pid = pid, "Failed to kill process");
                }
                Err(e) => {
                    warn!(pid = pid, error = %e, "Error killing process");
                }
            }
        }

        Ok(HealingResult::success_with_message(
            action,
            format!("Killed {}/{} zombie processes", killed, pids.len()),
        ))
    }

    /// Perform GPU soft reset using nvidia-smi
    pub fn gpu_soft_reset(&self, device: &DeviceId) -> Result<HealingResult, HealingError> {
        let action = HealingAction::GpuSoftReset {
            device_index: device.index,
        };

        if self.device_type != DeviceType::Nvidia {
            return Err(HealingError::UnsupportedDevice(self.device_type));
        }

        if self.config.dry_run {
            info!(device = %device, "[DRY-RUN] Would perform GPU soft reset");
            return Ok(HealingResult::success_with_message(
                action,
                "Dry run - no reset performed".to_string(),
            ));
        }

        warn!(device = %device, "Performing GPU soft reset - this may interrupt workloads");

        let output = Command::new("nvidia-smi")
            .arg("-i")
            .arg(device.index.to_string())
            .arg("-r")
            .output()?;

        if output.status.success() {
            info!(device = %device, "GPU soft reset completed");
            Ok(HealingResult::success_with_message(
                action,
                "GPU reset successful".to_string(),
            ))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(device = %device, error = %stderr, "GPU soft reset failed");
            Ok(HealingResult::failure(
                action,
                format!("GPU reset failed: {}", stderr),
            ))
        }
    }

    /// Reload NVIDIA driver (DANGEROUS - affects all GPUs)
    pub fn driver_reload(&self) -> Result<HealingResult, HealingError> {
        let action = HealingAction::DriverReload;

        if self.device_type != DeviceType::Nvidia {
            return Err(HealingError::UnsupportedDevice(self.device_type));
        }

        if self.config.dry_run {
            info!("[DRY-RUN] Would reload NVIDIA driver");
            return Ok(HealingResult::success_with_message(
                action,
                "Dry run - no driver reload performed".to_string(),
            ));
        }

        warn!("Reloading NVIDIA driver - THIS WILL INTERRUPT ALL GPU WORKLOADS");

        // First, try to unload the nvidia modules
        let unload = Command::new("sh")
            .arg("-c")
            .arg("modprobe -r nvidia_uvm nvidia_drm nvidia_modeset nvidia 2>/dev/null || true")
            .output()?;

        if !unload.status.success() {
            // This is expected if GPUs are in use
            warn!("Could not unload nvidia modules - GPUs may be in use");
        }

        // Reload the nvidia module
        let reload = Command::new("modprobe").arg("nvidia").output()?;

        if reload.status.success() {
            info!("NVIDIA driver reloaded successfully");
            Ok(HealingResult::success_with_message(
                action,
                "Driver reload successful".to_string(),
            ))
        } else {
            let stderr = String::from_utf8_lossy(&reload.stderr);
            warn!(error = %stderr, "Driver reload failed");
            Ok(HealingResult::failure(
                action,
                format!("Driver reload failed: {}", stderr),
            ))
        }
    }

    /// Kill a specific process by PID
    pub fn kill_process(&self, pid: u32) -> Result<HealingResult, HealingError> {
        let action = HealingAction::KillProcess { pid };

        if self.config.dry_run {
            info!(pid = pid, "[DRY-RUN] Would kill process");
            return Ok(HealingResult::success_with_message(
                action,
                "Dry run - process not killed".to_string(),
            ));
        }

        info!(pid = pid, "Killing process");

        let output = Command::new("kill").arg("-9").arg(pid.to_string()).output()?;

        if output.status.success() {
            info!(pid = pid, "Process killed successfully");
            Ok(HealingResult::success(action))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(pid = pid, error = %stderr, "Failed to kill process");
            Ok(HealingResult::failure(
                action,
                format!("Failed to kill process: {}", stderr),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healing_disabled_by_default() {
        let config = HealingConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.strategy, HealingStrategy::Conservative);
    }

    #[test]
    fn test_healer_disabled() {
        let config = HealingConfig::default();
        let healer = SelfHealer::new(config, DeviceType::Nvidia);

        assert!(!healer.is_enabled());
        assert!(healer.available_actions().is_empty());
    }

    #[test]
    fn test_healer_conservative_actions() {
        let config = HealingConfig {
            enabled: true,
            strategy: HealingStrategy::Conservative,
            ..Default::default()
        };
        let healer = SelfHealer::new(config, DeviceType::Nvidia);

        let actions = healer.available_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], HealingAction::KillZombieProcesses));
    }

    #[test]
    fn test_healer_moderate_actions() {
        let config = HealingConfig {
            enabled: true,
            strategy: HealingStrategy::Moderate,
            ..Default::default()
        };
        let healer = SelfHealer::new(config, DeviceType::Nvidia);

        let actions = healer.available_actions();
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], HealingAction::KillZombieProcesses));
        assert!(matches!(
            actions[1],
            HealingAction::GpuSoftReset { device_index: 0 }
        ));
    }

    #[test]
    fn test_healer_aggressive_actions() {
        let config = HealingConfig {
            enabled: true,
            strategy: HealingStrategy::Aggressive,
            ..Default::default()
        };
        let healer = SelfHealer::new(config, DeviceType::Nvidia);

        let actions = healer.available_actions();
        assert_eq!(actions.len(), 3);
        assert!(matches!(actions[2], HealingAction::DriverReload));
    }

    #[test]
    fn test_healer_dry_run() {
        let config = HealingConfig {
            enabled: true,
            strategy: HealingStrategy::Conservative,
            dry_run: true,
            ..Default::default()
        };
        let healer = SelfHealer::new(config, DeviceType::Nvidia);

        let device = DeviceId {
            index: 0,
            uuid: Some("GPU-TEST".to_string()),
            name: "Test GPU".to_string(),
        };

        // Dry run should succeed without actually doing anything
        let result = healer.kill_zombie_processes(&device).unwrap();
        assert!(result.success);
        assert!(result.message.unwrap().contains("Dry run"));
    }

    #[test]
    fn test_unsupported_device() {
        let config = HealingConfig {
            enabled: true,
            strategy: HealingStrategy::Moderate,
            ..Default::default()
        };
        let healer = SelfHealer::new(config, DeviceType::Ascend);

        let device = DeviceId {
            index: 0,
            uuid: None,
            name: "Ascend 910".to_string(),
        };

        // GPU soft reset should fail for Ascend devices
        let result = healer.gpu_soft_reset(&device);
        assert!(matches!(result, Err(HealingError::UnsupportedDevice(_))));
    }
}
