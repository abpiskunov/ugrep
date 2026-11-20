//! C ABI of the Grep seam's host callback interface (`signatures::search::GrepHost`): a table of C callbacks for what
//! stays in C++ (the `Grep` members, `Output`, `Stats` and the flags), in the style of `ZthreadEnv` of
//! `ffi/zthread.rs`. The table is fixed by the Prepare seam; the C++ facade fills it in the port child that first
//! switches a branch. Every callback receives the opaque `ctx` of the table first, bytes are a pointer and a length
//! without NUL, results of bytes are returned by pointer with the length written through an out parameter and stay
//! valid until the next callback. All callbacks are called on the searching thread only.

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::signatures::search::{GrepFlags, GrepHost, MatcherHandle, MatcherKind, SearchContext};
use crate::signatures::Bytes;

/// Identifiers of the integer flags, the argument of `flag_int`.
#[repr(C)]
#[derive(Clone, Copy)]
pub enum FlagInt {
    AfterContext,
    BeforeContext,
    AnyLine,
    BestMatch,
    BinaryWithoutMatch,
    Break,
    Count,
    Files,
    FilesWithMatches,
    Fuzzy,
    Hex,
    HexAfter,
    HexBefore,
    HexColumns,
    InvertMatch,
    LineNumber,
    Match,
    MaxCount,
    MaxFiles,
    MaxLine,
    MinCount,
    Mode,
    Multiline,
    NoHeader,
    Null,
    OnlyMatching,
    Quiet,
    Ungroup,
    WithFilename,
    WithHex,
}

/// Identifiers of the string flags, the argument of `flag_str` (NULL result for a NULL flag).
#[repr(C)]
#[derive(Clone, Copy)]
pub enum FlagStr {
    Color,
    Format,
    FormatOpen,
    FormatClose,
    Label,
    Replace,
    Separator,
    SeparatorBar,
    SeparatorDash,
    Stats,
    Tag,
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

/// The `Output` operations that take a byte string or numbers, the argument of `out_op`.
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

/// Mirrors the table the C++ facade provides (a `struct ugrep_core_search_env` of the header added with the first
/// switched branch).
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
    pub cnf_matching: extern "C" fn(*mut c_void, usize, usize, c_int) -> c_int,
    pub cnf_satisfied: extern "C" fn(*mut c_void, c_int) -> c_int,
    /// `matchers->size()`, or `usize::MAX` when `matchers` is NULL.
    pub matchers_len: extern "C" fn(*mut c_void) -> usize,
    pub matchers_matcher: extern "C" fn(*mut c_void, usize, *mut u8, *mut usize) -> c_int,
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
    pub out_format: extern "C" fn(*mut c_void, *const u8, usize, usize, c_int, c_int) -> c_int,
    pub out_dump_hex: extern "C" fn(*mut c_void, i16, usize, *const u8, usize),
    pub out_dump_next: extern "C" fn(*mut c_void, usize),
    /// `Stats::found_file`, `found_part`, `undo_found_part`: 0, 1, 2; the result of the first two is the bool.
    pub stats_found: extern "C" fn(*mut c_void, c_int) -> c_int,
    pub stats_found_parts: extern "C" fn(*mut c_void) -> usize,
    pub stats_score_matches: extern "C" fn(*mut c_void, usize, usize),
    pub flag_int: extern "C" fn(*mut c_void, FlagInt) -> usize,
    pub flag_str: extern "C" fn(*mut c_void, FlagStr, *mut usize) -> *const u8,
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

    fn int(&self, f: FlagInt) -> usize {
        (self.0.flag_int)(self.0.ctx, f)
    }

    fn string(&self, f: FlagStr) -> Option<Bytes> {
        let mut len = 0usize;
        let ptr = (self.0.flag_str)(self.0.ctx, f, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(bytes_of(ptr, len))
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
        let (mut kind, mut token) = (0u8, 0usize);
        if (self.0.matchers_matcher)(self.0.ctx, index, &mut kind, &mut token) != 0 {
            Some(MatcherHandle::new(MatcherKind(kind), token))
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
    fn out_format(&self, format: &[u8], matches: usize, body: bool, next: bool) -> bool {
        (self.0.out_format)(self.0.ctx, format.as_ptr(), format.len(), matches, body as c_int, next as c_int) != 0
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

    fn flags(&self) -> GrepFlags {
        let b = |f: FlagInt| self.int(f) != 0;
        GrepFlags {
            after_context: self.int(FlagInt::AfterContext),
            before_context: self.int(FlagInt::BeforeContext),
            any_line: b(FlagInt::AnyLine),
            best_match: b(FlagInt::BestMatch),
            binary_without_match: b(FlagInt::BinaryWithoutMatch),
            brk: b(FlagInt::Break),
            color: self.string(FlagStr::Color),
            count: b(FlagInt::Count),
            files: b(FlagInt::Files),
            files_with_matches: b(FlagInt::FilesWithMatches),
            format: self.string(FlagStr::Format),
            format_open: self.string(FlagStr::FormatOpen),
            format_close: self.string(FlagStr::FormatClose),
            fuzzy: self.int(FlagInt::Fuzzy),
            hex: b(FlagInt::Hex),
            hex_after: self.int(FlagInt::HexAfter),
            hex_before: self.int(FlagInt::HexBefore),
            hex_columns: self.int(FlagInt::HexColumns),
            invert_match: b(FlagInt::InvertMatch),
            label: self.string(FlagStr::Label),
            line_number: b(FlagInt::LineNumber),
            match_: b(FlagInt::Match),
            max_count: self.int(FlagInt::MaxCount),
            max_files: self.int(FlagInt::MaxFiles),
            max_line: self.int(FlagInt::MaxLine),
            min_count: self.int(FlagInt::MinCount),
            mode: self.int(FlagInt::Mode) as i32,
            multiline: b(FlagInt::Multiline),
            no_header: b(FlagInt::NoHeader),
            null: b(FlagInt::Null),
            only_matching: b(FlagInt::OnlyMatching),
            quiet: b(FlagInt::Quiet),
            replace: self.string(FlagStr::Replace),
            separator: self.string(FlagStr::Separator),
            separator_bar: self.string(FlagStr::SeparatorBar),
            separator_dash: self.string(FlagStr::SeparatorDash),
            stats: self.string(FlagStr::Stats),
            tag: self.string(FlagStr::Tag),
            ungroup: b(FlagInt::Ungroup),
            with_filename: b(FlagInt::WithFilename),
            with_hex: b(FlagInt::WithHex),
        }
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
}
