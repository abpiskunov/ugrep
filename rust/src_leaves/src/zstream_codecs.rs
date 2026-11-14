//! Unit `zstream_codecs` of the component `src_leaves`: the zip/7zip decompression state (`ZipInfo` of
//! src/zstream.hpp, its protected helpers) and the per-format decoder state that wraps the retained third-party codec
//! libraries (zlib, bzip2, lzma, lz4, zstd, viizip) together with the decoding loops of `zstreambuf::next` that drive
//! them. Types and signatures were fixed by the Prepare seam task (C++ line ranges: rust/src_leaves/SEAM.md). The
//! third-party libraries stay C: they are reached through `extern "C"` declarations in `ffi/codecs.rs`, and a decoder
//! exists only for a codec in `Config::codecs` (the HAVE_LIB* and WITH_NO_7ZIP macros the C++ build was compiled with).
//! Brotli and bzip3 are not linked into the Rust build (they are not built in the C++ reference either); the stream
//! reports them as an unsupported compression format.
use core::ffi::c_void;
use std::collections::VecDeque;

use crate::signatures::zopen::{self, ZState};

#[path = "ffi/codecs.rs"]
mod sys;
use sys::{BzStream, Lz4Stream, LzmaStream, Tm, Viizip, ZlibStream, ZstdStream};

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

pub const COMPRESS_HEADER_MAGIC: u16 = 0x9d1f;
pub const DEFLATE_HEADER_MAGIC: u16 = 0x8b1f;
pub const ZIP_HEADER_MAGIC: u32 = 0x04034b50;
pub const ZIP_EMPTY_MAGIC: u32 = 0x06054b50;
pub const ZIP_DESCRIPTOR_MAGIC: u32 = 0x08074b50;
pub const ZSTD_HEADER_MAGIC: u32 = 0xfd2fb528;
pub const XZ_HEADER_MAGIC_1: u32 = 0x587a37fd;
pub const XZ_HEADER_MAGIC_2: u16 = 0x005a;

/// `XZ::MEMLIMIT`: files compressed with `xz -9` require 65 MB to decompress.
const XZ_MEMLIMIT: u64 = 100 * 1024 * 1024;
const XZ_FLAGS: u32 = sys::LZMA_TELL_UNSUPPORTED_CHECK | sys::LZMA_CONCATENATED;
/// `PATH_MAX` of the 7zip part pathname buffer.
const PATH_MAX: usize = 4096;

const LZ4_MAX_BLOCK_SIZE: usize = 4194304;
const LZ4_RING_BUFFER_SIZE: usize = 65536 + 14 + LZ4_MAX_BLOCK_SIZE;
const LZ4_COMPRESSBOUND: usize = LZ4_MAX_BLOCK_SIZE + LZ4_MAX_BLOCK_SIZE / 255 + 16;

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

/// `u16(buf)`: 2 bytes in little endian format, a read past the end of `buf` yields 0.
pub fn u16le(buf: &[u8]) -> u16 {
    let at = |i: usize| buf.get(i).copied().unwrap_or(0);
    at(0) as u16 + ((at(1) as u16) << 8)
}

/// `u32(buf)`
pub fn u32le(buf: &[u8]) -> u32 {
    u16le(buf) as u32 + ((u16le(buf.get(2..).unwrap_or(&[])) as u32) << 16)
}

/// `u64(buf)`
pub fn u64le(buf: &[u8]) -> u64 {
    u32le(buf) as u64 + ((u32le(buf.get(4..).unwrap_or(&[])) as u64) << 32)
}

/// `fread(buf, 1, len, file)` of `zstreambuf::file_read`: `len` bytes when successful, fewer at EOF or on error.
pub fn file_read(file: FilePtr, buf: &mut [u8]) -> usize {
    sys::file_read(file, buf)
}

/// `feof(file)`
pub fn file_eof(file: FilePtr) -> bool {
    sys::file_eof(file)
}

/// Result of one decoding step of a stream: `num` is the number of bytes (0 at EOF, negative on error), `done` when
/// the legacy code deleted the decoder and set `file_` to NULL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub num: isize,
    pub done: bool,
}

const READ_ERR: &str = "cannot read";

fn lzma_start(strm: &mut LzmaStream) -> bool {
    strm.auto_decoder(XZ_MEMLIMIT, XZ_FLAGS) == sys::LZMA_OK
}

/// Zip or 7zip archive state: `ZipInfo` of src/zstream.hpp.
pub struct ZipState {
    pub info: ZipInfo,
    pub pathname: Vec<u8>,
    pub file: FilePtr,
    pub config: Config,
    zlib: Option<Box<ZlibStream>>,
    bzip: Option<Box<BzStream>>,
    lzma: Option<Box<LzmaStream>>,
    zstd: Option<ZstdStream>,
    viiz: Option<Viizip>,
    /// Compressed data, `zbuf[zcur..zlen]` is pending.
    zbuf: Vec<u8>,
    zcur: usize,
    zlen: usize,
    zcrc: u32,
    pub znew: bool,
    pub zend: bool,
}

impl core::fmt::Debug for ZipState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ZipState").field("info", &self.info).field("znew", &self.znew).field("zend", &self.zend).finish()
    }
}

impl Drop for ZipState {
    fn drop(&mut self) {
        if let Some(s) = self.zlib.as_mut() {
            s.end();
        }
        if let Some(s) = self.bzip.as_mut() {
            s.end();
        }
        if let Some(s) = self.lzma.as_mut() {
            s.end();
        }
    }
}

/// Reads the next block into `zbuf` (`ZipInfo::read`), true if it is not empty.
fn fill(file: FilePtr, zbuf: &mut [u8], zcur: &mut usize, zlen: &mut usize) -> bool {
    *zcur = 0;
    *zlen = sys::file_read(file, &mut zbuf[..ZIPBLOCK]);
    *zlen > 0
}

impl ZipState {
    /// `ZipInfo::ZipInfo(pathname, file, buf, len)`: `initial` is the data the caller already read.
    pub fn new(pathname: &[u8], file: FilePtr, config: Config, initial: &[u8]) -> ZipState {
        let take = initial.len().min(Z_BUF_LEN);
        let mut zbuf = vec![0u8; ZIPBLOCK];
        zbuf[..take].copy_from_slice(&initial[..take]);
        ZipState {
            info: ZipInfo::default(),
            pathname: pathname.to_vec(),
            file,
            config,
            zlib: None,
            bzip: None,
            lzma: None,
            zstd: None,
            viiz: None,
            zbuf,
            zcur: 0,
            zlen: take,
            zcrc: 0xffffffff,
            znew: true,
            zend: false,
        }
    }

