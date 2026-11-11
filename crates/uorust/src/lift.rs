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

use crate::lex::{camel, Decl, Expr};

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
    /// A unit-variant enum or a struct.
    Named(String),
    Option(Box<Ty>),
    /// `Result` of a value and an error.
    Outcome(Box<Ty>, Box<Ty>),
    List(Box<Ty>),
    Tuple(Vec<Ty>),
}

impl Ty {
    /// The LexLean type.
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Bool => "Bool".into(),
            Self::Nat(_) => "Nat".into(),
            Self::Int(_) => "Int64".into(),
            Self::Str => "String".into(),
            Self::Named(n) => n.clone(),
            Self::Option(t) => format!("(Option {})", t.render()),
            Self::Outcome(t, e) => format!("(Result {} {})", t.render(), e.render()),
            Self::List(t) => format!("(List {})", t.render()),
            Self::Tuple(ts) => tuple_name(ts),
        }
    }

    fn tag(&self) -> String {
        match self {
            Self::Bool => "Bool".into(),
            Self::Nat(_) => "Nat".into(),
            Self::Int(_) => "Int".into(),
            Self::Str => "String".into(),
            Self::Named(n) => n.clone(),
            Self::Option(t) => format!("Option{}", t.tag()),
            Self::Outcome(t, e) => format!("Result{}{}", t.tag(), e.tag()),
            Self::List(t) => format!("List{}", t.tag()),
            Self::Tuple(ts) => tuple_name(ts),
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
            Self::Option(t) | Self::List(t) => t.mentions(out),
            Self::Outcome(t, e) => {
                t.mentions(out);
                e.mentions(out);
            }
            _ => {}
        }
    }
}

/// The structure a tuple type lifts to: `TupleNatBool` for `(u32, bool)`.
pub(crate) fn tuple_name(ts: &[Ty]) -> String {
    let tags: String = ts.iter().map(Ty::tag).collect();
    format!("Tuple{tags}")
}

/// The unit-variant enums and the structs the lift can name.
#[derive(Clone, Debug, Default)]
pub(crate) struct Types {
    pub(crate) enums: BTreeMap<String, Vec<String>>,
    /// Struct name -> fields `(LexLean name, type)` in declaration order.
    pub(crate) structs: BTreeMap<String, Vec<(String, Ty)>>,
}

impl Types {
    /// The structs whose every field lifts, to the fixpoint where a struct
    /// only names enums and structs that lift.
    pub(crate) fn new(
        enums: BTreeMap<String, Vec<String>>,
        structs: &BTreeMap<String, Vec<(String, syn::Type)>>,
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
                };
            }
            names = lifted.into_keys().collect();
        }
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
                Continue(Ty::Tuple(ts))
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
                    ("Option", [t]) => Ty::Option(Box::new(t.clone())),
                    ("Result", [t, e]) => Ty::Outcome(Box::new(t.clone()), Box::new(e.clone())),
                    ("Vec", [t]) => Ty::List(Box::new(t.clone())),
                    ("Box", [t]) => t.clone(),
                    (n, []) if self.enums.contains_key(n) || self.structs.contains_key(n) => {
                        Ty::Named(n.to_string())
                    }
                    _ => return bad(),
                };
                Continue(ty)
            }
            _ => bad(),
        }
    }

    /// The LexLean signature of a candidate, or why it is not one.
    pub(crate) fn signature(&self, sig: &syn::Signature) -> Lift<(Vec<Param>, Ty)> {
        if !sig.generics.params.is_empty()
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
                syn::FnArg::Receiver(_) => {
                    return Break(Residue::Signature("takes self".to_string()));
                }
                syn::FnArg::Typed(pat) => {
                    let syn::Pat::Ident(name) = &*pat.pat else {
                        return Break(Residue::Signature("pattern parameter".to_string()));
                    };
                    let rust = name.ident.to_string();
                    params.push((rust.clone(), local_name(&rust), self.ty(&pat.ty)?));
                }
            }
        }
        let result = match &sig.output {
            syn::ReturnType::Default => return Break(Residue::Type("()".to_string())),
            syn::ReturnType::Type(_, t) => self.ty(t)?,
        };
        Continue((params, result))
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
}

impl Ctx<'_> {
    fn taken(&self, name: &str) -> bool {
        self.locals.values().any(|(n, _)| n == name) || self.fns.values().any(|f| f.name == name)
    }

    /// A LexLean binder for a Rust name: a binder may not shadow another.
    fn bind(&self, ident: &str, ty: Ty) -> (Self, String) {
        let mut name = local_name(ident);
        if self.taken(&name) {
            name = self.fresh(&name);
        }
        let mut next = self.clone();
        next.locals.insert(ident.to_string(), (name.clone(), ty));
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

fn flat_of(e: &syn::Expr, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(String, Ty)> {
    let (x, t) = expr(e, ctx, want)?;
    Continue((flat_text(x)?, t))
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

/// A LexLean string literal holding exactly `text`, when it has no control
/// characters.
fn exact_string(text: &str) -> Option<String> {
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    ))
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
        _ => Break(Residue::Form("literal".to_string())),
    }
}

