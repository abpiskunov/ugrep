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
use std::ffi::c_void;
use std::os::raw::c_int;

// the raw-pointer reads, in `ffi/` where `unsafe` is allowed
#[path = "ffi/matcher_raw.rs"]
pub mod raw;

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
#[repr(C)]
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

/// `reflex::AbstractMatcher::Context`: a run of the matcher buffer, `buf_[offset..offset + len]`, with the number of
/// lines in it. What `before()` and `after()` return and what the handler takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct BufContext {
    pub offset: usize,
    pub len: usize,
    pub num: usize,
}

/// Which buffer-shift handler class C++ registers on the matcher (`set_handler`): the numbers are the C++ values of
/// the facade, pinned by a test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandlerKind(pub u8);

impl HandlerKind {
    /// No handler.
    pub const NONE: HandlerKind = HandlerKind(0);
    /// `GrepHandler`.
    pub const GREP: HandlerKind = HandlerKind(1);
    /// `OnlyMatchingContextGrepHandler`.
    pub const ONLY_MATCHING_CONTEXT: HandlerKind = HandlerKind(2);
    /// `InvertMatchGrepHandler`.
    pub const INVERT_MATCH: HandlerKind = HandlerKind(3);
    /// `FormatInvertMatchGrepHandler`.
    pub const FORMAT_INVERT_MATCH: HandlerKind = HandlerKind(4);
    /// `AnyLineGrepHandler`.
    pub const ANY_LINE: HandlerKind = HandlerKind(5);
    /// `ContextGrepHandler`.
    pub const CONTEXT: HandlerKind = HandlerKind(6);
    /// `InvertContextGrepHandler`.
    pub const INVERT_CONTEXT: HandlerKind = HandlerKind(7);
    /// A handler written in Rust (`set_rust_handler`); C++ registers a trampoline into `shift_event`.
    pub const RUST: HandlerKind = HandlerKind(255);
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

/// The callback that the registered Rust handler receives its events through: `user` is the handler slot of the handle,
/// `buf` and `len` the text about to leave the buffer, `num` its number of lines. Mirrors `ugrep_core_shift_fn`.
pub use raw::ShiftFn;

/// The matcher callback table: how Rust calls the C++ matcher. Mirrors `struct ugrep_core_matcher_table` of
/// `src/search_seam.h` field for field (every field is one word; the layout is pinned by tests there and here). The C++
/// facade fills one per matcher: `ctx` is its handle of the matcher and the first argument of every function. A table is
/// plain data and is copied into the [`MatcherHandle`]; `ctx` must stay valid for the lifetime of the handle (C++ owns
/// the matcher and the facade, the handle only borrows).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MatcherTable {
    pub ctx: *mut c_void,
    /// `MatcherKind` value of the matcher.
    pub kind: usize,
    pub find: extern "C" fn(*mut c_void) -> c_int,
    pub next_match: extern "C" fn(*mut c_void, c_int) -> c_int,
    pub bol: extern "C" fn(*mut c_void) -> usize,
    pub eol: extern "C" fn(*mut c_void, c_int) -> usize,
    pub begin: extern "C" fn(*mut c_void) -> usize,
    pub end: extern "C" fn(*mut c_void) -> usize,
    pub text_bounds: extern "C" fn(*mut c_void, *mut TextBounds),
    pub first: extern "C" fn(*mut c_void) -> usize,
    pub last: extern "C" fn(*mut c_void) -> usize,
    pub size: extern "C" fn(*mut c_void) -> usize,
    pub lineno: extern "C" fn(*mut c_void) -> usize,
    pub lines: extern "C" fn(*mut c_void) -> usize,
    pub columno: extern "C" fn(*mut c_void) -> usize,
    pub at_bol: extern "C" fn(*mut c_void) -> c_int,
    pub hit_end: extern "C" fn(*mut c_void) -> c_int,
    pub skip: extern "C" fn(*mut c_void, c_int) -> c_int,
    pub border: extern "C" fn(*mut c_void) -> usize,
    pub before: extern "C" fn(*mut c_void, *mut BufContext),
    pub after: extern "C" fn(*mut c_void, *mut BufContext),
    pub bef: extern "C" fn(*mut c_void, usize) -> usize,
    pub aft: extern "C" fn(*mut c_void, usize) -> usize,
    /// `buf_ + offset`: `len` readable bytes, valid until the next call into the matcher.
    pub bytes: extern "C" fn(*mut c_void, usize, usize) -> *const u8,
    pub set_reserve: extern "C" fn(*mut c_void, usize),
    pub fuzzy_distance: extern "C" fn(*mut c_void, u16),
    pub fuzzy_edits: extern "C" fn(*mut c_void) -> u16,
    pub lineno_skip: extern "C" fn(*mut c_void, c_int),
    /// Construct the C++ handler class `HandlerKind` for the current input and register it; the previous handler is
    /// destroyed first. `HandlerKind::NONE` only unregisters.
    pub set_shift_handler: extern "C" fn(*mut c_void, c_int),
    /// Register a trampoline handler that calls `ShiftFn(user, ..)` for every shift event (`HandlerKind::RUST`).
    pub set_rust_handler: extern "C" fn(*mut c_void, *mut c_void, ShiftFn),
    /// `handler(*matcher, context.buf, context.len, context.num)` on the registered handler.
    pub invoke_handler: extern "C" fn(*mut c_void, *const BufContext),
    pub reset: extern "C" fn(*mut c_void),
}

