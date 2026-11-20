//! The raw-pointer reads of the Grep search seam (`search.rs`), kept in `ffi/` where `unsafe` is allowed. Each function
//! is the one place where a pointer that C++ or the bridge handed over becomes a Rust reference or value; the callers
//! pass pointers whose validity is the contract of the C table they came from (`src/search_seam.h`), stated at each
//! call. Declared from `search.rs` with `#[path]`, so the signatures-only harness builds it as well.

use std::ffi::c_void;

/// `len` bytes at `ptr` as a slice (empty for a NULL pointer or a zero length).
///
/// The caller passes the result of a matcher or host callback that returned `len` readable bytes valid for `'a`.
pub(crate) fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: per the contract above `ptr` addresses `len` readable bytes that outlive `'a`.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// The `T` that the opaque `user` pointer of a callback registration addresses.
///
/// The caller passes a pointer it registered itself, to a live `T`, and does not mutate the `T` while the result lives.
pub(crate) fn deref<'a, T>(user: *mut c_void) -> &'a T {
    // SAFETY: per the contract above `user` addresses a live `T`.
    unsafe { &*(user as *const T) }
}

/// A copy of the `T` at `ptr`.
///
/// The caller passes a pointer to a fully initialized `T` that is valid for the read.
pub(crate) fn read<T: Copy>(ptr: *const T) -> T {
    // SAFETY: per the contract above `ptr` addresses an initialized `T`.
    unsafe { ptr.read() }
}

/// Store `value` through the out-parameter `ptr` of a callback.
///
/// The caller passes a pointer to a `T` that is valid for the write.
#[cfg(test)]
pub(crate) fn write<T>(ptr: *mut T, value: T) {
    // SAFETY: per the contract above `ptr` is valid for the write.
    unsafe { ptr.write(value) }
}
