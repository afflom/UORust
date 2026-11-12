//! `syn` trees to the owned syntax of [`crate::ast`], once, at the shell.
//!
//! These are the only functions that read a `syn` type for the lift. Each
//! is a mechanical map from one `syn` form to its owned counterpart; what
//! the lift decides is decided over the owned values.

use quote::ToTokens;
use syn::punctuated::Punctuated;

use crate::ast::{
    Alias, BinOp, Block, ExprAssign, ExprBinary, ExprBlock, ExprBreak, ExprCall, ExprCast,
    ExprClosure, ExprField, ExprForLoop, ExprIf, ExprIndex, ExprLet, ExprMacro, ExprMatch,
    ExprMethodCall, ExprOther, ExprReference, ExprStruct, ExprUnary, FieldPat, FieldValue, FnArg,
    GenericArgument, GenericParam, Lit, Local, LocalInit, MatchArm, Member, Pat, PatGuard,
    PatIdent, PatRange, PatStruct, PatTupleStruct, PatType, Path, PathArguments, PathSegment,
    RustExpr, RustType, Signature, Stmt, TypeKind, UnOp,
};

fn text(tokens: &dyn ToTokens) -> String {
    crate::ingest::tidy(&tokens.to_token_stream().to_string())
}

/// A path.
pub(crate) fn path(p: &syn::Path) -> Path {
    Path {
        segments: p
            .segments
            .iter()
            .map(|s| PathSegment {
                ident: s.ident.to_string(),
                arguments: match &s.arguments {
                    syn::PathArguments::None => PathArguments::None,
                    syn::PathArguments::AngleBracketed(a) => {
                        PathArguments::AngleBracketed(generic_arguments(&a.args))
                    }
                    syn::PathArguments::Parenthesized(_) => PathArguments::Parenthesized,
                },
            })
            .collect(),
        text: text(p),
    }
}

fn generic_arguments(
    args: &Punctuated<syn::GenericArgument, syn::Token![,]>,
) -> Vec<GenericArgument> {
    args.iter()
        .map(|a| match a {
            syn::GenericArgument::Type(t) => GenericArgument::Type(ty(t)),
            other => GenericArgument::Other(text(other)),
        })
        .collect()
}

/// A type.
pub(crate) fn ty(t: &syn::Type) -> RustType {
    let kind = match t {
        syn::Type::Reference(r) => TypeKind::Reference {
            mutable: r.mutability.is_some(),
            elem: Box::new(ty(&r.elem)),
        },
        syn::Type::Slice(s) => TypeKind::Slice(Box::new(ty(&s.elem))),
        syn::Type::Paren(p) => TypeKind::Paren(Box::new(ty(&p.elem))),
        syn::Type::Group(g) => TypeKind::Paren(Box::new(ty(&g.elem))),
        syn::Type::Tuple(tp) => TypeKind::Tuple(tp.elems.iter().map(ty).collect()),
        syn::Type::Path(p) => TypeKind::Path {
            qself: p.qself.is_some(),
            path: path(&p.path),
        },
        _ => TypeKind::Other,
    };
    RustType {
        text: text(t),
        kind,
    }
}

/// A type alias: its type parameters and the type it stands for.
pub(crate) fn alias(generics: &syn::Generics, to: &syn::Type) -> Alias {
    Alias {
        params: generics
            .params
            .iter()
            .filter_map(|p| match p {
                syn::GenericParam::Type(t) => Some(t.ident.to_string()),
                _ => None,
            })
            .collect(),
        to: ty(to),
    }
}

/// A literal.
pub(crate) fn lit(l: &syn::Lit) -> Lit {
    match l {
        syn::Lit::Bool(b) => Lit::Bool(b.value),
        syn::Lit::Int(i) => Lit::Int {
            value: i.base10_parse::<u128>().ok(),
            suffix: i.suffix().to_string(),
        },
        syn::Lit::Str(s) => Lit::Str(s.value()),
        syn::Lit::Char(c) => Lit::Char(c.value()),
        syn::Lit::Byte(b) => Lit::Byte(b.value()),
        syn::Lit::ByteStr(b) => Lit::ByteStr(b.value()),
        _ => Lit::Other,
    }
}

fn member(m: &syn::Member) -> Member {
    match m {
        syn::Member::Named(id) => Member::Named(id.to_string()),
        syn::Member::Unnamed(i) => Member::Unnamed(i.index),
    }
}

