//! MIR to LexLean: one lowering per kind of [`super::spec`], each kind
//! either lowered or a typed residue that names it.
//!
//! The translation is functional, as Aeneas's is: a shared borrow is the
//! value it borrows (a `Box` too), locals are values threaded through the
//! control-flow graph, and a loop is a recursive definition of its own whose
//! parameters are the locals live at its header. Branches lower to `if` and
//! `match`; a join is translated once on each path into it, so every fact a
//! path establishes is in scope where it is used.
//!
//! Nothing is assumed:
//! - Every `assert` (bounds, overflow, division by zero), every call that
//!   does not return (a panic), and every `unreachable` becomes a theorem
//!   that it holds (or is unreachable) under the path's conditions, proved by
//!   `linear_arithmetic`. Overflow is checked at exactly the width the type
//!   has on every target the workspace declares (`usize` at the narrowest
//!   pointer width of `rust-toolchain.toml`'s targets), and a slice's length
//!   is bounded by `isize::MAX` at that width, the language's own bound for
//!   a type that is not zero-sized.
//! - A loop's measure is derived from the condition its header exits on
//!   (`b - a` for a loop that stays while `a < b`), and each recursive call
//!   carries a theorem that the measure decreases.
//! - The verifier decides each theorem. One that does not discharge makes
//!   the function residue, with the obligation as the cause (see `verdict`).

use std::collections::{BTreeMap, BTreeSet};

use super::float;
use super::int::Int;
use super::ir::{
    IrAggregate, IrBlockId, IrBody, IrConstant, IrLocal, IrOperand, IrPlace, IrProjection,
    IrRvalue, IrStatement, IrTerminator,
};
use super::parse::{split_path, split_top};
use super::spec;
use super::stdlib;
use crate::lex::{Arm, Decl, Expr};
use crate::lift::{self, Lift, Residue, Ty, Types};

/// A function the translator may call: its LexLean name and signature.
#[derive(Clone, Debug)]
pub(crate) struct Callee {
    pub(crate) name: String,
    pub(crate) params: Vec<Ty>,
    /// The forward result: the type of the body's `_0`.
    pub(crate) result: Ty,
    /// Where the definition threads `&mut` parameters: its result type and
    /// the positions of the parameters it gives back (see [`threaded`]).
    pub(crate) threads: Option<(Ty, Vec<usize>)>,
}

/// What every function of a crate is translated against.
pub(crate) struct Cx<'a> {
    pub(crate) types: &'a Types,
    /// `usize`'s widths on the declared targets, narrowest first.
    pub(crate) widths: &'a [(u32, Vec<String>)],
    /// Functions by the path MIR prints for them, with the crate prefixed.
    pub(crate) callees: &'a BTreeMap<String, Callee>,
    /// The crate's identifier, which MIR leaves off its own paths.
    pub(crate) krate: String,
    /// Names a parameter or helper may not take.
    pub(crate) taken: &'a BTreeSet<String>,
    /// Enums whose variants set their discriminants.
    pub(crate) explicit: &'a BTreeSet<String>,
    /// The size a definition's text may reach: the projection's file limit.
    pub(crate) limit: usize,
    /// The crate's constant bodies (promoted constants and constant
    /// items), by the path MIR prints for them.
    pub(crate) consts: &'a BTreeMap<String, IrBody>,
    /// The closures' bodies, by the closure's type as MIR prints it.
    pub(crate) closures: &'a BTreeMap<String, IrBody>,
    /// The std instances being translated, outermost first: one that would
    /// translate itself again is residue.
    pub(crate) active: &'a [String],
    /// In core's scope, core's own bodies, which its calls name.
    pub(crate) core: Option<&'a super::link::CoreLink>,
    /// The data layout of each target the translation answers for (see
    /// [`super::layout`]); none where the dumps record none.
    pub(crate) layouts: &'a [super::layout::DataLayout],
    /// Whether the body is lowered as its backward function: given the new
    /// value of the borrow it returns, the last value of the `&mut`
    /// parameter that borrow is into.
    pub(crate) backward: bool,
}

impl<'a> Cx<'a> {
    /// The same context, with `active` the instances being translated.
    fn within<'b>(&'b self, active: &'b [String]) -> Cx<'b>
    where
        'a: 'b,
    {
        Cx {
            types: self.types,
            widths: self.widths,
            callees: self.callees,
            krate: self.krate.clone(),
            taken: self.taken,
            explicit: self.explicit,
            limit: self.limit,
            consts: self.consts,
            closures: self.closures,
            active,
            core: self.core,
            layouts: self.layouts,
            backward: false,
        }
    }
}

/// A translated function: its declarations, in dependency order, the
/// functions it calls, the types it names and the auxiliary definitions it
/// needs.
#[derive(Clone, Debug)]
pub(crate) struct Lowered {
    pub(crate) decls: Vec<Decl>,
    pub(crate) calls: BTreeSet<String>,
    pub(crate) types: BTreeSet<Ty>,
    pub(crate) aux: BTreeMap<String, Decl>,
}

/// A value in the environment.
#[derive(Clone, Debug)]
enum Val {
    /// A term and its type.
    V(String, Ty),
    /// The pair an overflow-checked operator yields: the exact value, and
    /// whether it leaves the type's range.
    Checked(String, String, Ty),
    /// The discriminant of the value at a place.
    Disc(IrPlace),
    /// A mutable borrow of a place (a local or a part of one): a call that
    /// takes it returns the place's new value as well (its backward
    /// function, as in Aeneas), and a write through it writes the place.
    Mut(IrPlace),
    /// A closure's environment `_1`, the first parameter of the closure's
    /// definition: its name, its type, and the type of each capture.
    Env(String, Ty, Vec<Ty>),
    /// An `Option` of an item type that is `Some(value)` exactly where
    /// `guard` holds: a switch on its discriminant is an `if` on the guard,
    /// so a loop that runs while an iterator yields exits on an order of
    /// naturals, as a `while` loop does.
    Guarded {
        guard: String,
        value: String,
        item: Ty,
    },
}

/// What a call gives: its value, the new value of each mutably borrowed
/// argument (by position), and the guard of a guarded `Option`.
struct Called {
    term: String,
    updates: BTreeMap<usize, String>,
    guard: Option<String>,
    /// A callee that threads `&mut` parameters: `term` is its tuple, of
    /// this type, and the positions of the arguments it gives back, in the
    /// order the tuple holds them after `_0`'s value.
    threads: Option<(Ty, Vec<usize>)>,
    /// The callee takes the memory, and gives it back last in its tuple.
    memory: bool,
    /// A callee that returns a borrow into its `&mut` arguments: its
    /// backward function applied to the call's arguments, the positions of
    /// the arguments it gives back, and its tuple's type where it gives
    /// several.
    backward: Option<(String, Vec<usize>, Option<Ty>)>,
}

impl Called {
    fn value(term: String) -> Self {
        Self {
            term,
            updates: BTreeMap::new(),
            guard: None,
            threads: None,
            memory: false,
            backward: None,
        }
    }
}

/// A condition a branch establishes.
#[derive(Clone, Debug)]
enum MirStep {
    If(String, bool),
    /// The scrutinee equals this constructor term.
    Match(String, String),
}

/// The state along one path.
#[derive(Clone, Debug, Default)]
struct State {
    env: BTreeMap<IrLocal, Val>,
    path: Vec<MirStep>,
    /// Match binders in scope, with their types.
    binders: Vec<(String, Ty)>,
    /// The binders of each variant a place is known to hold: place text,
    /// variant -> binder names.
    bound: BTreeMap<(String, String), Vec<String>>,
    /// Facts the language guarantees about terms on this path.
    facts: BTreeSet<String>,
    /// The loop helper this path runs inside.
    inside: Option<IrBlockId>,
    /// The memory where control is, in a body that reaches it.
    memory: Option<String>,
    /// The locals whose values live in memory.
    placed: BTreeMap<IrLocal, Placed>,
    /// The mutable borrows a local's value holds (a struct built of a
    /// `&mut`, a closure that captures one): the part of the value that
    /// holds each, and the place it borrows, which takes that part's value
    /// each time the local is written, as a write through the borrow
    /// writes the place. A borrow a call returns lends through the callee's
    /// backward function, applied to the call's arguments: the place takes
    /// that function's value at the part's value.
    /// The function is applied to the part's value, and where it gives
    /// the last values of several arguments, its item of this argument's.
    #[allow(clippy::type_complexity)]
    lends: BTreeMap<IrLocal, Vec<(Vec<IrProjection>, IrPlace, Option<Transform>)>>,
    /// The place each local holding a shared borrow borrows.
    shared: BTreeMap<IrLocal, IrPlace>,
}

/// A backward function applied to a call's arguments, and, where it gives
/// the last values of several arguments, its tuple's type and the item of
/// the argument a lend writes.
type Transform = (String, Option<(Ty, usize)>);

/// A loop: its helper's name and parameters (the locals live at its header).
#[derive(Clone, Debug)]
struct Loop {
    name: String,
    live: Vec<(IrLocal, String, Ty)>,
    /// The locals whose values live in memory, by their pointers'
    /// parameters, and the memory's parameter, after the live locals.
    carried: Vec<(IrLocal, String)>,
    memory: Option<String>,
    /// The conditions every entry to the loop establishes and its body
    /// keeps, over the live locals' parameters: each a condition and the
    /// value it has. The helper takes them as given, and each call of it
    /// carries the theorem that they hold of its arguments.
    invariants: Vec<(String, bool)>,
    /// In a backward function, the parameter that takes the returned
    /// borrow's new value, last.
    new: Option<String>,
}

struct Fun<'a, 'c> {
    cx: &'a Cx<'c>,
    body: &'a IrBody,
    name: String,
    /// The main definition's parameters.
    params: Vec<(String, Ty)>,
    /// The type of the return place `_0`.
    ret: Ty,
    /// The definition's result: `_0`'s type, followed by the new value of
    /// each `&mut` parameter where it has any (see [`threaded`]).
    result: Ty,
    /// Whether the body returns `!`: it never returns, so a diverging call
    /// is its meaning, and the theorem that the call is unreachable is its
    /// callers' to prove.
    diverges: bool,
    loops: BTreeMap<IrBlockId, Option<Loop>>,
    regions: BTreeMap<IrBlockId, BTreeSet<IrBlockId>>,
    decls: Vec<Decl>,
    calls: BTreeSet<String>,
    types: BTreeSet<Ty>,
    aux: BTreeMap<String, Decl>,
    used: BTreeSet<String>,
    obligations: usize,
    size: usize,
    /// The parameters of the definition being built, and its evidence.
    scope: Vec<(String, Ty)>,
    evidence: Vec<Decl>,
    measure: Option<(String, Vec<IrLocal>)>,
    /// Terms of a fixed-width unsigned type, with its width on every
    /// declared target: a value of `u32` is at most `u32::MAX`.
    ranged: BTreeMap<String, u32>,
    /// Terms read out of a pattern-typed field, with what the pattern
    /// admits of them: `NonZero`'s value is not zero.
    admitted: BTreeMap<String, Vec<String>>,
    /// Each const parameter the body reads as a value, by the census's
    /// marker for it: the parameter of the definition that holds it.
    const_params: BTreeMap<String, (String, Ty)>,
    /// The `&mut` parameters: each holds the borrowed value, which writes
    /// through it replace, and the definition returns its last value.
    mut_params: BTreeSet<IrLocal>,
    /// Whether the definition takes and returns the memory (see
    /// [`super::memory`]): its last parameter, and its result's last item.
    memory: bool,
    /// The cell sizes the body's memory accesses use, whose laws the
    /// translation proves.
    strides: BTreeSet<u64>,
    /// Whether a place a shared borrow lends was copied into an allocation,
    /// and whether the body observes an address: together, an alias the
    /// copy does not keep could be observed.
    mirrored_shared: bool,
    observes_address: bool,
    /// Lowered as its backward function (see [`Cx::backward`]): the
    /// parameter that holds the returned borrow's new value, and its type.
    backward: Option<(String, Ty)>,
    /// The arguments the last callee takes by address (see
    /// [`address_params`]), by position.
    wants_address: Vec<usize>,
    /// The `&mut` parameters passed by address whose values are not
    /// given back (see [`unthreaded`]).
    unthreaded: BTreeSet<IrLocal>,
}

/// A local whose value lives in memory since its address was taken: the
/// pointer to its first cell, its cells' type and size, and their shape.
#[derive(Clone, Debug)]
struct Placed {
    pointer: String,
    elem: Ty,
    stride: u64,
    shape: Shape,
    /// The place is the caller's, lent by a borrow: a copy in an
    /// allocation of its own, which the caller does not see.
    mirror: bool,
    /// The place is the caller's, in the caller's memory, its address
    /// passed (see [`address_params`]): neither freed nor copied here.
    lent: bool,
}

/// How a value's cells are laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shape {
    /// One value.
    Scalar,
    /// A `MaybeUninit<T>`: one cell, initialized or not.
    Maybe,
    /// An array or slice: its items, one cell each.
    Items,
}

/// The parameters that are mutable borrows, in order, of a body with these
/// locals' types and this arity: a `&mut T` parameter is threaded as state,
/// as Aeneas's backward functions are. The definition takes the borrowed
/// `T` and returns, after `_0`'s value, the value each such parameter's
/// place holds when the body returns; a caller writes it back into the
/// place it lent. A value that holds a mutable borrow (a struct with a
/// `&mut` field, a closure that captures one) is threaded alike: the lift
/// holds the borrowed value inside it, and the caller writes it back into
/// the place the held borrow lent. A closure's environment `_1` is the
/// closure's own state, which its call gives back unchanged unless the
/// environment, by value or behind a `&mut`, holds such a borrow.
pub(crate) fn threaded(
    locals: &[String],
    arity: usize,
    closure: bool,
    types: &Types,
) -> Vec<usize> {
    // Whether a value of a type as printed holds a mutable borrow.
    let holds = |t: &str| {
        let mut stack = vec![t.trim().to_string()];
        let mut seen = BTreeSet::new();
        while let Some(t) = stack.pop() {
            // An option of a value that holds one holds it.
            if let Some(("Option", inner)) = wrapper(&t) {
                stack.push(inner.trim().to_string());
            }
            if mut_target(&t).is_some()
                || types
                    .aliases
                    .get(&t)
                    .is_some_and(|n| types.borrowing.contains_key(n))
            {
                return true;
            }
            if seen.insert(t.clone()) {
                stack.extend(types.closures.get(&t).into_iter().flatten().cloned());
            }
        }
        false
    };
    let mut out = Vec::new();
    let mut l = 1;
    while l <= arity {
        if let Some(t) = locals.get(l) {
            let threads = if closure && l == 1 {
                let env = mut_target(t).unwrap_or(t);
                types
                    .closures
                    .get(env.trim())
                    .is_some_and(|cs| cs.iter().any(|c| holds(c)))
            } else {
                holds(t)
            };
            if threads {
                out.push(l);
            }
        }
        l += 1;
    }
    out
}

/// `ret` followed by the threaded parameters' types, or `ret` alone.
pub(crate) fn threaded_result(ret: &Ty, rest: &[Ty], types: &Types) -> Ty {
    if rest.is_empty() {
        return ret.clone();
    }
    let mut ts = vec![ret.clone()];
    ts.extend(rest.iter().cloned());
    types.tuple(ts)
}

/// A type as printed with its lifetime arguments left out:
/// `CharSearcher<'_>` is `CharSearcher`, `Foo<'a, u8>` is `Foo<u8>`.
pub(crate) fn without_lifetimes(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let after_open = matches!(out.chars().last(), Some('<'));
        let after_comma = out.ends_with(", ");
        if c == '\'' && (after_open || after_comma) {
            while chars
                .peek()
                .is_some_and(|d| d.is_ascii_alphanumeric() || *d == '_')
            {
                chars.next();
            }
            if chars.peek() == Some(&',') {
                chars.next();
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
            } else if after_comma {
                out.truncate(out.len() - 2);
            }
            continue;
        }
        out.push(c);
    }
    out.replace("<>", "")
}

