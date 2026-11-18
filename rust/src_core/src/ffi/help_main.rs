//! C-ABI entries of the messages of src/ugrep.cpp (`usage`, `help`, `version`, `warning`, `error`, `abort`,
//! `is_directory`, `cannot_decompress`) and of `main()`. The C++ owns the streams, `errno`, `exit()` and the search:
//! it passes a table of callbacks and the build configuration, Rust formats and writes. Entries that end the
//! process in C++ return the exit status (or -1 to continue) and the C++ caller calls `exit()`.
//! The `unsafe` of this unit is here.

use crate::help_main::{self as hm, BuildConfig, Colors, Perl, VersionInfo};
use crate::settings::Settings;
use crate::settings_ffi::UGREP_SETTINGS;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};

const EXIT_ERROR: c_int = 2;
const EXIT_OK: c_int = 0;
const EXIT_FAIL: c_int = 1;

/// Mirrors `struct CoreDiagEnv` of src/ugrep.cpp.
#[repr(C)]
pub struct DiagEnv {
    pub ctx: *mut c_void,
    /// `std::cerr.write`
    pub cerr_write: extern "C" fn(*mut c_void, *const c_char, usize),
    /// `std::cout.write`
    pub cout_write: extern "C" fn(*mut c_void, *const c_char, usize),
    /// `fwrite` to `Static::errout`
    pub errout_write: extern "C" fn(*mut c_void, *const c_char, usize),
    /// `strerror_s`/`strerror` of an errno value into a buffer of the given size
    pub strerror_text: extern "C" fn(*mut c_void, c_int, *mut c_char, usize),
    pub version: *const c_char,
    pub platform: *const c_char,
    /// the CPU feature suffix of `version()`, computed by RE/flex in C++
    pub simd: *const c_char,
    /// the regex engine suffix of `version()`
    pub regex: *const c_char,
    /// 0 none, 1 PCRE2, 2 Boost.Regex
    pub perl: c_int,
    pub zlib: c_int,
    pub no_7zip: c_int,
    pub bzip2: c_int,
    pub lzma: c_int,
    pub zstd: c_int,
    pub lz4: c_int,
    pub brotli: c_int,
    pub bzip3: c_int,
    pub decompression_thread: c_int,
    pub os_win: c_int,
    pub os_win_or_mingw: c_int,
    /// `HAVE_STATVFS` or `HAVE_STATFS`
    pub statfs: c_int,
    /// `GREP_PATH`, or NULL when undefined
    pub grep_path: *const c_char,
    /// `DEFAULT_PAGER_COMMAND`
    pub default_pager: *const c_char,
    /// `DEFAULT_IGNORE_FILE`
    pub default_ignore_file: *const c_char,
    /// the text of the `std::out_of_range` of `std::string(size, ' ').at(n)`, into a buffer of the given size
    pub past_end_text: extern "C" fn(*mut c_void, usize, usize, *mut c_char, usize),
}

/// Mirrors `struct CoreMainEnv` of src/ugrep.cpp.
#[repr(C)]
pub struct MainEnv {
    pub ctx: *mut c_void,
    pub diag: *const DiagEnv,
    /// `init(argc, argv)`; the text of an exception, or NULL
    pub init: extern "C" fn(*mut c_void, c_int, *const *const c_char) -> *const c_char,
    /// `tui_query()`
    pub tui_query: extern "C" fn(*mut c_void),
    /// `ugrep()`; the text of an exception, or NULL
    pub ugrep: extern "C" fn(*mut c_void) -> *const c_char,
    /// `Static::warnings`
    pub warnings: extern "C" fn(*mut c_void) -> usize,
    /// `Stats::found_any_file()`
    pub found_any: extern "C" fn(*mut c_void) -> c_int,
}

/// Bytes of a C string, `None` for NULL.
///
/// # Safety
/// `p` is NULL or a NUL-terminated string that outlives the returned slice.
unsafe fn bytes<'a>(p: *const c_char) -> Option<&'a [u8]> {
    if p.is_null() {
        None
    } else {
        // SAFETY: see above.
        Some(unsafe { CStr::from_ptr(p) }.to_bytes())
    }
}

