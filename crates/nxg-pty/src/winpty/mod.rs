//! winpty adapter: the fallback for Windows without ConPTY (Server 2016).
//!
//! `winpty.dll` and `winpty-agent.exe` (x64, winpty 0.4.3, MIT; see
//! `crates/nxg-pty/winpty/README.md`) are embedded in x86_64 Windows builds
//! and unpacked to `%LOCALAPPDATA%\nxgterm\winpty\<tag>` on first use.

// The pure parts build everywhere so their tests run on every platform.
#[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
mod cmdline;
mod env;
#[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
mod unpack;

#[cfg(all(windows, target_arch = "x86_64"))]
mod session;

#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) use session::spawn;

/// winpty is only bundled for x86_64 Windows.
#[cfg(not(all(windows, target_arch = "x86_64")))]
pub(crate) fn spawn(
    _size: nxg_core::WinSize,
    _argv: &[std::ffi::OsString],
    _env: &[(&str, &str)],
) -> Result<nxg_core::ports::PtySession, String> {
    Err("winpty is only bundled for x86_64 Windows".into())
}
