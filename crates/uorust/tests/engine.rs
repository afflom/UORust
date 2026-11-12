//! The engine against its fixture, against this workspace, and against planted
//! defects (`UE-01` .. `UE-15`).

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
        advisories: None,
    }
}

fn project(root: &Path, name: &str) -> BTreeMap<String, Vec<u8>> {
    uorust::project_workspace(root, &options(name)).expect("projects")
}

/// The frontier without its lifted rows, which come last: what did not lift.
fn unlifted(files: &BTreeMap<String, Vec<u8>>) -> String {
    let frontier = text(files, "frontier.toml");
    frontier
        .split("\n[[lifted]]")
        .next()
        .unwrap_or_default()
        .to_string()
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
    assert!(behavior.contains("(prim quotient (low + high) 2 0 : Nat)"));
    let frontier = text(&files, "frontier.toml");
    let residue = |function: &str, reason: &str| {
        frontier.contains(&format!("function = \"{function}\"\nspan = "))
            && frontier.split("[[residue]]").any(|row| {
                row.contains(function) && row.contains(&format!("reason = \"{reason}\""))
            })
    };
    // Arithmetic whose meaning is a build setting stays typed residue.
    let planted = scratch(
        "arith",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"arith\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (
                "src/lib.rs",
                "/// Overflows.\npub fn mid(a: u32, b: u32) -> u32 {\n    (a + b) / 2\n}\n",
            ),
        ],
    );
    let planted_frontier = text(&project(&planted, "arith"), "frontier.toml");
    assert!(
        planted_frontier.contains("function = \"arith::mid\"")
            && planted_frontier.contains("reason = \"arithmetic\""),
        "{planted_frontier}"
    );
    let _ = std::fs::remove_dir_all(planted);
    // The I/O shell is an effect row, not residue: `load_target` reads a file
    // and `target_from_env` reads the environment, directly or through it.
    let effect = |function: &str, needle: &str| {
        frontier
            .split("[[effect]]")
            .skip(1)
            .any(|row| row.contains(&format!("function = \"{function}\"")) && row.contains(needle))
    };
    assert!(
        effect(
            "thermo_core::config::load_target",
            "Fs effect: fs::read_to_string"
        ),
        "{frontier}"
    );
    assert!(
        !residue("thermo_core::config::load_target", "type"),
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
        advisories: None,
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
        "cd crates/uorust/tests/fixtures/thermostat-prism && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check",
        "project . --out target/uorust-self",
        "cd target/uorust-self && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check",
        "--advisories crates/uorust/tests/fixtures/advised/advisory-db",
        "cd target/uorust-advised && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check",
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

#[test]
fn the_widened_lift_covers_the_fixture_ue_11() {
    let files = project(&fixture(), "thermostat");
    let behavior = text(&files, "src/Behavior.lex.tex");
    for (name, needle) in [
        (
            "width",
            "(prim subtract (project band high) (project band low) : Nat)",
        ),
        ("checkedMidpoint", "<? 4294967296)"),
        ("tick", "(prim remainder (counter + 1) 256 0 : Nat)"),
        (
            "shortfall",
            "(prim subtract (project band low) current : Nat)",
        ),
        ("freezing", "(prim less_than (0 : Int64) outdoor : Bool)"),
        (
            "delta",
            "(prim checked_subtract after before : (Option Int64))",
        ),
        ("checkedValue", "(record TupleBandBool {item0 := band"),
        ("isCelsius", "(prim equal unit \"C\" : Bool)"),
        ("above", "recursive_argument: readings"),
        ("firstReading", "(prim index readings 0 : (Option Nat))"),
        ("targetOr", "| Option.some"),
        ("heatFor", "(ctor Result.error [Bool, ConfigError]"),
        ("fanLabel", "(if (speed =? 0) then \"off\""),
        (
            "labelled",
            "(prim join (cons value (cons \" \" (cons unit (nil String)))) \"\" : String)",
        ),
    ] {
        let decl = behavior
            .split("definition{")
            .find(|d| d.contains(&format!("name: {name},")))
            .unwrap_or_else(|| panic!("{name} is lifted:\n{behavior}"));
        assert!(decl.contains(needle), "{name} lacks {needle}:\n{decl}");
    }
    assert!(behavior.contains("name: Band,"));
    assert!(behavior.contains("statement: ((freezing (-3 : Int64)) = true),"));
    assert!(behavior.contains("axioms: [propext]"));
    let frontier = unlifted(&files);
    assert!(!frontier.contains("function = \"thermo_core::midpoint\""));
    assert!(
        !frontier.contains("function = \"thermo_core::band::"),
        "{frontier}"
    );
}

#[test]
fn iterator_chains_methods_and_maps_lift_ue_15() {
    let files = project(&fixture(), "thermostat");
    let behavior = text(&files, "src/Behavior.lex.tex");
    let decl = |name: &str| -> String {
        behavior
            .split("definition{")
            .find(|d| d.contains(&format!("name: {name},")))
            .unwrap_or_else(|| panic!("{name} is lifted:\n{behavior}"))
            .to_string()
    };
    for (name, needle) in [
        ("roomIsCold", "parameters: [selfValue : Room, floor : Nat]"),
        ("roomReread", "result: Room,"),
        ("roomLabel", "then \"unnamed\" else"),
        ("namesMap1", "recursive_argument: items"),
        ("names", "(namesMap1 rooms)"),
        ("coldFilter1", "(roomIsCold room floor)"),
        ("anyColdAny1", "then true else"),
        ("allWithinAll1", "else false"),
        ("firstColdFind1", "(ctor Option.some [Room] room)"),
        ("highestFold1", "| List.nil => best"),
        (
            "coldCount",
            "(prim length (coldCountFilter1 rooms floor) : Nat)",
        ),
        ("scheduledContains1", "(prim equal item"),
        ("roll", "(prim join (names rooms) \", \" : String)"),
        ("pairs", "axioms: [Classical.choice, Quot.sound, propext]"),
        ("targetOf", "| Option.none => 18"),
        ("roomsOf", "(List TupleStringNat)"),
        ("hasTarget", "(hasTargetHas1 targets room)"),
        ("closed", "(closedContains1 rooms room)"),
        ("labelWidth", "(prim length label : Nat)"),
        ("clamp", "(let low : Nat := (project pair"),
        ("blank", "body: \"\""),
        ("nextMode", "coldArg : Bool"),
        ("coldNamesLoop1", "recursive_argument: items"),
        ("coldNames", "(coldNamesLoop1 rooms"),
        ("reportLoop1", "(record TupleStringNat {item0 := "),
        ("report", "(reportLoop1 rooms (record TupleStringNat"),
        ("readingsOfLoop1", "(prim append state"),
        // A mutual family: a struct joins its enum as an inductive of one
        // constructor, read through accessors, and recursion over it is
        // well-founded on its size.
        ("phaseLabel", "(field_Phase_label phase)"),
        ("hold", "(ctor Phase.mk label (ctor Step.Hold target))"),
        ("size_of_Step", "mutual: SizeOfPhase"),
        ("occupiedTarget", "measure: (size_of_Step step)"),
        ("isFinal", "| Menu.Choose"),
        // A lifetime parameter is erased.
        ("labelOr", "parameters: [phase : Phase, fallback : String]"),
        // An ordered set or map is built by a sorted insert.
        ("labels", "(labelsLoop1 phases out)"),
        (
            "labelsInsert1",
            "then (cons head1 rest1) else (if (prim less_than item1 head1 : Bool)",
        ),
        ("heldTargetsInsert1", "then (cons item1 rest1) else"),
        // format! holes of naturals, booleans and constants.
        ("describe", "(Prelude::natDecimal target)"),
        ("describe", "(if sequence then \"true\" else \"false\")"),
        ("describe", "(cons (heldUnit) "),
        // A while loop is a walk measured by its bound less its cursor,
        // with the theorem that each step decreases it.
        (
            "everyOtherLoop1",
            "measure: (prim subtract (prim length rooms : Nat) at1 : Nat)",
        ),
        (
            "everyOtherLoop1",
            "(everyOtherLoop1 (at1 + 2) (everyOtherStep1 at1 state1 rooms) rooms)",
        ),
        ("firstRoomsLoop1", "evidence: [decreases_firstRoomsLoop1_0]"),
        // A family held in an option of a struct of a list is measured through it.
        ("size_of_option_Floor", "(1 + (size_of_Floor x0))"),
        ("annexedRoom", "measure: (size_of_Plan plan)"),
        // A function parameter is applied; a closure passed for one is its
        // lambda, with the captures it reads.
        ("renamedMap1", "(apply name room)"),
        ("prefixed", "(fun [prefix] (room : String) =>"),
        // `m.entry(k).or_default().push(v)` reads the entry, updates it
        // and inserts it back.
        (
            "byReadingPart1",
            "(match (byReadingGet1 state1 (project room reading))",
        ),
        // A struct written through the crate's modules is the crate's.
        (
            "openingBand",
            "(record Band {low := (project room reading), high := target})",
        ),
        // `()` is LexLean's Unit.
        ("validate", "(ctor Result.ok [Unit, String] ())"),
    ] {
        let d = decl(name);
        assert!(d.contains(needle), "{name} lacks {needle}:\n{d}");
    }
    let frontier = unlifted(&files);
    assert!(!frontier.contains("thermo_core::roster::"), "{frontier}");
    assert!(!frontier.contains("thermo_core::schedule::"), "{frontier}");
    // A pair that names a data enum is a product, which the enum may hold.
    assert!(behavior.contains("{name: Submenu, fields: [(List (String * Menu))]}"));
    assert!(behavior.contains("statement: ((step = (ctor Step.Either occupied rest)) -> ((size_of_Step occupied) < (size_of_Step step)))"));
}

#[test]
fn advisories_are_risks_on_the_supply_chain_ue_12() {
    let advised = root().join("crates/uorust/tests/fixtures/advised");
    let db = advised.join("advisory-db");
    let with_db = Options {
        advisories: Some(db.clone()),
        ..options("advised")
    };
    let files = uorust::project_workspace(&advised, &with_db).expect("projects");
    let security = text(&files, "src/Security.lex.tex");
    for id in ["20180018", "20190009", "20190012", "20210003"] {
        assert!(
            security.contains(&format!("name: riskAdvisoryRustsec{id}V0p6p9,")),
            "{id}:\n{security}"
        );
    }
    // 0.6.9 is patched against RUSTSEC-2018-0003.
    assert!(!security.contains("Rustsec20180003"));
    // deny.toml ignores RUSTSEC-2018-0018 with a reason: an accepted risk, no control.
    assert!(security.contains("name: acceptedThreatAdvisoryRustsec20180018V0p6p9,"));
    assert!(!security.contains("name: controlAdvisoryRustsec20180018V0p6p9,"));
    // The others are answered by the advisory gate, which fails today.
    let measurement = security
        .split("definition{")
        .find(|d| d.contains("name: measurementAdvisoryRustsec20190012V0p6p9,"))
        .expect("a measurement");
    assert!(measurement.contains("passed := false"), "{measurement}");
    let frontier = text(&files, "frontier.toml");
    assert!(!frontier.contains("advisories-unconsulted"), "{frontier}");

    // Without a database, the frontier says no advisory was consulted.
    let files = project(&advised, "advised");
    assert!(text(&files, "frontier.toml").contains("reason = \"advisories-unconsulted\""));
    assert!(!text(&files, "src/Security.lex.tex").contains("Advisory"));

    // Without deny.toml, an advisory nothing answers is refused, not modeled.
    let copy = scratch(
        "ue12",
        &[
            (
                "Cargo.toml",
                &std::fs::read_to_string(advised.join("Cargo.toml")).expect("manifest"),
            ),
            (
                "Cargo.lock",
                &std::fs::read_to_string(advised.join("Cargo.lock")).expect("lock"),
            ),
            (
                "src/lib.rs",
                &std::fs::read_to_string(advised.join("src/lib.rs")).expect("lib"),
            ),
        ],
    );
    let bare = Options {
        advisories: Some(db),
        ..options("advised")
    };
    let files = uorust::project_workspace(&copy, &bare).expect("projects");
    let frontier = text(&files, "frontier.toml");
    assert_eq!(
        frontier.matches("reason = \"unanswered-advisory\"").count(),
        4,
        "{frontier}"
    );
    assert!(!text(&files, "src/Security.lex.tex").contains("Advisory"));
    let _ = std::fs::remove_dir_all(copy);
}

#[test]
fn every_feature_of_this_workspace_is_accepted_ue_13() {
    let files = project(&root(), "uorust");
    let frontier = text(&files, "frontier.toml");
    assert!(
        !frontier.contains("feature-without-acceptance"),
        "{frontier}"
    );
    let interface = text(&files, "src/Interface.lex.tex");
    for feature in ["featureRepoModelCodegen", "featureXtaskMain"] {
        assert!(
            interface.contains(&format!("name: {feature},")),
            "{feature}"
        );
    }
}

/// The `definition{...}` records of the given module texts, by name.
fn records(files: impl IntoIterator<Item = String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for body in files {
        for chunk in body.split("definition{").skip(1) {
            let first = chunk.lines().next().unwrap_or_default();
            let record = if first.contains("body:") {
                first
            } else {
                chunk.split("\n}").next().unwrap_or_default()
            };
            let Some(name) = record.split("name: ").nth(1) else {
                continue;
            };
            let name: String = name
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            out.insert(name, record.to_string());
        }
    }
    out
}

/// A `key := "value"` field of a rendered record.
fn field<'a>(record: &'a str, key: &str) -> &'a str {
    let start = record
        .find(&format!("{key} := \""))
        .unwrap_or_else(|| panic!("no {key} in {record}"))
        + key.len()
        + 5;
    let end = record[start..].find('"').expect("closing quote");
    &record[start..start + end]
}

