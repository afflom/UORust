//! CI-01: acceptance of the trusted-base, advisory change-impact reporter.
//! Python fixtures plant missing refs, graph-consumed documentation, renames,
//! symlinks, mixed changes and an untrusted PR-head classifier override.
//!
//! The script suite runner: every Python and shell test suite under
//! `scripts/` runs under `just vv`, refused when it fails, runs no test or
//! skips one, and must lie inside the factory's `test` obligation, so the
//! factory's receipt covers what `vv` runs. The node suites `try/test.mjs`
//! and `identity/test.mjs` drive Chromium through Playwright, which the SDK
//! image does not carry; they stay in `try-page.yml`.

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/conformance is two below the root")
        .to_path_buf()
}

#[test]
fn trusted_change_impact_classification_ci_01() {
    let output = Command::new("python3")
        .args([
            "-I",
            "-m",
            "unittest",
            "discover",
            "-s",
            "scripts/ci",
            "-p",
            "test_pr_stage.py",
            "-v",
        ])
        .current_dir(root())
        .output()
        .expect("Python regression suite is executable");
    let (out, err) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    if let Err(why) = verdict(Kind::Python, output.status.success(), &out, &err) {
        panic!("CI-01 regression suite: {why}\nstdout:\n{out}\nstderr:\n{err}");
    }
}

#[test]
fn inventory_diff_names_both_directions() {
    let v = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(
        inventory_diff(&v(&["a", "b"]), &v(&["b", "a"])),
        Vec::<String>::new()
    );
    assert_eq!(
        inventory_diff(&v(&["a", "b"]), &v(&["a"])),
        vec!["listed, not in this tree: b".to_string()]
    );
    assert_eq!(
        inventory_diff(&v(&["a"]), &v(&["a", "c"])),
        vec![format!("in this tree, not listed in {INVENTORY}: c")]
    );
}

#[test]
fn verdict_accepts_only_a_suite_that_ran_and_passed() {
    let pass = true;
    let fail = false;
    assert_eq!(
        verdict(Kind::Python, pass, "", "Ran 3 tests in 0.1s\n\nOK\n"),
        Ok(3)
    );
    assert_eq!(
        verdict(Kind::Python, pass, "", "Ran 1 test in 0.0s\n\nOK\n"),
        Ok(1)
    );
    assert_eq!(
        verdict(Kind::Shell, pass, "5 isolated shell scenarios passed\n", ""),
        Ok(5)
    );
    for (kind, status, out, err) in [
        (Kind::Python, fail, "", "Ran 3 tests in 0.1s\n\nOK\n"),
        (Kind::Python, pass, "", "OK\n"),
        (Kind::Python, pass, "", "Ran 0 tests in 0.0s\n\nOK\n"),
        (
            Kind::Python,
            pass,
            "",
            "Ran 3 tests in 0.1s\n\nOK (skipped=1)\n",
        ),
        (
            Kind::Python,
            pass,
            "",
            "Ran 3 tests in 0.1s\n\nOK (expected failures=1)\n",
        ),
        (
            Kind::Python,
            pass,
            "",
            "Ran 3 tests in 0.1s\n\nFAILED (failures=1)\n",
        ),
        (Kind::Shell, pass, "", ""),
        (Kind::Shell, pass, "0 isolated shell scenarios passed\n", ""),
        (Kind::Shell, pass, "", "Ran 3 tests in 0.1s\n\nOK\n"),
        (
            Kind::Python,
            pass,
            "5 isolated shell scenarios passed\n",
            "",
        ),
    ] {
        assert!(
            verdict(kind, status, out, err).is_err(),
            "{kind:?} {out:?} {err:?}"
        );
    }
}

#[test]
fn stale_names_exclusions_that_no_longer_exist() {
    let discovered = ["scripts/ci/test_pr_stage.py".to_string()];
    assert!(stale(&["scripts/ci/test_pr_stage.py"], &discovered).is_empty());
    assert_eq!(
        stale(&["scripts/ci/test_gone.py"], &discovered),
        ["scripts/ci/test_gone.py"]
    );
}

#[test]
fn judge_coverage_refuses_a_run_it_cannot_trust() {
    let pass = true;
    assert_eq!(
        judge_coverage(pass, "a.py\nchecked 8\n", 8),
        Ok(vec!["a.py".to_string()])
    );
    assert_eq!(judge_coverage(pass, "checked 8\n", 8), Ok(vec![]));
    assert!(judge_coverage(false, "checked 8\n", 8).is_err());
    assert!(judge_coverage(pass, "a.py\n", 8).is_err());
    assert!(judge_coverage(pass, "checked 1\n", 8).is_err());
    assert!(judge_coverage(pass, "checked 9\n", 8).is_err());
}

