//! C ABI of the retained third-party codec libraries (zlib, bzip2, liblzma, lz4, zstd, the in-repo viizip 7zip
//! wrapper) and of the C runtime calls of the unit `zstream_codecs`. The libraries stay C (decision D2); the structs
//! are the `#[repr(C)]` layouts of their public headers. Unsafe code of the unit lives here and in `zstream_codecs.rs`.
use core::ffi::{c_char, c_int, c_ulong, c_void};
#[cfg(not(windows))]
use core::ffi::c_long;

// ---- C runtime ----

extern "C" {
    fn fread(buf: *mut c_void, size: usize, n: usize, f: *mut c_void) -> usize;
    fn ferror(f: *mut c_void) -> c_int;
    fn feof(f: *mut c_void) -> c_int;
    #[cfg(windows)]
    fn _mktime64(tm: *mut Tm) -> i64;
    #[cfg(not(windows))]
    fn mktime(tm: *mut Tm) -> c_long;
}

/// `struct tm`
#[repr(C)]
#[derive(Default)]
pub struct Tm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    #[cfg(not(windows))]
    pub tm_gmtoff: c_long,
    #[cfg(not(windows))]
    pub tm_zone: usize,
}

/// `mktime(&tm)` as `time_t` (64 bit).
pub fn mktime_local(tm: &mut Tm) -> i64 {
    // SAFETY: `tm` is a valid, initialized `struct tm`.
    unsafe {
        #[cfg(windows)]
        {
            _mktime64(tm)
        }
        #[cfg(not(windows))]
        {
            mktime(tm) as i64
        }
    }
}

/// `fread(buf, 1, buf.len(), file)`
pub fn file_read(file: *mut c_void, buf: &mut [u8]) -> usize {
    if file.is_null() || buf.is_empty() {
        return 0;
    }
    // SAFETY: `buf` is writable for `buf.len()` bytes; `file` is the caller's valid `FILE*`.
    unsafe { fread(buf.as_mut_ptr().cast(), 1, buf.len(), file) }
}

pub fn file_error(file: *mut c_void) -> bool {
    // SAFETY: `file` is the caller's valid `FILE*`.
    !file.is_null() && unsafe { ferror(file) != 0 }
}

pub fn file_eof(file: *mut c_void) -> bool {
    // SAFETY: `file` is the caller's valid `FILE*`.
    !file.is_null() && unsafe { feof(file) != 0 }
}

// ---- zlib ----

pub const Z_OK: c_int = 0;
pub const Z_STREAM_END: c_int = 1;
pub const Z_BUF_ERROR: c_int = -5;
pub const Z_NO_FLUSH: c_int = 0;
pub const MAX_WBITS: c_int = 15;

/// `z_stream`
#[repr(C)]
pub struct ZlibStream {
    pub next_in: *const u8,
    pub avail_in: u32,
    pub total_in: c_ulong,
    pub next_out: *mut u8,
    pub avail_out: u32,
    pub total_out: c_ulong,
    pub msg: *const c_char,
    state: *mut c_void,
    zalloc: *mut c_void,
    zfree: *mut c_void,
    opaque: *mut c_void,
    pub data_type: c_int,
    pub adler: c_ulong,
    reserved: c_ulong,
}

extern "C" {
    fn inflateInit2_(strm: *mut ZlibStream, window_bits: c_int, version: *const c_char, size: c_int) -> c_int;
    fn inflate(strm: *mut ZlibStream, flush: c_int) -> c_int;
    fn inflateEnd(strm: *mut ZlibStream) -> c_int;
    fn inflateReset(strm: *mut ZlibStream) -> c_int;
}

impl ZlibStream {
    /// All fields zero: `zalloc`, `zfree`, `opaque`, `next_in`, `next_out` are `Z_NULL`, `avail_*` are 0, `msg` NULL.
    pub fn new() -> Box<ZlibStream> {
        // SAFETY: every field of `ZlibStream` is an integer or a pointer, for which all-zero is valid.
        Box::new(unsafe { core::mem::zeroed() })
    }