#[test]
fn the_self_projection_reproduces_the_hand_model_ue_14() {
    let app = root().join("app/src");
    let hand_files: Vec<String> = std::fs::read_dir(&app)
        .expect("app/src")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.to_string_lossy().ends_with(".lex.tex"))
        .map(|path| std::fs::read_to_string(path).expect("hand module"))
        .collect();
    let hand = records(hand_files);
    let files = project(&root(), "uorust");
    let derived = records(
        files
            .iter()
            .filter(|(path, _)| path.starts_with("src/") && path.ends_with(".lex.tex"))
            .map(|(_, bytes)| String::from_utf8(bytes.clone()).expect("utf-8")),
    );

    let table: toml::Value = toml::from_str(
        &std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixpoint.toml"),
        )
        .expect("fixpoint.toml"),
    )
    .expect("fixpoint.toml parses");
    let rows = |key: &str| {
        table
            .get(key)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let get = |row: &toml::Value, key: &str| -> String {
        row.get(key)
            .and_then(|v| v.as_str())
            .expect(key)
            .to_string()
    };

    // Each pair whose two records do not both state the fact is drift.
    let mut drift = Vec::new();
    let mut accounted = std::collections::BTreeSet::new();
    for row in rows("pair") {
        let (h, d, fact) = (get(&row, "hand"), get(&row, "derived"), get(&row, "fact"));
        accounted.insert(h.clone());
        for (side, name, set) in [("hand", &h, &hand), ("derived", &d, &derived)] {
            match set.get(name) {
                None => drift.push(format!("{side} record {name} is missing")),
                Some(body) if !body.contains(&fact) => {
                    drift.push(format!("{side} record {name} does not state `{fact}`"));
                }
                Some(_) => {}
            }
        }
    }
    for row in rows("outside") {
        let h = get(&row, "hand");
        assert!(
            hand.contains_key(&h),
            "outside row names no hand record: {h}"
        );
        assert!(
            !get(&row, "reason").is_empty(),
            "{h} is outside without a reason"
        );
        assert!(
            accounted.insert(h.clone()),
            "{h} is both paired and outside"
        );
    }
    // Every hand record of a checked kind is accounted for, one way or the other.
    let checked: Vec<String> = rows("checked")
        .iter()
        .map(|v| v.as_str().expect("prefix").to_string())
        .collect();
    for name in hand.keys() {
        let kind = checked.iter().find(|prefix| {
            name.strip_prefix(prefix.as_str())
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_ascii_uppercase())
        });
        if kind.is_some() && !accounted.contains(name) {
            drift.push(format!("hand record {name} is neither paired nor outside"));
        }
    }

    // The route table: the hand model's `routes`, in order, is uorust::ROUTES.
    let routes = &hand.get("routes").expect("hand routes")[..];
    let hand_routes: Vec<[String; 4]> = routes
        .split("record Route {")
        .skip(1)
        .map(|r| {
            let r = r.split('}').next().unwrap_or_default();
            ["id", "rustFact", "prismRecord", "facet"].map(|k| field(r, k).to_string())
        })
        .collect();
    let code_routes: Vec<[String; 4]> = uorust::ROUTES
        .iter()
        .map(|r| [r.id, r.rust_fact, r.prism_record, r.facet].map(str::to_string))
        .collect();
    if hand_routes != code_routes {
        drift.push(format!(
            "route table differs:\nhand {hand_routes:?}\ncode {code_routes:?}"
        ));
    }

    assert!(
        hand.get("measureSelfModelDrift")
            .is_some_and(|m| m.contains("threshold := 0")),
        "the hand model's drift threshold is 0"
    );
    assert!(
        drift.is_empty(),
        "self-model drift {}:\n{}",
        drift.len(),
        drift.join("\n")
    );
}

