//! C ABI of the Grep seam's host callback interface (`signatures::search::GrepHost`): a table of C callbacks for what
//! stays in C++ (the `Grep` members, `Output`, `Stats` and the flags), in the style of `ZthreadEnv` of
//! `ffi/zthread.rs`. The table is fixed by the Prepare seam; the C++ facade fills it in the port child that first
//! switches a branch. Every callback receives the opaque `ctx` of the table first, bytes are a pointer and a length
//! without NUL, results of bytes are returned by pointer with the length written through an out parameter and stay
//! valid until the next callback. All callbacks are called on the searching thread only.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::rc::Rc;

use crate::signatures::search::raw;
use crate::signatures::search::{
    CnfBits, CnfResult, FileRef, FileSource, GrepFlags, GrepHost, InputHandle, MatcherHandle, MatcherTable, OwnedFile, RestlineData, SearchContext,
};
use crate::signatures::Bytes;

/// The flags are not in the table: they are Rust-owned (`ugrep_settings`, `ffi/settings.rs`), so no callback is needed.
/// A string flag is NULL or a NUL-terminated string owned by C++.
fn cstr(p: *const std::os::raw::c_char) -> Option<Bytes> {
    if p.is_null() {
        None
    } else {
        // SAFETY: a non-NULL string flag points to a NUL-terminated string owned by C++ for the life of the process.
        Some(unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes().to_vec())
    }
}

/// The `GrepFlags` snapshot of the current `ugrep_settings`; no C++ call.
pub fn flags_from_settings() -> GrepFlags {
    // SAFETY: the settings are written during option parsing and only read while searching; no other reference is
    // live here.
    let s = unsafe { &*crate::settings_ffi::UGREP_SETTINGS.as_ptr() };
    GrepFlags {
        after_context: s.flag_after_context,
        before_context: s.flag_before_context,
        any_line: s.flag_any_line,
        best_match: s.flag_best_match,
        binary_without_match: s.flag_binary_without_match != 0,
        brk: s.flag_break != 0,
        color: cstr(s.flag_color),
        count: s.flag_count,
        decompress: s.flag_decompress,
        devices_action: s.flag_devices_action,
        encoding_type: s.flag_encoding_type,
        files: s.flag_files,
        files_with_matches: s.flag_files_with_matches,
        format: cstr(s.flag_format),
        format_open: cstr(s.flag_format_open),
        format_close: cstr(s.flag_format_close),
        fuzzy: s.flag_fuzzy,
        hex: s.flag_hex,
        hex_after: s.flag_hex_after,
        hex_before: s.flag_hex_before,
        hex_columns: s.flag_hex_columns,
        invert_match: s.flag_invert_match,
        label: cstr(s.flag_label),
        line_number: s.flag_line_number != 0,
        match_: s.flag_match,
        max_count: s.flag_max_count,
        max_files: s.flag_max_files,
        max_line: s.flag_max_line,
        min_count: s.flag_min_count,
        min_line: s.flag_min_line,
        mode: s.flag_mode,
        multiline: s.flag_multiline,
        no_header: s.flag_no_header,
        no_messages: s.flag_no_messages,
        null: s.flag_null,
        only_matching: s.flag_only_matching,
        quiet: s.flag_quiet,
        replace: cstr(s.flag_replace),
        separator: cstr(s.flag_separator),
        separator_bar: cstr(s.flag_separator_bar),
        separator_dash: cstr(s.flag_separator_dash),
        stats: cstr(s.flag_stats),
        tag: cstr(s.flag_tag),
        text: s.flag_text != 0,
        ungroup: s.flag_ungroup != 0,
        with_filename: s.flag_with_filename,
        with_hex: s.flag_with_hex,
        zmax: s.flag_zmax,
    }
}

/// Identifiers of the `bool` and `size_t` members of `Grep`, the argument of `member_get` and `member_set`.
#[repr(C)]
#[derive(Clone, Copy)]
pub enum Member {
    Matches,
    Lineno,
    Heading,
    Stop,
    Binfile,
    Hex,
    Binary,
    RestlineSize,
    RestlineLast,
    Interactive,
}

/// How the C++ facade decodes `restline_data_get` and `restline_data_set`: 0 NULL, 1 offset in the matcher buffer,
/// 2 the saved `restline` string.
pub const RESTLINE_NONE: c_int = 0;
pub const RESTLINE_BUFFER: c_int = 1;
pub const RESTLINE_SAVED: c_int = 2;

/// The handler methods the branches call, the argument of `handler_op` (`a`..`d` are the arguments in order).
#[repr(C)]
#[derive(Clone, Copy)]
pub enum HandlerOp {
    OutputBeforeContext,
    SetAfterLineno,
    AddBeforeContextLine,
    AddBeforeContextMatch,
}

