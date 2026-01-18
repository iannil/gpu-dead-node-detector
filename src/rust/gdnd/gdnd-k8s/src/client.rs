//! Kubernetes Client wrapper
//!
//! Provides a simplified interface to the Kubernetes API.

use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::{Node, Pod};
use kube::api::{Api, ListParams, Patch, PatchParams};
use kube::{Client, Config};
use serde_json::json;
use tracing::{debug, info};

/// Kubernetes client wrapper
pub struct K8sClient {
    client: Client,
}

impl K8sClient {
    /// Create a new K8s client using in-cluster config
    pub async fn new() -> Result<Self> {
        let client = Client::try_default()
            .await
            .context("Failed to create Kubernetes client")?;

        info!("Connected to Kubernetes API server");
        Ok(Self { client })
    }

    /// Create a new K8s client with custom config
    pub async fn with_config(config: Config) -> Result<Self> {
        let client = Client::try_from(config)
            .context("Failed to create Kubernetes client from config")?;

        Ok(Self { client })
    }

    /// Get the underlying kube client
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Get node API
    pub fn nodes(&self) -> Api<Node> {
        Api::all(self.client.clone())
    }

    /// Get pods API for a namespace
    pub fn pods(&self, namespace: &str) -> Api<Pod> {
        Api::namespaced(self.client.clone(), namespace)
    }

    /// Get all pods API
    pub fn pods_all(&self) -> Api<Pod> {
        Api::all(self.client.clone())
    }

    /// Get a node by name
    pub async fn get_node(&self, name: &str) -> Result<Node> {
        self.nodes()
            .get(name)
            .await
            .with_context(|| format!("Failed to get node: {}", name))
    }

    /// Check if the API server is reachable
    pub async fn health_check(&self) -> Result<()> {
        let _ = self
            .nodes()
            .list(&ListParams::default().limit(1))
            .await
            .context("Failed to list nodes")?;
        Ok(())
    }

    /// Cordon a node (mark as unschedulable)
    pub async fn cordon_node(&self, node_name: &str) -> Result<()> {
        let patch = json!({
            "spec": {
                "unschedulable": true
            }
        });

        let params = PatchParams::apply("gdnd");
        self.nodes()
            .patch(node_name, &params, &Patch::Merge(&patch))
            .await
            .with_context(|| format!("Failed to cordon node: {}", node_name))?;

        info!(node = node_name, "Node cordoned");
        Ok(())
    }

    /// Uncordon a node (mark as schedulable)
    pub async fn uncordon_node(&self, node_name: &str) -> Result<()> {
        let patch = json!({
            "spec": {
                "unschedulable": false
            }
        });

        let params = PatchParams::apply("gdnd");
        self.nodes()
            .patch(node_name, &params, &Patch::Merge(&patch))
            .await
            .with_context(|| format!("Failed to uncordon node: {}", node_name))?;

        info!(node = node_name, "Node uncordoned");
        Ok(())
    }

    /// Add a taint to a node
    pub async fn add_taint(
        &self,
        node_name: &str,
        key: &str,
        value: &str,
        effect: &str,
    ) -> Result<()> {
        let node = self.get_node(node_name).await?;

        let mut taints = node
            .spec
            .as_ref()
            .and_then(|s| s.taints.clone())
            .unwrap_or_default();

        // Check if taint already exists
        if taints.iter().any(|t| t.key == key) {
            debug!(node = node_name, key = key, "Taint already exists");
            return Ok(());
        }

        // Add new taint
        taints.push(k8s_openapi::api::core::v1::Taint {
            key: key.to_string(),
            value: Some(value.to_string()),
            effect: effect.to_string(),
            time_added: None,
        });

        let patch = json!({
            "spec": {
                "taints": taints
            }
        });

        let params = PatchParams::apply("gdnd");
        self.nodes()
            .patch(node_name, &params, &Patch::Merge(&patch))
            .await
            .with_context(|| format!("Failed to add taint to node: {}", node_name))?;

        info!(
            node = node_name,
            key = key,
            value = value,
            effect = effect,
            "Taint added to node"
        );
        Ok(())
    }

    /// Remove a taint from a node
    pub async fn remove_taint(&self, node_name: &str, key: &str) -> Result<()> {
        let node = self.get_node(node_name).await?;

        let taints = node
            .spec
            .as_ref()
            .and_then(|s| s.taints.clone())
            .unwrap_or_default();

        // Filter out the taint
        let new_taints: Vec<_> = taints.into_iter().filter(|t| t.key != key).collect();

        let patch = json!({
            "spec": {
                "taints": new_taints
            }
        });

        let params = PatchParams::apply("gdnd");
        self.nodes()
            .patch(node_name, &params, &Patch::Merge(&patch))
            .await
            .with_context(|| format!("Failed to remove taint from node: {}", node_name))?;

        info!(node = node_name, key = key, "Taint removed from node");
        Ok(())
    }

    /// List pods on a specific node
    pub async fn list_pods_on_node(&self, node_name: &str) -> Result<Vec<Pod>> {
        let params = ListParams::default().fields(&format!("spec.nodeName={}", node_name));

        let pods = self
            .pods_all()
            .list(&params)
            .await
            .with_context(|| format!("Failed to list pods on node: {}", node_name))?;

        Ok(pods.items)
    }

    /// Evict a pod
    pub async fn evict_pod(&self, namespace: &str, name: &str) -> Result<()> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        pods.evict(name, &Default::default())
            .await
            .with_context(|| format!("Failed to evict pod: {}/{}", namespace, name))?;

        info!(namespace = namespace, pod = name, "Pod evicted");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Integration tests would require a running Kubernetes cluster
    // Unit tests are limited for K8s client
}