/// Where the Rust shift handler of a handle lives. Boxed so that its address, which C++ holds as `user`, is stable.
type HandlerSlot = RefCell<Option<Box<dyn ShiftHandler>>>;

fn dispatch_shift(slot: &HandlerSlot, event: ShiftEvent, buf: &[u8]) {
    // a handler that is being replaced while it runs is skipped rather than aliased
    if let Ok(slot) = slot.try_borrow() {
        if let Some(handler) = slot.as_ref() {
            handler.on_shift(event, buf);
        }
    }
}

/// Handle of a matcher of any kind that stays owned by C++ (`Grep::matcher`, one element of `Grep::matchers`). The handle
/// borrows it through its [`MatcherTable`]: it never frees the matcher, and it must not outlive the `Grep::search` call it
/// was made for. Every method is one table entry; handler lifetime and `reset` are specified in SEAM.md ("Grep seam",
/// "Matcher callback table").
pub struct MatcherHandle {
    table: MatcherTable,
    handler: Box<HandlerSlot>,
    /// Set while `with_bytes` lends the buffer out: any other call on the handle would shift it.
    lent: Cell<bool>,
}

impl std::fmt::Debug for MatcherHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatcherHandle").field("kind", &self.kind()).finish_non_exhaustive()
    }
}

impl Drop for MatcherHandle {
    fn drop(&mut self) {
        // the matcher must not keep a pointer to the slot that is about to be freed
        if self.handler.get_mut().take().is_some() {
            (self.table.set_shift_handler)(self.table.ctx, HandlerKind::NONE.0 as c_int);
        }
    }
}

impl MatcherHandle {
    fn t(&self) -> &MatcherTable {
        assert!(!self.lent.get(), "Grep seam: matcher called while its buffer is lent by with_bytes");
        &self.table
    }

    /// The kind of the matcher.
    pub fn kind(&self) -> MatcherKind {
        MatcherKind(self.table.kind as u8)
    }

    /// `matcher->find()`: search for the next match from the current position; false when there is none. May fire
    /// shift events before it returns.
    pub fn find(&self) -> bool {
        let t = self.t();
        (t.find)(t.ctx) != 0
    }

    /// The match after the current one: `matcher->find()` resumes where the previous match ended, so this is the loop
    /// step of `while (matcher->find())`; with `skip_line` set the rest of the current line is skipped first
    /// (`matcher->skip('\n')`). May fire shift events.
    pub fn next_match(&self, skip_line: bool) -> bool {
        let t = self.t();
        (t.next_match)(t.ctx, skip_line as c_int) != 0
    }

    /// `matcher->bol()`: offset of the start of the line of the match. Call `eol` first, as the C++ does.
    pub fn bol(&self) -> usize {
        let t = self.t();
        (t.bol)(t.ctx)
    }

