//! The raw-pointer side of the Grep search seam (`search.rs`), kept in `ffi/` where `unsafe` is allowed: the reads of
//! pointers that C++ or the bridge handed over, the slice creation over matcher buffer bytes, and the C trampoline
//! that the C++ matcher calls for a buffer-shift event. The validity of every pointer is the contract of the C table
//! it came from (`src/search_seam.h`), stated at each function. Declared from `search.rs` with `#[path]`, so the
//! signatures-only harness builds it as well.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

use super::{dispatch_shift, HandlerSlot, MatcherHandle, MatcherTable, ShiftEvent};

/// The Rust shift handler the C++ trampoline handler calls for every buffer-shift event (`ugrep_core_shift_fn`).
pub type ShiftFn = unsafe extern "C" fn(user: *mut c_void, len: usize, num: usize, buf: *const u8);

/// A handle that calls the C++ matcher through `table`.
///
/// # Safety
/// Every entry of `table` must honour `src/search_seam.h` for as long as the handle lives: `ctx` stays valid, and
/// `bytes` returns either NULL or `len` bytes inside the matcher buffer. The safe methods of the handle rely on it.
pub(crate) unsafe fn new_handle(table: MatcherTable) -> MatcherHandle {
    MatcherHandle { table, handler: Box::new(RefCell::new(None)), lent: Cell::new(false) }
}

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
    assert!(!ptr.is_null(), "Grep seam: bytes {offset}+{len} lie outside the matcher buffer");
    handle.lent.set(true);
    let _lent = Lent(&handle.lent);
    // SAFETY: the callback returned `len` readable bytes; they stay valid until the next call into the matcher, and
    // `lent` plus the contract of this function rule out every such call until `f` returns.
    f(unsafe { bytes(ptr, len) })
}

/// An owned copy of the matcher buffer bytes `buf_[offset..offset + len]`; nothing borrowed survives the call.
pub(crate) fn copy_bytes(t: &MatcherTable, offset: usize, len: usize) -> Vec<u8> {
    let ptr = (t.bytes)(t.ctx, offset, len);
    assert!(!ptr.is_null(), "Grep seam: bytes {offset}+{len} lie outside the matcher buffer");
    // SAFETY: a non-NULL result of `bytes` is `len` bytes inside the matcher buffer (`new_handle` contract),
    // and the copy is made before anything else runs.
    unsafe { bytes(ptr, len) }.to_vec()
}

/// An owned copy of `len` bytes at `ptr` (the matched text of `begin_ptr`); nothing borrowed survives the call.
///
/// # Safety
/// `ptr` must address `len` readable bytes (the contract of a non-NULL `begin_ptr` result).
pub(crate) unsafe fn copy_begin(ptr: *const u8, len: usize) -> Vec<u8> {
    // SAFETY: the caller guarantees `ptr` addresses `len` readable bytes; the copy is made before anything else runs.
    unsafe { bytes(ptr, len) }.to_vec()
}

/// `len` bytes from the begin of the match of `handle`, copied out; `None` when `begin_ptr` returns NULL.
pub(crate) fn begin_bytes(handle: &MatcherHandle, len: usize) -> Option<Vec<u8>> {
    let t = handle.t();
    let ptr = (t.begin_ptr)(t.ctx, len);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the table contract (`new_handle`): a non-NULL `begin_ptr` result addresses `len` readable bytes inside
        // the matcher buffer data, and nothing runs before the copy.
        Some(unsafe { copy_begin(ptr, len) })
    }
}

/// A buffer over static data, for the tests.
#[cfg(test)]
pub(crate) fn static_buffer(data: &'static [u8]) -> BufferPtr {
    // SAFETY: `data` is readable for `'static` and never modified.
    unsafe { buffer_ptr(data.as_ptr(), data.len()) }
}

/// A `FILE*` that C++ still owns (`Grep::file_in`, `Static::source`, `stdin`, the file of `Grep::input`), borrowed for as
/// long as the host borrow lasts. It is never NULL, not `Copy`, and cannot be closed, consumed or stored: only the calls that
/// use a file take it. The field is private to this module, so the port modules (`search::file`, `search::branch`)
/// cannot forge one.
#[derive(Debug, PartialEq, Eq)]
pub struct FileRef<'a> {
    ptr: NonNull<c_void>,
    _host: PhantomData<&'a ()>,
}

/// A `FILE*` that the holder must close: made only by `file_open`, `filter_spawn`, `zthread_start`/`open_next` and
/// `take_file_in`. Never NULL (a failed call returns `None`), not `Copy` or `Clone`; `file_close`, `filter_spawn`,
/// `zthread_start` and `set_file_in` take it by value, so a double close does not compile. Dropping it leaks the stream
/// (no unsafe in `Drop`), which is safe.
#[derive(Debug, PartialEq, Eq)]
pub struct OwnedFile(NonNull<c_void>);

impl OwnedFile {
    /// The borrowed view for the calls that only use the file.
    pub fn as_file(&self) -> FileRef<'_> {
        FileRef { ptr: self.0, _host: PhantomData }
    }
}
/// `reflex::Input*` of C++ (`&Grep::input`), opaque to Rust and made only here ([`input_handle`]).
#[derive(Debug, PartialEq, Eq)]
pub struct InputHandle(*const c_void);

