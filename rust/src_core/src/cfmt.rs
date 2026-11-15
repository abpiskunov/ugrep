//! C `printf("%.Ng")` formatting for the stats report (MSVC runtime spelling of inf/nan).

/// `%.{prec}g` of `x`.
pub fn fmt_g(x: f64, prec: usize) -> String {
    if x.is_nan() {
        return if x.is_sign_negative() { "-nan(ind)".into() } else { "nan".into() };
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let p = prec.max(1);
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let e = format!("{:.*e}", p - 1, x);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if exp < -4 || exp >= p as i32 {
        let mant = strip_zeros(mant);
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", mant, sign, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp) as usize;
        strip_zeros(&format!("{:.*}", decimals, x)).to_string()
    }
}

fn strip_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

/// `100.0 * num / den` as the C++ evaluates it, with the x86 results for a zero denominator.
pub fn percent(num: usize, den: usize) -> f64 {
    let n = 100.0 * num as f64;
    if den == 0 {
        if n == 0.0 {
            // 0.0/0.0 is the negative quiet NaN on x86 ("-nan(ind)" in the MSVC runtime)
            return f64::from_bits(0xFFF8_0000_0000_0000);
        }
        return f64::INFINITY;
    }
    n / den as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_formats_like_printf() {
        assert_eq!(fmt_g(100.0, 4), "100");
        assert_eq!(fmt_g(33.333333, 4), "33.33");
        assert_eq!(fmt_g(0.0005, 3), "0.0005");
        assert_eq!(fmt_g(0.00001234, 3), "1.23e-05");
        assert_eq!(fmt_g(123456.0, 3), "1.23e+05");
        assert_eq!(fmt_g(0.0, 3), "0");
    }

    #[test]
    fn zero_denominator_spells_like_msvc() {
        assert_eq!(fmt_g(percent(0, 0), 4), "-nan(ind)");
        assert_eq!(fmt_g(percent(1, 0), 4), "inf");
    }
}
