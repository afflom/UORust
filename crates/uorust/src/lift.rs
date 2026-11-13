//! Function bodies to LexLean definitions, where the lift is mechanical.
//!
//! The lift covers the total, pure fragment whose meaning does not depend on
//! the machine: `bool` logic; unsigned integers with comparisons, division and
//! remainder by a non-zero literal, and the `checked_*`, `wrapping_*` and
//! `saturating_*` operations whose overflow behaviour the method names; `i8` to
//! `i64` with comparisons and, for `i64`, the checked operations; strings
//! compared for equality; unit-variant enums; structs and tuples of liftable
//! types; `Option` and `Result` of liftable types, with `?` on a `let`; slices
//! and `Vec` with their length, indexing and structural recursion over a slice
//! pattern; `let`, `if`, `if let`, `match` and calls among lifted functions.
//! Anything else (`+`, `-` and `*` without a named overflow behaviour,
//! references to mutable state, loops, closures, other methods) is not guessed
//! at: the function becomes a residue row with a typed reason.

use std::collections::{BTreeMap, BTreeSet};

/// A lift step: the value, or the residue that stops the lift.
pub(crate) type Lift<T> = Result<T, Residue>;

use crate::ast as rs;
use crate::lex::{camel, Arm, Decl, Expr};
// Imported by name: the lift reads a type two modules name `Refusal` by
// its file's import, never by a path written out.
use crate::mir::layout::Refusal;
use crate::order::ORDERING;

/// Why a function stays in the frontier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Residue {
    /// The function is generic or async, or takes `self`.
    Signature(String),
    /// A parameter or the result has a type outside the lift.
    Type(String),
    /// The body has statements other than `let` before its value.
    Statements,
    /// Integer arithmetic whose overflow behaviour is a build setting.
    Arithmetic(String),
    /// A `usize` operator whose value leaves the width of these targets,
    /// by pointer width: the operator, and the targets of each width.
    Overflow(String, Vec<(u32, Vec<String>)>),
    /// Integer arithmetic whose meaning depends on the width of the type
    /// (`usize`, or a signed type narrower than `i64`).
    Width(String),
    /// An expression form outside the lift.
    Form(String),
    /// A call to a function that is not lifted.
    Call(String),
    /// Another function of the crate lifts to the same LexLean name.
    Collision(String),
    /// A kind of MIR the translator does not lower (`Rvalue::Ref: Mut`), or
    /// an obligation of its translation the verifier did not discharge.
    Mir(String),
    /// A layout the targets' data layouts do not give (see
    /// `mir::layout::Refusal`).
    Layout(Refusal),
}

impl Residue {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Signature(_) => "signature",
            Self::Type(_) => "type",
            Self::Statements => "statements",
            Self::Arithmetic(_) | Self::Overflow(..) => "arithmetic",
            Self::Width(_) => "width",
            Self::Form(_) => "form",
            Self::Call(_) => "call",
            Self::Collision(_) => "collision",
            Self::Mir(_) | Self::Layout(_) => "mir",
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Signature(s) => format!("signature outside the lift: {s}"),
            Self::Type(t) => format!("type outside the lift: {t}"),
            Self::Statements => "the body has statements other than let".to_string(),
            Self::Arithmetic(op) => {
                format!("integer operator {op} wraps or panics depending on the build profile")
            }
            Self::Overflow(op, widths) => {
                let at: Vec<String> = widths
                    .iter()
                    .map(|(bits, targets)| format!("past 2^{bits} on {}", targets.join(" and ")))
                    .collect();
                format!(
                    "usize {op} wraps or panics where its value leaves the pointer width: {}",
                    at.join(", ")
                )
            }
            Self::Width(op) => {
                format!("{op} depends on an integer width LexLean does not fix")
            }
            Self::Form(f) => format!("expression form outside the lift: {f}"),
            Self::Call(c) => format!("calls {c}, which is not lifted"),
            Self::Collision(n) => format!("another function of the crate is also named {n}"),
            Self::Mir(cause) => format!("mir::{cause}"),
            Self::Layout(refusal) => format!("mir::Memory: {}", refusal.text()),
        }
    }
}

/// A Rust type the lift can name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Ty {
    /// `()`: LexLean's `Unit`, whose one value is `()`.
    Unit,
    Bool,
    /// An unsigned integer of this many bits; `None` for `usize`.
    Nat(Option<u32>),
    /// A signed integer of this many bits, held as `Int64`.
    Int(u32),
    /// An IEEE 754 binary floating-point number, held as its bit pattern
    /// (a `Nat`): the format's exponent and fraction widths, as core's own
    /// constants give them (see `mir::float`).
    Float(crate::mir::float::Format),
    /// The MIR path's signed integer of this many bits (`None` for
    /// `isize`), held as its two's-complement bit pattern in `Nat`: LexLean
    /// converts no `Nat` to a signed family, so a cast between the two
    /// closes only where both are `Nat`.
    Signed(Option<u32>),
    Str,
    /// A `char`, held as the one-character string it is.
    Char,
    /// The MIR path's `char`, held as its Unicode scalar value in `Nat`:
    /// at most `0x10FFFF` and outside the surrogates `0xD800..=0xDFFF`, as
    /// core's own `char` methods compute with it.
    Scalar,
    /// The error of an integer `parse`: the prelude's `ParseIntError`.
    ParseError,
    /// A `PathBuf` or `Path`: the prelude's owned path record, its
    /// components and whether it is absolute.
    Path,
    /// A unit-variant enum or a struct.
    Named(String),
    Option(Box<Ty>),
    /// `Result` of a value and an error.
    Outcome(Box<Ty>, Box<Ty>),
    List(Box<Ty>),
    Tuple(Vec<Ty>),
    /// A pair that names a data-carrying enum: Lean's product `(A * B)`,
    /// which an inductive may hold itself in (`Vec<(String, Self)>`), where
    /// a generated tuple structure would make the two mutually recursive.
    Pair(Vec<Ty>),
    /// A map, held as the list of its entries; `true` when its iteration
    /// order is its key order (`BTreeMap`), which `insert` keeps.
    Map(Box<Ty>, Box<Ty>, bool),
    /// A set, held as the list of its members; `true` when its iteration
    /// order is its member order (`BTreeSet`), which `insert` keeps.
    Set(Box<Ty>, bool),
    /// A pure function of its inputs: `&dyn Fn(A) -> R`, `impl Fn(A) -> R`.
    Fn(Vec<Ty>, Box<Ty>),
}

impl Ty {
    /// The LexLean type.
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Unit => "Unit".into(),
            Self::Bool => "Bool".into(),
            Self::Nat(_) | Self::Float(_) | Self::Signed(_) | Self::Scalar => "Nat".into(),
            Self::Int(_) => "Int64".into(),
            Self::Str | Self::Char => "String".into(),
            Self::ParseError => format!("{PRELUDE}::ParseIntError"),
            Self::Path => format!("{PRELUDE}::FilePath"),
            Self::Named(n) => n.clone(),
            Self::Option(t) => format!("(Option {})", t.render()),
            Self::Outcome(t, e) => format!("(Result {} {})", t.render(), e.render()),
            Self::List(t) | Self::Set(t, _) => format!("(List {})", t.render()),
            Self::Tuple(ts) => format!("Tuple{}", ts.iter().map(|t| t.tag()).collect::<String>()),
            Self::Pair(ts) => format!(
                "({})",
                ts.iter()
                    .map(|t| t.render())
                    .collect::<Vec<String>>()
                    .join(" * ")
            ),
            // The list of its entries, each the tuple of a key and a value.
            Self::Map(k, v, _) => format!("(List Tuple{}{})", k.tag(), v.tag()),
            Self::Fn(ins, out) => format!(
                "(Fn ({}) {})",
                ins.iter()
                    .map(Ty::render)
                    .collect::<Vec<String>>()
                    .join(" "),
                out.render()
            ),
        }
    }

    /// A map's entry: the tuple of its key and value.
    fn entry(&self) -> Ty {
        match self {
            Self::Map(k, v, _) => Ty::Tuple(vec![(**k).clone(), (**v).clone()]),
            other => other.clone(),
        }
    }

    pub(crate) fn tag(&self) -> String {
        match self {
            Self::Unit => "Unit".into(),
            Self::Bool => "Bool".into(),
            Self::Nat(_) | Self::Float(_) | Self::Signed(_) | Self::Scalar => "Nat".into(),
            Self::Int(_) => "Int".into(),
            Self::Str | Self::Char => "String".into(),
            Self::ParseError => "ParseIntError".into(),
            Self::Path => "FilePath".into(),
            Self::Named(n) => n.clone(),
            Self::Option(t) => format!("Option{}", t.tag()),
            Self::Outcome(t, e) => format!("Result{}{}", t.tag(), e.tag()),
            Self::List(t) | Self::Set(t, _) => format!("List{}", t.tag()),
            Self::Tuple(ts) => format!("Tuple{}", ts.iter().map(|t| t.tag()).collect::<String>()),
            Self::Pair(ts) => format!("Pair{}", ts.iter().map(|t| t.tag()).collect::<String>()),
            Self::Map(k, v, _) => format!("ListTuple{}{}", k.tag(), v.tag()),
            Self::Fn(ins, out) => format!(
                "Fn{}To{}",
                ins.iter().map(Ty::tag).collect::<String>(),
                out.tag()
            ),
        }
    }

    /// The same LexLean type.
    fn same(&self, other: &Self) -> bool {
        self.render() == other.render()
    }

    /// Every named type and tuple this type mentions, itself included.
    pub(crate) fn mentions(&self) -> BTreeSet<Ty> {
        match self {
            Self::Named(_) => BTreeSet::from([self.clone()]),
            Self::Tuple(ts) => {
                let mut out = BTreeSet::from([self.clone()]);
                for t in ts {
                    out.extend(t.mentions());
                }
                out
            }
            Self::Pair(ts) => {
                let mut out = BTreeSet::new();
                for t in ts {
                    out.extend(t.mentions());
                }
                out
            }
            Self::Option(t) | Self::List(t) | Self::Set(t, _) => t.mentions(),
            // Its entry: the tuple of a key and a value.
            Self::Map(k, v, _) => {
                let mut out = BTreeSet::from([Ty::Tuple(vec![(**k).clone(), (**v).clone()])]);
                out.extend(k.mentions());
                out.extend(v.mentions());
                out
            }
            Self::Outcome(t, e) => {
                let mut out = t.mentions();
                out.extend(e.mentions());
                out
            }
            Self::Fn(ins, result) => {
                let mut out = result.mentions();
                for t in ins {
                    out.extend(t.mentions());
                }
                out
            }
            _ => BTreeSet::new(),
        }
    }
}

/// The names of the enums and structs among the types a type mentions.
fn named_in(types: &BTreeSet<Ty>) -> BTreeSet<String> {
    types
        .iter()
        .filter_map(|t| match t {
            Ty::Named(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

/// A struct value from its fields in declaration order: a record, or the
/// constructor `mk` of a struct of a recursive family.
pub(crate) fn struct_value(name: &str, fields: &[(&String, String)], types: &Types) -> String {
    if types.boxed(name) {
        let args: String = fields.iter().map(|(_, v)| format!(" {v}")).collect();
        format!("(ctor {name}.mk{args})")
    } else {
        let parts: Vec<String> = fields.iter().map(|(f, v)| format!("{f} := {v}")).collect();
        format!("(record {name} {{{}}})", parts.join(", "))
    }
}

/// The `i`-th item of a tuple value `whole` of type `ty`.
pub(crate) fn tuple_item(ty: &Ty, whole: &str, i: usize) -> String {
    match ty {
        Ty::Pair(_) if i == 0 => format!("(first {whole})"),
        Ty::Pair(_) => format!("(second {whole})"),
        _ => format!("(project {whole} item{i})"),
    }
}

/// A tuple value of type `ty` from its items.
pub(crate) fn tuple_value(ty: &Ty, items: &[String]) -> String {
    match ty {
        Ty::Pair(_) => format!("({}, {})", items[0], items[1]),
        _ => {
            let parts: Vec<String> = items
                .iter()
                .enumerate()
                .map(|(i, v)| format!("item{i} := {v}"))
                .collect();
            format!("(record {} {{{}}})", ty.render(), parts.join(", "))
        }
    }
}

/// The unit-variant enums and the structs the lift can name.
#[derive(Clone, Debug, Default)]
pub(crate) struct Types {
    pub(crate) enums: BTreeMap<String, Vec<String>>,
    /// Struct name -> fields `(LexLean name, type)` in declaration order.
    pub(crate) structs: BTreeMap<String, Vec<(String, Ty)>>,
    /// A Rust type name -> the LexLean name it lifts to, where the crate's
    /// name qualifies it apart from another crate's type of the same name.
    pub(crate) aliases: BTreeMap<String, String>,
    /// Enums whose variants carry data -> each variant and its fields:
    /// the Rust field name of a struct variant, and the type.
    pub(crate) datas: BTreeMap<String, Vec<(String, Vec<DataField>)>>,
    /// The recursive families: the enums and structs that name themselves
    /// or each other, each one inductive group.
    pub(crate) families: Vec<BTreeSet<String>>,
    /// The traits each type derives (`Ord`, `PartialEq`, ...), by LexLean
    /// name: a derived order or equality is the structural one.
    pub(crate) derives: BTreeMap<String, BTreeSet<String>>,
    /// The MIR path's closures, by their type as MIR prints it: the types
    /// of their captures, as printed.
    pub(crate) closures: BTreeMap<String, Vec<String>>,
    /// Whether this is core's own scope, where MIR prints core's paths
    /// without the crate: `ops::ControlFlow` is `core::ops::ControlFlow`.
    pub(crate) core: bool,
    /// The floating-point types the scope reads, by name (`f32`), each the
    /// format core's own constants define for it.
    pub(crate) floats: BTreeMap<String, crate::mir::float::Format>,
    /// Each struct whose one field is a pattern type (`(u32) is 1..`), by
    /// LexLean name: the pattern's base type and the pattern, as written.
    pub(crate) valid: BTreeMap<String, (String, String)>,
    /// The structs that hold a mutable borrow, by LexLean name: each field
    /// that is a `&mut`, by position, with its type as printed. The lift
    /// holds the borrowed value there, threaded as the borrow is.
    pub(crate) borrowing: BTreeMap<String, Vec<(usize, String)>>,
}

/// The strongly connected components of a graph, each one set.
fn strong_components(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<BTreeSet<String>> {
    let reach = |start: &String| {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&String> = graph.get(start).into_iter().flatten().collect();
        while let Some(n) = stack.pop() {
            if seen.insert(n.clone()) {
                stack.extend(graph.get(n).into_iter().flatten());
            }
        }
        seen
    };
    let reaches: BTreeMap<&String, BTreeSet<String>> =
        graph.keys().map(|n| (n, reach(n))).collect();
    let mut out: Vec<BTreeSet<String>> = Vec::new();
    for n in graph.keys() {
        if out.iter().any(|c| c.contains(n)) {
            continue;
        }
        let mut family: BTreeSet<String> = reaches[n]
            .iter()
            .filter(|m| reaches.get(m).is_some_and(|r| r.contains(n)))
            .cloned()
            .collect();
        family.insert(n.clone());
        out.push(family);
    }
    out
}

/// Whether every occurrence of a member of `family` in `ty` is strictly
/// positive and nested only where Lean nests an inductive: directly, or
/// under `Option`, `List`, `Result` or a product.
fn positive(ty: &Ty, family: &BTreeSet<String>) -> bool {
    match ty {
        Ty::Option(t) | Ty::List(t) => positive(t, family),
        Ty::Outcome(t, e) => positive(t, family) && positive(e, family),
        Ty::Pair(ts) => ts.iter().all(|t| positive(t, family)),
        Ty::Tuple(_) | Ty::Map(..) | Ty::Set(..) => {
            let inner = ty.mentions();
            named_in(&inner).is_disjoint(family)
        }
        _ => true,
    }
}

/// A field of a data-carrying variant: its Rust name, if any, and its type.
pub(crate) type DataField = (Option<String>, Ty);

/// The fields of each variant of a data-carrying enum, as Rust writes them.
pub(crate) type RawVariants = Vec<(String, Vec<(Option<String>, rs::RustType)>)>;

/// Whether `ty` names `me` only where Lean accepts a nested occurrence:
/// directly, or inside `Option`, `List` or `Result`.
fn nested_ok(ty: &Ty, me: &str) -> bool {
    match ty {
        Ty::Option(t) | Ty::List(t) => nested_ok(t, me),
        Ty::Outcome(t, e) => nested_ok(t, me) && nested_ok(e, me),
        Ty::Pair(ts) => ts.iter().all(|t| nested_ok(t, me)),
        Ty::Tuple(_) | Ty::Map(..) | Ty::Set(..) => {
            let inner = ty.mentions();
            !inner.contains(&Ty::Named(me.to_string()))
        }
        _ => true,
    }
}

/// The suffix of the size function of a type of a nested family: the
/// member's name, behind `list_` or `option_` for a container around it.
fn size_suffix(ty: &Ty, family: &BTreeSet<String>) -> Option<String> {
    match ty {
        Ty::Named(n) if family.contains(n) => Some(n.clone()),
        // LexLean measures a member, or a list or an option of one: a
        // container of a container is outside every recursive family.
        Ty::List(t) => match &**t {
            Ty::Named(n) if family.contains(n) => Some(format!("list_{n}")),
            _ => None,
        },
        Ty::Option(t) => match &**t {
            Ty::Named(n) if family.contains(n) => Some(format!("option_{n}")),
            _ => None,
        },
        _ => None,
    }
}

/// The size domains with a field type's added: the containers inside it
/// first, then itself, where it is a container that has a size.
fn with_domains(ty: &Ty, family: &BTreeSet<String>, domains: Vec<Ty>) -> Vec<Ty> {
    let domains = match ty {
        Ty::List(t) | Ty::Option(t) => with_domains(t, family, domains),
        _ => domains,
    };
    if matches!(ty, Ty::Named(_)) || size_suffix(ty, family).is_none() || domains.contains(ty) {
        return domains;
    }
    let mut domains = domains;
    domains.push(ty.clone());
    domains
}

impl Types {
    /// The lifted types with data-carrying enums among them: every type to
    /// the fixpoint where each names only types that lift, and no two name
    /// each other (a data enum may name itself).
    pub(crate) fn with_data(
        enums: BTreeMap<String, Vec<String>>,
        datas: &BTreeMap<String, RawVariants>,
        structs: &BTreeMap<String, Vec<(String, rs::RustType)>>,
        aliases: BTreeMap<String, String>,
    ) -> Self {
        let base = Self::new(enums, structs, aliases);
        let mut names: BTreeSet<String> = datas
            .keys()
            .filter(|n| {
                !base.enums.contains_key(*n)
                    && !base.structs.contains_key(*n)
                    && !BARE_TYPES.contains(&n.as_str())
                    && !n.starts_with("LexLean")
                    && !n.starts_with("Tuple")
                    && !datas[*n].is_empty()
                    && datas[*n]
                        .iter()
                        .all(|(v, _)| !RESERVED.contains(&v.as_str()))
            })
            .cloned()
            .collect();
        // The structs `new` may name: not an enum, and not a name generated
        // Lean writes bare or that the projection generates.
        let mut struct_names: BTreeSet<String> = structs
            .keys()
            .filter(|n| {
                !base.enums.contains_key(*n)
                    && !BARE_TYPES.contains(&n.as_str())
                    && !n.starts_with("LexLean")
                    && !n.starts_with("Tuple")
            })
            .cloned()
            .collect();
        loop {
            let probe = Self {
                datas: names.iter().map(|n| (n.clone(), Vec::new())).collect(),
                structs: struct_names
                    .iter()
                    .filter(|n| !base.enums.contains_key(*n))
                    .map(|n| (n.clone(), Vec::new()))
                    .collect(),
                ..base.clone()
            };
            let mut lifted = BTreeMap::new();
            for name in &names {
                let mut variants = Vec::new();
                let mut ok = true;
                for (variant, fields) in &datas[name] {
                    let mut tys = Vec::new();
                    for (field, t) in fields {
                        match probe.ty(t) {
                            Ok(ty) if nested_ok(&ty, name) => tys.push((field.clone(), ty)),
                            _ => ok = false,
                        }
                    }
                    variants.push((variant.clone(), tys));
                }
                if ok {
                    lifted.insert(name.clone(), variants);
                }
            }
            let mut fields_of = BTreeMap::new();
            for name in &struct_names {
                let Some(raw) = structs.get(name) else {
                    continue;
                };
                let mut fields = Vec::new();
                let mut ok = !raw.is_empty();
                for (field, t) in raw {
                    match probe.ty(t) {
                        Ok(ty) => fields.push((local_name(field), ty)),
                        Err(_) => ok = false,
                    }
                }
                if ok {
                    fields_of.insert(name.clone(), fields);
                }
            }
            // The named types each type mentions, and those on a cycle
            // through another type, or a structure through itself.
            let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for (name, variants) in &lifted {
                let mut out = BTreeSet::new();
                for (_, fields) in variants {
                    for (_, t) in fields {
                        out.extend(t.mentions());
                    }
                }
                graph.insert(name.clone(), named_in(&out));
            }
            for (name, fields) in &fields_of {
                let mut out = BTreeSet::new();
                for (_, t) in fields {
                    out.extend(t.mentions());
                }
                graph.insert(name.clone(), named_in(&out));
            }
            // A family of types that name each other is one mutual group,
            // where every occurrence of a member is strictly positive: held
            // directly or under `Option`, `Vec` or `Result`. One through a
            // generated tuple structure, a map or a set is refused.
            let mut cyclic = BTreeSet::new();
            let mut families = Vec::new();
            for family in strong_components(&graph) {
                let recursive = family.len() > 1 || family.iter().all(|n| graph[n].contains(n));
                if !recursive {
                    continue;
                }
                let fields = family.iter().flat_map(|n| {
                    lifted
                        .get(n)
                        .into_iter()
                        .flatten()
                        .flat_map(|(_, fs)| fs.iter().map(|(_, t)| t.clone()))
                        .chain(
                            fields_of
                                .get(n)
                                .into_iter()
                                .flatten()
                                .map(|(_, t)| t.clone()),
                        )
                        .collect::<Vec<_>>()
                });
                let positive = fields.clone().all(|t| positive(&t, &family));
                if positive {
                    families.push(family);
                } else {
                    cyclic.extend(family);
                }
            }
            lifted.retain(|n, _| !cyclic.contains(n));
            fields_of.retain(|n, _| !cyclic.contains(n));
            if lifted.len() == names.len() && fields_of.len() == struct_names.len() {
                return Self {
                    datas: lifted,
                    structs: fields_of,
                    families,
                    ..base
                };
            }
            names = lifted.into_keys().collect();
            struct_names = fields_of.into_keys().collect();
        }
    }

    /// The structs whose every field lifts, to the fixpoint where a struct
    /// only names enums and structs that lift.
    /// Rust type names `aliases` maps to the LexLean names the maps are
    /// keyed by: a crate's own names, apart from another crate's.
    pub(crate) fn new(
        enums: BTreeMap<String, Vec<String>>,
        structs: &BTreeMap<String, Vec<(String, rs::RustType)>>,
        aliases: BTreeMap<String, String>,
    ) -> Self {
        let enums: BTreeMap<String, Vec<String>> = enums
            .into_iter()
            .filter(|(n, vs)| {
                !BARE_TYPES.contains(&n.as_str())
                    && !n.starts_with("LexLean")
                    && vs.iter().all(|v| !RESERVED.contains(&v.as_str()))
            })
            .collect();
        let mut names: BTreeSet<String> = structs
            .keys()
            .filter(|n| {
                !enums.contains_key(*n)
                    && !BARE_TYPES.contains(&n.as_str())
                    && !n.starts_with("LexLean")
                    && !n.starts_with("Tuple")
            })
            .cloned()
            .collect();
        loop {
            let probe = Self {
                enums: enums.clone(),
                structs: names.iter().map(|n| (n.clone(), Vec::new())).collect(),
                aliases: aliases.clone(),
                datas: BTreeMap::new(),
                families: Vec::new(),
                derives: BTreeMap::new(),
                closures: BTreeMap::new(),
                core: false,
                floats: BTreeMap::new(),
                valid: BTreeMap::new(),
                borrowing: BTreeMap::new(),
            };
            let mut lifted = BTreeMap::new();
            for name in &names {
                let mut fields = Vec::new();
                let mut ok = !structs[name].is_empty();
                for (field, t) in &structs[name] {
                    match probe.ty(t) {
                        Ok(ty) => fields.push((local_name(field), ty)),
                        Err(_) => ok = false,
                    }
                }
                if ok {
                    lifted.insert(name.clone(), fields);
                }
            }
            if lifted.len() == names.len() {
                return Self {
                    enums,
                    structs: lifted,
                    aliases,
                    datas: BTreeMap::new(),
                    families: Vec::new(),
                    derives: BTreeMap::new(),
                    closures: BTreeMap::new(),
                    core: false,
                    floats: BTreeMap::new(),
                    valid: BTreeMap::new(),
                    borrowing: BTreeMap::new(),
                };
            }
            names = lifted.into_keys().collect();
        }
    }

    /// The type of a tuple of these items: a product when it is a pair that
    /// names a data-carrying enum, a generated structure otherwise.
    pub(crate) fn tuple(&self, ts: Vec<Ty>) -> Ty {
        let mut inner = BTreeSet::new();
        for t in &ts {
            inner.extend(t.mentions());
        }
        let data = named_in(&inner).iter().any(|n| self.datas.contains_key(n));
        if ts.len() == 2 && data {
            Ty::Pair(ts)
        } else {
            Ty::Tuple(ts)
        }
    }

    /// The recursive family a named type belongs to.
    pub(crate) fn family_of(&self, name: &str) -> Option<&BTreeSet<String>> {
        self.families.iter().find(|f| f.contains(name))
    }

    /// A struct of a recursive family: declared as an inductive of one
    /// constructor `mk`, its fields read through generated accessors, since
    /// a structure cannot join a mutual group.
    pub(crate) fn boxed(&self, name: &str) -> bool {
        self.structs.contains_key(name) && self.family_of(name).is_some()
    }

    /// The field types of a member of a family, over all its constructors.
    pub(crate) fn member_fields(&self, name: &str) -> Vec<Ty> {
        let data = self.datas.get(name).into_iter().flatten();
        let mut out: Vec<Ty> = data
            .flat_map(|(_, fs)| fs.iter().map(|(_, t)| t.clone()))
            .collect();
        out.extend(
            self.structs
                .get(name)
                .into_iter()
                .flatten()
                .map(|(_, t)| t.clone()),
        );
        out
    }

    /// A type Lean calls nested or mutual: its family has more than one
    /// member, or a member occurs under a container. A function recursing
    /// on one is well-founded on its size, not structural.
    pub(crate) fn nested(&self, name: &str) -> bool {
        let Some(family) = self.family_of(name) else {
            return false;
        };
        family.len() > 1
            || family.iter().any(|m| {
                self.member_fields(m).iter().any(|t| {
                    !matches!(t, Ty::Named(_)) && {
                        let inner = t.mentions();
                        !named_in(&inner).is_disjoint(family)
                    }
                })
            })
    }

    /// The size function of a type of a nested family: the members, and
    /// lists and options of types that have one. `None` for any other type.
    pub(crate) fn size_name(&self, ty: &Ty, family: &BTreeSet<String>) -> Option<String> {
        size_suffix(ty, family).map(|s| format!("size_of_{s}"))
    }

    /// The types the sizes of a family range over: the members, then each
    /// list or option the members hold whose items have a size, after the
    /// ones inside it.
    pub(crate) fn size_domains(&self, family: &BTreeSet<String>) -> Vec<Ty> {
        let fields: Vec<Ty> = family.iter().flat_map(|m| self.member_fields(m)).collect();
        let members: Vec<Ty> = family.iter().map(|m| Ty::Named(m.clone())).collect();
        fields
            .iter()
            .fold(members, |domains, t| with_domains(t, family, domains))
    }

    /// Whether a nested family has a structural size: every occurrence of a
    /// member is direct, or the whole element of a list or an option.
    pub(crate) fn sizable(&self, family: &BTreeSet<String>) -> bool {
        family.iter().all(|m| {
            self.member_fields(m).iter().all(|t| {
                let inner = t.mentions();
                named_in(&inner).is_disjoint(family) || self.size_name(t, family).is_some()
            })
        })
    }

    /// The accessor of a field of a struct of a recursive family.
    pub(crate) fn accessor(name: &str, field: &str) -> String {
        format!("field_{name}_{field}")
    }

    /// The LexLean name of a Rust type name.
    pub(crate) fn resolve(&self, rust: &str) -> String {
        self.aliases
            .get(rust)
            .cloned()
            .unwrap_or_else(|| rust.to_string())
    }

    pub(crate) fn ty(&self, t: &rs::RustType) -> Lift<Ty> {
        let bad = || Err(Residue::Type(t.text.clone()));
        match &t.kind {
            rs::TypeKind::Reference {
                mutable: false,
                elem,
            } => match &elem.kind {
                rs::TypeKind::Path { qself: false, path } if path.is_ident("str") => Ok(Ty::Str),
                rs::TypeKind::Slice(s) => Ok(Ty::List(Box::new(self.ty(s)?))),
                _ => self.ty(elem),
            },
            rs::TypeKind::Paren(p) => self.ty(p),
            rs::TypeKind::Fn(ins, out) => {
                let mut inputs = Vec::new();
                for t in ins {
                    inputs.push(self.ty(t)?);
                }
                Ok(Ty::Fn(inputs, Box::new(self.ty(out)?)))
            }
            // An array is the list of its items: its length is the
            // literal's, which the value fixes.
            rs::TypeKind::Array(t, _) => Ok(Ty::List(Box::new(self.ty(t)?))),
            rs::TypeKind::Tuple(elems) if elems.is_empty() => Ok(Ty::Unit),
            rs::TypeKind::Tuple(elems) if elems.len() >= 2 => {
                let mut ts = Vec::new();
                for e in elems {
                    ts.push(self.ty(e)?);
                }
                Ok(self.tuple(ts))
            }
            // `std::path::PathBuf`, written out.
            rs::TypeKind::Path { qself: false, path }
                if path.segments.len() == 3
                    && path.segments[0].ident == "std"
                    && path.segments[1].ident == "path"
                    && matches!(path.segments[2].ident.as_str(), "Path" | "PathBuf") =>
            {
                Ok(Ty::Path)
            }
            rs::TypeKind::Path { qself: false, path } if path.segments.len() == 1 => {
                let seg = &path.segments[0];
                let name = seg.ident.clone();
                let mut args = Vec::new();
                match &seg.arguments {
                    rs::PathArguments::None => {}
                    rs::PathArguments::AngleBracketed(a) => {
                        for arg in a {
                            match arg {
                                rs::GenericArgument::Type(t) => args.push(self.ty(t)?),
                                // A lifetime is erased.
                                rs::GenericArgument::Other(l) if l.starts_with('\'') => {}
                                rs::GenericArgument::Other(_) => return bad(),
                            }
                        }
                    }
                    rs::PathArguments::Parenthesized => return bad(),
                }
                let ty = match (name.as_str(), args.as_slice()) {
                    ("bool", []) => Ty::Bool,
                    ("u8", []) => Ty::Nat(Some(8)),
                    ("u16", []) => Ty::Nat(Some(16)),
                    ("u32", []) => Ty::Nat(Some(32)),
                    ("u64", []) => Ty::Nat(Some(64)),
                    ("u128", []) => Ty::Nat(Some(128)),
                    ("usize", []) => Ty::Nat(None),
                    ("i8", []) => Ty::Int(8),
                    ("i16", []) => Ty::Int(16),
                    ("i32", []) => Ty::Int(32),
                    ("i64", []) => Ty::Int(64),
                    ("String", []) => Ty::Str,
                    ("char", []) => Ty::Char,
                    ("Option", [t]) => Ty::Option(Box::new(t.clone())),
                    ("Result", [t, e]) => Ty::Outcome(Box::new(t.clone()), Box::new(e.clone())),
                    ("Vec", [t]) => Ty::List(Box::new(t.clone())),
                    ("Box", [t]) => t.clone(),
                    ("BTreeMap", [k, v]) => Ty::Map(Box::new(k.clone()), Box::new(v.clone()), true),
                    ("HashMap", [k, v]) => Ty::Map(Box::new(k.clone()), Box::new(v.clone()), false),
                    ("BTreeSet", [t]) => Ty::Set(Box::new(t.clone()), true),
                    ("HashSet", [t]) => Ty::Set(Box::new(t.clone()), false),
                    (n, [])
                        if self.enums.contains_key(&self.resolve(n))
                            || self.structs.contains_key(&self.resolve(n))
                            || self.datas.contains_key(&self.resolve(n)) =>
                    {
                        Ty::Named(self.resolve(n))
                    }
                    // The standard path, where the crate declares no type
                    // of the name.
                    ("Path" | "PathBuf", []) => Ty::Path,
                    _ => return bad(),
                };
                Ok(ty)
            }
            _ => bad(),
        }
    }

    /// The LexLean signature of a candidate, or why it is not one.
    pub(crate) fn signature(
        &self,
        sig: &rs::Signature,
        owner: Option<&str>,
    ) -> Lift<(Vec<Param>, Ty)> {
        // A lifetime parameter says how long a borrow lives, which a value
        // of LexLean does not have: it is erased. A type or const parameter
        // is not.
        let generic = sig
            .generics
            .iter()
            .any(|p| !matches!(p, rs::GenericParam::Lifetime(_)));
        if generic || sig.asyncness || sig.unsafety || sig.variadic {
            return Err(Residue::Signature(
                "generic, async, unsafe or variadic".to_string(),
            ));
        }
        let mut params = Vec::new();
        for input in &sig.inputs {
            match input {
                rs::FnArg::Receiver { plain } => match owner {
                    Some(o)
                        if *plain
                            && (self.enums.contains_key(o)
                                || self.structs.contains_key(o)
                                || self.datas.contains_key(o)) =>
                    {
                        params.push((
                            "self".to_string(),
                            local_name("self"),
                            Ty::Named(o.to_string()),
                        ));
                    }
                    _ => return Err(Residue::Signature("takes self".to_string())),
                },
                rs::FnArg::Typed(pat) => {
                    let rs::Pat::Ident(name) = &*pat.pat else {
                        return Err(Residue::Signature("pattern parameter".to_string()));
                    };
                    let rust = name.ident.clone();
                    params.push((rust.clone(), local_name(&rust), self.ty_in(&pat.ty, owner)?));
                }
            }
        }
        let result = match &sig.output {
            None => Ty::Unit,
            Some(t) => self.ty_in(t, owner)?,
        };
        Ok((params, result))
    }

    /// A type where `Self` names the method's owner.
    fn ty_in(&self, t: &rs::RustType, owner: Option<&str>) -> Lift<Ty> {
        let plain = match &t.kind {
            rs::TypeKind::Reference {
                mutable: false,
                elem,
            } => elem,
            _ => t,
        };
        match (&plain.kind, owner) {
            (rs::TypeKind::Path { qself: false, path }, Some(o)) if path.is_ident("Self") => {
                Ok(Ty::Named(o.to_string()))
            }
            _ => self.ty(t),
        }
    }
}

/// Words a LexLean or Lean name may not be: the semantic notation's keywords
/// and Lean's.
const RESERVED: &[&str] = &[
    "at",
    "by",
    "calc",
    "def",
    "do",
    "else",
    "end",
    "example",
    "fun",
    "have",
    "if",
    "in",
    "instance",
    "let",
    "match",
    "mut",
    "nomatch",
    "open",
    "prop",
    "return",
    "show",
    "then",
    "theorem",
    "type",
    "where",
    "with",
    "from",
    "for",
    "try",
    "catch",
    "break",
    "continue",
    "unless",
    "structure",
    "inductive",
    "class",
    "namespace",
    "section",
    "local",
    "private",
    "protected",
    "partial",
    "unsafe",
    "macro",
    "syntax",
    "import",
    "export",
    "axiom",
    "abbrev",
    "opaque",
    "mutual",
    "variable",
    "universe",
    "deriving",
    "extends",
    "suffices",
    "obtain",
    "record",
    "ctor",
    "prim",
    "project",
    "cons",
    "nil",
    "not",
    "true",
    "false",
    "forall",
    "first",
    "second",
    "apply",
    "ref",
    "checked",
    "self",
    "noncomputable",
    "termination_by",
    // Lean names generated code writes bare.
    "and_congr",
    "and_true",
    "beq_iff_eq",
    "congr",
    "decide",
    "decide_eq_false_iff_not",
    "decide_eq_true_eq",
    "id",
    "reduceIte",
    "rfl",
    "true_and",
    // Constructors Lean resolves from the root in a pattern.
    "isFalse",
    "isTrue",
    "none",
    "some",
    // Members Lean declares for every structure.
    "below",
    "brecOn",
    "casesOn",
    "ctorElim",
    "ctorElimType",
    "ctorIdx",
    "mk",
    "noConfusion",
    "noConfusionType",
    "rec",
    "recOn",
    "toCtorIdx",
];

/// Type names generated Lean writes bare: a struct or enum may not take one.
pub(crate) const BARE_TYPES: &[&str] = &[
    "And",
    "BitVec",
    "Bool",
    "ByteArray",
    "ContractViolation",
    "Except",
    "Iff",
    "Int",
    "Int16",
    "Int32",
    "Int64",
    "Int8",
    "List",
    "Nat",
    "Option",
    "Ordering",
    "Prod",
    "Prop",
    "ReasoningFailure",
    "Result",
    "String",
    "Type",
    "UInt16",
    "UInt32",
    "UInt64",
    "UInt8",
    "Unit",
];

/// The LexLean name of a Rust local, parameter or field.
pub(crate) fn local_name(ident: &str) -> String {
    let name = camel(&[ident.trim_start_matches("r#")]);
    if name.is_empty() || RESERVED.contains(&name.as_str()) {
        format!("{name}Value")
    } else {
        name
    }
}

/// A parameter: its Rust name, its LexLean name and its type.
pub(crate) type Param = (String, String, Ty);

/// A lifted function's signature.
#[derive(Clone, Debug)]
pub(crate) struct Sig {
    pub(crate) name: String,
    pub(crate) params: Vec<Ty>,
    pub(crate) result: Ty,
}

/// What the lift reads and does not change while it lifts a crate: the
/// types it names and the candidate functions' signatures.
pub(crate) struct Env {
    pub(crate) types: Types,
    /// Candidate functions: Rust name -> signature.
    pub(crate) fns: BTreeMap<String, Sig>,
    /// The pointer widths `usize` takes, each with the targets it is the
    /// width of.
    pub(crate) widths: Vec<(u32, Vec<String>)>,
    /// The LexLean names of every crate's candidates, which share one
    /// module: a binder takes none of them.
    pub(crate) names: BTreeSet<String>,
}

/// The scope an expression or statement lifts in.
#[derive(Clone)]
struct Ctx {
    /// The Rust name of the function being lifted.
    current: String,
    /// Its parameters' LexLean names, in order.
    params: Vec<String>,
    /// Its result type.
    result: Ty,
    /// The LexLean name of the function being lifted, which its helpers extend.
    own: String,
    /// Rust local -> (LexLean name, type).
    locals: BTreeMap<String, (String, Ty)>,
    /// LexLean local bound to the tail of a slice parameter -> that parameter.
    tails: BTreeMap<String, String>,
    /// The statements in hand are the function's own, in tail position, so
    /// an early `return` leaves the function.
    top: bool,
    /// The locals of the loop whose step the statements in hand are: a
    /// `continue` there is the step's value, a `break` its last.
    looping: Option<Vec<String>>,
    /// Binders still in scope whose Rust local a later binding renamed.
    hidden: Vec<String>,
    /// The `use` imports of the function's file: name -> the path it names.
    imports: BTreeMap<String, String>,
    /// The closures `let` binds, by Rust name: a call of one is its body
    /// with its parameters bound to the arguments.
    closures: BTreeMap<String, Closure>,
    /// The parameter a list in hand is a sub-term of, where a closure
    /// walking it binds each element: a recursive call on one descends.
    elements: Option<String>,
    /// A parameter a match split -> its constructor applied to the arm's
    /// binders, which names it there.
    rebuilt: BTreeMap<String, String>,
    /// Natural-number terms -> a lower bound the conditions in scope
    /// establish: `n` at least 2 after `if n < 2 { return .. }`.
    bounds: BTreeMap<String, u128>,
    /// Natural-number terms the conditions in scope hold below a `usize`
    /// -> the largest constant that added to one stays in range: one below
    /// any `usize`, half the narrowest width below a collection's length,
    /// which is at most `isize::MAX`.
    capped: BTreeMap<String, u128>,
    /// The state type of the loop step the statements in hand are, where
    /// they are the function's own: a `return` or `?` there leaves the walk
    /// with the function's value.
    exits: Option<Ty>,
    /// The formatter parameter of a `Display` impl's `fmt`, which `write!`
    /// writes the function's text into.
    formatter: Option<String>,
}

/// A closure a `let` binds: its parameters, its body, and the locals in
/// scope where it is written, which its body reads.
#[derive(Clone)]
struct Closure {
    inputs: Vec<rs::Pat>,
    body: rs::RustExpr,
    locals: BTreeMap<String, (String, Ty)>,
}

/// What lifting a body gathers as it goes. Each step takes it and gives
/// back the state after it, so the steps of a body are a fold over it.
#[derive(Clone, Debug, Default)]
struct Acc {
    /// The parameter positions recursive calls descend on.
    descends: BTreeSet<usize>,
    /// The other lifted functions the body calls.
    calls: BTreeSet<String>,
    /// The fresh binders the body has taken, in order.
    given: Vec<String>,
    /// Every type an expression of the body has.
    seen: BTreeSet<Ty>,
    /// The Lean axioms the body's primitives elaborate with.
    axioms: BTreeSet<String>,
    /// The list helpers the body's iterator chains lift to.
    helpers: Vec<Helper>,
    /// The types whose derived order the body compares by.
    ordered: BTreeSet<Ty>,
}

impl Acc {
    /// The state with `name` called.
    fn called(self, name: String) -> Acc {
        let mut calls = self.calls;
        calls.insert(name);
        Acc { calls, ..self }
    }

    /// The state with every one of `names` called.
    fn called_all(self, names: BTreeSet<String>) -> Acc {
        let mut calls = self.calls;
        calls.extend(names);
        Acc { calls, ..self }
    }

    /// The state with the axiom `name`.
    fn axiom(self, name: &str) -> Acc {
        let mut axioms = self.axioms;
        axioms.insert(name.to_string());
        Acc { axioms, ..self }
    }

    /// The state with every one of `names`.
    fn axioms_all(self, names: BTreeSet<String>) -> Acc {
        let mut axioms = self.axioms;
        axioms.extend(names);
        Acc { axioms, ..self }
    }

    /// The state with the axioms of the runtime's string length and decimal
    /// formatting.
    fn all_axioms(self) -> Acc {
        self.axioms_all(ALL_AXIOMS.iter().map(|a| a.to_string()).collect())
    }

    /// The state with every type `ty` mentions seen.
    fn saw(self, ty: &Ty) -> Acc {
        let mut seen = self.seen;
        seen.extend(ty.mentions());
        Acc { seen, ..self }
    }

    /// The state comparing values of `ty` by its derived order: the
    /// comparison's axioms, and `Ordering`, which it results in.
    fn ordered(self, ty: &Ty, types: &Types) -> Acc {
        let mut ordered = self.ordered;
        ordered.insert(crate::order::normal(ty));
        let acc = Acc { ordered, ..self }.saw(&Ty::Named(ORDERING.into()));
        acc.axioms_all(crate::order::axioms(ty, types))
    }

    /// The state with a recursive call descending on parameter `at`.
    fn descended(self, at: usize) -> Acc {
        let mut descends = self.descends;
        descends.insert(at);
        Acc { descends, ..self }
    }

    /// The state with one more helper.
    fn helper(self, helper: Helper) -> Acc {
        let mut helpers = self.helpers;
        helpers.push(helper);
        Acc { helpers, ..self }
    }

    /// The state of a scope of its own, apart from the calls, axioms and
    /// descents gathered so far, which `rejoin` puts back.
    fn apart(self) -> (Acc, Gathered) {
        let gathered = Gathered {
            calls: self.calls,
            axioms: self.axioms,
            descends: self.descends,
        };
        let scope = Acc {
            calls: BTreeSet::new(),
            axioms: BTreeSet::new(),
            descends: BTreeSet::new(),
            ..self
        };
        (scope, gathered)
    }

    /// The state after a scope of its own: the scope's fresh names, types
    /// and helpers, with the calls, axioms and descents gathered before it,
    /// and the scope's own apart.
    fn rejoin(self, before: Gathered) -> (Acc, Gathered) {
        let own = Gathered {
            calls: self.calls,
            axioms: self.axioms,
            descends: self.descends,
        };
        let acc = Acc {
            calls: before.calls,
            axioms: before.axioms,
            descends: before.descends,
            ..self
        };
        (acc, own)
    }

    /// A call of a prelude definition, with the axioms it brings.
    fn prelude(self, name: &str, args: &[&str]) -> (String, Acc) {
        let acc = self.axioms_all(prelude_axioms(name).into_iter().collect());
        (format!("({PRELUDE}::{name} {})", args.join(" ")), acc)
    }
}

/// The calls, axioms and descents of a scope, held apart from another's.
#[derive(Clone, Debug, Default)]
struct Gathered {
    calls: BTreeSet<String>,
    axioms: BTreeSet<String>,
    descends: BTreeSet<usize>,
}

/// A structurally recursive definition an iterator chain lifts to: it walks
/// its first parameter, a list, and takes the closure's captures after it.
#[derive(Clone, Debug)]
pub(crate) struct Helper {
    pub(crate) name: String,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) result: String,
    pub(crate) body: Expr,
    /// The lifted functions the closure calls.
    pub(crate) calls: BTreeSet<String>,
    /// The Lean axioms the closure's primitives elaborate with.
    pub(crate) axioms: BTreeSet<String>,
    /// It recurses on its first parameter; a hoisted operand does not.
    pub(crate) recursive: bool,
    /// Its closure calls the function being lifted on an element of the
    /// list it walks, a sub-term of the function's recursive parameter:
    /// the two are one mutual group, each structural on its own.
    pub(crate) entwined: bool,
    /// A walk that is not structural: its measure and the theorems that
    /// each recursive call decreases it, which precede it.
    pub(crate) termination: Option<(String, Vec<Decl>)>,
}

/// The first of `base1`, `base2`, .. that `taken` does not hold. Each
/// taken name rules out at most one of the first `taken.len() + 1`
/// numbers, so one of them is free.
fn first_free(base: &str, taken: &[String]) -> String {
    #[cfg(not(uorust_lift))]
    crate::certified::certified!(lift_first_free, base, taken);
    (1..taken.len() + 2)
        .map(|k| format!("{base}{k}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

impl Ctx {
    /// A fresh name for a helper of the function being lifted.
    fn helper_name(&self, env: &Env, acc: &Acc, kind: &str) -> String {
        let base = format!("{}{kind}", self.own);
        let helpers: Vec<String> = acc.helpers.iter().map(|h| h.name.clone()).collect();
        first_free(&base, &self.names_from(env, acc, &base, helpers))
    }

    /// The names in scope, given or in `more` that start with `base`.
    fn names_from(&self, env: &Env, acc: &Acc, base: &str, more: Vec<String>) -> Vec<String> {
        self.locals
            .values()
            .map(|(n, _)| n.clone())
            .chain(self.hidden.iter().cloned())
            .chain(env.fns.values().map(|f| f.name.clone()))
            .chain(env.names.iter().cloned())
            .chain(acc.given.iter().cloned())
            .chain(more)
            .filter(|n| n.starts_with(base))
            .collect()
    }

    fn taken(&self, env: &Env, name: &str) -> bool {
        self.locals.values().any(|(n, _)| n == name)
            || self.hidden.iter().any(|n| n == name)
            || env.fns.values().any(|f| f.name == name)
            || env.names.contains(name)
    }

    /// A LexLean binder for a Rust name: a binder may not shadow another.
    fn bind(&self, env: &Env, acc: Acc, ident: &str, ty: Ty) -> (Ctx, String, Acc) {
        let name = local_name(ident);
        let (name, acc) = if self.taken(env, &name) {
            self.fresh(env, acc, &name)
        } else {
            (name, acc)
        };
        let mut next = self.clone();
        if let Some((old, _)) = next.locals.insert(ident.to_string(), (name.clone(), ty)) {
            next.hidden.push(old);
        }
        (next, name, acc)
    }

    /// A binder no name in scope and no name given before takes: `base`
    /// and a number.
    fn fresh(&self, env: &Env, acc: Acc, base: &str) -> (String, Acc) {
        let name = first_free(base, &self.names_from(env, &acc, base, Vec::new()));
        let mut given = acc.given;
        given.push(name.clone());
        (name, Acc { given, ..acc })
    }

    /// The same scope where `cond` is `holds`: with the lower bound it
    /// gives a natural-number term, from `x >= k`, `x > k`, `!(x < k)`,
    /// `!(x <= k)` or `x != 0`, either way round.
    fn assuming(&self, cond: &rs::RustExpr, holds: bool, env: &Env, acc: &Acc) -> Ctx {
        use rs::BinOp as B;
        let next = self.clone();
        let b = match cond {
            rs::RustExpr::Unary(u) if matches!(u.op, rs::UnOp::Not) => {
                return self.assuming(&u.expr, !holds, env, acc);
            }
            rs::RustExpr::Paren(p) => return self.assuming(p, holds, env, acc),
            rs::RustExpr::Binary(b) => b,
            _ => return next,
        };
        // The bound on the term, with the term on the left.
        let (term, op, k) = match (int_literal(&b.left), int_literal(&b.right)) {
            (None, Some(k)) => (&*b.left, b.op, k),
            (Some(k), None) => {
                let flipped = match b.op {
                    B::Lt => B::Gt,
                    B::Le => B::Ge,
                    B::Gt => B::Lt,
                    B::Ge => B::Le,
                    other => other,
                };
                (&*b.right, flipped, k)
            }
            _ => return next,
        };
        let at_least = match (op, holds) {
            (B::Ge, true) | (B::Lt, false) => Some(k),
            (B::Gt, true) | (B::Le, false) => k.checked_add(1),
            (B::Ne, true) | (B::Eq, false) if k == 0 => Some(1),
            _ => None,
        };
        let Some(at_least) = at_least else {
            return next;
        };
        let Ok(((t, Ty::Nat(_)), _)) = flat_of(term, env, &self.below(), acc.clone(), None) else {
            return next;
        };
        let mut next = next;
        let known = next.bounds.get(&t).copied().unwrap_or(0);
        next.bounds.insert(t, known.max(at_least));
        next
    }

    /// The same scope, below the function's own statements: a `return`
    /// there is not an early exit the lift can follow.
    fn below(&self) -> Ctx {
        let mut next = self.clone();
        next.top = false;
        next
    }

    /// A value the function returns, from where the statements in hand
    /// stand: itself, or in a loop step, the mark that leaves the walk.
    fn returning(&self, value: (Expr, Ty)) -> (Expr, Ty) {
        match &self.exits {
            Some(state) => (
                map_tails(value.0, &|s| format!("{LEAVE}{s}")),
                state.clone(),
            ),
            None => value,
        }
    }
}

fn pow2(bits: u32) -> &'static str {
    match bits {
        8 => "256",
        16 => "65536",
        32 => "4294967296",
        64 => "18446744073709551616",
        _ => "340282366920938463463374607431768211456",
    }
}

fn max_of(bits: u32) -> &'static str {
    match bits {
        8 => "255",
        16 => "65535",
        32 => "4294967295",
        64 => "18446744073709551615",
        _ => "340282366920938463463374607431768211455",
    }
}

/// The single text of an expression that must stay on one line.
fn flat_text(e: Expr) -> Lift<String> {
    match e {
        Expr::Flat(s) => Ok(s),
        Expr::If { .. } if !e.flat().contains('\n') && !e.flat().contains("(match") => Ok(e.flat()),
        _ => Err(Residue::Form("match or let inside an operand".to_string())),
    }
}

/// An operand on one line. A `match` or `let` there becomes a definition of
/// its own, a helper that takes the locals it names, called in its place.
fn flat_of(
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((String, Ty), Acc)> {
    let scope = Ctx {
        top: false,
        looping: None,
        hidden: Vec::new(),
        ..ctx.clone()
    };
    let (inner, before) = acc.apart();
    let ((x, t), inner) = expr(e, env, &scope, inner, want)?;
    let (acc, own) = inner.rejoin(before);
    if let Ok(text) = flat_text(x.clone()) {
        let acc = acc.called_all(own.calls).axioms_all(own.axioms);
        let mut descends = acc.descends;
        descends.extend(own.descends);
        return Ok(((text, t), Acc { descends, ..acc }));
    }
    if !own.descends.is_empty() {
        return Err(Residue::Form(
            "recursion inside a match or let in an operand".to_string(),
        ));
    }
    let caps = captures(ctx, &[x.flat()], &[]);
    let name = ctx.helper_name(env, &acc, "Part");
    let mut parts = vec![name.clone()];
    parts.extend(caps.iter().map(|(c, _)| c.clone()));
    let acc = acc.helper(Helper {
        name: name.clone(),
        params: caps
            .iter()
            .map(|(c, ty)| (c.clone(), ty.render()))
            .collect(),
        result: t.render(),
        body: x,
        calls: own.calls,
        axioms: own.axioms,
        recursive: false,
        entwined: false,
        termination: None,
    });
    let acc = acc.called(format!("#{name}"));
    Ok(((format!("({})", parts.join(" ")), t), acc))
}

/// The pointer width of a target, from the architecture its triple names
/// first, as rustc's target specifications set `target_pointer_width`.
pub(crate) fn pointer_width(triple: &str) -> Option<u32> {
    #[cfg(not(uorust_lift))]
    crate::certified::certified!(lift_pointer_width, triple);
    let arch = triple.split('-').next()?;
    let wide = [
        "x86_64",
        "aarch64",
        "arm64",
        "riscv64",
        "powerpc64",
        "powerpc64le",
        "s390x",
        "sparc64",
        "mips64",
        "mips64el",
        "loongarch64",
        "wasm64",
        "bpfel",
        "bpfeb",
    ];
    let narrow = [
        "i386",
        "i586",
        "i686",
        "x86",
        "arm",
        "armv7",
        "armv7a",
        "armv7r",
        "armebv7r",
        "armv5te",
        "armv4t",
        "armv6",
        "thumbv6m",
        "thumbv7m",
        "thumbv7em",
        "thumbv7neon",
        "thumbv8m.base",
        "thumbv8m.main",
        "riscv32i",
        "riscv32im",
        "riscv32imc",
        "riscv32imac",
        "riscv32imafc",
        "powerpc",
        "mips",
        "mipsel",
        "sparc",
        "wasm32",
        "hexagon",
        "csky",
        "xtensa",
        "m68k",
    ];
    if wide.contains(&arch) {
        Some(64)
    } else if narrow.contains(&arch) || arch.starts_with("thumbv") || arch.starts_with("armv") {
        Some(32)
    } else if matches!(arch, "avr" | "msp430") {
        Some(16)
    } else {
        None
    }
}

/// The widths `usize` takes on the declared targets, narrowest first, each
/// with its targets. A target whose width the triple does not name, and a
/// workspace that declares none, leave every width the language allows.
pub(crate) fn pointer_widths(targets: &[String]) -> Vec<(u32, Vec<String>)> {
    let mut by: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut open = targets.is_empty();
    for t in targets {
        match pointer_width(t) {
            Some(w) => by.entry(w).or_default().push(t.clone()),
            None => open = true,
        }
    }
    if open {
        for w in [16, 32, 64] {
            by.entry(w).or_default().push(format!("any {w}-bit target"));
        }
    }
    by.into_iter().collect()
}

/// `a + b` on `usize`, which agrees with the sum of naturals on every
/// declared target where the declared limits bound it: a collection's
/// length is at most `isize::MAX`, `2^(w-1) - 1` on a target of width `w`,
/// so a length plus a constant `k <= 2^(w-1)` stays below `2^w` on all of
/// them. Otherwise the residue names the targets, by width, where the sum
/// can leave the width.
fn usize_sum(
    left: &rs::RustExpr,
    right: &rs::RustExpr,
    l: &str,
    r: &str,
    env: &Env,
    capped: &BTreeMap<String, u128>,
) -> Lift<String> {
    let is_len = |e: &rs::RustExpr| matches!(e, rs::RustExpr::MethodCall(m) if m.method == "len" && m.args.is_empty());
    let narrowest = env.widths.first().map(|(w, _)| *w);
    let half = narrowest.and_then(|w| 1u128.checked_shl(w - 1));
    let bounded = |len: &rs::RustExpr, text: &str, k: &rs::RustExpr| match int_literal(k) {
        Some(k) if is_len(len) => half.is_some_and(|h| k <= h),
        Some(k) => capped.get(text).is_some_and(|m| k <= *m),
        None => false,
    };
    if bounded(left, l, right) || bounded(right, r, left) {
        return Ok(format!("({l} + {r})"));
    }
    Err(Residue::Overflow("+".to_string(), env.widths.clone()))
}

fn int_literal(e: &rs::RustExpr) -> Option<u128> {
    match e {
        rs::RustExpr::Lit(rs::Lit::Int { value, .. }) => *value,
        rs::RustExpr::Paren(p) => int_literal(p),
        _ => None,
    }
}

fn is_literal(e: &rs::RustExpr) -> bool {
    match e {
        rs::RustExpr::Lit(_) => true,
        rs::RustExpr::Unary(u) => u.op == rs::UnOp::Neg && is_literal(&u.expr),
        rs::RustExpr::Paren(p) => is_literal(p),
        _ => false,
    }
}

/// A LexLean string literal holding exactly `text`, control characters
/// escaped; `None` when LexLean source cannot write it.
pub(crate) fn exact_string(text: &str) -> Option<String> {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            // A raw percent sign would open a TeX comment, which LexLean
            // source forbids, and its canonical form writes every string
            // with its characters raw: no spelling of it checks.
            '%' => return None,
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    Some(out)
}

/// Why a string literal does not lift.
const PERCENT: &str = "a string with a percent sign, which LexLean source cannot write";

fn literal_of(lit: &rs::Lit, want: Option<&Ty>, negative: bool) -> Lift<(Expr, Ty)> {
    let flat = |s: String, t: Ty| Ok((Expr::Flat(s), t));
    match lit {
        rs::Lit::Bool(b) if !negative => flat(b.to_string(), Ty::Bool),
        rs::Lit::Int { value, suffix } => {
            let Some(v) = *value else {
                return Err(Residue::Form("integer literal".to_string()));
            };
            let ty = match suffix.as_str() {
                "u8" => Ty::Nat(Some(8)),
                "u16" => Ty::Nat(Some(16)),
                "u32" => Ty::Nat(Some(32)),
                "u64" => Ty::Nat(Some(64)),
                "u128" => Ty::Nat(Some(128)),
                "usize" => Ty::Nat(None),
                "i8" => Ty::Int(8),
                "i16" => Ty::Int(16),
                "i32" => Ty::Int(32),
                "i64" => Ty::Int(64),
                "" => match want {
                    Some(t @ (Ty::Nat(_) | Ty::Int(_))) => t.clone(),
                    _ if negative => Ty::Int(32),
                    _ => Ty::Nat(Some(32)),
                },
                _ => return Err(Residue::Form("integer literal".to_string())),
            };
            match ty {
                Ty::Int(_) if v <= i64::MAX as u128 + u128::from(negative) => {
                    let sign = if negative { "-" } else { "" };
                    flat(format!("({sign}{v} : Int64)"), ty)
                }
                Ty::Nat(_) if !negative => flat(v.to_string(), ty),
                _ => Err(Residue::Form("integer literal".to_string())),
            }
        }
        rs::Lit::Str(s) if !negative => match exact_string(s) {
            Some(text) => flat(text, Ty::Str),
            None => Err(Residue::Form(PERCENT.to_string())),
        },
        rs::Lit::Char(c) if !negative => match exact_string(&c.to_string()) {
            Some(text) => flat(text, Ty::Char),
            None => Err(Residue::Form(PERCENT.to_string())),
        },
        rs::Lit::Byte(b) if !negative => flat(b.to_string(), Ty::Nat(Some(8))),
        rs::Lit::ByteStr(b) if !negative => {
            let mut list = "(nil Nat)".to_string();
            for byte in b.iter().rev() {
                list = format!("(cons {byte} {list})");
            }
            flat(list, Ty::List(Box::new(Ty::Nat(Some(8)))))
        }
        _ => Err(Residue::Form("literal".to_string())),
    }
}

fn ctor(path: &rs::Path, types: &Types) -> Option<(String, String)> {
    let n = path.segments.len();
    if n < 2 {
        return None;
    }
    let owner = types.resolve(&path.segments[n - 2].ident.to_string());
    let variant = path.segments[n - 1].ident.to_string();
    let unit = types
        .enums
        .get(&owner)
        .is_some_and(|vs| vs.contains(&variant))
        || types
            .datas
            .get(&owner)
            .is_some_and(|vs| vs.iter().any(|(v, fs)| *v == variant && fs.is_empty()));
    unit.then_some((owner, variant))
}

/// Whether a type is a data-carrying enum.
fn types_has_data(t: &Ty, types: &Types) -> bool {
    matches!(t, Ty::Named(n) if types.datas.contains_key(n))
}

/// A variant of a data-carrying enum a path names, and its fields.
fn data_variant<'t>(
    path: &rs::Path,
    types: &'t Types,
) -> Option<(String, String, &'t [DataField])> {
    let n = path.segments.len();
    if n < 2 {
        return None;
    }
    let owner = types.resolve(&path.segments[n - 2].ident.to_string());
    let variant = path.segments[n - 1].ident.to_string();
    let (_, fields) = types
        .datas
        .get(&owner)?
        .iter()
        .find(|(v, _)| *v == variant)?;
    Some((owner, variant, fields))
}

/// `None`, `Some`, `Ok` or `Err`, written bare or through `Option::`/`Result::`.
fn wrapper(path: &rs::Path) -> Option<&'static str> {
    let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let (last, owner) = match segs.as_slice() {
        [last] => (last.as_str(), None),
        [owner, last] => (last.as_str(), Some(owner.as_str())),
        _ => return None,
    };
    match (owner, last) {
        (None | Some("Option"), "None") => Some("None"),
        (None | Some("Option"), "Some") => Some("Some"),
        (None | Some("Result"), "Ok") => Some("Ok"),
        (None | Some("Result"), "Err") => Some("Err"),
        _ => None,
    }
}

fn expr(
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let ((x, t), acc) = expr_inner(e, env, ctx, acc, want)?;
    let acc = acc.saw(&t);
    Ok(((x, t), acc))
}

fn expr_inner(
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    // The forms a nested expression recurses through, each in a function
    // of its own, so this frame stays small however deep they nest.
    match e {
        rs::RustExpr::Paren(p) => expr(p, env, ctx, acc, want),
        rs::RustExpr::Block(b) if !b.label => block(&b.block.stmts, env, ctx, acc, want),
        rs::RustExpr::Reference(r) if !r.mutable => expr(&r.expr, env, ctx, acc, want),
        rs::RustExpr::Binary(b) => binary(b, env, ctx, acc),
        rs::RustExpr::Call(c) => call(c, env, ctx, acc, want),
        rs::RustExpr::MethodCall(m) => method(m, env, ctx, acc, want),
        rs::RustExpr::Macro(m) if m.path.is_ident("format") => format_macro(m, env, ctx, acc),
        rs::RustExpr::Macro(m) if m.path.is_ident("write") => write_macro(m, env, ctx, acc),
        rs::RustExpr::Closure(_) if matches!(want, Some(Ty::Fn(..))) => {
            function_value(e, env, ctx, acc, want)
        }
        // A closure a `let` binds, where a function is wanted: its lambda,
        // in the scope it was written in.
        rs::RustExpr::Path(p)
            if matches!(want, Some(Ty::Fn(..)))
                && p.get_ident().is_some_and(|i| ctx.closures.contains_key(i)) =>
        {
            let Some(closure) = p.get_ident().and_then(|i| ctx.closures.get(i)) else {
                return Err(Residue::Form("a closure out of scope".to_string()));
            };
            let written = rs::RustExpr::Closure(rs::ExprClosure {
                inputs: closure.inputs.clone(),
                body: Box::new(closure.body.clone()),
                asyncness: false,
            });
            let scope = Ctx {
                locals: closure.locals.clone(),
                ..ctx.clone()
            };
            function_value(&written, env, &scope, acc, want)
        }
        _ => expr_form(e, env, ctx, acc, want),
    }
}

/// A closure where a function is wanted: the lambda of its body over its
/// parameters, with the captures it reads.
fn function_value(
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(f @ Ty::Fn(inputs, output)) = want else {
        return Err(Residue::Form(
            "a closure where no function is wanted".to_string(),
        ));
    };
    let (lam, acc) = lambda(e, inputs, env, ctx, acc, Some(output))?;
    if !lam.ty.same(output) {
        return Err(Residue::Form("a closure of another type".to_string()));
    }
    if !lam.descends.is_empty() {
        return Err(Residue::Form("recursion inside a closure".to_string()));
    }
    let body = lam.wrap(lam.core.clone()).flat();
    let texts = vec![body.clone()];
    let caps = captures(ctx, &texts, &lam.binders);
    let held = match caps.as_slice() {
        [] => String::new(),
        some => format!(
            "[{}] ",
            some.iter()
                .map(|(c, _)| c.clone())
                .collect::<Vec<String>>()
                .join(", ")
        ),
    };
    let params: Vec<String> = lam
        .binders
        .iter()
        .zip(inputs)
        .map(|(b, t)| format!("({b} : {})", t.render()))
        .collect();
    let acc = acc.called_all(lam.calls).axioms_all(lam.axioms);
    Ok((
        (
            Expr::Flat(format!("(fun {held}{} => {body})", params.join(" "))),
            f.clone(),
        ),
        acc,
    ))
}

/// A value with nothing left to lift: its term and type.
fn flat_lift(s: String, t: Ty, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    Ok(((Expr::Flat(s), t), acc))
}

/// The other forms of [`expr_inner`].
#[inline(never)]
fn expr_form(
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    match e {
        rs::RustExpr::Lit(lit) => literal_of(lit, want, false).map(|v| (v, acc)),
        rs::RustExpr::Path(p) => path_value(p, env, ctx, acc, want),
        rs::RustExpr::Unary(u) => unary_value(u, env, ctx, acc, want),
        rs::RustExpr::If(i) => if_value(i, env, ctx, acc, want),
        rs::RustExpr::Match(m) => match_value(m, env, ctx, acc, want),
        rs::RustExpr::Field(f) => field_value(f, env, ctx, acc),
        // `E::V { a, b }`: a struct variant, its fields in declaration order.
        rs::RustExpr::Struct(s)
            if s.rest.is_none() && !s.qself && data_variant(&s.path, &env.types).is_some() =>
        {
            variant_value(s, env, ctx, acc)
        }
        // `S { a: x, .. }`, or `S { a: x, ..base }` where `base` gives the
        // fields not written.
        rs::RustExpr::Struct(s) if !s.qself => struct_literal(s, env, ctx, acc),
        rs::RustExpr::Tuple(t) if t.is_empty() => flat_lift("()".into(), Ty::Unit, acc),
        rs::RustExpr::Tuple(t) if t.len() >= 2 => tuple_literal(t, env, ctx, acc, want),
        // `a..b`, `a..=b`: the naturals from `a`, as many as the range
        // holds; none where it is empty.
        rs::RustExpr::Other(o) if range_bounds(e).is_some() => range_value(o, e, env, ctx, acc),
        rs::RustExpr::Cast(c) => cast_value(c, env, ctx, acc),
        // `xs[i]`: the element, and where Rust's bounds check panics, the
        // type's default, as Lean's `xs[i]!` is.
        rs::RustExpr::Index(ix) => index_value(ix, env, ctx, acc),
        rs::RustExpr::Continue(label) => loop_exit(env, ctx, acc, *label, false),
        rs::RustExpr::Break(b) if b.expr.is_none() => loop_exit(env, ctx, acc, b.label, true),
        rs::RustExpr::Try(_) => Err(Residue::Form("? outside a let".to_string())),
        // A `return` in tail position: the function's value.
        rs::RustExpr::Return(Some(v)) if ctx.top => early_return(Some(v), env, ctx, acc),
        // `matches!(e, P)`: a match with `true` for `P` and `false` otherwise.
        rs::RustExpr::Macro(m) if m.path.is_ident("matches") => matches_value(m, env, ctx, acc),
        rs::RustExpr::Macro(m) if m.path.is_ident("vec") => {
            let Some(rs::MacroArgs { items }) = &m.args else {
                return Err(Residue::Form("vec! of a repeated value".to_string()));
            };
            list_of(items, env, ctx, acc, want)
        }
        rs::RustExpr::Array(items) => list_of(items, env, ctx, acc, want),
        _ => Err(Residue::Form(e.kind())),
    }
}

/// `!b`, `-lit` and `*e`.
fn unary_value(
    u: &rs::ExprUnary,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    match u.op {
        rs::UnOp::Not => {
            let ((x, t), acc) = flat_of(&u.expr, env, ctx, acc, Some(&Ty::Bool))?;
            if t != Ty::Bool {
                return Err(Residue::Form("bitwise not".to_string()));
            }
            flat_lift(format!("(not {x})"), Ty::Bool, acc)
        }
        rs::UnOp::Neg => match &*u.expr {
            rs::RustExpr::Lit(lit) => literal_of(lit, want, true).map(|v| (v, acc)),
            _ => Err(Residue::Arithmetic("-".to_string())),
        },
        rs::UnOp::Deref => expr(&u.expr, env, ctx, acc, want),
    }
}

/// A path: a local, a constant, `None`, or a unit variant.
fn path_value(
    p: &rs::Path,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    if let Some(ident) = p.get_ident() {
        if let Some((name, ty)) = ctx.locals.get(&ident.to_string()) {
            return flat_lift(name.clone(), ty.clone(), acc);
        }
    }
    // A constant: a definition of no parameters.
    if let Some(ident) = p.get_ident() {
        let key = ident.to_string();
        if let Some(sig) = env.fns.get(&key).filter(|s| s.params.is_empty()) {
            if key == ctx.current {
                return Err(Residue::Form("a constant that names itself".to_string()));
            }
            let acc = acc.called(key);
            return flat_lift(format!("({})", sig.name), sig.result.clone(), acc);
        }
    }
    if wrapper(p) == Some("None") {
        return match want {
            Some(t @ Ty::Option(inner)) => flat_lift(
                format!("(ctor Option.none [{}])", inner.render()),
                t.clone(),
                acc,
            ),
            _ => Err(Residue::Form("None of an unknown type".to_string())),
        };
    }
    match ctor(p, &env.types) {
        Some((owner, variant)) => {
            flat_lift(format!("(ctor {owner}.{variant})"), Ty::Named(owner), acc)
        }
        None => match p.segments.as_slice() {
            [owner, f] if owner.ident == "char" && unicode_property(&f.ident).is_some() => Err(
                Residue::Form(unicode_property(&f.ident).unwrap_or_default()),
            ),
            _ => Err(Residue::Form(format!("free name {}", p.text))),
        },
    }
}

/// `if c { a } else { b }` and `if let p = e { a } else { b }`.
fn if_value(
    i: &rs::ExprIf,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(otherwise) = &i.else_branch else {
        return Err(Residue::Form("if without else".to_string()));
    };
    if let rs::RustExpr::Let(l) = &*i.cond {
        let wild = rs::Pat::Wild;
        let arms = vec![
            (&l.pat, Body::Block(&i.then_branch)),
            (&wild, Body::Expr(otherwise)),
        ];
        return matching(&l.expr, &arms, env, ctx, acc, want);
    }
    let ((c, ct), acc) = flat_of(&i.cond, env, &ctx.below(), acc, Some(&Ty::Bool))?;
    if ct != Ty::Bool {
        return Err(Residue::Form("non-boolean condition".to_string()));
    }
    let yes = ctx.assuming(&i.cond, true, env, &acc);
    let no = ctx.assuming(&i.cond, false, env, &acc);
    let ((then, tt), (other, ot), acc) =
        match block(&i.then_branch.stmts, env, &yes, acc.clone(), want) {
            // A branch whose type only the other says: that one first.
            Err(r) if want.is_none() && untyped(&r) => {
                let ((other, ot), acc) = expr(otherwise, env, &no, acc, None)?;
                let (then, acc) = block(&i.then_branch.stmts, env, &yes, acc, Some(&ot))?;
                (then, (other, ot), acc)
            }
            lifted => {
                let ((then, tt), acc) = lifted?;
                let (other, acc) = expr(otherwise, env, &no, acc, Some(want.unwrap_or(&tt)))?;
                ((then, tt), other, acc)
            }
        };
    if !tt.same(&ot) {
        return Err(Residue::Form("branches of different types".to_string()));
    }
    Ok((
        (
            Expr::If {
                condition: Box::new(Expr::Flat(c)),
                then: Box::new(then),
                otherwise: Box::new(other),
            },
            tt,
        ),
        acc,
    ))
}

/// `match e { .. }`: the match compiler over its arms.
fn match_value(
    m: &rs::ExprMatch,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let arms: Vec<(&rs::Pat, Body<'_>)> = m
        .arms
        .iter()
        .map(|a| match &a.pat {
            rs::Pat::Guard(g) => (&*g.pat, Body::Guarded(&g.guard, &a.body)),
            p => (p, Body::Expr(&a.body)),
        })
        .collect();
    matching(&m.expr, &arms, env, ctx, acc, want)
}

/// `e.f` and `e.0`: a field of a structure or an item of a tuple.
fn field_value(f: &rs::ExprField, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let ((base, bt), acc) = flat_of(&f.base, env, ctx, acc, None)?;
    match (&f.member, &bt) {
        (rs::Member::Named(field), Ty::Named(s)) => {
            let name = local_name(field);
            let Some((_, ty)) = env
                .types
                .structs
                .get(s)
                .and_then(|fs| fs.iter().find(|(n, _)| *n == name))
            else {
                return Err(Residue::Form("field of an unknown struct".to_string()));
            };
            if env.types.boxed(s) {
                flat_lift(
                    format!("({} {base})", Types::accessor(s, &name)),
                    ty.clone(),
                    acc,
                )
            } else {
                flat_lift(format!("(project {base} {name})"), ty.clone(), acc)
            }
        }
        (rs::Member::Unnamed(i), Ty::Tuple(ts) | Ty::Pair(ts)) => match ts.get(*i as usize) {
            Some(ty) => flat_lift(tuple_item(&bt, &base, *i as usize), ty.clone(), acc),
            None => Err(Residue::Form("tuple field".to_string())),
        },
        _ => Err(Residue::Form("field access".to_string())),
    }
}

/// `E::V { a, b }`: a struct variant, its fields in declaration order.
fn variant_value(s: &rs::ExprStruct, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let Some((owner, variant, fields)) = data_variant(&s.path, &env.types) else {
        return Err(Residue::Form("struct variant".to_string()));
    };
    let mut given = BTreeMap::new();
    for fv in &s.fields {
        let rs::Member::Named(id) = &fv.member else {
            return Err(Residue::Form("struct variant field".to_string()));
        };
        given.insert(id.to_string(), &fv.expr);
    }
    if given.len() != fields.len() {
        return Err(Residue::Form("struct variant literal".to_string()));
    }
    let mut parts = vec![format!("ctor {owner}.{variant}")];
    let mut acc = acc;
    for (name, ty) in fields {
        let Some(value) = name.as_ref().and_then(|n| given.get(n)) else {
            return Err(Residue::Form("struct variant literal".to_string()));
        };
        let ((v, vt), next) = flat_of(value, env, ctx, acc, Some(ty))?;
        acc = next;
        if !vt.same(ty) {
            return Err(Residue::Form("struct variant field of another type".into()));
        }
        parts.push(v);
    }
    flat_lift(format!("({})", parts.join(" ")), Ty::Named(owner), acc)
}

/// `S { a: x, .. }`, or `S { a: x, ..base }` where `base` gives the
/// fields not written.
fn struct_literal(s: &rs::ExprStruct, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let Some(written) = s.path.segments.last().map(|seg| seg.ident.clone()) else {
        return Err(Residue::Form("struct path".to_string()));
    };
    let owner = ctx.current.split_once("::").map(|(o, _)| o.to_string());
    let name = match (written.as_str(), owner) {
        ("Self", Some(o)) => env.types.resolve(&o),
        _ => env.types.resolve(&written),
    };
    // `S`, `Self`, or `m::S` through modules of this crate, which
    // declares a type name once.
    let through_modules = s.path.segments.split_last().is_some_and(|(_, prefix)| {
        prefix
            .first()
            .is_some_and(|h| rs::crate_module(&h.ident, &ctx.imports))
            && prefix
                .iter()
                .all(|seg| seg.arguments == rs::PathArguments::None)
    });
    let local = s.path.segments.len() == 1 || written == "Self" || through_modules;
    let Some(fields) = env.types.structs.get(&name).filter(|_| local) else {
        return Err(Residue::Form(format!("struct {name}")));
    };
    let mut given = BTreeMap::new();
    for fv in &s.fields {
        let rs::Member::Named(id) = &fv.member else {
            return Err(Residue::Form("struct field".to_string()));
        };
        given.insert(local_name(id), &fv.expr);
    }
    let mut acc = acc;
    let base = match &s.rest {
        Some(b) => {
            let ((bv, bt), next) = flat_of(b, env, ctx, acc, Some(&Ty::Named(name.clone())))?;
            acc = next;
            if bt != Ty::Named(name.clone()) {
                return Err(Residue::Form("struct update of another type".to_string()));
            }
            Some(bv)
        }
        None if given.len() != fields.len() => {
            return Err(Residue::Form("struct literal".to_string()));
        }
        None => None,
    };
    let mut parts = Vec::new();
    for (field, ty) in fields {
        let v = match (given.get(field), &base) {
            (Some(value), _) => {
                let ((v, vt), next) = flat_of(value, env, ctx, acc, Some(ty))?;
                acc = next;
                if !vt.same(ty) {
                    return Err(Residue::Form("struct field of another type".to_string()));
                }
                v
            }
            (None, Some(b)) if env.types.boxed(&name) => {
                format!("({} {b})", Types::accessor(&name, field))
            }
            (None, Some(b)) => format!("(project {b} {field})"),
            (None, None) => return Err(Residue::Form("struct literal".to_string())),
        };
        parts.push((field, v));
    }
    flat_lift(
        struct_value(&name, &parts, &env.types),
        Ty::Named(name),
        acc,
    )
}

/// `(a, b, ..)`: a tuple of its items.
fn tuple_literal(
    t: &[rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let wants: Vec<Option<&Ty>> = match want {
        Some(Ty::Tuple(ts) | Ty::Pair(ts)) if ts.len() == t.len() => ts.iter().map(Some).collect(),
        _ => vec![None; t.len()],
    };
    let mut parts = Vec::new();
    let mut tys = Vec::new();
    let mut acc = acc;
    for (e, w) in t.iter().zip(wants) {
        let ((v, vt), next) = flat_of(e, env, ctx, acc, w)?;
        acc = next;
        parts.push(v);
        tys.push(vt);
    }
    let ty = env.types.tuple(tys);
    flat_lift(tuple_value(&ty, &parts), ty, acc)
}

/// `a..b`, `a..=b`: the naturals from `a`, as many as the range holds.
fn range_value(
    o: &rs::ExprOther,
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let Some((lo, hi, closed)) = range_bounds(e) else {
        return Err(Residue::Form(o.kind.clone()));
    };
    let ((l, lt), acc) = flat_of(lo, env, ctx, acc, Some(&Ty::Nat(None)))?;
    let ((h, ht), acc) = flat_of(hi, env, ctx, acc, Some(&lt))?;
    if !matches!(lt, Ty::Nat(_)) || !ht.same(&lt) {
        return Err(Residue::Form(
            "a range of something other than naturals".into(),
        ));
    }
    let end = if closed { format!("({h} + 1)") } else { h };
    let count = format!("(prim subtract {end} {l} : Nat)");
    let (list, acc) = acc.prelude("natRange", &[&l, &count]);
    flat_lift(list, Ty::List(Box::new(lt)), acc)
}

/// `e as T`.
fn cast_value(c: &rs::ExprCast, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let ((v, vt), acc) = flat_of(&c.expr, env, ctx, acc, None)?;
    let target = match env.types.ty(&c.ty.localized(&ctx.imports)) {
        Ok(t) => t,
        Err(_) => return Err(Residue::Form("cast".to_string())),
    };
    // `usize` is as wide as the narrowest declared target's pointer.
    let narrowest = env.widths.first().map(|(w, _)| *w);
    match (&vt, &target) {
        (Ty::Nat(Some(a)), Ty::Nat(Some(b))) if a <= b => flat_lift(v, target, acc),
        (Ty::Nat(Some(a)), Ty::Nat(None)) if narrowest.is_some_and(|w| *a <= w) => {
            flat_lift(v, target, acc)
        }
        // A cast to a narrower unsigned type keeps the low bits: the
        // value modulo 2^bits, whatever the source's width.
        (Ty::Nat(_), Ty::Nat(Some(b))) => flat_lift(
            format!("(prim remainder {v} {} 0 : Nat)", pow2(*b)),
            target,
            acc,
        ),
        (Ty::Int(a), Ty::Int(b)) if a <= b => flat_lift(v, target, acc),
        (Ty::Nat(_), Ty::Int(_)) => Err(Residue::Form(
            "a cast from an unsigned to a signed integer, where LexLean binds no \
             conversion from Nat to Int64"
                .to_string(),
        )),
        _ => Err(Residue::Form("narrowing or sign-changing cast".to_string())),
    }
}

/// `xs[i]`.
fn index_value(ix: &rs::ExprIndex, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let ((base, bt), acc) = flat_of(&ix.expr, env, ctx, acc, None)?;
    let Ty::List(elem) = &bt else {
        return Err(Residue::Form(format!("index of {}", bt.render())));
    };
    let ((i, it), acc) = flat_of(&ix.index, env, ctx, acc, Some(&Ty::Nat(None)))?;
    if !matches!(it, Ty::Nat(_)) {
        return Err(Residue::Form("index by a range".to_string()));
    }
    let Some(d) = default_of(elem, &env.types) else {
        return Err(Residue::Form("index of a type with no value".to_string()));
    };
    let (v, acc) = ctx.fresh(env, acc, "item");
    Ok((
        (
            Expr::Match {
                scrutinee: format!("(prim index {base} {i} : (Option {}))", elem.render()),
                arms: vec![
                    Arm {
                        pattern: "Option.none".into(),
                        body: Expr::Flat(d),
                    },
                    Arm {
                        pattern: format!("Option.some {v}"),
                        body: Expr::Flat(v),
                    },
                ],
            },
            (**elem).clone(),
        ),
        acc,
    ))
}

/// `matches!(e, P)`: a match with `true` for `P` and `false` otherwise.
fn matches_value(m: &rs::ExprMacro, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let Some(rs::MacroMatch {
        expr: scrutinee,
        pat,
    }) = &m.matches
    else {
        return Err(Residue::Form("matches! of another form".to_string()));
    };
    let yes = rs::RustExpr::Lit(rs::Lit::Bool(true));
    let no = rs::RustExpr::Lit(rs::Lit::Bool(false));
    let wild = rs::Pat::Wild;
    let first = match &**pat {
        rs::Pat::Guard(g) => (&*g.pat, Body::Guarded(&g.guard, &yes)),
        p => (p, Body::Expr(&yes)),
    };
    let arms = vec![first, (&wild, Body::Expr(&no))];
    matching(scrutinee, &arms, env, ctx, acc, Some(&Ty::Bool))
}

/// An array or `vec!` of items, as the list of them.
fn list_of(
    items: &[rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let mut elem = match want {
        Some(Ty::List(t) | Ty::Set(t, _)) => Some((**t).clone()),
        _ => None,
    };
    let mut parts = Vec::new();
    let mut acc = acc;
    for item in items {
        let ((v, vt), next) = flat_of(item, env, ctx, acc, elem.as_ref())?;
        acc = next;
        match &elem {
            Some(t) if !t.same(&vt) => {
                return Err(Residue::Form(
                    "a list of items of different types".to_string(),
                ))
            }
            Some(_) => {}
            None => elem = Some(vt),
        }
        parts.push(v);
    }
    let Some(elem) = elem else {
        return Err(Residue::Form(
            "an empty list of an unknown type".to_string(),
        ));
    };
    let mut list = format!("(nil {})", elem.render());
    for part in parts.iter().rev() {
        list = format!("(cons {part} {list})");
    }
    Ok(((Expr::Flat(list), Ty::List(Box::new(elem))), acc))
}

/// Whether a format hole names a value: an identifier, without a format
/// specification.
fn is_identifier(hole: &str) -> bool {
    let mut chars = hole.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && hole != "_"
}

/// `write!(f, ..)` into the formatter of a `Display` impl: the text the
/// same `format!` makes, which the impl's lifted function returns.
fn write_macro(mac: &rs::ExprMacro, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let into_formatter = |e: &rs::RustExpr| match (e, &ctx.formatter) {
        (rs::RustExpr::Path(p), Some(f)) => p.is_ident(f),
        _ => false,
    };
    match mac.args.as_ref().map(|a| a.items.as_slice()) {
        Some([f, rest @ ..]) if into_formatter(f) => {
            let text = rs::ExprMacro {
                path: mac.path.clone(),
                args: Some(rs::MacroArgs {
                    items: rest.to_vec(),
                }),
                matches: None,
            };
            format_macro(&text, env, ctx, acc)
        }
        _ => Err(Residue::Form(
            "write! into something other than the formatter".to_string(),
        )),
    }
}

/// `format!` whose holes are all strings: the pieces joined.
fn format_macro(mac: &rs::ExprMacro, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let bad = |what: &str| Err(Residue::Form(format!("format! {what}")));
    let Some(rs::MacroArgs { items: args }) = &mac.args else {
        return bad("arguments");
    };
    let mut args = args.iter();
    let Some(rs::RustExpr::Lit(rs::Lit::Str(template))) = args.next() else {
        return bad("template");
    };
    let mut positional = Vec::new();
    let mut named = BTreeMap::new();
    for arg in args {
        match arg {
            rs::RustExpr::Assign(a) => {
                let rs::RustExpr::Path(p) = &*a.left else {
                    return bad("argument");
                };
                let Some(id) = p.get_ident() else {
                    return bad("argument");
                };
                named.insert(id.to_string(), &*a.right);
            }
            other => positional.push(other),
        }
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut next = 0;
    let mut acc = acc;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let mut hole = String::new();
                for h in chars.by_ref() {
                    if h == '}' {
                        break;
                    }
                    hole.push(h);
                }
                // `{b:02x}`: a byte as two lowercase hex digits, the only
                // format spec the lift reads.
                let (hole, hex) = match hole.split_once(':') {
                    Some((name, "02x")) => (name.to_string(), true),
                    Some(_) => return bad("hole"),
                    None => (hole, false),
                };
                if !text.is_empty() {
                    let Some(piece) = exact_string(&text) else {
                        return bad("text");
                    };
                    pieces.push(piece);
                    text.clear();
                }
                let (value, after) = if hole.is_empty() {
                    let Some(e) = positional.get(next) else {
                        return bad("hole");
                    };
                    next += 1;
                    flat_of(e, env, ctx, acc, Some(&Ty::Str))?
                } else if let Some(e) = named.get(&hole) {
                    flat_of(e, env, ctx, acc, Some(&Ty::Str))?
                } else if let Some((name, ty)) = ctx.locals.get(&hole) {
                    ((name.clone(), ty.clone()), acc)
                } else if is_identifier(&hole) {
                    // A constant named in the hole.
                    let path = rs::RustExpr::Path(rs::Path::ident(&hole));
                    flat_of(&path, env, ctx, acc, Some(&Ty::Str))?
                } else {
                    return bad("hole");
                };
                acc = after;
                match value.1 {
                    Ty::Nat(Some(8)) if hex => {
                        let (piece, after) = acc.prelude("hexByte", &[&value.0]);
                        acc = after;
                        pieces.push(piece);
                    }
                    _ if hex => return bad("hex of another type"),
                    Ty::Str | Ty::Char => pieces.push(value.0),
                    Ty::Bool => {
                        pieces.push(format!("(if {} then \"true\" else \"false\")", value.0))
                    }
                    Ty::Nat(_) => {
                        // A natural has as many digits as steps of division
                        // by ten, never more than itself and one: the fuel.
                        let (piece, after) = acc.prelude("natDecimal", &[&value.0]);
                        acc = after;
                        pieces.push(piece);
                    }
                    Ty::Int(_) => {
                        acc = acc.all_axioms();
                        pieces.push(format!("(prim format_decimal {} : String)", value.0));
                    }
                    _ => return bad("hole of another type"),
                }
            }
            '}' => return bad("template"),
            c => text.push(c),
        }
    }
    if !text.is_empty() {
        let Some(piece) = exact_string(&text) else {
            return bad("text");
        };
        pieces.push(piece);
    }
    // Joining strings elaborates through propositional extensionality.
    let acc = acc.axiom("propext");
    let mut list = "(nil String)".to_string();
    for piece in pieces.iter().rev() {
        list = format!("(cons {piece} {list})");
    }
    Ok((
        (
            Expr::Flat(format!("(prim join {list} \"\" : String)")),
            Ty::Str,
        ),
        acc,
    ))
}

/// A comparison `==`, `!=`, `<`, `<=`, `>` or `>=`, apart from [`binary`]
/// so the frame of each stays small where expressions nest deeply.
#[inline(never)]
fn comparison(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    use rs::BinOp as B;
    let flat = |s: String, t: Ty, acc: Acc| Ok(((Expr::Flat(s), t), acc));
    let op_text = || b.op.text().to_string();
    let ((l, lt), (r, _), acc) = if is_literal(&b.left) {
        let (right, acc) = flat_of(&b.right, env, ctx, acc, None)?;
        let (left, acc) = flat_of(&b.left, env, ctx, acc, Some(&right.1))?;
        (left, right, acc)
    } else {
        let (left, acc) = flat_of(&b.left, env, ctx, acc, None)?;
        let (right, acc) = flat_of(&b.right, env, ctx, acc, Some(&left.1))?;
        (left, right, acc)
    };
    let (text, acc) = match (&lt, b.op) {
        (Ty::Nat(_), B::Eq) => (format!("({l} =? {r})"), acc),
        (Ty::Nat(_), B::Ne) => (format!("(not ({l} =? {r}))"), acc),
        (Ty::Nat(_), B::Lt) => (format!("({l} <? {r})"), acc),
        (Ty::Nat(_), B::Le) => (format!("({l} <=? {r})"), acc),
        (Ty::Nat(_), B::Gt) => (format!("({r} <? {l})"), acc),
        (Ty::Nat(_), B::Ge) => (format!("({r} <=? {l})"), acc),
        (Ty::Bool | Ty::Named(_), B::Eq) if is_enum_or_bool(&lt, env) => {
            (same_constructor(&lt, &l, &r), acc)
        }
        (Ty::Bool | Ty::Named(_), B::Ne) if is_enum_or_bool(&lt, env) => {
            (format!("(not {})", same_constructor(&lt, &l, &r)), acc)
        }
        (Ty::Int(_) | Ty::Str | Ty::Char, B::Eq) => (format!("(prim equal {l} {r} : Bool)"), acc),
        (Ty::Int(_) | Ty::Str | Ty::Char, B::Ne) => {
            (format!("(not (prim equal {l} {r} : Bool))"), acc)
        }
        // Strings order by their scalar values, as Rust's `str` and
        // `char` do.
        (Ty::Int(_) | Ty::Str | Ty::Char, B::Lt | B::Gt | B::Le | B::Ge) => {
            // Int64's order is decided through propositional
            // extensionality; a string's through its scalars.
            let acc = acc.axiom("propext");
            let acc = if matches!(lt, Ty::Int(_)) {
                acc
            } else {
                acc.all_axioms()
            };
            let (x, y) = match b.op {
                B::Lt | B::Ge => (&l, &r),
                _ => (&r, &l),
            };
            let less = format!("(prim less_than {x} {y} : Bool)");
            if matches!(b.op, B::Lt | B::Gt) {
                (less, acc)
            } else {
                (format!("(not {less})"), acc)
            }
        }
        (Ty::Option(inner), B::Eq | B::Ne) => {
            let (x, acc) = ctx.fresh(env, acc, "left");
            let (y, acc) = ctx.fresh(env, acc, "right");
            let Some((same, acc)) = same_value(inner, &x, &y, env, acc) else {
                return Err(Residue::Form(format!(
                    "comparison {} of {}",
                    op_text(),
                    lt.render()
                )));
            };
            let eq = matches!(b.op, B::Eq);
            let (yes, no) = if eq {
                ("true", "false")
            } else {
                ("false", "true")
            };
            let same = if eq { same } else { format!("(not {same})") };
            let none_r = Expr::Match {
                scrutinee: r.clone(),
                arms: vec![
                    Arm {
                        pattern: "Option.none".into(),
                        body: Expr::Flat(yes.into()),
                    },
                    Arm {
                        pattern: format!("Option.some {y}"),
                        body: Expr::Flat(no.into()),
                    },
                ],
            };
            let some_r = Expr::Match {
                scrutinee: r.clone(),
                arms: vec![
                    Arm {
                        pattern: "Option.none".into(),
                        body: Expr::Flat(no.into()),
                    },
                    Arm {
                        pattern: format!("Option.some {y}"),
                        body: Expr::Flat(same),
                    },
                ],
            };
            return Ok((
                (
                    Expr::Match {
                        scrutinee: l.clone(),
                        arms: vec![
                            Arm {
                                pattern: "Option.none".into(),
                                body: none_r,
                            },
                            Arm {
                                pattern: format!("Option.some {x}"),
                                body: some_r,
                            },
                        ],
                    },
                    Ty::Bool,
                ),
                acc,
            ));
        }
        // Any other type compares by its derived order.
        _ => match crate::order::operator(&op_text(), &lt, &l, &r) {
            Some(test) if crate::order::derived(&lt, &env.types) => {
                (test, acc.ordered(&lt, &env.types))
            }
            _ => {
                return Err(Residue::Form(format!(
                    "comparison {} of {}",
                    op_text(),
                    lt.render()
                )))
            }
        },
    };
    flat(text, Ty::Bool, acc)
}

fn binary(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    use rs::BinOp as B;
    match b.op {
        B::And | B::Or => logic(b, env, ctx, acc),
        B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge => comparison(b, env, ctx, acc),
        // `s + t` of a `String` and a `&str`: the two joined.
        B::Add if text_operand(&b.left, env, ctx, &acc) => text_join(b, env, ctx, acc),
        B::Add => nat_sum(b, env, ctx, acc),
        // `a - k` where the conditions in scope bound `a` below by `k` or
        // more: it does not underflow, so it is the natural difference.
        B::Sub => nat_difference(b, env, ctx, acc),
        B::Div | B::Rem => nat_division(b, env, ctx, acc),
        _ => Err(Residue::Arithmetic(b.op.text().to_string())),
    }
}

/// `a && b` and `a || b`.
fn logic(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, Some(&Ty::Bool))?;
    let ((r, rt), acc) = flat_of(&b.right, env, ctx, acc, Some(&Ty::Bool))?;
    if lt != Ty::Bool || rt != Ty::Bool {
        return Err(Residue::Form("logic on non-booleans".to_string()));
    }
    let op = if matches!(b.op, rs::BinOp::And) {
        "&&"
    } else {
        "||"
    };
    flat_lift(format!("({l} {op} {r})"), Ty::Bool, acc)
}

/// `s + t` of a `String` and a `&str`: the two joined.
fn text_join(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let ((l, _), acc) = flat_of(&b.left, env, ctx, acc, Some(&Ty::Str))?;
    let ((r, rt), acc) = flat_of(&b.right, env, ctx, acc, Some(&Ty::Str))?;
    if rt != Ty::Str {
        return Err(Residue::Form("+ of a string and another type".to_string()));
    }
    let acc = acc.axiom("propext");
    flat_lift(
        format!("(prim join (cons {l} (cons {r} (nil String))) \"\" : String)"),
        Ty::Str,
        acc,
    )
}

/// `a + b` of naturals the conditions in scope keep in range.
fn nat_sum(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let want = Ty::Nat(None);
    let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, Some(&want))?;
    let ((r, rt), acc) = flat_of(&b.right, env, ctx, acc, Some(&want))?;
    if lt != want || rt != want {
        return Err(Residue::Arithmetic(b.op.text().to_string()));
    }
    flat_lift(
        usize_sum(&b.left, &b.right, &l, &r, env, &ctx.capped)?,
        want,
        acc,
    )
}

/// `a - k` where the conditions in scope bound `a` below by `k` or
/// more: it does not underflow, so it is the natural difference.
fn nat_difference(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, Some(&Ty::Nat(None)))?;
    match (&lt, int_literal(&b.right)) {
        (Ty::Nat(_), Some(k)) if ctx.bounds.get(&l).is_some_and(|m| *m >= k) => {
            flat_lift(format!("(prim subtract {l} {k} : Nat)"), lt, acc)
        }
        _ => Err(Residue::Arithmetic(b.op.text().to_string())),
    }
}

/// `a / k` and `a % k` by a divisor that is not zero.
fn nat_division(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, None)?;
    match (&lt, int_literal(&b.right)) {
        (Ty::Nat(_), Some(k)) if k > 0 => {
            let prim = if matches!(b.op, rs::BinOp::Div) {
                "quotient"
            } else {
                "remainder"
            };
            flat_lift(format!("(prim {prim} {l} {k} 0 : Nat)"), lt, acc)
        }
        // A divisor the conditions in scope bound away from zero.
        (Ty::Nat(_), None) => {
            let ((r, rt), acc) = flat_of(&b.right, env, ctx, acc, Some(&lt))?;
            if !rt.same(&lt) || !ctx.bounds.get(&r).is_some_and(|m| *m > 0) {
                return Err(Residue::Arithmetic(b.op.text().to_string()));
            }
            let prim = if matches!(b.op, rs::BinOp::Div) {
                "quotient"
            } else {
                "remainder"
            };
            flat_lift(format!("(prim {prim} {l} {r} 0 : Nat)"), lt, acc)
        }
        _ => Err(Residue::Arithmetic(b.op.text().to_string())),
    }
}

/// Whether an operand lifts to text: the left side of a `String` `+`.
fn text_operand(e: &rs::RustExpr, env: &Env, ctx: &Ctx, acc: &Acc) -> bool {
    matches!(
        flat_of(e, env, ctx, acc.clone(), None),
        Ok(((_, Ty::Str), _))
    )
}

/// Equality of two values of a unit-variant enum, through the enum's own
/// equality definition (`=?` is `Nat`'s), or of two `Bool`s, by cases on the
/// left.
fn same_constructor(ty: &Ty, l: &str, r: &str) -> String {
    match ty {
        Ty::Named(n) => format!("({} {l} {r})", equality_name(n)),
        _ => format!("(if {l} then {r} else (not {r}))"),
    }
}

/// The definition that decides equality of a unit-variant enum's values,
/// which the projection emits after the enum for every enum a lifted body
/// compares.
pub(crate) fn equality_name(enum_name: &str) -> String {
    #[cfg(not(uorust_lift))]
    crate::certified::certified!(lift_equality_name, enum_name);
    format!("equal{enum_name}")
}

fn is_enum_or_bool(t: &Ty, env: &Env) -> bool {
    match t {
        Ty::Bool => true,
        Ty::Named(n) => env.types.enums.contains_key(n),
        _ => false,
    }
}

/// A call of a closure a `let` bound: its body, in the scope it was
/// written in, with each parameter bound to its argument.
fn applied_closure(
    closure: &Closure,
    args: &[rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    if closure.inputs.len() != args.len() {
        return Err(Residue::Form("a closure call of another arity".to_string()));
    }
    let mut scope = ctx.clone();
    scope.locals = closure.locals.clone();
    // The binders of the call's scope stay taken, so a parameter's binder
    // does not shadow one an argument names.
    scope
        .hidden
        .extend(ctx.locals.values().map(|(name, _)| name.clone()));
    let mut lets = Vec::new();
    let mut acc = acc;
    for (input, arg) in closure.inputs.iter().zip(args) {
        let (pat, declared) = match input {
            rs::Pat::Type(pt) => (
                &*pt.pat,
                Some(env.types.ty(&pt.ty.localized(&ctx.imports))?),
            ),
            p => (p, None),
        };
        let ((value, ty), after) = flat_of(arg, env, &ctx.below(), acc, declared.as_ref())?;
        acc = after;
        if declared.as_ref().is_some_and(|d| !d.same(&ty)) {
            return Err(Residue::Form(
                "a closure argument of another type".to_string(),
            ));
        }
        let rust = match pat {
            rs::Pat::Ident(i) if i.subpat.is_none() => i.ident.clone(),
            rs::Pat::Reference(r) => match &**r {
                rs::Pat::Ident(i) if i.subpat.is_none() => i.ident.clone(),
                _ => return Err(Residue::Form("closure pattern".to_string())),
            },
            rs::Pat::Wild => continue,
            _ => return Err(Residue::Form("closure pattern".to_string())),
        };
        let (next, name, after) = scope.bind(env, acc, &rust, ty.clone());
        scope = next;
        acc = after;
        lets.push((name, ty.render(), value));
    }
    let ((e, t), acc) = expr(&closure.body, env, &scope.below(), acc, want)?;
    Ok(((wrap_lets(lets, e), t), acc))
}

fn call(
    c: &rs::ExprCall,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let rs::RustExpr::Path(p) = &*c.func else {
        return Err(Residue::Form("call of an expression".to_string()));
    };
    if let Some(closure) = p.get_ident().and_then(|i| ctx.closures.get(i)) {
        return applied_closure(closure, &c.args, env, ctx, acc, want);
    }
    // `f(x)` of a function parameter: its application.
    if let Some((f, Ty::Fn(inputs, output))) = p.get_ident().and_then(|i| ctx.locals.get(i)) {
        return applied_param(c, f, inputs, output, env, ctx, acc);
    }
    if let (Some(w), [arg]) = (wrapper(p), c.args.as_slice()) {
        return wrapped_value(w, arg, env, ctx, acc, want);
    }
    // `E::V(a, b)`: a variant of a data-carrying enum.
    if let Some((owner, variant, fields)) = data_variant(p, &env.types) {
        return variant_call(c, owner, &variant, fields, env, ctx, acc);
    }
    path_call(c, p, env, ctx, acc, want)
}

/// `f(x)` of a function parameter `f`: its application.
fn applied_param(
    c: &rs::ExprCall,
    f: &str,
    inputs: &[Ty],
    output: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    if inputs.len() != c.args.len() {
        return Err(Residue::Form(
            "a function applied to another arity".to_string(),
        ));
    }
    let mut parts = vec!["apply".to_string(), f.to_string()];
    let mut acc = acc;
    for (arg, ty) in c.args.iter().zip(inputs) {
        let ((v, vt), next) = flat_of(arg, env, &ctx.below(), acc, Some(ty))?;
        if !vt.same(ty) {
            return Err(Residue::Form(
                "a function applied to another type".to_string(),
            ));
        }
        acc = next;
        parts.push(v);
    }
    flat_lift(format!("({})", parts.join(" ")), output.clone(), acc)
}

/// `Some(v)`, `Ok(v)` and `Err(e)`.
fn wrapped_value(
    w: &str,
    arg: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    match (w, want) {
        ("Some", _) => {
            let inner = match want {
                Some(Ty::Option(t)) => Some(&**t),
                _ => None,
            };
            let ((v, vt), acc) = flat_of(arg, env, ctx, acc, inner)?;
            flat_lift(
                format!("(ctor Option.some [{}] {v})", vt.render()),
                Ty::Option(Box::new(vt)),
                acc,
            )
        }
        ("Ok", Some(t @ Ty::Outcome(ok, err))) | ("Err", Some(t @ Ty::Outcome(ok, err))) => {
            let (ctor, inner) = if w == "Ok" {
                ("ok", &**ok)
            } else {
                ("error", &**err)
            };
            let ((v, vt), acc) = flat_of(arg, env, ctx, acc, Some(inner))?;
            if !vt.same(inner) {
                return Err(Residue::Form("result of another type".to_string()));
            }
            flat_lift(
                format!(
                    "(ctor Result.{ctor} [{}, {}] {v})",
                    ok.render(),
                    err.render()
                ),
                t.clone(),
                acc,
            )
        }
        _ => Err(Residue::Form("result of an unknown type".to_string())),
    }
}

/// `E::V(a, b)`: a variant of a data-carrying enum.
fn variant_call(
    c: &rs::ExprCall,
    owner: String,
    variant: &str,
    fields: &[DataField],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    if fields.len() != c.args.len() {
        return Err(Residue::Form(format!(
            "{owner}::{variant} of another arity"
        )));
    }
    let mut parts = vec![format!("ctor {owner}.{variant}")];
    let mut acc = acc;
    for (arg, (_, ty)) in c.args.iter().zip(fields) {
        let ((v, vt), next) = flat_of(arg, env, ctx, acc, Some(ty))?;
        acc = next;
        if !vt.same(ty) {
            return Err(Residue::Form(format!("{owner}::{variant} of another type")));
        }
        parts.push(v);
    }
    flat_lift(format!("({})", parts.join(" ")), Ty::Named(owner), acc)
}

/// A call of a path: a constructor of the standard library the lift
/// knows, or a lifted function.
fn path_call(
    c: &rs::ExprCall,
    p: &rs::Path,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let segments: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
    let call_args: Vec<&rs::RustExpr> = c.args.iter().collect();
    // `Sha256::digest(bytes)`: the SHA-256 of a byte list, which the prelude
    // computes.
    if let ([.., owner, method], [bytes]) = (segments.as_slice(), call_args.as_slice()) {
        let byte_list = Ty::List(Box::new(Ty::Nat(Some(8))));
        let named = match segments.len() {
            2 => ctx.imports.get(owner).cloned(),
            _ => Some(segments[..segments.len() - 1].join("::")),
        };
        if method == "digest" && named.as_deref() == Some("sha2::Sha256") {
            let ((v, vt), acc) = flat_of(bytes, env, ctx, acc, Some(&byte_list))?;
            if vt != byte_list {
                return Err(Residue::Form(
                    "a digest of something other than bytes".into(),
                ));
            }
            let (s, acc) = acc.prelude("sha256", &[&v]);
            return flat_lift(s, byte_list, acc);
        }
    }
    let (known, acc) = std_call(&segments, &call_args, env, ctx, acc, want)?;
    if let Some(lifted) = known {
        return Ok((lifted, acc));
    }
    fn_call(c, &segments, env, ctx, acc)
}

/// A constructor of the standard library the lift knows, by its path's
/// segments: `String::new()`, `Vec::new()`, `Box::new(v)` and the like.
fn std_call(
    segments: &[String],
    call_args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    match (segments, call_args, want) {
        ([s, n], [], _) if s == "String" && n == "new" => flat_some("\"\"".into(), Ty::Str, acc),
        ([s, n], [], Some(t @ Ty::List(inner))) if s == "Vec" && n == "new" => {
            flat_some(format!("(nil {})", inner.render()), t.clone(), acc)
        }
        // An empty ordered set or map: the empty list of its members or of
        // its entries, which `insert` keeps sorted and free of duplicates.
        ([s, n], [], Some(t @ Ty::Set(inner, true))) if s == "BTreeSet" && n == "new" => {
            flat_some(format!("(nil {})", inner.render()), t.clone(), acc)
        }
        ([s, n], [], Some(t @ Ty::Map(_, _, true))) if s == "BTreeMap" && n == "new" => {
            flat_some(format!("(nil {})", t.entry().render()), t.clone(), acc)
        }
        // `std::slice::from_ref(&x)`: the list of the one value.
        ([.., s, n], [a], _) if s == "slice" && n == "from_ref" => {
            let elem = match want {
                Some(Ty::List(t)) => Some(&**t),
                _ => None,
            };
            let ((v, vt), acc) = flat_of(a, env, ctx, acc, elem)?;
            let list = Ty::List(Box::new(vt.clone()));
            flat_some(format!("(cons {v} (nil {}))", vt.render()), list, acc)
        }
        // A capacity is a hint to the allocator: the value is the empty list.
        ([s, n], [_], Some(t @ Ty::List(inner))) if s == "Vec" && n == "with_capacity" => {
            flat_some(format!("(nil {})", inner.render()), t.clone(), acc)
        }
        // A box is its value: LexLean holds every value by reference.
        // `BTreeSet::from([..])`: each member inserted in turn, from empty.
        ([s, n], [items @ rs::RustExpr::Array(_)], _) if s == "BTreeSet" && n == "from" => {
            let ((v, vt), acc) = flat_of(items, env, ctx, acc, want)?;
            let Ty::List(elem) = &vt else {
                return Err(Residue::Form("BTreeSet::from of another type".into()));
            };
            let set = Ty::Set(elem.clone(), true);
            let (insert, acc) = sorted_insert(&set, env, ctx, acc)?;
            let (helper, acc) = insert_all(&set, &vt, &insert, env, ctx, acc);
            flat_some(format!("({helper} {v} (nil {}))", elem.render()), set, acc)
        }
        // `Path::new(s)`, `PathBuf::from(s)`: the path the text names.
        ([s, n], [a], _) | ([_, _, s, n], [a], _)
            if (s == "Path" && n == "new" || s == "PathBuf" && n == "from")
                && (segments.len() == 2 || segments[..2] == ["std", "path"])
                && !env.types.structs.contains_key(s.as_str()) =>
        {
            let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(&Ty::Str))?;
            if vt != Ty::Str {
                return Err(Residue::Form("a path of something other than text".into()));
            }
            let (path, acc) = acc.prelude("pathOf", &[&v]);
            flat_some(path, Ty::Path, acc)
        }
        ([s, n], [a], _) if s == "Box" && n == "new" => {
            let ((v, vt), acc) = flat_of(a, env, ctx, acc, want)?;
            flat_some(v, vt, acc)
        }
        ([s, n], [a], _) if s == "String" && n == "from" => {
            let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(&Ty::Str))?;
            if vt == Ty::Str {
                return flat_some(v, Ty::Str, acc);
            }
            Err(Residue::Form("String::from of another type".to_string()))
        }
        _ => Ok((None, acc)),
    }
}

/// A call of a lifted function, or of the function itself where the call
/// descends.
fn fn_call(
    c: &rs::ExprCall,
    segments: &[String],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let mut name = segments.last().cloned().unwrap_or_default();
    if let [.., owner, method] = segments {
        let owner = if owner == "Self" {
            ctx.current
                .split_once("::")
                .map_or(owner.as_str(), |(o, _)| o)
        } else {
            owner.as_str()
        };
        let key = format!("{}::{method}", env.types.resolve(owner));
        if env.fns.contains_key(&key) {
            name = key;
        }
    }
    let Some(sig) = env.fns.get(&name) else {
        return Err(Residue::Call(name));
    };
    if sig.params.len() != c.args.len() {
        return Err(Residue::Call(name));
    }
    let mut parts = vec![sig.name.clone()];
    let mut descends = None;
    let mut acc = acc;
    for (i, (arg, ty)) in c.args.iter().zip(&sig.params).enumerate() {
        let ((v, vt), next) = flat_of(arg, env, ctx, acc, Some(ty))?;
        acc = next;
        if !vt.same(ty) {
            return Err(Residue::Form(format!("argument of another type to {name}")));
        }
        if name == ctx.current && ctx.tails.get(&v) == ctx.params.get(i) && descends.is_none() {
            descends = Some(i);
        }
        parts.push(v);
    }
    let acc = if name == ctx.current {
        let Some(i) = descends else {
            return Err(Residue::Form(
                "recursion that does not descend on a slice".to_string(),
            ));
        };
        acc.descended(i)
    } else {
        acc.called(name)
    };
    flat_lift(format!("({})", parts.join(" ")), sig.result.clone(), acc)
}

/// The module of the byte-level text prelude every projection carries.
pub(crate) const PRELUDE: &str = "Prelude";

/// The prelude's source: the definitions string methods lift to. Its SHA-256
/// is ported from UOR-Foundation/compression, `src/Uorc/Integrity/Sha256.lex.tex`,
/// under the Apache License 2.0: the round functions, constants and word
/// loading are that module's, renamed; the padding, block walk and digest
/// read a whole message at once.
pub(crate) const PRELUDE_SOURCE: &str = include_str!("../assets/project/src/Prelude.lex.tex");

/// The prelude definitions the kernel cannot compute by unfolding: those
/// that reach a primitive Lean implements opaquely, directly or through
/// another prelude definition.
fn prelude_opaque() -> BTreeSet<String> {
    let decls: Vec<(&str, &str)> = PRELUDE_SOURCE
        .split("\ndefinition{\n  name: ")
        .skip(1)
        .filter_map(|decl| {
            let (name, rest) = decl.split_once(",\n")?;
            Some((name, rest.split("\n}\n").next().unwrap_or(rest)))
        })
        .collect();
    let mut opaque: BTreeSet<String> = decls
        .iter()
        .filter(|(_, body)| KERNEL_OPAQUE.iter().any(|p| body.contains(p)))
        .map(|(name, _)| (*name).to_string())
        .collect();
    loop {
        let more: Vec<String> = decls
            .iter()
            .filter(|(name, body)| {
                !opaque.contains(*name)
                    && body
                        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .any(|token| opaque.contains(token))
            })
            .map(|(name, _)| (*name).to_string())
            .collect();
        if more.is_empty() {
            return opaque;
        }
        opaque.extend(more);
    }
}

/// The prelude definitions a lifted body calls.
fn prelude_calls(text: &str) -> Vec<String> {
    let marker = format!("{PRELUDE}::");
    text.split(marker.as_str())
        .skip(1)
        .map(|rest| {
            rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

/// The Lean axioms a prelude definition elaborates with, as its source
/// declares them.
fn prelude_axioms(name: &str) -> Vec<String> {
    let Some(start) = PRELUDE_SOURCE.find(&format!("  name: {name},\n")) else {
        return Vec::new();
    };
    let decl = &PRELUDE_SOURCE[start..];
    let decl = &decl[..decl.find("\n}\n").unwrap_or(decl.len())];
    decl.find("  axioms: [")
        .and_then(|i| {
            let list = &decl[i + "  axioms: [".len()..];
            list.find(']').map(|j| &list[..j])
        })
        .map(|list| {
            list.split(',')
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The byte list `bytes` of `bytes.iter().map(|b| format!("{b:02x}"))`.
fn hex_of(e: &rs::RustExpr) -> Option<&rs::RustExpr> {
    let rs::RustExpr::MethodCall(map) = e else {
        return None;
    };
    let [rs::RustExpr::Closure(f)] = map.args.as_slice() else {
        return None;
    };
    let rs::RustExpr::MethodCall(iter) = &*map.receiver else {
        return None;
    };
    if map.method != "map" || iter.method != "iter" || !iter.args.is_empty() {
        return None;
    }
    let [rs::Pat::Ident(b)] = f.inputs.as_slice() else {
        return None;
    };
    let rs::RustExpr::Macro(mac) = &*f.body else {
        return None;
    };
    let hole = format!("{{{}:02x}}", b.ident);
    let named = match mac.args.as_ref().map(|a| a.items.as_slice()) {
        Some([rs::RustExpr::Lit(rs::Lit::Str(t))]) => *t == hole,
        Some([rs::RustExpr::Lit(rs::Lit::Str(t)), rs::RustExpr::Path(p)]) => {
            t == "{:02x}" && p.is_ident(&b.ident)
        }
        _ => false,
    };
    (mac.path.is_ident("format") && named && b.subpat.is_none() && !b.by_ref)
        .then_some(&*iter.receiver)
}

/// The methods of `str` and `char` over the prelude: `None` when the method
/// is none of them.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn text_method(
    m: &rs::ExprMethodCall,
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let flat = |s: String, t: Ty, acc: Acc| Ok((Some((Expr::Flat(s), t)), acc));
    // A pattern: a string or a character.
    let pattern = |a: &rs::RustExpr, acc: Acc| -> Lift<(String, Acc)> {
        let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(&Ty::Str))?;
        match vt {
            Ty::Str | Ty::Char => Ok((v, acc)),
            _ => Err(Residue::Form(format!("a .{name} pattern that is not text"))),
        }
    };
    let opt = |t: Ty| Ty::Option(Box::new(t));
    let list = |t: Ty| Ty::List(Box::new(t));
    let chars = rt == &Ty::Char;
    match (name, args) {
        ("len", []) if !chars => {
            let (s, acc) = acc.prelude("textLength", &[r]);
            flat(s, Ty::Nat(None), acc)
        }
        ("as_str" | "as_ref" | "into" | "to_owned" | "borrow", []) => {
            flat(r.to_string(), rt.clone(), acc)
        }
        ("to_string", []) => flat(r.to_string(), Ty::Str, acc),
        ("trim", []) => {
            let (s, acc) = acc.prelude("textTrim", &[r]);
            flat(s, Ty::Str, acc)
        }
        ("trim_start", []) => {
            let (s, acc) = acc.prelude("textTrimStart", &[r]);
            flat(s, Ty::Str, acc)
        }
        ("trim_end", []) => {
            let (s, acc) = acc.prelude("textTrimEnd", &[r]);
            flat(s, Ty::Str, acc)
        }
        // A predicate on the first or last character.
        ("starts_with" | "ends_with", [f]) if !chars && matches!(f, rs::RustExpr::Closure(_)) => {
            let (lam, acc) = lambda(f, &[Ty::Char], env, ctx, acc, Some(&Ty::Bool))?;
            if lam.ty != Ty::Bool {
                return Err(Residue::Form(format!(
                    "a .{name} predicate of another type"
                )));
            }
            let acc = acc.called_all(lam.calls.clone());
            let acc = acc.axioms_all(lam.axioms.clone());
            let (all, acc) = acc.prelude("textChars", &[r]);
            let at = if name == "starts_with" {
                "0".to_string()
            } else {
                format!("(prim subtract (prim length {all} : Nat) 1 : Nat)")
            };
            Ok((
                Some((
                    Expr::Match {
                        scrutinee: format!("(prim index {all} {at} : (Option String))"),
                        arms: vec![
                            Arm {
                                pattern: "Option.none".into(),
                                body: Expr::Flat("false".into()),
                            },
                            Arm {
                                pattern: format!("Option.some {}", lam.binders[0]),
                                body: lam.wrap(lam.core.clone()),
                            },
                        ],
                    },
                    Ty::Bool,
                )),
                acc,
            ))
        }
        ("starts_with" | "ends_with" | "contains", [p]) if !chars => {
            let (p, acc) = pattern(p, acc)?;
            let f = match name {
                "starts_with" => "textStartsWith",
                "ends_with" => "textEndsWith",
                _ => "textContains",
            };
            {
                let (s, acc) = acc.prelude(f, &[r, &p]);
                flat(s, Ty::Bool, acc)
            }
        }
        ("strip_prefix" | "strip_suffix", [p]) if !chars => {
            let (p, acc) = pattern(p, acc)?;
            let f = if name == "strip_prefix" {
                "textStripPrefix"
            } else {
                "textStripSuffix"
            };
            {
                let (s, acc) = acc.prelude(f, &[r, &p]);
                flat(s, opt(Ty::Str), acc)
            }
        }
        ("find" | "rfind", [p]) if !chars => {
            let (p, acc) = pattern(p, acc)?;
            let f = if name == "find" {
                "textFirst"
            } else {
                "textLast"
            };
            {
                let (s, acc) = acc.prelude(f, &[r, &p]);
                flat(s, opt(Ty::Nat(None)), acc)
            }
        }
        ("split" | "rsplit", [p]) if !chars => {
            let (p, acc) = pattern(p, acc)?;
            let f = if name == "split" {
                "textSplit"
            } else {
                "textRsplit"
            };
            {
                let (s, acc) = acc.prelude(f, &[r, &p]);
                flat(s, list(Ty::Str), acc)
            }
        }
        ("split_once" | "rsplit_once", [p]) if !chars => {
            let (p, acc) = pattern(p, acc)?;
            let f = if name == "split_once" {
                "textFirst"
            } else {
                "textLast"
            };
            let pair = Ty::Tuple(vec![Ty::Str, Ty::Str]);
            let acc = acc.saw(&pair);
            let (at, acc) = ctx.fresh(env, acc, "at");
            let (whole, acc) = acc.prelude("textLength", &[r]);
            let (width, acc) = acc.prelude("textLength", &[&p]);
            let (before, acc) = acc.prelude("textSlice", &[r, "0", &at]);
            let (after, acc) = acc.prelude("textSlice", &[r, &format!("({at} + {width})"), &whole]);
            let (scrutinee, acc) = acc.prelude(f, &[r, &p]);
            let body = Expr::Match {
                scrutinee,
                arms: vec![Arm { pattern: "Option.none".into(), body: Expr::Flat(format!("(ctor Option.none [{}])", pair.render())) }, Arm { pattern: format!("Option.some {at}"), body: Expr::Flat(format!(
                            "(ctor Option.some [{0}] (record {0} {{item0 := {before}, item1 := {after}}}))",
                            pair.render()
                        )) }],
            };
            Ok((Some((body, opt(pair))), acc))
        }
        ("replace", [from, to]) if !chars => {
            let (from, acc) = pattern(from, acc)?;
            let ((to, tt), acc) = flat_of(to, env, ctx, acc, Some(&Ty::Str))?;
            if !matches!(tt, Ty::Str | Ty::Char) {
                return Err(Residue::Form("a replacement that is not text".into()));
            }
            {
                let (s, acc) = acc.prelude("textReplace", &[r, &from, &to]);
                flat(s, Ty::Str, acc)
            }
        }
        ("trim_start_matches" | "trim_end_matches" | "trim_matches", [p]) if !chars => {
            let (p, acc) = pattern(p, acc)?;
            let f = match name {
                "trim_start_matches" => "textTrimStartMatches",
                "trim_end_matches" => "textTrimEndMatches",
                _ => "textTrimMatches",
            };
            {
                let (s, acc) = acc.prelude(f, &[r, &p]);
                flat(s, Ty::Str, acc)
            }
        }
        ("lines", []) if !chars => {
            let (s, acc) = acc.prelude("textLines", &[r]);
            flat(s, list(Ty::Str), acc)
        }
        ("split_whitespace", []) if !chars => {
            let (s, acc) = acc.prelude("textWords", &[r]);
            flat(s, list(Ty::Str), acc)
        }
        ("chars", []) if !chars => {
            let (s, acc) = acc.prelude("textChars", &[r]);
            flat(s, list(Ty::Char), acc)
        }
        ("bytes" | "as_bytes", []) if !chars => {
            let (s, acc) = acc.prelude("textBytes", &[r]);
            flat(s, list(Ty::Nat(Some(8))), acc)
        }
        ("repeat", [n]) if !chars => {
            let ((n, nt), acc) = flat_of(n, env, ctx, acc, Some(&Ty::Nat(None)))?;
            if !matches!(nt, Ty::Nat(_)) {
                return Err(Residue::Form("a repeat count that is not a number".into()));
            }
            {
                let (s, acc) = acc.prelude("textRepeat", &[&n, r]);
                flat(s, Ty::Str, acc)
            }
        }
        ("to_ascii_lowercase", []) => {
            let (s, acc) = acc.prelude("textAsciiLower", &[r]);
            flat(s, rt.clone(), acc)
        }
        // No Unicode case table is bound in LexLean or the SDK: the ASCII
        // letters lower as Rust lowers them, and the definition records
        // that it diverges on the others.
        ("to_lowercase", []) => {
            let (s, acc) = acc.prelude("textAsciiLower", &[r]);
            flat(s, rt.clone(), acc.axiom(&format!("{DIVERGES}{LOWERCASE}")))
        }
        ("to_ascii_uppercase", []) => {
            let (s, acc) = acc.prelude("textAsciiUpper", &[r]);
            flat(s, rt.clone(), acc)
        }
        ("eq_ignore_ascii_case", [other]) => {
            let ((o, ot), acc) = flat_of(other, env, ctx, acc, Some(rt))?;
            if !ot.same(rt) {
                return Err(Residue::Form("eq_ignore_ascii_case of another type".into()));
            }
            let (a, acc) = acc.prelude("textAsciiLower", &[r]);
            let (b, acc) = acc.prelude("textAsciiLower", &[&o]);
            flat(format!("(prim equal {a} {b} : Bool)"), Ty::Bool, acc)
        }
        ("is_ascii", []) => {
            let (s, acc) = acc.prelude("textIsAscii", &[r]);
            flat(s, Ty::Bool, acc)
        }
        (
            "is_ascii_digit"
            | "is_ascii_uppercase"
            | "is_ascii_lowercase"
            | "is_ascii_alphabetic"
            | "is_ascii_alphanumeric"
            | "is_ascii_hexdigit"
            | "is_ascii_whitespace"
            | "is_ascii_punctuation"
            | "is_ascii_control"
            | "is_whitespace"
            | "is_control",
            [],
        ) if chars => {
            let f = match name {
                "is_ascii_digit" => "textIsAsciiDigit",
                "is_ascii_uppercase" => "textIsAsciiUppercase",
                "is_ascii_lowercase" => "textIsAsciiLowercase",
                "is_ascii_alphabetic" => "textIsAsciiAlphabetic",
                "is_ascii_alphanumeric" => "textIsAsciiAlphanumeric",
                "is_ascii_hexdigit" => "textIsAsciiHexdigit",
                "is_ascii_whitespace" => "textIsAsciiWhitespace",
                "is_ascii_punctuation" => "textIsAsciiPunctuation",
                "is_ascii_control" => "textIsAsciiControl",
                "is_whitespace" => "textIsWhitespace",
                _ => "textIsControl",
            };
            {
                let (s, acc) = acc.prelude(f, &[r]);
                flat(s, Ty::Bool, acc)
            }
        }
        ("parse", []) if !chars => {
            let target = match &m.turbofish {
                Some(t) => match t.as_slice() {
                    [rs::GenericArgument::Type(t)] => {
                        Some(env.types.ty(&t.localized(&ctx.imports))?)
                    }
                    _ => None,
                },
                None => match want {
                    Some(Ty::Outcome(t, _) | Ty::Option(t)) => Some((**t).clone()),
                    _ => None,
                },
            };
            let bits = match target {
                Some(Ty::Nat(Some(bits))) if bits <= 64 => bits,
                Some(Ty::Nat(None)) => return Err(Residue::Width(".parse into usize".to_string())),
                _ => return Err(Residue::Form("parse into a type outside the lift".into())),
            };
            let (parsed, acc) = acc.prelude("textParseNat", &[r, max_of(bits)]);
            flat(
                parsed,
                Ty::Outcome(Box::new(Ty::Nat(Some(bits))), Box::new(Ty::ParseError)),
                acc,
            )
        }
        _ => Ok((None, acc)),
    }
}

/// The mark of a divergence among a definition's axioms: what follows it
/// says where the definition and the Rust function part, and it leaves the
/// axioms for the register row.
pub(crate) const DIVERGES: &str = "#diverges ";

/// Where a lifted `to_lowercase` parts from Rust's.
pub(crate) const LOWERCASE: &str = "to_lowercase lowers every letter the Unicode case table maps; \
     the lift lowers the ASCII letters only, since no Unicode case table is bound in LexLean or \
     the SDK, so the two differ on text holding a non-ASCII letter that has a lowercase form";

/// The axioms of the runtime's string length and decimal formatting.
const ALL_AXIOMS: [&str; 3] = ["Classical.choice", "Quot.sound", "propext"];

fn method(
    m: &rs::ExprMethodCall,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let name = m.method.to_string();
    let (shaped, acc) = receiver_shape(m, &name, env, ctx, acc, want)?;
    if let Some(lifted) = shaped {
        return Ok((lifted, acc));
    }
    // What the receiver must be for the result to be what is wanted: a
    // `parse` takes its target from the `Option` or `Result` around it.
    let receiver_want = match (name.as_str(), want) {
        ("ok", Some(Ty::Option(t))) | ("map_err", Some(Ty::Outcome(t, _))) => {
            Some(Ty::Outcome(t.clone(), Box::new(Ty::ParseError)))
        }
        ("unwrap_or" | "unwrap_or_default" | "unwrap" | "expect", Some(t)) => {
            Some(Ty::Option(Box::new(t.clone())))
        }
        _ => None,
    };
    let ((r, rt), acc) = flat_of(&m.receiver, env, ctx, acc, receiver_want.as_ref())?;
    method_on(m, &name, r, rt, env, ctx, acc, want)
}

/// A method call on the receiver's lifted value `r` of type `rt`.
#[allow(clippy::too_many_arguments)]
fn method_on(
    m: &rs::ExprMethodCall,
    name: &str,
    r: String,
    rt: Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let mut acc = acc;
    let args: Vec<&rs::RustExpr> = m.args.iter().collect();
    // A path's text, and the path itself where Rust borrows or copies it.
    if rt == Ty::Path && args.is_empty() {
        match name {
            "display" => {
                let (text, acc) = acc.prelude("pathDisplay", &[&r]);
                return flat_lift(text, Ty::Str, acc);
            }
            "to_path_buf" | "clone" | "to_owned" => return flat_lift(r, Ty::Path, acc),
            _ => {}
        }
    }
    // `p.join(q)`: the path of `q` where it is absolute, else `q` pushed
    // onto `p` after a separator.
    if let (Ty::Path, "join", [piece]) = (&rt, name, args.as_slice()) {
        let ((v, vt), acc) = flat_of(piece, env, ctx, acc, Some(&Ty::Str))?;
        let (text, acc) = match vt {
            Ty::Str => (v, acc),
            Ty::Path => acc.prelude("pathDisplay", &[&v]),
            _ => {
                return Err(Residue::Form(
                    "a join of something other than a path".into(),
                ))
            }
        };
        let (joined, acc) = acc.prelude("pathJoin", &[&r, &text]);
        return flat_lift(joined, Ty::Path, acc);
    }
    if let Ty::Named(owner) = &rt {
        let key = format!("{owner}::{name}");
        if let Some(sig) = env.fns.get(&key) {
            if key == ctx.current {
                // Recursion on a sub-term of `self` descends; any other does not.
                if ctx.tails.get(&r) != ctx.params.first() {
                    return Err(Residue::Form("a method that calls itself".to_string()));
                }
                acc = acc.descended(0);
            }
            if sig.params.len() != args.len() + 1 {
                return Err(Residue::Call(key));
            }
            let mut parts = vec![sig.name.clone(), r];
            for (a, ty) in args.iter().zip(&sig.params[1..]) {
                let ((v, vt), next) = flat_of(a, env, ctx, acc, Some(ty))?;
                acc = next;
                if !vt.same(ty) {
                    return Err(Residue::Form(format!(
                        "argument of another type to .{name}"
                    )));
                }
                parts.push(v);
            }
            let acc = if key == ctx.current {
                acc
            } else {
                acc.called(key)
            };
            return flat_lift(format!("({})", parts.join(" ")), sig.result.clone(), acc);
        }
    }
    if matches!(rt, Ty::Map(..) | Ty::Set(..)) {
        return collection(name, &r, &rt, &args, env, ctx, acc);
    }
    if matches!(rt, Ty::Str | Ty::Char) {
        let (lifted, after) = text_method(m, name, &r, &rt, &args, env, ctx, acc, want)?;
        if let Some(lifted) = lifted {
            return Ok((lifted, after));
        }
        acc = after;
    }
    if let Ty::List(elem) = &rt {
        let (lifted, after) = combinator(m, name, &r, elem, &args, env, ctx, acc)?;
        if let Some(lifted) = lifted {
            return Ok((lifted, after));
        }
        acc = after;
    }
    if rt == Ty::Nat(Some(8)) && args.is_empty() {
        if let Some(test) = ascii_byte(name, &r) {
            return flat_lift(test, Ty::Bool, acc);
        }
    }
    method_typed(name, &r, &rt, &args, env, ctx, acc, want)
}
/// A method call whose receiver is itself a call the lift reads as a
/// whole: `bytes..collect()` into hex, `s.chars().count()`,
/// `s.matches(p).count()` and `(a..b).contains(&x)`. Each shape is tried
/// in turn, and the first that applies gives the value.
fn receiver_shape(
    m: &rs::ExprMethodCall,
    name: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let (lifted, acc) = hex_collect(m, name, env, ctx, acc, want)?;
    if lifted.is_some() {
        return Ok((lifted, acc));
    }
    let (lifted, acc) = chars_count(m, name, env, ctx, acc)?;
    if lifted.is_some() {
        return Ok((lifted, acc));
    }
    let (lifted, acc) = matches_count(m, name, env, ctx, acc)?;
    if lifted.is_some() {
        return Ok((lifted, acc));
    }
    range_contains(m, name, env, ctx, acc)
}

/// `bytes.iter().map(|b| format!("{b:02x}")).collect()` into a `String`:
/// the bytes as lowercase hex.
fn hex_collect(
    m: &rs::ExprMethodCall,
    name: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let mut acc = acc;
    let into_string = want == Some(&Ty::Str)
        || matches!(
            m.turbofish.as_deref(),
            Some([rs::GenericArgument::Type(t)]) if t.text == "String"
        );
    if name == "collect" && m.args.is_empty() && into_string {
        if let Some(bytes) = hex_of(&m.receiver) {
            let byte_list = Ty::List(Box::new(Ty::Nat(Some(8))));
            let ((v, vt), after) = flat_of(bytes, env, ctx, acc, Some(&byte_list))?;
            acc = after;
            if vt == byte_list {
                let (s, acc) = acc.prelude("hexBytes", &[&v]);
                return flat_some(s, Ty::Str, acc);
            }
        }
    }
    Ok((None, acc))
}

/// `s.chars().count()`: the number of characters, which is LexLean's
/// string length.
fn chars_count(
    m: &rs::ExprMethodCall,
    name: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let mut acc = acc;
    if let rs::RustExpr::MethodCall(inner) = &*m.receiver {
        if name == "count" && m.args.is_empty() && inner.method == "chars" && inner.args.is_empty()
        {
            let ((s, st), after) = flat_of(&inner.receiver, env, ctx, acc, Some(&Ty::Str))?;
            acc = after;
            if st == Ty::Str {
                let acc = acc.all_axioms();
                return flat_some(format!("(prim length {s} : Nat)"), Ty::Nat(None), acc);
            }
        }
    }
    Ok((None, acc))
}

/// `s.matches(p).count()`: the pieces `s.split(p)` cuts, less one.
fn matches_count(
    m: &rs::ExprMethodCall,
    name: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    if let rs::RustExpr::MethodCall(inner) = &*m.receiver {
        if let ("count", true, "matches", [p]) = (
            name,
            m.args.is_empty(),
            inner.method.as_str(),
            inner.args.as_slice(),
        ) {
            let ((s, st), after) = flat_of(&inner.receiver, env, ctx, acc, Some(&Ty::Str))?;
            let ((p, pt), after) = flat_of(p, env, ctx, after, Some(&Ty::Str))?;
            if st != Ty::Str || !matches!(pt, Ty::Str | Ty::Char) {
                return Err(Residue::Form("a .matches pattern that is not text".into()));
            }
            let (pieces, after) = after.prelude("textSplit", &[&s, &p]);
            return flat_some(
                format!("(prim subtract (prim length {pieces} : Nat) 1 : Nat)"),
                Ty::Nat(None),
                after,
            );
        }
    }
    Ok((None, acc))
}

/// `(a..b).contains(&x)`: `a <= x` and `x` below the end, or at it for
/// `a..=b`.
fn range_contains(
    m: &rs::ExprMethodCall,
    name: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    if let (Some((lo, hi, closed)), "contains", [x]) =
        (range_bounds(&m.receiver), name, m.args.as_slice())
    {
        let ((l, lt), acc) = flat_of(lo, env, ctx, acc, Some(&Ty::Nat(None)))?;
        let ((h, ht), acc) = flat_of(hi, env, ctx, acc, Some(&lt))?;
        let x = match x {
            rs::RustExpr::Reference(r) => &*r.expr,
            other => other,
        };
        let ((v, vt), acc) = flat_of(x, env, ctx, acc, Some(&lt))?;
        if !matches!(lt, Ty::Nat(_)) || !ht.same(&lt) || !vt.same(&lt) {
            return Err(Residue::Form(
                "a range of something other than naturals".into(),
            ));
        }
        let below = if closed { "<=?" } else { "<?" };
        return flat_some(
            format!("(({l} <=? {v}) && ({v} {below} {h}))"),
            Ty::Bool,
            acc,
        );
    }
    Ok((None, acc))
}

/// A method of a receiver of a type the lift knows, by its type and name.
#[allow(clippy::too_many_arguments)]
fn method_typed(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    match (rt, name) {
        // `a.checked_div(b)`, `a.checked_rem(b)`: none for a zero divisor.
        (Ty::Nat(_), "checked_div" | "checked_rem") if args.len() == 1 => {
            method_nat_checked_div(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(w), "checked_add" | "checked_mul") => {
            method_nat_checked_add(w, name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(_), "checked_sub") => {
            method_nat_checked_sub(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(_), "checked_div" | "checked_rem") => {
            method_nat_checked_div_args(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(w), "wrapping_add" | "wrapping_mul" | "wrapping_sub") => {
            method_nat_wrapping_add(w, name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(w), "saturating_add" | "saturating_mul") => {
            method_nat_saturating_add(w, name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(_), "saturating_sub") => {
            method_nat_saturating_sub(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(_), "abs_diff") => {
            method_nat_abs_diff(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(_), "midpoint") => {
            method_nat_midpoint(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Nat(_), "min" | "max") => {
            method_nat_min(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Int(bits), "checked_add" | "checked_sub" | "checked_mul" | "checked_div") => {
            method_int_checked_add(bits, name, r, rt, args, env, ctx, acc)
        }
        (Ty::Int(bits), "checked_neg") if args.is_empty() => {
            method_int_checked_neg(bits, name, r, rt, acc)
        }
        (Ty::Int(_), "min" | "max") => {
            method_int_min(name, r, rt, args, env, ctx, acc)
        }
        (Ty::List(_), "len") if args.is_empty() => {
            method_list_len(r, acc)
        }
        (Ty::List(_), "is_empty") if args.is_empty() => {
            method_list_is_empty(r, acc)
        }
        (Ty::List(t), "first") if args.is_empty() => flat_lift(
            format!("(prim index {r} 0 : (Option {}))", t.render()),
            opt_of(t),
            acc,
        ),
        (Ty::List(t), "get") => {
            method_list_get(t, name, r, args, env, ctx, acc)
        }
        (Ty::Str, "is_empty") if args.is_empty() => {
            method_str_is_empty(r, acc)
        }
        (Ty::Str, "to_string" | "to_owned")
        | (Ty::List(_), "as_slice")
        | (Ty::Option(_), "as_deref")
        // An option of a borrow, or a borrowed slice: the same value, since
        // LexLean holds every value by reference.
        | (Ty::Option(_) | Ty::List(_), "as_ref")
        | (_, "clone" | "copied" | "cloned" | "to_vec")
            if args.is_empty() =>
        {
            method_str_to_string(r, rt, acc)
        }
        // `b.then_some(v)`: `Some(v)` where `b` holds.
        // `c.then(|| v)`: the value where the test holds. A lifted value
        // is pure and total, so building it eagerly changes nothing.
        (Ty::Bool, "then") if matches!(args, [rs::RustExpr::Closure(c)] if c.inputs.is_empty()) =>
        {
            method_bool_then(r, args, env, ctx, acc, want)
        }
        (Ty::Bool, "then_some") if args.len() == 1 => {
            method_bool_then_some(r, args, env, ctx, acc)
        }
        (Ty::Option(_), "is_some" | "is_none") if args.is_empty() => {
            method_option_is_some(name, r, env, ctx, acc)
        }
        (Ty::Option(t), "unwrap_or") => {
            method_option_unwrap_or(t, name, r, args, env, ctx, acc)
        }
        (Ty::Outcome(_, _), "is_ok" | "is_err") if args.is_empty() => {
            method_outcome_is_ok(name, r, env, ctx, acc)
        }
        (Ty::Outcome(t, _), "ok") if args.is_empty() => {
            method_outcome_ok(t, r, env, ctx, acc)
        }
        (Ty::Option(_) | Ty::Outcome(..), _) => {
            method_option_any(name, r, rt, args, env, ctx, acc)
        }
        (Ty::Char, _) if unicode_property(name).is_some() => {
            method_char_any(name)
        }
        _ => Err(Residue::Form(format!("method call .{name}"))),
    }
}

/// `.checked_div` on a receiver of type `Nat`.
fn method_nat_checked_div(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let ((d, dt), acc) = flat_of(args[0], env, ctx, acc, Some(rt))?;
    if !dt.same(rt) {
        return Err(Residue::Form(format!(
            "argument of another type to .{name}"
        )));
    }
    let prim = if name == "checked_div" {
        "quotient"
    } else {
        "remainder"
    };
    flat_lift(
        format!(
            "(if ({d} =? 0) then {} else {})",
            none_of(rt),
            some_of(rt, &format!("(prim {prim} {r} {d} 0 : Nat)"))
        ),
        opt_of(rt),
        acc,
    )
}

/// `.checked_add` on a receiver of type `Nat`.
#[allow(clippy::too_many_arguments)]
fn method_nat_checked_add(
    w: &Option<u32>,
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(bits) = w else {
        return Err(Residue::Width(format!(".{name} on usize")));
    };
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    let v = if name == "checked_add" {
        format!("({r} + {a})")
    } else {
        format!("(prim multiply {r} {a} : Nat)")
    };
    flat_lift(
        format!(
            "(if ({v} <? {}) then {} else {})",
            pow2(*bits),
            some_of(rt, &v),
            none_of(rt)
        ),
        opt_of(rt),
        acc,
    )
}

/// `.checked_sub` on a receiver of type `Nat`.
fn method_nat_checked_sub(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    flat_lift(
        format!(
            "(if ({a} <=? {r}) then {} else {})",
            some_of(rt, &format!("(prim subtract {r} {a} : Nat)")),
            none_of(rt)
        ),
        opt_of(rt),
        acc,
    )
}

/// `.checked_div` on a receiver of type `Nat`.
fn method_nat_checked_div_args(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    let prim = if name == "checked_div" {
        "quotient"
    } else {
        "remainder"
    };
    flat_lift(
        format!(
            "(if ({a} =? 0) then {} else {})",
            none_of(rt),
            some_of(rt, &format!("(prim {prim} {r} {a} 0 : Nat)"))
        ),
        opt_of(rt),
        acc,
    )
}

/// `.wrapping_add` on a receiver of type `Nat`.
#[allow(clippy::too_many_arguments)]
fn method_nat_wrapping_add(
    w: &Option<u32>,
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(bits) = w else {
        return Err(Residue::Width(format!(".{name} on usize")));
    };
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    let m = pow2(*bits);
    let v = match name {
        "wrapping_add" => format!("({r} + {a})"),
        "wrapping_mul" => format!("(prim multiply {r} {a} : Nat)"),
        _ => format!("(prim subtract ({r} + {m}) {a} : Nat)"),
    };
    flat_lift(format!("(prim remainder {v} {m} 0 : Nat)"), rt.clone(), acc)
}

/// `.saturating_add` on a receiver of type `Nat`.
#[allow(clippy::too_many_arguments)]
fn method_nat_saturating_add(
    w: &Option<u32>,
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(bits) = w else {
        return Err(Residue::Width(format!(".{name} on usize")));
    };
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    let v = if name == "saturating_add" {
        format!("({r} + {a})")
    } else {
        format!("(prim multiply {r} {a} : Nat)")
    };
    flat_lift(
        format!(
            "(if ({v} <? {}) then {v} else {})",
            pow2(*bits),
            max_of(*bits)
        ),
        rt.clone(),
        acc,
    )
}

/// `.saturating_sub` on a receiver of type `Nat`.
fn method_nat_saturating_sub(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    flat_lift(format!("(prim subtract {r} {a} : Nat)"), rt.clone(), acc)
}

/// `.abs_diff` on a receiver of type `Nat`.
fn method_nat_abs_diff(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    flat_lift(
        format!("(if ({r} <=? {a}) then (prim subtract {a} {r} : Nat) else (prim subtract {r} {a} : Nat))"),
        rt.clone(), acc)
}

/// `.midpoint` on a receiver of type `Nat`.
fn method_nat_midpoint(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    flat_lift(
        format!("(prim quotient ({r} + {a}) 2 0 : Nat)"),
        rt.clone(),
        acc,
    )
}

/// `.min` on a receiver of type `Nat`.
fn method_nat_min(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    let (x, y) = if name == "min" {
        (r, a.as_str())
    } else {
        (a.as_str(), r)
    };
    flat_lift(
        format!("(if ({r} <=? {a}) then {x} else {y})"),
        rt.clone(),
        acc,
    )
}

/// `.checked_add` on a receiver of type `Int`.
#[allow(clippy::too_many_arguments)]
fn method_int_checked_add(
    bits: &u32,
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    if *bits != 64 {
        return Err(Residue::Width(format!(".{name} on i{bits}")));
    }
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    let prim = match name {
        "checked_add" => "checked_add",
        "checked_sub" => "checked_subtract",
        "checked_mul" => "checked_multiply",
        _ => "checked_quotient",
    };
    flat_lift(
        format!("(prim {prim} {r} {a} : (Option Int64))"),
        opt_of(rt),
        acc,
    )
}

/// `.checked_neg` on a receiver of type `Int`.
fn method_int_checked_neg(
    bits: &u32,
    name: &str,
    r: &str,
    rt: &Ty,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    if *bits != 64 {
        return Err(Residue::Width(format!(".{name} on i{bits}")));
    }
    flat_lift(
        format!("(prim checked_negate {r} : (Option Int64))"),
        opt_of(rt),
        acc,
    )
}

/// `.min` on a receiver of type `Int`.
fn method_int_min(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let acc = acc.axiom("propext");
    let (a, acc) = one_arg(args, name, env, ctx, rt, acc)?;
    let (x, y) = if name == "min" {
        (r, a.as_str())
    } else {
        (a.as_str(), r)
    };
    flat_lift(
        format!("(if (prim less_than {a} {r} : Bool) then {y} else {x})"),
        rt.clone(),
        acc,
    )
}

/// `.len` on a receiver of type `List`.
fn method_list_len(r: &str, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    flat_lift(format!("(prim length {r} : Nat)"), Ty::Nat(None), acc)
}

/// `.is_empty` on a receiver of type `List`.
fn method_list_is_empty(r: &str, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    flat_lift(format!("((prim length {r} : Nat) =? 0)"), Ty::Bool, acc)
}

/// `.get` on a receiver of type `List`.
fn method_list_get(
    t: &Ty,
    name: &str,
    r: &str,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (i, acc) = one_arg(args, name, env, ctx, &Ty::Nat(None), acc)?;
    flat_lift(
        format!("(prim index {r} {i} : (Option {}))", t.render()),
        opt_of(t),
        acc,
    )
}

/// `.is_empty` on a receiver of type `Str`.
fn method_str_is_empty(r: &str, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    flat_lift(format!("(prim equal {r} \"\" : Bool)"), Ty::Bool, acc)
}

/// `.to_string` on a receiver of type `Str`.
fn method_str_to_string(r: &str, rt: &Ty, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    flat_lift(r.to_string(), rt.clone(), acc)
}

/// `.then` on a receiver of type `Bool`.
fn method_bool_then(
    r: &str,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let [rs::RustExpr::Closure(c)] = args else {
        return Err(Residue::Form(".then of another argument".into()));
    };
    let inner = match want {
        Some(Ty::Option(t)) => Some(&**t),
        _ => None,
    };
    let ((v, vt), acc) = flat_of(&c.body, env, &ctx.below(), acc, inner)?;
    flat_lift(
        format!("(if {r} then {} else {})", some_of(&vt, &v), none_of(&vt)),
        opt_of(&vt),
        acc,
    )
}

/// `.then_some` on a receiver of type `Bool`.
fn method_bool_then_some(
    r: &str,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let ((v, vt), acc) = flat_of(args[0], env, ctx, acc, None)?;
    flat_lift(
        format!("(if {r} then {} else {})", some_of(&vt, &v), none_of(&vt)),
        opt_of(&vt),
        acc,
    )
}

/// `.is_some` on a receiver of type `Option`.
fn method_option_is_some(
    name: &str,
    r: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (v, acc) = ctx.fresh(env, acc, "value");
    let (n, s) = if name == "is_some" {
        ("false", "true")
    } else {
        ("true", "false")
    };
    Ok((
        (
            Expr::Match {
                scrutinee: r.to_string(),
                arms: vec![
                    Arm {
                        pattern: "Option.none".into(),
                        body: Expr::Flat(n.into()),
                    },
                    Arm {
                        pattern: format!("Option.some {v}"),
                        body: Expr::Flat(s.into()),
                    },
                ],
            },
            Ty::Bool,
        ),
        acc,
    ))
}

/// `.unwrap_or` on a receiver of type `Option`.
fn method_option_unwrap_or(
    t: &Ty,
    name: &str,
    r: &str,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (d, acc) = one_arg(args, name, env, ctx, t, acc)?;
    let (v, acc) = ctx.fresh(env, acc, "value");
    Ok((
        (
            Expr::Match {
                scrutinee: r.to_string(),
                arms: vec![
                    Arm {
                        pattern: "Option.none".into(),
                        body: Expr::Flat(d),
                    },
                    Arm {
                        pattern: format!("Option.some {v}"),
                        body: Expr::Flat(v.clone()),
                    },
                ],
            },
            t.clone(),
        ),
        acc,
    ))
}

/// `.is_ok` on a receiver of type `Outcome`.
fn method_outcome_is_ok(
    name: &str,
    r: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let (v, acc) = ctx.fresh(env, acc, "value");
    let (e, acc) = ctx.fresh(env, acc, "error");
    let (o, x) = if name == "is_ok" {
        ("true", "false")
    } else {
        ("false", "true")
    };
    Ok((
        (
            Expr::Match {
                scrutinee: r.to_string(),
                arms: vec![
                    Arm {
                        pattern: format!("Result.ok {v}"),
                        body: Expr::Flat(o.into()),
                    },
                    Arm {
                        pattern: format!("Result.error {e}"),
                        body: Expr::Flat(x.into()),
                    },
                ],
            },
            Ty::Bool,
        ),
        acc,
    ))
}

/// `.ok` on a receiver of type `Outcome`.
fn method_outcome_ok(t: &Ty, r: &str, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let (v, acc) = ctx.fresh(env, acc, "value");
    let (e, acc) = ctx.fresh(env, acc, "error");
    Ok((
        (
            Expr::Match {
                scrutinee: r.to_string(),
                arms: vec![
                    Arm {
                        pattern: format!("Result.ok {v}"),
                        body: Expr::Flat(some_of(t, &v)),
                    },
                    Arm {
                        pattern: format!("Result.error {e}"),
                        body: Expr::Flat(none_of(t)),
                    },
                ],
            },
            opt_of(t),
        ),
        acc,
    ))
}

/// `._` on a receiver of type `Option`.
fn method_option_any(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    wrapped_method(name, r, rt, args, env, ctx, acc)
}

/// `._` on a receiver of type `Char`.
fn method_char_any(name: &str) -> Lift<((Expr, Ty), Acc)> {
    Err(Residue::Form(unicode_property(name).unwrap_or_default()))
}

/// `Some(v)` of type `t`.
fn some_of(t: &Ty, v: &str) -> String {
    format!("(ctor Option.some [{}] {v})", t.render())
}

/// `None` of type `t`.
fn none_of(t: &Ty) -> String {
    format!("(ctor Option.none [{}])", t.render())
}

/// `Option<t>`.
fn opt_of(t: &Ty) -> Ty {
    Ty::Option(Box::new(t.clone()))
}

/// The one argument of a method call, of the type it must have.
fn one_arg(
    args: &[&rs::RustExpr],
    name: &str,
    env: &Env,
    ctx: &Ctx,
    want: &Ty,
    acc: Acc,
) -> Lift<(String, Acc)> {
    let [a] = args else {
        return Err(Residue::Form(format!("method call .{name}")));
    };
    let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(want))?;
    if vt.same(want) {
        Ok((v, acc))
    } else {
        Err(Residue::Form(format!(
            "argument of another type to .{name}"
        )))
    }
}

/// The bounds of `a..b` or `a..=b`, in parentheses or not, and whether the
/// range holds its end.
fn range_bounds(e: &rs::RustExpr) -> Option<(&rs::RustExpr, &rs::RustExpr, bool)> {
    let bare = match e {
        rs::RustExpr::Paren(inner) => &**inner,
        other => other,
    };
    match bare {
        rs::RustExpr::Other(o) => match (o.kind.as_str(), o.exprs.as_slice()) {
            ("expr::range", [lo, hi]) => Some((lo, hi, false)),
            ("expr::range_inclusive", [lo, hi]) => Some((lo, hi, true)),
            _ => None,
        },
        _ => None,
    }
}

/// The gap a Unicode property of a `char` names: the property tests the
/// Unicode Character Database, and no table of it is bound in LexLean or
/// the SDK. The ASCII forms (`is_ascii_alphabetic`, ..) lift.
fn unicode_property(name: &str) -> Option<String> {
    #[cfg(not(uorust_lift))]
    crate::certified::certified!(lift_unicode_property, name);
    let property = match name {
        "is_alphabetic" => "Alphabetic",
        "is_alphanumeric" => "Alphabetic or Numeric",
        "is_numeric" => "Numeric",
        "is_uppercase" => "Uppercase",
        "is_lowercase" => "Lowercase",
        _ => return None,
    };
    Some(format!(
        "char::{name} tests the Unicode {property} property, and no Unicode table is bound \
         in LexLean or the SDK"
    ))
}

/// A value of a type, where the lift needs one that nothing observes: what
/// a Rust `unwrap` or index panics to. Lean's own `get!` returns the same
/// `default`, so the definition agrees with Rust wherever Rust returns.
pub(crate) fn default_of(ty: &Ty, types: &Types) -> Option<String> {
    default_in(ty, types, 0)
}

fn default_in(ty: &Ty, types: &Types, depth: usize) -> Option<String> {
    if depth > 16 {
        return None;
    }
    Some(match ty {
        Ty::Unit => "()".into(),
        Ty::Bool => "false".into(),
        Ty::Nat(_) | Ty::Float(_) | Ty::Signed(_) | Ty::Scalar => "0".into(),
        Ty::Int(_) => "(0 : Int64)".into(),
        Ty::Str | Ty::Char => "\"\"".into(),
        Ty::ParseError => format!("(ctor {PRELUDE}::ParseIntError.InvalidDigit)"),
        Ty::Option(t) => format!("(ctor Option.none [{}])", t.render()),
        // `Ok` of the success type's default, or `Err` of the error's.
        Ty::Outcome(ok, err) => match default_in(ok, types, depth + 1) {
            Some(v) => format!("(ctor Result.ok [{}, {}] {v})", ok.render(), err.render()),
            None => format!(
                "(ctor Result.error [{}, {}] {})",
                ok.render(),
                err.render(),
                default_in(err, types, depth + 1)?
            ),
        },
        Ty::Path => return None,
        // A function: the one that gives every input its result's default.
        Ty::Fn(ins, out) => {
            let params: Vec<String> = ins
                .iter()
                .enumerate()
                .map(|(i, t)| format!("(input{i} : {})", t.render()))
                .collect();
            format!(
                "(fun {} => {})",
                params.join(" "),
                default_in(out, types, depth + 1)?
            )
        }
        Ty::List(t) | Ty::Set(t, _) => format!("(nil {})", t.render()),
        Ty::Map(..) => format!("(nil {})", ty.entry().render()),
        Ty::Tuple(ts) | Ty::Pair(ts) => {
            let mut parts = Vec::new();
            for t in ts {
                parts.push(default_in(t, types, depth + 1)?);
            }
            tuple_value(ty, &parts)
        }
        Ty::Named(n) => {
            if let Some(vs) = types.enums.get(n) {
                format!("(ctor {n}.{})", vs.first()?)
            } else if let Some(vs) = types.datas.get(n) {
                // The first variant whose fields all have a value.
                vs.iter().find_map(|(v, fields)| {
                    let mut parts = vec![format!("ctor {n}.{v}")];
                    for (_, t) in fields {
                        parts.push(default_in(t, types, depth + 1)?);
                    }
                    Some(format!("({})", parts.join(" ")))
                })?
            } else {
                let fields = types.structs.get(n)?;
                let mut parts = Vec::new();
                for (f, t) in fields {
                    parts.push((f, default_in(t, types, depth + 1)?));
                }
                struct_value(n, &parts, types)
            }
        }
    })
}

/// The combinators of `Option` and `Result`, each a match on the receiver:
/// `map`, `and_then`, `map_err`, `filter`, `is_some_and`, `is_none_or`,
/// `is_ok_and`, `ok_or`, `err`, `unwrap_or`, `unwrap_or_else`,
/// `unwrap_or_default`, `unwrap` and `expect`.
#[allow(clippy::too_many_lines)]
fn wrapped_method(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let bad = || Err(Residue::Form(format!("method call .{name}")));
    let some = |t: &Ty, v: &str| format!("(ctor Option.some [{}] {v})", t.render());
    let none = |t: &Ty| format!("(ctor Option.none [{}])", t.render());
    let opt = |t: &Ty| Ty::Option(Box::new(t.clone()));
    // The receiver's value, its failure, and the constructor patterns.
    let (value, failure) = match rt {
        Ty::Option(t) => ((**t).clone(), None),
        Ty::Outcome(t, e) => ((**t).clone(), Some((**e).clone())),
        _ => return bad(),
    };
    let ok_pattern = |v: &str| {
        if failure.is_some() {
            format!("Result.ok {v}")
        } else {
            format!("Option.some {v}")
        }
    };
    let fail_pattern = |e: &str| {
        if failure.is_some() {
            format!("Result.error {e}")
        } else {
            "Option.none".to_string()
        }
    };
    let result_of = |t: &Ty, e: &Ty| format!("[{}, {}]", t.render(), e.render());
    let two = |ok_v: &str, ok: Expr, fail_e: &str, fail: Expr| Expr::Match {
        scrutinee: r.to_string(),
        arms: if failure.is_some() {
            vec![
                Arm {
                    pattern: ok_pattern(ok_v),
                    body: ok,
                },
                Arm {
                    pattern: fail_pattern(fail_e),
                    body: fail,
                },
            ]
        } else {
            vec![
                Arm {
                    pattern: fail_pattern(fail_e),
                    body: fail,
                },
                Arm {
                    pattern: ok_pattern(ok_v),
                    body: ok,
                },
            ]
        },
    };
    let closure =
        |f: &rs::RustExpr, params: &[Ty], acc: Acc, want: Option<&Ty>| -> Lift<(Lambda, Acc)> {
            let (lam, acc) = lambda(f, params, env, ctx, acc, want)?;
            let acc = acc.called_all(lam.calls.clone());
            let acc = acc.axioms_all(lam.axioms.clone());
            Ok((lam, acc))
        };
    let (fresh_e, acc) = ctx.fresh(env, acc, "error");
    match (name, args) {
        ("map", [f]) => {
            let (lam, acc) = closure(f, std::slice::from_ref(&value), acc, None)?;
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            match &failure {
                None => Ok((
                    (
                        two(&v, wrap_some(&lam.ty, body), "", Expr::Flat(none(&lam.ty))),
                        opt(&lam.ty),
                    ),
                    acc,
                )),
                Some(e) => Ok((
                    (
                        two(
                            &v,
                            wrap_ctor(&format!("Result.ok {}", result_of(&lam.ty, e)), body),
                            &fresh_e,
                            Expr::Flat(format!(
                                "(ctor Result.error {} {fresh_e})",
                                result_of(&lam.ty, e)
                            )),
                        ),
                        Ty::Outcome(Box::new(lam.ty.clone()), Box::new(e.clone())),
                    ),
                    acc,
                )),
            }
        }
        ("map_or", [d, f]) => {
            let (lam, acc) = closure(f, std::slice::from_ref(&value), acc, None)?;
            let ((dv, dt), acc) = flat_of(d, env, ctx, acc, Some(&lam.ty))?;
            if !dt.same(&lam.ty) {
                return Err(Residue::Form("map_or of another type".into()));
            }
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            Ok((
                (two(&v, body, &fresh_e, Expr::Flat(dv)), lam.ty.clone()),
                acc,
            ))
        }
        ("map_err", [f]) => {
            let Some(e) = &failure else { return bad() };
            let (lam, acc) = closure(f, std::slice::from_ref(e), acc, None)?;
            let (v, acc) = ctx.fresh(env, acc, "value");
            let x = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            let types = result_of(&value, &lam.ty);
            Ok((
                (
                    two(
                        &v,
                        Expr::Flat(format!("(ctor Result.ok {types} {v})")),
                        &x,
                        wrap_ctor(&format!("Result.error {types}"), body),
                    ),
                    Ty::Outcome(Box::new(value.clone()), Box::new(lam.ty.clone())),
                ),
                acc,
            ))
        }
        ("and_then", [f]) => {
            let (lam, acc) = closure(f, std::slice::from_ref(&value), acc, None)?;
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            let fail = match (&failure, &lam.ty) {
                (None, Ty::Option(u)) => none(u),
                (Some(e), Ty::Outcome(u, e2)) if e.same(e2) => {
                    format!("(ctor Result.error {} {fresh_e})", result_of(u, e))
                }
                _ => return Err(Residue::Form("and_then of another type".into())),
            };
            Ok((
                (two(&v, body, &fresh_e, Expr::Flat(fail)), lam.ty.clone()),
                acc,
            ))
        }
        ("filter", [f]) if failure.is_none() => {
            let (lam, acc) = closure(f, std::slice::from_ref(&value), acc, Some(&Ty::Bool))?;
            if lam.ty != Ty::Bool {
                return bad();
            }
            let v = lam.binders[0].clone();
            let test = lam.wrap(format!(
                "(if {} then {} else {})",
                lam.core,
                some(&value, &v),
                none(&value)
            ));
            Ok((
                (two(&v, test, "", Expr::Flat(none(&value))), rt.clone()),
                acc,
            ))
        }
        ("is_some_and" | "is_none_or" | "is_ok_and" | "is_err_and", [f]) => {
            let on_err = name == "is_err_and";
            let param = if on_err {
                match &failure {
                    Some(e) => e.clone(),
                    None => return bad(),
                }
            } else {
                value.clone()
            };
            let (lam, acc) = closure(f, std::slice::from_ref(&param), acc, Some(&Ty::Bool))?;
            if lam.ty != Ty::Bool {
                return bad();
            }
            let b = lam.binders[0].clone();
            let test = lam.wrap(lam.core.clone());
            let otherwise = Expr::Flat((name == "is_none_or").to_string());
            if on_err {
                let (v, acc) = ctx.fresh(env, acc, "value");
                return Ok(((two(&v, otherwise, &b, test), Ty::Bool), acc));
            }
            Ok(((two(&b, test, &fresh_e, otherwise), Ty::Bool), acc))
        }
        ("ok_or", [e]) if failure.is_none() => {
            let ((ev, et), acc) = flat_of(e, env, ctx, acc, None)?;
            let (v, acc) = ctx.fresh(env, acc, "value");
            let types = result_of(&value, &et);
            Ok((
                (
                    two(
                        &v,
                        Expr::Flat(format!("(ctor Result.ok {types} {v})")),
                        "",
                        Expr::Flat(format!("(ctor Result.error {types} {ev})")),
                    ),
                    Ty::Outcome(Box::new(value.clone()), Box::new(et)),
                ),
                acc,
            ))
        }
        ("err", []) => {
            let Some(e) = &failure else { return bad() };
            let (v, acc) = ctx.fresh(env, acc, "value");
            Ok((
                (
                    two(
                        &v,
                        Expr::Flat(none(e)),
                        &fresh_e,
                        Expr::Flat(some(e, &fresh_e)),
                    ),
                    opt(e),
                ),
                acc,
            ))
        }
        ("unwrap_or", [d]) => {
            let ((dv, dt), acc) = flat_of(d, env, ctx, acc, Some(&value))?;
            if !dt.same(&value) {
                return Err(Residue::Form("unwrap_or of another type".into()));
            }
            let (v, acc) = ctx.fresh(env, acc, "value");
            Ok((
                (
                    two(&v, Expr::Flat(v.clone()), &fresh_e, Expr::Flat(dv)),
                    value,
                ),
                acc,
            ))
        }
        // `o.or(p)`, `o.or_else(|| p)`: the option where it holds a value,
        // else the other.
        ("or" | "or_else", [d]) if failure.is_none() => {
            let whole = opt(&value);
            let (other, acc) = if name == "or" {
                let ((dv, dt), acc) = flat_of(d, env, ctx, acc, Some(&whole))?;
                if !dt.same(&whole) {
                    return Err(Residue::Form(format!("{name} of another type")));
                }
                (Expr::Flat(dv), acc)
            } else {
                let (lam, acc) = closure(d, &[], acc, Some(&whole))?;
                if !lam.ty.same(&whole) {
                    return Err(Residue::Form(format!("{name} of another type")));
                }
                (lam.wrap(lam.core.clone()), acc)
            };
            let (v, acc) = ctx.fresh(env, acc, "value");
            let kept = Expr::Flat(some(&value, &v));
            Ok(((two(&v, kept, &fresh_e, other), whole), acc))
        }
        ("unwrap_or_else", [f]) => {
            let params: Vec<Ty> = failure.iter().cloned().collect();
            let (lam, acc) = closure(f, &params, acc, Some(&value))?;
            if !lam.ty.same(&value) {
                return Err(Residue::Form("unwrap_or_else of another type".into()));
            }
            let e = lam
                .binders
                .first()
                .cloned()
                .unwrap_or_else(|| fresh_e.clone());
            let (v, acc) = ctx.fresh(env, acc, "value");
            let fallback = lam.wrap(lam.core.clone());
            Ok(((two(&v, Expr::Flat(v.clone()), &e, fallback), value), acc))
        }
        ("unwrap_or_default" | "unwrap" | "expect", _) => {
            let rust_default = name == "unwrap_or_default";
            if rust_default && matches!(value, Ty::Named(_)) {
                return Err(Residue::Form("unwrap_or_default of a named type".into()));
            }
            let Some(d) = default_of(&value, &env.types) else {
                return Err(Residue::Form(format!(".{name} of a type with no value")));
            };
            let (v, acc) = ctx.fresh(env, acc, "value");
            Ok((
                (
                    two(&v, Expr::Flat(v.clone()), &fresh_e, Expr::Flat(d)),
                    value,
                ),
                acc,
            ))
        }
        _ => bad(),
    }
}

/// `(ctor Option.some [T] body)` around a body that may hold lets.
fn wrap_some(t: &Ty, body: Expr) -> Expr {
    wrap_ctor(&format!("Option.some [{}]", t.render()), body)
}

/// A constructor applied to the tail of a body: lets stay outside.
fn wrap_ctor(ctor: &str, body: Expr) -> Expr {
    map_tails(body, &|tail| format!("(ctor {ctor} {tail})"))
}

/// The ASCII class tests of a byte, as ranges of its value.
fn ascii_byte(name: &str, b: &str) -> Option<String> {
    let range = |lo: u8, hi: u8| format!("(({lo} <=? {b}) && ({b} <=? {hi}))");
    let upper = range(b'A', b'Z');
    let lower = range(b'a', b'z');
    let digit = range(b'0', b'9');
    Some(match name {
        "is_ascii" => format!("({b} <? 128)"),
        "is_ascii_digit" => digit,
        "is_ascii_uppercase" => upper,
        "is_ascii_lowercase" => lower,
        "is_ascii_alphabetic" => format!("({upper} || {lower})"),
        "is_ascii_alphanumeric" => format!("({upper} || ({lower} || {digit}))"),
        "is_ascii_hexdigit" => format!(
            "({digit} || ({} || {}))",
            range(b'A', b'F'),
            range(b'a', b'f')
        ),
        "is_ascii_whitespace" => format!(
            "(({b} =? 32) || (({b} =? 9) || (({b} =? 10) || (({b} =? 12) || ({b} =? 13)))))"
        ),
        "is_ascii_punctuation" => format!(
            "({} || ({} || ({} || {})))",
            range(33, 47),
            range(58, 64),
            range(91, 96),
            range(123, 126)
        ),
        "is_ascii_control" => format!("(({b} <? 32) || ({b} =? 127))"),
        "is_ascii_graphic" => range(33, 126),
        _ => return None,
    })
}

/// A closure lifted in a scope of its own.
struct Lambda {
    /// The LexLean binder of each closure parameter.
    binders: Vec<String>,
    /// Projections a tuple parameter's pattern binds: `(name, type, value)`.
    lets: Vec<(String, String, String)>,
    /// The body, on one line.
    core: String,
    ty: Ty,
    calls: BTreeSet<String>,
    axioms: BTreeSet<String>,
    /// The parameters of the function being lifted that the body's
    /// recursive calls descend on: through an element of a list that is a
    /// sub-term of one, so the helper walking the list and the function
    /// are one mutual group.
    descends: BTreeSet<usize>,
}

impl Lambda {
    /// The body's projections around a term that uses its binders.
    fn wrap(&self, term: String) -> Expr {
        self.wrap_expr(Expr::Flat(term))
    }

    /// The body's projections around an expression that uses its binders.
    fn wrap_expr(&self, term: Expr) -> Expr {
        self.lets
            .iter()
            .rev()
            .fold(term, |body, (name, ty, value)| Expr::Let {
                name: name.clone(),
                ty: ty.clone(),
                value: Box::new(Expr::Flat(value.clone())),
                body: Box::new(body),
            })
    }
}

/// Bind a closure parameter's pattern: a name, `_`, `&p`, `p: T`, or a tuple
/// of those over a tuple element.
/// The projections bound so far come in and go out with the new ones.
type Lets = Vec<(String, String, String)>;

/// A tuple or struct pattern: the whole bound to a fresh binder, and each
/// part's pattern bound to its item or field of it, in turn, so a part's
/// own parts follow it.
fn bind_parts(
    parts: &[(&rs::Pat, Ty, String)],
    ty: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    lets: Lets,
) -> Lift<(Ctx, String, Acc, Lets)> {
    let (whole, mut acc) = ctx.fresh(env, acc, "pair");
    let mut scope = ctx.clone();
    let mut lets = lets;
    for (pat, t, part) in parts {
        let (next, name, after, inner) = bind_pat(pat, t, env, &scope, acc, Vec::new())?;
        scope = next;
        acc = after;
        let value = match (ty, part.parse::<usize>()) {
            (Ty::Tuple(_) | Ty::Pair(_), Ok(i)) => tuple_item(ty, &whole, i),
            (Ty::Named(n), _) if env.types.boxed(n) => {
                format!("({} {whole})", Types::accessor(n, part))
            }
            _ => format!("(project {whole} {part})"),
        };
        lets.push((name, t.render(), value));
        lets.extend(inner);
    }
    Ok((scope, whole, acc, lets))
}

fn bind_pat(
    p: &rs::Pat,
    ty: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    lets: Lets,
) -> Lift<(Ctx, String, Acc, Lets)> {
    match (p, ty) {
        (rs::Pat::Ident(i), _) if i.subpat.is_none() && !i.by_ref => {
            let (scope, name, acc) = ctx.bind(env, acc, &i.ident, ty.clone());
            Ok((scope, name, acc, lets))
        }
        (rs::Pat::Wild, _) => {
            let (name, acc) = ctx.fresh(env, acc, "item");
            Ok((ctx.clone(), name, acc, lets))
        }
        (rs::Pat::Reference(r), _) => bind_pat(r, ty, env, ctx, acc, lets),
        (rs::Pat::Type(pt), _) => bind_pat(&pt.pat, ty, env, ctx, acc, lets),
        (rs::Pat::Tuple(tp), Ty::Tuple(ts) | Ty::Pair(ts)) if tp.len() == ts.len() => {
            let parts: Vec<(&rs::Pat, Ty, String)> = tp
                .iter()
                .zip(ts)
                .enumerate()
                .map(|(i, (e, t))| (e, t.clone(), format!("{i}")))
                .collect();
            bind_parts(&parts, ty, env, ctx, acc, lets)
        }
        // `S { a, b: p, .. }`: each named field's pattern, from the value.
        (rs::Pat::Struct(ps), Ty::Named(n)) if !ps.path.segments.is_empty() => {
            let fields = env
                .types
                .structs
                .get(n)
                .ok_or_else(|| Residue::Form("closure pattern".to_string()))?;
            let last = ps.path.segments.last().map(|s| env.types.resolve(&s.ident));
            if last.as_deref() != Some(n.as_str()) || (!ps.rest && ps.fields.len() != fields.len())
            {
                return Err(Residue::Form("closure pattern".to_string()));
            }
            let mut parts = Vec::new();
            for f in &ps.fields {
                let rs::Member::Named(m) = &f.member else {
                    return Err(Residue::Form("closure pattern".to_string()));
                };
                let field = local_name(m);
                let Some((_, t)) = fields.iter().find(|(name, _)| *name == field) else {
                    return Err(Residue::Form("closure pattern".to_string()));
                };
                parts.push((&f.pat, t.clone(), field));
            }
            bind_parts(&parts, ty, env, ctx, acc, lets)
        }
        _ => Err(Residue::Form("closure pattern".to_string())),
    }
}

/// A closure argument over parameters of these types.
/// The closure `|a0, ..| f(a0, ..)` of a function path `f` of `arity`
/// parameters; for a path `T::m` that names no lifted function, a method
/// of another type, `|a0, ..| a0.m(..)`.
fn applying(f: &rs::Path, arity: usize, env: &Env) -> rs::RustExpr {
    let names: Vec<String> = (0..arity).map(|i| format!("argument{i}")).collect();
    let local = |n: &String| {
        rs::RustExpr::Path(rs::Path {
            segments: vec![rs::PathSegment {
                ident: n.clone(),
                arguments: rs::PathArguments::None,
            }],
            text: n.clone(),
        })
    };
    rs::RustExpr::Closure(rs::ExprClosure {
        inputs: names
            .iter()
            .map(|n| {
                rs::Pat::Ident(rs::PatIdent {
                    ident: n.clone(),
                    by_ref: false,
                    subpat: None,
                })
            })
            .collect(),
        body: Box::new(applied_path(f, names.iter().map(local).collect(), env)),
        asyncness: false,
    })
}

/// A call of the function path `f` on `args`: [`applying`]'s body.
fn applied_path(f: &rs::Path, args: Vec<rs::RustExpr>, env: &Env) -> rs::RustExpr {
    let names: Vec<&String> = f.segments.iter().map(|s| &s.ident).collect();
    let lifted = match names.as_slice() {
        [.., owner, method] => env
            .fns
            .contains_key(&format!("{}::{method}", env.types.resolve(owner))),
        _ => true,
    };
    // `T::from` converts its argument: a call, not a method of it.
    let converts = names.last().is_some_and(|m| *m == "from");
    match (lifted || converts, names.last(), args.split_first()) {
        (false, Some(method), Some((receiver, rest))) => {
            rs::RustExpr::MethodCall(rs::ExprMethodCall {
                receiver: Box::new(receiver.clone()),
                method: (*method).clone(),
                turbofish: None,
                args: rest.to_vec(),
            })
        }
        _ => rs::RustExpr::Call(rs::ExprCall {
            func: Box::new(rs::RustExpr::Path(f.clone())),
            args,
        }),
    }
}

fn lambda(
    e: &rs::RustExpr,
    params: &[Ty],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<(Lambda, Acc)> {
    // A function given by its path is the closure that calls it.
    let named = match e {
        rs::RustExpr::Path(p) => Some(applying(p, params.len(), env)),
        _ => None,
    };
    let Some(rs::RustExpr::Closure(c)) = named.as_ref().or(Some(e)) else {
        return Err(Residue::Form(
            "an iterator adapter given a function, not a closure".to_string(),
        ));
    };
    if c.inputs.len() != params.len() || c.asyncness {
        return Err(Residue::Form("closure".to_string()));
    }
    let (mut acc, before) = acc.apart();
    let mut scope = Ctx {
        tails: BTreeMap::new(),
        top: false,
        looping: None,
        hidden: Vec::new(),
        elements: None,
        ..ctx.clone()
    };
    let mut binders = Vec::new();
    let mut lets = Vec::new();
    for (p, ty) in c.inputs.iter().zip(params) {
        let (mut next, name, after, more) = bind_pat(p, ty, env, &scope, acc, lets)?;
        // An element of a list that is a sub-term of a parameter is one.
        if let (Some(param), true) = (&ctx.elements, binders.is_empty()) {
            next.tails.insert(name.clone(), param.clone());
        }
        scope = next;
        acc = after;
        lets = more;
        binders.push(name);
    }
    // A `?` in a closure that gives an option leaves the closure with
    // `None`: the body is the match it stands for.
    let body = match want {
        Some(Ty::Option(_)) => closure_tries(&c.body, acc.given.len()),
        _ => (*c.body).clone(),
    };
    let ((core, ty), acc) = flat_of(&body, env, &scope, acc, want)?;
    let (acc, own) = acc.rejoin(before);
    if !own.descends.is_empty() && ctx.elements.is_none() {
        return Err(Residue::Form("recursion inside a closure".to_string()));
    }
    Ok((
        Lambda {
            binders,
            lets,
            core,
            ty,
            calls: own.calls,
            axioms: own.axioms,
            descends: own.descends,
        },
        acc,
    ))
}

/// A closure body giving an option, each `?` it evaluates unconditionally
/// written as the match that leaves with `None`, numbered from `n`.
fn closure_tries(body: &rs::RustExpr, n: usize) -> rs::RustExpr {
    let var = format!("tried{}", n + 1);
    let Some((found, rest)) = take_try(body, &var) else {
        return body.clone();
    };
    let held = rs::Pat::TupleStruct(rs::PatTupleStruct {
        path: rs::Path::ident("Some"),
        elems: vec![rs::Pat::Ident(rs::PatIdent {
            ident: var,
            by_ref: false,
            subpat: None,
        })],
    });
    let none = rs::Path::ident("None");
    rs::RustExpr::Match(rs::ExprMatch {
        expr: Box::new(found),
        arms: vec![
            rs::MatchArm {
                pat: held,
                body: Box::new(closure_tries(&rest, n + 1)),
            },
            rs::MatchArm {
                pat: rs::Pat::Path(none.clone()),
                body: Box::new(rs::RustExpr::Path(none)),
            },
        ],
    })
}

/// The outer locals a helper's body names: they become its parameters.
fn captures(ctx: &Ctx, texts: &[String], bound: &[String]) -> Vec<(String, Ty)> {
    let found: BTreeSet<(String, Ty)> = ctx
        .locals
        .values()
        .filter(|(n, _)| !bound.contains(n) && texts.iter().any(|t| mentions_word(t, n)))
        .cloned()
        .collect();
    found.into_iter().collect()
}

/// Whether a character continues a LexLean word.
fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `text` holds `word` as a whole word: an occurrence that no word
/// character continues on either side.
fn mentions_word(text: &str, word: &str) -> bool {
    #[cfg(not(uorust_lift))]
    crate::certified::certified!(lift_mentions_word, text, word);
    !word.is_empty()
        && word.chars().all(word_char)
        && text.contains(word)
        && apart(&text.split(word).collect::<Vec<&str>>())
}

/// Whether two consecutive pieces of a split meet where neither continues
/// a word: the separator between them stands alone.
fn apart(pieces: &[&str]) -> bool {
    pieces
        .iter()
        .zip(pieces.iter().skip(1))
        .any(|(before, after)| {
            !before.chars().last().is_some_and(word_char)
                && !after.chars().next().is_some_and(word_char)
        })
}

/// The recursive call of a walk, given the values it threads.
type Recur<'a> = &'a dyn Fn(&[String]) -> String;
/// A walk's step from its recursive call.
type WalkStep<'a> = &'a dyn Fn(Recur<'_>, &str) -> Expr;

/// A walk over a list: a helper that matches its first parameter and calls
/// itself on the tail. `lead` are parameters after the list that the walk
/// threads (a fold's accumulator); `cons` builds the step from the head's
/// binder and the recursive call's text given the lead values.
#[allow(clippy::too_many_arguments)]
fn walk(
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    kind: &str,
    list: &str,
    elem: &Ty,
    lam: &Lambda,
    head: &str,
    lead: &[(String, Ty, String)],
    result: &Ty,
    nil: String,
    cons: WalkStep<'_>,
) -> ((Expr, Ty), Acc) {
    let name = ctx.helper_name(env, &acc, kind);
    let (items, acc) = ctx.fresh(env, acc, "items");
    let (rest, acc) = ctx.fresh(env, acc, "rest");
    let mut bound: Vec<String> = lam.binders.clone();
    bound.extend(lam.lets.iter().map(|(n, _, _)| n.clone()));
    bound.extend(lead.iter().map(|(n, _, _)| n.clone()));
    let lets_text: Vec<String> = lam.lets.iter().map(|(_, _, v)| v.clone()).collect();
    let mut texts = vec![lam.core.clone()];
    texts.extend(lets_text);
    let caps = captures(ctx, &texts, &bound);
    let cap_names: Vec<String> = caps.iter().map(|(n, _)| n.clone()).collect();
    let recur = |values: &[String]| {
        let mut parts = vec![name.clone(), rest.clone()];
        parts.extend(values.iter().cloned());
        parts.extend(cap_names.iter().cloned());
        format!("({})", parts.join(" "))
    };
    let step = cons(&recur, &rest);
    let mut params = vec![(items.clone(), Ty::List(Box::new(elem.clone())).render())];
    params.extend(lead.iter().map(|(n, t, _)| (n.clone(), t.render())));
    params.extend(caps.iter().map(|(n, t)| (n.clone(), t.render())));
    let acc = acc.helper(Helper {
        name: name.clone(),
        params,
        result: result.render(),
        body: Expr::Match {
            scrutinee: items,
            arms: vec![
                Arm {
                    pattern: "List.nil".to_string(),
                    body: Expr::Flat(nil),
                },
                Arm {
                    pattern: format!("List.cons {head} {rest}"),
                    body: lam.wrap_expr(step),
                },
            ],
        },
        calls: lam.calls.clone(),
        axioms: lam.axioms.clone(),
        recursive: true,
        entwined: !lam.descends.is_empty(),
        termination: None,
    });
    let acc = lam
        .descends
        .iter()
        .fold(acc.called(format!("#{name}")), |acc, i| acc.descended(*i));
    let mut parts = vec![name, list.to_string()];
    parts.extend(lead.iter().map(|(_, _, v)| v.clone()));
    parts.extend(cap_names);
    (
        (Expr::Flat(format!("({})", parts.join(" "))), result.clone()),
        acc,
    )
}

/// A lifted value and the state after it, as the value of an optional lift.
fn some<T>((value, acc): (T, Acc)) -> (Option<T>, Acc) {
    (Some(value), acc)
}

/// Equality of two values of a type the lift compares.
fn equality(ty: &Ty, a: &str, b: &str) -> Option<String> {
    match ty {
        Ty::Nat(_) => Some(format!("({a} =? {b})")),
        Ty::Str | Ty::Char | Ty::Int(_) => Some(format!("(prim equal {a} {b} : Bool)")),
        _ => None,
    }
}

/// Equality of two values of `ty`: a primitive's, a unit-variant enum's
/// constructors, or else its derived order's `Equal`.
fn same_value(ty: &Ty, a: &str, b: &str, env: &Env, acc: Acc) -> Option<(String, Acc)> {
    if let Some(e) = equality(ty, a, b) {
        return Some((e, acc));
    }
    if is_enum_or_bool(ty, env) {
        return Some((same_constructor(ty, a, b), acc));
    }
    let test = crate::order::operator("==", ty, a, b)?;
    crate::order::derived(ty, &env.types).then(|| (test, acc.ordered(ty, &env.types)))
}

/// A search of a list for a needle: `contains` on a list or set (`entry`
/// `None`), or a map's `contains_key` and `get` over its entries (`entry`
/// the value type, `get` when `kind` is `Get`).
#[allow(clippy::too_many_arguments)]
fn lookup(
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    kind: &str,
    r: &str,
    elem: &Ty,
    value: Option<&Ty>,
    needle: &rs::RustExpr,
) -> Lift<((Expr, Ty), Acc)> {
    let key_ty = match (elem, value) {
        (Ty::Tuple(kv), Some(_)) => kv[0].clone(),
        _ => elem.clone(),
    };
    let ((v, vt), acc) = flat_of(needle, env, ctx, acc, Some(&key_ty))?;
    if !vt.same(&key_ty) {
        return Err(Residue::Form(format!("{kind} of another type")));
    }
    let (head, acc) = ctx.fresh(env, acc, "item");
    let (probe, acc) = ctx.fresh(env, acc, "needle");
    let subject = if value.is_some() {
        format!("(project {head} item0)")
    } else {
        head.clone()
    };
    let ordered = equality(&key_ty, &subject, &probe).is_none() && !is_enum_or_bool(&key_ty, env);
    let Some((test, acc)) = same_value(&key_ty, &subject, &probe, env, acc) else {
        return Err(Residue::Form(format!("{kind} over keys of this type")));
    };
    let lam = Lambda {
        binders: vec![head.clone()],
        lets: Vec::new(),
        core: test.clone(),
        ty: Ty::Bool,
        calls: BTreeSet::new(),
        axioms: if ordered {
            crate::order::axioms(&key_ty, &env.types)
        } else {
            BTreeSet::new()
        },
        descends: BTreeSet::new(),
    };
    let (result, nil, hit) = match value {
        Some(vt) if kind == "Get" => (
            Ty::Option(Box::new(vt.clone())),
            format!("(ctor Option.none [{}])", vt.render()),
            format!(
                "(ctor Option.some [{}] (project {head} item1))",
                vt.render()
            ),
        ),
        _ => (Ty::Bool, "false".to_string(), "true".to_string()),
    };
    let probe_arg = probe.clone();
    let cons = |rec: Recur<'_>, _: &str| {
        Expr::Flat(format!(
            "(if {test} then {hit} else {})",
            rec(std::slice::from_ref(&probe_arg))
        ))
    };
    Ok(walk(
        env,
        ctx,
        acc,
        kind,
        r,
        elem,
        &lam,
        &head,
        &[(probe, key_ty, v)],
        &result,
        nil,
        &cons,
    ))
}

/// The Rust name the lift binds a member under while it searches another
/// collection for it: no Rust identifier spells it.
const PROBE: &str = "#probe";

/// `a.is_disjoint(b)`: no member of `a` is a member of `b`, which must be
/// a local the search captures.
fn disjoint(
    r: &str,
    elem: &Ty,
    other: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let set = Ty::Set(Box::new(elem.clone()), true);
    let ((o, ot), acc) = flat_of(other, env, ctx, acc, Some(&set))?;
    let local = ctx.locals.values().any(|(n, _)| *n == o);
    if !local || !matches!(&ot, Ty::Set(t, _) | Ty::List(t) if t.same(elem)) {
        return Err(Residue::Form(".is_disjoint of another set".into()));
    }
    let (head, acc) = ctx.fresh(env, acc, "member");
    let mut probing = ctx.clone();
    probing
        .locals
        .insert(PROBE.into(), (head.clone(), elem.clone()));
    let probe = rs::RustExpr::Path(rs::Path {
        segments: vec![rs::PathSegment {
            ident: PROBE.into(),
            arguments: rs::PathArguments::None,
        }],
        text: PROBE.into(),
    });
    let before = acc.calls.clone();
    let ((test, _), acc) = lookup(env, &probing, acc, "Contains", &o, elem, None, &probe)?;
    let calls: BTreeSet<String> = acc.calls.difference(&before).cloned().collect();
    let axioms: BTreeSet<String> = acc
        .helpers
        .iter()
        .filter(|h| calls.contains(&format!("#{}", h.name)))
        .flat_map(|h| h.axioms.iter().cloned())
        .collect();
    let core = test.flat();
    let lam = Lambda {
        binders: vec![head.clone()],
        lets: Vec::new(),
        core: core.clone(),
        ty: Ty::Bool,
        calls,
        axioms,
        descends: BTreeSet::new(),
    };
    let cons =
        |rec: Recur<'_>, _: &str| Expr::Flat(format!("(if {core} then false else {})", rec(&[])));
    Ok(walk(
        env,
        ctx,
        acc,
        "Disjoint",
        r,
        elem,
        &lam,
        &head,
        &[],
        &Ty::Bool,
        "true".into(),
        &cons,
    ))
}

/// Reads of a map or a set, held as the list of its entries or members:
/// `get`, `contains_key`, `contains`, `len` and `is_empty`, and, where the
/// order is the key order, `iter`, `keys` and `values`.
fn collection(
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    let flat = |s: String, t: Ty, acc: Acc| Ok(((Expr::Flat(s), t), acc));
    let unordered = || {
        Err(Residue::Form(format!(
            ".{name} on a hash collection, whose order is unspecified"
        )))
    };
    match (rt, name, args) {
        (_, "clone", []) => flat(r.to_string(), rt.clone(), acc),
        (_, "len", []) => flat(format!("(prim length {r} : Nat)"), Ty::Nat(None), acc),
        (_, "is_empty", []) => flat(format!("((prim length {r} : Nat) =? 0)"), Ty::Bool, acc),
        (Ty::Map(_, v, _), "get", [k]) => lookup(env, ctx, acc, "Get", r, &rt.entry(), Some(v), k),
        (Ty::Map(_, v, _), "contains_key", [k]) => {
            lookup(env, ctx, acc, "Has", r, &rt.entry(), Some(v), k)
        }
        (Ty::Set(t, _), "contains", [k]) => lookup(env, ctx, acc, "Contains", r, t, None, k),
        (Ty::Set(t, _), "is_disjoint", [other]) => disjoint(r, t, other, env, ctx, acc),
        (Ty::Map(..), "iter" | "into_iter", []) => match rt {
            Ty::Map(_, _, true) => flat(r.to_string(), Ty::List(Box::new(rt.entry())), acc),
            _ => unordered(),
        },
        (Ty::Set(t, sorted), "iter" | "into_iter", []) => {
            if *sorted {
                flat(r.to_string(), Ty::List(t.clone()), acc)
            } else {
                unordered()
            }
        }
        (Ty::Map(k, v, sorted), "keys" | "values", []) => {
            if !*sorted {
                return unordered();
            }
            let (field, ty) = if name == "keys" {
                ("item0", k)
            } else {
                ("item1", v)
            };
            let (head, acc) = ctx.fresh(env, acc, "entry");
            let core = format!("(project {head} {field})");
            let lam = Lambda {
                binders: vec![head.clone()],
                lets: Vec::new(),
                core: core.clone(),
                ty: (**ty).clone(),
                calls: BTreeSet::new(),
                axioms: BTreeSet::new(),
                descends: BTreeSet::new(),
            };
            let cons = |rec: Recur<'_>, _: &str| Expr::Flat(format!("(cons {core} {})", rec(&[])));
            let kind = if name == "keys" { "Keys" } else { "Values" };
            Ok(walk(
                env,
                ctx,
                acc,
                kind,
                r,
                &rt.entry(),
                &lam,
                &head,
                &[],
                &Ty::List(ty.clone()),
                format!("(nil {})", ty.render()),
                &cons,
            ))
        }
        _ => Err(Residue::Form(format!("method call .{name}"))),
    }
}

/// A list combinator's value with nothing left to lift: its term and type.
fn flat_some(s: String, t: Ty, acc: Acc) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    Ok((Some((Expr::Flat(s), t)), acc))
}

/// Iterator adapters and consumers over a list, with pure closures:
/// `iter`, `into_iter`, `map`, `filter`, `any`, `all`, `find`, `fold`,
/// `count`, `contains`, `collect` and `join`. `None` when the method is none
/// of them.
#[allow(clippy::too_many_arguments)]
fn combinator(
    m: &rs::ExprMethodCall,
    name: &str,
    r: &str,
    elem: &Ty,
    args: &[&rs::RustExpr],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let list = Ty::List(Box::new(elem.clone()));
    let flat = |s: String, t: Ty, acc: Acc| Ok((Some((Expr::Flat(s), t)), acc));
    match (name, args) {
        ("iter" | "into_iter", []) => flat(r.to_string(), list, acc),
        ("collect", []) => comb_collect(m, r, elem, &list, acc),
        ("count", []) => flat(format!("(prim length {r} : Nat)"), Ty::Nat(None), acc),
        ("join", [sep]) if *elem == Ty::Str => comb_join(sep, r, env, ctx, acc),
        // `a.zip(b)`: the pairs of items at the same place, as long as the
        // shorter.
        ("zip", [other]) => comb_zip(other, r, elem, &list, env, ctx, acc),
        ("contains", [needle]) => Ok(some(lookup(
            env, ctx, acc, "Contains", r, elem, None, needle,
        )?)),
        ("map" | "filter" | "any" | "all" | "find", [f]) => {
            comb_map(f, name, r, elem, &list, env, ctx, acc)
        }
        ("fold", [init, f]) => comb_fold(init, f, r, elem, env, ctx, acc),
        ("next" | "first", []) => flat(
            format!("(prim index {r} 0 : (Option {}))", elem.render()),
            Ty::Option(Box::new(elem.clone())),
            acc,
        ),
        ("last", []) => flat(
            format!(
                "(prim index {r} (prim subtract (prim length {r} : Nat) 1 : Nat) : (Option {}))",
                elem.render()
            ),
            Ty::Option(Box::new(elem.clone())),
            acc,
        ),
        ("nth", [n]) => comb_nth(n, r, elem, env, ctx, acc),
        ("chain", [other]) => comb_chain(other, r, &list, env, ctx, acc),
        ("rev" | "enumerate", []) => comb_rev(name, r, elem, &list, env, ctx, acc),
        ("skip" | "take", [n]) => comb_skip(n, name, r, elem, &list, env, ctx, acc),
        ("take_while" | "skip_while" | "position", [f]) => {
            comb_while(f, name, r, elem, &list, env, ctx, acc)
        }
        ("filter_map" | "find_map" | "flat_map", [f]) => {
            comb_filter_map(f, name, r, elem, env, ctx, acc)
        }
        _ => Ok((None, acc)),
    }
}

/// `.collect()`: the list itself, or, into a `String`, its pieces joined.
fn comb_collect(
    m: &rs::ExprMethodCall,
    r: &str,
    elem: &Ty,
    list: &Ty,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let to_string = matches!(
        m.turbofish.as_deref(),
        Some([rs::GenericArgument::Type(t)]) if t.text == "String"
    );
    if to_string {
        // A `char` is held as the one-character string it is.
        if !matches!(elem, Ty::Str | Ty::Char) {
            return Err(Residue::Form("collect of non-strings into a String".into()));
        }
        let acc = acc.axiom("propext");
        return flat_some(format!("(prim join {r} \"\" : String)"), Ty::Str, acc);
    }
    flat_some(r.to_string(), list.clone(), acc)
}

/// `.join(sep)` of strings.
fn comb_join(
    sep: &rs::RustExpr,
    r: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let ((s, st), acc) = flat_of(sep, env, ctx, acc, Some(&Ty::Str))?;
    if st != Ty::Str {
        return Err(Residue::Form(
            "join with a separator of another type".into(),
        ));
    }
    let acc = acc.axiom("propext");
    flat_some(format!("(prim join {r} {s} : String)"), Ty::Str, acc)
}

/// `a.zip(b)`: the pairs of items at the same place, as long as the shorter.
fn comb_zip(
    other: &rs::RustExpr,
    r: &str,
    elem: &Ty,
    list: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let ((o, ot), acc) = flat_of(other, env, ctx, acc, None)?;
    let oe = match &ot {
        Ty::List(t) | Ty::Set(t, true) => (**t).clone(),
        Ty::Map(_, _, true) => ot.entry(),
        _ => {
            return Err(Residue::Form(
                ".zip with something other than a list".into(),
            ))
        }
    };
    let pair = env.types.tuple(vec![elem.clone(), oe.clone()]);
    let pairs = Ty::List(Box::new(pair.clone()));
    let name = ctx.helper_name(env, &acc, "Zip");
    let (items, acc) = ctx.fresh(env, acc, "items");
    let (others, acc) = ctx.fresh(env, acc, "others");
    let (head, acc) = ctx.fresh(env, acc, "item");
    let (rest, acc) = ctx.fresh(env, acc, "rest");
    let (partner, acc) = ctx.fresh(env, acc, "partner");
    let (more, acc) = ctx.fresh(env, acc, "more");
    let value = tuple_value(&pair, &[head.clone(), partner.clone()]);
    let body = Expr::Match {
        scrutinee: items.clone(),
        arms: vec![
            Arm {
                pattern: "List.nil".into(),
                body: Expr::Flat(format!("(nil {})", pair.render())),
            },
            Arm {
                pattern: format!("List.cons {head} {rest}"),
                body: Expr::Match {
                    scrutinee: others.clone(),
                    arms: vec![
                        Arm {
                            pattern: "List.nil".into(),
                            body: Expr::Flat(format!("(nil {})", pair.render())),
                        },
                        Arm {
                            pattern: format!("List.cons {partner} {more}"),
                            body: Expr::Flat(format!("(cons {value} ({name} {rest} {more}))")),
                        },
                    ],
                },
            },
        ],
    };
    let acc = acc.saw(&pair).helper(Helper {
        name: name.clone(),
        params: vec![
            (items, list.render()),
            (others, Ty::List(Box::new(oe)).render()),
        ],
        result: pairs.render(),
        body,
        calls: BTreeSet::new(),
        axioms: BTreeSet::new(),
        recursive: true,
        entwined: false,
        termination: None,
    });
    let acc = acc.called(format!("#{name}"));
    flat_some(format!("({name} {r} {o})"), pairs, acc)
}

/// `.map(f)`, `.filter(p)`, `.any(p)`, `.all(p)` and `.find(p)`.
#[allow(clippy::too_many_arguments)]
fn comb_map(
    f: &rs::RustExpr,
    name: &str,
    r: &str,
    elem: &Ty,
    list: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let want = if name == "map" { None } else { Some(&Ty::Bool) };
    // A list that is a sub-term of a parameter: the closure may
    // recurse on its elements.
    let ranged = Ctx {
        elements: ctx.tails.get(r).cloned(),
        ..ctx.clone()
    };
    let (lam, acc) = lambda(f, std::slice::from_ref(elem), env, &ranged, acc, want)?;
    if name != "map" && lam.ty != Ty::Bool {
        return Err(Residue::Form(format!(
            "a .{name} predicate of another type"
        )));
    }
    let head = lam.binders[0].clone();
    let core = lam.core.clone();
    let (kind, result, nil): (&str, Ty, String) = match name {
        "map" => (
            "Map",
            Ty::List(Box::new(lam.ty.clone())),
            format!("(nil {})", lam.ty.render()),
        ),
        "filter" => ("Filter", list.clone(), format!("(nil {})", elem.render())),
        "any" => ("Any", Ty::Bool, "false".into()),
        "all" => ("All", Ty::Bool, "true".into()),
        _ => (
            "Find",
            Ty::Option(Box::new(elem.clone())),
            format!("(ctor Option.none [{}])", elem.render()),
        ),
    };
    let elem_text = elem.render();
    let cons = |rec: Recur<'_>, _: &str| -> Expr {
        let next = rec(&[]);
        Expr::Flat(match name {
            "map" => format!("(cons {core} {next})"),
            "filter" => format!("(if {core} then (cons {head} {next}) else {next})"),
            "any" => format!("(if {core} then true else {next})"),
            "all" => format!("(if {core} then {next} else false)"),
            _ => format!("(if {core} then (ctor Option.some [{elem_text}] {head}) else {next})"),
        })
    };
    Ok(some(walk(
        env,
        ctx,
        acc,
        kind,
        r,
        elem,
        &lam,
        &head,
        &[],
        &result,
        nil,
        &cons,
    )))
}

/// `.fold(init, f)`.
fn comb_fold(
    init: &rs::RustExpr,
    f: &rs::RustExpr,
    r: &str,
    elem: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let ((iv, it), acc) = flat_of(init, env, ctx, acc, None)?;
    let (lam, acc) = lambda(f, &[it.clone(), elem.clone()], env, ctx, acc, Some(&it))?;
    if !lam.ty.same(&it) {
        return Err(Residue::Form("a fold step of another type".into()));
    }
    let folded = lam.binders[0].clone();
    let head = lam.binders[1].clone();
    let core = lam.core.clone();
    let cons = |rec: Recur<'_>, _: &str| Expr::Flat(rec(std::slice::from_ref(&core)));
    Ok(some(walk(
        env,
        ctx,
        acc,
        "Fold",
        r,
        elem,
        &lam,
        &head,
        &[(folded.clone(), it.clone(), iv)],
        &it,
        folded,
        &cons,
    )))
}

/// `.nth(n)`.
fn comb_nth(
    n: &rs::RustExpr,
    r: &str,
    elem: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let ((n, nt), acc) = flat_of(n, env, ctx, acc, Some(&Ty::Nat(None)))?;
    if !matches!(nt, Ty::Nat(_)) {
        return Err(Residue::Form("nth of another type".into()));
    }
    flat_some(
        format!("(prim index {r} {n} : (Option {}))", elem.render()),
        Ty::Option(Box::new(elem.clone())),
        acc,
    )
}

/// `.chain(other)`.
fn comb_chain(
    other: &rs::RustExpr,
    r: &str,
    list: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let ((o, ot), acc) = flat_of(other, env, ctx, acc, Some(list))?;
    if !ot.same(list) {
        return Err(Residue::Form("chain of another type".into()));
    }
    flat_some(
        format!("(prim append {r} {o} : {})", list.render()),
        list.clone(),
        acc,
    )
}

/// `.rev()` and `.enumerate()`.
#[allow(clippy::too_many_arguments)]
fn comb_rev(
    name: &str,
    r: &str,
    elem: &Ty,
    list: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let (head, acc) = ctx.fresh(env, acc, "item");
    let lam = Lambda {
        binders: vec![head.clone()],
        lets: Vec::new(),
        core: head.clone(),
        ty: elem.clone(),
        calls: BTreeSet::new(),
        axioms: BTreeSet::new(),
        descends: BTreeSet::new(),
    };
    if name == "rev" {
        let e = elem.render();
        let cons = |rec: Recur<'_>, _: &str| {
            Expr::Flat(format!(
                "(prim append {} (cons {head} (nil {e})) : (List {e}))",
                rec(&[])
            ))
        };
        return Ok(some(walk(
            env,
            ctx,
            acc,
            "Rev",
            r,
            elem,
            &lam,
            &head,
            &[],
            list,
            format!("(nil {e})"),
            &cons,
        )));
    }
    let pair = env.types.tuple(vec![Ty::Nat(None), elem.clone()]);
    let acc = acc.saw(&pair);
    let (index, acc) = ctx.fresh(env, acc, "index");
    let pair_name = pair.render();
    let item = tuple_value(&pair, &[index.clone(), head.clone()]);
    let cons = |rec: Recur<'_>, _: &str| {
        Expr::Flat(format!(
            "(cons {item} {})",
            rec(&[format!("({index} + 1)")])
        ))
    };
    Ok(some(walk(
        env,
        ctx,
        acc,
        "Enumerate",
        r,
        elem,
        &lam,
        &head,
        &[(index.clone(), Ty::Nat(None), "0".into())],
        &Ty::List(Box::new(pair.clone())),
        format!("(nil {pair_name})"),
        &cons,
    )))
}

/// `.skip(n)` and `.take(n)`.
#[allow(clippy::too_many_arguments)]
fn comb_skip(
    n: &rs::RustExpr,
    name: &str,
    r: &str,
    elem: &Ty,
    list: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let ((n, nt), acc) = flat_of(n, env, ctx, acc, Some(&Ty::Nat(None)))?;
    if !matches!(nt, Ty::Nat(_)) {
        return Err(Residue::Form(format!(".{name} of another type")));
    }
    let (head, acc) = ctx.fresh(env, acc, "item");
    let (count, acc) = ctx.fresh(env, acc, "count");
    let lam = Lambda {
        binders: vec![head.clone()],
        lets: Vec::new(),
        core: head.clone(),
        ty: elem.clone(),
        calls: BTreeSet::new(),
        axioms: BTreeSet::new(),
        descends: BTreeSet::new(),
    };
    let e = elem.render();
    let take = name == "take";
    let cons = |rec: Recur<'_>, _: &str| {
        let fewer = rec(&[format!("(prim subtract {count} 1 : Nat)")]);
        let same = rec(std::slice::from_ref(&count));
        Expr::Flat(if take {
            format!("(if ({count} =? 0) then (nil {e}) else (cons {head} {fewer}))")
        } else {
            format!("(if ({count} =? 0) then (cons {head} {same}) else {fewer})")
        })
    };
    let kind = if take { "Take" } else { "Skip" };
    Ok(some(walk(
        env,
        ctx,
        acc,
        kind,
        r,
        elem,
        &lam,
        &head,
        &[(count.clone(), Ty::Nat(None), n)],
        list,
        format!("(nil {e})"),
        &cons,
    )))
}

/// `.take_while(p)`, `.skip_while(p)` and `.position(p)`.
#[allow(clippy::too_many_arguments)]
fn comb_while(
    f: &rs::RustExpr,
    name: &str,
    r: &str,
    elem: &Ty,
    list: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let (lam, acc) = lambda(
        f,
        std::slice::from_ref(elem),
        env,
        ctx,
        acc,
        Some(&Ty::Bool),
    )?;
    if lam.ty != Ty::Bool {
        return Err(Residue::Form(format!(
            "a .{name} predicate of another type"
        )));
    }
    let head = lam.binders[0].clone();
    let core = lam.core.clone();
    let e = elem.render();
    if name == "position" {
        let (index, acc) = ctx.fresh(env, acc, "index");
        let cons = |rec: Recur<'_>, _: &str| {
            Expr::Flat(format!(
                "(if {core} then (ctor Option.some [Nat] {index}) else {})",
                rec(&[format!("({index} + 1)")])
            ))
        };
        return Ok(some(walk(
            env,
            ctx,
            acc,
            "Position",
            r,
            elem,
            &lam,
            &head,
            &[(index.clone(), Ty::Nat(None), "0".into())],
            &Ty::Option(Box::new(Ty::Nat(None))),
            "(ctor Option.none [Nat])".into(),
            &cons,
        )));
    }
    let take = name == "take_while";
    let cons = |rec: Recur<'_>, rest: &str| {
        let next = rec(&[]);
        Expr::Flat(if take {
            format!("(if {core} then (cons {head} {next}) else (nil {e}))")
        } else {
            format!("(if {core} then {next} else (cons {head} {rest}))")
        })
    };
    let kind = if take { "TakeWhile" } else { "SkipWhile" };
    Ok(some(walk(
        env,
        ctx,
        acc,
        kind,
        r,
        elem,
        &lam,
        &head,
        &[],
        list,
        format!("(nil {e})"),
        &cons,
    )))
}

/// `.filter_map(f)`, `.find_map(f)` and `.flat_map(f)`.
fn comb_filter_map(
    f: &rs::RustExpr,
    name: &str,
    r: &str,
    elem: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(Option<(Expr, Ty)>, Acc)> {
    let (lam, acc) = lambda(f, std::slice::from_ref(elem), env, ctx, acc, None)?;
    let inner = match (&lam.ty, name) {
        (Ty::Option(u), "filter_map" | "find_map") | (Ty::List(u), "flat_map") => (**u).clone(),
        _ => return Err(Residue::Form(format!("a .{name} step of another type"))),
    };
    let head = lam.binders[0].clone();
    let core = lam.core.clone();
    let (found, acc) = ctx.fresh(env, acc, "found");
    let u = inner.render();
    let (kind, result, nil) = match name {
        "filter_map" => (
            "FilterMap",
            Ty::List(Box::new(inner.clone())),
            format!("(nil {u})"),
        ),
        "find_map" => (
            "FindMap",
            Ty::Option(Box::new(inner.clone())),
            format!("(ctor Option.none [{u}])"),
        ),
        _ => (
            "FlatMap",
            Ty::List(Box::new(inner.clone())),
            format!("(nil {u})"),
        ),
    };
    let cons = |rec: Recur<'_>, _: &str| {
        let next = rec(&[]);
        match name {
            "flat_map" => Expr::Flat(format!("(prim append {core} {next} : (List {u}))")),
            _ => Expr::Match {
                scrutinee: core.clone(),
                arms: vec![
                    Arm {
                        pattern: "Option.none".into(),
                        body: Expr::Flat(next.clone()),
                    },
                    Arm {
                        pattern: format!("Option.some {found}"),
                        body: Expr::Flat(if name == "find_map" {
                            format!("(ctor Option.some [{u}] {found})")
                        } else {
                            format!("(cons {found} {next})")
                        }),
                    },
                ],
            },
        }
    };
    Ok(some(walk(
        env,
        ctx,
        acc,
        kind,
        r,
        elem,
        &lam,
        &head,
        &[],
        &result,
        nil,
        &cons,
    )))
}

/// A match arm's body: an expression, or the block of an `if let`.
#[derive(Clone, Copy)]
enum Body<'a> {
    Expr(&'a rs::RustExpr),
    Block(&'a rs::Block),
    /// An arm with a guard: the guard, then the arm's expression.
    Guarded(&'a rs::RustExpr, &'a rs::RustExpr),
}

fn body(b: Body<'_>, env: &Env, ctx: &Ctx, acc: Acc, want: Option<&Ty>) -> Lift<((Expr, Ty), Acc)> {
    match b {
        Body::Expr(e) | Body::Guarded(_, e) => expr(e, env, ctx, acc, want),
        Body::Block(b) => block(&b.stmts, env, ctx, acc, want),
    }
}

/// The constructors of a scrutinee type: `(Rust shape key, LexLean
/// constructor, binder types)`.
fn constructors(t: &Ty, types: &Types) -> Option<Vec<(String, String, Vec<Ty>)>> {
    Some(match t {
        Ty::Named(n) if types.datas.contains_key(n) => types.datas[n]
            .iter()
            .map(|(v, fields)| {
                (
                    v.clone(),
                    format!("{n}.{v}"),
                    fields.iter().map(|(_, t)| t.clone()).collect(),
                )
            })
            .collect(),
        Ty::Named(n) => {
            let vs = types.enums.get(n)?;
            vs.iter()
                .map(|v| (v.clone(), format!("{n}.{v}"), Vec::new()))
                .collect()
        }
        Ty::Option(inner) => vec![
            ("None".into(), "Option.none".into(), Vec::new()),
            ("Some".into(), "Option.some".into(), vec![(**inner).clone()]),
        ],
        Ty::Outcome(ok, err) => vec![
            ("Ok".into(), "Result.ok".into(), vec![(**ok).clone()]),
            ("Err".into(), "Result.error".into(), vec![(**err).clone()]),
        ],
        Ty::List(inner) => vec![
            ("[]".into(), "List.nil".into(), Vec::new()),
            (
                "[..]".into(),
                "List.cons".into(),
                vec![(**inner).clone(), t.clone()],
            ),
        ],
        _ => return None,
    })
}

/// A cell of the match compiler: one Rust pattern, a wildcard the compiler
/// fills in, or the elements of a slice pattern after its head.
#[derive(Clone, Copy, PartialEq)]
enum Cell<'a> {
    Pat(&'a rs::Pat),
    Wild,
    Elems(&'a [rs::Pat]),
}

/// What a cell tests of its column, once the names it binds are taken.
enum Head<'a> {
    /// Every value.
    Any,
    /// A constructor, by its Rust key, with a cell for each field.
    Ctor(String, Vec<Cell<'a>>),
    /// A tuple's or a structure's items, in order.
    Items(Vec<Cell<'a>>),
    /// A literal or a range, tested on the value.
    Test(&'a rs::Pat),
    /// Alternatives.
    Or(Vec<Cell<'a>>),
}

/// A clause (a row) of the match compiler: a cell per column, the Rust names its
/// cells bind (with the value and type each names), and its arm.
#[derive(Clone)]
struct Clause<'a> {
    /// The arm the row is of: the alternatives of an or-pattern are rows
    /// of one arm.
    arm: usize,
    cells: Vec<Cell<'a>>,
    binds: Vec<(String, String, Ty)>,
    body: Body<'a>,
}

/// A column of the match compiler: the value it tests, and its type.
#[derive(Clone)]
struct Column {
    value: String,
    ty: Ty,
}

/// The Rust key of a constructor path: `Some`, `Ok`, `Err`, `None`, or the
/// variant's own name.
fn ctor_key(path: &rs::Path) -> String {
    match wrapper(path) {
        Some(w) => w.to_string(),
        None => path
            .segments
            .last()
            .map(|s| s.ident.clone())
            .unwrap_or_default(),
    }
}

/// Whether a slice element is `..` or `name @ ..`.
fn is_rest(p: &rs::Pat) -> bool {
    match p {
        rs::Pat::Rest => true,
        rs::Pat::Ident(i) => matches!(i.subpat.as_deref(), Some(rs::Pat::Rest)),
        _ => false,
    }
}

/// The head of the elements of a slice pattern: `[]`, the rest, or the
/// first element and the elements after it.
fn slice_head(elems: &[rs::Pat]) -> Option<(Vec<String>, Head<'_>)> {
    match elems {
        [] => Some((Vec::new(), Head::Ctor("[]".into(), Vec::new()))),
        [rs::Pat::Rest] => Some((Vec::new(), Head::Any)),
        [rs::Pat::Ident(i)] if is_rest(&elems[0]) => Some((vec![i.ident.clone()], Head::Any)),
        [first, rest @ ..] => {
            if is_rest(first) {
                return None;
            }
            let cells = vec![Cell::Pat(first), Cell::Elems(rest)];
            Some((Vec::new(), Head::Ctor("[..]".into(), cells)))
        }
    }
}

/// The cells of a struct pattern, a field's in the order `names` gives
/// (LexLean names): the pattern given for it, or a wildcard where `..`
/// leaves it out.
fn struct_cells<'a>(ps: &'a rs::PatStruct, names: &[String]) -> Option<Vec<Cell<'a>>> {
    let mut cells = Vec::new();
    for name in names {
        let given = ps
            .fields
            .iter()
            .find(|f| matches!(&f.member, rs::Member::Named(n) if local_name(n) == *name));
        match given {
            Some(f) => cells.push(Cell::Pat(&f.pat)),
            None if ps.rest => cells.push(Cell::Wild),
            None => return None,
        }
    }
    let used = cells.iter().filter(|c| matches!(c, Cell::Pat(_))).count();
    (used == ps.fields.len()).then_some(cells)
}

/// The names a cell binds and what it tests, in a column of type `ty`.
fn head<'a>(cell: Cell<'a>, ty: &Ty, types: &Types) -> Option<(Vec<String>, Head<'a>)> {
    match cell {
        Cell::Wild => Some((Vec::new(), Head::Any)),
        Cell::Elems(elems) => slice_head(elems),
        Cell::Pat(p) => pat_head(p, ty, types),
    }
}

fn pat_head<'a>(p: &'a rs::Pat, ty: &Ty, types: &Types) -> Option<(Vec<String>, Head<'a>)> {
    match p {
        rs::Pat::Wild | rs::Pat::Rest => Some((Vec::new(), Head::Any)),
        rs::Pat::Reference(inner) => pat_head(inner, ty, types),
        rs::Pat::Type(t) => pat_head(&t.pat, ty, types),
        rs::Pat::Ident(i) => match &i.subpat {
            Some(sub) => {
                let (names, h) = pat_head(sub, ty, types)?;
                let mut all = vec![i.ident.clone()];
                all.extend(names);
                Some((all, h))
            }
            None => {
                let unit = constructors(ty, types).is_some_and(|cs| {
                    cs.iter()
                        .any(|(k, _, fields)| *k == i.ident && fields.is_empty())
                });
                if unit && i.ident.starts_with(char::is_uppercase) {
                    Some((Vec::new(), Head::Ctor(i.ident.clone(), Vec::new())))
                } else {
                    Some((vec![i.ident.clone()], Head::Any))
                }
            }
        },
        rs::Pat::Path(path) => Some((Vec::new(), Head::Ctor(ctor_key(path), Vec::new()))),
        rs::Pat::Lit(rs::Lit::Bool(b)) => Some((Vec::new(), Head::Ctor(b.to_string(), Vec::new()))),
        rs::Pat::Lit(_) | rs::Pat::Range(_) => Some((Vec::new(), Head::Test(p))),
        rs::Pat::Tuple(items) => Some((
            Vec::new(),
            Head::Items(items.iter().map(Cell::Pat).collect()),
        )),
        rs::Pat::Slice(elems) => slice_head(elems),
        rs::Pat::Or(alts) => Some((Vec::new(), Head::Or(alts.iter().map(Cell::Pat).collect()))),
        rs::Pat::TupleStruct(ts) => {
            let key = ctor_key(&ts.path);
            let ctors = constructors(ty, types)?;
            let (_, _, fields) = ctors.iter().find(|(k, _, _)| *k == key)?;
            let arity = fields.len();
            let cells: Vec<Cell<'a>> =
                match ts.elems.iter().position(|e| matches!(e, rs::Pat::Rest)) {
                    None => ts.elems.iter().map(Cell::Pat).collect(),
                    Some(at) => {
                        let before = &ts.elems[..at];
                        let after = &ts.elems[at + 1..];
                        let given = before.len() + after.len();
                        if given > arity {
                            return None;
                        }
                        before
                            .iter()
                            .map(Cell::Pat)
                            .chain(std::iter::repeat_n(Cell::Wild, arity - given))
                            .chain(after.iter().map(Cell::Pat))
                            .collect()
                    }
                };
            (cells.len() == arity).then_some((Vec::new(), Head::Ctor(key, cells)))
        }
        rs::Pat::Struct(ps) => {
            let Ty::Named(n) = ty else { return None };
            if let Some(variants) = types.datas.get(n) {
                let key = ctor_key(&ps.path);
                let (_, fields) = variants.iter().find(|(v, _)| *v == key)?;
                let names: Vec<String> = fields
                    .iter()
                    .map(|(name, _)| name.as_deref().map(local_name).unwrap_or_default())
                    .collect();
                let cells = struct_cells(ps, &names)?;
                return Some((Vec::new(), Head::Ctor(key, cells)));
            }
            let fields = types.structs.get(n)?;
            let names: Vec<String> = fields.iter().map(|(f, _)| f.clone()).collect();
            Some((Vec::new(), Head::Items(struct_cells(ps, &names)?)))
        }
        rs::Pat::Guard(_) | rs::Pat::Other => None,
    }
}

/// The row's heads at column `j`, one row per alternative of an or-pattern,
/// each with the names the cell binds there.
fn heads_at<'a>(
    row: &Clause<'a>,
    j: usize,
    col: &Column,
    types: &Types,
) -> Option<Vec<(Clause<'a>, Head<'a>)>> {
    let (names, h) = head(row.cells[j], &col.ty, types)?;
    let mut row = row.clone();
    row.binds.extend(
        names
            .into_iter()
            .map(|n| (n, col.value.clone(), col.ty.clone())),
    );
    match h {
        Head::Or(alts) => {
            let mut out = Vec::new();
            for alt in alts {
                let mut one = row.clone();
                one.cells[j] = alt;
                out.extend(heads_at(&one, j, col, types)?);
            }
            Some(out)
        }
        h => Some(vec![(row, h)]),
    }
}

/// The test that a value `s` of a literal type `st` matches a literal or
/// range pattern, and the state with the axioms the test elaborates with.
fn literal_test(s: &str, st: &Ty, p: &rs::Pat, acc: Acc) -> Option<(String, Acc)> {
    let constant = |e: &rs::RustExpr| -> Option<String> {
        match e {
            rs::RustExpr::Lit(l) => match literal_of(l, Some(st), false) {
                Ok((Expr::Flat(k), kt)) if kt.same(st) => Some(k),
                _ => None,
            },
            rs::RustExpr::Unary(u) if matches!(u.op, rs::UnOp::Neg) => match &*u.expr {
                rs::RustExpr::Lit(l) => match literal_of(l, Some(st), true) {
                    Ok((Expr::Flat(k), kt)) if kt.same(st) => Some(k),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    };
    let at_most = |a: &str, b: &str| -> Option<String> {
        match st {
            Ty::Nat(_) => Some(format!("({a} <=? {b})")),
            Ty::Int(_) | Ty::Str | Ty::Char => {
                Some(format!("(not (prim less_than {b} {a} : Bool))"))
            }
            _ => None,
        }
    };
    // The axioms an order test elaborates with.
    let ordered = |acc: Acc| match st {
        Ty::Int(_) => acc.axiom("propext"),
        Ty::Str | Ty::Char => acc.all_axioms(),
        _ => acc,
    };
    match p {
        rs::Pat::Lit(l) => {
            let k = constant(&rs::RustExpr::Lit(l.clone()))?;
            let test = match st {
                Ty::Nat(_) => format!("({s} =? {k})"),
                Ty::Int(_) | Ty::Str | Ty::Char => format!("(prim equal {s} {k} : Bool)"),
                _ => return None,
            };
            Some((test, acc))
        }
        rs::Pat::Range(rg) => {
            let mut acc = acc;
            let mut parts = Vec::new();
            if let Some(e) = &rg.start {
                let lo = constant(e)?;
                parts.push(at_most(&lo, s)?);
                acc = ordered(acc);
            }
            if let Some(e) = &rg.end {
                let hi = constant(e)?;
                let c = if rg.closed {
                    at_most(s, &hi)?
                } else {
                    format!("(not {})", at_most(&hi, s)?)
                };
                parts.push(c);
                acc = ordered(acc);
            }
            match parts.as_slice() {
                [one] => Some((one.clone(), acc)),
                [a, b] => Some((format!("({a} && {b})"), acc)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A match: its scrutinee once, then its arms compiled to LexLean matches
/// on one constructor at a time and `if` tests on literals and guards.
fn matching<'a>(
    scrutinee: &rs::RustExpr,
    arms: &[(&'a rs::Pat, Body<'a>)],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    // `xs.split_first()`: a match on the slice itself, so a recursive call
    // on its rest descends.
    if let Some((slice, pats)) = split_first(scrutinee, arms) {
        let arms: Vec<(&rs::Pat, Body<'_>)> =
            pats.iter().zip(arms).map(|(p, (_, b))| (p, *b)).collect();
        return matching(&slice, &arms, env, ctx, acc, want);
    }
    // A tuple written in place is its items, a column each.
    let parts: Vec<&rs::RustExpr> = match scrutinee {
        rs::RustExpr::Tuple(items) if items.len() >= 2 => items.iter().collect(),
        one => vec![one],
    };
    let mut cols = Vec::new();
    let mut acc = acc;
    for part in &parts {
        let ((value, ty), after) = flat_of(part, env, &ctx.below(), acc, None)?;
        acc = after;
        cols.push(Column { value, ty });
    }
    let spread = |p: &'a rs::Pat| -> Vec<Cell<'a>> {
        match p {
            rs::Pat::Tuple(items) if parts.len() > 1 && items.len() == parts.len() => {
                items.iter().map(Cell::Pat).collect()
            }
            _ if parts.len() > 1 && matches!(p, rs::Pat::Wild) => vec![Cell::Wild; parts.len()],
            _ => vec![Cell::Pat(p)],
        }
    };
    // An arm of alternatives over the columns is an arm per alternative,
    // each with the arm's body, tried in order as Rust tries them.
    let arms: Vec<(&'a rs::Pat, Body<'a>)> = arms
        .iter()
        .flat_map(|(p, b)| match p {
            rs::Pat::Or(alts) if parts.len() > 1 => alts.iter().map(|a| (a, *b)).collect(),
            _ => vec![(*p, *b)],
        })
        .collect();
    let rows: Option<Vec<Clause<'a>>> = arms
        .iter()
        .enumerate()
        .map(|(arm, (p, b))| {
            let cells = spread(p);
            (cells.len() == cols.len()).then_some(Clause {
                arm,
                cells,
                binds: Vec::new(),
                body: *b,
            })
        })
        .collect();
    let Some(rows) = rows else {
        return Err(Residue::Form("match pattern".to_string()));
    };
    compile(&cols, &rows, env, ctx, acc, want)
}

/// A match on `xs.split_first()`, alone or as an item of a tuple, as the
/// match on `xs` itself: `None` is `[]`, and `Some((head, rest))` is
/// `[head, rest @ ..]`.
fn split_first(
    scrutinee: &rs::RustExpr,
    arms: &[(&rs::Pat, Body<'_>)],
) -> Option<(rs::RustExpr, Vec<rs::Pat>)> {
    let is_split = |e: &rs::RustExpr| matches!(e, rs::RustExpr::MethodCall(m) if m.method == "split_first" && m.args.is_empty());
    let receiver = |e: &rs::RustExpr| match e {
        rs::RustExpr::MethodCall(m) => (*m.receiver).clone(),
        other => other.clone(),
    };
    let (slice, at) = match scrutinee {
        rs::RustExpr::Tuple(items) if items.len() >= 2 => {
            let k = items.iter().position(is_split)?;
            let mut items = items.clone();
            items[k] = receiver(&items[k]);
            (rs::RustExpr::Tuple(items), Some(k))
        }
        one if is_split(one) => (receiver(one), None),
        _ => return None,
    };
    let pats = arms
        .iter()
        .map(|(p, _)| match (at, p) {
            (None, p) => sliced(p),
            (Some(_), rs::Pat::Wild) => Some(rs::Pat::Wild),
            (Some(k), rs::Pat::Tuple(items)) => {
                let mut items = items.clone();
                items[k] = sliced(items.get(k)?)?;
                Some(rs::Pat::Tuple(items))
            }
            _ => None,
        })
        .collect::<Option<Vec<rs::Pat>>>()?;
    Some((slice, pats))
}

/// A pattern of `split_first`'s value as a pattern of the slice.
fn sliced(p: &rs::Pat) -> Option<rs::Pat> {
    match p {
        rs::Pat::Wild => Some(rs::Pat::Wild),
        rs::Pat::Path(path) if wrapper(path) == Some("None") => Some(rs::Pat::Slice(Vec::new())),
        rs::Pat::Ident(i) if i.ident == "None" && i.subpat.is_none() => {
            Some(rs::Pat::Slice(Vec::new()))
        }
        rs::Pat::TupleStruct(ts) if wrapper(&ts.path) == Some("Some") => {
            let [rs::Pat::Tuple(pair)] = ts.elems.as_slice() else {
                return None;
            };
            let [head, rest] = pair.as_slice() else {
                return None;
            };
            let mut elems = vec![head.clone()];
            match rest {
                rs::Pat::Wild => elems.push(rs::Pat::Rest),
                rs::Pat::Ident(i) if i.subpat.is_none() => {
                    elems.push(rs::Pat::Ident(rs::PatIdent {
                        subpat: Some(Box::new(rs::Pat::Rest)),
                        ..i.clone()
                    }))
                }
                rs::Pat::Slice(more) => elems.extend(more.iter().cloned()),
                _ => return None,
            }
            Some(rs::Pat::Slice(elems))
        }
        _ => None,
    }
}

/// The row's cells with the cells at column `j` in place of its cell there.
fn replaced<'a>(row: &Clause<'a>, j: usize, cells: Vec<Cell<'a>>) -> Vec<Cell<'a>> {
    let mut out = row.cells[..j].to_vec();
    out.extend(cells);
    out.extend(row.cells[j + 1..].iter().copied());
    out
}

/// The rows of a match over the columns, as one expression: the first row
/// whose cells all match, and whose guard holds, gives the value.
fn compile<'a>(
    cols: &[Column],
    rows: &[Clause<'a>],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some((first, rest)) = rows.split_first() else {
        return Err(Residue::Form("non-exhaustive match".to_string()));
    };
    // The first column the first row tests, and the names it binds before.
    let mut binds = first.binds.clone();
    let mut tested = None;
    for (j, (cell, col)) in first.cells.iter().zip(cols).enumerate() {
        let Some((names, h)) = head(*cell, &col.ty, &env.types) else {
            return refused();
        };
        if matches!(h, Head::Any) {
            binds.extend(
                names
                    .into_iter()
                    .map(|n| (n, col.value.clone(), col.ty.clone())),
            );
        } else {
            tested = Some(j);
            break;
        }
    }
    let Some(j) = tested else {
        let matched = Matched {
            cols,
            first,
            rest,
            binds,
        };
        return matched_row(&matched, env, ctx, acc, want);
    };
    let col = &cols[j];
    let mut heads = Vec::new();
    for row in rows {
        let Some(hs) = heads_at(row, j, col, &env.types) else {
            return refused();
        };
        heads.extend(hs);
    }
    let split = Split { cols, j, heads };
    if let Some(items) = item_columns(col, env) {
        return compile_items(&split, items, env, ctx, acc, want);
    }
    if matches!(split.heads.first(), Some((_, Head::Test(_)))) {
        return compile_tests(&split, env, ctx, acc, want);
    }
    if col.ty == Ty::Bool {
        return compile_bool(&split, env, ctx, acc, want);
    }
    compile_ctors(&split, env, ctx, acc, want)
}

/// A match pattern outside the compiler.
fn refused<T>() -> Lift<T> {
    Err(Residue::Form("match pattern".to_string()))
}

/// A first row that matches every value, with the names its cells bind,
/// and the rows after it.
struct Matched<'c, 'a> {
    cols: &'c [Column],
    first: &'c Clause<'a>,
    rest: &'c [Clause<'a>],
    binds: Vec<(String, String, Ty)>,
}

/// The first row matches every value: its arm, when its guard holds. A
/// name bound to a part of the scrutinee is a binder of its own, so what
/// the arm lifts to names it.
fn matched_row(
    m: &Matched<'_, '_>,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let mut scope = ctx.clone();
    let mut lets = Vec::new();
    let mut acc = acc;
    for (rust, value, ty) in m.binds.iter().cloned() {
        let value = ctx.rebuilt.get(&value).cloned().unwrap_or(value);
        if value.chars().all(|c| c.is_alphanumeric() || c == '_') {
            if let Some((old, _)) = scope.locals.insert(rust, (value, ty)) {
                scope.hidden.push(old);
            }
        } else {
            let (next, name, after) = scope.bind(env, acc, &rust, ty.clone());
            scope = next;
            acc = after;
            lets.push((name, ty.render(), value));
        }
    }
    let Body::Guarded(g, e) = m.first.body else {
        let ((e, t), acc) = body(m.first.body, env, &scope, acc, want)?;
        return Ok(((wrap_lets(lets, e), t), acc));
    };
    let ((gv, gt), acc) = flat_of(g, env, &scope.below(), acc, Some(&Ty::Bool))?;
    if gt != Ty::Bool {
        return Err(Residue::Form("non-boolean guard".to_string()));
    }
    let ((then, tt), acc) = expr(e, env, &scope, acc, want)?;
    let ((other, ot), acc) = compile(m.cols, m.rest, env, ctx, acc, Some(want.unwrap_or(&tt)))?;
    if !tt.same(&ot) {
        return Err(Residue::Form("arms of different types".to_string()));
    }
    let e = Expr::If {
        condition: Box::new(Expr::Flat(gv)),
        then: Box::new(then),
        otherwise: Box::new(other),
    };
    Ok(((wrap_lets(lets, e), tt), acc))
}

/// The rows of a match split at the column `j` they first test, each with
/// what its cell there tests.
struct Split<'c, 'a> {
    cols: &'c [Column],
    j: usize,
    heads: Vec<(Clause<'a>, Head<'a>)>,
}

impl<'a> Split<'_, 'a> {
    /// The columns with `middle` in place of the split one.
    fn around(&self, middle: Vec<Column>) -> Vec<Column> {
        let mut out = self.cols[..self.j].to_vec();
        out.extend(middle);
        out.extend(self.cols[self.j + 1..].iter().cloned());
        out
    }

    /// The rows a constructor's values reach, its fields' cells in place
    /// of the column's.
    fn reach(&self, key: &str, arity: usize) -> Option<Vec<Clause<'a>>> {
        let mut out = Vec::new();
        for (row, h) in &self.heads {
            let cells = match h {
                Head::Any => vec![Cell::Wild; arity],
                Head::Ctor(k, cs) if k == key && cs.len() == arity => cs.clone(),
                Head::Ctor(k, _) if k != key => continue,
                _ => return None,
            };
            out.push(Clause {
                cells: replaced(row, self.j, cells),
                ..row.clone()
            });
        }
        Some(out)
    }
}

/// The columns of a tuple's or a structure's items, which a column of
/// that type splits into.
fn item_columns(col: &Column, env: &Env) -> Option<Vec<Column>> {
    match &col.ty {
        Ty::Tuple(ts) | Ty::Pair(ts) => Some(
            ts.iter()
                .enumerate()
                .map(|(k, t)| Column {
                    value: tuple_item(&col.ty, &col.value, k),
                    ty: t.clone(),
                })
                .collect(),
        ),
        Ty::Named(n) if env.types.structs.contains_key(n) => Some(
            env.types.structs[n]
                .iter()
                .map(|(f, t)| Column {
                    value: if env.types.boxed(n) {
                        format!("({} {})", Types::accessor(n, f), col.value)
                    } else {
                        format!("(project {} {f})", col.value)
                    },
                    ty: t.clone(),
                })
                .collect(),
        ),
        _ => None,
    }
}

/// A tuple or a structure: a column for each item.
fn compile_items(
    split: &Split<'_, '_>,
    items: Vec<Column>,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let mut next = Vec::new();
    for (row, h) in &split.heads {
        let cells = match h {
            Head::Any => vec![Cell::Wild; items.len()],
            Head::Items(cs) if cs.len() == items.len() => cs.clone(),
            _ => return refused(),
        };
        next.push(Clause {
            cells: replaced(row, split.j, cells),
            ..row.clone()
        });
    }
    compile(&split.around(items), &next, env, ctx, acc, want)
}

/// A literal or a range: a test, then the rows after it.
fn compile_tests(
    split: &Split<'_, '_>,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let (cols, j, heads) = (split.cols, split.j, &split.heads);
    let col = &cols[j];
    let Some((row, _)) = heads.first() else {
        return refused();
    };
    // The alternatives of the first arm that test this column and agree
    // elsewhere: one test, their disjunction.
    let mut tests = Vec::new();
    let mut acc = acc;
    let mut taken = 0;
    for (other, h) in heads {
        let Head::Test(p) = h else { break };
        let alike = other.arm == row.arm
            && other.binds == row.binds
            && (0..cols.len()).all(|i| i == j || other.cells[i] == row.cells[i]);
        if !alike {
            break;
        }
        let Some((test, after)) = literal_test(&col.value, &col.ty, p, acc) else {
            return refused();
        };
        acc = after;
        tests.push(test);
        taken += 1;
    }
    let Some(test) = tests.into_iter().reduce(|a, b| format!("({a} || {b})")) else {
        return refused();
    };
    let mut matched = row.clone();
    matched.cells[j] = Cell::Wild;
    let after: Vec<Clause<'_>> = heads[taken..].iter().map(|(r, _)| r.clone()).collect();
    let then_rows: Vec<Clause<'_>> = std::iter::once(matched)
        .chain(after.iter().cloned())
        .collect();
    let ((then, tt), acc) = compile(cols, &then_rows, env, ctx, acc, want)?;
    let ((other, ot), acc) = compile(cols, &after, env, ctx, acc, Some(want.unwrap_or(&tt)))?;
    if !tt.same(&ot) {
        return Err(Residue::Form("arms of different types".to_string()));
    }
    let e = Expr::If {
        condition: Box::new(Expr::Flat(test)),
        then: Box::new(then),
        otherwise: Box::new(other),
    };
    Ok(((e, tt), acc))
}

/// A `bool` column: an `if` on it.
fn compile_bool(
    split: &Split<'_, '_>,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let (Some(yes), Some(no)) = (split.reach("true", 0), split.reach("false", 0)) else {
        return refused();
    };
    let ((then, tt), acc) = compile(&split.around(Vec::new()), &yes, env, ctx, acc, want)?;
    let want = Some(want.unwrap_or(&tt));
    let ((other, ot), acc) = compile(&split.around(Vec::new()), &no, env, ctx, acc, want)?;
    if !tt.same(&ot) {
        return Err(Residue::Form("arms of different types".to_string()));
    }
    let e = Expr::If {
        condition: Box::new(Expr::Flat(split.cols[split.j].value.clone())),
        then: Box::new(then),
        otherwise: Box::new(other),
    };
    Ok(((e, tt), acc))
}

/// An arm of a constructor match still to lift: where it goes, its
/// columns, its rows and its scope.
type Pending<'a> = (usize, Vec<Column>, Vec<Clause<'a>>, Ctx);

/// A column of a data type: a match with an arm for each constructor.
fn compile_ctors(
    split: &Split<'_, '_>,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let col = &split.cols[split.j];
    let Some(ctors) = constructors(&col.ty, &env.types) else {
        return Err(Residue::Form(format!("match on {}", col.ty.render())));
    };
    let mut out = Vec::new();
    let mut result: Option<Ty> = want.cloned();
    let mut deferred: Vec<Pending<'_>> = Vec::new();
    let mut acc = acc;
    for (key, lex, field_tys) in &ctors {
        let Some(next) = split.reach(key, field_tys.len()) else {
            return refused();
        };
        let (scope, names, after) = ctor_scope(split, &next, lex, field_tys, env, ctx, acc);
        acc = after;
        let binders: Vec<String> = names.iter().map(|c| c.value.clone()).collect();
        let pattern = std::iter::once(lex.clone())
            .chain(binders)
            .collect::<Vec<_>>()
            .join(" ");
        let cols = split.around(names);
        let ((e, t), after) = match compile(&cols, &next, env, &scope, acc.clone(), result.as_ref())
        {
            // An arm whose type only a sibling says waits for the others.
            Err(r) if result.is_none() && untyped(&r) => {
                deferred.push((out.len(), cols, next, scope));
                out.push(Arm {
                    pattern,
                    body: Expr::Flat(String::new()),
                });
                continue;
            }
            lifted => lifted?,
        };
        acc = after;
        match &result {
            Some(r) if !r.same(&t) => {
                return Err(Residue::Form("arms of different types".to_string()));
            }
            _ => result = Some(t),
        }
        out.push(Arm { pattern, body: e });
    }
    let Some(result) = result else {
        return Err(Residue::Form("empty match".to_string()));
    };
    for (at, cols, next, scope) in deferred {
        let ((e, t), after) = compile(&cols, &next, env, &scope, acc, Some(&result))?;
        acc = after;
        if !result.same(&t) {
            return Err(Residue::Form("arms of different types".to_string()));
        }
        out[at].body = e;
    }
    let e = Expr::Match {
        scrutinee: col.value.clone(),
        arms: out,
    };
    Ok(((e, result), acc))
}

/// The scope of a constructor's arm and its fields' columns: each field's
/// binder is the name the first row reaching it gives it, or a fresh one.
fn ctor_scope(
    split: &Split<'_, '_>,
    next: &[Clause<'_>],
    lex: &str,
    field_tys: &[Ty],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> (Ctx, Vec<Column>, Acc) {
    let (col, j) = (&split.cols[split.j], split.j);
    let mut acc = acc;
    let mut scope = ctx.clone();
    let mut names = Vec::new();
    for (i, ty) in field_tys.iter().enumerate() {
        let given = next.iter().find_map(|row| match row.cells[j + i] {
            Cell::Pat(rs::Pat::Ident(id)) if id.subpat.is_none() => Some(local_name(&id.ident)),
            _ => None,
        });
        let base = given.unwrap_or_else(|| {
            if i == 0 {
                "value".into()
            } else {
                "rest".into()
            }
        });
        let (name, after) = if scope.taken(env, &base) {
            scope.fresh(env, acc, &base)
        } else {
            (base, acc)
        };
        acc = after;
        scope.hidden.push(name.clone());
        names.push(Column {
            value: name,
            ty: ty.clone(),
        });
    }
    if ctx.params.contains(&col.value) {
        // The tail of a list parameter, and a field of a data parameter's
        // own type, are sub-terms: a recursive call on one descends.
        for (i, field) in names.iter().enumerate() {
            let tail = lex == "List.cons" && i == 1;
            let data = types_has_data(&col.ty, &env.types);
            let list = Ty::List(Box::new(col.ty.clone()));
            if tail || (data && (field.ty.same(&col.ty) || field.ty.same(&list))) {
                scope.tails.insert(field.value.clone(), col.value.clone());
            }
        }
        // A structural match names the parameter only through its
        // parts: in the arm, the parameter is its constructor applied
        // to them.
        if types_has_data(&col.ty, &env.types) {
            let parts: String = names.iter().map(|f| format!(" {}", f.value)).collect();
            let whole = format!("(ctor {lex}{parts})");
            let rust: Vec<String> = ctx
                .locals
                .iter()
                .filter(|(_, (n, _))| *n == col.value)
                .map(|(r, _)| r.clone())
                .collect();
            for r in rust {
                scope.locals.insert(r, (whole.clone(), col.ty.clone()));
            }
            scope.rebuilt.insert(col.value.clone(), whole);
        }
    }
    (scope, names, acc)
}

/// A residue that says only that an expression's type was not known where
/// it was lifted: an empty collection or a `None` no sibling had typed yet.
fn untyped(r: &Residue) -> bool {
    match r {
        Residue::Call(n) => n == "new",
        Residue::Form(f) => f == "None of an unknown type",
        _ => false,
    }
}

/// A branch followed by the statements after it, as one block expression.
fn then_rest(body: &rs::RustExpr, rest: &[rs::Stmt]) -> rs::RustExpr {
    let mut stmts = match body {
        rs::RustExpr::Block(b) if !b.label => b.block.stmts.clone(),
        rs::RustExpr::Tuple(t) if t.is_empty() => Vec::new(),
        other => vec![rs::Stmt::Expr(other.clone(), true)],
    };
    stmts.extend(rest.iter().cloned());
    rs::RustExpr::Block(rs::ExprBlock {
        label: false,
        block: rs::Block { stmts },
    })
}

/// The `return v` a block is made of, alone: `{ return v; }`.
fn returned(b: &rs::Block) -> Option<&rs::RustExpr> {
    match b.stmts.as_slice() {
        [rs::Stmt::Expr(rs::RustExpr::Return(r), _)] => r.as_deref(),
        _ => None,
    }
}

/// The local a statement updates in place, and how: `x = e`, `x += e` on a
/// string, `x.push(e)` on a list or string, `x.push_str(e)`.
fn update_target(e: &rs::RustExpr) -> Option<String> {
    let ident = |e: &rs::RustExpr| match e {
        rs::RustExpr::Path(p) => p.get_ident().cloned(),
        _ => None,
    };
    match e {
        // `x.f = v`: the structure `x` with its field `f` replaced.
        rs::RustExpr::Assign(a) => match &*a.left {
            rs::RustExpr::Field(f) if matches!(f.member, rs::Member::Named(_)) => ident(&f.base),
            left => ident(left),
        },
        // `*m.entry(k).or_default() += v` and `m.entry(k).or_default().push(v)`
        // update the map.
        rs::RustExpr::Binary(b) if b.op == rs::BinOp::AddAssign => match &*b.left {
            rs::RustExpr::Unary(u) if matches!(u.op, rs::UnOp::Deref) => {
                ident(default_entry(&u.expr)?.0)
            }
            left => ident(left),
        },
        rs::RustExpr::MethodCall(m) if default_entry(&m.receiver).is_some() => {
            ident(default_entry(&m.receiver)?.0)
        }
        rs::RustExpr::MethodCall(m)
            if matches!(m.method.as_str(), "push" | "push_str" | "extend") && m.args.len() == 1 =>
        {
            ident(&m.receiver).or_else(|| ident(&named_field(&m.receiver)?.base))
        }
        // The value of `insert` (whether it was new, or the replaced value)
        // is dropped where it is a statement of a loop.
        rs::RustExpr::MethodCall(m) if m.method == "insert" && (1..=2).contains(&m.args.len()) => {
            ident(&m.receiver).or_else(|| ident(&named_field(&m.receiver)?.base))
        }
        _ => None,
    }
}

/// `x.f`: a named field of a value.
fn named_field(e: &rs::RustExpr) -> Option<&rs::ExprField> {
    match e {
        rs::RustExpr::Field(f) if matches!(f.member, rs::Member::Named(_)) => Some(f),
        _ => None,
    }
}

/// A character literal as the one-character string it pushes.
fn char_text(e: &rs::RustExpr) -> Option<String> {
    match e {
        rs::RustExpr::Lit(rs::Lit::Char(c)) => exact_string(&c.to_string()),
        _ => None,
    }
}

/// The new value of the local an update statement changes.
fn updated(
    e: &rs::RustExpr,
    ty: &Ty,
    current: &str,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<(String, Acc)> {
    let inner = ctx.below();
    let joined = |piece: String, acc: Acc| {
        let acc = acc.axiom("propext");
        Ok((
            format!("(prim join (cons {current} (cons {piece} (nil String))) \"\" : String)"),
            acc,
        ))
    };
    let value_of = |v: &rs::RustExpr, want: &Ty, acc: Acc| -> Lift<(String, Acc)> {
        let ((x, xt), acc) = flat_of(v, env, &inner, acc, Some(want))?;
        if xt.same(want) {
            Ok((x, acc))
        } else {
            Err(Residue::Form("an update of another type".to_string()))
        }
    };
    match (e, ty) {
        // `x.f.push(v)`: the structure `x` with its field `f` updated.
        (rs::RustExpr::MethodCall(m), Ty::Named(name)) if named_field(&m.receiver).is_some() => {
            let Some(f) = named_field(&m.receiver) else {
                return Err(Residue::Form("an update outside the lift".to_string()));
            };
            let rs::Member::Named(member) = &f.member else {
                return Err(Residue::Form("an update outside the lift".to_string()));
            };
            let Some(fields) = env.types.structs.get(name) else {
                return Err(Residue::Form(format!("a field update of {name}")));
            };
            let target = local_name(member);
            let Some((_, fty)) = fields.iter().find(|(f, _)| *f == target) else {
                return Err(Residue::Form(format!("a field update of {name}")));
            };
            let read = |field: &String| {
                if env.types.boxed(name) {
                    format!("({} {current})", Types::accessor(name, field))
                } else {
                    format!("(project {current} {field})")
                }
            };
            let call = rs::RustExpr::MethodCall(rs::ExprMethodCall {
                receiver: f.base.clone(),
                ..m.clone()
            });
            let (v, acc) = updated(&call, fty, &read(&target), env, ctx, acc)?;
            let parts: Vec<(&String, String)> = fields
                .iter()
                .map(|(field, _)| {
                    let value = if *field == target {
                        v.clone()
                    } else {
                        read(field)
                    };
                    (field, value)
                })
                .collect();
            Ok((struct_value(name, &parts, &env.types), acc))
        }
        (rs::RustExpr::Assign(a), Ty::Named(name))
            if matches!(&*a.left, rs::RustExpr::Field(_)) =>
        {
            let rs::RustExpr::Field(f) = &*a.left else {
                return Err(Residue::Form("an update outside the lift".to_string()));
            };
            let rs::Member::Named(member) = &f.member else {
                return Err(Residue::Form("an update outside the lift".to_string()));
            };
            let Some(fields) = env.types.structs.get(name) else {
                return Err(Residue::Form(format!("a field update of {name}")));
            };
            let target = local_name(member);
            let mut acc = acc;
            let mut parts = Vec::new();
            for (field, fty) in fields {
                let v = if *field == target {
                    let (v, after) = value_of(&a.right, fty, acc)?;
                    acc = after;
                    v
                } else if env.types.boxed(name) {
                    format!("({} {current})", Types::accessor(name, field))
                } else {
                    format!("(project {current} {field})")
                };
                parts.push((field, v));
            }
            if !fields.iter().any(|(f, _)| *f == target) {
                return Err(Residue::Form(format!("a field update of {name}")));
            }
            Ok((struct_value(name, &parts, &env.types), acc))
        }
        (rs::RustExpr::Assign(a), _) => value_of(&a.right, ty, acc),
        // `xs.extend(o)` of an option: the one value it holds, if any.
        (rs::RustExpr::MethodCall(m), Ty::List(t) | Ty::Set(t, true))
            if m.method == "extend" && extends_by_option(&m.args[0], t, env, &inner, &acc) =>
        {
            let ((v, _), acc) = flat_of(&m.args[0], env, &inner, acc, None)?;
            let (x, acc) = ctx.fresh(env, acc, "item");
            let (added, acc) = match ty {
                Ty::Set(..) => {
                    let (insert, acc) = sorted_insert(ty, env, ctx, acc)?;
                    (format!("({insert} {current} {x})"), acc)
                }
                _ => (
                    format!(
                        "(prim append {current} (cons {x} (nil {})) : {})",
                        t.render(),
                        ty.render()
                    ),
                    acc,
                ),
            };
            Ok((
                format!("(match {v} | Option.none => {current} | Option.some {x} => {added})"),
                acc,
            ))
        }
        // `xs.extend(ys)`: the list followed by the items.
        (rs::RustExpr::MethodCall(m), Ty::List(_)) if m.method == "extend" => {
            let (v, acc) = value_of(&m.args[0], ty, acc)?;
            Ok((
                format!("(prim append {current} {v} : {})", ty.render()),
                acc,
            ))
        }
        // `set.extend(items)`: each item inserted in turn.
        (rs::RustExpr::MethodCall(m), Ty::Set(t, true)) if m.method == "extend" => {
            let items = Ty::List(t.clone());
            let ((v, vt), acc) = flat_of(&m.args[0], env, &inner, acc, Some(ty))?;
            let held = matches!(&vt, Ty::List(e) | Ty::Set(e, true) if e.same(t));
            if !held {
                return Err(Residue::Form("an update of another type".to_string()));
            }
            let (insert, acc) = sorted_insert(ty, env, ctx, acc)?;
            let (helper, acc) = insert_all(ty, &items, &insert, env, ctx, acc);
            Ok((format!("({helper} {v} {current})"), acc))
        }
        (rs::RustExpr::Binary(b), Ty::Str) => {
            let (piece, acc) = value_of(&b.right, &Ty::Str, acc)?;
            joined(piece, acc)
        }
        // `i += k` on `usize`: the sum, where it stays in range.
        (rs::RustExpr::Binary(b), Ty::Nat(None)) if b.op == rs::BinOp::AddAssign => {
            let (k, acc) = value_of(&b.right, ty, acc)?;
            let sum = usize_sum(&b.left, &b.right, current, &k, env, &ctx.capped)?;
            Ok((sum, acc))
        }
        (rs::RustExpr::Binary(b), _) => Err(Residue::Arithmetic(b.op.text().to_string())),
        (rs::RustExpr::MethodCall(m), Ty::Set(t, true))
            if m.method == "insert" && m.args.len() == 1 =>
        {
            let (v, acc) = value_of(&m.args[0], t, acc)?;
            let (helper, acc) = sorted_insert(ty, env, ctx, acc)?;
            Ok((format!("({helper} {current} {v})"), acc))
        }
        (rs::RustExpr::MethodCall(m), Ty::Map(k, v, true))
            if m.method == "insert" && m.args.len() == 2 =>
        {
            let (key, acc) = value_of(&m.args[0], k, acc)?;
            let (value, acc) = value_of(&m.args[1], v, acc)?;
            let (helper, acc) = sorted_insert(ty, env, ctx, acc)?;
            let entry = ty.entry();
            Ok((
                format!(
                    "({helper} {current} {})",
                    tuple_value(&entry, &[key, value])
                ),
                acc,
            ))
        }
        (rs::RustExpr::MethodCall(m), Ty::List(t)) if m.method == "push" => {
            let (v, acc) = value_of(&m.args[0], t, acc)?;
            Ok((
                format!(
                    "(prim append {current} (cons {v} (nil {})) : {})",
                    t.render(),
                    ty.render()
                ),
                acc,
            ))
        }
        (rs::RustExpr::MethodCall(m), Ty::Str) => {
            let (piece, acc) = match char_text(&m.args[0]) {
                Some(text) if m.method == "push" => (text, acc),
                _ if m.method == "push_str" => value_of(&m.args[0], &Ty::Str, acc)?,
                _ => value_of(&m.args[0], &Ty::Char, acc)?,
            };
            joined(piece, acc)
        }
        _ => Err(Residue::Form("an update outside the lift".to_string())),
    }
}

/// Whether `e` is an option of the element type `t`: what `extend` adds
/// at most one of.
fn extends_by_option(e: &rs::RustExpr, t: &Ty, env: &Env, ctx: &Ctx, acc: &Acc) -> bool {
    match flat_of(e, env, ctx, acc.clone(), None) {
        Ok(((_, Ty::Option(inner)), _)) => inner.same(t),
        _ => false,
    }
}

/// The helper inserting into the sorted list an ordered set or map is held
/// as, keeping it sorted by key with each key once: a set ignores a member
/// it holds, a map replaces the value of a key it holds. `BTreeSet` and
/// `BTreeMap` order `String` keys by their bytes, which is the order of
/// their scalars, and naturals as numbers; other keys' order is not modelled.
fn sorted_insert(ty: &Ty, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<(String, Acc)> {
    let (key, entry) = match ty {
        Ty::Set(t, true) => ((**t).clone(), (**t).clone()),
        Ty::Map(k, _, true) => ((**k).clone(), ty.entry()),
        _ => return Err(Residue::Form("insert into an unordered collection".into())),
    };
    let list = Ty::List(Box::new(entry.clone()));
    let (items, acc) = ctx.fresh(env, acc, "items");
    let (item, acc) = ctx.fresh(env, acc, "item");
    let (head, acc) = ctx.fresh(env, acc, "head");
    let (rest, acc) = ctx.fresh(env, acc, "rest");
    let key_of = |v: &str| match ty {
        Ty::Map(..) => tuple_item(&entry, v, 0),
        _ => v.to_string(),
    };
    let (k, h) = (key_of(&item), key_of(&head));
    let mut axioms = BTreeSet::new();
    let (same, less) = match &key {
        Ty::Nat(_) => (format!("({k} =? {h})"), format!("({k} <? {h})")),
        Ty::Str | Ty::Char => {
            axioms.insert("propext".to_string());
            axioms.extend(ALL_AXIOMS.map(String::from));
            (
                format!("(prim equal {k} {h} : Bool)"),
                format!("(prim less_than {k} {h} : Bool)"),
            )
        }
        other if crate::order::derived(other, &env.types) => {
            axioms.extend(crate::order::axioms(other, &env.types));
            (
                crate::order::operator("==", other, &k, &h).unwrap_or_default(),
                crate::order::operator("<", other, &k, &h).unwrap_or_default(),
            )
        }
        other => {
            return Err(Residue::Form(format!(
                "insert by a key of {}, whose order the lift does not model",
                other.render()
            )))
        }
    };
    let acc = if crate::order::derived(&key, &env.types)
        && !matches!(key, Ty::Nat(_) | Ty::Str | Ty::Char)
    {
        acc.ordered(&key, &env.types)
    } else {
        acc
    };
    let name = ctx.helper_name(env, &acc, "Insert");
    let acc = acc.saw(&entry);
    // A structural match names the list only through its parts.
    let whole = format!("(cons {head} {rest})");
    let kept = match ty {
        Ty::Map(..) => format!("(cons {item} {rest})"),
        _ => whole.clone(),
    };
    let body = Expr::Match {
        scrutinee: items.clone(),
        arms: vec![
            Arm {
                pattern: "List.nil".into(),
                body: Expr::Flat(format!("(cons {item} (nil {}))", entry.render())),
            },
            Arm {
                pattern: format!("List.cons {head} {rest}"),
                body: Expr::Flat(format!(
                    "(if {same} then {kept} else (if {less} then (cons {item} {whole}) else (cons {head} ({name} {rest} {item}))))"
                )),
            },
        ],
    };
    let acc = acc.axioms_all(axioms.clone());
    let acc = acc.helper(Helper {
        name: name.clone(),
        params: vec![(items, list.render()), (item, entry.render())],
        result: list.render(),
        body,
        calls: BTreeSet::new(),
        axioms,
        recursive: true,
        entwined: false,
        termination: None,
    });
    let acc = acc.called(format!("#{name}"));
    Ok((name, acc))
}

/// The helper inserting each item of a list into an ordered set, in turn,
/// through the set's `insert` helper.
fn insert_all(ty: &Ty, items: &Ty, insert: &str, env: &Env, ctx: &Ctx, acc: Acc) -> (String, Acc) {
    let (set, acc) = ctx.fresh(env, acc, "set");
    let (list, acc) = ctx.fresh(env, acc, "items");
    let (head, acc) = ctx.fresh(env, acc, "head");
    let (rest, acc) = ctx.fresh(env, acc, "rest");
    let name = ctx.helper_name(env, &acc, "InsertAll");
    let body = Expr::Match {
        scrutinee: list.clone(),
        arms: vec![
            Arm {
                pattern: "List.nil".into(),
                body: Expr::Flat(set.clone()),
            },
            Arm {
                pattern: format!("List.cons {head} {rest}"),
                body: Expr::Flat(format!("({name} {rest} ({insert} {set} {head}))")),
            },
        ],
    };
    let axioms = acc
        .helpers
        .iter()
        .find(|h| h.name == insert)
        .map(|h| h.axioms.clone())
        .unwrap_or_default();
    let acc = acc.helper(Helper {
        name: name.clone(),
        params: vec![(list, items.render()), (set, ty.render())],
        result: ty.render(),
        body,
        calls: BTreeSet::from([format!("#{insert}")]),
        axioms,
        recursive: true,
        entwined: false,
        termination: None,
    });
    let acc = acc.called(format!("#{name}"));
    (name, acc)
}

/// The locals the statements update and the locals they declare, outside
/// closures.
fn scan_stmts(stmts: &[rs::Stmt]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut updated = BTreeSet::new();
    let mut declared = BTreeSet::new();
    for stmt in stmts {
        let (u, d) = match stmt {
            rs::Stmt::Local(l) => {
                let mut exprs: Vec<&rs::RustExpr> = Vec::new();
                if let Some(init) = &l.init {
                    exprs.push(&init.expr);
                    exprs.extend(init.diverge.as_deref());
                }
                let (u, mut d) = scan_exprs(&exprs);
                if let rs::Pat::Ident(i) = &l.pat {
                    d.insert(i.ident.clone());
                }
                (u, d)
            }
            rs::Stmt::Expr(e, _) => scan_exprs(&[e]),
            rs::Stmt::Macro(_) | rs::Stmt::Other => (BTreeSet::new(), BTreeSet::new()),
        };
        updated.extend(u);
        declared.extend(d);
    }
    (updated, declared)
}

/// [`scan_stmts`] over expressions.
fn scan_exprs(exprs: &[&rs::RustExpr]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut updated = BTreeSet::new();
    let mut declared = BTreeSet::new();
    for e in exprs {
        if matches!(e, rs::RustExpr::Closure(_)) {
            continue;
        }
        updated.extend(update_target(e));
        let (inner, blocks) = e.parts();
        let (u, d) = scan_exprs(&inner);
        updated.extend(u);
        declared.extend(d);
        for b in blocks {
            let (u, d) = scan_stmts(&b.stmts);
            updated.extend(u);
            declared.extend(d);
        }
    }
    (updated, declared)
}

/// The locals a loop body updates that are bound before the loop.
fn loop_state(body: &[rs::Stmt], ctx: &Ctx) -> Lift<Vec<String>> {
    let (updated, declared) = scan_stmts(body);
    // A local the body declares and updates is the body's own, unless it
    // shadows one bound before the loop.
    if updated
        .iter()
        .any(|u| declared.contains(u) && ctx.locals.contains_key(u))
    {
        return Err(Residue::Form("a loop that shadows its state".to_string()));
    }
    let state: Vec<String> = updated
        .into_iter()
        .filter(|u| ctx.locals.contains_key(u) && !declared.contains(u))
        .collect();
    // A loop of the function's own statements that updates nothing may
    // still leave it: a walk whose state is `()`.
    if state.is_empty() && !ctx.top {
        return Err(Residue::Form("a loop that updates no local".to_string()));
    }
    Ok(state)
}

/// The state of a loop as one value: the local, or the tuple of them.
fn state_expr(state: &[String]) -> rs::RustExpr {
    let path = |name: &String| rs::RustExpr::Path(rs::Path::ident(name));
    match state {
        [one] => path(one),
        many => rs::RustExpr::Tuple(many.iter().map(path).collect()),
    }
}

/// Every tail of a lifted expression, rewritten: the step of a loop ends in
/// its recursive call on the new state.
fn map_tails(e: Expr, f: &dyn Fn(String) -> String) -> Expr {
    match e {
        Expr::Flat(s) => Expr::Flat(f(s)),
        Expr::If {
            condition,
            then,
            otherwise,
        } => Expr::If {
            condition,
            then: Box::new(map_tails(*then, f)),
            otherwise: Box::new(map_tails(*otherwise, f)),
        },
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|a| Arm {
                    pattern: a.pattern,
                    body: map_tails(a.body, f),
                })
                .collect(),
        },
        Expr::Let {
            name,
            ty,
            value,
            body,
        } => Expr::Let {
            name,
            ty,
            value,
            body: Box::new(map_tails(*body, f)),
        },
    }
}

/// Bind a loop's state to the names its locals take next: the value itself,
/// or each item of the tuple.
fn rebind(
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    state: &[String],
    whole: &str,
    lets: Lets,
) -> (Ctx, Acc, Lets) {
    let mut scope = ctx.clone();
    if let [one] = state {
        let ty = ctx.locals[one].1.clone();
        if let Some((old, _)) = scope.locals.insert(one.clone(), (whole.to_string(), ty)) {
            scope.hidden.push(old);
        }
        return (scope, acc, lets);
    }
    let mut acc = acc;
    let mut lets = lets;
    for (i, var) in state.iter().enumerate() {
        let ty = ctx.locals[var].1.clone();
        let (next, name, after) = scope.bind(env, acc, var, ty.clone());
        scope = next;
        acc = after;
        lets.push((name, ty.render(), format!("(project {whole} item{i})")));
    }
    (scope, acc, lets)
}

fn wrap_lets(lets: Vec<(String, String, String)>, body: Expr) -> Expr {
    lets.into_iter()
        .rev()
        .fold(body, |body, (name, ty, value)| Expr::Let {
            name,
            ty,
            value: Box::new(Expr::Flat(value)),
            body: Box::new(body),
        })
}

/// The mark on a loop step's value that ends the walk: a `break`.
const BREAK: &str = "#break ";

/// The mark on a loop step's value that leaves the function: a `return`
/// or a failed `?`.
const LEAVE: &str = "#leave ";

/// The value of a `continue` or `break` in a loop step: the loop's state as
/// the step leaves it.
fn loop_exit(env: &Env, ctx: &Ctx, acc: Acc, label: bool, breaks: bool) -> Lift<((Expr, Ty), Acc)> {
    let Some(state) = &ctx.looping else {
        return Err(Residue::Form(
            "continue or break outside a loop step".to_string(),
        ));
    };
    if label {
        return Err(Residue::Form("labelled continue or break".to_string()));
    }
    let ((v, t), acc) = flat_of(&state_expr(state), env, ctx, acc, None)?;
    let v = if breaks { format!("{BREAK}{v}") } else { v };
    Ok(((Expr::Flat(v), t), acc))
}

/// `for p in xs { body } rest`: a helper that walks `xs` with the locals the
/// body updates as its accumulator, then `rest` with their final values.
fn for_loop(
    f: &rs::ExprForLoop,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    if f.label {
        return Err(Residue::Form("labelled loop".to_string()));
    }
    let ((list, lt), acc) = flat_of(&f.expr, env, &ctx.below(), acc, None)?;
    let elem = match &lt {
        Ty::List(t) => (**t).clone(),
        Ty::Set(t, true) => (**t).clone(),
        Ty::Map(_, _, true) => lt.entry(),
        _ => {
            return Err(Residue::Form(
                "a loop over something other than a list".to_string(),
            ))
        }
    };
    let state = loop_state(&f.body.stmts, ctx)?;
    let tys: Vec<Ty> = state.iter().map(|v| ctx.locals[v].1.clone()).collect();
    let state_ty = match tys.as_slice() {
        [] => Ty::Unit,
        [one] => one.clone(),
        many => Ty::Tuple(many.to_vec()),
    };
    let acc = acc.saw(&state_ty);
    // The step, in a scope of its own.
    let (acc, before) = acc.apart();
    let mut base = Ctx {
        tails: BTreeMap::new(),
        looping: Some(state.clone()),
        exits: ctx.top.then(|| state_ty.clone()),
        ..ctx.clone()
    };
    // The outer names stay taken inside the step: the step's binders may
    // not shadow a capture.
    for (rust, binding) in &ctx.locals {
        base.locals
            .insert(format!("#outer {rust}"), binding.clone());
    }
    let (current, acc) = base.fresh(env, acc, "state");
    let (scope, acc, lets) = rebind(env, &base, acc, &state, &current, Vec::new());
    let (mut scope, head, acc, pat_lets) = bind_pat(&f.pat, &elem, env, &scope, acc, Vec::new())?;
    // Each element of a list that is a sub-term of a parameter is one: a
    // recursive call on it descends, and the loop and the function are one
    // mutual group.
    let element_of = ctx.tails.get(&list).cloned();
    if let Some(param) = &element_of {
        scope.tails.insert(head.clone(), param.clone());
    }
    let mut stmts = f.body.stmts.clone();
    stmts.push(rs::Stmt::Expr(state_expr(&state), false));
    let ((step, st), acc) = block(&stmts, env, &scope, acc, Some(&state_ty))?;
    let (acc, own) = acc.rejoin(before);
    if !st.same(&state_ty) || (!own.descends.is_empty() && element_of.is_none()) {
        return Err(Residue::Form("a loop step of another type".to_string()));
    }
    let entwined = !own.descends.is_empty();
    let acc = own.descends.iter().fold(acc, |acc, i| acc.descended(*i));
    let name = ctx.helper_name(env, &acc, "Loop");
    let (items, acc) = ctx.fresh(env, acc, "items");
    let (rest_name, acc) = ctx.fresh(env, acc, "rest");
    let text = step.flat();
    // A step that can leave the function walks to the final state or to
    // the function's value.
    let leaving = text.contains(LEAVE);
    if state.is_empty() && !leaving {
        return Err(Residue::Form("a loop that updates no local".to_string()));
    }
    let walked = if leaving {
        Ty::Outcome(Box::new(state_ty.clone()), Box::new(ctx.result.clone()))
    } else {
        state_ty.clone()
    };
    let acc = acc.saw(&walked);
    let done = |s: String| {
        if leaving {
            format!(
                "(ctor Result.ok [{}, {}] {s})",
                state_ty.render(),
                ctx.result.render()
            )
        } else {
            s
        }
    };
    let caps = captures(ctx, &[text], std::slice::from_ref(&current));
    let cap_names: Vec<String> = caps.iter().map(|(c, _)| c.clone()).collect();
    let recur = |s: String| {
        if let Some(last) = s.strip_prefix(BREAK) {
            return done(last.to_string());
        }
        if let Some(value) = s.strip_prefix(LEAVE) {
            return format!(
                "(ctor Result.error [{}, {}] {value})",
                state_ty.render(),
                ctx.result.render()
            );
        }
        let mut parts = vec![name.clone(), rest_name.clone(), s];
        parts.extend(cap_names.iter().cloned());
        format!("({})", parts.join(" "))
    };
    let body = wrap_lets(lets, wrap_lets(pat_lets, map_tails(step, &recur)));
    let mut params = vec![
        (items.clone(), lt.render()),
        (current.clone(), state_ty.render()),
    ];
    params.extend(caps.iter().map(|(c, t)| (c.clone(), t.render())));
    let acc = acc.helper(Helper {
        name: name.clone(),
        params,
        result: walked.render(),
        body: Expr::Match {
            scrutinee: items,
            arms: vec![
                Arm {
                    pattern: "List.nil".to_string(),
                    body: Expr::Flat(done(current)),
                },
                Arm {
                    pattern: format!("List.cons {head} {rest_name}"),
                    body,
                },
            ],
        },
        calls: own.calls,
        axioms: own.axioms,
        recursive: true,
        entwined,
        termination: None,
    });
    let acc = acc.called(format!("#{name}"));
    // The loop's value, then the rest with the final state.
    let ((init, _), acc) = flat_of(&state_expr(&state), env, &ctx.below(), acc, Some(&state_ty))?;
    let mut parts = vec![name, list, init];
    parts.extend(cap_names);
    let (whole, acc) = ctx.fresh(env, acc, "state");
    let (after, acc, lets) = rebind(env, ctx, acc, &state, &whole, Vec::new());
    let ((rest, rt), acc) = block(rest, env, &after, acc, want)?;
    let call = format!("({})", parts.join(" "));
    if !leaving {
        return Ok((
            (
                Expr::Let {
                    name: whole,
                    ty: state_ty.render(),
                    value: Box::new(Expr::Flat(call)),
                    body: Box::new(wrap_lets(lets, rest)),
                },
                rt,
            ),
            acc,
        ));
    }
    // The walk's value: the final state goes on to the rest, the
    // function's value leaves.
    let (value, acc) = ctx.fresh(env, acc, "value");
    let (left, left_ty) = ctx.returning((Expr::Flat(value.clone()), ctx.result.clone()));
    if !left_ty.same(&rt) {
        return Err(Residue::Form("a loop step of another type".to_string()));
    }
    Ok((
        (
            Expr::Match {
                scrutinee: call,
                arms: vec![
                    Arm {
                        pattern: format!("Result.ok {whole}"),
                        body: wrap_lets(lets, rest),
                    },
                    Arm {
                        pattern: format!("Result.error {value}"),
                        body: left,
                    },
                ],
            },
            rt,
        ),
        acc,
    ))
}

/// The cursor and bound of a loop condition `i < n`: a local the loop
/// updates below a value it does not.
fn cursor_of<'e>(cond: &'e rs::RustExpr, state: &[String]) -> Option<(String, &'e rs::RustExpr)> {
    let rs::RustExpr::Binary(b) = cond else {
        return None;
    };
    let rs::RustExpr::Path(p) = &*b.left else {
        return None;
    };
    let cursor = p.get_ident()?.clone();
    let (updated_bound, _) = scan_exprs(&[&b.right]);
    (b.op == rs::BinOp::Lt
        && state.contains(&cursor)
        && updated_bound.is_empty()
        && !state.iter().any(|name| uses_local(&b.right, name)))
    .then_some((cursor, &*b.right))
}

/// Whether an expression reads the local `name`.
fn uses_local(e: &rs::RustExpr, name: &str) -> bool {
    match e {
        rs::RustExpr::Path(p) => p.is_ident(name),
        rs::RustExpr::Macro(_) => true,
        other => {
            let (exprs, blocks) = other.parts();
            exprs.iter().any(|x| uses_local(x, name))
                || blocks.iter().any(|b| {
                    b.stmts.iter().any(|s| match s {
                        rs::Stmt::Local(l) => {
                            l.init.as_ref().is_some_and(|i| uses_local(&i.expr, name))
                        }
                        rs::Stmt::Expr(x, _) => uses_local(x, name),
                        rs::Stmt::Macro(_) | rs::Stmt::Other => true,
                    })
                })
        }
    }
}

/// The constant a statement advances the local `cursor` by: `i += k` or
/// `i = i + k`, with `k` at least one.
fn advance(stmt: &rs::Stmt, cursor: &str) -> Option<u128> {
    let rs::Stmt::Expr(e, true) = stmt else {
        return None;
    };
    let is_cursor = |x: &rs::RustExpr| matches!(x, rs::RustExpr::Path(p) if p.is_ident(cursor));
    let k = match e {
        rs::RustExpr::Binary(b) if b.op == rs::BinOp::AddAssign && is_cursor(&b.left) => {
            int_literal(&b.right)
        }
        rs::RustExpr::Assign(a) if is_cursor(&a.left) => match &*a.right {
            rs::RustExpr::Binary(b) if b.op == rs::BinOp::Add && is_cursor(&b.left) => {
                int_literal(&b.right)
            }
            _ => None,
        },
        _ => None,
    }?;
    (k >= 1).then_some(k)
}

/// Whether statements leave a loop step other than at its end: a
/// `continue`, `break`, `return` or `?` outside a closure.
fn leaves_step(stmts: &[rs::Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        rs::Stmt::Local(l) => l
            .init
            .as_ref()
            .is_some_and(|i| i.diverge.is_some() || leaves_in(&i.expr)),
        rs::Stmt::Expr(e, _) => leaves_in(e),
        rs::Stmt::Macro(_) | rs::Stmt::Other => false,
    })
}

/// [`leaves_step`] in an expression.
fn leaves_in(e: &rs::RustExpr) -> bool {
    match e {
        rs::RustExpr::Continue(_)
        | rs::RustExpr::Break(_)
        | rs::RustExpr::Return(_)
        | rs::RustExpr::Try(_) => true,
        rs::RustExpr::Closure(_) => false,
        other => {
            let (exprs, blocks) = other.parts();
            exprs.iter().any(|x| leaves_in(x)) || blocks.iter().any(|b| leaves_step(&b.stmts))
        }
    }
}

/// `while i < n { body } rest`: a walk whose state is the locals the body
/// updates, measured by `n - i`. The body advances `i` by a constant once
/// and leaves only at its end, so each step is a helper of the state, and
/// the walk calls itself on the step while the condition holds; its one
/// recursive call carries the theorem that the step decreases `n - i`.
fn while_loop(
    w: &rs::ExprWhile,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    if w.label {
        return Err(Residue::Form("labelled loop".to_string()));
    }
    if matches!(&*w.cond, rs::RustExpr::Let(_)) {
        return Err(Residue::Form(
            "a while let, whose walk the lift reads no measure for".to_string(),
        ));
    }
    let state = loop_state(&w.body.stmts, ctx)?;
    let Some((cursor, bound)) = cursor_of(&w.cond, &state) else {
        return Err(Residue::Form(
            "a while loop whose condition is not a cursor below a bound".to_string(),
        ));
    };
    let steps: Vec<&rs::Stmt> = w
        .body
        .stmts
        .iter()
        .filter(|s| advance(s, &cursor).is_some())
        .collect();
    let elsewhere = w
        .body
        .stmts
        .iter()
        .filter(|s| advance(s, &cursor).is_none())
        .any(|s| scan_stmts(std::slice::from_ref(s)).0.contains(&cursor));
    if steps.len() != 1 || elsewhere || leaves_step(&w.body.stmts) {
        return Err(Residue::Form(
            "a while loop that does not advance its cursor once per step".to_string(),
        ));
    }
    // `i < n` types `i` as `n`: a cursor below a `usize` is one, though
    // the lift read its unsuffixed literal as a `u32`. The bound's type is
    // checked below.
    let mut retyped = ctx.clone();
    match retyped.locals.get_mut(&cursor) {
        Some((_, t @ Ty::Nat(_))) => *t = Ty::Nat(None),
        _ => {
            return Err(Residue::Form(
                "a while loop over a cursor of another type".to_string(),
            ))
        }
    }
    let ctx = &retyped;
    let tys: Vec<Ty> = state.iter().map(|v| ctx.locals[v].1.clone()).collect();
    let state_ty = match tys.as_slice() {
        [one] => one.clone(),
        many => Ty::Tuple(many.to_vec()),
    };
    let acc = acc.saw(&state_ty);
    let Some(step_by) = steps.first().and_then(|s| advance(s, &cursor)) else {
        return Err(Residue::Form(
            "a while loop that does not advance its cursor once per step".to_string(),
        ));
    };
    // The walk takes the cursor apart from the rest of the state: the
    // decrease reads the cursor alone, and its next value is the cursor
    // plus the constant, which the step adds once.
    let others: Vec<String> = state.iter().filter(|v| **v != cursor).cloned().collect();
    let other_tys: Vec<Ty> = others.iter().map(|v| ctx.locals[v].1.clone()).collect();
    let rest_ty = match other_tys.as_slice() {
        [] => None,
        [one] => Some(one.clone()),
        many => Some(Ty::Tuple(many.to_vec())),
    };
    let acc = rest_ty.as_ref().map_or(acc.clone(), |t| acc.saw(t));
    let (acc, before) = acc.apart();
    let mut base = Ctx {
        tails: BTreeMap::new(),
        looping: None,
        exits: None,
        top: false,
        ..ctx.clone()
    };
    // The outer names stay taken inside the walk.
    for (rust, binding) in &ctx.locals {
        base.locals
            .insert(format!("#outer {rust}"), binding.clone());
    }
    let (at, acc) = base.fresh(env, acc, "at");
    let (held, acc) = base.fresh(env, acc, "state");
    let mut scope = base.clone();
    if let Some((old, _)) = scope
        .locals
        .insert(cursor.clone(), (at.clone(), Ty::Nat(None)))
    {
        scope.hidden.push(old);
    }
    // The condition and the bound read the cursor from the walk's
    // parameter: LexLean states a decrease over parameters only.
    let ((cond, ct), acc) = flat_of(&w.cond, env, &scope.below(), acc, Some(&Ty::Bool))?;
    let ((limit, lt), acc) = flat_of(bound, env, &scope.below(), acc, Some(&Ty::Nat(None)))?;
    if ct != Ty::Bool || lt != Ty::Nat(None) {
        return Err(Residue::Form(
            "a while loop over a cursor of another type".to_string(),
        ));
    }
    // The rest of the state after one step, from the state where the
    // condition holds: the cursor is below a length there.
    let mut step_text = Vec::new();
    let mut step_def = None;
    let mut acc = acc;
    if let Some(rt) = &rest_ty {
        let (inner, after, lets) = rebind(env, &scope, acc, &others, &held, Vec::new());
        let mut inner = inner;
        let is_len =
            matches!(bound, rs::RustExpr::MethodCall(m) if m.method == "len" && m.args.is_empty());
        let half = env
            .widths
            .first()
            .and_then(|(w, _)| 1u128.checked_shl(w - 1));
        let room = if is_len { half.unwrap_or(1) } else { 1 };
        inner.capped.insert(at.clone(), room);
        let mut stmts = w.body.stmts.clone();
        stmts.push(rs::Stmt::Expr(state_expr(&others), false));
        let ((step, st), after) = block(&stmts, env, &inner, after, Some(rt))?;
        if !st.same(rt) {
            return Err(Residue::Form("a loop step of another type".to_string()));
        }
        acc = after;
        let step = wrap_lets(lets, step);
        step_text.push(step.flat());
        step_def = Some(step);
    }
    let (acc, own) = acc.rejoin(before);
    if !own.descends.is_empty() {
        return Err(Residue::Form("recursion inside a loop step".to_string()));
    }
    let mut texts = step_text;
    texts.push(cond.clone());
    texts.push(limit.clone());
    let caps = captures(ctx, &texts, &[at.clone(), held.clone()]);
    let cap_names: Vec<String> = caps.iter().map(|(c, _)| c.clone()).collect();
    let mut params = vec![(at.clone(), Ty::Nat(None).render())];
    if let Some(rt) = &rest_ty {
        params.push((held.clone(), rt.render()));
    }
    params.extend(caps.iter().map(|(c, t)| (c.clone(), t.render())));
    let applied = |head: &str, lead: &[String]| {
        let mut parts = vec![head.to_string()];
        parts.extend(lead.iter().cloned());
        parts.extend(cap_names.iter().cloned());
        format!("({})", parts.join(" "))
    };
    let mut lead = vec![format!("({at} + {step_by})")];
    let mut acc = acc;
    let mut own_calls = BTreeSet::new();
    if let Some(step) = step_def {
        let step_name = ctx.helper_name(env, &acc, "Step");
        acc = acc
            .helper(Helper {
                name: step_name.clone(),
                params: params.clone(),
                result: rest_ty.as_ref().map(Ty::render).unwrap_or_default(),
                body: step,
                calls: own.calls,
                axioms: own.axioms,
                recursive: false,
                entwined: false,
                termination: None,
            })
            .called(format!("#{step_name}"));
        own_calls.insert(format!("#{step_name}"));
        lead.push(applied(&step_name, &[at.clone(), held.clone()]));
    }
    // The final state, in the locals' order.
    let items: Vec<String> = state
        .iter()
        .map(|v| match others.iter().position(|o| o == v) {
            None => at.clone(),
            Some(_) if others.len() == 1 => held.clone(),
            Some(j) => tuple_item(rest_ty.as_ref().unwrap_or(&Ty::Unit), &held, j),
        })
        .collect();
    let done = match items.as_slice() {
        [one] => one.clone(),
        _ => tuple_value(&state_ty, &items),
    };
    let measure = |cursor_text: &str| format!("(prim subtract {limit} {cursor_text} : Nat)");
    let name = ctx.helper_name(env, &acc, "Loop");
    let evidence = format!("decreases_{name}_0");
    let axioms: Vec<String> = ["Quot.sound", "propext"].map(String::from).to_vec();
    let theorem = Decl::Theorem {
        name: evidence,
        parameters: params.clone(),
        statement: format!(
            "(({cond} = true) -> ({} < {}))",
            measure(&lead[0]),
            measure(&at)
        ),
        proof: "linear_arithmetic{}".to_string(),
        axioms: axioms.clone(),
    };
    let acc = acc.helper(Helper {
        name: name.clone(),
        params,
        result: state_ty.render(),
        body: Expr::If {
            condition: Box::new(Expr::Flat(cond)),
            then: Box::new(Expr::Flat(applied(&name, &lead))),
            otherwise: Box::new(Expr::Flat(done)),
        },
        calls: own_calls,
        axioms: axioms.into_iter().collect(),
        recursive: false,
        entwined: false,
        termination: Some((measure(&at), vec![theorem])),
    });
    let acc = acc.called(format!("#{name}"));
    // The walk from the state before the loop, then the rest with its end.
    let ((first, _), acc) = flat_of(
        &rs::RustExpr::Path(rs::Path::ident(&cursor)),
        env,
        &ctx.below(),
        acc,
        Some(&Ty::Nat(None)),
    )?;
    let mut start = vec![first];
    if let Some(rt) = &rest_ty {
        let ((init, _), next) = flat_of(
            &state_expr(&others),
            env,
            &ctx.below(),
            acc.clone(),
            Some(rt),
        )?;
        start.push(init);
        return finish_walk(
            applied(&name, &start),
            &state,
            &state_ty,
            rest,
            env,
            ctx,
            next,
            want,
        );
    }
    finish_walk(
        applied(&name, &start),
        &state,
        &state_ty,
        rest,
        env,
        ctx,
        acc,
        want,
    )
}

/// The rest of a block after a walk: the walk's value bound, and the
/// statements after it with the locals its state names taking their final
/// values.
#[allow(clippy::too_many_arguments)]
fn finish_walk(
    call: String,
    state: &[String],
    state_ty: &Ty,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let (whole, acc) = ctx.fresh(env, acc, "state");
    let (after, acc, lets) = rebind(env, ctx, acc, state, &whole, Vec::new());
    let ((rest, rt), acc) = block(rest, env, &after, acc, want)?;
    Ok((
        (
            Expr::Let {
                name: whole,
                ty: state_ty.render(),
                value: Box::new(Expr::Flat(call)),
                body: Box::new(wrap_lets(lets, rest)),
            },
            rt,
        ),
        acc,
    ))
}

/// The first `?` an expression evaluates unconditionally, and the
/// expression with the local `var` in its place: its operand, and the rest.
/// A `?` under a closure, a branch or the right side of `&&`/`||` may not
/// run, so it stays.
fn take_try(e: &rs::RustExpr, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    use rs::RustExpr as E;
    match e {
        E::Try(t) => Some(match take_try(t, var) {
            Some((found, inner)) => (found, E::Try(Box::new(inner))),
            None => ((**t).clone(), E::Path(rs::Path::ident(var))),
        }),
        E::MethodCall(m) => try_in_method(m, var),
        E::Call(c) => try_in_call(c, var),
        E::Field(f) => try_in_field(f, var),
        E::Paren(p) => {
            let (found, inner) = take_try(p, var)?;
            Some((found, E::Paren(Box::new(inner))))
        }
        E::Reference(r) => try_in_reference(r, var),
        E::Unary(u) => try_in_unary(u, var),
        E::Cast(c) => try_in_cast(c, var),
        E::Index(i) => try_in_index(i, var),
        E::Binary(b) => try_in_binary(b, var),
        E::Tuple(items) => {
            let (found, items) = take_first(items, var)?;
            Some((found, E::Tuple(items)))
        }
        E::Struct(st) => try_in_struct(st, var),
        E::If(i) => try_in_if(i, var),
        E::Match(m) => try_in_match(m, var),
        _ => None,
    }
}

/// The first `?` of a method call: in its receiver, else its arguments.
fn try_in_method(m: &rs::ExprMethodCall, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    if let Some((found, receiver)) = take_try(&m.receiver, var) {
        let mut m = m.clone();
        m.receiver = Box::new(receiver);
        return Some((found, rs::RustExpr::MethodCall(m)));
    }
    let (found, args) = take_first(&m.args, var)?;
    let mut m = m.clone();
    m.args = args;
    Some((found, rs::RustExpr::MethodCall(m)))
}

/// The first `?` of a call's arguments.
fn try_in_call(c: &rs::ExprCall, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let (found, args) = take_first(&c.args, var)?;
    let mut c = c.clone();
    c.args = args;
    Some((found, rs::RustExpr::Call(c)))
}

/// The first `?` of a field's base.
fn try_in_field(f: &rs::ExprField, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let (found, base) = take_try(&f.base, var)?;
    let mut f = f.clone();
    f.base = Box::new(base);
    Some((found, rs::RustExpr::Field(f)))
}

/// The first `?` under a reference.
fn try_in_reference(r: &rs::ExprReference, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let (found, inner) = take_try(&r.expr, var)?;
    let mut r = r.clone();
    r.expr = Box::new(inner);
    Some((found, rs::RustExpr::Reference(r)))
}

/// The first `?` under a unary operator.
fn try_in_unary(u: &rs::ExprUnary, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let (found, inner) = take_try(&u.expr, var)?;
    let mut u = u.clone();
    u.expr = Box::new(inner);
    Some((found, rs::RustExpr::Unary(u)))
}

/// The first `?` of a cast's operand.
fn try_in_cast(c: &rs::ExprCast, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let (found, inner) = take_try(&c.expr, var)?;
    let mut c = c.clone();
    c.expr = Box::new(inner);
    Some((found, rs::RustExpr::Cast(c)))
}

/// The first `?` of an index: the indexed value's, else the index's.
fn try_in_index(i: &rs::ExprIndex, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let mut i = i.clone();
    if let Some((found, inner)) = take_try(&i.expr, var) {
        i.expr = Box::new(inner);
        return Some((found, rs::RustExpr::Index(i)));
    }
    let (found, index) = take_try(&i.index, var)?;
    i.index = Box::new(index);
    Some((found, rs::RustExpr::Index(i)))
}

/// The first `?` of a binary operation: the left's, else, where the
/// right is always evaluated, the right's.
fn try_in_binary(b: &rs::ExprBinary, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let mut b = b.clone();
    if let Some((found, left)) = take_try(&b.left, var) {
        b.left = Box::new(left);
        return Some((found, rs::RustExpr::Binary(b)));
    }
    if matches!(b.op, rs::BinOp::And | rs::BinOp::Or) {
        return None;
    }
    let (found, right) = take_try(&b.right, var)?;
    b.right = Box::new(right);
    Some((found, rs::RustExpr::Binary(b)))
}

/// The first `?` of a struct literal's field values.
fn try_in_struct(st: &rs::ExprStruct, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let values: Vec<rs::RustExpr> = st.fields.iter().map(|f| f.expr.clone()).collect();
    let (found, values) = take_first(&values, var)?;
    let mut st = st.clone();
    for (field, value) in st.fields.iter_mut().zip(values) {
        field.expr = value;
    }
    Some((found, rs::RustExpr::Struct(st)))
}

/// The first `?` of an `if`'s condition.
fn try_in_if(i: &rs::ExprIf, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let (found, cond) = take_try(&i.cond, var)?;
    let mut i = i.clone();
    i.cond = Box::new(cond);
    Some((found, rs::RustExpr::If(i)))
}

/// The first `?` of a match's scrutinee.
fn try_in_match(m: &rs::ExprMatch, var: &str) -> Option<(rs::RustExpr, rs::RustExpr)> {
    let (found, scrutinee) = take_try(&m.expr, var)?;
    let mut m = m.clone();
    m.expr = Box::new(scrutinee);
    Some((found, rs::RustExpr::Match(m)))
}

/// [`take_try`] on the first of these expressions that has one.
fn take_first(items: &[rs::RustExpr], var: &str) -> Option<(rs::RustExpr, Vec<rs::RustExpr>)> {
    let (at, (found, replaced)) = items
        .iter()
        .enumerate()
        .find_map(|(i, e)| take_try(e, var).map(|hit| (i, hit)))?;
    let mut items = items.to_vec();
    items[at] = replaced;
    Some((found, items))
}

/// The map and key of `m.entry(k).or_default()`, where `m` is a local.
fn default_entry(e: &rs::RustExpr) -> Option<(&rs::RustExpr, &rs::RustExpr)> {
    let rs::RustExpr::MethodCall(d) = e else {
        return None;
    };
    let rs::RustExpr::MethodCall(en) = &*d.receiver else {
        return None;
    };
    match (
        d.method.as_str(),
        d.args.as_slice(),
        en.method.as_str(),
        en.args.as_slice(),
    ) {
        ("or_default", [], "entry", [k]) if matches!(&*en.receiver, rs::RustExpr::Path(p) if p.get_ident().is_some()) => {
            Some((&en.receiver, k))
        }
        _ => None,
    }
}

/// An update through an entry, `m.entry(k).or_default().push(v);` or
/// `*m.entry(k).or_default() += v;`, as the statements that read the entry
/// into the local `entry{n}`, update it, and insert it back.
fn entry_update(stmt: &rs::Stmt, n: usize) -> Option<Vec<rs::Stmt>> {
    let rs::Stmt::Expr(e, true) = stmt else {
        return None;
    };
    let var = format!("entry{n}");
    let local = rs::RustExpr::Path(rs::Path::ident(&var));
    let ((map, key), update) = match e {
        rs::RustExpr::MethodCall(m) => {
            let found = default_entry(&m.receiver)?;
            let update = rs::RustExpr::MethodCall(rs::ExprMethodCall {
                receiver: Box::new(local.clone()),
                ..m.clone()
            });
            (found, update)
        }
        rs::RustExpr::Binary(b) if b.op == rs::BinOp::AddAssign => {
            let rs::RustExpr::Unary(u) = &*b.left else {
                return None;
            };
            if !matches!(u.op, rs::UnOp::Deref) {
                return None;
            }
            let found = default_entry(&u.expr)?;
            let update = rs::RustExpr::Binary(rs::ExprBinary {
                left: Box::new(local.clone()),
                ..b.clone()
            });
            (found, update)
        }
        _ => return None,
    };
    let call = |receiver: rs::RustExpr, method: &str, args: Vec<rs::RustExpr>| {
        rs::RustExpr::MethodCall(rs::ExprMethodCall {
            receiver: Box::new(receiver),
            method: method.to_string(),
            turbofish: None,
            args,
        })
    };
    let borrowed = rs::RustExpr::Reference(rs::ExprReference {
        mutable: false,
        expr: Box::new(key.clone()),
    });
    let read = call(
        call(
            call(map.clone(), "get", vec![borrowed]),
            "cloned",
            Vec::new(),
        ),
        "unwrap_or_default",
        Vec::new(),
    );
    let bind = rs::Stmt::Local(rs::Local {
        pat: rs::Pat::Ident(rs::PatIdent {
            ident: var,
            by_ref: false,
            subpat: None,
        }),
        init: Some(rs::LocalInit {
            expr: Box::new(read),
            diverge: None,
        }),
    });
    let write = call(map.clone(), "insert", vec![key.clone(), local]);
    Some(vec![
        bind,
        rs::Stmt::Expr(update, true),
        rs::Stmt::Expr(write, true),
    ])
}

/// A statement with a `?` inside it, as `let v = e?;` before the statement
/// with `v` in its place.
fn hoist_try(stmt: &rs::Stmt, n: usize) -> Option<(rs::Stmt, rs::Stmt)> {
    let var = format!("tried{n}");
    let (inner, stmt) = match stmt {
        rs::Stmt::Expr(e, semi) => {
            let (inner, e) = take_try(e, &var)?;
            (inner, rs::Stmt::Expr(e, *semi))
        }
        rs::Stmt::Local(l) => {
            let init = l.init.as_ref()?;
            if matches!(&*init.expr, rs::RustExpr::Try(_)) && matches!(l.pat, rs::Pat::Ident(_)) {
                return None;
            }
            let (inner, e) = take_try(&init.expr, &var)?;
            let mut l = l.clone();
            if let Some(init) = &mut l.init {
                *init.expr = e;
            }
            (inner, rs::Stmt::Local(l))
        }
        rs::Stmt::Macro(_) | rs::Stmt::Other => return None,
    };
    let binding = rs::Stmt::Local(rs::Local {
        pat: rs::Pat::Ident(rs::PatIdent {
            ident: var,
            by_ref: false,
            subpat: None,
        }),
        init: Some(rs::LocalInit {
            expr: Box::new(rs::RustExpr::Try(Box::new(inner))),
            diverge: None,
        }),
    });
    Some((binding, stmt))
}

fn block(
    stmts: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    match stmts.first() {
        Some(rs::Stmt::Expr(rs::RustExpr::Continue(label), _)) => {
            return loop_exit(env, ctx, acc, *label, false);
        }
        Some(rs::Stmt::Expr(rs::RustExpr::Break(b), _)) if b.expr.is_none() => {
            return loop_exit(env, ctx, acc, b.label, true);
        }
        _ => {}
    }
    if let Some((first, rest)) = stmts.split_first() {
        if let Some(rewritten) = rewritten(first, ctx, &acc) {
            return block_rewritten(rewritten, rest, env, ctx, acc, want);
        }
    }
    match stmts {
        // A block with no value is `()`.
        [] if want == Some(&Ty::Unit) => Ok(((Expr::Flat("()".into()), Ty::Unit), acc)),
        // The arms of a match and the branches of an if in tail position
        // are in tail position too: a `return` there leaves the function.
        [rs::Stmt::Expr(e @ (rs::RustExpr::Match(_) | rs::RustExpr::If(_)), false)] => {
            expr(e, env, ctx, acc, want)
        }
        [rs::Stmt::Expr(e, false)] if !(ctx.top && matches!(e, rs::RustExpr::Return(_))) => {
            below(e, env, ctx, acc, want)
        }
        // `let P = match s { .., _ => return v }; rest`: the match, with
        // the binding and the rest in each arm that does not leave.
        [rs::Stmt::Local(l), rest @ ..] if ctx.top && leaving_match(l).is_some() => {
            leaving_let(l, rest, env, ctx, acc, want)
        }
        [rs::Stmt::Expr(rs::RustExpr::Return(r), _), ..] if ctx.top => {
            early_return(r.as_deref(), env, ctx, acc)
        }
        // `if c { return v; } rest`: an early exit from the function.
        [rs::Stmt::Expr(rs::RustExpr::If(i), _), rest @ ..]
            if ctx.top
                && !rest.is_empty()
                && i.else_branch.is_none()
                && returned(&i.then_branch).is_some() =>
        {
            early_if(i, rest, env, ctx, acc, want)
        }
        // `let P = e else { return v; }; rest`: a match with an early exit.
        [rs::Stmt::Local(local), rest @ ..]
            if ctx.top
                && !rest.is_empty()
                && local.init.as_ref().is_some_and(|i| i.diverge.is_some()) =>
        {
            let_else(local, rest, env, ctx, acc, want)
        }
        // `let (a, b) = e; rest`: the tuple, then its items.
        [rs::Stmt::Local(local), rest @ ..]
            if !rest.is_empty()
                && matches!(local.pat, rs::Pat::Tuple(_))
                && local.init.as_ref().is_some_and(|i| {
                    i.diverge.is_none() && !matches!(*i.expr, rs::RustExpr::Try(_))
                }) =>
        {
            let_tuple(local, rest, env, ctx, acc, want)
        }
        // `for p in xs { .. }`: a walk with the updated locals as its state.
        [rs::Stmt::Expr(rs::RustExpr::ForLoop(f), _), rest @ ..] if !rest.is_empty() => {
            for_loop(f, rest, env, ctx, acc, want)
        }
        // `while i < n { .. }`: a walk measured by `n - i`.
        [rs::Stmt::Expr(rs::RustExpr::While(w), _), rest @ ..] if !rest.is_empty() => {
            while_loop(w, rest, env, ctx, acc, want)
        }
        // `x = e;`, `x += s;`, `x.push(e);`: the local's next value.
        [rs::Stmt::Expr(e, true), rest @ ..] if !rest.is_empty() && update_target(e).is_some() => {
            update_stmt(e, rest, env, ctx, acc, want)
        }
        // `()` as a statement does nothing, nor does a local's name: what
        // `x?;` leaves once its `?` is bound.
        [rs::Stmt::Expr(rs::RustExpr::Tuple(t), _), rest @ ..]
            if t.is_empty() && !rest.is_empty() =>
        {
            block(rest, env, ctx, acc, want)
        }
        [rs::Stmt::Expr(rs::RustExpr::Path(p), true), rest @ ..]
            if !rest.is_empty() && p.get_ident().is_some_and(|i| ctx.locals.contains_key(i)) =>
        {
            block(rest, env, ctx, acc, want)
        }
        // `match e { p => .. } rest`: the rest after each arm.
        [rs::Stmt::Expr(rs::RustExpr::Match(m), _), rest @ ..] if !rest.is_empty() => {
            match_rest(m, rest, env, ctx, acc, want)
        }
        // `if let p = e { .. } [else { .. }] rest`: the rest after each branch.
        [rs::Stmt::Expr(rs::RustExpr::If(i), _), rest @ ..]
            if !rest.is_empty()
                && matches!(&*i.cond, rs::RustExpr::Let(_))
                && returned(&i.then_branch).is_none() =>
        {
            if_let_rest(i, rest, env, ctx, acc, want)
        }
        // `if c { .. } [else { .. }] rest` with statements in its branches: the
        // rest after each branch.
        [rs::Stmt::Expr(rs::RustExpr::If(i), _), rest @ ..]
            if !rest.is_empty()
                && !matches!(&*i.cond, rs::RustExpr::Let(_))
                && returned(&i.then_branch).is_none() =>
        {
            if_rest(i, rest, env, ctx, acc, want)
        }
        [rs::Stmt::Local(local), rest @ ..] if !rest.is_empty() => {
            let_stmt(local, rest, env, ctx, acc, want)
        }
        _ => Err(Residue::Statements),
    }
}

/// A statement the block reads as others: `m.entry(k).or_default().push(v);`
/// as the entry read, updated and written back, and, among the function's
/// own statements, a `?` hoisted to a binding of its own. With them, the
/// fresh binder the rewrite takes.
fn rewritten(first: &rs::Stmt, ctx: &Ctx, acc: &Acc) -> Option<(Vec<rs::Stmt>, String)> {
    let n = acc.given.len() + 1;
    if let Some(written) = entry_update(first, n) {
        return Some((written, format!("entry{n}")));
    }
    if !ctx.top {
        return None;
    }
    let (binding, stmt) = hoist_try(first, n)?;
    Some((vec![binding, stmt], format!("tried{n}")))
}

/// The block with its first statement rewritten, the rewrite's binder taken.
fn block_rewritten(
    rewritten: (Vec<rs::Stmt>, String),
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let (mut out, binder) = rewritten;
    let mut given = acc.given;
    given.push(binder);
    let acc = Acc { given, ..acc };
    out.extend(rest.iter().cloned());
    block(&out, env, ctx, acc, want)
}

/// An expression below the function's own statements.
fn below(
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    expr(e, env, &ctx.below(), acc, want)
}

/// `let P = match s { .., _ => return v }; rest`: the match, with the
/// binding and the rest in each arm that does not leave.
fn leaving_let(
    l: &rs::Local,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(m) = leaving_match(l) else {
        return Err(Residue::Statements);
    };
    let arms = m
        .arms
        .iter()
        .map(|a| {
            let body = if leaves(&a.body) {
                (*a.body).clone()
            } else {
                let mut stmts = vec![rs::Stmt::Local(rs::Local {
                    pat: l.pat.clone(),
                    init: Some(rs::LocalInit {
                        expr: a.body.clone(),
                        diverge: None,
                    }),
                })];
                stmts.extend(rest.iter().cloned());
                rs::RustExpr::Block(rs::ExprBlock {
                    label: false,
                    block: rs::Block { stmts },
                })
            };
            rs::MatchArm {
                pat: a.pat.clone(),
                body: Box::new(body),
            }
        })
        .collect();
    let whole = rs::RustExpr::Match(rs::ExprMatch {
        expr: m.expr.clone(),
        arms,
    });
    expr(&whole, env, ctx, acc, want)
}

/// `return v` among the function's own statements: the function's value.
fn early_return(
    r: Option<&rs::RustExpr>,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
) -> Lift<((Expr, Ty), Acc)> {
    match r {
        Some(v) => {
            let (value, acc) = expr(v, env, &ctx.below(), acc, Some(&ctx.result))?;
            Ok((ctx.returning(value), acc))
        }
        None if ctx.result == Ty::Unit => {
            Ok((ctx.returning((Expr::Flat("()".into()), Ty::Unit)), acc))
        }
        None => Err(Residue::Type("()".to_string())),
    }
}

/// `if c { return v; } rest`: an early exit from the function.
fn early_if(
    i: &rs::ExprIf,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(v) = returned(&i.then_branch) else {
        return Err(Residue::Statements);
    };
    let inner = ctx.below();
    if matches!(&*i.cond, rs::RustExpr::Let(_)) {
        let wild = rs::Pat::Wild;
        let rs::RustExpr::Let(l) = &*i.cond else {
            return Err(Residue::Statements);
        };
        let tail = rs::Block {
            stmts: rest.to_vec(),
        };
        let early = rs::Block {
            stmts: vec![rs::Stmt::Expr(leaving_value(v, ctx), false)],
        };
        let arms = vec![(&l.pat, Body::Block(&early)), (&wild, Body::Block(&tail))];
        return matching(&l.expr, &arms, env, ctx, acc, want);
    }
    let ((c, ct), acc) = flat_of(&i.cond, env, &inner, acc, Some(&Ty::Bool))?;
    if ct != Ty::Bool {
        return Err(Residue::Form("non-boolean condition".to_string()));
    }
    let (value, acc) = expr(v, env, &inner, acc, Some(&ctx.result))?;
    if !value.1.same(&ctx.result) {
        return Err(Residue::Form("branches of different types".to_string()));
    }
    let (then, tt) = ctx.returning(value);
    let after = ctx.assuming(&i.cond, false, env, &acc);
    let ((other, ot), acc) = block(rest, env, &after, acc, want)?;
    if !ot.same(&tt) {
        return Err(Residue::Form("branches of different types".to_string()));
    }
    Ok((
        (
            Expr::If {
                condition: Box::new(Expr::Flat(c)),
                then: Box::new(then),
                otherwise: Box::new(other),
            },
            tt,
        ),
        acc,
    ))
}

/// `let P = e else { return v; }; rest`: a match with an early exit.
fn let_else(
    local: &rs::Local,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(init) = &local.init else {
        return Err(Residue::Statements);
    };
    let Some(otherwise) = &init.diverge else {
        return Err(Residue::Statements);
    };
    let rs::RustExpr::Block(eb) = &**otherwise else {
        return Err(Residue::Form("let else".to_string()));
    };
    // A `continue` or `break` in a loop step leaves the step.
    let skips = ctx.looping.is_some()
        && matches!(
            eb.block.stmts.as_slice(),
            [rs::Stmt::Expr(
                rs::RustExpr::Continue(_) | rs::RustExpr::Break(_),
                _
            )]
        );
    let early = match returned(&eb.block) {
        Some(v) => rs::Block {
            stmts: vec![rs::Stmt::Expr(leaving_value(v, ctx), false)],
        },
        None if skips => eb.block.clone(),
        None => return Err(Residue::Form("let else".to_string())),
    };
    let wild = rs::Pat::Wild;
    let tail = rs::Block {
        stmts: rest.to_vec(),
    };
    let arms = vec![
        (&local.pat, Body::Block(&tail)),
        (&wild, Body::Block(&early)),
    ];
    matching(&init.expr, &arms, env, ctx, acc, want)
}

/// `let (a, b) = e; rest`: the tuple, then its items.
fn let_tuple(
    local: &rs::Local,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(init) = &local.init else {
        return Err(Residue::Statements);
    };
    let ((value, vt), acc) = flat_of(&init.expr, env, &ctx.below(), acc, None)?;
    let (scope, whole, acc, lets) = bind_pat(&local.pat, &vt, env, ctx, acc, Vec::new())?;
    let ((body, bt), acc) = block(rest, env, &scope, acc, want)?;
    let body = lets
        .into_iter()
        .rev()
        .fold(body, |body, (name, ty, value)| Expr::Let {
            name,
            ty,
            value: Box::new(Expr::Flat(value)),
            body: Box::new(body),
        });
    Ok((
        (
            Expr::Let {
                name: whole,
                ty: vt.render(),
                value: Box::new(Expr::Flat(value)),
                body: Box::new(body),
            },
            bt,
        ),
        acc,
    ))
}

/// `x = e;`, `x += s;`, `x.push(e);`: the local's next value.
fn update_stmt(
    e: &rs::RustExpr,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(target) = update_target(e) else {
        return Err(Residue::Statements);
    };
    let Some((current, ty)) = ctx.locals.get(&target).cloned() else {
        return Err(Residue::Form(format!("an update of {target}")));
    };
    let (value, acc) = updated(e, &ty, &current, env, ctx, acc)?;
    let (scope, name, acc) = ctx.bind(env, acc, &target, ty.clone());
    let ((rest, rt), acc) = block(rest, env, &scope, acc, want)?;
    Ok((
        (
            Expr::Let {
                name,
                ty: ty.render(),
                value: Box::new(Expr::Flat(value)),
                body: Box::new(rest),
            },
            rt,
        ),
        acc,
    ))
}

/// `match e { p => .. } rest`: the rest after each arm.
fn match_rest(
    m: &rs::ExprMatch,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let bodies: Vec<rs::RustExpr> = m.arms.iter().map(|a| then_rest(&a.body, rest)).collect();
    let arms: Vec<(&rs::Pat, Body<'_>)> = m
        .arms
        .iter()
        .zip(&bodies)
        .map(|(a, b)| match &a.pat {
            rs::Pat::Guard(g) => (&*g.pat, Body::Guarded(&g.guard, b)),
            p => (p, Body::Expr(b)),
        })
        .collect();
    matching(&m.expr, &arms, env, ctx, acc, want)
}

/// `if let p = e { .. } [else { .. }] rest`: the rest after each branch.
fn if_let_rest(
    i: &rs::ExprIf,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let rs::RustExpr::Let(l) = &*i.cond else {
        return Err(Residue::Statements);
    };
    if let Some(state) = branch_state(i, ctx) {
        let tail = [rs::Stmt::Expr(state_expr(&state), false)];
        let then = then_rest(
            &rs::RustExpr::Block(rs::ExprBlock {
                label: false,
                block: i.then_branch.clone(),
            }),
            &tail,
        );
        let unit = rs::RustExpr::Tuple(Vec::new());
        let other = then_rest(i.else_branch.as_deref().unwrap_or(&unit), &tail);
        let wild = rs::Pat::Wild;
        let arms = vec![(&l.pat, Body::Expr(&then)), (&wild, Body::Expr(&other))];
        let state_ty = state_type(&state, ctx);
        let (value, acc) = matching(&l.expr, &arms, env, &ctx.below(), acc, Some(&state_ty))?;
        return after_state(value, &state, rest, env, ctx, acc, want);
    }
    let then = then_rest(
        &rs::RustExpr::Block(rs::ExprBlock {
            label: false,
            block: i.then_branch.clone(),
        }),
        rest,
    );
    let unit = rs::RustExpr::Tuple(Vec::new());
    let other = then_rest(i.else_branch.as_deref().unwrap_or(&unit), rest);
    let wild = rs::Pat::Wild;
    let arms = vec![(&l.pat, Body::Expr(&then)), (&wild, Body::Expr(&other))];
    matching(&l.expr, &arms, env, ctx, acc, want)
}

/// `if c { .. } [else { .. }] rest` with statements in its branches: the
/// rest after each branch.
fn if_rest(
    i: &rs::ExprIf,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let ((c, ct), acc) = flat_of(&i.cond, env, &ctx.below(), acc, Some(&Ty::Bool))?;
    if ct != Ty::Bool {
        return Err(Residue::Form("non-boolean condition".to_string()));
    }
    if let Some(state) = branch_state(i, ctx) {
        let state_ty = state_type(&state, ctx);
        let tail = [rs::Stmt::Expr(state_expr(&state), false)];
        let then = settled(&i.then_branch.stmts, &tail);
        let other = match &i.else_branch {
            None => tail.to_vec(),
            Some(e) => match &**e {
                rs::RustExpr::Block(b) => settled(&b.block.stmts, &tail),
                rs::RustExpr::If(_) => settled(&[rs::Stmt::Expr((**e).clone(), false)], &tail),
                _ => return Err(Residue::Statements),
            },
        };
        let inner = ctx.below();
        let yes = inner.assuming(&i.cond, true, env, &acc);
        let no = inner.assuming(&i.cond, false, env, &acc);
        let ((a, at), acc) = block(&then, env, &yes, acc, Some(&state_ty))?;
        let ((b, bt), acc) = block(&other, env, &no, acc, Some(&state_ty))?;
        if !at.same(&state_ty) || !bt.same(&state_ty) {
            return Err(Residue::Form("branches of different types".to_string()));
        }
        let value = Expr::If {
            condition: Box::new(Expr::Flat(c)),
            then: Box::new(a),
            otherwise: Box::new(b),
        };
        return after_state((value, state_ty), &state, rest, env, ctx, acc, want);
    }
    let mut then: Vec<rs::Stmt> = i.then_branch.stmts.clone();
    then.extend(rest.iter().cloned());
    let mut other: Vec<rs::Stmt> = match &i.else_branch {
        None => Vec::new(),
        Some(e) => match &**e {
            rs::RustExpr::Block(b) => b.block.stmts.clone(),
            rs::RustExpr::If(_) => vec![rs::Stmt::Expr((**e).clone(), false)],
            _ => return Err(Residue::Statements),
        },
    };
    other.extend(rest.iter().cloned());
    let ((a, at), acc) = block(&then, env, ctx, acc, want)?;
    let ((b, bt), acc) = block(&other, env, ctx, acc, Some(want.unwrap_or(&at)))?;
    if !at.same(&bt) {
        return Err(Residue::Form("branches of different types".to_string()));
    }
    Ok((
        (
            Expr::If {
                condition: Box::new(Expr::Flat(c)),
                then: Box::new(a),
                otherwise: Box::new(b),
            },
            at,
        ),
        acc,
    ))
}

/// The locals the branches of an `if` that is a statement update, when
/// the branches only update them: no branch leaves the statement, and
/// none shadows a local it updates. The `if` is then the value of those
/// locals after it, and the statements after it lift once, in its scope.
fn branch_state(i: &rs::ExprIf, ctx: &Ctx) -> Option<Vec<String>> {
    let mut stmts = i.then_branch.stmts.clone();
    if let Some(e) = &i.else_branch {
        stmts.push(rs::Stmt::Expr((**e).clone(), true));
    }
    if leaves_step(&stmts) {
        return None;
    }
    let (updated, declared) = scan_stmts(&stmts);
    if updated.iter().any(|u| declared.contains(u)) {
        return None;
    }
    let state: Vec<String> = updated
        .into_iter()
        .filter(|u| ctx.locals.contains_key(u))
        .collect();
    (!state.is_empty()).then_some(state)
}

/// The type of the locals `state` names, as one value.
fn state_type(state: &[String], ctx: &Ctx) -> Ty {
    let tys: Vec<Ty> = state.iter().map(|v| ctx.locals[v].1.clone()).collect();
    match tys.as_slice() {
        [] => Ty::Unit,
        [one] => one.clone(),
        many => Ty::Tuple(many.to_vec()),
    }
}

/// A branch's statements, each a statement, then `tail`.
fn settled(stmts: &[rs::Stmt], tail: &[rs::Stmt]) -> Vec<rs::Stmt> {
    let mut out: Vec<rs::Stmt> = stmts
        .iter()
        .map(|s| match s {
            rs::Stmt::Expr(e, _) => rs::Stmt::Expr(e.clone(), true),
            other => other.clone(),
        })
        .collect();
    out.extend(tail.iter().cloned());
    out
}

/// The statements after a branching statement, in the scope of the state
/// it leaves: the state bound once, its locals rebound to its items.
fn after_state(
    value: (Expr, Ty),
    state: &[String],
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let (value, state_ty) = value;
    let acc = acc.saw(&state_ty);
    let (whole, acc) = ctx.fresh(env, acc, "state");
    let (after, acc, lets) = rebind(env, ctx, acc, state, &whole, Vec::new());
    let ((rest, rt), acc) = block(rest, env, &after, acc, want)?;
    Ok((
        (
            Expr::Let {
                name: whole,
                ty: state_ty.render(),
                value: Box::new(value),
                body: Box::new(wrap_lets(lets, rest)),
            },
            rt,
        ),
        acc,
    ))
}

/// `let x = e; rest`: the binding, then the rest in its scope.
fn let_stmt(
    local: &rs::Local,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let (ident, declared) = match &local.pat {
        rs::Pat::Ident(i) if i.subpat.is_none() && !i.by_ref => (i, None),
        rs::Pat::Type(pt) => match &*pt.pat {
            rs::Pat::Ident(i) if i.subpat.is_none() && !i.by_ref => {
                (i, Some(env.types.ty(&pt.ty.localized(&ctx.imports))?))
            }
            other => return let_pattern(other, local, rest, env, ctx, acc, want),
        },
        other => return let_pattern(other, local, rest, env, ctx, acc, want),
    };
    let Some(init) = &local.init else {
        return Err(Residue::Statements);
    };
    if init.diverge.is_some() {
        return let_pattern(&local.pat, local, rest, env, ctx, acc, want);
    }
    if let rs::RustExpr::Closure(c) = &*init.expr {
        if c.asyncness || declared.is_some() {
            return Err(Residue::Form("expr::closure".to_string()));
        }
        let mut scope = ctx.clone();
        scope.closures.insert(
            ident.ident.clone(),
            Closure {
                inputs: c.inputs.clone(),
                body: (*c.body).clone(),
                locals: ctx.locals.clone(),
            },
        );
        return block(rest, env, &scope, acc, want);
    }
    if let rs::RustExpr::Try(t) = &*init.expr {
        if !ctx.top {
            return Err(Residue::Form(
                "? below the function's own statements".to_string(),
            ));
        }
        return question(t, &ident.ident, declared, rest, env, ctx, acc, want);
    }
    // An empty vector the block goes on to return takes the type the
    // block returns.
    let hint = match &declared {
        Some(d) => Some(d.clone()),
        None if is_empty_vec(&init.expr) => returned_type(&ident.ident.clone(), rest, ctx, want)
            .or_else(|| used_type(&ident.ident, &init.expr, rest, env, ctx)),
        None => None,
    };
    let ((value, vt), acc) = flat_of(&init.expr, env, &ctx.below(), acc, hint.as_ref())?;
    if declared.as_ref().is_some_and(|d| !d.same(&vt)) {
        return Err(Residue::Form("let of another type".to_string()));
    }
    let (scope, name, acc) = ctx.bind(env, acc, &ident.ident.clone(), vt.clone());
    let ((rest, rt), acc) = block(rest, env, &scope, acc, want)?;
    Ok((
        (
            Expr::Let {
                name,
                ty: vt.render(),
                value: Box::new(Expr::Flat(value)),
                body: Box::new(rest),
            },
            rt,
        ),
        acc,
    ))
}

/// `let P = e; rest` where `P` takes every value of `e`: a match of one arm.
fn let_pattern(
    pat: &rs::Pat,
    local: &rs::Local,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let Some(init) = &local.init else {
        return Err(Residue::Statements);
    };
    if matches!(&*init.expr, rs::RustExpr::Try(_)) {
        return Err(Residue::Form("let pattern".to_string()));
    }
    let tail = rs::Block {
        stmts: rest.to_vec(),
    };
    let wild = rs::Pat::Wild;
    // `let P = e else { .. };`: the block the other values take, such as
    // a `continue` of the loop the statements are a step of.
    let arms = match init.diverge.as_deref() {
        None => vec![(pat, Body::Block(&tail))],
        Some(rs::RustExpr::Block(b)) if !b.label => {
            vec![(pat, Body::Block(&tail)), (&wild, Body::Block(&b.block))]
        }
        Some(_) => return Err(Residue::Form("let else".to_string())),
    };
    matching(&init.expr, &arms, env, ctx, acc, want)
}

/// The value of an early exit, as the arm that takes it states it: the
/// value itself, or in a loop step a `return` of it, which leaves the walk.
fn leaving_value(v: &rs::RustExpr, ctx: &Ctx) -> rs::RustExpr {
    match ctx.exits {
        Some(_) => rs::RustExpr::Return(Some(Box::new(v.clone()))),
        None => v.clone(),
    }
}

/// Whether an arm's value leaves the function: a `return`, alone or as
/// the last statement of a block.
fn leaves(e: &rs::RustExpr) -> bool {
    match e {
        rs::RustExpr::Return(_) => true,
        rs::RustExpr::Block(b) if !b.label => matches!(
            b.block.stmts.last(),
            Some(rs::Stmt::Expr(rs::RustExpr::Return(_), _))
        ),
        _ => false,
    }
}

/// The match a `let` binds where an arm of it leaves the function.
fn leaving_match(l: &rs::Local) -> Option<&rs::ExprMatch> {
    let init = l.init.as_ref()?;
    if init.diverge.is_some() {
        return None;
    }
    match &*init.expr {
        rs::RustExpr::Match(m) if m.arms.iter().any(|a| leaves(&a.body)) => Some(m),
        _ => None,
    }
}

/// `Vec::new()` or `Vec::with_capacity(n)`: a vector whose element type the
/// expression does not say.
fn is_empty_vec(e: &rs::RustExpr) -> bool {
    let rs::RustExpr::Call(c) = e else {
        return false;
    };
    let rs::RustExpr::Path(p) = &*c.func else {
        return false;
    };
    let names: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
    matches!(
        (
            names
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
            c.args.len()
        ),
        (["Vec" | "BTreeSet" | "BTreeMap", "new"], 0) | (["Vec", "with_capacity"], 1)
    )
}

/// How a later statement uses an empty collection: it adds an element,
/// an entry, or it is passed where a type is declared.
enum Use {
    Element(Ty),
    Entry(Ty, Ty),
    Whole(Ty),
}

/// The type an empty collection bound to `ident` takes from its first use
/// in the statements after it whose type the lifted signatures say: an
/// element `insert`ed, `push`ed or `extend`ed with, an entry `insert`ed,
/// or an argument of a lifted function's declared type.
fn used_type(
    ident: &str,
    init: &rs::RustExpr,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
) -> Option<Ty> {
    let rs::RustExpr::Call(c) = init else {
        return None;
    };
    let rs::RustExpr::Path(p) = &*c.func else {
        return None;
    };
    let kind = p.segments.first()?.ident.clone();
    let locals: BTreeMap<String, Ty> = ctx
        .locals
        .iter()
        .map(|(r, (_, t))| (r.clone(), t.clone()))
        .collect();
    match (kind.as_str(), use_in_stmts(ident, rest, env, &locals)?) {
        (_, Use::Whole(t)) => Some(t),
        ("Vec", Use::Element(t)) => Some(Ty::List(Box::new(t))),
        ("BTreeSet", Use::Element(t)) => Some(Ty::Set(Box::new(t), true)),
        ("BTreeMap", Use::Entry(k, v)) => Some(Ty::Map(Box::new(k), Box::new(v), true)),
        _ => None,
    }
}

/// [`used_type`] over statements, with the locals they bind in turn.
fn use_in_stmts(
    ident: &str,
    stmts: &[rs::Stmt],
    env: &Env,
    locals: &BTreeMap<String, Ty>,
) -> Option<Use> {
    let mut scope = locals.clone();
    for stmt in stmts {
        match stmt {
            rs::Stmt::Local(l) => {
                let init = l.init.as_ref()?;
                if let Some(u) = use_in(ident, &init.expr, env, &scope) {
                    return Some(u);
                }
                let ty = match &l.pat {
                    rs::Pat::Type(pt) => env.types.ty(&pt.ty).ok(),
                    _ => static_type(&init.expr, env, &scope),
                };
                scope = typed_binders(&l.pat, ty.as_ref(), scope);
            }
            rs::Stmt::Expr(e, _) => {
                if let Some(u) = use_in(ident, e, env, &scope) {
                    return Some(u);
                }
            }
            rs::Stmt::Macro(_) | rs::Stmt::Other => {}
        }
    }
    None
}

/// The locals with the binders of `pat` typed from `ty`, the type of the
/// value it matches: an identifier takes it, a tuple's items take its
/// items'. A binder of a type not known is dropped.
fn typed_binders(
    pat: &rs::Pat,
    ty: Option<&Ty>,
    locals: BTreeMap<String, Ty>,
) -> BTreeMap<String, Ty> {
    match (pat, ty) {
        (rs::Pat::Ident(i), Some(t)) if i.subpat.is_none() => {
            let mut locals = locals;
            locals.insert(i.ident.clone(), t.clone());
            locals
        }
        (rs::Pat::Ident(i), _) => locals
            .into_iter()
            .filter(|(name, _)| *name != i.ident)
            .collect(),
        (rs::Pat::Type(pt), _) => typed_binders(&pt.pat, ty, locals),
        (rs::Pat::Reference(inner), _) => typed_binders(inner, ty, locals),
        (rs::Pat::Tuple(items), Some(Ty::Tuple(ts) | Ty::Pair(ts))) if items.len() == ts.len() => {
            items
                .iter()
                .zip(ts)
                .fold(locals, |scope, (p, t)| typed_binders(p, Some(t), scope))
        }
        (rs::Pat::Tuple(items), _) => items
            .iter()
            .fold(locals, |scope, p| typed_binders(p, None, scope)),
        _ => locals,
    }
}

/// [`used_type`] in an expression.
fn use_in(ident: &str, e: &rs::RustExpr, env: &Env, locals: &BTreeMap<String, Ty>) -> Option<Use> {
    let named = |x: &rs::RustExpr| match x {
        rs::RustExpr::Reference(r) => {
            matches!(&*r.expr, rs::RustExpr::Path(p) if p.is_ident(ident))
        }
        rs::RustExpr::Path(p) => p.is_ident(ident),
        _ => false,
    };
    match e {
        rs::RustExpr::MethodCall(m) if named(&m.receiver) => {
            let ty = |x: &rs::RustExpr| static_type(x, env, locals);
            match (m.method.as_str(), m.args.as_slice()) {
                ("insert" | "push", [x]) => ty(x).map(Use::Element),
                ("insert", [k, v]) => Some(Use::Entry(ty(k)?, ty(v)?)),
                ("extend", [x]) => match ty(x)? {
                    Ty::List(t) | Ty::Set(t, _) => Some(Use::Element(*t)),
                    _ => None,
                },
                _ => None,
            }
        }
        rs::RustExpr::Call(c) => match c.args.iter().position(named) {
            Some(i) => {
                let rs::RustExpr::Path(p) = &*c.func else {
                    return None;
                };
                let sig = env.fns.get(&p.segments.last()?.ident)?;
                sig.params.get(i).cloned().map(Use::Whole)
            }
            None => c.args.iter().find_map(|a| use_in(ident, a, env, locals)),
        },
        rs::RustExpr::ForLoop(f) => {
            if let Some(u) = use_in(ident, &f.expr, env, locals) {
                return Some(u);
            }
            let mut inner = locals.clone();
            if let rs::Pat::Ident(i) = &f.pat {
                let elem = match static_type(&f.expr, env, locals) {
                    Some(Ty::List(t) | Ty::Set(t, _)) => Some(*t),
                    Some(m @ Ty::Map(..)) => Some(m.entry()),
                    _ => None,
                };
                match elem {
                    Some(t) => inner.insert(i.ident.clone(), t),
                    None => inner.remove(&i.ident),
                };
            }
            use_in_stmts(ident, &f.body.stmts, env, &inner)
        }
        rs::RustExpr::Closure(_) => None,
        other => {
            let (exprs, blocks) = other.parts();
            exprs
                .iter()
                .find_map(|x| use_in(ident, x, env, locals))
                .or_else(|| {
                    blocks
                        .iter()
                        .find_map(|b| use_in_stmts(ident, &b.stmts, env, locals))
                })
        }
    }
}

/// The type of an expression of locals, borrows, clones and calls of
/// lifted functions and methods, read from their signatures.
fn static_type(e: &rs::RustExpr, env: &Env, locals: &BTreeMap<String, Ty>) -> Option<Ty> {
    match e {
        rs::RustExpr::Path(p) => match p.get_ident() {
            Some(ident) => locals.get(ident).cloned(),
            None => variant_owner(p, env),
        },
        rs::RustExpr::Reference(r) => static_type(&r.expr, env, locals),
        rs::RustExpr::Paren(x) => static_type(x, env, locals),
        // `x?`: the value an option or result holds.
        rs::RustExpr::Try(x) => match static_type(x, env, locals)? {
            Ty::Option(t) | Ty::Outcome(t, _) => Some(*t),
            _ => None,
        },
        rs::RustExpr::MethodCall(m)
            if m.args.is_empty()
                && matches!(
                    m.method.as_str(),
                    "iter" | "into_iter" | "clone" | "to_owned" | "cloned"
                ) =>
        {
            static_type(&m.receiver, env, locals)
        }
        rs::RustExpr::MethodCall(m) if m.args.is_empty() && m.method == "to_string" => {
            Some(Ty::Str)
        }
        rs::RustExpr::MethodCall(m)
            if m.args.is_empty() && matches!(m.method.as_str(), "keys" | "values") =>
        {
            match static_type(&m.receiver, env, locals)? {
                Ty::Map(k, _, _) if m.method == "keys" => Some(Ty::List(k)),
                Ty::Map(_, v, _) => Some(Ty::List(v)),
                _ => None,
            }
        }
        rs::RustExpr::MethodCall(m) => match static_type(&m.receiver, env, locals)? {
            Ty::Named(owner) => env
                .fns
                .get(&format!("{owner}::{}", m.method))
                .map(|s| s.result.clone()),
            _ => None,
        },
        rs::RustExpr::Call(c) => match &*c.func {
            rs::RustExpr::Path(p) => {
                let last = &p.segments.last()?.ident;
                if let Some(sig) = env.fns.get(last) {
                    return Some(sig.result.clone());
                }
                match (last.as_str(), c.args.as_slice()) {
                    ("Some", [x]) => Some(Ty::Option(Box::new(static_type(x, env, locals)?))),
                    _ => variant_owner(p, env),
                }
            }
            _ => None,
        },
        // A struct literal is of the struct it names.
        rs::RustExpr::Struct(st) => {
            let name = env.types.resolve(&st.path.segments.last()?.ident);
            env.types
                .structs
                .contains_key(&name)
                .then_some(Ty::Named(name))
        }
        rs::RustExpr::Tuple(items) if items.len() > 1 => items
            .iter()
            .map(|x| static_type(x, env, locals))
            .collect::<Option<Vec<Ty>>>()
            .map(Ty::Tuple),
        rs::RustExpr::Lit(rs::Lit::Str(_)) => Some(Ty::Str),
        rs::RustExpr::Lit(rs::Lit::Bool(_)) => Some(Ty::Bool),
        rs::RustExpr::Macro(m) if m.path.is_ident("format") => Some(Ty::Str),
        rs::RustExpr::Field(f) => match (static_type(&f.base, env, locals)?, &f.member) {
            (Ty::Named(n), rs::Member::Named(m)) => env
                .types
                .structs
                .get(&n)?
                .iter()
                .find(|(field, _)| *field == local_name(m))
                .map(|(_, t)| t.clone()),
            (Ty::Tuple(ts), rs::Member::Unnamed(i)) => ts.get(usize::try_from(*i).ok()?).cloned(),
            _ => None,
        },
        _ => None,
    }
}

/// The enum a variant's path names: `Kind::Variant`, of the enum `Kind`
/// the lift declares.
fn variant_owner(p: &rs::Path, env: &Env) -> Option<Ty> {
    let owner = p.segments.iter().rev().nth(1)?;
    let name = env.types.resolve(&owner.ident);
    (env.types.datas.contains_key(&name) || env.types.enums.contains_key(&name))
        .then_some(Ty::Named(name))
}

/// The type a local takes from where the block returns it: the block's own
/// type when its last statement is the local, or the function's result when
/// a `return` of the local appears.
fn returned_type(ident: &str, rest: &[rs::Stmt], ctx: &Ctx, want: Option<&Ty>) -> Option<Ty> {
    if let (Some(rs::Stmt::Expr(e, false)), Some(w)) = (rest.last(), want) {
        if let Some(t) = tail_type(e, ident, w) {
            return Some(t);
        }
    }
    returns_local(rest, ident).then(|| ctx.result.clone())
}

/// The type of the local `ident` where a tail of type `want` holds it: the
/// tail itself, a member of a tuple, or the value of `Some`, `Ok` or
/// `then_some`.
fn tail_type(e: &rs::RustExpr, ident: &str, want: &Ty) -> Option<Ty> {
    let is_local = |e: &rs::RustExpr| matches!(e, rs::RustExpr::Path(p) if p.is_ident(ident));
    let inner = match want {
        Ty::Option(t) | Ty::Outcome(t, _) => Some((**t).clone()),
        _ => None,
    };
    match (e, want) {
        (rs::RustExpr::Path(p), _) if p.is_ident(ident) => Some(want.clone()),
        (rs::RustExpr::Tuple(items), Ty::Tuple(ts)) if items.len() == ts.len() => items
            .iter()
            .zip(ts)
            .find(|(item, _)| is_local(item))
            .map(|(_, t)| t.clone()),
        (rs::RustExpr::Call(c), _) => match (&*c.func, c.args.as_slice()) {
            (rs::RustExpr::Path(p), [arg])
                if is_local(arg) && (p.is_ident("Some") || p.is_ident("Ok")) =>
            {
                inner
            }
            _ => None,
        },
        (rs::RustExpr::MethodCall(m), _) => match m.args.as_slice() {
            [arg] if m.method == "then_some" && is_local(arg) => inner,
            _ => None,
        },
        _ => None,
    }
}

/// Whether a `return` of the local `ident` appears in the statements,
/// outside closures.
fn returns_local(stmts: &[rs::Stmt], ident: &str) -> bool {
    stmts.iter().any(|stmt| match stmt {
        rs::Stmt::Local(l) => l.init.as_ref().is_some_and(|init| {
            let mut exprs = vec![&*init.expr];
            exprs.extend(init.diverge.as_deref());
            exprs.iter().any(|e| returns_in(e, ident))
        }),
        rs::Stmt::Expr(e, _) => returns_in(e, ident),
        rs::Stmt::Macro(_) | rs::Stmt::Other => false,
    })
}

/// [`returns_local`] in an expression.
fn returns_in(e: &rs::RustExpr, ident: &str) -> bool {
    if matches!(e, rs::RustExpr::Closure(_)) {
        return false;
    }
    if let rs::RustExpr::Return(Some(v)) = e {
        if matches!(&**v, rs::RustExpr::Path(p) if p.is_ident(ident)) {
            return true;
        }
    }
    let (exprs, blocks) = e.parts();
    exprs.iter().any(|x| returns_in(x, ident))
        || blocks.iter().any(|b| returns_local(&b.stmts, ident))
}

/// `let x = e?; rest`: a match that returns the failure and continues with
/// the value.
#[allow(clippy::too_many_arguments)]
fn question(
    e: &rs::RustExpr,
    ident: &str,
    declared: Option<Ty>,
    rest: &[rs::Stmt],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let ((s, st), mut acc) = flat_of(e, env, &ctx.below(), acc, None)?;
    let (inner, early) = match (&st, &ctx.result) {
        (Ty::Option(inner), Ty::Option(r)) => (
            (**inner).clone(),
            (
                "Option.none".to_string(),
                format!("(ctor Option.none [{}])", r.render()),
            ),
        ),
        (Ty::Outcome(inner, err), Ty::Outcome(ok, rerr)) if err.same(rerr) => {
            let (e, after) = ctx.fresh(env, acc, "error");
            acc = after;
            (
                (**inner).clone(),
                (
                    format!("Result.error {e}"),
                    format!(
                        "(ctor Result.error [{}, {}] {e})",
                        ok.render(),
                        rerr.render()
                    ),
                ),
            )
        }
        _ => return Err(Residue::Form("? of another type".to_string())),
    };
    if declared.as_ref().is_some_and(|d| !d.same(&inner)) {
        return Err(Residue::Form("let of another type".to_string()));
    }
    let (scope, name, acc) = ctx.bind(env, acc, ident, inner.clone());
    let ((rest, rt), acc) = block(rest, env, &scope, acc, want)?;
    let ok = if matches!(st, Ty::Option(_)) {
        format!("Option.some {name}")
    } else {
        format!("Result.ok {name}")
    };
    let (failed, _) = ctx.returning((Expr::Flat(early.1), ctx.result.clone()));
    let arms = if matches!(st, Ty::Option(_)) {
        vec![
            Arm {
                pattern: early.0,
                body: failed,
            },
            Arm {
                pattern: ok,
                body: rest,
            },
        ]
    } else {
        vec![
            Arm {
                pattern: ok,
                body: rest,
            },
            Arm {
                pattern: early.0,
                body: failed,
            },
        ]
    };
    Ok(((Expr::Match { scrutinee: s, arms }, rt), acc))
}

/// A candidate function: its LexLean name, signature and body.
pub(crate) struct Candidate<'a> {
    pub(crate) rust: String,
    pub(crate) name: String,
    pub(crate) params: Vec<Param>,
    pub(crate) result: Ty,
    pub(crate) block: &'a rs::Block,
    /// The `use` imports of its file: name -> the path it names.
    pub(crate) imports: BTreeMap<String, String>,
    /// How its file reads the names two of its crate's types share: each
    /// such name -> the LexLean name of the one the file means.
    pub(crate) aliases: BTreeMap<String, String>,
    /// How its file reads the keys two of its crate's functions share
    /// (`toml_string` in two modules): each such key, as a call writes it
    /// -> the module-qualified `rust` key of the one the file means, or
    /// none where the file means neither.
    pub(crate) homonyms: BTreeMap<String, Option<String>>,
    /// The key its own body calls it by: `rust`, or for a function whose
    /// key the crate shares, the key a call writes.
    pub(crate) called: String,
    /// The formatter parameter of a `Display` impl's `fmt`, which lifts as
    /// the function from the value to the text it writes.
    pub(crate) formatter: Option<String>,
}

/// What a lift produced.
pub(crate) struct Lifted {
    pub(crate) decls: Vec<Decl>,
    /// Rust name -> why it did not lift.
    pub(crate) failed: BTreeMap<String, Residue>,
    /// The enums, structs and tuples the lifted definitions name.
    pub(crate) types: BTreeSet<Ty>,
    /// Rust name -> the Lean axioms its definition elaborates with.
    pub(crate) axioms: BTreeMap<String, Vec<String>>,
    /// The lifted functions whose value the kernel cannot compute by
    /// unfolding: they reach a primitive Lean implements opaquely (string
    /// encoding, joining, decimal text), so `decide` cannot close a closed
    /// instance of them.
    pub(crate) opaque: BTreeSet<String>,
    /// The types whose derived order the lifted definitions compare by.
    pub(crate) ordered: BTreeSet<Ty>,
    /// Rust name -> where its definition parts from the Rust function, its
    /// own and its callees'.
    pub(crate) divergences: BTreeMap<String, Vec<String>>,
}

/// The primitives whose Lean definitions the kernel does not unfold.
const KERNEL_OPAQUE: [&str; 6] = [
    "prim split_exact",
    "prim parse_decimal",
    "prim format_decimal",
    "prim compare_bytes",
    "prim utf8_encode",
    "prim join",
];

/// Lift every candidate, to the fixpoint where no lifted body calls a function
/// that failed to lift and no two lifted functions call each other.
pub(crate) fn lift_all(
    candidates: &[Candidate<'_>],
    types: &Types,
    widths: &[(u32, Vec<String>)],
    names: &BTreeSet<String>,
) -> Lifted {
    let mut failed: BTreeMap<String, Residue> = BTreeMap::new();
    loop {
        let fns: BTreeMap<String, Sig> = candidates
            .iter()
            .filter(|c| !failed.contains_key(&c.rust))
            .map(|c| {
                (
                    c.rust.clone(),
                    Sig {
                        name: c.name.clone(),
                        params: c.params.iter().map(|(_, _, t)| t.clone()).collect(),
                        result: c.result.clone(),
                    },
                )
            })
            .collect();
        let mut env = Env {
            types: types.clone(),
            fns: fns.clone(),
            widths: widths.to_vec(),
            names: names.clone(),
        };
        let mut decls = Vec::new();
        let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut mentioned: BTreeMap<String, BTreeSet<Ty>> = BTreeMap::new();
        let mut compares: BTreeMap<String, BTreeSet<Ty>> = BTreeMap::new();
        // Each mutual group of a function and its entwined helpers, by key.
        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut own: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut changed = false;
        for c in candidates {
            if failed.contains_key(&c.rust) {
                continue;
            }
            let names: Vec<String> = c.params.iter().map(|(_, n, _)| n.clone()).collect();
            let ctx = Ctx {
                current: c.called.clone(),
                params: names.clone(),
                result: c.result.clone(),
                locals: c
                    .params
                    .iter()
                    .map(|(rust, n, t)| (rust.clone(), (n.clone(), t.clone())))
                    .collect(),
                tails: BTreeMap::new(),
                elements: None,
                rebuilt: BTreeMap::new(),
                bounds: BTreeMap::new(),
                capped: BTreeMap::new(),
                exits: None,
                own: c.name.clone(),
                top: true,
                looping: None,
                hidden: Vec::new(),
                closures: BTreeMap::new(),
                imports: c.imports.clone(),
                formatter: c.formatter.clone(),
            };
            // Every candidate's file reads each shared name, so one file's
            // reading never outlives its candidates.
            env.types
                .aliases
                .extend(c.aliases.iter().map(|(n, t)| (n.clone(), t.clone())));
            // Each key the crate's functions share reads as the function
            // its file means, or as no function.
            for (key, meant) in &c.homonyms {
                match meant.as_ref().and_then(|m| fns.get(m)) {
                    Some(sig) => env.fns.insert(key.clone(), sig.clone()),
                    None => env.fns.remove(key),
                };
            }
            let lifted = block(&c.block.stmts, &env, &ctx, Acc::default(), Some(&c.result));
            let outcome = match lifted {
                Ok(((body, ty), acc)) if ty.same(&c.result) => {
                    let descends = acc.descends.clone();
                    if descends.len() > 1 {
                        Err(Residue::Form(
                            "recursion on more than one parameter".to_string(),
                        ))
                    } else {
                        Ok((
                            body,
                            descends.into_iter().next().map(|i| names[i].clone()),
                            acc,
                        ))
                    }
                }
                Ok(_) => Err(Residue::Form("a body of another type".to_string())),
                Err(r) => Err(r),
            };
            let outcome = match outcome {
                // Recursion through a helper of the same mutual group is
                // structural on each member's own argument.
                Ok((body, Some(arg), acc)) if acc.helpers.iter().any(|h| h.entwined) => {
                    Ok((body, Some(arg), None, Vec::new(), acc))
                }
                Ok((body, Some(arg), acc)) => {
                    well_founded(c, &body, &arg, types).map(|(recursive, termination, theorems)| {
                        (body, recursive, termination, theorems, acc)
                    })
                }
                Ok((body, None, acc)) => Ok((body, None, None, Vec::new(), acc)),
                Err(r) => Err(r),
            };
            match outcome {
                Ok((body, recursive, termination, theorems, acc)) => {
                    // A well-founded definition elaborates with its evidence.
                    let mut evidence_axioms = BTreeSet::new();
                    for t in theorems {
                        if let Decl::Theorem { axioms, .. } = &t {
                            evidence_axioms.extend(axioms.iter().cloned());
                        }
                        decls.push((format!("#{}", t.name()), t));
                    }
                    let calls = acc
                        .calls
                        .into_iter()
                        .map(|k| match c.homonyms.get(&k) {
                            Some(Some(meant)) => meant.clone(),
                            _ => k,
                        })
                        .collect();
                    graph.insert(c.rust.clone(), calls);
                    let mut seen = acc.seen;
                    for (_, _, t) in &c.params {
                        seen.extend(t.mentions());
                    }
                    seen.extend(c.result.mentions());
                    mentioned.insert(c.rust.clone(), seen);
                    compares.insert(c.rust.clone(), acc.ordered);
                    let mut axioms = acc.axioms;
                    axioms.extend(evidence_axioms);
                    own.insert(c.rust.clone(), axioms);
                    let group = acc
                        .helpers
                        .iter()
                        .any(|h| h.entwined)
                        .then(|| format!("{}Mutual", c.name));
                    if group.is_some() {
                        let mut members = vec![c.rust.clone()];
                        members.extend(
                            acc.helpers
                                .iter()
                                .filter(|h| h.entwined)
                                .map(|h| format!("#{}", h.name)),
                        );
                        groups.push(members);
                    }
                    for h in acc.helpers {
                        let key = format!("#{}", h.name);
                        graph.insert(key.clone(), h.calls);
                        own.insert(key.clone(), h.axioms);
                        let termination = h.termination.map(|(measure, theorems)| {
                            let names: Vec<String> =
                                theorems.iter().map(|t| t.name().to_string()).collect();
                            for t in theorems {
                                decls.push((format!("#{}", t.name()), t));
                            }
                            (measure, names)
                        });
                        decls.push((
                            key,
                            Decl::Definition {
                                recursive: if h.recursive {
                                    h.params.first().map(|(n, _)| n.clone())
                                } else {
                                    None
                                },
                                name: h.name,
                                parameters: h.params,
                                result: h.result,
                                body: h.body,
                                axioms: Vec::new(),
                                termination,
                                mutual: if h.entwined { group.clone() } else { None },
                            },
                        ));
                    }
                    decls.push((
                        c.rust.clone(),
                        Decl::Definition {
                            name: c.name.clone(),
                            parameters: c
                                .params
                                .iter()
                                .map(|(_, n, t)| (n.clone(), t.render()))
                                .collect(),
                            result: c.result.render(),
                            body,
                            axioms: Vec::new(),
                            recursive,
                            termination,
                            mutual: group,
                        },
                    ));
                }
                Err(reason) => {
                    failed.insert(c.rust.clone(), reason);
                    changed = true;
                }
            }
        }
        // A call cycle is a mutual group once its members lift on their
        // own: before that, what stops a member is the member's own cause.
        let mut mutual_of: BTreeMap<String, String> = BTreeMap::new();
        if !changed {
            for component in call_cycles(&graph) {
                match mutual_group(&component, candidates, &mut decls, types) {
                    Ok(label) => {
                        for m in &component {
                            if let Some(axioms) = own.get_mut(m) {
                                axioms.extend(evidence_axioms());
                            }
                            mutual_of.insert(m.clone(), label.clone());
                        }
                        groups.push(component.into_iter().collect());
                    }
                    Err(reason) => {
                        for m in component.iter().filter(|m| !m.starts_with('#')) {
                            failed.insert(m.clone(), reason.clone());
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            let types = mentioned.into_values().flatten().collect();
            // A definition elaborates with its own axioms and its callees'.
            let mut axioms: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (rust, callees) in &graph {
                let mut all = own[rust].clone();
                let mut stack: Vec<&String> = callees.iter().collect();
                let mut seen = BTreeSet::new();
                while let Some(n) = stack.pop() {
                    if seen.insert(n) {
                        all.extend(own.get(n).into_iter().flatten().cloned());
                        stack.extend(graph.get(n).into_iter().flatten());
                    }
                }
                axioms.insert(rust.clone(), all.into_iter().collect());
            }
            // A mutual group elaborates as one: each member with every
            // member's axioms.
            for members in &groups {
                let all: BTreeSet<String> = members
                    .iter()
                    .flat_map(|m| axioms.get(m).cloned().unwrap_or_default())
                    .collect();
                for m in members {
                    axioms.insert(m.clone(), all.iter().cloned().collect());
                }
            }
            // A divergence is the register's, not Lean's: it leaves the
            // axioms, and a definition diverges where a callee does.
            let mut divergences: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (key, all) in &mut axioms {
                let parted: Vec<String> = all
                    .iter()
                    .filter_map(|a| a.strip_prefix(DIVERGES).map(String::from))
                    .collect();
                all.retain(|a| !a.starts_with(DIVERGES));
                if !parted.is_empty() {
                    divergences.insert(key.clone(), parted);
                }
            }
            for (key, d) in &mut decls {
                if let Decl::Definition { axioms: a, .. } = d {
                    a.clone_from(&axioms[key.as_str()]);
                }
            }
            axioms.retain(|key, _| !key.starts_with('#'));
            divergences.retain(|key, _| !key.starts_with('#'));
            let opaque_prelude = prelude_opaque();
            let mut opaque: BTreeSet<String> = decls
                .iter()
                .filter(|(_, d)| match d {
                    // Well-founded recursion does not reduce in the kernel.
                    Decl::Definition {
                        termination: Some(_),
                        ..
                    } => true,
                    Decl::Definition { body, .. } => {
                        let text = body.flat();
                        prelude_calls(&text)
                            .iter()
                            .any(|name| opaque_prelude.contains(name))
                            || KERNEL_OPAQUE.iter().any(|p| text.contains(p))
                    }
                    _ => false,
                })
                .map(|(key, _)| key.clone())
                .collect();
            loop {
                let more: Vec<String> = graph
                    .iter()
                    .filter(|(k, callees)| {
                        !opaque.contains(*k) && callees.iter().any(|c| opaque.contains(c))
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                if more.is_empty() {
                    break;
                }
                opaque.extend(more);
            }
            opaque.retain(|key| !key.starts_with('#'));
            // Each definition after the definitions it calls.
            let mut placed: BTreeSet<String> = BTreeSet::new();
            let mut ordered = Vec::new();
            let mut pending: Vec<(String, Decl)> = decls;
            while !pending.is_empty() {
                let before = ordered.len();
                let mut rest = Vec::new();
                for (key, d) in pending {
                    let ready = graph.get(&key).is_none_or(|callees| {
                        callees.iter().all(|c| {
                            placed.contains(c)
                                || c == &key
                                || !graph.contains_key(c)
                                || mutual_of
                                    .get(c)
                                    .is_some_and(|l| mutual_of.get(&key) == Some(l))
                        })
                    });
                    if ready {
                        placed.insert(key);
                        ordered.push(d);
                    } else {
                        rest.push((key, d));
                    }
                }
                if ordered.len() == before {
                    ordered.extend(rest.into_iter().map(|(_, d)| d));
                    break;
                }
                pending = rest;
            }
            return Lifted {
                decls: grouped(ordered),
                failed,
                types,
                axioms,
                opaque,
                ordered: compares.into_values().flatten().collect(),
                divergences,
            };
        }
    }
}

/// The definitions with each mutual group's members together, where the
/// group's last member stands.
fn grouped(decls: Vec<Decl>) -> Vec<Decl> {
    let label = |d: &Decl| match d {
        Decl::Definition { mutual, .. } => mutual.clone(),
        _ => None,
    };
    let last: BTreeMap<String, usize> = decls
        .iter()
        .enumerate()
        .filter_map(|(i, d)| label(d).map(|l| (l, i)))
        .collect();
    let mut held: BTreeMap<String, Vec<Decl>> = BTreeMap::new();
    let mut out = Vec::new();
    for (i, d) in decls.into_iter().enumerate() {
        match label(&d) {
            Some(l) if last[&l] == i => {
                out.extend(held.remove(&l).unwrap_or_default());
                out.push(d);
            }
            Some(l) => held.entry(l).or_default().push(d),
            None => out.push(d),
        }
    }
    out
}

/// The field types of a constructor of a family's member, as a match
/// enclosing a recursive call binds them: `None` for no such constructor.
fn binder_types(types: &Types, ctor: &str) -> Option<Vec<String>> {
    let (owner, variant) = ctor.rsplit_once('.')?;
    if let Some(vs) = types.datas.get(owner) {
        let (_, fs) = vs.iter().find(|(v, _)| v == variant)?;
        return Some(fs.iter().map(|(_, t)| t.render()).collect());
    }
    if types.boxed(owner) && variant == "mk" {
        return Some(
            types.structs[owner]
                .iter()
                .map(|(_, t)| t.render())
                .collect(),
        );
    }
    types
        .enums
        .get(owner)
        .filter(|vs| vs.iter().any(|v| v == variant))
        .map(|_| Vec::new())
}

/// The definitions a decrease theorem over `family` unfolds: the family's
/// size functions and field accessors its statements name.
fn evidence_definitions(
    types: &Types,
    family: &BTreeSet<String>,
    statements: &[&str],
) -> Vec<String> {
    let mut definitions: Vec<String> = types
        .size_domains(family)
        .iter()
        .filter_map(|d| types.size_name(d, family))
        .collect();
    for m in family {
        if let Some(fields) = types.structs.get(m).filter(|_| types.boxed(m)) {
            definitions.extend(fields.iter().map(|(f, _)| Types::accessor(m, f)));
        }
    }
    let used: BTreeSet<&str> = statements
        .iter()
        .flat_map(|s| s.split(|ch: char| !ch.is_alphanumeric() && ch != '_'))
        .collect();
    definitions.retain(|d| used.contains(d.as_str()));
    definitions.sort();
    definitions
}

/// The proof of a decrease theorem: linear arithmetic after unfolding.
fn evidence_proof(definitions: &[String]) -> String {
    format!(
        "linear_arithmetic{{definitions: [{}]}}",
        definitions.join(", ")
    )
}

/// What a decrease theorem's proof elaborates with: omega's axioms.
fn evidence_axioms() -> Vec<String> {
    vec!["Quot.sound".to_string(), "propext".to_string()]
}

/// The recursion of a candidate descending on `arg`: structural, or, over
/// a nested or mutual family, well-founded on the family's size with one
/// evidence theorem per recursive call.
#[allow(clippy::type_complexity)]
fn well_founded(
    c: &Candidate<'_>,
    body: &Expr,
    arg: &str,
    types: &Types,
) -> Lift<(Option<String>, Option<(String, Vec<String>)>, Vec<Decl>)> {
    let structural = || Ok((Some(arg.to_string()), None, Vec::new()));
    let Some(index) = c.params.iter().position(|(_, n, _)| n == arg) else {
        return structural();
    };
    let Ty::Named(n) = &c.params[index].2 else {
        return structural();
    };
    if !types.nested(n) {
        return structural();
    }
    let family = types.family_of(n).cloned().unwrap_or_default();
    let refused = || {
        Err(Residue::Form(format!(
            "recursion over the nested type {n} that LexLean cannot measure"
        )))
    };
    if !types.sizable(&family) {
        return refused();
    }
    let size = format!("size_of_{n}");
    let binder_types = |ctor: &str| binder_types(types, ctor);
    let params: Vec<(String, String)> = c
        .params
        .iter()
        .map(|(_, n, t)| (n.clone(), t.render()))
        .collect();
    let Some(obligations) =
        crate::wf::obligations(&c.name, &params, index, &size, &body.flat(), &binder_types)
    else {
        return refused();
    };
    let statements: Vec<&str> = obligations.iter().map(|(_, s)| s.as_str()).collect();
    let definitions = evidence_definitions(types, &family, &statements);
    let mut theorems = Vec::new();
    let mut evidence = Vec::new();
    for (i, (parameters, statement)) in obligations.into_iter().enumerate() {
        let name = format!("decreases_{}_{i}", c.name);
        evidence.push(name.clone());
        theorems.push(Decl::Theorem {
            name,
            parameters,
            statement,
            proof: evidence_proof(&definitions),
            axioms: evidence_axioms(),
        });
    }
    Ok((None, Some((format!("({size} {arg})"), evidence)), theorems))
}

/// The call cycles of the graph: each set of functions that call one
/// another, round and round, whole.
fn call_cycles(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<BTreeSet<String>> {
    let reach = |start: &String| -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&String> = graph[start].iter().collect();
        while let Some(n) = stack.pop() {
            if seen.insert(n.clone()) {
                stack.extend(graph.get(n).into_iter().flatten());
            }
        }
        seen
    };
    let on = cyclic(graph);
    let reached: BTreeMap<&String, BTreeSet<String>> = on.iter().map(|n| (n, reach(n))).collect();
    let mut out: Vec<BTreeSet<String>> = Vec::new();
    for n in &on {
        if out.iter().any(|c| c.contains(n)) {
            continue;
        }
        out.push(
            on.iter()
                .filter(|m| reached[n].contains(*m) && reached[*m].contains(n))
                .cloned()
                .collect(),
        );
    }
    out
}

/// A measured parameter's position, size function and family.
type Measured = (usize, String, BTreeSet<String>);

/// The parameter a member of a mutual group is measured on: the first of a
/// nested family's member, or a list or an option of one, with its size
/// function and family.
fn measured(c: &Candidate<'_>, types: &Types) -> Option<Measured> {
    c.params.iter().enumerate().find_map(|(i, (_, _, t))| {
        let n = match t {
            Ty::Named(n) => n,
            Ty::List(inner) | Ty::Option(inner) => match &**inner {
                Ty::Named(n) => n,
                _ => return None,
            },
            _ => return None,
        };
        let family = types
            .family_of(n)
            .filter(|f| types.nested(n) && types.sizable(f))?;
        let size = types.size_name(t, family)?;
        Some((i, size, family.clone()))
    })
}

/// Every text of a lifted expression, rewritten.
fn map_text(e: Expr, f: &dyn Fn(String) -> String) -> Expr {
    match e {
        Expr::Flat(s) => Expr::Flat(f(s)),
        Expr::If {
            condition,
            then,
            otherwise,
        } => Expr::If {
            condition: Box::new(map_text(*condition, f)),
            then: Box::new(map_text(*then, f)),
            otherwise: Box::new(map_text(*otherwise, f)),
        },
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee: f(scrutinee),
            arms: arms
                .into_iter()
                .map(|a| Arm {
                    pattern: a.pattern,
                    body: map_text(a.body, f),
                })
                .collect(),
        },
        Expr::Let {
            name,
            ty,
            value,
            body,
        } => Expr::Let {
            name,
            ty,
            value: Box::new(map_text(*value, f)),
            body: Box::new(map_text(*body, f)),
        },
    }
}

/// A body that reads fields of its measured parameter, a struct of a
/// family, through accessors, as a match on the parameter whose binders
/// are the fields: a call on a binder is on a part, whose decrease the
/// match states.
fn fields_bound(body: Expr, param: &str, ty: &Ty, types: &Types, params: &[String]) -> Expr {
    let Ty::Named(name) = ty else {
        return body;
    };
    let Some(fields) = types.structs.get(name).filter(|_| types.boxed(name)) else {
        return body;
    };
    let reads = |e: &Expr| {
        let text = e.flat();
        fields
            .iter()
            .any(|(f, _)| text.contains(&format!("({} {param})", Types::accessor(name, f))))
    };
    if !reads(&body) {
        return body;
    }
    let text = body.flat();
    let words: BTreeSet<&str> = text
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .chain(params.iter().map(String::as_str))
        .collect();
    let mut binders: Vec<String> = Vec::new();
    for (f, _) in fields {
        let base = local_name(f);
        let taken: Vec<String> = words
            .iter()
            .map(|w| w.to_string())
            .chain(binders.iter().cloned())
            .filter(|w| w.starts_with(&base))
            .collect();
        let free = if taken.contains(&base) {
            first_free(&base, &taken)
        } else {
            base
        };
        binders.push(free);
    }
    let reads: Vec<(String, String)> = fields
        .iter()
        .zip(&binders)
        .map(|((f, _), b)| (format!("({} {param})", Types::accessor(name, f)), b.clone()))
        .collect();
    let replaced = map_text(body, &|s: String| {
        reads.iter().fold(s, |s, (read, b)| s.replace(read, b))
    });
    Expr::Match {
        scrutinee: param.to_string(),
        arms: vec![Arm {
            pattern: format!("{name}.mk {}", binders.join(" ")),
            body: replaced,
        }],
    }
}

/// A call cycle of lifted functions as one LexLean mutual group, each
/// member well-founded on the size of its measured argument, with a theorem
/// that each call of a member decreases the measure. The group's label.
fn mutual_group(
    members: &BTreeSet<String>,
    candidates: &[Candidate<'_>],
    decls: &mut Vec<(String, Decl)>,
    types: &Types,
) -> Lift<String> {
    let form = |why: &str| Err(Residue::Form(format!("mutual recursion {why}")));
    if members.iter().any(|m| m.starts_with('#')) {
        return form("through a helper");
    }
    let mut rows = Vec::new();
    for m in members {
        let Some(c) = candidates.iter().find(|c| &c.rust == m) else {
            return form("of a function outside the lift");
        };
        let Some((at, size, family)) = measured(c, types) else {
            return form("on no argument of a nested family");
        };
        let names: Vec<String> = c.params.iter().map(|(_, n, _)| n.clone()).collect();
        let Some((_, Decl::Definition { body, mutual, .. })) =
            decls.iter_mut().find(|(k, _)| k == m)
        else {
            return form("of a function outside the lift");
        };
        if mutual.is_some() {
            return form("of a function with entwined helpers");
        }
        let (_, param, ty) = &c.params[at];
        *body = fields_bound(body.clone(), param, ty, types, &names);
        let params: Vec<(String, String)> = c
            .params
            .iter()
            .map(|(_, n, t)| (n.clone(), t.render()))
            .collect();
        rows.push((c, params, at, size, family, body.flat()));
    }
    let group: Vec<crate::wf::GroupMember<'_>> = rows
        .iter()
        .map(|(c, params, at, size, _, body)| crate::wf::GroupMember {
            name: &c.name,
            params,
            measured: *at,
            size,
            body,
        })
        .collect();
    let (measures, obligations) =
        crate::wf::group_obligations(&group, &|ctor| binder_types(types, ctor))?;
    let family: BTreeSet<String> = rows.iter().flat_map(|r| r.4.iter().cloned()).collect();
    let label = format!("{}Mutual", rows[0].0.name);
    // The theorems a member's own recursion carried give way to the group's.
    let names: Vec<String> = rows.iter().map(|r| r.0.name.clone()).collect();
    decls.retain(|(key, _)| {
        !names.iter().any(|n| {
            key.strip_prefix(&format!("#decreases_{n}_"))
                .is_some_and(|i| i.chars().all(|ch| ch.is_ascii_digit()))
        })
    });
    // The theorems stand before every member, which cites them.
    let mut at = decls
        .iter()
        .position(|(k, _)| members.contains(k))
        .unwrap_or(decls.len());
    for ((row, measure), owed) in rows.iter().zip(measures).zip(obligations) {
        let statements: Vec<&str> = owed.iter().map(|(_, s)| s.as_str()).collect();
        let definitions = evidence_definitions(types, &family, &statements);
        let mut evidence = Vec::new();
        for (i, (parameters, statement)) in owed.into_iter().enumerate() {
            let name = format!("decreases_{}_{i}", row.0.name);
            evidence.push(name.clone());
            decls.insert(
                at,
                (
                    format!("#{name}"),
                    Decl::Theorem {
                        name,
                        parameters,
                        statement,
                        proof: evidence_proof(&definitions),
                        axioms: evidence_axioms(),
                    },
                ),
            );
            at += 1;
        }
        if let Some((
            _,
            Decl::Definition {
                recursive,
                termination,
                mutual,
                ..
            },
        )) = decls.iter_mut().find(|(k, _)| k == &row.0.rust)
        {
            *recursive = None;
            *termination = Some((measure, evidence));
            *mutual = Some(label.clone());
        }
    }
    Ok(label)
}

/// The functions on a call cycle through another function.
fn cyclic(graph: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for start in graph.keys() {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&String> = graph[start].iter().collect();
        while let Some(n) = stack.pop() {
            if n == start {
                out.insert(start.clone());
                break;
            }
            if seen.insert(n.clone()) {
                if let Some(next) = graph.get(n) {
                    stack.extend(next.iter());
                }
            }
        }
    }
    out
}

/// A literal argument of an acceptance assertion, of the parameter's type.
fn literal(e: &rs::RustExpr, want: &Ty, types: &Types) -> Option<String> {
    match e {
        rs::RustExpr::Lit(lit) => match literal_of(lit, Some(want), false) {
            Ok((Expr::Flat(s), t)) if t.same(want) => Some(s),
            _ => None,
        },
        rs::RustExpr::Unary(u) if matches!(u.op, rs::UnOp::Neg) => match &*u.expr {
            rs::RustExpr::Lit(lit) => match literal_of(lit, Some(want), true) {
                Ok((Expr::Flat(s), t)) if t.same(want) => Some(s),
                _ => None,
            },
            _ => None,
        },
        rs::RustExpr::Path(p) if wrapper(p) == Some("None") => match want {
            Ty::Option(inner) => Some(format!("(ctor Option.none [{}])", inner.render())),
            _ => None,
        },
        rs::RustExpr::Path(p) => {
            let (owner, variant) = ctor(p, types)?;
            (Ty::Named(owner.clone()) == *want).then(|| format!("(ctor {owner}.{variant})"))
        }
        rs::RustExpr::Paren(p) => literal(p, want, types),
        rs::RustExpr::Reference(r) => literal(&r.expr, want, types),
        // `"text".to_string()`: the string.
        rs::RustExpr::MethodCall(m)
            if *want == Ty::Str
                && m.args.is_empty()
                && matches!(m.method.as_str(), "to_string" | "to_owned" | "into") =>
        {
            match &*m.receiver {
                rs::RustExpr::Lit(rs::Lit::Str(_)) => literal(&m.receiver, want, types),
                _ => None,
            }
        }
        // `S { a: lit, .. }`: a structure of literals, every field given.
        rs::RustExpr::Struct(st) if st.rest.is_none() && !st.qself => {
            // The crate declares a structure's name once: its last segment.
            let name = types.resolve(&st.path.segments.last()?.ident);
            let fields = types.structs.get(&name)?;
            if *want != Ty::Named(name.clone()) || st.fields.len() != fields.len() {
                return None;
            }
            let mut parts = Vec::new();
            for (field, ty) in fields {
                let given = st.fields.iter().find(
                    |f| matches!(&f.member, rs::Member::Named(n) if local_name(n) == *field),
                )?;
                parts.push((field, literal(&given.expr, ty, types)?));
            }
            Some(struct_value(&name, &parts, types))
        }
        // `Some(lit)`, and `E::V(lit, ..)` of a data-carrying enum.
        rs::RustExpr::Call(c) => {
            let rs::RustExpr::Path(p) = &*c.func else {
                return None;
            };
            match (wrapper(p), want, c.args.as_slice()) {
                (Some("Some"), Ty::Option(inner), [one]) => Some(format!(
                    "(ctor Option.some [{}] {})",
                    inner.render(),
                    literal(one, inner, types)?
                )),
                _ => {
                    let (owner, variant, fields) = data_variant(p, types)?;
                    if Ty::Named(owner.clone()) != *want || fields.len() != c.args.len() {
                        return None;
                    }
                    let mut parts = vec![format!("ctor {owner}.{variant}")];
                    for (arg, (_, ty)) in c.args.iter().zip(fields) {
                        parts.push(literal(arg, ty, types)?);
                    }
                    Some(format!("({})", parts.join(" ")))
                }
            }
        }
        // `[a, b]`: a list of literals.
        rs::RustExpr::Array(items) => {
            let Ty::List(elem) = want else {
                return None;
            };
            let mut list = format!("(nil {})", elem.render());
            for item in items.iter().rev() {
                list = format!("(cons {} {list})", literal(item, elem, types)?);
            }
            Some(list)
        }
        _ => None,
    }
}

/// `f(lit, ..)` against a lifted function: the applied term, the result
/// type and the function's Rust name.
fn applied(
    e: &rs::RustExpr,
    lifted: &BTreeMap<String, Sig>,
    types: &Types,
) -> Option<(String, Ty, String)> {
    let rs::RustExpr::Call(c) = e else {
        return None;
    };
    let rs::RustExpr::Path(p) = &*c.func else {
        return None;
    };
    let name = p.segments.last()?.ident.to_string();
    let sig = lifted.get(&name)?;
    if sig.params.len() != c.args.len() {
        return None;
    }
    let mut parts = vec![sig.name.clone()];
    for (arg, ty) in c.args.iter().zip(&sig.params) {
        parts.push(literal(arg, ty, types)?);
    }
    Some((format!("({})", parts.join(" ")), sig.result.clone(), name))
}

/// The acceptance statement an assertion makes about a lifted function, and
/// that function's Rust name, when it is a closed instance: `assert!(f(1, 2))`,
/// `assert!(!f(..))`, `assert_eq!(f(..), 3)`.
pub(crate) fn acceptance(
    kind: &str,
    args: &[rs::RustExpr],
    lifted: &BTreeMap<String, Sig>,
    types: &Types,
) -> Option<(String, String)> {
    match (kind, args) {
        ("assert", [single, ..]) => {
            let (inner, value) = match single {
                rs::RustExpr::Unary(rs::ExprUnary {
                    op: rs::UnOp::Not,
                    expr,
                    ..
                }) => (&**expr, "false"),
                other => (other, "true"),
            };
            let (call, result, name) = applied(inner, lifted, types)?;
            (result == Ty::Bool).then(|| (format!("({call} = {value})"), name))
        }
        ("assert_eq", [left, right, ..]) => {
            let (call, result, name) = applied(left, lifted, types)?;
            let decidable = |t: &Ty| matches!(t, Ty::Bool | Ty::Nat(_) | Ty::Str);
            let comparable = match &result {
                Ty::Option(inner) => decidable(inner),
                t => decidable(t),
            };
            if !comparable {
                return None;
            }
            let value = literal(right, &result, types)?;
            Some((format!("({call} = {value})"), name))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lift(src: &str, types: &Types) -> Result<String, Residue> {
        let f: syn::ItemFn = syn::parse_str(src).expect("parses");
        let sig = crate::convert::signature(&f.sig);
        let block = crate::convert::block(&f.block);
        let (params, result) = types.signature(&sig, None)?;
        let c = Candidate {
            rust: sig.ident.clone(),
            name: camel(&[&sig.ident]),
            params,
            result,
            block: &block,
            imports: BTreeMap::from([("Sha256".to_string(), "sha2::Sha256".to_string())]),
            aliases: BTreeMap::new(),
            homonyms: BTreeMap::new(),
            called: sig.ident.clone(),
            formatter: None,
        };
        let out = lift_all(&[c], types, &pointer_widths(&[]), &BTreeSet::new());
        match out.failed.into_values().next() {
            Some(r) => Err(r),
            None => Ok(out
                .decls
                .iter()
                .map(crate::lex::render_decl)
                .collect::<Vec<_>>()
                .join("\n")),
        }
    }

    #[test]
    fn usize_widths_come_from_the_declared_targets() {
        let declared = [
            "x86_64-unknown-linux-gnu".to_string(),
            "wasm32-unknown-unknown".to_string(),
        ];
        let widths = pointer_widths(&declared);
        assert_eq!(widths.len(), 2);
        assert_eq!(widths[0].0, 32);
        assert_eq!(widths[0].1, vec!["wasm32-unknown-unknown".to_string()]);
        assert_eq!(widths[1].0, 64);
        let open: Vec<u32> = pointer_widths(&[]).iter().map(|(w, _)| *w).collect();
        assert_eq!(open, vec![16, 32, 64]);
        assert_eq!(pointer_width("avr-unknown-gnu-atmega328"), Some(16));
    }

    fn plain() -> Types {
        Types::default()
    }

    /// A function of `n` statement `if`s in a row, each updating the locals
    /// the next reads, and its statement count.
    fn ifs_in_a_row(n: usize) -> (String, usize) {
        let mut body = String::from("let mut a = x; let mut b = false; ");
        for k in 0..n {
            body.push_str(&format!(
                "if a < {k} {{ a = {k}; b = !b; }} else {{ b = a == {k}; }} "
            ));
        }
        body.push_str("(a, b)");
        (
            format!("fn steps(x: u32) -> (u32, bool) {{ {body} }}"),
            n + 3,
        )
    }

    #[test]
    fn statement_ifs_lift_the_statements_after_them_once() {
        // The whole function with one `if` bounds what each statement of a
        // longer one costs, where copying the statements after each `if`
        // into both its branches doubles the output per `if`.
        let (one, one_stmts) = ifs_in_a_row(1);
        let one_len = lift(&one, &plain()).expect("lifts").len();
        // Up to as many `if`s as one statement costs bytes: past the point
        // where doubling per `if` would outgrow any linear bound.
        let mut n = 1;
        while n <= one_len / one_stmts {
            let (src, stmts) = ifs_in_a_row(n);
            let out = lift(&src, &plain()).expect("lifts");
            assert!(
                out.len() <= one_len * stmts,
                "{n} ifs: {} bytes for {stmts} statements",
                out.len()
            );
            n *= 2;
        }
    }

    #[test]
    fn a_percent_sign_is_residue() {
        let out = lift(r#"fn rem() -> String { "%=".to_string() }"#, &plain());
        assert_eq!(out, Err(Residue::Form(PERCENT.to_string())));
    }

    #[test]
    fn comparisons_and_logic_lift() {
        let out = lift(
            "fn within(c: u32, lo: u32, hi: u32) -> bool { lo <= c && c <= hi }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("((lo <=? c) && (c <=? hi))"), "{out}");
    }

    #[test]
    fn arithmetic_without_a_named_overflow_is_residue() {
        assert_eq!(
            lift("fn add(a: u32, b: u32) -> u32 { a + b }", &plain()),
            Err(Residue::Arithmetic("+".to_string()))
        );
        assert_eq!(
            lift(
                "fn add(a: usize, b: usize) -> Option<usize> { a.checked_add(b) }",
                &plain()
            ),
            Err(Residue::Width(".checked_add on usize".to_string()))
        );
    }

    #[test]
    fn named_overflow_lifts() {
        let out = lift(
            "fn add(a: u8, b: u8) -> Option<u8> { a.checked_add(b) }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("(if ((a + b) <? 256) then (ctor Option.some [Nat] (a + b)) else (ctor Option.none [Nat]))"), "{out}");
        let out =
            lift("fn add(a: u8, b: u8) -> u8 { a.wrapping_add(b) }", &plain()).expect("lifts");
        assert!(
            out.contains("(prim remainder (a + b) 256 0 : Nat)"),
            "{out}"
        );
        let out = lift("fn half(a: u32) -> u32 { a / 2 }", &plain()).expect("lifts");
        assert!(out.contains("(prim quotient a 2 0 : Nat)"), "{out}");
    }

    #[test]
    fn signed_strings_options_and_lets_lift() {
        let out = lift("fn neg(a: i64) -> bool { a < 0 }", &plain()).expect("lifts");
        assert!(
            out.contains("(prim less_than a (0 : Int64) : Bool)"),
            "{out}"
        );
        let out = lift("fn yes(s: &str) -> bool { s == \"yes\" }", &plain()).expect("lifts");
        assert!(out.contains("(prim equal s \"yes\" : Bool)"), "{out}");
        let out = lift(
            "fn f(o: Option<u32>) -> Option<u32> { let v = o?; let w = v / 2; Some(w) }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("| Option.some v => (let w : Nat := (prim quotient v 2 0 : Nat); (ctor Option.some [Nat] w)))"), "{out}");
    }

    #[test]
    fn slice_recursion_is_structural() {
        let out = lift(
            "fn count(xs: &[bool]) -> u32 { match xs { [] => 0, [x, rest @ ..] => if *x { count(rest).saturating_add(1) } else { count(rest) } } }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("recursive_argument: xs"), "{out}");
        assert_eq!(
            lift("fn spin(n: u32) -> u32 { spin(n) }", &plain()),
            Err(Residue::Form(
                "recursion that does not descend on a slice".to_string()
            ))
        );
    }

    #[test]
    fn literal_matches_and_string_formats_lift() {
        let out = lift(
            "fn name(n: u8) -> &'static str { match n { 0 | 1 => \"low\", _ => \"high\" } }",
            &plain(),
        )
        .expect("lifts");
        assert!(
            out.contains("(if ((n =? 0) || (n =? 1)) then \"low\" else \"high\")"),
            "{out}"
        );
        let out = lift(
            "fn q(m: &str, n: &str) -> String { format!(\"{m}::{}\", n) }",
            &plain(),
        )
        .expect("lifts");
        assert!(
            out.contains("(prim join (cons m (cons \"::\" (cons n (nil String)))) \"\" : String)"),
            "{out}"
        );
    }

    #[test]
    fn a_match_on_a_state_enum_lifts_exhaustively() {
        let mut types = plain();
        types
            .enums
            .insert("Mode".into(), vec!["Idle".into(), "On".into()]);
        let out = lift(
            "fn flip(m: Mode) -> Mode { match m { Mode::Idle => Mode::On, _ => Mode::Idle } }",
            &types,
        )
        .expect("lifts");
        assert!(
            out.contains("(match m | Mode.Idle => (ctor Mode.On) | Mode.On => (ctor Mode.Idle))"),
            "{out}"
        );
    }

    #[test]
    fn structs_and_tuples_lift() {
        let mut raw = BTreeMap::new();
        let ty = crate::convert::ty(&syn::parse_str("u32").expect("type"));
        raw.insert(
            "Band".to_string(),
            vec![("low".to_string(), ty.clone()), ("high".to_string(), ty)],
        );
        let types = Types::new(BTreeMap::new(), &raw, BTreeMap::new());
        let out = lift(
            "fn widen(b: Band) -> (u32, bool) { (b.high, b.low <= b.high) }",
            &types,
        )
        .expect("lifts");
        assert!(
            out.contains("(record TupleNatBool {item0 := (project b high), item1 := ((project b low) <=? (project b high))})"),
            "{out}"
        );
    }

    #[test]
    fn iterator_chains_lift_to_structural_helpers() {
        let out = lift(
            "fn over(xs: &[u32], floor: u32) -> bool { xs.iter().filter(|x| **x < floor).any(|&x| x == 0) }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("name: overFilter1,"), "{out}");
        assert!(out.contains("recursive_argument: items"), "{out}");
        assert!(out.contains("(overAny"), "{out}");
        let folded = lift(
            "fn total(xs: &[u32]) -> Option<u32> { xs.iter().fold(Some(0), |acc, &x| match acc { Some(a) => a.checked_add(x), None => None }) }",
            &plain(),
        )
        .expect("a fold step with a match lifts through a helper");
        assert!(folded.contains("name: totalPart1,"), "{folded}");
        let sum = lift("fn sum(xs: &[u32]) -> u32 { xs.iter().sum() }", &plain());
        assert!(matches!(sum, Err(Residue::Form(_))), "{sum:?}");
    }

    #[test]
    fn early_returns_and_tuple_lets_lift() {
        let out = lift(
            "fn pick(p: (u32, u32), c: bool) -> u32 { let (a, b) = p; if c { return a; } b }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("(let a : Nat := (project pair"), "{out}");
        assert!(out.contains("(if c then a else b)"), "{out}");
        let nested = lift(
            "fn nested(c: bool) -> u32 { let x = { if c { return 1; } 2 }; x }",
            &plain(),
        );
        assert!(
            nested.is_err(),
            "a return below the function's own statements: {nested:?}"
        );
    }

    #[test]
    fn maps_lift_only_where_their_order_is_defined() {
        let ordered = lift(
            "fn keys(m: &BTreeMap<String, u32>) -> Vec<String> { m.keys().cloned().collect() }",
            &plain(),
        )
        .expect("lifts");
        assert!(ordered.contains("(List TupleStringNat)"), "{ordered}");
        let hashed = lift(
            "fn keys(m: &HashMap<String, u32>) -> Vec<String> { m.keys().cloned().collect() }",
            &plain(),
        );
        assert!(matches!(hashed, Err(Residue::Form(_))), "{hashed:?}");
        let lookup = lift(
            "fn has(m: &HashMap<String, u32>, k: &str) -> bool { m.contains_key(k) }",
            &plain(),
        )
        .expect("lifts");
        assert!(lookup.contains("(prim equal (project item"), "{lookup}");
    }

    #[test]
    fn string_methods_lift_to_the_prelude() {
        let out = lift(
            "fn t(text: &str) -> Option<u32> { text.trim().parse::<u32>().ok() }",
            &plain(),
        )
        .expect("lifts");
        assert!(
            out.contains("(Prelude::textParseNat (Prelude::textTrim text) 4294967295)"),
            "{out}"
        );
        let out = lift(
            "fn t(text: &str) -> Option<(String, String)> { let (a, b) = text.split_once('=')?; Some((a.to_string(), b.to_string())) }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("(Prelude::textFirst text \"=\")"), "{out}");
        // `to_lowercase` lifts to the ASCII lowering, and its row records
        // the divergence rather than an axiom.
        let out = lift(
            "fn t(text: &str) -> String { text.to_lowercase() }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("(Prelude::textAsciiLower text)"), "{out}");
        assert!(!out.contains(DIVERGES), "{out}");
    }

    #[test]
    fn character_matches_lift_to_tests() {
        let out = lift(
            "fn k(c: char) -> u8 { match c { '-' => 1, '0'..='9' => 2, c if c.is_whitespace() => 3, _ => 4 } }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("(prim equal c \"-\" : Bool)"), "{out}");
        assert!(out.contains("(Prelude::textIsWhitespace c)"), "{out}");
    }

    #[test]
    fn loops_continue_break_and_enumerate() {
        let out = lift(
            "fn f(xs: &[u32]) -> u32 { let mut n: u32 = 0; for (i, x) in xs.iter().enumerate() { if *x == 0 { continue; } if i > 3 { break; } n = n.saturating_add(1); } n }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("fEnumerate"), "{out}");
        assert!(out.contains("fLoop"), "{out}");
    }

    #[test]
    fn a_question_mark_in_an_operand_hoists() {
        let out = lift(
            "fn f(o: Option<u32>) -> Option<u32> { Some(o?.min(3)) }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("Option.some tried"), "{out}");
    }

    #[test]
    fn an_empty_vector_takes_the_type_the_block_returns() {
        let out = lift(
            "fn f(xs: &[u32]) -> Vec<u32> { let mut out = Vec::new(); for x in xs { out.push(*x); } out }",
            &plain(),
        )
        .expect("lifts");
        assert!(out.contains("(nil Nat)"), "{out}");
        let early = lift(
            "fn g(n: u32) -> Vec<u32> { let out = Vec::with_capacity(4); if n > 2 { return out; } out }",
            &plain(),
        )
        .expect("lifts");
        assert!(early.contains("(nil Nat)"), "{early}");
    }

    #[test]
    fn an_index_is_the_element_or_the_default() {
        let out = lift("fn f(xs: &[u32]) -> u32 { xs[2] }", &plain()).expect("lifts");
        assert!(out.contains("(prim index xs 2 : (Option Nat))"), "{out}");
        assert!(out.contains("Option.none => 0"), "{out}");
    }
}
