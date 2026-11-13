//! Derived orders: the comparison `#[derive(PartialOrd, Ord)]` defines, as
//! LexLean definitions.
//!
//! A derived order is lexicographic: two values of an enum compare by the
//! order their constructors are declared in, then field by field; a struct
//! compares field by field; `Option` puts `None` first, `Result` puts `Ok`
//! first, and a sequence compares element by element with a prefix first.
//! `BTreeSet` and `BTreeMap` compare their members or entries in order,
//! which the lift holds as a sorted list, so they compare as that list.
//! Each comparison is a definition `compare_T` to `Ordering`, the inductive
//! of `std::cmp::Ordering`; the definitions of a recursive family are one
//! mutual group, structural on their left argument. A unit-variant enum's
//! order laws (reflexive, antisymmetric, transitive) are theorems proved by
//! cases on its finitely many values.

use std::collections::{BTreeMap, BTreeSet};

use crate::lex::{Arm, Decl, Expr};
use crate::lift::{Ty, Types};

/// The inductive of `std::cmp::Ordering`, apart from Lean's own
/// `Ordering`, and its constructors in order.
pub(crate) const ORDERING: &str = "CmpOrdering";
/// The Rust name of `std::cmp::Ordering`.
pub(crate) const RUST_ORDERING: &str = "Ordering";
pub(crate) const ORDERINGS: [&str; 3] = ["Less", "Equal", "Greater"];

/// The value of one constructor of `Ordering`.
pub(crate) fn ordering_value(v: &str) -> String {
    format!("(ctor {ORDERING}.{v})")
}

/// The type a comparison ranges over: widths and characters read like
/// their LexLean type, and an ordered set or map like its sorted list.
pub(crate) fn normal(ty: &Ty) -> Ty {
    match ty {
        Ty::Nat(_) => Ty::Nat(None),
        Ty::Char => Ty::Str,
        Ty::Set(t, _) => Ty::List(Box::new(normal(t))),
        Ty::Map(k, v, _) => Ty::List(Box::new(Ty::Tuple(vec![normal(k), normal(v)]))),
        Ty::Option(t) => Ty::Option(Box::new(normal(t))),
        Ty::List(t) => Ty::List(Box::new(normal(t))),
        Ty::Outcome(t, e) => Ty::Outcome(Box::new(normal(t)), Box::new(normal(e))),
        Ty::Tuple(ts) => Ty::Tuple(ts.iter().map(normal).collect()),
        Ty::Pair(ts) => Ty::Pair(ts.iter().map(normal).collect()),
        other => other.clone(),
    }
}

/// The definition comparing values of `ty`.
pub(crate) fn name(ty: &Ty) -> String {
    format!("compare_{}", normal(ty).tag())
}

/// A comparison of `left` and `right`, of type `ty`.
pub(crate) fn call(ty: &Ty, left: &str, right: &str) -> String {
    format!("({} {left} {right})", name(ty))
}

/// Whether the projection declares `Ordering` as `std::cmp::Ordering`.
pub(crate) fn has_ordering(types: &Types) -> bool {
    types
        .enums
        .get(ORDERING)
        .is_some_and(|vs| vs.iter().map(String::as_str).eq(ORDERINGS))
}

/// The derives under which a type's equality and order are structural.
const DERIVED: [&str; 4] = ["PartialEq", "Eq", "PartialOrd", "Ord"];

/// Whether `ty` has a derived order the lift writes: every named type it
/// reaches derives equality and order, and a recursive family among
/// them has a structural comparison.
pub(crate) fn derived(ty: &Ty, types: &Types) -> bool {
    has_ordering(types) && derived_in(ty, types, &BTreeSet::new())
}

