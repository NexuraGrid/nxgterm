//! A child process in a winpty console, through the winpty C API
//! (`winpty.h`) of the unpacked `winpty.dll`.

use std::ffi::{OsStr, OsString, c_void};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::OnceLock;

use nxg_core::WinSize;
use nxg_core::ports::{ChildProcess, PtyControl, PtySession};
use windows_sys::Win32::Foundation::{HANDLE, HMODULE, WAIT_FAILED};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
};
use windows_sys::Win32::System::Threading::{INFINITE, TerminateProcess, WaitForSingleObject};

use super::cmdline::command_line;
use super::env::child_env;
use super::unpack;

/// Upstream winpty release of the embedded binaries.
const VERSION: &str = "0.4.3";

/// Embedded binaries; `winpty.dll` starts the agent from its own directory.
const FILES: [(&str, &[u8]); 2] = [
    ("winpty.dll", include_bytes!("../../winpty/winpty.dll")),
    (
        "winpty-agent.exe",
        include_bytes!("../../winpty/winpty-agent.exe"),
    ),
];

/// `WINPTY_FLAG_COLOR_ESCAPES`: emit ANSI colours, not attribute records.
const FLAG_COLOR_ESCAPES: u64 = 0x4;
/// `WINPTY_SPAWN_FLAG_AUTO_SHUTDOWN`: stop the agent when the child exits.
const SPAWN_FLAG_AUTO_SHUTDOWN: u64 = 0x1;

/// `winpty_error_ptr_t`.
type ErrorPtr = *mut c_void;

/// Declares the winpty entry points and resolves them from the DLL.
macro_rules! winpty_api {
    ($($field:ident = $symbol:literal: $ty:ty;)*) => {
        struct Api {
            $($field: $ty,)*
        }

        impl Api {
            /// Resolves every entry point.
            ///
            /// # Safety
            /// `lib` must be a loaded `winpty.dll` that is never unloaded,
            /// whose exports have the declared signatures.
            unsafe fn resolve(lib: HMODULE) -> Result<Self, String> {
                Ok(Self {
                    $($field: {
                        let symbol = concat!($symbol, "\0");
                        // SAFETY: `symbol` is NUL-terminated and `lib` is loaded.
                        let address = unsafe { GetProcAddress(lib, symbol.as_ptr()) }
                            .ok_or_else(|| format!("winpty.dll has no {}", $symbol))?;
                        // SAFETY: the export has this signature (winpty.h).
                        unsafe {
                            std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(
                                address,
                            )
                        }
                    },)*
                })
            }
        }
    };
}

winpty_api! {
    error_msg = "winpty_error_msg": unsafe extern "C" fn(ErrorPtr) -> *const u16;
    error_free = "winpty_error_free": unsafe extern "C" fn(ErrorPtr);
    config_new = "winpty_config_new": unsafe extern "C" fn(u64, *mut ErrorPtr) -> *mut c_void;
    config_free = "winpty_config_free": unsafe extern "C" fn(*mut c_void);
    config_set_initial_size = "winpty_config_set_initial_size":
        unsafe extern "C" fn(*mut c_void, i32, i32);
    open = "winpty_open": unsafe extern "C" fn(*const c_void, *mut ErrorPtr) -> *mut c_void;
    conin_name = "winpty_conin_name": unsafe extern "C" fn(*mut c_void) -> *const u16;
    conout_name = "winpty_conout_name": unsafe extern "C" fn(*mut c_void) -> *const u16;
    spawn_config_new = "winpty_spawn_config_new": unsafe extern "C" fn(
        u64,
        *const u16,
        *const u16,
        *const u16,
        *const u16,
        *mut ErrorPtr,
    ) -> *mut c_void;
    spawn_config_free = "winpty_spawn_config_free": unsafe extern "C" fn(*mut c_void);
    spawn = "winpty_spawn": unsafe extern "C" fn(
        *mut c_void,
        *const c_void,
        *mut HANDLE,
        *mut HANDLE,
        *mut u32,
        *mut ErrorPtr,
    ) -> i32;
    set_size = "winpty_set_size": unsafe extern "C" fn(*mut c_void, i32, i32, *mut ErrorPtr) -> i32;
    free = "winpty_free": unsafe extern "C" fn(*mut c_void);
}

