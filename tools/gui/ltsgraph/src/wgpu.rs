use merc_utilities::MercError;
use wgpu::Instance;

/// Initializes wgpu and returns the device and queue.
pub async fn init_wgpu() -> Result<(wgpu::Device, wgpu::Queue), MercError> {
    let instance = Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|e| MercError::from(format!("Cannot find a suitable adapter: {e}")))?;

    // The vello compute shaders crash with an access violation on the DirectX 12 software rasterizer (WARP),
    // which is the only adapter available on machines without a GPU, such as the Windows CI runners.
    let info = adapter.get_info();
    if info.backend == wgpu::Backend::Dx12 && info.device_type == wgpu::DeviceType::Cpu {
        return Err(MercError::from(format!(
            "The software adapter '{}' is not supported, use a CPU based renderer instead",
            info.name
        )));
    }

    // The vello compute pipeline needs more resources than the conservative default limits provide.
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .map_err(|e| MercError::from(format!("Failed to create device: {e}")))
}
