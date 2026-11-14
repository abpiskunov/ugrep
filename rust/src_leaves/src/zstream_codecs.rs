//! Unit `zstream_codecs` of the component `src_leaves`: the zip/7zip decompression state (`ZipInfo` of
//! src/zstream.hpp, its protected helpers) and the per-format decoder state that wraps the retained third-party codec
//! libraries (zlib, bzip2, lzma, lz4, zstd, brotli, bzip3, viizip). Types and signatures are fixed by the Prepare seam
//! task; the bodies are stubs until the Port task of this unit. C++ line ranges: rust/src_leaves/SEAM.md.
//!
//! The third-party libraries stay C: the Port reaches them through `extern "C"` declarations of their C APIs, gated on
//! `Config::codecs`, the codec set the C++ build was compiled with (the HAVE_LIB* and WITH_NO_7ZIP macros).
use core::ffi::c_void;
use std::collections::VecDeque;

/// `FILE*` of the caller, opaque to the signatures; borrowed for the life of the stream.
pub type FilePtr = *mut c_void;

/// Size of the compressed-data buffers (`Z_BUF_LEN`) and of a zip block (`ZipInfo::ZIPBLOCK`).
pub const Z_BUF_LEN: usize = 65536;
pub const ZIPBLOCK: usize = 65536;

pub const CODEC_ZLIB: u32 = 0x01;
pub const CODEC_BZ2: u32 = 0x02;
pub const CODEC_LZMA: u32 = 0x04;
pub const CODEC_LZ4: u32 = 0x08;
pub const CODEC_ZSTD: u32 = 0x10;
pub const CODEC_BROTLI: u32 = 0x20;
pub const CODEC_BZIP3: u32 = 0x40;
pub const CODEC_7ZIP: u32 = 0x80;

/// Build configuration of the C++ side, handed over when a stream is opened (`ugrep_rust_zs_config`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// `CODEC_*` bits: which of HAVE_LIBBZ2, HAVE_LIBLZMA, HAVE_LIBLZ4, HAVE_LIBZSTD, HAVE_LIBBROTLI, HAVE_LIBBZIP3
    /// are defined, zlib always, and 7zip unless WITH_NO_7ZIP.
    pub codecs: u32,
    /// WITH_ZIP_CRC32: verify the zip crc-32.
    pub zip_crc32: bool,
    /// WITH_MAX_7ZIP_SIZE, 0 when not defined.
    pub max_7zip_size: u64,
}

impl Config {
    pub fn has(&self, codec: u32) -> bool {
        self.codecs & codec != 0
    }
}

/// `ZipInfo::Compression`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Compression {
    Store = 0,
    Deflate = 8,
    Bzip2 = 12,
    Lzma = 14,
    Zstd = 93,
    Xz = 95,
}

/// The public fields of `ZipInfo`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZipInfo {
    pub version: u16,
    pub flag: u16,
    /// `Compression` as the raw method number, so an unsupported method survives the bridge.
    pub method: u16,
    pub mtime: i64,
    pub crc: u32,
    pub size: u64,
    pub usize: u64,
    pub name: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagKind {
    /// `cannot_decompress(pathname, message)`
    CannotDecompress = 1,
    /// `warning(message, pathname)`
    Warning = 2,
}

/// Messages the legacy code printed through `cannot_decompress()` and `warning()`, in order. The C++ adapter drains
/// them after each call, so Rust never calls back into C++.
#[derive(Debug, Default)]
pub struct Diagnostics {
    pub queue: VecDeque<(DiagKind, Vec<u8>)>,
}

impl Diagnostics {
    pub fn cannot_decompress(&mut self, message: &str) {
        self.queue.push_back((DiagKind::CannotDecompress, message.as_bytes().to_vec()));
    }

    pub fn warning(&mut self, message: &str) {
        self.queue.push_back((DiagKind::Warning, message.as_bytes().to_vec()));
    }
}

/// Zip or 7zip archive state: `ZipInfo` of src/zstream.hpp.
#[derive(Debug)]
pub struct ZipState {
    pub info: ZipInfo,
    pub pathname: Vec<u8>,
    pub file: FilePtr,
    pub config: Config,
    /// Compressed data already read, `ZipInfo::zbuf_[zcur_..zlen_]`.
    pub pending: Vec<u8>,
    pub znew: bool,
    pub zend: bool,
}

impl ZipState {
    /// `ZipInfo::ZipInfo(pathname, file, buf, len)`: `initial` is the data the caller already read.
    pub fn new(pathname: &[u8], file: FilePtr, config: Config, initial: &[u8]) -> ZipState {
        let take = initial.len().min(Z_BUF_LEN);
        ZipState {
            info: ZipInfo::default(),
            pathname: pathname.to_vec(),
            file,
            config,
            pending: initial[..take].to_vec(),
            znew: true,
            zend: false,
        }
    }

    /// `ZipInfo::header`: read the next local file header (or 7zip entry); false at the end of the archive or on error.
    pub fn header(&mut self, _diag: &mut Diagnostics) -> bool {
        false
    }

    /// `ZipInfo::decompress`: next block of the current entry into `buf`; 0 at the end of the entry, negative on error.
    pub fn decompress(&mut self, _buf: &mut [u8], _diag: &mut Diagnostics) -> isize {
        -1
    }

    /// `ZipInfo::peek`: the compressed zip data available, up to `ZIPBLOCK`.
    pub fn peek(&mut self, _diag: &mut Diagnostics) -> &[u8] {
        &self.pending
    }
}

/// The compressed format of a stream, chosen by `zstream_core` from the file name and the magic bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Gzip,
    Compress,
    Bzip2,
    Xz,
    Lz4,
    Zstd,
    Brotli,
    Bzip3,
    Zip,
    SevenZip,
}

/// Per-format decoder state: the `Z`, `BZ`, `XZ`, `LZ4`, `ZSTD`, `BR`, `BZ3` structs of src/zstream.hpp, `zzFile`
/// for `.Z`, and the zip/7zip archive.
#[derive(Debug)]
pub enum Decoder {
    Stream(Format),
    Zip(Box<ZipState>),
}

impl Decoder {
    /// The constructors of the decoder structs: the state for `format`, `None` when its codec is not in
    /// `config.codecs` or it cannot be allocated. The decoding loops that drive it are `zstream_core::ZStream::next`.
    pub fn new(format: Format, _config: Config) -> Option<Decoder> {
        Some(Decoder::Stream(format))
    }
}

/// `fread(buf, 1, len, file)` of `zstreambuf::file_read`: `len` bytes when successful, fewer at EOF or on error.
pub fn file_read(_file: FilePtr, _buf: &mut [u8]) -> usize {
    0
}
