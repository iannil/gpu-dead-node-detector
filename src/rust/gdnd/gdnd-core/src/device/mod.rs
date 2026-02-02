//! Device abstraction layer
//!
//! Provides a unified interface for different GPU/NPU devices.

mod ascend;
mod interface;
mod mock;
mod nvidia;

pub use ascend::AscendDevice;
pub use interface::*;
pub use mock::MockDevice;
pub use nvidia::NvidiaDevice;

use std::sync::Arc;

/// Create a device interface based on the device type
pub async fn create_device_interface(
    device_type: DeviceType,
) -> Result<Arc<dyn DeviceInterface>, DeviceError> {
    match device_type {
        DeviceType::Auto => {
            // Try NVIDIA first, then Ascend, then fall back to mock
            match NvidiaDevice::new() {
                Ok(device) => {
                    tracing::info!("Auto-detected NVIDIA device");
                    Ok(Arc::new(device))
                }
                Err(nvidia_err) => {
                    tracing::debug!(error = %nvidia_err, "NVIDIA device not available, trying Ascend");
                    match AscendDevice::new() {
                        Ok(device) => {
                            tracing::info!("Auto-detected Ascend NPU device");
                            Ok(Arc::new(device))
                        }
                        Err(ascend_err) => {
                            tracing::warn!(
                                nvidia_error = %nvidia_err,
                                ascend_error = %ascend_err,
                                "No GPU/NPU device available, using mock"
                            );
                            Ok(Arc::new(MockDevice::new()))
                        }
                    }
                }
            }
        }
        DeviceType::Nvidia => {
            let device = NvidiaDevice::new()?;
            Ok(Arc::new(device))
        }
        DeviceType::Ascend => {
            let device = AscendDevice::new()?;
            Ok(Arc::new(device))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_device_auto_fallback_to_mock() {
        // In CI/test environment without GPU, should fall back to MockDevice
        let device = create_device_interface(DeviceType::Auto)
            .await
            .expect("Should always return a device (at least mock)");

        // Verify it's a working device interface
        let devices = device.list_devices().await.unwrap();
        // MockDevice returns 2 mock devices
        assert!(
            !devices.is_empty(),
            "MockDevice should return at least one device"
        );
    }

    #[tokio::test]
    async fn test_create_device_nvidia_error_without_gpu() {
        // Should fail if NVIDIA is not available
        let result = create_device_interface(DeviceType::Nvidia).await;

        // In test environment without GPU, this should return Err
        match result {
            Ok(_) => {
                // If we're in an environment with GPU, test passes
            }
            Err(e) => {
                // Expected in test environment - should be NvmlInitError
                assert!(
                    matches!(e, DeviceError::NvmlInitError(_)),
                    "Expected NvmlInitError, got: {:?}",
                    e
                );
            }
        }
    }

    #[tokio::test]
    async fn test_create_device_ascend_error_without_npu() {
        // Should fail if Ascend is not available
        let result = create_device_interface(DeviceType::Ascend).await;

        // In test environment without NPU, this should return Err
        match result {
            Ok(_) => {
                // If we're in an environment with NPU, test passes
            }
            Err(e) => {
                // Expected in test environment - npu-smi not found
                assert!(
                    matches!(e, DeviceError::Other(_) | DeviceError::IoError(_)),
                    "Expected Other or IoError, got: {:?}",
                    e
                );
            }
        }
    }
}
