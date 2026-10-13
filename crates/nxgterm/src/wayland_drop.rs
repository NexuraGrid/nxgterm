//! Drag and drop on Wayland, which winit does not implement: winit only
//! reports `WindowEvent::DroppedFile` on X11, Windows and macOS.
//!
//! [`DropTarget`] joins winit's Wayland connection on its own event queue,
//! creates a `wl_data_device` for each seat, accepts `text/uri-list` offers
//! over the window and posts the dropped paths as
//! [`UserEvent::DroppedPaths`](crate::app::UserEvent::DroppedPaths).
// The uri-list parsing is only used on Wayland, and tested everywhere.
#![cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]

use std::path::PathBuf;

#[cfg(all(unix, not(target_os = "macos")))]
pub use self::wayland::DropTarget;

/// Windows and macOS: winit reports dropped files itself.
#[cfg(not(all(unix, not(target_os = "macos"))))]
#[derive(Debug)]
pub struct DropTarget;

#[cfg(not(all(unix, not(target_os = "macos"))))]
impl DropTarget {
    pub fn attach(
        _window: &winit::window::Window,
        _proxy: winit::event_loop::EventLoopProxy<crate::app::UserEvent>,
    ) -> Option<Self> {
        None
    }
}

/// The local paths of a `text/uri-list` (RFC 2483): one URI per line,
/// `#` comment lines skipped. Only `file://` URIs without a host or with
/// `localhost` are kept, percent-decoded; a path whose decoded bytes are
/// not UTF-8, or with a malformed escape, is kept as written.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(local_path)
        .map(PathBuf::from)
        .collect()
}

/// The decoded path of a `file://` URI on this host.
fn local_path(uri: &str) -> Option<String> {
    let scheme = uri.get(..7)?;
    if !scheme.eq_ignore_ascii_case("file://") {
        return None;
    }
    let rest = &uri[7..];
    let slash = rest.find('/')?;
    let host = &rest[..slash];
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let path = &rest[slash..];
    Some(percent_decode(path).unwrap_or_else(|| path.to_owned()))
}

/// `text` with its `%XX` escapes decoded, or `None` when an escape is
/// malformed or the result is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = tail
                .get(..2)
                .filter(|hex| hex.iter().all(u8::is_ascii_hexdigit))?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(all(unix, not(target_os = "macos")))]
mod wayland {
    use std::error::Error;
    use std::ffi::c_void;
    use std::io::{self, Read};
    use std::os::fd::AsFd;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread::{self, JoinHandle};

    use wayland_client::backend::{Backend, ObjectId};
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_callback::{self, WlCallback};
    use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
    use wayland_client::protocol::wl_data_device_manager::{self, DndAction, WlDataDeviceManager};
    use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::protocol::wl_seat::{self, WlSeat};
    use wayland_client::protocol::wl_surface::WlSurface;
    use wayland_client::{
        Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, event_created_child,
    };
    use winit::event_loop::EventLoopProxy;
    use winit::raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    use winit::window::Window;

    use super::parse_uri_list;
    use crate::app::UserEvent;

    const URI_LIST: &str = "text/uri-list";
    /// Bound on a dropped list; far more than any real selection of files.
    const MAX_LIST: u64 = 1 << 20;

    /// Takes files dropped on the window while it lives. Drop it before
    /// the event loop closes the Wayland display.
    #[derive(Debug)]
    pub struct DropTarget {
        connection: Connection,
        queue: QueueHandle<State>,
        /// Shared with the threads that read drops: once false (set under
        /// the lock) they no longer touch the connection.
        alive: Arc<Mutex<bool>>,
        /// Asks the dispatch thread to return.
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl DropTarget {
        /// A drop target for `window`; `None` when it is not a Wayland
        /// window, or when the setup fails (reported: the terminal works
        /// on without drops).
        pub fn attach(window: &Window, proxy: EventLoopProxy<UserEvent>) -> Option<Self> {
            let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
                return None;
            };
            let RawWindowHandle::Wayland(surface) = window.window_handle().ok()?.as_raw() else {
                return None;
            };
            // SAFETY: both pointers come from winit's live window; the
            // session drops this target before the event loop closes the
            // display (`App::exiting`).
            match unsafe { Self::start(display.display, surface.surface, proxy) } {
                Ok(target) => Some(target),
                Err(error) => {
                    eprintln!("nxgterm: drag and drop is unavailable: {error}");
                    None
                }
            }
        }

