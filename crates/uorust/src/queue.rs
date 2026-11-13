//! Merging the module files a sharded run of the MIR path writes into its
//! register.
//!
//! A sharded run translates each module of a crate on its own runner and
//! writes that module's file; [`merge`] admits each file against the
//! register's own (no κ the register records as translated may come back as
//! residue) and writes the summary over the merged register, whose ranked
//! causes the Quality facet reads as the cause queue.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::mir::core::{summarize_register, Scope};
use crate::UorustError;

fn io(p: &Path, e: impl std::fmt::Display) -> UorustError {
    UorustError::Tool(format!("{}: {e}", p.display()))
}

/// A register's module files: every `<module>.toml` but `frontier.toml`.
fn module_files(dir: &Path) -> Result<Vec<PathBuf>, UorustError> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "toml")
                && p.file_stem().is_some_and(|n| n != "frontier")
        })
        .collect();
    files.sort();
    Ok(files)
}

/// The rows of one module file.
fn rows(file: &Path) -> Result<Vec<toml::Value>, UorustError> {
    let text = std::fs::read_to_string(file).map_err(|e| io(file, e))?;
    let doc: toml::Table = text.parse().map_err(|e| io(file, e))?;
    Ok(doc
        .get("fn")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn field<'a>(row: &'a toml::Value, key: &str) -> &'a str {
    row.get(key).and_then(toml::Value::as_str).unwrap_or("")
}

/// The rows of `new` that `old` records as translated and `new` as residue,
/// by κ, each as `path: cause`.
pub fn regressions(old: &[toml::Value], new: &[toml::Value]) -> Vec<String> {
    let now: BTreeMap<&str, &toml::Value> = new.iter().map(|r| (field(r, "kappa"), r)).collect();
    old.iter()
        .filter(|r| field(r, "status") == "translated")
        .filter_map(|r| now.get(field(r, "kappa")))
        .filter(|r| field(r, "status") != "translated")
        .map(|r| format!("{}: {}", field(r, "path"), field(r, "cause")))
        .collect()
}

/// Merge the module files under `shards` into the register `register` of
/// the crate `scope` names, refusing the merge when a body the register
/// records as translated is residue in a shard, then rewrite its summary.
/// Returns the modules merged.
pub fn merge(scope: &Scope, register: &Path, shards: &Path) -> Result<Vec<String>, UorustError> {
    std::fs::create_dir_all(register).map_err(|e| io(register, e))?;
    let files = module_files(shards)?;
    let mut regressed = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap_or_default();
        let old = register.join(name);
        if old.exists() {
            regressed.extend(regressions(&rows(&old)?, &rows(file)?));
        }
    }
    if !regressed.is_empty() {
        return Err(UorustError::Tool(format!(
            "a body the {} register records as translated is now residue: {}",
            scope.krate,
            regressed.join("; ")
        )));
    }
    let mut merged = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap_or_default();
        std::fs::copy(file, register.join(name)).map_err(|e| io(file, e))?;
        merged.push(
            file.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string(),
        );
    }
    summarize_register(scope, register)?;
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kappa: &str, status: &str) -> toml::Value {
        let mut t = toml::Table::new();
        t.insert("kappa".into(), kappa.into());
        t.insert("path".into(), format!("core::m::{kappa}").into());
        t.insert("status".into(), status.into());
        t.insert("cause".into(), "mir::Ty: f32".into());
        toml::Value::Table(t)
    }

    #[test]
    fn a_translated_kappa_may_not_turn_residue() {
        let old = [row("a", "translated"), row("b", "residue")];
        assert!(regressions(&old, &[row("a", "translated"), row("b", "translated")]).is_empty());
        // A body whose κ changed is a new question, not a regression.
        assert!(regressions(&old, &[row("c", "residue")]).is_empty());
        assert_eq!(
            regressions(&old, &[row("a", "residue")]),
            ["core::m::a: mir::Ty: f32"]
        );
    }
}
