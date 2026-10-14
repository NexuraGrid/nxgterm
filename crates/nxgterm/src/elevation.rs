//! Running as Administrator on Windows: detecting an elevated process and
//! relaunching through UAC for `--elevate`.

use std::borrow::Cow;
use std::ffi::OsString;
use std::io;
use std::path::Path;

use nxg_config::WorkingDirectory;
use std::sync::OnceLock;

/// Whether this process runs elevated (as Administrator). Always false
/// outside Windows. Asked once; elevation cannot change while running.
pub fn is_elevated() -> bool {
    static ELEVATED: OnceLock<bool> = OnceLock::new();
    *ELEVATED.get_or_init(platform::is_elevated)
}

/// The window title for a pane `label`, marked when the process is elevated.
pub fn window_title(label: &str, elevated: bool) -> Cow<'_, str> {
    if elevated {
        Cow::Owned(format!("{label} (Administrator)"))
    } else {
        Cow::Borrowed(label)
    }
}

/// Starts this executable again as Administrator, which shows the UAC
/// prompt, forwarding `config` and `working_directory` (made absolute,
/// since the elevated process may start elsewhere). Returns once the new
/// process is launched.
#[cfg(windows)]
pub fn relaunch_elevated(
    config: Option<&Path>,
    working_directory: Option<&WorkingDirectory>,
) -> io::Result<()> {
    let unicode = |value: Option<OsString>, flag: &str| {
        value
            .map(|value| {
                value.into_string().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("the {flag} path is not valid Unicode"),
                    )
                })
            })
            .transpose()
    };
    let config = config.map(std::path::absolute).transpose()?;
    let config = unicode(config.map(Into::into), "--config")?;
    let dir = std::env::current_dir().ok();
    let working_directory =
        working_directory.map(|wanted| working_directory_arg(wanted, dir.as_deref()));
    let working_directory = unicode(working_directory, "--working-directory")?;
    let exe = std::env::current_exe()?;
    let parameters = relaunch_parameters(config.as_deref(), working_directory.as_deref());
    platform::run_as(&exe, &parameters, dir.as_deref())
}

#[cfg(not(windows))]
pub fn relaunch_elevated(
    _config: Option<&Path>,
    _working_directory: Option<&WorkingDirectory>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "--elevate is only supported on Windows",
    ))
}

/// Adds a "New window (Administrator)" task, which runs this executable
/// with `--elevate`, to the taskbar jump list. Failures are only logged:
/// the jump list never stops the terminal from starting. Call it on the
/// main thread before the event loop; it joins (or creates) the thread's
/// single-threaded COM apartment, which winit uses too.
#[cfg(windows)]
pub fn register_jump_list() {
    if let Err(error) = jump_list::register() {
        eprintln!("nxgterm: cannot add the Administrator jump list task: {error}");
    }
}

#[cfg(not(windows))]
pub fn register_jump_list() {}