    /// `matcher->eol(inclusive)`: offset of the end of the line of the match; `inclusive` is the `eol(true)` form that
    /// reads on until the whole line is in the buffer (and may shift it, firing shift events).
    pub fn eol(&self, inclusive: bool) -> usize {
        let t = self.t();
        (t.eol)(t.ctx, inclusive as c_int)
    }

    /// `matcher->begin()`: offset of the first byte of the match.
    pub fn begin(&self) -> usize {
        let t = self.t();
        (t.begin)(t.ctx)
    }

    /// `matcher->end()`: offset one past the match.
    pub fn end(&self) -> usize {
        let t = self.t();
        (t.end)(t.ctx)
    }

    /// All the positions of the current match at once.
    pub fn text_bounds(&self) -> TextBounds {
        let t = self.t();
        let mut bounds = TextBounds::default();
        (t.text_bounds)(t.ctx, &mut bounds);
        bounds
    }

    /// `matcher->first()`: offset of the match in the input.
    pub fn first(&self) -> usize {
        let t = self.t();
        (t.first)(t.ctx)
    }

    /// `matcher->last()`: offset one past the match in the input.
    pub fn last(&self) -> usize {
        let t = self.t();
        (t.last)(t.ctx)
    }

    /// `matcher->size()`.
    pub fn size(&self) -> usize {
        let t = self.t();
        (t.size)(t.ctx)
    }

    /// `matcher->lineno()`.
    pub fn lineno(&self) -> usize {
        let t = self.t();
        (t.lineno)(t.ctx)
    }

    /// `matcher->lines()`: number of lines the match spans.
    pub fn lines(&self) -> usize {
        let t = self.t();
        (t.lines)(t.ctx)
    }

    /// `matcher->columno()`.
    pub fn columno(&self) -> usize {
        let t = self.t();
        (t.columno)(t.ctx)
    }

    /// `matcher->at_bol()`.
    pub fn at_bol(&self) -> bool {
        let t = self.t();
        (t.at_bol)(t.ctx) != 0
    }

    /// `matcher->hit_end()`.
    pub fn hit_end(&self) -> bool {
        let t = self.t();
        (t.hit_end)(t.ctx) != 0
    }

    /// `matcher->skip(c)`: skip input up to and including the byte `c`; false at the end of the input.
    pub fn skip(&self, c: u8) -> bool {
        let t = self.t();
        (t.skip)(t.ctx, c as c_int) != 0
    }

    /// `matcher->border()`: the border offset of the match.
    pub fn border(&self) -> usize {
        let t = self.t();
        (t.border)(t.ctx)
    }

    /// `matcher->before()`: the buffered text before the matching line as `{offset, len, num}`, which goes straight to
    /// the handler (`invoke_handler`).
    pub fn before(&self) -> BufContext {
        let t = self.t();
        let mut context = BufContext::default();
        (t.before)(t.ctx, &mut context);
        context
    }

    /// `matcher->after()`: the buffered text after EOF is reached as `{offset, len, num}`.
    pub fn after(&self) -> BufContext {
        let t = self.t();
        let mut context = BufContext::default();
        (t.after)(t.ctx, &mut context);
        context
    }

    /// `matcher->bef(len)`: the offset of the begin of the match minus `len`, or 0 at the begin of the buffer.
    pub fn bef(&self, len: usize) -> usize {
        let t = self.t();
        (t.bef)(t.ctx, len)
    }

    /// `matcher->aft(len)`: the offset of the end of the match plus `len`, or of the end of the input. It fetches
    /// input, so it invalidates earlier offsets, as the C++ pointers.
    pub fn aft(&self, len: usize) -> usize {
        let t = self.t();
        (t.aft)(t.ctx, len)
    }

    /// Copy of the bytes `buf_[offset..offset + len]` of the matcher buffer. Owned, so a later `find` cannot invalidate
    /// it; the copy is the cost of soundness on the hot path, [`raw::with_bytes`] is the zero-copy (unsafe) form.
    pub fn bytes_to_vec(&self, offset: usize, len: usize) -> Bytes {
        raw::copy_bytes(self.t(), offset, len)
    }

    /// `matcher->set_reserve(size)`: reserve buffer space for a line kept across a buffer shift.
    pub fn set_reserve(&self, size: usize) {
        let t = self.t();
        (t.set_reserve)(t.ctx, size)
    }

