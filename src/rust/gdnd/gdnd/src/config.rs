//! Configuration module for GDND
//!
//! Handles loading and validating configuration from YAML files and environment variables.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Device type for detection
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

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    /// Number of consecutive failures before marking as UNHEALTHY
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,

    /// Fatal XID error codes that trigger immediate isolation
    #[serde(default = "default_fatal_xids")]
    pub fatal_xids: Vec<u32>,

    /// Temperature threshold in Celsius
    #[serde(default = "default_temperature_threshold")]
    pub temperature_threshold: u32,

    /// Timeout for active check operations
    #[serde(with = "humantime_serde", default = "default_active_check_timeout")]
    pub active_check_timeout: Duration,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            failure_threshold: default_failure_threshold(),
            fatal_xids: default_fatal_xids(),
            temperature_threshold: default_temperature_threshold(),
            active_check_timeout: default_active_check_timeout(),
        }
    }
}

/// Isolation action configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationConfig {
    /// Whether to cordon the node
    #[serde(default = "default_true")]
    pub cordon: bool,

    /// Whether to evict pods from the node
    #[serde(default)]
    pub evict_pods: bool,

    /// Taint key to apply
    #[serde(default = "default_taint_key")]
    pub taint_key: String,

    /// Taint value to apply
    #[serde(default = "default_taint_value")]
    pub taint_value: String,

    /// Taint effect (NoSchedule, PreferNoSchedule, NoExecute)
    #[serde(default = "default_taint_effect")]
    pub taint_effect: String,
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            cordon: true,
            evict_pods: false,
            taint_key: default_taint_key(),
            taint_value: default_taint_value(),
            taint_effect: default_taint_effect(),
        }
    }
}

/// Metrics export configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Whether metrics are enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Port to expose metrics on
    #[serde(default = "default_metrics_port")]
    pub port: u16,

    /// Path for metrics endpoint
    #[serde(default = "default_metrics_path")]
    pub path: String,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: default_metrics_port(),
            path: default_metrics_path(),
        }
    }
}

/// Healing strategy for self-healing operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
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

/// Self-healing configuration
/// WARNING: Healing operations may interrupt running GPU workloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealingConfig {
    /// Enable self-healing (disabled by default for safety)
    #[serde(default)]
    pub enabled: bool,

    /// Healing strategy to use
    #[serde(default)]
    pub strategy: HealingStrategy,

    /// Run healing operations in dry-run mode (log but don't execute)
    #[serde(default)]
    pub dry_run: bool,

    /// Timeout for healing operations
    #[serde(with = "humantime_serde", default = "default_healing_timeout")]
    pub timeout: Duration,
}

impl Default for HealingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: HealingStrategy::default(),
            dry_run: false,
            timeout: default_healing_timeout(),
        }
    }
}

