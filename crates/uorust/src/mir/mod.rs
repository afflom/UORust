//! The MIR path of the lift: every function body as rustc's mid-level IR,
//! translated by one lowering per IR kind instead of one per surface form.
//!
//! MIR covers all of Rust with a fixed set of statement, rvalue, operand,
//! place and terminator kinds. [`spec`] lists those kinds as rustc's own
//! definition at the pinned toolchain declares them (generated, see
//! `generate`), and [`lower`] matches each exhaustively: a kind is lowered, or
//! it becomes a residue row that names it. A new Rust form needs no new
//! lowering, only the kinds it compiles to.
//!
//! The bodies come from the pinned toolchain itself: [`dump`] runs `cargo
//! rustc` on every library and binary target of the workspace with
//! `-Zdump-mir=ElaborateBoxDerefs` (MIR after drop elaboration, with a `Box`
//! still a place), the overflow checks of the debug profile, and untrimmed
//! paths. The flags are the stable compiler's, opened with
//! `RUSTC_BOOTSTRAP=1`: no other toolchain, driver or download is involved,
//! and the MIR is exactly the one the shipped build compiles.

pub mod core;
pub mod coverage;
pub(crate) mod float;
mod link;
pub(crate) mod lower;
mod parse;
// Generated: its layout is the generator's, which the spec test compares.
#[rustfmt::skip]
pub(crate) mod spec;

#[cfg(test)]
mod generate;
mod int;
mod intrinsic;
pub(crate) mod ir;
mod prove;
// Generated: its layout is the generator's, which the std test compares.
#[cfg(test)]
#[rustfmt::skip]
mod std_vectors;
pub(crate) mod stdlib;
pub mod verdict;

pub use prove::{prove, Summary};

/// The toolchain the MIR path is pinned to, for the binary's scopes.
pub const TOOLCHAIN: &str = spec::TOOLCHAIN;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) use ir::IrBody;

use crate::error::io;
use crate::UorustError;

/// How a projection runs the MIR path.
#[derive(Clone, Debug, Default)]
pub struct MirOptions {
    /// The directory [`dump`] wrote the workspace's MIR into.
    pub dumps: PathBuf,
    /// The verdict register: a translation is admitted on a proved row.
    pub verdicts: verdict::Verdicts,
    /// Translations admitted without a verdict, for the prover's trial run.
    pub trial: Trial,
    /// The axioms the prover learned for translations on trial, by κ and
    /// declaration.
    pub learned: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

/// Which translations without a verdict a projection admits on trial.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Trial {
    /// None: only proved translations (every projection but the prover's).
    #[default]
    Off,
    /// Every translation that has no verdict of its digest.
    All,
    /// Those of these κ.
    Only(BTreeSet<String>),
}

impl Trial {
    /// Whether a translation of `kappa` without a verdict is admitted.
    pub(crate) fn admits(&self, kappa: &str) -> bool {
        match self {
            Self::Off => false,
            Self::All => true,
            Self::Only(set) => set.contains(kappa),
        }
    }
}

/// The flags `cargo rustc` passes the compiler for a dump. `uorust_lift`
/// leaves out what the lift must not read: a hand-written function's
/// dispatch to its certified realization (`crate::certified::certified!`).
pub const FLAGS: &[&str] = &[
    "--cfg=uorust_lift",
    "-Ztrim-diagnostic-paths=no",
    "-Zdump-mir=ElaborateBoxDerefs",
    "-Coverflow-checks=on",
    "-Cdebug-assertions=off",
];

/// The file a trial projection lists its translations on trial in.
pub const CANDIDATES: &str = "mir-candidates.toml";

/// The suffix of the dump file of each body that the translator reads.
const DUMPED: &str = ".ElaborateBoxDerefs.before.mir";

