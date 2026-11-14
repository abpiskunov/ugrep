//! C runtime calls used by the unit `zopen` (`FILE*` streams and errno). Unsafe code of the unit lives here only.
//! Reached from `zopen.rs` by `#[path]`, so it also builds inside the replay harness.
use core::ffi::{c_int, c_void};

extern "C" {
    fn fread(buf: *mut c_void, size: usize, n: usize, f: *mut c_void) -> usize;
    fn fwrite(buf: *const c_void, size: usize, n: usize, f: *mut c_void) -> usize;
    #[cfg(windows)]
    fn _errno() -> *mut c_int;
    #[cfg(target_os = "linux")]
    fn __errno_location() -> *mut c_int;
    #[cfg(target_vendor = "apple")]
    fn __error() -> *mut c_int;
    #[cfg(test)]
    fn tmpfile() -> *mut c_void;
    #[cfg(test)]
    fn fclose(f: *mut c_void) -> c_int;
    #[cfg(test)]
    fn rewind(f: *mut c_void);
}

/// `EINVAL` of the C runtime (22 on Windows, Linux and macOS).
pub const EINVAL: c_int = 22;

pub fn set_errno(v: c_int) {
    // SAFETY: the C runtime returns the address of the calling thread's errno, valid for the thread's life.
    unsafe {
        #[cfg(windows)]
        {
            *_errno() = v;
        }
        #[cfg(target_os = "linux")]
        {
            *__errno_location() = v;
        }
        #[cfg(target_vendor = "apple")]
        {
            *__error() = v;
        }
    }
}

/// `fread(buf, 1, buf.len(), file)`: bytes read.
pub fn read(file: *mut c_void, buf: &mut [u8]) -> usize {
    // SAFETY: `buf` is writable for `buf.len()` bytes; `file` is the caller's valid `FILE*` (contract of `z_open`).
    unsafe { fread(buf.as_mut_ptr().cast(), 1, buf.len(), file) }
}

/// `fwrite(buf, 1, buf.len(), file)`: bytes written.
pub fn write(file: *mut c_void, buf: &[u8]) -> usize {
    // SAFETY: `buf` is readable for `buf.len()` bytes; `file` is the caller's valid `FILE*` (contract of `z_open`).
    unsafe { fwrite(buf.as_ptr().cast(), 1, buf.len(), file) }
}

#[cfg(test)]
pub fn temp_file() -> *mut c_void {
    // SAFETY: plain C call without arguments.
    unsafe { tmpfile() }
}

#[cfg(test)]
pub fn rewind_file(file: *mut c_void) {
    // SAFETY: `file` came from `temp_file` and is still open.
    unsafe { rewind(file) }
}

#[cfg(test)]
pub fn close_file(file: *mut c_void) {
    // SAFETY: `file` came from `temp_file` and is closed once.
    unsafe {
        fclose(file);
    }
}