    /// `ZipInfo::viiz_stream::viiz_stream(pathname, file)`: false for a non-seekable or password-protected 7zip file.
    pub fn attach_7zip(&mut self) -> bool {
        self.viiz = Viizip::new(self.file);
        self.viiz.is_some()
    }

    /// `ZipInfo::header`: read the next local file header (or 7zip entry); false at the end of the archive or on error.
    pub fn header(&mut self, diag: &mut Diagnostics) -> bool {
        if self.viiz.is_some() {
            return self.header_7zip(diag);
        }

        // are we at a new header? If not, do nothing and return true
        if !self.znew {
            // try to read the descriptor, if present, before the next header
            if !self.descriptor(diag) {
                return false;
            }
            if !self.znew {
                return true;
            }
        }

        // read the header data and check header magic
        let Some(d) = self.read_num(30, diag) else { return false };
        if u32le(&self.zbuf[d..]) != ZIP_HEADER_MAGIC {
            return false;
        }

        // we're reading the zip local file header
        self.znew = false;

        let h = &self.zbuf[d..d + 30];
        self.info.version = u16le(&h[4..]);
        self.info.flag = u16le(&h[6..]);
        self.info.method = u16le(&h[8..]);
        let ziptime = u16le(&h[10..]);
        let zipdate = u16le(&h[12..]);
        self.info.crc = u32le(&h[14..]);
        self.info.size = u32le(&h[18..]) as u64;
        self.info.usize = u32le(&h[22..]) as u64;
        let namelen = u16le(&h[26..]) as usize;
        let extralen = u16le(&h[28..]) as usize;

        // convert zip time and date
        let mut tm = Tm {
            tm_sec: 2 * (ziptime & 0x1f) as i32,
            tm_min: ((ziptime >> 5) & 0x3f) as i32,
            tm_hour: (ziptime >> 11) as i32,
            tm_mday: (zipdate & 0x1f) as i32,
            tm_mon: ((zipdate >> 5) & 0x3f) as i32,
            tm_year: 80 + (zipdate >> 9) as i32,
            tm_isdst: -1,
            ..Tm::default()
        };
        self.info.mtime = sys::mktime_local(&mut tm);

        // if zip data is encrypted, we give up
        if self.info.flag & 1 != 0 {
            diag.cannot_decompress("zip data is encrypted");
            return false;
        }

        // read the file name
        let Some(n) = self.read_num_msg(namelen, diag) else { return false };
        self.info.name = self.zbuf[n..n + namelen.min(ZIPBLOCK)].to_vec();

        // read the extra field
        let Some(e) = self.read_num_msg(extralen, diag) else { return false };
        self.extra_fields(e, extralen);

        match self.info.method {
            m if m == Compression::Deflate as u16 => {
                // Zip deflate method
                let zcur = self.zcur;
                let avail = self.zlen.wrapping_sub(zcur) as u32;
                let next_in = self.zbuf[zcur..].as_ptr();
                let strm = self.zlib.get_or_insert_with(ZlibStream::new);
                strm.next_in = next_in;
                strm.avail_in = avail;
                strm.next_out = core::ptr::null_mut();
                strm.avail_out = 0;
                if strm.init2(-sys::MAX_WBITS) != sys::Z_OK {
                    let msg = strm.message().unwrap_or_else(|| "inflateInit2 failed".to_string());
                    diag.cannot_decompress(&msg);
                    return false;
                }
            }
            m if m == Compression::Bzip2 as u16 => {
                if !self.config.has(CODEC_BZ2) {
                    diag.cannot_decompress("unsupported zip compression method bzip2");
                    return false;
                }
                let zcur = self.zcur;
                let avail = self.zlen.wrapping_sub(zcur) as u32;
                let next_in = self.zbuf[zcur..].as_ptr() as *mut core::ffi::c_char;
                let strm = self.bzip.get_or_insert_with(BzStream::new);
                strm.next_in = next_in;
                strm.avail_in = avail;
                strm.next_out = core::ptr::null_mut();
                strm.avail_out = 0;
                if strm.init() != sys::BZ_OK {
                    diag.cannot_decompress("BZ2_bzDecompressInit failed");
                    return false;
                }
            }
            m if m == Compression::Zstd as u16 => {
                if !self.config.has(CODEC_ZSTD) {
                    diag.cannot_decompress("unsupported zip compression method zstd");
                    return false;
                }
                match self.zstd.as_mut() {
                    // reinitialize zstd decompress
                    Some(strm) => strm.init(),
                    None => match ZstdStream::new() {
                        Some(strm) => self.zstd = Some(strm),
                        None => {
                            diag.cannot_decompress("out of memory");
                            return false;
                        }
                    },
                }
            }
            m if m == Compression::Lzma as u16 || m == Compression::Xz as u16 => {
                if !self.config.has(CODEC_LZMA) {
                    diag.cannot_decompress("unsupported zip compression method lzma");
                    return false;
                }
                // if bit 1 is clear, then there is no EOS in the lzma stream that we require to terminate decompression
                if m == Compression::Lzma as u16 && self.info.flag & 2 == 0 {
                    diag.cannot_decompress("unsupported zip compression method lzma without EOS");
                    return false;
                }
                let zcur = self.zcur;
                let avail = self.zlen.wrapping_sub(zcur);
                let next_in = self.zbuf[zcur..].as_ptr();
                let mut strm = LzmaStream::new();
                strm.next_in = next_in;
                strm.avail_in = avail;
                let ok = lzma_start(&mut strm);
                self.lzma = Some(strm);
                if !ok {
                    diag.cannot_decompress("lzma_auto_decoder failed");
                    return false;
                }
            }
            m if m == Compression::Store as u16 => {
                if self.info.flag & 8 != 0 {
                    diag.cannot_decompress("zip STORE data size unknown");
                    return false;
                }
            }
            m => {
                diag.cannot_decompress(&format!("unsupported zip compression method {m}"));
                return false;
            }
        }

        // init crc32 to the complement of 0
        self.zcrc = 0xffffffff;
        self.zend = false;
        true
    }