    /// `FuzzyMatcher::distance(max)`, the `-Zbest` call: `max` is the cost in the lower byte and the INS, DEL, SUB and
    /// BIN flags in the upper byte. Only for [`MatcherKind::FUZZY`].
    pub fn fuzzy_distance(&self, max: u16) {
        assert_eq!(self.kind(), MatcherKind::FUZZY, "Grep seam: fuzzy_distance on a matcher that is not fuzzy");
        let t = self.t();
        (t.fuzzy_distance)(t.ctx, max)
    }

    /// `FuzzyMatcher::edits()`, the `-Zbest` call of `compute_cost`: edit distance of the current match. Only for
    /// [`MatcherKind::FUZZY`].
    pub fn fuzzy_edits(&self) -> u16 {
        assert_eq!(self.kind(), MatcherKind::FUZZY, "Grep seam: fuzzy_edits on a matcher that is not fuzzy");
        let t = self.t();
        (t.fuzzy_edits)(t.ctx)
    }

    /// `matcher->lineno_skip(on)`: count only matching lines (cheaper) when on.
    pub fn lineno_skip(&self, on: bool) {
        let t = self.t();
        (t.lineno_skip)(t.ctx, on as c_int)
    }

    /// Register the buffer-shift handler of the given kind (`matcher->set_handler`), or clear it with
    /// [`HandlerKind::NONE`]. C++ constructs the handler (`GrepHandler` or a class derived from it, with `*this` and the
    /// pathname) and owns it; the previous one is destroyed. The matcher forgets the handler when it is reset or a new
    /// input source is assigned, so this is called once per file. [`HandlerKind::RUST`] is registered by
    /// [`MatcherHandle::set_rust_handler`], not here.
    pub fn set_shift_handler(&self, kind: HandlerKind) {
        assert_ne!(kind, HandlerKind::RUST, "Grep seam: register a Rust handler with set_rust_handler");
        let t = self.t();
        (t.set_shift_handler)(t.ctx, kind.0 as c_int);
        // C++ has replaced the trampoline, so the Rust handler can go
        self.handler.borrow_mut().take();
    }

    /// Install the Rust [`ShiftHandler`] and register the C trampoline that forwards the matcher's shift events to it,
    /// replacing any handler. The handler lives in the handle: it is dropped by `reset`, by the next
    /// `set_shift_handler` or `set_rust_handler`, and when the handle is dropped (which first unregisters the trampoline).
    pub fn set_rust_handler(&self, handler: Box<dyn ShiftHandler>) {
        let t = self.t();
        *self.handler.borrow_mut() = Some(handler);
        (t.set_rust_handler)(t.ctx, raw::slot_user(self), raw::rust_shift_trampoline);
    }

    /// The buffer-shift event, as the C++ trampoline delivers it to the Rust handler. Runs inside `find`, `next_match` or
    /// `eol`; `buf` is the text about to leave the buffer. Does nothing when no Rust handler is registered.
    pub fn shift_event(&self, event: ShiftEvent, buf: &[u8]) {
        dispatch_shift(&self.handler, event, buf);
    }

    /// Call the registered handler explicitly, as the branches do with `handler(*matcher, context.buf, context.len,
    /// context.num)` after `before()` or `after()`.
    pub fn invoke_handler(&self, context: BufContext) {
        let t = self.t();
        (t.invoke_handler)(t.ctx, &context);
    }