/// A block.
pub(crate) fn block(b: &syn::Block) -> Block {
    // A `use` in a body names paths for the statements after it: they are
    // read as written out in full.
    let mut stmts = Vec::new();
    let mut rest: Vec<syn::Stmt> = b.stmts.clone();
    while !rest.is_empty() {
        let first = rest.remove(0);
        match &first {
            syn::Stmt::Item(syn::Item::Use(u)) if u.leading_colon.is_none() => {
                let mut names = Vec::new();
                if !used_names(&u.tree, &mut Vec::new(), &mut names) {
                    stmts.push(Stmt::Other);
                    continue;
                }
                let mut written = WrittenOut { names };
                for s in &mut rest {
                    syn::visit_mut::VisitMut::visit_stmt_mut(&mut written, s);
                }
            }
            other => stmts.push(stmt(other)),
        }
    }
    Block { stmts }
}

/// The names a `use` tree binds, each with the path it names, or `false`
/// for a glob, whose names the tree does not say.
fn used_names(
    tree: &syn::UseTree,
    prefix: &mut Vec<syn::Ident>,
    out: &mut Vec<(syn::Ident, Vec<syn::Ident>)>,
) -> bool {
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.clone());
            let ok = used_names(&p.tree, prefix, out);
            prefix.pop();
            ok
        }
        syn::UseTree::Name(n) => {
            let mut full = prefix.clone();
            full.push(n.ident.clone());
            out.push((n.ident.clone(), full));
            true
        }
        syn::UseTree::Rename(r) => {
            let mut full = prefix.clone();
            full.push(r.ident.clone());
            out.push((r.rename.clone(), full));
            true
        }
        syn::UseTree::Group(g) => g.items.iter().all(|t| used_names(t, prefix, out)),
        syn::UseTree::Glob(_) => false,
    }
}

/// Rewrites each path that starts with a name a body's `use` binds into
/// the path that name stands for.
struct WrittenOut {
    names: Vec<(syn::Ident, Vec<syn::Ident>)>,
}

impl syn::visit_mut::VisitMut for WrittenOut {
    fn visit_path_mut(&mut self, p: &mut syn::Path) {
        if p.leading_colon.is_none() {
            let head = p.segments.first().map(|s| s.ident.clone());
            if let Some((_, full)) = self.names.iter().find(|(n, _)| Some(n) == head.as_ref()) {
                let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = full
                    .iter()
                    .map(|i| syn::PathSegment::from(i.clone()))
                    .collect();
                let first = p.segments.iter().next().cloned();
                if let (Some(last), Some(first)) = (segments.last_mut(), first) {
                    last.arguments = first.arguments;
                }
                segments.extend(p.segments.iter().skip(1).cloned());
                p.segments = segments;
            }
        }
        syn::visit_mut::visit_path_mut(self, p);
    }
}

fn stmt(s: &syn::Stmt) -> Stmt {
    match s {
        syn::Stmt::Local(l) => Stmt::Local(Local {
            pat: pat(&l.pat),
            init: l.init.as_ref().map(|i| LocalInit {
                expr: Box::new(expr(&i.expr)),
                diverge: i.diverge.as_ref().map(|(_, e)| Box::new(expr(e))),
            }),
        }),
        syn::Stmt::Expr(e, semi) => Stmt::Expr(expr(e), semi.is_some()),
        syn::Stmt::Macro(m) => Stmt::Macro(mac(&m.mac)),
        // `const N: T = e;` in a body is the value `e` of type `T`.
        syn::Stmt::Item(syn::Item::Const(c)) => Stmt::Local(Local {
            pat: Pat::Type(PatType {
                pat: Box::new(Pat::Ident(PatIdent {
                    ident: c.ident.to_string(),
                    by_ref: false,
                    subpat: None,
                })),
                ty: ty(&c.ty),
            }),
            init: Some(LocalInit {
                expr: Box::new(expr(&c.expr)),
                diverge: None,
            }),
        }),
        syn::Stmt::Item(_) => Stmt::Other,
    }
}