    /// The 7zip part of `ZipInfo::header`: get the next file name and info, start decompressing, false if none.
    fn header_7zip(&mut self, diag: &mut Diagnostics) -> bool {
        // if info was already retrieved, then return
        if !self.znew {
            return true;
        }

        let mut name = [0u8; PATH_MAX];
        let mut mtime = self.info.mtime;
        let mut usize_ = self.info.usize;
        let res = match self.viiz.as_mut() {
            Some(v) => v.get(&mut name, &mut mtime, &mut usize_),
            None => 1,
        };
        self.info.mtime = mtime;
        self.info.usize = usize_;
        let res = if res != 0 {
            res
        } else {
            let n = name.iter().position(|&b| b == 0).unwrap_or(PATH_MAX);
            self.info.name = name[..n].to_vec();
            0
        };

        if res < 0 {
            diag.cannot_decompress("corrupt 7zip archive");
            return false;
        }

        if self.config.max_7zip_size > 0 && res == 0 && self.info.usize > self.config.max_7zip_size {
            let mut message = b"7zip archived file size exceeds the set limit: ".to_vec();
            message.extend_from_slice(&self.info.name);
            diag.queue.push_back((DiagKind::CannotDecompress, message));
            self.znew = true;
            self.zend = true;
            return true;
        }

        // don't advance to the next file
        self.znew = false;
        self.zend = res != 0;
        !self.zend
    }

    /// The extra field loop of `ZipInfo::header`; `num` is a `uint16_t` there and wraps.
    fn extra_fields(&mut self, e: usize, extralen: usize) {
        let data = &self.zbuf[e..];
        let mut num: u16 = 0;
        while (num as usize) < extralen {
            let n = num as usize;
            let id = u16le(data.get(n..).unwrap_or(&[]));
            let flen = u16le(data.get(n + 2..).unwrap_or(&[]));
            if id == 0x0001 {
                // Zip64 Extended Information Extra Field
                self.info.usize = u64le(data.get(n + 4..).unwrap_or(&[]));
                self.info.size = u64le(data.get(n + 12..).unwrap_or(&[]));
                // reset flag bit 3, we check this bit later
                self.info.flag &= !8u16;
            } else if id == 0x7075 {
                // Info-ZIP Unicode Path Extra Field
                let len = flen as usize;
                if len >= 9 && n + len - 9 < extralen {
                    let from = (n + 9).min(data.len());
                    let to = (n + 9 + len - 9).min(data.len());
                    self.info.name = data[from..to].to_vec();
                }
            }
            num = (num as u32).wrapping_add(4 + flen as u32) as u16;
        }
    }

    /// `ZipInfo::decompress`: next block of the current entry into `buf`; 0 at the end of the entry, negative on error.
    pub fn decompress(&mut self, buf: &mut [u8], diag: &mut Diagnostics) -> isize {
        // if header() was not called or no more data to decompress, then return 0 to indicate EOF
        if self.znew || self.zend {
            return 0;
        }

        let len = buf.len();
        if let Some(v) = self.viiz.as_mut() {
            // 7zip decompression
            let n = v.decompress(buf) as isize;
            if n < len as isize {
                self.znew = true;
                self.zend = true;
            }
            return n;
        }

        let method = self.info.method;
        let num = if method == Compression::Deflate as u16 && self.zlib.is_some() {
            self.decompress_deflate(buf, diag)
        } else if method == Compression::Bzip2 as u16 && self.bzip.is_some() {
            self.decompress_bzip2(buf, diag)
        } else if (method == Compression::Lzma as u16 || method == Compression::Xz as u16) && self.lzma.is_some() {
            self.decompress_lzma(buf, diag)
        } else if method == Compression::Zstd as u16 && self.zstd.is_some() {
            self.decompress_zstd(buf, diag)
        } else {
            self.decompress_store(buf, diag)
        };

        if self.config.zip_crc32 && num > 0 {
            self.crc32(&buf[..num as usize]);
        }

        num
    }

    fn decompress_store(&mut self, buf: &mut [u8], diag: &mut Diagnostics) -> isize {
        let mut num: isize = 0;
        // copy the stored zip data until the stored data size is reduced to zero
        if self.info.size > 0 {
            if self.zcur < self.zlen || fill(self.file, &mut self.zbuf, &mut self.zcur, &mut self.zlen) {
                let mut n = self.zlen - self.zcur;
                if (self.info.size as u128) < n as u128 {
                    n = self.info.size as usize;
                }
                n = n.min(buf.len());
                buf[..n].copy_from_slice(&self.zbuf[self.zcur..self.zcur + n]);
                self.zcur += n;
                self.info.size -= n as u64;
                num = n as isize;
            } else {
                diag.cannot_decompress("EOF detected in the zip compressed data");
                self.zend = true;
                num = -1;
            }
        }
        if self.info.size == 0 {
            self.zend = true;
        }
        num
    }

    fn decompress_deflate(&mut self, buf: &mut [u8], diag: &mut Diagnostics) -> isize {
        const ERR: &str = "a zlib decompression error was detected in the zip compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let ZipState { zbuf, zcur, zlen, file, zlib, zend, .. } = self;
        let Some(strm) = zlib.as_mut() else { return 0 };
        loop {
            let mut ret = sys::Z_OK;

            // decompress non-empty zbuf[] into the given buf[]
            if strm.avail_in > 0 && strm.avail_out == 0 {
                strm.next_out = buf.as_mut_ptr();
                strm.avail_out = len as u32;
                ret = strm.inflate(sys::Z_NO_FLUSH);
                *zcur = zlen.wrapping_sub(strm.avail_in as usize);
                if ret != sys::Z_OK && ret != sys::Z_STREAM_END && ret != sys::Z_BUF_ERROR {
                    diag.cannot_decompress(ERR);
                    *zend = true;
                    num = -1;
                } else {
                    num = len as isize - strm.avail_out as isize;
                    *zend = ret == sys::Z_STREAM_END;
                }
            }

            // read compressed data into zbuf[] and decompress zbuf[] into the given buf[]
            if ret == sys::Z_OK && strm.avail_in == 0 && num < (len as i32) as isize {
                if fill(*file, zbuf, zcur, zlen) {
                    strm.next_in = zbuf.as_ptr();
                    strm.avail_in = *zlen as u32;
                    if num == 0 {
                        strm.next_out = buf.as_mut_ptr();
                        strm.avail_out = len as u32;
                    }
                    ret = strm.inflate(sys::Z_NO_FLUSH);
                    *zcur = zlen.wrapping_sub(strm.avail_in as usize);
                    if ret != sys::Z_OK && ret != sys::Z_STREAM_END && ret != sys::Z_BUF_ERROR {
                        diag.cannot_decompress(ERR);
                        *zend = true;
                        num = -1;
                    } else {
                        num = len as isize - strm.avail_out as isize;
                        if num == 0 && ret == sys::Z_OK {
                            continue;
                        }
                        *zend = ret == sys::Z_STREAM_END;
                    }
                } else {
                    diag.cannot_decompress("EOF detected in the zip compressed data");
                    *zend = true;
                    num = -1;
                }
            }

            if *zend && strm.end() != sys::Z_OK {
                diag.cannot_decompress(ERR);
                num = -1;
            }
            break;
        }
        num
    }

