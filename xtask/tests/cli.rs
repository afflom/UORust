//! The `xtask` binary as `just` runs it.

use std::process::Command;

fn xtask(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .expect("xtask runs")
}

/// CM-01: `xtask check-model` passes on this repository, whose committed
/// CONFORMANCE.md is the rendering of its register.
#[test]
fn check_model_passes_on_this_repository_cm_01() {
    let out = xtask(&["check-model"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("CONFORMANCE.md equals the model"));
}

/// An unknown task prints the task list and exits 2, so a typo never passes.
#[test]
fn an_unknown_task_prints_the_task_list_and_fails() {
    let out = xtask(&["no-such-task"]);
    assert_eq!(out.status.code(), Some(2));
    let usage = String::from_utf8_lossy(&out.stderr);
    for task in ["check-model", "audit-limits", "audit-deferral", "validate"] {
        assert!(usage.contains(task), "{usage}");
    }
}