/// The `Output` operations that take no argument, the argument of `out_op`.
#[repr(C)]
#[derive(Clone, Copy)]
pub enum OutOp {
    Hold,
    Acquire,
    Release,
    CheckFlush,
    Nl,
    NlLf,
    HeaderFile,
    BinaryFileMatches,
    DumpDone,
    Launch,
}

/// Mirrors `struct ugrep_core_search_env` of `src/search_seam.h`; the order of the fields and of the enum values is
/// pinned by the tests below and by the header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GrepEnv {
    /// Opaque `Grep *` plus the search state; first argument of every callback.
    pub ctx: *mut c_void,
    pub member_get: extern "C" fn(*mut c_void, Member) -> usize,
    pub member_set: extern "C" fn(*mut c_void, Member, usize),
    pub partname: extern "C" fn(*mut c_void, *mut usize) -> *const u8,
    pub restline_output: extern "C" fn(*mut c_void, c_int, c_int, c_int),
    pub restline_save: extern "C" fn(*mut c_void),
    /// Returns `RESTLINE_*`; the offset in the buffer is written through the pointer for `RESTLINE_BUFFER`.
    pub restline_data_get: extern "C" fn(*mut c_void, *mut usize) -> c_int,
    pub restline_data_set: extern "C" fn(*mut c_void, c_int, usize),
    pub restline_advance: extern "C" fn(*mut c_void, usize),
    /// `restline_data[..restline_size]`, length through the pointer, NULL when `restline_data` is NULL.
    pub restline_bytes: extern "C" fn(*mut c_void, *mut usize) -> *const u8,
    /// The methods of the registered handler; `a`..`d` are the arguments of `HandlerOp`, unused ones are 0.
    pub handler_op: extern "C" fn(*mut c_void, HandlerOp, usize, usize, usize, usize),
    pub cnf_matching: extern "C" fn(*mut c_void, usize, usize, c_int) -> c_int,
    pub cnf_satisfied: extern "C" fn(*mut c_void, c_int) -> c_int,
    /// `matchers->size()`, or `usize::MAX` when `matchers` is NULL.
    pub matchers_len: extern "C" fn(*mut c_void) -> usize,
    /// Fills the matcher callback table of `matchers->at(index)` (any kind); returns 0 when there is none.
    pub matchers_matcher: extern "C" fn(*mut c_void, usize, *mut MatcherTable) -> c_int,
    pub matchers_reset: extern "C" fn(*mut c_void),
    pub out_eof: extern "C" fn(*mut c_void) -> c_int,
    pub out_holding: extern "C" fn(*mut c_void) -> c_int,
    pub out_op: extern "C" fn(*mut c_void, OutOp),
    pub out_chr: extern "C" fn(*mut c_void, u8),
    pub out_str: extern "C" fn(*mut c_void, *const u8, usize),
    pub out_utf8strn: extern "C" fn(*mut c_void, *const u8, usize, usize),
    pub out_num: extern "C" fn(*mut c_void, usize, c_int),
    pub out_header: extern "C" fn(*mut c_void, usize, usize, *const u8, usize, c_int),
    pub out_format_count: extern "C" fn(*mut c_void, *const u8, usize, usize),
    /// `out.format(...)`: format, matches, in/out `size_t *matching` (NULL for --format-open/-close), body, next.
    pub out_format: extern "C" fn(*mut c_void, *const u8, usize, usize, *mut usize, c_int, c_int) -> c_int,
    pub out_dump_hex: extern "C" fn(*mut c_void, i16, usize, *const u8, usize),
    pub out_dump_next: extern "C" fn(*mut c_void, usize),
    /// `Stats::found_file`, `found_part`, `undo_found_part`: 0, 1, 2; the result of the first two is the bool.
    pub stats_found: extern "C" fn(*mut c_void, c_int) -> c_int,
    pub stats_found_parts: extern "C" fn(*mut c_void) -> usize,
    pub stats_score_matches: extern "C" fn(*mut c_void, usize, usize),
    // the file members of Grep (S4c-2): FILE* and reflex::Input* are opaque pointers
    pub filename: extern "C" fn(*mut c_void, *mut usize) -> *const u8,
    pub file_in: extern "C" fn(*mut c_void) -> *mut c_void,
    pub set_file_in: extern "C" fn(*mut c_void, *mut c_void),
    pub input: extern "C" fn(*mut c_void) -> *mut c_void,
    pub input_file: extern "C" fn(*mut c_void) -> *mut c_void,
    pub input_set_file: extern "C" fn(*mut c_void, *mut c_void, c_int),
    pub input_clear: extern "C" fn(*mut c_void),
    pub input_get: extern "C" fn(*mut c_void, *mut u8, usize) -> usize,
    pub source: extern "C" fn(*mut c_void) -> *mut c_void,
    pub stdin_file: extern "C" fn(*mut c_void) -> *mut c_void,
    pub filter_magic_scan: extern "C" fn(*mut c_void, *mut c_void) -> usize,
    pub warning: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    /// The zthread slots are `Option`: a facade built without `HAVE_LIBZ` may leave them NULL (the facade fills stubs,
    /// but a NULL must never be UB).
    pub zthread_start: Option<extern "C" fn(*mut c_void, usize, *const c_char, *mut c_void, *const c_char) -> *mut c_void>,
    pub zthread_open_next: Option<extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    pub zthread_cancel: Option<extern "C" fn(*mut c_void)>,
    pub cnf_alts: extern "C" fn(*mut c_void, usize) -> usize,
    pub cnf_alt_matcher: extern "C" fn(*mut c_void, usize, usize, *mut MatcherTable) -> c_int,
    pub cnf_bit_get: extern "C" fn(*mut c_void, c_int, usize, usize) -> c_int,
    pub cnf_bit_set: extern "C" fn(*mut c_void, c_int, usize, usize),
    /// 1 when a callback of the facade threw: the exception is stored on the C++ side (SEAM.md, "Exception channel").
    pub error_pending: extern "C" fn(*mut c_void) -> c_int,
    pub restline_set: extern "C" fn(*mut c_void, *const u8, usize),
    pub input_encoding: extern "C" fn(*mut c_void) -> c_int,
    pub encoding_null_data: extern "C" fn(*mut c_void) -> c_int,
    pub flag_filter: extern "C" fn(*mut c_void, *mut usize) -> *const u8,
    /// `flag_filter_magic_label[index]`, NULL past the last.
    pub flag_filter_magic_label: extern "C" fn(*mut c_void, usize, *mut usize) -> *const u8,
    pub file_open: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pub file_close: extern "C" fn(*mut c_void, *mut c_void),
    pub file_read_error: extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> c_int,
    pub file_set_binary: extern "C" fn(*mut c_void, *mut c_void),
    pub file_nonblock_regular: extern "C" fn(*mut c_void, *mut c_void),
    pub file_probe_interactive: extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub file_seek_set: extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub stdin_drain_check: extern "C" fn(*mut c_void) -> c_int,
    /// Returns 0 when the pipe could not be created; else 1 and the reading end (maybe NULL) through the last pointer.
    pub filter_spawn: extern "C" fn(*mut c_void, *mut c_void, *const c_char, *const c_char, *mut *mut c_void) -> c_int,
    pub is_binary: extern "C" fn(*mut c_void, *const u8, usize) -> c_int,
}

