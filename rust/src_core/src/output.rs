//! Types-and-signatures module for the `output` seam (src/output.cpp, src/output.hpp). See rust/src_core/SEAM.md,
//! section Seam: output.
//!
//! The shared structs are declared in the cxx bridge (`lib.rs`, module `ffi`) and are the types of these signatures.
//! The bodies are stubs until the Port tasks (`panic!`; panic is abort in this workspace, so a call aborts the process). They are
//! only reachable when the C++ is built with `CPP_RUST_IMPL_OUTPUT=rust`.
//!
//! Shape of the seam: every function is a pure function of its inputs (except `--tree` headers, which stay in C++
//! because they take the Sync lock and update the process-wide `Output::Tree` state). C++ keeps the `Output` object (buffer,
//! `Sync`, hex-dump state, `FILE*`), builds a plain-data snapshot per call, and appends the returned bytes to its
//! buffer with `Output::str`. The Rust side reads the option state it already owns (`settings`, `color`).

use super::ffi::{
    OutputArgs, OutputBinaryResult, OutputCtx, OutputDumpLine, OutputFormatResult, OutputHeaderResult, OutputMatch,
};

fn not_ported(name: &str) -> ! {
    panic!("output seam: {name} is not ported yet (CPP_RUST_IMPL_OUTPUT=rust)")
}

/// `Output::quote(data, size)`: the text appended to the buffer.
pub fn quote(_data: &[u8]) -> Vec<u8> {
    not_ported("quote")
}

/// `Output::cpp(data, size)`.
pub fn cpp(_data: &[u8]) -> Vec<u8> {
    not_ported("cpp")
}

/// `Output::csv(data, size)`.
pub fn csv(_data: &[u8]) -> Vec<u8> {
    not_ported("csv")
}

/// `Output::hex(data, size)`.
pub fn hex(_data: &[u8]) -> Vec<u8> {
    not_ported("hex")
}

/// `Output::json(data, size)`.
pub fn json(_data: &[u8]) -> Vec<u8> {
    not_ported("json")
}

/// `Output::xml(data, size)`.
pub fn xml(_data: &[u8]) -> Vec<u8> {
    not_ported("xml")
}

/// `Output::color(arg)`: `arg` is the two-letter colour code, `None` (`has_arg` false) is `color_off`.
pub fn color(_has_arg: bool, _arg: &[u8]) -> Vec<u8> {
    not_ported("color")
}

/// `Output::Dump::line()`: one hex dump line from `bytes`/`prevb` (`-1` is an unused column).
pub fn dump_line(_bytes: &[i16], _prevb: &[i16], _pstar: bool, _offset: usize) -> OutputDumpLine {
    not_ported("dump_line")
}

/// `Output::header(pathname, partname, heading, lineno, matcher, byte_offset, separator, newline)`.
/// `window` is the matcher's line when the format needs it (see `OutputMatch`); `matcher.present` false is a NULL matcher; the separator is
/// `args.has_separator` and `separator`.
pub fn header_line(
    _pathname: &[u8],
    _partname: &[u8],
    _separator: &[u8],
    _window: &[u8],
    _args: &OutputArgs,
    _matcher: &OutputMatch,
    _ctx: &OutputCtx,
) -> OutputHeaderResult {
    not_ported("header_line")
}

/// `Output::header(pathname, partname)` (--files-with-matches, --count). Not used with `--tree`: the facade keeps
/// that branch in C++ (it takes the Sync lock and updates the process-wide `Output::Tree` state).
pub fn header_file(_pathname: &[u8], _partname: &[u8], _ctx: &OutputCtx) -> OutputHeaderResult {
    not_ported("header_file")
}

/// `Output::binary_file_matches(pathname, partname)`.
pub fn binary_file_matches(_pathname: &[u8], _partname: &[u8], _ctx: &OutputCtx) -> OutputBinaryResult {
    not_ported("binary_file_matches")
}

/// `Output::format(format, matches)` (--format-begin, --format-end).
pub fn format_count(_format: &[u8], _matches: usize, _ctx: &OutputCtx) -> Vec<u8> {
    not_ported("format_count")
}

/// `Output::format(format, pathname, partname, matches, matching, matcher, heading, body, next)`.
pub fn format_match(
    _format: &[u8],
    _pathname: &[u8],
    _partname: &[u8],
    _window: &[u8],
    _args: &OutputArgs,
    _matcher: &OutputMatch,
    _ctx: &OutputCtx,
) -> OutputFormatResult {
    not_ported("format_match")
}

/// `Output::format_invert(format, pathname, partname, matches, lineno, offset, ptr, size, heading, next)`:
/// `line` is `ptr[..size]`.
pub fn format_invert(
    _format: &[u8],
    _pathname: &[u8],
    _partname: &[u8],
    _line: &[u8],
    _args: &OutputArgs,
    _ctx: &OutputCtx,
) -> OutputFormatResult {
    not_ported("format_invert")
}