    /// `matcher->reset()`: back to the start of the input, drops the match and the registered handler (C++ destroys
    /// its handler object, and the Rust handler is dropped here). Register a handler again afterwards.
    pub fn reset(&self) {
        let t = self.t();
        (t.reset)(t.ctx);
        self.handler.borrow_mut().take();
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

/// The hex dump modes of `Output::Dump` (`output.hpp`), the `mode` of `out_dump_hex`.
pub const HEX_MATCH: i16 = 0;
pub const HEX_LINE: i16 = 1;
pub const HEX_CONTEXT_MATCH: i16 = 2;
pub const HEX_CONTEXT_LINE: i16 = 3;

/// Where `Grep::restline_data` points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestlineData {
    /// NULL.
    None,
    /// An offset into the matcher buffer (`restline_data = end`).
    Buffer(usize),
    /// The saved copy `Grep::restline` (`restline_data = restline.c_str()`), which survives a buffer shift.
    Saved,
}

/// Snapshot of the `flag_*` globals the search branches read (`flag_before_context` and the like), taken from
/// `ugrep_settings` (Rust-owned since S1, `ffi/settings.rs`) when the host is attached. Strings are bytes, `None` is
/// NULL. The `color_*` and `match_*` strings the branches also read stay in `ugrep_settings` and are read from there.
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
    /// `Grep::restline_data`: NULL, a position in the matcher buffer or the saved `restline` string.
    fn restline_data(&self) -> RestlineData;
    /// Set `restline_data` (`= NULL`, `= end`, `= restline.c_str()`).
    fn set_restline_data(&self, data: RestlineData);
    /// `restline_data += n`, in the buffer or in the saved string.
    fn restline_advance(&self, n: usize);
    /// `restline_data[..restline_size]` copied out, empty when NULL.
    fn restline_bytes(&self) -> Bytes;
    fn restline_output(&self, invert: bool, hex_context: bool);
    fn restline_output_or_save(&self, invert: bool, hex_context: bool);
    fn restline_save(&self);

    // the methods of the registered handler object (`ContextGrepHandler`, `InvertContextGrepHandler`) that the branches
    // call; they act on the handler `MatcherHandle::set_shift_handler` registered last. A handler class ported to Rust
    // has these as its own methods and does not use them.
    /// `handler.output_before_context()`.
    fn handler_output_before_context(&self);
    /// `handler.set_after_lineno(lineno)`.
    fn handler_set_after_lineno(&self, lineno: usize);
    /// `handler.add_before_context_line(bol, eol, columno, offset)`, `bol` and `eol` are matcher buffer offsets.
    fn handler_add_before_context_line(&self, bol: usize, eol: usize, columno: usize, offset: usize);
    /// `handler.add_before_context_match(pos, size, offset)`.
    fn handler_add_before_context_match(&self, pos: usize, size: usize, offset: usize);

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
    /// `out.format(format, pathname, partname, matches, matching, matcher, heading, body, next)`; `matching` is the
    /// in/out counter (`&format_matching`, `&replace_matching`) that the branch owns and passes on every call, `None`
    /// for the NULL of `--format-open` and `--format-close`.
    fn out_format(&self, format: &[u8], matches: usize, matching: Option<&mut usize>, body: bool, next: bool) -> bool;
    fn out_dump_hex(&self, mode: i16, byte_offset: usize, data: &[u8]);
    fn out_dump_next(&self, byte_offset: usize);
    fn out_dump_done(&self);

    // `Stats`
    fn stats_found_file(&self) -> bool;
    fn stats_found_part(&self) -> bool;
    fn stats_undo_found_part(&self);
    fn stats_found_parts(&self) -> usize;
    fn stats_score_matches(&self, matches: usize, lines: usize);
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
    flags: RefCell<GrepFlags>,
}

impl SearchContext {
    /// A context for `search(pathname, cost)`; `matched` starts false and no host is attached yet.
    pub fn new(pathname: &[u8], cost: u16) -> SearchContext {
        SearchContext {
            pathname: pathname.to_vec(),
            cost,
            matched: Cell::new(false),
            host: RefCell::new(None),
            flags: RefCell::new(GrepFlags::default()),
        }
    }

    /// Attach the callback interface to the C++ `Grep`; the shell does it once before the first branch.
    pub fn set_host(&self, host: Box<dyn GrepHost>) {
        *self.host.borrow_mut() = Some(host);
    }

    /// Set the flags snapshot (read from `ugrep_settings` by `ffi/search_host.rs` when the host is attached).
    pub fn set_flags(&self, flags: GrepFlags) {
        *self.flags.borrow_mut() = flags;
    }

    /// The flags the branches read.
    pub fn flags(&self) -> std::cell::Ref<'_, GrepFlags> {
        self.flags.borrow()
    }

    /// Run `f` with the host. Panics when none was attached: the shell always attaches one.
    pub fn with_host<R>(&self, f: impl FnOnce(&dyn GrepHost) -> R) -> R {
        match self.host.borrow().as_deref() {
            Some(host) => f(host),
            None => panic!("Grep seam: no GrepHost attached to the SearchContext"),
        }
    }
}

