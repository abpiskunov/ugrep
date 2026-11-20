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
use std::rc::Rc;

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
    /// `matcher->input(*(const reflex::Input*)host_input)`, the pointer of [`GrepHost::input`].
    pub input: extern "C" fn(*mut c_void, *const c_void),
    /// `matcher->buffer(base, size)`: read from memory; `size` is passed as is (the C++ passes `size + 1` for mmap).
    pub buffer: extern "C" fn(*mut c_void, *const u8, usize),
    /// `matcher->buffer()`: buffer all input; 0 when a read error occurred.
    pub buffer_all: extern "C" fn(*mut c_void) -> c_int,
    /// `matcher->avail(len)`: bytes available from `begin()` on, reading more input to make `len` available.
    pub avail: extern "C" fn(*mut c_void, usize) -> usize,
    /// `matcher->begin()` as a pointer to the matched text (`txt_`), NULL when `len` bytes from it are not inside the
    /// buffer data; valid until the next call into the matcher.
    pub begin_ptr: extern "C" fn(*mut c_void, usize) -> *const u8,
    /// `matcher->in.set_handler(&handler)`: an `INPUT_HANDLER_*` value, `INPUT_HANDLER_NONE` sets NULL.
    pub set_input_handler: extern "C" fn(*mut c_void, c_int),
    /// `matcher->buffer(src_bol, src_eol - src_bol + 1).find()` where `src_*` address the buffer of the matcher with
    /// context `src`: this matcher buffers one line of another matcher and searches it in one call (`cnf_matching`), so
    /// it never keeps a pointer into the buffer of `src` after the call. Returns 1 for a match, 0 for none and, with an
    /// error of the channel, for offsets that are not `bol <= eol <= end` of the data of `src`.
    pub find_in_line: extern "C" fn(*mut c_void, *mut c_void, usize, usize) -> c_int,
}

/// The `reflex::Input::Handler` that `matcher->in.set_handler` registers: the numbers are the C++ values of the facade.
pub const INPUT_HANDLER_NONE: i32 = 0;
/// `Grep::bin_handler`: NUL bytes set `binfile`.
pub const INPUT_HANDLER_BINARY: i32 = 1;
/// `Grep::stdin_handler`: slow TTY or pipe input (not on Windows).
pub const INPUT_HANDLER_STDIN: i32 = 2;

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
        if len == 0 {
            return Bytes::new();
        }
        raw::copy_bytes(self, offset, len)
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

    /// `matcher->input(input)`: assign the host-owned `Grep::input` to the matcher and restart it.
    pub fn input(&self, input: &InputHandle) {
        let t = self.t();
        (t.input)(t.ctx, raw::input_ptr(input));
    }

    /// `matcher->buffer(base, size)`: make the matcher read from memory the caller keeps alive (an mmap). `size` is
    /// passed as is.
    ///
    /// The memory is a [`BufferPtr`], which only `ffi/` makes (unsafe) and which carries the size it vouches for.
    pub fn buffer(&self, memory: &BufferPtr) {
        let t = self.t();
        let (base, size) = raw::buffer_parts(memory);
        (t.buffer)(t.ctx, base, size);
    }

    /// `matcher->buffer(bol, eol - bol + 1).find() != 0` with `bol` and `eol` offsets into the buffer of `src` (another
    /// matcher): search that line of `src`, as `cnf_matching` does, in one call so that this matcher is not left
    /// pointing at memory `src` may shift. False for no match, and for offsets that are not `bol <= eol <=` the end of
    /// the data of `src` (nothing done; see [`GrepHost::error_pending`]).
    pub fn find_in_line(&self, src: &MatcherHandle, bol: usize, eol: usize) -> bool {
        let src_ctx = src.t().ctx;
        let t = self.t();
        (t.find_in_line)(t.ctx, src_ctx, bol, eol) != 0
    }

    /// `matcher->buffer()`: buffer all input; false when a read error occurred.
    pub fn buffer_all(&self) -> bool {
        let t = self.t();
        (t.buffer_all)(t.ctx) != 0
    }

    /// `matcher->avail(len)`: the number of bytes available in the buffer from `begin()` on, after reading more input to
    /// make `len` available when possible.
    pub fn avail(&self, len: usize) -> usize {
        let t = self.t();
        (t.avail)(t.ctx, len)
    }

    /// `len` bytes from `matcher->begin()` (`txt_`), copied out so a later call cannot invalidate them. `None` when the
    /// matcher has no buffer or `len` bytes from `begin()` are not inside the buffer data (the facade checks).
    pub fn begin_bytes(&self, len: usize) -> Option<Bytes> {
        raw::begin_bytes(self, len)
    }

    /// `matcher->in.set_handler(&handler)`: `INPUT_HANDLER_BINARY`, `INPUT_HANDLER_STDIN` or `INPUT_HANDLER_NONE`. The
    /// handler objects are `Grep::bin_handler` and `Grep::stdin_handler`, owned by C++.
    pub fn set_input_handler(&self, kind: i32) {
        let t = self.t();
        (t.set_input_handler)(t.ctx, kind as c_int);
    }
}

pub use raw::{BufferPtr, FileRef, GrepState, InputHandle, OwnedFile};

/// The stream that `Grep::file_in` holds, or that is handed to the filter or the decompression thread: a file that
/// the receiver closes, or the process `stdin` or `Static::source` (the TUI pipe of `-Q`), which are never closed.
#[derive(Debug, PartialEq, Eq)]
pub enum FileSource {
    Owned(OwnedFile),
    Stdin,
    Source,
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

/// Result of `cnf_matching` and `cnf_satisfied`. The C++ members return a bool and throw `Grep::EXIT_SEARCH`; an exception
/// must not unwind through the seam, so the facade catches it and returns `Exit`, and the caller of the seam rethrows
/// it (C++) or leaves the search (Rust, as `SearchStep::EXIT`). The numbers are the `UGREP_CORE_CNF_*` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CnfResult(pub u8);

impl CnfResult {
    /// `false`: the line does not satisfy the CNF.
    pub const NO: CnfResult = CnfResult(0);
    /// `true`.
    pub const YES: CnfResult = CnfResult(1);
    /// `Grep::EXIT_SEARCH` was thrown: stop searching this file (equals `SearchStep::EXIT`).
    pub const EXIT: CnfResult = CnfResult(2);
}

/// The bitmask of `Grep` that `cnf_bit_get` and `cnf_bit_set` address.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CnfBits {
    /// `matching[k]`.
    Matching,
    /// `notmatching[k][l]`.
    NotMatching,
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
    /// The saved copy `Grep::restline` (`restline_data = restline.c_str() + offset`), which survives a buffer shift;
    /// the offset is nonzero after `restline_data += n` on a saved line (the hex path).
    Saved(usize),
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
    pub decompress: bool,
    pub devices_action: i32,
    pub encoding_type: u16,
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
    pub min_line: usize,
    /// `flag_mode` (the C++ `Mode` enum value).
    pub mode: i32,
    pub multiline: bool,
    pub no_header: bool,
    pub no_messages: bool,
    pub null: bool,
    pub only_matching: bool,
    pub quiet: bool,
    pub replace: Option<Bytes>,
    pub separator: Option<Bytes>,
    pub separator_bar: Option<Bytes>,
    pub separator_dash: Option<Bytes>,
    pub stats: Option<Bytes>,
    pub tag: Option<Bytes>,
    pub text: bool,
    pub ungroup: bool,
    pub with_filename: bool,
    pub with_hex: bool,
    pub zmax: usize,
    /// `flag_max_mmap` (`--max-mmap`), 0 disables the mmap of `init_read`.
    pub max_mmap: usize,
    /// `color_sl`, `color_cx`, `color_se` and `color_off`, NUL-free copies.
    pub color_se: Bytes,
    pub color_sl: Bytes,
    pub color_cx: Bytes,
    pub color_off: Bytes,
    /// `color_cn`, `match_ms` and `match_off` (S4c-4, additive like `color_se`).
    pub color_cn: Bytes,
    pub match_ms: Bytes,
    pub match_off: Bytes,
    /// `match_mc` (S4c-5, additive like `match_ms`): the match color of `-y -v`.
    pub match_mc: Bytes,
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
    /// `restline = bytes; restline_data = restline.c_str()`: store a copy of the rest of the line in the saved string
    /// and point `restline_data` at it. The primitive of `save_restline` for the Rust port: `restline_save` calls the
    /// C++ member that is being replaced and must not be used from it. `restline_size` is not changed.
    fn restline_set(&self, bytes: &[u8]);
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

    // the CNF of `--files` / `-e -e`: `cnf_matching`, `cnf_satisfied`, `matchers`, `matching`, `notmatching`. The C++ throws
    // `Grep::EXIT_SEARCH` in both; the table returns it as `CnfResult::Exit` (SEAM.md, "Grep file members").
    fn cnf_matching(&self, bol: usize, eol: usize, acquire: bool) -> CnfResult;
    fn cnf_satisfied(&self, acquire: bool) -> CnfResult;
    /// `out.launch()`.
    fn out_launch(&self);
    /// `Grep::matchers` is a list of lists: the number of alternatives of AND term `term`.
    fn cnf_alts(&self, term: usize) -> usize;
    /// The matcher at (`term`, `alt`) of `matchers`, `None` for a NULL element (an undefined pattern). The handle is
    /// made once per element and shared: the table is filled once, not on every line of `cnf_matching`.
    fn cnf_alt_matcher(&self, term: usize, alt: usize) -> Option<Rc<MatcherHandle>>;
    /// `matching[k]` (`CnfBits::Matching`, `l` unused) or `notmatching[k][l]`; false for an index outside the masks.
    fn cnf_bit_get(&self, which: CnfBits, k: usize, l: usize) -> bool;
    /// Set the same bit to true; an index outside the masks is an error (see [`GrepHost::error_pending`]).
    fn cnf_bit_set(&self, which: CnfBits, k: usize, l: usize);

    /// A callback of the facade threw (or was asked something out of range): the exception is stored on the C++ side
    /// and the callback returned a neutral value (0, NULL, false). The Rust side must stop, return `EXIT` (a
    /// [`SearchStep`] or [`CnfResult`]) and leave the C++ caller to rethrow it; check after every callback that can
    /// fail (all of them can: `bad_alloc`, `regex_error`). See SEAM.md, "Exception channel".
    fn error_pending(&self) -> bool;

