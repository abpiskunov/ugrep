//! Types and signatures of the `Grep` search seam of `src/ugrep.cpp` (Prepare seam: Grep search), fixed by the
//! Prepare seam task. Declared from `signatures.rs` with `#[path]`, so the replay harness that includes
//! `signatures.rs` alone builds it; it names no `cxx` type and uses no crate path. Every body is a stub that panics
//! until a Port task; the Port never edits a signature (the signature change protocol of the plan applies).
//!
//! Two things live here:
//! - [`MatcherHandle`]: the matcher-handle API at the `reflex::AbstractMatcher` level (`find`/`next_match`,
//!   `bol`/`eol`/`begin`/`end`, text bounds, the buffer-shift event, `reset`). **Every matcher kind crosses the seam**
//!   ([`MatcherKind`]): `reflex::Matcher`, `PCRE2Matcher`, `LineMatcher` (`--match`), `FuzzyMatcher` (`-Z`, whose
//!   `-Zbest` calls are the explicit methods `fuzzy_distance` and `fuzzy_edits`) and `BoostPerlMatcher`. No search
//!   stays in C++.
//! - [`GrepHost`]: the callback interface to what stays in C++ (the `Grep` members and the flags), in the style of
//!   the Zthread `Host`; its C-ABI table is `ffi/search_host.rs`.
//! - [`SearchContext`]: the Rust-owned counterpart of `Grep::SearchContext`, the locals of `Grep::search` that its
//!   branches share, and the one stub per branch in [`branch`] (named as the C++ `Grep::search_*` functions).
//!
//! Re-entrancy shapes the signatures: the C++ matcher calls the buffer-shift handler *while* `find`, `next_match` or
//! `eol(true)` runs, and the handler reads the `pathname` of the context. So every method takes `&self` (the matcher
//! lives in C++ and mutates there; a Rust `&mut` held across the call would be aliased by the event) and the branches
//! take `&SearchContext` (`matched` is a `Cell`; the handle and the context are confined to the searching thread).
//!
//! The notes (encoding, re-entrancy, thread affinity, owner side, error kinds) are in `SEAM.md`, "Grep seam".

use std::cell::{Cell, RefCell};

use super::Bytes;

/// Kind of the matcher a handle stands for. A transparent newtype so the C++ values map 1:1; a kind that is not listed
/// here never gets a handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatcherKind(pub u8);

impl MatcherKind {
    /// `reflex::Matcher` (RE-flex DFA).
    pub const REFLEX: MatcherKind = MatcherKind(1);
    /// `reflex::PCRE2Matcher`.
    pub const PCRE2: MatcherKind = MatcherKind(2);
    /// `reflex::LineMatcher` (`--match`).
    pub const LINE: MatcherKind = MatcherKind(3);
    /// `reflex::FuzzyMatcher` (`-Z`).
    pub const FUZZY: MatcherKind = MatcherKind(4);
    /// `reflex::BoostPerlMatcher`.
    pub const BOOST: MatcherKind = MatcherKind(5);
}

/// Positions of the current match, as byte offsets from the start of the matcher's buffer (`reflex::AbstractMatcher`
/// pointers minus `buf_`), valid until the next buffer shift event. `text()` is never part of this: it writes a NUL
/// into the buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextBounds {
    /// `begin()`: first byte of the match.
    pub begin: usize,
    /// `end()`: one past the last byte of the match.
    pub end: usize,
    /// `first()`: offset of the match in the input.
    pub first: usize,
    /// `last()`: offset one past the match in the input.
    pub last: usize,
    /// `size()`: `end - begin`.
    pub size: usize,
    /// `bol()`: first byte of the line of the match.
    pub bol: usize,
    /// `eol()`: the end of the line of the match, after `eol(true)` when the line must be complete.
    pub eol: usize,
    /// `lineno()`.
    pub lineno: usize,
    /// `lines()`: number of lines the match spans.
    pub lines: usize,
}

/// The buffer-shift event: `reflex::AbstractMatcher::Handler::operator()(matcher, buf, len, num)`, called with the text
/// that is about to leave the buffer (`len` is 0 when the buffer only grows).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShiftEvent {
    /// `len`: number of bytes shifted out, `buf[..len]` is passed next to the event.
    pub len: usize,
    /// `num`: number of lines in the shifted out text.
    pub num: usize,
}