/// One stub per search branch, named as the C++ `Grep::search_*` functions. The 12 stubs are ported by S4c-3 (`setup`,
/// `quiet_list`, `count`, `format`), S4c-4 (`only_matching`, `only_matching_context`), S4c-5 (`plain`, `invert`,
/// `any_line`) and S4c-6 (`context`, `context_invert`, `epilogue`); see SEAM.md. They
/// take the context and the matcher handle; what they need of the C++ `Grep` is reached through [`GrepHost`] of `ctx`.
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

#[cfg(test)]
mod tests {
    use super::*;

    // the numbers the C++ facade (src/search_seam.h) uses for the kinds, steps and handler classes
    #[test]
    fn numbers_match_the_header() {
        assert_eq!(
            [MatcherKind::REFLEX.0, MatcherKind::PCRE2.0, MatcherKind::LINE.0, MatcherKind::FUZZY.0, MatcherKind::BOOST.0],
            [1, 2, 3, 4, 5]
        );
        assert_eq!([SearchStep::CONTINUE.0, SearchStep::DONE.0, SearchStep::EXIT.0], [0, 1, 2]);
        assert_eq!(
            [
                HandlerKind::NONE.0,
                HandlerKind::GREP.0,
                HandlerKind::ONLY_MATCHING_CONTEXT.0,
                HandlerKind::INVERT_MATCH.0,
                HandlerKind::FORMAT_INVERT_MATCH.0,
                HandlerKind::ANY_LINE.0,
                HandlerKind::CONTEXT.0,
                HandlerKind::INVERT_CONTEXT.0,
                HandlerKind::RUST.0,
            ],
            [0, 1, 2, 3, 4, 5, 6, 7, 255]
        );
        assert_eq!([HEX_MATCH, HEX_LINE, HEX_CONTEXT_MATCH, HEX_CONTEXT_LINE], [0, 1, 2, 3]);
    }

    #[test]
    fn struct_layouts_match_the_header() {
        use std::mem::{offset_of, size_of};
        let w = size_of::<usize>();
        assert_eq!(size_of::<TextBounds>(), 9 * w);
        assert_eq!(
            [
                offset_of!(TextBounds, begin),
                offset_of!(TextBounds, end),
                offset_of!(TextBounds, first),
                offset_of!(TextBounds, last),
                offset_of!(TextBounds, size),
                offset_of!(TextBounds, bol),
                offset_of!(TextBounds, eol),
                offset_of!(TextBounds, lineno),
                offset_of!(TextBounds, lines),
            ],
            [0, w, 2 * w, 3 * w, 4 * w, 5 * w, 6 * w, 7 * w, 8 * w]
        );
        assert_eq!(size_of::<BufContext>(), 3 * w);
        assert_eq!([offset_of!(BufContext, offset), offset_of!(BufContext, len), offset_of!(BufContext, num)], [0, w, 2 * w]);
    }

    #[test]
    fn matcher_table_layout_matches_the_header() {
        use std::mem::{offset_of, size_of};
        let w = size_of::<usize>();
        let offsets = [
            offset_of!(MatcherTable, ctx),
            offset_of!(MatcherTable, kind),
            offset_of!(MatcherTable, find),
            offset_of!(MatcherTable, next_match),
            offset_of!(MatcherTable, bol),
            offset_of!(MatcherTable, eol),
            offset_of!(MatcherTable, begin),
            offset_of!(MatcherTable, end),
            offset_of!(MatcherTable, text_bounds),
            offset_of!(MatcherTable, first),
            offset_of!(MatcherTable, last),
            offset_of!(MatcherTable, size),
            offset_of!(MatcherTable, lineno),
            offset_of!(MatcherTable, lines),
            offset_of!(MatcherTable, columno),
            offset_of!(MatcherTable, at_bol),
            offset_of!(MatcherTable, hit_end),
            offset_of!(MatcherTable, skip),
            offset_of!(MatcherTable, border),
            offset_of!(MatcherTable, before),
            offset_of!(MatcherTable, after),
            offset_of!(MatcherTable, bef),
            offset_of!(MatcherTable, aft),
            offset_of!(MatcherTable, bytes),
            offset_of!(MatcherTable, set_reserve),
            offset_of!(MatcherTable, fuzzy_distance),
            offset_of!(MatcherTable, fuzzy_edits),
            offset_of!(MatcherTable, lineno_skip),
            offset_of!(MatcherTable, set_shift_handler),
            offset_of!(MatcherTable, set_rust_handler),
            offset_of!(MatcherTable, invoke_handler),
            offset_of!(MatcherTable, reset),
        ];
        for (i, o) in offsets.iter().enumerate() {
            assert_eq!(*o, i * w, "field {i}");
        }
        assert_eq!(size_of::<MatcherTable>(), 32 * w);
    }