/// Memory that a matcher reads from after [`MatcherHandle::buffer`], with the size to pass (the C++ passes `size + 1`
/// for an mmap, which counts the terminating NUL). Made only here ([`buffer_ptr`], unsafe); not `Copy`.
#[derive(Debug, PartialEq, Eq)]
pub struct BufferPtr {
    ptr: *const u8,
    size: usize,
}

/// The Rust state that lives as long as the C++ `Grep` (see `GrepState` in `search.rs`). `mapping` is private: it is set
/// only by [`set_mapping`] (unsafe), so safe code cannot make a later unmapping touch memory the state does not own.
#[derive(Debug, Default)]
pub struct GrepState {
    mapping: RefCell<Option<(usize, usize)>>,
}

impl GrepState {
    pub fn new() -> GrepState {
        GrepState::default()
    }

    /// The mapping reused across files: `(base address, mapped size)`, `None` when nothing is mapped.
    pub fn mapping(&self) -> Option<(usize, usize)> {
        *self.mapping.borrow()
    }
}

/// Record the mapping that the state unmaps (or reuses with `MAP_FIXED`) when the `Grep` goes.
///
/// # Safety
/// `mapping` must be `None` or `(base, size)` of a live mapping that the caller made and that nothing else unmaps.
pub unsafe fn set_mapping(state: &GrepState, mapping: Option<(usize, usize)>) {
    *state.mapping.borrow_mut() = mapping;
}

/// A borrowed handle of a `FILE*` that C++ keeps owning; `None` for NULL.
///
/// # Safety
/// `ptr` must be NULL or a `FILE*` that stays valid and owned by C++ for `'a`.
pub(crate) unsafe fn file_ref<'a>(ptr: *mut c_void) -> Option<FileRef<'a>> {
    NonNull::new(ptr).map(|ptr| FileRef { ptr, _host: PhantomData })
}

/// An owned handle of a `FILE*` that the caller now has to close; `None` for NULL.
///
/// # Safety
/// `ptr` must be NULL or an open `FILE*` that nothing else closes.
pub(crate) unsafe fn owned_file(ptr: *mut c_void) -> Option<OwnedFile> {
    NonNull::new(ptr).map(OwnedFile)
}

/// The `FILE*` of a borrowed file, for the callbacks of the facade.
pub(crate) fn file_ptr(file: &FileRef<'_>) -> *mut c_void {
    file.ptr.as_ptr()
}

/// The `FILE*` of a file that is given up (closed, consumed or handed to C++ by the callback).
pub(crate) fn file_into_ptr(file: OwnedFile) -> *mut c_void {
    file.0.as_ptr()
}
/// The handle of the host-owned `reflex::Input` (`&Grep::input`).
///
/// # Safety
/// `ptr` must be the address of the `reflex::Input` of the `Grep`, valid while the handle is used.
pub(crate) unsafe fn input_handle(ptr: *const c_void) -> InputHandle {
    InputHandle(ptr)
}

/// The address of the `reflex::Input` of a handle.
pub(crate) fn input_ptr(handle: &InputHandle) -> *const c_void {
    handle.0
}

/// The base and size of a buffer, for `MatcherHandle::buffer`.
pub(crate) fn buffer_parts(memory: &BufferPtr) -> (*const u8, usize) {
    (memory.ptr, memory.size)
}

/// Memory for [`MatcherHandle::buffer`]; the mmap port makes these in `ffi/`. Only the tests make one until then.
///
/// # Safety
/// `ptr` must address `size` readable bytes (the C++ counts the terminating NUL in an mmap) that stay valid and
/// unmodified until the matcher reads from something else or is destroyed.
#[cfg(test)]
pub(crate) unsafe fn buffer_ptr(ptr: *const u8, size: usize) -> BufferPtr {
    BufferPtr { ptr, size }
}

/// The C function that the C++ matcher's registered trampoline calls for a buffer-shift event; `user` is the handler
/// slot address given to `set_rust_handler`, `buf` and `len` the text about to leave the buffer, `num` its lines.
///
/// # Safety
/// `user` must be the slot address from [`slot_user`] of a live handle, and `buf` must point to `len` readable bytes.
pub(crate) unsafe extern "C" fn rust_shift_trampoline(user: *mut c_void, len: usize, num: usize, buf: *const u8) {
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

    pub(crate) fn handle(table: MatcherTable) -> MatcherHandle {
        // SAFETY: the fake tables of the tests honour the header for as long as their fake matcher lives.
        unsafe { new_handle(table) }
    }

    pub(crate) fn call_shift(shift: ShiftFn, user: *mut c_void, len: usize, num: usize, buf: *const u8) {
        // SAFETY: the fake passes the slot address it was registered with and its own buffer.
        unsafe { shift(user, len, num, buf) }
    }

    pub(crate) fn with_bytes<R>(handle: &MatcherHandle, offset: usize, len: usize, f: impl FnOnce(&[u8]) -> R) -> R {
        // SAFETY: the tests have a single handle on the fake matcher; the one test whose `f` calls the handle checks
        // that the guard fires.
        unsafe { super::with_bytes(handle, offset, len, f) }
    }
}
