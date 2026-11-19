//! C-ABI entries of the Zthread seam for src/ugrep.cpp (declared in src/zthread_seam.h): an opaque handle owned by
//! Rust, a table of C callbacks for what stays in C++ (`Grep::partname`, `warning()`, the flags part of
//! `select_matching()`), and the pipe read end handed back as a file descriptor. The `unsafe` of the Zthread
//! unit is here. The bodies behind the entries are the stubs of `zthread.rs` until the Port task.

use crate::signatures::zthread::{Host, Outcome, Zthread, NO_FD};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

const ZTHREAD_OK: c_int = 0;
const ZTHREAD_NONE: c_int = 1;
const ZTHREAD_NOT_PORTED: c_int = -1;

/// Mirrors `struct ugrep_core_zthread_env` of src/zthread_seam.h.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ZthreadEnv {
    pub ctx: *mut c_void,
    pub set_partname: extern "C" fn(*mut c_void, *const c_char, usize),
    pub warning: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub select_by_flags: extern "C" fn(*mut c_void, *const c_char, *const u8, usize) -> c_int,
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

    fn select_by_flags(&self, path: &[u8], head: &[u8]) -> bool {
        let path = c_string(path);
        (self.0.select_by_flags)(self.0.ctx, path.as_ptr().cast(), head.as_ptr(), head.len()) != 0
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

/// Stores the outcome in `*fd` and returns the `ZTHREAD_*` status.
///
/// # Safety
/// `fd` is valid for a write.
unsafe fn report(outcome: Outcome, fd: *mut c_int) -> c_int {
    let (status, value) = match outcome {
        Outcome::Pipe(read_end) => (ZTHREAD_OK, read_end),
        Outcome::None => (ZTHREAD_NONE, NO_FD),
        Outcome::NotPorted => (ZTHREAD_NOT_PORTED, NO_FD),
    };
    // SAFETY: see above.
    unsafe { *fd = value };
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
/// `handle` comes from [`ugrep_core_zthread_new`] and is used by one thread at a time; `pathname` is
/// NUL-terminated, `find` is NULL or NUL-terminated, `file_in` is a `FILE*` that stays open until the next call
/// of this function or [`ugrep_core_zthread_close`]; `fd` is valid for a write.
#[export_name = "ugrep_core_zthread_start"]
pub unsafe extern "C" fn ugrep_core_zthread_start(
    handle: *mut c_void,
    ztstage: usize,
    pathname: *const c_char,
    file_in: *mut c_void,
    find: *const c_char,
    fd: *mut c_int,
) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let (zthread, pathname, find) = unsafe { (&mut *handle.cast::<Zthread>(), bytes(pathname).unwrap_or(b""), bytes(find)) };
        let outcome = zthread.start(ztstage, pathname, file_in, find);
        // SAFETY: see above.
        unsafe { report(outcome, fd) }
    })
}

/// `ZthreadCpp::open_next`.
///
/// # Safety
/// As [`ugrep_core_zthread_start`].
#[export_name = "ugrep_core_zthread_open_next"]
pub unsafe extern "C" fn ugrep_core_zthread_open_next(handle: *mut c_void, pathname: *const c_char, fd: *mut c_int) -> c_int {
    guarded(|| {
        // SAFETY: see above.
        let (zthread, pathname) = unsafe { (&mut *handle.cast::<Zthread>(), bytes(pathname).unwrap_or(b"")) };
        let outcome = zthread.open_next(pathname);
        // SAFETY: see above.
        unsafe { report(outcome, fd) }
    })
}

/// `ZthreadCpp::cancel`.
///
/// # Safety
/// `handle` comes from [`ugrep_core_zthread_new`] and is not closed.
#[export_name = "ugrep_core_zthread_cancel"]
pub unsafe extern "C" fn ugrep_core_zthread_cancel(handle: *mut c_void) {
    guarded(|| {
        // SAFETY: see above; `cancel` takes `&self` because the flag it sets is read by the decompression threads.
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
    extern "C" fn select_by_flags(_ctx: *mut c_void, _path: *const c_char, _buf: *const u8, _len: usize) -> c_int {
        0
    }

    #[test]
    fn stub_handle_round_trip() {
        let env = ZthreadEnv { ctx: std::ptr::null_mut(), set_partname, warning, select_by_flags };
        // SAFETY: the table is valid, the handle is used by this thread only and closed once; the strings are
        // NUL-terminated and `fd` is a local.
        unsafe {
            let handle = ugrep_core_zthread_new(&env, 0);
            let mut fd = 7;
            assert_eq!(ugrep_core_zthread_start(handle, 1, c"a.gz".as_ptr(), std::ptr::null_mut(), std::ptr::null(), &mut fd), ZTHREAD_NOT_PORTED);
            assert_eq!(fd, NO_FD);
            assert_eq!(ugrep_core_zthread_open_next(handle, c"a.gz".as_ptr(), &mut fd), ZTHREAD_NOT_PORTED);
            ugrep_core_zthread_cancel(handle);
            ugrep_core_zthread_close(handle);
            ugrep_core_zthread_close(std::ptr::null_mut());
        }
    }
}
