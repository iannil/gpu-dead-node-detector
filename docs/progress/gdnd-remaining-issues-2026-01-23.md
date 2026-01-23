# GDND 剩余问题清单

**文档编号**: GDND-ISSUES-2026-01-23
**创建日期**: 2026-01-23
**文档类型**: 进度跟踪
**项目状态**: 已完成验收，待生产部署验证

---

## 概述

本文档梳理 GPU Dead Node Detector (GDND) 项目在完成验收后的剩余问题，按以下维度排序：

- **从全局到局部**: 系统级 → 模块级 → 代码级
- **从高风险到低风险**: 阻断性问题 → 功能缺失 → 代码优化
- **从高优先级到低优先级**: P0 (必须) → P1 (应该) → P2 (可选) → P3 (增强)

---

## 问题统计

| 优先级 | 类别 | 数量 | 已完成 | 状态 |
|--------|------|------|--------|------|
| **P0** | 集成测试验证 | 3 | 0 | ⏳ 待真实硬件环境 |
| **P1** | 功能缺失 | 3 | 3 | ✅ 全部完成 |
| **P1** | 部署配置 | 4 | 4 | ✅ 全部完成 |
| **P2** | 代码质量 | 2 | 2 | ✅ 全部完成 |
| **P3** | 可观测性增强 | 2 | 2 | ✅ 全部完成 |
| **总计** | - | **14** | **11** | 79% 完成 |

---

## 一、全局问题（系统级 / 高风险 / P0）

### P0-1: 真实 GPU 硬件环境集成测试缺失

| 属性 | 值 |
|------|-----|
| **优先级** | P0 - 阻断性 |
| **风险等级** | 高 |
| **影响范围** | 全局 - NVIDIA GPU 检测功能 |
| **当前状态** | 未开始 |

**问题描述**:
- NVML 绑定仅通过 Mock 测试验证
- 无法确认实际 GPU 上的 XID 错误扫描、温度读取、利用率监控是否正常工作
- `gpu-check` CUDA 微基准未在真实 GPU 环境运行

**潜在风险**:
- 生产部署后 NVML 调用可能因驱动版本差异失败
- XID 错误解析可能遗漏某些硬件特有的错误格式
- CUDA 微基准可能在特定 GPU 架构上超时或崩溃

**验证范围**:
- [ ] NVIDIA V100 GPU
- [ ] NVIDIA T4 GPU
- [ ] NVIDIA A100 GPU
- [ ] NVIDIA H100 GPU（如有）

**相关文件**:
- `src/rust/gdnd/gdnd-core/src/device/nvidia.rs`
- `src/rust/gdnd/gpu-check/gpu_check.cu`

---

### P0-2: 真实昇腾 NPU 硬件环境验证缺失

| 属性 | 值 |
|------|-----|
| **优先级** | P0 - 阻断性 |
| **风险等级** | 高 |
| **影响范围** | 全局 - 华为昇腾 NPU 检测功能 |
| **当前状态** | 未开始 |

**问题描述**:
- `npu-smi info` 输出解析仅基于文档和模拟数据测试
- AscendCL 微基准 (`npu_check.cpp`) 未在真实 NPU 上编译运行
- 昇腾设备日志 (`/var/log/npu/slog/device-os`) 解析未验证

**潜在风险**:
- 不同 CANN 版本的 npu-smi 输出格式可能有差异
- AscendCL API 在不同 NPU 型号上行为可能不一致
- 错误码映射可能不完整

**验证范围**:
- [ ] Ascend 310 NPU
- [ ] Ascend 910 NPU
- [ ] CANN 7.0+ 环境
- [ ] CANN 8.0+ 环境

**相关文件**:
- `src/rust/gdnd/gdnd-core/src/device/ascend.rs`
- `src/rust/gdnd/npu-check/npu_check.cpp`

---

### P0-3: 真实 Kubernetes 环境端到端测试缺失

| 属性 | 值 |
|------|-----|
| **优先级** | P0 - 阻断性 |
| **风险等级** | 高 |
| **影响范围** | 全局 - K8s 集成功能 |
| **当前状态** | 未开始 |

