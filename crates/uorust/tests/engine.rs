//! The engine against its fixture, against this workspace, and against planted
//! defects (`UE-01` .. `UE-17`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uorust::{Options, UorustError};

const STAMP: &str = "2026-11-11T00:00:00Z";

type Projection = BTreeMap<String, Vec<u8>>;

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
        mir: None,
        limits: None,
        embedding: None,
    }
}

/// A projection; this workspace's runs the MIR path on its committed verdict
/// register, as `just register` and `just uorust` do.
fn project(root: &Path, name: &str) -> BTreeMap<String, Vec<u8>> {
    // The acceptance run reads an immutable checkout. Share that expensive
    // self projection across independent assertions; determinism stays fresh.
    static SELF: std::sync::Mutex<BTreeMap<String, Projection>> =
        std::sync::Mutex::new(BTreeMap::new());
    if root != self::root() {
        return project_fresh(root, name);
    }
    let mut cache = SELF
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry(name.to_string())
        .or_insert_with(|| project_fresh(root, name))
        .clone()
}

fn project_fresh(root: &Path, name: &str) -> BTreeMap<String, Vec<u8>> {
    let mut options = options(name);
    if root == self::root() {
        options.mir = Some(mir_options(
            dumped(root, name),
            verdicts(&root.join("lift-verdicts.toml")),
            uorust::mir::Trial::Off,
        ));
    }
    uorust::project_workspace(root, &options).expect("projects")
}

/// The MIR of the workspace at `root`, dumped once per test run.
fn dumped(root: &Path, name: &str) -> PathBuf {
    if !root.join("Cargo.lock").is_file() {
        let locked = std::process::Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(root)
            .output()
            .expect("lock the compiler fixture");
        assert!(
            locked.status.success(),
            "{}",
            String::from_utf8_lossy(&locked.stderr)
        );
    }
    static DUMPS: std::sync::Mutex<BTreeMap<String, PathBuf>> =
        std::sync::Mutex::new(BTreeMap::new());
    let mut dumps = DUMPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    dumps
        .entry(name.to_string())
        .or_insert_with(|| {
            let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mir-{name}"));
            uorust::mir::dump(root, &out, &Default::default()).expect("the MIR dumps");
            out
        })
        .clone()
}

fn verdicts(path: &Path) -> uorust::mir::verdict::Verdicts {
    uorust::mir::verdict::Verdicts::read(path).expect("the verdict register parses")
}

fn mir_options(
    dumps: PathBuf,
    verdicts: uorust::mir::verdict::Verdicts,
    trial: uorust::mir::Trial,
) -> uorust::mir::MirOptions {
    uorust::mir::MirOptions {
        dumps,
        verdicts,
        trial,
        learned: BTreeMap::new(),
    }
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

/// The rows of the array of tables `table` in a projection's frontier.
fn frontier_rows(files: &BTreeMap<String, Vec<u8>>, table: &str) -> Vec<toml::Value> {
    let frontier: toml::Value =
        toml::from_str(&text(files, "frontier.toml")).expect("frontier parses");
    frontier
        .get(table)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// The `[[refusal]]` rows of `reason` in a projection's frontier, and the
/// count the work queue's `[[reason]]` row of that reason carries: read as
/// rows, so a reason text in the queue never counts as a refusal and a
/// refusal row is never stood in for by the queue's summary.
fn refused(files: &BTreeMap<String, Vec<u8>>, reason: &str) -> (usize, Option<i64>) {
    let of = |row: &toml::Value| row.get("reason").and_then(|v| v.as_str()) == Some(reason);
    let rows = frontier_rows(files, "refusal")
        .iter()
        .filter(|r| of(r))
        .count();
    let queued: Vec<i64> = frontier_rows(files, "reason")
        .iter()
        .filter(|r| of(r))
        .map(|r| {
            r.get("refusals")
                .and_then(|v| v.as_integer())
                .expect("a queue row counts its refusals")
        })
        .collect();
    assert!(queued.len() <= 1, "{reason} is queued twice: {queued:?}");
    (rows, queued.first().copied())
}

/// The reasons of the `[[residue]]` rows of `function`.
fn residue_reasons(files: &BTreeMap<String, Vec<u8>>, function: &str) -> Vec<String> {
    frontier_rows(files, "residue")
        .iter()
        .filter(|r| r.get("function").and_then(|v| v.as_str()) == Some(function))
        .filter_map(|r| r.get("reason").and_then(|v| v.as_str()))
        .map(str::to_string)
        .collect()
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
        let first = project_fresh(&root, name);
        let second = project_fresh(&root, name);
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
                "//! Planted defects.\npub mod a;\npub mod b;\npub use a::*;\n\n/// Calls go.\npub fn run() -> u8 {\n    go()\n}\n\n/// Picks an implementation by its argument's type.\npub fn pick(make: impl FnOnce() -> u8) -> a::Wide {\n    a::Wide::from(make())\n}\n",
            ),
            (
                "src/a.rs",
                "/// One.\npub fn go() -> u8 {\n    1\n}\n\n/// A byte.\npub struct Pair(pub u8);\n\n/// A wide value.\npub struct Wide(pub u32);\n\nimpl From<u8> for Wide {\n    fn from(x: u8) -> Wide {\n        Wide(u32::from(x))\n    }\n}\n\nimpl From<u16> for Wide {\n    fn from(x: u16) -> Wide {\n        Wide(u32::from(x) + 1)\n    }\n}\n\n/// Both a state and an error.\n#[derive(Debug)]\npub enum LoadState {\n    /// Idle.\n    Idle,\n    /// Done.\n    Done,\n}\n\nimpl std::fmt::Display for LoadState {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        write!(f, \"{self:?}\")\n    }\n}\n\nimpl std::error::Error for LoadState {}\n",
            ),
            ("src/b.rs", "/// Two.\npub fn go() -> u8 {\n    2\n}\n"),
        ],
    );
    let files = project(&dir, "amb");
    let frontier = text(&files, "frontier.toml");
    // Each reason from a refusal row of its own, an open claim; the queue's
    // [[reason]] counts alone do not show one.
    let refusals: Vec<&str> = frontier
        .split("\n[[")
        .filter_map(|row| row.strip_prefix("refusal]]\n"))
        .collect();
    for reason in [
        "ambiguous-call",
        "ambiguous-fact",
        "feature-without-acceptance",
        "no-msrv",
    ] {
        assert!(
            refusals.iter().any(
                |row| row.lines().any(|l| l == format!("reason = \"{reason}\""))
                    && row.lines().any(|l| l == "status = \"open\"")
            ),
            "{reason} missing:\n{frontier}"
        );
    }
    // The resolver now reads tuple fields. A callback's result keeps this
    // valid Rust call outside its type-reading floor, with both `From`
    // implementations still possible to the resolver.
    let ambiguous = refusals
        .iter()
        .find(|row| row.contains("reason = \"ambiguous-call\""))
        .expect("an ambiguous-call row");
    assert!(
        ambiguous.contains("span = \"src/lib.rs:12\"")
            && ambiguous.contains("Wide::from")
            && ambiguous.contains("src/a.rs:13")
            && ambiguous.contains("src/a.rs:19"),
        "{ambiguous}"
    );
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
fn a_call_admission_resolves_lifts_as_the_function_it_resolves_to() {
    // `go` of the root module is `a::go`, through the glob import: one
    // function by the language's name resolution, which admission and the
    // lift both read it by.
    let dir = scratch(
        "glob-call",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"amb\"\nversion = \"0.1.0\"\nedition = \"2021\"\nrust-version = \"1.70\"\n\n[workspace]\n",
            ),
            (
                "src/lib.rs",
                "//! A glob import.\npub mod a;\npub mod b;\npub use a::*;\n\n/// Calls go.\npub fn run() -> u8 {\n    go()\n}\n",
            ),
            ("src/a.rs", "/// One.\npub fn go() -> u8 {\n    1\n}\n"),
            ("src/b.rs", "/// Two.\npub fn go() -> u8 {\n    2\n}\n"),
        ],
    );
    let files = project(&dir, "amb");
    let _ = std::fs::remove_dir_all(&dir);
    let frontier = text(&files, "frontier.toml");
    assert!(!frontier.contains("ambiguous-call"), "{frontier}");
    assert!(
        frontier.contains("[lift]\nfunctions = 3\nlifted = 3\nresidue = 0\n"),
        "{frontier}"
    );
    let behavior = text(&files, "src/Behavior.lex.tex");
    let run = behavior
        .split("definition{")
        .find(|d| d.contains("name: run,"))
        .unwrap_or_else(|| panic!("run is not lifted:\n{behavior}"));
    assert!(run.contains("body: (aGo)}"), "run calls another go:\n{run}");
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
    let planted_files = project(&planted, "arith");
    assert_eq!(
        residue_reasons(&planted_files, "arith::mid"),
        ["arithmetic"],
        "{}",
        text(&planted_files, "frontier.toml")
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
        residue_reasons(&files, "thermo_core::config::load_target").is_empty(),
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
        mir: None,
        limits: None,
        embedding: None,
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
        "thermostat-prism/library/*/; do (cd \"$lib\" && lexlean fmt --check && lexlean lock --check && prismpm check && prismpm verify) || exit 1; done",
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
        feature
            .contains("\"Interface::acThermoCoreThermoCoreSensorTestsAFixedSensorReadsItsValue\""),
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
        ("holdsOff", "mutual: holdsOffMutual"),
        (
            "steady",
            "(if coldArg then (if occupied then false else true)",
        ),
        ("hasLine", "(let lines : (List String) := (nil String);"),
        ("phasesOff", "measure: (size_of_list_Phase phases)"),
        (
            "phaseOff",
            "(match phase | Phase.mk label step => (holdsOff step))",
        ),
        (
            "clamped",
            "(let state1 : TupleBoolNat := (if (value <? (project band low))",
        ),
        ("above", "recursive_argument: readings"),
        ("firstReading", "(prim index readings 0 : (Option Nat))"),
        ("targetOr", "| Option.some"),
        ("heatFor", "(ctor Result.error [Bool, ConfigError]"),
        ("fanLabel", "(if (speed =? 0) then \"off\""),
        (
            "labelled",
            "(prim join (prim append (cons value (nil String)) (cons \" \" (cons unit (nil String))) : (List String)) \"\" : String)",
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
    assert_eq!(
        refused(&files, "advisories-unconsulted"),
        (1, Some(1)),
        "{}",
        text(&files, "frontier.toml")
    );
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
    // Four refusals, one per advisory, and the work queue counts them.
    assert_eq!(
        refused(&files, "unanswered-advisory"),
        (4, Some(4)),
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
    // `uorust mir` and the module `uorust::mir` are two features, told apart.
    for feature in [
        "featureRepoModelCodegen",
        "featureXtaskMain",
        "featureUorustMir",
        "featureUorustCommandMir",
    ] {
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
        // The reason is typed, and what each kind claims is checked.
        let body = &hand[&h];
        match get(&row, "kind").as_str() {
            "external-system" => assert!(
                body.contains("ComponentKind.externalSystem"),
                "{h} is outside as an external system but is not one"
            ),
            "private-stage" => {
                let module = get(&row, "module");
                let lib = std::fs::read_to_string(root().join("crates/uorust/src/lib.rs"))
                    .expect("lib.rs");
                assert!(
                    lib.lines().any(|l| l == format!("mod {module};")),
                    "{h} names {module}, which is not a private module of the library"
                );
            }
            "outside-rust" => {
                let path = root().join(get(&row, "path"));
                assert!(path.exists(), "{h} names a path that does not exist");
                let mut stack = vec![path];
                while let Some(at) = stack.pop() {
                    if at.is_dir() {
                        if at.ends_with("target")
                            || at.file_name().is_some_and(|n| {
                                n.to_string_lossy().starts_with('.') && n != ".github"
                            })
                        {
                            continue;
                        }
                        for entry in std::fs::read_dir(&at).expect("dir") {
                            stack.push(entry.expect("entry").path());
                        }
                    } else {
                        assert!(
                            at.extension().is_none_or(|e| e != "rs"),
                            "{h} is outside the Rust code, but {} is Rust",
                            at.display()
                        );
                    }
                }
            }
            "run-time" => {
                let gate = get(&row, "gate");
                let ids = std::fs::read_to_string(root().join("model/ids.toml")).expect("ids");
                let just = std::fs::read_to_string(root().join("Justfile")).expect("Justfile");
                assert!(
                    ids.contains(&format!("id = \"{gate}\""))
                        || just.lines().any(|l| l.starts_with(&format!("{gate}:"))),
                    "{h} names a gate, {gate}, that is neither a register ID nor a recipe"
                );
            }
            "reviewer-judgement" => assert!(
                h.starts_with("modeledThreat") || h.starts_with("modeledControl"),
                "{h} is a reviewer's judgement but not a modeled threat or control"
            ),
            "this-gate" => assert_eq!(h, "measureSelfModelDrift", "{h} is not this gate's measure"),
            other => panic!("{h} is outside with an untyped kind {other}"),
        }
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
/// at item level, outside `#[cfg(test)]` modules (inline, or out of line in
/// their own file or directory) and not `#[test]`, in the `src/` tree of
/// every package of the workspace at `root` ([`members`]).
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
    /// The functions of `list`, and in `tests` the file and the directory of
    /// each out-of-line `#[cfg(test)] mod name;`, which lie in `dir`.
    fn items(
        list: &[syn::Item],
        at: &str,
        dir: &Path,
        out: &mut Vec<String>,
        tests: &mut Vec<PathBuf>,
    ) {
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
                        items(inner, at, &dir.join(m.ident.to_string()), out, tests);
                    }
                }
                syn::Item::Mod(m) if m.content.is_none() => {
                    assert!(
                        !m.attrs.iter().any(|a| a.path().is_ident("path")),
                        "{at}a #[path] test module this walker does not resolve"
                    );
                    let name = m.ident.to_string();
                    tests.push(dir.join(format!("{name}.rs")));
                    tests.push(dir.join(name));
                }
                _ => {}
            }
        }
    }
    fn walk(dir: &Path, top: bool, out: &mut Vec<PathBuf>) {
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
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    for package in members(root) {
        assert!(
            package.join("src").is_dir(),
            "{}: a member with no src/ tree",
            package.display()
        );
        let mut files = Vec::new();
        walk(&package.join("src"), true, &mut files);
        let (mut found, mut tests) = (Vec::new(), Vec::new());
        for path in &files {
            let text = std::fs::read_to_string(path).expect("source");
            let file = syn::parse_file(&text).expect("parses");
            // A module's children lie beside a crate root or a `mod.rs`, and
            // in the directory named after any other file.
            let parent = path.parent().expect("a directory");
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let root_file = matches!(stem, "lib" | "main" | "mod") || parent.ends_with("src/bin");
            let dir = if root_file {
                parent.to_path_buf()
            } else {
                parent.join(stem)
            };
            let mut functions = Vec::new();
            items(
                &file.items,
                &format!("{}: ", path.display()),
                &dir,
                &mut functions,
                &mut tests,
            );
            found.push((path, functions));
        }
        for (path, functions) in found {
            if !tests.iter().any(|t| path.starts_with(t)) {
                out.extend(functions);
            }
        }
    }
    out
}

/// The packages of the workspace whose root is `root`, as cargo decides
/// them and read apart from the engine (which asks `cargo metadata`): the
/// root itself when its manifest has a `[package]`, and every directory a
/// `workspace.members` entry names or matches, less `workspace.exclude`. A
/// package below `root` that is not a member is not this workspace's.
fn members(root: &Path) -> Vec<PathBuf> {
    let manifest: toml::Value = toml::from_str(
        &std::fs::read_to_string(root.join("Cargo.toml")).expect("the root manifest"),
    )
    .expect("the root manifest parses");
    let list = |key: &str| -> Vec<String> {
        manifest
            .get("workspace")
            .and_then(|w| w.get(key))
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .map(|v| v.as_str().expect("a path").to_string())
            .collect()
    };
    let excluded: Vec<PathBuf> = list("exclude").iter().map(|e| root.join(e)).collect();
    let mut out = Vec::new();
    if manifest.get("package").is_some() {
        out.push(root.to_path_buf());
    }
    for pattern in list("members") {
        let mut found = vec![root.to_path_buf()];
        for part in pattern.split('/').filter(|p| !p.is_empty() && *p != ".") {
            assert!(
                !part.contains("**") && !part.contains('['),
                "{pattern}: a member glob this reader does not expand"
            );
            let mut next = Vec::new();
            for dir in found {
                if !part.contains(['*', '?']) {
                    next.push(dir.join(part));
                    continue;
                }
                let mut matched: Vec<PathBuf> = std::fs::read_dir(&dir)
                    .expect("a member's parent")
                    .map(|e| e.expect("entry").path())
                    .filter(|p| p.is_dir())
                    .filter(|p| {
                        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        glob_matches(part, name)
                    })
                    .collect();
                matched.sort();
                next.extend(matched);
            }
            found = next;
        }
        for dir in found {
            assert!(
                dir.join("Cargo.toml").is_file(),
                "{pattern}: {} is a member with no manifest",
                dir.display()
            );
            if !excluded.contains(&dir) && !out.contains(&dir) {
                out.push(dir);
            }
        }
    }
    out
}

/// Whether one path component `name` matches the glob component `pattern`
/// (`*` any run of characters, `?` one character), as cargo's member globs.
fn glob_matches(pattern: &str, name: &str) -> bool {
    fn at(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| at(rest, &n[i..])),
            Some(('?', rest)) => !n.is_empty() && at(rest, &n[1..]),
            Some((c, rest)) => n.first() == Some(c) && at(rest, &n[1..]),
        }
    }
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    at(&p, &n)
}

/// Product code is what cargo makes a member of the workspace: the walker
/// reads the root manifest's `workspace.members` and `exclude` itself, so a
/// member with no row fails this test whatever its directory is called, and a
/// package that is not a member is not counted.
///
/// Left outside by that rule today, as an open row for a later projection of
/// that workspace under its own profile: the three packages the installed
/// Prism factory brings, none a member here. `prism-factory/host` declares
/// its own `[workspace]` and is upstream's factory host, held to upstream's
/// text by `prism-factory/upstream.toml` and `graph.py`; its one adapted file
/// is covered by the factory's own gate (UE-20), not by this lift register.
/// `prism-factory/certified/packages/rust-std/factory_plan` and
/// `factory_reweigh` are Rust that a LexLean production generated and that
/// `verify-artifact` replays byte for byte.
#[test]
fn every_function_of_product_code_has_exactly_one_row_ue_16() {
    // The walker's own rule, on a planted workspace: members by path and by
    // glob count, an excluded directory, a package that is not a member (its
    // own `[workspace]`, beside the members) and an out-of-line test module
    // do not.
    let package = |name: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
    let planted = scratch(
        "ue16-members",
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\", \"tool\"]\nexclude = [\"crates/skipped\"]\n",
            ),
            ("crates/alpha/Cargo.toml", &package("alpha")),
            (
                "crates/alpha/src/lib.rs",
                "/// A.\npub fn alpha() {}\n/// T.\npub struct T;\nimpl T {\n    /// M.\n    pub fn method(&self) {}\n}\n",
            ),
            ("crates/beta/Cargo.toml", &package("beta")),
            (
                "crates/beta/src/lib.rs",
                "/// B.\npub fn beta() {}\nmod inner;\n#[cfg(test)]\nmod tests;\n",
            ),
            ("crates/beta/src/inner.rs", "/// I.\npub fn inner() {}\n"),
            ("crates/beta/src/tests.rs", "/// Only a test's.\npub fn helper() {}\n"),
            ("crates/beta/src/tests/deep.rs", "/// A test's too.\npub fn deep() {}\n"),
            ("crates/skipped/Cargo.toml", &package("skipped")),
            ("crates/skipped/src/lib.rs", "/// S.\npub fn skipped() {}\n"),
            ("tool/Cargo.toml", &package("tool")),
            ("tool/src/main.rs", "fn main() {}\n"),
            (
                "outside/host/Cargo.toml",
                &format!("{}\n[workspace]\n", package("host")),
            ),
            ("outside/host/src/lib.rs", "/// H.\npub fn host() {}\n"),
        ],
    );
    let names: Vec<String> = product_functions(&planted)
        .iter()
        .map(|f| f.rsplit(": ").next().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        names,
        ["alpha", "impl method", "inner", "beta", "main"],
        "{names:?}"
    );
    let _ = std::fs::remove_dir_all(planted);

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

/// The law per function, on a register the tool wrote: two functions of a
/// synthetic workspace stopped by one construct of their signatures, which
/// the register records among every construct their signatures state. The
/// planted defect moves one of them, unchanged, under another construct
/// while the register still records the first for it: `uorust::admit_register`
/// (the `uorust admit --parent` command) refuses it, naming the function,
/// whether or not the other moves too. Moving both on where the candidate's
/// signatures no longer state the construct (its blocker shown gone) raises
/// the other cause and is admitted.
#[test]
fn a_function_stays_under_its_construct_while_it_stops_one_ue_16() {
    use uorust::register::{Register, Status};
    let manifest =
        "[package]\nname = \"moves\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n";
    let lib = "//! M.\n\nuse std::cell::RefCell;\n\n\
               /// One.\npub fn one(x: &RefCell<u8>) -> u8 {\n    *x.borrow()\n}\n\n\
               /// Two.\npub fn two(x: &RefCell<u8>) -> u8 {\n    *x.borrow() + 1\n}\n";
    let root = scratch(
        "ue16-moves",
        &[("Cargo.toml", manifest), ("src/lib.rs", lib)],
    );
    let parent = uorust::lift_register(&project(&root, "moves")).expect("a register");
    let stopped = |name: &str| {
        parent
            .rows
            .iter()
            .find(|(_, r)| r.function == format!("moves::{name}"))
            .map(|(k, r)| (k.clone(), r.status.clone()))
            .expect("a row")
    };
    let ((k1, s1), (k2, s2)) = (stopped("one"), stopped("two"));
    let construct = s1.construct().map(|(c, d)| (c, d.to_string()));
    assert!(
        construct.is_some() && construct == s2.construct().map(|(c, d)| (c, d.to_string())),
        "both functions are stopped by one construct: {s1:?} {s2:?}"
    );
    let recorded = |k: &String| {
        let stops = parent.rows[k].stops.clone().unwrap_or_default();
        let (cause, detail) = s1.construct().expect("stopped");
        stops.contains(&(cause, detail.to_string()))
    };
    assert!(recorded(&k1) && recorded(&k2), "{parent:?}");
    let next = Status::Residue("form".into(), "a construct planted as the next".into());
    let with = |moved: &[&String], gone: bool| {
        let mut r: Register = parent.clone();
        for k in moved {
            let row = r.rows.get_mut(*k).expect("a row");
            row.status = next.clone();
            if gone {
                row.stops = Some(Vec::new());
            }
        }
        r
    };
    let dir = scratch("ue16-moves-registers", &[]);
    std::fs::create_dir_all(&dir).expect("dir");
    let file = |name: &str, r: &Register| {
        let path = dir.join(name);
        std::fs::write(&path, r.to_text()).expect("write");
        path
    };
    let at = file("parent.toml", &parent);
    // Both moved on, the construct gone from both signatures: the cause
    // `form` rises, admitted.
    let moved = with(&[&k1, &k2], true);
    assert!(uorust::admit_register(&at, &file("moved.toml", &moved)).is_ok());
    // One moved while its signature still states the construct: refused,
    // whether the other stays or moves too.
    for planted in [with(&[&k1], false), {
        let mut both = with(&[&k1, &k2], false);
        both.rows.get_mut(&k2).expect("two").stops = Some(Vec::new());
        both
    }] {
        match uorust::admit_register(&at, &file("planted.toml", &planted)) {
            Err(UorustError::Regressed(rows)) => {
                assert!(
                    rows.len() == 1 && rows[0].starts_with("moves::one "),
                    "{rows:?}"
                );
            }
            other => panic!("expected Regressed, got {other:?}"),
        }
    }
    for dir in [root, dir] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn mir_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mir-path")
}

/// The fixture projected through the MIR path on `verdicts`.
fn walk(
    verdicts: uorust::mir::verdict::Verdicts,
    trial: uorust::mir::Trial,
) -> BTreeMap<String, Vec<u8>> {
    let root = mir_fixture();
    let mut options = options("walk");
    options.mir = Some(mir_options(dumped(&root, "walk"), verdicts, trial));
    uorust::project_workspace(&root, &options).expect("projects")
}

/// The detail of the residue row of `function` in a projection's frontier.
fn residue_detail(files: &BTreeMap<String, Vec<u8>>, function: &str) -> Option<String> {
    let frontier: toml::Value = toml::from_str(&text(files, "frontier.toml")).expect("frontier");
    frontier
        .get("residue")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|r| r.get("function").and_then(|v| v.as_str()) == Some(function))
        .and_then(|r| r.get("detail").and_then(|v| v.as_str()))
        .map(str::to_string)
}

fn lifted_functions(files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    let frontier: toml::Value = toml::from_str(&text(files, "frontier.toml")).expect("frontier");
    frontier
        .get("lifted")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("function").and_then(|v| v.as_str()))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_mir_kind_of_the_pinned_toolchain_has_a_coverage_row_ue_17() {
    let toolchain = std::fs::read_to_string(root().join("rust-toolchain.toml")).expect("toolchain");
    let source =
        std::fs::read_to_string(root().join("crates/uorust/mir/SOURCE.toml")).expect("source");
    let channel = |text: &str, key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key} = \"")))
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(
        channel(&source, "toolchain"),
        channel(&toolchain, "channel")
    );
    let report = uorust::mir::coverage::report();
    assert!(report.contains(&format!("at toolchain {}", channel(&toolchain, "channel"))));
    for kind in [
        "StatementKind::Assign: lowered",
        "TerminatorKind::SwitchInt: lowered",
        "TerminatorKind::Assert: lowered",
        "Rvalue::BinaryOp: lowered",
        "BinOp::AddWithOverflow: lowered",
        "Rvalue::RawPtr: residue",
        "TerminatorKind::InlineAsm: residue",
    ] {
        assert!(report.contains(kind), "no coverage row {kind}");
    }
}

#[test]
fn the_mir_path_translates_and_admits_on_verdicts_ue_17() {
    use uorust::mir::verdict::Decision;
    use uorust::mir::Trial;
    // The dump: one file per body of the library.
    let fresh = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mir-walk-fresh");
    uorust::mir::dump(&mir_fixture(), &fresh, &Default::default()).expect("the MIR dumps");
    assert!(std::fs::read_dir(fresh.join("lib-walk"))
        .expect("the library's dumps")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .any(|n| n.starts_with("walk.count.")));
    // The trial run: every translation is admitted, with its loop helper,
    // the helper's measure and decrease theorems, and a theorem per
    // obligation; the prover reads the listing of candidates.
    let trial = walk(Default::default(), Trial::All);
    let behavior = text(&trial, "src/Behavior.lex.tex");
    let rows = frontier_rows(&trial, "lifted");
    let name = |function: &str| -> String {
        rows.iter()
            .find(|r| r["function"].as_str() == Some(function))
            .and_then(|r| r["definition"].as_str())
            .and_then(|d| d.strip_prefix("Behavior::"))
            .unwrap_or_else(|| panic!("no lifted trial declaration for {function}"))
            .to_string()
    };
    let capped = name("walk::capped_sum");
    let count = name("walk::count");
    let above = name("walk::above");
    let rise = name("walk::first_rise");
    for decl in [
        format!("name: {capped}Loop1,"),
        format!("name: decreases_{capped}Loop1_0,"),
        format!("name: holds_{capped}_0,"),
        format!("name: {capped},"),
        format!("name: {count},"),
        format!("name: {above},"),
        format!("name: {rise}Loop2,"),
    ] {
        assert!(
            behavior.contains(&decl),
            "the trial projection lacks {decl}"
        );
    }
    for decl in [
        "termination: {",
        // std's own `Option::unwrap_or`, lifted from its vendored MIR at
        // `u32`, and a promoted constant lowered from its own body.
        "name: stdOptionOptionUnwrapOrU32,",
        // `BTreeSet::insert` by its model, a backward function of the
        // mutable borrow; `str::trim` by its model over the prelude.
        "name: stdInsertString,",
        "(stdInsertString (stdInsertString (nil String) a) b)",
        "(Prelude::textTrim ",
        // A `for` loop over a slice: its iterator a cursor, the loop's
        // measure the distance between the cursor's positions, which its
        // guarded `next` is `Some` within.
        "measure: (prim subtract (project (project loopState item1) item2) (project (project loopState item1) item1) : Nat)",
        "name: stdAtNat,",
        // A closure: a definition over the value of its captures, called
        // from std's `find`, which is lifted from std's own MIR.
        "name: closureFirstNamed0,",
        "name: stdSliceIterIterFindClosure",
    ] {
        assert!(behavior.contains(decl), "the trial projection lacks {decl}");
    }
    let listing = text(&trial, uorust::mir::CANDIDATES);
    for function in ["walk::capped_sum", "walk::count"] {
        assert!(
            listing.contains(&format!("function = \"{function}\"\ndigest = ")),
            "{function} is not a candidate"
        );
    }
    // A kind or type the translator does not lower is a residue row that
    // names it.
    assert_eq!(
        residue_detail(&trial, "walk::bump").as_deref(),
        Some("mir::Ty: std::cell::Cell<u32>")
    );

    // The committed verdicts: the proved translation lifts, the refused one
    // is residue naming the verifier's diagnostic.
    let committed = verdicts(&mir_fixture().join("lift-verdicts.toml"));
    let decided = walk(committed.clone(), Trial::Off);
    for f in [
        "walk::capped_sum",
        "walk::distinct",
        "walk::first_blank",
        "walk::above",
        // `for` loops over a slice and a set, `break`, and `?` on an option.
        "walk::first_rise",
        "walk::sum_until",
        "walk::second_known",
        // A closure passed to std's `find`.
        "walk::first_named",
        // Invariant closure captures remain available in the loop state.
        "walk::any_above",
        // Signed integers compared as their bit patterns.
        "walk::first_negative",
    ] {
        assert!(
            lifted_functions(&decided).contains(&f.to_string()),
            "{f} does not lift"
        );
    }
    assert!(text(&decided, "src/Behavior.lex.tex").contains(&format!("name: {capped}Loop1,")));
    assert!(!decided.contains_key(uorust::mir::CANDIDATES));
    let count = residue_detail(&decided, "walk::count").expect("count is residue");
    assert!(count.starts_with("mir::Verdict: LLV"), "{count}");
    // The MIR path only adds: whatever lifts without it lifts with it.
    let surface = project(&mir_fixture(), "walk");
    for f in lifted_functions(&surface) {
        assert!(
            lifted_functions(&decided).contains(&f),
            "{f} stopped lifting"
        );
    }

    // A verdict of another digest admits nothing: a changed translation is a
    // new question.
    let mut stale = committed;
    for v in stale.rows.values_mut() {
        if v.status == Decision::Proved {
            v.digest = "0".repeat(64);
        }
    }
    let planted = walk(stale, Trial::Off);
    assert!(!lifted_functions(&planted).contains(&"walk::capped_sum".to_string()));
    assert_eq!(
        residue_detail(&planted, "walk::capped_sum").as_deref(),
        Some("mir::Verdict: a translation the verifier has not decided yet")
    );
    assert!(!text(&planted, "src/Behavior.lex.tex").contains("name: walkCappedSum,"));
}

/// `prove` on a register that decides every translation runs no verifier and
/// writes the same register back.
#[test]
fn the_prover_keeps_a_decided_register_ue_17() {
    let root = mir_fixture();
    let committed = std::fs::read_to_string(root.join("lift-verdicts.toml")).expect("verdicts");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mir-walk-prove");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let verdicts = dir.join("lift-verdicts.toml");
    std::fs::write(&verdicts, &committed).expect("verdicts");
    let summary = uorust::mir::prove(
        &root,
        &options("walk"),
        &dumped(&root, "walk"),
        &verdicts,
        &dir.join("scratch"),
    )
    .expect("proves");
    assert_eq!((summary.proved, summary.refused, summary.runs), (0, 0, 0));
    assert_eq!(
        std::fs::read_to_string(&verdicts).expect("verdicts"),
        committed
    );
}

/// A planted defect: a shard whose rust-src is absent. Its census refuses,
/// so the shard and the run are red, rather than translating every body
/// against no declarations and recording what it got.
#[test]
fn a_census_without_its_library_sources_refuses() {
    use uorust::mir::core;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core");
    let missing = fixture.join("absent/library");
    assert!(!missing.exists());
    assert!(
        core::core_census(&missing, &fixture.join("mir")).is_err(),
        "a census with no rust-src must refuse"
    );
}

/// Each library workflow's shards read rust-src before translating, and
/// its merge writes the register only when every shard succeeded.
#[test]
fn library_shards_read_their_sources_and_merge_only_whole() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for workflow in ["core.yml", "std.yml"] {
        let text = std::fs::read_to_string(root.join(".github/workflows").join(workflow))
            .expect("workflow");
        let translate = text
            .lines()
            .find(|l| l.contains("--only '$MODULE'"))
            .expect("the shard's census");
        let fetch = translate
            .find("scripts/mir/dump-lib.sh")
            .expect("rust-src fetched");
        assert!(fetch < translate.find("target/bin/uorust").expect("census"));
        let merge = &text[text.find("\n  merge:").expect("merge job")..];
        let refuse = merge
            .find("Refuse a partial set of shards")
            .expect("refusal");
        assert!(merge[refuse..].contains("needs.shard.result != 'success'"));
        assert!(refuse < merge.find("uorust merge").expect("merge step"));
        // The register lands through land-register.sh, which pushes it
        // with push.sh: both after the refusal.
        let land = merge
            .find("scripts/ci/land-register.sh")
            .expect("landing step");
        assert!(refuse < land);
        let script = std::fs::read_to_string(root.join("scripts/ci/land-register.sh"))
            .expect("land-register.sh");
        assert!(script.contains("scripts/ci/push.sh"));
    }
}

/// The witness of `crates/uorust/core/retracted.toml`'s row: a switch on
/// `cmp::Ordering`'s discriminant reads the discriminants core declares
/// (-1, 0, 1; a switch prints -1 as its `i8` bits, 255), not the variants'
/// indices. Read by index, the arm on 255 matched no variant and its call
/// was never lowered, so the body translated as if `Less` took the
/// otherwise arm (and `then_with`'s arm on 0 ran its closure on `Less`).
#[test]
fn a_switch_on_ordering_reads_its_declared_discriminants() {
    use uorust::mir::core;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ordering");
    let shards = core::core_census(&fixture.join("library"), &fixture.join("mir")).expect("census");
    let row = shards
        .iter()
        .flat_map(|s| &s.rows)
        .find(|r| r.path == "core::cmp::less_reaches")
        .expect("less_reaches");
    // `Less` reaches the call this fixture leaves undefined.
    assert!(row.digest.is_none(), "{row:?}");
    let cause = row.cause.as_deref().unwrap_or_default();
    assert!(cause.contains("absent"), "{cause}");
    let row = |path: &str| {
        shards
            .iter()
            .flat_map(|s| &s.rows)
            .find(|r| r.path == path)
            .unwrap_or_else(|| panic!("{path}"))
    };
    // An arm on each declared discriminant leaves the otherwise arm
    // unreached; read by index, `Greater` (2) fell to it.
    let every = row("core::cmp::every_arm");
    assert!(every.digest.is_some(), "{every:?}");
    // `Less`, the first variant, takes the otherwise arm, not the arm on 0.
    let equal = row("core::cmp::equal_arm");
    let cause = equal.cause.as_deref().unwrap_or_default();
    assert!(cause.contains("on_other"), "{cause}");
}

/// The witness of `crates/uorust/core/retracted.toml`'s `int_sqrt` rows:
/// rustc prints two constant items of `u32_stages`, `middle_stage!`'s
/// `HALF_BITS` (`u16::BITS >> 1`) and `last_stage!`'s (`u32::BITS >> 1`),
/// and every use of either, under one path; only the dumps' file names
/// tell them apart (`HALF_BITS#1`). The census took the first dump it read
/// for the path, `last_stage!`'s, so `middle_stage!`'s `u16`
/// `LOWER_HALF_1_BITS` read 16 where its own item is 8. A use of such a
/// path is now residue.
#[test]
fn a_constant_path_rustc_prints_for_two_items_is_refused() {
    use uorust::mir::core;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/twins");
    // The two items have other values: `u32`'s `BITS` and `u16`'s.
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixture.join("mir/num"))
        .expect("dumps")
        .map(|e| e.expect("dump").path())
        .filter(|p| p.to_string_lossy().contains("-HALF_BITS"))
        .collect();
    names.sort();
    let texts: Vec<String> = names
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("dump"))
        .collect();
    assert_eq!(texts.len(), 2);
    // The first read, the one the old census took, is `u32`'s; the `u16`
    // constant that uses it is `middle_stage!`'s, whose own is `u16`'s.
    assert!(
        texts[0].contains("const num::<impl u32>::BITS"),
        "{}",
        texts[0]
    );
    assert!(
        texts[1].contains("const num::<impl u16>::BITS"),
        "{}",
        texts[1]
    );
    let lower = std::fs::read_to_string(
        fixture.join("mir/num/core.num-imp-int_sqrt-u32_stages-LOWER_HALF_1_BITS.2-2-012.ElaborateBoxDerefs.before.mir"),
    )
    .expect("lower");
    assert!(lower.contains("const u32_stages::LOWER_HALF_1_BITS: u16"));
    let shards = core::core_census(&fixture.join("library"), &fixture.join("mir")).expect("census");
    let rows: Vec<&core::CoreRow> = shards.iter().flat_map(|s| &s.rows).collect();
    let half: Vec<&&core::CoreRow> = rows
        .iter()
        .filter(|r| r.path.contains("::HALF_BITS"))
        .collect();
    assert_eq!(half.len(), 2, "{rows:?}");
    assert!(half.iter().all(|r| r.digest.is_some()), "{half:?}");
    assert_ne!(
        half[0].digest, half[1].digest,
        "the two items' values differ"
    );
    let user = rows
        .iter()
        .find(|r| r.path.ends_with("::LOWER_HALF_1_BITS"))
        .expect("LOWER_HALF_1_BITS");
    let cause = user.cause.as_deref().unwrap_or_default();
    assert!(cause.contains("more than one constant item"), "{user:?}");
}

/// Every retraction of a library register names its body by a κ and path
/// the register holds, and its witness is a test of these tests, which
/// `cargo test` runs: a retraction whose witness fails is a red run.
#[test]
fn every_retraction_is_witnessed() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut tests = String::new();
    for entry in std::fs::read_dir(crate_dir.join("tests")).expect("tests") {
        let p = entry.expect("entry").path();
        if p.extension().is_some_and(|x| x == "rs") {
            tests.push_str(&std::fs::read_to_string(&p).expect("test source"));
        }
    }
    for register in ["core", "std"] {
        let dir = crate_dir.join(register);
        for r in uorust::queue::retractions(&dir).expect("retractions") {
            let test = format!("#[test]\nfn {}() {{", r.witness);
            assert!(tests.contains(&test), "{}: no test {}", r.path, r.witness);
            let held = std::fs::read_dir(&dir)
                .expect("register")
                .filter_map(Result::ok)
                .filter(|e| {
                    e.path()
                        .file_stem()
                        .is_some_and(|n| n != uorust::queue::RETRACTED)
                })
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .any(|t| t.contains(&format!("path = {:?}\nkappa = \"{}\"", r.path, r.kappa)));
            assert!(held, "{} is no row of the {register} register", r.path);
        }
    }
}

