//! C-ABI entries of `load_config` and `save_config` for src/ugrep.cpp: a table of C callbacks for the file,
//! stream and container operations that stay in C++, and the exported entries that run the Rust logic on the
//! Rust-owned `ugrep_settings`. The only `unsafe` of the config unit is here.

use crate::options_ffi::{run_options, CEnv, OptionsEnv};
use crate::config::{self, ConfigConsts, ConfigEnv, Sink};
use crate::options::{Env, List, Z};
use crate::settings_ffi::UGREP_SETTINGS;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

/// Mirrors `struct ConfigEnv` of src/ugrep.cpp.
#[repr(C)]
pub struct ConfigTable {
    pub opts: *const OptionsEnv,
    pub open: extern "C" fn(*const c_char, c_int) -> *mut c_void,
    pub next_line: extern "C" fn(*mut c_void) -> c_int,
    pub line_text: extern "C" fn(*mut c_void) -> *const c_char,
    pub prefix_line: extern "C" fn(*mut c_void) -> *const c_char,
    pub file_error: extern "C" fn(*mut c_void) -> c_int,
    pub close: extern "C" fn(*mut c_void),
    pub files_contains: extern "C" fn(*const c_char) -> c_int,
    pub files_insert: extern "C" fn(*const c_char),
    pub files_len: extern "C" fn() -> usize,
    pub files_get: extern "C" fn(usize) -> *const c_char,
    pub home_dir: extern "C" fn() -> *const c_char,
    pub warnings_reset: extern "C" fn(),
    pub warnings: extern "C" fn() -> usize,
    pub write: extern "C" fn(c_int, *const c_char, usize),
    pub error: extern "C" fn(*const c_char, *const c_char),
    pub warning: extern "C" fn(*const c_char, *const c_char),
    pub exit_error: extern "C" fn(),
    pub regexp_clear: extern "C" fn(),
    pub filter: extern "C" fn() -> *const c_char,
    pub list_len: extern "C" fn(c_int) -> usize,
    pub list_get: extern "C" fn(c_int, usize) -> *const c_char,
    pub rename_old: extern "C" fn(*const c_char, *const c_char) -> c_int,
    pub save_open: extern "C" fn(*const c_char) -> *mut c_void,
    pub save_write: extern "C" fn(*mut c_void, *const c_char, usize),
    pub save_error: extern "C" fn(*mut c_void) -> c_int,
    pub save_close: extern "C" fn(*mut c_void),
    pub default_query_delay: usize,
    pub path_sep: c_int,
    pub xdg: c_int,
}

#[derive(Clone, Copy)]
struct Handle(*mut c_void);

struct CCfg<'a> {
    t: &'a ConfigTable,
    opts: CEnv<'a>,
}

fn z_ptr(z: Z<'_>) -> *const c_char {
    z.as_ptr().cast()
}

fn bytes(p: *const c_char) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    // SAFETY: the callbacks return NUL-terminated strings that stay valid until the next call of the same kind.
    unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
}

