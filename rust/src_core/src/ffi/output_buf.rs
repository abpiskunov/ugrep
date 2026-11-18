//! C ABI of `OutputBuf` and `Tree` for the C++ `Output` wrapper (declared in src/output.hpp).
//! `ugrep_output_new` returns the state that the C++ `Output` owns until `ugrep_output_free`. Every other function takes
//! that handle, which only the owner thread uses. The inline appenders of C++ do not call in: they write into the block
//! through the shared `Regs`, and call `ugrep_output_next` only when the block is full.

use crate::output_buf::{self, OutputBuf, Regs};
use crate::output_sync::OutputSync;
use std::ffi::{c_char, c_void};
use std::sync::Arc;

/// # Safety
/// `regs` and `eof` are members of the C++ `Output` (`std::atomic_bool` for `eof`), which stay at their address until
/// `ugrep_output_free`; `file` is the open `FILE*` of the output.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_new(regs: *mut Regs, file: *mut c_void, eof: *mut bool) -> *mut OutputBuf {
    // SAFETY: guaranteed by the caller.
    Box::into_raw(Box::new(unsafe { OutputBuf::new(regs, file, eof) }))
}

/// Flushes the output and frees the state.
///
/// # Safety
/// `out` is from `ugrep_output_new` and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_free(out: *mut OutputBuf) {
    if !out.is_null() {
        // SAFETY: the pointer came from `Box::into_raw`, per the contract above.
        drop(unsafe { Box::from_raw(out) });
    }
}

/// Borrows the state.
///
/// # Safety
/// `out` is a live handle from `ugrep_output_new`, used by one thread at a time.
unsafe fn state<'a>(out: *mut OutputBuf) -> &'a mut OutputBuf {
    // SAFETY: guaranteed by the caller.
    unsafe { &mut *out }
}

/// Synchronize the output on the given Sync, taking a reference to it that the output keeps until it is freed.
///
/// # Safety
/// `out` is a live handle; `sync` is a live Sync handle (`ugrep_output_sync_new`) that the caller holds a reference to.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_on(out: *mut OutputBuf, sync: *const OutputSync) {
    // SAFETY: the caller holds a reference, so the strong count is at least one before it is incremented; the new
    // reference is the one that `Arc::from_raw` takes over.
    let sync = unsafe {
        Arc::increment_strong_count(sync);
        Arc::from_raw(sync)
    };
    // SAFETY: the handle is live, per the contract above.
    unsafe { state(out) }.sync_on(sync);
}

macro_rules! entry {
    ($(#[$m:meta])* $name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)? => $method:ident) => {
        $(#[$m])*
        /// # Safety
        /// `out` is a live handle from `ugrep_output_new`, used by one thread at a time.
        #[no_mangle]
        pub unsafe extern "C" fn $name(out: *mut OutputBuf $(, $arg: $ty)*) $(-> $ret)? {
            // SAFETY: the handle is live, per the contract above.
            unsafe { state(out) }.$method($($arg),*)
        }
    };
}

entry!(ugrep_output_begin(slot: usize) => begin);
entry!(ugrep_output_acquire() => acquire);
entry!(ugrep_output_release() => release);
entry!(ugrep_output_end() => end);
entry!(ugrep_output_cancel() => cancel);
entry!(ugrep_output_cancelled() -> bool => cancelled);
entry!(ugrep_output_discard() => discard);
entry!(ugrep_output_flush() => flush);
entry!(ugrep_output_next() => next);
entry!(ugrep_output_try_flush() => try_flush);
entry!(ugrep_output_dump_next(byte_offset: usize) => dump_next);
entry!(ugrep_output_dump_incomplete() -> bool => dump_incomplete);
entry!(ugrep_output_dump_complete(off: usize) => dump_complete);
entry!(ugrep_output_dump_done() => dump_done);

/// `Dump::hex`: dump `size` bytes of `data` in hex.
///
/// # Safety
/// `out` is a live handle; `data` points to `size` readable bytes (it may be NULL if `size` is 0).
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_dump_hex(out: *mut OutputBuf, mode: i16, byte_offset: usize, data: *const u8, size: usize) {
    let bytes = if size == 0 {
        &[]
    } else {
        // SAFETY: guaranteed by the caller.
        unsafe { std::slice::from_raw_parts(data, size) }
    };
    // SAFETY: the handle is live, per the contract above.
    unsafe { state(out) }.dump_hex(mode, byte_offset, bytes);
}

/// `Tree::path.clear(); Tree::depth = 0;`
#[no_mangle]
pub extern "C" fn ugrep_output_tree_reset() {
    let mut t = output_buf::tree();
    t.state.path.clear();
    t.state.depth = 0;
}

/// `Tree::depth`
#[no_mangle]
pub extern "C" fn ugrep_output_tree_depth() -> i32 {
    output_buf::tree().state.depth
}

/// The `Tree::end` string, NUL-terminated; valid until the glyphs are set again (the terminal setup sets them once,
/// before any output).
#[no_mangle]
pub extern "C" fn ugrep_output_tree_end() -> *const c_char {
    output_buf::tree().end_c_str().cast()
}
/// Set `Tree::bar`, `ptr` and `end`.
///
/// # Safety
/// The three pointers are NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_tree_glyphs(bar: *const c_char, ptr: *const c_char, end: *const c_char) {
    // SAFETY: guaranteed by the caller.
    let (bar, ptr, end) = unsafe {
        (
            std::ffi::CStr::from_ptr(bar).to_bytes(),
            std::ffi::CStr::from_ptr(ptr).to_bytes(),
            std::ffi::CStr::from_ptr(end).to_bytes(),
        )
    };
    output_buf::tree().set_glyphs(bar, ptr, end);
}
