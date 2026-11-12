//! MIR's kinds, generated from rustc's own definition of MIR at toolchain
//! 1.97.1 (`crates/uorust/mir/syntax.rs`, SHA-256
//! 055bfc5e061f716a64beea97b701bef2dbd9e7e4b9d4104daca9d3c9eb4fc5f4) by
//! `scripts/mir/fetch-syntax.sh`. Do not edit: the translator matches each
//! enum exhaustively, so its coverage of MIR is complete by construction, and
//! a kind it does not lower is a typed residue that names the kind.

/// The toolchain whose MIR these kinds are.
pub(crate) const TOOLCHAIN: &str = "1.97.1";

/// The kinds of `rustc_middle::mir::StatementKind`: a statement of a basic block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirStatementKind {
    Assign,
    FakeRead,
    SetDiscriminant,
    StorageLive,
    StorageDead,
    PlaceMention,
    AscribeUserType,
    Coverage,
    Intrinsic,
    ConstEvalCounter,
    Nop,
    BackwardIncompatibleDropHint,
}

impl MirStatementKind {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Assign,
        Self::FakeRead,
        Self::SetDiscriminant,
        Self::StorageLive,
        Self::StorageDead,
        Self::PlaceMention,
        Self::AscribeUserType,
        Self::Coverage,
        Self::Intrinsic,
        Self::ConstEvalCounter,
        Self::Nop,
        Self::BackwardIncompatibleDropHint,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Assign => "StatementKind::Assign",
            Self::FakeRead => "StatementKind::FakeRead",
            Self::SetDiscriminant => "StatementKind::SetDiscriminant",
            Self::StorageLive => "StatementKind::StorageLive",
            Self::StorageDead => "StatementKind::StorageDead",
            Self::PlaceMention => "StatementKind::PlaceMention",
            Self::AscribeUserType => "StatementKind::AscribeUserType",
            Self::Coverage => "StatementKind::Coverage",
            Self::Intrinsic => "StatementKind::Intrinsic",
            Self::ConstEvalCounter => "StatementKind::ConstEvalCounter",
            Self::Nop => "StatementKind::Nop",
            Self::BackwardIncompatibleDropHint => "StatementKind::BackwardIncompatibleDropHint",
        }
    }
}

/// The kinds of `rustc_middle::mir::NonDivergingIntrinsic`: an intrinsic statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirNonDivergingIntrinsic {
    Assume,
    CopyNonOverlapping,
}

impl MirNonDivergingIntrinsic {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Assume,
        Self::CopyNonOverlapping,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Assume => "NonDivergingIntrinsic::Assume",
            Self::CopyNonOverlapping => "NonDivergingIntrinsic::CopyNonOverlapping",
        }
    }
}

/// The kinds of `rustc_middle::mir::TerminatorKind`: the terminator of a basic block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirTerminatorKind {
    Goto,
    SwitchInt,
    UnwindResume,
    UnwindTerminate,
    Return,
    Unreachable,
    Drop,
    Call,
    TailCall,
    Assert,
    Yield,
    CoroutineDrop,
    FalseEdge,
    FalseUnwind,
    InlineAsm,
}

impl MirTerminatorKind {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Goto,
        Self::SwitchInt,
        Self::UnwindResume,
        Self::UnwindTerminate,
        Self::Return,
        Self::Unreachable,
        Self::Drop,
        Self::Call,
        Self::TailCall,
        Self::Assert,
        Self::Yield,
        Self::CoroutineDrop,
        Self::FalseEdge,
        Self::FalseUnwind,
        Self::InlineAsm,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Goto => "TerminatorKind::Goto",
            Self::SwitchInt => "TerminatorKind::SwitchInt",
            Self::UnwindResume => "TerminatorKind::UnwindResume",
            Self::UnwindTerminate => "TerminatorKind::UnwindTerminate",
            Self::Return => "TerminatorKind::Return",
            Self::Unreachable => "TerminatorKind::Unreachable",
            Self::Drop => "TerminatorKind::Drop",
            Self::Call => "TerminatorKind::Call",
            Self::TailCall => "TerminatorKind::TailCall",
            Self::Assert => "TerminatorKind::Assert",
            Self::Yield => "TerminatorKind::Yield",
            Self::CoroutineDrop => "TerminatorKind::CoroutineDrop",
            Self::FalseEdge => "TerminatorKind::FalseEdge",
            Self::FalseUnwind => "TerminatorKind::FalseUnwind",
            Self::InlineAsm => "TerminatorKind::InlineAsm",
        }
    }
}

