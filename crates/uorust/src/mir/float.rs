//! IEEE 754 binary floating point, one model for every format.
//!
//! A format is the pair (exponent width, fraction width), and core's own
//! constants give it: `BITS`, `MANTISSA_DIGITS` and `MAX_EXP` of
//! `library/core/src/num/f*.rs`. A value is held as its bit pattern, a `Nat`
//! below `2^BITS`. So `to_bits`, `from_bits`, classification, comparison
//! and the sign operations are exact by construction.
//!
//! Each operation is one LexLean definition per format. One generator
//! writes them from the format's parameters, and no format has a model
//! written by hand. Arithmetic is correctly rounded as IEEE 754 §4.3.1
//! defines it, to nearest with ties to even. The exact result is formed in
//! `Nat`, with a sticky bit where it is not finite (division), and one
//! rounder packs it. Where an input is NaN, the result is that NaN, quieted
//! (the first operand's, then the second's). Where an operation makes a NaN
//! of its own, the result is the positive quiet NaN core's `NAN` is. Both
//! are NaNs that Rust's semantics allow (`f32` docs, "NaN bit patterns").
//!
//! The definitions are kept as a small term language, [`M`]. It prints as
//! LexLean, and the tests evaluate it exactly against the host's own `f32`
//! and `f64`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::lex::{Decl, Expr};

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
        (exp >= 2 && frac >= 1 && i64::from(exp) < 63 && max_exp == 1i64 << (exp - 1))
            .then_some(Format { exp, frac })
    }

    pub(crate) fn width(self) -> u32 {
        1 + self.exp + self.frac
    }

    /// The prefix of this format's definitions.
    fn prefix(self) -> String {
        format!("mirFloatE{}M{}", self.exp, self.frac)
    }

    /// The name of one of this format's definitions.
    pub(crate) fn def(self, op: &str) -> String {
        format!("{}{op}", self.prefix())
    }

    fn sign(self) -> Big {
        Big::pow2(self.width() - 1)
    }
    fn unit(self) -> Big {
        Big::pow2(self.frac)
    }
    fn emax(self) -> Big {
        Big::pow2(self.exp).sub(&Big::from(1))
    }
    fn bias(self) -> Big {
        Big::pow2(self.exp - 1).sub(&Big::from(1))
    }
    /// `bias + frac`: a value is `significand * 2^(e - bm)`, `e` the
    /// biased exponent (1 for a subnormal).
    fn bm(self) -> Big {
        self.bias().add(&Big::from(u128::from(self.frac)))
    }
    fn infinity(self) -> Big {
        self.emax().mul(&self.unit())
    }
    /// The quiet bit, the fraction's top bit (IEEE 754 §6.2.1).
    fn quiet(self) -> Big {
        Big::pow2(self.frac - 1)
    }
    /// Core's `NAN`: the positive quiet NaN with no other payload bit.
    pub(crate) fn nan(self) -> Big {
        self.infinity().add(&self.quiet())
    }
    /// The extra bits a quotient carries so its sticky bit falls below the
    /// rounding bit: two guard bits past the significand.
    fn quotient_shift(self) -> u32 {
        2 * self.frac + 3
    }
}

