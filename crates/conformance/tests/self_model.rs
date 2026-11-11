//! The `self-model` suite: one test per scenario in `features/suites/self-model.feature`.
//!
//! The proofs are checked by the Lean kernel when `just vv` runs
//! `prismpm verify` on `app/`, and the facets by `prismpm check`. What these
//! tests hold fixed is what those checks are about: that the verified table is
//! the table in `model/routes.toml` (which the Rust router replays too), that
//! the theorems quantify over the whole table, and that every facet is
//! populated. A dropped route, a narrowed theorem or an empty facet would still
//! verify, and would no longer be the claim the register makes.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/conformance is two below the root")
        .to_path_buf()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The body of the top-level declaration named `name`, up to its closing brace.
fn declaration<'a>(model: &'a str, name: &str) -> &'a str {
    let start = model
        .find(&format!("\n  name: {name},\n"))
        .unwrap_or_else(|| panic!("{name} is not declared"));
    let end = model[start..]
        .find("\n}\n")
        .unwrap_or_else(|| panic!("{name} is not closed"));
    &model[start..start + end]
}

/// A TOML basic string, as `model/routes.toml` writes them.
fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or_else(|| panic!("not a basic string: {value}"));
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                other => panic!("unexpected escape {other:?} in {value}"),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The tables named `[[name]]` in `model/routes.toml`, each as its key-value pairs.
fn tables(text: &str, name: &str) -> Vec<Vec<(String, String)>> {
    let header = format!("[[{name}]]");
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("[[") {
            inside = line == header;
            if inside {
                out.push(Vec::new());
            }
        } else if inside {
            if let Some((key, value)) = line.split_once(" = ") {
                out.last_mut()
                    .expect("a table")
                    .push((key.to_string(), unquote(value)));
            }
        }
    }
    out
}

