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

/// The wgpu backends to try, one instance each and in order, on `os`
/// (`std::env::consts::OS`). Each backend loads its driver libraries,
/// which stay resident, so only one is tried at a time: DX12, Vulkan then
/// GL on Windows, Metal on macOS, Vulkan then GL elsewhere. A non-empty
/// `WGPU_BACKEND` choice (`env`, see [`wgpu::Backends::from_env`]) is
/// used as is, in one instance.
pub fn backend_order(os: &str, env: Option<wgpu::Backends>) -> Vec<wgpu::Backends> {
    use wgpu::Backends;
    if let Some(chosen) = env.filter(|chosen| !chosen.is_empty()) {
        return vec![chosen];
    }
    match os {
        "windows" => vec![Backends::DX12, Backends::VULKAN, Backends::GL],
        "macos" | "ios" => vec![Backends::METAL],
        _ => vec![Backends::VULKAN, Backends::GL],
    }
}

/// The result of `connect` on the first of `order` that succeeds; later
/// backends are not tried. When all fail, the error lists each backend's
/// failure, in order.
pub fn first_connected<T, E: std::fmt::Display>(
    order: impl IntoIterator<Item = wgpu::Backends>,
    mut connect: impl FnMut(wgpu::Backends) -> Result<T, E>,
) -> Result<T, String> {
    let mut failures = Vec::new();
    for backends in order {
        match connect(backends) {
            Ok(connected) => return Ok(connected),
            Err(error) => failures.push(format!("{backends:?}: {error}")),
        }
    }
    Err(failures.join("; "))
}

impl Gpu {
    /// Picks the best adapter (see [`adapter::pick`]) of `backends` that
    /// can present to `surface`, or any adapter when `surface` is `None`.
    pub fn new(
        instance: &wgpu::Instance,
        backends: wgpu::Backends,
        surface: Option<&wgpu::Surface<'_>>,
        allow_software: bool,
    ) -> Result<Self, GpuError> {
        let adapters = pollster::block_on(instance.enumerate_adapters(backends));
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

#[cfg(test)]
mod order_tests {
    use wgpu::Backends;

    use super::{backend_order, first_connected};

    #[test]
    fn windows_tries_dx12_then_vulkan_then_gl() {
        let order = backend_order("windows", None);
        assert_eq!(order, [Backends::DX12, Backends::VULKAN, Backends::GL]);
    }

    #[test]
    fn macos_uses_metal_only() {
        assert_eq!(backend_order("macos", None), [Backends::METAL]);
    }

    #[test]
    fn linux_and_others_try_vulkan_then_gl() {
        for os in ["linux", "freebsd", "openbsd"] {
            assert_eq!(backend_order(os, None), [Backends::VULKAN, Backends::GL]);
        }
    }

    #[test]
    fn each_step_names_one_backend() {
        for os in ["windows", "macos", "linux"] {
            for backends in backend_order(os, None) {
                assert_eq!(backends.iter().count(), 1, "{os}: {backends:?}");
            }
        }
    }

    #[test]
    fn wgpu_backend_from_the_environment_wins() {
        let chosen = Backends::VULKAN | Backends::GL;
        assert_eq!(backend_order("windows", Some(chosen)), [chosen]);
        assert_eq!(backend_order("macos", Some(Backends::GL)), [Backends::GL]);
    }

    #[test]
    fn an_empty_environment_choice_falls_back_to_the_default_order() {
        // `WGPU_BACKEND=` or an unknown name parses to no backend at all.
        assert_eq!(
            backend_order("linux", Some(Backends::empty())),
            [Backends::VULKAN, Backends::GL]
        );
    }

    #[test]
    fn the_first_backend_that_connects_wins_and_later_ones_are_not_tried() {
        let mut tried = Vec::new();
        let order = [Backends::DX12, Backends::VULKAN, Backends::GL];
        let found = first_connected(order, |backends| {
            tried.push(backends);
            if backends == Backends::VULKAN {
                Ok("vulkan")
            } else {
                Err("no adapter")
            }
        });
        assert_eq!(found, Ok("vulkan"));
        assert_eq!(tried, [Backends::DX12, Backends::VULKAN]);
    }

    #[test]
    fn when_every_backend_fails_each_failure_is_reported_in_order() {
        let order = [Backends::DX12, Backends::GL];
        let found: Result<(), String> = first_connected(order, |backends| {
            Err(if backends == Backends::DX12 { "a" } else { "b" })
        });
        let error = found.unwrap_err();
        let expected = format!("{:?}: a; {:?}: b", Backends::DX12, Backends::GL);
        assert_eq!(error, expected);
    }
}