/// Dump the MIR of every library and binary target of the workspace at
/// `root` under `out`, one directory per target, `<kind>-<name>`. This
/// compiles the workspace, offline: its build scripts and macros run, as in
/// any build of it.
pub fn dump(root: &Path, out: &Path) -> Result<(), UorustError> {
    let tool = |what: &str, reason: String| UorustError::Tool(format!("{what}: {reason}"));
    let meta = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--locked",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| tool("cargo metadata", e.to_string()))?;
    if !meta.status.success() {
        return Err(tool(
            "cargo metadata",
            String::from_utf8_lossy(&meta.stderr).into_owned(),
        ));
    }
    let doc: serde_json::Value =
        serde_json::from_slice(&meta.stdout).map_err(|e| tool("cargo metadata", e.to_string()))?;
    let target_dir =
        std::env::var_os("UORUST_MIR_TARGET_DIR").map_or_else(|| out.join("build"), PathBuf::from);
    let packages = doc
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        // The package's own artifacts are cleaned, so that each of its
        // targets compiles again and dumps; its dependencies stay built.
        let clean = Command::new("cargo")
            .args(["clean", "--locked", "--offline", "-p", name])
            .current_dir(root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()
            .map_err(|e| tool("cargo clean", e.to_string()))?;
        if !clean.status.success() {
            return Err(tool(
                &format!("cargo clean -p {name}"),
                String::from_utf8_lossy(&clean.stderr).into_owned(),
            ));
        }
        let targets = package
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for target in targets {
            let kinds: Vec<&str> = target
                .get("kind")
                .and_then(serde_json::Value::as_array)
                .map(|k| k.iter().filter_map(serde_json::Value::as_str).collect())
                .unwrap_or_default();
            let tname = target
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let (flag, kind) = if kinds.iter().any(|k| matches!(*k, "lib" | "rlib")) {
                (vec!["--lib".to_string()], "lib")
            } else if kinds.contains(&"bin") {
                (vec!["--bin".to_string(), tname.to_string()], "bin")
            } else {
                continue;
            };
            let dir = out.join(format!("{kind}-{tname}"));
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
            }
            std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
            let dir = dir.canonicalize().map_err(|e| io(&dir, e))?;
            let mut args = vec![
                "rustc".to_string(),
                "--locked".to_string(),
                "--offline".to_string(),
                "-p".to_string(),
                name.to_string(),
            ];
            args.extend(flag);
            args.push("--".to_string());
            args.extend(FLAGS.iter().map(|f| (*f).to_string()));
            args.push(format!("-Zdump-mir-dir={}", dir.display()));
            let run = Command::new("cargo")
                .args(&args)
                .current_dir(root)
                .env("RUSTC_BOOTSTRAP", "1")
                .env("CARGO_TARGET_DIR", &target_dir)
                .env("CARGO_INCREMENTAL", "0")
                .output()
                .map_err(|e| tool("cargo rustc", e.to_string()))?;
            if !run.status.success() {
                return Err(tool(
                    &format!("cargo rustc -p {name} ({kind} {tname})"),
                    String::from_utf8_lossy(&run.stderr).into_owned(),
                ));
            }
            // Only the bodies the translator reads are kept.
            for entry in std::fs::read_dir(&dir).map_err(|e| io(&dir, e))? {
                let path = entry.map_err(|e| io(&dir, e))?.path();
                let keep = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(DUMPED));
                if !keep {
                    std::fs::remove_file(&path).map_err(|e| io(&path, e))?;
                }
            }
        }
    }
    Ok(())
}

/// The dumped bodies of a workspace, by crate identifier, each keyed by its
/// path with the impl it sits in named by its type: `ast::Path::ident`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Dumps {
    pub(crate) crates: BTreeMap<String, BTreeMap<String, IrBody>>,
    /// The constants' bodies (promoted constants and constant items), by
    /// crate and the path MIR prints for them where they are used.
    pub(crate) consts: BTreeMap<String, BTreeMap<String, IrBody>>,
    /// The closures' bodies, by crate and the closure's type as MIR prints
    /// it, `{closure@file:l:c: l:c}`.
    pub(crate) closures: BTreeMap<String, BTreeMap<String, IrBody>>,
}

/// The type of the closure whose body this is: its environment `_1`'s type,
/// by value or behind a reference.
pub(crate) fn closure_type(body: &IrBody) -> Option<String> {
    if !body.path.contains("{closure#") {
        return None;
    }
    let env = body.locals.get(1)?;
    let at = env.find("{closure@")?;
    let end = at + env[at..].find('}')?;
    Some(env[at..=end].to_string())
}

