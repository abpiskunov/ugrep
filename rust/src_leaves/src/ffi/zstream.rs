//! C-ABI boundary of the units `zstream_core` and `zstream_codecs` (seam ids `src_leaves.zs_open|zs_close|
//! zs_decompressing|zs_next|zs_zipinfo|zs_zippeek`). It takes a `FILE*` and hands out a Rust-owned handle, which cxx
//! cannot express, so it is plain `extern "C"` declared in src/zstream_seam.h. The handle is a leaked `Box<ZStream>`
//! between `ugrep_rust_zs_open` and `ugrep_rust_zs_close`. The filename predicates are in the cxx bridge (lib.rs).
use core::ffi::{c_char, c_int, c_void};

use crate::signatures::zstream_codecs::Config;
use crate::signatures::zstream_core::ZStream;

#[repr(C)]
pub struct ZsConfig {
    codecs: u32,
    zip_crc32: u32,
    max_7zip_size: u64,
}

#[repr(C)]
pub struct ZsZipInfo {
    name: *const c_char,
    name_len: usize,
    mtime: i64,
    size: u64,
    usize: u64,
    crc: u32,
    version: u16,
    flag: u16,
    method: u16,
}

/// # Safety
/// `pathname` is a NUL-terminated string, `file` a valid `FILE*` for the whole life of the handle, `config` is NULL
/// or valid for the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_open(pathname: *const c_char, file: *mut c_void, config: *const ZsConfig) -> *mut c_void {
    if file.is_null() {
        return core::ptr::null_mut();
    }
    let pathname: &[u8] = if pathname.is_null() { &[] } else { core::ffi::CStr::from_ptr(pathname).to_bytes() };
    let config = match config.as_ref() {
        Some(c) => Config { codecs: c.codecs, zip_crc32: c.zip_crc32 != 0, max_7zip_size: c.max_7zip_size },
        None => Config::default(),
    };
    Box::into_raw(Box::new(ZStream::open(pathname, file, config))).cast()
}

/// # Safety
/// `handle` is NULL or came from `ugrep_rust_zs_open` and is not used again.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_close(handle: *mut c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(handle.cast::<ZStream>()));
    }
}

/// # Safety
/// `handle` is NULL or a live handle.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_decompressing(handle: *const c_void) -> c_int {
    match handle.cast::<ZStream>().as_ref() {
        Some(s) => s.decompressing() as c_int,
        None => 0,
    }
}

/// # Safety
/// `handle` is NULL or a live handle; `buf` is writable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_next(handle: *mut c_void, buf: *mut u8, len: usize) -> isize {
    let Some(s) = handle.cast::<ZStream>().as_mut() else { return 0 };
    let buf: &mut [u8] = if len == 0 || buf.is_null() { &mut [] } else { core::slice::from_raw_parts_mut(buf, len) };
    s.next(buf)
}

/// # Safety
/// `handle` is NULL or a live handle.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_at_end(handle: *const c_void) -> c_int {
    match handle.cast::<ZStream>().as_ref() {
        Some(s) => s.at_end as c_int,
        None => 1,
    }
}

/// # Safety
/// `handle` is NULL or a live handle; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_zipinfo(handle: *mut c_void, out: *mut ZsZipInfo) -> c_int {
    let (Some(s), false) = (handle.cast::<ZStream>().as_mut(), out.is_null()) else { return 0 };
    match s.zipinfo() {
        Some(info) => {
            *out = ZsZipInfo {
                name: info.name.as_ptr().cast(),
                name_len: info.name.len(),
                mtime: info.mtime,
                size: info.size,
                usize: info.usize,
                crc: info.crc,
                version: info.version,
                flag: info.flag,
                method: info.method,
            };
            1
        }
        None => 0,
    }
}

/// # Safety
/// `handle` is NULL or a live handle; `ptr` and `len` are writable.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_zippeek(handle: *mut c_void, ptr: *mut *const u8, len: *mut usize) -> c_int {
    let (Some(s), false, false) = (handle.cast::<ZStream>().as_mut(), ptr.is_null(), len.is_null()) else { return 0 };
    match s.zippeek() {
        Some(data) => {
            *ptr = data.as_ptr();
            *len = data.len();
            1
        }
        None => 0,
    }
}

/// # Safety
/// `handle` is NULL or a live handle; `kind` and `message` are writable.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_zs_diag(handle: *mut c_void, kind: *mut c_int, message: *mut *const c_char) -> c_int {
    let (Some(s), false, false) = (handle.cast::<ZStream>().as_mut(), kind.is_null(), message.is_null()) else { return 0 };
    match s.diag.queue.pop_front() {
        Some((k, mut text)) => {
            text.push(0);
            s.last_message = text;
            *kind = k as c_int;
            *message = s.last_message.as_ptr().cast();
            1
        }
        None => 0,
    }
}
