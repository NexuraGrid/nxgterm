//! Window, event loop and the glue between pty, terminal and renderer.

use std::error::Error;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::thread;

use nxg_core::Terminal;
use nxg_core::ports::{ChildProcess, PtyControl};
use nxg_render::font::DEFAULT_PX;
use nxg_render::{CpuRenderer, Font, Frame, Palette};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::keys;

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
    surface: Surface<Arc<Window>, Arc<Window>>,
    renderer: CpuRenderer,
    terminal: Terminal,
    pty: Box<dyn PtyControl>,
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
        let context = Context::new(window.clone())?;
        let surface = Surface::new(&context, window.clone())?;

        // TODO(phase 4): font size from config; re-rasterize on scale changes.
        let px = DEFAULT_PX * window.scale_factor() as f32;
        let renderer = CpuRenderer::new(Font::system(px)?, Palette::default());
        let pixels = window.inner_size();
        let size = renderer.cell_size().grid_size(pixels.width, pixels.height);

        let pty = nxg_pty::spawn_shell(size)?;
        spawn_reader(pty.reader, self.proxy.clone());
        spawn_waiter(pty.child, self.proxy.clone());

        Ok(Session {
            window,
            surface,
            renderer,
            terminal: Terminal::new(size),
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
                let size = session
                    .renderer
                    .cell_size()
                    .grid_size(pixels.width, pixels.height);
                if size != session.terminal.size() {
                    session.terminal.resize(size);
                    if let Err(error) = session.pty.resize(size) {
                        eprintln!("nxgterm: pty resize failed: {error}");
                    }
                }
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

impl Session {
    fn redraw(&mut self) -> Result<(), Box<dyn Error>> {
        let pixels = self.window.inner_size();
        let (Some(width), Some(height)) = (
            NonZeroU32::new(pixels.width),
            NonZeroU32::new(pixels.height),
        ) else {
            return Ok(()); // Minimized.
        };
        self.surface.resize(width, height)?;
        let mut buffer = self.surface.buffer_mut()?;
        let mut frame = Frame::new(&mut buffer, width.get(), height.get())
            .ok_or("surface buffer smaller than the window")?;
        self.renderer.render(&self.terminal, &mut frame);
        buffer.present()?;
        Ok(())
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
