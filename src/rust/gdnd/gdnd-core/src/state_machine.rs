//! GPU Health State Machine
//!
//! Manages the health state of each GPU device:
//! HEALTHY → SUSPECTED → UNHEALTHY → ISOLATED
//!
//! State transitions:
//! - HEALTHY → SUSPECTED: Single check failure
//! - SUSPECTED → UNHEALTHY: failure_threshold consecutive failures OR fatal XID
//! - SUSPECTED → HEALTHY: Check passes
//! - UNHEALTHY → ISOLATED: Isolation actions completed

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::detection::{DetectionResult, Finding};
use crate::device::DeviceId;

/// Health states for a GPU
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthState {
    /// GPU is healthy and functioning normally
    Healthy,
    /// GPU has shown issues but not yet confirmed unhealthy
    Suspected,
    /// GPU is confirmed unhealthy and needs isolation
    Unhealthy,
    /// GPU has been isolated (cordoned, tainted)
    Isolated,
}

impl std::fmt::Display for HealthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthState::Healthy => write!(f, "HEALTHY"),
            HealthState::Suspected => write!(f, "SUSPECTED"),
            HealthState::Unhealthy => write!(f, "UNHEALTHY"),
            HealthState::Isolated => write!(f, "ISOLATED"),
        }
    }
}

/// Actions to take for isolation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IsolationAction {
    /// Cordon the node (prevent new pods from scheduling)
    Cordon,
    /// Add taint to the node
    Taint {
        key: String,
        value: String,
        effect: String,
    },
    /// Evict pods from the node
    EvictPods,
    /// Send alert
    Alert { message: String, severity: String },
}

/// Health record for a single GPU
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuHealth {
    /// Device ID
    pub device: DeviceId,
    /// Current health state
    pub state: HealthState,
    /// Number of consecutive failures
    pub failure_count: u32,
    /// Last check timestamp
    pub last_check: DateTime<Utc>,
    /// State change timestamp
    pub state_changed_at: DateTime<Utc>,
    /// Last findings that caused state change
    pub last_findings: Vec<Finding>,
}

impl GpuHealth {
    /// Create a new healthy GPU record
    pub fn new(device: DeviceId) -> Self {
        let now = Utc::now();
        Self {
            device,
            state: HealthState::Healthy,
            failure_count: 0,
            last_check: now,
            state_changed_at: now,
            last_findings: Vec::new(),
        }
    }
}

/// Event for state transition
#[derive(Debug, Clone)]
pub enum HealthEvent {
    /// Check passed
    CheckPassed,
    /// Check failed (non-fatal)
    CheckFailed { findings: Vec<Finding> },
    /// Fatal error detected
    FatalError { findings: Vec<Finding> },
    /// Isolation completed
    IsolationCompleted,
}

/// State transition result
#[derive(Debug, Clone)]
pub struct StateTransition {
    /// Previous state
    pub from: HealthState,
    /// New state
    pub to: HealthState,
    /// Actions to perform (if any)
    pub actions: Vec<IsolationAction>,
    /// Whether state actually changed
    pub changed: bool,
}

impl StateTransition {
    fn no_change(state: HealthState) -> Self {
        Self {
            from: state,
            to: state,
            actions: Vec::new(),
            changed: false,
        }
    }

    fn transition(from: HealthState, to: HealthState, actions: Vec<IsolationAction>) -> Self {
        Self {
            from,
            to,
            actions,
            changed: from != to,
        }
    }
}

/// GPU Health Manager
///
/// Tracks health state for all GPUs and handles state transitions.
pub struct GpuHealthManager {
    /// Health records per device
    health: HashMap<String, GpuHealth>,
    /// Failure threshold for SUSPECTED → UNHEALTHY transition
    failure_threshold: u32,
    /// Fatal XID codes (unused but kept for reference)
    #[allow(dead_code)]
    fatal_xids: Vec<u32>,
}

