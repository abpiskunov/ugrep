//! The C runtime calls of the Zthread unit (pipes, `fdopen`, `fclose`) as safe functions, and the two wrappers that
//! carry a raw `FILE*` or a stream between threads. Every `unsafe` of the decompression thread (`zthread_core.rs`)
//! is here, next to the C-ABI entries of `ffi/zthread.rs`.

use std::ffi::{c_char, c_int, c_void};

/// A C `FILE*`.
pub type FilePtr = *mut c_void;

#[cfg(windows)]
mod c {
    use std::ffi::{c_char, c_int, c_uint, c_void};

    extern "C" {
        pub fn _pipe(fds: *mut c_int, size: c_uint, mode: c_int) -> c_int;
        pub fn _close(fd: c_int) -> c_int;
        pub fn _write(fd: c_int, buf: *const c_void, count: c_uint) -> c_int;
        pub fn _fdopen(fd: c_int, mode: *const c_char) -> *mut c_void;
        pub fn fclose(file: *mut c_void) -> c_int;
        #[cfg(test)]
        pub fn _read(fd: c_int, buf: *mut c_void, count: c_uint) -> c_int;
    }
}

#[cfg(not(windows))]
mod c {
    use std::ffi::{c_char, c_int, c_void};

    extern "C" {
        pub fn pipe(fds: *mut c_int) -> c_int;
        pub fn close(fd: c_int) -> c_int;
        pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        pub fn fdopen(fd: c_int, mode: *const c_char) -> *mut c_void;
        pub fn fclose(file: *mut c_void) -> c_int;
        #[cfg(test)]
        pub fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    }
}

/// `pipe()` of ugrep.hpp: `(read end, write end)`. On Windows a binary, non-inherited pipe with the default buffer.
pub fn pipe() -> Option<(i32, i32)> {
    let mut fds: [c_int; 2] = [-1, -1];
    #[cfg(windows)]
    // SAFETY: `fds` has room for the two descriptors.
    let rc = unsafe { c::_pipe(fds.as_mut_ptr(), 0, 0x8000 | 0x80) };
    #[cfg(not(windows))]
    // SAFETY: `fds` has room for the two descriptors.
    let rc = unsafe { c::pipe(fds.as_mut_ptr()) };
    (rc == 0).then_some((fds[0], fds[1]))
}

/// `close(fd)`; a negative descriptor is ignored.
pub fn close(fd: i32) {
    if fd < 0 {
        return;
    }
    #[cfg(windows)]
    // SAFETY: closing a descriptor this unit owns; an invalid one only makes the call fail.
    unsafe {
        c::_close(fd)
    };
    #[cfg(not(windows))]
    // SAFETY: as above.
    unsafe {
        c::close(fd)
    };
}

/// `write(fd, buf, len)`: the number of bytes written, negative on error (a broken pipe).
pub fn write(fd: i32, buf: &[u8]) -> isize {
    if buf.is_empty() {
        return 0;
    }
    #[cfg(windows)]
    {
        let count = buf.len().min(i32::MAX as usize) as u32;
        // SAFETY: `buf` is valid for `count` bytes.
        unsafe { c::_write(fd, buf.as_ptr().cast(), count) as isize }
    }
    #[cfg(not(windows))]
    {
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        unsafe { c::write(fd, buf.as_ptr().cast(), buf.len()) }
    }
}

/// An open `FILE*` (the read end of a pipe through `fdopen(fd, "rb")`), closed with `fclose` when dropped, which
/// closes the descriptor.
pub struct CFile(FilePtr);

impl CFile {
    /// `fdopen(fd, "rb")`; on failure the descriptor is still open and owned by the caller.
    pub fn from_fd(fd: i32) -> Option<CFile> {
        #[cfg(windows)]
        // SAFETY: the mode is a NUL-terminated string.
        let file = unsafe { c::_fdopen(fd, c"rb".as_ptr() as *const c_char) };
        #[cfg(not(windows))]
        // SAFETY: the mode is a NUL-terminated string.
        let file = unsafe { c::fdopen(fd, c"rb".as_ptr() as *const c_char) };
        (!file.is_null()).then_some(CFile(file))
    }

    pub fn as_ptr(&self) -> FilePtr {
        self.0
    }
}

impl Drop for CFile {
    fn drop(&mut self) {
        // SAFETY: the `FILE*` came from `fdopen` and is closed once, here.
        unsafe { c::fclose(self.0) };
    }
}

/// Moves a value that holds a raw `FILE*` to the decompression thread.
///
/// SAFETY of the `Send` impl: the wrapped value (a zstream over a `FILE*` that the C++ caller keeps open until the next
/// `start`/`close`, and the `FILE*` of a pipe owned by the value) is built on the caller thread, handed over once under
/// a mutex or by `thread::spawn`, and from then on used by the decompression thread only, until it has been joined.
pub struct AssertSend<T>(T);

impl<T> AssertSend<T> {
    pub fn new(value: T) -> AssertSend<T> {
        AssertSend(value)
    }

    /// Taking the value by method makes a closure capture the whole wrapper, not its non-`Send` field.
    pub fn into_inner(self) -> T {
        self.0
    }
}

// SAFETY: see the type documentation.
unsafe impl<T> Send for AssertSend<T> {}

#[cfg(test)]
pub fn read(fd: i32, buf: &mut [u8]) -> isize {
    #[cfg(windows)]
    // SAFETY: `buf` is valid for `buf.len()` bytes.
    unsafe {
        c::_read(fd, buf.as_mut_ptr().cast(), buf.len() as u32) as isize
    }
    #[cfg(not(windows))]
    // SAFETY: `buf` is valid for `buf.len()` bytes.
    unsafe {
        c::read(fd, buf.as_mut_ptr().cast(), buf.len())
    }
}
