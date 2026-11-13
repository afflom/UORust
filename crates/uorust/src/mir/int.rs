//! Integers in the MIR path: every width and signedness by one rule.
//!
//! An integer is `Nat` whatever its type, because LexLean converts no `Nat`
//! to a signed family (`checked_convert` and `to_int` take only the
//! fixed-width families, see `upstream.toml`), so a cast between an
//! unsigned and a signed type closes only where both are `Nat`. An unsigned
//! value is held as itself. A signed value is held as its two's-complement
//! bit pattern, so the bits of an `iN` are the bits of the `uN` that Rust
//! itself reinterprets it as.
//!
//! An integer type is (bits, signed), and everything else comes from that
//! pair: the bits come from the type's name (see `lower::ty`), or for a
//! pointer-sized type from the declared targets' pointer widths, and `MIN`,
//! `MAX` and `BITS` come from the bits and the signedness. Every
//! `IntToInt` cast between any two widths is one rule, wrap or
//! sign-extend, parameterized by (from bits, from signed, to bits, to
//! signed).

use crate::lift::Ty;

/// An integer type at the declared targets' pointer widths (narrowest
/// first, as [`super::lower::Cx`] holds them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Int {
    pub(crate) signed: bool,
    /// The width a value is held at: a pointer-sized signed value is the
    /// bit pattern at the widest declared width.
    pub(crate) held: u32,
    /// The width, where every declared target agrees on it.
    pub(crate) width: Option<u32>,
    /// The width the range every target agrees on is checked at: the
    /// narrowest.
    pub(crate) check: u32,
}

impl Int {
    /// The integer type of a MIR-path `Ty`.
    pub(crate) fn of(t: &Ty, widths: &[(u32, Vec<String>)]) -> Option<Self> {
        let narrow = widths.first().map_or(64, |(w, _)| *w);
        let wide = widths.last().map_or(narrow, |(w, _)| *w);
        let (bits, signed) = match t {
            Ty::Nat(b) => (*b, false),
            Ty::Signed(b) => (*b, true),
            _ => return None,
        };
        Some(match bits {
            Some(b) => Self {
                signed,
                held: b,
                width: Some(b),
                check: b,
            },
            None => Self {
                signed,
                held: wide,
                width: (narrow == wide).then_some(wide),
                check: narrow,
            },
        })
    }
}

/// `2^e` in decimal, for any `e`.
pub(crate) fn pow2(e: u32) -> String {
    if e < 128 {
        return (1u128 << e).to_string();
    }
    let mut digits: Vec<u8> = vec![1];
    for _ in 0..e {
        let mut carry = 0;
        for d in &mut digits {
            let v = *d * 2 + carry;
            *d = v % 10;
            carry = v / 10;
        }
        if carry > 0 {
            digits.push(carry);
        }
    }
    digits.iter().rev().map(|d| char::from(b'0' + d)).collect()
}

/// The bit pattern of the literal `n` of the type, when `n` is one of its
/// values on every declared target.
pub(crate) fn literal(n: i128, int: Int) -> Option<String> {
    let m = n.unsigned_abs();
    let c = int.check;
    if !int.signed {
        return (n >= 0 && (c >= 128 || m < (1u128 << c))).then(|| m.to_string());
    }
    let half = 1u128 << (c - 1);
    if (n < 0 && m > half) || (n >= 0 && m >= half) {
        return None;
    }
    Some(match (n < 0, int.held) {
        (false, _) => m.to_string(),
        // `2^held - m`, below `2^128`.
        (true, 128) => (u128::MAX - m + 1).to_string(),
        (true, h) => ((1u128 << h) - m).to_string(),
    })
}