/// Recovery detection configuration
/// Allow isolated GPUs to return to healthy state after recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Enable recovery detection (disabled by default)
    #[serde(default)]
    pub enabled: bool,

    /// Number of consecutive healthy checks before recovery
    #[serde(default = "default_recovery_threshold")]
    pub threshold: u32,

    /// Interval between recovery checks
    #[serde(with = "humantime_serde", default = "default_recovery_interval")]
    pub interval: Duration,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: default_recovery_threshold(),
            interval: default_recovery_interval(),
        }
    }
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Device type to detect
    #[serde(default)]
    pub device_type: DeviceType,

    /// Node name (from K8s downward API)
    #[serde(default)]
    pub node_name: Option<String>,

    /// L1 passive detection interval
    #[serde(with = "humantime_serde", default = "default_l1_interval")]
    pub l1_interval: Duration,

    /// L2 active detection interval
    #[serde(with = "humantime_serde", default = "default_l2_interval")]
    pub l2_interval: Duration,

    /// L3 PCIe detection interval (optional, only if enabled)
    #[serde(with = "humantime_serde", default = "default_l3_interval")]
    pub l3_interval: Duration,

    /// Whether L3 detection is enabled
    #[serde(default)]
    pub l3_enabled: bool,

    /// Path to gpu-check binary
    #[serde(default = "default_gpu_check_path")]
    pub gpu_check_path: String,

    /// Health check configuration
    #[serde(default)]
    pub health: HealthConfig,

    /// Isolation configuration
    #[serde(default)]
    pub isolation: IsolationConfig,

    /// Metrics configuration
    #[serde(default)]
    pub metrics: MetricsConfig,

    /// Self-healing configuration
    #[serde(default)]
    pub healing: HealingConfig,

    /// Recovery detection configuration
    #[serde(default)]
    pub recovery: RecoveryConfig,

    /// Dry run mode - log actions but don't execute
    #[serde(default)]
    pub dry_run: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device_type: DeviceType::default(),
            node_name: None,
            l1_interval: default_l1_interval(),
            l2_interval: default_l2_interval(),
            l3_interval: default_l3_interval(),
            l3_enabled: false,
            gpu_check_path: default_gpu_check_path(),
            health: HealthConfig::default(),
            isolation: IsolationConfig::default(),
            metrics: MetricsConfig::default(),
            healing: HealingConfig::default(),
            recovery: RecoveryConfig::default(),
            dry_run: false,
        }
    }
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;
        Self::from_yaml(&content)
    }

    /// Parse configuration from YAML string
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml).context("Failed to parse YAML configuration")
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.health.failure_threshold == 0 {
            anyhow::bail!("failure_threshold must be > 0");
        }
        if self.health.temperature_threshold == 0 || self.health.temperature_threshold > 150 {
            anyhow::bail!("temperature_threshold must be between 1 and 150");
        }
        if self.l1_interval.is_zero() {
            anyhow::bail!("l1_interval must be > 0");
        }
        if self.l2_interval.is_zero() {
            anyhow::bail!("l2_interval must be > 0");
        }
        if self.metrics.enabled && self.metrics.port == 0 {
            anyhow::bail!("metrics.port must be > 0 when metrics are enabled");
        }
        Ok(())
    }

    /// Override node_name from environment if not set
    pub fn with_node_name_from_env(mut self) -> Self {
        if self.node_name.is_none() {
            self.node_name = std::env::var("NODE_NAME").ok();
        }
        self
    }
}

// Default value functions
fn default_failure_threshold() -> u32 {
    3
}

fn default_fatal_xids() -> Vec<u32> {
    vec![31, 43, 48, 79]
}

fn default_temperature_threshold() -> u32 {
    85
}

fn default_active_check_timeout() -> Duration {
    Duration::from_secs(5)
}

fn default_l1_interval() -> Duration {
    Duration::from_secs(30)
}

fn default_l2_interval() -> Duration {
    Duration::from_secs(300)
}

fn default_l3_interval() -> Duration {
    Duration::from_secs(86400) // 24 hours
}

fn default_gpu_check_path() -> String {
    "/usr/local/bin/gpu-check".to_string()
}

fn default_taint_key() -> String {
    "nvidia.com/gpu-health".to_string()
}

fn default_taint_value() -> String {
    "failed".to_string()
}

fn default_taint_effect() -> String {
    "NoSchedule".to_string()
}

fn default_metrics_port() -> u16 {
    9100
}

fn default_metrics_path() -> String {
    "/metrics".to_string()
}

fn default_true() -> bool {
    true
}

fn default_healing_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_recovery_threshold() -> u32 {
    5
}

fn default_recovery_interval() -> Duration {
    Duration::from_secs(300) // 5 minutes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
device_type: nvidia
l1_interval: 30s
l2_interval: 5m

health:
  failure_threshold: 3
  fatal_xids: [31, 43, 48, 79]
  temperature_threshold: 85
  active_check_timeout: 5s

isolation:
  cordon: true
  evict_pods: false
  taint_key: nvidia.com/gpu-health
  taint_value: failed
  taint_effect: NoSchedule

metrics:
  enabled: true
  port: 9100
"#;
        let config = Config::from_yaml(yaml).unwrap();
        assert_eq!(config.device_type, DeviceType::Nvidia);
        assert_eq!(config.l1_interval, Duration::from_secs(30));
        assert_eq!(config.l2_interval, Duration::from_secs(300));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_fatal_xids() {
        let config = Config::default();
        assert!(config.health.fatal_xids.contains(&31));
        assert!(config.health.fatal_xids.contains(&43));
        assert!(config.health.fatal_xids.contains(&48));
        assert!(config.health.fatal_xids.contains(&79));
    }
}