#[test]
fn no_lifted_kappa_turns_unlifted_ue_16() {
    use uorust::register::{admit, Register};
    // The incumbent registers: the committed fixture frontier and this
    // workspace's committed lift register.
    for (root, name, incumbent) in [
        (root(), "uorust", root().join("frontier.toml")),
        (fixture(), "thermostat", golden().join("frontier.toml")),
    ] {
        let files = project(&root, name);
        // Typed, keyed by κ, every function once.
        let candidate = uorust::lift_register(&files).expect("the frontier is a register");
        let committed = std::fs::read_to_string(&incumbent).expect("the committed register");
        let incumbent =
            Register::parse(&incumbent, &committed).expect("the committed register parses");
        let regressed = admit(&incumbent, &candidate);
        assert!(
            regressed.is_empty(),
            "{name}: lifted κ decided otherwise:\n{}",
            regressed.join("\n")
        );
        if name == "uorust" {
            assert!(
                committed == candidate.to_text(),
                "frontier.toml is not this workspace's lift register; run `just register`"
            );
        }
        let frontier: toml::Value =
            toml::from_str(&text(&files, "frontier.toml")).expect("frontier parses");
        let rows = |table: &str| {
            frontier
                .get(table)
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        };
        for row in &rows("residue") {
            for field in ["function", "span", "reason", "detail"] {
                assert!(row.get(field).is_some(), "{name}: untyped residue {row}");
            }
        }
        // A divergence is a lifted row's alone, and it says where the
        // definition parts from the Rust function; the register keeps it.
        for table in ["residue", "foreign", "effect"] {
            for row in rows(table) {
                assert!(row.get("divergence").is_none(), "{name}: {row}");
            }
        }
        for row in rows("lifted") {
            if let Some(divergence) = row.get("divergence") {
                assert!(
                    divergence.as_str().is_some_and(|d| !d.trim().is_empty()),
                    "{name}: an empty divergence {row}"
                );
                let function = row.get("function").and_then(|v| v.as_str());
                assert!(
                    candidate
                        .rows
                        .values()
                        .any(|r| Some(r.function.as_str()) == function
                            && r.divergence.as_deref() == divergence.as_str()),
                    "{name}: the register drops the divergence of {row}"
                );
            }
        }
        for row in &rows("foreign") {
            let detail = row
                .get("detail")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            assert!(
                detail.contains(" crate with no LexLean model"),
                "{name}: {row}"
            );
        }
        // An effect row names the effect it performs or reaches.
        for row in rows("effect") {
            let effect = row
                .get("effect")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            assert!(
                effect.contains(" effect: ") || effect.contains("reaches an effect"),
                "{name}: an effect row without its effect: {row}"
            );
        }
    }
}

