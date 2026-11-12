
use rustc_abi::{FieldIdx, VariantIdx};
use rustc_ast::{InlineAsmOptions, InlineAsmTemplatePiece, Mutability};
use rustc_data_structures::packed::Pu128;
use rustc_hir::CoroutineKind;
use rustc_hir::def_id::DefId;
use rustc_index::IndexVec;
use rustc_macros::{StableHash, TyDecodable, TyEncodable, TypeFoldable, TypeVisitable};
use rustc_span::def_id::LocalDefId;
use rustc_span::{Span, Spanned, Symbol};
use rustc_target::asm::InlineAsmRegOrRegClass;
use smallvec::SmallVec;

use super::{BasicBlock, Const, Local, UserTypeProjection};
use crate::mir::coverage::CoverageKind;
use crate::ty::adjustment::PointerCoercion;
use crate::ty::{self, GenericArgsRef, List, Region, Ty, UserTypeAnnotationIndex};

#[derive(Copy, Clone, TyEncodable, TyDecodable, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[derive(StableHash)]
pub enum MirPhase {
    Built,

    Analysis(AnalysisPhase),

    Runtime(RuntimePhase),
}

#[derive(Copy, Clone, TyEncodable, TyDecodable, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[derive(StableHash)]
pub enum AnalysisPhase {
    Initial = 0,
    PostCleanup = 1,
}

#[derive(Copy, Clone, TyEncodable, TyDecodable, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[derive(StableHash)]
pub enum RuntimePhase {
    Initial = 0,
    PostCleanup = 1,
    Optimized = 2,
}


#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, TyEncodable, TyDecodable)]
#[derive(Hash, StableHash)]
pub enum BorrowKind {
    Shared,

    Fake(FakeBorrowKind),

    Mut { kind: MutBorrowKind },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, TyEncodable, TyDecodable)]
#[derive(Hash, StableHash)]
pub enum RawPtrKind {
    Mut,
    Const,
    FakeForPtrMetadata,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, TyEncodable, TyDecodable)]
#[derive(Hash, StableHash)]
pub enum MutBorrowKind {
    Default,
    TwoPhaseBorrow,
    ClosureCapture,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, TyEncodable, TyDecodable)]
#[derive(Hash, StableHash)]
pub enum FakeBorrowKind {
    Shallow,
    Deep,
}