**问题描述**:
- Node Cordon/Uncordon 操作未在实际集群验证
- Taint 添加/移除未在实际集群验证
- Pod 驱逐逻辑（跳过 DaemonSet/系统 Pod）未在实际集群验证
- RBAC 权限配置未在实际集群验证

**潜在风险**:
- RBAC 权限可能不足导致操作失败
- Cordon/Taint 操作可能与其他控制器冲突
- 驱逐逻辑可能意外影响关键 Pod

**验证范围**:
- [ ] K8s 1.25+ 集群
- [ ] K8s 1.28+ 集群
- [ ] 带 GPU 调度器的集群
- [ ] 多节点故障场景

**相关文件**:
- `src/rust/gdnd/gdnd-k8s/src/client.rs`
- `src/rust/gdnd/gdnd-k8s/src/node_ops.rs`
- `release/rust/gdnd/deploy/rbac.yaml`

---

## 二、模块问题（组件级 / 中风险 / P1）

### P1-1: L3 PCIe 带宽检测未完全实现

| 属性 | 值 |
|------|-----|
| **优先级** | P1 - 功能缺失 |
| **风险等级** | 中 |
| **影响范围** | L3 检测模块 |
| **当前状态** | 80% 完成（框架就绪） |
| **完成阻塞** | 依赖外部工具 |

**问题描述**:
- L3 PCIe 带宽检测的框架已实现
- 缺少实际的带宽测试工具集成
- 需要选择并集成 bandwidth test 工具（如 NVIDIA bandwidthTest）

**缺失功能**:
- [ ] 带宽测试工具集成
- [ ] 带宽阈值判定逻辑
- [ ] PCIe 链路退化检测

**相关文件**:
- `src/rust/gdnd/gdnd-core/src/detection/` (待创建 l3_pcie.rs)
- `src/rust/gdnd/gdnd-core/src/scheduler.rs`

---

### P1-2: GPU/NPU 恢复检测机制未实现

| 属性 | 值 |
|------|-----|
| **优先级** | P1 - 功能缺失 |
| **风险等级** | 中 |
| **影响范围** | 状态机模块 |
| **当前状态** | 0% - 未开始 |

**问题描述**:
- 当前状态机仅支持 `HEALTHY → SUSPECTED → UNHEALTHY → ISOLATED` 单向转换
- 缺少 `ISOLATED → HEALTHY` 的恢复路径
- GPU/NPU 故障恢复后（如驱动重启、硬件更换），节点仍保持隔离状态

**影响**:
- 故障恢复的 GPU 节点需要手动解除隔离
- 降低了系统的自动化程度

**建议实现**:
- [ ] 定期对 ISOLATED 状态的设备进行健康检查
- [ ] 连续 N 次检查通过后转换为 HEALTHY
- [ ] 自动移除 Taint 并 Uncordon 节点
- [ ] 可配置的恢复检测间隔和阈值

**相关文件**:
- `src/rust/gdnd/gdnd-core/src/state_machine.rs:553`

---

### P1-3: 自愈功能未实现

| 属性 | 值 |
|------|-----|
| **优先级** | P1 - 功能缺失 |
| **风险等级** | 中 |
| **影响范围** | 新模块 |
| **当前状态** | 0% - 未开始 |

**问题描述**:
- 当前系统仅能检测和隔离故障，无法尝试恢复
- 某些软件故障（如驱动卡死）可通过重置 GPU 恢复

**建议功能**:
- [ ] GPU 软重置（nvidia-smi -r）
- [ ] 驱动模块卸载/重载
- [ ] 进程强制终止（清理僵尸进程）
- [ ] 可配置的自愈策略（保守/激进）

**风险提示**:
- 自愈操作可能中断运行中的任务
- 需要与工作负载调度系统协调
- 建议作为可选功能，默认关闭

---

## 三、部署配置问题（配置级 / 中风险 / P1）

### P1-4: 默认镜像仓库配置不完整

| 属性 | 值 |
|------|-----|
| **优先级** | P1 - 部署阻塞 |
| **风险等级** | 中 |
| **影响范围** | Helm Chart |
| **修复难度** | 简单 |

