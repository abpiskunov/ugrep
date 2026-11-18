//! `set_color`, `trim` and `copy_color` of src/ugrep.cpp: GREP_COLORS to ANSI SGR strings and white-space trimming.
//!
//! Strings are byte slices; a C string ends at the first NUL or at the end of the slice.

use crate::signatures::TrimRange;

/// `isspace` in the C locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The bytes before the first NUL.
pub fn c_str(s: &[u8]) -> &[u8] {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    &s[..end]
}

/// `strstr`: the offset of the first occurrence of `needle` (the empty needle is found at 0).
pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Append `codes` to `color` at `t`, after a `;` when a code precedes.
fn emit(color: &mut [u8], t: &mut usize, codes: &[u8], sep: &mut bool) {
    if *sep {
        color[*t] = b';';
        *t += 1;
    }
    for &b in codes {
        color[*t] = b;
        *t += 1;
    }
    *sep = true;
}

/// `set_color(colors, parameter, color)`: `color` is the `char[COLORLEN]` buffer; it is left alone when `parameter`
/// is not in `colors`, else it receives `ESC [ ... m` and a NUL, or an empty string when no codes follow.
pub fn set_color(colors: &[u8], parameter: &[u8], color: &mut [u8]) {
    let colors = c_str(colors);
    let parameter = c_str(parameter);
    let Some(pos) = find(colors, parameter) else {
        return;
    };
    let s = &colors[pos + parameter.len()..];
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let len = color.len();
    let mut i = 0;
    let mut t = 2;

    // foreground colors: k=black, r=red, g=green, y=yellow b=blue, m=magenta, c=cyan, w=white
    // background colors: K R G Y B M C W, bright colors: +k ... +W
    // modifiers: h=highlight, u=underline, i=invert, f=faint, n=normal, H, U, I turn them off
    let mut offset: u8 = 30;
    let mut sep = false;

    while at(i) != 0 && at(i) != b':' && t < len.saturating_sub(6) {
        let c = at(i);
        if c.is_ascii_digit() {
            if sep {
                color[t] = b';';
                t += 1;
            }
            if offset == 90 {
                color[t] = b'1';
                color[t + 1] = b';';
                t += 2;
                offset = 30;
            }
            color[t] = c;
            t += 1;
            i += 1;
            while at(i).is_ascii_digit() && t < len.saturating_sub(2) {
                color[t] = at(i);
                t += 1;
                i += 1;
            }
            sep = true;
            continue;
        }

        match c {
            b'+' => offset = 90,
            b'n' => emit(color, &mut t, b"0", &mut sep),
            b'h' => emit(color, &mut t, b"1", &mut sep),
            b'H' => {
                emit(color, &mut t, b"21", &mut sep);
                offset = 30;
            }
            b'f' => emit(color, &mut t, b"2", &mut sep),
            b'u' => emit(color, &mut t, b"4", &mut sep),
            b'U' => emit(color, &mut t, b"24", &mut sep),
            b'i' => emit(color, &mut t, b"7", &mut sep),
            b'I' => emit(color, &mut t, b"27", &mut sep),
            b',' | b';' => {
                if sep {
                    color[t] = b';';
                    t += 1;
                }
                sep = false;
            }
            _ if is_space(c) => {
                if sep {
                    color[t] = b';';
                    t += 1;
                }
                sep = false;
            }
            _ => {
                if let Some(k) = b"krgybmcw  KRGYBMCW".iter().position(|&x| x == c) {
                    if sep {
                        color[t] = b';';
                        t += 1;
                    }
                    let mut n = offset + k as u8;
                    if n >= 100 {
                        color[t] = b'1';
                        t += 1;
                        n -= 100;
                    }
                    color[t] = b'0' + n / 10;
                    color[t + 1] = b'0' + n % 10;
                    t += 2;
                    offset = 30;
                    sep = true;
                }
            }
        }
        i += 1;
    }

    if t > 2 {
        color[0] = 0x1b;
        color[1] = b'[';
        color[t] = b'm';
        color[t + 1] = 0;
    } else {
        color[0] = 0;
    }
}

/// `copy_color(to, from)`: copy at most `COLORLEN - 1` bytes of the C string `from` and cut at the first comma.
pub fn copy_color(to: &mut [u8], from: &[u8]) {
    let from = c_str(from);
    let len = from.len().min(to.len().saturating_sub(1));
    to[..len].copy_from_slice(&from[..len]);
    to[len] = 0;
    if let Some(comma) = to[..len].iter().position(|&b| b == b',') {
        to[comma] = 0;
    }
}

/// `trim(line)`: the part of `line` left after removing white space at both ends.
pub fn trim(line: &[u8]) -> TrimRange {
    let begin = line.iter().take_while(|&&b| is_space(b)).count();
    let rest = &line[begin..];
    let tail = rest.iter().rev().take_while(|&&b| is_space(b)).count();
    TrimRange { begin, end: begin + rest.len() - tail }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(colors: &str, parameter: &str) -> Vec<u8> {
        let mut buf = [0xaa_u8; 32];
        set_color(colors.as_bytes(), parameter.as_bytes(), &mut buf);
        c_str(&buf).to_vec()
    }

    #[test]
    fn numeric_and_easy_codes() {
        assert_eq!(color("mt=1;31:fn=1;35", "mt="), b"\x1b[1;31m");
        assert_eq!(color("mt=1;31:fn=1;35", "fn="), b"\x1b[1;35m");
        assert_eq!(color("ms=hr", "ms="), b"\x1b[1;31m");
        assert_eq!(color("ms=+r", "ms="), b"\x1b[91m");
        assert_eq!(color("ms=+W", "ms="), b"\x1b[107m");
        assert_eq!(color("ms=+12", "ms="), b"\x1b[1;12m");
        assert_eq!(color("ms=H", "ms="), b"\x1b[21m");
    }

    #[test]
    fn empty_and_missing() {
        assert_eq!(color("ms=:fn=1", "ms="), b"");
        let mut buf = [7_u8; 32];
        set_color(b"fn=1", b"ms=", &mut buf);
        assert_eq!(buf, [7_u8; 32]);
        assert_eq!(color("4", ""), b"\x1b[4m");
    }

    #[test]
    fn long_codes_stay_in_the_buffer() {
        let out = color("ms=1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18", "ms=");
        assert!(out.len() < 32 && out.ends_with(b"m"));
    }

    #[test]
    fn copy_cuts_at_comma() {
        let mut to = [0_u8; 32];
        copy_color(&mut to, b"\x1b[1,2m");
        assert_eq!(c_str(&to), b"\x1b[1");
    }

    #[test]
    fn trim_ranges() {
        assert_eq!((trim(b"").begin, trim(b"").end), (0, 0));
        let r = trim(b"  a b \t");
        assert_eq!((r.begin, r.end), (2, 5));
        let r = trim(b" \t ");
        assert_eq!((r.begin, r.end), (3, 3));
    }
}
