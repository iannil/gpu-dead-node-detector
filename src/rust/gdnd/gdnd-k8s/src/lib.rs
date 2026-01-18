//! GDND Kubernetes Integration
//!
//! Provides Kubernetes client and node operations for GPU Dead Node Detector.

pub mod client;
pub mod node_ops;

pub use client::K8sClient;
pub use node_ops::{IsolationConfig, NodeOperator};