/// Core's register: each body of a module shard translated at the canonical
/// instantiation and recorded by κ; a body recorded as translated that is
/// residue later is a regression the register refuses.
#[test]
fn core_bodies_are_translated_into_a_monotone_register() {
    use uorust::mir::core;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core");
    let shards = core::core_census(&fixture.join("library"), &fixture.join("mir")).expect("census");
    assert_eq!(shards.len(), 1);
    let bool_shard = &shards[0];
    assert_eq!(bool_shard.module, "bool");
    let status = |path: &str| {
        bool_shard
            .rows
            .iter()
            .find(|r| r.path == path)
            .map(|r| r.digest.is_some())
    };
    // The impl is named by the type its source implements; a generic body
    // is translated with its parameters at the canonical type.
    assert_eq!(status("core::bool::bool::then_some"), Some(true));
    assert_eq!(status("core::bool::bool::ok_or"), Some(true));
    // `then` calls its closure parameter, which the canonical type is not.
    assert_eq!(status("core::bool::bool::then"), Some(false));
    let row = bool_shard
        .rows
        .iter()
        .find(|r| r.path == "core::bool::bool::then_some")
        .expect("then_some");
    assert_eq!(row.at, format!("T={}", core::CANONICAL));

    let dir = std::env::temp_dir().join(format!("uorust-core-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    core::write_core_register(&dir, &shards).expect("writes");
    let summary = std::fs::read_to_string(dir.join("frontier.toml")).expect("summary");
    assert!(summary.contains("[module.bool]\nbodies = 4\ntranslated = 2\nresidue = 2"));
    assert!(core::core_regressions(&dir, &shards).is_empty());
    // A register that records `then` as translated refuses the census that
    // leaves it residue.
    let rows = std::fs::read_to_string(dir.join("bool.toml")).expect("rows");
    let then = rows
        .split("\n\n")
        .find(|r| r.contains("path = \"core::bool::bool::then\""))
        .expect("then's row");
    let forged = then
        .lines()
        .take_while(|l| !l.starts_with("status"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\nstatus = \"translated\"\ndigest = \"0\"";
    std::fs::write(dir.join("bool.toml"), rows.replace(then, &forged)).expect("forges");
    let regressed = core::core_regressions(&dir, &shards);
    assert_eq!(regressed.len(), 1, "{regressed:?}");
    assert!(regressed[0].starts_with("core::bool::bool::then:"));
    // Retracted by its κ and path, it may be residue; under another path,
    // or with no witness, it still refuses.
    let kappa = then
        .lines()
        .find_map(|l| l.strip_prefix("kappa = "))
        .expect("then's kappa");
    let retract = |path: &str, witness: &str| {
        std::fs::write(
            dir.join("retracted.toml"),
            format!("[[retracted]]\npath = {path:?}\nkappa = {kappa}\nreason = \"r\"\nwitness = {witness:?}\n"),
        )
        .expect("retracts");
        core::core_regressions(&dir, &shards)
    };
    assert!(retract("core::bool::bool::then", "w").is_empty());
    assert_eq!(retract("core::bool::bool::then_some", "w").len(), 1);
    assert_eq!(retract("core::bool::bool::then", "").len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every body of the core fixture is embedded as a value of the MIR
/// datatype beside the semantics, with its census; dumps that do not say
/// the target they were built for are refused, since an integer's width
/// depends on it.
#[test]
fn every_core_body_is_embedded_ue_19() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core");
    let work = std::env::temp_dir().join(format!("uorust-embed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let out = work.join("out");
    let refused = uorust::embed::embed(
        "core",
        &fixture.join("library"),
        &fixture.join("mir"),
        &out,
        &[],
        &uorust::embed::Outputs::default(),
    );
    assert!(
        matches!(refused, Err(UorustError::Malformed { .. })),
        "{refused:?}"
    );
    // The fixture's dumps with the target the bodies were built for.
    let dumps = work.join("mir");
    std::fs::create_dir_all(dumps.join("bool")).expect("the dumps");
    for entry in std::fs::read_dir(fixture.join("mir/bool")).expect("the fixture") {
        let entry = entry.expect("a dump");
        std::fs::copy(entry.path(), dumps.join("bool").join(entry.file_name())).expect("copied");
    }
    std::fs::write(
        dumps.join("target.toml"),
        "triple = \"x86_64-unknown-linux-gnu\"\npointer_width = 64\n",
    )
    .expect("the target");
    let harness = work.join("harness");
    let census = uorust::embed::embed(
        "core",
        &fixture.join("library"),
        &dumps,
        &out,
        &[],
        &uorust::embed::Outputs {
            harness: Some(&harness),
            ..Default::default()
        },
    )
    .expect("the embedding");
    assert_eq!(census.bodies, 4);
    assert!(census.evaluable <= census.bodies);
    let rendered = census.render();
    assert!(rendered.contains("bodies = 4"), "{rendered}");
    let part =
        std::fs::read_to_string(out.join("src/CoreMir/BoolPart0.lex.tex")).expect("the bodies");
    assert_eq!(part.matches("result: Mir::Body").count(), 4);
    assert!(part.contains("\\importmodule{Mir}"));
    assert!(out.join("src/Mir.lex.tex").exists());
    let config = std::fs::read_to_string(out.join("lexlean.toml")).expect("the project");
    assert!(config.contains("\"src/CoreMir/BoolPart0.lex.tex\", \"src/Mir.lex.tex\""));
    assert_eq!(
        std::fs::read_to_string(out.join("census.toml")).expect("the census"),
        rendered
    );
    // No body of `bool` is a scalar method of an integer, so there is
    // nothing for a harness to call.
    assert!(!harness.exists());
    let _ = std::fs::remove_dir_all(&work);
}

/// Compiler inputs shared by the method-identity and bounded-proof regressions.
fn mir_method_fixture(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let work =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mir-method-homonyms-{case}"));
    let _ = std::fs::remove_dir_all(&work);
    let source = work.join("source");
    std::fs::create_dir_all(source.join("src")).expect("source");
    std::fs::write(
        source.join("Cargo.toml"),
        r#"[package]
name = "owners"
version = "0.0.0"
edition = "2021"
[workspace]
"#,
    )
    .expect("manifest");
    let mut code = String::new();
    for module in ["a", "b", "c"] {
        code.push_str(&format!("pub mod {module} {{ pub struct Token; impl Token {{ pub fn convert(mut value: u32) -> u32 {{ while value > 0 {{ value -= 1; }} value }} }} }}\n"));
    }
    std::fs::write(source.join("src/lib.rs"), code).expect("source bytes");
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&source)
        .output()
        .expect("lock fixture");
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    let dumps = work.join("all");
    uorust::mir::dump(&source, &dumps, &Default::default()).expect("actual compiler MIR");
    (work, source, dumps)
}

/// UE-28: every homonym retains its source-qualified name.
#[test]
fn mir_method_homonyms_have_stable_names_ue_28() {
    let (work, source, dumps) = mir_method_fixture("names");
    let candidates = |at: &Path| {
        let mut opts = options("owners");
        opts.mir = Some(mir_options(
            at.to_path_buf(),
            Default::default(),
            uorust::mir::Trial::All,
        ));
        let files = uorust::project_workspace(&source, &opts).expect("trial projection");
        let listing: toml::Value = text(&files, uorust::mir::CANDIDATES)
            .parse()
            .expect("candidates");
        listing
            .get("candidate")
            .and_then(toml::Value::as_array)
            .unwrap_or_else(|| panic!("no MIR candidates: {}", text(&files, "frontier.toml")))
            .iter()
            .map(|row| {
                (
                    row["function"].as_str().expect("function").to_string(),
                    row["decls"]
                        .as_array()
                        .expect("declarations")
                        .last()
                        .expect("body")
                        .as_str()
                        .expect("name")
                        .to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let all = candidates(&dumps);
    assert_eq!(all.len(), 3, "a conversion method was lost: {all:?}");
    assert_eq!(
        all.values()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    let selected = work.join("selected");
    std::fs::create_dir_all(selected.join("lib-owners")).expect("selected view");
    for name in ["configuration.toml", "layouts.toml"] {
        std::fs::copy(dumps.join(name), selected.join(name)).expect("compiler metadata");
    }
    for file in std::fs::read_dir(dumps.join("lib-owners")).expect("compiler bodies") {
        let file = file.expect("file").path();
        if file.extension().is_some_and(|e| e == "mir")
            && std::fs::read_to_string(&file)
                .expect("MIR")
                .contains("fn c::<impl at ")
        {
            std::fs::copy(
                &file,
                selected
                    .join("lib-owners")
                    .join(file.file_name().expect("name")),
            )
            .expect("exact compiler body");
        }
    }
    let one = candidates(&selected);
    assert_eq!(one.len(), 1, "the selected compiler view: {one:?}");
    assert_eq!(
        one["owners::c::Token::convert"],
        all["owners::c::Token::convert"]
    );
    std::fs::remove_dir_all(work).expect("cleanup");
}

/// UE-29: bounded plans preserve all work and dependency order.
#[test]
fn mir_proof_batches_keep_dependencies_and_refuse_invalid_budgets_ue_29() {
    use std::collections::BTreeSet;
    let graph = BTreeMap::from([
        ("leaf".to_string(), BTreeSet::new()),
        ("middle".to_string(), BTreeSet::from(["leaf".to_string()])),
        ("top".to_string(), BTreeSet::from(["middle".to_string()])),
        (
            "cycle-a".to_string(),
            BTreeSet::from(["cycle-b".to_string()]),
        ),
        (
            "cycle-b".to_string(),
            BTreeSet::from(["cycle-a".to_string()]),
        ),
    ]);
    let plan = uorust::mir::proof_batches(&graph, 2).expect("bounded plan");
    let mut done = BTreeSet::new();
    for batch in &plan {
        assert!(!batch.is_empty() && batch.len() <= 2);
        assert!(batch.is_disjoint(&done), "work was repeated");
        for node in batch {
            assert!(
                graph[node]
                    .iter()
                    .all(|dependency| done.contains(dependency) || batch.contains(dependency)),
                "a dependency was omitted"
            );
        }
        done.extend(batch.iter().cloned());
    }
    assert_eq!(done, graph.keys().cloned().collect());
    assert!(plan
        .iter()
        .any(|batch| batch.contains("cycle-a") && batch.contains("cycle-b")));
    for maximum in [0, 1] {
        assert!(matches!(
            uorust::mir::proof_batches(&graph, maximum),
            Err(UorustError::Tool(_))
        ));
    }
    let missing = BTreeMap::from([("caller".to_string(), BTreeSet::from(["absent".to_string()]))]);
    assert!(matches!(
        uorust::mir::proof_batches(&missing, 2),
        Err(UorustError::Tool(_))
    ));
    assert!(uorust::mir::proof_batches(&BTreeMap::new(), 2)
        .expect("no translations")
        .is_empty());
    let (work, source, dumps) = mir_method_fixture("batches");
    let verdicts = work.join("verdicts.toml");
    let scratch = work.join("trial");
    let proved =
        uorust::mir::prove_bounded(&source, &options("owners"), &dumps, &verdicts, &scratch, 1)
            .expect("actual SDK proofs");
    assert_eq!(proved.proved, 3);
    assert_eq!(proved.refused, 0);
    // Each single-function trial discovers its actual axiom set, then verifies it.
    assert_eq!(proved.runs, 6);
    let reused =
        uorust::mir::prove_bounded(&source, &options("owners"), &dumps, &verdicts, &scratch, 1)
            .expect("reuse verified digests");
    assert_eq!(reused.runs, 0, "unchanged proofs were repeated");
    std::fs::remove_dir_all(work).expect("cleanup");
}

/// UE-30: registry alias discovery cannot erase an admitted number model.
#[test]
fn registered_number_alias_keeps_its_admitted_model_ue_30() {
    let dir = scratch("registered-number-alias", &[
        ("Cargo.toml", "[package]\nname = \"number_alias\"\nversion = \"0.1.0\"\nedition = \"2021\"\nrust-version = \"1.97\"\n[dependencies]\nnum-bigint = \"=0.4.8\"\nnum-rational = \"=0.4.2\"\n"),
        ("src/lib.rs", "use num_rational::BigRational;\npub fn keep(r: BigRational) -> BigRational { r }\n"),
    ]);
    let locked = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("Cargo lock");
    assert!(
        locked.status.success(),
        "{}",
        String::from_utf8_lossy(&locked.stderr)
    );
    let lifted = |files: &BTreeMap<String, Vec<u8>>| {
        frontier_rows(files, "lifted")
            .iter()
            .any(|row| row["function"].as_str() == Some("number_alias::keep"))
    };
    assert!(
        lifted(&project(&dir, "number_alias")),
        "the registered alias lost its type model"
    );
    let lock = dir.join("Cargo.lock");
    let original = std::fs::read_to_string(&lock).expect("lock");
    let checksum = "f83d14da390562dca69fc84082e73e548e1ad308d24accdedd2720017cb37824";
    assert!(
        original.contains(checksum),
        "the fixture did not lock the registered archive"
    );
    std::fs::write(
        &lock,
        original.replace(checksum, &"0".repeat(checksum.len())),
    )
    .expect("planted wrong checksum");
    assert!(
        !lifted(&project(&dir, "number_alias")),
        "a different archive acquired the registered model"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-33: transport is modeled without assuming JSON storage equality.
#[test]
fn locked_json_carriers_preserve_data_and_refuse_unmodeled_equality_ue_33() {
    let dir = scratch("registered-json-carrier", &[
        ("Cargo.toml", "[package]\nname = \"json_carrier\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nserde_json = \"=1.0.114\"\n"),
        ("src/lib.rs", "use serde_json::{Value, Number};\n#[derive(PartialEq)] pub struct Held { pub payload: Value, pub id: u8 }\npub fn keep(v: Value) -> Value { v }\npub fn number(n: Number) -> Number { n }\npub fn pair(v: Value, n: u8) -> (Value, u8) { (v, n) }\npub fn id(h: &Held) -> u8 { h.id }\npub fn compared(a: Value, b: Value) -> bool { a == b }\npub fn numbers_equal(a: Number, b: Number) -> bool { a == b }\npub fn held_equal(a: Held, b: Held) -> bool { a == b }\n"),
    ]);
    let locked = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock");
    assert!(
        locked.status.success(),
        "{}",
        String::from_utf8_lossy(&locked.stderr)
    );
    let files = project(&dir, "json_carrier");
    let functions = |files: &BTreeMap<String, Vec<u8>>| -> Vec<String> {
        frontier_rows(files, "lifted")
            .iter()
            .filter_map(|row| row["function"].as_str().map(str::to_string))
            .collect()
    };
    let lifted = functions(&files);
    for function in [
        "json_carrier::keep",
        "json_carrier::number",
        "json_carrier::pair",
        "json_carrier::id",
    ] {
        assert!(
            lifted.iter().any(|name| name == function),
            "carrier function did not lift: {function}"
        );
    }
    assert!(
        !lifted.iter().any(|name| name == "json_carrier::compared"),
        "Storage equality was substituted for JSON equality"
    );
    for function in ["json_carrier::numbers_equal", "json_carrier::held_equal"] {
        assert!(
            !lifted.iter().any(|name| name == function),
            "Unmodeled JSON equality lifted: {function}"
        );
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write JSON projection");
    let kernel = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("actual SDK verifier");
    assert!(
        kernel.status.success(),
        "{}{}",
        String::from_utf8_lossy(&kernel.stdout),
        String::from_utf8_lossy(&kernel.stderr)
    );
    let lock = dir.join("Cargo.lock");
    let original = std::fs::read_to_string(&lock).expect("lock");
    let checksum = "c5f09b1bd632ef549eaa9f60a1f8de742bdbc698e6cee2095fc84dde5f549ae0";
    assert!(original.contains(checksum));
    std::fs::write(&lock, original.replace(checksum, &"0".repeat(64))).expect("wrong checksum");
    assert!(
        functions(&project(&dir, "json_carrier")).is_empty(),
        "An unregistered archive acquired the JSON model"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-34: an alias transports its reference, never a frozen cell value.
#[test]
fn shared_cell_references_transport_without_snapshotting_contents_ue_34() {
    let dir = scratch("shared-cell-references", &[
        ("Cargo.toml", "[package]\nname = \"shared_cells\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("src/lib.rs", "use std::{cell::{Cell, RefCell}, rc::Rc, sync::Arc};\n#[derive(PartialEq)] pub struct Held { pub first: Rc<RefCell<u8>>, pub second: Rc<RefCell<u8>>, pub id: u8 }\npub fn keep(x: Rc<RefCell<u8>>) -> Rc<RefCell<u8>> { x }\npub fn cloned(x: &Rc<RefCell<u8>>) -> Rc<RefCell<u8>> { Rc::clone(x) }\npub fn pair(x: Rc<RefCell<u8>>) -> (Rc<RefCell<u8>>, Rc<RefCell<u8>>) { (x.clone(), x) }\npub fn field(h: &Held) -> Rc<RefCell<u8>> { h.first.clone() }\npub fn id(h: &Held) -> u8 { h.id }\npub fn arc_keep(x: Arc<Cell<u8>>) -> Arc<Cell<u8>> { x }\npub fn read(x: &Rc<RefCell<u8>>) -> u8 { *x.borrow() }\npub fn write(x: &Rc<RefCell<u8>>, value: u8) { *x.borrow_mut() = value; }\npub fn make(value: u8) -> Rc<RefCell<u8>> { Rc::new(RefCell::new(value)) }\npub fn counts(x: &Rc<RefCell<u8>>) -> usize { Rc::strong_count(x) }\npub fn equal(x: Rc<RefCell<u8>>, y: Rc<RefCell<u8>>) -> bool { x == y }\npub fn held_equal(x: Held, y: Held) -> bool { x == y }\n"),
    ]);
    let files = project(&dir, "shared_cells");
    let lifted: Vec<String> = frontier_rows(&files, "lifted")
        .iter()
        .filter_map(|row| row["function"].as_str().map(str::to_string))
        .collect();
    for name in ["keep", "cloned", "pair", "field", "id", "arc_keep"] {
        assert!(
            lifted.contains(&format!("shared_cells::{name}")),
            "reference transport did not lift: {name}"
        );
    }
    for name in ["read", "write", "make", "counts", "equal", "held_equal"] {
        assert!(
            !lifted.contains(&format!("shared_cells::{name}")),
            "unmodeled shared heap operation lifted: {name}"
        );
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write reference projection");
    let kernel = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("actual SDK verifier");
    assert!(
        kernel.status.success(),
        "{}{}",
        String::from_utf8_lossy(&kernel.stdout),
        String::from_utf8_lossy(&kernel.stderr)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-35: wide field and recursive-proof records must survive SDK formatting.
#[test]
fn long_type_declarations_are_canonical_without_rewriting_ue_35() {
    let dir = scratch("wide-type-layout", &[
        ("Cargo.toml", "[package]\nname = \"wide_types\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("src/lib.rs", "pub struct PayloadWithAnIntentionallyLongQualifiedNameForCanonicalLayout { pub value: u8 }\npub struct Holder { pub overridesWithAnIntentionallyWideFieldName: Vec<PayloadWithAnIntentionallyLongQualifiedNameForCanonicalLayout> }\npub enum RecursiveTreeWithAnIntentionallyLongQualifiedNameForCanonicalLayout { End, Next(Box<RecursiveTreeWithAnIntentionallyLongQualifiedNameForCanonicalLayout>) }\npub fn keep(h: Holder) -> Holder { h }\npub fn depth(t: &RecursiveTreeWithAnIntentionallyLongQualifiedNameForCanonicalLayout) -> u32 { match t { RecursiveTreeWithAnIntentionallyLongQualifiedNameForCanonicalLayout::End => 0, RecursiveTreeWithAnIntentionallyLongQualifiedNameForCanonicalLayout::Next(rest) => depth(rest).saturating_add(1) } }\n"),
    ]);
    let files = project(&dir, "wide_types");
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write wide declarations");
    let check = std::process::Command::new("lexlean")
        .args(["fmt", "--check"])
        .current_dir(&out)
        .output()
        .expect("actual SDK formatter");
    assert!(
        check.status.success(),
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-36: an immutable slice owner retains the whole ordered sequence.
#[test]
fn owned_slices_retain_sequences_without_snapshotting_cells_ue_36() {
    let dir = scratch("owned-slices", &[
        ("Cargo.toml", "[package]\nname = \"owned_slices\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("src/lib.rs", "use std::{cell::Cell, rc::Rc, sync::Arc};\npub struct Held { pub order: Rc<[u8]> }\npub fn keep(x: Box<[u8]>) -> Box<[u8]> { x }\npub fn arc_keep(x: Arc<[u8]>) -> Arc<[u8]> { x }\npub fn len(x: &Held) -> usize { x.order.len() }\npub fn cloned(x: &Held) -> Rc<[u8]> { x.order.clone() }\npub fn counts(x: &Held) -> usize { Rc::strong_count(&x.order) }\npub fn cells(x: Rc<[Cell<u8>]>) -> Rc<[Cell<u8>]> { x }\n"),
    ]);
    let files = project(&dir, "owned_slices");
    let lifted: Vec<String> = frontier_rows(&files, "lifted")
        .iter()
        .filter_map(|r| r["function"].as_str().map(str::to_string))
        .collect();
    for name in ["keep", "arc_keep", "len", "cloned"] {
        assert!(
            lifted.contains(&format!("owned_slices::{name}")),
            "owned slice did not lift: {name}"
        );
    }
    for name in ["counts", "cells"] {
        assert!(
            !lifted.contains(&format!("owned_slices::{name}")),
            "unmodeled owner operation lifted: {name}"
        );
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write owned slices");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK verifier");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-37: default BitVec's logical sequence excludes capacity and dead bits.
#[test]
fn locked_bit_vectors_retain_logical_bits_without_raw_storage_ue_37() {
    let dir=scratch("locked-bit-vector", &[
        ("Cargo.toml","[package]\nname = \"logical_bits\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nbitvec = \"=1.0.1\"\n"),
        ("src/lib.rs","use bitvec::prelude::BitVec;\npub fn keep(x: BitVec) -> BitVec { x }\npub fn len(x: &BitVec) -> usize { x.len() }\npub fn capacity(x: &BitVec) -> usize { x.capacity() }\npub fn raw(x: &BitVec) -> &[usize] { x.as_raw_slice() }\n"),
    ]);
    let locked = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock bitvec");
    assert!(
        locked.status.success(),
        "{}",
        String::from_utf8_lossy(&locked.stderr)
    );
    let files = project(&dir, "logical_bits");
    let functions = |files: &BTreeMap<String, Vec<u8>>| -> Vec<String> {
        frontier_rows(files, "lifted")
            .iter()
            .filter_map(|r| r["function"].as_str().map(str::to_string))
            .collect()
    };
    let lifted = functions(&files);
    for name in ["keep", "len"] {
        assert!(
            lifted.contains(&format!("logical_bits::{name}")),
            "bit vector did not lift: {name}"
        );
    }
    for name in ["capacity", "raw"] {
        assert!(
            !lifted.contains(&format!("logical_bits::{name}")),
            "unmodeled storage operation lifted: {name}"
        );
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write bit projection");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK verifier");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let lock = dir.join("Cargo.lock");
    let source = std::fs::read_to_string(&lock).expect("lock text");
    let checksum = "1bc2832c24239b0141d5674bb9174f9d68a8b5b3f2753311927c172ca46f7e9c";
    assert!(source.contains(checksum));
    std::fs::write(&lock, source.replace(checksum, &"0".repeat(64))).expect("wrong checksum");
    assert!(
        functions(&project(&dir, "logical_bits")).is_empty(),
        "another archive acquired the bit-vector model"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-38: stale bytes never inherit proof, while observed policies save discovery.
#[test]
fn native_proof_refresh_reuses_policy_hints_but_verifies_new_bytes_ue_38() {
    let (work, source, dumps) = mir_method_fixture("policy-cache");
    let verdicts = work.join("verdicts.toml");
    let scratch = work.join("trial");
    let opts = options("owners");
    let first = uorust::mir::prove_bounded(&source, &opts, &dumps, &verdicts, &scratch, 3)
        .expect("initial SDK proofs");
    assert_eq!(first.proved, 3);
    assert_eq!(first.refused, 0);
    let mut stale = uorust::mir::verdict::Verdicts::read(&verdicts).expect("actual verdicts");
    let key = stale.rows.keys().next().expect("proved row").clone();
    let actual = stale.rows[&key].clone();
    assert!(
        !actual.axioms.is_empty(),
        "fixture has no observed policies"
    );
    stale.rows.get_mut(&key).expect("row").digest = "0".repeat(64);
    std::fs::write(&verdicts, stale.to_text()).expect("stale digest");
    let refreshed = uorust::mir::prove_bounded(&source, &opts, &dumps, &verdicts, &scratch, 3)
        .expect("SDK refresh");
    assert_eq!(refreshed.proved, 1);
    assert_eq!(refreshed.refused, 0);
    assert_eq!(
        refreshed.runs, 1,
        "the observed policies were needlessly rediscovered"
    );
    let fresh = uorust::mir::verdict::Verdicts::read(&verdicts).expect("fresh verdict");
    assert_eq!(fresh.rows[&key], actual);
    let replay =
        uorust::mir::prove_bounded(&source, &opts, &dumps, &verdicts, &scratch, 3).expect("replay");
    assert_eq!(replay.runs, 0);
    let mut wrong = fresh;
    let row = wrong.rows.get_mut(&key).expect("row");
    row.digest = "0".repeat(64);
    for policy in row.axioms.values_mut() {
        policy.clear();
    }
    std::fs::write(&verdicts, wrong.to_text()).expect("incorrect hints");
    let corrected = uorust::mir::prove_bounded(&source, &opts, &dumps, &verdicts, &scratch, 3)
        .expect("SDK correction");
    assert_eq!(corrected.proved, 1);
    assert_eq!(corrected.refused, 0);
    assert_eq!(corrected.runs, 2);
    let checked = uorust::mir::verdict::Verdicts::read(&verdicts).expect("corrected verdict");
    assert_eq!(checked.rows[&key], actual);
    std::fs::remove_dir_all(work).expect("cleanup");
}

/// UE-39: expanding an alias must not rebind its element names at the use site.
#[test]
fn aliases_keep_their_declaring_scope_through_nested_types_ue_39() {
    let dir=scratch("scoped-aliases", &[
        ("Cargo.toml","[package]\nname = \"scoped_aliases\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("src/lib.rs","pub mod left; pub mod right; pub mod holder;\n"),
        ("src/left.rs","use std::collections::BTreeMap;\npub struct Node { pub value: u8 }\npub type Pool = BTreeMap<String, Vec<(String, Vec<Node>)>>;\npub type Items<Node> = Vec<Node>;\npub type Nested = Pool;\npub struct Local { pub pool: Pool }\npub fn keep(x: Local) -> Local { x }\n"),
        ("src/right.rs","pub struct Node { pub flag: bool }\npub type Pool = Vec<Node>;\n"),
        ("src/holder.rs","use crate::left::{Nested as Pool, Items};\nuse crate::right::Node;\npub struct Held { pub pool: Pool, pub flags: Items<Node>, pub others: crate::right::Pool }\npub fn keep(x: Held) -> Held { x }\npub fn retained(x: Held) -> crate::left::Nested { x.pool }\npub fn unread(x: std::cell::UnsafeCell<u8>) -> std::cell::UnsafeCell<u8> { x }\n"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["check", "--offline"])
        .env("CARGO_TARGET_DIR", dir.join("rust-check"))
        .current_dir(&dir)
        .output()
        .expect("check source fixture");
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let files = project(&dir, "scoped_aliases");
    let lifted: Vec<String> = frontier_rows(&files, "lifted")
        .iter()
        .filter_map(|r| r["function"].as_str().map(str::to_string))
        .collect();
    for name in [
        "scoped_aliases::left::keep",
        "scoped_aliases::holder::keep",
        "scoped_aliases::holder::retained",
    ] {
        assert!(
            lifted.contains(&name.to_string()),
            "alias scope did not lift: {name}"
        );
    }
    assert!(!lifted.contains(&"scoped_aliases::holder::unread".to_string()));
    let behavior = text(&files, "src/Behavior.lex.tex");
    assert!(behavior.contains("LeftNode"), "{behavior}");
    assert!(behavior.contains("RightNode"), "{behavior}");
    assert!(
        behavior.contains("{name: flags, type: (List ScopedAliasesRightNode)}"),
        "generic alias parameter was rebound: {behavior}"
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write aliases");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK verifier");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-56: mathematical signed integers cross native word boundaries explicitly.
#[test]
fn native_signed_slots_use_explicit_scalar_codecs_ue_56() {
    let dir=scratch("native-signed-codecs",&[("Cargo.toml","[package]\nname=\"signed_codecs\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
pub struct Wide {pub value:i128}
pub struct Size {pub value:isize}
pub struct Fixed {pub value:i32}
pub fn identity128(value:i128)->i128 {value}
pub fn identity_size(value:isize)->isize {value}
pub fn read128(mut n:u8,value:&Wide)->i128 {while n>0 {n-=1;} value.value}
pub fn make128(mut n:u8,value:i128)->Wide {while n>0 {n-=1;} Wide {value}}
pub fn call128(mut n:u8,value:i128)->i128 {while n>0 {n-=1;} identity128(value)}
pub fn read_size(mut n:u8,value:&Size)->isize {while n>0 {n-=1;} value.value}
pub fn make_size(mut n:u8,value:isize)->Size {while n>0 {n-=1;} Size {value}}
pub fn call_size(mut n:u8,value:isize)->isize {while n>0 {n-=1;} identity_size(value)}
pub fn fixed(mut n:u8,value:&Fixed)->i32 {while n>0 {n-=1;} value.value}
#[test] fn reference() {for value in [i128::MIN,-1,0,i128::MAX] {assert_eq!(read128(2,&make128(2,value)),value);assert_eq!(call128(2,value),value);}for value in [isize::MIN,-1,0,isize::MAX] {assert_eq!(read_size(2,&make_size(2,value)),value);assert_eq!(call_size(2,value),value);}}
"#)]);
    let reference = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("Rust boundary oracle");
    assert!(reference.status.success());
    let dumps = dumped(&dir, "native-signed-codecs");
    let mut opts = options("signed_codecs");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).expect("proposals");
    let functions = [
        "read128",
        "make128",
        "call128",
        "read_size",
        "make_size",
        "call_size",
        "fixed",
    ];
    for f in functions {
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("signed_codecs::{f}\"")),
            "{f}: {:?}",
            residue_detail(&proposed, &format!("signed_codecs::{f}"))
        );
    }
    let register = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("signed-slot-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("signed_codecs"),
        &dumps,
        &register,
        &dir.join("trial"),
        7,
    )
    .expect("SDK proofs");
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&register);
    for f in functions {
        assert!(
            decisions
                .rows
                .values()
                .any(|r| r.function == format!("signed_codecs::{f}")
                    && matches!(r.status, uorust::mir::verdict::Decision::Proved)),
            "{f}"
        );
    }
    opts.mir = Some(mir_options(
        dumps.clone(),
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("proved projection");
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (suffix, bits, record) in [("128", 128, "Wide"), ("_size", usize::BITS, "Size")] {
        let sign = 1u128 << (bits - 1);
        let max = sign - 1;
        let mask = if bits == 128 {
            u128::MAX
        } else {
            (1u128 << bits) - 1
        };
        for (j, (raw, signed)) in [
            (sign, format!("-{sign}")),
            (mask, "-1".into()),
            (0, "0".into()),
            (max, max.to_string()),
        ]
        .iter()
        .enumerate()
        {
            for mode in ["read", "make", "call"] {
                let f = format!("{mode}{suffix}");
                let definition = rows
                    .iter()
                    .find(|r| r["function"].as_str() == Some(&format!("signed_codecs::{f}")))
                    .and_then(|r| r["definition"].as_str())
                    .unwrap()
                    .strip_prefix("Behavior::")
                    .unwrap();
                let (term, expected) = match mode {
                    "read" => (
                        format!("({definition} 2 (record {record} {{value := ({signed} : Int)}}))"),
                        raw.to_string(),
                    ),
                    "make" => (
                        format!("(project ({definition} 2 {raw}) value)"),
                        format!("({signed} : Int)"),
                    ),
                    _ => (format!("({definition} 2 {raw})"), raw.to_string()),
                };
                let policy = decisions
                    .rows
                    .values()
                    .find(|r| r.function == format!("signed_codecs::{f}"))
                    .and_then(|r| r.axioms.get(definition))
                    .cloned()
                    .unwrap_or_default();
                vectors.push_str(&format!("theorem{{ name: vector{bits}{mode}{j}, parameters: [], statement: ({term} = {expected}), proof: decide{{}}, axioms: [{}] }}\n",policy.join(", ")));
            }
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write");
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), body).unwrap();
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK vectors");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let source = dir.join("src/lib.rs");
    let original = std::fs::read_to_string(&source).unwrap();
    std::fs::write(
        &source,
        original.replace("pub value:i128", "pub value:bool"),
    )
    .unwrap();
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let bad = uorust::project_workspace(&dir, &opts).expect("typed incompatible source schema");
    assert!(residue_detail(&bad, "signed_codecs::read128")
        .is_some_and(|s| s.contains("field representation")));
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-58: unary numeric models cover extrema and preserve exact stored values.
#[test]
fn native_unary_numbers_share_width_and_registry_models_ue_58() {
    let dir=scratch("native-unary-numbers",&[("Cargo.toml","[package]\nname=\"unary_numbers\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nnum-bigint=\"=0.4.8\"\nnum-rational=\"=0.4.2\"\n[workspace]\n"),("src/lib.rs",r#"
use num_bigint::BigInt;
use num_rational::BigRational;
pub fn checked(mut n:u8,value:i128)->Option<i128> {while n>0 {n-=1;} value.checked_neg()}
pub fn magnitude(mut n:u8,value:i128)->u128 {while n>0 {n-=1;} value.unsigned_abs()}
pub fn wrapping(mut n:u8,value:i128)->i128 {while n>0 {n-=1;} value.wrapping_neg()}
pub fn sign(mut n:u8,value:i128)->i128 {while n>0 {n-=1;} value.signum()}
pub fn big_neg(mut n:u8,value:&BigInt)->BigInt {while n>0 {n-=1;} -value}
pub fn ratio_neg(mut n:u8,value:&BigRational)->BigRational {while n>0 {n-=1;} -value}
pub fn bad(mut n:u8,value:i128)->i128 {while n>0 {n-=1;} value.checked_neg().unwrap()}
#[test] fn reference() {for n in [-3,0,3] {let x=BigInt::from(n);assert_eq!(big_neg(2,&x),BigInt::from(-n));let r=BigRational::new_raw(x,BigInt::from(-6));let y=ratio_neg(2,&r);assert_eq!(y.numer(),&BigInt::from(-n));assert_eq!(y.denom(),&BigInt::from(-6));}}
"#)]);
    let reference = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("Rust exact unary vectors");
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let dumps = dumped(&dir, "native-unary-numbers");
    let mut opts = options("unary_numbers");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).expect("unary candidates");
    let functions = [
        "checked",
        "magnitude",
        "wrapping",
        "sign",
        "big_neg",
        "ratio_neg",
    ];
    for f in functions {
        let function = format!("unary_numbers::{f}");
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{function}\"")),
            "{function}: {:?}",
            residue_detail(&proposed, &function)
        );
    }
    let register = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("unary-number-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("unary_numbers"),
        &dumps,
        &register,
        &dir.join("trial"),
        8,
    )
    .expect("actual unary SDK decisions");
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&register);
    opts.mir = Some(mir_options(
        dumps.clone(),
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("proved unary projection");
    assert!(residue_detail(&files, "unary_numbers::bad").is_some());
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for f in functions {
        let function = format!("unary_numbers::{f}");
        let definition = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(function.as_str()))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == function)
            .and_then(|r| r.axioms.get(definition))
            .cloned()
            .unwrap_or_default();
        if matches!(f, "big_neg" | "ratio_neg") {
            for (j, n) in [-3i32, 0, 3].iter().enumerate() {
                let (argument, expected) = if f == "big_neg" {
                    (format!("({n} : Int)"), format!("({} : Int)", -n))
                } else {
                    (
                        format!("(({n} : Int), (-6 : Int))"),
                        format!("(({} : Int), (-6 : Int))", -n),
                    )
                };
                vectors.push_str(&format!("theorem{{name: vector{f}{j}, parameters: [], statement: (({definition} 2 {argument}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
            }
        } else {
            for (j, value) in [i128::MIN, -1, 0, i128::MAX].iter().enumerate() {
                let expected = match f {
                    "checked" => value.checked_neg().map_or_else(
                        || "(ctor Option.none [Nat])".into(),
                        |v| format!("(ctor Option.some [Nat] {})", v as u128),
                    ),
                    "magnitude" => value.unsigned_abs().to_string(),
                    "wrapping" => (value.wrapping_neg() as u128).to_string(),
                    _ => (value.signum() as u128).to_string(),
                };
                vectors.push_str(&format!("theorem{{name: vector{f}{j}, parameters: [], statement: (({definition} 2 {}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",*value as u128,policy.join(", ")));
            }
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write unary vectors");
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), body).unwrap();
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK unary vectors");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let raw = std::fs::read_dir(dumps.join("lib-unary_numbers"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .find(|p| std::fs::read_to_string(p).is_ok_and(|s| s.contains("fn checked(")))
        .expect("compiler-derived checked-negation body");
    let original = std::fs::read_to_string(&raw).unwrap();
    assert!(original.contains("core::num::<impl i128>::checked_neg"));
    std::fs::write(
        &raw,
        original.replace(
            "core::num::<impl i128>::checked_neg",
            "core::num::<impl u128>::checked_neg",
        ),
    )
    .unwrap();
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let bad = uorust::project_workspace(&dir, &opts).expect("typed mismatched primitive owner");
    assert!(
        residue_detail(&bad, "unary_numbers::checked").is_some_and(|s| s.contains("checked_neg"))
    );
    std::fs::write(raw, original).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-57: readonly owners share the source's content semantics, not identity semantics.
#[test]
fn native_readonly_owners_use_content_models_ue_57() {
    let dir=scratch("native-content-owners",&[("Cargo.toml","[package]\nname=\"content_owners\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
use std::rc::Rc;
use std::sync::{Arc,Mutex};
pub struct Wide {pub value:u32}
pub fn arc_read(mut n:u8,value:&Arc<Wide>)->u32 {while n>0 {n-=1;} value.value}
pub fn rc_read(mut n:u8,value:&Rc<Wide>)->u32 {while n>0 {n-=1;} value.value}
pub fn arc_make(mut n:u8,value:u32)->Arc<Wide> {while n>0 {n-=1;} Arc::new(Wide {value})}
pub fn rc_make(mut n:u8,value:u32)->Rc<Wide> {while n>0 {n-=1;} Rc::new(Wide {value})}
pub fn arc_copy(mut n:u8,value:&Arc<Wide>)->Arc<Wide> {while n>0 {n-=1;} value.clone()}
pub fn rc_copy(mut n:u8,value:&Rc<Wide>)->Rc<Wide> {while n>0 {n-=1;} value.clone()}
pub fn count(mut n:u8,value:&Arc<Wide>)->usize {while n>0 {n-=1;} Arc::strong_count(value)}
pub fn identity(mut n:u8,left:&Arc<Wide>,right:&Arc<Wide>)->bool {while n>0 {n-=1;} Arc::ptr_eq(left,right)}
pub fn cell(mut n:u8,value:&Arc<Mutex<u32>>)->u32 {while n>0 {n-=1;} *value.lock().unwrap()}
#[test] fn reference() {for value in [0,1,127,u32::MAX] {let a=arc_make(2,value);let r=rc_make(2,value);assert_eq!(arc_read(2,&arc_copy(2,&a)),value);assert_eq!(rc_read(2,&rc_copy(2,&r)),value);assert!(identity(2,&a,&a));assert_eq!(count(2,&a),1);}}
"#)]);
    let reference = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("Rust owner vectors");
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let dumps = dumped(&dir, "native-content-owners");
    let mut opts = options("content_owners");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).expect("owner candidates");
    for owner in ["arc", "rc"] {
        for mode in ["read", "make", "copy"] {
            let function = format!("content_owners::{owner}_{mode}");
            assert!(
                text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{function}\"")),
                "{function}: {:?}",
                residue_detail(&proposed, &function)
            );
        }
    }
    for function in ["count", "identity", "cell"] {
        assert!(residue_detail(&proposed, &format!("content_owners::{function}")).is_some());
    }
    let register = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("content-owner-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("content_owners"),
        &dumps,
        &register,
        &dir.join("trial"),
        6,
    )
    .expect("SDK owner proofs");
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&register);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("proved owner projection");
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for owner in ["arc", "rc"] {
        for mode in ["read", "make", "copy"] {
            let function = format!("content_owners::{owner}_{mode}");
            let definition = rows
                .iter()
                .find(|r| r["function"].as_str() == Some(function.as_str()))
                .and_then(|r| r["definition"].as_str())
                .unwrap()
                .strip_prefix("Behavior::")
                .unwrap();
            let policy = decisions
                .rows
                .values()
                .find(|r| r.function == function)
                .and_then(|r| r.axioms.get(definition))
                .cloned()
                .unwrap_or_default();
            for (j, value) in [0u32, 1, 127, u32::MAX].iter().enumerate() {
                let record = format!("(record Wide {{value := {value}}})");
                let term = match mode {
                    "read" => format!("({definition} 2 {record})"),
                    "make" => format!("(project ({definition} 2 {value}) value)"),
                    _ => format!("(project ({definition} 2 {record}) value)"),
                };
                vectors.push_str(&format!("theorem{{ name: vector{owner}{mode}{j}, parameters: [], statement: ({term} = {value}), proof: decide{{}}, axioms: [{}] }}\n",policy.join(", ")));
            }
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write owners");
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), body).unwrap();
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK owner vectors");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-55: compiler temporaries preserve matched enum values across reference copies.
#[test]
fn native_tuple_reference_matches_preserve_value_identity_ue_55() {
    let dir=scratch("native-match-aliases",&[("Cargo.toml","[package]\nname=\"match_aliases\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
#[derive(Clone, Copy)] pub enum Choice { A(bool), B }
pub fn paired(mut n:u8,left:&Choice,right:&Choice)->bool {while n>0 {n-=1;} match(left,right) {(Choice::A(a),Choice::A(b))=>*a==*b,_=>false}}
pub fn optional(mut n:u8,gate:bool,value:&Option<bool>)->bool {while n>0 {n-=1;} match(gate,value) {(true,Some(flag))=>*flag,_=>false}}
pub fn updated(mut n:u8,mut value:Choice)->bool {while n>0 {n-=1;} let before=match &value {Choice::A(flag)=>*flag,Choice::B=>false};value=Choice::A(!before);match(&value,&Choice::B) {(Choice::A(flag),_)=>*flag,_=>false}}
pub fn overflow(mut n:u8,left:&Choice,right:&Choice,x:u8)->u8 {while n>0 {n-=1;} match(left,right) {(Choice::A(_),Choice::A(_))=>x+1,_=>x}}
#[test] fn reference() {for a in [Choice::A(false),Choice::A(true),Choice::B] {for b in [Choice::A(false),Choice::A(true),Choice::B] {let expected=matches!((a,b),(Choice::A(false),Choice::A(false))|(Choice::A(true),Choice::A(true)));assert_eq!(paired(2,&a,&b),expected);}assert_eq!(updated(2,a),!matches!(a,Choice::A(true)));}assert!(optional(2,true,&Some(true)));assert!(!optional(2,true,&None));assert!(!optional(2,false,&Some(true)));}
"#)]);
    let reference = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("Rust vectors");
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let dumps = dumped(&dir, "native-match-aliases");
    let mut trial = options("match_aliases");
    trial.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &trial).expect("compiler-derived candidates");
    for function in ["paired", "optional", "updated"] {
        assert!(
            text(&proposed, uorust::mir::CANDIDATES)
                .contains(&format!("match_aliases::{function}\"")),
            "{function}: {:?}",
            residue_detail(&proposed, &format!("match_aliases::{function}"))
        );
    }
    let register = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("match-alias-verdicts.toml");
    let summary = uorust::mir::prove_bounded(
        &dir,
        &options("match_aliases"),
        &dumps,
        &register,
        &dir.join("trial"),
        4,
    )
    .expect("SDK decisions");
    let decisions = verdicts(&register);
    for f in ["paired", "optional", "updated"] {
        assert!(
            decisions
                .rows
                .values()
                .any(|r| r.function == format!("match_aliases::{f}")
                    && matches!(r.status, uorust::mir::verdict::Decision::Proved)),
            "{f}: {summary:?}"
        );
    }
    assert!(decisions
        .rows
        .values()
        .any(|r| r.function == "match_aliases::overflow"
            && matches!(r.status, uorust::mir::verdict::Decision::Refused(_))));
    let mut opts = options("match_aliases");
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("proved projection");
    let rows = frontier_rows(&files, "lifted");
    let mut model = String::new();
    for (i, (f, args, expected)) in [
        (
            "paired",
            "2 (ctor Choice.A true) (ctor Choice.A true)",
            true,
        ),
        (
            "paired",
            "2 (ctor Choice.A true) (ctor Choice.A false)",
            false,
        ),
        ("paired", "2 (ctor Choice.B) (ctor Choice.B)", false),
        ("optional", "2 true (ctor Option.some [Bool] true)", true),
        ("optional", "2 false (ctor Option.some [Bool] true)", false),
        ("optional", "2 true (ctor Option.none [Bool])", false),
        ("updated", "2 (ctor Choice.A true)", false),
        ("updated", "2 (ctor Choice.A false)", true),
        ("updated", "2 (ctor Choice.B)", true),
    ]
    .iter()
    .enumerate()
    {
        let definition = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&format!("match_aliases::{f}")))
            .and_then(|r| r["definition"].as_str())
            .expect("definition");
        let declaration = definition.strip_prefix("Behavior::").expect("namespace");
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == format!("match_aliases::{f}"))
            .and_then(|r| r.axioms.get(declaration))
            .cloned()
            .unwrap_or_default();
        model.push_str(&format!("theorem{{ name: vector{i}, parameters: [], statement: (({declaration} {args}) = {expected}), proof: decide{{}}, axioms: [{}] }}\n",policy.join(", ")));
    }

    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write");
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        text(&files, "src/Behavior.lex.tex").replace(
            "\\end{semanticmodule}",
            &format!("{model}\\end{{semanticmodule}}"),
        ),
    )
    .expect("vectors");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("actual SDK vectors");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let updated = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("match_aliases::updated"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let path = out.join("src/Behavior.lex.tex");
    let valid = std::fs::read_to_string(&path).expect("validated source");
    let defect=format!("theorem{{ name: plantedStaleBinding, parameters: [], statement: (({updated} 2 (ctor Choice.A true)) = true), proof: decide{{}} }}\n");
    std::fs::write(
        &path,
        valid.replace(
            "\\end{semanticmodule}",
            &format!("{defect}\\end{{semanticmodule}}"),
        ),
    )
    .expect("planted stale-value assertion");
    let rejected = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("negative SDK gate");
    let message = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success() && message.contains("LLV7002") && message.contains("is false"),
        "{message}"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-53: unrelated source assertions do not belong to a native trial's proof closure.
#[test]
fn native_trials_keep_only_reachable_obligations_ue_53() {
    let dir=scratch("native-proof-closure",&[("Cargo.toml","[package]\nname=\"proof_closure\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs","pub fn pass(mut n:u8,x:bool)->bool {while n>0 {n-=1;} x}\npub fn overflow(mut n:u8,x:u8)->u8 {while n>0 {n-=1;} x+1}\npub fn unrelated()->bool {false}\n#[test] fn unrelated_claim() {assert!(unrelated());}\n")]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock");
    assert!(lock.status.success());
    let dumps = dumped(&dir, "native-proof-closure");
    let register = dir.join("verdicts.toml");
    let trial = dir.join("trial");
    let summary = uorust::mir::prove_bounded(
        &dir,
        &options("proof_closure"),
        &dumps,
        &register,
        &trial,
        2,
    )
    .expect("actual scoped decisions");
    // The normal overflow model is refused; its explicit panic-outcome
    // model is separately proved. The frozen pre-catalogue producer gives
    // the same two positive decisions and one refusal.
    assert_eq!((summary.proved, summary.refused), (2, 1));
    assert_eq!(
        summary.projections, 2,
        "the refusal must refresh the catalogue once"
    );
    let scoped =
        std::fs::read_to_string(trial.join("src/Behavior.lex.tex")).expect("actual proof input");
    assert!(!scoped.contains("unrelated"));
    let mut opts = options("proof_closure");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&register),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("full projection");
    assert!(text(&files, "src/Behavior.lex.tex").contains("unrelated"));
    let out = dir.join("full");
    uorust::write(&out, &files).expect("full project");
    let full = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("full acceptance remains required");
    assert!(!full.status.success() && String::from_utf8_lossy(&full.stdout).contains("unrelated"));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-52: native callers cannot reuse a proof of a different source callee.
#[test]
fn native_proof_cache_binds_source_dependencies_ue_52() {
    let first="pub fn flag(x:bool)->bool { x }\npub fn caller(mut n:u8,x:bool)->bool {while n>0 {n-=1;} flag(x)}\n";
    let dir=scratch("native-context-cache",&[("Cargo.toml","[package]\nname=\"context_cache\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",first)]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock");
    assert!(lock.status.success());
    let dumps = dumped(&dir, "native-context-cache");
    let register = dir.join("verdicts.toml");
    let prove = || {
        uorust::mir::prove_bounded(
            &dir,
            &options("context_cache"),
            &dumps,
            &register,
            &dir.join("trial"),
            1,
        )
        .expect("actual context proof")
    };
    let before = prove();
    assert_eq!((before.proved, before.refused), (1, 0));
    let original = verdicts(&register);
    std::fs::write(dir.join("src/lib.rs"), first.replace("{ x }", "{ !x }"))
        .expect("changed callee only");
    let after = prove();
    assert!(
        after.runs > 0,
        "changed source dependency reused an old SDK decision"
    );
    assert_eq!((after.proved, after.refused), (1, 0));
    assert_ne!(
        original.rows.values().next().expect("old").digest,
        verdicts(&register)
            .rows
            .values()
            .next()
            .expect("fresh")
            .digest
    );
    let replay = prove();
    assert_eq!((replay.proved, replay.refused, replay.runs), (0, 0, 0));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-51: one binary conversion rule covers all primitive word widths.
#[test]
fn numeric_word_constructors_follow_widths_ue_51() {
    let types = [
        ("u8", u8::BITS, false),
        ("u16", u16::BITS, false),
        ("u32", u32::BITS, false),
        ("u64", u64::BITS, false),
        ("u128", u128::BITS, false),
        ("usize", usize::BITS, false),
        ("i8", i8::BITS, true),
        ("i16", i16::BITS, true),
        ("i32", i32::BITS, true),
        ("i64", i64::BITS, true),
        ("i128", i128::BITS, true),
        ("isize", isize::BITS, true),
    ];
    let mut code="use num_bigint::BigInt;\nuse num_rational::BigRational;\npub fn source_unsigned(x:u64)->BigInt {BigInt::from(x)}\n".to_string();
    for (ty, _, _) in types {
        code.push_str(&format!(
            "pub fn from_{ty}(mut n:u8,x:{ty})->BigInt {{while n>0 {{n-=1;}} BigInt::from(x)}}\n"
        ));
    }
    code.push_str("pub fn ratio(mut n:u8,x:BigInt)->BigRational {while n>0 {n-=1;} BigRational::from_integer(x)}\n");
    code.push_str("#[test] fn reference_boundaries() {\n");
    for (ty, _, _) in types {
        code.push_str(&format!("assert_eq!(from_{ty}(0,{ty}::MIN).to_string(),{ty}::MIN.to_string()); assert_eq!(from_{ty}(0,{ty}::MAX).to_string(),{ty}::MAX.to_string());\n"));
    }
    code.push_str("}\n");
    let dir=scratch("word-constructors",&[("Cargo.toml","[package]\nname=\"word_constructors\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n[dependencies]\nnum-bigint=\"=0.4.8\"\nnum-rational=\"=0.4.2\"\n"),("src/lib.rs",&code)]);
    let reference = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("Rust word boundary vectors");
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let files = uorust::project_workspace(&dir, &options("word_constructors"))
        .expect("source word construction");
    assert!(
        frontier_rows(&files, "lifted")
            .iter()
            .any(|r| r["function"].as_str() == Some("word_constructors::source_unsigned")),
        "source unsigned construction is not lifted"
    );
    let dumps = dumped(&dir, "word-constructors");
    let mut opts = options("word_constructors");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("native constructors");
    let listing = text(&files, uorust::mir::CANDIDATES);
    for (ty, _, _) in types {
        assert!(
            listing.contains(&format!("word_constructors::from_{ty}\"")),
            "missing width {ty}: {listing}"
        );
    }
    assert!(listing.contains("word_constructors::ratio\""));
    let register =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("word-constructor-verdicts.toml");
    let summary = uorust::mir::prove_bounded(
        &dir,
        &options("word_constructors"),
        &dumps,
        &register,
        &dir.join("trial"),
        8,
    )
    .expect("actual word proofs");
    assert_eq!(summary.refused, 0);
    let decisions = verdicts(&register);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("proved word constructors");
    let rows = frontier_rows(&files, "lifted");
    let name = |f: &str| {
        rows.iter()
            .find(|r| r["function"].as_str() == Some(&format!("word_constructors::{f}")))
            .and_then(|r| r["definition"].as_str())
            .expect("proved constructor")
            .to_string()
    };
    let mut vectors = Vec::new();
    for (ty, width, signed) in types {
        let mask = u128::MAX >> (u128::BITS - width);
        let half = 1u128 << (width - 1);
        let f = format!("from_{ty}");
        if signed {
            vectors.push((
                f.clone(),
                format!("0 {half}"),
                format!("(prim negate ({half} : Int) : Int)"),
            ));
            vectors.push((
                f.clone(),
                format!("0 {mask}"),
                "(prim negate (1 : Int) : Int)".to_string(),
            ));
            vectors.push((
                f,
                format!("0 {}", half - 1),
                format!("({} : Int)", half - 1),
            ));
        } else {
            vectors.push((f.clone(), "0 0".to_string(), "(0 : Int)".to_string()));
            vectors.push((f, format!("0 {mask}"), format!("({mask} : Int)")));
        }
    }
    vectors.push((
        "source_unsigned".into(),
        u64::MAX.to_string(),
        format!("({} : Int)", u64::MAX),
    ));
    vectors.push((
        "ratio".into(),
        "0 (prim negate (2 : Int) : Int)".into(),
        "((prim negate (2 : Int) : Int), (1 : Int))".into(),
    ));
    let mut model="\\begin{lexlean}{WordVectors}\n\\useglossary{uorust.titles@1.0.0}\n\\importmodule{Behavior}\n\\title{Lifted Rust behavior}\n\\begin{semanticmodule}\n".to_string();
    for (i, (f, args, expected)) in vectors.iter().enumerate() {
        let def = name(f);
        let decl = def.strip_prefix("Behavior::").expect("namespace");
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == format!("word_constructors::{f}"))
            .and_then(|r| r.axioms.get(decl))
            .cloned()
            .unwrap_or_default();
        model.push_str(&format!("theorem{{name: word{i}, parameters: [], statement: (({def} {args}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
    }
    model.push_str("\\end{semanticmodule}\n\\end{lexlean}\n");
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write constructors");
    std::fs::write(out.join("src/WordVectors.lex.tex"), model).expect("word vectors");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/WordVectors.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK word boundary proof");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-49: numeric predicates and accessors retain the locked crates' stored-value semantics.
#[test]
fn native_numeric_observations_follow_locked_values_ue_49() {
    let dir = scratch("native-numeric-observations", &[
        ("Cargo.toml", "[package]\nname = \"numeric_observations\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nnum-bigint = \"=0.4.8\"\nnum-rational = \"=0.4.2\"\nnum-traits = \"=0.2.19\"\n"),
        ("src/lib.rs", r#"
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Zero, One, Signed};
pub fn iz(mut n:u8,x:&BigInt)->bool { while n>0 {n-=1;} x.is_zero() }
pub fn io(mut n:u8,x:&BigInt)->bool { while n>0 {n-=1;} x.is_one() }
pub fn ip(mut n:u8,x:&BigInt)->bool { while n>0 {n-=1;} x.is_positive() }
pub fn ine(mut n:u8,x:&BigInt)->bool { while n>0 {n-=1;} x.is_negative() }
pub fn ia(mut n:u8,x:&BigInt)->BigInt { while n>0 {n-=1;} x.abs() }
pub fn isig(mut n:u8,x:&BigInt)->BigInt { while n>0 {n-=1;} x.signum() }
pub fn rz(mut n:u8,x:&BigRational)->bool { while n>0 {n-=1;} x.is_zero() }
pub fn ro(mut n:u8,x:&BigRational)->bool { while n>0 {n-=1;} x.is_one() }
pub fn rp(mut n:u8,x:&BigRational)->bool { while n>0 {n-=1;} x.is_positive() }
pub fn rn(mut n:u8,x:&BigRational)->bool { while n>0 {n-=1;} x.is_negative() }
pub fn ra(mut n:u8,x:&BigRational)->BigRational { while n>0 {n-=1;} x.abs() }
pub fn ri(mut n:u8,x:&BigRational)->bool { while n>0 {n-=1;} x.is_integer() }
pub fn numer(mut n:u8,x:&BigRational)->&BigInt { while n>0 {n-=1;} x.numer() }
pub fn denom(mut n:u8,x:&BigRational)->&BigInt { while n>0 {n-=1;} x.denom() }
pub fn izero(mut n:u8)->BigInt { while n>0 {n-=1;} BigInt::zero() }
pub fn ione(mut n:u8)->BigInt { while n>0 {n-=1;} BigInt::one() }
pub fn rzero(mut n:u8)->BigRational { while n>0 {n-=1;} BigRational::zero() }
pub fn rone(mut n:u8)->BigRational { while n>0 {n-=1;} BigRational::one() }
pub fn unsupported(mut n:u8,x:&BigInt)->u64 { while n>0 {n-=1;} x.bits() }
#[test] fn reference_raw_ratios() {
    let raw=BigRational::new_raw(BigInt::zero(),BigInt::zero()); assert!(raw.is_zero() && raw.is_one());
    let q=BigRational::new_raw(BigInt::from(2),BigInt::from(-2)); assert!(q.is_negative());
    assert_eq!((q.abs().numer().clone(),q.abs().denom().clone()),(BigInt::from(-2),BigInt::from(-2)));
}
"#),
    ]);
    let reference = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("locked Rust numeric vectors");
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let dumps = dumped(&dir, "native-numeric-observations");
    let mut opts = options("numeric_observations");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("numeric observations");
    let listing = text(&files, uorust::mir::CANDIDATES);
    let names = [
        "iz", "io", "ip", "ine", "ia", "isig", "rz", "ro", "rp", "rn", "ra", "ri", "numer",
        "denom", "izero", "ione", "rzero", "rone",
    ];
    for name in names {
        assert!(
            listing.contains(&format!("numeric_observations::{name}\"")),
            "missing numeric observation {name}: {listing}"
        );
    }
    assert!(!listing.contains("numeric_observations::unsupported\""));
    // Reuse actual digest-bound SDK decisions when only the surrounding
    // acceptance vectors change. UORust invalidates changed translations.
    let register =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("numeric-observation-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("numeric_observations"),
        &dumps,
        &register,
        &dir.join("trial"),
        8,
    )
    .expect("actual numeric SDK proofs");
    assert_eq!(proof.refused, 0);
    opts.mir = Some(mir_options(
        dumps.clone(),
        verdicts(&register),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("proved observations");
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write observations");
    let rows = frontier_rows(&files, "lifted");
    let named = |function: &str| {
        rows.iter()
            .find(|r| r["function"].as_str() == Some(&format!("numeric_observations::{function}")))
            .and_then(|r| r["definition"].as_str())
            .expect("proved definition")
            .to_string()
    };
    let vectors = [
        ("iz", "0 (0 : Int)", "true"),
        ("iz", "0 (2 : Int)", "false"),
        ("io", "0 (1 : Int)", "true"),
        ("ip", "0 (0 : Int)", "false"),
        ("ine", "0 (prim negate (2 : Int) : Int)", "true"),
        ("ia", "0 (prim negate (2 : Int) : Int)", "(2 : Int)"),
        (
            "isig",
            "0 (prim negate (2 : Int) : Int)",
            "(prim negate (1 : Int) : Int)",
        ),
        ("rz", "0 ((0 : Int), (0 : Int))", "true"),
        ("ro", "0 ((0 : Int), (0 : Int))", "true"),
        ("rn", "0 ((2 : Int), (prim negate (2 : Int) : Int))", "true"),
        (
            "rp",
            "0 ((prim negate (2 : Int) : Int), (prim negate (2 : Int) : Int))",
            "true",
        ),
        (
            "ra",
            "0 ((2 : Int), (prim negate (2 : Int) : Int))",
            "((prim negate (2 : Int) : Int), (prim negate (2 : Int) : Int))",
        ),
        ("ri", "0 ((2 : Int), (1 : Int))", "true"),
        ("numer", "0 ((2 : Int), (0 : Int))", "(2 : Int)"),
        ("denom", "0 ((2 : Int), (0 : Int))", "(0 : Int)"),
        ("izero", "0", "(0 : Int)"),
        ("ione", "0", "(1 : Int)"),
        ("rzero", "0", "((0 : Int), (1 : Int))"),
        ("rone", "0", "((1 : Int), (1 : Int))"),
    ];
    let mut model =
        "\\begin{lexlean}{NumericVectors}\n\\useglossary{uorust.titles@1.0.0}\n\\importmodule{Behavior}\n\\title{Lifted Rust behavior}\n\\begin{semanticmodule}\n"
            .to_string();
    let decisions = verdicts(&register);
    for (i, (f, args, expected)) in vectors.iter().enumerate() {
        let definition = named(f);
        let declaration = definition
            .strip_prefix("Behavior::")
            .expect("native namespace");
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == format!("numeric_observations::{f}"))
            .and_then(|r| r.axioms.get(declaration))
            .cloned()
            .unwrap_or_default();
        model.push_str(&format!("theorem{{ name: vector{i}, parameters: [], statement: (({definition} {args}) = {expected}), proof: decide{{}}, axioms: [{}] }}\n",policy.join(", ")));
    }
    model.push_str("\\end{semanticmodule}\n\\end{lexlean}\n");
    std::fs::write(out.join("src/NumericVectors.lex.tex"), model).expect("closed numeric vectors");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/NumericVectors.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("actual SDK numeric vectors");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let lock = dir.join("Cargo.lock");
    let original = std::fs::read_to_string(&lock).expect("lock");
    std::fs::write(
        &lock,
        original.replace(
            "c89e69e7e0f03bea5ef08013795c25018e101932225a656383bd384495ecc367",
            &"0".repeat(64),
        ),
    )
    .expect("wrong checksum");
    let refused = uorust::project_workspace(&dir, &opts).expect("checksum refusal");
    assert!(!frontier_rows(&refused, "lifted")
        .iter()
        .any(|r| r["function"].as_str() == Some("numeric_observations::iz")));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-48: early exits retain the updates already performed through mutable parameters.
#[test]
fn source_propagation_preserves_mutable_state_ue_48() {
    let dir = scratch("threaded-propagation", &[
        ("Cargo.toml", "[package]\nname = \"propagate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub struct State { pub flag: bool }
pub fn option(s: &mut State, x: Option<bool>) -> Option<bool> {
    s.flag = true; let y = x?; s.flag = y; Some(y)
}
pub fn result(s: &mut State, x: Result<bool, bool>) -> Result<bool, bool> {
    s.flag = true; let y = x?; s.flag = y; Ok(y)
}
pub fn callee(s: &mut State) -> Result<bool, bool> { s.flag = true; Err(false) }
pub fn caller(s: &mut State) -> Result<bool, bool> {
    let y = callee(s)?; s.flag = y; Ok(y)
}
pub fn both(a: &mut State, b: &mut State, x: Option<bool>) -> Option<bool> {
    a.flag = true; b.flag = true; let y = x?; a.flag = y; Some(y)
}
pub fn alias(s: &mut State, x: Option<bool>) -> Option<bool> {
    let other = s; other.flag = true; let y = x?; Some(y)
}
pub fn conversion(s: &mut State, x: Result<bool, bool>) -> Result<bool, u8> {
    s.flag = true; let y = x?; Ok(y)
}
pub fn option_failure() -> bool {
    let mut s = State { flag: false }; let r = option(&mut s, None); r == None && s.flag
}
pub fn option_success() -> bool {
    let mut s = State { flag: true }; let r = option(&mut s, Some(false)); r == Some(false) && !s.flag
}
pub fn result_failure() -> bool {
    let mut s = State { flag: false }; let r = result(&mut s, Err(false)); r == Err(false) && s.flag
}
pub fn result_success() -> bool {
    let mut s = State { flag: true }; let r = result(&mut s, Ok(false)); r == Ok(false) && !s.flag
}
pub fn callee_failure() -> bool {
    let mut s = State { flag: false }; let r = caller(&mut s); r == Err(false) && s.flag
}
pub fn two_states() -> bool {
    let mut a = State { flag: false }; let mut b = State { flag: false };
    let r = both(&mut a, &mut b, None); r == None && a.flag && b.flag
}
#[test] fn option_failure_vector() { assert!(option_failure()); }
#[test] fn option_success_vector() { assert!(option_success()); }
#[test] fn result_failure_vector() { assert!(result_failure()); }
#[test] fn result_success_vector() { assert!(result_success()); }
#[test] fn callee_failure_vector() { assert!(callee_failure()); }
#[test] fn two_states_vector() { assert!(two_states()); }
"#),
    ]);
    let reference = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("reference Rust vectors");
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let files = uorust::project_workspace(&dir, &options("propagate")).expect("source projection");
    let rows = frontier_rows(&files, "lifted");
    for name in [
        "option",
        "result",
        "callee",
        "caller",
        "both",
        "option_failure",
        "option_success",
        "result_failure",
        "result_success",
        "callee_failure",
        "two_states",
    ] {
        assert!(
            rows.iter()
                .any(|r| r["function"].as_str() == Some(&format!("propagate::{name}"))),
            "missing threaded propagation {name}: {}",
            text(&files, "frontier.toml")
        );
    }
    assert!(!rows
        .iter()
        .any(|r| r["function"].as_str() == Some("propagate::alias")));
    assert!(!rows
        .iter()
        .any(|r| r["function"].as_str() == Some("propagate::conversion")));
    let behavior = text(&files, "src/Behavior.lex.tex");
    assert!(
        behavior.matches("proof: decide").count() >= 6,
        "missing executable acceptance roots: {behavior}"
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write propagation projection");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("actual SDK propagation proof");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    // Plant loss of the update on the failure path. The same acceptance
    // roots must reject it; a merely well-typed tuple is not evidence.
    let marker = "item0 := (ctor Option.none [Bool]), item1 := s1";
    assert!(behavior.contains(marker), "missing failure-state boundary");
    let mut wrong = files.clone();
    wrong.insert(
        "src/Behavior.lex.tex".into(),
        behavior
            .replacen(marker, "item0 := (ctor Option.none [Bool]), item1 := s", 1)
            .into_bytes(),
    );
    let mutant = dir.join("lost-state");
    uorust::write(&mutant, &wrong).expect("write planted state-loss defect");
    let rejected = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&mutant)
        .output()
        .expect("SDK rejects lost state");
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("optionFailure")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-47: native workspaces use the same core-derived IEEE formats as source lifting.
#[test]
fn native_workspace_floats_preserve_root_names_ue_47() {
    let dir = scratch("native-root-float-name", &[
        ("Cargo.toml", "[package]\nname = \"float_shadow\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n"),
        ("src/lib.rs", "#[allow(non_camel_case_types)] pub struct f64 { pub flag: bool }\npub fn flag(mut n: u8, x: f64) -> bool { while n > 0 { n -= 1; } x.flag }\n"),
    ]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock");
    assert!(lock.status.success());
    let mut opts = options("float_shadow");
    opts.mir = Some(mir_options(
        dumped(&dir, "native-root-float-name"),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("root primitive-name declaration");
    assert!(
        text(&files, uorust::mir::CANDIDATES).contains("float_shadow::flag\""),
        "the root declaration was replaced by a primitive"
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write root declaration");
    let checked = std::process::Command::new("lexlean")
        .args([
            "check",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK type check");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-47: native workspaces use the same core-derived IEEE formats as source lifting.
#[test]
fn native_workspace_floats_share_source_readings_ue_47() {
    let dir = scratch("native-workspace-floats", &[
        ("Cargo.toml", "[package]\nname = \"native_floats\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n"),
        ("src/lib.rs", "pub fn id32(mut n: u8, x: f32) -> f32 { while n > 0 { n -= 1; } x }\npub fn id64(mut n: u8, x: f64) -> f64 { while n > 0 { n -= 1; } x }\npub fn bits32(mut n: u8, x: f32) -> u32 { while n > 0 { n -= 1; } x.to_bits() }\npub fn arithmetic64(mut n: u8, x: f64, y: f64) -> f64 { while n > 0 { n -= 1; } (x + y).abs() }\npub fn rebuilt64(mut n: u8, x: u64) -> f64 { while n > 0 { n -= 1; } f64::from_bits(x) }\npub fn negative_zero(mut n: u8) -> u64 { while n > 0 { n -= 1; } (-0.0f64).to_bits() }\npub fn finite64(mut n: u8, x: f64) -> bool { while n > 0 { n -= 1; } x.is_finite() }\npub fn unmodeled(mut n: u8, x: f64) -> f64 { while n > 0 { n -= 1; } x.sin() }\npub mod shadow { #[allow(non_camel_case_types)] pub struct f64 { pub flag: bool } pub fn transport(mut n: u8, x: f64) -> f64 { while n > 0 { n -= 1; } x } }\n"),
    ]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock");
    assert!(lock.status.success());
    let dumps = dumped(&dir, "native-workspace-floats");
    let mut opts = options("native_floats");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("native float projection");
    let listing = text(&files, uorust::mir::CANDIDATES);
    for name in [
        "id32",
        "id64",
        "bits32",
        "arithmetic64",
        "rebuilt64",
        "negative_zero",
        "finite64",
        "shadow::transport",
    ] {
        assert!(
            listing.contains(&format!("native_floats::{name}\"")),
            "missing native float {name}: {listing}"
        );
    }
    assert!(!listing.contains("native_floats::unmodeled\""));
    let behavior = text(&files, "src/Behavior.lex.tex");
    let rows = frontier_rows(&files, "lifted");
    for function in ["native_floats::id32", "native_floats::id64"] {
        let definition = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(function))
            .and_then(|r| r["definition"].as_str())
            .and_then(|d| d.strip_prefix("Behavior::"))
            .expect("primitive definition");
        let start = behavior
            .find(&format!("name: {definition},"))
            .expect("primitive header");
        let header = behavior[start..].split("body:").next().expect("header");
        assert!(
            header.contains("x : Nat") && header.contains("result: Nat"),
            "primitive was read as a workspace homonym: {header}"
        );
    }
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("native_floats"),
        &dumps,
        &dir.join("verdicts.toml"),
        &dir.join("trial"),
        8,
    )
    .expect("actual IEEE proofs");
    assert_eq!((proof.proved, proof.refused), (8, 0));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-65: import semantic type readings; never freeze a shared cell.
#[test]
fn native_type_models_import_source_readings_ue_65() {
    let dir=scratch("source-type-readings",&[("Cargo.toml","[package]\nname=\"type_readings\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
use std::rc::Rc;use std::sync::{Arc,Mutex};use std::cell::{RefCell,Cell};
pub fn rc(mut n:u8,x:Rc<RefCell<u32>>)->Rc<RefCell<u32>> {while n>0 {n-=1;} x}
pub fn arc(mut n:u8,x:Arc<Cell<u32>>)->Arc<Cell<u32>> {while n>0 {n-=1;} x.clone()}
pub fn some(mut n:u8,x:Option<Rc<RefCell<u32>>>)->bool {while n>0 {n-=1;} x.is_some()}
pub fn length(mut n:u8,x:&[Rc<RefCell<u32>>])->usize {while n>0 {n-=1;} x.len()}
pub fn read(mut n:u8,x:Rc<RefCell<u32>>)->u32 {while n>0 {n-=1;} *x.borrow()}
pub fn write(mut n:u8,x:Rc<RefCell<u32>>,v:u32) {while n>0 {n-=1;} *x.borrow_mut()=v;}
pub fn locked(mut n:u8,x:Arc<Mutex<u32>>)->u32 {while n>0 {n-=1;} *x.lock().unwrap()}
pub fn counts(mut n:u8,x:Rc<RefCell<u32>>)->usize {while n>0 {n-=1;} Rc::strong_count(&x)}
pub fn pointer_equal(mut n:u8,x:Rc<RefCell<u32>>,y:Rc<RefCell<u32>>)->bool {while n>0 {n-=1;} Rc::ptr_eq(&x,&y)}
"#),("src/bin/vectors.rs",r#"
use type_readings::*;use std::rc::Rc;use std::sync::Arc;use std::cell::{RefCell,Cell};
fn main(){let cell=Rc::new(RefCell::new(19));let _borrow=cell.borrow_mut();let atomic=Arc::new(Cell::new(23));for n in [0,2] {assert!(Rc::ptr_eq(&rc(n,cell.clone()),&cell));assert!(Arc::ptr_eq(&arc(n,atomic.clone()),&atomic));assert!(!some(n,None));assert!(some(n,Some(cell.clone())));assert_eq!(length(n,&[]),0);assert_eq!(length(n,&[cell.clone(),cell.clone()]),2);}}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "source-type-readings");
    let mut opts = options("type_readings");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for f in ["rc", "arc", "some", "length"] {
        let path = format!("type_readings::{f}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&files, &path)
        );
    }
    for f in ["read", "write", "locked", "counts", "pointer_equal"] {
        assert!(
            residue_detail(&files, &format!("type_readings::{f}")).is_some(),
            "{f}"
        );
    }
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("source-type-reading-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("type_readings"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for f in ["rc", "arc", "some", "length"] {
        let path = format!("type_readings::{f}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let allocation = if f == "arc" {
            "(ctor Prelude::ArcCellRef.Allocation 9)"
        } else {
            "(ctor Prelude::RcCellRef.Allocation 9)"
        };
        let examples = match f {
            "rc" | "arc" => vec![(allocation.to_string(), allocation.to_string())],
            "some" => vec![
                (
                    "(ctor Option.none [Prelude::RcCellRef])".into(),
                    "false".into(),
                ),
                (
                    format!("(ctor Option.some [Prelude::RcCellRef] {allocation})"),
                    "true".into(),
                ),
            ],
            _ => vec![
                ("(nil Prelude::RcCellRef)".into(), "0".into()),
                (
                    format!("(cons {allocation} (cons {allocation} (nil Prelude::RcCellRef)))"),
                    "2".into(),
                ),
            ],
        };
        for n in [0, 2] {
            for (i, (input, expected)) in examples.iter().enumerate() {
                // Opaque allocation references deliberately expose no decidable
                // equality. Transport is definitional identity, so use rfl.
                let proof = if matches!(f, "rc" | "arc") {
                    "reflexivity"
                } else {
                    "decide"
                };
                vectors.push_str(&format!("theorem{{name: typeModel{f}{n}Vector{i}, parameters: [], statement: (({name} {n} {input}) = {expected}), proof: {proof}{{}}, axioms: [{}]}}\n",policy.join(", ")));
            }
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = vectors.replacen("= true)", "= false)", 1);
    assert_ne!(vectors, wrong, "planted wrong container observation");
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&vectors, &wrong, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-64: callable values come from compiler bodies, with no operation recipes.
#[test]
fn immutable_compiler_closures_share_model_callables_ue_64() {
    let dir=scratch("compiler-model-callables",&[("Cargo.toml","[package]\nname=\"model_callables\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
pub fn flip(x:bool)->bool {!x}
pub fn predicate(mut n:u8,x:Option<bool>,expected:bool)->bool {while n>0 {n-=1;} x.is_some_and(|v|v==expected)}
pub fn mapped(mut n:u8,x:Option<bool>,negate:bool)->Option<bool> {while n>0 {n-=1;} x.map(|v|if negate {!v} else {v})}
pub fn lazy(mut n:u8,x:Option<bool>,fallback:bool)->bool {while n>0 {n-=1;} x.unwrap_or_else(||fallback)}
pub fn item(mut n:u8,x:Option<bool>)->bool {while n>0 {n-=1;} x.is_some_and(flip)}
pub fn mutable(mut n:u8,x:Option<bool>,state:&mut bool)->Option<bool> {while n>0 {n-=1;} x.map(|v|{*state=v;v})}
pub fn overflow(mut n:u8,x:Option<u8>)->Option<u8> {while n>0 {n-=1;} x.map(|v|v+1)}
"#),("src/bin/vectors.rs",r#"
use model_callables::*;
fn option(x:Option<bool>)->String {match x {None=>"(ctor Option.none [Bool])".into(),Some(v)=>format!("(ctor Option.some [Bool] {v})")}}
fn main(){assert!(std::panic::catch_unwind(||overflow(2,Some(255))).is_err());let mut state=false;assert_eq!(mutable(2,Some(true),&mut state),Some(true));assert!(state);for x in [None,Some(false),Some(true)] {let input=option(x);println!("item|2 {input}|{}",item(2,x));for y in [false,true] {println!("predicate|2 {input} {y}|{}",predicate(2,x,y));println!("mapped|2 {input} {y}|{}",option(mapped(2,x,y)));println!("lazy|2 {input} {y}|{}",lazy(2,x,y));}}}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 21);
    let dumps = dumped(&dir, "compiler-model-callables");
    let mut opts = options("model_callables");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    for f in ["predicate", "mapped", "lazy", "item"] {
        let path = format!("model_callables::{f}");
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    assert!(residue_detail(&proposed, "model_callables::mutable").is_some());
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("model-callable-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("model_callables"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert!(proof.refused <= 1, "{proof:?}");
    let decisions = verdicts(&cache);
    assert!(decisions
        .rows
        .values()
        .any(|row| row.function == "model_callables::overflow"
            && matches!(row.status, uorust::mir::verdict::Decision::Refused(_))));
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(residue_detail(&files, "model_callables::overflow").is_some());
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [function, input, expected] = parts.as_slice() else {
            panic!("vector {line}")
        };
        let path = format!("model_callables::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name: callableVector{i}, parameters: [], statement: (({name} {input}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = vectors.replacen("= false)", "= true)", 1);
    assert_ne!(vectors, wrong, "planted false callback result");
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&vectors, &wrong, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-64: one model bridge, exercised across unrelated standard receiver types.
/// Compiler-generated dependencies use actual closed bodies, not operation recipes.
#[test]
fn generated_dependencies_import_compiler_bodies_ue_85() {
    let dir = scratch("compiler-dependencies", &[
        ("Cargo.toml", "[package]\nname=\"compiler_dependencies\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
#[derive(Clone,PartialEq)] pub struct Value {pub flag:bool}
#[derive(Default)] pub enum Choice {#[default] First, Second}
pub mod nested {#[derive(Clone,PartialEq)] pub struct Value {pub flag:bool}}
pub fn copied(mut n:u8,x:&Value)->bool {while n>0 {n-=1;} x.clone().flag}
pub fn nested_copy(mut n:u8,x:&nested::Value)->bool {while n>0 {n-=1;} x.clone().flag}
pub fn equal(mut n:u8,x:&Value,y:&Value)->bool {while n>0 {n-=1;} <Value as PartialEq>::eq(x,y)}
pub fn first(mut n:u8)->bool {while n>0 {n-=1;} matches!(Choice::default(),Choice::First)}
"#),
        ("src/bin/vectors.rs", r#"
use compiler_dependencies::*;
fn main(){for x in [false,true] {
println!("copied|2 (record CompilerDependenciesValue {{flag := {x}}})|{}",copied(2,&Value{flag:x}));
println!("nested_copy|2 (record CompilerDependenciesNestedValue {{flag := {x}}})|{}",nested_copy(2,&nested::Value{flag:x}));
for y in [false,true] {println!("equal|2 (record CompilerDependenciesValue {{flag := {x}}}) (record CompilerDependenciesValue {{flag := {y}}})|{}",equal(2,&Value{flag:x},&Value{flag:y}));}
} println!("first|2|{}",first(2));}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    let dumps = dumped(&dir, "compiler-dependencies");
    let companion = dumps.with_extension("compiler-definitions.toml");
    let catalogue = std::fs::read(&companion).unwrap();
    uorust::mir::definitions::augment(&dir, &dumps).unwrap();
    assert_eq!(
        std::fs::read(&companion).unwrap(),
        catalogue,
        "same-input companion changed on capture-only replay"
    );
    let mut opts = options("compiler_dependencies");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    for function in ["copied", "nested_copy", "equal", "first"] {
        let path = format!("compiler_dependencies::{function}");
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    let cache = dir.join("verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("compiler_dependencies"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [function, input, expected] = parts.as_slice() else {
            panic!("{line}")
        };
        let path = format!("compiler_dependencies::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name: dependencyVector{i}, parameters: [], statement: (({name} {input}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
    }
    assert_eq!(reference.lines().count(), 9);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let defect = body.replacen("= false)", "= true)", 1);
    assert_ne!(body, defect);
    std::fs::write(&file, defect).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::write(&file, &body).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_calls_import_source_semantic_models_ue_64_ue_81_ue_82_ue_83() {
    let dir=scratch("source-model-import",&[("Cargo.toml","[package]\nname=\"source_models\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
pub fn fallback(mut n:u8,x:Option<u8>)->u8 {while n>0 {n-=1;} x.unwrap_or_default()}
pub fn error(mut n:u8,x:Result<bool,u8>)->Option<u8> {while n>0 {n-=1;} x.err()}
pub fn conditional(mut n:u8,x:bool,y:u8)->Option<u8> {while n>0 {n-=1;} x.then_some(y)}
pub fn ascii(mut n:u8,x:&str)->String {while n>0 {n-=1;} x.to_ascii_lowercase()}
pub fn panic_default(mut n:u8,x:Option<u8>)->u8 {while n>0 {n-=1;} x.unwrap()}
pub fn unicode(mut n:u8,x:&str)->String {while n>0 {n-=1;} x.to_lowercase()}
pub fn capacity(mut n:u8,x:usize)->Vec<u8> {while n>0 {n-=1;} Vec::with_capacity(x)}
pub fn empty_map(mut n:u8)->std::collections::BTreeMap<u8,u8> {while n>0 {n-=1;} std::collections::BTreeMap::new()}
pub enum Payload { Wrapped(u8) }
pub fn payload_len(mut n:u8,x:&[Payload])->usize {while n>0 {n-=1;} x.iter().len()}
pub fn reference_len(mut n:u8,x:&[std::rc::Rc<std::cell::RefCell<u8>>])->usize {while n>0 {n-=1;} x.iter().len()}
pub struct Accumulator { pub flag:bool }
impl Accumulator {pub fn update(&mut self,x:bool){self.flag=x;}}
pub fn last(mut n:u8,x:&[bool])->bool {while n>0 {n-=1;} let mut acc=Accumulator{flag:false};for item in x {acc.update(*item);}acc.flag}


"#),("src/bin/vectors.rs",r#"
use source_models::*;
fn option(x:Option<u8>)->String {match x {None=>"(ctor Option.none [Nat])".into(),Some(v)=>format!("(ctor Option.some [Nat] {v})")}}
fn main() {
 for x in [None,Some(0),Some(255)] {println!("fallback|2 {}|{}",option(x),fallback(2,x));}
 for x in [Ok(false),Ok(true),Err(0),Err(255)] {let term=match x {Ok(v)=>format!("(ctor Result.ok [Bool, Nat] {v})"),Err(e)=>format!("(ctor Result.error [Bool, Nat] {e})")};println!("error|2 {term}|{}",option(error(2,x)));}
 for x in [false,true] {for y in [0,255] {println!("conditional|2 {x} {y}|{}",option(conditional(2,x,y)));}}
 for x in ["","PRISM","XyZ","PRIΣM"] {println!("ascii|2 {x:?}|{:?}",ascii(2,x));}
 for x in [0,1,64] {assert!(capacity(2,x).is_empty());println!("capacity|2 {x}|(nil Nat)");}
 assert!(empty_map(2).is_empty());println!("empty_map|2|(nil TupleNatNat)");
 for xs in [vec![],vec![false],vec![true],vec![true,false,true]] {
  let mut term="(nil Bool)".to_string();
  for x in xs.iter().rev(){term=format!("(cons {x} {term})");}
  println!("last|2 {term}|{}",last(2,&xs));
 }
 for count in [0,1,3] {
  let payloads:Vec<_>=(0..count).map(|i|Payload::Wrapped(i as u8)).collect();
  let references:Vec<_>=(0..count).map(|i|std::rc::Rc::new(std::cell::RefCell::new(i as u8))).collect();
  let mut ps="(nil Payload)".to_string();
  let mut rs="(nil Prelude::RcCellRef)".to_string();
  for i in (0..count).rev() {
   ps=format!("(cons (ctor Payload.Wrapped {i}) {ps})");
   rs=format!("(cons (ctor Prelude::RcCellRef.Allocation {i}) {rs})");
  }
  println!("payload_len|2 {ps}|{}",payload_len(2,&payloads));
  println!("reference_len|2 {rs}|{}",reference_len(2,&references));
 }

}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 29);
    let dumps = dumped(&dir, "source-model-import");
    let mut opts = options("source_models");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    for f in [
        "fallback",
        "error",
        "conditional",
        "ascii",
        "capacity",
        "empty_map",
        "payload_len",
        "reference_len",
        "last",
    ] {
        let path = format!("source_models::{f}");
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    for f in ["panic_default", "unicode"] {
        assert!(
            residue_detail(&proposed, &format!("source_models::{f}")).is_some(),
            "{f}"
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("source-model-import-policy-retry-verdicts.toml");
    if cache.exists() {
        std::fs::remove_file(&cache).unwrap();
    }
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("source_models"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    assert!(
        proof.projection_reuses > 0,
        "SDK policy correction repeated whole-workspace analysis: {proof:?}"
    );
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [function, input, expected] = parts.as_slice() else {
            panic!("vector {line}")
        };
        let path = format!("source_models::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        if *function == "ascii" {
            // UTF-8 encoding is opaque to the kernel. Prove the bridge's
            // definitional equality to the existing source model instead of
            // pretending `decide` can evaluate that runtime primitive.
            let text = input.strip_prefix("2 ").unwrap();
            vectors.push_str(&format!("theorem{{name: importedVector{i}, parameters: [], statement: (({name} {input}) = (Prelude::textAsciiLower {text})), proof: reflexivity{{}}, axioms: [{}]}}\n",policy.join(", ")));
        } else {
            vectors.push_str(&format!("theorem{{name: importedVector{i}, parameters: [], statement: (({name} {input}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let incorrect_vectors = vectors.replacen("= 255)", "= 254)", 1);
    assert_ne!(vectors, incorrect_vectors, "planted false oracle vector");
    let defect = body.replacen(&vectors, &incorrect_vectors, 1);
    assert_ne!(body, defect, "incorrect imported-model output planted");
    std::fs::write(out.join("src/Behavior.lex.tex"), defect).unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-63: region composition is derived from compiler edges, not function names.
#[test]
fn compiler_regions_compose_nested_continuations_ue_63() {
    let dir=scratch("compiler-regions",&[("Cargo.toml","[package]\nname=\"compiler_regions\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
pub fn nested(mut n:u8,m:u8,mut v:bool)->bool {while n>0 {n-=1;let mut j=m;while j>0 {j-=1;v=!v;}}v}
pub fn triple(mut n:u8,m:u8,mut v:bool)->bool {while n>0 {n-=1;let mut j=m;while j>0 {j-=1;let mut k=m;while k>0 {k-=1;v=!v;}}}v}
pub fn early(mut n:u8,m:u8,mut v:bool)->bool {while n>0 {n-=1;let mut j=m;while j>0 {j-=1;if j==2 && v {return false;}v=!v;}}v}
pub fn exits(mut n:u8,m:u8,mut v:bool)->bool {'outer:while n>0 {n-=1;let mut j=m;while j>0 {j-=1;if j==1 {continue 'outer;}if j==2 {break;}v=!v;}}v}
pub fn overflow(mut n:u8,m:u8,mut value:u8)->u8 {while n>0 {n-=1;let mut j=m;while j>0 {j-=1;value+=1;}}value}
pub fn borrowed(mut n:u8,m:u8,value:&mut bool)->bool {while n>0 {n-=1;let reference=&mut *value;let mut j=m;while j>0 {j-=1;*reference=!*reference;}}*value}
"#),("src/bin/vectors.rs",r#"
use compiler_regions::*;
fn main(){for n in 0..=3 {for m in 0..=3 {for v in [false,true] {for (name,out) in [("nested",nested(n,m,v)),("triple",triple(n,m,v)),("early",early(n,m,v)),("exits",exits(n,m,v))] {println!("{name},{n},{m},{v},{out}");}}}}}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 128);
    let dumps = dumped(&dir, "compiler-regions");
    let mut opts = options("compiler_regions");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    for f in ["nested", "triple", "early", "exits"] {
        let path = format!("compiler_regions::{f}");
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    assert!(
        residue_detail(&proposed, "compiler_regions::borrowed").is_some(),
        "borrow state must not be snapshotted"
    );
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("compiler-region-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("compiler_regions"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert!(proof.refused <= 1, "{proof:?}");
    let decisions = verdicts(&cache);
    assert!(decisions
        .rows
        .values()
        .any(|row| row.function == "compiler_regions::overflow"
            && matches!(row.status, uorust::mir::verdict::Decision::Refused(_))));
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(residue_detail(&files, "compiler_regions::overflow").is_some());
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split(',').collect();
        let [function, n, m, input, expected] = parts.as_slice() else {
            panic!("oracle vector {line}")
        };
        let path = format!("compiler_regions::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name: regionVector{i}, parameters: [], statement: (({name} {n} {m} {input}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let sdk = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    assert!(
        sdk.status.success(),
        "{}{}",
        String::from_utf8_lossy(&sdk.stdout),
        String::from_utf8_lossy(&sdk.stderr)
    );
    let nested = rows
        .iter()
        .find(|row| row["function"].as_str() == Some("compiler_regions::nested"))
        .unwrap()["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let mut prefix = nested.to_string();
    prefix.replace_range(..1, &nested[..1].to_uppercase());
    let ctor = body
        .find(&format!("(ctor {prefix}Loop"))
        .expect("generated nested exit");
    let path_start = ctor + "(ctor ".len();
    let value_start = path_start + body[path_start..].find(' ').unwrap() + 1;
    assert!(
        body[value_start..].starts_with("(project "),
        "exit transports the Boolean state"
    );
    let value_end = value_start + body[value_start..].find(')').unwrap() + 1;
    let mut broken = body.clone();
    broken.replace_range(value_start..value_end, "false");
    assert_ne!(broken, body, "planted lost Boolean exit state");
    std::fs::write(out.join("src/Behavior.lex.tex"), broken).unwrap();
    let rejected = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-62: the delivery policy rejects regressions in the actual automation gate.
#[test]
fn factory_delivery_guard_enforces_structural_automation_ue_62() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let result = std::process::Command::new("python3")
        .args(["-I", "crates/uorust/delivery/test_guard.py"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

/// UE-61: function values reuse admitted signatures and standard content borrows.
#[test]
fn native_function_items_reuse_admitted_readings_ue_61() {
    let dir=scratch("function-items",&[("Cargo.toml","[package]\nname=\"function_items\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
use std::sync::Arc;
use std::rc::Rc;
use std::borrow::Borrow;
use std::ops::Deref;
pub fn flip(x:bool)->bool {!x}
pub fn flip_option(mut n:u8,x:Option<bool>)->Option<bool> {while n>0 {n-=1;} x.map(flip)}
pub fn arc_ref(mut n:u8,x:Option<&Arc<u32>>)->Option<&u32> {while n>0 {n-=1;} x.map(Arc::as_ref)}
pub fn rc_ref(mut n:u8,x:Option<&Rc<u32>>)->Option<&u32> {while n>0 {n-=1;} x.map(Rc::as_ref)}
pub fn arc_borrow(mut n:u8,x:Option<&Arc<u32>>)->Option<&u32> {while n>0 {n-=1;} x.map(<Arc<u32> as Borrow<u32>>::borrow)}
pub fn rc_deref(mut n:u8,x:Option<&Rc<u32>>)->Option<&u32> {while n>0 {n-=1;} x.map(<Rc<u32> as Deref>::deref)}
pub struct Custom(u32);
impl AsRef<u32> for Custom {fn as_ref(&self)->&u32 {&self.0}}
pub fn custom(mut n:u8,x:Option<&Custom>)->Option<&u32> {while n>0 {n-=1;} x.map(Custom::as_ref)}
pub fn change(x:&mut u32)->u32 {*x=2;*x}
pub fn mutable(mut n:u8,x:Option<&mut u32>)->Option<u32> {while n>0 {n-=1;} x.map(change)}
#[test] fn reference(){let arc=Arc::new(7);let rc=Rc::new(7);assert_eq!(arc_ref(2,Some(&arc)),Some(&7));assert_eq!(rc_ref(2,Some(&rc)),Some(&7));assert_eq!(arc_borrow(2,Some(&arc)),Some(&7));assert_eq!(rc_deref(2,Some(&rc)),Some(&7));assert_eq!(flip_option(2,Some(true)),Some(false));assert_eq!(arc_ref(2,None),None);assert_eq!(flip_option(2,None),None);}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "function-items");
    let mut opts = options("function_items");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    let functions = ["flip_option", "arc_ref", "rc_ref", "arc_borrow", "rc_deref"];
    for f in functions {
        let path = format!("function_items::{f}");
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    for f in ["custom", "mutable"] {
        assert!(
            residue_detail(&proposed, &format!("function_items::{f}")).is_some(),
            "{f}"
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("function-items-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("function_items"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for f in functions {
        let path = format!("function_items::{f}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        for (i, (input, expected)) in if f == "flip_option" {
            vec![
                ("(ctor Option.none [Bool])", "(ctor Option.none [Bool])"),
                (
                    "(ctor Option.some [Bool] false)",
                    "(ctor Option.some [Bool] true)",
                ),
                (
                    "(ctor Option.some [Bool] true)",
                    "(ctor Option.some [Bool] false)",
                ),
            ]
        } else {
            vec![
                ("(ctor Option.none [Nat])", "(ctor Option.none [Nat])"),
                ("(ctor Option.some [Nat] 0)", "(ctor Option.some [Nat] 0)"),
                (
                    "(ctor Option.some [Nat] 4294967295)",
                    "(ctor Option.some [Nat] 4294967295)",
                ),
            ]
        }
        .into_iter()
        .enumerate()
        {
            vectors.push_str(&format!("theorem{{name: vector{f}{i}, parameters: [], statement: (({name} 2 {input}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let sdk = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    assert!(
        sdk.status.success(),
        "{}{}",
        String::from_utf8_lossy(&sdk.stdout),
        String::from_utf8_lossy(&sdk.stderr)
    );
    let defect = body.replacen(
        "= (ctor Option.some [Nat] 4294967295)",
        "= (ctor Option.some [Nat] 4294967294)",
        1,
    );
    assert_ne!(defect, body, "planted incorrect borrow result");
    std::fs::write(out.join("src/Behavior.lex.tex"), defect).unwrap();
    let rejected = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-60: lookup is independent of a hash map's unmodeled iteration order.
#[test]
fn native_hash_map_reads_reuse_logical_lookup_models_ue_60() {
    let dir=scratch("hash-map-reads",&[("Cargo.toml","[package]\nname=\"hash_reads\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
use std::collections::HashMap;
use std::hash::{Hash,Hasher,BuildHasher};
#[derive(Clone,Eq,PartialEq,Ord,PartialOrd,Hash)] pub struct Key {pub id:u32}
pub fn source_less(a:&Key,b:&Key)->bool {a<b}
#[derive(Eq)] pub struct Parity(pub u8);
impl PartialEq for Parity {fn eq(&self,other:&Self)->bool {self.0%2==other.0%2}}
impl Hash for Parity {fn hash<H:Hasher>(&self,state:&mut H){(self.0%2).hash(state)}}
pub fn get_num<'a>(mut n:u8,map:&'a HashMap<u32,u8>,key:&u32)->Option<&'a u8> {while n>0 {n-=1;} map.get(key)}
pub fn has_num(mut n:u8,map:&HashMap<u32,u8>,key:&u32)->bool {while n>0 {n-=1;} map.contains_key(key)}
pub fn get_text<'a>(mut n:u8,map:&'a HashMap<String,u8>,key:&str)->Option<&'a u8> {while n>0 {n-=1;} map.get(key)}
pub fn has_text(mut n:u8,map:&HashMap<String,u8>,key:&str)->bool {while n>0 {n-=1;} map.contains_key(key)}
pub fn get_key<'a>(mut n:u8,map:&'a HashMap<Key,u8>,key:&Key)->Option<&'a u8> {while n>0 {n-=1;} map.get(key)}
pub fn has_key(mut n:u8,map:&HashMap<Key,u8>,key:&Key)->bool {while n>0 {n-=1;} map.contains_key(key)}
pub fn sizes(mut n:u8,map:&HashMap<u32,u8>)->bool {while n>0 {n-=1;} map.is_empty()==(map.len()==0)}
pub fn custom<'a>(mut n:u8,map:&'a HashMap<Parity,u8>,key:&Parity)->Option<&'a u8> {while n>0 {n-=1;} map.get(key)}
pub fn observe(mut n:u8,map:&HashMap<u32,u8>)->u64 {while n>0 {n-=1;} map.hasher().build_hasher().finish()}
pub fn iteration(mut n:u8,map:&HashMap<u32,u8>)->Option<&u32> {while n>0 {n-=1;} map.keys().next()}
#[test] fn reference(){for pairs in [[(3,7),(1,4)],[(1,4),(3,7)]] {let map=HashMap::from(pairs);assert_eq!(get_num(2,&map,&3),Some(&7));assert_eq!(get_num(2,&map,&2),None);assert!(has_num(2,&map,&3));assert!(!has_num(2,&map,&2));assert!(sizes(2,&map));let named=map.iter().map(|(k,v)|(Key{id:*k},*v)).collect();assert_eq!(get_key(2,&named,&Key{id:3}),Some(&7));assert!(!has_key(2,&named,&Key{id:2}));} let text=HashMap::from([("z".to_string(),7),("a".to_string(),4)]);assert_eq!(get_text(2,&text,"z"),Some(&7));assert!(!has_text(2,&text,"missing"));assert!(sizes(2,&HashMap::new()));}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["test", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("Rust lookup vectors");
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "hash-map-reads");
    let mut opts = options("hash_reads");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).expect("lookup candidates");
    let functions = [
        "get_num", "has_num", "get_text", "has_text", "get_key", "has_key", "sizes",
    ];
    for f in functions {
        let path = format!("hash_reads::{f}");
        assert!(
            text(&proposed, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    for f in ["custom", "observe", "iteration"] {
        assert!(
            residue_detail(&proposed, &format!("hash_reads::{f}")).is_some(),
            "{f}"
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hash-read-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("hash_reads"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .expect("SDK lookup proofs");
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("proved lookup projection");
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for f in functions {
        let path = format!("hash_reads::{f}");
        let definition = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(path.as_str()))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(definition))
            .cloned()
            .unwrap_or_default();
        for swapped in [false, true] {
            for present in [false, true] {
                let named = f.ends_with("_key");
                let text_key = f.ends_with("_text");
                let entry = if named {
                    "TupleKeyNat"
                } else if text_key {
                    "TupleStringNat"
                } else {
                    "TupleNatNat"
                };
                let key = |v: u32| {
                    if named {
                        format!("(record Key {{idValue := {v}}})")
                    } else if text_key {
                        format!(
                            "\"{}\"",
                            if v == 3 {
                                "z"
                            } else if v == 1 {
                                "a"
                            } else {
                                "missing"
                            }
                        )
                    } else {
                        v.to_string()
                    }
                };
                let pairs = if swapped {
                    [(1, 4), (3, 7)]
                } else {
                    [(3, 7), (1, 4)]
                };
                let mut map = format!("(nil {entry})");
                for (k, v) in pairs.iter().rev() {
                    map = format!(
                        "(cons (record {entry} {{item0 := {}, item1 := {v}}}) {map})",
                        key(*k)
                    );
                }
                let (term, expected) = if f == "sizes" {
                    let argument = if present {
                        map
                    } else {
                        format!("(nil {entry})")
                    };
                    (format!("({definition} 2 {argument})"), "true".into())
                } else {
                    let expected = if f.starts_with("get_") {
                        if present {
                            "(ctor Option.some [Nat] 7)"
                        } else {
                            "(ctor Option.none [Nat])"
                        }
                    } else if present {
                        "true"
                    } else {
                        "false"
                    };
                    (
                        format!(
                            "({definition} 2 {map} {})",
                            key(if present { 3 } else { 2 })
                        ),
                        expected.to_string(),
                    )
                };
                vectors.push_str(&format!("theorem{{name: vector{f}{}{}, parameters: [], statement: ({term} = {expected}), proof: decide{{}}, axioms: [{}]}}\n",u8::from(swapped),u8::from(present),policy.join(", ")));
            }
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write vectors");
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let sdk = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK lookup vectors");
    assert!(
        sdk.status.success(),
        "{}{}",
        String::from_utf8_lossy(&sdk.stdout),
        String::from_utf8_lossy(&sdk.stderr)
    );
    let defect = body.replace(
        "= (ctor Option.some [Nat] 7)",
        "= (ctor Option.some [Nat] 6)",
    );
    assert_ne!(defect, body, "planted incorrect lookup result");
    std::fs::write(out.join("src/Behavior.lex.tex"), defect).unwrap();
    let rejected = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK rejects incorrect lookup vector");
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-59: a missing dependency is not a newly verifiable translation.
#[test]
fn blocked_partial_callers_preserve_complete_context_verdicts_ue_59() {
    let dir=scratch("blocked-history",&[("Cargo.toml","[package]\nname=\"blocked_history\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs","pub fn leaf(mut n:u8)->u8 {while n>0 {n-=1;} n}\npub fn middle(mut n:u8,value:u8)->u8 {while n>0 {n-=1;} leaf(value)}\npub fn caller(mut n:u8,value:u8)->u8 {while n>0 {n-=1;} middle(n,value)}\n")]);
    assert!(std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .status()
        .unwrap()
        .success());
    let all = dumped(&dir, "blocked-history-three");
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("blocked-history-three");
    std::fs::create_dir_all(&work).unwrap();
    let register = work.join("verdicts.toml");
    let complete = work.join("complete-verdicts.toml");
    if complete.exists() {
        std::fs::copy(&complete, &register).unwrap();
    }
    let full = uorust::mir::prove_bounded(
        &dir,
        &options("blocked_history"),
        &all,
        &register,
        &work.join("trial"),
        3,
    )
    .expect("actual full-context SDK proofs");
    assert_eq!(full.refused, 0);
    let before = verdicts(&register);
    assert_eq!(before.rows.len(), 3);
    assert!(before
        .rows
        .values()
        .all(|r| matches!(r.status, uorust::mir::verdict::Decision::Proved)));
    std::fs::copy(&register, &complete).unwrap();
    let selected = work.join("selected");
    if selected.exists() {
        std::fs::remove_dir_all(&selected).unwrap();
    }
    std::fs::create_dir_all(selected.join("lib-blocked_history")).unwrap();
    for name in ["configuration.toml", "layouts.toml"] {
        std::fs::copy(all.join(name), selected.join(name)).unwrap();
    }
    let mut copied = 0;
    for entry in std::fs::read_dir(all.join("lib-blocked_history")).unwrap() {
        let p = entry.unwrap().path();
        if std::fs::read_to_string(&p)
            .is_ok_and(|s| s.contains("fn caller(") || s.contains("fn middle("))
        {
            std::fs::copy(
                &p,
                selected
                    .join("lib-blocked_history")
                    .join(p.file_name().unwrap()),
            )
            .unwrap();
            copied += 1;
        }
    }
    assert_eq!(copied, 2);
    let mut opts = options("blocked_history");
    opts.mir = Some(mir_options(
        selected.clone(),
        before.clone(),
        uorust::mir::Trial::All,
    ));
    let partial = uorust::project_workspace(&dir, &opts).expect("blocked partial projection");
    let candidates: toml::Value = text(&partial, uorust::mir::CANDIDATES).parse().unwrap();
    let caller = candidates["candidate"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["function"].as_str() == Some("blocked_history::caller"))
        .unwrap();
    assert_eq!(caller["trial"].as_bool(), Some(false));
    let kappa = caller["kappa"].as_str().unwrap();
    assert_ne!(
        caller["digest"].as_str().unwrap(),
        before.rows[kappa].digest
    );
    assert!(residue_detail(&partial, "blocked_history::caller").is_some());
    let narrowed = uorust::mir::prove_bounded(
        &dir,
        &options("blocked_history"),
        &selected,
        &register,
        &work.join("trial"),
        3,
    )
    .expect("partial cache replay");
    assert_eq!(
        (narrowed.proved, narrowed.refused, narrowed.runs),
        (0, 0, 0)
    );
    assert_eq!(
        verdicts(&register).rows,
        before.rows,
        "blocked context erased a reusable complete-context proof"
    );
    let replay = uorust::mir::prove_bounded(
        &dir,
        &options("blocked_history"),
        &all,
        &register,
        &work.join("trial"),
        3,
    )
    .expect("restored complete replay");
    assert_eq!((replay.proved, replay.refused, replay.runs), (0, 0, 0));
    assert_eq!(verdicts(&register).rows, before.rows);
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-46: narrowing the compiler view must not destroy reusable SDK decisions.
#[test]
fn native_verdicts_survive_compiler_input_selections_ue_46() {
    let source = mir_fixture();
    let all = dumped(&source, "walk");
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-selection-history");
    let _ = std::fs::remove_dir_all(&work);
    let selected = work.join("selected");
    std::fs::create_dir_all(selected.join("lib-walk")).expect("selected input");
    for name in ["configuration.toml", "layouts.toml"] {
        std::fs::copy(all.join(name), selected.join(name)).expect("compiler metadata");
    }
    let mut count = 0;
    for entry in std::fs::read_dir(all.join("lib-walk")).expect("actual compiler bodies") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|x| x == "mir")
            && std::fs::read_to_string(&path)
                .expect("body")
                .contains("fn through_raw(")
        {
            std::fs::copy(
                &path,
                selected
                    .join("lib-walk")
                    .join(path.file_name().expect("name")),
            )
            .expect("exact compiler bytes");
            count += 1;
        }
    }
    assert_eq!(count, 1);
    let register = work.join("verdicts.toml");
    std::fs::copy(source.join("lift-verdicts.toml"), &register)
        .expect("actual committed SDK verdicts");
    let before = verdicts(&register);
    assert!(before
        .rows
        .values()
        .any(|r| matches!(r.status, uorust::mir::verdict::Decision::Refused(_))));
    for inputs in [&all, &selected, &all] {
        let replay = uorust::mir::prove_bounded(
            &source,
            &options("walk"),
            inputs,
            &register,
            &work.join("trial"),
            16,
        )
        .expect("cached selection replay");
        assert_eq!((replay.proved, replay.refused, replay.runs), (0, 0, 0));
        assert_eq!(
            verdicts(&register).rows,
            before.rows,
            "selection discarded reusable proof history"
        );
    }
    std::fs::remove_dir_all(work).expect("cleanup");
}

/// UE-45: transport does not inspect a hash builder or invent iteration order.
#[test]
fn native_hash_collections_reuse_unordered_carriers_ue_45() {
    let dir = scratch("native-hash-carriers", &[
        ("Cargo.toml", "[package]\nname = \"hash_carriers\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n"),
        ("src/lib.rs", "use std::collections::{HashMap, HashSet};\npub struct Seed { pub state: u64 }\npub fn map(mut n: u8, x: HashMap<u8, u16>) -> HashMap<u8, u16> { while n > 0 { n -= 1; } x }\npub fn set(mut n: u8, x: HashSet<u8>) -> HashSet<u8> { while n > 0 { n -= 1; } x }\npub fn custom_map(mut n: u8, x: HashMap<u8, u16, Seed>) -> HashMap<u8, u16, Seed> { while n > 0 { n -= 1; } x }\npub fn custom_set(mut n: u8, x: HashSet<u8, Seed>) -> HashSet<u8, Seed> { while n > 0 { n -= 1; } x }\npub fn inspect(mut n: u8, x: HashMap<u8, u16, Seed>) -> u64 { while n > 0 { n -= 1; } x.hasher().state }\npub fn ordered(mut n: u8, x: HashSet<u8>) -> Vec<u8> { while n > 0 { n -= 1; } x.into_iter().collect() }\npub fn unread(mut n: u8, x: HashMap<u8, std::cell::UnsafeCell<u8>, Seed>) -> HashMap<u8, std::cell::UnsafeCell<u8>, Seed> { while n > 0 { n -= 1; } x }\npub mod shadow { pub struct HashSet { pub flag: bool } pub fn transport(mut n: u8, x: HashSet) -> HashSet { while n > 0 { n -= 1; } x } }\n"),
    ]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock source");
    assert!(lock.status.success());
    let dumps = dumped(&dir, "native-hash-carriers");
    let mut opts = options("hash_carriers");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("collection projection");
    let listing = text(&files, uorust::mir::CANDIDATES);
    for name in [
        "map",
        "set",
        "custom_map",
        "custom_set",
        "shadow::transport",
    ] {
        assert!(
            listing.contains(&format!("hash_carriers::{name}\"")),
            "missing native collection {name}: {listing}"
        );
    }
    for name in ["inspect", "ordered", "unread"] {
        assert!(
            !listing.contains(&format!("hash_carriers::{name}\"")),
            "unmodeled observation was admitted: {name}"
        );
    }
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("hash_carriers"),
        &dumps,
        &dir.join("verdicts.toml"),
        &dir.join("trial"),
        5,
    )
    .expect("actual collection proofs");
    assert_eq!((proof.proved, proof.refused), (5, 0));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-44: one changing state keeps loop invariants in the termination context.
#[test]
fn native_loop_state_preserves_invariants_in_evidence_ue_44() {
    let dir = scratch("loop-evidence-scope", &[
        ("Cargo.toml", "[package]\nname = \"loop_scope\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n"),
        ("src/lib.rs", "pub fn carried(mut n: u8, x: bool) -> bool { while n > 0 { n -= 1; } x }\n"),
    ]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock");
    assert!(lock.status.success());
    let dumps = dumped(&dir, "loop-evidence-scope");
    let result = uorust::mir::prove_bounded(
        &dir,
        &options("loop_scope"),
        &dumps,
        &dir.join("verdicts.toml"),
        &dir.join("trial"),
        1,
    )
    .expect("SDK proof");
    assert_eq!((result.proved, result.refused), (1, 0));
    let behavior =
        std::fs::read_to_string(dir.join("trial/src/Behavior.lex.tex")).expect("actual trial");
    let start = behavior
        .find("name: decreases_")
        .expect("decrease evidence");
    let parameters = behavior[start..]
        .split("statement:")
        .next()
        .expect("evidence header");
    assert!(
        !parameters.contains(": Bool"),
        "invariant was left as a fixed argument: {parameters}"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-43: the proof feedback loop checks only its closure and preserves decisions.
#[test]
fn native_verification_scopes_feedback_and_reuses_decisions_ue_43() {
    let (work, source, _) = mir_method_fixture("scoped-feedback");
    let path = source.join("src/lib.rs");
    let mut code = std::fs::read_to_string(&path).expect("source");
    code.push_str(
        "pub fn overflow(x: u8) -> u8 { let mut n = 1u8; while n > 0 { n -= 1; } x + 1 }\n",
    );
    std::fs::write(&path, code).expect("overflow case");
    let dumps = work.join("with-overflow");
    uorust::mir::dump(&source, &dumps, &Default::default()).expect("actual compiler");
    let verdict_path = work.join("verdicts.toml");
    let trial = work.join("trial");
    let summary = uorust::mir::prove_bounded(
        &source,
        &options("owners"),
        &dumps,
        &verdict_path,
        &trial,
        4,
    )
    .expect("actual SDK decisions");
    assert_eq!((summary.proved, summary.refused), (3, 1));
    let result: serde_json::Value = serde_json::from_slice(
        &std::fs::read(trial.join("uorust-proof-result.json"))
            .expect("machine-readable SDK feedback"),
    )
    .expect("SDK JSON");
    let modules = result["modules"].as_array().expect("verified modules");
    assert_eq!(
        modules.len(),
        2,
        "native trial verified unrelated modules: {result}"
    );
    let before = std::fs::read(&verdict_path).expect("actual verdicts");
    let replay = uorust::mir::prove_bounded(
        &source,
        &options("owners"),
        &dumps,
        &verdict_path,
        &trial,
        4,
    )
    .expect("unchanged replay");
    assert_eq!((replay.proved, replay.refused, replay.runs), (0, 0, 0));
    assert_eq!(
        std::fs::read(&verdict_path).expect("cached verdicts"),
        before
    );
    std::fs::remove_dir_all(work).expect("cleanup");
}

/// UE-42: native transport reads the same lock-admitted types as source transport.
#[test]
fn native_registry_carriers_require_locked_readings_ue_42() {
    let dir = scratch("native-registry-carriers", &[
        ("Cargo.toml", "[package]\nname = \"native_carriers\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nnum-bigint = \"=0.4.8\"\nnum-rational = \"=0.4.2\"\nserde_json = \"=1.0.114\"\nbitvec = \"=1.0.1\"\n"),
        ("src/lib.rs", "use num_bigint::BigInt;\nuse num_rational::BigRational;\nuse serde_json::{Value, Number};\nuse bitvec::vec::BitVec;\npub fn big(x: BigInt) -> BigInt { let mut n = 1u8; while n > 0 { n -= 1; } x }\npub fn ratio(x: BigRational) -> BigRational { let mut n = 1u8; while n > 0 { n -= 1; } x }\npub fn json(x: Value) -> Value { let mut n = 1u8; while n > 0 { n -= 1; } x }\npub fn number(x: Number) -> Number { let mut n = 1u8; while n > 0 { n -= 1; } x }\npub fn bits(x: BitVec) -> BitVec { let mut n = 1u8; while n > 0 { n -= 1; } x }\npub fn equal(a: Value, b: Value) -> bool { let mut n = 1u8; while n > 0 { n -= 1; } a == b }\n"),
    ]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock native carriers");
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    let dumps = dumped(&dir, "native-registry-carriers");
    let mut opts = options("native_carriers");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("native carriers");
    let listing = text(&files, uorust::mir::CANDIDATES);
    for name in ["big", "ratio", "json", "number", "bits"] {
        assert!(
            listing.contains(&format!("native_carriers::{name}\"")),
            "missing native carrier {name}: {listing}"
        );
    }
    assert!(
        !listing.contains("native_carriers::equal\""),
        "unmodeled JSON equality was admitted"
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write native carriers");
    let checked = std::process::Command::new("lexlean")
        .args([
            "check",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK verifies native carriers");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("native_carriers"),
        &dumps,
        &dir.join("verdicts.toml"),
        &dir.join("proof"),
        5,
    )
    .expect("actual native carrier proofs");
    assert_eq!((proof.proved, proof.refused), (5, 0));
    let lock_path = dir.join("Cargo.lock");
    let original = std::fs::read_to_string(&lock_path).expect("actual lock");
    let checksum = "c89e69e7e0f03bea5ef08013795c25018e101932225a656383bd384495ecc367";
    assert!(original.contains(checksum));
    std::fs::write(&lock_path, original.replace(checksum, &"0".repeat(64)))
        .expect("wrong checksum");
    let wrong = uorust::project_workspace(&dir, &opts).expect("refused carrier projection");
    assert!(
        !text(&wrong, uorust::mir::CANDIDATES).contains("native_carriers::big\""),
        "wrong checksum admitted native BigInt"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-41: source integer fields cannot silently become native bit patterns.
#[test]
fn native_aggregates_check_field_representations_ue_41() {
    let dir = scratch("aggregate-representations", &[
        ("Cargo.toml", "[package]\nname = \"aggregate_representations\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n"),
        ("src/lib.rs", "pub struct Signed { pub x: i32 }\npub enum Payload { Number(i32), Empty }\npub fn variant(n: i32) -> Payload { let mut count = 1u8; while count > 0 { count -= 1; } Payload::Number(n) }\npub fn array(s: Signed) -> [i32; 2] { let mut n = 1u8; while n > 0 { n -= 1; } [s.x, s.x] }\npub fn optional(s: Signed) -> Option<i32> { let mut n = 1u8; while n > 0 { n -= 1; } Some(s.x) }\npub fn unpack(s: Signed) -> (i32, u8, u8) { let mut n = 1u8; while n > 0 { n -= 1; } (s.x, n, 0) }\npub fn pack(n: i32) -> Signed { let mut count = 1u8; while count > 0 { count -= 1; } Signed { x: n } }\npub fn good(mut n: u8) -> (u8, u8, u8) { while n > 0 { n -= 1; } (n, 1, 2) }\n"),
    ]);
    let lock = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("lock source fixture");
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    let dumps = dumped(&dir, "aggregate-representations");
    let mut opts = options("aggregate_representations");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).expect("native aggregate projection");
    let listing = text(&files, uorust::mir::CANDIDATES);
    for name in ["unpack", "array", "optional", "pack", "variant"] {
        assert!(
            listing.contains(&format!("aggregate_representations::{name}\"")),
            "source signed read did not use its admitted codec: {name}"
        );
    }
    assert!(
        listing.contains("aggregate_representations::good"),
        "{listing}"
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write aggregates");
    let checked = std::process::Command::new("lexlean")
        .args([
            "check",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK type checker");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let source_path = dir.join("src/lib.rs");
    let original = std::fs::read_to_string(&source_path).unwrap();
    let drifted = original
        .replace("pub x: i32", "pub x: bool")
        .replace("Number(i32)", "Number(bool)");
    assert_ne!(original, drifted);
    std::fs::write(&source_path, drifted).unwrap();
    let mismatched = uorust::project_workspace(&dir, &opts).unwrap();
    for name in ["unpack", "pack", "variant", "array", "optional"] {
        assert!(
            residue_detail(&mismatched, &format!("aggregate_representations::{name}"))
                .is_some_and(|s| s.contains("field representation")),
            "an incompatible stored schema acquired a signed codec: {name}"
        );
    }
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-40: imported names are resolved, while function type parameters shadow them.
#[test]
fn renamed_unique_types_preserve_imports_and_generic_shadowing_ue_40() {
    let dir=scratch("renamed-unique-types", &[
        ("Cargo.toml","[package]\nname = \"renamed_types\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("src/lib.rs","pub mod data; pub mod holder;\n"),
        ("src/data.rs","pub struct Payload { pub value: u8 }\n"),
        ("src/holder.rs","use crate::data::Payload as Renamed;\nuse crate::data::Payload as T;\npub struct Held { pub payload: Renamed }\npub fn keep(x: Held) -> Held { x }\npub fn get(x: &Held) -> u8 { x.payload.value }\npub fn direct(x: Renamed) -> Renamed { x }\npub trait Read { fn read(&self) -> u8; }\npub fn hold_generic<T: Read>(x: &T) -> u8 { x.read() }\n"),
    ]);
    let files = project(&dir, "renamed_types");
    let lifted: Vec<String> = frontier_rows(&files, "lifted")
        .iter()
        .filter_map(|r| r["function"].as_str().map(str::to_string))
        .collect();
    for name in ["keep", "get", "direct", "hold_generic"] {
        assert!(
            lifted.contains(&format!("renamed_types::holder::{name}")),
            "renamed type function did not lift: {name}"
        );
    }
    let behavior = text(&files, "src/Behavior.lex.tex");
    let start = behavior
        .find("name: holdGeneric,")
        .expect("generic definition");
    let header = behavior[start..].split("body:").next().expect("header");
    assert!(
        header.contains("type_parameters: [T]") && header.contains("x : 'T"),
        "generic parameter was rebound to the imported concrete type: {header}"
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).expect("write renamed types");
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .expect("SDK verifier");
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// UE-66: import whole generic dependencies through the existing source binder.
#[test]
fn compiler_dependencies_reuse_generic_source_bindings_ue_66() {
    let dir = scratch("generic-compiler-dependencies", &[
        ("Cargo.toml", "[package]\nname=\"generic_dependencies\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn transport<T>(x:T)->T {x}
pub fn wrapped<T>(x:Option<T>)->Option<T> {x}
pub trait Observe {fn observe(&self)->bool;}
pub struct Flag {pub enabled:bool}
impl Observe for Flag {fn observe(&self)->bool {self.enabled}}
pub fn observe<T:Observe>(x:&T)->bool {x.observe()}
pub fn word(mut n:u8,x:u8)->u8 {while n>0 {n-=1;} transport(x)}
pub fn signed(mut n:u8,x:i8)->i8 {while n>0 {n-=1;} transport(x)}
pub fn option(mut n:u8,x:Option<u8>)->Option<u8> {while n>0 {n-=1;} wrapped(x)}
pub fn dictionary(mut n:u8,x:&Flag)->bool {while n>0 {n-=1;} observe(x)}
pub fn apply<F:Fn(bool)->bool>(f:F,x:bool)->bool {f(x)}
pub fn callback(mut n:u8,x:bool,capture:bool)->bool {while n>0 {n-=1;} apply(|b| b!=capture,x)}
pub struct Unavailable {pub enabled:bool}
impl Observe for Unavailable {fn observe(&self)->bool {std::process::id()>0}}
pub fn unavailable(mut n:u8,x:&Unavailable)->bool {while n>0 {n-=1;} observe(x)}
pub trait SignedRead {fn read(&self)->i8;}
pub struct SignedFlag {pub value:i8}
impl SignedRead for SignedFlag {fn read(&self)->i8 {self.value}}
pub fn signed_read<T:SignedRead>(x:&T)->i8 {x.read()}
pub fn incompatible(mut n:u8,x:&SignedFlag)->i8 {while n>0 {n-=1;} signed_read(x)}
pub fn partial<T>(_:T,v:Option<bool>)->bool {v.unwrap()}
pub fn partial_native(mut n:u8,x:u8,v:Option<bool>)->bool {while n>0 {n-=1;} partial(x,v)}
pub fn mutable(mut n:u8,x:&mut u8)->&mut u8 {while n>0 {n-=1;} transport(x)}
pub struct SharedFlag {pub enabled:std::cell::Cell<bool>}
impl Observe for SharedFlag {fn observe(&self)->bool {self.enabled.get()}}
pub fn shared(mut n:u8,x:&SharedFlag)->bool {while n>0 {n-=1;} observe(x)}
"#),
        ("src/bin/vectors.rs", r#"
use generic_dependencies::*;
fn main(){for n in [0,2] {
 for x in [0,19,255] {println!("word {n} {x} {}",word(n,x));}
 for x in [-128i8,-1,0,127] {println!("signed {n} {} {}", x as u8,signed(n,x) as u8);}
 for x in [None,Some(0),Some(255)] {let term=|v:Option<u8>|match v {None=>"(ctor Option.none [Nat])".to_string(),Some(x)=>format!("(ctor Option.some [Nat] {x})")};println!("option|{n}|{}|{}",term(x),term(option(n,x)));}
 for x in [false,true] {println!("dictionary {n} {x} {}",dictionary(n,&Flag{enabled:x}));for capture in [false,true] {println!("callback {n} {x} {capture} {}",callback(n,x,capture));}}
}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "generic-compiler-dependencies");
    let mut opts = options("generic_dependencies");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for f in ["word", "signed", "option", "dictionary", "callback"] {
        let path = format!("generic_dependencies::{f}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&files, &path)
        );
    }

    for f in [
        "unavailable",
        "incompatible",
        "partial_native",
        "mutable",
        "shared",
    ] {
        let path = format!("generic_dependencies::{f}");
        assert!(
            !frontier_rows(&files, "lifted")
                .iter()
                .any(|r| r["function"].as_str() == Some(&path)),
            "{f}"
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("generic-dependency-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("generic_dependencies"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    let output = String::from_utf8(rust.stdout).unwrap();
    for (i, line) in output.lines().enumerate() {
        let parts: Vec<_> = if line.starts_with("option|") {
            line.split('|').collect()
        } else {
            line.split_whitespace().collect()
        };
        let path = format!("generic_dependencies::{}", parts[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let input = if parts[0] == "dictionary" {
            format!("{} (record Flag {{enabled := {}}})", parts[1], parts[2])
        } else {
            parts[1..parts.len() - 1].join(" ")
        };
        let expected = parts.last().unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name: genericDependencyVector{i}, parameters: [], statement: (({name} {input}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
    }
    assert_eq!(output.lines().count(), 32);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = vectors.replacen("= 255)", "= 254)", 1);
    assert_ne!(wrong, vectors);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&vectors, &wrong, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-67: generic bodies use the same compiler pipeline and SDK products.
#[test]
fn compiler_generic_bodies_preserve_product_state_ue_67() {
    let dir = scratch("generic-compiler-bodies", &[
        ("Cargo.toml", "[package]\nname=\"generic_bodies\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn transport<T>(mut n:u8,x:T)->T {while n>0 {n-=1;}x}
pub fn product<T>(mut n:u8,x:T)->(T,bool,u8) {while n>0 {n-=1;}(x,true,n)}
pub fn tuple<T>(x:T,n:u8,b:bool)->(T,u8,bool) {(x,n,b)}
pub fn nested<T>(mut n:u8,x:T)->T {while n>0 {let mut m=n;while m>0 {m-=1;}n-=1;}x}
pub fn discard<T>(mut n:u8,x:T)->bool {while n>0 {n-=1;}true}
pub trait Read {fn read(&self)->bool;}
pub fn dispatch<T:Read>(mut n:u8,x:&T)->bool {while n>0 {n-=1;}x.read()}
pub trait Mimic {fn clone(&self)->Self;}
pub fn mimic<T:Mimic>(mut n:u8,x:&T)->T {while n>0 {n-=1;}x.clone()}
pub fn bounded<T:Clone>(mut n:u8,x:T)->T {while n>0 {n-=1;}x}
pub fn cloned<T:Clone>(mut n:u8,x:&T)->T {while n>0 {n-=1;}x.clone()}
"#),
        ("src/bin/vectors.rs", r#"
use generic_bodies::*;
fn main(){for n in [0,2] {for x in [false,true] {
 println!("transport|Bool|{n}|{x}|{}",transport(n,x));println!("nested|Bool|{n}|{x}|{}",nested(n,x));
 let (a,b,c)=product(n,x);println!("product|Bool|{n}|{x}|({a}, ({b}, {c}))");
 let (a,b,c)=tuple(x,n,true);println!("tuple|Bool|{n}|{x}|({a}, ({b}, {c}))");
}for x in [0u8,19,255] {
 println!("transport|Nat|{n}|{x}|{}",transport(n,x));println!("nested|Nat|{n}|{x}|{}",nested(n,x));
 let (a,b,c)=product(n,x);println!("product|Nat|{n}|{x}|({a}, ({b}, {c}))");
 let (a,b,c)=tuple(x,n,false);println!("tuple|Nat|{n}|{x}|({a}, ({b}, {c}))");
}}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "generic-compiler-bodies");
    let mut opts = options("generic_bodies");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for f in ["mimic", "cloned"] {
        assert!(
            residue_detail(&files, &format!("generic_bodies::{f}")).is_some(),
            "{f}: generic dispatch has no admitted dictionary"
        );
    }
    for f in ["transport", "product", "nested", "bounded"] {
        let path = format!("generic_bodies::{f}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&files, &path)
        );
    }
    assert!(
        frontier_rows(&files, "lifted")
            .iter()
            .any(|r| r["function"].as_str() == Some("generic_bodies::tuple")),
        "generic source product"
    );
    for f in ["discard", "dispatch"] {
        assert!(
            residue_detail(&files, &format!("generic_bodies::{f}")).is_some(),
            "{f}"
        );
    }

    assert!(residue_detail(&files, "generic_bodies::discard")
        .unwrap()
        .contains("drop glue"));
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("generic-body-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("generic_bodies"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let output = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(output.lines().count(), 40);
    let mut vectors = String::new();
    for (i, line) in output.lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("generic_bodies::{}", p[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let args = if p[0] == "tuple" {
            format!(
                "{} {} {}",
                p[3],
                p[2],
                if p[1] == "Bool" { "true" } else { "false" }
            )
        } else {
            format!("{} {}", p[2], p[3])
        };
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name: genericBodyVector{i}, parameters: [], statement: (({name} [{}] {args}) = {}), proof: decide{{}}, axioms: [{}]}}\n",p[1],p[4],policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = vectors.replacen("= 255)", "= 254)", 1);
    assert_ne!(wrong, vectors);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&vectors, &wrong, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-68: source context outside the model dependency closure is not new work.
#[test]
fn native_proof_identity_tracks_reached_models_ue_68() {
    let source = r#"
pub struct Payload {pub flag:bool,pub padding:bool}
pub fn helper(x:bool)->bool {x}
pub fn kept(mut n:u8,x:&Payload)->bool {while n>0 {n-=1;}helper(x.flag)}
pub fn refused(a:u8)->u8 {a+1}
"#;
    let dir=scratch("rooted-native-models",&[("Cargo.toml","[package]\nname=\"rooted_models\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",source)]);
    let rust = std::process::Command::new("cargo")
        .args(["check", "--offline", "--quiet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(rust.status.success());
    let dumps = dumped(&dir, "rooted-native-models");
    let mut opts = options("rooted_models");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let identity = || {
        let files = uorust::project_workspace(&dir, &opts).unwrap();
        let doc: toml::Value = toml::from_str(&text(&files, uorust::mir::CANDIDATES)).unwrap();
        let row = doc["candidate"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["function"].as_str() == Some("rooted_models::kept"))
            .unwrap();
        row.get("model_context_sha256")
            .unwrap_or(&row["digest"])
            .as_str()
            .unwrap()
            .to_string()
    };
    let before = identity();
    std::fs::write(
        dir.join("src/lib.rs"),
        format!("{source}\npub fn unrelated(x:bool)->bool {{!x}}\n"),
    )
    .unwrap();
    assert_eq!(
        before,
        identity(),
        "unrelated source context rescheduled an unchanged model"
    );
    std::fs::write(
        dir.join("src/lib.rs"),
        source.replace(
            "fn helper(x:bool)->bool {x}",
            "fn helper(x:bool)->bool {!x}",
        ),
    )
    .unwrap();
    assert_ne!(before, identity(), "reached source meaning changed");
    std::fs::write(
        dir.join("src/lib.rs"),
        source.replace("pub padding:bool", "pub padding:u8"),
    )
    .unwrap();
    assert_ne!(before, identity(), "reached schema changed");

    std::fs::write(dir.join("src/lib.rs"), source).unwrap();
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rooted-model-verdicts.toml");
    let prove = || {
        uorust::mir::prove_bounded(
            &dir,
            &options("rooted_models"),
            &dumps,
            &cache,
            &dir.join("trial"),
            8,
        )
        .unwrap()
    };
    let first = prove();
    assert_eq!(first.refused, 1, "{first:?}");
    let original = verdicts(&cache);
    assert_eq!(
        original
            .rows
            .values()
            .filter(|v| v.status == uorust::mir::verdict::Decision::Proved)
            .count(),
        1
    );
    // Reproduce a legacy cache by removing only the new optional observation.
    let legacy = std::fs::read_to_string(&cache)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("model_context_sha256 = "))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&cache, legacy).unwrap();
    let legacy_run = prove();
    assert_eq!(
        (legacy_run.proved, legacy_run.refused, legacy_run.runs),
        (0, 0, 0)
    );
    let learned = verdicts(&cache);
    for (key, old) in &original.rows {
        let new = &learned.rows[key];
        assert_eq!(
            (&new.digest, &new.status, &new.axioms),
            (&old.digest, &old.status, &old.axioms)
        );
        assert!(new.model_context_sha256.is_some());
    }
    let unrelated = format!("{source}\npub fn unrelated(x:bool)->bool {{!x}}\n");
    std::fs::write(dir.join("src/lib.rs"), &unrelated).unwrap();
    let reused = prove();
    assert_eq!((reused.proved, reused.refused, reused.runs), (0, 0, 0));
    assert_eq!(
        verdicts(&cache),
        learned,
        "original decision provenance changed on reuse"
    );
    std::fs::write(
        dir.join("src/lib.rs"),
        unrelated.replace(
            "fn helper(x:bool)->bool {x}",
            "fn helper(x:bool)->bool {!x}",
        ),
    )
    .unwrap();
    let changed = prove();
    assert_eq!((changed.proved, changed.refused), (1, 0));
    assert!(
        changed.runs > 0,
        "changed reached meaning skipped the actual SDK"
    );
    let good = verdicts(&cache);
    let mut planted = good.clone();
    for row in planted
        .rows
        .values_mut()
        .filter(|v| v.status == uorust::mir::verdict::Decision::Proved)
    {
        row.model_context_sha256 = Some("0".repeat(64));
    }
    let mut bound = options("rooted_models");
    bound.mir = Some(mir_options(dumps, planted, uorust::mir::Trial::Off));
    let rejected = uorust::project_workspace(&dir, &bound).unwrap();
    assert!(
        residue_detail(&rejected, "rooted_models::kept").is_some(),
        "a planted wrong model binding was admitted"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_bound_type_flow_preserves_abstract_field_values_ue_86() {
    let dir = scratch(
        "source-type-flow",
        &[
            (
                "Cargo.toml",
                "[package]\nname=\"type_flow\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n",
            ),
            (
                "src/lib.rs",
                r#"
pub trait Service {fn run(&self)->bool;}
pub struct Holder {pub ready:bool,pub service:&'static dyn Service}
pub fn carried(mut n:u8,x:&Holder)->&'static dyn Service {while n>0 {n-=1;} x.service}
pub fn copied(mut n:u8,x:&Holder)->&'static dyn Service {while n>0 {n-=1;} let first=x.service;let second=first;second}
pub fn changed(mut n:u8,x:&mut Holder,new:&'static dyn Service)->bool {while n>0 {n-=1;} x.service=new;x.ready}
pub fn operation(mut n:u8,x:&Holder)->bool {while n>0 {n-=1;} x.service.run()}
pub struct Pair {pub left:&'static dyn Service,pub right:&'static dyn Service}
pub fn conflict(mut n:u8,x:&Pair,choose:bool)->&'static dyn Service {while n>0 {n-=1;} if choose {x.left} else {x.right}}
pub fn unanchored(mut n:u8,x:&'static dyn Service)->&'static dyn Service {while n>0 {n-=1;} x}
"#,
            ),
            (
                "src/bin/vectors.rs",
                r#"
use type_flow::*;
struct False;impl Service for False {fn run(&self)->bool {false}}
struct True;impl Service for True {fn run(&self)->bool {true}}
static FALSE:False=False;static TRUE:True=True;
fn service(value:bool)->&'static dyn Service {if value {&TRUE} else {&FALSE}}
fn main(){for n in [0,2] {for ready in [false,true] {for old in [false,true] {
let holder=Holder{ready,service:service(old)};
println!("carried|{n}|{ready}|{old}|false|{}",carried(n,&holder).run());
println!("copied|{n}|{ready}|{old}|false|{}",copied(n,&holder).run());
for new in [false,true] {
let mut holder=Holder{ready,service:service(old)};
let result=changed(n,&mut holder,service(new));
println!("changed|{n}|{ready}|{old}|{new}|({result}, ({ready}, {}))",holder.service.run());
let pair=Pair{left:service(old),right:service(new)};
println!("conflict|{n}|{ready}|{old}|{new}|{}",conflict(n,&pair,ready).run());
}}}}}
"#,
            ),
        ],
    );
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 48);
    let dumps = dumped(&dir, "source-type-flow");
    let mut opts = options("type_flow");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for function in ["carried", "copied", "changed", "conflict"] {
        let path = format!("type_flow::{function}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&files, &path)
        );
    }
    for function in ["operation", "unanchored"] {
        let path = format!("type_flow::{function}");
        assert!(
            residue_detail(&files, &path).is_some(),
            "{path} acquired an unbound model"
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("source-type-flow-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("type_flow"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [function, n, ready, old, new, expected] = parts.as_slice() else {
            panic!("{line}")
        };
        let path = format!("type_flow::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let extra = if *function == "changed" {
            format!(" {new}")
        } else {
            String::new()
        };
        let call = if *function == "conflict" {
            format!("{name} [Bool] {n} ({old}, {new}) {ready}")
        } else {
            format!("{name} [Bool] {n} ({ready}, {old}){extra}")
        };
        vectors.push_str(&format!("theorem{{name: typeFlowVector{i}, parameters: [], statement: (({call}) = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = body.replacen("= false)", "= true)", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::write(&file, &body).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-69: field transport does not require invented service semantics.
#[test]
fn compiler_struct_views_preserve_opaque_fields_ue_69() {
    let dir=scratch("native-struct-views",&[("Cargo.toml","[package]\nname=\"struct_views\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
pub trait Service {fn run(&self)->bool;}
pub struct Holder {pub ready:bool,pub service:Box<dyn Service>}
pub fn known(mut n:u8,x:&Holder)->bool {while n>0 {n-=1;}x.ready}
pub struct Outer {pub inner:Holder,pub tag:u8}
pub fn nested(mut n:u8,x:&Outer)->u8 {while n>0 {n-=1;}if x.inner.ready {x.tag} else {0}}
pub fn caller(mut n:u8,x:&Holder)->bool {while n>0 {n-=1;}known(n,x)}
pub fn changed(mut n:u8,x:&mut Holder)->bool {while n>0 {n-=1;}x.ready=!x.ready;x.ready}
pub fn unknown(mut n:u8,x:&Holder)->bool {while n>0 {n-=1;}x.service.run()}
pub fn discard(mut n:u8,x:Holder)->bool {while n>0 {n-=1;}x.ready}
pub trait Mimic {fn clone(&self)->bool;}
impl Mimic for Holder {fn clone(&self)->bool {!self.ready}}
pub fn mimic(mut n:u8,x:&Holder)->bool {while n>0 {n-=1;}x.clone()}
pub struct Generic<T> {pub ready:bool,pub service:T}
pub fn incomplete(mut n:u8,x:&Generic<Box<dyn Service>>)->bool {while n>0 {n-=1;}x.ready}
"#),("src/bin/vectors.rs",r#"
use struct_views::*;
struct Unused(u8);impl Service for Unused {fn run(&self)->bool {panic!("service was evaluated: {}",self.0)}}
fn main(){for n in [0,2] {for ready in [false,true] {for payload in [0u8,23,255] {
 let holder=||Holder{ready,service:Box::new(Unused(payload))};
 println!("known|{n}|{ready}|{payload}|{}",known(n,&holder()));
 println!("caller|{n}|{ready}|{payload}|{}",caller(n,&holder()));
 println!("nested|{n}|{ready}|{payload}|{}",nested(n,&Outer{inner:holder(),tag:payload}));
 let mut x=holder();let result=changed(n,&mut x);println!("changed|{n}|{ready}|{payload}|({result}, ({}, {payload}))",x.ready);
}}}}
"#)]);
    let checked = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let dumps = dumped(&dir, "native-struct-views");
    let mut opts = options("struct_views");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        text(&files, uorust::mir::CANDIDATES).contains("struct_views::known\""),
        "{:?}",
        residue_detail(&files, "struct_views::known")
    );
    for f in ["known", "nested", "caller", "changed"] {
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("struct_views::{f}\"")),
            "{f}: {:?}",
            residue_detail(&files, &format!("struct_views::{f}"))
        );
    }
    for f in ["unknown", "discard", "mimic", "incomplete"] {
        assert!(
            residue_detail(&files, &format!("struct_views::{f}")).is_some(),
            "{f} acquired invented semantics"
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-struct-view-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("struct_views"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let output = String::from_utf8(checked.stdout).unwrap();
    assert_eq!(output.lines().count(), 48);
    let mut vectors = String::new();
    for (i, line) in output.lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("struct_views::{}", p[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let value = if p[0] == "nested" {
            format!("(({}, {}), {})", p[2], p[3], p[3])
        } else {
            format!("({}, {})", p[2], p[3])
        };
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name: structViewVector{i}, parameters: [], statement: (({name} [Nat] {} {value}) = {}), proof: decide{{}}, axioms: [{}]}}\n",p[1],p[4],policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = vectors.replacen("= true)", "= false)", 1);
    assert_ne!(wrong, vectors);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&vectors, &wrong, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-69: a nominal service call cannot fall through to a same-named adapter.
#[test]
fn compiler_struct_views_refuse_unbound_custom_calls_ue_69() {
    let dir=scratch("native-view-custom-dispatch",&[("Cargo.toml","[package]\nname=\"view_dispatch\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
pub trait Service {fn run(&self)->bool;}
pub struct Holder {pub ready:bool,pub service:Box<dyn Service>}
pub struct Always;impl Service for Always {fn run(&self)->bool {true}}
impl Holder {pub fn to_owned(&self)->Self {Holder{ready:!self.ready,service:Box::new(Always)}}}
pub fn impostor(mut n:u8,x:&Holder)->Holder {while n>0 {n-=1;}x.to_owned()}
"#)]);
    let checked = std::process::Command::new("cargo")
        .args(["check", "--offline", "--quiet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let dumps = dumped(&dir, "native-view-custom-dispatch");
    let mut opts = options("view_dispatch");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        residue_detail(&files, "view_dispatch::impostor").is_some(),
        "an unbound custom call acquired a standard adapter meaning"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-70: incoming native words retain their type until the storage boundary.
#[test]
fn compiler_storage_carriers_preserve_nested_state_ue_70() {
    let dir=scratch("typed-storage-carriers",&[("Cargo.toml","[package]\nname=\"storage_carriers\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
pub struct WordPacket {pub value:i128}
pub struct NestedPacket {pub inner:WordPacket}
pub fn put(mut n:u8,x:&mut WordPacket,v:i128) {while n>0 {n-=1;}x.value=v;}
pub fn nested(mut n:u8,x:&mut NestedPacket,v:i128) {while n>0 {n-=1;}x.inner.value=v;}
"#),("src/bin/vectors.rs",r#"
use storage_carriers::*;
fn main(){for n in [0,2] {for v in [i128::MIN,-1,0,1,1i128<<100,i128::MAX] {
 let mut x=WordPacket{value:0};put(n,&mut x,v);println!("put|{n}|{}|{}",v as u128,x.value);
 let mut y=NestedPacket{inner:WordPacket{value:0}};nested(n,&mut y,v);println!("nested|{n}|{}|{}",v as u128,y.inner.value);
}}}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "typed-storage-carriers");
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("typed-storage-carrier-verdicts.toml");
    let proved = uorust::mir::prove_bounded(
        &dir,
        &options("storage_carriers"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proved.refused, 0, "{proved:?}");
    let decisions = verdicts(&cache);
    let mut opts = options("storage_carriers");
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let output = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(output.lines().count(), 24);
    let mut vectors = String::new();
    let mut first_wrong = None;
    for (i, line) in output.lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("storage_carriers::{}", p[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap_or_else(|| panic!("{path}: {:?}", residue_detail(&files, &path)))
            .strip_prefix("Behavior::")
            .unwrap();
        let word = "(record WordPacket {value := (0 : Int)})";
        let input = if p[0] == "put" {
            word.to_string()
        } else {
            format!("(record NestedPacket {{inner := {word}}})")
        };
        let state = format!("(project ({name} {} {input} {}) item1)", p[1], p[2]);
        let word = if p[0] == "put" {
            state
        } else {
            format!("(project {state} inner)")
        };
        let stored = format!("(project {word} value)");
        let expected = match p[3].strip_prefix('-') {
            Some(v) => format!("(prim negate ({v} : Int) : Int)"),
            None => format!("({} : Int)", p[3]),
        };
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let theorem=format!("theorem{{name: storageVector{i}, parameters: [], statement: ({stored} = {expected}), proof: decide{{}}, axioms: [{}]}}\n",policy.join(", "));
        if i == 0 {
            first_wrong = Some((theorem.clone(), theorem.replacen(&expected, "(0 : Int)", 1)));
        }
        vectors.push_str(&theorem);
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = first_wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-79: compiler trait paths bind to the exact source provider.
#[test]
fn compiler_calls_bind_trait_definition_identities_ue_79() {
    let code="pub mod maps {\npub trait Select<T>{fn pick(x:T)->Self;}\npub trait Other{fn pick(x:u8)->Self;}\npub mod fake{pub trait From<T>{fn from(x:T)->Self;}}\npub struct Mark{pub code:u8}\nimpl Select<u8> for Mark{fn pick(_x:u8)->Self{Self{code:0}}}\nimpl Select<u16> for Mark{fn pick(_x:u16)->Self{Self{code:1}}}\nimpl Other for Mark{fn pick(_x:u8)->Self{Self{code:2}}}\nimpl From<u8> for Mark{fn from(_x:u8)->Self{Self{code:3}}}\nimpl From<u16> for Mark{fn from(_x:u16)->Self{Self{code:4}}}\nimpl Mark{pub fn pick(_x:u8)->Self{Self{code:5}}}\nimpl fake::From<u8> for Mark{fn from(_x:u8)->Self{Self{code:6}}}\n}\npub fn select_byte(mut n:u8)->maps::Mark{while n>0{n-=1;}<maps::Mark as maps::Select<u8>>::pick(n)}\npub fn select_short(mut n:u16)->maps::Mark{while n>0{n-=1;}<maps::Mark as maps::Select<u16>>::pick(n)}\npub fn other(mut n:u8)->maps::Mark{while n>0{n-=1;}<maps::Mark as maps::Other>::pick(n)}\npub fn from_byte(mut n:u8)->maps::Mark{while n>0{n-=1;}<maps::Mark as From<u8>>::from(n)}\npub fn from_short(mut n:u16)->maps::Mark{while n>0{n-=1;}<maps::Mark as From<u16>>::from(n)}\npub fn inherent(mut n:u8)->maps::Mark{while n>0{n-=1;}maps::Mark::pick(n)}\npub fn fake(mut n:u8)->maps::Mark{while n>0{n-=1;}<maps::Mark as maps::fake::From<u8>>::from(n)}\n";
    let oracle="use trait_calls::*;fn main(){for n in [0u8,1,3]{println!(\"select_byte|{n}|{}\",select_byte(n).code);println!(\"select_short|{n}|{}\",select_short(n as u16).code);println!(\"other|{n}|{}\",other(n).code);println!(\"from_byte|{n}|{}\",from_byte(n).code);println!(\"from_short|{n}|{}\",from_short(n as u16).code);println!(\"inherent|{n}|{}\",inherent(n).code);println!(\"fake|{n}|{}\",fake(n).code);}}\n";
    let dir=scratch("compiler-trait-call-identities",&[("Cargo.toml","[package]\nname=\"trait_calls\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",code),("src/bin/vectors.rs",oracle)]);
    let dumps = dumped(&dir, "compiler-trait-call-identities");
    let mut opts = options("trait_calls");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    let paths = [
        "trait_calls::select_byte",
        "trait_calls::select_short",
        "trait_calls::other",
        "trait_calls::from_byte",
        "trait_calls::from_short",
        "trait_calls::inherent",
        "trait_calls::fake",
    ];
    for path in paths {
        assert!(
            frontier_rows(&proposed, "lifted")
                .iter()
                .any(|r| r["function"].as_str() == Some(path)),
            "{path}: {:?}",
            residue_detail(&proposed, path)
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("compiler-trait-call-identities-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("trait_calls"),
        &dumps,
        &cache,
        &dir.join("trial"),
        3,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let rust = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--locked", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = paths
            .iter()
            .find(|path| path.ends_with(&format!("::{}", p[0])))
            .unwrap();
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(*path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == *path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let theorem=format!("theorem{{name: definitionVector{i},parameters:[],statement:((project ({name} {}) code) = {}),proof:decide{{}},axioms:[{}]}}\n",p[1],p[2],policy.join(", "));
        if p[0] == "select_short" && wrong.is_none() {
            wrong = Some((theorem.clone(), theorem.replacen("= 1)", "= 0)", 1)));
        }
        vectors.push_str(&theorem);
    }
    assert_eq!(vectors.lines().count(), 21);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-78: compiler implementation sites distinguish overloaded method bodies.
#[test]
fn overloaded_methods_keep_compiler_definition_identities_ue_78() {
    let code="pub struct Mark{pub code:u8}\nimpl From<u8> for Mark{fn from(mut n:u8)->Self{while n>0{n-=1;}Self{code:0}}}\nimpl From<u16> for Mark{fn from(mut n:u16)->Self{while n>0{n-=1;}Self{code:1}}}\nimpl Mark{pub fn from(mut n:u32)->Self{while n>0{n-=1;}Self{code:2}}}\n";
    let oracle="use definitions::*;fn main(){for n in [0u8,1,3]{println!(\"byte|{n}|{}\",<Mark as From<u8>>::from(n).code);println!(\"short|{n}|{}\",<Mark as From<u16>>::from(n as u16).code);println!(\"inherent|{n}|{}\",Mark::from(n as u32).code);}}\n";
    let dir=scratch("compiler-definition-identities",&[("Cargo.toml","[package]\nname=\"definitions\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",code),("src/bin/vectors.rs",oracle)]);
    let dumps = dumped(&dir, "compiler-definition-identities");
    let mut opts = options("definitions");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    let paths = [
        "definitions::<Mark as From<u8>>::from",
        "definitions::<Mark as From<u16>>::from",
        "definitions::Mark::from",
    ];
    for path in paths {
        assert!(
            frontier_rows(&proposed, "lifted")
                .iter()
                .any(|r| r["function"].as_str() == Some(path)),
            "{path}: {:?}",
            residue_detail(&proposed, path)
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("compiler-definition-identities-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("definitions"),
        &dumps,
        &cache,
        &dir.join("trial"),
        3,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let rust = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--locked", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = match p[0] {
            "byte" => paths[0],
            "short" => paths[1],
            _ => paths[2],
        };
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let theorem=format!("theorem{{name: definitionVector{i},parameters:[],statement:((project ({name} {}) code) = {}),proof:decide{{}},axioms:[{}]}}\n",p[1],p[2],policy.join(", "));
        if p[0] == "short" && wrong.is_none() {
            wrong = Some((theorem.clone(), theorem.replacen("= 1)", "= 0)", 1)));
        }
        vectors.push_str(&theorem);
    }
    assert_eq!(vectors.lines().count(), 9);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-77: parameterless source definitions are invoked, never emitted as locals.
#[test]
fn compiler_calls_apply_parameterless_source_models_ue_77() {
    let code="pub struct Flag{pub value:bool}\npub fn zero()->bool{true}\npub fn seven()->u8{7}\npub fn read(mut n:u8)->bool{while n>0{n-=1;}zero()}\npub fn number(mut n:u8)->u8{while n>0{n-=1;}seven()}\npub fn make(mut n:u8)->Flag{while n>0{n-=1;}Flag{value:zero()}}\npub fn update(mut n:u8,x:&mut Flag)->bool{while n>0{n-=1;}x.value=zero();x.value}\n";
    let oracle="use applications::*;fn main(){for n in [0,1,2,255]{println!(\"read|{n}|{}\",read(n));println!(\"number|{n}|{}\",number(n));println!(\"make|{n}|{}\",make(n).value);let mut x=Flag{value:false};let out=update(n,&mut x);assert_eq!(out,x.value);println!(\"update|{n}|{}\",x.value);}}\n";
    let dir=scratch("parameterless-applications",&[("Cargo.toml","[package]\nname=\"applications\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",code),("src/bin/vectors.rs",oracle)]);
    let dumps = dumped(&dir, "parameterless-applications");
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("parameterless-applications-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("applications"),
        &dumps,
        &cache,
        &dir.join("trial"),
        4,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let old = verdicts(&cache);
    let mut opts = options("applications");
    opts.mir = Some(mir_options(
        dumps.clone(),
        old.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let rust = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--locked", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("applications::{}", p[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap_or_else(|| panic!("{path}: {:?}", residue_detail(&files, &path)))
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = old
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let call=match p[0]{"make"=>format!("(project ({name} {}) value)",p[1]),"update"=>format!("((project ({name} {} (record Flag {{value := false}})) item0) && (project (project ({name} {} (record Flag {{value := false}})) item1) value))",p[1],p[1]),_=>format!("({name} {})",p[1])};
        let theorem=format!("theorem{{name: applicationVector{i},parameters:[],statement:({call} = {}),proof:decide{{}},axioms:[{}]}}\n",p[2],policy.join(", "));
        if p[0] == "read" && wrong.is_none() {
            wrong = Some((theorem.clone(), theorem.replacen("= true)", "= false)", 1)));
        }
        vectors.push_str(&theorem);
    }
    assert_eq!(vectors.lines().count(), 16);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::write(
        dir.join("src/lib.rs"),
        code.replacen("{true}", "{false}", 1),
    )
    .unwrap();
    let changed = uorust::project_workspace(&dir, &opts).unwrap();
    let changed_rows = frontier_rows(&changed, "lifted");
    for mode in ["read", "make", "update"] {
        let path = format!("applications::{mode}");
        assert!(
            !changed_rows
                .iter()
                .any(|r| r["function"].as_str() == Some(&path)),
            "changed source reused {path}"
        );
    }
    let refreshed = uorust::mir::prove_bounded(
        &dir,
        &options("applications"),
        &dumps,
        &cache,
        &dir.join("trial"),
        4,
    )
    .unwrap();
    assert_eq!(refreshed.proved, 3, "{refreshed:?}");
    assert_eq!(refreshed.refused, 0);
    assert!(refreshed.runs > 0);
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-76: one structural boundary composes element codecs throughout containers.
#[test]
fn integer_carriers_compose_through_structures_ue_76() {
    let mut code=String::from("use std::collections::BTreeMap;\npub fn raw(x:u8)->u8{x}\npub fn codec_word(x:u8)->u8{x}\npub fn codec_integer(x:u8)->u8{x}\npub fn carrier_value(x:u8)->u8{x}\npub fn carrier_head(x:u8)->u8{x}\npub fn carrier_tail(x:u8)->u8{x}\npub fn carrier_some(x:u8)->u8{x}\npub fn carrier_result(x:u8)->u8{x}\n");
    for (name, ty) in [
        ("Nested", "Vec<Option<(i8,Result<i16,i32>)>>"),
        ("Entries", "BTreeMap<i16,i8>"),
    ] {
        let tag = name.to_ascii_lowercase();
        code.push_str(&format!("pub struct {name}{{pub values:{ty}}}\npub fn identity_{tag}(values:{ty})->{ty}{{values}}\npub fn take_{tag}(mut n:u8,x:{name})->{ty}{{while n>0{{n-=1;}}x.values}}\npub fn make_{tag}(mut n:u8,values:{ty})->{name}{{while n>0{{n-=1;}}{name}{{values}}}}\npub fn pass_{tag}(mut n:u8,values:{ty})->{ty}{{while n>0{{n-=1;}}identity_{tag}(values)}}\npub fn update_{tag}(mut n:u8,x:&mut {name},values:{ty}){{while n>0{{n-=1;}}x.values=values;}}\n"));
    }
    let oracle = r#"use carriers::*;
use std::collections::BTreeMap;
fn nested(x:&[Option<(i8,Result<i16,i32>)>],word:bool)->String {
 let t=if word {"TupleNatResultNatNat"}else{"TupleIntResultIntInt"};
 let r=if word {"Nat"}else{"Int64"};
 let number=|n:i64,raw:u64| if word {raw.to_string()}else{format!("({n} : Int64)")};
 x.iter().rev().fold(format!("(nil (Option {t}))"),|tail,v|{
  let head=match v {None=>format!("(ctor Option.none [{t}])"),Some((a,b))=>{
   let b=match b {Ok(b)=>format!("(ctor Result.ok [{r}, {r}] {})",number(*b as i64,*b as u16 as u64)),Err(c)=>format!("(ctor Result.error [{r}, {r}] {})",number(*c as i64,*c as u32 as u64))};
   format!("(ctor Option.some [{t}] (record {t} {{item0 := {}, item1 := {b}}}))",number(*a as i64,*a as u8 as u64))
  }};format!("(cons {head} {tail})")})
}
fn entries(x:&BTreeMap<i16,i8>,word:bool)->String {
 let t=if word {"TupleNatNat"}else{"TupleIntInt"};
 x.iter().rev().fold(format!("(nil {t})"),|tail,(a,b)|{
 let a=if word {(*a as u16).to_string()}else{format!("({a} : Int64)")};
 let b=if word {(*b as u8).to_string()}else{format!("({b} : Int64)")};
 format!("(cons (record {t} {{item0 := {a}, item1 := {b}}}) {tail})")})
}
fn main(){for n in [0,2]{for sample in 0..3 {
 let x=match sample {0=>vec![],1=>vec![None],_=>vec![Some((i8::MIN,Ok(i16::MIN))),None,Some((i8::MAX,Err(i32::MIN))),Some((-1,Err(i32::MAX)))]};
 let source=nested(&x,false);let word=nested(&x,true);
 let out=take_nested(n,Nested{values:x.clone()});println!("nested|take|{n}|{source}|{}",nested(&out,true));
 let out=make_nested(n,x.clone());println!("nested|make|{n}|{word}|{}",nested(&out.values,false));
 let out=pass_nested(n,x.clone());println!("nested|pass|{n}|{word}|{}",nested(&out,true));
 let mut out=Nested{values:vec![Some((1,Ok(2)))]};update_nested(n,&mut out,x);println!("nested|update|{n}|{word}|{}",nested(&out.values,false));
 let x=match sample {0=>BTreeMap::new(),1=>BTreeMap::from([(0,0)]),_=>BTreeMap::from([(i16::MIN,i8::MAX),(-1,-1),(i16::MAX,i8::MIN)])};
 let source=entries(&x,false);let word=entries(&x,true);
 let out=take_entries(n,Entries{values:x.clone()});println!("entries|take|{n}|{source}|{}",entries(&out,true));
 let out=make_entries(n,x.clone());println!("entries|make|{n}|{word}|{}",entries(&out.values,false));
 let out=pass_entries(n,x.clone());println!("entries|pass|{n}|{word}|{}",entries(&out,true));
 let mut out=Entries{values:BTreeMap::new()};update_entries(n,&mut out,x);println!("entries|update|{n}|{word}|{}",entries(&out.values,false));
}}}
"#;
    let dir = scratch(
        "structural-carriers",
        &[
            (
                "Cargo.toml",
                "[package]\nname=\"carriers\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n",
            ),
            ("src/lib.rs", &code),
            ("src/bin/vectors.rs", oracle),
        ],
    );
    let dumps = dumped(&dir, "structural-carriers");
    let mut opts = options("carriers");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposals = uorust::project_workspace(&dir, &opts).unwrap();
    uorust::write(&dir.join("proposal"), &proposals).unwrap();
    let mut candidates = frontier_rows(&proposals, "candidate");
    candidates.extend(frontier_rows(&proposals, "lifted"));
    for shape in ["nested", "entries"] {
        for mode in ["take", "make", "pass", "update"] {
            let path = format!("carriers::{mode}_{shape}");
            assert!(
                candidates
                    .iter()
                    .any(|r| r["function"].as_str() == Some(&path)),
                "{path}: {:?}",
                residue_detail(&proposals, &path)
            );
        }
    }
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("structural-carriers-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("carriers"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps.clone(),
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let rust = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--locked", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("carriers::{}_{}", p[1], p[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap_or_else(|| panic!("{path}: {:?}", residue_detail(&files, &path)))
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let record = if p[0] == "nested" {
            "Nested"
        } else {
            "Entries"
        };
        let call = match p[1] {
            "take" => format!("({name} {} (record {record} {{values := {}}}))", p[2], p[3]),
            "update" => {
                let empty = if p[0] == "nested" {
                    "(nil (Option TupleIntResultIntInt))"
                } else {
                    "(nil TupleIntInt)"
                };
                format!("(project (project ({name} {} (record {record} {{values := {empty}}}) {}) item1) values)",p[2],p[3])
            }
            "make" => format!("(project ({name} {} {}) values)", p[2], p[3]),
            _ => format!("({name} {} {})", p[2], p[3]),
        };
        let word = matches!(p[1], "take" | "pass");
        let comparator = format!(
            "vector{}{}",
            if p[0] == "nested" {
                "Nested"
            } else {
                "Entries"
            },
            if word { "Word" } else { "Source" }
        );
        let theorem=format!("theorem{{name: structuralVector{i}, parameters: [], statement: (({comparator} {call} {}) = true), proof: decide{{}}, axioms: [{}]}}\n",p[4],policy.join(", "));
        if p[0] == "nested" && p[1] == "take" && p[4].contains("128") && wrong.is_none() {
            wrong = Some((
                theorem.clone(),
                theorem.replacen("item0 := 128", "item0 := 129", 1),
            ));
        }
        vectors.push_str(&theorem);
    }
    assert_eq!(vectors.lines().count(), 48);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    // SDK records do not promise a synthesized DecidableEq instance. Compare
    // every element and constructor explicitly, independently of the codec.
    let mut comparisons = String::new();
    for word in [false, true] {
        let kind = if word { "Word" } else { "Source" };
        let scalar = if word { "Nat" } else { "Int64" };
        let eq = |a: &str, b: &str| {
            if word {
                format!("({a} =? {b})")
            } else {
                format!("(prim equal {a} {b} : Bool)")
            }
        };
        for nested in [false, true] {
            let (label, elem) = if nested {
                (
                    "Nested",
                    format!("(Option Tuple{scalar}Result{scalar}{scalar})").replace("Int64", "Int"),
                )
            } else {
                (
                    "Entries",
                    format!("Tuple{scalar}{scalar}").replace("Int64", "Int"),
                )
            };
            let name = format!("vector{label}{kind}");
            let item = if nested {
                let first = eq(
                    "(project vectorLeftTuple item0)",
                    "(project vectorRightTuple item0)",
                );
                let ok = eq("vectorLeftResult", "vectorRightResult");
                format!("(match vectorLeftItem | Option.none => (match vectorRightItem | Option.none => true | Option.some vectorRightTuple => false) | Option.some vectorLeftTuple => (match vectorRightItem | Option.none => false | Option.some vectorRightTuple => ({first} && (match (project vectorLeftTuple item1) | Result.ok vectorLeftResult => (match (project vectorRightTuple item1) | Result.ok vectorRightResult => {ok} | Result.error vectorRightResult => false) | Result.error vectorLeftResult => (match (project vectorRightTuple item1) | Result.ok vectorRightResult => false | Result.error vectorRightResult => {ok})))) )")
            } else {
                format!(
                    "({} && {})",
                    eq(
                        "(project vectorLeftItem item0)",
                        "(project vectorRightItem item0)"
                    ),
                    eq(
                        "(project vectorLeftItem item1)",
                        "(project vectorRightItem item1)"
                    )
                )
            };
            comparisons.push_str(&format!("definition{{name:{name},parameters:[vectorLeftList:(List {elem}),vectorRightList:(List {elem})],result:Bool,body:(match vectorLeftList | List.nil => (match vectorRightList | List.nil => true | List.cons vectorRightItem vectorRightTail => false) | List.cons vectorLeftItem vectorLeftTail => (match vectorRightList | List.nil => false | List.cons vectorRightItem vectorRightTail => ({item} && ({name} vectorLeftTail vectorRightTail)))), recursive_argument:vectorLeftList}}\n"));
        }
    }
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{comparisons}{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    for (old, new, path) in [
        (
            "pub values:Vec<Option<(i8,Result<i16,i32>)>>",
            "pub values:Vec<Option<(bool,Result<i16,i32>)>>",
            "carriers::take_nested",
        ),
        (
            "pub values:BTreeMap<i16,i8>",
            "pub values:std::collections::HashMap<i16,i8>",
            "carriers::take_entries",
        ),
    ] {
        let changed = code.replacen(old, new, 1);
        assert_ne!(changed, code);
        std::fs::write(dir.join("src/lib.rs"), changed).unwrap();
        opts.mir = Some(mir_options(
            dumps.clone(),
            Default::default(),
            uorust::mir::Trial::All,
        ));
        let drift = uorust::project_workspace(&dir, &opts).unwrap();
        assert!(
            residue_detail(&drift, path).is_some_and(|s| s.contains("field representation")),
            "{path}: {:?}",
            residue_detail(&drift, path)
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-75: constructors, source arguments and state share signed word kernels.
#[test]
fn native_words_reuse_signed_source_carriers_ue_75() {
    let mut code = String::new();
    let mut oracle = String::from("use signed_boundaries::*;fn main(){\n");
    for bits in [8, 16, 32, 64] {
        code.push_str(&format!("pub struct Signed{bits}{{pub value:i{bits}}}\npub fn identity{bits}(value:i{bits})->i{bits}{{value}}\npub fn make{bits}(mut n:u8,value:i{bits})->Signed{bits}{{while n>0{{n-=1;}}Signed{bits}{{value}}}}\npub fn pass{bits}(mut n:u8,value:i{bits})->i{bits}{{while n>0{{n-=1;}}identity{bits}(value)}}\npub fn update{bits}(mut n:u8,x:&mut Signed{bits},value:i{bits})->i{bits}{{while n>0{{n-=1;}}x.value=value;x.value}}\n"));
        oracle.push_str(&format!("for value in [i{bits}::MIN,-1,0,1,i{bits}::MAX]{{for n in [0,2]{{let raw=value as u{bits};println!(\"make|{bits}|{{n}}|{{raw}}|{{}}\",make{bits}(n,value).value);println!(\"pass|{bits}|{{n}}|{{raw}}|{{}}\",pass{bits}(n,value) as u{bits});let mut x=Signed{bits}{{value:-1}};let out=update{bits}(n,&mut x,value);println!(\"update|{bits}|{{n}}|{{raw}}|{{}}|{{}}\",out as u{bits},x.value);}}}}\n"));
    }
    oracle.push_str("}\n");
    let dir=scratch("signed-source-boundaries",&[
        ("Cargo.toml","[package]\nname=\"signed_boundaries\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",&code),("src/bin/vectors.rs",&oracle),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "signed-source-boundaries");
    let mut opts = options("signed_boundaries");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    for bits in [8, 16, 32, 64] {
        for mode in ["make", "pass", "update"] {
            let path = format!("signed_boundaries::{mode}{bits}");
            assert!(
                residue_detail(&proposed, &path).is_none(),
                "{path}: {:?}",
                residue_detail(&proposed, &path)
            );
        }
    }
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("signed-source-boundary-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("signed_boundaries"),
        &dumps,
        &cache,
        &dir.join("trial"),
        12,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps.clone(),
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("signed_boundaries::{}{}", p[0], p[1]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap_or_else(|| panic!("{path}: {:?}", residue_detail(&files, &path)))
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let statement = match p[0] {
            "make" => format!(
                "((project ({name} {} {}) value) = ({} : Int64))",
                p[2], p[3], p[4]
            ),
            "pass" => format!("(({name} {} {}) = {})", p[2], p[3], p[4]),
            _ => {
                let call = format!(
                    "({name} {} (record Signed{} {{value := (-1 : Int64)}}) {})",
                    p[2], p[1], p[3]
                );
                format!("(((project {call} item0) =? {}) && (prim equal (project (project {call} item1) value) ({} : Int64) : Bool)) = true",p[4],p[5])
            }
        };
        let statement = if p[0] == "update" {
            format!("({statement})")
        } else {
            statement
        };
        let theorem=format!("theorem{{name: signedBoundaryVector{i}, parameters: [], statement: {statement}, proof: decide{{}}, axioms: [{}]}}\n",policy.join(", "));
        if p[0] == "pass" && p[4] == "0" && wrong.is_none() {
            wrong = Some((theorem.clone(), theorem.replacen("= 0)", "= 1)", 1)));
        }
        vectors.push_str(&theorem);
    }
    assert_eq!(vectors.lines().count(), 120);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::write(
        dir.join("src/lib.rs"),
        code.replacen("value:i8", "value:bool", 1),
    )
    .unwrap();
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let mismatched = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(residue_detail(&mismatched, "signed_boundaries::make8")
        .is_some_and(|s| s.contains("field representation")));
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-74: each dependency context retains its original SDK decision.
#[test]
fn native_receipts_survive_context_alternation_ue_74() {
    let original="pub fn source(value:bool)->bool {value}\npub fn query(mut n:u8)->bool {while n>0 {n-=1;}source(true)}\n";
    let dir=scratch("native-context-alternation",&[
        ("Cargo.toml","[package]\nname=\"context_receipts\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",original),
    ]);
    let dumps = dumped(&dir, "native-context-alternation");
    let a = options("context_receipts");
    let scaffolding = uorust::project_workspace(&dir, &a).unwrap();
    let ll: toml::Value = text(&scaffolding, "lexlean.toml").parse().unwrap();
    let pm: toml::Value = text(&scaffolding, "prismpm.toml").parse().unwrap();
    let limits = toml::Value::Table(
        [
            ("lexlean".to_string(), ll["limits"].clone()),
            ("prismpm".to_string(), pm["limits"].clone()),
        ]
        .into_iter()
        .collect(),
    );
    let limit_path = dir.join("limits.toml");
    std::fs::write(&limit_path, toml::to_string(&limits).unwrap()).unwrap();
    let mut b = a.clone();
    b.limits = Some(limit_path);
    let cache = dir.join("decisions.toml");
    let scratch = dir.join("trial");
    let first = uorust::mir::prove_bounded(&dir, &a, &dumps, &cache, &scratch, 1).unwrap();
    assert_eq!(first.refused, 0);
    let first_register = verdicts(&cache);
    let (key, first_row) = first_register
        .rows
        .iter()
        .find(|(_, r)| r.function == "context_receipts::query")
        .unwrap();
    let first_context = first_row.model_context_sha256.as_ref().unwrap();
    assert!(matches!(
        first_row.status,
        uorust::mir::verdict::Decision::Proved
    ));
    let first_snapshot = dir.join("first-verdicts.toml");
    std::fs::write(&first_snapshot, first_register.to_text()).unwrap();
    let second = uorust::mir::prove_bounded(&dir, &b, &dumps, &cache, &scratch, 1).unwrap();
    assert_eq!(second.refused, 0);
    let second_register = verdicts(&cache);
    assert_ne!(
        first_context,
        second_register.rows[key]
            .model_context_sha256
            .as_ref()
            .unwrap()
    );
    let saved = std::fs::read(&cache).unwrap();
    assert!(
        String::from_utf8_lossy(&saved).contains(first_context),
        "the original SDK context was discarded"
    );
    assert!(
        second_register.historical[key].contains(first_row),
        "the original decision fields changed"
    );
    let imported = dir.join("imported-decisions.toml");
    let mut only_latest = second_register.clone();
    only_latest.historical.clear();
    std::fs::write(&imported, only_latest.to_text()).unwrap();
    uorust::mir::verdict::Verdicts::import_files(&imported, &[first_snapshot]).unwrap();
    let imported_bytes = std::fs::read(&imported).unwrap();
    for context in [&a, &b] {
        let replay =
            uorust::mir::prove_bounded(&dir, context, &dumps, &imported, &scratch, 1).unwrap();
        assert_eq!((replay.proved, replay.refused, replay.runs), (0, 0, 0));
        assert_eq!(std::fs::read(&imported).unwrap(), imported_bytes);
    }
    let mut corrupted = second_register.clone();
    corrupted.historical.get_mut(key).unwrap()[0].model_context_sha256 = Some("0".repeat(64));
    let mut rejected = a.clone();
    rejected.mir = Some(mir_options(
        dumps.clone(),
        corrupted,
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &rejected).unwrap();
    assert!(
        residue_detail(&files, "context_receipts::query").is_some(),
        "a corrupted historic context acquired a current proof"
    );
    for context in [&a, &b, &a, &b] {
        let replay =
            uorust::mir::prove_bounded(&dir, context, &dumps, &cache, &scratch, 1).unwrap();
        assert_eq!(
            (replay.proved, replay.refused, replay.runs),
            (0, 0, 0),
            "an intact context was verified again"
        );
        assert_eq!(
            std::fs::read(&cache).unwrap(),
            saved,
            "replay rewrote original decision records"
        );
    }
    std::fs::write(
        dir.join("src/lib.rs"),
        original.replace("{value}", "{!value}"),
    )
    .unwrap();
    let changed = uorust::mir::prove_bounded(&dir, &a, &dumps, &cache, &scratch, 1).unwrap();
    assert!(
        changed.runs > 0 && changed.proved > 0,
        "a changed source dependency reused an old decision"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-73: source fixed-width signed fields reuse the common word codec.
#[test]
fn fixed_signed_source_fields_share_integer_codec_ue_73() {
    let mut code = String::new();
    let mut oracle = String::from("use fixed_fields::*;fn main(){\n");
    for bits in [8, 16, 32, 64] {
        code.push_str(&format!("pub struct Signed{bits} {{pub value:i{bits}}}\npub fn read{bits}(mut n:u8,x:&Signed{bits})->i{bits} {{while n>0 {{n-=1;}}x.value}}\npub fn make{bits}(mut n:u8,value:i{bits})->Signed{bits} {{while n>0 {{n-=1;}}Signed{bits}{{value}}}}\npub fn source{bits}(x:&Signed{bits})->i{bits} {{x.value}}\npub fn call{bits}(mut n:u8,x:&Signed{bits})->i{bits} {{while n>0 {{n-=1;}}source{bits}(x)}}\n"));
        oracle.push_str(&format!("for value in [i{bits}::MIN,-1,0,1,i{bits}::MAX] {{for n in [0,2] {{for (mode,got) in [(\"read\",read{bits}(n,&Signed{bits}{{value}})),(\"call\",call{bits}(n,&Signed{bits}{{value}}))] {{println!(\"{bits}|{{mode}}|{{n}}|{{value}}|{{}}\",got as u{bits});}}}}}}\n"));
    }
    oracle.push_str("}\n");
    let dir=scratch("fixed-source-fields",&[
        ("Cargo.toml","[package]\nname=\"fixed_fields\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",&code),("src/bin/vectors.rs",&oracle),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "fixed-source-fields");
    let mut opts = options("fixed_fields");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    for bits in [8, 16, 32, 64] {
        let path = format!("fixed_fields::read{bits}");
        assert!(
            residue_detail(&proposed, &path).is_none(),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
        assert!(
            residue_detail(&proposed, &format!("fixed_fields::call{bits}")).is_none(),
            "source result codec not shared"
        );
        assert!(
            residue_detail(&proposed, &format!("fixed_fields::make{bits}")).is_none(),
            "the admitted reverse codec was not reused"
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fixed-source-field-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("fixed_fields"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps.clone(),
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let bits = p[0];
        let path = format!("fixed_fields::{}{bits}", p[1]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap_or_else(|| panic!("{path}: {:?}", residue_detail(&files, &path)))
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let theorem=format!("theorem{{name: fixedFieldVector{i}, parameters: [], statement: (({name} {} (record Signed{bits} {{value := ({} : Int64)}})) = {}), proof: decide{{}}, axioms: [{}]}}\n",p[2],p[3],p[4],policy.join(", "));
        if p[4] == "0" && wrong.is_none() {
            wrong = Some((theorem.clone(), theorem.replacen("= 0)", "= 1)", 1)));
        }
        vectors.push_str(&theorem);
    }
    assert_eq!(vectors.lines().count(), 80);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::write(
        dir.join("src/lib.rs"),
        code.replacen("value:i8", "value:i16", 1),
    )
    .unwrap();
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let mismatched = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(residue_detail(&mismatched, "fixed_fields::read8")
        .is_some_and(|s| s.contains("field representation")));
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-72: native constants import admitted source definitions, never guessed values.
#[test]
fn compiler_constants_reuse_source_dependencies_ue_72() {
    let dir = scratch("source-constant-dependencies", &[
        ("Cargo.toml", "[package]\nname=\"constant_models\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub mod left { pub const VALUE:u32=7; }
pub mod right { pub const VALUE:u32=19; }
pub const FLAG:bool=true;
pub const SIGNED:i64=-3;
pub const UNAVAILABLE:usize=std::mem::size_of::<usize>();
pub fn left_value(mut n:u8)->u32 {while n>0 {n-=1;}left::VALUE}
pub fn right_value(mut n:u8)->u32 {while n>0 {n-=1;}right::VALUE}
pub fn flag(mut n:u8)->bool {while n>0 {n-=1;}FLAG}
pub fn signed(mut n:u8)->i64 {while n>0 {n-=1;}SIGNED}
pub fn unavailable(mut n:u8)->usize {while n>0 {n-=1;}UNAVAILABLE}
"#),
        ("src/bin/vectors.rs", r#"
use constant_models::*;
fn main(){for n in [0,1,3,255] {
println!("left_value|{n}|{}",left_value(n));
println!("right_value|{n}|{}",right_value(n));
println!("flag|{n}|{}",flag(n));
}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "source-constant-dependencies");
    let mut opts = options("constant_models");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    for f in ["left_value", "right_value", "flag"] {
        let path = format!("constant_models::{f}");
        assert!(
            residue_detail(&proposed, &path).is_none(),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    for f in ["signed", "unavailable"] {
        assert!(
            residue_detail(&proposed, &format!("constant_models::{f}")).is_some(),
            "{f} acquired an unavailable constant meaning"
        );
    }
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("source-constant-dependency-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("constant_models"),
        &dumps,
        &cache,
        &dir.join("trial"),
        3,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("constant_models::{}", p[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap_or_else(|| panic!("{path}: {:?}", residue_detail(&files, &path)))
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let theorem=format!("theorem{{name: constantVector{i}, parameters: [], statement: (({name} {}) = {}), proof: decide{{}}, axioms: [{}]}}\n",p[1],p[2],policy.join(", "));
        if p[0] == "left_value" && wrong.is_none() {
            wrong = Some((theorem.clone(), theorem.replacen("= 7)", "= 8)", 1)));
        }
        vectors.push_str(&theorem);
    }
    assert_eq!(vectors.lines().count(), 12);
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    let source_path = dir.join("src/lib.rs");
    let original = std::fs::read_to_string(&source_path).unwrap();
    let changed = original.replacen("VALUE:u32=7", "VALUE:u32=8", 1);
    assert_ne!(original, changed);
    std::fs::write(&source_path, changed).unwrap();
    assert!(
        matches!(uorust::project_workspace(&dir,&opts),Err(UorustError::Tool(message)) if message.contains("compiler definitions: source, configuration or pinned compiler binding mismatch")),
        "changed source must reject the previous compiler capture"
    );
    opts.mir.as_mut().unwrap().dumps = dumped(&dir, "source-constant-dependencies-changed");
    let stale = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        residue_detail(&stale, "constant_models::left_value").is_some(),
        "a changed initializer reused its previous native decision"
    );
    assert!(
        residue_detail(&stale, "constant_models::right_value").is_none(),
        "an unrelated constant invalidated an intact rooted decision"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-71: generic entries cannot create unbound global tuple schemas.
#[test]
fn parametric_map_carriers_share_sdk_products_ue_71() {
    let dir=scratch("parametric-map-carriers",&[("Cargo.toml","[package]\nname=\"map_carriers\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),("src/lib.rs",r#"
use std::collections::{HashMap,BTreeMap};
pub fn keep<K,V>(mut n:u8,m:HashMap<K,V>)->HashMap<K,V> {while n>0 {n-=1;}m}
pub fn length<K,V>(mut n:u8,m:&HashMap<K,V>)->usize {while n>0 {n-=1;}m.len()}
pub fn source_length<K,V>(m:&HashMap<K,V>)->usize {m.len()}
pub fn concrete_keep(m:HashMap<u8,bool>)->HashMap<u8,bool> {keep(0,m)}
pub fn concrete_natural_keep(m:HashMap<u8,u8>)->HashMap<u8,u8> {keep(0,m)}
pub fn get<V>(mut n:u8,m:&HashMap<u8,V>,k:u8)->Option<&V> {while n>0 {n-=1;}m.get(&k)}
pub fn source_get<V>(m:&HashMap<u8,V>,k:u8)->Option<&V> {m.get(&k)}
pub fn tree_get<V>(m:&BTreeMap<u8,V>,k:u8)->Option<&V> {m.get(&k)}
pub fn concrete_length(m:&HashMap<u8,bool>)->usize {source_length(m)}
pub fn native_source_length(mut n:u8,m:&HashMap<u8,bool>)->usize {while n>0 {n-=1;}source_length(m)}
pub fn unavailable<'a,K:std::cmp::Eq+std::hash::Hash,V>(mut n:u8,m:&'a HashMap<K,V>,k:&K)->Option<&'a V> {while n>0 {n-=1;}m.get(k)}
pub fn discard<K,V>(mut n:u8,m:HashMap<K,V>)->usize {while n>0 {n-=1;}m.len()}
"#),("src/bin/vectors.rs",r#"
use map_carriers::*;use std::collections::{HashMap,BTreeMap};use std::fmt::{Display,Debug};
fn emit<V:Copy+PartialEq+Display+Debug>(tag:&str,m:HashMap<u8,V>) {
 for n in [0,2] {
  let kept=keep(n,m.clone());assert_eq!(kept,m);println!("keep|{tag}|{n}|{}|{}",kept[&1],kept[&3]);
  println!("length|{tag}|{n}|{}",length(n,&m));
  for k in [0,1,3] {println!("get|{tag}|{n}|{k}|{}",get(n,&m,k).map(|v|v.to_string()).unwrap_or("none".into()));}
 }
 println!("source_length|{tag}|0|{}",source_length(&m));
 let tree:BTreeMap<u8,V>=m.iter().map(|(&k,&v)|(k,v)).collect();
 for k in [0,1,3] {
  println!("source_get|{tag}|0|{k}|{}",source_get(&m,k).map(|v|v.to_string()).unwrap_or("none".into()));
  println!("tree_get|{tag}|0|{k}|{}",tree_get(&tree,k).map(|v|v.to_string()).unwrap_or("none".into()));
 }
}
fn main(){
 let b=HashMap::from([(1u8,false),(3,true)]);let n=HashMap::from([(1u8,8u8),(3,23)]);
 emit("Bool",b.clone());emit("Nat",n.clone());
 let kept=concrete_keep(b.clone());println!("concrete_keep|Bool|0|{}|{}",kept[&1],kept[&3]);
 let kept=concrete_natural_keep(n);println!("concrete_natural_keep|Nat|0|{}|{}",kept[&1],kept[&3]);
 println!("concrete_length|Bool|0|{}",concrete_length(&b));
 for n in [0,2] {println!("native_source_length|Bool|{n}|{}",native_source_length(n,&b));}
}
"#)]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "parametric-map-carriers");
    let mut opts = options("map_carriers");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let checked = std::process::Command::new("lexlean")
        .args([
            "check",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    for function in ["unavailable", "discard"] {
        assert!(
            residue_detail(&files, &format!("map_carriers::{function}")).is_some(),
            "{function} acquired unavailable semantics"
        );
    }
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("parametric-map-carrier-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("map_carriers"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    assert!(
        !decisions
            .rows
            .values()
            .any(|v| v.function == "map_carriers::concrete_length"),
        "the source generic-call bridge did not compose"
    );
    let output = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(output.lines().count(), 39);
    let mut vectors = String::new();
    let mut wrong = None;
    for (i, line) in output.lines().enumerate() {
        let p: Vec<_> = line.split('|').collect();
        let path = format!("map_carriers::{}", p[0]);
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap_or_else(|| panic!("{path}: {:?}", residue_detail(&files, &path)))
            .strip_prefix("Behavior::")
            .unwrap();
        let (first, last) = if p[1] == "Bool" {
            ("false", "true")
        } else {
            ("8", "23")
        };
        let closed = p[0].starts_with("concrete_") || p[0] == "native_source_length";
        let entry = |key: &str, value: &str| {
            if closed {
                format!(
                    "(record TupleNat{} {{item0 := {key}, item1 := {value}}})",
                    p[1]
                )
            } else {
                format!("({key}, {value})")
            }
        };
        let entry_type = if closed {
            format!("TupleNat{}", p[1])
        } else {
            format!("(Nat * {})", p[1])
        };
        let list = |a: &str, b: &str| {
            format!(
                "(cons {} (cons {} (nil {entry_type})))",
                entry("1", a),
                entry("3", b)
            )
        };
        let input = list(first, last);
        let generic = if closed {
            String::new()
        } else if ["get", "source_get", "tree_get"].contains(&p[0]) {
            format!("[{}] ", p[1])
        } else {
            format!("[Nat, {}] ", p[1])
        };
        let prefix = if ["keep", "length", "get", "native_source_length"].contains(&p[0]) {
            format!("{} ", p[2])
        } else {
            String::new()
        };
        let (args, expected) = if ["keep", "concrete_keep", "concrete_natural_keep"].contains(&p[0])
        {
            (String::new(), list(p[3], p[4]))
        } else if ["get", "source_get", "tree_get"].contains(&p[0]) {
            (
                format!(" {}", p[3]),
                if p[4] == "none" {
                    format!("(ctor Option.none [{}])", p[1])
                } else {
                    format!("(ctor Option.some [{}] {})", p[1], p[4])
                },
            )
        } else {
            (String::new(), p[3].to_string())
        };
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let call = format!("({name} {generic}{prefix}{input}{args})");
        let statement = if closed && ["concrete_keep", "concrete_natural_keep"].contains(&p[0]) {
            let eq_value = |item: &str, value: &str| {
                if p[1] == "Bool" {
                    format!("(prim equal (project {item} item1) {value} : Bool)")
                } else {
                    format!("((project {item} item1) =? {value})")
                }
            };
            let tests = [
                "((project carrierFirst item0) =? 1)".to_string(),
                eq_value("carrierFirst", p[3]),
                "((project carrierSecond item0) =? 3)".to_string(),
                eq_value("carrierSecond", p[4]),
                "((prim length carrierTail : Nat) =? 0)".to_string(),
            ];
            let test = tests
                .into_iter()
                .reduce(|a, b| format!("({a} && {b})"))
                .unwrap();
            format!("((match {call} | List.nil => false | List.cons carrierFirst carrierRest => (match carrierRest | List.nil => false | List.cons carrierSecond carrierTail => {test})) = true)")
        } else {
            format!("({call} = {expected})")
        };
        let theorem = format!("theorem{{name: mapCarrierVector{i}, parameters: [], statement: {statement}, proof: decide{{}}, axioms: [{}]}}\n",policy.join(", "));
        if p[0] == "length" && wrong.is_none() {
            wrong = Some((theorem.clone(), theorem.replacen("= 2)", "= 3)", 1)));
        }
        vectors.push_str(&theorem);
    }
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    std::fs::write(out.join("src/Behavior.lex.tex"), &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let (good, bad) = wrong.unwrap();
    assert_ne!(good, bad);
    std::fs::write(
        out.join("src/Behavior.lex.tex"),
        body.replacen(&good, &bad, 1),
    )
    .unwrap();
    let rejected = verify();
    let diagnostic = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        !rejected.status.success()
            && diagnostic.contains("LLV7002")
            && diagnostic.contains("is false"),
        "{diagnostic}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-87: the public compiler path specializes equal opaque source slots.
#[test]
fn compiler_type_equalities_specialize_source_frames_ue_87() {
    let dir = scratch("carrier-frames", &[
        ("Cargo.toml", "[package]\nname=\"carrier_frames\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub trait Service {fn run(&self)->bool;}
pub struct Pair {pub left:&'static dyn Service,pub right:&'static dyn Service}
pub fn choose(mut n:u8,x:&Pair,right:bool)->&'static dyn Service {while n>0 {n-=1;} if right {x.right} else {x.left}}
pub fn unknown(mut n:u8,x:&Pair)->bool {while n>0 {n-=1;} x.left.run()}
"#),
    ]);
    let dumps = dumped(&dir, "carrier-frames");
    let mut opts = options("carrier_frames");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        text(&files, uorust::mir::CANDIDATES).contains("carrier_frames::choose\""),
        "{:?}",
        residue_detail(&files, "carrier_frames::choose")
    );
    assert!(residue_detail(&files, "carrier_frames::unknown").is_some());
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-88: one actual constructor suffices without an artificial discriminant.
#[test]
fn compiler_single_constructor_fields_share_projection_kernels_ue_88() {
    let dir=scratch("single-constructor-fields",&[
        ("Cargo.toml","[package]\nname=\"single_constructor\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",r#"
pub enum Envelope {Only(u8,bool)}
pub enum Choice {A(u8),B(u8)}
pub fn number(mut n:u8,x:&Envelope)->u8 {while n>0 {n-=1;} match x {Envelope::Only(a,_)=>*a}}
pub fn flag(mut n:u8,x:&Envelope)->bool {while n>0 {n-=1;} match x {Envelope::Only(_,b)=>*b}}
pub fn again(mut n:u8,x:&Envelope)->u8 {while n>0 {n-=1;} match x {Envelope::Only(a,_)=>*a}}
pub fn select(mut n:u8,x:&Choice)->u8 {while n>0 {n-=1;} match x {Choice::A(a)=>*a,Choice::B(_)=>0}}
"#),
        ("src/bin/vectors.rs",r#"
use single_constructor::*;
fn main(){for n in [0,2] {for a in [0u8,7,255] {for b in [false,true] {
 let x=Envelope::Only(a,b);
 println!("number|{n}|{a}|{b}|{}",number(n,&x));
 println!("flag|{n}|{a}|{b}|{}",flag(n,&x));
 println!("again|{n}|{a}|{b}|{}",again(n,&x));
 for (tag,x) in [("A",Choice::A(a)),("B",Choice::B(a))] {
  println!("select{tag}|{n}|{a}|{b}|{}",select(n,&x));
 }
}}}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 60);
    let dumps = dumped(&dir, "single-constructor-fields");
    let mut opts = options("single_constructor");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for function in ["number", "flag", "again", "select"] {
        let path = format!("single_constructor::{function}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&files, &path)
        );
    }
    let body = text(&files, "src/Behavior.lex.tex");
    assert!(body.contains("mirSingletonProjection"));
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("single-constructor-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("single_constructor"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [tag, n, a, b, expected] = parts.as_slice() else {
            panic!("{line}")
        };
        let function = if tag.starts_with("select") {
            "select"
        } else {
            tag
        };
        let path = format!("single_constructor::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let value = if function == "select" {
            format!("(ctor Choice.{} {a})", tag.strip_prefix("select").unwrap())
        } else {
            format!("(ctor Envelope.Only {a} {b})")
        };
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name: singletonVector{i},parameters:[],statement:(({name} {n} {value}) = {expected}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = body.replacen(" = 0)", " = 1)", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-89: a standard source combinator transports the native signed ABI.
#[test]
fn compiler_parametric_source_calls_preserve_native_words_ue_89() {
    let dir=scratch("parametric-source-abi",&[
        ("Cargo.toml","[package]\nname=\"parametric_source\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",r#"
pub fn negative(x:i8)->bool {let mut n=1u8;while n>0 {n-=1;} x<0}
pub fn mapped(mut n:u8,x:Option<i8>)->Option<bool> {while n>0 {n-=1;} x.map(negative)}
"#),
        ("src/bin/vectors.rs",r#"
use parametric_source::*;
fn main(){for n in [0,2] {for x in [None,Some(-128i8),Some(-1),Some(0),Some(1),Some(127)] {
 let raw=x.map(|v|v as u8);let out=mapped(n,x);
 println!("{n}|{}|{}",raw.map(|v|v.to_string()).unwrap_or("none".into()),out.map(|v|v.to_string()).unwrap_or("none".into()));
}}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 12);
    let dumps = dumped(&dir, "parametric-source-abi");
    let mut opts = options("parametric_source");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        text(&files, uorust::mir::CANDIDATES).contains("parametric_source::mapped\""),
        "{:?}",
        residue_detail(&files, "parametric_source::mapped")
    );
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("parametric-source-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("parametric_source"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let name = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("parametric_source::mapped"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let policy = decisions
        .rows
        .values()
        .find(|r| r.function == "parametric_source::mapped")
        .and_then(|r| r.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [n, input, expected] = parts.as_slice() else {
            panic!("{line}")
        };
        let input = if *input == "none" {
            "(ctor Option.none [Nat])".into()
        } else {
            format!("(ctor Option.some [Nat] {input})")
        };
        let expected = if *expected == "none" {
            "(ctor Option.none [Bool])".into()
        } else {
            format!("(ctor Option.some [Bool] {expected})")
        };
        vectors.push_str(&format!("theorem{{name:parametricVector{i},parameters:[],statement:(({name} {n} {input}) = {expected}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = body.replacen(
        "(ctor Option.some [Bool] true))",
        "(ctor Option.some [Bool] false))",
        1,
    );
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-90: the admitted declaration determines a constructor's kind.
#[test]
fn compiler_aggregate_forms_follow_admitted_shapes_ue_90() {
    let dir=scratch("aggregate-source-shapes",&[
        ("Cargo.toml","[package]\nname=\"aggregate_shapes\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",r#"
pub enum Record {Named {flag:bool,number:u8},Empty}
pub enum Status {Zero,One}
#[allow(non_snake_case)] pub mod Capital {pub struct Plain {pub number:u8} pub struct Pair(pub u8,pub bool);}
pub fn make(mut n:u8,number:u8,flag:bool)->Record {while n>0 {n-=1;} Record::Named{number,flag}}
pub fn plain(mut n:u8,number:u8)->Capital::Plain {while n>0 {n-=1;} Capital::Plain{number}}
pub fn pair(mut n:u8,number:u8,flag:bool)->Capital::Pair {while n>0 {n-=1;} Capital::Pair(number,flag)}
pub fn status(mut n:u8,flag:bool)->Status {while n>0 {n-=1;} if flag {Status::One} else {Status::Zero}}
"#),
        ("src/bin/vectors.rs",r#"
use aggregate_shapes::*;
fn main(){for n in [0,2] {for number in [0u8,7,255] {for flag in [false,true] {
 let Record::Named{number:a,flag:b}=make(n,number,flag) else {panic!()};println!("make|{n}|{number}|{flag}|{a}|{b}");
 let p=plain(n,number);println!("plain|{n}|{number}|{flag}|{}|false",p.number);
 let p=pair(n,number,flag);println!("pair|{n}|{number}|{flag}|{}|{}",p.0,p.1);
 let b=matches!(status(n,flag),Status::One);println!("status|{n}|{number}|{flag}|0|{b}");
}}}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 48);
    let dumps = dumped(&dir, "aggregate-source-shapes");
    let mut opts = options("aggregate_shapes");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for f in ["make", "plain", "pair", "status"] {
        let path = format!("aggregate_shapes::{f}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{path}\"")),
            "{path}: {:?}",
            residue_detail(&files, &path)
        );
    }
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("aggregate-shapes-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("aggregate_shapes"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [tag, n, number, flag, a, b] = parts.as_slice() else {
            panic!("{line}")
        };
        let path = format!("aggregate_shapes::{tag}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == path)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let check=match *tag {
            "make"=>format!("(match ({name} {n} {number} {flag}) | Record.Named observedFlag observedNumber => ((prim equal observedFlag {b} : Bool) && (observedNumber =? {a})) | Record.Empty => false)"),
            "plain"=>format!("((project ({name} {n} {number}) number) =? {a})"),
            "pair"=>format!("(((project ({name} {n} {number} {flag}) item0) =? {a}) && (prim equal (project ({name} {n} {number} {flag}) item1) {b} : Bool))"),
            _=>format!("(prim equal (match ({name} {n} {flag}) | Status.Zero => false | Status.One => true) {b} : Bool)"),
        };
        vectors.push_str(&format!("theorem{{name:aggregateVector{i},parameters:[],statement:({check} = true),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let accepted = verify();
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let wrong = body.replacen(" =? 0)", " =? 1)", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-91: a closure's native callback must precede its auxiliary definition.
#[test]
fn compiler_callback_dependencies_precede_native_auxiliaries_ue_91() {
    let dir=scratch("callback-declaration-order",&[
        ("Cargo.toml","[package]\nname=\"callback_order\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",r#"
pub enum Choice {Left(u8),Right(u8)}
pub fn is_right(x:&Choice)->bool {let mut n=1u8;while n>0 {n-=1;} matches!(x,Choice::Right(_))}
pub fn filtered(mut n:u8,x:Option<Choice>)->Option<Choice> {while n>0 {n-=1;} x.filter(|x|!is_right(x))}
"#),
    ]);
    let dumps = dumped(&dir, "callback-declaration-order");
    let mut opts = options("callback_order");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    assert!(
        text(&files, uorust::mir::CANDIDATES).contains("callback_order::filtered\""),
        "{:?}",
        residue_detail(&files, "callback_order::filtered")
    );
    let checked = std::process::Command::new("lexlean")
        .args([
            "check",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-92: numeric source models use the same representation kernels as source dependencies.
#[test]
fn compiler_source_numeric_models_use_exact_abi_kernels_ue_92() {
    let dir = scratch("source-numeric-abi", &[
        ("Cargo.toml", "[package]\nname=\"source_numeric\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn added(mut n:u8,x:i128,y:i128)->Option<i128> {while n>0 {n-=1;} x.checked_add(y)}
pub fn multiplied(mut n:u8,x:i128,y:i128)->Option<i128> {while n>0 {n-=1;} x.checked_mul(y)}
"#),
        ("src/bin/vectors.rs", r#"
use source_numeric::*;
fn main() {
 for n in [0,2] {for (x,y) in [(i128::MIN,-1),(i128::MAX,1),(i128::MIN,1),(-1,-1),(0,0),(123,-5)] {
  for (name,result) in [("added",added(n,x,y)),("multiplied",multiplied(n,x,y))] {
   println!("{name}|{n}|{}|{}|{}",x as u128,y as u128,result.map(|v|(v as u128).to_string()).unwrap_or("none".into()));
  }
 }}
}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 24);
    let dumps = dumped(&dir, "source-numeric-abi");
    let mut opts = options("source_numeric");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for name in ["added", "multiplied"] {
        let key = format!("source_numeric::{name}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{key}\"")),
            "{:?}",
            residue_detail(&files, &key)
        );
    }
    let cache = dir.join("numeric-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("source_numeric"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [function, n, x, y, result] = parts.as_slice() else {
            panic!("{line}")
        };
        let key = format!("source_numeric::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&key))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == key)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let expected = if *result == "none" {
            "(ctor Option.none [Nat])".into()
        } else {
            format!("(ctor Option.some [Nat] {result})")
        };
        vectors.push_str(&format!("theorem{{name:numericVector{i},parameters:[],statement:(({name} {n} {x} {y}) = {expected}),proof:decide{{}},axioms:[{}]}}\n", policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(
        "= (ctor Option.some [Nat] 1))",
        "= (ctor Option.some [Nat] 2))",
        1,
    );
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// UE-93: the compiler supplies the closed primitive implementation identity.
#[test]
fn compiler_primitive_trait_calls_reuse_semantic_abi_kernels_ue_93() {
    let dir = scratch("primitive-trait-abi", &[
        ("Cargo.toml", "[package]\nname=\"primitive_trait\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn greatest(mut n:u8,x:i32,y:i32)->i32 {while n>0 {n-=1;} x.max(y)}
pub fn least(mut n:u8,x:i32,y:i32)->i32 {while n>0 {n-=1;} x.min(y)}
"#),
        ("src/bin/vectors.rs", r#"
use primitive_trait::*;
fn main() {for n in [0,2] {for (x,y) in [(i32::MIN,i32::MAX),(i32::MIN,i32::MIN),(-1,0),(i32::MAX,-1)] {
 for (name,result) in [("greatest",greatest(n,x,y)),("least",least(n,x,y))] {
  println!("{name}|{n}|{}|{}|{}",x as u32,y as u32,result as u32);
 }
}}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 16);
    let dumps = dumped(&dir, "primitive-trait-abi");
    let mut opts = options("primitive_trait");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for function in ["greatest", "least"] {
        let key = format!("primitive_trait::{function}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{key}\"")),
            "{:?}",
            residue_detail(&files, &key)
        );
    }
    let cache = dir.join("primitive-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("primitive_trait"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [function, n, x, y, expected] = parts.as_slice() else {
            panic!("{line}")
        };
        let key = format!("primitive_trait::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&key))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == key)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name:primitiveVector{i},parameters:[],statement:(({name} {n} {x} {y}) = {expected}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = 2147483647)", " = 2147483646)", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_primitive_arguments_keep_projected_and_literal_types_ue_93() {
    let dir = scratch("primitive-operand-types", &[
        ("Cargo.toml", "[package]\nname=\"primitive_operands\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub struct Bounds { pub value:i32 }
pub fn capped(mut n:u8,b:Bounds)->i32 {while n>0 {n-=1;} b.value.max(0)}
"#),
    ]);
    let dumps = dumped(&dir, "primitive-operand-types");
    let mut opts = options("primitive_operands");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        text(&files, uorust::mir::CANDIDATES).contains("primitive_operands::capped\""),
        "{:?}",
        residue_detail(&files, "primitive_operands::capped")
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let checked = std::process::Command::new("lexlean")
        .args([
            "check",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_source_imports_bind_the_captured_pointer_abi_ue_94() {
    let dir = scratch("captured-pointer-abi", &[
        ("Cargo.toml", "[package]\nname=\"captured_pointer\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn scaled(mut n:u8,x:usize,y:usize)->Option<usize> {while n>0 {n-=1;} x.checked_mul(y)}
"#),
        ("src/bin/vectors.rs", r#"
use captured_pointer::*;
fn main() {
 for n in [0,2] {for (x,y) in [(0,usize::MAX),(usize::MAX,1),(usize::MAX,2),(1,0),(usize::MAX/2,2),(usize::MAX/2+1,2)] {
  println!("scaled|{n}|{x}|{y}|{}",scaled(n,x,y).map(|v|v.to_string()).unwrap_or("none".into()));
 }}
}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 12);
    let dumps = dumped(&dir, "captured-pointer-abi");
    let mut opts = options("captured_pointer");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    {
        let name = "scaled";
        let key = format!("captured_pointer::{name}");
        assert!(
            text(&files, uorust::mir::CANDIDATES).contains(&format!("{key}\"")),
            "{:?}",
            residue_detail(&files, &key)
        );
    }
    let cache = dir.join("captured-pointer-verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("captured_pointer"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let [function, n, x, y, result] = parts.as_slice() else {
            panic!("{line}")
        };
        let key = format!("captured_pointer::{function}");
        let name = rows
            .iter()
            .find(|r| r["function"].as_str() == Some(&key))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|r| r.function == key)
            .and_then(|r| r.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let expected = if *result == "none" {
            "(ctor Option.none [Nat])".into()
        } else {
            format!("(ctor Option.some [Nat] {result})")
        };
        vectors.push_str(&format!("theorem{{name:pointerVector{i},parameters:[],statement:(({name} {n} {x} {y}) = {expected}),proof:decide{{}},axioms:[{}]}}\n", policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(
        "= (ctor Option.some [Nat] 0))",
        "= (ctor Option.some [Nat] 1))",
        1,
    );
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_target_metadata_refuses_malformed_toml_ue_94() {
    let dir = scratch("malformed-compiler-target", &[
        ("Cargo.toml", "[package]\nname=\"bad_target\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("Cargo.lock", "version = 4\n[[package]]\nname = \"bad_target\"\nversion = \"0.1.0\"\n"),
        ("src/lib.rs", "pub fn pending() { loop {} }"),
        ("dumps/configuration.toml", "target = ["),
    ]);
    let mut opts = options("bad_target");
    opts.mir = Some(mir_options(
        dir.join("dumps"),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    assert!(matches!(
        uorust::project_workspace(&dir, &opts),
        Err(uorust::UorustError::Malformed { .. })
    ));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_assertion_outcomes_preserve_value_state_ue_95() {
    let dir = scratch("compiler-assertion-outcomes", &[
        ("Cargo.toml", "[package]\nname=\"assertion_outcomes\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn step(x:&mut u8,delta:u8)->u8 {
 let mut n=1u8; while n>0 {n-=1;}
 *x=x.wrapping_add(1);
 let next=*x+delta;
 *x=next;
 next
}
"#),
        ("src/bin/vectors.rs", r#"
use assertion_outcomes::step;
fn main() {for (initial,delta) in [(0u8,0u8),(0,255),(254,0),(254,1),(255,0),(255,255),(200,54),(200,55)] {
 let mut state=initial;
 let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||step(&mut state,delta)));
 match result {Ok(value)=>println!("{initial}|{delta}|ok|{value}|{state}"),Err(_)=>println!("{initial}|{delta}|panic|0|{state}")}
}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 8);
    let dumps = dumped(&dir, "compiler-assertion-outcomes");
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("assertion_outcomes"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("assertion_outcomes");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    assert!(
        rows.iter()
            .any(|r| r["function"].as_str() == Some("assertion_outcomes::step")),
        "{:?}",
        residue_detail(&files, "assertion_outcomes::step")
    );
    assert!(text(&files, "src/Behavior.lex.tex").contains("Result.error"));
    let name = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("assertion_outcomes::step"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|r| r.function == "assertion_outcomes::step")
        .and_then(|r| r.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let fields: Vec<_> = line.split('|').collect();
        let [initial, delta, kind, value, state] = fields.as_slice() else {
            panic!("{line}")
        };
        let expected = if *kind == "ok" {
            value.to_string()
        } else {
            "256".into()
        };
        let observed = format!("(match (project ({name} {initial} {delta}) item0) | Result.ok value => value | Result.error reason => 256)");
        vectors.push_str(&format!("theorem{{name:outcomeVector{i},parameters:[],statement:({observed} = {expected}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
        let tag = if *kind == "ok" {
            ""
        } else {
            "AssertKind::Overflow"
        };
        let observed = format!("(match (project ({name} {initial} {delta}) item0) | Result.ok value => \"\" | Result.error reason => reason)");
        vectors.push_str(&format!("theorem{{name:panicTagVector{i},parameters:[],statement:({observed} = \"{tag}\"),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
        vectors.push_str(&format!("theorem{{name:stateVector{i},parameters:[],statement:((project ({name} {initial} {delta}) item1) = {state}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = 255)", " = 254)", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_outcome_dependencies_preserve_state_ue_96() {
    let dir = scratch("native-outcome-dependencies", &[
        ("Cargo.toml", "[package]\nname=\"outcome_dependencies\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub trait Advance { fn step(x:u8,delta:u8)->u8; }
pub struct Worker;
impl Advance for Worker {
 fn step(x:u8,delta:u8)->u8 {
  let mut n=1u8; while n>0 {n-=1;}
  x+delta
 }
}
pub fn relay(x:&mut u8,delta:u8)->u8 {
 let mut n=1u8; while n>0 {n-=1; *x=x.wrapping_add(1); *x=<Worker as Advance>::step(*x,delta);}
 *x=x.wrapping_add(1);
 *x
}
pub fn root(x:&mut u8,delta:u8)->u8 {
 let mut n=1u8; while n>0 {n-=1;}
 relay(x,delta)
}
"#),
        ("src/bin/vectors.rs", r#"
use outcome_dependencies::root;
fn main() {for (initial,delta) in [(0u8,0u8),(0,255),(254,0),(254,1),(255,0),(255,255),(200,54),(200,55)] {
 let mut state=initial;
 let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||root(&mut state,delta)));
 match result {Ok(value)=>println!("{initial}|{delta}|ok|{value}|{state}"),Err(_)=>println!("{initial}|{delta}|panic|0|{state}")}
}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 8);
    let dumps = dumped(&dir, "native-outcome-dependencies");
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("outcome_dependencies"),
        &dumps,
        &cache,
        &dir.join("trial"),
        12,
    )
    .unwrap();
    let mut opts = options("outcome_dependencies");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    assert!(
        rows.iter()
            .any(|r| r["function"].as_str() == Some("outcome_dependencies::root")),
        "{:?}",
        residue_detail(&files, "outcome_dependencies::root")
    );
    assert!(text(&files, "src/Behavior.lex.tex").contains("Result.error"));
    let name = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("outcome_dependencies::root"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|r| r.function == "outcome_dependencies::root")
        .and_then(|r| r.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let fields: Vec<_> = line.split('|').collect();
        let [initial, delta, kind, value, state] = fields.as_slice() else {
            panic!("{line}")
        };
        let expected = if *kind == "ok" {
            value.to_string()
        } else {
            "256".into()
        };
        let observed = format!("(match (project ({name} {initial} {delta}) item0) | Result.ok value => value | Result.error reason => 256)");
        vectors.push_str(&format!("theorem{{name:outcomeVector{i},parameters:[],statement:({observed} = {expected}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
        let tag = if *kind == "ok" {
            ""
        } else {
            "AssertKind::Overflow"
        };
        let observed = format!("(match (project ({name} {initial} {delta}) item0) | Result.ok value => \"\" | Result.error reason => reason)");
        vectors.push_str(&format!("theorem{{name:panicTagVector{i},parameters:[],statement:({observed} = \"{tag}\"),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
        vectors.push_str(&format!("theorem{{name:stateVector{i},parameters:[],statement:((project ({name} {initial} {delta}) item1) = {state}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = 255)", " = 254)", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_owned_constant_namespaces_preserve_values_ue_97() {
    let dir=scratch("source-owned-constants", &[
      ("Cargo.toml", "[package]\nname=\"owned_constants\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub struct Token { pub value:u8 }
impl Token {
 pub const VALUE:u8=7;
 pub const INSTANCE:Self=Self {value:7};
}
pub struct Other;
impl Other {
 #[cfg(any())]
 pub const VALUE:u8=23;
 #[cfg(all())]
 pub const VALUE:u8=19;
}
pub fn read(x:u8)->u8 {
 let mut n=1u8;while n>0 {n-=1;}
 if x==0 {Token::INSTANCE.value} else {Other::VALUE}
}
"#),
      ("src/bin/vectors.rs", r#"fn main(){for x in [0u8,1,255] {println!("{x}|{}",owned_constants::read(x));}}"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "source-owned-constants");
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("owned_constants"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("owned_constants");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let row = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("owned_constants::read"));
    assert!(
        row.is_some(),
        "{:?}",
        residue_detail(&files, "owned_constants::read")
    );
    let name = row.unwrap()["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|v| v.function == "owned_constants::read")
        .and_then(|v| v.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let (arg, value) = line.split_once('|').unwrap();
        vectors.push_str(&format!("theorem{{name:ownedConstantVector{i},parameters:[],statement:(({name} {arg}) = {value}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = 19)", " = 18)", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_memory_outcomes_preserve_frames_ue_98() {
    let dir = scratch("native-memory-outcomes", &[
        ("Cargo.toml", "[package]\nname=\"heap_outcomes\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn leaf(x:u8,delta:u8)->u8 {
 let mut cell=x;
 let p=&raw mut cell;
 unsafe {*p=(*p).wrapping_add(1); let next=*p+delta; *p=next; next}
}
pub fn relay(x:&mut u8,delta:u8)->u8 {
 let mut n=1u8;while n>0 {n-=1;}
 *x=x.wrapping_add(1);
 leaf(*x,delta)
}
pub fn root(x:&mut u8,delta:u8)->u8 {
 let mut n=1u8;while n>0 {n-=1;}
 let result=relay(x,delta);
 *x=x.wrapping_add(1);
 result
}
"#),
        ("src/bin/vectors.rs", r#"
use heap_outcomes::root;
fn main() {for (initial,delta) in [(0u8,0u8),(0,255),(254,0),(254,1),(255,0),(255,255),(200,54),(200,55)] {
 let mut state=initial;
 let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||root(&mut state,delta)));
 match result {Ok(value)=>println!("{initial}|{delta}|ok|{value}|{state}"),Err(_)=>println!("{initial}|{delta}|panic|0|{state}")}
}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 8);
    let dumps = dumped(&dir, "native-memory-outcomes");
    let cache =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-memory-outcome-verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("heap_outcomes"),
        &dumps,
        &cache,
        &dir.join("trial"),
        16,
    )
    .unwrap();
    let mut opts = options("heap_outcomes");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    assert!(
        rows.iter()
            .any(|r| r["function"].as_str() == Some("heap_outcomes::root")),
        "{:?}",
        residue_detail(&files, "heap_outcomes::root")
    );
    assert!(text(&files, "src/Behavior.lex.tex").contains("Result.error"));
    let name = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("heap_outcomes::root"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|r| r.function == "heap_outcomes::root")
        .and_then(|r| r.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let memory="(record MirMemory {count := 1, top := 128, base := (fun (index : Nat) => 64), length := (fun (index : Nat) => 1), stride := (fun (index : Nat) => 1), live := (fun (index : Nat) => (if (index =? 1) then 1 else 0)), cells := (fun (allocation : Nat) (offset : Nat) => 42), defined := (fun (allocation : Nat) (offset : Nat) => 1)})";
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let fields: Vec<_> = line.split('|').collect();
        let [initial, delta, kind, value, state] = fields.as_slice() else {
            panic!("{line}")
        };
        let expected = if *kind == "ok" {
            value.to_string()
        } else {
            "256".into()
        };
        let observed = format!("(match (project ({name} {initial} {delta} {memory}) item0) | Result.ok value => value | Result.error reason => 256)");
        vectors.push_str(&format!("theorem{{name:outcomeVector{i},parameters:[],statement:({observed} = {expected}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
        let tag = if *kind == "ok" {
            ""
        } else {
            "AssertKind::Overflow"
        };
        let observed = format!("(match (project ({name} {initial} {delta} {memory}) item0) | Result.ok value => \"\" | Result.error reason => reason)");
        vectors.push_str(&format!("theorem{{name:panicTagVector{i},parameters:[],statement:({observed} = \"{tag}\"),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
        vectors.push_str(&format!("theorem{{name:stateVector{i},parameters:[],statement:((project ({name} {initial} {delta} {memory}) item1) = {state}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    for (i, line) in reference.lines().enumerate() {
        let fields: Vec<_> = line.split('|').collect();
        let initial = fields[0];
        let delta = fields[1];
        let heap = format!("(project ({name} {initial} {delta} {memory}) item2)");
        for (label, expression, expected) in [
            (
                "retainedLive",
                format!("(apply (project {heap} live) 1)"),
                1,
            ),
            (
                "releasedFrame",
                format!("(apply (project {heap} live) 2)"),
                0,
            ),
            (
                "retainedCell",
                format!("(apply (project {heap} cells) 1 0)"),
                42,
            ),
        ] {
            vectors.push_str(&format!("theorem{{name:{label}{i},parameters:[],statement:({expression} = {expected}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:stateVector0,"))
        .unwrap();
    let bad = good.replace(" = 2)", " = 3)");
    assert_ne!(good, bad);
    let wrong = body.replacen(good, &bad, 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002"),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_registry_calls_reuse_source_semantics_ue_99() {
    let dir=scratch("registry-source-import", &[
      ("Cargo.toml", "[package]\nname=\"registry_calls\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n[dependencies]\nnum-bigint=\"=0.4.8\"\n"),
      ("src/lib.rs",r#"
use num_bigint::BigInt;
pub fn product(a:&BigInt,b:&BigInt)->BigInt {let mut n=1u8;while n>0 {n-=1;} a*b}
pub fn sum(a:&BigInt,b:&BigInt)->BigInt {let mut n=1u8;while n>0 {n-=1;} a+b}
pub fn difference(a:&BigInt,b:&BigInt)->BigInt {let mut n=1u8;while n>0 {n-=1;} a-b}
"#),
      ("src/bin/vectors.rs", r#"
use registry_calls::*;
use num_bigint::BigInt;
fn main(){for (a,b) in [(-7i64,3i64),(0,0),(9,-4),(i64::MAX,2),(i64::MIN,-1),(12345,67890)] {
 let x=BigInt::from(a);let y=BigInt::from(b);
 for (name,value) in [("product",product(&x,&y)),("sum",sum(&x,&y)),("difference",difference(&x,&y))] {
 println!("{name}|({a} : Int) ({b} : Int)|({value} : Int)");
 }
}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "registry-source-import");
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("registry_calls"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("registry_calls");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let decisions = verdicts(&cache);
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 18);
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let fields: Vec<_> = line.split('|').collect();
        let path = format!("registry_calls::{}", fields[0]);
        let row = rows.iter().find(|r| r["function"].as_str() == Some(&path));
        assert!(row.is_some(), "{path}: {:?}", residue_detail(&files, &path));
        let name = row.unwrap()["definition"]
            .as_str()
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name:registryVector{i},parameters:[],statement:(({name} {}) = {}),proof:decide{{}},axioms:[{}]}}\n",fields[1],fields[2],policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = (-21 : Int))", " = (-20 : Int))", 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_primitive_namespaces_reuse_kernels_ue_100() {
    let dir=scratch("primitive-namespaces", &[
      ("Cargo.toml", "[package]\nname=\"primitive_namespaces\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub fn read(x:u8)->u128 {
 let mut n=1u8;while n>0 {n-=1;}
 match x {
 0=>i8::MIN as u8 as u128,
 1=>i32::MIN as u32 as u128,
 2=>i64::MAX as u64 as u128,
 3=>u128::MAX,
 4=>f64::INFINITY.to_bits() as u128,
 5=>f32::NEG_INFINITY.to_bits() as u128,
 _=>u16::BITS as u128,
 }
}
"#),
      ("src/bin/vectors.rs", r#"fn main(){for x in [0u8,1,2,3,4,5,6,255] {println!("{x}|{}",primitive_namespaces::read(x));}}"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "primitive-namespaces");
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("primitive_namespaces"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("primitive_namespaces");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let row = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("primitive_namespaces::read"));
    assert!(
        row.is_some(),
        "{:?}",
        residue_detail(&files, "primitive_namespaces::read")
    );
    let name = row.unwrap()["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|v| v.function == "primitive_namespaces::read")
        .and_then(|v| v.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let (arg, value) = line.split_once('|').unwrap();
        vectors.push_str(&format!("theorem{{name:primitiveVector{i},parameters:[],statement:(({name} {arg}) = {value}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:primitiveVector0,"))
        .unwrap();
    let bad = good.replace(" = 128)", " = 127)");
    assert_ne!(good, bad);
    let wrong = body.replacen(good, &bad, 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_constructor_function_items_reuse_schema_kernels_ue_101() {
    let dir=scratch("constructor-function-items", &[
      ("Cargo.toml", "[package]\nname=\"constructor_calls\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub enum Packet { Value(u8) }
pub fn packet(x:Option<u8>)->u8 {
 let mut n=1u8;while n>0 {n-=1;}
 match x.map(Packet::Value) {Some(Packet::Value(y))=>y,None=>0}
}
pub fn option(x:Option<u8>)->u8 {
 let mut n=1u8;while n>0 {n-=1;}
 match x.map(Some) {Some(Some(y))=>y,_=>0}
}
pub fn result(x:Option<u8>)->u8 {
 let mut n=1u8;while n>0 {n-=1;}
 match x.map(Result::<u8,bool>::Ok) {Some(Ok(y))=>y,_=>0}
}
"#),
      ("src/bin/vectors.rs", r#"
use constructor_calls::*;
fn main(){for x in [None,Some(0u8),Some(7),Some(255)] {
 let input=match x {None=>"(ctor Option.none [Nat])".into(),Some(y)=>format!("(ctor Option.some [Nat] {y})")};
 for (name,value) in [("packet",packet(x)),("option",option(x)),("result",result(x))] {println!("{name}|{input}|{value}");}
}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "constructor-function-items");
    let mut probe = options("constructor_calls");
    probe.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &probe).unwrap();
    for function in ["packet", "option", "result"] {
        let path = format!("constructor_calls::{function}");
        assert!(
            residue_detail(&proposed, &path).is_none(),
            "{path}: {:?}",
            residue_detail(&proposed, &path)
        );
    }
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("constructor_calls"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("constructor_calls");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let decisions = verdicts(&cache);
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 12);
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let fields: Vec<_> = line.split('|').collect();
        let path = format!("constructor_calls::{}", fields[0]);
        let row = rows.iter().find(|r| r["function"].as_str() == Some(&path));
        assert!(row.is_some(), "{path}: {:?}", residue_detail(&files, &path));
        let name = row.unwrap()["definition"]
            .as_str()
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name:constructorVector{i},parameters:[],statement:(({name} {}) = {}),proof:decide{{}},axioms:[{}]}}\n",fields[1],fields[2],policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:constructorVector6,"))
        .unwrap();
    let bad = good.replace(" = 7)", " = 8)");
    assert_ne!(good, bad);
    let wrong = body.replacen(good, &bad, 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_generic_bodies_reuse_source_dictionaries_ue_102() {
    let dir=scratch("native-source-dictionaries", &[
      ("Cargo.toml", "[package]\nname=\"dictionary_calls\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub trait Read {fn read(&self)->u8;}
pub struct First {pub value:u8}
pub struct Second {pub value:u8}
impl Read for First {fn read(&self)->u8 {self.value}}
impl Read for Second {fn read(&self)->u8 {self.value.wrapping_add(1)}}
pub fn generic<T:Read>(x:&T)->u8 {let mut n=1u8;while n>0 {n-=1;}x.read()}
pub fn first(x:u8)->u8 {generic(&First{value:x})}
pub fn second(x:u8)->u8 {generic(&Second{value:x})}
"#),
      ("src/bin/vectors.rs",r#"
use dictionary_calls::*;
fn main(){for x in [0u8,7,254,255] {println!("first|{x}|{}",first(x));println!("second|{x}|{}",second(x));}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "native-source-dictionaries");
    let mut probe = options("dictionary_calls");
    probe.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &probe).unwrap();
    assert!(
        residue_detail(&proposed, "dictionary_calls::generic").is_none(),
        "{:?}",
        residue_detail(&proposed, "dictionary_calls::generic")
    );
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("dictionary_calls"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("dictionary_calls");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let decisions = verdicts(&cache);
    let reference = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(reference.lines().count(), 8);
    let mut vectors = String::new();
    for (i, line) in reference.lines().enumerate() {
        let fields: Vec<_> = line.split('|').collect();
        let path = format!("dictionary_calls::{}", fields[0]);
        let row = rows.iter().find(|r| r["function"].as_str() == Some(&path));
        assert!(row.is_some(), "{path}: {:?}", residue_detail(&files, &path));
        let name = row.unwrap()["definition"]
            .as_str()
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let policy = decisions
            .rows
            .values()
            .find(|v| v.function == path)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        vectors.push_str(&format!("theorem{{name:dictionaryVector{i},parameters:[],statement:(({name} {}) = {}),proof:decide{{}},axioms:[{}]}}\n",fields[1],fields[2],policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:dictionaryVector2,"))
        .unwrap();
    let bad = good.replace(" = 7)", " = 8)");
    assert_ne!(good, bad);
    let wrong = body.replacen(good, &bad, 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_dictionary_boundaries_fail_closed_ue_102() {
    let dir=scratch("native-dictionary-boundaries", &[
        ("Cargo.toml","[package]\nname=\"dictionary_boundaries\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",r#"
pub trait Read {fn read(&self)->u8;}
pub fn exclusive<T:Read>(x:&mut T)->u8 {let mut n=1u8;while n>0 {n-=1;}x.read()}
pub fn uncaptured<T:Read>(x:&T)->u8 {let mut n=1u8;while n>0 {n-=1;}let f=||x.read();f()}
pub fn unavailable<T:std::hash::Hasher>(x:&T)->u64 {let mut n=1u8;while n>0 {n-=1;}x.finish()}
"#),
    ]);
    let dumps = dumped(&dir, "native-dictionary-boundaries");
    let mut opts = options("dictionary_boundaries");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for function in ["exclusive", "uncaptured", "unavailable"] {
        let path = format!("dictionary_boundaries::{function}");
        let detail = residue_detail(&files, &path)
            .unwrap_or_else(|| panic!("{path} acquired an unavailable dictionary interface"));
        assert!(detail.contains("dictionary"), "{path}: {detail}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_library_schemas_reuse_compiler_declarations_ue_103() {
    check_native_library_schema_vectors(false);
}

#[test]
fn native_record_schemas_precede_source_carriers_ue_110() {
    check_native_library_schema_vectors(true);
}

fn check_native_library_schema_vectors(unsigned: bool) {
    let label = if unsigned {
        "native-source-carrier-schemas"
    } else {
        "native-library-schemas"
    };
    let dir=scratch(label, &[
      ("Cargo.toml", "[package]\nname=\"library_schemas\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub fn read(a:i32,b:i32,end:bool)->u32 {
 let mut n=1u8;while n>0 {n-=1;}
 let range=a..b;
 if end {range.end as u32} else {range.start as u32}
}
"#),
      ("src/bin/vectors.rs",r#"
fn main(){for (a,b) in [(i32::MIN,i32::MAX),(-7,3),(0,0),(12345,-67890)] {for end in [false,true] {println!("{} {} {end}|{}",a as u32,b as u32,library_schemas::read(a,b,end));}}}
"#),
    ]);
    if unsigned {
        for file in ["src/lib.rs", "src/bin/vectors.rs"] {
            let path = dir.join(file);
            let source = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                path,
                source
                    .replace("i32", "u32")
                    .replace("(-7,3)", "(7,3)")
                    .replace("-67890", "67890"),
            )
            .unwrap();
        }
    }
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, label);
    let mut probe = options("library_schemas");
    probe.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &probe).unwrap();
    assert!(
        residue_detail(&proposed, "library_schemas::read").is_none(),
        "{:?}",
        residue_detail(&proposed, "library_schemas::read")
    );
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("library_schemas"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("library_schemas");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let row = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("library_schemas::read"));
    assert!(
        row.is_some(),
        "{:?}",
        residue_detail(&files, "library_schemas::read")
    );
    let name = row.unwrap()["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|v| v.function == "library_schemas::read")
        .and_then(|v| v.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let (arg, value) = line.split_once('|').unwrap();
        vectors.push_str(&format!("theorem{{name:librarySchemaVector{i},parameters:[],statement:(({name} {arg}) = {value}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:librarySchemaVector0,"))
        .unwrap();
    let bad = if unsigned {
        good.replace(" = 0)", " = 1)")
    } else {
        good.replace(" = 2147483648)", " = 2147483649)")
    };
    assert_ne!(good, bad);
    let wrong = body.replacen(good, &bad, 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    if let Some(export) = std::env::var_os("UORUST_TEST_OUTPUT_VERDICTS") {
        std::fs::copy(&cache, export).unwrap();
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_library_bodies_reuse_compiler_graph_ue_104() {
    let previous_verdicts = std::env::var_os("UORUST_TEST_VERDICTS")
        .map(|path| std::fs::read(path).expect("prior verdict bytes"));

    let dir=scratch("native-library-bodies", &[
      ("Cargo.toml", "[package]\nname=\"library_bodies\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub fn read(a:i32,b:i32,end:bool)->u32 {
 let mut n=1u8;while n>0 {n-=1;}
 let range=a..b;
 if end {range.is_empty() as u32} else {a.midpoint(b) as u32}
}
"#),
      ("src/bin/vectors.rs",r#"
fn main(){for (a,b) in [(i32::MIN,i32::MAX),(-7,3),(0,0),(12345,-67890)] {for end in [false,true] {println!("{} {} {end}|{}",a as u32,b as u32,library_bodies::read(a,b,end));}}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "native-library-bodies");
    let mut probe = options("library_bodies");
    probe.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &probe).unwrap();
    assert!(
        residue_detail(&proposed, "library_bodies::read").is_none(),
        "{:?}",
        residue_detail(&proposed, "library_bodies::read")
    );
    let cache = dir.join("verdicts.toml");
    if let Some(previous) = previous_verdicts {
        std::fs::write(&cache, previous).unwrap();
    }
    uorust::mir::prove_bounded(
        &dir,
        &options("library_bodies"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("library_bodies");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let row = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("library_bodies::read"));
    assert!(
        row.is_some(),
        "{:?}",
        residue_detail(&files, "library_bodies::read")
    );
    let name = row.unwrap()["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|v| v.function == "library_bodies::read")
        .and_then(|v| v.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let module = text(&files, "src/Behavior.lex.tex");
    let definition = module
        .split("definition{")
        .find(|block| block.contains(&format!("name: {name},")))
        .expect("lifted definition");
    let outcomes = definition.contains("result: (Result Nat String)");
    assert!(outcomes || definition.contains("result: Nat,"));
    let mut vectors = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let (arg, value) = line.split_once('|').unwrap();
        let observed = if outcomes {
            format!("(match ({name} {arg}) | Result.ok returned => returned | Result.error reason => 4294967296)")
        } else {
            format!("({name} {arg})")
        };
        vectors.push_str(&format!("theorem{{name:libraryBodyVector{i},parameters:[],statement:({observed} = {value}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:libraryBodyVector0,"))
        .unwrap();
    let bad = good.replace(" = 0)", " = 1)");
    assert_ne!(good, bad);
    let wrong = body.replacen(good, &bad, 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_library_effects_propagate_before_proof_ue_104() {
    let dir=scratch("library-effects", &[
      ("Cargo.toml", "[package]\nname=\"library_effects\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub fn leaf(a:i32,b:i32)->u32 {
 let mut n=1u8;while n>0 {n-=1;}
 a.midpoint(b) as u32
}
pub fn relay(a:i32,b:i32)->u32 {
 let mut n=1u8;while n>0 {n-=1;}
 leaf(a,b)
}
"#),
    ]);
    let dumps = dumped(&dir, "library-effects");
    let mut opts = options("library_effects");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let lifted = frontier_rows(&files, "lifted");
    for function in ["library_effects::leaf", "library_effects::relay"] {
        let row = lifted
            .iter()
            .find(|row| row["function"].as_str() == Some(function))
            .unwrap_or_else(|| panic!("{function}: {:?}", residue_detail(&files, function)));
        assert!(
            row["definition"]
                .as_str()
                .unwrap()
                .starts_with("Behavior::nativeOutcome"),
            "{function}: library panic ABI was delayed until proof work"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_optimized_library_bodies_preserve_borrowed_records_ue_105() {
    let previous_verdicts = std::env::var_os("UORUST_TEST_VERDICTS")
        .map(|path| std::fs::read(path).expect("prior verdict bytes"));

    let dir=scratch("native-optimized-library", &[
      ("Cargo.toml", "[package]\nname=\"optimized_library\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
      ("src/lib.rs",r#"
pub struct Record {word:i32, flag:bool}
pub fn read(a:i32,b:i32,end:bool)->u32 {
 let mut n=1u8;while n>0 {n-=1;}
 let mut current=Record{word:a,flag:end};
 let previous=std::mem::replace(&mut current,Record{word:b,flag:!end});
 if previous.flag==end && current.flag!=end {
  if end {previous.word as u32} else {current.word as u32}
 }else{0}
}
"#),
      ("src/bin/vectors.rs",r#"
fn main(){for (a,b) in [(i32::MIN,i32::MAX),(-7,3),(0,0),(12345,-67890)] {for end in [false,true] {println!("{} {} {end}|{}",a as u32,b as u32,optimized_library::read(a,b,end));}}}
"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "native-optimized-library");
    let mut probe = options("optimized_library");
    probe.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &probe).unwrap();
    assert!(
        residue_detail(&proposed, "optimized_library::read").is_none(),
        "{:?}",
        residue_detail(&proposed, "optimized_library::read")
    );
    let cache = dir.join("verdicts.toml");
    if let Some(previous) = previous_verdicts {
        std::fs::write(&cache, previous).unwrap();
    }
    uorust::mir::prove_bounded(
        &dir,
        &options("optimized_library"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("optimized_library");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let row = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("optimized_library::read"));
    assert!(
        row.is_some(),
        "{:?}",
        residue_detail(&files, "optimized_library::read")
    );
    let name = row.unwrap()["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|v| v.function == "optimized_library::read")
        .and_then(|v| v.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let module = text(&files, "src/Behavior.lex.tex");
    let definition = module
        .split("definition{")
        .find(|block| block.contains(&format!("name: {name},")))
        .expect("lifted definition");
    let outcomes = definition.contains("result: (Result Nat String)");
    assert!(outcomes || definition.contains("result: Nat,"));
    let mut vectors = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let (arg, value) = line.split_once('|').unwrap();
        let observed = if outcomes {
            format!("(match ({name} {arg}) | Result.ok returned => returned | Result.error reason => 4294967296)")
        } else {
            format!("({name} {arg})")
        };
        vectors.push_str(&format!("theorem{{name:optimizedLibraryVector{i},parameters:[],statement:({observed} = {value}),proof:decide{{}},axioms:[{}]}}\n",policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:optimizedLibraryVector0,"))
        .unwrap();
    let bad = good.replace(" = 2147483647)", " = 2147483646)");
    assert_ne!(good, bad);
    let wrong = body.replacen(good, &bad, 1);
    assert_ne!(wrong, body);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mixed_source_native_schema_dependencies_ue_106() {
    let dir = scratch("mixed-schema-order", &[
        ("Cargo.toml", "[package]\nname=\"mixed_schema_order\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn packet(n:u8)->(Result<(),std::fmt::Error>,u8) {
    (Ok(()),n)
}
"#),
    ]);
    let dumps = dumped(&dir, "mixed-schema-order");
    let mut opts = options("mixed_schema_order");
    opts.mir = Some(mir_options(
        dumps,
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        residue_detail(&files, "mixed_schema_order::packet").is_none(),
        "{:?}",
        residue_detail(&files, "mixed_schema_order::packet")
    );
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let checked = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let body = text(&files, "src/Behavior.lex.tex");
    let changed = body.replacen("name: CoreFmtError,", "name: AbsentLibrarySchema,", 1);
    assert_ne!(body, changed);
    std::fs::write(out.join("src/Behavior.lex.tex"), changed).unwrap();
    let rejected = std::process::Command::new("lexlean")
        .args([
            "verify",
            "src/Behavior.lex.tex",
            "--diagnostic-format",
            "json",
        ])
        .current_dir(&out)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stdout).contains("LLT4001"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_integer_forms_share_modular_kernels_ue_107() {
    let dir = scratch("compiler-integer-forms", &[
        ("Cargo.toml", "[package]\nname=\"compiler_integer_forms\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn read(a:u64,b:i64)->u64 {
 let mut n=1u8;while n>0 {n-=1;}
 a.wrapping_add_signed(b)
}
"#),
    ]);
    let dumps = dumped(&dir, "compiler-integer-forms");
    let cache = dir.join("verdicts.toml");
    uorust::mir::prove_bounded(
        &dir,
        &options("compiler_integer_forms"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    let mut opts = options("compiler_integer_forms");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    assert!(
        rows.iter()
            .any(|row| row["function"].as_str() == Some("compiler_integer_forms::read")),
        "{:?}",
        residue_detail(&files, "compiler_integer_forms::read")
    );
    let name = rows
        .iter()
        .find(|row| row["function"].as_str() == Some("compiler_integer_forms::read"))
        .unwrap()["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .values()
        .find(|v| v.function == "compiler_integer_forms::read")
        .and_then(|v| v.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, (a, b, expected)) in [
        (0u64, -1i64, u64::MAX),
        (u64::MAX, 1, 0),
        (0, i64::MIN, 1u64 << 63),
        (u64::MAX, i64::MAX, (1u64 << 63) - 2),
        (1, -1, 0),
        (42, 7, 49),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(a.wrapping_add_signed(b), expected);
        vectors.push_str(&format!("theorem{{name:compilerIntegerVector{i},parameters:[],statement:(({name} {a} {}) = {expected}),proof:decide{{}},axioms:[{}]}}\n",b as u64,policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|l| l.contains("name:compilerIntegerVector0,"))
        .unwrap();
    let bad = good.replace(" = 18446744073709551615)", " = 0)");
    assert_ne!(good, bad);
    std::fs::write(&file, body.replacen(good, &bad, 1)).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_reinterpret_forms_share_typed_kernels_ue_108() {
    let previous_verdicts = std::env::var_os("UORUST_TEST_VERDICTS")
        .map(|path| std::fs::read(path).expect("prior verdict bytes"));
    let dir = scratch("reinterpret-forms", &[
        ("Cargo.toml", "[package]\nname=\"reinterpret_forms\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn bytes(text:&str)->&[u8] {unsafe{std::mem::transmute(text)}}
pub fn word(x:u32)->[u8;4] {unsafe{std::mem::transmute(x)}}
pub fn pointer(p:*const u8)->*mut u8 {unsafe{std::mem::transmute(p)}}
pub unsafe fn invalid_text(bytes:&[u8])->&str {unsafe{std::mem::transmute(bytes)}}
pub unsafe fn invalid_scalar(bits:u32)->char {unsafe{std::mem::transmute(bits)}}
pub unsafe fn invalid_pointer(p:*const u8)->std::ptr::NonNull<u8> {unsafe{std::mem::transmute(p)}}
"#),
    ]);
    let dumps = dumped(&dir, "reinterpret-forms");
    let mut opts = options("reinterpret_forms");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let lifted = frontier_rows(&files, "lifted");
    for function in [
        "reinterpret_forms::bytes",
        "reinterpret_forms::word",
        "reinterpret_forms::pointer",
    ] {
        assert!(
            lifted
                .iter()
                .any(|row| row["function"].as_str() == Some(function)),
            "{function}: {:?}",
            residue_detail(&files, function)
        );
    }
    let cache = dir.join("verdicts.toml");
    if let Some(bytes) = previous_verdicts {
        std::fs::write(&cache, bytes).unwrap();
    }

    uorust::mir::prove_bounded(
        &dir,
        &options("reinterpret_forms"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let lifted = frontier_rows(&files, "lifted");
    for function in [
        "reinterpret_forms::bytes",
        "reinterpret_forms::word",
        "reinterpret_forms::pointer",
    ] {
        assert!(
            lifted
                .iter()
                .any(|row| row["function"].as_str() == Some(function)),
            "{function}: {:?}",
            residue_detail(&files, function)
        );
    }
    for function in [
        "reinterpret_forms::invalid_text",
        "reinterpret_forms::invalid_scalar",
        "reinterpret_forms::invalid_pointer",
    ] {
        assert!(
            !lifted
                .iter()
                .any(|row| row["function"].as_str() == Some(function)),
            "{function}: validity was discarded"
        );
        assert!(residue_detail(&files, function).is_some());
    }
    let definitions: std::collections::BTreeMap<_, _> = lifted
        .iter()
        .filter_map(|row| {
            Some((
                row["function"].as_str()?.rsplit("::").next()?,
                row["definition"].as_str()?.strip_prefix("Behavior::")?,
            ))
        })
        .collect();
    if let Some(output) = std::env::var_os("UORUST_TEST_OUTPUT_VERDICTS") {
        std::fs::copy(&cache, output).unwrap();
    }
    let decisions = verdicts(&cache);
    let mut vectors = String::new();
    let mut add = |function: &str, index: usize, observation: String, expected: String| {
        let name = definitions[function];
        let path = format!("reinterpret_forms::{function}");
        let kappa = lifted
            .iter()
            .find(|r| r["function"].as_str() == Some(&path))
            .unwrap()["kappa"]
            .as_str()
            .unwrap();
        let policy = decisions
            .rows
            .get(kappa)
            .and_then(|v| v.axioms.get(name))
            .cloned()
            .unwrap_or_default();
        let proof = if function == "bytes" {
            "reflexivity"
        } else {
            "decide"
        };
        vectors.push_str(&format!("theorem{{name:representationVector{function}{index},parameters:[],statement:({observation} = {expected}),proof:{proof}{{}},axioms:[{}]}}\n",policy.join(", ")));
    };
    for (i, value) in ["", "ascii", "é", "🦀"].into_iter().enumerate() {
        // SDK UTF-8 is opaque: prove equality to the admitted byte-view model.
        let literal = serde_json::to_string(value).unwrap();
        let expected = format!("(Prelude::textBytes {literal})");
        add(
            "bytes",
            i,
            format!("({} {literal})", definitions["bytes"]),
            expected,
        );
    }
    for (i, value) in [0u32, 1, 0x1234abcd, u32::MAX].into_iter().enumerate() {
        let expected = value.to_le_bytes();
        let mut list = "(nil Nat)".to_string();
        for byte in expected.into_iter().rev() {
            list = format!("(cons {byte} {list})");
        }
        add(
            "word",
            i,
            format!("({} {value})", definitions["word"]),
            list,
        );
    }
    for (i, address) in [0usize, 42, usize::MAX].into_iter().enumerate() {
        let pointer = std::ptr::without_provenance::<u8>(address);
        let returned = pointer.cast_mut();
        assert_eq!(returned.addr(), address);
        let call = format!(
            "({} (record RawPointer {{allocation := 7, address := {address}, metadata := 0}}))",
            definitions["pointer"]
        );
        for (field, expected) in [
            ("allocation", "7".to_string()),
            ("address", address.to_string()),
            ("metadata", "0".to_string()),
        ] {
            add(
                "pointer",
                3 * i
                    + match field {
                        "allocation" => 0,
                        "address" => 1,
                        _ => 2,
                    },
                format!("(project {call} {field})"),
                expected,
            );
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|l| l.contains("name:representationVectorword0,"))
        .unwrap();
    let bad = good.replacen("(cons 0", "(cons 1", 1);
    assert_ne!(good, bad);
    std::fs::write(&file, body.replacen(good, &bad, 1)).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn canonical_compiler_records_preserve_native_observations_ue_111() {
    let previous_verdicts = std::env::var_os("UORUST_TEST_VERDICTS")
        .map(|path| std::fs::read(path).expect("prior actual verdict bytes"));
    let dir = scratch("canonical-compiler-records", &[
        ("Cargo.toml", "[package]\nname=\"canonical_records\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", "pub fn reverse(a:u32,b:u32)->std::iter::Rev<std::ops::Range<u32>> { let mut n=1u8; while n>0 {n-=1;} (a..b).rev() }\n"),
        ("src/bin/vectors.rs", "fn main(){for (a,b) in [(0,0),(0,1),(5,9),(100,87),(u32::MAX-3,u32::MAX)] { println!(\"{a} {b}|{}\",canonical_records::reverse(a,b).count()); }}\n"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "canonical-compiler-records");
    let mut opts = options("canonical_records");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let proposed = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        residue_detail(&proposed, "canonical_records::reverse").is_none(),
        "{:?}",
        residue_detail(&proposed, "canonical_records::reverse")
    );
    let cache = dir.join("verdicts.toml");
    if let Some(bytes) = previous_verdicts {
        std::fs::write(&cache, bytes).unwrap();
    }
    uorust::mir::prove_bounded(
        &dir,
        &options("canonical_records"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let row = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("canonical_records::reverse"))
        .unwrap_or_else(|| panic!("{:?}", residue_detail(&files, "canonical_records::reverse")));
    let name = row["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let decisions = verdicts(&cache);
    let policy = decisions
        .rows
        .get(row["kappa"].as_str().unwrap())
        .and_then(|r| r.axioms.get(name))
        .cloned()
        .unwrap_or_default();
    let mut vectors = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let (args, value) = line.split_once('|').unwrap();
        let range = format!("(project ({name} {args}) iter)");
        vectors.push_str(&format!("theorem{{name:canonicalRecordVector{i},parameters:[],statement:((prim subtract (project {range} endValue) (project {range} start) : Nat) = {value}),proof:decide{{}},axioms:[{}]}}\n", policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|line| line.contains("name:canonicalRecordVector0,"))
        .unwrap();
    let bad = good.replace(" = 0)", " = 1)");
    assert_ne!(good, bad);
    std::fs::write(&file, body.replacen(good, &bad, 1)).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    if let Some(export) = std::env::var_os("UORUST_TEST_OUTPUT_VERDICTS") {
        std::fs::copy(&cache, export).unwrap();
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn resolved_workspace_types_survive_external_globs_in_sdk_ue_113() {
    let dir = scratch("resolved-source-identities", &[
        ("Cargo.toml", "[package]\nname=\"resolved_identity\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n[dependencies]\nserde=\"=1.0.229\"\n"),
        ("src/lib.rs", "pub mod defs {pub enum PayloadKind {First,Second}}\npub mod parent {pub use crate::defs::PayloadKind; pub mod wire {#[allow(unused_imports)] use serde::*; pub struct Payload {pub kind:super::PayloadKind} pub fn keep(value:Payload)->Payload {value}}}\n"),
        ("src/bin/vectors.rs", "use resolved_identity::{defs::PayloadKind,parent::wire::{Payload,keep}}; fn main(){for tag in [0u8,1] {let kind=if tag==0 {PayloadKind::First}else{PayloadKind::Second};let out=keep(Payload{kind});let label=match out.kind {PayloadKind::First=>\"First\",PayloadKind::Second=>\"Second\"};println!(\"{tag}|{label}\");}}\n"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let files = uorust::project_workspace(&dir, &options("resolved_identity")).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let row = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("resolved_identity::parent::wire::keep"))
        .unwrap_or_else(|| {
            panic!(
                "{:?}",
                residue_detail(&files, "resolved_identity::parent::wire::keep")
            )
        });
    let name = row["definition"]
        .as_str()
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let mut vectors = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let (tag, expected) = line.split_once('|').unwrap();
        assert!(matches!(expected, "First" | "Second"));
        let input = if tag == "0" { "First" } else { "Second" };
        vectors.push_str(&format!("theorem{{name:sourceIdentityVector{i},parameters:[],statement:((project ({name} (record Payload {{kind := (ctor PayloadKind.{input})}})) kind) = (ctor PayloadKind.{expected})),proof:reflexivity{{}},axioms:[]}}\n"));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{vectors}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let good = vectors
        .lines()
        .find(|s| s.contains("name:sourceIdentityVector0,"))
        .unwrap();
    let bad = good.replace(
        " = (ctor PayloadKind.First)",
        " = (ctor PayloadKind.Second)",
    );
    assert_ne!(good, bad);
    std::fs::write(&file, body.replacen(good, &bad, 1)).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bounded_proof_batches_reuse_the_catalogue_ue_114() {
    let dir = scratch("catalogue-batches", &[
        ("Cargo.toml", "[package]\nname=\"catalogue_batches\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"
pub fn first(x:u8)->u8 {let mut n=1u8;while n>0 {n-=1;} x}
pub fn second(x:u8)->u8 {let mut n=1u8;while n>0 {n-=1;} x}
"#),
        ("src/bin/vectors.rs", r#"fn main(){for x in [0u8,1,254,255] {println!("{x}|{}|{}",catalogue_batches::first(x),catalogue_batches::second(x));}}"#),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let dumps = dumped(&dir, "catalogue-batches");
    let cache = dir.join("verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("catalogue_batches"),
        &dumps,
        &cache,
        &dir.join("trial"),
        1,
    )
    .unwrap();
    assert_eq!(proof.refused, 0, "{proof:?}");
    assert_eq!(proof.proved, 2, "{proof:?}");
    assert_eq!(
        proof.projections, 1,
        "unchanged proof batches repeated whole-workspace projection: {proof:?}"
    );
    let mut opts = options("catalogue_batches");
    opts.mir = Some(mir_options(
        dumps,
        verdicts(&cache),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let name = |function: &str| {
        rows.iter()
            .find(|r| r["function"].as_str() == Some(function))
            .and_then(|r| r["definition"].as_str())
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap()
    };
    let first = name("catalogue_batches::first");
    let second = name("catalogue_batches::second");
    let decisions = verdicts(&cache);
    let mut observations = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let values = line.split('|').collect::<Vec<_>>();
        assert_eq!(values.len(), 3);
        for (which, model) in [first, second].into_iter().enumerate() {
            let policy = decisions
                .rows
                .values()
                .find_map(|row| row.axioms.get(model))
                .cloned()
                .unwrap_or_default();
            observations.push_str(&format!("theorem{{name:catalogueVector{i}x{which},parameters:[],statement:(({model} {}) = {}),proof:decide{{}},axioms:[{}]}}\n",values[0],values[which+1],policy.join(", ")));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observations}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let result = verify();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let wrong = body.replacen(" = 255)", " = 254)", 1);
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn retained_library_capture_supplies_native_dependencies_ue_115() {
    let dir=scratch("captured-library",&[
        ("Cargo.toml","[package]\nname=\"captured_library\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs","pub fn empty()->Vec<u8>{let mut n=1u8;while n>0 {n-=1;} Vec::default()}\npub fn length()->usize {let mut n=1u8;while n>0 {n-=1;} empty().len()}\n"),
        ("src/bin/vectors.rs","fn main(){println!(\"{}\",captured_library::length());}"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(expected.trim(), "0");
    let dumps = dumped(&dir, "captured-library");
    let mut opts = options("captured_library");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        frontier_rows(&files, "lifted")
            .iter()
            .any(|r| r["function"].as_str() == Some("captured_library::empty")),
        "{:?}",
        residue_detail(&files, "captured_library::empty")
    );
    let cache = dir.join("verdicts.toml");
    let proved = uorust::mir::prove_bounded(
        &dir,
        &options("captured_library"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proved.refused, 0, "{proved:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let model = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("captured_library::length"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let policy = decisions
        .rows
        .values()
        .find_map(|r| r.axioms.get(model))
        .cloned()
        .unwrap_or_default();
    let observation=format!("theorem{{name:capturedLibraryObservation,parameters:[],statement:(({model}) = {}),proof:decide{{}},axioms:[{}]}}\n",expected.trim(),policy.join(", "));
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observation}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replace(" = 0)", " = 1)");
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn empty_stateful_payloads_match_rust_sdk_ue_116() {
    let dir=scratch("empty-payload",&[
        ("Cargo.toml","[package]\nname=\"empty_payload\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs","use std::cell::Cell;\npub struct Payload {pub value:Cell<u8>}\npub fn empty()->Vec<Payload> {let mut n=1u8;while n>0 {n-=1;} Vec::default()}\n"),
        ("src/bin/vectors.rs","fn main(){println!(\"{}\",empty_payload::empty().len());}"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(expected.trim(), "0");
    let dumps = dumped(&dir, "empty-payload");
    let cache = dir.join("verdicts.toml");
    let proved = uorust::mir::prove_bounded(
        &dir,
        &options("empty_payload"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proved.refused, 0, "{proved:?}");
    let decisions = verdicts(&cache);
    let mut opts = options("empty_payload");
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let model = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("empty_payload::empty"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let policy = decisions
        .rows
        .values()
        .find_map(|r| r.axioms.get(model))
        .cloned()
        .unwrap_or_default();
    let observation=format!("theorem{{name:emptyPayloadObservation,parameters:[],statement:((prim length ({model}) : Nat) = {}),proof:decide{{}},axioms:[{}]}}\n",expected.trim(),policy.join(", "));
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observation}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replace(" = 0)", " = 1)");
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_normalization_preserves_caller_interfaces_ue_117() {
    let dir = scratch("compiler-caller-interface", &[
        ("Cargo.toml", "[package]\nname=\"compiler_caller_interface\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", "pub fn exchange(value:&mut Vec<String>,next:Vec<String>)->Vec<String>{let mut n=1u8;while n>0 {n-=1;} std::mem::replace(value,next)}\n"),
        ("src/bin/vectors.rs", "fn main(){let mut value=vec![String::from(\"before\")];let old=compiler_caller_interface::exchange(&mut value,vec![String::from(\"after\"),String::from(\"more\")]);println!(\"{}|{}\",old.join(\",\"),value.join(\",\"));}"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(expected.trim(), "before|after,more");
    let dumps = dumped(&dir, "compiler-caller-interface");
    let mut opts = options("compiler_caller_interface");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        frontier_rows(&files, "lifted")
            .iter()
            .any(|r| r["function"].as_str() == Some("compiler_caller_interface::exchange")),
        "{:?}",
        residue_detail(&files, "compiler_caller_interface::exchange")
    );
    let cache = dir.join("verdicts.toml");
    let proved = uorust::mir::prove_bounded(
        &dir,
        &options("compiler_caller_interface"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proved.refused, 0, "{proved:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let model = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("compiler_caller_interface::exchange"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let policy = decisions
        .rows
        .values()
        .find_map(|r| r.axioms.get(model))
        .cloned()
        .unwrap_or_default();
    let list = |text: &str| {
        text.split(',')
            .rev()
            .fold("(nil String)".to_string(), |tail, value| {
                format!("(cons {value:?} {tail})")
            })
    };
    let result = format!("({model} {} {})", list("before"), list("after,more"));
    let mut observations = String::new();
    for (index, values) in expected.trim().split('|').enumerate() {
        observations.push_str(&format!("theorem{{name:callerInterfaceObservation{index},parameters:[],statement:((project {result} item{index}) = {}),proof:decide{{}},axioms:[{}]}}\n",list(values),policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observations}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(
        &format!(" = {})", list("before")),
        &format!(" = {})", list("wrong")),
        1,
    );
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn imported_borrowed_results_preserve_caller_state_ue_118() {
    let dir = scratch("compiler-borrow-backward", &[
        ("Cargo.toml", "[package]\nname=\"compiler_borrow_backward\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", "pub fn update(value:&mut (u8,u16),next:u16){let mut n=1u8;while n>0 {n-=1;} std::convert::identity(value).1=next;}\n"),
        ("src/bin/vectors.rs", "fn main(){for (a,b,n) in [(3u8,5u16,11u16),(0,65535,0),(255,0,65535)] {let mut value=(a,b);compiler_borrow_backward::update(&mut value,n);println!(\"{a}|{b}|{n}|{}|{}\",value.0,value.1);}}"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8(rust.stdout).unwrap();
    let dumps = dumped(&dir, "compiler-borrow-backward");
    let mut opts = options("compiler_borrow_backward");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        frontier_rows(&files, "lifted")
            .iter()
            .any(|r| r["function"].as_str() == Some("compiler_borrow_backward::update")),
        "{:?}",
        residue_detail(&files, "compiler_borrow_backward::update")
    );
    let cache = dir.join("verdicts.toml");
    let proved = uorust::mir::prove_bounded(
        &dir,
        &options("compiler_borrow_backward"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proved.refused, 0, "{proved:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let model = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("compiler_borrow_backward::update"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let policy = decisions
        .rows
        .values()
        .find_map(|r| r.axioms.get(model))
        .cloned()
        .unwrap_or_default();
    let mut observations = String::new();
    for (i, line) in expected.lines().enumerate() {
        let values = line
            .split('|')
            .map(|v| v.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 5);
        let term = format!(
            "(project ({model} (record TupleNatNat {{item0 := {}, item1 := {}}}) {}) item1)",
            values[0], values[1], values[2]
        );
        for field in 0..2 {
            observations.push_str(&format!("theorem{{name:borrowedState{i}Field{field},parameters:[],statement:((project {term} item{field}) = {}),proof:decide{{}},axioms:[{}]}}\n", values[field+3], policy.join(", ")));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observations}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = 11)", " = 12)", 1);
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn imported_generic_wrappers_keep_consumer_dispatch_ue_119() {
    let dir=scratch("imported-consumer-dispatch",&[
        ("Cargo.toml","[package]\nname=\"imported_consumer_dispatch\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs","pub struct Token {pub value:String,pub tag:u8}\nimpl From<String> for Token {fn from(value:String)->Self {Self {value,tag:1}}}\nimpl From<&str> for Token {fn from(_: &str)->Self {Self {value:String::new(),tag:2}}}\npub mod foreign {pub trait From<T>{fn from(value:T)->super::Token;}}\nimpl foreign::From<String> for Token {fn from(_:String)->Self {Self {value:String::new(),tag:3}}}\npub fn wrap(value:String)->Token {let mut n=1u8;while n>0 {n-=1;} value.into()}\n"),
        ("src/bin/vectors.rs","fn main(){for input in [\"\",\"hello\",\"日本語\"] {let result=imported_consumer_dispatch::wrap(input.to_owned());println!(\"{input}|{}|{}\",result.value,result.tag);}}"),
    ]);
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8(rust.stdout).unwrap();
    let dumps = dumped(&dir, "imported-consumer-dispatch");
    let mut opts = options("imported_consumer_dispatch");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(
        frontier_rows(&files, "lifted")
            .iter()
            .any(|r| r["function"].as_str() == Some("imported_consumer_dispatch::wrap")),
        "{:?}",
        residue_detail(&files, "imported_consumer_dispatch::wrap")
    );
    let cache = dir.join("verdicts.toml");
    let proved = uorust::mir::prove_bounded(
        &dir,
        &options("imported_consumer_dispatch"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!(proved.refused, 0, "{proved:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let model = rows
        .iter()
        .find(|r| r["function"].as_str() == Some("imported_consumer_dispatch::wrap"))
        .and_then(|r| r["definition"].as_str())
        .unwrap()
        .strip_prefix("Behavior::")
        .unwrap();
    let policy = decisions
        .rows
        .values()
        .find_map(|r| r.axioms.get(model))
        .cloned()
        .unwrap_or_default();
    let mut observations = String::new();
    for (i, line) in expected.lines().enumerate() {
        let parts = line.split('|').collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        let term = format!("({model} {:?})", parts[0]);
        observations.push_str(&format!("theorem{{name:consumerPayload{i},parameters:[],statement:((project {term} value) = {:?}),proof:decide{{}},axioms:[{}]}}\n",parts[1],policy.join(", ")));
        observations.push_str(&format!("theorem{{name:consumerProvider{i},parameters:[],statement:((project {term} tag) = {}),proof:decide{{}},axioms:[{}]}}\n",parts[2],policy.join(", ")));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observations}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = 1)", " = 2)", 1);
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn local_annotations_share_authoritative_aliases_ue_120() {
    let dir = scratch("local-alias-annotations", &[
        ("Cargo.toml", "[package]\nname=\"local_alias_annotations\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n[dependencies]\nrustc-hash=\"=1.1.0\"\n"),
        ("src/lib.rs", "use rustc_hash::{FxHashMap,FxHashSet};\npub fn through(values:FxHashMap<u32,u32>)->usize {let local:FxHashMap<u32,u32>=values;local.len()}\npub fn through_set(values:FxHashSet<u32>)->usize {let local:FxHashSet<u32>=values;local.len()}\npub type Chain<K,V> = FxHashMap<K,V>;\npub fn nested(values:Option<Chain<u32,u32>>)->Option<Chain<u32,u32>> {let local:Option<Chain<u32,u32>>=values;local}\npub mod scope {pub struct FxHashMap {pub value:u8} pub fn shadow(value:FxHashMap)->FxHashMap {let local:FxHashMap=value;local}}\npub fn generic<FxHashMap>(value:FxHashMap)->FxHashMap {let local:FxHashMap=value;local}\n"),
    ]);
    let locked = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        locked.status.success(),
        "{}",
        String::from_utf8_lossy(&locked.stderr)
    );
    let files = project(&dir, "local_alias_annotations");
    let lifted = frontier_rows(&files, "lifted");
    for name in [
        "through",
        "through_set",
        "nested",
        "scope::shadow",
        "generic",
    ] {
        let function = format!("local_alias_annotations::{name}");
        assert!(
            lifted
                .iter()
                .any(|r| r["function"].as_str() == Some(&function)),
            "{function}: {:?}",
            residue_detail(&files, &function)
        );
    }
    let vectors = dir.join("src/bin/vectors.rs");
    std::fs::create_dir_all(vectors.parent().unwrap()).unwrap();
    std::fs::write(&vectors, "fn main(){for n in [0u32,1,3] {let map=(0..n).map(|x|(x,x+7)).collect();let set=(0..n).collect();println!(\"map|{n}|{}\",local_alias_annotations::through(map));println!(\"set|{n}|{}\",local_alias_annotations::through_set(set));}}\n").unwrap();
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let mut observations = String::new();
    for (i, line) in String::from_utf8(rust.stdout).unwrap().lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        let count: u32 = parts[1].parse().unwrap();
        let map = parts[0] == "map";
        let function = format!(
            "local_alias_annotations::{}",
            if map { "through" } else { "through_set" }
        );
        let model = lifted
            .iter()
            .find(|r| r["function"].as_str() == Some(&function))
            .unwrap()["definition"]
            .as_str()
            .unwrap()
            .strip_prefix("Behavior::")
            .unwrap();
        let entry = if map { "TupleNatNat" } else { "Nat" };
        let mut input = format!("(nil {entry})");
        for n in (0..count).rev() {
            let item = if map {
                format!("(record TupleNatNat {{item0 := {n}, item1 := {}}})", n + 7)
            } else {
                n.to_string()
            };
            input = format!("(cons {item} {input})");
        }
        observations.push_str(&format!("theorem{{name:aliasObservation{i},parameters:[],statement:(({model} {input}) = {}),proof:decide{{}}}}\n",parts[2]));
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observations}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = 0)", " = 9)", 1);
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_constants_reuse_complete_factory_capture_ue_121() {
    let dir = scratch("compiler-constant-closure", &[
        ("Cargo.toml", "[package]\nname=\"compiler_constant_closure\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", "#![feature(sized_type_properties)]\npub fn float_property(mut n:u8)->bool {while n>0 {n-=1;} <f64 as core::mem::SizedTypeProperties>::IS_ZST}\npub fn unit_property(mut n:u8)->bool {while n>0 {n-=1;} <() as core::mem::SizedTypeProperties>::IS_ZST}\n"),
    ]);
    let vectors = dir.join("src/bin/vectors.rs");
    std::fs::create_dir_all(vectors.parent().unwrap()).unwrap();
    std::fs::write(&vectors,"fn main(){for n in [0u8,1,7] {println!(\"{n}|{}|{}\",compiler_constant_closure::float_property(n),compiler_constant_closure::unit_property(n));}}\n").unwrap();
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "vectors"])
        .current_dir(&dir)
        .env("RUSTC_BOOTSTRAP", "1")
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8(rust.stdout).unwrap();
    let dumps = dumped(&dir, "compiler-constant-closure");
    let mut opts = options("compiler_constant_closure");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    for name in ["float_property", "unit_property"] {
        let function = format!("compiler_constant_closure::{name}");
        assert!(
            frontier_rows(&files, "lifted")
                .iter()
                .any(|r| r["function"].as_str() == Some(&function)),
            "{function}: {:?}",
            residue_detail(&files, &function)
        );
    }
    let cache = dir.join("verdicts.toml");
    let proved = uorust::mir::prove_bounded(
        &dir,
        &options("compiler_constant_closure"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!((proved.proved, proved.refused), (2, 0), "{proved:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut observations = String::new();
    for (i, line) in expected.lines().enumerate() {
        let parts: Vec<_> = line.split('|').collect();
        for (j, function) in ["float_property", "unit_property"].iter().enumerate() {
            let path = format!("compiler_constant_closure::{function}");
            let model = rows
                .iter()
                .find(|r| r["function"].as_str() == Some(&path))
                .unwrap()["definition"]
                .as_str()
                .unwrap()
                .strip_prefix("Behavior::")
                .unwrap();
            let policy = decisions
                .rows
                .values()
                .find(|r| r.function == path)
                .and_then(|r| r.axioms.get(model))
                .cloned()
                .unwrap_or_default();
            observations.push_str(&format!("theorem{{name:constantObservation{i}{j},parameters:[],statement:(({model} {}) = {}),proof:decide{{}},axioms:[{}]}}\n",parts[0],parts[j+1],policy.join(", ")));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{observations}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = body.replacen(" = false)", " = true)", 1);
    assert_ne!(body, wrong);
    std::fs::write(&file, wrong).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiler_discriminant_snapshots_ue_122() {
    let dir = scratch("compiler-enum-snapshot", &[
        ("Cargo.toml", "[package]\nname=\"enum_snapshot\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", r#"#![feature(custom_mir, core_intrinsics)]
use core::intrinsics::mir::*;
#[custom_mir(dialect = "built")]
pub fn before(mut prior: Option<u8>, replacement: Option<u8>) -> isize {
    mir! {
        let tag: isize;
        {
            tag = Discriminant(prior);
            prior = Move(replacement);
            RET = tag;
            Return()
        }
    }
}
#[custom_mir(dialect = "built")]
pub fn direct(prior: Option<u8>, _replacement: Option<u8>) -> isize {
    mir! {{ RET = Discriminant(prior); Return() }}
}
#[custom_mir(dialect = "built")]
pub fn branch(mut prior: Option<u8>, replacement: Option<u8>) -> isize {
    mir! {
        let tag: isize;
        {
            tag = Discriminant(prior);
            prior = Move(replacement);
            match tag { 0 => empty, _ => full }
        }
        empty = { RET = 0; Return() }
        full = { RET = 1; Return() }
    }
}
#[custom_mir(dialect = "built")]
pub fn field(prior: Option<u8>, _replacement: Option<u8>) -> isize {
    mir! {
        let pair: (isize, isize);
        {
            pair = (0_isize, 0_isize);
            pair.0 = Discriminant(prior);
            RET = pair.0;
            Return()
        }
    }
}
"#),
    ]);
    let vectors = dir.join("src/bin/observations.rs");
    std::fs::create_dir_all(vectors.parent().unwrap()).unwrap();
    std::fs::write(&vectors, "fn main(){for (a,b) in [(None,Some(7)),(Some(7),None)] {println!(\"{}\",[enum_snapshot::before(a,b),enum_snapshot::direct(a,b),enum_snapshot::branch(a,b),enum_snapshot::field(a,b)].iter().map(isize::to_string).collect::<Vec<_>>().join(\"|\"));}}").unwrap();
    let rust = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "observations"])
        .env("RUSTC_BOOTSTRAP", "1")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8(rust.stdout).unwrap();
    assert_eq!(expected.lines().collect::<Vec<_>>(), ["0|0|0|0", "1|1|1|1"]);
    let dumps = dumped(&dir, "enum-snapshot");
    let mut opts = options("enum_snapshot");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let paths = [
        "enum_snapshot::before",
        "enum_snapshot::direct",
        "enum_snapshot::branch",
        "enum_snapshot::field",
    ];
    for path in paths {
        assert!(
            frontier_rows(&files, "lifted")
                .iter()
                .any(|r| r["function"].as_str() == Some(path)),
            "{path}: {:?}",
            residue_detail(&files, path)
        );
    }
    let cache = dir.join("verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("enum_snapshot"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!((proof.proved, proof.refused), (4, 0), "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let none = "(ctor Option.none [Nat])";
    let some = "(ctor Option.some [Nat] 7)";
    let mut checks = String::new();
    for (i, ((a, b), results)) in [(none, some), (some, none)]
        .into_iter()
        .zip(expected.lines())
        .enumerate()
    {
        for (j, (path, result)) in paths.into_iter().zip(results.split('|')).enumerate() {
            let model = rows
                .iter()
                .find(|r| r["function"].as_str() == Some(path))
                .unwrap()["definition"]
                .as_str()
                .unwrap()
                .strip_prefix("Behavior::")
                .unwrap();
            let axioms = decisions
                .rows
                .values()
                .find(|r| r.function == path)
                .and_then(|r| r.axioms.get(model))
                .cloned()
                .unwrap_or_default()
                .join(", ");
            checks.push_str(&format!("theorem{{name:tagObservation{i}{j},parameters:[],statement:(({model} {a} {b}) = {result}),proof:decide{{}},axioms:[{axioms}]}}\n"));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let body = text(&files, "src/Behavior.lex.tex").replace(
        "\\end{semanticmodule}",
        &format!("{checks}\\end{{semanticmodule}}"),
    );
    let file = out.join("src/Behavior.lex.tex");
    std::fs::write(&file, &body).unwrap();
    let verify = || {
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let checked = verify();
    assert!(
        checked.status.success(),
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let wrong = checks.replacen(" = 0)", " = 1)", 1);
    assert_ne!(wrong, checks);
    std::fs::write(file, body.replace(&checks, &wrong)).unwrap();
    let rejected = verify();
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn path_module_dependencies_ue_123() {
    let dir = scratch("path-module-dependencies", &[
        ("Cargo.toml","[package]\nname=\"module_bindings\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs",r#"#[path="../generated/first.rs"] mod first;
#[path="../generated/second.rs"] mod second;
pub fn left(a: bool, b: bool) -> bool { first::run(a,b) }
pub fn right(a: bool, b: bool) -> bool { second::run(a,b) }
"#),
        ("generated/first.rs","fn inner(a:bool,b:bool)->bool {a && b}\npub fn run(a:bool,b:bool)->bool { inner(a,b) }"),
        ("generated/second.rs","fn inner(a:bool,b:bool)->bool {a || b}\npub fn run(a:bool,b:bool)->bool { inner(a,b) }"),
        ("src/bin/oracle.rs","fn main(){for a in [false,true]{for b in [false,true]{println!(\"{a} {b} {} {}\",module_bindings::left(a,b),module_bindings::right(a,b));}}}"),
    ]);
    let oracle = std::process::Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--bin", "oracle"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let dumps = dumped(&dir, "module-bindings");
    let mut opts = options("module_bindings");
    opts.mir = Some(mir_options(
        dumps.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let paths = ["module_bindings::left", "module_bindings::right"];
    for path in paths {
        assert!(
            frontier_rows(&files, "lifted")
                .iter()
                .any(|r| r["function"].as_str() == Some(path)),
            "{path}: {:?}",
            residue_detail(&files, path)
        );
    }
    let source = dir.join("generated/first.rs");
    let original = std::fs::read(&source).unwrap();
    std::fs::write(
        dir.join("generated/first.rs"),
        "pub fn run(a:bool,b:bool)->bool {a || b}",
    )
    .unwrap();
    assert!(
        uorust::project_workspace(&dir, &opts).is_err(),
        "changed path module reused a compiler catalogue from other source bytes"
    );
    std::fs::write(source, original).unwrap();
    let cache = dir.join("verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("module_bindings"),
        &dumps,
        &cache,
        &dir.join("trial"),
        8,
    )
    .unwrap();
    assert_eq!((proof.proved, proof.refused), (2, 0), "{proof:?}");
    let decisions = verdicts(&cache);
    opts.mir = Some(mir_options(
        dumps,
        decisions.clone(),
        uorust::mir::Trial::Off,
    ));
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let rows = frontier_rows(&files, "lifted");
    let mut observations = String::new();
    for (i, line) in String::from_utf8(oracle.stdout)
        .unwrap()
        .lines()
        .enumerate()
    {
        let columns: Vec<_> = line.split_whitespace().collect();
        for (j, path) in paths.iter().enumerate() {
            let name = rows
                .iter()
                .find(|r| r["function"].as_str() == Some(path))
                .unwrap()["definition"]
                .as_str()
                .unwrap()
                .strip_prefix("Behavior::")
                .unwrap();
            let axioms = decisions
                .rows
                .values()
                .find(|r| r.function == *path)
                .and_then(|r| r.axioms.get(name))
                .cloned()
                .unwrap_or_default()
                .join(", ");
            observations.push_str(&format!("theorem{{name:moduleObservation{i}{j},parameters:[],statement:(({name} {} {}) = {}),proof:decide{{}},axioms:[{axioms}]}}\n",columns[0],columns[1],columns[j+2]));
        }
    }
    let out = dir.join("projection");
    uorust::write(&out, &files).unwrap();
    let file = out.join("src/Behavior.lex.tex");
    let template = text(&files, "src/Behavior.lex.tex");
    let verify = |checks: &str| {
        std::fs::write(
            &file,
            template.replace(
                "\\end{semanticmodule}",
                &format!("{checks}\\end{{semanticmodule}}"),
            ),
        )
        .unwrap();
        std::process::Command::new("lexlean")
            .args([
                "verify",
                "src/Behavior.lex.tex",
                "--diagnostic-format",
                "json",
            ])
            .current_dir(&out)
            .output()
            .unwrap()
    };
    let good = verify(&observations);
    assert!(
        good.status.success(),
        "{}{}",
        String::from_utf8_lossy(&good.stdout),
        String::from_utf8_lossy(&good.stderr)
    );
    let wrong = observations.replacen(" = false)", " = true)", 1);
    assert_ne!(wrong, observations);
    let rejected = verify(&wrong);
    assert!(
        !rejected.status.success() && String::from_utf8_lossy(&rejected.stdout).contains("LLV7002")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn residue_recovery_from_paired_compiler_mir_ue_124() {
    let dir = scratch("compiler-residue-recovery", &[
        ("Cargo.toml", "[package]\nname=\"pass_pair\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n"),
        ("src/lib.rs", "extern \"C\" { fn opaque(x:u32)->u32; }\npub fn identity(x:u32)->u32 { if std::mem::size_of::<u32>() == 0 {unsafe {opaque(x)}} else {x} }\n"),
        (".cargo/config.toml","[build]\nrustflags=[\"-Zmir-opt-level=4\"]\n")
    ]);
    let dump = dumped(&dir, "pass-pair");
    let mut opts = options("pass_pair");
    opts.mir = Some(mir_options(
        dump.clone(),
        Default::default(),
        uorust::mir::Trial::All,
    ));
    let name = "pass_pair::identity";
    let files = uorust::project_workspace(&dir, &opts).unwrap();
    let candidates = std::str::from_utf8(&files[uorust::mir::CANDIDATES])
        .unwrap()
        .parse::<toml::Table>()
        .unwrap();
    let row = candidates["candidate"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["function"].as_str() == Some(name))
        .unwrap();
    assert!(row["primary_residue"]
        .as_str()
        .is_some_and(|r| !r.is_empty()));
    assert_ne!(row["primary_mir_sha256"], row["runtime_mir_sha256"]);
    assert!(frontier_rows(&files, "lifted")
        .iter()
        .any(|r| r["function"].as_str() == Some(name)));
    let cache = dir.join("verdicts.toml");
    let proof = uorust::mir::prove_bounded(
        &dir,
        &options("pass_pair"),
        &dump,
        &cache,
        &dir.join("trial"),
        4,
    )
    .unwrap();
    assert_eq!((proof.proved, proof.refused), (1, 0), "{proof:?}");
    let index = dump.join("lib-pass_pair/residue-mir/pairs.json");
    std::fs::remove_file(index).unwrap();
    let original = uorust::project_workspace(&dir, &opts).unwrap();
    assert!(!frontier_rows(&original, "lifted")
        .iter()
        .any(|r| r["function"].as_str() == Some(name)));
    std::fs::remove_dir_all(dir).unwrap();
}
