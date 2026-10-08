//! The window renderer: a wgpu surface driven by the shared [`Painter`].

use std::fmt;

use nxg_core::Terminal;
use nxg_core::ports::{RenderError, Renderer};
use raw_window_handle::HasDisplayHandle;
use wgpu::CurrentSurfaceTexture;

use super::GpuError;
use super::device::Gpu;
use super::format;
use super::painter::Painter;
use crate::shape::Shape;
use crate::style::{Overlay, Style, WindowRenderer};

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
    /// fall back to the CPU renderer. When the style's background opacity
    /// is below 1.0 it asks for a surface the system blends with what is
    /// behind the window; [`WindowRenderer::translucent`] tells whether it
    /// got one (the window itself must have been created transparent).
    pub fn new<W>(window: W, width: u32, height: u32, style: Style) -> Result<Self, GpuError>
    where
        W: wgpu::WindowHandle + HasDisplayHandle + Clone + fmt::Debug + 'static,
    {
        // Asking for a blending surface only when the window starts
        // translucent keeps opaque windows exactly as they were.
        let translucent = style.background_opacity < 1.0;
        // The display handle lets EGL pick the right platform (Wayland).
        let mut descriptor =
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()));
        descriptor.backend_options.dx12.presentation_system =
            format::dx12_presentation(translucent);
        // The WGPU_* environment variables still override these choices.
        let instance = wgpu::Instance::new(descriptor.with_env());
        let surface = instance
            .create_surface(window)
            .map_err(|error| GpuError(format!("cannot create surface: {error}")))?;
        let gpu = Gpu::new(&instance, Some(&surface), false)?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = format::choose(&caps.formats)
            .ok_or_else(|| GpuError(format!("{}: no surface formats", gpu.describe())))?;
        let alpha_mode = format::alpha_mode(&caps.alpha_modes, translucent);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: 0,
            height: 0,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        };
        let mut painter = Painter::new(&gpu, format, style);
        painter.set_translucent(format::blends(alpha_mode));
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
        self.draw_layers(None, terminal, None, &[])
    }
}

impl WindowRenderer for GpuRenderer {
    fn set_style(&mut self, style: Style) {
        self.painter.set_style(style);
    }

    fn translucent(&self) -> bool {
        format::blends(self.config.alpha_mode)
    }

    fn draw_layers(
        &mut self,
        header: Option<&Terminal>,
        terminal: &Terminal,
        overlay: Option<Overlay<'_>>,
        shapes: &[Shape],
    ) -> Result<(), RenderError> {
        if let Some(failure) = self.gpu.failure() {
            return Err(RenderError::Fatal(failure));
        }
        if !self.visible() {
            return Ok(()); // Minimized.
        }
        let (frame, suboptimal) = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => (frame, false),
            CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            CurrentSurfaceTexture::Outdated => {
                self.configure();
                return Err(RenderError::Transient("surface outdated".into()));
            }
            CurrentSurfaceTexture::Lost => {
                self.configure();
                return Err(RenderError::Transient("surface lost".into()));
            }
            CurrentSurfaceTexture::Timeout => {
                return Err(RenderError::Transient("surface timeout".into()));
            }
            CurrentSurfaceTexture::Occluded => {
                return Err(RenderError::Transient("surface occluded".into()));
            }
            CurrentSurfaceTexture::Validation => {
                let failure = self.gpu.failure();
                return Err(RenderError::Fatal(
                    failure.unwrap_or_else(|| "surface validation error".into()),
                ));
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (width, height) = (self.config.width, self.config.height);
        self.painter.render(
            &self.gpu, &view, width, height, header, terminal, overlay, shapes,
        );
        self.gpu.queue.present(frame);
        if suboptimal {
            self.configure();
        }
        match self.gpu.failure() {
            Some(failure) => Err(RenderError::Fatal(failure)),
            None => Ok(()),
        }
    }
}
