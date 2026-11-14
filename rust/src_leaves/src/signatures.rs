//! Types and signatures of the port units of `src_leaves`, fixed by the Prepare seam task.
//! The unit `glob` is ported here (port of src/glob.cpp, gitignore-style globbing): the replay
//! harness includes this file alone, so the port lives in it. Strings are NUL-terminated in C++;
//! here they are byte slices where a read past the end yields NUL, so the pointer arithmetic keeps its shape.
//! The unit `zopen` (src/zopen.c) is the submodule `zopen` (src/zopen.rs next to this file): a `mod`
//! declared here is resolved by the harness's `#[path]` include without any harness change.
#[path = "zopen.rs"]
pub mod zopen;

#[cfg(windows)]
const PATHSEP: u8 = b'\\';
#[cfg(not(windows))]
const PATHSEP: u8 = b'/';

fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn has(s: &[u8], from: usize, c: u8) -> bool {
    s.get(from..).map(|t| t.contains(&c)).unwrap_or(false)
}

fn ascii_lower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

/// Wide character of the UTF-8 multi-byte sequence at `*i`, ASCII lower case if `icase`.
fn utf8(s: &[u8], i: &mut usize, icase: bool) -> i32 {
    let c = at(s, *i) as i32;
    if c != 0 {
        *i += 1;
    }
    if c < 0x80 {
        return if icase { ascii_lower(c as u8) as i32 } else { c };
    }
    let c1 = at(s, *i) as i32;
    if c < 0xC0 || (c == 0xC0 && c1 != 0x80) || c == 0xC1 || (c1 & 0xC0) != 0x80 {
        return 0xFFFD;
    }
    if c1 != 0 {
        *i += 1;
    }
    let c1 = c1 & 0x3F;
    if c < 0xE0 {
        return ((c & 0x1F) << 6) | c1;
    }
    let c2 = at(s, *i) as i32;
    if (c == 0xE0 && c1 < 0x20) || (c2 & 0xC0) != 0x80 {
        return 0xFFFD;
    }
    if c2 != 0 {
        *i += 1;
    }
    let c2 = c2 & 0x3F;
    if c < 0xF0 {
        return ((c & 0x0F) << 12) | (c1 << 6) | c2;
    }
    let c3 = at(s, *i) as i32;
    if c3 != 0 {
        *i += 1;
    }
    if (c == 0xF0 && c1 < 0x10) || (c == 0xF4 && c1 >= 0x10) || c >= 0xF5 || (c3 & 0xC0) != 0x80 {
        return 0xFFFD;
    }
    ((c & 0x07) << 18) | (c1 << 12) | (c2 << 6) | (c3 & 0x3F)
}

