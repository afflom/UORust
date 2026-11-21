//! The one error type a run of the engine can end in.
//!
//! An ambiguity in the workspace is not an error: admission records it as a
//! typed refusal (an open claim in `frontier.toml`) and the run continues. An
//! error is a condition under which no projection exists at all.

use std::fmt;
use std::path::{Path, PathBuf};

/// Why no projection was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UorustError {
    /// `cargo metadata` could not be run or did not succeed.
    CargoMetadata(String),
    /// A manifest, lockfile or metadata document did not parse.
    Malformed {
        /// The document.
        path: PathBuf,
        /// What the parser reported.
        reason: String,
    },
    /// A file could not be read or written.
    Io {
        /// The file.
        path: PathBuf,
        /// What the operating system reported.
        reason: String,
    },
    /// The control set needs a `lastModified` instant and none was given: no
    /// `--last-modified`, and the workspace is not a git checkout with a commit.
    NoTimestamp,
    /// A `--last-modified` value is not `YYYY-MM-DDThh:mm:ssZ`.
    BadTimestamp(String),
    /// The output directory holds files this engine did not write.
    ForeignOutput(PathBuf),
    /// `uorust check` found the committed projection differs from a fresh one.
    Stale(Vec<String>),
    /// A function the incumbent lift register records as lifted (a proved
    /// claim, by κ) is decided unlifted by this projection.
    Regressed(Vec<String>),
    /// A module of the projection is larger than the `max_file_bytes` the
    /// projection's own `lexlean.toml` states, so LexLean would refuse the
    /// project it is part of.
    ModuleTooLarge {
        /// The module's path in the projection.
        module: String,
        /// Its size in bytes.
        bytes: usize,
        /// The limit the projection states.
        limit: usize,
    },
    /// A tool the engine runs could not be run or failed: `cargo rustc`
    /// dumping MIR, or `lexlean verify` deciding the MIR path's theorems.
    Tool(String),
    /// The ratchet: more translator functions are uncertified than in the
    /// parent commit; the first line counts both, each other names a new
    /// uncertified function and its cause.
    Grown(Vec<String>),
    /// The differential: an output of the certified translator differs from
    /// the hand-written one's, by κ; each names the output and both κ.
    Disagree(Vec<String>),
}

impl fmt::Display for UorustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CargoMetadata(reason) => write!(f, "cargo metadata failed: {reason}"),
            Self::Malformed { path, reason } => {
                write!(f, "{} does not parse: {reason}", path.display())
            }
            Self::Io { path, reason } => write!(f, "{}: {reason}", path.display()),
            Self::NoTimestamp => write!(
                f,
                "no lastModified instant: pass --last-modified YYYY-MM-DDThh:mm:ssZ or run \
                 inside a git checkout with a commit"
            ),
            Self::BadTimestamp(value) => {
                write!(
                    f,
                    "{value} is not an instant of the form YYYY-MM-DDThh:mm:ssZ"
                )
            }
            Self::ForeignOutput(path) => write!(
                f,
                "{} holds files uorust did not write; choose an empty or uorust-owned directory",
                path.display()
            ),
            Self::Stale(paths) => write!(
                f,
                "the projection is stale; these files differ: {}",
                paths.join(", ")
            ),
            Self::Regressed(rows) => write!(
                f,
                "a lifted function is no longer lifted, and a lifted κ is never re-decided: {}",
                rows.join("; ")
            ),
            Self::ModuleTooLarge {
                module,
                bytes,
                limit,
            } => write!(
                f,
                "the module {module} is {bytes} bytes, beyond the max_file_bytes of {limit} the \
                 projection states in lexlean.toml"
            ),
            Self::Tool(reason) => write!(f, "{reason}"),
            Self::Disagree(rows) => write!(
                f,
                "the certified and the hand-written translator disagree: {}",
                rows.join("; ")
            ),
            Self::Grown(rows) => write!(
                f,
                "the hand-written translator may only shrink: {}",
                rows.join("\n  ")
            ),
        }
    }
}

impl std::error::Error for UorustError {}

pub(crate) fn io(path: &Path, error: std::io::Error) -> UorustError {
    UorustError::Io {
        path: path.to_path_buf(),
        reason: error.to_string(),
    }
}

/// Whether the engine may remove `dir` while running in `cwd`: a directory
/// that does not exist holds nothing to lose; one holding `.git` (a checkout,
/// or a worktree's `.git` file) or holding `cwd` itself is never the engine's.
/// This does not prove the engine wrote `dir`; it rules out the directories
/// whose loss cannot be undone from the repository.
pub(crate) fn owned(dir: &Path, cwd: &Path) -> Result<bool, UorustError> {
    if std::fs::symlink_metadata(dir).is_err() {
        return Ok(true);
    }
    if std::fs::symlink_metadata(dir.join(".git")).is_ok() {
        return Ok(false);
    }
    let dir = dir.canonicalize().map_err(|e| io(dir, e))?;
    let cwd = cwd.canonicalize().map_err(|e| io(cwd, e))?;
    Ok(!cwd.starts_with(&dir))
}

/// Remove `dir` when [`owned`] admits it; otherwise remove nothing and refuse
/// it as not the engine's output.
pub(crate) fn remove_owned(dir: &Path) -> Result<(), UorustError> {
    if std::fs::symlink_metadata(dir).is_err() {
        return Ok(());
    }
    let cwd = std::env::current_dir().map_err(|e| io(dir, e))?;
    if !owned(dir, &cwd)? {
        return Err(UorustError::ForeignOutput(dir.to_path_buf()));
    }
    std::fs::remove_dir_all(dir).map_err(|e| io(dir, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SEC-08, on temporary directories only: the process's current
    /// directory is neither changed nor removed.
    #[test]
    fn owned_refuses_a_checkout_and_the_working_directory() {
        let root = std::env::temp_dir().join(format!("uorust-owned-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cwd = root.join("work/here");
        let checkout = root.join("checkout");
        let worktree = root.join("worktree");
        let sibling = root.join("work/sibling");
        for d in [&cwd, &checkout.join(".git"), &worktree, &sibling] {
            std::fs::create_dir_all(d).expect("temp tree");
        }
        std::fs::write(worktree.join(".git"), "gitdir: elsewhere\n").expect("worktree");
        assert_eq!(owned(&checkout, &cwd), Ok(false), "a .git directory");
        assert_eq!(owned(&worktree, &cwd), Ok(false), "a .git file");
        assert_eq!(owned(&cwd, &cwd), Ok(false), "the working directory");
        assert_eq!(owned(&root.join("work"), &cwd), Ok(false), "its parent");
        assert_eq!(owned(&sibling, &cwd), Ok(true), "a sibling");
        assert_eq!(
            owned(&root.join("absent"), &cwd),
            Ok(true),
            "an absent path"
        );
        assert!(matches!(
            owned(&sibling, &root.join("no-such-cwd")),
            Err(UorustError::Io { .. })
        ));
        std::fs::remove_dir_all(&root).expect("clean up");
    }
}