/// `x < y` on values of the type, as a formula of the naturals' order: a
/// signed pattern at or above `2^(w-1)` is negative, below every pattern
/// under it, and two patterns of one sign order as naturals. A literal
/// side's sign is decided here, so no case is left for it.
pub(crate) fn less(x: &str, y: &str, int: Int) -> String {
    let lt = format!("({x} <? {y})");
    if !int.signed {
        return lt;
    }
    let half = 1u128 << (int.held - 1);
    let (xv, yv) = (x.parse::<u128>().ok(), y.parse::<u128>().ok());
    let h = pow2(int.held - 1);
    let (xn, yp) = (format!("({h} <=? {x})"), format!("({y} <? {h})"));
    match (xv.map(|n| n >= half), yv.map(|n| n >= half)) {
        (Some(a), Some(b)) if a == b => (xv < yv).to_string(),
        (Some(a), Some(_)) => a.to_string(),
        (_, Some(false)) => format!("({lt} || {xn})"),
        (_, Some(true)) => format!("({xn} && {lt})"),
        (Some(false), None) => format!("({yp} && {lt})"),
        (Some(true), None) => format!("({yp} || {lt})"),
        (None, None) => format!("((((not {xn}) && {yp}) && {lt}) || ({xn} && ({yp} || {lt})))"),
    }
}

/// `x as to`, for `x` of the type `from`: wrap, or sign-extend. The value,
/// and the condition under which it is the cast's result on every declared
/// target (`None` when it always is).
///
/// The result is held at the width the target type is held at. A signed
/// source widened has its sign bit copied into every new bit: a negative
/// value gains `2^to - 2^from`. Any other cast keeps the low `to` bits.
/// When the target's width differs between the declared targets, the
/// result is the same on all of them exactly when the value is in the
/// target's range at the narrowest width, and that is the condition.
pub(crate) fn cast(x: &str, from: Int, to: Int, same: bool) -> (String, Option<String>) {
    let (f, t) = (from.held, to.held);
    if same {
        return (x.to_string(), None);
    }
    // `2^a - 2^b`, for `b < a <= 128`.
    let gap = |a: u32, b: u32| (u128::MAX >> (128 - a)) - ((1u128 << b) - 1);
    let value = if from.signed && t > f {
        format!(
            "(if ({x} <? {}) then {x} else ({x} + {}))",
            pow2(f - 1),
            gap(t, f)
        )
    } else if t >= f {
        x.to_string()
    } else {
        format!("(prim remainder {x} {} 0 : Nat)", pow2(t))
    };
    if to.width.is_some() {
        return (value, None);
    }
    // A pointer-sized target whose width differs between targets.
    let n = to.check;
    let holds = match (from.signed, to.signed) {
        // `[0, 2^n)`; a signed value's nonnegative half is below `2^(f-1)`.
        (false, false) => (f > n).then(|| format!("({x} <? {})", pow2(n))),
        (true, false) => Some(format!("({x} <? {})", pow2((f - 1).min(n)))),
        // `[0, 2^(n-1))`.
        (false, true) => (f >= n).then(|| format!("({x} <? {})", pow2(n - 1))),
        // `[-2^(n-1), 2^(n-1))`: the low half, or the top of the pattern.
        (true, true) => {
            (f > n).then(|| format!("(({x} <? {}) || ({} <=? {x}))", pow2(n - 1), gap(f, n - 1)))
        }
    };
    (value, holds)
}

/// An associated constant of a primitive integer's, as [`assoc`] finds it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Assoc {
    /// The value held, and its type.
    Held(String, Ty),
    /// `BITS`, `MAX` or `MIN` of a pointer-sized type, whose value differs
    /// between the declared targets.
    Varies,
}