        /// # Safety
        ///
        /// `display` must be a live `wl_display` and `surface` a live
        /// `wl_surface` on it, the display outliving the target.
        unsafe fn start(
            display: NonNull<c_void>,
            surface: NonNull<c_void>,
            proxy: EventLoopProxy<UserEvent>,
        ) -> Result<Self, Box<dyn Error>> {
            // SAFETY: guaranteed by the caller; a foreign display is never
            // closed by the backend.
            let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
            let connection = Connection::from_backend(backend);
            // SAFETY: guaranteed by the caller.
            let surface =
                unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) }?;
            let (globals, queue) = registry_queue_init::<State>(&connection)?;
            let handle = queue.handle();
            let manager: WlDataDeviceManager = globals.bind(&handle, 1..=3, ())?;
            let alive = Arc::new(Mutex::new(true));
            let stop = Arc::new(AtomicBool::new(false));
            let state = State {
                connection: connection.clone(),
                proxy,
                surface: surface.protocol_id(),
                manager,
                offer: None,
                alive: alive.clone(),
                stop: stop.clone(),
            };
            globals.contents().with_list(|list| {
                for global in list
                    .iter()
                    .filter(|g| g.interface == WlSeat::interface().name)
                {
                    state.add_seat(globals.registry(), global.name, &handle);
                }
            });
            connection.flush()?;
            let thread = thread::Builder::new()
                .name("wayland-drop".into())
                .spawn(move || state.run(queue))?;
            Ok(Self {
                connection,
                queue: handle,
                alive,
                stop,
                thread: Some(thread),
            })
        }
    }

    impl Drop for DropTarget {
        fn drop(&mut self) {
            *self.alive.lock().unwrap_or_else(PoisonError::into_inner) = false;
            self.stop.store(true, Ordering::Relaxed);
            // The compositor answers the sync on our queue, which wakes the
            // dispatch thread; a broken connection ends it with an error.
            self.connection.display().sync(&self.queue, ());
            let _ = self.connection.flush();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// What the dispatch thread owns.
    struct State {
        connection: Connection,
        proxy: EventLoopProxy<UserEvent>,
        /// Protocol id of the window's `wl_surface`.
        surface: u32,
        manager: WlDataDeviceManager,
        /// The offer being dragged, and whether it was accepted.
        offer: Option<(WlDataOffer, bool)>,
        alive: Arc<Mutex<bool>>,
        stop: Arc<AtomicBool>,
    }

    /// What an offer advertised: its mime types, and the drag-and-drop
    /// action the compositor chose (`DndAction` bits, 0 for none).
    #[derive(Default)]
    struct OfferData {
        mimes: Mutex<Vec<String>>,
        action: AtomicU32,
    }

    /// Whether `offer` may be finished: `finish` is a protocol error, which
    /// would close winit's connection too, unless the compositor chose an
    /// action other than none or ask.
    fn finishable(offer: &WlDataOffer) -> bool {
        offer.version() >= 3
            && offer.data::<OfferData>().is_some_and(|data| {
                let action = data.action.load(Ordering::Relaxed);
                action == DndAction::Copy.bits() || action == DndAction::Move.bits()
            })
    }

    impl State {
        fn run(mut self, mut queue: EventQueue<Self>) {
            while !self.stop.load(Ordering::Relaxed) {
                if let Err(error) = queue.blocking_dispatch(&mut self) {
                    eprintln!("nxgterm: drag and drop stopped: {error}");
                    return;
                }
            }
        }

        fn add_seat(&self, registry: &WlRegistry, name: u32, handle: &QueueHandle<Self>) {
            let seat: WlSeat = registry.bind(name, 1, handle, ());
            self.manager.get_data_device(&seat, handle, ());
        }

        /// Destroys the offer being dragged, if any.
        fn forget(&mut self) {
            if let Some((offer, _)) = self.offer.take() {
                offer.destroy();
            }
        }

        /// Reads the dropped list on its own thread: the source writes it
        /// only after the request is flushed, and may take its time.
        fn receive(&self, offer: WlDataOffer) {
            let (reader, writer) = match io::pipe() {
                Ok(pipe) => pipe,
                Err(error) => {
                    eprintln!("nxgterm: cannot receive a drop: {error}");
                    offer.destroy();
                    return;
                }
            };
            offer.receive(URI_LIST.into(), writer.as_fd());
            // Our copy of the write end goes, so the read ends at EOF.
            drop(writer);
            if let Err(error) = self.connection.flush() {
                eprintln!("nxgterm: cannot receive a drop: {error}");
                return;
            }
            let connection = self.connection.clone();
            let proxy = self.proxy.clone();
            let alive = self.alive.clone();
            let stop = self.stop.clone();
            let spawned = thread::Builder::new()
                .name("wayland-drop-read".into())
                .spawn(move || {
                    let mut list = Vec::new();
                    let read = reader.take(MAX_LIST).read_to_end(&mut list);
                    {
                        let alive = alive.lock().unwrap_or_else(PoisonError::into_inner);
                        if *alive {
                            if finishable(&offer) {
                                offer.finish();
                            }
                            offer.destroy();
                            let _ = connection.flush();
                        }
                    }
                    if let Err(error) = read {
                        eprintln!("nxgterm: cannot read a drop: {error}");
                        return;
                    }
                    let paths = parse_uri_list(&String::from_utf8_lossy(&list));
                    if !paths.is_empty()
                        && proxy.send_event(UserEvent::DroppedPaths(paths)).is_err()
                    {
                        // The event loop is gone.
                        stop.store(true, Ordering::Relaxed);
                    }
                });
            if let Err(error) = spawned {
                eprintln!("nxgterm: cannot read a drop: {error}");
            }
        }
    }

    impl Dispatch<WlRegistry, GlobalListContents> for State {
        fn event(
            state: &mut Self,
            registry: &WlRegistry,
            event: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            handle: &QueueHandle<Self>,
        ) {
            // A seat plugged in later.
            if let wl_registry::Event::Global {
                name, interface, ..
            } = event
            {
                if interface == WlSeat::interface().name {
                    state.add_seat(registry, name, handle);
                }
            }
        }
    }

    impl Dispatch<WlDataDevice, ()> for State {
        fn event(
            state: &mut Self,
            _: &WlDataDevice,
            event: wl_data_device::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                wl_data_device::Event::Enter {
                    serial,
                    surface,
                    id,
                    ..
                } => {
                    state.forget();
                    let Some(offer) = id else {
                        return;
                    };
                    let ours = surface.id().protocol_id() == state.surface;
                    let uri_list = ours && offers(&offer, URI_LIST);
                    if uri_list {
                        offer.accept(serial, Some(URI_LIST.into()));
                        if offer.version() >= 3 {
                            offer.set_actions(DndAction::Copy, DndAction::Copy);
                        }
                    } else {
                        // Shows the "blocked" cursor.
                        offer.accept(serial, None);
                    }
                    state.offer = Some((offer, uri_list));
                }
                wl_data_device::Event::Leave => state.forget(),
                wl_data_device::Event::Drop => match state.offer.take() {
                    Some((offer, true)) => state.receive(offer),
                    Some((offer, false)) => offer.destroy(),
                    None => {}
                },
                // Clipboard offers: arboard reads the clipboard on its own
                // connection.
                wl_data_device::Event::Selection { id: Some(offer) } => offer.destroy(),
                _ => {}
            }
        }

        event_created_child!(State, WlDataDevice, [
            wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, OfferData::default()),
        ]);
    }

    /// Whether `offer` advertised `mime`.
    fn offers(offer: &WlDataOffer, mime: &str) -> bool {
        offer.data::<OfferData>().is_some_and(|data| {
            data.mimes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|m| m == mime)
        })
    }

    impl Dispatch<WlDataOffer, OfferData> for State {
        fn event(
            _: &mut Self,
            _: &WlDataOffer,
            event: wl_data_offer::Event,
            data: &OfferData,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                wl_data_offer::Event::Offer { mime_type } => data
                    .mimes
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(mime_type),
                wl_data_offer::Event::Action {
                    dnd_action: WEnum::Value(action),
                } => data.action.store(action.bits(), Ordering::Relaxed),
                _ => {}
            }
        }
    }

    /// Objects whose events are not needed.
    macro_rules! ignore_events {
        ($($interface:ty => $event:ty),* $(,)?) => {$(
            impl Dispatch<$interface, ()> for State {
                fn event(
                    _: &mut Self,
                    _: &$interface,
                    _: $event,
                    _: &(),
                    _: &Connection,
                    _: &QueueHandle<Self>,
                ) {
                }
            }
        )*};
    }

    ignore_events!(
        WlSeat => wl_seat::Event,
        WlDataDeviceManager => wl_data_device_manager::Event,
        WlCallback => wl_callback::Event,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str) -> Vec<String> {
        parse_uri_list(text)
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            paths("file:///home/me/a.txt\r\nfile:///tmp/b\r\n"),
            ["/home/me/a.txt", "/tmp/b"]
        );
        assert_eq!(paths("file:///a\nfile:///b"), ["/a", "/b"], "LF only");
    }

    #[test]
    fn localhost_is_the_only_host_kept() {
        assert_eq!(
            paths("file://localhost/etc/hosts\nfile://other/etc/x\n"),
            ["/etc/hosts"]
        );
    }

    #[test]
    fn comments_blank_lines_and_other_schemes_are_skipped() {
        let text =
            "# from dolphin\r\n\r\nhttps://example.com/x\r\nsmb://nas/share\r\nfile:///ok\r\n";
        assert_eq!(paths(text), ["/ok"]);
        assert_eq!(paths(""), Vec::<String>::new());
    }

    #[test]
    fn escapes_are_decoded_as_utf8() {
        assert_eq!(
            paths("file:///home/me/My%20Files/caf%C3%A9%2B.txt"),
            ["/home/me/My Files/café+.txt"]
        );
    }

    #[test]
    fn invalid_escapes_are_kept_raw() {
        assert_eq!(paths("file:///a%zzb%2"), ["/a%zzb%2"], "malformed");
        assert_eq!(paths("file:///x%E9y"), ["/x%E9y"], "not UTF-8");
    }
}
