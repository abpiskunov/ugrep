//! C ABI of `OutputSync` for the C++ `Output::Sync` wrapper (declared in src/output.hpp).
//! The handle is a reference-counted pointer: `ugrep_output_sync_new` returns the first reference, every owner lock
//! takes its own with `ugrep_output_sync_retain`, and `ugrep_output_sync_free` drops one. The object lives until the
//! last reference is gone, so a lock may outlive the C++ `Sync` that made it.

use crate::output_sync::OutputSync;
use std::sync::Arc;

/// Creates a Sync; `ordered` selects ORDERED mode, `stop` is `Output::STOP`.
#[no_mangle]
pub extern "C" fn ugrep_output_sync_new(ordered: bool, stop: usize) -> *mut OutputSync {
    Arc::into_raw(Arc::new(OutputSync::new(ordered, stop))).cast_mut()
}

/// # Safety
/// `sync` is a reference from `ugrep_output_sync_new` or `ugrep_output_sync_retain` that is not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_free(sync: *mut OutputSync) {
    if !sync.is_null() {
        // SAFETY: the pointer came from `Arc::into_raw` and this reference returns here, per the contract above.
        drop(unsafe { Arc::from_raw(sync) });
    }
}

/// Takes another reference to the Sync.
///
/// # Safety
/// `sync` is a live handle: the caller holds a reference to it.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_retain(sync: *const OutputSync) -> *mut OutputSync {
    // SAFETY: the caller holds a reference, so the strong count is at least one.
    unsafe { Arc::increment_strong_count(sync) };
    sync.cast_mut()
}

/// Borrows the handle.
///
/// # Safety
/// `sync` is a live handle: the caller holds a reference to it for the duration of the call.
unsafe fn sync<'a>(sync: *const OutputSync) -> &'a OutputSync {
    // SAFETY: guaranteed by the caller.
    unsafe { &*sync }
}

/// # Safety
/// `s` is a live handle: the caller holds a reference to it for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_acquire(s: *const OutputSync, owned: bool, slot: usize) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.acquire(owned, slot)
}

/// # Safety
/// `s` is a live handle: the caller holds a reference to it for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_try_acquire(s: *const OutputSync, owned: bool) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.try_acquire(owned)
}

/// # Safety
/// `s` is a live handle: the caller holds a reference to it for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_release(s: *const OutputSync, owned: bool) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.release(owned)
}

/// # Safety
/// `s` is a live handle: the caller holds a reference to it for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_finish(s: *const OutputSync, owned: bool, slot: usize) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.finish(owned, slot)
}

/// # Safety
/// `s` is a live handle: the caller holds a reference to it for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_cancel(s: *const OutputSync) {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.cancel();
}

/// # Safety
/// `s` is a live handle: the caller holds a reference to it for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_cancelled(s: *const OutputSync) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.cancelled()
}

/// # Safety
/// `s` is a live handle: the caller holds a reference to it for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_drop_owner(s: *const OutputSync, owned: bool) {
    if owned {
        // SAFETY: the handle is live, per the contract above.
        unsafe { sync(s) }.drop_owner(true);
    }
}