    /// `inflateInit2(strm, window_bits)`
    pub fn init2(&mut self, window_bits: c_int) -> c_int {
        // SAFETY: `self` is a valid `z_stream` with null allocators; the version string is NUL-terminated.
        unsafe { inflateInit2_(self, window_bits, c"1.3.1".as_ptr(), core::mem::size_of::<ZlibStream>() as c_int) }
    }

    pub fn inflate(&mut self, flush: c_int) -> c_int {
        // SAFETY: `next_in`/`next_out` point to buffers valid for `avail_in`/`avail_out` bytes, kept alive by the caller.
        unsafe { inflate(self, flush) }
    }

    pub fn end(&mut self) -> c_int {
        // SAFETY: `inflateEnd` on a stream that was never initialized or is already ended returns Z_STREAM_ERROR.
        unsafe { inflateEnd(self) }
    }

    pub fn reset(&mut self) -> c_int {
        // SAFETY: `self` is an initialized stream.
        unsafe { inflateReset(self) }
    }

    /// The `msg` string of the last error, if any.
    pub fn message(&self) -> Option<String> {
        if self.msg.is_null() {
            return None;
        }
        // SAFETY: zlib sets `msg` to a static NUL-terminated string.
        Some(unsafe { core::ffi::CStr::from_ptr(self.msg) }.to_string_lossy().into_owned())
    }
}

// ---- bzip2 ----

pub const BZ_OK: c_int = 0;
pub const BZ_STREAM_END: c_int = 4;

/// `bz_stream`
#[repr(C)]
pub struct BzStream {
    pub next_in: *mut c_char,
    pub avail_in: u32,
    total_in_lo32: u32,
    total_in_hi32: u32,
    pub next_out: *mut c_char,
    pub avail_out: u32,
    total_out_lo32: u32,
    total_out_hi32: u32,
    state: *mut c_void,
    bzalloc: *mut c_void,
    bzfree: *mut c_void,
    opaque: *mut c_void,
}

extern "C" {
    fn BZ2_bzDecompressInit(strm: *mut BzStream, verbosity: c_int, small: c_int) -> c_int;
    fn BZ2_bzDecompress(strm: *mut BzStream) -> c_int;
    fn BZ2_bzDecompressEnd(strm: *mut BzStream) -> c_int;
}

impl BzStream {
    pub fn new() -> Box<BzStream> {
        // SAFETY: every field is an integer or a pointer, all-zero is valid (`bzalloc`/`bzfree`/`opaque` NULL).
        Box::new(unsafe { core::mem::zeroed() })
    }

    pub fn init(&mut self) -> c_int {
        // SAFETY: `self` is a valid `bz_stream` with null allocators.
        unsafe { BZ2_bzDecompressInit(self, 0, 0) }
    }

    pub fn decompress(&mut self) -> c_int {
        // SAFETY: `next_in`/`next_out` point to buffers valid for `avail_in`/`avail_out` bytes, kept alive by the caller.
        unsafe { BZ2_bzDecompress(self) }
    }

    pub fn end(&mut self) -> c_int {
        // SAFETY: on a stream without state (never initialized or already ended) bzip2 returns BZ_PARAM_ERROR.
        unsafe { BZ2_bzDecompressEnd(self) }
    }
}

// ---- liblzma ----

pub const LZMA_OK: c_int = 0;
pub const LZMA_STREAM_END: c_int = 1;
pub const LZMA_RUN: c_int = 0;
pub const LZMA_FINISH: c_int = 3;
pub const LZMA_TELL_UNSUPPORTED_CHECK: u32 = 0x02;
pub const LZMA_CONCATENATED: u32 = 0x08;

