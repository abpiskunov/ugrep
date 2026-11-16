//! Rust side of the `src_indexer` component (src/ugrep-indexer.cpp).
//! One bridge module (`ffi`); all boundary code is here. The bodies are stubs (they abort) until the Port tasks.
//!
//! Only `extern "Rust"` functions: the C++ facade (src/ugrep-indexer_facade.cpp) calls Rust, Rust never calls the
//! component's legacy C++. State stays in C++ (see SEAM.md): the 13 scalar `flag_*` values and `flag_ignore_files`
//! cross by value as a snapshot, and what a function changes comes back in its result.

// public so the signatures that are not called yet (stubs until the Port tasks) are part of the crate interface
pub mod signatures;

use signatures as sig;

#[cxx::bridge(namespace = "ugrep_rust")]
mod ffi {
    /// Bytes of a string, no NUL, no encoding assumed.
    struct IndexerString {
        data: Vec<u8>,
    }

    /// Snapshot of the `flag_*` globals of src/ugrep-indexer.cpp (`signatures::IndexerFlags`).
    /// `delete_indexes` is `flag_delete`, named so because `delete` is a C++ keyword.
    struct IndexerFlags {
        accuracy: i32,
        check: bool,
        decompress: bool,
        delete_indexes: bool,
        dereference_files: bool,
        force: bool,
        hidden: bool,
        ignore_binary: bool,
        no_messages: bool,
        quiet: bool,
        usage_warnings: bool,
        verbose: bool,
        zmax: usize,
        ignore_files: Vec<IndexerString>,
    }

    /// How a call ended (`signatures::Exit`): `exited` means the C++ `exit(exit_code)` must run in the facade.
    struct IndexerExit {
        exited: bool,
        exit_code: i32,
        warnings: usize,
    }

    /// Result of `index` (`signatures::Indexed`). `hashes` holds the first `hashes_size` bytes of the table.
    struct IndexerIndexed {
        ok: bool,
        hashes: Vec<u8>,
        noise: f32,
        compressed: bool,
        archive: bool,
        binary: bool,
        size: u64,
        partname: Vec<u8>,
        handle: u64,
    }

    /// Result of `options` (`signatures::OptionsResult`).
    struct IndexerOptions {
        flags: IndexerFlags,
        arg_path_index: i32,
        exit: IndexerExit,
    }

    extern "Rust" {
        /// Seam function `src_indexer.index` (unit indexer_core, `index()`).
        fn indexer_index(flags: IndexerFlags, handle: u64, pathname: &[u8], archive: bool) -> IndexerIndexed;
        /// Releases the Rust stream state of `handle` (called by `~Stream`, never with 0).
        fn indexer_stream_drop(handle: u64);
        /// Seam function `src_indexer.indexer` (unit indexer_walk, `indexer()`).
        fn indexer_run(flags: IndexerFlags, path: &[u8], has_path: bool, warnings: usize) -> IndexerExit;
        /// Seam function `src_indexer.deleter` (unit indexer_walk, `deleter()`).
        fn indexer_delete(flags: IndexerFlags, path: &[u8], has_path: bool, warnings: usize) -> IndexerExit;
        /// Seam function `src_indexer.options` (unit indexer_cli, `options()`); `args` is argv[0..argc].
        fn indexer_options(
            flags: IndexerFlags,
            args: Vec<IndexerString>,
            arg_path: &[u8],
            has_arg_path: bool,
            warnings: usize,
        ) -> IndexerOptions;
    }
}

fn flags_in(f: ffi::IndexerFlags) -> sig::IndexerFlags {
    sig::IndexerFlags {
        accuracy: f.accuracy,
        check: f.check,
        decompress: f.decompress,
        delete: f.delete_indexes,
        dereference_files: f.dereference_files,
        force: f.force,
        hidden: f.hidden,
        ignore_binary: f.ignore_binary,
        no_messages: f.no_messages,
        quiet: f.quiet,
        usage_warnings: f.usage_warnings,
        verbose: f.verbose,
        zmax: f.zmax,
        ignore_files: f.ignore_files.into_iter().map(|s| s.data).collect(),
    }
}

fn flags_out(f: sig::IndexerFlags) -> ffi::IndexerFlags {
    ffi::IndexerFlags {
        accuracy: f.accuracy,
        check: f.check,
        decompress: f.decompress,
        delete_indexes: f.delete,
        dereference_files: f.dereference_files,
        force: f.force,
        hidden: f.hidden,
        ignore_binary: f.ignore_binary,
        no_messages: f.no_messages,
        quiet: f.quiet,
        usage_warnings: f.usage_warnings,
        verbose: f.verbose,
        zmax: f.zmax,
        ignore_files: f.ignore_files.into_iter().map(|data| ffi::IndexerString { data }).collect(),
    }
}

fn exit_out(e: sig::Exit) -> ffi::IndexerExit {
    ffi::IndexerExit { exited: e.exited, exit_code: e.exit_code, warnings: e.warnings }
}

fn indexer_index(flags: ffi::IndexerFlags, handle: u64, pathname: &[u8], archive: bool) -> ffi::IndexerIndexed {
    let r = sig::indexer_core::index(&flags_in(flags), handle, pathname, archive);
    ffi::IndexerIndexed {
        ok: r.ok,
        hashes: r.hashes,
        noise: r.noise,
        compressed: r.compressed,
        archive: r.archive,
        binary: r.binary,
        size: r.size,
        partname: r.partname,
        handle: r.handle,
    }
}

fn indexer_stream_drop(handle: u64) {
    sig::indexer_core::stream_drop(handle)
}

fn indexer_run(flags: ffi::IndexerFlags, path: &[u8], has_path: bool, warnings: usize) -> ffi::IndexerExit {
    let path = if has_path { Some(path) } else { None };
    exit_out(sig::indexer_walk::indexer(&flags_in(flags), path, warnings))
}

fn indexer_delete(flags: ffi::IndexerFlags, path: &[u8], has_path: bool, warnings: usize) -> ffi::IndexerExit {
    let path = if has_path { Some(path) } else { None };
    exit_out(sig::indexer_walk::deleter(&flags_in(flags), path, warnings))
}

fn indexer_options(
    flags: ffi::IndexerFlags,
    args: Vec<ffi::IndexerString>,
    arg_path: &[u8],
    has_arg_path: bool,
    warnings: usize,
) -> ffi::IndexerOptions {
    let args: Vec<sig::Bytes> = args.into_iter().map(|s| s.data).collect();
    let arg_path = if has_arg_path { Some(arg_path) } else { None };
    let r = sig::indexer_cli::options(&flags_in(flags), &args, arg_path, warnings);
    ffi::IndexerOptions { flags: flags_out(r.flags), arg_path_index: r.arg_path_index, exit: exit_out(r.exit) }
}
