# GDND - GPU Dead Node Detector

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Kubernetes](https://img.shields.io/badge/kubernetes-1.25%2B-326CE5.svg)](https://kubernetes.io/)

[English](README.md) | [中文](README_CN.md)

**GDND** is a proactive GPU health monitoring and fault isolation system for Kubernetes clusters. It runs as a DaemonSet on all GPU nodes, detects unhealthy GPUs through multi-level detection, and automatically isolates faulty nodes via Taint/Cordon mechanisms.

## Features

- **Three-tier Detection Pipeline**
  - **L1 Passive Detection** (30s): NVML queries, XID error scanning, zombie process detection
  - **L2 Active Detection** (5min): CUDA 128x128 matrix multiplication micro-benchmark
  - **L3 PCIe Detection** (24h, optional): PCIe bandwidth testing

- **Health State Machine**: `HEALTHY` → `SUSPECTED` → `UNHEALTHY` → `ISOLATED`

- **Automatic Isolation**: Cordon nodes, apply taints, evict pods (configurable)

- **Prometheus Metrics**: Full observability with `gdnd_gpu_status`, temperature, utilization metrics

- **Lightweight**: Target image size < 50MB, minimal resource footprint (10m CPU, 32Mi memory)

- **Extensible**: Device abstraction layer supports NVIDIA GPUs and Huawei Ascend NPUs

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         GDND DaemonSet                          │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │ L1 Passive  │  │ L2 Active   │  │ L3 PCIe     │  Detectors  │
│  │ (30s)       │  │ (5min)      │  │ (24h)       │             │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘             │
│         │                │                │                     │
│         └────────────────┼────────────────┘                     │
│                          ▼                                      │
│              ┌───────────────────────┐                          │
│              │   Health State Machine │                         │
│              │  HEALTHY → SUSPECTED  │                          │
│              │  → UNHEALTHY → ISOLATED│                         │
│              └───────────┬───────────┘                          │
│                          │                                      │
│         ┌────────────────┼────────────────┐                     │
│         ▼                ▼                ▼                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │   Cordon    │  │    Taint    │  │    Alert    │  Actions    │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- Kubernetes cluster 1.25+
- NVIDIA GPU nodes with drivers installed
- `kubectl` configured to access your cluster

### Install with Helm (Recommended)

```bash
# Install from local chart
helm install gdnd ./release/rust/gdnd/chart \
  --namespace kube-system \
  --set config.dryRun=true  # Start in dry-run mode for safety

# After verifying logs, disable dry-run
helm upgrade gdnd ./release/rust/gdnd/chart \
  --namespace kube-system \
  --set config.dryRun=false
```

### Install with kubectl

```bash
cd release/rust/gdnd/deploy

# Apply RBAC
kubectl apply -f rbac.yaml

# Apply ConfigMap
kubectl apply -f configmap.yaml

# Deploy DaemonSet
kubectl apply -f daemonset.yaml
```

### Verify Installation

```bash
# Check DaemonSet status
kubectl get daemonset gdnd -n kube-system

# View logs
kubectl logs -l app.kubernetes.io/name=gdnd -n kube-system -f

# Check metrics
kubectl port-forward -n kube-system daemonset/gdnd 9100:9100
curl http://localhost:9100/metrics | grep gdnd_gpu
```

## Configuration

### Key Configuration Options

| Parameter | Description | Default |
| ----------- | ------------- | --------- |
| `device_type` | Device type: `auto`, `nvidia`, `ascend` | `auto` |
| `l1_interval` | L1 passive detection interval | `30s` |
| `l2_interval` | L2 active detection interval | `5m` |
| `health.failure_threshold` | Consecutive failures before UNHEALTHY | `3` |
| `health.fatal_xids` | Fatal XID codes for immediate isolation | `[31, 43, 48, 79]` |
| `health.temperature_threshold` | Temperature threshold (Celsius) | `85` |
| `isolation.cordon` | Whether to cordon unhealthy nodes | `true` |
| `isolation.evict_pods` | Whether to evict pods | `false` |
| `isolation.taint_key` | Taint key | `nvidia.com/gpu-health` |
| `isolation.taint_effect` | Taint effect | `NoSchedule` |
| `dry_run` | Log actions without executing | `false` |

### Example config.yaml

```yaml
device_type: auto
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

dry_run: false
```

## Fatal XID Error Codes

These XID errors trigger immediate GPU isolation:

| XID | Description |
| ----- | ------------- |
| 31 | GPU memory page fault / MMU fault |
| 43 | GPU stopped processing |
| 48 | Double Bit ECC Error |
| 79 | GPU has fallen off the bus |

## Prometheus Metrics

| Metric | Type | Labels | Description |
| -------- | ------ | -------- | ------------- |
| `gdnd_gpu_status` | Gauge | gpu, uuid, name | Health status (0=healthy, 1=suspected, 2=unhealthy, 3=isolated) |
| `gdnd_gpu_temperature_celsius` | Gauge | gpu | GPU temperature |
| `gdnd_gpu_utilization_percent` | Gauge | gpu | GPU utilization |
| `gdnd_gpu_memory_used_bytes` | Gauge | gpu | GPU memory used |
| `gdnd_check_duration_seconds` | Histogram | level, gpu | Detection check duration |
| `gdnd_check_failures_total` | Counter | level, gpu, reason | Total detection failures |
| `gdnd_isolation_actions_total` | Counter | action | Total isolation actions |
| `gdnd_gpu_count` | Gauge | - | Number of GPUs detected |

## Development

- Rust 1.75+
- CUDA Toolkit 12.2+ (for gpu-check binary)

### Build from Source

```bash
cd src/rust/gdnd

# Check compilation
cargo check

# Run tests
cargo test

# Build release binary
cargo build --release

# Run locally (dry-run mode)
cargo run -- --config configs/config.yaml --node-name test-node --dry-run
```

### Build Docker Image

```bash
cd release/rust/gdnd

# Build release binaries
./build.sh

# Build Docker image
./build.sh --docker
```

### Project Structure

```
src/rust/gdnd/
├── gdnd/                    # Main binary
│   └── src/
│       ├── main.rs          # Entry point
│       ├── config.rs        # Configuration
│       └── cli.rs           # CLI arguments
├── gdnd-core/               # Core detection logic
│   └── src/
│       ├── device/          # Device abstraction
│       │   ├── interface.rs # DeviceInterface trait
│       │   ├── nvidia.rs    # NVIDIA implementation
│       │   └── mock.rs      # Mock for testing
│       ├── detection/       # Detectors
│       │   ├── l1_passive.rs
│       │   └── l2_active.rs
│       ├── state_machine.rs # Health state machine
│       ├── scheduler.rs     # Detection scheduler
│       └── metrics.rs       # Prometheus metrics
├── gdnd-k8s/                # Kubernetes integration
│   └── src/
│       ├── client.rs        # K8s client
│       └── node_ops.rs      # Node operations
└── gpu-check/               # CUDA micro-benchmark
    └── gpu_check.cu         # 128x128 matrix multiply

release/rust/gdnd/
├── build.sh                 # Build script
├── chart/                   # Helm chart
├── configs/                 # Production configs
└── deploy/                  # K8s manifests
```

## Comparison with Alternatives

| Feature | GDND | Node Problem Detector | DIY Scripts |
| --------- | ------ | ---------------------- | ------------- |
| GPU-specific detection | ✅ XID, ECC, driver deadlock | ❌ Generic | Varies |
| Active health check | ✅ CUDA matrix mul | ❌ | Varies |
| Automatic isolation | ✅ Cordon + Taint | ⚠️ Manual rules | ⚠️ |
| Image size | < 50MB | ~100MB | Varies |
| Configuration | Simple YAML | Complex | Custom |
| Prometheus metrics | ✅ Built-in | ✅ | Manual |

## Roadmap

- [ ] ECC error detection enhancement
- [ ] Huawei Ascend NPU full support
- [ ] L3 PCIe bandwidth test implementation
- [ ] Grafana dashboard templates
- [ ] AlertManager integration
- [ ] Node auto-recovery (GPU reset)
- [ ] Multi-GPU per-device isolation

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Run tests (`cargo test`)
4. Commit your changes (`git commit -m 'Add amazing feature'`)
5. Push to the branch (`git push origin feature/amazing-feature`)
6. Open a Pull Request

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper) - Rust bindings for NVIDIA NVML
- [kube-rs](https://github.com/kube-rs/kube) - Kubernetes client for Rust
- [prometheus-rs](https://github.com/tikv/rust-prometheus) - Prometheus client for Rust
