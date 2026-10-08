//! Adapter and device setup plus failure tracking.

use std::sync::{Arc, Mutex, PoisonError};

use super::{GpuError, adapter};

/// A wgpu device and queue, recording the first asynchronous failure.
///
/// wgpu reports device loss and validation errors through callbacks (the
/// default error handler panics). Both are captured here so the renderer
/// can turn them into a fatal error and let the caller fall back.
#[derive(Debug)]
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    failure: Arc<Mutex<Option<String>>>,
}

impl Gpu {
    /// Picks the best adapter (see [`adapter::pick`]) that can present to
    /// `surface`, or any adapter when `surface` is `None`.
    pub fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        allow_software: bool,
    ) -> Result<Self, GpuError> {
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        let found: Vec<String> = adapters.iter().map(|a| describe(&a.get_info())).collect();
        let candidates = adapters
            .into_iter()
            .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
            .map(|a| {
                let device_type = a.get_info().device_type;
                (a, device_type)
            })
            .collect();
        let adapter = adapter::pick(candidates, allow_software).ok_or_else(|| {
            let found = if found.is_empty() {
                "none".to_owned()
            } else {
                found.join(", ")
            };
            GpuError(format!("no usable hardware adapter (found: {found})"))
        })?;

        let descriptor = wgpu::DeviceDescriptor {
            label: Some("nxgterm"),
            required_features: wgpu::Features::empty(),
            // GLES 3.0-level limits so old GPUs and GL drivers qualify.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
            .map_err(|error| GpuError(format!("{}: {error}", describe(&adapter.get_info()))))?;

        let failure = Arc::new(Mutex::new(None));
        let lost = Arc::clone(&failure);
        device.set_device_lost_callback(move |reason, message| {
            record(&lost, format!("device lost ({reason:?}): {message}"));
        });
        let uncaptured = Arc::clone(&failure);
        device.on_uncaptured_error(Arc::new(move |error| {
            record(&uncaptured, error.to_string());
        }));

        Ok(Self {
            adapter,
            device,
            queue,
            failure,
        })
    }

    /// The first device loss or uncaptured error, if any happened.
    pub fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn describe(&self) -> String {
        describe(&self.adapter.get_info())
    }
}

/// Keeps the first failure; later ones are usually consequences of it.
fn record(slot: &Mutex<Option<String>>, message: String) {
    slot.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert(message);
}

/// e.g. `Intel(R) UHD Graphics (Dx12, IntegratedGpu)`.
fn describe(info: &wgpu::AdapterInfo) -> String {
    format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
}