/// The kinds of `rustc_middle::mir::Rvalue`: the right side of an assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirRvalue {
    Use,
    Repeat,
    Ref,
    ThreadLocalRef,
    RawPtr,
    Cast,
    BinaryOp,
    UnaryOp,
    Discriminant,
    Aggregate,
    CopyForDeref,
    WrapUnsafeBinder,
    Reborrow,
}

impl MirRvalue {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Use,
        Self::Repeat,
        Self::Ref,
        Self::ThreadLocalRef,
        Self::RawPtr,
        Self::Cast,
        Self::BinaryOp,
        Self::UnaryOp,
        Self::Discriminant,
        Self::Aggregate,
        Self::CopyForDeref,
        Self::WrapUnsafeBinder,
        Self::Reborrow,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Use => "Rvalue::Use",
            Self::Repeat => "Rvalue::Repeat",
            Self::Ref => "Rvalue::Ref",
            Self::ThreadLocalRef => "Rvalue::ThreadLocalRef",
            Self::RawPtr => "Rvalue::RawPtr",
            Self::Cast => "Rvalue::Cast",
            Self::BinaryOp => "Rvalue::BinaryOp",
            Self::UnaryOp => "Rvalue::UnaryOp",
            Self::Discriminant => "Rvalue::Discriminant",
            Self::Aggregate => "Rvalue::Aggregate",
            Self::CopyForDeref => "Rvalue::CopyForDeref",
            Self::WrapUnsafeBinder => "Rvalue::WrapUnsafeBinder",
            Self::Reborrow => "Rvalue::Reborrow",
        }
    }
}

/// The kinds of `rustc_middle::mir::Operand`: an operand of an rvalue or a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirOperand {
    Copy,
    Move,
    Constant,
    RuntimeChecks,
}

impl MirOperand {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Copy,
        Self::Move,
        Self::Constant,
        Self::RuntimeChecks,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Copy => "Operand::Copy",
            Self::Move => "Operand::Move",
            Self::Constant => "Operand::Constant",
            Self::RuntimeChecks => "Operand::RuntimeChecks",
        }
    }
}

/// The kinds of `rustc_middle::mir::ProjectionElem`: a step of a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirProjectionElem {
    Deref,
    Field,
    Index,
    ConstantIndex,
    Subslice,
    Downcast,
    OpaqueCast,
    UnwrapUnsafeBinder,
}

impl MirProjectionElem {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Deref,
        Self::Field,
        Self::Index,
        Self::ConstantIndex,
        Self::Subslice,
        Self::Downcast,
        Self::OpaqueCast,
        Self::UnwrapUnsafeBinder,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Deref => "ProjectionElem::Deref",
            Self::Field => "ProjectionElem::Field",
            Self::Index => "ProjectionElem::Index",
            Self::ConstantIndex => "ProjectionElem::ConstantIndex",
            Self::Subslice => "ProjectionElem::Subslice",
            Self::Downcast => "ProjectionElem::Downcast",
            Self::OpaqueCast => "ProjectionElem::OpaqueCast",
            Self::UnwrapUnsafeBinder => "ProjectionElem::UnwrapUnsafeBinder",
        }
    }
}

/// The kinds of `rustc_middle::mir::BorrowKind`: the kind of a reference rvalue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirBorrowKind {
    Shared,
    Fake,
    Mut,
}

