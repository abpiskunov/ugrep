//! C-ABI entries of `terminal()` and `set_terminal_hyperlink()` for src/ugrep.cpp: a table of C callbacks for
//! what stays in C++ (`Static::output_stat*`, `Output::Tree`, the `std::string` hyperlink options, `usage`) and
//! the operating system calls (`isatty`, `getenv`, `getcwd`, `gethostname`, the Windows console mode).
//! The `unsafe` of the terminal unit is here.

use crate::terminal::{self, Consts, Host};
use crate::settings_ffi::UGREP_SETTINGS;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

/// Mirrors `struct TerminalEnv` of src/ugrep.cpp.
#[repr(C)]
pub struct TerminalEnv {
    pub ctx: *mut c_void,
    pub usage: extern "C" fn(*mut c_void, *const c_char),
    /// `fstat(STDOUT)` into `Static::output_stat*`; non-zero when the output is `/dev/null`.
    pub probe_output: extern "C" fn(*mut c_void) -> c_int,
    pub tree_utf8: extern "C" fn(*mut c_void),
    /// `flag_hyperlink_host.assign`, `flag_hyperlink_path.append`, `flag_hyperlink_prefix.assign`.
    pub hyperlink_set: extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char),
    pub never: *const c_char,
    pub always: *const c_char,
    pub auto: *const c_char,
    pub default_grep_colors: *const c_char,
    pub os_win: c_int,
    pub pathsep: c_int,
}

#[cfg(not(windows))]
mod sys {
    use std::ffi::{c_char, c_int};

    extern "C" {
        pub fn isatty(fd: c_int) -> c_int;
        pub fn getenv(name: *const c_char) -> *const c_char;
        pub fn gethostname(name: *mut c_char, len: usize) -> c_int;
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::{c_char, c_int, c_void};

    extern "C" {
        #[link_name = "_isatty"]
        pub fn isatty(fd: c_int) -> c_int;
        pub fn getenv(name: *const c_char) -> *const c_char;
    }

    #[link(name = "ws2_32")]
    extern "system" {
        pub fn WSAStartup(version: u16, data: *mut c_void) -> c_int;
        pub fn WSACleanup() -> c_int;
        pub fn gethostname(name: *mut c_char, len: c_int) -> c_int;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetStdHandle(which: u32) -> isize;
        pub fn SetConsoleOutputCP(cp: u32) -> c_int;
        pub fn GetConsoleMode(handle: isize, mode: *mut u32) -> c_int;
        pub fn SetConsoleMode(handle: isize, mode: u32) -> c_int;
    }
}

struct SysHost<'a>(&'a TerminalEnv);

impl SysHost<'_> {
    fn host_name() -> Vec<u8> {
        let mut host = [0 as c_char; 80];
        for (to, &from) in host.iter_mut().zip(b"localhost") {
            *to = from as c_char;
        }

        #[cfg(windows)]
        {
            // 512 bytes of 8-byte aligned storage hold a WSADATA of either pointer width
            let mut data = [0_u64; 64];
            // SAFETY: `data` is larger than WSADATA; `host` is a valid 80 byte buffer; WSACleanup pairs the startup.
            unsafe {
                if sys::WSAStartup(0x0101, data.as_mut_ptr().cast()) == 0 {
                    sys::gethostname(host.as_mut_ptr(), host.len() as c_int);
                    sys::WSACleanup();
                }
            }
        }
        #[cfg(not(windows))]
        {
            // SAFETY: `host` is a valid buffer of the length passed.
            unsafe {
                sys::gethostname(host.as_mut_ptr(), host.len());
            }
        }

        host.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect()
    }
}

#[cfg(test)]
pub(crate) fn test_c_bytes(p: *const c_char) -> Vec<u8> {
    // SAFETY: callers pass a pointer to a NUL-terminated string that outlives the call.
    unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
}