    // a fake C++ matcher over a byte buffer, to run the handle through the table
    mod fake {
        use super::super::*;
        use std::cell::RefCell;

        pub struct Fake {
            pub buf: Vec<u8>,
            pub pos: Cell<usize>,
            pub calls: RefCell<Vec<&'static str>>,
            pub user: Cell<*mut c_void>,
            pub shift: Cell<Option<ShiftFn>>,
        }

        fn f<'a>(ctx: *mut c_void) -> &'a Fake {
            raw::fake::deref_ctx(ctx)
        }

        extern "C" fn find(ctx: *mut c_void) -> c_int {
            let m = f(ctx);
            m.calls.borrow_mut().push("find");
            if let Some(shift) = m.shift.get() {
                raw::fake::call_shift(shift, m.user.get(), m.buf.len(), 2, m.buf.as_ptr());
            }
            m.pos.set(m.pos.get() + 1);
            (m.pos.get() < 3) as c_int
        }
        extern "C" fn next_match(_: *mut c_void, _: c_int) -> c_int {
            0
        }
        extern "C" fn one(_: *mut c_void) -> usize {
            1
        }
        extern "C" fn eol(_: *mut c_void, _: c_int) -> usize {
            2
        }
        extern "C" fn bounds(_: *mut c_void, out: *mut TextBounds) {
            raw::fake::write_value(out, TextBounds { begin: 7, ..TextBounds::default() });
        }
        extern "C" fn flag(_: *mut c_void) -> c_int {
            1
        }
        extern "C" fn skip(_: *mut c_void, c: c_int) -> c_int {
            (c == 10) as c_int
        }
        extern "C" fn ctx_out(_: *mut c_void, out: *mut BufContext) {
            raw::fake::write_value(out, BufContext { offset: 1, len: 2, num: 3 });
        }
        extern "C" fn plus(_: *mut c_void, n: usize) -> usize {
            n + 10
        }
        extern "C" fn bytes(ctx: *mut c_void, offset: usize, _len: usize) -> *const u8 {
            f(ctx).buf[offset..].as_ptr()
        }
        extern "C" fn set_usize(_: *mut c_void, _: usize) {}
        extern "C" fn fuzzy_distance(_: *mut c_void, _: u16) {}
        extern "C" fn fuzzy_edits(_: *mut c_void) -> u16 {
            3
        }
        extern "C" fn lineno_skip(_: *mut c_void, _: c_int) {}
        extern "C" fn set_shift_handler(ctx: *mut c_void, kind: c_int) {
            let m = f(ctx);
            m.calls.borrow_mut().push(if kind == 0 { "unset" } else { "set" });
            m.shift.set(None);
        }
        extern "C" fn set_rust_handler(ctx: *mut c_void, user: *mut c_void, shift: ShiftFn) {
            let m = f(ctx);
            m.user.set(user);
            m.shift.set(Some(shift));
        }
        extern "C" fn invoke_handler(ctx: *mut c_void, context: *const BufContext) {
            let m = f(ctx);
            let c = raw::fake::read_value(context);
            if let Some(shift) = m.shift.get() {
                raw::fake::call_shift(shift, m.user.get(), c.len, c.num, m.buf.as_ptr());
            }
        }
        extern "C" fn reset(ctx: *mut c_void) {
            let m = f(ctx);
            m.pos.set(0);
            m.shift.set(None);
            m.calls.borrow_mut().push("reset");
        }