    fn decompress_bzip2(&mut self, buf: &mut [u8], diag: &mut Diagnostics) -> isize {
        const ERR: &str = "a bzip2 decompression error was detected in the zip compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let ZipState { zbuf, zcur, zlen, file, bzip, zend, .. } = self;
        let Some(strm) = bzip.as_mut() else { return 0 };
        loop {
            let mut ret = sys::BZ_OK;

            // decompress non-empty zbuf[] into the given buf[]
            if strm.avail_in > 0 && strm.avail_out == 0 {
                strm.next_out = buf.as_mut_ptr().cast();
                strm.avail_out = len as u32;
                ret = strm.decompress();
                *zcur = zlen.wrapping_sub(strm.avail_in as usize);
                if ret != sys::BZ_OK && ret != sys::BZ_STREAM_END {
                    diag.cannot_decompress(ERR);
                    *zend = true;
                    num = -1;
                } else {
                    num = len as isize - strm.avail_out as isize;
                    *zend = ret == sys::BZ_STREAM_END;
                }
            }

            // read compressed data into zbuf[] and decompress zbuf[] into the given buf[]
            if ret == sys::BZ_OK && strm.avail_in == 0 && num < (len as i32) as isize {
                if fill(*file, zbuf, zcur, zlen) {
                    strm.next_in = zbuf.as_mut_ptr().cast();
                    strm.avail_in = *zlen as u32;
                    if num == 0 {
                        strm.next_out = buf.as_mut_ptr().cast();
                        strm.avail_out = len as u32;
                    }
                    ret = strm.decompress();
                    *zcur = zlen.wrapping_sub(strm.avail_in as usize);
                    if ret != sys::BZ_OK && ret != sys::BZ_STREAM_END {
                        diag.cannot_decompress(ERR);
                        *zend = true;
                        num = -1;
                    } else {
                        num = len as isize - strm.avail_out as isize;
                        if num == 0 && ret == sys::BZ_OK {
                            continue;
                        }
                        *zend = ret == sys::BZ_STREAM_END;
                    }
                } else {
                    diag.cannot_decompress("EOF detected in the zip compressed data");
                    *zend = true;
                    num = -1;
                }
            }

            if *zend && strm.end() != sys::BZ_OK {
                diag.cannot_decompress(ERR);
                num = -1;
            }
            break;
        }
        num
    }

    fn decompress_lzma(&mut self, buf: &mut [u8], diag: &mut Diagnostics) -> isize {
        const ERR: &str = "a lzma decompression error was detected in the zip compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let ZipState { zbuf, zcur, zlen, file, lzma, zend, .. } = self;
        let Some(strm) = lzma.as_mut() else { return 0 };
        let mut ret = sys::LZMA_OK;

        // decompress non-empty zbuf[] into the given buf[]
        if strm.avail_in > 0 && strm.avail_out == 0 {
            strm.next_out = buf.as_mut_ptr();
            strm.avail_out = len;
            ret = strm.code(sys::LZMA_RUN);
            *zcur = zlen.wrapping_sub(strm.avail_in);
            if ret != sys::LZMA_OK && ret != sys::LZMA_STREAM_END {
                diag.cannot_decompress(ERR);
                *zend = true;
                num = -1;
            } else {
                num = len as isize - strm.avail_out as isize;
                *zend = ret == sys::LZMA_STREAM_END;
            }
        }

        // read compressed data into zbuf[] and decompress zbuf[] into the given buf[]
        if ret == sys::LZMA_OK && strm.avail_in == 0 && num < len as isize {
            if fill(*file, zbuf, zcur, zlen) {
                strm.next_in = zbuf.as_ptr();
                strm.avail_in = *zlen;
                if num == 0 {
                    strm.next_out = buf.as_mut_ptr();
                    strm.avail_out = len;
                }
                ret = strm.code(sys::LZMA_RUN);
                *zcur = zlen.wrapping_sub(strm.avail_in);
                if ret != sys::LZMA_OK && ret != sys::LZMA_STREAM_END {
                    diag.cannot_decompress(ERR);
                    *zend = true;
                    num = -1;
                } else {
                    num = len as isize - strm.avail_out as isize;
                    *zend = ret == sys::LZMA_STREAM_END;
                }
            } else {
                diag.cannot_decompress("EOF detected in the zip compressed data");
                *zend = true;
                num = -1;
            }
        }

        if *zend {
            strm.end();
        }
        num
    }

    fn decompress_zstd(&mut self, buf: &mut [u8], diag: &mut Diagnostics) -> isize {
        const ERR: &str = "a zstd decompression error was detected in the zip compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let ZipState { zbuf, zcur, zlen, file, zstd, zend, .. } = self;
        let Some(strm) = zstd.as_mut() else { return 0 };
        let mut out_pos = 0usize;
        let mut ret = 1usize;

        if *zcur < *zlen {
            match strm.decompress(buf, 0, &zbuf[..*zlen], *zcur) {
                Err(()) => {
                    diag.cannot_decompress(ERR);
                    *zend = true;
                    num = -1;
                    ret = usize::MAX;
                }
                Ok((in_pos, out, r)) => {
                    *zcur = in_pos;
                    out_pos = out;
                    num = out as isize;
                    ret = r;
                }
            }
        }

        if ret != 0 && num < len as isize && !*zend {
            if fill(*file, zbuf, zcur, zlen) {
                match strm.decompress(buf, out_pos, &zbuf[..*zlen], *zcur) {
                    Err(()) => {
                        diag.cannot_decompress(ERR);
                        *zend = true;
                        num = -1;
                    }
                    Ok((in_pos, out, _)) => {
                        *zcur = in_pos;
                        num = out as isize;
                    }
                }
            } else {
                diag.cannot_decompress("EOF detected in the zip compressed data");
                *zend = true;
                num = -1;
            }
        }
        num
    }

