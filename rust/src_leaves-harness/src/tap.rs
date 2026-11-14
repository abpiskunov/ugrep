//! tap.rs - seam tap for the Rust side. TEMPLATE: copy into the harness crate
//! (`rust/<component>-harness/src/tap.rs`), never into a product crate.
//! Emits the same cpp2rust-trace/1 events as `cpp2rust_tap.hpp`. No dependencies.
//!
//! The Rust side is normally driven by REPLAY: the harness reads the recorded C++ trace,
//! calls the Rust function for every inbound `call`, and answers the function's outbound
//! calls from the recorded `out_ret` events (see `Replay`). So the Rust unit is judged with
//! the inputs the real C++ saw, including injected faults, without linking any C++.
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::Write as _;

/// Neutral encoding: must produce exactly the text `to_neutral` produces in C++.
pub trait ToNeutral {
    fn to_neutral(&self) -> String;
}
macro_rules! int_neutral { ($($t:ty),*) => { $(impl ToNeutral for $t { fn to_neutral(&self) -> String { self.to_string() } })* } }
int_neutral!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
impl ToNeutral for bool {
    fn to_neutral(&self) -> String { self.to_string() }
}
impl ToNeutral for f64 {
    // C++ writes "%.17g" as text; format_g17 reproduces it so floats compare bit-faithfully.
    fn to_neutral(&self) -> String { format!("\"{}\"", format_g17(*self)) }
}
impl ToNeutral for f32 {
    fn to_neutral(&self) -> String { (*self as f64).to_neutral() }
}
impl ToNeutral for [u8] {
    fn to_neutral(&self) -> String { quote(self) }
}
impl ToNeutral for str {
    fn to_neutral(&self) -> String { quote(self.as_bytes()) }
}
impl ToNeutral for String {
    fn to_neutral(&self) -> String { quote(self.as_bytes()) }
}
impl<T: ToNeutral> ToNeutral for Option<T> {
    fn to_neutral(&self) -> String { self.as_ref().map_or("null".into(), |v| v.to_neutral()) }
}
impl<T: ToNeutral> ToNeutral for Vec<T> {
    fn to_neutral(&self) -> String { format!("[{}]", self.iter().map(|v| v.to_neutral()).collect::<Vec<_>>().join(",")) }
}
impl<T: ToNeutral + ?Sized> ToNeutral for &T {
    fn to_neutral(&self) -> String { (**self).to_neutral() }
}

/// Bytes as a JSON string with the same escaping as the C++ tap (bytes, not UTF-8).
pub fn quote(bytes: &[u8]) -> String {
    let mut o = String::from("\"");
    for &c in bytes {
        match c {
            b'"' | b'\\' => { o.push('\\'); o.push(c as char) }
            0x20..=0x7e => o.push(c as char),
            _ => { let _ = write!(o, "\\u{:04x}", c); }
        }
    }
    o.push('"');
    o
}

/// printf("%.17g") for f64.
pub fn format_g17(v: f64) -> String {
    if v.is_nan() { return if v.is_sign_negative() { "-nan".into() } else { "nan".into() }; }
    if v.is_infinite() { return if v < 0.0 { "-inf".into() } else { "inf".into() }; }
    if v == 0.0 { return if v.is_sign_negative() { "-0".into() } else { "0".into() }; }
    let sci = format!("{:.16e}", v);                      // d.dddddddddddddddde[-]x
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let trim = |s: &str| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() };
    if exp < -4 || exp >= 17 {
        format!("{}e{}{:02}", trim(mant), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        trim(&format!("{:.*}", (16 - exp).max(0) as usize, v))
    }
}

/// Event writer for one case on one logical thread.
pub struct Tap {
    out: Option<std::fs::File>,
    case: String,
    seq: u64,
    current_in: String,
}