impl MirBorrowKind {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Shared,
        Self::Fake,
        Self::Mut,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Shared => "BorrowKind::Shared",
            Self::Fake => "BorrowKind::Fake",
            Self::Mut => "BorrowKind::Mut",
        }
    }
}

/// The kinds of `rustc_middle::mir::CastKind`: the kind of a cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirCastKind {
    PointerExposeProvenance,
    PointerWithExposedProvenance,
    PointerCoercion,
    IntToInt,
    FloatToInt,
    FloatToFloat,
    IntToFloat,
    PtrToPtr,
    FnPtrToPtr,
    Transmute,
    Subtype,
}

impl MirCastKind {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::PointerExposeProvenance,
        Self::PointerWithExposedProvenance,
        Self::PointerCoercion,
        Self::IntToInt,
        Self::FloatToInt,
        Self::FloatToFloat,
        Self::IntToFloat,
        Self::PtrToPtr,
        Self::FnPtrToPtr,
        Self::Transmute,
        Self::Subtype,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::PointerExposeProvenance => "CastKind::PointerExposeProvenance",
            Self::PointerWithExposedProvenance => "CastKind::PointerWithExposedProvenance",
            Self::PointerCoercion => "CastKind::PointerCoercion",
            Self::IntToInt => "CastKind::IntToInt",
            Self::FloatToInt => "CastKind::FloatToInt",
            Self::FloatToFloat => "CastKind::FloatToFloat",
            Self::IntToFloat => "CastKind::IntToFloat",
            Self::PtrToPtr => "CastKind::PtrToPtr",
            Self::FnPtrToPtr => "CastKind::FnPtrToPtr",
            Self::Transmute => "CastKind::Transmute",
            Self::Subtype => "CastKind::Subtype",
        }
    }
}

/// The kinds of `rustc_middle::mir::AggregateKind`: the kind of an aggregate rvalue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirAggregateKind {
    Array,
    Tuple,
    Adt,
    Closure,
    Coroutine,
    CoroutineClosure,
    RawPtr,
}

impl MirAggregateKind {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Array,
        Self::Tuple,
        Self::Adt,
        Self::Closure,
        Self::Coroutine,
        Self::CoroutineClosure,
        Self::RawPtr,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Array => "AggregateKind::Array",
            Self::Tuple => "AggregateKind::Tuple",
            Self::Adt => "AggregateKind::Adt",
            Self::Closure => "AggregateKind::Closure",
            Self::Coroutine => "AggregateKind::Coroutine",
            Self::CoroutineClosure => "AggregateKind::CoroutineClosure",
            Self::RawPtr => "AggregateKind::RawPtr",
        }
    }
}

/// The kinds of `rustc_middle::mir::BinOp`: a binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirBinOp {
    Add,
    AddUnchecked,
    AddWithOverflow,
    Sub,
    SubUnchecked,
    SubWithOverflow,
    Mul,
    MulUnchecked,
    MulWithOverflow,
    Div,
    Rem,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    ShlUnchecked,
    Shr,
    ShrUnchecked,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    Cmp,
    Offset,
}

