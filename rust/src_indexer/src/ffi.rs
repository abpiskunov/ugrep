//! The platform calls of the unit `indexer_walk`: CRT stdout and `strerror`, and the Win32 directory listing.
//! Every `unsafe` of the unit is here; the wrappers are safe functions.

#[cfg(windows)]
pub mod sys {
    use std::ffi::{c_char, c_void, CStr};

    extern "C" {
        fn __acrt_iob_func(index: u32) -> *mut c_void;
        fn fwrite(buf: *const c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
        fn fflush(stream: *mut c_void) -> i32;
        fn strerror(errnum: i32) -> *const c_char;
        fn _wremove(path: *const u16) -> i32;
        fn _errno() -> *mut i32;
    }

    /// CRT `remove()`: the error is the `errno` of the failed call
    pub fn remove_file(path: &[u8]) -> Result<(), i32> {
        let wpath = super::win::wide(path);
        // SAFETY: the wide name is NUL-terminated; `_errno` returns a valid thread-local pointer
        unsafe {
            if _wremove(wpath.as_ptr()) == 0 {
                Ok(())
            } else {
                Err(*_errno())
            }
        }
    }

    pub fn write_stdout(bytes: &[u8]) {
        // SAFETY: stdout of the CRT, `bytes` is valid for its length
        unsafe {
            fwrite(bytes.as_ptr().cast(), 1, bytes.len(), __acrt_iob_func(1));
        }
    }

    pub fn flush_stdout() {
        // SAFETY: stdout of the CRT
        unsafe {
            fflush(__acrt_iob_func(1));
        }
    }

    pub fn strerror_bytes(errnum: i32) -> Vec<u8> {
        // SAFETY: strerror returns a NUL-terminated string that stays valid until the next call
        unsafe { CStr::from_ptr(strerror(errnum)).to_bytes().to_vec() }
    }
}

#[cfg(not(windows))]
pub mod sys {
    use std::ffi::{c_char, CStr};
    use std::io::Write;

    extern "C" {
        fn strerror(errnum: i32) -> *const c_char;
    }

    /// `remove()`: the error is the `errno` of the failed call
    pub fn remove_file(path: &[u8]) -> Result<(), i32> {
        use std::os::unix::ffi::OsStrExt;
        std::fs::remove_file(std::ffi::OsStr::from_bytes(path)).map_err(|e| e.raw_os_error().unwrap_or(22))
    }

    pub fn write_stdout(bytes: &[u8]) {
        let _ = std::io::stdout().write_all(bytes);
    }

    pub fn flush_stdout() {
        let _ = std::io::stdout().flush();
    }

    pub fn strerror_bytes(errnum: i32) -> Vec<u8> {
        // SAFETY: strerror returns a NUL-terminated string that stays valid until the next call
        unsafe { CStr::from_ptr(strerror(errnum)).to_bytes().to_vec() }
    }
}

#[cfg(windows)]
pub mod win {
    pub const ERROR_FILE_NOT_FOUND: u32 = 2;
    pub const ERROR_ACCESS_DENIED: i32 = 5;
    pub const INVALID_FILE_ATTRIBUTES: u32 = 0xffff_ffff;
    pub const ATTR_HIDDEN: u32 = 0x2;
    pub const ATTR_SYSTEM: u32 = 0x4;
    pub const ATTR_DIRECTORY: u32 = 0x10;
    pub const ATTR_DEVICE: u32 = 0x40;
    pub const ATTR_REPARSE_POINT: u32 = 0x400;
    pub const IO_REPARSE_TAG_SYMLINK: u32 = 0xa000_000c;

    const INVALID_HANDLE_VALUE: isize = -1;

    /// `WIN32_FIND_DATAW`
    #[repr(C)]
    pub struct FindData {
        pub attributes: u32,
        pub creation_time: [u32; 2],
        pub access_time: [u32; 2],
        pub write_time_low: u32,
        pub write_time_high: u32,
        pub size_high: u32,
        pub size_low: u32,
        pub reserved0: u32,
        pub reserved1: u32,
        pub name: [u16; 260],
        pub alternate_name: [u16; 14],
    }

    impl FindData {
        pub fn new() -> FindData {
            FindData {
                attributes: 0,
                creation_time: [0; 2],
                access_time: [0; 2],
                write_time_low: 0,
                write_time_high: 0,
                size_high: 0,
                size_low: 0,
                reserved0: 0,
                reserved1: 0,
                name: [0; 260],
                alternate_name: [0; 14],
            }
        }

        /// `utf8_encode(cFileName)`
        pub fn file_name(&self) -> Vec<u8> {
            let n = self.name.iter().position(|&c| c == 0).unwrap_or(self.name.len());
            String::from_utf16_lossy(&self.name[..n]).into_bytes()
        }

        pub fn modified_time(&self) -> u64 {
            u64::from(self.write_time_low) | (u64::from(self.write_time_high) << 32)
        }

        pub fn file_size(&self) -> u64 {
            u64::from(self.size_low) | (u64::from(self.size_high) << 32)
        }
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn FindFirstFileW(name: *const u16, data: *mut FindData) -> isize;
        fn FindNextFileW(handle: isize, data: *mut FindData) -> i32;
        fn FindClose(handle: isize) -> i32;
        fn GetFileAttributesW(name: *const u16) -> u32;
        fn GetLastError() -> u32;
    }

    /// An open `FindFirstFileW` search
    pub struct Find(isize);

    impl Find {
        /// `FindFirstFileW(glob, data)`; the error is `GetLastError()`
        pub fn first(glob: &[u8], data: &mut FindData) -> Result<Find, u32> {
            let wglob = wide(glob);
            // SAFETY: `wglob` is NUL-terminated and `data` is a valid WIN32_FIND_DATAW
            let handle = unsafe { FindFirstFileW(wglob.as_ptr(), data) };
            if handle == INVALID_HANDLE_VALUE {
                // SAFETY: no pointers
                Err(unsafe { GetLastError() })
            } else {
                Ok(Find(handle))
            }
        }

        /// `FindNextFileW`: false at the end of the directory
        pub fn next(&mut self, data: &mut FindData) -> bool {
            // SAFETY: the handle is a valid find handle and `data` a valid WIN32_FIND_DATAW
            unsafe { FindNextFileW(self.0, data) != 0 }
        }
    }

    impl Drop for Find {
        fn drop(&mut self) {
            // SAFETY: the handle is a valid find handle
            unsafe {
                FindClose(self.0);
            }
        }
    }

    /// `GetFileAttributesW(path)`
    pub fn file_attributes(path: &[u8]) -> u32 {
        let wpath = wide(path);
        // SAFETY: the wide name is NUL-terminated
        unsafe { GetFileAttributesW(wpath.as_ptr()) }
    }

    /// `utf8_decode()` plus the terminating NUL: invalid UTF-8 becomes U+FFFD as with `MultiByteToWideChar`
    pub fn wide(path: &[u8]) -> Vec<u16> {
        String::from_utf8_lossy(path).encode_utf16().chain(std::iter::once(0)).collect()
    }
}
