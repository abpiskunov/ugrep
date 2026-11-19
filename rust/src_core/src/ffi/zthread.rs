//! C-ABI entries of the Zthread seam for src/ugrep.cpp (declared in src/zthread_seam.h): an opaque handle owned by
//! Rust, a table of C callbacks for what stays in C++ (`Grep::partname`, `warning()`, `cannot_decompress()`, the flags
//! part of `select_matching()`), and the pipe read end handed back as a file descriptor. The decompression thread is
//! `zthread_core.rs`; the C runtime calls it makes are in `zthread_sys.rs`.

use crate::signatures::zthread::{Host, Outcome, ZstreamSettings};
use crate::zthread_core::Zthread;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

const ZTHREAD_OK: c_int = 0;
const ZTHREAD_NONE: c_int = 1;

/// Mirrors `struct ugrep_core_zthread_env` of src/zthread_seam.h.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ZthreadEnv {
    pub ctx: *mut c_void,
    pub set_partname: extern "C" fn(*mut c_void, *const c_char, usize),
    pub warning: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub cannot_decompress: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub archive_found: extern "C" fn(*mut c_void),
    pub select_by_flags: extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> c_int,
    pub zs_codecs: u32,
    pub zs_zip_crc32: c_int,
    pub zs_max_7zip_size: u64,
}

/// The C++ facade behind the callbacks table.
struct CHost(ZthreadEnv);

// SAFETY: `ctx` is the C++ facade object, which outlives the handle (it closes the handle in its destructor) and
// whose callbacks are written to be called from the decompression threads.
unsafe impl Send for CHost {}
// SAFETY: as above, the callbacks take no `&mut` state of this side.
unsafe impl Sync for CHost {}

/// The bytes as a NUL-terminated copy for a callback; bytes after an interior NUL are dropped, as C would read them.
fn c_string(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = bytes[..end].to_vec();
    v.push(0);
    v
}

impl Host for CHost {
    fn set_partname(&self, name: &[u8]) {
        (self.0.set_partname)(self.0.ctx, name.as_ptr().cast(), name.len());
    }

    fn warning(&self, message: &[u8], arg: Option<&[u8]>) {
        let message = c_string(message);
        let arg = arg.map(c_string);
        let arg_ptr = arg.as_ref().map_or(std::ptr::null(), |a| a.as_ptr().cast());
        (self.0.warning)(self.0.ctx, message.as_ptr().cast(), arg_ptr);
    }

    fn cannot_decompress(&self, pathname: &[u8], message: &[u8]) {
        let pathname = c_string(pathname);
        let message = c_string(message);
        (self.0.cannot_decompress)(self.0.ctx, pathname.as_ptr().cast(), message.as_ptr().cast());
    }

    fn archive_found(&self) {
        (self.0.archive_found)(self.0.ctx);
    }

    fn select_by_flags(&self, path: &[u8], head: &[u8]) -> bool {
        let path = c_string(path);
        (self.0.select_by_flags)(self.0.ctx, path.as_ptr().cast(), head.as_ptr(), head.len()) != 0
    }

    fn zstream_config(&self) -> ZstreamSettings {
        ZstreamSettings { codecs: self.0.zs_codecs, zip_crc32: self.0.zs_zip_crc32 != 0, max_7zip_size: self.0.zs_max_7zip_size }
    }
}

/// Bytes of a C string, `None` for NULL.
///
/// # Safety
/// `p` is NULL or a NUL-terminated string that outlives the returned slice.
unsafe fn bytes<'a>(p: *const c_char) -> Option<&'a [u8]> {
    if p.is_null() {
        None
    } else {
        // SAFETY: see above.
        Some(unsafe { CStr::from_ptr(p) }.to_bytes())
    }
}

/// Stores the outcome in `*file` and returns the `ZTHREAD_*` status.
///
/// # Safety
/// `file` is valid for a write.
unsafe fn report(outcome: Outcome, file: *mut *mut c_void) -> c_int {
    let (status, value) = match outcome {
        Outcome::Pipe(read_end) => (ZTHREAD_OK, read_end),
        Outcome::None => (ZTHREAD_NONE, std::ptr::null_mut()),
    };
    // SAFETY: see above.
    unsafe { *file = value };
    status
}

/// Runs `f` so that a panic never crosses into C++: the status is then `ZTHREAD_NONE`.
fn guarded(f: impl FnOnce() -> c_int) -> c_int {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(ZTHREAD_NONE)
}

/// `ZthreadCpp::ZthreadCpp`.
///
/// # Safety
/// `env` is a valid table whose `ctx` outlives the returned handle, which is released only by
/// [`ugrep_core_zthread_close`].
#[export_name = "ugrep_core_zthread_new"]
pub unsafe extern "C" fn ugrep_core_zthread_new(env: *const ZthreadEnv, is_chained: c_int) -> *mut c_void {
    // SAFETY: the caller passes a valid table; it is copied.
    let host: Arc<dyn Host> = Arc::new(CHost(unsafe { *env }));
    Box::into_raw(Box::new(Zthread::new(host, is_chained != 0))).cast()
}