#[derive(Clone, PartialEq, TyEncodable, TyDecodable, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub enum StatementKind<'tcx> {
    Assign(Box<(Place<'tcx>, Rvalue<'tcx>)>),

    FakeRead(Box<(FakeReadCause, Place<'tcx>)>),

    SetDiscriminant { place: Box<Place<'tcx>>, variant_index: VariantIdx },

    StorageLive(Local),

    StorageDead(Local),

    PlaceMention(Box<Place<'tcx>>),

    AscribeUserType(Box<(Place<'tcx>, UserTypeProjection)>, ty::Variance),

    Coverage(
        #[type_foldable(identity)]
        #[type_visitable(ignore)]
        CoverageKind,
    ),

    Intrinsic(Box<NonDivergingIntrinsic<'tcx>>),

    ConstEvalCounter,

    Nop,

    BackwardIncompatibleDropHint {
        place: Box<Place<'tcx>>,
        reason: BackwardIncompatibleDropReason,
    },
}

#[derive(
    Clone,
    TyEncodable,
    TyDecodable,
    Debug,
    PartialEq,
    Hash,
    StableHash,
    TypeFoldable,
    TypeVisitable
)]
pub enum NonDivergingIntrinsic<'tcx> {
    Assume(Operand<'tcx>),

    CopyNonOverlapping(CopyNonOverlapping<'tcx>),
}

#[derive(Copy, Clone, TyEncodable, TyDecodable, Debug, PartialEq, Eq, Hash, StableHash)]
#[rustc_pass_by_value]
pub enum WithRetag {
    Yes,
    No,
}

impl WithRetag {
    #[inline]
    pub fn yes(self) -> bool {
        matches!(self, Self::Yes)
    }
    #[inline]
    pub fn no(self) -> bool {
        matches!(self, Self::No)
    }
}

#[derive(Copy, Clone, TyEncodable, TyDecodable, Debug, Hash, StableHash, PartialEq)]
pub enum FakeReadCause {
    ForMatchGuard,

    ForMatchedPlace(Option<LocalDefId>),

    ForGuardBinding,

    ForLet(Option<LocalDefId>),

    ForIndex,
}

#[derive(Clone, Debug, PartialEq, TyEncodable, TyDecodable, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub struct CopyNonOverlapping<'tcx> {
    pub src: Operand<'tcx>,
    pub dst: Operand<'tcx>,
    pub count: Operand<'tcx>,
}

#[derive(Clone, Copy, TyEncodable, TyDecodable, Debug, PartialEq, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub enum CallSource {
    OverloadedOperator,
    MatchCmp,
    Misc,
    Use,
    Normal,
}

#[derive(Clone, Copy, Debug, TyEncodable, TyDecodable, Hash, StableHash, PartialEq)]
#[derive(TypeFoldable, TypeVisitable)]
pub enum InlineAsmMacro {
    Asm,
    NakedAsm,
}


#[derive(Clone, TyEncodable, TyDecodable, Hash, StableHash, PartialEq, TypeFoldable, TypeVisitable)]
pub enum TerminatorKind<'tcx> {
    Goto { target: BasicBlock },

    SwitchInt {
        discr: Operand<'tcx>,
        targets: SwitchTargets,
    },

    UnwindResume,

    UnwindTerminate(UnwindTerminateReason),

    Return,

    Unreachable,

    Drop {
        place: Place<'tcx>,
        target: BasicBlock,
        unwind: UnwindAction,
        replace: bool,
        drop: Option<BasicBlock>,
        async_fut: Option<Local>,
    },

    Call {
        func: Operand<'tcx>,
        args: Box<[Spanned<Operand<'tcx>>]>,
        destination: Place<'tcx>,
        target: Option<BasicBlock>,
        unwind: UnwindAction,
        call_source: CallSource,
        fn_span: Span,
    },

    TailCall {
        func: Operand<'tcx>,
        args: Box<[Spanned<Operand<'tcx>>]>,
        fn_span: Span,
    },

    Assert {
        cond: Operand<'tcx>,
        expected: bool,
        msg: Box<AssertMessage<'tcx>>,
        target: BasicBlock,
        unwind: UnwindAction,
    },

    Yield {
        value: Operand<'tcx>,
        resume: BasicBlock,
        resume_arg: Place<'tcx>,
        drop: Option<BasicBlock>,
    },

    CoroutineDrop,

    FalseEdge {
        real_target: BasicBlock,
        imaginary_target: BasicBlock,
    },

    FalseUnwind {
        real_target: BasicBlock,
        unwind: UnwindAction,
    },

    InlineAsm {
        asm_macro: InlineAsmMacro,

        #[type_foldable(identity)]
        #[type_visitable(ignore)]
        template: &'tcx [InlineAsmTemplatePiece],

        operands: Box<[InlineAsmOperand<'tcx>]>,

        options: InlineAsmOptions,

        #[type_foldable(identity)]
        #[type_visitable(ignore)]
        line_spans: &'tcx [Span],

        targets: Box<[BasicBlock]>,

        unwind: UnwindAction,
    },
}

#[derive(
    Clone,
    Debug,
    TyEncodable,
    TyDecodable,
    Hash,
    StableHash,
    PartialEq,
    TypeFoldable,
    TypeVisitable
)]
pub enum BackwardIncompatibleDropReason {
    Edition2024,
}

#[derive(Debug, Clone, TyEncodable, TyDecodable, Hash, StableHash, PartialEq)]
pub struct SwitchTargets {
    pub(super) values: SmallVec<[Pu128; 1]>,

    pub(super) targets: SmallVec<[BasicBlock; 2]>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, TyEncodable, TyDecodable, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub enum UnwindAction {
    Continue,
    Unreachable,
    Terminate(UnwindTerminateReason),
    Cleanup(BasicBlock),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, TyEncodable, TyDecodable, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub enum UnwindTerminateReason {
    Abi,
    InCleanup,
}

#[derive(Clone, Hash, StableHash, PartialEq, Debug)]
#[derive(TyEncodable, TyDecodable, TypeFoldable, TypeVisitable)]
pub enum AssertKind<O> {
    BoundsCheck { len: O, index: O },
    Overflow(BinOp, O, O),
    OverflowNeg(O),
    DivisionByZero(O),
    RemainderByZero(O),
    ResumedAfterReturn(CoroutineKind),
    ResumedAfterPanic(CoroutineKind),
    ResumedAfterDrop(CoroutineKind),
    MisalignedPointerDereference { required: O, found: O },
    NullPointerDereference,
    InvalidEnumConstruction(O),
}

#[derive(Clone, Debug, PartialEq, TyEncodable, TyDecodable, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub enum InlineAsmOperand<'tcx> {
    In {
        reg: InlineAsmRegOrRegClass,
        value: Operand<'tcx>,
    },
    Out {
        reg: InlineAsmRegOrRegClass,
        late: bool,
        place: Option<Place<'tcx>>,
    },
    InOut {
        reg: InlineAsmRegOrRegClass,
        late: bool,
        in_value: Operand<'tcx>,
        out_place: Option<Place<'tcx>>,
    },
    Const {
        value: Box<ConstOperand<'tcx>>,
    },
    SymFn {
        value: Box<ConstOperand<'tcx>>,
    },
    SymStatic {
        def_id: DefId,
    },
    Label {
        target_index: usize,
    },
}

pub type AssertMessage<'tcx> = AssertKind<Operand<'tcx>>;


#[derive(Copy, Clone, PartialEq, Eq, Hash, TyEncodable, StableHash, TypeFoldable, TypeVisitable)]
pub struct Place<'tcx> {
    pub local: Local,

    pub projection: &'tcx List<PlaceElem<'tcx>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[derive(TyEncodable, TyDecodable, StableHash, TypeFoldable, TypeVisitable)]
pub enum ProjectionElem<V, T> {
    Deref,

    Field(FieldIdx, T),

    Index(V),

    ConstantIndex {
        offset: u64,
        min_length: u64,
        from_end: bool,
    },

    Subslice {
        from: u64,
        to: u64,
        from_end: bool,
    },

    Downcast(Option<Symbol>, VariantIdx),

    OpaqueCast(T),

    UnwrapUnsafeBinder(T),
}

pub type PlaceElem<'tcx> = ProjectionElem<Local, Ty<'tcx>>;


#[derive(Clone, PartialEq, TyEncodable, TyDecodable, Hash, StableHash, TypeFoldable, TypeVisitable)]
pub enum Operand<'tcx> {
    Copy(Place<'tcx>),

    Move(Place<'tcx>),

    Constant(Box<ConstOperand<'tcx>>),

    RuntimeChecks(RuntimeChecks),
}

#[derive(Clone, Copy, PartialEq, TyEncodable, TyDecodable, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub struct ConstOperand<'tcx> {
    pub span: Span,

    pub user_ty: Option<UserTypeAnnotationIndex>,

    pub const_: Const<'tcx>,
}


#[derive(Clone, TyEncodable, TyDecodable, Hash, StableHash, PartialEq, TypeFoldable, TypeVisitable)]
pub enum Rvalue<'tcx> {
    Use(Operand<'tcx>, WithRetag),

    Repeat(Operand<'tcx>, ty::Const<'tcx>),

    Ref(Region<'tcx>, BorrowKind, Place<'tcx>),

    ThreadLocalRef(DefId),

    RawPtr(RawPtrKind, Place<'tcx>),

    Cast(CastKind, Operand<'tcx>, Ty<'tcx>),

    BinaryOp(BinOp, Box<(Operand<'tcx>, Operand<'tcx>)>),

    UnaryOp(UnOp, Operand<'tcx>),

    Discriminant(Place<'tcx>),

    Aggregate(Box<AggregateKind<'tcx>>, IndexVec<FieldIdx, Operand<'tcx>>),

    CopyForDeref(Place<'tcx>),

    WrapUnsafeBinder(Operand<'tcx>, Ty<'tcx>),

    Reborrow(Ty<'tcx>, Mutability, Place<'tcx>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TyEncodable, TyDecodable, Hash, StableHash)]
pub enum CastKind {
    PointerExposeProvenance,
    PointerWithExposedProvenance,
    PointerCoercion(PointerCoercion, CoercionSource),
    IntToInt,
    FloatToInt,
    FloatToFloat,
    IntToFloat,
    PtrToPtr,
    FnPtrToPtr,
    Transmute,

    Subtype,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TyEncodable, TyDecodable, Hash, StableHash)]
pub enum CoercionSource {
    AsCast,
    Implicit,
}

#[derive(Clone, Debug, PartialEq, Eq, TyEncodable, TyDecodable, Hash, StableHash)]
#[derive(TypeFoldable, TypeVisitable)]
pub enum AggregateKind<'tcx> {
    Array(Ty<'tcx>),
    Tuple,

    Adt(DefId, VariantIdx, GenericArgsRef<'tcx>, Option<UserTypeAnnotationIndex>, Option<FieldIdx>),

    Closure(DefId, GenericArgsRef<'tcx>),
    Coroutine(DefId, GenericArgsRef<'tcx>),
    CoroutineClosure(DefId, GenericArgsRef<'tcx>),

    RawPtr(Ty<'tcx>, Mutability),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, TyEncodable, TyDecodable, Hash, StableHash)]
pub enum RuntimeChecks {
    UbChecks,
    ContractChecks,
    OverflowChecks,
}

impl RuntimeChecks {
    pub fn value(self, sess: &rustc_session::Session) -> bool {
        match self {
            Self::UbChecks => sess.ub_checks(),
            Self::ContractChecks => sess.contract_checks(),
            Self::OverflowChecks => sess.overflow_checks(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[derive(StableHash, TyEncodable, TyDecodable, TypeFoldable, TypeVisitable)]
pub enum UnOp {
    Not,
    Neg,
    PtrMetadata,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[derive(TyEncodable, TyDecodable, StableHash, TypeFoldable, TypeVisitable)]
pub enum BinOp {
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, StableHash)]
pub enum AssignOp {
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

impl From<AssignOp> for BinOp {
    fn from(op: AssignOp) -> BinOp {
        match op {
            AssignOp::AddAssign => BinOp::Add,
            AssignOp::SubAssign => BinOp::Sub,
            AssignOp::MulAssign => BinOp::Mul,
            AssignOp::DivAssign => BinOp::Div,
            AssignOp::RemAssign => BinOp::Rem,
            AssignOp::BitXorAssign => BinOp::BitXor,
            AssignOp::BitAndAssign => BinOp::BitAnd,
            AssignOp::BitOrAssign => BinOp::BitOr,
            AssignOp::ShlAssign => BinOp::Shl,
            AssignOp::ShrAssign => BinOp::Shr,
        }
    }
}

#[cfg(target_pointer_width = "64")]
mod size_asserts {
    use rustc_data_structures::static_assert_size;

    use super::*;
    static_assert_size!(AggregateKind<'_>, 32);
    static_assert_size!(Operand<'_>, 24);
    static_assert_size!(Place<'_>, 16);
    static_assert_size!(PlaceElem<'_>, 24);
    static_assert_size!(Rvalue<'_>, 40);
    static_assert_size!(StatementKind<'_>, 16);
    static_assert_size!(TerminatorKind<'_>, 80);
}
