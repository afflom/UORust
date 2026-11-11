//! Function bodies to LexLean definitions, where the lift is mechanical.
//!
//! The floor is the total, pure fragment whose meaning does not depend on the
//! machine: `bool` logic, comparisons of unsigned integers, unit-variant enums,
//! `if`/`match` and calls among lifted functions. Anything else — arithmetic
//! that wraps or panics, references, statements, methods — is not guessed at:
//! the function becomes a residue row with a typed reason.

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
    /// A parameter or the result has a type outside the floor.
    Type(String),
    /// The body has statements before its value.
    Statements,
    /// Integer arithmetic, whose overflow behaviour is a build setting.
    Arithmetic(String),
    /// An expression form outside the floor.
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
            Self::Form(_) => "form",
            Self::Call(_) => "call",
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Signature(s) => format!("signature outside the floor: {s}"),
            Self::Type(t) => format!("type outside the floor: {t}"),
            Self::Statements => "the body has statements before its value".to_string(),
            Self::Arithmetic(op) => {
                format!("integer operator {op} wraps or panics depending on the build profile")
            }
            Self::Form(f) => format!("expression form outside the floor: {f}"),
            Self::Call(c) => format!("calls {c}, which is not lifted"),
        }
    }
}

/// A unit-variant enum the lift can name as an inductive.
pub(crate) type Enums = BTreeMap<String, Vec<String>>;

fn ty(t: &syn::Type, enums: &Enums) -> Lift<String> {
    let text = crate::ingest::tidy(&quote::ToTokens::to_token_stream(t).to_string());
    match t {
        syn::Type::Path(p) if p.qself.is_none() => {
            let Some(ident) = p.path.get_ident() else {
                return Break(Residue::Type(text));
            };
            let name = ident.to_string();
            match name.as_str() {
                "bool" => Continue("Bool".to_string()),
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => Continue("Nat".to_string()),
                _ if enums.contains_key(&name) => Continue(name),
                _ => Break(Residue::Type(text)),
            }
        }
        _ => Break(Residue::Type(text)),
    }
}

/// The LexLean signature of a candidate, or why it is not one.
pub(crate) fn signature(
    sig: &syn::Signature,
    enums: &Enums,
) -> Lift<(Vec<(String, String)>, String)> {
    if !sig.generics.params.is_empty()
        || sig.asyncness.is_some()
        || sig.unsafety.is_some()
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
                params.push((camel(&[&name.ident.to_string()]), ty(&pat.ty, enums)?));
            }
        }
    }
    let result = match &sig.output {
        syn::ReturnType::Default => return Break(Residue::Type("()".to_string())),
        syn::ReturnType::Type(_, t) => ty(t, enums)?,
    };
    Continue((params, result))
}

struct Ctx<'a> {
    enums: &'a Enums,
    /// Candidate functions: Rust name -> LexLean name.
    lifted: &'a BTreeMap<String, String>,
    params: BTreeSet<String>,
}