impl Api {
    /// Unpacks and loads `winpty.dll` once per process; it stays loaded.
    fn get() -> Result<&'static Self, String> {
        static API: OnceLock<Result<Api, String>> = OnceLock::new();
        API.get_or_init(|| Self::load(&unpacked_dir()?.join("winpty.dll")))
            .as_ref()
            .map_err(Clone::clone)
    }

    fn load(path: &Path) -> Result<Self, String> {
        let wide = wide_nul(path.as_os_str());
        // SAFETY: `wide` is NUL-terminated. The altered search path makes
        // the DLL's own directory part of its dependency search.
        let lib = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                ptr::null_mut(),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if lib.is_null() {
            let error = io::Error::last_os_error();
            return Err(format!("cannot load {}: {error}", path.display()));
        }
        // SAFETY: `lib` is winpty.dll 0.4.3 and is never freed.
        unsafe { Self::resolve(lib) }
    }

    /// Describes a winpty failure and frees `error`.
    fn error(&self, what: &str, error: ErrorPtr) -> String {
        if error.is_null() {
            return format!("{what} failed");
        }
        // SAFETY: `error` came from winpty and is freed exactly once here.
        let message = unsafe {
            let message = wide_str((self.error_msg)(error));
            (self.error_free)(error);
            message
        };
        let message = message.to_string_lossy();
        match message.trim() {
            "" => format!("{what} failed"),
            message => format!("{what}: {message}"),
        }
    }
}

/// Unpacks the embedded binaries once per process.
fn unpacked_dir() -> Result<&'static Path, String> {
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = unpack::data_root(
            std::env::var_os("LOCALAPPDATA"),
            std::env::var_os("APPDATA"),
            std::env::temp_dir(),
        );
        let contents = FILES.map(|(_, bytes)| bytes);
        let dir = unpack::unpack_dir(&root, &unpack::tag(VERSION, &contents));
        unpack::unpack(&dir, &FILES)
            .map_err(|e| format!("cannot unpack winpty to {}: {e}", dir.display()))?;
        Ok(dir)
    })
    .as_ref()
    .map(PathBuf::as_path)
    .map_err(Clone::clone)
}

/// A winpty object released with its `*_free` function on drop.
struct Owned {
    ptr: *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: `ptr` is a live object of the kind `free` releases.
        unsafe { (self.free)(self.ptr) }
    }
}

// SAFETY: winpty objects are not bound to the thread that created them, and
// each one is used by a single owner at a time.
unsafe impl Send for Owned {}

/// Starts `argv` (program first) in a new winpty console of `size`, with
/// this process's environment plus `overrides`.
pub(crate) fn spawn(
    size: WinSize,
    argv: &[OsString],
    overrides: &[(&str, &str)],
) -> Result<PtySession, String> {
    let wide_argv: Vec<Vec<u16>> = argv.iter().map(|arg| arg.encode_wide().collect()).collect();
    let mut cmdline = command_line(&wide_argv)?;
    cmdline.push(0);
    let env = env_block(&child_env(std::env::vars_os(), overrides));
    let api = Api::get()?;
    let (cols, rows) = cells(size);

    let mut error = ptr::null_mut();
    // SAFETY (all winpty calls below): arguments are live winpty objects
    // created here, NUL-terminated UTF-16 strings, or valid out-pointers.
    let config = unsafe { (api.config_new)(FLAG_COLOR_ESCAPES, &mut error) };
    if config.is_null() {
        return Err(api.error("winpty_config_new", error));
    }
    let config = Owned {
        ptr: config,
        free: api.config_free,
    };
    unsafe { (api.config_set_initial_size)(config.ptr, cols, rows) };
    let pty = unsafe { (api.open)(config.ptr, &mut error) };
    if pty.is_null() {
        return Err(api.error("winpty_open", error));
    }
    let pty = Owned {
        ptr: pty,
        free: api.free,
    };
    drop(config);

    // SAFETY: the names are NUL-terminated strings owned by `pty`.
    let conout = open_pipe(unsafe { wide_str((api.conout_name)(pty.ptr)) }, false)?;
    let conin = open_pipe(unsafe { wide_str((api.conin_name)(pty.ptr)) }, true)?;

    let spawn_config = unsafe {
        (api.spawn_config_new)(
            SPAWN_FLAG_AUTO_SHUTDOWN,
            ptr::null(),
            cmdline.as_ptr(),
            ptr::null(),
            env.as_ptr(),
            &mut error,
        )
    };
    if spawn_config.is_null() {
        return Err(api.error("winpty_spawn_config_new", error));
    }
    let spawn_config = Owned {
        ptr: spawn_config,
        free: api.spawn_config_free,
    };
    let mut process: HANDLE = ptr::null_mut();
    let mut create_error = 0_u32;
    let spawned = unsafe {
        (api.spawn)(
            pty.ptr,
            spawn_config.ptr,
            &mut process,
            ptr::null_mut(),
            &mut create_error,
            &mut error,
        )
    };
    if spawned == 0 {
        let message = api.error("winpty_spawn", error);
        return Err(if create_error == 0 {
            message
        } else {
            let program = argv
                .first()
                .map(|p| p.to_string_lossy())
                .unwrap_or_default();
            let cause = io::Error::from_raw_os_error(create_error as i32);
            format!("cannot start {program}: {cause}")
        });
    }
    if process.is_null() {
        return Err("winpty_spawn returned no process handle".into());
    }
    // SAFETY: winpty hands over ownership of the child's process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let waiter = process.try_clone().map_err(|e| e.to_string())?;

    Ok(PtySession {
        reader: Box::new(conout),
        control: Box::new(WinptyControl {
            api,
            conin,
            process,
            pty,
        }),
        child: Box::new(WinptyChild(waiter)),
    })
}

