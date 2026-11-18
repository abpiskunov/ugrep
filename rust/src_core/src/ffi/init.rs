//! C-ABI entry of `init(argc, argv)` for src/ugrep.cpp (built with CPP_RUST_IMPL_SRC_CORE_INIT_RUST).

use crate::signatures as sig;
use std::os::raw::{c_char, c_int};

/// # Safety
/// `argv` points to `argc` entries that are valid NUL-terminated strings living for the whole process.
#[export_name = "ugrep_core_init"]
pub unsafe extern "C" fn ugrep_core_init(argc: c_int, argv: *mut *const c_char) {
    let argc = usize::try_from(argc).unwrap_or(0);
    if argv.is_null() {
        return;
    }
    // SAFETY: the caller passes the process `argv` with `argc` entries, which stay valid and unaliased for the call.
    let args = unsafe { std::slice::from_raw_parts_mut(argv, argc) };
    sig::ui::init(args)
}