/// An associated constant of a primitive integer, `BITS`, `MAX` or `MIN`,
/// from the type's (bits, signed); `None` for any other name.
pub(crate) fn assoc(name: &str, t: &Ty, int: Int) -> Option<Assoc> {
    let Some(w) = int.width else {
        return matches!(name, "BITS" | "MAX" | "MIN").then_some(Assoc::Varies);
    };
    // `2^k - 1`, for `1 <= k <= 128`.
    let ones = |k: u32| (u128::MAX >> (128 - k)).to_string();
    let (v, t) = match (name, int.signed) {
        ("BITS", _) => (w.to_string(), Ty::Nat(Some(32))),
        ("MAX", false) => (ones(w), t.clone()),
        ("MAX", true) => (ones(w - 1), t.clone()),
        ("MIN", false) => ("0".into(), t.clone()),
        ("MIN", true) => (pow2(w - 1), t.clone()),
        _ => return None,
    };
    Some(Assoc::Held(v, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one() -> Vec<(u32, Vec<String>)> {
        vec![(64, vec!["x86_64-unknown-linux-gnu".into()])]
    }

    fn many() -> Vec<(u32, Vec<String>)> {
        vec![(16, Vec::new()), (32, Vec::new()), (64, Vec::new())]
    }

    /// Every primitive integer's `Ty`.
    fn every() -> Vec<Ty> {
        let mut out = Vec::new();
        for b in [Some(8), Some(16), Some(32), Some(64), Some(128), None] {
            out.push(Ty::Nat(b));
            out.push(Ty::Signed(b));
        }
        out
    }

    fn get(name: &str, t: &Ty, w: &[(u32, Vec<String>)]) -> String {
        let int = Int::of(t, w).unwrap();
        match assoc(name, t, int) {
            Some(Assoc::Held(v, _)) => v,
            other => panic!("{name}: {other:?}"),
        }
    }

    /// The bounds the rule derives are Rust's own.
    #[test]
    fn bounds_are_rusts() {
        let w = one();
        let max = |t: Ty| get("MAX", &t, &w);
        let min = |t: Ty| get("MIN", &t, &w);
        assert_eq!(max(Ty::Nat(Some(8))), u8::MAX.to_string());
        assert_eq!(max(Ty::Nat(Some(16))), u16::MAX.to_string());
        assert_eq!(max(Ty::Nat(Some(32))), u32::MAX.to_string());
        assert_eq!(max(Ty::Nat(Some(64))), u64::MAX.to_string());
        assert_eq!(max(Ty::Nat(Some(128))), u128::MAX.to_string());
        assert_eq!(max(Ty::Nat(None)), u64::MAX.to_string());
        assert_eq!(max(Ty::Signed(Some(8))), i8::MAX.to_string());
        assert_eq!(max(Ty::Signed(Some(16))), i16::MAX.to_string());
        assert_eq!(max(Ty::Signed(Some(32))), i32::MAX.to_string());
        assert_eq!(max(Ty::Signed(Some(64))), i64::MAX.to_string());
        assert_eq!(max(Ty::Signed(Some(128))), i128::MAX.to_string());
        assert_eq!(max(Ty::Signed(None)), i64::MAX.to_string());
        // MIN is held as its bit pattern: the unsigned reinterpretation.
        assert_eq!(min(Ty::Signed(Some(8))), (i8::MIN as u8).to_string());
        assert_eq!(min(Ty::Signed(Some(16))), (i16::MIN as u16).to_string());
        assert_eq!(min(Ty::Signed(Some(32))), (i32::MIN as u32).to_string());
        assert_eq!(min(Ty::Signed(Some(64))), (i64::MIN as u64).to_string());
        assert_eq!(min(Ty::Signed(Some(128))), (i128::MIN as u128).to_string());
        assert_eq!(min(Ty::Nat(Some(128))), "0");
        assert_eq!(get("BITS", &Ty::Signed(None), &w), "64");
        assert_eq!(get("BITS", &Ty::Nat(Some(128)), &w), u128::BITS.to_string());
        // 2^128 is u128::MAX plus one, in decimal.
        let max = u128::MAX.to_string();
        let p = pow2(128);
        assert_eq!(p.len(), max.len());
        assert_eq!(p[..p.len() - 1], max[..max.len() - 1]);
        assert_eq!(p.as_bytes()[p.len() - 1], max.as_bytes()[max.len() - 1] + 1);
        let isize = Int::of(&Ty::Signed(None), &many()).unwrap();
        assert_eq!(assoc("MAX", &Ty::Signed(None), isize), Some(Assoc::Varies));
        assert!(assoc("count", &Ty::Signed(None), isize).is_none());
    }

    /// Evaluate a lowered cast's term on one value: the term is one of the
    /// three forms [`cast`] writes.
    fn eval(v: &str, x: u128) -> u128 {
        let num = |s: &str| s.parse::<u128>().unwrap();
        if let Some(rest) = v.strip_prefix("(if (x <? ") {
            let (half, rest) = rest.split_once(')').unwrap();
            let add = rest.rsplit_once("(x + ").unwrap().1.trim_end_matches(')');
            if x < num(half) {
                x
            } else {
                x.wrapping_add(num(add))
            }
        } else if let Some(rest) = v.strip_prefix("(prim remainder x ") {
            x % num(rest.split_once(' ').unwrap().0)
        } else {
            assert_eq!(v, "x");
            x
        }
    }

    /// Every cast between two widths agrees with Rust's own `as`, on the
    /// edges of every source type, on one 64-bit target.
    #[test]
    fn casts_are_rusts() {
        let w = one();
        // The bit pattern of the value `v` as the type `t`, by Rust's `as`.
        let pattern = |v: i128, t: &Ty| -> u128 {
            match t {
                Ty::Nat(Some(8)) | Ty::Signed(Some(8)) => v as u8 as u128,
                Ty::Nat(Some(16)) | Ty::Signed(Some(16)) => v as u16 as u128,
                Ty::Nat(Some(32)) | Ty::Signed(Some(32)) => v as u32 as u128,
                Ty::Nat(Some(64)) | Ty::Signed(Some(64)) => v as u64 as u128,
                Ty::Nat(None) | Ty::Signed(None) => v as u64 as u128,
                _ => v as u128,
            }
        };
        // The value a bit pattern of the type `t` stands for.
        let value = |p: u128, t: &Ty| -> i128 {
            match t {
                Ty::Signed(Some(8)) => p as u8 as i8 as i128,
                Ty::Signed(Some(16)) => p as u16 as i16 as i128,
                Ty::Signed(Some(32)) => p as u32 as i32 as i128,
                Ty::Signed(Some(64)) | Ty::Signed(None) => p as u64 as i64 as i128,
                _ => p as i128,
            }
        };
        let samples: [i128; 9] = [0, 1, 127, 128, 255, -1, -128, i64::MIN as i128, i128::MIN];
        for from in every() {
            for to in every() {
                let (fi, ti) = (Int::of(&from, &w).unwrap(), Int::of(&to, &w).unwrap());
                let (v, holds) = cast("x", fi, ti, from == to);
                assert_eq!(holds, None);
                for s in samples {
                    let x = pattern(s, &from);
                    let want = pattern(value(x, &from), &to);
                    assert_eq!(eval(&v, x), want, "{s} as {from:?} as {to:?}");
                }
            }
        }
    }

    /// Signed order on bit patterns is Rust's order on the values.
    #[test]
    fn order_is_rusts() {
        let int = Int::of(&Ty::Signed(Some(8)), &one()).unwrap();
        // -56 < 5.
        assert_eq!(less("200", "5", int), "true");
        assert_eq!(less("5", "200", int), "false");
        assert_eq!(less("x", "0", int), "((x <? 0) || (128 <=? x))");
        assert_eq!(
            less("x", "y", Int::of(&Ty::Nat(Some(8)), &one()).unwrap()),
            "(x <? y)"
        );
    }

    /// A pointer-sized target whose width differs states the range it
    /// agrees on.
    #[test]
    fn varying_targets_state_their_range() {
        let w = many();
        let i = |t: Ty| Int::of(&t, &w).unwrap();
        let holds = |a: Ty, b: Ty| cast("x", i(a), i(b), false).1;
        assert_eq!(holds(Ty::Nat(Some(8)), Ty::Nat(None)), None);
        assert_eq!(
            holds(Ty::Nat(Some(32)), Ty::Nat(None)),
            Some("(x <? 65536)".into())
        );
        assert_eq!(
            holds(Ty::Signed(Some(8)), Ty::Nat(None)),
            Some("(x <? 128)".into())
        );
        assert_eq!(holds(Ty::Signed(Some(16)), Ty::Signed(None)), None);
        assert_eq!(holds(Ty::Nat(None), Ty::Nat(Some(64))), None);
        assert_eq!(literal(-1, i(Ty::Signed(None))), Some(u64::MAX.to_string()));
        assert_eq!(literal(40000, i(Ty::Signed(None))), None);
        assert_eq!(
            literal(-1, i(Ty::Signed(Some(128)))),
            Some(u128::MAX.to_string())
        );
        assert_eq!(literal(256, i(Ty::Nat(Some(8)))), None);
    }
}
