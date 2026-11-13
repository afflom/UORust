//! IEEE 754 binary floating point: the map from core's float types to the
//! prelude's one model of every binary format.
//!
//! A format is the pair (exponent width, fraction width), and core's own
//! constants give it: `BITS`, `MANTISSA_DIGITS` and `MAX_EXP` of
//! `library/core/src/num/f*.rs`. A value is held as its bit pattern, a `Nat`
//! below `2^BITS`. So `to_bits`, `from_bits`, classification, comparison
//! and the sign operations are exact by construction.
//!
//! The model is LexLean, written once in the prelude
//! (`assets/project/src/Prelude.lex.tex`, the `float*` definitions). Each
//! definition takes the format's two widths first, and nothing in it is
//! written for one width. Arithmetic is correctly rounded as IEEE 754
//! §4.3.1 defines it, to nearest with ties to even. The exact result is
//! formed in `Nat`, with a sticky bit where it is not finite (division,
//! square root), and one rounder, `floatRound`, packs it. Where an input is
//! NaN, the result is that NaN, quieted (the first operand's, then the
//! second's). Where an operation makes a NaN of its own, the result is the
//! positive quiet NaN core's `NAN` is. Both are NaNs that Rust's semantics
//! allow (the `f32` docs, "NaN bit patterns").
//!
//! The tests evaluate the prelude's own text exactly, as the differential
//! oracle against the host's `f32` and `f64`, and `f16` through `f32`.

use std::collections::BTreeMap;
use std::path::Path;

/// An IEEE 754 binary interchange format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Format {
    /// The exponent field's width.
    pub(crate) exp: u32,
    /// The fraction field's width: `MANTISSA_DIGITS - 1`.
    pub(crate) frac: u32,
}

impl Format {
    /// The format of `BITS`, `MANTISSA_DIGITS` and `MAX_EXP`, when they
    /// describe a binary interchange format: the exponent field is what is
    /// left after the sign and the fraction, and `MAX_EXP` is `2^(exp-1)`.
    pub(crate) fn from_core(bits: u32, digits: u32, max_exp: i64) -> Option<Format> {
        let exp = bits.checked_sub(digits)?;
        let frac = digits.checked_sub(1)?;
        (exp >= 2 && frac >= 1 && exp < 63 && max_exp == 1i64 << (exp - 1))
            .then_some(Format { exp, frac })
    }

    /// The prelude's operation `op` of this format on the terms `args`.
    pub(crate) fn term(self, op: &str, args: &[&str]) -> String {
        let mut parts = vec![
            format!("{}::float{op}", crate::lift::PRELUDE),
            self.exp.to_string(),
            self.frac.to_string(),
        ];
        parts.extend(args.iter().map(|a| a.to_string()));
        format!("({})", parts.join(" "))
    }
}

/// The formats of the float types core defines: every `num/f<N>.rs` of
/// `library/core/src` whose `impl` block states `BITS`, `MANTISSA_DIGITS`
/// and `MAX_EXP` as literals.
pub(crate) fn formats(library: &Path) -> BTreeMap<String, Format> {
    let dir = library.join("core").join("src").join("num");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let float = name
            .strip_prefix('f')
            .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
            && path.extension().is_some_and(|e| e == "rs");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if let (true, Some(f)) = (float, format_of(&text)) {
            out.insert(name.to_string(), f);
        }
    }
    out
}

/// The format a float module's source states.
pub(crate) fn format_of(source: &str) -> Option<Format> {
    let constant = |name: &str| -> Option<i64> {
        source.lines().find_map(|l| {
            let rest = l.trim().strip_prefix("pub const ")?.strip_prefix(name)?;
            let (_, value) = rest.strip_prefix(':')?.split_once('=')?;
            value
                .trim()
                .trim_end_matches(';')
                .replace('_', "")
                .parse()
                .ok()
        })
    };
    let bits = u32::try_from(constant("BITS")?).ok()?;
    let digits = u32::try_from(constant("MANTISSA_DIGITS")?).ok()?;
    Format::from_core(bits, digits, constant("MAX_EXP")?)
}

/// A float literal as MIR prints it, without its type (`1.0E+16`, `-0`,
/// `9.99999974E-5`, `+Inf`, `NaN`), or one of the constants IEEE 754
/// fixes for the format by core's name for it (`NAN`, `MAX`, ...): the
/// prelude's term for its bit pattern, rounded to nearest, ties to even.
pub(crate) fn literal(f: Format, text: &str) -> Option<String> {
    if let Some((_, op)) = NAMED.iter().find(|(n, _)| *n == text) {
        return Some(f.term(op, &[]));
    }
    let (negative, body) = match text.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let sign = negative.to_string();
    match body {
        "Inf" => return Some(f.term("Signed", &[&sign, &f.term("Inf", &[])])),
        "NaN" => return Some(f.term("Signed", &[&sign, &f.term("Nan", &[])])),
        _ => {}
    }
    let (mantissa, exp10) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.trim_start_matches('+').parse::<i64>().ok()?),
        None => (body, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all = format!("{int}{frac}");
    if int.is_empty() || !all.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let digits = match all.trim_start_matches('0') {
        "" => "0",
        d => d,
    };
    let exp10 = exp10 - i64::try_from(frac.len()).ok()?;
    let (up, down) = (exp10.max(0).to_string(), (-exp10).max(0).to_string());
    Some(f.term("OfDecimal", &[&sign, digits, &up, &down]))
}

