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

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Pod;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

    /// Create a test pod for testing should_skip_pod logic
    fn create_test_pod(
        name: &str,
        namespace: Option<&str>,
        annotations: Option<Vec<(String, String)>>,
        owner_refs: Option<Vec<OwnerReference>>,
    ) -> Pod {
        let metadata = ObjectMeta {
            name: Some(name.to_string()),
            namespace: namespace.map(|s| s.to_string()),
            annotations: annotations.map(|a| a.into_iter().collect()),
            owner_references: owner_refs,
            ..Default::default()
        };
        Pod {
            metadata,
            ..Default::default()
        }
    }

    /// Helper to create a NodeOperator for testing
    /// Note: This requires a valid K8s client for struct initialization,
    /// but most tests only use the should_skip_pod method
    #[allow(dead_code)]
    fn create_test_operator() -> NodeOperator {
        // We need a way to create a K8sClient without connecting
        // For now, we'll skip tests that require full client initialization
        // and focus on testing the logic that doesn't need a client
        panic!("Create test operator requires K8s cluster connection");
    }

    #[test]
    fn test_isolation_config_default() {
        let config = IsolationConfig::default();
        assert!(config.cordon, "Cordon should be enabled by default");
        assert!(
            !config.evict_pods,
            "Evict pods should be disabled by default"
        );
        assert_eq!(
            config.taint_key, "nvidia.com/gpu-health",
            "Taint key should have default value"
        );
        assert_eq!(
            config.taint_value, "failed",
            "Taint value should be 'failed'"
        );
        assert_eq!(
            config.taint_effect, "NoSchedule",
            "Taint effect should be 'NoSchedule'"
        );
    }

    #[test]
    fn test_isolation_config_custom() {
        let config = IsolationConfig {
            cordon: false,
            evict_pods: true,
            taint_key: "custom-key".to_string(),
            taint_value: "custom-value".to_string(),
            taint_effect: "NoExecute".to_string(),
        };
        assert!(!config.cordon);
        assert!(config.evict_pods);
        assert_eq!(config.taint_key, "custom-key");
        assert_eq!(config.taint_value, "custom-value");
        assert_eq!(config.taint_effect, "NoExecute");
    }

    // Test the should_skip_pod logic by creating a minimal test helper
    // This doesn't require a K8s connection
    fn test_should_skip_pod_logic(pod: &Pod) -> bool {
        let metadata = &pod.metadata;

        // Skip mirror pods
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

    #[test]
    fn test_should_skip_mirror_pod() {
        let pod = create_test_pod(
            "test-pod",
            Some("default"),
            Some(vec![(
                "kubernetes.io/config.mirror".to_string(),
                "true".to_string(),
            )]),
            None,
        );

        assert!(
            test_should_skip_pod_logic(&pod),
            "Mirror pods should be skipped"
        );
    }

    #[test]
    fn test_should_skip_daemonset_pod() {
        let owner_ref = OwnerReference {
            api_version: "apps/v1".to_string(),
            kind: "DaemonSet".to_string(),
            name: "test-daemonset".to_string(),
            uid: "123".to_string(),
            ..Default::default()
        };

        let pod = create_test_pod("test-pod", Some("default"), None, Some(vec![owner_ref]));

        assert!(
            test_should_skip_pod_logic(&pod),
            "DaemonSet pods should be skipped"
        );
    }

    #[test]
    fn test_should_skip_kube_system_pods() {
        let test_cases = vec![
            ("kube-proxy-xxx", true),
            ("kube-flannel-xxx", true),
            ("calico-node-xxx", true),
            ("coredns-xxx", false), // Not in skip list
        ];

        for (name, should_skip) in test_cases {
            let pod = create_test_pod(name, Some("kube-system"), None, None);
            assert_eq!(
                test_should_skip_pod_logic(&pod),
                should_skip,
                "Pod {} should skip: {}",
                name,
                should_skip
            );
        }
    }

    #[test]
    fn test_should_not_skip_regular_pod() {
        let owner_ref = OwnerReference {
            api_version: "apps/v1".to_string(),
            kind: "ReplicaSet".to_string(),
            name: "test-rs".to_string(),
            uid: "123".to_string(),
            ..Default::default()
        };

        let pod = create_test_pod("regular-pod", Some("default"), None, Some(vec![owner_ref]));

        assert!(
            !test_should_skip_pod_logic(&pod),
            "Regular pods should not be skipped"
        );
    }

    #[test]
    fn test_should_not_skip_deployment_pod() {
        let owner_ref = OwnerReference {
            api_version: "apps/v1".to_string(),
            kind: "ReplicaSet".to_string(),
            name: "deployment-rs".to_string(),
            uid: "456".to_string(),
            ..Default::default()
        };

        let pod = create_test_pod("app-pod-123", Some("default"), None, Some(vec![owner_ref]));

        assert!(
            !test_should_skip_pod_logic(&pod),
            "Deployment pods should not be skipped"
        );
    }
}