impl Tap {
    /// Records to the file named by CPP2RUST_TAP_OUT; without it the tap is silent.
    pub fn from_env(case: &str) -> Tap {
        let out = std::env::var_os("CPP2RUST_TAP_OUT")
            .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
        Tap { out, case: case.to_string(), seq: 0, current_in: String::new() }
    }
    pub fn to_file(path: &std::path::Path, case: &str) -> std::io::Result<Tap> {
        let out = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Tap { out: Some(out), case: case.to_string(), seq: 0, current_in: String::new() })
    }
    fn emit(&mut self, kind: &str, func: &str, fields: &str) {
        let Some(f) = self.out.as_mut() else { return };
        let inn = if self.current_in.is_empty() { String::new() } else { format!(",\"in\":{}", quote(self.current_in.as_bytes())) };
        let sep = if fields.is_empty() { "" } else { "," };
        let _ = writeln!(f, "{{\"impl\":\"rust\",\"case\":{},\"thread\":0,\"seq\":{},\"kind\":\"{}\",\"fn\":{}{}{}{}}}",
            quote(self.case.as_bytes()), self.seq, kind, quote(func.as_bytes()), inn, sep, fields);
        self.seq += 1;
    }
    pub fn call(&mut self, func: &str, args: &[&dyn ToNeutral]) {
        self.current_in = func.to_string();
        let a = args.iter().map(|v| v.to_neutral()).collect::<Vec<_>>().join(",");
        self.emit("call", func, &format!("\"args\":[{}]", a));
    }
    pub fn state(&mut self, func: &str, v: &dyn ToNeutral) { self.emit("state", func, &format!("\"state\":{}", v.to_neutral())) }
    /// A state event of `func` recorded after its `ret` (the C++ driver's observations of errno and the FILE).
    pub fn state_after(&mut self, func: &str, v: &dyn ToNeutral) {
        self.current_in = func.to_string();
        self.state(func, v);
        self.current_in.clear();
    }
    pub fn case(&self) -> &str { &self.case }
    pub fn ret(&mut self, func: &str, v: &dyn ToNeutral) { self.emit("ret", func, &format!("\"ret\":{}", v.to_neutral())); self.current_in.clear() }
    pub fn err(&mut self, func: &str, v: &dyn ToNeutral) { self.emit("ret", func, &format!("\"err\":{}", v.to_neutral())); self.current_in.clear() }
    pub fn out_call(&mut self, callee: &str, args: &[&dyn ToNeutral]) {
        let a = args.iter().map(|v| v.to_neutral()).collect::<Vec<_>>().join(",");
        self.emit("out_call", callee, &format!("\"args\":[{}]", a));
    }
    /// `raw` is the recorded neutral JSON text (replay) or a freshly encoded value (live).
    pub fn out_ret_raw(&mut self, callee: &str, key: &str, raw: &str) { self.emit("out_ret", callee, &format!("\"{}\":{}", key, raw)) }
}

/// Recorded answers of outbound calls for one inbound call, in recorded order.
/// The generated dependency stub pops the next answer for its callee. A callee that does not
/// match the recording is an interaction divergence: the stub returns `Diverged`, the port
/// returns its error, and trace_diff reports the first divergent outbound call.
pub struct Replay {
    pub answers: VecDeque<Recorded>,
}
pub struct Recorded {
    pub callee: String,
    /// "ret" or "err"
    pub key: String,
    /// neutral JSON text exactly as recorded
    pub raw: String,
}
pub enum Answer {
    Ret(String),
    Err(String),
    Diverged,
}
impl Replay {
    pub fn next(&mut self, tap: &mut Tap, callee: &str, args: &[&dyn ToNeutral]) -> Answer {
        tap.out_call(callee, args);
        match self.answers.front() {
            Some(r) if r.callee == callee => {
                let r = self.answers.pop_front().unwrap();
                tap.out_ret_raw(callee, &r.key, &r.raw);
                if r.key == "err" { Answer::Err(r.raw) } else { Answer::Ret(r.raw) }
            }
            _ => Answer::Diverged,
        }
    }
}
