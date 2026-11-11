//! UORust: any Cargo workspace to a complete, checked Prism project.
//!
//! The pipeline is the Foundry one. Ingest reads `cargo metadata`, the
//! manifests, `Cargo.lock` and every source file (with `syn`, keeping spans)
//! into facts. The router proposes a Prism record for each fact along the
//! [`ROUTES`]. Admission refuses what it cannot decide, with a typed reason
//! recorded as an open claim in `frontier.toml`, and never fills a gap with a
//! default. Projection writes the admitted records as LexLean modules, one per
//! facet, plus the register, scenarios, supply-chain document and lockfile, the
//! same bytes on every run.

#![forbid(unsafe_code)]

mod admit;
mod assets;
mod error;
mod facts;
mod ingest;
mod lex;
mod lift;
mod project;
mod route;

use std::collections::BTreeMap;
use std::path::Path;

pub use error::UorustError;
pub use project::Options;
pub use route::{classify, respond, Route, Routing, REQUEST_LIMIT, ROUTES};

/// The name of the file that lists what a projection wrote, with digests.
pub const MANIFEST: &str = "uorust.manifest";

/// Project the workspace at `root`: every file of the Prism project, by path.
pub fn project_workspace(
    root: &Path,
    options: &Options,
) -> Result<BTreeMap<String, Vec<u8>>, UorustError> {
    if !is_timestamp(&options.last_modified) {
        return Err(UorustError::BadTimestamp(options.last_modified.clone()));
    }
    let (workspace, syntax) = ingest::workspace(root, options.advisories.as_deref())?;
    let mut files = project::project(&workspace, &syntax, options);
    let manifest: String = files
        .iter()
        .map(|(path, bytes)| format!("{}  {path}\n", project::sha(bytes)))
        .collect();
    files.insert(MANIFEST.to_string(), manifest.into_bytes());
    Ok(files)
}

/// The workspace's name: its directory name.
pub fn default_name(root: &Path) -> Result<String, UorustError> {
    let root = root
        .canonicalize()
        .map_err(|e| error::io(root.to_path_buf(), e))?;
    Ok(root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string())
}

/// The commit time of `HEAD` in the workspace's git checkout, in UTC.
pub fn commit_time(root: &Path) -> Result<String, UorustError> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .current_dir(root)
        .output()
        .map_err(|_| UorustError::NoTimestamp)?;
    let seconds: i64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| UorustError::NoTimestamp)?;
    if !output.status.success() {
        return Err(UorustError::NoTimestamp);
    }
    Ok(utc(seconds))
}

/// `YYYY-MM-DDThh:mm:ssZ` for seconds since the Unix epoch.
fn utc(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Is `value` an instant of the form `YYYY-MM-DDThh:mm:ssZ`?
pub fn is_timestamp(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
}

/// Write a projection into `out`, replacing an earlier projection there and
/// refusing a directory that holds anything else.
pub fn write(out: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<(), UorustError> {
    let manifest = out.join(MANIFEST);
    if manifest.is_file() {
        let listed = std::fs::read_to_string(&manifest).map_err(|e| error::io(&manifest, e))?;
        for line in listed.lines() {
            if let Some((_, path)) = line.split_once("  ") {
                let path = out.join(path);
                if path.is_file() {
                    std::fs::remove_file(&path).map_err(|e| error::io(&path, e))?;
                }
            }
        }
    } else if out.is_dir() {
        let mut entries = std::fs::read_dir(out).map_err(|e| error::io(out, e))?;
        if entries.next().is_some() {
            return Err(UorustError::ForeignOutput(out.to_path_buf()));
        }
    }
    for (path, bytes) in files {
        let path = out.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| error::io(parent, e))?;
        }
        std::fs::write(&path, bytes).map_err(|e| error::io(&path, e))?;
    }
    Ok(())
}

/// Compare a fresh projection with the one in `out`, byte for byte.
pub fn check(out: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<(), UorustError> {
    let mut stale = Vec::new();
    for (path, bytes) in files {
        match std::fs::read(out.join(path)) {
            Ok(found) if &found == bytes => {}
            _ => stale.push(path.clone()),
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(UorustError::Stale(stale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_renders_the_epoch_and_a_leap_day() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert!(is_timestamp(&utc(1_762_884_000)));
        assert!(!is_timestamp("2026-11-11"));
    }
}