fn settings() -> *mut Settings {
    UGREP_SETTINGS.as_ptr()
}

/// Reads a field of the settings without forming a reference: callers up the stack may hold `&mut Settings`.
macro_rules! setting {
    ($field:ident) => {
        // SAFETY: the settings object is a static; the field is a plain scalar or pointer read from the main thread.
        unsafe { (*settings()).$field }
    };
}

fn config(env: &DiagEnv) -> BuildConfig<'_> {
    BuildConfig {
        perl: match env.perl {
            1 => Perl::Pcre2,
            2 => Perl::Boost,
            _ => Perl::None,
        },
        zlib: env.zlib != 0,
        no_7zip: env.no_7zip != 0,
        bzip2: env.bzip2 != 0,
        lzma: env.lzma != 0,
        zstd: env.zstd != 0,
        lz4: env.lz4 != 0,
        brotli: env.brotli != 0,
        bzip3: env.bzip3 != 0,
        decompression_thread: env.decompression_thread != 0,
        os_win: env.os_win != 0,
        os_win_or_mingw: env.os_win_or_mingw != 0,
        statfs: env.statfs != 0,
        // SAFETY: the table holds NULL or a NUL-terminated string that outlives it.
        grep_path: unsafe { bytes(env.grep_path) },
        // SAFETY: the table holds NUL-terminated strings that outlive it.
        default_pager: unsafe { bytes(env.default_pager) }.unwrap_or(b""),
        // SAFETY: as above.
        default_ignore_file: unsafe { bytes(env.default_ignore_file) }.unwrap_or(b""),
        grep: setting!(flag_grep),
    }
}

/// Runs `f` so that a panic never crosses into C++: the status is then `EXIT_ERROR`.
fn guarded(f: impl FnOnce() -> c_int) -> c_int {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(EXIT_ERROR)
}

/// Runs `f` with the colors of the settings.
fn with_colors<R>(f: impl FnOnce(&Colors) -> R) -> R {
    // SAFETY: the color pointers of the settings are NULL or NUL-terminated strings that live for the process.
    unsafe {
        let s = settings();
        let fn_ptr = std::ptr::addr_of!((*s).color_fn).cast::<c_char>();
        let c = Colors {
            off: bytes((*s).color_off).unwrap_or(b""),
            high: bytes((*s).color_high).unwrap_or(b""),
            error: bytes((*s).color_error).unwrap_or(b""),
            warning: bytes((*s).color_warning).unwrap_or(b""),
            message: bytes((*s).color_message).unwrap_or(b""),
            fn_: bytes(fn_ptr).unwrap_or(b""),
        };
        f(&c)
    }
}

fn write_cerr(env: &DiagEnv, data: &[u8]) {
    (env.cerr_write)(env.ctx, data.as_ptr().cast(), data.len());
}

fn write_cout(env: &DiagEnv, data: &[u8]) {
    (env.cout_write)(env.ctx, data.as_ptr().cast(), data.len());
}

fn write_errout(env: &DiagEnv, data: &[u8]) {
    (env.errout_write)(env.ctx, data.as_ptr().cast(), data.len());
}

fn errno_text(env: &DiagEnv, errnum: c_int) -> Vec<u8> {
    let mut buf = [0 as c_char; 256];
    (env.strerror_text)(env.ctx, errnum, buf.as_mut_ptr(), buf.len());
    buf[buf.len() - 1] = 0;
    // SAFETY: `buf` is NUL-terminated above.
    unsafe { CStr::from_ptr(buf.as_ptr()) }.to_bytes().to_vec()
}

fn past_end_text(env: &DiagEnv, p: hm::PastEnd) -> Vec<u8> {
    let mut buf = [0 as c_char; 512];
    (env.past_end_text)(env.ctx, p.n, p.size, buf.as_mut_ptr(), buf.len());
    buf[buf.len() - 1] = 0;
    // SAFETY: `buf` is NUL-terminated above.
    unsafe { CStr::from_ptr(buf.as_ptr()) }.to_bytes().to_vec()
}

