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
    pub(crate) fn expanded(&self, aliases: &BTreeMap<String, Alias>, depth: usize) -> RustType {
        if let TypeKind::Path { qself: false, path } = &self.kind {
            if let [one] = path.segments.as_slice() {
                if depth > 0 {
                    if let Some(alias) = aliases.get(&one.ident) {
                        let args: Vec<RustType> = match &one.arguments {
                            PathArguments::AngleBracketed(args) => args
                                .iter()
                                .filter_map(|a| match a {
                                    GenericArgument::Type(t) => Some(t.clone()),
                                    GenericArgument::Other(_) => None,
                                })
                                .collect(),
                            _ => Vec::new(),
                        };
                        if args.len() == alias.params.len() {
                            let bound: BTreeMap<String, RustType> =
                                alias.params.iter().cloned().zip(args).collect();
                            return alias.to.substituted(&bound).expanded(aliases, depth - 1);
                        }
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
            TypeKind::Array(t, n) => TypeKind::Array(inner(t), n.clone()),
            TypeKind::Fn(ins, out) => {
                TypeKind::Fn(ins.iter().map(|t| *inner(t)).collect(), inner(out))
            }
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

/// A type alias `type Name<P, ..> = To;`: its type parameters, and what
/// it stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Alias {
    pub(crate) params: Vec<String>,
    pub(crate) to: RustType,
}

impl RustType {
    /// The type with each type parameter `bound` names replaced by the
    /// type it is bound to.
    pub(crate) fn substituted(&self, bound: &BTreeMap<String, RustType>) -> RustType {
        if let TypeKind::Path { qself: false, path } = &self.kind {
            if let [one] = path.segments.as_slice() {
                if one.arguments == PathArguments::None {
                    if let Some(to) = bound.get(&one.ident) {
                        return to.clone();
                    }
                }
            }
        }
        let inner = |t: &RustType| Box::new(t.substituted(bound));
        let kind = match &self.kind {
            TypeKind::Reference { mutable, elem } => TypeKind::Reference {
                mutable: *mutable,
                elem: inner(elem),
            },
            TypeKind::Slice(t) => TypeKind::Slice(inner(t)),
            TypeKind::Array(t, n) => TypeKind::Array(inner(t), n.clone()),
            TypeKind::Fn(ins, out) => {
                TypeKind::Fn(ins.iter().map(|t| *inner(t)).collect(), inner(out))
            }
            TypeKind::Paren(t) => TypeKind::Paren(inner(t)),
            TypeKind::Tuple(ts) => {
                TypeKind::Tuple(ts.iter().map(|t| t.substituted(bound)).collect())
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
                                                GenericArgument::Type(t) => {
                                                    GenericArgument::Type(t.substituted(bound))
                                                }
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

impl RustType {
    /// The type with each path that names a type of this crate through
    /// its modules written as the type's own name: `crate::m::T`,
    /// `self::T`, `super::T`, or `m::T` where the file imports `m` from
    /// this crate (`imports`: imported name -> the path it names). A crate
    /// declares a type name once, so the name alone is the type.
    pub(crate) fn localized(&self, imports: &BTreeMap<String, String>) -> RustType {
        let inner = |t: &RustType| Box::new(t.localized(imports));
        let kind = match &self.kind {
            TypeKind::Reference { mutable, elem } => TypeKind::Reference {
                mutable: *mutable,
                elem: inner(elem),
            },
            TypeKind::Slice(t) => TypeKind::Slice(inner(t)),
            TypeKind::Array(t, n) => TypeKind::Array(inner(t), n.clone()),
            TypeKind::Fn(ins, out) => {
                TypeKind::Fn(ins.iter().map(|t| *inner(t)).collect(), inner(out))
            }
            TypeKind::Paren(t) => TypeKind::Paren(inner(t)),
            TypeKind::Tuple(ts) => {
                TypeKind::Tuple(ts.iter().map(|t| t.localized(imports)).collect())
            }
            TypeKind::Path { qself, path } => TypeKind::Path {
                qself: *qself,
                path: path.localized(imports, *qself),
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

/// Whether a path's first segment names a module of this crate: `crate`,
/// `self`, `super`, or a name the file imports from this crate.
fn crate_module(head: &str, imports: &BTreeMap<String, String>) -> bool {
    head == "crate"
        || head == "self"
        || head == "super"
        || imports
            .get(head)
            .is_some_and(|p| p == "crate" || p.starts_with("crate::"))
}

impl Path {
    /// The path with its generic arguments localized, and written as its
    /// last segment when the segments before it are modules of this crate.
    fn localized(&self, imports: &BTreeMap<String, String>, qself: bool) -> Path {
        let segments: Vec<PathSegment> = self
            .segments
            .iter()
            .map(|s| PathSegment {
                ident: s.ident.clone(),
                arguments: match &s.arguments {
                    PathArguments::AngleBracketed(args) => PathArguments::AngleBracketed(
                        args.iter()
                            .map(|a| match a {
                                GenericArgument::Type(t) => {
                                    GenericArgument::Type(t.localized(imports))
                                }
                                other => other.clone(),
                            })
                            .collect(),
                    ),
                    other => other.clone(),
                },
            })
            .collect();
        let modules = match segments.split_last() {
            Some((_, prefix)) => {
                !qself
                    && prefix
                        .first()
                        .is_some_and(|h| crate_module(&h.ident, imports))
                    && prefix.iter().all(|s| s.arguments == PathArguments::None)
            }
            None => false,
        };
        let kept: Vec<PathSegment> = if modules {
            segments.last().into_iter().cloned().collect()
        } else {
            segments
        };
        Path {
            segments: kept,
            text: self.text.clone(),
        }
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
        TypeKind::Array(t, n) => format!("[{}; {n}]", t.text),
        TypeKind::Fn(ins, out) => {
            let items: Vec<&str> = ins.iter().map(|t| t.text.as_str()).collect();
            format!("dyn Fn({}) -> {}", items.join(", "), out.text)
        }
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
    /// `[T; N]`, and the length as written.
    Array(Box<RustType>, String),
    /// `dyn Fn(A, ..) -> R` or `impl Fn(A, ..) -> R`: a pure function of
    /// its inputs.
    Fn(Vec<RustType>, Box<RustType>),
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
    /// A macro in statement position, such as `assert!(..);`.
    Macro(ExprMacro),
    /// An item in statement position.
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
    /// `loop { body }`.
    Loop(ExprLoop),
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
    /// `while cond { body }`; `while let p = e` has the `let` as its
    /// condition.
    While(ExprWhile),
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

/// `loop { body }`, and whether it has a label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprLoop {
    pub(crate) label: bool,
    pub(crate) body: Block,
}

/// `while cond { body }`, and whether it has a label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExprWhile {
    pub(crate) label: bool,
    pub(crate) cond: Box<RustExpr>,
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
    pub(crate) args: Option<MacroArgs>,
    pub(crate) matches: Option<MacroMatch>,
}

/// The arguments of a macro that read as a comma-separated list of
/// expressions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MacroArgs {
    pub(crate) items: Vec<RustExpr>,
}

/// The scrutinee and pattern of `matches!(e, P)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MacroMatch {
    pub(crate) expr: Box<RustExpr>,
    pub(crate) pat: Box<Pat>,
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
    pub(crate) rest: Option<Box<RustExpr>>,
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
            Self::Loop(l) => (Vec::new(), vec![&l.body]),
            Self::While(w) => (vec![&w.cond], vec![&w.body]),
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
            Self::Struct(s) => (
                s.fields
                    .iter()
                    .map(|f| &f.expr)
                    .chain(s.rest.as_deref())
                    .collect(),
                none,
            ),
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
            Self::Loop(_) => "loop",
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
            Self::While(_) => "while",
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

/// What a body says, in source order: the paths it calls and the methods
/// (as `.name`), its string literals, its assertions with their arguments,
/// and the transitions `E::A => E::B` its matches take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scan {
    pub(crate) calls: Vec<String>,
    pub(crate) literals: Vec<String>,
    pub(crate) asserts: Vec<(String, Vec<RustExpr>)>,
    pub(crate) transitions: Vec<(String, String, String)>,
}

impl Scan {
    /// Nothing said.
    pub(crate) fn none() -> Scan {
        Scan {
            calls: Vec::new(),
            literals: Vec::new(),
            asserts: Vec::new(),
            transitions: Vec::new(),
        }
    }

    /// This scan followed by `next`.
    fn then(self, next: Scan) -> Scan {
        let mut calls = self.calls;
        calls.extend(next.calls);
        let mut literals = self.literals;
        literals.extend(next.literals);
        let mut asserts = self.asserts;
        asserts.extend(next.asserts);
        let mut transitions = self.transitions;
        transitions.extend(next.transitions);
        Scan {
            calls,
            literals,
            asserts,
            transitions,
        }
    }

    /// The scans, one after another.
    fn all(scans: Vec<Scan>) -> Scan {
        scans.into_iter().fold(Scan::none(), |a, b| a.then(b))
    }
}

impl Block {
    /// What the block says.
    pub(crate) fn scan(&self) -> Scan {
        Scan::all(self.stmts.iter().map(|x| x.scan()).collect())
    }
}

impl Stmt {
    fn scan(&self) -> Scan {
        match self {
            Stmt::Local(l) => {
                let init = match &l.init {
                    Some(i) => {
                        let diverge = match &i.diverge {
                            Some(d) => d.scan(),
                            None => Scan::none(),
                        };
                        i.expr.scan().then(diverge)
                    }
                    None => Scan::none(),
                };
                l.pat.scan().then(init)
            }
            Stmt::Expr(e, _) => e.scan(),
            Stmt::Macro(m) => m.scan(),
            Stmt::Other => Scan::none(),
        }
    }
}

impl ExprMacro {
    /// An assertion's arguments, then what each argument says.
    fn scan(&self) -> Scan {
        let name = self
            .path
            .segments
            .last()
            .map(|s| s.ident.clone())
            .unwrap_or_default();
        let Some(MacroArgs { items: args }) = &self.args else {
            return Scan::none();
        };
        let own = if matches!(name.as_str(), "assert" | "assert_eq" | "assert_ne") {
            Scan {
                asserts: vec![(name, args.clone())],
                ..Scan::none()
            }
        } else {
            Scan::none()
        };
        own.then(Scan::all(args.iter().map(|x| x.scan()).collect()))
    }
}

impl Pat {
    /// The string literals a pattern tests and what its guards say.
    fn scan(&self) -> Scan {
        match self {
            Pat::Lit(Lit::Str(text)) => Scan {
                literals: vec![text.clone()],
                ..Scan::none()
            },
            Pat::Ident(i) => match &i.subpat {
                Some(p) => p.scan(),
                None => Scan::none(),
            },
            Pat::Reference(p) => p.scan(),
            Pat::Type(t) => t.pat.scan(),
            Pat::Tuple(ps) | Pat::Slice(ps) | Pat::Or(ps) => {
                Scan::all(ps.iter().map(|x| x.scan()).collect())
            }
            Pat::TupleStruct(ts) => Scan::all(ts.elems.iter().map(|x| x.scan()).collect()),
            Pat::Struct(ps) => Scan::all(ps.fields.iter().map(|f| f.pat.scan()).collect()),
            Pat::Range(r) => {
                let ends: Vec<Scan> = r
                    .start
                    .iter()
                    .chain(r.end.iter())
                    .map(|e| e.scan())
                    .collect();
                Scan::all(ends)
            }
            Pat::Guard(g) => g.pat.scan().then(g.guard.scan()),
            Pat::Wild | Pat::Rest | Pat::Lit(_) | Pat::Path(_) | Pat::Other => Scan::none(),
        }
    }
}

/// The segments of every path an arm body can evaluate to: through `if` and
/// `else` branches when `branches`, and through a one-expression block,
/// parentheses or `Some(..)`/`Ok(..)`.
fn arm_paths(e: &RustExpr, branches: bool) -> Vec<Vec<String>> {
    let segments = |p: &Path| p.segments.iter().map(|s| s.ident.clone()).collect();
    match e {
        RustExpr::If(i) if branches => {
            let then: Vec<Vec<String>> = match i.then_branch.stmts.as_slice() {
                [Stmt::Expr(e, false)] => arm_paths(e, true),
                _ => Vec::new(),
            };
            let other = match &i.else_branch {
                Some(e) => arm_paths(e, true),
                None => Vec::new(),
            };
            then.into_iter().chain(other).collect()
        }
        RustExpr::Block(b) => match b.block.stmts.as_slice() {
            [Stmt::Expr(e, false)] => arm_paths(e, branches),
            _ => Vec::new(),
        },
        RustExpr::Path(p) => vec![segments(p)],
        RustExpr::Call(c) => match (&*c.func, c.args.as_slice()) {
            (RustExpr::Path(p), [arg]) if p.is_ident("Some") || p.is_ident("Ok") => {
                arm_paths(arg, false)
            }
            _ => Vec::new(),
        },
        RustExpr::Paren(p) => arm_paths(p, false),
        _ => Vec::new(),
    }
}

/// The transitions `E::A => E::B` a match's arms take: an arm on a unit
/// variant whose body evaluates to a variant of the same enum.
fn transitions(m: &ExprMatch) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for arm in &m.arms {
        let Pat::Path(p) = &arm.pat else {
            continue;
        };
        let from: Vec<String> = p.segments.iter().map(|s| s.ident.clone()).collect();
        let Some((e1, a)) = crate::ingest::enum_path(&from) else {
            continue;
        };
        for target in arm_paths(&arm.body, true) {
            if let Some((e2, b)) = crate::ingest::enum_path(&target) {
                if e1 == e2 {
                    out.push((e1.clone(), a.clone(), b));
                }
            }
        }
    }
    out
}

impl RustExpr {
    /// What the expression says, in source order.
    pub(crate) fn scan(&self) -> Scan {
        let exprs = |es: Vec<&RustExpr>| Scan::all(es.into_iter().map(|x| x.scan()).collect());
        match self {
            RustExpr::Call(c) => {
                let own = match &*c.func {
                    RustExpr::Path(p) => Scan {
                        calls: vec![p.text.clone()],
                        ..Scan::none()
                    },
                    _ => Scan::none(),
                };
                own.then(c.func.scan()).then(exprs(c.args.iter().collect()))
            }
            RustExpr::MethodCall(m) => {
                let own = Scan {
                    calls: vec![format!(".{}", m.method)],
                    ..Scan::none()
                };
                own.then(m.receiver.scan())
                    .then(exprs(m.args.iter().collect()))
            }
            RustExpr::Lit(Lit::Str(text)) => Scan {
                literals: vec![text.clone()],
                ..Scan::none()
            },
            RustExpr::Macro(m) => m.scan(),
            RustExpr::If(i) => {
                let other = match &i.else_branch {
                    Some(e) => e.scan(),
                    None => Scan::none(),
                };
                i.cond.scan().then(i.then_branch.scan()).then(other)
            }
            RustExpr::Match(m) => {
                let own = Scan {
                    transitions: transitions(m),
                    ..Scan::none()
                };
                let arms: Vec<Scan> = m
                    .arms
                    .iter()
                    .map(|a| a.pat.scan().then(a.body.scan()))
                    .collect();
                own.then(m.expr.scan()).then(Scan::all(arms))
            }
            RustExpr::Let(l) => l.pat.scan().then(l.expr.scan()),
            RustExpr::Closure(c) => {
                Scan::all(c.inputs.iter().map(|x| x.scan()).collect()).then(c.body.scan())
            }
            RustExpr::ForLoop(f) => f.pat.scan().then(f.expr.scan()).then(f.body.scan()),
            RustExpr::Other(o) => exprs(o.exprs.iter().collect())
                .then(Scan::all(o.blocks.iter().map(|x| x.scan()).collect())),
            other => {
                let (es, blocks) = other.parts();
                exprs(es).then(Scan::all(blocks.into_iter().map(|x| x.scan()).collect()))
            }
        }
    }
}