/// The arguments of a macro, where they read as expressions separated by
/// commas, and those of `matches!`.
fn mac(m: &syn::Macro) -> ExprMacro {
    let args = m
        .parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        .ok()
        .map(|list| list.iter().map(expr).collect());
    let matches = if m.path.is_ident("matches") {
        m.parse_body_with(|input: syn::parse::ParseStream<'_>| {
            let e: syn::Expr = input.parse()?;
            input.parse::<syn::Token![,]>()?;
            let p = syn::Pat::parse_multi_with_leading_vert(input)?;
            // `matches!(e, P if guard)`.
            let guard = if input.peek(syn::Token![if]) {
                input.parse::<syn::Token![if]>()?;
                Some(input.parse::<syn::Expr>()?)
            } else {
                None
            };
            let _ = input.parse::<Option<syn::Token![,]>>()?;
            Ok((e, p, guard))
        })
        .ok()
        .map(|(e, p, guard)| {
            let p = match guard {
                Some(g) => Pat::Guard(PatGuard {
                    pat: Box::new(pat(&p)),
                    guard: Box::new(expr(&g)),
                }),
                None => pat(&p),
            };
            (Box::new(expr(&e)), Box::new(p))
        })
    } else {
        None
    };
    ExprMacro {
        path: path(&m.path),
        args,
        matches,
    }
}

fn other(kind: &str, exprs: Vec<&syn::Expr>, blocks: Vec<&syn::Block>) -> RustExpr {
    RustExpr::Other(ExprOther {
        kind: format!("expr::{kind}"),
        exprs: exprs.into_iter().map(expr).collect(),
        blocks: blocks.into_iter().map(block).collect(),
    })
}

