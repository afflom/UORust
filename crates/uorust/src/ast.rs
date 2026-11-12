//! The Rust syntax the lift reads, as owned values.
//!
//! [`crate::convert`] reads each `syn` tree into these once, at the shell,
//! so the lift is a function of plain data: enums and structs of strings,
//! numbers, booleans, options and lists, which the lift itself can name.
//! Each form keeps what the lift decides on and nothing else; a form the
//! lift does not read is [`RustExpr::Other`], [`Pat::Other`] or
//! [`TypeKind::Other`], still holding its subexpressions where a scan of
//! the body looks through it.

use std::collections::BTreeMap;

/// A path `a::b::C<T>`, and its text as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Path {
    pub(crate) segments: Vec<PathSegment>,
    /// The tokens of the path, tidied: what a residue names.
    pub(crate) text: String,
}

/// One segment of a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PathSegment {
    pub(crate) ident: String,
    pub(crate) arguments: PathArguments,
}

/// The generic arguments of a path segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathArguments {
    None,
    /// `<A, B>`.
    AngleBracketed(Vec<GenericArgument>),
    /// `(A) -> B`, as `Fn` traits write it.
    Parenthesized,
}

/// One generic argument: a type, or something else (a lifetime, a const),
/// as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GenericArgument {
    Type(RustType),
    Other(String),
}

impl Path {
    /// A path of one plain segment.
    pub(crate) fn ident(name: &str) -> Path {
        Path {
            segments: vec![PathSegment {
                ident: name.to_string(),
                arguments: PathArguments::None,
            }],
            text: name.to_string(),
        }
    }

    /// The identifier a path of one plain segment is.
    pub(crate) fn get_ident(&self) -> Option<&String> {
        if self.segments.len() != 1 {
            return None;
        }
        match self.segments.first() {
            Some(one) => {
                if matches!(one.arguments, PathArguments::None) {
                    Some(&one.ident)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Whether the path is this one identifier.
    pub(crate) fn is_ident(&self, name: &str) -> bool {
        self.get_ident().is_some_and(|i| i == name)
    }
}

/// A type, and its text as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RustType {
    /// The tokens of the type, tidied: what a residue names.
    pub(crate) text: String,
    pub(crate) kind: TypeKind,
}

impl RustType {
    /// The type with every alias of `aliases` it names replaced by what the
    /// alias stands for. An alias may name another; each expansion counts
    /// against `depth`, the aliases left to expand along one path, so a
    /// cycle of aliases ends.
    pub(crate) fn expanded(&self, aliases: &BTreeMap<String, RustType>, depth: usize) -> RustType {
        if let TypeKind::Path { qself: false, path } = &self.kind {
            if let [one] = path.segments.as_slice() {
                if one.arguments == PathArguments::None && depth > 0 {
                    if let Some(to) = aliases.get(&one.ident) {
                        return to.expanded(aliases, depth - 1);
                    }
                }
            }
        }
        let inner = |t: &RustType| Box::new(t.expanded(aliases, depth));
        let kind = match &self.kind {
            TypeKind::Reference { mutable, elem } => TypeKind::Reference {
                mutable: *mutable,
                elem: inner(elem),
            },
            TypeKind::Slice(t) => TypeKind::Slice(inner(t)),
            TypeKind::Paren(t) => TypeKind::Paren(inner(t)),
            TypeKind::Tuple(ts) => {
                TypeKind::Tuple(ts.iter().map(|t| t.expanded(aliases, depth)).collect())
            }
            TypeKind::Path { qself, path } => TypeKind::Path {
                qself: *qself,
                path: Path {
                    segments: path
                        .segments
                        .iter()
                        .map(|s| PathSegment {
                            ident: s.ident.clone(),
                            arguments: match &s.arguments {
                                PathArguments::AngleBracketed(args) => {
                                    PathArguments::AngleBracketed(
                                        args.iter()
                                            .map(|a| match a {
                                                GenericArgument::Type(t) => GenericArgument::Type(
                                                    t.expanded(aliases, depth),
                                                ),
                                                other => other.clone(),
                                            })
                                            .collect(),
                                    )
                                }
                                other => other.clone(),
                            },
                        })
                        .collect(),
                    text: path.text.clone(),
                },
            },
            TypeKind::Other => TypeKind::Other,
        };
        if kind == self.kind {
            return self.clone();
        }
        let text = written(&kind);
        RustType { text, kind }
    }
}

/// The text of a type rebuilt from its parts.
fn written(kind: &TypeKind) -> String {
    match kind {
        TypeKind::Reference {
            mutable: true,
            elem,
        } => format!("&mut {}", elem.text),
        TypeKind::Reference {
            mutable: false,
            elem,
        } => format!("&{}", elem.text),
        TypeKind::Slice(t) => format!("[{}]", t.text),
        TypeKind::Paren(t) => format!("({})", t.text),
        TypeKind::Tuple(ts) => {
            let items: Vec<&str> = ts.iter().map(|t| t.text.as_str()).collect();
            format!("({})", items.join(", "))
        }
        TypeKind::Path { path, .. } => {
            let segments: Vec<String> = path
                .segments
                .iter()
                .map(|s| match &s.arguments {
                    PathArguments::AngleBracketed(args) => {
                        let items: Vec<&str> = args
                            .iter()
                            .map(|a| match a {
                                GenericArgument::Type(t) => t.text.as_str(),
                                GenericArgument::Other(text) => text.as_str(),
                            })
                            .collect();
                        format!("{}<{}>", s.ident, items.join(", "))
                    }
                    _ => s.ident.clone(),
                })
                .collect();
            segments.join("::")
        }
        TypeKind::Other => String::new(),
    }
}

/// The forms of a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TypeKind {
    /// `&T` or `&mut T`.
    Reference {
        mutable: bool,
        elem: Box<RustType>,
    },
    /// `[T]`.
    Slice(Box<RustType>),
    /// `(T)`, or an invisible group.
    Paren(Box<RustType>),
    /// `()` and `(A, B, ..)`.
    Tuple(Vec<RustType>),
    /// A path, with `qself` when it is `<T as Trait>::Name`.
    Path {
        qself: bool,
        path: Path,
    },
    Other,
}

/// A literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lit {
    Bool(bool),
    /// An integer: its value where it fits a `u128`, and its suffix.
    Int {
        value: Option<u128>,
        suffix: String,
    },
    Str(String),
    Char(char),
    Byte(u8),
    ByteStr(Vec<u8>),
    Other,
}

/// A binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    RemAssign,
    BitXorAssign,
    BitAndAssign,
    BitOrAssign,
    ShlAssign,
    ShrAssign,
}