/// `usage()`: returns the exit status, or -1 when the C++ continues (reading a configuration file).
///
/// # Safety
/// `env` is a valid table; the strings are NULL or NUL-terminated.
#[export_name = "ugrep_core_usage"]
pub unsafe extern "C" fn ugrep_core_usage(env: *const DiagEnv, message: *const c_char, arg: *const c_char, valid: *const c_char) -> c_int {
    guarded(|| {
        // SAFETY: the caller passes a valid table and strings (see above).
        let (env, message, arg, valid) = unsafe { (&*env, bytes(message).unwrap_or(b""), bytes(arg), bytes(valid)) };
        write_cerr(env, &hm::usage_text(message, arg, valid));
        // do not exit when reading a config file
        if setting!(flag_usage_warnings) {
            -1
        } else {
            EXIT_ERROR
        }
    })
}

/// `help(what)`: returns the exit status.
///
/// # Safety
/// As [`ugrep_core_usage`].
#[export_name = "ugrep_core_help"]
pub unsafe extern "C" fn ugrep_core_help(env: *const DiagEnv, what: *const c_char) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let (env, what) = unsafe { (&*env, bytes(what)) };
        let help = hm::help_what(what, &config(env));
        write_cout(env, &help.out);
        if let Some(p) = help.past_end {
            // C++ `str.at()` threw out_of_range, which main() reports as an error
            let what = past_end_text(env, p);
            write_errout(env, &with_colors(|c| hm::abort_what_text(c, Some(b"error: "), &what)));
        }
        EXIT_ERROR
    })
}

/// `version()`: returns the exit status.
///
/// # Safety
/// As [`ugrep_core_usage`]; the build strings of the table are NUL-terminated.
#[export_name = "ugrep_core_version"]
pub unsafe extern "C" fn ugrep_core_version(env: *const DiagEnv) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let env = unsafe { &*env };
        let mode = setting!(flag_mode);
        // SAFETY: see above.
        let info = unsafe {
            VersionInfo {
                version: bytes(env.version).unwrap_or(b""),
                platform: bytes(env.platform).unwrap_or(b""),
                simd: bytes(env.simd).unwrap_or(b""),
                regex: bytes(env.regex).unwrap_or(b""),
                mode,
            }
        };
        write_cout(env, &hm::version_text(&info, &config(env)));
        EXIT_OK
    })
}

/// `is_directory()`
///
/// # Safety
/// As [`ugrep_core_usage`].
#[export_name = "ugrep_core_is_directory"]
pub unsafe extern "C" fn ugrep_core_is_directory(env: *const DiagEnv, pathname: *const c_char) {
    guarded(|| {
        // SAFETY: see above.
        let (env, pathname) = unsafe { (&*env, bytes(pathname).unwrap_or(b"")) };
        if !setting!(flag_no_messages) {
            write_errout(env, &with_colors(|c| hm::is_directory_text(c, pathname)));
        }
        0
    });
}

/// `cannot_decompress()`
///
/// # Safety
/// As [`ugrep_core_usage`].
#[export_name = "ugrep_core_cannot_decompress"]
pub unsafe extern "C" fn ugrep_core_cannot_decompress(env: *const DiagEnv, pathname: *const c_char, message: *const c_char) {
    guarded(|| {
        // SAFETY: see above.
        let (env, pathname, message) = unsafe { (&*env, bytes(pathname).unwrap_or(b""), bytes(message)) };
        if !setting!(flag_no_messages) {
            write_errout(env, &with_colors(|c| hm::cannot_decompress_text(c, pathname, message)));
        }
        0
    });
}