fn matches(text: &[u8], glob: &[u8], icase: bool, lead: bool, path: bool) -> bool {
    let (mut t, mut g) = (0usize, 0usize);
    let mut text1_backup: Option<usize> = None;
    let mut glob1_backup: Option<usize> = None;
    let mut text2_backup: usize = 0;
    let mut glob2_backup: Option<usize> = None;

    'outer: while at(text, t) != 0 {
        'sw: {
            match at(glob, g) {
                b'*' => {
                    g += 1;
                    if at(glob, g) == b'*' {
                        g += 1;
                        if at(glob, g) == 0 {
                            return true;
                        }
                        if at(glob, g) != b'/' {
                            return false;
                        }
                        text1_backup = None;
                        glob1_backup = None;
                        text2_backup = t;
                        glob2_backup = Some(g);
                        if at(text, t) != b'/' {
                            g += 1;
                        }
                        continue 'outer;
                    }
                    text1_backup = Some(t);
                    glob1_backup = Some(g);
                    continue 'outer;
                }
                b'?' => {
                    if at(text, t) == PATHSEP {
                        break 'sw;
                    }
                    utf8(text, &mut t, false);
                    g += 1;
                    continue 'outer;
                }
                b'[' => {
                    let mut chr = utf8(text, &mut t, icase);
                    let mut last: i32 = 0x10ffff;
                    if chr == PATHSEP as i32 {
                        break 'sw;
                    }
                    let mut matched = false;
                    let reverse = at(glob, g + 1) == b'^' || at(glob, g + 1) == b'!';
                    if reverse {
                        g += 1;
                    }
                    g += 1;
                    let lc = chr;
                    let uc = if icase && (b'a' as i32..=b'z' as i32).contains(&lc) { lc - 32 } else { chr };
                    if lc == uc {
                        while at(glob, g) != 0 && at(glob, g) != b']' {
                            let hit = if last < 0x10ffff && at(glob, g) == b'-' && at(glob, g + 1) != b']' && at(glob, g + 1) != 0 {
                                g += 1;
                                chr <= utf8(glob, &mut g, false) && chr >= last
                            } else {
                                last = utf8(glob, &mut g, false);
                                chr == last
                            };
                            if hit {
                                matched = true;
                            }
                        }
                    } else {
                        while at(glob, g) != 0 && at(glob, g) != b']' {
                            let hit = if last < 0x10ffff && at(glob, g) == b'-' && at(glob, g + 1) != b']' && at(glob, g + 1) != 0 {
                                g += 1;
                                chr = utf8(glob, &mut g, false);
                                (lc <= chr && lc >= last) || (uc <= chr && uc >= last)
                            } else {
                                last = utf8(glob, &mut g, false);
                                lc == last || uc == last
                            };
                            if hit {
                                matched = true;
                            }
                        }
                    }
                    if matched == reverse {
                        break 'sw;
                    }
                    if at(glob, g) != 0 {
                        g += 1;
                    }
                    continue 'outer;
                }
                b'/' => {
                    if at(text, t) != PATHSEP {
                        break 'sw;
                    }
                    t += 1;
                    g += 1;
                    continue 'outer;
                }
                c => {
                    if c == b'\\' {
                        g += 1;
                    }
                    let (a, b) = (at(glob, g), at(text, t));
                    let differ = if icase { ascii_lower(a) != ascii_lower(b) } else { a != b };
                    if differ {
                        break 'sw;
                    }
                    t += 1;
                    g += 1;
                    continue 'outer;
                }
            }
        }

        if path && at(glob, g) == 0 && at(text, t) == PATHSEP && !has(text, t + 1, PATHSEP) {
            return true;
        }

        if let Some(gb) = glob1_backup {
            let tb = text1_backup.unwrap_or(0);
            if at(text, tb) != PATHSEP {
                text1_backup = Some(tb + 1);
                t = tb + 1;
                g = gb;
                continue;
            }
        }

        if let Some(gb) = glob2_backup {
            text2_backup += 1;
            t = text2_backup;
            g = gb;
            continue;
        }

        return false;
    }

    while at(glob, g) == b'*' {
        g += 1;
    }
    (at(glob, g) == 0 && !path) || (at(glob, g) == b'/' && lead)
}

/// C++ `bool glob_match(const char *pathname, const char *basename, const char *glob,
/// bool icase, bool lead, bool path)` from src/glob.hpp. Text arguments are bytes.
pub fn glob_match(pathname: &[u8], basename: &[u8], glob: &[u8], icase: bool, lead: bool, path: bool) -> bool {
    let pathname = cstr(pathname);
    let basename = cstr(basename);
    let mut glob = cstr(glob);

    let mut p = 0usize;
    while at(pathname, p) == b'.' && at(pathname, p + 1) == PATHSEP {
        p += 2;
    }
    while at(pathname, p) == PATHSEP {
        p += 1;
    }
    let pathname = &pathname[p..];

    if glob.contains(&b'/') {
        if at(glob, 0) == b'.' && at(glob, 1) == b'/' {
            glob = &glob[2..];
        } else if at(glob, 0) == b'/' {
            glob = &glob[1..];
        }
        if !glob.is_empty() {
            return matches(pathname, glob, icase, lead, path);
        }
        return pathname.is_empty() || (path && !pathname.contains(&PATHSEP));
    }

    path || matches(basename, glob, icase, false, false)
}
#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn matches_basic_patterns() {
        assert!(glob_match(b"a/foo.c", b"foo.c", b"*.c", false, false, false));
        assert!(!glob_match(b"a/foo.h", b"foo.h", b"*.c", false, false, false));
        assert!(glob_match(b"FOO.C", b"FOO.C", b"*.c", true, false, false));
    }
}