fn expr(e: &syn::Expr, ctx: &Ctx<'_>) -> Lift<Expr> {
    let flat = |s: String| Continue(Expr::Flat(s));
    match e {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Bool(b) => flat(b.value.to_string()),
            syn::Lit::Int(i)
                if matches!(i.suffix(), "" | "u8" | "u16" | "u32" | "u64" | "usize") =>
            {
                match i.base10_parse::<u128>() {
                    Ok(v) => flat(v.to_string()),
                    Err(_) => Break(Residue::Form("integer literal".to_string())),
                }
            }
            _ => Break(Residue::Form("literal".to_string())),
        },
        syn::Expr::Paren(p) => expr(&p.expr, ctx),
        syn::Expr::Group(g) => expr(&g.expr, ctx),
        syn::Expr::Block(b) if b.label.is_none() => block(&b.block, ctx),
        syn::Expr::Path(p) => {
            if let Some(ident) = p.path.get_ident() {
                let name = camel(&[&ident.to_string()]);
                if ctx.params.contains(&name) {
                    return flat(name);
                }
                return Break(Residue::Form(format!("free name {ident}")));
            }
            match ctor(&p.path, ctx) {
                Some(c) => flat(format!("(ctor {c})")),
                None => Break(Residue::Form("path".to_string())),
            }
        }
        syn::Expr::Unary(u) => match u.op {
            syn::UnOp::Not(_) => flat(format!("(not {})", flat_of(&u.expr, ctx)?)),
            _ => Break(Residue::Form("unary operator".to_string())),
        },
        syn::Expr::Binary(b) => {
            let l = flat_of(&b.left, ctx)?;
            let r = flat_of(&b.right, ctx)?;
            let text = match b.op {
                syn::BinOp::And(_) => format!("({l} && {r})"),
                syn::BinOp::Or(_) => format!("({l} || {r})"),
                syn::BinOp::Eq(_) => format!("({l} =? {r})"),
                syn::BinOp::Ne(_) => format!("(not ({l} =? {r}))"),
                syn::BinOp::Lt(_) => format!("({l} <? {r})"),
                syn::BinOp::Le(_) => format!("({l} <=? {r})"),
                syn::BinOp::Gt(_) => format!("({r} <? {l})"),
                syn::BinOp::Ge(_) => format!("({r} <=? {l})"),
                other => {
                    let op =
                        crate::ingest::tidy(&quote::ToTokens::to_token_stream(&other).to_string());
                    return Break(Residue::Arithmetic(op));
                }
            };
            flat(text)
        }
        syn::Expr::If(i) => {
            let Some((_, otherwise)) = &i.else_branch else {
                return Break(Residue::Form("if without else".to_string()));
            };
            Continue(Expr::If {
                condition: Box::new(expr(&i.cond, ctx)?),
                then: Box::new(block(&i.then_branch, ctx)?),
                otherwise: Box::new(expr(otherwise, ctx)?),
            })
        }
        syn::Expr::Match(m) => matching(m, ctx),
        syn::Expr::Call(c) => {
            let syn::Expr::Path(p) = &*c.func else {
                return Break(Residue::Form("call of an expression".to_string()));
            };
            let name = p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            let Some(target) = ctx.lifted.get(&name) else {
                return Break(Residue::Call(name));
            };
            let mut parts = vec![target.clone()];
            for arg in &c.args {
                parts.push(flat_of(arg, ctx)?);
            }
            flat(format!("({})", parts.join(" ")))
        }
        syn::Expr::MethodCall(m) => Break(Residue::Form(format!("method call .{}", m.method))),
        _ => Break(Residue::Form(kind_of(e))),
    }
}

fn kind_of(e: &syn::Expr) -> String {
    let text = format!("{e:?}");
    text.split(['(', ' ', '{'])
        .next()
        .unwrap_or("expression")
        .to_lowercase()
}

fn flat_of(e: &syn::Expr, ctx: &Ctx<'_>) -> Lift<String> {
    match expr(e, ctx)? {
        Expr::Flat(s) => Continue(s),
        other => {
            if matches!(other, Expr::If { .. }) {
                let rendered = other.render(0);
                if !rendered.contains('\n') {
                    return Continue(rendered);
                }
            }
            Break(Residue::Form("match inside an operand".to_string()))
        }
    }
}

fn ctor(path: &syn::Path, ctx: &Ctx<'_>) -> Option<String> {
    let n = path.segments.len();
    if n < 2 {
        return None;
    }
    let owner = path.segments[n - 2].ident.to_string();
    let variant = path.segments[n - 1].ident.to_string();
    ctx.enums
        .get(&owner)
        .filter(|vs| vs.contains(&variant))
        .map(|_| format!("{owner}.{variant}"))
}

fn block(b: &syn::Block, ctx: &Ctx<'_>) -> Lift<Expr> {
    match b.stmts.as_slice() {
        [syn::Stmt::Expr(e, None)] => expr(e, ctx),
        _ => Break(Residue::Statements),
    }
}

