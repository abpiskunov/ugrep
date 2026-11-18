//! C-ABI entry of `options()` for src/ugrep.cpp: a table of C callbacks for what stays in C++ (the string
//! and container options, the pattern list, `usage`, `help`, `version`, ...) and the exported entry that runs
//! the Rust parser on the Rust-owned `ugrep_settings`. The only `unsafe` of the options unit is here.

use crate::options::{self, Consts, Env, List, Z};
use crate::settings_ffi::UGREP_SETTINGS;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

/// Mirrors `struct OptionsEnv` of src/ugrep.cpp. The `list` numbers are the order of [`List`].
#[repr(C)]
pub struct OptionsEnv {
    pub ctx: *mut c_void,
    pub usage: extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char),
    pub help: extern "C" fn(*mut c_void, *const c_char),
    pub version: extern "C" fn(*mut c_void),
    pub strarg: extern "C" fn(*mut c_void, *const c_char) -> *const c_char,
    pub pattern_push: extern "C" fn(*mut c_void, c_int, *const c_char),
    pub patterns_empty: extern "C" fn(*mut c_void) -> c_int,
    pub list_push: extern "C" fn(*mut c_void, c_int, *const c_char),
    pub list_clear: extern "C" fn(*mut c_void, c_int),
    pub list_empty: extern "C" fn(*mut c_void, c_int) -> c_int,
    pub filter_append: extern "C" fn(*mut c_void, *const c_char),
    pub filter_clear: extern "C" fn(*mut c_void),
    pub arg_pattern_is_null: extern "C" fn(*mut c_void) -> c_int,
    pub set_arg_pattern: extern "C" fn(*mut c_void, *const c_char),
    pub push_arg_file: extern "C" fn(*mut c_void, *const c_char),
    pub screen_width: extern "C" fn(*mut c_void) -> usize,
    pub auto: *const c_char,
    pub never: *const c_char,
    pub max_mmap_size: usize,
    pub default_tabs: usize,
    pub default_tag: *const c_char,
    pub default_ignore_file: *const c_char,
    pub os_win: c_int,
}

fn z_ptr(z: Z<'_>) -> *const c_char {
    z.as_ptr().cast()
}

/// An optional text as a C string for a callback (`None` is NULL).
fn opt_cstring(s: Option<&[u8]>) -> Option<CString> {
    s.map(|b| CString::new(b).unwrap_or_default())
}

fn opt_ptr(c: &Option<CString>) -> *const c_char {
    c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())
}

struct CEnv<'a>(&'a OptionsEnv);

impl Env for CEnv<'_> {
    fn usage(&mut self, message: &str, arg: Option<&[u8]>, valid: Option<&str>) {
        let message = CString::new(message).unwrap_or_default();
        let arg = opt_cstring(arg);
        let valid = opt_cstring(valid.map(str::as_bytes));
        (self.0.usage)(self.0.ctx, message.as_ptr(), opt_ptr(&arg), opt_ptr(&valid));
    }

    fn help(&mut self, what: Option<Z<'_>>) {
        (self.0.help)(self.0.ctx, what.map_or(std::ptr::null(), z_ptr));
    }

    fn version(&mut self) {
        (self.0.version)(self.0.ctx);
    }

    fn strarg(&mut self, string: Z<'_>) -> *const c_char {
        (self.0.strarg)(self.0.ctx, z_ptr(string))
    }

    fn pattern_push(&mut self, mask: i32, arg: Z<'_>) {
        (self.0.pattern_push)(self.0.ctx, mask, z_ptr(arg));
    }

    fn patterns_empty(&mut self) -> bool {
        (self.0.patterns_empty)(self.0.ctx) != 0
    }

    fn list_push(&mut self, list: List, value: Z<'_>) {
        (self.0.list_push)(self.0.ctx, list as c_int, z_ptr(value));
    }

    fn list_clear(&mut self, list: List) {
        (self.0.list_clear)(self.0.ctx, list as c_int);
    }

    fn list_empty(&mut self, list: List) -> bool {
        (self.0.list_empty)(self.0.ctx, list as c_int) != 0
    }

    fn filter_append(&mut self, value: Z<'_>) {
        (self.0.filter_append)(self.0.ctx, z_ptr(value));
    }

    fn filter_clear(&mut self) {
        (self.0.filter_clear)(self.0.ctx);
    }

    fn arg_pattern_is_null(&mut self) -> bool {
        (self.0.arg_pattern_is_null)(self.0.ctx) != 0
    }

    fn set_arg_pattern(&mut self, arg: Z<'_>) {
        (self.0.set_arg_pattern)(self.0.ctx, z_ptr(arg));
    }

    fn push_arg_file(&mut self, arg: Z<'_>) {
        (self.0.push_arg_file)(self.0.ctx, z_ptr(arg));
    }

    fn screen_width(&mut self) -> usize {
        (self.0.screen_width)(self.0.ctx)
    }

    fn c_string(&mut self, p: *const c_char) -> Vec<u8> {
        if p.is_null() {
            return Vec::new();
        }
        // SAFETY: the settings hold NUL-terminated strings that live for the whole process (literals or argv).
        unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
    }
}

/// `options(pattern_args, argc, argv)`.
///
/// # Safety
/// `env` points to a valid table whose callbacks accept `env.ctx`; `argv` has `argc` entries that are NULL or
/// NUL-terminated strings living for the whole process; no other thread touches the settings during the call.
#[export_name = "ugrep_core_options"]
pub unsafe extern "C" fn ugrep_core_options(env: *const OptionsEnv, argc: c_int, argv: *const *const c_char) {
    if env.is_null() || argv.is_null() {
        return;
    }
    let argc = usize::try_from(argc).unwrap_or(0);
    // SAFETY: the caller passes a valid table and `argc` argv entries (see above).
    let (env, args) = unsafe { (&*env, std::slice::from_raw_parts(argv, argc)) };
    let argv: Vec<Z<'_>> = args
        .iter()
        .map(|&p| {
            if p.is_null() {
                b"\0".as_slice()
            } else {
                // SAFETY: a non-NULL argv entry is a NUL-terminated string that outlives the call.
                unsafe { CStr::from_ptr(p) }.to_bytes_with_nul()
            }
        })
        .collect();
    let consts = Consts {
        auto: env.auto,
        never: env.never,
        max_mmap_size: env.max_mmap_size,
        default_tabs: env.default_tabs,
        default_tag: env.default_tag,
        // SAFETY: `default_ignore_file` is the DEFAULT_IGNORE_FILE string literal.
        default_ignore_file: unsafe { CStr::from_ptr(env.default_ignore_file) }.to_bytes_with_nul(),
        os_win: env.os_win != 0,
    };
    // SAFETY: option parsing runs on the main thread before the workers start; C++ callbacks read the settings
    // only after this function returns or between statements, never concurrently.
    let settings = unsafe { &mut *UGREP_SETTINGS.as_ptr() };
    options::options(settings, &mut CEnv(env), consts, &argv);
}