/// The callback side of the buffer-shift event: what the registered C++ handler (`GrepHandler` and its derived
/// handlers) does with it. Rust never owns the C++ handler; the Port receives it as this trait. It takes `&self`
/// because it runs re-entrantly inside `find`.
pub trait ShiftHandler {
    /// The matcher is about to shift `buf` (the text of the event) out of its buffer.
    fn on_shift(&self, event: ShiftEvent, buf: &[u8]);
}

/// Handle of a matcher of any kind that stays owned by C++ (`Grep::matcher`, one element of `Grep::matchers`). The handle borrows it:
/// it never frees it, and it must not outlive the `Grep::search` call it was made for. The Port decides how the calls
/// reach the C++ matcher (SEAM.md, "Grep seam", unknown ownership).
#[derive(Debug)]
pub struct MatcherHandle {
    pub(crate) kind: MatcherKind,
    /// Opaque token of the C++ matcher (never dereferenced here).
    pub(crate) token: usize,
}

impl MatcherHandle {
    /// A handle for the C++ matcher `token` of the given kind.
    pub fn new(kind: MatcherKind, token: usize) -> MatcherHandle {
        MatcherHandle { kind, token }
    }

    /// The kind of the matcher.
    pub fn kind(&self) -> MatcherKind {
        self.kind
    }

    /// The opaque token of the C++ matcher this handle borrows.
    pub fn token(&self) -> usize {
        self.token
    }

    /// `matcher->find()`: search for the next match from the current position; false when there is none. May fire
    /// shift events before it returns.
    pub fn find(&self) -> bool {
        panic!("Grep seam: MatcherHandle::find is not ported")
    }

    /// The match after the current one: `matcher->find()` resumes where the previous match ended, so this is the loop
    /// step of `while (matcher->find())`; with `skip_line` set the rest of the current line is skipped first
    /// (`matcher->skip('\n')`). May fire shift events.
    pub fn next_match(&self, _skip_line: bool) -> bool {
        panic!("Grep seam: MatcherHandle::next_match is not ported")
    }

    /// `matcher->bol()`: offset of the start of the line of the match. Call `eol` first, as the C++ does.
    pub fn bol(&self) -> usize {
        panic!("Grep seam: MatcherHandle::bol is not ported")
    }

    /// `matcher->eol(inclusive)`: offset of the end of the line of the match; `inclusive` is the `eol(true)` form that
    /// reads on until the whole line is in the buffer (and may shift it, firing shift events).
    pub fn eol(&self, _inclusive: bool) -> usize {
        panic!("Grep seam: MatcherHandle::eol is not ported")
    }

    /// `matcher->begin()`: offset of the first byte of the match.
    pub fn begin(&self) -> usize {
        panic!("Grep seam: MatcherHandle::begin is not ported")
    }

    /// `matcher->end()`: offset one past the match.
    pub fn end(&self) -> usize {
        panic!("Grep seam: MatcherHandle::end is not ported")
    }

    /// All the positions of the current match at once.
    pub fn text_bounds(&self) -> TextBounds {
        panic!("Grep seam: MatcherHandle::text_bounds is not ported")
    }

    /// `matcher->first()`: offset of the match in the input.
    pub fn first(&self) -> usize {
        panic!("Grep seam: MatcherHandle::first is not ported")
    }

    /// `matcher->last()`: offset one past the match in the input.
    pub fn last(&self) -> usize {
        panic!("Grep seam: MatcherHandle::last is not ported")
    }

    /// `matcher->size()`.
    pub fn size(&self) -> usize {
        panic!("Grep seam: MatcherHandle::size is not ported")
    }

    /// `matcher->lineno()`.
    pub fn lineno(&self) -> usize {
        panic!("Grep seam: MatcherHandle::lineno is not ported")
    }

    /// `matcher->lines()`: number of lines the match spans.
    pub fn lines(&self) -> usize {
        panic!("Grep seam: MatcherHandle::lines is not ported")
    }

    /// `matcher->columno()`.
    pub fn columno(&self) -> usize {
        panic!("Grep seam: MatcherHandle::columno is not ported")
    }

    /// `matcher->at_bol()`.
    pub fn at_bol(&self) -> bool {
        panic!("Grep seam: MatcherHandle::at_bol is not ported")
    }

    /// `matcher->hit_end()`.
    pub fn hit_end(&self) -> bool {
        panic!("Grep seam: MatcherHandle::hit_end is not ported")
    }