/// `lzma_stream`; all-zero is `LZMA_STREAM_INIT`.
#[repr(C)]
pub struct LzmaStream {
    pub next_in: *const u8,
    pub avail_in: usize,
    total_in: u64,
    pub next_out: *mut u8,
    pub avail_out: usize,
    total_out: u64,
    allocator: *const c_void,
    internal: *mut c_void,
    reserved_ptr: [*mut c_void; 4],
    seek_pos: u64,
    reserved_int2: u64,
    reserved_int3: usize,
    reserved_int4: usize,
    reserved_enum1: c_int,
    reserved_enum2: c_int,
}

extern "C" {
    fn lzma_auto_decoder(strm: *mut LzmaStream, memlimit: u64, flags: u32) -> c_int;
    fn lzma_code(strm: *mut LzmaStream, action: c_int) -> c_int;
    fn lzma_end(strm: *mut LzmaStream);
}

impl LzmaStream {
    /// `LZMA_STREAM_INIT`
    pub fn new() -> Box<LzmaStream> {
        // SAFETY: every field is an integer or a pointer, all-zero is `LZMA_STREAM_INIT`.
        Box::new(unsafe { core::mem::zeroed() })
    }

    pub fn auto_decoder(&mut self, memlimit: u64, flags: u32) -> c_int {
        // SAFETY: `self` is an initialized (`LZMA_STREAM_INIT`) stream.
        unsafe { lzma_auto_decoder(self, memlimit, flags) }
    }

    pub fn code(&mut self, action: c_int) -> c_int {
        // SAFETY: `next_in`/`next_out` point to buffers valid for `avail_in`/`avail_out` bytes, kept alive by the caller.
        unsafe { lzma_code(self, action) }
    }

    pub fn end(&mut self) {
        // SAFETY: `lzma_end` accepts a stream with no internal state.
        unsafe { lzma_end(self) }
    }
}

// ---- lz4 ----

extern "C" {
    fn LZ4_createStreamDecode() -> *mut c_void;
    fn LZ4_freeStreamDecode(strm: *mut c_void) -> c_int;
    fn LZ4_decompress_safe_continue(
        strm: *mut c_void,
        src: *const c_char,
        dst: *mut c_char,
        src_size: c_int,
        dst_capacity: c_int,
    ) -> c_int;
}

/// `LZ4_streamDecode_t*`
pub struct Lz4Stream(*mut c_void);

impl Lz4Stream {
    pub fn new() -> Option<Lz4Stream> {
        // SAFETY: plain allocation call.
        let p = unsafe { LZ4_createStreamDecode() };
        if p.is_null() {
            None
        } else {
            Some(Lz4Stream(p))
        }
    }

    /// `LZ4_decompress_safe_continue`: `src` and `dst` must stay valid while the stream continues to use them.
    pub fn decompress_continue(&mut self, src: &[u8], dst: &mut [u8], dst_capacity: c_int) -> c_int {
        // SAFETY: `src` is readable for `src.len()` bytes and `dst` writable for `dst_capacity` bytes (checked by callers).
        unsafe {
            LZ4_decompress_safe_continue(self.0, src.as_ptr().cast(), dst.as_mut_ptr().cast(), src.len() as c_int, dst_capacity)
        }
    }
}

impl Drop for Lz4Stream {
    fn drop(&mut self) {
        // SAFETY: the stream came from `LZ4_createStreamDecode` and is freed once.
        unsafe {
            LZ4_freeStreamDecode(self.0);
        }
    }
}

// ---- zstd ----

#[repr(C)]
struct ZstdInBuffer {
    src: *const c_void,
    size: usize,
    pos: usize,
}

#[repr(C)]
struct ZstdOutBuffer {
    dst: *mut c_void,
    size: usize,
    pos: usize,
}

extern "C" {
    fn ZSTD_createDStream() -> *mut c_void;
    fn ZSTD_freeDStream(zds: *mut c_void) -> usize;
    fn ZSTD_initDStream(zds: *mut c_void) -> usize;
    fn ZSTD_decompressStream(zds: *mut c_void, output: *mut ZstdOutBuffer, input: *mut ZstdInBuffer) -> usize;
    fn ZSTD_isError(code: usize) -> u32;
    fn ZSTD_DStreamInSize() -> usize;
}

