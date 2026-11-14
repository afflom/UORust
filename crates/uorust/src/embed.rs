//! The deep embedding of MIR in LexLean (UE-19): every body a dump holds,
//! written as a value of the MIR datatype that `assets/embed/src/Mir.lex.tex`
//! declares, beside the one evaluator that module defines over it. The
//! embedding never refuses a body: what the parser reads structurally is
//! written as its construct, and what it keeps as text is written as the
//! `other` node of its kind, which the evaluator answers with
//! `Outcome.undefined` naming the kind. Types, generic arguments and calls
//! are data: a call is the body the linker resolves it to, or, where a
//! trait bound leaves the callee abstract, a name the caller's dictionary
//! (`Program.dictionary`) binds.
//!
//! Coverage is measured per construct, not per body. The constructs the
//! evaluator does not define are read from its own source: each is a
//! string it answers `Outcome.undefined` with, so the source stays the one
//! authority (R1). A body is blocked by every such construct it uses, and
//! the census ranks them by how many bodies they block: a finite queue,
//! since MIR's kinds are finite (`crate::mir::spec`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::mir::ir::{
    IrAggregate, IrBody, IrConstant, IrOperand, IrPlace, IrProjection, IrRvalue, IrStatement,
    IrTerminator,
};
use crate::mir::spec;
use crate::UorustError;

/// The MIR datatype and its evaluator.
pub(crate) const SEMANTICS: &str = include_str!("../assets/embed/src/Mir.lex.tex");

/// The lexicon the embedding's titles are words of.
const LEXICON: &str = include_str!("../assets/embed/language/uorust.mir/lexicon.toml");

/// The lexicon's entries, by file name.
const ENTRIES: [(&str, &str); 3] = [
    (
        "bodies.toml",
        include_str!("../assets/embed/language/uorust.mir/entries/bodies.toml"),
    ),
    (
        "checks.toml",
        include_str!("../assets/embed/language/uorust.mir/entries/checks.toml"),
    ),
    (
        "semantics.toml",
        include_str!("../assets/embed/language/uorust.mir/entries/semantics.toml"),
    ),
];

/// The semantics module's name, which every body module imports and
/// qualifies the datatype's names by.
const MIR: &str = "Mir";

/// Each enum by its name's last segment: its paths, and per path its
/// variants with their discriminants.
pub(crate) type Enums = BTreeMap<String, Vec<(String, Vec<(String, i128)>)>>;

/// How a callee resolves, from the callee as printed, its arguments' types
/// and its destination's, where MIR prints them.
pub(crate) type Resolve<'a> = dyn Fn(&str, &[Option<String>], &str) -> Callee + 'a;

/// What an embedded body's construction reads beside the body: the target's
/// pointer width (`usize`'s and `isize`'s), each enum's variants and
/// discriminants, and how a call's callee resolves.
pub(crate) struct Cx<'a> {
    pub(crate) width: u32,
    /// Each enum by its name's last segment: its paths, and per path its
    /// variants with their discriminants.
    pub(crate) enums: &'a Enums,
    /// The path of the body a printed callee names, where one is linked.
    /// Its arguments: the callee as printed, its arguments' types and its
    /// destination's, where MIR prints them.
    pub(crate) resolve: &'a Resolve<'a>,
}

/// How a call's callee is embedded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Callee {
    /// A body of the program, by its path.
    Body(String),
    /// An intrinsic, with no body of its own.
    Intrinsic(String),
    /// A callee a bound leaves abstract: the caller's dictionary binds it.
    Bound(String),
}

/// One embedded body: its term, the constructs it uses, those of them the
/// evaluator does not define, and the callees it leaves to a dictionary.
#[derive(Clone, Debug, Default)]
pub(crate) struct Embedded {
    pub(crate) term: String,
    pub(crate) uses: BTreeSet<String>,
    pub(crate) blocked: BTreeSet<String>,
    pub(crate) bound: BTreeSet<String>,
    /// The bodies of the program it calls or names, by key.
    pub(crate) callees: BTreeSet<String>,
    /// The intrinsics it calls, which have no body to evaluate.
    pub(crate) intrinsics: BTreeSet<String>,
}

/// The constructs the evaluator answers `Outcome.undefined` with, read
/// from its source: the string after each `Outcome.undefined`.
pub(crate) fn undefined_constructs() -> &'static BTreeSet<String> {
    static READ: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    READ.get_or_init(read_undefined)
}

fn read_undefined() -> BTreeSet<String> {
    let needle = "Outcome.undefined \"";
    SEMANTICS
        .match_indices(needle)
        .filter_map(|(at, _)| {
            let rest = &SEMANTICS[at + needle.len()..];
            rest.find('"').map(|end| rest[..end].to_string())
        })
        .collect()
}

/// A LexLean name of the datatype, qualified by the semantics module.
fn mir_name(name: &str) -> String {
    format!("{MIR}::{name}")
}

/// `(ctor Mir::T.c a b)`.
fn mir_ctor(name: &str, args: &[String]) -> String {
    if args.is_empty() {
        format!("(ctor {})", mir_name(name))
    } else {
        format!("(ctor {} {})", mir_name(name), args.join(" "))
    }
}

/// The constructor a kind of `spec` is in the datatype: `BinOp::AddUnchecked`
/// is `BinOp.addUnchecked`. `BorrowKind::Mut` is `mutable`, `mut` being a
/// Lean keyword.
pub(crate) fn kind_ctor(path: &str) -> String {
    let (ty, variant) = path.split_once("::").unwrap_or(("", path));
    let mut chars = variant.chars();
    let lower = match chars.next() {
        Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    };
    let lower = if lower == "mut" {
        "mutable".to_string()
    } else {
        lower
    };
    format!("{ty}.{lower}")
}

/// A list term of items of type `ty`. A long list is the `append` of runs
/// of about its length's square root, so neither a run nor the appends nest
/// deeper than that however long it grows: the linker reads a term to a
/// bounded depth.
fn term_list(items: &[String], ty: &str) -> String {
    let chain = |part: &[String]| {
        let mut out = format!("(nil {ty})");
        for item in part.iter().rev() {
            out = format!("(cons {item} {out})");
        }
        out
    };
    let run = items.len().isqrt() + 1;
    if items.len() <= run {
        return chain(items);
    }
    let runs: Vec<String> = items.chunks(run).map(chain).collect();
    let mut out = runs.last().cloned().unwrap_or_default();
    for r in runs.iter().rev().skip(1) {
        out = format!("(prim append {r} {out} : (List {ty}))");
    }
    out
}

/// The width and signedness of an integer type as MIR prints it: `u8`,
/// `i128`, `usize` (the target's pointer width).
fn int_ty(printed: &str, width: u32) -> Option<(u32, bool)> {
    match printed {
        "usize" => return Some((width, false)),
        "isize" => return Some((width, true)),
        _ => {}
    }
    let signed = match printed.chars().next()? {
        'u' => false,
        'i' => true,
        _ => return None,
    };
    printed[1..]
        .parse::<u32>()
        .ok()
        .filter(|b| *b > 0)
        .map(|b| (b, signed))
}

/// The two's complement bits of `n` at `bits`.
fn repr(n: i128, bits: u32) -> u128 {
    // `n as u128` is n's two's complement at 128 bits; the low `bits` of
    // it are n's at `bits`.
    let full = n as u128;
    match 1u128.checked_shl(bits) {
        Some(m) if bits < u128::BITS => full & (m - 1),
        _ => full,
    }
}