/// `warning()`: the C++ increments `Static::warnings`.
///
/// # Safety
/// As [`ugrep_core_usage`]; `errnum` is the value of `errno` at the call.
#[export_name = "ugrep_core_warning"]
pub unsafe extern "C" fn ugrep_core_warning(env: *const DiagEnv, message: *const c_char, arg: *const c_char, errnum: c_int) {
    guarded(|| {
        // SAFETY: see above.
        let (env, message, arg) = unsafe { (&*env, bytes(message), bytes(arg)) };
        if !setting!(flag_no_messages) {
            let errmsg = (errnum != 0).then(|| errno_text(env, errnum));
            write_errout(env, &with_colors(|c| hm::warning_text(c, message, arg, errmsg.as_deref())));
        }
        0
    });
}

/// `error()`: returns the exit status.
///
/// # Safety
/// As [`ugrep_core_warning`].
#[export_name = "ugrep_core_error"]
pub unsafe extern "C" fn ugrep_core_error(env: *const DiagEnv, message: *const c_char, arg: *const c_char, errnum: c_int) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let (env, message, arg) = unsafe { (&*env, bytes(message), bytes(arg)) };
        let errmsg = errno_text(env, errnum);
        write_errout(env, &with_colors(|c| hm::error_text(c, message, arg, &errmsg)));
        EXIT_ERROR
    })
}

/// `abort(message)` and `abort(message, what)` (`what` not NULL): returns the exit status.
///
/// # Safety
/// As [`ugrep_core_usage`].
#[export_name = "ugrep_core_abort"]
pub unsafe extern "C" fn ugrep_core_abort(env: *const DiagEnv, message: *const c_char, what: *const c_char) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let (env, message, what) = unsafe { (&*env, bytes(message), bytes(what)) };
        let text = with_colors(|c| match what {
            Some(what) => hm::abort_what_text(c, message, what),
            None => hm::abort_text(c, message.unwrap_or(b"")),
        });
        write_errout(env, &text);
        EXIT_ERROR
    })
}

/// The body of `main()` after the arguments and signal handlers are set up: returns the exit status.
///
/// # Safety
/// `env` is a valid table; `argv` holds `argc` NUL-terminated strings that outlive the call.
#[export_name = "ugrep_core_main"]
pub unsafe extern "C" fn ugrep_core_main(env: *const MainEnv, argc: c_int, argv: *const *const c_char) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let env = unsafe { &*env };
        // SAFETY: the diagnostics table lives as long as the main table.
        let diag = unsafe { &*env.diag };
        let abort = |message: &[u8], what: Option<&[u8]>| {
            let text = with_colors(|c| match what {
                Some(what) => hm::abort_what_text(c, Some(message), what),
                None => hm::abort_text(c, message),
            });
            write_errout(diag, &text);
            EXIT_ERROR
        };

        let err = (env.init)(env.ctx, argc, argv);
        // SAFETY: the callback returns NULL or a string that lives until the next callback.
        if let Some(what) = unsafe { bytes(err) } {
            return abort(b"error: ", Some(what));
        }

        if setting!(flag_query) {
            if !setting!(flag_no_messages) && (env.warnings)(env.ctx) > 0 {
                return abort(b"option -Q: warnings are present, specify -s to ignore", None);
            }
            // increase worker threads queue
            // SAFETY: plain scalar write in the main thread before the workers start.
            unsafe { (*settings()).flag_max_queue = 65536 };
            // -Q: TUI query mode
            (env.tui_query)(env.ctx);
        } else {
            if !setting!(flag_no_messages) && !setting!(flag_pager).is_null() && (env.warnings)(env.ctx) > 0 {
                return abort(b"option --pager: warnings are present, specify -s to ignore", None);
            }
            let err = (env.ugrep)(env.ctx);
            // SAFETY: as above.
            if let Some(what) = unsafe { bytes(err) } {
                return abort(b"error: ", Some(what));
            }
        }

        if (env.warnings)(env.ctx) > 0 {
            EXIT_ERROR
        } else if (env.found_any)(env.ctx) != 0 {
            EXIT_OK
        } else {
            EXIT_FAIL
        }
    })
}
