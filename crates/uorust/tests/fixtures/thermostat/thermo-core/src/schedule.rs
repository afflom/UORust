//! A heating schedule: steps that hold a target, choose between steps, or
//! run labelled phases in order.

/// One step of a schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Hold a target.
    Hold(u32),
    /// The first step when the room is occupied, the second otherwise.
    Either(Box<Step>, Box<Step>),
    /// Phases run in order.
    Sequence(Vec<Phase>),
}

/// A labelled step of a sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phase {
    /// What the phase is called.
    pub label: String,
    /// What it does.
    pub step: Step,
}

/// The target an occupied room is held at first, if the step holds one
/// before any sequence.
pub fn occupied_target(step: &Step) -> Option<u32> {
    match step {
        Step::Hold(target) => Some(*target),
        Step::Either(occupied, _) => occupied_target(occupied),
        Step::Sequence(_) => None,
    }
}

/// Whether a step is a sequence of phases.
pub fn is_sequence(step: &Step) -> bool {
    matches!(step, Step::Sequence(_))
}

/// A phase that holds a target.
pub fn hold(label: &str, target: u32) -> Phase {
    Phase {
        label: label.to_string(),
        step: Step::Hold(target),
    }
}

/// The label of a phase.
pub fn phase_label(phase: &Phase) -> String {
    phase.label.clone()
}

/// A menu of named choices, each a menu again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Menu {
    /// A final choice: the target it sets.
    Choose(u32),
    /// Named sub-menus.
    Submenu(Vec<(String, Menu)>),
}

/// Whether a menu is a final choice.
pub fn is_final(menu: &Menu) -> bool {
    matches!(menu, Menu::Choose(_))
}