/// `ZSTD_DStreamInSize()`
pub fn zstd_dstream_in_size() -> usize {
    // SAFETY: pure function.
    unsafe { ZSTD_DStreamInSize() }
}

/// `ZSTD_DStream*`
pub struct ZstdStream(*mut c_void);

impl ZstdStream {
    pub fn new() -> Option<ZstdStream> {
        // SAFETY: plain allocation call.
        let p = unsafe { ZSTD_createDStream() };
        if p.is_null() {
            None
        } else {
            Some(ZstdStream(p))
        }
    }

    pub fn init(&mut self) {
        // SAFETY: the stream is valid.
        unsafe {
            ZSTD_initDStream(self.0);
        }
    }

    /// `ZSTD_decompressStream(strm, &out, &in)` with `in = { input, input.len(), pos }` and
    /// `out = { output, output.len(), out_pos }`; `Ok((in.pos, out.pos, ret))` or `Err(())` for an error code.
    pub fn decompress(&mut self, output: &mut [u8], out_pos: usize, input: &[u8], pos: usize) -> Result<(usize, usize, usize), ()> {
        let mut out = ZstdOutBuffer { dst: output.as_mut_ptr().cast(), size: output.len(), pos: out_pos };
        let mut inp = ZstdInBuffer { src: input.as_ptr().cast(), size: input.len(), pos };
        // SAFETY: the buffers describe valid memory of the given sizes for the duration of the call.
        let ret = unsafe { ZSTD_decompressStream(self.0, &mut out, &mut inp) };
        // SAFETY: pure function.
        if unsafe { ZSTD_isError(ret) } != 0 {
            Err(())
        } else {
            Ok((inp.pos, out.pos, ret))
        }
    }
}

impl Drop for ZstdStream {
    fn drop(&mut self) {
        // SAFETY: the stream came from `ZSTD_createDStream` and is freed once.
        unsafe {
            ZSTD_freeDStream(self.0);
        }
    }
}

// ---- viizip (7zip) ----

#[cfg(windows)]
type SSize = c_int; // viizip.h: `typedef int ssize_t` on Windows
#[cfg(not(windows))]
type SSize = isize;

extern "C" {
    fn viinew(file: *mut c_void) -> *mut c_void;
    fn viifree(vii: *mut c_void);
    fn viiget(vii: *mut c_void, name: *mut c_char, max: usize, mtime: *mut i64, usize: *mut u64) -> c_int;
    fn viidec(vii: *mut c_void, buf: *mut u8, len: usize) -> SSize;
}

/// `struct viizip*`
pub struct Viizip(*mut c_void);

impl Viizip {
    /// `viinew(file)`, `None` for a non-seekable or password-protected 7zip file.
    pub fn new(file: *mut c_void) -> Option<Viizip> {
        // SAFETY: `file` is the caller's valid `FILE*`, which outlives the decompressor.
        let p = unsafe { viinew(file) };
        if p.is_null() {
            None
        } else {
            Some(Viizip(p))
        }
    }

    /// `viiget(..., name buffer of max bytes, &mtime, &usize)`: 0 if success, 1 when the end was reached, -1 on error.
    pub fn get(&mut self, name: &mut [u8], mtime: &mut i64, usize: &mut u64) -> c_int {
        // SAFETY: `name` is writable for `name.len()` bytes; the pointers are valid.
        unsafe { viiget(self.0, name.as_mut_ptr().cast(), name.len(), mtime, usize) }
    }

    pub fn decompress(&mut self, buf: &mut [u8]) -> i64 {
        // SAFETY: `buf` is writable for `buf.len()` bytes.
        unsafe { viidec(self.0, buf.as_mut_ptr(), buf.len()) as i64 }
    }
}

impl Drop for Viizip {
    fn drop(&mut self) {
        // SAFETY: the decompressor came from `viinew` and is freed once.
        unsafe { viifree(self.0) }
    }
}
