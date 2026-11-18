//! The only `unsafe` of the settings: the exported `ugrep_settings` object that the C++ side reads and writes.

use crate::settings::Settings;
use std::cell::UnsafeCell;

/// The C++ side mutates the settings; Rust only reads them through `as_ptr`.
pub struct SettingsCell(UnsafeCell<Settings>);

// SAFETY: option parsing runs before the worker threads start and the fields are plain scalars and pointers
// to C strings that live for the whole process; this is the contract the C++ globals had.
unsafe impl Sync for SettingsCell {}

impl SettingsCell {
    pub fn as_ptr(&self) -> *mut Settings {
        self.0.get()
    }
}

/// Run `f` on the settings. The `&mut` lives only for `f`, which must not call back into C++.
pub fn cfg<R>(f: impl FnOnce(&mut Settings) -> R) -> R {
    // SAFETY: option parsing is single-threaded and C++ code does not run while `f` does.
    f(unsafe { &mut *UGREP_SETTINGS.as_ptr() })
}

/// Whether `flag_binary_files` is set to `s`; false for NULL.
pub fn binary_files_is(s: &[u8]) -> bool {
    let p = cfg(|c| c.flag_binary_files);
    // SAFETY: settings hold NULL or NUL-terminated strings that live for the process.
    !p.is_null() && unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes() == s
}

#[export_name = "ugrep_settings"]
pub static UGREP_SETTINGS: SettingsCell = SettingsCell(UnsafeCell::new(Settings::DEFAULT));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_object_has_the_cpp_defaults() {
        let s = UGREP_SETTINGS.as_ptr();
        // SAFETY: single-threaded test, no C++ writer.
        let s = unsafe { &*s };
        assert!(s.flag_hex_cbr && s.flag_hex_chr && s.flag_hex_hbr);
        assert!(!s.flag_count);
        assert_eq!(s.flag_hex_columns, 16);
        assert_eq!(s.flag_zmax, 1);
        assert_eq!(s.flag_min_magic, 1);
        assert_eq!(s.flag_binary, -1);
        assert!(s.flag_color_query.is_null());
        assert!(!s.flag_group_separator.is_null());
    }
}
