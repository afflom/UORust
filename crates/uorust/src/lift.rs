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
            Self::Nat(_) => "Nat".into(),
            Self::Int(_) => "Int".into(),
            Self::Str | Self::Char => "String".into(),
            Self::ParseError => "ParseIntError".into(),
            Self::Named(n) => n.clone(),
            Self::Option(t) => format!("Option{}", t.tag()),
            Self::Outcome(t, e) => format!("Result{}{}", t.tag(), e.tag()),
            Self::List(t) | Self::Set(t, _) => format!("List{}", t.tag()),
            Self::Tuple(ts) => format!("Tuple{}", ts.iter().map(|t| t.tag()).collect::<String>()),
            Self::Pair(ts) => format!("Pair{}", ts.iter().map(|t| t.tag()).collect::<String>()),
            Self::Map(k, v, _) => format!("ListTuple{}{}", k.tag(), v.tag()),
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
    /// The traits each type derives (`Ord`, `PartialEq`, ...), by LexLean
    /// name: a derived order or equality is the structural one.
    pub(crate) derives: BTreeMap<String, BTreeSet<String>>,
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
            rs::TypeKind::Tuple(elems) if elems.is_empty() => Ok(Ty::Unit),
            rs::TypeKind::Tuple(elems) if elems.len() >= 2 => {
                let mut ts = Vec::new();
                for e in elems {
                    ts.push(self.ty(e)?);
                }
                Ok(self.tuple(ts))
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
    /// How many fresh binders the body has taken.
    fresh: usize,
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
    });
    let acc = acc.called(format!("#{name}"));
    Ok(((format!("({})", parts.join(" ")), t), acc))
}

