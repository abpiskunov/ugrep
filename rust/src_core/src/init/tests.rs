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

#[test]
fn extension_globs_keep_brackets_and_escapes() {
    assert_eq!(glob_item_end(b"a,b", 0), 1);
    assert_eq!(glob_item_end(b"[a,b],c", 0), 5);
    assert_eq!(glob_item_end(b"abc", 0), 3);
}