/// [`derived`], with the named types already assumed.
fn derived_in(ty: &Ty, types: &Types, assumed: &BTreeSet<String>) -> bool {
    match ty {
        Ty::Unit | Ty::Bool | Ty::Nat(_) | Ty::Str | Ty::Char => true,
        // A float's order is partial, and a signed bit pattern orders as
        // its value, not as the natural: no derived total order holds.
        Ty::Int(_) | Ty::Float(_) | Ty::Signed(_) | Ty::ParseError | Ty::Path | Ty::Fn(..) => false,
        Ty::Option(t) | Ty::List(t) | Ty::Set(t, true) => derived_in(t, types, assumed),
        Ty::Set(_, false) | Ty::Map(_, _, false) => false,
        Ty::Map(k, v, true) | Ty::Outcome(k, v) => {
            derived_in(k, types, assumed) && derived_in(v, types, assumed)
        }
        Ty::Tuple(ts) | Ty::Pair(ts) => ts.iter().all(|t| derived_in(t, types, assumed)),
        Ty::Named(n) if assumed.contains(n) => true,
        Ty::Named(n) => {
            let derives = types.derives.get(n);
            let ord = derives.is_some_and(|d| DERIVED.iter().all(|t| d.contains(*t)));
            let family = types
                .family_of(n)
                .is_none_or(|f| !types.nested(n) || types.sizable(f));
            let mut inner = assumed.clone();
            inner.insert(n.clone());
            ord && family
                && types
                    .member_fields(n)
                    .iter()
                    .all(|t| derived_in(t, types, &inner))
        }
    }
}

/// Whether a comparison of `ty` reads strings, whose order the runtime
/// decides with these axioms.
pub(crate) fn axioms(ty: &Ty, types: &Types) -> BTreeSet<String> {
    if reads_text(ty, types, &BTreeSet::new()) {
        ["Classical.choice", "Quot.sound", "propext"]
            .iter()
            .map(|a| a.to_string())
            .collect()
    } else {
        BTreeSet::new()
    }
}

fn reads_text(ty: &Ty, types: &Types, assumed: &BTreeSet<String>) -> bool {
    match ty {
        Ty::Str | Ty::Char => true,
        Ty::Option(t) | Ty::List(t) | Ty::Set(t, _) => reads_text(t, types, assumed),
        Ty::Map(k, v, _) | Ty::Outcome(k, v) => {
            reads_text(k, types, assumed) || reads_text(v, types, assumed)
        }
        Ty::Tuple(ts) | Ty::Pair(ts) => ts.iter().any(|t| reads_text(t, types, assumed)),
        Ty::Named(n) if !assumed.contains(n) => {
            let mut inner = assumed.clone();
            inner.insert(n.clone());
            types
                .member_fields(n)
                .iter()
                .any(|t| reads_text(t, types, &inner))
        }
        _ => false,
    }
}

/// The comparisons of these types and of every type they compare through,
/// each after the ones it calls, with the order laws of each unit-variant
/// enum among them.
pub(crate) fn decls(wanted: &BTreeSet<Ty>, types: &Types) -> Vec<Decl> {
    let (_, out) = wanted
        .iter()
        .map(normal)
        .fold((BTreeSet::new(), Vec::new()), |state, ty| {
            emit(&ty, types, state)
        });
    out
}

/// The emitted comparisons, by name, and the declarations so far.
type Emitted = (BTreeSet<String>, Vec<Decl>);

/// The types a comparison of `ty` calls the comparison of.
fn parts(ty: &Ty, types: &Types) -> Vec<Ty> {
    match ty {
        Ty::Option(t) => vec![(**t).clone()],
        Ty::List(t) => vec![(**t).clone(), ty.clone()],
        Ty::Outcome(t, e) => vec![(**t).clone(), (**e).clone()],
        Ty::Tuple(ts) | Ty::Pair(ts) => ts.clone(),
        Ty::Named(n) => types.member_fields(n),
        _ => Vec::new(),
    }
    .iter()
    .map(normal)
    .collect()
}

