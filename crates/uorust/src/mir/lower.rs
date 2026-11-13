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
}

impl Called {
    fn value(term: String) -> Self {
        Self {
            term,
            updates: BTreeMap::new(),
            guard: None,
            threads: None,
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
}

/// A loop: its helper's name and parameters (the locals live at its header).
#[derive(Clone, Debug)]
struct Loop {
    name: String,
    live: Vec<(IrLocal, String, Ty)>,
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
    /// The `&mut` parameters: each holds the borrowed value, which writes
    /// through it replace, and the definition returns its last value.
    mut_params: BTreeSet<IrLocal>,
}

/// The parameters that are mutable borrows, in order, of a body with these
/// locals' types and this arity: a `&mut T` parameter is threaded as state,
/// as Aeneas's backward functions are. The definition takes the borrowed
/// `T` and returns, after `_0`'s value, the value each such parameter's
/// place holds when the body returns; a caller writes it back into the
/// place it lent. A closure's environment `_1` is the closure's own state,
/// which its call gives back unchanged.
pub(crate) fn threaded(locals: &[String], arity: usize, closure: bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut l = 1;
    while l <= arity {
        if !(closure && l == 1) {
            if let Some(t) = locals.get(l) {
                if mut_target(t).is_some() {
                    out.push(l);
                }
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
    let rest = t
        .strip_prefix("*const ")
        .or_else(|| t.strip_prefix("*mut "))?
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
        mut_params: BTreeSet::new(),
    };
    f.used.insert(name.to_string());
    let threads = threaded(
        &body.locals,
        body.arity,
        super::closure_type(body).is_some(),
    );
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
    // A `&mut` parameter is read as the value it borrows.
    for l in 1..=body.arity {
        if closure && l == 1 {
            continue;
        }
        let ty = f.local_ty(l, &[])?;
        let p = f.fresh(body.names.get(l).cloned().flatten().as_deref(), l);
        st.env.insert(l, Val::V(p.clone(), ty.clone()));
        f.params.push((p, ty));
    }
    let mut threads_ty = Vec::new();
    for &l in &threads {
        threads_ty.push(f.local_ty(l, &[])?);
    }
    f.result = threaded_result(&f.ret, &threads_ty, cx.types);
    f.find_loops()?;
    f.scope = f.params.clone();
    let value = match f.float_intrinsic_body() {
        Some(v) => v,
        None => f.block(0, st)?,
    };
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
    /// The model's definitions a float term needs, held as auxiliaries;
    /// the term itself.
    fn float_use(&mut self, u: float::FloatUse) -> String {
        for d in u.defs {
            self.calls.insert(d.name.clone());
            self.aux.entry(d.name.clone()).or_insert_with(|| d.decl());
        }
        u.term
    }

    fn float_model(&self, f: float::Format) -> float::Ieee {
        float::Ieee::of(f, &self.cx.types.floats)
    }

    /// A call of one of core's float intrinsics, by the name MIR prints:
    /// the model's operation, where the arguments are the intrinsic's
    /// format.
    fn float_intrinsic(&mut self, callee: &str, args: &[(String, Ty)], dt: &Ty) -> Option<String> {
        let path = erase(callee);
        let segs = split_path(&path);
        // Core's own bodies call an intrinsic they import by its bare name.
        let bare = segs.len() == 1 && self.cx.types.core;
        if !bare
            && !matches!(
                segs.as_slice(),
                ["intrinsics", _] | ["core" | "std", "intrinsics", _]
            )
        {
            return None;
        }
        // `float_to_int_unchecked`: its result where it is defined, a value
        // in the target's range, is the truncation `as` gives.
        if segs.last() == Some(&"float_to_int_unchecked") {
            let ([(v, Ty::Float(f))], Ty::Nat(Some(w))) = (args, dt) else {
                return None;
            };
            let u = self.float_model(*f).to_nat(*w, v);
            return Some(self.float_use(u));
        }
        let (base, f) = float::split_intrinsic(segs.last()?, &self.cx.types.floats)?;
        let (op, arity) = float::intrinsic(base)?;
        let typed = args.len() == arity
            && args.iter().all(|(_, t)| *t == Ty::Float(f))
            && *dt == Ty::Float(f);
        if !typed {
            return None;
        }
        let terms: Vec<&str> = args.iter().map(|(v, _)| v.as_str()).collect();
        let u = self.float_model(f).apply(op, &terms);
        Some(self.float_use(u))
    }

    /// A method of a float type, or one of its operator impls, which core
    /// writes as the operator itself: the model's meaning of it.
    fn float_method(&mut self, callee: &str, args: &[(String, Ty)], dt: &Ty) -> Option<Called> {
        let path = erase(callee);
        // `<f32 as ops::arith::Add>::add`, or `f32::<impl f32>::is_nan`.
        let (on, trait_name, name) = match path.strip_prefix('<') {
            Some(rest) => {
                let (q, m) = rest.rsplit_once(">::")?;
                let (on, tr) = q.split_once(" as ")?;
                (
                    on.to_string(),
                    split_path(tr).last().map(|t| t.to_string()),
                    m.to_string(),
                )
            }
            None => {
                let segs = split_path(&path);
                let (name, head) = segs.split_last()?;
                let on = head.first()?.to_string();
                let inherent =
                    head.len() == 1 || (head.len() == 2 && head[1] == format!("<impl {on}>"));
                if !inherent {
                    return None;
                }
                (on, None, name.to_string())
            }
        };
        let f = *self.cx.types.floats.get(&on)?;
        let (m, arity) = float::method(trait_name.as_deref(), &name)?;
        if args.len() != arity {
            return None;
        }
        let model = self.float_model(f);
        let float = Ty::Float(f);
        let terms: Vec<&str> = args.iter().map(|(v, _)| v.as_str()).collect();
        let floats = |n: usize| args.iter().take(n).all(|(_, t)| *t == float);
        match m {
            float::Method::Op(op) if floats(arity) && *dt == float => {
                let u = model.apply(op, &terms);
                Some(Called::value(self.float_use(u)))
            }
            float::Method::Test(op) if floats(arity) && *dt == Ty::Bool => {
                let u = model.test(op, &terms)?;
                Some(Called::value(self.float_use(u)))
            }
            float::Method::Bits => {
                let bits = Ty::Nat(Some(f.width()));
                let (a, at) = &args[0];
                ((*at == float && *dt == bits) || (*at == bits && *dt == float))
                    .then(|| Called::value(a.clone()))
            }
            float::Method::Assign(op) if floats(arity) && *dt == Ty::Unit => {
                let u = model.apply(op, &terms);
                let new = self.float_use(u);
                Some(Called {
                    term: "()".into(),
                    updates: BTreeMap::from([(0, new)]),
                    guard: None,
                    threads: None,
                })
            }
            _ => None,
        }
    }

    /// The body of a float intrinsic core declares without one (it is
    /// `unreachable`, the compiler supplying it): the model's operation on
    /// its parameters.
    fn float_intrinsic_body(&mut self) -> Option<Expr> {
        let b = self.body;
        let bodiless = b.blocks.len() == 1
            && b.blocks[0].statements.is_empty()
            && b.blocks[0].terminator == IrTerminator::Unreachable;
        if !bodiless {
            return None;
        }
        let args = self.params.clone();
        let dt = self.result.clone();
        let callee = format!("intrinsics::{}", split_path(&b.path).last()?);
        self.float_intrinsic(&callee, &args, &dt).map(Expr::Flat)
    }

    /// A float literal as MIR prints it (`0f32`, `-0f64`, `1.0E+4f16`), or
    /// one of the constants IEEE 754 fixes, by core's name for it
    /// (`f32::<impl f32>::NAN`).
    fn float_constant(&self, text: &str) -> Option<(String, Ty)> {
        let floats = &self.cx.types.floats;
        for (t, f) in floats {
            // `1f32`, and `+Inf_f16`, as rustc prints a value with no
            // decimal form.
            let lit = text.strip_suffix(t.as_str());
            if let Some(lit) = lit.map(|l| l.strip_suffix('_').unwrap_or(l)) {
                if let Some(bits) = float::literal(*f, lit, float::widest(floats)) {
                    return Some((bits.decimal(), Ty::Float(*f)));
                }
            }
            let named = text
                .strip_prefix(&format!("{t}::<impl {t}>::"))
                .or_else(|| text.strip_prefix(&format!("core::{t}::<impl {t}>::")));
            if let Some(bits) = named.and_then(|n| self.float_model(*f).named(n)) {
                return Some((bits.decimal(), Ty::Float(*f)));
            }
        }
        None
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
        for step in steps {
            t = match step {
                IrProjection::Deref if is_pointer(&t) => return Err(through_pointer("write")),
                IrProjection::Deref | IrProjection::Downcast(_) => t,
                IrProjection::Field(_, fty) => ty(fty, self.cx.types)?,
                IrProjection::Index(_) | IrProjection::ConstantIndex(..) => match t {
                    Ty::List(elem) => *elem,
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
        let threads = threaded(
            &self.body.locals,
            self.body.arity,
            super::closure_type(self.body).is_some(),
        );
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
        let lp = self.helper(b)?;
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
    fn helper(&mut self, h: IrBlockId) -> Lift<Loop> {
        if let Some(Some(lp)) = self.loops.get(&h) {
            return Ok(lp.clone());
        }
        let live = self.live_at(h);
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
        let lp = Loop {
            name: name.clone(),
            live: params.clone(),
        };
        self.loops.insert(h, Some(lp.clone()));
        let saved = (
            std::mem::take(&mut self.scope),
            std::mem::take(&mut self.evidence),
            self.measure.take(),
        );
        self.scope = params
            .iter()
            .map(|(_, p, t)| (p.clone(), t.clone()))
            .collect();
        let measure = self.derive_measure(h, &st)?;
        self.measure = Some(measure.clone());
        let body = self.block(h, st)?;
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
        let mut next = State {
            inside: Some(h),
            ..State::default()
        };
        for (l, _, _) in &lp.live {
            if let Some(v) = st.env.get(l) {
                next.env.insert(*l, v.clone());
            }
        }
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
        self.decls.push(Decl::Theorem {
            name,
            parameters,
            statement: implication(&hyps, goal),
            proof: "linear_arithmetic{}".into(),
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
                let threads = threaded(
                    &self.body.locals,
                    self.body.arity,
                    super::closure_type(self.body).is_some(),
                );
                let v = if threads.is_empty() {
                    v
                } else {
                    // `_0`, then the last value of each `&mut` parameter.
                    let mut items = vec![v];
                    for l in threads {
                        let place = IrPlace {
                            local: l,
                            projection: Vec::new(),
                        };
                        items.push(self.read(&place, &st)?.0);
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
                let Some(target) = target else {
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
        let k = spec::MirStatementKind::Assign.path();
        if let (Some(Val::Mut(q)), Some((IrProjection::Deref, rest))) =
            (st.env.get(&p.local), p.projection.split_first())
        {
            let mut q = q.clone();
            q.projection.extend(rest.iter().cloned());
            return self.write(&q, v, st);
        }
        let steps: Vec<IrProjection> = p
            .projection
            .iter()
            .filter(|s| !(matches!(s, IrProjection::Deref) && self.mut_params.contains(&p.local)))
            .cloned()
            .collect();
        if steps.is_empty() {
            st.env.insert(p.local, v);
            return Ok(());
        }
        let Val::V(new, _) = v else {
            return Err(kind(k, "a pair or a borrow written into a part of a value"));
        };
        let Some(Val::V(whole, wt)) = st.env.get(&p.local).cloned() else {
            return Err(kind(
                k,
                "a write into a part of a local that holds no value",
            ));
        };
        let updated = self.replace(&whole, &wt, &steps, &new, st)?;
        self.grow(&updated)?;
        st.env.insert(p.local, Val::V(updated, wt));
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
        // The one variant a place is matched as: its binder replaced.
        if let IrProjection::Downcast(_) = step {
            return Err(kind(k, "a write into a field of an enum's variant"));
        }
        if matches!(step, IrProjection::Deref) && is_pointer(t) {
            return Err(through_pointer("write"));
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
        let mut vals = Vec::new();
        let mut texts = Vec::new();
        let mut borrowed: BTreeMap<usize, IrPlace> = BTreeMap::new();
        for (i, a) in args.iter().enumerate() {
            let whole = match a {
                IrOperand::Copy(p) | IrOperand::Move(p) if p.projection.is_empty() => Some(p.local),
                _ => None,
            };
            texts.push(whole.and_then(|l| self.body.locals.get(l).cloned()));
            match whole.and_then(|l| st.env.get(&l)) {
                Some(Val::Mut(place)) => {
                    let place = place.clone();
                    vals.push(self.read(&place, st)?);
                    borrowed.insert(i, place);
                }
                _ => {
                    // A `&mut` parameter passed on lends the place it holds.
                    if let Some(l) = whole.filter(|l| self.mut_params.contains(l)) {
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
        let called = self.call(callee, vals.clone(), &texts, &dest_text, &dt, st)?;
        let Called {
            term,
            mut updates,
            guard,
            threads,
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
        for (i, place) in borrowed {
            let Some(new) = updates.get(&i) else {
                // A borrowed value no one reads again needs no new value.
                if !self.live_at(target).contains(&place.local) {
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
        if destination.projection.is_empty() {
            st.env.insert(destination.local, value);
        } else {
            self.write(destination, value, st)?;
        }
        Ok(binding)
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
        yes.bound
            .insert((place_key(&place), "Some".into()), vec![value.to_string()]);
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
            arm.bound.insert(
                (place_key(place), only.variant.to_string()),
                vec![scrutinee],
            );
            return self.goto(target, arm);
        }
        let ctors = constructors(&t, self.cx.types, self.cx.explicit)?;
        let key = place_key(place);
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
                if p.projection.is_empty() {
                    let v = self.rvalue(rv, p, st)?;
                    st.env.insert(p.local, v);
                    return Ok(());
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
                self.write(p, v, st)
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
            IrConstant::Path(p) | IrConstant::Other(p) if self.float_constant(p).is_some() => {
                self.float_constant(p).ok_or_else(|| kind(k, p))
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
                        Some(Ok(v)) => Ok(v),
                        Some(Err(d)) => Err(kind(k, &format!("{o}::{d}"))),
                        None => Err(kind(k, &format!("the constant item {p}"))),
                    }
                }
                Some(body) => self.const_body(p, body),
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
                    let key = (place_key(&prefix), variant.clone());
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
        while let Some(step) = steps.next() {
            match step {
                // A shared reference or a box is the value it points to.
                IrProjection::Deref if is_pointer(&t) => return Err(through_pointer("read")),
                IrProjection::Deref => {}
                IrProjection::Field(i, fty) => {
                    let ft = ty(fty, self.cx.types)?;
                    v = self.field(&v, &t, *i)?;
                    t = ft;
                }
                IrProjection::Downcast(variant) => {
                    // `ControlFlow`'s variants are `Result`'s.
                    let variant = match variant.as_str() {
                        "Continue" if matches!(t, Ty::Outcome(..)) => "Ok".to_string(),
                        "Break" if matches!(t, Ty::Outcome(..)) => "Err".to_string(),
                        v => v.to_string(),
                    };
                    let key = (place_key(&prefix), variant.clone());
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
                if self.cx.types.boxed(n) {
                    Ok(format!("({} {v})", Types::accessor(n, f)))
                } else {
                    Ok(format!("(project {v} {f})"))
                }
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
            // A mutable borrow moved or copied whole is the same borrow.
            IrRvalue::Use(IrOperand::Copy(q) | IrOperand::Move(q))
                if q.projection.is_empty() && matches!(st.env.get(&q.local), Some(Val::Mut(_))) =>
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
                    (spec::MirUnOp::Not, Ty::Bool) => Ok(Val::V(format!("(not {v})"), Ty::Bool)),
                    (spec::MirUnOp::Neg, Ty::Float(f)) => {
                        let u = self.float_model(*f).apply("Neg", &[&v]);
                        Ok(Val::V(self.float_use(u), t.clone()))
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
                    (spec::MirCastKind::IntToFloat, Ty::Nat(_), Ty::Float(f)) => {
                        let u = self.float_model(*f).of_nat(&v);
                        Ok(Val::V(self.float_use(u), to))
                    }
                    (spec::MirCastKind::FloatToInt, Ty::Float(f), Ty::Nat(Some(w))) => {
                        let u = self.float_model(*f).to_nat(*w, &v);
                        Ok(Val::V(self.float_use(u), to))
                    }
                    (spec::MirCastKind::FloatToFloat, Ty::Float(f), Ty::Float(g)) => {
                        let u = self.float_model(*g).of_float(*f, &v);
                        Ok(Val::V(self.float_use(u), to))
                    }
                    // A float is its bit pattern: `transmute` to or from the
                    // unsigned integer of its width is the value itself.
                    (spec::MirCastKind::Transmute, Ty::Float(f), Ty::Nat(Some(w)))
                    | (spec::MirCastKind::Transmute, Ty::Nat(Some(w)), Ty::Float(f))
                        if f.width() == *w =>
                    {
                        Ok(Val::V(v, to))
                    }
                    // `ptr as usize`, `expose_provenance`: the address. The
                    // provenance it exposes matters only to
                    // `with_exposed_provenance`, which is residue.
                    (spec::MirCastKind::PointerExposeProvenance, f, Ty::Nat(None))
                        if is_pointer(f) =>
                    {
                        Ok(Val::V(address(&v), to))
                    }
                    (spec::MirCastKind::Transmute, f, Ty::Nat(None)) if is_pointer(f) => {
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
            IrRvalue::Other(k, _) => Err(kind(k.path(), "")),
        }
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

    fn binary(
        &mut self,
        op: spec::MirBinOp,
        a: &IrOperand,
        b: &IrOperand,
        st: &mut State,
    ) -> Lift<Val> {
        use spec::MirBinOp as B;
        let (x, xt) = self.operand(a, st)?;
        let (y, yt) = self.operand(b, st)?;
        // Raw pointers compare by their addresses: `core::ptr` compares thin
        // pointers as addresses, provenance aside.
        // A wide pointer's metadata breaks a tie, as rustc's comparison of
        // wide pointers does; a thin pointer's is always the unit.
        if is_pointer(&xt)
            && is_pointer(&yt)
            && matches!(op, B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge)
        {
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
        if is_pointer(&xt) && matches!(op, B::Offset) {
            return Err(kind(
                op.path(),
                "an offset of a raw pointer, whose bounds are its allocation's, which the value model does not hold",
            ));
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
        if let (Ty::Float(f), true) = (&xt, xt == yt) {
            let (name, args) = match op {
                B::Add => ("Add", [&x, &y]),
                B::Sub => ("Sub", [&x, &y]),
                B::Mul => ("Mul", [&x, &y]),
                B::Div => ("Div", [&x, &y]),
                B::Rem => ("Rem", [&x, &y]),
                B::Eq | B::Ne => ("Eq", [&x, &y]),
                B::Lt => ("Lt", [&x, &y]),
                B::Le => ("Le", [&x, &y]),
                B::Gt => ("Lt", [&y, &x]),
                B::Ge => ("Le", [&y, &x]),
                _ => return Err(bad()),
            };
            let u = self
                .float_model(*f)
                .apply(name, &[args[0].as_str(), args[1].as_str()]);
            let term = self.float_use(u);
            return Ok(match op {
                B::Ne => Val::V(format!("(not {term})"), Ty::Bool),
                B::Add | B::Sub | B::Mul | B::Div | B::Rem => Val::V(term, xt.clone()),
                _ => Val::V(term, Ty::Bool),
            });
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
                        let name = "mirPowTwo".to_string();
                        if !self.aux.contains_key(&name) {
                            let evidence = format!("decreases_{name}_0");
                            self.aux.insert(
                                evidence.clone(),
                                Decl::Theorem {
                                    name: evidence.clone(),
                                    parameters: vec![("k".into(), "Nat".into())],
                                    statement: implication(
                                        &["((k =? 0) = false)".to_string()],
                                        "((prim subtract k 1 : Nat) < k)",
                                    ),
                                    proof: "linear_arithmetic{}".into(),
                                    axioms: vec!["Quot.sound".into(), "propext".into()],
                                },
                            );
                            self.aux.insert(
                                name.clone(),
                                Decl::Definition {
                                    name: name.clone(),
                                    parameters: vec![("k".into(), "Nat".into())],
                                    result: "Nat".into(),
                                    body: Expr::Flat(format!(
                                        "(if (k =? 0) then 1 else (prim multiply 2 ({name} (prim subtract k 1 : Nat)) : Nat))"
                                    )),
                                    axioms: vec!["Quot.sound".into(), "propext".into()],
                                    recursive: None,
                                    termination: Some(("k".into(), vec![evidence])),
                                    mutual: None,
                                },
                            );
                        }
                        self.calls.insert(name.clone());
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
                    ("Ok", [v]) => Ok(format!("(ctor Result.ok {args} {v})")),
                    ("Err", [v]) => Ok(format!("(ctor Result.error {args} {v})")),
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
            let _ = (captured, et);
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
            // hold apart.
            let cb = &self.cx.closures[&closure];
            if !threaded(&cb.locals, cb.arity, true).is_empty() {
                return Err(kind(
                    k,
                    "a closure that writes through a `&mut` argument, passed in the arguments' tuple",
                ));
            }
            let term = format!("({name} {})", items.join(" "));
            return Ok(Called {
                term,
                updates: BTreeMap::from([(0, env.clone())]),
                guard: None,
                threads: None,
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
        if let Some(v) = self.float_intrinsic(callee, &args, dt) {
            return Ok(Called::value(v));
        }
        if let Some(c) = self.float_method(callee, &args, dt) {
            return Ok(c);
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
            });
        }
        // Otherwise std's own MIR, at the call's types.
        match self.std_instance(callee, &args, texts, dest, dt)? {
            Some(called) => Ok(called),
            None => Err(kind(k, &erase(callee))),
        }
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
            ty(b, self.cx.types)?;
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
        let at = |t: &Ty| t.render();
        // A body that threads `&mut` parameters returns them after `_0`.
        let positions: Vec<usize> = threaded(&body.locals, body.arity, false)
            .into_iter()
            .map(|l| l - 1)
            .collect();
        let rest: Vec<Ty> = positions
            .iter()
            .filter_map(|&i| args.get(i).map(|(_, t)| t.clone()))
            .collect();
        let full = threaded_result(dt, &rest, self.cx.types);
        if parameters.len() != args.len()
            || parameters
                .iter()
                .zip(args)
                .any(|((_, p), (_, a))| *p != at(a))
            || *result != at(&full)
        {
            return Err(kind(k, &format!("{path}, at other types than the call's")));
        }
        self.calls.insert(name.clone());
        let a: Vec<String> = args.iter().map(|(v, _)| v.clone()).collect();
        let term = if a.is_empty() {
            name
        } else {
            format!("({name} {})", a.join(" "))
        };
        Ok(Some(Called {
            threads: (!positions.is_empty()).then_some((full, positions)),
            ..Called::value(term)
        }))
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
    let mut out = goal.to_string();
    for h in hyps.iter().rev() {
        out = format!("({h} -> {out})");
    }
    out
}

fn place_key(p: &IrPlace) -> String {
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
        IrRvalue::Ref(_, p) | IrRvalue::CopyForDeref(p) | IrRvalue::Discriminant(p) => {
            place_reads(p)
        }
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
    // A raw pointer, where the scope declares the pointer struct: a typed
    // address with its provenance and metadata (see [`POINTER`]). A pointer
    // to a trait object, whose metadata is a vtable, is residue.
    if pointee(t).is_some() && types.structs.contains_key(POINTER) {
        return Ok(Ty::Named(POINTER.to_string()));
    }
    if t.starts_with('*') || t.starts_with("dyn ") || t.starts_with("impl ") {
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
    let raw: Vec<&str> = split_top(args_text, ',')
        .into_iter()
        .filter(|a| !a.is_empty() && !a.starts_with('\''))
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
        if let Some(n) = types
            .aliases
            .get(t)
            .filter(|n| types.structs.contains_key(*n) || types.enums.contains_key(*n))
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
            return stdlib::model_type(&path, &args).ok_or_else(|| Residue::Type(t.to_string()));
        }
        (n, []) => {
            // A type of the crate by the full path MIR prints, then by name.
            let mut printed: Vec<&str> = segs[..segs.len() - 1].to_vec();
            printed.push(n);
            let r = types
                .aliases
                .get(&printed.join("::"))
                .cloned()
                .unwrap_or_else(|| types.resolve(n));
            if types.enums.contains_key(&r)
                || types.structs.contains_key(&r)
                || types.datas.contains_key(&r)
            {
                Ty::Named(r)
            } else {
                return std_printed(&segs, name, &args).ok_or_else(|| Residue::Type(t.to_string()));
            }
        }
        _ => return std_printed(&segs, name, &args).ok_or_else(|| Residue::Type(t.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn mut_parameters_are_threaded() {
        let locals: Vec<String> = ["()", "&mut u32", "u8", "&'a mut [u8]"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(threaded(&locals, 3, false), vec![1, 3]);
        assert_eq!(threaded(&locals, 3, true), vec![3]);
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