    /// `matcher->skip(c)`: skip input up to and including the byte `c`; false at the end of the input.
    pub fn skip(&self, _c: u8) -> bool {
        panic!("Grep seam: MatcherHandle::skip is not ported")
    }

    /// `matcher->border()`: the border offset of the match.
    pub fn border(&self) -> usize {
        panic!("Grep seam: MatcherHandle::border is not ported")
    }

    /// `matcher->before()`: the offset of the start of the text before the match.
    pub fn before(&self) -> usize {
        panic!("Grep seam: MatcherHandle::before is not ported")
    }

    /// `matcher->after()`: the offset of the end of the text after the match.
    pub fn after(&self) -> usize {
        panic!("Grep seam: MatcherHandle::after is not ported")
    }

    /// `matcher->bef()`.
    pub fn bef(&self) -> usize {
        panic!("Grep seam: MatcherHandle::bef is not ported")
    }

    /// `matcher->aft()`.
    pub fn aft(&self) -> usize {
        panic!("Grep seam: MatcherHandle::aft is not ported")
    }

    /// `matcher->set_reserve(size)`: reserve buffer space for a line kept across a buffer shift.
    pub fn set_reserve(&self, _size: usize) {
        panic!("Grep seam: MatcherHandle::set_reserve is not ported")
    }

    /// `FuzzyMatcher::distance(max)`, the `-Zbest` call: `max` is the cost in the lower byte and the INS, DEL, SUB and
    /// BIN flags in the upper byte. Only for [`MatcherKind::FUZZY`].
    pub fn fuzzy_distance(&self, _max: u16) {
        panic!("Grep seam: MatcherHandle::fuzzy_distance is not ported")
    }

    /// `FuzzyMatcher::edits()`, the `-Zbest` call of `compute_cost`: edit distance of the current match. Only for
    /// [`MatcherKind::FUZZY`].
    pub fn fuzzy_edits(&self) -> u16 {
        panic!("Grep seam: MatcherHandle::fuzzy_edits is not ported")
    }

    /// `matcher->lineno_skip(on)`: count only matching lines (cheaper) when on.
    pub fn lineno_skip(&self, _on: bool) {
        panic!("Grep seam: MatcherHandle::lineno_skip is not ported")
    }

    /// Register or clear the buffer-shift listener (`matcher->set_handler`); C++ keeps the handler object, the
    /// events come back through [`MatcherHandle::shift_event`]. The matcher forgets the handler when a new input
    /// source is assigned, so this is called once per file.
    pub fn set_shift_listener(&self, _enabled: bool) {
        panic!("Grep seam: MatcherHandle::set_shift_listener is not ported")
    }

    /// The buffer-shift event, forwarded by the C++ handler to the listener. Runs inside `find`, `next_match` or `eol`.
    pub fn shift_event(&self, _event: ShiftEvent, _buf: &[u8]) {
        panic!("Grep seam: MatcherHandle::shift_event is not ported")
    }

    /// `matcher->reset()`: back to the start of the input, drops the shift listener and the match.
    pub fn reset(&self) {
        panic!("Grep seam: MatcherHandle::reset is not ported")
    }
}

/// How a search branch ends (`Grep::SearchStep`): `CONTINUE` and `DONE` go on to the epilogue (`done_search`), `EXIT`
/// leaves it out (`exit_search`). The numbers are the C++ enum values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchStep(pub u8);

impl SearchStep {
    pub const CONTINUE: SearchStep = SearchStep(0);
    pub const DONE: SearchStep = SearchStep(1);
    pub const EXIT: SearchStep = SearchStep(2);
}

/// Snapshot of the `flag_*` globals the search branches read (`flag_before_context` and the like), taken by C++ at the
/// start of `search`. Strings are bytes, `None` is NULL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrepFlags {
    pub after_context: usize,
    pub before_context: usize,
    pub any_line: bool,
    pub best_match: bool,
    pub binary_without_match: bool,
    pub brk: bool,
    pub color: Option<Bytes>,
    pub count: bool,
    pub files: bool,
    pub files_with_matches: bool,
    pub format: Option<Bytes>,
    pub format_open: Option<Bytes>,
    pub format_close: Option<Bytes>,
    pub fuzzy: usize,
    pub hex: bool,
    pub hex_after: usize,
    pub hex_before: usize,
    pub hex_columns: usize,
    pub invert_match: bool,
    pub label: Option<Bytes>,
    pub line_number: bool,
    pub match_: bool,
    pub max_count: usize,
    pub max_files: usize,
    pub max_line: usize,
    pub min_count: usize,
    /// `flag_mode` (the C++ `Mode` enum value).
    pub mode: i32,
    pub multiline: bool,
    pub no_header: bool,
    pub null: bool,
    pub only_matching: bool,
    pub quiet: bool,
    pub replace: Option<Bytes>,
    pub separator: Option<Bytes>,
    pub separator_bar: Option<Bytes>,
    pub separator_dash: Option<Bytes>,
    pub stats: Option<Bytes>,
    pub tag: Option<Bytes>,
    pub ungroup: bool,
    pub with_filename: bool,
    pub with_hex: bool,
}