    /// `ZipInfo::descriptor`: read the zip data descriptor, if present, and check the crc.
    fn descriptor(&mut self, diag: &mut Diagnostics) -> bool {
        if self.zend {
            // if we are at the end of the zip data and there is a descriptor, then read it
            if self.info.flag & 8 != 0 {
                let Some(d) = self.read_num(16, diag) else { return false };
                let data = &self.zbuf[d..];
                if u32le(data) != ZIP_DESCRIPTOR_MAGIC {
                    diag.cannot_decompress("an error was detected in the zip compressed data");
                    return false;
                }
                self.info.crc = u32le(&data[4..]);
                self.info.size = u32le(&data[8..]) as u64;
                self.info.usize = u32le(&data[12..]) as u64;
            }

            // we arrived at a new zip local file header
            self.znew = true;

            // now that we have the crc32 value (from the header or the descriptor), check the integrity of the data
            if self.config.zip_crc32 && !self.zcrc != self.info.crc {
                diag.cannot_decompress("a crc error was detected in the zip compressed data");
                return false;
            }
        }
        true
    }

    /// `ZipInfo::peek`: the compressed zip data available, up to `ZIPBLOCK`.
    pub fn peek(&mut self) -> &[u8] {
        if self.zcur > 0 {
            self.zlen -= self.zcur;
            self.zbuf.copy_within(self.zcur..self.zcur + self.zlen, 0);
            self.zcur = 0;
            let ret = sys::file_read(self.file, &mut self.zbuf[self.zlen..ZIPBLOCK]);
            self.zlen += ret;
            if let Some(strm) = self.zlib.as_mut() {
                strm.next_in = self.zbuf.as_ptr();
                strm.avail_in = self.zlen as u32;
            } else if let Some(strm) = self.bzip.as_mut() {
                strm.next_in = self.zbuf.as_mut_ptr().cast();
                strm.avail_in = self.zlen as u32;
            } else if let Some(strm) = self.lzma.as_mut() {
                strm.next_in = self.zbuf.as_ptr();
                strm.avail_in = self.zlen;
            }
        }
        &self.zbuf[..self.zlen]
    }

    /// `ZipInfo::read_num`: block of `num` bytes as an offset into `zbuf`, `None` when unsuccessful.
    fn read_num(&mut self, num: usize, diag: &mut Diagnostics) -> Option<usize> {
        let num = num.min(ZIPBLOCK);

        if self.zlen - self.zcur >= num {
            let ptr = self.zcur;
            self.zcur += num;
            return Some(ptr);
        }

        self.zlen -= self.zcur;
        self.zbuf.copy_within(self.zcur..self.zcur + self.zlen, 0);
        self.zcur = 0;
        let ret = sys::file_read(self.file, &mut self.zbuf[self.zlen..ZIPBLOCK]);
        self.zlen += ret;
        if self.zlen >= num {
            self.zcur = num;
            return Some(0);
        }

        if sys::file_error(self.file) {
            diag.warning(READ_ERR);
        } else {
            diag.cannot_decompress("an error was detected in the zip compressed data");
        }
        None
    }

    /// `read_num` for the file name and the extra field, which report "corrupt zip archive" on failure.
    fn read_num_msg(&mut self, num: usize, diag: &mut Diagnostics) -> Option<usize> {
        let mut inner = Diagnostics::default();
        let r = self.read_num(num, &mut inner);
        diag.queue.append(&mut inner.queue);
        if r.is_none() {
            diag.cannot_decompress("corrupt zip archive");
        }
        r
    }

    /// `ZipInfo::crc32`
    fn crc32(&mut self, buf: &[u8]) {
        for &b in buf {
            self.zcrc ^= b as u32;
            for _ in 0..8 {
                self.zcrc = (self.zcrc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(self.zcrc & 1));
            }
        }
    }
}

/// zlib (gzip) decoder state: `zstreambuf::Z`.
pub struct Gzip {
    strm: Box<ZlibStream>,
    zbuf: Vec<u8>,
    zlen: usize,
    zend: bool,
}

impl Drop for Gzip {
    fn drop(&mut self) {
        self.strm.end();
    }
}

impl Gzip {
    /// Open on the 2 gzip magic bytes already read; `Err(message)` for `cannot_decompress`.
    pub fn new(magic: &[u8]) -> Result<Gzip, String> {
        let mut z = Gzip { strm: ZlibStream::new(), zbuf: vec![0u8; Z_BUF_LEN], zlen: 0, zend: false };
        // copy the gzip header's magic bytes to zbuf[], needed by inflate()
        z.zbuf[..magic.len()].copy_from_slice(magic);
        z.zlen = magic.len();
        z.strm.next_in = z.zbuf.as_ptr();
        z.strm.avail_in = z.zlen as u32;
        // inflate gzip compressed data starting with a gzip header
        if z.strm.init2(16 + sys::MAX_WBITS) != sys::Z_OK {
            return Err(z.strm.message().unwrap_or_else(|| "inflateInit2 failed".to_string()));
        }
        Ok(z)
    }

    pub fn next(&mut self, file: FilePtr, diag: &mut Diagnostics, buf: &mut [u8]) -> Step {
        const ERR: &str = "an error was detected in the gzip compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let strm = &mut self.strm;
        loop {
            let mut ret = sys::Z_OK;

            // decompress non-empty zbuf[] into the given buf[]
            if strm.avail_in > 0 && strm.avail_out == 0 {
                strm.next_out = buf.as_mut_ptr();
                strm.avail_out = len as u32;
                ret = strm.inflate(sys::Z_NO_FLUSH);
                if ret != sys::Z_OK && ret != sys::Z_STREAM_END && ret != sys::Z_BUF_ERROR {
                    diag.cannot_decompress(ERR);
                    num = -1;
                } else {
                    num = len as isize - strm.avail_out as isize;
                }
            }

            // read compressed data into zbuf[] and decompress zbuf[] into the given buf[]
            if ret == sys::Z_OK && strm.avail_in == 0 && num < len as isize && !self.zend {
                self.zlen = sys::file_read(file, &mut self.zbuf[..Z_BUF_LEN]);
                if sys::file_error(file) {
                    diag.warning(READ_ERR);
                    self.zend = true;
                    num = -1;
                } else {
                    if sys::file_eof(file) {
                        self.zend = true;
                    }
                    strm.next_in = self.zbuf.as_ptr();
                    strm.avail_in = self.zlen as u32;
                    if num == 0 {
                        strm.next_out = buf.as_mut_ptr();
                        strm.avail_out = len as u32;
                    }
                    ret = strm.inflate(sys::Z_NO_FLUSH);
                    if ret != sys::Z_OK && ret != sys::Z_STREAM_END && ret != sys::Z_BUF_ERROR {
                        diag.cannot_decompress(ERR);
                        num = -1;
                    } else {
                        num = len as isize - strm.avail_out as isize;
                        if num == 0 && ret == sys::Z_OK {
                            continue;
                        }
                    }
                }
            }

            if num >= 0 && ret == sys::Z_STREAM_END {
                // try to decompress the next concatenated gzip data
                if strm.reset() == sys::Z_OK {
                    strm.next_out = core::ptr::null_mut();
                    strm.avail_out = 0;
                    ret = sys::Z_OK;
                    if num == 0 {
                        continue;
                    }
                }
            }

            // decompressed the last block or there was an error?
            return Step { num, done: num <= 0 || ret == sys::Z_STREAM_END };
        }
    }
}