fn field<'a>(table: &'a [(String, String)], key: &str) -> &'a str {
    table
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("no {key} in {table:?}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `SM-01`: UORust's application answers the route table of model/routes.toml route for route, and replays every router and workbench vector of that file in order
#[test]
fn self_model_sm_01() {
    let file = read("model/routes.toml");
    let model = read("app/src/Routes.lex.tex");
    let routes = tables(&file, "route");
    assert_eq!(routes.len(), 28, "the route table has 28 routes");

    let table = declaration(&model, "routes");
    let route_of = declaration(&model, "routeOf");
    for (index, route) in routes.iter().enumerate() {
        let record = format!(
            "(record Route {{index := {index}, id := \"{}\", rustFact := \"{}\", prismRecord := \"{}\", facet := \"{}\"}})",
            field(route, "id"),
            field(route, "rust_fact"),
            field(route, "prism_record"),
            field(route, "facet")
        );
        assert!(
            table.contains(&record),
            "routes lacks row {index}: {record}"
        );
        assert!(
            route_of.contains(&format!(
                "(if (prim equal tag \"{}\" : Bool) then (ctor Option.some [Route] {record})",
                field(route, "rust_fact")
            )),
            "routeOf does not answer {record}"
        );
    }
    assert_eq!(
        table.matches("(record Route {").count(),
        routes.len(),
        "the model's table has a row the file does not"
    );
    assert_eq!(
        route_of.matches("(record Route {").count(),
        routes.len(),
        "routeOf answers a fact the file does not route"
    );

    let mut expected = String::new();
    for name in ["vector", "workbench_vector"] {
        for vector in tables(&file, name) {
            expected.push_str(&format!(
                "(cons (record Foundation.View.Text.V1.Model::AcceptanceVector {{request := 0x{}, response := 0x{}}}) ",
                field(&vector, "request_hex"),
                hex(field(&vector, "response").as_bytes())
            ));
        }
    }
    let vectors = declaration(&model, "acceptanceVectors");
    assert!(
        vectors.contains(&format!(
            "body: {expected}(nil Foundation.View.Text.V1.Model::AcceptanceVector)"
        )),
        "the application's acceptance vectors are not the file's vectors, in order"
    );
    let application = declaration(&model, "application");
    assert!(
        application.contains("acceptanceVectors := (acceptanceVectors)")
            && application.contains("entryRoot := \"Uorust.Routes.dispatch\""),
        "the application must replay acceptanceVectors through dispatch:\n{application}"
    );
}

/// `SM-02`: The route table's theorems state, over the whole table and by decide, that every fact kind routes to its own row, that facts are distinct, and that any two kinds naming different routes are refused as ambiguous
#[test]
fn self_model_sm_02() {
    let model = read("app/src/Routes.lex.tex");
    for (name, statement) in [
        (
            "routeTableIsTotal",
            "statement: ((everyFactRoutesHome (routes)) = true),",
        ),
        (
            "routeFactsAreDistinct",
            "statement: ((firstMatchIsSelf (routes)) = true),",
        ),
        (
            "routeTableRefusesAmbiguity",
            "statement: ((everyPairDecided (routes) (routes)) = true),",
        ),
        (
            "routeIndexesArePositions",
            "statement: ((indexedFrom 0 (routes)) = true),",
        ),
    ] {
        let theorem = declaration(&model, name);
        assert!(
            theorem.contains(statement),
            "{name}'s statement changed:\n{theorem}"
        );
        assert!(
            theorem.contains("proof: decide{}") && !theorem.contains("axioms:"),
            "{name} is decided with no axiom:\n{theorem}"
        );
    }
    // Ambiguity is decided on the answer the application gives: classify is
    // what verdictOf calls, and agree refuses a second kind of another route.
    let pair = declaration(&model, "refusedAgainst");
    assert!(
        pair.contains("(isAmbiguous (classify (cons (project lead rustFact) (cons (project head rustFact) (nil String))))"),
        "refusedAgainst must decide ambiguity through classify:\n{pair}"
    );
    let agree = declaration(&model, "agree");
    assert!(
        agree.contains("then (agree lead leadTag more) else (ctor Verdict.Ambiguous leadTag tag)"),
        "a kind naming another route must be refused as ambiguous:\n{agree}"
    );
    let verdict = declaration(&model, "verdictOf");
    assert!(
        verdict.contains("(classify tags)"),
        "the application's verdict must be classify's:\n{verdict}"
    );
}

/// `SM-03`: UORust is modeled in every facet: architecture, design views, features and journeys, security with its threat model, quality, the browser system release and compliance, each with records, and every module title has a project-local glossary entry
#[test]
fn self_model_sm_03() {
    let facets: [(&str, &[(&str, usize)]); 7] = [
        (
            "app/src/Architecture.lex.tex",
            &[
                ("Foundation.Arch::Stakeholder", 5),
                ("Foundation.Arch::Concern", 7),
                ("Foundation.Arch::Component", 14),
                ("Foundation.Arch::Edge", 20),
                ("Foundation.Arch::Viewpoint", 7),
                ("Foundation.Arch::View", 7),
            ],
        ),
        (
            "app/src/Design.lex.tex",
            &[
                ("Foundation.Arch::DesignView", 4),
                ("Foundation.Arch::SystemProcess", 9),
                ("Foundation.Arch::SystemState", 5),
                ("Foundation.Arch::OpmThing", 10),
                ("Foundation.Arch::OpmLink", 8),
                ("Foundation.Arch::OpmZoom", 4),
                ("Foundation.Arch::UmlComponent", 5),
                ("Foundation.Arch::UmlMessage", 6),
                ("Foundation.Arch::UmlTransition", 4),
            ],
        ),
        (
            "app/src/Features.lex.tex",
            &[
                ("Foundation.Arch::Feature", 10),
                ("Foundation.Arch::Journey", 5),
            ],
        ),
        (
            "app/src/Security.lex.tex",
            &[
                ("Foundation.Sec::Asset", 6),
                ("Foundation.Sec::Threat", 11),
                ("Foundation.Sec::Risk", 11),
                ("Foundation.Sec::ApplicationSecurityControl", 13),
                ("Production.ThreatModel::ModeledControl", 13),
            ],
        ),
        (
            "app/src/Quality.lex.tex",
            &[
                ("Foundation.Qual::ProductQualityCharacteristic", 6),
                ("Foundation.Qual::QualityRequirement", 11),
                ("Foundation.Qual::QualityMeasure", 11),
            ],
        ),
        (
            "app/src/System.lex.tex",
            &[
                ("Production.BrowserSystem::SystemModel", 1),
                ("Production.BrowserSystem::SystemManifest", 1),
            ],
        ),
        ("app/src/Compliance.lex.tex", &[("(List Obligation)", 1)]),
    ];
    for (path, kinds) in facets {
        let module = read(path);
        for (kind, at_least) in kinds {
            let count = module.matches(&format!("  result: {kind},\n")).count();
            assert!(
                count >= *at_least,
                "{path} declares {count} {kind} records, fewer than {at_least}"
            );
        }
    }

    let mut surfaces = BTreeSet::new();
    for package in ["prism.arch", "prism.qual", "prism.sec"] {
        let dir = root().join("app/language").join(package).join("entries");
        for entry in std::fs::read_dir(&dir).expect("entries") {
            let text = std::fs::read_to_string(entry.expect("entry").path()).expect("entry");
            for line in text.lines() {
                if let Some(surface) = line.strip_prefix("surface = ") {
                    surfaces.insert(unquote(surface));
                }
            }
        }
    }
    let mut modules = vec![root().join("app/src")];
    while let Some(dir) = modules.pop() {
        for entry in std::fs::read_dir(&dir).expect("src") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                modules.push(path);
            } else if path.to_string_lossy().ends_with(".lex.tex") {
                let text = std::fs::read_to_string(&path).expect("module");
                let title = text
                    .lines()
                    .find_map(|l| l.strip_prefix("\\title{")?.strip_suffix('}'))
                    .unwrap_or_else(|| panic!("{} has no title", path.display()));
                assert!(
                    title == "Boolean" || surfaces.contains(title),
                    "{}: the title `{title}` has no glossary entry",
                    path.display()
                );
            }
        }
    }
}

