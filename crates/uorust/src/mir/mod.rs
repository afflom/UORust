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

mod codec;
pub mod core;
pub mod coverage;
pub(crate) mod float;
pub(crate) mod flow;
pub(crate) mod layout;
pub(crate) mod link;
pub(crate) mod lower;
pub(crate) mod memory;
pub(crate) mod parse;
pub(crate) mod site;
// Generated: its layout is the generator's, which the spec test compares.
#[rustfmt::skip]
pub(crate) mod spec;
pub(crate) mod trips;

pub mod definitions;
#[cfg(test)]
mod generate;
pub(crate) mod identity;
pub(crate) mod int;
mod intrinsic;
pub(crate) mod ir;
mod prove;
mod residual;
mod source;
pub(crate) mod views;
// Generated: its layout is the generator's, which the std test compares.
#[cfg(test)]
#[rustfmt::skip]
mod std_vectors;
pub(crate) mod stdlib;
pub mod verdict;

pub use prove::{proof_batches, prove, prove_bounded, Summary, DEFAULT_PROOF_BATCH_SIZE};

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
    "-Zdump-mir=ElaborateBoxDerefs|runtime-optimized",
    "-Coverflow-checks=on",
    "-Cdebug-assertions=off",
    "--emit=metadata",
    "-Zalways-encode-mir",
    "-Zdump-mir-graphviz",
];

/// The file a trial projection lists its translations on trial in.
pub const CANDIDATES: &str = "mir-candidates.toml";

/// The suffix of the dump file of each body that the translator reads.
const DUMPED: &str = ".ElaborateBoxDerefs.before.mir";

/// Dump the MIR of every library and binary target of the workspace at
/// `root` under `out`, one directory per target, `<kind>-<name>`, built in
/// the configuration `config` (its features and target), which is recorded
/// beside them. Each library's dumps are also laid out as `uorust embed`
/// reads a crate's, `shards-<name>` ([`crate::shard`]). This compiles the
/// workspace, offline: its build scripts and macros run, as in any build of
/// it.
pub fn dump(
    root: &Path,
    out: &Path,
    config: &crate::shard::Configuration,
) -> Result<(), UorustError> {
    dump_inner(root, out, config, false)
}