/// An expression.
#[allow(clippy::too_many_lines)]
pub(crate) fn expr(e: &syn::Expr) -> RustExpr {
    use syn::Expr as E;
    match e {
        E::Array(a) => RustExpr::Array(a.elems.iter().map(expr).collect()),
        E::Assign(a) => RustExpr::Assign(ExprAssign {
            left: Box::new(expr(&a.left)),
            right: Box::new(expr(&a.right)),
        }),
        E::Binary(b) => {
            use syn::BinOp as B;
            let op = match b.op {
                B::Add(_) => BinOp::Add,
                B::Sub(_) => BinOp::Sub,
                B::Mul(_) => BinOp::Mul,
                B::Div(_) => BinOp::Div,
                B::Rem(_) => BinOp::Rem,
                B::And(_) => BinOp::And,
                B::Or(_) => BinOp::Or,
                B::BitXor(_) => BinOp::BitXor,
                B::BitAnd(_) => BinOp::BitAnd,
                B::BitOr(_) => BinOp::BitOr,
                B::Shl(_) => BinOp::Shl,
                B::Shr(_) => BinOp::Shr,
                B::Eq(_) => BinOp::Eq,
                B::Lt(_) => BinOp::Lt,
                B::Le(_) => BinOp::Le,
                B::Ne(_) => BinOp::Ne,
                B::Ge(_) => BinOp::Ge,
                B::Gt(_) => BinOp::Gt,
                B::AddAssign(_) => BinOp::AddAssign,
                B::SubAssign(_) => BinOp::SubAssign,
                B::MulAssign(_) => BinOp::MulAssign,
                B::DivAssign(_) => BinOp::DivAssign,
                B::RemAssign(_) => BinOp::RemAssign,
                B::BitXorAssign(_) => BinOp::BitXorAssign,
                B::BitAndAssign(_) => BinOp::BitAndAssign,
                B::BitOrAssign(_) => BinOp::BitOrAssign,
                B::ShlAssign(_) => BinOp::ShlAssign,
                _ => BinOp::ShrAssign,
            };
            RustExpr::Binary(ExprBinary {
                left: Box::new(expr(&b.left)),
                op,
                right: Box::new(expr(&b.right)),
            })
        }
        E::Block(b) => RustExpr::Block(ExprBlock {
            label: b.label.is_some(),
            block: block(&b.block),
        }),
        E::Break(b) => RustExpr::Break(ExprBreak {
            label: b.label.is_some(),
            expr: b.expr.as_deref().map(|e| Box::new(expr(e))),
        }),
        E::Call(c) => RustExpr::Call(ExprCall {
            func: Box::new(expr(&c.func)),
            args: c.args.iter().map(expr).collect(),
        }),
        E::Cast(c) => RustExpr::Cast(ExprCast {
            expr: Box::new(expr(&c.expr)),
            ty: ty(&c.ty),
        }),
        E::Closure(c) => RustExpr::Closure(ExprClosure {
            inputs: c.inputs.iter().map(pat).collect(),
            body: Box::new(expr(&c.body)),
            asyncness: c.asyncness.is_some(),
        }),
        E::Continue(c) => RustExpr::Continue(c.label.is_some()),
        E::Field(f) => RustExpr::Field(ExprField {
            base: Box::new(expr(&f.base)),
            member: member(&f.member),
        }),
        E::ForLoop(f) => RustExpr::ForLoop(ExprForLoop {
            label: f.label.is_some(),
            pat: pat(&f.pat),
            expr: Box::new(expr(&f.expr)),
            body: block(&f.body),
        }),
        E::If(i) => RustExpr::If(ExprIf {
            cond: Box::new(expr(&i.cond)),
            then_branch: block(&i.then_branch),
            else_branch: i.else_branch.as_ref().map(|(_, e)| Box::new(expr(e))),
        }),
        E::Index(i) => RustExpr::Index(ExprIndex {
            expr: Box::new(expr(&i.expr)),
            index: Box::new(expr(&i.index)),
        }),
        E::Let(l) => RustExpr::Let(ExprLet {
            pat: pat(&l.pat),
            expr: Box::new(expr(&l.expr)),
        }),
        E::Lit(l) => RustExpr::Lit(lit(&l.lit)),
        E::Macro(m) => RustExpr::Macro(mac(&m.mac)),
        E::Match(m) => RustExpr::Match(ExprMatch {
            expr: Box::new(expr(&m.expr)),
            arms: m
                .arms
                .iter()
                .map(|a| MatchArm {
                    pat: pat(&a.pat),
                    body: Box::new(expr(&a.body)),
                })
                .collect(),
        }),
        E::MethodCall(m) => RustExpr::MethodCall(ExprMethodCall {
            receiver: Box::new(expr(&m.receiver)),
            method: m.method.to_string(),
            turbofish: m.turbofish.as_ref().map(|t| generic_arguments(&t.args)),
            args: m.args.iter().map(expr).collect(),
        }),
        E::Paren(p) => RustExpr::Paren(Box::new(expr(&p.expr))),
        E::Group(g) => RustExpr::Paren(Box::new(expr(&g.expr))),
        E::Path(p) => RustExpr::Path(path(&p.path)),
        E::Reference(r) => RustExpr::Reference(ExprReference {
            mutable: r.mutability.is_some(),
            expr: Box::new(expr(&r.expr)),
        }),
        E::Return(r) => RustExpr::Return(r.expr.as_deref().map(|e| Box::new(expr(e)))),
        E::Struct(s) => RustExpr::Struct(ExprStruct {
            qself: s.qself.is_some(),
            path: path(&s.path),
            fields: s
                .fields
                .iter()
                .map(|f| FieldValue {
                    member: member(&f.member),
                    expr: expr(&f.expr),
                })
                .collect(),
            rest: s.rest.as_ref().map(|e| Box::new(expr(e))),
        }),
        E::Try(t) => RustExpr::Try(Box::new(expr(&t.expr))),
        E::Tuple(t) => RustExpr::Tuple(t.elems.iter().map(expr).collect()),
        E::Unary(u) => RustExpr::Unary(ExprUnary {
            op: match u.op {
                syn::UnOp::Deref(_) => UnOp::Deref,
                syn::UnOp::Not(_) => UnOp::Not,
                _ => UnOp::Neg,
            },
            expr: Box::new(expr(&u.expr)),
        }),
        E::Async(a) => other("async", Vec::new(), vec![&a.block]),
        E::Await(a) => other("await", vec![&a.base], Vec::new()),
        E::Const(c) => other("const", Vec::new(), vec![&c.block]),
        E::Loop(l) => other("loop", Vec::new(), vec![&l.body]),
        E::Range(r) => other(
            "range",
            r.start.iter().chain(&r.end).map(|e| &**e).collect(),
            Vec::new(),
        ),
        E::RawAddr(r) => other("rawaddr", vec![&r.expr], Vec::new()),
        E::Repeat(r) => other("repeat", vec![&r.expr, &r.len], Vec::new()),
        E::TryBlock(t) => other("tryblock", Vec::new(), vec![&t.block]),
        E::Unsafe(u) => other("unsafe", Vec::new(), vec![&u.block]),
        E::While(w) => other("while", vec![&w.cond], vec![&w.body]),
        E::Yield(y) => other("yield", y.expr.iter().map(|e| &**e).collect(), Vec::new()),
        _ => {
            let debug = format!("{e:?}");
            let kind = debug
                .split(['(', ' ', '{'])
                .next()
                .unwrap_or("expression")
                .to_lowercase();
            RustExpr::Other(ExprOther {
                kind,
                exprs: Vec::new(),
                blocks: Vec::new(),
            })
        }
    }
}

