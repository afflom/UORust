//! The schedule's steps.

use thermo_core::schedule::{hold, is_sequence, phase_label, Step};

/// A held phase keeps its label and is no sequence.
#[test]
fn a_held_phase_keeps_its_label() {
    assert_eq!(phase_label(&hold("night", 16)), "night");
    assert!(!is_sequence(&Step::Hold(21)));
}

/// A label appears once, however many phases carry it.
#[test]
fn labels_are_kept_once_and_in_order() {
    let phases = [hold("night", 16), hold("day", 21), hold("night", 17)];
    assert_eq!(thermo_core::schedule::labels(&phases).len(), 2);
    assert_eq!(thermo_core::schedule::held_targets(&phases).len(), 2);
}
