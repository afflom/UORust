//! The `uorust` binary end to end: `project`, `check` and `routes`.

use std::path::PathBuf;
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn uorust(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_uorust"))
        .args(args)
        .current_dir(manifest_dir())
        .output()
        .expect("uorust runs")
}

/// `project` writes the committed fixture projection, `check` accepts it and
/// refuses a stale one.
#[test]
fn the_cli_projects_and_checks_the_fixture_ue_01() {
    let out = std::env::temp_dir().join(format!("uorust-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let out_arg = out.to_string_lossy().to_string();
    let common = [
        "tests/fixtures/thermostat",
        "--name",
        "thermostat",
        "--last-modified",
        "2026-11-11T00:00:00Z",
    ];
    let mut args = vec!["project"];
    args.extend(common);
    args.extend(["--out", out_arg.as_str()]);
    assert!(uorust(&args).status.success());
    let mut args = vec!["check"];
    args.extend(common);
    args.extend(["--out", "tests/fixtures/thermostat-prism"]);
    let checked = uorust(&args);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::write(out.join("frontier.toml"), "planted\n").expect("plant");
    let mut args = vec!["check"];
    args.extend(common);
    args.extend(["--out", out_arg.as_str()]);
    let stale = uorust(&args);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("frontier.toml"));
    let _ = std::fs::remove_dir_all(out);
}

/// `routes` prints the route table, one route per line.
#[test]
fn the_cli_lists_the_routes_ue_05() {
    let listed = uorust(&["routes"]);
    assert!(listed.status.success());
    let text = String::from_utf8_lossy(&listed.stdout).to_string();
    assert_eq!(text.lines().count(), uorust::ROUTES.len());
    assert!(text.starts_with("UR-R01\tcrate-bin-target\t"));
}
