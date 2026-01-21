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
