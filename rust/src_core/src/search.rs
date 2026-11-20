//! Types and signatures of the `Grep` search seam of `src/ugrep.cpp` (Prepare seam: Grep search), fixed by the
//! Prepare seam task. Declared from `signatures.rs` with `#[path]`, so the replay harness that includes
//! `signatures.rs` alone builds it; it names no `cxx` type and uses no crate path. Every body is a stub that panics
//! until a Port task; the Port never edits a signature (the signature change protocol of the plan applies).
//!
//! Two things live here:
//! - [`MatcherHandle`]: the matcher-handle API (`find`/`next_match`, `bol`/`eol`/`begin`/`end`, text bounds, the
//!   buffer-shift event, `reset`). **Only RE-flex and PCRE2 matchers cross the seam** ([`MatcherKind`]): `reflex::Matcher`,
//!   `reflex::PCRE2Matcher` and `reflex::PCRE2UTFMatcher`. `reflex::FuzzyMatcher` (`-Z`), `reflex::LineMatcher` and
//!   `reflex::BoostPerlMatcher` stay C++: a search with one of them is not handed to a Rust branch.
//! - [`SearchContext`]: the Rust-owned counterpart of `Grep::SearchContext`, the locals of `Grep::search` that its
//!   branches share, and the one stub per branch in [`branch`] (named as the C++ `Grep::search_*` functions).
//!
//! Re-entrancy shapes the signatures: the C++ matcher calls the buffer-shift handler *while* `find`, `next_match` or
//! `eol(true)` runs, and the handler reads the `pathname` of the context. So every method takes `&self` (the matcher
//! lives in C++ and mutates there; a Rust `&mut` held across the call would be aliased by the event) and the branches
//! take `&SearchContext` (`matched` is a `Cell`; the handle and the context are confined to the searching thread).
//!
//! The notes (encoding, re-entrancy, thread affinity, owner side, error kinds) are in `SEAM.md`, "Grep seam".

use std::cell::Cell;

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
    /// `reflex::PCRE2UTFMatcher`.
    pub const PCRE2_UTF: MatcherKind = MatcherKind(3);
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

/// Handle of a matcher that stays owned by C++ (`Grep::matcher`, one element of `Grep::matchers`). The handle borrows it:
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

/// The locals of `Grep::search` that its branches share (`Grep::SearchContext`). Rust-owned; the C++ `Grep` members
/// (`matches`, `lineno`, `out`, `partname`, ...) are not in it, see SEAM.md. Read by the shift handler while a branch
/// runs, so branches get it by shared reference.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchContext {
    /// `pathname` after `flag_label` replaced the standard input label. Bytes, no NUL.
    pub pathname: Bytes,
    /// The `cost` argument of `search`, used before the branches only.
    pub cost: u16,
    /// This file or archive has a match (set by the epilogue).
    pub matched: Cell<bool>,
}

impl SearchContext {
    /// A context for `search(pathname, cost)`; `matched` starts false.
    pub fn new(pathname: &[u8], cost: u16) -> SearchContext {
        SearchContext { pathname: pathname.to_vec(), cost, matched: Cell::new(false) }
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