/// Every function of product code, counted from the source apart from the
/// engine: free functions, inherent and trait-impl methods and trait methods
/// at item level, outside `#[cfg(test)]` modules and not `#[test]`, in the
/// `src/` tree of every package under `root`.
fn product_functions(root: &Path) -> Vec<String> {
    fn cfg_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            a.path().is_ident("cfg")
                && quote::ToTokens::to_token_stream(a)
                    .to_string()
                    .contains("test")
        })
    }
    fn is_test(attrs: &[syn::Attribute]) -> bool {
        attrs
            .iter()
            .any(|a| a.path().segments.last().is_some_and(|s| s.ident == "test"))
    }
    fn items(list: &[syn::Item], at: &str, out: &mut Vec<String>) {
        for item in list {
            match item {
                syn::Item::Fn(f) if !is_test(&f.attrs) => out.push(format!("{at}{}", f.sig.ident)),
                syn::Item::Impl(i) => {
                    for m in &i.items {
                        if let syn::ImplItem::Fn(f) = m {
                            if !is_test(&f.attrs) {
                                out.push(format!("{at}impl {}", f.sig.ident));
                            }
                        }
                    }
                }
                syn::Item::Trait(t) => {
                    for m in &t.items {
                        if let syn::TraitItem::Fn(f) = m {
                            out.push(format!("{at}trait {}", f.sig.ident));
                        }
                    }
                }
                syn::Item::Mod(m) if !cfg_test(&m.attrs) => {
                    if let Some((_, inner)) = &m.content {
                        items(inner, at, out);
                    }
                }
                _ => {}
            }
        }
    }
    fn walk(dir: &Path, top: bool, out: &mut Vec<String>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .expect("dir")
            .map(|e| e.expect("entry").path())
            .collect();
        entries.sort();
        if !top && entries.iter().any(|p| p.ends_with("Cargo.toml")) {
            return;
        }
        for path in entries {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if !matches!(name, "target" | "fixtures") {
                    walk(&path, false, out);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("source");
                let file = syn::parse_file(&text).expect("parses");
                items(&file.items, &format!("{}: ", path.display()), out);
            }
        }
    }
    fn packages(dir: &Path, out: &mut Vec<String>) {
        if dir.join("Cargo.toml").is_file() && dir.join("src").is_dir() {
            walk(&dir.join("src"), true, out);
        }
        let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .expect("dir")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            let name = sub.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Dot directories hold build output (`app/.prism` holds the Rust
            // prismpm generates), never a package of this workspace.
            if !name.starts_with('.')
                && !matches!(
                    name,
                    "target" | "fixtures" | "src" | "node_modules" | "tests"
                )
            {
                packages(&sub, out);
            }
        }
    }
    let mut out = Vec::new();
    packages(root, &mut out);
    out
}

