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
use std::ops::ControlFlow::{self, Break, Continue};

/// A lift step: the value, or the residue that stops the lift.
pub(crate) type Lift<T> = ControlFlow<Residue, T>;

use crate::ast as rs;
use crate::lex::{camel, Arm, Decl, Expr};

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
    /// Integer arithmetic whose meaning depends on the width of the type
    /// (`usize`, or a signed type narrower than `i64`).
    Width(String),
    /// An expression form outside the lift.
    Form(String),
    /// A call to a function that is not lifted.
    Call(String),
    /// Another function of the crate lifts to the same LexLean name.
    Collision(String),
}

impl Residue {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Signature(_) => "signature",
            Self::Type(_) => "type",
            Self::Statements => "statements",
            Self::Arithmetic(_) => "arithmetic",
            Self::Width(_) => "width",
            Self::Form(_) => "form",
            Self::Call(_) => "call",
            Self::Collision(_) => "collision",
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
            Self::Width(op) => {
                format!("{op} depends on an integer width LexLean does not fix")
            }
            Self::Form(f) => format!("expression form outside the lift: {f}"),
            Self::Call(c) => format!("calls {c}, which is not lifted"),
            Self::Collision(n) => format!("another function of the crate is also named {n}"),
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
    Str,
    /// A `char`, held as the one-character string it is.
    Char,
    /// The error of an integer `parse`: the prelude's `ParseIntError`.
    ParseError,
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
}

