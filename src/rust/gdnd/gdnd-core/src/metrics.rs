//! Prometheus metrics for GDND

use once_cell::sync::Lazy;
use prometheus::{
    opts, register_gauge_vec, register_histogram_vec, register_int_counter_vec, register_int_gauge,
    GaugeVec, HistogramVec, IntCounterVec, IntGauge,
};

use crate::device::DeviceId;
use crate::state_machine::HealthState;

/// GPU status metric (0=healthy, 1=suspected, 2=unhealthy, 3=isolated)
static GPU_STATUS: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        opts!("gdnd_gpu_status", "GPU health status"),
        &["gpu", "uuid", "name"]
    )
    .expect("Failed to create gpu_status metric")
});

/// GPU temperature metric
static GPU_TEMPERATURE: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        opts!("gdnd_gpu_temperature_celsius", "GPU temperature in Celsius"),
        &["gpu"]
    )
    .expect("Failed to create gpu_temperature metric")
});

/// GPU utilization metric
static GPU_UTILIZATION: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        opts!("gdnd_gpu_utilization_percent", "GPU utilization percentage"),
        &["gpu"]
    )
    .expect("Failed to create gpu_utilization metric")
});

/// GPU memory used metric
static GPU_MEMORY_USED: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        opts!("gdnd_gpu_memory_used_bytes", "GPU memory used in bytes"),
        &["gpu"]
    )
    .expect("Failed to create gpu_memory_used metric")
});

/// Detection check duration histogram
static CHECK_DURATION: Lazy<HistogramVec> = Lazy::new(|| {
    register_histogram_vec!(
        "gdnd_check_duration_seconds",
        "Duration of detection checks",
        &["level", "gpu"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
    )
    .expect("Failed to create check_duration metric")
});

/// Detection failure counter
static CHECK_FAILURES: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        opts!("gdnd_check_failures_total", "Total number of detection failures"),
        &["level", "gpu", "reason"]
    )
    .expect("Failed to create check_failures metric")
});

/// Isolation action counter
static ISOLATION_ACTIONS: Lazy<IntCounterVec> = Lazy::new(|| {
    register_int_counter_vec!(
        opts!("gdnd_isolation_actions_total", "Total number of isolation actions"),
        &["action"]
    )
    .expect("Failed to create isolation_actions metric")
});

/// Number of GPUs detected
static GPU_COUNT: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        opts!("gdnd_gpu_count", "Number of GPUs detected")
    )
    .expect("Failed to create gpu_count metric")
});

/// Metrics registry wrapper
pub struct MetricsRegistry;

impl MetricsRegistry {
    /// Create a new metrics registry
    pub fn new() -> Self {
        // Force initialization of lazy statics
        let _ = &*GPU_STATUS;
        let _ = &*GPU_TEMPERATURE;
        let _ = &*GPU_UTILIZATION;
        let _ = &*GPU_MEMORY_USED;
        let _ = &*CHECK_DURATION;
        let _ = &*CHECK_FAILURES;
        let _ = &*ISOLATION_ACTIONS;
        let _ = &*GPU_COUNT;
        Self
    }

    /// Set GPU count
    pub fn set_gpu_count(&self, count: i64) {
        GPU_COUNT.set(count);
    }

    /// Set GPU status
    pub fn set_gpu_status(&self, device: &DeviceId, state: HealthState) {
        let status = match state {
            HealthState::Healthy => 0.0,
            HealthState::Suspected => 1.0,
            HealthState::Unhealthy => 2.0,
            HealthState::Isolated => 3.0,
        };
        GPU_STATUS
            .with_label_values(&[
                &device.index.to_string(),
                device.uuid.as_deref().unwrap_or(""),
                &device.name,
            ])
            .set(status);
    }

    /// Set GPU temperature
    pub fn set_gpu_temperature(&self, device: &DeviceId, temp: f64) {
        GPU_TEMPERATURE
            .with_label_values(&[&device.index.to_string()])
            .set(temp);
    }

    /// Set GPU utilization
    pub fn set_gpu_utilization(&self, device: &DeviceId, util: f64) {
        GPU_UTILIZATION
            .with_label_values(&[&device.index.to_string()])
            .set(util);
    }

    /// Set GPU memory used
    pub fn set_gpu_memory_used(&self, device: &DeviceId, bytes: f64) {
        GPU_MEMORY_USED
            .with_label_values(&[&device.index.to_string()])
            .set(bytes);
    }

    /// Record check duration
    pub fn observe_check_duration(&self, level: &str, device: &DeviceId, duration_secs: f64) {
        CHECK_DURATION
            .with_label_values(&[level, &device.index.to_string()])
            .observe(duration_secs);
    }

    /// Increment check failure counter
    pub fn inc_check_failure(&self, level: &str, device: &DeviceId, reason: &str) {
        CHECK_FAILURES
            .with_label_values(&[level, &device.index.to_string(), reason])
            .inc();
    }

    /// Increment isolation action counter
    pub fn inc_isolation_action(&self, action: &str) {
        ISOLATION_ACTIONS.with_label_values(&[action]).inc();
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_registry() {
        let registry = MetricsRegistry::new();
        let device = DeviceId {
            index: 0,
            uuid: Some("GPU-TEST".to_string()),
            name: "Test GPU".to_string(),
        };

        registry.set_gpu_count(2);
        registry.set_gpu_status(&device, HealthState::Healthy);
        registry.set_gpu_temperature(&device, 45.0);
        registry.set_gpu_utilization(&device, 75.0);
        registry.set_gpu_memory_used(&device, 8_000_000_000.0);
        registry.observe_check_duration("L1", &device, 0.025);
        registry.inc_check_failure("L2", &device, "timeout");
        registry.inc_isolation_action("cordon");
    }
}
