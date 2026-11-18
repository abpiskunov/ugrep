//! The C entry of ugrep-indexer: `main` in src/ugrep-indexer.cpp calls `ugrep_indexer_main` with its argv.

use std::ffi::{c_char, c_int, CStr};

/// Runs ugrep-indexer and returns the exit code for `main` to return.
///
/// # Safety
/// `argv` must point to `argc` valid NUL-terminated strings, as the arguments of `main` do.
#[no_mangle]
pub unsafe extern "C" fn ugrep_indexer_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let mut args = Vec::new();
    for i in 0..usize::try_from(argc).unwrap_or(0) {
        // SAFETY: the caller passes `argc` valid C strings
        let arg = unsafe { CStr::from_ptr(*argv.add(i)) };
        args.push(arg.to_bytes().to_vec());
    }
    crate::signatures::indexer_cli::run(&args)
}
