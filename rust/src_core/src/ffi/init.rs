//! C-ABI entry of `init(argc, argv)` for src/ugrep.cpp: a table of C callbacks for what stays in C++ (the
//! `std::string` and container options, the CNF, `load_config`/`save_config`, `options`, `terminal`, `usage`,
//! `warning`, `error`, `abort`, `exit`, the type and encoding tables, the reflex magic patterns and the file
//! access) and the exported entry that runs the Rust `init` on the Rust-owned `ugrep_settings`.
//! The only `unsafe` of the init unit is here.

use crate::init::{self, ArgFile, Consts, Env, TypeEntry, Z};
use crate::options::List;
use crate::settings::Settings;
use crate::settings_ffi::UGREP_SETTINGS;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

/// Mirrors `struct InitEnv` of src/ugrep.cpp. The `list` numbers are the order of [`List`].
#[repr(C)]
pub struct InitEnv {
    pub ctx: *mut c_void,
    pub set_home_dir: extern "C" fn(*mut c_void, *const c_char),
    pub cerr: extern "C" fn(*mut c_void, *const c_char, usize),
    pub exit: extern "C" fn(*mut c_void, c_int),
    pub usage: extern "C" fn(*mut c_void, *const c_char),
    pub warning: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub error: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub abort: extern "C" fn(*mut c_void, *const c_char),
    pub abort_what: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub load_config: extern "C" fn(*mut c_void),
    pub save_config: extern "C" fn(*mut c_void),
    pub warnings_reset: extern "C" fn(*mut c_void),
    pub warnings: extern "C" fn(*mut c_void) -> usize,
    pub options: extern "C" fn(*mut c_void),
    pub terminal: extern "C" fn(*mut c_void),
    pub patterns_len: extern "C" fn(*mut c_void) -> usize,
    pub pattern_get: extern "C" fn(*mut c_void, usize, *mut c_int, *mut *const c_char),
    pub bcnf_new_pattern: extern "C" fn(*mut c_void, c_int, *const c_char),
    pub bcnf_new_pattern_regexp: extern "C" fn(*mut c_void),
    pub bcnf_new_term: extern "C" fn(*mut c_void),
    pub regexp_append: extern "C" fn(*mut c_void, *const c_char),
    pub list_len: extern "C" fn(*mut c_void, c_int) -> usize,
    pub list_get: extern "C" fn(*mut c_void, c_int, usize) -> *const c_char,
    pub list_push: extern "C" fn(*mut c_void, c_int, *const c_char),
    pub list_truncate: extern "C" fn(*mut c_void, c_int, usize, usize),
    pub ignore_file_erase: extern "C" fn(*mut c_void, *const c_char),
    pub arg_pattern: extern "C" fn(*mut c_void) -> *const c_char,
    pub set_arg_pattern: extern "C" fn(*mut c_void, *const c_char),
    pub arg_files_len: extern "C" fn(*mut c_void) -> usize,
    pub arg_file: extern "C" fn(*mut c_void, usize) -> *const c_char,
    pub arg_files_erase: extern "C" fn(*mut c_void, usize),
    pub arg_files_insert_front: extern "C" fn(*mut c_void, *const c_char),
    pub expand_arg_files: extern "C" fn(*mut c_void),
    /// 0 missing, 1 symbolic link to skip, 2 unreadable, 3 directory, 4 other.
    pub check_arg_file: extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int,
    pub trim_pathname_arg: extern "C" fn(*mut c_void, *const c_char),
    pub file_open: extern "C" fn(*mut c_void, *const c_char) -> c_int,
    pub file_getline: extern "C" fn(*mut c_void, *mut usize) -> c_int,
    pub arg_files_push_line: extern "C" fn(*mut c_void),
    pub file_close: extern "C" fn(*mut c_void),
    pub import_globs: extern "C" fn(*mut c_void, c_int, c_int),
    pub fs_probe: extern "C" fn(*mut c_void, *const c_char, *mut u64) -> c_int,
    pub fs_insert: extern "C" fn(*mut c_void, c_int, u64),
    pub type_entry: extern "C" fn(
        *mut c_void,
        usize,
        *mut *const c_char,
        *mut *const c_char,
        *mut *const c_char,
        *mut *const c_char,
    ) -> c_int,
    pub encoding_entry: extern "C" fn(*mut c_void, usize, *mut *const c_char, *mut u16) -> c_int,
    pub magic_pattern_assign: extern "C" fn(*mut c_void, *const c_char) -> *const c_char,
    pub filter_magic_assign: extern "C" fn(*mut c_void, *const c_char) -> *const c_char,
    pub filter_empty: extern "C" fn(*mut c_void) -> c_int,
    pub isatty_stdin: extern "C" fn(*mut c_void) -> c_int,
    pub set_binary_stdout: extern "C" fn(*mut c_void),
    pub home_var: *const c_char,
    pub auto: *const c_char,
    pub pathsep: c_int,
    pub os_win: c_int,
    pub os_win_or_mingw: c_int,
    pub have_libz: c_int,
    pub have_regex_perl: c_int,
    pub have_stat_fs: c_int,
    pub cygwin: c_int,
    pub max_hex_columns: usize,
    pub encoding_null_data: u16,
}

