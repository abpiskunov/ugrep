//! Unit `zstream_core` of the component `src_leaves`: the format selection of `zstreambuf::open`, the filename
//! predicates, `close`, `decompressing`, `zipinfo`, `zippeek` and the `next` decoding loops of src/zstream.hpp. The
//! `std::streambuf` part (buffer, `cur_`/`len_`, underflow/uflow/xsgetn) stays C++ in the adapter. Types and signatures
//! are fixed by the Prepare seam task; the bodies are stubs until the Port task of this unit. C++ line ranges:
//! rust/src_leaves/SEAM.md. The decoder state and the zip/7zip parsing are in `zstream_codecs`.
use crate::signatures::zstream_codecs::{Config, Decoder, Diagnostics, FilePtr, ZipInfo};

// The filename predicates of `zstreambuf` (`is_bz` ... `is_rar`), on the bytes of the C string without NUL.
// They are pure and cross the cxx bridge.

/// `.bz .bz2 .bzip2 .tb2 .tbz .tbz2 .tz2`
pub fn is_bz(_pathname: &[u8]) -> bool {
    false
}

/// `.lzma .xz .tlz .txz`
pub fn is_xz(_pathname: &[u8]) -> bool {
    false
}

/// `.lz4`
pub fn is_lz4(_pathname: &[u8]) -> bool {
    false
}

/// `.zst .zstd .tzst`
pub fn is_zstd(_pathname: &[u8]) -> bool {
    false
}

/// `.br`
pub fn is_br(_pathname: &[u8]) -> bool {
    false
}

/// `.bz3`
pub fn is_bz3(_pathname: &[u8]) -> bool {
    false
}

/// `.Z .taZ .tZ`
pub fn is_z(_pathname: &[u8]) -> bool {
    false
}

/// `.zip .zipx .ZIP`
pub fn is_zip(_pathname: &[u8]) -> bool {
    false
}

/// `.7z .7Z`
pub fn is_7z(_pathname: &[u8]) -> bool {
    false
}

/// `.rar .RAR`
pub fn is_rar(_pathname: &[u8]) -> bool {
    false
}

/// The Rust-owned stream behind `zstreambuf` (`rust_`): one per `open`, a leaked `Box` at the C boundary.
#[derive(Debug)]
pub struct ZStream {
    pub pathname: Vec<u8>,
    pub file: FilePtr,
    pub config: Config,
    /// `None` for a plain file, which is passed through.
    pub decoder: Option<Decoder>,
    /// True once the input is exhausted or failed: the legacy code sets `file_` to NULL.
    pub at_end: bool,
    pub diag: Diagnostics,
    /// Message handed out by the last `diag` pop, NUL-terminated, valid until the next pop.
    pub last_message: Vec<u8>,
}

impl ZStream {
    /// `zstreambuf::open(pathname, file)` for a non-NULL `file` (the adapter handles NULL): pick the format from the
    /// name and the magic bytes, set up the decoder, queue the diagnostics the legacy code printed.
    pub fn open(pathname: &[u8], file: FilePtr, config: Config) -> ZStream {
        ZStream {
            pathname: pathname.to_vec(),
            file,
            config,
            decoder: None,
            at_end: false,
            diag: Diagnostics::default(),
            last_message: Vec::new(),
        }
    }

    /// `zstreambuf::decompressing`.
    pub fn decompressing(&self) -> bool {
        self.decoder.is_some()
    }

    /// `zstreambuf::next`: decompress or copy the next block into `buf`; 0 at EOF, negative on error.
    pub fn next(&mut self, _buf: &mut [u8]) -> isize {
        self.at_end = true;
        0
    }

    /// `zstreambuf::zipinfo`: advance to the next zip/7zip entry; `None` when there is none or not unzipping.
    pub fn zipinfo(&mut self) -> Option<&ZipInfo> {
        None
    }

    /// `zstreambuf::zippeek`: the pending compressed data when unzipping.
    pub fn zippeek(&mut self) -> Option<&[u8]> {
        None
    }
}