fn ctor(path: &syn::Path, types: &Types) -> Option<(String, String)> {
    let n = path.segments.len();
    if n < 2 {
        return None;
    }
    let owner = path.segments[n - 2].ident.to_string();
    let variant = path.segments[n - 1].ident.to_string();
    types
        .enums
        .get(&owner)
        .filter(|vs| vs.contains(&variant))
        .map(|_| (owner, variant))
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
                .map(|a| (&a.pat, Body::Expr(&a.body)))
                .collect();
            matching(&m.expr, &arms, ctx, want)
        }
        syn::Expr::Call(c) => call(c, ctx, want),
        syn::Expr::MethodCall(m) => method(m, ctx),
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
                    flat(format!("(project {base} {name})"), ty.clone())
                }
                (syn::Member::Unnamed(i), Ty::Tuple(ts)) => match ts.get(i.index as usize) {
                    Some(ty) => flat(format!("(project {base} item{})", i.index), ty.clone()),
                    None => Break(Residue::Form("tuple field".to_string())),
                },
                _ => Break(Residue::Form("field access".to_string())),
            }
        }
        syn::Expr::Struct(s) if s.rest.is_none() && s.qself.is_none() => {
            let Some(name) = s.path.get_ident().map(ToString::to_string) else {
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
                parts.push(format!("{field} := {v}"));
            }
            flat(
                format!("(record {name} {{{}}})", parts.join(", ")),
                Ty::Named(name),
            )
        }
        syn::Expr::Tuple(t) if t.elems.len() >= 2 => {
            let wants: Vec<Option<&Ty>> = match want {
                Some(Ty::Tuple(ts)) if ts.len() == t.elems.len() => ts.iter().map(Some).collect(),
                _ => vec![None; t.elems.len()],
            };
            let mut parts = Vec::new();
            let mut tys = Vec::new();
            for (i, (e, w)) in t.elems.iter().zip(wants).enumerate() {
                let (v, vt) = flat_of(e, ctx, w)?;
                parts.push(format!("item{i} := {v}"));
                tys.push(vt);
            }
            flat(
                format!("(record {} {{{}}})", tuple_name(&tys), parts.join(", ")),
                Ty::Tuple(tys),
            )
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
        syn::Expr::Try(_) => Break(Residue::Form("? outside a let".to_string())),
        syn::Expr::Macro(m) if m.mac.path.is_ident("format") => format_macro(&m.mac, ctx),
        _ => Break(Residue::Form(kind_of(e))),
    }
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
                if value.1 != Ty::Str {
                    return bad("hole of another type");
                }
                pieces.push(value.0);
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
                    format!("({l} =? {r})")
                }
                (Ty::Bool | Ty::Named(_), B::Ne(_)) if is_enum_or_bool(&lt, ctx) => {
                    format!("(not ({l} =? {r}))")
                }
                (Ty::Int(_) | Ty::Str, B::Eq(_)) => format!("(prim equal {l} {r} : Bool)"),
                (Ty::Int(_) | Ty::Str, B::Ne(_)) => format!("(not (prim equal {l} {r} : Bool))"),
                (Ty::Int(_), B::Lt(_) | B::Gt(_) | B::Le(_) | B::Ge(_)) => {
                    // Int64's order is decided through propositional
                    // extensionality.
                    ctx.axioms.borrow_mut().insert("propext".to_string());
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
    let name = p
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
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

fn method(m: &syn::ExprMethodCall, ctx: &Ctx<'_>) -> Lift<(Expr, Ty)> {
    let flat = |s: String, t: Ty| Continue((Expr::Flat(s), t));
    let name = m.method.to_string();
    let (r, rt) = flat_of(&m.receiver, ctx, None)?;
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
                        ("Option.none".into(), Expr::Flat(n.into())),
                        (format!("Option.some {v}"), Expr::Flat(s.into())),
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
                        ("Option.none".into(), Expr::Flat(d)),
                        (format!("Option.some {v}"), Expr::Flat(v.clone())),
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
                        (format!("Result.ok {v}"), Expr::Flat(o.into())),
                        (format!("Result.error {e}"), Expr::Flat(x.into())),
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
                        (format!("Result.ok {v}"), Expr::Flat(some(t, &v))),
                        (format!("Result.error {e}"), Expr::Flat(none(t))),
                    ],
                },
                opt(t),
            ))
        }
        _ => Break(Residue::Form(format!("method call .{name}"))),
    }
}

