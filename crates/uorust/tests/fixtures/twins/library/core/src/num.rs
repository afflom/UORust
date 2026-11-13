//! Two macro expansions in one function, each with a constant item of one
//! name, as `int_sqrt`'s `middle_stage!` and `last_stage!` in `u32_stages`:
//! rustc prints both items, and every use of either, under one path.

macro_rules! stage {
    ($ty:ty) => {{
        const HALF_BITS: u32 = <$ty>::BITS >> 1;
        const LOWER_HALF_1_BITS: $ty = (1 << HALF_BITS) - 1;
        LOWER_HALF_1_BITS
    }};
}

const fn u32_stages() -> u32 {
    let lower = stage!(u16);
    stage!(u32) + lower as u32
}