/// The comparisons a recursive family's group defines: its members, and the
/// lists and options of members they hold.
fn group(family: &BTreeSet<String>, types: &Types) -> Vec<Ty> {
    let members = family.iter().map(|m| Ty::Named(m.clone()));
    let held = family
        .iter()
        .flat_map(|m| types.member_fields(m))
        .map(|t| normal(&t))
        .filter(|t| !matches!(t, Ty::Named(_)) && types.size_name(t, family).is_some());
    members.chain(held).fold(Vec::new(), |mut acc, t| {
        if !acc.contains(&t) {
            acc.push(t);
        }
        acc
    })
}

fn emit(ty: &Ty, types: &Types, state: Emitted) -> Emitted {
    if state.0.contains(&name(ty)) {
        return state;
    }
    let family = match ty {
        Ty::Named(n) => types.family_of(n).cloned(),
        Ty::List(t) | Ty::Option(t) => match &**t {
            Ty::Named(n) => types
                .family_of(n)
                .filter(|f| types.size_name(ty, f).is_some())
                .filter(|f| group(f, types).contains(ty))
                .cloned(),
            _ => None,
        },
        _ => None,
    };
    let members = match &family {
        Some(f) => group(f, types),
        None => vec![ty.clone()],
    };
    let (mut done, decls) = state;
    for m in &members {
        done.insert(name(m));
    }
    // The comparisons the members call, outside the group, first.
    let (done, mut decls) = members
        .iter()
        .flat_map(|m| parts(m, types))
        .filter(|p| !members.contains(p))
        .fold((done, decls), |state, p| emit(&p, types, state));
    let label = (members.len() > 1).then(|| format!("CompareOf{}", members[0].tag()));
    let recursive = family.is_some()
        || members
            .iter()
            .any(|m| matches!(m, Ty::List(_)) || parts(m, types).contains(m));
    for m in &members {
        decls.push(Decl::Definition {
            name: name(m),
            parameters: vec![("left".into(), m.render()), ("right".into(), m.render())],
            result: ORDERING.into(),
            body: body(m, types),
            axioms: axioms(m, types).into_iter().collect(),
            recursive: recursive.then(|| "left".to_string()),
            termination: None,
            mutual: label.clone(),
        });
        if let Ty::Named(n) = m {
            if let Some(variants) = types.enums.get(n) {
                decls.extend(laws(m, variants));
            }
        }
    }
    (done, decls)
}

/// `Ordering` from a strict test and an equality test.
fn by_tests(less: &str, same: &str) -> Expr {
    Expr::Flat(format!(
        "(if {less} then {} else (if {same} then {} else {}))",
        ordering_value("Less"),
        ordering_value("Equal"),
        ordering_value("Greater")
    ))
}

/// The lexicographic comparison of these fields, in order.
fn lexicographic(fields: &[(Ty, String, String)]) -> Expr {
    match fields.split_first() {
        None => Expr::Flat(ordering_value("Equal")),
        Some(((t, l, r), rest)) => Expr::Match {
            scrutinee: call(t, l, r),
            arms: vec![
                Arm {
                    pattern: format!("{ORDERING}.Less"),
                    body: Expr::Flat(ordering_value("Less")),
                },
                Arm {
                    pattern: format!("{ORDERING}.Equal"),
                    body: lexicographic(rest),
                },
                Arm {
                    pattern: format!("{ORDERING}.Greater"),
                    body: Expr::Flat(ordering_value("Greater")),
                },
            ],
        },
    }
}

/// A constructor of an inductive: its pattern head and its field types.
type Ctor = (String, Vec<Ty>);

