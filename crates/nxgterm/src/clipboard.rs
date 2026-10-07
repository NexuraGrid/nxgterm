//! The system clipboard adapter (arboard), plus a stand-in for systems
//! where it cannot start.

use nxg_core::ports::{Clipboard, ClipboardError, ClipboardKind};

/// Whether this system has a PRIMARY selection (X11 and Wayland).
pub const HAS_PRIMARY: bool = cfg!(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "android",
        target_os = "emscripten"
    ))
));

/// The OS clipboard through arboard. Kept for the whole run: on X11 and
/// Wayland the text copied last is served by this object, and is lost
/// when it is dropped.
pub struct SystemClipboard {
    inner: arboard::Clipboard,
}

impl std::fmt::Debug for SystemClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemClipboard").finish_non_exhaustive()
    }
}

impl SystemClipboard {
    pub fn new() -> Result<Self, ClipboardError> {
        let inner = arboard::Clipboard::new().map_err(error)?;
        Ok(Self { inner })
    }
}

fn error(error: arboard::Error) -> ClipboardError {
    ClipboardError(error.to_string())
}

impl Clipboard for SystemClipboard {
    fn get_text(&mut self, kind: ClipboardKind) -> Result<String, ClipboardError> {
        match kind {
            ClipboardKind::Clipboard => self.inner.get_text().map_err(error),
            ClipboardKind::Primary => primary::get(&mut self.inner),
        }
    }

    fn set_text(&mut self, kind: ClipboardKind, text: String) -> Result<(), ClipboardError> {
        match kind {
            ClipboardKind::Clipboard => self.inner.set_text(text).map_err(error),
            ClipboardKind::Primary => primary::set(&mut self.inner, text),
        }
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
mod primary {
    use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};

    use super::{ClipboardError, error};

    pub fn get(clipboard: &mut arboard::Clipboard) -> Result<String, ClipboardError> {
        clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
            .map_err(error)
    }

    pub fn set(clipboard: &mut arboard::Clipboard, text: String) -> Result<(), ClipboardError> {
        clipboard
            .set()
            .clipboard(LinuxClipboardKind::Primary)
            .text(text)
            .map_err(error)
    }
}

#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
)))]
mod primary {
    use super::ClipboardError;

    fn unsupported() -> ClipboardError {
        ClipboardError("no PRIMARY selection on this system".into())
    }

    pub fn get(_: &mut arboard::Clipboard) -> Result<String, ClipboardError> {
        Err(unsupported())
    }

    pub fn set(_: &mut arboard::Clipboard, _: String) -> Result<(), ClipboardError> {
        Err(unsupported())
    }
}

/// Used when the system clipboard cannot start (no display server
/// connection): every call fails with the reason, which callers log.
#[derive(Debug)]
pub struct Unavailable(pub ClipboardError);

impl Clipboard for Unavailable {
    fn get_text(&mut self, _: ClipboardKind) -> Result<String, ClipboardError> {
        Err(self.0.clone())
    }

    fn set_text(&mut self, _: ClipboardKind, _: String) -> Result<(), ClipboardError> {
        Err(self.0.clone())
    }
}

/// The system clipboard, or [`Unavailable`] with a warning.
pub fn system() -> Box<dyn Clipboard> {
    match SystemClipboard::new() {
        Ok(clipboard) => Box::new(clipboard),
        Err(error) => {
            eprintln!("nxgterm: warning: {error}; copy and paste are disabled");
            Box::new(Unavailable(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unavailable_clipboard_reports_its_reason_every_time() {
        let mut clipboard = Unavailable(ClipboardError("no display".into()));
        let reason = Err(ClipboardError("no display".into()));
        assert_eq!(clipboard.get_text(ClipboardKind::Clipboard), reason);
        assert_eq!(
            clipboard.set_text(ClipboardKind::Primary, "x".into()),
            Err(ClipboardError("no display".into()))
        );
    }
}