/// The C++ facade behind the callbacks table.
type AltCache = RefCell<HashMap<(usize, usize), Option<Rc<MatcherHandle>>>>;
struct CHost(GrepEnv, AltCache);

/// A NUL-terminated copy for the `const char *` arguments; a NUL inside the bytes ends the string, as in C++.
fn c_string(s: &[u8]) -> CString {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    CString::new(&s[..end]).unwrap_or_default()
}

fn bytes_of(ptr: *const u8, len: usize) -> Bytes {
    if ptr.is_null() {
        Bytes::new()
    } else {
        // SAFETY: the callback returned `len` readable bytes at `ptr`, valid until the next callback.
        unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
    }
}

impl CHost {
    fn get(&self, m: Member) -> usize {
        (self.0.member_get)(self.0.ctx, m)
    }

    fn set(&self, m: Member, v: usize) {
        (self.0.member_set)(self.0.ctx, m, v)
    }

    fn op(&self, op: OutOp) {
        (self.0.out_op)(self.0.ctx, op)
    }

    /// The `FILE*` that a callback receives for a [`FileSource`] (NULL for `None`); an owned file is given up.
    fn source_ptr(&self, file: Option<FileSource>) -> *mut c_void {
        match file {
            None => std::ptr::null_mut(),
            Some(FileSource::Stdin) => (self.0.stdin_file)(self.0.ctx),
            Some(FileSource::Owned(file)) => raw::file_into_ptr(file),
        }
    }
}

impl GrepHost for CHost {
    fn matches(&self) -> usize {
        self.get(Member::Matches)
    }
    fn set_matches(&self, matches: usize) {
        self.set(Member::Matches, matches)
    }
    fn lineno(&self) -> usize {
        self.get(Member::Lineno)
    }
    fn set_lineno(&self, lineno: usize) {
        self.set(Member::Lineno, lineno)
    }
    fn heading(&self) -> bool {
        self.get(Member::Heading) != 0
    }
    fn set_heading(&self, heading: bool) {
        self.set(Member::Heading, heading as usize)
    }
    fn stop(&self) -> bool {
        self.get(Member::Stop) != 0
    }
    fn set_stop(&self, stop: bool) {
        self.set(Member::Stop, stop as usize)
    }
    fn binfile(&self) -> bool {
        self.get(Member::Binfile) != 0
    }
    fn set_binfile(&self, binfile: bool) {
        self.set(Member::Binfile, binfile as usize)
    }
    fn hex(&self) -> bool {
        self.get(Member::Hex) != 0
    }
    fn set_hex(&self, hex: bool) {
        self.set(Member::Hex, hex as usize)
    }
    fn binary(&self) -> bool {
        self.get(Member::Binary) != 0
    }
    fn set_binary(&self, binary: bool) {
        self.set(Member::Binary, binary as usize)
    }
    fn partname(&self) -> Bytes {
        let mut len = 0usize;
        let ptr = (self.0.partname)(self.0.ctx, &mut len);
        bytes_of(ptr, len)
    }

