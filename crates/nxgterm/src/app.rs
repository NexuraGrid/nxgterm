//! Window, event loop and the glue between pty, terminal and renderer.

use std::env;
use std::error::Error;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use nxg_config::{Backend, Bindings, Config, FontConfig};
use nxg_core::fallback::{self, Init};
use nxg_core::mouse::{MouseAction, MouseButton, MouseEvent};
use nxg_core::ports::{ChildProcess, PtyControl, RenderError, Renderer};
use nxg_core::{CellPixels, TermSize, Terminal, WinSize};
use nxg_pty::ShellCommand;
use nxg_render::{
    CellSize, CpuWindowRenderer, FontError, FontFaces, GpuRenderer, Layout, Style, WindowRenderer,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::bindings::Action;
use crate::mouse::{ViewportScroll, Wheel, WheelAction};
use crate::{appearance, bindings, choice, keys, mouse, reload};

/// Events posted to the event loop from background threads.
#[derive(Debug)]
pub enum UserEvent {
    /// A chunk of child output.
    Output(Vec<u8>),
    /// The child exited or its output stream closed.
    Exited,
    /// The config file changed on disk.
    ConfigChanged,
}

/// Everything that exists once the window is up.
struct Session {
    window: Arc<Window>,
    renderer: Box<dyn WindowRenderer>,
    /// Font files of the configured family, rasterized at any size.
    faces: FontFaces,
    /// Current font size in points; differs from the config when zoomed.
    font_size: f32,
    /// Window scale factor the style was built for.
    scale: f64,
    /// Padding of the current style, in physical pixels.
    padding: u32,
    terminal: Terminal,
    pty: Box<dyn PtyControl>,
    /// Consecutive skipped frames; bounds retries so a surface that keeps
    /// failing (e.g. occluded) does not spin the event loop.
    skipped_frames: u8,
    /// Wheel movement not yet worth a whole line.
    wheel: Wheel,
    /// The cell under the pointer.
    pointer: (u16, u16),
    /// The button held down, for drag reports.
    held: Option<MouseButton>,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    /// Config file to reload on change; `None` when no location is known.
    config_path: Option<PathBuf>,
    session: Option<Session>,
    /// Key bindings from the config, resolved for this platform;
    /// [`Bindings::shortcuts`] lists every action with its shortcut.
    bindings: Bindings,
    modifiers: ModifiersState,
    error: Option<Box<dyn Error>>,
}

/// Cmd instead of Ctrl for the default zoom bindings, and in shortcut
/// labels.
const MACOS: bool = cfg!(target_os = "macos");

impl App {
    pub fn new(
        proxy: EventLoopProxy<UserEvent>,
        config: Config,
        config_path: Option<PathBuf>,
    ) -> Self {
        let bindings = config.keybindings.resolve(MACOS);
        Self {
            proxy,
            config,
            config_path,
            session: None,
            bindings,
            modifiers: ModifiersState::empty(),
            error: None,
        }
    }

    /// The fatal error that stopped the event loop, if any.
    pub fn take_error(&mut self) -> Option<Box<dyn Error>> {
        self.error.take()
    }

    fn start(&self, event_loop: &ActiveEventLoop) -> Result<Session, Box<dyn Error>> {
        let config = &self.config;
        let attributes = Window::default_attributes().with_title("nxgterm");
        let window = Arc::new(event_loop.create_window(attributes)?);
        let scale = window.scale_factor();
        let faces = load_faces(&config.font)?;
        let style = build_style(&faces, config, config.font.size, scale)?;

        let initial = TermSize::new(config.window.columns.get(), config.window.rows.get())?;
        let (width, height) = style.layout().window_size(initial);
        // May be ignored (tiling window managers) or applied later through
        // a `Resized` event; the grid follows the actual size either way.
        let _ = window.request_inner_size(PhysicalSize::new(width, height));

        let padding = style.padding;
        let mut renderer = select_renderer(&window, &style, config.renderer.backend)?;
        let pixels = window.inner_size();
        renderer.resize(pixels.width, pixels.height);
        let layout = layout(renderer.as_ref(), padding);
        let size = layout.grid_size(pixels.width, pixels.height);
        let cell = CellPixels::new(layout.cell.width, layout.cell.height);

        let shell = config.shell.program.clone().map(|program| ShellCommand {
            program,
            args: config.shell.args.clone(),
        });
        let win_size = WinSize {
            cells: size,
            cell: Some(cell),
        };
        let backends = nxg_pty::backends_from_env(env::var(nxg_pty::ENV_VAR).ok().as_deref());
        let pty = nxg_pty::spawn_shell_with(win_size, shell.as_ref(), backends)?;
        for attempt in &pty.skipped {
            eprintln!("nxgterm: skipped {}: {}", attempt.name, attempt.error);
        }
        eprintln!("nxgterm: pty {}", pty.name);
        let pty = pty.backend;
        spawn_reader(pty.reader, self.proxy.clone());
        spawn_waiter(pty.child, self.proxy.clone());

        let mut terminal = Terminal::new(size);
        terminal.set_cell_pixels(cell.width, cell.height);
        terminal.set_scrollback_limit(config.scrollback.lines);
        Ok(Session {
            window,
            renderer,
            faces,
            font_size: config.font.size,
            scale,
            padding,
            terminal,
            skipped_frames: 0,
            pty: pty.control,
            wheel: Wheel::default(),
            pointer: (0, 0),
            held: None,
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Box<dyn Error>) {
        self.error = Some(error);
        event_loop.exit();
    }

    /// Handles a key press: a bound action, or bytes for the pty.
    fn key_pressed(&mut self, event: &KeyEvent) {
        let action = bindings::resolve(&self.bindings, &event.logical_key, self.modifiers);
        if let Some(action) = action {
            if self.perform(action) {
                return;
            }
        }
        let Some(session) = &mut self.session else {
            return;
        };
        let modes = session.terminal.modes();
        let bytes = keys::encode(
            &event.logical_key,
            event.text.as_deref(),
            self.modifiers,
            modes,
        );
        if let Some(bytes) = bytes {
            // Typing shows what is being typed into.
            session.scroll_viewport(ViewportScroll::Bottom);
            session.send(&bytes);
        }
    }

    /// Runs `action`. Returns `false` when it does not apply right now and
    /// the key should go to the pty instead (scrolling on the alternate
    /// screen, which has no history).
    fn perform(&mut self, action: Action) -> bool {
        if action == Action::ReloadConfig {
            self.reload_config();
            return true;
        }
        let config = &self.config;
        let Some(session) = &mut self.session else {
            return true;
        };
        if let Some(size) = appearance::zoom(action, session.font_size, config.font.size) {
            session.font_size = size;
            session.restyle(config);
            return true;
        }
        let modes = session.terminal.modes();
        let rows = session.terminal.size().rows();
        if let Some(scroll) = mouse::viewport_scroll(action, modes, rows) {
            session.scroll_viewport(scroll);
            return true;
        }
        match action {
            Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollToTop
            | Action::ScrollToBottom => false,
            // Tabs and the command palette are not built yet. Their keys
            // are still consumed so the bindings behave the same once they
            // are, and never leak to the shell in the meantime.
            Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PreviousTab
            | Action::GotoTab(_)
            | Action::CommandPalette => true,
            // Handled above.
            Action::ZoomIn | Action::ZoomOut | Action::ResetZoom | Action::ReloadConfig => true,
        }
    }

    /// Reloads the config file, applying what can change live. An invalid
    /// file keeps the previous config.
    fn reload_config(&mut self) {
        let Some(path) = &self.config_path else {
            return;
        };
        let new = match Config::load(path) {
            Ok(Some(config)) => config,
            Ok(None) => Config::default(),
            Err(error) => {
                eprintln!("nxgterm: {error}; keeping the previous config");
                return;
            }
        };
        let changes = reload::diff(&self.config, &new);
        if changes == reload::Changes::default() {
            return;
        }
        for what in &changes.on_restart {
            eprintln!("nxgterm: {what} changes apply on restart");
        }
        if changes.keybindings {
            self.bindings = new.keybindings.resolve(MACOS);
        }
        if let Some(session) = &mut self.session {
            if changes.font_faces {
                match load_faces(&new.font) {
                    Ok(faces) => session.faces = faces,
                    Err(error) => eprintln!("nxgterm: {error}; keeping the previous font"),
                }
            }
            if changes.font_size {
                session.font_size = new.font.size;
            }
            if changes.restyle {
                session.restyle(&new);
            }
            if changes.scrollback {
                session.terminal.set_scrollback_limit(new.scrollback.lines);
                session.window.request_redraw();
            }
        }
        eprintln!("nxgterm: config reloaded");
        self.config = new;
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
            UserEvent::ConfigChanged => self.reload_config(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            if event.state == ElementState::Pressed {
                self.key_pressed(event);
            }
            return;
        }
        let config = &self.config;
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
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // Re-rasterize at the new pixel size; the `Resized` event
                // that follows adjusts the surface.
                session.scale = scale_factor;
                session.restyle(config);
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::MouseWheel { delta, .. } => {
                let cell = layout(session.renderer.as_ref(), session.padding).cell;
                let lines = session.wheel.lines(delta, cell.height);
                let modes = session.terminal.modes();
                match mouse::wheel_action(modes, self.modifiers.shift_key(), lines) {
                    Some(WheelAction::Report { button, count }) => {
                        let (col, row) = session.pointer;
                        let event = MouseEvent {
                            button,
                            action: MouseAction::Press,
                            col,
                            row,
                            mods: mouse::mouse_mods(self.modifiers),
                        };
                        if let Some(bytes) = nxg_core::mouse::encode(event, modes.mouse_encoding) {
                            session.send(&bytes.repeat(count as usize));
                        }
                    }
                    Some(WheelAction::Keys(bytes)) => session.send(&bytes),
                    Some(WheelAction::Scroll(lines)) => {
                        session.scroll_viewport(ViewportScroll::Lines(lines));
                    }
                    None => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let layout = layout(session.renderer.as_ref(), session.padding);
                let size = session.terminal.size();
                let cell = mouse::cell_at(layout, size, position.x, position.y);
                if cell == session.pointer {
                    return;
                }
                session.pointer = cell;
                let event = MouseEvent {
                    button: session.held.unwrap_or(MouseButton::None),
                    action: MouseAction::Motion,
                    col: cell.0,
                    row: cell.1,
                    mods: mouse::mouse_mods(self.modifiers),
                };
                let modes = session.terminal.modes();
                let shift = self.modifiers.shift_key();
                let held = session.held.is_some();
                if let Some(bytes) = mouse::button_report(modes, shift, event, held) {
                    session.send(&bytes);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = mouse::button(button) else {
                    return;
                };
                let action = mouse::action(state);
                session.held = (action == MouseAction::Press).then_some(button);
                let (col, row) = session.pointer;
                let event = MouseEvent {
                    button,
                    action,
                    col,
                    row,
                    mods: mouse::mouse_mods(self.modifiers),
                };
                let modes = session.terminal.modes();
                let shift = self.modifiers.shift_key();
                if let Some(bytes) = mouse::button_report(modes, shift, event, false) {
                    session.send(&bytes);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = session.redraw(config) {
                    self.fail(event_loop, error);
                }
            }
            _ => {}
        }
    }
}

const MAX_FRAME_RETRIES: u8 = 3;

impl Session {
    /// Writes input for the child.
    fn send(&mut self, bytes: &[u8]) {
        if let Err(error) = self.pty.write_all(bytes) {
            eprintln!("nxgterm: pty write failed: {error}");
        }
    }

    /// Moves the scrollback viewport, redrawing when it changed.
    fn scroll_viewport(&mut self, scroll: ViewportScroll) {
        let before = self.terminal.display_offset();
        match scroll {
            ViewportScroll::Lines(lines) => self.terminal.scroll_display(lines),
            ViewportScroll::Top => self.terminal.scroll_display(i32::MAX),
            ViewportScroll::Bottom => self.terminal.scroll_display_to_bottom(),
        }
        if self.terminal.display_offset() != before {
            self.window.request_redraw();
        }
    }

    fn redraw(&mut self, config: &Config) -> Result<(), Box<dyn Error>> {
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
                self.fall_back_to_cpu(config)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Replaces a failed renderer with the CPU one and redraws.
    fn fall_back_to_cpu(&mut self, config: &Config) -> Result<(), Box<dyn Error>> {
        // Release the failed renderer's surface before attaching a new one:
        // some platforms do not allow two presenters on one window.
        drop(std::mem::replace(&mut self.renderer, Box::new(Detached)));
        let style = self.style(config)?;
        self.padding = style.padding;
        self.renderer = cpu_renderer(self.window.clone(), style)?;
        let pixels = self.window.inner_size();
        self.renderer.resize(pixels.width, pixels.height);
        eprintln!("nxgterm: renderer cpu");
        self.sync_grid_size();
        self.window.request_redraw();
        Ok(())
    }

    /// The style for the current font size, scale and `config`.
    fn style(&self, config: &Config) -> Result<Style, FontError> {
        build_style(&self.faces, config, self.font_size, self.scale)
    }

    /// Rebuilds the renderer style (font size, scale, colors or padding
    /// changed) and refits the grid to the window.
    fn restyle(&mut self, config: &Config) {
        match self.style(config) {
            Ok(style) => {
                self.padding = style.padding;
                self.renderer.set_style(style);
                self.sync_grid_size();
                self.window.request_redraw();
            }
            Err(error) => eprintln!("nxgterm: cannot apply font: {error}"),
        }
    }

    /// Resizes the terminal and the pty to what fits the window with the
    /// active renderer's cell size and the padding, and tells both the
    /// cell size in pixels (images and size reports depend on it).
    fn sync_grid_size(&mut self) {
        let pixels = self.window.inner_size();
        let layout = layout(self.renderer.as_ref(), self.padding);
        let size = layout.grid_size(pixels.width, pixels.height);
        let cell = CellPixels::new(layout.cell.width, layout.cell.height);
        if size == self.terminal.size() && cell == self.terminal.cell_pixels() {
            return;
        }
        if size != self.terminal.size() {
            self.terminal.resize(size);
        }
        self.terminal.set_cell_pixels(cell.width, cell.height);
        let win_size = WinSize {
            cells: size,
            cell: Some(cell),
        };
        if let Err(error) = self.pty.resize(win_size) {
            eprintln!("nxgterm: pty resize failed: {error}");
        }
    }
}

/// The renderer's cells inset by `padding` pixels. Takes the subtrait\n/// object directly: upcasting to `&dyn Renderer` needs Rust 1.86.
fn layout(renderer: &dyn WindowRenderer, padding: u32) -> Layout {
    let (width, height) = renderer.cell_size();
    Layout {
        cell: CellSize { width, height },
        padding,
        top: 0,
    }
}

/// Font files for the configured family and fallbacks, reporting the
/// ones that are not installed and the fallback faces in use.
fn load_faces(font: &FontConfig) -> Result<FontFaces, FontError> {
    let faces = FontFaces::system(font.family.as_deref(), &font.fallback)?;
    if let Some(family) = &font.family {
        if !faces.is_family(family) {
            eprintln!(
                "nxgterm: font family `{family}` not found; using `{}`",
                faces.family()
            );
        }
    }
    let found = faces.fallback_families();
    for family in &font.fallback {
        let used = faces.is_family(family) || found.iter().any(|f| f.eq_ignore_ascii_case(family));
        if !used {
            eprintln!("nxgterm: fallback font family `{family}` not found");
        }
    }
    if !found.is_empty() {
        eprintln!("nxgterm: fallback fonts {}", found.join(", "));
    }
    Ok(faces)
}

/// Font at `font_size` points, colors and padding from `config`, all at
/// the window `scale`.
fn build_style(
    faces: &FontFaces,
    config: &Config,
    font_size: f32,
    scale: f64,
) -> Result<Style, FontError> {
    Ok(Style {
        font: faces.font(appearance::font_px(font_size, scale))?,
        palette: appearance::palette(&config.colors.resolve()),
        padding: appearance::padding_px(config.window.padding, scale),
    })
}

/// Picks the first renderer that starts, in the order given by
/// `NXGTERM_RENDERER` or the configured backend, and logs the choice.
fn select_renderer(
    window: &Arc<Window>,
    style: &Style,
    backend: Backend,
) -> Result<Box<dyn WindowRenderer>, Box<dyn Error>> {
    let order = choice::renderer_order(env::var(choice::ENV_VAR).ok().as_deref(), backend);
    let candidates = order
        .iter()
        .map(|&name| {
            let (window, style) = (window.clone(), style.clone());
            let init: Init<'_, Box<dyn WindowRenderer>, Box<dyn Error>> = match name {
                "gpu" => Box::new(move || gpu_renderer(window, style)),
                _ => Box::new(move || cpu_renderer(window, style)),
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

fn gpu_renderer(
    window: Arc<Window>,
    style: Style,
) -> Result<Box<dyn WindowRenderer>, Box<dyn Error>> {
    let pixels = window.inner_size();
    // A driver or wgpu panic during setup must not take the terminal down.
    let renderer = panic::catch_unwind(AssertUnwindSafe(|| {
        GpuRenderer::new(window, pixels.width, pixels.height, style)
    }))
    .map_err(|_| "panicked during initialization")??;
    eprintln!("nxgterm: gpu adapter {}", renderer.adapter());
    Ok(Box::new(renderer))
}

fn cpu_renderer(
    window: Arc<Window>,
    style: Style,
) -> Result<Box<dyn WindowRenderer>, Box<dyn Error>> {
    Ok(Box::new(CpuWindowRenderer::new(window, style)?))
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

impl WindowRenderer for Detached {
    fn set_style(&mut self, _style: Style) {}

    fn draw_with_header(
        &mut self,
        _header: Option<&Terminal>,
        terminal: &Terminal,
    ) -> Result<(), RenderError> {
        self.draw(terminal)
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
