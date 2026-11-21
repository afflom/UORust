//! The `uorust` binary end to end: `project`, `check`, `routes`, `mir`,
//! `prove`, `core`, `std`, `embed`, `merge`, `crate` and `certify`
//! `production`, `sync` and `agree`.

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

/// `mir` dumps the fixture's MIR, and `prove` on its committed verdict
/// register, which decides every translation, runs no verifier and leaves the
/// register as it was.
#[test]
fn the_cli_dumps_mir_and_proves_on_the_verdicts_ue_17() {
    let tmp = std::env::temp_dir().join(format!("uorust-cli-mir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("scratch");
    let dumps = tmp.join("mir");
    let dumps_arg = dumps.to_string_lossy().to_string();
    let dumped = uorust(&[
        "mir",
        "tests/fixtures/mir-path",
        "--out",
        dumps_arg.as_str(),
    ]);
    assert!(
        dumped.status.success(),
        "{}",
        String::from_utf8_lossy(&dumped.stderr)
    );
    let bodies: Vec<String> = std::fs::read_dir(dumps.join("lib-walk"))
        .expect("the library's dumps")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    assert!(
        bodies.iter().any(|b| b.starts_with("walk.capped_sum.")),
        "{bodies:?}"
    );
    let committed =
        std::fs::read_to_string(manifest_dir().join("tests/fixtures/mir-path/lift-verdicts.toml"))
            .expect("the committed verdicts");
    let verdicts = tmp.join("lift-verdicts.toml");
    std::fs::write(&verdicts, &committed).expect("verdicts");
    let (out, scratch) = (tmp.join("out"), tmp.join("prove"));
    let proved = uorust(&[
        "prove",
        "tests/fixtures/mir-path",
        "--name",
        "walk",
        "--last-modified",
        "2026-11-11T00:00:00Z",
        "--out",
        &out.to_string_lossy(),
        "--scratch",
        &scratch.to_string_lossy(),
        "--mir",
        dumps_arg.as_str(),
        "--verdicts",
        &verdicts.to_string_lossy(),
    ]);
    assert!(
        proved.status.success(),
        "{}",
        String::from_utf8_lossy(&proved.stderr)
    );
    assert!(String::from_utf8_lossy(&proved.stdout).contains("in 0 verifier runs"));
    assert_eq!(
        std::fs::read_to_string(&verdicts).expect("verdicts"),
        committed
    );
    let _ = std::fs::remove_dir_all(tmp);
}

/// `certify agree` states each pair's κ when the certified and the
/// hand-written translator wrote the same bytes, and refuses a pair one byte
/// apart (UE-18).
#[test]
fn the_cli_compares_the_two_translators_by_kappa_ue_18() {
    let dir = std::env::temp_dir().join(format!("uorust-agree-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for side in ["core-certified", "core-hand"] {
        std::fs::create_dir_all(dir.join(side)).expect("a side");
        std::fs::write(dir.join(side).join("frontier.toml"), "same\n").expect("write");
    }
    let dir_arg = dir.to_string_lossy().to_string();
    let agreed = uorust(&["certify", "agree", "--dir", dir_arg.as_str()]);
    assert!(
        agreed.status.success(),
        "{}",
        String::from_utf8_lossy(&agreed.stderr)
    );
    assert!(String::from_utf8_lossy(&agreed.stdout).starts_with("core "));
    std::fs::write(dir.join("core-hand").join("frontier.toml"), "samf\n").expect("plant");
    let refused = uorust(&["certify", "agree", "--dir", dir_arg.as_str()]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("disagree"));
    let _ = std::fs::remove_dir_all(dir);
}

/// `core` translates a library's module shards into a register; `std`
/// refuses a library that holds no sources of std, since its declarations
/// are read from them; `merge` takes a sharded run's
/// module files into a register and refuses one that turns a translated body
/// to residue; `crate` refuses a spec that is not `<name>@<version>` before
/// it fetches anything.
#[test]
fn the_cli_writes_and_merges_library_registers() {
    let tmp = std::env::temp_dir().join(format!("uorust-registers-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = |name: &str| tmp.join(name).to_string_lossy().to_string();
    let (core_reg, std_reg, merged, planted) =
        (dir("core"), dir("std"), dir("merged"), dir("planted"));
    let run = |args: &[&str]| {
        let out = uorust(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let fixture = [
        "--mir",
        "tests/fixtures/core/mir",
        "--library",
        "tests/fixtures/core/library",
    ];
    let mut args = vec!["core"];
    args.extend(fixture);
    args.extend(["--register", core_reg.as_str()]);
    assert!(run(&args).starts_with("core: "));
    let core_bool = std::fs::read_to_string(tmp.join("core/bool.toml")).expect("core's bool");
    assert!(core_bool.contains("status = \"translated\""), "{core_bool}");
    // The fixture's library holds core's sources only.
    let mut args = vec!["std"];
    args.extend(fixture);
    args.extend(["--register", std_reg.as_str()]);
    let refused = uorust(&args);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("library/std/src"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        !tmp.join("std").exists(),
        "a refused census writes no register"
    );

    let merge = |shards: &str| {
        uorust(&[
            "merge",
            "--crate",
            "core",
            "--shards",
            shards,
            "--register",
            merged.as_str(),
        ])
    };
    let out = merge(core_reg.as_str());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("core: merged 1 modules"));
    std::fs::create_dir_all(&planted).expect("planted shards");
    std::fs::write(
        tmp.join("planted/bool.toml"),
        core_bool.replace(
            "status = \"translated\"",
            "status = \"residue\"\ncause = \"planted\"",
        ),
    )
    .expect("plant");
    let refused = merge(planted.as_str());
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("is now residue"));

    let refused = uorust(&["crate", "serde"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("is not <name>@<version>"));
    let _ = std::fs::remove_dir_all(tmp);
}

/// `certify production` refuses a targets file with no `rust-std`
/// descriptor before it writes the production project or runs `lexlean`;
/// `certify sync` refuses a kept bundle with no `result.json` before it
/// writes the certified crate or its adapters.
#[test]
fn the_cli_plans_an_export_and_refuses_to_diagnose_without_a_target() {
    let tmp = std::env::temp_dir().join(format!("uorust-certify-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("scratch");
    let missing = tmp.join("no-prior-manifest.toml");
    let planned = uorust(&[
        "certify",
        "plan",
        "--register",
        "tests/fixtures/thermostat-prism/frontier.toml",
        "--prior",
        &missing.to_string_lossy(),
        "--budget",
        "3600",
    ]);
    assert!(planned.status.success(), "{planned:?}");
    let plan: serde_json::Value =
        serde_json::from_slice(&planned.stdout).expect("the plan is JSON");
    assert!(
        plan["shards"].as_array().is_some_and(|s| !s.is_empty()),
        "{plan}"
    );
    assert_eq!(plan["diagnose"], serde_json::json!([]));
    let targets = tmp.join("targets.json");
    std::fs::write(&targets, r#"{"targets":[]}"#).expect("targets");
    let out = tmp.join("diagnose");
    let refused = uorust(&[
        "certify",
        "diagnose",
        "--projection",
        "tests/fixtures/thermostat-prism",
        "--register",
        "tests/fixtures/thermostat-prism/frontier.toml",
        "--targets",
        &targets.to_string_lossy(),
        "--root",
        "any",
        "--out",
        &out.to_string_lossy(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("no rust-std descriptor"));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn the_cli_refuses_a_bundle_it_cannot_export_or_take_in() {
    let tmp = std::env::temp_dir().join(format!("uorust-certify-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("scratch");
    let path = |name: &str| tmp.join(name).to_string_lossy().to_string();
    let targets = tmp.join("targets.json");
    std::fs::write(&targets, r#"{"targets":[{"id":"other","digest":"00"}]}"#).expect("targets");
    let (out, kept, dir, adapters) = (
        path("production"),
        path("kept"),
        path("certified"),
        path("certified.rs"),
    );
    let refused = uorust(&[
        "certify",
        "production",
        "--projection",
        "tests/fixtures/thermostat-prism",
        "--register",
        "tests/fixtures/thermostat-prism/frontier.toml",
        "--targets",
        &targets.to_string_lossy(),
        "--out",
        out.as_str(),
        "--kept",
        kept.as_str(),
        // No export before this one: a manifest that does not exist reads
        // as empty.
        "--prior",
        path("no-prior-manifest.toml").as_str(),
    ]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("no rust-std descriptor"),
        "{refused:?}"
    );
    assert!(!tmp.join("production").exists());

    std::fs::create_dir_all(&kept).expect("kept");
    let refused = uorust(&[
        "certify",
        "sync",
        "--kept",
        kept.as_str(),
        "--register",
        "tests/fixtures/thermostat-prism/frontier.toml",
        "--mir",
        path("mir").as_str(),
        "--workspace",
        "tests/fixtures/thermostat",
        "--dir",
        dir.as_str(),
        "--adapters",
        adapters.as_str(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("result.json"));
    assert!(!tmp.join("certified").exists() && !tmp.join("certified.rs").exists());
    let _ = std::fs::remove_dir_all(tmp);
}

/// `embed` refuses dumps that do not name the target they were built for.
#[test]
fn the_cli_refuses_an_embedding_without_a_target_ue_19() {
    let out = std::env::temp_dir().join(format!("uorust-cli-embed-{}", std::process::id()));
    let out_arg = out.to_string_lossy().to_string();
    let run = uorust(&[
        "embed",
        "--crate",
        "core",
        "--mir",
        "tests/fixtures/core/mir",
        "--library",
        "tests/fixtures/core/library",
        "--out",
        out_arg.as_str(),
    ]);
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("pointer_width")
            || String::from_utf8_lossy(&run.stderr).contains("target.toml")
    );
    let _ = std::fs::remove_dir_all(&out);
}

/// `bridge` refuses a project `embed --bridge` did not write, and `carry`
/// keeps a row's bridge stamps on a census taken again only while the row
/// is the one they were measured on (UE-19).
#[test]
fn the_bridge_refuses_an_unwritten_project_and_carries_only_its_own_rows_ue_19() {
    let tmp = std::env::temp_dir().join(format!("uorust-cli-bridge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (project, from, to) = (tmp.join("project"), tmp.join("from"), tmp.join("to"));
    for d in [&project, &from, &to] {
        std::fs::create_dir_all(d).expect("dir");
    }
    let refused = uorust(&["bridge", "--project", project.to_str().expect("utf-8")]);
    assert!(!refused.status.success(), "a project without bridge.json");
    assert!(uorust::bridge::verify(&project, None, None).is_err());
    let row = |k: &str, term: &str, stamp: &str| {
        format!("[[fn]]\npath = \"core::{k}\"\nkappa = \"{k}\"\nembedded = \"{term}\"\nstatus = \"translated\"\n{stamp}digest = \"d\"\n\n")
    };
    let proved = "bridged = \"proved\"\n";
    std::fs::write(
        from.join("m.toml"),
        row("a", "t", proved) + &row("b", "t", proved),
    )
    .expect("measured");
    std::fs::write(to.join("m.toml"), row("a", "t", "") + &row("b", "u", "")).expect("census");
    let carried = uorust(&[
        "carry",
        "--from",
        from.to_str().expect("utf-8"),
        "--register",
        to.to_str().expect("utf-8"),
        "--measured",
        "c0",
    ]);
    assert!(carried.status.success(), "{carried:?}");
    assert!(String::from_utf8_lossy(&carried.stdout)
        .contains("1 bridge stamps carried from c0, 1 rows"));
    let text = std::fs::read_to_string(to.join("m.toml")).expect("carried");
    assert!(text.contains("bridged = \"proved\"\nmeasured = \"c0\""));
    assert!(text.contains("pending = \"remeasure\""));
    assert_eq!(
        uorust::bridge::carry(&from, &to, "c1").expect("again"),
        (1, 1)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// `layouts` over dumps that print no aggregate asks rustc for nothing and
/// writes an empty layout file (UE-19).
#[test]
fn layouts_of_no_aggregate_ask_for_nothing_ue_19() {
    let tmp = std::env::temp_dir().join(format!("uorust-cli-layouts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (mir, copy) = (tmp.join("mir"), tmp.join("library"));
    std::fs::create_dir_all(&mir).expect("dumps");
    std::fs::write(
        mir.join("target.toml"),
        "triple = \"x86_64-unknown-linux-gnu\"\npointer_width = 64\n",
    )
    .expect("target");
    std::fs::create_dir_all(copy.join("core/src")).expect("copy");
    std::fs::write(copy.join("core/src/lib.rs"), "").expect("lib");
    let out = uorust(&[
        "layouts",
        "--crate",
        "core",
        "--mir",
        mir.to_str().expect("utf-8"),
        "--copy",
        copy.to_str().expect("utf-8"),
        "--check",
        "true",
    ]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("layouts: 0 types laid out"));
    assert_eq!(
        uorust::layouts::run("core", &mir, &copy, "true").expect("again"),
        0
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The pre-push hook skips a tested patch's tests only when every commit main
/// gained since touches nothing the patch touches or its tests read
/// (scripts/hooks/passes.py).
#[test]
fn the_hooks_retest_a_patch_unless_main_moved_disjointly() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/hooks/passes.py");
    let run = std::process::Command::new("python3")
        .arg("-I")
        .arg(&script)
        .arg("self-test")
        .output()
        .expect("python3");
    assert!(run.status.success(), "{run:?}");
    assert!(String::from_utf8_lossy(&run.stdout).contains("the disjointness rule holds"));
}

/// SEC-07: `merge --crate` with a version refuses a spec that could name a
/// path, instead of merging it under a library scope.
#[test]
fn merge_refuses_a_bad_registry_spec() {
    let dir = std::env::temp_dir().join(format!("uorust-merge-{}", std::process::id()));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_uorust"))
        .args(["merge", "--crate", "serde@../x", "--shards"])
        .arg(dir.join("shards"))
        .arg("--register")
        .arg(dir.join("register"))
        .output()
        .expect("uorust runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains("`serde@../x` is not a crates.io name and version"),
        "{stderr}"
    );
}