    fn restline_size(&self) -> usize {
        self.get(Member::RestlineSize)
    }
    fn set_restline_size(&self, size: usize) {
        self.set(Member::RestlineSize, size)
    }
    fn restline_last(&self) -> usize {
        self.get(Member::RestlineLast)
    }
    fn set_restline_last(&self, last: usize) {
        self.set(Member::RestlineLast, last)
    }
    fn restline_data(&self) -> RestlineData {
        let mut offset = 0usize;
        match (self.0.restline_data_get)(self.0.ctx, &mut offset) {
            RESTLINE_NONE => RestlineData::None,
            RESTLINE_BUFFER => RestlineData::Buffer(offset),
            _ => RestlineData::Saved(offset),
        }
    }
    fn set_restline_data(&self, data: RestlineData) {
        let (kind, offset) = match data {
            RestlineData::None => (RESTLINE_NONE, 0),
            RestlineData::Buffer(offset) => (RESTLINE_BUFFER, offset),
            RestlineData::Saved(offset) => (RESTLINE_SAVED, offset),
        };
        (self.0.restline_data_set)(self.0.ctx, kind, offset)
    }
    fn restline_advance(&self, n: usize) {
        (self.0.restline_advance)(self.0.ctx, n)
    }
    fn restline_bytes(&self) -> Bytes {
        let mut len = 0usize;
        let ptr = (self.0.restline_bytes)(self.0.ctx, &mut len);
        bytes_of(ptr, len)
    }
    fn restline_set(&self, bytes: &[u8]) {
        (self.0.restline_set)(self.0.ctx, bytes.as_ptr(), bytes.len())
    }
    fn handler_output_before_context(&self) {
        (self.0.handler_op)(self.0.ctx, HandlerOp::OutputBeforeContext, 0, 0, 0, 0)
    }
    fn handler_set_after_lineno(&self, lineno: usize) {
        (self.0.handler_op)(self.0.ctx, HandlerOp::SetAfterLineno, lineno, 0, 0, 0)
    }
    fn handler_add_before_context_line(&self, bol: usize, eol: usize, columno: usize, offset: usize) {
        (self.0.handler_op)(self.0.ctx, HandlerOp::AddBeforeContextLine, bol, eol, columno, offset)
    }
    fn handler_add_before_context_match(&self, pos: usize, size: usize, offset: usize) {
        (self.0.handler_op)(self.0.ctx, HandlerOp::AddBeforeContextMatch, pos, size, offset, 0)
    }
    fn restline_output(&self, invert: bool, hex_context: bool) {
        (self.0.restline_output)(self.0.ctx, 0, invert as c_int, hex_context as c_int)
    }
    fn restline_output_or_save(&self, invert: bool, hex_context: bool) {
        (self.0.restline_output)(self.0.ctx, 1, invert as c_int, hex_context as c_int)
    }
    fn restline_save(&self) {
        (self.0.restline_save)(self.0.ctx)
    }

    fn cnf_matching(&self, bol: usize, eol: usize, acquire: bool) -> CnfResult {
        CnfResult((self.0.cnf_matching)(self.0.ctx, bol, eol, acquire as c_int) as u8)
    }
    fn cnf_satisfied(&self, acquire: bool) -> CnfResult {
        CnfResult((self.0.cnf_satisfied)(self.0.ctx, acquire as c_int) as u8)
    }
    fn out_launch(&self) {
        self.op(OutOp::Launch)
    }
    fn cnf_alts(&self, term: usize) -> usize {
        (self.0.cnf_alts)(self.0.ctx, term)
    }
    fn cnf_alt_matcher(&self, term: usize, alt: usize) -> Option<Rc<MatcherHandle>> {
        if let Some(cached) = self.1.borrow().get(&(term, alt)) {
            return cached.clone();
        }
        let mut table = std::mem::MaybeUninit::<MatcherTable>::uninit();
        let handle = if (self.0.cnf_alt_matcher)(self.0.ctx, term, alt, table.as_mut_ptr()) != 0 {
            // SAFETY: a nonzero return means the callback filled the whole table, as for `matchers_matcher`.
            Some(Rc::new(unsafe { raw::new_handle(table.assume_init()) }))
        } else {
            None
        };
        // a failed callback (pending error) is not cached
        if !self.error_pending() {
            self.1.borrow_mut().insert((term, alt), handle.clone());
        }
        handle
    }
    fn cnf_bit_get(&self, which: CnfBits, k: usize, l: usize) -> bool {
        (self.0.cnf_bit_get)(self.0.ctx, which as c_int, k, l) != 0
    }
    fn cnf_bit_set(&self, which: CnfBits, k: usize, l: usize) {
        (self.0.cnf_bit_set)(self.0.ctx, which as c_int, k, l)
    }
    fn error_pending(&self) -> bool {
        (self.0.error_pending)(self.0.ctx) != 0
    }