/// A match arm's body: an expression, or the block of an `if let`.
#[derive(Clone, Copy)]
enum Body<'a> {
    Expr(&'a syn::Expr),
    Block(&'a syn::Block),
}

fn body(b: Body<'_>, ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    match b {
        Body::Expr(e) => expr(e, ctx, want),
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

/// The constructors of a scrutinee type: `(Rust shape key, LexLean
/// constructor, binder types)`.
fn constructors(t: &Ty, types: &Types) -> Option<Vec<(String, String, Vec<Ty>)>> {
    Some(match t {
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
    let (s, st) = flat_of(scrutinee, ctx, None)?;
    if matches!(st, Ty::Nat(_) | Ty::Int(_)) {
        return literal_match(&s, &st, arms, ctx, want);
    }
    let mut shapes = Vec::new();
    for (pat, b) in arms {
        let Some(sh) = shape(pat) else {
            return Break(Residue::Form("match pattern".to_string()));
        };
        shapes.push((sh, *b));
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
        for (i, ty) in binder_tys.iter().enumerate() {
            let ident = binders.as_ref().and_then(|bs| bs.get(i).cloned().flatten());
            let name = match ident {
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
        let (e, t) = body(b, &scope, result.as_ref())?;
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
        out.push((pattern, e));
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

/// A match on integer literals: a chain of `if`s ending in the wildcard arm.
fn literal_match(
    s: &str,
    st: &Ty,
    arms: &[(&syn::Pat, Body<'_>)],
    ctx: &Ctx<'_>,
    want: Option<&Ty>,
) -> Lift<(Expr, Ty)> {
    let bad = || Break(Residue::Form("match pattern".to_string()));
    let mut tests = Vec::new();
    let mut otherwise = None;
    for (pat, b) in arms {
        if otherwise.is_some() {
            return bad();
        }
        let cases: Vec<&syn::Pat> = match pat {
            syn::Pat::Or(o) => o.cases.iter().collect(),
            syn::Pat::Wild(_) => {
                otherwise = Some(*b);
                continue;
            }
            p => vec![p],
        };
        let mut conds = Vec::new();
        for case in cases {
            let syn::Pat::Lit(l) = case else {
                return bad();
            };
            let (k, kt) = match literal_of(&l.lit, Some(st), false) {
                Continue((Expr::Flat(k), kt)) => (k, kt),
                _ => return bad(),
            };
            if !kt.same(st) {
                return bad();
            }
            conds.push(if matches!(st, Ty::Int(_)) {
                format!("(prim equal {s} {k} : Bool)")
            } else {
                format!("({s} =? {k})")
            });
        }
        let cond = conds
            .into_iter()
            .reduce(|a, b| format!("({a} || {b})"))
            .unwrap_or_default();
        tests.push((cond, *b));
    }
    let Some(last) = otherwise else {
        return Break(Residue::Form("non-exhaustive match".to_string()));
    };
    let mut result = want.cloned();
    let mut bodies = Vec::new();
    for (_, b) in &tests {
        let (e, t) = body(*b, ctx, result.as_ref())?;
        if result.as_ref().is_some_and(|r| !r.same(&t)) {
            return Break(Residue::Form("arms of different types".to_string()));
        }
        result = Some(t);
        bodies.push(e);
    }
    let (mut out, t) = body(last, ctx, result.as_ref())?;
    if result.as_ref().is_some_and(|r| !r.same(&t)) {
        return Break(Residue::Form("arms of different types".to_string()));
    }
    for ((cond, _), e) in tests.into_iter().zip(bodies).rev() {
        out = Expr::If {
            condition: Box::new(Expr::Flat(cond)),
            then: Box::new(e),
            otherwise: Box::new(out),
        };
    }
    Continue((out, t))
}

fn block(stmts: &[syn::Stmt], ctx: &Ctx<'_>, want: Option<&Ty>) -> Lift<(Expr, Ty)> {
    match stmts {
        [syn::Stmt::Expr(e, None)] => expr(e, ctx, want),
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
                return question(&t.expr, &ident.ident.to_string(), declared, rest, ctx, want);
            }
            let (value, vt) = flat_of(&init.expr, ctx, declared.as_ref())?;
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
    let (s, st) = flat_of(e, ctx, None)?;
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
        vec![(early.0, Expr::Flat(early.1)), (ok, rest)]
    } else {
        vec![(ok, rest), (early.0, Expr::Flat(early.1))]
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
}

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
            };
            let lifted = block(&c.block.stmts, &ctx, Some(&c.result));
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
            match outcome {
                Continue((body, recursive)) => {
                    graph.insert(c.rust.clone(), calls.into_inner());
                    let mut seen = seen.into_inner();
                    for (_, _, t) in &c.params {
                        t.mentions(&mut seen);
                    }
                    c.result.mentions(&mut seen);
                    mentioned.insert(c.rust.clone(), seen);
                    own.insert(c.rust.clone(), axioms.into_inner());
                    decls.push(Decl::Definition {
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
                    });
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
            for (c, d) in candidates
                .iter()
                .filter(|c| !failed.contains_key(&c.rust))
                .zip(decls.iter_mut())
            {
                if let Decl::Definition { axioms: a, .. } = d {
                    a.clone_from(&axioms[&c.rust]);
                }
            }
            return Lifted {
                decls,
                failed,
                types,
                axioms,
            };
        }
    }
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
        let (params, result) = match types.signature(&f.sig) {
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
            None => Ok(crate::lex::render_decl(&out.decls[0])),
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
        let types = Types::new(BTreeMap::new(), &raw);
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
}