    // the file members: `filename`, `file_in`, `input`, `interactive`, `Static::source`, `zthread`, `warning`, the
    // OS file operations and `flag_filter`
    /// `Grep::filename`, `None` when NULL.
    fn filename(&self) -> Option<Bytes>;
    /// `Grep::interactive`.
    fn interactive(&self) -> bool;
    fn set_interactive(&self, interactive: bool);
    /// `Grep::file_in`, borrowed: `None` when NULL. It stays owned by C++ and is closed only by
    /// [`GrepHost::close_file_in`], which needs `&mut self`, so no borrow is alive when it is closed.
    fn file_in(&self) -> Option<FileRef<'_>>;
    /// `Grep::file_in = file` (NULL for `None`); C++ owns it from then on. It overwrites the previous value without
    /// closing it: close that first ([`GrepHost::close_file_in`]).
    /// `&mut self`: no [`FileRef`] from [`GrepHost::file_in`] or [`GrepHost::input_file`] can be alive across the call.
    fn set_file_in(&mut self, file: Option<FileSource>);
    /// The close of `close_file` for the decompressed pipe: `if (input.file() != NULL) { fclose(input.file()); input.clear(); }`.
    /// The host cancels an active zthread that reads the stream, fcloses it (never `stdin` or `Static::source`), clears
    /// `Grep::input` and detaches every matcher that reads the stream, so none keeps a dangling `FILE*`.
    /// `&mut self`: no [`FileRef`] is alive. Nothing is returned: the stream is never handed to Rust.
    fn close_input_file(&mut self);
    /// The close of `close_file` for the opened file: `if (file_in != NULL && file_in != stdin && file_in != Static::source)
    /// { fclose(file_in); file_in = NULL; } input.clear();`. Same effects and guarantees as [`GrepHost::close_input_file`],
    /// and `Grep::file_in` is NULL afterwards, also when it was `stdin` or `Static::source` (those are not closed).
    fn close_file_in(&mut self);
    /// `&Grep::input`, for [`MatcherHandle::input`].
    fn input(&self) -> InputHandle;
    /// `Grep::input.file()`.
    fn input_file(&self) -> Option<FileRef<'_>>;
    /// `Grep::input.file_encoding()`, which `mmap.file(input, ...)` checks.
    fn input_encoding(&self) -> i32;
    /// `Grep::input = reflex::Input(file, encoding)` (`encoding` is `flag_encoding_type`) for a pipe or stream that the
    /// host takes over: it keeps `file` open for `Grep::input`, so the caller cannot close it or use it afterwards. A
    /// decompression pipe that `Grep::input` held before is closed first, as `close_file` does before the input changes.
    fn input_set_file(&mut self, file: FileSource, encoding: i32);
    /// `Grep::input = reflex::Input(Grep::file_in, encoding)`: the stream stays owned by `Grep::file_in`. A NULL `file_in` is
    /// an error on the exception channel.
    fn input_set_file_in(&mut self, encoding: i32);
    /// `Grep::input.clear()`, closing first the decompression pipe that it held (never `file_in`, stdin or the TUI pipe).
    /// `&mut self`: no [`FileRef`] of `input_file` is alive when that pipe is closed.
    fn input_clear(&mut self);
    /// `Grep::input.get(buf, buf.len())`: the number of bytes read.
    fn input_get(&self, buf: &mut [u8]) -> usize;
    /// `Static::source`, and `stdin`.
    fn source(&self) -> Option<FileRef<'_>>;
    fn stdin_file(&self) -> FileRef<'_>;
    /// `reflex::Input::file_encoding::null_data`, which `flag_encoding_type` is compared with.
    fn encoding_null_data(&self) -> i32;
    /// `flag_filter` (a `std::string`, `--filter`), empty when unset.
    fn flag_filter(&self) -> Bytes;
    /// `flag_filter_magic_label[index]` (`--filter-magic-label`), `None` past the last label.
    fn flag_filter_magic_label(&self, index: usize) -> Option<Bytes>;
    /// `reflex::Matcher(Static::filter_magic_pattern, file).scan()`, then `rewind(file)` as `Grep::filter` does.
    fn filter_magic_scan(&self, file: &FileRef<'_>) -> usize;
    /// `warning(message, arg)`.
    fn warning(&self, message: &[u8], arg: &[u8]);
    /// `fopenw_s(&file, pathname, "rb")`: NULL when it fails, after `warning("cannot read", pathname)` (so that the
    /// warning sees the `errno` of the failed open).
    fn file_open(&self, pathname: &[u8]) -> Option<OwnedFile>;
    /// `fclose(file)`: the handle is consumed, so a closed file cannot be used again.
    fn file_close(&self, file: OwnedFile);
    /// `ferror(file)`; true after `warning("cannot read", pathname)` (so that the warning sees the stream's `errno`).
    fn file_read_error(&self, file: &FileRef<'_>, pathname: &[u8]) -> bool;
    /// `_setmode(fileno(file), _O_BINARY)` on Windows, nothing elsewhere.
    fn file_set_binary(&self, file: &FileRef<'_>);
    /// Linux: make a regular file non-blocking (`fstat` S_ISREG then `fcntl` O_NONBLOCK, to raise EAGAIN for `/proc`
    /// files); nothing elsewhere. The caller checks `flag_devices_action != READ` and `file != stdin, source`.
    fn file_nonblock_regular(&self, file: &FileRef<'_>);
    /// Non-Windows: true when `file` is a character device, pipe or socket that was made non-blocking (`interactive`);
    /// false elsewhere (Windows decides by `input == stdin`).
    fn file_probe_interactive(&self, file: &FileRef<'_>) -> bool;
    /// `fseek(file, 0, SEEK_SET) == 0`.
    fn file_seek_set(&self, file: &FileRef<'_>) -> bool;
    /// `!feof(stdin) && fseek(stdin, 0, SEEK_END) < 0 && errno != EINVAL` with `WITH_STDIN_DRAIN`, false on Windows:
    /// stdin is a non-seekable pipe that `close_file` drains.
    fn stdin_drain_check(&self) -> bool;
    /// The retained C++ part of `--filter`: create the pipe, `fork`/`CreateProcess` the `command`, close `file` unless it
    /// is stdin, and return the reading end (`fdopen`, may be NULL). `None` when the pipe could not be created, after
    /// the warning and after `file` was closed.
    fn filter_spawn(&self, file: FileSource, pathname: &[u8], command: &[u8]) -> Option<OwnedFile>;
    /// `is_binary(bytes, len)` of ugrep.cpp (`flag_encoding_type`, `-a`, `-U`, `-W`, `reflex::isutf8`).
    fn is_binary(&self, bytes: &[u8]) -> bool;
    /// `Grep::zthread.start(ztstage, pathname, Grep::file_in, find)`; on a pipe the host installs it into `Grep::input`
    /// (`reflex::Input(pipe, encoding)`, owned: closed by [`GrepHost::close_input_file`]) and returns true, else false.
    /// The pipe never reaches Rust. The thread reads `Grep::file_in`, which stays owned by the host (closed by
    /// [`GrepHost::close_file_in`]); a NULL `file_in` is an error of the exception channel. `&mut self`: no [`FileRef`]
    /// of `input_file` is alive. A job still running is drained and the held input closed before the new one starts.
    fn zthread_start(&mut self, ztstage: usize, pathname: &[u8], find: Option<&[u8]>, encoding: i32) -> bool;
    /// `Grep::zthread.open_next(pathname)`: as `zthread_start`, true when the next pipe is now `Grep::input`. The host
    /// first closes the previous part's pipe that `Grep::input` holds (as `close_file` does before `open_next`), so the
    /// thread blocked writing to it is released and the wait for the next part cannot hang.
    fn zthread_open_next(&mut self, pathname: &[u8], encoding: i32) -> bool;
    /// `Grep::zthread.cancel()`.
    fn zthread_cancel(&self);

    /// `matchers->size()`, `None` when `matchers` is NULL.
    fn matchers_len(&self) -> Option<usize>;
    /// The matcher handle of element `index` of `matchers` (`matchers->at(index)`), any kind: its first alternative,
    /// so it is the shared handle of `cnf_alt_matcher(index, 0)` (one handle per C++ matcher).
    fn matchers_matcher(&self, index: usize) -> Option<Rc<MatcherHandle>>;
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
    /// `out.header(pathname, partname, heading, lineno, NULL, byte_offset, sep, false)`: with a NULL matcher the column
    /// number of `-k` is 1 (S4c-4, additive: the `-o` multi-line echo passes NULL).
    fn out_header_no_matcher(&self, lineno: usize, byte_offset: usize, sep: &[u8]);
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

    /// Run `f` with exclusive access to the host, for the `&mut self` methods ([`GrepHost::set_file_in`],
    /// [`GrepHost::close_input_file`], [`GrepHost::close_file_in`], [`GrepHost::input_set_file`]): they cannot be called
    /// while a [`FileRef`] of the host is alive. Panics when no host is attached or the host is already borrowed (from
    /// inside `with_host` or a handler that runs under it): the file members never nest.
    pub fn with_host_mut<R>(&self, f: impl FnOnce(&mut dyn GrepHost) -> R) -> R {
        match self.host.try_borrow_mut() {
            Ok(mut guard) => match guard.as_deref_mut() {
                Some(host) => f(host),
                None => panic!("Grep seam: no GrepHost attached to the SearchContext"),
            },
            Err(_) => panic!("Grep seam: the GrepHost is already borrowed"),
        }
    }

    /// A facade callback failed and its exception is pending on the C++ side ([`GrepHost::error_pending`]); false when
    /// no host is attached.
    pub fn error_pending(&self) -> bool {
        self.host.borrow().as_deref().is_some_and(|host| host.error_pending())
    }
}

/// One stub per search branch, named as the C++ `Grep::search_*` functions. The 12 stubs are ported by S4c-3 (`setup`,
/// `quiet_list`, `count`, `format`), S4c-4 (`only_matching`, `only_matching_context`), S4c-5 (`plain`, `invert`,
/// `any_line`) and S4c-6 (`context`, `context_invert`, `epilogue`); see SEAM.md. They
/// take the context and the matcher handle; what they need of the C++ `Grep` is reached through [`GrepHost`] of `ctx`.
pub mod branch {
    use super::{file, CnfResult, GrepHost, HandlerKind, MatcherHandle, RestlineData, SearchContext, SearchStep, HEX_CONTEXT_LINE, HEX_CONTEXT_MATCH, HEX_LINE, HEX_MATCH};

    /// `Grep::matches` and `Grep::lineno` of a branch that does not register a shift handler: the branch counts in locals
    /// (no host call per match) and stores them back on every way out, so the members hold what the C++ left in them.
    struct Counters {
        matches: usize,
        lineno: usize,
    }

    impl Counters {
        fn load(ctx: &SearchContext) -> Counters {
            ctx.with_host(|h| Counters { matches: h.matches(), lineno: h.lineno() })
        }

        fn store(&self, ctx: &SearchContext) {
            ctx.with_host(|h| {
                h.set_matches(self.matches);
                h.set_lineno(self.lineno);
            });
        }
    }

    /// `matchers != NULL`: the CNF of AND/OR/NOT matchers is active.
    fn has_cnf(ctx: &SearchContext) -> bool {
        ctx.with_host(|h| h.matchers_len().is_some())
    }

    /// `cnf_matching(bol, eol, acquire)` of the line of the current match: `matcher->eol(true)` first, then `bol()`.
    /// `Some(step)` is a `throw EXIT_SEARCH`, `None` continues with the verdict in `matched`.
    fn cnf_line(ctx: &SearchContext, matcher: &MatcherHandle, acquire: bool, matched: &mut bool) -> Option<SearchStep> {
        let eol = matcher.eol(true);
        let bol = matcher.bol();
        let result = file::cnf_matching(ctx, matcher, bol, eol, acquire);
        if result == CnfResult::EXIT {
            return Some(SearchStep::EXIT);
        }
        *matched = result == CnfResult::YES;
        None
    }

    /// `--format-open` / `--format-close` of a file: `out.format(format, ..., Stats::found_parts(), NULL, ..., false,
    /// Stats::found_parts() > 1)`.
    fn out_format_part(h: &dyn GrepHost, format: &[u8]) {
        let parts = h.stats_found_parts();
        h.out_format(format, parts, None, false, parts > 1);
    }

    /// `Grep::search_setup`: `--files` resets the matching bitmasks.
    pub fn setup(ctx: &SearchContext) {
        if !ctx.flags().files {
            return;
        }
        ctx.with_host(|h| {
            // matchers != NULL: hold the output and reset the bit of each matcher and of its OR NOT terms
            if h.matchers_len().is_some() {
                h.out_hold();
                h.matchers_reset();
            }
        });
    }

    /// `Grep::search_quiet_list`: `-q`, `-l`, `-L`.
    pub fn quiet_list(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let mut counters = Counters::load(ctx);
        let step = quiet_list_scan(ctx, matcher, &mut counters);
        counters.store(ctx);
        step
    }

    fn quiet_list_scan(ctx: &SearchContext, matcher: &MatcherHandle, c: &mut Counters) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);

        // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
        let acquire = (f.format.is_some() && (f.format_open.is_some() || f.format_close.is_some())) || (!f.quiet && f.max_files > 0);

        // skip accurate line number counting, only count matching lines
        if !f.multiline && f.max_line == 0 && f.stats.is_none() {
            matcher.lineno_skip(true);
        }

        while matcher.find() {
            let current_lineno = matcher.lineno();

            // --range: max line exceeded?
            if f.max_line > 0 && current_lineno > f.max_line {
                break;
            }

            if cnf {
                // check CNF AND/OR/NOT matching
                let mut matched = false;
                if let Some(step) = cnf_line(ctx, matcher, acquire, &mut matched) {
                    return step;
                }
                if !matched || ctx.with_host(|h| h.out_holding()) {
                    continue;
                }
            }

            if f.min_count == 0 || f.ungroup || f.only_matching || c.lineno != current_lineno {
                c.matches += 1;
            }

            // --min-count: require at least min-count matches, otherwise stop searching
            if f.min_count == 0 || c.matches >= f.min_count {
                break;
            }

            c.lineno = current_lineno;
        }

        // --min-count: require at least min-count matches
        if f.min_count > 0 && c.matches < f.min_count {
            c.matches = 0;
        }

        // --files: if we are still holding the output and CNF is finally satisfyable then a match was made
        if f.files && cnf {
            if file::cnf_satisfied(ctx, acquire) != CnfResult::YES {
                return SearchStep::EXIT;
            }
            c.matches = 1;
        }

        // -v: invert
        if f.invert_match {
            c.matches = usize::from(c.matches == 0);
        }

        if c.matches > 0 {
            return ctx.with_host(|h| {
                // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
                if acquire {
                    h.out_acquire();
                }

                // --max-files: max reached?
                if (!f.files || !cnf) && !h.stats_found_part() {
                    return SearchStep::EXIT;
                }

                // -l or -L
                if f.files_with_matches {
                    if let Some(format) = f.format.as_deref() {
                        if let Some(open) = f.format_open.as_deref() {
                            out_format_part(h, open);
                        }
                        h.out_format(format, 1, None, false, false);
                        if let Some(close) = f.format_close.as_deref() {
                            out_format_part(h, close);
                        }
                    } else {
                        h.out_header_file();
                        if f.null {
                            h.out_chr(0);
                        } else {
                            h.out_nl(false);
                        }
                    }
                }
                SearchStep::DONE
            });
        }

        SearchStep::DONE
    }

    /// `Grep::search_count`: `-c`.
    pub fn count(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let mut counters = Counters::load(ctx);
        let step = count_scan(ctx, matcher, &mut counters);
        counters.store(ctx);
        step
    }

    fn count_scan(ctx: &SearchContext, matcher: &MatcherHandle, c: &mut Counters) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);

        // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
        let acquire = (f.format.is_some() && (f.format_open.is_some() || f.format_close.is_some())) || f.max_files > 0;

        // not --match to match all or not -v
        if !f.match_ || !f.invert_match {
            if f.ungroup || f.only_matching {
                // -co or -cu: count the number of patterns matched

                // skip accurate line number counting, only count matching lines
                if !f.multiline && f.max_line == 0 && f.stats.is_none() {
                    matcher.lineno_skip(true);
                }

                while matcher.find() {
                    // --range: max line exceeded?
                    if f.max_line > 0 && matcher.lineno() > f.max_line {
                        break;
                    }

                    if cnf {
                        // check CNF AND/OR/NOT matching, with --files acquire lock before Stats::found_part()
                        let mut matched = false;
                        if let Some(step) = cnf_line(ctx, matcher, acquire, &mut matched) {
                            return step;
                        }
                        if !matched {
                            continue;
                        }
                    }

                    c.matches += 1;

                    // -m: max number of matches reached?
                    if f.max_count > 0 && c.matches >= f.max_count {
                        break;
                    }
                }
            } else if !f.multiline && !f.invert_match && f.max_line == 0 && !cnf && f.stats.is_none() {
                // -c without -o/-u/-v/-K/-%: count the number of matching lines

                // skip accurate line number counting, only count matching lines
                matcher.lineno_skip(true);

                let mut found = matcher.find();
                while found {
                    c.matches += 1;

                    // -m: max number of matches reached?
                    if f.max_count > 0 && c.matches >= f.max_count {
                        break;
                    }

                    // if the match does not span more than one line, then skip to end of the line (we count matching lines)
                    found = matcher.next_match(!matcher.at_bol());
                }
            } else {
                // -c without -o/-u: count the number of matching lines

                while matcher.find() {
                    let current_lineno = matcher.lineno();

                    if c.lineno != current_lineno {
                        // --range: max line exceeded?
                        if f.max_line > 0 && current_lineno > f.max_line {
                            break;
                        }

                        if cnf {
                            // check CNF AND/OR/NOT matching, with --files acquire lock before Stats::found_part()
                            let mut matched = false;
                            if let Some(step) = cnf_line(ctx, matcher, acquire, &mut matched) {
                                return step;
                            }
                            if !matched {
                                continue;
                            }
                        }

                        c.matches += 1;

                        // -m: max number of matches reached?
                        if f.max_count > 0 && c.matches >= f.max_count {
                            break;
                        }

                        c.lineno = current_lineno;

                        // if the match does not span more than one line, then skip to end of the line (we count matching lines)
                        if !f.multiline && !matcher.at_bol() {
                            matcher.skip(b'\n');
                        }
                    }
                }

                // -c with -v: count non-matching lines
                if f.invert_match {
                    c.matches = matcher.lineno().wrapping_sub(c.matches);
                    if c.matches > 0 {
                        c.matches -= 1;
                    }
                }
            }
        }

        // --min-count: require at least min-count matches
        if f.min_count > 0 && c.matches < f.min_count {
            return SearchStep::EXIT;
        }

        // --files: if we are still holding the output and CNF is not satisfyable then no global matches were made
        if f.files && cnf {
            // we cannot report 0 matches and ensure accurate output
            if file::cnf_satisfied(ctx, acquire) != CnfResult::YES {
                return SearchStep::EXIT;
            }
        } else {
            let exit = ctx.with_host(|h| {
                // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
                if acquire {
                    h.out_acquire();
                }

                // --max-files: max reached?
                // unfortunately, allowing 'acquire' below produces "x matching + y in archives"
                // but without this we cannot produce correct format-open and format-close outputs
                (c.matches > 0 || acquire) && !h.stats_found_part()
            });
            if exit {
                return SearchStep::EXIT;
            }
        }

        ctx.with_host(|h| {
            if let Some(format) = f.format.as_deref() {
                if let Some(open) = f.format_open.as_deref() {
                    out_format_part(h, open);
                }
                h.out_format(format, c.matches, None, false, false);
                if let Some(close) = f.format_close.as_deref() {
                    out_format_part(h, close);
                }
            } else {
                if f.with_filename || !h.partname().is_empty() {
                    h.out_header_file();

                    if f.null {
                        h.out_chr(0);
                    } else {
                        h.out_str(&f.color_se);
                        h.out_str(f.separator.as_deref().unwrap_or_default());
                        h.out_str(&f.color_off);
                    }
                }
                h.out_num(c.matches, 0);
                h.out_nl(false);
            }
        });

        SearchStep::DONE
    }

    /// `Grep::search_format`: `--format`.
    pub fn format(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        if ctx.flags().invert_match {
            // the registered handler works on the members, they stay in the host
            return format_invert(ctx, matcher);
        }
        let mut counters = Counters::load(ctx);
        let step = format_scan(ctx, matcher, &mut counters);
        counters.store(ctx);
        step
    }

    /// What follows the loops of `search_format`: `--min-count`, `--files` and `--format-close`.
    fn format_finish(ctx: &SearchContext, matches: usize, cnf: bool) -> SearchStep {
        let f = ctx.flags();

        // --min-count: require at least min-count matches
        if f.min_count > 0 && matches < f.min_count {
            return SearchStep::EXIT;
        }

        // --files: if we are still holding the output and CNF is not satisfyable then no global matches were made
        if f.files && cnf && file::cnf_satisfied(ctx, true) != CnfResult::YES {
            return SearchStep::EXIT;
        }

        // output --format-close=FORMAT
        if matches > 0 {
            if let Some(close) = f.format_close.as_deref() {
                ctx.with_host(|h| out_format_part(h, close));
            }
        }
        SearchStep::DONE
    }

    /// `--format` with `-v`: the `FormatInvertMatchGrepHandler` of the host outputs the non-matching lines, and reads and
    /// writes `matches`, `lineno` and `stop` itself, so they are not cached here.
    fn format_invert(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);

        // register the event handler to output non-matching lines, the matcher resets it when a new input source is assigned
        matcher.set_shift_handler(HandlerKind::FORMAT_INVERT_MATCH);

        while matcher.find() {
            let current_lineno = matcher.lineno();

            if ctx.with_host(|h| h.lineno()) != current_lineno {
                if cnf {
                    // check CNF AND/OR/NOT matching
                    let mut matched = false;
                    if let Some(step) = cnf_line(ctx, matcher, false, &mut matched) {
                        return step;
                    }
                    if !matched {
                        continue;
                    }
                }

                // get the lines before the matched line and output the non-matching lines up to this line
                let context = matcher.before();
                if context.len > 0 {
                    matcher.invoke_handler(context);
                }

                // --range: max line exceeded?
                if f.max_line > 0 && current_lineno > f.max_line {
                    return SearchStep::DONE;
                }

                // --max-files: max reached?
                if ctx.with_host(|h| h.stop()) {
                    return SearchStep::EXIT;
                }

                // -m: max number of matches reached?
                if f.max_count > 0 && ctx.with_host(|h| h.matches()) >= f.max_count {
                    return SearchStep::DONE;
                }

                // output blocked?
                if ctx.with_host(|h| h.out_eof()) {
                    return SearchStep::EXIT;
                }
            }

            let lines = matcher.lines();
            ctx.with_host(|h| h.set_lineno(current_lineno + lines - 1));
        }

        // get the remaining context after the last match to output
        let context = matcher.after();
        if context.len > 0 {
            matcher.invoke_handler(context);
        }

        let matches = ctx.with_host(|h| h.matches());
        format_finish(ctx, matches, cnf)
    }

    fn format_scan(ctx: &SearchContext, matcher: &MatcherHandle, c: &mut Counters) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);
        let format = f.format.as_deref().unwrap_or_default();
        let format_open = f.format_open.as_deref();

        // --format with open/close: we must out.acquire() lock before Stats::found_part()
        let acquire = format_open.is_some() || f.format_close.is_some();

        // counter for format field %m matches
        let mut format_matching = 0usize;

        while matcher.find() {
            let current_lineno = matcher.lineno();

            // --range: max line exceeded?
            if f.max_line > 0 && current_lineno > f.max_line {
                break;
            }

            if cnf {
                // check CNF AND/OR/NOT matching
                let mut matched = false;
                if let Some(step) = cnf_line(ctx, matcher, acquire, &mut matched) {
                    return step;
                }
                if !matched {
                    continue;
                }
            }

            if current_lineno != c.lineno || f.ungroup {
                c.matches += 1;

                // --min-count: require at least min-count matches
                if f.min_count > 0 && c.matches < f.min_count {
                    c.lineno = current_lineno;
                    continue;
                }

                // output --format-open=FORMAT for the first (or --min-count) match found
                if c.matches == f.min_count + usize::from(f.min_count == 0) {
                    let exit = ctx.with_host(|h| {
                        if f.files && cnf {
                            // --format-open: we must acquire lock early before Stats::found_part()
                            if acquire && h.out_holding() {
                                h.out_acquire();

                                // --max-files: max reached?
                                if !h.stats_found_part() {
                                    return true;
                                }
                            }
                        } else {
                            // --format-open: we must acquire lock early before Stats::found_part()
                            if acquire {
                                h.out_acquire();
                            }

                            // --max-files: max reached?
                            if !h.stats_found_part() {
                                return true;
                            }
                        }

                        if let Some(open) = format_open {
                            // output --format-open=FORMAT
                            out_format_part(h, open);

                            // --files: undo files count
                            if f.files && cnf && h.out_holding() {
                                h.stats_undo_found_part();
                            }
                        }
                        false
                    });
                    if exit {
                        return SearchStep::EXIT;
                    }
                }
            }

            // output --format=FORMAT
            // if the match does not span more than one line, then skip to end of the line
            let body = c.matches > 1 || current_lineno == c.lineno;
            let next = c.matches > 1;
            let output = ctx.with_host(|h| h.out_format(format, c.matches, Some(&mut format_matching), body, next));
            if !output && !f.multiline && !matcher.at_bol() && !matcher.skip(b'\n') {
                break;
            }

            // --max-count: max number of matches reached?
            if f.max_count > 0 && c.matches >= f.max_count {
                break;
            }

            ctx.with_host(|h| h.out_check_flush());

            c.lineno = current_lineno;
        }