/// bzip2 decoder state: `zstreambuf::BZ`.
pub struct Bzip2 {
    strm: Box<BzStream>,
    zbuf: Vec<u8>,
    zlen: usize,
    zend: bool,
}

impl Drop for Bzip2 {
    fn drop(&mut self) {
        self.strm.end();
    }
}

impl Bzip2 {
    /// `initial` is the data of the magic bytes already read, passed to the input buffer. `Err` when
    /// `BZ2_bzDecompressInit` fails (the legacy code warns).
    pub fn new(initial: &[u8]) -> Option<Bzip2> {
        let mut z = Bzip2 { strm: BzStream::new(), zbuf: vec![0u8; Z_BUF_LEN], zlen: 0, zend: false };
        if !initial.is_empty() {
            z.zbuf[..initial.len()].copy_from_slice(initial);
            z.strm.next_in = z.zbuf.as_mut_ptr().cast();
            z.strm.avail_in = initial.len() as u32;
        }
        if z.strm.init() != sys::BZ_OK {
            return None;
        }
        Some(z)
    }

    pub fn next(&mut self, file: FilePtr, diag: &mut Diagnostics, buf: &mut [u8]) -> Step {
        const ERR: &str = "an error was detected in the bzip2 compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let strm = &mut self.strm;
        loop {
            let mut ret = sys::BZ_OK;

            // decompress non-empty zbuf[] into the given buf[]
            if strm.avail_in > 0 && strm.avail_out == 0 {
                strm.next_out = buf.as_mut_ptr().cast();
                strm.avail_out = len as u32;
                ret = strm.decompress();
                if ret != sys::BZ_OK && ret != sys::BZ_STREAM_END {
                    diag.cannot_decompress(ERR);
                    num = -1;
                } else {
                    num = len as isize - strm.avail_out as isize;
                }
            }

            // read compressed data into zbuf[] and decompress zbuf[] into the given buf[]
            if ret == sys::BZ_OK && strm.avail_in == 0 && num < len as isize && !self.zend {
                self.zlen = sys::file_read(file, &mut self.zbuf[..Z_BUF_LEN]);
                if sys::file_error(file) {
                    diag.warning(READ_ERR);
                    self.zend = true;
                    num = -1;
                } else {
                    if sys::file_eof(file) {
                        self.zend = true;
                    }
                    strm.next_in = self.zbuf.as_mut_ptr().cast();
                    strm.avail_in = self.zlen as u32;
                    if num == 0 {
                        strm.next_out = buf.as_mut_ptr().cast();
                        strm.avail_out = len as u32;
                    }
                    ret = strm.decompress();
                    if ret != sys::BZ_OK && ret != sys::BZ_STREAM_END {
                        diag.cannot_decompress(ERR);
                        num = -1;
                    } else {
                        num = len as isize - strm.avail_out as isize;
                        if num == 0 && ret == sys::BZ_OK {
                            continue;
                        }
                    }
                }
            }

            if num >= 0 && ret == sys::BZ_STREAM_END {
                // try to decompress the next concatenated bzip2 data
                if strm.end() == sys::BZ_OK && strm.init() == sys::BZ_OK {
                    strm.next_out = core::ptr::null_mut();
                    strm.avail_out = 0;
                    ret = sys::BZ_OK;
                    if num == 0 {
                        continue;
                    }
                }
            }

            // decompressed the last block or there was an error?
            return Step { num, done: num <= 0 || ret == sys::BZ_STREAM_END };
        }
    }
}

/// lzma and xz decoder state: `zstreambuf::XZ`.
pub struct Xz {
    strm: Box<LzmaStream>,
    zbuf: Vec<u8>,
    zlen: usize,
    zend: bool,
}

impl Drop for Xz {
    fn drop(&mut self) {
        self.strm.end();
    }
}

impl Xz {
    /// `initial` is the data of the magic bytes already read, passed to the input buffer. `Err` when
    /// `lzma_auto_decoder` fails (the legacy code warns).
    pub fn new(initial: &[u8]) -> Option<Xz> {
        let mut z = Xz { strm: LzmaStream::new(), zbuf: vec![0u8; Z_BUF_LEN], zlen: 0, zend: false };
        if !initial.is_empty() {
            z.zbuf[..initial.len()].copy_from_slice(initial);
            z.strm.next_in = z.zbuf.as_ptr();
            z.strm.avail_in = initial.len();
        }
        if !lzma_start(&mut z.strm) {
            return None;
        }
        Some(z)
    }

    pub fn next(&mut self, file: FilePtr, diag: &mut Diagnostics, buf: &mut [u8]) -> Step {
        const ERR: &str = "an error was detected in the xz/lzma compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let strm = &mut self.strm;
        let mut ret = sys::LZMA_OK;

        // decompress non-empty zbuf[] into the given buf[]
        if strm.avail_in > 0 && strm.avail_out == 0 {
            strm.next_out = buf.as_mut_ptr();
            strm.avail_out = len;
            ret = strm.code(if self.zend { sys::LZMA_FINISH } else { sys::LZMA_RUN });
            if ret != sys::LZMA_OK && ret != sys::LZMA_STREAM_END {
                diag.cannot_decompress(ERR);
                num = -1;
            } else {
                num = len as isize - strm.avail_out as isize;
            }
        }

        // read compressed data into zbuf[] and decompress zbuf[] into the given buf[]
        if ret == sys::LZMA_OK && strm.avail_in == 0 && num < len as isize && !self.zend {
            self.zlen = sys::file_read(file, &mut self.zbuf[..Z_BUF_LEN]);
            if sys::file_error(file) {
                diag.warning(READ_ERR);
                self.zend = true;
                num = -1;
            } else {
                if sys::file_eof(file) {
                    self.zend = true;
                }
                strm.next_in = self.zbuf.as_ptr();
                strm.avail_in = self.zlen;
                if num == 0 {
                    strm.next_out = buf.as_mut_ptr();
                    strm.avail_out = len;
                }
                ret = strm.code(if self.zend { sys::LZMA_FINISH } else { sys::LZMA_RUN });
                if ret != sys::LZMA_OK && ret != sys::LZMA_STREAM_END {
                    diag.cannot_decompress(ERR);
                    num = -1;
                } else {
                    num = len as isize - strm.avail_out as isize;
                }
            }
        }

        // decompressed the last block or there was an error?
        Step { num, done: num <= 0 || ret == sys::LZMA_STREAM_END }
    }
}