impl GpuHealthManager {
    /// Create a new health manager
    pub fn new(failure_threshold: u32, fatal_xids: Vec<u32>) -> Self {
        Self {
            health: HashMap::new(),
            failure_threshold,
            fatal_xids,
        }
    }

    /// Get or create health record for a device
    fn get_or_create_mut(&mut self, device: &DeviceId) -> &mut GpuHealth {
        let key = device_key(device);
        self.health
            .entry(key)
            .or_insert_with(|| GpuHealth::new(device.clone()))
    }

    /// Get health record for a device
    pub fn get(&self, device: &DeviceId) -> Option<&GpuHealth> {
        self.health.get(&device_key(device))
    }

    /// Get all health records
    pub fn all(&self) -> impl Iterator<Item = &GpuHealth> {
        self.health.values()
    }

    /// Process detection result and update state
    pub fn process_result(&mut self, result: &DetectionResult) -> StateTransition {
        let event = if result.passed {
            HealthEvent::CheckPassed
        } else if result.has_fatal_finding() {
            HealthEvent::FatalError {
                findings: result.findings.clone(),
            }
        } else {
            HealthEvent::CheckFailed {
                findings: result.findings.clone(),
            }
        };

        self.transition(&result.device, event)
    }

    /// Generate isolation actions for unhealthy GPU
    fn isolation_actions(device: &DeviceId, findings: &[Finding]) -> Vec<IsolationAction> {
        let mut actions = Vec::new();

        // Cordon node
        actions.push(IsolationAction::Cordon);

        // Add taint
        actions.push(IsolationAction::Taint {
            key: "nvidia.com/gpu-health".to_string(),
            value: "failed".to_string(),
            effect: "NoSchedule".to_string(),
        });

        // Alert
        let finding_msgs: Vec<String> = findings.iter().map(|f| f.message.clone()).collect();
        actions.push(IsolationAction::Alert {
            message: format!(
                "GPU {} marked unhealthy: {}",
                device,
                finding_msgs.join(", ")
            ),
            severity: "critical".to_string(),
        });

        actions
    }

