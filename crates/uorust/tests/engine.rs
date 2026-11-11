//! The engine against its fixture, against this workspace, and against planted
//! defects (`UE-01` .. `UE-07`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uorust::{Options, UorustError};

const STAMP: &str = "2026-11-11T00:00:00Z";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/uorust is two below the root")
        .to_path_buf()
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thermostat")
}

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thermostat-prism")
}

fn options(name: &str) -> Options {
    Options {
        name: name.to_string(),
        last_modified: STAMP.to_string(),
    }
}

fn project(root: &Path, name: &str) -> BTreeMap<String, Vec<u8>> {
    uorust::project_workspace(root, &options(name)).expect("projects")
}

fn text(files: &BTreeMap<String, Vec<u8>>, path: &str) -> String {
    String::from_utf8(files.get(path).cloned().unwrap_or_default()).expect("utf-8")
}

/// A scratch workspace, written fresh for one test.
fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uorust-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, body) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, body).expect("write");
    }
    dir
}

#[test]
fn the_fixture_projects_to_the_committed_project_ue_01() {
    let files = project(&fixture(), "thermostat");
    if let Err(error) = uorust::check(&golden(), &files) {
        panic!(
            "{error}\nregenerate with: cargo run -p uorust -- project \
             crates/uorust/tests/fixtures/thermostat --out \
             crates/uorust/tests/fixtures/thermostat-prism --name thermostat \
             --last-modified {STAMP}"
        );
    }
    // And nothing stale lingers beside it: every committed file is projected.
    let manifest = std::fs::read_to_string(golden().join(uorust::MANIFEST)).expect("manifest");
    assert_eq!(manifest.lines().count() + 1, files.len());
}

#[test]
fn projections_are_byte_identical_ue_02() {
    for (root, name) in [(fixture(), "thermostat"), (root(), "uorust")] {
        let first = project(&root, name);
        let second = project(&root, name);
        assert_eq!(first, second, "{name} projects differently twice");
        assert!(first.contains_key("src/Architecture.lex.tex"));
    }
}

