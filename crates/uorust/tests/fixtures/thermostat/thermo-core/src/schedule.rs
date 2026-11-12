//! A heating schedule: steps that hold a target, choose between steps, or
//! run labelled phases in order.

use std::collections::{BTreeMap, BTreeSet};

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

/// The label of a phase, or the fallback when it has none. The borrow's
/// lifetime says nothing about the value.
pub fn label_or<'a>(phase: &'a Phase, fallback: &'a str) -> &'a str {
    if phase.label.is_empty() {
        fallback
    } else {
        &phase.label
    }
}

/// The labels of the phases, each once, in order.
pub fn labels(phases: &[Phase]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for phase in phases {
        out.insert(phase.label.clone());
    }
    out
}

/// Each label's held target; a later phase of the same label replaces an
/// earlier one.
pub fn held_targets(phases: &[Phase]) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for phase in phases {
        out.insert(phase.label.clone(), occupied_target(&phase.step).unwrap_or(0));
    }
    out
}

/// The unit targets are held in.
pub const HELD_UNIT: &str = "C";

/// A phase as text: its label, its target and whether it is a sequence.
pub fn describe(phase: &Phase) -> String {
    let target = occupied_target(&phase.step).unwrap_or(0);
    let sequence = is_sequence(&phase.step);
    format!("{}: {target}{HELD_UNIT} (sequence: {sequence})", phase.label)
}

/// A phase is valid when it has a label; nothing else is returned.
pub fn validate(phase: &Phase) -> Result<(), String> {
    if phase.label.is_empty() {
        return Err("a phase has no label".to_string());
    }
    Ok(())
}

/// The target a step holds first: none for a hold at zero, none when the
/// other choice of an `Either` is a sequence, else the occupied target.
pub fn first_hold(step: &Step) -> Option<u32> {
    match step {
        Step::Hold(0) => None,
        Step::Either(_, other) if is_sequence(other) => None,
        other => occupied_target(other),
    }
}

/// The label of a schedule of exactly one phase that holds a target.
pub fn single_label(phases: &[Phase]) -> Option<String> {
    match phases {
        [Phase {
            label,
            step: Step::Hold(_),
        }] => Some(label.clone()),
        _ => None,
    }
}

/// The mode two readings choose: heat when only cold, cool when only hot.
pub fn mode_for(cold: bool, hot: bool) -> crate::Mode {
    match (cold, hot) {
        (true, false) => crate::Mode::Heating,
        (false, true) => crate::Mode::Cooling,
        _ => crate::Mode::Idle,
    }
}

/// Whether both ends of a band lie in the comfort range: a let pattern
/// takes the band apart and a closure tests each end.
pub fn comfortable(band: crate::band::Band) -> bool {
    let crate::band::Band { low, high } = band;
    let comfy = |t: u32| t >= 18 && t <= 24;
    comfy(low) && comfy(high)
}

/// A choice between two steps, each boxed.
pub fn either(occupied: Step, otherwise: Step) -> Step {
    Step::Either(Box::new(occupied), Box::new(otherwise))
}

/// Whether a step turns the heating off anywhere, holding a target of
/// zero, through every phase of its sequences: the step, its phases and
/// each phase walk one another.
pub fn holds_off(step: &Step) -> bool {
    match step {
        Step::Hold(target) => *target == 0,
        Step::Either(occupied, otherwise) => holds_off(occupied) || holds_off(otherwise),
        Step::Sequence(phases) => phases_off(phases),
    }
}

/// Whether any of the phases turns the heating off.
pub fn phases_off(phases: &[Phase]) -> bool {
    match phases {
        [] => false,
        [first, rest @ ..] => phase_off(first) || phases_off(rest),
    }
}

/// Whether a phase's step turns the heating off.
pub fn phase_off(phase: &Phase) -> bool {
    holds_off(&phase.step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_patterns_choose_the_arm() {
        assert_eq!(first_hold(&Step::Hold(0)), None);
        assert_eq!(first_hold(&Step::Hold(19)), Some(19));
        assert_eq!(single_label(&[hold("day", 21)]), Some("day".to_string()));
        assert_eq!(single_label(&[]), None);
        assert_eq!(mode_for(true, false), crate::Mode::Heating);
        assert_eq!(mode_for(false, true), crate::Mode::Cooling);
        assert_eq!(mode_for(true, true), crate::Mode::Idle);
        assert!(comfortable(crate::band::Band { low: 18, high: 22 }));
        assert!(!comfortable(crate::band::Band { low: 16, high: 22 }));
        assert!(!is_sequence(&either(Step::Hold(16), Step::Hold(21))));
        let day = Step::Sequence(vec![hold("morning", 20), hold("evening", 18)]);
        assert!(!holds_off(&either(Step::Hold(21), day.clone())));
        let night = Step::Sequence(vec![hold("night", 0)]);
        assert!(holds_off(&either(day, Step::Sequence(vec![hold("late", 17), Phase { label: "out".to_string(), step: night }]))));
    }
}