impl ConfigEnv for CCfg<'_> {
    type File = Handle;

    fn usage(&mut self, message: &str, arg: Option<&[u8]>, valid: Option<&str>) {
        self.opts.usage(message, arg, valid);
    }

    fn list_clear(&mut self, list: List) {
        self.opts.list_clear(list);
    }

    fn list_empty(&mut self, list: List) -> bool {
        self.opts.list_empty(list)
    }

    fn filter_clear(&mut self) {
        self.opts.filter_clear();
    }

    fn c_string(&mut self, p: *const c_char) -> Vec<u8> {
        self.opts.c_string(p)
    }

    fn open(&mut self, path: Z<'_>, check_owner: bool) -> Option<Handle> {
        let p = (self.t.open)(z_ptr(path), c_int::from(check_owner));
        (!p.is_null()).then_some(Handle(p))
    }

    fn next_line(&mut self, file: Handle) -> bool {
        (self.t.next_line)(file.0) != 0
    }

    fn line_text(&mut self, file: Handle) -> Vec<u8> {
        bytes((self.t.line_text)(file.0))
    }

    fn prefix_line(&mut self, file: Handle) -> *const c_char {
        (self.t.prefix_line)(file.0)
    }

    fn file_error(&mut self, file: Handle) -> bool {
        (self.t.file_error)(file.0) != 0
    }

    fn close(&mut self, file: Handle) {
        (self.t.close)(file.0);
    }

    fn files_contains(&mut self, path: Z<'_>) -> bool {
        (self.t.files_contains)(z_ptr(path)) != 0
    }

    fn files_insert(&mut self, path: Z<'_>) {
        (self.t.files_insert)(z_ptr(path));
    }

    fn files_len(&mut self) -> usize {
        (self.t.files_len)()
    }

    fn files_get(&mut self, index: usize) -> Vec<u8> {
        bytes((self.t.files_get)(index))
    }

    fn home_dir(&mut self) -> Option<Vec<u8>> {
        let p = (self.t.home_dir)();
        (!p.is_null()).then(|| bytes(p))
    }

    fn warnings_reset(&mut self) {
        (self.t.warnings_reset)();
    }

    fn warnings(&mut self) -> usize {
        (self.t.warnings)()
    }

    fn write(&mut self, sink: Sink, data: &[u8]) {
        (self.t.write)(sink as c_int, data.as_ptr().cast(), data.len());
    }

    fn error(&mut self, message: &str, arg: Z<'_>) {
        let message = CString::new(message).unwrap_or_default();
        (self.t.error)(message.as_ptr(), z_ptr(arg));
    }

    fn warning(&mut self, message: &str, arg: Z<'_>) {
        let message = CString::new(message).unwrap_or_default();
        (self.t.warning)(message.as_ptr(), z_ptr(arg));
    }

    fn exit_error(&mut self) -> ! {
        (self.t.exit_error)();
        std::process::exit(2)
    }

    fn parse_option(&mut self, settings: &mut crate::settings::Settings, arg: *const c_char) {
        run_options(self.t_opts(), settings, &[std::ptr::null(), arg]);
    }

    fn regexp_clear(&mut self) {
        (self.t.regexp_clear)();
    }

    fn filter(&mut self) -> Vec<u8> {
        bytes((self.t.filter)())
    }

    fn list_len(&mut self, list: List) -> usize {
        (self.t.list_len)(list as c_int)
    }

    fn list_get(&mut self, list: List, index: usize) -> Vec<u8> {
        bytes((self.t.list_get)(list as c_int, index))
    }

    fn rename_old(&mut self, path: Z<'_>, old: Z<'_>) -> bool {
        (self.t.rename_old)(z_ptr(path), z_ptr(old)) != 0
    }

    fn save_open(&mut self, path: Z<'_>) -> Option<Handle> {
        let p = (self.t.save_open)(z_ptr(path));
        (!p.is_null()).then_some(Handle(p))
    }

    fn save_write(&mut self, file: Handle, data: &[u8]) {
        (self.t.save_write)(file.0, data.as_ptr().cast(), data.len());
    }

    fn save_error(&mut self, file: Handle) -> bool {
        (self.t.save_error)(file.0) != 0
    }

    fn save_close(&mut self, file: Handle) {
        (self.t.save_close)(file.0);
    }
}

impl CCfg<'_> {
    fn t_opts(&self) -> &OptionsEnv {
        self.opts.0
    }
}

fn consts(t: &ConfigTable) -> ConfigConsts {
    ConfigConsts {
        default_query_delay: t.default_query_delay,
        path_sep: u8::try_from(t.path_sep).unwrap_or(b'/'),
        xdg: t.xdg != 0,
    }
}

/// `load_config(pattern_args, recurse)`
///
/// # Safety
/// `table` and `table.opts` point to valid tables whose callbacks follow the contract of src/ugrep.cpp; the call
/// runs on the main thread before the workers start.
#[export_name = "ugrep_core_load_config"]
pub unsafe extern "C" fn ugrep_core_load_config(table: *const ConfigTable, recurse: c_int) {
    if table.is_null() {
        return;
    }
    // SAFETY: valid table (see above).
    let t = unsafe { &*table };
    if t.opts.is_null() {
        return;
    }
    // SAFETY: valid options table (see above); main thread, no concurrent settings access.
    let (opts, settings) = unsafe { (&*t.opts, &mut *UGREP_SETTINGS.as_ptr()) };
    config::load_config(settings, &mut CCfg { t, opts: CEnv(opts) }, consts(t), recurse != 0);
}

/// `save_config()`
///
/// # Safety
/// Same contract as [`ugrep_core_load_config`].
#[export_name = "ugrep_core_save_config"]
pub unsafe extern "C" fn ugrep_core_save_config(table: *const ConfigTable) {
    if table.is_null() {
        return;
    }
    // SAFETY: valid table (see above).
    let t = unsafe { &*table };
    if t.opts.is_null() {
        return;
    }
    // SAFETY: valid options table (see above); main thread, no concurrent settings access.
    let (opts, settings) = unsafe { (&*t.opts, &mut *UGREP_SETTINGS.as_ptr()) };
    config::save_config(settings, &mut CCfg { t, opts: CEnv(opts) }, consts(t));
}