/// The census's markers of the const parameters `body` reads as values
/// (`const N`, bound to `#usize_N`), in order.
fn const_reads(body: &IrBody) -> BTreeSet<String> {
    // Every operand of the body, as its blocks print.
    let printed = format!("{:?}", body.blocks);
    let mut out = BTreeSet::new();
    let mut rest = printed.as_str();
    while let Some(at) = rest.find("Other(\"#") {
        let after = &rest[at + "Other(\"".len()..];
        let end = after.find('"').unwrap_or(after.len());
        out.insert(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

/// Whether a generic argument as MIR prints it is a const parameter's
/// value (`40`, `-1`, `true`), or `#` where the census binds a const
/// parameter, not a type.
fn is_const_argument(a: &str) -> bool {
    a.starts_with('#')
        || a == "true"
        || a == "false"
        || a.starts_with(|c: char| c.is_ascii_digit() || c == '-')
}

/// The type a `&mut T` (or `&'a mut T`) points to, as printed.
pub(crate) fn mut_target(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix('&')?;
    let rest = match rest.strip_prefix('\'') {
        Some(r) => r.split_once(' ').map_or(r, |(_, r)| r),
        None => rest,
    };
    rest.strip_prefix("mut ")
}

/// The residue a kind of MIR becomes where it is not lowered.
fn kind(k: &str, detail: &str) -> Residue {
    if detail.is_empty() {
        Residue::Mir(k.to_string())
    } else {
        Residue::Mir(format!("{k}: {detail}"))
    }
}

/// The bit patterns of a `bits`-bit integer (`signed` for a signed one)
/// that the pattern `pat` admits, as one wrapping range `start..=end`:
/// each alternative a range of literals, characters or the type's own
/// `MIN` and `MAX`, an open end at the type's bound.
fn valid_range(pat: &str, signed: bool, bits: u32) -> Option<(u128, u128)> {
    let mask = if bits >= 128 {
        u128::MAX
    } else {
        (1u128 << bits) - 1
    };
    let (min, max): (i128, Option<i128>) = if signed {
        let min = (-1i128).checked_shl(bits.checked_sub(1)?)?;
        (min, Some(!min))
    } else {
        (0, i128::try_from(mask).ok())
    };
    let value = |text: &str| -> Option<i128> {
        let t = text.replace('_', "");
        if let Some(c) = t.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
            return match c.strip_prefix("\\u{").and_then(|r| r.strip_suffix('}')) {
                Some(hex) => i128::from_str_radix(hex, 16).ok(),
                None => c.chars().next().map(|ch| i128::from(u32::from(ch))),
            };
        }
        if t.ends_with("::MAX") {
            return max;
        }
        if t.ends_with("::MIN") {
            return Some(min);
        }
        let (neg, digits) = t
            .strip_prefix('-')
            .map_or((false, t.as_str()), |d| (true, d));
        match digits.strip_prefix("0x") {
            Some(hex) => {
                let v = i128::from_str_radix(hex, 16).ok()?;
                if neg {
                    v.checked_neg()
                } else {
                    Some(v)
                }
            }
            // The sign read with the digits, so `i128::MIN` reads.
            None => t.parse::<i128>().ok(),
        }
    };
    let pat = pat
        .trim()
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(pat.trim());
    let mut spans: Vec<(u128, u128)> = Vec::new();
    for alt in pat.split('|').map(str::trim) {
        let (lo, hi) = match (alt.split_once("..="), alt.split_once("..")) {
            (Some((lo, hi)), _) => (
                if lo.is_empty() { Some(min) } else { value(lo) },
                if hi.is_empty() { max } else { value(hi) },
            ),
            (None, Some((lo, hi))) => (
                if lo.is_empty() { Some(min) } else { value(lo) },
                if hi.is_empty() {
                    max
                } else {
                    value(hi).and_then(|h| h.checked_sub(1))
                },
            ),
            (None, None) => (value(alt), value(alt)),
        };
        // An unsigned type whose largest value no `i128` holds ends open.
        let lo = lo?;
        let hi_pattern = hi.map_or(mask, |h| (h as u128) & mask);
        let pattern = |v: i128| (v as u128) & mask;
        // A span on one side of zero is one span of patterns; one across
        // it wraps, from the negatives' patterns up to all ones.
        if lo >= 0 || !signed || hi.is_some_and(|h| h < 0) {
            spans.push((pattern(lo), hi_pattern));
        } else {
            spans.push((0, hi_pattern));
            spans.push((pattern(lo), mask));
        }
    }
    spans.sort_unstable();
    let mut merged: Vec<(u128, u128)> = Vec::new();
    for (s, e) in spans {
        match merged.last_mut() {
            Some((_, last)) if s <= last.saturating_add(1) => *last = (*last).max(e),
            _ => merged.push((s, e)),
        }
    }
    match merged.as_slice() {
        [one] => Some(*one),
        [(0, first_end), (second_start, last)] if *last == mask => {
            Some((*second_start, *first_end))
        }
        _ => None,
    }
}

/// The pattern an `Option` of a value of the valid range `start..=end`
/// (patterns up to `mask`) holds `None` as: rustc's `Niche::reserve` of one
/// value, which moves the bound whose next value is nearer zero.
fn niche(start: u128, end: u128, mask: u128) -> Option<u128> {
    if end.wrapping_add(1) & mask == start {
        return None;
    }
    let after_end = end.wrapping_add(1) & mask;
    let before_start = start.wrapping_sub(1) & mask;
    Some(if start > end {
        after_end
    } else if start <= mask - end {
        if start >= 1 {
            before_start
        } else {
            after_end
        }
    } else if (1..=end).contains(&after_end) {
        before_start
    } else {
        after_end
    })
}

/// The largest value of an unsigned type of `bits` bits.
fn max_of(bits: u32) -> String {
    if bits >= 128 {
        u128::MAX.to_string()
    } else {
        ((1u128 << bits) - 1).to_string()
    }
}

/// The struct a raw pointer is, in a scope that declares it: an address,
/// its provenance and its metadata, as `core::ptr`'s documentation gives
/// them. A pointer is a typed address (its pointee is the type MIR prints
/// for it, and a cast between pointer types keeps the address and the
/// provenance); `allocation` names the allocation its provenance grants
/// access to, `0` for none (`ptr::without_provenance`, `ptr::null`);
/// `metadata` is a slice's or a `str`'s length, and `0` standing for the
/// unit a thin pointer's metadata is. Reads and writes through one reach
/// memory, which the value model does not hold.
pub(crate) const POINTER: &str = "RawPointer";

/// The fields of [`POINTER`]: LexLean name and type.
pub(crate) fn pointer_fields() -> Vec<(String, Ty)> {
    vec![
        ("allocation".to_string(), Ty::Nat(None)),
        ("address".to_string(), Ty::Nat(None)),
        ("metadata".to_string(), Ty::Nat(None)),
    ]
}

/// A [`POINTER`] value from its fields' terms. A pointer without provenance
/// (`allocation` `0`) at an address is what an integer transmuted to a
/// pointer is (`core::ptr`: "transmuting an integer to a pointer produces a
/// pointer without provenance").
fn pointer_value(allocation: &str, address: &str, metadata: &str) -> String {
    format!(
        "(record RawPointer {{allocation := {allocation}, address := {address}, metadata := {metadata}}})"
    )
}

/// The pointee of a raw pointer type as printed, and whether its metadata
/// is a length (a slice or a `str`); `None` for any other type, and for a
/// trait object, whose metadata is a vtable.
fn pointee(text: &str) -> Option<(&str, bool)> {
    let t = text.trim();
    let wrapped = wrapper(t)
        .filter(|(n, _)| matches!(*n, "NonNull" | "Unique"))
        .map(|(_, arg)| arg);
    let rest = t
        .strip_prefix("*const ")
        .or_else(|| t.strip_prefix("*mut "))
        .or(wrapped)?
        .trim();
    if rest.starts_with("dyn ") {
        return None;
    }
    let sized_len = (rest.starts_with('[') && !rest.contains(';')) || rest == "str";
    Some((rest, sized_len))
}

/// Whether a type is the lift's raw pointer.
fn is_pointer(t: &Ty) -> bool {
    matches!(t, Ty::Named(n) if n == POINTER)
}

/// The address of a pointer value.
fn address(v: &str) -> String {
    #[cfg(not(uorust_lift))]
    crate::certified::certified!(mir_lower_address, v);
    format!("(project {v} address)")
}

/// The residue of a read or write through a raw pointer.
fn through_pointer(what: &str) -> Residue {
    kind(
        spec::MirProjectionElem::Deref.path(),
        &format!("a {what} through a raw pointer, into memory the value model does not hold"),
    )
}

/// Translate one body to the definition `name`.
pub(crate) fn lower(cx: &Cx<'_>, body: &IrBody, name: &str) -> Lift<Lowered> {
    // A body reaches memory where it takes an address or reads through a
    // pointer, or where a callee does: then the memory is threaded through
    // it.
    let direct = reaches_memory(body);
    match lower_in(cx, body, name, direct) {
        Err(r) if !direct && is_needs_memory(&r) => lower_in(cx, body, name, true),
        Err(r) if is_needs_memory(&r) => Err(kind(
            MEMORY_KIND,
            "a memory the body's translation cannot thread",
        )),
        other => other,
    }
}

fn lower_in(cx: &Cx<'_>, body: &IrBody, name: &str, memory: bool) -> Lift<Lowered> {
    let mut f = Fun {
        cx,
        body,
        name: name.to_string(),
        params: Vec::new(),
        ret: Ty::Unit,
        result: Ty::Unit,
        diverges: body.locals.first().is_some_and(|t| t.trim() == "!"),
        loops: BTreeMap::new(),
        regions: BTreeMap::new(),
        decls: Vec::new(),
        calls: BTreeSet::new(),
        types: BTreeSet::new(),
        aux: BTreeMap::new(),
        used: cx.taken.clone(),
        obligations: 0,
        size: 0,
        scope: Vec::new(),
        evidence: Vec::new(),
        measure: None,
        ranged: BTreeMap::new(),
        admitted: BTreeMap::new(),
        const_params: BTreeMap::new(),
        mut_params: BTreeSet::new(),
        memory,
        strides: BTreeSet::new(),
        mirrored_shared: false,
        observes_address: false,
        backward: None,
        wants_address: Vec::new(),
        unthreaded: if memory && cx.core.is_some() {
            unthreaded(body)
        } else {
            BTreeSet::new()
        },
    };
    f.used.insert(name.to_string());
    let threads = f.threads();
    // A `&mut` parameter rebound to another borrow no longer reaches the
    // place its caller lent, whose last value the definition returns.
    for block in &body.blocks {
        let rebinds = |p: &IrPlace| p.projection.is_empty() && threads.contains(&p.local);
        let rebound = block
            .statements
            .iter()
            .any(|s| matches!(s, IrStatement::Assign(p, _) if rebinds(p)))
            || matches!(&block.terminator, IrTerminator::Call { destination, .. } if rebinds(destination));
        if rebound {
            return Err(kind(
                spec::MirStatementKind::Assign.path(),
                "a `&mut` parameter rebound to another borrow",
            ));
        }
    }
    f.mut_params.extend(threads.iter().copied());
    f.ret = f.local_ty(0, &[])?;
    let mut st = State::default();
    // A closure's definition takes its environment first (the struct of its
    // captures), then its arguments.
    let closure = super::closure_type(body);
    if let Some(ct) = &closure {
        let mut captures = Vec::new();
        for (k, up) in body.upvars.iter().enumerate() {
            let Some((_, t)) = up else {
                return Err(kind(
                    spec::MirAggregateKind::Closure.path(),
                    &format!("a capture {k} debug info does not name"),
                ));
            };
            captures.push(ty(t, cx.types)?);
        }
        let et = ty(ct, cx.types)?;
        let p = f.fresh(Some("env"), 1);
        f.params.push((p.clone(), et.clone()));
        st.env.insert(1, Val::Env(p, et, captures));
    }
    let closure = closure.is_some();
    // A const parameter the body reads as a value, `const N`, is a
    // parameter of the definition of the type its source gives it, ahead of
    // the body's own.
    for marker in const_reads(body) {
        let Some((Some(t), name)) = super::core::const_of_marker(&marker) else {
            return Err(kind(
                spec::MirOperand::Constant.path(),
                "a const parameter's value, of a type its source does not give",
            ));
        };
        let t = ty(t, cx.types)?;
        let p = f.fresh(Some(&name.to_lowercase()), usize::MAX);
        f.params.push((p.clone(), t.clone()));
        f.const_params.insert(marker, (p, t));
    }
    // A borrow a raw pointer is taken of is passed as the address of the
    // place it lends, in the caller's memory.
    let addressed = if memory && cx.core.is_some() {
        address_params(body)
    } else {
        BTreeSet::new()
    };
    let mut lent = Vec::new();
    // A `&mut` parameter is read as the value it borrows.
    for l in 1..=body.arity {
        if closure && l == 1 {
            continue;
        }
        let p = f.fresh(body.names.get(l).cloned().flatten().as_deref(), l);
        if addressed.contains(&l) {
            let pt = Ty::Named(POINTER.to_string());
            f.mention(&pt);
            lent.push((l, p.clone()));
            f.params.push((p, pt));
            continue;
        }
        let ty = f.local_ty(l, &[])?;
        st.env.insert(l, Val::V(p.clone(), ty.clone()));
        f.params.push((p, ty));
    }
    for (l, p) in lent {
        let referent_text = referent(&body.locals[l])
            .ok_or_else(|| kind(MEMORY_KIND, "an address parameter that is not a borrow"))?;
        let (elem, stride, _, shape) = f.cells_of(&referent_text)?;
        st.placed.insert(
            l,
            Placed {
                pointer: p,
                elem,
                stride,
                shape,
                mirror: true,
                lent: true,
            },
        );
    }
    let mut threads_ty = Vec::new();
    for &l in &threads {
        threads_ty.push(f.local_ty(l, &[])?);
    }
    // The memory, last: taken, and given back after the `&mut` parameters.
    if memory {
        if !cx.types.structs.contains_key(super::memory::MEMORY) {
            return Err(kind(MEMORY_KIND, "memory, in a scope that declares none"));
        }
        let m = f.fresh(Some("memory"), 0);
        f.params.push((m.clone(), super::memory::memory_ty()));
        st.memory = Some(m);
        threads_ty.push(super::memory::memory_ty());
    }
    f.result = threaded_result(&f.ret, &threads_ty, cx.types);
    // The backward function takes the returned borrow's new value last, and
    // gives the last value of the one `&mut` parameter it threads.
    if cx.backward {
        let (false, false) = (threads_ty.is_empty(), memory) else {
            return Err(kind(
                "TerminatorKind::Return",
                "a backward function of a body that threads no borrow, or the memory",
            ));
        };
        let one = match threads_ty.as_slice() {
            [one] => one.clone(),
            several => cx.types.tuple(several.to_vec()),
        };
        let rt = f.ret.clone();
        let n = f.fresh(Some("new"), 0);
        f.params.push((n.clone(), rt.clone()));
        f.backward = Some((n, rt));
        f.result = one;
    }
    f.find_loops()?;
    f.scope = f.params.clone();
    // A float intrinsic core declares without a body (it is `unreachable`,
    // the compiler supplying it) is the prelude's model of it, as a call
    // of it is.
    let bodiless = body.blocks.len() == 1
        && body.blocks[0].statements.is_empty()
        && body.blocks[0].terminator == IrTerminator::Unreachable
        && split_path(&body.path).first() == Some(&"intrinsics");
    let modeled = match bodiless {
        true => f
            .call(
                &body.path,
                f.params.clone(),
                &[],
                "",
                &f.result.clone(),
                &mut st.clone(),
            )
            .ok()
            .filter(|c| {
                c.updates.is_empty() && c.term.starts_with(&format!("({}::float", lift::PRELUDE))
            }),
        false => None,
    };
    let value = match modeled {
        Some(c) => Expr::Flat(c.term),
        None => f.block(0, st)?,
    };
    if f.mirrored_shared && f.observes_address {
        return Err(kind(
            MEMORY_KIND,
            "the address of a place a shared borrow lends, copied apart from any other borrow of it",
        ));
    }
    // The memory's model and its laws at each cell size the body uses.
    if memory {
        for d in super::memory::declarations(&f.strides) {
            f.aux.entry(d.name().to_string()).or_insert(d);
        }
    }
    let result = f.result.clone();
    f.mention(&result);
    for (_, t) in f.params.clone() {
        f.mention(&t);
    }
    let main = Decl::Definition {
        name: name.to_string(),
        parameters: f
            .params
            .iter()
            .map(|(n, t)| (n.clone(), t.render()))
            .collect(),
        result: result.render(),
        body: value,
        axioms: Vec::new(),
        recursive: None,
        termination: None,
        mutual: None,
    };
    f.decls.push(main);
    Ok(Lowered {
        decls: f.decls,
        calls: f.calls,
        types: f.types,
        aux: f.aux,
    })
}

impl Fun<'_, '_> {
    /// The `&mut` parameters whose last values the definition gives back.
    fn threads(&self) -> Vec<usize> {
        threaded(
            &self.body.locals,
            self.body.arity,
            super::closure_type(self.body).is_some(),
            self.cx.types,
        )
        .into_iter()
        .filter(|l| !self.unthreaded.contains(l))
        .collect()
    }

    fn mention(&mut self, t: &Ty) {
        self.types.extend(t.mentions());
        self.types.insert(t.clone());
    }

    /// A name for a parameter or binder: the source name where debug info
    /// gives one, never one a definition or another local has.
    fn fresh(&mut self, source: Option<&str>, local: IrLocal) -> String {
        let base = match source {
            Some(s) if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_') => {
                lift::local_name(s)
            }
            _ => format!("local{local}"),
        };
        let mut k = 0usize;
        loop {
            let candidate = if k == 0 {
                base.clone()
            } else {
                format!("{base}{k}")
            };
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            k += 1;
        }
    }

    /// The type of the value at the place `steps` reach from the local
    /// `l`: a borrow (shared or mutable) is the value it borrows, so a
    /// dereference keeps the type.
    fn local_ty(&self, l: IrLocal, steps: &[IrProjection]) -> Lift<Ty> {
        let text = self
            .body
            .locals
            .get(l)
            .ok_or_else(|| kind("IrBody", "a local without a declaration"))?;
        let mut t = ty(text, self.cx.types)?;
        let mut at = Some(text.clone());
        for step in steps {
            t = match step {
                // A raw pointer's pointee, or a reference's referent.
                IrProjection::Deref => match at.as_deref().map(|x| (pointee(x), referent(x))) {
                    Some((Some((pt, _)), _)) => {
                        let pt = pt.to_string();
                        at = Some(pt.clone());
                        ty(&pt, self.cx.types)?
                    }
                    Some((None, Some(r))) => {
                        at = Some(r);
                        t
                    }
                    _ if is_pointer(&t) => return Err(through_pointer("write")),
                    _ => {
                        at = None;
                        t
                    }
                },
                IrProjection::Downcast(_) => {
                    at = None;
                    t
                }
                IrProjection::Field(_, fty) => {
                    at = Some(fty.clone());
                    ty(fty, self.cx.types)?
                }
                IrProjection::Index(_) | IrProjection::ConstantIndex(..) => match t {
                    Ty::List(elem) => {
                        at = at.as_deref().and_then(item_of);
                        *elem
                    }
                    other => {
                        return Err(kind(
                            spec::MirProjectionElem::Index.path(),
                            &format!("an index into {}", other.render()),
                        ))
                    }
                },
                IrProjection::Other(k, _) => return Err(kind(k.path(), "")),
            };
        }
        Ok(t)
    }

    /// The width `usize` has on the narrowest declared target.
    fn pointer_width(&self) -> u32 {
        self.cx.widths.first().map_or(16, |(w, _)| *w)
    }

    fn bits(&self, t: &Ty) -> Option<u32> {
        match t {
            Ty::Nat(Some(b)) => Some(*b),
            Ty::Nat(None) => Some(self.pointer_width()),
            _ => None,
        }
    }

    /// Back edges, loop headers and their regions. A loop inside another
    /// would make two helpers call each other: that is residue.
    fn find_loops(&mut self) -> Lift<()> {
        let n = self.body.blocks.len();
        let succ: Vec<Vec<IrBlockId>> = self
            .body
            .blocks
            .iter()
            .map(|b| b.terminator.successors())
            .collect();
        // Depth-first, iteratively: an edge to a block on the stack is a back
        // edge.
        let mut state = vec![0u8; n];
        let mut back: Vec<(IrBlockId, IrBlockId)> = Vec::new();
        let mut stack: Vec<(IrBlockId, usize)> = vec![(0, 0)];
        state[0] = 1;
        while let Some((b, i)) = stack.pop() {
            if let Some(&s) = succ[b].get(i) {
                stack.push((b, i + 1));
                if s >= n {
                    return Err(kind("TerminatorKind::Goto", "an edge to a missing block"));
                }
                match state[s] {
                    0 => {
                        state[s] = 1;
                        stack.push((s, 0));
                    }
                    1 => back.push((b, s)),
                    _ => {}
                }
            } else {
                state[b] = 2;
            }
        }
        let mut pred: Vec<Vec<IrBlockId>> = vec![Vec::new(); n];
        for (b, ss) in succ.iter().enumerate() {
            for &s in ss {
                pred[s].push(b);
            }
        }
        for (tail, head) in back {
            let region = self.regions.entry(head).or_default();
            region.insert(head);
            let mut work = vec![tail];
            while let Some(b) = work.pop() {
                if region.insert(b) {
                    work.extend(pred[b].iter().copied());
                }
            }
        }
        let heads: Vec<IrBlockId> = self.regions.keys().copied().collect();
        for &h in &heads {
            for &g in &heads {
                if h != g && self.regions[&h].contains(&g) {
                    return Err(kind(
                        "TerminatorKind::Goto",
                        "a loop nested in a loop, whose helpers would call each other",
                    ));
                }
            }
            self.loops.insert(h, None);
        }
        Ok(())
    }

    /// The locals live on entry to `b`: read on some path from it before
    /// they are written.
    fn live_at(&self, b: IrBlockId) -> BTreeSet<IrLocal> {
        let blocks = &self.body.blocks;
        let threads = self.threads();
        let mut uses: Vec<BTreeSet<IrLocal>> = Vec::new();
        let mut defs: Vec<BTreeSet<IrLocal>> = Vec::new();
        for block in blocks {
            let (mut u, mut d) = (BTreeSet::new(), BTreeSet::new());
            let mut read = |l: IrLocal, d: &BTreeSet<IrLocal>| {
                if !d.contains(&l) {
                    u.insert(l);
                }
            };
            for s in &block.statements {
                match s {
                    IrStatement::Assign(p, rv) => {
                        for l in rvalue_reads(rv) {
                            read(l, &d);
                        }
                        if p.projection.is_empty() {
                            d.insert(p.local);
                        } else {
                            for l in place_reads(p) {
                                read(l, &d);
                            }
                        }
                    }
                    IrStatement::Assume(o) => {
                        for l in operand_reads(o) {
                            read(l, &d);
                        }
                    }
                    IrStatement::Marker(_) | IrStatement::Other(..) => {}
                }
            }
            match &block.terminator {
                IrTerminator::Switch(o, ..) | IrTerminator::Assert { condition: o, .. } => {
                    for l in operand_reads(o) {
                        read(l, &d);
                    }
                }
                IrTerminator::Call {
                    destination, args, ..
                } => {
                    for o in args {
                        for l in operand_reads(o) {
                            read(l, &d);
                        }
                    }
                    if destination.projection.is_empty() {
                        d.insert(destination.local);
                    }
                }
                // The return gives `_0` and each `&mut` parameter's value.
                IrTerminator::Return => {
                    read(0, &d);
                    for &l in &threads {
                        read(l, &d);
                    }
                }
                _ => {}
            }
            uses.push(u);
            defs.push(d);
        }
        let mut live_in: Vec<BTreeSet<IrLocal>> = vec![BTreeSet::new(); blocks.len()];
        loop {
            let mut changed = false;
            for i in (0..blocks.len()).rev() {
                let mut out = BTreeSet::new();
                for s in blocks[i].terminator.successors() {
                    out.extend(live_in[s].iter().copied());
                }
                let mut inn: BTreeSet<IrLocal> = out.difference(&defs[i]).copied().collect();
                inn.extend(uses[i].iter().copied());
                if inn != live_in[i] {
                    live_in[i] = inn;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        live_in[b].clone()
    }

    fn grow(&mut self, text: &str) -> Lift<()> {
        self.size += text.len();
        if self.size > self.cx.limit {
            return Err(kind(
                "IrBody",
                "the translation exceeds the projection's file limit",
            ));
        }
        Ok(())
    }

    /// Control reaching `b`: a loop header becomes a call of its helper.
    fn goto(&mut self, b: IrBlockId, st: State) -> Lift<Expr> {
        if !self.loops.contains_key(&b) {
            return self.block(b, st);
        }
        // A loop's helper takes the live locals' values, not the places
        // their borrows lend.
        if !st.lends.is_empty() {
            return Err(kind(
                "TerminatorKind::Goto",
                "a loop header where a live value holds a mutable borrow",
            ));
        }
        let lp = self.helper(b, &st)?;
        let mut args = Vec::new();
        for (l, _, t) in &lp.live {
            match st.env.get(l) {
                Some(Val::V(v, vt)) if vt.render() == t.render() => args.push(v.clone()),
                _ => {
                    return Err(kind(
                        "TerminatorKind::Goto",
                        "a loop header where a live local has no value of its type",
                    ))
                }
            }
        }
        // The loop's invariants hold of the values it is entered with, and
        // of those each pass gives the next.
        for (c, holds) in &lp.invariants {
            let mut goal = String::new();
            let mut word = String::new();
            for ch in c.chars().chain(std::iter::once(' ')) {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    word.push(ch);
                    continue;
                }
                let arg = lp.live.iter().position(|(_, p, _)| *p == word);
                goal.push_str(arg.map_or(word.as_str(), |i| args[i].as_str()));
                word.clear();
                goal.push(ch);
            }
            self.obligation(
                &st,
                &format!("({} = {holds})", goal.trim_end()),
                "TerminatorKind::Goto",
            );
        }
        // The memory the loop runs in, and where its placed locals are.
        let same = st.placed.len() == lp.carried.len()
            && lp.carried.iter().all(|(l, _)| st.placed.contains_key(l));
        if !same {
            return Err(kind(
                MEMORY_KIND,
                "a loop header reached with other locals in memory",
            ));
        }
        for (l, _) in &lp.carried {
            args.push(st.placed[l].pointer.clone());
        }
        if lp.memory.is_some() {
            args.push(self.memory_at(&st)?);
        }
        if let (Some(_), Some((n, _))) = (&lp.new, &self.backward) {
            args.push(n.clone());
        }
        let call = if args.is_empty() {
            lp.name.clone()
        } else {
            format!("({} {})", lp.name, args.join(" "))
        };
        if st.inside == Some(b) {
            self.decreases(b, &lp, &st)?;
        } else {
            self.calls.insert(lp.name.clone());
        }
        self.grow(&call)?;
        Ok(Expr::Flat(call))
    }

    /// The helper of the loop at `h`, built on first use.
    fn helper(&mut self, h: IrBlockId, entry: &State) -> Lift<Loop> {
        if let Some(Some(lp)) = self.loops.get(&h) {
            return Ok(lp.clone());
        }
        let mut live = self.live_at(h);
        live.retain(|l| !entry.placed.contains_key(l));
        let mut params = Vec::new();
        let base = format!("{}Loop{h}", self.name);
        let name = {
            let mut k = 0usize;
            loop {
                let c = if k == 0 {
                    base.clone()
                } else {
                    format!("{base}_{k}")
                };
                if self.used.insert(c.clone()) {
                    break c;
                }
                k += 1;
            }
        };
        let mut st = State {
            inside: Some(h),
            ..State::default()
        };
        for l in live {
            let t = self.local_ty(l, &[]).map_err(|_| {
                kind(
                    "TerminatorKind::Goto",
                    "a loop whose live state has a type outside the lift",
                )
            })?;
            let p = self.fresh(self.body.names.get(l).cloned().flatten().as_deref(), l);
            st.env.insert(l, Val::V(p.clone(), t.clone()));
            params.push((l, p, t));
        }
        let pt = Ty::Named(POINTER.to_string());
        let mut carried = Vec::new();
        for (l, placed) in &entry.placed {
            let p = self.fresh(Some("pointer"), *l);
            st.placed.insert(
                *l,
                Placed {
                    pointer: p.clone(),
                    ..placed.clone()
                },
            );
            carried.push((*l, p));
        }
        let memory = match &entry.memory {
            Some(_) => {
                let m = self.fresh(Some("memory"), 0);
                st.memory = Some(m.clone());
                Some(m)
            }
            None => None,
        };
        // A backward function's helper takes the returned borrow's new
        // value last, and its body writes it at each return.
        let outer = self.backward.clone();
        let new = outer.clone().map(|(_, t)| (self.fresh(Some("new"), 0), t));
        let saved = (
            std::mem::take(&mut self.scope),
            std::mem::take(&mut self.evidence),
            self.measure.take(),
        );
        self.scope = params
            .iter()
            .map(|(_, p, t)| (p.clone(), t.clone()))
            .chain(carried.iter().map(|(_, p)| (p.clone(), pt.clone())))
            .chain(
                memory
                    .iter()
                    .map(|m| (m.clone(), super::memory::memory_ty())),
            )
            .chain(new.iter().cloned())
            .collect();
        // A loop whose header does not exit on an order of naturals: its
        // measure a natural every write in the loop makes smaller (`x / c`,
        // `x - c`), which decreases where the conditions the loop is
        // entered under, and keeps, hold (`exp != 0` before `loop { ..
        // exp /= 2 }`): the helper's invariants.
        let mut invariants: Vec<(String, bool)> = Vec::new();
        let measure = match self.derive_measure(h, &st) {
            Ok(m) => m,
            Err(none) => {
                let region = self.regions.get(&h).cloned().unwrap_or_default();
                // Each local the loop writes, and whether every write of it
                // makes it smaller.
                let mut written: BTreeMap<IrLocal, bool> = BTreeMap::new();
                for b in &region {
                    let block = &self.body.blocks[*b];
                    for s in &block.statements {
                        let IrStatement::Assign(p, rv) = s else {
                            continue;
                        };
                        let shrinks = p.projection.is_empty()
                            && match rv {
                                IrRvalue::Binary(
                                    op,
                                    IrOperand::Copy(q) | IrOperand::Move(q),
                                    IrOperand::Constant(IrConstant::Int(c, _)),
                                ) => {
                                    q.projection.is_empty()
                                        && q.local == p.local
                                        && ((*op == spec::MirBinOp::Div && *c >= 2)
                                            || (*op == spec::MirBinOp::Sub && *c >= 1))
                                }
                                _ => false,
                            };
                        *written.entry(p.local).or_insert(true) &= shrinks;
                        if let IrRvalue::Ref(spec::MirBorrowKind::Mut, q) = rv {
                            written.insert(q.local, false);
                        }
                    }
                    if let IrTerminator::Call { destination, .. } = &block.terminator {
                        written.insert(destination.local, false);
                    }
                }
                let Some((l, p)) = params
                    .iter()
                    .find(|(l, _, t)| matches!(t, Ty::Nat(_)) && written.get(l) == Some(&true))
                    .map(|(l, p, _)| (*l, p.clone()))
                else {
                    return Err(none);
                };
                // The conditions on the way in, over the parameters: of the
                // measure, and of locals the loop does not write.
                let mut at: BTreeMap<String, Option<String>> = BTreeMap::new();
                for (k, q, _) in &params {
                    if let Some(Val::V(v, _)) = entry.env.get(k) {
                        let fixed = *k == l || !written.contains_key(k);
                        let slot = at
                            .entry(v.clone())
                            .or_insert_with(|| fixed.then(|| q.clone()));
                        if !fixed || slot.as_deref() != Some(q.as_str()) {
                            *slot = None;
                        }
                    }
                }
                const WORDS: [&str; 9] = [
                    "prim",
                    "remainder",
                    "quotient",
                    "subtract",
                    "multiply",
                    "Nat",
                    "not",
                    "true",
                    "false",
                ];
                for step in &entry.path {
                    let MirStep::If(c, holds) = step else {
                        continue;
                    };
                    let mut out = String::new();
                    let mut word = String::new();
                    let mut fits = true;
                    let mut named = false;
                    for ch in c.chars().chain(std::iter::once(' ')) {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            word.push(ch);
                            continue;
                        }
                        if !word.is_empty() {
                            if word.chars().all(|d| d.is_ascii_digit())
                                || WORDS.contains(&word.as_str())
                            {
                                out.push_str(&word);
                            } else if let Some(Some(q)) = at.get(&word) {
                                out.push_str(q);
                                named = true;
                            } else {
                                fits = false;
                            }
                            word.clear();
                        }
                        out.push(ch);
                    }
                    if fits && named {
                        invariants.push((out.trim_end().to_string(), *holds));
                    }
                }
                if invariants.is_empty() || lift::default_of(&self.result, self.cx.types).is_none()
                {
                    return Err(none);
                }
                (p, vec![l])
            }
        };
        let lp = Loop {
            name: name.clone(),
            live: params.clone(),
            carried: carried.clone(),
            memory: memory.clone(),
            invariants: invariants.clone(),
            new: new.as_ref().map(|(n, _)| n.clone()),
        };
        self.loops.insert(h, Some(lp.clone()));
        self.measure = Some(measure.clone());
        for (c, holds) in &invariants {
            st.path.push(MirStep::If(c.clone(), *holds));
        }
        if new.is_some() {
            self.backward = new.clone();
        }
        let body = self.block(h, st);
        self.backward = outer;
        let mut body = body?;
        // Where an invariant fails the helper is never called: its value
        // there stands for no path.
        if let Some(d) = lift::default_of(&self.result, self.cx.types) {
            for (c, holds) in invariants.iter().rev() {
                let (then, otherwise) = if *holds {
                    (body, Expr::Flat(d.clone()))
                } else {
                    (Expr::Flat(d.clone()), body)
                };
                body = Expr::If {
                    condition: Box::new(Expr::Flat(c.clone())),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                };
            }
        }
        let evidence = std::mem::take(&mut self.evidence);
        let parameters: Vec<(String, String)> = self
            .scope
            .iter()
            .map(|(n, t)| (n.clone(), t.render()))
            .collect();
        for (_, t) in self.scope.clone() {
            self.mention(&t);
        }
        let result = self.result.clone();
        let names: Vec<String> = evidence
            .iter()
            .filter_map(|d| match d {
                Decl::Theorem { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        self.decls.extend(evidence);
        self.decls.push(Decl::Definition {
            name: name.clone(),
            parameters,
            result: result.render(),
            body,
            axioms: vec!["Quot.sound".into(), "propext".into()],
            recursive: None,
            termination: Some((measure.0, names)),
            mutual: None,
        });
        (self.scope, self.evidence, self.measure) = saved;
        Ok(lp)
    }

    /// The measure of the loop at `h`, from the condition its header exits
    /// on: the header's blocks up to the first `switchInt` on a `bool`, one
    /// of whose targets leaves the loop.
    fn derive_measure(&mut self, h: IrBlockId, st: &State) -> Lift<(String, Vec<IrLocal>)> {
        let none = || {
            kind(
                "TerminatorKind::SwitchInt",
                "a loop whose header does not exit on an order of naturals, so no measure is derived",
            )
        };
        let (cond, stay) = self.header_condition(h, st.clone())?.ok_or_else(none)?;
        let (op, x, y) = comparison(&cond).ok_or_else(none)?;
        let measure = match (op, stay) {
            ("<?", true) => format!("(prim subtract {y} {x} : Nat)"),
            ("<=?", true) => format!("(prim subtract ({y} + 1) {x} : Nat)"),
            ("<?", false) => format!("(prim subtract ({x} + 1) {y} : Nat)"),
            ("<=?", false) => format!("(prim subtract {x} {y} : Nat)"),
            _ => return Err(none()),
        };
        Ok((measure, Vec::new()))
    }

    /// The condition the header at `h` exits on in state `st`, and whether
    /// the loop continues when it is true.
    fn header_condition(&mut self, h: IrBlockId, mut st: State) -> Lift<Option<(String, bool)>> {
        let region = self.regions.get(&h).cloned().unwrap_or_default();
        let mut b = h;
        let mut seen = BTreeSet::new();
        while seen.insert(b) {
            let block = &self.body.blocks[b];
            for s in &block.statements {
                self.statement(s, &mut st)?;
            }
            match &block.terminator {
                IrTerminator::Goto(n) | IrTerminator::False(_, n) | IrTerminator::Drop(n) => {
                    if self.loops.contains_key(n) {
                        return Ok(None);
                    }
                    b = *n;
                }
                // A call on the way, `xs.len()` or `it.next()`: its value,
                // and the new values of the locals it borrows mutably.
                IrTerminator::Call {
                    destination,
                    callee,
                    args,
                    target: Some(n),
                } if destination.projection.is_empty() && !self.loops.contains_key(n) => {
                    // A threading call's tuple is bound in the body, not in
                    // a measure.
                    if self
                        .call_into(destination, callee, args, *n, &mut st)?
                        .is_some()
                    {
                        return Ok(None);
                    }
                    b = *n;
                }
                IrTerminator::Switch(op, arms, otherwise) => {
                    let target = |v: i128| {
                        arms.iter()
                            .find(|(a, _)| *a == v)
                            .map_or(*otherwise, |(_, b)| *b)
                    };
                    // A switch on a guarded option's discriminant exits on
                    // its guard: `None` is 0 and `Some` is 1.
                    let (c, on_true, on_false) = match self.guarded_disc(op, &st) {
                        Some((guard, _, _)) => (guard, target(1), target(0)),
                        None => {
                            let (c, t) = self.operand(op, &st)?;
                            if t != Ty::Bool {
                                return Ok(None);
                            }
                            match arms.as_slice() {
                                [(0, f)] => (c, *otherwise, *f),
                                [(1, t)] => (c, *t, *otherwise),
                                _ => return Ok(None),
                            }
                        }
                    };
                    return Ok(
                        match (region.contains(&on_true), region.contains(&on_false)) {
                            (true, false) => Some((c, true)),
                            (false, true) => Some((c, false)),
                            _ => None,
                        },
                    );
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    /// The theorem that a recursive call of the loop at `h` decreases its
    /// measure: the header's condition, evaluated at the call's arguments,
    /// against the same condition at the parameters.
    fn decreases(&mut self, h: IrBlockId, lp: &Loop, st: &State) -> Lift<()> {
        let Some((measure, _)) = self.measure.clone() else {
            return Err(kind("TerminatorKind::Goto", "a loop without a measure"));
        };
        // A measure the loop's writes make smaller: its value on the way
        // back, below its value on the way in.
        let shrinking = self
            .measure
            .as_ref()
            .and_then(|(_, ls)| ls.first().copied());
        if let Some(l) = shrinking {
            let Some(Val::V(after, _)) = st.env.get(&l) else {
                return Err(kind(
                    "TerminatorKind::Goto",
                    "a recursive call whose measure does not read",
                ));
            };
            let name = format!("decreases_{}_{}", lp.name, self.evidence.len());
            let statement = implication(&hypotheses(&st.path), &format!("({after} < {measure})"));
            let mut parameters: Vec<(String, String)> = self
                .scope
                .iter()
                .map(|(n, t)| (n.clone(), t.render()))
                .collect();
            parameters.extend(st.binders.iter().map(|(n, t)| (n.clone(), t.render())));
            self.evidence.push(Decl::Theorem {
                name,
                parameters,
                statement,
                proof: "linear_arithmetic{}".into(),
                axioms: vec!["Quot.sound".into(), "propext".into()],
            });
            return Ok(());
        }
        let mut next = State {
            inside: Some(h),
            ..State::default()
        };
        for (l, _, _) in &lp.live {
            if let Some(v) = st.env.get(l) {
                next.env.insert(*l, v.clone());
            }
        }
        next.placed = st.placed.clone();
        next.memory = st.memory.clone();
        let after = self
            .header_condition(h, next)?
            .and_then(|(c, stay)| comparison(&c).map(|(op, x, y)| (op.to_string(), x, y, stay)));
        let Some((op, x, y, stay)) = after else {
            return Err(kind(
                "TerminatorKind::Goto",
                "a recursive call whose measure does not read",
            ));
        };
        let then = match (op.as_str(), stay) {
            ("<?", true) => format!("(prim subtract {y} {x} : Nat)"),
            ("<=?", true) => format!("(prim subtract ({y} + 1) {x} : Nat)"),
            ("<?", false) => format!("(prim subtract ({x} + 1) {y} : Nat)"),
            _ => format!("(prim subtract {x} {y} : Nat)"),
        };
        let name = format!("decreases_{}_{}", lp.name, self.evidence.len());
        let statement = implication(&hypotheses(&st.path), &format!("({then} < {measure})"));
        let mut parameters: Vec<(String, String)> = self
            .scope
            .iter()
            .map(|(n, t)| (n.clone(), t.render()))
            .collect();
        parameters.extend(st.binders.iter().map(|(n, t)| (n.clone(), t.render())));
        self.evidence.push(Decl::Theorem {
            name,
            parameters,
            statement,
            proof: "linear_arithmetic{}".into(),
            axioms: vec!["Quot.sound".into(), "propext".into()],
        });
        Ok(())
    }

    /// The theorem that `goal` holds where control is: under the path's
    /// conditions and the language's facts about its terms.
    fn obligation(&mut self, st: &State, goal: &str, what: &str) {
        let mut hyps: Vec<String> = st.facts.iter().cloned().collect();
        let path = hypotheses(&st.path);
        // The range of every value of a fixed-width type the theorem names.
        let text = format!("{} {goal}", path.join(" "));
        let mut ranged: Vec<(String, u32)> = self
            .scope
            .iter()
            .chain(st.binders.iter())
            .filter_map(|(n, t)| fixed(t).map(|b| (n.clone(), b)))
            .collect();
        ranged.extend(self.ranged.iter().map(|(t, b)| (t.clone(), *b)));
        for (term, bits) in ranged {
            if term_mentions(&text, &term) {
                hyps.push(format!("({term} <= {})", max_of(bits)));
            }
        }
        for (term, facts) in &self.admitted {
            if term_mentions(&text, term) {
                hyps.extend(facts.iter().cloned());
            }
        }
        hyps.extend(path);
        let name = format!("holds_{}_{}", self.name, self.obligations);
        self.obligations += 1;
        let mut parameters: Vec<(String, String)> = self
            .scope
            .iter()
            .map(|(n, t)| (n.clone(), t.render()))
            .collect();
        parameters.extend(st.binders.iter().map(|(n, t)| (n.clone(), t.render())));
        let _ = what;
        let statement = implication(&hyps, goal);
        // A goal about memory is decided by the model's definitions.
        let unfolded = super::memory::unfolding(&statement);
        let proof = format!(
            "linear_arithmetic{{definitions: [{}]}}",
            unfolded.join(", ")
        );
        let proof = if unfolded.is_empty() {
            "linear_arithmetic{}".into()
        } else {
            proof
        };
        self.decls.push(Decl::Theorem {
            name,
            parameters,
            statement,
            proof,
            axioms: vec!["Quot.sound".into(), "propext".into()],
        });
    }

    fn block(&mut self, b: IrBlockId, mut st: State) -> Lift<Expr> {
        let block = self
            .body
            .blocks
            .get(b)
            .ok_or_else(|| kind("TerminatorKind::Goto", "an edge to a missing block"))?;
        for s in &block.statements {
            self.statement(s, &mut st)?;
        }
        match &block.terminator {
            IrTerminator::Goto(n) | IrTerminator::Drop(n) | IrTerminator::False(_, n) => {
                self.goto(*n, st)
            }
            IrTerminator::Return => {
                let whole = IrPlace {
                    local: 0,
                    projection: Vec::new(),
                };
                // The backward function: the returned borrow takes its new
                // value, which writes the place it borrows, and the
                // parameter's last value is the result. A `None` returned
                // holds no borrow.
                if let Some((new, nt)) = self.backward.clone() {
                    let mut st = st.clone();
                    match st.env.get(&0).cloned() {
                        Some(Val::Mut(q)) => self.write(&q, Val::V(new, nt), &mut st)?,
                        Some(Val::V(..)) if st.lends.contains_key(&0) => {
                            self.write(&whole, Val::V(new, nt), &mut st)?
                        }
                        Some(Val::V(v, _)) if v.starts_with("(ctor Option.none ") => {}
                        _ => {
                            return Err(kind(
                                "TerminatorKind::Return",
                                "a returned borrow whose place is not known",
                            ))
                        }
                    }
                    let threads = self.threads();
                    let mut items = Vec::new();
                    for l in threads {
                        let place = IrPlace {
                            local: l,
                            projection: Vec::new(),
                        };
                        items.push(self.read(&place, &st)?.0);
                    }
                    let v = match items.as_slice() {
                        [one] => one.clone(),
                        _ => lift::tuple_value(&self.result, &items),
                    };
                    self.grow(&v)?;
                    return Ok(Expr::Flat(v));
                }
                let v = match st.env.get(&0) {
                    // A returned borrow is the value of the place it borrows.
                    Some(Val::V(..) | Val::Mut(_)) => self.read(&whole, &st)?.0,
                    _ if self.ret == Ty::Unit => "()".into(),
                    _ => {
                        return Err(kind(
                            "TerminatorKind::Return",
                            "the return place holds no value",
                        ))
                    }
                };
                let threads = self.threads();
                let v = if threads.is_empty() && !self.memory {
                    v
                } else {
                    // `_0`, then the last value of each `&mut` parameter,
                    // then the memory.
                    let mut items = vec![v];
                    for l in threads {
                        let place = IrPlace {
                            local: l,
                            projection: Vec::new(),
                        };
                        items.push(self.read(&place, &st)?.0);
                    }
                    if self.memory {
                        items.push(self.returned_memory(&items[0], &st)?);
                    }
                    lift::tuple_value(&self.result, &items)
                };
                self.grow(&v)?;
                Ok(Expr::Flat(v))
            }
            IrTerminator::Unreachable => self.unreachable(&st, "unreachable"),
            IrTerminator::Assert {
                condition,
                expected,
                kind: k,
                target,
            } => {
                let (c, t) = self.operand(condition, &st)?;
                if t != Ty::Bool {
                    return Err(kind(
                        spec::MirTerminatorKind::Assert.path(),
                        "a condition that is not a bool",
                    ));
                }
                let goal = format!("({c} = {expected})");
                self.obligation(&st, &goal, k.path());
                self.goto(*target, st)
            }
            IrTerminator::Switch(op, arms, otherwise) => self.switch(op, arms, *otherwise, st),
            IrTerminator::Call {
                destination,
                callee,
                args,
                target,
            } => {
                // A call into a place of type `!` does not return: MIR
                // prints its one successor, the unwind, as `-> bb`.
                let never = destination.projection.is_empty()
                    && self
                        .body
                        .locals
                        .get(destination.local)
                        .is_some_and(|t| t.trim() == "!");
                let Some(target) = target.as_ref().filter(|_| !never) else {
                    // A call that does not return panics or diverges: the
                    // path to it must be unreachable, unless the body
                    // itself returns `!`, whose callers carry that theorem.
                    // `!` has no value, so its stand-in is never observed.
                    if self.diverges {
                        let d = lift::default_of(&self.result, self.cx.types).ok_or_else(|| {
                            kind(
                                spec::MirTerminatorKind::Call.path(),
                                "a diverging body whose result has no value to stand for `!`",
                            )
                        })?;
                        return Ok(Expr::Flat(d));
                    }
                    return self.unreachable(&st, callee);
                };
                let binding = self.call_into(destination, callee, args, *target, &mut st)?;
                let rest = self.goto(*target, st)?;
                Ok(match binding {
                    Some((name, ty, value)) => Expr::Let {
                        name,
                        ty,
                        value: Box::new(Expr::Flat(value)),
                        body: Box::new(rest),
                    },
                    None => rest,
                })
            }
            IrTerminator::Other(k, _) => Err(kind(k.path(), "")),
        }
    }

    /// The place a mutable borrow of `p` borrows: through a borrow it
    /// reborrows, the place that borrow borrows; a `&mut` parameter is the
    /// value it holds.
    fn borrowed(&self, p: &IrPlace, st: &State) -> Lift<IrPlace> {
        match (st.env.get(&p.local), p.projection.split_first()) {
            (Some(Val::Mut(q)), Some((IrProjection::Deref, rest))) => {
                let mut q = q.clone();
                q.projection.extend(rest.iter().cloned());
                Ok(q)
            }
            (_, Some((IrProjection::Deref, rest))) if self.mut_params.contains(&p.local) => {
                Ok(IrPlace {
                    local: p.local,
                    projection: rest.to_vec(),
                })
            }
            (Some(Val::V(..)), _)
                if p.projection
                    .iter()
                    .all(|s| matches!(s, IrProjection::Field(..))) =>
            {
                Ok(p.clone())
            }
            // Through a raw pointer: the place in memory it points to.
            (Some(Val::V(..)), Some((IrProjection::Deref, _)))
                if self
                    .body
                    .locals
                    .get(p.local)
                    .and_then(|t| pointee(t))
                    .is_some() =>
            {
                Ok(p.clone())
            }
            // Through a `&mut` a threaded value or a lending local holds:
            // the part of the value that holds what it borrows.
            // A variant's field the value is matched as, too.
            (Some(Val::V(..) | Val::Env(..)), _)
                if (self.mut_params.contains(&p.local) || st.lends.contains_key(&p.local))
                    && p.projection.iter().all(|s| {
                        matches!(
                            s,
                            IrProjection::Field(..)
                                | IrProjection::Deref
                                | IrProjection::Downcast(_)
                        )
                    }) =>
            {
                Ok(p.clone())
            }
            _ => Err(kind(
                spec::MirRvalue::Ref.path(),
                "BorrowKind::Mut of a place behind a shared reference",
            )),
        }
    }

    /// Write `v` into the place `p`: a local takes it, a part of a local's
    /// value is replaced in a new value of the local, and a write through a
    /// mutable borrow writes the place it borrows.
    fn write(&mut self, p: &IrPlace, v: Val, st: &mut State) -> Lift<()> {
        if let Some((view, steps)) = self.view_of(p, st)? {
            return self.store(&view, &steps, v, st);
        }
        let k = spec::MirStatementKind::Assign.path();
        if let (Some(Val::Mut(q)), Some((IrProjection::Deref, rest))) =
            (st.env.get(&p.local), p.projection.split_first())
        {
            let mut q = q.clone();
            q.projection.extend(rest.iter().cloned());
            return self.write(&q, v, st);
        }
        // A `&mut` the local holds (as a parameter, or inside its value)
        // is the value it borrows, so a write through it writes that part.
        let held = self.mut_params.contains(&p.local) || st.lends.contains_key(&p.local);
        let mut steps: Vec<IrProjection> = p
            .projection
            .iter()
            .filter(|s| !(matches!(s, IrProjection::Deref) && held))
            .cloned()
            .collect();
        if steps.is_empty() {
            st.env.insert(p.local, v);
        } else {
            let Val::V(new, _) = v else {
                return Err(kind(k, "a pair or a borrow written into a part of a value"));
            };
            let (whole, wt, env) = match st.env.get(&p.local).cloned() {
                Some(Val::V(whole, wt)) => (whole, wt, None),
                // A closure's environment it threads: a capture, the
                // environment itself where it is the one capture.
                Some(Val::Env(whole, et, captures)) if held => {
                    if captures.len() == 1 {
                        steps.remove(0);
                    }
                    (whole, et, Some(captures))
                }
                _ => {
                    return Err(kind(
                        k,
                        "a write into a part of a local that holds no value",
                    ))
                }
            };
            let updated = self.replace(&whole, &wt, &steps, &new, st)?;
            self.grow(&updated)?;
            st.env.insert(
                p.local,
                match env {
                    Some(captures) => Val::Env(updated, wt, captures),
                    None => Val::V(updated, wt),
                },
            );
        }
        // Each place a borrow the local holds lends takes the value the
        // local now holds there.
        for (path, place, back) in st.lends.get(&p.local).cloned().unwrap_or_default() {
            let apply = |v: &str| match &back {
                Some((b, None)) => format!("({b} {v})"),
                Some((b, Some((rt, k)))) => lift::tuple_item(rt, &format!("({b} {v})"), *k),
                None => v.to_string(),
            };
            // A borrow `Some` holds lends while the value holds `Some`:
            // otherwise the borrow is gone, and the place keeps its value.
            if let Some(i) = path
                .iter()
                .position(|s| matches!(s, IrProjection::Downcast(_)))
            {
                let (Some(IrProjection::Downcast(variant)), Some(IrProjection::Field(0, _)), true) =
                    (path.get(i), path.get(i + 1), path.len() == i + 2)
                else {
                    return Err(kind(
                        k,
                        "a borrow held in a variant other than `Some`'s field",
                    ));
                };
                let outer = IrPlace {
                    local: p.local,
                    projection: path[..i].to_vec(),
                };
                let (o, ot) = self.read(&outer, st)?;
                if variant != "Some" || !matches!(ot, Ty::Option(_)) {
                    return Err(kind(
                        k,
                        "a borrow held in a variant other than `Some`'s field",
                    ));
                }
                let (cur, pt) = self.read(&place, st)?;
                let held = self.fresh(Some("held"), p.local);
                let v = format!(
                    "(match {o} | Option.none => {cur} | Option.some {held} => {})",
                    apply(&held)
                );
                self.write(&place, Val::V(v, pt), st)?;
                continue;
            }
            let at = IrPlace {
                local: p.local,
                projection: path,
            };
            let (v, t) = self.read(&at, st)?;
            let (v, t) = match &back {
                Some(_) => (apply(&v), self.read(&place, st)?.1),
                None => (v, t),
            };
            self.write(&place, Val::V(v, t), st)?;
        }
        Ok(())
    }

    /// `whole` with the part `steps` reach replaced by `new`.
    fn replace(
        &mut self,
        whole: &str,
        t: &Ty,
        steps: &[IrProjection],
        new: &str,
        st: &State,
    ) -> Lift<String> {
        let k = spec::MirStatementKind::Assign.path();
        let Some((step, rest)) = steps.split_first() else {
            return Ok(new.to_string());
        };
        // An item of a list: the list with that item replaced, which an
        // `assert` (or a slice pattern's length check) before the write
        // proves in bounds.
        let at = match (step, t) {
            (IrProjection::Index(l), Ty::List(_)) => match st.env.get(l) {
                Some(Val::V(i, Ty::Nat(_))) => Some(i.clone()),
                _ => {
                    return Err(kind(
                        spec::MirProjectionElem::Index.path(),
                        "an index that is not a natural",
                    ))
                }
            },
            (IrProjection::ConstantIndex(n, true), Ty::List(_)) => Some(format!(
                "(prim subtract (prim length {whole} : Nat) {n} : Nat)"
            )),
            (IrProjection::ConstantIndex(n, false), Ty::List(_)) => Some(n.to_string()),
            _ => None,
        };
        if let (Some(i), Ty::List(elem)) = (at, t) {
            let read = self.index_aux(elem, false)?;
            let part = format!("({read} {whole} {i})");
            let inner = self.replace(&part, elem, rest, new, st)?;
            let update = self.index_aux(elem, true)?;
            return Ok(format!("({update} {whole} {i} {inner})"));
        }
        // `Some`'s field: the option with it replaced where it holds
        // `Some`, else the option as it is.
        if let (
            IrProjection::Downcast(v),
            Some((IrProjection::Field(0, _), after)),
            Ty::Option(item),
        ) = (step, rest.split_first(), t)
        {
            if v == "Some" {
                let held = self.fresh(Some("held"), 0);
                let inner = self.replace(&held, item, after, new, st)?;
                let i = item.render();
                return Ok(format!(
                    "(match {whole} | Option.none => (ctor Option.none [{i}]) | Option.some {held} => (ctor Option.some [{i}] {inner}))"
                ));
            }
        }
        // The one variant a place is matched as: its binder replaced.
        if let IrProjection::Downcast(_) = step {
            return Err(kind(k, "a write into a field of an enum's variant"));
        }
        if matches!(step, IrProjection::Deref) && is_pointer(t) {
            return Err(through_pointer("write"));
        }
        // A `MaybeUninit`'s value written: it holds that value now.
        if let (IrProjection::Field(1, fty), Ty::Option(inner)) = (step, t) {
            if wrapper(fty).is_some_and(|(n, _)| n == "ManuallyDrop") {
                let d = lift::default_of(inner, self.cx.types)
                    .ok_or_else(|| kind(MEMORY_KIND, "a MaybeUninit of a type with no value"))?;
                let part =
                    format!("(match {whole} | Option.none => {d} | Option.some held => held)");
                let inner_new = self.replace(&part, inner, rest, new, st)?;
                return Ok(format!(
                    "(ctor Option.some [{}] {inner_new})",
                    inner.render()
                ));
            }
        }
        if let IrProjection::Field(0, fty) = step {
            if (is_pointer(t) || !matches!(t, Ty::Named(_) | Ty::Tuple(_) | Ty::Pair(_)))
                && ty(fty, self.cx.types).ok().as_ref() == Some(t)
            {
                return self.replace(whole, t, rest, new, st);
            }
        }
        let IrProjection::Field(i, _) = step else {
            let step = match step {
                IrProjection::Deref => spec::MirProjectionElem::Deref,
                IrProjection::Index(_) => spec::MirProjectionElem::Index,
                IrProjection::ConstantIndex(..) => spec::MirProjectionElem::ConstantIndex,
                IrProjection::Downcast(_) => spec::MirProjectionElem::Downcast,
                IrProjection::Other(k, _) => *k,
                IrProjection::Field(..) => spec::MirProjectionElem::Field,
            };
            return Err(kind(k, &format!("a write through {}", step.path())));
        };
        let part = self.field(whole, t, *i)?;
        let pt = self.field_ty(t, *i)?;
        let inner = self.replace(&part, &pt, rest, new, st)?;
        match t {
            Ty::Tuple(ts) | Ty::Pair(ts) => {
                let items: Vec<String> = (0..ts.len())
                    .map(|j| {
                        if j == *i {
                            inner.clone()
                        } else {
                            lift::tuple_item(t, whole, j)
                        }
                    })
                    .collect();
                Ok(lift::tuple_value(t, &items))
            }
            Ty::Named(n) => {
                let fields = self.cx.types.structs.get(n).cloned().unwrap_or_default();
                let mut parts = Vec::new();
                for (j, (f, _)) in fields.iter().enumerate() {
                    let v = if j == *i {
                        inner.clone()
                    } else {
                        self.field(whole, t, j)?
                    };
                    parts.push((f, v));
                }
                Ok(lift::struct_value(n, &parts, self.cx.types))
            }
            _ => Err(kind(k, &format!("a write into a field of {}", t.render()))),
        }
    }

    /// The type of the `i`-th field of a struct or tuple type.
    fn field_ty(&self, t: &Ty, i: usize) -> Lift<Ty> {
        let k = spec::MirProjectionElem::Field.path();
        match t {
            Ty::Tuple(ts) | Ty::Pair(ts) => ts
                .get(i)
                .cloned()
                .ok_or_else(|| kind(k, "a field past the tuple's")),
            Ty::Named(n) => self
                .cx
                .types
                .structs
                .get(n)
                .and_then(|fs| fs.get(i))
                .map(|(_, t)| t.clone())
                .ok_or_else(|| kind(k, &format!("a field of {n}, which is not a struct"))),
            _ => Err(kind(k, &format!("a field of {}", t.render()))),
        }
    }

    /// A call that returns: its value into `destination`, and each mutably
    /// borrowed place's new value from the callee's backward function. A
    /// callee that threads its `&mut` parameters returns a tuple, which is
    /// bound once: the binding to wrap the rest of the path in.
    fn call_into(
        &mut self,
        destination: &IrPlace,
        callee: &str,
        args: &[IrOperand],
        target: IrBlockId,
        st: &mut State,
    ) -> Lift<Option<(String, String, String)>> {
        if let Some(g) = self
            .selected(callee, args)
            .filter(|_| destination.projection.is_empty())
        {
            self.select_into(destination, &g, &args[0], st)?;
            return Ok(None);
        }
        let mut vals = Vec::new();
        let mut texts = Vec::new();
        let mut borrowed: BTreeMap<usize, IrPlace> = BTreeMap::new();
        // The place each argument that is a borrow lends.
        let mut lends_place: Vec<Option<IrPlace>> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let whole = match a {
                IrOperand::Copy(p) | IrOperand::Move(p) if p.projection.is_empty() => Some(p.local),
                _ => None,
            };
            texts.push(whole.and_then(|l| self.body.locals.get(l).cloned()));
            lends_place.push(whole.and_then(|l| match st.env.get(&l) {
                Some(Val::Mut(q)) => Some(q.clone()),
                _ => st.shared.get(&l).cloned().or_else(|| {
                    // A borrow parameter of this body passed on, which
                    // lends the place it borrows.
                    st.placed.get(&l).filter(|p| p.lent).map(|_| IrPlace {
                        local: l,
                        projection: vec![IrProjection::Deref],
                    })
                }),
            }));
            match whole.and_then(|l| st.env.get(&l)) {
                Some(Val::Mut(place)) => {
                    let place = place.clone();
                    vals.push(self.read(&place, st)?);
                    borrowed.insert(i, place);
                }
                _ => {
                    // A `&mut` parameter passed on lends the place it holds,
                    // and a value holding borrows, the local that holds it.
                    if let Some(l) =
                        whole.filter(|l| self.mut_params.contains(l) || st.lends.contains_key(l))
                    {
                        borrowed.insert(
                            i,
                            IrPlace {
                                local: l,
                                projection: Vec::new(),
                            },
                        );
                    }
                    vals.push(self.operand(a, st)?);
                }
            }
        }
        let dt = self.local_ty(destination.local, &destination.projection)?;
        let dest_text = if destination.projection.is_empty() {
            self.body.locals[destination.local].clone()
        } else {
            String::new()
        };
        // The arguments passed by address, whose writes the memory holds.
        let mut by_address = BTreeSet::new();
        let called = match self.call(callee, vals.clone(), &texts, &dest_text, &dt, st) {
            Err(r) if is_needs_address(&r) => {
                for i in std::mem::take(&mut self.wants_address) {
                    by_address.insert(i);
                    let Some(q) = lends_place.get(i).cloned().flatten() else {
                        return Err(kind(
                            MEMORY_KIND,
                            "an argument a callee takes the address of, whose place is not known",
                        ));
                    };
                    let wide = texts
                        .get(i)
                        .cloned()
                        .flatten()
                        .and_then(|t| referent(&t))
                        .is_some_and(|r| (r.starts_with('[') && !r.contains(';')) || r == "str");
                    // A place with no cells of its own here (a field of a
                    // struct the memory does not hold) lends a cell of its
                    // own: its value, copied in; a `&mut`'s new value comes
                    // back as the callee's threaded result.
                    let pointer = match self.address_at(&q, wide, st) {
                        Ok(pointer) => pointer,
                        Err(r) if !wide => {
                            let referent_text = texts
                                .get(i)
                                .cloned()
                                .flatten()
                                .and_then(|t| referent(&t))
                                .ok_or_else(|| r.clone())?;
                            self.lent_cell(&q, &referent_text, st).map_err(|_| r)?
                        }
                        Err(r) => return Err(r),
                    };
                    vals[i] = (pointer, Ty::Named(POINTER.to_string()));
                }
                self.call(callee, vals.clone(), &texts, &dest_text, &dt, st)?
            }
            other => other?,
        };
        let Called {
            term,
            mut updates,
            guard,
            threads,
            memory,
            backward,
        } = called;
        // A threading callee's tuple, bound once: `_0`'s value first, then
        // each `&mut` argument's new value.
        let mut binding = None;
        let term = match threads {
            Some((rt, positions)) => {
                let name = self.fresh(Some("called"), destination.local);
                st.facts.insert(format!("({name} = {term})"));
                st.binders.push((name.clone(), rt.clone()));
                self.mention(&rt);
                for (k, p) in positions.iter().enumerate() {
                    updates.insert(*p, lift::tuple_item(&rt, &name, k + 1));
                }
                if memory {
                    st.memory = Some(lift::tuple_item(&rt, &name, positions.len() + 1));
                }
                binding = Some((name.clone(), rt.render(), term));
                lift::tuple_item(&rt, &name, 0)
            }
            None => term,
        };
        // An argument the callee writes through, whose place the translation
        // does not hold, would lose the write.
        for (i, new) in &updates {
            if !borrowed.contains_key(i) && vals.get(*i).is_some_and(|(v, _)| v != new) {
                return Err(kind(
                    spec::MirRvalue::Ref.path(),
                    &format!(
                        "BorrowKind::Mut passed to {}, a borrow whose place is not known",
                        erase(callee)
                    ),
                ));
            }
        }
        // Each mutably borrowed place takes the value the call's backward
        // function gives it.
        let borrowed_places = borrowed.clone();
        for (i, place) in borrowed {
            if by_address.contains(&i) && !updates.contains_key(&i) {
                continue;
            }
            let Some(new) = updates.get(&i) else {
                // A borrowed value no one reads again needs no new value,
                // unless it holds a borrow of a place that lives on.
                if !self.live_at(target).contains(&place.local)
                    && !st.lends.contains_key(&place.local)
                {
                    continue;
                }
                return Err(kind(
                    spec::MirRvalue::Ref.path(),
                    &format!(
                        "BorrowKind::Mut passed to {}, which has no backward function",
                        erase(callee)
                    ),
                ));
            };
            self.grow(new)?;
            self.write(&place, Val::V(new.clone(), vals[i].1.clone()), st)?;
        }
        let v = term;
        let value = match (guard, &dt) {
            (Some(guard), Ty::Option(item)) => Val::Guarded {
                guard,
                value: v,
                item: (**item).clone(),
            },
            (Some(_), _) => {
                return Err(kind(
                    spec::MirTerminatorKind::Call.path(),
                    &format!(
                        "{}, a guarded model whose result is no Option",
                        erase(callee)
                    ),
                ))
            }
            (None, _) => {
                if let Some(b) = fixed(&dt) {
                    self.ranged.insert(v.clone(), b);
                }
                Val::V(v, dt)
            }
        };
        if destination.projection.is_empty() && !st.placed.contains_key(&destination.local) {
            st.env.insert(destination.local, value);
            st.lends.remove(&destination.local);
            // A struct returned holding a borrow: each `&mut` field
            // borrows the one place the call lends at the field's type.
            let held = self
                .cx
                .types
                .aliases
                .get(dest_text.trim())
                .and_then(|n| self.cx.types.borrowing.get(n))
                .cloned()
                .unwrap_or_default();
            let mut lends = Vec::new();
            for (f, text) in held {
                let at = ty(&text, self.cx.types)?;
                let mut lent = borrowed_places.iter().filter(|(i, _)| {
                    texts
                        .get(**i)
                        .and_then(|t| t.as_deref())
                        .and_then(mut_target)
                        .is_some_and(|t| ty(t, self.cx.types).is_ok_and(|t| t == at))
                });
                match (lent.next(), lent.next()) {
                    (Some((_, place)), None) => {
                        lends.push((vec![IrProjection::Field(f, text)], place.clone(), None));
                    }
                    _ => {
                        return Err(kind(
                            spec::MirTerminatorKind::Call.path(),
                            &format!(
                            "{}, whose value holds a borrow of a place the call does not lend once",
                            erase(callee)
                        ),
                        ))
                    }
                }
            }
            // A borrow the call returns into a `&mut` argument lends that
            // argument's place, through the callee's backward function.
            if let (Some((back, positions, rt)), true) = (backward, lends.is_empty()) {
                for (k, i) in positions.iter().enumerate() {
                    if let Some(place) = borrowed_places.get(i) {
                        let item = rt.clone().map(|t| (t, k));
                        lends.push((Vec::new(), place.clone(), Some((back.clone(), item))));
                    }
                }
            }
            if !lends.is_empty() {
                st.lends.insert(destination.local, lends);
            }
        } else {
            self.write(destination, value, st)?;
        }
        Ok(binding)
    }

    /// In core's scope, the function a call of the intrinsic
    /// `const_eval_select(arg, called_in_const, called_at_rt)` selects at
    /// run time: its third argument.
    fn selected(&self, callee: &str, args: &[IrOperand]) -> Option<String> {
        let link = self.cx.core?;
        let path = link
            .path(&erase(callee))
            .filter(|p| link.decls.intrinsics.contains(p))?;
        if split_path(&path).last() != Some(&"const_eval_select") {
            return None;
        }
        match args {
            [_, _, IrOperand::Constant(IrConstant::Path(g) | IrConstant::Other(g))] => {
                Some(g.clone())
            }
            _ => None,
        }
    }

    /// `g` applied to the items of the tuple `arg`, into `destination`.
    fn select_into(
        &mut self,
        destination: &IrPlace,
        g: &str,
        arg: &IrOperand,
        st: &mut State,
    ) -> Lift<()> {
        let k = spec::MirTerminatorKind::Call.path();
        let (v, t) = self.operand(arg, st)?;
        let text = match arg {
            IrOperand::Copy(p) | IrOperand::Move(p) if p.projection.is_empty() => {
                self.body.locals.get(p.local).cloned()
            }
            _ => None,
        }
        .ok_or_else(|| kind(k, "const_eval_select of no local's tuple"))?;
        let inner = text
            .trim()
            .strip_prefix('(')
            .and_then(|r| r.strip_suffix(')'))
            .ok_or_else(|| kind(k, "const_eval_select of no tuple"))?;
        let texts: Vec<Option<String>> = split_top(inner, ',')
            .into_iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| Some(p.trim().to_string()))
            .collect();
        let vals: Vec<(String, Ty)> = match (texts.len(), &t) {
            (0, _) => Vec::new(),
            (1, _) => vec![(v, t.clone())],
            (n, Ty::Tuple(ts) | Ty::Pair(ts)) if ts.len() == n => (0..n)
                .map(|j| (lift::tuple_item(&t, &v, j), ts[j].clone()))
                .collect(),
            _ => {
                return Err(kind(
                    k,
                    "const_eval_select of a tuple the lift does not split",
                ))
            }
        };
        let dt = self.local_ty(destination.local, &[])?;
        let dest_text = self.body.locals[destination.local].clone();
        let called = self.call(g, vals, &texts, &dest_text, &dt, st)?;
        if called.guard.is_some() || !called.updates.is_empty() || called.threads.is_some() {
            return Err(kind(
                k,
                &format!("{}, selected with a guard or an update", erase(g)),
            ));
        }
        if let Some(b) = fixed(&dt) {
            self.ranged.insert(called.term.clone(), b);
        }
        st.env.insert(destination.local, Val::V(called.term, dt));
        Ok(())
    }

    /// The guard, held value and item type of the guarded option whose
    /// discriminant `op` reads.
    fn guarded_disc(&self, op: &IrOperand, st: &State) -> Option<(String, String, Ty)> {
        let (IrOperand::Copy(p) | IrOperand::Move(p)) = op else {
            return None;
        };
        let Some(Val::Disc(place)) = st.env.get(&p.local) else {
            return None;
        };
        if !(p.projection.is_empty() && place.projection.is_empty()) {
            return None;
        }
        match st.env.get(&place.local) {
            Some(Val::Guarded { guard, value, item }) => {
                Some((guard.clone(), value.clone(), item.clone()))
            }
            _ => None,
        }
    }

    fn unreachable(&mut self, st: &State, what: &str) -> Lift<Expr> {
        self.obligation(st, "(false = true)", what);
        let result = self.result.clone();
        let d = lift::default_of(&result, self.cx.types).ok_or_else(|| {
            kind(
                spec::MirTerminatorKind::Unreachable.path(),
                "a result type with no value to stand for a path proved unreachable",
            )
        })?;
        Ok(Expr::Flat(d))
    }

    fn switch(
        &mut self,
        op: &IrOperand,
        arms: &[(i128, IrBlockId)],
        otherwise: IrBlockId,
        st: State,
    ) -> Lift<Expr> {
        if let Some((guard, value, _)) = self.guarded_disc(op, &st) {
            return self.guarded_switch(op, &guard, &value, arms, otherwise, st);
        }
        let disc = match op {
            IrOperand::Copy(p) | IrOperand::Move(p) if p.projection.is_empty() => {
                match st.env.get(&p.local) {
                    Some(Val::Disc(place)) => Some(place.clone()),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(place) = disc {
            return self.match_on(&place, arms, otherwise, st);
        }
        let (c, t) = self.operand(op, &st)?;
        let target = |v: i128| {
            arms.iter()
                .find(|(a, _)| *a == v)
                .map_or(otherwise, |(_, b)| *b)
        };
        match t {
            // A switch on a constant takes its one arm.
            Ty::Bool if c == "true" || c == "false" => {
                self.goto(target(i128::from(c == "true")), st)
            }
            Ty::Bool => {
                let (on_true, on_false) = (target(1), target(0));
                let mut yes = st.clone();
                yes.path.push(MirStep::If(c.clone(), true));
                let mut no = st;
                no.path.push(MirStep::If(c.clone(), false));
                let then = self.goto(on_true, yes)?;
                let otherwise = self.goto(on_false, no)?;
                Ok(Expr::If {
                    condition: Box::new(Expr::Flat(c)),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                })
            }
            // MIR prints a switch's values as their bit patterns.
            Ty::Nat(_) | Ty::Signed(_) => {
                // An `if` per value, in order, then `otherwise`.
                let mut rest = st.clone();
                let mut branches = Vec::new();
                for (v, b) in arms {
                    let test = format!("({c} =? {v})");
                    let mut yes = rest.clone();
                    yes.path.push(MirStep::If(test.clone(), true));
                    rest.path.push(MirStep::If(test.clone(), false));
                    branches.push((test, *b, yes));
                }
                let mut expr = self.goto(otherwise, rest)?;
                // Build from the inside out so each `if` holds the next.
                let mut built = Vec::new();
                for (test, b, yes) in branches {
                    built.push((test, self.goto(b, yes)?));
                }
                for (test, then) in built.into_iter().rev() {
                    expr = Expr::If {
                        condition: Box::new(Expr::Flat(test)),
                        then: Box::new(then),
                        otherwise: Box::new(expr),
                    };
                }
                Ok(expr)
            }
            _ => Err(kind(
                spec::MirTerminatorKind::SwitchInt.path(),
                &format!("a switch on {}", t.render()),
            )),
        }
    }

    /// A switch on a guarded option's discriminant: an `if` on the guard,
    /// whose `Some` arm reads the held value as the variant's field.
    fn guarded_switch(
        &mut self,
        op: &IrOperand,
        guard: &str,
        value: &str,
        arms: &[(i128, IrBlockId)],
        otherwise: IrBlockId,
        st: State,
    ) -> Lift<Expr> {
        let target = |v: i128| {
            arms.iter()
                .find(|(a, _)| *a == v)
                .map_or(otherwise, |(_, b)| *b)
        };
        let (IrOperand::Copy(p) | IrOperand::Move(p)) = op else {
            return Err(kind(spec::MirRvalue::Discriminant.path(), "of a constant"));
        };
        let Some(Val::Disc(place)) = st.env.get(&p.local).cloned() else {
            return Err(kind(spec::MirRvalue::Discriminant.path(), "of no place"));
        };
        let mut yes = st.clone();
        yes.path.push(MirStep::If(guard.to_string(), true));
        yes.bound.insert(
            (
                place_key(
                    &place,
                    self.mut_params.contains(&place.local) || st.lends.contains_key(&place.local),
                ),
                "Some".into(),
            ),
            vec![value.to_string()],
        );
        let mut no = st;
        no.path.push(MirStep::If(guard.to_string(), false));
        let then = self.goto(target(1), yes)?;
        let otherwise = self.goto(target(0), no)?;
        Ok(Expr::If {
            condition: Box::new(Expr::Flat(guard.to_string())),
            then: Box::new(then),
            otherwise: Box::new(otherwise),
        })
    }

    /// A switch on a discriminant: a `match` with one arm per constructor,
    /// in declaration order, which is the discriminant's value.
    fn match_on(
        &mut self,
        place: &IrPlace,
        arms: &[(i128, IrBlockId)],
        otherwise: IrBlockId,
        st: State,
    ) -> Lift<Expr> {
        let (scrutinee, t) = self.read(place, &st)?;
        let key = place_key(
            place,
            self.mut_params.contains(&place.local) || st.lends.contains_key(&place.local),
        );
        // An enum with one inhabited variant is that variant's field: the
        // switch takes that arm, whose field is the value itself.
        let printed = (place.projection.is_empty())
            .then(|| self.body.locals.get(place.local))
            .flatten()
            .and_then(|text| inhabited_variant(text));
        if let Some(only) = printed {
            let target = arms
                .iter()
                .find(|(a, _)| *a == only.discriminant)
                .map_or(otherwise, |(_, b)| *b);
            let mut arm = st;
            arm.bound
                .insert((key, only.variant.to_string()), vec![scrutinee]);
            return self.goto(target, arm);
        }
        let ctors = constructors(&t, self.cx.types, self.cx.explicit)?;
        let mut out = Vec::new();
        for (i, (variant, ctor, fields)) in ctors.iter().enumerate() {
            let target = arms
                .iter()
                .find(|(a, _)| *a == i as i128)
                .map_or(otherwise, |(_, b)| *b);
            let mut arm = st.clone();
            let mut names = Vec::new();
            for (j, ft) in fields.iter().enumerate() {
                let n = self.fresh(Some(&format!("{}{j}", variant.to_lowercase())), j);
                arm.binders.push((n.clone(), ft.clone()));
                names.push(n);
            }
            let pattern = std::iter::once(ctor.clone())
                .chain(names.iter().cloned())
                .collect::<Vec<String>>()
                .join(" ");
            let term = ctor_term(&t, ctor, &names);
            arm.path.push(MirStep::Match(scrutinee.clone(), term));
            arm.bound.insert((key.clone(), variant.clone()), names);
            let body = self.goto(target, arm)?;
            out.push(Arm { pattern, body });
        }
        self.mention(&t);
        Ok(Expr::Match {
            scrutinee,
            arms: out,
        })
    }

    fn statement(&mut self, s: &IrStatement, st: &mut State) -> Lift<()> {
        match s {
            IrStatement::Marker(_) | IrStatement::Assume(_) => Ok(()),
            IrStatement::Other(k, _) => Err(kind(k.path(), "")),
            IrStatement::Assign(p, rv) => {
                // A `&mut` writer coerced to `&mut dyn fmt::Write`: the
                // object is the text written through it, from none, and
                // lends the writer the value its `write_str` of that text
                // gives from the writer's value at the coercion (a text
                // written in pieces is written whole: the writer's
                // chunking theorem).
                if let (
                    true,
                    IrRvalue::Cast(
                        spec::MirCastKind::PointerCoercion,
                        IrOperand::Copy(q) | IrOperand::Move(q),
                        target,
                    ),
                ) = (p.projection.is_empty(), rv)
                {
                    let source = self
                        .body
                        .locals
                        .get(q.local)
                        .and_then(|t| mut_target(t))
                        .map(|t| t.trim().to_string());
                    if let (true, true, Some(source), Some(Val::Mut(place))) = (
                        q.projection.is_empty(),
                        target.trim() == "&mut dyn fmt::Write",
                        source.filter(|t| !t.starts_with("dyn ") && t != "String" && t != "str"),
                        st.env.get(&q.local).cloned(),
                    ) {
                        let k = spec::MirCastKind::PointerCoercion.path();
                        let (snapshot, wt) = self.read(&place, st)?;
                        let to = self.local_ty(p.local, &[])?;
                        let hole = self.fresh(Some("text"), p.local);
                        // Spelled in halves: the result type as MIR prints it, which the
                        // workspace's scan for fallible signatures (R5) would read as one.
                        let rt_text = concat!("Result", "<(), fmt::Error>");
                        let rt = ty(rt_text, self.cx.types)?;
                        let called = self.call(
                            &format!("<{source} as fmt::Write>::write_str"),
                            vec![(snapshot, wt), (hole.clone(), to.clone())],
                            &[Some(format!("&mut {source}")), Some("&str".into())],
                            rt_text,
                            &rt,
                            st,
                        )?;
                        let prefix = called
                            .term
                            .strip_prefix('(')
                            .and_then(|t| t.strip_suffix(&format!(" {hole})")))
                            .map(str::to_string);
                        let (Some(prefix), Some((tt, positions))) = (prefix, called.threads) else {
                            return Err(kind(
                                k,
                                &format!("from {source} to a writer, whose write_str gives no writer back"),
                            ));
                        };
                        if positions != [0] || !called.updates.is_empty() || called.memory {
                            return Err(kind(
                                k,
                                &format!("from {source} to a writer, whose write_str gives no writer back"),
                            ));
                        }
                        let empty = lift::default_of(&to, self.cx.types)
                            .ok_or_else(|| kind(k, "to a writer with no empty text"))?;
                        st.env.insert(p.local, Val::V(empty, to));
                        st.lends.insert(
                            p.local,
                            vec![(Vec::new(), place, Some((prefix, Some((tt, 1)))))],
                        );
                        return Ok(());
                    }
                }
                if p.projection.is_empty() {
                    match rv {
                        IrRvalue::Ref(spec::MirBorrowKind::Shared, q) => {
                            st.shared.insert(p.local, q.clone());
                        }
                        _ => {
                            st.shared.remove(&p.local);
                        }
                    }
                }
                if p.projection.is_empty() && !st.placed.contains_key(&p.local) {
                    let v = self.rvalue(rv, p, st)?;
                    // The borrows the new value holds: each mutable borrow
                    // it is built of (a borrow, a `&mut` parameter, a value
                    // holding borrows, or a parameter threaded as one), at
                    // the part of the value that holds it.
                    let mut lends = Vec::new();
                    let ops: Vec<(Vec<IrProjection>, &IrOperand)> = match rv {
                        IrRvalue::Use(o) => vec![(Vec::new(), o)],
                        IrRvalue::Aggregate(
                            IrAggregate::Adt(_, None) | IrAggregate::Tuple,
                            ops,
                        ) => ops
                            .iter()
                            .enumerate()
                            .map(|(j, (_, o))| {
                                let text = match o {
                                    IrOperand::Copy(q) | IrOperand::Move(q) => {
                                        self.body.locals.get(q.local).cloned().unwrap_or_default()
                                    }
                                    IrOperand::Constant(_) | IrOperand::Check(_) => String::new(),
                                };
                                (vec![IrProjection::Field(j, text)], o)
                            })
                            .collect(),
                        // `Some` of a borrow, in a backward function: the
                        // variant's field lends it.
                        IrRvalue::Aggregate(IrAggregate::Adt(_, Some(variant)), ops)
                            if self.backward.is_some()
                                && variant == "Some"
                                && matches!(self.local_ty(p.local, &[]), Ok(Ty::Option(_))) =>
                        {
                            ops.iter()
                                .map(|(_, o)| {
                                    let text = match o {
                                        IrOperand::Copy(q) | IrOperand::Move(q) => self
                                            .body
                                            .locals
                                            .get(q.local)
                                            .cloned()
                                            .unwrap_or_default(),
                                        IrOperand::Constant(_) | IrOperand::Check(_) => {
                                            String::new()
                                        }
                                    };
                                    (
                                        vec![
                                            IrProjection::Downcast(variant.clone()),
                                            IrProjection::Field(0, text),
                                        ],
                                        o,
                                    )
                                })
                                .collect()
                        }
                        IrRvalue::Aggregate(IrAggregate::Closure(ct), ops) => {
                            let upvars = self
                                .cx
                                .closures
                                .get(ct)
                                .map(|b| b.upvars.clone())
                                .unwrap_or_default();
                            let one = upvars.len() == 1;
                            upvars
                                .iter()
                                .enumerate()
                                .filter_map(|(j, up)| {
                                    let (n, text) = up.as_ref()?;
                                    let o = ops
                                        .iter()
                                        .find(|(f, _)| f.as_deref() == Some(n.as_str()))
                                        .or_else(|| ops.get(j))
                                        .map(|(_, o)| o)?;
                                    let path = if one {
                                        Vec::new()
                                    } else {
                                        vec![IrProjection::Field(j, text.clone())]
                                    };
                                    Some((path, o))
                                })
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    for (path, o) in ops {
                        let (IrOperand::Copy(q) | IrOperand::Move(q)) = o else {
                            continue;
                        };
                        if !q.projection.is_empty() {
                            continue;
                        }
                        let whole = IrPlace {
                            local: q.local,
                            projection: Vec::new(),
                        };
                        match st.env.get(&q.local) {
                            Some(Val::Mut(place)) => lends.push((path, place.clone(), None)),
                            _ if self.mut_params.contains(&q.local) => {
                                lends.push((path, whole, None))
                            }
                            _ => {
                                for (inner, place, back) in
                                    st.lends.get(&q.local).cloned().unwrap_or_default()
                                {
                                    let mut at = path.clone();
                                    at.extend(inner);
                                    lends.push((at, place, back));
                                }
                            }
                        }
                    }
                    // A borrow moved or copied whole stays the borrow it is.
                    if matches!(&v, Val::Mut(_)) {
                        lends.clear();
                    }
                    st.lends.remove(&p.local);
                    if !lends.is_empty() {
                        st.lends.insert(p.local, lends);
                    }
                    return self.write(p, v, st);
                }
                // A write into a place: the rvalue at the place's type, as
                // a functional update of the local that holds the place.
                let v = self.rvalue(rv, p, st)?;
                let v = match v {
                    // An overflow-checked pair written whole is a tuple.
                    Val::Checked(value, overflow, t) => {
                        let pair = self.cx.types.tuple(vec![t, Ty::Bool]);
                        self.mention(&pair);
                        Val::V(lift::tuple_value(&pair, &[value, overflow]), pair)
                    }
                    other => other,
                };
                self.write(p, v, st)?;
                // In a backward function, a borrow written into a place
                // (`*self = Some(value)`, `value` holding borrows) lends
                // from there: the returned borrow's new value reaches it.
                if self.backward.is_some() {
                    let (path, o) = match rv {
                        IrRvalue::Use(o) => (Vec::new(), o),
                        IrRvalue::Aggregate(IrAggregate::Adt(_, Some(variant)), ops)
                            if variant == "Some" && ops.len() == 1 =>
                        {
                            let text = match &ops[0].1 {
                                IrOperand::Copy(q) | IrOperand::Move(q) => {
                                    self.body.locals.get(q.local).cloned().unwrap_or_default()
                                }
                                IrOperand::Constant(_) | IrOperand::Check(_) => String::new(),
                            };
                            (
                                vec![
                                    IrProjection::Downcast(variant.clone()),
                                    IrProjection::Field(0, text),
                                ],
                                &ops[0].1,
                            )
                        }
                        _ => return Ok(()),
                    };
                    let (IrOperand::Copy(q) | IrOperand::Move(q)) = o else {
                        return Ok(());
                    };
                    if !q.projection.is_empty() {
                        return Ok(());
                    }
                    let held =
                        self.mut_params.contains(&p.local) || st.lends.contains_key(&p.local);
                    let mut at: Vec<IrProjection> = p
                        .projection
                        .iter()
                        .filter(|s| !(held && matches!(s, IrProjection::Deref)))
                        .cloned()
                        .collect();
                    at.extend(path);
                    let mut lends = Vec::new();
                    match st.env.get(&q.local) {
                        Some(Val::Mut(place)) => lends.push((at, place.clone(), None)),
                        _ if self.mut_params.contains(&q.local) => lends.push((
                            at,
                            IrPlace {
                                local: q.local,
                                projection: Vec::new(),
                            },
                            None,
                        )),
                        _ => {
                            for (inner, place, back) in
                                st.lends.get(&q.local).cloned().unwrap_or_default()
                            {
                                let mut part = at.clone();
                                part.extend(inner);
                                lends.push((part, place, back));
                            }
                        }
                    }
                    st.lends.entry(p.local).or_default().extend(lends);
                }
                Ok(())
            }
        }
    }

    fn operand(&mut self, op: &IrOperand, st: &State) -> Lift<(String, Ty)> {
        match op {
            IrOperand::Copy(p) | IrOperand::Move(p) => self.read(p, st),
            IrOperand::Constant(c) => self.constant(c),
            IrOperand::Check(spec::MirRuntimeChecks::OverflowChecks) => {
                Ok(("true".into(), Ty::Bool))
            }
            IrOperand::Check(k) => Err(kind(k.path(), "")),
        }
    }

    fn constant(&mut self, c: &IrConstant) -> Lift<(String, Ty)> {
        let k = spec::MirOperand::Constant.path();
        match c {
            IrConstant::Bool(b) => Ok((b.to_string(), Ty::Bool)),
            IrConstant::Unit => Ok(("()".into(), Ty::Unit)),
            IrConstant::Int(n, suffix) => {
                let t = ty(suffix, self.cx.types)?;
                match t {
                    Ty::Int(_) => Ok((format!("({n} : Int64)"), t)),
                    _ => Int::of(&t, self.cx.widths)
                        .and_then(|i| super::int::literal(*n, i))
                        .map(|v| (v, t.clone()))
                        .ok_or_else(|| kind(k, &format!("a {suffix} literal"))),
                }
            }
            IrConstant::Str(s) => lift::exact_string(s)
                .map(|s| (s, Ty::Str))
                .ok_or_else(|| kind(k, "a string LexLean source cannot write")),
            IrConstant::Char(ch) => lift::exact_string(&ch.to_string())
                .map(|s| (s, Ty::Char))
                .ok_or_else(|| kind(k, "a char LexLean source cannot write")),
            // A const parameter's value, bound to `#` where the census
            // instantiates the body for every length at once.
            IrConstant::Other(p) if p.starts_with('#') => self
                .const_params
                .get(p)
                .cloned()
                .ok_or_else(|| kind(k, "a const parameter's value")),
            // A byte string, as MIR prints it with Rust's escapes: the list
            // of its bytes.
            IrConstant::Other(p) if p.starts_with("b\"") && p.ends_with('"') && p.len() >= 3 => {
                let inner = &p[2..p.len() - 1];
                let mut bytes: Vec<u8> = Vec::new();
                let mut chars = inner.chars();
                while let Some(c) = chars.next() {
                    if c != '\\' {
                        let mut buf = [0u8; 4];
                        bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        continue;
                    }
                    let b = match chars.next() {
                        Some('n') => b'\n',
                        Some('t') => b'\t',
                        Some('r') => b'\r',
                        Some('0') => 0,
                        Some('\\') => b'\\',
                        Some('"') => b'"',
                        Some('\'') => b'\'',
                        Some('x') => {
                            let hex: String = chars.by_ref().take(2).collect();
                            u8::from_str_radix(&hex, 16)
                                .map_err(|_| kind(k, &format!("a byte string escape \\x{hex}")))?
                        }
                        other => return Err(kind(k, &format!("a byte string escape \\{other:?}"))),
                    };
                    bytes.push(b);
                }
                let mut out = "(nil Nat)".to_string();
                for b in bytes.iter().rev() {
                    out = format!("(cons {b} {out})");
                }
                Ok((out, Ty::List(Box::new(Ty::Nat(Some(8))))))
            }
            // A constant cast between integers, `const u32::MAX as u128
            // (IntToInt)` or `const 1_i32 as u32 (IntToInt)`: the constant,
            // by its own rule, cast by the cast's.
            IrConstant::Path(p) | IrConstant::Other(p)
                if p.strip_suffix(" (IntToInt)")
                    .is_some_and(|q| q.contains(" as ")) =>
            {
                let (inner, to) = p
                    .strip_suffix(" (IntToInt)")
                    .and_then(|q| q.rsplit_once(" as "))
                    .ok_or_else(|| kind(k, p))?;
                let inner = match inner.rsplit_once('_') {
                    Some((n, suffix)) if n.parse::<i128>().is_ok() => {
                        IrConstant::Int(n.parse().unwrap_or_default(), suffix.to_string())
                    }
                    _ => IrConstant::Path(inner.to_string()),
                };
                let (v, from) = self.constant(&inner)?;
                let to = ty(to, self.cx.types)?;
                let w = self.cx.widths;
                let (Some(fi), Some(ti)) = (Int::of(&from, w), Int::of(&to, w)) else {
                    return Err(kind(k, p));
                };
                match super::int::cast(&v, fi, ti, from == to) {
                    (value, None) => Ok((value, to)),
                    (_, Some(_)) => Err(kind(
                        k,
                        &format!("{p}, a cast whose range differs between the declared targets"),
                    )),
                }
            }
            // A float literal as MIR prints it (`0f32`, `-0f64`, `1.0E+4f16`,
            // `+Inf_f16`), or a constant IEEE 754 fixes, by core's name for
            // it (`f32::<impl f32>::NAN`).
            IrConstant::Path(p) | IrConstant::Other(p)
                if self.cx.types.floats.iter().any(|(t, f)| {
                    let lit = p
                        .strip_suffix(t.as_str())
                        .map(|l| l.strip_suffix('_').unwrap_or(l));
                    let named = p.strip_prefix(&format!("{t}::<impl {t}>::"));
                    lit.or(named).and_then(|l| float::literal(*f, l)).is_some()
                }) =>
            {
                let floats = &self.cx.types.floats;
                floats
                    .iter()
                    .find_map(|(t, f)| {
                        let lit = p
                            .strip_suffix(t.as_str())
                            .map(|l| l.strip_suffix('_').unwrap_or(l));
                        let named = p.strip_prefix(&format!("{t}::<impl {t}>::"));
                        let term = lit.or(named).and_then(|l| float::literal(*f, l))?;
                        Some((term, Ty::Float(*f)))
                    })
                    .ok_or_else(|| kind(k, p))
            }
            IrConstant::Path(p) | IrConstant::Other(p) => match self.cx.consts.get(p) {
                // A primitive integer's `BITS`, `MAX` or `MIN`: `num::<impl
                // u32>::MAX`, or a module's `i8::MIN`, from its own rule.
                None if p.rsplit_once("::").is_some_and(|(o, _)| {
                    let o = o
                        .strip_prefix("num::<impl ")
                        .and_then(|o| o.strip_suffix('>'))
                        .unwrap_or(o);
                    ty(o, self.cx.types).is_ok_and(|t| Int::of(&t, self.cx.widths).is_some())
                }) =>
                {
                    let (o, name) = p.rsplit_once("::").unwrap_or_default();
                    let o = o
                        .strip_prefix("num::<impl ")
                        .and_then(|o| o.strip_suffix('>'))
                        .unwrap_or(o);
                    let t = ty(o, self.cx.types)?;
                    let int = Int::of(&t, self.cx.widths).ok_or_else(|| kind(k, p))?;
                    match super::int::assoc(name, &t, int) {
                        Some(super::int::Assoc::Held(v, t)) => Ok((v, t)),
                        Some(super::int::Assoc::Varies) => Err(kind(
                            k,
                            &format!("{o}::{name}, which differs between the declared targets"),
                        )),
                        None => Err(kind(k, &format!("the constant item {p}"))),
                    }
                }
                Some(body) => self.const_body(p, body),
                // In core's scope, a constant a trait of core provides.
                None if self.cx.core.and_then(|l| l.constant(p)).is_some() => {
                    let inst = self.cx.core.and_then(|l| l.constant(p)).expect("checked");
                    let body = super::parse::bodies(&inst.text)
                        .into_iter()
                        .next()
                        .ok_or_else(|| {
                            kind(
                                k,
                                &format!("the constant item {p}, whose body does not read"),
                            )
                        })?;
                    self.const_body(p, &body)
                }
                None => match c {
                    IrConstant::Path(p) => Err(kind(k, &format!("the constant item {p}"))),
                    _ => Err(kind(k, p)),
                },
            },
        }
    }

    /// A constant's own body (a promoted constant or a constant item),
    /// lowered like any body: its value inline where it is one term, else
    /// a definition of its own.
    fn const_body(&mut self, path: &str, body: &IrBody) -> Lift<(String, Ty)> {
        let k = spec::MirOperand::Constant.path();
        let segs: Vec<String> = super::parse::split_path(path)
            .iter()
            .map(|s| s.replace(['[', ']'], " "))
            .collect();
        let parts: Vec<&str> = std::iter::once("const")
            .chain(std::iter::once(self.name.as_str()))
            .chain(segs.iter().map(String::as_str))
            .collect();
        let name = crate::lex::camel(&parts);
        if self.cx.active.contains(&name) {
            return Err(kind(k, &format!("{path}, which refers to itself")));
        }
        let mut active = self.cx.active.to_vec();
        active.push(name.clone());
        let cx = self.cx.within(&active);
        let lowered =
            lower(&cx, body, &name).map_err(|r| kind(k, &format!("{path}: {}", r.detail())))?;
        let t = ty(&body.locals[0], self.cx.types)?;
        self.types.extend(lowered.types.iter().cloned());
        self.calls.extend(lowered.calls.iter().cloned());
        if let (
            [Decl::Definition {
                body: Expr::Flat(v),
                ..
            }],
            true,
        ) = (lowered.decls.as_slice(), lowered.aux.is_empty())
        {
            return Ok((v.clone(), t));
        }
        for d in lowered.decls {
            self.aux.insert(d.name().to_string(), d);
        }
        for (n, d) in lowered.aux {
            self.aux.entry(n).or_insert(d);
        }
        self.calls.insert(name.clone());
        Ok((name, t))
    }

    /// The value at a place.
    fn read(&mut self, p: &IrPlace, st: &State) -> Lift<(String, Ty)> {
        if let Some((view, steps)) = self.view_of(p, st)? {
            return self.load(&view, &steps, st);
        }
        let base = st.env.get(&p.local).cloned().ok_or_else(|| {
            kind(
                spec::MirOperand::Copy.path(),
                "a local read before it is written",
            )
        })?;
        let mut steps = p.projection.iter().peekable();
        let mut prefix = IrPlace {
            local: p.local,
            projection: Vec::new(),
        };
        let (mut v, mut t) = match base {
            Val::V(v, t) => (v, t),
            // Read whole, a guarded option is the `if` it stands for.
            Val::Guarded { guard, value, item } if p.projection.is_empty() => {
                let i = item.render();
                return Ok((
                    format!(
                        "(if {guard} then (ctor Option.some [{i}] {value}) else (ctor Option.none [{i}]))"
                    ),
                    Ty::Option(Box::new(item)),
                ));
            }
            Val::Guarded { item, .. } => match p.projection.first() {
                Some(IrProjection::Downcast(variant)) => {
                    let key = (
                        place_key(
                            &prefix,
                            self.mut_params.contains(&prefix.local)
                                || st.lends.contains_key(&prefix.local),
                        ),
                        variant.clone(),
                    );
                    let Some(names) = st.bound.get(&key).cloned() else {
                        return Err(kind(
                            spec::MirProjectionElem::Downcast.path(),
                            "a variant's field read outside a match on it",
                        ));
                    };
                    match p.projection.get(1..) {
                        Some([IrProjection::Field(0, _), rest @ ..]) if names.len() == 1 => {
                            let _ = rest;
                            steps.next();
                            steps.next();
                            prefix.projection.extend(p.projection[..2].iter().cloned());
                            (names[0].clone(), item)
                        }
                        _ => {
                            return Err(kind(
                                spec::MirProjectionElem::Downcast.path(),
                                "a variant read whole",
                            ))
                        }
                    }
                }
                _ => {
                    return Err(kind(
                        spec::MirProjectionElem::Field.path(),
                        "a field of an Option",
                    ))
                }
            },
            Val::Checked(value, overflow, t) => match p.projection.as_slice() {
                [IrProjection::Field(0, _)] => return Ok((value, t)),
                [IrProjection::Field(1, _)] => return Ok((overflow, Ty::Bool)),
                _ => {
                    return Err(kind(
                        spec::MirBinOp::AddWithOverflow.path(),
                        "a checked pair read whole",
                    ))
                }
            },
            Val::Disc(_) => {
                return Err(kind(
                    spec::MirRvalue::Discriminant.path(),
                    "a discriminant read as a value",
                ))
            }
            // A capture of a closure's environment: the environment itself
            // where it is the one capture, else that item of it.
            Val::Env(env, et, captures) => {
                // Read whole, as a threading closure gives it back.
                if matches!(p.projection.as_slice(), [] | [IrProjection::Deref]) {
                    return Ok((env, et));
                }
                let mut rest = p.projection.as_slice();
                if let Some((IrProjection::Deref, r)) = rest.split_first() {
                    rest = r;
                }
                let Some((IrProjection::Field(k, _), rest)) = rest.split_first() else {
                    return Err(kind(
                        spec::MirAggregateKind::Closure.path(),
                        "a closure's environment read whole",
                    ));
                };
                let t = captures.get(*k).cloned().ok_or_else(|| {
                    kind(
                        spec::MirAggregateKind::Closure.path(),
                        "a capture past the environment's",
                    )
                })?;
                let n = if captures.len() == 1 {
                    env.clone()
                } else {
                    lift::tuple_item(&et, &env, *k)
                };
                // The capture as a local of its own, read on through the
                // rest of the place.
                let id = self.body.locals.len() + 1 + *k;
                let mut inner = st.clone();
                inner.env.insert(id, Val::V(n, t));
                let through = IrPlace {
                    local: id,
                    projection: rest.to_vec(),
                };
                return self.read(&through, &inner);
            }
            // Through a mutable borrow: the borrowed place's current value.
            Val::Mut(q) => match p.projection.split_first() {
                Some((IrProjection::Deref, rest)) => {
                    let mut through = q.clone();
                    through.projection.extend(rest.iter().cloned());
                    return self.read(&through, st);
                }
                // Read whole, a borrow is the value of the place it borrows.
                None => return self.read(&q, st),
                _ => {
                    return Err(kind(
                        spec::MirRvalue::Ref.path(),
                        "a mutable borrow used other than by a call or a read through it",
                    ))
                }
            },
        };
        // The Rust type of the place read so far, where MIR names it: a
        // tuple of one item lifts as the item, so its field is the value.
        let mut text: Option<String> = self.body.locals.get(p.local).cloned();
        while let Some(step) = steps.next() {
            match step {
                // A shared reference or a box is the value it points to.
                IrProjection::Deref
                    if is_pointer(&t) && text.as_deref().is_none_or(|x| referent(x).is_none()) =>
                {
                    return Err(through_pointer("read"))
                }
                IrProjection::Deref => text = text.as_deref().and_then(referent),
                IrProjection::Field(0, fty) if text.as_deref().is_some_and(one_tuple) => {
                    t = ty(fty, self.cx.types)?;
                    text = Some(fty.clone());
                }
                // A `MaybeUninit`'s value, read as a `T`: it must hold one.
                IrProjection::Field(1, fty)
                    if matches!(t, Ty::Option(_))
                        && wrapper(fty).is_some_and(|(n, _)| n == "ManuallyDrop") =>
                {
                    let Ty::Option(inner) = t.clone() else {
                        unreachable!()
                    };
                    let d = lift::default_of(&inner, self.cx.types).ok_or_else(|| {
                        kind(MEMORY_KIND, "a MaybeUninit of a type with no value")
                    })?;
                    self.obligation(
                        st,
                        &format!("((match {v} | Option.none => false | Option.some held => true) = true)"),
                        "an initialized MaybeUninit",
                    );
                    v = format!("(match {v} | Option.none => {d} | Option.some held => held)");
                    t = *inner;
                    text = None;
                }
                // Its `uninit` field, the unit.
                IrProjection::Field(0, fty) if matches!(t, Ty::Option(_)) && fty.trim() == "()" => {
                    v = "()".into();
                    t = Ty::Unit;
                    text = Some(fty.clone());
                }
                // `ManuallyDrop`'s field, the value it wraps; `NonNull`'s and
                // `Unique`'s, the pointer they are (a raw pointer has no
                // fields of its own).
                IrProjection::Field(0, fty)
                    if (is_pointer(&t)
                        || !matches!(t, Ty::Named(_) | Ty::Tuple(_) | Ty::Pair(_)))
                        && ty(fty, self.cx.types).ok().as_ref() == Some(&t) =>
                {
                    text = Some(fty.clone());
                }
                IrProjection::Field(i, fty) => {
                    let ft = ty(fty, self.cx.types)?;
                    v = self.field(&v, &t, *i)?;
                    t = ft;
                    text = Some(fty.clone());
                }
                IrProjection::Downcast(variant) => {
                    // `ControlFlow`'s variants are `Result`'s.
                    let variant = match variant.as_str() {
                        "Continue" if matches!(t, Ty::Outcome(..)) => "Ok".to_string(),
                        "Break" if matches!(t, Ty::Outcome(..)) => "Err".to_string(),
                        v => v.to_string(),
                    };
                    let key = (
                        place_key(
                            &prefix,
                            self.mut_params.contains(&prefix.local)
                                || st.lends.contains_key(&prefix.local),
                        ),
                        variant.clone(),
                    );
                    // The one inhabited variant of an enum is its value,
                    // matched or not.
                    let only = (prefix.projection.is_empty())
                        .then(|| self.body.locals.get(prefix.local))
                        .flatten()
                        .and_then(|text| inhabited_variant(text))
                        .filter(|o| o.variant == variant && o.field.is_some())
                        .map(|_| vec![v.clone()]);
                    let Some(names) = st.bound.get(&key).cloned().or(only) else {
                        return Err(kind(
                            spec::MirProjectionElem::Downcast.path(),
                            "a variant's field read outside a match on it",
                        ));
                    };
                    match steps.next() {
                        Some(IrProjection::Field(i, fty)) => {
                            let ft = ty(fty, self.cx.types)?;
                            v = names.get(*i).cloned().ok_or_else(|| {
                                kind(
                                    spec::MirProjectionElem::Field.path(),
                                    "a field past the variant's",
                                )
                            })?;
                            t = ft;
                            prefix.projection.push(step.clone());
                            prefix.projection.push(IrProjection::Field(*i, fty.clone()));
                            continue;
                        }
                        _ => {
                            return Err(kind(
                                spec::MirProjectionElem::Downcast.path(),
                                "a variant read whole",
                            ))
                        }
                    }
                }
                IrProjection::Index(l) => {
                    let i = match st.env.get(l) {
                        Some(Val::V(i, Ty::Nat(_))) => i.clone(),
                        _ => {
                            return Err(kind(
                                spec::MirProjectionElem::Index.path(),
                                "an index that is not a natural",
                            ))
                        }
                    };
                    let Ty::List(elem) = t.clone() else {
                        return Err(kind(
                            spec::MirProjectionElem::Index.path(),
                            &format!("an index into {}", t.render()),
                        ));
                    };
                    let f = self.index_aux(&elem, false)?;
                    v = format!("({f} {v} {i})");
                    t = *elem;
                    if let Some(b) = fixed(&t) {
                        self.ranged.insert(v.clone(), b);
                    }
                }
                // `[n of m]` and `[-n of m]`, which a slice pattern reads
                // after its match has checked the length: the item `n` from
                // the start, or from the end.
                IrProjection::ConstantIndex(n, from_end) => {
                    let Ty::List(elem) = t.clone() else {
                        return Err(kind(
                            spec::MirProjectionElem::ConstantIndex.path(),
                            &format!("an index into {}", t.render()),
                        ));
                    };
                    let i = if *from_end {
                        format!("(prim subtract (prim length {v} : Nat) {n} : Nat)")
                    } else {
                        n.to_string()
                    };
                    let f = self.index_aux(&elem, false)?;
                    v = format!("({f} {v} {i})");
                    t = *elem;
                    if let Some(b) = fixed(&t) {
                        self.ranged.insert(v.clone(), b);
                    }
                }
                IrProjection::Other(k, _) => return Err(kind(k.path(), "")),
            }
            prefix.projection.push(step.clone());
        }
        Ok((v, t))
    }

    /// The `i`-th field of a struct or tuple value.
    fn field(&mut self, v: &str, t: &Ty, i: usize) -> Lift<String> {
        let k = spec::MirProjectionElem::Field.path();
        match t {
            Ty::Tuple(ts) | Ty::Pair(ts) if i < ts.len() => Ok(lift::tuple_item(t, v, i)),
            Ty::Named(n) => {
                let fields =
                    self.cx.types.structs.get(n).ok_or_else(|| {
                        kind(k, &format!("a field of {n}, which is not a struct"))
                    })?;
                let (f, _) = fields
                    .get(i)
                    .ok_or_else(|| kind(k, "a field past the struct's"))?;
                let read = if self.cx.types.boxed(n) {
                    format!("({} {v})", Types::accessor(n, f))
                } else {
                    format!("(project {v} {f})")
                };
                // What a pattern-typed field holds is within its pattern.
                if let Some(facts) = self.valid_facts(n, &read) {
                    self.admitted.insert(read.clone(), facts);
                }
                Ok(read)
            }
            _ => Err(kind(k, &format!("a field of {}", t.render()))),
        }
    }

    /// The definition reading a list at an index, which an `assert` before
    /// it proves in bounds: the item there, or a value of its type where the
    /// index is past the end (never, by that theorem). With `write`, the
    /// definition writing an item, by structural recursion on the list: the
    /// list with the item at the index replaced, unchanged where the index
    /// is past the end.
    fn index_aux(&mut self, elem: &Ty, write: bool) -> Lift<String> {
        if write {
            let name = format!("mirUpdate{}", elem.tag());
            if !self.aux.contains_key(&name) {
                let e = elem.render();
                self.aux.insert(
                    name.clone(),
                    Decl::Definition {
                        name: name.clone(),
                        parameters: vec![
                            ("items".into(), format!("(List {e})")),
                            ("index".into(), "Nat".into()),
                            ("value".into(), e.clone()),
                        ],
                        result: format!("(List {e})"),
                        body: Expr::Match {
                            scrutinee: "items".into(),
                            arms: vec![
                                Arm {
                                    pattern: "List.nil".into(),
                                    body: Expr::Flat(format!("(nil {e})")),
                                },
                                Arm {
                                    pattern: "List.cons head rest".into(),
                                    body: Expr::If {
                                        condition: Box::new(Expr::Flat("(index =? 0)".into())),
                                        then: Box::new(Expr::Flat("(cons value rest)".into())),
                                        otherwise: Box::new(Expr::Flat(format!(
                                            "(cons head ({name} rest (prim subtract index 1 : Nat) value))"
                                        ))),
                                    },
                                },
                            ],
                        },
                        axioms: Vec::new(),
                        recursive: Some("items".into()),
                        termination: None,
                        mutual: None,
                    },
                );
            }
            self.mention(elem);
            self.calls.insert(name.clone());
            return Ok(name);
        }
        let name = format!("mirIndex{}", elem.tag());
        if !self.aux.contains_key(&name) {
            let d = lift::default_of(elem, self.cx.types).ok_or_else(|| {
                kind(
                    spec::MirProjectionElem::Index.path(),
                    &format!("an index into a list of {}, which has no value to stand for a missing item", elem.render()),
                )
            })?;
            let e = elem.render();
            self.aux.insert(
                name.clone(),
                Decl::Definition {
                    name: name.clone(),
                    parameters: vec![
                        ("items".into(), format!("(List {e})")),
                        ("index".into(), "Nat".into()),
                    ],
                    result: e.clone(),
                    body: Expr::Match {
                        scrutinee: format!("(prim index items index : (Option {e}))"),
                        arms: vec![
                            Arm {
                                pattern: "Option.none".into(),
                                body: Expr::Flat(d),
                            },
                            Arm {
                                pattern: "Option.some item".into(),
                                body: Expr::Flat("item".into()),
                            },
                        ],
                    },
                    axioms: Vec::new(),
                    recursive: None,
                    termination: None,
                    mutual: None,
                },
            );
        }
        self.mention(elem);
        self.calls.insert(name.clone());
        Ok(name)
    }

    fn rvalue(&mut self, rv: &IrRvalue, dest: &IrPlace, st: &mut State) -> Lift<Val> {
        // A `&mut` read out of a place into a `&mut` local (`deref_copy
        // (*_1)` of a `&mut &mut T`) is a reborrow of the place it points
        // to, where the translation knows that place.
        if let IrRvalue::Use(IrOperand::Copy(q) | IrOperand::Move(q)) | IrRvalue::CopyForDeref(q) =
            rv
        {
            let into_mut = dest.projection.is_empty()
                && self
                    .body
                    .locals
                    .get(dest.local)
                    .and_then(|t| mut_target(t))
                    .is_some();
            if into_mut && !q.projection.is_empty() {
                let mut through = q.clone();
                through.projection.push(IrProjection::Deref);
                if let Ok(place) = self.borrowed(&through, st) {
                    return Ok(Val::Mut(place));
                }
            }
        }
        match rv {
            // A mutable borrow moved or copied whole is the same borrow, and
            // so is one coerced to a type the lift holds alike (`&mut
            // String` to `&mut dyn fmt::Write`, an unsizing coercion).
            IrRvalue::Use(IrOperand::Copy(q) | IrOperand::Move(q))
                if q.projection.is_empty() && matches!(st.env.get(&q.local), Some(Val::Mut(_))) =>
            {
                Ok(st.env[&q.local].clone())
            }
            IrRvalue::Cast(
                spec::MirCastKind::PointerCoercion,
                IrOperand::Copy(q) | IrOperand::Move(q),
                _,
            ) if q.projection.is_empty()
                && matches!(st.env.get(&q.local), Some(Val::Mut(_)))
                && self.local_ty(q.local, &[])?.render()
                    == self.local_ty(dest.local, &dest.projection)?.render() =>
            {
                Ok(st.env[&q.local].clone())
            }
            IrRvalue::Use(o) => {
                let (v, t) = self.operand(o, st)?;
                Ok(Val::V(v, t))
            }
            IrRvalue::Ref(spec::MirBorrowKind::Shared | spec::MirBorrowKind::Fake, p)
            | IrRvalue::CopyForDeref(p) => {
                let (v, t) = self.read(p, st)?;
                Ok(Val::V(v, t))
            }
            IrRvalue::Ref(spec::MirBorrowKind::Mut, p) => self.borrowed(p, st).map(Val::Mut),
            IrRvalue::Discriminant(p) => Ok(Val::Disc(p.clone())),
            IrRvalue::Unary(op, o) => {
                let (v, t) = self.operand(o, st)?;
                match (op, &t) {
                    // A constant's negation is the other constant.
                    (spec::MirUnOp::Not, Ty::Bool) if v == "true" || v == "false" => {
                        Ok(Val::V((v == "false").to_string(), Ty::Bool))
                    }
                    (spec::MirUnOp::Not, Ty::Bool) => Ok(Val::V(format!("(not {v})"), Ty::Bool)),
                    (spec::MirUnOp::Neg, Ty::Float(f)) => {
                        Ok(Val::V(f.term("Neg", &[&v]), t.clone()))
                    }
                    // A raw pointer's metadata: a slice's length, or the
                    // unit of a thin pointer.
                    (spec::MirUnOp::PtrMetadata, t) if is_pointer(t) => {
                        let mt = self.local_ty(dest.local, &dest.projection)?;
                        match mt {
                            Ty::Unit => Ok(Val::V("()".into(), Ty::Unit)),
                            Ty::Nat(None) => {
                                Ok(Val::V(format!("(project {v} metadata)"), Ty::Nat(None)))
                            }
                            _ => Err(kind(op.path(), &format!("of {}", mt.render()))),
                        }
                    }
                    // `!x` is `2^w - 1 - x` at the width the value is held
                    // at, and `-x` is `2^w - x` wrapped (MIR asserts that
                    // `x` is not `MIN` first): a signed value's at any width,
                    // an unsigned one's where the width is the same on every
                    // declared target.
                    (spec::MirUnOp::Not | spec::MirUnOp::Neg, Ty::Nat(_) | Ty::Signed(_)) => {
                        let int = Int::of(&t, self.cx.widths)
                            .filter(|i| i.signed || i.width.is_some())
                            .filter(|i| i.signed || *op == spec::MirUnOp::Not)
                            .ok_or_else(|| kind(op.path(), &format!("on {}", t.render())))?;
                        let m = super::int::pow2(int.held);
                        Ok(Val::V(
                            if *op == spec::MirUnOp::Not {
                                format!("(prim subtract {m} ({v} + 1) : Nat)")
                            } else {
                                format!(
                                    "(prim remainder (prim subtract {m} {v} : Nat) {m} 0 : Nat)"
                                )
                            },
                            t,
                        ))
                    }
                    (spec::MirUnOp::PtrMetadata, Ty::List(elem)) => {
                        let len = format!("(prim length {v} : Nat)");
                        self.length_fact(&len, elem, st);
                        Ok(Val::V(len, Ty::Nat(None)))
                    }
                    _ => Err(kind(op.path(), &format!("on {}", t.render()))),
                }
            }
            IrRvalue::Binary(op, a, b) => self.binary(*op, a, b, st),
            IrRvalue::Aggregate(IrAggregate::Closure(ct), ops) => self.closure_value(ct, ops, st),
            IrRvalue::Aggregate(agg, ops) => {
                // A mutable borrow of a writer held in a value (a
                // `Formatter` built of the `&mut dyn Write` it is given, a
                // builder of the `&mut Formatter`): the lift holds the
                // value it borrows, so a write through the held borrow
                // would not reach the place it borrows.
                // A struct or a tuple built whole into a local holds the
                // borrow, which the local's writes write back.
                let held = dest.projection.is_empty()
                    && matches!(agg, IrAggregate::Adt(_, None) | IrAggregate::Tuple);
                if !held
                    && ops.iter().any(|(_, o)| match o {
                        IrOperand::Copy(q) | IrOperand::Move(q) => {
                            q.projection.is_empty()
                                && (matches!(st.env.get(&q.local), Some(Val::Mut(_)))
                                    || self.mut_params.contains(&q.local))
                                && self
                                    .body
                                    .locals
                                    .get(q.local)
                                    .and_then(|t| mut_target(t))
                                    .is_some_and(|t| {
                                        t.contains("dyn ") || t.contains("fmt::Formatter")
                                    })
                        }
                        _ => false,
                    })
                {
                    return Err(kind(
                        spec::MirRvalue::Aggregate.path(),
                        "a mutable borrow held in a value, whose writes would not reach the place it borrows",
                    ));
                }
                let t = self.local_ty(dest.local, &dest.projection)?;
                let mut vals = Vec::new();
                for (_, o) in ops {
                    vals.push(self.operand(o, st)?.0);
                }
                let v = self.aggregate(agg, &t, ops, vals)?;
                self.mention(&t);
                Ok(Val::V(v, t))
            }
            IrRvalue::Cast(k, o, target) => {
                let (v, from) = self.operand(o, st)?;
                let to = self.local_ty(dest.local, &dest.projection)?;
                // A reference coerced to a trait object the table models as
                // a function (`&dyn fmt::Debug`): the trait's method applied
                // to the value, as an `rt::Argument` is.
                if let (
                    spec::MirCastKind::PointerCoercion,
                    Ty::Fn(..),
                    Some(object),
                    Some(source),
                ) = (
                    k,
                    &to,
                    target
                        .trim()
                        .strip_prefix('&')
                        .and_then(|t| t.trim_start().strip_prefix("dyn ")),
                    match o {
                        IrOperand::Copy(q) | IrOperand::Move(q) if q.projection.is_empty() => {
                            self.body.locals.get(q.local).and_then(|t| referent(t))
                        }
                        _ => None,
                    },
                ) {
                    let method = format!("<{source} as {}>::fmt", object.trim());
                    let text = format!("&{source}");
                    let called =
                        self.call(&method, vec![(v, from)], &[Some(text)], target, &to, st)?;
                    return Ok(Val::V(called.term, to));
                }
                // A cast to a thin pointer drops the metadata; one from a
                // thin pointer to a wide one does not type in Rust.
                if matches!(k, spec::MirCastKind::PtrToPtr) && is_pointer(&from) && is_pointer(&to)
                {
                    let source = match o {
                        IrOperand::Copy(q) | IrOperand::Move(q) if q.projection.is_empty() => {
                            self.body.locals.get(q.local).and_then(|t| pointee(t))
                        }
                        _ => None,
                    };
                    return match (source, pointee(target)) {
                        (Some((_, true)), Some((_, false))) => Ok(Val::V(
                            pointer_value(&format!("(project {v} allocation)"), &address(&v), "0"),
                            to,
                        )),
                        (Some((_, wide)), Some((_, into))) if wide == into => Ok(Val::V(v, to)),
                        _ => Err(kind(k.path(), "between pointers whose metadata differ")),
                    };
                }
                match (k, &from, &to) {
                    // Wrap or sign-extend, by the two types' widths and
                    // signedness; where the target's width differs between
                    // targets, a theorem that the value is in its range on
                    // all of them.
                    (spec::MirCastKind::IntToInt, _, _)
                        if Int::of(&from, self.cx.widths).is_some()
                            && Int::of(&to, self.cx.widths).is_some() =>
                    {
                        let w = self.cx.widths;
                        let (Some(fi), Some(ti)) = (Int::of(&from, w), Int::of(&to, w)) else {
                            return Err(kind(k.path(), "between integers"));
                        };
                        let (value, holds) = super::int::cast(&v, fi, ti, from == to);
                        if let Some(h) = &holds {
                            self.obligation(st, &format!("({h} = true)"), k.path());
                        }
                        Ok(Val::V(value, to))
                    }
                    // A `bool` as an integer: `1` for `true`.
                    (spec::MirCastKind::IntToInt, Ty::Bool, _)
                        if Int::of(&to, self.cx.widths).is_some() =>
                    {
                        Ok(Val::V(format!("(if {v} then 1 else 0)"), to))
                    }
                    // `as` on floats: IEEE 754's conversions, rounded to
                    // nearest, and toward zero with saturation into an
                    // integer (the Reference, "Numeric cast").
                    // A signed integer is its two's-complement bit pattern
                    // at a width every target agrees on.
                    (spec::MirCastKind::IntToFloat, i, Ty::Float(f)) => {
                        match Int::of(i, self.cx.widths) {
                            Some(Int { signed: false, .. }) => {
                                Ok(Val::V(f.term("OfNat", &[&v]), to))
                            }
                            Some(Int { width: Some(w), .. }) => {
                                Ok(Val::V(f.term("OfInt", &[&w.to_string(), &v]), to))
                            }
                            _ => Err(kind(k.path(), "from a pointer-sized signed integer")),
                        }
                    }
                    (spec::MirCastKind::FloatToInt, Ty::Float(f), i) => {
                        let Some(Int {
                            signed,
                            width: Some(w),
                            ..
                        }) = Int::of(i, self.cx.widths)
                        else {
                            return Err(kind(k.path(), "into a pointer-sized integer"));
                        };
                        let op = if signed { "ToInt" } else { "ToNat" };
                        Ok(Val::V(f.term(op, &[&w.to_string(), &v]), to))
                    }
                    (spec::MirCastKind::FloatToFloat, Ty::Float(f), Ty::Float(g)) => {
                        let (e, m) = (f.exp.to_string(), f.frac.to_string());
                        Ok(Val::V(g.term("Convert", &[&e, &m, &v]), to))
                    }
                    // A float is its bit pattern: `transmute` to or from the
                    // unsigned integer of its width is the value itself.
                    (spec::MirCastKind::Transmute, Ty::Float(f), Ty::Nat(Some(w)))
                    | (spec::MirCastKind::Transmute, Ty::Nat(Some(w)), Ty::Float(f))
                        if 1 + f.exp + f.frac == *w =>
                    {
                        Ok(Val::V(v, to))
                    }
                    // `ptr as usize`, `expose_provenance`: the address. The
                    // provenance it exposes matters only to
                    // `with_exposed_provenance`, which is residue.
                    (spec::MirCastKind::PointerExposeProvenance, f, Ty::Nat(None))
                        if is_pointer(f) =>
                    {
                        self.observes_address = true;
                        Ok(Val::V(address(&v), to))
                    }
                    // A reference is its referent here: a coercion of one,
                    // `&[T; N]` to `&[T]`, keeps the value where its lifted
                    // type is unchanged. A raw pointer's metadata may change.
                    (spec::MirCastKind::PointerCoercion, f, t) if f == t && !is_pointer(f) => {
                        Ok(Val::V(v, to))
                    }
                    (spec::MirCastKind::Transmute, f, Ty::Nat(None)) if is_pointer(f) => {
                        self.observes_address = true;
                        Ok(Val::V(address(&v), to))
                    }
                    (spec::MirCastKind::Transmute, Ty::Nat(None), t) if is_pointer(t) => {
                        Ok(Val::V(pointer_value("0", &v, "0"), to))
                    }
                    _ => Err(kind(
                        k.path(),
                        &format!("from {} to {}", from.render(), to.render()),
                    )),
                }
            }
            IrRvalue::RawPtr(_, p) => self.address_of(p, dest, st),
            // A pointer only its metadata is read of: the place's length
            // where it is a slice, without provenance, so that nothing is
            // read or written through it.
            IrRvalue::MetadataPtr(p) => {
                let (v, t) = self.read(p, st)?;
                let meta = match &t {
                    Ty::List(_) => format!("(prim length {v} : Nat)"),
                    Ty::Str => {
                        return Err(kind(
                            spec::MirRvalue::RawPtr.path(),
                            "the metadata of a str's place",
                        ))
                    }
                    _ => "0".into(),
                };
                self.mention(&Ty::Named(POINTER.to_string()));
                Ok(Val::V(
                    pointer_value("0", "0", &meta),
                    Ty::Named(POINTER.to_string()),
                ))
            }
            // `[c; n]` of a constant: the list of `n` copies of it, as long as
            // the projection's file can hold it.
            IrRvalue::Other(spec::MirRvalue::Repeat, text) => {
                let k = spec::MirRvalue::Repeat.path();
                let inner = text
                    .trim()
                    .strip_prefix('[')
                    .and_then(|r| r.strip_suffix(']'))
                    .unwrap_or("");
                let (Some(IrOperand::Constant(c)), Some(n)) = (
                    super::parse::split_top(inner, ';')
                        .first()
                        .and_then(|o| super::parse::operand(o)),
                    super::parse::split_top(inner, ';').get(1).and_then(|n| {
                        let n = n.trim().trim_start_matches("const ");
                        n.split('_').next().and_then(|d| d.parse::<usize>().ok())
                    }),
                ) else {
                    return Err(kind(k, "of a value that is not a constant"));
                };
                let (v, _) = self.constant(&c)?;
                let Ty::List(e) = self.local_ty(dest.local, &dest.projection)? else {
                    return Err(kind(k, "into a place that holds no list"));
                };
                if n.saturating_mul(v.len() + "(cons  )".len()) > self.cx.limit {
                    return Err(kind(k, "longer than the projection's file can hold"));
                }
                let mut out = format!("(nil {})", e.render());
                for _ in 0..n {
                    out = format!("(cons {v} {out})");
                }
                self.grow(&out)?;
                Ok(Val::V(out, Ty::List(e)))
            }
            IrRvalue::Other(k, _) => Err(kind(k.path(), "")),
        }
    }

    /// The valid range of the pattern-typed struct `n` at each declared
    /// pointer width (one width where its base has its own), each a
    /// wrapping range of bit patterns with the width's largest pattern.
    fn valid_ranges(&self, n: &str) -> Option<Vec<(u128, u128, u128)>> {
        let (base, pat) = self.cx.types.valid.get(n)?;
        let widths: Vec<u32> = match ty(base, self.cx.types).ok()? {
            Ty::Nat(Some(b)) | Ty::Signed(Some(b)) => vec![b],
            Ty::Nat(None) | Ty::Signed(None) => self.cx.widths.iter().map(|(w, _)| *w).collect(),
            _ => return None,
        };
        let signed = base.starts_with('i');
        widths
            .into_iter()
            .map(|bits| {
                let mask = if bits >= 128 {
                    u128::MAX
                } else {
                    (1u128 << bits) - 1
                };
                let (s, e) = valid_range(pat, signed, bits)?;
                Some((s, e, mask))
            })
            .collect()
    }

    /// The facts the pattern of the struct `n` gives the value `read` out
    /// of it, where its valid range does not wrap: at least its first
    /// pattern, at most its last.
    fn valid_facts(&self, n: &str, read: &str) -> Option<Vec<String>> {
        let ranges = self.valid_ranges(n)?;
        let mut facts = Vec::new();
        let (s, _, _) = *ranges.first()?;
        if ranges.iter().all(|(s2, e, _)| *s2 == s && s <= *e) && s > 0 {
            facts.push(format!("({s} <= {read})"));
        }
        let (_, e, _) = *ranges.first()?;
        if ranges
            .iter()
            .all(|(s2, e2, m)| *e2 == e && *s2 <= e && e < *m)
        {
            facts.push(format!("({read} <= {e})"));
        }
        Some(facts)
    }

    /// The widest a type is on any declared target.
    fn max_bits(&self, t: &Ty) -> u32 {
        match t {
            Ty::Nat(Some(b)) => *b,
            _ => self.cx.widths.last().map_or(64, |(w, _)| *w),
        }
    }

    /// The narrowest a type is on any declared target.
    fn min_bits(&self, t: &Ty) -> u32 {
        match t {
            Ty::Nat(Some(b)) => *b,
            _ => self.pointer_width(),
        }
    }

    /// A slice's length is at most `isize::MAX` at the narrowest pointer
    /// width, unless its items are zero-sized.
    fn length_fact(&mut self, len: &str, elem: &Ty, st: &mut State) {
        let zero_sized = match elem {
            Ty::Unit => true,
            Ty::Named(n) => self.cx.types.structs.get(n).is_some_and(Vec::is_empty),
            _ => false,
        };
        if !zero_sized {
            let half = max_of(self.pointer_width() - 1);
            st.facts.insert(format!("({len} <= {half})"));
        }
    }

    /// Declares the integer helper `name` (see [`super::int::HELPERS`]) and
    /// the helpers it calls, with each one's decreasing measure, and
    /// returns its name.
    fn use_helper(&mut self, name: &str) -> String {
        let Some(h) = super::int::helper(name) else {
            return name.to_string();
        };
        self.calls.insert(name.to_string());
        if self.aux.contains_key(name) {
            return name.to_string();
        }
        for u in h.uses {
            self.use_helper(u);
        }
        let parameters: Vec<(String, String)> = h
            .params
            .iter()
            .map(|p| (p.to_string(), "Nat".to_string()))
            .collect();
        let measure = h.params[0];
        let evidence = format!("decreases_{name}_0");
        let axioms = vec!["Quot.sound".to_string(), "propext".to_string()];
        self.aux.insert(
            evidence.clone(),
            Decl::Theorem {
                name: evidence.clone(),
                parameters: parameters.clone(),
                statement: implication(
                    &[format!("(({measure} =? 0) = false)")],
                    &format!("({} < {measure})", h.step),
                ),
                proof: "linear_arithmetic{}".into(),
                axioms: axioms.clone(),
            },
        );
        self.aux.insert(
            name.to_string(),
            Decl::Definition {
                name: name.to_string(),
                parameters,
                result: "Nat".into(),
                body: Expr::Flat(h.body.to_string()),
                axioms,
                recursive: None,
                termination: Some((measure.to_string(), vec![evidence])),
                mutual: None,
            },
        );
        name.to_string()
    }

    fn binary(
        &mut self,
        op: spec::MirBinOp,
        a: &IrOperand,
        b: &IrOperand,
        st: &mut State,
    ) -> Lift<Val> {
        let (x, xt) = self.operand(a, st)?;
        let (y, yt) = self.operand(b, st)?;
        // An offset moves the pointer by whole pointees, which the
        // operand's type names.
        if is_pointer(&xt) && matches!(op, spec::MirBinOp::Offset) {
            let text = match a {
                IrOperand::Copy(q) | IrOperand::Move(q) if q.projection.is_empty() => {
                    self.body.locals.get(q.local).cloned().unwrap_or_default()
                }
                _ => String::new(),
            };
            let Some((pointee_text, _)) = pointee(&text) else {
                return Err(kind(
                    op.path(),
                    "an offset of a pointer whose type is not known",
                ));
            };
            let pointee_text = pointee_text.to_string();
            let moved = self.offset_pointer(&x, &pointee_text, &y, &yt, true, st)?;
            return Ok(Val::V(moved, xt));
        }
        self.binary_on(op, x, xt, y, yt, st)
    }

    /// The binary operator `op` on two terms of their types.
    fn binary_on(
        &mut self,
        op: spec::MirBinOp,
        x: String,
        xt: Ty,
        y: String,
        yt: Ty,
        st: &mut State,
    ) -> Lift<Val> {
        use spec::MirBinOp as B;
        // Raw pointers compare by their addresses: `core::ptr` compares thin
        // pointers as addresses, provenance aside.
        // A wide pointer's metadata breaks a tie, as rustc's comparison of
        // wide pointers does; a thin pointer's is always the unit.
        if is_pointer(&xt)
            && is_pointer(&yt)
            && matches!(op, B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge)
        {
            self.observes_address = true;
            let (ax, ay) = (address(&x), address(&y));
            let (mx, my) = (
                format!("(project {x} metadata)"),
                format!("(project {y} metadata)"),
            );
            let eq = format!("(({ax} =? {ay}) && ({mx} =? {my}))");
            let lt = |a: &str, b: &str, ma: &str, mb: &str, strict: bool| {
                let tie = if strict { "<?" } else { "<=?" };
                format!("(({a} <? {b}) || (({a} =? {b}) && ({ma} {tie} {mb})))")
            };
            let s = match op {
                B::Eq => eq,
                B::Ne => format!("(not {eq})"),
                B::Lt => lt(&ax, &ay, &mx, &my, true),
                B::Le => lt(&ax, &ay, &mx, &my, false),
                B::Gt => lt(&ay, &ax, &my, &mx, true),
                _ => lt(&ay, &ax, &my, &mx, false),
            };
            return Ok(Val::V(s, Ty::Bool));
        }
        // A signed integer by its Rust name, which the `Nat` it is held as
        // would not say.
        let bad = || {
            let name = |t: &Ty| match t {
                Ty::Signed(Some(b)) => format!("i{b}"),
                Ty::Signed(None) => "isize".into(),
                _ => t.render(),
            };
            kind(op.path(), &format!("on {} and {}", name(&xt), name(&yt)))
        };
        // IEEE 754's operations and comparisons: the prelude's model.
        if let (Ty::Float(f), true) = (&xt, xt == yt) {
            let (name, t) = match op {
                B::Add => ("Add", xt.clone()),
                B::Sub => ("Sub", xt.clone()),
                B::Mul => ("Mul", xt.clone()),
                B::Div => ("Div", xt.clone()),
                B::Rem => ("Rem", xt.clone()),
                B::Eq => ("Eq", Ty::Bool),
                B::Ne => ("Ne", Ty::Bool),
                B::Lt => ("Lt", Ty::Bool),
                B::Le => ("Le", Ty::Bool),
                B::Gt => ("Gt", Ty::Bool),
                B::Ge => ("Ge", Ty::Bool),
                _ => return Err(bad()),
            };
            return Ok(Val::V(f.term(name, &[&x, &y]), t));
        }
        let nat = matches!(xt, Ty::Nat(_)) && matches!(yt, Ty::Nat(_));
        let boolean = xt == Ty::Bool && yt == Ty::Bool;
        let v = |s: String, t: Ty| Ok(Val::V(s, t));
        match op {
            B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge if nat => {
                let s = match op {
                    B::Eq => format!("({x} =? {y})"),
                    B::Ne => format!("(not ({x} =? {y}))"),
                    B::Lt => format!("({x} <? {y})"),
                    B::Le => format!("({x} <=? {y})"),
                    B::Gt => format!("({y} <? {x})"),
                    _ => format!("({y} <=? {x})"),
                };
                v(s, Ty::Bool)
            }
            // A signed bit pattern's equality is the natural's; its order is
            // its order key's.
            B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge
                if matches!(xt, Ty::Signed(_)) && xt == yt =>
            {
                let i = Int::of(&xt, self.cx.widths).ok_or_else(bad)?;
                let less = super::int::less;
                let s = match op {
                    B::Eq => format!("({x} =? {y})"),
                    B::Ne => format!("(not ({x} =? {y}))"),
                    B::Lt => less(&x, &y, i),
                    B::Le => format!("(not {})", less(&y, &x, i)),
                    B::Gt => less(&y, &x, i),
                    _ => format!("(not {})", less(&x, &y, i)),
                };
                v(s, Ty::Bool)
            }
            // Int64's equality and order, decided as the AST path's are.
            B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge
                if matches!(xt, Ty::Int(_)) && xt == yt =>
            {
                let s = match op {
                    B::Eq => format!("(prim equal {x} {y} : Bool)"),
                    B::Ne => format!("(not (prim equal {x} {y} : Bool))"),
                    B::Lt => format!("(prim less_than {x} {y} : Bool)"),
                    B::Le => format!("(not (prim less_than {y} {x} : Bool))"),
                    B::Gt => format!("(prim less_than {y} {x} : Bool)"),
                    _ => format!("(not (prim less_than {x} {y} : Bool))"),
                };
                v(s, Ty::Bool)
            }
            B::Eq if boolean => v(format!("(if {x} then {y} else (not {y}))"), Ty::Bool),
            B::Ne if boolean => v(format!("(if {x} then (not {y}) else {y})"), Ty::Bool),
            B::BitAnd if boolean => v(format!("({x} && {y})"), Ty::Bool),
            B::BitOr if boolean => v(format!("({x} || {y})"), Ty::Bool),
            B::BitXor if boolean => v(format!("(if {x} then (not {y}) else {y})"), Ty::Bool),
            B::Eq | B::Ne if matches!(xt, Ty::Str | Ty::Char) && xt == yt => {
                let eq = format!("(prim equal {x} {y} : Bool)");
                v(
                    if op == B::Eq {
                        eq
                    } else {
                        format!("(not {eq})")
                    },
                    Ty::Bool,
                )
            }
            B::AddWithOverflow | B::SubWithOverflow | B::MulWithOverflow if nat => {
                let bits = self.bits(&xt).ok_or_else(bad)?;
                let max = max_of(bits);
                let (value, overflow) = match op {
                    B::AddWithOverflow => {
                        (format!("({x} + {y})"), format!("({max} <? ({x} + {y}))"))
                    }
                    B::SubWithOverflow => (
                        format!("(prim subtract {x} {y} : Nat)"),
                        format!("({x} <? {y})"),
                    ),
                    _ => (
                        format!("(prim multiply {x} {y} : Nat)"),
                        format!("({max} <? (prim multiply {x} {y} : Nat))"),
                    ),
                };
                Ok(Val::Checked(value, overflow, xt))
            }
            // The unchecked forms, and the plain ones MIR wraps: exact only
            // where the value stays in range, which a theorem states.
            B::Add | B::AddUnchecked | B::Sub | B::SubUnchecked | B::Mul | B::MulUnchecked
                if nat =>
            {
                let bits = self.bits(&xt).ok_or_else(bad)?;
                let max = max_of(bits);
                let (value, fits) = match op {
                    B::Add | B::AddUnchecked => {
                        (format!("({x} + {y})"), format!("(({x} + {y}) <=? {max})"))
                    }
                    B::Sub | B::SubUnchecked => (
                        format!("(prim subtract {x} {y} : Nat)"),
                        format!("({y} <=? {x})"),
                    ),
                    _ => (
                        format!("(prim multiply {x} {y} : Nat)"),
                        format!("((prim multiply {x} {y} : Nat) <=? {max})"),
                    ),
                };
                self.obligation(st, &format!("({fits} = true)"), op.path());
                v(value, xt)
            }
            // A divisor of zero is excluded by the assert before it.
            B::Div if nat => v(format!("(prim quotient {x} {y} 0 : Nat)"), xt),
            B::Rem if nat => v(format!("(prim remainder {x} {y} 0 : Nat)"), xt),
            // Signed arithmetic on bit patterns. The key `x + 2^(w-1)` (mod
            // `2^w`) is the value offset by `2^(w-1)`, so sums, differences
            // and products of values are sums, differences and products of
            // keys, offset; the result is in range when its offset value is
            // within `2^(n-1)` of the offset, `n` the narrowest width, and
            // its pattern is the patterns' sum, difference or product,
            // wrapped (they agree with the values mod `2^w`). Division
            // truncates: magnitudes divide, and the sign is the signs'.
            B::AddWithOverflow
            | B::SubWithOverflow
            | B::MulWithOverflow
            | B::Add
            | B::AddUnchecked
            | B::Sub
            | B::SubUnchecked
            | B::Mul
            | B::MulUnchecked
            | B::Div
            | B::Rem
                if matches!(xt, Ty::Signed(_)) && xt == yt =>
            {
                let int = Int::of(&xt, self.cx.widths).ok_or_else(bad)?;
                let pow2 = super::int::pow2;
                let (m, h, hn) = (pow2(int.held), pow2(int.held - 1), pow2(int.check - 1));
                let (lo, hi) = (
                    format!("(prim subtract {m} {hn} : Nat)"),
                    format!("({m} + {hn})"),
                );
                let key = |t: &str| {
                    format!("(if ({t} <? {h}) then ({t} + {h}) else (prim subtract {t} {h} : Nat))")
                };
                let (kx, ky) = (key(&x), key(&y));
                let wrap = |t: String| format!("(prim remainder {t} {m} 0 : Nat)");
                let (value, fits) = match op {
                    B::AddWithOverflow | B::Add | B::AddUnchecked => {
                        let s = format!("({kx} + {ky})");
                        (
                            wrap(format!("({x} + {y})")),
                            format!("(({lo} <=? {s}) && ({s} <? {hi}))"),
                        )
                    }
                    B::SubWithOverflow | B::Sub | B::SubUnchecked => {
                        let d = format!("(prim subtract ({kx} + {m}) {ky} : Nat)");
                        (
                            wrap(format!("(prim subtract ({x} + {m}) {y} : Nat)")),
                            format!("(({lo} <=? {d}) && ({d} <? {hi}))"),
                        )
                    }
                    B::Div | B::Rem => {
                        let abs = |t: &str| {
                            format!("(if ({t} <? {h}) then {t} else (prim subtract {m} {t} : Nat))")
                        };
                        let (ax, ay) = (abs(&x), abs(&y));
                        let negate = |t: String| wrap(format!("(prim subtract {m} {t} : Nat)"));
                        let value = if op == B::Div {
                            let q = format!("(prim quotient {ax} {ay} 0 : Nat)");
                            format!(
                                "(if (if ({x} <? {h}) then ({h} <=? {y}) else ({y} <? {h})) then {} else {q})",
                                negate(q.clone())
                            )
                        } else {
                            let r = format!("(prim remainder {ax} {ay} 0 : Nat)");
                            format!("(if ({x} <? {h}) then {r} else {})", negate(r.clone()))
                        };
                        // Its divisor and overflow are the asserts before it.
                        return v(value, xt);
                    }
                    _ => {
                        let p = format!(
                            "((prim multiply {kx} {ky} : Nat) + {})",
                            pow2(2 * int.held - 2)
                        );
                        let q = format!("(prim multiply {h} ({kx} + {ky}) : Nat)");
                        (
                            wrap(format!("(prim multiply {x} {y} : Nat)")),
                            format!("(({q} <=? ({p} + {hn})) && ({p} <? ({q} + {hn})))"),
                        )
                    }
                };
                if matches!(
                    op,
                    B::AddWithOverflow | B::SubWithOverflow | B::MulWithOverflow
                ) {
                    return Ok(Val::Checked(value, format!("(not {fits})"), xt));
                }
                self.obligation(st, &format!("({fits} = true)"), op.path());
                v(value, xt)
            }
            // A shift by `k`, which MIR asserts is below the width: `x *
            // 2^k` wrapped where the width is the same on every target, `x /
            // 2^k`, and for a signed value the sign's bits shifted in. `2^k`
            // is a literal for a literal `k`, else `mirPowTwo k`, the power
            // by its recursion on `k`.
            B::Shl | B::ShlUnchecked | B::Shr | B::ShrUnchecked
                if Int::of(&xt, self.cx.widths).is_some()
                    && Int::of(&yt, self.cx.widths).is_some() =>
            {
                let int = Int::of(&xt, self.cx.widths).ok_or_else(bad)?;
                let pow2 = super::int::pow2;
                let m = pow2(int.held);
                let (pk, fill) = match y.parse::<u32>() {
                    Ok(k) if k < int.held => (pow2(k), pow2(int.held - k)),
                    Ok(_) => return Err(bad()),
                    Err(_) => {
                        let name = self.use_helper("mirPowTwo");
                        let pk = format!("({name} {y})");
                        (pk.clone(), format!("(prim quotient {m} {pk} 0 : Nat)"))
                    }
                };
                let value = match op {
                    B::Shl | B::ShlUnchecked if int.width.is_some() || int.signed => {
                        format!("(prim remainder (prim multiply {x} {pk} : Nat) {m} 0 : Nat)")
                    }
                    B::Shl | B::ShlUnchecked => return Err(bad()),
                    _ if int.signed => {
                        let q = format!("(prim quotient {x} {pk} 0 : Nat)");
                        format!(
                            "(if ({x} <? {}) then {q} else ({q} + (prim subtract {m} {fill} : Nat)))",
                            pow2(int.held - 1)
                        )
                    }
                    _ => format!("(prim quotient {x} {pk} 0 : Nat)"),
                };
                v(value, xt)
            }
            // A mask of the low bits by a literal `2^k - 1`: the value
            // modulo `2^k`.
            B::BitAnd
                if xt == yt
                    && Int::of(&xt, self.cx.widths).is_some()
                    && [&x, &y].iter().any(|t| {
                        t.parse::<u128>()
                            .is_ok_and(|c| c.checked_add(1).is_some_and(u128::is_power_of_two))
                    }) =>
            {
                let (c, other) = match x.parse::<u128>() {
                    Ok(c) if c.checked_add(1).is_some_and(u128::is_power_of_two) => (c, &y),
                    _ => (y.parse::<u128>().map_err(|_| bad())?, &x),
                };
                v(format!("(prim remainder {other} {} 0 : Nat)", c + 1), xt)
            }
            // AND, OR and XOR of two values bit by bit, by their recursion
            // on the low bit (LexLean's bit operations take no `Nat`, see
            // `upstream.toml`): a signed value's on its pattern.
            B::BitAnd | B::BitOr | B::BitXor
                if xt == yt && Int::of(&xt, self.cx.widths).is_some() =>
            {
                let name = self.use_helper(match op {
                    B::BitAnd => "mirBitAnd",
                    B::BitOr => "mirBitOr",
                    _ => "mirBitXor",
                });
                v(format!("({name} {x} {y})"), xt)
            }
            _ => Err(bad()),
        }
    }

    fn aggregate(
        &mut self,
        agg: &IrAggregate,
        t: &Ty,
        ops: &[(Option<String>, IrOperand)],
        vals: Vec<String>,
    ) -> Lift<String> {
        let k = spec::MirRvalue::Aggregate.path();
        // `MaybeUninit { uninit: () }` holds no value, `MaybeUninit { value }`
        // its value; `ManuallyDrop { value }` is the value.
        if let IrAggregate::Adt(path, None) = agg {
            let name = wrapper(path)
                .map(|(n, _)| n)
                .or_else(|| split_path(path).last().copied());
            let field = ops.first().and_then(|(f, _)| f.as_deref());
            match (name, field, t, vals.as_slice()) {
                (Some("MaybeUninit"), Some("uninit"), Ty::Option(inner), _) => {
                    return Ok(format!("(ctor Option.none [{}])", inner.render()))
                }
                (Some("MaybeUninit"), Some("value"), Ty::Option(inner), [v]) => {
                    return Ok(format!("(ctor Option.some [{}] {v})", inner.render()))
                }
                (Some("ManuallyDrop"), Some("value"), _, [v]) => return Ok(v.clone()),
                // `NonNull { pointer }` and `Unique { pointer, _marker }`
                // are the pointer they hold.
                (Some("NonNull" | "Unique"), Some("pointer"), t, [v, ..]) if is_pointer(t) => {
                    return Ok(v.clone())
                }
                _ => {}
            }
        }
        match (agg, t) {
            (IrAggregate::Tuple, Ty::Unit) if vals.is_empty() => Ok("()".into()),
            (IrAggregate::Tuple, Ty::Tuple(_) | Ty::Pair(_)) => Ok(lift::tuple_value(t, &vals)),
            // A tuple of one item is the item.
            (IrAggregate::Tuple, _) if vals.len() == 1 => Ok(vals[0].clone()),
            (IrAggregate::Array, Ty::List(e)) => {
                let mut out = format!("(nil {})", e.render());
                for v in vals.iter().rev() {
                    out = format!("(cons {v} {out})");
                }
                Ok(out)
            }
            (IrAggregate::Adt(_, Some(variant)), Ty::Option(inner)) => {
                match (variant.as_str(), vals.as_slice()) {
                    ("Some", [v]) => Ok(format!("(ctor Option.some [{}] {v})", inner.render())),
                    ("None", []) => Ok(format!("(ctor Option.none [{}])", inner.render())),
                    _ => Err(kind(k, "an Option variant")),
                }
            }
            (IrAggregate::Adt(_, Some(variant)), Ty::Outcome(ok, err)) => {
                let args = format!("[{}, {}]", ok.render(), err.render());
                match (variant.as_str(), vals.as_slice()) {
                    // `ControlFlow` is the `Result` of its continue and its
                    // break values, `Continue` first as `Ok` is.
                    ("Ok" | "Continue", [v]) => Ok(format!("(ctor Result.ok {args} {v})")),
                    ("Err" | "Break", [v]) => Ok(format!("(ctor Result.error {args} {v})")),
                    _ => Err(kind(k, "a Result variant")),
                }
            }
            (IrAggregate::Adt(_, Some(variant)), Ty::Named(n)) => {
                let types = self.cx.types;
                if types.enums.get(n).is_some_and(|vs| vs.contains(variant)) && vals.is_empty() {
                    return Ok(format!("(ctor {n}.{variant})"));
                }
                if types.datas.get(n).is_some_and(|vs| {
                    vs.iter()
                        .any(|(v, fs)| v == variant && fs.len() == vals.len())
                }) {
                    let mut parts = vec![format!("ctor {n}.{variant}")];
                    parts.extend(vals);
                    return Ok(format!("({})", parts.join(" ")));
                }
                Err(kind(k, &format!("the variant {n}::{variant}")))
            }
            (IrAggregate::Adt(_, None), Ty::Named(n)) => {
                let fields = self
                    .cx
                    .types
                    .structs
                    .get(n)
                    .ok_or_else(|| kind(k, &format!("{n}, which is not a struct")))?;
                // Named fields in the order the struct declares them.
                let mut by: BTreeMap<String, String> = BTreeMap::new();
                for (i, ((f, _), v)) in ops.iter().zip(vals).enumerate() {
                    // A tuple struct's fields by position.
                    let f = match f {
                        Some(f) => lift::local_name(f),
                        None => fields
                            .get(i)
                            .map(|(f, _)| f.clone())
                            .ok_or_else(|| kind(k, "a tuple struct's field past its own"))?,
                    };
                    by.insert(f, v);
                }
                let mut parts = Vec::new();
                for (f, _) in fields {
                    let v = by.get(f).ok_or_else(|| {
                        kind(k, &format!("a struct literal without the field {f}"))
                    })?;
                    parts.push((f, v.clone()));
                }
                Ok(lift::struct_value(n, &parts, self.cx.types))
            }
            (IrAggregate::Other(a), _) => Err(kind(a.path(), "")),
            (IrAggregate::Closure(_), _) => Err(kind(
                spec::MirAggregateKind::Closure.path(),
                "a closure built where its type is not a function",
            )),
            _ => Err(kind(k, &format!("of {}", t.render()))),
        }
    }

    /// The definition a closure's MIR body lifts to (its captures as its
    /// first parameters, then its arguments), lowered on first use: its
    /// name and the number of captures.
    fn closure_def(&mut self, closure: &str) -> Lift<(String, usize)> {
        let k = spec::MirAggregateKind::Closure.path();
        let body = self
            .cx
            .closures
            .get(closure)
            .ok_or_else(|| kind(k, "a closure whose body the dump does not hold"))?;
        let segs: Vec<String> = std::iter::once("closure".to_string())
            .chain(
                body.path
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .filter(|s| !s.is_empty() && *s != "closure")
                    .map(str::to_string),
            )
            .collect();
        let parts: Vec<&str> = segs.iter().map(String::as_str).collect();
        let name = crate::lex::camel(&parts);
        if self.cx.active.contains(&name) {
            return Err(kind(k, &format!("{}, which calls itself", body.path)));
        }
        if !self.aux.contains_key(&name) {
            let mut active = self.cx.active.to_vec();
            active.push(name.clone());
            let cx = self.cx.within(&active);
            let lowered = lower(&cx, body, &name)
                .map_err(|r| kind(k, &format!("{}: {}", body.path, r.detail())))?;
            for d in lowered.decls {
                self.aux.insert(d.name().to_string(), d);
            }
            for (n, d) in lowered.aux {
                self.aux.entry(n).or_insert(d);
            }
            self.calls.extend(lowered.calls);
            self.types.extend(lowered.types);
        }
        self.calls.insert(name.clone());
        Ok((name, body.upvars.len()))
    }

    /// A closure's value: its environment, the values it captures (as Rust
    /// holds them, a closure is the struct of its captures), whose type is
    /// the closure's type; a call of it applies the closure's definition to
    /// them and to the arguments.
    fn closure_value(
        &mut self,
        closure: &str,
        ops: &[(Option<String>, IrOperand)],
        st: &State,
    ) -> Lift<Val> {
        let k = spec::MirAggregateKind::Closure.path();
        self.closure_def(closure)?;
        let upvars = self.cx.closures[closure].upvars.clone();
        let mut values = Vec::new();
        for (j, up) in upvars.iter().enumerate() {
            let by_name = up.as_ref().and_then(|(n, _)| {
                ops.iter()
                    .find(|(f, _)| f.as_deref() == Some(n.as_str()))
                    .map(|(_, o)| o)
            });
            let o = by_name
                .or_else(|| ops.get(j).map(|(_, o)| o))
                .ok_or_else(|| kind(k, "a capture the closure is not given"))?;
            values.push(self.operand(o, st)?.0);
        }
        let t = ty(closure, self.cx.types)?;
        self.mention(&t);
        let term = match values.len() {
            0 => "()".to_string(),
            1 => values.remove(0),
            _ => lift::tuple_value(&t, &values),
        };
        Ok(Val::V(term, t))
    }

    /// A call: a function the lift has, or one of the standard library's
    /// operations whose meaning on values is fixed.
    fn call(
        &mut self,
        callee: &str,
        args: Vec<(String, Ty)>,
        texts: &[Option<String>],
        dest: &str,
        dt: &Ty,
        st: &mut State,
    ) -> Lift<Called> {
        let k = spec::MirTerminatorKind::Call.path();
        // A call of a closure through `Fn`, `FnMut` or `FnOnce`: its
        // definition applied to the captures its environment holds and to
        // the arguments, which MIR passes as one tuple. The environment is
        // unchanged by the call.
        let erased = erase(callee);
        let through_fn = erased
            .strip_prefix('<')
            .and_then(|r| r.rsplit_once(">::"))
            .and_then(|(q, m)| {
                let (on, tr) = q.split_once(" as ")?;
                let fun = matches!(split_path(tr).last(), Some(&("Fn" | "FnMut" | "FnOnce")))
                    && matches!(m, "call" | "call_mut" | "call_once");
                fun.then(|| {
                    on.trim_start_matches('&')
                        .trim_start_matches("mut ")
                        .to_string()
                })
            });
        if let Some(closure) = through_fn.filter(|c| c.starts_with("{closure@")) {
            let (name, captured) = self.closure_def(&closure)?;
            let [(env, et), (a, at)] = args.as_slice() else {
                return Err(kind(k, "a closure called without its arguments' tuple"));
            };
            let _ = captured;
            let mut items: Vec<String> = vec![env.clone()];
            let inputs = self.cx.closures[&closure].arity.saturating_sub(1);
            match (inputs, at) {
                (0, _) => {}
                (1, _) => items.push(a.clone()),
                (n, Ty::Tuple(ts) | Ty::Pair(ts)) if ts.len() == n => {
                    items.extend((0..n).map(|j| lift::tuple_item(at, a, j)));
                }
                _ => return Err(kind(k, "a closure applied to another arity")),
            }
            // A closure that writes through a `&mut` argument gives it back
            // inside the arguments' tuple, whose places the call does not
            // hold apart. One whose environment holds a mutable borrow
            // gives its environment back, after its value.
            // A closure of one input gives that input back as the
            // arguments' tuple, which lifts as the input.
            let cb = self.cx.closures[&closure].clone();
            let threads = threaded(&cb.locals, cb.arity, true, self.cx.types);
            if threads.iter().any(|l| *l != 1 && !(inputs == 1 && *l == 2)) {
                return Err(kind(
                    k,
                    "a closure that writes through a `&mut` argument, passed in the arguments' tuple",
                ));
            }
            let term = format!("({name} {})", items.join(" "));
            if !threads.is_empty() {
                let positions: Vec<usize> = threads.iter().map(|l| l - 1).collect();
                let rest: Vec<Ty> = threads
                    .iter()
                    .map(|l| if *l == 1 { et.clone() } else { at.clone() })
                    .collect();
                let full = threaded_result(dt, &rest, self.cx.types);
                // A closure that returns a borrow into what it threads: its
                // backward function, lowered from the same body.
                let returns_borrow = cb.locals.first().is_some_and(|t| {
                    threaded(&[String::new(), t.clone()], 1, false, self.cx.types).len() == 1
                });
                let back = format!("{name}Back");
                if returns_borrow && !self.aux.contains_key(&back) {
                    let mut active = self.cx.active.to_vec();
                    active.push(back.clone());
                    let mut cx = self.cx.within(&active);
                    cx.backward = true;
                    if let Ok(lowered) = lower(&cx, &cb, &back) {
                        for d in lowered.decls {
                            self.aux.insert(d.name().to_string(), d);
                        }
                        for (n, d) in lowered.aux {
                            self.aux.entry(n).or_insert(d);
                        }
                        self.calls.extend(lowered.calls);
                        self.types.extend(lowered.types);
                    }
                }
                let backward = (returns_borrow && self.aux.contains_key(&back)).then(|| {
                    self.calls.insert(back.clone());
                    (
                        std::iter::once(back.clone())
                            .chain(items.iter().cloned())
                            .collect::<Vec<_>>()
                            .join(" "),
                        positions.clone(),
                        (rest.len() > 1).then(|| self.cx.types.tuple(rest.clone())),
                    )
                });
                return Ok(Called {
                    threads: Some((full, positions)),
                    backward,
                    ..Called::value(term)
                });
            }
            return Ok(Called {
                term,
                updates: BTreeMap::from([(0, env.clone())]),
                guard: None,
                threads: None,
                memory: false,
                backward: None,
            });
        }
        let key = format!("{}::{callee}", self.cx.krate);
        // A trait method of one of the crate's types is its impl's method.
        let own = callee
            .strip_prefix('<')
            .and_then(|r| r.split_once(" as "))
            .and_then(|(t, rest)| Some((t, rest.rsplit_once(">::")?.1)))
            .map(|(t, m)| format!("{}::{}::{m}", self.cx.krate, erase(t)));
        if let Some(c) = self
            .cx
            .callees
            .get(&key)
            .or_else(|| self.cx.callees.get(callee))
            .or_else(|| own.as_ref().and_then(|o| self.cx.callees.get(o)))
        {
            if c.name == self.name {
                return Err(kind(
                    k,
                    "a call of the function itself, whose measure is not derived",
                ));
            }
            if c.params.len() != args.len()
                || c.params
                    .iter()
                    .zip(&args)
                    .any(|(p, (_, a))| p.render() != a.render())
                || c.result.render() != dt.render()
            {
                return Err(kind(
                    k,
                    &format!("{callee}, at other types than the lift gives it"),
                ));
            }
            self.calls.insert(c.name.clone());
            let term = if args.is_empty() {
                c.name.clone()
            } else {
                let a: Vec<String> = args.into_iter().map(|(v, _)| v).collect();
                format!("({} {})", c.name, a.join(" "))
            };
            return Ok(Called {
                threads: c.threads.clone(),
                ..Called::value(term)
            });
        }
        // Core's float intrinsics (which core's own bodies call by their
        // bare name), and the methods and operator impls of a float type,
        // which core writes as the operators themselves: the prelude's
        // model of each.
        let floats = &self.cx.types.floats;
        let segs = split_path(&erased);
        let intrinsic = matches!(
            segs.as_slice(),
            ["intrinsics", _] | ["core" | "std", "intrinsics", _]
        ) || (segs.len() == 1 && self.cx.types.core);
        let name = segs.last().copied().unwrap_or("");
        // `float_to_int_unchecked`: its result where it is defined, a value
        // in the target's range, is the truncation `as` gives.
        if let (
            true,
            "float_to_int_unchecked",
            [(v, Ty::Float(f))],
            Some(i @ Int { width: Some(w), .. }),
        ) = (
            intrinsic,
            name,
            args.as_slice(),
            Int::of(dt, self.cx.widths),
        ) {
            let op = if i.signed { "ToInt" } else { "ToNat" };
            return Ok(Called::value(f.term(op, &[&w.to_string(), v])));
        }
        let found = if intrinsic {
            floats.iter().find_map(|(t, f)| {
                let base = name.strip_suffix(t.as_str())?;
                let (_, op, n) = float::INTRINSICS.iter().find(|(b, _, _)| *b == base)?;
                Some((*f, *op, *n, float::Kind::Value))
            })
        } else {
            // `<f32 as ops::arith::Add>::add`, or `f32::<impl f32>::is_nan`.
            let parts = match erased.strip_prefix('<') {
                Some(rest) => rest.rsplit_once(">::").and_then(|(q, m)| {
                    let (on, tr) = q.split_once(" as ")?;
                    Some((
                        on.to_string(),
                        split_path(tr).last()?.to_string(),
                        m.to_string(),
                    ))
                }),
                None => match segs.as_slice() {
                    [on, m] => Some((on.to_string(), String::new(), m.to_string())),
                    [on, imp, m] if *imp == format!("<impl {on}>") => {
                        Some((on.to_string(), String::new(), m.to_string()))
                    }
                    _ => None,
                },
            };
            parts.and_then(|(on, tr, m)| {
                let f = *floats.get(&on)?;
                let (_, _, op, n, kind) = float::METHODS.iter().find(|r| r.0 == tr && r.1 == m)?;
                Some((f, *op, *n, *kind))
            })
        };
        if let Some((f, op, n, kind)) = found {
            let float = Ty::Float(f);
            let terms: Vec<&str> = args.iter().map(|(v, _)| v.as_str()).collect();
            let typed = args.len() == n && args.iter().all(|(_, t)| *t == float);
            let bits = Ty::Nat(Some(1 + f.exp + f.frac));
            match kind {
                float::Kind::Value if typed && *dt == float => {
                    return Ok(Called::value(f.term(op, &terms)))
                }
                float::Kind::Test if typed && *dt == Ty::Bool => {
                    return Ok(Called::value(f.term(op, &terms)))
                }
                float::Kind::Assign if typed && *dt == Ty::Unit => {
                    return Ok(Called {
                        term: "()".into(),
                        updates: BTreeMap::from([(0, f.term(op, &terms))]),
                        guard: None,
                        threads: None,
                        memory: false,
                        backward: None,
                    })
                }
                float::Kind::Bits
                    if args.len() == 1
                        && ((args[0].1 == float && *dt == bits)
                            || (args[0].1 == bits && *dt == float)) =>
                {
                    return Ok(Called::value(args[0].0.clone()))
                }
                _ => {}
            }
        }
        if let Some(v) = std_call(callee, &args, dt, self, st) {
            return Ok(Called::value(v));
        }
        // A std operation whose body bottoms out below values: its model.
        let types = self.cx.types;
        let defaults = |t: &Ty| lift::default_of(t, types);
        if let Some(applied) = stdlib::apply(&erase(callee), &args, dt, &defaults) {
            for t in &applied.mentions {
                self.mention(t);
            }
            for d in applied.aux {
                self.calls.insert(d.name().to_string());
                self.aux.entry(d.name().to_string()).or_insert(d);
            }
            return Ok(Called {
                term: applied.term,
                updates: applied.updates,
                guard: applied.guard,
                threads: None,
                memory: false,
                backward: None,
            });
        }
        // `Arguments::new(template, args)` of `format_args!`, where the
        // table models Arguments as its pieces: the template is the
        // constant `format_args!` writes, read here in core's own encoding
        // (core/src/fmt/mod.rs, at `Arguments`): a literal piece is its
        // length (one byte below 0x80, or 0x80 and 16 bits) and its bytes;
        // a placeholder is a byte of the top two bits set, whose low bits
        // say which of its flags (32 bits), width, precision and argument
        // index (16 bits each, little-endian) follow; a zero byte ends it.
        // Each piece is its literal text and, for a placeholder, its
        // options (core's `FormattingOptions::new()` where the template
        // gives none) and the index of the argument it formats.
        if let (Some(link), Ty::Tuple(parts), [(template, Ty::List(_)), (list, Ty::List(_))]) =
            (self.cx.core, dt, args.as_slice())
        {
            let piece_ty = match parts.as_slice() {
                [Ty::Option(lit), Ty::List(p), Ty::List(_)] if **lit == Ty::Str => {
                    Some((**p).clone())
                }
                _ => None,
            };
            let is_new = link
                .instance(callee, texts, dest)
                .is_some_and(|i| i.text.contains("fn ") && i.path.ends_with("::new"));
            let bytes: Option<Vec<u32>> = {
                let mut rest = template.trim();
                let mut out = Some(Vec::new());
                while let Some(r) = rest.strip_prefix("(cons ") {
                    let (n, r) = r.split_once(' ').unwrap_or(("", ""));
                    match (out.as_mut(), n.parse::<u32>()) {
                        (Some(o), Ok(b)) => o.push(b),
                        _ => out = None,
                    }
                    rest = r;
                }
                out.filter(|_| rest.starts_with("(nil "))
            };
            if let (true, Some(Ty::Tuple(pt)), Some(bytes)) = (is_new, &piece_ty, bytes) {
                let [Ty::Str, Ty::Option(ph)] = pt.as_slice() else {
                    return Err(kind(k, "a template piece of another shape"));
                };
                let Ty::Tuple(pht) = &**ph else {
                    return Err(kind(k, "a placeholder of another shape"));
                };
                let opts_ty = pht.first().cloned().unwrap_or(Ty::Unit);
                let Ty::Named(opts_name) = &opts_ty else {
                    return Err(kind(k, "placeholder options that are no struct"));
                };
                let fields = self
                    .cx
                    .types
                    .structs
                    .get(opts_name)
                    .cloned()
                    .unwrap_or_default();
                let defaults = self.call(
                    "FormattingOptions::new",
                    Vec::new(),
                    &[],
                    "fmt::FormattingOptions",
                    &opts_ty,
                    st,
                )?;
                let le = |b: &[u32]| {
                    b.iter()
                        .rev()
                        .fold(0u64, |acc, x| acc * 256 + u64::from(*x))
                };
                let bad = |what: &str| kind(k, &format!("a format_args! template {what}"));
                let mut pieces: Vec<String> = Vec::new();
                let mut at = 0usize;
                let mut next = 0u64;
                loop {
                    let n = *bytes.get(at).ok_or_else(|| bad("without its end"))?;
                    at += 1;
                    if n == 0 {
                        break;
                    }
                    let literal = |at: &mut usize, len: usize| -> Lift<String> {
                        let raw: Vec<u8> = bytes
                            .get(*at..*at + len)
                            .ok_or_else(|| bad("past its end"))?
                            .iter()
                            .map(|b| u8::try_from(*b).unwrap_or(0))
                            .collect();
                        *at += len;
                        let s =
                            String::from_utf8(raw).map_err(|_| bad("piece that is not UTF-8"))?;
                        lift::exact_string(&s)
                            .ok_or_else(|| bad("piece LexLean source cannot write"))
                    };
                    let none = format!("(ctor Option.none [{}])", ph.render());
                    if n < 0x80 {
                        let s = literal(&mut at, n as usize)?;
                        pieces.push(lift::tuple_value(
                            &piece_ty.clone().unwrap_or(Ty::Unit),
                            &[s, none],
                        ));
                        continue;
                    }
                    if n == 0x80 {
                        let len = le(bytes.get(at..at + 2).ok_or_else(|| bad("past its end"))?);
                        at += 2;
                        let s = literal(&mut at, len as usize)?;
                        pieces.push(lift::tuple_value(
                            &piece_ty.clone().unwrap_or(Ty::Unit),
                            &[s, none],
                        ));
                        continue;
                    }
                    if n < 0xC0 {
                        return Err(bad("byte core does not write"));
                    }
                    if n & 0x30 != 0 {
                        return Err(bad("whose width or precision is an argument"));
                    }
                    let mut values: Vec<String> = Vec::new();
                    for (i, w) in [(1u32, 4usize), (2, 2), (4, 2)] {
                        let j = values.len();
                        if n & i != 0 {
                            let v = le(bytes.get(at..at + w).ok_or_else(|| bad("past its end"))?);
                            at += w;
                            values.push(v.to_string());
                        } else {
                            values.push(self.field(&defaults.term, &opts_ty, j)?);
                        }
                    }
                    if n & 8 != 0 {
                        next = le(bytes.get(at..at + 2).ok_or_else(|| bad("past its end"))?);
                        at += 2;
                    }
                    if fields.len() != values.len() {
                        return Err(kind(k, "options of other fields than the template's"));
                    }
                    let named: Vec<(&String, String)> =
                        fields.iter().map(|(f, _)| f).zip(values).collect();
                    let opts = lift::struct_value(opts_name, &named, self.cx.types);
                    let ph_value = lift::tuple_value(ph, &[opts, next.to_string()]);
                    next += 1;
                    pieces.push(lift::tuple_value(
                        &piece_ty.clone().unwrap_or(Ty::Unit),
                        &[
                            "\"\"".to_string(),
                            format!("(ctor Option.some [{}] {ph_value})", ph.render()),
                        ],
                    ));
                }
                let pt_ty = piece_ty.clone().unwrap_or(Ty::Unit);
                let mut items = format!("(nil {})", pt_ty.render());
                for p in pieces.iter().rev() {
                    items = format!("(cons {p} {items})");
                }
                self.mention(dt);
                self.mention(&pt_ty);
                self.mention(ph);
                let term = lift::tuple_value(
                    dt,
                    &[
                        "(ctor Option.none [String])".to_string(),
                        items,
                        list.clone(),
                    ],
                );
                return Ok(Called::value(term));
            }
        }
        // A formatting method called through a trait object the table
        // models as a function (`<dyn fmt::Debug as fmt::Debug>::fmt`): the
        // function applied to the formatter's options and text, its result
        // the call's, and the text after the formatter's new text.
        if let ([(v, Ty::Fn(ins, out)), (f, Ty::Named(fname))], true) = (
            args.as_slice(),
            erase(callee).starts_with("<dyn ") && erase(callee).ends_with(">::fmt"),
        ) {
            let fields = self
                .cx
                .types
                .structs
                .get(fname)
                .cloned()
                .unwrap_or_default();
            if let ([(of, ot), (bf, Ty::Str)], [opts, Ty::Str], Ty::Tuple(outs)) =
                (fields.as_slice(), ins.as_slice(), &**out)
            {
                if ot.render() == opts.render() && outs.len() == 2 && outs[1] == Ty::Str {
                    let applied = format!("({v} (project {f} {of}) (project {f} {bf}))");
                    let after = lift::struct_value(
                        fname,
                        &[
                            (of, format!("(project {f} {of})")),
                            (bf, lift::tuple_item(out, &applied, 1)),
                        ],
                        self.cx.types,
                    );
                    self.mention(out);
                    return Ok(Called {
                        updates: BTreeMap::from([(1, after)]),
                        ..Called::value(lift::tuple_item(out, &applied, 0))
                    });
                }
            }
        }
        // An `Argument` of `format_args!`, where the table models it as a
        // function (`fmt::rt::Argument`): core's body reifies the method
        // that formats the value into a function pointer whose types it
        // erases. The lift's Argument is that method applied to the value,
        // a function of the options a placeholder gives and the text
        // written so far, to the method's result and the text after.
        if let (Some(link), Ty::Fn(ins, out), [(x, xt)]) = (self.cx.core, dt, args.as_slice()) {
            // A formatting trait's method given the value alone (a
            // coercion to its trait object) is that method.
            let partial = erase(callee)
                .strip_suffix(">::fmt")
                .and_then(|r| r.rsplit_once(" as "))
                .is_some_and(|(_, tr)| tr.starts_with("fmt::") || tr.starts_with("core::fmt::"));
            let reified: Vec<String> = if partial {
                vec![callee.to_string()]
            } else {
                link.instance(callee, texts, dest)
                    .map(|inst| {
                        inst.text
                            .lines()
                            .filter(|l| l.contains("ReifyFnPointer"))
                            .filter_map(|l| {
                                let rhs = l.split_once(" = ")?.1;
                                let end = rhs.find(" as for<").or_else(|| rhs.find(" as fn("))?;
                                Some(rhs[..end].trim().to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            if let ([opts, Ty::Str], Ty::Tuple(outs)) = (ins.as_slice(), &**out) {
                let formatter = ty("fmt::Formatter<'_>", self.cx.types)?;
                let fields = match &formatter {
                    Ty::Named(n) => self.cx.types.structs.get(n).cloned().unwrap_or_default(),
                    _ => Vec::new(),
                };
                if let ([method], [(of, ot), (bf, Ty::Str)], [result, Ty::Str]) =
                    (reified.as_slice(), fields.as_slice(), outs.as_slice())
                {
                    if ot.render() != opts.render() {
                        return Err(kind(k, "an Argument whose formatter holds other options"));
                    }
                    let o = self.fresh(Some("options"), usize::MAX);
                    let b = self.fresh(Some("written"), usize::MAX);
                    let Ty::Named(fname) = &formatter else {
                        return Err(kind(k, "a Formatter that is no struct"));
                    };
                    let fv = lift::struct_value(
                        fname,
                        &[(of, o.clone()), (bf, b.clone())],
                        self.cx.types,
                    );
                    let arg_texts = [
                        texts.first().cloned().flatten(),
                        Some("&mut fmt::Formatter<'_>".to_string()),
                    ];
                    let applied = self.call(
                        method,
                        vec![(x.clone(), xt.clone()), (fv, formatter.clone())],
                        &arg_texts,
                        // In halves, as above: not a signature for R5's scan.
                        concat!("result::Result", "<(), fmt::Error>"),
                        result,
                        st,
                    )?;
                    let (res, after) = match &applied.threads {
                        Some((rt, positions)) if positions.as_slice() == [1] => {
                            let f = lift::tuple_item(rt, &applied.term, 1);
                            (
                                lift::tuple_item(rt, &applied.term, 0),
                                self.field(&f, &formatter, 1)?,
                            )
                        }
                        Some(_) => {
                            return Err(kind(k, &format!("{method}, which threads its value")))
                        }
                        None => (applied.term.clone(), b.clone()),
                    };
                    self.mention(out);
                    self.mention(&formatter);
                    let body = lift::tuple_value(out, &[res, after]);
                    return Ok(Called::value(format!(
                        "(fun ({o} : {}) ({b} : String) => {body})",
                        opts.render()
                    )));
                }
            }
        }
        // In a library's scope, an intrinsic's model or the library's own
        // body.
        let mut unimplemented: Option<String> = None;
        if let Some(link) = self.cx.core {
            if let Some(path) = link
                .path(&erase(callee))
                .filter(|p| link.decls.intrinsics.contains(p))
            {
                // An intrinsic that reaches memory: the memory's model.
                let name = split_path(&path).last().copied().unwrap_or_default();
                // A pointer's metadata: a slice's length, a thin pointer's
                // unit.
                if let ("ptr_metadata", [(p, pt)]) = (name, args.as_slice()) {
                    if is_pointer(pt) {
                        return match dt {
                            Ty::Unit => Ok(Called::value("()".into())),
                            Ty::Nat(None) => Ok(Called::value(format!("(project {p} metadata)"))),
                            _ => Err(kind(k, &format!("ptr_metadata at {}", dt.render()))),
                        };
                    }
                }
                if MEMORY_INTRINSICS.contains(&name) {
                    let mut generics = turbofish(callee);
                    if generics.is_empty() {
                        let text = texts.first().cloned().flatten().unwrap_or_default();
                        generics.push(match name {
                            "offset" | "arith_offset" => text,
                            _ => pointee(&text)
                                .map(|(t, _)| t.to_string())
                                .unwrap_or_default(),
                        });
                    }
                    if let Some(term) = self.memory_intrinsic(name, &generics, &args, st)? {
                        return Ok(Called::value(term));
                    }
                }
                return self
                    .intrinsic(link, &path, callee, &args, dt, st)
                    .map(Called::value);
            }
            if let Some(inst) = link.instance(callee, texts, dest) {
                let krate = self.cx.krate.clone();
                let memory = st.memory.clone();
                return self.shared(inst, &krate, &args, dt, memory.as_deref());
            }
            // `<X as Trait>::m` where no impl of `Trait` in core is for
            // `X`'s type: the census bound a parameter that `Trait` bounds
            // to `X`, which does not implement it.
            let qualified = erased
                .strip_prefix('<')
                .and_then(|r| r.rsplit_once(">::"))
                .and_then(|(q, _)| q.split_once(" as "));
            if let Some((x, tr)) = qualified {
                let tr = tr.split('<').next().unwrap_or(tr);
                let tr = tr.rsplit("::").next().unwrap_or(tr);
                let head = |t: &str| {
                    t.split('<')
                        .next()
                        .unwrap_or(t)
                        .rsplit("::")
                        .next()
                        .unwrap_or(t)
                        .to_string()
                };
                let bound = x == super::core::CANONICAL;
                let is_for = |span: &str, ty: &str| {
                    if ty == "*" {
                        true
                    } else if ty == "_" {
                        link.wild.get(span).is_none_or(|w| w.contains(&head(x)))
                    } else {
                        head(ty) == head(x)
                    }
                };
                // An impl of `Trait` for `X`, or a method `m` of an impl for
                // `X` of a trait a macro names by a metavariable.
                let method = split_path(&erased).last().copied().unwrap_or_default();
                let implemented = link
                    .impls
                    .iter()
                    .any(|(span, (t, ty))| t.as_deref() == Some(tr) && is_for(span, ty))
                    || link.lasts.get(method).into_iter().flatten().any(|p| {
                        let span = p
                            .rfind("<impl at ")
                            .map(|at| &p[at + "<impl at ".len()..])
                            .and_then(|a| Some(&a[..a.find('>')?]));
                        span.and_then(|sp| Some((sp, link.impls.get(sp)?)))
                            .is_some_and(|(sp, (t, ty))| {
                                t.as_deref() == Some("_") && is_for(sp, ty)
                            })
                    });
                if bound && !implemented {
                    unimplemented = Some(format!(
                        "{x} for a parameter bounded by {tr}, which {x} does not implement in core"
                    ));
                }
            }
        }
        // Otherwise std's own MIR, at the call's types. Where neither has
        // it, a parameter the census bound to a type its bound's trait has
        // no impl for in the bodies the link holds is the cause.
        let memory = st.memory.clone();
        match self.std_instance(callee, &args, texts, dest, dt, memory.as_deref())? {
            Some(called) => Ok(called),
            None => Err(match unimplemented {
                Some(why) => kind("Instantiation", &why),
                None => kind(k, &erase(callee)),
            }),
        }
    }

    /// The model of a call of the intrinsic `path`, by the signature core
    /// declares it with.
    fn intrinsic(
        &mut self,
        link: &super::link::CoreLink,
        path: &str,
        callee: &str,
        args: &[(String, Ty)],
        dt: &Ty,
        st: &mut State,
    ) -> Lift<String> {
        let k = spec::MirTerminatorKind::Call.path();
        let generics = link.bind(path, callee, None, &[], "").ok_or_else(|| {
            kind(
                k,
                &format!("{path}, an intrinsic its call does not instantiate"),
            )
        })?;
        let name = split_path(path).last().copied().unwrap_or(path);
        // The model the table names for it, at the types its call binds in
        // the order core declares its parameters.
        use super::intrinsic::Model as M;
        let model = super::intrinsic::TABLE
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, m)| *m);
        let order = link.decls.generics.get(path).cloned().unwrap_or_default();
        let types: Vec<&String> = order.iter().filter_map(|n| generics.get(n)).collect();
        let unmodeled = || kind(k, &format!("{path}, an intrinsic the table does not model"));
        match (model, args, types.as_slice()) {
            // `size_of::<T>() -> usize`, `size_of_val` of a sized `T`, and
            // `align_of::<T>() -> usize`: `T`'s size or ABI alignment in the
            // targets' data layouts, where they agree.
            (Some(m @ (M::Size | M::SizeOfVal | M::Align)), [] | [_], [t])
                if *dt == Ty::Nat(None) && (args.len() == 1) == (m == M::SizeOfVal) =>
            {
                let t = ty(t, self.cx.types)?;
                let l = super::layout::agreed(&t, self.cx.layouts)
                    .map_err(|r| kind(k, &format!("{path} of {}", super::layout::reason(&r))))?;
                Ok(if m == M::Align { l.align } else { l.size }.to_string())
            }
            (Some(M::RawEq), [(a, at), (b, bt)], [t]) if at == bt && *dt == Ty::Bool => {
                let n: Option<u32> = t
                    .strip_prefix('[')
                    .and_then(|r| r.strip_suffix(']'))
                    .and_then(|r| r.rsplit_once("; "))
                    .and_then(|(_, n)| n.parse().ok());
                match (at, n) {
                    (Ty::Nat(_) | Ty::Signed(_), _) => Ok(format!("({a} =? {b})")),
                    (Ty::List(e), Some(n)) if matches!(**e, Ty::Nat(_) | Ty::Signed(_)) => {
                        let e = e.render();
                        let held = self.fresh(Some("held"), usize::MAX);
                        let at = |v: &str, i: u32| {
                            format!("(match (prim index {v} {i} : (Option {e})) | Option.none => 0 | Option.some {held} => {held})")
                        };
                        let mut all = "true".to_string();
                        for i in (0..n).rev() {
                            all = format!("(({} =? {}) && {all})", at(a, i), at(b, i));
                        }
                        Ok(all)
                    }
                    _ => Err(unmodeled()),
                }
            }
            (Some(M::AlignOfVal), [_], [t]) if *dt == Ty::Nat(None) => {
                let item = match t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                    Some(item) if !item.contains(';') => item,
                    _ if *t == "str" => "u8",
                    _ => t,
                };
                let t = ty(item, self.cx.types)?;
                let l = super::layout::agreed(&t, self.cx.layouts)
                    .map_err(|r| kind(k, &format!("{path} of {}", super::layout::reason(&r))))?;
                Ok(l.align.to_string())
            }
            (Some(M::Identity), [(v, t)], _) if t == dt => Ok(v.clone()),
            (Some(M::Unit), _, _) if *dt == Ty::Unit => Ok("()".into()),
            // A validity assertion is the unit where `T` passes it: every
            // type the lift holds is inhabited; the all-zero pattern is a
            // number, `false`, the unit or `'\0'`; the all-`0x01` pattern
            // of `mem::uninitialized` is a number, `true` or the unit, but
            // no `char` (`0x01010101` is above `char::MAX`).
            (Some(m @ (M::Inhabited | M::ZeroValid | M::UninitValid)), [], [t])
                if *dt == Ty::Unit =>
            {
                let t = ty(t, self.cx.types)?;
                let number = matches!(t, Ty::Nat(_) | Ty::Int(_) | Ty::Signed(_) | Ty::Float(_));
                let valid = match m {
                    M::Inhabited => true,
                    M::ZeroValid => number || matches!(t, Ty::Bool | Ty::Unit | Ty::Char),
                    _ => number || matches!(t, Ty::Bool | Ty::Unit),
                };
                if valid {
                    Ok("()".into())
                } else {
                    Err(kind(k, &format!("{path} of {}, which panics", t.render())))
                }
            }
            // `transmute` between two types the lift holds alike.
            (Some(M::Transmute), [(v, t)], [_, _]) if t == dt => Ok(v.clone()),
            // `discriminant_value` of an enum whose variants set no
            // discriminant of their own: the index of `v`'s variant, read by
            // a match on it.
            (Some(M::Discriminant), [(v, vt)], [printed])
                if Int::of(dt, self.cx.widths).is_some() =>
            {
                let name = printed.split('<').next().unwrap_or(printed);
                let tail = format!("::{name}");
                // The discriminants its source sets, where it sets any.
                let set = link
                    .decls
                    .structs
                    .iter()
                    .find(|s| s.explicit && (s.path == name || s.path.ends_with(&tail)));
                let values = match set {
                    Some(s) => Some(s.values.clone().ok_or_else(|| {
                        kind(
                            k,
                            &format!("{path} of {printed}, whose discriminants are no literals"),
                        )
                    })?),
                    None => None,
                };
                let int = Int::of(dt, self.cx.widths).ok_or_else(unmodeled)?;
                let ctors = match (vt, values.is_some()) {
                    // The source's own variants, in order, where the lift's
                    // enum holds them (as `CmpOrdering` holds `Ordering`'s).
                    (Ty::Named(n), true) => self
                        .cx
                        .types
                        .enums
                        .get(n)
                        .ok_or_else(unmodeled)?
                        .iter()
                        .map(|v| (v.clone(), format!("{n}.{v}"), Vec::new()))
                        .collect(),
                    _ => constructors(vt, self.cx.types, self.cx.explicit)?,
                };
                if values.as_ref().is_some_and(|vs| vs.len() != ctors.len()) {
                    // A variant the source declares under a `cfg` the
                    // census's target leaves out.
                    return Err(kind(
                        k,
                        &format!(
                            "{path} of {printed}, whose variants its source declares by target"
                        ),
                    ));
                }
                let mut arms = Vec::new();
                for (i, (variant, ctor, fields)) in ctors.iter().enumerate() {
                    let n = match &values {
                        Some(vs) => Some(super::core::discriminant(vs[i], int.signed)),
                        None => i128::try_from(i).ok(),
                    };
                    let value = n
                        .and_then(|n| super::int::literal(n, int))
                        .ok_or_else(unmodeled)?;
                    let mut pattern = ctor.clone();
                    for j in 0..fields.len() {
                        pattern.push(' ');
                        pattern.push_str(
                            &self.fresh(Some(&format!("{}{j}", variant.to_lowercase())), j),
                        );
                    }
                    arms.push(format!("| {pattern} => {value}"));
                }
                self.mention(vt);
                Ok(format!("(match {v} {})", arms.join(" ")))
            }
            // A transmute between types of one layout: the scalar the
            // source wraps, rewrapped as the destination wraps it. Into an
            // `Option` of a pattern-typed wrapper, the scalar's niche is
            // `None`; out of one, `None` is the niche.
            //
            // Between fixed-width integers and arrays of them, the bits are
            // the elements' in the targets' byte order: element `i` of `n`
            // holds the bits from `e * i` (little-endian) or from
            // `e * (n - 1 - i)` (big-endian), `e` its width. An array's
            // length is its printed type's; a scalar side's width is both.
            (Some(M::Transmute), [(v, st)], [ps, pd]) => {
                let unrelated = || {
                    kind(
                        k,
                        &format!(
                            "{path} from {} to {}, of layouts the lift does not relate",
                            st.render(),
                            dt.render()
                        ),
                    )
                };
                if let Some(t) = self.transmuted(v, st, dt) {
                    return Ok(t);
                }
                // (total bits, the element width of an array side).
                let shape = |t: &Ty, printed: &str| match t {
                    Ty::List(e) => {
                        let e = fixed(e)?;
                        let n: u32 = printed
                            .strip_prefix('[')?
                            .strip_suffix(']')?
                            .rsplit_once("; ")?
                            .1
                            .parse()
                            .ok()?;
                        Some((e.checked_mul(n)?, Some(e)))
                    }
                    // A scalar's width is its size in every target's layout.
                    _ => {
                        let size = super::layout::agreed(t, self.cx.layouts).ok()?.size;
                        let w = u32::try_from(size.checked_mul(u64::from(u8::BITS))?).ok()?;
                        (Int::of(t, self.cx.widths)?.held == w).then_some((w, None))
                    }
                };
                let orders: BTreeSet<bool> = self.cx.layouts.iter().map(|l| l.big_endian).collect();
                let orders: Vec<bool> = orders.into_iter().collect();
                let (Some((sw, se)), Some((dw, de)), [big]) =
                    (shape(st, ps), shape(dt, pd), orders.as_slice())
                else {
                    return Err(unrelated());
                };
                if sw != dw || (se.is_none() && de.is_none()) {
                    return Err(unrelated());
                }
                // The shift of element `i` of the `n` of width `e`.
                let big = *big;
                let shift = |i: u32, n: u32, e: u32| if big { e * (n - 1 - i) } else { e * i };
                let whole = match (se, st) {
                    (Some(e), Ty::List(et)) => {
                        let n = sw / e;
                        let et = et.render();
                        let held = self.fresh(Some("held"), usize::MAX);
                        let mut sum = "0".to_string();
                        for i in 0..n {
                            sum = format!(
                                "({sum} + ((match (prim index {v} {i} : (Option {et})) | Option.none => 0 | Option.some {held} => {held}) * {}))",
                                super::int::pow2(shift(i, n, e))
                            );
                        }
                        sum
                    }
                    _ => v.clone(),
                };
                match (de, dt) {
                    (Some(e), Ty::List(et)) => {
                        let n = dw / e;
                        // An array source's sum, bound once.
                        let bits = self.fresh(Some("bits"), usize::MAX);
                        let (bound, whole) = match se {
                            Some(_) => (Some(whole), bits.clone()),
                            None => (None, whole),
                        };
                        let mut out = format!("(nil {})", et.render());
                        for i in (0..n).rev() {
                            out = format!(
                                "(cons (prim remainder (prim quotient {whole} {} 0 : Nat) {} 0 : Nat) {out})",
                                super::int::pow2(shift(i, n, e)),
                                super::int::pow2(e)
                            );
                        }
                        Ok(match bound {
                            Some(sum) => format!("(let {bits} : Nat := {sum}; {out})"),
                            None => out,
                        })
                    }
                    _ => Ok(whole),
                }
            }
            // `three_way_compare` of two integers: `Less` where `a < b` by
            // the type's order, `Equal` where they are equal.
            (Some(M::Compare), [(a, at), (b, bt)], [_])
                if at == bt && *dt == Ty::Named(crate::order::ORDERING.to_string()) =>
            {
                let int = Int::of(at, self.cx.widths).ok_or_else(unmodeled)?;
                let less = super::int::less(a, b, int);
                let v = crate::order::ordering_value;
                Ok(format!(
                    "(if {less} then {} else (if ({a} =? {b}) then {} else {}))",
                    v("Less"),
                    v("Equal"),
                    v("Greater")
                ))
            }
            // An integer operation, by its rule in (bits, signed).
            (Some(M::Int), [(_, t), ..], _) if Int::of(t, self.cx.widths).is_some() => {
                self.int_intrinsic(path, name, args, st)
            }
            _ => Err(unmodeled()),
        }
    }

    /// One of core's integer intrinsics on integer values: the rule
    /// `int::intrinsic` gives in (bits, signed), or for `unchecked_*` and
    /// `exact_div` the MIR operator's own, each with core's precondition as
    /// an obligation.
    fn int_intrinsic(
        &mut self,
        path: &str,
        name: &str,
        args: &[(String, Ty)],
        st: &mut State,
    ) -> Lift<String> {
        use spec::MirBinOp as B;
        let k = spec::MirTerminatorKind::Call.path();
        let bad = || {
            let ts: Vec<String> = args.iter().map(|(_, t)| t.render()).collect();
            kind(k, &format!("{path} on {}", ts.join(" and ")))
        };
        let t0 = args.first().map(|(_, t)| t.clone()).ok_or_else(bad)?;
        let int = Int::of(&t0, self.cx.widths).ok_or_else(bad)?;
        let pow2 = super::int::pow2;
        let op = match name {
            "unchecked_add" => Some(B::AddUnchecked),
            "unchecked_sub" => Some(B::SubUnchecked),
            "unchecked_mul" => Some(B::MulUnchecked),
            "unchecked_div" | "exact_div" => Some(B::Div),
            "unchecked_rem" => Some(B::Rem),
            "unchecked_shl" => Some(B::ShlUnchecked),
            "unchecked_shr" => Some(B::ShrUnchecked),
            _ => None,
        };
        if let Some(op) = op {
            let [(x, xt), (y, yt)] = args else {
                return Err(bad());
            };
            match op {
                B::Div | B::Rem => {
                    self.obligation(st, &format!("(({y} =? 0) = false)"), k);
                    if int.signed {
                        let (h, m) = (pow2(int.held - 1), pow2(int.held));
                        self.obligation(
                            st,
                            &format!(
                                "((({x} =? {h}) && ({y} =? (prim subtract {m} 1 : Nat))) = false)"
                            ),
                            k,
                        );
                    }
                }
                B::ShlUnchecked | B::ShrUnchecked => {
                    self.obligation(st, &format!("(({y} <? {}) = true)", int.check), k);
                }
                _ => {}
            }
            let Val::V(value, _) =
                self.binary_on(op, x.clone(), xt.clone(), y.clone(), yt.clone(), st)?
            else {
                return Err(bad());
            };
            if name == "exact_div" {
                let Val::V(r, _) =
                    self.binary_on(B::Rem, x.clone(), xt.clone(), y.clone(), yt.clone(), st)?
                else {
                    return Err(bad());
                };
                self.obligation(st, &format!("({r} = 0)"), k);
            }
            return Ok(value);
        }
        // Two operands of one type, but for a rotation's amount.
        if args.len() == 2 && !name.starts_with("rotate_") && args[1].1 != t0 {
            return Err(bad());
        }
        let terms: Vec<&str> = args.iter().map(|(v, _)| v.as_str()).collect();
        let applied = super::int::intrinsic(name, &terms, int).ok_or_else(|| {
            kind(
                k,
                &format!(
                    "{path} on {}, whose width differs between the declared targets",
                    t0.render()
                ),
            )
        })?;
        for u in &applied.uses {
            self.use_helper(u);
        }
        for r in &applied.requires {
            self.obligation(st, r, k);
        }
        Ok(match applied.overflow {
            None => applied.value,
            Some(over) => {
                let pair = self.cx.types.tuple(vec![t0, Ty::Bool]);
                self.mention(&pair);
                lift::tuple_value(&pair, &[applied.value, over])
            }
        })
    }

    /// `v` of type `from` as a value of type `to` of the same layout.
    fn transmuted(&mut self, v: &str, from: &Ty, to: &Ty) -> Option<String> {
        let types = self.cx.types;
        // The scalar inside the single-field structs a type nests, and the
        // structs and fields, outermost first.
        let peel = |t: &Ty| {
            let mut chain: Vec<(String, String)> = Vec::new();
            let mut at = t.clone();
            while let Ty::Named(n) = &at {
                match types.structs.get(n).map(Vec::as_slice) {
                    Some([(f, inner)]) => {
                        chain.push((n.clone(), f.clone()));
                        at = inner.clone();
                    }
                    _ => break,
                }
            }
            (at, chain)
        };
        let one = |a: &Ty, b: &Ty| {
            a == b || (a.render() == b.render() && fixed(a).is_some() && fixed(a) == fixed(b))
        };
        let unwrap = |v: &str, chain: &[(String, String)]| {
            chain.iter().fold(v.to_string(), |acc, (n, f)| {
                if types.boxed(n) {
                    format!("({} {acc})", Types::accessor(n, f))
                } else {
                    format!("(project {acc} {f})")
                }
            })
        };
        let wrap = |v: &str, chain: &[(String, String)]| {
            chain.iter().rev().fold(v.to_string(), |acc, (n, f)| {
                lift::struct_value(n, &[(f, acc)], types)
            })
        };
        let (fs, fc) = peel(from);
        let (ts, tc) = peel(to);
        if one(&fs, &ts) {
            return Some(wrap(&unwrap(v, &fc), &tc));
        }
        // The niche of the innermost pattern-typed struct of a chain, the
        // same on every declared target.
        let niche_of = |f: &Self, chain: &[(String, String)]| -> Option<u128> {
            let (n, _) = chain.last()?;
            let ranges = f.valid_ranges(n)?;
            let niches: Vec<u128> = ranges
                .iter()
                .map(|(s, e, m)| niche(*s, *e, *m))
                .collect::<Option<_>>()?;
            let first = *niches.first()?;
            niches.iter().all(|x| *x == first).then_some(first)
        };
        // A unit enum whose source sets its discriminants, and an integer:
        // each variant is its discriminant (`Alignment`'s
        // `AlignmentEnum`, `ascii::Char`). The declaration is the one whose
        // variants the lift's enum holds, of the declarations `cfg` picks
        // between; an integer no variant has is no value of the enum, and
        // takes the last.
        let discriminants = |t: &Ty, signed: bool| -> Option<(String, Vec<(String, i128)>)> {
            let Ty::Named(n) = t else { return None };
            let variants = types.enums.get(n)?;
            let decl = self.cx.core?.decls.structs.iter().find(|s| {
                let mut parts = vec!["core"];
                parts.extend(s.path.split("::"));
                s.explicit
                    && s.variants.as_ref() == Some(variants)
                    && crate::lex::pascal(&parts) == *n
            })?;
            let values = decl.values.as_ref()?;
            Some((
                n.clone(),
                variants
                    .iter()
                    .cloned()
                    .zip(values.iter().map(|v| super::core::discriminant(*v, signed)))
                    .collect(),
            ))
        };
        if let Some((int, (n, table))) = Int::of(&ts, self.cx.widths)
            .and_then(|int| Some((int, discriminants(&fs, int.signed)?)))
        {
            let held = unwrap(v, &fc);
            let mut arms = Vec::new();
            for (variant, value) in &table {
                arms.push(format!(
                    "| {n}.{variant} => {}",
                    super::int::literal(*value, int)?
                ));
            }
            return Some(wrap(&format!("(match {held} {})", arms.join(" ")), &tc));
        }
        if let Some((int, (n, table))) = Int::of(&fs, self.cx.widths)
            .and_then(|int| Some((int, discriminants(&ts, int.signed)?)))
        {
            let held = unwrap(v, &fc);
            let ((last, _), rest) = table.split_last()?;
            let mut out = format!("{n}.{last}");
            for (variant, value) in rest.iter().rev() {
                let lit = super::int::literal(*value, int)?;
                out = format!("(if ({held} =? {lit}) then {n}.{variant} else {out})");
            }
            return Some(wrap(&out, &tc));
        }
        match (from, to) {
            // `&str` read as `&[u8]`: a `str` is laid out as its UTF-8
            // encoding, which the prelude's `textBytes` gives byte by byte.
            (Ty::Str, Ty::List(b)) if **b == Ty::Nat(Some(u8::BITS)) => {
                Some(format!("({}::textBytes {v})", lift::PRELUDE))
            }
            // `&[u8]` read as `&str`, whose bytes the caller vouches are
            // UTF-8 (`from_utf8_unchecked`): the text they decode to.
            (Ty::List(b), Ty::Str) if **b == Ty::Nat(Some(u8::BITS)) => {
                Some(decoded(&format!("({}::shaBytesOf {v})", lift::PRELUDE)))
            }
            // A `u32` read as a `char`, a scalar value the caller vouches
            // for (`from_u32_unchecked`): the text of its UTF-8 encoding.
            (Ty::Nat(Some(u32::BITS)), Ty::Char) => Some(decoded(&utf8_encoded(v))),
            (_, Ty::Option(inner)) => {
                let (is, ic) = peel(inner);
                if !one(&fs, &is) || !fc.is_empty() {
                    return None;
                }
                let n = niche_of(self, &ic)?;
                let i = inner.render();
                Some(format!(
                    "(if (prim equal {v} {n} : Bool) then (ctor Option.none [{i}]) else (ctor Option.some [{i}] {}))",
                    wrap(v, &ic)
                ))
            }
            (Ty::Option(inner), _) => {
                let (is, ic) = peel(inner);
                if !one(&is, &ts) || !tc.is_empty() {
                    return None;
                }
                let n = niche_of(self, &ic)?;
                let held = self.fresh(Some("held"), usize::MAX);
                Some(format!(
                    "(match {v} | Option.none => {n} | Option.some {held} => {})",
                    unwrap(&held, &ic)
                ))
            }
            _ => None,
        }
    }

    /// The definition an instance of a library body is lowered to, shared
    /// by its callers and named by its path and the types it binds, applied
    /// to the call's arguments; a definition of no parameters whose value
    /// is a literal is that literal.
    fn shared(
        &mut self,
        inst: stdlib::Instance,
        root: &str,
        args: &[(String, Ty)],
        dt: &Ty,
        memory: Option<&str>,
    ) -> Lift<Called> {
        let k = spec::MirTerminatorKind::Call.path();
        let stdlib::Instance { path, text, bound } = inst;
        // Named by the types as MIR prints them, so instances at `u8` and
        // `u64`, whose overflow obligations differ, are two definitions.
        let mut tags = String::new();
        for (_, b) in &bound {
            // A const parameter's value, `40`, names the instance; a type
            // parameter's type must lift.
            if !is_const_argument(b) {
                ty(b, self.cx.types)?;
            }
            tags.push_str(&crate::lex::pascal(&[b.as_str()]));
        }
        let segs: Vec<&str> = std::iter::once(root).chain(split_path(&path)).collect();
        let name = format!("{}{tags}", crate::lex::camel(&segs));
        if self.cx.active.contains(&name) || name == self.name {
            return Err(kind(
                k,
                &format!("{path}, which {root} defines through itself"),
            ));
        }
        let body = super::parse::bodies(&text)
            .into_iter()
            .next()
            .ok_or_else(|| kind(k, &format!("{path}, whose body does not read")))?;
        if !self.aux.contains_key(&name) {
            let mut active = self.cx.active.to_vec();
            active.push(name.clone());
            let cx = self.cx.within(&active);
            let lowered = lower(&cx, &body, &name)
                .map_err(|r| kind(k, &format!("{path}: {root}'s MIR stops at {}", r.detail())))?;
            for d in lowered.decls {
                self.aux.insert(d.name().to_string(), d);
            }
            for (n, d) in lowered.aux {
                self.aux.entry(n).or_insert(d);
            }
            self.calls.extend(lowered.calls);
            self.types.extend(lowered.types);
        }
        let Some(Decl::Definition {
            parameters,
            result,
            body: value,
            ..
        }) = self.aux.get(&name)
        else {
            return Err(kind(
                k,
                &format!("{path}, whose translation has no definition"),
            ));
        };
        if let (true, Expr::Flat(v)) = (parameters.is_empty(), value) {
            if v == "true" || v == "false" || v.chars().all(|c| c.is_ascii_digit()) {
                return Ok(Called::value(v.clone()));
            }
        }
        let (parameters, result) = (parameters.clone(), result.clone());
        self.instance_called(&name, &path, &body, &parameters, &result, args, dt, memory)
    }

    /// The call of an instance's definition: its arguments, then the
    /// memory where it takes it; its tuple where it gives back `&mut`
    /// arguments or the memory, `_0`'s value first.
    #[allow(clippy::too_many_arguments)]
    fn instance_called(
        &mut self,
        name: &str,
        path: &str,
        body: &IrBody,
        parameters: &[(String, String)],
        result: &str,
        args: &[(String, Ty)],
        dt: &Ty,
        memory: Option<&str>,
    ) -> Lift<Called> {
        let k = spec::MirTerminatorKind::Call.path();
        let at = |t: &Ty| t.render();
        let takes_memory = parameters
            .last()
            .is_some_and(|(_, t)| t == super::memory::MEMORY);
        let mut args: Vec<(String, Ty)> = args.to_vec();
        if takes_memory {
            let m = memory.ok_or_else(needs_memory)?;
            args.push((m.to_string(), super::memory::memory_ty()));
        }
        // A body that threads `&mut` parameters returns them after `_0`,
        // then the memory.
        let positions: Vec<usize> = threaded(&body.locals, body.arity, false, self.cx.types)
            .into_iter()
            .filter(|&l| {
                !(parameters.get(l - 1).is_some_and(|(_, t)| t == POINTER)
                    && body
                        .locals
                        .get(l)
                        .and_then(|t| referent(t))
                        .is_some_and(|r| r.starts_with('[') || r == "str"))
            })
            .map(|l| l - 1)
            .collect();
        // A borrow the callee takes the address of: the caller's place's
        // address, which the call is made again with.
        let addressed: Vec<usize> = (0..body.arity)
            .filter(|&i| {
                parameters.get(i).is_some_and(|(_, t)| t == POINTER)
                    && body
                        .locals
                        .get(i + 1)
                        .is_some_and(|t| referent(t).is_some())
            })
            .collect();
        // A `&mut` argument gives back the value it borrows, also where its
        // address is passed.
        let mut rest: Vec<Ty> = Vec::new();
        for &i in &positions {
            if addressed.contains(&i) {
                let text = referent(&body.locals[i + 1]).unwrap_or_default();
                rest.push(ty(&text, self.cx.types)?);
            } else if let Some((_, t)) = args.get(i) {
                rest.push(t.clone());
            }
        }
        if takes_memory {
            rest.push(super::memory::memory_ty());
        }
        let full = threaded_result(dt, &rest, self.cx.types);
        if addressed
            .iter()
            .any(|&i| args.get(i).is_some_and(|(_, t)| !is_pointer(t)))
        {
            self.wants_address = addressed;
            return Err(needs_address());
        }
        if parameters.len() != args.len()
            || parameters
                .iter()
                .zip(&args)
                .any(|((_, p), (_, a))| *p != at(a))
            || result != at(&full)
        {
            return Err(kind(k, &format!("{path}, at other types than the call's")));
        }
        self.calls.insert(name.to_string());
        let a: Vec<String> = args.iter().map(|(v, _)| v.clone()).collect();
        let term = if a.is_empty() {
            name.to_string()
        } else {
            format!("({name} {})", a.join(" "))
        };
        // A callee that returns a borrow into its one `&mut` argument: its
        // backward function, lowered from the same body. A body that has
        // none returns a borrow no write through reaches.
        let returns_borrow = body.locals.first().is_some_and(|t| {
            threaded(&[String::new(), t.clone()], 1, false, self.cx.types).len() == 1
        });
        let backward = match (returns_borrow, positions.is_empty(), takes_memory) {
            (true, false, false) => {
                let back = format!("{name}Back");
                if !self.aux.contains_key(&back) {
                    let mut active = self.cx.active.to_vec();
                    active.push(back.clone());
                    let mut cx = self.cx.within(&active);
                    cx.backward = true;
                    if let Ok(lowered) = lower(&cx, body, &back) {
                        for d in lowered.decls {
                            self.aux.insert(d.name().to_string(), d);
                        }
                        for (n, d) in lowered.aux {
                            self.aux.entry(n).or_insert(d);
                        }
                        self.calls.extend(lowered.calls);
                        self.types.extend(lowered.types);
                    }
                }
                let rt = (rest.len() > 1).then(|| self.cx.types.tuple(rest.clone()));
                self.aux.contains_key(&back).then(|| {
                    self.calls.insert(back.clone());
                    (
                        std::iter::once(back)
                            .chain(a.iter().cloned())
                            .collect::<Vec<_>>()
                            .join(" "),
                        positions.clone(),
                        rt,
                    )
                })
            }
            _ => None,
        };
        Ok(Called {
            threads: (!rest.is_empty()).then_some((full, positions)),
            memory: takes_memory,
            backward,
            ..Called::value(term)
        })
    }

    /// std's own body of `callee`, instantiated at the call's types and
    /// lowered as one definition its callers share: `None` when the table
    /// does not lift the callee.
    fn std_instance(
        &mut self,
        callee: &str,
        args: &[(String, Ty)],
        texts: &[Option<String>],
        dest: &str,
        dt: &Ty,
        memory: Option<&str>,
    ) -> Lift<Option<Called>> {
        let k = spec::MirTerminatorKind::Call.path();
        let Some(stdlib::Instance { path, text, bound }) = stdlib::instance(callee, texts, dest)
        else {
            return Ok(None);
        };
        // Named by the types as MIR prints them, so instances at `u8` and
        // `u64`, whose overflow obligations differ, are two definitions.
        let mut tags = String::new();
        for (_, b) in &bound {
            // A const parameter's value, `40`, names the instance; a type
            // parameter's type must lift.
            if !is_const_argument(b) {
                ty(b, self.cx.types)?;
            }
            tags.push_str(&crate::lex::pascal(&[b.as_str()]));
        }
        let segs: Vec<&str> = std::iter::once("std").chain(path.split("::")).collect();
        let name = format!("{}{tags}", crate::lex::camel(&segs));
        if self.cx.active.contains(&name) {
            return Err(kind(
                k,
                &format!("{path}, which std defines through itself"),
            ));
        }
        let body = super::parse::bodies(&text)
            .into_iter()
            .next()
            .ok_or_else(|| kind(k, &format!("{path}, whose vendored body does not read")))?;
        if !self.aux.contains_key(&name) {
            let mut active = self.cx.active.to_vec();
            active.push(name.clone());
            let cx = self.cx.within(&active);
            let lowered = lower(&cx, &body, &name)
                .map_err(|r| kind(k, &format!("{path}: std's MIR stops at {}", r.detail())))?;
            for d in lowered.decls {
                self.aux.insert(d.name().to_string(), d);
            }
            for (n, d) in lowered.aux {
                self.aux.entry(n).or_insert(d);
            }
            self.calls.extend(lowered.calls);
            self.types.extend(lowered.types);
        }
        let Some(Decl::Definition {
            parameters, result, ..
        }) = self.aux.get(&name)
        else {
            return Err(kind(
                k,
                &format!("{path}, whose translation has no definition"),
            ));
        };
        let (parameters, result) = (parameters.clone(), result.clone());
        self.instance_called(&name, &path, &body, &parameters, &result, args, dt, memory)
            .map(Some)
    }
}

/// The standard library's operations on values that the lift's types model
/// exactly. Generic arguments are erased before matching; every other call
/// is residue naming its callee.
fn std_call(
    callee: &str,
    args: &[(String, Ty)],
    dt: &Ty,
    f: &mut Fun<'_, '_>,
    st: &mut State,
) -> Option<String> {
    let path = erase(callee);
    let segs = split_path(&path);
    let last = *segs.last()?;
    let one = || args.first().map(|(v, _)| v.clone());
    let same = |a: &Ty, b: &Ty| a.render() == b.render();
    match (last, args) {
        // Value-preserving conversions and borrows.
        ("clone" | "deref" | "as_str" | "to_string" | "to_owned" | "into" | "from" | "as_ref" | "borrow" | "new"
            | "as_slice" | "to_vec" | "as_deref", [(v, t)])
            if same(t, dt)
                && (last != "new" || path.starts_with("std::boxed::Box"))
                && (last != "to_string" || matches!(t, Ty::Str))
                && (last != "from" && last != "into" || matches!(t, Ty::Str)) =>
        {
            Some(v.clone())
        }
        // `?` on a `Result`: `branch` is the value itself as a
        // `ControlFlow`, and `from_residual` of the error is that error.
        ("branch", [(v, t @ Ty::Outcome(..))]) if same(t, dt) && path.contains("Try>") => Some(v.clone()),
        ("from_residual", [(e, te)]) if path.contains("FromResidual>") => match dt {
            Ty::Outcome(ok, err) if same(err, te) => Some(format!(
                "(ctor Result.error [{}, {}] {e})",
                ok.render(),
                err.render()
            )),
            // `Option<Infallible>`, the residual of `?` on an option, holds
            // only `None`.
            Ty::Option(inner) if *te == Ty::Unit => {
                Some(format!("(ctor Option.none [{}])", inner.render()))
            }
            _ => None,
        },
        // `?` on an `Option`: `Continue` with the value, or `Break` with the
        // residual `None`, which is the unit.
        ("branch", [(v, Ty::Option(inner))]) if path.contains("Try>") => match dt {
            Ty::Outcome(ok, err) if same(ok, inner) && **err == Ty::Unit => {
                let held = f.fresh(Some("held"), usize::MAX);
                let args = format!("[{}, Unit]", ok.render());
                Some(format!(
                    "(match {v} | Option.none => (ctor Result.error {args} ()) | Option.some {held} => (ctor Result.ok {args} {held}))"
                ))
            }
            _ => None,
        },
        // A lossless conversion between naturals: `u32::from(b)` of a `u8`.
        ("from" | "into", [(v, t @ Ty::Nat(_))])
            if matches!(dt, Ty::Nat(_)) && f.max_bits(t) <= f.min_bits(dt) =>
        {
            Some(v.clone())
        }
        // `transmute` between a raw pointer and `usize`: its address one
        // way, a pointer without provenance the other (`core::ptr`).
        ("transmute", [(v, t)]) if is_pointer(t) && matches!(dt, Ty::Nat(None)) => Some(address(v)),
        ("transmute", [(v, Ty::Nat(None))]) if is_pointer(dt) => {
            Some(pointer_value("0", v, "0"))
        }
        // An iterator is its own `IntoIterator`.
        ("into_iter", [(v, t)]) if same(t, dt) && path.contains("IntoIterator>") => Some(v.clone()),
        ("new", []) if path == "std::string::String::new" => Some("\"\"".into()),
        ("new", []) if path.starts_with("std::vec::Vec") => match dt {
            Ty::List(e) => Some(format!("(nil {})", e.render())),
            _ => None,
        },
        ("len", [(v, Ty::List(e))]) if matches!(dt, Ty::Nat(None)) => {
            let len = format!("(prim length {v} : Nat)");
            f.length_fact(&len, e, st);
            Some(len)
        }
        ("is_some" | "is_none", [(v, Ty::Option(_))]) if path.starts_with("std::option::Option") => {
            let held = f.fresh(Some("held"), usize::MAX);
            let (none, some) = if last == "is_some" { ("false", "true") } else { ("true", "false") };
            Some(format!("(match {v} | Option.none => {none} | Option.some {held} => {some})"))
        }
        ("or", [(a, ta @ Ty::Option(_)), (b, tb)]) if path.starts_with("std::option::Option") && same(ta, tb) => {
            let held = f.fresh(Some("held"), usize::MAX);
            Some(format!("(match {a} | Option.none => {b} | Option.some {held} => {a})"))
        }
        // A slice's first and last items, `none` when it is empty.
        ("first" | "last", [(v, Ty::List(e))]) if matches!(dt, Ty::Option(d) if same(d, e)) => {
            let at = if last == "first" {
                "0".to_string()
            } else {
                format!("(prim subtract (prim length {v} : Nat) 1 : Nat)")
            };
            Some(format!("(prim index {v} {at} : (Option {}))", e.render()))
        }
        ("is_empty", [(v, Ty::List(_))]) => Some(format!("((prim length {v} : Nat) =? 0)")),
        ("is_empty", [(v, Ty::Str)]) => Some(format!("(prim equal {v} \"\" : Bool)")),
        ("eq" | "ne", [(a, ta), (b, tb)]) if matches!(ta, Ty::Str | Ty::Char) && same(ta, tb) => {
            let eq = format!("(prim equal {a} {b} : Bool)");
            Some(if last == "eq" { eq } else { format!("(not {eq})") })
        }
        ("eq" | "ne", [(a, Ty::Nat(_)), (b, Ty::Nat(_))]) => {
            let eq = format!("({a} =? {b})");
            Some(if last == "eq" { eq } else { format!("(not {eq})") })
        }
        ("min" | "max", [(a, ta @ Ty::Nat(_)), (b, tb)]) if same(ta, tb) => Some(if last == "min" {
            format!("(if ({b} <? {a}) then {b} else {a})")
        } else {
            format!("(if ({b} <? {a}) then {a} else {b})")
        }),
        ("saturating_sub", [(a, Ty::Nat(_)), (b, Ty::Nat(_))]) => Some(format!("(prim subtract {a} {b} : Nat)")),
        ("saturating_add", [(a, t @ Ty::Nat(_)), (b, _)]) => {
            let max = max_of(f.bits(t)?);
            if matches!(t, Ty::Nat(None)) && f.cx.widths.len() > 1 {
                return None;
            }
            Some(format!("(if ({max} <? ({a} + {b})) then {max} else ({a} + {b}))"))
        }
        ("wrapping_add", [(a, t @ Ty::Nat(Some(bits))), (b, _)]) => {
            let _ = t;
            let modulus = if *bits >= 128 { return None } else { (1u128 << bits).to_string() };
            Some(format!("(prim remainder ({a} + {b}) {modulus} 0 : Nat)"))
        }
        ("abs_diff", [(a, Ty::Nat(_)), (b, Ty::Nat(_))]) => {
            Some(format!("(if ({a} <? {b}) then (prim subtract {b} {a} : Nat) else (prim subtract {a} {b} : Nat))"))
        }
        _ => {
            let _ = one;
            None
        }
    }
}

/// A callee's path with its generic arguments erased: `Vec::<u8>::len` is
/// `Vec::len`, `<Vec<u8> as Clone>::clone` is `<Vec as Clone>::clone`.
pub(crate) fn erase(path: &str) -> String {
    let mut out = String::new();
    // For each open `<`: whether its contents are erased.
    let mut stack: Vec<bool> = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let erasing = stack.iter().any(|e| *e);
        match c {
            '<' => {
                // A qualified segment, `<impl X>` or `<X as T>`, is kept; the
                // generic arguments of a segment, `Vec::<u8>` or `Vec<u8>`,
                // are not.
                let rest: String = chars[i + 1..].iter().collect();
                let segment_start =
                    i == 0 || (i >= 2 && chars[i - 1] == ':' && chars[i - 2] == ':');
                let qualified = segment_start && (i == 0 || rest.starts_with("impl "));
                stack.push(!qualified);
                if !qualified && !erasing && out.ends_with("::") {
                    out.truncate(out.len() - 2);
                }
                if qualified && !erasing {
                    out.push(c);
                }
            }
            // `->` is an arrow, not a closing bracket.
            '>' if i > 0 && chars[i - 1] == '-' => {
                if !erasing {
                    out.push(c);
                }
            }
            '>' => {
                if let Some(e) = stack.pop() {
                    if !e && !stack.iter().any(|e| *e) {
                        out.push(c);
                    }
                }
            }
            _ if !erasing => out.push(c),
            _ => {}
        }
    }
    out
}

/// `(x <? y)` or `(x <=? y)` read back into its operator and operands.
fn comparison(cond: &str) -> Option<(&'static str, String, String)> {
    let inner = cond.strip_prefix('(')?.strip_suffix(')')?;
    let mut depth = 0i64;
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ' ' if depth == 0 => {
                for op in ["<?", "<=?"] {
                    let rest = &inner[i + 1..];
                    if let Some(y) = rest.strip_prefix(op).and_then(|r| r.strip_prefix(' ')) {
                        let x = &inner[..i];
                        if !x.is_empty() && !y.is_empty() {
                            return Some((op, x.to_string(), y.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// The text `bytes` (a `Bytes` term) decode to as UTF-8, where they are
/// UTF-8; the caller's precondition leaves no other case, which reads as
/// the empty text.
fn decoded(bytes: &str) -> String {
    format!("(match (prim utf8_decode {bytes} : (Option String)) | Option.none => \"\" | Option.some text => text)")
}

/// The UTF-8 encoding of the scalar value `code` (a `Nat` term), as a
/// `Bytes` term (RFC 3629, section 3): a code point below `2^7` is one
/// byte; below `2^11`, `2^16` and past them, two, three and four bytes,
/// the first holding the leading marker of its length and the high bits,
/// each later one the continuation marker and six bits more.
fn utf8_encoded(code: &str) -> String {
    /// The bits each continuation byte carries.
    const SIX: u32 = 6;
    /// Each shorter encoding's length in bytes, and the bits of a code
    /// point it holds.
    const LIMITS: [(u32, u32); 3] = [(1, 7), (2, 11), (3, 16)];
    /// The longest encoding's length in bytes.
    const LONGEST: u32 = 4;
    let byte = |n: String| format!("({}::shaByte {n})", lift::PRELUDE);
    // Bits `6 * k` up of `code`, and their low six.
    let shifted = |k: u32| match k {
        0 => code.to_string(),
        _ => format!("(prim quotient {code} {} 0 : Nat)", 1u64 << (SIX * k)),
    };
    let low = |k: u32| format!("(prim remainder {} {} 0 : Nat)", shifted(k), 1u64 << SIX);
    // A continuation byte: `10` and six bits.
    let more = |k: u32| byte(format!("({} + {})", 1u64 << (u8::BITS - 1), low(k)));
    // The `n`-byte encoding: its leading byte, `n` ones and a zero over
    // the high bits, then `n - 1` continuation bytes.
    let of = |n: u32| {
        let lead = (0..n).fold(0u64, |acc, i| acc | (1u64 << (u8::BITS - 1 - i)));
        let mut out = byte(format!("({lead} + {})", shifted(n - 1)));
        for k in (0..n - 1).rev() {
            out = format!("(prim append {out} {} : Bytes)", more(k));
        }
        out
    };
    let mut out = of(LONGEST);
    for (n, bits) in LIMITS.iter().rev() {
        let encoded = if *n == 1 {
            byte(code.to_string())
        } else {
            of(*n)
        };
        out = format!(
            "(if ({code} <? {}) then {encoded} else {out})",
            1u64 << bits
        );
    }
    out
}

/// The width of a fixed-width unsigned type. A `usize` has none: its range
/// differs between targets, so no theorem may assume the narrowest one.
fn fixed(t: &Ty) -> Option<u32> {
    match t {
        // A signed value's bit pattern is below `2^b` as well.
        Ty::Nat(Some(b)) | Ty::Signed(Some(b)) => Some(*b),
        _ => None,
    }
}

/// Whether `text` names `term`: as a whole word when it is a name.
fn term_mentions(text: &str, term: &str) -> bool {
    if term.starts_with('(') {
        return text.contains(term);
    }
    text.match_indices(term).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + term.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        !word(before) && !word(after)
    })
}

/// Hypotheses for the steps of a path.
fn hypotheses(path: &[MirStep]) -> Vec<String> {
    path.iter()
        .map(|s| match s {
            MirStep::If(c, b) => format!("({c} = {b})"),
            MirStep::Match(s, t) => format!("({s} = {t})"),
        })
        .collect()
}

/// `h1 -> (h2 -> ... goal)`.
fn implication(hyps: &[String], goal: &str) -> String {
    #[cfg(not(uorust_lift))]
    crate::certified::certified!(mir_lower_implication, hyps, goal);
    let mut out = goal.to_string();
    for h in hyps.iter().rev() {
        out = format!("({h} -> {out})");
    }
    out
}

/// The key a match binds a variant's fields under, of the place it
/// switches on. A local that holds the value a `&mut` borrows (a `&mut` parameter, a
/// lending local) is that value, so `(*_1)` and `_1` are the one place: a
/// match on `(*_1)` binds the fields a read through the borrow, of `_1`,
/// finds.
fn place_key(p: &IrPlace, held: bool) -> String {
    if held {
        let steps: Vec<&IrProjection> = p
            .projection
            .iter()
            .filter(|s| !matches!(s, IrProjection::Deref))
            .collect();
        return format!("{:?}{steps:?}", p.local);
    }
    format!("{p:?}")
}

/// The constructors of a type a discriminant switches over, in the order of
/// their discriminants: each variant, its LexLean constructor pattern head
/// and its fields' types.
#[allow(clippy::type_complexity)]
fn constructors(
    t: &Ty,
    types: &Types,
    explicit: &BTreeSet<String>,
) -> Lift<Vec<(String, String, Vec<Ty>)>> {
    let k = spec::MirRvalue::Discriminant.path();
    Ok(match t {
        Ty::Option(inner) => vec![
            ("None".into(), "Option.none".into(), Vec::new()),
            ("Some".into(), "Option.some".into(), vec![(**inner).clone()]),
        ],
        Ty::Outcome(ok, err) => vec![
            ("Ok".into(), "Result.ok".into(), vec![(**ok).clone()]),
            ("Err".into(), "Result.error".into(), vec![(**err).clone()]),
        ],
        Ty::Named(n) if explicit.contains(n) => {
            return Err(kind(
                k,
                &format!("{n}, whose variants set their discriminants"),
            ))
        }
        Ty::Named(n) => {
            if let Some(vs) = types.enums.get(n) {
                vs.iter()
                    .map(|v| (v.clone(), format!("{n}.{v}"), Vec::new()))
                    .collect()
            } else if let Some(vs) = types.datas.get(n) {
                vs.iter()
                    .map(|(v, fs)| {
                        (
                            v.clone(),
                            format!("{n}.{v}"),
                            fs.iter().map(|(_, t)| t.clone()).collect(),
                        )
                    })
                    .collect()
            } else {
                return Err(kind(k, &format!("of {n}, which is not an enum")));
            }
        }
        _ => return Err(kind(k, &format!("of {}", t.render()))),
    })
}

/// The constructor term a match arm's hypothesis equates the scrutinee with.
fn ctor_term(t: &Ty, ctor: &str, names: &[String]) -> String {
    let args = match t {
        Ty::Option(inner) => format!(" [{}]", inner.render()),
        Ty::Outcome(ok, err) => format!(" [{}, {}]", ok.render(), err.render()),
        _ => String::new(),
    };
    let fields: String = names.iter().map(|n| format!(" {n}")).collect();
    format!("(ctor {ctor}{args}{fields})")
}

fn place_reads(p: &IrPlace) -> Vec<IrLocal> {
    let mut out = vec![p.local];
    for s in &p.projection {
        if let IrProjection::Index(l) = s {
            out.push(*l);
        }
    }
    out
}

fn operand_reads(o: &IrOperand) -> Vec<IrLocal> {
    match o {
        IrOperand::Copy(p) | IrOperand::Move(p) => place_reads(p),
        _ => Vec::new(),
    }
}

fn rvalue_reads(rv: &IrRvalue) -> Vec<IrLocal> {
    match rv {
        IrRvalue::Use(o) | IrRvalue::Unary(_, o) | IrRvalue::Cast(_, o, _) => operand_reads(o),
        IrRvalue::Ref(_, p)
        | IrRvalue::RawPtr(_, p)
        | IrRvalue::MetadataPtr(p)
        | IrRvalue::CopyForDeref(p)
        | IrRvalue::Discriminant(p) => place_reads(p),
        IrRvalue::Binary(_, a, b) => {
            let mut out = operand_reads(a);
            out.extend(operand_reads(b));
            out
        }
        IrRvalue::Aggregate(_, ops) => ops.iter().flat_map(|(_, o)| operand_reads(o)).collect(),
        IrRvalue::Other(..) => Vec::new(),
    }
}

/// Whether a type as MIR prints it has no values.
fn uninhabited(text: &str) -> bool {
    matches!(
        text.trim(),
        "!" | "std::convert::Infallible" | "core::convert::Infallible"
    )
}

/// The one inhabited variant of an enum of std whose other variants hold an
/// uninhabited field.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Inhabited {
    /// Its discriminant and name.
    discriminant: i128,
    variant: &'static str,
    /// The type of its field, if it has one.
    field: Option<String>,
}

/// The inhabited variant of `text`, when exactly one of its variants is.
/// Whether a Rust type MIR prints is a tuple of one item, `(T,)`.
fn one_tuple(text: &str) -> bool {
    let Some(inner) = text
        .trim()
        .strip_prefix('(')
        .and_then(|t| t.strip_suffix(",)"))
    else {
        return false;
    };
    // `(A, B,)` is never printed; a top-level comma means more than one item.
    let mut depth = 0i32;
    for c in inner.replace("->", "").chars() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth -= 1,
            ',' if depth == 0 => return false,
            _ => {}
        }
    }
    true
}

/// The type a reference or box MIR prints points to: `T` of `&T`,
/// `&mut T` or `Box<T>`.
fn referent(text: &str) -> Option<String> {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix('&') {
        let rest = rest.trim_start();
        // A lifetime, `&'a T`, names no type.
        let rest = match rest.strip_prefix('\'') {
            Some(r) => r.split_once(' ').map_or("", |(_, r)| r),
            None => rest,
        };
        return Some(rest.strip_prefix("mut ").unwrap_or(rest).to_string());
    }
    t.strip_prefix("std::boxed::Box<")
        .or_else(|| t.strip_prefix("Box<"))
        .and_then(|r| r.strip_suffix('>'))
        .map(str::to_string)
}

fn inhabited_variant(text: &str) -> Option<Inhabited> {
    let segs = split_path(text.trim());
    if !matches!(segs.first(), Some(&("std" | "core"))) {
        return None;
    }
    let last = segs.last()?;
    let at = last.find('<')?;
    let name = &last[..at];
    let args: Vec<&str> = split_top(last[at + 1..].strip_suffix('>')?, ',')
        .into_iter()
        .filter(|a| !a.is_empty() && !a.starts_with('\''))
        .collect();
    let one = |discriminant, variant, field: &str| {
        Some(Inhabited {
            discriminant,
            variant,
            field: Some(field.to_string()),
        })
    };
    match (name, args.as_slice()) {
        ("Option", [x]) if uninhabited(x) => Some(Inhabited {
            discriminant: 0,
            variant: "None",
            field: None,
        }),
        ("Result", [x, e]) if uninhabited(x) && !uninhabited(e) => one(1, "Err", e),
        ("Result", [v, x]) if uninhabited(x) && !uninhabited(v) => one(0, "Ok", v),
        _ => None,
    }
}

/// A std type as std's own bodies print it, without its crate
/// (`slice::iter::Iter<'_, T>` inside core): the table's model of it.
fn std_printed(segs: &[&str], name: &str, args: &[Ty]) -> Option<Ty> {
    if segs.len() < 2 {
        return None;
    }
    let head: Vec<&str> = segs[..segs.len() - 1].to_vec();
    stdlib::model_type(&format!("{}::{name}", head.join("::")), args)
}

/// A type as MIR prints it, as the lift names it.
pub(crate) fn ty(text: &str, types: &Types) -> Lift<Ty> {
    let t = text.trim();
    let bad = || Err(Residue::Type(t.to_string()));
    // `!`, the type of no value: a place of it is never written, so its
    // stand-in is the unit, never observed (a call that returns it does not
    // return, and the path to it is proved unreachable).
    if t == "!" {
        return Ok(Ty::Unit);
    }
    // A pointee's metadata (`core::ptr::Pointee`): a slice's or a `str`'s
    // is its length, a `usize`; a sized type's is the unit. A trait
    // object's, a vtable, is residue.
    if let Some(rest) = t.strip_prefix('<') {
        // An associated type an impl sets to a struct of core's, by the
        // projection's type, trait and name, arguments and paths dropped.
        let key = rest.split_once(" as ").and_then(|(x, r)| {
            let (tr, assoc) = r.rsplit_once(">::")?;
            let tr = tr.split('<').next().unwrap_or(tr);
            Some(format!(
                "<{x} as {}>::{}",
                tr.rsplit("::").next().unwrap_or(tr),
                assoc.split('<').next().unwrap_or(assoc)
            ))
        });
        if let Some(n) = key.and_then(|k| types.aliases.get(&k)).filter(|n| {
            types.structs.contains_key(*n)
                || types.datas.contains_key(*n)
                || types.enums.contains_key(*n)
        }) {
            return Ok(Ty::Named(n.clone()));
        }
        if let Some((pointee_text, assoc)) = rest.rsplit_once(" as ") {
            let assoc = assoc.trim_start_matches("core::");
            if matches!(
                assoc,
                "ptr::metadata::Pointee>::Metadata" | "Pointee>::Metadata"
            ) {
                return match pointee(&format!("*const {pointee_text}")) {
                    Some((_, true)) => Ok(Ty::Nat(None)),
                    Some((p, false)) if !p.starts_with("dyn ") && types.core => {
                        // A type parameter could be unsized; a concrete type
                        // that lifts is sized.
                        ty(p, types).map(|_| Ty::Unit)
                    }
                    _ => bad(),
                };
            }
        }
    }
    // A raw pointer, where the scope declares the pointer struct: a typed
    // address with its provenance and metadata (see [`POINTER`]). A pointer
    // to a trait object, whose metadata is a vtable, is residue.
    if pointee(t).is_some() && types.structs.contains_key(POINTER) {
        return Ok(Ty::Named(POINTER.to_string()));
    }
    // A trait object of a trait the table models as a sink (`dyn
    // fmt::Write`): the value its writes build, its lifetimes and auto
    // traits dropped.
    if let Some(object) = t.strip_prefix("dyn ") {
        let trait_path = split_top(object, '+')
            .into_iter()
            .map(str::trim)
            .find(|b| !b.starts_with('\'') && !matches!(*b, "Send" | "Sync" | "Unpin"));
        if let Some(model) = trait_path.and_then(|p| stdlib::model_type(&format!("dyn {p}"), &[])) {
            return Ok(model);
        }
        return bad();
    }
    if t.starts_with('*') || t.starts_with("impl ") {
        return bad();
    }
    if let Some(rest) = t.strip_prefix('&') {
        // A lifetime is erased.
        let rest = match rest.strip_prefix('\'') {
            Some(r) => r.split_once(' ').map_or(r, |(_, r)| r),
            None => rest,
        };
        // A mutable borrow holds the value it borrows, as a shared one does.
        // A write through one is a write of the place it borrows: the
        // translation follows a borrow whose place it knows (`Val::Mut`, a
        // `&mut` parameter), and any other is residue where it is written.
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        if rest == "str" {
            return Ok(Ty::Str);
        }
        return ty(rest, types);
    }
    // A pattern type, `(u32) is 1..`, holds a value of its base type: the
    // values its pattern leaves out are its niche, not other values.
    if let Some((base, _)) = t.strip_prefix('(').and_then(|r| r.split_once(") is ")) {
        return ty(base, types);
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let parts = split_top(inner, ';');
        return Ok(Ty::List(Box::new(ty(parts[0], types)?)));
    }
    if t == "()" {
        return Ok(Ty::Unit);
    }
    // A closure is the struct of its captures, as Rust holds it: the unit,
    // the one value it captures, or the tuple of them.
    if t.starts_with("{closure@") {
        let Some(captures) = types.closures.get(t) else {
            return bad();
        };
        let mut ts = Vec::new();
        for c in captures {
            ts.push(ty(c, types)?);
        }
        return Ok(match ts.len() {
            0 => Ty::Unit,
            1 => ts.remove(0),
            _ => Ty::Tuple(ts),
        });
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<&str> = split_top(inner, ',')
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect();
        let mut ts = Vec::new();
        for p in &parts {
            ts.push(ty(p, types)?);
        }
        return match ts.len() {
            0 => Ok(Ty::Unit),
            1 => Ok(ts.remove(0)),
            _ => Ok(types.tuple(ts)),
        };
    }
    let segs = split_path(t);
    let last = segs.last().copied().unwrap_or(t);
    let (name, args_text) = match last.find('<') {
        Some(at) => (
            last[..at].trim_end_matches("::"),
            last[at + 1..].strip_suffix('>').unwrap_or(""),
        ),
        None => (last, ""),
    };
    // Its type arguments: neither a lifetime nor a const argument, a
    // length (`#` where the census binds a const parameter) or a value.
    let raw: Vec<&str> = split_top(args_text, ',')
        .into_iter()
        .filter(|a| !a.is_empty() && !a.starts_with('\'') && !is_const_argument(a))
        .collect();
    // A variant whose field is uninhabited never holds a value, so an enum
    // of std is the type of its one inhabited variant: `Result<T, E>` is `T`
    // where `E` is uninhabited and `E` where `T` is, and `Option<T>` holds
    // only `None` where `T` is, the unit. `ControlFlow<B, C>` is
    // `Result<C, B>`, `Continue` first as `Ok` is.
    if let Some(erased) = inhabited_variant(t) {
        return match erased.field {
            Some(f) => ty(&f, types),
            None => Ok(Ty::Unit),
        };
    }
    // An instance of a struct of core's own, in core's scope.
    if types.core {
        let erased = without_lifetimes(t);
        if let Some(n) = types
            .aliases
            .get(t)
            .or_else(|| types.aliases.get(&erased))
            .filter(|n| {
                types.structs.contains_key(*n)
                    || types.enums.contains_key(*n)
                    || types.datas.contains_key(*n)
            })
        {
            return Ok(Ty::Named(n.clone()));
        }
    }
    // A path of std's, or in core's own scope a path of core's: MIR prints
    // core's paths there without the crate.
    let std = segs.len() > 1 && (matches!(segs[0], "std" | "core" | "alloc") || types.core);
    match (name, raw.as_slice()) {
        ("ControlFlow", [b, c]) if std => {
            return Ok(Ty::Outcome(
                Box::new(ty(c, types)?),
                Box::new(ty(b, types)?),
            ))
        }
        // `ControlFlow<B>`, its continue value the unit by default.
        ("ControlFlow", [b]) if std => {
            return Ok(Ty::Outcome(Box::new(Ty::Unit), Box::new(ty(b, types)?)))
        }
        // `ManuallyDrop<T>` holds a `T`, which it only keeps from dropping;
        // a `MaybeUninit<T>` holds one or none (the Reference: reading it
        // uninitialized as a `T` is undefined), so it is an `Option<T>`,
        // `None` where uninitialized.
        ("ManuallyDrop", [t]) if std => return ty(t, types),
        // `NonNull<T>` and `Unique<T>` are a `*const T` that is not null
        // (the `NonNull` docs), which their constructors' obligations say.
        ("NonNull" | "Unique", [p]) if std || types.core => {
            return ty(&format!("*const {p}"), types)
        }
        ("MaybeUninit", [t]) if std => return Ok(Ty::Option(Box::new(ty(t, types)?))),
        _ => {}
    }
    // Every primitive integer, its signedness and width from its name: `u`
    // or `i`, then its bits (a power of two of whole bytes) or `size`.
    if let (1, true, Some(sign @ ('u' | 'i'))) = (segs.len(), raw.is_empty(), name.chars().next()) {
        let bits = match &name[1..] {
            "size" => Some(None),
            b if !b.starts_with('0') => b
                .parse::<u32>()
                .ok()
                .filter(|b| *b >= 8 && b.is_power_of_two())
                .map(Some),
            _ => None,
        };
        if let Some(bits) = bits {
            return Ok(if sign == 'i' {
                Ty::Signed(bits)
            } else {
                Ty::Nat(bits)
            });
        }
    }
    let mut args = Vec::new();
    for a in raw {
        args.push(ty(a, types)?);
    }
    Ok(match (name, args.as_slice()) {
        ("bool", []) => Ty::Bool,
        (f, []) if segs.len() == 1 && types.floats.contains_key(f) => Ty::Float(types.floats[f]),
        ("char", []) => Ty::Char,
        ("str", []) => Ty::Str,
        ("String", []) if std || segs.len() == 1 => Ty::Str,
        ("Option", [a]) => Ty::Option(Box::new(a.clone())),
        ("Result", [a, e]) => Ty::Outcome(Box::new(a.clone()), Box::new(e.clone())),
        ("Vec", [a]) => Ty::List(Box::new(a.clone())),
        ("Box", [a]) => a.clone(),
        // A std type whose representation bottoms out in raw pointers: the
        // table's model of it.
        // A std type the scope lifts as its own: `cmp::Ordering`.
        (n, []) if std && types.enums.contains_key(&types.resolve(n)) => {
            Ty::Named(types.resolve(n))
        }
        _ if std => {
            let head: Vec<&str> = segs[..segs.len() - 1].to_vec();
            let path = format!("{}::{name}", head.join("::"));
            // A model that names a struct of the crate's needs the scope
            // to declare it.
            return stdlib::model_type(&path, &args)
                .filter(|m| {
                    m.mentions().iter().all(|n| match n {
                        Ty::Named(n) => {
                            types.structs.contains_key(n) || types.enums.contains_key(n)
                        }
                        _ => true,
                    })
                })
                .ok_or_else(|| Residue::Type(t.to_string()));
        }
        (n, []) => {
            // A type of the crate by the full path MIR prints, then by name.
            let mut printed: Vec<&str> = segs[..segs.len() - 1].to_vec();
            printed.push(n);
            // MIR prints a type by its name alone where no other type has
            // it (`TryFromIntError`): the one alias whose path ends so.
            let tail = format!("::{n}");
            let mut ends = types
                .aliases
                .iter()
                .filter(|(k, _)| segs.len() == 1 && k.ends_with(&tail));
            let bare = match (ends.next(), ends.next()) {
                (Some((_, v)), None) => Some(v.clone()),
                _ => None,
            };
            let known = |r: &String| {
                types.enums.contains_key(r)
                    || types.structs.contains_key(r)
                    || types.datas.contains_key(r)
            };
            // The scope's own type of that name first: the bare alias only
            // where the name resolves to no type of the scope's.
            let r = types
                .aliases
                .get(&printed.join("::"))
                .cloned()
                .or_else(|| Some(types.resolve(n)).filter(known))
                .or(bare)
                .unwrap_or_else(|| types.resolve(n));
            if known(&r) {
                Ty::Named(r)
            } else {
                // In core's scope, a type the std table models, printed by
                // its name alone.
                if types.core {
                    if let Some(m) = stdlib::model_type(n, &args).filter(|m| {
                        m.mentions().iter().all(|x| match x {
                            Ty::Named(x) => {
                                types.structs.contains_key(x) || types.enums.contains_key(x)
                            }
                            _ => true,
                        })
                    }) {
                        return Ok(m);
                    }
                }
                return std_printed(&segs, name, &args).ok_or_else(|| Residue::Type(t.to_string()));
            }
        }
        _ => return std_printed(&segs, name, &args).ok_or_else(|| Residue::Type(t.to_string())),
    })
}

/// The residue kind of what the memory model does not hold.
const MEMORY_KIND: &str = "Memory";

/// The detail of the signal that a body reaches memory only through a
/// callee: its translation is made again with the memory threaded.
const NEEDS_MEMORY: &str = "a callee reaches memory";

fn needs_memory() -> Residue {
    kind(MEMORY_KIND, NEEDS_MEMORY)
}

/// The borrow parameters a raw pointer is taken of (`&raw const (*_1)`):
/// passed as the address of the place they lend, so that the pointer
/// reaches the caller's place. Every other parameter is passed by value.
pub(crate) fn address_params(body: &IrBody) -> BTreeSet<IrLocal> {
    let mut out = BTreeSet::new();
    for b in &body.blocks {
        for s in &b.statements {
            if let IrStatement::Assign(_, IrRvalue::RawPtr(_, p)) = s {
                let borrow = body
                    .locals
                    .get(p.local)
                    .is_some_and(|t| referent(t).is_some());
                if (1..=body.arity).contains(&p.local)
                    && borrow
                    && p.projection.first() == Some(&IrProjection::Deref)
                {
                    out.insert(p.local);
                }
            }
        }
    }
    out
}

/// The parameters passed by address (see [`address_params`]) that borrow
/// an array, a slice or a `str`: the memory holds their every write, and
/// no value of theirs is read whole, so none is given back.
pub(crate) fn unthreaded(body: &IrBody) -> BTreeSet<IrLocal> {
    address_params(body)
        .into_iter()
        .filter(|&l| {
            body.locals
                .get(l)
                .and_then(|t| referent(t))
                .is_some_and(|r| r.starts_with('[') || r == "str")
        })
        .collect()
}

/// The residue a call gives when its callee takes the addresses of some
/// of its arguments: the call is made again with those addresses.
const NEEDS_ADDRESS: &str = "a callee takes an argument's address";

fn needs_address() -> Residue {
    kind(MEMORY_KIND, NEEDS_ADDRESS)
}

fn is_needs_address(r: &Residue) -> bool {
    matches!(r, Residue::Mir(s) if *s == format!("{MEMORY_KIND}: {NEEDS_ADDRESS}"))
}

fn is_needs_memory(r: &Residue) -> bool {
    matches!(r, Residue::Mir(s) if *s == format!("{MEMORY_KIND}: {NEEDS_MEMORY}"))
}

/// The intrinsics that read or write memory, or compute a pointer with
/// the bounds of its allocation, by name.
const MEMORY_INTRINSICS: &[&str] = &[
    "read_via_copy",
    "write_via_move",
    "typed_swap_nonoverlapping",
    "copy_nonoverlapping",
    "copy",
    "offset",
    "arith_offset",
    "aggregate_raw_ptr",
    "volatile_load",
    "volatile_store",
];

/// The type arguments a callee's last segment states: `["u32"]` of
/// `read_via_copy::<u32>`.
fn turbofish(callee: &str) -> Vec<String> {
    let last = split_path(callee).last().copied().unwrap_or_default();
    let Some(at) = last.find("::<") else {
        return Vec::new();
    };
    let Some(inner) = last[at + 3..].strip_suffix('>') else {
        return Vec::new();
    };
    split_top(inner, ',')
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Whether a body takes an address, reads or writes through a raw pointer,
/// offsets one, or calls a memory intrinsic.
fn reaches_memory(body: &IrBody) -> bool {
    let raw = |l: IrLocal| body.locals.get(l).is_some_and(|t| pointee(t).is_some());
    let through = |p: &IrPlace| {
        (raw(p.local) && p.projection.first() == Some(&IrProjection::Deref))
            || p.projection.windows(2).any(|w| {
                matches!(w, [IrProjection::Field(_, fty), IrProjection::Deref] if pointee(fty).is_some())
            })
    };
    let operand =
        |o: &IrOperand| matches!(o, IrOperand::Copy(p) | IrOperand::Move(p) if through(p));
    body.blocks.iter().any(|b| {
        b.statements.iter().any(|s| match s {
            IrStatement::Assign(p, rv) => {
                through(p)
                    || match rv {
                        IrRvalue::RawPtr(..) => true,
                        IrRvalue::Binary(spec::MirBinOp::Offset, ..) => true,
                        IrRvalue::Ref(_, q) | IrRvalue::CopyForDeref(q) => through(q),
                        IrRvalue::Use(o) | IrRvalue::Unary(_, o) | IrRvalue::Cast(_, o, _) => {
                            operand(o)
                        }
                        IrRvalue::Binary(_, a, c) => operand(a) || operand(c),
                        IrRvalue::Aggregate(_, ops) => ops.iter().any(|(_, o)| operand(o)),
                        _ => false,
                    }
            }
            _ => false,
        }) || match &b.terminator {
            IrTerminator::Call {
                callee,
                args,
                destination,
                ..
            } => {
                let path = erase(callee);
                let last = split_path(&path).last().copied().unwrap_or_default();
                MEMORY_INTRINSICS.contains(&last)
                    || args.iter().any(operand)
                    || through(destination)
            }
            IrTerminator::Switch(o, ..) => operand(o),
            _ => false,
        }
    })
}

/// A way into memory: the pointer to a value's first cell, its cells' type
/// and size, and their shape.
#[derive(Clone, Debug)]
struct View {
    pointer: String,
    elem: Ty,
    stride: u64,
    shape: Shape,
}

/// What a place reached through a [`View`] is.
enum Access {
    /// A cell's value.
    Cell(String),
    /// A `MaybeUninit`'s cell, read whole.
    Maybe(String),
    /// A `MaybeUninit`'s `uninit` field, the unit.
    Unit(String),
}

/// The item type of an array or slice type as printed: `T` of `[T]` or
/// `[T; N]`.
fn item_of(text: &str) -> Option<String> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let parts = split_top(inner, ';');
    Some(parts.first()?.trim().to_string())
}

/// Whether a value of `t` is its cell's number as it is: an integer or a
/// float's bits.
fn plain_cell(t: &Ty) -> bool {
    matches!(t, Ty::Nat(_) | Ty::Signed(_) | Ty::Float(_))
}

/// How a value of `t` is held in one cell, where it is: as itself (see
/// [`plain_cell`]), a `bool` as `0` or `1` (its bytes, the Reference's
/// "bool"), and an `Option` of a plain value as `0` for `None` and the
/// value plus one for `Some` (a code of the model's own: Rust specifies no
/// layout for it, so no program observes the difference).
fn encoding(t: &Ty) -> Option<()> {
    match t {
        _ if plain_cell(t) => Some(()),
        Ty::Bool => Some(()),
        Ty::Option(inner) if matches!(**inner, Ty::Nat(_)) => Some(()),
        _ => None,
    }
}

/// The cell holding value `v` of type `t` (see [`encoding`]).
fn encode(t: &Ty, v: &str) -> String {
    match t {
        Ty::Bool => format!("(if {v} then 1 else 0)"),
        Ty::Option(_) => format!("(mirEncodeOption {v})"),
        _ => v.to_string(),
    }
}

/// The value of type `t` cell `c` holds (see [`encoding`]).
fn decode(t: &Ty, c: &str) -> String {
    match t {
        Ty::Bool => format!("({c} =? 1)"),
        Ty::Option(_) => format!("(mirDecodeOption {c})"),
        _ => c.to_string(),
    }
}

/// Whether no value of `t` holds a pointer: a value loaded from memory
/// names the pointer it was loaded through, but is not one.
fn holds_no_pointer(t: &Ty) -> bool {
    match t {
        Ty::Unit
        | Ty::Bool
        | Ty::Nat(_)
        | Ty::Int(_)
        | Ty::Signed(_)
        | Ty::Char
        | Ty::Float(_)
        | Ty::Str => true,
        Ty::Option(e) | Ty::List(e) => holds_no_pointer(e),
        Ty::Tuple(ts) | Ty::Pair(ts) => ts.iter().all(holds_no_pointer),
        Ty::Outcome(a, b) => holds_no_pointer(a) && holds_no_pointer(b),
        _ => false,
    }
}

/// The type's name and its one argument, where `text` is an instance of a
/// generic struct of one parameter: `MaybeUninit<T>`.
fn wrapper(text: &str) -> Option<(&str, &str)> {
    let t = text.trim();
    let last = split_path(t).last().copied()?;
    let at = last.find('<')?;
    let name = last[..at].trim_end_matches("::");
    let arg = last[at + 1..].strip_suffix('>')?;
    (split_top(arg, ',').len() == 1).then_some((name, arg))
}

impl Fun<'_, '_> {
    /// The memory where control is.
    fn memory_at(&self, st: &State) -> Lift<String> {
        st.memory.clone().ok_or_else(needs_memory)
    }

    /// The cells of a value of the type printed `text`: their type, size
    /// and alignment, and their shape.
    fn cells_of(&mut self, text: &str) -> Lift<(Ty, u64, u64, Shape)> {
        let t = text.trim();
        let (elem_text, shape) =
            if let Some(inner) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                (split_top(inner, ';')[0], Shape::Items)
            } else {
                match wrapper(t) {
                    Some(("MaybeUninit", arg)) => (arg, Shape::Maybe),
                    Some(("ManuallyDrop", arg)) => (arg, Shape::Scalar),
                    _ => (t, Shape::Scalar),
                }
            };
        let elem = ty(elem_text, self.cx.types)?;
        let encoded = encoding(&elem).is_some();
        if !encoded || (shape != Shape::Scalar && !plain_cell(&elem)) {
            return Err(kind(
                MEMORY_KIND,
                &format!(
                    "a value of {}, which the memory holds no cells of",
                    elem.render()
                ),
            ));
        }
        let l = match &elem {
            // A layout rustc chooses (an `Option`'s): Rust specifies none,
            // so the model's own is as good as any. It is one cell of
            // twice the item's size, holding the value's code.
            Ty::Option(inner) => {
                let i = super::layout::agreed(inner, self.cx.layouts)
                    .map_err(|r| kind(MEMORY_KIND, &super::layout::reason(&r)))?;
                super::layout::Layout {
                    size: 2 * i.size,
                    align: i.align,
                }
            }
            _ => super::layout::agreed(&elem, self.cx.layouts)
                .map_err(|r| kind(MEMORY_KIND, &super::layout::reason(&r)))?,
        };
        if l.size == 0 {
            return Err(kind(MEMORY_KIND, "a zero-sized value in memory"));
        }
        self.strides.insert(l.size);
        self.mention(&super::memory::memory_ty());
        self.mention(&Ty::Named(POINTER.to_string()));
        Ok((elem, l.size, l.align, shape))
    }

    /// The view through a raw pointer `term` to a value of the type printed
    /// `text`.
    fn pointer_view(&mut self, term: &str, text: &str) -> Lift<View> {
        let (elem, stride, _, shape) = self.cells_of(text)?;
        Ok(View {
            pointer: term.to_string(),
            elem,
            stride,
            shape,
        })
    }

    /// The view a place is reached through, where it is in memory: a local
    /// whose address was taken, or a dereference of a raw pointer. The
    /// steps left are the place's projection past it.
    fn view_of(&mut self, p: &IrPlace, st: &State) -> Lift<Option<(View, Vec<IrProjection>)>> {
        if let Some(placed) = st.placed.get(&p.local) {
            let through = placed.mirror && p.projection.first() == Some(&IrProjection::Deref);
            let steps = p.projection[usize::from(through)..].to_vec();
            return Ok(Some((
                View {
                    pointer: placed.pointer.clone(),
                    elem: placed.elem.clone(),
                    stride: placed.stride,
                    shape: placed.shape.clone(),
                },
                steps,
            )));
        }
        let text = self.body.locals.get(p.local).cloned().unwrap_or_default();
        if let (Some((pointee_text, _)), Some((IrProjection::Deref, rest))) =
            (pointee(&text), p.projection.split_first())
        {
            let Some(Val::V(term, _)) = st.env.get(&p.local) else {
                return Err(kind(MEMORY_KIND, "a pointer read before it is written"));
            };
            let term = term.clone();
            let view = self.pointer_view(&term, pointee_text)?;
            return Ok(Some((view, rest.to_vec())));
        }
        // A raw pointer held in a field or behind a reference, dereferenced:
        // the pointer read, then the memory it points into (the last such
        // step, so that a pointer read through another is loaded first).
        let mut at = Some(text.clone());
        let mut held = None;
        for (j, step) in p.projection.iter().enumerate() {
            at = match (step, at.as_deref()) {
                (IrProjection::Deref, Some(x)) => match pointee(x) {
                    Some((pt, _)) => {
                        held = Some((j, pt.to_string()));
                        Some(pt.to_string())
                    }
                    None => referent(x),
                },
                (IrProjection::Field(_, fty), _) => Some(fty.clone()),
                (IrProjection::Index(_) | IrProjection::ConstantIndex(..), Some(x)) => item_of(x),
                _ => None,
            };
        }
        if let Some((j, pointee_text)) = held.filter(|(j, _)| *j > 0) {
            let prefix = IrPlace {
                local: p.local,
                projection: p.projection[..j].to_vec(),
            };
            let (term, t) = self.read(&prefix, st)?;
            if !is_pointer(&t) {
                return Ok(None);
            }
            let view = self.pointer_view(&term, &pointee_text)?;
            return Ok(Some((view, p.projection[j + 1..].to_vec())));
        }
        Ok(None)
    }

    /// The pointer to the part of a view `steps` reach, and what it is.
    fn access(
        &mut self,
        view: &View,
        steps: &[IrProjection],
        st: &State,
    ) -> Lift<(String, Access)> {
        let mut pointer = view.pointer.clone();
        let mut rest = steps;
        if view.shape == Shape::Items {
            let at = match rest.first() {
                Some(IrProjection::Index(l)) => match st.env.get(l) {
                    Some(Val::V(i, Ty::Nat(_))) => i.clone(),
                    _ => return Err(kind(MEMORY_KIND, "an index that is not a natural")),
                },
                Some(IrProjection::ConstantIndex(n, false)) => n.to_string(),
                _ => return Err(kind(MEMORY_KIND, "an array or slice in memory, read whole")),
            };
            pointer = format!(
                "(mirAdvance {pointer} (prim multiply {at} {} : Nat))",
                view.stride
            );
            rest = &rest[1..];
        }
        // A wrapper's field is the wrapped value: `MaybeUninit`'s `value`
        // (its `ManuallyDrop`), and `ManuallyDrop`'s own.
        let mut maybe = view.shape == Shape::Maybe;
        for step in rest {
            match step {
                IrProjection::Field(1, fty)
                    if maybe && wrapper(fty).is_some_and(|(n, _)| n == "ManuallyDrop") =>
                {
                    maybe = false;
                }
                IrProjection::Field(0, fty) if maybe && fty.trim() == "()" => {
                    return Ok((pointer.clone(), Access::Unit(pointer)));
                }
                IrProjection::Field(0, fty)
                    if !maybe && ty(fty, self.cx.types).ok().as_ref() == Some(&view.elem) => {}
                _ => {
                    return Err(kind(
                        MEMORY_KIND,
                        &format!("a part of a value in memory: {step:?}"),
                    ))
                }
            }
        }
        Ok(if maybe {
            (pointer.clone(), Access::Maybe(pointer))
        } else {
            (pointer.clone(), Access::Cell(pointer))
        })
    }

    /// A read of the part of a view `steps` reach.
    fn load(&mut self, view: &View, steps: &[IrProjection], st: &State) -> Lift<(String, Ty)> {
        let m = self.memory_at(st)?;
        let s = view.stride;
        // An array or slice read whole. Every cell of a placement is
        // defined when it is made, and a store keeps it so; through any
        // other pointer, each item must be readable.
        if steps.is_empty() && view.shape == Shape::Items && matches!(view.elem, Ty::Nat(_)) {
            let p = &view.pointer;
            let count = format!("(project {p} metadata)");
            if !st.placed.values().any(|q| q.pointer == view.pointer) {
                self.obligation(
                    st,
                    &format!("((mirReadableItems {m} {p} {s} {count}) = true)"),
                    "a slice read whole through a pointer",
                );
            }
            let out = format!("(mirLoadItems {m} {p} {s} {count})");
            self.grow(&out)?;
            return Ok((out, Ty::List(Box::new(view.elem.clone()))));
        }
        let (_, access) = self.access(view, steps, st)?;
        let out = match access {
            Access::Cell(p) => {
                self.obligation(
                    st,
                    &format!("((mirReadable {m} {p} {s}) = true)"),
                    "a read through a pointer",
                );
                (
                    decode(&view.elem, &format!("(mirLoad {m} {p} {s})")),
                    view.elem.clone(),
                )
            }
            Access::Maybe(p) => (
                format!("(mirLoadMaybe {m} {p} {s})"),
                Ty::Option(Box::new(view.elem.clone())),
            ),
            Access::Unit(_) => ("()".to_string(), Ty::Unit),
        };
        self.grow(&out.0)?;
        Ok(out)
    }

    /// A write of `v` into the part of a view `steps` reach.
    fn store(&mut self, view: &View, steps: &[IrProjection], v: Val, st: &mut State) -> Lift<()> {
        let m = self.memory_at(st)?;
        let s = view.stride;
        let (_, access) = self.access(view, steps, st)?;
        let Val::V(value, _) = v else {
            return Err(kind(MEMORY_KIND, "a borrow or a pair written into memory"));
        };
        let new = match access {
            Access::Cell(p) => {
                self.obligation(
                    st,
                    &format!("((mirWritable {m} {p} {s}) = true)"),
                    "a write through a pointer",
                );
                format!("(mirStore {m} {p} {s} {})", encode(&view.elem, &value))
            }
            Access::Maybe(p) => {
                self.obligation(
                    st,
                    &format!("((mirWritable {m} {p} {s}) = true)"),
                    "a write through a pointer",
                );
                format!("(mirStoreMaybe {m} {p} {s} {value})")
            }
            Access::Unit(p) => {
                self.obligation(
                    st,
                    &format!("((mirWritable {m} {p} {s}) = true)"),
                    "a write through a pointer",
                );
                format!("(mirForget {m} {p} {s})")
            }
        };
        self.grow(&new)?;
        st.memory = Some(new);
        Ok(())
    }

    /// `&raw const p` or `&raw mut p`: the place's address. A place not yet
    /// in memory is moved into an allocation of its own.
    fn address_of(&mut self, p: &IrPlace, dest: &IrPlace, st: &mut State) -> Lift<Val> {
        let pt = Ty::Named(POINTER.to_string());
        // Through a mutable borrow: the place it borrows.
        if let (Some(Val::Mut(q)), Some((IrProjection::Deref, rest))) =
            (st.env.get(&p.local).cloned(), p.projection.split_first())
        {
            let mut through = q.clone();
            through.projection.extend(rest.iter().cloned());
            return self.address_of(&through, dest, st);
        }
        // A pointer to a slice carries its length.
        let wide = dest.projection.is_empty()
            && self
                .body
                .locals
                .get(dest.local)
                .and_then(|t| pointee(t))
                .is_some_and(|(_, w)| w);
        let pointer = self.address_at(p, wide, st)?;
        self.mention(&pt);
        Ok(Val::V(pointer, pt))
    }

    /// The address of place `p`, placed in memory where it is not yet;
    /// `wide`, a slice's address with its length.
    fn address_at(&mut self, p: &IrPlace, wide: bool, st: &mut State) -> Lift<String> {
        if let (Some(Val::Mut(q)), Some((IrProjection::Deref, rest))) =
            (st.env.get(&p.local).cloned(), p.projection.split_first())
        {
            let mut through = q.clone();
            through.projection.extend(rest.iter().cloned());
            return self.address_at(&through, wide, st);
        }
        if self.view_of(p, st)?.is_none() {
            self.place_root(p, st)?;
        }
        let (view, steps) = self
            .view_of(p, st)?
            .ok_or_else(|| kind(MEMORY_KIND, "a place not in memory"))?;
        // The address of a whole array or slice is its first item's.
        let (pointer, _) = if steps.is_empty() && view.shape == Shape::Items {
            (view.pointer.clone(), Access::Unit(view.pointer.clone()))
        } else {
            self.access(&view, &steps, st)?
        };
        let pointer = if wide && steps.is_empty() && view.shape == Shape::Items {
            match st.placed.get(&p.local) {
                Some(placed) => format!(
                    "(record {POINTER} {{allocation := (project {pointer} allocation), address := (project {pointer} address), metadata := (project {} metadata)}})",
                    placed.pointer
                ),
                None => pointer,
            }
        } else {
            pointer
        };
        self.mention(&Ty::Named(POINTER.to_string()));
        Ok(pointer)
    }

    /// A fresh allocation of one cell holding the value at place `p`, of
    /// the type printed `text`, for a callee that takes its address: its
    /// pointer.
    fn lent_cell(&mut self, p: &IrPlace, text: &str, st: &mut State) -> Lift<String> {
        let (elem, stride, align, shape) = self.cells_of(text)?;
        if shape != Shape::Scalar {
            return Err(kind(MEMORY_KIND, "a lent cell of more than one value"));
        }
        let (value, _) = self.read(p, st)?;
        let m = self.memory_at(st)?;
        let pointer = format!("(mirAllocated {m} {align} 0)");
        let memory = format!(
            "(mirAllocate {m} {stride} {align} 1 (mirCellsOf {}) (mirDefinedAs 1))",
            encode(&elem, &value)
        );
        self.grow(&memory)?;
        st.memory = Some(memory);
        self.mention(&Ty::Named(POINTER.to_string()));
        Ok(pointer)
    }

    /// Move the local a place is rooted at into an allocation of its own:
    /// the local itself, or the place a borrow it holds lends.
    fn place_root(&mut self, p: &IrPlace, st: &mut State) -> Lift<()> {
        let l = p.local;
        let text = self
            .body
            .locals
            .get(l)
            .cloned()
            .ok_or_else(|| kind(MEMORY_KIND, "a local without a declaration"))?;
        // A `&mut` parameter stands for the place it borrows, also whole.
        let through = p.projection.first() == Some(&IrProjection::Deref)
            || (p.projection.is_empty() && self.mut_params.contains(&l));
        let (value_text, mirror, shared) = if through {
            let Some(r) = referent(&text) else {
                return Err(kind(
                    MEMORY_KIND,
                    &format!("the address of a place behind {text}"),
                ));
            };
            let mutable = mut_target(&text).is_some();
            (r.to_string(), true, !mutable)
        } else {
            (text.clone(), false, false)
        };
        let (elem, stride, align, shape) = self.cells_of(&value_text)?;
        let m = self.memory_at(st)?;
        let value = match st.env.get(&l) {
            Some(Val::V(v, _)) => Some(v.clone()),
            None if shape == Shape::Maybe => None,
            _ => {
                return Err(kind(
                    MEMORY_KIND,
                    "the address of a local that holds no value",
                ))
            }
        };
        let (length, cells, defined, metadata) = match (&shape, value) {
            (Shape::Scalar, Some(v)) => (
                "1".to_string(),
                format!("(mirCellsOf {})", encode(&elem, &v)),
                "(mirDefinedAs 1)".to_string(),
                "0".to_string(),
            ),
            (Shape::Maybe, Some(v)) => (
                "1".to_string(),
                format!("(mirCellsOfMaybe {v})"),
                format!("(mirDefinedOfMaybe {v})"),
                "0".to_string(),
            ),
            (Shape::Maybe, None) => (
                "1".to_string(),
                "(mirCellsOf 0)".to_string(),
                "(mirDefinedAs 0)".to_string(),
                "0".to_string(),
            ),
            (Shape::Items, Some(v)) => {
                let len = format!("(prim length {v} : Nat)");
                (
                    len.clone(),
                    format!("(mirCellsOfList {v} {stride})"),
                    "(mirDefinedAs 1)".to_string(),
                    len,
                )
            }
            _ => {
                return Err(kind(
                    MEMORY_KIND,
                    "the address of a local that holds no value",
                ))
            }
        };
        let pointer = format!("(mirAllocated {m} {align} {metadata})");
        let memory = format!("(mirAllocate {m} {stride} {align} {length} {cells} {defined})");
        self.grow(&memory)?;
        if shared {
            self.mirrored_shared = true;
        }
        st.memory = Some(memory);
        st.placed.insert(
            l,
            Placed {
                pointer,
                elem,
                stride,
                shape,
                mirror,
                lent: false,
            },
        );
        Ok(())
    }

    /// The memory a body gives back: every allocation it made freed, its
    /// locals' and its copies of the places its callers lent, which no
    /// pointer it returns may reach.
    fn returned_memory(&mut self, returned: &str, st: &State) -> Lift<String> {
        let mut m = self.memory_at(st)?;
        let plain = holds_no_pointer(&self.ret);
        for placed in st.placed.values().filter(|p| !p.lent) {
            if placed.mirror && !plain && returned.contains(&placed.pointer) {
                return Err(kind(
                    MEMORY_KIND,
                    "a pointer to a place the caller lent, returned: the value model gives that place no address",
                ));
            }
            m = format!("(mirFree {m} (project {} allocation))", placed.pointer);
        }
        self.grow(&m)?;
        Ok(m)
    }

    /// `p.offset(n)` and `p.add(n)`: `n` items further on, which must stay
    /// in its allocation or one past its end (`checked`), as `offset`
    /// requires; `arith_offset`'s wrapping pointer is not checked.
    fn offset_pointer(
        &mut self,
        p: &str,
        pointee_text: &str,
        n: &str,
        nt: &Ty,
        checked: bool,
        st: &State,
    ) -> Lift<String> {
        if !matches!(nt, Ty::Nat(_)) {
            return Err(kind(MEMORY_KIND, &format!("an offset by {}", nt.render())));
        }
        let (_, stride, _, _) = self.cells_of(pointee_text)?;
        let moved = format!("(mirAdvance {p} (prim multiply {n} {stride} : Nat))");
        if checked {
            let m = self.memory_at(st)?;
            self.obligation(
                st,
                &format!("((mirWithin {m} {moved}) = true)"),
                "an offset within its allocation",
            );
        }
        self.grow(&moved)?;
        Ok(moved)
    }

    /// A call of an intrinsic that reaches memory, by its name and the
    /// types it binds: its model.
    fn memory_intrinsic(
        &mut self,
        name: &str,
        generics: &[String],
        args: &[(String, Ty)],
        st: &mut State,
    ) -> Lift<Option<String>> {
        let one = |i: usize| generics.get(i).cloned().unwrap_or_default();
        match (name, args) {
            // A volatile access is an ordinary one in the model: the model
            // has no other observer of memory.
            ("read_via_copy" | "volatile_load", [(p, _)]) => {
                let view = self.pointer_view(p, &one(0))?;
                Ok(Some(self.load(&view, &[], st)?.0))
            }
            ("write_via_move" | "volatile_store", [(p, _), (v, t)]) => {
                let view = self.pointer_view(p, &one(0))?;
                self.store(&view, &[], Val::V(v.clone(), t.clone()), st)?;
                Ok(Some("()".into()))
            }
            ("typed_swap_nonoverlapping", [(x, _), (y, _)]) => {
                let vx = self.pointer_view(x, &one(0))?;
                let vy = self.pointer_view(y, &one(0))?;
                let (a, at) = self.load(&vx, &[], st)?;
                let (b, bt) = self.load(&vy, &[], st)?;
                self.obligation(
                    st,
                    &format!("((((project {x} allocation) =? (project {y} allocation)) && ((project {x} address) =? (project {y} address))) = false)"),
                    "pointers that do not overlap",
                );
                self.store(&vx, &[], Val::V(b, bt), st)?;
                self.store(&vy, &[], Val::V(a, at), st)?;
                Ok(Some("()".into()))
            }
            ("copy_nonoverlapping" | "copy", [(src, _), (dst, _), (count, _)]) if count == "1" => {
                let vs = self.pointer_view(src, &one(0))?;
                let vd = self.pointer_view(dst, &one(0))?;
                let (v, t) = self.load(&vs, &[], st)?;
                self.store(&vd, &[], Val::V(v, t), st)?;
                Ok(Some("()".into()))
            }
            // `count` items: both ranges live and in bounds, which their
            // first and last items being so gives (an allocation is one
            // range), and apart for `copy_nonoverlapping`; the target's
            // cells then hold the source's, read before any is written.
            ("copy_nonoverlapping" | "copy", [(src, _), (dst, _), (count, ct)]) => {
                if !matches!(ct, Ty::Nat(_)) {
                    return Err(kind(
                        MEMORY_KIND,
                        &format!("a copy of {} items", ct.render()),
                    ));
                }
                let vs = self.pointer_view(src, &one(0))?;
                let vd = self.pointer_view(dst, &one(0))?;
                if vs.shape == Shape::Items || vd.shape == Shape::Items || vs.stride != vd.stride {
                    return Err(kind(MEMORY_KIND, "a copy between cells of other sizes"));
                }
                let m = self.memory_at(st)?;
                let s = vs.stride;
                // A copy of bytes (`copy::<u8>`, the bytes of a value of
                // any type): whole cells of two allocations of one cell
                // size, which their cells' copy is.
                if s == 1 {
                    self.obligation(
                        st,
                        &format!(
                            "(((mirWholeCells {m} {src} {count}) && ((mirWholeCells {m} {dst} {count}) && ((apply (project {m} stride) (project {src} allocation)) =? (apply (project {m} stride) (project {dst} allocation))))) = true)"
                        ),
                        "a copy of bytes, as whole cells of one size",
                    );
                    if name == "copy_nonoverlapping" {
                        self.obligation(
                            st,
                            &format!(
                                "(((not ((project {src} allocation) =? (project {dst} allocation))) || ((((project {src} address) + {count}) <=? (project {dst} address)) || (((project {dst} address) + {count}) <=? (project {src} address)))) = true)"
                            ),
                            "a copy's ranges that do not overlap",
                        );
                    }
                    let new = format!("(mirCopy {m} {src} {dst} 1 {count})");
                    self.grow(&new)?;
                    st.memory = Some(new);
                    return Ok(Some("()".into()));
                }
                let last = |p: &str| {
                    format!(
                        "(mirAdvance {p} (prim multiply (prim subtract {count} 1 : Nat) {s} : Nat))"
                    )
                };
                for (p, what) in [(src, "a copy's source"), (dst, "a copy's target")] {
                    self.obligation(
                        st,
                        &format!(
                            "((({count} =? 0) || ((mirWritable {m} {p} {s}) && (mirWritable {m} {} {s}))) = true)",
                            last(p)
                        ),
                        &format!("{what}, valid for its items"),
                    );
                }
                if name == "copy_nonoverlapping" {
                    let span = format!("(prim multiply {count} {s} : Nat)");
                    self.obligation(
                        st,
                        &format!(
                            "(((not ((project {src} allocation) =? (project {dst} allocation))) || ((((project {src} address) + {span}) <=? (project {dst} address)) || (((project {dst} address) + {span}) <=? (project {src} address)))) = true)"
                        ),
                        "a copy's ranges that do not overlap",
                    );
                }
                let new = format!("(mirCopy {m} {src} {dst} {s} {count})");
                self.grow(&new)?;
                st.memory = Some(new);
                Ok(Some("()".into()))
            }
            ("offset" | "arith_offset", [(p, _), (n, nt)]) => {
                // `offset::<*const T, usize>`: `T` is the pointer's pointee.
                let pointer_text = one(0);
                let Some((pointee_text, _)) = pointee(&pointer_text) else {
                    return Err(kind(MEMORY_KIND, &format!("an offset of {pointer_text}")));
                };
                let pointee_text = pointee_text.to_string();
                let checked = name == "offset";
                Ok(Some(self.offset_pointer(
                    p,
                    &pointee_text,
                    n,
                    nt,
                    checked,
                    st,
                )?))
            }
            ("aggregate_raw_ptr", [(data, _), (meta, mt)]) => {
                let meta = match mt {
                    Ty::Nat(_) => meta.clone(),
                    Ty::Unit => "0".to_string(),
                    other => {
                        return Err(kind(
                            MEMORY_KIND,
                            &format!("a pointer's metadata of {}", other.render()),
                        ))
                    }
                };
                Ok(Some(format!(
                    "(record {POINTER} {{allocation := (project {data} allocation), address := (project {data} address), metadata := {meta}}})"
                )))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tuple_of_one_item_is_told_from_a_wider_one() {
        assert!(one_tuple("(&&std::string::String,)"));
        assert!(one_tuple("(std::collections::BTreeMap<u8, u8>,)"));
        assert!(one_tuple("(fn(u8, u8) -> u8,)"));
        assert!(!one_tuple("(u8, u8)"));
        assert!(!one_tuple("(u8, u8,)"));
        assert!(!one_tuple("std::string::String"));
        assert_eq!(
            referent("&&std::string::String").as_deref(),
            Some("&std::string::String")
        );
        assert_eq!(referent("&'a mut (u8,)").as_deref(), Some("(u8,)"));
        assert_eq!(referent("u8"), None);
    }

    #[test]
    fn patterns_give_rustcs_valid_ranges_and_niches() {
        let max32 = u128::from(u32::MAX);
        // `NonZero`: every pattern but zero, the niche zero, signed too.
        assert_eq!(valid_range("1..", false, 32), Some((1, max32)));
        assert_eq!(valid_range("..0|1..", true, 32), Some((1, max32)));
        assert_eq!(
            valid_range("(i32::MIN..=-1 | 1..)", true, 32),
            Some((1, max32))
        );
        assert_eq!(niche(1, max32, max32), Some(0));
        // `Nanoseconds`: its niche the first value past its range.
        assert_eq!(
            valid_range("0..=999_999_999", false, 32),
            Some((0, 999_999_999))
        );
        assert_eq!(niche(0, 999_999_999, max32), Some(1_000_000_000));
        // All ones left out: the niche is all ones.
        assert_eq!(valid_range("0..u32::MAX", false, 32), Some((0, max32 - 1)));
        assert_eq!(valid_range("..-1|0..", true, 32), Some((0, max32 - 1)));
        assert_eq!(niche(0, max32 - 1, max32), Some(max32));
        // A `char` from one: the niche is zero, below its start.
        assert_eq!(
            valid_range("'\\u{1}'..='\\u{10ffff}'", false, 32),
            Some((1, 0x10ffff))
        );
        assert_eq!(niche(1, 0x10ffff, max32), Some(0));
        // Every pattern: no niche.
        assert_eq!(niche(0, max32, max32), None);
        assert_eq!(valid_range("1..", false, 128), Some((1, u128::MAX)));
        // At 128 bits signed, the bounds are `i128`'s own.
        assert_eq!(
            valid_range("(i128::MIN..=-1 | 1..)", true, 128),
            Some((1, u128::MAX))
        );
        assert_eq!(
            valid_range("-170141183460469231731687303715884105728..=-1", true, 128),
            Some((1 << 127, u128::MAX))
        );
        assert_eq!(niche(1, u128::MAX, u128::MAX), Some(0));
    }

    #[test]
    fn generic_arguments_are_erased_from_callees() {
        assert_eq!(erase("std::vec::Vec::<u8>::len"), "std::vec::Vec::len");
        assert_eq!(
            erase("<std::vec::Vec<u8> as std::clone::Clone>::clone"),
            "<std::vec::Vec as std::clone::Clone>::clone"
        );
        assert_eq!(
            erase("core::num::<impl u64>::wrapping_add"),
            "core::num::<impl u64>::wrapping_add"
        );
    }

    #[test]
    fn comparisons_read_back() {
        assert_eq!(
            comparison("(i <? (prim length xs : Nat))"),
            Some(("<?", "i".to_string(), "(prim length xs : Nat)".to_string()))
        );
        assert_eq!(comparison("(not x)"), None);
    }

    #[test]
    fn a_pointer_value_is_the_pointer_struct() {
        let v = pointer_value("1", "2", "3");
        assert!(v.starts_with(&format!("(record {POINTER} {{")), "{v}");
        for (f, _) in pointer_fields() {
            assert!(v.contains(&format!("{f} := ")), "{v} lacks {f}");
        }
    }

    #[test]
    fn a_held_borrow_and_its_place_share_a_match_key() {
        let deref = IrPlace {
            local: 1,
            projection: vec![IrProjection::Deref],
        };
        let whole = IrPlace {
            local: 1,
            projection: Vec::new(),
        };
        assert_eq!(place_key(&deref, true), place_key(&whole, true));
        assert_ne!(place_key(&deref, false), place_key(&whole, false));
    }

    #[test]
    fn lifetime_arguments_are_left_out() {
        assert_eq!(
            without_lifetimes("str::pattern::CharSearcher<'_>"),
            "str::pattern::CharSearcher"
        );
        assert_eq!(without_lifetimes("DebugStruct<'_, '_>"), "DebugStruct");
        assert_eq!(without_lifetimes("Foo<'a, u8>"), "Foo<u8>");
        assert_eq!(without_lifetimes("Foo<u8, 'a>"), "Foo<u8>");
        assert_eq!(without_lifetimes("Option<&'a str>"), "Option<&'a str>");
    }

    #[test]
    fn mut_parameters_are_threaded() {
        let locals: Vec<String> = ["()", "&mut u32", "u8", "&'a mut [u8]"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let types = Types::default();
        assert_eq!(threaded(&locals, 3, false, &types), vec![1, 3]);
        assert_eq!(threaded(&locals, 3, true, &types), vec![3]);
    }

    #[test]
    fn mir_types_name_the_lifts_types() {
        let types = Types::default();
        assert_eq!(
            ty("&[u64]", &types),
            Ok(Ty::List(Box::new(Ty::Nat(Some(64)))))
        );
        assert_eq!(ty("&'a str", &types), Ok(Ty::Str));
        assert_eq!(
            ty("std::option::Option<std::string::String>", &types),
            Ok(Ty::Option(Box::new(Ty::Str)))
        );
        // A mutable borrow holds the value it borrows; `!` stands as the unit.
        assert_eq!(ty("&mut u8", &types), Ok(Ty::Nat(Some(8))));
        assert_eq!(
            ty("&'a mut [u8]", &types),
            Ok(Ty::List(Box::new(Ty::Nat(Some(8)))))
        );
        assert_eq!(ty("!", &types), Ok(Ty::Unit));
        assert!(ty("*const u8", &types).is_err());
        assert_eq!(ty("std::boxed::Box<bool>", &types), Ok(Ty::Bool));
        assert_eq!(
            ty("std::result::Result<std::convert::Infallible, u8>", &types),
            Ok(Ty::Nat(Some(8)))
        );
        assert_eq!(
            ty(
                "std::ops::ControlFlow<std::result::Result<std::convert::Infallible, u8>, bool>",
                &types
            ),
            Ok(Ty::Outcome(Box::new(Ty::Bool), Box::new(Ty::Nat(Some(8)))))
        );
    }
}
