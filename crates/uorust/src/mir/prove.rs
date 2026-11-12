//! `uorust prove`: decide every translation of the MIR path with the
//! verifier, with no one in the loop.
//!
//! The prover projects the workspace with every undecided translation
//! admitted on trial, writes the projection to a scratch directory, and runs
//! `lexlean verify` on it. When it verifies, every translation on trial is
//! proved. When it does not, each diagnostic is attributed to the
//! declaration it names (or whose source lines it points at): a report of
//! the axioms a declaration elaborated with is learned, any other refusal
//! decides that translation refused, with the diagnostic as its cause. A
//! failure no diagnostic attributes splits the trial in halves until it
//! does. The verdicts are written after every decision, so an interrupted
//! run keeps what it decided.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::process::Command;

use super::verdict::{Decision, Verdict, Verdicts};
use super::{MirOptions, Trial, CANDIDATES};
use crate::error::io;
use crate::{Options, UorustError};

/// One translation a trial projection lists.
#[derive(Clone, Debug)]
struct Trialled {
    kappa: String,
    function: String,
    digest: String,
    trial: bool,
    decls: Vec<String>,
}

/// What a run decided.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    /// Translations proved by this run.
    pub proved: usize,
    /// Translations refused by this run.
    pub refused: usize,
    /// Verifier runs.
    pub runs: usize,
}

fn candidates(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<Trialled>, UorustError> {
    let text = String::from_utf8_lossy(files.get(CANDIDATES).map_or(&[][..], Vec::as_slice));
    let bad = |reason: String| UorustError::Malformed {
        path: CANDIDATES.into(),
        reason,
    };
    let doc: toml::Value = text.parse().map_err(|e| bad(format!("{e}")))?;
    let mut out = Vec::new();
    for row in doc
        .get("candidate")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let field = |n: &str| {
            row.get(n)
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| bad(format!("a candidate without {n}")))
        };
        out.push(Trialled {
            kappa: field("kappa")?,
            function: field("function")?,
            digest: field("digest")?,
            trial: row
                .get("trial")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            decls: row
                .get("decls")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect(),
        });
    }
    Ok(out)
}

/// A verifier diagnostic.
#[derive(Clone, Debug)]
struct Diagnostic {
    code: String,
    message: String,
    path: String,
    line: usize,
}

/// Run `lexlean verify` in `dir`: success, or its diagnostics.
fn verify(dir: &Path) -> Result<Option<Vec<Diagnostic>>, UorustError> {
    let run = Command::new("lexlean")
        .args(["verify", "--diagnostic-format", "json"])
        .current_dir(dir)
        .output()
        .map_err(|e| UorustError::Tool(format!("lexlean verify: {e}")))?;
    let doc: serde_json::Value = serde_json::from_slice(&run.stdout).map_err(|e| {
        UorustError::Tool(format!(
            "lexlean verify: {e}: {}",
            String::from_utf8_lossy(&run.stderr)
        ))
    })?;
    if doc.get("success").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(None);
    }
    let mut out = Vec::new();
    for d in doc
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let s = |v: Option<&serde_json::Value>| {
            v.and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let primary = d.get("primary");
        out.push(Diagnostic {
            code: s(d.get("code")),
            message: s(d.get("message")),
            path: s(primary.and_then(|p| p.get("path"))),
            line: primary
                .and_then(|p| p.get("line_start"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|l| usize::try_from(l).ok())
                .unwrap_or(0),
        });
    }
    Ok(Some(out))
}

/// The declarations a diagnostic names: `` `Prefix.Behavior.name` `` in its
/// message, or the declaration of `Behavior.lex.tex` at its line.
fn named(d: &Diagnostic, behavior: &str) -> Vec<String> {
    let mut out: Vec<String> = d
        .message
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|q| q.contains(".Behavior."))
        .filter_map(|q| q.rsplit('.').next().map(str::to_string))
        .collect();
    if out.is_empty() && d.path.ends_with("Behavior.lex.tex") && d.line > 0 {
        let mut current = None;
        for (i, line) in behavior.lines().enumerate() {
            if let Some(n) = line.strip_prefix("  name: ") {
                if behavior
                    .lines()
                    .nth(i.wrapping_sub(1))
                    .is_some_and(|p| p.ends_with('{'))
                {
                    current = Some(n.trim_end_matches(',').to_string());
                }
            }
            if i + 1 >= d.line {
                break;
            }
        }
        out.extend(current);
    }
    out
}