/// The comparison of two values of constructors listed in order: the same
/// constructor compares its fields, an earlier one is less.
fn by_constructors(ctors: &[Ctor]) -> Expr {
    let binders = |prefix: &str, n: usize| -> Vec<String> {
        (0..n).map(|i| format!("{prefix}{i}")).collect()
    };
    let pattern = |head: &str, names: &[String]| -> String {
        names
            .iter()
            .fold(head.to_string(), |acc, n| format!("{acc} {n}"))
    };
    Expr::Match {
        scrutinee: "left".into(),
        arms: ctors
            .iter()
            .enumerate()
            .map(|(i, (head, fields))| {
                let xs = binders("x", fields.len());
                Arm {
                    pattern: pattern(head, &xs),
                    body: Expr::Match {
                        scrutinee: "right".into(),
                        arms: ctors
                            .iter()
                            .enumerate()
                            .map(|(j, (other, others))| {
                                let ys = binders("y", others.len());
                                let body = match i.cmp(&j) {
                                    std::cmp::Ordering::Less => Expr::Flat(ordering_value("Less")),
                                    std::cmp::Ordering::Greater => {
                                        Expr::Flat(ordering_value("Greater"))
                                    }
                                    std::cmp::Ordering::Equal => {
                                        let triples: Vec<(Ty, String, String)> = fields
                                            .iter()
                                            .zip(xs.iter().zip(&ys))
                                            .map(|(t, (x, y))| (t.clone(), x.clone(), y.clone()))
                                            .collect();
                                        lexicographic(&triples)
                                    }
                                };
                                Arm {
                                    pattern: pattern(other, &ys),
                                    body,
                                }
                            })
                            .collect(),
                    },
                }
            })
            .collect(),
    }
}