fn dump_inner(
    root: &Path,
    out: &Path,
    config: &crate::shard::Configuration,
    capture_only: bool,
) -> Result<(), UorustError> {
    let tool = |what: &str, reason: String| UorustError::Tool(format!("{what}: {reason}"));
    let compiler = Command::new("rustc")
        .arg("-vV")
        .current_dir(root)
        .output()
        .map_err(|e| tool("rustc -vV", e.to_string()))?;
    if !compiler.status.success()
        || !String::from_utf8_lossy(&compiler.stdout)
            .lines()
            .any(|line| line == format!("release: {TOOLCHAIN}"))
    {
        return Err(tool(
            "compiler definitions",
            format!("requires pinned rustc {TOOLCHAIN}"),
        ));
    }
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
    std::fs::create_dir_all(out).map_err(|e| io(out, e))?;
    let workspace = root.canonicalize().map_err(|e| io(root, e))?;
    let triple = match &config.target {
        Some(t) => t.clone(),
        None => crate::shard::host(root, None)?,
    };
    let sysroot = crate::shard::sysroot(root, None)?;
    let recorded = out.join(crate::shard::CONFIGURATION);
    std::fs::write(&recorded, config.render(&triple)).map_err(|e| io(&recorded, e))?;
    let mut compiler_definitions = Vec::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        // The package's own artifacts are cleaned, so that each of its
        // targets compiles again and dumps; its dependencies stay built.
        if !capture_only {
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
        }
        // The configuration this package is built in: the features of
        // `config` it declares.
        let declared: std::collections::BTreeSet<String> = package
            .get("features")
            .and_then(serde_json::Value::as_object)
            .map(|f| f.keys().cloned().collect())
            .unwrap_or_default();
        let mut own_config = config.for_package(name, &declared);
        if capture_only
            && own_config.target.as_deref() == Some(crate::shard::host(root, None)?.as_str())
        {
            own_config.target = None;
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
            args.extend(own_config.cargo_args(false));
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
            definitions::collect(&dir, out, &mut compiler_definitions)?;
            residual::collect(&dir)?;
            // Only the bodies the translator reads are kept.
            for entry in std::fs::read_dir(&dir).map_err(|e| io(&dir, e))? {
                let path = entry.map_err(|e| io(&dir, e))?.path();
                let keep = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(DUMPED));
                if path.is_file() && !keep {
                    std::fs::remove_file(&path).map_err(|e| io(&path, e))?;
                }
            }
            if kind == "lib" && !capture_only {
                // The package's sources as the dump names them: relative to
                // the workspace root, which cargo runs rustc in.
                let own = package
                    .get("manifest_path")
                    .and_then(serde_json::Value::as_str)
                    .map(Path::new)
                    .and_then(Path::parent)
                    .and_then(|d| d.strip_prefix(&workspace).ok())
                    .map(|d| d.display().to_string())
                    .filter(|d| !d.is_empty())
                    .map_or_else(String::new, |d| format!("{d}/"));
                let shards = out.join(format!(
                    "{}{}",
                    crate::shard::SHARDS,
                    tname.replace('-', "_")
                ));
                if shards.exists() {
                    std::fs::remove_dir_all(&shards).map_err(|e| io(&shards, e))?;
                }
                std::fs::create_dir_all(&shards).map_err(|e| io(&shards, e))?;
                crate::shard::lay_out(&dir, &shards, &own, &sysroot)?;
                crate::shard::write_target(&shards, &triple, root, None)?;
                crate::shard::write_modules(&shards)?;
                let recorded = shards.join(crate::shard::CONFIGURATION);
                std::fs::write(&recorded, own_config.render(&triple))
                    .map_err(|e| io(&recorded, e))?;
            }
        }
    }
    definitions::write(root, out, compiler_definitions)?;
    record_layouts(root, out, &triple)
}

/// The file beside the dumps that records each declared target's data
/// layout, as rustc's target specification states it.
pub(crate) const LAYOUTS: &str = "layouts.toml";

/// Record, in [`LAYOUTS`] under `out`, the data layout of each target the
/// workspace's `rust-toolchain.toml` declares (`rustc --print
/// target-spec-json`, its `data-layout`). A workspace that declares none
/// records none, and its memory is then refused.
fn record_layouts(root: &Path, out: &Path, active: &str) -> Result<(), UorustError> {
    let tool = |what: &str, reason: String| UorustError::Tool(format!("{what}: {reason}"));
    let toolchain = root.join("rust-toolchain.toml");
    let mut targets: Vec<String> = match std::fs::read_to_string(&toolchain) {
        Ok(text) => text
            .parse::<toml::Table>()
            .ok()
            .and_then(|t| t.get("toolchain")?.get("targets")?.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect(),
        Err(_) => Vec::new(),
    };
    if !targets.iter().any(|target| target == active) {
        targets.push(active.to_string());
    }
    let mut text =
        String::from("# Each declared target's data layout, from rustc's target specification.\n");
    for target in targets {
        let run = Command::new("rustc")
            .args([
                "-Zunstable-options",
                "--print",
                "target-spec-json",
                "--target",
            ])
            .arg(&target)
            .current_dir(root)
            .env("RUSTC_BOOTSTRAP", "1")
            .output()
            .map_err(|e| tool("rustc --print target-spec-json", e.to_string()))?;
        if !run.status.success() {
            return Err(tool(
                &format!("rustc --print target-spec-json --target {target}"),
                String::from_utf8_lossy(&run.stderr).into_owned(),
            ));
        }
        let spec: serde_json::Value = serde_json::from_slice(&run.stdout)
            .map_err(|e| tool("rustc --print target-spec-json", e.to_string()))?;
        let Some(layout) = spec.get("data-layout").and_then(serde_json::Value::as_str) else {
            return Err(tool(
                "rustc --print target-spec-json",
                format!("no data-layout for {target}"),
            ));
        };
        text.push_str(&format!(
            "\n[[target]]\ntriple = {}\ndata_layout = {}\n",
            toml::Value::String(target.clone()),
            toml::Value::String(layout.to_string())
        ));
    }
    let path = out.join(LAYOUTS);
    std::fs::write(&path, text).map_err(|e| io(&path, e))
}

/// The dumped bodies of a workspace, by crate identifier, each keyed by its
/// path with the impl it sits in named by its type: `ast::Path::ident`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Dumps {
    /// The target of this compilation, distinct from installed toolchains.
    pub(crate) target: Option<String>,
    pub(crate) crates: BTreeMap<String, BTreeMap<String, IrBody>>,
    /// Compiler source identities preserve overloaded implementation bodies.
    /// Repeated source identities are removed rather than guessed.
    pub(crate) definitions: BTreeMap<crate::embedding::Key, IrBody>,
    pub(crate) residual_bodies: BTreeMap<(String, String), (IrBody, String, String)>,
    pub(crate) free_body_hashes: BTreeMap<(String, String), String>,
    pub(crate) compiler_identities: BTreeMap<crate::embedding::Key, definitions::Definition>,
    /// The constants' bodies (promoted constants and constant items), by
    /// crate and the path MIR prints for them where they are used.
    pub(crate) consts: BTreeMap<String, BTreeMap<String, IrBody>>,
    /// The closures' bodies, by crate and the closure's type as MIR prints
    /// it, `{closure@file:l:c: l:c}`.
    pub(crate) closures: BTreeMap<String, BTreeMap<String, IrBody>>,
    /// The declared targets' data layouts, as [`LAYOUTS`] records them.
    pub(crate) layouts: Vec<layout::DataLayout>,
}