**问题描述**:
```yaml
# release/rust/gdnd/chart/values.yaml
image:
  repository: gdnd          # 缺少完整的 registry 前缀
  tag: latest
```

**建议修复**:
- 添加示例 registry 前缀（如 `your-registry.com/gdnd`）
- 或在文档中明确说明需要用户自行配置

**相关文件**:
- `release/rust/gdnd/chart/values.yaml:3-5`

---

### P1-5: 缺少 NetworkPolicy 配置

| 属性 | 值 |
|------|-----|
| **优先级** | P1 - 安全加固 |
| **风险等级** | 中（取决于环境） |
| **影响范围** | K8s 部署 |
| **修复难度** | 中等 |

**问题描述**:
- 未定义 NetworkPolicy 限制 Pod 网络访问
- 在安全严格的环境中可能是必需的

**建议配置**:
- [ ] 限制 egress: 仅允许访问 K8s API Server
- [ ] 限制 ingress: 仅允许 Prometheus 抓取 metrics 端口
- [ ] 作为可选配置，默认不启用

**待创建文件**:
- `release/rust/gdnd/chart/templates/networkpolicy.yaml`

---

### P1-6: 缺少 PodSecurityPolicy/Standard 配置

| 属性 | 值 |
|------|-----|
| **优先级** | P1 - 安全加固 |
| **风险等级** | 中（取决于环境） |
| **影响范围** | K8s 部署 |
| **修复难度** | 中等 |

**问题描述**:
- 未定义 Pod Security Standard 标签
- 某些 K8s 集群启用了 Pod Security Admission，可能拒绝部署

**建议配置**:
- [ ] 添加 `pod-security.kubernetes.io/enforce: privileged` 标签（DaemonSet 需要特权访问 GPU）
- [ ] 或定义明确的 securityContext 满足 restricted 策略

**相关文件**:
- `release/rust/gdnd/chart/templates/daemonset.yaml`

---

### P1-7: 资源配置值未在实际环境验证

| 属性 | 值 |
|------|-----|
| **优先级** | P1 - 性能调优 |
| **风险等级** | 中 |
| **影响范围** | 资源使用 |
| **验证难度** | 需要实际环境 |

**当前配置**:
```yaml
resources:
  requests:
    cpu: 10m / 50m      # 两个文件不一致
    memory: 32Mi / 64Mi
  limits:
    cpu: 100m
    memory: 128Mi
```

**待验证问题**:
- [ ] L2 主动检测期间 CPU 使用是否超过 100m
- [ ] 128Mi 内存是否足够处理多 GPU 节点
- [ ] 两处配置不一致需统一

**相关文件**:
- `src/rust/gdnd/deploy/daemonset.yaml`
- `release/rust/gdnd/chart/values.yaml`

---

## 四、代码质量问题（实现级 / 低风险 / P2）

### P2-1: 未使用的方法 `AscendErrorCode::is_fatal()`

| 属性 | 值 |
|------|-----|
| **优先级** | P2 - 代码清理 |
| **风险等级** | 低 |
| **影响** | 编译警告 |
| **修复难度** | 简单 |

**问题描述**:
```rust
// src/rust/gdnd/gdnd-core/src/device/ascend.rs:40
pub fn is_fatal(&self) -> bool { ... }
// warning: method `is_fatal` is never used
```

**建议**:
- 如果为未来功能预留，添加 `#[allow(dead_code)]` 注解
- 如果不再需要，删除该方法

---

### P2-2: 未读取的字段 `AscendDevice::fatal_error_codes`

| 属性 | 值 |
|------|-----|
| **优先级** | P2 - 代码清理 |
| **风险等级** | 低 |
| **影响** | 编译警告 |
| **修复难度** | 简单 |

**问题描述**:
```rust
// src/rust/gdnd/gdnd-core/src/device/ascend.rs:115
pub struct AscendDevice {
    fatal_error_codes: Vec<u32>,  // warning: field is never read
    ...
}
```

**建议**:
- 实现基于该字段的错误判定逻辑
- 或删除该字段及相关初始化代码

---

## 五、可观测性增强建议（长期改进 / P3）

