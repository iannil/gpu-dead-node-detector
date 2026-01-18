//! Device abstraction layer
//!
//! Provides a unified interface for different GPU/NPU devices.

mod interface;
mod mock;
mod nvidia;

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
            // Try NVIDIA first, then fall back to mock
            match NvidiaDevice::new() {
                Ok(device) => {
                    tracing::info!("Auto-detected NVIDIA device");
                    Ok(Arc::new(device))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "NVIDIA device not available, using mock");
                    Ok(Arc::new(MockDevice::new()))
                }
            }
        }
        DeviceType::Nvidia => {
            let device = NvidiaDevice::new()?;
            Ok(Arc::new(device))
        }
        DeviceType::Ascend => {
            // TODO: Implement Ascend NPU support
            tracing::warn!("Ascend NPU support not yet implemented, using mock");
            Ok(Arc::new(MockDevice::new()))
        }
    }
}