#[test]
fn uncovered_follows_the_factory_rule() {
    let release = "scripts/release/test_platforms.py";
    assert_eq!(
        uncovered(Some(&["scripts/", "!scripts/release/"]), &[release]),
        Ok(vec![release.to_string()])
    );
    let content = "scripts/content/test_x.py";
    assert_eq!(
        uncovered(Some(&["scripts/ci/"]), &[content]),
        Ok(vec![content.to_string()])
    );
    assert_eq!(uncovered(Some(&["scripts/"]), &[release]), Ok(vec![]));
}

/// How a suite is run and how its summary is read.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// `python3 -I <path>`; unittest's summary is on stderr.
    Python,
    /// `bash <path>`; the summary line is on stdout.
    Shell,
}

impl Kind {
    fn of(path: &str) -> Kind {
        if path.ends_with(".sh") {
            Kind::Shell
        } else {
            Kind::Python
        }
    }

    fn program(self) -> &'static str {
        match self {
            Kind::Python => "python3",
            Kind::Shell => "bash",
        }
    }
}

/// Suites `vv` already runs through another test, with the reason. Each of
/// those runs refuses a failing, empty or skipping suite as [`verdict`] does:
/// CI-01 above through [`verdict`], `test_change_impact.py` through
/// `test_pr_stage.py`'s nested check, and `test_pages_identity.py` through
/// its own `complete_result`, which PG-01 runs, as `test_obligation_plan.py`
/// does under CI-02.
const ELSEWHERE: [(&str, &str); 4] = [
    ("scripts/ci/test_pr_stage.py", "run by CI-01 above"),
    (
        "scripts/ci/test_change_impact.py",
        "run by CI-01's suite, which checks its count and summary",
    ),
    (
        "scripts/ci/test_pages_identity.py",
        "run by PG-01 (pages_identity.rs); refuses an incomplete run itself",
    ),
    (
        "scripts/ci/test_obligation_plan.py",
        "run by CI-02 below; refuses an incomplete run itself",
    ),
];

/// The committed list of every suite, one path per line. Discovery reads the
/// tree it runs in, and the factory runs the `test` obligation in a closure
/// that holds only the obligation's inputs, so there a suite outside them
/// would be absent rather than refused. The list lies in an input
/// (`scripts/ci/`): a listed suite the closure lacks is refused, and the full
/// checkout refuses a list that omits a suite.
const INVENTORY: &str = "scripts/ci/suites.txt";

/// How the inventory and the discovered suites differ, one line each.
fn inventory_diff(listed: &[String], found: &[String]) -> Vec<String> {
    let mut out: Vec<String> = listed
        .iter()
        .filter(|path| !found.contains(path))
        .map(|path| format!("listed, not in this tree: {path}"))
        .collect();
    out.extend(
        found
            .iter()
            .filter(|path| !listed.contains(path))
            .map(|path| format!("in this tree, not listed in {INVENTORY}: {path}")),
    );
    out
}

/// The paths [`INVENTORY`] lists.
fn listed() -> Vec<String> {
    std::fs::read_to_string(root().join(INVENTORY))
        .unwrap_or_else(|e| panic!("{INVENTORY}: {e}"))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

/// The number of tests a passing suite ran, or why its run is refused: a
/// failing exit, no test, a skipped or expected-failure test, a missing summary.
fn verdict(kind: Kind, passed: bool, stdout: &str, stderr: &str) -> Result<usize, String> {
    if !passed {
        return Err("the suite exited non-zero".into());
    }
    let count = match kind {
        Kind::Python => python_count(stderr)?,
        Kind::Shell => shell_count(stdout)?,
    };
    if count == 0 {
        return Err("the suite ran no test".into());
    }
    Ok(count)
}

fn python_count(stderr: &str) -> Result<usize, String> {
    let ran = stderr
        .lines()
        .find_map(|line| line.strip_prefix("Ran ")?.split(' ').next()?.parse().ok())
        .ok_or("no `Ran N test(s)` line")?;
    match stderr.lines().rev().find(|line| !line.trim().is_empty()) {
        Some("OK") => Ok(ran),
        other => Err(format!("summary {other:?} is not exactly `OK`")),
    }
}

fn shell_count(stdout: &str) -> Result<usize, String> {
    stdout
        .lines()
        .find_map(|line| {
            line.strip_suffix(" isolated shell scenarios passed")?
                .parse()
                .ok()
        })
        .ok_or_else(|| "no `N isolated shell scenarios passed` line".into())
}

/// The `ELSEWHERE` entries that discovery no longer finds.
fn stale<'a>(elsewhere: &[&'a str], discovered: &[String]) -> Vec<&'a str> {
    elsewhere
        .iter()
        .filter(|path| !discovered.iter().any(|found| found == *path))
        .copied()
        .collect()
}