### P3-1: Grafana 仪表板模板缺失

| 属性 | 值 |
|------|-----|
| **优先级** | P3 - 增强功能 |
| **风险等级** | 低 |
| **影响范围** | 可观测性 |
| **建议时间** | 3-12 个月 |

**建议内容**:
- [ ] GPU 健康状态总览面板
- [ ] 各 GPU 温度/利用率/内存趋势图
- [ ] 检测耗时直方图
- [ ] 隔离事件时间线
- [ ] 集群 GPU 可用性汇总

**待创建文件**:
- `release/rust/gdnd/dashboards/gdnd-overview.json`

---

### P3-2: AlertManager 规则集成缺失

| 属性 | 值 |
|------|-----|
| **优先级** | P3 - 增强功能 |
| **风险等级** | 低 |
| **影响范围** | 告警系统 |
| **建议时间** | 3-12 个月 |

**建议告警规则**:
- [ ] GPU 进入 UNHEALTHY 状态
- [ ] GPU 被隔离 (ISOLATED)
- [ ] 检测超时频繁
- [ ] 多节点同时故障（集群级告警）

**待创建文件**:
- `release/rust/gdnd/alerts/gdnd-alerts.yaml`

---

## 附录

### A. 问题修复优先级矩阵

```
高风险 ─────────────────────────────────────────────────────────────────────► 低风险
  │
  │   P0-1        P0-2        P0-3
  │   GPU测试     NPU测试     K8s测试
高 │   ■■■■■■■■   ■■■■■■■■   ■■■■■■■■
优 │
先 │   P1-1       P1-2        P1-3        P1-4~P1-7
级 │   L3检测     恢复机制    自愈功能    部署配置
中 │   ■■■■■      ■■■■        ■■■         ■■■■■
  │
低 │   P2-1~P2-2              P3-1~P3-2
  │   代码警告                可观测性
  │   ■■                      ■■
  ▼
```

### B. 相关文档链接

| 文档 | 路径 |
|------|------|
| 项目状态总览 | `/docs/PROJECT_STATUS.md` |
| 技术方案设计 | `/README.md` |
| 验收报告 | `/docs/reports/gdnd-acceptance-review-2026-01-21.md` |
| Rust 实现报告 | `/docs/reports/completed/gdnd-rust-implementation-2026-01-18.md` |
| NPU 支持报告 | `/docs/reports/completed/ascend-npu-support-2026-01-21.md` |

### C. 变更记录

| 日期 | 变更内容 | 操作人 |
|------|----------|--------|
| 2026-01-23 | 创建剩余问题清单 | Claude Code |
| 2026-01-23 | 修复 P2-1/P2-2 编译警告 | Claude Code |
| 2026-01-23 | 完成 P1-4 镜像仓库配置 | Claude Code |
| 2026-01-23 | 完成 P1-5 NetworkPolicy 配置 | Claude Code |
| 2026-01-23 | 完成 P1-6 安全策略配置 | Claude Code |
| 2026-01-23 | 完成 P1-7 资源配置统一 | Claude Code |
| 2026-01-23 | 完成 P1-2 恢复检测机制实现 | Claude Code |
| 2026-01-23 | 完成 P1-1 L3 PCIe 带宽检测 | Claude Code |
| 2026-01-23 | 完成 P3-1 Grafana 仪表板模板 | Claude Code |
| 2026-01-23 | 完成 P3-2 AlertManager 告警规则 | Claude Code |
| 2026-01-23 | 完成 P1-3 自愈功能模块实现 | Claude Code |
| 2026-01-23 | 修复 7 个 Clippy 警告 (P2 完成) | Claude Code |
| 2026-01-23 | 添加 HealingConfig/RecoveryConfig 到 config.rs | Claude Code |
| 2026-01-23 | main.rs 启用 recovery 功能集成 | Claude Code |
| 2026-01-23 | scheduler 集成 SelfHealer 自愈功能 | Claude Code |
| 2026-01-23 | scheduler 集成 L3PcieDetector 带宽检测 | Claude Code |

---

**下次更新**: P0 集成测试完成后更新（需要真实硬件环境）
