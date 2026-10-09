//! Window, event loop and the glue between pty, terminal and renderer.

use std::env;
use std::error::Error;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use nxg_config::{Backend, Bindings, Config, Decorations, FontConfig, TabBar};
use nxg_core::fallback::{self, Init};
use nxg_core::mouse::{MouseAction, MouseButton, MouseEvent};
use nxg_core::ports::{ChildProcess, Clipboard, ClipboardKind, PtyControl, RenderError, Renderer};
use nxg_core::{CellPixels, TermSize, Terminal, WinSize};
use nxg_pty::ShellCommand;
use nxg_render::{
    CellSize, CpuWindowRenderer, FontError, FontFaces, GpuRenderer, Layout, Overlay, PaneView,
    Shape, Style, WindowRenderer,
};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, ResizeDirection, Window, WindowId};

use crate::bindings::Action;
use crate::clipboard::HAS_PRIMARY;
use crate::command_palette::{self, CommandPalette, Outcome};
use crate::mouse::{Capture, Clicks, Drag, Pointer, ViewportScroll, Wheel, WheelAction};
use crate::panes::{Axis, DividerPath, PaneId, SplitError};
use crate::tab::{self, Exit, PaneCommand, PaneIds, Tab, Under};
use crate::tabs::{TabId, Tabs};
use crate::throttle::Throttle;
use crate::title_bar::{self, Button, ButtonColors, Chrome, Rect, Region};
use crate::watch::ConfigWatcher;
use crate::{
    appearance, bindings, choice, clipboard, dividers, icon, keys, mouse, reload, tab_bar,
};

/// Events posted to the event loop from background threads.
#[derive(Debug)]
pub enum UserEvent {
    /// A chunk of output from the child of a pane.
    Output(PaneId, Vec<u8>),
    /// The child of a pane exited or its output stream closed.
    Exited(PaneId),
    /// The config file changed on disk.
    ConfigChanged,
}