/// Read the dumps under `dir`, resolving each `<impl at file:l:c: l:c>` to
/// the type the source at that span implements.
pub(crate) fn load(root: &Path, dir: &Path) -> Result<Dumps, UorustError> {
    let mut dumps = Dumps::default();
    let mut ambiguous: Vec<(String, String)> = Vec::new();
    if !dir.is_dir() {
        return Ok(dumps);
    }
    let mut targets: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("lib-") || n.starts_with("bin-"))
        })
        .collect();
    targets.sort();
    for target in targets {
        let krate = target
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n[4..].replace('-', "_"))
            .unwrap_or_default();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&target)
            .map_err(|e| io(&target, e))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(DUMPED))
            })
            .collect();
        files.sort();
        let entry = dumps.crates.entry(krate.clone()).or_default();
        for file in files {
            let text = std::fs::read_to_string(&file).map_err(|e| io(&file, e))?;
            for mut body in parse::bodies(&text) {
                if body.arity == 0 && (body.path.contains("promoted[") || !body.path.contains('{'))
                {
                    dumps
                        .consts
                        .entry(krate.clone())
                        .or_default()
                        .insert(body.path.clone(), body.clone());
                }
                if let Some(t) = closure_type(&body) {
                    dumps
                        .closures
                        .entry(krate.clone())
                        .or_default()
                        .insert(t, body.clone());
                }
                let Some(path) = resolve_impls(root, &body.path) else {
                    continue;
                };
                body.path.clone_from(&path);
                if entry.insert(path.clone(), body).is_some() {
                    ambiguous.push((krate.clone(), path));
                }
            }
        }
    }
    // A path two targets of one crate name both define is no one's.
    for (k, p) in ambiguous {
        if let Some(e) = dumps.crates.get_mut(&k) {
            e.remove(&p);
        }
    }
    Ok(dumps)
}

/// `m::<impl at src/x.rs:3:1: 3:9>::f` with the impl named by the type it
/// implements, `m::X::f`; `None` for a closure, a promoted constant or an
/// impl whose source does not read.
fn resolve_impls(root: &Path, path: &str) -> Option<String> {
    if path.contains("{closure") || path.contains("promoted[") || path.contains("{constant") {
        return None;
    }
    let mut out = String::new();
    let mut rest = path;
    while let Some(at) = rest.find("<impl at ") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "<impl at ".len()..];
        let end = after.find('>')?;
        out.push_str(&impl_type(root, &after[..end])?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The type an impl at `file:l1:c1: l2:c2` implements, read from its source.
pub(super) fn impl_type(root: &Path, span: &str) -> Option<String> {
    let (file, range) = span.split_once(':')?;
    let (from, to) = range.split_once(": ")?;
    let (l1, c1) = from.split_once(':')?;
    let (l2, c2) = to.split_once(':')?;
    let (l1, c1, l2, c2): (usize, usize, usize, usize) = (
        l1.parse().ok()?,
        c1.parse().ok()?,
        l2.parse().ok()?,
        c2.parse().ok()?,
    );
    let text = std::fs::read_to_string(root.join(file)).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let mut src = String::new();
    for l in l1..=l2 {
        let line = lines.get(l - 1)?;
        let chars: Vec<char> = line.chars().collect();
        let from = if l == l1 { c1 - 1 } else { 0 };
        let to = if l == l2 {
            (c2 - 1).min(chars.len())
        } else {
            chars.len()
        };
        src.extend(chars.get(from..to)?);
        src.push(' ');
    }
    let head = src.trim().strip_prefix("impl")?;
    // `impl<'a, T: X>`: skip the impl's own generics.
    let head = if head.starts_with('<') {
        let mut depth = 0i64;
        let mut end = 0;
        for (i, c) in head.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        &head[end..]
    } else {
        head
    };
    let ty = head.rsplit(" for ").next()?.trim();
    let ty = ty.split(" where").next()?.trim();
    let ty = ty.split('<').next()?.trim();
    let last = ty.rsplit("::").next()?.trim();
    (!last.is_empty() && last.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .then(|| last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impls_are_named_by_the_type_their_source_implements() {
        let dir = std::env::temp_dir().join(format!("uorust-mir-impl-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/a.rs"),
            "struct S;\nimpl S {}\nimpl<'a> std::fmt::Display for Wrap<'a> {}\n",
        )
        .unwrap();
        assert_eq!(
            resolve_impls(&dir, "a::<impl at src/a.rs:2:1: 2:7>::get").as_deref(),
            Some("a::S::get")
        );
        assert_eq!(
            resolve_impls(&dir, "a::<impl at src/a.rs:3:1: 3:41>::fmt").as_deref(),
            Some("a::Wrap::fmt")
        );
        assert_eq!(resolve_impls(&dir, "a::f::{closure#0}"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
