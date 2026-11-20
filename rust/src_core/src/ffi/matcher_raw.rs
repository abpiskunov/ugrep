//! The raw-pointer side of the Grep search seam (`search.rs`), kept in `ffi/` where `unsafe` is allowed: the reads of
//! pointers that C++ or the bridge handed over, the slice creation over matcher buffer bytes, and the C trampoline
//! that the C++ matcher calls for a buffer-shift event. The validity of every pointer is the contract of the C table
//! it came from (`src/search_seam.h`), stated at each function. Declared from `search.rs` with `#[path]`, so the
//! signatures-only harness builds it as well.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

use super::{dispatch_shift, FileSource, GrepHost, HandlerSlot, MatcherHandle, MatcherTable, ShiftEvent};

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
pub(crate) fn copy_bytes(handle: &MatcherHandle, offset: usize, len: usize) -> Vec<u8> {
    let t = handle.t();
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

/// A `FILE*` that the holder must close: made only by `file_open` and `filter_spawn`.
/// Never NULL (a failed call returns `None`), not `Copy` or `Clone`; `file_close`, `filter_spawn` and `set_file_in` take it
/// by value, so a double close does not compile. A stream that the host owns (`Grep::file_in`, the file of
/// `Grep::input`) is closed by the host primitives `close_input_file` and `close_file_in`, never handed to Rust.
/// Dropping it leaks the stream (no unsafe in `Drop`), which is safe.
/// (no unsafe in `Drop`), which is safe.
#[derive(Debug, PartialEq, Eq)]
pub struct OwnedFile(NonNull<c_void>);

impl OwnedFile {
    /// The borrowed view for the calls that only use the file.
    pub fn as_file(&self) -> FileRef<'_> {
        FileRef { ptr: self.0, _host: PhantomData }
    }
}

/// Run the `--filter` command on `Grep::file_in` (`Grep::filter_spawn(file_in, pathname, command)`): the stream goes to
/// `filter_spawn`, which closes it unless it is stdin, and `Grep::file_in` becomes the reading end of the pipe, or NULL
/// when the pipe could not be created (false). The host's `file_in` is replaced before the call returns, and nothing else
/// can run in between (`&mut` host), so the stream the host forgets here is never used again.
pub fn filter_file_in(host: &mut dyn GrepHost, pathname: &[u8], command: &[u8]) -> bool {
    let source = {
        let Some(file) = host.file_in() else { return true };
        if file == host.stdin_file() {
            FileSource::Stdin
        } else if host.source().as_ref() == Some(&file) {
            FileSource::Source
        } else {
            // the host keeps the pointer until `set_file_in` below overwrites it without closing
            FileSource::Owned(OwnedFile(file.ptr))
        }
    };
    match host.filter_spawn(source, pathname, command) {
        Some(pipe) => {
            host.set_file_in(Some(FileSource::Owned(pipe)));
            true
        }
        None => {
            host.set_file_in(None);
            false
        }
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

impl Drop for GrepState {
    /// `MMap::~MMap`: unmap the region reserved for the files that were mapped.
    fn drop(&mut self) {
        if let Some((base, size)) = self.mapping.get_mut().take() {
            sys::unmap(base, size);
        }
    }
}

/// Record the mapping that the state unmaps (or reuses with `MAP_FIXED`) when the `Grep` goes.
///
/// # Safety
/// `mapping` must be `None` or `(base, size)` of a live mapping that the caller made and that nothing else unmaps.
pub unsafe fn set_mapping(state: &GrepState, mapping: Option<(usize, usize)>) {
    *state.mapping.borrow_mut() = mapping;
}

/// `MMap::file`: map the regular file `file` (plain encoding already checked by the caller) read-only over the region the
/// state reserved, as `src/mmap.hpp` does. `None` when mmap is not available (Windows, as `HAVE_MMAP` is not defined by the
/// MSVC build), when `file` is not a regular file of at least `MIN_MMAP_SIZE` (16384) bytes, is larger than `max_mmap`
/// (`--max-mmap`, 0 disables) or a system call fails. On success the buffer is the file followed by the terminating NUL
/// (`size + 1`), valid until the next call on this state or until the state is dropped.
pub fn map_input(state: &GrepState, file: &FileRef<'_>, max_mmap: usize) -> Option<BufferPtr> {
    let mut lost = false;
    let mapped = sys::map_file(state.mapping(), file.ptr.as_ptr(), max_mmap, &mut lost);
    if lost {
        // MAP_FIXED failed over the reserved region: the kernel may have unmapped it already, so the state forgets it (a
        // leak if it is still mapped) and never maps over or unmaps it again, as `MMap::file` does
        // SAFETY: `None` is always a valid mapping record.
        unsafe { set_mapping(state, None) };
    }
    let (base, size) = mapped?;
    // SAFETY: `map_file` returns the live mapping it made at `base.0` of `base.1` bytes, which only this state unmaps
    // (on the next mapping, which replaces it with MAP_FIXED, or when it is dropped).
    unsafe { set_mapping(state, Some(base)) };
    Some(BufferPtr { ptr: base.0 as *const u8, size: size + 1 })
}

/// The mmap system calls, `src/mmap.hpp` under `HAVE_MMAP`: 64-bit Unix systems whose constants are listed here.
#[cfg(all(
    unix,
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly")
))]
mod sys {
    use std::ffi::{c_int, c_void};
    use std::mem::ManuallyDrop;
    use std::os::unix::io::FromRawFd;