/// lz4 decoder state: `zstreambuf::LZ4`.
pub struct Lz4 {
    strm: Lz4Stream,
    buf: Vec<u8>,
    loc: usize,
    len: usize,
    crc: u32,
    size: u64,
    dict: u32,
    zbuf: Vec<u8>,
    zflg: u8,
    zloc: usize,
    zlen: usize,
    zcrc: u32,
}

impl Lz4 {
    /// `None` when `LZ4_createStreamDecode` fails (the legacy code warns).
    pub fn new() -> Option<Lz4> {
        Some(Lz4 {
            strm: Lz4Stream::new()?,
            buf: vec![0u8; LZ4_RING_BUFFER_SIZE],
            loc: 0,
            len: 0,
            crc: 0,
            size: 0,
            dict: 0,
            zbuf: vec![0u8; LZ4_COMPRESSBOUND + Z_BUF_LEN + 8],
            zflg: 0,
            zloc: 0,
            zlen: 0,
            zcrc: 0,
        })
    }

    /// Move the incomplete data `zbuf[zloc..zlen]` to the start of `zbuf`; false if `zloc` is past `zlen` (the legacy
    /// code wraps `zlen` there and crashes in `memmove`).
    fn compact(&mut self) -> bool {
        if self.zloc > self.zlen {
            return false;
        }
        self.zlen -= self.zloc;
        if self.zlen > 0 {
            self.zbuf.copy_within(self.zloc..self.zloc + self.zlen, 0);
        }
        self.zloc = 0;
        true
    }

    /// `fread` into `zbuf[zlen..]`, at most `Z_BUF_LEN` bytes.
    fn more(&mut self, file: FilePtr) {
        let z = self.zlen;
        let n = sys::file_read(file, &mut self.zbuf[z..z + Z_BUF_LEN]);
        self.zlen += n;
    }

    pub fn next(&mut self, file: FilePtr, diag: &mut Diagnostics, buf: &mut [u8]) -> Step {
        const ERR: &str = "an error was detected in the lz4 compressed data";
        let mut len = buf.len();
        let mut num: isize = 0;

        if self.loc < self.len {
            // copy decompressed data from buf[] into the given buf[]
            if self.loc + len > self.len {
                len = self.len - self.loc;
            }
            buf[..len].copy_from_slice(&self.buf[self.loc..self.loc + len]);
            self.loc += len;
            return Step { num: len as isize, done: false };
        }

        // loop over frames
        loop {
            // get next frame if we do not have a valid frame
            if self.zflg == 0 {
                // move incomplete frame header to the start of zbuf[]
                if !self.compact() {
                    diag.cannot_decompress(ERR);
                    num = -1;
                    break;
                }

                // frame header is 7 to 19 bytes long
                if self.zlen < 19 {
                    self.more(file);
                }

                // if not enough frame data, error if not at EOF, otherwise EOF is OK
                if self.zlen < 7 {
                    if self.zlen > 0 || sys::file_error(file) {
                        diag.warning(READ_ERR);
                        num = -1;
                    }
                    break;
                }

                // check magic bytes and flags
                let magic = u32le(&self.zbuf);
                if (0x184D2A50..=0x184D2A5F).contains(&magic) {
                    // skippable frame with 4 byte frame size
                    if self.zlen < 8 {
                        diag.warning(READ_ERR);
                        num = -1;
                        break;
                    }

                    // skip this skippable frame, then continue with next frame
                    let mut size = u32le(&self.zbuf[4..]) as usize;
                    self.zloc = 8;
                    if self.zloc + size > self.zlen {
                        size -= self.zlen - self.zloc;
                        self.zloc = self.zlen;

                        while size > 0 {
                            let n = sys::file_read(file, &mut self.zbuf[..size.min(Z_BUF_LEN)]);
                            if n == 0 {
                                break;
                            }
                            size -= n;
                        }

                        if size > 0 {
                            diag.warning(READ_ERR);
                            num = -1;
                            break;
                        }

                        self.zloc = 0;
                        self.zlen = 0;
                    } else {
                        self.zloc += size;
                    }
                    continue;
                }
                self.zflg = self.zbuf[4];
                if magic != 0x184D2204 || (self.zflg & 0xc0) != 0x40 {
                    diag.cannot_decompress(ERR);
                    num = -1;
                    break;
                }

                // get decompressed content size and dict, position to the start of the block
                self.zloc = 7;
                if self.zbuf[4] & 0x08 != 0 {
                    self.size = u64le(&self.zbuf[self.zloc..]);
                    self.zloc += 8;
                }
                if self.zbuf[4] & 0x01 != 0 {
                    self.dict = u32le(&self.zbuf[self.zloc..]);
                    self.zloc += 4;
                }

                // error if header was too short
                if self.zlen < self.zloc {
                    self.zloc = self.zlen;
                    diag.cannot_decompress(ERR);
                    num = -1;
                    break;
                }
            }

            // move incomplete block data to the start of zbuf
            if !self.compact() {
                diag.cannot_decompress(ERR);
                num = -1;
                break;
            }

            // need 4 bytes with block size
            if self.zlen < 4 {
                self.more(file);
            }
            if self.zlen < 4 {
                diag.warning(READ_ERR);
                num = -1;
                break;
            }

            // get the block size
            let mut size = u32le(&self.zbuf);
            self.zloc = 4;

            if size == 0 {
                // reached the end marker, get footer with crc when present
                if self.zflg & 0x04 != 0 {
                    self.crc = u32le(&self.zbuf[self.zloc..]);
                    self.zloc += 4;
                }

                // reset flags to start a new frame and continue to read the frame
                self.zflg = 0;
                continue;
            }

            // if MSB(size) is set then data is uncompressed
            let compressed = size & 0x80000000 == 0;
            if !compressed {
                size &= 0x7fffffff;
            }
            let size = size as usize;

            // block cannot be larger than the documented max block size of 4MB
            if size > LZ4_MAX_BLOCK_SIZE {
                diag.cannot_decompress(ERR);
                num = -1;
                break;
            }

            // read the rest of the block, may overshoot by up to Z_BUF_LEN bytes
            while self.zloc + size > self.zlen {
                let before = self.zlen;
                self.more(file);
                if self.zlen == before {
                    break;
                }
            }
            if self.zloc + size > self.zlen {
                diag.warning(READ_ERR);
                num = -1;
                break;
            }

            if compressed {
                // decompress zbuf[] block into buf[]
                if self.loc >= LZ4_RING_BUFFER_SIZE - LZ4_MAX_BLOCK_SIZE {
                    self.loc = 0;
                }
                let ret = self.strm.decompress_continue(
                    &self.zbuf[self.zloc..self.zloc + size],
                    &mut self.buf[self.loc..],
                    LZ4_MAX_BLOCK_SIZE as i32,
                );
                if ret <= 0 {
                    diag.cannot_decompress(ERR);
                    num = -1;
                    break;
                }
                self.len = self.loc + ret as usize;
            } else {
                // copy uncompressed data into buf[]
                self.buf[self.loc..self.loc + size].copy_from_slice(&self.zbuf[self.zloc..self.zloc + size]);
                self.len = self.loc + size;
            }

            // move over processed data
            self.zloc += size;

            // copy data from buf[] into the given buf[]
            if self.loc + len > self.len {
                len = self.len - self.loc;
            }
            buf[..len].copy_from_slice(&self.buf[self.loc..self.loc + len]);
            self.loc += len;
            num = len as isize;

            if self.zflg & 0x10 != 0 {
                // need 4 bytes with block checksum
                if self.zloc + 4 > self.zlen {
                    self.more(file);
                }
                if self.zloc + 4 > self.zlen {
                    diag.warning(READ_ERR);
                    num = -1;
                    break;
                }

                // get block checksum
                self.zcrc = u32le(&self.zbuf[self.zloc..]);
                self.zloc += 4;
            }

            if num > 0 {
                break;
            }
        }
        Step { num, done: false }
    }
}