#[test]
fn ambiguity_is_refused_with_a_typed_open_claim_ue_03() {
    let dir = scratch(
        "ue03",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"amb\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
            ),
            (
                "src/lib.rs",
                "//! Planted defects.\npub mod a;\npub mod b;\npub use a::*;\n\n/// Calls go.\npub fn run() -> u8 {\n    go()\n}\n",
            ),
            (
                "src/a.rs",
                "/// One.\npub fn go() -> u8 {\n    1\n}\n\n/// Both a state and an error.\n#[derive(Debug)]\npub enum LoadState {\n    /// Idle.\n    Idle,\n    /// Done.\n    Done,\n}\n\nimpl std::fmt::Display for LoadState {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        write!(f, \"{self:?}\")\n    }\n}\n\nimpl std::error::Error for LoadState {}\n",
            ),
            ("src/b.rs", "/// Two.\npub fn go() -> u8 {\n    2\n}\n"),
        ],
    );
    let files = project(&dir, "amb");
    let frontier = text(&files, "frontier.toml");
    for reason in [
        "ambiguous-call",
        "ambiguous-fact",
        "feature-without-acceptance",
        "no-msrv",
    ] {
        assert!(
            frontier.contains(&format!("reason = \"{reason}\"")),
            "{reason} missing:\n{frontier}"
        );
    }
    assert!(frontier.contains("status = \"open\""));
    let interface = text(&files, "src/Interface.lex.tex");
    assert!(
        !interface.contains("Foundation.Arch::Feature {"),
        "an unaccepted feature was emitted:\n{interface}"
    );
    let behavior = text(&files, "src/Behavior.lex.tex");
    assert!(!behavior.contains("LoadStateFailure"), "{behavior}");
    assert!(!text(&files, "src/Design.lex.tex").contains("LoadState"));
    let quality = text(&files, "src/Quality.lex.tex");
    assert!(!quality.contains("MinimumRustMinor"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_floor_lifts_and_the_rest_is_typed_residue_ue_04() {
    let files = project(&fixture(), "thermostat");
    let behavior = text(&files, "src/Behavior.lex.tex");
    for name in ["shouldHeat", "withinBand", "nextMode", "halt"] {
        assert!(
            behavior.contains(&format!("name: {name},")),
            "{name}:\n{behavior}"
        );
    }
    assert!(behavior.contains("statement: ((shouldHeat 18 21) = true),"));
    assert!(behavior.contains("statement: ((withinBand 10 5 9) = false),"));
    assert!(behavior.contains("proof: decide{}"));
    assert!(!behavior.contains("name: midpoint"));
    let frontier = text(&files, "frontier.toml");
    let residue = |function: &str, reason: &str| {
        frontier.contains(&format!("function = \"{function}\"\nspan = "))
            && frontier.split("[[residue]]").any(|row| {
                row.contains(function) && row.contains(&format!("reason = \"{reason}\""))
            })
    };
    assert!(residue("thermo_core::midpoint", "arithmetic"), "{frontier}");
    assert!(
        residue("thermo_core::config::parse_target", "type"),
        "{frontier}"
    );
}

#[test]
fn every_failure_is_a_typed_variant_ue_06() {
    let missing = std::env::temp_dir().join("uorust-ue06-does-not-exist");
    assert!(matches!(
        uorust::project_workspace(&missing, &options("x")),
        Err(UorustError::Io { .. })
    ));

    let empty = scratch("ue06-empty", &[("README", "no manifest\n")]);
    assert!(matches!(
        uorust::project_workspace(&empty, &options("x")),
        Err(UorustError::CargoMetadata(_))
    ));
    assert!(matches!(
        uorust::commit_time(&empty),
        Err(UorustError::NoTimestamp)
    ));

    let bad_lock = scratch(
        "ue06-lock",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"l\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
            ),
            ("src/lib.rs", "//! L.\n"),
            ("Cargo.lock", "this is = = not toml\n"),
        ],
    );
    assert!(matches!(
        uorust::project_workspace(&bad_lock, &options("x")),
        Err(UorustError::Malformed { .. })
    ));

    let bad_stamp = Options {
        name: "x".into(),
        last_modified: "yesterday".into(),
    };
    assert!(matches!(
        uorust::project_workspace(&fixture(), &bad_stamp),
        Err(UorustError::BadTimestamp(_))
    ));

    let foreign = scratch("ue06-foreign", &[("notes.txt", "mine\n")]);
    let files = project(&fixture(), "thermostat");
    assert!(matches!(
        uorust::write(&foreign, &files),
        Err(UorustError::ForeignOutput(_))
    ));

    let out = std::env::temp_dir().join(format!("uorust-ue06-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    uorust::write(&out, &files).expect("writes into an absent directory");
    uorust::write(&out, &files).expect("rewrites its own projection");
    uorust::check(&out, &files).expect("a fresh write is current");
    std::fs::write(out.join("src/Behavior.lex.tex"), "planted\n").expect("plant");
    match uorust::check(&out, &files) {
        Err(UorustError::Stale(paths)) => assert_eq!(paths, vec!["src/Behavior.lex.tex"]),
        other => panic!("expected Stale, got {other:?}"),
    }
    for dir in [empty, bad_lock, foreign, out] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn vv_runs_the_sdk_checks_on_both_projections_ue_07() {
    let justfile = std::fs::read_to_string(root().join("Justfile")).expect("Justfile");
    let vv = justfile
        .lines()
        .find(|l| l.starts_with("vv:"))
        .expect("a vv recipe");
    assert!(vv.split_whitespace().any(|w| w == "uorust"), "{vv}");
    let recipe: String = justfile
        .split("\nuorust:\n")
        .nth(1)
        .expect("a uorust recipe")
        .lines()
        .take_while(|l| l.starts_with("    ") || l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    for needle in [
        "check crates/uorust/tests/fixtures/thermostat",
        "cd crates/uorust/tests/fixtures/thermostat-prism && lexlean fmt --check && lexlean lock --check && prismpm check",
        "project . --out target/uorust-self",
        "cd target/uorust-self && lexlean fmt --check && lexlean lock --check && prismpm check",
    ] {
        assert!(recipe.contains(needle), "the uorust recipe lacks `{needle}`:\n{recipe}");
    }
}

#[test]
fn pub_trait_methods_are_interface_calls_ue_08() {
    let files = project(&fixture(), "thermostat");
    let interface = text(&files, "src/Interface.lex.tex");
    for call in ["Read", "Calibrated"] {
        assert!(
            interface.contains(&format!("name: callThermoCoreSensorSensor{call},")),
            "{interface}"
        );
    }
    let feature = interface
        .split("definition{")
        .find(|d| d.contains("name: featureThermoCoreSensor,"))
        .expect("the sensor module is a feature");
    assert!(feature.contains("\"Interface::callThermoCoreSensorSensorRead\""));
    assert!(feature.contains("\"Interface::callThermoCoreSensorSensorCalibrated\""));
    assert!(
        feature.contains("\"Interface::acThermoCoreThermoCoreSensorAFixedSensorReadsItsValue\""),
        "{feature}"
    );
}

#[test]
fn each_module_is_an_opm_view_ue_09() {
    let files = project(&fixture(), "thermostat");
    let design = text(&files, "src/Design.lex.tex");
    for view in [
        "opmModuleLibThermoCore",
        "opmModuleLibThermoCoreConfig",
        "opmModuleLibThermoCoreSensor",
        "opmModuleBinThermo",
    ] {
        assert!(design.contains(&format!("name: {view},")), "{view}");
    }
    let zoom = design
        .split("definition{")
        .find(|d| d.contains("name: opmModuleLibThermoCoreConfigZoom"))
        .expect("load_target zooms into parse_target");
    assert!(zoom.contains("process := \"Design::processThermoCoreConfigLoadTarget\""));
    assert!(zoom.contains("subprocess := \"Design::processThermoCoreConfigParseTarget\""));
    assert!(design.contains("name: opmModuleLibThermoCoreConfigInvokesThermoCoreConfigLoadTargetThermoCoreConfigParseTarget,"));
}

#[test]
fn engine_modules_have_their_own_titles_ue_10() {
    let files = project(&fixture(), "thermostat");
    for (module, title) in [
        ("src/Behavior.lex.tex", "Lifted Rust behavior"),
        ("src/SupplyChain.lex.tex", "Locked supply chain"),
    ] {
        let source = text(&files, module);
        assert!(source.contains("\\useglossary{uorust.titles@1.0.0}"));
        assert!(source.contains(&format!("\\title{{{title}}}")), "{source}");
    }
    for path in ["lexlean.toml", "lexlean.lock"] {
        assert!(text(&files, path).contains("language/uorust.titles"));
    }
    let surfaces: String = ["behavior", "supply-chain"]
        .iter()
        .map(|e| text(&files, &format!("language/uorust.titles/entries/{e}.toml")))
        .collect();
    assert!(surfaces.contains("surface = \"Lifted Rust behavior\""));
    assert!(surfaces.contains("surface = \"Locked supply chain\""));
}
