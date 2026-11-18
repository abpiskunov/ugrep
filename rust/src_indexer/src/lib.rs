//! Rust side of the `src_indexer` component: the whole of ugrep-indexer (src/ugrep-indexer.cpp is only a `main`
//! that calls `ugrep_indexer_main`). The option state (`indexer_cli`), the index (`indexer_core`) and the tree walk
//! (`indexer_walk`) are the three units of `signatures`.

// public so every unit is part of the crate interface
pub mod signatures;

#[path = "ffi/entry.rs"]
mod entry;