/// The body of the comparison of `ty`, over `left` and `right`.
fn body(ty: &Ty, types: &Types) -> Expr {
    let item = |t: &Ty| normal(t);
    match ty {
        Ty::Unit => Expr::Flat(ordering_value("Equal")),
        Ty::Bool => Expr::Flat(format!(
            "(if left then (if right then {} else {}) else (if right then {} else {}))",
            ordering_value("Equal"),
            ordering_value("Greater"),
            ordering_value("Less"),
            ordering_value("Equal")
        )),
        Ty::Nat(_) => by_tests("(left <? right)", "(left =? right)"),
        Ty::Str | Ty::Char => by_tests(
            "(prim less_than left right : Bool)",
            "(prim equal left right : Bool)",
        ),
        Ty::Option(t) => by_constructors(&[
            ("Option.none".into(), Vec::new()),
            ("Option.some".into(), vec![item(t)]),
        ]),
        Ty::Outcome(t, e) => by_constructors(&[
            ("Result.ok".into(), vec![item(t)]),
            ("Result.error".into(), vec![item(e)]),
        ]),
        Ty::List(t) => by_constructors(&[
            ("List.nil".into(), Vec::new()),
            ("List.cons".into(), vec![item(t), ty.clone()]),
        ]),
        Ty::Tuple(ts) | Ty::Pair(ts) => {
            let triples: Vec<(Ty, String, String)> = ts
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    (
                        item(t),
                        crate::lift::tuple_item(ty, "left", i),
                        crate::lift::tuple_item(ty, "right", i),
                    )
                })
                .collect();
            lexicographic(&triples)
        }
        Ty::Named(n) => match (types.enums.get(n), types.datas.get(n), types.structs.get(n)) {
            (Some(variants), _, _) => by_constructors(
                &variants
                    .iter()
                    .map(|v| (format!("{n}.{v}"), Vec::new()))
                    .collect::<Vec<_>>(),
            ),
            (None, Some(variants), _) => by_constructors(
                &variants
                    .iter()
                    .map(|(v, fs)| {
                        (
                            format!("{n}.{v}"),
                            fs.iter().map(|(_, t)| item(t)).collect(),
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
            (None, None, Some(fields)) if types.boxed(n) => by_constructors(&[(
                format!("{n}.mk"),
                fields.iter().map(|(_, t)| item(t)).collect(),
            )]),
            (None, None, Some(fields)) => {
                let triples: Vec<(Ty, String, String)> = fields
                    .iter()
                    .map(|(f, t)| {
                        (
                            item(t),
                            format!("(project left {f})"),
                            format!("(project right {f})"),
                        )
                    })
                    .collect();
                lexicographic(&triples)
            }
            (None, None, None) => Expr::Flat(ordering_value("Equal")),
        },
        Ty::Int(_)
        | Ty::Float(_)
        | Ty::Signed(_)
        | Ty::ParseError
        | Ty::Path
        | Ty::Set(..)
        | Ty::Map(..)
        | Ty::Fn(..) => Expr::Flat(ordering_value("Equal")),
    }
}

/// A test of an `Ordering`: whether it is the constructor `v`.
pub(crate) fn is(v: &str) -> String {
    format!("ordering_is_{v}")
}

/// The definitions of the tests [`is`] names.
pub(crate) fn tests() -> Vec<Decl> {
    ORDERINGS
        .iter()
        .map(|v| Decl::Definition {
            name: is(v),
            parameters: vec![("value".into(), ORDERING.into())],
            result: "Bool".into(),
            body: Expr::Match {
                scrutinee: "value".into(),
                arms: ORDERINGS
                    .iter()
                    .map(|w| Arm {
                        pattern: format!("{ORDERING}.{w}"),
                        body: Expr::Flat((v == w).to_string()),
                    })
                    .collect(),
            },
            axioms: Vec::new(),
            recursive: None,
            termination: None,
            mutual: None,
        })
        .collect()
}

/// The test of the comparison of `left` and `right` an operator makes:
/// `==`, `!=`, `<`, `<=`, `>` or `>=`.
pub(crate) fn operator(op: &str, ty: &Ty, left: &str, right: &str) -> Option<String> {
    let c = call(ty, left, right);
    let test = |v: &str| format!("({} {c})", is(v));
    Some(match op {
        "==" => test("Equal"),
        "!=" => format!("(not {})", test("Equal")),
        "<" => test("Less"),
        ">" => test("Greater"),
        "<=" => format!("(not {})", test("Greater")),
        ">=" => format!("(not {})", test("Less")),
        _ => return None,
    })
}

/// The reversal of an `Ordering`: `Ordering::reverse`.
pub(crate) const REVERSE: &str = "reverse_Ordering";

/// The definition of [`REVERSE`].
pub(crate) fn reverse() -> Decl {
    let flip = BTreeMap::from([("Less", "Greater"), ("Equal", "Equal"), ("Greater", "Less")]);
    Decl::Definition {
        name: REVERSE.into(),
        parameters: vec![("value".into(), ORDERING.into())],
        result: ORDERING.into(),
        body: Expr::Match {
            scrutinee: "value".into(),
            arms: ORDERINGS
                .iter()
                .map(|v| Arm {
                    pattern: format!("{ORDERING}.{v}"),
                    body: Expr::Flat(ordering_value(flip[v])),
                })
                .collect(),
        },
        axioms: Vec::new(),
        recursive: None,
        termination: None,
        mutual: None,
    }
}

/// The width LexLean's canonical layout breaks a line past.
const WIDTH: usize = 100;

/// The column a theorem's proof starts at: after `  proof: `.
const PROOF_COLUMN: usize = 9;

/// A proof by cases on each of `vars` in turn, over these constructors,
/// that closes every case with `leaf`, laid out as `lexlean fmt` lays it
/// out: from column `col` of a line indented `indent`, with `trail`
/// characters after it.
fn by_cases(vars: &[&str], variants: &[String], leaf: &str) -> String {
    laid(vars, variants, leaf, 2, PROOF_COLUMN, 0)
}

/// [`by_cases`] on one line.
fn flat_cases(vars: &[&str], variants: &[String], leaf: &str) -> String {
    match vars.split_first() {
        None => leaf.to_string(),
        Some((v, rest)) => {
            let inner = flat_cases(rest, variants, leaf);
            let branches: Vec<String> = variants
                .iter()
                .map(|c| format!("{{proof: {inner}, binders: [], constructor: {c}}}"))
                .collect();
            format!(
                "cases{{branches: [{}], scrutinee: {v}}}",
                branches.join(", ")
            )
        }
    }
}

/// [`by_cases`] from column `col` of a line indented `indent`.
fn laid(
    vars: &[&str],
    variants: &[String],
    leaf: &str,
    indent: usize,
    col: usize,
    trail: usize,
) -> String {
    let flat = flat_cases(vars, variants, leaf);
    let Some((v, rest)) = vars.split_first() else {
        return flat;
    };
    if col + flat.len() + trail <= WIDTH {
        return flat;
    }
    let pad = |n: usize| " ".repeat(n);
    let inner = flat_cases(rest, variants, leaf);
    let items: Vec<String> = variants
        .iter()
        .map(|c| format!("{{proof: {inner}, binders: [], constructor: {c}}}"))
        .collect();
    let one_line = format!("{}branches: [{}],", pad(indent + 2), items.join(", "));
    let branches = if one_line.len() <= WIDTH {
        one_line
    } else {
        let last = variants.len().saturating_sub(1);
        let lines: Vec<String> = variants
            .iter()
            .zip(&items)
            .enumerate()
            .map(|(i, (c, item))| {
                let comma = usize::from(i < last);
                let at = indent + 4;
                let text = if at + item.len() + comma <= WIDTH {
                    item.clone()
                } else {
                    let key = indent + 6;
                    let proof = laid(rest, variants, leaf, key, key + "proof: ".len(), 1);
                    format!(
                        "{{\n{}proof: {proof},\n{}binders: [],\n{}constructor: {c}\n{}}}",
                        pad(key),
                        pad(key),
                        pad(key),
                        pad(at)
                    )
                };
                format!("{}{text}{}", pad(at), if comma == 1 { "," } else { "" })
            })
            .collect();
        format!(
            "{}branches: [\n{}\n{}],",
            pad(indent + 2),
            lines.join("\n"),
            pad(indent + 2)
        )
    };
    format!(
        "cases{{\n{branches}\n{}scrutinee: {v}\n{}}}",
        pad(indent + 2),
        pad(indent)
    )
}

/// The order laws of a unit-variant enum's comparison, each by cases on
/// its values: it is reflexive, antisymmetric (comparing the other way
/// reverses it), and transitive, stated over the Boolean test of `Less`
/// since an `Ordering` has no decidable equality the kernel can use.
fn laws(ty: &Ty, variants: &[String]) -> Vec<Decl> {
    let compare = name(ty);
    let t = ty.render();
    let less = is("Less");
    vec![
        Decl::Theorem {
            name: format!("{compare}_reflexive"),
            parameters: vec![("a".into(), t.clone())],
            statement: format!("(({compare} a a) = {})", ordering_value("Equal")),
            proof: by_cases(&["a"], variants, "reflexivity{}"),
            axioms: Vec::new(),
        },
        Decl::Theorem {
            name: format!("{compare}_antisymmetric"),
            parameters: vec![("a".into(), t.clone()), ("b".into(), t.clone())],
            statement: format!("(({compare} a b) = ({REVERSE} ({compare} b a)))"),
            proof: by_cases(&["a", "b"], variants, "reflexivity{}"),
            axioms: Vec::new(),
        },
        Decl::Theorem {
            name: format!("{compare}_transitive"),
            parameters: vec![
                ("a".into(), t.clone()),
                ("b".into(), t.clone()),
                ("c".into(), t),
            ],
            statement: format!(
                "(((not (({less} ({compare} a b)) && ({less} ({compare} b c)))) || ({less} ({compare} a c))) = true)"
            ),
            proof: by_cases(&["a", "b", "c"], variants, "reflexivity{}"),
            axioms: Vec::new(),
        },
    ]
}