    /// Perform state transition
    pub fn transition(&mut self, device: &DeviceId, event: HealthEvent) -> StateTransition {
        // Copy threshold before mutable borrow
        let failure_threshold = self.failure_threshold;

        let health = self.get_or_create_mut(device);
        let old_state = health.state;
        health.last_check = Utc::now();

        match (&health.state, event) {
            // HEALTHY state transitions
            (HealthState::Healthy, HealthEvent::CheckPassed) => {
                health.failure_count = 0;
                StateTransition::no_change(HealthState::Healthy)
            }
            (HealthState::Healthy, HealthEvent::CheckFailed { findings }) => {
                health.failure_count = 1;
                health.state = HealthState::Suspected;
                health.state_changed_at = Utc::now();
                health.last_findings = findings;
                info!(
                    device = %device,
                    from = %old_state,
                    to = %health.state,
                    "GPU state changed"
                );
                StateTransition::transition(old_state, HealthState::Suspected, Vec::new())
            }
            (HealthState::Healthy, HealthEvent::FatalError { findings }) => {
                // Fatal error: skip SUSPECTED, go straight to UNHEALTHY
                health.failure_count = failure_threshold;
                health.state = HealthState::Unhealthy;
                health.state_changed_at = Utc::now();
                health.last_findings = findings.clone();
                warn!(
                    device = %device,
                    from = %old_state,
                    to = %health.state,
                    "Fatal error detected, GPU marked unhealthy"
                );
                StateTransition::transition(
                    old_state,
                    HealthState::Unhealthy,
                    Self::isolation_actions(device, &findings),
                )
            }

            // SUSPECTED state transitions
            (HealthState::Suspected, HealthEvent::CheckPassed) => {
                health.failure_count = 0;
                health.state = HealthState::Healthy;
                health.state_changed_at = Utc::now();
                health.last_findings.clear();
                info!(
                    device = %device,
                    from = %old_state,
                    to = %health.state,
                    "GPU recovered"
                );
                StateTransition::transition(old_state, HealthState::Healthy, Vec::new())
            }
            (HealthState::Suspected, HealthEvent::CheckFailed { findings }) => {
                health.failure_count += 1;
                let failure_count = health.failure_count;
                health.last_findings = findings.clone();

                if failure_count >= failure_threshold {
                    health.state = HealthState::Unhealthy;
                    health.state_changed_at = Utc::now();
                    warn!(
                        device = %device,
                        from = %old_state,
                        to = %health.state,
                        failures = failure_count,
                        "GPU marked unhealthy after consecutive failures"
                    );
                    StateTransition::transition(
                        old_state,
                        HealthState::Unhealthy,
                        Self::isolation_actions(device, &findings),
                    )
                } else {
                    debug!(
                        device = %device,
                        failures = failure_count,
                        threshold = failure_threshold,
                        "GPU check failed, still suspected"
                    );
                    StateTransition::no_change(HealthState::Suspected)
                }
            }
            (HealthState::Suspected, HealthEvent::FatalError { findings }) => {
                health.failure_count = failure_threshold;
                health.state = HealthState::Unhealthy;
                health.state_changed_at = Utc::now();
                health.last_findings = findings.clone();
                warn!(
                    device = %device,
                    from = %old_state,
                    to = %health.state,
                    "Fatal error detected, GPU marked unhealthy"
                );
                StateTransition::transition(
                    old_state,
                    HealthState::Unhealthy,
                    Self::isolation_actions(device, &findings),
                )
            }

            // UNHEALTHY state transitions
            (HealthState::Unhealthy, HealthEvent::IsolationCompleted) => {
                health.state = HealthState::Isolated;
                health.state_changed_at = Utc::now();
                info!(
                    device = %device,
                    from = %old_state,
                    to = %health.state,
                    "GPU isolation completed"
                );
                StateTransition::transition(old_state, HealthState::Isolated, Vec::new())
            }
            (HealthState::Unhealthy, _) => {
                // Already unhealthy, no change
                StateTransition::no_change(HealthState::Unhealthy)
            }

            // ISOLATED state - no transitions (requires manual intervention)
            (HealthState::Isolated, _) => {
                StateTransition::no_change(HealthState::Isolated)
            }

            // IsolationCompleted for non-UNHEALTHY states - should not happen, but handle gracefully
            (_, HealthEvent::IsolationCompleted) => {
                StateTransition::no_change(health.state)
            }
        }
    }

    /// Check if any GPU is unhealthy
    pub fn has_unhealthy(&self) -> bool {
        self.health
            .values()
            .any(|h| h.state == HealthState::Unhealthy)
    }

    /// Get all unhealthy GPUs
    pub fn unhealthy_gpus(&self) -> Vec<&GpuHealth> {
        self.health
            .values()
            .filter(|h| h.state == HealthState::Unhealthy)
            .collect()
    }
}

