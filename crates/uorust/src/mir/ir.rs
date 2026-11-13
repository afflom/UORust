//! MIR as the translator reads it: the bodies rustc dumps before
//! `ElaborateBoxDerefs` (drops elaborated, a `Box` still dereferenced as a
//! place), parsed from rustc's own printing into one small IR whose every
//! kind is a kind of [`super::spec`].
//!
//! The parser never guesses: a line it cannot read is kept as the kind it
//! starts with and the text, and the lowering turns it into a residue that
//! names the kind.

use super::spec;

/// A local, `_n`.
pub(crate) type IrLocal = usize;
/// A basic block, `bbn`.
pub(crate) type IrBlockId = usize;

/// One function body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IrBody {
    /// The path rustc prints in the header, `module::<impl at ...>::name`.
    pub(crate) path: String,
    /// The number of parameters: locals `_1` to `_n`.
    pub(crate) arity: usize,
    /// Each local's type, as rustc prints it; `_0` is the return place.
    pub(crate) locals: Vec<String>,
    /// The source name of a local, where debug info gives one.
    pub(crate) names: Vec<Option<String>>,
    /// A closure's captures, by their index in its environment `_1`: the
    /// source name and the type of the field that holds it, as debug info
    /// gives them.
    pub(crate) upvars: Vec<Option<(String, String)>>,
    pub(crate) blocks: Vec<IrBlock>,
}

/// A basic block: statements, then its terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IrBlock {
    pub(crate) statements: Vec<IrStatement>,
    pub(crate) terminator: IrTerminator,
}

/// A statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrStatement {
    Assign(IrPlace, IrRvalue),
    /// A statement with no effect on values: storage markers, fake reads,
    /// place mentions, type ascriptions, coverage counters, `nop`.
    Marker(spec::MirStatementKind),
    /// `assume(c)`: a fact the compiler may rely on.
    Assume(IrOperand),
    /// Any other statement: its kind and text.
    Other(spec::MirStatementKind, String),
}

/// A place: a local and the steps from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IrPlace {
    pub(crate) local: IrLocal,
    pub(crate) projection: Vec<IrProjection>,
}

/// A step of a place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrProjection {
    Deref,
    /// A field by index, with the field's type.
    Field(usize, String),
    /// An index by a local.
    Index(IrLocal),
    /// `[n of m]`, or `[-n of m]` from the end: an index by a constant,
    /// which a slice pattern reads after checking the length.
    ConstantIndex(u64, bool),
    /// The place as one variant of its enum.
    Downcast(String),
    /// Any other step: its kind and text.
    Other(spec::MirProjectionElem, String),
}

/// An operand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrOperand {
    Copy(IrPlace),
    Move(IrPlace),
    Constant(IrConstant),
    /// A runtime check, which is a constant of the build.
    Check(spec::MirRuntimeChecks),
}

/// A constant operand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrConstant {
    /// An integer literal and its type suffix (`u8`, `usize`, `i32`, ...).
    Int(i128, String),
    Bool(bool),
    /// A string literal, unescaped.
    Str(String),
    Char(char),
    Unit,
    /// A path: a function item, or a constant of a unit variant or a unit
    /// struct.
    Path(String),
    /// Any other constant: its text.
    Other(String),
}

/// What an aggregate builds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrAggregate {
    Tuple,
    Array,
    /// A struct, or a variant of an enum: the type's path, its generic
    /// arguments as printed, and the variant.
    Adt(String, Option<String>),
    /// A closure, by its type as printed (`{closure@file:l:c: l:c}`); its
    /// operands are its captures, named.
    Closure(String),
    /// Any other aggregate kind.
    Other(spec::MirAggregateKind),
}

/// The right side of an assignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrRvalue {
    Use(IrOperand),
    Ref(spec::MirBorrowKind, IrPlace),
    CopyForDeref(IrPlace),
    Binary(spec::MirBinOp, IrOperand, IrOperand),
    Unary(spec::MirUnOp, IrOperand),
    Discriminant(IrPlace),
    /// An aggregate and its operands; a struct aggregate's operands are
    /// named by field.
    Aggregate(IrAggregate, Vec<(Option<String>, IrOperand)>),
    Cast(spec::MirCastKind, IrOperand, String),
    /// `&raw const p` (`false`) or `&raw mut p` (`true`): a place's address.
    RawPtr(bool, IrPlace),
    /// Any other rvalue: its kind and text.
    Other(spec::MirRvalue, String),
}

/// A terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrTerminator {
    Goto(IrBlockId),
    /// `switchInt`: the operand, the value of each arm, and `otherwise`.
    Switch(IrOperand, Vec<(i128, IrBlockId)>, IrBlockId),
    Return,
    Unreachable,
    /// A drop: the value's destructor runs, then control goes on.
    Drop(IrBlockId),
    /// A call: where the result goes, the callee as printed, the arguments,
    /// and where control returns (none for a call that does not return).
    Call {
        destination: IrPlace,
        callee: String,
        args: Vec<IrOperand>,
        target: Option<IrBlockId>,
    },
    /// An assert: the condition, the value it must have, what it checks,
    /// and the block after it.
    Assert {
        condition: IrOperand,
        expected: bool,
        kind: spec::MirAssertKind,
        target: IrBlockId,
    },
    /// `falseEdge` and `falseUnwind`: control goes to the real edge.
    False(spec::MirTerminatorKind, IrBlockId),
    /// Any other terminator: its kind and text.
    Other(spec::MirTerminatorKind, String),
}

impl IrTerminator {
    /// The blocks control may go to next, on the paths that do not unwind.
    pub(crate) fn successors(&self) -> Vec<IrBlockId> {
        match self {
            Self::Goto(b) | Self::Drop(b) | Self::False(_, b) => vec![*b],
            Self::Switch(_, arms, otherwise) => {
                let mut out: Vec<IrBlockId> = arms.iter().map(|(_, b)| *b).collect();
                out.push(*otherwise);
                out
            }
            Self::Call { target, .. } => target.iter().copied().collect(),
            Self::Assert { target, .. } => vec![*target],
            Self::Return | Self::Unreachable | Self::Other(..) => Vec::new(),
        }
    }
}