        format_finish(ctx, c.matches, cnf)
    }

    /// The members the `-o` branches write besides `matches` and `lineno`: `binary` and `hex`. They are kept in locals
    /// and stored on every way out; the restline members stay in the host, the shift handler works on them.
    struct OnlyMatchingState {
        counters: Counters,
        binary: bool,
        hex: bool,
    }

    impl OnlyMatchingState {
        fn load(ctx: &SearchContext) -> OnlyMatchingState {
            let counters = Counters::load(ctx);
            ctx.with_host(|h| OnlyMatchingState { counters, binary: h.binary(), hex: h.hex() })
        }

        fn store(&self, ctx: &SearchContext) {
            self.counters.store(ctx);
            ctx.with_host(|h| {
                h.set_binary(self.binary);
                h.set_hex(self.hex);
            });
        }
    }

    /// `check_binary(s, n)`: `-X`, or `-W` and the match is binary.
    fn check_binary(ctx: &SearchContext, bytes: &[u8]) -> bool {
        let f = ctx.flags();
        if f.hex {
            return true;
        }
        if !f.with_hex {
            return false;
        }
        // the UTF-8 check of C++ reads whole SIMD words past the end, as it does in the matcher buffer: pad the copy
        let mut padded = Vec::with_capacity(bytes.len() + 64);
        padded.extend_from_slice(bytes);
        padded.resize(bytes.len() + 64, 0);
        ctx.with_host(|h| h.is_binary(&padded[..bytes.len()]))
    }

    /// `utf8nlen(s, n)`: the number of UTF-8-encoded characters, without validity checking.
    fn utf8nlen(s: &[u8]) -> usize {
        s.iter().filter(|&&c| c & 0xc0 != 0x80).count()
    }

    /// `utf8skipn(s, n, k)`: the offset after advancing `k` UTF-8-encoded characters of the first `n` bytes of `s`.
    fn utf8skipn(s: &[u8], n: usize, k: usize) -> usize {
        let (mut p, mut n, mut k) = (0usize, n, k);
        while n > 0 && k > 0 {
            n -= 1;
            p += 1;
            k -= usize::from(s.get(p).is_some_and(|&c| c & 0xc0 != 0x80));
        }
        p
    }

    /// `Grep::search_only_matching`: `-o` without `-A`, `-B`, `-C`.
    pub fn only_matching(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let mut state = OnlyMatchingState::load(ctx);
        let step = only_matching_scan(ctx, matcher, &mut state);
        state.store(ctx);
        step
    }

    /// What `search_only_matching` and `search_only_matching_context` do for a match that is binary and not shown as hex:
    /// `--binary-files=without-match` or "Binary file matches". `true` leaves the file (`goto done_search`), `false` is a
    /// `continue` of the `--files` hold.
    fn binary_match(ctx: &SearchContext, c: &mut Counters, cnf: bool) -> bool {
        let f = ctx.flags();
        ctx.with_host(|h| {
            if f.binary_without_match {
                c.matches = 0;
            } else {
                h.out_binary_file_matches();
                c.matches = 1;
            }
            !(f.files && cnf && h.out_holding())
        })
    }

    /// What the part of `-o` and `-o -ABC` before the output of a match decides: the loop goes on with the output (`Go`),
    /// takes the next match (`Continue`), stops (`Break`) or the branch ends with a step (`Leave`).
    enum Gate {
        Go,
        Continue,
        Break,
        Leave(SearchStep),
    }

    fn match_gate(ctx: &SearchContext, matcher: &MatcherHandle, c: &mut Counters, current_lineno: usize, binary: bool, cnf: bool) -> Gate {
        let f = ctx.flags();

        // --range: max line exceeded?
        if f.max_line > 0 && current_lineno > f.max_line {
            return Gate::Break;
        }

        // output blocked?
        if ctx.with_host(|h| h.out_eof()) {
            return Gate::Leave(SearchStep::EXIT);
        }

        if cnf {
            // check CNF AND/OR/NOT matching
            let mut matched = false;
            if let Some(step) = cnf_line(ctx, matcher, false, &mut matched) {
                return Gate::Leave(step);
            }
            if !matched {
                return Gate::Continue;
            }
        }

        if c.lineno != current_lineno || f.ungroup {
            c.lineno = current_lineno;

            c.matches += 1;

            // --min-count: require at least min-count matches
            if f.min_count > 0 && c.matches < f.min_count {
                return Gate::Continue;
            }

            // --max-count: max number of matches reached?
            if f.max_count > 0 && c.matches > f.max_count {
                return Gate::Break;
            }

            if c.matches == f.min_count + usize::from(f.min_count == 0) && (!f.files || !cnf) {
                // --max-files: max reached?
                if !ctx.with_host(|h| h.stats_found_part()) {
                    return Gate::Leave(SearchStep::EXIT);
                }
            }
        }

        if ctx.with_host(|h| h.binfile()) || (binary && !f.hex && !f.with_hex) {
            return if binary_match(ctx, c, cnf) { Gate::Leave(SearchStep::DONE) } else { Gate::Continue };
        }

        Gate::Go
    }

    fn only_matching_scan(ctx: &SearchContext, matcher: &MatcherHandle, s: &mut OnlyMatchingState) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);
        let separator = f.separator.as_deref().unwrap_or_default();
        let separator_bar = f.separator_bar.as_deref().unwrap_or_default();

        // counter for replace field %m matches
        let mut replace_matching = 0usize;
        let mut nl = false;

        // skip accurate line number counting, only count matching lines
        if !f.line_number && !f.multiline && f.max_line == 0 && f.stats.is_none() {
            matcher.lineno_skip(true);
        }

        while matcher.find() {
            let begin = matcher.begin();
            let size = matcher.size();
            let data = matcher.bytes_to_vec(begin, size);

            s.binary = check_binary(ctx, &data);

            ctx.with_host(|h| {
                if s.hex && !s.binary {
                    h.out_dump_done();
                } else if !s.hex && s.binary && nl {
                    h.out_nl(false);
                    nl = false;
                }

                if nl {
                    h.out_nl(false);
                    nl = false;
                }
            });

            let current_lineno = matcher.lineno();

            match match_gate(ctx, matcher, &mut s.counters, current_lineno, s.binary, cnf) {
                Gate::Go => {}
                Gate::Continue => continue,
                Gate::Break => break,
                Gate::Leave(step) => return step,
            }

            let first = matcher.first();

            if !f.no_header {
                ctx.with_host(|h| h.out_header(current_lineno, first, separator, s.binary));
            }

            // --min-count: require at least min-count matches
            if f.min_count > 0 && s.counters.matches < f.min_count {
                continue;
            }

            s.hex = s.binary;

            if s.binary {
                if f.hex || f.with_hex {
                    ctx.with_host(|h| {
                        h.out_dump_next(first);
                        h.out_dump_hex(HEX_MATCH, first, &data);
                    });
                } else {
                    if binary_match(ctx, &mut s.counters, cnf) {
                        return SearchStep::DONE;
                    }
                    continue;
                }

                s.counters.lineno = s.counters.lineno.wrapping_add(matcher.lines().wrapping_sub(1));
            } else if let Some(replace) = f.replace.as_deref() {
                // output --replace=FORMAT
                let matches = s.counters.matches;
                ctx.with_host(|h| {
                    h.out_str(&f.match_ms);
                    h.out_format(replace, matches, Some(&mut replace_matching), matches > 1, matches > 1);
                    h.out_str(&f.match_off);
                });
            } else {
                let mut rest: &[u8] = &data;
                let lineno = &mut s.counters.lineno;
                ctx.with_host(|h| {
                    if f.multiline {
                        // echo multi-line matches line-by-line
                        let mut from = 0;
                        while let Some(p) = rest[from..].iter().position(|&c| c == b'\n') {
                            let to = from + p;
                            h.out_str(&f.match_ms);
                            h.out_str(&rest[from..to]);
                            h.out_str(&f.match_off);
                            h.out_chr(b'\n');

                            *lineno += 1;
                            h.out_header_no_matcher(*lineno, first + to + 1, separator_bar);

                            from = to + 1;
                        }
                        rest = &rest[from..];
                    }

                    if !rest.is_empty() {
                        let lf_only = rest[rest.len() - 1] == b'\n';
                        let n = rest.len() - usize::from(lf_only);
                        if n > 0 {
                            h.out_str(&f.match_ms);
                            h.out_str(&rest[..n]);
                            h.out_str(&f.match_off);
                        }
                        h.out_nl(lf_only);
                    } else {
                        nl = true;
                    }
                });
            }
        }

        // --min-count: require at least min-count matches
        if f.min_count > 0 && s.counters.matches < f.min_count {
            return SearchStep::EXIT;
        }

        ctx.with_host(|h| {
            if nl {
                h.out_nl(false);
            }
            if s.hex {
                h.out_dump_done();
            }
        });

        SearchStep::DONE
    }

    /// The end of a line that `-o -ABC` shows: the rest of the line after the last match, and the number of matches that
    /// did not fit, then the newline (`width` is what is left of the context width).
    fn only_matching_context_tail(h: &dyn GrepHost, f: &super::GrepFlags, width: usize, count: usize) {
        if h.restline_size() > 0 {
            let rest = h.restline_bytes();
            h.out_str(&f.color_cx);
            if utf8nlen(&rest) > width.wrapping_add(3) {
                h.out_utf8strn(&rest, width);
                h.out_str(&f.color_off);
                h.out_str(&f.color_se);
                h.out_str(b"...");
            } else {
                h.out_str(&rest);
            }
            h.out_str(&f.color_off);
            h.set_restline_size(0);
        }

        if count > 0 {
            h.out_str(&f.color_se);
            h.out_str(b"...");
            h.out_str(&f.color_off);
            h.out_str(&f.color_cn);
            h.out_str(b"[+");
            h.out_num(count, 0);
            h.out_str(b" more]");
            h.out_str(&f.color_off);
        }

        h.out_nl(false);
    }

    /// `Grep::search_only_matching_context`: `-o` with `-A`, `-B` or `-C`.
    pub fn only_matching_context(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let mut state = OnlyMatchingState::load(ctx);
        let step = only_matching_context_scan(ctx, matcher, &mut state);
        state.store(ctx);
        step
    }

    fn only_matching_context_scan(ctx: &SearchContext, matcher: &MatcherHandle, s: &mut OnlyMatchingState) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);
        let separator = f.separator.as_deref().unwrap_or_default();
        let separator_bar = f.separator_bar.as_deref().unwrap_or_default();

        let full_width = f.before_context + f.after_context;
        let mut count = 0usize;
        let mut width = full_width;
        let mut nl = false;

        // skip accurate line number counting, only count matching lines
        if !f.line_number && !f.multiline && f.max_line == 0 && f.stats.is_none() {
            matcher.lineno_skip(true);
        }

        // register the event handler to update restline on buffer shift, the matcher resets it when a new input source is
        // assigned
        matcher.set_shift_handler(HandlerKind::ONLY_MATCHING_CONTEXT);

        // the rest of the matching line
        ctx.with_host(|h| {
            h.set_restline_data(RestlineData::None);
            h.set_restline_size(0);
            h.set_restline_last(0);
        });

        while matcher.find() {
            let begin = matcher.begin();
            let size = matcher.size();
            let data = matcher.bytes_to_vec(begin, size);

            s.binary = check_binary(ctx, &data);

            ctx.with_host(|h| {
                if s.hex && !s.binary {
                    h.out_dump_done();
                } else if !s.hex && s.binary && nl {
                    h.out_nl(false);
                    nl = false;
                }

                if nl {
                    only_matching_context_tail(h, &f, width, count);

                    count = 0;
                    width = full_width;
                    nl = false;
                }
            });

            let current_lineno = matcher.lineno();

            match match_gate(ctx, matcher, &mut s.counters, current_lineno, s.binary, cnf) {
                Gate::Go => {}
                Gate::Continue => continue,
                Gate::Break => break,
                Gate::Leave(step) => return step,
            }

            let first = matcher.first();
            let last = matcher.last();

            if !f.no_header {
                ctx.with_host(|h| h.out_header(current_lineno, first, separator, s.binary));
            }

            // --min-count: require at least min-count matches
            if f.min_count > 0 && s.counters.matches < f.min_count {
                continue;
            }

            s.hex = s.binary;

            if s.binary {
                if f.hex || f.with_hex {
                    ctx.with_host(|h| {
                        h.out_dump_next(first);
                        h.out_dump_hex(HEX_MATCH, first, &data);
                    });
                } else {
                    if binary_match(ctx, &mut s.counters, cnf) {
                        return SearchStep::DONE;
                    }
                    continue;
                }

                s.counters.lineno = s.counters.lineno.wrapping_add(matcher.lines().wrapping_sub(1));
                continue;
            }

            ctx.with_host(|h| {
                if h.restline_size() > 0 {
                    if first.wrapping_sub(h.restline_last()) < h.restline_size() {
                        h.set_restline_size(first.wrapping_sub(h.restline_last()));
                    }

                    let rest = h.restline_bytes();
                    let after = utf8nlen(&rest).min(width);

                    width -= after;

                    if after > 0 {
                        h.out_str(&f.color_cx);
                        h.out_utf8strn(&rest, after);
                        h.out_str(&f.color_off);
                    }

                    h.set_restline_size(0);
                }
            });

            if width == 0 {
                count += 1;
            } else {
                let mut pos = 0usize;
                let mut size = size;

                loop {
                    // check for additional lines in the match output
                    let mut rest = 0usize;
                    let to = data[pos..pos + size].iter().position(|&c| c == b'\n');
                    if let Some(p) = to {
                        rest = size;
                        size = p;
                        rest -= size;
                    }

                    // length of the match is the number of UTF-8-encoded Unicode characters
                    let piece = &data[pos..pos + size];
                    let length = utf8nlen(piece);
                    let mut fit_length = length;

                    if fit_length > width {
                        if fit_length > width + 4 {
                            fit_length = width;
                        }
                        width = 0;
                    } else {
                        width -= fit_length;
                    }

                    if !nl {
                        let bol = matcher.bol();
                        let border = matcher.border();
                        let before_bytes = matcher.bytes_to_vec(bol, border);
                        let margin = utf8nlen(&before_bytes);
                        let mut before = f.before_context * fit_length / (f.before_context + f.after_context);

                        before = f.before_context.saturating_sub(before);

                        if margin > before {
                            let skip = utf8skipn(&before_bytes, border, margin - before);
                            // the C++ gives utf8strn `border` bytes from the skipped position, up to the end of the match
                            let window = matcher.bytes_to_vec(bol + skip, border.min(border + data.len() - skip));
                            ctx.with_host(|h| {
                                h.out_str(&f.color_se);
                                h.out_str(b"...");
                                h.out_str(&f.color_off);
                                h.out_str(&f.color_cx);
                                h.out_utf8strn(&window, before);
                                h.out_str(&f.color_off);
                            });
                            width = width.wrapping_sub(before);
                        } else {
                            ctx.with_host(|h| {
                                if border > 0 {
                                    h.out_str(&f.color_cx);
                                    h.out_str(&before_bytes);
                                    h.out_str(&f.color_off);
                                }
                            });
                            if margin >= 3 {
                                width = width.wrapping_sub(margin - 3);
                            } else {
                                width = width.wrapping_add(3 - margin);
                            }
                        }
                    }

                    ctx.with_host(|h| {
                        h.out_str(&f.match_ms);
                        if fit_length == length {
                            h.out_str(piece);
                        } else {
                            h.out_utf8strn(piece, fit_length);
                            h.out_str(b"[+");
                            h.out_num(length - fit_length, 0);
                            h.out_chr(b']');
                        }
                        h.out_str(&f.match_off);
                    });

                    // no more additional lines to output?
                    let Some(p) = to else { break };

                    ctx.with_host(|h| h.out_nl(false));

                    // output header and repeat to output a multiline match after previous newline
                    if !f.no_header {
                        let lineno = s.counters.lineno + 1;
                        s.counters.lineno = lineno;
                        ctx.with_host(|h| h.out_header(lineno, first + p, separator_bar, false));
                    }

                    pos += p + 1;
                    size = rest - 1;
                    width = full_width;

                    nl = true;
                }

                let eol = matcher.eol(false); // warning: call eol() before bol() and end()
                let end = matcher.end();

                ctx.with_host(|h| {
                    h.set_restline_data(RestlineData::Buffer(end));
                    h.set_restline_size(eol.wrapping_sub(end));
                    h.set_restline_last(last);
                });
            }

            nl = true;
        }

        // --min-count: require at least min-count matches
        if f.min_count > 0 && s.counters.matches < f.min_count {
            return SearchStep::EXIT;
        }

        ctx.with_host(|h| {
            if nl {
                only_matching_context_tail(h, &f, width, count);
            }
            if s.hex {
                h.out_dump_done();
            }
        });

        SearchStep::DONE
    }

    /// `Grep::search_plain`: plain matches and `--hex`.
    pub fn plain(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let mut state = OnlyMatchingState::load(ctx);
        let step = plain_scan(ctx, matcher, &mut state);
        state.store(ctx);
        step
    }

    /// `check_binary(bol, n)` of the bytes at `offset` of the matcher buffer: copies them only when `-W` needs the check.
    fn check_binary_at(ctx: &SearchContext, matcher: &MatcherHandle, offset: usize, len: usize) -> bool {
        let f = ctx.flags();
        if f.hex {
            return true;
        }
        if !f.with_hex {
            return false;
        }
        let bytes = matcher.bytes_to_vec(offset, len);
        check_binary(ctx, &bytes)
    }

    /// Echo a multi-line match line by line: the text up to each `\n` with the match colors, then the header of the
    /// next line. Returns the offset in `data` of the text after the last `\n`. `next_lineno` gives the line number of
    /// each header.
    fn echo_lines(h: &dyn GrepHost, f: &super::GrepFlags, ms: &[u8], data: &[u8], first: usize, mut next_lineno: impl FnMut() -> usize) -> usize {
        let separator_bar = f.separator_bar.as_deref().unwrap_or_default();
        let mut from = 0;
        while let Some(p) = data[from..].iter().position(|&c| c == b'\n') {
            let to = from + p;
            h.out_str(ms);
            h.out_str(&data[from..to]);
            h.out_str(&f.match_off);
            h.out_chr(b'\n');

            h.out_header_no_matcher(next_lineno(), first + to + 1, separator_bar);

            from = to + 1;
        }
        from
    }

    /// The rest of a line after its last match when `-u` is set: up to the end of the line, then the newline.
    fn ungroup_tail(ctx: &SearchContext, matcher: &MatcherHandle, color_sl: &[u8], end: usize, eol: usize) {
        let f = ctx.flags();
        if eol > end {
            let mut rest = matcher.bytes_to_vec(end, eol - end);
            let lf_only = rest[rest.len() - 1] == b'\n';
            rest.truncate(rest.len() - usize::from(lf_only));
            ctx.with_host(|h| {
                if !rest.is_empty() {
                    h.out_str(color_sl);
                    h.out_str(&rest);
                    h.out_str(&f.color_off);
                }
                h.out_nl(lf_only);
            });
        } else if matcher.hit_end() {
            ctx.with_host(|h| h.out_nl(false));
        } else {
            ctx.with_host(|h| h.out_check_flush());
        }
    }

    /// `restline_data = end; restline_size = eol - end; restline_last = last`.
    fn set_restline(ctx: &SearchContext, end: usize, eol: usize, last: usize) {
        ctx.with_host(|h| {
            h.set_restline_data(RestlineData::Buffer(end));
            h.set_restline_size(eol.wrapping_sub(end));
            h.set_restline_last(last);
        });
    }

    /// The `--hexdump` context after the match: `right` bytes of the line that follow the last column (`-A`).
    fn hex_right(f: &super::GrepFlags, last: usize) -> usize {
        f.hex_after
            .wrapping_mul(f.hex_columns)
            .wrapping_sub(last.wrapping_sub(1).checked_rem(f.hex_columns).unwrap_or(0))
            .wrapping_sub(1)
    }

    fn plain_scan(ctx: &SearchContext, matcher: &MatcherHandle, s: &mut OnlyMatchingState) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);
        let separator = f.separator.as_deref().unwrap_or_default();
        let columns = f.hex_columns;
        let hex_context = f.hex_after > 0 || f.hex_before > 0;
        let colorize = f.color.is_some() || f.replace.is_some() || f.tag.is_some();

        // counter for replace field %m matches
        let mut replace_matching = 0usize;

        // skip accurate line number counting, only count matching lines
        if !f.line_number && !f.multiline && f.max_line == 0 && f.stats.is_none() {
            matcher.lineno_skip(true);
        }

        // register the event handler to update restline on buffer shift, the matcher resets it when a new input source is
        // assigned
        matcher.set_shift_handler(HandlerKind::GREP);

        // --hexdump: keep -B NUM+1 before context times hex column bytes in the buffer when shifting
        if f.hex_before > 0 {
            matcher.set_reserve(f.hex_before * columns);
        }

        // the rest of the matching line
        ctx.with_host(|h| {
            h.set_restline_data(RestlineData::None);
            h.set_restline_size(0);
            h.set_restline_last(0);
        });

        loop {
            // the GREP shift handler that find() may run compares Grep::lineno with the matcher's line: keep the host
            // current, as the C++ member was
            ctx.with_host(|h| h.set_lineno(s.counters.lineno));
            if !matcher.find() {
                break;
            }
            let current_lineno = matcher.lineno();

            if s.counters.lineno != current_lineno || f.ungroup {
                ctx.with_host(|h| h.restline_output(false, f.hex_after > 0));

                // --range: max line exceeded?
                if f.max_line > 0 && current_lineno > f.max_line {
                    break;
                }

                // --max-count: max number of matches reached?
                if f.max_count > 0 && s.counters.matches >= f.max_count {
                    break;
                }

                // output blocked?
                if ctx.with_host(|h| h.out_eof()) {
                    return SearchStep::EXIT;
                }

                let mut eol = matcher.eol(true); // warning: call eol() before bol() and end()
                let mut bol = matcher.bol();

                // check CNF AND/OR/NOT matching
                if cnf {
                    match file::cnf_matching(ctx, matcher, bol, eol, false) {
                        CnfResult::EXIT => return SearchStep::EXIT,
                        CnfResult::YES => {}
                        _ => continue,
                    }
                }

                s.counters.matches += 1;

                // --min-count: require at least min-count matches
                if f.min_count > 0 && s.counters.matches < f.min_count {
                    s.counters.lineno = current_lineno;
                    continue;
                }

                if s.counters.matches == f.min_count + usize::from(f.min_count == 0) && (!f.files || !cnf) {
                    // --max-files: max reached?
                    if !ctx.with_host(|h| h.stats_found_part()) {
                        return SearchStep::EXIT;
                    }
                }

                s.binary = check_binary_at(ctx, matcher, bol, eol.wrapping_sub(bol));
                ctx.with_host(|h| h.set_binary(s.binary));

                // --hexdump with ABC context: if binary then show hex context
                if s.binary && hex_context {
                    let aft = matcher.aft((f.hex_after + f.hex_before) * columns);
                    // aft() may shift the buffer: take eol and bol from the matcher again
                    eol = matcher.eol(true);
                    bol = matcher.bol();
                    if aft > eol {
                        eol = aft;
                    }
                }

                if ctx.with_host(|h| h.binfile()) || (s.binary && !f.hex && !f.with_hex) {
                    if binary_match(ctx, &mut s.counters, cnf) {
                        return SearchStep::DONE;
                    }
                    continue;
                }

                let mut border = matcher.border();
                let first = matcher.first();
                let last = matcher.last();
                let begin = matcher.begin();
                let end = matcher.end();
                let size = matcher.size();

                if s.hex && !s.binary {
                    ctx.with_host(|h| h.out_dump_done());
                }

                if !f.no_header {
                    ctx.with_host(|h| h.out_header(current_lineno, first, separator, s.binary));
                }

                s.hex = s.binary;

                s.counters.lineno = current_lineno;

                if s.binary {
                    // --hexdump with ABC context: if binary then show hex context
                    if f.hex_before > 0 {
                        let bef = matcher.bef(f.hex_before * columns);
                        if bef < bol {
                            bol = bef;
                            border = begin.wrapping_sub(bol);
                        }

                        let mut left = 0usize;
                        let (rest_last, rest_size) = ctx.with_host(|h| (h.restline_last(), h.restline_size()));
                        if rest_last.wrapping_add(rest_size) < first {
                            left = (f.hex_before * columns + first % columns).wrapping_sub(columns);
                            if rest_last.wrapping_add(rest_size).wrapping_add(left) > first {
                                left = first.wrapping_sub(rest_last.wrapping_add(rest_size));
                            }
                        }

                        if begin > bol.wrapping_add(left) {
                            bol = begin.wrapping_sub(left);
                            border = left;
                        }
                    }

                    let line = matcher.bytes_to_vec(bol, border);
                    let body = matcher.bytes_to_vec(begin, size);
                    ctx.with_host(|h| {
                        h.out_dump_hex(HEX_LINE, first.wrapping_sub(border), &line);
                        h.out_dump_hex(HEX_MATCH, first, &body);
                    });

                    if f.ungroup {
                        // --hexdump with ABC context: if binary then show hex context
                        if f.hex_after > 0 {
                            let right = hex_right(&f, last);
                            if end.wrapping_add(right) < eol {
                                eol = end + right;
                            }
                        }

                        let tail = matcher.bytes_to_vec(end, eol.wrapping_sub(end));
                        ctx.with_host(|h| {
                            h.out_dump_hex(HEX_LINE, last, &tail);
                            h.out_dump_done();
                        });
                    } else {
                        set_restline(ctx, end, eol, last);
                    }

                    s.counters.lineno = s.counters.lineno.wrapping_add(matcher.lines().wrapping_sub(1));
                } else {
                    if border > 0 {
                        let line = matcher.bytes_to_vec(bol, border);
                        ctx.with_host(|h| {
                            h.out_str(&f.color_sl);
                            h.out_str(&line);
                            h.out_str(&f.color_off);
                        });
                    }

                    if let Some(replace) = f.replace.as_deref() {
                        // output --replace=FORMAT
                        let matches = s.counters.matches;
                        ctx.with_host(|h| {
                            h.out_str(&f.match_ms);
                            h.out_format(replace, matches, Some(&mut replace_matching), matches > 1, matches > 1);
                            h.out_str(&f.match_off);
                        });

                        s.counters.lineno = s.counters.lineno.wrapping_add(matcher.lines().wrapping_sub(1));
                    } else {
                        let data = matcher.bytes_to_vec(begin, size);
                        let mut from = 0;

                        if f.multiline {
                            // echo multi-line matches line-by-line
                            let lineno = &mut s.counters.lineno;
                            from = ctx.with_host(|h| {
                                echo_lines(h, &f, &f.match_ms, &data, first, || {
                                    *lineno += 1;
                                    *lineno
                                })
                            });
                        }

                        ctx.with_host(|h| {
                            h.out_str(&f.match_ms);
                            h.out_str(&data[from..]);
                            h.out_str(&f.match_off);
                        });
                    }

                    if f.ungroup {
                        ungroup_tail(ctx, matcher, &f.color_sl, end, eol);
                    } else {
                        set_restline(ctx, end, eol, last);
                    }
                }

                // no -u, no --hexdump and no colors: if the match does not span more than one line, then skip to end of the line
                // (skip() may shift the buffer and run the GREP handler, which reads Grep::lineno)
                ctx.with_host(|h| h.set_lineno(s.counters.lineno));
                if !f.ungroup && !colorize && !(s.binary && hex_context) && !f.multiline && !matcher.at_bol() && !matcher.skip(b'\n') {
                    break;
                }
            } else {
                // --min-count: require at least min-count matches
                if f.min_count > 0 && s.counters.matches < f.min_count {
                    continue;
                }

                let size = matcher.size();

                if size > 0 {
                    let lines = matcher.lines();

                    if lines > 1 || colorize {
                        plain_same_line(ctx, matcher, s, &mut replace_matching, current_lineno);
                    }
                }
            }
        }

        // --min-count: require at least min-count matches
        if f.min_count > 0 && s.counters.matches < f.min_count {
            return SearchStep::EXIT;
        }

        ctx.with_host(|h| {
            h.restline_output(false, f.hex_after > 0);
            if s.binary {
                h.out_dump_done();
            }
        });

        SearchStep::DONE
    }

    /// The match is on the line of the previous match (`lineno == current_lineno`, no `-u`) and spans lines or colors.
    fn plain_same_line(ctx: &SearchContext, matcher: &MatcherHandle, s: &mut OnlyMatchingState, replace_matching: &mut usize, current_lineno: usize) {
        let f = ctx.flags();
        let separator = f.separator.as_deref().unwrap_or_default();
        let separator_bar = f.separator_bar.as_deref().unwrap_or_default();
        let columns = f.hex_columns;
        let hex_context = f.hex_after > 0 || f.hex_before > 0;

        let lines = matcher.lines();
        let size = matcher.size();
        let first = matcher.first();
        let last = matcher.last();
        let begin = matcher.begin();
        let data = matcher.bytes_to_vec(begin, size);

        if s.binary {
            ctx.with_host(|h| {
                if h.restline_data() != RestlineData::None {
                    if h.restline_last().wrapping_add(h.restline_size()) > first {
                        h.set_restline_size(first.wrapping_sub(h.restline_last()));
                    }

                    // --hexdump with ABC context: if binary then show hex context
                    if f.hex_after > 0 {
                        let rest_last = h.restline_last();
                        let right = hex_right(&f, rest_last);
                        if right < h.restline_size() {
                            let bytes = h.restline_bytes();
                            h.out_dump_hex(HEX_LINE, rest_last, &bytes[..right]);

                            h.restline_advance(right);
                            h.set_restline_size(h.restline_size() - right);
                            h.set_restline_last(rest_last + right);
                        } else {
                            h.out_dump_hex(HEX_LINE, rest_last, &h.restline_bytes());

                            h.set_restline_data(RestlineData::None);
                        }
                    }

                    if h.restline_data() != RestlineData::None {
                        // --hexdump with ABC context: if binary then show hex context
                        if f.hex_before > 0 {
                            let left = (f.hex_before * columns + first % columns).wrapping_sub(columns);
                            if left < h.restline_size() {
                                if !f.no_header {
                                    h.out_header(current_lineno, first, separator_bar, s.binary);
                                }

                                let skip = h.restline_size() - left;
                                h.restline_advance(skip);
                                h.set_restline_last(h.restline_last() + skip);
                                h.set_restline_size(left);
                            }
                        }

                        h.out_dump_hex(HEX_LINE, h.restline_last(), &h.restline_bytes());
                    }

                    h.set_restline_data(RestlineData::None);
                }

                h.out_dump_hex(HEX_MATCH, first, &data);
            });
        } else {
            ctx.with_host(|h| {
                if h.restline_data() != RestlineData::None {
                    if h.restline_last().wrapping_add(h.restline_size()) > first {
                        h.set_restline_size(first.wrapping_sub(h.restline_last()));
                    }
                    h.out_str(&f.color_sl);
                    h.out_str(&h.restline_bytes());
                    h.out_str(&f.color_off);
                }

                if let Some(replace) = f.replace.as_deref() {
                    // output --replace=FORMAT
                    let matches = s.counters.matches;
                    h.out_str(&f.match_ms);
                    h.out_format(replace, matches, Some(replace_matching), matches > 1, matches > 1);
                    h.out_str(&f.match_off);
                } else {
                    let mut from = 0;

                    if lines > 1 {
                        // echo multi-line matches line-by-line
                        let mut num = 0usize;
                        let lineno = s.counters.lineno;
                        from = echo_lines(h, &f, &f.match_ms, &data, first, || {
                            num += 1;
                            lineno + num
                        });
                    }

                    h.out_str(&f.match_ms);
                    h.out_str(&data[from..]);
                    h.out_str(&f.match_off);
                }
            });
        }

        let mut eol = matcher.eol(true); // warning: call eol() before end()
        let mut end = matcher.end();

        if lines == 1 {
            // --hexdump with ABC context: if binary then show hex context
            if s.binary && hex_context {
                let aft = matcher.aft((f.hex_after + f.hex_before) * columns);
                // aft() may shift the buffer: take eol and end from the matcher again
                eol = matcher.eol(true);
                end = matcher.end();
                if aft > eol {
                    eol = aft;
                }
            }

            set_restline(ctx, end, eol, last);
        } else {
            s.binary = check_binary_at(ctx, matcher, end, eol.wrapping_sub(end));
            ctx.with_host(|h| h.set_binary(s.binary));

            // --hexdump with ABC context: if binary then show hex context
            if s.binary && hex_context {
                let aft = matcher.aft((f.hex_after + f.hex_before) * columns);
                // aft() may shift the buffer: take eol and end from the matcher again
                eol = matcher.eol(true);
                end = matcher.end();
                if aft > eol {
                    eol = aft;
                }
            }

            ctx.with_host(|h| {
                if s.hex && !s.binary {
                    h.out_dump_done();
                } else if !s.hex && s.binary {
                    h.out_nl(false);
                }

                if s.hex != s.binary && !f.no_header {
                    h.out_header(s.counters.lineno + lines - 1, last, separator, s.binary);
                }
            });

            s.hex = s.binary;

            if f.ungroup {
                if s.binary {
                    // --hexdump with ABC context: if binary then show hex context
                    if f.hex_after > 0 {
                        let right = hex_right(&f, last);
                        if end.wrapping_add(right) < eol {
                            eol = end + right;
                        }
                    }

                    let tail = matcher.bytes_to_vec(end, eol.wrapping_sub(end));
                    ctx.with_host(|h| {
                        h.out_dump_hex(HEX_LINE, last, &tail);
                        h.out_dump_done();
                    });
                } else {
                    ungroup_tail(ctx, matcher, &f.color_sl, end, eol);
                }
            } else {
                set_restline(ctx, end, eol, last);
            }

            s.counters.lineno += lines - 1;
        }
    }

    /// `Grep::matches`, `lineno`, `binary`, `hex` and `stop` read and written in the host: the handlers of `-v` and `-y`
    /// (`InvertMatchGrepHandler`, `AnyLineGrepHandler`) work on them from inside `find` and the explicit calls.
    struct Live<'a>(&'a SearchContext);

    impl Live<'_> {
        fn matches(&self) -> usize {
            self.0.with_host(|h| h.matches())
        }
        fn set_matches(&self, v: usize) {
            self.0.with_host(|h| h.set_matches(v));
        }
        fn lineno(&self) -> usize {
            self.0.with_host(|h| h.lineno())
        }
        fn set_lineno(&self, v: usize) {
            self.0.with_host(|h| h.set_lineno(v));
        }
        fn binary(&self) -> bool {
            self.0.with_host(|h| h.binary())
        }
        fn set_binary(&self, v: bool) {
            self.0.with_host(|h| h.set_binary(v));
        }
        fn hex(&self) -> bool {
            self.0.with_host(|h| h.hex())
        }
        fn set_hex(&self, v: bool) {
            self.0.with_host(|h| h.set_hex(v));
        }
        fn stop(&self) -> bool {
            self.0.with_host(|h| h.stop())
        }
        /// `binfile || (binary && !flag_hex && !flag_with_hex)`: the file is binary and not shown as hex.
        fn binary_hidden(&self) -> bool {
            let f = self.0.flags();
            self.0.with_host(|h| h.binfile() || (h.binary() && !f.hex && !f.with_hex))
        }
        /// `flag_binary_without_match ? matches = 0 : out.binary_file_matches()`.
        fn binary_matches(&self, set_one: bool) {
            let f = self.0.flags();
            self.0.with_host(|h| {
                if f.binary_without_match {
                    h.set_matches(0);
                } else {
                    h.out_binary_file_matches();
                    if set_one {
                        h.set_matches(1);
                    }
                }
            });
        }
    }

    /// `Grep::search_invert`: `-v` without `-A`, `-B`, `-C`, `-y`.
    pub fn invert(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);
        let live = Live(ctx);

        // register the event handler to output non-matching lines, the matcher resets it when a new input source is assigned
        matcher.set_shift_handler(HandlerKind::INVERT_MATCH);

        while matcher.find() {
            let current_lineno = matcher.lineno();

            if live.lineno() != current_lineno {
                if cnf {
                    let mut matched = false;
                    if let Some(step) = cnf_line(ctx, matcher, false, &mut matched) {
                        return step;
                    }
                    // check CNF AND/OR/NOT matching
                    if !matched {
                        continue;
                    }
                }

                // get the lines before the matched line and output the non-matching lines up to this line
                let context = matcher.before();
                if context.len > 0 {
                    matcher.invoke_handler(context);
                }

                if live.matches() > 0 && live.binary_hidden() {
                    break;
                }

                if live.binary() {
                    ctx.with_host(|h| h.out_dump_done());
                }

                // --range: max line exceeded?
                if f.max_line > 0 && current_lineno > f.max_line {
                    return SearchStep::DONE;
                }

                // --max-files: max reached?
                if live.stop() {
                    return SearchStep::EXIT;
                }

                // --max-count: max number of matches reached?
                if f.max_count > 0 && live.matches() >= f.max_count {
                    return SearchStep::DONE;
                }

                // output blocked?
                if ctx.with_host(|h| h.out_eof()) {
                    return SearchStep::EXIT;
                }
            }

            live.set_lineno(current_lineno.wrapping_add(matcher.lines()).wrapping_sub(1));
        }

        // get the remaining context after the last match to output
        let context = matcher.after();
        if context.len > 0 {
            matcher.invoke_handler(context);
        }

        if live.matches() > 0 && live.binary_hidden() {
            if f.binary_without_match {
                live.set_matches(0);
            } else {
                ctx.with_host(|h| h.out_binary_file_matches());
            }
        }

        if live.binary() {
            ctx.with_host(|h| h.out_dump_done());
        }

        SearchStep::DONE
    }

    /// `Grep::search_any_line`: `-y`.
    pub fn any_line(ctx: &SearchContext, matcher: &MatcherHandle) -> SearchStep {
        let f = ctx.flags();
        let cnf = has_cnf(ctx);
        let live = Live(ctx);
        let colorize = f.color.is_some() || f.replace.is_some() || f.tag.is_some();
        let invert = f.invert_match;

        // counter for replace field %m matches
        let mut replace_matching = 0usize;

        // to output the rest of the matching line
        ctx.with_host(|h| {
            h.set_restline_data(RestlineData::None);
            h.set_restline_size(0);
            h.set_restline_last(0);
        });

        // register the event handler functor to output non-matching lines, the matcher resets it when a new input source is
        // assigned
        matcher.set_shift_handler(HandlerKind::ANY_LINE);

        // to output colors with or without -v
        let colors = AnyColors {
            hex_line: if invert { HEX_CONTEXT_LINE } else { HEX_LINE },
            hex_match: if invert { HEX_CONTEXT_MATCH } else { HEX_MATCH },
            sl: if invert { &f.color_cx } else { &f.color_sl },
            ms: if invert { &f.match_mc } else { &f.match_ms },
            separator: if invert { f.separator_dash.as_deref() } else { f.separator.as_deref() }.unwrap_or_default(),
        };

        while matcher.find() {
            let current_lineno = matcher.lineno();

            if live.lineno() != current_lineno {
                ctx.with_host(|h| h.restline_output(invert, false));

                let eol = matcher.eol(true); // warning: call eol() before bol() and end()
                let bol = matcher.bol();

                // check CNF AND/OR/NOT matching
                if cnf {
                    match file::cnf_matching(ctx, matcher, bol, eol, false) {
                        CnfResult::EXIT => return SearchStep::EXIT,
                        CnfResult::YES => {}
                        _ => continue,
                    }
                }

                if !invert {
                    live.set_matches(live.matches() + 1);
                    let matches = live.matches();

                    // --min-count: require at least min-count matches
                    if f.min_count > 0 && matches < f.min_count {
                        live.set_lineno(current_lineno);
                        continue;
                    }

                    if matches == f.min_count + usize::from(f.min_count == 0) && (!f.files || !cnf) {
                        // --max-files: max reached?
                        if !ctx.with_host(|h| h.stats_found_part()) {
                            return SearchStep::EXIT;
                        }
                    }
                }

                // --range: max line exceeded?
                if f.max_line > 0 && current_lineno > f.max_line {
                    break;
                }

                // --max-files: max reached?
                if live.stop() {
                    return SearchStep::EXIT;
                }

                // get the lines before the matched line
                let context = matcher.before();

                if context.len > 0 {
                    matcher.invoke_handler(context);

                    if live.matches() > 0 && live.binary_hidden() {
                        live.binary_matches(true);

                        if f.files && cnf && ctx.with_host(|h| h.out_holding()) {
                            continue;
                        }

                        return SearchStep::DONE;
                    }
                }

                // output blocked?
                if ctx.with_host(|h| h.out_eof()) {
                    return SearchStep::EXIT;
                }

                live.set_binary(check_binary_at(ctx, matcher, bol, eol.wrapping_sub(bol)));

                if live.binary_hidden() {
                    if f.binary_without_match {
                        live.set_matches(0);
                    } else if invert {
                        live.set_lineno(current_lineno.wrapping_add(matcher.lines()).wrapping_sub(1));
                        continue;
                    } else {
                        ctx.with_host(|h| h.out_binary_file_matches());
                        live.set_matches(1);
                    }

                    if f.files && cnf && ctx.with_host(|h| h.out_holding()) {
                        continue;
                    }

                    return SearchStep::DONE;
                }

                any_line_first(ctx, matcher, &live, &colors, &mut replace_matching, LineAt { lineno: current_lineno, bol, eol });

                // --max-count: max number of matches reached?
                if f.max_count > 0 && live.matches() >= f.max_count {
                    break;
                }

                // no -u and no colors: if the match does not span more than one line, then skip to end of the line
                if !f.ungroup && !colorize && !f.multiline && !matcher.at_bol() && !matcher.skip(b'\n') {
                    break;
                }
            } else if !live.binary_hidden() {
                // --min-count: require at least min-count matches
                if f.min_count > 0 && live.matches() < f.min_count {
                    continue;
                }

                if matcher.size() > 0 && (matcher.lines() > 1 || colorize) {
                    any_line_same_line(ctx, matcher, &live, &colors, &mut replace_matching);
                }
            }
        }

        ctx.with_host(|h| h.restline_output(invert, false));

        if f.max_count == 0 || live.matches() < f.max_count {
            // get the remaining context after the last match to output
            let context = matcher.after();

            if context.len > 0 {
                matcher.invoke_handler(context);
            }
        }

        // --min-count: require at least min-count matches
        if f.min_count > 0 && live.matches() < f.min_count {
            live.set_matches(0);
        }

        if live.matches() > 0 && live.binary_hidden() {
            live.binary_matches(false);
        }

        if live.binary() {
            ctx.with_host(|h| h.out_dump_done());
        }

        SearchStep::DONE
    }

    /// The colors, hex dump modes and separator of `-y` with or without `-v`.
    struct AnyColors<'a> {
        hex_line: i16,
        hex_match: i16,
        sl: &'a [u8],
        ms: &'a [u8],
        separator: &'a [u8],
    }

    /// The line of the current match: its number and the offsets of its begin and end in the matcher buffer.
    struct LineAt {
        lineno: usize,
        bol: usize,
        eol: usize,
    }

    /// The output of the first match of a line of `-y` (the `lineno != current_lineno` part after the checks): header, the
    /// text before the match, the match, and the rest of the line saved for later.
    fn any_line_first(ctx: &SearchContext, matcher: &MatcherHandle, live: &Live, c: &AnyColors, replace_matching: &mut usize, line_at: LineAt) {
        let LineAt { lineno: current_lineno, bol, eol } = line_at;
        let f = ctx.flags();
        let border = matcher.border();
        let first = matcher.first();
        let last = matcher.last();
        let begin = matcher.begin();
        let end = matcher.end();
        let size = matcher.size();
        let binary = live.binary();

        if live.hex() && !binary {
            ctx.with_host(|h| h.out_dump_done());
        }

        if !f.no_header {
            ctx.with_host(|h| h.out_header(current_lineno, first, c.separator, binary));
        }

        live.set_hex(binary);

        let mut lineno = current_lineno;
        live.set_lineno(lineno);

        if binary {
            let line = matcher.bytes_to_vec(bol, border);
            let body = matcher.bytes_to_vec(begin, size);
            ctx.with_host(|h| {
                h.out_dump_hex(c.hex_line, first.wrapping_sub(border), &line);
                h.out_dump_hex(c.hex_match, first, &body);
            });

            set_restline(ctx, end, eol, last);

            lineno += matcher.lines() - 1;
        } else {
            if border > 0 {
                let line = matcher.bytes_to_vec(bol, border);
                ctx.with_host(|h| {
                    h.out_str(c.sl);
                    h.out_str(&line);
                    h.out_str(&f.color_off);
                });
            }

            if let Some(replace) = f.replace.as_deref() {
                // output --replace=FORMAT
                let matches = live.matches();
                ctx.with_host(|h| {
                    h.out_str(c.ms);
                    h.out_format(replace, matches, Some(replace_matching), matches > 1, matches > 1);
                    h.out_str(&f.match_off);
                });

                lineno += matcher.lines() - 1;
            } else {
                let data = matcher.bytes_to_vec(begin, size);
                let mut from = 0;

                if f.multiline {
                    // echo multi-line matches line-by-line
                    from = ctx.with_host(|h| {
                        echo_lines(h, &f, c.ms, &data, first, || {
                            lineno += 1;
                            lineno
                        })
                    });
                }

                ctx.with_host(|h| {
                    h.out_str(c.ms);
                    h.out_str(&data[from..]);
                    h.out_str(&f.match_off);
                });
            }

            set_restline(ctx, end, eol, last);
        }

        live.set_lineno(lineno);
    }

    /// The match is on the line of the previous match (`lineno == current_lineno`) and spans lines or colors, for `-y`.
    fn any_line_same_line(ctx: &SearchContext, matcher: &MatcherHandle, live: &Live, c: &AnyColors, replace_matching: &mut usize) {
        let f = ctx.flags();
        let lines = matcher.lines();
        let size = matcher.size();
        let first = matcher.first();
        let last = matcher.last();
        let begin = matcher.begin();
        let data = matcher.bytes_to_vec(begin, size);
        let binary = live.binary();
        let lineno = live.lineno();

        ctx.with_host(|h| {
            if h.restline_data() != RestlineData::None {
                if h.restline_last().wrapping_add(h.restline_size()) > first {
                    h.set_restline_size(first.wrapping_sub(h.restline_last()));
                }
                if binary {
                    h.out_dump_hex(c.hex_line, h.restline_last(), &h.restline_bytes());
                } else {
                    h.out_str(c.sl);
                    h.out_str(&h.restline_bytes());
                    h.out_str(&f.color_off);
                }
            }

            if binary {
                h.out_dump_hex(c.hex_match, first, &data);
            } else if let Some(replace) = f.replace.as_deref() {
                // output --replace=FORMAT
                let matches = h.matches();
                h.out_str(c.ms);
                h.out_format(replace, matches, Some(replace_matching), matches > 1, matches > 1);
                h.out_str(&f.match_off);
            } else {
                let mut from = 0;

                if lines > 1 {
                    // echo multi-line matches line-by-line
                    let mut num = 0usize;
                    from = echo_lines(h, &f, c.ms, &data, first, || {
                        num += 1;
                        lineno + num
                    });
                }

                h.out_str(c.ms);
                h.out_str(&data[from..]);
                h.out_str(&f.match_off);
            }
        });

        let eol = matcher.eol(true); // warning: call eol() before end()
        let end = matcher.end();

        if lines == 1 {
            set_restline(ctx, end, eol, last);
        } else {
            let binary = check_binary_at(ctx, matcher, end, eol.wrapping_sub(end));
            live.set_binary(binary);
            let hex = live.hex();

            ctx.with_host(|h| {
                if hex && !binary {
                    h.out_dump_done();
                } else if !hex && binary {
                    h.out_nl(false);
                }

                if hex != binary && !f.no_header {
                    h.out_header(lineno + lines - 1, last, f.separator.as_deref().unwrap_or_default(), binary);
                }
            });

            live.set_hex(binary);

            set_restline(ctx, end, eol, last);

            live.set_lineno(lineno + lines - 1);
        }
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

/// One stub per file member of `Grep` (ugrep.cpp ~2141-2960), named as the C++ members; ported by S4c-2b. `matcher` is
/// `Grep::matcher`. What they need of `Grep` (`file_in`, `input`, `interactive`, `zthread`, `restline*`, `out`, the CNF
/// bits) is reached through the [`GrepHost`] of `ctx`. The `mmap` of `init_read` is ported to Rust, not a callback.
pub mod file {
    use super::{
        raw, Bytes, CnfBits, CnfResult, FileSource, GrepHost, GrepState, MatcherHandle, MatcherKind, RestlineData, SearchContext, HEX_CONTEXT_LINE,
        HEX_LINE, INPUT_HANDLER_BINARY, INPUT_HANDLER_STDIN,
    };

    /// `reflex::AbstractMatcher::Const::BUFSZ`: how much of the input `init_is_binary` checks.
    const BUFSZ: usize = 256 * 1024;
    /// `Action::READ` of `flag_devices_action`.
    const ACTION_READ: i32 = 2;
    /// `Mode::PERL` of `flag_mode`.
    const MODE_PERL: i32 = 4;
    /// `PATHSEPCHR`.
    const PATHSEPCHR: u8 = if cfg!(windows) { b'\\' } else { b'/' };

    /// `Grep::cnf_matching(bol, eol, acquire)`: `bol` and `eol` are offsets into the buffer of `matcher`. `Exit` is the
    /// `throw EXIT_SEARCH()` of the C++.
    pub fn cnf_matching(ctx: &SearchContext, matcher: &MatcherHandle, bol: usize, eol: usize, acquire: bool) -> CnfResult {
        ctx.with_host(|h| {
            let terms = h.matchers_len().unwrap_or(0);
            if ctx.flags().files {
                if h.out_holding() {
                    let mut all = true; // all terms matched
                    for k in 0..terms {
                        // an OR term hasn't matched before
                        if h.cnf_bit_get(CnfBits::Matching, k, 0) {
                            continue;
                        }
                        let alts = h.cnf_alts(k);
                        if alts == 0 {
                            continue;
                        }
                        let first = h.cnf_alt_matcher(k, 0);
                        // check OR terms
                        if first.as_ref().is_some_and(|m| m.find_in_line(matcher, bol, eol)) {
                            h.cnf_bit_set(CnfBits::Matching, k, 0);
                            continue;
                        }
                        // check OR NOT terms
                        let mut none = true; // all not-terms matched
                        for l in 0..alts - 1 {
                            if let Some(m) = h.cnf_alt_matcher(k, l + 1) {
                                if !h.cnf_bit_get(CnfBits::NotMatching, k, l) {
                                    if m.find_in_line(matcher, bol, eol) {
                                        h.cnf_bit_set(CnfBits::NotMatching, k, l);
                                    } else {
                                        all = false;
                                        none = false;
                                    }
                                }
                            }
                        }
                        if none {
                            // when all not-terms matched and we don't have a positive alternative then stop searching this file
                            if first.is_none() {
                                return CnfResult::EXIT;
                            }
                            all = false;
                        }
                    }
                    if h.error_pending() {
                        return CnfResult::EXIT;
                    }
                    // if all terms matched globally per file then remove the hold to launch output
                    if all {
                        if let Some(exit) = launch(h, acquire) {
                            return exit;
                        }
                    }
                }
            } else {
                // for each AND term check if the line has a match
                for k in 0..terms {
                    let alts = h.cnf_alts(k);
                    if alts == 0 {
                        continue;
                    }
                    // check OR terms
                    if h.cnf_alt_matcher(k, 0).is_some_and(|m| m.find_in_line(matcher, bol, eol)) {
                        continue;
                    }
                    // check OR NOT terms
                    let not_matched = (1..alts).any(|j| h.cnf_alt_matcher(k, j).is_some_and(|m| !m.find_in_line(matcher, bol, eol)));
                    if !not_matched {
                        return CnfResult::NO;
                    }
                }
            }
            if h.error_pending() {
                CnfResult::EXIT
            } else {
                CnfResult::YES
            }
        })
    }

    /// `if (acquire) out.acquire(); if (!Stats::found_part()) throw EXIT_SEARCH(); out.launch();`
    fn launch(h: &dyn GrepHost, acquire: bool) -> Option<CnfResult> {
        if acquire {
            h.out_acquire();
        }
        // --max-files: max reached?
        if !h.stats_found_part() {
            return Some(CnfResult::EXIT);
        }
        h.out_launch();
        None
    }

    /// `Grep::cnf_satisfied(acquire)`; `Exit` is the `throw EXIT_SEARCH()` of the C++.
    pub fn cnf_satisfied(ctx: &SearchContext, acquire: bool) -> CnfResult {
        ctx.with_host(|h| {
            if h.out_holding() {
                // for each AND term check if the term was matched before
                for k in 0..h.matchers_len().unwrap_or(0) {
                    // an OR term hasn't matched
                    if h.cnf_bit_get(CnfBits::Matching, k, 0) {
                        continue;
                    }
                    let alts = h.cnf_alts(k);
                    // return if there are no OR NOT terms to check
                    if alts <= 1 {
                        return CnfResult::NO;
                    }
                    // check if not all of the OR NOT terms matched
                    let open = (0..alts - 1).any(|l| h.cnf_alt_matcher(k, l + 1).is_some() && !h.cnf_bit_get(CnfBits::NotMatching, k, l));
                    if !open {
                        return CnfResult::NO;
                    }
                }
                if let Some(exit) = launch(h, acquire) {
                    return exit;
                }
            }
            if h.error_pending() {
                CnfResult::EXIT
            } else {
                CnfResult::YES
            }
        })
    }

    /// `Grep::open_file(pathname, find)`: `find` is NULL for `None`. `is_stdin` is the C++ test `pathname ==
    /// Static::LABEL_STANDARD_INPUT`, by pointer identity, which the bytes of `pathname` cannot express.
    pub fn open_file(ctx: &SearchContext, pathname: &[u8], find: Option<&[u8]>, is_stdin: bool) -> bool {
        let (decompress, zmax, encoding, devices_action, probe) = {
            let f = ctx.flags();
            (f.decompress, f.zmax, i32::from(f.encoding_type), f.devices_action, !f.quiet && !f.files_with_matches && !f.count)
        };
        let label: Bytes;
        let pathname: &[u8] = if is_stdin {
            if ctx.with_host(|h| h.source().is_none()) {
                return false;
            }
            label = ctx.flags().label.clone().unwrap_or_default();
            ctx.with_host_mut(|h| h.set_file_in(Some(FileSource::Source)));
            ctx.with_host(|h| {
                if let Some(source) = h.source() {
                    h.file_set_binary(&source);
                }
            });
            &label
        } else {
            // the warning for a failed open is issued by `file_open`
            match ctx.with_host(|h| h.file_open(pathname)) {
                Some(file) => ctx.with_host_mut(|h| h.set_file_in(Some(FileSource::Owned(file)))),
                None => {
                    ctx.with_host_mut(|h| h.set_file_in(None));
                    return false;
                }
            }
            pathname
        };

        // "interactive" means possibly slow input from a TTY or pipe
        ctx.with_host(|h| h.set_interactive(false));

        // --filter: fork process to filter file, when applicable
        if !filter(ctx, pathname) {
            return false;
        }

        if devices_action != ACTION_READ {
            // Linux has system "regular files" that can be set non-blocking to raise EAGAIN e.g. for /proc and /sys files
            ctx.with_host(|h| {
                if let Some(file) = h.file_in() {
                    if file != h.stdin_file() && h.source().as_ref() != Some(&file) {
                        h.file_nonblock_regular(&file);
                    }
                }
            });
        }

        if decompress {
            // start decompression thread if not running, get pipe with decompressed input
            if !ctx.with_host_mut(|h| h.zthread_start(zmax, pathname, find, encoding)) {
                ctx.with_host_mut(|h| h.close_file_in());
                return false;
            }
        } else {
            ctx.with_host_mut(|h| h.input_set_file_in(encoding));
        }

        if cfg!(windows) {
            // for Windows we don't check for invalid Unicode input, no stdin handler
            if probe && ctx.with_host(|h| h.input_file().is_some_and(|file| file == h.stdin_file())) {
                ctx.with_host(|h| h.set_interactive(true));
            }
        } else if !decompress && probe {
            // if input is a character device (e.g. TTY) or a pipe, then make it non-blocking and register a stdin handler
            // to continue reading and flush results to output, don't check for invalid Unicode input
            ctx.with_host(|h| {
                let interactive = h.file_in().is_some_and(|file| h.file_probe_interactive(&file));
                h.set_interactive(interactive);
            });
        }

        !ctx.error_pending()
    }

    /// `Grep::filter(file_in, pathname)`: `--filter`, replaces `file_in` through the host.
    pub fn filter(ctx: &SearchContext, pathname: &[u8]) -> bool {
        let filter = ctx.with_host(|h| h.flag_filter());
        if filter.is_empty() || ctx.with_host(|h| h.file_in().is_none()) {
            return true;
        }
        // C strings
        let pathname = until_nul(pathname);
        let basename = match pathname.iter().rposition(|&c| c == PATHSEPCHR) {
            Some(i) => &pathname[i + 1..],
            None => pathname,
        };

        // get the basenames's extension suffix, don't consider . at the front of basename, otherwise skip .
        let mut suffix: Option<Bytes> = match basename.iter().rposition(|&c| c == b'.') {
            Some(0) | None => None,
            Some(i) => Some(until_nul(&basename[i + 1..]).to_vec()),
        };

        // --filter-magic-label: if the file is seekable (to rewind later), then check for a magic pattern match
        let labels: Vec<Bytes> = ctx.with_host(|h| (0..).map_while(|i| h.flag_filter_magic_label(i)).collect());
        if !labels.is_empty() && ctx.with_host(|h| h.file_in().is_some_and(|file| h.file_seek_set(&file))) {
            // --filter-magic-label: check for overriding +
            let is_plus = suffix.is_some() && labels.iter().any(|label| label.first() == Some(&b'+'));

            // --filter-magic-label: if the basename has no suffix or a +LABEL + then check magic bytes
            if suffix.is_none() || is_plus {
                // match the magic pattern, the input is rewound after the scan
                let found = ctx.with_host(|h| h.file_in().map_or(0, |file| h.filter_magic_scan(&file)));
                if found > 0 && found <= labels.len() {
                    let label = until_nul(&labels[found - 1]);
                    suffix = Some(label.strip_prefix(b"+").unwrap_or(label).to_vec());
                }
            }
        }

        // basenames without a suffix get "*" as a suffix
        let suffix = match suffix {
            Some(s) if !s.is_empty() => s,
            _ => b"*".to_vec(),
        };

        match find_filter_command(&filter, &suffix) {
            // suffix has a command to execute
            Some(command) => ctx.with_host_mut(|h| raw::filter_file_in(h, pathname, &command)),
            None => true,
        }
    }

    /// The prefix of `bytes` before the first NUL, as a C string reads it.
    fn until_nul(bytes: &[u8]) -> &[u8] {
        bytes.split(|&c| c == 0).next().unwrap_or(&[])
    }

    /// C `isspace` in the "C" locale.
    fn is_space(c: u8) -> bool {
        matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
    }

    /// The command of the first `--filter=SUFFIXES:COMMAND,...` entry that matches `suffix` (case insensitive), else the
    /// wildcard `*:COMMAND` entry; `None` when there is no command.
    fn find_filter_command(filter: &[u8], suffix: &[u8]) -> Option<Bytes> {
        let filter = until_nul(filter);
        let at = |i: usize| filter.get(i).copied().unwrap_or(0);
        let find = |c: u8, from: usize| filter.get(from..).and_then(|rest| rest.iter().position(|&x| x == c)).map(|i| i + from);
        let mut pos = 0;
        let mut default_command = None;

        // find the command corresponding to the first matching suffix specified in the filter
        let command = loop {
            while is_space(at(pos)) {
                pos += 1;
            }

            // wildcard *:command is considered only when no matching suffix was found
            if at(pos) == b'*' {
                default_command = find(b':', pos);
            }

            // match filter filename extension (case insensitive)
            let ext_match = suffix.iter().enumerate().all(|(i, &c)| c.eq_ignore_ascii_case(&at(pos + i)));
            let next = at(pos + suffix.len());
            if ext_match && (next == b':' || next == b',' || is_space(next)) {
                break find(b':', pos);
            }

            match find(b',', pos) {
                Some(comma) => pos = comma + 1,
                None => break None,
            }
        };

        // if no matching command, use the wildcard *:command when specified, skip over the ':'
        command.or(default_command).map(|colon| filter[colon + 1..].to_vec())
    }

    /// `Grep::close_file(pathname)`: true when the next file of an archive is to be searched.
    pub fn close_file(ctx: &SearchContext, pathname: &[u8]) -> bool {
        let (decompress, encoding, drain) = {
            let f = ctx.flags();
            (f.decompress, i32::from(f.encoding_type), !f.quiet && !f.files_with_matches && f.max_count == 0 && f.max_line == 0)
        };

        // check if the input has an error condition, but do not check stdin which is handled differently
        ctx.with_host(|h| {
            if let Some(file) = h.file_in() {
                if file != h.stdin_file() && h.source().as_ref() != Some(&file) {
                    h.file_read_error(&file, pathname);
                }
            }
        });

        // -z: open next archived file if any or close the compressed file/archive
        if decompress {
            // close the input FILE* and its underlying pipe
            ctx.with_host_mut(|h| h.close_input_file());

            // if output is blocked, then cancel decompression threads
            ctx.with_host(|h| {
                if h.out_eof() {
                    h.zthread_cancel();
                }
            });

            // open pipe to the next file or part in an archive if there is a next file to extract, and loop back in search()
            if ctx.with_host_mut(|h| h.zthread_open_next(pathname, encoding)) {
                return true;
            }
        }

        // drain stdin when non-seekable file such as a pipe until eof, unless option -q is used
        if drain && ctx.with_host(|h| h.file_in().is_some_and(|file| file == h.stdin_file()) && h.stdin_drain_check()) {
            let mut buf = [0u8; 16384];
            while ctx.with_host(|h| h.input_get(&mut buf)) > 0 {}
        }

        // close the current input file
        ctx.with_host_mut(|h| h.close_file_in());

        false
    }

    /// `Grep::init_read()`: false when the file is skipped (`-I` and binary). `state` is the Grep-lifetime Rust state
    /// (the mmap of the input).
    pub fn init_read(state: &GrepState, ctx: &SearchContext, matcher: &MatcherHandle) -> bool {
        let (min_line, with_filename, max_mmap, mode, binary_without_match, check_binary, null_data_flag) = {
            let f = ctx.flags();
            (
                f.min_line,
                f.with_filename,
                f.max_mmap,
                f.mode,
                f.binary_without_match,
                !f.quiet && !f.files_with_matches && !f.count && f.format.is_none() && !f.text && !f.hex && !f.with_hex,
                i32::from(f.encoding_type),
            )
        };

        // initialize Grep search and output variables
        let interactive = ctx.with_host(|h| {
            h.set_binfile(false);
            h.set_lineno(if min_line > 0 { min_line - 1 } else { 0 });
            h.set_heading(with_filename);
            h.set_hex(false);
            h.set_binary(false);
            h.set_matches(0);
            h.set_stop(false);
            h.interactive()
        });

        // attempt to mmap the input file, if mmap is supported and enabled (disabled by default)
        let mapped = if max_mmap > 0 {
            ctx.with_host(|h| if h.input_encoding() == 0 { h.input_file().and_then(|file| raw::map_input(state, &file, max_mmap)) } else { None })
        } else {
            None
        };
        if let Some(memory) = mapped {
            // matcher reads directly from protected mmap memory
            matcher.buffer(&memory);
        } else {
            // assign input to the matcher to search
            let input = ctx.with_host(|h| h.input());
            matcher.input(&input);

            if matcher.kind() == MatcherKind::BOOST && mode == MODE_PERL {
                // buffer all input to work around Boost.Regex partial matching bug
                matcher.buffer_all();
            } else if !cfg!(windows) && interactive {
                // set interactive search handler e.g. for slow stdin input
                matcher.set_input_handler(INPUT_HANDLER_STDIN);
            }
        }

        // interactive: do not check init_is_binary(), wait until handler detects binary files that have NUL bytes
        if !interactive {
            // otherwise, check if the file is a binary file
            if binary_without_match {
                // -I: do not match binary files
                if init_is_binary(ctx, matcher) {
                    return false;
                }
            } else if check_binary && null_data_flag != ctx.with_host(|h| h.encoding_null_data()) {
                // not -q, -l, -c, --format, -a, -X, -W, -00: check if initial part of the file is binary
                let binfile = init_is_binary(ctx, matcher);
                ctx.with_host(|h| h.set_binfile(binfile));

                // detect binary input dynamically when more input is read after checking the inital input
                matcher.set_input_handler(INPUT_HANDLER_BINARY);
            }
        }

        // --range=[MIN,][MAX]: start searching at line MIN
        for _ in 1..min_line {
            if !matcher.skip(b'\n') {
                break;
            }
        }

        true
    }

    /// `Grep::init_is_binary()`.
    pub fn init_is_binary(ctx: &SearchContext, matcher: &MatcherHandle) -> bool {
        // check initial input for binary data, fill up to BUFSZ bytes to check (similar GNU grep, but a bit more)
        let mut avail = matcher.avail(BUFSZ);
        if avail == 0 {
            return false;
        }
        let Some(buf) = matcher.begin_bytes(avail) else { return false };

        // do not cut off the last UTF-8 sequence, ignore it, otherwise we risk failing the UTF-8 check
        if buf[avail - 1] & 0x80 == 0x80 {
            let mut n = avail.min(4); // note: 1 <= n <= 4 bytes to check
            while n > 0 {
                avail -= 1;
                if buf[avail] & 0xc0 != 0x80 {
                    break;
                }
                n -= 1;
            }
            if buf[avail] & 0xc0 != 0xc0 {
                return true;
            }
        }

        ctx.with_host(|h| h.is_binary(&buf[..avail]))
    }

    /// `Grep::output_or_save_restline(invert, hex_context)`.
    pub fn output_or_save_restline(ctx: &SearchContext, matcher: &MatcherHandle, invert: bool, hex_context: bool) {
        if ctx.with_host(|h| h.restline_data()) != RestlineData::None {
            if ctx.flags().ungroup || ctx.with_host(|h| h.lineno()) != matcher.lineno() {
                output_restline(ctx, matcher, invert, hex_context);
            } else {
                save_restline(ctx);
            }
        }
    }

    /// `Grep::output_restline(invert, hex_context)`.
    pub fn output_restline(ctx: &SearchContext, matcher: &MatcherHandle, invert: bool, hex_context: bool) {
        if ctx.with_host(|h| h.restline_data()) == RestlineData::None {
            return;
        }
        if ctx.with_host(|h| h.binary()) {
            // --hexdump with ABC context: if binary then show hex context
            if hex_context {
                let (after, columns) = {
                    let f = ctx.flags();
                    (f.hex_after, f.hex_columns)
                };
                let (last, size) = ctx.with_host(|h| (h.restline_last(), h.restline_size()));
                let mut right = after
                    .wrapping_mul(columns)
                    .wrapping_sub(last.wrapping_sub(1).checked_rem(columns).unwrap_or(0))
                    .wrapping_sub(1);
                let first = matcher.first();
                if last.wrapping_add(right) > first {
                    right = first.wrapping_sub(last);
                }
                if right < size {
                    ctx.with_host(|h| h.set_restline_size(right));
                }
            }
            ctx.with_host(|h| h.out_dump_hex(if invert { HEX_CONTEXT_LINE } else { HEX_LINE }, h.restline_last(), &h.restline_bytes()));
        } else {
            let (color, off) = {
                let f = ctx.flags();
                (if invert { f.color_cx.clone() } else { f.color_sl.clone() }, f.color_off.clone())
            };
            ctx.with_host(|h| {
                let mut lf_only = false;
                let mut bytes = h.restline_bytes();
                if let Some(&last) = bytes.last() {
                    lf_only = last == b'\n';
                    bytes.truncate(bytes.len() - usize::from(lf_only));
                    h.set_restline_size(bytes.len());
                    if !bytes.is_empty() {
                        h.out_str(&color);
                        h.out_str(&bytes);
                        h.out_str(&off);
                    }
                }
                h.out_nl(lf_only);
            });
        }
        ctx.with_host(|h| h.set_restline_data(RestlineData::None));
    }

    /// `Grep::save_restline()`.
    pub fn save_restline(ctx: &SearchContext) {
        ctx.with_host(|h| {
            let bytes = h.restline_bytes();
            h.restline_set(&bytes);
        });
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
        assert_eq!([CnfResult::NO.0, CnfResult::YES.0, CnfResult::EXIT.0], [0, 1, 2]);
        assert_eq!(CnfResult::EXIT.0, SearchStep::EXIT.0, "an exit from the CNF is a SearchStep::EXIT");
        assert_eq!([INPUT_HANDLER_NONE, INPUT_HANDLER_BINARY, INPUT_HANDLER_STDIN], [0, 1, 2]);
        assert_eq!([CnfBits::Matching as i32, CnfBits::NotMatching as i32], [0, 1]);
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
            offset_of!(MatcherTable, input),
            offset_of!(MatcherTable, buffer),
            offset_of!(MatcherTable, buffer_all),
            offset_of!(MatcherTable, avail),
            offset_of!(MatcherTable, begin_ptr),
            offset_of!(MatcherTable, set_input_handler),
            offset_of!(MatcherTable, find_in_line),
        ];
        for (i, o) in offsets.iter().enumerate() {
            assert_eq!(*o, i * w, "field {i}");
        }
        assert_eq!(size_of::<MatcherTable>(), 39 * w);
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
        extern "C" fn input(_: *mut c_void, _: *const c_void) {}
        extern "C" fn buffer(_: *mut c_void, _: *const u8, _: usize) {}
        extern "C" fn buffer_all(_: *mut c_void) -> c_int {
            1
        }
        extern "C" fn avail(ctx: *mut c_void, _: usize) -> usize {
            f(ctx).buf.len()
        }
        extern "C" fn begin_ptr(ctx: *mut c_void, len: usize) -> *const u8 {
            let m = f(ctx);
            if len <= m.buf.len() {
                m.buf.as_ptr()
            } else {
                std::ptr::null()
            }
        }
        extern "C" fn find_in_line(_: *mut c_void, _: *mut c_void, _: usize, _: usize) -> c_int {
            1
        }
        extern "C" fn set_input_handler(ctx: *mut c_void, kind: c_int) {
            f(ctx).calls.borrow_mut().push(if kind == 0 { "input_handler_none" } else { "input_handler" });
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
                input,
                buffer,
                buffer_all,
                avail,
                begin_ptr,
                set_input_handler,
                find_in_line,
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
        assert_eq!(h.avail(131072), 4);
        assert_eq!(h.begin_bytes(3), Some(b"abc".to_vec()));
        assert_eq!(h.begin_bytes(5), None);
        assert!(h.buffer_all());
        let data = b"xyz";
        h.buffer(&raw::static_buffer(data));
        assert!(h.find_in_line(&h, 0, 3));
        h.set_input_handler(INPUT_HANDLER_BINARY);
        h.set_input_handler(INPUT_HANDLER_NONE);
        assert_eq!(*m.calls.borrow(), vec!["find", "find", "find", "input_handler", "input_handler_none"]);
    }

    /// A fake `Grep`: the files are made-up addresses (1 is `stdin`, 2 the TUI `Static::source`), every host call that
    /// matters to a file member is logged, the output goes to a byte string.
    mod host {
        use super::super::*;
        use std::cell::RefCell;

        pub const STDIN: usize = 1;
        pub const SOURCE: usize = 2;
        pub const OPENED: usize = 3;
        pub const PIPE: usize = 9;

        pub struct St {
            pub log: Vec<String>,
            pub file_in: Option<usize>,
            pub input_file: Option<usize>,
            pub has_source: bool,
            pub open_fails: bool,
            pub spawn_fails: bool,
            pub zthread_ok: bool,
            pub next_part: bool,
            pub interactive: bool,
            pub probe: bool,
            pub drain: bool,
            pub drain_chunks: usize,
            pub binary_content: bool,
            pub is_binary_seen: Vec<Vec<u8>>,
            pub filter: Vec<u8>,
            pub labels: Vec<Vec<u8>>,
            pub magic: usize,
            pub eof: bool,
            pub holding: bool,
            pub found_part: bool,
            pub lineno: usize,
            pub binary: bool,
            pub binfile: bool,
            pub restline_data: RestlineData,
            pub restline_size: usize,
            pub restline_last: usize,
            pub buffer: Vec<u8>,
            pub saved: Vec<u8>,
            pub out: Vec<u8>,
            pub alts: Vec<Vec<bool>>,
            pub matching: Vec<bool>,
        }

        pub struct TestHost {
            pub st: std::rc::Rc<RefCell<St>>,
            pub matcher: Rc<MatcherHandle>,
        }

        pub fn state() -> std::rc::Rc<RefCell<St>> {
            std::rc::Rc::new(RefCell::new(St {
                log: Vec::new(),
                file_in: None,
                input_file: None,
                has_source: true,
                open_fails: false,
                spawn_fails: false,
                zthread_ok: true,
                next_part: false,
                interactive: false,
                probe: false,
                drain: false,
                drain_chunks: 0,
                binary_content: false,
                is_binary_seen: Vec::new(),
                filter: Vec::new(),
                labels: Vec::new(),
                magic: 0,
                eof: false,
                holding: false,
                found_part: true,
                lineno: 0,
                binary: false,
                binfile: true,
                restline_data: RestlineData::None,
                restline_size: 0,
                restline_last: 0,
                buffer: Vec::new(),
                saved: Vec::new(),
                out: Vec::new(),
                alts: Vec::new(),
                matching: Vec::new(),
            }))
        }

        impl TestHost {
            fn log(&self, what: &str) {
                self.st.borrow_mut().log.push(what.to_string());
            }
            fn file(addr: usize) -> FileRef<'static> {
                raw::fake::file_ref(addr)
            }
            fn addr(file: &FileRef<'_>) -> usize {
                raw::file_ptr(file) as usize
            }
        }

        impl GrepHost for TestHost {
            fn matches(&self) -> usize {
                0
            }
            fn set_matches(&self, _: usize) {
                self.log("set_matches");
            }
            fn lineno(&self) -> usize {
                self.st.borrow().lineno
            }
            fn set_lineno(&self, lineno: usize) {
                self.st.borrow_mut().lineno = lineno;
            }
            fn heading(&self) -> bool {
                false
            }
            fn set_heading(&self, _: bool) {
                self.log("set_heading");
            }
            fn stop(&self) -> bool {
                false
            }
            fn set_stop(&self, _: bool) {
                self.log("set_stop");
            }
            fn binfile(&self) -> bool {
                self.st.borrow().binfile
            }
            fn set_binfile(&self, binfile: bool) {
                self.st.borrow_mut().binfile = binfile;
            }
            fn hex(&self) -> bool {
                false
            }
            fn set_hex(&self, _: bool) {
                self.log("set_hex");
            }
            fn binary(&self) -> bool {
                self.st.borrow().binary
            }
            fn set_binary(&self, binary: bool) {
                self.st.borrow_mut().binary = binary;
            }
            fn partname(&self) -> Bytes {
                Bytes::new()
            }
            fn restline_size(&self) -> usize {
                self.st.borrow().restline_size
            }
            fn set_restline_size(&self, size: usize) {
                self.st.borrow_mut().restline_size = size;
            }
            fn restline_last(&self) -> usize {
                self.st.borrow().restline_last
            }
            fn set_restline_last(&self, last: usize) {
                self.st.borrow_mut().restline_last = last;
            }
            fn restline_data(&self) -> RestlineData {
                self.st.borrow().restline_data
            }
            fn set_restline_data(&self, data: RestlineData) {
                self.st.borrow_mut().restline_data = data;
            }
            fn restline_advance(&self, _: usize) {
                self.log("restline_advance");
            }
            fn restline_bytes(&self) -> Bytes {
                let st = self.st.borrow();
                match st.restline_data {
                    RestlineData::None => Bytes::new(),
                    RestlineData::Buffer(off) => st.buffer[off..off + st.restline_size].to_vec(),
                    RestlineData::Saved(off) => st.saved[off..off + st.restline_size].to_vec(),
                }
            }
            fn restline_set(&self, bytes: &[u8]) {
                let mut st = self.st.borrow_mut();
                st.saved = bytes.to_vec();
                st.restline_data = RestlineData::Saved(0);
            }
            fn restline_output(&self, _: bool, _: bool) {
                self.log("host restline_output (must not be called by file::*)");
            }
            fn restline_output_or_save(&self, _: bool, _: bool) {
                self.log("host restline_output_or_save (must not be called by file::*)");
            }
            fn restline_save(&self) {
                self.log("host restline_save (must not be called by file::*)");
            }
            fn handler_output_before_context(&self) {}
            fn handler_set_after_lineno(&self, _: usize) {}
            fn handler_add_before_context_line(&self, _: usize, _: usize, _: usize, _: usize) {}
            fn handler_add_before_context_match(&self, _: usize, _: usize, _: usize) {}
            fn cnf_matching(&self, _: usize, _: usize, _: bool) -> CnfResult {
                self.log("host cnf_matching (must not be called by file::*)");
                CnfResult::NO
            }
            fn cnf_satisfied(&self, _: bool) -> CnfResult {
                self.log("host cnf_satisfied (must not be called by file::*)");
                CnfResult::NO
            }
            fn out_launch(&self) {
                self.log("launch");
            }
            fn cnf_alts(&self, term: usize) -> usize {
                self.st.borrow().alts.get(term).map_or(0, Vec::len)
            }
            fn cnf_alt_matcher(&self, term: usize, alt: usize) -> Option<Rc<MatcherHandle>> {
                let present = self.st.borrow().alts.get(term).and_then(|a| a.get(alt)).copied().unwrap_or(false);
                present.then(|| Rc::clone(&self.matcher))
            }
            fn cnf_bit_get(&self, which: CnfBits, k: usize, _: usize) -> bool {
                which == CnfBits::Matching && self.st.borrow().matching.get(k).copied().unwrap_or(false)
            }
            fn cnf_bit_set(&self, which: CnfBits, k: usize, _: usize) {
                if which == CnfBits::Matching {
                    self.st.borrow_mut().matching[k] = true;
                }
            }
            fn error_pending(&self) -> bool {
                false
            }
            fn filename(&self) -> Option<Bytes> {
                None
            }
            fn interactive(&self) -> bool {
                self.st.borrow().interactive
            }
            fn set_interactive(&self, interactive: bool) {
                self.st.borrow_mut().interactive = interactive;
            }
            fn file_in(&self) -> Option<FileRef<'_>> {
                self.st.borrow().file_in.map(Self::file)
            }
            fn set_file_in(&mut self, file: Option<FileSource>) {
                let addr = file.map(|f| match f {
                    FileSource::Stdin => STDIN,
                    FileSource::Source => SOURCE,
                    FileSource::Owned(f) => raw::file_into_ptr(f) as usize,
                });
                self.st.borrow_mut().file_in = addr;
            }
            fn close_input_file(&mut self) {
                self.log("close_input_file");
                self.st.borrow_mut().input_file = None;
            }
            fn close_file_in(&mut self) {
                self.log("close_file_in");
                self.st.borrow_mut().file_in = None;
            }
            fn input(&self) -> InputHandle {
                raw::fake::input_handle()
            }
            fn input_file(&self) -> Option<FileRef<'_>> {
                self.st.borrow().input_file.map(Self::file)
            }
            fn input_encoding(&self) -> i32 {
                0
            }
            fn input_set_file(&mut self, _: FileSource, _: i32) {
                self.log("input_set_file");
            }
            fn input_set_file_in(&mut self, encoding: i32) {
                self.log(&format!("input_set_file_in {encoding}"));
                let file_in = self.st.borrow().file_in;
                self.st.borrow_mut().input_file = file_in;
            }
            fn input_clear(&mut self) {
                self.log("input_clear");
            }
            fn input_get(&self, _: &mut [u8]) -> usize {
                let mut st = self.st.borrow_mut();
                st.log.push("input_get".to_string());
                if st.drain_chunks > 0 {
                    st.drain_chunks -= 1;
                    16384
                } else {
                    0
                }
            }
            fn source(&self) -> Option<FileRef<'_>> {
                self.st.borrow().has_source.then(|| Self::file(SOURCE))
            }
            fn stdin_file(&self) -> FileRef<'_> {
                Self::file(STDIN)
            }
            fn encoding_null_data(&self) -> i32 {
                99
            }
            fn flag_filter(&self) -> Bytes {
                self.st.borrow().filter.clone()
            }
            fn flag_filter_magic_label(&self, index: usize) -> Option<Bytes> {
                self.st.borrow().labels.get(index).cloned()
            }
            fn filter_magic_scan(&self, _: &FileRef<'_>) -> usize {
                self.log("magic_scan");
                self.st.borrow().magic
            }
            fn warning(&self, message: &[u8], arg: &[u8]) {
                self.log(&format!("warning {} {}", String::from_utf8_lossy(message), String::from_utf8_lossy(arg)));
            }
            fn file_open(&self, pathname: &[u8]) -> Option<OwnedFile> {
                self.log(&format!("open {}", String::from_utf8_lossy(pathname)));
                if self.st.borrow().open_fails {
                    self.log("warning cannot read");
                    None
                } else {
                    Some(raw::fake::owned_file(OPENED))
                }
            }
            fn file_close(&self, _: OwnedFile) {
                self.log("file_close");
            }
            fn file_read_error(&self, file: &FileRef<'_>, _: &[u8]) -> bool {
                self.log(&format!("read_error {}", Self::addr(file)));
                false
            }
            fn file_set_binary(&self, _: &FileRef<'_>) {
                self.log("set_binary");
            }
            fn file_nonblock_regular(&self, file: &FileRef<'_>) {
                self.log(&format!("nonblock {}", Self::addr(file)));
            }
            fn file_probe_interactive(&self, _: &FileRef<'_>) -> bool {
                self.st.borrow().probe
            }
            fn file_seek_set(&self, _: &FileRef<'_>) -> bool {
                true
            }
            fn stdin_drain_check(&self) -> bool {
                self.st.borrow().drain
            }
            fn filter_spawn(&self, file: FileSource, pathname: &[u8], command: &[u8]) -> Option<OwnedFile> {
                let kind = match file {
                    FileSource::Stdin => "stdin".to_string(),
                    FileSource::Source => "source".to_string(),
                    FileSource::Owned(f) => format!("owned {}", raw::file_into_ptr(f) as usize),
                };
                self.log(&format!("spawn {kind} {} {}", String::from_utf8_lossy(pathname), String::from_utf8_lossy(command)));
                (!self.st.borrow().spawn_fails).then(|| raw::fake::owned_file(PIPE))
            }
            fn is_binary(&self, bytes: &[u8]) -> bool {
                let mut st = self.st.borrow_mut();
                st.is_binary_seen.push(bytes.to_vec());
                st.binary_content
            }
            fn zthread_start(&mut self, _: usize, pathname: &[u8], find: Option<&[u8]>, _: i32) -> bool {
                self.log(&format!("zthread_start {} {:?}", String::from_utf8_lossy(pathname), find.map(<[u8]>::to_vec)));
                self.st.borrow().zthread_ok
            }
            fn zthread_open_next(&mut self, _: &[u8], _: i32) -> bool {
                self.log("zthread_open_next");
                self.st.borrow().next_part
            }
            fn zthread_cancel(&self) {
                self.log("zthread_cancel");
            }
            fn matchers_len(&self) -> Option<usize> {
                Some(self.st.borrow().alts.len())
            }
            fn matchers_matcher(&self, index: usize) -> Option<Rc<MatcherHandle>> {
                self.cnf_alt_matcher(index, 0)
            }
            fn matchers_reset(&self) {}
            fn out_eof(&self) -> bool {
                self.st.borrow().eof
            }
            fn out_hold(&self) {}
            fn out_holding(&self) -> bool {
                self.st.borrow().holding
            }
            fn out_acquire(&self) {
                self.log("acquire");
            }
            fn out_release(&self) {}
            fn out_check_flush(&self) {}
            fn out_chr(&self, c: u8) {
                self.st.borrow_mut().out.push(c);
            }
            fn out_str(&self, s: &[u8]) {
                self.st.borrow_mut().out.extend_from_slice(s);
            }
            fn out_utf8strn(&self, _: &[u8], _: usize) {}
            fn out_num(&self, _: usize, _: i32) {}
            fn out_nl(&self, lf_only: bool) {
                self.st.borrow_mut().out.extend_from_slice(if lf_only { b"<LF>" } else { b"<CRLF>" });
            }
            fn out_header(&self, _: usize, _: usize, _: &[u8], _: bool) {}
            fn out_header_no_matcher(&self, _: usize, _: usize, _: &[u8]) {}
            fn out_header_file(&self) {}
            fn out_binary_file_matches(&self) {}
            fn out_format_count(&self, _: &[u8], _: usize) {}
            fn out_format(&self, _: &[u8], _: usize, _: Option<&mut usize>, _: bool, _: bool) -> bool {
                false
            }
            fn out_dump_hex(&self, mode: i16, byte_offset: usize, data: &[u8]) {
                let line = format!("hex({mode},{byte_offset},{})", String::from_utf8_lossy(data));
                self.st.borrow_mut().out.extend_from_slice(line.as_bytes());
            }
            fn out_dump_next(&self, _: usize) {}
            fn out_dump_done(&self) {}
            fn stats_found_file(&self) -> bool {
                true
            }
            fn stats_found_part(&self) -> bool {
                self.st.borrow().found_part
            }
            fn stats_undo_found_part(&self) {}
            fn stats_found_parts(&self) -> usize {
                0
            }
            fn stats_score_matches(&self, _: usize, _: usize) {}
        }
    }

    /// A context over a fake `Grep` and a fake matcher over `buf`.
    fn file_ctx(buf: &[u8], flags: GrepFlags) -> (SearchContext, std::rc::Rc<RefCell<host::St>>, Rc<MatcherHandle>, Box<fake::Fake>) {
        let mut m = fake();
        m.buf = buf.to_vec();
        let matcher = Rc::new(raw::fake::handle(fake::table(&m, MatcherKind::REFLEX)));
        let st = host::state();
        st.borrow_mut().buffer = buf.to_vec();
        let ctx = SearchContext::new(b"", 0);
        ctx.set_flags(flags);
        ctx.set_host(Box::new(host::TestHost { st: st.clone(), matcher: matcher.clone() }));
        (ctx, st, matcher, m)
    }

    fn log_of(st: &std::rc::Rc<RefCell<host::St>>) -> Vec<String> {
        st.borrow().log.clone()
    }

    #[test]
    fn open_file_opens_and_assigns_the_input() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags { encoding_type: 7, ..GrepFlags::default() });
        assert!(file::open_file(&ctx, b"a.txt", None, false));
        let log = log_of(&st);
        assert_eq!(log, vec!["open a.txt", "nonblock 3", "input_set_file_in 7"]);
        assert_eq!(st.borrow().file_in, Some(host::OPENED));
        assert!(!st.borrow().interactive);
    }

    #[test]
    fn open_file_failure_leaves_no_file() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags::default());
        st.borrow_mut().open_fails = true;
        assert!(!file::open_file(&ctx, b"missing", None, false));
        assert_eq!(log_of(&st), vec!["open missing", "warning cannot read"]);
        assert_eq!(st.borrow().file_in, None);
    }

    #[test]
    fn open_file_standard_input_uses_the_source_and_label() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags { label: Some(b"(stdin)".to_vec()), devices_action: 2, ..GrepFlags::default() });
        assert!(file::open_file(&ctx, b"-", None, true));
        assert_eq!(log_of(&st), vec!["set_binary", "input_set_file_in 0"]);
        assert_eq!(st.borrow().file_in, Some(host::SOURCE));
        st.borrow_mut().has_source = false;
        assert!(!file::open_file(&ctx, b"-", None, true));
    }

    #[test]
    fn open_file_probes_interactive_unless_quiet_or_decompressing() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags { devices_action: 2, ..GrepFlags::default() });
        st.borrow_mut().probe = true;
        assert!(file::open_file(&ctx, b"p", None, false));
        assert_eq!(st.borrow().interactive, cfg!(not(windows)));
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags { devices_action: 2, quiet: true, ..GrepFlags::default() });
        st.borrow_mut().probe = true;
        assert!(file::open_file(&ctx, b"p", None, false));
        assert!(!st.borrow().interactive);
    }

    #[test]
    fn open_file_decompress_starts_the_thread_and_closes_on_failure() {
        let flags = GrepFlags { decompress: true, zmax: 3, devices_action: 2, ..GrepFlags::default() };
        let (ctx, st, _m, _f) = file_ctx(b"", flags);
        assert!(file::open_file(&ctx, b"a.gz", Some(b"x"), false));
        assert_eq!(log_of(&st), vec!["open a.gz", "zthread_start a.gz Some([120])"]);
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags { decompress: true, devices_action: 2, ..GrepFlags::default() });
        st.borrow_mut().zthread_ok = false;
        assert!(!file::open_file(&ctx, b"a.gz", None, false));
        assert_eq!(log_of(&st), vec!["open a.gz", "zthread_start a.gz None", "close_file_in"]);
        assert_eq!(st.borrow().file_in, None);
    }

    fn filter_spawns(filter: &[u8], pathname: &[u8], labels: &[&[u8]], magic: usize) -> Vec<String> {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags::default());
        {
            let mut s = st.borrow_mut();
            s.file_in = Some(host::OPENED);
            s.filter = filter.to_vec();
            s.labels = labels.iter().map(|l| l.to_vec()).collect();
            s.magic = magic;
        }
        assert!(file::filter(&ctx, pathname));
        let log = log_of(&st);
        assert_eq!(st.borrow().file_in, if log.iter().any(|l| l.starts_with("spawn")) { Some(host::PIPE) } else { Some(host::OPENED) });
        log
    }

    #[test]
    fn filter_picks_the_command_of_the_suffix() {
        let log = filter_spawns(b"pdf:pdftotext % -, DOC:cat", &[&b"dir"[..], &[PATH_SEP][..], &b"x.Pdf"[..]].concat(), &[], 0);
        assert_eq!(log.len(), 1);
        assert!(log[0].starts_with("spawn owned 3 dir") && log[0].ends_with("x.Pdf pdftotext % -, DOC:cat"), "{log:?}");
        let log = filter_spawns(b"pdf:a,doc:b", b"x.doc", &[], 0);
        assert_eq!(log, vec!["spawn owned 3 x.doc b"]);
        // a leading dot is no suffix: the wildcard command applies
        let log = filter_spawns(b"txt:a,*:wild", b".bashrc", &[], 0);
        assert_eq!(log, vec!["spawn owned 3 .bashrc wild"]);
        let log = filter_spawns(b"txt:a, *:wild", b"noext", &[], 0);
        assert_eq!(log, vec!["spawn owned 3 noext wild"]);
        // no match and no wildcard: unchanged
        assert!(filter_spawns(b"txt:a", b"x.rs", &[], 0).is_empty());
        // an entry that lists suffixes only, without a command
        assert!(filter_spawns(b"rs", b"x.rs", &[], 0).is_empty());
    }

    #[test]
    fn filter_magic_label_supplies_the_suffix() {
        let log = filter_spawns(b"pdf:p,*:wild", b"noext", &[b"pdf", b"+doc"], 1);
        assert_eq!(log, vec!["magic_scan",         "spawn owned 3 noext p,*:wild"]);
        // + overrides a file name suffix
        let log = filter_spawns(b"doc:d,txt:t", b"x.txt", &[b"pdf", b"+doc"], 2);
                assert_eq!(log, vec!["magic_scan", "spawn owned 3 x.txt d,txt:t"]);
        // without +, a suffix is trusted
        let log = filter_spawns(b"doc:d,txt:t", b"x.txt", &[b"pdf", b"doc"], 2);
        assert_eq!(log, vec!["spawn owned 3 x.txt t"]);
        // no magic match
        let log = filter_spawns(b"pdf:p,*:wild", b"noext", &[b"pdf"], 0);
        assert_eq!(log, vec!["magic_scan", "spawn owned 3 noext wild"]);
    }

    #[test]
    fn filter_spawn_failure_clears_file_in() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags::default());
        {
            let mut s = st.borrow_mut();
            s.file_in = Some(host::STDIN);
            s.filter = b"*:cmd".to_vec();
            s.spawn_fails = true;
        }
        assert!(!file::filter(&ctx, b"x"));
        assert_eq!(log_of(&st), vec!["spawn stdin x cmd"]);
        assert_eq!(st.borrow().file_in, None);
    }

    #[test]
    fn close_file_closes_in_the_documented_order() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags::default());
        st.borrow_mut().file_in = Some(host::OPENED);
        assert!(!file::close_file(&ctx, b"a"));
        assert_eq!(log_of(&st), vec!["read_error 3", "close_file_in"]);

        // stdin is drained, not checked for errors
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags::default());
        {
            let mut s = st.borrow_mut();
            s.file_in = Some(host::STDIN);
            s.drain = true;
            s.drain_chunks = 2;
        }
        assert!(!file::close_file(&ctx, b"-"));
        assert_eq!(log_of(&st), vec!["input_get", "input_get", "input_get", "close_file_in"]);

        // not with -q
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags { quiet: true, ..GrepFlags::default() });
        {
            let mut s = st.borrow_mut();
            s.file_in = Some(host::STDIN);
            s.drain = true;
        }
        assert!(!file::close_file(&ctx, b"-"));
        assert_eq!(log_of(&st), vec!["close_file_in"]);
    }

    #[test]
    fn close_file_decompress_continues_with_the_next_part() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags { decompress: true, ..GrepFlags::default() });
        {
            let mut s = st.borrow_mut();
            s.file_in = Some(host::OPENED);
            s.next_part = true;
            s.eof = true;
        }
        assert!(file::close_file(&ctx, b"a.zip"));
        assert_eq!(log_of(&st), vec!["read_error 3", "close_input_file", "zthread_cancel", "zthread_open_next"]);
        st.borrow_mut().next_part = false;
        st.borrow_mut().log.clear();
        assert!(!file::close_file(&ctx, b"a.zip"));
        assert_eq!(log_of(&st).last().map(String::as_str), Some("close_file_in"));
    }

    #[test]
    fn init_is_binary_ignores_a_cut_utf8_sequence() {
        // "é" is c3 a9: cut after c3 the tail is ignored and the rest checked
        let (ctx, st, matcher, _f) = file_ctx(b"ab\xc3", GrepFlags::default());
        assert!(!file::init_is_binary(&ctx, &matcher));
        assert_eq!(st.borrow().is_binary_seen, vec![b"ab".to_vec()]);
        // a stray continuation byte at the end with no lead byte is binary
        let (ctx, _st, matcher, _f) = file_ctx(b"ab\x80\x80\x80\x80", GrepFlags::default());
        assert!(file::init_is_binary(&ctx, &matcher));
        // a complete sequence at the end is dropped from the check too, with its lead byte
        let (ctx, st, matcher, _f) = file_ctx(b"a\xc3\xa9", GrepFlags::default());
        st.borrow_mut().binary_content = true;
        assert!(file::init_is_binary(&ctx, &matcher));
        assert_eq!(st.borrow().is_binary_seen, vec![b"a".to_vec()]);
        // empty input
        let (ctx, _st, matcher, _f) = file_ctx(b"", GrepFlags::default());
        assert!(!file::init_is_binary(&ctx, &matcher));
    }

    #[test]
    fn init_read_sets_the_state_and_checks_for_binary() {
        let state = GrepState::new();
        let (ctx, st, matcher, f) = file_ctx(b"abc\n", GrepFlags { min_line: 3, ..GrepFlags::default() });
        st.borrow_mut().binary_content = false;
        assert!(file::init_read(&state, &ctx, &matcher));
        assert_eq!(st.borrow().lineno, 2);
        assert!(!st.borrow().binfile);
        assert_eq!(*f.calls.borrow(), vec!["input_handler"]);

        // -I with binary content skips the file
        let (ctx, st, matcher, _f) = file_ctx(b"abc\n", GrepFlags { binary_without_match: true, ..GrepFlags::default() });
        st.borrow_mut().binary_content = true;
        assert!(!file::init_read(&state, &ctx, &matcher));

        // -a and interactive input do not check
        let (ctx, st, matcher, f) = file_ctx(b"abc\n", GrepFlags { text: true, ..GrepFlags::default() });
        assert!(file::init_read(&state, &ctx, &matcher));
        assert!(st.borrow().is_binary_seen.is_empty());
        assert!(f.calls.borrow().is_empty());
        let (ctx, st, matcher, f) = file_ctx(b"abc\n", GrepFlags::default());
        st.borrow_mut().interactive = true;
        assert!(file::init_read(&state, &ctx, &matcher));
        assert!(st.borrow().is_binary_seen.is_empty());
        assert_eq!(f.calls.borrow().len(), usize::from(cfg!(not(windows))));
        assert_eq!(state.mapping(), None);
    }

    #[test]
    fn restline_is_output_or_saved() {
        let (ctx, st, matcher, _f) = file_ctx(b"rest of line\n", GrepFlags { color_sl: b"<sl>".to_vec(), color_cx: b"<cx>".to_vec(), color_off: b"</>".to_vec(), ..GrepFlags::default() });
        {
            let mut s = st.borrow_mut();
            s.restline_data = RestlineData::Buffer(5);
            s.restline_size = 8;
            s.lineno = 1; // the matcher is at line 1: the line is saved
        }
        file::output_or_save_restline(&ctx, &matcher, false, false);
        assert_eq!(st.borrow().restline_data, RestlineData::Saved(0));
        assert_eq!(st.borrow().saved, b"of line\n".to_vec());
        assert!(st.borrow().out.is_empty());

        // another line: output, the line feed is output as the end of line
        st.borrow_mut().lineno = 2;
        file::output_or_save_restline(&ctx, &matcher, true, false);
        assert_eq!(st.borrow().out, b"<cx>of line</><LF>".to_vec());
        assert_eq!(st.borrow().restline_data, RestlineData::None);
        assert_eq!(st.borrow().restline_size, 7);
        assert!(log_of(&st).is_empty(), "file::* must not call the host restline slots: {:?}", log_of(&st));

        // nothing to do without a restline
        file::output_or_save_restline(&ctx, &matcher, false, false);
        file::output_restline(&ctx, &matcher, false, false);
        assert_eq!(st.borrow().out, b"<cx>of line</><LF>".to_vec());
    }

    #[test]
    fn restline_without_text_only_ends_the_line() {
        let (ctx, st, matcher, _f) = file_ctx(b"\n", GrepFlags { ungroup: true, ..GrepFlags::default() });
        {
            let mut s = st.borrow_mut();
            s.restline_data = RestlineData::Buffer(0);
            s.restline_size = 1;
            s.lineno = 1;
        }
        file::output_or_save_restline(&ctx, &matcher, false, false);
        assert_eq!(st.borrow().out, b"<LF>".to_vec());
        assert_eq!(st.borrow().restline_size, 0);
    }

    #[test]
    fn restline_of_binary_is_a_hex_dump_with_context() {
        let flags = GrepFlags { hex_after: 1, hex_columns: 4, ..GrepFlags::default() };
        let (ctx, st, matcher, _f) = file_ctx(b"0123456789", flags);
        {
            let mut s = st.borrow_mut();
            s.binary = true;
            s.restline_data = RestlineData::Buffer(2);
            s.restline_size = 8;
            s.restline_last = 2;
        }
        // right = 1 * 4 - ((2 - 1) % 4) - 1 = 2, matcher.first() = 1 < last + right: right = first - last wraps
        file::output_restline(&ctx, &matcher, false, true);
        let out = String::from_utf8_lossy(&st.borrow().out).to_string();
        assert!(out.starts_with("hex(1,2,"), "{out}");
        let (ctx, st, matcher, _f) = file_ctx(b"0123456789", GrepFlags::default());
        {
            let mut s = st.borrow_mut();
            s.binary = true;
            s.restline_data = RestlineData::Buffer(2);
            s.restline_size = 3;
            s.restline_last = 2;
        }
        file::output_restline(&ctx, &matcher, true, false);
        assert_eq!(st.borrow().out, b"hex(3,2,234)".to_vec());
    }

    #[test]
    fn cnf_matching_checks_every_and_term() {
        let (ctx, st, matcher, _f) = file_ctx(b"abc\n", GrepFlags::default());
        // (alt0 or not alt1) and (alt0): the fake matcher always finds
        st.borrow_mut().alts = vec![vec![true, true], vec![true]];
        assert_eq!(file::cnf_matching(&ctx, &matcher, 0, 3, false), CnfResult::YES);
        // no positive alternative: the line must not match one of the not-terms: it does, so no
        st.borrow_mut().alts = vec![vec![false, true]];
        assert_eq!(file::cnf_matching(&ctx, &matcher, 0, 3, false), CnfResult::NO);
        st.borrow_mut().alts = vec![vec![false]];
        assert_eq!(file::cnf_matching(&ctx, &matcher, 0, 3, false), CnfResult::NO);
        assert!(log_of(&st).is_empty());
    }

    #[test]
    fn cnf_matching_with_files_launches_when_all_terms_matched() {
        let (ctx, st, matcher, _f) = file_ctx(b"abc\n", GrepFlags { files: true, ..GrepFlags::default() });
        {
            let mut s = st.borrow_mut();
            s.holding = true;
            s.alts = vec![vec![true], vec![true]];
            s.matching = vec![false, false];
        }
        assert_eq!(file::cnf_matching(&ctx, &matcher, 0, 3, true), CnfResult::YES);
        assert_eq!(st.borrow().matching, vec![true, true]);
        assert_eq!(log_of(&st), vec!["acquire", "launch"]);

        // --max-files reached: exit
        st.borrow_mut().log.clear();
        st.borrow_mut().found_part = false;
        assert_eq!(file::cnf_matching(&ctx, &matcher, 0, 3, false), CnfResult::EXIT);

        // a term that only has not-terms, all of which matched: stop searching this file
        let (ctx, st, matcher, _f) = file_ctx(b"abc\n", GrepFlags { files: true, ..GrepFlags::default() });
        {
            let mut s = st.borrow_mut();
            s.holding = true;
            s.alts = vec![vec![false]];
            s.matching = vec![false];
        }
        assert_eq!(file::cnf_matching(&ctx, &matcher, 0, 3, false), CnfResult::EXIT);
    }

    #[test]
    fn cnf_satisfied_needs_every_term_matched() {
        let (ctx, st, _m, _f) = file_ctx(b"", GrepFlags::default());
        // not holding: nothing to launch
        assert_eq!(file::cnf_satisfied(&ctx, false), CnfResult::YES);
        assert!(log_of(&st).is_empty());
        {
            let mut s = st.borrow_mut();
            s.holding = true;
            s.alts = vec![vec![true], vec![true]];
            s.matching = vec![true, false];
        }
        assert_eq!(file::cnf_satisfied(&ctx, true), CnfResult::NO);
        st.borrow_mut().matching = vec![true, true];
        assert_eq!(file::cnf_satisfied(&ctx, true), CnfResult::YES);
        assert_eq!(log_of(&st), vec!["acquire", "launch"]);
        st.borrow_mut().found_part = false;
        assert_eq!(file::cnf_satisfied(&ctx, false), CnfResult::EXIT);
    }

    #[test]
    fn save_restline_copies_the_rest_of_the_line() {
        let (ctx, st, _m, _f) = file_ctx(b"0123456789", GrepFlags::default());
        {
            let mut s = st.borrow_mut();
            s.restline_data = RestlineData::Buffer(4);
            s.restline_size = 3;
        }
        file::save_restline(&ctx);
        assert_eq!(st.borrow().saved, b"456".to_vec());
        assert_eq!(st.borrow().restline_data, RestlineData::Saved(0));
        assert_eq!(st.borrow().restline_size, 3);
    }

    const PATH_SEP: u8 = if cfg!(windows) { b'\\' } else { b'/' };

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