impl Host for SysHost<'_> {
    fn c_string(&mut self, p: *const c_char) -> Vec<u8> {
        if p.is_null() {
            return Vec::new();
        }
        // SAFETY: the settings hold NUL-terminated strings that live for the whole process (literals, argv or leaked copies).
        unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
    }

    fn isatty(&mut self, fd: i32) -> bool {
        // SAFETY: isatty accepts any descriptor number.
        unsafe { sys::isatty(fd) != 0 }
    }

    fn getenv(&mut self, name: &str) -> Option<Vec<u8>> {
        let name = CString::new(name).ok()?;
        // SAFETY: `name` is NUL-terminated; the returned string is copied before any other environment call.
        let value = unsafe { sys::getenv(name.as_ptr()) };
        if value.is_null() {
            None
        } else {
            // SAFETY: a non-NULL result of getenv is a NUL-terminated string.
            Some(unsafe { CStr::from_ptr(value) }.to_bytes().to_vec())
        }
    }

    fn probe_output(&mut self) -> bool {
        (self.0.probe_output)(self.0.ctx) != 0
    }

    fn tree_utf8(&mut self) {
        (self.0.tree_utf8)(self.0.ctx);
    }

    #[cfg(windows)]
    fn enable_vt(&mut self) -> Option<bool> {
        const STD_OUTPUT_HANDLE: u32 = -11_i32 as u32;
        const CP_UTF8: u32 = 65001;
        const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 4;
        // SAFETY: plain Win32 console calls on the process standard output handle; `mode` is a valid out pointer.
        unsafe {
            let handle = sys::GetStdHandle(STD_OUTPUT_HANDLE);
            if handle == -1 {
                return None;
            }
            // enable UTF-8 output
            sys::SetConsoleOutputCP(CP_UTF8);
            // try virtual terminal processing for ANSI SGR codes, enable colors when successful
            let mut mode = 0_u32;
            sys::GetConsoleMode(handle, &mut mode);
            mode |= ENABLE_VIRTUAL_TERMINAL_PROCESSING;
            Some(sys::SetConsoleMode(handle, mode) != 0)
        }
    }

    #[cfg(not(windows))]
    fn enable_vt(&mut self) -> Option<bool> {
        None
    }

    fn cwd(&mut self) -> Option<Vec<u8>> {
        let dir = std::env::current_dir().ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            Some(dir.into_os_string().into_vec())
        }
        #[cfg(not(unix))]
        {
            Some(dir.to_string_lossy().into_owned().into_bytes())
        }
    }

    fn hostname(&mut self) -> Vec<u8> {
        Self::host_name()
    }

    fn hyperlink_set(&mut self, host: &[u8], path: &[u8], prefix: &[u8]) {
        let (host, path, prefix) = (
            CString::new(host).unwrap_or_default(),
            CString::new(path).unwrap_or_default(),
            CString::new(prefix).unwrap_or_default(),
        );
        (self.0.hyperlink_set)(self.0.ctx, host.as_ptr(), path.as_ptr(), prefix.as_ptr());
    }

    fn usage(&mut self, message: &str) {
        let message = CString::new(message).unwrap_or_default();
        (self.0.usage)(self.0.ctx, message.as_ptr());
    }
}

/// The build-dependent values and C++ constants of the table.
///
/// # Safety
/// `env` is a valid table; `default_grep_colors` is a NUL-terminated string.
unsafe fn consts(env: &TerminalEnv) -> Consts {
    Consts {
        never: env.never,
        always: env.always,
        auto: env.auto,
        // SAFETY: see above.
        default_grep_colors: unsafe { CStr::from_ptr(env.default_grep_colors) }.to_bytes().to_vec(),
        os_win: env.os_win != 0,
        pathsep: u8::try_from(env.pathsep).unwrap_or(b'/'),
    }
}

/// `terminal()`.
///
/// # Safety
/// `env` points to a valid table whose callbacks accept `env.ctx`; no other thread touches the settings during the call.
#[export_name = "ugrep_core_terminal"]
pub unsafe extern "C" fn ugrep_core_terminal(env: *const TerminalEnv) {
    if env.is_null() {
        return;
    }
    // SAFETY: the caller passes a valid table (see above).
    let env = unsafe { &*env };
    // SAFETY: the table is valid; the settings are accessed from the main thread before the workers start.
    // The usage callback re-enters C++, which only reads flag_usage_warnings and then exits (accepted in decisions.md).
    let (consts, settings) = unsafe { (consts(env), &mut *UGREP_SETTINGS.as_ptr()) };
    terminal::terminal(settings, &mut SysHost(env), &consts);
}

/// `set_terminal_hyperlink()`.
///
/// # Safety
/// As [`ugrep_core_terminal`].
#[export_name = "ugrep_core_set_terminal_hyperlink"]
pub unsafe extern "C" fn ugrep_core_set_terminal_hyperlink(env: *const TerminalEnv) {
    if env.is_null() {
        return;
    }
    // SAFETY: the caller passes a valid table (see above).
    let env = unsafe { &*env };
    // SAFETY: the table is valid; the settings are accessed from one thread at a time, as the C++ globals were.
    let (consts, settings) = unsafe { (consts(env), &mut *UGREP_SETTINGS.as_ptr()) };
    terminal::set_terminal_hyperlink(settings, &mut SysHost(env), &consts);
}