impl BinOp {
    /// The operator as Rust writes it.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::And => "&&",
            Self::Or => "||",
            Self::BitXor => "^",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Eq => "==",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Ne => "!=",
            Self::Ge => ">=",
            Self::Gt => ">",
            Self::AddAssign => "+=",
            Self::SubAssign => "-=",
            Self::MulAssign => "*=",
            Self::DivAssign => "/=",
            Self::RemAssign => "%=",
            Self::BitXorAssign => "^=",
            Self::BitAndAssign => "&=",
            Self::BitOrAssign => "|=",
            Self::ShlAssign => "<<=",
            Self::ShrAssign => ">>=",
        }
    }
}

/// A unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnOp {
    Deref,
    Not,
    Neg,
}

/// A field of a struct or a tuple: by name, or by position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    Named(String),
    Unnamed(u32),
}

/// A block of statements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) stmts: Vec<Stmt>,
}

/// A statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stmt {
    Local(Local),
    /// An expression, and whether a semicolon ends it.
    Expr(RustExpr, bool),
    /// An item or a macro in statement position.
    Other,
}

/// `let pat = init;`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Local {
    pub(crate) pat: Pat,
    pub(crate) init: Option<LocalInit>,
}

/// The value of a `let`, and its `else` branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalInit {
    pub(crate) expr: Box<RustExpr>,
    pub(crate) diverge: Option<Box<RustExpr>>,
}

/// An expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RustExpr {
    /// `[a, b, c]`.
    Array(Vec<RustExpr>),
    Assign(ExprAssign),
    Binary(ExprBinary),
    Block(ExprBlock),
    Break(ExprBreak),
    Call(ExprCall),
    Cast(ExprCast),
    Closure(ExprClosure),
    /// `continue`, and whether it has a label.
    Continue(bool),
    Field(ExprField),
    ForLoop(ExprForLoop),
    If(ExprIf),
    Index(ExprIndex),
    Let(ExprLet),
    Lit(Lit),
    Macro(ExprMacro),
    Match(ExprMatch),
    MethodCall(ExprMethodCall),
    /// `(e)`, or an invisible group.
    Paren(Box<RustExpr>),
    Path(Path),
    Reference(ExprReference),
    /// `return`, with its value.
    Return(Option<Box<RustExpr>>),
    Struct(ExprStruct),
    /// `e?`.
    Try(Box<RustExpr>),
    /// `()` and `(a, b, ..)`.
    Tuple(Vec<RustExpr>),
    Unary(ExprUnary),
    /// Any other form: its kind (`expr::while`) and the expressions and
    /// blocks inside it.
    Other(ExprOther),
}

/// `left = right`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprAssign {
    pub(crate) left: Box<RustExpr>,
    pub(crate) right: Box<RustExpr>,
}

/// `left op right`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprBinary {
    pub(crate) left: Box<RustExpr>,
    pub(crate) op: BinOp,
    pub(crate) right: Box<RustExpr>,
}

