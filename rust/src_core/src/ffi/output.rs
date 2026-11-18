//! The only `unsafe` of the output seam besides the Sync C ABI: reading the exported settings object into `Opts`.

use crate::output::Opts;
use crate::settings::Settings;
use std::ffi::{c_char, CStr};

/// The bytes of a C string, empty for NULL.
///
/// # Safety
/// `p` is NULL or points to a NUL-terminated string that lives as long as `'a`.
unsafe fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        // SAFETY: guaranteed by the caller.
        unsafe { CStr::from_ptr(p) }.to_bytes()
    }
}

/// The bytes of a fixed colour array up to the first NUL.
fn arr(a: &[c_char]) -> &[u8] {
    let len = a.iter().position(|&c| c == 0).unwrap_or(a.len());
    // SAFETY: `c_char` and `u8` have the same size and alignment, and `len <= a.len()`.
    unsafe { std::slice::from_raw_parts(a.as_ptr().cast::<u8>(), len) }
}

/// `Flag` of C++ is three-valued: -1 undefined, 0 false, 1 true; only true counts.
fn flag(v: i32) -> bool {
    v == 1
}

/// Builds the `Opts` snapshot from `ugrep_settings` and the `--hyperlink` strings that C++ passes.
pub fn opts<'a>(hyperlink_prefix: &'a [u8], hyperlink_host: &'a [u8], hyperlink_path: &'a [u8]) -> Opts<'a> {
    // SAFETY: the settings are written by C++ option parsing before any output is produced and are only read here;
    // the pointers inside refer to C strings that live for the whole process.
    let s: &'a Settings = unsafe { &*crate::settings_ffi::UGREP_SETTINGS.as_ptr() };
    // SAFETY: the colour and separator pointers are NULL or NUL-terminated strings with process lifetime (see above).
    let (hl, st, del, off, sep) = unsafe {
        (
            (!s.color_hl.is_null()).then(|| cstr(s.color_hl)),
            cstr(s.color_st),
            cstr(s.color_del),
            cstr(s.color_off),
            cstr(s.flag_separator),
        )
    };
    Opts {
        color_sl: arr(&s.color_sl),
        color_cx: arr(&s.color_cx),
        color_fn: arr(&s.color_fn),
        color_ln: arr(&s.color_ln),
        color_cn: arr(&s.color_cn),
        color_bn: arr(&s.color_bn),
        color_se: arr(&s.color_se),
        color_mt: arr(&s.color_mt),
        color_ms: arr(&s.color_ms),
        color_mc: arr(&s.color_mc),
        match_ms: arr(&s.match_ms),
        match_mc: arr(&s.match_mc),
        match_off: arr(&s.match_off),
        color_off: off,
        color_st: st,
        color_del: del,
        color_hl: hl,
        color: !s.flag_color.is_null(),
        no_filename: s.flag_no_filename,
        line_number: flag(s.flag_line_number),
        column_number: flag(s.flag_column_number),
        byte_offset: flag(s.flag_byte_offset),
        query: s.flag_query,
        text: flag(s.flag_text),
        null: s.flag_null,
        heading: flag(s.flag_heading),
        initial_tab: flag(s.flag_initial_tab),
        hyperlink_line: s.flag_hyperlink_line,
        hex_star: s.flag_hex_star,
        hex_cbr: s.flag_hex_cbr,
        hex_chr: s.flag_hex_chr,
        hex_hbr: s.flag_hex_hbr,
        hex_columns: s.flag_hex_columns,
        width: s.flag_width,
        separator: sep,
        with_filename: s.flag_with_filename,
        brk: flag(s.flag_break),
        files_with_matches: s.flag_files_with_matches,
        count: s.flag_count,
        invert_match: s.flag_invert_match,
        only_matching: s.flag_only_matching,
        hex_all: s.flag_hex,
        with_hex: s.flag_with_hex,
        match_all: s.flag_match,
        ungroup: flag(s.flag_ungroup),
        fuzzy: s.flag_fuzzy,
        hyperlink_prefix,
        hyperlink_host,
        hyperlink_path,
    }
}