    fn filename(&self) -> Option<Bytes> {
        let mut len = 0usize;
        let ptr = (self.0.filename)(self.0.ctx, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(bytes_of(ptr, len))
        }
    }
    fn interactive(&self) -> bool {
        self.get(Member::Interactive) != 0
    }
    fn set_interactive(&self, interactive: bool) {
        self.set(Member::Interactive, interactive as usize)
    }
    fn file_in(&self) -> Option<FileRef<'_>> {
        // SAFETY: the facade returns the `FILE*` of `Grep::file_in` (or NULL), which C++ keeps open while the host is borrowed.
        unsafe { raw::file_ref((self.0.file_in)(self.0.ctx)) }
    }
    fn set_file_in(&self, file: Option<FileSource>) {
        let ptr = self.source_ptr(file);
        (self.0.set_file_in)(self.0.ctx, ptr)
    }
    fn take_file_in(&self) -> Option<FileSource> {
        let ptr = (self.0.file_in)(self.0.ctx);
        (self.0.set_file_in)(self.0.ctx, std::ptr::null_mut());
        if ptr.is_null() {
            None
        } else if ptr == (self.0.stdin_file)(self.0.ctx) {
            Some(FileSource::Stdin)
        } else {
            // SAFETY: `ptr` was `Grep::file_in`, now cleared, so the caller is the only owner of the open stream.
            unsafe { raw::owned_file(ptr) }.map(FileSource::Owned)
        }
    }
    fn input(&self) -> InputHandle {
        // SAFETY: the facade returns the address of `Grep::input`, which lives as long as the `Grep`.
        unsafe { raw::input_handle((self.0.input)(self.0.ctx) as *const c_void) }
    }
    fn input_file(&self) -> Option<FileRef<'_>> {
        // SAFETY: the facade returns `Grep::input.file()` (or NULL), owned by C++.
        unsafe { raw::file_ref((self.0.input_file)(self.0.ctx)) }
    }
    fn input_encoding(&self) -> i32 {
        (self.0.input_encoding)(self.0.ctx)
    }
    fn input_set_file(&self, file: &FileRef<'_>, encoding: i32) {
        (self.0.input_set_file)(self.0.ctx, raw::file_ptr(file), encoding)
    }
    fn input_clear(&self) {
        (self.0.input_clear)(self.0.ctx)
    }
    fn input_get(&self, buf: &mut [u8]) -> usize {
        (self.0.input_get)(self.0.ctx, buf.as_mut_ptr(), buf.len())
    }
    fn source(&self) -> Option<FileRef<'_>> {
        // SAFETY: the facade returns `Static::source` (or NULL), owned by C++.
        unsafe { raw::file_ref((self.0.source)(self.0.ctx)) }
    }
    fn stdin_file(&self) -> FileRef<'_> {
        // SAFETY: the facade returns `stdin`, which C++ never closes; the facade turns NULL into an error before it gets here.
        unsafe { raw::file_ref((self.0.stdin_file)(self.0.ctx)) }.expect("stdin is not NULL")
    }
    fn encoding_null_data(&self) -> i32 {
        (self.0.encoding_null_data)(self.0.ctx)
    }
    fn flag_filter(&self) -> Bytes {
        let mut len = 0usize;
        let ptr = (self.0.flag_filter)(self.0.ctx, &mut len);
        bytes_of(ptr, len)
    }
    fn flag_filter_magic_label(&self, index: usize) -> Option<Bytes> {
        let mut len = 0usize;
        let ptr = (self.0.flag_filter_magic_label)(self.0.ctx, index, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(bytes_of(ptr, len))
        }
    }
    fn filter_magic_scan(&self, file: &FileRef<'_>) -> usize {
        (self.0.filter_magic_scan)(self.0.ctx, raw::file_ptr(file))
    }
    fn warning(&self, message: &[u8], arg: &[u8]) {
        let (message, arg) = (c_string(message), c_string(arg));
        (self.0.warning)(self.0.ctx, message.as_ptr(), arg.as_ptr())
    }
    fn file_open(&self, pathname: &[u8]) -> Option<OwnedFile> {
        let pathname = c_string(pathname);
        // SAFETY: the facade returns the `FILE*` of `fopen` (or NULL), which the caller has to close.
        unsafe { raw::owned_file((self.0.file_open)(self.0.ctx, pathname.as_ptr())) }
    }
    fn file_close(&self, file: OwnedFile) {
        (self.0.file_close)(self.0.ctx, raw::file_into_ptr(file))
    }
    fn file_read_error(&self, file: &FileRef<'_>, pathname: &[u8]) -> bool {
        let pathname = c_string(pathname);
        (self.0.file_read_error)(self.0.ctx, raw::file_ptr(file), pathname.as_ptr()) != 0
    }
    fn file_set_binary(&self, file: &FileRef<'_>) {
        (self.0.file_set_binary)(self.0.ctx, raw::file_ptr(file))
    }
    fn file_nonblock_regular(&self, file: &FileRef<'_>) {
        (self.0.file_nonblock_regular)(self.0.ctx, raw::file_ptr(file))
    }
    fn file_probe_interactive(&self, file: &FileRef<'_>) -> bool {
        (self.0.file_probe_interactive)(self.0.ctx, raw::file_ptr(file)) != 0
    }
    fn file_seek_set(&self, file: &FileRef<'_>) -> bool {
        (self.0.file_seek_set)(self.0.ctx, raw::file_ptr(file)) != 0
    }
    fn stdin_drain_check(&self) -> bool {
        (self.0.stdin_drain_check)(self.0.ctx) != 0
    }
    fn filter_spawn(&self, file: FileSource, pathname: &[u8], command: &[u8]) -> Option<OwnedFile> {
        let (pathname, command) = (c_string(pathname), c_string(command));
        let mut out: *mut c_void = std::ptr::null_mut();
        if (self.0.filter_spawn)(self.0.ctx, self.source_ptr(Some(file)), pathname.as_ptr(), command.as_ptr(), &mut out) != 0 {
            // SAFETY: the facade returns the `FILE*` of `fdopen` (or NULL), which the caller has to close.
            unsafe { raw::owned_file(out) }
        } else {
            None
        }
    }
    fn is_binary(&self, bytes: &[u8]) -> bool {
        (self.0.is_binary)(self.0.ctx, bytes.as_ptr(), bytes.len()) != 0
    }
    fn zthread_start(&self, ztstage: usize, pathname: &[u8], file_in: FileSource, find: Option<&[u8]>) -> Option<OwnedFile> {
        let Some(start) = self.0.zthread_start else {
            // a file that is given up and cannot be consumed is closed here
            if let FileSource::Owned(file) = file_in {
                self.file_close(file);
            }
            return None;
        };
        let pathname = c_string(pathname);
        let find = find.map(c_string);
        let find_ptr = find.as_ref().map_or(std::ptr::null(), |f| f.as_ptr());
        let file_in = self.source_ptr(Some(file_in));
        // SAFETY: the facade returns the `FILE*` of the decompression pipe (or NULL), which the caller has to close.
        unsafe { raw::owned_file(start(self.0.ctx, ztstage, pathname.as_ptr(), file_in, find_ptr)) }
    }
    fn zthread_open_next(&self, pathname: &[u8]) -> Option<OwnedFile> {
        let open_next = self.0.zthread_open_next?;
        let pathname = c_string(pathname);
        // SAFETY: the facade returns the `FILE*` of the next decompression pipe (or NULL), which the caller has to close.
        unsafe { raw::owned_file(open_next(self.0.ctx, pathname.as_ptr())) }
    }
    fn zthread_cancel(&self) {
        if let Some(cancel) = self.0.zthread_cancel {
            cancel(self.0.ctx)
        }
    }
    fn matchers_len(&self) -> Option<usize> {
        match (self.0.matchers_len)(self.0.ctx) {
            usize::MAX => None,
            n => Some(n),
        }
    }
    fn matchers_matcher(&self, index: usize) -> Option<Rc<MatcherHandle>> {
        // the first alternative of the AND term: the one cached handle of that matcher
        self.cnf_alt_matcher(index, 0)
    }
    fn matchers_reset(&self) {
        (self.0.matchers_reset)(self.0.ctx)
    }

    fn out_eof(&self) -> bool {
        (self.0.out_eof)(self.0.ctx) != 0
    }
    fn out_hold(&self) {
        self.op(OutOp::Hold)
    }
    fn out_holding(&self) -> bool {
        (self.0.out_holding)(self.0.ctx) != 0
    }
    fn out_acquire(&self) {
        self.op(OutOp::Acquire)
    }
    fn out_release(&self) {
        self.op(OutOp::Release)
    }
    fn out_check_flush(&self) {
        self.op(OutOp::CheckFlush)
    }
    fn out_chr(&self, c: u8) {
        (self.0.out_chr)(self.0.ctx, c)
    }
    fn out_str(&self, s: &[u8]) {
        (self.0.out_str)(self.0.ctx, s.as_ptr(), s.len())
    }
    fn out_utf8strn(&self, s: &[u8], k: usize) {
        (self.0.out_utf8strn)(self.0.ctx, s.as_ptr(), s.len(), k)
    }
    fn out_num(&self, i: usize, width: i32) {
        (self.0.out_num)(self.0.ctx, i, width)
    }
    fn out_nl(&self, lf_only: bool) {
        self.op(if lf_only { OutOp::NlLf } else { OutOp::Nl })
    }
    fn out_header(&self, lineno: usize, byte_offset: usize, sep: &[u8], newline: bool) {
        (self.0.out_header)(self.0.ctx, lineno, byte_offset, sep.as_ptr(), sep.len(), newline as c_int)
    }
    fn out_header_file(&self) {
        self.op(OutOp::HeaderFile)
    }
    fn out_binary_file_matches(&self) {
        self.op(OutOp::BinaryFileMatches)
    }
    fn out_format_count(&self, format: &[u8], matches: usize) {
        (self.0.out_format_count)(self.0.ctx, format.as_ptr(), format.len(), matches)
    }
    fn out_format(&self, format: &[u8], matches: usize, matching: Option<&mut usize>, body: bool, next: bool) -> bool {
        let matching = matching.map_or(std::ptr::null_mut(), |m| m as *mut usize);
        (self.0.out_format)(self.0.ctx, format.as_ptr(), format.len(), matches, matching, body as c_int, next as c_int) != 0
    }
    fn out_dump_hex(&self, mode: i16, byte_offset: usize, data: &[u8]) {
        (self.0.out_dump_hex)(self.0.ctx, mode, byte_offset, data.as_ptr(), data.len())
    }
    fn out_dump_next(&self, byte_offset: usize) {
        (self.0.out_dump_next)(self.0.ctx, byte_offset)
    }
    fn out_dump_done(&self) {
        self.op(OutOp::DumpDone)
    }

    fn stats_found_file(&self) -> bool {
        (self.0.stats_found)(self.0.ctx, 0) != 0
    }
    fn stats_found_part(&self) -> bool {
        (self.0.stats_found)(self.0.ctx, 1) != 0
    }
    fn stats_undo_found_part(&self) {
        (self.0.stats_found)(self.0.ctx, 2);
    }
    fn stats_found_parts(&self) -> usize {
        (self.0.stats_found_parts)(self.0.ctx)
    }
    fn stats_score_matches(&self, matches: usize, lines: usize) {
        (self.0.stats_score_matches)(self.0.ctx, matches, lines)
    }
}