/// `ZthreadCpp::start`.
///
/// # Safety
/// `handle` comes from [`ugrep_core_zthread_new`] and is not closed; start/open_next are called by the one search
/// thread, and [`ugrep_core_zthread_cancel`] may run on any thread at the same time, so only a shared `&Zthread`
/// is formed here; `pathname` is
/// NUL-terminated, `find` is NULL or NUL-terminated, `file_in` is a `FILE*` that stays open until the next call
/// of this function or [`ugrep_core_zthread_close`]; `file` is valid for a write and receives the read end of the
/// pipe as an open `FILE*` that the caller closes with `fclose`.
#[export_name = "ugrep_core_zthread_start"]
pub unsafe extern "C" fn ugrep_core_zthread_start(
    handle: *mut c_void,
    ztstage: usize,
    pathname: *const c_char,
    file_in: *mut c_void,
    find: *const c_char,
    file: *mut *mut c_void,
) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let (zthread, pathname, find) = unsafe { (&*handle.cast::<Zthread>(), bytes(pathname).unwrap_or(b""), bytes(find)) };
        let outcome = zthread.start(ztstage, pathname, file_in, find);
        // SAFETY: see above.
        unsafe { report(outcome, file) }
    })
}

/// `ZthreadCpp::open_next`.
///
/// # Safety
/// As [`ugrep_core_zthread_start`].
#[export_name = "ugrep_core_zthread_open_next"]
pub unsafe extern "C" fn ugrep_core_zthread_open_next(handle: *mut c_void, pathname: *const c_char, file: *mut *mut c_void) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let (zthread, pathname) = unsafe { (&*handle.cast::<Zthread>(), bytes(pathname).unwrap_or(b"")) };
        let outcome = zthread.open_next(pathname);
        // SAFETY: see above.
        unsafe { report(outcome, file) }
    })
}

/// `ZthreadCpp::cancel`.
///
/// # Safety
/// `handle` comes from [`ugrep_core_zthread_new`] and is not closed during the call; may run on any thread, also
/// while another thread is inside start/open_next.
#[export_name = "ugrep_core_zthread_cancel"]
pub unsafe extern "C" fn ugrep_core_zthread_cancel(handle: *mut c_void) {
    guarded(|| {
        // SAFETY: see above; a shared reference only, `cancel` sets an atomic flag read by the decompression threads.
        unsafe { &*handle.cast::<Zthread>() }.cancel();
        0
    });
}

/// `~ZthreadCpp`: joins the decompression threads and frees the handle.
///
/// # Safety
/// `handle` is NULL or comes from [`ugrep_core_zthread_new`]; it is not used afterwards.
#[export_name = "ugrep_core_zthread_close"]
pub unsafe extern "C" fn ugrep_core_zthread_close(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    guarded(|| {
        // SAFETY: see above.
        let mut zthread = unsafe { Box::from_raw(handle.cast::<Zthread>()) };
        zthread.close();
        0
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn set_partname(_ctx: *mut c_void, _name: *const c_char, _len: usize) {}
    extern "C" fn warning(_ctx: *mut c_void, _message: *const c_char, _arg: *const c_char) {}
    extern "C" fn cannot_decompress(_ctx: *mut c_void, _pathname: *const c_char, _message: *const c_char) {}
    extern "C" fn archive_found(_ctx: *mut c_void) {}
    extern "C" fn select_by_flags(_ctx: *mut c_void, _path: *const c_char, _buf: *const u8, _len: usize) -> c_int {
        1
    }

    #[test]
    fn handle_round_trip() {
        let env = ZthreadEnv {
            ctx: std::ptr::null_mut(),
            set_partname,
            warning,
            cannot_decompress,
            archive_found,
            select_by_flags,
            zs_codecs: 1,
            zs_zip_crc32: 0,
            zs_max_7zip_size: 0,
        };
        // SAFETY: the table is valid, the handle is used by this thread only and closed once; the strings are
        // NUL-terminated and `file` is a local. There is no input file, so the thread has nothing to write.
        unsafe {
            let handle = ugrep_core_zthread_new(&env, 0);
            let mut file: *mut c_void = std::ptr::null_mut();
            assert_eq!(ugrep_core_zthread_start(handle, 1, c"a.gz".as_ptr(), std::ptr::null_mut(), std::ptr::null(), &mut file), ZTHREAD_OK);
            let mut buf = [0u8; 8];
            let stream = crate::zthread_sys::CFile::from_raw(file);
            assert_eq!(stream.read(&mut buf), 0);
            drop(stream);
            assert_eq!(ugrep_core_zthread_open_next(handle, c"a.gz".as_ptr(), &mut file), ZTHREAD_NONE);
            assert!(file.is_null());
            ugrep_core_zthread_cancel(handle);
            ugrep_core_zthread_close(handle);
            ugrep_core_zthread_close(std::ptr::null_mut());
        }
    }
}