fn matching(m: &syn::ExprMatch, ctx: &Ctx<'_>) -> Lift<Expr> {
    let scrutinee = flat_of(&m.expr, ctx)?;
    // The scrutinee's enum, read from the arm patterns.
    let mut owner: Option<String> = None;
    let mut arms: Vec<(Vec<String>, &syn::Expr)> = Vec::new();
    let mut wildcard: Option<&syn::Expr> = None;
    for arm in &m.arms {
        if arm.guard.is_some() {
            return Break(Residue::Form("match guard".to_string()));
        }
        let mut names = Vec::new();
        let pats: Vec<&syn::Pat> = match &arm.pat {
            syn::Pat::Or(o) => o.cases.iter().collect(),
            p => vec![p],
        };
        for pat in pats {
            match pat {
                syn::Pat::Path(p) => {
                    let Some(c) = ctor(&p.path, ctx) else {
                        return Break(Residue::Form("match pattern".to_string()));
                    };
                    let (e, v) = c.split_once('.').unwrap_or_default();
                    if owner.get_or_insert_with(|| e.to_string()) != e {
                        return Break(Residue::Form("mixed match".to_string()));
                    }
                    names.push(v.to_string());
                }
                syn::Pat::Wild(_) => wildcard = Some(&arm.body),
                _ => return Break(Residue::Form("match pattern".to_string())),
            }
        }
        if !names.is_empty() {
            arms.push((names, &arm.body));
        }
    }
    let Some(owner) = owner else {
        return Break(Residue::Form("match without enum arms".to_string()));
    };
    let variants = &ctx.enums[&owner];
    let mut out = Vec::new();
    for variant in variants {
        let Some(body) = arms
            .iter()
            .find(|(names, _)| names.contains(variant))
            .map(|(_, b)| *b)
            .or(wildcard)
        else {
            return Break(Residue::Form("non-exhaustive match".to_string()));
        };
        out.push((format!("{owner}.{variant}"), expr(body, ctx)?));
    }
    Continue(Expr::Match {
        scrutinee,
        arms: out,
    })
}

/// A candidate function: its LexLean name, signature and body.
pub(crate) struct Candidate<'a> {
    pub(crate) rust: String,
    pub(crate) name: String,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) result: String,
    pub(crate) block: &'a syn::Block,
}

/// Lift every candidate, to the fixpoint where no lifted body calls a function
/// that failed to lift.
pub(crate) fn lift_all(
    candidates: &[Candidate<'_>],
    enums: &Enums,
) -> (Vec<Decl>, BTreeMap<String, Residue>) {
    let mut lifted: BTreeMap<String, String> = candidates
        .iter()
        .map(|c| (c.rust.clone(), c.name.clone()))
        .collect();
    let mut failed: BTreeMap<String, Residue> = BTreeMap::new();
    loop {
        let mut decls = Vec::new();
        let mut changed = false;
        for c in candidates {
            if failed.contains_key(&c.rust) {
                continue;
            }
            let ctx = Ctx {
                enums,
                lifted: &lifted,
                params: c.params.iter().map(|(n, _)| n.clone()).collect(),
            };
            match block(c.block, &ctx) {
                Continue(body) => decls.push(Decl::Definition {
                    name: c.name.clone(),
                    parameters: c.params.clone(),
                    result: c.result.clone(),
                    body,
                }),
                Break(reason) => {
                    failed.insert(c.rust.clone(), reason);
                    changed = true;
                }
            }
        }
        if !changed {
            return (decls, failed);
        }
        lifted.retain(|rust, _| !failed.contains_key(rust));
    }
}

/// A literal argument of an acceptance assertion.
fn literal(e: &syn::Expr, enums: &Enums) -> Option<String> {
    match e {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Bool(b) => Some(b.value.to_string()),
            syn::Lit::Int(i) => i.base10_parse::<u128>().ok().map(|v| v.to_string()),
            _ => None,
        },
        syn::Expr::Path(p) => {
            let n = p.path.segments.len();
            if n < 2 {
                return None;
            }
            let owner = p.path.segments[n - 2].ident.to_string();
            let variant = p.path.segments[n - 1].ident.to_string();
            enums
                .get(&owner)
                .filter(|v| v.contains(&variant))
                .map(|_| format!("(ctor {owner}.{variant})"))
        }
        syn::Expr::Paren(p) => literal(&p.expr, enums),
        _ => None,
    }
}