impl Ty {
    /// The LexLean type.
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Unit => "Unit".into(),
            Self::Bool => "Bool".into(),
            Self::Nat(_) => "Nat".into(),
            Self::Int(_) => "Int64".into(),
            Self::Str | Self::Char => "String".into(),
            Self::ParseError => format!("{PRELUDE}::ParseIntError"),
            Self::Named(n) => n.clone(),
            Self::Option(t) => format!("(Option {})", t.render()),
            Self::Outcome(t, e) => format!("(Result {} {})", t.render(), e.render()),
            Self::List(t) | Self::Set(t, _) => format!("(List {})", t.render()),
            Self::Tuple(ts) => tuple_name(ts),
            Self::Pair(ts) => format!("({} * {})", ts[0].render(), ts[1].render()),
            Self::Map(..) => format!("(List {})", self.entry().render()),
        }
    }

    /// A map's entry: the tuple of its key and value.
    fn entry(&self) -> Ty {
        match self {
            Self::Map(k, v, _) => Ty::Tuple(vec![(**k).clone(), (**v).clone()]),
            other => other.clone(),
        }
    }

    fn tag(&self) -> String {
        match self {
            Self::Unit => "Unit".into(),
            Self::Bool => "Bool".into(),
            Self::Nat(_) => "Nat".into(),
            Self::Int(_) => "Int".into(),
            Self::Str | Self::Char => "String".into(),
            Self::ParseError => "ParseIntError".into(),
            Self::Named(n) => n.clone(),
            Self::Option(t) => format!("Option{}", t.tag()),
            Self::Outcome(t, e) => format!("Result{}{}", t.tag(), e.tag()),
            Self::List(t) | Self::Set(t, _) => format!("List{}", t.tag()),
            Self::Tuple(ts) => tuple_name(ts),
            Self::Pair(ts) => format!("Pair{}{}", ts[0].tag(), ts[1].tag()),
            Self::Map(..) => format!("List{}", self.entry().tag()),
        }
    }

    /// The same LexLean type.
    fn same(&self, other: &Self) -> bool {
        self.render() == other.render()
    }

    /// Every named type and tuple this type mentions, itself included.
    pub(crate) fn mentions(&self, out: &mut BTreeSet<Ty>) {
        match self {
            Self::Named(_) => {
                out.insert(self.clone());
            }
            Self::Tuple(ts) => {
                out.insert(self.clone());
                for t in ts {
                    t.mentions(out);
                }
            }
            Self::Pair(ts) => {
                for t in ts {
                    t.mentions(out);
                }
            }
            Self::Option(t) | Self::List(t) | Self::Set(t, _) => t.mentions(out),
            Self::Map(..) => self.entry().mentions(out),
            Self::Outcome(t, e) => {
                t.mentions(out);
                e.mentions(out);
            }
            _ => {}
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

/// The structure a tuple type lifts to: `TupleNatBool` for `(u32, bool)`.
pub(crate) fn tuple_name(ts: &[Ty]) -> String {
    let tags: String = ts.iter().map(Ty::tag).collect();
    format!("Tuple{tags}")
}

/// A struct value from its fields in declaration order: a record, or the
/// constructor `mk` of a struct of a recursive family.
fn struct_value(name: &str, fields: &[(&String, String)], types: &Types) -> String {
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
            let mut inner = BTreeSet::new();
            ty.mentions(&mut inner);
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
            let mut inner = BTreeSet::new();
            ty.mentions(&mut inner);
            !inner.contains(&Ty::Named(me.to_string()))
        }
        _ => true,
    }
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
                            Continue(ty) if nested_ok(&ty, name) => tys.push((field.clone(), ty)),
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
                        Continue(ty) => fields.push((local_name(field), ty)),
                        Break(_) => ok = false,
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
                        t.mentions(&mut out);
                    }
                }
                graph.insert(name.clone(), named_in(&out));
            }
            for (name, fields) in &fields_of {
                let mut out = BTreeSet::new();
                for (_, t) in fields {
                    t.mentions(&mut out);
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
            };
            let mut lifted = BTreeMap::new();
            for name in &names {
                let mut fields = Vec::new();
                let mut ok = !structs[name].is_empty();
                for (field, t) in &structs[name] {
                    match probe.ty(t) {
                        Continue(ty) => fields.push((local_name(field), ty)),
                        Break(_) => ok = false,
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
            t.mentions(&mut inner);
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
                        let mut inner = BTreeSet::new();
                        t.mentions(&mut inner);
                        !named_in(&inner).is_disjoint(family)
                    }
                })
            })
    }

    /// The size function of a type of a nested family: the members, and
    /// lists and options of them. `None` for any other type.
    pub(crate) fn size_name(&self, ty: &Ty, family: &BTreeSet<String>) -> Option<String> {
        match ty {
            Ty::Named(n) if family.contains(n) => Some(format!("size_of_{n}")),
            Ty::List(t) => match &**t {
                Ty::Named(n) if family.contains(n) => Some(format!("size_of_list_{n}")),
                _ => None,
            },
            Ty::Option(t) => match &**t {
                Ty::Named(n) if family.contains(n) => Some(format!("size_of_option_{n}")),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether a nested family has a structural size: every occurrence of a
    /// member is direct, or the whole element of a list or an option.
    pub(crate) fn sizable(&self, family: &BTreeSet<String>) -> bool {
        family.iter().all(|m| {
            self.member_fields(m).iter().all(|t| {
                let mut inner = BTreeSet::new();
                t.mentions(&mut inner);
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
        let bad = || Break(Residue::Type(t.text.clone()));
        match &t.kind {
            rs::TypeKind::Reference {
                mutable: false,
                elem,
            } => match &elem.kind {
                rs::TypeKind::Path { qself: false, path } if path.is_ident("str") => {
                    Continue(Ty::Str)
                }
                rs::TypeKind::Slice(s) => Continue(Ty::List(Box::new(self.ty(s)?))),
                _ => self.ty(elem),
            },
            rs::TypeKind::Paren(p) => self.ty(p),
            rs::TypeKind::Tuple(elems) if elems.is_empty() => Continue(Ty::Unit),
            rs::TypeKind::Tuple(elems) if elems.len() >= 2 => {
                let mut ts = Vec::new();
                for e in elems {
                    ts.push(self.ty(e)?);
                }
                Continue(self.tuple(ts))
            }
            rs::TypeKind::Path { qself: false, path } if path.segments.len() == 1 => {
                let seg = &path.segments[0];
                let name = seg.ident.clone();
                let mut args = Vec::new();
                match &seg.arguments {
                    rs::PathArguments::None => {}
                    rs::PathArguments::AngleBracketed(a) => {
                        for arg in a {
                            let rs::GenericArgument::Type(t) = arg else {
                                return bad();
                            };
                            args.push(self.ty(t)?);
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
                    _ => return bad(),
                };
                Continue(ty)
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
            return Break(Residue::Signature(
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
                    _ => return Break(Residue::Signature("takes self".to_string())),
                },
                rs::FnArg::Typed(pat) => {
                    let rs::Pat::Ident(name) = &*pat.pat else {
                        return Break(Residue::Signature("pattern parameter".to_string()));
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
        Continue((params, result))
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
                Continue(Ty::Named(o.to_string()))
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
}

/// What lifting a body gathers as it goes. Each step takes it and gives
/// back the state after it, so the steps of a body are a fold over it.
#[derive(Clone, Debug, Default)]
struct Acc {
    /// The parameter positions recursive calls descend on.
    descends: BTreeSet<usize>,
    /// The other lifted functions the body calls.
    calls: BTreeSet<String>,
    /// How many fresh binders the body has taken.
    fresh: usize,
    /// Every type an expression of the body has.
    seen: BTreeSet<Ty>,
    /// The Lean axioms the body's primitives elaborate with.
    axioms: BTreeSet<String>,
    /// The list helpers the body's iterator chains lift to.
    helpers: Vec<Helper>,
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
        ty.mentions(&mut seen);
        Acc { seen, ..self }
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
}

impl Ctx {
    /// A fresh name for a helper of the function being lifted.
    fn helper_name(&self, env: &Env, acc: &Acc, kind: &str) -> String {
        let mut n = acc.helpers.len() + 1;
        loop {
            let candidate = format!("{}{kind}{n}", self.own);
            if !acc.helpers.iter().any(|h| h.name == candidate) && !self.taken(env, &candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    fn taken(&self, env: &Env, name: &str) -> bool {
        self.locals.values().any(|(n, _)| n == name)
            || self.hidden.iter().any(|n| n == name)
            || env.fns.values().any(|f| f.name == name)
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

    /// A binder no name in scope takes: `base` and a number.
    fn fresh(&self, env: &Env, acc: Acc, base: &str) -> (String, Acc) {
        let mut n = acc.fresh;
        loop {
            n += 1;
            let name = format!("{base}{n}");
            if !self.taken(env, &name) {
                return (name, Acc { fresh: n, ..acc });
            }
        }
    }

    /// The same scope, below the function's own statements: a `return`
    /// there is not an early exit the lift can follow.
    fn below(&self) -> Ctx {
        let mut next = self.clone();
        next.top = false;
        next
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
        Expr::Flat(s) => Continue(s),
        Expr::If { .. } if !e.flat().contains('\n') && !e.flat().contains("(match") => {
            Continue(e.flat())
        }
        _ => Break(Residue::Form("match or let inside an operand".to_string())),
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
    if let Continue(text) = flat_text(x.clone()) {
        let acc = acc.called_all(own.calls).axioms_all(own.axioms);
        let mut descends = acc.descends;
        descends.extend(own.descends);
        return Continue(((text, t), Acc { descends, ..acc }));
    }
    if !own.descends.is_empty() {
        return Break(Residue::Form(
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
    });
    let acc = acc.called(format!("#{name}"));
    Continue(((format!("({})", parts.join(" ")), t), acc))
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
fn exact_string(text: &str) -> Option<String> {
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
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    match lit {
        rs::Lit::Bool(b) if !negative => flat(b.to_string(), Ty::Bool),
        rs::Lit::Int { value, suffix } => {
            let Some(v) = *value else {
                return Break(Residue::Form("integer literal".to_string()));
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
                _ => return Break(Residue::Form("integer literal".to_string())),
            };
            match ty {
                Ty::Int(_) if v <= i64::MAX as u128 + u128::from(negative) => {
                    let sign = if negative { "-" } else { "" };
                    flat(format!("({sign}{v} : Int64)"), ty)
                }
                Ty::Nat(_) if !negative => flat(v.to_string(), ty),
                _ => Break(Residue::Form("integer literal".to_string())),
            }
        }
        rs::Lit::Str(s) if !negative => match exact_string(s) {
            Some(text) => flat(text, Ty::Str),
            None => Break(Residue::Form(PERCENT.to_string())),
        },
        rs::Lit::Char(c) if !negative => match exact_string(&c.to_string()) {
            Some(text) => flat(text, Ty::Char),
            None => Break(Residue::Form(PERCENT.to_string())),
        },
        rs::Lit::Byte(b) if !negative => flat(b.to_string(), Ty::Nat(Some(8))),
        rs::Lit::ByteStr(b) if !negative => {
            let mut list = "(nil Nat)".to_string();
            for byte in b.iter().rev() {
                list = format!("(cons {byte} {list})");
            }
            flat(list, Ty::List(Box::new(Ty::Nat(Some(8)))))
        }
        _ => Break(Residue::Form("literal".to_string())),
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
    Continue(((x, t), acc))
}

fn expr_inner(
    e: &rs::RustExpr,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let flat = |s: String, t: Ty, acc: Acc| Continue(((Expr::Flat(s), t), acc));
    match e {
        rs::RustExpr::Lit(lit) => literal_of(lit, want, false).map_continue(|v| (v, acc)),
        rs::RustExpr::Paren(p) => expr(p, env, ctx, acc, want),
        rs::RustExpr::Block(b) if !b.label => block(&b.block.stmts, env, ctx, acc, want),
        rs::RustExpr::Reference(r) if !r.mutable => expr(&r.expr, env, ctx, acc, want),
        rs::RustExpr::Path(p) => {
            if let Some(ident) = p.get_ident() {
                if let Some((name, ty)) = ctx.locals.get(&ident.to_string()) {
                    return flat(name.clone(), ty.clone(), acc);
                }
            }
            // A constant: a definition of no parameters.
            if let Some(ident) = p.get_ident() {
                let key = ident.to_string();
                if let Some(sig) = env.fns.get(&key).filter(|s| s.params.is_empty()) {
                    if key == ctx.current {
                        return Break(Residue::Form("a constant that names itself".to_string()));
                    }
                    let acc = acc.called(key);
                    return flat(format!("({})", sig.name), sig.result.clone(), acc);
                }
            }
            if wrapper(p) == Some("None") {
                return match want {
                    Some(t @ Ty::Option(inner)) => flat(
                        format!("(ctor Option.none [{}])", inner.render()),
                        t.clone(),
                        acc,
                    ),
                    _ => Break(Residue::Form("None of an unknown type".to_string())),
                };
            }
            match ctor(p, &env.types) {
                Some((owner, variant)) => {
                    flat(format!("(ctor {owner}.{variant})"), Ty::Named(owner), acc)
                }
                None => Break(Residue::Form(format!("free name {}", p.text))),
            }
        }
        rs::RustExpr::Unary(u) => match u.op {
            rs::UnOp::Not => {
                let ((x, t), acc) = flat_of(&u.expr, env, ctx, acc, Some(&Ty::Bool))?;
                if t != Ty::Bool {
                    return Break(Residue::Form("bitwise not".to_string()));
                }
                flat(format!("(not {x})"), Ty::Bool, acc)
            }
            rs::UnOp::Neg => match &*u.expr {
                rs::RustExpr::Lit(lit) => literal_of(lit, want, true).map_continue(|v| (v, acc)),
                _ => Break(Residue::Arithmetic("-".to_string())),
            },
            rs::UnOp::Deref => expr(&u.expr, env, ctx, acc, want),
        },
        rs::RustExpr::Binary(b) => binary(b, env, ctx, acc),
        rs::RustExpr::If(i) => {
            let Some(otherwise) = &i.else_branch else {
                return Break(Residue::Form("if without else".to_string()));
            };
            if let rs::RustExpr::Let(l) = &*i.cond {
                let wild = rs::Pat::Wild;
                let arms = vec![
                    (&l.pat, Body::Block(&i.then_branch)),
                    (&wild, Body::Expr(otherwise)),
                ];
                return matching(&l.expr, &arms, env, ctx, acc, want);
            }
            let ((c, ct), acc) = expr(&i.cond, env, ctx, acc, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Break(Residue::Form("non-boolean condition".to_string()));
            }
            let ((then, tt), acc) = block(&i.then_branch.stmts, env, ctx, acc, want)?;
            let ((other, ot), acc) = expr(otherwise, env, ctx, acc, Some(want.unwrap_or(&tt)))?;
            if !tt.same(&ot) {
                return Break(Residue::Form("branches of different types".to_string()));
            }
            Continue((
                (
                    Expr::If {
                        condition: Box::new(Expr::Flat(flat_text(c)?)),
                        then: Box::new(then),
                        otherwise: Box::new(other),
                    },
                    tt,
                ),
                acc,
            ))
        }
        rs::RustExpr::Match(m) => {
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
        rs::RustExpr::Call(c) => call(c, env, ctx, acc, want),
        rs::RustExpr::MethodCall(m) => method(m, env, ctx, acc, want),
        rs::RustExpr::Field(f) => {
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
                        return Break(Residue::Form("field of an unknown struct".to_string()));
                    };
                    if env.types.boxed(s) {
                        flat(
                            format!("({} {base})", Types::accessor(s, &name)),
                            ty.clone(),
                            acc,
                        )
                    } else {
                        flat(format!("(project {base} {name})"), ty.clone(), acc)
                    }
                }
                (rs::Member::Unnamed(i), Ty::Tuple(ts) | Ty::Pair(ts)) => {
                    match ts.get(*i as usize) {
                        Some(ty) => flat(tuple_item(&bt, &base, *i as usize), ty.clone(), acc),
                        None => Break(Residue::Form("tuple field".to_string())),
                    }
                }
                _ => Break(Residue::Form("field access".to_string())),
            }
        }
        // `E::V { a, b }`: a struct variant, its fields in declaration order.
        rs::RustExpr::Struct(s)
            if !s.rest && !s.qself && data_variant(&s.path, &env.types).is_some() =>
        {
            let Some((owner, variant, fields)) = data_variant(&s.path, &env.types) else {
                return Break(Residue::Form("struct variant".to_string()));
            };
            let mut given = BTreeMap::new();
            for fv in &s.fields {
                let rs::Member::Named(id) = &fv.member else {
                    return Break(Residue::Form("struct variant field".to_string()));
                };
                given.insert(id.to_string(), &fv.expr);
            }
            if given.len() != fields.len() {
                return Break(Residue::Form("struct variant literal".to_string()));
            }
            let mut parts = vec![format!("ctor {owner}.{variant}")];
            let mut acc = acc;
            for (name, ty) in fields {
                let Some(value) = name.as_ref().and_then(|n| given.get(n)) else {
                    return Break(Residue::Form("struct variant literal".to_string()));
                };
                let ((v, vt), next) = flat_of(value, env, ctx, acc, Some(ty))?;
                acc = next;
                if !vt.same(ty) {
                    return Break(Residue::Form("struct variant field of another type".into()));
                }
                parts.push(v);
            }
            flat(format!("({})", parts.join(" ")), Ty::Named(owner), acc)
        }
        rs::RustExpr::Struct(s) if !s.rest && !s.qself => {
            let Some(name) = s.path.get_ident().map(|i| env.types.resolve(i)) else {
                return Break(Residue::Form("struct path".to_string()));
            };
            let Some(fields) = env.types.structs.get(&name) else {
                return Break(Residue::Form(format!("struct {name}")));
            };
            let mut given = BTreeMap::new();
            for fv in &s.fields {
                let rs::Member::Named(id) = &fv.member else {
                    return Break(Residue::Form("struct field".to_string()));
                };
                given.insert(local_name(id), &fv.expr);
            }
            if given.len() != fields.len() {
                return Break(Residue::Form("struct literal".to_string()));
            }
            let mut parts = Vec::new();
            let mut acc = acc;
            for (field, ty) in fields {
                let Some(value) = given.get(field) else {
                    return Break(Residue::Form("struct literal".to_string()));
                };
                let ((v, vt), next) = flat_of(value, env, ctx, acc, Some(ty))?;
                acc = next;
                if !vt.same(ty) {
                    return Break(Residue::Form("struct field of another type".to_string()));
                }
                parts.push((field, v));
            }
            flat(
                struct_value(&name, &parts, &env.types),
                Ty::Named(name),
                acc,
            )
        }
        rs::RustExpr::Tuple(t) if t.is_empty() => flat("()".into(), Ty::Unit, acc),
        rs::RustExpr::Tuple(t) if t.len() >= 2 => {
            let wants: Vec<Option<&Ty>> = match want {
                Some(Ty::Tuple(ts) | Ty::Pair(ts)) if ts.len() == t.len() => {
                    ts.iter().map(Some).collect()
                }
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
            flat(tuple_value(&ty, &parts), ty, acc)
        }
        rs::RustExpr::Cast(c) => {
            let ((v, vt), acc) = flat_of(&c.expr, env, ctx, acc, None)?;
            let target = match env.types.ty(&c.ty) {
                Continue(t) => t,
                Break(_) => return Break(Residue::Form("cast".to_string())),
            };
            match (&vt, &target) {
                (Ty::Nat(Some(a)), Ty::Nat(Some(b))) if a <= b => flat(v, target, acc),
                (Ty::Int(a), Ty::Int(b)) if a <= b => flat(v, target, acc),
                _ => Break(Residue::Form("narrowing or sign-changing cast".to_string())),
            }
        }
        // `xs[i]`: the element, and where Rust's bounds check panics, the
        // type's default, as Lean's `xs[i]!` is.
        rs::RustExpr::Index(ix) => {
            let ((base, bt), acc) = flat_of(&ix.expr, env, ctx, acc, None)?;
            let Ty::List(elem) = &bt else {
                return Break(Residue::Form(format!("index of {}", bt.render())));
            };
            let ((i, it), acc) = flat_of(&ix.index, env, ctx, acc, Some(&Ty::Nat(None)))?;
            if !matches!(it, Ty::Nat(_)) {
                return Break(Residue::Form("index by a range".to_string()));
            }
            let Some(d) = default_of(elem, &env.types) else {
                return Break(Residue::Form("index of a type with no value".to_string()));
            };
            let (v, acc) = ctx.fresh(env, acc, "item");
            Continue((
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
        rs::RustExpr::Continue(label) => loop_exit(env, ctx, acc, *label, false),
        rs::RustExpr::Break(b) if b.expr.is_none() => loop_exit(env, ctx, acc, b.label, true),
        rs::RustExpr::Try(_) => Break(Residue::Form("? outside a let".to_string())),
        rs::RustExpr::Macro(m) if m.path.is_ident("format") => format_macro(m, env, ctx, acc),
        // `matches!(e, P)`: a match with `true` for `P` and `false` otherwise.
        rs::RustExpr::Macro(m) if m.path.is_ident("matches") => {
            let Some((scrutinee, pat)) = &m.matches else {
                return Break(Residue::Form("matches! with a guard".to_string()));
            };
            let yes = rs::RustExpr::Lit(rs::Lit::Bool(true));
            let no = rs::RustExpr::Lit(rs::Lit::Bool(false));
            let wild = rs::Pat::Wild;
            let arms = vec![(&**pat, Body::Expr(&yes)), (&wild, Body::Expr(&no))];
            matching(scrutinee, &arms, env, ctx, acc, Some(&Ty::Bool))
        }
        rs::RustExpr::Macro(m) if m.path.is_ident("vec") => {
            let Some(items) = &m.args else {
                return Break(Residue::Form("vec! of a repeated value".to_string()));
            };
            list_of(items, env, ctx, acc, want)
        }
        rs::RustExpr::Array(items) => list_of(items, env, ctx, acc, want),
        _ => Break(Residue::Form(e.kind())),
    }
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
                return Break(Residue::Form(
                    "a list of items of different types".to_string(),
                ))
            }
            Some(_) => {}
            None => elem = Some(vt),
        }
        parts.push(v);
    }
    let Some(elem) = elem else {
        return Break(Residue::Form(
            "an empty list of an unknown type".to_string(),
        ));
    };
    let mut list = format!("(nil {})", elem.render());
    for part in parts.iter().rev() {
        list = format!("(cons {part} {list})");
    }
    Continue(((Expr::Flat(list), Ty::List(Box::new(elem))), acc))
}

/// Whether a format hole names a value: an identifier, without a format
/// specification.
fn is_identifier(hole: &str) -> bool {
    let mut chars = hole.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && hole != "_"
}

/// `format!` whose holes are all strings: the pieces joined.
fn format_macro(mac: &rs::ExprMacro, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let bad = |what: &str| Break(Residue::Form(format!("format! {what}")));
    let Some(args) = &mac.args else {
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
    Continue((
        (
            Expr::Flat(format!("(prim join {list} \"\" : String)")),
            Ty::Str,
        ),
        acc,
    ))
}

fn binary(b: &rs::ExprBinary, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    use rs::BinOp as B;
    let flat = |s: String, t: Ty, acc: Acc| Continue(((Expr::Flat(s), t), acc));
    let op_text = || b.op.text().to_string();
    match b.op {
        B::And | B::Or => {
            let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, Some(&Ty::Bool))?;
            let ((r, rt), acc) = flat_of(&b.right, env, ctx, acc, Some(&Ty::Bool))?;
            if lt != Ty::Bool || rt != Ty::Bool {
                return Break(Residue::Form("logic on non-booleans".to_string()));
            }
            let op = if matches!(b.op, B::And) { "&&" } else { "||" };
            flat(format!("({l} {op} {r})"), Ty::Bool, acc)
        }
        B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge => {
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
                (Ty::Int(_) | Ty::Str | Ty::Char, B::Eq) => {
                    (format!("(prim equal {l} {r} : Bool)"), acc)
                }
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
                    let same = match &**inner {
                        Ty::Bool | Ty::Named(_) if is_enum_or_bool(inner, env) => {
                            same_constructor(inner, &x, &y)
                        }
                        t => match equality(t, &x, &y) {
                            Some(e) => e,
                            None => {
                                return Break(Residue::Form(format!(
                                    "comparison {} of {}",
                                    op_text(),
                                    lt.render()
                                )))
                            }
                        },
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
                    return Continue((
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
                _ => {
                    return Break(Residue::Form(format!(
                        "comparison {} of {}",
                        op_text(),
                        lt.render()
                    )))
                }
            };
            flat(text, Ty::Bool, acc)
        }
        B::Div | B::Rem => {
            let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, None)?;
            match (&lt, int_literal(&b.right)) {
                (Ty::Nat(_), Some(k)) if k > 0 => {
                    let prim = if matches!(b.op, B::Div) {
                        "quotient"
                    } else {
                        "remainder"
                    };
                    flat(format!("(prim {prim} {l} {k} 0 : Nat)"), lt, acc)
                }
                _ => Break(Residue::Arithmetic(op_text())),
            }
        }
        _ => Break(Residue::Arithmetic(op_text())),
    }
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
    format!("equal{enum_name}")
}

fn is_enum_or_bool(t: &Ty, env: &Env) -> bool {
    match t {
        Ty::Bool => true,
        Ty::Named(n) => env.types.enums.contains_key(n),
        _ => false,
    }
}

fn call(
    c: &rs::ExprCall,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let flat = |s: String, t: Ty, acc: Acc| Continue(((Expr::Flat(s), t), acc));
    let rs::RustExpr::Path(p) = &*c.func else {
        return Break(Residue::Form("call of an expression".to_string()));
    };
    if let (Some(w), [arg]) = (wrapper(p), c.args.iter().collect::<Vec<_>>().as_slice()) {
        return match (w, want) {
            ("Some", _) => {
                let inner = match want {
                    Some(Ty::Option(t)) => Some(&**t),
                    _ => None,
                };
                let ((v, vt), acc) = flat_of(arg, env, ctx, acc, inner)?;
                flat(
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
                    return Break(Residue::Form("result of another type".to_string()));
                }
                flat(
                    format!(
                        "(ctor Result.{ctor} [{}, {}] {v})",
                        ok.render(),
                        err.render()
                    ),
                    t.clone(),
                    acc,
                )
            }
            _ => Break(Residue::Form("result of an unknown type".to_string())),
        };
    }
    // `E::V(a, b)`: a variant of a data-carrying enum.
    if let Some((owner, variant, fields)) = data_variant(p, &env.types) {
        if fields.len() != c.args.len() {
            return Break(Residue::Form(format!(
                "{owner}::{variant} of another arity"
            )));
        }
        let mut parts = vec![format!("ctor {owner}.{variant}")];
        let mut acc = acc;
        for (arg, (_, ty)) in c.args.iter().zip(fields) {
            let ((v, vt), next) = flat_of(arg, env, ctx, acc, Some(ty))?;
            acc = next;
            if !vt.same(ty) {
                return Break(Residue::Form(format!("{owner}::{variant} of another type")));
            }
            parts.push(v);
        }
        return flat(format!("({})", parts.join(" ")), Ty::Named(owner), acc);
    }
    let segments: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
    let call_args: Vec<&rs::RustExpr> = c.args.iter().collect();
    match (segments.as_slice(), call_args.as_slice(), want) {
        ([s, n], [], _) if s == "String" && n == "new" => return flat("\"\"".into(), Ty::Str, acc),
        ([s, n], [], Some(t @ Ty::List(inner))) if s == "Vec" && n == "new" => {
            return flat(format!("(nil {})", inner.render()), t.clone(), acc);
        }
        // An empty ordered set or map: the empty list of its members or of
        // its entries, which `insert` keeps sorted and free of duplicates.
        ([s, n], [], Some(t @ Ty::Set(inner, true))) if s == "BTreeSet" && n == "new" => {
            return flat(format!("(nil {})", inner.render()), t.clone(), acc);
        }
        ([s, n], [], Some(t @ Ty::Map(_, _, true))) if s == "BTreeMap" && n == "new" => {
            return flat(format!("(nil {})", t.entry().render()), t.clone(), acc);
        }
        // A capacity is a hint to the allocator: the value is the empty list.
        ([s, n], [_], Some(t @ Ty::List(inner))) if s == "Vec" && n == "with_capacity" => {
            return flat(format!("(nil {})", inner.render()), t.clone(), acc);
        }
        ([s, n], [a], _) if s == "String" && n == "from" => {
            let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(&Ty::Str))?;
            if vt == Ty::Str {
                return flat(v, Ty::Str, acc);
            }
            return Break(Residue::Form("String::from of another type".to_string()));
        }
        _ => {}
    }
    let mut name = segments.last().cloned().unwrap_or_default();
    if let [.., owner, method] = segments.as_slice() {
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
        return Break(Residue::Call(name));
    };
    if sig.params.len() != c.args.len() {
        return Break(Residue::Call(name));
    }
    let mut parts = vec![sig.name.clone()];
    let mut descends = None;
    let mut acc = acc;
    for (i, (arg, ty)) in c.args.iter().zip(&sig.params).enumerate() {
        let ((v, vt), next) = flat_of(arg, env, ctx, acc, Some(ty))?;
        acc = next;
        if !vt.same(ty) {
            return Break(Residue::Form(format!("argument of another type to {name}")));
        }
        if name == ctx.current && ctx.tails.get(&v) == ctx.params.get(i) && descends.is_none() {
            descends = Some(i);
        }
        parts.push(v);
    }
    let acc = if name == ctx.current {
        let Some(i) = descends else {
            return Break(Residue::Form(
                "recursion that does not descend on a slice".to_string(),
            ));
        };
        acc.descended(i)
    } else {
        acc.called(name)
    };
    flat(format!("({})", parts.join(" ")), sig.result.clone(), acc)
}

/// The module of the byte-level text prelude every projection carries.
pub(crate) const PRELUDE: &str = "Prelude";

/// The prelude's source: the definitions string methods lift to.
pub(crate) const PRELUDE_SOURCE: &str = include_str!("../assets/project/src/Prelude.lex.tex");

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
    let flat = |s: String, t: Ty, acc: Acc| Continue((Some((Expr::Flat(s), t)), acc));
    // A pattern: a string or a character.
    let pattern = |a: &rs::RustExpr, acc: Acc| -> Lift<(String, Acc)> {
        let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(&Ty::Str))?;
        match vt {
            Ty::Str | Ty::Char => Continue((v, acc)),
            _ => Break(Residue::Form(format!("a .{name} pattern that is not text"))),
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
                return Break(Residue::Form(format!(
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
            Continue((
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
            Continue((Some((body, opt(pair))), acc))
        }
        ("replace", [from, to]) if !chars => {
            let (from, acc) = pattern(from, acc)?;
            let ((to, tt), acc) = flat_of(to, env, ctx, acc, Some(&Ty::Str))?;
            if !matches!(tt, Ty::Str | Ty::Char) {
                return Break(Residue::Form("a replacement that is not text".into()));
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
                return Break(Residue::Form("a repeat count that is not a number".into()));
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
        ("to_ascii_uppercase", []) => {
            let (s, acc) = acc.prelude("textAsciiUpper", &[r]);
            flat(s, rt.clone(), acc)
        }
        ("eq_ignore_ascii_case", [other]) => {
            let ((o, ot), acc) = flat_of(other, env, ctx, acc, Some(rt))?;
            if !ot.same(rt) {
                return Break(Residue::Form("eq_ignore_ascii_case of another type".into()));
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
                    [rs::GenericArgument::Type(t)] => Some(env.types.ty(t)?),
                    _ => None,
                },
                None => match want {
                    Some(Ty::Outcome(t, _) | Ty::Option(t)) => Some((**t).clone()),
                    _ => None,
                },
            };
            let bits = match target {
                Some(Ty::Nat(Some(bits))) if bits <= 64 => bits,
                Some(Ty::Nat(None)) => {
                    return Break(Residue::Width(".parse into usize".to_string()))
                }
                _ => return Break(Residue::Form("parse into a type outside the lift".into())),
            };
            let (parsed, acc) = acc.prelude("textParseNat", &[r, max_of(bits)]);
            flat(
                parsed,
                Ty::Outcome(Box::new(Ty::Nat(Some(bits))), Box::new(Ty::ParseError)),
                acc,
            )
        }
        _ => Continue((None, acc)),
    }
}

/// The axioms of the runtime's string length and decimal formatting.
const ALL_AXIOMS: [&str; 3] = ["Classical.choice", "Quot.sound", "propext"];

fn method(
    m: &rs::ExprMethodCall,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let flat = |s: String, t: Ty, acc: Acc| Continue(((Expr::Flat(s), t), acc));
    let mut acc = acc;
    let name = m.method.to_string();
    // `s.chars().count()`: the number of characters, which is LexLean's
    // string length.
    if let rs::RustExpr::MethodCall(inner) = &*m.receiver {
        if name == "count" && m.args.is_empty() && inner.method == "chars" && inner.args.is_empty()
        {
            let ((s, st), after) = flat_of(&inner.receiver, env, ctx, acc, Some(&Ty::Str))?;
            acc = after;
            if st == Ty::Str {
                let acc = acc.all_axioms();
                return flat(format!("(prim length {s} : Nat)"), Ty::Nat(None), acc);
            }
        }
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
    let ((r, rt), mut acc) = flat_of(&m.receiver, env, ctx, acc, receiver_want.as_ref())?;
    let args: Vec<&rs::RustExpr> = m.args.iter().collect();
    let arg = |want: &Ty, acc: Acc| -> Lift<(String, Acc)> {
        let [a] = args.as_slice() else {
            return Break(Residue::Form(format!("method call .{name}")));
        };
        let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(want))?;
        if vt.same(want) {
            Continue((v, acc))
        } else {
            Break(Residue::Form(format!(
                "argument of another type to .{name}"
            )))
        }
    };
    let some = |t: &Ty, v: &str| format!("(ctor Option.some [{}] {v})", t.render());
    let none = |t: &Ty| format!("(ctor Option.none [{}])", t.render());
    let opt = |t: &Ty| Ty::Option(Box::new(t.clone()));
    if let Ty::Named(owner) = &rt {
        let key = format!("{owner}::{name}");
        if let Some(sig) = env.fns.get(&key) {
            if key == ctx.current {
                // Recursion on a sub-term of `self` descends; any other does not.
                if ctx.tails.get(&r) != ctx.params.first() {
                    return Break(Residue::Form("a method that calls itself".to_string()));
                }
                acc = acc.descended(0);
            }
            if sig.params.len() != args.len() + 1 {
                return Break(Residue::Call(key));
            }
            let mut parts = vec![sig.name.clone(), r];
            for (a, ty) in args.iter().zip(&sig.params[1..]) {
                let ((v, vt), next) = flat_of(a, env, ctx, acc, Some(ty))?;
                acc = next;
                if !vt.same(ty) {
                    return Break(Residue::Form(format!(
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
            return flat(format!("({})", parts.join(" ")), sig.result.clone(), acc);
        }
    }
    if matches!(rt, Ty::Map(..) | Ty::Set(..)) {
        return collection(&name, &r, &rt, &args, env, ctx, acc);
    }
    if matches!(rt, Ty::Str | Ty::Char) {
        let (lifted, after) = text_method(m, &name, &r, &rt, &args, env, ctx, acc, want)?;
        if let Some(lifted) = lifted {
            return Continue((lifted, after));
        }
        acc = after;
    }
    if let Ty::List(elem) = &rt {
        let (lifted, after) = combinator(m, &name, &r, elem, &args, env, ctx, acc)?;
        if let Some(lifted) = lifted {
            return Continue((lifted, after));
        }
        acc = after;
    }
    if rt == Ty::Nat(Some(8)) && args.is_empty() {
        if let Some(test) = ascii_byte(&name, &r) {
            return flat(test, Ty::Bool, acc);
        }
    }
    match (&rt, name.as_str()) {
        (Ty::Nat(w), "checked_add" | "checked_mul") => {
            let Some(bits) = w else {
                return Break(Residue::Width(format!(".{name} on usize")));
            };
            let (a, acc) = arg(&rt, acc)?;
            let v = if name == "checked_add" {
                format!("({r} + {a})")
            } else {
                format!("(prim multiply {r} {a} : Nat)")
            };
            flat(
                format!(
                    "(if ({v} <? {}) then {} else {})",
                    pow2(*bits),
                    some(&rt, &v),
                    none(&rt)
                ),
                opt(&rt),
                acc,
            )
        }
        (Ty::Nat(_), "checked_sub") => {
            let (a, acc) = arg(&rt, acc)?;
            flat(
                format!(
                    "(if ({a} <=? {r}) then {} else {})",
                    some(&rt, &format!("(prim subtract {r} {a} : Nat)")),
                    none(&rt)
                ),
                opt(&rt),
                acc,
            )
        }
        (Ty::Nat(_), "checked_div" | "checked_rem") => {
            let (a, acc) = arg(&rt, acc)?;
            let prim = if name == "checked_div" {
                "quotient"
            } else {
                "remainder"
            };
            flat(
                format!(
                    "(if ({a} =? 0) then {} else {})",
                    none(&rt),
                    some(&rt, &format!("(prim {prim} {r} {a} 0 : Nat)"))
                ),
                opt(&rt),
                acc,
            )
        }
        (Ty::Nat(w), "wrapping_add" | "wrapping_mul" | "wrapping_sub") => {
            let Some(bits) = w else {
                return Break(Residue::Width(format!(".{name} on usize")));
            };
            let (a, acc) = arg(&rt, acc)?;
            let m = pow2(*bits);
            let v = match name.as_str() {
                "wrapping_add" => format!("({r} + {a})"),
                "wrapping_mul" => format!("(prim multiply {r} {a} : Nat)"),
                _ => format!("(prim subtract ({r} + {m}) {a} : Nat)"),
            };
            flat(format!("(prim remainder {v} {m} 0 : Nat)"), rt.clone(), acc)
        }
        (Ty::Nat(w), "saturating_add" | "saturating_mul") => {
            let Some(bits) = w else {
                return Break(Residue::Width(format!(".{name} on usize")));
            };
            let (a, acc) = arg(&rt, acc)?;
            let v = if name == "saturating_add" {
                format!("({r} + {a})")
            } else {
                format!("(prim multiply {r} {a} : Nat)")
            };
            flat(
                format!(
                    "(if ({v} <? {}) then {v} else {})",
                    pow2(*bits),
                    max_of(*bits)
                ),
                rt.clone(),
                acc,
            )
        }
        (Ty::Nat(_), "saturating_sub") => {
            let (a, acc) = arg(&rt, acc)?;
            flat(format!("(prim subtract {r} {a} : Nat)"), rt.clone(), acc)
        }
        (Ty::Nat(_), "abs_diff") => {
            let (a, acc) = arg(&rt, acc)?;
            flat(
                format!("(if ({r} <=? {a}) then (prim subtract {a} {r} : Nat) else (prim subtract {r} {a} : Nat))"),
                rt.clone(), acc)
        }
        (Ty::Nat(_), "midpoint") => {
            let (a, acc) = arg(&rt, acc)?;
            flat(
                format!("(prim quotient ({r} + {a}) 2 0 : Nat)"),
                rt.clone(),
                acc,
            )
        }
        (Ty::Nat(_), "min" | "max") => {
            let (a, acc) = arg(&rt, acc)?;
            let (x, y) = if name == "min" { (&r, &a) } else { (&a, &r) };
            flat(
                format!("(if ({r} <=? {a}) then {x} else {y})"),
                rt.clone(),
                acc,
            )
        }
        (Ty::Int(bits), "checked_add" | "checked_sub" | "checked_mul" | "checked_div") => {
            if *bits != 64 {
                return Break(Residue::Width(format!(".{name} on i{bits}")));
            }
            let (a, acc) = arg(&rt, acc)?;
            let prim = match name.as_str() {
                "checked_add" => "checked_add",
                "checked_sub" => "checked_subtract",
                "checked_mul" => "checked_multiply",
                _ => "checked_quotient",
            };
            flat(
                format!("(prim {prim} {r} {a} : (Option Int64))"),
                opt(&rt),
                acc,
            )
        }
        (Ty::Int(bits), "checked_neg") if args.is_empty() => {
            if *bits != 64 {
                return Break(Residue::Width(format!(".{name} on i{bits}")));
            }
            flat(
                format!("(prim checked_negate {r} : (Option Int64))"),
                opt(&rt),
                acc,
            )
        }
        (Ty::Int(_), "min" | "max") => {
            let acc = acc.axiom("propext");
            let (a, acc) = arg(&rt, acc)?;
            let (x, y) = if name == "min" { (&r, &a) } else { (&a, &r) };
            flat(
                format!("(if (prim less_than {a} {r} : Bool) then {y} else {x})"),
                rt.clone(),
                acc,
            )
        }
        (Ty::List(_), "len") if args.is_empty() => {
            flat(format!("(prim length {r} : Nat)"), Ty::Nat(None), acc)
        }
        (Ty::List(_), "is_empty") if args.is_empty() => {
            flat(format!("((prim length {r} : Nat) =? 0)"), Ty::Bool, acc)
        }
        (Ty::List(t), "first") if args.is_empty() => flat(
            format!("(prim index {r} 0 : (Option {}))", t.render()),
            opt(t),
            acc,
        ),
        (Ty::List(t), "get") => {
            let (i, acc) = arg(&Ty::Nat(None), acc)?;
            flat(
                format!("(prim index {r} {i} : (Option {}))", t.render()),
                opt(t),
                acc,
            )
        }
        (Ty::Str, "is_empty") if args.is_empty() => {
            flat(format!("(prim equal {r} \"\" : Bool)"), Ty::Bool, acc)
        }
        (Ty::Str, "to_string" | "to_owned") | (_, "clone" | "copied" | "cloned" | "to_vec")
            if args.is_empty() =>
        {
            flat(r, rt.clone(), acc)
        }
        (Ty::Option(_), "is_some" | "is_none") if args.is_empty() => {
            let (v, acc) = ctx.fresh(env, acc, "value");
            let (n, s) = if name == "is_some" {
                ("false", "true")
            } else {
                ("true", "false")
            };
            Continue((
                (
                    Expr::Match {
                        scrutinee: r,
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
        (Ty::Option(t), "unwrap_or") => {
            let (d, acc) = arg(t, acc)?;
            let (v, acc) = ctx.fresh(env, acc, "value");
            Continue((
                (
                    Expr::Match {
                        scrutinee: r,
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
                    (**t).clone(),
                ),
                acc,
            ))
        }
        (Ty::Outcome(_, _), "is_ok" | "is_err") if args.is_empty() => {
            let (v, acc) = ctx.fresh(env, acc, "value");
            let (e, acc) = ctx.fresh(env, acc, "error");
            let (o, x) = if name == "is_ok" {
                ("true", "false")
            } else {
                ("false", "true")
            };
            Continue((
                (
                    Expr::Match {
                        scrutinee: r,
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
        (Ty::Outcome(t, _), "ok") if args.is_empty() => {
            let (v, acc) = ctx.fresh(env, acc, "value");
            let (e, acc) = ctx.fresh(env, acc, "error");
            Continue((
                (
                    Expr::Match {
                        scrutinee: r,
                        arms: vec![
                            Arm {
                                pattern: format!("Result.ok {v}"),
                                body: Expr::Flat(some(t, &v)),
                            },
                            Arm {
                                pattern: format!("Result.error {e}"),
                                body: Expr::Flat(none(t)),
                            },
                        ],
                    },
                    opt(t),
                ),
                acc,
            ))
        }
        (Ty::Option(_) | Ty::Outcome(..), _) => {
            wrapped_method(&name, &r, &rt, &args, env, ctx, acc)
        }
        _ => Break(Residue::Form(format!("method call .{name}"))),
    }
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
        Ty::Nat(_) => "0".into(),
        Ty::Int(_) => "(0 : Int64)".into(),
        Ty::Str | Ty::Char => "\"\"".into(),
        Ty::ParseError => format!("(ctor {PRELUDE}::ParseIntError.InvalidDigit)"),
        Ty::Option(t) => format!("(ctor Option.none [{}])", t.render()),
        Ty::Outcome(..) => return None,
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
    let bad = || Break(Residue::Form(format!("method call .{name}")));
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
            Continue((lam, acc))
        };
    let (fresh_e, acc) = ctx.fresh(env, acc, "error");
    match (name, args) {
        ("map", [f]) => {
            let (lam, acc) = closure(f, std::slice::from_ref(&value), acc, None)?;
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            match &failure {
                None => Continue((
                    (
                        two(&v, wrap_some(&lam.ty, body), "", Expr::Flat(none(&lam.ty))),
                        opt(&lam.ty),
                    ),
                    acc,
                )),
                Some(e) => Continue((
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
                return Break(Residue::Form("map_or of another type".into()));
            }
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            Continue((
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
            Continue((
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
                _ => return Break(Residue::Form("and_then of another type".into())),
            };
            Continue((
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
            Continue((
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
                return Continue(((two(&v, otherwise, &b, test), Ty::Bool), acc));
            }
            Continue(((two(&b, test, &fresh_e, otherwise), Ty::Bool), acc))
        }
        ("ok_or", [e]) if failure.is_none() => {
            let ((ev, et), acc) = flat_of(e, env, ctx, acc, None)?;
            let (v, acc) = ctx.fresh(env, acc, "value");
            let types = result_of(&value, &et);
            Continue((
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
            Continue((
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
                return Break(Residue::Form("unwrap_or of another type".into()));
            }
            let (v, acc) = ctx.fresh(env, acc, "value");
            Continue((
                (
                    two(&v, Expr::Flat(v.clone()), &fresh_e, Expr::Flat(dv)),
                    value,
                ),
                acc,
            ))
        }
        ("unwrap_or_else", [f]) => {
            let params: Vec<Ty> = failure.iter().cloned().collect();
            let (lam, acc) = closure(f, &params, acc, Some(&value))?;
            if !lam.ty.same(&value) {
                return Break(Residue::Form("unwrap_or_else of another type".into()));
            }
            let e = lam
                .binders
                .first()
                .cloned()
                .unwrap_or_else(|| fresh_e.clone());
            let (v, acc) = ctx.fresh(env, acc, "value");
            let fallback = lam.wrap(lam.core.clone());
            Continue(((two(&v, Expr::Flat(v.clone()), &e, fallback), value), acc))
        }
        ("unwrap_or_default" | "unwrap" | "expect", _) => {
            let rust_default = name == "unwrap_or_default";
            if rust_default && matches!(value, Ty::Named(_)) {
                return Break(Residue::Form("unwrap_or_default of a named type".into()));
            }
            let Some(d) = default_of(&value, &env.types) else {
                return Break(Residue::Form(format!(".{name} of a type with no value")));
            };
            let (v, acc) = ctx.fresh(env, acc, "value");
            Continue((
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

fn bind_pat(
    p: &rs::Pat,
    ty: &Ty,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    lets: Lets,
    nested: bool,
) -> Lift<(Ctx, String, Acc, Lets)> {
    match (p, ty) {
        (rs::Pat::Ident(i), _) if i.subpat.is_none() && !i.by_ref => {
            let (scope, name, acc) = ctx.bind(env, acc, &i.ident, ty.clone());
            Continue((scope, name, acc, lets))
        }
        (rs::Pat::Wild, _) => {
            let (name, acc) = ctx.fresh(env, acc, "item");
            Continue((ctx.clone(), name, acc, lets))
        }
        (rs::Pat::Reference(r), _) => bind_pat(r, ty, env, ctx, acc, lets, nested),
        (rs::Pat::Type(pt), _) => bind_pat(&pt.pat, ty, env, ctx, acc, lets, nested),
        (rs::Pat::Tuple(tp), Ty::Tuple(ts) | Ty::Pair(ts)) if !nested && tp.len() == ts.len() => {
            let (whole, mut acc) = ctx.fresh(env, acc, "pair");
            let mut scope = ctx.clone();
            let mut lets = lets;
            for (i, (e, t)) in tp.iter().zip(ts).enumerate() {
                let (next, name, after, more) = bind_pat(e, t, env, &scope, acc, lets, true)?;
                scope = next;
                acc = after;
                lets = more;
                lets.push((name, t.render(), tuple_item(ty, &whole, i)));
            }
            Continue((scope, whole, acc, lets))
        }
        _ => Break(Residue::Form("closure pattern".to_string())),
    }
}

/// A closure argument over parameters of these types.
fn lambda(
    e: &rs::RustExpr,
    params: &[Ty],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<(Lambda, Acc)> {
    let rs::RustExpr::Closure(c) = e else {
        return Break(Residue::Form(
            "an iterator adapter given a function, not a closure".to_string(),
        ));
    };
    if c.inputs.len() != params.len() || c.asyncness {
        return Break(Residue::Form("closure".to_string()));
    }
    let (mut acc, before) = acc.apart();
    let mut scope = Ctx {
        tails: BTreeMap::new(),
        top: false,
        looping: None,
        hidden: Vec::new(),
        ..ctx.clone()
    };
    let mut binders = Vec::new();
    let mut lets = Vec::new();
    for (p, ty) in c.inputs.iter().zip(params) {
        let (next, name, after, more) = bind_pat(p, ty, env, &scope, acc, lets, false)?;
        scope = next;
        acc = after;
        lets = more;
        binders.push(name);
    }
    let ((core, ty), acc) = flat_of(&c.body, env, &scope, acc, want)?;
    let (acc, own) = acc.rejoin(before);
    if !own.descends.is_empty() {
        return Break(Residue::Form("recursion inside a closure".to_string()));
    }
    Continue((
        Lambda {
            binders,
            lets,
            core,
            ty,
            calls: own.calls,
            axioms: own.axioms,
        },
        acc,
    ))
}

/// The outer locals a helper's body names: they become its parameters.
fn captures(ctx: &Ctx, texts: &[String], bound: &[String]) -> Vec<(String, Ty)> {
    let mut tokens: BTreeSet<&str> = BTreeSet::new();
    for text in texts {
        tokens.extend(text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')));
    }
    let found: BTreeSet<(String, Ty)> = ctx
        .locals
        .values()
        .filter(|(n, _)| tokens.contains(n.as_str()) && !bound.contains(n))
        .cloned()
        .collect();
    found.into_iter().collect()
}

/// The recursive call of a walk, given the values it threads.
type Recur<'a> = &'a dyn Fn(&[String]) -> String;
/// A walk's step from its recursive call.
type Step<'a> = &'a dyn Fn(Recur<'_>, &str) -> Expr;

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
    cons: Step<'_>,
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
    });
    let acc = acc.called(format!("#{name}"));
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
        return Break(Residue::Form(format!("{kind} of another type")));
    }
    let (head, acc) = ctx.fresh(env, acc, "item");
    let (probe, acc) = ctx.fresh(env, acc, "needle");
    let subject = if value.is_some() {
        format!("(project {head} item0)")
    } else {
        head.clone()
    };
    let Some(test) = equality(&key_ty, &subject, &probe) else {
        return Break(Residue::Form(format!("{kind} over keys of this type")));
    };
    let lam = Lambda {
        binders: vec![head.clone()],
        lets: Vec::new(),
        core: test.clone(),
        ty: Ty::Bool,
        calls: BTreeSet::new(),
        axioms: BTreeSet::new(),
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
    Continue(walk(
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
    let flat = |s: String, t: Ty, acc: Acc| Continue(((Expr::Flat(s), t), acc));
    let unordered = || {
        Break(Residue::Form(format!(
            ".{name} on a hash collection, whose order is unspecified"
        )))
    };
    match (rt, name, args) {
        (_, "len", []) => flat(format!("(prim length {r} : Nat)"), Ty::Nat(None), acc),
        (_, "is_empty", []) => flat(format!("((prim length {r} : Nat) =? 0)"), Ty::Bool, acc),
        (Ty::Map(_, v, _), "get", [k]) => lookup(env, ctx, acc, "Get", r, &rt.entry(), Some(v), k),
        (Ty::Map(_, v, _), "contains_key", [k]) => {
            lookup(env, ctx, acc, "Has", r, &rt.entry(), Some(v), k)
        }
        (Ty::Set(t, _), "contains", [k]) => lookup(env, ctx, acc, "Contains", r, t, None, k),
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
            };
            let cons = |rec: Recur<'_>, _: &str| Expr::Flat(format!("(cons {core} {})", rec(&[])));
            let kind = if name == "keys" { "Keys" } else { "Values" };
            Continue(walk(
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
        _ => Break(Residue::Form(format!("method call .{name}"))),
    }
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
    let flat = |s: String, t: Ty, acc: Acc| Continue((Some((Expr::Flat(s), t)), acc));
    match (name, args) {
        ("iter" | "into_iter", []) => flat(r.to_string(), list, acc),
        ("collect", []) => {
            let to_string = matches!(
                m.turbofish.as_deref(),
                Some([rs::GenericArgument::Type(t)]) if t.text == "String"
            );
            if to_string {
                if *elem != Ty::Str {
                    return Break(Residue::Form("collect of non-strings into a String".into()));
                }
                let acc = acc.axiom("propext");
                return flat(format!("(prim join {r} \"\" : String)"), Ty::Str, acc);
            }
            flat(r.to_string(), list, acc)
        }
        ("count", []) => flat(format!("(prim length {r} : Nat)"), Ty::Nat(None), acc),
        ("join", [sep]) if *elem == Ty::Str => {
            let ((s, st), acc) = flat_of(sep, env, ctx, acc, Some(&Ty::Str))?;
            if st != Ty::Str {
                return Break(Residue::Form(
                    "join with a separator of another type".into(),
                ));
            }
            let acc = acc.axiom("propext");
            flat(format!("(prim join {r} {s} : String)"), Ty::Str, acc)
        }
        ("contains", [needle]) => Continue(some(lookup(
            env, ctx, acc, "Contains", r, elem, None, needle,
        )?)),
        ("map" | "filter" | "any" | "all" | "find", [f]) => {
            let want = if name == "map" { None } else { Some(&Ty::Bool) };
            let (lam, acc) = lambda(f, std::slice::from_ref(elem), env, ctx, acc, want)?;
            if name != "map" && lam.ty != Ty::Bool {
                return Break(Residue::Form(format!(
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
                    _ => format!(
                        "(if {core} then (ctor Option.some [{elem_text}] {head}) else {next})"
                    ),
                })
            };
            Continue(some(walk(
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
        ("fold", [init, f]) => {
            let ((iv, it), acc) = flat_of(init, env, ctx, acc, None)?;
            let (lam, acc) = lambda(f, &[it.clone(), elem.clone()], env, ctx, acc, Some(&it))?;
            if !lam.ty.same(&it) {
                return Break(Residue::Form("a fold step of another type".into()));
            }
            let folded = lam.binders[0].clone();
            let head = lam.binders[1].clone();
            let core = lam.core.clone();
            let cons = |rec: Recur<'_>, _: &str| Expr::Flat(rec(std::slice::from_ref(&core)));
            Continue(some(walk(
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
        ("nth", [n]) => {
            let ((n, nt), acc) = flat_of(n, env, ctx, acc, Some(&Ty::Nat(None)))?;
            if !matches!(nt, Ty::Nat(_)) {
                return Break(Residue::Form("nth of another type".into()));
            }
            flat(
                format!("(prim index {r} {n} : (Option {}))", elem.render()),
                Ty::Option(Box::new(elem.clone())),
                acc,
            )
        }
        ("chain", [other]) => {
            let ((o, ot), acc) = flat_of(other, env, ctx, acc, Some(&list))?;
            if !ot.same(&list) {
                return Break(Residue::Form("chain of another type".into()));
            }
            flat(
                format!("(prim append {r} {o} : {})", list.render()),
                list,
                acc,
            )
        }
        ("rev" | "enumerate", []) => {
            let (head, acc) = ctx.fresh(env, acc, "item");
            let lam = Lambda {
                binders: vec![head.clone()],
                lets: Vec::new(),
                core: head.clone(),
                ty: elem.clone(),
                calls: BTreeSet::new(),
                axioms: BTreeSet::new(),
            };
            if name == "rev" {
                let e = elem.render();
                let cons = |rec: Recur<'_>, _: &str| {
                    Expr::Flat(format!(
                        "(prim append {} (cons {head} (nil {e})) : (List {e}))",
                        rec(&[])
                    ))
                };
                return Continue(some(walk(
                    env,
                    ctx,
                    acc,
                    "Rev",
                    r,
                    elem,
                    &lam,
                    &head,
                    &[],
                    &list,
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
            Continue(some(walk(
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
        ("skip" | "take", [n]) => {
            let ((n, nt), acc) = flat_of(n, env, ctx, acc, Some(&Ty::Nat(None)))?;
            if !matches!(nt, Ty::Nat(_)) {
                return Break(Residue::Form(format!(".{name} of another type")));
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
            Continue(some(walk(
                env,
                ctx,
                acc,
                kind,
                r,
                elem,
                &lam,
                &head,
                &[(count.clone(), Ty::Nat(None), n)],
                &list,
                format!("(nil {e})"),
                &cons,
            )))
        }
        ("take_while" | "skip_while" | "position", [f]) => {
            let (lam, acc) = lambda(
                f,
                std::slice::from_ref(elem),
                env,
                ctx,
                acc,
                Some(&Ty::Bool),
            )?;
            if lam.ty != Ty::Bool {
                return Break(Residue::Form(format!(
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
                return Continue(some(walk(
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
            Continue(some(walk(
                env,
                ctx,
                acc,
                kind,
                r,
                elem,
                &lam,
                &head,
                &[],
                &list,
                format!("(nil {e})"),
                &cons,
            )))
        }
        ("filter_map" | "find_map" | "flat_map", [f]) => {
            let (lam, acc) = lambda(f, std::slice::from_ref(elem), env, ctx, acc, None)?;
            let inner = match (&lam.ty, name) {
                (Ty::Option(u), "filter_map" | "find_map") | (Ty::List(u), "flat_map") => {
                    (**u).clone()
                }
                _ => return Break(Residue::Form(format!("a .{name} step of another type"))),
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
            Continue(some(walk(
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
        _ => Continue((None, acc)),
    }
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

/// The binder of a sub-pattern: a name, or `_`.
fn binder(p: &rs::Pat) -> Option<Option<String>> {
    match p {
        rs::Pat::Ident(i) if i.subpat.is_none() && !i.by_ref => Some(Some(i.ident.to_string())),
        rs::Pat::Wild => Some(None),
        rs::Pat::Reference(r) => binder(r),
        // A tuple of names, which the arm binds by projection.
        rs::Pat::Tuple(t) if t.len() >= 2 => {
            let mut items = Vec::new();
            for e in t {
                items.push(binder(e)?.unwrap_or_else(|| "_".to_string()));
            }
            if items.iter().any(|i| i.starts_with('#')) {
                return None;
            }
            Some(Some(format!("#tuple:{}", items.join(","))))
        }
        _ => None,
    }
}

/// A constructor pattern: `None`, `Some(x)`, `Ok(x)`, `Err(x)`, `E::V`, a
/// slice pattern, `true`/`false`, or `_`.
enum Shape {
    Wild,
    Ctor(String, Vec<Option<String>>),
}

fn shape(p: &rs::Pat) -> Option<Shape> {
    match p {
        rs::Pat::Wild => Some(Shape::Wild),
        rs::Pat::Ident(i) if i.ident == "None" && i.subpat.is_none() => {
            Some(Shape::Ctor("None".into(), Vec::new()))
        }
        rs::Pat::Path(pp) => {
            let segs: Vec<String> = pp.segments.iter().map(|s| s.ident.to_string()).collect();
            if wrapper(pp) == Some("None") {
                return Some(Shape::Ctor("None".into(), Vec::new()));
            }
            Some(Shape::Ctor(segs.join("::"), Vec::new()))
        }
        rs::Pat::TupleStruct(ts) => {
            let w = wrapper(&ts.path)?;
            let [inner] = ts.elems.iter().collect::<Vec<_>>()[..] else {
                return None;
            };
            Some(Shape::Ctor(w.into(), vec![binder(inner)?]))
        }
        rs::Pat::Lit(rs::Lit::Bool(b)) => Some(Shape::Ctor(b.to_string(), Vec::new())),
        rs::Pat::Slice(s) => {
            let elems: Vec<&rs::Pat> = s.iter().collect();
            match elems.as_slice() {
                [] => Some(Shape::Ctor("[]".into(), Vec::new())),
                [head, tail] => {
                    let tail = match tail {
                        rs::Pat::Rest => None,
                        rs::Pat::Ident(i) => match &i.subpat {
                            Some(sub) if matches!(**sub, rs::Pat::Rest) => {
                                Some(i.ident.to_string())
                            }
                            _ => return None,
                        },
                        _ => return None,
                    };
                    Some(Shape::Ctor("[..]".into(), vec![binder(head)?, tail]))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// A pattern of a data-carrying enum's variant: `E::V(a, _)`, `E::V(..)`,
/// `E::V { a, b: c, .. }`, as its binders in field order.
fn data_shape(p: &rs::Pat, st: &Ty, types: &Types) -> Option<Shape> {
    let Ty::Named(n) = st else { return None };
    let variants = types.datas.get(n)?;
    let path = match p {
        rs::Pat::TupleStruct(ts) => &ts.path,
        rs::Pat::Struct(ps) => &ps.path,
        _ => return None,
    };
    let variant = path.segments.last()?.ident.to_string();
    let (_, fields) = variants.iter().find(|(v, _)| *v == variant)?;
    let binders = match p {
        rs::Pat::TupleStruct(ts) => {
            let elems: Vec<&rs::Pat> = ts.elems.iter().collect();
            let rest = elems.iter().position(|e| matches!(e, rs::Pat::Rest));
            let mut out = Vec::new();
            match rest {
                None if elems.len() == fields.len() => {
                    for e in elems {
                        out.push(binder(e)?);
                    }
                }
                Some(at) if elems.len() <= fields.len() + 1 => {
                    for e in &elems[..at] {
                        out.push(binder(e)?);
                    }
                    let after = &elems[at + 1..];
                    out.extend(std::iter::repeat_n(None, fields.len() + 1 - elems.len()));
                    for e in after {
                        out.push(binder(e)?);
                    }
                }
                _ => return None,
            }
            out
        }
        rs::Pat::Struct(ps) => {
            let mut given = BTreeMap::new();
            for f in &ps.fields {
                let rs::Member::Named(id) = &f.member else {
                    return None;
                };
                given.insert(id.to_string(), binder(&f.pat)?);
            }
            if !ps.rest && given.len() != fields.len() {
                return None;
            }
            let mut out = Vec::new();
            for (name, _) in fields {
                out.push(match name {
                    Some(name) => given.remove(name).flatten(),
                    None => None,
                });
            }
            if !given.is_empty() {
                return None;
            }
            out
        }
        _ => return None,
    };
    Some(Shape::Ctor(variant, binders))
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

fn matching(
    scrutinee: &rs::RustExpr,
    arms: &[(&rs::Pat, Body<'_>)],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let ((s, st), acc) = flat_of(scrutinee, env, &ctx.below(), acc, None)?;
    if matches!(st, Ty::Nat(_) | Ty::Int(_) | Ty::Str | Ty::Char)
        || arms.iter().any(|(_, b)| matches!(b, Body::Guarded(..)))
    {
        return literal_match(&s, &st, arms, env, ctx, acc, want);
    }
    let mut shapes = Vec::new();
    for (pat, b) in arms {
        let cases: Vec<&rs::Pat> = match pat {
            rs::Pat::Or(o) => o.iter().collect(),
            p => vec![*p],
        };
        for case in cases {
            let Some(sh) = data_shape(case, &st, &env.types).or_else(|| shape(case)) else {
                return Break(Residue::Form("match pattern".to_string()));
            };
            shapes.push((sh, *b));
        }
    }
    if st == Ty::Bool {
        let pick = |v: &str| {
            shapes.iter().find_map(|(sh, b)| match sh {
                Shape::Ctor(k, _) if k == v => Some(*b),
                Shape::Wild => Some(*b),
                Shape::Ctor(..) => None,
            })
        };
        let (Some(t), Some(f)) = (pick("true"), pick("false")) else {
            return Break(Residue::Form("non-exhaustive match".to_string()));
        };
        let ((then, tt), acc) = body(t, env, ctx, acc, want)?;
        let ((other, ot), acc) = body(f, env, ctx, acc, Some(want.unwrap_or(&tt)))?;
        if !tt.same(&ot) {
            return Break(Residue::Form("arms of different types".to_string()));
        }
        return Continue((
            (
                Expr::If {
                    condition: Box::new(Expr::Flat(s)),
                    then: Box::new(then),
                    otherwise: Box::new(other),
                },
                tt,
            ),
            acc,
        ));
    }
    let Some(ctors) = constructors(&st, &env.types) else {
        return Break(Residue::Form(format!("match on {}", st.render())));
    };
    let mut out = Vec::new();
    let mut result: Option<Ty> = want.cloned();
    let mut acc = acc;
    for (key, lex, binder_tys) in &ctors {
        let found = shapes.iter().find_map(|(sh, b)| match sh {
            Shape::Ctor(k, binders) => {
                let k = k.rsplit("::").next().unwrap_or(k);
                (k == key).then(|| (Some(binders.clone()), *b))
            }
            Shape::Wild => Some((None, *b)),
        });
        let Some((binders, b)) = found else {
            return Break(Residue::Form("non-exhaustive match".to_string()));
        };
        let mut scope = ctx.clone();
        let mut names = Vec::new();
        let mut lets = Vec::new();
        for (i, ty) in binder_tys.iter().enumerate() {
            let ident = binders.as_ref().and_then(|bs| bs.get(i).cloned().flatten());
            let name = match ident {
                Some(ident) if ident.starts_with("#tuple:") => {
                    let items: Vec<&str> = ident["#tuple:".len()..].split(',').collect();
                    let (Ty::Tuple(ts) | Ty::Pair(ts)) = ty else {
                        return Break(Residue::Form("tuple pattern of another type".into()));
                    };
                    if ts.len() != items.len() {
                        return Break(Residue::Form("tuple pattern of another type".into()));
                    }
                    let (whole, after) = ctx.fresh(env, acc, "pair");
                    acc = after;
                    for (k, (item, t)) in items.iter().zip(ts).enumerate() {
                        if *item == "_" {
                            continue;
                        }
                        let (next, name, after) = scope.bind(env, acc, item, t.clone());
                        scope = next;
                        acc = after;
                        lets.push((name, t.render(), tuple_item(ty, &whole, k)));
                    }
                    whole
                }
                Some(ident) => {
                    let (next, name, after) = scope.bind(env, acc, &ident, ty.clone());
                    scope = next;
                    acc = after;
                    name
                }
                None => {
                    let (name, after) = ctx.fresh(env, acc, if i == 0 { "value" } else { "rest" });
                    acc = after;
                    name
                }
            };
            names.push(name);
        }
        if lex == "List.cons" && ctx.params.contains(&s) {
            scope.tails.insert(names[1].clone(), s.clone());
        }
        // A field of the scrutinee's own type is a sub-term: a recursive call
        // on it descends.
        if ctx.params.contains(&s) && types_has_data(&st, &env.types) {
            for (name, ty) in names.iter().zip(binder_tys) {
                if ty.same(&st) {
                    scope.tails.insert(name.clone(), s.clone());
                }
            }
        }
        let ((e, t), after) = body(b, env, &scope, acc, result.as_ref())?;
        acc = after;
        let e = wrap_lets(lets, e);
        match &result {
            Some(r) if !r.same(&t) => {
                return Break(Residue::Form("arms of different types".to_string()));
            }
            _ => result = Some(t),
        }
        let pattern = std::iter::once(lex.clone())
            .chain(names)
            .collect::<Vec<_>>()
            .join(" ");
        out.push(Arm { pattern, body: e });
    }
    let Some(result) = result else {
        return Break(Residue::Form("empty match".to_string()));
    };
    Continue((
        (
            Expr::Match {
                scrutinee: s,
                arms: out,
            },
            result,
        ),
        acc,
    ))
}

/// A match on literals (integers, strings, characters and their ranges), or
/// with guards: a chain of `if`s ending in an arm that takes every value. An
/// arm may bind the scrutinee by name, and its guard joins its test.
fn literal_match(
    s: &str,
    st: &Ty,
    arms: &[(&rs::Pat, Body<'_>)],
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let bad = || Break(Residue::Form("match pattern".to_string()));
    let constant = |e: &rs::RustExpr| -> Option<String> {
        match e {
            rs::RustExpr::Lit(l) => match literal_of(l, Some(st), false) {
                Continue((Expr::Flat(k), kt)) if kt.same(st) => Some(k),
                _ => None,
            },
            rs::RustExpr::Unary(u) if matches!(u.op, rs::UnOp::Neg) => match &*u.expr {
                rs::RustExpr::Lit(l) => match literal_of(l, Some(st), true) {
                    Continue((Expr::Flat(k), kt)) if kt.same(st) => Some(k),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    };
    let equal = |k: &str| match st {
        Ty::Nat(_) => Some(format!("({s} =? {k})")),
        Ty::Int(_) | Ty::Str | Ty::Char => Some(format!("(prim equal {s} {k} : Bool)")),
        Ty::Bool => Some(format!("({s} =? {k})")),
        _ => None,
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
    let mut acc = acc;
    // Each arm: its test (`None` takes every value), the name it binds the
    // scrutinee to, and its body.
    let mut tests: Vec<(Option<String>, Option<String>, Body<'_>)> = Vec::new();
    for (pat, b) in arms {
        let cases: Vec<&rs::Pat> = match pat {
            rs::Pat::Or(o) => o.iter().collect(),
            p => vec![p],
        };
        let mut conds = Vec::new();
        let mut all = false;
        let mut bound = None;
        for case in cases {
            match case {
                rs::Pat::Wild => all = true,
                rs::Pat::Ident(i) if i.subpat.is_none() && !i.by_ref => {
                    let id = i.ident.to_string();
                    if id == "None" || id.starts_with(char::is_uppercase) {
                        return bad();
                    }
                    all = true;
                    bound = Some(id);
                }
                rs::Pat::Lit(l) => {
                    let lit = rs::RustExpr::Lit(l.clone());
                    let Some(k) = constant(&lit) else {
                        return bad();
                    };
                    let Some(c) = equal(&k) else { return bad() };
                    conds.push(c);
                }
                rs::Pat::Range(rg) => {
                    let lo = match &rg.start {
                        Some(e) => match constant(e) {
                            Some(k) => Some(k),
                            None => return bad(),
                        },
                        None => None,
                    };
                    let hi = match &rg.end {
                        Some(e) => match constant(e) {
                            Some(k) => Some(k),
                            None => return bad(),
                        },
                        None => None,
                    };
                    let mut parts = Vec::new();
                    if let Some(lo) = lo {
                        let Some(c) = at_most(&lo, s) else {
                            return bad();
                        };
                        acc = ordered(acc);
                        parts.push(c);
                    }
                    if let Some(hi) = hi {
                        let c = if rg.closed {
                            at_most(s, &hi)
                        } else {
                            at_most(&hi, s).map(|c| format!("(not {c})"))
                        };
                        let Some(c) = c else { return bad() };
                        acc = ordered(acc);
                        parts.push(c);
                    }
                    match parts.as_slice() {
                        [] => all = true,
                        [one] => conds.push(one.clone()),
                        [a, b] => conds.push(format!("({a} && {b})")),
                        _ => return bad(),
                    }
                }
                _ => return bad(),
            }
        }
        if bound.is_some() && !conds.is_empty() {
            return bad();
        }
        let mut cond = if all {
            None
        } else {
            conds.into_iter().reduce(|a, b| format!("({a} || {b})"))
        };
        let mut scope = ctx.clone();
        if let Some(id) = &bound {
            scope.locals.insert(id.clone(), (s.to_string(), st.clone()));
        }
        if let Body::Guarded(g, _) = b {
            let ((gv, gt), after) = flat_of(g, env, &scope.below(), acc, Some(&Ty::Bool))?;
            acc = after;
            if gt != Ty::Bool {
                return bad();
            }
            cond = Some(match cond {
                Some(c) => format!("({c} && {gv})"),
                None => gv,
            });
        }
        let last = cond.is_none();
        tests.push((cond, bound, *b));
        if last {
            break;
        }
    }
    if tests.last().is_none_or(|(c, _, _)| c.is_some()) {
        return Break(Residue::Form("non-exhaustive match".to_string()));
    }
    let mut result = want.cloned();
    let mut bodies = Vec::new();
    for (cond, bound, b) in &tests {
        let mut scope = ctx.clone();
        if let Some(id) = bound {
            scope.locals.insert(id.clone(), (s.to_string(), st.clone()));
        }
        let ((e, t), after) = body(*b, env, &scope, acc, result.as_ref())?;
        acc = after;
        if result.as_ref().is_some_and(|r| !r.same(&t)) {
            return Break(Residue::Form("arms of different types".to_string()));
        }
        result = Some(t);
        bodies.push((cond.clone(), e));
    }
    let Some(t) = result else {
        return Break(Residue::Form("empty match".to_string()));
    };
    let mut bodies = bodies.into_iter().rev();
    let Some((_, mut out)) = bodies.next() else {
        return Break(Residue::Form("empty match".to_string()));
    };
    for (cond, e) in bodies {
        let Some(cond) = cond else { return bad() };
        out = Expr::If {
            condition: Box::new(Expr::Flat(cond)),
            then: Box::new(e),
            otherwise: Box::new(out),
        };
    }
    Continue(((out, t), acc))
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
        rs::RustExpr::Assign(a) => ident(&a.left),
        rs::RustExpr::Binary(b) if b.op == rs::BinOp::AddAssign => ident(&b.left),
        rs::RustExpr::MethodCall(m)
            if (m.method == "push" || m.method == "push_str") && m.args.len() == 1 =>
        {
            ident(&m.receiver)
        }
        // The value of `insert` (whether it was new, or the replaced value)
        // is dropped where it is a statement of a loop.
        rs::RustExpr::MethodCall(m) if m.method == "insert" && (1..=2).contains(&m.args.len()) => {
            ident(&m.receiver)
        }
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
        Continue((
            format!("(prim join (cons {current} (cons {piece} (nil String))) \"\" : String)"),
            acc,
        ))
    };
    let value_of = |v: &rs::RustExpr, want: &Ty, acc: Acc| -> Lift<(String, Acc)> {
        let ((x, xt), acc) = flat_of(v, env, &inner, acc, Some(want))?;
        if xt.same(want) {
            Continue((x, acc))
        } else {
            Break(Residue::Form("an update of another type".to_string()))
        }
    };
    match (e, ty) {
        (rs::RustExpr::Assign(a), _) => value_of(&a.right, ty, acc),
        (rs::RustExpr::Binary(b), Ty::Str) => {
            let (piece, acc) = value_of(&b.right, &Ty::Str, acc)?;
            joined(piece, acc)
        }
        (rs::RustExpr::Binary(b), _) => Break(Residue::Arithmetic(b.op.text().to_string())),
        (rs::RustExpr::MethodCall(m), Ty::Set(t, true))
            if m.method == "insert" && m.args.len() == 1 =>
        {
            let (v, acc) = value_of(&m.args[0], t, acc)?;
            let (helper, acc) = sorted_insert(ty, env, ctx, acc)?;
            Continue((format!("({helper} {current} {v})"), acc))
        }
        (rs::RustExpr::MethodCall(m), Ty::Map(k, v, true))
            if m.method == "insert" && m.args.len() == 2 =>
        {
            let (key, acc) = value_of(&m.args[0], k, acc)?;
            let (value, acc) = value_of(&m.args[1], v, acc)?;
            let (helper, acc) = sorted_insert(ty, env, ctx, acc)?;
            let entry = ty.entry();
            Continue((
                format!(
                    "({helper} {current} {})",
                    tuple_value(&entry, &[key, value])
                ),
                acc,
            ))
        }
        (rs::RustExpr::MethodCall(m), Ty::List(t)) if m.method == "push" => {
            let (v, acc) = value_of(&m.args[0], t, acc)?;
            Continue((
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
        _ => Break(Residue::Form("an update outside the lift".to_string())),
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
        _ => return Break(Residue::Form("insert into an unordered collection".into())),
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
    let (same, less) = match key {
        Ty::Nat(_) => (format!("({k} =? {h})"), format!("({k} <? {h})")),
        Ty::Str | Ty::Char => {
            axioms.insert("propext".to_string());
            axioms.extend(ALL_AXIOMS.map(String::from));
            (
                format!("(prim equal {k} {h} : Bool)"),
                format!("(prim less_than {k} {h} : Bool)"),
            )
        }
        other => {
            return Break(Residue::Form(format!(
                "insert by a key of {}, whose order the lift does not model",
                other.render()
            )))
        }
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
    });
    let acc = acc.called(format!("#{name}"));
    Continue((name, acc))
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
            rs::Stmt::Other => (BTreeSet::new(), BTreeSet::new()),
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
    if updated.iter().any(|u| declared.contains(u)) {
        return Break(Residue::Form("a loop that shadows its state".to_string()));
    }
    let state: Vec<String> = updated
        .into_iter()
        .filter(|u| ctx.locals.contains_key(u))
        .collect();
    if state.is_empty() {
        return Break(Residue::Form("a loop that updates no local".to_string()));
    }
    Continue(state)
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

/// The value of a `continue` or `break` in a loop step: the loop's state as
/// the step leaves it.
fn loop_exit(env: &Env, ctx: &Ctx, acc: Acc, label: bool, breaks: bool) -> Lift<((Expr, Ty), Acc)> {
    let Some(state) = &ctx.looping else {
        return Break(Residue::Form(
            "continue or break outside a loop step".to_string(),
        ));
    };
    if label {
        return Break(Residue::Form("labelled continue or break".to_string()));
    }
    let ((v, t), acc) = flat_of(&state_expr(state), env, ctx, acc, None)?;
    let v = if breaks { format!("{BREAK}{v}") } else { v };
    Continue(((Expr::Flat(v), t), acc))
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
        return Break(Residue::Form("labelled loop".to_string()));
    }
    let ((list, lt), acc) = flat_of(&f.expr, env, &ctx.below(), acc, None)?;
    let elem = match &lt {
        Ty::List(t) => (**t).clone(),
        Ty::Set(t, true) => (**t).clone(),
        Ty::Map(_, _, true) => lt.entry(),
        _ => {
            return Break(Residue::Form(
                "a loop over something other than a list".to_string(),
            ))
        }
    };
    let state = loop_state(&f.body.stmts, ctx)?;
    let tys: Vec<Ty> = state.iter().map(|v| ctx.locals[v].1.clone()).collect();
    let state_ty = match tys.as_slice() {
        [one] => one.clone(),
        many => Ty::Tuple(many.to_vec()),
    };
    let acc = acc.saw(&state_ty);
    // The step, in a scope of its own.
    let (acc, before) = acc.apart();
    let mut base = Ctx {
        tails: BTreeMap::new(),
        top: false,
        looping: Some(state.clone()),
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
    let (scope, head, acc, pat_lets) =
        bind_pat(&f.pat, &elem, env, &scope, acc, Vec::new(), false)?;
    let mut stmts = f.body.stmts.clone();
    stmts.push(rs::Stmt::Expr(state_expr(&state), false));
    let ((step, st), acc) = block(&stmts, env, &scope, acc, Some(&state_ty))?;
    let (acc, own) = acc.rejoin(before);
    if !st.same(&state_ty) || !own.descends.is_empty() {
        return Break(Residue::Form("a loop step of another type".to_string()));
    }
    let name = ctx.helper_name(env, &acc, "Loop");
    let (items, acc) = ctx.fresh(env, acc, "items");
    let (rest_name, acc) = ctx.fresh(env, acc, "rest");
    let text = step.flat();
    let caps = captures(ctx, &[text], std::slice::from_ref(&current));
    let cap_names: Vec<String> = caps.iter().map(|(c, _)| c.clone()).collect();
    let recur = |s: String| {
        if let Some(last) = s.strip_prefix(BREAK) {
            return last.to_string();
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
        result: state_ty.render(),
        body: Expr::Match {
            scrutinee: items,
            arms: vec![
                Arm {
                    pattern: "List.nil".to_string(),
                    body: Expr::Flat(current),
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
    });
    let acc = acc.called(format!("#{name}"));
    // The loop's value, then the rest with the final state.
    let ((init, _), acc) = flat_of(&state_expr(&state), env, &ctx.below(), acc, Some(&state_ty))?;
    let mut parts = vec![name, list, init];
    parts.extend(cap_names);
    let (whole, acc) = ctx.fresh(env, acc, "state");
    let (after, acc, lets) = rebind(env, ctx, acc, &state, &whole, Vec::new());
    let ((rest, rt), acc) = block(rest, env, &after, acc, want)?;
    Continue((
        (
            Expr::Let {
                name: whole,
                ty: state_ty.render(),
                value: Box::new(Expr::Flat(format!("({})", parts.join(" ")))),
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
    let boxed = |x: rs::RustExpr| Box::new(x);
    match e {
        E::Try(t) => Some(match take_try(t, var) {
            Some((found, inner)) => (found, E::Try(boxed(inner))),
            None => ((**t).clone(), E::Path(rs::Path::ident(var))),
        }),
        E::MethodCall(m) => {
            if let Some((found, receiver)) = take_try(&m.receiver, var) {
                let mut m = m.clone();
                m.receiver = boxed(receiver);
                return Some((found, E::MethodCall(m)));
            }
            let (found, args) = take_first(&m.args, var)?;
            let mut m = m.clone();
            m.args = args;
            Some((found, E::MethodCall(m)))
        }
        E::Call(c) => {
            let (found, args) = take_first(&c.args, var)?;
            let mut c = c.clone();
            c.args = args;
            Some((found, E::Call(c)))
        }
        E::Field(f) => {
            let (found, base) = take_try(&f.base, var)?;
            let mut f = f.clone();
            f.base = boxed(base);
            Some((found, E::Field(f)))
        }
        E::Paren(p) => {
            let (found, inner) = take_try(p, var)?;
            Some((found, E::Paren(boxed(inner))))
        }
        E::Reference(r) => {
            let (found, inner) = take_try(&r.expr, var)?;
            let mut r = r.clone();
            r.expr = boxed(inner);
            Some((found, E::Reference(r)))
        }
        E::Unary(u) => {
            let (found, inner) = take_try(&u.expr, var)?;
            let mut u = u.clone();
            u.expr = boxed(inner);
            Some((found, E::Unary(u)))
        }
        E::Cast(c) => {
            let (found, inner) = take_try(&c.expr, var)?;
            let mut c = c.clone();
            c.expr = boxed(inner);
            Some((found, E::Cast(c)))
        }
        E::Index(i) => {
            let mut i = i.clone();
            if let Some((found, inner)) = take_try(&i.expr, var) {
                i.expr = boxed(inner);
                return Some((found, E::Index(i)));
            }
            let (found, index) = take_try(&i.index, var)?;
            i.index = boxed(index);
            Some((found, E::Index(i)))
        }
        E::Binary(b) => {
            let mut b = b.clone();
            if let Some((found, left)) = take_try(&b.left, var) {
                b.left = boxed(left);
                return Some((found, E::Binary(b)));
            }
            if matches!(b.op, rs::BinOp::And | rs::BinOp::Or) {
                return None;
            }
            let (found, right) = take_try(&b.right, var)?;
            b.right = boxed(right);
            Some((found, E::Binary(b)))
        }
        E::Tuple(items) => {
            let (found, items) = take_first(items, var)?;
            Some((found, E::Tuple(items)))
        }
        E::Struct(st) => {
            let values: Vec<rs::RustExpr> = st.fields.iter().map(|f| f.expr.clone()).collect();
            let (found, values) = take_first(&values, var)?;
            let mut st = st.clone();
            for (field, value) in st.fields.iter_mut().zip(values) {
                field.expr = value;
            }
            Some((found, E::Struct(st)))
        }
        E::If(i) => {
            let (found, cond) = take_try(&i.cond, var)?;
            let mut i = i.clone();
            i.cond = boxed(cond);
            Some((found, E::If(i)))
        }
        E::Match(m) => {
            let (found, scrutinee) = take_try(&m.expr, var)?;
            let mut m = m.clone();
            m.expr = boxed(scrutinee);
            Some((found, E::Match(m)))
        }
        _ => None,
    }
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
        rs::Stmt::Other => return None,
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
    if ctx.top {
        if let Some((first, rest)) = stmts.split_first() {
            let n = acc.fresh + 1;
            if let Some((binding, stmt)) = hoist_try(first, n) {
                let acc = Acc { fresh: n, ..acc };
                let mut out = vec![binding, stmt];
                out.extend(rest.iter().cloned());
                return block(&out, env, ctx, acc, want);
            }
        }
    }
    match stmts {
        // A block with no value is `()`.
        [] if want == Some(&Ty::Unit) => Continue(((Expr::Flat("()".into()), Ty::Unit), acc)),
        [rs::Stmt::Expr(e, false)] => expr(e, env, &ctx.below(), acc, want),
        [rs::Stmt::Expr(rs::RustExpr::Return(r), _), ..] if ctx.top => match r {
            Some(v) => expr(v, env, &ctx.below(), acc, Some(&ctx.result)),
            None if ctx.result == Ty::Unit => Continue(((Expr::Flat("()".into()), Ty::Unit), acc)),
            None => Break(Residue::Type("()".to_string())),
        },
        // `if c { return v; } rest`: an early exit from the function.
        [rs::Stmt::Expr(rs::RustExpr::If(i), _), rest @ ..]
            if ctx.top
                && !rest.is_empty()
                && i.else_branch.is_none()
                && returned(&i.then_branch).is_some() =>
        {
            let Some(v) = returned(&i.then_branch) else {
                return Break(Residue::Statements);
            };
            let inner = ctx.below();
            if matches!(&*i.cond, rs::RustExpr::Let(_)) {
                let wild = rs::Pat::Wild;
                let rs::RustExpr::Let(l) = &*i.cond else {
                    return Break(Residue::Statements);
                };
                let tail = rs::Block {
                    stmts: rest.to_vec(),
                };
                let early = rs::Block {
                    stmts: vec![rs::Stmt::Expr((*v).clone(), false)],
                };
                let arms = vec![(&l.pat, Body::Block(&early)), (&wild, Body::Block(&tail))];
                return matching(&l.expr, &arms, env, ctx, acc, want);
            }
            let ((c, ct), acc) = expr(&i.cond, env, &inner, acc, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Break(Residue::Form("non-boolean condition".to_string()));
            }
            let ((then, tt), acc) = expr(v, env, &inner, acc, Some(&ctx.result))?;
            let ((other, ot), acc) = block(rest, env, ctx, acc, want)?;
            if !tt.same(&ctx.result) || !ot.same(&tt) {
                return Break(Residue::Form("branches of different types".to_string()));
            }
            Continue((
                (
                    Expr::If {
                        condition: Box::new(Expr::Flat(flat_text(c)?)),
                        then: Box::new(then),
                        otherwise: Box::new(other),
                    },
                    tt,
                ),
                acc,
            ))
        }
        // `let P = e else { return v; }; rest`: a match with an early exit.
        [rs::Stmt::Local(local), rest @ ..]
            if ctx.top
                && !rest.is_empty()
                && local.init.as_ref().is_some_and(|i| i.diverge.is_some()) =>
        {
            let Some(init) = &local.init else {
                return Break(Residue::Statements);
            };
            let Some(otherwise) = &init.diverge else {
                return Break(Residue::Statements);
            };
            let rs::RustExpr::Block(eb) = &**otherwise else {
                return Break(Residue::Form("let else".to_string()));
            };
            let Some(v) = returned(&eb.block) else {
                return Break(Residue::Form("let else".to_string()));
            };
            let wild = rs::Pat::Wild;
            let tail = rs::Block {
                stmts: rest.to_vec(),
            };
            let early = rs::Block {
                stmts: vec![rs::Stmt::Expr((*v).clone(), false)],
            };
            let arms = vec![
                (&local.pat, Body::Block(&tail)),
                (&wild, Body::Block(&early)),
            ];
            matching(&init.expr, &arms, env, ctx, acc, want)
        }
        // `let (a, b) = e; rest`: the tuple, then its items.
        [rs::Stmt::Local(local), rest @ ..]
            if !rest.is_empty()
                && matches!(local.pat, rs::Pat::Tuple(_))
                && local.init.as_ref().is_some_and(|i| {
                    i.diverge.is_none() && !matches!(*i.expr, rs::RustExpr::Try(_))
                }) =>
        {
            let Some(init) = &local.init else {
                return Break(Residue::Statements);
            };
            let ((value, vt), acc) = flat_of(&init.expr, env, &ctx.below(), acc, None)?;
            let (scope, whole, acc, lets) =
                bind_pat(&local.pat, &vt, env, ctx, acc, Vec::new(), false)?;
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
            Continue((
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
        // `for p in xs { .. }`: a walk with the updated locals as its state.
        [rs::Stmt::Expr(rs::RustExpr::ForLoop(f), _), rest @ ..] if !rest.is_empty() => {
            for_loop(f, rest, env, ctx, acc, want)
        }
        // `x = e;`, `x += s;`, `x.push(e);`: the local's next value.
        [rs::Stmt::Expr(e, true), rest @ ..] if !rest.is_empty() && update_target(e).is_some() => {
            let Some(target) = update_target(e) else {
                return Break(Residue::Statements);
            };
            let Some((current, ty)) = ctx.locals.get(&target).cloned() else {
                return Break(Residue::Form(format!("an update of {target}")));
            };
            let (value, acc) = updated(e, &ty, &current, env, ctx, acc)?;
            let (scope, name, acc) = ctx.bind(env, acc, &target, ty.clone());
            let ((rest, rt), acc) = block(rest, env, &scope, acc, want)?;
            Continue((
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
        // `()` as a statement does nothing.
        [rs::Stmt::Expr(rs::RustExpr::Tuple(t), _), rest @ ..]
            if t.is_empty() && !rest.is_empty() =>
        {
            block(rest, env, ctx, acc, want)
        }
        // `match e { p => .. } rest`: the rest after each arm.
        [rs::Stmt::Expr(rs::RustExpr::Match(m), _), rest @ ..] if !rest.is_empty() => {
            let bodies: Vec<rs::RustExpr> =
                m.arms.iter().map(|a| then_rest(&a.body, rest)).collect();
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
        // `if let p = e { .. } [else { .. }] rest`: the rest after each branch.
        [rs::Stmt::Expr(rs::RustExpr::If(i), _), rest @ ..]
            if !rest.is_empty()
                && matches!(&*i.cond, rs::RustExpr::Let(_))
                && returned(&i.then_branch).is_none() =>
        {
            let rs::RustExpr::Let(l) = &*i.cond else {
                return Break(Residue::Statements);
            };
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
        // `if c { .. } [else { .. }] rest` with statements in its branches: the
        // rest after each branch.
        [rs::Stmt::Expr(rs::RustExpr::If(i), _), rest @ ..]
            if !rest.is_empty()
                && !matches!(&*i.cond, rs::RustExpr::Let(_))
                && returned(&i.then_branch).is_none() =>
        {
            let ((c, ct), acc) = expr(&i.cond, env, &ctx.below(), acc, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Break(Residue::Form("non-boolean condition".to_string()));
            }
            let mut then: Vec<rs::Stmt> = i.then_branch.stmts.clone();
            then.extend(rest.iter().cloned());
            let mut other: Vec<rs::Stmt> = match &i.else_branch {
                None => Vec::new(),
                Some(e) => match &**e {
                    rs::RustExpr::Block(b) => b.block.stmts.clone(),
                    rs::RustExpr::If(_) => vec![rs::Stmt::Expr((**e).clone(), false)],
                    _ => return Break(Residue::Statements),
                },
            };
            other.extend(rest.iter().cloned());
            let ((a, at), acc) = block(&then, env, ctx, acc, want)?;
            let ((b, bt), acc) = block(&other, env, ctx, acc, Some(want.unwrap_or(&at)))?;
            if !at.same(&bt) {
                return Break(Residue::Form("branches of different types".to_string()));
            }
            Continue((
                (
                    Expr::If {
                        condition: Box::new(Expr::Flat(flat_text(c)?)),
                        then: Box::new(a),
                        otherwise: Box::new(b),
                    },
                    at,
                ),
                acc,
            ))
        }
        [rs::Stmt::Local(local), rest @ ..] if !rest.is_empty() => {
            let (ident, declared) = match &local.pat {
                rs::Pat::Ident(i) if i.subpat.is_none() && !i.by_ref => (i, None),
                rs::Pat::Type(pt) => match &*pt.pat {
                    rs::Pat::Ident(i) if i.subpat.is_none() && !i.by_ref => {
                        (i, Some(env.types.ty(&pt.ty)?))
                    }
                    _ => return Break(Residue::Form("let pattern".to_string())),
                },
                _ => return Break(Residue::Form("let pattern".to_string())),
            };
            let Some(init) = &local.init else {
                return Break(Residue::Statements);
            };
            if init.diverge.is_some() {
                return Break(Residue::Form("let else".to_string()));
            }
            if let rs::RustExpr::Try(t) = &*init.expr {
                if !ctx.top {
                    return Break(Residue::Form(
                        "? below the function's own statements".to_string(),
                    ));
                }
                return question(t, &ident.ident, declared, rest, env, ctx, acc, want);
            }
            // An empty vector the block goes on to return takes the type the
            // block returns.
            let hint = match &declared {
                Some(d) => Some(d.clone()),
                None if is_empty_vec(&init.expr) => {
                    returned_type(&ident.ident.clone(), rest, ctx, want)
                }
                None => None,
            };
            let ((value, vt), acc) = flat_of(&init.expr, env, &ctx.below(), acc, hint.as_ref())?;
            if declared.as_ref().is_some_and(|d| !d.same(&vt)) {
                return Break(Residue::Form("let of another type".to_string()));
            }
            let (scope, name, acc) = ctx.bind(env, acc, &ident.ident.clone(), vt.clone());
            let ((rest, rt), acc) = block(rest, env, &scope, acc, want)?;
            Continue((
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
        _ => Break(Residue::Statements),
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

/// The type a local takes from where the block returns it: the block's own
/// type when its last statement is the local, or the function's result when
/// a `return` of the local appears.
fn returned_type(ident: &str, rest: &[rs::Stmt], ctx: &Ctx, want: Option<&Ty>) -> Option<Ty> {
    let is_local = |e: &rs::RustExpr| matches!(e, rs::RustExpr::Path(p) if p.is_ident(ident));
    if let Some(rs::Stmt::Expr(e, false)) = rest.last() {
        if is_local(e) {
            return want.cloned();
        }
    }
    returns_local(rest, ident).then(|| ctx.result.clone())
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
        rs::Stmt::Other => false,
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
        _ => return Break(Residue::Form("? of another type".to_string())),
    };
    if declared.as_ref().is_some_and(|d| !d.same(&inner)) {
        return Break(Residue::Form("let of another type".to_string()));
    }
    let (scope, name, acc) = ctx.bind(env, acc, ident, inner.clone());
    let ((rest, rt), acc) = block(rest, env, &scope, acc, want)?;
    let ok = if matches!(st, Ty::Option(_)) {
        format!("Option.some {name}")
    } else {
        format!("Result.ok {name}")
    };
    let arms = if matches!(st, Ty::Option(_)) {
        vec![
            Arm {
                pattern: early.0,
                body: Expr::Flat(early.1),
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
                body: Expr::Flat(early.1),
            },
        ]
    };
    Continue(((Expr::Match { scrutinee: s, arms }, rt), acc))
}

/// A candidate function: its LexLean name, signature and body.
pub(crate) struct Candidate<'a> {
    pub(crate) rust: String,
    pub(crate) name: String,
    pub(crate) params: Vec<Param>,
    pub(crate) result: Ty,
    pub(crate) block: &'a rs::Block,
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
pub(crate) fn lift_all(candidates: &[Candidate<'_>], types: &Types) -> Lifted {
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
        let env = Env {
            types: types.clone(),
            fns,
        };
        let mut decls = Vec::new();
        let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut mentioned: BTreeMap<String, BTreeSet<Ty>> = BTreeMap::new();
        let mut own: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut changed = false;
        for c in candidates {
            if failed.contains_key(&c.rust) {
                continue;
            }
            let names: Vec<String> = c.params.iter().map(|(_, n, _)| n.clone()).collect();
            let ctx = Ctx {
                current: c.rust.clone(),
                params: names.clone(),
                result: c.result.clone(),
                locals: c
                    .params
                    .iter()
                    .map(|(rust, n, t)| (rust.clone(), (n.clone(), t.clone())))
                    .collect(),
                tails: BTreeMap::new(),
                own: c.name.clone(),
                top: true,
                looping: None,
                hidden: Vec::new(),
            };
            let lifted = block(&c.block.stmts, &env, &ctx, Acc::default(), Some(&c.result));
            let outcome = match lifted {
                Continue(((body, ty), acc)) if ty.same(&c.result) => {
                    let descends = acc.descends.clone();
                    if descends.len() > 1 {
                        Break(Residue::Form(
                            "recursion on more than one parameter".to_string(),
                        ))
                    } else {
                        Continue((
                            body,
                            descends.into_iter().next().map(|i| names[i].clone()),
                            acc,
                        ))
                    }
                }
                Continue(_) => Break(Residue::Form("a body of another type".to_string())),
                Break(r) => Break(r),
            };
            let outcome = match outcome {
                Continue((body, Some(arg), acc)) => well_founded(c, &body, &arg, types)
                    .map_continue(|(recursive, termination, theorems)| {
                        (body, recursive, termination, theorems, acc)
                    }),
                Continue((body, None, acc)) => Continue((body, None, None, Vec::new(), acc)),
                Break(r) => Break(r),
            };
            match outcome {
                Continue((body, recursive, termination, theorems, acc)) => {
                    // A well-founded definition elaborates with its evidence.
                    let mut evidence_axioms = BTreeSet::new();
                    for t in theorems {
                        if let Decl::Theorem { axioms, .. } = &t {
                            evidence_axioms.extend(axioms.iter().cloned());
                        }
                        decls.push((format!("#{}", t.name()), t));
                    }
                    graph.insert(c.rust.clone(), acc.calls);
                    let mut seen = acc.seen;
                    for (_, _, t) in &c.params {
                        t.mentions(&mut seen);
                    }
                    c.result.mentions(&mut seen);
                    mentioned.insert(c.rust.clone(), seen);
                    let mut axioms = acc.axioms;
                    axioms.extend(evidence_axioms);
                    own.insert(c.rust.clone(), axioms);
                    for h in acc.helpers {
                        let key = format!("#{}", h.name);
                        graph.insert(key.clone(), h.calls);
                        own.insert(key.clone(), h.axioms);
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
                                termination: None,
                                mutual: None,
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
                            mutual: None,
                        },
                    ));
                }
                Break(reason) => {
                    failed.insert(c.rust.clone(), reason);
                    changed = true;
                }
            }
        }
        for rust in cyclic(&graph) {
            failed.insert(rust, Residue::Form("mutual recursion".to_string()));
            changed = true;
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
            for (key, d) in &mut decls {
                if let Decl::Definition { axioms: a, .. } = d {
                    a.clone_from(&axioms[key.as_str()]);
                }
            }
            axioms.retain(|key, _| !key.starts_with('#'));
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
                        text.contains(&format!("{PRELUDE}::"))
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
                        callees
                            .iter()
                            .all(|c| placed.contains(c) || c == &key || !graph.contains_key(c))
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
                decls: ordered,
                failed,
                types,
                axioms,
                opaque,
            };
        }
    }
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
    let structural = || Continue((Some(arg.to_string()), None, Vec::new()));
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
        Break(Residue::Form(format!(
            "recursion over the nested type {n} that LexLean cannot measure"
        )))
    };
    if !types.sizable(&family) {
        return refused();
    }
    let size = format!("size_of_{n}");
    let binder_types = |ctor: &str| -> Option<Vec<String>> {
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
    };
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
    let mut definitions: Vec<String> = Vec::new();
    for m in &family {
        definitions.push(format!("size_of_{m}"));
        definitions.push(format!("size_of_list_{m}"));
        definitions.push(format!("size_of_option_{m}"));
        if let Some(fields) = types.structs.get(m).filter(|_| types.boxed(m)) {
            definitions.extend(fields.iter().map(|(f, _)| Types::accessor(m, f)));
        }
    }
    let used: BTreeSet<String> = obligations
        .iter()
        .flat_map(|(_, s)| {
            s.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect();
    definitions.retain(|d| used.contains(d));
    definitions.sort();
    let mut theorems = Vec::new();
    let mut evidence = Vec::new();
    for (i, (parameters, statement)) in obligations.into_iter().enumerate() {
        let name = format!("decreases_{}_{i}", c.name);
        evidence.push(name.clone());
        theorems.push(Decl::Theorem {
            name,
            parameters,
            statement,
            proof: format!(
                "linear_arithmetic{{definitions: [{}]}}",
                definitions.join(", ")
            ),
            // What omega's elaboration depends on.
            axioms: vec!["Quot.sound".to_string(), "propext".to_string()],
        });
    }
    Continue((None, Some((format!("({size} {arg})"), evidence)), theorems))
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
            Continue((Expr::Flat(s), t)) if t.same(want) => Some(s),
            _ => None,
        },
        rs::RustExpr::Unary(u) if matches!(u.op, rs::UnOp::Neg) => match &*u.expr {
            rs::RustExpr::Lit(lit) => match literal_of(lit, Some(want), true) {
                Continue((Expr::Flat(s), t)) if t.same(want) => Some(s),
                _ => None,
            },
            _ => None,
        },
        rs::RustExpr::Path(p) => {
            let (owner, variant) = ctor(p, types)?;
            (Ty::Named(owner.clone()) == *want).then(|| format!("(ctor {owner}.{variant})"))
        }
        rs::RustExpr::Paren(p) => literal(p, want, types),
        rs::RustExpr::Reference(r) => literal(&r.expr, want, types),
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
            if !matches!(result, Ty::Bool | Ty::Nat(_)) {
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
        let (params, result) = match types.signature(&sig, None) {
            Continue(sig) => sig,
            Break(r) => return Err(r),
        };
        let c = Candidate {
            rust: sig.ident.clone(),
            name: camel(&[&sig.ident]),
            params,
            result,
            block: &block,
        };
        let out = lift_all(&[c], types);
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

    fn plain() -> Types {
        Types::default()
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
        assert_eq!(
            lift(
                "fn t(text: &str) -> String { text.to_lowercase() }",
                &plain()
            ),
            Err(Residue::Form("method call .to_lowercase".to_string()))
        );
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