/// Core's constants that IEEE 754 fixes for a format, and the prelude's
/// definition of each.
const NAMED: &[(&str, &str)] = &[
    ("NAN", "Nan"),
    ("INFINITY", "Inf"),
    ("NEG_INFINITY", "NegInf"),
    ("MAX", "Max"),
    ("MIN", "Min"),
    ("MIN_POSITIVE", "MinPositive"),
    ("EPSILON", "Epsilon"),
];

/// The prelude's operation for each of core's float intrinsics, by the
/// intrinsic's name without the type (`floorf32` is `floor` at `f32`), and
/// its arity.
pub(crate) const INTRINSICS: &[(&str, &str, usize)] = &[
    ("fabs", "Abs", 1),
    ("copysign", "CopySign", 2),
    ("floor", "Floor", 1),
    ("ceil", "Ceil", 1),
    ("trunc", "Trunc", 1),
    ("round", "RoundHalfAway", 1),
    ("round_ties_even_", "RoundTiesEven", 1),
    ("rint", "RoundTiesEven", 1),
    ("sqrt", "Sqrt", 1),
    ("minimum", "Minimum", 2),
    ("maximum", "Maximum", 2),
    ("minimum_number_nsz_", "MinNum", 2),
    ("maximum_number_nsz_", "MaxNum", 2),
    ("minnum", "MinNum", 2),
    ("maxnum", "MaxNum", 2),
    // `fmuladd` may fuse or not; the model fuses, as `fma` does.
    ("fma", "Fma", 3),
    ("fmuladd", "Fma", 3),
];

/// The kind of a float method's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The format's value.
    Value,
    /// A `bool`.
    Test,
    /// The bit pattern itself, as the unsigned integer of the width
    /// (`to_bits`) or back (`from_bits`).
    Bits,
    /// An operator that assigns through `&mut`: the operation's value is
    /// the first operand's new value.
    Assign,
}

/// What a method or operator impl of a float type is in the prelude, by
/// the trait it implements (empty for an inherent method) and its name:
/// the operation, its arity, and its kind.
pub(crate) const METHODS: &[(&str, &str, &str, usize, Kind)] = &[
    ("Add", "add", "Add", 2, Kind::Value),
    ("Sub", "sub", "Sub", 2, Kind::Value),
    ("Mul", "mul", "Mul", 2, Kind::Value),
    ("Div", "div", "Div", 2, Kind::Value),
    ("Rem", "rem", "Rem", 2, Kind::Value),
    ("Neg", "neg", "Neg", 1, Kind::Value),
    ("AddAssign", "add_assign", "Add", 2, Kind::Assign),
    ("SubAssign", "sub_assign", "Sub", 2, Kind::Assign),
    ("MulAssign", "mul_assign", "Mul", 2, Kind::Assign),
    ("DivAssign", "div_assign", "Div", 2, Kind::Assign),
    ("RemAssign", "rem_assign", "Rem", 2, Kind::Assign),
    ("PartialEq", "eq", "Eq", 2, Kind::Test),
    ("PartialEq", "ne", "Ne", 2, Kind::Test),
    ("PartialOrd", "lt", "Lt", 2, Kind::Test),
    ("PartialOrd", "le", "Le", 2, Kind::Test),
    ("PartialOrd", "gt", "Gt", 2, Kind::Test),
    ("PartialOrd", "ge", "Ge", 2, Kind::Test),
    ("", "is_nan", "IsNan", 1, Kind::Test),
    ("", "is_infinite", "IsInfinite", 1, Kind::Test),
    ("", "is_finite", "IsFinite", 1, Kind::Test),
    ("", "is_normal", "IsNormal", 1, Kind::Test),
    ("", "is_subnormal", "IsSubnormal", 1, Kind::Test),
    ("", "is_sign_negative", "IsSignNegative", 1, Kind::Test),
    ("", "is_sign_positive", "IsSignPositive", 1, Kind::Test),
    ("", "to_bits", "", 1, Kind::Bits),
    ("", "from_bits", "", 1, Kind::Bits),
    ("", "abs", "Abs", 1, Kind::Value),
    ("", "copysign", "CopySign", 2, Kind::Value),
    ("", "min", "MinNum", 2, Kind::Value),
    ("", "max", "MaxNum", 2, Kind::Value),
    ("", "minimum", "Minimum", 2, Kind::Value),
    ("", "maximum", "Maximum", 2, Kind::Value),
    ("", "mul_add", "Fma", 3, Kind::Value),
    ("", "sqrt", "Sqrt", 1, Kind::Value),
    ("", "floor", "Floor", 1, Kind::Value),
    ("", "ceil", "Ceil", 1, Kind::Value),
    ("", "trunc", "Trunc", 1, Kind::Value),
    ("", "round", "RoundHalfAway", 1, Kind::Value),
    ("", "round_ties_even", "RoundTiesEven", 1, Kind::Value),
];