/// The callback interface of the Grep seam: everything a search branch uses of the C++ `Grep` object, its `Output`,
/// `Stats` and the flags, in the style of the Zthread `Host` (C-ABI table in `ffi/search_host.rs`). It is fixed by the
/// Prepare seam so that no Port child edits a signature. Methods take `&self`: C++ mutates behind them and calls back
/// into the matcher handle re-entrantly. Bytes carry no NUL; positions are offsets into the matcher buffer, as in
/// [`TextBounds`]. Used on the searching thread only.
pub trait GrepHost {
    // `Grep::matches`, `lineno`, `heading`, `stop`, `binfile`, `hex`, `binary`, `partname`
    fn matches(&self) -> usize;
    fn set_matches(&self, matches: usize);
    fn lineno(&self) -> usize;
    fn set_lineno(&self, lineno: usize);
    fn heading(&self) -> bool;
    fn set_heading(&self, heading: bool);
    fn stop(&self) -> bool;
    fn set_stop(&self, stop: bool);
    fn binfile(&self) -> bool;
    fn set_binfile(&self, binfile: bool);
    fn hex(&self) -> bool;
    fn set_hex(&self, hex: bool);
    fn binary(&self) -> bool;
    fn set_binary(&self, binary: bool);
    fn partname(&self) -> Bytes;

    // `Grep::restline*` and `output_or_save_restline`, `output_restline`, `save_restline`
    fn restline_size(&self) -> usize;
    fn set_restline_size(&self, size: usize);
    fn restline_last(&self) -> usize;
    fn set_restline_last(&self, last: usize);
    fn restline_output(&self, invert: bool, hex_context: bool);
    fn restline_output_or_save(&self, invert: bool, hex_context: bool);
    fn restline_save(&self);

    // the CNF of `--files` / `-e -e`: `cnf_matching`, `cnf_satisfied`, `matchers`, `matching`, `notmatching`
    fn cnf_matching(&self, bol: usize, eol: usize, acquire: bool) -> bool;
    fn cnf_satisfied(&self, acquire: bool) -> bool;
    /// `matchers->size()`, `None` when `matchers` is NULL.
    fn matchers_len(&self) -> Option<usize>;
    /// The matcher handle of element `index` of `matchers` (`matchers->at(index)`), any kind.
    fn matchers_matcher(&self, index: usize) -> Option<MatcherHandle>;
    /// `--files`: reset the `matching` and `notmatching` bitmasks for the `matchers` (the body of `search_setup`).
    fn matchers_reset(&self);

    // `Grep::out` (`Output`)
    fn out_eof(&self) -> bool;
    fn out_hold(&self);
    fn out_holding(&self) -> bool;
    fn out_acquire(&self);
    fn out_release(&self);
    fn out_check_flush(&self);
    fn out_chr(&self, c: u8);
    fn out_str(&self, s: &[u8]);
    fn out_utf8strn(&self, s: &[u8], k: usize);
    fn out_num(&self, i: usize, width: i32);
    fn out_nl(&self, lf_only: bool);
    /// `out.header(pathname, partname, heading, lineno, matcher, byte_offset, sep, newline)`.
    fn out_header(&self, lineno: usize, byte_offset: usize, sep: &[u8], newline: bool);
    /// `out.header(pathname, partname)`.
    fn out_header_file(&self);
    fn out_binary_file_matches(&self);
    /// `out.format(format, matches)`.
    fn out_format_count(&self, format: &[u8], matches: usize);
    /// `out.format(format, pathname, partname, matches, matching, matcher, heading, body, next)`.
    fn out_format(&self, format: &[u8], matches: usize, body: bool, next: bool) -> bool;
    fn out_dump_hex(&self, mode: i16, byte_offset: usize, data: &[u8]);
    fn out_dump_next(&self, byte_offset: usize);
    fn out_dump_done(&self);

