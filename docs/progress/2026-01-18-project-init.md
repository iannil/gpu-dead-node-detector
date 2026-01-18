# 项目初始化进展文档

**文档编号**: PROG-2026-01-18-001
**创建日期**: 2026-01-18
**最后更新**: 2026-01-18
**状态**: 进行中

---

## 概述

本文档记录 GPU Dead Node Detector (GDND) 项目的初始化状态与设计完成情况。

---

## 已完成工作

### 1. 技术方案设计 (2026-01-18)

完成了完整的技术方案设计，包含以下内容：

- **项目定位**：Kubernetes 集群的 GPU 健康守门员，主动式故障隔离系统
- **部署形态**：DaemonSet + RBAC
- **适用硬件**：NVIDIA GPUs（主）、华为昇腾 NPUs（扩展支持）

#### 三级巡检流水线设计

| 级别 | 名称 | 频率 | 开销 | 检查内容 |
|------|------|------|------|----------|
| L1 | 被动式硬件状态检查 | 每30秒 | 极低 | NVML查询、XID错误扫描、僵尸进程检测 |
| L2 | 主动式微计算检查 | 每5分钟 | 毫秒级 | CUDA矩阵乘法测试（128x128） |
| L3 | PCIe带宽检查 | 每天 | 可选 | PCIe带宽测试 |

#### 健康状态机设计

```
HEALTHY → SUSPECTED → UNHEALTHY → ISOLATED
```

- 连续 N 次检测失败或命中致命 XID 触发状态转换
- 进入 UNHEALTHY 状态时执行：Cordon、Taint、Alert

#### 致命 XID 错误码定义

- XID 31: MMU Fault
- XID 43: GPU stopped processing
- XID 48: Double Bit ECC
- XID 79: Fallen off the bus（掉卡）

### 2. 项目规范制定 (2026-01-18)

完成了 CLAUDE.md 规范文档，定义了：

- 语言约定（交流/文档使用中文，代码使用英文）
- 发布约定（/release 目录结构）
- 文档约定（进展/完成/验收文档管理）
- LLM Friendly 设计原则

---

## 待实现功能清单

### 核心模块

| 模块 | 优先级 | 状态 | 说明 |
|------|--------|------|------|
| 项目骨架搭建 | P0 | 未开始 | Rust 项目结构、Cargo.toml |
| DeviceInterface 抽象层 | P0 | 未开始 | 设备操作接口定义 |
| NVIDIA 设备实现 | P0 | 未开始 | NVML 绑定、状态查询 |
| L1 被动检测 | P0 | 未开始 | XID 扫描、温度/功率监控 |
| L2 主动检测 | P0 | 未开始 | gpu_check.cu 二进制 |
| 健康状态机 | P0 | 未开始 | FSM 实现 |
| K8s 集成 | P0 | 未开始 | Node Taint/Cordon 操作 |

### 扩展模块

| 模块 | 优先级 | 状态 | 说明 |
|------|--------|------|------|
| 华为昇腾 NPU 支持 | P1 | 未开始 | npu-smi 解析、AscendCL 检测 |
| L3 PCIe 带宽检测 | P2 | 未开始 | 带宽测试实现 |
| 自愈功能 | P2 | 未开始 | GPU 重置尝试 |
| Prometheus Metrics | P1 | 未开始 | 指标暴露 |

### 部署相关

| 模块 | 优先级 | 状态 | 说明 |
|------|--------|------|------|
| Dockerfile | P0 | 未开始 | 多阶段构建，目标 < 50MB |
| Helm Chart | P1 | 未开始 | 一键部署方案 |
| RBAC 配置 | P0 | 未开始 | Node 操作权限 |
| ConfigMap 模板 | P1 | 未开始 | 配置示例 |

---

## 当前阻塞项

无

---

## 下一步计划

1. 搭建 Rust 项目骨架
2. 定义 DeviceInterface trait
3. 实现 NVIDIA 设备的 NVML 绑定
4. 编写 gpu_check.cu 微检测二进制

---

## 参考文档

- `/README.md` - 详细技术方案设计
- `/CLAUDE.md` - 项目开发规范