/// The formats of the float types core defines: every `num/f<N>.rs` of
/// `library/core/src` whose `impl` block states `BITS`, `MANTISSA_DIGITS`
/// and `MAX_EXP` as literals.
pub(crate) fn formats(library: &Path) -> BTreeMap<String, Format> {
    let dir = library.join("core").join("src").join("num");
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let float = name
            .strip_prefix('f')
            .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
            && path.extension().is_some_and(|e| e == "rs");
        if !float {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(f) = format_of(&text) {
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

/// The widest integer the lift holds, whose casts the rounder and the
/// conversions must cover: rustc's widest primitive integer.
const WIDEST_INT: u32 = u128::BITS;

// ---------------------------------------------------------------------------
// Natural numbers of any size, for the literals and for the evaluator.

/// A natural number, little-endian base `2^32`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) struct Big(Vec<u32>);

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
    pub(crate) fn pow2(k: u32) -> Big {
        let mut v = vec![0u32; (k / 32) as usize + 1];
        v[(k / 32) as usize] = 1 << (k % 32);
        Big(v).trim()
    }
    pub(crate) fn is_zero(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn to_u128(&self) -> Option<u128> {
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
    pub(crate) fn bits(&self) -> u32 {
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
    pub(crate) fn add(&self, o: &Big) -> Big {
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
    pub(crate) fn sub(&self, o: &Big) -> Big {
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
    pub(crate) fn mul(&self, o: &Big) -> Big {
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
    pub(crate) fn divmod(&self, d: &Big) -> (Big, Big) {
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
    pub(crate) fn decimal(&self) -> String {
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
// The term language the definitions are written in.

/// A term over `Nat` and `Bool`.
#[derive(Clone, Debug)]
pub(crate) enum M {
    N(Big),
    B(bool),
    V(String),
    Add(Box<M>, Box<M>),
    /// Truncated subtraction.
    Sub(Box<M>, Box<M>),
    Mul(Box<M>, Box<M>),
    Div(Box<M>, Box<M>),
    Mod(Box<M>, Box<M>),
    Lt(Box<M>, Box<M>),
    Le(Box<M>, Box<M>),
    Eq(Box<M>, Box<M>),
    And(Box<M>, Box<M>),
    Or(Box<M>, Box<M>),
    Not(Box<M>),
    If(Box<M>, Box<M>, Box<M>),
    /// `let name : sort := value; body`, the sort `Bool` when true.
    Let(String, bool, Box<M>, Box<M>),
    Call(String, Vec<M>),
}

fn n(v: u128) -> M {
    M::N(Big::from(v))
}
fn big(v: Big) -> M {
    M::N(v)
}
fn v(name: &str) -> M {
    M::V(name.to_string())
}
fn bx(m: M) -> Box<M> {
    Box::new(m)
}
fn add(a: M, b: M) -> M {
    M::Add(bx(a), bx(b))
}
fn sub(a: M, b: M) -> M {
    M::Sub(bx(a), bx(b))
}
fn mul(a: M, b: M) -> M {
    M::Mul(bx(a), bx(b))
}
fn div(a: M, b: M) -> M {
    M::Div(bx(a), bx(b))
}
fn rem(a: M, b: M) -> M {
    M::Mod(bx(a), bx(b))
}
fn lt(a: M, b: M) -> M {
    M::Lt(bx(a), bx(b))
}
fn le(a: M, b: M) -> M {
    M::Le(bx(a), bx(b))
}
fn eq(a: M, b: M) -> M {
    M::Eq(bx(a), bx(b))
}
fn and(a: M, b: M) -> M {
    M::And(bx(a), bx(b))
}
fn or(a: M, b: M) -> M {
    M::Or(bx(a), bx(b))
}
fn not(a: M) -> M {
    M::Not(bx(a))
}
fn ite(c: M, t: M, e: M) -> M {
    M::If(bx(c), bx(t), bx(e))
}
fn let_n(name: &str, value: M, body: M) -> M {
    M::Let(name.to_string(), false, bx(value), bx(body))
}
fn let_b(name: &str, value: M, body: M) -> M {
    M::Let(name.to_string(), true, bx(value), bx(body))
}
fn call(name: &str, args: Vec<M>) -> M {
    M::Call(name.to_string(), args)
}
/// Two booleans equal.
fn iff(a: M, b: M) -> M {
    ite(a, b.clone(), not(b))
}

impl M {
    fn flat(&self) -> String {
        let two = |op: &str, a: &M, b: &M| format!("({} {op} {})", a.flat(), b.flat());
        let prim = |p: &str, a: &M, b: &M, zero: bool| {
            format!(
                "(prim {p} {} {}{} : Nat)",
                a.flat(),
                b.flat(),
                if zero { " 0" } else { "" }
            )
        };
        match self {
            M::N(b) => b.decimal(),
            M::B(b) => b.to_string(),
            M::V(x) => x.clone(),
            M::Add(a, b) => two("+", a, b),
            M::Sub(a, b) => prim("subtract", a, b, false),
            M::Mul(a, b) => prim("multiply", a, b, false),
            M::Div(a, b) => prim("quotient", a, b, true),
            M::Mod(a, b) => prim("remainder", a, b, true),
            M::Lt(a, b) => two("<?", a, b),
            M::Le(a, b) => two("<=?", a, b),
            M::Eq(a, b) => two("=?", a, b),
            M::And(a, b) => two("&&", a, b),
            M::Or(a, b) => two("||", a, b),
            M::Not(a) => format!("(not {})", a.flat()),
            M::If(..) | M::Let(..) => self.expr().flat(),
            M::Call(f, args) => {
                let a: Vec<String> = args.iter().map(M::flat).collect();
                format!("({f} {})", a.join(" "))
            }
        }
    }

    /// The term as a LexLean expression, its `if`s and `let`s as forms the
    /// printer lays out.
    pub(crate) fn expr(&self) -> Expr {
        match self {
            M::If(c, t, e) => Expr::If {
                condition: Box::new(Expr::Flat(c.flat())),
                then: Box::new(t.expr()),
                otherwise: Box::new(e.expr()),
            },
            M::Let(x, boolean, value, body) => Expr::Let {
                name: x.clone(),
                ty: if *boolean { "Bool" } else { "Nat" }.into(),
                value: Box::new(value.expr()),
                body: Box::new(body.expr()),
            },
            _ => Expr::Flat(self.flat()),
        }
    }
}

/// One definition of the model.
#[derive(Clone, Debug)]
pub(crate) struct Def {
    pub(crate) name: String,
    /// Its parameters, each `true` where it is a `Bool`.
    pub(crate) params: Vec<(String, bool)>,
    pub(crate) boolean: bool,
    pub(crate) body: M,
}

impl Def {
    pub(crate) fn decl(&self) -> Decl {
        let sort = |b: bool| if b { "Bool" } else { "Nat" }.to_string();
        Decl::Definition {
            name: self.name.clone(),
            parameters: self
                .params
                .iter()
                .map(|(p, b)| (p.clone(), sort(*b)))
                .collect(),
            result: sort(self.boolean),
            body: self.body.expr(),
            axioms: Vec::new(),
            recursive: None,
            termination: None,
            mutual: None,
        }
    }
}

fn def(name: String, params: &[&str], boolean: bool, body: M) -> Def {
    Def {
        name,
        params: params.iter().map(|p| (p.to_string(), false)).collect(),
        boolean,
        body,
    }
}

// ---------------------------------------------------------------------------
// Format-independent helpers, bounded by a level count: `bits` of a number
// below `2^(2^levels)` and `2^k` for `k < 2^levels`, each unrolled into
// `levels` steps so no definition recurses.

/// The levels that cover numbers of `width` bits and shifts up to it.
fn levels_for(width: u32) -> u32 {
    32 - width.leading_zeros()
}

fn bits_def(levels: u32) -> Def {
    // From the top level down: where `x >= 2^(2^j)`, shift it by `2^j`.
    let mut body = add(v(&format!("l{levels}")), v(&format!("x{levels}")));
    for step in (0..levels).rev() {
        let j = levels - 1 - step;
        let threshold = big(Big::pow2(1 << j));
        let (x, l) = (format!("x{step}"), format!("l{step}"));
        let (x1, l1) = (format!("x{}", step + 1), format!("l{}", step + 1));
        let c = le(threshold.clone(), v(&x));
        body = let_n(
            &x1,
            ite(c.clone(), div(v(&x), threshold), v(&x)),
            let_n(&l1, add(v(&l), ite(c, n(1 << j), n(0))), body),
        );
    }
    let body = let_n("x0", v("x"), let_n("l0", n(0), body));
    def(format!("mirFloatBits{levels}"), &["x"], false, body)
}

fn pow_def(levels: u32) -> Def {
    let mut body = v(&format!("p{levels}"));
    for j in (0..levels).rev() {
        let set = eq(rem(div(v("k"), big(Big::pow2(j))), n(2)), n(1));
        body = let_n(
            &format!("p{}", j + 1),
            ite(
                set,
                mul(v(&format!("p{j}")), big(Big::pow2(1 << j))),
                v(&format!("p{j}")),
            ),
            body,
        );
    }
    def(
        format!("mirFloatPow{levels}"),
        &["k"],
        false,
        let_n("p0", n(1), body),
    )
}

/// `2^k mod m` for `k < 2^levels`, by squaring.
fn pow_mod_def(levels: u32) -> Def {
    let mut body = v(&format!("a{levels}"));
    for j in (0..levels).rev() {
        let set = eq(rem(div(v("k"), big(Big::pow2(j))), n(2)), n(1));
        let (a, b) = (format!("a{j}"), format!("b{j}"));
        body = let_n(
            &format!("a{}", j + 1),
            ite(set, rem(mul(v(&a), v(&b)), v("m")), v(&a)),
            let_n(&format!("b{}", j + 1), rem(mul(v(&b), v(&b)), v("m")), body),
        );
    }
    let body = let_n(
        "a0",
        rem(n(1), v("m")),
        let_n("b0", rem(n(2), v("m")), body),
    );
    def(format!("mirFloatPowMod{levels}"), &["k", "m"], false, body)
}

/// The integer square root of a number below `2^width`, a bit at a time
/// from the top.
fn isqrt_def(width: u32) -> Def {
    let top = width.div_ceil(2);
    let mut body = v(&format!("r{top}"));
    for step in (0..top).rev() {
        let bit = top - 1 - step;
        let (r, r1) = (format!("r{step}"), format!("r{}", step + 1));
        let c = add(v(&r), big(Big::pow2(bit)));
        body = let_n(
            &r1,
            ite(le(mul(c.clone(), c.clone()), v("x")), c, v(&r)),
            body,
        );
    }
    def(
        format!("mirFloatRoot{width}"),
        &["x"],
        false,
        let_n("r0", n(0), body),
    )
}

// ---------------------------------------------------------------------------
// The model of one format.

/// The parts of a model: every definition it holds, by name.
pub(crate) struct Ieee {
    pub(crate) format: Format,
    /// The widest significand of any format the scope holds, which a
    /// conversion into this one rounds.
    pub(crate) widest: u32,
}

/// What a float operation needs: its definitions, the dependencies first.
pub(crate) type Defs = Vec<Def>;

impl Ieee {
    fn f(&self) -> Format {
        self.format
    }
    fn name(&self, op: &str) -> String {
        self.f().def(op)
    }
    /// The widest number the rounder takes: a quotient, an integer, or a
    /// significand of the widest format, with the guard bits.
    fn levels(&self) -> u32 {
        let f = self.f();
        let quotient = f.frac + 1 + f.quotient_shift();
        // The fused sum: a product of `2 * (frac + 1)` bits shifted by up
        // to its guard, `3 * frac + 5`, and a carry.
        let fused = 2 * (f.frac + 1) + 3 * f.frac + 5 + 1;
        levels_for(quotient.max(fused).max(WIDEST_INT).max(self.widest) + 2)
    }
    fn bits(&self, x: M) -> M {
        call(&format!("mirFloatBits{}", self.levels()), vec![x])
    }
    fn pow(&self, k: M) -> M {
        call(&format!("mirFloatPow{}", self.levels()), vec![k])
    }
    fn exp_levels(&self) -> u32 {
        levels_for(self.f().exp + 1)
            .max(levels_for(1 << self.f().exp.min(31)))
            .max(1)
    }
    fn op(&self, op: &str, args: Vec<M>) -> M {
        call(&self.name(op), args)
    }

    // Decoding.
    fn negative(&self, x: M) -> M {
        le(big(self.f().sign()), x)
    }
    fn mag(&self, x: M) -> M {
        rem(x, big(self.f().sign()))
    }
    fn field(&self, x: M) -> M {
        div(self.mag(x), big(self.f().unit()))
    }
    fn fraction(&self, x: M) -> M {
        rem(x, big(self.f().unit()))
    }
    fn is_nan(&self, x: M) -> M {
        lt(big(self.f().infinity()), self.mag(x))
    }
    fn is_inf(&self, x: M) -> M {
        eq(self.mag(x), big(self.f().infinity()))
    }
    fn signed(&self, s: M, x: M) -> M {
        add(ite(s, big(self.f().sign()), n(0)), x)
    }

    /// Every definition of the model, the helpers first.
    pub(crate) fn all(&self) -> Defs {
        let f = self.f();
        let mut out: Defs = vec![
            bits_def(self.levels()),
            pow_def(self.levels()),
            pow_mod_def(self.exp_levels()),
        ];
        let x = || v("x");
        // The significand and the biased exponent (1 for a subnormal).
        out.push(def(
            self.name("Sig"),
            &["x"],
            false,
            let_n(
                "e",
                self.field(x()),
                ite(
                    eq(v("e"), n(0)),
                    self.fraction(x()),
                    add(self.fraction(x()), big(f.unit())),
                ),
            ),
        ));
        out.push(def(
            self.name("Exp"),
            &["x"],
            false,
            let_n("e", self.field(x()), ite(eq(v("e"), n(0)), n(1), v("e"))),
        ));
        out.push(def(self.name("IsNan"), &["x"], true, self.is_nan(x())));
        out.push(def(self.name("IsInfinite"), &["x"], true, self.is_inf(x())));
        out.push(def(
            self.name("Quiet"),
            &["x"],
            false,
            ite(
                eq(rem(div(x(), big(f.quiet())), n(2)), n(1)),
                x(),
                add(x(), big(f.quiet())),
            ),
        ));
        out.push(def(
            self.name("Neg"),
            &["x"],
            false,
            ite(
                self.negative(x()),
                sub(x(), big(f.sign())),
                add(x(), big(f.sign())),
            ),
        ));
        out.push(self.round());
        out.extend(self.arithmetic());
        out.extend(self.compare());
        out.extend(self.integral());
        out.extend(self.exact_ops());
        out
    }

    /// `round s n tp tn st`: the format's value nearest to
    /// `(-1)^s * (n + ε) * 2^(tp - tn - bm)`, ties to even, where `ε` is a
    /// positive amount below one unit of `n` exactly where `st` holds.
    /// A caller that sets `st` gives `n` two bits past the significand.
    fn round(&self) -> Def {
        let f = self.f();
        let p = f.frac + 1;
        let top = big(Big::pow2(p));
        let hidden = big(f.unit());
        let zero = self.signed(v("s"), n(0));
        let normal = let_n(
            "ef",
            sub(v("el"), v("tn")),
            ite(
                le(big(f.emax()), v("ef")),
                self.signed(v("s"), big(f.infinity())),
                self.signed(
                    v("s"),
                    add(mul(v("ef"), big(f.unit())), sub(v("q3"), hidden.clone())),
                ),
            ),
        );
        let packed = let_n(
            "p",
            self.pow(v("k")),
            let_n(
                "q",
                div(v("n"), v("p")),
                let_n(
                    "r",
                    rem(v("n"), v("p")),
                    let_n(
                        "h",
                        div(v("p"), n(2)),
                        let_b(
                            "up",
                            and(
                                le(n(1), v("k")),
                                or(
                                    lt(v("h"), v("r")),
                                    and(
                                        eq(v("r"), v("h")),
                                        or(v("st"), eq(rem(v("q"), n(2)), n(1))),
                                    ),
                                ),
                            ),
                            let_n(
                                "q2",
                                add(v("q"), ite(v("up"), n(1), n(0))),
                                let_b(
                                    "carry",
                                    eq(v("q2"), top),
                                    let_n(
                                        "q3",
                                        ite(v("carry"), hidden.clone(), v("q2")),
                                        let_n(
                                            "el",
                                            add(add(v("tp"), v("k")), ite(v("carry"), n(1), n(0))),
                                            ite(
                                                lt(v("q3"), hidden),
                                                self.signed(v("s"), v("q3")),
                                                normal,
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
        // A number of fewer than `p` bits is first widened to `p`, exactly;
        // then the shift that leaves `p` bits, or reaches the subnormal unit.
        let body = let_n(
            "l0",
            self.bits(v("n0")),
            let_n(
                "w",
                sub(n(u128::from(p)), v("l0")),
                let_n(
                    "n",
                    mul(v("n0"), self.pow(v("w"))),
                    let_n(
                        "tn",
                        add(v("tn0"), v("w")),
                        let_n(
                            "l",
                            add(v("l0"), v("w")),
                            let_n(
                                "k1",
                                sub(v("l"), n(u128::from(p))),
                                let_n(
                                    "k2",
                                    sub(add(n(1), v("tn")), v("tp")),
                                    let_n(
                                        "k",
                                        ite(le(v("k2"), v("k1")), v("k1"), v("k2")),
                                        // Shifted past every bit and the
                                        // rounding bit: zero.
                                        ite(le(add(v("l"), n(2)), v("k")), zero, packed),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
        Def {
            name: self.name("Round"),
            params: vec![
                ("s".into(), true),
                ("n0".into(), false),
                ("tp".into(), false),
                ("tn0".into(), false),
                ("st".into(), true),
            ],
            boolean: false,
            body,
        }
    }

    fn rnd(&self, s: M, x: M, tp: M, tn: M, st: M) -> M {
        self.op("Round", vec![s, x, tp, tn, st])
    }

    /// Bind each operand's sign, magnitude, significand and exponent.
    fn decoded(&self, body: M) -> M {
        let mut out = body;
        for x in ["b", "a"] {
            out = let_b(
                &format!("s{x}"),
                self.negative(v(x)),
                let_n(
                    &format!("m{x}"),
                    self.mag(v(x)),
                    let_n(
                        &format!("g{x}"),
                        self.op("Sig", vec![v(x)]),
                        let_n(&format!("e{x}"), self.op("Exp", vec![v(x)]), out),
                    ),
                ),
            );
        }
        out
    }

    /// NaN in, its quieted self out: the first operand's, then the second's.
    fn nan_first(&self, rest: M) -> M {
        ite(
            self.is_nan(v("a")),
            self.op("Quiet", vec![v("a")]),
            ite(self.is_nan(v("b")), self.op("Quiet", vec![v("b")]), rest),
        )
    }

    fn arithmetic(&self) -> Defs {
        let f = self.f();
        let nan = || big(f.nan());
        let inf = |s: M| self.signed(s, big(f.infinity()));
        let zero = |s: M| self.signed(s, n(0));
        let xor = || not(iff(v("sa"), v("sb")));
        // Two operands of at most `frac + 1` bits: the smaller is sticky
        // past `frac + 3` places, and two guard bits hold it.
        let add_core = self.sum_core("AddCore", f.frac + 3, 2, n(0));
        let add_body = self.nan_first(self.decoded(ite(
            self.is_inf(v("a")),
            ite(and(self.is_inf(v("b")), xor()), nan(), v("a")),
            ite(
                self.is_inf(v("b")),
                v("b"),
                ite(
                    and(eq(v("ma"), n(0)), eq(v("mb"), n(0))),
                    zero(and(v("sa"), v("sb"))),
                    ite(
                        eq(v("ma"), n(0)),
                        v("b"),
                        ite(
                            eq(v("mb"), n(0)),
                            v("a"),
                            ite(
                                le(v("eb"), v("ea")),
                                self.op(
                                    "AddCore",
                                    vec![v("sa"), v("ga"), v("ea"), v("sb"), v("gb"), v("eb")],
                                ),
                                self.op(
                                    "AddCore",
                                    vec![v("sb"), v("gb"), v("eb"), v("sa"), v("ga"), v("ea")],
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        )));
        let ab = &["a", "b"];
        let add_def = def(self.name("Add"), ab, false, add_body);
        // `a - b` is `a + -b`; a NaN `b` keeps its own sign.
        let sub_def = def(
            self.name("Sub"),
            ab,
            false,
            ite(
                self.is_nan(v("b")),
                self.op("Add", vec![v("a"), v("b")]),
                self.op("Add", vec![v("a"), self.op("Neg", vec![v("b")])]),
            ),
        );
        let bm = || big(f.bm());
        let mul_def = def(
            self.name("Mul"),
            ab,
            false,
            self.nan_first(self.decoded(ite(
                or(self.is_inf(v("a")), self.is_inf(v("b"))),
                ite(or(eq(v("ma"), n(0)), eq(v("mb"), n(0))), nan(), inf(xor())),
                ite(
                    or(eq(v("ma"), n(0)), eq(v("mb"), n(0))),
                    zero(xor()),
                    self.rnd(
                        xor(),
                        mul(v("ga"), v("gb")),
                        add(v("ea"), v("eb")),
                        bm(),
                        M::B(false),
                    ),
                ),
            ))),
        );
        let shift = f.quotient_shift();
        let div_def = def(
            self.name("Div"),
            ab,
            false,
            self.nan_first(self.decoded(ite(
                self.is_inf(v("a")),
                ite(self.is_inf(v("b")), nan(), inf(xor())),
                ite(
                    self.is_inf(v("b")),
                    zero(xor()),
                    ite(
                        eq(v("mb"), n(0)),
                        ite(eq(v("ma"), n(0)), nan(), inf(xor())),
                        ite(
                            eq(v("ma"), n(0)),
                            zero(xor()),
                            let_n(
                                "num",
                                mul(v("ga"), big(Big::pow2(shift))),
                                self.rnd(
                                    xor(),
                                    div(v("num"), v("gb")),
                                    add(v("ea"), bm()),
                                    add(v("eb"), n(u128::from(shift))),
                                    lt(n(0), rem(v("num"), v("gb"))),
                                ),
                            ),
                        ),
                    ),
                ),
            ))),
        );
        // `%`: `a - b * trunc(a / b)`, exact, with `a`'s sign.
        let pow_mod = format!("mirFloatPowMod{}", self.exp_levels());
        let rem_def = def(
            self.name("Rem"),
            ab,
            false,
            self.nan_first(self.decoded(ite(
                or(self.is_inf(v("a")), eq(v("mb"), n(0))),
                nan(),
                ite(
                    or(self.is_inf(v("b")), eq(v("ma"), n(0))),
                    v("a"),
                    ite(
                        le(v("eb"), v("ea")),
                        self.rnd(
                            v("sa"),
                            rem(
                                mul(
                                    v("ga"),
                                    call(&pow_mod, vec![sub(v("ea"), v("eb")), v("gb")]),
                                ),
                                v("gb"),
                            ),
                            v("eb"),
                            n(0),
                            M::B(false),
                        ),
                        let_n(
                            "d",
                            sub(v("eb"), v("ea")),
                            ite(
                                le(n(u128::from(f.frac) + 1), v("d")),
                                v("a"),
                                self.rnd(
                                    v("sa"),
                                    rem(v("ga"), mul(v("gb"), self.pow(v("d")))),
                                    v("ea"),
                                    n(0),
                                    M::B(false),
                                ),
                            ),
                        ),
                    ),
                ),
            ))),
        );
        vec![add_core, add_def, sub_def, mul_def, div_def, rem_def]
    }

    /// The sum of two finite nonzero values `g * 2^(e - tn - bm)`, the
    /// first's exponent the larger. It is exact where the exponents differ
    /// by at most `guard`. Past that, the smaller is a sticky bit below the
    /// larger shifted left by `shift`. `guard` must leave the smaller below
    /// half a unit of that, and `shift` must give the rounder two bits
    /// past the significand.
    fn sum_core(&self, name: &str, guard: u32, shift: u32, tn: M) -> Def {
        let exact = |s: &str, x: M| self.rnd(v(s), x, v("e2"), tn.clone(), M::B(false));
        let wide = || mul(v("g1"), big(Big::pow2(shift)));
        let sticky = |x: M| {
            self.rnd(
                v("s1"),
                x,
                v("e1"),
                add(tn.clone(), n(u128::from(shift))),
                M::B(true),
            )
        };
        let body = let_n(
            "d",
            sub(v("e1"), v("e2")),
            ite(
                le(v("d"), n(u128::from(guard))),
                let_n(
                    "x",
                    mul(v("g1"), self.pow(v("d"))),
                    ite(
                        iff(v("s1"), v("s2")),
                        exact("s1", add(v("x"), v("g2"))),
                        ite(
                            eq(v("x"), v("g2")),
                            n(0),
                            ite(
                                lt(v("g2"), v("x")),
                                exact("s1", sub(v("x"), v("g2"))),
                                exact("s2", sub(v("g2"), v("x"))),
                            ),
                        ),
                    ),
                ),
                ite(
                    iff(v("s1"), v("s2")),
                    // Strictly between `x` and `x + 1`, or `x - 1` and `x`.
                    sticky(wide()),
                    sticky(sub(wide(), n(1))),
                ),
            ),
        );
        Def {
            name: self.name(name),
            params: vec![
                ("s1".into(), true),
                ("g1".into(), false),
                ("e1".into(), false),
                ("s2".into(), true),
                ("g2".into(), false),
                ("e2".into(), false),
            ],
            boolean: false,
            body,
        }
    }

    /// The operations IEEE 754 defines exactly that core's intrinsics name:
    /// `abs`, `copySign`, `fusedMultiplyAdd`, `squareRoot`, `minimum`,
    /// `maximum`, `minimumNumber` and `maximumNumber` (§5.4.1, §5.5.1, §9.6).
    fn exact_ops(&self) -> Defs {
        let f = self.f();
        let sign = || big(f.sign());
        let mut out = Vec::new();
        out.push(def(self.name("Abs"), &["x"], false, self.mag(v("x"))));
        out.push(def(
            self.name("CopySign"),
            &["a", "b"],
            false,
            add(self.mag(v("a")), ite(self.negative(v("b")), sign(), n(0))),
        ));
        // `minimum` and `maximum`: NaN wins, and -0 is below +0.
        let key = |x: &str| self.op("Key", vec![v(x)]);
        let below = || {
            or(
                lt(key("a"), key("b")),
                and(eq(key("a"), key("b")), self.negative(v("a"))),
            )
        };
        let above = || {
            or(
                lt(key("b"), key("a")),
                and(eq(key("a"), key("b")), not(self.negative(v("a")))),
            )
        };
        for (op, pick) in [("Minimum", below()), ("Maximum", above())] {
            out.push(def(
                self.name(op),
                &["a", "b"],
                false,
                self.nan_first(ite(pick, v("a"), v("b"))),
            ));
        }
        // `minimumNumber` and `maximumNumber`: a number wins over NaN.
        for (op, pick) in [("MinNum", below()), ("MaxNum", above())] {
            out.push(def(
                self.name(op),
                &["a", "b"],
                false,
                ite(
                    self.is_nan(v("a")),
                    ite(self.is_nan(v("b")), self.op("Quiet", vec![v("a")]), v("b")),
                    ite(self.is_nan(v("b")), v("a"), ite(pick, v("a"), v("b"))),
                ),
            ));
        }
        out.extend(self.fma());
        out.extend(self.sqrt());
        out
    }

    /// `a * b + c`, rounded once.
    fn fma(&self) -> Defs {
        let f = self.f();
        let bm = || big(f.bm());
        // The product has up to `2 * (frac + 1)` bits and is shifted by
        // `frac + 3`, so its own width plus that is where it turns sticky.
        let core = self.sum_core("FmaCore", 3 * f.frac + 5, f.frac + 3, bm());
        let sp = || not(iff(v("sa"), v("sb")));
        let nan = || big(f.nan());
        let decoded = |body: M| {
            self.decoded(let_b(
                "sc",
                self.negative(v("c")),
                let_n(
                    "mc",
                    self.mag(v("c")),
                    let_n(
                        "gc",
                        self.op("Sig", vec![v("c")]),
                        let_n("ec", self.op("Exp", vec![v("c")]), body),
                    ),
                ),
            ))
        };
        let pzero = || or(eq(v("ma"), n(0)), eq(v("mb"), n(0)));
        let pinf = || or(self.is_inf(v("a")), self.is_inf(v("b")));
        let body = ite(
            self.is_nan(v("a")),
            self.op("Quiet", vec![v("a")]),
            ite(
                self.is_nan(v("b")),
                self.op("Quiet", vec![v("b")]),
                ite(
                    self.is_nan(v("c")),
                    self.op("Quiet", vec![v("c")]),
                    decoded(ite(
                        and(pinf(), pzero()),
                        nan(),
                        ite(
                            pinf(),
                            ite(
                                and(self.is_inf(v("c")), not(iff(sp(), v("sc")))),
                                nan(),
                                self.signed(sp(), big(f.infinity())),
                            ),
                            ite(
                                self.is_inf(v("c")),
                                v("c"),
                                ite(
                                    pzero(),
                                    ite(
                                        eq(v("mc"), n(0)),
                                        self.signed(and(sp(), v("sc")), n(0)),
                                        v("c"),
                                    ),
                                    let_n(
                                        "gp",
                                        mul(v("ga"), v("gb")),
                                        let_n(
                                            "ep",
                                            add(v("ea"), v("eb")),
                                            ite(
                                                eq(v("mc"), n(0)),
                                                self.rnd(sp(), v("gp"), v("ep"), bm(), M::B(false)),
                                                let_n(
                                                    "eq",
                                                    add(v("ec"), bm()),
                                                    ite(
                                                        le(v("eq"), v("ep")),
                                                        self.op(
                                                            "FmaCore",
                                                            vec![
                                                                sp(),
                                                                v("gp"),
                                                                v("ep"),
                                                                v("sc"),
                                                                v("gc"),
                                                                v("eq"),
                                                            ],
                                                        ),
                                                        self.op(
                                                            "FmaCore",
                                                            vec![
                                                                v("sc"),
                                                                v("gc"),
                                                                v("eq"),
                                                                sp(),
                                                                v("gp"),
                                                                v("ep"),
                                                            ],
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    )),
                ),
            ),
        );
        vec![core, def(self.name("Fma"), &["a", "b", "c"], false, body)]
    }

    /// The square root, rounded once: the integer root of the significand
    /// widened by `2 * guard` bits (and one where the exponent is odd),
    /// sticky where it is not exact.
    fn sqrt(&self) -> Defs {
        let f = self.f();
        let guard = f.frac + 3;
        let width = f.frac + 1 + 2 * guard + 1;
        let root = isqrt_def(width);
        let body = ite(
            self.is_nan(v("x")),
            self.op("Quiet", vec![v("x")]),
            ite(
                eq(self.mag(v("x")), n(0)),
                v("x"),
                ite(
                    self.negative(v("x")),
                    big(f.nan()),
                    ite(
                        self.is_inf(v("x")),
                        v("x"),
                        let_n(
                            "z",
                            add(self.op("Exp", vec![v("x")]), big(f.bm())),
                            let_n(
                                "odd",
                                rem(v("z"), n(2)),
                                let_n(
                                    "w",
                                    mul(
                                        self.op("Sig", vec![v("x")]),
                                        mul(big(Big::pow2(2 * guard)), add(n(1), v("odd"))),
                                    ),
                                    let_n(
                                        "q",
                                        call(&root.name, vec![v("w")]),
                                        self.rnd(
                                            M::B(false),
                                            v("q"),
                                            div(sub(v("z"), v("odd")), n(2)),
                                            n(u128::from(guard)),
                                            not(eq(mul(v("q"), v("q")), v("w"))),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
        vec![root, def(self.name("Sqrt"), &["x"], false, body)]
    }

    fn compare(&self) -> Defs {
        let f = self.f();
        let ab = &["a", "b"];
        let ordered = || and(not(self.is_nan(v("a"))), not(self.is_nan(v("b"))));
        // The order of the extended reals as a `Nat`, `-0` and `+0` one.
        let key = Def {
            name: self.name("Key"),
            params: vec![("x".into(), false)],
            boolean: false,
            body: ite(
                self.negative(v("x")),
                sub(big(f.sign()), self.mag(v("x"))),
                add(big(f.sign()), v("x")),
            ),
        };
        let k = |x: &str| self.op("Key", vec![v(x)]);
        let eq_def = def(
            self.name("Eq"),
            ab,
            true,
            and(ordered(), eq(k("a"), k("b"))),
        );
        let lt_def = def(
            self.name("Lt"),
            ab,
            true,
            and(ordered(), lt(k("a"), k("b"))),
        );
        let le_def = def(
            self.name("Le"),
            ab,
            true,
            and(ordered(), le(k("a"), k("b"))),
        );
        vec![key, eq_def, lt_def, le_def]
    }

    /// Rounding to an integral value, each mode the decision whether the
    /// magnitude's truncation steps up: `up(sign, rest, half, odd)`.
    fn integral(&self) -> Defs {
        let f = self.f();
        let modes: [(&str, M); 5] = [
            ("Trunc", M::B(false)),
            ("Floor", and(v("s"), lt(n(0), v("r")))),
            ("Ceil", and(not(v("s")), lt(n(0), v("r")))),
            ("RoundHalfAway", and(lt(n(0), v("r")), le(v("h"), v("r")))),
            (
                "RoundTiesEven",
                or(
                    lt(v("h"), v("r")),
                    and(
                        and(lt(n(0), v("r")), eq(v("r"), v("h"))),
                        eq(rem(v("q"), n(2)), n(1)),
                    ),
                ),
            ),
        ];
        let cap = u128::from(f.frac) + 2;
        modes
            .into_iter()
            .map(|(mode, up)| {
                let body = ite(
                    or(self.is_nan(v("x")), self.is_inf(v("x"))),
                    self.op("Quiet", vec![v("x")]),
                    let_b(
                        "s",
                        self.negative(v("x")),
                        let_n(
                            "e",
                            self.op("Exp", vec![v("x")]),
                            ite(
                                le(big(f.bm()), v("e")),
                                v("x"),
                                let_n(
                                    "d",
                                    sub(big(f.bm()), v("e")),
                                    let_n(
                                        "p",
                                        self.pow(ite(le(v("d"), n(cap)), v("d"), n(cap))),
                                        let_n(
                                            "g",
                                            self.op("Sig", vec![v("x")]),
                                            let_n(
                                                "q",
                                                div(v("g"), v("p")),
                                                let_n(
                                                    "r",
                                                    rem(v("g"), v("p")),
                                                    let_n(
                                                        "h",
                                                        div(v("p"), n(2)),
                                                        self.rnd(
                                                            v("s"),
                                                            add(v("q"), ite(up, n(1), n(0))),
                                                            big(f.bm()),
                                                            n(0),
                                                            M::B(false),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                );
                // A NaN or an infinity is its own integral value; NaN is
                // quieted, and an infinity has no quiet bit to set.
                let body = ite(self.is_inf(v("x")), v("x"), body);
                def(self.name(mode), &["x"], false, body)
            })
            .collect()
    }

    /// The conversion of an unsigned integer.
    pub(crate) fn from_nat(&self, x: M) -> M {
        self.rnd(M::B(false), x, big(self.f().bm()), n(0), M::B(false))
    }

    /// The conversion into an unsigned integer of `width` bits, as `as`
    /// does it: toward zero, saturating, NaN to 0.
    pub(crate) fn to_nat_def(&self, width: u32) -> Def {
        let f = self.f();
        let max = big(Big::pow2(width).sub(&Big::from(1)));
        let body = ite(
            or(self.is_nan(v("x")), self.negative(v("x"))),
            n(0),
            ite(
                self.is_inf(v("x")),
                max.clone(),
                let_n(
                    "e",
                    self.op("Exp", vec![v("x")]),
                    let_n(
                        "g",
                        self.op("Sig", vec![v("x")]),
                        ite(
                            le(big(f.bm()), v("e")),
                            let_n(
                                "d",
                                sub(v("e"), big(f.bm())),
                                ite(
                                    le(n(u128::from(width)), v("d")),
                                    max.clone(),
                                    let_n(
                                        "y",
                                        mul(v("g"), self.pow(v("d"))),
                                        ite(le(v("y"), max.clone()), v("y"), max),
                                    ),
                                ),
                            ),
                            let_n(
                                "d",
                                sub(big(f.bm()), v("e")),
                                ite(
                                    le(n(u128::from(f.frac) + 2), v("d")),
                                    n(0),
                                    div(v("g"), self.pow(v("d"))),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
        def(self.name(&format!("ToU{width}")), &["x"], false, body)
    }

    /// The conversion of a value of `from` into this format.
    pub(crate) fn from_float_def(&self, from: Format) -> Def {
        let to = self.f();
        let src = Ieee {
            format: from,
            widest: self.widest,
        };
        let s = || src.negative(v("x"));
        let body = ite(
            src.is_nan(v("x")),
            self.signed(s(), big(to.nan())),
            ite(
                src.is_inf(v("x")),
                self.signed(s(), big(to.infinity())),
                ite(
                    eq(src.mag(v("x")), n(0)),
                    self.signed(s(), n(0)),
                    let_n(
                        "e",
                        src.field(v("x")),
                        self.rnd(
                            s(),
                            ite(
                                eq(v("e"), n(0)),
                                src.fraction(v("x")),
                                add(src.fraction(v("x")), big(from.unit())),
                            ),
                            add(ite(eq(v("e"), n(0)), n(1), v("e")), big(to.bm())),
                            big(from.bm()),
                            M::B(false),
                        ),
                    ),
                ),
            ),
        );
        def(
            self.name(&format!("FromE{}M{}", from.exp, from.frac)),
            &["x"],
            false,
            body,
        )
    }
}

// ---------------------------------------------------------------------------
// What the lowering asks of the model.

/// A use of the model: the term, and the definitions it needs, the
/// dependencies first.
pub(crate) struct FloatUse {
    pub(crate) term: String,
    pub(crate) defs: Defs,
}

/// The widest significand among the scope's formats.
pub(crate) fn widest(floats: &BTreeMap<String, Format>) -> u32 {
    floats.values().map(|f| f.frac + 1).max().unwrap_or(0)
}

impl Ieee {
    pub(crate) fn of(format: Format, floats: &BTreeMap<String, Format>) -> Ieee {
        Ieee {
            format,
            widest: widest(floats),
        }
    }

    /// A call of one of the model's own operations on terms.
    pub(crate) fn apply(&self, op: &str, args: &[&str]) -> FloatUse {
        FloatUse {
            term: self
                .op(op, args.iter().map(|a| M::V(a.to_string())).collect())
                .flat(),
            defs: self.all(),
        }
    }

    /// `x as uN`, `N` the width.
    pub(crate) fn to_nat(&self, width: u32, x: &str) -> FloatUse {
        let d = self.to_nat_def(width);
        let term = call(&d.name, vec![M::V(x.to_string())]).flat();
        let mut defs = self.all();
        defs.push(d);
        FloatUse { term, defs }
    }

    /// An unsigned integer `x as` this format.
    pub(crate) fn of_nat(&self, x: &str) -> FloatUse {
        FloatUse {
            term: self.from_nat(M::V(x.to_string())).flat(),
            defs: self.all(),
        }
    }

    /// A value of `from` `as` this format.
    pub(crate) fn of_float(&self, from: Format, x: &str) -> FloatUse {
        if from == self.f() {
            return FloatUse {
                term: x.to_string(),
                defs: Vec::new(),
            };
        }
        let d = self.from_float_def(from);
        let term = call(&d.name, vec![M::V(x.to_string())]).flat();
        let mut defs = self.all();
        defs.push(d);
        FloatUse { term, defs }
    }

    /// The bit pattern of one of the constants IEEE 754 fixes for the
    /// format, by core's name for it.
    pub(crate) fn named(&self, name: &str) -> Option<Big> {
        let f = self.f();
        let max = f
            .emax()
            .sub(&Big::from(1))
            .mul(&f.unit())
            .add(&f.unit().sub(&Big::from(1)));
        Some(match name {
            "NAN" => f.nan(),
            "INFINITY" => f.infinity(),
            "NEG_INFINITY" => f.sign().add(&f.infinity()),
            "MAX" => max,
            "MIN" => f.sign().add(&max),
            "MIN_POSITIVE" => f.unit(),
            // `2^-frac`: the biased exponent `bias - frac`, no fraction.
            "EPSILON" => f.bias().sub(&Big::from(u128::from(f.frac))).mul(&f.unit()),
            _ => return None,
        })
    }
}

/// What a method or operator of a float type is, in the model.
pub(crate) enum Method {
    /// One of the model's operations, its result the format's value.
    Op(&'static str),
    /// A test of the bit pattern, its result a `bool`.
    Test(&'static str),
    /// The bit pattern itself, as the unsigned integer of the width
    /// (`to_bits`) or back (`from_bits`).
    Bits,
    /// An operator that assigns through `&mut`: the operation whose value
    /// the first operand takes.
    Assign(&'static str),
}

/// The meaning of a float type's method or operator impl, by its name and
/// the trait it implements (`None` for an inherent method).
pub(crate) fn method(trait_name: Option<&str>, name: &str) -> Option<(Method, usize)> {
    use Method::*;
    Some(match (trait_name, name) {
        (Some("Add"), "add") => (Op("Add"), 2),
        (Some("Sub"), "sub") => (Op("Sub"), 2),
        (Some("Mul"), "mul") => (Op("Mul"), 2),
        (Some("Div"), "div") => (Op("Div"), 2),
        (Some("Rem"), "rem") => (Op("Rem"), 2),
        (Some("Neg"), "neg") => (Op("Neg"), 1),
        (Some("AddAssign"), "add_assign") => (Assign("Add"), 2),
        (Some("SubAssign"), "sub_assign") => (Assign("Sub"), 2),
        (Some("MulAssign"), "mul_assign") => (Assign("Mul"), 2),
        (Some("DivAssign"), "div_assign") => (Assign("Div"), 2),
        (Some("RemAssign"), "rem_assign") => (Assign("Rem"), 2),
        (Some("PartialEq"), "eq") => (Test("Eq"), 2),
        (Some("PartialEq"), "ne") => (Test("Ne"), 2),
        (Some("PartialOrd"), "lt") => (Test("Lt"), 2),
        (Some("PartialOrd"), "le") => (Test("Le"), 2),
        (Some("PartialOrd"), "gt") => (Test("Gt"), 2),
        (Some("PartialOrd"), "ge") => (Test("Ge"), 2),
        (None, "is_nan") => (Test("IsNan"), 1),
        (None, "is_infinite") => (Test("IsInfinite"), 1),
        (None, "is_finite") => (Test("IsFinite"), 1),
        (None, "is_normal") => (Test("IsNormal"), 1),
        (None, "is_subnormal") => (Test("IsSubnormal"), 1),
        (None, "is_sign_negative") => (Test("IsSignNegative"), 1),
        (None, "is_sign_positive") => (Test("IsSignPositive"), 1),
        (None, "to_bits" | "from_bits") => (Bits, 1),
        (None, "abs") => (Op("Abs"), 1),
        (None, "copysign") => (Op("CopySign"), 2),
        (None, "min") => (Op("MinNum"), 2),
        (None, "max") => (Op("MaxNum"), 2),
        (None, "minimum") => (Op("Minimum"), 2),
        (None, "maximum") => (Op("Maximum"), 2),
        (None, "mul_add") => (Op("Fma"), 3),
        (None, "sqrt") => (Op("Sqrt"), 1),
        (None, "floor") => (Op("Floor"), 1),
        (None, "ceil") => (Op("Ceil"), 1),
        (None, "trunc") => (Op("Trunc"), 1),
        (None, "round") => (Op("RoundHalfAway"), 1),
        (None, "round_ties_even") => (Op("RoundTiesEven"), 1),
        _ => return None,
    })
}

impl Ieee {
    /// A test of one value or two, as a term.
    pub(crate) fn test(&self, op: &str, args: &[&str]) -> Option<FloatUse> {
        let f = self.f();
        let x = || M::V(args[0].to_string());
        let field = || self.field(x());
        let term = match op {
            "Eq" | "Lt" | "Le" | "IsNan" | "IsInfinite" => return Some(self.apply(op, args)),
            "Ne" => not(self.op("Eq", vec![x(), M::V(args.get(1)?.to_string())])),
            "Gt" => self.op("Lt", vec![M::V(args.get(1)?.to_string()), x()]),
            "Ge" => self.op("Le", vec![M::V(args.get(1)?.to_string()), x()]),
            "IsFinite" => lt(field(), big(f.emax())),
            "IsNormal" => and(lt(n(0), field()), lt(field(), big(f.emax()))),
            "IsSubnormal" => and(eq(field(), n(0)), lt(n(0), self.fraction(x()))),
            "IsSignNegative" => self.negative(x()),
            "IsSignPositive" => not(self.negative(x())),
            _ => return None,
        };
        Some(FloatUse {
            term: term.flat(),
            defs: self.all(),
        })
    }
}

/// The model's operation for one of core's float intrinsics, by its name
/// without the type (`floorf32` is `floor` at `f32`) and its arity.
pub(crate) fn intrinsic(base: &str) -> Option<(&'static str, usize)> {
    Some(match base {
        "fabs" => ("Abs", 1),
        "copysign" => ("CopySign", 2),
        "floor" => ("Floor", 1),
        "ceil" => ("Ceil", 1),
        "trunc" => ("Trunc", 1),
        "round" => ("RoundHalfAway", 1),
        "round_ties_even_" | "rint" => ("RoundTiesEven", 1),
        "sqrt" => ("Sqrt", 1),
        "minimum" => ("Minimum", 2),
        "maximum" => ("Maximum", 2),
        "minimum_number_nsz_" | "minnum" => ("MinNum", 2),
        "maximum_number_nsz_" | "maxnum" => ("MaxNum", 2),
        // `fmuladd` may fuse or not; the model fuses, as `fma` does.
        "fma" | "fmuladd" => ("Fma", 3),
        _ => return None,
    })
}

/// The name of a float intrinsic split into its operation and its type:
/// `floorf32` into `floor` and `f32`.
pub(crate) fn split_intrinsic<'a>(
    name: &'a str,
    floats: &BTreeMap<String, Format>,
) -> Option<(&'a str, Format)> {
    floats
        .iter()
        .find_map(|(t, f)| name.strip_suffix(t.as_str()).map(|b| (b, *f)))
        .filter(|(b, _)| !b.is_empty())
}

// ---------------------------------------------------------------------------
// Literals.

/// The bit pattern of a decimal literal as MIR prints it (`1.0E+16`,
/// `-0`, `9.99999974E-5`), rounded to nearest, ties to even.
pub(crate) fn literal(f: Format, text: &str, widest: u32) -> Option<Big> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let sign = if negative { f.sign() } else { Big::default() };
    match body {
        "Inf" => return Some(sign.add(&f.infinity())),
        "NaN" => return Some(sign.add(&f.nan())),
        _ => {}
    }
    let (mantissa, exp10) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.trim_start_matches('+').parse::<i64>().ok()?),
        None => (body, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.is_empty() || !(int.chars().chain(frac.chars())).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut digits = Big::default();
    for c in int.chars().chain(frac.chars()) {
        digits = digits
            .mul(&Big::from(10))
            .add(&Big::from(u128::from(c.to_digit(10)?)));
    }
    let exp10 = exp10 - i64::try_from(frac.len()).ok()?;
    let m = Ieee { format: f, widest };
    let pow10 = |k: u64| (0..k).fold(Big::from(1), |acc, _| acc.mul(&Big::from(10)));
    let bm = f.bm();
    // value = digits * 10^exp10; scaled to `num / den * 2^-shift`.
    let (num, den, shift) = if exp10 >= 0 {
        (digits.mul(&pow10(exp10.unsigned_abs())), Big::from(1), 0u32)
    } else {
        let den = pow10(exp10.unsigned_abs());
        let shift = den.bits() + f.frac + 3;
        (digits.shl(shift), den, shift)
    };
    let (q, r) = num.divmod(&den);
    let defs: BTreeMap<String, Def> = m.all().into_iter().map(|d| (d.name.clone(), d)).collect();
    let mut env = BTreeMap::new();
    env.insert("s".into(), Value::B(negative));
    env.insert("n0".into(), Value::N(q));
    env.insert("tp".into(), Value::N(bm));
    env.insert("tn0".into(), Value::N(Big::from(u128::from(shift))));
    env.insert("st".into(), Value::B(!r.is_zero()));
    match eval(&defs[&m.name("Round")].body, &env, &defs) {
        Value::N(b) => Some(b),
        Value::B(_) => None,
    }
}

// ---------------------------------------------------------------------------
// The evaluator: the model's own meaning, exactly, which the tests compare
// with the host's arithmetic and the literals are read by.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    N(Big),
    B(bool),
}

impl Value {
    fn n(self) -> Big {
        match self {
            Value::N(b) => b,
            Value::B(_) => panic!("a Bool where the model wants a Nat"),
        }
    }
    fn b(self) -> bool {
        match self {
            Value::B(b) => b,
            Value::N(_) => panic!("a Nat where the model wants a Bool"),
        }
    }
}

pub(crate) fn eval(m: &M, env: &BTreeMap<String, Value>, defs: &BTreeMap<String, Def>) -> Value {
    let e = |x: &M| eval(x, env, defs);
    let nn = |a: &M, b: &M| (e(a).n(), e(b).n());
    match m {
        M::N(b) => Value::N(b.clone()),
        M::B(b) => Value::B(*b),
        M::V(x) => env
            .get(x)
            .cloned()
            .unwrap_or_else(|| panic!("the model reads an unbound {x}")),
        M::Add(a, b) => {
            let (a, b) = nn(a, b);
            Value::N(a.add(&b))
        }
        M::Sub(a, b) => {
            let (a, b) = nn(a, b);
            Value::N(a.sub(&b))
        }
        M::Mul(a, b) => {
            let (a, b) = nn(a, b);
            Value::N(a.mul(&b))
        }
        M::Div(a, b) => {
            let (a, b) = nn(a, b);
            Value::N(a.divmod(&b).0)
        }
        M::Mod(a, b) => {
            let (a, b) = nn(a, b);
            Value::N(a.divmod(&b).1)
        }
        M::Lt(a, b) => {
            let (a, b) = nn(a, b);
            Value::B(a < b)
        }
        M::Le(a, b) => {
            let (a, b) = nn(a, b);
            Value::B(a <= b)
        }
        M::Eq(a, b) => {
            let (a, b) = nn(a, b);
            Value::B(a == b)
        }
        M::And(a, b) => Value::B(e(a).b() && e(b).b()),
        M::Or(a, b) => Value::B(e(a).b() || e(b).b()),
        M::Not(a) => Value::B(!e(a).b()),
        M::If(c, t, f) => {
            if e(c).b() {
                e(t)
            } else {
                e(f)
            }
        }
        M::Let(x, _, value, body) => {
            let mut inner = env.clone();
            inner.insert(x.clone(), e(value));
            eval(body, &inner, defs)
        }
        M::Call(f, args) => {
            let d = defs
                .get(f)
                .unwrap_or_else(|| panic!("the model calls an undefined {f}"));
            let mut inner = BTreeMap::new();
            for ((p, _), a) in d.params.iter().zip(args) {
                inner.insert(p.clone(), e(a));
            }
            eval(&d.body, &inner, defs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F32: Format = Format { exp: 8, frac: 23 };
    const F64: Format = Format { exp: 11, frac: 52 };

    struct Run {
        defs: BTreeMap<String, Def>,
        model: Ieee,
    }

    impl Run {
        fn new(format: Format) -> Run {
            let model = Ieee {
                format,
                widest: 113,
            };
            let mut all = model.all();
            all.push(model.to_nat_def(32));
            for from in [F32, F64, Format { exp: 5, frac: 10 }] {
                if from != format {
                    all.push(model.from_float_def(from));
                }
            }
            let defs = all.into_iter().map(|d| (d.name.clone(), d)).collect();
            Run { defs, model }
        }
        fn call(&self, op: &str, args: &[u128]) -> Value {
            let d = &self.defs[&self.model.name(op)];
            let mut env = BTreeMap::new();
            for ((p, _), a) in d.params.iter().zip(args) {
                env.insert(p.clone(), Value::N(Big::from(*a)));
            }
            eval(&d.body, &env, &self.defs)
        }
        fn nat(&self, op: &str, args: &[u128]) -> u128 {
            self.call(op, args).n().to_u128().unwrap()
        }
        fn bool(&self, op: &str, args: &[u128]) -> bool {
            self.call(op, args).b()
        }
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
            1.17549435e-38,
            1.1754942e-38,
            f32::MAX,
            f32::MIN,
            f32::EPSILON,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            -f32::NAN,
            f32::from_bits(0x7fa0_0001),
            16777216.0,
            16777217.0,
            1e10,
            123.456,
            0.1,
            1.0 + f32::EPSILON,
            8388607.5,
            4294967295.0,
            4294967296.0,
        ];
        let mut seed = 0x2545_f491_u64;
        for _ in 0..90 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            out.push(f32::from_bits(seed as u32));
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
        let r = Run::new(F32);
        let xs = samples32();
        for &a in &xs {
            for &b in xs.iter().step_by(3) {
                let (x, y) = (u128::from(a.to_bits()), u128::from(b.to_bits()));
                for (op, host) in [
                    ("Add", a + b),
                    ("Sub", a - b),
                    ("Mul", a * b),
                    ("Div", a / b),
                    ("Rem", a % b),
                ] {
                    let got = r.nat(op, &[x, y]);
                    assert!(
                        same32(got, host),
                        "{op} {a:e} {b:e}: model {got:#x}, host {:#x}",
                        host.to_bits()
                    );
                }
                assert_eq!(r.bool("Eq", &[x, y]), a == b, "{a} == {b}");
                assert_eq!(r.bool("Lt", &[x, y]), a < b, "{a} < {b}");
                assert_eq!(r.bool("Le", &[x, y]), a <= b, "{a} <= {b}");
            }
        }
    }

    #[test]
    fn f32_rounding_to_integers_is_the_hosts() {
        let r = Run::new(F32);
        for a in samples32() {
            let x = u128::from(a.to_bits());
            for (op, host) in [
                ("Trunc", a.trunc()),
                ("Floor", a.floor()),
                ("Ceil", a.ceil()),
                ("RoundHalfAway", a.round()),
                ("RoundTiesEven", a.round_ties_even()),
            ] {
                let got = r.nat(op, &[x]);
                assert!(same32(got, host), "{op} {a:e}: model {got:#x}");
            }
            let got = r.nat("Neg", &[x]);
            assert_eq!(got, u128::from((-a).to_bits()));
            assert_eq!(r.nat("ToU32", &[x]), u128::from(a as u32), "{a} as u32");
        }
    }

    #[test]
    fn conversions_are_the_hosts() {
        let r32 = Run::new(F32);
        let r64 = Run::new(F64);
        for a in samples32() {
            let x = u128::from(a.to_bits());
            let wide = r64.nat("FromE8M23", &[x]);
            assert!(same32(r32.nat("FromE11M52", &[wide]), a), "{a} through f64");
            if !a.is_nan() {
                assert_eq!(wide, u128::from(f64::from(a).to_bits()), "{a} as f64");
            }
        }
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        for _ in 0..300 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let d = f64::from_bits(seed);
            let got = r32.nat("FromE11M52", &[u128::from(seed)]);
            assert!(same32(got, d as f32), "{d:e} as f32: {got:#x}");
            let n = seed >> (seed % 64);
            let m = r32.model.from_nat(M::N(Big::from(u128::from(n))));
            let got = eval(&m, &BTreeMap::new(), &r32.defs).n().to_u128().unwrap();
            assert_eq!(got, u128::from((n as f32).to_bits()), "{n} as f32");
        }
    }

    #[test]
    fn f64_arithmetic_is_the_hosts() {
        let r = Run::new(F64);
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
        for _ in 0..40 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            xs.push(f64::from_bits(seed));
            xs.push(f64::from_bits(seed >> 2));
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
                    let got = r.nat(op, &[x, y]);
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
        }
    }

    #[test]
    fn exact_operations_are_the_hosts() {
        let r = Run::new(F32);
        let xs = samples32();
        for &a in &xs {
            let x = u128::from(a.to_bits());
            assert!(same32(r.nat("Sqrt", &[x]), a.sqrt()), "sqrt {a:e}");
            assert_eq!(r.nat("Abs", &[x]), u128::from(a.abs().to_bits()));
            for &b in xs.iter().step_by(4) {
                let y = u128::from(b.to_bits());
                assert_eq!(
                    r.nat("CopySign", &[x, y]),
                    u128::from(a.copysign(b).to_bits())
                );
                // The sign of a zero result is either, for the host and for
                // `minimumNumber`'s `nsz` form; the model puts -0 below +0.
                let zeros = |m: u128, h: f32| h == 0.0 && f32::from_bits(m as u32) == 0.0;
                let (lo, hi) = (r.nat("MinNum", &[x, y]), r.nat("MaxNum", &[x, y]));
                assert!(same32(lo, a.min(b)) || zeros(lo, a.min(b)), "min {a} {b}");
                assert!(same32(hi, a.max(b)) || zeros(hi, a.max(b)), "max {a} {b}");
                for &c in xs.iter().step_by(13) {
                    let z = u128::from(c.to_bits());
                    let got = r.nat("Fma", &[x, y, z]);
                    assert!(
                        same32(got, a.mul_add(b, c)),
                        "fma {a:e} {b:e} {c:e}: {got:#x}"
                    );
                }
            }
        }
    }

    /// `f16`'s operations against the host's `f32` rounded to `f16`: the
    /// exact sum, product, quotient and root of two `f16`s rounded to
    /// `f32` and then to `f16` is rounded once, since `24 >= 2 * 11 + 2`.
    #[test]
    fn f16_arithmetic_is_the_hosts_rounded_twice() {
        let f16 = Format { exp: 5, frac: 10 };
        let r = Run::new(f16);
        let narrow = |h: f32| r.nat("FromE8M23", &[u128::from(h.to_bits())]);
        let r32 = Run::new(F32);
        let to32 = |x: u128| f32::from_bits(r32.nat("FromE5M10", &[x]) as u32);
        let xs: Vec<u128> = (0..1u128 << 16)
            .step_by(797)
            .chain([
                0, 0x8000, 0x7c00, 0xfc00, 0x0001, 0x03ff, 0x0400, 0x7bff, 0x3c00,
            ])
            .collect();
        for &x in &xs {
            let a = to32(x);
            let same = |got: u128, h: f32| {
                let want = narrow(h);
                let nan = |b: u128| (b & 0x7fff) > 0x7c00;
                if nan(want) {
                    nan(got)
                } else {
                    got == want
                }
            };
            assert!(same(r.nat("Sqrt", &[x]), a.sqrt()), "sqrt {x:#x}");
            for &y in xs.iter().step_by(5) {
                let b = to32(y);
                for (op, h) in [
                    ("Add", a + b),
                    ("Sub", a - b),
                    ("Mul", a * b),
                    ("Div", a / b),
                ] {
                    assert!(same(r.nat(op, &[x, y]), h), "{op} {x:#x} {y:#x}");
                }
            }
        }
    }

    #[test]
    fn literals_round_to_nearest() {
        assert_eq!(
            literal(F32, "+Inf", 113).unwrap().to_u128(),
            Some(u128::from(f32::INFINITY.to_bits()))
        );
        assert_eq!(
            literal(F32, "-Inf", 113).unwrap().to_u128(),
            Some(u128::from(f32::NEG_INFINITY.to_bits()))
        );
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
        ] {
            let host: f32 = text.parse().unwrap();
            let got = literal(F32, text, 113).unwrap().to_u128().unwrap();
            assert_eq!(got, u128::from(host.to_bits()), "{text}");
        }
        for text in [
            "1.0E+22",
            "1.0E-4",
            "2.2250738585072014E-308",
            "5.0E-324",
            "0.30000000000000004",
        ] {
            let host: f64 = text.parse().unwrap();
            let got = literal(F64, text, 113).unwrap().to_u128().unwrap();
            assert_eq!(got, u128::from(host.to_bits()), "{text}");
        }
    }

    #[test]
    fn the_model_prints_as_lexlean() {
        let m = Ieee {
            format: F32,
            widest: 113,
        };
        let add = m
            .all()
            .into_iter()
            .find(|d| d.name == "mirFloatE8M23Add")
            .unwrap();
        let text = crate::lex::render_decl(&add.decl());
        assert!(text.contains("name: mirFloatE8M23Add,"), "{text}");
        assert!(text.contains("2139095040"), "{text}");
    }
}
