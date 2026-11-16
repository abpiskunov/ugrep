//! Unit `indexer_cli`: src/ugrep-indexer.cpp lines 588-780 (version, help, usage, warning, error,
//! cannot_decompress, strtonum, strtopos) and 1844-2090 (`options`; `load_config` and `main` are retained C++).
//! Stub until the Port.

use super::{Bytes, IndexerFlags, OptionsResult};

/// Seam function `src_indexer.options` (`options()`): parse `args` (= argv[0..argc], `args[0]` is ignored) on top
/// of `flags`. `arg_path` is the PATH argument already taken (None if none). `--help`, `--version` and usage errors
/// end the process (`Exit::exited`) unless `flags.usage_warnings`, which counts a warning instead.
pub fn options(_flags: &IndexerFlags, _args: &[Bytes], _arg_path: Option<&[u8]>, _warnings: usize) -> OptionsResult {
    std::process::abort()
}
