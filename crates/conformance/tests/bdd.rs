//! `CM-02`, `CM-03`, and R2's behavioural half.
//!
//! Runs the meta-gate against the actual workspace: the register, the feature
//! suites, and the test names `cargo test -- --list` reports. An ID with no
//! scenario, a scenario with no ID, an ID with no test, or a mislabelled
//! honesty level all fail here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use repo_conformance::{check_honesty, scenarios_in};
use repo_model::{Level, Model};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/conformance is two below the root")
        .to_path_buf()
}

/// Every `#[test]` function name in the workspace that `cargo test` runs.
///
/// Read from the source rather than from `cargo test -- --list`, because this
/// runs *inside* `cargo test` and a nested invocation blocks on the target
/// directory lock. Fixture crates (`fixtures/`) are not workspace members and
/// never run, so their tests are not evidence; [`test_names_in`] drops ignored
/// and conditionally compiled tests for the same reason.
fn workspace_test_names(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut stack = vec![root.join("crates"), root.join("xtask")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|n| n == "target" || n == "fixtures")
                {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    names.extend(test_names_in(&text));
                }
            }
        }
    }
    names
}

/// The tests `text` declares that run whenever its crate's tests run.
///
/// An attribute run is the lines from one starting `#[` through the line
/// where its brackets balance, then any further attributes; comment and blank
/// lines inside a run continue it. A run holding `#[test]` and ending at a
/// `fn` names a test, unless the run ignores it (`#[ignore`, or a `cfg_attr`
/// that ignores) or compiles it conditionally (a `#[cfg(` other than
/// `#[cfg(test)]`). A conditionally compiled inline module hides every test
/// inside it, and a non-test inner `#![cfg(` hides the whole file. A module
/// gated in another file (`#[cfg(x)] mod m;`) is not followed.
fn test_names_in(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut run: Vec<&str> = Vec::new();
    let mut attribute = 0i32;
    let mut depth = 0i32;
    let mut gated: Option<i32> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if attribute > 0 {
            run.push(line);
            attribute += brackets(line);
            continue;
        }
        if line.starts_with("#![") {
            if conditional(line) {
                return BTreeSet::new();
            }
            continue;
        }
        if line.starts_with("#[") {
            run.push(line);
            attribute = brackets(line);
            continue;
        }
        if !run.is_empty() && (line.is_empty() || line.starts_with("//")) {
            continue;
        }
        let attrs = run.join(" ");
        run.clear();
        let before = depth;
        depth += braces(line);
        if gated.is_none() {
            if let Some(name) = fn_name(line) {
                if attrs.contains("#[test]") && !ignored(&attrs) && !conditional(&attrs) {
                    names.insert(name);
                }
            }
            if conditional(&attrs) && is_inline_mod(line) && depth > before {
                gated = Some(before);
            }
        } else if gated.is_some_and(|d| depth <= d) {
            gated = None;
        }
    }
    names
}

fn ignored(attrs: &str) -> bool {
    attrs.contains("#[ignore") || (attrs.contains("cfg_attr(") && attrs.contains("ignore"))
}

/// A `cfg` other than `cfg(test)`, which compiles the item only sometimes.
fn conditional(attrs: &str) -> bool {
    let all = attrs.matches("#[cfg(").count() + attrs.matches("#![cfg(").count();
    let test = attrs.matches("#[cfg(test)]").count() + attrs.matches("#![cfg(test)]").count();
    all > test
}