/// Attach the C++ host to a search context.
///
/// # Safety
/// `ctx` must point to a live `SearchContext` and `env` to a valid table whose `ctx` outlives the search context; the
/// callbacks are called on the calling thread only.
#[no_mangle]
pub unsafe extern "C" fn ugrep_core_search_set_host(ctx: *const SearchContext, env: *const GrepEnv) {
    // SAFETY: the caller guarantees both pointers are valid.
    let (ctx, env) = unsafe { (&*ctx, *env) };
    ctx.set_host(Box::new(CHost(env, RefCell::new(HashMap::new()))));
    ctx.set_flags(flags_from_settings());
}

/// Make the handle of a C++ matcher: `table` is filled by the facade, is copied, and its `ctx` must outlive the handle.
/// The result is passed to the cxx matcher functions as their `MatcherHandle` argument and freed with
/// [`ugrep_core_matcher_handle_free`]. There is one handle per C++ matcher (SEAM.md, "Matcher callback table").
///
/// # Safety
/// `table` must point to a fully filled `ugrep_core_matcher_table` whose entries honour `src/search_seam.h` for as
/// long as the handle lives.
#[no_mangle]
pub unsafe extern "C" fn ugrep_core_matcher_handle_new(table: *const MatcherTable) -> *mut MatcherHandle {
    // SAFETY: the caller guarantees `table` points to a filled table.
    let table = unsafe { crate::signatures::search::raw::read(table) };
    // SAFETY: the caller's table honours `src/search_seam.h` (this function's contract).
    Box::into_raw(Box::new(unsafe { crate::signatures::search::raw::new_handle(table) }))
}

