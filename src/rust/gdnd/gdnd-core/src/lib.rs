//! GDND Core Library
//!
//! Core detection logic for GPU Dead Node Detector.
//! This crate provides device abstraction, health detection, and state management.

pub mod detection;
pub mod device;
pub mod healing;
pub mod metrics;
pub mod scheduler;
pub mod state_machine;

// Re-export common types
pub use device::{DeviceError, DeviceId, DeviceInterface, DeviceMetrics, DeviceType};
pub use healing::{HealingAction, HealingConfig, HealingStrategy, SelfHealer};
pub use state_machine::{GpuHealth, GpuHealthManager, HealthState, IsolationAction};
