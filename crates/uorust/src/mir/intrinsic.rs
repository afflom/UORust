//! Core's intrinsics, one table: where core bottoms out in a function it
//! declares `#[rustc_intrinsic]` without a body (`library/core/src/
//! intrinsics/mod.rs`), the call is the model this table names for it,
//! keyed by the intrinsic's own name and applied only to an item core
//! declares so (`CoreDecls::intrinsics`). Each model is the semantics the
//! intrinsic's documentation states, parameterized by the types its call
//! instantiates; none holds a constant of its own.
//!
//! An intrinsic core declares with a fallback body (`likely`, `unlikely`,
//! `ub_checks`, `is_val_statically_known`, `cold_path`, ..) is that body,
//! lifted like any other: its MIR is the build's own configuration (the
//! dump is built `-Cdebug-assertions=off`, so `ub_checks` is `false`) and
//! the fallback is, by core's contract, a valid implementation of it.
//!
//! An integer operation's model is a rule parameterized by (bits, signed),
//! see `mir::int`.

/// How the lift models an intrinsic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Model {
    /// `size_of::<T>() -> usize`: `T`'s size in the target data layout.
    Size,
    /// `align_of::<T>() -> usize`: `T`'s ABI alignment in the target data
    /// layout.
    Align,
    /// `size_of_val::<T>(*const T)`: the size of a sized `T`, which no
    /// value of it changes.
    SizeOfVal,
    /// The value of its one argument: `black_box` is "an identity function"
    /// whose only effect is on the optimizer.
    Identity,
    /// The unit: an effect on no value the lift holds (`forget` runs no
    /// destructor and returns nothing).
    Unit,
    /// `assert_inhabited::<T>()`: the unit when `T` has a value, as every
    /// type the lift holds does; it panics otherwise.
    Inhabited,
    /// `assert_zero_valid::<T>()`: the unit when the all-zero bit pattern
    /// is a value of `T`; it panics otherwise.
    ZeroValid,
    /// `assert_mem_uninitialized_valid::<T>()`: the unit when the pattern
    /// `mem::uninitialized` leaves (every byte `0x01`) is a value of `T`;
    /// it panics otherwise.
    UninitValid,
    /// `transmute::<Src, Dst>(x)`: the bits of `x` read as `Dst`, which is
    /// `x` itself where the lift holds `Src` and `Dst` alike.
    Transmute,
    /// `three_way_compare::<T>(a, b) -> Ordering`: `Less`, `Equal` or
    /// `Greater` by `T`'s order.
    Compare,
    /// An integer operation, parameterized by (bits, signed): its rule in
    /// `mir::int`, with core's precondition as an obligation.
    Int,
}

/// Each modeled intrinsic by the name core declares it with.
pub(crate) const TABLE: &[(&str, Model)] = &[
    ("size_of", Model::Size),
    ("align_of", Model::Align),
    ("size_of_val", Model::SizeOfVal),
    ("black_box", Model::Identity),
    ("forget", Model::Unit),
    ("assert_inhabited", Model::Inhabited),
    ("assert_zero_valid", Model::ZeroValid),
    ("assert_mem_uninitialized_valid", Model::UninitValid),
    ("transmute", Model::Transmute),
    ("three_way_compare", Model::Compare),
    ("wrapping_add", Model::Int),
    ("wrapping_sub", Model::Int),
    ("wrapping_mul", Model::Int),
    ("add_with_overflow", Model::Int),
    ("sub_with_overflow", Model::Int),
    ("mul_with_overflow", Model::Int),
    ("saturating_add", Model::Int),
    ("saturating_sub", Model::Int),
    ("rotate_left", Model::Int),
    ("rotate_right", Model::Int),
    ("ctpop", Model::Int),
    ("ctlz", Model::Int),
    ("ctlz_nonzero", Model::Int),
    ("cttz", Model::Int),
    ("cttz_nonzero", Model::Int),
    ("bswap", Model::Int),
    ("bitreverse", Model::Int),
    ("disjoint_bitor", Model::Int),
    ("unchecked_add", Model::Int),
    ("unchecked_sub", Model::Int),
    ("unchecked_mul", Model::Int),
    ("unchecked_div", Model::Int),
    ("unchecked_rem", Model::Int),
    ("unchecked_shl", Model::Int),
    ("unchecked_shr", Model::Int),
    ("exact_div", Model::Int),
];