    const PROT_READ: c_int = 1;
    const MAP_PRIVATE: c_int = 2;
    const MAP_FIXED: c_int = 0x10;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const MAP_ANONYMOUS: c_int = 0x20;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const MAP_ANONYMOUS: c_int = 0x1000;
    const MADV_SEQUENTIAL: c_int = 2;
    /// `MIN_MMAP_SIZE`: smaller files are efficiently read in one go.
    const MIN_MMAP_SIZE: u64 = 16384;

    extern "C" {
        fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, offset: i64) -> *mut c_void;
        fn munmap(addr: *mut c_void, len: usize) -> c_int;
        fn madvise(addr: *mut c_void, len: usize, advice: c_int) -> c_int;
        fn fileno(stream: *mut c_void) -> c_int;
    }

    fn failed(p: *mut c_void) -> bool {
        p as usize == usize::MAX
    }

    pub(super) fn unmap(base: usize, size: usize) {
        // SAFETY: `base`/`size` come from a mapping that `map_file` made and that nobody else unmaps.
        unsafe { munmap(base as *mut c_void, size) };
    }

    /// Returns the mapping `((base, mapped size), file size)`; sets `lost` when the MAP_FIXED over an existing region
    /// failed, so the caller forgets that region.
    pub(super) fn map_file(
        current: Option<(usize, usize)>,
        file: *mut c_void,
        max_mmap: usize,
        lost: &mut bool,
    ) -> Option<((usize, usize), usize)> {
        // SAFETY: `file` is a live `FILE*` (a `FileRef`).
        let fd = unsafe { fileno(file) };
        if fd < 0 {
            return None;
        }
        // SAFETY: `fd` is open for the duration of this call; `ManuallyDrop` keeps the `File` from closing it.
        let meta = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) }).metadata().ok()?;
        if !meta.is_file() || meta.len() < MIN_MMAP_SIZE {
            return None;
        }
        let size = usize::try_from(meta.len()).ok()?;
        // is this file not larger than --max-mmap?
        if size > max_mmap {
            return None;
        }
        // allocate the fixed mmap region to reuse, rounded up to 4K
        let (base, mapped) = match current {
            Some(region) => region,
            None => {
                let mapped = max_mmap.checked_add(0xfff)? & !0xfffusize;
                // SAFETY: an anonymous private mapping of `mapped` bytes at an address chosen by the system.
                let p = unsafe { mmap(std::ptr::null_mut(), mapped, PROT_READ, MAP_ANONYMOUS | MAP_PRIVATE, -1, 0) };
                if failed(p) {
                    return None;
                }
                // files are sequentially read
                // SAFETY: `p` addresses the live mapping of `mapped` bytes just made.
                unsafe { madvise(p, mapped, MADV_SEQUENTIAL) };
                (p as usize, mapped)
            }
        };
        // the buffer is the file plus its terminating NUL, which must lie inside the region (a later max_mmap does not
        // grow a region made for a smaller one)
        if size >= mapped {
            if current.is_none() {
                unmap(base, mapped);
            }
            return None;
        }
        // mmap the (next) file over the fixed region
        // SAFETY: MAP_FIXED over the region this state reserved (or just made) and owns.
        let p = unsafe { mmap(base as *mut c_void, mapped, PROT_READ, MAP_FIXED | MAP_PRIVATE, fd, 0) };
        if failed(p) {
            if current.is_none() {
                unmap(base, mapped);
            } else {
                *lost = true;
            }
            return None;
        }
        Some(((p as usize, mapped), size))
    }
}

/// No `HAVE_MMAP` (Windows, other platforms): mmap is never used, as in the C++ build.
#[cfg(not(all(
    unix,
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly")
)))]
mod sys {
    use std::ffi::c_void;

    pub(super) fn unmap(_base: usize, _size: usize) {}

    pub(super) fn map_file(
        _current: Option<(usize, usize)>,
        _file: *mut c_void,
        _max_mmap: usize,
        _lost: &mut bool,
    ) -> Option<((usize, usize), usize)> {
        None
    }
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

    /// A borrowed file at a made-up address that is never dereferenced.
    pub(crate) fn file_ref<'a>(addr: usize) -> FileRef<'a> {
        // SAFETY: the tests never dereference the pointer; the fake host only compares it.
        unsafe { super::file_ref(addr as *mut c_void) }.expect("non-NULL fake file")
    }

    /// An owned file at a made-up address that is never dereferenced or closed.
    pub(crate) fn owned_file(addr: usize) -> OwnedFile {
        // SAFETY: as above; dropping an `OwnedFile` closes nothing.
        unsafe { super::owned_file(addr as *mut c_void) }.expect("non-NULL fake file")
    }

    pub(crate) fn input_handle() -> InputHandle {
        // SAFETY: the fake host only passes the handle to the fake matcher, which ignores it.
        unsafe { super::input_handle(std::ptr::null()) }
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
