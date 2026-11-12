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

/// A fresh `cargo new` package, with nothing to lift, projects to modules that
/// each declare something: LexLean refuses an empty semantic module (LLT4001),
/// so the Behavior module is left out rather than written empty.
#[test]
fn the_cli_projects_a_fresh_package_without_an_empty_module_ue_01() {
    let base = std::env::temp_dir().join(format!("uorust-fresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let ws = base.join("hello");
    std::fs::create_dir_all(ws.join("src")).expect("mkdir");
    std::fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    )
    .expect("manifest");
    std::fs::write(
        ws.join("src/main.rs"),
        "fn main() {\n    println!(\"Hello, world!\");\n}\n",
    )
    .expect("main");
    let out = base.join("hello-prism");
    let (ws_arg, out_arg) = (
        ws.to_string_lossy().to_string(),
        out.to_string_lossy().to_string(),
    );
    let common = [
        ws_arg.as_str(),
        "--out",
        out_arg.as_str(),
        "--last-modified",
        "2026-11-11T00:00:00Z",
    ];
    for verb in ["project", "check"] {
        let mut args = vec![verb];
        args.extend(common);
        let run = uorust(&args);
        assert!(
            run.status.success(),
            "{verb}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
    let mut modules = Vec::new();
    let mut stack = vec![out.join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.to_string_lossy().ends_with(".lex.tex") {
                modules.push(path);
            }
        }
    }
    assert!(!modules.is_empty());
    for path in modules {
        let body = std::fs::read_to_string(&path).expect("module");
        assert!(
            !body.contains("\\begin{semanticmodule}\n\\end{semanticmodule}"),
            "{} declares nothing",
            path.display()
        );
    }
    let _ = std::fs::remove_dir_all(base);
}
