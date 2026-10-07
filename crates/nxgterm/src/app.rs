//! Window, event loop and the glue between pty, terminal and renderer.

use std::env;
use std::error::Error;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::thread;

use nxg_core::fallback::{self, Init};
use nxg_core::ports::{ChildProcess, PtyControl, RenderError, Renderer};
use nxg_core::{TermSize, Terminal};
use nxg_render::font::DEFAULT_PX;
use nxg_render::{CellSize, CpuWindowRenderer, Font, GpuRenderer, Palette};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::{choice, keys};

/// Events posted to the event loop from background threads.
#[derive(Debug)]
pub enum UserEvent {
    /// A chunk of child output.
    Output(Vec<u8>),
    /// The child exited or its output stream closed.
    Exited,
}

/// Everything that exists once the window is up.
struct Session {
    window: Arc<Window>,
    renderer: Box<dyn Renderer>,
    /// Font size in pixels, kept to rebuild a renderer on fallback.
    font_px: f32,
    terminal: Terminal,
    pty: Box<dyn PtyControl>,
    /// Consecutive skipped frames; bounds retries so a surface that keeps
    /// failing (e.g. occluded) does not spin the event loop.
    skipped_frames: u8,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    session: Option<Session>,
    modifiers: ModifiersState,
    error: Option<Box<dyn Error>>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            session: None,
            modifiers: ModifiersState::empty(),
            error: None,
        }
    }

    /// The fatal error that stopped the event loop, if any.
    pub fn take_error(&mut self) -> Option<Box<dyn Error>> {
        self.error.take()
    }

    fn start(&self, event_loop: &ActiveEventLoop) -> Result<Session, Box<dyn Error>> {
        let attributes = Window::default_attributes().with_title("nxgterm");
        let window = Arc::new(event_loop.create_window(attributes)?);

        // TODO(phase 4): font size from config; re-rasterize on scale changes.
        let font_px = DEFAULT_PX * window.scale_factor() as f32;
        let mut renderer = select_renderer(&window, font_px)?;
        let pixels = window.inner_size();
        renderer.resize(pixels.width, pixels.height);
        let size = grid_size(renderer.as_ref(), &window);

        let pty = nxg_pty::spawn_shell(size)?;
        spawn_reader(pty.reader, self.proxy.clone());
        spawn_waiter(pty.child, self.proxy.clone());

        Ok(Session {
            window,
            renderer,
            font_px,
            terminal: Terminal::new(size),
            skipped_frames: 0,
            pty: pty.control,
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Box<dyn Error>) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(session) => self.session = Some(session),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Output(bytes) => {
                if let Some(session) = &mut self.session {
                    session.terminal.advance(&bytes);
                    let responses = session.terminal.take_responses();
                    if !responses.is_empty() {
                        if let Err(error) = session.pty.write_all(&responses) {
                            eprintln!("nxgterm: pty write failed: {error}");
                        }
                    }
                    session.window.request_redraw();
                }
            }
            UserEvent::Exited => event_loop.exit(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(session) = &mut self.session else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(pixels) => {
                session.renderer.resize(pixels.width, pixels.height);
                session.sync_grid_size();
                session.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let bytes = keys::encode(&event.logical_key, event.text.as_deref(), self.modifiers);
                if let Some(bytes) = bytes {
                    if let Err(error) = session.pty.write_all(&bytes) {
                        eprintln!("nxgterm: pty write failed: {error}");
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = session.redraw() {
                    self.fail(event_loop, error);
                }
            }
            _ => {}
        }
    }
}

const MAX_FRAME_RETRIES: u8 = 3;

impl Session {
    fn redraw(&mut self) -> Result<(), Box<dyn Error>> {
        match self.renderer.draw(&self.terminal) {
            Ok(()) => {
                self.skipped_frames = 0;
                Ok(())
            }
            Err(RenderError::Transient(_)) => {
                // Retry a few times, then wait for the next event to redraw.
                if self.skipped_frames < MAX_FRAME_RETRIES {
                    self.skipped_frames += 1;
                    self.window.request_redraw();
                }
                Ok(())
            }
            Err(error) if self.renderer.name() != "cpu" => {
                eprintln!(
                    "nxgterm: {} renderer failed: {error}; falling back to cpu",
                    self.renderer.name()
                );
                self.fall_back_to_cpu()
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Replaces a failed renderer with the CPU one and redraws.
    fn fall_back_to_cpu(&mut self) -> Result<(), Box<dyn Error>> {
        // Release the failed renderer's surface before attaching a new one:
        // some platforms do not allow two presenters on one window.
        drop(std::mem::replace(&mut self.renderer, Box::new(Detached)));
        self.renderer = cpu_renderer(self.window.clone(), self.font_px)?;
        let pixels = self.window.inner_size();
        self.renderer.resize(pixels.width, pixels.height);
        eprintln!("nxgterm: renderer cpu");
        self.sync_grid_size();
        self.window.request_redraw();
        Ok(())
    }

    /// Resizes the terminal and the pty to what fits the window with the
    /// active renderer's cell size.
    fn sync_grid_size(&mut self) {
        let size = grid_size(self.renderer.as_ref(), &self.window);
        if size != self.terminal.size() {
            self.terminal.resize(size);
            if let Err(error) = self.pty.resize(size) {
                eprintln!("nxgterm: pty resize failed: {error}");
            }
        }
    }
}

/// Grid size that fits the window with `renderer`'s cells.
fn grid_size(renderer: &dyn Renderer, window: &Window) -> TermSize {
    let (width, height) = renderer.cell_size();
    let pixels = window.inner_size();
    CellSize { width, height }.grid_size(pixels.width, pixels.height)
}

/// Picks the first renderer that starts, in the order given by
/// `NXGTERM_RENDERER` (default: gpu, then cpu), and logs the choice.
fn select_renderer(
    window: &Arc<Window>,
    font_px: f32,
) -> Result<Box<dyn Renderer>, Box<dyn Error>> {
    let order = choice::renderer_order(env::var(choice::ENV_VAR).ok().as_deref());
    let candidates = order
        .iter()
        .map(|&name| {
            let window = window.clone();
            let init: Init<'_, Box<dyn Renderer>, Box<dyn Error>> = match name {
                "gpu" => Box::new(move || gpu_renderer(window, font_px)),
                _ => Box::new(move || cpu_renderer(window, font_px)),
            };
            (name, init)
        })
        .collect();
    match fallback::first_available(candidates) {
        Ok(selected) => {
            for attempt in &selected.skipped {
                eprintln!("nxgterm: skipped {}: {}", attempt.name, attempt.error);
            }
            eprintln!("nxgterm: renderer {}", selected.name);
            Ok(selected.backend)
        }
        Err(attempts) => {
            let reasons: Vec<String> = attempts
                .iter()
                .map(|attempt| format!("{}: {}", attempt.name, attempt.error))
                .collect();
            Err(format!("no renderer available ({})", reasons.join("; ")).into())
        }
    }
}

fn gpu_renderer(window: Arc<Window>, font_px: f32) -> Result<Box<dyn Renderer>, Box<dyn Error>> {
    let font = Font::system(font_px)?;
    let pixels = window.inner_size();
    // A driver or wgpu panic during setup must not take the terminal down.
    let renderer = panic::catch_unwind(AssertUnwindSafe(|| {
        GpuRenderer::new(
            window,
            pixels.width,
            pixels.height,
            font,
            Palette::default(),
        )
    }))
    .map_err(|_| "panicked during initialization")??;
    eprintln!("nxgterm: gpu adapter {}", renderer.adapter());
    Ok(Box::new(renderer))
}

fn cpu_renderer(window: Arc<Window>, font_px: f32) -> Result<Box<dyn Renderer>, Box<dyn Error>> {
    let font = Font::system(font_px)?;
    Ok(Box::new(CpuWindowRenderer::new(
        window,
        font,
        Palette::default(),
    )?))
}

/// Placeholder that holds no surface, used only while swapping renderers.
struct Detached;

impl Renderer for Detached {
    fn name(&self) -> &'static str {
        "none"
    }

    fn cell_size(&self) -> (u32, u32) {
        (1, 1)
    }

    fn resize(&mut self, _width: u32, _height: u32) {}

    fn draw(&mut self, _terminal: &Terminal) -> Result<(), RenderError> {
        Err(RenderError::Fatal("no renderer attached".into()))
    }
}

/// Drains child output on a background thread until EOF or error.
fn spawn_reader(mut reader: Box<dyn Read + Send>, proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if proxy
                        .send_event(UserEvent::Output(buf[..n].to_vec()))
                        .is_err()
                    {
                        return; // Event loop is gone.
                    }
                }
            }
        }
        let _ = proxy.send_event(UserEvent::Exited);
    });
}

/// Reports child exit; needed where the reader never sees EOF (ConPTY).
fn spawn_waiter(mut child: Box<dyn ChildProcess>, proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        let _ = child.wait();
        let _ = proxy.send_event(UserEvent::Exited);
    });
}