#[cfg(test)]
mod tests {
    //! The differential oracle: the prelude's `float*` definitions, read from
    //! its own text and evaluated exactly, against the host's arithmetic.

    use std::collections::BTreeMap;

    use super::*;

    // ---------------------------------------------------------------------------
    // Natural numbers of any size.

    /// A natural number, little-endian base `2^32`.
    #[derive(Clone, Debug, PartialEq, Eq, Default)]
    struct Big(Vec<u32>);

    impl From<u128> for Big {
        fn from(mut n: u128) -> Self {
            let mut v = Vec::new();
            while n > 0 {
                v.push(n as u32);
                n >>= 32;
            }
            Big(v)
        }
    }

    impl Big {
        fn trim(mut self) -> Self {
            while self.0.last() == Some(&0) {
                self.0.pop();
            }
            self
        }
        fn pow2(k: u32) -> Big {
            let mut v = vec![0u32; (k / 32) as usize + 1];
            v[(k / 32) as usize] = 1 << (k % 32);
            Big(v).trim()
        }
        fn is_zero(&self) -> bool {
            self.0.is_empty()
        }
        fn to_u128(&self) -> Option<u128> {
            if self.0.len() > 4 {
                return None;
            }
            Some(
                self.0
                    .iter()
                    .rev()
                    .fold(0u128, |acc, &d| (acc << 32) | u128::from(d)),
            )
        }
        fn bits(&self) -> u32 {
            match self.0.last() {
                None => 0,
                Some(top) => 32 * (self.0.len() as u32 - 1) + (32 - top.leading_zeros()),
            }
        }
        fn bit(&self, i: u32) -> bool {
            self.0
                .get((i / 32) as usize)
                .is_some_and(|d| d >> (i % 32) & 1 == 1)
        }
        fn add(&self, o: &Big) -> Big {
            let mut v = Vec::new();
            let mut carry = 0u64;
            for i in 0..self.0.len().max(o.0.len()) {
                let s = u64::from(*self.0.get(i).unwrap_or(&0))
                    + u64::from(*o.0.get(i).unwrap_or(&0))
                    + carry;
                v.push(s as u32);
                carry = s >> 32;
            }
            v.push(carry as u32);
            Big(v).trim()
        }
        /// Truncated subtraction, as `Nat`'s.
        fn sub(&self, o: &Big) -> Big {
            if self <= o {
                return Big::default();
            }
            let mut v = Vec::new();
            let mut borrow = 0i64;
            for i in 0..self.0.len() {
                let mut d = i64::from(self.0[i]) - i64::from(*o.0.get(i).unwrap_or(&0)) - borrow;
                borrow = i64::from(d < 0);
                if d < 0 {
                    d += 1 << 32;
                }
                v.push(d as u32);
            }
            Big(v).trim()
        }
        fn mul(&self, o: &Big) -> Big {
            let mut v = vec![0u64; self.0.len() + o.0.len() + 1];
            for (i, &a) in self.0.iter().enumerate() {
                let mut carry = 0u64;
                for (j, &b) in o.0.iter().enumerate() {
                    let t = v[i + j] + u64::from(a) * u64::from(b) + carry;
                    v[i + j] = t & 0xffff_ffff;
                    carry = t >> 32;
                }
                let mut k = i + o.0.len();
                while carry > 0 {
                    let t = v[k] + carry;
                    v[k] = t & 0xffff_ffff;
                    carry = t >> 32;
                    k += 1;
                }
            }
            Big(v.into_iter().map(|d| d as u32).collect()).trim()
        }
        fn shl(&self, k: u32) -> Big {
            self.mul(&Big::pow2(k))
        }
        /// Quotient and remainder; a zero divisor gives `(0, self)`, as `prim
        /// quotient` and `prim remainder` with their zero case.
        fn divmod(&self, d: &Big) -> (Big, Big) {
            if d.is_zero() {
                return (Big::default(), self.clone());
            }
            let mut q = Big::default();
            let mut r = Big::default();
            for i in (0..self.bits()).rev() {
                r = r.shl(1);
                if self.bit(i) {
                    r = r.add(&Big::from(1));
                }
                if *d <= r {
                    r = r.sub(d);
                    q = q.add(&Big::pow2(i));
                }
            }
            (q, r)
        }
        fn decimal(&self) -> String {
            if self.is_zero() {
                return "0".into();
            }
            let mut digits = Vec::new();
            let mut n = self.clone();
            let ten18 = Big::from(1_000_000_000_000_000_000u128);
            while !n.is_zero() {
                let (q, r) = n.divmod(&ten18);
                digits.push(r.to_u128().unwrap_or(0));
                n = q;
            }
            let mut out = digits.pop().unwrap_or(0).to_string();
            for d in digits.iter().rev() {
                out.push_str(&format!("{d:018}"));
            }
            out
        }
    }

