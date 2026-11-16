//! Types and signatures of the seam of `src_indexer` (src/ugrep-indexer.cpp), fixed by the Prepare seam task.
//! Every body is a stub that aborts until a Port task; the Port never edits a signature (the signature change
//! protocol of the plan applies). The replay harness includes this file alone, so it names no `cxx` type:
//! `lib.rs` converts between the bridge structs and the types here.
//!
//! Ownership decisions are in `SEAM.md`. The short form:
//! - `flag_*`, `warnings`, `arg_path` and the `Stream` objects of the C++ stay owned by C++ (`main` and
//!   `load_config` are retained). Rust sees the flags as a typed snapshot taken by the facade at the crossing
//!   (`IndexerFlags`) and never writes a flag; what a function changes comes back in its result.
//! - The stream state of `index` (open file, decompression chain, archive cursor) is owned by Rust behind a `u64`
//!   handle the C++ `Stream` carries; 0 means none.
//! - Units: `indexer_core` (src/ugrep-indexer.cpp lines 1-1050), `indexer_walk` (1050-1843) and `indexer_cli`
//!   (588-780 and 1844-2090), one module each, reached through `#[path]` like the other crates.

/// Bytes of a C string without its NUL, or of a `std::string`. No encoding is assumed (UTF-8 is not validated).
pub type Bytes = Vec<u8>;

#[path = "indexer_cli.rs"]
pub mod indexer_cli;
#[path = "indexer_core.rs"]
pub mod indexer_core;
#[path = "indexer_walk.rs"]
pub mod indexer_walk;

/// The `flag_*` globals of src/ugrep-indexer.cpp as they are at the crossing. Read only.
/// Defaults of the C++: `accuracy` 4, `zmax` 1, everything else false/empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexerFlags {
    /// `-0` ... `-9`, `--accuracy`: 0 to 9
    pub accuracy: i32,
    /// `-c`, `--check`
    pub check: bool,
    /// `-z`, `--decompress`
    pub decompress: bool,
    /// `-d`, `--delete`
    pub delete: bool,
    /// `-S`, `--dereference-files`
    pub dereference_files: bool,
    /// `-f`, `--force`
    pub force: bool,
    /// `-.`, `--hidden`
    pub hidden: bool,
    /// `-I`, `--ignore-binary`
    pub ignore_binary: bool,
    /// `-s`, `--no-messages`
    pub no_messages: bool,
    /// `-q`, `--quiet`
    pub quiet: bool,
    /// internal flag: configuration file lines warn instead of exiting
    pub usage_warnings: bool,
    /// `-v`, `--verbose`
    pub verbose: bool,
    /// `--zmax`
    pub zmax: usize,
    /// `-X`, `--ignore-files`: file names in order, may repeat
    pub ignore_files: Vec<Bytes>,
}

/// How a seam call ended. `exited` is true when the C++ called `exit(exit_code)` inside the call (`--check`,
/// `--help`, `--version`, a usage error); the facade then runs `exit(exit_code)`. `warnings` is the value of the
/// C++ `warnings` counter after the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    pub exited: bool,
    pub exit_code: i32,
    pub warnings: usize,
}

/// Result of `indexer_core::index`: the out-parameters of `index()`. `hashes` has `hashes_size` bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Indexed {
    /// the return value of `index()`
    pub ok: bool,
    pub hashes: Bytes,
    pub noise: f32,
    pub compressed: bool,
    /// in/out of `index()`: true while parts of an archive remain
    pub archive: bool,
    pub binary: bool,
    pub size: u64,
    /// `Stream::partname` after the call (archive part name); the walk prints it with `-v`
    pub partname: Bytes,
    /// the stream handle after the call; 0 when the stream was closed (where the C++ calls `stream.close()`)
    pub handle: u64,
}

/// Result of `indexer_cli::options`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionsResult {
    /// all flags after parsing (the C++ writes them back to the globals)
    pub flags: IndexerFlags,
    /// index in `args` of the first non-option argument taken as PATH, -1 when `arg_path` is unchanged
    pub arg_path_index: i32,
    pub exit: Exit,
}
