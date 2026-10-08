//! Window, event loop and the glue between pty, terminal and renderer.

use std::env;
use std::error::Error;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use nxg_config::{Backend, Bindings, Config, FontConfig, TabBar};
use nxg_core::fallback::{self, Init};
use nxg_core::mouse::{MouseAction, MouseButton, MouseEvent};
use nxg_core::ports::{ChildProcess, Clipboard, ClipboardKind, PtyControl, RenderError, Renderer};
use nxg_core::selection::{Point, SelectionKind};
use nxg_core::{CellPixels, TermSize, Terminal, WinSize};
use nxg_pty::ShellCommand;
use nxg_render::{
    CellSize, CpuWindowRenderer, FontError, FontFaces, GpuRenderer, Layout, Overlay, Style,
    WindowRenderer,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::bindings::Action;
use crate::clipboard::HAS_PRIMARY;
use crate::command_palette::{self, CommandPalette, Outcome};
use crate::mouse::{Clicks, ViewportScroll, Wheel, WheelAction};
use crate::tabs::{TabId, Tabs};
use crate::{appearance, bindings, choice, clipboard, keys, mouse, reload, tab_bar};

/// Events posted to the event loop from background threads.
#[derive(Debug)]
pub enum UserEvent {
    /// A chunk of output from the child of a tab.
    Output(TabId, Vec<u8>),
    /// The child of a tab exited or its output stream closed.
    Exited(TabId),
    /// The config file changed on disk.
    ConfigChanged,
}

/// One tab: a child process on its own pty and the terminal it draws.
struct Tab {
    terminal: Terminal,
    pty: Box<dyn PtyControl>,
    /// Label on the tab bar: the program name (OSC 0/2 titles are not
    /// supported yet).
    title: String,
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
    /// Every tab has its own shell; only the active one is drawn and gets
    /// input, the others keep reading their output.
    tabs: Tabs<Tab>,
    /// When the tab bar shows; the grid is one row shorter while it does.
    tab_bar: TabBar,
    /// Consecutive skipped frames; bounds retries so a surface that keeps
    /// failing (e.g. occluded) does not spin the event loop.
    skipped_frames: u8,
    /// Wheel movement not yet worth a whole line.
    wheel: Wheel,
    /// The cell under the pointer.
    pointer: (u16, u16),
    /// The tab bar column under the pointer, while it is over the bar.
    bar_pointer: Option<u16>,
    /// The button held down, for drag reports.
    held: Option<MouseButton>,
    /// The command palette, while it is open: it takes the keys and the
    /// clicks, and is drawn over the grid.
    palette: Option<CommandPalette>,
    /// Left-button presses, for double and triple clicks.
    clicks: Clicks,
    /// The selection being dragged with the left button.
    drag: Option<Drag>,
}

/// A selection in the making: started by a left press, extended while the
/// pointer moves, finished on release. A single click selects nothing
/// until the pointer leaves the cell it pressed.
#[derive(Debug, Clone, Copy)]
struct Drag {
    kind: SelectionKind,
    anchor: Point,
    started: bool,
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
    /// The system clipboard, kept for the whole run (X11 and Wayland
    /// serve copied text from it).
    clipboard: Box<dyn Clipboard>,
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
            clipboard: clipboard::system(),
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
        let mut session = Session {
            window,
            renderer,
            faces,
            font_size: config.font.size,
            scale,
            padding,
            tabs: Tabs::new(),
            tab_bar: config.window.tab_bar,
            skipped_frames: 0,
            wheel: Wheel::default(),
            pointer: (0, 0),
            bar_pointer: None,
            held: None,
            palette: None,
            clicks: Clicks::default(),
            drag: None,
        };
        session.open_tab(config, &self.proxy)?;
        Ok(session)
    }

    /// Opens a tab running the configured shell next to the active one.
    fn new_tab(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        match session.open_tab(&self.config, &self.proxy) {
            Ok(_) => session.window.request_redraw(),
            Err(error) => eprintln!("nxgterm: cannot open a tab: {error}"),
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Box<dyn Error>) {
        self.error = Some(error);
        event_loop.exit();
    }

    /// Opens the command palette, or closes it when it is open.
    fn toggle_palette(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.palette = match session.palette {
            Some(_) => None,
            None => Some(CommandPalette::new(self.bindings.shortcuts())),
        };
        // A drag in progress ends here: the palette takes the mouse.
        session.held = None;
        session.drag = None;
        session.window.request_redraw();
    }

    /// Applies what the palette asked for after a key or a click.
    fn palette_done(&mut self, outcome: Outcome) {
        let Some(session) = &mut self.session else {
            return;
        };
        match outcome {
            Outcome::Ignore => {}
            Outcome::Redraw => session.window.request_redraw(),
            Outcome::Close => {
                session.palette = None;
                session.window.request_redraw();
            }
            Outcome::Run(action) => {
                session.palette = None;
                session.window.request_redraw();
                self.perform(action);
            }
        }
    }

    /// Handles a key press while the palette is open: nothing reaches the
    /// pty. Its own binding closes it.
    fn palette_key(&mut self, event: &KeyEvent) {
        let action = bindings::resolve(&self.bindings, &event.logical_key, self.modifiers);
        if action == Some(Action::CommandPalette) {
            self.toggle_palette();
            return;
        }
        let input =
            command_palette::input(&event.logical_key, event.text.as_deref(), self.modifiers);
        let Some(input) = input else {
            return;
        };
        let Some(session) = &mut self.session else {
            return;
        };
        let rows = session.palette_rect().map_or(0, |rect| rect.list_rows());
        let Some(palette) = &mut session.palette else {
            return;
        };
        let outcome = palette.handle(input, rows);
        self.palette_done(outcome);
    }

    /// Handles a mouse event while the palette is open, so it does not
    /// reach the terminal: the wheel moves the selection, a left click on
    /// a row runs it and one outside the box closes it. `None` when the
    /// palette is closed or the event is not a mouse event.
    fn palette_mouse(&mut self, event: &WindowEvent) -> Option<Outcome> {
        let session = self.session.as_mut()?;
        session.palette.as_ref()?;
        let rect = session.palette_rect()?;
        let outcome = match *event {
            WindowEvent::MouseWheel { delta, .. } => {
                let cell = layout(session.renderer.as_ref(), session.padding).cell;
                let lines = session.wheel.lines(delta, cell.height);
                let palette = session.palette.as_mut()?;
                // Positive lines scroll back: up the list.
                palette.scroll_by(-lines, rect.list_rows())
            }
            WindowEvent::CursorMoved { position, .. } => {
                session.track_pointer(position.x, position.y);
                Outcome::Ignore
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                let (col, row) = session.pointer;
                let palette = session.palette.as_mut()?;
                if session.bar_pointer.is_some() {
                    Outcome::Close
                } else {
                    palette.click(rect, col, row)
                }
            }
            WindowEvent::MouseInput { .. } => Outcome::Ignore,
            _ => return None,
        };
        Some(outcome)
    }

    /// Handles a key press: a bound action, or bytes for the pty.
    fn key_pressed(&mut self, event: &KeyEvent) {
        if self.session.as_ref().is_some_and(|s| s.palette.is_some()) {
            self.palette_key(event);
            return;
        }
        let action = bindings::resolve(&self.bindings, &event.logical_key, self.modifiers);
        if let Some(action) = action {
            if self.perform(action) {
                return;
            }
        }
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(tab) = session.tabs.active_mut() else {
            return;
        };
        let modes = tab.terminal.modes();
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
        match action {
            Action::ReloadConfig => {
                self.reload_config();
                return true;
            }
            Action::NewTab => {
                self.new_tab();
                return true;
            }
            Action::CommandPalette => {
                self.toggle_palette();
                return true;
            }
            Action::Copy => {
                self.copy();
                return true;
            }
            Action::Paste => {
                self.paste(ClipboardKind::Clipboard);
                return true;
            }
            Action::SelectAll => {
                self.select_all();
                return true;
            }
            _ => {}
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
        let tabs = &mut session.tabs;
        match action {
            Action::CloseTab => {
                let index = tabs.active_index();
                session.close_tab(index);
                return true;
            }
            Action::NextTab => tabs.next(),
            Action::PreviousTab => tabs.previous(),
            Action::GotoTab(n) => tabs.goto(n),
            _ => {}
        }
        if matches!(
            action,
            Action::NextTab | Action::PreviousTab | Action::GotoTab(_)
        ) {
            session.tab_switched();
            return true;
        }
        let Some(tab) = session.tabs.active() else {
            return true;
        };
        let modes = tab.terminal.modes();
        let rows = tab.terminal.size().rows();
        if let Some(scroll) = mouse::viewport_scroll(action, modes, rows) {
            session.scroll_viewport(scroll);
            return true;
        }
        match action {
            Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollToTop
            | Action::ScrollToBottom => false,
            // Handled above.
            Action::Copy
            | Action::Paste
            | Action::SelectAll
            | Action::CommandPalette
            | Action::ZoomIn
            | Action::ZoomOut
            | Action::ResetZoom
            | Action::ReloadConfig
            | Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PreviousTab
            | Action::GotoTab(_) => true,
        }
    }

    /// The selected text of the active tab.
    fn selected_text(&self) -> Option<String> {
        self.session
            .as_ref()?
            .tabs
            .active()?
            .terminal
            .selection_text()
    }

    /// Copies the selection to the clipboard; nothing without one.
    fn copy(&mut self) {
        if let Some(text) = self.selected_text() {
            if let Err(error) = self.clipboard.set_text(ClipboardKind::Clipboard, text) {
                eprintln!("nxgterm: {error}");
            }
        }
    }

    /// Pastes the text of the clipboard `kind` into the active tab.
    fn paste(&mut self, kind: ClipboardKind) {
        let text = match self.clipboard.get_text(kind) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("nxgterm: {error}");
                return;
            }
        };
        if let Some(session) = &mut self.session {
            session.paste(&text);
        }
    }

    /// Selects everything in the active tab, history included.
    fn select_all(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(tab) = session.tabs.active_mut() else {
            return;
        };
        tab.terminal.select_all();
        session.window.request_redraw();
        copy_on_select(self.clipboard.as_mut(), &self.config, &tab.terminal);
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
                session.tab_bar = new.window.tab_bar;
                session.restyle(&new);
            }
            if changes.scrollback {
                for tab in session.tabs.iter_mut() {
                    tab.terminal.set_scrollback_limit(new.scrollback.lines);
                }
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

    /// Tears the window down while the event loop still owns the display
    /// connection. `run_app` consumes the event loop, so anything left in
    /// `self` outlives the Wayland display: wgpu's EGL then marshals on a
    /// dead `wl_display` and the process segfaults on exit.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut session) = self.session.take() {
            // The surface goes before the window it presents to.
            drop(std::mem::replace(&mut session.renderer, Box::new(Detached)));
            drop(session);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Output(id, bytes) => {
                if let Some(session) = &mut self.session {
                    session.output(id, &bytes);
                }
            }
            UserEvent::Exited(id) => {
                if let Some(session) = &mut self.session {
                    if let Some(index) = session.tabs.index_of(id) {
                        session.close_tab(index);
                    }
                    if session.tabs.is_empty() {
                        event_loop.exit();
                    }
                }
            }
            UserEvent::ConfigChanged => self.reload_config(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            if event.state == ElementState::Pressed {
                self.key_pressed(event);
            }
            // The last tab may have been closed by a key.
            if self.session.as_ref().is_some_and(|s| s.tabs.is_empty()) {
                event_loop.exit();
            }
            return;
        }
        if let Some(outcome) = self.palette_mouse(&event) {
            self.palette_done(outcome);
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
                let Some(tab) = session.tabs.active_mut() else {
                    return;
                };
                let modes = tab.terminal.modes();
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
                            tab.send(&bytes.repeat(count as usize));
                        }
                    }
                    Some(WheelAction::Keys(bytes)) => tab.send(&bytes),
                    Some(WheelAction::Scroll(lines)) => {
                        session.scroll_viewport(ViewportScroll::Lines(lines));
                    }
                    None => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let moved = session.track_pointer(position.x, position.y);
                if session.drag.is_some() {
                    session.drag_to(position.y);
                    return;
                }
                if !moved {
                    return;
                }
                let cell = session.pointer;
                let Some(tab) = session.tabs.active_mut() else {
                    return;
                };
                let event = MouseEvent {
                    button: session.held.unwrap_or(MouseButton::None),
                    action: MouseAction::Motion,
                    col: cell.0,
                    row: cell.1,
                    mods: mouse::mouse_mods(self.modifiers),
                };
                let modes = tab.terminal.modes();
                let shift = self.modifiers.shift_key();
                let held = session.held.is_some();
                if let Some(bytes) = mouse::button_report(modes, shift, event, held) {
                    tab.send(&bytes);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = mouse::button(button) else {
                    return;
                };
                let action = mouse::action(state);
                if button == MouseButton::Left && action == MouseAction::Release {
                    if let Some(drag) = session.drag.take() {
                        if let (true, Some(tab)) = (drag.started, session.tabs.active()) {
                            copy_on_select(self.clipboard.as_mut(), config, &tab.terminal);
                        }
                        return;
                    }
                }
                let shift = self.modifiers.shift_key();
                let selects = session
                    .tabs
                    .active()
                    .is_some_and(|tab| mouse::selects(tab.terminal.modes(), shift));
                if selects && session.bar_pointer.is_none() && action == MouseAction::Press {
                    match button {
                        MouseButton::Left => {
                            let alt = self.modifiers.alt_key();
                            session.start_drag(Instant::now(), alt);
                            return;
                        }
                        MouseButton::Middle if HAS_PRIMARY => {
                            let text = self.clipboard.get_text(ClipboardKind::Primary);
                            match text {
                                Ok(text) => session.paste(&text),
                                Err(error) => eprintln!("nxgterm: {error}"),
                            }
                            return;
                        }
                        _ => {}
                    }
                }
                if let Some(col) = session.bar_pointer {
                    if action == MouseAction::Press {
                        if button == MouseButton::Left {
                            session.click_bar(col);
                        }
                        return;
                    }
                    if session.held.is_none() {
                        return; // Released after a press on the bar.
                    }
                    // A drag from the grid ends over the bar: report the
                    // release at the last grid cell.
                }
                if action == MouseAction::Release && session.held.is_none() {
                    // Released after a press the terminal never saw, such
                    // as the click that closed the command palette.
                    return;
                }
                session.held = (action == MouseAction::Press).then_some(button);
                let (col, row) = session.pointer;
                let event = MouseEvent {
                    button,
                    action,
                    col,
                    row,
                    mods: mouse::mouse_mods(self.modifiers),
                };
                let Some(tab) = session.tabs.active_mut() else {
                    return;
                };
                let modes = tab.terminal.modes();
                let shift = self.modifiers.shift_key();
                if let Some(bytes) = mouse::button_report(modes, shift, event, false) {
                    tab.send(&bytes);
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

impl Tab {
    /// Writes input for the child.
    fn send(&mut self, bytes: &[u8]) {
        if let Err(error) = self.pty.write_all(bytes) {
            eprintln!("nxgterm: pty write failed: {error}");
        }
    }
}

impl Session {
    /// Writes input for the child of the active tab.
    fn send(&mut self, bytes: &[u8]) {
        if let Some(tab) = self.tabs.active_mut() {
            tab.send(bytes);
        }
    }

    /// Spawns the configured shell in a new tab next to the active one,
    /// sized to the grid, and activates it.
    fn open_tab(
        &mut self,
        config: &Config,
        proxy: &EventLoopProxy<UserEvent>,
    ) -> Result<TabId, Box<dyn Error>> {
        let pixels = self.window.inner_size();
        // The bar may appear with this tab.
        let layout = self.grid_layout_for(self.tabs.len() + 1);
        let win_size = WinSize {
            cells: layout.grid_size(pixels.width, pixels.height),
            cell: Some(CellPixels::new(layout.cell.width, layout.cell.height)),
        };
        let id = self
            .tabs
            .add_with(|id| spawn_tab(id, config, win_size, proxy))?;
        self.tab_switched();
        Ok(id)
    }

    /// Closes the tab at `index`, ending its child. With no tab left the
    /// caller exits.
    fn close_tab(&mut self, index: usize) {
        if self.tabs.close(index).is_some() {
            self.tab_switched();
        }
    }

    /// Feeds output to the tab `id` (ignored once it is closed), answering
    /// the queries in it; only the active tab is redrawn.
    fn output(&mut self, id: TabId, bytes: &[u8]) {
        let active = self.tabs.index_of(id) == Some(self.tabs.active_index());
        let Some(tab) = self.tabs.get_mut(id) else {
            return;
        };
        tab.terminal.advance(bytes);
        let responses = tab.terminal.take_responses();
        if !responses.is_empty() {
            tab.send(&responses);
        }
        if active {
            self.window.request_redraw();
        }
    }

    /// Sends pasted `text` to the active tab, bracketed when the
    /// application asked for it, and shows the live screen.
    fn paste(&mut self, text: &str) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let bytes = nxg_core::paste::encode(text, tab.terminal.modes().bracketed_paste);
        self.scroll_viewport(ViewportScroll::Bottom);
        // Written on the UI thread: the reader thread keeps draining the
        // child's output meanwhile, so a large paste cannot deadlock.
        self.send(&bytes);
    }

    /// A left press on the grid: a single click clears the selection and
    /// may start a new one, a double or triple click selects the word or
    /// line under the pointer.
    fn start_drag(&mut self, now: Instant, alt: bool) {
        let (col, row) = self.pointer;
        let Some(tab) = self.tabs.active_mut() else {
            return;
        };
        let anchor = tab.terminal.point_at(col, row);
        let clicks = self.clicks.press(now, anchor);
        let kind = mouse::selection_kind(clicks, alt);
        let started = clicks > 1;
        if started {
            tab.terminal.start_selection(kind, anchor);
        } else {
            tab.terminal.clear_selection();
        }
        self.drag = Some(Drag {
            kind,
            anchor,
            started,
        });
        self.window.request_redraw();
    }

    /// The pointer moved to window height `y` while selecting: extends the
    /// selection to the cell under it, scrolling the history a line when
    /// it is above or below the grid.
    fn drag_to(&mut self, y: f64) {
        let layout = self.grid_layout();
        let (col, row) = self.pointer;
        let (Some(drag), Some(tab)) = (&mut self.drag, self.tabs.active_mut()) else {
            return;
        };
        let terminal = &mut tab.terminal;
        let scroll = mouse::drag_scroll(layout, terminal.size(), y);
        let before = terminal.display_offset();
        if scroll != 0 {
            terminal.scroll_display(scroll);
        }
        let scrolled = terminal.display_offset() != before;
        let at = terminal.point_at(col, row);
        let previous = terminal.selection().map(|selection| selection.head());
        if !drag.started && at != drag.anchor {
            terminal.start_selection(drag.kind, drag.anchor);
            drag.started = true;
        }
        if drag.started {
            terminal.extend_selection(at);
        }
        let extended = terminal.selection().map(|selection| selection.head()) != previous;
        if scrolled || extended {
            self.window.request_redraw();
        }
    }

    /// After the active tab or the number of tabs changed: forgets the
    /// held button (and the pointer on a bar that went away), refits the
    /// grid and redraws.
    fn tab_switched(&mut self) {
        self.held = None;
        self.drag = None;
        if !self.tab_bar.visible(self.tabs.len()) {
            self.bar_pointer = None;
        }
        self.sync_grid_size();
        self.window.request_redraw();
    }

    /// Moves the scrollback viewport, redrawing when it changed.
    fn scroll_viewport(&mut self, scroll: ViewportScroll) {
        let Some(tab) = self.tabs.active_mut() else {
            return;
        };
        let terminal = &mut tab.terminal;
        let before = terminal.display_offset();
        match scroll {
            ViewportScroll::Lines(lines) => terminal.scroll_display(lines),
            ViewportScroll::Top => terminal.scroll_display(i32::MAX),
            ViewportScroll::Bottom => terminal.scroll_display_to_bottom(),
        }
        if terminal.display_offset() != before {
            self.window.request_redraw();
        }
    }

    /// Where the grid sits in the window, below the tab bar if it shows.
    fn grid_layout(&self) -> Layout {
        self.grid_layout_for(self.tabs.len())
    }

    /// [`Session::grid_layout`] with `tabs` tabs open.
    fn grid_layout_for(&self, tabs: usize) -> Layout {
        let bar = u16::from(self.tab_bar.visible(tabs));
        layout(self.renderer.as_ref(), self.padding).below(bar)
    }

    /// Follows the pointer at window pixel `x`, `y` over the tab bar and
    /// the grid. Returns whether it moved to another grid cell (motion to
    /// report).
    fn track_pointer(&mut self, x: f64, y: f64) -> bool {
        self.bar_pointer = self.bar_column_at(x, y);
        if self.bar_pointer.is_some() {
            // The bar is not part of the terminal: no reports.
            return false;
        }
        let layout = self.grid_layout();
        let Some(tab) = self.tabs.active() else {
            return false;
        };
        let cell = mouse::cell_at(layout, tab.terminal.size(), x, y);
        if cell == self.pointer {
            return false;
        }
        self.pointer = cell;
        true
    }

    /// Where the open palette sits on the active tab's grid.
    fn palette_rect(&self) -> Option<command_palette::Rect> {
        let size = self.tabs.active()?.terminal.size();
        Some(self.palette.as_ref()?.rect(size))
    }

    /// The tab bar labels for a bar `cols` columns wide.
    fn bar_labels(&self, cols: u16) -> Vec<tab_bar::Label> {
        let titles: Vec<&str> = self.tabs.iter().map(|tab| tab.title.as_str()).collect();
        tab_bar::layout(&titles, self.tabs.active_index(), cols)
    }

    /// The tab bar column at window pixel `x`, `y`, if the bar shows there.
    fn bar_column_at(&self, x: f64, y: f64) -> Option<u16> {
        if !self.tab_bar.visible(self.tabs.len()) {
            return None;
        }
        let cols = self.tabs.active()?.terminal.size().cols();
        let layout = layout(self.renderer.as_ref(), self.padding);
        tab_bar::column_at(layout, cols, x, y)
    }

    /// Activates the tab whose label is at bar column `col`.
    fn click_bar(&mut self, col: u16) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let labels = self.bar_labels(tab.terminal.size().cols());
        if let Some(index) = tab_bar::tab_at(&labels, col) {
            if index != self.tabs.active_index() && self.tabs.select(index) {
                self.tab_switched();
            }
        }
    }

    fn redraw(&mut self, config: &Config) -> Result<(), Box<dyn Error>> {
        let Some(tab) = self.tabs.active() else {
            return Ok(());
        };
        let bar = self.tab_bar.visible(self.tabs.len()).then(|| {
            let cols = tab.terminal.size().cols();
            tab_bar::render(&self.bar_labels(cols), cols)
        });
        let palette = self.palette.as_ref().map(|palette| {
            let rect = palette.rect(tab.terminal.size());
            let theme = appearance::palette(&config.colors.resolve());
            let surface = command_palette::surface(theme.background, theme.foreground);
            (palette.render(rect, surface), rect)
        });
        let overlay = palette.as_ref().map(|(terminal, rect)| Overlay {
            terminal,
            col: rect.col,
            row: rect.row,
        });
        match self
            .renderer
            .draw_layers(bar.as_ref(), &tab.terminal, overlay)
        {
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

    /// Resizes the terminals and ptys of every tab to what fits the
    /// window with the active renderer's cell size and the padding, and
    /// tells them the cell size in pixels (images and size reports depend
    /// on it).
    fn sync_grid_size(&mut self) {
        let pixels = self.window.inner_size();
        let layout = self.grid_layout();
        let size = layout.grid_size(pixels.width, pixels.height);
        let cell = CellPixels::new(layout.cell.width, layout.cell.height);
        for tab in self.tabs.iter_mut() {
            tab.fit(size, cell);
        }
    }
}

impl Tab {
    /// Resizes the terminal and the pty to `size` cells of `cell` pixels.
    fn fit(&mut self, size: TermSize, cell: CellPixels) {
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

/// Starts the configured shell (or the platform default) on a new pty of
/// `win_size` for tab `id`, with threads that post its output and exit.
/// Every tab starts in the working directory nxgterm was started in.
fn spawn_tab(
    id: TabId,
    config: &Config,
    win_size: WinSize,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<Tab, Box<dyn Error>> {
    let shell = config.shell.program.clone().map(|program| ShellCommand {
        program,
        args: config.shell.args.clone(),
    });
    let backends = nxg_pty::backends_from_env(env::var(nxg_pty::ENV_VAR).ok().as_deref());
    let pty = nxg_pty::spawn_shell_with(win_size, shell.as_ref(), backends)?;
    for attempt in &pty.skipped {
        eprintln!("nxgterm: skipped {}: {}", attempt.name, attempt.error);
    }
    eprintln!("nxgterm: pty {}", pty.name);
    // Windows: whether the bundled ConPTY (which passes images through) is used.
    if let Some(host) =
        nxg_pty::conpty_host().filter(|_| pty.name == nxg_pty::Backend::Native.name())
    {
        eprintln!("nxgterm: conpty {host}");
    }
    let pty = pty.backend;
    spawn_reader(id, pty.reader, proxy.clone());
    spawn_waiter(id, pty.child, proxy.clone());

    let mut terminal = Terminal::new(win_size.cells);
    if let Some(cell) = win_size.cell {
        terminal.set_cell_pixels(cell.width, cell.height);
    }
    terminal.set_scrollback_limit(config.scrollback.lines);
    let shell_env = env::var("SHELL").ok();
    Ok(Tab {
        terminal,
        pty: pty.control,
        title: tab_bar::title(
            config.shell.program.as_deref(),
            shell_env.as_deref(),
            cfg!(windows),
        ),
    })
}

/// Copies the selection of `terminal` to the PRIMARY selection when the
/// config asks for it and the system has one; failures only warn.
fn copy_on_select(clipboard: &mut dyn Clipboard, config: &Config, terminal: &Terminal) {
    if !(HAS_PRIMARY && config.selection.copy_on_select) {
        return;
    }
    if let Some(text) = terminal.selection_text() {
        if let Err(error) = clipboard.set_text(ClipboardKind::Primary, text) {
            eprintln!("nxgterm: {error}");
        }
    }
}

/// The renderer's cells inset by `padding` pixels. Takes the subtrait\n/// object directly: upcasting to `&dyn Renderer` needs Rust 1.86.
fn layout(renderer: &dyn WindowRenderer, padding: u32) -> Layout {
    let (width, height) = renderer.cell_size();
    Layout {
        cell: CellSize { width, height },
        padding,
        left: 0,
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

    fn draw_layers(
        &mut self,
        _header: Option<&Terminal>,
        terminal: &Terminal,
        _overlay: Option<Overlay<'_>>,
    ) -> Result<(), RenderError> {
        self.draw(terminal)
    }
}

/// Drains the output of tab `id` on a background thread until EOF or
/// error.
fn spawn_reader(id: TabId, mut reader: Box<dyn Read + Send>, proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if proxy
                        .send_event(UserEvent::Output(id, buf[..n].to_vec()))
                        .is_err()
                    {
                        return; // Event loop is gone.
                    }
                }
            }
        }
        let _ = proxy.send_event(UserEvent::Exited(id));
    });
}

/// Reports the exit of the child of tab `id`; needed where the reader
/// never sees EOF (ConPTY).
fn spawn_waiter(id: TabId, mut child: Box<dyn ChildProcess>, proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        let _ = child.wait();
        let _ = proxy.send_event(UserEvent::Exited(id));
    });
}
