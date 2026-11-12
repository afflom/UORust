//! The schedule's steps.

use thermo_core::schedule::{hold, is_sequence, phase_label, Step};

/// A held phase keeps its label and is no sequence.
#[test]
fn a_held_phase_keeps_its_label() {
    assert_eq!(phase_label(&hold("night", 16)), "night");
    assert!(!is_sequence(&Step::Hold(21)));
}
