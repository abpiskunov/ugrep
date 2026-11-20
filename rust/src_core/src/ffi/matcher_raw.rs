//! The raw-pointer side of the Grep search seam (`search.rs`), kept in `ffi/` where `unsafe` is allowed: the reads of
//! pointers that C++ or the bridge handed over, the slice creation over matcher buffer bytes, and the C trampoline
//! that the C++ matcher calls for a buffer-shift event. The validity of every pointer is the contract of the C table
//! it came from (`src/search_seam.h`), stated at each function. Declared from `search.rs` with `#[path]`, so the
//! signatures-only harness builds it as well.

use std::cell::Cell;
use std::ffi::c_void;

use super::{dispatch_shift, HandlerSlot, MatcherHandle, MatcherTable, ShiftEvent};

/// `len` bytes at `ptr` as a slice (empty for a NULL pointer or a zero length).
///
/// # Safety
/// `ptr` must address `len` readable bytes that stay valid and unmodified for `'a`.
pub(crate) unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller guarantees `ptr` addresses `len` readable bytes valid for `'a`.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// The `T` that the opaque `user` pointer of a callback registration addresses.
///
/// # Safety
/// `user` must address a live `T` that is not mutated while the result lives.
pub(crate) unsafe fn deref<'a, T>(user: *mut c_void) -> &'a T {
    // SAFETY: the caller guarantees `user` addresses a live `T`.
    unsafe { &*(user as *const T) }
}

/// A copy of the `T` at `ptr`.
///
/// # Safety
/// `ptr` must address a fully initialized `T` that is valid for the read.
pub(crate) unsafe fn read<T: Copy>(ptr: *const T) -> T {
    // SAFETY: the caller guarantees `ptr` addresses an initialized `T`.
    unsafe { ptr.read() }
}

/// Zero-copy view of the bytes `buf_[offset..offset + len]` of the matcher buffer, for `out_str`, `check_binary`,
/// `memchr`, `utf8nlen` and the like. The slice lives only inside `f`. While `f` runs every other call on `handle`
/// panics (any of them could shift the buffer), but that guard is per handle, so soundness also needs one handle per
/// C++ matcher (SEAM.md, "Matcher callback table"): hence `unsafe`. `f` may call the host (`out_str` and the like),
/// which does not touch the matcher.
///
/// # Safety
/// `handle` must be the only handle on the C++ matcher (no other handle or C++ code may call the matcher while `f`
/// runs), and `offset..offset + len` must lie inside the matcher buffer.
pub unsafe fn with_bytes<R>(handle: &MatcherHandle, offset: usize, len: usize, f: impl FnOnce(&[u8]) -> R) -> R {
    struct Lent<'a>(&'a Cell<bool>);
    impl Drop for Lent<'_> {
        fn drop(&mut self) {
            self.0.set(false);
        }
    }
    let t = handle.t();
    let ptr = (t.bytes)(t.ctx, offset, len);
    handle.lent.set(true);
    let _lent = Lent(&handle.lent);
    // SAFETY: the callback returned `len` readable bytes; they stay valid until the next call into the matcher, and
    // `lent` plus the contract of this function rule out every such call until `f` returns.
    f(unsafe { bytes(ptr, len) })
}

/// An owned copy of the matcher buffer bytes `buf_[offset..offset + len]`; nothing borrowed survives the call.
pub(crate) fn copy_bytes(t: &MatcherTable, offset: usize, len: usize) -> Vec<u8> {
    let ptr = (t.bytes)(t.ctx, offset, len);
    // SAFETY: the callback just returned `len` readable bytes and the copy is made before anything else runs.
    unsafe { bytes(ptr, len) }.to_vec()
}

/// The C function that the C++ matcher's registered trampoline calls for a buffer-shift event; `user` is the handler
/// slot address given to `set_rust_handler`, `buf` and `len` the text about to leave the buffer, `num` its lines.
pub(crate) extern "C" fn rust_shift_trampoline(user: *mut c_void, len: usize, num: usize, buf: *const u8) {
    // SAFETY: `user` is the address of the boxed `HandlerSlot` of a live `MatcherHandle`, which unregisters the
    // trampoline from the matcher before the slot is dropped; the matcher passes `len` readable bytes valid for this
    // call.
    let (slot, text): (&HandlerSlot, &[u8]) = unsafe { (deref(user), bytes(buf, len)) };
    dispatch_shift(slot, ShiftEvent { len, num }, text);
}

/// The address of the handler slot of `handle`, as registered with `set_rust_handler`.
pub(crate) fn slot_user(handle: &MatcherHandle) -> *mut c_void {
    &*handle.handler as *const HandlerSlot as *mut c_void
}

/// Test-only safe wrappers for the fake matcher of the tests of `search.rs`, which has no `unsafe` of its own.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    pub(crate) fn deref_ctx<'a, T>(ctx: *mut c_void) -> &'a T {
        // SAFETY: the tests pass the address of a live fake that outlives the handle.
        unsafe { deref(ctx) }
    }

    pub(crate) fn read_value<T: Copy>(ptr: *const T) -> T {
        // SAFETY: the tests pass the address of an initialized value.
        unsafe { read(ptr) }
    }

    pub(crate) fn write_value<T>(ptr: *mut T, value: T) {
        // SAFETY: the tests pass the address of a `T` that is valid for the write.
        unsafe { ptr.write(value) }
    }

    pub(crate) fn with_bytes<R>(handle: &MatcherHandle, offset: usize, len: usize, f: impl FnOnce(&[u8]) -> R) -> R {
        // SAFETY: the tests have a single handle on the fake matcher; the one test whose `f` calls the handle checks
        // that the guard fires.
        unsafe { super::with_bytes(handle, offset, len, f) }
    }
}
