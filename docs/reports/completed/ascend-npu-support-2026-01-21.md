# 华为昇腾 NPU 支持实现进展

**更新日期：** 2026-01-21
**状态：** 已完成

---

## 实现概述

为 GDND 项目添加了完整的华为昇腾 NPU 支持，包括设备检测、指标采集、错误监控和主动健康检测。

---

## 完成的工作

### 1. 核心设备实现 (`ascend.rs`)

**文件：** `src/rust/gdnd/gdnd-core/src/device/ascend.rs`

实现了 `AscendDevice` 结构体，完整实现 `DeviceInterface` trait：

| 方法 | 功能 | 实现方式 |
|------|------|----------|
| `list_devices()` | 列出所有 NPU | 解析 `npu-smi info` 输出 |
| `get_metrics()` | 获取设备指标 | 解析 npu-smi 输出：温度、功率、AICore利用率、HBM使用量 |
| `get_xid_errors()` | 获取错误信息 | 解析 `/var/log/npu/slog/device-os-{id}/` 日志 + 健康状态 |
| `check_zombie_processes()` | 检查僵尸进程 | 解析 `npu-smi info -t usages` + `/proc/{pid}/stat` |
| `run_active_check()` | 主动健康检测 | 调用 `npu-check` 二进制 |
| `run_pcie_test()` | PCIe 带宽测试 | 调用 `npu-check --pcie-test` |

**错误码映射：**
| 错误码 | 名称 | 致命性 |
|-------|------|--------|
| 1001 | HbmError | Fatal |
| 1002 | AiCoreHang | Fatal |
| 1003 | OverTemperature | Warning |
| 1005 | PcieLinkError | Fatal |
| 1007 | DeviceLost | Fatal |
| 1008 | EccUncorrectable | Fatal |

**npu-smi 输出解析：**
支持解析以下格式的输出：
```
| NPU     Name              | Health        | Power(W)    Temp(C)    ...
| Chip                      | Bus-Id        | AICore(%)   Memory-Usage(MB)   HBM-Usage(MB)
+===========================+===============+====================================================+
| 0       910B3             | OK            | 112.5       37         0 / 0
| 0                         | 0000:C1:00.0  | 6           0 / 0              33551/ 65536
```

### 2. 工厂函数更新 (`mod.rs`)

**文件：** `src/rust/gdnd/gdnd-core/src/device/mod.rs`

更新了 `create_device_interface()` 函数：
- `DeviceType::Auto`: 先尝试 NVIDIA → 再尝试 Ascend → 最后 MockDevice
- `DeviceType::Ascend`: 直接创建 AscendDevice

### 3. 主动检测程序 (`npu_check.cpp`)

**文件：** `src/rust/gdnd/npu-check/npu_check.cpp`

使用 AscendCL API 实现的微基准测试：
- 内存分配和拷贝测试（Host → Device → Device → Host）
- PCIe 带宽测试（64MB 数据 H2D/D2H）
- 超时检测（SIGALRM）

**退出码：**
- 0: 健康
- 1: AscendCL 错误
- 2: 结果验证失败
- 3: 超时

**命令行选项：**
```
-d <device_id>    指定设备 ID（默认：0）
-t <seconds>      超时时间（默认：5s）
-v                详细输出
--pcie-test       运行 PCIe 带宽测试
```

### 4. 构建脚本 (`build.sh`)

**文件：** `src/rust/gdnd/npu-check/build.sh`

- 自动检测 CANN Toolkit 路径
- 支持 debug/release 构建模式
- 验证 AscendCL 头文件和库

### 5. 配置文件更新 (`config.yaml`)

**文件：** `src/rust/gdnd/configs/config.yaml`

新增配置项：
```yaml
npu_check_path: /usr/local/bin/npu-check

health:
  fatal_ascend_errors:
    - 1001  # HBM memory error
    - 1002  # AI Core hang
    - 1007  # Device lost
    - 1008  # ECC uncorrectable error

ascend:
  npu_smi_path: /usr/local/bin/npu-smi
  log_dir: /var/log/npu/slog

isolation:
  ascend_taint_key: huawei.com/npu-health
  ascend_taint_value: failed
  ascend_taint_effect: NoSchedule
```

### 6. Dockerfile (`Dockerfile.ascend`)

**文件：** `release/rust/gdnd/deploy/Dockerfile.ascend`

三阶段构建：
1. `rust-builder`: 编译 Rust 二进制
2. `ascend-builder`: 编译 npu-check（使用 CANN Toolkit）
3. `ascend-infer`: 最终运行时镜像

---

## 测试结果

### 单元测试
```
running 27 tests
test device::ascend::tests::test_ascend_error_codes ... ok
test device::ascend::tests::test_health_status_mapping ... ok
test device::ascend::tests::test_parse_npu_smi_output ... ok
test device::ascend::tests::test_parse_device_metrics ... ok
... (所有测试通过)

test result: ok. 27 passed; 0 failed
```

### 新增测试用例
- `test_ascend_error_codes`: 错误码转换和致命性判断
- `test_parse_npu_smi_output`: npu-smi 输出解析
- `test_parse_device_metrics`: 指标提取
- `test_health_status_mapping`: 健康状态映射

---

## 文件清单

### 新增文件
| 文件 | 行数 | 描述 |
|------|------|------|
| `gdnd-core/src/device/ascend.rs` | ~550 | AscendDevice 核心实现 |
| `npu-check/npu_check.cpp` | ~330 | AscendCL 微基准测试 |
| `npu-check/build.sh` | ~100 | 构建脚本 |
| `deploy/Dockerfile.ascend` | ~100 | 昇腾专用 Dockerfile |

### 修改文件
| 文件 | 修改内容 |
|------|----------|
| `gdnd-core/src/device/mod.rs` | 添加 ascend 模块导出和工厂函数更新 |
| `configs/config.yaml` | 添加昇腾配置项 |

---

## 依赖说明

### 运行时依赖
- CANN Toolkit 8.0+ (华为昇腾计算架构)
- npu-smi 工具 (随驱动安装)
- AscendCL 运行时库

### 构建依赖
- CANN Toolkit (含 AscendCL SDK)
- g++ (C++11)
- Rust 1.75+

---

## 部署说明

### 使用 NVIDIA GPU
```bash
docker build -t gdnd:nvidia -f release/rust/gdnd/deploy/Dockerfile src/rust/gdnd
```

### 使用 Ascend NPU
```bash
docker build -t gdnd:ascend -f release/rust/gdnd/deploy/Dockerfile.ascend src/rust/gdnd
```

### Helm 部署（昇腾节点）
```yaml
nodeSelector:
  huawei.com/npu: "true"

tolerations:
  - key: "huawei.com/npu"
    operator: "Exists"
    effect: "NoSchedule"
```

---

## 后续优化建议

1. **配置扩展**: 将 `fatal_error_codes` 和 `log_dir` 从配置文件读取，支持运行时配置
2. **日志解析优化**: 使用更精确的时间戳解析，而非当前时间
3. **集成测试**: 在真实昇腾硬件环境进行端到端测试
4. **CANN 版本兼容**: 测试不同 CANN 版本的兼容性

---

**完成人：** Claude Code
**完成日期：** 2026-01-21
