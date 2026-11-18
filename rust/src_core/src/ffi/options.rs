//! C-ABI entries of the option parser for src/ugrep.cpp: `ugrep_options` and `ugrep_trim_pathname_arg`.
//! The C++ side passes a table of callbacks (`UgrepOptionsEnv` in src/ugrep.cpp) for what stays C++.

use crate::options::{self, Arg, Consts, Host, Vector};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

/// Mirror of `struct UgrepOptionsEnv` in src/ugrep.cpp.
#[repr(C)]
pub struct Env {
    never: *const c_char,
    always: *const c_char,
    auto: *const c_char,
    default_ignore_file: *const c_char,
    default_tag: *const c_char,
    default_tabs: usize,
    max_mmap_size: usize,
    os_win: c_int,
    usage: unsafe extern "C" fn(message: *const c_char, arg: *const c_char, valid: *const c_char),
    help: unsafe extern "C" fn(what: *const c_char),
    version: unsafe extern "C" fn(),
    screen_width: unsafe extern "C" fn() -> usize,
    pattern_push: unsafe extern "C" fn(list: *mut c_void, kind: c_int, arg: *const c_char),
    pattern_empty: unsafe extern "C" fn(list: *mut c_void) -> c_int,
    vec_push: unsafe extern "C" fn(vector: c_int, arg: *const c_char),
    vec_clear: unsafe extern "C" fn(vector: c_int),
    vec_empty: unsafe extern "C" fn(vector: c_int) -> c_int,
    filter_append: unsafe extern "C" fn(arg: *const c_char),
    filter_clear: unsafe extern "C" fn(),
    ignore_files_insert: unsafe extern "C" fn(arg: *const c_char),
    ignore_files_clear: unsafe extern "C" fn(),
    arg_pattern_set: unsafe extern "C" fn() -> c_int,
    set_arg_pattern: unsafe extern "C" fn(arg: *const c_char),
    push_arg_file: unsafe extern "C" fn(arg: *const c_char),
}

struct CppHost<'a> {
    env: &'a Env,
    list: *mut c_void,
}

fn p(a: Arg) -> *const c_char {
    a.as_ptr().cast()
}

fn opt(a: Option<Arg>) -> *const c_char {
    a.map_or(std::ptr::null(), p)
}

// SAFETY (all calls): the callbacks are the C++ functions of `UgrepOptionsEnv`; every string passed is
// NUL-terminated (`Arg` is followed by its NUL) and the list pointer is the C++ list given to `ugrep_options`.
impl Host for CppHost<'_> {
    fn usage(&self, message: Arg, arg: Option<Arg>, valid: Option<Arg>) {
        unsafe { (self.env.usage)(p(message), opt(arg), opt(valid)) }
    }
    fn help(&self, what: Option<Arg>) {
        unsafe { (self.env.help)(opt(what)) }
    }
    fn version(&self) {
        unsafe { (self.env.version)() }
    }
    fn screen_width(&self) -> usize {
        unsafe { (self.env.screen_width)() }
    }
    fn pattern_push(&self, kind: i32, arg: Arg) {
        unsafe { (self.env.pattern_push)(self.list, kind, p(arg)) }
    }
    fn pattern_empty(&self) -> bool {
        unsafe { (self.env.pattern_empty)(self.list) != 0 }
    }
    fn vec_push(&self, vector: Vector, arg: Arg) {
        unsafe { (self.env.vec_push)(vector as c_int, p(arg)) }
    }
    fn vec_clear(&self, vector: Vector) {
        unsafe { (self.env.vec_clear)(vector as c_int) }
    }
    fn vec_empty(&self, vector: Vector) -> bool {
        unsafe { (self.env.vec_empty)(vector as c_int) != 0 }
    }
    fn filter_append(&self, arg: Arg) {
        unsafe { (self.env.filter_append)(p(arg)) }
    }
    fn filter_clear(&self) {
        unsafe { (self.env.filter_clear)() }
    }
    fn ignore_files_insert(&self, arg: Arg) {
        unsafe { (self.env.ignore_files_insert)(p(arg)) }
    }
    fn ignore_files_clear(&self) {
        unsafe { (self.env.ignore_files_clear)() }
    }
    fn arg_pattern_set(&self) -> bool {
        unsafe { (self.env.arg_pattern_set)() != 0 }
    }
    fn set_arg_pattern(&self, arg: Arg) {
        unsafe { (self.env.set_arg_pattern)(p(arg)) }
    }
    fn push_arg_file(&self, arg: Arg) {
        unsafe { (self.env.push_arg_file)(p(arg)) }
    }
}

/// An argument string of the process.
///
/// # Safety
/// `s` is a valid NUL-terminated string that lives for the whole process and is not modified while used.
unsafe fn arg_of(s: *const c_char) -> Arg {
    // SAFETY: by the contract of this function.
    unsafe { CStr::from_ptr(s) }.to_bytes()
}

/// `options(pattern_args, argc, argv)`
///
/// # Safety
/// `env` points to a filled `UgrepOptionsEnv`, `list` is the C++ pattern list the callbacks expect, `argv` has
/// `argc` valid NUL-terminated strings that live for the whole process.
#[export_name = "ugrep_options"]
pub unsafe extern "C" fn ugrep_options(
    env: *const Env,
    list: *mut c_void,
    argc: c_int,
    argv: *const *const c_char,
) {
    // SAFETY: by the contract of this function.
    let env = unsafe { &*env };
    let argc = usize::try_from(argc).unwrap_or(0);
    let mut args: Vec<Arg> = Vec::with_capacity(argc);
    for k in 0..argc {
        // SAFETY: `argv` has `argc` entries; load_config passes NULL for argv[0], which is never read.
        let s = unsafe { *argv.add(k) };
        args.push(if s.is_null() {
            b""
        } else {
            unsafe { arg_of(s) }
        });
    }
    let consts = Consts {
        // SAFETY: the constants are C++ string literals.
        never: unsafe { arg_of(env.never) },
        always: unsafe { arg_of(env.always) },
        auto: unsafe { arg_of(env.auto) },
        default_ignore_file: unsafe { arg_of(env.default_ignore_file) },
        default_tag: unsafe { arg_of(env.default_tag) },
        default_tabs: env.default_tabs,
        max_mmap_size: env.max_mmap_size,
        os_win: env.os_win != 0,
    };
    options::options(&CppHost { env, list }, consts, &args);
}

/// `trim_pathname_arg(arg)`: truncates the `argv[]` string `arg` at its trailing path separators.
///
/// # Safety
/// `arg` is a valid, writable NUL-terminated string.
#[export_name = "ugrep_trim_pathname_arg"]
pub unsafe extern "C" fn ugrep_trim_pathname_arg(arg: *mut c_char, sep: c_char) {
    // SAFETY: by the contract of this function.
    let bytes = unsafe { CStr::from_ptr(arg) }.to_bytes();
    let len = bytes.len();
    if let Some(start) = options::trim_pathname_start(bytes, sep as u8) {
        for k in start..len {
            // SAFETY: `k < len`, inside the string.
            unsafe { *arg.add(k) = 0 };
        }
    }
}