/// `{ .. }`, and whether it has a label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprBlock {
    pub(crate) label: bool,
    pub(crate) block: Block,
}

/// `break`, whether it has a label, and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprBreak {
    pub(crate) label: bool,
    pub(crate) expr: Option<Box<RustExpr>>,
}

/// `func(args)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprCall {
    pub(crate) func: Box<RustExpr>,
    pub(crate) args: Vec<RustExpr>,
}

/// `expr as ty`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprCast {
    pub(crate) expr: Box<RustExpr>,
    pub(crate) ty: RustType,
}

/// `|inputs| body`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprClosure {
    pub(crate) inputs: Vec<Pat>,
    pub(crate) body: Box<RustExpr>,
    pub(crate) asyncness: bool,
}

/// `base.member`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprField {
    pub(crate) base: Box<RustExpr>,
    pub(crate) member: Member,
}

/// `for pat in expr { body }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprForLoop {
    pub(crate) label: bool,
    pub(crate) pat: Pat,
    pub(crate) expr: Box<RustExpr>,
    pub(crate) body: Block,
}

/// `if cond { then } else otherwise`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprIf {
    pub(crate) cond: Box<RustExpr>,
    pub(crate) then_branch: Block,
    pub(crate) else_branch: Option<Box<RustExpr>>,
}

/// `expr[index]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprIndex {
    pub(crate) expr: Box<RustExpr>,
    pub(crate) index: Box<RustExpr>,
}

/// `let pat = expr`, as an `if let` condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprLet {
    pub(crate) pat: Pat,
    pub(crate) expr: Box<RustExpr>,
}

/// `path!(..)`: its arguments where they read as a comma-separated list of
/// expressions, and, for `matches!`, the scrutinee and pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprMacro {
    pub(crate) path: Path,
    pub(crate) args: Option<Vec<RustExpr>>,
    pub(crate) matches: Option<(Box<RustExpr>, Box<Pat>)>,
}

/// `match expr { arms }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprMatch {
    pub(crate) expr: Box<RustExpr>,
    pub(crate) arms: Vec<MatchArm>,
}

/// One arm of a match; a guard is [`Pat::Guard`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MatchArm {
    pub(crate) pat: Pat,
    pub(crate) body: Box<RustExpr>,
}

/// `receiver.method::<turbofish>(args)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprMethodCall {
    pub(crate) receiver: Box<RustExpr>,
    pub(crate) method: String,
    pub(crate) turbofish: Option<Vec<GenericArgument>>,
    pub(crate) args: Vec<RustExpr>,
}

/// `&expr` or `&mut expr`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprReference {
    pub(crate) mutable: bool,
    pub(crate) expr: Box<RustExpr>,
}

/// `path { fields, ..rest }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprStruct {
    pub(crate) qself: bool,
    pub(crate) path: Path,
    pub(crate) fields: Vec<FieldValue>,
    pub(crate) rest: bool,
}

/// `member: expr` in a struct expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldValue {
    pub(crate) member: Member,
    pub(crate) expr: RustExpr,
}

/// `op expr`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprUnary {
    pub(crate) op: UnOp,
    pub(crate) expr: Box<RustExpr>,
}

/// A form the lift does not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprOther {
    /// `expr::` and the form's name, lowercase.
    pub(crate) kind: String,
    pub(crate) exprs: Vec<RustExpr>,
    pub(crate) blocks: Vec<Block>,
}

impl RustExpr {
    /// The expressions and blocks directly inside, where a scan of a body
    /// looks: not into a macro's arguments, which are tokens to Rust.
    pub(crate) fn parts(&self) -> (Vec<&RustExpr>, Vec<&Block>) {
        let none: Vec<&Block> = Vec::new();
        match self {
            Self::Array(items) | Self::Tuple(items) => (items.iter().collect(), none),
            Self::Assign(a) => (vec![&a.left, &a.right], none),
            Self::Binary(b) => (vec![&b.left, &b.right], none),
            Self::Block(b) => (Vec::new(), vec![&b.block]),
            Self::Break(b) => (b.expr.iter().map(|e| &**e).collect(), none),
            Self::Call(c) => {
                let mut out = vec![&*c.func];
                out.extend(&c.args);
                (out, none)
            }
            Self::Cast(c) => (vec![&c.expr], none),
            Self::Closure(c) => (vec![&c.body], none),
            Self::Field(f) => (vec![&f.base], none),
            Self::ForLoop(f) => (vec![&f.expr], vec![&f.body]),
            Self::If(i) => {
                let mut out = vec![&*i.cond];
                out.extend(i.else_branch.as_deref());
                (out, vec![&i.then_branch])
            }
            Self::Index(i) => (vec![&i.expr, &i.index], none),
            Self::Let(l) => (vec![&l.expr], none),
            Self::Match(m) => {
                let mut out = vec![&*m.expr];
                for arm in &m.arms {
                    if let Pat::Guard(g) = &arm.pat {
                        out.push(&g.guard);
                    }
                    out.push(&arm.body);
                }
                (out, none)
            }
            Self::MethodCall(m) => {
                let mut out = vec![&*m.receiver];
                out.extend(&m.args);
                (out, none)
            }
            Self::Paren(e) | Self::Try(e) => (vec![&**e], none),
            Self::Reference(r) => (vec![&r.expr], none),
            Self::Return(r) => (r.iter().map(|e| &**e).collect(), none),
            Self::Struct(s) => (s.fields.iter().map(|f| &f.expr).collect(), none),
            Self::Unary(u) => (vec![&u.expr], none),
            Self::Other(o) => (o.exprs.iter().collect(), o.blocks.iter().collect()),
            Self::Continue(_) | Self::Lit(_) | Self::Macro(_) | Self::Path(_) => (Vec::new(), none),
        }
    }

