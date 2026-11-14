//! C-ABI boundary of the unit `zopen` (seam ids `src_leaves.z_open|z_close|z_read|z_write`). It takes a
//! `FILE*` and a `void*` handle, which cxx cannot express, so it is plain `extern "C"` declared in
//! src/zopen_seam.h. The handle is a leaked `Box<ZState>` between `z_open` and `z_close`.
use core::ffi::{c_char, c_int, c_void};
use crate::signatures::zopen::{self, ZState};

/// # Safety
/// `mode` is NULL or a NUL-terminated string; `file` is a valid `FILE*` for the whole stream.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_z_open(file: *mut c_void, mode: *const c_char, bits: c_int, plain: c_int) -> *mut c_void {
    let mode: &[u8] = if mode.is_null() { &[] } else { core::ffi::CStr::from_ptr(mode).to_bytes() };
    match zopen::z_open(file, mode, bits, plain) {
        Some(state) => Box::into_raw(state).cast(),
        None => core::ptr::null_mut(),
    }
}

/// # Safety
/// `handle` came from `ugrep_rust_z_open` and is not used again.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_z_close(handle: *mut c_void) -> c_int {
    if handle.is_null() {
        return -1;
    }
    zopen::z_close(*Box::from_raw(handle.cast::<ZState>()))
}

/// # Safety
/// `handle` is a live handle; `buf` is writable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_z_read(handle: *mut c_void, buf: *mut u8, len: c_int) -> c_int {
    if handle.is_null() || len < 0 || (buf.is_null() && len > 0) {
        return -1;
    }
    let buf: &mut [u8] = if len == 0 { &mut [] } else { core::slice::from_raw_parts_mut(buf, len as usize) };
    zopen::z_read(&mut *handle.cast::<ZState>(), buf)
}

/// # Safety
/// `handle` is a live handle; `buf` is readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn ugrep_rust_z_write(handle: *mut c_void, buf: *const u8, len: c_int) -> c_int {
    if handle.is_null() || len < 0 || (buf.is_null() && len > 0) {
        return -1;
    }
    let buf: &[u8] = if len == 0 { &[] } else { core::slice::from_raw_parts(buf, len as usize) };
    zopen::z_write(&mut *handle.cast::<ZState>(), buf)
}
