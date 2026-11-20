//! C ABI of the Grep seam's host callback interface (`signatures::search::GrepHost`): a table of C callbacks for what
//! stays in C++ (the `Grep` members, `Output`, `Stats` and the flags), in the style of `ZthreadEnv` of
//! `ffi/zthread.rs`. The table is fixed by the Prepare seam; the C++ facade fills it in the port child that first
//! switches a branch. Every callback receives the opaque `ctx` of the table first, bytes are a pointer and a length
//! without NUL, results of bytes are returned by pointer with the length written through an out parameter and stay
//! valid until the next callback. All callbacks are called on the searching thread only.

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::signatures::search::{GrepFlags, GrepHost, MatcherHandle, MatcherTable, RestlineData, SearchContext};
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
        mode: s.flag_mode,
        multiline: s.flag_multiline,
        no_header: s.flag_no_header,
        null: s.flag_null,
        only_matching: s.flag_only_matching,
        quiet: s.flag_quiet,
        replace: cstr(s.flag_replace),
        separator: cstr(s.flag_separator),
        separator_bar: cstr(s.flag_separator_bar),
        separator_dash: cstr(s.flag_separator_dash),
        stats: cstr(s.flag_stats),
        tag: cstr(s.flag_tag),
        ungroup: s.flag_ungroup != 0,
        with_filename: s.flag_with_filename,
        with_hex: s.flag_with_hex,
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
}

/// The C++ facade behind the callbacks table.
struct CHost(GrepEnv);

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
            _ => RestlineData::Saved,
        }
    }
    fn set_restline_data(&self, data: RestlineData) {
        let (kind, offset) = match data {
            RestlineData::None => (RESTLINE_NONE, 0),
            RestlineData::Buffer(offset) => (RESTLINE_BUFFER, offset),
            RestlineData::Saved => (RESTLINE_SAVED, 0),
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

    fn cnf_matching(&self, bol: usize, eol: usize, acquire: bool) -> bool {
        (self.0.cnf_matching)(self.0.ctx, bol, eol, acquire as c_int) != 0
    }
    fn cnf_satisfied(&self, acquire: bool) -> bool {
        (self.0.cnf_satisfied)(self.0.ctx, acquire as c_int) != 0
    }
    fn matchers_len(&self) -> Option<usize> {
        match (self.0.matchers_len)(self.0.ctx) {
            usize::MAX => None,
            n => Some(n),
        }
    }
    fn matchers_matcher(&self, index: usize) -> Option<MatcherHandle> {
        let mut table = std::mem::MaybeUninit::<MatcherTable>::uninit();
        if (self.0.matchers_matcher)(self.0.ctx, index, table.as_mut_ptr()) != 0 {
            // SAFETY: a nonzero return means the callback filled the whole table, and the C++ facade's table honours
            // `src/search_seam.h` while the matcher lives.
            Some(unsafe { crate::signatures::search::raw::new_handle(table.assume_init()) })
        } else {
            None
        }
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
    ctx.set_host(Box::new(CHost(env)));
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
        ];
        for (i, o) in offsets.iter().enumerate() {
            assert_eq!(*o, i * w, "field {i}");
        }
        assert_eq!(size_of::<GrepEnv>(), offsets.len() * w);
    }
}
