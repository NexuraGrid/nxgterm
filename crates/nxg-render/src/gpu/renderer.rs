//! The window renderer: a wgpu surface driven by the shared [`Painter`].

use nxg_core::Terminal;
use nxg_core::ports::{RenderError, Renderer};

use super::GpuError;
use super::device::Gpu;
use super::format;
use super::painter::Painter;
use crate::font::Font;
use crate::palette::Palette;

/// GPU renderer presenting to a window surface.
#[derive(Debug)]
pub struct GpuRenderer {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    painter: Painter,
}

impl GpuRenderer {
    /// Creates a surface for `window` (`width x height` pixels) on the best
    /// hardware adapter. Fails on software-only adapters so the caller can
    /// fall back to the CPU renderer.
    pub fn new<W>(
        window: W,
        width: u32,
        height: u32,
        font: Font,
        palette: Palette,
    ) -> Result<Self, GpuError>
    where
        W: wgpu::WindowHandle + 'static,
    {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = instance
            .create_surface(window)
            .map_err(|error| GpuError(format!("cannot create surface: {error}")))?;
        let gpu = Gpu::new(&instance, Some(&surface), false)?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = format::choose(&caps.formats)
            .ok_or_else(|| GpuError(format!("{}: no surface formats", gpu.describe())))?;
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto)
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 0,
            height: 0,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        };
        let painter = Painter::new(&gpu, format, font, palette);
        let mut renderer = Self {
            surface,
            config,
            gpu,
            painter,
        };
        renderer.resize(width, height);
        match renderer.gpu.failure() {
            Some(failure) => Err(GpuError(format!("{}: {failure}", renderer.gpu.describe()))),
            None => Ok(renderer),
        }
    }

    /// Adapter name, backend and type, for logs.
    pub fn adapter(&self) -> String {
        self.gpu.describe()
    }

    fn configure(&self) {
        self.surface.configure(&self.gpu.device, &self.config);
    }

    fn visible(&self) -> bool {
        self.config.width > 0 && self.config.height > 0
    }
}

impl Renderer for GpuRenderer {
    fn name(&self) -> &'static str {
        "gpu"
    }

    fn cell_size(&self) -> (u32, u32) {
        let cell = self.painter.cell_size();
        (cell.width, cell.height)
    }

    fn resize(&mut self, width: u32, height: u32) {
        let max = self.gpu.device.limits().max_texture_dimension_2d;
        self.config.width = width.min(max);
        self.config.height = height.min(max);
        if self.visible() {
            self.configure();
        }
    }

    fn draw(&mut self, terminal: &Terminal) -> Result<(), RenderError> {
        if let Some(failure) = self.gpu.failure() {
            return Err(RenderError::Fatal(failure));
        }
        if !self.visible() {
            return Ok(()); // Minimized.
        }
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(error @ (wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost)) => {
                self.configure();
                return Err(RenderError::Transient(error.to_string()));
            }
            Err(error @ wgpu::SurfaceError::Timeout) => {
                return Err(RenderError::Transient(error.to_string()));
            }
            Err(error) => return Err(RenderError::Fatal(error.to_string())),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (width, height) = (self.config.width, self.config.height);
        self.painter
            .render(&self.gpu, &view, width, height, terminal);
        let suboptimal = frame.suboptimal;
        frame.present();
        if suboptimal {
            self.configure();
        }
        match self.gpu.failure() {
            Some(failure) => Err(RenderError::Fatal(failure)),
            None => Ok(()),
        }
    }
}