    impl PartialOrd for Big {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Big {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            self.0
                .len()
                .cmp(&o.0.len())
                .then_with(|| self.0.iter().rev().cmp(o.0.iter().rev()))
        }
    }

    // ---------------------------------------------------------------------------
    // The LexLean the prelude's float definitions are written in: a reader
    // and an exact evaluator of that subset.

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum V {
        N(Big),
        B(bool),
    }

    impl V {
        fn n(self) -> Big {
            match self {
                V::N(b) => b,
                V::B(_) => panic!("a Bool where a Nat is due"),
            }
        }
        fn b(self) -> bool {
            match self {
                V::B(b) => b,
                V::N(_) => panic!("a Nat where a Bool is due"),
            }
        }
    }

    #[derive(Clone, Debug)]
    enum T {
        Num(Big),
        Var(String),
        App(String, Vec<T>),
        Prim(String, Vec<T>),
        Bin(String, Box<T>, Box<T>),
        Not(Box<T>),
        If(Box<T>, Box<T>, Box<T>),
        Let(String, Box<T>, Box<T>),
        /// `match x | Nat.zero => z | Nat.succ r => s`.
        Match(Box<T>, Box<T>, String, Box<T>),
    }

    fn tokens(s: &str) -> Vec<String> {
        let cs: Vec<char> = s.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c.is_whitespace() {
                i += 1;
            } else if c == '|' && cs.get(i + 1) == Some(&'|') {
                out.push("||".into());
                i += 2;
            } else if "();|,".contains(c) {
                out.push(c.to_string());
                i += 1;
            } else if c.is_ascii_alphanumeric() || c == '_' {
                let start = i;
                while i < cs.len()
                    && (cs[i].is_ascii_alphanumeric()
                        || cs[i] == '_'
                        || cs[i] == '.'
                        || (cs[i] == ':' && cs.get(i + 1) == Some(&':'))
                        || (cs[i] == ':' && i > 0 && cs[i - 1] == ':'))
                {
                    i += 1;
                }
                out.push(cs[start..i].iter().collect());
            } else {
                let start = i;
                while i < cs.len() && "<=?:+&>".contains(cs[i]) {
                    i += 1;
                }
                assert!(
                    i > start,
                    "a character LexLean's float subset has none of: {c:?}"
                );
                out.push(cs[start..i].iter().collect());
            }
        }
        out
    }

    struct Reader {
        toks: Vec<String>,
        at: usize,
    }

    impl Reader {
        fn next(&mut self) -> String {
            self.at += 1;
            self.toks[self.at - 1].clone()
        }
        fn peek(&self) -> &str {
            &self.toks[self.at]
        }
        fn expect(&mut self, t: &str) {
            let got = self.next();
            assert_eq!(got, t, "at token {}", self.at);
        }
        fn term(&mut self) -> T {
            let t = self.next();
            if t != "(" {
                return match t.parse::<u128>() {
                    Ok(n) => T::Num(Big::from(n)),
                    Err(_) => T::Var(t),
                };
            }
            match self.peek() {
                "match" => {
                    self.next();
                    let x = self.term();
                    self.expect("|");
                    self.expect("Nat.zero");
                    self.expect("=>");
                    let z = self.term();
                    self.expect("|");
                    self.expect("Nat.succ");
                    let r = self.next();
                    self.expect("=>");
                    let s = self.term();
                    self.expect(")");
                    T::Match(Box::new(x), Box::new(z), r, Box::new(s))
                }
                "if" => {
                    self.next();
                    let c = self.term();
                    self.expect("then");
                    let a = self.term();
                    self.expect("else");
                    let b = self.term();
                    self.expect(")");
                    T::If(Box::new(c), Box::new(a), Box::new(b))
                }
                "let" => {
                    self.next();
                    let x = self.next();
                    self.expect(":");
                    self.next();
                    self.expect(":=");
                    let v = self.term();
                    self.expect(";");
                    let b = self.term();
                    self.expect(")");
                    T::Let(x, Box::new(v), Box::new(b))
                }
                "prim" => {
                    self.next();
                    let op = self.next();
                    let mut args = Vec::new();
                    while self.peek() != ":" {
                        args.push(self.term());
                    }
                    self.next();
                    self.next();
                    self.expect(")");
                    T::Prim(op, args)
                }
                "not" => {
                    self.next();
                    let a = self.term();
                    self.expect(")");
                    T::Not(Box::new(a))
                }
                _ => {
                    let first = self.term();
                    if ["+", "<?", "<=?", "=?", "&&", "||"].contains(&self.peek()) {
                        let op = self.next();
                        let b = self.term();
                        self.expect(")");
                        return T::Bin(op, Box::new(first), Box::new(b));
                    }
                    let T::Var(f) = first else {
                        panic!("an application of a non-name")
                    };
                    let mut args = Vec::new();
                    while self.peek() != ")" {
                        args.push(self.term());
                    }
                    self.next();
                    T::App(f.trim_start_matches("Prelude::").to_string(), args)
                }
            }
        }
    }

    fn read(text: &str) -> T {
        let mut r = Reader {
            toks: tokens(text),
            at: 0,
        };
        r.term()
    }

    /// The prelude's float definitions: each one's parameters and body.
    type Defs = BTreeMap<String, (Vec<String>, T)>;

    fn prelude() -> Defs {
        let mut out = Defs::new();
        for chunk in crate::lift::PRELUDE_SOURCE.split("definition{").skip(1) {
            // A definition ends at its own closing brace: on a line of its own,
            // or the last character of a one-line definition.
            let chunk = match chunk.strip_prefix('\n') {
                Some(rest) => &rest[..rest.find("\n}").unwrap_or(rest.len()) + 2],
                None => chunk.lines().next().unwrap_or(chunk),
            };
            let field = |key: &str| chunk.split_once(key).map(|(_, r)| r);
            let name = field("name: ").unwrap().split(',').next().unwrap().trim();
            if !name.starts_with("float") {
                continue;
            }
            let params = field("parameters: [").unwrap().split(']').next().unwrap();
            let params = params
                .split(',')
                .map(|p| p.split(':').next().unwrap().trim().to_string())
                .collect();
            let body = field("body: ").unwrap();
            let end = body
                .find(",\n  recursive_argument")
                .or_else(|| body.find(", recursive_argument"))
                .unwrap_or_else(|| body.rfind('}').unwrap());
            out.insert(name.to_string(), (params, read(&body[..end])));
        }
        out
    }

    struct Eval {
        defs: Defs,
        memo: std::cell::RefCell<BTreeMap<(String, Vec<V>), V>>,
    }

    impl Eval {
        fn new() -> Eval {
            Eval {
                defs: prelude(),
                memo: Default::default(),
            }
        }
        fn eval(&self, t: &T, env: &BTreeMap<String, V>) -> V {
            let e = |t: &T| self.eval(t, env);
            match t {
                T::Num(b) => V::N(b.clone()),
                T::Var(x) if x == "true" => V::B(true),
                T::Var(x) if x == "false" => V::B(false),
                T::Var(x) => env.get(x).cloned().unwrap_or_else(|| panic!("unbound {x}")),
                T::Not(a) => V::B(!e(a).b()),
                T::If(c, a, b) => {
                    if e(c).b() {
                        e(a)
                    } else {
                        e(b)
                    }
                }
                T::Let(x, v, b) => {
                    let mut inner = env.clone();
                    inner.insert(x.clone(), e(v));
                    self.eval(b, &inner)
                }
                T::Match(x, z, r, s) => {
                    let n = e(x).n();
                    if n.is_zero() {
                        e(z)
                    } else {
                        let mut inner = env.clone();
                        inner.insert(r.clone(), V::N(n.sub(&Big::from(1))));
                        self.eval(s, &inner)
                    }
                }
                T::Bin(op, a, b) => match op.as_str() {
                    "&&" => V::B(e(a).b() && e(b).b()),
                    "||" => V::B(e(a).b() || e(b).b()),
                    _ => {
                        let (x, y) = (e(a).n(), e(b).n());
                        match op.as_str() {
                            "+" => V::N(x.add(&y)),
                            "<?" => V::B(x < y),
                            "<=?" => V::B(x <= y),
                            "=?" => V::B(x == y),
                            _ => panic!("operator {op}"),
                        }
                    }
                },
                T::Prim(op, args) => {
                    let a: Vec<Big> = args.iter().map(|t| e(t).n()).collect();
                    V::N(match op.as_str() {
                        "subtract" => a[0].sub(&a[1]),
                        "multiply" => a[0].mul(&a[1]),
                        // The third operand is the value at a zero divisor.
                        "quotient" if a[1].is_zero() => a[2].clone(),
                        "remainder" if a[1].is_zero() => a[2].clone(),
                        "quotient" => a[0].divmod(&a[1]).0,
                        "remainder" => a[0].divmod(&a[1]).1,
                        _ => panic!("primitive {op}"),
                    })
                }
                T::App(f, args) => {
                    let vals: Vec<V> = args.iter().map(e).collect();
                    self.call(f, vals)
                }
            }
        }
        fn call(&self, f: &str, vals: Vec<V>) -> V {
            let key = (f.to_string(), vals);
            if let Some(v) = self.memo.borrow().get(&key) {
                return v.clone();
            }
            let (params, body) = self
                .defs
                .get(f)
                .unwrap_or_else(|| panic!("no definition {f}"));
            let env = params.iter().cloned().zip(key.1.iter().cloned()).collect();
            let v = self.eval(body, &env);
            // The format's constants: calls on small arguments only, so the
            // table stays small.
            let small = key.1.iter().all(|a| match a {
                V::N(n) => n.bits() <= 16,
                V::B(_) => true,
            });
            if small {
                self.memo.borrow_mut().insert(key, v.clone());
            }
            v
        }
        /// An operation of `f` on bit patterns.
        fn op(&self, f: Format, op: &str, args: &[u128]) -> V {
            let mut vals = vec![
                V::N(Big::from(u128::from(f.exp))),
                V::N(Big::from(u128::from(f.frac))),
            ];
            vals.extend(args.iter().map(|a| V::N(Big::from(*a))));
            self.call(&format!("float{op}"), vals)
        }
        fn nat(&self, f: Format, op: &str, args: &[u128]) -> u128 {
            self.op(f, op, args).n().to_u128().unwrap()
        }
        fn term(&self, text: &str) -> u128 {
            self.eval(&read(text), &BTreeMap::new())
                .n()
                .to_u128()
                .unwrap()
        }
    }

    /// Run on a thread whose stack holds the evaluator's recursion.
    fn deep(f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(1 << 26)
            .spawn(f)
            .unwrap()
            .join()
            .unwrap();
    }

    const F16: Format = Format { exp: 5, frac: 10 };
    const F32: Format = Format { exp: 8, frac: 23 };
    const F64: Format = Format { exp: 11, frac: 52 };

    fn xorshift(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    /// Values that reach every branch: zeros, subnormals, the normal
    /// boundaries, infinities, NaNs, and a spread of ordinary values.
    fn samples32() -> Vec<f32> {
        let mut out = vec![
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            1.5,
            2.5,
            -2.5,
            3.0,
            1e-45,
            -1e-45,
            1.175_494_4e-38,
            1.175_494_2e-38,
            f32::MAX,
            f32::MIN,
            f32::EPSILON,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            // The ends of the signed ranges `as` saturates to.
            127.5,
            -128.5,
            -129.0,
            2_147_483_648.0,
            -2_147_483_648.0,
            -f32::NAN,
            f32::from_bits(0x7fa0_0001),
            16_777_216.0,
            16_777_217.0,
            1e10,
            123.456,
            0.1,
            1.0 + f32::EPSILON,
            8_388_607.5,
            4_294_967_295.0,
            4_294_967_296.0,
        ];
        let mut seed = 0x2545_f491_u64;
        for _ in 0..50 {
            out.push(f32::from_bits(xorshift(&mut seed) as u32));
        }
        out
    }

    fn same32(model: u128, host: f32) -> bool {
        if host.is_nan() {
            f32::from_bits(model as u32).is_nan()
        } else {
            model == u128::from(host.to_bits())
        }
    }

    fn bits(x: f32) -> u128 {
        u128::from(x.to_bits())
    }

    #[test]
    fn formats_are_read_from_cores_constants() {
        let src = "impl f32 {\n    pub const BITS: u32 = 32;\n    pub const MANTISSA_DIGITS: u32 = 24;\n    pub const MAX_EXP: i32 = 128;\n}";
        assert_eq!(format_of(src), Some(F32));
        let f128 = "pub const BITS: u32 = 128;\npub const MANTISSA_DIGITS: u32 = 113;\npub const MAX_EXP: i32 = 16_384;";
        assert_eq!(format_of(f128), Some(Format { exp: 15, frac: 112 }));
        assert_eq!(Format::from_core(32, 24, 127), None);
    }

    #[test]
    fn f32_arithmetic_is_the_hosts() {
        deep(|| {
            let ev = Eval::new();
            let xs = samples32();
            for &a in &xs {
                for &b in xs.iter().step_by(3) {
                    let (x, y) = (bits(a), bits(b));
                    for (op, host) in [
                        ("Add", a + b),
                        ("Sub", a - b),
                        ("Mul", a * b),
                        ("Div", a / b),
                        ("Rem", a % b),
                    ] {
                        let got = ev.nat(F32, op, &[x, y]);
                        assert!(
                            same32(got, host),
                            "{op} {a:e} {b:e}: model {got:#x}, host {:#x}",
                            host.to_bits()
                        );
                    }
                    for (op, host) in [
                        ("Eq", a == b),
                        ("Ne", a != b),
                        ("Lt", a < b),
                        ("Le", a <= b),
                        ("Gt", a > b),
                        ("Ge", a >= b),
                    ] {
                        assert_eq!(ev.op(F32, op, &[x, y]).b(), host, "{op} {a} {b}");
                    }
                }
            }
        });
    }

    #[test]
    fn f32_unary_operations_are_the_hosts() {
        deep(|| {
            let ev = Eval::new();
            for a in samples32() {
                let x = bits(a);
                for (op, host) in [
                    ("Trunc", a.trunc()),
                    ("Floor", a.floor()),
                    ("Ceil", a.ceil()),
                    ("RoundHalfAway", a.round()),
                    ("RoundTiesEven", a.round_ties_even()),
                    ("Sqrt", a.sqrt()),
                    ("Abs", a.abs()),
                    ("Neg", -a),
                ] {
                    let got = ev.nat(F32, op, &[x]);
                    assert!(same32(got, host), "{op} {a:e}: model {got:#x}");
                }
                for (op, host) in [
                    ("IsNan", a.is_nan()),
                    ("IsInfinite", a.is_infinite()),
                    ("IsFinite", a.is_finite()),
                    ("IsNormal", a.is_normal()),
                    ("IsSubnormal", a.is_subnormal()),
                    ("IsSignNegative", a.is_sign_negative()),
                    ("IsSignPositive", a.is_sign_positive()),
                ] {
                    assert_eq!(ev.op(F32, op, &[x]).b(), host, "{op} {a}");
                }
                assert_eq!(
                    ev.nat(F32, "ToNat", &[32, x]),
                    u128::from(a as u32),
                    "{a} as u32"
                );
                assert_eq!(
                    ev.nat(F32, "ToNat", &[8, x]),
                    u128::from(a as u8),
                    "{a} as u8"
                );
                assert_eq!(
                    ev.nat(F32, "ToInt", &[32, x]),
                    u128::from((a as i32) as u32),
                    "{a} as i32"
                );
                assert_eq!(
                    ev.nat(F32, "ToInt", &[8, x]),
                    u128::from((a as i8) as u8),
                    "{a} as i8"
                );
            }
        });
    }

    #[test]
    fn f32_binary_and_fused_operations_are_the_hosts() {
        deep(|| {
            let ev = Eval::new();
            let xs = samples32();
            // The sign of a zero result is either, for the host's `min` and for
            // `minimumNumber`'s `nsz` form; the model puts -0 below +0.
            let zeros = |m: u128, h: f32| h == 0.0 && f32::from_bits(m as u32) == 0.0;
            for &a in &xs {
                for &b in xs.iter().step_by(4) {
                    let (x, y) = (bits(a), bits(b));
                    assert_eq!(ev.nat(F32, "CopySign", &[x, y]), bits(a.copysign(b)));
                    let (lo, hi) = (
                        ev.nat(F32, "MinNum", &[x, y]),
                        ev.nat(F32, "MaxNum", &[x, y]),
                    );
                    assert!(same32(lo, a.min(b)) || zeros(lo, a.min(b)), "min {a} {b}");
                    assert!(same32(hi, a.max(b)) || zeros(hi, a.max(b)), "max {a} {b}");
                    for &c in xs.iter().step_by(13) {
                        let got = ev.nat(F32, "Fma", &[x, y, bits(c)]);
                        assert!(
                            same32(got, a.mul_add(b, c)),
                            "fma {a:e} {b:e} {c:e}: {got:#x}"
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn conversions_are_the_hosts() {
        deep(|| {
            let ev = Eval::new();
            let (e32, m32, e64, m64) = (8, 23, 11, 52);
            for a in samples32() {
                let wide = ev.nat(F64, "Convert", &[e32, m32, bits(a)]);
                assert!(
                    same32(ev.nat(F32, "Convert", &[e64, m64, wide]), a),
                    "{a} through f64"
                );
                if !a.is_nan() {
                    assert_eq!(wide, u128::from(f64::from(a).to_bits()), "{a} as f64");
                }
            }
            let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
            for _ in 0..200 {
                let r = xorshift(&mut seed);
                let d = f64::from_bits(r);
                let got = ev.nat(F32, "Convert", &[e64, m64, u128::from(r)]);
                assert!(same32(got, d as f32), "{d:e} as f32: {got:#x}");
                let n = r >> (r % 64);
                assert_eq!(
                    ev.nat(F32, "OfNat", &[u128::from(n)]),
                    bits(n as f32),
                    "{n} as f32"
                );
                let i = n as i64;
                assert_eq!(
                    ev.nat(F32, "OfInt", &[64, u128::from(i as u64)]),
                    bits(i as f32),
                    "{i} as f32"
                );
                assert_eq!(
                    ev.nat(F32, "OfInt", &[64, u128::from(i.wrapping_neg() as u64)]),
                    bits(i.wrapping_neg() as f32),
                    "-{i} as f32"
                );
            }
        });
    }

    #[test]
    fn f64_arithmetic_is_the_hosts() {
        deep(|| {
            let ev = Eval::new();
            let mut seed = 0x1234_5678_9abc_def1_u64;
            let mut xs: Vec<f64> = vec![
                0.0,
                -0.0,
                1.0,
                0.1,
                5e-324,
                f64::MAX,
                f64::INFINITY,
                f64::NAN,
                2.0f64.powi(-1022),
            ];
            for _ in 0..20 {
                let r = xorshift(&mut seed);
                xs.push(f64::from_bits(r));
                xs.push(f64::from_bits(r >> 2));
            }
            for &a in &xs {
                for &b in &xs {
                    let (x, y) = (u128::from(a.to_bits()), u128::from(b.to_bits()));
                    for (op, host) in [
                        ("Add", a + b),
                        ("Sub", a - b),
                        ("Mul", a * b),
                        ("Div", a / b),
                        ("Rem", a % b),
                    ] {
                        let got = ev.nat(F64, op, &[x, y]);
                        let ok = if host.is_nan() {
                            f64::from_bits(got as u64).is_nan()
                        } else {
                            got == u128::from(host.to_bits())
                        };
                        assert!(
                            ok,
                            "{op} {a:e} {b:e}: model {got:#x}, host {:#x}",
                            host.to_bits()
                        );
                    }
                }
                let x = u128::from(a.to_bits());
                let got = ev.nat(F64, "Sqrt", &[x]);
                assert!(
                    f64::from_bits(got as u64).to_bits() == a.sqrt().to_bits() || a.sqrt().is_nan(),
                    "sqrt {a:e}"
                );
            }
        });
    }

    /// `f16` against the host's `f32` rounded to `f16`: the exact result of
    /// two `f16`s rounded to `f32` and then to `f16` is rounded once, since
    /// `24 >= 2 * 11 + 2`.
    #[test]
    fn f16_arithmetic_is_the_hosts_rounded_twice() {
        deep(|| {
            let ev = Eval::new();
            let to32 = |x: u128| f32::from_bits(ev.nat(F32, "Convert", &[5, 10, x]) as u32);
            let narrow = |h: f32| ev.nat(F16, "Convert", &[8, 23, bits(h)]);
            let nan = |b: u128| (b & 0x7fff) > 0x7c00;
            let mut xs: Vec<u128> = (0..1u128 << 16).step_by(1597).collect();
            xs.extend([
                0, 0x8000, 0x7c00, 0xfc00, 0x0001, 0x03ff, 0x0400, 0x7bff, 0x3c00,
            ]);
            for &x in &xs {
                let a = to32(x);
                let same = |got: u128, h: f32| {
                    let want = narrow(h);
                    if nan(want) {
                        nan(got)
                    } else {
                        got == want
                    }
                };
                assert!(same(ev.nat(F16, "Sqrt", &[x]), a.sqrt()), "sqrt {x:#x}");
                for &y in xs.iter().step_by(3) {
                    let b = to32(y);
                    for (op, h) in [
                        ("Add", a + b),
                        ("Sub", a - b),
                        ("Mul", a * b),
                        ("Div", a / b),
                    ] {
                        assert!(same(ev.nat(F16, op, &[x, y]), h), "{op} {x:#x} {y:#x}");
                    }
                }
            }
        });
    }

    #[test]
    fn literals_and_named_constants_round_to_nearest() {
        deep(|| {
            let ev = Eval::new();
            for text in [
                "0",
                "-0",
                "1",
                "0.1",
                "9.99999974E-5",
                "1.0E+10",
                "3.4028235E+38",
                "1.0E-45",
                "1.0E+39",
                "+Inf",
                "-Inf",
            ] {
                let host: f32 = text.replace("Inf", "inf").parse().unwrap();
                assert_eq!(ev.term(&literal(F32, text).unwrap()), bits(host), "{text}");
            }
            for text in [
                "1.0E+22",
                "1.0E-4",
                "2.2250738585072014E-308",
                "5.0E-324",
                "0.30000000000000004",
            ] {
                let host: f64 = text.parse().unwrap();
                let got = ev.term(&literal(F64, text).unwrap());
                assert_eq!(got, u128::from(host.to_bits()), "{text}");
            }
            assert!(f32::from_bits(ev.term(&literal(F32, "NaN").unwrap()) as u32).is_nan());
            for (name, host) in [
                ("NAN", f32::NAN),
                ("INFINITY", f32::INFINITY),
                ("NEG_INFINITY", f32::NEG_INFINITY),
                ("MAX", f32::MAX),
                ("MIN", f32::MIN),
                ("MIN_POSITIVE", f32::MIN_POSITIVE),
                ("EPSILON", f32::EPSILON),
            ] {
                assert_eq!(ev.term(&literal(F32, name).unwrap()), bits(host), "{name}");
            }
        });
    }

    /// Every operation the lowering names is a definition of the prelude.
    #[test]
    fn every_operation_the_tables_name_is_defined() {
        let defs = prelude();
        let named = INTRINSICS
            .iter()
            .map(|(_, op, _)| *op)
            .chain(METHODS.iter().map(|m| m.2).filter(|op| !op.is_empty()))
            .chain(NAMED.iter().map(|(_, op)| *op))
            .chain([
                "Convert",
                "OfNat",
                "ToNat",
                "OfInt",
                "ToInt",
                "OfDecimal",
                "Signed",
            ]);
        for op in named {
            assert!(defs.contains_key(&format!("float{op}")), "float{op}");
        }
    }
}