/// The command-line parameters for the elevated relaunch: the config path
/// and the start directory, if any, and never `--elevate`, so the child
/// cannot relaunch again.
#[cfg_attr(not(windows), allow(dead_code))]
fn relaunch_parameters(config: Option<&str>, working_directory: Option<&str>) -> String {
    let config = config.map(|path| format!("--config {}", quote_arg(path)));
    let working_directory =
        working_directory.map(|dir| format!("--working-directory {}", quote_arg(dir)));
    [config, working_directory]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `--working-directory` value for the elevated relaunch, which starts
/// in System32: `inherit` and relative paths become absolute against
/// `cwd`, this process's directory.
#[cfg_attr(not(windows), allow(dead_code))]
fn working_directory_arg(wanted: &WorkingDirectory, cwd: Option<&Path>) -> OsString {
    match (wanted, cwd) {
        (WorkingDirectory::Auto, _) => "auto".into(),
        (WorkingDirectory::Home, _) => "home".into(),
        (WorkingDirectory::Inherit, Some(cwd)) => cwd.into(),
        (WorkingDirectory::Inherit, None) => "inherit".into(),
        (WorkingDirectory::Path(path), Some(cwd))
            if path.is_relative() && !path.starts_with("~") =>
        {
            cwd.join(path).into()
        }
        (WorkingDirectory::Path(path), _) => path.into(),
    }
}

/// Quotes one argument so `CommandLineToArgvW` and the MSVC runtime read it
/// back unchanged: backslashes are literal except before a `"`, where they
/// are doubled, and the `"` itself is escaped.
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_arg(arg: &str) -> Cow<'_, str> {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return Cow::Borrowed(arg);
    }
    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    // Doubled so the closing quote is not escaped.
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    Cow::Owned(quoted)
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, GetLastError};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    pub fn is_elevated() -> bool {
        let mut token = std::ptr::null_mut();
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no
        // closing, and `token` is a valid out pointer for the opened handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut length = 0;
        // SAFETY: `token` is open with TOKEN_QUERY, and the buffer is a
        // TOKEN_ELEVATION of exactly the size passed.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                (&raw mut elevation).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        };
        // SAFETY: `token` was opened above and is closed exactly once.
        unsafe { CloseHandle(token) };
        ok != 0 && elevation.TokenIsElevated != 0
    }

    /// `ShellExecuteW` with the `runas` verb: launches `exe` elevated.
    pub fn run_as(exe: &Path, parameters: &str, dir: Option<&Path>) -> io::Result<()> {
        let verb = wide(OsStr::new("runas"));
        let file = wide(exe.as_os_str());
        let parameters = wide(OsStr::new(parameters));
        let dir = dir.map(|dir| wide(dir.as_os_str()));
        let dir_ptr = dir.as_ref().map_or(std::ptr::null(), |dir| dir.as_ptr());
        // ShellExecute may delegate to shell extensions that need COM, so
        // initialise it first as Microsoft recommends. The result is ignored:
        // an already initialised thread works too, and the process exits
        // right after the relaunch, so it is never paired with CoUninitialize.
        // SAFETY: plain COM initialisation of this thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        // SAFETY: every string is NUL-terminated UTF-16 that outlives the
        // call; a null window handle and directory are allowed.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                dir_ptr,
                SW_SHOWNORMAL,
            )
        } as isize;
        // Values above 32 mean success; the rest are error codes.
        if result > 32 {
            return Ok(());
        }
        // SAFETY: plain Win32 call reading this thread's last error.
        let error = unsafe { GetLastError() };
        Err(if error == ERROR_CANCELLED {
            io::Error::other("elevation was cancelled")
        } else {
            io::Error::other(format!(
                "cannot relaunch as Administrator (ShellExecute error {result})"
            ))
        })
    }

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(windows)]
mod jump_list {
    use std::mem::ManuallyDrop;

    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::SHStrDupW;
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
    };
    use windows::core::{HSTRING, Interface, Result};

    const TITLE: &str = "New window (Administrator)";

    pub fn register() -> Result<()> {
        // SAFETY: plain COM initialisation of this thread. COM stays
        // initialised for the life of the process, so it is never paired
        // with CoUninitialize.
        let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        // S_FALSE (already initialised) is a success code; a different
        // apartment already set up on this thread works as well.
        if init.is_err() && init != RPC_E_CHANGED_MODE {
            return Err(init.into());
        }
        let exe = HSTRING::from(std::env::current_exe()?.as_os_str());
        // SAFETY: COM is initialised on this thread; every string passed
        // below is a NUL-terminated HSTRING or buffer that outlives the
        // call that reads it, and the shell copies what it keeps.
        unsafe {
            let list: ICustomDestinationList =
                CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
            let mut min_slots = 0;
            let _removed: IObjectArray = list.BeginList(&mut min_slots)?;

            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&exe)?;
            link.SetArguments(&HSTRING::from("--elevate"))?;
            link.SetIconLocation(&exe, 0)?;
            link.SetDescription(&HSTRING::from("Open nxgterm as Administrator"))?;
            // A task's visible name is the link's title property.
            let title = string_value(TITLE)?;
            let store: IPropertyStore = link.cast()?;
            store.SetValue(&PKEY_Title, &title)?;
            store.Commit()?;

            let tasks: IObjectCollection =
                CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
            tasks.AddObject(&link)?;
            list.AddUserTasks(&tasks.cast::<IObjectArray>()?)?;
            list.CommitList()
        }
    }

    /// A `VT_LPWSTR` property value that owns its string. `PROPVARIANT`
    /// frees the string with `PropVariantClear` when dropped, so it must
    /// come from the COM allocator, as `InitPropVariantFromString` does with
    /// `SHStrDupW`. Pointing it at a Rust buffer frees that buffer twice and
    /// corrupts the heap.
    pub(super) fn string_value(text: &str) -> Result<PROPVARIANT> {
        let text = HSTRING::from(text);
        // SAFETY: `text` is a NUL-terminated string that outlives the call;
        // the copy is owned by the returned value from here on.
        let copy = unsafe { SHStrDupW(&text)? };
        Ok(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_LPWSTR,
                    Anonymous: PROPVARIANT_0_0_0 { pwszVal: copy },
                    ..Default::default()
                }),
            },
        })
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn is_elevated() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0.10.0 freed the task title twice, and the heap corruption killed
    // nxgterm on every start. Run the whole real path a few times; it must
    // succeed, so the test cannot pass by bailing out before the title.
    #[cfg(windows)]
    #[test]
    fn registering_the_jump_list_keeps_the_heap_intact() {
        for _ in 0..3 {
            jump_list::register().unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn string_values_own_and_free_their_string() {
        use windows::Win32::System::Variant::VT_LPWSTR;

        for _ in 0..100 {
            let value = jump_list::string_value("New window (Administrator)").unwrap();
            assert_eq!(value.vt(), VT_LPWSTR);
            // SAFETY: a VT_LPWSTR value holds a NUL-terminated string.
            let text = unsafe { value.Anonymous.Anonymous.Anonymous.pwszVal.to_string() };
            assert_eq!(text.unwrap(), "New window (Administrator)");
        }
    }

    #[test]
    fn plain_arguments_are_left_alone() {
        assert_eq!(quote_arg(r"C:\x\a.toml"), r"C:\x\a.toml");
        assert_eq!(quote_arg(r"\\server\share\a"), r"\\server\share\a");
    }

    #[test]
    fn arguments_with_spaces_or_quotes_are_quoted() {
        assert_eq!(quote_arg(""), r#""""#);
        assert_eq!(quote_arg(r"C:\My Files\a.toml"), r#""C:\My Files\a.toml""#);
        assert_eq!(quote_arg("a\tb"), "\"a\tb\"");
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
    }

    #[test]
    fn backslashes_before_quotes_are_doubled() {
        // A trailing backslash would otherwise escape the closing quote.
        assert_eq!(quote_arg(r"C:\My Dir\"), r#""C:\My Dir\\""#);
        assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quote_arg(r"a\\ b"), r#""a\\ b""#);
    }

    #[test]
    fn relaunch_forwards_the_config_but_not_elevate() {
        assert_eq!(relaunch_parameters(None, None), "");
        assert_eq!(
            relaunch_parameters(Some(r"C:\x\a.toml"), None),
            r"--config C:\x\a.toml"
        );
        assert_eq!(
            relaunch_parameters(Some(r"C:\My Files\a.toml"), None),
            r#"--config "C:\My Files\a.toml""#
        );
    }

    #[test]
    fn relaunch_forwards_the_working_directory() {
        assert_eq!(
            relaunch_parameters(None, Some("home")),
            "--working-directory home"
        );
        assert_eq!(
            relaunch_parameters(Some(r"C:\a.toml"), Some(r"D:\My Code")),
            r#"--config C:\a.toml --working-directory "D:\My Code""#
        );
    }

    #[test]
    fn forwarded_working_directories_do_not_depend_on_the_cwd() {
        let cwd = Some(Path::new("/work"));
        let arg = |wanted: WorkingDirectory| working_directory_arg(&wanted, cwd);
        assert_eq!(arg(WorkingDirectory::Auto), "auto");
        assert_eq!(arg(WorkingDirectory::Home), "home");
        assert_eq!(arg(WorkingDirectory::Inherit), "/work");
        assert_eq!(
            arg(WorkingDirectory::Path("sub".into())),
            Path::new("/work").join("sub").into_os_string()
        );
        assert_eq!(arg(WorkingDirectory::Path("~/src".into())), "~/src");
        let absolute = std::env::temp_dir();
        assert_eq!(
            arg(WorkingDirectory::Path(absolute.clone())),
            absolute.into_os_string()
        );
        assert_eq!(
            working_directory_arg(&WorkingDirectory::Inherit, None),
            "inherit"
        );
    }

    #[test]
    fn elevated_titles_get_a_suffix() {
        assert_eq!(window_title("pwsh", false), "pwsh");
        assert_eq!(window_title("pwsh", true), "pwsh (Administrator)");
    }

    #[test]
    fn non_windows_cannot_elevate() {
        if cfg!(not(windows)) {
            assert!(!is_elevated());
            let error = relaunch_elevated(None, None).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        }
    }
}
