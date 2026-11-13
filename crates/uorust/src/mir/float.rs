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
mod tests;