/// `f(lit, ..)` against a lifted function returning `Bool` or `Nat`.
fn applied(
    e: &syn::Expr,
    lifted: &BTreeMap<String, (String, String)>,
    enums: &Enums,
) -> Option<(String, String)> {
    let syn::Expr::Call(c) = e else {
        return None;
    };
    let syn::Expr::Path(p) = &*c.func else {
        return None;
    };
    let name = p.path.segments.last()?.ident.to_string();
    let (target, result) = lifted.get(&name)?;
    let mut parts = vec![target.clone()];
    for arg in &c.args {
        parts.push(literal(arg, enums)?);
    }
    Some((format!("({})", parts.join(" ")), result.clone()))
}

/// The acceptance statement an assertion makes about lifted functions, when it
/// is a closed instance: `assert!(f(1, 2))`, `assert!(!f(..))`,
/// `assert_eq!(f(..), 3)`.
pub(crate) fn acceptance(
    kind: &str,
    args: &[syn::Expr],
    lifted: &BTreeMap<String, (String, String)>,
    enums: &Enums,
) -> Option<String> {
    match (kind, args) {
        ("assert", [single, ..]) => {
            if let syn::Expr::Unary(syn::ExprUnary {
                op: syn::UnOp::Not(_),
                expr,
                ..
            }) = single
            {
                let (call, result) = applied(expr, lifted, enums)?;
                (result == "Bool").then(|| format!("({call} = false)"))
            } else {
                let (call, result) = applied(single, lifted, enums)?;
                (result == "Bool").then(|| format!("({call} = true)"))
            }
        }
        ("assert_eq", [left, right, ..]) => {
            let (call, result) = applied(left, lifted, enums)?;
            let value = literal(right, enums)?;
            matches!(result.as_str(), "Bool" | "Nat").then(|| format!("({call} = {value})"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lift(src: &str, enums: &Enums) -> Result<String, Residue> {
        let f: syn::ItemFn = syn::parse_str(src).expect("parses");
        let (params, result) = match signature(&f.sig, enums) {
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
        let (decls, failed) = lift_all(&[c], enums);
        match failed.into_values().next() {
            Some(r) => Err(r),
            None => Ok(crate::lex::render_decl(&decls[0])),
        }
    }

    #[test]
    fn comparisons_and_logic_lift() {
        let out = lift(
            "fn within(c: u32, lo: u32, hi: u32) -> bool { lo <= c && c <= hi }",
            &Enums::new(),
        )
        .expect("lifts");
        assert!(out.contains("((lo <=? c) && (c <=? hi))"), "{out}");
    }

    #[test]
    fn arithmetic_is_residue() {
        assert_eq!(
            lift("fn add(a: u32, b: u32) -> u32 { a + b }", &Enums::new()),
            Err(Residue::Arithmetic("+".to_string()))
        );
    }

    #[test]
    fn a_match_on_a_state_enum_lifts_exhaustively() {
        let mut enums = Enums::new();
        enums.insert("Mode".into(), vec!["Idle".into(), "On".into()]);
        let out = lift(
            "fn flip(m: Mode) -> Mode { match m { Mode::Idle => Mode::On, _ => Mode::Idle } }",
            &enums,
        )
        .expect("lifts");
        assert!(out.contains("| Mode.Idle => (ctor Mode.On)"), "{out}");
        assert!(out.contains("| Mode.On => (ctor Mode.Idle))"), "{out}");
    }
}