/// The axioms a `LLV7005` report says a declaration elaborated with.
fn observed(d: &Diagnostic) -> Option<Vec<String>> {
    if d.code != "LLV7005" {
        return None;
    }
    let list = d.message.rsplit("observed [").next()?.split(']').next()?;
    Some(
        list.split(',')
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// Decide every undecided translation of the workspace at `root`, writing
/// the verdict register to `verdicts` after each decision.
pub fn prove(
    root: &Path,
    options: &Options,
    dumps: &Path,
    verdicts: &Path,
    scratch: &Path,
) -> Result<Summary, UorustError> {
    let mut register = Verdicts::read(verdicts)?;
    let mut summary = Summary::default();
    // Axioms learned for declarations of translations still on trial.
    let mut learned: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    let mut groups: VecDeque<Trial> = VecDeque::from([Trial::All]);
    while let Some(trial) = groups.pop_front() {
        let current = register.clone();
        let opts = Options {
            mir: Some(MirOptions {
                dumps: dumps.to_path_buf(),
                verdicts: current,
                trial: trial.clone(),
                // A translation on trial elaborates with what was learned for it.
                learned: learned.clone(),
            }),
            ..options.clone()
        };
        let mut files = crate::project_workspace(root, &opts)?;
        let all = candidates(&files)?;
        // Rows of translations that no longer exist are dropped.
        let live: BTreeSet<(&String, &String)> =
            all.iter().map(|c| (&c.kappa, &c.digest)).collect();
        register.rows.retain(|k, v| live.contains(&(k, &v.digest)));
        let on_trial: Vec<&Trialled> = all.iter().filter(|c| c.trial).collect();
        if on_trial.is_empty() {
            continue;
        }
        files.remove(CANDIDATES);
        write_scratch(scratch, &files)?;
        summary.runs += 1;
        let behavior = String::from_utf8_lossy(
            files
                .iter()
                .find(|(p, _)| p.ends_with("Behavior.lex.tex"))
                .map_or(&[][..], |(_, b)| b.as_slice()),
        )
        .into_owned();
        let Some(diagnostics) = verify(scratch)? else {
            for c in &on_trial {
                register.rows.insert(
                    c.kappa.clone(),
                    Verdict {
                        function: c.function.clone(),
                        digest: c.digest.clone(),
                        status: Decision::Proved,
                        axioms: learned.get(&c.kappa).cloned().unwrap_or_default(),
                    },
                );
                summary.proved += 1;
            }
            std::fs::write(verdicts, register.to_text()).map_err(|e| io(verdicts, e))?;
            continue;
        };
        let owner = |decl: &str| -> Vec<&Trialled> {
            on_trial
                .iter()
                .filter(|c| c.decls.iter().any(|d| d == decl))
                .copied()
                .collect()
        };
        let mut progress = false;
        for d in &diagnostics {
            for decl in named(d, &behavior) {
                for c in owner(&decl) {
                    if let Some(axioms) = observed(d) {
                        let entry = learned.entry(c.kappa.clone()).or_default();
                        if entry.get(&decl) != Some(&axioms) {
                            entry.insert(decl.clone(), axioms);
                            progress = true;
                        }
                    } else if !register.rows.contains_key(&c.kappa) {
                        let first = d.message.lines().next().unwrap_or_default();
                        register.rows.insert(
                            c.kappa.clone(),
                            Verdict {
                                function: c.function.clone(),
                                digest: c.digest.clone(),
                                status: Decision::Refused(format!("{}: {first}", d.code)),
                                axioms: BTreeMap::new(),
                            },
                        );
                        summary.refused += 1;
                        progress = true;
                    }
                }
            }
        }
        if progress {
            std::fs::write(verdicts, register.to_text()).map_err(|e| io(verdicts, e))?;
            groups.push_front(trial);
            continue;
        }
        // Nothing attributed: split the trial.
        let kappas: Vec<String> = on_trial.iter().map(|c| c.kappa.clone()).collect();
        if let [only] = kappas.as_slice() {
            let c = on_trial[0];
            let first = diagnostics
                .first()
                .map(|d| {
                    format!(
                        "{}: {}",
                        d.code,
                        d.message.lines().next().unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| "the verifier failed without a diagnostic".to_string());
            register.rows.insert(
                only.clone(),
                Verdict {
                    function: c.function.clone(),
                    digest: c.digest.clone(),
                    status: Decision::Refused(first),
                    axioms: BTreeMap::new(),
                },
            );
            summary.refused += 1;
            std::fs::write(verdicts, register.to_text()).map_err(|e| io(verdicts, e))?;
            continue;
        }
        let half = kappas.len() / 2;
        groups.push_front(Trial::Only(kappas[half..].iter().cloned().collect()));
        groups.push_front(Trial::Only(kappas[..half].iter().cloned().collect()));
    }
    std::fs::write(verdicts, register.to_text()).map_err(|e| io(verdicts, e))?;
    Ok(summary)
}

/// Replace the scratch projection with `files`, keeping the verifier's
/// build cache, whose entries are content-addressed.
fn write_scratch(scratch: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<(), UorustError> {
    if scratch.is_dir() {
        for entry in std::fs::read_dir(scratch).map_err(|e| io(scratch, e))? {
            let path = entry.map_err(|e| io(scratch, e))?.path();
            if path.file_name().is_some_and(|n| n == ".lexlean") {
                continue;
            }
            if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(|e| io(&path, e))?;
            } else {
                std::fs::remove_file(&path).map_err(|e| io(&path, e))?;
            }
        }
    }
    for (path, bytes) in files {
        let path = scratch.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        std::fs::write(&path, bytes).map_err(|e| io(&path, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_name_their_declarations() {
        let d = Diagnostic {
            code: "LLV7005".into(),
            message: "`Uorust.Behavior.mirSum` violates its exact axiom policy: observed [Quot.sound, propext]".into(),
            path: "lexlean.toml".into(),
            line: 1,
        };
        assert_eq!(named(&d, ""), ["mirSum"]);
        assert_eq!(
            observed(&d),
            Some(vec!["Quot.sound".to_string(), "propext".to_string()])
        );
        let behavior = "x\ndefinition{\n  name: a,\n  body: 1\n}\n\ntheorem{\n  name: b,\n  statement: (1 = 1)\n}\n";
        let at = Diagnostic {
            code: "LLV7001".into(),
            message: "omega could not prove the goal".into(),
            path: "src/Behavior.lex.tex".into(),
            line: 9,
        };
        assert_eq!(named(&at, behavior), ["b"]);
        assert_eq!(observed(&at), None);
    }
}
