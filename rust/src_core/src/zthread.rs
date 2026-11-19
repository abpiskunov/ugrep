//! Types of the Zthread seam (the decompression thread of src/ugrep.cpp), fixed by the Prepare seam task: the
//! caller-side view (`Outcome`, `FilePtr`) and what stays in C++ and is called back (`Host`). Declared from
//! `signatures.rs` with `#[path]`, so the replay harness that includes `signatures.rs` alone builds it; it names no
//! `cxx` type and uses no crate path. The decompression thread itself is `zthread_core.rs`, which needs the zstream
//! core of `src_leaves`, so it is declared from `lib.rs` and not from here.
//!
//! The notes (encoding, re-entrancy, thread affinity, owner side, error kinds) are in `SEAM.md`, "Zthread seam".
//! Port changes to the Prepare seam signatures (decision D-zthread-signature): `Host` gained `cannot_decompress`,
//! `archive_found` and `zstream_config`, and the `Zthread` struct moved to `zthread_core.rs`.

use std::ffi::c_void;

/// A C `FILE*`, borrowed from the caller. Opaque here; `ffi/zthread_sys.rs` reads it.
pub type FilePtr = *mut c_void;

/// The "no descriptor" value.
pub const NO_FD: i32 = -1;

/// What a crossing hands back instead of a `FILE*`: the legacy functions return NULL on failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The read end of a new pipe as an open `FILE*` that the caller owns from now on and closes with `fclose`.
    Pipe(FilePtr),
    /// The legacy NULL: failure already reported through [`Host::warning`], or no next file in the archive.
    None,
}

/// What the C++ build of the zstream (`HAVE_LIB*`, `WITH_ZIP_CRC32`, `WITH_MAX_7ZIP_SIZE`) decides, as `src_leaves`
/// `Config`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZstreamSettings {
    pub codecs: u32,
    pub zip_crc32: bool,
    pub max_7zip_size: u64,
}

/// What stays in C++ and is called back; the facade of src/ugrep.cpp implements it. Bytes carry no NUL and no
/// encoding is assumed (file names are OS bytes).
pub trait Host: Send + Sync {
    /// `Grep::partname` (or the partname of the previous stage) becomes `name`; called before data enters a new pipe.
    fn set_partname(&self, name: &[u8]);
    /// `warning(message, arg)` of ugrep.hpp, `arg` is NULL when `None`.
    fn warning(&self, message: &[u8], arg: Option<&[u8]>);
    /// `cannot_decompress(pathname, message)` of ugrep.cpp.
    fn cannot_decompress(&self, pathname: &[u8], message: &[u8]);
    /// An archive was detected (zip, tar, cpio): `if (!flag_no_filename) flag_no_header = false`.
    fn archive_found(&self);
    /// The flags part of `select_matching()`: hidden files, `--exclude`/`--include` globs, `-M` magic bytes.
    fn select_by_flags(&self, path: &[u8], head: &[u8]) -> bool;
    /// The zstream build configuration of this C++ build.
    fn zstream_config(&self) -> ZstreamSettings;
}