impl MirBinOp {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Add,
        Self::AddUnchecked,
        Self::AddWithOverflow,
        Self::Sub,
        Self::SubUnchecked,
        Self::SubWithOverflow,
        Self::Mul,
        Self::MulUnchecked,
        Self::MulWithOverflow,
        Self::Div,
        Self::Rem,
        Self::BitXor,
        Self::BitAnd,
        Self::BitOr,
        Self::Shl,
        Self::ShlUnchecked,
        Self::Shr,
        Self::ShrUnchecked,
        Self::Eq,
        Self::Lt,
        Self::Le,
        Self::Ne,
        Self::Ge,
        Self::Gt,
        Self::Cmp,
        Self::Offset,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Add => "BinOp::Add",
            Self::AddUnchecked => "BinOp::AddUnchecked",
            Self::AddWithOverflow => "BinOp::AddWithOverflow",
            Self::Sub => "BinOp::Sub",
            Self::SubUnchecked => "BinOp::SubUnchecked",
            Self::SubWithOverflow => "BinOp::SubWithOverflow",
            Self::Mul => "BinOp::Mul",
            Self::MulUnchecked => "BinOp::MulUnchecked",
            Self::MulWithOverflow => "BinOp::MulWithOverflow",
            Self::Div => "BinOp::Div",
            Self::Rem => "BinOp::Rem",
            Self::BitXor => "BinOp::BitXor",
            Self::BitAnd => "BinOp::BitAnd",
            Self::BitOr => "BinOp::BitOr",
            Self::Shl => "BinOp::Shl",
            Self::ShlUnchecked => "BinOp::ShlUnchecked",
            Self::Shr => "BinOp::Shr",
            Self::ShrUnchecked => "BinOp::ShrUnchecked",
            Self::Eq => "BinOp::Eq",
            Self::Lt => "BinOp::Lt",
            Self::Le => "BinOp::Le",
            Self::Ne => "BinOp::Ne",
            Self::Ge => "BinOp::Ge",
            Self::Gt => "BinOp::Gt",
            Self::Cmp => "BinOp::Cmp",
            Self::Offset => "BinOp::Offset",
        }
    }
}

/// The kinds of `rustc_middle::mir::UnOp`: a unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirUnOp {
    Not,
    Neg,
    PtrMetadata,
}

impl MirUnOp {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::Not,
        Self::Neg,
        Self::PtrMetadata,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Not => "UnOp::Not",
            Self::Neg => "UnOp::Neg",
            Self::PtrMetadata => "UnOp::PtrMetadata",
        }
    }
}

/// The kinds of `rustc_middle::mir::RuntimeChecks`: a runtime-check operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirRuntimeChecks {
    UbChecks,
    ContractChecks,
    OverflowChecks,
}

impl MirRuntimeChecks {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::UbChecks,
        Self::ContractChecks,
        Self::OverflowChecks,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::UbChecks => "RuntimeChecks::UbChecks",
            Self::ContractChecks => "RuntimeChecks::ContractChecks",
            Self::OverflowChecks => "RuntimeChecks::OverflowChecks",
        }
    }
}

/// The kinds of `rustc_middle::mir::AssertKind`: what an assert terminator checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // rustc's own names
pub(crate) enum MirAssertKind {
    BoundsCheck,
    Overflow,
    OverflowNeg,
    DivisionByZero,
    RemainderByZero,
    ResumedAfterReturn,
    ResumedAfterPanic,
    ResumedAfterDrop,
    MisalignedPointerDereference,
    NullPointerDereference,
    InvalidEnumConstruction,
}

impl MirAssertKind {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &[Self] = &[
        Self::BoundsCheck,
        Self::Overflow,
        Self::OverflowNeg,
        Self::DivisionByZero,
        Self::RemainderByZero,
        Self::ResumedAfterReturn,
        Self::ResumedAfterPanic,
        Self::ResumedAfterDrop,
        Self::MisalignedPointerDereference,
        Self::NullPointerDereference,
        Self::InvalidEnumConstruction,
    ];

    /// The kind's path, `Enum::Variant`.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::BoundsCheck => "AssertKind::BoundsCheck",
            Self::Overflow => "AssertKind::Overflow",
            Self::OverflowNeg => "AssertKind::OverflowNeg",
            Self::DivisionByZero => "AssertKind::DivisionByZero",
            Self::RemainderByZero => "AssertKind::RemainderByZero",
            Self::ResumedAfterReturn => "AssertKind::ResumedAfterReturn",
            Self::ResumedAfterPanic => "AssertKind::ResumedAfterPanic",
            Self::ResumedAfterDrop => "AssertKind::ResumedAfterDrop",
            Self::MisalignedPointerDereference => "AssertKind::MisalignedPointerDereference",
            Self::NullPointerDereference => "AssertKind::NullPointerDereference",
            Self::InvalidEnumConstruction => "AssertKind::InvalidEnumConstruction",
        }
    }
}