#[test]
fn every_function_of_product_code_has_exactly_one_row_ue_16() {
    for (root, name) in [(root(), "uorust"), (fixture(), "thermostat")] {
        let files = project(&root, name);
        let frontier: toml::Value =
            toml::from_str(&text(&files, "frontier.toml")).expect("frontier parses");
        let mut rows: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for table in ["residue", "foreign", "effect", "lifted"] {
            for row in frontier
                .get(table)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                let function = row
                    .get("function")
                    .and_then(|v| v.as_str())
                    .expect("a function")
                    .to_string();
                rows.entry(function).or_default().push(table);
            }
        }
        for (function, tables) in &rows {
            assert_eq!(tables.len(), 1, "{name}: {function} has rows in {tables:?}");
        }
        let source = product_functions(&root);
        assert_eq!(
            rows.len(),
            source.len(),
            "{name}: {} functions of product code, {} with a row",
            source.len(),
            rows.len()
        );
    }
}

/// The planted defect: a function the incumbent register records as lifted,
/// whose own source is unchanged (so its κ is the same question), is decided
/// unlifted because a function it calls stopped lifting. The gate refuses the
/// projection, by the register and by `uorust::write` into the old output,
/// and leaves the old projection in place.
#[test]
fn a_lifted_kappa_that_turns_unlifted_is_refused_ue_16() {
    use uorust::register::{admit, Status};
    let manifest =
        "[package]\nname = \"ratchet\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n";
    let caller = "/// Calls the test.\npub fn ready(x: u8) -> bool {\n    over(x)\n}\n";
    let before = scratch(
        "ue16-before",
        &[
            ("Cargo.toml", manifest),
            (
                "src/lib.rs",
                &format!("//! R.\n\n/// The test.\npub fn over(x: u8) -> bool {{\n    x > 1\n}}\n\n{caller}"),
            ),
        ],
    );
    let after = scratch(
        "ue16-after",
        &[
            ("Cargo.toml", manifest),
            (
                "src/lib.rs",
                &format!("//! R.\n\n/// The test.\npub fn over(x: u8) -> bool {{\n    x + 1 > 2\n}}\n\n{caller}"),
            ),
        ],
    );
    let old = project(&before, "ratchet");
    let new = project(&after, "ratchet");
    let incumbent = uorust::lift_register(&old).expect("a register");
    let candidate = uorust::lift_register(&new).expect("a register");
    let (kappa, row) = incumbent
        .rows
        .iter()
        .find(|(_, r)| r.function == "ratchet::ready")
        .expect("ready has a row");
    assert!(matches!(row.status, Status::Lifted(_)), "{row:?}");
    // The same question, now decided otherwise.
    assert!(
        !matches!(candidate.rows[kappa].status, Status::Lifted(_)),
        "{:?}",
        candidate.rows[kappa]
    );
    let refused = admit(&incumbent, &candidate);
    assert!(
        refused.len() == 1 && refused[0].contains(kappa.as_str()),
        "{refused:?}"
    );
    // The changed function is a new question: no regression of its own.
    assert!(refused.iter().all(|r| !r.starts_with("ratchet::over")));
    // Projecting into the incumbent's output is refused, and writes nothing.
    let out = std::env::temp_dir().join(format!("uorust-ue16-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    uorust::write(&out, &old).expect("a first projection");
    match uorust::write(&out, &new) {
        Err(UorustError::Regressed(rows)) => assert_eq!(rows, refused),
        other => panic!("expected Regressed, got {other:?}"),
    }
    assert!(matches!(
        uorust::check(&out, &new),
        Err(UorustError::Regressed(_))
    ));
    assert_eq!(
        std::fs::read(out.join("frontier.toml")).expect("kept"),
        old["frontier.toml"]
    );
    for dir in [before, after, out] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