/// Free a handle of [`ugrep_core_matcher_handle_new`]; this unregisters a Rust shift handler from the matcher first.
///
/// # Safety
/// `handle` must come from [`ugrep_core_matcher_handle_new`] and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn ugrep_core_matcher_handle_free(handle: *mut MatcherHandle) {
    if !handle.is_null() {
        // SAFETY: the caller guarantees `handle` is a live box made by `ugrep_core_matcher_handle_new`.
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    // the values of the C enums of src/search_seam.h
    #[test]
    fn enum_discriminants_match_the_header() {
        let members = [
            Member::Matches,
            Member::Lineno,
            Member::Heading,
            Member::Stop,
            Member::Binfile,
            Member::Hex,
            Member::Binary,
            Member::RestlineSize,
            Member::RestlineLast,
            Member::Interactive,
        ];
        for (i, m) in members.iter().enumerate() {
            assert_eq!(*m as i32, i as i32);
        }
        let ops = [
            OutOp::Hold,
            OutOp::Acquire,
            OutOp::Release,
            OutOp::CheckFlush,
            OutOp::Nl,
            OutOp::NlLf,
            OutOp::HeaderFile,
            OutOp::BinaryFileMatches,
            OutOp::DumpDone,
            OutOp::Launch,
        ];
        for (i, o) in ops.iter().enumerate() {
            assert_eq!(*o as i32, i as i32);
        }
        let handler_ops =
            [HandlerOp::OutputBeforeContext, HandlerOp::SetAfterLineno, HandlerOp::AddBeforeContextLine, HandlerOp::AddBeforeContextMatch];
        for (i, o) in handler_ops.iter().enumerate() {
            assert_eq!(*o as i32, i as i32);
        }
        assert_eq!((RESTLINE_NONE, RESTLINE_BUFFER, RESTLINE_SAVED), (0, 1, 2));
        assert_eq!(size_of::<Member>(), size_of::<c_int>());
        assert_eq!(size_of::<OutOp>(), size_of::<c_int>());
        assert_eq!(size_of::<HandlerOp>(), size_of::<c_int>());
    }

    // every member of the table is a pointer, in the order of `struct ugrep_core_search_env`
    #[test]
    fn env_layout_matches_the_header() {
        let w = size_of::<usize>();
        let offsets = [
            offset_of!(GrepEnv, ctx),
            offset_of!(GrepEnv, member_get),
            offset_of!(GrepEnv, member_set),
            offset_of!(GrepEnv, partname),
            offset_of!(GrepEnv, restline_output),
            offset_of!(GrepEnv, restline_save),
            offset_of!(GrepEnv, restline_data_get),
            offset_of!(GrepEnv, restline_data_set),
            offset_of!(GrepEnv, restline_advance),
            offset_of!(GrepEnv, restline_bytes),
            offset_of!(GrepEnv, handler_op),
            offset_of!(GrepEnv, cnf_matching),
            offset_of!(GrepEnv, cnf_satisfied),
            offset_of!(GrepEnv, matchers_len),
            offset_of!(GrepEnv, matchers_matcher),
            offset_of!(GrepEnv, matchers_reset),
            offset_of!(GrepEnv, out_eof),
            offset_of!(GrepEnv, out_holding),
            offset_of!(GrepEnv, out_op),
            offset_of!(GrepEnv, out_chr),
            offset_of!(GrepEnv, out_str),
            offset_of!(GrepEnv, out_utf8strn),
            offset_of!(GrepEnv, out_num),
            offset_of!(GrepEnv, out_header),
            offset_of!(GrepEnv, out_format_count),
            offset_of!(GrepEnv, out_format),
            offset_of!(GrepEnv, out_dump_hex),
            offset_of!(GrepEnv, out_dump_next),
            offset_of!(GrepEnv, stats_found),
            offset_of!(GrepEnv, stats_found_parts),
            offset_of!(GrepEnv, stats_score_matches),
            offset_of!(GrepEnv, filename),
            offset_of!(GrepEnv, file_in),
            offset_of!(GrepEnv, set_file_in),
            offset_of!(GrepEnv, input),
            offset_of!(GrepEnv, input_file),
            offset_of!(GrepEnv, input_set_file),
            offset_of!(GrepEnv, input_clear),
            offset_of!(GrepEnv, input_get),
            offset_of!(GrepEnv, source),
            offset_of!(GrepEnv, stdin_file),
            offset_of!(GrepEnv, filter_magic_scan),
            offset_of!(GrepEnv, warning),
            offset_of!(GrepEnv, zthread_start),
            offset_of!(GrepEnv, zthread_open_next),
            offset_of!(GrepEnv, zthread_cancel),
            offset_of!(GrepEnv, cnf_alts),
            offset_of!(GrepEnv, cnf_alt_matcher),
            offset_of!(GrepEnv, cnf_bit_get),
            offset_of!(GrepEnv, cnf_bit_set),
            offset_of!(GrepEnv, error_pending),
            offset_of!(GrepEnv, restline_set),
            offset_of!(GrepEnv, input_encoding),
            offset_of!(GrepEnv, encoding_null_data),
            offset_of!(GrepEnv, flag_filter),
            offset_of!(GrepEnv, flag_filter_magic_label),
            offset_of!(GrepEnv, file_open),
            offset_of!(GrepEnv, file_close),
            offset_of!(GrepEnv, file_read_error),
            offset_of!(GrepEnv, file_set_binary),
            offset_of!(GrepEnv, file_nonblock_regular),
            offset_of!(GrepEnv, file_probe_interactive),
            offset_of!(GrepEnv, file_seek_set),
            offset_of!(GrepEnv, stdin_drain_check),
            offset_of!(GrepEnv, filter_spawn),
            offset_of!(GrepEnv, is_binary),
        ];
        for (i, o) in offsets.iter().enumerate() {
            assert_eq!(*o, i * w, "field {i}");
        }
        assert_eq!(offsets.len(), 66);
        assert_eq!(size_of::<GrepEnv>(), offsets.len() * w);
        // an Option of a fn pointer is a pointer: the zthread slots may be NULL
        assert_eq!(size_of::<Option<extern "C" fn(*mut c_void)>>(), w);
    }
}
