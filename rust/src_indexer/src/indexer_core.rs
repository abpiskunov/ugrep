//! Unit `indexer_core`: src/ugrep-indexer.cpp lines 1-1050 (Stream, noise_percentage, is_binary, indexhash, `index`).
//! `index` is ONE coarse seam call per file (or per archive part): reading, the 8 staggered Bloom filters and the
//! table compression all run on the Rust side of the call. Stub until the Port.

use super::{Bytes, IndexerFlags, Indexed};

/// Seam function `src_indexer.index` (`index()` of src/ugrep-indexer.cpp).
/// `handle` is the stream state of the previous call for the same file (0 for a new file), `archive` is the
/// in/out flag of `index()`. Reads flags: `accuracy`, `decompress`, `hidden`, `ignore_binary`, `zmax`.
pub fn index(_flags: &IndexerFlags, _handle: u64, _pathname: &[u8], _archive: bool) -> Indexed {
    std::process::abort()
}

/// Releases the stream state behind `handle`; called by `~Stream` when a handle is still held.
pub fn stream_drop(_handle: u64) {
    std::process::abort()
}

/// `noise_percentage()`: noise level in percent for accuracy 0 to 9. Internal to the unit, not a seam function.
pub fn noise_percentage(_accuracy: i32) -> u32 {
    std::process::abort()
}

/// `is_binary()`: true if the bytes contain a NUL or an invalid UTF-8 sequence. Internal to the unit.
pub fn is_binary(_bytes: &[u8]) -> bool {
    std::process::abort()
}

/// The 65536-byte table `index()` fills, before compression. Internal to the unit.
pub fn new_table() -> Bytes {
    std::process::abort()
}
