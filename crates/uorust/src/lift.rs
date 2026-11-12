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

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow::{self, Break, Continue};

/// A lift step: the value, or the residue that stops the lift.
pub(crate) type Lift<T> = ControlFlow<Residue, T>;

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
pub(crate) type RawVariants = Vec<(String, Vec<(Option<String>, syn::Type)>)>;

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
        structs: &BTreeMap<String, Vec<(String, syn::Type)>>,
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
        let mut struct_names: BTreeSet<String> = structs.keys().cloned().collect();
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
        structs: &BTreeMap<String, Vec<(String, syn::Type)>>,
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

    pub(crate) fn ty(&self, t: &syn::Type) -> Lift<Ty> {
        let text = crate::ingest::tidy(&quote::ToTokens::to_token_stream(t).to_string());
        let bad = || Break(Residue::Type(text.clone()));
        match t {
            syn::Type::Reference(r) if r.mutability.is_none() => match &*r.elem {
                syn::Type::Path(p) if p.path.is_ident("str") => Continue(Ty::Str),
                syn::Type::Slice(s) => Continue(Ty::List(Box::new(self.ty(&s.elem)?))),
                inner => self.ty(inner),
            },
            syn::Type::Paren(p) => self.ty(&p.elem),
            syn::Type::Group(g) => self.ty(&g.elem),
            syn::Type::Tuple(tp) if tp.elems.len() >= 2 => {
                let mut ts = Vec::new();
                for e in &tp.elems {
                    ts.push(self.ty(e)?);
                }
                Continue(self.tuple(ts))
            }
            syn::Type::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
                let seg = &p.path.segments[0];
                let name = seg.ident.to_string();
                let mut args = Vec::new();
                match &seg.arguments {
                    syn::PathArguments::None => {}
                    syn::PathArguments::AngleBracketed(a) => {
                        for arg in &a.args {
                            let syn::GenericArgument::Type(t) = arg else {
                                return bad();
                            };
                            args.push(self.ty(t)?);
                        }
                    }
                    syn::PathArguments::Parenthesized(_) => return bad(),
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
        sig: &syn::Signature,
        owner: Option<&str>,
    ) -> Lift<(Vec<Param>, Ty)> {
        // A lifetime parameter says how long a borrow lives, which a value
        // of LexLean does not have: it is erased. A type or const parameter
        // is not.
        let generic = sig
            .generics
            .params
            .iter()
            .any(|p| !matches!(p, syn::GenericParam::Lifetime(_)));
        if generic
            || sig.asyncness.is_some()
            || matches!(sig.safety, syn::Safety::Unsafe(_))
            || sig.variadic.is_some()
        {
            return Break(Residue::Signature(
                "generic, async, unsafe or variadic".to_string(),
            ));
        }
        let mut params = Vec::new();
        for input in &sig.inputs {
            match input {
                syn::FnArg::Receiver(r) => {
                    let plain = r.mutability.is_none()
                        && matches!(
                            r.kind,
                            syn::ReceiverKind::Value | syn::ReceiverKind::Reference(_, _, None)
                        );
                    match owner {
                        Some(o)
                            if plain
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
                    }
                }
                syn::FnArg::Typed(pat) => {
                    let syn::Pat::Ident(name) = &*pat.pat else {
                        return Break(Residue::Signature("pattern parameter".to_string()));
                    };
                    let rust = name.ident.to_string();
                    params.push((rust.clone(), local_name(&rust), self.ty_in(&pat.ty, owner)?));
                }
            }
        }
        let result = match &sig.output {
            syn::ReturnType::Default => return Break(Residue::Type("()".to_string())),
            syn::ReturnType::Type(_, t) => self.ty_in(t, owner)?,
        };
        Continue((params, result))
    }

    /// A type where `Self` names the method's owner.
    fn ty_in(&self, t: &syn::Type, owner: Option<&str>) -> Lift<Ty> {
        let plain = match t {
            syn::Type::Reference(r) if r.mutability.is_none() => &*r.elem,
            other => other,
        };
        match (plain, owner) {
            (syn::Type::Path(p), Some(o)) if p.qself.is_none() && p.path.is_ident("Self") => {
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

#[derive(Clone)]
struct Ctx<'a> {
    types: &'a Types,
    /// Candidate functions: Rust name -> signature.
    fns: &'a BTreeMap<String, Sig>,
    /// The Rust name of the function being lifted.
    current: &'a str,
    /// Its parameters' LexLean names, in order.
    params: &'a [String],
    /// Its result type.
    result: &'a Ty,
    /// Rust local -> (LexLean name, type).
    locals: BTreeMap<String, (String, Ty)>,
    /// LexLean local bound to the tail of a slice parameter -> that parameter.
    tails: BTreeMap<String, String>,
    /// The parameter positions recursive calls descend on.
    descends: &'a RefCell<BTreeSet<usize>>,
    /// The other lifted functions the body calls.
    calls: &'a RefCell<BTreeSet<String>>,
    /// Names already bound, for fresh binders.
    fresh: &'a RefCell<usize>,
    /// Every type an expression of the body has.
    seen: &'a RefCell<BTreeSet<Ty>>,
    /// The Lean axioms the body's primitives elaborate with.
    axioms: &'a RefCell<BTreeSet<String>>,
    /// The LexLean name of the function being lifted, which its helpers extend.
    own: &'a str,
    /// The list helpers the body's iterator chains lift to.
    helpers: &'a RefCell<Vec<Helper>>,
    /// The statements in hand are the function's own, in tail position, so
    /// an early `return` leaves the function.
    top: bool,
    /// The locals of the loop whose step the statements in hand are: a
    /// `continue` there is the step's value, a `break` its last.
    looping: Option<Vec<String>>,
    /// Binders still in scope whose Rust local a later binding renamed.
    hidden: Vec<String>,
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

impl Ctx<'_> {
    /// A fresh name for a helper of the function being lifted.
    fn helper_name(&self, kind: &str) -> String {
        let helpers = self.helpers.borrow();
        let mut n = helpers.len() + 1;
        loop {
            let candidate = format!("{}{kind}{n}", self.own);
            if !self.fns.values().any(|f| f.name == candidate)
                && !helpers.iter().any(|h| h.name == candidate)
                && !self.taken(&candidate)
            {
                return candidate;
            }
            n += 1;
        }
    }

    fn taken(&self, name: &str) -> bool {
        self.locals.values().any(|(n, _)| n == name)
            || self.hidden.iter().any(|n| n == name)
            || self.fns.values().any(|f| f.name == name)
    }

    /// A LexLean binder for a Rust name: a binder may not shadow another.
    fn bind(&self, ident: &str, ty: Ty) -> (Self, String) {
        let mut name = local_name(ident);
        if self.taken(&name) {
            name = self.fresh(&name);
        }
        let mut next = self.clone();
        if let Some((old, _)) = next.locals.insert(ident.to_string(), (name.clone(), ty)) {
            next.hidden.push(old);
        }
        (next, name)
    }

    fn fresh(&self, base: &str) -> String {
        loop {
            let name = {
                let mut n = self.fresh.borrow_mut();
                *n += 1;
                format!("{base}{n}")
            };
            if !self.taken(&name) {
                return name;
            }
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

fn kind_of(e: &syn::Expr) -> String {
    let text = format!("{e:?}");
    text.split(['(', ' ', '{'])
        .next()
        .unwrap_or("expression")
        .to_lowercase()
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
fn flat_of(e: &syn::Expr, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(String, Ty)> {
    let calls = RefCell::new(BTreeSet::new());
    let axioms = RefCell::new(BTreeSet::new());
    let descends = RefCell::new(BTreeSet::new());
    let scope = Ctx {
        calls: &calls,
        axioms: &axioms,
        descends: &descends,
        top: false,
        looping: None,
        hidden: Vec::new(),
        ..ctx.clone()
    };
    let (x, t) = expr(e, &scope, want)?;
    drop(scope);
    let (calls, axioms, descends) = (
        calls.into_inner(),
        axioms.into_inner(),
        descends.into_inner(),
    );
    if let Continue(text) = flat_text(x.clone()) {
        ctx.calls.borrow_mut().extend(calls);
        ctx.axioms.borrow_mut().extend(axioms);
        ctx.descends.borrow_mut().extend(descends);
        return Continue((text, t));
    }
    if !descends.is_empty() {
        return Break(Residue::Form(
            "recursion inside a match or let in an operand".to_string(),
        ));
    }
    let caps = captures(ctx, &[x.flat()], &[]);
    let name = ctx.helper_name("Part");
    let mut parts = vec![name.clone()];
    parts.extend(caps.iter().map(|(c, _)| c.clone()));
    ctx.helpers.borrow_mut().push(Helper {
        name: name.clone(),
        params: caps
            .iter()
            .map(|(c, ty)| (c.clone(), ty.render()))
            .collect(),
        result: t.render(),
        body: x,
        calls,
        axioms,
        recursive: false,
    });
    ctx.calls.borrow_mut().insert(format!("#{name}"));
    Continue((format!("({})", parts.join(" ")), t))
}

fn int_literal(e: &syn::Expr) -> Option<u128> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(i),
            ..
        }) => i.base10_parse::<u128>().ok(),
        syn::Expr::Paren(p) => int_literal(&p.expr),
        _ => None,
    }
}

fn is_literal(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::Lit(_) => true,
        syn::Expr::Unary(u) => matches!(u.op, syn::UnOp::Neg(_)) && is_literal(&u.expr),
        syn::Expr::Paren(p) => is_literal(&p.expr),
        _ => false,
    }
}

/// A LexLean string literal holding exactly `text`, control characters
/// escaped.
fn exact_string(text: &str) -> Option<String> {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    Some(out)
}

fn literal_of(lit: &syn::Lit, want: Option<&Ty>, negative: bool) -> Lift<(Expr, Ty)> {
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    match lit {
        syn::Lit::Bool(b) if !negative => flat(b.value.to_string(), Ty::Bool),
        syn::Lit::Int(i) => {
            let Ok(v) = i.base10_parse::<u128>() else {
                return Break(Residue::Form("integer literal".to_string()));
            };
            let ty = match i.suffix() {
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
        syn::Lit::Str(s) if !negative => match exact_string(&s.value()) {
            Some(text) => flat(text, Ty::Str),
            None => Break(Residue::Form("string with control characters".to_string())),
        },
        syn::Lit::Char(c) if !negative => match exact_string(&c.value().to_string()) {
            Some(text) => flat(text, Ty::Char),
            None => Break(Residue::Form("string with control characters".to_string())),
        },
        syn::Lit::Byte(b) if !negative => flat(b.value().to_string(), Ty::Nat(Some(8))),
        syn::Lit::ByteStr(b) if !negative => {
            let mut list = "(nil Nat)".to_string();
            for byte in b.value().iter().rev() {
                list = format!("(cons {byte} {list})");
            }
            flat(list, Ty::List(Box::new(Ty::Nat(Some(8)))))
        }
        _ => Break(Residue::Form("literal".to_string())),
    }
}

fn ctor(path: &syn::Path, types: &Types) -> Option<(String, String)> {
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
    path: &syn::Path,
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
fn wrapper(path: &syn::Path) -> Option<&'static str> {
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

fn expr(e: &syn::Expr, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    let (x, t) = expr_inner(e, ctx, want)?;
    t.mentions(&mut ctx.seen.borrow_mut());
    Continue((x, t))
}

fn expr_inner(e: &syn::Expr, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    match e {
        syn::Expr::Lit(lit) => literal_of(&lit.lit, want, false),
        syn::Expr::Paren(p) => expr(&p.expr, ctx, want),
        syn::Expr::Group(g) => expr(&g.expr, ctx, want),
        syn::Expr::Block(b) if b.label.is_none() => block(&b.block.stmts, ctx, want),
        syn::Expr::Reference(r) if r.mutability.is_none() => expr(&r.expr, ctx, want),
        syn::Expr::Path(p) => {
            if let Some(ident) = p.path.get_ident() {
                if let Some((name, ty)) = ctx.locals.get(&ident.to_string()) {
                    return flat(name.clone(), ty.clone());
                }
            }
            // A constant: a definition of no parameters.
            if let Some(ident) = p.path.get_ident() {
                let key = ident.to_string();
                if let Some(sig) = ctx.fns.get(&key).filter(|s| s.params.is_empty()) {
                    if key == ctx.current {
                        return Break(Residue::Form("a constant that names itself".to_string()));
                    }
                    ctx.calls.borrow_mut().insert(key);
                    return flat(format!("({})", sig.name), sig.result.clone());
                }
            }
            if wrapper(&p.path) == Some("None") {
                return match want {
                    Some(t @ Ty::Option(inner)) => flat(
                        format!("(ctor Option.none [{}])", inner.render()),
                        t.clone(),
                    ),
                    _ => Break(Residue::Form("None of an unknown type".to_string())),
                };
            }
            match ctor(&p.path, ctx.types) {
                Some((owner, variant)) => {
                    flat(format!("(ctor {owner}.{variant})"), Ty::Named(owner))
                }
                None => Break(Residue::Form(format!(
                    "free name {}",
                    crate::ingest::tidy(&quote::ToTokens::to_token_stream(&p.path).to_string())
                ))),
            }
        }
        syn::Expr::Unary(u) => match u.op {
            syn::UnOp::Not(_) => {
                let (x, t) = flat_of(&u.expr, ctx, Some(&Ty::Bool))?;
                if t != Ty::Bool {
                    return Break(Residue::Form("bitwise not".to_string()));
                }
                flat(format!("(not {x})"), Ty::Bool)
            }
            syn::UnOp::Neg(_) => match &*u.expr {
                syn::Expr::Lit(lit) => literal_of(&lit.lit, want, true),
                _ => Break(Residue::Arithmetic("-".to_string())),
            },
            syn::UnOp::Deref(_) => expr(&u.expr, ctx, want),
            _ => Break(Residue::Form("unary operator".to_string())),
        },
        syn::Expr::Binary(b) => binary(b, ctx),
        syn::Expr::If(i) => {
            let Some((_, otherwise)) = &i.else_branch else {
                return Break(Residue::Form("if without else".to_string()));
            };
            if let syn::Expr::Let(l) = &*i.cond {
                let wild = syn::Pat::Wild(syn::PatWild {
                    attrs: Vec::new(),
                    underscore_token: syn::token::Underscore::default(),
                });
                let arms = vec![
                    (&*l.pat, Body::Block(&i.then_branch)),
                    (&wild, Body::Expr(otherwise)),
                ];
                return matching(&l.expr, &arms, ctx, want);
            }
            let (c, ct) = expr(&i.cond, ctx, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Break(Residue::Form("non-boolean condition".to_string()));
            }
            let (then, tt) = block(&i.then_branch.stmts, ctx, want)?;
            let (other, ot) = expr(otherwise, ctx, Some(want.unwrap_or(&tt)))?;
            if !tt.same(&ot) {
                return Break(Residue::Form("branches of different types".to_string()));
            }
            Continue((
                Expr::If {
                    condition: Box::new(Expr::Flat(flat_text(c)?)),
                    then: Box::new(then),
                    otherwise: Box::new(other),
                },
                tt,
            ))
        }
        syn::Expr::Match(m) => {
            let arms: Vec<(&syn::Pat, Body<'_>)> = m
                .arms
                .iter()
                .map(|a| match &a.pat {
                    syn::Pat::Guard(g) => (&*g.pat, Body::Guarded(&g.guard, &a.body)),
                    p => (p, Body::Expr(&a.body)),
                })
                .collect();
            matching(&m.expr, &arms, ctx, want)
        }
        syn::Expr::Call(c) => call(c, ctx, want),
        syn::Expr::MethodCall(m) => method(m, ctx, want),
        syn::Expr::Field(f) => {
            let (base, bt) = flat_of(&f.base, ctx, None)?;
            match (&f.member, &bt) {
                (syn::Member::Named(field), Ty::Named(s)) => {
                    let name = local_name(&field.to_string());
                    let Some((_, ty)) = ctx
                        .types
                        .structs
                        .get(s)
                        .and_then(|fs| fs.iter().find(|(n, _)| *n == name))
                    else {
                        return Break(Residue::Form("field of an unknown struct".to_string()));
                    };
                    if ctx.types.boxed(s) {
                        flat(
                            format!("({} {base})", Types::accessor(s, &name)),
                            ty.clone(),
                        )
                    } else {
                        flat(format!("(project {base} {name})"), ty.clone())
                    }
                }
                (syn::Member::Unnamed(i), Ty::Tuple(ts) | Ty::Pair(ts)) => {
                    match ts.get(i.index as usize) {
                        Some(ty) => flat(tuple_item(&bt, &base, i.index as usize), ty.clone()),
                        None => Break(Residue::Form("tuple field".to_string())),
                    }
                }
                _ => Break(Residue::Form("field access".to_string())),
            }
        }
        // `E::V { a, b }`: a struct variant, its fields in declaration order.
        syn::Expr::Struct(s)
            if s.rest.is_none()
                && s.qself.is_none()
                && data_variant(&s.path, ctx.types).is_some() =>
        {
            let Some((owner, variant, fields)) = data_variant(&s.path, ctx.types) else {
                return Break(Residue::Form("struct variant".to_string()));
            };
            let mut given = BTreeMap::new();
            for fv in &s.fields {
                let syn::Member::Named(id) = &fv.member else {
                    return Break(Residue::Form("struct variant field".to_string()));
                };
                given.insert(id.to_string(), &fv.expr);
            }
            if given.len() != fields.len() {
                return Break(Residue::Form("struct variant literal".to_string()));
            }
            let mut parts = vec![format!("ctor {owner}.{variant}")];
            for (name, ty) in fields {
                let Some(value) = name.as_ref().and_then(|n| given.get(n)) else {
                    return Break(Residue::Form("struct variant literal".to_string()));
                };
                let (v, vt) = flat_of(value, ctx, Some(ty))?;
                if !vt.same(ty) {
                    return Break(Residue::Form("struct variant field of another type".into()));
                }
                parts.push(v);
            }
            flat(format!("({})", parts.join(" ")), Ty::Named(owner))
        }
        syn::Expr::Struct(s) if s.rest.is_none() && s.qself.is_none() => {
            let Some(name) = s
                .path
                .get_ident()
                .map(|i| ctx.types.resolve(&i.to_string()))
            else {
                return Break(Residue::Form("struct path".to_string()));
            };
            let Some(fields) = ctx.types.structs.get(&name) else {
                return Break(Residue::Form(format!("struct {name}")));
            };
            let mut given = BTreeMap::new();
            for fv in &s.fields {
                let syn::Member::Named(id) = &fv.member else {
                    return Break(Residue::Form("struct field".to_string()));
                };
                given.insert(local_name(&id.to_string()), &fv.expr);
            }
            if given.len() != fields.len() {
                return Break(Residue::Form("struct literal".to_string()));
            }
            let mut parts = Vec::new();
            for (field, ty) in fields {
                let Some(value) = given.get(field) else {
                    return Break(Residue::Form("struct literal".to_string()));
                };
                let (v, vt) = flat_of(value, ctx, Some(ty))?;
                if !vt.same(ty) {
                    return Break(Residue::Form("struct field of another type".to_string()));
                }
                parts.push((field, v));
            }
            flat(struct_value(&name, &parts, ctx.types), Ty::Named(name))
        }
        syn::Expr::Tuple(t) if t.elems.len() >= 2 => {
            let wants: Vec<Option<&Ty>> = match want {
                Some(Ty::Tuple(ts) | Ty::Pair(ts)) if ts.len() == t.elems.len() => {
                    ts.iter().map(Some).collect()
                }
                _ => vec![None; t.elems.len()],
            };
            let mut parts = Vec::new();
            let mut tys = Vec::new();
            for (e, w) in t.elems.iter().zip(wants) {
                let (v, vt) = flat_of(e, ctx, w)?;
                parts.push(v);
                tys.push(vt);
            }
            let ty = ctx.types.tuple(tys);
            flat(tuple_value(&ty, &parts), ty)
        }
        syn::Expr::Cast(c) => {
            let (v, vt) = flat_of(&c.expr, ctx, None)?;
            let target = match ctx.types.ty(&c.ty) {
                Continue(t) => t,
                Break(_) => return Break(Residue::Form("cast".to_string())),
            };
            match (&vt, &target) {
                (Ty::Nat(Some(a)), Ty::Nat(Some(b))) if a <= b => flat(v, target),
                (Ty::Int(a), Ty::Int(b)) if a <= b => flat(v, target),
                _ => Break(Residue::Form("narrowing or sign-changing cast".to_string())),
            }
        }
        // `xs[i]`: the element, and where Rust's bounds check panics, the
        // type's default, as Lean's `xs[i]!` is.
        syn::Expr::Index(ix) => {
            let (base, bt) = flat_of(&ix.expr, ctx, None)?;
            let Ty::List(elem) = &bt else {
                return Break(Residue::Form(format!("index of {}", bt.render())));
            };
            let (i, it) = flat_of(&ix.index, ctx, Some(&Ty::Nat(None)))?;
            if !matches!(it, Ty::Nat(_)) {
                return Break(Residue::Form("index by a range".to_string()));
            }
            let Some(d) = default_of(elem, ctx.types) else {
                return Break(Residue::Form("index of a type with no value".to_string()));
            };
            let v = ctx.fresh("item");
            Continue((
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
            ))
        }
        syn::Expr::Continue(c) => loop_exit(ctx, c.label.is_some(), false),
        syn::Expr::Break(b) if b.expr.is_none() => loop_exit(ctx, b.label.is_some(), true),
        syn::Expr::Try(_) => Break(Residue::Form("? outside a let".to_string())),
        syn::Expr::Macro(m) if m.mac.path.is_ident("format") => format_macro(&m.mac, ctx),
        // `matches!(e, P)`: a match with `true` for `P` and `false` otherwise.
        syn::Expr::Macro(m) if m.mac.path.is_ident("matches") => {
            let Ok((scrutinee, pat)) =
                m.mac.parse_body_with(|input: syn::parse::ParseStream<'_>| {
                    let e: syn::Expr = input.parse()?;
                    input.parse::<syn::Token![,]>()?;
                    let p = syn::Pat::parse_multi_with_leading_vert(input)?;
                    let _ = input.parse::<Option<syn::Token![,]>>()?;
                    Ok((e, p))
                })
            else {
                return Break(Residue::Form("matches! with a guard".to_string()));
            };
            let yes: syn::Expr = syn::parse_quote!(true);
            let no: syn::Expr = syn::parse_quote!(false);
            let wild = syn::Pat::Wild(syn::PatWild {
                attrs: Vec::new(),
                underscore_token: syn::token::Underscore::default(),
            });
            let arms = vec![(&pat, Body::Expr(&yes)), (&wild, Body::Expr(&no))];
            matching(&scrutinee, &arms, ctx, Some(&Ty::Bool))
        }
        syn::Expr::Macro(m) if m.mac.path.is_ident("vec") => {
            use syn::punctuated::Punctuated;
            let Ok(items) = m
                .mac
                .parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
            else {
                return Break(Residue::Form("vec! of a repeated value".to_string()));
            };
            let items: Vec<syn::Expr> = items.into_iter().collect();
            list_of(&items, ctx, want)
        }
        syn::Expr::Array(a) => {
            let items: Vec<syn::Expr> = a.elems.iter().cloned().collect();
            list_of(&items, ctx, want)
        }
        _ => Break(Residue::Form(kind_of(e))),
    }
}

/// An array or `vec!` of items, as the list of them.
fn list_of(items: &[syn::Expr], ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    let mut elem = match want {
        Some(Ty::List(t) | Ty::Set(t, _)) => Some((**t).clone()),
        _ => None,
    };
    let mut parts = Vec::new();
    for item in items {
        let (v, vt) = flat_of(item, ctx, elem.as_ref())?;
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
    Continue((Expr::Flat(list), Ty::List(Box::new(elem))))
}

/// `format!` whose holes are all strings: the pieces joined.
fn format_macro(mac: &syn::Macro, ctx: &Ctx<'_>) -> Lift<(Expr, Ty)> {
    use syn::punctuated::Punctuated;
    let bad = |what: &str| Break(Residue::Form(format!("format! {what}")));
    let Ok(args) = mac.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
    else {
        return bad("arguments");
    };
    let mut args = args.into_iter();
    let Some(syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(template),
        ..
    })) = args.next()
    else {
        return bad("template");
    };
    let mut positional = Vec::new();
    let mut named = BTreeMap::new();
    for arg in args {
        match arg {
            syn::Expr::Assign(a) => {
                let syn::Expr::Path(p) = &*a.left else {
                    return bad("argument");
                };
                let Some(id) = p.path.get_ident() else {
                    return bad("argument");
                };
                named.insert(id.to_string(), *a.right);
            }
            other => positional.push(other),
        }
    }
    let template = template.value();
    let mut pieces: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut next = 0;
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
                let value = if hole.is_empty() {
                    let Some(e) = positional.get(next) else {
                        return bad("hole");
                    };
                    next += 1;
                    flat_of(e, ctx, Some(&Ty::Str))?
                } else if let Some(e) = named.get(&hole) {
                    flat_of(e, ctx, Some(&Ty::Str))?
                } else if let Some((name, ty)) = ctx.locals.get(&hole) {
                    (name.clone(), ty.clone())
                } else {
                    return bad("hole");
                };
                match value.1 {
                    Ty::Str => pieces.push(value.0),
                    Ty::Int(_) => {
                        ctx.axioms.borrow_mut().extend(ALL_AXIOMS.map(String::from));
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
    ctx.axioms.borrow_mut().insert("propext".to_string());
    let mut list = "(nil String)".to_string();
    for piece in pieces.iter().rev() {
        list = format!("(cons {piece} {list})");
    }
    Continue((
        Expr::Flat(format!("(prim join {list} \"\" : String)")),
        Ty::Str,
    ))
}

fn binary(b: &syn::ExprBinary, ctx: &Ctx<'_>) -> Lift<(Expr, Ty)> {
    use syn::BinOp as B;
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    let op_text = || crate::ingest::tidy(&quote::ToTokens::to_token_stream(&b.op).to_string());
    match b.op {
        B::And(_) | B::Or(_) => {
            let (l, lt) = flat_of(&b.left, ctx, Some(&Ty::Bool))?;
            let (r, rt) = flat_of(&b.right, ctx, Some(&Ty::Bool))?;
            if lt != Ty::Bool || rt != Ty::Bool {
                return Break(Residue::Form("logic on non-booleans".to_string()));
            }
            let op = if matches!(b.op, B::And(_)) {
                "&&"
            } else {
                "||"
            };
            flat(format!("({l} {op} {r})"), Ty::Bool)
        }
        B::Eq(_) | B::Ne(_) | B::Lt(_) | B::Le(_) | B::Gt(_) | B::Ge(_) => {
            let ((l, lt), (r, _)) = if is_literal(&b.left) {
                let right = flat_of(&b.right, ctx, None)?;
                (flat_of(&b.left, ctx, Some(&right.1))?, right)
            } else {
                let left = flat_of(&b.left, ctx, None)?;
                let right = flat_of(&b.right, ctx, Some(&left.1))?;
                (left, right)
            };
            let text = match (&lt, b.op) {
                (Ty::Nat(_), B::Eq(_)) => format!("({l} =? {r})"),
                (Ty::Nat(_), B::Ne(_)) => format!("(not ({l} =? {r}))"),
                (Ty::Nat(_), B::Lt(_)) => format!("({l} <? {r})"),
                (Ty::Nat(_), B::Le(_)) => format!("({l} <=? {r})"),
                (Ty::Nat(_), B::Gt(_)) => format!("({r} <? {l})"),
                (Ty::Nat(_), B::Ge(_)) => format!("({r} <=? {l})"),
                (Ty::Bool | Ty::Named(_), B::Eq(_)) if is_enum_or_bool(&lt, ctx) => {
                    same_constructor(&lt, &l, &r)
                }
                (Ty::Bool | Ty::Named(_), B::Ne(_)) if is_enum_or_bool(&lt, ctx) => {
                    format!("(not {})", same_constructor(&lt, &l, &r))
                }
                (Ty::Int(_) | Ty::Str | Ty::Char, B::Eq(_)) => {
                    format!("(prim equal {l} {r} : Bool)")
                }
                (Ty::Int(_) | Ty::Str | Ty::Char, B::Ne(_)) => {
                    format!("(not (prim equal {l} {r} : Bool))")
                }
                // Strings order by their scalar values, as Rust's `str` and
                // `char` do.
                (Ty::Int(_) | Ty::Str | Ty::Char, B::Lt(_) | B::Gt(_) | B::Le(_) | B::Ge(_)) => {
                    // Int64's order is decided through propositional
                    // extensionality; a string's through its scalars.
                    ctx.axioms.borrow_mut().insert("propext".to_string());
                    if !matches!(lt, Ty::Int(_)) {
                        ctx.axioms.borrow_mut().extend(ALL_AXIOMS.map(String::from));
                    }
                    let (x, y) = match b.op {
                        B::Lt(_) | B::Ge(_) => (&l, &r),
                        _ => (&r, &l),
                    };
                    let less = format!("(prim less_than {x} {y} : Bool)");
                    if matches!(b.op, B::Lt(_) | B::Gt(_)) {
                        less
                    } else {
                        format!("(not {less})")
                    }
                }
                (Ty::Option(inner), B::Eq(_) | B::Ne(_)) => {
                    let (x, y) = (ctx.fresh("left"), ctx.fresh("right"));
                    let same = match &**inner {
                        Ty::Bool | Ty::Named(_) if is_enum_or_bool(inner, ctx) => {
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
                    let eq = matches!(b.op, B::Eq(_));
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
            flat(text, Ty::Bool)
        }
        B::Div(_) | B::Rem(_) => {
            let (l, lt) = flat_of(&b.left, ctx, None)?;
            match (&lt, int_literal(&b.right)) {
                (Ty::Nat(_), Some(k)) if k > 0 => {
                    let prim = if matches!(b.op, B::Div(_)) {
                        "quotient"
                    } else {
                        "remainder"
                    };
                    flat(format!("(prim {prim} {l} {k} 0 : Nat)"), lt)
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

fn is_enum_or_bool(t: &Ty, ctx: &Ctx<'_>) -> bool {
    match t {
        Ty::Bool => true,
        Ty::Named(n) => ctx.types.enums.contains_key(n),
        _ => false,
    }
}

fn call(c: &syn::ExprCall, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    let syn::Expr::Path(p) = &*c.func else {
        return Break(Residue::Form("call of an expression".to_string()));
    };
    if let (Some(w), [arg]) = (
        wrapper(&p.path),
        c.args.iter().collect::<Vec<_>>().as_slice(),
    ) {
        return match (w, want) {
            ("Some", _) => {
                let inner = match want {
                    Some(Ty::Option(t)) => Some(&**t),
                    _ => None,
                };
                let (v, vt) = flat_of(arg, ctx, inner)?;
                flat(
                    format!("(ctor Option.some [{}] {v})", vt.render()),
                    Ty::Option(Box::new(vt)),
                )
            }
            ("Ok", Some(t @ Ty::Outcome(ok, err))) | ("Err", Some(t @ Ty::Outcome(ok, err))) => {
                let (ctor, inner) = if w == "Ok" {
                    ("ok", &**ok)
                } else {
                    ("error", &**err)
                };
                let (v, vt) = flat_of(arg, ctx, Some(inner))?;
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
                )
            }
            _ => Break(Residue::Form("result of an unknown type".to_string())),
        };
    }
    // `E::V(a, b)`: a variant of a data-carrying enum.
    if let Some((owner, variant, fields)) = data_variant(&p.path, ctx.types) {
        if fields.len() != c.args.len() {
            return Break(Residue::Form(format!(
                "{owner}::{variant} of another arity"
            )));
        }
        let mut parts = vec![format!("ctor {owner}.{variant}")];
        for (arg, (_, ty)) in c.args.iter().zip(fields) {
            let (v, vt) = flat_of(arg, ctx, Some(ty))?;
            if !vt.same(ty) {
                return Break(Residue::Form(format!("{owner}::{variant} of another type")));
            }
            parts.push(v);
        }
        return flat(format!("({})", parts.join(" ")), Ty::Named(owner));
    }
    let segments: Vec<String> = p
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    let call_args: Vec<&syn::Expr> = c.args.iter().collect();
    match (segments.as_slice(), call_args.as_slice(), want) {
        ([s, n], [], _) if s == "String" && n == "new" => return flat("\"\"".into(), Ty::Str),
        ([s, n], [], Some(t @ Ty::List(inner))) if s == "Vec" && n == "new" => {
            return flat(format!("(nil {})", inner.render()), t.clone());
        }
        // An empty ordered set or map: the empty list of its members or of
        // its entries, which `insert` keeps sorted and free of duplicates.
        ([s, n], [], Some(t @ Ty::Set(inner, true))) if s == "BTreeSet" && n == "new" => {
            return flat(format!("(nil {})", inner.render()), t.clone());
        }
        ([s, n], [], Some(t @ Ty::Map(_, _, true))) if s == "BTreeMap" && n == "new" => {
            return flat(format!("(nil {})", t.entry().render()), t.clone());
        }
        // A capacity is a hint to the allocator: the value is the empty list.
        ([s, n], [_], Some(t @ Ty::List(inner))) if s == "Vec" && n == "with_capacity" => {
            return flat(format!("(nil {})", inner.render()), t.clone());
        }
        ([s, n], [a], _) if s == "String" && n == "from" => {
            let (v, vt) = flat_of(a, ctx, Some(&Ty::Str))?;
            if vt == Ty::Str {
                return flat(v, Ty::Str);
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
        let key = format!("{}::{method}", ctx.types.resolve(owner));
        if ctx.fns.contains_key(&key) {
            name = key;
        }
    }
    let Some(sig) = ctx.fns.get(&name) else {
        return Break(Residue::Call(name));
    };
    if sig.params.len() != c.args.len() {
        return Break(Residue::Call(name));
    }
    let mut parts = vec![sig.name.clone()];
    let mut descends = None;
    for (i, (arg, ty)) in c.args.iter().zip(&sig.params).enumerate() {
        let (v, vt) = flat_of(arg, ctx, Some(ty))?;
        if !vt.same(ty) {
            return Break(Residue::Form(format!("argument of another type to {name}")));
        }
        if name == ctx.current && ctx.tails.get(&v) == ctx.params.get(i) && descends.is_none() {
            descends = Some(i);
        }
        parts.push(v);
    }
    if name == ctx.current {
        let Some(i) = descends else {
            return Break(Residue::Form(
                "recursion that does not descend on a slice".to_string(),
            ));
        };
        ctx.descends.borrow_mut().insert(i);
    } else {
        ctx.calls.borrow_mut().insert(name);
    }
    flat(format!("({})", parts.join(" ")), sig.result.clone())
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

/// A call of a prelude definition, with the axioms it brings.
fn prelude(ctx: &Ctx<'_>, name: &str, args: &[&str]) -> String {
    ctx.axioms.borrow_mut().extend(prelude_axioms(name));
    format!("({PRELUDE}::{name} {})", args.join(" "))
}

/// The methods of `str` and `char` over the prelude: `None` when the method
/// is none of them.
#[allow(clippy::too_many_lines)]
fn text_method(
    m: &syn::ExprMethodCall,
    name: &str,
    r: &str,
    rt: &Ty,
    args: &[&syn::Expr],
    ctx: &Ctx<'_>,
    want: Option<&Ty>,
) -> Lift<Option<(Expr, Ty)>> {
    let flat = |s: String, t: Ty| Continue(Some((Expr::Flat(s), t)));
    // A pattern: a string or a character.
    let pattern = |a: &syn::Expr| -> Lift<String> {
        let (v, vt) = flat_of(a, ctx, Some(&Ty::Str))?;
        match vt {
            Ty::Str | Ty::Char => Continue(v),
            _ => Break(Residue::Form(format!("a .{name} pattern that is not text"))),
        }
    };
    let opt = |t: Ty| Ty::Option(Box::new(t));
    let list = |t: Ty| Ty::List(Box::new(t));
    let chars = rt == &Ty::Char;
    match (name, args) {
        ("len", []) if !chars => flat(prelude(ctx, "textLength", &[r]), Ty::Nat(None)),
        ("as_str" | "as_ref" | "into" | "to_owned" | "borrow", []) => {
            flat(r.to_string(), rt.clone())
        }
        ("to_string", []) => flat(r.to_string(), Ty::Str),
        ("trim", []) => flat(prelude(ctx, "textTrim", &[r]), Ty::Str),
        ("trim_start", []) => flat(prelude(ctx, "textTrimStart", &[r]), Ty::Str),
        ("trim_end", []) => flat(prelude(ctx, "textTrimEnd", &[r]), Ty::Str),
        // A predicate on the first or last character.
        ("starts_with" | "ends_with", [f]) if !chars && matches!(f, syn::Expr::Closure(_)) => {
            let lam = lambda(f, &[Ty::Char], ctx, Some(&Ty::Bool))?;
            if lam.ty != Ty::Bool {
                return Break(Residue::Form(format!(
                    "a .{name} predicate of another type"
                )));
            }
            ctx.calls.borrow_mut().extend(lam.calls.iter().cloned());
            ctx.axioms.borrow_mut().extend(lam.axioms.iter().cloned());
            let all = prelude(ctx, "textChars", &[r]);
            let at = if name == "starts_with" {
                "0".to_string()
            } else {
                format!("(prim subtract (prim length {all} : Nat) 1 : Nat)")
            };
            Continue(Some((
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
            )))
        }
        ("starts_with" | "ends_with" | "contains", [p]) if !chars => {
            let p = pattern(p)?;
            let f = match name {
                "starts_with" => "textStartsWith",
                "ends_with" => "textEndsWith",
                _ => "textContains",
            };
            flat(prelude(ctx, f, &[r, &p]), Ty::Bool)
        }
        ("strip_prefix" | "strip_suffix", [p]) if !chars => {
            let p = pattern(p)?;
            let f = if name == "strip_prefix" {
                "textStripPrefix"
            } else {
                "textStripSuffix"
            };
            flat(prelude(ctx, f, &[r, &p]), opt(Ty::Str))
        }
        ("find" | "rfind", [p]) if !chars => {
            let p = pattern(p)?;
            let f = if name == "find" {
                "textFirst"
            } else {
                "textLast"
            };
            flat(prelude(ctx, f, &[r, &p]), opt(Ty::Nat(None)))
        }
        ("split" | "rsplit", [p]) if !chars => {
            let p = pattern(p)?;
            let f = if name == "split" {
                "textSplit"
            } else {
                "textRsplit"
            };
            flat(prelude(ctx, f, &[r, &p]), list(Ty::Str))
        }
        ("split_once" | "rsplit_once", [p]) if !chars => {
            let p = pattern(p)?;
            let f = if name == "split_once" {
                "textFirst"
            } else {
                "textLast"
            };
            let pair = Ty::Tuple(vec![Ty::Str, Ty::Str]);
            pair.mentions(&mut ctx.seen.borrow_mut());
            let at = ctx.fresh("at");
            let whole = prelude(ctx, "textLength", &[r]);
            let width = prelude(ctx, "textLength", &[&p]);
            let before = prelude(ctx, "textSlice", &[r, "0", &at]);
            let after = prelude(ctx, "textSlice", &[r, &format!("({at} + {width})"), &whole]);
            let body = Expr::Match {
                scrutinee: prelude(ctx, f, &[r, &p]),
                arms: vec![Arm { pattern: "Option.none".into(), body: Expr::Flat(format!("(ctor Option.none [{}])", pair.render())) }, Arm { pattern: format!("Option.some {at}"), body: Expr::Flat(format!(
                            "(ctor Option.some [{0}] (record {0} {{item0 := {before}, item1 := {after}}}))",
                            pair.render()
                        )) }],
            };
            Continue(Some((body, opt(pair))))
        }
        ("replace", [from, to]) if !chars => {
            let from = pattern(from)?;
            let (to, tt) = flat_of(to, ctx, Some(&Ty::Str))?;
            if !matches!(tt, Ty::Str | Ty::Char) {
                return Break(Residue::Form("a replacement that is not text".into()));
            }
            flat(prelude(ctx, "textReplace", &[r, &from, &to]), Ty::Str)
        }
        ("trim_start_matches" | "trim_end_matches" | "trim_matches", [p]) if !chars => {
            let p = pattern(p)?;
            let f = match name {
                "trim_start_matches" => "textTrimStartMatches",
                "trim_end_matches" => "textTrimEndMatches",
                _ => "textTrimMatches",
            };
            flat(prelude(ctx, f, &[r, &p]), Ty::Str)
        }
        ("lines", []) if !chars => flat(prelude(ctx, "textLines", &[r]), list(Ty::Str)),
        ("split_whitespace", []) if !chars => flat(prelude(ctx, "textWords", &[r]), list(Ty::Str)),
        ("chars", []) if !chars => flat(prelude(ctx, "textChars", &[r]), list(Ty::Char)),
        ("bytes" | "as_bytes", []) if !chars => {
            flat(prelude(ctx, "textBytes", &[r]), list(Ty::Nat(Some(8))))
        }
        ("repeat", [n]) if !chars => {
            let (n, nt) = flat_of(n, ctx, Some(&Ty::Nat(None)))?;
            if !matches!(nt, Ty::Nat(_)) {
                return Break(Residue::Form("a repeat count that is not a number".into()));
            }
            flat(prelude(ctx, "textRepeat", &[&n, r]), Ty::Str)
        }
        ("to_ascii_lowercase", []) => flat(prelude(ctx, "textAsciiLower", &[r]), rt.clone()),
        ("to_ascii_uppercase", []) => flat(prelude(ctx, "textAsciiUpper", &[r]), rt.clone()),
        ("eq_ignore_ascii_case", [other]) => {
            let (o, ot) = flat_of(other, ctx, Some(rt))?;
            if !ot.same(rt) {
                return Break(Residue::Form("eq_ignore_ascii_case of another type".into()));
            }
            let a = prelude(ctx, "textAsciiLower", &[r]);
            let b = prelude(ctx, "textAsciiLower", &[&o]);
            flat(format!("(prim equal {a} {b} : Bool)"), Ty::Bool)
        }
        ("is_ascii", []) => flat(prelude(ctx, "textIsAscii", &[r]), Ty::Bool),
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
            flat(prelude(ctx, f, &[r]), Ty::Bool)
        }
        ("parse", []) if !chars => {
            let target = match &m.turbofish {
                Some(t) => match t.args.iter().collect::<Vec<_>>().as_slice() {
                    [syn::GenericArgument::Type(t)] => Some(ctx.types.ty(t)?),
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
            flat(
                prelude(ctx, "textParseNat", &[r, max_of(bits)]),
                Ty::Outcome(Box::new(Ty::Nat(Some(bits))), Box::new(Ty::ParseError)),
            )
        }
        _ => Continue(None),
    }
}

/// The axioms of the runtime's string length and decimal formatting.
const ALL_AXIOMS: [&str; 3] = ["Classical.choice", "Quot.sound", "propext"];

fn method(m: &syn::ExprMethodCall, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    let name = m.method.to_string();
    // `s.chars().count()`: the number of characters, which is LexLean's
    // string length.
    if let syn::Expr::MethodCall(inner) = &*m.receiver {
        if name == "count" && m.args.is_empty() && inner.method == "chars" && inner.args.is_empty()
        {
            let (s, st) = flat_of(&inner.receiver, ctx, Some(&Ty::Str))?;
            if st == Ty::Str {
                ctx.axioms.borrow_mut().extend(ALL_AXIOMS.map(String::from));
                return flat(format!("(prim length {s} : Nat)"), Ty::Nat(None));
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
    let (r, rt) = flat_of(&m.receiver, ctx, receiver_want.as_ref())?;
    let args: Vec<&syn::Expr> = m.args.iter().collect();
    let arg = |want: &Ty| -> Lift<String> {
        let [a] = args.as_slice() else {
            return Break(Residue::Form(format!("method call .{name}")));
        };
        let (v, vt) = flat_of(a, ctx, Some(want))?;
        if vt.same(want) {
            Continue(v)
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
        if let Some(sig) = ctx.fns.get(&key) {
            if key == ctx.current {
                // Recursion on a sub-term of `self` descends; any other does not.
                if ctx.tails.get(&r) != ctx.params.first() {
                    return Break(Residue::Form("a method that calls itself".to_string()));
                }
                ctx.descends.borrow_mut().insert(0);
            }
            if sig.params.len() != args.len() + 1 {
                return Break(Residue::Call(key));
            }
            let mut parts = vec![sig.name.clone(), r];
            for (a, ty) in args.iter().zip(&sig.params[1..]) {
                let (v, vt) = flat_of(a, ctx, Some(ty))?;
                if !vt.same(ty) {
                    return Break(Residue::Form(format!(
                        "argument of another type to .{name}"
                    )));
                }
                parts.push(v);
            }
            if key != ctx.current {
                ctx.calls.borrow_mut().insert(key);
            }
            return flat(format!("({})", parts.join(" ")), sig.result.clone());
        }
    }
    if matches!(rt, Ty::Map(..) | Ty::Set(..)) {
        return collection(&name, &r, &rt, &args, ctx);
    }
    if matches!(rt, Ty::Str | Ty::Char) {
        if let Some(lifted) = text_method(m, &name, &r, &rt, &args, ctx, want)? {
            return Continue(lifted);
        }
    }
    if let Ty::List(elem) = &rt {
        if let Some(lifted) = combinator(m, &name, &r, elem, &args, ctx)? {
            return Continue(lifted);
        }
    }
    if rt == Ty::Nat(Some(8)) && args.is_empty() {
        if let Some(test) = ascii_byte(&name, &r) {
            return flat(test, Ty::Bool);
        }
    }
    match (&rt, name.as_str()) {
        (Ty::Nat(w), "checked_add" | "checked_mul") => {
            let Some(bits) = w else {
                return Break(Residue::Width(format!(".{name} on usize")));
            };
            let a = arg(&rt)?;
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
            )
        }
        (Ty::Nat(_), "checked_sub") => {
            let a = arg(&rt)?;
            flat(
                format!(
                    "(if ({a} <=? {r}) then {} else {})",
                    some(&rt, &format!("(prim subtract {r} {a} : Nat)")),
                    none(&rt)
                ),
                opt(&rt),
            )
        }
        (Ty::Nat(_), "checked_div" | "checked_rem") => {
            let a = arg(&rt)?;
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
            )
        }
        (Ty::Nat(w), "wrapping_add" | "wrapping_mul" | "wrapping_sub") => {
            let Some(bits) = w else {
                return Break(Residue::Width(format!(".{name} on usize")));
            };
            let a = arg(&rt)?;
            let m = pow2(*bits);
            let v = match name.as_str() {
                "wrapping_add" => format!("({r} + {a})"),
                "wrapping_mul" => format!("(prim multiply {r} {a} : Nat)"),
                _ => format!("(prim subtract ({r} + {m}) {a} : Nat)"),
            };
            flat(format!("(prim remainder {v} {m} 0 : Nat)"), rt.clone())
        }
        (Ty::Nat(w), "saturating_add" | "saturating_mul") => {
            let Some(bits) = w else {
                return Break(Residue::Width(format!(".{name} on usize")));
            };
            let a = arg(&rt)?;
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
            )
        }
        (Ty::Nat(_), "saturating_sub") => {
            let a = arg(&rt)?;
            flat(format!("(prim subtract {r} {a} : Nat)"), rt.clone())
        }
        (Ty::Nat(_), "abs_diff") => {
            let a = arg(&rt)?;
            flat(
                format!("(if ({r} <=? {a}) then (prim subtract {a} {r} : Nat) else (prim subtract {r} {a} : Nat))"),
                rt.clone(),
            )
        }
        (Ty::Nat(_), "midpoint") => {
            let a = arg(&rt)?;
            flat(format!("(prim quotient ({r} + {a}) 2 0 : Nat)"), rt.clone())
        }
        (Ty::Nat(_), "min" | "max") => {
            let a = arg(&rt)?;
            let (x, y) = if name == "min" { (&r, &a) } else { (&a, &r) };
            flat(format!("(if ({r} <=? {a}) then {x} else {y})"), rt.clone())
        }
        (Ty::Int(bits), "checked_add" | "checked_sub" | "checked_mul" | "checked_div") => {
            if *bits != 64 {
                return Break(Residue::Width(format!(".{name} on i{bits}")));
            }
            let a = arg(&rt)?;
            let prim = match name.as_str() {
                "checked_add" => "checked_add",
                "checked_sub" => "checked_subtract",
                "checked_mul" => "checked_multiply",
                _ => "checked_quotient",
            };
            flat(format!("(prim {prim} {r} {a} : (Option Int64))"), opt(&rt))
        }
        (Ty::Int(bits), "checked_neg") if args.is_empty() => {
            if *bits != 64 {
                return Break(Residue::Width(format!(".{name} on i{bits}")));
            }
            flat(
                format!("(prim checked_negate {r} : (Option Int64))"),
                opt(&rt),
            )
        }
        (Ty::Int(_), "min" | "max") => {
            ctx.axioms.borrow_mut().insert("propext".to_string());
            let a = arg(&rt)?;
            let (x, y) = if name == "min" { (&r, &a) } else { (&a, &r) };
            flat(
                format!("(if (prim less_than {a} {r} : Bool) then {y} else {x})"),
                rt.clone(),
            )
        }
        (Ty::List(_), "len") if args.is_empty() => {
            flat(format!("(prim length {r} : Nat)"), Ty::Nat(None))
        }
        (Ty::List(_), "is_empty") if args.is_empty() => {
            flat(format!("((prim length {r} : Nat) =? 0)"), Ty::Bool)
        }
        (Ty::List(t), "first") if args.is_empty() => flat(
            format!("(prim index {r} 0 : (Option {}))", t.render()),
            opt(t),
        ),
        (Ty::List(t), "get") => {
            let i = arg(&Ty::Nat(None))?;
            flat(
                format!("(prim index {r} {i} : (Option {}))", t.render()),
                opt(t),
            )
        }
        (Ty::Str, "is_empty") if args.is_empty() => {
            flat(format!("(prim equal {r} \"\" : Bool)"), Ty::Bool)
        }
        (Ty::Str, "to_string" | "to_owned") | (_, "clone" | "copied" | "cloned" | "to_vec")
            if args.is_empty() =>
        {
            flat(r, rt.clone())
        }
        (Ty::Option(_), "is_some" | "is_none") if args.is_empty() => {
            let v = ctx.fresh("value");
            let (n, s) = if name == "is_some" {
                ("false", "true")
            } else {
                ("true", "false")
            };
            Continue((
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
            ))
        }
        (Ty::Option(t), "unwrap_or") => {
            let d = arg(t)?;
            let v = ctx.fresh("value");
            Continue((
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
            ))
        }
        (Ty::Outcome(_, _), "is_ok" | "is_err") if args.is_empty() => {
            let (v, e) = (ctx.fresh("value"), ctx.fresh("error"));
            let (o, x) = if name == "is_ok" {
                ("true", "false")
            } else {
                ("false", "true")
            };
            Continue((
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
            ))
        }
        (Ty::Outcome(t, _), "ok") if args.is_empty() => {
            let (v, e) = (ctx.fresh("value"), ctx.fresh("error"));
            Continue((
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
            ))
        }
        (Ty::Option(_) | Ty::Outcome(..), _) => wrapped_method(&name, &r, &rt, &args, ctx),
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
    args: &[&syn::Expr],
    ctx: &Ctx<'_>,
) -> Lift<(Expr, Ty)> {
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
    let closure = |f: &syn::Expr, params: &[Ty], want: Option<&Ty>| -> Lift<Lambda> {
        let lam = lambda(f, params, ctx, want)?;
        ctx.calls.borrow_mut().extend(lam.calls.iter().cloned());
        ctx.axioms.borrow_mut().extend(lam.axioms.iter().cloned());
        Continue(lam)
    };
    let fresh_e = ctx.fresh("error");
    match (name, args) {
        ("map", [f]) => {
            let lam = closure(f, std::slice::from_ref(&value), None)?;
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            match &failure {
                None => Continue((
                    two(&v, wrap_some(&lam.ty, body), "", Expr::Flat(none(&lam.ty))),
                    opt(&lam.ty),
                )),
                Some(e) => Continue((
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
                )),
            }
        }
        ("map_or", [d, f]) => {
            let lam = closure(f, std::slice::from_ref(&value), None)?;
            let (dv, dt) = flat_of(d, ctx, Some(&lam.ty))?;
            if !dt.same(&lam.ty) {
                return Break(Residue::Form("map_or of another type".into()));
            }
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            Continue((two(&v, body, &fresh_e, Expr::Flat(dv)), lam.ty.clone()))
        }
        ("map_err", [f]) => {
            let Some(e) = &failure else { return bad() };
            let lam = closure(f, std::slice::from_ref(e), None)?;
            let v = ctx.fresh("value");
            let x = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            let types = result_of(&value, &lam.ty);
            Continue((
                two(
                    &v,
                    Expr::Flat(format!("(ctor Result.ok {types} {v})")),
                    &x,
                    wrap_ctor(&format!("Result.error {types}"), body),
                ),
                Ty::Outcome(Box::new(value.clone()), Box::new(lam.ty.clone())),
            ))
        }
        ("and_then", [f]) => {
            let lam = closure(f, std::slice::from_ref(&value), None)?;
            let v = lam.binders[0].clone();
            let body = lam.wrap(lam.core.clone());
            let fail = match (&failure, &lam.ty) {
                (None, Ty::Option(u)) => none(u),
                (Some(e), Ty::Outcome(u, e2)) if e.same(e2) => {
                    format!("(ctor Result.error {} {fresh_e})", result_of(u, e))
                }
                _ => return Break(Residue::Form("and_then of another type".into())),
            };
            Continue((two(&v, body, &fresh_e, Expr::Flat(fail)), lam.ty.clone()))
        }
        ("filter", [f]) if failure.is_none() => {
            let lam = closure(f, std::slice::from_ref(&value), Some(&Ty::Bool))?;
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
            Continue((two(&v, test, "", Expr::Flat(none(&value))), rt.clone()))
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
            let lam = closure(f, std::slice::from_ref(&param), Some(&Ty::Bool))?;
            if lam.ty != Ty::Bool {
                return bad();
            }
            let b = lam.binders[0].clone();
            let test = lam.wrap(lam.core.clone());
            let otherwise = Expr::Flat((name == "is_none_or").to_string());
            if on_err {
                let v = ctx.fresh("value");
                return Continue((two(&v, otherwise, &b, test), Ty::Bool));
            }
            Continue((two(&b, test, &fresh_e, otherwise), Ty::Bool))
        }
        ("ok_or", [e]) if failure.is_none() => {
            let (ev, et) = flat_of(e, ctx, None)?;
            let v = ctx.fresh("value");
            let types = result_of(&value, &et);
            Continue((
                two(
                    &v,
                    Expr::Flat(format!("(ctor Result.ok {types} {v})")),
                    "",
                    Expr::Flat(format!("(ctor Result.error {types} {ev})")),
                ),
                Ty::Outcome(Box::new(value.clone()), Box::new(et)),
            ))
        }
        ("err", []) => {
            let Some(e) = &failure else { return bad() };
            let v = ctx.fresh("value");
            Continue((
                two(
                    &v,
                    Expr::Flat(none(e)),
                    &fresh_e,
                    Expr::Flat(some(e, &fresh_e)),
                ),
                opt(e),
            ))
        }
        ("unwrap_or", [d]) => {
            let (dv, dt) = flat_of(d, ctx, Some(&value))?;
            if !dt.same(&value) {
                return Break(Residue::Form("unwrap_or of another type".into()));
            }
            let v = ctx.fresh("value");
            Continue((
                two(&v, Expr::Flat(v.clone()), &fresh_e, Expr::Flat(dv)),
                value,
            ))
        }
        ("unwrap_or_else", [f]) => {
            let params: Vec<Ty> = failure.iter().cloned().collect();
            let lam = closure(f, &params, Some(&value))?;
            if !lam.ty.same(&value) {
                return Break(Residue::Form("unwrap_or_else of another type".into()));
            }
            let e = lam
                .binders
                .first()
                .cloned()
                .unwrap_or_else(|| fresh_e.clone());
            let v = ctx.fresh("value");
            let fallback = lam.wrap(lam.core.clone());
            Continue((two(&v, Expr::Flat(v.clone()), &e, fallback), value))
        }
        ("unwrap_or_default" | "unwrap" | "expect", _) => {
            let rust_default = name == "unwrap_or_default";
            if rust_default && matches!(value, Ty::Named(_)) {
                return Break(Residue::Form("unwrap_or_default of a named type".into()));
            }
            let Some(d) = default_of(&value, ctx.types) else {
                return Break(Residue::Form(format!(".{name} of a type with no value")));
            };
            let v = ctx.fresh("value");
            Continue((
                two(&v, Expr::Flat(v.clone()), &fresh_e, Expr::Flat(d)),
                value,
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
fn bind_pat<'a>(
    p: &syn::Pat,
    ty: &Ty,
    ctx: &Ctx<'a>,
    lets: &mut Vec<(String, String, String)>,
    nested: bool,
) -> Lift<(Ctx<'a>, String)> {
    match (p, ty) {
        (syn::Pat::Ident(i), _) if i.subpat.is_none() && i.by_ref.is_none() => {
            Continue(ctx.bind(&i.ident.to_string(), ty.clone()))
        }
        (syn::Pat::Wild(_), _) => Continue((ctx.clone(), ctx.fresh("item"))),
        (syn::Pat::Reference(r), _) => bind_pat(&r.pat, ty, ctx, lets, nested),
        (syn::Pat::Type(pt), _) => bind_pat(&pt.pat, ty, ctx, lets, nested),
        (syn::Pat::Tuple(tp), Ty::Tuple(ts) | Ty::Pair(ts))
            if !nested && tp.elems.len() == ts.len() =>
        {
            let whole = ctx.fresh("pair");
            let mut scope = ctx.clone();
            for (i, (e, t)) in tp.elems.iter().zip(ts).enumerate() {
                let (next, name) = bind_pat(e, t, &scope, lets, true)?;
                scope = next;
                lets.push((name, t.render(), tuple_item(ty, &whole, i)));
            }
            Continue((scope, whole))
        }
        _ => Break(Residue::Form("closure pattern".to_string())),
    }
}

/// A closure argument over parameters of these types.
fn lambda(e: &syn::Expr, params: &[Ty], ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<Lambda> {
    let syn::Expr::Closure(c) = e else {
        return Break(Residue::Form(
            "an iterator adapter given a function, not a closure".to_string(),
        ));
    };
    if c.inputs.len() != params.len() || c.asyncness.is_some() {
        return Break(Residue::Form("closure".to_string()));
    }
    let calls = RefCell::new(BTreeSet::new());
    let axioms = RefCell::new(BTreeSet::new());
    let descends = RefCell::new(BTreeSet::new());
    let mut scope = Ctx {
        calls: &calls,
        axioms: &axioms,
        descends: &descends,
        tails: BTreeMap::new(),
        top: false,
        looping: None,
        hidden: Vec::new(),
        ..ctx.clone()
    };
    let mut binders = Vec::new();
    let mut lets = Vec::new();
    for (p, ty) in c.inputs.iter().zip(params) {
        let (next, name) = bind_pat(p, ty, &scope, &mut lets, false)?;
        scope = next;
        binders.push(name);
    }
    let (core, ty) = flat_of(&c.body, &scope, want)?;
    if !descends.borrow().is_empty() {
        return Break(Residue::Form("recursion inside a closure".to_string()));
    }
    drop(scope);
    Continue(Lambda {
        binders,
        lets,
        core,
        ty,
        calls: calls.into_inner(),
        axioms: axioms.into_inner(),
    })
}

/// The outer locals a helper's body names: they become its parameters.
fn captures(ctx: &Ctx<'_>, texts: &[String], bound: &[String]) -> Vec<(String, Ty)> {
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
    ctx: &Ctx<'_>,
    kind: &str,
    list: &str,
    elem: &Ty,
    lam: &Lambda,
    head: &str,
    lead: &[(String, Ty, String)],
    result: &Ty,
    nil: String,
    cons: Step<'_>,
) -> (Expr, Ty) {
    let mut helpers = ctx.helpers.borrow_mut();
    let base = format!("{}{kind}", ctx.own);
    let mut n = helpers.len() + 1;
    let name = loop {
        let candidate = format!("{base}{n}");
        if !ctx.fns.values().any(|f| f.name == candidate)
            && !helpers.iter().any(|h| h.name == candidate)
            && !ctx.taken(&candidate)
        {
            break candidate;
        }
        n += 1;
    };
    let items = ctx.fresh("items");
    let rest = ctx.fresh("rest");
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
    helpers.push(Helper {
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
    ctx.calls.borrow_mut().insert(format!("#{name}"));
    let mut parts = vec![name, list.to_string()];
    parts.extend(lead.iter().map(|(_, _, v)| v.clone()));
    parts.extend(cap_names);
    (Expr::Flat(format!("({})", parts.join(" "))), result.clone())
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
fn lookup(
    ctx: &Ctx<'_>,
    kind: &str,
    r: &str,
    elem: &Ty,
    value: Option<&Ty>,
    needle: &syn::Expr,
) -> Lift<(Expr, Ty)> {
    let key_ty = match (elem, value) {
        (Ty::Tuple(kv), Some(_)) => kv[0].clone(),
        _ => elem.clone(),
    };
    let (v, vt) = flat_of(needle, ctx, Some(&key_ty))?;
    if !vt.same(&key_ty) {
        return Break(Residue::Form(format!("{kind} of another type")));
    }
    let head = ctx.fresh("item");
    let probe = ctx.fresh("needle");
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
        ctx,
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
    args: &[&syn::Expr],
    ctx: &Ctx<'_>,
) -> Lift<(Expr, Ty)> {
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    let unordered = || {
        Break(Residue::Form(format!(
            ".{name} on a hash collection, whose order is unspecified"
        )))
    };
    match (rt, name, args) {
        (_, "len", []) => flat(format!("(prim length {r} : Nat)"), Ty::Nat(None)),
        (_, "is_empty", []) => flat(format!("((prim length {r} : Nat) =? 0)"), Ty::Bool),
        (Ty::Map(_, v, _), "get", [k]) => lookup(ctx, "Get", r, &rt.entry(), Some(v), k),
        (Ty::Map(_, v, _), "contains_key", [k]) => lookup(ctx, "Has", r, &rt.entry(), Some(v), k),
        (Ty::Set(t, _), "contains", [k]) => lookup(ctx, "Contains", r, t, None, k),
        (Ty::Map(..), "iter" | "into_iter", []) => match rt {
            Ty::Map(_, _, true) => flat(r.to_string(), Ty::List(Box::new(rt.entry()))),
            _ => unordered(),
        },
        (Ty::Set(t, sorted), "iter" | "into_iter", []) => {
            if *sorted {
                flat(r.to_string(), Ty::List(t.clone()))
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
            let head = ctx.fresh("entry");
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
                ctx,
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
fn combinator(
    m: &syn::ExprMethodCall,
    name: &str,
    r: &str,
    elem: &Ty,
    args: &[&syn::Expr],
    ctx: &Ctx<'_>,
) -> Lift<Option<(Expr, Ty)>> {
    let list = Ty::List(Box::new(elem.clone()));
    let flat = |s: String, t: Ty| Continue(Some((Expr::Flat(s), t)));
    match (name, args) {
        ("iter" | "into_iter", []) => flat(r.to_string(), list),
        ("collect", []) => {
            let to_string = m
                .turbofish
                .as_ref()
                .is_some_and(|t| quote::ToTokens::to_token_stream(&t.args).to_string() == "String");
            if to_string {
                if *elem != Ty::Str {
                    return Break(Residue::Form("collect of non-strings into a String".into()));
                }
                ctx.axioms.borrow_mut().insert("propext".to_string());
                return flat(format!("(prim join {r} \"\" : String)"), Ty::Str);
            }
            flat(r.to_string(), list)
        }
        ("count", []) => flat(format!("(prim length {r} : Nat)"), Ty::Nat(None)),
        ("join", [sep]) if *elem == Ty::Str => {
            let (s, st) = flat_of(sep, ctx, Some(&Ty::Str))?;
            if st != Ty::Str {
                return Break(Residue::Form(
                    "join with a separator of another type".into(),
                ));
            }
            ctx.axioms.borrow_mut().insert("propext".to_string());
            flat(format!("(prim join {r} {s} : String)"), Ty::Str)
        }
        ("contains", [needle]) => Continue(Some(lookup(ctx, "Contains", r, elem, None, needle)?)),
        ("map" | "filter" | "any" | "all" | "find", [f]) => {
            let want = if name == "map" { None } else { Some(&Ty::Bool) };
            let lam = lambda(f, std::slice::from_ref(elem), ctx, want)?;
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
            Continue(Some(walk(
                ctx,
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
            let (iv, it) = flat_of(init, ctx, None)?;
            let lam = lambda(f, &[it.clone(), elem.clone()], ctx, Some(&it))?;
            if !lam.ty.same(&it) {
                return Break(Residue::Form("a fold step of another type".into()));
            }
            let acc = lam.binders[0].clone();
            let head = lam.binders[1].clone();
            let core = lam.core.clone();
            let cons = |rec: Recur<'_>, _: &str| Expr::Flat(rec(std::slice::from_ref(&core)));
            Continue(Some(walk(
                ctx,
                "Fold",
                r,
                elem,
                &lam,
                &head,
                &[(acc.clone(), it.clone(), iv)],
                &it,
                acc,
                &cons,
            )))
        }
        ("next" | "first", []) => flat(
            format!("(prim index {r} 0 : (Option {}))", elem.render()),
            Ty::Option(Box::new(elem.clone())),
        ),
        ("last", []) => flat(
            format!(
                "(prim index {r} (prim subtract (prim length {r} : Nat) 1 : Nat) : (Option {}))",
                elem.render()
            ),
            Ty::Option(Box::new(elem.clone())),
        ),
        ("nth", [n]) => {
            let (n, nt) = flat_of(n, ctx, Some(&Ty::Nat(None)))?;
            if !matches!(nt, Ty::Nat(_)) {
                return Break(Residue::Form("nth of another type".into()));
            }
            flat(
                format!("(prim index {r} {n} : (Option {}))", elem.render()),
                Ty::Option(Box::new(elem.clone())),
            )
        }
        ("chain", [other]) => {
            let (o, ot) = flat_of(other, ctx, Some(&list))?;
            if !ot.same(&list) {
                return Break(Residue::Form("chain of another type".into()));
            }
            flat(format!("(prim append {r} {o} : {})", list.render()), list)
        }
        ("rev" | "enumerate", []) => {
            let head = ctx.fresh("item");
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
                return Continue(Some(walk(
                    ctx,
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
            let pair = ctx.types.tuple(vec![Ty::Nat(None), elem.clone()]);
            pair.mentions(&mut ctx.seen.borrow_mut());
            let index = ctx.fresh("index");
            let pair_name = pair.render();
            let item = tuple_value(&pair, &[index.clone(), head.clone()]);
            let cons = |rec: Recur<'_>, _: &str| {
                Expr::Flat(format!(
                    "(cons {item} {})",
                    rec(&[format!("({index} + 1)")])
                ))
            };
            Continue(Some(walk(
                ctx,
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
            let (n, nt) = flat_of(n, ctx, Some(&Ty::Nat(None)))?;
            if !matches!(nt, Ty::Nat(_)) {
                return Break(Residue::Form(format!(".{name} of another type")));
            }
            let head = ctx.fresh("item");
            let count = ctx.fresh("count");
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
            Continue(Some(walk(
                ctx,
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
            let lam = lambda(f, std::slice::from_ref(elem), ctx, Some(&Ty::Bool))?;
            if lam.ty != Ty::Bool {
                return Break(Residue::Form(format!(
                    "a .{name} predicate of another type"
                )));
            }
            let head = lam.binders[0].clone();
            let core = lam.core.clone();
            let e = elem.render();
            if name == "position" {
                let index = ctx.fresh("index");
                let cons = |rec: Recur<'_>, _: &str| {
                    Expr::Flat(format!(
                        "(if {core} then (ctor Option.some [Nat] {index}) else {})",
                        rec(&[format!("({index} + 1)")])
                    ))
                };
                return Continue(Some(walk(
                    ctx,
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
            Continue(Some(walk(
                ctx,
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
            let lam = lambda(f, std::slice::from_ref(elem), ctx, None)?;
            let inner = match (&lam.ty, name) {
                (Ty::Option(u), "filter_map" | "find_map") | (Ty::List(u), "flat_map") => {
                    (**u).clone()
                }
                _ => return Break(Residue::Form(format!("a .{name} step of another type"))),
            };
            let head = lam.binders[0].clone();
            let core = lam.core.clone();
            let found = ctx.fresh("found");
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
            Continue(Some(walk(
                ctx,
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
        _ => Continue(None),
    }
}

/// A match arm's body: an expression, or the block of an `if let`.
#[derive(Clone, Copy)]
enum Body<'a> {
    Expr(&'a syn::Expr),
    Block(&'a syn::Block),
    /// An arm with a guard: the guard, then the arm's expression.
    Guarded(&'a syn::Expr, &'a syn::Expr),
}

fn body(b: Body<'_>, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    match b {
        Body::Expr(e) | Body::Guarded(_, e) => expr(e, ctx, want),
        Body::Block(b) => block(&b.stmts, ctx, want),
    }
}

/// The binder of a sub-pattern: a name, or `_`.
fn binder(p: &syn::Pat) -> Option<Option<String>> {
    match p {
        syn::Pat::Ident(i) if i.subpat.is_none() && i.by_ref.is_none() => {
            Some(Some(i.ident.to_string()))
        }
        syn::Pat::Wild(_) => Some(None),
        syn::Pat::Reference(r) => binder(&r.pat),
        // A tuple of names, which the arm binds by projection.
        syn::Pat::Tuple(t) if t.elems.len() >= 2 => {
            let mut items = Vec::new();
            for e in &t.elems {
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

fn shape(p: &syn::Pat) -> Option<Shape> {
    match p {
        syn::Pat::Wild(_) => Some(Shape::Wild),
        syn::Pat::Ident(i) if i.ident == "None" && i.subpat.is_none() => {
            Some(Shape::Ctor("None".into(), Vec::new()))
        }
        syn::Pat::Path(pp) => {
            let segs: Vec<String> = pp
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if wrapper(&pp.path) == Some("None") {
                return Some(Shape::Ctor("None".into(), Vec::new()));
            }
            Some(Shape::Ctor(segs.join("::"), Vec::new()))
        }
        syn::Pat::TupleStruct(ts) => {
            let w = wrapper(&ts.path)?;
            let [inner] = ts.elems.iter().collect::<Vec<_>>()[..] else {
                return None;
            };
            Some(Shape::Ctor(w.into(), vec![binder(inner)?]))
        }
        syn::Pat::Lit(l) => match &l.lit {
            syn::Lit::Bool(b) => Some(Shape::Ctor(b.value.to_string(), Vec::new())),
            _ => None,
        },
        syn::Pat::Slice(s) => {
            let elems: Vec<&syn::Pat> = s.elems.iter().collect();
            match elems.as_slice() {
                [] => Some(Shape::Ctor("[]".into(), Vec::new())),
                [head, tail] => {
                    let tail = match tail {
                        syn::Pat::Rest(_) => None,
                        syn::Pat::Ident(i) => match &i.subpat {
                            Some((_, sub)) if matches!(**sub, syn::Pat::Rest(_)) => {
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
fn data_shape(p: &syn::Pat, st: &Ty, types: &Types) -> Option<Shape> {
    let Ty::Named(n) = st else { return None };
    let variants = types.datas.get(n)?;
    let path = match p {
        syn::Pat::TupleStruct(ts) => &ts.path,
        syn::Pat::Struct(ps) => &ps.path,
        _ => return None,
    };
    let variant = path.segments.last()?.ident.to_string();
    let (_, fields) = variants.iter().find(|(v, _)| *v == variant)?;
    let binders = match p {
        syn::Pat::TupleStruct(ts) => {
            let elems: Vec<&syn::Pat> = ts.elems.iter().collect();
            let rest = elems.iter().position(|e| matches!(e, syn::Pat::Rest(_)));
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
        syn::Pat::Struct(ps) => {
            let mut given = BTreeMap::new();
            for f in &ps.fields {
                let syn::Member::Named(id) = &f.member else {
                    return None;
                };
                given.insert(id.to_string(), binder(&f.pat)?);
            }
            if ps.rest.is_none() && given.len() != fields.len() {
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
    scrutinee: &syn::Expr,
    arms: &[(&syn::Pat, Body<'_>)],
    ctx: &Ctx<'_>,
    want: Option<&Ty>,
) -> Lift<(Expr, Ty)> {
    let (s, st) = flat_of(scrutinee, &ctx.below(), None)?;
    if matches!(st, Ty::Nat(_) | Ty::Int(_) | Ty::Str | Ty::Char)
        || arms.iter().any(|(_, b)| matches!(b, Body::Guarded(..)))
    {
        return literal_match(&s, &st, arms, ctx, want);
    }
    let mut shapes = Vec::new();
    for (pat, b) in arms {
        let cases: Vec<&syn::Pat> = match pat {
            syn::Pat::Or(o) => o.cases.iter().collect(),
            p => vec![*p],
        };
        for case in cases {
            let Some(sh) = data_shape(case, &st, ctx.types).or_else(|| shape(case)) else {
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
        let (then, tt) = body(t, ctx, want)?;
        let (other, ot) = body(f, ctx, Some(want.unwrap_or(&tt)))?;
        if !tt.same(&ot) {
            return Break(Residue::Form("arms of different types".to_string()));
        }
        return Continue((
            Expr::If {
                condition: Box::new(Expr::Flat(s)),
                then: Box::new(then),
                otherwise: Box::new(other),
            },
            tt,
        ));
    }
    let Some(ctors) = constructors(&st, ctx.types) else {
        return Break(Residue::Form(format!("match on {}", st.render())));
    };
    let mut out = Vec::new();
    let mut result: Option<Ty> = want.cloned();
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
                    let whole = ctx.fresh("pair");
                    for (k, (item, t)) in items.iter().zip(ts).enumerate() {
                        if *item == "_" {
                            continue;
                        }
                        let (next, name) = scope.bind(item, t.clone());
                        scope = next;
                        lets.push((name, t.render(), tuple_item(ty, &whole, k)));
                    }
                    whole
                }
                Some(ident) => {
                    let (next, name) = scope.bind(&ident, ty.clone());
                    scope = next;
                    name
                }
                None => ctx.fresh(if i == 0 { "value" } else { "rest" }),
            };
            names.push(name);
        }
        if lex == "List.cons" && ctx.params.contains(&s) {
            scope.tails.insert(names[1].clone(), s.clone());
        }
        // A field of the scrutinee's own type is a sub-term: a recursive call
        // on it descends.
        if ctx.params.contains(&s) && types_has_data(&st, ctx.types) {
            for (name, ty) in names.iter().zip(binder_tys) {
                if ty.same(&st) {
                    scope.tails.insert(name.clone(), s.clone());
                }
            }
        }
        let (e, t) = body(b, &scope, result.as_ref())?;
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
        Expr::Match {
            scrutinee: s,
            arms: out,
        },
        result,
    ))
}

/// A match on literals (integers, strings, characters and their ranges), or
/// with guards: a chain of `if`s ending in an arm that takes every value. An
/// arm may bind the scrutinee by name, and its guard joins its test.
fn literal_match(
    s: &str,
    st: &Ty,
    arms: &[(&syn::Pat, Body<'_>)],
    ctx: &Ctx<'_>,
    want: Option<&Ty>,
) -> Lift<(Expr, Ty)> {
    let bad = || Break(Residue::Form("match pattern".to_string()));
    let constant = |e: &syn::Expr| -> Option<String> {
        match e {
            syn::Expr::Lit(l) => match literal_of(&l.lit, Some(st), false) {
                Continue((Expr::Flat(k), kt)) if kt.same(st) => Some(k),
                _ => None,
            },
            syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => match &*u.expr {
                syn::Expr::Lit(l) => match literal_of(&l.lit, Some(st), true) {
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
            Ty::Int(_) => {
                ctx.axioms.borrow_mut().insert("propext".to_string());
                Some(format!("(not (prim less_than {b} {a} : Bool))"))
            }
            Ty::Str | Ty::Char => {
                ctx.axioms.borrow_mut().extend(ALL_AXIOMS.map(String::from));
                Some(format!("(not (prim less_than {b} {a} : Bool))"))
            }
            _ => None,
        }
    };
    // Each arm: its test (`None` takes every value), the name it binds the
    // scrutinee to, and its body.
    let mut tests: Vec<(Option<String>, Option<String>, Body<'_>)> = Vec::new();
    for (pat, b) in arms {
        let cases: Vec<&syn::Pat> = match pat {
            syn::Pat::Or(o) => o.cases.iter().collect(),
            p => vec![p],
        };
        let mut conds = Vec::new();
        let mut all = false;
        let mut bound = None;
        for case in cases {
            match case {
                syn::Pat::Wild(_) => all = true,
                syn::Pat::Ident(i) if i.subpat.is_none() && i.by_ref.is_none() => {
                    let id = i.ident.to_string();
                    if id == "None" || id.starts_with(char::is_uppercase) {
                        return bad();
                    }
                    all = true;
                    bound = Some(id);
                }
                syn::Pat::Lit(l) => {
                    let lit = syn::Expr::Lit(l.clone());
                    let Some(k) = constant(&lit) else {
                        return bad();
                    };
                    let Some(c) = equal(&k) else { return bad() };
                    conds.push(c);
                }
                syn::Pat::Range(rg) => {
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
                        parts.push(c);
                    }
                    if let Some(hi) = hi {
                        let c = if matches!(rg.limits, syn::RangeLimits::Closed(_)) {
                            at_most(s, &hi)
                        } else {
                            at_most(&hi, s).map(|c| format!("(not {c})"))
                        };
                        let Some(c) = c else { return bad() };
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
            let (gv, gt) = flat_of(g, &scope.below(), Some(&Ty::Bool))?;
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
        let (e, t) = body(*b, &scope, result.as_ref())?;
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
    Continue((out, t))
}

/// A branch followed by the statements after it, as one block expression.
fn then_rest(body: &syn::Expr, rest: &[syn::Stmt]) -> syn::Expr {
    let mut stmts = match body {
        syn::Expr::Block(b) if b.label.is_none() => b.block.stmts.clone(),
        syn::Expr::Tuple(t) if t.elems.is_empty() => Vec::new(),
        other => vec![syn::Stmt::Expr(
            other.clone(),
            Some(syn::token::Semi::default()),
        )],
    };
    stmts.extend(rest.iter().cloned());
    syn::Expr::Block(syn::ExprBlock {
        attrs: Vec::new(),
        label: None,
        block: syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts,
        },
    })
}

/// The `return v` a block is made of, alone: `{ return v; }`.
fn returned(b: &syn::Block) -> Option<&syn::Expr> {
    match b.stmts.as_slice() {
        [syn::Stmt::Expr(syn::Expr::Return(r), _)] => r.expr.as_deref(),
        _ => None,
    }
}

impl Ctx<'_> {
    /// The same scope, below the function's own statements: a `return`
    /// there is not an early exit the lift can follow.
    fn below(&self) -> Self {
        let mut next = self.clone();
        next.top = false;
        next
    }
}

/// The local a statement updates in place, and how: `x = e`, `x += e` on a
/// string, `x.push(e)` on a list or string, `x.push_str(e)`.
fn update_target(e: &syn::Expr) -> Option<String> {
    let ident = |e: &syn::Expr| match e {
        syn::Expr::Path(p) => p.path.get_ident().map(ToString::to_string),
        _ => None,
    };
    match e {
        syn::Expr::Assign(a) => ident(&a.left),
        syn::Expr::Binary(b) if matches!(b.op, syn::BinOp::AddAssign(_)) => ident(&b.left),
        syn::Expr::MethodCall(m)
            if (m.method == "push" || m.method == "push_str") && m.args.len() == 1 =>
        {
            ident(&m.receiver)
        }
        // The value of `insert` (whether it was new, or the replaced value)
        // is dropped where it is a statement of a loop.
        syn::Expr::MethodCall(m) if m.method == "insert" && (1..=2).contains(&m.args.len()) => {
            ident(&m.receiver)
        }
        _ => None,
    }
}

/// A character literal as the one-character string it pushes.
fn char_text(e: &syn::Expr) -> Option<String> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Char(c),
            ..
        }) => exact_string(&c.value().to_string()),
        _ => None,
    }
}

/// The new value of the local an update statement changes.
fn updated(e: &syn::Expr, ty: &Ty, current: &str, ctx: &Ctx<'_>) -> Lift<String> {
    let inner = ctx.below();
    let joined = |piece: String| {
        ctx.axioms.borrow_mut().insert("propext".to_string());
        format!("(prim join (cons {current} (cons {piece} (nil String))) \"\" : String)")
    };
    let value_of = |v: &syn::Expr, want: &Ty| -> Lift<String> {
        let (x, xt) = flat_of(v, &inner, Some(want))?;
        if xt.same(want) {
            Continue(x)
        } else {
            Break(Residue::Form("an update of another type".to_string()))
        }
    };
    match (e, ty) {
        (syn::Expr::Assign(a), _) => value_of(&a.right, ty),
        (syn::Expr::Binary(b), Ty::Str) => Continue(joined(value_of(&b.right, &Ty::Str)?)),
        (syn::Expr::Binary(b), _) => Break(Residue::Arithmetic(
            quote::ToTokens::to_token_stream(&b.op).to_string(),
        )),
        (syn::Expr::MethodCall(m), Ty::Set(t, true))
            if m.method == "insert" && m.args.len() == 1 =>
        {
            let v = value_of(&m.args[0], t)?;
            let helper = sorted_insert(ty, ctx)?;
            Continue(format!("({helper} {current} {v})"))
        }
        (syn::Expr::MethodCall(m), Ty::Map(k, v, true))
            if m.method == "insert" && m.args.len() == 2 =>
        {
            let key = value_of(&m.args[0], k)?;
            let value = value_of(&m.args[1], v)?;
            let helper = sorted_insert(ty, ctx)?;
            let entry = ty.entry();
            Continue(format!(
                "({helper} {current} {})",
                tuple_value(&entry, &[key, value])
            ))
        }
        (syn::Expr::MethodCall(m), Ty::List(t)) if m.method == "push" => {
            let v = value_of(&m.args[0], t)?;
            Continue(format!(
                "(prim append {current} (cons {v} (nil {})) : {})",
                t.render(),
                ty.render()
            ))
        }
        (syn::Expr::MethodCall(m), Ty::Str) => {
            let piece = match char_text(&m.args[0]) {
                Some(text) if m.method == "push" => text,
                _ if m.method == "push_str" => value_of(&m.args[0], &Ty::Str)?,
                _ => value_of(&m.args[0], &Ty::Char)?,
            };
            Continue(joined(piece))
        }
        _ => Break(Residue::Form("an update outside the lift".to_string())),
    }
}

/// The helper inserting into the sorted list an ordered set or map is held
/// as, keeping it sorted by key with each key once: a set ignores a member
/// it holds, a map replaces the value of a key it holds. `BTreeSet` and
/// `BTreeMap` order `String` keys by their bytes, which is the order of
/// their scalars, and naturals as numbers; other keys' order is not modelled.
fn sorted_insert(ty: &Ty, ctx: &Ctx<'_>) -> Lift<String> {
    let (key, entry) = match ty {
        Ty::Set(t, true) => ((**t).clone(), (**t).clone()),
        Ty::Map(k, _, true) => ((**k).clone(), ty.entry()),
        _ => return Break(Residue::Form("insert into an unordered collection".into())),
    };
    let list = Ty::List(Box::new(entry.clone()));
    let items = ctx.fresh("items");
    let item = ctx.fresh("item");
    let head = ctx.fresh("head");
    let rest = ctx.fresh("rest");
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
    let name = ctx.helper_name("Insert");
    entry.mentions(&mut ctx.seen.borrow_mut());
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
    ctx.axioms.borrow_mut().extend(axioms.iter().cloned());
    ctx.helpers.borrow_mut().push(Helper {
        name: name.clone(),
        params: vec![(items, list.render()), (item, entry.render())],
        result: list.render(),
        body,
        calls: BTreeSet::new(),
        axioms,
        recursive: true,
    });
    ctx.calls.borrow_mut().insert(format!("#{name}"));
    Continue(name)
}

/// The locals a loop body updates that are bound before the loop.
fn loop_state(body: &[syn::Stmt], ctx: &Ctx<'_>) -> Lift<Vec<String>> {
    struct Scan {
        updated: BTreeSet<String>,
        declared: BTreeSet<String>,
    }
    impl<'ast> syn::visit::Visit<'ast> for Scan {
        fn visit_expr(&mut self, e: &'ast syn::Expr) {
            if let Some(t) = update_target(e) {
                self.updated.insert(t);
            }
            syn::visit::visit_expr(self, e);
        }
        fn visit_local(&mut self, l: &'ast syn::Local) {
            if let syn::Pat::Ident(i) = &l.pat {
                self.declared.insert(i.ident.to_string());
            }
            syn::visit::visit_local(self, l);
        }
        fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
    }
    let mut scan = Scan {
        updated: BTreeSet::new(),
        declared: BTreeSet::new(),
    };
    for s in body {
        syn::visit::Visit::visit_stmt(&mut scan, s);
    }
    if scan.updated.iter().any(|u| scan.declared.contains(u)) {
        return Break(Residue::Form("a loop that shadows its state".to_string()));
    }
    let state: Vec<String> = scan
        .updated
        .into_iter()
        .filter(|u| ctx.locals.contains_key(u))
        .collect();
    if state.is_empty() {
        return Break(Residue::Form("a loop that updates no local".to_string()));
    }
    Continue(state)
}

/// The state of a loop as one value: the local, or the tuple of them.
fn state_expr(state: &[String]) -> syn::Expr {
    let path = |name: &str| -> syn::Expr {
        syn::parse_str(name).unwrap_or_else(|_| syn::parse_quote!(state))
    };
    match state {
        [one] => path(one),
        many => {
            let elems: Vec<syn::Expr> = many.iter().map(|n| path(n)).collect();
            syn::parse_quote!((#(#elems),*))
        }
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
fn rebind<'a>(
    ctx: &Ctx<'a>,
    state: &[String],
    whole: &str,
    lets: &mut Vec<(String, String, String)>,
) -> Ctx<'a> {
    let mut scope = ctx.clone();
    if let [one] = state {
        let ty = ctx.locals[one].1.clone();
        if let Some((old, _)) = scope.locals.insert(one.clone(), (whole.to_string(), ty)) {
            scope.hidden.push(old);
        }
        return scope;
    }
    for (i, var) in state.iter().enumerate() {
        let ty = ctx.locals[var].1.clone();
        let (next, name) = scope.bind(var, ty.clone());
        scope = next;
        lets.push((name, ty.render(), format!("(project {whole} item{i})")));
    }
    scope
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
fn loop_exit(ctx: &Ctx<'_>, label: bool, breaks: bool) -> Lift<(Expr, Ty)> {
    let Some(state) = &ctx.looping else {
        return Break(Residue::Form(
            "continue or break outside a loop step".to_string(),
        ));
    };
    if label {
        return Break(Residue::Form("labelled continue or break".to_string()));
    }
    let (v, t) = flat_of(&state_expr(state), ctx, None)?;
    let v = if breaks { format!("{BREAK}{v}") } else { v };
    Continue((Expr::Flat(v), t))
}

/// `for p in xs { body } rest`: a helper that walks `xs` with the locals the
/// body updates as its accumulator, then `rest` with their final values.
fn for_loop(
    f: &syn::ExprForLoop,
    rest: &[syn::Stmt],
    ctx: &Ctx<'_>,
    want: Option<&Ty>,
) -> Lift<(Expr, Ty)> {
    if f.label.is_some() {
        return Break(Residue::Form("labelled loop".to_string()));
    }
    let (list, lt) = flat_of(&f.expr, &ctx.below(), None)?;
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
    state_ty.mentions(&mut ctx.seen.borrow_mut());
    // The step, in a scope of its own.
    let calls = RefCell::new(BTreeSet::new());
    let axioms = RefCell::new(BTreeSet::new());
    let descends = RefCell::new(BTreeSet::new());
    let base = Ctx {
        calls: &calls,
        axioms: &axioms,
        descends: &descends,
        tails: BTreeMap::new(),
        top: false,
        looping: Some(state.clone()),
        ..ctx.clone()
    };
    // The outer names stay taken inside the step: the step's binders may
    // not shadow a capture.
    let mut base = base;
    for (rust, binding) in &ctx.locals {
        base.locals
            .insert(format!("#outer {rust}"), binding.clone());
    }
    let acc = base.fresh("state");
    let mut lets = Vec::new();
    let mut scope = rebind(&base, &state, &acc, &mut lets);
    let mut pat_lets = Vec::new();
    let (next, head) = bind_pat(&f.pat, &elem, &scope, &mut pat_lets, false)?;
    scope = next;
    let mut stmts = f.body.stmts.clone();
    stmts.push(syn::Stmt::Expr(state_expr(&state), None));
    let (step, st) = block(&stmts, &scope, Some(&state_ty))?;
    if !st.same(&state_ty) || !descends.borrow().is_empty() {
        return Break(Residue::Form("a loop step of another type".to_string()));
    }
    drop(scope);
    drop(base);
    let mut helpers = ctx.helpers.borrow_mut();
    let mut n = helpers.len() + 1;
    let name = loop {
        let candidate = format!("{}Loop{n}", ctx.own);
        if !ctx.fns.values().any(|g| g.name == candidate)
            && !helpers.iter().any(|h| h.name == candidate)
            && !ctx.taken(&candidate)
        {
            break candidate;
        }
        n += 1;
    };
    let items = ctx.fresh("items");
    let rest_name = ctx.fresh("rest");
    let text = step.flat();
    let caps = captures(ctx, &[text], std::slice::from_ref(&acc));
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
        (acc.clone(), state_ty.render()),
    ];
    params.extend(caps.iter().map(|(c, t)| (c.clone(), t.render())));
    helpers.push(Helper {
        name: name.clone(),
        params,
        result: state_ty.render(),
        body: Expr::Match {
            scrutinee: items,
            arms: vec![
                Arm {
                    pattern: "List.nil".to_string(),
                    body: Expr::Flat(acc),
                },
                Arm {
                    pattern: format!("List.cons {head} {rest_name}"),
                    body,
                },
            ],
        },
        calls: calls.into_inner(),
        axioms: axioms.into_inner(),
        recursive: true,
    });
    drop(helpers);
    ctx.calls.borrow_mut().insert(format!("#{name}"));
    // The loop's value, then the rest with the final state.
    let (init, _) = flat_of(&state_expr(&state), &ctx.below(), Some(&state_ty))?;
    let mut parts = vec![name, list, init];
    parts.extend(cap_names);
    let whole = ctx.fresh("state");
    let mut lets = Vec::new();
    let after = rebind(ctx, &state, &whole, &mut lets);
    let (rest, rt) = block(rest, &after, want)?;
    Continue((
        Expr::Let {
            name: whole,
            ty: state_ty.render(),
            value: Box::new(Expr::Flat(format!("({})", parts.join(" ")))),
            body: Box::new(wrap_lets(lets, rest)),
        },
        rt,
    ))
}

/// The first `?` an expression evaluates unconditionally, replaced by the
/// local `var`: its operand. A `?` under a closure, a branch or the right
/// side of `&&`/`||` may not run, so it stays.
fn take_try(e: &mut syn::Expr, var: &syn::Ident) -> Option<syn::Expr> {
    use syn::Expr as E;
    match e {
        E::Try(t) => {
            if let Some(inner) = take_try(&mut t.expr, var) {
                return Some(inner);
            }
            let inner = (*t.expr).clone();
            *e = syn::parse_quote!(#var);
            Some(inner)
        }
        E::MethodCall(m) => {
            if let Some(found) = take_try(&mut m.receiver, var) {
                return Some(found);
            }
            m.args.iter_mut().find_map(|a| take_try(a, var))
        }
        E::Call(c) => c.args.iter_mut().find_map(|a| take_try(a, var)),
        E::Field(f) => take_try(&mut f.base, var),
        E::Paren(p) => take_try(&mut p.expr, var),
        E::Group(g) => take_try(&mut g.expr, var),
        E::Reference(r) => take_try(&mut r.expr, var),
        E::Unary(u) => take_try(&mut u.expr, var),
        E::Cast(c) => take_try(&mut c.expr, var),
        E::Index(i) => take_try(&mut i.expr, var).or_else(|| take_try(&mut i.index, var)),
        E::Binary(b) => {
            if let Some(found) = take_try(&mut b.left, var) {
                return Some(found);
            }
            if matches!(b.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
                return None;
            }
            take_try(&mut b.right, var)
        }
        E::Tuple(t) => t.elems.iter_mut().find_map(|a| take_try(a, var)),
        E::Struct(st) => st
            .fields
            .iter_mut()
            .find_map(|f| take_try(&mut f.expr, var)),
        E::If(i) => take_try(&mut i.cond, var),
        E::Match(m) => take_try(&mut m.expr, var),
        _ => None,
    }
}

/// A statement with a `?` inside it, as `let v = e?;` before the statement
/// with `v` in its place.
fn hoist_try(stmt: &syn::Stmt, n: usize) -> Option<(syn::Stmt, syn::Stmt)> {
    let var = syn::Ident::new(&format!("tried{n}"), proc_macro2::Span::call_site());
    let mut stmt = stmt.clone();
    let inner = match &mut stmt {
        syn::Stmt::Expr(e, _) => take_try(e, &var)?,
        syn::Stmt::Local(l) => {
            let init = l.init.as_mut()?;
            if matches!(&*init.expr, syn::Expr::Try(_)) && matches!(l.pat, syn::Pat::Ident(_)) {
                return None;
            }
            take_try(&mut init.expr, &var)?
        }
        _ => return None,
    };
    let binding: syn::Stmt = syn::parse_quote!(let #var = #inner?;);
    Some((binding, stmt))
}

fn block(stmts: &[syn::Stmt], ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    match stmts.first() {
        Some(syn::Stmt::Expr(syn::Expr::Continue(c), _)) => {
            return loop_exit(ctx, c.label.is_some(), false);
        }
        Some(syn::Stmt::Expr(syn::Expr::Break(b), _)) if b.expr.is_none() => {
            return loop_exit(ctx, b.label.is_some(), true);
        }
        _ => {}
    }
    if ctx.top {
        if let Some((first, rest)) = stmts.split_first() {
            let n = *ctx.fresh.borrow() + 1;
            if let Some((binding, stmt)) = hoist_try(first, n) {
                *ctx.fresh.borrow_mut() = n;
                let mut out = vec![binding, stmt];
                out.extend(rest.iter().cloned());
                return block(&out, ctx, want);
            }
        }
    }
    match stmts {
        [syn::Stmt::Expr(e, None)] => expr(e, &ctx.below(), want),
        [syn::Stmt::Expr(syn::Expr::Return(r), _), ..] if ctx.top => match &r.expr {
            Some(v) => expr(v, &ctx.below(), Some(ctx.result)),
            None => Break(Residue::Type("()".to_string())),
        },
        // `if c { return v; } rest`: an early exit from the function.
        [syn::Stmt::Expr(syn::Expr::If(i), _), rest @ ..]
            if ctx.top
                && !rest.is_empty()
                && i.else_branch.is_none()
                && returned(&i.then_branch).is_some() =>
        {
            let Some(v) = returned(&i.then_branch) else {
                return Break(Residue::Statements);
            };
            let inner = ctx.below();
            if matches!(&*i.cond, syn::Expr::Let(_)) {
                let wild = syn::Pat::Wild(syn::PatWild {
                    attrs: Vec::new(),
                    underscore_token: syn::token::Underscore::default(),
                });
                let syn::Expr::Let(l) = &*i.cond else {
                    return Break(Residue::Statements);
                };
                let tail = syn::Block {
                    brace_token: syn::token::Brace::default(),
                    stmts: rest.to_vec(),
                };
                let early = syn::Block {
                    brace_token: syn::token::Brace::default(),
                    stmts: vec![syn::Stmt::Expr((*v).clone(), None)],
                };
                let arms = vec![(&*l.pat, Body::Block(&early)), (&wild, Body::Block(&tail))];
                return matching(&l.expr, &arms, ctx, want);
            }
            let (c, ct) = expr(&i.cond, &inner, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Break(Residue::Form("non-boolean condition".to_string()));
            }
            let (then, tt) = expr(v, &inner, Some(ctx.result))?;
            let (other, ot) = block(rest, ctx, want)?;
            if !tt.same(ctx.result) || !ot.same(&tt) {
                return Break(Residue::Form("branches of different types".to_string()));
            }
            Continue((
                Expr::If {
                    condition: Box::new(Expr::Flat(flat_text(c)?)),
                    then: Box::new(then),
                    otherwise: Box::new(other),
                },
                tt,
            ))
        }
        // `let P = e else { return v; }; rest`: a match with an early exit.
        [syn::Stmt::Local(local), rest @ ..]
            if ctx.top
                && !rest.is_empty()
                && local.init.as_ref().is_some_and(|i| i.diverge.is_some()) =>
        {
            let Some(init) = &local.init else {
                return Break(Residue::Statements);
            };
            let Some((_, otherwise)) = &init.diverge else {
                return Break(Residue::Statements);
            };
            let syn::Expr::Block(eb) = &**otherwise else {
                return Break(Residue::Form("let else".to_string()));
            };
            let Some(v) = returned(&eb.block) else {
                return Break(Residue::Form("let else".to_string()));
            };
            let wild = syn::Pat::Wild(syn::PatWild {
                attrs: Vec::new(),
                underscore_token: syn::token::Underscore::default(),
            });
            let tail = syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: rest.to_vec(),
            };
            let early = syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: vec![syn::Stmt::Expr((*v).clone(), None)],
            };
            let arms = vec![
                (&local.pat, Body::Block(&tail)),
                (&wild, Body::Block(&early)),
            ];
            matching(&init.expr, &arms, ctx, want)
        }
        // `let (a, b) = e; rest`: the tuple, then its items.
        [syn::Stmt::Local(local), rest @ ..]
            if !rest.is_empty()
                && matches!(local.pat, syn::Pat::Tuple(_))
                && local.init.as_ref().is_some_and(|i| {
                    i.diverge.is_none() && !matches!(*i.expr, syn::Expr::Try(_))
                }) =>
        {
            let Some(init) = &local.init else {
                return Break(Residue::Statements);
            };
            let (value, vt) = flat_of(&init.expr, &ctx.below(), None)?;
            let mut lets = Vec::new();
            let (scope, whole) = bind_pat(&local.pat, &vt, ctx, &mut lets, false)?;
            let (body, bt) = block(rest, &scope, want)?;
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
                Expr::Let {
                    name: whole,
                    ty: vt.render(),
                    value: Box::new(Expr::Flat(value)),
                    body: Box::new(body),
                },
                bt,
            ))
        }
        // `for p in xs { .. }`: a walk with the updated locals as its state.
        [syn::Stmt::Expr(syn::Expr::ForLoop(f), _), rest @ ..] if !rest.is_empty() => {
            for_loop(f, rest, ctx, want)
        }
        // `x = e;`, `x += s;`, `x.push(e);`: the local's next value.
        [syn::Stmt::Expr(e, Some(_)), rest @ ..]
            if !rest.is_empty() && update_target(e).is_some() =>
        {
            let Some(target) = update_target(e) else {
                return Break(Residue::Statements);
            };
            let Some((current, ty)) = ctx.locals.get(&target).cloned() else {
                return Break(Residue::Form(format!("an update of {target}")));
            };
            let value = updated(e, &ty, &current, ctx)?;
            let (scope, name) = ctx.bind(&target, ty.clone());
            let (rest, rt) = block(rest, &scope, want)?;
            Continue((
                Expr::Let {
                    name,
                    ty: ty.render(),
                    value: Box::new(Expr::Flat(value)),
                    body: Box::new(rest),
                },
                rt,
            ))
        }
        // `()` as a statement does nothing.
        [syn::Stmt::Expr(syn::Expr::Tuple(t), _), rest @ ..]
            if t.elems.is_empty() && !rest.is_empty() =>
        {
            block(rest, ctx, want)
        }
        // `match e { p => .. } rest`: the rest after each arm.
        [syn::Stmt::Expr(syn::Expr::Match(m), _), rest @ ..] if !rest.is_empty() => {
            let bodies: Vec<syn::Expr> = m.arms.iter().map(|a| then_rest(&a.body, rest)).collect();
            let arms: Vec<(&syn::Pat, Body<'_>)> = m
                .arms
                .iter()
                .zip(&bodies)
                .map(|(a, b)| match &a.pat {
                    syn::Pat::Guard(g) => (&*g.pat, Body::Guarded(&g.guard, b)),
                    p => (p, Body::Expr(b)),
                })
                .collect();
            matching(&m.expr, &arms, ctx, want)
        }
        // `if let p = e { .. } [else { .. }] rest`: the rest after each branch.
        [syn::Stmt::Expr(syn::Expr::If(i), _), rest @ ..]
            if !rest.is_empty()
                && matches!(&*i.cond, syn::Expr::Let(_))
                && returned(&i.then_branch).is_none() =>
        {
            let syn::Expr::Let(l) = &*i.cond else {
                return Break(Residue::Statements);
            };
            let then = then_rest(
                &syn::Expr::Block(syn::ExprBlock {
                    attrs: Vec::new(),
                    label: None,
                    block: i.then_branch.clone(),
                }),
                rest,
            );
            let unit: syn::Expr = syn::parse_quote!(());
            let other = then_rest(i.else_branch.as_ref().map_or(&unit, |(_, e)| &**e), rest);
            let wild = syn::Pat::Wild(syn::PatWild {
                attrs: Vec::new(),
                underscore_token: syn::token::Underscore::default(),
            });
            let arms = vec![(&*l.pat, Body::Expr(&then)), (&wild, Body::Expr(&other))];
            matching(&l.expr, &arms, ctx, want)
        }
        // `if c { .. } [else { .. }] rest` with statements in its branches: the
        // rest after each branch.
        [syn::Stmt::Expr(syn::Expr::If(i), _), rest @ ..]
            if !rest.is_empty()
                && !matches!(&*i.cond, syn::Expr::Let(_))
                && returned(&i.then_branch).is_none() =>
        {
            let (c, ct) = expr(&i.cond, &ctx.below(), Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Break(Residue::Form("non-boolean condition".to_string()));
            }
            let mut then: Vec<syn::Stmt> = i.then_branch.stmts.clone();
            then.extend(rest.iter().cloned());
            let mut other: Vec<syn::Stmt> = match &i.else_branch {
                None => Vec::new(),
                Some((_, e)) => match &**e {
                    syn::Expr::Block(b) => b.block.stmts.clone(),
                    syn::Expr::If(_) => vec![syn::Stmt::Expr((**e).clone(), None)],
                    _ => return Break(Residue::Statements),
                },
            };
            other.extend(rest.iter().cloned());
            let (a, at) = block(&then, ctx, want)?;
            let (b, bt) = block(&other, ctx, Some(want.unwrap_or(&at)))?;
            if !at.same(&bt) {
                return Break(Residue::Form("branches of different types".to_string()));
            }
            Continue((
                Expr::If {
                    condition: Box::new(Expr::Flat(flat_text(c)?)),
                    then: Box::new(a),
                    otherwise: Box::new(b),
                },
                at,
            ))
        }
        [syn::Stmt::Local(local), rest @ ..] if !rest.is_empty() => {
            let (ident, declared) = match &local.pat {
                syn::Pat::Ident(i) if i.subpat.is_none() && i.by_ref.is_none() => (i, None),
                syn::Pat::Type(pt) => match &*pt.pat {
                    syn::Pat::Ident(i) if i.subpat.is_none() && i.by_ref.is_none() => {
                        (i, Some(ctx.types.ty(&pt.ty)?))
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
            if let syn::Expr::Try(t) = &*init.expr {
                if !ctx.top {
                    return Break(Residue::Form(
                        "? below the function's own statements".to_string(),
                    ));
                }
                return question(&t.expr, &ident.ident.to_string(), declared, rest, ctx, want);
            }
            // An empty vector the block goes on to return takes the type the
            // block returns.
            let hint = match &declared {
                Some(d) => Some(d.clone()),
                None if is_empty_vec(&init.expr) => {
                    returned_type(&ident.ident.to_string(), rest, ctx, want)
                }
                None => None,
            };
            let (value, vt) = flat_of(&init.expr, &ctx.below(), hint.as_ref())?;
            if declared.as_ref().is_some_and(|d| !d.same(&vt)) {
                return Break(Residue::Form("let of another type".to_string()));
            }
            let (scope, name) = ctx.bind(&ident.ident.to_string(), vt.clone());
            let (rest, rt) = block(rest, &scope, want)?;
            Continue((
                Expr::Let {
                    name,
                    ty: vt.render(),
                    value: Box::new(Expr::Flat(value)),
                    body: Box::new(rest),
                },
                rt,
            ))
        }
        _ => Break(Residue::Statements),
    }
}

/// `Vec::new()` or `Vec::with_capacity(n)`: a vector whose element type the
/// expression does not say.
fn is_empty_vec(e: &syn::Expr) -> bool {
    let syn::Expr::Call(c) = e else {
        return false;
    };
    let syn::Expr::Path(p) = &*c.func else {
        return false;
    };
    let names: Vec<String> = p
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
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
fn returned_type(ident: &str, rest: &[syn::Stmt], ctx: &Ctx<'_>, want: Option<&Ty>) -> Option<Ty> {
    let is_local = |e: &syn::Expr| matches!(e, syn::Expr::Path(p) if p.path.is_ident(ident));
    if let Some(syn::Stmt::Expr(e, None)) = rest.last() {
        if is_local(e) {
            return want.cloned();
        }
    }
    struct Returns<'i> {
        ident: &'i str,
        found: bool,
    }
    impl<'ast> syn::visit::Visit<'ast> for Returns<'_> {
        fn visit_expr_return(&mut self, r: &'ast syn::ExprReturn) {
            if let Some(e) = &r.expr {
                if matches!(&**e, syn::Expr::Path(p) if p.path.is_ident(self.ident)) {
                    self.found = true;
                }
            }
            syn::visit::visit_expr_return(self, r);
        }
        fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
    }
    let mut returns = Returns {
        ident,
        found: false,
    };
    for stmt in rest {
        syn::visit::Visit::visit_stmt(&mut returns, stmt);
    }
    returns.found.then(|| ctx.result.clone())
}

/// `let x = e?; rest`: a match that returns the failure and continues with
/// the value.
fn question(
    e: &syn::Expr,
    ident: &str,
    declared: Option<Ty>,
    rest: &[syn::Stmt],
    ctx: &Ctx<'_>,
    want: Option<&Ty>,
) -> Lift<(Expr, Ty)> {
    let (s, st) = flat_of(e, &ctx.below(), None)?;
    let (inner, early) = match (&st, ctx.result) {
        (Ty::Option(inner), Ty::Option(r)) => (
            (**inner).clone(),
            (
                "Option.none".to_string(),
                format!("(ctor Option.none [{}])", r.render()),
            ),
        ),
        (Ty::Outcome(inner, err), Ty::Outcome(ok, rerr)) if err.same(rerr) => {
            let e = ctx.fresh("error");
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
    let (scope, name) = ctx.bind(ident, inner.clone());
    let (rest, rt) = block(rest, &scope, want)?;
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
    Continue((Expr::Match { scrutinee: s, arms }, rt))
}

/// A candidate function: its LexLean name, signature and body.
pub(crate) struct Candidate<'a> {
    pub(crate) rust: String,
    pub(crate) name: String,
    pub(crate) params: Vec<Param>,
    pub(crate) result: Ty,
    pub(crate) block: &'a syn::Block,
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
            let descends = RefCell::new(BTreeSet::new());
            let calls = RefCell::new(BTreeSet::new());
            let fresh = RefCell::new(0);
            let seen = RefCell::new(BTreeSet::new());
            let axioms = RefCell::new(BTreeSet::new());
            let helpers = RefCell::new(Vec::new());
            let ctx = Ctx {
                types,
                fns: &fns,
                current: &c.rust,
                params: &names,
                result: &c.result,
                locals: c
                    .params
                    .iter()
                    .map(|(rust, n, t)| (rust.clone(), (n.clone(), t.clone())))
                    .collect(),
                tails: BTreeMap::new(),
                descends: &descends,
                calls: &calls,
                fresh: &fresh,
                seen: &seen,
                axioms: &axioms,
                own: &c.name,
                helpers: &helpers,
                top: true,
                looping: None,
                hidden: Vec::new(),
            };
            let lifted = block(&c.block.stmts, &ctx, Some(&c.result));
            drop(ctx);
            let outcome = match lifted {
                Continue((body, ty)) if ty.same(&c.result) => {
                    let descends = descends.into_inner();
                    if descends.len() > 1 {
                        Break(Residue::Form(
                            "recursion on more than one parameter".to_string(),
                        ))
                    } else {
                        Continue((body, descends.into_iter().next().map(|i| names[i].clone())))
                    }
                }
                Continue(_) => Break(Residue::Form("a body of another type".to_string())),
                Break(r) => Break(r),
            };
            let outcome = match outcome {
                Continue((body, Some(arg))) => well_founded(c, &body, &arg, types).map_continue(
                    |(recursive, termination, theorems)| (body, recursive, termination, theorems),
                ),
                Continue((body, None)) => Continue((body, None, None, Vec::new())),
                Break(r) => Break(r),
            };
            match outcome {
                Continue((body, recursive, termination, theorems)) => {
                    // A well-founded definition elaborates with its evidence.
                    let mut evidence_axioms = BTreeSet::new();
                    for t in theorems {
                        if let Decl::Theorem { axioms, .. } = &t {
                            evidence_axioms.extend(axioms.iter().cloned());
                        }
                        decls.push((format!("#{}", t.name()), t));
                    }
                    graph.insert(c.rust.clone(), calls.into_inner());
                    let mut seen = seen.into_inner();
                    for (_, _, t) in &c.params {
                        t.mentions(&mut seen);
                    }
                    c.result.mentions(&mut seen);
                    mentioned.insert(c.rust.clone(), seen);
                    let mut axioms = axioms.into_inner();
                    axioms.extend(evidence_axioms);
                    own.insert(c.rust.clone(), axioms);
                    for h in helpers.into_inner() {
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
fn literal(e: &syn::Expr, want: &Ty, types: &Types) -> Option<String> {
    match e {
        syn::Expr::Lit(lit) => match literal_of(&lit.lit, Some(want), false) {
            Continue((Expr::Flat(s), t)) if t.same(want) => Some(s),
            _ => None,
        },
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => match &*u.expr {
            syn::Expr::Lit(lit) => match literal_of(&lit.lit, Some(want), true) {
                Continue((Expr::Flat(s), t)) if t.same(want) => Some(s),
                _ => None,
            },
            _ => None,
        },
        syn::Expr::Path(p) => {
            let (owner, variant) = ctor(&p.path, types)?;
            (Ty::Named(owner.clone()) == *want).then(|| format!("(ctor {owner}.{variant})"))
        }
        syn::Expr::Paren(p) => literal(&p.expr, want, types),
        syn::Expr::Reference(r) => literal(&r.expr, want, types),
        _ => None,
    }
}

/// `f(lit, ..)` against a lifted function: the applied term, the result
/// type and the function's Rust name.
fn applied(
    e: &syn::Expr,
    lifted: &BTreeMap<String, Sig>,
    types: &Types,
) -> Option<(String, Ty, String)> {
    let syn::Expr::Call(c) = e else {
        return None;
    };
    let syn::Expr::Path(p) = &*c.func else {
        return None;
    };
    let name = p.path.segments.last()?.ident.to_string();
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
    args: &[syn::Expr],
    lifted: &BTreeMap<String, Sig>,
    types: &Types,
) -> Option<(String, String)> {
    match (kind, args) {
        ("assert", [single, ..]) => {
            let (inner, value) = match single {
                syn::Expr::Unary(syn::ExprUnary {
                    op: syn::UnOp::Not(_),
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
        let (params, result) = match types.signature(&f.sig, None) {
            Continue(sig) => sig,
            Break(r) => return Err(r),
        };
        let c = Candidate {
            rust: f.sig.ident.to_string(),
            name: camel(&[&f.sig.ident.to_string()]),
            params,
            result,
            block: &f.block,
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
        let ty: syn::Type = syn::parse_str("u32").expect("type");
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
