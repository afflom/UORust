//! The coverage table of the MIR path: for every kind of every enum of
//! [`super::spec`], how [`super::lower`] translates it, or the residue it
//! becomes. Each function matches its enum exhaustively, so a toolchain whose
//! MIR adds a kind does not compile until the kind has a row here; the
//! honesty report prints the table.

use super::spec::{
    MirAggregateKind, MirAssertKind, MirBinOp, MirBorrowKind, MirCastKind,
    MirNonDivergingIntrinsic, MirOperand, MirProjectionElem, MirRuntimeChecks, MirRvalue,
    MirStatementKind, MirTerminatorKind, MirUnOp, TOOLCHAIN,
};

/// How a kind is translated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coverage {
    /// Lowered, as this says.
    Lowered(&'static str),
    /// Every occurrence is a residue row naming the kind, for this reason.
    Residue(&'static str),
}

const NO_EFFECT: Coverage = Coverage::Lowered("no effect on values");
const BORROWS: &str = "a mutable or raw borrow, which a value model does not hold";
const FLOATS: &str = "floating point, which the lift's types do not model";

fn statement(k: MirStatementKind) -> Coverage {
    use MirStatementKind as S;
    match k {
        S::Assign => Coverage::Lowered(
            "the local holds the rvalue's value; a write through a projection is residue",
        ),
        S::FakeRead
        | S::StorageLive
        | S::StorageDead
        | S::PlaceMention
        | S::AscribeUserType
        | S::Coverage
        | S::ConstEvalCounter
        | S::Nop
        | S::BackwardIncompatibleDropHint => NO_EFFECT,
        S::Intrinsic => {
            Coverage::Lowered("assume has no effect on values; copy_nonoverlapping is residue")
        }
        S::SetDiscriminant => Coverage::Residue("a write of a discriminant into a place"),
    }
}

fn intrinsic(k: MirNonDivergingIntrinsic) -> Coverage {
    match k {
        MirNonDivergingIntrinsic::Assume => NO_EFFECT,
        MirNonDivergingIntrinsic::CopyNonOverlapping => {
            Coverage::Residue("a copy between raw pointers")
        }
    }
}

fn terminator(k: MirTerminatorKind) -> Coverage {
    use MirTerminatorKind as T;
    match k {
        T::Goto | T::FalseEdge | T::FalseUnwind => {
            Coverage::Lowered("control goes on; a back edge to a loop header calls the loop's helper")
        }
        T::SwitchInt => Coverage::Lowered("if on a bool, an if per value on a natural, a match on a discriminant"),
        T::Return => Coverage::Lowered("the value of the return place"),
        T::Unreachable => Coverage::Lowered("a theorem that the path is unreachable"),
        T::Drop => Coverage::Lowered("control goes on: a value has no destructor in the model"),
        T::Call => Coverage::Lowered("a lifted function; a std operation as std's own MIR at the call's types, or its model where std bottoms out below values (mir/std.toml); a call that does not return is a theorem that the path is unreachable"),
        T::Assert => Coverage::Lowered("a theorem that the condition holds on the path"),
        T::UnwindResume | T::UnwindTerminate => {
            Coverage::Lowered("never reached: unwinding paths are excluded by the theorems of the panics")
        }
        T::TailCall => Coverage::Residue("an explicit tail call"),
        T::Yield | T::CoroutineDrop => Coverage::Residue("a coroutine"),
        T::InlineAsm => Coverage::Residue("inline assembly"),
    }
}

fn rvalue(k: MirRvalue) -> Coverage {
    use MirRvalue as R;
    match k {
        R::Use | R::CopyForDeref => Coverage::Lowered("the operand's value"),
        R::Ref => {
            Coverage::Lowered("a shared borrow is the value it borrows; a mutable borrow of a local is passed to a call whose backward function gives the local its new value")
        }
        R::Cast => {
            Coverage::Lowered("a widening integer cast keeps the value; any other is residue")
        }
        R::BinaryOp | R::UnaryOp => Coverage::Lowered("by operator, below"),
        R::Discriminant => Coverage::Lowered("the switch on it matches the value"),
        R::Aggregate => Coverage::Lowered("by aggregate kind, below"),
        R::Repeat => Coverage::Residue("an array of repeated values"),
        R::ThreadLocalRef => Coverage::Residue("a thread-local"),
        R::RawPtr | R::Reborrow => Coverage::Residue(BORROWS),
        R::WrapUnsafeBinder => Coverage::Residue("an unsafe binder"),
    }
}

fn operand(k: MirOperand) -> Coverage {
    match k {
        MirOperand::Copy | MirOperand::Move => Coverage::Lowered("the place's value"),
        MirOperand::Constant => Coverage::Lowered("a literal, or the constant's own body (a promoted constant or a constant item) lowered like any body"),
        MirOperand::RuntimeChecks => {
            Coverage::Lowered("overflow checks are on; the other checks are residue")
        }
    }
}

fn runtime_checks(k: MirRuntimeChecks) -> Coverage {
    match k {
        MirRuntimeChecks::OverflowChecks => {
            Coverage::Lowered("true: the dump's profile checks overflow")
        }
        MirRuntimeChecks::UbChecks | MirRuntimeChecks::ContractChecks => {
            Coverage::Residue("a check whose value is a build setting")
        }
    }
}

fn projection(k: MirProjectionElem) -> Coverage {
    use MirProjectionElem as P;
    match k {
        P::Deref => Coverage::Lowered("a shared reference or a box is its value"),
        P::Field => Coverage::Lowered("a struct's or a tuple's field, or a variant's binder"),
        P::Index => Coverage::Lowered("the item, its bound a theorem of the assert before it"),
        P::Downcast => Coverage::Lowered("the binders of the match arm on the variant"),
        P::ConstantIndex | P::Subslice => Coverage::Residue("a slice pattern"),
        P::OpaqueCast | P::UnwrapUnsafeBinder => Coverage::Residue("a cast of a place's type"),
    }
}

fn borrow(k: MirBorrowKind) -> Coverage {
    match k {
        MirBorrowKind::Shared | MirBorrowKind::Fake => Coverage::Lowered("the value borrowed"),
        MirBorrowKind::Mut => Coverage::Lowered("a local passed to a call with a backward function, which returns the local's new value"),
    }
}

fn cast(k: MirCastKind) -> Coverage {
    use MirCastKind as C;
    match k {
        C::IntToInt => Coverage::Lowered("widening keeps the value; narrowing is residue"),
        C::FloatToInt | C::FloatToFloat | C::IntToFloat => Coverage::Residue(FLOATS),
        C::PointerExposeProvenance
        | C::PointerWithExposedProvenance
        | C::PointerCoercion
        | C::PtrToPtr
        | C::FnPtrToPtr
        | C::Transmute
        | C::Subtype => Coverage::Residue("a pointer or representation cast"),
    }
}

fn aggregate(k: MirAggregateKind) -> Coverage {
    use MirAggregateKind as A;
    match k {
        A::Tuple => Coverage::Lowered("the lift's tuple"),
        A::Array => Coverage::Lowered("the list of its items"),
        A::Adt => Coverage::Lowered("a struct record, or a variant's constructor"),
        A::Closure | A::Coroutine | A::CoroutineClosure => {
            Coverage::Residue("a closure or coroutine value")
        }
        A::RawPtr => Coverage::Residue(BORROWS),
    }
}

fn binop(k: MirBinOp) -> Coverage {
    use MirBinOp as B;
    match k {
        B::AddWithOverflow | B::SubWithOverflow | B::MulWithOverflow => Coverage::Lowered(
            "the exact value, and its overflow at the type's width on every declared target",
        ),
        B::Add | B::AddUnchecked | B::Sub | B::SubUnchecked | B::Mul | B::MulUnchecked => {
            Coverage::Lowered("the exact value, with a theorem that it stays in range")
        }
        B::Div | B::Rem => {
            Coverage::Lowered("on naturals, the divisor's zero excluded by its assert")
        }
        B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge => {
            Coverage::Lowered("on naturals, bools and strings")
        }
        B::BitAnd | B::BitOr | B::BitXor => Coverage::Lowered("on bools; on integers residue"),
        B::Shl | B::ShlUnchecked | B::Shr | B::ShrUnchecked => Coverage::Residue("a shift"),
        B::Cmp => Coverage::Residue("a three-way comparison"),
        B::Offset => Coverage::Residue("pointer arithmetic"),
    }
}

fn unop(k: MirUnOp) -> Coverage {
    match k {
        MirUnOp::Not => Coverage::Lowered("on bools; on integers residue"),
        MirUnOp::Neg => Coverage::Residue("negation, of a signed integer the lift does not model"),
        MirUnOp::PtrMetadata => {
            Coverage::Lowered("a slice's length, at most isize::MAX at the narrowest width")
        }
    }
}

fn assert(k: MirAssertKind) -> Coverage {
    use MirAssertKind as A;
    match k {
        A::BoundsCheck
        | A::Overflow
        | A::OverflowNeg
        | A::DivisionByZero
        | A::RemainderByZero
        | A::ResumedAfterReturn
        | A::ResumedAfterPanic
        | A::ResumedAfterDrop
        | A::MisalignedPointerDereference
        | A::NullPointerDereference
        | A::InvalidEnumConstruction => Coverage::Lowered("a theorem that the check passes"),
    }
}

/// Every kind of every MIR enum with its coverage, as `(path, coverage)`.
pub(crate) fn table() -> Vec<(&'static str, Coverage)> {
    let mut out = Vec::new();
    out.extend(
        MirStatementKind::ALL
            .iter()
            .map(|k| (k.path(), statement(*k))),
    );
    out.extend(
        MirNonDivergingIntrinsic::ALL
            .iter()
            .map(|k| (k.path(), intrinsic(*k))),
    );
    out.extend(
        MirTerminatorKind::ALL
            .iter()
            .map(|k| (k.path(), terminator(*k))),
    );
    out.extend(MirRvalue::ALL.iter().map(|k| (k.path(), rvalue(*k))));
    out.extend(MirOperand::ALL.iter().map(|k| (k.path(), operand(*k))));
    out.extend(
        MirRuntimeChecks::ALL
            .iter()
            .map(|k| (k.path(), runtime_checks(*k))),
    );
    out.extend(
        MirProjectionElem::ALL
            .iter()
            .map(|k| (k.path(), projection(*k))),
    );
    out.extend(MirBorrowKind::ALL.iter().map(|k| (k.path(), borrow(*k))));
    out.extend(MirCastKind::ALL.iter().map(|k| (k.path(), cast(*k))));
    out.extend(
        MirAggregateKind::ALL
            .iter()
            .map(|k| (k.path(), aggregate(*k))),
    );
    out.extend(MirBinOp::ALL.iter().map(|k| (k.path(), binop(*k))));
    out.extend(MirUnOp::ALL.iter().map(|k| (k.path(), unop(*k))));
    out.extend(MirAssertKind::ALL.iter().map(|k| (k.path(), assert(*k))));
    out
}

/// The table as the honesty report's comment lines.
pub fn report() -> String {
    let table = table();
    let lowered = table
        .iter()
        .filter(|(_, c)| matches!(c, Coverage::Lowered(_)))
        .count();
    let mut out = format!(
        "# MIR kinds at toolchain {TOOLCHAIN}: {lowered} of {} lowered; every other kind is a\n\
         # residue row that names it.\n",
        table.len()
    );
    for (path, c) in &table {
        match c {
            Coverage::Lowered(how) => out.push_str(&format!("#   {path}: lowered: {how}\n")),
            Coverage::Residue(why) => out.push_str(&format!("#   {path}: residue: {why}\n")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_the_pinned_mir_has_a_row() {
        let table = table();
        let total: usize = [
            MirStatementKind::ALL.len(),
            MirNonDivergingIntrinsic::ALL.len(),
            MirTerminatorKind::ALL.len(),
            MirRvalue::ALL.len(),
            MirOperand::ALL.len(),
            MirRuntimeChecks::ALL.len(),
            MirProjectionElem::ALL.len(),
            MirBorrowKind::ALL.len(),
            MirCastKind::ALL.len(),
            MirAggregateKind::ALL.len(),
            MirBinOp::ALL.len(),
            MirUnOp::ALL.len(),
            MirAssertKind::ALL.len(),
        ]
        .iter()
        .sum();
        assert_eq!(table.len(), total);
        assert!(report().contains("Rvalue::Ref: lowered"));
        assert!(report().contains("TerminatorKind::InlineAsm: residue"));
    }
}
