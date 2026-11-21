//! The `certify` commands on a workspace that is not the translator's: the
//! profile a workspace declares, the ratchet over the scope its manifest
//! states, and the agreement's refusal of what is not a cases export.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A κ: the SHA-256 digits a register requires.
fn k(digit: char) -> String {
    digit.to_string().repeat(64)
}

/// A text with each `<kN>` the κ of the digit N.
fn with_kappas(text: &str) -> String {
    let mut out = text.to_string();
    for digit in '1'..='5' {
        out = out.replace(&format!("<k{digit}>"), &k(digit));
    }
    out
}

fn uorust(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_uorust"))
        .args(args)
        .output()
        .expect("uorust runs")
}

/// A workspace of two crates that declares the first as its scope, the
/// register of its functions, and the manifest of its one certified root.
fn meter(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uorust-certify-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("meter-ws")).expect("the workspace");
    std::fs::write(
        dir.join("meter-ws/Cargo.toml"),
        "[workspace]\nmembers = [\"meter\", \"gauge\"]\n\n[workspace.metadata.uorust.certify]\nscope = [\"meter::\"]\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("base.toml"),
        with_kappas("spec = \"uorust/register/1\"\n\n\
         [[lifted]]\nkappa = \"<k1>\"\nfunction = \"meter::read\"\ndefinition = \"Behavior::read\"\n\n\
         [[residue]]\nkappa = \"<k2>\"\nfunction = \"meter::scale\"\nreason = \"form\"\ndetail = \"a loop\"\n\n\
         [[residue]]\nkappa = \"<k3>\"\nfunction = \"gauge::show\"\nreason = \"form\"\ndetail = \"a loop\"\n"),
    )
    .expect("write");
    std::fs::write(
        dir.join("manifest.toml"),
        with_kappas("schema = \"uorust/certified/1\"\nresult = \"r\"\ncompiler = \"0.4.0\"\n\n\
         [profile]\nscope = [\"meter::\"]\nentry = \"\"\npackage = \"meter_ws_certified_\"\n\
         effects = [\"allocation\", \"overflow\", \"recursion\"]\ntarget = \"rust-std\"\nroots = \"functions\"\n\n\
         [share]\ntranslator = 2\ncertified = 1\nreachable = 0\nreachable_certified = 0\n\n\
         [[root]]\nfunction = \"meter::read\"\nkappa = \"<k1>\"\ndefinition = \"read\"\n\
         package = \"meter_ws_certified_read\"\nsource = \"s\"\ncertification = \"kernel-checked\"\n"),
    )
    .expect("write");
    dir
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The ratchet measures the scope the head's manifest states: the base
/// against itself passes, a head with a new function that is not certified
/// is refused naming it with its cause, a function outside the scope is not
/// measured, and a head whose scope leaves out what the base measured is
/// refused before any count.
#[test]
fn the_ratchet_holds_a_workspaces_own_scope() {
    let dir = meter("ratchet");
    let (base, manifest) = (
        text(&dir.join("base.toml")),
        text(&dir.join("manifest.toml")),
    );
    let ratchet = |head: &str, more: &[&str]| {
        let mut args = vec![
            "certify",
            "ratchet",
            "--register",
            head,
            "--manifest",
            manifest.as_str(),
            "--base-register",
            base.as_str(),
            "--base-manifest",
            manifest.as_str(),
        ];
        args.extend(more);
        uorust(&args)
    };
    let held = ratchet(&base, &[]);
    assert!(
        held.status.success(),
        "{}",
        String::from_utf8_lossy(&held.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&held.stdout),
        "1 functions in scope uncertified, none more than in the base\n"
    );
    // The planted defect: one more function in scope that does not lift,
    // and one outside the scope, which is not measured.
    let grown = dir.join("head.toml");
    let planted = std::fs::read_to_string(dir.join("base.toml")).expect("the base")
        + "\n[[effect]]\nkappa = \"<k4>\"\nfunction = \"meter::sum\"\neffect = \"Fs effect: fs::read\"\n\n\
           [[residue]]\nkappa = \"<k5>\"\nfunction = \"gauge::dial\"\nreason = \"form\"\ndetail = \"a loop\"\n";
    std::fs::write(&grown, with_kappas(&planted)).expect("plant");
    let refused = ratchet(&text(&grown), &[]);
    assert!(!refused.status.success());
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(
        message.contains("2 uncertified functions in scope, 1 in the parent")
            && message.contains("meter::sum: effect: Fs effect: fs::read")
            && !message.contains("gauge::dial"),
        "{message}"
    );
    // A head that measures less than its base.
    let narrowed = ratchet(
        &base,
        &[
            "--scope",
            "meter::dial::",
            "--package",
            "meter_ws_certified_",
        ],
    );
    assert!(!narrowed.status.success());
    let message = String::from_utf8_lossy(&narrowed.stderr);
    assert!(message.contains("meter::: no longer in scope"), "{message}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A plan reads the workspace's declared scope, and the agreement refuses a
/// crate that is not of a cases export.
#[test]
fn a_plan_reads_the_workspaces_profile_and_agree_refuses_a_functions_crate() {
    let dir = meter("plan");
    let (base, workspace) = (text(&dir.join("base.toml")), text(&dir.join("meter-ws")));
    let none = text(&dir.join("none.toml"));
    let plan = |more: &[&str]| {
        let mut args = vec![
            "certify",
            "plan",
            "--register",
            base.as_str(),
            "--prior",
            none.as_str(),
            "--budget",
            "1",
        ];
        args.extend(more);
        uorust(&args)
    };
    // One lifted function in the workspace's scope: one shard, and the plan
    // says what the export will do with its roots: one to build, none
    // refused, and no carry decided (`carried` is null without the
    // projection and the targets). The translator's own scope holds none of
    // these functions, and plans one shard of nothing all the same.
    let planned = plan(&["--workspace", workspace.as_str()]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&planned.stdout),
        "{\"build\":1,\"carried\":null,\"diagnose\":[],\"refused\":0,\"shards\":[\"0-1\"]}\n"
    );
    // A cases plan reads the projection.
    let cases = plan(&["--workspace", workspace.as_str(), "--cases"]);
    assert!(!cases.status.success());
    assert!(String::from_utf8_lossy(&cases.stderr).contains("pass --projection"));
    // An effect outside the closed registry is refused.
    let effect = plan(&["--workspace", workspace.as_str(), "--effect", "io"]);
    assert!(!effect.status.success());
    assert!(String::from_utf8_lossy(&effect.stderr).contains("no effect of the closed registry"));
    let refused = uorust(&[
        "certify",
        "agree",
        "--cases",
        text(&dir).as_str(),
        "--workspace",
        workspace.as_str(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not the manifest of a cases export"));
    let _ = std::fs::remove_dir_all(dir);
}

/// GATE-10: an absent manifest, at the base or the head, is refused by name
/// rather than read as one that certifies nothing.
#[test]
fn ratchet_refuses_an_absent_manifest() {
    let dir = meter("absent");
    let (base, manifest) = (
        text(&dir.join("base.toml")),
        text(&dir.join("manifest.toml")),
    );
    let missing = text(&dir.join("no-such-manifest.toml"));
    for (head_manifest, base_manifest) in [
        (manifest.as_str(), missing.as_str()),
        (missing.as_str(), manifest.as_str()),
    ] {
        let out = uorust(&[
            "certify",
            "ratchet",
            "--register",
            &base,
            "--manifest",
            head_manifest,
            "--base-register",
            &base,
            "--base-manifest",
            base_manifest,
        ]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "an absent manifest must be refused");
        assert!(
            stderr.contains("no-such-manifest.toml does not exist"),
            "{stderr}"
        );
    }
    let held = uorust(&[
        "certify",
        "ratchet",
        "--register",
        &base,
        "--manifest",
        &manifest,
        "--base-register",
        &base,
        "--base-manifest",
        &manifest,
    ]);
    assert!(
        held.status.success(),
        "{}",
        String::from_utf8_lossy(&held.stderr)
    );
    let _ = std::fs::remove_dir_all(dir);
}
