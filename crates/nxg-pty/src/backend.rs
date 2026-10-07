//! PTY backend selection: which adapters to try, in which order.

use std::any::Any;

/// Environment variable that overrides the PTY backend.
pub const ENV_VAR: &str = "NXGTERM_PTY";

/// A PTY adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Unix pty, or ConPTY on Windows 10 1809+ / Server 2019+.
    Native,
    /// Embedded winpty, for Windows without ConPTY (Server 2016).
    Winpty,
}

impl Backend {
    /// Short identifier for logs, e.g. `"native"` or `"winpty"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Winpty => "winpty",
        }
    }
}

/// Backends tried by default on this platform.
pub fn auto_backends() -> &'static [Backend] {
    backend_order(None, cfg!(windows))
}

/// Backends to try given the `NXGTERM_PTY` value.
pub fn backends_from_env(env: Option<&str>) -> &'static [Backend] {
    backend_order(env, cfg!(windows))
}

/// Backend candidates, in the order they are tried.
///
/// `env` (`auto`, `native` or `winpty`, any case, surrounding spaces
/// ignored) forces a single backend; anything else means `auto`: native
/// then winpty on Windows, native alone elsewhere.
pub(crate) fn backend_order(env: Option<&str>, windows: bool) -> &'static [Backend] {
    match env.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("native") => &[Backend::Native],
        Some("winpty") => &[Backend::Winpty],
        _ if windows => &[Backend::Native, Backend::Winpty],
        _ => &[Backend::Native],
    }
}

/// Maps the ConPTY probe to the native backend's precondition.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn require_conpty(available: bool) -> Result<(), String> {
    if available {
        Ok(())
    } else {
        Err(
            "ConPTY is unavailable (kernel32.dll has no CreatePseudoConsole; \
             needs Windows 10 1809 / Server 2019 or newer)"
                .into(),
        )
    }
}

/// Describes a panic payload caught from a backend.
pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> String {
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
    match message {
        Some(message) => format!("panicked: {message}"),
        None => "panicked".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Backend::{Native, Winpty};

    #[test]
    fn auto_tries_winpty_after_native_on_windows() {
        assert_eq!(backend_order(None, true), [Native, Winpty]);
        assert_eq!(backend_order(Some("auto"), true), [Native, Winpty]);
    }

    #[test]
    fn auto_is_native_only_elsewhere() {
        assert_eq!(backend_order(None, false), [Native]);
        assert_eq!(backend_order(Some("auto"), false), [Native]);
    }

    #[test]
    fn env_forces_a_single_backend() {
        assert_eq!(backend_order(Some("native"), true), [Native]);
        assert_eq!(backend_order(Some("winpty"), true), [Winpty]);
        assert_eq!(backend_order(Some(" WinPTY "), true), [Winpty]);
        // Forcing winpty off Windows is honoured; it then fails with a reason.
        assert_eq!(backend_order(Some("winpty"), false), [Winpty]);
    }

    #[test]
    fn unknown_env_values_mean_auto() {
        assert_eq!(backend_order(Some(""), true), [Native, Winpty]);
        assert_eq!(backend_order(Some("conpty"), false), [Native]);
    }

    #[test]
    fn names_are_stable_log_identifiers() {
        assert_eq!(Native.name(), "native");
        assert_eq!(Winpty.name(), "winpty");
    }

    #[test]
    fn conpty_present_allows_the_native_backend() {
        assert_eq!(require_conpty(true), Ok(()));
    }

    #[test]
    fn missing_conpty_is_a_clear_error() {
        let error = require_conpty(false).unwrap_err();
        assert!(error.contains("ConPTY"), "{error}");
        assert!(error.contains("CreatePseudoConsole"), "{error}");
    }

    #[test]
    fn panic_messages_are_extracted() {
        let text: Box<dyn Any + Send> = Box::new("boom");
        let owned: Box<dyn Any + Send> = Box::new(String::from("bang"));
        let other: Box<dyn Any + Send> = Box::new(7_u8);
        assert_eq!(panic_message(text.as_ref()), "panicked: boom");
        assert_eq!(panic_message(owned.as_ref()), "panicked: bang");
        assert_eq!(panic_message(other.as_ref()), "panicked");
    }
}