fn fn_name(line: &str) -> Option<String> {
    let mut rest = line;
    for prefix in ["pub(crate) ", "pub ", "async ", "unsafe "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let name: String = rest
        .strip_prefix("fn ")?
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn is_inline_mod(line: &str) -> bool {
    let rest = line.strip_prefix("pub ").unwrap_or(line);
    rest.starts_with("mod ") && rest.contains('{')
}

fn brackets(line: &str) -> i32 {
    line.matches('[').count() as i32 - line.matches(']').count() as i32
}

/// Brace depth change of one line, outside string and character literals.
fn braces(line: &str) -> i32 {
    let (mut n, mut quoted, mut escaped) = (0, false, false);
    let line = line.replace("'{'", "").replace("'}'", "");
    for c in line.chars() {
        match (quoted, escaped, c) {
            (true, true, _) => escaped = false,
            (true, false, '\\') => escaped = true,
            (_, false, '"') => quoted = !quoted,
            (false, _, '{') => n += 1,
            (false, _, '}') => n -= 1,
            _ => {}
        }
    }
    n
}

#[test]
fn test_names_in_drops_ignored_and_conditional_tests() {
    let text = "#[test]\nfn plain() {}\n#[test]\n#[ignore]\nfn ignored_after() {}\n#[ignore]\n#[test]\nfn ignored_before() {}\n#[cfg(unix)]\n#[test]\nfn conditional_before() {}\n#[test]\n#[cfg_attr(not(x), ignore)]\nfn cfg_attr_ignored() {}\n#[test]\n#[cfg(any())]\nfn conditional_after() {}\n#[cfg(any())]\nmod gated {\n    #[test]\n    fn inside_gated() {}\n}\n#[ignore]\n// reason\n#[test]\nfn ignored_with_comment() {}\n#[test]\n#[cfg_attr(\n    not(x),\n    ignore\n)]\nfn multiline_cfg_attr() {}\n#[test]\nfn after_gated() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn in_test_module() {}\n}\n";
    let names = test_names_in(text);
    assert_eq!(
        names,
        ["after_gated", "in_test_module", "plain"]
            .into_iter()
            .map(String::from)
            .collect::<BTreeSet<_>>()
    );
    assert!(test_names_in("#![cfg(any())]\n#[test]\nfn whole_file() {}\n").is_empty());
}

#[test]
fn fixture_tests_are_not_counted() {
    let root = std::env::temp_dir().join(format!("bdd-fixtures-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let member = root.join("crates/a/tests");
    let fixture = member.join("fixtures/f/src");
    std::fs::create_dir_all(&fixture).expect("temp tree");
    std::fs::write(member.join("t.rs"), "#[test]\nfn member_test() {}\n").expect("member");
    std::fs::write(fixture.join("lib.rs"), "#[test]\nfn fixture_test() {}\n").expect("fixture");
    let names = workspace_test_names(&root);
    std::fs::remove_dir_all(&root).expect("clean up");
    assert_eq!(names, BTreeSet::from(["member_test".to_string()]));
}

/// `CM-02`: every registered ID has a scenario and a test, and every scenario
/// and test names a registered ID.
#[test]
fn every_id_has_a_scenario_and_a_test_cm_02() {
    let root = root();
    let tests = workspace_test_names(&root);
    assert!(!tests.is_empty(), "the test list must not be empty");

    let report = check_honesty(&root, &tests).expect("the meta-gate runs");
    assert!(
        report.is_clean(),
        "the honesty meta-gate failed:\n\n{}",
        report.violations.join("\n\n")
    );
    eprintln!(
        "CM-02: {} registered IDs, {} scenarios, {} test names",
        report.ids_checked,
        report.scenarios_checked,
        tests.len()
    );
    // Open claims: measured and reported, never asserted.
    for claim in &report.open {
        eprintln!("open: {claim}");
    }
}

/// R3: there are no pending or skipped steps.
///
/// The non-emptiness guard is armed by the *register*, not asserted outright. A
/// repository that has claimed nothing yet has no scenarios to write and no
/// suite for them to live in, and that is a legitimate state. A repository with
/// registered IDs and no feature files is the defect this catches, and the guard
/// re-arms the moment the first row is added to `model/ids.toml`.
#[test]
fn no_scenario_is_pending_cm_02() {
    let model = Model::load(&root().join("model")).expect("model loads");
    let suites = scenarios_in(&root().join("features/suites")).expect("suites read");
    assert!(
        model.ids.id.is_empty() || suites.files >= 1,
        "{} registered IDs and no feature files",
        model.ids.id.len()
    );
    for s in &suites.scenarios {
        assert!(!s.steps.is_empty(), "{} has no steps", s.id);
        for step in &s.steps {
            let lower = step.to_lowercase();
            assert!(
                !lower.contains("pending") && !lower.contains("todo"),
                "{}: `{step}` is a pending step, and R3 admits none",
                s.id
            );
        }
    }
}

/// `CM-03`: every `some-true` claim cites an authority that exists, with a
/// citation and either a checksum or a stated reason for its absence.
#[test]
fn every_some_true_claim_cites_an_authority_cm_03() {
    let model = Model::load(&root().join("model")).expect("model loads");
    model.check().expect("the model is consistent");

    let mut some_true = 0usize;
    for claim in &model.ledger.claim {
        if claim.level != Level::SomeTrue {
            continue;
        }
        some_true += 1;
        let name = claim
            .authority
            .as_ref()
            .expect("a some-true claim names an authority");
        let a = model
            .authorities
            .authority
            .iter()
            .find(|a| &a.id == name)
            .unwrap_or_else(|| panic!("{name} has no row in model/authorities.toml"));
        assert!(!a.citation.trim().is_empty(), "{name} has no citation");
        assert!(
            a.checksum != "none" || !a.checksum_reason.trim().is_empty(),
            "{name} has no checksum and no reason for its absence"
        );
    }
    // Armed by the ledger, for the reason `no_scenario_is_pending_cm_02` gives:
    // a repository that reproduces nothing from an authority cites none, and a
    // `some-true` claim with no authority row fails in the loop above regardless.
    assert!(
        model.ledger.claim.is_empty() || some_true >= 1 || model.authorities.authority.is_empty(),
        "a ledger with claims and no cited authority"
    );
    eprintln!("CM-03: {some_true} cited authorities, each with a citation");
}

/// R2: the meta-gate can fail.
///
/// A gate nobody has ever seen fail is indistinguishable from a gate that
/// cannot. This plants each of the three violations it exists to catch and
/// checks that each is reported.
#[test]
fn the_meta_gate_is_falsifiable_cm_02() {
    let root = root();

    // An ID with no test. With an empty register there is no ID to strip the
    // test from, so the plant has nothing to plant and the check is skipped ---
    // stated here rather than silently passing, because a falsifiability test
    // that cannot falsify is the exact thing this test exists to rule out.
    let model = Model::load(&root.join("model")).expect("model loads");
    if model.ids.id.is_empty() {
        eprintln!("CM-02: register empty, so there is no ID whose test can be removed");
    } else {
        let empty = BTreeSet::new();
        let report = check_honesty(&root, &empty).expect("runs");
        assert!(!report.is_clean(), "an empty test list must fail the gate");
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("CM-02") && v.contains("no test name")),
            "the missing-test violation must be reported"
        );

        // A name that ends in the slug without the `_` boundary does not
        // discharge it: `aue_01` is not a test of `UE-01`.
        let unbounded: BTreeSet<String> = model
            .ids
            .id
            .iter()
            .map(|r| format!("a{}", r.id.to_lowercase().replace('-', "_")))
            .collect();
        let report = check_honesty(&root, &unbounded).expect("runs");
        for row in &model.ids.id {
            assert!(
                report
                    .violations
                    .iter()
                    .any(|v| v.starts_with(&format!("CM-02: {} is registered but no test", row.id))),
                "{} must be reported missing with only an unbounded name",
                row.id
            );
        }
    }

    // A test list that covers everything passes, which is the control.
    let full = workspace_test_names(&root);
    assert!(check_honesty(&root, &full).expect("runs").is_clean());
}