    // `Stats`
    fn stats_found_file(&self) -> bool;
    fn stats_found_part(&self) -> bool;
    fn stats_undo_found_part(&self);
    fn stats_found_parts(&self) -> usize;
    fn stats_score_matches(&self, matches: usize, lines: usize);

    /// The flags the branches read.
    fn flags(&self) -> GrepFlags;
}

/// The locals of `Grep::search` that its branches share (`Grep::SearchContext`). Rust-owned; the C++ `Grep` members
/// (`matches`, `lineno`, `out`, `partname`, ...) are reached through the [`GrepHost`] it carries. Read by the shift
/// handler while a branch runs, so branches get it by shared reference.
#[derive(Default)]
pub struct SearchContext {
    /// `pathname` after `flag_label` replaced the standard input label. Bytes, no NUL.
    pub pathname: Bytes,
    /// The `cost` argument of `search`, used before the branches only.
    pub cost: u16,
    /// This file or archive has a match (set by the epilogue).
    pub matched: Cell<bool>,
    host: RefCell<Option<Box<dyn GrepHost>>>,
}

impl SearchContext {
    /// A context for `search(pathname, cost)`; `matched` starts false and no host is attached yet.
    pub fn new(pathname: &[u8], cost: u16) -> SearchContext {
        SearchContext { pathname: pathname.to_vec(), cost, matched: Cell::new(false), host: RefCell::new(None) }
    }

    /// Attach the callback interface to the C++ `Grep`; the shell does it once before the first branch.
    pub fn set_host(&self, host: Box<dyn GrepHost>) {
        *self.host.borrow_mut() = Some(host);
    }

    /// Run `f` with the host. Panics when none was attached: the shell always attaches one.
    pub fn with_host<R>(&self, f: impl FnOnce(&dyn GrepHost) -> R) -> R {
        match self.host.borrow().as_deref() {
            Some(host) => f(host),
            None => panic!("Grep seam: no GrepHost attached to the SearchContext"),
        }
    }
}

/// One stub per search branch, named as the C++ `Grep::search_*` functions; a later Port unit replaces one each. They
/// take the context and the matcher handle; what a branch needs of the C++ `Grep` members is decided by that unit.
pub mod branch {
    use super::{MatcherHandle, SearchContext, SearchStep};

    /// `Grep::search_setup`: `--files` resets the matching bitmasks.
    pub fn setup(_ctx: &SearchContext) {
        panic!("Grep seam: branch::setup is not ported")
    }

    /// `Grep::search_quiet_list`: `-q`, `-l`, `-L`.
    pub fn quiet_list(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::quiet_list is not ported")
    }

    /// `Grep::search_count`: `-c`.
    pub fn count(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::count is not ported")
    }

    /// `Grep::search_format`: `--format`.
    pub fn format(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::format is not ported")
    }

    /// `Grep::search_only_matching`: `-o` without `-A`, `-B`, `-C`.
    pub fn only_matching(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::only_matching is not ported")
    }

    /// `Grep::search_only_matching_context`: `-o` with `-A`, `-B` or `-C`.
    pub fn only_matching_context(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::only_matching_context is not ported")
    }

    /// `Grep::search_plain`: plain matches and `--hex`.
    pub fn plain(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::plain is not ported")
    }

    /// `Grep::search_invert`: `-v`.
    pub fn invert(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::invert is not ported")
    }

    /// `Grep::search_any_line`: `-y`.
    pub fn any_line(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::any_line is not ported")
    }

    /// `Grep::search_context`: `-A`, `-B`, `-C`.
    pub fn context(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::context is not ported")
    }

    /// `Grep::search_context_invert`: `-A`, `-B`, `-C` with `-v`.
    pub fn context_invert(_ctx: &SearchContext, _matcher: &MatcherHandle) -> SearchStep {
        panic!("Grep seam: branch::context_invert is not ported")
    }

    /// `Grep::search_epilogue`: `--files` check, `--break`, `--stats`; sets `ctx.matched`.
    pub fn epilogue(_ctx: &SearchContext, _matcher: &MatcherHandle) {
        panic!("Grep seam: branch::epilogue is not ported")
    }
}
