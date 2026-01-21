# GDND - GPU 故障节点检测器

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Kubernetes](https://img.shields.io/badge/kubernetes-1.25%2B-326CE5.svg)](https://kubernetes.io/)

[English](README.md) | [中文](README_CN.md)

**GDND** 是一个面向 Kubernetes 集群的主动式 GPU 健康监控与故障隔离系统。它以 DaemonSet 形式运行在所有 GPU 节点上，通过多级检测发现不健康的 GPU，并自动通过 Taint/Cordon 机制隔离故障节点。

## 核心特性

- **三级检测流水线**
  - **L1 被动检测** (30秒): NVML 查询、XID 错误扫描、僵尸进程检测
  - **L2 主动检测** (5分钟): CUDA 128x128 矩阵乘法微基准测试
  - **L3 PCIe 检测** (24小时，可选): PCIe 带宽测试

- **健康状态机**: `HEALTHY` → `SUSPECTED` → `UNHEALTHY` → `ISOLATED`

- **自动隔离**: Cordon 节点、添加 Taint、驱逐 Pod (可配置)

- **Prometheus 指标**: 完整的可观测性支持，包括 `gdnd_gpu_status`、温度、利用率等指标

- **轻量级**: 目标镜像 < 50MB，资源占用极低 (10m CPU, 32Mi 内存)

- **可扩展**: 设备抽象层支持 NVIDIA GPU 和华为昇腾 NPU

## 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                         GDND DaemonSet                          │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │ L1 被动检测 │  │ L2 主动检测 │  │ L3 PCIe     │  检测器     │
│  │ (30秒)      │  │ (5分钟)     │  │ (24小时)    │             │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘             │
│         │                │                │                     │
│         └────────────────┼────────────────┘                     │
│                          ▼                                      │
│              ┌───────────────────────┐                          │
│              │     健康状态机        │                          │
│              │  HEALTHY → SUSPECTED  │                          │
│              │  → UNHEALTHY → ISOLATED│                         │
│              └───────────┬───────────┘                          │
│                          │                                      │
│         ┌────────────────┼────────────────┐                     │
│         ▼                ▼                ▼                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │   Cordon    │  │    Taint    │  │    告警     │  隔离动作   │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
```

## 快速开始

### 前置条件

- Kubernetes 集群 1.25+
- 已安装驱动的 NVIDIA GPU 节点
- 已配置访问集群的 `kubectl`

### 使用 Helm 安装（推荐）

```bash
# 从本地 chart 安装
helm install gdnd ./release/rust/gdnd/chart \
  --namespace kube-system \
  --set config.dryRun=true  # 先以 dry-run 模式启动以确保安全

# 验证日志无误后，禁用 dry-run
helm upgrade gdnd ./release/rust/gdnd/chart \
  --namespace kube-system \
  --set config.dryRun=false
```

### 使用 kubectl 安装

```bash
cd release/rust/gdnd/deploy

# 应用 RBAC
kubectl apply -f rbac.yaml

# 应用 ConfigMap
kubectl apply -f configmap.yaml

# 部署 DaemonSet
kubectl apply -f daemonset.yaml
```

### 验证安装

```bash
# 检查 DaemonSet 状态
kubectl get daemonset gdnd -n kube-system

# 查看日志
kubectl logs -l app.kubernetes.io/name=gdnd -n kube-system -f

# 检查指标
kubectl port-forward -n kube-system daemonset/gdnd 9100:9100
curl http://localhost:9100/metrics | grep gdnd_gpu
```

## 配置

### 主要配置项

| 参数 | 说明 | 默认值 |
| ------ | ------ | -------- |
| `device_type` | 设备类型: `auto`, `nvidia`, `ascend` | `auto` |
| `l1_interval` | L1 被动检测间隔 | `30s` |
| `l2_interval` | L2 主动检测间隔 | `5m` |
| `health.failure_threshold` | 连续失败多少次后标记为 UNHEALTHY | `3` |
| `health.fatal_xids` | 致命 XID 错误码（立即隔离） | `[31, 43, 48, 79]` |
| `health.temperature_threshold` | 温度阈值（摄氏度） | `85` |
| `isolation.cordon` | 是否 Cordon 不健康的节点 | `true` |
| `isolation.evict_pods` | 是否驱逐 Pod | `false` |
| `isolation.taint_key` | Taint 键名 | `nvidia.com/gpu-health` |
| `isolation.taint_effect` | Taint 效果 | `NoSchedule` |
| `dry_run` | 只记录日志不执行操作 | `false` |

### 配置示例 config.yaml

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

## 致命 XID 错误码

以下 XID 错误会触发 GPU 立即隔离：

| XID | 说明 |
| ----- | ------ |
| 31 | GPU 内存页错误 / MMU 故障 |
| 43 | GPU 停止处理 |
| 48 | 双比特 ECC 错误 |
| 79 | GPU 从总线脱落 |

## Prometheus 指标

| 指标名 | 类型 | 标签 | 说明 |
| -------- | ------ | ------ | ------ |
| `gdnd_gpu_status` | Gauge | gpu, uuid, name | 健康状态 (0=健康, 1=疑似, 2=不健康, 3=已隔离) |
| `gdnd_gpu_temperature_celsius` | Gauge | gpu | GPU 温度 |
| `gdnd_gpu_utilization_percent` | Gauge | gpu | GPU 利用率 |
| `gdnd_gpu_memory_used_bytes` | Gauge | gpu | GPU 已用显存 |
| `gdnd_check_duration_seconds` | Histogram | level, gpu | 检测耗时 |
| `gdnd_check_failures_total` | Counter | level, gpu, reason | 检测失败总数 |
| `gdnd_isolation_actions_total` | Counter | action | 隔离动作总数 |
| `gdnd_gpu_count` | Gauge | - | 检测到的 GPU 数量 |

## 开发

- Rust 1.75+
- CUDA Toolkit 12.2+ (用于编译 gpu-check 二进制文件)

### 从源码构建

```bash
cd src/rust/gdnd

# 检查编译
cargo check

# 运行测试
cargo test

# 构建发布版本
cargo build --release

# 本地运行 (dry-run 模式)
cargo run -- --config configs/config.yaml --node-name test-node --dry-run
```

### 构建 Docker 镜像

```bash
cd release/rust/gdnd

# 构建发布版本二进制
./build.sh

# 构建 Docker 镜像
./build.sh --docker
```

### 项目结构

```
src/rust/gdnd/
├── gdnd/                    # 主程序
│   └── src/
│       ├── main.rs          # 入口点
│       ├── config.rs        # 配置
│       └── cli.rs           # 命令行参数
├── gdnd-core/               # 核心检测逻辑
│   └── src/
│       ├── device/          # 设备抽象
│       │   ├── interface.rs # DeviceInterface trait
│       │   ├── nvidia.rs    # NVIDIA 实现
│       │   └── mock.rs      # 测试用 Mock
│       ├── detection/       # 检测器
│       │   ├── l1_passive.rs
│       │   └── l2_active.rs
│       ├── state_machine.rs # 健康状态机
│       ├── scheduler.rs     # 检测调度器
│       └── metrics.rs       # Prometheus 指标
├── gdnd-k8s/                # Kubernetes 集成
│   └── src/
│       ├── client.rs        # K8s 客户端
│       └── node_ops.rs      # 节点操作
└── gpu-check/               # CUDA 微基准测试
    └── gpu_check.cu         # 128x128 矩阵乘法

release/rust/gdnd/
├── build.sh                 # 构建脚本
├── chart/                   # Helm chart
├── configs/                 # 生产配置
└── deploy/                  # K8s 部署清单
```

## 与其他方案对比

| 特性 | GDND | Node Problem Detector | DIY 脚本 |
| ------ | ------ | ---------------------- | ---------- |
| GPU 专项检测 | ✅ XID、ECC、驱动死锁 | ❌ 通用 | 视情况 |
| 主动健康检查 | ✅ CUDA 矩阵乘法 | ❌ | 视情况 |
| 自动隔离 | ✅ Cordon + Taint | ⚠️ 需手动规则 | ⚠️ |
| 镜像大小 | < 50MB | ~100MB | 视情况 |
| 配置方式 | 简单 YAML | 复杂 | 自定义 |
| Prometheus 指标 | ✅ 内置 | ✅ | 需手动 |

## 路线图

### 已完成 (v1.0)

- [x] Rust 核心实现，支持 NVIDIA GPU
- [x] L1 被动检测（NVML/npu-smi、XID 扫描、僵尸进程检测）
- [x] L2 主动检测（CUDA/AscendCL 微基准测试）
- [x] 健康状态机（HEALTHY → SUSPECTED → UNHEALTHY → ISOLATED）
- [x] Kubernetes 集成（Cordon/Taint/Evict）
- [x] Prometheus 指标
- [x] Helm Chart 部署
- [x] **华为昇腾 NPU 完整支持** (2026-01-21)

### 进行中

- [ ] L3 PCIe 带宽测试（框架就绪，完成 80%）
- [ ] 真实硬件集成测试

### 规划中

- [ ] ECC 错误检测增强
- [ ] Grafana 仪表板模板
- [ ] AlertManager 集成
- [ ] 节点自动恢复（GPU 重置，ISOLATED → HEALTHY）
- [ ] 多 GPU 单卡隔离

## 参与贡献

欢迎贡献代码！请随时提交 Issue 和 Pull Request。

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 运行测试 (`cargo test`)
4. 提交更改 (`git commit -m 'Add amazing feature'`)
5. 推送分支 (`git push origin feature/amazing-feature`)
6. 发起 Pull Request

## 许可证

本项目采用 Apache License 2.0 许可证 - 详见 [LICENSE](LICENSE) 文件。

## 致谢

- [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper) - NVIDIA NVML 的 Rust 绑定
- [kube-rs](https://github.com/kube-rs/kube) - Rust Kubernetes 客户端
- [prometheus-rs](https://github.com/tikv/rust-prometheus) - Rust Prometheus 客户端
