//! C runtime calls of the replay harness (FILE streams and errno), the only unsafe code of the harness.
//! The harness hands the Rust `zopen` a real `FILE*` and observes what it left in the file and in errno,
//! exactly as `migration/harness/src_leaves/zopen_driver.cpp` does for the C++.
use core::ffi::{c_char, c_int, c_long, c_void};

extern "C" {
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fclose(f: *mut c_void) -> c_int;
    fn fflush(f: *mut c_void) -> c_int;
    fn fseek(f: *mut c_void, off: c_long, whence: c_int) -> c_int;
    fn ftell(f: *mut c_void) -> c_long;
    fn fread(buf: *mut c_void, size: usize, n: usize, f: *mut c_void) -> usize;
    fn fwrite(buf: *const c_void, size: usize, n: usize, f: *mut c_void) -> usize;
    fn _errno() -> *mut c_int;
}

pub fn set_errno(v: i32) {
    unsafe { *_errno() = v }
}

pub fn errno() -> i32 {
    unsafe { *_errno() }
}

/// Creates the scratch FILE holding `data`, positioned at 0; read-only if `ro`.
pub fn open_scratch(path: &str, data: &[u8], ro: bool) -> *mut c_void {
    let p = std::ffi::CString::new(path).unwrap();
    unsafe {
        let mut f = fopen(p.as_ptr(), c"w+b".as_ptr());
        assert!(!f.is_null(), "cannot create {path}");
        if !data.is_empty() {
            fwrite(data.as_ptr().cast(), 1, data.len(), f);
        }
        fflush(f);
        fseek(f, 0, 0);
        if ro {
            fclose(f);
            f = fopen(p.as_ptr(), c"rb".as_ptr());
            assert!(!f.is_null(), "cannot reopen {path}");
        }
        f
    }
}

/// `[ftell]`, or `[ftell, "bytes of the FILE"]` if `with_data`, as the neutral JSON the C++ driver writes.
pub fn end_state(f: *mut c_void, with_data: bool, quote: impl Fn(&[u8]) -> String) -> String {
    unsafe {
        fflush(f);
        let pos = ftell(f);
        let mut s = format!("[{pos}");
        if with_data {
            fseek(f, 0, 2);
            let n = ftell(f);
            fseek(f, 0, 0);
            let mut data = vec![0u8; n.max(0) as usize];
            if !data.is_empty() && fread(data.as_mut_ptr().cast(), 1, data.len(), f) != data.len() {
                data.clear();
            }
            s.push(',');
            s.push_str(&quote(&data));
        }
        s.push(']');
        s
    }
}

pub fn close(f: *mut c_void) {
    unsafe {
        fclose(f);
    }
}