/// The pointer width of a target, from the architecture its triple names
/// first, as rustc's target specifications set `target_pointer_width`.
pub(crate) fn pointer_width(triple: &str) -> Option<u32> {
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
) -> Lift<String> {
    let is_len = |e: &rs::RustExpr| matches!(e, rs::RustExpr::MethodCall(m) if m.method == "len" && m.args.is_empty());
    let narrowest = env.widths.first().map(|(w, _)| *w);
    let half = narrowest.and_then(|w| 1u128.checked_shl(w - 1));
    let bounded = |len: &rs::RustExpr, k: &rs::RustExpr| {
        is_len(len) && matches!((int_literal(k), half), (Some(k), Some(h)) if k <= h)
    };
    if bounded(left, right) || bounded(right, left) {
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
        _ => expr_form(e, env, ctx, acc, want),
    }
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
    let flat = |s: String, t: Ty, acc: Acc| Ok(((Expr::Flat(s), t), acc));
    match e {
        rs::RustExpr::Lit(lit) => literal_of(lit, want, false).map(|v| (v, acc)),
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
                        return Err(Residue::Form("a constant that names itself".to_string()));
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
                    _ => Err(Residue::Form("None of an unknown type".to_string())),
                };
            }
            match ctor(p, &env.types) {
                Some((owner, variant)) => {
                    flat(format!("(ctor {owner}.{variant})"), Ty::Named(owner), acc)
                }
                None => Err(Residue::Form(format!("free name {}", p.text))),
            }
        }
        rs::RustExpr::Unary(u) => match u.op {
            rs::UnOp::Not => {
                let ((x, t), acc) = flat_of(&u.expr, env, ctx, acc, Some(&Ty::Bool))?;
                if t != Ty::Bool {
                    return Err(Residue::Form("bitwise not".to_string()));
                }
                flat(format!("(not {x})"), Ty::Bool, acc)
            }
            rs::UnOp::Neg => match &*u.expr {
                rs::RustExpr::Lit(lit) => literal_of(lit, want, true).map(|v| (v, acc)),
                _ => Err(Residue::Arithmetic("-".to_string())),
            },
            rs::UnOp::Deref => expr(&u.expr, env, ctx, acc, want),
        },
        rs::RustExpr::If(i) => {
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
            let ((c, ct), acc) = expr(&i.cond, env, ctx, acc, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Err(Residue::Form("non-boolean condition".to_string()));
            }
            let ((then, tt), (other, ot), acc) =
                match block(&i.then_branch.stmts, env, ctx, acc.clone(), want) {
                    // A branch whose type only the other says: that one first.
                    Err(r) if want.is_none() && untyped(&r) => {
                        let ((other, ot), acc) = expr(otherwise, env, ctx, acc, None)?;
                        let (then, acc) = block(&i.then_branch.stmts, env, ctx, acc, Some(&ot))?;
                        (then, (other, ot), acc)
                    }
                    lifted => {
                        let ((then, tt), acc) = lifted?;
                        let (other, acc) =
                            expr(otherwise, env, ctx, acc, Some(want.unwrap_or(&tt)))?;
                        ((then, tt), other, acc)
                    }
                };
            if !tt.same(&ot) {
                return Err(Residue::Form("branches of different types".to_string()));
            }
            Ok((
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
                        return Err(Residue::Form("field of an unknown struct".to_string()));
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
                        None => Err(Residue::Form("tuple field".to_string())),
                    }
                }
                _ => Err(Residue::Form("field access".to_string())),
            }
        }
        // `E::V { a, b }`: a struct variant, its fields in declaration order.
        rs::RustExpr::Struct(s)
            if s.rest.is_none() && !s.qself && data_variant(&s.path, &env.types).is_some() =>
        {
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
            flat(format!("({})", parts.join(" ")), Ty::Named(owner), acc)
        }
        // `S { a: x, .. }`, or `S { a: x, ..base }` where `base` gives the
        // fields not written.
        rs::RustExpr::Struct(s) if !s.qself => {
            let Some(written) = s.path.segments.last().map(|seg| seg.ident.clone()) else {
                return Err(Residue::Form("struct path".to_string()));
            };
            let owner = ctx.current.split_once("::").map(|(o, _)| o.to_string());
            let name = match (written.as_str(), owner) {
                ("Self", Some(o)) => env.types.resolve(&o),
                _ => env.types.resolve(&written),
            };
            let local = s.path.segments.len() == 1 || written == "Self";
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
                    let ((bv, bt), next) =
                        flat_of(b, env, ctx, acc, Some(&Ty::Named(name.clone())))?;
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
                Ok(t) => t,
                Err(_) => return Err(Residue::Form("cast".to_string())),
            };
            match (&vt, &target) {
                (Ty::Nat(Some(a)), Ty::Nat(Some(b))) if a <= b => flat(v, target, acc),
                (Ty::Int(a), Ty::Int(b)) if a <= b => flat(v, target, acc),
                _ => Err(Residue::Form("narrowing or sign-changing cast".to_string())),
            }
        }
        // `xs[i]`: the element, and where Rust's bounds check panics, the
        // type's default, as Lean's `xs[i]!` is.
        rs::RustExpr::Index(ix) => {
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
        rs::RustExpr::Continue(label) => loop_exit(env, ctx, acc, *label, false),
        rs::RustExpr::Break(b) if b.expr.is_none() => loop_exit(env, ctx, acc, b.label, true),
        rs::RustExpr::Try(_) => Err(Residue::Form("? outside a let".to_string())),
        // `matches!(e, P)`: a match with `true` for `P` and `false` otherwise.
        rs::RustExpr::Macro(m) if m.path.is_ident("matches") => {
            let Some((scrutinee, pat)) = &m.matches else {
                return Err(Residue::Form("matches! with a guard".to_string()));
            };
            let yes = rs::RustExpr::Lit(rs::Lit::Bool(true));
            let no = rs::RustExpr::Lit(rs::Lit::Bool(false));
            let wild = rs::Pat::Wild;
            let arms = vec![(&**pat, Body::Expr(&yes)), (&wild, Body::Expr(&no))];
            matching(scrutinee, &arms, env, ctx, acc, Some(&Ty::Bool))
        }
        rs::RustExpr::Macro(m) if m.path.is_ident("vec") => {
            let Some(items) = &m.args else {
                return Err(Residue::Form("vec! of a repeated value".to_string()));
            };
            list_of(items, env, ctx, acc, want)
        }
        rs::RustExpr::Array(items) => list_of(items, env, ctx, acc, want),
        _ => Err(Residue::Form(e.kind())),
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

/// `format!` whose holes are all strings: the pieces joined.
fn format_macro(mac: &rs::ExprMacro, env: &Env, ctx: &Ctx, acc: Acc) -> Lift<((Expr, Ty), Acc)> {
    let bad = |what: &str| Err(Residue::Form(format!("format! {what}")));
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
    let flat = |s: String, t: Ty, acc: Acc| Ok(((Expr::Flat(s), t), acc));
    let op_text = || b.op.text().to_string();
    match b.op {
        B::And | B::Or => {
            let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, Some(&Ty::Bool))?;
            let ((r, rt), acc) = flat_of(&b.right, env, ctx, acc, Some(&Ty::Bool))?;
            if lt != Ty::Bool || rt != Ty::Bool {
                return Err(Residue::Form("logic on non-booleans".to_string()));
            }
            let op = if matches!(b.op, B::And) { "&&" } else { "||" };
            flat(format!("({l} {op} {r})"), Ty::Bool, acc)
        }
        B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge => comparison(b, env, ctx, acc),
        B::Add => {
            let want = Ty::Nat(None);
            let ((l, lt), acc) = flat_of(&b.left, env, ctx, acc, Some(&want))?;
            let ((r, rt), acc) = flat_of(&b.right, env, ctx, acc, Some(&want))?;
            if lt != want || rt != want {
                return Err(Residue::Arithmetic(op_text()));
            }
            flat(usize_sum(&b.left, &b.right, &l, &r, env)?, want, acc)
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
                _ => Err(Residue::Arithmetic(op_text())),
            }
        }
        _ => Err(Residue::Arithmetic(op_text())),
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
            rs::Pat::Type(pt) => (&*pt.pat, Some(env.types.ty(&pt.ty)?)),
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
    let flat = |s: String, t: Ty, acc: Acc| Ok(((Expr::Flat(s), t), acc));
    let rs::RustExpr::Path(p) = &*c.func else {
        return Err(Residue::Form("call of an expression".to_string()));
    };
    if let Some(closure) = p.get_ident().and_then(|i| ctx.closures.get(i)) {
        return applied_closure(closure, &c.args, env, ctx, acc, want);
    }
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
                    return Err(Residue::Form("result of another type".to_string()));
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
            _ => Err(Residue::Form("result of an unknown type".to_string())),
        };
    }
    // `E::V(a, b)`: a variant of a data-carrying enum.
    if let Some((owner, variant, fields)) = data_variant(p, &env.types) {
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
        return flat(format!("({})", parts.join(" ")), Ty::Named(owner), acc);
    }
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
            return flat(s, byte_list, acc);
        }
    }
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
            return flat(format!("({helper} {v} (nil {}))", elem.render()), set, acc);
        }
        ([s, n], [a], _) if s == "Box" && n == "new" => {
            let ((v, vt), acc) = flat_of(a, env, ctx, acc, want)?;
            return flat(v, vt, acc);
        }
        ([s, n], [a], _) if s == "String" && n == "from" => {
            let ((v, vt), acc) = flat_of(a, env, ctx, acc, Some(&Ty::Str))?;
            if vt == Ty::Str {
                return flat(v, Ty::Str, acc);
            }
            return Err(Residue::Form("String::from of another type".to_string()));
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
    flat(format!("({})", parts.join(" ")), sig.result.clone(), acc)
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
    let named = match mac.args.as_deref() {
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

/// The axioms of the runtime's string length and decimal formatting.
const ALL_AXIOMS: [&str; 3] = ["Classical.choice", "Quot.sound", "propext"];

fn method(
    m: &rs::ExprMethodCall,
    env: &Env,
    ctx: &Ctx,
    acc: Acc,
    want: Option<&Ty>,
) -> Lift<((Expr, Ty), Acc)> {
    let flat = |s: String, t: Ty, acc: Acc| Ok(((Expr::Flat(s), t), acc));
    let mut acc = acc;
    let name = m.method.to_string();
    // `bytes.iter().map(|b| format!("{b:02x}")).collect()` into a `String`:
    // the bytes as lowercase hex.
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
                return flat(s, Ty::Str, acc);
            }
        }
    }
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
            return flat(format!("({})", parts.join(" ")), sig.result.clone(), acc);
        }
    }
    if matches!(rt, Ty::Map(..) | Ty::Set(..)) {
        return collection(&name, &r, &rt, &args, env, ctx, acc);
    }
    if matches!(rt, Ty::Str | Ty::Char) {
        let (lifted, after) = text_method(m, &name, &r, &rt, &args, env, ctx, acc, want)?;
        if let Some(lifted) = lifted {
            return Ok((lifted, after));
        }
        acc = after;
    }
    if let Ty::List(elem) = &rt {
        let (lifted, after) = combinator(m, &name, &r, elem, &args, env, ctx, acc)?;
        if let Some(lifted) = lifted {
            return Ok((lifted, after));
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
                return Err(Residue::Width(format!(".{name} on usize")));
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
                return Err(Residue::Width(format!(".{name} on usize")));
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
                return Err(Residue::Width(format!(".{name} on usize")));
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
                return Err(Residue::Width(format!(".{name} on i{bits}")));
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
                return Err(Residue::Width(format!(".{name} on i{bits}")));
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
        (Ty::Str, "to_string" | "to_owned")
        | (Ty::List(_), "as_slice")
        | (Ty::Option(_), "as_deref")
        | (_, "clone" | "copied" | "cloned" | "to_vec")
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
            Ok((
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
            Ok((
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
            Ok((
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
            Ok((
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
        _ => Err(Residue::Form(format!("method call .{name}"))),
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
            Ok((scope, name, acc, lets))
        }
        (rs::Pat::Wild, _) => {
            let (name, acc) = ctx.fresh(env, acc, "item");
            Ok((ctx.clone(), name, acc, lets))
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
            Ok((scope, whole, acc, lets))
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
        let (mut next, name, after, more) = bind_pat(p, ty, env, &scope, acc, lets, false)?;
        // An element of a list that is a sub-term of a parameter is one.
        if let (Some(param), true) = (&ctx.elements, binders.is_empty()) {
            next.tails.insert(name.clone(), param.clone());
        }
        scope = next;
        acc = after;
        lets = more;
        binders.push(name);
    }
    let ((core, ty), acc) = flat_of(&c.body, env, &scope, acc, want)?;
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
        entwined: !lam.descends.is_empty(),
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
        ("collect", []) => {
            let to_string = matches!(
                m.turbofish.as_deref(),
                Some([rs::GenericArgument::Type(t)]) if t.text == "String"
            );
            if to_string {
                if *elem != Ty::Str {
                    return Err(Residue::Form("collect of non-strings into a String".into()));
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
                return Err(Residue::Form(
                    "join with a separator of another type".into(),
                ));
            }
            let acc = acc.axiom("propext");
            flat(format!("(prim join {r} {s} : String)"), Ty::Str, acc)
        }
        ("contains", [needle]) => Ok(some(lookup(
            env, ctx, acc, "Contains", r, elem, None, needle,
        )?)),
        ("map" | "filter" | "any" | "all" | "find", [f]) => {
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
                    _ => format!(
                        "(if {core} then (ctor Option.some [{elem_text}] {head}) else {next})"
                    ),
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
        ("fold", [init, f]) => {
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
                return Err(Residue::Form("nth of another type".into()));
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
                return Err(Residue::Form("chain of another type".into()));
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
        ("skip" | "take", [n]) => {
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
        _ => Ok((None, acc)),
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
    let refuse = || Err(Residue::Form("match pattern".to_string()));
    let Some((first, rest)) = rows.split_first() else {
        return Err(Residue::Form("non-exhaustive match".to_string()));
    };
    // The first column the first row tests, and the names it binds before.
    let mut binds = first.binds.clone();
    let mut tested = None;
    for (j, (cell, col)) in first.cells.iter().zip(cols).enumerate() {
        let Some((names, h)) = head(*cell, &col.ty, &env.types) else {
            return refuse();
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
        // The first row matches every value: its arm, when its guard holds.
        // A name bound to a part of the scrutinee is a binder of its own,
        // so what the arm lifts to names it.
        let mut scope = ctx.clone();
        let mut lets = Vec::new();
        let mut acc = acc;
        for (rust, value, ty) in binds {
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
        let Body::Guarded(g, e) = first.body else {
            let ((e, t), acc) = body(first.body, env, &scope, acc, want)?;
            return Ok(((wrap_lets(lets, e), t), acc));
        };
        let ((gv, gt), acc) = flat_of(g, env, &scope.below(), acc, Some(&Ty::Bool))?;
        if gt != Ty::Bool {
            return Err(Residue::Form("non-boolean guard".to_string()));
        }
        let ((then, tt), acc) = expr(e, env, &scope, acc, want)?;
        let ((other, ot), acc) = compile(cols, rest, env, ctx, acc, Some(want.unwrap_or(&tt)))?;
        if !tt.same(&ot) {
            return Err(Residue::Form("arms of different types".to_string()));
        }
        let e = Expr::If {
            condition: Box::new(Expr::Flat(gv)),
            then: Box::new(then),
            otherwise: Box::new(other),
        };
        return Ok(((wrap_lets(lets, e), tt), acc));
    };
    let col = &cols[j];
    let mut heads = Vec::new();
    for row in rows {
        let Some(hs) = heads_at(row, j, col, &env.types) else {
            return refuse();
        };
        heads.extend(hs);
    }
    let around = |middle: Vec<Column>| -> Vec<Column> {
        let mut out = cols[..j].to_vec();
        out.extend(middle);
        out.extend(cols[j + 1..].iter().cloned());
        out
    };
    // A tuple or a structure: a column for each item.
    let items: Option<Vec<Column>> = match &col.ty {
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
    };
    if let Some(items) = items {
        let mut next = Vec::new();
        for (row, h) in &heads {
            let cells = match h {
                Head::Any => vec![Cell::Wild; items.len()],
                Head::Items(cs) if cs.len() == items.len() => cs.clone(),
                _ => return refuse(),
            };
            next.push(Clause {
                cells: replaced(row, j, cells),
                ..row.clone()
            });
        }
        return compile(&around(items), &next, env, ctx, acc, want);
    }
    // A literal or a range: a test, then the rows after it.
    if let Some((row, Head::Test(_))) = heads.first() {
        // The alternatives of the first arm that test this column and agree
        // elsewhere: one test, their disjunction.
        let mut tests = Vec::new();
        let mut acc = acc;
        let mut taken = 0;
        for (other, h) in &heads {
            let Head::Test(p) = h else { break };
            let alike = other.arm == row.arm
                && other.binds == row.binds
                && (0..cols.len()).all(|i| i == j || other.cells[i] == row.cells[i]);
            if !alike {
                break;
            }
            let Some((test, after)) = literal_test(&col.value, &col.ty, p, acc) else {
                return refuse();
            };
            acc = after;
            tests.push(test);
            taken += 1;
        }
        let Some(test) = tests.into_iter().reduce(|a, b| format!("({a} || {b})")) else {
            return refuse();
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
        return Ok(((e, tt), acc));
    }
    // The rows a constructor's values reach, its fields' cells in place
    // of the column's.
    let reach = |key: &str, arity: usize| -> Option<Vec<Clause<'_>>> {
        let mut out = Vec::new();
        for (row, h) in &heads {
            let cells = match h {
                Head::Any => vec![Cell::Wild; arity],
                Head::Ctor(k, cs) if k == key && cs.len() == arity => cs.clone(),
                Head::Ctor(k, _) if k != key => continue,
                _ => return None,
            };
            out.push(Clause {
                cells: replaced(row, j, cells),
                ..row.clone()
            });
        }
        Some(out)
    };
    if col.ty == Ty::Bool {
        let (Some(yes), Some(no)) = (reach("true", 0), reach("false", 0)) else {
            return refuse();
        };
        let ((then, tt), acc) = compile(&around(Vec::new()), &yes, env, ctx, acc, want)?;
        let want = Some(want.unwrap_or(&tt));
        let ((other, ot), acc) = compile(&around(Vec::new()), &no, env, ctx, acc, want)?;
        if !tt.same(&ot) {
            return Err(Residue::Form("arms of different types".to_string()));
        }
        let e = Expr::If {
            condition: Box::new(Expr::Flat(col.value.clone())),
            then: Box::new(then),
            otherwise: Box::new(other),
        };
        return Ok(((e, tt), acc));
    }
    let Some(ctors) = constructors(&col.ty, &env.types) else {
        return Err(Residue::Form(format!("match on {}", col.ty.render())));
    };
    let mut out = Vec::new();
    let mut result: Option<Ty> = want.cloned();
    let mut deferred = Vec::new();
    let mut acc = acc;
    for (key, lex, field_tys) in &ctors {
        let Some(next) = reach(key, field_tys.len()) else {
            return refuse();
        };
        // Each field's binder: the name the first row reaching it gives it,
        // or a fresh one.
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
        let binders: Vec<String> = names.iter().map(|c| c.value.clone()).collect();
        let pattern = std::iter::once(lex.clone())
            .chain(binders)
            .collect::<Vec<_>>()
            .join(" ");
        let cols = around(names);
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
        rs::RustExpr::Binary(b) if b.op == rs::BinOp::AddAssign => ident(&b.left),
        rs::RustExpr::MethodCall(m)
            if matches!(m.method.as_str(), "push" | "push_str" | "extend") && m.args.len() == 1 =>
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
    if updated.iter().any(|u| declared.contains(u)) {
        return Err(Residue::Form("a loop that shadows its state".to_string()));
    }
    let state: Vec<String> = updated
        .into_iter()
        .filter(|u| ctx.locals.contains_key(u))
        .collect();
    if state.is_empty() {
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
    let (mut scope, head, acc, pat_lets) =
        bind_pat(&f.pat, &elem, env, &scope, acc, Vec::new(), false)?;
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
        entwined,
    });
    let acc = acc.called(format!("#{name}"));
    // The loop's value, then the rest with the final state.
    let ((init, _), acc) = flat_of(&state_expr(&state), env, &ctx.below(), acc, Some(&state_ty))?;
    let mut parts = vec![name, list, init];
    parts.extend(cap_names);
    let (whole, acc) = ctx.fresh(env, acc, "state");
    let (after, acc, lets) = rebind(env, ctx, acc, &state, &whole, Vec::new());
    let ((rest, rt), acc) = block(rest, env, &after, acc, want)?;
    Ok((
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
        [] if want == Some(&Ty::Unit) => Ok(((Expr::Flat("()".into()), Ty::Unit), acc)),
        [rs::Stmt::Expr(e, false)] => expr(e, env, &ctx.below(), acc, want),
        [rs::Stmt::Expr(rs::RustExpr::Return(r), _), ..] if ctx.top => match r {
            Some(v) => expr(v, env, &ctx.below(), acc, Some(&ctx.result)),
            None if ctx.result == Ty::Unit => Ok(((Expr::Flat("()".into()), Ty::Unit), acc)),
            None => Err(Residue::Type("()".to_string())),
        },
        // `if c { return v; } rest`: an early exit from the function.
        [rs::Stmt::Expr(rs::RustExpr::If(i), _), rest @ ..]
            if ctx.top
                && !rest.is_empty()
                && i.else_branch.is_none()
                && returned(&i.then_branch).is_some() =>
        {
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
                    stmts: vec![rs::Stmt::Expr((*v).clone(), false)],
                };
                let arms = vec![(&l.pat, Body::Block(&early)), (&wild, Body::Block(&tail))];
                return matching(&l.expr, &arms, env, ctx, acc, want);
            }
            let ((c, ct), acc) = expr(&i.cond, env, &inner, acc, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Err(Residue::Form("non-boolean condition".to_string()));
            }
            let ((then, tt), acc) = expr(v, env, &inner, acc, Some(&ctx.result))?;
            let ((other, ot), acc) = block(rest, env, ctx, acc, want)?;
            if !tt.same(&ctx.result) || !ot.same(&tt) {
                return Err(Residue::Form("branches of different types".to_string()));
            }
            Ok((
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
                return Err(Residue::Statements);
            };
            let Some(otherwise) = &init.diverge else {
                return Err(Residue::Statements);
            };
            let rs::RustExpr::Block(eb) = &**otherwise else {
                return Err(Residue::Form("let else".to_string()));
            };
            let Some(v) = returned(&eb.block) else {
                return Err(Residue::Form("let else".to_string()));
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
                return Err(Residue::Statements);
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
        // `for p in xs { .. }`: a walk with the updated locals as its state.
        [rs::Stmt::Expr(rs::RustExpr::ForLoop(f), _), rest @ ..] if !rest.is_empty() => {
            for_loop(f, rest, env, ctx, acc, want)
        }
        // `x = e;`, `x += s;`, `x.push(e);`: the local's next value.
        [rs::Stmt::Expr(e, true), rest @ ..] if !rest.is_empty() && update_target(e).is_some() => {
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
                return Err(Residue::Statements);
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
                return Err(Residue::Form("non-boolean condition".to_string()));
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
                None if is_empty_vec(&init.expr) => {
                    returned_type(&ident.ident.clone(), rest, ctx, want)
                        .or_else(|| used_type(&ident.ident, &init.expr, rest, env, ctx))
                }
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
        _ => Err(Residue::Statements),
    }
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
                if let rs::Pat::Ident(i) = &l.pat {
                    match static_type(&init.expr, env, &scope) {
                        Some(t) => scope.insert(i.ident.clone(), t),
                        None => scope.remove(&i.ident),
                    };
                }
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
        rs::RustExpr::Path(p) => locals.get(p.get_ident()?).cloned(),
        rs::RustExpr::Reference(r) => static_type(&r.expr, env, locals),
        rs::RustExpr::Paren(x) => static_type(x, env, locals),
        rs::RustExpr::MethodCall(m)
            if m.args.is_empty()
                && matches!(
                    m.method.as_str(),
                    "iter" | "into_iter" | "clone" | "to_owned" | "cloned"
                ) =>
        {
            static_type(&m.receiver, env, locals)
        }
        rs::RustExpr::MethodCall(m) => match static_type(&m.receiver, env, locals)? {
            Ty::Named(owner) => env
                .fns
                .get(&format!("{owner}::{}", m.method))
                .map(|s| s.result.clone()),
            _ => None,
        },
        rs::RustExpr::Call(c) => match &*c.func {
            rs::RustExpr::Path(p) => env
                .fns
                .get(&p.segments.last()?.ident)
                .map(|s| s.result.clone()),
            _ => None,
        },
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
        let env = Env {
            types: types.clone(),
            fns,
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
                current: c.rust.clone(),
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
                own: c.name.clone(),
                top: true,
                looping: None,
                hidden: Vec::new(),
                closures: BTreeMap::new(),
                imports: c.imports.clone(),
            };
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
                    graph.insert(c.rust.clone(), acc.calls);
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
            for (key, d) in &mut decls {
                if let Decl::Definition { axioms: a, .. } = d {
                    a.clone_from(&axioms[key.as_str()]);
                }
            }
            axioms.retain(|key, _| !key.starts_with('#'));
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
                decls: grouped(ordered),
                failed,
                types,
                axioms,
                opaque,
                ordered: compares.into_values().flatten().collect(),
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
    Ok((None, Some((format!("({size} {arg})"), evidence)), theorems))
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
