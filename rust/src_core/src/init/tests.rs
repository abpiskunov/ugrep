use super::*;

#[test]
fn strncmp_and_setw_helpers() {
    assert!(strncmp_eq(b"ug", "ug", 2));
    assert!(strncmp_eq(b"", "ug", 0));
    assert!(!strncmp_eq(b"ugx", "ug", 3));
    assert!(strncmp_eq(b"u", "ug", 1));
    assert_eq!(setw(5, b"ab"), b"   ab");
    assert_eq!(setw(1, b"abc"), b"abc");
    assert_eq!(negate_items(b"a,b"), b"!a,!b");
}

struct HexEnv {
    settings: Settings,
    usages: Vec<Vec<u8>>,
}

impl Env for HexEnv {
    fn settings(&mut self) -> &mut Settings {
        &mut self.settings
    }
    fn c_string(&mut self, p: *const c_char) -> Vec<u8> {
        // SAFETY: the tests pass pointers to NUL-terminated literals.
        unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes().to_vec()
    }
    fn usage(&mut self, message: &[u8]) {
        self.usages.push(message.to_vec());
    }
    fn set_home_dir(&mut self) { unimplemented!() }
    fn cerr(&mut self, _: &[u8]) { unimplemented!() }
    fn exit(&mut self, _: i32) { unimplemented!() }
    fn warning(&mut self, _: Option<&[u8]>, _: &[u8]) { unimplemented!() }
    fn error(&mut self, _: &[u8], _: &[u8]) { unimplemented!() }
    fn abort(&mut self, _: &[u8]) { unimplemented!() }
    fn abort_what(&mut self, _: &[u8], _: &[u8]) { unimplemented!() }
    fn load_config(&mut self) { unimplemented!() }
    fn save_config(&mut self) { unimplemented!() }
    fn warnings_reset(&mut self) { unimplemented!() }
    fn warnings(&mut self) -> usize { unimplemented!() }
    fn options(&mut self) { unimplemented!() }
    fn terminal(&mut self) { unimplemented!() }
    fn patterns(&mut self) -> Vec<(i32, *const c_char)> { unimplemented!() }
    fn bcnf_new_pattern(&mut self, _: i32, _: *const c_char) { unimplemented!() }
    fn bcnf_new_pattern_regexp(&mut self) { unimplemented!() }
    fn bcnf_new_term(&mut self) { unimplemented!() }
    fn regexp_append(&mut self, _: &[u8]) { unimplemented!() }
    fn list_all(&mut self, _: List) -> Vec<Vec<u8>> { unimplemented!() }
    fn list_push(&mut self, _: List, _: &[u8]) { unimplemented!() }
    fn list_truncate(&mut self, _: List, _: usize, _: usize) { unimplemented!() }
    fn ignore_file_erase(&mut self, _: &[u8]) { unimplemented!() }
    fn arg_pattern(&mut self) -> *const c_char { unimplemented!() }
    fn set_arg_pattern(&mut self, _: *const c_char) { unimplemented!() }
    fn arg_files_len(&mut self) -> usize { unimplemented!() }
    fn arg_file(&mut self, _: usize) -> *const c_char { unimplemented!() }
    fn arg_files_erase(&mut self, _: usize) { unimplemented!() }
    fn arg_files_insert_front(&mut self, _: *const c_char) { unimplemented!() }
    fn expand_arg_files(&mut self) { unimplemented!() }
    fn check_arg_file(&mut self, _: *const c_char, _: bool) -> ArgFile { unimplemented!() }
    fn trim_pathname_arg(&mut self, _: *const c_char) { unimplemented!() }
    fn file_open(&mut self, _: &[u8]) -> bool { unimplemented!() }
    fn file_getline(&mut self) -> (bool, usize) { unimplemented!() }
    fn arg_files_push_line(&mut self) { unimplemented!() }
    fn file_close(&mut self) { unimplemented!() }
    fn import_globs(&mut self, _: bool, _: bool) { unimplemented!() }
    fn fs_probe(&mut self, _: &[u8]) -> Option<u64> { unimplemented!() }
    fn fs_insert(&mut self, _: bool, _: u64) { unimplemented!() }
    fn type_entry(&mut self, _: usize) -> Option<TypeEntry> { unimplemented!() }
    fn encoding_entry(&mut self, _: usize) -> Option<(Vec<u8>, u16)> { unimplemented!() }
    fn magic_pattern_assign(&mut self, _: Option<&[u8]>) -> Option<Vec<u8>> { unimplemented!() }
    fn filter_magic_assign(&mut self, _: &[u8]) -> Option<Vec<u8>> { unimplemented!() }
    fn filter_empty(&mut self) -> bool { unimplemented!() }
    fn isatty_stdin(&mut self) -> bool { unimplemented!() }
    fn set_binary_stdout(&mut self) { unimplemented!() }
}

fn run_hexdump(arg: &'static std::ffi::CStr) -> HexEnv {
    let mut env = HexEnv { settings: Settings::DEFAULT, usages: Vec::new() };
    env.settings.flag_hexdump = arg.as_ptr();
    let k = Consts {
        auto: std::ptr::null(),
        pathsep: b'/',
        os_win: false,
        os_win_or_mingw: false,
        have_libz: false,
        have_regex_perl: false,
        have_stat_fs: false,
        cygwin: false,
        max_hex_columns: 64,
        encoding_null_data: 0,
    };
    hexdump(&mut env, &k);
    env
}

#[test]
fn hexdump_context_without_digit_reports_invalid_argument() {
    let msg = b"invalid argument --hexdump=[1-8][a][bch][A[NUM]][B[NUM]][C[NUM]]".to_vec();
    for arg in [c"Ax", c"C-", c"Bx", c"A"] {
        let env = run_hexdump(arg);
        if arg == c"A" {
            // no character after the letter: the scan ends normally
            assert!(env.usages.is_empty());
            assert!(env.settings.flag_hex);
            continue;
        }
        assert_eq!(env.usages, vec![msg.clone()], "{arg:?}");
        assert!(!env.settings.flag_hex, "{arg:?}");
    }
}

#[test]
fn hexdump_valid_forms_do_not_report() {
    let env = run_hexdump(c"2A3");
    assert!(env.usages.is_empty());
    assert!(env.settings.flag_hex);
    assert_eq!(env.settings.flag_hex_after, 4);
}

#[test]
fn hexdump_column_digit_error_reports_once() {
    let env = run_hexdump(c"x");
    assert_eq!(env.usages.len(), 1);
}

#[test]
fn extension_globs_keep_brackets_and_escapes() {
    assert_eq!(glob_item_end(b"a,b", 0), 1);
    assert_eq!(glob_item_end(b"[a,b],c", 0), 5);
    assert_eq!(glob_item_end(b"abc", 0), 3);
}