    /// The form's kind, as a residue names it: `expr::closure`.
    pub(crate) fn kind(&self) -> String {
        let name = match self {
            Self::Array(_) => "array",
            Self::Assign(_) => "assign",
            Self::Binary(_) => "binary",
            Self::Block(_) => "block",
            Self::Break(_) => "break",
            Self::Call(_) => "call",
            Self::Cast(_) => "cast",
            Self::Closure(_) => "closure",
            Self::Continue(_) => "continue",
            Self::Field(_) => "field",
            Self::ForLoop(_) => "forloop",
            Self::If(_) => "if",
            Self::Index(_) => "index",
            Self::Let(_) => "let",
            Self::Lit(_) => "lit",
            Self::Macro(_) => "macro",
            Self::Match(_) => "match",
            Self::MethodCall(_) => "methodcall",
            Self::Paren(_) => "paren",
            Self::Path(_) => "path",
            Self::Reference(_) => "reference",
            Self::Return(_) => "return",
            Self::Struct(_) => "struct",
            Self::Try(_) => "try",
            Self::Tuple(_) => "tuple",
            Self::Unary(_) => "unary",
            Self::Other(o) => return o.kind.clone(),
        };
        format!("expr::{name}")
    }
}

/// A pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pat {
    Ident(PatIdent),
    /// `_`.
    Wild,
    /// `&pat`.
    Reference(Box<Pat>),
    Type(PatType),
    /// `(a, b, ..)`.
    Tuple(Vec<Pat>),
    TupleStruct(PatTupleStruct),
    Struct(PatStruct),
    /// `[a, b, ..]`.
    Slice(Vec<Pat>),
    /// `..`.
    Rest,
    Lit(Lit),
    Range(PatRange),
    Path(Path),
    /// `a | b`.
    Or(Vec<Pat>),
    Guard(PatGuard),
    Other,
}

/// `ref mut name @ subpat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatIdent {
    pub(crate) ident: String,
    pub(crate) by_ref: bool,
    pub(crate) subpat: Option<Box<Pat>>,
}

/// `pat: ty`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatType {
    pub(crate) pat: Box<Pat>,
    pub(crate) ty: RustType,
}

/// `Path(elems)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatTupleStruct {
    pub(crate) path: Path,
    pub(crate) elems: Vec<Pat>,
}

/// `Path { fields, .. }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatStruct {
    pub(crate) path: Path,
    pub(crate) fields: Vec<FieldPat>,
    pub(crate) rest: bool,
}

/// `member: pat` in a struct pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldPat {
    pub(crate) member: Member,
    pub(crate) pat: Pat,
}

/// `start..end` or `start..=end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatRange {
    pub(crate) start: Option<Box<RustExpr>>,
    pub(crate) end: Option<Box<RustExpr>>,
    pub(crate) closed: bool,
}

/// `pat if guard`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatGuard {
    pub(crate) pat: Box<Pat>,
    pub(crate) guard: Box<RustExpr>,
}

/// A function's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Signature {
    pub(crate) ident: String,
    pub(crate) generics: Vec<GenericParam>,
    pub(crate) asyncness: bool,
    pub(crate) unsafety: bool,
    pub(crate) variadic: bool,
    pub(crate) inputs: Vec<FnArg>,
    /// The result type; `None` for `()` left unwritten.
    pub(crate) output: Option<RustType>,
}

/// A generic parameter, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GenericParam {
    Lifetime(String),
    Type(String),
    Const(String),
}

/// A parameter of a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FnArg {
    /// `self`, `&self`, `mut self`, `&mut self` or `self: T`; `plain` for
    /// `self` and `&self`.
    Receiver {
        plain: bool,
    },
    Typed(PatType),
}
