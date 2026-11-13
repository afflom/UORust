//! The certified translator's adapters, written by `uorust certify sync`
//! (`just certified`): for each certified root whose parameter and result
//! types convert, a function over references to the hand-written one's
//! arguments that runs the certified package (crates/certified) on them. A
//! hand-written function runs its certified realization through
//! [`certified!`], its first statement; a site whose root this commit does
//! not certify has a stub that answers `None`, so the hand-written body runs.
//! `--translator hand` runs every hand-written body, the differential's second
//! path. A certified root that overflows its target's width stops with a
//! panic, which the differential sees as a disagreement.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};

static HAND: AtomicBool = AtomicBool::new(false);

/// Run the hand-written translator in place of the certified one: the
/// differential's second path.
pub fn use_hand(hand: bool) {
    HAND.store(hand, Ordering::Relaxed);
}

/// Whether the hand-written translator runs.
pub(crate) fn hand() -> bool {
    HAND.load(Ordering::Relaxed)
}

/// `certified!(adapter, args...)`, a hand-written function's first
/// statement under `#[cfg(not(uorust_lift))]`: return what the adapter
/// answers, unless the hand-written translator runs. The lift's dump sets
/// `uorust_lift`, so the self register lifts the hand-written body alone.
macro_rules! certified {
    ($adapter:ident $(, $arg:expr)* $(,)?) => {
        if !$crate::certified::hand() {
            if let Some(v) = $crate::certified::$adapter($(&$arg),*) {
                return v;
            }
        }
    };
}
pub(crate) use certified;

/// Not certified at this commit: the hand-written body runs.
pub(crate) fn lift_equality_name<A0: ?Sized, R>(_: &A0) -> Option<R> {
    None
}

/// Not certified at this commit: the hand-written body runs.
pub(crate) fn lift_first_free<A0: ?Sized, A1: ?Sized, R>(_: &A0, _: &A1) -> Option<R> {
    None
}

/// Not certified at this commit: the hand-written body runs.
pub(crate) fn lift_mentions_word<A0: ?Sized, A1: ?Sized, R>(_: &A0, _: &A1) -> Option<R> {
    None
}

/// Not certified at this commit: the hand-written body runs.
pub(crate) fn lift_pointer_width<A0: ?Sized, R>(_: &A0) -> Option<R> {
    None
}

/// Not certified at this commit: the hand-written body runs.
pub(crate) fn lift_unicode_property<A0: ?Sized, R>(_: &A0) -> Option<R> {
    None
}

/// Not certified at this commit: the hand-written body runs.
pub(crate) fn mir_lower_address<A0: ?Sized, R>(_: &A0) -> Option<R> {
    None
}

/// Not certified at this commit: the hand-written body runs.
pub(crate) fn mir_lower_implication<A0: ?Sized, A1: ?Sized, R>(_: &A0, _: &A1) -> Option<R> {
    None
}

#[cfg(test)]
mod tests {
    // The switch the differential runs the hand-written translator through.
    #[test]
    fn use_hand_selects_the_hand_written_translator() {
        super::use_hand(true);
        assert!(super::hand());
        super::use_hand(false);
        assert!(!super::hand());
    }
}
