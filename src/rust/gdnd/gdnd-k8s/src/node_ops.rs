//! Node Operations
//!
//! Implements node isolation actions: Cordon, Taint, Pod Eviction

use anyhow::Result;
use tracing::{info, warn};

use super::client::K8sClient;
use gdnd_core::state_machine::IsolationAction;

/// Isolation configuration
#[derive(Debug, Clone)]
pub struct IsolationConfig {
    /// Whether to cordon the node
    pub cordon: bool,
    /// Whether to evict pods
    pub evict_pods: bool,
    /// Taint key
    pub taint_key: String,
    /// Taint value
    pub taint_value: String,
    /// Taint effect
    pub taint_effect: String,
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            cordon: true,
            evict_pods: false,
            taint_key: "nvidia.com/gpu-health".to_string(),
            taint_value: "failed".to_string(),
            taint_effect: "NoSchedule".to_string(),
        }
    }
}

/// Node operator for isolation actions
pub struct NodeOperator {
    client: K8sClient,
    node_name: String,
    config: IsolationConfig,
    dry_run: bool,
}

impl NodeOperator {
    /// Create a new node operator
    pub fn new(
        client: K8sClient,
        node_name: String,
        config: IsolationConfig,
        dry_run: bool,
    ) -> Self {
        Self {
            client,
            node_name,
            config,
            dry_run,
        }
    }

    /// Execute isolation actions
    pub async fn execute_actions(&self, actions: &[IsolationAction]) -> Result<()> {
        for action in actions {
            self.execute_action(action).await?;
        }
        Ok(())
    }

    /// Execute a single isolation action
    pub async fn execute_action(&self, action: &IsolationAction) -> Result<()> {
        match action {
            IsolationAction::Cordon => {
                if self.config.cordon {
                    self.cordon().await?;
                }
            }
            IsolationAction::Taint { key, value, effect } => {
                self.add_taint(key, value, effect).await?;
            }
            IsolationAction::EvictPods => {
                if self.config.evict_pods {
                    self.evict_pods().await?;
                }
            }
            IsolationAction::Alert { message, severity } => {
                self.send_alert(message, severity).await?;
            }
            IsolationAction::Uncordon => {
                self.uncordon().await?;
            }
            IsolationAction::RemoveTaint { key } => {
                self.remove_taint(key).await?;
            }
        }
        Ok(())
    }

    /// Cordon the node
    pub async fn cordon(&self) -> Result<()> {
        if self.dry_run {
            info!(node = %self.node_name, "[DRY-RUN] Would cordon node");
            return Ok(());
        }

        self.client.cordon_node(&self.node_name).await
    }

    /// Uncordon the node
    pub async fn uncordon(&self) -> Result<()> {
        if self.dry_run {
            info!(node = %self.node_name, "[DRY-RUN] Would uncordon node");
            return Ok(());
        }

        self.client.uncordon_node(&self.node_name).await
    }

    /// Add taint to the node
    pub async fn add_taint(&self, key: &str, value: &str, effect: &str) -> Result<()> {
        if self.dry_run {
            info!(
                node = %self.node_name,
                key = key,
                value = value,
                effect = effect,
                "[DRY-RUN] Would add taint"
            );
            return Ok(());
        }

        self.client
            .add_taint(&self.node_name, key, value, effect)
            .await
    }

    /// Remove taint from the node
    pub async fn remove_taint(&self, key: &str) -> Result<()> {
        if self.dry_run {
            info!(
                node = %self.node_name,
                key = key,
                "[DRY-RUN] Would remove taint"
            );
            return Ok(());
        }

        self.client.remove_taint(&self.node_name, key).await
    }

    /// Evict all pods from the node
    pub async fn evict_pods(&self) -> Result<()> {
        let pods = self.client.list_pods_on_node(&self.node_name).await?;

        for pod in pods {
            let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");
            let name = pod.metadata.name.as_deref().unwrap_or("unknown");

            // Skip daemonset pods, mirror pods, and system pods
            if self.should_skip_pod(&pod) {
                continue;
            }

            if self.dry_run {
                info!(
                    namespace = namespace,
                    pod = name,
                    "[DRY-RUN] Would evict pod"
                );
            } else {
                match self.client.evict_pod(namespace, name).await {
                    Ok(_) => info!(namespace = namespace, pod = name, "Pod evicted"),
                    Err(e) => warn!(
                        namespace = namespace,
                        pod = name,
                        error = %e,
                        "Failed to evict pod"
                    ),
                }
            }
        }

        Ok(())
    }

    /// Check if a pod should be skipped during eviction
    fn should_skip_pod(&self, pod: &k8s_openapi::api::core::v1::Pod) -> bool {
        let metadata = &pod.metadata;

        // Skip mirror pods (created by kubelet for static pods)
        if let Some(annotations) = &metadata.annotations {
            if annotations.contains_key("kubernetes.io/config.mirror") {
                return true;
            }
        }

        // Skip DaemonSet pods
        if let Some(owner_refs) = &metadata.owner_references {
            for owner in owner_refs {
                if owner.kind == "DaemonSet" {
                    return true;
                }
            }
        }

        // Skip kube-system namespace critical pods
        if metadata.namespace.as_deref() == Some("kube-system") {
            // Skip known critical system pods
            if let Some(name) = &metadata.name {
                if name.starts_with("kube-proxy")
                    || name.starts_with("kube-flannel")
                    || name.starts_with("calico-node")
                {
                    return true;
                }
            }
        }

        false
    }

    /// Send an alert (logging for now, can be extended)
    pub async fn send_alert(&self, message: &str, severity: &str) -> Result<()> {
        // For now, just log the alert
        // In production, this could send to PagerDuty, Slack, etc.
        match severity {
            "critical" => {
                tracing::error!(
                    node = %self.node_name,
                    severity = severity,
                    message = message,
                    "ALERT"
                );
            }
            "warning" => {
                tracing::warn!(
                    node = %self.node_name,
                    severity = severity,
                    message = message,
                    "ALERT"
                );
            }
            _ => {
                tracing::info!(
                    node = %self.node_name,
                    severity = severity,
                    message = message,
                    "ALERT"
                );
            }
        }

        Ok(())
    }

    /// Perform full isolation (cordon + taint)
    pub async fn isolate(&self) -> Result<()> {
        if self.config.cordon {
            self.cordon().await?;
        }

        self.add_taint(
            &self.config.taint_key,
            &self.config.taint_value,
            &self.config.taint_effect,
        )
        .await?;

        if self.config.evict_pods {
            self.evict_pods().await?;
        }

        Ok(())
    }

    /// Remove isolation (uncordon + remove taint)
    pub async fn unisolate(&self) -> Result<()> {
        self.remove_taint(&self.config.taint_key).await?;
        self.uncordon().await?;
        Ok(())
    }
}