impl Dumps {
    pub(crate) fn source_width(&self) -> Option<u32> {
        let target = self.target.as_ref()?;
        let mut matched = self.layouts.iter().filter(|l| &l.target == target);
        let width = matched.next()?.pointer.0;
        (matched.next().is_none() && [16, 32, 64].contains(&width)).then_some(width)
    }
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
    let mut ambiguous_definitions = std::collections::BTreeSet::new();
    if !dir.is_dir() {
        return Ok(dumps);
    }
    let configuration = dir.join(crate::shard::CONFIGURATION);
    if configuration.is_file() {
        let text = std::fs::read_to_string(&configuration).map_err(|e| io(&configuration, e))?;
        let doc: toml::Table =
            text.parse()
                .map_err(|e: toml::de::Error| UorustError::Malformed {
                    path: configuration.clone(),
                    reason: e.to_string(),
                })?;
        dumps.target = doc
            .get("target")
            .and_then(toml::Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
    }
    let recorded = dir.join(LAYOUTS);
    if recorded.is_file() {
        let text = std::fs::read_to_string(&recorded).map_err(|e| io(&recorded, e))?;
        let bad = |reason: String| UorustError::Malformed {
            path: recorded.clone(),
            reason,
        };
        let doc: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| bad(e.to_string()))?;
        for t in doc
            .get("target")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(t) = t.as_table() else {
                return Err(bad("a target that is not a table".into()));
            };
            dumps
                .layouts
                .extend(layout::from_target_toml(t).map_err(|r| bad(layout::reason(&r)))?);
        }
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
        let pairs = residual::index(&target)?;
        let entry = dumps.crates.entry(krate.clone()).or_default();
        for file in files {
            let text = std::fs::read_to_string(&file).map_err(|e| io(&file, e))?;
            for mut body in parse::bodies(&text) {
                // rustc's file stem includes the complete path in the target. A
                // shortened display name or an impl span cannot acquire ownership.
                let stem = format!("{krate}.{}.", body.path.replace("::", "-"));
                if body.path.contains("::")
                    && file
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(&stem))
                    && text.lines().any(|line| {
                        line.strip_prefix("fn ")
                            .and_then(|f| f.split_once('('))
                            .is_some_and(|(path, _)| path == body.path)
                    })
                {
                    dumps.free_body_hashes.insert(
                        (krate.clone(), body.path.clone()),
                        crate::project::sha(text.as_bytes()),
                    );
                }
                let definition_key = crate::embedding::body_key(&krate, &body.path);
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
                let alternative = if pairs.is_empty() {
                    None
                } else {
                    residual::read(&target, &pairs, &text, &body)?
                };
                let resolved = resolve_impls(root, root, &body.path);
                if let Some(path) = &resolved {
                    body.path.clone_from(path);
                }
                if let Some(key) = definition_key.filter(|key| key.impl_at.is_some()) {
                    if dumps
                        .definitions
                        .insert(key.clone(), body.clone())
                        .is_some()
                    {
                        ambiguous_definitions.insert(key);
                    }
                }
                let Some(path) = resolved else {
                    continue;
                };
                if let Some(mut alternate) = alternative {
                    alternate.path.clone_from(&path);
                    let primary = crate::project::sha(text.as_bytes());
                    let runtime = pairs[&primary].clone();
                    dumps
                        .residual_bodies
                        .insert((krate.clone(), path.clone()), (alternate, primary, runtime));
                }
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
    for key in ambiguous_definitions {
        dumps.definitions.remove(&key);
    }
    if !dir.with_extension("compiler-definitions.toml").is_file() {
        dumps.free_body_hashes.clear();
    }
    dumps.compiler_identities = definitions::load(root, dir)?;
    Ok(dumps)
}

/// `m::<impl at src/x.rs:3:1: 3:9>::f` with the impl named by the type it
/// implements, `m::X::f`; `None` for a closure, a promoted constant or an
/// impl whose source does not read.
pub(crate) fn resolve_impls(root: &Path, own_root: &Path, path: &str) -> Option<String> {
    if path.contains("{closure") || path.contains("promoted[") || path.contains("{constant") {
        return None;
    }
    let mut out = String::new();
    let mut rest = path;
    while let Some(at) = rest.find("<impl at ") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "<impl at ".len()..];
        let end = after.find('>')?;
        let (file, range) = after[..end].split_once(':')?;
        let source = if Path::new(file).is_absolute() || file.starts_with("library/") {
            root.join(file)
        } else {
            own_root.join(file)
        };
        let span = format!("{}:{range}", source.display());
        out.push_str(&impl_type(Path::new(""), &span)?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The type an impl at `file:l1:c1: l2:c2` implements, read from its source.
pub(super) fn impl_type(root: &Path, span: &str) -> Option<String> {
    let src = impl_header(root, span)?;
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

/// The text of the impl header at `file:l1:c1: l2:c2`, read from its
/// source.
pub(super) fn impl_header(root: &Path, span: &str) -> Option<String> {
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
    Some(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_source_width_requires_one_matching_layout_ue_94() {
        let a = layout::DataLayout::parse("captured", "e-p:64:64").unwrap();
        let other = layout::DataLayout::parse("installed", "e-p:32:32").unwrap();
        let mut dumps = Dumps {
            target: Some("captured".into()),
            layouts: vec![a.clone(), other],
            ..Dumps::default()
        };
        assert_eq!(dumps.source_width(), Some(64));
        dumps.target = None;
        assert_eq!(dumps.source_width(), None);
        dumps.target = Some("unknown".into());
        assert_eq!(dumps.source_width(), None);
        dumps.target = Some("captured".into());
        dumps.layouts.push(a);
        assert_eq!(dumps.source_width(), None);
    }

    #[test]
    fn generated_compiler_definitions_survive_source_header_lookup_ue_85() {
        let dir = std::env::temp_dir().join(format!(
            "uorust-generated-definition-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#[derive(Default)]\npub struct Mark { pub flag: bool }\n",
        )
        .unwrap();
        let target = dir.join("dumps/lib-generated");
        std::fs::create_dir_all(&target).unwrap();
        let body = "fn <impl at src/lib.rs:1:10: 1:17>::default() -> Mark {\n    let mut _0: Mark;\n    bb0: {\n        _0 = Mark { flag: const false };\n        return;\n    }\n}\n";
        std::fs::write(target.join(format!("derived{DUMPED}")), body).unwrap();
        let loaded = load(&dir, &dir.join("dumps")).unwrap();
        assert_eq!(
            loaded.definitions.len(),
            1,
            "compiler-generated body was discarded before identity import"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repeated_compiler_source_identities_remain_ambiguous_ue_78() {
        let dir = std::env::temp_dir().join(format!(
            "uorust-ambiguous-definition-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub struct Mark;\nimpl Mark { fn from()->u8 { 0 } }\n",
        )
        .unwrap();
        let target = dir.join("dumps/lib-definitions");
        std::fs::create_dir_all(&target).unwrap();
        let body = "fn <impl at src/lib.rs:2:1: 2:10>::from() -> u8 {\n    let mut _0: u8;\n\n    bb0: {\n        _0 = const 0_u8;\n        return;\n    }\n}\n";
        std::fs::write(target.join(format!("first{DUMPED}")), body).unwrap();
        let unique = load(&dir, &dir.join("dumps")).unwrap();
        assert_eq!(unique.definitions.len(), 1);
        assert!(unique.crates["definitions"].contains_key("Mark::from"));
        std::fs::write(target.join(format!("duplicate{DUMPED}")), body).unwrap();
        let ambiguous = load(&dir, &dir.join("dumps")).unwrap();
        assert!(ambiguous.definitions.is_empty());
        assert!(!ambiguous.crates["definitions"].contains_key("Mark::from"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn metadata_collection_preserves_pre_llvm_bodies_ue_32() {
        let dir = std::env::temp_dir().join(format!("uorust-metadata-mir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("lib.rs"),
            "pub fn count(mut n:u32)->u32 { let mut x=0; while n>0 { x+=1;n-=1; } x }\n\
             fn unused(n:u32)->u32 { n.wrapping_add(1) }\n\
             pub fn generic<T:Copy>(x:T)->T { x }\n\
             pub fn closure(x:u32)->u32 { (|| x.wrapping_add(1))() }\n\
             pub struct Item(pub u8); impl Item { pub fn get(&self)->u8 {self.0} }\n",
        )
        .unwrap();
        let common: Vec<&str> = FLAGS
            .iter()
            .copied()
            .filter(|f| !f.starts_with("--emit=") && *f != "-Zalways-encode-mir")
            .collect();
        let mut linked = common.clone();
        linked.push("--emit=link");
        let omitted: Vec<&str> = FLAGS
            .iter()
            .copied()
            .filter(|f| *f != "-Zalways-encode-mir")
            .collect();
        let mut inventories = BTreeMap::new();
        for (name, flags) in [
            ("linked", linked),
            ("production", FLAGS.to_vec()),
            ("omitted", omitted),
        ] {
            let output = dir.join(name);
            std::fs::create_dir_all(&output).unwrap();
            let result = Command::new("rustc")
                .args([
                    "--crate-name",
                    "probe",
                    "--crate-type",
                    "lib",
                    "--edition",
                    "2021",
                    "lib.rs",
                ])
                .args(flags)
                .arg("--out-dir")
                .arg(&output)
                .arg(format!("-Zdump-mir-dir={}", output.display()))
                .env("RUSTC_BOOTSTRAP", "1")
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let files: BTreeMap<_, _> = std::fs::read_dir(&output)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "mir"))
                .map(|p| (p.file_name().unwrap().to_owned(), std::fs::read(p).unwrap()))
                .collect();
            if name == "production" {
                assert!(output.join("libprobe.rmeta").is_file());
                assert!(
                    !output.join("libprobe.rlib").exists(),
                    "MIR collection emitted a native archive"
                );
            }
            inventories.insert(name, files);
        }
        assert!(!inventories["linked"].is_empty());
        assert_eq!(inventories["linked"], inventories["production"]);
        assert_ne!(
            inventories["linked"], inventories["omitted"],
            "Missing encoding must be detected"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
            resolve_impls(&dir, &dir, "a::<impl at src/a.rs:2:1: 2:7>::get").as_deref(),
            Some("a::S::get")
        );
        assert_eq!(
            resolve_impls(&dir, &dir, "a::<impl at src/a.rs:3:1: 3:41>::fmt").as_deref(),
            Some("a::Wrap::fmt")
        );
        assert_eq!(resolve_impls(&dir, &dir, "a::f::{closure#0}"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