/// A pattern.
pub(crate) fn pat(p: &syn::Pat) -> Pat {
    match p {
        syn::Pat::Ident(i) => Pat::Ident(PatIdent {
            ident: i.ident.to_string(),
            by_ref: i.by_ref.is_some(),
            subpat: i.subpat.as_ref().map(|(_, s)| Box::new(pat(s))),
        }),
        syn::Pat::Wild(_) => Pat::Wild,
        syn::Pat::Reference(r) => Pat::Reference(Box::new(pat(&r.pat))),
        syn::Pat::Type(t) => Pat::Type(PatType {
            pat: Box::new(pat(&t.pat)),
            ty: ty(&t.ty),
        }),
        syn::Pat::Tuple(t) => Pat::Tuple(t.elems.iter().map(pat).collect()),
        syn::Pat::TupleStruct(t) => Pat::TupleStruct(PatTupleStruct {
            path: path(&t.path),
            elems: t.elems.iter().map(pat).collect(),
        }),
        syn::Pat::Struct(s) => Pat::Struct(PatStruct {
            path: path(&s.path),
            fields: s
                .fields
                .iter()
                .map(|f| FieldPat {
                    member: member(&f.member),
                    pat: pat(&f.pat),
                })
                .collect(),
            rest: s.rest.is_some(),
        }),
        syn::Pat::Slice(s) => Pat::Slice(s.elems.iter().map(pat).collect()),
        syn::Pat::Rest(_) => Pat::Rest,
        syn::Pat::Lit(l) => Pat::Lit(lit(&l.lit)),
        syn::Pat::Range(r) => Pat::Range(PatRange {
            start: r.start.as_deref().map(|e| Box::new(expr(e))),
            end: r.end.as_deref().map(|e| Box::new(expr(e))),
            closed: matches!(r.limits, syn::RangeLimits::Closed(_)),
        }),
        syn::Pat::Path(p) => Pat::Path(path(&p.path)),
        syn::Pat::Or(o) => Pat::Or(o.cases.iter().map(pat).collect()),
        syn::Pat::Guard(g) => Pat::Guard(PatGuard {
            pat: Box::new(pat(&g.pat)),
            guard: Box::new(expr(&g.guard)),
        }),
        _ => Pat::Other,
    }
}

/// A signature.
pub(crate) fn signature(sig: &syn::Signature) -> Signature {
    Signature {
        ident: sig.ident.to_string(),
        generics: sig
            .generics
            .params
            .iter()
            .map(|p| match p {
                syn::GenericParam::Lifetime(l) => {
                    GenericParam::Lifetime(l.lifetime.ident.to_string())
                }
                syn::GenericParam::Type(t) => GenericParam::Type(t.ident.to_string()),
                syn::GenericParam::Const(c) => GenericParam::Const(c.ident.to_string()),
            })
            .collect(),
        asyncness: sig.asyncness.is_some(),
        unsafety: matches!(sig.safety, syn::Safety::Unsafe(_)),
        variadic: sig.variadic.is_some(),
        inputs: sig
            .inputs
            .iter()
            .map(|input| match input {
                syn::FnArg::Receiver(r) => FnArg::Receiver {
                    plain: r.mutability.is_none()
                        && matches!(
                            r.kind,
                            syn::ReceiverKind::Value | syn::ReceiverKind::Reference(_, _, None)
                        ),
                },
                syn::FnArg::Typed(t) => FnArg::Typed(PatType {
                    pat: Box::new(pat(&t.pat)),
                    ty: ty(&t.ty),
                }),
            })
            .collect(),
        output: match &sig.output {
            syn::ReturnType::Default => None,
            syn::ReturnType::Type(_, t) => Some(ty(t)),
        },
    }
}