/// zstd decoder state: `zstreambuf::ZSTD`.
pub struct Zstd {
    strm: ZstdStream,
    zbuf: Vec<u8>,
    zloc: usize,
    zlen: usize,
    zend: bool,
}

impl Zstd {
    /// `None` when `ZSTD_createDStream` fails (the legacy code warns). `initial` is the magic already read, which is
    /// passed to the input buffer.
    pub fn new(initial: &[u8]) -> Option<Zstd> {
        let mut z = Zstd { strm: ZstdStream::new()?, zbuf: vec![0u8; sys::zstd_dstream_in_size()], zloc: 0, zlen: 0, zend: false };
        z.zbuf[..initial.len()].copy_from_slice(initial);
        z.zlen = initial.len();
        Some(z)
    }

    pub fn next(&mut self, file: FilePtr, diag: &mut Diagnostics, buf: &mut [u8]) -> Step {
        const ERR: &str = "an error was detected in the zstd compressed data";
        let len = buf.len();
        let mut num: isize = 0;
        let mut out_pos = 0usize;

        // decompress zbuf[] into buf[]
        if self.zloc < self.zlen {
            match self.strm.decompress(buf, 0, &self.zbuf[..self.zlen], self.zloc) {
                Err(()) => {
                    diag.cannot_decompress(ERR);
                    num = -1;
                }
                Ok((in_pos, out, _)) => {
                    self.zloc = in_pos;
                    out_pos = out;
                    num = out as isize;
                }
            }
        }

        // read compressed data into zbuf[] and decompress zbuf[] into the given buf[]
        if num >= 0 && num < len as isize && self.zloc >= self.zlen && !self.zend {
            self.zloc = 0;
            let size = self.zbuf.len();
            self.zlen = sys::file_read(file, &mut self.zbuf[..size]);

            if sys::file_error(file) {
                diag.warning(READ_ERR);
                self.zend = true;
            } else if sys::file_eof(file) {
                self.zend = true;
            }

            match self.strm.decompress(buf, out_pos, &self.zbuf[..self.zlen], self.zloc) {
                Err(()) => {
                    diag.cannot_decompress(ERR);
                    num = -1;
                }
                Ok((in_pos, out, _)) => {
                    self.zloc = in_pos;
                    num = out as isize;
                }
            }
        }

        // decompressed the last block or there was an error?
        Step { num, done: num <= 0 }
    }
}

/// Per-format decoder state: the `Z`, `BZ`, `XZ`, `LZ4`, `ZSTD` structs of src/zstream.hpp, the `zzFile` of `.Z`
/// and the zip/7zip archive.
pub enum Decoder {
    Gzip(Gzip),
    Compress(Box<ZState>),
    Bzip2(Bzip2),
    Xz(Xz),
    Lz4(Box<Lz4>),
    Zstd(Zstd),
    Zip(Box<ZipState>),
}

impl core::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Decoder::Gzip(_) => "Gzip",
            Decoder::Compress(_) => "Compress",
            Decoder::Bzip2(_) => "Bzip2",
            Decoder::Xz(_) => "Xz",
            Decoder::Lz4(_) => "Lz4",
            Decoder::Zstd(_) => "Zstd",
            Decoder::Zip(_) => "Zip",
        })
    }
}

impl Decoder {
    /// The `next` decoding loop of the format: decompress the next block into `buf`; 0 at EOF, negative on error.
    pub fn next(&mut self, file: FilePtr, diag: &mut Diagnostics, buf: &mut [u8]) -> Step {
        match self {
            Decoder::Gzip(z) => z.next(file, diag, buf),
            Decoder::Compress(z) => {
                // decompress a compress (Z) compressed block into the given buf[]
                let len = (buf.len() as i32).max(0) as usize;
                let num = zopen::z_read(z, &mut buf[..len]) as isize;
                // an error occurred?
                if num < 0 {
                    diag.cannot_decompress("an error was detected in the compressed data");
                }
                // decompressed the last block or there was an error?
                Step { num, done: num <= 0 }
            }
            Decoder::Bzip2(z) => z.next(file, diag, buf),
            Decoder::Xz(z) => z.next(file, diag, buf),
            Decoder::Lz4(z) => z.next(file, diag, buf),
            Decoder::Zstd(z) => z.next(file, diag, buf),
            Decoder::Zip(z) => {
                // decompress a zip compressed block into the given buf[]
                let num = z.decompress(buf, diag);
                // there was an error?
                Step { num, done: num < 0 }
            }
        }
    }

    /// `zstreambuf::close`: the legacy `close_cpp` frees the handle; the `.Z` handle is closed through `z_close`.
    pub fn close(self) {
        if let Decoder::Compress(z) = self {
            zopen::z_close(*z);
        }
    }
}

/// `zopen::z_open(file, "r", 0, 1)` for a compress (Z) file.
pub fn open_compress(file: FilePtr) -> Option<Decoder> {
    zopen::z_open(file, b"r", 0, 1).map(Decoder::Compress)
}