extern "C" {
    fn getenv(name: *const c_char) -> *const c_char;
}

/// A text as a C string for a callback.
fn cstr(bytes: &[u8]) -> CString {
    CString::new(bytes).unwrap_or_default()
}

fn opt_ptr(c: &Option<CString>) -> *const c_char {
    c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())
}

/// The bytes of a C string, `None` for NULL.
fn opt_bytes(p: *const c_char) -> Option<Vec<u8>> {
    if p.is_null() {
        return None;
    }
    // SAFETY: a non-NULL result of a callback or a settings field is a NUL-terminated string that is valid
    // until the next callback (the settings point to literals, argv or strings kept for the whole run).
    Some(unsafe { CStr::from_ptr(p) }.to_bytes().to_vec())
}

fn list_id(list: List) -> c_int {
    list as c_int
}

struct CEnv<'a>(&'a InitEnv);

impl Env for CEnv<'_> {
    fn settings(&mut self) -> &mut Settings {
        // SAFETY: `init` runs on the main thread before the workers start and takes a fresh borrow for every
        // access; it keeps none across a call of the environment, where C++ and the option parser re-enter and
        // use the same settings (the `&mut self` of this method enforces that).
        unsafe { &mut *UGREP_SETTINGS.as_ptr() }
    }

    fn c_string(&mut self, p: *const c_char) -> Vec<u8> {
        opt_bytes(p).unwrap_or_default()
    }

    fn set_home_dir(&mut self) {
        // SAFETY: `home_var` is a NUL-terminated string literal; getenv returns NULL or a string that lives for the run.
        let home = unsafe { getenv(self.0.home_var) };
        (self.0.set_home_dir)(self.0.ctx, home);
    }

    fn cerr(&mut self, text: &[u8]) {
        let c = cstr(text);
        (self.0.cerr)(self.0.ctx, c.as_ptr(), c.as_bytes().len());
    }

    fn exit(&mut self, code: i32) {
        (self.0.exit)(self.0.ctx, code);
    }

    fn usage(&mut self, message: &[u8]) {
        (self.0.usage)(self.0.ctx, cstr(message).as_ptr());
    }

    fn warning(&mut self, message: Option<&[u8]>, arg: &[u8]) {
        let message = message.map(cstr);
        (self.0.warning)(self.0.ctx, opt_ptr(&message), cstr(arg).as_ptr());
    }

    fn error(&mut self, message: &[u8], arg: &[u8]) {
        (self.0.error)(self.0.ctx, cstr(message).as_ptr(), cstr(arg).as_ptr());
    }

    fn abort(&mut self, message: &[u8]) {
        (self.0.abort)(self.0.ctx, cstr(message).as_ptr());
    }

    fn abort_what(&mut self, message: &[u8], what: &[u8]) {
        (self.0.abort_what)(self.0.ctx, cstr(message).as_ptr(), cstr(what).as_ptr());
    }

    fn load_config(&mut self) {
        (self.0.load_config)(self.0.ctx);
    }

    fn save_config(&mut self) {
        (self.0.save_config)(self.0.ctx);
    }

    fn warnings_reset(&mut self) {
        (self.0.warnings_reset)(self.0.ctx);
    }

    fn warnings(&mut self) -> usize {
        (self.0.warnings)(self.0.ctx)
    }

    fn options(&mut self) {
        (self.0.options)(self.0.ctx);
    }

    fn terminal(&mut self) {
        (self.0.terminal)(self.0.ctx);
    }

    fn patterns(&mut self) -> Vec<(i32, *const c_char)> {
        let n = (self.0.patterns_len)(self.0.ctx);
        (0..n)
            .map(|i| {
                let mut mask: c_int = 0;
                let mut arg: *const c_char = std::ptr::null();
                (self.0.pattern_get)(self.0.ctx, i, &mut mask, &mut arg);
                (mask, arg)
            })
            .collect()
    }

    fn bcnf_new_pattern(&mut self, mask: i32, arg: *const c_char) {
        (self.0.bcnf_new_pattern)(self.0.ctx, mask, arg);
    }

    fn bcnf_new_pattern_regexp(&mut self) {
        (self.0.bcnf_new_pattern_regexp)(self.0.ctx);
    }

    fn bcnf_new_term(&mut self) {
        (self.0.bcnf_new_term)(self.0.ctx);
    }

    fn regexp_append(&mut self, text: &[u8]) {
        (self.0.regexp_append)(self.0.ctx, cstr(text).as_ptr());
    }

    fn list_all(&mut self, list: List) -> Vec<Vec<u8>> {
        let n = (self.0.list_len)(self.0.ctx, list_id(list));
        (0..n)
            .map(|i| opt_bytes((self.0.list_get)(self.0.ctx, list_id(list), i)).unwrap_or_default())
            .collect()
    }

    fn list_push(&mut self, list: List, value: &[u8]) {
        (self.0.list_push)(self.0.ctx, list_id(list), cstr(value).as_ptr());
    }

    fn list_truncate(&mut self, list: List, index: usize, len: usize) {
        (self.0.list_truncate)(self.0.ctx, list_id(list), index, len);
    }

    fn ignore_file_erase(&mut self, name: &[u8]) {
        (self.0.ignore_file_erase)(self.0.ctx, cstr(name).as_ptr());
    }

    fn arg_pattern(&mut self) -> *const c_char {
        (self.0.arg_pattern)(self.0.ctx)
    }

    fn set_arg_pattern(&mut self, p: *const c_char) {
        (self.0.set_arg_pattern)(self.0.ctx, p);
    }

    fn arg_files_len(&mut self) -> usize {
        (self.0.arg_files_len)(self.0.ctx)
    }

    fn arg_file(&mut self, i: usize) -> *const c_char {
        (self.0.arg_file)(self.0.ctx, i)
    }

    fn arg_files_erase(&mut self, i: usize) {
        (self.0.arg_files_erase)(self.0.ctx, i);
    }

    fn arg_files_insert_front(&mut self, p: *const c_char) {
        (self.0.arg_files_insert_front)(self.0.ctx, p);
    }

    fn expand_arg_files(&mut self) {
        (self.0.expand_arg_files)(self.0.ctx);
    }

    fn check_arg_file(&mut self, p: *const c_char, no_dereference: bool) -> ArgFile {
        match (self.0.check_arg_file)(self.0.ctx, p, c_int::from(no_dereference)) {
            0 => ArgFile::Missing,
            1 => ArgFile::Symlink,
            2 => ArgFile::Unreadable,
            3 => ArgFile::Directory,
            _ => ArgFile::Other,
        }
    }

    fn trim_pathname_arg(&mut self, p: *const c_char) {
        (self.0.trim_pathname_arg)(self.0.ctx, p);
    }

    fn file_open(&mut self, name: &[u8]) -> bool {
        (self.0.file_open)(self.0.ctx, cstr(name).as_ptr()) != 0
    }

    fn file_getline(&mut self) -> (bool, usize) {
        let mut len = 0;
        let eof = (self.0.file_getline)(self.0.ctx, &mut len) != 0;
        (eof, len)
    }

    fn arg_files_push_line(&mut self) {
        (self.0.arg_files_push_line)(self.0.ctx);
    }

    fn file_close(&mut self) {
        (self.0.file_close)(self.0.ctx);
    }

    fn import_globs(&mut self, include: bool, gitignore: bool) {
        (self.0.import_globs)(self.0.ctx, c_int::from(include), c_int::from(gitignore));
    }

    fn fs_probe(&mut self, path: &[u8]) -> Option<u64> {
        let mut id = 0;
        ((self.0.fs_probe)(self.0.ctx, cstr(path).as_ptr(), &mut id) != 0).then_some(id)
    }

    fn fs_insert(&mut self, include: bool, id: u64) {
        (self.0.fs_insert)(self.0.ctx, c_int::from(include), id);
    }

    fn type_entry(&mut self, i: usize) -> Option<TypeEntry> {
        let mut name: *const c_char = std::ptr::null();
        let mut extensions: *const c_char = std::ptr::null();
        let mut filenames: *const c_char = std::ptr::null();
        let mut magic: *const c_char = std::ptr::null();
        if (self.0.type_entry)(self.0.ctx, i, &mut name, &mut extensions, &mut filenames, &mut magic) == 0 {
            return None;
        }
        Some(TypeEntry {
            name: opt_bytes(name).unwrap_or_default(),
            extensions: opt_bytes(extensions).unwrap_or_default(),
            filenames: opt_bytes(filenames),
            magic: opt_bytes(magic),
        })
    }

    fn encoding_entry(&mut self, i: usize) -> Option<(Vec<u8>, u16)> {
        let mut format: *const c_char = std::ptr::null();
        let mut encoding = 0;
        if (self.0.encoding_entry)(self.0.ctx, i, &mut format, &mut encoding) == 0 {
            return None;
        }
        Some((opt_bytes(format).unwrap_or_default(), encoding))
    }

    fn magic_pattern_assign(&mut self, regex: Option<&[u8]>) -> Option<Vec<u8>> {
        let regex = regex.map(cstr);
        opt_bytes((self.0.magic_pattern_assign)(self.0.ctx, opt_ptr(&regex)))
    }

    fn filter_magic_assign(&mut self, regex: &[u8]) -> Option<Vec<u8>> {
        opt_bytes((self.0.filter_magic_assign)(self.0.ctx, cstr(regex).as_ptr()))
    }

    fn filter_empty(&mut self) -> bool {
        (self.0.filter_empty)(self.0.ctx) != 0
    }

    fn isatty_stdin(&mut self) -> bool {
        (self.0.isatty_stdin)(self.0.ctx) != 0
    }

    fn set_binary_stdout(&mut self) {
        (self.0.set_binary_stdout)(self.0.ctx);
    }
}

/// `init(argc, argv)`.
///
/// # Safety
/// `env` points to a valid table whose callbacks accept `env.ctx`; `argv` has `argc` entries that are NULL or
/// NUL-terminated strings living for the whole process (the settings keep pointers into them and
/// `trim_pathname_arg` modifies them in place); no other thread touches the settings during the call.
#[export_name = "ugrep_core_init"]
pub unsafe extern "C" fn ugrep_core_init(env: *const InitEnv, argc: c_int, argv: *const *const c_char) {
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
        pathsep: u8::try_from(env.pathsep).unwrap_or(b'/'),
        os_win: env.os_win != 0,
        os_win_or_mingw: env.os_win_or_mingw != 0,
        have_libz: env.have_libz != 0,
        have_regex_perl: env.have_regex_perl != 0,
        have_stat_fs: env.have_stat_fs != 0,
        cygwin: env.cygwin != 0,
        max_hex_columns: env.max_hex_columns,
        encoding_null_data: env.encoding_null_data,
    };
    init::init(&mut CEnv(env), &consts, &argv);
}