/// One pane: a child process on its own pty and the terminal it draws.
struct Pane {
    terminal: Terminal,
    pty: Box<dyn PtyControl>,
    /// Label on the tab bar while the pane has focus: the program name
    /// (OSC 0/2 titles are not supported yet).
    title: String,
    /// Sizes the pty has not been told yet while a divider is dragged.
    resize: Throttle<WinSize>,
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
    /// The window was created transparent (background opacity below 1.0 at
    /// start); X11 cannot change it later.
    transparent: bool,
    /// Padding of the current style, in physical pixels.
    padding: u32,
    /// Every tab has its own panes, each with its own shell; only the
    /// active tab is drawn and its focused pane gets input, the others
    /// keep reading their output.
    tabs: Tabs<Tab<Pane>>,
    /// Pane ids, unique across every tab.
    pane_ids: PaneIds,
    /// When the tab bar shows; the grid is one row shorter while it does.
    tab_bar: TabBar,
    /// The tab bar is the window's title bar (`window.decorations =
    /// "integrated"` at start): it always shows, moves the window and,
    /// except on macOS, has the window buttons and resizing edges.
    integrated: bool,
    /// The pointer in window pixels.
    cursor: (f64, f64),
    /// The window edge under the pointer, shown with a resize cursor.
    resize_hover: Option<ResizeDirection>,
    /// The bar button under the pointer, drawn highlighted.
    bar_hover: Option<Region>,
    /// The bar button pressed with the left button: it acts when released
    /// over it.
    bar_pressed: Option<Region>,
    /// The last left press on the empty bar, for double clicks.
    bar_drag_press: Option<Instant>,
    /// Consecutive skipped frames; bounds retries so a surface that keeps
    /// failing (e.g. occluded) does not spin the event loop.
    skipped_frames: u8,
    /// Wheel movement not yet worth a whole line.
    wheel: Wheel,
    /// What the pressed button holds the pointer for, and the cell last
    /// reported.
    pointer: Pointer,
    /// The way of the divider under the pointer, shown with a resize
    /// cursor.
    divider_hover: Option<Axis>,
    /// The tab bar column under the pointer, while it is over the bar.
    bar_pointer: Option<u16>,
    /// The command palette, while it is open: it takes the keys and the
    /// clicks, and is drawn over the grid.
    palette: Option<CommandPalette>,
    /// Left-button presses, for double and triple clicks.
    clicks: Clicks,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    /// Config file to reload on change; `None` when no location is known.
    config_path: Option<PathBuf>,
    /// Watches the config files; told about new imports and theme files on
    /// reload.
    watcher: Option<ConfigWatcher>,
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
        watcher: Option<ConfigWatcher>,
    ) -> Self {
        let bindings = config.keybindings.resolve(MACOS);
        Self {
            proxy,
            config,
            config_path,
            watcher,
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
        let transparent = config.window.translucent();
        let blur = appearance::blur(transparent, config.window.blur);
        let integrated = config.window.decorations == Decorations::Integrated;
        // A DirectComposition swapchain (translucent DX12 windows) shows
        // white until its first present: show the window after one frame.
        let show_after_first_frame = cfg!(windows) && transparent;
        let attributes = Window::default_attributes()
            .with_title("nxgterm")
            .with_visible(!show_after_first_frame)
            .with_window_icon(icon::window_icon())
            .with_theme(Some(appearance::window_theme(&config.colors.resolve())))
            .with_transparent(transparent)
            .with_blur(blur)
            // macOS keeps its frame and window buttons, under the content.
            .with_decorations(!integrated || MACOS);
        #[cfg(windows)]
        let attributes = {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes
                .with_system_backdrop(appearance::backdrop(blur))
                .with_undecorated_shadow(integrated)
        };
        #[cfg(target_os = "macos")]
        let attributes = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attributes
                .with_titlebar_transparent(integrated)
                .with_fullsize_content_view(integrated)
                .with_title_hidden(integrated)
        };
        let window = Arc::new(event_loop.create_window(attributes)?);
        let scale = window.scale_factor();
        let faces = load_faces(&config.font)?;
        let style = build_style(&faces, config, config.font.size, scale)?;

        let initial = TermSize::new(config.window.columns.get(), config.window.rows.get())?;
        // The integrated bar is part of the window: add its row.
        let (width, height) = style
            .layout()
            .below(u16::from(integrated))
            .window_size(initial);
        // May be ignored (tiling window managers) or applied later through
        // a `Resized` event; the grid follows the actual size either way.
        let _ = window.request_inner_size(PhysicalSize::new(width, height));

        let padding = style.padding;
        let mut renderer = select_renderer(&window, &style, config.renderer.backend, transparent)?;
        let pixels = window.inner_size();
        renderer.resize(pixels.width, pixels.height);
        report_opacity(config, transparent, renderer.as_ref());
        let mut session = Session {
            window,
            renderer,
            faces,
            font_size: config.font.size,
            scale,
            transparent,
            padding,
            tabs: Tabs::new(),
            pane_ids: PaneIds::default(),
            tab_bar: config.window.tab_bar,
            integrated,
            cursor: (0.0, 0.0),
            resize_hover: None,
            bar_hover: None,
            bar_pressed: None,
            bar_drag_press: None,
            skipped_frames: 0,
            wheel: Wheel::default(),
            pointer: Pointer::default(),
            divider_hover: None,
            bar_pointer: None,
            palette: None,
            clicks: Clicks::default(),
        };
        session.open_tab(config, &self.proxy)?;
        if show_after_first_frame {
            // Hidden windows get no redraw requests, so draw here.
            let drawn = session.redraw(config);
            session.window.set_visible(true);
            drawn?;
        }
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
        session.pointer.reset();
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
                let (col, row) = session.grid_cell();
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

    /// Handles what the integrated title bar does with the mouse, before
    /// the palette, the selection or the terminal see it: a left press on a
    /// window edge resizes, one on the empty bar moves the window (twice
    /// quickly: maximizes or restores), and the window and new-tab buttons
    /// act when released over. A right press on the empty bar opens the
    /// window menu (Windows only). Returns whether the event was used up;
    /// pointer motion never is.
    fn chrome_mouse(&mut self, event_loop: &ActiveEventLoop, event: &WindowEvent) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        if !session.integrated {
            return false;
        }
        let (x, y) = session.cursor;
        let pressed = match *event {
            WindowEvent::CursorMoved { position, .. } => {
                session.hover(position.x, position.y);
                return false;
            }
            WindowEvent::CursorLeft { .. } => {
                session.unhover();
                return false;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Right,
                ..
            } if session.bar_region_at(x, y) == Some(Region::Drag) => {
                session.window.show_window_menu(PhysicalPosition::new(x, y));
                return true;
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => state == ElementState::Pressed,
            _ => return false,
        };
        if pressed {
            if let Some(direction) = session.resize_edge_at(x, y) {
                // Fails only where unsupported; then nothing happens.
                let _ = session.window.drag_resize_window(direction);
                return true;
            }
            return match session.bar_region_at(x, y) {
                Some(Region::Drag) => {
                    session.drag_bar(Instant::now());
                    true
                }
                Some(region @ (Region::NewTab | Region::Button(_))) => {
                    session.bar_pressed = Some(region);
                    true
                }
                Some(Region::Tab(_)) | None => false,
            };
        }
        let Some(region) = session.bar_pressed.take() else {
            return false;
        };
        if session.bar_region_at(x, y) != Some(region) {
            return true; // Released elsewhere: cancelled.
        }
        let window = &session.window;
        match region {
            Region::NewTab => {
                session.palette = None;
                self.new_tab();
            }
            Region::Button(Button::Minimize) => window.set_minimized(true),
            Region::Button(Button::Maximize) => window.set_maximized(!window.is_maximized()),
            Region::Button(Button::Close) => event_loop.exit(),
            Region::Tab(_) | Region::Drag => {}
        }
        true
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
        let Some(pane) = tab::focused_mut(&mut session.tabs) else {
            return;
        };
        let modes = pane.terminal.modes();
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
        if let Some(command) = tab::command(action) {
            self.pane_command(command);
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
        let Some(pane) = tab::focused(&session.tabs) else {
            return true;
        };
        let modes = pane.terminal.modes();
        let rows = pane.terminal.size().rows();
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
            | Action::GotoTab(_)
            // Handled above (`tab::command`).
            | Action::SplitRight
            | Action::SplitDown
            | Action::FocusPaneLeft
            | Action::FocusPaneRight
            | Action::FocusPaneUp
            | Action::FocusPaneDown
            | Action::ResizePaneLeft
            | Action::ResizePaneRight
            | Action::ResizePaneUp
            | Action::ResizePaneDown
            | Action::ClosePane
            | Action::ZoomPane
            | Action::EqualizePanes => true,
        }
    }

    /// Runs a pane action on the active tab.
    fn pane_command(&mut self, command: PaneCommand) {
        let Some(session) = &mut self.session else {
            return;
        };
        let area = session.content_size();
        match command {
            PaneCommand::Split(axis) => session.split(axis, &self.config, &self.proxy),
            PaneCommand::Focus(dir) => {
                if tab::focus_active(&mut session.tabs, dir, area) {
                    session.panes_changed();
                }
            }
            PaneCommand::Resize(dir) => {
                if tab::resize_active(&mut session.tabs, dir, area) {
                    session.panes_changed();
                }
            }
            PaneCommand::Close => {
                if tab::close_focused(&mut session.tabs) != Exit::Stale {
                    session.tab_switched();
                }
            }
            PaneCommand::Zoom => {
                if tab::zoom_active(&mut session.tabs) {
                    session.panes_changed();
                }
            }
            PaneCommand::Equalize => {
                if tab::equalize_active(&mut session.tabs, area) {
                    session.panes_changed();
                }
            }
        }
    }

    /// The selected text of the focused pane.
    fn selected_text(&self) -> Option<String> {
        let session = self.session.as_ref()?;
        tab::focused(&session.tabs)?.terminal.selection_text()
    }

    /// Copies the selection to the clipboard; nothing without one.
    fn copy(&mut self) {
        if let Some(text) = self.selected_text() {
            if let Err(error) = self.clipboard.set_text(ClipboardKind::Clipboard, text) {
                eprintln!("nxgterm: {error}");
            }
        }
    }

    /// Pastes the text of the clipboard `kind` into the focused pane.
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

    /// Selects everything in the focused pane, history included.
    fn select_all(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(pane) = tab::focused_mut(&mut session.tabs) else {
            return;
        };
        pane.terminal.select_all();
        copy_on_select(self.clipboard.as_mut(), &self.config, &pane.terminal);
        session.window.request_redraw();
    }

    /// Reloads the config file, applying what can change live, and
    /// watches the imports and theme file it now names. An invalid file
    /// keeps the previous config.
    fn reload_config(&mut self) {
        let Some(path) = &self.config_path else {
            return;
        };
        let new = match Config::load_with_files(path) {
            Ok(Some(loaded)) => {
                if let Some(watcher) = &mut self.watcher {
                    watcher.add(&loaded.files);
                }
                loaded.config
            }
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
            if self.config.colors != new.colors {
                let theme = appearance::window_theme(&new.colors.resolve());
                session.window.set_theme(Some(theme));
            }
            if self.config.window.opacity != new.window.opacity {
                report_opacity(&new, session.transparent, session.renderer.as_ref());
            }
            if changes.blur {
                let blur = appearance::blur(session.transparent, new.window.blur);
                appearance::apply_blur(&session.window, blur);
            }
            if changes.panes {
                session.window.request_redraw();
            }
            if changes.scrollback {
                for tab in session.tabs.iter_mut() {
                    tab.each_mut(|pane| pane.terminal.set_scrollback_limit(new.scrollback.lines));
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

    /// Sends the pty sizes that waited out their interval and sleeps until
    /// the next one is due.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.poll_resizes(Instant::now());
        event_loop.set_control_flow(match session.next_resize() {
            Some(due) => ControlFlow::WaitUntil(due),
            None => ControlFlow::Wait,
        });
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
                    session.exited(id);
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
        if self.chrome_mouse(event_loop, &event) {
            return;
        }
        if let Some(outcome) = self.palette_mouse(&event) {
            self.palette_done(outcome);
            // The palette may have closed the last tab.
            if self.session.as_ref().is_some_and(|s| s.tabs.is_empty()) {
                event_loop.exit();
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
                // The pane under the pointer scrolls, focused or not.
                let Under::Pane(id) = session.under() else {
                    return;
                };
                let (col, row) = session.cell_in(id).unwrap_or_default();
                let Some(pane) = tab::pane_mut(&mut session.tabs, id) else {
                    return;
                };
                let modes = pane.terminal.modes();
                match mouse::wheel_action(modes, self.modifiers.shift_key(), lines) {
                    Some(WheelAction::Report { button, count }) => {
                        let event = MouseEvent {
                            button,
                            action: MouseAction::Press,
                            col,
                            row,
                            mods: mouse::mouse_mods(self.modifiers),
                        };
                        if let Some(bytes) = nxg_core::mouse::encode(event, modes.mouse_encoding) {
                            pane.send(&bytes.repeat(count as usize));
                        }
                    }
                    Some(WheelAction::Keys(bytes)) => pane.send(&bytes),
                    Some(WheelAction::Scroll(lines)) => {
                        session.scroll_pane(id, ViewportScroll::Lines(lines));
                    }
                    None => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                session.track_pointer(position.x, position.y);
                if session.pointer.capture.is_some() {
                    session.move_captured(self.modifiers, Instant::now());
                } else {
                    session.hover_grid(self.modifiers);
                }
            }
            WindowEvent::Focused(false) => {
                session.flush_resizes();
                session.pointer.reset();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = mouse::button(button) else {
                    return;
                };
                let action = mouse::action(state);
                let mods = self.modifiers;
                if action == MouseAction::Release {
                    // A release with nothing captured follows a press the
                    // terminal never saw, such as the click that closed the
                    // command palette.
                    let ended = session.pointer.capture.take_if(|c| c.ends_with(button));
                    match ended {
                        Some(Capture::Selection { pane, drag }) => {
                            let pane = tab::pane_mut(&mut session.tabs, pane);
                            if let (true, Some(pane)) = (drag.started, pane) {
                                copy_on_select(self.clipboard.as_mut(), config, &pane.terminal);
                            }
                        }
                        Some(Capture::Divider(_)) => session.flush_resizes(),
                        Some(Capture::Report { pane, .. }) => {
                            session.report_button(pane, button, action, mods);
                        }
                        None => {}
                    }
                    return;
                }
                if let Some(col) = session.bar_pointer {
                    if button == MouseButton::Left {
                        session.click_bar(col);
                    }
                    return;
                }
                let id = match session.under() {
                    Under::Divider(path, _) => {
                        if button == MouseButton::Left {
                            session.pointer.capture = Some(Capture::Divider(path));
                        }
                        return;
                    }
                    Under::Pane(id) => id,
                    Under::Nothing => return,
                };
                // Any press focuses the pane it lands on and is delivered to it.
                if tab::focus_pane(&mut session.tabs, id) {
                    session.window.request_redraw();
                }
                let shift = mods.shift_key();
                let selects = tab::pane_mut(&mut session.tabs, id)
                    .is_some_and(|pane| mouse::selects(pane.terminal.modes(), shift));
                if selects {
                    match button {
                        MouseButton::Left => {
                            session.start_drag(id, Instant::now(), mods.alt_key());
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
                session.pointer.capture = Some(Capture::Report { pane: id, button });
                session.report_button(id, button, action, mods);
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

impl Pane {
    /// Writes input for the child.
    fn send(&mut self, bytes: &[u8]) {
        if let Err(error) = self.pty.write_all(bytes) {
            eprintln!("nxgterm: pty write failed: {error}");
        }
    }
}

impl Session {
    /// Writes input for the child of the focused pane.
    fn send(&mut self, bytes: &[u8]) {
        if let Some(pane) = tab::focused_mut(&mut self.tabs) {
            pane.send(bytes);
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
        let pane = self.pane_ids.next();
        let id = self.tabs.add_with(|_| {
            spawn_pane(pane, config, win_size, proxy).map(|spawned| Tab::new(pane, spawned))
        })?;
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

    /// Feeds output to the pane `id` (ignored once it is closed), answering
    /// the queries in it; the window redraws only if the pane is on screen.
    fn output(&mut self, id: PaneId, bytes: &[u8]) {
        let visible = tab::shows(&self.tabs, id);
        let Some(pane) = tab::pane_mut(&mut self.tabs, id) else {
            return;
        };
        pane.terminal.advance(bytes);
        let responses = pane.terminal.take_responses();
        if !responses.is_empty() {
            pane.send(&responses);
        }
        if visible {
            self.window.request_redraw();
        }
    }

    /// The child of pane `id` ended: the pane leaves its tab, which closes
    /// with its last pane. With no tab left the caller exits.
    fn exited(&mut self, id: PaneId) {
        if tab::exit(&mut self.tabs, id) != Exit::Stale {
            self.tab_switched();
        }
    }

    /// Sends pasted `text` to the focused pane, bracketed when the
    /// application asked for it, and shows the live screen.
    fn paste(&mut self, text: &str) {
        let Some(pane) = tab::focused(&self.tabs) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let bytes = nxg_core::paste::encode(text, pane.terminal.modes().bracketed_paste);
        self.scroll_viewport(ViewportScroll::Bottom);
        // Written on the UI thread: the reader thread keeps draining the
        // child's output meanwhile, so a large paste cannot deadlock.
        self.send(&bytes);
    }

    /// A left press in pane `id`: a single click clears the selection and
    /// may start a new one, a double or triple click selects the word or
    /// line under the pointer. The pane keeps the pointer until release.
    fn start_drag(&mut self, id: PaneId, now: Instant, alt: bool) {
        let Some((col, row)) = self.cell_in(id) else {
            return;
        };
        let Some(pane) = tab::pane_mut(&mut self.tabs, id) else {
            return;
        };
        let anchor = pane.terminal.point_at(col, row);
        let clicks = self.clicks.press(now, anchor);
        let kind = mouse::selection_kind(clicks, alt);
        let started = clicks > 1;
        if started {
            pane.terminal.start_selection(kind, anchor);
        } else {
            pane.terminal.clear_selection();
        }
        let drag = Drag {
            kind,
            anchor,
            started,
        };
        self.pointer.capture = Some(Capture::Selection { pane: id, drag });
        self.window.request_redraw();
    }

    /// The pointer moved while pane `id` holds a selection: extends it to
    /// the cell under the pointer (clamped to the pane), scrolling the
    /// history a line when the pointer is above or below the pane.
    fn drag_to(&mut self, id: PaneId) {
        let Some((layout, size)) = self.pane_frame(id) else {
            return;
        };
        let (x, y) = self.cursor;
        let (col, row) = mouse::cell_at(layout, size, x, y);
        let Some(Capture::Selection { drag, .. }) = &mut self.pointer.capture else {
            return;
        };
        let Some(pane) = tab::pane_mut(&mut self.tabs, id) else {
            return;
        };
        let terminal = &mut pane.terminal;
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

    /// The pointer moved with a button captured: the captured pane or
    /// divider follows it, whatever lies under it.
    fn move_captured(&mut self, mods: ModifiersState, now: Instant) {
        match self.pointer.capture.clone() {
            Some(Capture::Divider(path)) => self.drag_divider(&path, now),
            Some(Capture::Selection { pane, .. }) => self.drag_to(pane),
            Some(Capture::Report { pane, button }) => self.report_motion(pane, Some(button), mods),
            None => {}
        }
    }

    /// The pointer moved with no button down: a resize cursor over a
    /// divider, motion reports for the pane under it (not on the tab bar,
    /// which is no part of a terminal).
    fn hover_grid(&mut self, mods: ModifiersState) {
        let under = if self.bar_pointer.is_some() {
            Under::Nothing
        } else {
            self.under()
        };
        match under {
            Under::Divider(_, axis) => self.set_divider_hover(Some(axis)),
            Under::Pane(id) => {
                self.set_divider_hover(None);
                self.report_motion(id, None, mods);
            }
            Under::Nothing => self.set_divider_hover(None),
        }
    }

    /// Shows the resize cursor of a divider along `axis`, or the default.
    fn set_divider_hover(&mut self, axis: Option<Axis>) {
        if axis != self.divider_hover {
            self.divider_hover = axis;
            self.window
                .set_cursor(axis.map_or(CursorIcon::Default, mouse::divider_icon));
        }
    }

    /// Tells pane `id`, if its application asked for it, that the pointer
    /// moved to another cell (with `held` down).
    fn report_motion(&mut self, id: PaneId, held: Option<MouseButton>, mods: ModifiersState) {
        let Some((col, row)) = self.cell_in(id) else {
            return;
        };
        if self.pointer.over == Some((id, (col, row))) {
            return;
        }
        self.pointer.over = Some((id, (col, row)));
        let Some(pane) = tab::pane_mut(&mut self.tabs, id) else {
            return;
        };
        let event = MouseEvent {
            button: held.unwrap_or(MouseButton::None),
            action: MouseAction::Motion,
            col,
            row,
            mods: mouse::mouse_mods(mods),
        };
        let reported = mouse::button_report(
            pane.terminal.modes(),
            mods.shift_key(),
            event,
            held.is_some(),
        );
        if let Some(bytes) = reported {
            pane.send(&bytes);
        }
    }

    /// Tells pane `id`, if its application asked for it, about a button
    /// press or release at the pointer.
    fn report_button(
        &mut self,
        id: PaneId,
        button: MouseButton,
        action: MouseAction,
        mods: ModifiersState,
    ) {
        let Some((col, row)) = self.cell_in(id) else {
            return;
        };
        let Some(pane) = tab::pane_mut(&mut self.tabs, id) else {
            return;
        };
        let event = MouseEvent {
            button,
            action,
            col,
            row,
            mods: mouse::mouse_mods(mods),
        };
        let reported = mouse::button_report(pane.terminal.modes(), mods.shift_key(), event, false);
        if let Some(bytes) = reported {
            pane.send(&bytes);
        }
    }

    /// The divider under the captured pointer follows it; the terminals
    /// resize at once, the ptys at most every 30 ms.
    fn drag_divider(&mut self, divider: &DividerPath, now: Instant) {
        let (x, y) = self.cursor;
        let (layout, area) = (self.grid_layout(), self.content_size());
        if tab::drag_divider(&mut self.tabs, layout, area, divider, x, y) {
            self.sync_grid_size_at(Some(now));
            self.window.request_redraw();
        }
    }

    /// Tells every pty the size it is still waiting for.
    fn flush_resizes(&mut self) {
        for tab in self.tabs.iter_mut() {
            tab.each_mut(Pane::flush_resize);
        }
    }

    /// Tells the ptys whose waiting size is due at `now`.
    fn poll_resizes(&mut self, now: Instant) {
        for tab in self.tabs.iter_mut() {
            tab.each_mut(|pane| pane.poll_resize(now));
        }
    }

    /// When the next waiting pty size is due.
    fn next_resize(&self) -> Option<Instant> {
        let panes = self.tabs.iter().flat_map(|tab| tab.panes.iter());
        panes.filter_map(|(_, pane)| pane.resize.deadline()).min()
    }

    /// Splits the focused pane of the active tab along `axis`, running the
    /// configured shell in the new pane at its final size. A pane that is
    /// too small, or a shell that does not start, leaves the layout as it
    /// was.
    fn split(&mut self, axis: Axis, config: &Config, proxy: &EventLoopProxy<UserEvent>) {
        let area = self.content_size();
        let layout = self.grid_layout();
        let cell = CellPixels::new(layout.cell.width, layout.cell.height);
        let id = self.pane_ids.next();
        let split = tab::split_active(&mut self.tabs, axis, id, area, |rect| {
            let cells = TermSize::new(rect.cols, rect.rows)?;
            let win_size = WinSize {
                cells,
                cell: Some(cell),
            };
            spawn_pane(id, config, win_size, proxy)
        });
        match split {
            Some(Ok(())) => self.panes_changed(),
            Some(Err(SplitError::TooSmall)) => eprintln!("nxgterm: pane too small to split"),
            Some(Err(SplitError::Make(error))) => eprintln!("nxgterm: cannot split: {error}"),
            None => {}
        }
    }

    /// After the layout of the active tab changed (split, focus, resize,
    /// zoom, equalize): forgets what the pointer held, which belonged to the
    /// old layout, refits the panes and redraws.
    fn panes_changed(&mut self) {
        self.pointer.reset();
        self.set_divider_hover(None);
        self.sync_grid_size();
        self.window.request_redraw();
    }

    /// After the active tab or the number of tabs changed: forgets the
    /// held button (and the pointer on a bar that went away), refits the
    /// grid and redraws.
    fn tab_switched(&mut self) {
        self.pointer.reset();
        self.set_divider_hover(None);
        if !self.bar_visible(self.tabs.len()) {
            self.bar_pointer = None;
        }
        self.sync_grid_size();
        self.window.request_redraw();
    }

    /// Moves the scrollback viewport of the focused pane, redrawing when it
    /// changed.
    fn scroll_viewport(&mut self, scroll: ViewportScroll) {
        if let Some(id) = self.tabs.active().map(|tab| tab.panes.focused().0) {
            self.scroll_pane(id, scroll);
        }
    }

    /// [`Session::scroll_viewport`] for pane `id`.
    fn scroll_pane(&mut self, id: PaneId, scroll: ViewportScroll) {
        let Some(pane) = tab::pane_mut(&mut self.tabs, id) else {
            return;
        };
        let terminal = &mut pane.terminal;
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

    /// Whether the tab bar shows with `tabs` tabs open: always when it is
    /// the title bar.
    fn bar_visible(&self, tabs: usize) -> bool {
        self.integrated || self.tab_bar.visible(tabs)
    }

    /// [`Session::grid_layout`] with `tabs` tabs open.
    fn grid_layout_for(&self, tabs: usize) -> Layout {
        let bar = u16::from(self.bar_visible(tabs));
        layout(self.renderer.as_ref(), self.padding).below(bar)
    }

    /// Follows the pointer at window pixel `x`, `y`: remembers it and
    /// whether it is over the tab bar.
    fn track_pointer(&mut self, x: f64, y: f64) {
        self.cursor = (x, y);
        self.bar_pointer = self.bar_column_at(x, y);
    }

    /// The cell of the content area under the pointer, clamped to it.
    fn grid_cell(&self) -> (u16, u16) {
        let (x, y) = self.cursor;
        mouse::cell_at(self.grid_layout(), self.content_size(), x, y)
    }

    /// What the active tab shows under the pointer.
    fn under(&self) -> Under {
        let (x, y) = self.cursor;
        let (layout, area) = (self.grid_layout(), self.content_size());
        self.tabs
            .active()
            .map_or(Under::Nothing, |tab| tab::under(tab, layout, area, x, y))
    }

    /// Where pane `id` of the active tab sits and how big it is.
    fn pane_frame(&self, id: PaneId) -> Option<(Layout, TermSize)> {
        let tab = self.tabs.active()?;
        tab::frame(tab, self.grid_layout(), self.content_size(), id)
    }

    /// The cell of pane `id` under the pointer, clamped to the pane.
    fn cell_in(&self, id: PaneId) -> Option<(u16, u16)> {
        let (layout, size) = self.pane_frame(id)?;
        let (x, y) = self.cursor;
        Some(mouse::cell_at(layout, size, x, y))
    }

    /// Where the open palette sits on the content area.
    fn palette_rect(&self) -> Option<command_palette::Rect> {
        Some(self.palette.as_ref()?.rect(self.content_size()))
    }

    /// The area the panes of a tab share: what the window leaves below the
    /// tab bar. The title bar and the palette are sized from it.
    fn content_size(&self) -> TermSize {
        let pixels = self.window.inner_size();
        self.grid_layout().grid_size(pixels.width, pixels.height)
    }

    /// What the tab bar holds besides the labels.
    fn chrome(&self) -> Chrome {
        if !self.integrated {
            return Chrome::NATIVE;
        }
        let cell = layout(self.renderer.as_ref(), self.padding).cell;
        let inset = if MACOS {
            title_bar::macos_inset(self.scale, self.padding, cell.width)
        } else {
            0
        };
        let buttons = self
            .window_buttons()
            .map(|buttons| title_bar::first_button_col(&buttons, self.padding, cell.width));
        Chrome {
            inset,
            new_tab: true,
            buttons,
        }
    }

    /// The minimize, maximize and close buttons of the integrated title
    /// bar in window pixels, except on macOS (which keeps its own): as
    /// tall as the bar, from the window top to below its row.
    fn window_buttons(&self) -> Option<[(Button, Rect); 3]> {
        if !self.integrated || MACOS || !self.bar_visible(self.tabs.len()) {
            return None;
        }
        let layout = layout(self.renderer.as_ref(), self.padding);
        let bar_height = layout.padding + layout.cell.height;
        let width = self.window.inner_size().width;
        Some(title_bar::button_rects(width, bar_height, self.scale))
    }

    /// The tab bar laid out `cols` columns wide.
    fn bar(&self, cols: u16) -> title_bar::Bar {
        let titles: Vec<&str> = self
            .tabs
            .iter()
            .map(|tab| tab.focused().title.as_str())
            .collect();
        title_bar::layout(&titles, self.tabs.active_index(), cols, self.chrome())
    }

    /// What is on the tab bar at window pixel `x`, `y`, if it shows there.
    fn bar_region_at(&self, x: f64, y: f64) -> Option<Region> {
        let button = self
            .window_buttons()
            .and_then(|buttons| title_bar::button_at(&buttons, x, y));
        if let Some(button) = button {
            return Some(Region::Button(button));
        }
        let col = self.bar_column_at(x, y)?;
        Some(self.bar(self.content_size().cols()).region_at(col))
    }

    /// The window edge a press at pixel `x`, `y` resizes: only for the
    /// integrated title bar outside macOS (which keeps its own frame), and
    /// not while maximized or fullscreen.
    fn resize_edge_at(&self, x: f64, y: f64) -> Option<ResizeDirection> {
        if !self.integrated || MACOS {
            return None;
        }
        let window = &self.window;
        if window.is_maximized() || window.fullscreen().is_some() {
            return None;
        }
        let size = window.inner_size();
        let border = title_bar::resize_border(self.scale);
        title_bar::resize_edge(x, y, f64::from(size.width), f64::from(size.height), border)
    }

    /// Follows the pointer at pixel `x`, `y` for the integrated title bar:
    /// a resize cursor over the window edges, a highlight on the button
    /// under it.
    fn hover(&mut self, x: f64, y: f64) {
        self.cursor = (x, y);
        let edge = self.resize_edge_at(x, y);
        if edge != self.resize_hover {
            self.resize_hover = edge;
            let icon = edge.map_or(CursorIcon::Default, CursorIcon::from);
            self.window.set_cursor(icon);
        }
        let button = match edge {
            Some(_) => None,
            None => self
                .bar_region_at(x, y)
                .filter(|region| matches!(region, Region::NewTab | Region::Button(_))),
        };
        if button != self.bar_hover {
            self.bar_hover = button;
            self.window.request_redraw();
        }
    }

    /// The pointer left the window: no edge or button is hovered.
    fn unhover(&mut self) {
        self.set_divider_hover(None);
        if self.resize_hover.take().is_some() {
            self.window.set_cursor(CursorIcon::Default);
        }
        if self.bar_hover.take().is_some() {
            self.window.request_redraw();
        }
    }

    /// A left press on the empty title bar: moves the window, or toggles
    /// maximized when it is the second press of a double click.
    fn drag_bar(&mut self, now: Instant) {
        if title_bar::double_click(self.bar_drag_press, now) {
            self.bar_drag_press = None;
            self.window.set_maximized(!self.window.is_maximized());
        } else {
            self.bar_drag_press = Some(now);
            // Fails only where unsupported; then nothing happens.
            let _ = self.window.drag_window();
        }
    }

    /// The tab bar column at window pixel `x`, `y`, if the bar shows there.
    fn bar_column_at(&self, x: f64, y: f64) -> Option<u16> {
        if !self.bar_visible(self.tabs.len()) {
            return None;
        }
        let cols = self.content_size().cols();
        let layout = layout(self.renderer.as_ref(), self.padding);
        tab_bar::column_at(layout, cols, x, y)
    }

    /// Activates the tab whose label is at bar column `col`.
    fn click_bar(&mut self, col: u16) {
        let bar = self.bar(self.content_size().cols());
        if let Region::Tab(index) = bar.region_at(col) {
            if index != self.tabs.active_index() && self.tabs.select(index) {
                self.tab_switched();
            }
        }
    }

    fn redraw(&mut self, config: &Config) -> Result<(), Box<dyn Error>> {
        let Some(tab) = self.tabs.active() else {
            return Ok(());
        };
        let content = self.content_size();
        let bar = self
            .bar_visible(self.tabs.len())
            .then(|| title_bar::render(&self.bar(content.cols()), self.bar_hover));
        let buttons = self.window_buttons().map(|buttons| {
            let theme = appearance::palette(&config.colors.resolve());
            let colors = ButtonColors {
                glyph: theme.foreground,
                // The new-tab button's hover background.
                hover: theme.ansi[8],
            };
            let hover = match self.bar_hover {
                Some(Region::Button(button)) => Some(button),
                _ => None,
            };
            let maximized = self.window.is_maximized();
            title_bar::button_shapes(&buttons, hover, maximized, self.scale, colors)
        });
        let palette = self.palette.as_ref().map(|palette| {
            let rect = palette.rect(content);
            let theme = appearance::palette(&config.colors.resolve());
            let surface = command_palette::surface(theme.background, theme.foreground);
            (palette.render(rect, surface), rect)
        });
        let views: Vec<PaneView<'_>> = tab::placements(tab, content)
            .into_iter()
            .filter_map(|placed| {
                let pane = tab.panes.get(placed.id)?;
                Some(PaneView {
                    id: placed.id.raw(),
                    terminal: &pane.terminal,
                    col: placed.rect.col,
                    row: placed.rect.row,
                    focused: placed.focused,
                    dim: tab::dim_of(placed.focused, config.panes.inactive_dim),
                })
            })
            .collect();
        // Dividers first, so the window buttons stay on top.
        let theme = appearance::palette(&config.colors.resolve());
        let (width, color) = dividers::style(&config.panes, theme.foreground, theme.background);
        let mut shapes = dividers::shapes(
            &tab.panes.dividers((content.cols(), content.rows())),
            self.grid_layout(),
            width,
            color,
        );
        shapes.extend(buttons.into_iter().flatten());
        let overlay = palette.as_ref().map(|(terminal, rect)| Overlay {
            terminal,
            col: rect.col,
            row: rect.row,
        });
        match self
            .renderer
            .draw_layers(bar.as_ref(), &views, overlay, &shapes)
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
        self.renderer = cpu_renderer(self.window.clone(), style, self.transparent)?;
        let pixels = self.window.inner_size();
        self.renderer.resize(pixels.width, pixels.height);
        eprintln!("nxgterm: renderer cpu");
        report_opacity(config, self.transparent, self.renderer.as_ref());
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

    /// Resizes the terminals and ptys of every pane to the rect its tab
    /// gives it in what fits the window with the active renderer's cell
    /// size and the padding, and tells them the cell size in pixels
    /// (images and size reports depend on it).
    fn sync_grid_size(&mut self) {
        self.sync_grid_size_at(None);
    }

    /// [`Session::sync_grid_size`]; with `dragging` the ptys hear of the
    /// new sizes at most every 30 ms (see [`Pane::fit_at`]).
    fn sync_grid_size_at(&mut self, dragging: Option<Instant>) {
        let content = self.content_size();
        let layout = self.grid_layout();
        let cell = CellPixels::new(layout.cell.width, layout.cell.height);
        for tab in self.tabs.iter_mut() {
            for placed in tab::placements(tab, content) {
                let size = TermSize::new(placed.rect.cols, placed.rect.rows);
                if let (Ok(size), Some(pane)) = (size, tab.panes.get_mut(placed.id)) {
                    pane.fit_at(size, cell, dragging);
                }
            }
        }
    }
}

impl Pane {
    /// Resizes the terminal and the pty to `size` cells of `cell` pixels.
    /// With `dragging` the terminal still follows at once but the pty hears
    /// of it at most every 30 ms (see [`Pane::poll_resize`] and
    /// [`Pane::flush_resize`]).
    fn fit_at(&mut self, size: TermSize, cell: CellPixels, dragging: Option<Instant>) {
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
        let now = match dragging {
            Some(now) => self.resize.push(win_size, now),
            None => {
                self.resize.clear();
                Some(win_size)
            }
        };
        if let Some(win_size) = now {
            self.tell_pty(win_size);
        }
    }

    /// Sends the size that waited out its interval.
    fn poll_resize(&mut self, now: Instant) {
        if let Some(win_size) = self.resize.poll(now) {
            self.tell_pty(win_size);
        }
    }

    /// Sends the size that is still waiting (the drag ended).
    fn flush_resize(&mut self) {
        if let Some(win_size) = self.resize.flush() {
            self.tell_pty(win_size);
        }
    }

    fn tell_pty(&mut self, win_size: WinSize) {
        if let Err(error) = self.pty.resize(win_size) {
            eprintln!("nxgterm: pty resize failed: {error}");
        }
    }
}

/// Starts the configured shell (or the platform default) on a new pty of
/// `win_size` for pane `id`, with threads that post its output and exit.
/// Every pane starts in the working directory nxgterm was started in.
fn spawn_pane(
    id: PaneId,
    config: &Config,
    win_size: WinSize,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<Pane, Box<dyn Error>> {
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
    Ok(Pane {
        terminal,
        pty: pty.control,
        title: tab_bar::title(
            config.shell.program.as_deref(),
            shell_env.as_deref(),
            cfg!(windows),
        ),
        resize: Throttle::default(),
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
    let faces = FontFaces::system(&font.family, &font.fallback)?;
    let requested = faces.requested_family();
    let skipped: Vec<&str> = font
        .family
        .iter()
        .map(String::as_str)
        .take_while(|&family| Some(family) != requested)
        .collect();
    if !skipped.is_empty() {
        eprintln!(
            "nxgterm: font family `{}` not found; using `{}`",
            skipped.join("`, `"),
            faces.family()
        );
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
        background_opacity: config.window.opacity,
    })
}

/// Says on stderr why the configured background opacity does not show,
/// if it does not (see [`appearance::opacity_notice`]).
fn report_opacity(config: &Config, transparent: bool, renderer: &dyn WindowRenderer) {
    let notice = appearance::opacity_notice(
        config.window.opacity,
        transparent,
        renderer.name(),
        renderer.translucent(),
    );
    if let Some(notice) = notice {
        eprintln!("nxgterm: {notice}");
    }
}

/// Picks the first renderer that starts, in the order given by
/// `NXGTERM_RENDERER` or the configured backend, and logs the choice.
/// `transparent` tells that the window was created transparent.
fn select_renderer(
    window: &Arc<Window>,
    style: &Style,
    backend: Backend,
    transparent: bool,
) -> Result<Box<dyn WindowRenderer>, Box<dyn Error>> {
    let order = choice::renderer_order(env::var(choice::ENV_VAR).ok().as_deref(), backend);
    let candidates = order
        .iter()
        .map(|&name| {
            let (window, style) = (window.clone(), style.clone());
            let init: Init<'_, Box<dyn WindowRenderer>, Box<dyn Error>> = match name {
                "gpu" => Box::new(move || gpu_renderer(window, style)),
                _ => Box::new(move || cpu_renderer(window, style, transparent)),
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
    transparent: bool,
) -> Result<Box<dyn WindowRenderer>, Box<dyn Error>> {
    Ok(Box::new(CpuWindowRenderer::new(
        window,
        style,
        transparent,
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

impl WindowRenderer for Detached {
    fn set_style(&mut self, _style: Style) {}

    fn translucent(&self) -> bool {
        false
    }

    fn draw_layers(
        &mut self,
        _header: Option<&Terminal>,
        _panes: &[PaneView<'_>],
        _overlay: Option<Overlay<'_>>,
        _shapes: &[Shape],
    ) -> Result<(), RenderError> {
        Err(RenderError::Fatal("no renderer attached".into()))
    }
}

/// Drains the output of pane `id` on a background thread until EOF or
/// error.
fn spawn_reader(id: PaneId, mut reader: Box<dyn Read + Send>, proxy: EventLoopProxy<UserEvent>) {
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

/// Reports the exit of the child of pane `id`; needed where the reader
/// never sees EOF (ConPTY).
fn spawn_waiter(id: PaneId, mut child: Box<dyn ChildProcess>, proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        let _ = child.wait();
        let _ = proxy.send_event(UserEvent::Exited(id));
    });
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A pty that records the sizes it was resized to and the bytes written.
    struct FakePty {
        resizes: Arc<Mutex<Vec<WinSize>>>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for FakePty {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl PtyControl for FakePty {
        fn resize(&mut self, size: WinSize) -> io::Result<()> {
            self.resizes.lock().unwrap().push(size);
            Ok(())
        }
    }

    type Resizes = Arc<Mutex<Vec<WinSize>>>;
    type Written = Arc<Mutex<Vec<u8>>>;

    fn size(cols: u16, rows: u16) -> TermSize {
        TermSize::new(cols, rows).unwrap()
    }

    fn pane(cols: u16, rows: u16) -> (Pane, Resizes, Written) {
        let resizes = Resizes::default();
        let written = Written::default();
        let pane = Pane {
            terminal: Terminal::new(size(cols, rows)),
            pty: Box::new(FakePty {
                resizes: resizes.clone(),
                written: written.clone(),
            }),
            title: "sh".into(),
            resize: Throttle::default(),
        };
        (pane, resizes, written)
    }

    #[test]
    fn fit_resizes_the_terminal_and_the_pty() {
        let (mut pane, resizes, _) = pane(80, 24);
        let cell = CellPixels::new(9, 18);
        pane.fit_at(size(100, 30), cell, None);
        assert_eq!(pane.terminal.size(), size(100, 30));
        assert_eq!(pane.terminal.cell_pixels(), cell);
        let expected = WinSize {
            cells: size(100, 30),
            cell: Some(cell),
        };
        assert_eq!(*resizes.lock().unwrap(), vec![expected]);
    }

    #[test]
    fn fit_to_the_same_size_and_cell_is_a_no_op() {
        let (mut pane, resizes, _) = pane(80, 24);
        let cell = CellPixels::new(9, 18);
        pane.fit_at(size(100, 30), cell, None);
        pane.fit_at(size(100, 30), cell, None);
        assert_eq!(resizes.lock().unwrap().len(), 1);
    }

    #[test]
    fn fit_tells_the_pty_when_only_the_cell_size_changed() {
        let (mut pane, resizes, _) = pane(80, 24);
        pane.fit_at(size(80, 24), CellPixels::new(7, 14), None);
        pane.fit_at(size(80, 24), CellPixels::new(9, 18), None);
        let resizes = resizes.lock().unwrap();
        assert_eq!(resizes.len(), 2);
        assert_eq!(resizes[1].cells, size(80, 24));
        assert_eq!(resizes[1].cell, Some(CellPixels::new(9, 18)));
    }

    #[test]
    fn a_drag_resizes_the_terminal_at_once_and_the_pty_every_30_ms() {
        let (mut pane, resizes, _) = pane(80, 24);
        let cell = CellPixels::new(9, 18);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + std::time::Duration::from_millis(ms);
        pane.fit_at(size(81, 24), cell, Some(t0));
        pane.fit_at(size(82, 24), cell, Some(at(5)));
        pane.fit_at(size(83, 24), cell, Some(at(10)));
        assert_eq!(pane.terminal.size(), size(83, 24), "the grid follows");
        assert_eq!(resizes.lock().unwrap().len(), 1, "the shell hears once");
        pane.poll_resize(at(20));
        assert_eq!(resizes.lock().unwrap().len(), 1, "too early");
        pane.poll_resize(at(30));
        let sent = resizes.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].cells, size(83, 24), "only the last size");
    }

    #[test]
    fn flush_delivers_the_last_size_on_release_and_a_direct_fit_supersedes() {
        let (mut pane, resizes, _) = pane(80, 24);
        let cell = CellPixels::new(9, 18);
        let t0 = Instant::now();
        pane.fit_at(size(81, 24), cell, Some(t0));
        pane.fit_at(size(82, 24), cell, Some(t0));
        pane.flush_resize();
        assert_eq!(resizes.lock().unwrap().last().unwrap().cells, size(82, 24));
        pane.fit_at(size(83, 24), cell, Some(t0));
        pane.fit_at(size(90, 24), cell, None);
        pane.flush_resize();
        let sent = resizes.lock().unwrap().clone();
        assert_eq!(sent.last().unwrap().cells, size(90, 24));
        assert_eq!(sent.len(), 3, "the superseded size is never sent");
    }

    #[test]
    fn send_writes_to_the_pty() {
        let (mut pane, _, written) = pane(80, 24);
        pane.send(b"ls\r");
        assert_eq!(*written.lock().unwrap(), b"ls\r");
    }
}
