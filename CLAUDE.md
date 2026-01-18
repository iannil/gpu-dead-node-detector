# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在此代码仓库中工作时提供指导。

## 项目概述

GPU Dead Node Detector (GDND) 是一个基于 Kubernetes 的 GPU 节点主动式故障隔离系统。它以 DaemonSet 形式运行在所有 GPU 节点上，检测不健康的 GPU 并通过 Taint/Cordon 自动隔离故障节点。

**支持硬件：** NVIDIA GPUs（主要）、华为昇腾 NPUs（扩展支持）

## 项目指南

- 语言约定：交流与文档使用中文；生成的代码使用英文；文档放在 `docs` 且使用 Markdown。
- 发布约定：
  - 发布固定在 `/release` 文件夹，如 rust 服务固定发布在 `/release/rust` 文件夹。
  - 发布的成果物必须且始终以生产环境为标准，要包含所有发布生产所应该包含的文件或数据（包含全量发布与增量发布，首次发布与非首次发布）。
- 文档约定：
  - 每次修改都必须延续上一次的进展，每次修改的进展都必须保存在对应的 `docs` 文件夹下的文档中。
  - 执行修改过程中，进展随时保存文档，带上实际修改的时间，便于追溯修改历史。
  - 未完成的修改，文档保存在 `/docs/progress` 文件夹下。
  - 已完成的修改，文档保存在 `/docs/reports/completed` 文件夹下。
  - 对修改进行验收，文档保存在 `/docs/reports` 文件夹下。
  - 对重复的、冗余的、不能体现实际情况的文档或文档内容，要保持更新和调整。
  - 文档模板和命名规范可以参考 `/docs/standards` 和 `docs/templates` 文件夹下的内容。
- 数据约定：数据固定在`/data`文件夹下

### 面向大模型的可改写性（LLM Friendly）

- 一致的分层与目录：相同功能在各应用/包中遵循相同结构与命名，使检索与大范围重构更可控。
- 明确边界与单一职责：函数/类保持单一职责；公共模块暴露极少稳定接口；避免隐式全局状态。
- 显式类型与契约优先：导出 API 均有显式类型；运行时与编译时契约一致（zod schema 即类型源）。
- 声明式配置：将重要行为转为数据驱动（配置对象 + `as const`/`satisfies`），减少分支与条件散落。
- 可搜索性：统一命名（如 `parseXxx`、`assertNever`、`safeJsonParse`、`createXxxService`），降低 LLM 与人类的检索成本。
- 小步提交与计划：通过 `IMPLEMENTATION_PLAN.md` 和小步提交让模型理解上下文、意图与边界。
- 变更安全策略：批量程序性改动前先将原文件备份至 `/backup` 相对路径；若错误数异常上升，立即回滚备份。

## 架构

GDND 使用三级巡检流水线，开销逐级递增：

1. **被动检测 (L1)** - 高频（约30秒），开销极低
   - NVML 查询 GPU 状态、温度、功率
   - XID 错误扫描（致命错误：31, 43, 48, 79）
   - 僵尸进程检测（D 状态的 GPU 进程）

2. **主动微检测 (L2)** - 中频（约5分钟），毫秒级开销
   - 小型 CUDA 矩阵乘法（128x128），设置严格超时
   - 检测 nvidia-smi 无法发现的驱动死锁

3. **IO/压力检测 (L3)** - 低频（每天），可选
   - PCIe 带宽测试，检测链路退化

## 健康状态机

每个 GPU 维护状态：`HEALTHY` → `SUSPECTED` → `UNHEALTHY` → `ISOLATED`

当状态变为 `UNHEALTHY` 时：Cordon 节点、打上污点 `nvidia.com/gpu-health=failed:NoSchedule`、发送告警

## 实现指南

- 使用纯 C++/CUDA 编写微检测二进制文件（`gpu_check.cu`）- 避免 PyTorch 依赖地狱
- 目标镜像大小：Alpine + 二进制 < 50MB
- 通过 `DeviceInterface` 抽象设备操作，同时支持 NVIDIA 和昇腾 NPU
- 昇腾 NPU：解析 `npu-smi info`，监控 `/var/log/npu/slog/device-os`，使用 AscendCL 进行主动检测

## 配置

ConfigMap 中的关键配置项：

- `check_interval_seconds`、`failure_threshold`
- `fatal_xids` 列表
- `device_type`：auto、nvidia、ascend
- 动作标志：`cordon`、`evict_pods`、污点设置

## 部署

- 以 Kubernetes DaemonSet 运行，配合 RBAC 进行 Node 操作授权
- 暴露 Prometheus 指标：`gdnd_gpu_status{gpu="0"}`
- 提供 Helm Chart 一键安装
