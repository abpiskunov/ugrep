//! C ABI of `OutputSync` for the C++ `Output::Sync` wrapper (declared in src/output.hpp).
//! The handle is created by `ugrep_output_sync_new` and freed by `ugrep_output_sync_free`; the C++ side owns it.

use crate::output_sync::OutputSync;

/// Creates a Sync; `ordered` selects ORDERED mode, `stop` is `Output::STOP`.
#[no_mangle]
pub extern "C" fn ugrep_output_sync_new(ordered: bool, stop: usize) -> *mut OutputSync {
    Box::into_raw(Box::new(OutputSync::new(ordered, stop)))
}

/// # Safety
/// `sync` comes from `ugrep_output_sync_new`, is not used afterwards and no thread is inside a call on it.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_free(sync: *mut OutputSync) {
    if !sync.is_null() {
        // SAFETY: the pointer came from `Box::into_raw` and ownership returns here, per the contract above.
        drop(unsafe { Box::from_raw(sync) });
    }
}

/// Borrows the handle.
///
/// # Safety
/// `sync` comes from `ugrep_output_sync_new` and is not freed during the call.
unsafe fn sync<'a>(sync: *const OutputSync) -> &'a OutputSync {
    // SAFETY: guaranteed by the caller.
    unsafe { &*sync }
}

/// # Safety
/// `s` is a live handle from `ugrep_output_sync_new`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_acquire(s: *const OutputSync, owned: bool, slot: usize) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.acquire(owned, slot)
}

/// # Safety
/// `s` is a live handle from `ugrep_output_sync_new`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_try_acquire(s: *const OutputSync, owned: bool) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.try_acquire(owned)
}

/// # Safety
/// `s` is a live handle from `ugrep_output_sync_new`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_release(s: *const OutputSync, owned: bool) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.release(owned)
}

/// # Safety
/// `s` is a live handle from `ugrep_output_sync_new`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_finish(s: *const OutputSync, owned: bool, slot: usize) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.finish(owned, slot)
}

/// # Safety
/// `s` is a live handle from `ugrep_output_sync_new`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_cancel(s: *const OutputSync) {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.cancel();
}

/// # Safety
/// `s` is a live handle from `ugrep_output_sync_new`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_cancelled(s: *const OutputSync) -> bool {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.cancelled()
}

/// # Safety
/// `s` is a live handle from `ugrep_output_sync_new`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_output_sync_drop_owner(s: *const OutputSync, owned: bool) {
    // SAFETY: the handle is live, per the contract above.
    unsafe { sync(s) }.drop_owner(owned);
}