        pub fn table(m: &Fake, kind: MatcherKind) -> MatcherTable {
            MatcherTable {
                ctx: m as *const Fake as *mut c_void,
                kind: kind.0 as usize,
                find,
                next_match,
                bol: one,
                eol,
                begin: one,
                end: one,
                text_bounds: bounds,
                first: one,
                last: one,
                size: one,
                lineno: one,
                lines: one,
                columno: one,
                at_bol: flag,
                hit_end: flag,
                skip,
                border: one,
                before: ctx_out,
                after: ctx_out,
                bef: plus,
                aft: plus,
                bytes,
                set_reserve: set_usize,
                fuzzy_distance,
                fuzzy_edits,
                lineno_skip,
                set_shift_handler,
                set_rust_handler,
                invoke_handler,
                reset,
            }
        }
    }

    fn fake() -> Box<fake::Fake> {
        Box::new(fake::Fake {
            buf: b"abc\n".to_vec(),
            pos: Cell::new(0),
            calls: RefCell::new(Vec::new()),
            user: Cell::new(std::ptr::null_mut()),
            shift: Cell::new(None),
        })
    }

    #[test]
    fn handle_calls_through_the_table() {
        let m = fake();
        let h = raw::fake::handle(fake::table(&m, MatcherKind::FUZZY));
        assert_eq!(h.kind(), MatcherKind::FUZZY);
        assert!(h.find());
        assert!(h.find());
        assert!(!h.find());
        assert!(!h.next_match(true));
        assert_eq!((h.bol(), h.eol(true), h.begin(), h.end()), (1, 2, 1, 1));
        assert_eq!(h.text_bounds().begin, 7);
        assert!(h.at_bol() && h.hit_end() && h.skip(b'\n') && !h.skip(b'x'));
        assert_eq!(h.before(), BufContext { offset: 1, len: 2, num: 3 });
        assert_eq!((h.bef(1), h.aft(2)), (11, 12));
        assert_eq!(h.fuzzy_edits(), 3);
        assert_eq!(h.bytes_to_vec(1, 2), b"bc");
        assert_eq!(raw::fake::with_bytes(&h, 0, 3, |b| b.len()), 3);
    }

    #[test]
    #[should_panic(expected = "lent by with_bytes")]
    fn matcher_is_not_callable_while_bytes_are_lent() {
        let m = fake();
        let h = raw::fake::handle(fake::table(&m, MatcherKind::REFLEX));
        raw::fake::with_bytes(&h, 0, 3, |_| h.find());
    }

    type Seen = std::rc::Rc<RefCell<Vec<(usize, usize, Vec<u8>)>>>;

    struct Recorder(Seen);

    impl ShiftHandler for Recorder {
        fn on_shift(&self, event: ShiftEvent, buf: &[u8]) {
            self.0.borrow_mut().push((event.len, event.num, buf.to_vec()));
        }
    }

    #[test]
    fn rust_handler_gets_events_until_reset() {
        let m = fake();
        let h = raw::fake::handle(fake::table(&m, MatcherKind::REFLEX));
        let seen = std::rc::Rc::new(RefCell::new(Vec::new()));
        h.set_rust_handler(Box::new(Recorder(seen.clone())));
        h.find();
        h.invoke_handler(BufContext { offset: 0, len: 2, num: 1 });
        assert_eq!(*seen.borrow(), vec![(4, 2, b"abc\n".to_vec()), (2, 1, b"abc\n"[..2].to_vec())]);
        h.reset();
        h.find();
        assert_eq!(seen.borrow().len(), 2, "reset unregisters the handler");
        h.set_rust_handler(Box::new(Recorder(seen.clone())));
        h.set_shift_handler(HandlerKind::GREP);
        h.find();
        assert_eq!(seen.borrow().len(), 2, "a C++ handler replaces the Rust one");
    }

    #[test]
    fn dropping_the_handle_unregisters_the_rust_handler() {
        let m = fake();
        {
            let h = raw::fake::handle(fake::table(&m, MatcherKind::LINE));
            h.set_rust_handler(Box::new(Recorder(Default::default())));
        }
        assert!(m.shift.get().is_none());
        assert_eq!(*m.calls.borrow(), vec!["unset"]);
    }
}
