//! Unit `indexer_walk`: src/ugrep-indexer.cpp lines 1050-1843 (trim, getline, import_globs, include_dir,
//! include_file, `cat`, `deleter`, `indexer`). One coarse seam call each: the whole tree walk, the index file
//! reads and writes and the progress output happen inside the call. Stub until the Port.

use super::{Exit, IndexerFlags};

/// Seam function `src_indexer.indexer` (`indexer()`): recursively index the tree at `path` (None is `.`).
/// `warnings` is the C++ counter at the call. `--check` ends the process: `Exit::exited` with the exit code.
pub fn indexer(_flags: &IndexerFlags, _path: Option<&[u8]>, _warnings: usize) -> Exit {
    std::process::abort()
}

/// Seam function `src_indexer.deleter` (`deleter()`): recursively delete the index files under `path`.
pub fn deleter(_flags: &IndexerFlags, _path: Option<&[u8]>, _warnings: usize) -> Exit {
    std::process::abort()
}