/// Generate unique key for device
fn device_key(device: &DeviceId) -> String {
    device
        .uuid
        .clone()
        .unwrap_or_else(|| format!("gpu-{}", device.index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_device(index: u32) -> DeviceId {
        DeviceId {
            index,
            uuid: Some(format!("GPU-TEST-{}", index)),
            name: "Test GPU".to_string(),
        }
    }

    #[test]
    fn test_healthy_to_suspected() {
        let mut manager = GpuHealthManager::new(3, vec![31, 43, 48, 79]);
        let device = test_device(0);

        let findings = vec![Finding::high_temperature(90, 85)];
        let transition = manager.transition(
            &device,
            HealthEvent::CheckFailed {
                findings: findings.clone(),
            },
        );

        assert!(transition.changed);
        assert_eq!(transition.from, HealthState::Healthy);
        assert_eq!(transition.to, HealthState::Suspected);
        assert!(transition.actions.is_empty());

        let health = manager.get(&device).unwrap();
        assert_eq!(health.state, HealthState::Suspected);
        assert_eq!(health.failure_count, 1);
    }

    #[test]
    fn test_suspected_to_healthy() {
        let mut manager = GpuHealthManager::new(3, vec![31, 43, 48, 79]);
        let device = test_device(0);

        // First failure
        manager.transition(
            &device,
            HealthEvent::CheckFailed {
                findings: vec![Finding::high_temperature(90, 85)],
            },
        );

        // Recovery
        let transition = manager.transition(&device, HealthEvent::CheckPassed);

        assert!(transition.changed);
        assert_eq!(transition.from, HealthState::Suspected);
        assert_eq!(transition.to, HealthState::Healthy);

        let health = manager.get(&device).unwrap();
        assert_eq!(health.state, HealthState::Healthy);
        assert_eq!(health.failure_count, 0);
    }

    #[test]
    fn test_suspected_to_unhealthy_threshold() {
        let mut manager = GpuHealthManager::new(3, vec![31, 43, 48, 79]);
        let device = test_device(0);
        let findings = vec![Finding::high_temperature(90, 85)];

        // First two failures - still suspected
        for _ in 0..2 {
            manager.transition(
                &device,
                HealthEvent::CheckFailed {
                    findings: findings.clone(),
                },
            );
            assert!(
                manager.get(&device).unwrap().state == HealthState::Healthy
                    || manager.get(&device).unwrap().state == HealthState::Suspected
            );
        }

        // Third failure - should become unhealthy
        let transition = manager.transition(
            &device,
            HealthEvent::CheckFailed {
                findings: findings.clone(),
            },
        );

        assert!(transition.changed);
        assert_eq!(transition.to, HealthState::Unhealthy);
        assert!(!transition.actions.is_empty());

        let health = manager.get(&device).unwrap();
        assert_eq!(health.state, HealthState::Unhealthy);
    }

    #[test]
    fn test_fatal_error_immediate_unhealthy() {
        let mut manager = GpuHealthManager::new(3, vec![31, 43, 48, 79]);
        let device = test_device(0);

        let findings = vec![Finding::fatal_xid(31, "GPU memory page fault")];
        let transition = manager.transition(
            &device,
            HealthEvent::FatalError {
                findings: findings.clone(),
            },
        );

        assert!(transition.changed);
        assert_eq!(transition.from, HealthState::Healthy);
        assert_eq!(transition.to, HealthState::Unhealthy);
        assert!(!transition.actions.is_empty());
    }

    #[test]
    fn test_unhealthy_to_isolated() {
        let mut manager = GpuHealthManager::new(3, vec![31, 43, 48, 79]);
        let device = test_device(0);

        // Make unhealthy
        manager.transition(
            &device,
            HealthEvent::FatalError {
                findings: vec![Finding::fatal_xid(79, "GPU fallen off the bus")],
            },
        );

        // Complete isolation
        let transition = manager.transition(&device, HealthEvent::IsolationCompleted);

        assert!(transition.changed);
        assert_eq!(transition.from, HealthState::Unhealthy);
        assert_eq!(transition.to, HealthState::Isolated);
    }

    #[test]
    fn test_isolated_no_transition() {
        let mut manager = GpuHealthManager::new(3, vec![31, 43, 48, 79]);
        let device = test_device(0);

        // Make isolated
        manager.transition(
            &device,
            HealthEvent::FatalError {
                findings: vec![Finding::fatal_xid(79, "GPU fallen off the bus")],
            },
        );
        manager.transition(&device, HealthEvent::IsolationCompleted);

        // Try various events - should all stay isolated
        let transition = manager.transition(&device, HealthEvent::CheckPassed);
        assert!(!transition.changed);
        assert_eq!(transition.to, HealthState::Isolated);

        let transition = manager.transition(
            &device,
            HealthEvent::CheckFailed {
                findings: vec![Finding::high_temperature(90, 85)],
            },
        );
        assert!(!transition.changed);
        assert_eq!(transition.to, HealthState::Isolated);
    }
}
