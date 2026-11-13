//! The declarations a switch on an `Ordering` reads.

/// The result of a comparison, with the discriminants core declares.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(i8)]
pub enum Ordering {
    Less = -1,
    Equal = 0,
    Greater = 1,
}

/// Whether `o` is `Less`, by a call where it is of a function this fixture
/// leaves undefined, so that reaching the arm is residue.
pub fn less_reaches(o: Ordering) -> bool {
    match o {
        Ordering::Less => absent(),
        _ => false,
    }
}

/// `then`'s switch: an arm on `Equal` (0) and the otherwise arm, each a
/// call this fixture leaves undefined; `Less` takes the otherwise arm.
pub fn equal_arm(o: Ordering) -> bool {
    match o {
        Ordering::Equal => on_equal(),
        _ => on_other(),
    }
}

/// `reverse`'s switch: an arm on each variant, so the otherwise arm, a call
/// this fixture leaves undefined, is unreachable.
pub fn every_arm(o: Ordering) -> bool {
    match o {
        Ordering::Less => true,
        Ordering::Equal => false,
        Ordering::Greater => false,
    }
}