/// `Mir::Signed`.
fn signed(n: i128) -> String {
    format!(
        "(record {} {{negative := {}, magnitude := {}}})",
        mir_name("Signed"),
        n < 0,
        n.unsigned_abs()
    )
}

/// A LexLean string literal of `text`, every character kept: a quote and a
/// backslash escaped, and a control character and `%` (which LexLean's TeX
/// surface reserves) written as their `\\u` escape.
fn mir_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c == '%' || c.is_control() => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{u:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Cx<'_> {
    fn ty(&self, printed: &str) -> String {
        let printed = printed.trim();
        if let Some((bits, s)) = int_ty(printed, self.width) {
            return mir_ctor("Ty.int", &[bits.to_string(), s.to_string()]);
        }
        match printed {
            "bool" => mir_ctor("Ty.boolean", &[]),
            "char" => mir_ctor("Ty.character", &[]),
            "()" => mir_ctor("Ty.unit", &[]),
            _ => mir_ctor("Ty.other", &[mir_string(printed)]),
        }
    }

    /// The discriminant of `variant` of the enum MIR prints as `ty`, where
    /// core declares one enum by that name with that variant.
    fn discriminant(&self, ty: &str, variant: &str) -> Option<i128> {
        let erased = crate::mir::lower::erase(ty);
        let last = crate::mir::parse::split_path(&erased)
            .last()
            .copied()
            .unwrap_or(&erased)
            .trim_end_matches("::")
            .to_string();
        let candidates = self.enums.get(&last)?;
        let mut found = candidates.iter().filter(|(path, variants)| {
            (candidates.len() == 1 || erased.ends_with(path.as_str()))
                && variants.iter().any(|(v, _)| v == variant)
        });
        let (_, variants) = found.next()?;
        if found.next().is_some() {
            return None;
        }
        variants.iter().find(|(v, _)| v == variant).map(|(_, d)| *d)
    }

    fn place(&self, p: &IrPlace, e: &mut Embedded) -> String {
        let steps: Vec<String> = p
            .projection
            .iter()
            .map(|step| {
                use spec::MirProjectionElem as P;
                let (kind, term) = match step {
                    IrProjection::Deref => (P::Deref, mir_ctor("Proj.deref", &[])),
                    IrProjection::Field(i, _) => {
                        (P::Field, mir_ctor("Proj.field", &[i.to_string()]))
                    }
                    IrProjection::Index(l) => (P::Index, mir_ctor("Proj.index", &[l.to_string()])),
                    IrProjection::ConstantIndex(n, back) => (
                        P::ConstantIndex,
                        mir_ctor("Proj.constantIndex", &[n.to_string(), back.to_string()]),
                    ),
                    IrProjection::Downcast(v) => {
                        (P::Downcast, mir_ctor("Proj.downcast", &[mir_string(v)]))
                    }
                    IrProjection::Other(k, text) => {
                        e.blocked.insert(k.path().to_string());
                        (
                            *k,
                            mir_ctor("Proj.other", &[mir_string(k.path()), mir_string(text)]),
                        )
                    }
                };
                e.uses.insert(kind.path().to_string());
                term
            })
            .collect();
        format!(
            "(record {} {{base := {}, projection := {}}})",
            mir_name("Place"),
            p.local,
            term_list(&steps, &mir_name("Proj"))
        )
    }

    fn constant(&self, c: &IrConstant, e: &mut Embedded) -> String {
        match c {
            IrConstant::Int(n, suffix) => match int_ty(suffix, self.width) {
                Some((bits, s)) => mir_ctor(
                    "Const.int",
                    &[bits.to_string(), s.to_string(), repr(*n, bits).to_string()],
                ),
                None => self.unread(c, e),
            },
            IrConstant::Bool(b) => mir_ctor("Const.boolean", &[b.to_string()]),
            IrConstant::Str(s) => mir_ctor("Const.text", &[mir_string(s)]),
            IrConstant::Char(ch) => mir_ctor("Const.character", &[u32::from(*ch).to_string()]),
            IrConstant::Unit => mir_ctor("Const.unit", &[]),
            IrConstant::Path(p) => {
                let segments = crate::mir::parse::split_path(p);
                let variant = match segments.as_slice() {
                    [.., ty, v] => self.discriminant(ty, v).map(|d| ((*v).to_string(), d)),
                    _ => None,
                };
                match variant {
                    Some((v, d)) => mir_ctor("Const.variant", &[mir_string(&v), signed(d)]),
                    None => match (self.resolve)(p, &[], "") {
                        Callee::Body(path) => {
                            e.callees.insert(path.clone());
                            mir_ctor("Const.function", &[mir_string(&path)])
                        }
                        Callee::Intrinsic(path) => {
                            e.intrinsics.insert(path.clone());
                            e.blocked.insert(format!("intrinsic {path}"));
                            mir_ctor("Const.function", &[mir_string(&path)])
                        }
                        Callee::Bound(name) => {
                            e.bound.insert(name.clone());
                            mir_ctor("Const.function", &[mir_string(&name)])
                        }
                    },
                }
            }
            IrConstant::Other(_) => self.unread(c, e),
        }
    }

    /// A constant the evaluator does not read: the construct it answers
    /// with, named as it names it.
    fn unread(&self, c: &IrConstant, e: &mut Embedded) -> String {
        let text = match c {
            IrConstant::Other(t) => t.clone(),
            IrConstant::Int(n, s) => format!("{n}_{s}"),
            _ => String::new(),
        };
        e.blocked
            .insert("Operand::Constant of a constant the embedding does not read".to_string());
        mir_ctor("Const.other", &[mir_string(&text)])
    }

    fn operand(&self, o: &IrOperand, e: &mut Embedded) -> String {
        use spec::MirOperand as O;
        let (kind, term) = match o {
            IrOperand::Copy(p) => (O::Copy, mir_ctor("Operand.copy", &[self.place(p, e)])),
            IrOperand::Move(p) => (O::Move, mir_ctor("Operand.move", &[self.place(p, e)])),
            IrOperand::Constant(c) => (
                O::Constant,
                mir_ctor("Operand.constant", &[self.constant(c, e)]),
            ),
            IrOperand::Check(k) => {
                e.uses.insert(k.path().to_string());
                (
                    O::RuntimeChecks,
                    mir_ctor("Operand.check", &[mir_ctor(&kind_ctor(k.path()), &[])]),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    fn operands<'o>(
        &self,
        ops: impl IntoIterator<Item = &'o IrOperand>,
        e: &mut Embedded,
    ) -> String {
        let terms: Vec<String> = ops.into_iter().map(|o| self.operand(o, e)).collect();
        term_list(&terms, &mir_name("Operand"))
    }

    /// An rvalue assigned to a place of type `dest`.
    fn rvalue(&self, r: &IrRvalue, dest: Option<&str>, e: &mut Embedded) -> String {
        use spec::MirRvalue as R;
        let undefined = undefined_constructs();
        let gap = |label: &str, e: &mut Embedded| {
            if undefined.contains(label) {
                e.blocked.insert(label.to_string());
            }
        };
        let (kind, term) = match r {
            IrRvalue::Use(o) => (R::Use, mir_ctor("Rvalue.value", &[self.operand(o, e)])),
            IrRvalue::Ref(k, p) => {
                e.uses.insert(k.path().to_string());
                (
                    R::Ref,
                    mir_ctor(
                        "Rvalue.borrow",
                        &[mir_ctor(&kind_ctor(k.path()), &[]), self.place(p, e)],
                    ),
                )
            }
            IrRvalue::CopyForDeref(p) => (
                R::CopyForDeref,
                mir_ctor("Rvalue.copyForDeref", &[self.place(p, e)]),
            ),
            IrRvalue::Binary(op, a, b) => {
                e.uses.insert(op.path().to_string());
                gap(op.path(), e);
                let (a, b) = (self.operand(a, e), self.operand(b, e));
                (
                    R::BinaryOp,
                    mir_ctor(
                        "Rvalue.binary",
                        &[mir_ctor(&kind_ctor(op.path()), &[]), a, b],
                    ),
                )
            }
            IrRvalue::Unary(op, a) => {
                e.uses.insert(op.path().to_string());
                gap(op.path(), e);
                (
                    R::UnaryOp,
                    mir_ctor(
                        "Rvalue.unary",
                        &[mir_ctor(&kind_ctor(op.path()), &[]), self.operand(a, e)],
                    ),
                )
            }
            IrRvalue::Discriminant(p) => {
                let ty = self.ty(dest.unwrap_or_default());
                if ty.contains("Ty.other") {
                    gap(
                        "Rvalue::Discriminant at a type the embedding does not read",
                        e,
                    );
                }
                (
                    R::Discriminant,
                    mir_ctor("Rvalue.discriminant", &[self.place(p, e), ty]),
                )
            }
            IrRvalue::Aggregate(a, ops) => {
                use spec::MirAggregateKind as A;
                let (k, agg) = match a {
                    IrAggregate::Tuple => (A::Tuple, mir_ctor("Agg.tuple", &[])),
                    IrAggregate::Array => (A::Array, mir_ctor("Agg.array", &[])),
                    IrAggregate::Adt(ty, variant) => {
                        let v = variant.clone().unwrap_or_default();
                        let d = variant
                            .as_deref()
                            .and_then(|v| self.discriminant(ty, v))
                            .unwrap_or_default();
                        if variant.is_some() && self.discriminant(ty, &v).is_none() {
                            e.blocked.insert(
                                "AggregateKind::Adt of an enum whose discriminants core's source does not give"
                                    .to_string(),
                            );
                        }
                        (A::Adt, mir_ctor("Agg.adt", &[mir_string(&v), signed(d)]))
                    }
                    IrAggregate::Closure(t) => {
                        (A::Closure, mir_ctor("Agg.closure", &[mir_string(t)]))
                    }
                    IrAggregate::Other(k) => {
                        e.blocked.insert(k.path().to_string());
                        (*k, mir_ctor("Agg.other", &[mir_string(k.path())]))
                    }
                };
                e.uses.insert(k.path().to_string());
                (
                    R::Aggregate,
                    mir_ctor(
                        "Rvalue.aggregate",
                        &[agg, self.operands(ops.iter().map(|(_, o)| o), e)],
                    ),
                )
            }
            IrRvalue::Cast(k, o, ty) => {
                e.uses.insert(k.path().to_string());
                gap(k.path(), e);
                let ty = self.ty(ty);
                if *k == spec::MirCastKind::IntToInt && ty.contains("Ty.other") {
                    gap("CastKind::IntToInt to a type that is not an integer", e);
                }
                (
                    R::Cast,
                    mir_ctor(
                        "Rvalue.cast",
                        &[mir_ctor(&kind_ctor(k.path()), &[]), self.operand(o, e), ty],
                    ),
                )
            }
            IrRvalue::RawPtr(_, p) | IrRvalue::MetadataPtr(p) => {
                (R::RawPtr, mir_ctor("Rvalue.rawPtr", &[self.place(p, e)]))
            }
            IrRvalue::Other(k, text) => {
                e.blocked.insert(k.path().to_string());
                (
                    *k,
                    mir_ctor("Rvalue.other", &[mir_string(k.path()), mir_string(text)]),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    fn statement(&self, s: &IrStatement, body: &IrBody, e: &mut Embedded) -> String {
        use spec::MirStatementKind as S;
        let (kind, term) = match s {
            IrStatement::Assign(p, r) => {
                let dest = p
                    .projection
                    .is_empty()
                    .then(|| body.locals.get(p.local).map(String::as_str))
                    .flatten();
                let place = self.place(p, e);
                (
                    S::Assign,
                    mir_ctor("Statement.assign", &[place, self.rvalue(r, dest, e)]),
                )
            }
            IrStatement::Marker(k) => (*k, mir_ctor("Statement.marker", &[mir_string(k.path())])),
            IrStatement::Assume(o) => {
                e.uses
                    .insert(spec::MirNonDivergingIntrinsic::Assume.path().to_string());
                (
                    S::Intrinsic,
                    mir_ctor("Statement.assume", &[self.operand(o, e)]),
                )
            }
            IrStatement::Other(k, text) => {
                e.blocked.insert(k.path().to_string());
                (
                    *k,
                    mir_ctor("Statement.other", &[mir_string(k.path()), mir_string(text)]),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    fn terminator(&self, t: &IrTerminator, body: &IrBody, e: &mut Embedded) -> String {
        use spec::MirTerminatorKind as T;
        let (kind, term) = match t {
            IrTerminator::Goto(b) => (T::Goto, mir_ctor("Terminator.jump", &[b.to_string()])),
            IrTerminator::False(k, b) => (*k, mir_ctor("Terminator.jump", &[b.to_string()])),
            IrTerminator::Switch(o, arms, otherwise) => {
                let arms: Vec<String> = arms
                    .iter()
                    .map(|(v, b)| {
                        // A switch's values are printed as the bits of the
                        // operand's type, which are never negative.
                        format!(
                            "(record {} {{value := {}, target := {b}}})",
                            mir_name("Arm"),
                            repr(*v, u128::BITS)
                        )
                    })
                    .collect();
                (
                    T::SwitchInt,
                    mir_ctor(
                        "Terminator.switch",
                        &[
                            self.operand(o, e),
                            term_list(&arms, &mir_name("Arm")),
                            otherwise.to_string(),
                        ],
                    ),
                )
            }
            IrTerminator::Return => (T::Return, mir_ctor("Terminator.exit", &[])),
            IrTerminator::Unreachable => (T::Unreachable, mir_ctor("Terminator.unreachable", &[])),
            IrTerminator::Drop(b) => (T::Drop, mir_ctor("Terminator.drop", &[b.to_string()])),
            IrTerminator::Call {
                destination,
                callee,
                args,
                target,
            } => {
                let callee = match crate::mir::parse::operand(callee) {
                    Some(o @ (IrOperand::Copy(_) | IrOperand::Move(_))) => {
                        mir_ctor("Callee.operand", &[self.operand(&o, e)])
                    }
                    _ => match (self.resolve)(
                        callee,
                        &args.iter().map(|a| arg_type(body, a)).collect::<Vec<_>>(),
                        &place_type(body, destination).unwrap_or_default(),
                    ) {
                        Callee::Body(p) => {
                            e.callees.insert(p.clone());
                            mir_ctor("Callee.body", &[mir_string(&p)])
                        }
                        Callee::Intrinsic(p) => {
                            e.intrinsics.insert(p.clone());
                            e.blocked.insert(format!("intrinsic {p}"));
                            mir_ctor("Callee.intrinsic", &[mir_string(&p)])
                        }
                        Callee::Bound(n) => {
                            e.bound.insert(n.clone());
                            mir_ctor("Callee.bound", &[mir_string(&n)])
                        }
                    },
                };
                let target = match target {
                    Some(b) => format!("(ctor Option.some [Nat] {b})"),
                    None => "(ctor Option.none [Nat])".to_string(),
                };
                let destination = self.place(destination, e);
                let args = self.operands(args, e);
                (
                    T::Call,
                    mir_ctor("Terminator.call", &[destination, callee, args, target]),
                )
            }
            IrTerminator::Assert {
                condition,
                expected,
                kind,
                target,
            } => {
                e.uses.insert(kind.path().to_string());
                (
                    T::Assert,
                    mir_ctor(
                        "Terminator.assert",
                        &[
                            self.operand(condition, e),
                            expected.to_string(),
                            mir_ctor(&kind_ctor(kind.path()), &[]),
                            target.to_string(),
                        ],
                    ),
                )
            }
            IrTerminator::Other(k, text) => {
                e.blocked.insert(k.path().to_string());
                (
                    *k,
                    mir_ctor(
                        "Terminator.other",
                        &[mir_string(k.path()), mir_string(text)],
                    ),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    /// The body `b`, named `path` in the program, as a `Mir::Body` term.
    pub(crate) fn body(&self, b: &IrBody, path: &str) -> Embedded {
        let mut e = Embedded::default();
        let locals: Vec<String> = b.locals.iter().map(|t| self.ty(t)).collect();
        // The blocks control reaches from the entry without unwinding: the
        // evaluator ends a run at a panic, so a cleanup block is never
        // evaluated, and what it uses blocks nothing.
        let mut reached = BTreeSet::from([0]);
        let mut work = vec![0];
        while let Some(at) = work.pop() {
            for next in b
                .blocks
                .get(at)
                .map(|k| k.terminator.successors())
                .unwrap_or_default()
            {
                if reached.insert(next) {
                    work.push(next);
                }
            }
        }
        let blocks: Vec<String> = b
            .blocks
            .iter()
            .enumerate()
            .map(|(i, block)| {
                let mut cleanup = Embedded::default();
                let e = if reached.contains(&i) {
                    &mut e
                } else {
                    &mut cleanup
                };
                let statements: Vec<String> = block
                    .statements
                    .iter()
                    .map(|s| self.statement(s, b, e))
                    .collect();
                format!(
                    "(record {} {{statements := {}, terminator := {}}})",
                    mir_name("Block"),
                    term_list(&statements, &mir_name("Statement")),
                    self.terminator(&block.terminator, b, e)
                )
            })
            .collect();
        e.term = format!(
            "(record {} {{path := {}, arity := {}, locals := {}, blocks := {}}})",
            mir_name("Body"),
            mir_string(path),
            b.arity,
            term_list(&locals, &mir_name("Ty")),
            term_list(&blocks, &mir_name("Block"))
        );
        e
    }
}

/// Each enum core's source declares, by its name's last segment, with its
/// variants' discriminants: those its declaration sets, else each
/// variant's index.
pub(crate) fn enums(decls: &crate::mir::core::CoreDecls) -> Enums {
    let mut out: Enums = BTreeMap::new();
    for s in &decls.structs {
        let names: Vec<String> = match (&s.variants, &s.data) {
            (Some(v), _) => v.clone(),
            (None, Some(d)) => d.iter().map(|(v, _)| v.clone()).collect(),
            (None, None) => continue,
        };
        let values: Vec<i128> = match &s.values {
            Some(vs) if vs.len() == names.len() => vs
                .iter()
                .map(|v| crate::mir::core::discriminant(*v, *v < 0))
                .collect(),
            _ => (0..).take(names.len()).collect(),
        };
        let last = crate::mir::parse::split_path(&s.path)
            .last()
            .copied()
            .unwrap_or(&s.path)
            .to_string();
        out.entry(last)
            .or_default()
            .push((s.path.clone(), names.into_iter().zip(values).collect()));
    }
    out
}

/// The name of a body's definition: `b` and the first half of its κ.
pub(crate) fn def_name(kappa: &str) -> String {
    format!("b{}", &kappa[..kappa.len() / 2])
}

/// What `uorust embed` writes and measures.
#[derive(Clone, Debug, Default)]
pub struct EmbedCensus {
    /// The bodies embedded.
    pub bodies: usize,
    /// Those whose every construct the evaluator defines.
    pub evaluable: usize,
    /// For each construct, how many bodies use it.
    pub uses: BTreeMap<String, usize>,
    /// For each construct the evaluator does not define, how many bodies it
    /// blocks.
    pub blocked: BTreeMap<String, usize>,
    /// How many bodies leave a callee to a dictionary.
    pub bound: usize,
}

impl EmbedCensus {
    fn add(&mut self, e: &Embedded) {
        self.bodies += 1;
        if e.blocked.is_empty() {
            self.evaluable += 1;
        }
        for u in &e.uses {
            *self.uses.entry(u.clone()).or_default() += 1;
        }
        for b in &e.blocked {
            *self.blocked.entry(b.clone()).or_default() += 1;
        }
        if !e.bound.is_empty() {
            self.bound += 1;
        }
    }

    /// The census as TOML: the counts, then each blocking construct, most
    /// bodies first, then each construct's uses.
    pub fn render(&self) -> String {
        let mut out = format!(
            "# The embedding's census (written by `uorust embed`; do not edit): every\n\
             # body embedded, those whose every construct the evaluator defines, and\n\
             # the constructs it does not, by the bodies each blocks.\n\n\
             bodies = {}\nevaluable = {}\nbound = {}\n",
            self.bodies, self.evaluable, self.bound
        );
        let mut ranked: Vec<(&String, &usize)> = self.blocked.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (c, n) in ranked {
            let _ = write!(
                out,
                "\n[[blocked]]\nconstruct = {}\nbodies = {n}\n",
                census_string(c)
            );
        }
        for (c, n) in &self.uses {
            let _ = write!(
                out,
                "\n[[construct]]\nconstruct = {}\nbodies = {n}\n",
                census_string(c)
            );
        }
        out
    }
}

fn census_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// The LexLean project of the embedding: `Mir` and, for each module, its
/// bodies in parts of at most `budget` bytes, each a definition named by
/// its κ ([`def_name`]).
pub(crate) fn embed_project(
    modules: &[(String, Vec<(String, Embedded)>)],
    budget: usize,
) -> BTreeMap<String, Vec<u8>> {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert(format!("src/{MIR}.lex.tex"), SEMANTICS.as_bytes().to_vec());
    files.insert(
        "language/uorust.mir/lexicon.toml".into(),
        LEXICON.as_bytes().to_vec(),
    );
    for (name, text) in ENTRIES {
        files.insert(
            format!("language/uorust.mir/entries/{name}"),
            text.as_bytes().to_vec(),
        );
    }
    let mut entrypoints = vec![format!("src/{MIR}.lex.tex")];
    for (module, bodies) in modules {
        let pascal = crate::lex::pascal(&[module]);
        let mut parts: Vec<Vec<String>> = vec![Vec::new()];
        let mut size = 0;
        for (kappa, e) in bodies {
            let decl = format!(
                "definition{{\n  name: {},\n  parameters: [],\n  result: {},\n  body: {}\n}}",
                def_name(kappa),
                mir_name("Body"),
                e.term
            );
            if size + decl.len() > budget && !parts.last().is_some_and(Vec::is_empty) {
                parts.push(Vec::new());
                size = 0;
            }
            size += decl.len() + 2;
            if let Some(last) = parts.last_mut() {
                last.push(decl);
            }
        }
        for (k, decls) in parts.iter().enumerate().filter(|(_, d)| !d.is_empty()) {
            let name = format!("Core.{pascal}Part{k}");
            let path = format!("src/Core/{pascal}Part{k}.lex.tex");
            let text = format!(
                "\\begin{{lexlean}}{{{name}}}\n\\useglossary{{lexlean.std.bool@1.2.0}}\n\
                 \\useglossary{{lexlean.std.nat@1.2.0}}\n\\useglossary{{uorust.mir@1.0.0}}\n\
                 \\importmodule{{{MIR}}}\n\\title{{Embedded MIR bodies}}\n\\begin{{semanticmodule}}\n{}\n\
                 \\end{{semanticmodule}}\n\\end{{lexlean}}\n",
                decls.join("\n\n")
            );
            entrypoints.push(path.clone());
            files.insert(path, text.into_bytes());
        }
    }
    entrypoints.sort();
    let toolchain = include_str!("../assets/project/lean-toolchain");
    let prefix = "UorustEmbed";
    let lexlean = format!(
        "spec = \"lexlean/project/1\"\nname = \"uorust-embed\"\nlanguage = \"1.2\"\n\
         module_prefix = \"{prefix}\"\nsource_roots = [\"src\"]\nentrypoints = [{}]\n\
         build_root = \".lexlean\"\nlockfile = \"lexlean.lock\"\nlean_workspace = \".\"\n\
         lean_toolchain = \"{}\"\n\n\
         [[lexicon_source]]\npackage = \"lexlean.std.nat\"\nkind = \"builtin\"\n\n\
         [[lexicon_source]]\npackage = \"uorust.mir\"\nkind = \"path\"\npath = \"language/uorust.mir\"\n\n\
         [limits]\nmax_file_bytes = {budget}\nmax_total_source_bytes = 67108864\n\
         max_primitive_atoms = 2000000\nmax_token_lattice_edges = 4000000\nmax_parse_states = 4000000\n\
         max_ir_nodes = 2000000\nmax_scope_depth = 1024\nmax_import_depth = 128\nmax_diagnostics = 256\n\
         max_child_output_bytes = 16777216\nchild_timeout_ms = 300000\n",
        entrypoints
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", "),
        toolchain.trim(),
    );
    files.insert("lexlean.toml".into(), lexlean.into_bytes());
    files.insert(
        "lakefile.toml".into(),
        format!("name = \"{prefix}\"\nversion = \"0.1.0\"\ndefaultTargets = [\"{prefix}\"]\n\n[[lean_lib]]\nname = \"{prefix}\"\n").into_bytes(),
    );
    files.insert(
        "lake-manifest.json".into(),
        format!("{{\"version\":\"1.2.0\",\"packagesDir\":\".lake/packages\",\"packages\":[],\"name\":\"{prefix}\",\"lakeDir\":\".lake\",\"fixedToolchain\":false}}\n").into_bytes(),
    );
    files.insert("lean-toolchain".into(), toolchain.as_bytes().to_vec());
    files
}

/// Embed every body of the module shards under `dumps` (one directory per
/// module; `only`, when nonempty, the modules to embed) whose sources are
/// under `library` (rust-src's `library`), and write the LexLean project to
/// `out` with the census, `census.toml`.
///
/// The differential check is two runs. With `harness`, a Rust crate is
/// written there that calls each sampled body's function, as rustc
/// compiles core, on inputs drawn from its types' widths, and prints what
/// each returns. With `observed`, that output, the project gains the
/// module `Differential`: for each input a theorem that the evaluator,
/// run on the embedded body, returns what rustc's build returned, which
/// `lexlean verify` decides, and `differential.toml` names each theorem's
/// body and inputs.
pub fn embed(
    library: &Path,
    dumps: &Path,
    out: &Path,
    only: &[String],
    harness: Option<&Path>,
    observed: Option<&Path>,
) -> Result<EmbedCensus, UorustError> {
    let root = library.parent().unwrap_or(library);
    let (modules, census, program) = embed_census(root, dumps, only)?;
    let mut files = embed_project(&modules, crate::project::MAX_FILE_BYTES);
    files.insert("census.toml".into(), census.render().into_bytes());
    let io = |p: &Path, e: std::io::Error| UorustError::Tool(format!("{}: {e}", p.display()));
    if let Some(observed) = observed {
        let text = std::fs::read_to_string(observed).map_err(|e| io(observed, e))?;
        let (module, index) = differential(&program, observed, &text)?;
        files.insert("src/Differential.lex.tex".into(), module.into_bytes());
        files.insert("differential.toml".into(), index.into_bytes());
        let config = String::from_utf8_lossy(&files["lexlean.toml"]).replace(
            "entrypoints = [",
            "entrypoints = [\"src/Differential.lex.tex\", ",
        );
        files.insert("lexlean.toml".into(), config.into_bytes());
    }
    let mut written = vec![(out.to_path_buf(), files)];
    // A module none of whose bodies is sampled has no harness.
    if let Some(dir) = harness {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        if !program.samples.is_empty() {
            written.push((dir.to_path_buf(), harness_crate(&program)));
        }
    }
    for (dir, files) in written {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
        }
        for (path, bytes) in &files {
            let file = dir.join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
            }
            std::fs::write(&file, bytes).map_err(|e| io(&file, e))?;
        }
    }
    Ok(census)
}

/// The rust-src `library` directory under the root the dumps name their
/// sources from.
fn library_root(root: &Path) -> std::path::PathBuf {
    root.join("library")
}

/// A scalar type of a sampled signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sampled {
    Int(u32, bool),
    Bool,
}

impl Sampled {
    fn of(printed: &str, width: u32) -> Option<Self> {
        if printed == "bool" {
            return Some(Self::Bool);
        }
        int_ty(printed, width).map(|(b, s)| Self::Int(b, s))
    }

    /// The inputs a check draws: the values at the edges of the type's
    /// range and its width, as two's complement bits.
    fn inputs(self) -> Vec<u128> {
        match self {
            Self::Bool => vec![0, 1],
            Self::Int(bits, signed) => {
                let top = 1u128 << (bits - 1);
                let max = top - 1 + top;
                let mut v = vec![0, 1, u128::from(bits - 1), top, top - 1, max];
                if !signed {
                    v.push(2);
                }
                v.sort_unstable();
                v.dedup();
                v
            }
        }
    }

    /// `x` as a Rust expression of the type.
    fn rust(self, printed: &str, x: u128) -> String {
        match self {
            Self::Bool => (x == 1).to_string(),
            Self::Int(_, false) => format!("{x}{printed}"),
            Self::Int(..) => format!("({x}{} as {printed})", unsigned(printed)),
        }
    }

    /// `x` as a `Mir::Value` term.
    fn value(self, x: u128) -> String {
        match self {
            Self::Bool => mir_ctor("Value.boolean", &[(x == 1).to_string()]),
            Self::Int(bits, signed) => mir_ctor(
                "Value.int",
                &[bits.to_string(), signed.to_string(), x.to_string()],
            ),
        }
    }
}

/// The unsigned integer type of a signed one's width: `i32` is `u32`'s.
fn unsigned(printed: &str) -> String {
    match printed.strip_prefix('i') {
        Some(rest) => format!("u{rest}"),
        None => printed.to_string(),
    }
}

/// A body the differential check runs: its key in the program, its
/// definition's name, the function rustc's build calls it by, its
/// parameters' and result's types as printed and read, and the bodies it
/// reaches.
#[derive(Clone, Debug)]
pub(crate) struct Sample {
    key: String,
    kappa: String,
    function: String,
    params: Vec<(String, Sampled)>,
    result: (String, Sampled),
    closure: Vec<String>,
}

impl Sample {
    /// Each case's inputs: the `i`th case takes each parameter's
    /// `(i + j)`th input, cycling, for as many cases as the parameter with
    /// the most inputs has.
    fn cases(&self) -> Vec<Vec<u128>> {
        let inputs: Vec<Vec<u128>> = self.params.iter().map(|(_, s)| s.inputs()).collect();
        let n = inputs.iter().map(Vec::len).max().unwrap_or(0);
        (0..n)
            .map(|i| {
                inputs
                    .iter()
                    .enumerate()
                    .map(|(j, v)| v[(i + j) % v.len()])
                    .collect()
            })
            .collect()
    }
}

/// The body `b` as a sample, where it is a stable, safe method of a
/// primitive integer type that core's source under `num` declares, over
/// scalars.
fn sample(
    root: &Path,
    num: &Path,
    b: &IrBody,
    key: &str,
    kappa: &str,
    width: u32,
) -> Option<Sample> {
    let display = crate::mir::resolve_impls(root, &b.path).unwrap_or_else(|| b.path.clone());
    let [_, ty, name] = crate::mir::parse::split_path(&display)[..] else {
        return None;
    };
    int_ty(ty, width)?;
    let scalar = |i: usize| {
        let t = b.locals.get(i)?;
        Some((t.clone(), Sampled::of(t, width)?))
    };
    let params: Vec<(String, Sampled)> = (1..=b.arity).map(scalar).collect::<Option<_>>()?;
    let result = scalar(0)?;
    if params.is_empty() || !stable_and_safe(num, name) {
        return None;
    }
    Some(Sample {
        key: key.to_string(),
        kappa: kappa.to_string(),
        function: format!("{ty}::{name}"),
        params,
        result,
        closure: Vec::new(),
    })
}

/// Whether every declaration of `fn name` in the sources under `num` is
/// stable and safe: its attributes say `#[stable(` and not `#[unstable(`,
/// and it is no `unsafe fn`. A harness on the stable toolchain can call it,
/// and on any input.
fn stable_and_safe(num: &Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(num) else {
        return false;
    };
    let decl = format!("fn {name}(");
    let mut found = false;
    for entry in entries.filter_map(Result::ok) {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains(&decl) {
                continue;
            }
            found = true;
            if line.contains("unsafe fn") {
                return false;
            }
            // Its attributes and doc comment: the lines above it up to the
            // end of the item before.
            let mut attrs = String::new();
            for above in lines[..i].iter().rev() {
                let t = above.trim();
                if t.is_empty() || t.ends_with('}') || t.ends_with(';') {
                    break;
                }
                attrs.push_str(t);
            }
            if !attrs.contains("#[stable(") || attrs.contains("#[unstable(") {
                return false;
            }
        }
    }
    found
}

/// Every body the program embeds, by key, with its κ, and the samples.
pub(crate) struct Program {
    every: BTreeMap<String, (String, Embedded)>,
    samples: Vec<Sample>,
}

/// The keys of the bodies `key` reaches, itself first, where the evaluator
/// defines each: `None` where one is not embedded, is blocked, calls an
/// intrinsic or leaves a callee to a dictionary.
fn closure(every: &BTreeMap<String, (String, Embedded)>, key: &str) -> Option<Vec<String>> {
    let mut seen = vec![key.to_string()];
    let mut at = 0;
    while let Some(k) = seen.get(at).cloned() {
        let (_, e) = every.get(&k)?;
        if !e.blocked.is_empty() || !e.bound.is_empty() || !e.intrinsics.is_empty() {
            return None;
        }
        for c in &e.callees {
            if !seen.contains(c) {
                seen.push(c.clone());
            }
        }
        at += 1;
    }
    Some(seen)
}

/// The harness crate: one call per case, under `catch_unwind`, each
/// printed as `<case> <result>` with an integer as its two's complement
/// bits, after a first line with the build's checks.
fn harness_crate(program: &Program) -> BTreeMap<String, Vec<u8>> {
    let mut main = String::from(
        "// Written by `uorust embed --harness`; do not edit.\n\
         #![allow(unused_parens)]\n\
         fn main() {\n    std::panic::set_hook(Box::new(|_| {}));\n    \
         let overflow = std::panic::catch_unwind(|| std::hint::black_box(u8::MAX) + 1).is_err();\n    \
         println!(\"checks {} {}\", cfg!(debug_assertions), overflow);\n",
    );
    let mut case = 0usize;
    for s in &program.samples {
        for inputs in s.cases() {
            let args: Vec<String> = s
                .params
                .iter()
                .zip(&inputs)
                .map(|((printed, scalar), x)| scalar.rust(printed, *x))
                .collect();
            let show = match s.result.1 {
                Sampled::Bool => "r.to_string()".to_string(),
                Sampled::Int(..) => {
                    format!("((r as {}) as u128).to_string()", unsigned(&s.result.0))
                }
            };
            let _ = write!(
                main,
                "    match std::panic::catch_unwind(|| {}({})) {{\n        \
                 Ok(r) => println!(\"{case} {{}}\", {show}),\n        \
                 Err(_) => println!(\"{case} panic\"),\n    }}\n",
                s.function,
                args.join(", ")
            );
            case += 1;
        }
    }
    main.push_str("}\n");
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            "[package]\nname = \"uorust-embed-harness\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n"
                .as_bytes()
                .to_vec(),
        ),
        ("src/main.rs".to_string(), main.into_bytes()),
    ])
}

/// The steps a run of a sample takes at most: each statement and
/// terminator of its closure, once per bit of its widest input and once
/// more, since core's loops over an integer iterate at most once per bit.
fn fuel(program: &Program, s: &Sample) -> usize {
    let steps: usize = s
        .closure
        .iter()
        .filter_map(|k| program.every.get(k))
        .map(|(_, e)| {
            e.term.matches("(record Mir::Block ").count()
                + e.term.matches("(ctor Mir::Statement.").count()
        })
        .sum();
    let bits = s
        .params
        .iter()
        .map(|(_, sc)| match sc {
            Sampled::Int(b, _) => *b as usize,
            Sampled::Bool => 1,
        })
        .max()
        .unwrap_or(1);
    steps * (bits + 1)
}

/// The module `Differential` and its index from the harness's output.
fn differential(
    program: &Program,
    file: &Path,
    observed: &str,
) -> Result<(String, String), UorustError> {
    let bad = |reason: String| UorustError::Malformed {
        path: file.to_path_buf(),
        reason,
    };
    let mut lines = observed.lines();
    let checks: Vec<&str> = lines
        .next()
        .and_then(|l| l.strip_prefix("checks "))
        .ok_or_else(|| bad("no checks line".into()))?
        .split(' ')
        .collect();
    let [debug, overflow] = checks[..] else {
        return Err(bad("a checks line of other than two flags".into()));
    };
    let results: BTreeMap<usize, &str> = lines
        .filter_map(|l| {
            let (n, r) = l.split_once(' ')?;
            Some((n.parse().ok()?, r))
        })
        .collect();
    let mut decls = Vec::new();
    let mut index = String::from(
        "# The differential check (written by `uorust embed --observed`; do not edit):\n\
         # each theorem of Differential, the body it runs, its inputs and what\n\
         # rustc's build returned.\n",
    );
    let mut case = 0usize;
    let mut defined = BTreeSet::new();
    for s in &program.samples {
        let cases = s.cases();
        let first = case;
        case += cases.len();
        let name = def_name(&s.kappa);
        let mut theorems = Vec::new();
        for (i, inputs) in cases.iter().enumerate() {
            let Some(result) = results.get(&(first + i)) else {
                return Err(bad(format!("no result for case {}", first + i)));
            };
            if *result == "panic" {
                continue;
            }
            let expected = match s.result.1 {
                Sampled::Bool => s.result.1.value(u128::from(*result == "true")),
                Sampled::Int(..) => s.result.1.value(
                    result
                        .parse()
                        .map_err(|_| bad(format!("case {}: {result}", first + i)))?,
                ),
            };
            let args: Vec<String> = s
                .params
                .iter()
                .zip(inputs)
                .map(|((_, sc), x)| sc.value(*x))
                .collect();
            let theorem = format!("t{name}x{i}");
            let _ = write!(
                index,
                "\n[[case]]\ntheorem = \"{theorem}\"\nbody = {}\nfunction = \"{}\"\ninputs = [{}]\nreturned = \"{result}\"\n",
                census_string(&s.key),
                s.function,
                inputs.iter().map(u128::to_string).collect::<Vec<_>>().join(", ")
            );
            theorems.push(format!(
                "theorem{{\n  name: {theorem},\n  parameters: [],\n  statement: ((Mir::evaluate {} (p{name}) {} {}) = {}),\n  proof: reflexivity{{}}\n}}",
                fuel(program, s),
                mir_string(&s.key),
                term_list(&args, &mir_name("Value")),
                mir_ctor("Outcome.returned", &[expected])
            ));
        }
        if theorems.is_empty() {
            continue;
        }
        let mut bodies = Vec::new();
        for k in &s.closure {
            let Some((kappa, e)) = program.every.get(k) else {
                continue;
            };
            let c = format!("c{}", def_name(kappa));
            if defined.insert(c.clone()) {
                decls.push(format!(
                    "definition{{\n  name: {c},\n  parameters: [],\n  result: {},\n  body: {}\n}}",
                    mir_name("Body"),
                    e.term
                ));
            }
            bodies.push(format!("({c})"));
        }
        // Contracts are checked only under `-Zcontract-checks`, which no
        // build of core here passes.
        decls.push(format!(
            "definition{{\n  name: p{name},\n  parameters: [],\n  result: {},\n  body: (record {} {{bodies := {}, dictionary := (nil {}), ubChecks := {debug}, contractChecks := false, overflowChecks := {overflow}}})\n}}",
            mir_name("Program"),
            mir_name("Program"),
            term_list(&bodies, &mir_name("Body")),
            mir_name("Binding")
        ));
        decls.extend(theorems);
    }
    let module = format!(
        "\\begin{{lexlean}}{{Differential}}\n\\useglossary{{lexlean.std.bool@1.2.0}}\n\
         \\useglossary{{lexlean.std.nat@1.2.0}}\n\\useglossary{{uorust.mir@1.0.0}}\n\
         \\importmodule{{{MIR}}}\n\\title{{Embedded MIR checks}}\n\\begin{{semanticmodule}}\n{}\n\
         \\end{{semanticmodule}}\n\\end{{lexlean}}\n",
        decls.join("\n\n")
    );
    Ok((module, index))
}

/// A body's κ, as the core register keys it: its crate path and its text,
/// with the build's allocation numbers renumbered.
pub(crate) fn body_kappa(krate: &str, path: &str, text: &str) -> String {
    crate::project::sha(
        format!(
            "{krate}::{path}\n{}",
            crate::mir::core::numbered_allocations(text)
        )
        .as_bytes(),
    )
}

/// Whether a body has a row of its own in the register: closures,
/// promoted constants and anonymous constants are parts of the bodies
/// that use them.
pub(crate) fn has_row(path: &str) -> bool {
    !(path.contains("{closure") || path.contains("promoted[") || path.contains("{constant"))
}

/// The type MIR prints of a place that is a local, unprojected.
fn place_type(body: &IrBody, p: &IrPlace) -> Option<String> {
    p.projection
        .is_empty()
        .then(|| body.locals.get(p.local).cloned())
        .flatten()
}

/// The type MIR prints of an argument, where it does: a local's, or a
/// literal's suffix.
fn arg_type(body: &IrBody, o: &IrOperand) -> Option<String> {
    match o {
        IrOperand::Copy(p) | IrOperand::Move(p) => place_type(body, p),
        IrOperand::Constant(IrConstant::Int(_, suffix)) => Some(suffix.clone()),
        IrOperand::Constant(IrConstant::Bool(_)) => Some("bool".into()),
        IrOperand::Constant(IrConstant::Char(_)) => Some("char".into()),
        _ => None,
    }
}

/// The name a body has in the program: its path, or, where a macro's
/// impls share it, its path and its definition's name.
pub(crate) fn program_key(link: &crate::mir::link::CoreLink, path: &str, kappa: &str) -> String {
    if link.bodies.get(path).is_some_and(|b| b.len() > 1) {
        format!("{path}#{}", def_name(kappa))
    } else {
        path.to_string()
    }
}

/// How the linker resolves a printed callee with arguments and a
/// destination of the types MIR prints: the one body of `krate` it names
/// (of a macro's impls, the one whose signature the call's types
/// instantiate), an intrinsic, or, where it names none, a name for the
/// dictionary: a trait's method at an abstract type.
pub(crate) fn resolve_callee(
    link: &crate::mir::link::CoreLink,
    krate: &str,
    callee: &str,
    args: &[Option<String>],
    dest: &str,
) -> Callee {
    let erased = crate::mir::lower::erase(callee);
    let path = link.path(&erased);
    if let Some(p) = path.as_ref().filter(|p| link.decls.intrinsics.contains(*p)) {
        return Callee::Intrinsic(p.clone());
    }
    if let Some(p) = path.as_ref() {
        if let Some([_]) = link.bodies.get(p).map(Vec::as_slice) {
            return Callee::Body(p.clone());
        }
    }
    let Some(instance) = link.instance(callee, args, dest) else {
        return Callee::Bound(callee.to_string());
    };
    let Some(texts) = link.bodies.get(&instance.path) else {
        return Callee::Bound(callee.to_string());
    };
    if let [_] = texts.as_slice() {
        return Callee::Body(instance.path);
    }
    // Of a macro's impls, the one whose signature, at the instance's
    // bindings, is the instance's.
    let generics: BTreeMap<String, String> = instance
        .bound
        .iter()
        .filter(|(k, _)| k != "Self")
        .cloned()
        .collect();
    let head = |t: &str| t.lines().find(|l| l.starts_with("fn ")).map(str::to_string);
    let want = head(&instance.text);
    let mut hits = texts
        .iter()
        .filter(|t| head(t).map(|l| crate::mir::stdlib::substitute(&l, &generics)) == want);
    match (hits.next(), hits.next()) {
        (Some(t), None) => Callee::Body(program_key(
            link,
            &instance.path,
            &body_kappa(krate, &instance.path, t),
        )),
        _ => Callee::Bound(callee.to_string()),
    }
}

/// The pointer width the dumps under `dumps` were built for, from the
/// `target.toml` beside them.
fn dump_pointer_width(dumps: &Path) -> Result<u32, UorustError> {
    let file = dumps.join("target.toml");
    let bad = |reason: String| UorustError::Malformed {
        path: file.clone(),
        reason,
    };
    let text = std::fs::read_to_string(&file).map_err(|e| bad(e.to_string()))?;
    let doc: toml::Table = text.parse().map_err(|e| bad(format!("{e}")))?;
    doc.get("pointer_width")
        .and_then(toml::Value::as_integer)
        .and_then(|w| u32::try_from(w).ok())
        .ok_or_else(|| bad("no pointer_width".into()))
}

/// Every module's bodies, embedded, by module and κ, with the census of
/// the modules `only` names (every module when empty). Every module's
/// dumps are linked, so a call into another module names its body.
#[allow(clippy::type_complexity)]
fn embed_census(
    root: &Path,
    dumps: &Path,
    only: &[String],
) -> Result<(Vec<(String, Vec<(String, Embedded)>)>, EmbedCensus, Program), UorustError> {
    let io = |p: &Path, e: std::io::Error| UorustError::Tool(format!("{}: {e}", p.display()));
    let width = dump_pointer_width(dumps)?;
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(dumps)
        .map_err(|e| io(dumps, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut read: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for dir in dirs {
        let module = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| io(&dir, e))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        let mut texts = Vec::new();
        for file in &files {
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            texts.push((
                name,
                std::fs::read_to_string(file).map_err(|e| io(file, e))?,
            ));
        }
        read.push((module, texts));
    }
    let link = crate::mir::link::CoreLink::new(
        crate::mir::core::core_decls(root),
        root,
        read.iter()
            .flat_map(|(_, texts)| texts.iter().map(|(n, t)| (n.as_str(), t))),
    );
    let enums = enums(&link.decls);
    let resolve = |callee: &str, args: &[Option<String>], dest: &str| {
        resolve_callee(&link, "core", callee, args, dest)
    };
    let cx = Cx {
        width,
        enums: &enums,
        resolve: &resolve,
    };
    let mut census = EmbedCensus::default();
    let mut modules = Vec::new();
    let mut every: BTreeMap<String, (String, Embedded)> = BTreeMap::new();
    let mut samples = Vec::new();
    let num = library_root(root).join("core").join("src").join("num");
    for (module, texts) in &read {
        let selected = only.is_empty() || only.contains(module);
        let mut bodies = Vec::new();
        for (_, text) in texts {
            for b in crate::mir::parse::bodies(text) {
                let kappa = body_kappa("core", &b.path, text);
                let key = program_key(&link, &b.path, &kappa);
                let e = cx.body(&b, &key);
                if selected && has_row(&b.path) {
                    census.add(&e);
                    if let Some(sample) = sample(root, &num, &b, &key, &kappa, width) {
                        samples.push(sample);
                    }
                }
                every.insert(key, (kappa.clone(), e.clone()));
                if selected {
                    bodies.push((kappa, e));
                }
            }
        }
        if !selected {
            continue;
        }
        // Two bodies of one κ (a macro's impls of one text) are one
        // definition.
        bodies.sort_by(|a, b| a.0.cmp(&b.0));
        bodies.dedup_by(|a, b| a.0 == b.0);
        modules.push((module.clone(), bodies));
    }
    // A sample is checked where the evaluator defines its whole closure:
    // every body it reaches is embedded, uses no construct the evaluator
    // does not define, calls no intrinsic and leaves no callee to a
    // dictionary.
    samples.retain_mut(|s| match closure(&every, &s.key) {
        Some(c) => {
            s.closure = c;
            true
        }
        None => false,
    });
    let program = Program { every, samples };
    Ok((modules, census, program))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constructors of the inductive `name` in the semantics.
    fn constructors(name: &str) -> Vec<String> {
        let head = format!("inductive{{\n  name: {name},");
        let at = SEMANTICS.find(&head).expect("the inductive");
        let rest = &SEMANTICS[at..];
        let end = rest.find("\n}\n").expect("its end");
        rest[..end]
            .match_indices("{name: ")
            .map(|(i, _)| {
                let r = &rest[i + "{name: ".len()..];
                r[..r.find(',').expect("a name")].to_string()
            })
            .collect()
    }

    #[test]
    fn the_datatype_kinds_are_rustcs_kinds_ue_19() {
        let check = |name: &str, paths: Vec<&str>| {
            let want: Vec<String> = paths
                .iter()
                .map(|p| kind_ctor(p).split_once('.').expect("a kind").1.to_string())
                .collect();
            assert_eq!(constructors(name), want, "{name}");
        };
        check(
            "BinOp",
            spec::MirBinOp::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "UnOp",
            spec::MirUnOp::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "CastKind",
            spec::MirCastKind::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "BorrowKind",
            spec::MirBorrowKind::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "RuntimeChecks",
            spec::MirRuntimeChecks::ALL
                .iter()
                .map(|k| k.path())
                .collect(),
        );
        check(
            "AssertKind",
            spec::MirAssertKind::ALL.iter().map(|k| k.path()).collect(),
        );
    }

    #[test]
    fn the_undefined_constructs_are_read_from_the_evaluator_ue_19() {
        let undefined = undefined_constructs();
        assert!(undefined.contains("BinOp::Offset"));
        assert!(undefined.contains("CastKind::Transmute"));
        assert!(!undefined.contains("BinOp::Add"));
    }

    #[test]
    fn a_long_list_nests_no_deeper_than_its_root_ue_19() {
        let items: Vec<String> = (0..10_000).map(|i| i.to_string()).collect();
        let term = term_list(&items, "Nat");
        let (mut depth, mut deepest) = (0usize, 0usize);
        for c in term.chars() {
            match c {
                '(' => {
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                ')' => depth -= 1,
                _ => {}
            }
        }
        assert!(deepest <= 2 * (items.len().isqrt() + 1), "{deepest}");
        assert_eq!(term.matches("(cons ").count(), items.len());
    }

    #[test]
    fn a_harness_output_without_its_checks_is_refused_ue_19() {
        let program = Program {
            every: BTreeMap::new(),
            samples: Vec::new(),
        };
        let file = Path::new("observed.txt");
        assert!(matches!(
            differential(&program, file, "0 1\n"),
            Err(UorustError::Malformed { .. })
        ));
        assert!(differential(&program, file, "checks false true\n").is_ok());
    }

    #[test]
    fn a_string_keeps_every_character_ue_19() {
        assert_eq!(mir_string("a%b"), "\"a\\u0025b\"");
        assert_eq!(mir_string("q\"\\\n"), "\"q\\\"\\\\\\u000a\"");
    }

    #[test]
    fn an_integer_constant_is_its_twos_complement_ue_19() {
        assert_eq!(repr(-1, 8), 255);
        assert_eq!(repr(5, 32), 5);
        assert_eq!(repr(-1, 128), u128::MAX);
        assert_eq!(int_ty("usize", 64), Some((64, false)));
        assert_eq!(int_ty("i16", 64), Some((16, true)));
        assert_eq!(int_ty("u", 64), None);
    }
}