/// winpty sizes in cells; it has no pixel dimensions.
fn cells(size: WinSize) -> (i32, i32) {
    (i32::from(size.cells.cols()), i32::from(size.cells.rows()))
}

/// Opens one end of the agent's console pipes.
fn open_pipe(name: OsString, write: bool) -> Result<File, String> {
    if name.is_empty() {
        return Err("winpty returned an empty pipe name".into());
    }
    OpenOptions::new()
        .read(!write)
        .write(write)
        .open(&name)
        .map_err(|e| format!("cannot open {}: {e}", name.to_string_lossy()))
}

/// A `CreateProcess` environment block: `NAME=value\0` entries, then `\0`.
fn env_block(vars: &[(OsString, OsString)]) -> Vec<u16> {
    let mut block = Vec::new();
    for (name, value) in vars {
        block.extend(name.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

fn wide_nul(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain([0]).collect()
}

/// Copies a NUL-terminated UTF-16 string; null gives an empty string.
///
/// # Safety
/// `ptr` must be null or point to a NUL-terminated UTF-16 string.
unsafe fn wide_str(ptr: *const u16) -> OsString {
    if ptr.is_null() {
        return OsString::new();
    }
    let mut len = 0;
    // SAFETY: the string is NUL-terminated, so every index up to the NUL
    // is in bounds.
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    OsString::from_wide(unsafe { std::slice::from_raw_parts(ptr, len) })
}

struct WinptyControl {
    api: &'static Api,
    conin: File,
    process: OwnedHandle,
    // Freed last: closing the console ends the agent and the reader's pipe.
    pty: Owned,
}

impl Write for WinptyControl {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.conin.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.conin.flush()
    }
}

impl PtyControl for WinptyControl {
    fn resize(&mut self, size: WinSize) -> io::Result<()> {
        let (cols, rows) = cells(size);
        let mut error = ptr::null_mut();
        // SAFETY: `pty` is live and `error` is a valid out-pointer.
        let resized = unsafe { (self.api.set_size)(self.pty.ptr, cols, rows, &mut error) };
        if resized == 0 {
            return Err(io::Error::other(self.api.error("winpty_set_size", error)));
        }
        Ok(())
    }
}

impl Drop for WinptyControl {
    fn drop(&mut self) {
        // The child may already be gone; nothing useful to do on failure.
        // SAFETY: `process` is a live process handle owned by this struct.
        unsafe { TerminateProcess(self.process.as_raw_handle(), 1) };
    }
}

struct WinptyChild(OwnedHandle);

impl ChildProcess for WinptyChild {
    fn wait(&mut self) -> io::Result<()> {
        // SAFETY: the handle is a live process handle owned by `self`.
        let result = unsafe { WaitForSingleObject(self.0.as_raw_handle(), INFINITE) };
        if result == WAIT_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