/// Loads `prism-factory/graph.py` and prints every path its `covers` rejects,
/// then `checked N`, counting each path it judged.
const COVERAGE: &str = r#"
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location("graph", "prism-factory/graph.py")
graph = importlib.util.module_from_spec(spec)
spec.loader.exec_module(graph)
mode, value, paths = sys.argv[1], sys.argv[2], sys.argv[3:]
if mode == "--inputs":
    inputs = json.loads(value)
elif mode == "--obligation":
    inputs = next(o["inputs"] for o in graph.load()[2] if o["name"] == value)
else:
    sys.exit("unknown mode " + mode)
checked = 0
for path in paths:
    checked += 1
    if not graph.covers(inputs, path):
        print(path)
print(f"checked {checked}")
"#;

/// The uncovered paths of a coverage run, or why the run is not trusted.
fn judge_coverage(passed: bool, stdout: &str, expected: usize) -> Result<Vec<String>, String> {
    if !passed {
        return Err("the coverage check exited non-zero".into());
    }
    let mut lines: Vec<&str> = stdout.lines().collect();
    let last = lines.pop().unwrap_or_default();
    match last
        .strip_prefix("checked ")
        .and_then(|n| n.parse::<usize>().ok())
    {
        Some(n) if n == expected => Ok(lines.into_iter().map(String::from).collect()),
        Some(n) => Err(format!("checked {n} paths, expected {expected}")),
        None => Err(format!("last line {last:?} is not `checked N`")),
    }
}

/// The paths the factory's `covers` rejects, against `inputs` or, when `None`,
/// the `test` obligation's inputs from `graph.load()`.
fn uncovered(inputs: Option<&[&str]>, paths: &[&str]) -> Result<Vec<String>, String> {
    let (mode, value) = match inputs {
        Some(list) => {
            let quoted: Vec<String> = list.iter().map(|i| format!("{i:?}")).collect();
            ("--inputs", format!("[{}]", quoted.join(",")))
        }
        None => ("--obligation", "test".to_string()),
    };
    let output = Command::new("python3")
        .args(["-I", "-c", COVERAGE, mode, &value])
        .args(paths)
        .current_dir(root())
        .output()
        .map_err(|e| format!("python3: {e}"))?;
    judge_coverage(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
        paths.len(),
    )
}

/// Every tracked Python and shell test suite under `scripts/`.
fn discovered() -> Vec<String> {
    let output = Command::new("git")
        .args([
            "ls-files",
            ":(glob)scripts/**/test_*.py",
            ":(glob)scripts/**/test-*.sh",
        ])
        .current_dir(root())
        .output()
        .expect("git lists the tracked suites");
    assert!(output.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn script_suites_pass() {
    let found = discovered();
    assert_eq!(
        inventory_diff(&listed(), &found),
        Vec::<String>::new(),
        "the suite inventory and this tree disagree"
    );
    let elsewhere: Vec<&str> = ELSEWHERE.iter().map(|(path, _)| *path).collect();
    assert_eq!(
        stale(&elsewhere, &found),
        Vec::<&str>::new(),
        "stale ELSEWHERE entries"
    );
    let suites: Vec<&str> = found
        .iter()
        .map(String::as_str)
        .filter(|path| !elsewhere.contains(path))
        .collect();
    assert!(!suites.is_empty(), "discovery found no suite");
    for suite in &suites {
        let kind = Kind::of(suite);
        let mut command = Command::new(kind.program());
        if kind == Kind::Python {
            command.arg("-I");
        }
        let output = command
            .arg(suite)
            .current_dir(root())
            .output()
            .expect("suite runs");
        let (out, err) = (
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        if let Err(why) = verdict(kind, output.status.success(), &out, &err) {
            panic!("{suite}: {why}\nstdout:\n{out}\nstderr:\n{err}");
        }
    }
    let all: Vec<&str> = found.iter().map(String::as_str).collect();
    assert_eq!(
        uncovered(None, &all),
        Ok(vec![]),
        "suites outside the factory `test` obligation"
    );
}

#[test]
fn factory_closure_holds() {
    let output = Command::new("python3")
        .args(["-I", "prism-factory/graph.py", "reads"])
        .current_dir(root())
        .output()
        .expect("graph.py runs");
    assert!(
        output.status.success(),
        "an obligation reads a file it does not declare:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// CI-02 shares the strict Python suite invoked by CI-01, but its registered
/// test name makes the new behavior independently visible to conformance.
#[test]
fn trusted_factory_obligation_plan_ci_02() {
    let output = Command::new("python3")
        .args(["-I", "scripts/ci/test_obligation_plan.py"])
        .current_dir(root())
        .output()
        .expect("CI-02 fixture suite is executable");
    assert!(
        output.status.success(),
        "CI-02 negative controls failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