/// `SM-04`: The threat model maps every threat to a control with evidence or accepts it with a rationale, and every control's evidence names a gate, proof or review
#[test]
fn self_model_sm_04() {
    let module = read("app/src/Security.lex.tex");
    let names_of = |kind: &str| -> Vec<String> {
        module
            .split("definition{\n  name: ")
            .skip(1)
            .filter(|d| d.contains(&format!("  result: {kind},\n")))
            .map(|d| d.split(',').next().expect("name").to_string())
            .collect()
    };
    let threats = names_of("Foundation.Sec::Threat");
    let controls = names_of("Foundation.Sec::ApplicationSecurityControl");
    assert!(!threats.is_empty() && !controls.is_empty());
    for threat in &threats {
        let id = format!("threat := \"Security::{threat}\"");
        let mapped = module
            .lines()
            .any(|l| l.contains("Production.ThreatModel::ThreatMapping {") && l.contains(&id));
        let accepted = module.lines().any(|l| {
            l.contains("Production.ThreatModel::AcceptedRisk {")
                && l.contains(&id)
                && !l.contains("rationale := \"\"")
        });
        assert!(
            mapped || accepted,
            "{threat} is neither mapped nor accepted"
        );
    }
    for control in &controls {
        let id = format!("control := \"Security::{control}\"");
        let row = module
            .lines()
            .find(|l| l.contains("Production.ThreatModel::ModeledControl {") && l.contains(&id))
            .unwrap_or_else(|| panic!("{control} has no modeled row"));
        assert!(
            row.contains("(cons (record Production.ThreatModel::Evidence {kind := (ctor Production.ThreatModel::EvidenceKind."),
            "{control} names no evidence"
        );
    }
}
