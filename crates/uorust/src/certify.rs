//! The certified translator: the lifted translator, exported through
//! `lexlean production` (SPEC §24.7) as certified Rust, and the ratchet that
//! lets the hand-written translator only shrink.
//!
//! The translator is the code of [`SCOPE`]: the MIR path (`mir`), the
//! surface lift (`lift`) and the LexLean writer (`lex`). Each of its
//! functions the self register records as lifted has a LexLean definition in
//! the self projection's `Behavior` module, and every such definition is a
//! production root: [`production`] writes the projection again with each one
//! marked executable and the request that names them, so that `lexlean
//! production build` kernel-checks a Rust package per root. [`sync`] takes
//! the published bundle into the repository (`crates/certified`): each
//! package's source byte for byte, as the bundle's result records its
//! SHA-256, and the manifest that keys every certified root by the κ of the
//! Rust function it certifies.
//!
//! A function is certified when the manifest has a root of its κ: a changed
//! function is a new question and is uncertified until the bundle is rebuilt
//! from the projection that lifts it. [`ratchet`] refuses a register and a
//! manifest under which more translator functions are uncertified than under
//! the parent's, naming each new one with its cause.
//!
//! None of it is the translator's alone. A [`Profile`] says what an export
//! is of: the scope, the entry, the package prefix, the effects and the
//! target. The constants below are the translator's own profile, the default
//! of every step, so its gate reads as before; any other workspace states
//! its own (`--workspace`, the table of its Cargo manifest, the command's
//! options) and has every lifted function in its scope exported, taken into
//! a directory it names and held by the same ratchet. Its agreement is
//! [`cases`]: the closed assertions of its own tests, each a production
//! root of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::io;
use crate::mir::ir::{IrAggregate, IrConstant, IrOperand, IrRvalue, IrStatement, IrTerminator};
use crate::mir::{Dumps, IrBody};
use crate::register::{Register, Status};
use crate::UorustError;

pub mod cases;
mod profile;

pub use profile::{Kind, Options, Profile, PROFILE};

/// The modules of the translator, by their crate paths.
pub const SCOPE: &[&str] = &["uorust::mir::", "uorust::lift::", "uorust::lex::"];

/// The translate entry: the function that translates the bodies of one
/// module of a library, from their MIR text to their LexLean and κ rows.
pub const ENTRY: &str = "uorust::mir::core::core_shard";

/// The schema of the manifest [`sync`] writes.
pub const SCHEMA: &str = "uorust/certified/1";

/// The prefix of every certified package's name.
pub const PACKAGE: &str = "uorust_certified_";

/// The version every certified package is requested at, and the crate that
/// holds them is written at.
const PACKAGE_VERSION: &str = "0.1.0";

/// The effects every root admits: the whole closed registry of SPEC §17.13.
/// The translator builds strings and lists (allocation), computes on
/// naturals of unbounded width (overflow, an explicit failure at the
/// target's 64 bits) and walks its inputs by structural recursion
/// (recursion); the compiler states which ones each package realizes.
pub const EFFECTS: &[&str] = &["allocation", "overflow", "recursion"];

/// The target every root is exported to: the translator runs where `std`
/// does.
pub const TARGET: &str = "rust-std";

/// Whether a function is translator code.
pub fn in_scope(function: &str) -> bool {
    SCOPE.iter().any(|s| function.starts_with(s))
}

/// A production root: a lifted translator function and its definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// The Rust function's crate path.
    pub function: String,
    /// Its κ in the register.
    pub kappa: String,
    /// The definition in the projection's `Behavior` module, unqualified.
    pub definition: String,
    /// The prefix of its package's name, the profile's.
    pub prefix: String,
}

/// The most characters a crate's name has, as the compiler states the rule
/// (`LLC0201`: a crate name is `[a-z][a-z0-9_]{0,63}`).
const CRATE_NAME: usize = 64;

/// The hexadecimal digits of the SHA-256 of a longer name that end its
/// shortened form: a quarter of the digest, 64 bits, which no two roots of a
/// workspace share by chance; two that did would be refused by name
/// ([`Profile::export_roots`]).
const NAME_DIGEST: usize = CRATE_NAME / 4;

impl Root {
    /// The certified package's name: the profile's prefix ([`PACKAGE`], for
    /// the translator) and the definition in snake case. A name longer than
    /// a crate's may be ([`CRATE_NAME`]) keeps its start and ends with the
    /// first [`NAME_DIGEST`] digits of its own SHA-256, so it is still the
    /// root's alone and the same at every export.
    pub fn package(&self) -> String {
        let mut out = self.prefix.clone();
        for c in self.definition.chars() {
            if c.is_ascii_uppercase() {
                out.push('_');
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
        if out.chars().count() <= CRATE_NAME {
            return out;
        }
        let digest = crate::project::sha(out.as_bytes());
        let kept: String = out.chars().take(CRATE_NAME - NAME_DIGEST - 1).collect();
        format!("{kept}_{}", &digest[..NAME_DIGEST])
    }
}

/// The production roots of a register: every lifted translator function, by
/// definition.
pub fn roots(register: &Register) -> Vec<Root> {
    Profile::default().roots(register)
}

impl Profile {
    /// The function roots of a register under this profile: every lifted
    /// function in its scope, by definition.
    pub fn roots(&self, register: &Register) -> Vec<Root> {
        let mut out: Vec<Root> = register
            .rows
            .iter()
            .filter(|(_, row)| self.in_scope(&row.function))
            .filter_map(|(kappa, row)| match &row.status {
                Status::Lifted(definition) => Some(Root {
                    function: row.function.clone(),
                    kappa: kappa.clone(),
                    definition: definition
                        .rsplit("::")
                        .next()
                        .unwrap_or(definition)
                        .to_string(),
                    prefix: self.package.clone(),
                }),
                _ => None,
            })
            .collect();
        out.sort_by(|a, b| a.definition.cmp(&b.definition));
        out
    }

    /// The roots of an export under this profile: its functions, or, of a
    /// cases export, the closed assertions the projection at `projection`
    /// states over them.
    pub fn export_roots(
        &self,
        projection: &Path,
        register: &Register,
    ) -> Result<Vec<Root>, UorustError> {
        let roots: Vec<Root> = match self.kind {
            Kind::Functions => self.roots(register),
            Kind::Cases => cases::cases(self, &modules(projection)?, register)
                .into_iter()
                .map(|c| c.root)
                .collect(),
        };
        // A package is one root's: two definitions that come to one name
        // would have one certify the other's function.
        let mut packages: BTreeMap<String, &str> = BTreeMap::new();
        for root in &roots {
            if let Some(other) = packages.insert(root.package(), &root.definition) {
                return Err(UorustError::Tool(format!(
                    "the definitions {other} and {} come to one package name, {}",
                    root.definition,
                    root.package()
                )));
            }
        }
        Ok(roots)
    }
}

/// The modules of a projection a root's closure lies in, by name, with
/// their texts: `Behavior`, and the `Prelude` it imports.
pub(crate) fn modules(projection: &Path) -> Result<BTreeMap<String, String>, UorustError> {
    let mut modules = BTreeMap::new();
    for module in [BEHAVIOR, "Prelude"] {
        let path = projection.join("src").join(format!("{module}.lex.tex"));
        let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        modules.insert(module.to_string(), text);
    }
    Ok(modules)
}

/// Write the production project: the projection at `projection` with every
/// root's definition marked as a production root, and `request.json`, the
/// `lexlean/production-request/1` document naming one package per root at
/// the `rust-std` descriptor of `targets` (the output of `lexlean production
/// targets`), restricted to the definitions of `batch` when it is given.
/// Returns the roots the request names, by definition.
pub fn production(
    profile: &Profile,
    projection: &Path,
    out: &Path,
    register: &Register,
    targets: &str,
    compiler: &BTreeMap<String, String>,
    batch: Option<&BTreeSet<String>>,
) -> Result<Vec<String>, UorustError> {
    let bad = |path: &Path, reason: String| UorustError::Malformed {
        path: path.to_path_buf(),
        reason,
    };
    let digest = profile.descriptor(targets)?;
    let prefix = module_prefix(projection)?;
    copy_tree(projection, out)?;
    let roots = profile.export_roots(projection, register)?;
    let names: BTreeSet<&str> = roots.iter().map(|r| r.definition.as_str()).collect();
    let modules = modules(out)?;
    let exported: BTreeSet<&str> = names
        .iter()
        .copied()
        .filter(|n| !compiler.contains_key(*n))
        .collect();
    let (mut marked, found, mut refused) = profile.mark(&modules, &exported);
    // The compiler generates a package for every root the source marks, so
    // a batch marks only its own roots, and their closures executable.
    if let Some(batch) = batch {
        let own: BTreeSet<&str> = exported
            .iter()
            .copied()
            .filter(|n| batch.contains(*n))
            .collect();
        marked = profile.mark(&modules, &own).0;
    }
    refused.extend(compiler.iter().map(|(k, v)| (k.clone(), v.clone())));
    if let Some(missing) = names
        .iter()
        .find(|n| !found.contains(**n) && !refused.contains_key(**n))
    {
        return Err(bad(
            &out.join("src").join("Behavior.lex.tex"),
            format!("the register's definition {missing} is not in the projection"),
        ));
    }
    for (module, text) in marked {
        let path = out.join("src").join(format!("{module}.lex.tex"));
        std::fs::write(&path, text).map_err(|e| io(&path, e))?;
    }
    let refusals: String = roots
        .iter()
        .filter_map(|r| refused.get(&r.definition).map(|c| (r, c)))
        .map(|(r, c)| {
            format!(
                "[[refused]]\nkappa = \"{}\"\ncause = {}\n\n",
                r.kappa,
                toml_quoted(c)
            )
        })
        .collect();
    let path = out.join(REFUSALS);
    std::fs::write(&path, refusals).map_err(|e| io(&path, e))?;
    let named: Vec<String> = roots
        .iter()
        .filter(|r| !refused.contains_key(&r.definition))
        .filter(|r| batch.is_none_or(|b| b.contains(&r.definition)))
        .map(|r| r.definition.clone())
        .collect();
    let mut entries: Vec<(String, serde_json::Value)> = roots
        .iter()
        .filter(|r| named.contains(&r.definition))
        .map(|r| {
            let root = format!("{prefix}.Behavior.{}", r.definition);
            let entry = serde_json::json!({
                "package": {"name": r.package(), "version": PACKAGE_VERSION},
                "root": root,
                "targets": [{"digest": digest, "id": profile.target}],
            });
            (root, entry)
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let request = serde_json::json!({
        "roots": entries.into_iter().map(|(_, e)| e).collect::<Vec<_>>(),
        "spec": "lexlean/production-request/1",
    });
    let path = out.join("request.json");
    let body = serde_json::to_string_pretty(&request).unwrap_or_default() + "\n";
    std::fs::write(&path, body).map_err(|e| io(&path, e))?;
    Ok(named)
}

/// The limit of a `lexlean` project on the bytes one build generates over all
/// its roots (`[limits] max_total_source_bytes`): a build that names it is
/// over budget as a whole, not refused for any one root.
const TOTAL_BUDGET: &str = "max_total_source_bytes";

/// The build seconds per root an export without a measured figure plans
/// with: run 43653074598 built 80-root batches in 560 to 800 seconds.
const MEASURED_SECONDS_PER_ROOT: u64 = 10;

/// The builds of its batch a shard is sized to fit before the deadline: its
/// first and three more after roots the kernel refuses late.
const BUILDS_PER_SHARD: u64 = 4;

/// The most roots a shard holds, whatever the timing: the kernel refuses
/// roots late, one per build, so a shard of this many on its two workers
/// rebuilds batches of at most half of it, and its deadline holds as many
/// such rebuilds as it has roots.
const MAX_ROOTS_PER_SHARD: usize = 8;

/// The most shards an export plans: a matrix GitHub runs at once.
const MAX_SHARDS: usize = 64;

/// The plan of a sharded export: how many shards build the roots `prior`
/// did not refuse, each sized so that [`BUILDS_PER_SHARD`] builds of it fit
/// in `budget` seconds at the prior export's seconds per root, and the roots
/// whose refusal is the compiler's own (`LLB6005`, or `LLV7015` built alone), whose full
/// diagnostics the diagnose job keeps.
pub fn plan(register: &Register, prior: &Manifest, budget: u64) -> (usize, Vec<String>) {
    plan_of(&roots(register), prior, budget)
}

/// [`plan`] over the roots of any export.
pub fn plan_of(all: &[Root], prior: &Manifest, budget: u64) -> (usize, Vec<String>) {
    let plan = plan_keyed(all, None, prior, budget);
    (plan.shards, plan.diagnose)
}

/// Build `root` alone in a production project at `out` and keep, in
/// `out/build.log`, everything `lexlean production build` prints in human
/// form: a refusal's whole diagnostic, which the JSON form cuts short.
pub fn diagnose(input: &Input<'_>, out: &Path, root: &str) -> Result<(), UorustError> {
    let Input {
        profile,
        projection,
        register,
        targets,
    } = *input;
    profile.descriptor(targets)?;
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(|e| io(out, e))?;
    }
    let only: BTreeSet<String> = [root.to_string()].into_iter().collect();
    let named = production(
        profile,
        projection,
        out,
        register,
        targets,
        &BTreeMap::new(),
        Some(&only),
    )?;
    if named.is_empty() {
        return Err(UorustError::Tool(format!("{root} is not an eligible root")));
    }
    lexlean(out, &["lock"])?;
    let run = std::process::Command::new("lexlean")
        .args([
            "production",
            "build",
            "--request",
            "request.json",
            "--diagnostic-format",
            "human",
            "--color",
            "never",
        ])
        .current_dir(out)
        .output()
        .map_err(|e| UorustError::Tool(format!("lexlean production build: {e}")))?;
    let log = out.join("build.log");
    let mut text = run.stdout;
    text.extend(run.stderr);
    std::fs::write(&log, text).map_err(|e| io(&log, e))
}

/// What an export reads: its profile, the projection, the projection's
/// register, and the target descriptors (the output of `lexlean production
/// targets`).
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    /// What the export is of.
    pub profile: &'a Profile,
    /// The projection (`uorust project <workspace> --out ...`).
    pub projection: &'a Path,
    /// Its lift register.
    pub register: &'a Register,
    /// The target descriptors.
    pub targets: &'a str,
}

/// What a check of an export's roots decided, without a build.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Admission {
    /// The roots of the export.
    pub roots: usize,
    /// Those the compiler admits, by definition.
    pub admitted: Vec<String>,
    /// Those refused, by κ, with their causes.
    pub refused: BTreeMap<String, String>,
    /// The module prefix of the projection, which starts every name of its
    /// own a diagnostic quotes.
    pub prefix: String,
}

impl Admission {
    /// The causes of the refusals, each as its class ([`class`]), with the
    /// roots it refuses: the most first.
    pub fn causes(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for cause in self.refused.values() {
            *counts.entry(class(cause, &self.prefix)).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

/// The class of a refusal's cause: its first line in the compiler's own
/// terms, without what is the project's. Each quoted name of the project
/// (`<prefix>.<Module>.<name>`) and each definition named by its module
/// (`Module::name`) is `_`, and the closing parenthesis that locates the
/// violation in the project is left out.
pub(crate) fn class(cause: &str, prefix: &str) -> String {
    let line = cause.lines().next().unwrap_or_default();
    let line = match line.rfind(" (") {
        Some(at) if line.ends_with(')') => &line[..at],
        _ => line,
    };
    let own = format!("{prefix}.");
    let mut out = String::new();
    for (i, part) in line.split('`').enumerate() {
        if i % 2 == 1 {
            let name = if part.starts_with(&own) { "_" } else { part };
            out.push_str(&format!("`{name}`"));
            continue;
        }
        let words: Vec<&str> = part
            .split(' ')
            .map(|w| if w.contains("::") { "_" } else { w })
            .collect();
        out.push_str(&words.join(" "));
    }
    out
}

/// Decide which roots of an export the compiler admits, without a build:
/// the production project at `out` with every root marked is checked by
/// `lexlean check`, which types the project and decides each root's
/// eligibility (SPEC §17.13) and runs no Lean; each root a diagnostic names
/// is refused with it and the rest are checked again, until the check
/// passes. What is left are the roots a build is asked for: the kernel's
/// own refusals are a build's to find.
pub fn admit_roots(input: &Input<'_>, out: &Path) -> Result<Admission, UorustError> {
    let Input {
        profile,
        projection,
        register,
        targets,
    } = *input;
    let all = profile.export_roots(projection, register)?;
    let prefix = format!("{}.{BEHAVIOR}.", module_prefix(projection)?);
    let mut compiler: BTreeMap<String, String> = BTreeMap::new();
    let admitted = loop {
        if out.exists() {
            std::fs::remove_dir_all(out).map_err(|e| io(out, e))?;
        }
        let named = production(profile, projection, out, register, targets, &compiler, None)?;
        if named.is_empty() {
            break named;
        }
        lexlean(out, &["lock"])?;
        let result = lexlean(out, &["check"])?;
        if result.get("success").and_then(serde_json::Value::as_bool) == Some(true) {
            break named;
        }
        let messages = diagnostics(&result);
        let refused: BTreeMap<String, String> = messages
            .iter()
            .filter_map(|m| first_root(m, &prefix).map(|r| (r, m.clone())))
            .filter(|(r, _)| named.contains(r))
            .collect();
        if refused.is_empty() {
            return Err(UorustError::Tool(format!(
                "lexlean check: {}",
                messages
                    .first()
                    .map_or("refused without a diagnostic", String::as_str)
            )));
        }
        compiler.extend(refused);
    };
    let path = out.join(REFUSALS);
    let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
    let doc: toml::Value = toml::from_str(&text).map_err(|e| UorustError::Malformed {
        path: path.clone(),
        reason: format!("{e}"),
    })?;
    let refused = doc
        .get("refused")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some((
                r.get("kappa")?.as_str()?.to_string(),
                r.get("cause")?.as_str()?.to_string(),
            ))
        })
        .collect();
    Ok(Admission {
        roots: all.len(),
        admitted,
        refused,
        prefix: module_prefix(projection)?,
    })
}

impl Profile {
    /// The digest of the descriptor of the profile's target ([`TARGET`], for
    /// the translator) in `targets` (the output of `lexlean production
    /// targets`).
    fn descriptor(&self, targets: &str) -> Result<String, UorustError> {
        let bad = |reason: String| UorustError::Malformed {
            path: PathBuf::from("targets.json"),
            reason,
        };
        let descriptors: serde_json::Value =
            serde_json::from_str(targets).map_err(|e| bad(e.to_string()))?;
        descriptors
            .get("targets")
            .and_then(|t| t.as_array())
            .into_iter()
            .flatten()
            .find(|t| t.get("id").and_then(|i| i.as_str()) == Some(self.target.as_str()))
            .and_then(|t| t.get("digest"))
            .and_then(|d| d.as_str())
            .map(str::to_string)
            .ok_or_else(|| bad(format!("no {} descriptor", self.target)))
    }

    /// The whole descriptor of the profile's target in `targets`, as JSON
    /// text: its renderer and runtime digests with the rest, all of which a
    /// root's identity binds.
    fn descriptor_text(&self, targets: &str) -> Result<String, UorustError> {
        let bad = |reason: String| UorustError::Malformed {
            path: PathBuf::from("targets.json"),
            reason,
        };
        let descriptors: serde_json::Value =
            serde_json::from_str(targets).map_err(|e| bad(e.to_string()))?;
        descriptors
            .get("targets")
            .and_then(|t| t.as_array())
            .into_iter()
            .flatten()
            .find(|t| t.get("id").and_then(|i| i.as_str()) == Some(self.target.as_str()))
            .map(serde_json::Value::to_string)
            .ok_or_else(|| bad(format!("no {} descriptor", self.target)))
    }
}

/// The file of `kept` that records the build seconds of every pass and the
/// roots its bundles published, from which the next export sizes its shards.
pub const TIMING: &str = "timing.toml";

/// The directories of a shard's kept output under a merged `kept`.
pub const SHARD: &str = "shard-";

/// The kept outputs under `kept`: each `kept/shard-<i>` when the export was
/// sharded, else `kept` itself.
fn kept_roots(kept: &Path) -> Result<Vec<PathBuf>, UorustError> {
    let mut shards: Vec<PathBuf> = std::fs::read_dir(kept)
        .map_err(|e| io(kept, e))?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(SHARD))
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    shards.sort();
    Ok(if shards.is_empty() {
        vec![kept.to_path_buf()]
    } else {
        shards
    })
}

/// The file of `kept` that records the κ each root's package was exported
/// at, by package.
pub const KAPPAS: &str = "kappas.toml";

/// What part of the export one run builds, and for how long.
#[derive(Clone, Copy, Debug, Default)]
pub struct Scope {
    /// The time it may build for; the roots unbuilt then are refused.
    pub within: Option<std::time::Duration>,
    /// Shard I of N: every Nth eligible root, from the Ith.
    pub shard: Option<(usize, usize)>,
}

/// The code of the kernel's rejection of a certificate B. Run 43700278692's
/// diagnose job built roots alone that a batch had refused with it, and the
/// kernel accepted them.
const KERNEL: &str = "LLV7015";

/// What a refusal's cause starts with when the root was built alone.
const ALONE: &str = "built alone: ";

/// The cause of a root no build reached before the export's deadline.
const UNBUILT: &str = "not built before the export's deadline";

/// The cores one build occupies: `lexlean production build` and the `lean`
/// it runs beside itself, each a core at full load.
const CORES_PER_BUILD: usize = 2;

/// The work [`production_build`]'s workers share: the batches still to
/// build, the workers building one, the roots refused by the compiler, the
/// bundles kept, the passes run, and the first error.
#[derive(Default)]
struct Export {
    batches: Vec<Vec<String>>,
    busy: usize,
    compiler: BTreeMap<String, String>,
    bundles: usize,
    passes: usize,
    error: Option<UorustError>,
    late: bool,
    seconds: u64,
    published: usize,
    builds: usize,
}

/// What one build of a batch asks of the export next.
enum Outcome {
    /// It published the bundle at this path.
    Bundle(PathBuf),
    /// The compiler refused these roots, with their diagnostics.
    Refused(Vec<(String, String)>),
    /// The batch is over the project's total budget, with the diagnostic.
    Over(String),
    /// The export's deadline passed before the build ended.
    Late,
}

/// Export the translator: write the production project and build its roots
/// with `lexlean production build`, in batches on parallel workers, then
/// replay each bundle with `verify-artifact` and keep under `kept` what
/// [`sync`] reads of them: each bundle's result and package sources under
/// `kept/<n>`, and the refusals.
///
/// The roots are cut into one batch per worker, a worker per
/// [`CORES_PER_BUILD`] available cores, each built in its own project under
/// `out/<worker>` that marks only that batch's roots: a build's fixed cost,
/// verifying the marked project, is paid once per batch. A root the compiler finds ineligible is refused with
/// its diagnostic and only its batch is built again without it. A batch over
/// the project's total budget ([`TOTAL_BUDGET`]) is split in two, so no root
/// is refused for the size of the others. The roots `prior` (the manifest of
/// the last export) refused, at the same κ and by the same compiler, are
/// refused again before any build, so a steady export builds each batch
/// once. Each pass is printed to stderr. When `within` passes, the builds
/// still running are stopped and the roots not yet built are refused as
/// [`UNBUILT`], so the export ends with what it published. The project and
/// the build may live on a scratch volume.
///
/// A root whose closure κ ([`closure_kappas`], under `compiler`) the prior
/// manifest holds as certified ([`carry`]) is carried: its package source is
/// taken from `prior`'s source directories byte for byte, checked against
/// the SHA-256 the manifest records, and kept under [`CARRIED_DIR`] for
/// `sync`; it is not built, so an export whose roots are all unchanged runs
/// no build. It is carried only into a project that verifies: the prior
/// export verified the same [`project_identity`], or a build of this export
/// published (every build verifies the whole selection first, SPEC §24.7),
/// or, when none did, one verification of the production project with a
/// single carried root marked passes (`prior.verify`; its cost does not grow
/// with the roots, as a verification with every root marked does until it
/// passes the compiler's total budget). When that verification refuses, the
/// carried roots are built as a fresh export builds them. An export that
/// stops (an error, the deadline) before its project is verified publishes
/// no carried root. A shard carries every Nth carried root from the Ith, as
/// it builds every Nth of the rest.
pub fn production_build(
    input: &Input<'_>,
    out: &Path,
    kept: &Path,
    prior: &Prior<'_>,
    compiler: &Compiler,
    scope: Scope,
) -> Result<Exported, UorustError> {
    let carry = prior;
    let prior = prior.manifest;
    let Input {
        profile,
        projection,
        register,
        targets,
    } = *input;
    let Scope { within, shard } = scope;
    if let Some((k, n)) = shard.filter(|(k, n)| k >= n) {
        return Err(UorustError::Tool(format!(
            "shard {k} of {n}: a shard is below the count"
        )));
    }
    let deadline = within.map(|w| std::time::Instant::now() + w);
    let prefix = format!("{}.{BEHAVIOR}.", module_prefix(projection)?);
    let all = profile.export_roots(projection, register)?;
    // Refused before anything is written: no descriptor to export to.
    profile.descriptor(targets)?;
    for dir in [out, kept] {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let program = compiler.program.clone();
    let decided = self::carry(input, &all, compiler, carry)?;
    let Carry {
        ids,
        project,
        verified,
        carried,
        refused: compiler,
        marked,
    } = decided;
    let carried_names: BTreeSet<&str> = carried.iter().map(|c| c.definition.as_str()).collect();
    // The κ each package is exported at, so that a sync on a newer commit
    // takes a package only while its function is the one exported.
    let measured: String = all
        .iter()
        .map(|r| format!("{} = \"{}\"\n", r.package(), r.kappa))
        .collect();
    let path = kept.join(KAPPAS);
    std::fs::write(&path, measured).map_err(|e| io(&path, e))?;
    // The identity each package is exported at, which the manifest keeps so
    // that the next export carries the package while it is unchanged.
    let identified: String = all
        .iter()
        .filter_map(|r| {
            Some(format!(
                "{} = \"{}\"\n",
                r.package(),
                ids.get(&r.definition)?
            ))
        })
        .collect();
    let path = kept.join(CLOSURES);
    std::fs::write(&path, identified).map_err(|e| io(&path, e))?;
    let own_carried: Vec<&Carried> = carried
        .iter()
        .enumerate()
        .filter(|(i, _)| shard.is_none_or(|(k, n)| i % n == k))
        .map(|(_, c)| c)
        .collect();
    // What the export is of, for the steps that take it in. The
    // translator's own is every step's default and is not restated.
    if *profile != Profile::default() {
        let path = kept.join(PROFILE);
        std::fs::write(&path, profile.rows()).map_err(|e| io(&path, e))?;
    }
    // Every root the request may name: the higher-order ones are refused by
    // `production` itself.
    let probe = out.join("probe");
    let admitted = production(
        profile, projection, &probe, register, targets, &compiler, None,
    )?;
    let eligible: Vec<String> = admitted
        .into_iter()
        .filter(|r| !carried_names.contains(r.as_str()))
        .enumerate()
        .filter(|(i, _)| shard.is_none_or(|(k, n)| i % n == k))
        .map(|(_, r)| r)
        .collect();
    std::fs::remove_dir_all(&probe).map_err(|e| io(&probe, e))?;
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let workers = (cores / CORES_PER_BUILD).max(1);
    let count = workers.min(eligible.len()).max(1);
    let mut batches: Vec<Vec<String>> = vec![Vec::new(); count];
    for (i, root) in eligible.into_iter().enumerate() {
        batches[i % count].push(root);
    }
    batches.retain(|b| !b.is_empty());
    batches.reverse();
    eprintln!(
        "certify production: {} roots carried from the prior export, {} to build in {} batches on {workers} workers, {} roots refused before any build",
        own_carried.len(),
        batches.iter().map(Vec::len).sum::<usize>(),
        batches.len(),
        compiler.len() + marked.len()
    );
    let state = std::sync::Mutex::new(Export {
        batches,
        compiler,
        ..Export::default()
    });
    let source = Source {
        profile,
        projection,
        register,
        targets,
        prefix: &prefix,
        deadline,
        program: &program,
    };
    build_all(&source, out, kept, workers, &state);
    // A carried root is published only into a project a build or a
    // verification of this export verified (SPEC §24.7: every build verifies
    // the whole entrypoint selection before it renders a package). A build
    // that published has; when none did, one verification with the fewest
    // roots marked (one carried root) does, so that what it costs does not
    // grow with the roots. When it refuses, the carried roots are built as a
    // fresh export builds them, and each is refused as such a build refuses.
    let mut own_carried = own_carried;
    let mut verified = verified;
    let (unverified, stopped) = {
        let s = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (s.published == 0 && !verified, s.error.is_some() || s.late)
    };
    if unverified && stopped {
        // No build passed and none may run: the carried roots are left
        // unbuilt, never published unverified.
        let roots: Vec<String> = own_carried.iter().map(|c| c.definition.clone()).collect();
        own_carried.clear();
        state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .batches
            .push(roots);
    } else if unverified {
        // The verification runs where the first worker builds, so a project
        // that links packages finds its workspace as a build does.
        if let Some(first) = own_carried.first() {
            let only: BTreeSet<String> = std::iter::once(first.definition.clone()).collect();
            let scratch = out.join("0");
            if scratch.exists() {
                std::fs::remove_dir_all(&scratch).map_err(|e| io(&scratch, e))?;
            }
            let refusals = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .compiler
                .clone();
            production(
                profile,
                projection,
                &scratch,
                register,
                targets,
                &refusals,
                Some(&only),
            )?;
            let verdict = (carry.verify)(&scratch)?;
            std::fs::remove_dir_all(&scratch).map_err(|e| io(&scratch, e))?;
            match verdict {
                None => verified = true,
                Some(refusal) => {
                    eprintln!(
                        "certify production: the project does not verify, so nothing is carried and the {} carried roots are built: {}",
                        own_carried.len(),
                        refusal.lines().next().unwrap_or_default()
                    );
                    let roots: Vec<String> =
                        own_carried.iter().map(|c| c.definition.clone()).collect();
                    own_carried.clear();
                    let count = workers.min(roots.len()).max(1);
                    let mut batches: Vec<Vec<String>> = vec![Vec::new(); count];
                    for (i, root) in roots.into_iter().enumerate() {
                        batches[i % count].push(root);
                    }
                    state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .batches
                        .extend(batches.into_iter().rev());
                    build_all(&source, out, kept, workers, &state);
                }
            }
        }
    }
    keep_carried(kept, &own_carried, &prior.result)?;
    let mut s = state
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(e) = s.error {
        return Err(e);
    }
    // The roots no build reached by the deadline: uncertified at this
    // export, and built again by the next, since no compiler refused them.
    let unbuilt: Vec<String> = s.batches.drain(..).flatten().collect();
    for root in unbuilt {
        s.compiler
            .entry(root)
            .or_insert_with(|| UNBUILT.to_string());
    }
    // The refusals of every root refused by the end.
    let last = out.join("refusals");
    production(
        profile,
        projection,
        &last,
        register,
        targets,
        &s.compiler,
        None,
    )?;
    let refusals = last.join(REFUSALS);
    std::fs::copy(&refusals, kept.join(REFUSALS)).map_err(|e| io(&refusals, e))?;
    let timing = kept.join(TIMING);
    // An export that built nothing measured nothing: it keeps the prior
    // export's figure, which the next export sizes its shards by.
    let (seconds, roots) = match (s.published, prior.seconds_per_root) {
        (0, Some(per_root)) => (per_root, 1),
        _ => (s.seconds, u64::try_from(s.published).unwrap_or(u64::MAX)),
    };
    let text = format!("seconds = {seconds}\nroots = {roots}\n");
    std::fs::write(&timing, text).map_err(|e| io(&timing, e))?;
    // A build that published verified the whole project it was built in.
    if verified || s.published > 0 {
        let path = kept.join(PROJECT);
        std::fs::write(&path, format!("identity = \"{project}\"\n")).map_err(|e| io(&path, e))?;
    }
    // Builds are the passes that reached `lexlean production build`: a
    // batch every root of which was refused before is no build.
    eprintln!(
        "certify production: {} roots carried, {} bundles in {} passes, {} roots refused by the compiler, {} by the marking",
        own_carried.len(),
        s.bundles,
        s.passes,
        s.compiler.len(),
        marked.len()
    );
    Ok(Exported {
        carried: own_carried.len(),
        built: s.published,
        builds: s.builds,
        refused: s.compiler.len() + marked.len(),
    })
}

/// Run the batches of `state` to the end on `workers` parallel workers,
/// each in its own directory under `out`, keeping each published bundle
/// under `kept` ([`production_build`]).
fn build_all(
    source: &Source<'_>,
    out: &Path,
    kept: &Path,
    workers: usize,
    state: &std::sync::Mutex<Export>,
) {
    let wake = std::sync::Condvar::new();
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let wake = &wake;
            let dir = out.join(worker.to_string());
            scope.spawn(move || loop {
                let (batch, compiler, pass) = {
                    let mut s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    loop {
                        if s.error.is_some() || s.late || (s.batches.is_empty() && s.busy == 0) {
                            wake.notify_all();
                            return;
                        }
                        if let Some(batch) = s.batches.pop() {
                            s.busy += 1;
                            s.passes += 1;
                            break (batch, s.compiler.clone(), s.passes);
                        }
                        s = wake.wait(s).unwrap_or_else(std::sync::PoisonError::into_inner);
                    }
                };
                let started = std::time::Instant::now();
                let outcome = build_batch(source, &dir, &compiler, &batch);
                let mut s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                s.busy -= 1;
                let seconds = started.elapsed().as_secs();
                s.seconds += seconds;
                if matches!(outcome, Ok((_, Some(_)))) {
                    s.builds += 1;
                }
                match outcome {
                    Ok((_, None)) => {}
                    Ok((named, Some(Outcome::Bundle(bundle)))) => {
                        s.bundles += 1;
                        s.published += named.len();
                        let to = kept.join(s.bundles.to_string());
                        eprintln!(
                            "certify production: pass {pass} (worker {worker}): {} roots published in {seconds}s",
                            named.len()
                        );
                        if let Err(e) = keep(&bundle, &to) {
                            s.error = Some(e);
                        }
                    }
                    Ok((named, Some(Outcome::Refused(refused)))) => {
                        let mut rest = named.clone();
                        for (root, cause) in refused {
                            // The kernel rejects some roots' certificates in a
                            // batch that it accepts alone: such a root is
                            // built alone before it is refused.
                            if cause.starts_with(KERNEL) && named.len() > 1 {
                                eprintln!(
                                    "certify production: pass {pass} (worker {worker}): {root} rejected in a batch after {seconds}s; built alone next"
                                );
                                rest.retain(|r| *r != root);
                                s.batches.push(vec![root]);
                                continue;
                            }
                            eprintln!(
                                "certify production: pass {pass} (worker {worker}): {root} refused after {seconds}s: {}",
                                cause.lines().next().unwrap_or_default()
                            );
                            let cause = if cause.starts_with(KERNEL) {
                                format!("{ALONE}{cause}")
                            } else {
                                cause
                            };
                            s.compiler.insert(root, cause);
                        }
                        if !rest.is_empty() {
                            s.batches.push(rest);
                        }
                    }
                    Ok((named, Some(Outcome::Over(cause)))) => {
                        eprintln!(
                            "certify production: pass {pass} (worker {worker}): {} roots over budget after {seconds}s: {}",
                            named.len(),
                            cause.lines().next().unwrap_or_default()
                        );
                        if let [root] = named.as_slice() {
                            s.compiler.insert(root.clone(), cause);
                        } else {
                            let (first, second) = named.split_at(named.len() / 2);
                            s.batches.push(second.to_vec());
                            s.batches.push(first.to_vec());
                        }
                    }
                    Ok((named, Some(Outcome::Late))) => {
                        eprintln!(
                            "certify production: pass {pass} (worker {worker}): {} roots unbuilt at the deadline, after {seconds}s",
                            named.len()
                        );
                        s.late = true;
                        s.batches.push(named);
                    }
                    Err(e) => s.error = Some(e),
                }
                wake.notify_all();
            });
        }
    });
}

/// What every build of an export reads: the projection, the register, the
/// target descriptors, the roots' qualified prefix and the deadline.
struct Source<'a> {
    profile: &'a Profile,
    projection: &'a Path,
    register: &'a Register,
    targets: &'a str,
    prefix: &'a str,
    deadline: Option<std::time::Instant>,
    /// The compiler's program ([`Compiler::program`]).
    program: &'a Path,
}

/// Build one batch in the production project at `dir`: the roots the request
/// named (none when the compiler had refused them all) and what the build
/// asks next, its bundle replayed with `verify-artifact` when it published.
fn build_batch(
    source: &Source<'_>,
    dir: &Path,
    compiler: &BTreeMap<String, String>,
    batch: &[String],
) -> Result<(Vec<String>, Option<Outcome>), UorustError> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let only: BTreeSet<String> = batch.iter().cloned().collect();
    let named = production(
        source.profile,
        source.projection,
        dir,
        source.register,
        source.targets,
        compiler,
        Some(&only),
    )?;
    if named.is_empty() {
        return Ok((named, None));
    }
    let program = source.program;
    run_lexlean(program, dir, &["lock"])?;
    let Some(result) = lexlean_until(
        program,
        dir,
        &["production", "build", "--request", "request.json"],
        source.deadline,
    )?
    else {
        return Ok((named, Some(Outcome::Late)));
    };
    if result.get("success").and_then(serde_json::Value::as_bool) == Some(true) {
        // The artifact's path is the project's, relative to `dir`, where
        // lexlean runs.
        let artifact = result
            .get("artifacts")
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|a| a.as_str())
            .ok_or_else(|| UorustError::Tool("lexlean production build: no bundle".into()))?;
        let bundle = dir.join(artifact);
        let replay = run_lexlean(program, dir, &["production", "verify-artifact", artifact])?;
        if replay.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(UorustError::Tool(format!(
                "lexlean production verify-artifact: {}",
                diagnostics(&replay).join("; ")
            )));
        }
        return Ok((named, Some(Outcome::Bundle(bundle))));
    }
    let messages = diagnostics(&result);
    let refused: Vec<(String, String)> = messages
        .iter()
        .filter(|m| !m.contains(TOTAL_BUDGET))
        .filter_map(|m| first_root(m, source.prefix).map(|r| (r, m.clone())))
        .filter(|(r, _)| !compiler.contains_key(r))
        .collect();
    if !refused.is_empty() {
        return Ok((named, Some(Outcome::Refused(refused))));
    }
    // A refusal that names no root (the total budget, or a limit of the
    // build's own, as run 43706236017's LLI9001 on a deep package manifest)
    // is the batch's: it is split until the refusal names its root or one
    // root holds it.
    match messages
        .iter()
        .find(|m| m.contains(TOTAL_BUDGET))
        .or(messages.first())
    {
        Some(m) => Ok((named, Some(Outcome::Over(m.clone())))),
        None => Err(UorustError::Tool(
            "lexlean production build: refused without a diagnostic".into(),
        )),
    }
}

/// The LexLean compiler's version, as `lexlean --version` states it.
fn compiler_version() -> Result<String, UorustError> {
    Ok(Compiler::installed()?.version())
}

/// The compiler an export builds with: every line `lexlean --version`
/// prints. Its version alone is not the compiler: two builds of one version
/// can differ in their compiler semantics (the digest `lexlean --version`
/// states on its own line, which the lock and the result bind, SPEC §21.2,
/// §24.7) or in the Lean toolchain, and either changes what a build emits.
///
/// The program a build runs is part of the compiler too, so that what an
/// export states it builds with is what it runs: a build never falls back
/// to whichever `lexlean` the path holds. The program is no input of an
/// identity: where a compiler is installed does not change what it emits,
/// and what it states of itself does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiler {
    /// Every line `<program> --version` prints.
    pub stated: String,
    /// The program every build of the export runs.
    pub program: PathBuf,
}

/// The compiler's program on the path.
pub const LEXLEAN: &str = "lexlean";

impl Compiler {
    /// The compiler on the path.
    pub fn installed() -> Result<Compiler, UorustError> {
        let run = std::process::Command::new(LEXLEAN)
            .arg("--version")
            .output()
            .map_err(|e| UorustError::Tool(format!("lexlean --version: {e}")))?;
        Ok(Compiler {
            stated: String::from_utf8_lossy(&run.stdout).into_owned(),
            program: PathBuf::from(LEXLEAN),
        })
    }

    /// The version the manifest records: the second word of the first line.
    pub fn version(&self) -> String {
        self.stated
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or_default()
            .to_string()
    }
}

/// The domain tag of a root's closure κ ([`closure_kappas`]): the identity
/// of everything its exported package is a function of. Its text is the one
/// the first keyed export recorded, so the roots a manifest holds at it are
/// still carried; a manifest's closure κ of another tag (an export's before
/// the project, the linked packages and the order of declarations were in
/// it) never equals one of this tag, and its roots are built once again.
pub const CLOSURE: &str = "uorust/certified-root/1";

/// The file of `kept` that records the closure κ each root's package was
/// exported at, by package, so that the manifest keeps it for the next export.
pub const CLOSURES: &str = "closures.toml";

/// The file of `kept` that lists the packages an export carried from the
/// prior one; their sources are kept beside it under [`CARRIED_DIR`].
pub const CARRIED: &str = "carried.toml";

/// The directory of `kept` that holds each carried package's `src/lib.rs`,
/// as `<package>.rs`.
pub const CARRIED_DIR: &str = "carried";

/// The file of `kept` that records the project identity ([`project_identity`])
/// the export verified, by a build that published or by the verification
/// before a carry, so that the manifest keeps it for the next export.
pub const PROJECT: &str = "project.toml";

/// A module's text read for an identity: its frame (every non-blank line
/// outside a top-level declaration, in order), each top-level declaration (a
/// definition, structure, inductive, theorem, any `kind{..}` block) by name
/// with its text, and the names in the order the module declares them.
/// Blank lines are left out. The order is kept apart from the texts because
/// it is an input only relative to the declarations a root reaches: the
/// language links a module's declarations in order and refuses a forward
/// reference (SPEC §17.11, `LLT4001`), so two closures of equal texts in
/// another relative order are another program, while a declaration
/// inserted elsewhere moves the others without changing that order.
fn declarations(source: &str) -> (Vec<String>, BTreeMap<String, String>, Vec<String>) {
    let mut frame = Vec::new();
    let mut decls: BTreeMap<String, String> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut open: Option<(Option<String>, String)> = None;
    let starts = |line: &str| {
        line.split_once('{').is_some_and(|(kind, _)| {
            !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        })
    };
    let mut close = |name: Option<String>, text: String, frame: &mut Vec<String>| match name {
        Some(name) => {
            if !order.contains(&name) {
                order.push(name.clone());
            }
            decls.entry(name).or_default().push_str(&text);
        }
        None => frame.extend(text.lines().map(str::to_string)),
    };
    for line in source.lines() {
        if let Some((name, text)) = open.as_mut() {
            if name.is_none() {
                *name = line
                    .strip_prefix("  name: ")
                    .and_then(|n| n.strip_suffix(','))
                    .map(str::to_string);
            }
            text.push_str(line);
            text.push('\n');
            if line == "}" {
                let (name, text) = open.take().unwrap_or_default();
                close(name, text, &mut frame);
            }
            continue;
        }
        if !starts(line) {
            if !line.trim().is_empty() {
                frame.push(line.to_string());
            }
            continue;
        }
        let one_line = line.ends_with('}');
        let name = line
            .split_once("{name: ")
            .and_then(|(_, rest)| rest.split([',', '}']).next())
            .map(str::to_string);
        if one_line {
            close(name, format!("{line}\n"), &mut frame);
        } else {
            open = Some((name, format!("{line}\n")));
        }
    }
    if let Some((name, text)) = open {
        close(name, text, &mut frame);
    }
    (frame, decls, order)
}

/// The files of a projection outside its modules that a build reads, by
/// project-relative path, with their SHA-256: `lexlean.toml`; the workspace
/// files its lock binds (`[[workspace_file]]` of the lock it names, the Lake
/// files and the Lean toolchain); and every file of each lexicon source it
/// reads from a path. The builtin lexicons are the compiler's own and are
/// bound by its semantics ([`Compiler`]). A listed file that is absent is
/// stated absent, so that its appearing changes the identity too.
fn project_files(projection: &Path) -> Result<Vec<(String, String)>, UorustError> {
    let config_path = projection.join("lexlean.toml");
    let config_text = std::fs::read_to_string(&config_path).map_err(|e| io(&config_path, e))?;
    let config: toml::Value = toml::from_str(&config_text).map_err(|e| UorustError::Malformed {
        path: config_path.clone(),
        reason: format!("{e}"),
    })?;
    let mut files: BTreeSet<String> = ["lexlean.toml".to_string()].into_iter().collect();
    let lock = config
        .get("lockfile")
        .and_then(toml::Value::as_str)
        .unwrap_or("lexlean.lock");
    if let Ok(text) = std::fs::read_to_string(projection.join(lock)) {
        if let Ok(doc) = toml::from_str::<toml::Value>(&text) {
            files.extend(
                doc.get("workspace_file")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.get("path")?.as_str().map(str::to_string)),
            );
        }
    }
    for source in config
        .get("lexicon_source")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|s| s.get("kind").and_then(toml::Value::as_str) == Some("path"))
    {
        let Some(dir) = source.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let root = projection.join(dir);
        if root.is_dir() {
            let mut found = Vec::new();
            tree_files(projection, &root, &mut found)?;
            files.extend(found.into_iter().map(|(rel, _)| rel));
        } else {
            files.insert(dir.to_string());
        }
    }
    files
        .into_iter()
        .map(|rel| {
            let path = projection.join(&rel);
            let digest = match std::fs::read(&path) {
                Ok(bytes) => crate::project::sha(&bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => "absent".to_string(),
                Err(e) => return Err(io(&path, e)),
            };
            Ok((rel, digest))
        })
        .collect()
}

/// The `[[source_package]]` rows of a project's (or a package's)
/// `lexlean.toml`, each its id, version and workspace-relative path.
fn package_rows(config: &Path) -> Result<Vec<(String, String, String)>, UorustError> {
    let text = match std::fs::read_to_string(config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(config, e)),
    };
    let doc: toml::Value = toml::from_str(&text).map_err(|e| UorustError::Malformed {
        path: config.to_path_buf(),
        reason: format!("{e}"),
    })?;
    let field = |row: &toml::Value, key: &str| {
        row.get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok(doc
        .get("source_package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            (
                field(row, "package"),
                field(row, "version"),
                field(row, "path"),
            )
        })
        .collect())
}

/// The source roots a project's (or a package's) `lexlean.toml` declares;
/// `src` when it declares none.
fn source_roots(config: &Path) -> Vec<String> {
    let roots: Vec<String> = std::fs::read_to_string(config)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|doc| {
            Some(
                doc.get("source_roots")?
                    .as_array()?
                    .iter()
                    .filter_map(|r| r.as_str().map(str::to_string))
                    .collect(),
            )
        })
        .unwrap_or_default();
    if roots.is_empty() {
        vec!["src".to_string()]
    } else {
        roots
    }
}

/// The SHA-256 over `dir`'s `lexlean.toml` and every file beneath its
/// source roots, each by its path relative to `dir`, in bytewise path
/// order: the files of a package the language reads (SPEC §10.5); a
/// directory that is absent is `absent`.
fn package_tree(dir: &Path) -> Result<String, UorustError> {
    if !dir.is_dir() {
        return Ok("absent".to_string());
    }
    let config = dir.join("lexlean.toml");
    let mut files: Vec<(String, PathBuf)> = vec![("lexlean.toml".to_string(), config.clone())];
    for root in source_roots(&config) {
        let at = dir.join(&root);
        if at.is_dir() {
            tree_files(dir, &at, &mut files)?;
        }
    }
    files.sort();
    let mut text = String::new();
    for (rel, path) in files {
        let digest = match std::fs::read(&path) {
            Ok(bytes) => crate::project::sha(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "absent".to_string(),
            Err(e) => return Err(io(&path, e)),
        };
        text.push_str(&format!("{digest} {rel}\n"));
    }
    Ok(crate::project::sha(text.as_bytes()))
}

/// The packages a projection links, as identity lines: `workspace
/// <sha256>`, the workspace file that grants them (SPEC §10.5: the nearest
/// proper ancestor of the project that holds `lexlean-workspace.toml`), and
/// one `source-package <id> <version> <path> <sha256>` per package of the
/// transitive closure of the project's `[[source_package]]` rows, each
/// path resolved from the workspace root and hashed by [`package_tree`].
/// A package module is linked into a root's package like a module of the
/// project, but it lies outside the project's source roots, so without
/// these lines a changed package body would leave every identity as it was.
/// A project that declares no package reads nothing outside its root and
/// has no lines here.
fn source_packages(projection: &Path) -> Result<Vec<String>, UorustError> {
    let declared = package_rows(&projection.join("lexlean.toml"))?;
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    let canonical = projection.canonicalize().map_err(|e| io(projection, e))?;
    let workspace = canonical
        .ancestors()
        .skip(1)
        .find(|a| a.join("lexlean-workspace.toml").is_file());
    let mut lines = vec![match workspace {
        Some(root) => {
            let path = root.join("lexlean-workspace.toml");
            let bytes = std::fs::read(&path).map_err(|e| io(&path, e))?;
            format!("workspace {}", crate::project::sha(&bytes))
        }
        None => "workspace absent".to_string(),
    }];
    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut work = declared;
    while let Some(row) = work.pop() {
        if !seen.insert(row.clone()) {
            continue;
        }
        let (id, version, path) = &row;
        let digest = match workspace {
            Some(root) => {
                let dir = root.join(path);
                work.extend(package_rows(&dir.join("lexlean.toml"))?);
                package_tree(&dir)?
            }
            None => "absent".to_string(),
        };
        lines.push(format!("source-package {id} {version} {path} {digest}"));
    }
    lines.sort();
    Ok(lines)
}

/// The domain tag of a projection's identity ([`project_identity`]).
pub const PROJECT_IDENTITY: &str = "uorust/certified-project/1";

/// The identity of what every build of an export verifies besides a root's
/// closure: the SHA-256 of the text below.
///
/// ```text
/// uorust/certified-project/1
/// compiler <line>                one per line of `lexlean --version`
/// target <id> <sha256>           the profile's target descriptor, as JSON
/// effects <effect> ..            the effects every root admits
/// roots <definition> ..          the roots the production project marks
/// file <sha256> <path>           one per file of [`project_files`], the
///                                lock, and every file beneath the
///                                project's source roots
/// <line>                         one per line of [`source_packages`]
/// ```
///
/// A build verifies the whole entrypoint selection (SPEC §24.7), so a root
/// whose closure is unchanged is still refused by a fresh build when any
/// other declaration of the project, in any module, no longer verifies (a
/// false theorem, a forward reference). A root's identity ([`closure_kappas`])
/// therefore says what its package is, and this identity says whether the
/// project it would be built in is the one an earlier export verified:
/// a root is carried only under both ([`carry`]).
pub fn project_identity(input: &Input<'_>, compiler: &Compiler) -> Result<String, UorustError> {
    let Input {
        profile,
        projection,
        register,
        targets,
    } = *input;
    let mut text = format!("{PROJECT_IDENTITY}\n");
    for line in compiler.stated.lines() {
        text.push_str(&format!("compiler {line}\n"));
    }
    let descriptor = profile.descriptor_text(targets)?;
    text.push_str(&format!(
        "target {} {}\n",
        profile.target,
        crate::project::sha(descriptor.as_bytes())
    ));
    text.push_str(&format!("effects {}\n", profile.effects.join(" ")));
    let mut roots: Vec<String> = profile
        .export_roots(projection, register)?
        .into_iter()
        .map(|r| r.definition)
        .collect();
    roots.sort();
    text.push_str(&format!("roots {}\n", roots.join(" ")));
    let config = projection.join("lexlean.toml");
    let mut files: BTreeMap<String, String> = project_files(projection)?.into_iter().collect();
    let lock = std::fs::read_to_string(&config)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|d| d.get("lockfile")?.as_str().map(str::to_string))
        .unwrap_or_else(|| "lexlean.lock".to_string());
    let lock_path = projection.join(&lock);
    let mut read = vec![(lock, lock_path)];
    for root in source_roots(&config) {
        let at = projection.join(&root);
        if at.is_dir() {
            tree_files(projection, &at, &mut read)?;
        }
    }
    for (rel, path) in read {
        let digest = match std::fs::read(&path) {
            Ok(bytes) => crate::project::sha(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "absent".to_string(),
            Err(e) => return Err(io(&path, e)),
        };
        files.insert(rel, digest);
    }
    for (rel, digest) in files {
        text.push_str(&format!("file {digest} {rel}\n"));
    }
    for line in source_packages(projection)? {
        text.push_str(&format!("{line}\n"));
    }
    Ok(crate::project::sha(text.as_bytes()))
}

/// The identity of each root's export, by definition: the SHA-256 of the
/// text below, which states once everything the root's package source is a
/// function of.
///
/// ```text
/// uorust/certified-root/1
/// compiler <line>                one per line of `lexlean --version`
/// target <id> <sha256>           the profile's target descriptor, as JSON
/// effects <effect> ..            the effects every root admits
/// file <sha256> <path>           one per file of [`project_files`]
/// <line>                         one per line of [`source_packages`]
/// frame <module> <sha256>        Behavior's and Prelude's lines outside declarations
/// module <module> <sha256>       any other module of the project their imports reach, whole
/// package <name> <version>       the package the request names
/// root <prefix>.Behavior.<name>  the production root
/// declaration <Module::name> <sha256>   one per declaration of the closure
/// order <Module> <name> ..       the closure's declarations of each module, in its order
/// ```
///
/// The closure is the root's declaration and every declaration of Behavior
/// and Prelude its text reaches, definitions and the types they name alike,
/// by the same reading of names as [`Definitions::closure`]: a callee is in
/// it, so a change in a callee changes the identity although the root's own
/// κ (its Rust function's) does not change. A module of a linked package
/// (`Prefix.Module::name`) is not read through the closure: every package
/// the project links is bound whole, by its tree, so a changed package body
/// changes every root's identity. What the closure leaves out does not
/// reach the package: the compiler renders a root's package from its
/// verified lowered program alone (SPEC §24.7, "the renderer of §17.16 over
/// the verified lowered program"), and `sync` takes only its `src/lib.rs`,
/// not the manifests that bind the whole project's semantic identity. The
/// batch a root is built in is no input either: a request names roots, and
/// each package is rendered for its own root. Whether the rest of the
/// project still verifies, which decides whether a build publishes the
/// package at all, is the project's identity's ([`project_identity`]).
pub fn closure_kappas(
    input: &Input<'_>,
    roots: &[Root],
    compiler: &Compiler,
) -> Result<BTreeMap<String, String>, UorustError> {
    let Input {
        profile,
        projection,
        targets,
        ..
    } = *input;
    let prefix = module_prefix(projection)?;
    let modules = modules(projection)?;
    let mut shared = format!("{CLOSURE}\n");
    for line in compiler.stated.lines() {
        shared.push_str(&format!("compiler {line}\n"));
    }
    let descriptor = profile.descriptor_text(targets)?;
    shared.push_str(&format!(
        "target {} {}\n",
        profile.target,
        crate::project::sha(descriptor.as_bytes())
    ));
    shared.push_str(&format!("effects {}\n", profile.effects.join(" ")));
    for (path, digest) in project_files(projection)? {
        shared.push_str(&format!("file {digest} {path}\n"));
    }
    for line in source_packages(projection)? {
        shared.push_str(&format!("{line}\n"));
    }
    let mut all: BTreeMap<String, String> = BTreeMap::new();
    let mut orders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut imported: BTreeSet<String> = BTreeSet::new();
    for (module, text) in &modules {
        let (frame, decls, order) = declarations(text);
        orders.insert(module.clone(), order);
        let frame = frame.join("\n");
        shared.push_str(&format!(
            "frame {module} {}\n",
            crate::project::sha(frame.as_bytes())
        ));
        imported.extend(
            frame
                .lines()
                .filter_map(|l| l.strip_prefix("\\importmodule{")?.strip_suffix('}'))
                .filter(|m| !modules.contains_key(*m))
                .map(str::to_string),
        );
        all.extend(
            decls
                .into_iter()
                .map(|(n, t)| (format!("{module}::{n}"), t)),
        );
    }
    // A module is bound whole with every module its own imports reach: the
    // compiler renders a root through each of them, so one read only at the
    // first step would leave a body two imports away in no identity.
    let mut reached: BTreeMap<String, String> = BTreeMap::new();
    let mut pending: Vec<String> = imported.into_iter().collect();
    while let Some(module) = pending.pop() {
        if reached.contains_key(&module) {
            continue;
        }
        let path = projection
            .join("src")
            .join(format!("{}.lex.tex", module.replace('.', "/")));
        let digest = match std::fs::read(&path) {
            Ok(bytes) => {
                pending.extend(
                    String::from_utf8_lossy(&bytes)
                        .lines()
                        .filter_map(|l| l.trim().strip_prefix("\\importmodule{")?.strip_suffix('}'))
                        .filter(|m| !modules.contains_key(*m))
                        .map(str::to_string),
                );
                crate::project::sha(&bytes)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "absent".to_string(),
            Err(e) => return Err(io(&path, e)),
        };
        reached.insert(module, digest);
    }
    for (module, digest) in &reached {
        shared.push_str(&format!("module {module} {digest}\n"));
    }
    let declarations = Definitions(all);
    let mut out = BTreeMap::new();
    for root in roots {
        let mut text = shared.clone();
        text.push_str(&format!("package {} {PACKAGE_VERSION}\n", root.package()));
        text.push_str(&format!("root {prefix}.{BEHAVIOR}.{}\n", root.definition));
        let closure = declarations.closure(&format!("{BEHAVIOR}::{}", root.definition));
        for d in &closure {
            let body = declarations.text(d).unwrap_or_default();
            text.push_str(&format!(
                "declaration {d} {}\n",
                crate::project::sha(body.as_bytes())
            ));
        }
        for (module, order) in &orders {
            let reached: Vec<&str> = order
                .iter()
                .filter(|n| closure.contains(&format!("{module}::{n}")))
                .map(String::as_str)
                .collect();
            if !reached.is_empty() {
                text.push_str(&format!("order {module} {}\n", reached.join(" ")));
            }
        }
        out.insert(
            root.definition.clone(),
            crate::project::sha(text.as_bytes()),
        );
    }
    Ok(out)
}

/// What an export takes from the one before it: its manifest, and the
/// directories that hold its packages' sources in a certified crate's layout
/// (`<dir>/roots/<package>.rs`), searched in order.
#[derive(Clone, Copy, Debug)]
pub struct Prior<'a> {
    /// The manifest of the last export.
    pub manifest: &'a Manifest,
    /// Where its package sources are.
    pub sources: &'a [PathBuf],
    /// How a project the prior export did not verify is verified before
    /// anything is carried into it: [`verify_project`].
    pub verify: Verifier,
}

/// A check of the production project in a directory: `None` when it
/// verifies, else the refusal.
pub type Verifier = fn(&Path) -> Result<Option<String>, UorustError>;

/// The verification every build of an export runs before it renders a
/// package, run once on the production project in `dir` with no package
/// requested: `lexlean lock`, then `lexlean verify`. `None` when it
/// verifies, else its diagnostics.
pub fn verify_project(dir: &Path) -> Result<Option<String>, UorustError> {
    lexlean(dir, &["lock"])?;
    let result = lexlean(dir, &["verify"])?;
    if result.get("success").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(None);
    }
    let found = diagnostics(&result);
    Ok(Some(if found.is_empty() {
        "lexlean verify: refused without a diagnostic".to_string()
    } else {
        found.join("; ")
    }))
}

/// A root an export takes from the prior export without a build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    /// The definition.
    pub definition: String,
    /// Its package.
    pub package: String,
    /// The identity it was exported at, which is this export's too.
    pub closure: String,
    /// The SHA-256 of the package's `src/lib.rs`, as the prior manifest
    /// records it.
    pub source: String,
    /// The kernel's status of its certificates, as the prior manifest
    /// records it.
    pub certification: String,
    /// The Rust edition its package's Cargo manifest declared, as the prior
    /// manifest records it: the sources are written for it, and a crate
    /// that compiles them under another does not compile (an all-carried
    /// sync has no bundle to read it from).
    pub edition: String,
    /// The package's `src/lib.rs`.
    pub bytes: Vec<u8>,
}

/// The roots of `roots` the prior export certified at the identity `ids`
/// states for them, with their package sources: a root is carried only when
/// the prior manifest has a root of the same package at that identity and a
/// source directory holds a file whose SHA-256 is the one the manifest
/// records, so what is carried is byte for byte what the prior build
/// published, and the manifest records the package's edition. Any other
/// root is built. Whether the project still verifies is [`carry`]'s.
pub fn carried(
    roots: &[Root],
    ids: &BTreeMap<String, String>,
    prior: &Prior<'_>,
) -> Result<Vec<Carried>, UorustError> {
    let mut out = Vec::new();
    for root in roots {
        let Some(id) = ids.get(&root.definition) else {
            continue;
        };
        let package = root.package();
        let Some(was) = prior
            .manifest
            .roots
            .iter()
            .find(|c| c.package == package && c.closure.as_deref() == Some(id.as_str()))
        else {
            continue;
        };
        let Some(edition) = was.edition.clone() else {
            continue;
        };
        for dir in prior.sources {
            let path = dir.join("roots").join(format!("{package}.rs"));
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io(&path, e)),
            };
            if crate::project::sha(&bytes) != was.source {
                continue;
            }
            out.push(Carried {
                definition: root.definition.clone(),
                package: package.clone(),
                closure: id.clone(),
                source: was.source.clone(),
                certification: was.certification.clone(),
                edition: edition.clone(),
                bytes,
            });
            break;
        }
    }
    Ok(out)
}

/// What an export takes from the prior one and what it refuses before any
/// build, decided once for the plan and the export alike.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Carry {
    /// Every root's identity, by definition.
    pub ids: BTreeMap<String, String>,
    /// The projection's identity ([`project_identity`]).
    pub project: String,
    /// Whether this export's project is known to verify without a build or
    /// a verification: the prior export verified a project of the same
    /// identity.
    pub verified: bool,
    /// The roots carried when the project verifies (candidates, until the
    /// export has verified it).
    pub carried: Vec<Carried>,
    /// The roots the prior export's compiler refused, refused again before
    /// any build (by definition, with the cause).
    pub refused: BTreeMap<String, String>,
    /// The roots the production project's marking refuses (a definition
    /// with a function parameter in the closure), by definition.
    pub marked: BTreeMap<String, String>,
}

/// The refusals of the prior export an export repeats before any build: a
/// root at the κ the prior refused under the same compiler version, whose
/// cause names a production root, and not a kernel rejection in a batch.
fn prior_refusals(
    roots: &[Root],
    prior: &Manifest,
    version: &str,
    prefix: &str,
) -> BTreeMap<String, String> {
    if prior.compiler.is_empty() || prior.compiler != version {
        return BTreeMap::new();
    }
    let by_kappa: BTreeMap<&str, &str> = roots
        .iter()
        .map(|r| (r.kappa.as_str(), r.definition.as_str()))
        .collect();
    prior
        .refused
        .iter()
        .filter_map(|(kappa, cause)| {
            by_kappa
                .get(kappa.as_str())
                .filter(|_| cause.contains(&format!("`{prefix}")))
                // A kernel rejection is carried only when the root was built
                // alone: in a batch it can be the batch's, not the root's.
                .filter(|_| !cause.starts_with(KERNEL))
                .map(|d| ((*d).to_string(), cause.clone()))
        })
        .collect()
}

/// Decide what an export of `roots` carries from `prior` and refuses before
/// any build. A root is a carried candidate when the prior manifest
/// certified it at its closure κ ([`carried`]): its package is then the one
/// a build would render. Whether the project it is carried into verifies is
/// decided by the export, never here and never by a verification that grows
/// with the number of roots ([`production_build`]): `verified` says only
/// whether the prior export verified a project of the same
/// [`project_identity`]. The marking's own refusals are counted here too, so
/// that a plan and its export state the same counts.
pub fn carry(
    input: &Input<'_>,
    roots: &[Root],
    compiler: &Compiler,
    prior: &Prior<'_>,
) -> Result<Carry, UorustError> {
    let Input {
        profile,
        projection,
        ..
    } = *input;
    let ids = closure_kappas(input, roots, compiler)?;
    let project = project_identity(input, compiler)?;
    let prefix = format!("{}.{BEHAVIOR}.", module_prefix(projection)?);
    let before = prior_refusals(roots, prior.manifest, &compiler.version(), &prefix);
    let carried = carried(roots, &ids, prior)?;
    let verified = prior.manifest.project.as_deref() == Some(project.as_str());
    let names: BTreeSet<&str> = carried.iter().map(|c| c.definition.as_str()).collect();
    let refused: BTreeMap<String, String> = before
        .into_iter()
        .filter(|(d, _)| !names.contains(d.as_str()))
        .collect();
    let exported: BTreeSet<&str> = roots
        .iter()
        .map(|r| r.definition.as_str())
        .filter(|d| !refused.contains_key(*d) && !names.contains(d))
        .collect();
    let marked = profile.mark(&modules(projection)?, &exported).2;
    Ok(Carry {
        ids,
        project,
        verified,
        carried,
        refused,
        marked,
    })
}

/// What an export is left to do: the shards that build it within a budget,
/// the roots to diagnose, and the counts of roots carried from the prior
/// export (`None` when no identity was computed, so nothing could be
/// carried), left to build, and refused before any build.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Shards.
    pub shards: usize,
    /// Roots whose compiler refusal is kept in full.
    pub diagnose: Vec<String>,
    /// Roots carried, when identities were computed.
    pub carried: Option<usize>,
    /// Roots left to build.
    pub build: usize,
    /// Roots the prior export refused.
    pub refused: usize,
}

/// [`plan_of`] that leaves out what `carry` decided (the roots carried and
/// those refused before any build, by the prior compiler or by the
/// marking, exactly as the export decides them): only what is left to
/// build is sharded. Without it, the prior manifest's refusals by κ are
/// the only ones known.
pub fn plan_keyed(all: &[Root], carry: Option<&Carry>, prior: &Manifest, budget: u64) -> Plan {
    let refused = |r: &Root| match carry {
        Some(c) => c.refused.contains_key(&r.definition) || c.marked.contains_key(&r.definition),
        None => {
            prior.refused.contains_key(&r.kappa) && !prior.refused[&r.kappa].starts_with(UNBUILT)
        }
    };
    let is_carried =
        |r: &Root| carry.is_some_and(|c| c.carried.iter().any(|k| k.definition == r.definition));
    let left: Vec<&Root> = all
        .iter()
        .filter(|r| !is_carried(r) && !refused(r))
        .collect();
    let per_root = prior
        .seconds_per_root
        .unwrap_or(MEASURED_SECONDS_PER_ROOT)
        .max(1);
    let per_shard = usize::try_from(budget / (per_root * BUILDS_PER_SHARD))
        .unwrap_or(usize::MAX)
        .clamp(1, MAX_ROOTS_PER_SHARD);
    let shards = left.len().div_ceil(per_shard).clamp(1, MAX_SHARDS);
    let diagnose = all
        .iter()
        .filter(|r| !is_carried(r))
        .filter(|r| {
            prior
                .refused
                .get(&r.kappa)
                .is_some_and(|c| c.starts_with(ALONE) || c.starts_with("LLB6005"))
        })
        .map(|r| r.definition.clone())
        .collect();
    Plan {
        shards,
        diagnose,
        carried: carry.map(|_| all.iter().filter(|r| is_carried(r)).count()),
        build: left.len(),
        refused: all.iter().filter(|r| !is_carried(r) && refused(r)).count(),
    }
}

/// What one export did: the roots it carried, the roots it published by a
/// build, the builds it ran, and the roots refused by the end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Exported {
    /// Roots carried from the prior export.
    pub carried: usize,
    /// Roots published by this export's builds.
    pub built: usize,
    /// Builds run (`lexlean production build` invocations).
    pub builds: usize,
    /// Roots refused.
    pub refused: usize,
}

/// Write what an export carried under `kept`: [`CARRIED`] with one row per
/// package and the prior export's result ids, and each package's source
/// under [`CARRIED_DIR`].
fn keep_carried(kept: &Path, carried: &[&Carried], results: &str) -> Result<(), UorustError> {
    let dir = kept.join(CARRIED_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let mut text = format!("results = {}\n", toml_quoted(results));
    for c in carried {
        let path = dir.join(format!("{}.rs", c.package));
        std::fs::write(&path, &c.bytes).map_err(|e| io(&path, e))?;
        text.push_str(&format!(
            "\n[[carried]]\npackage = \"{}\"\nclosure = \"{}\"\nsource = \"{}\"\ncertification = {}\nedition = {}\n",
            c.package,
            c.closure,
            c.source,
            toml_quoted(&c.certification),
            toml_quoted(&c.edition)
        ));
    }
    let path = kept.join(CARRIED);
    std::fs::write(&path, text).map_err(|e| io(&path, e))
}

/// Keep what [`sync`] reads of a bundle under `to`: its `result.json` and
/// every package's `src/lib.rs` and Cargo manifest, at their paths in the
/// bundle.
fn keep(bundle: &Path, to: &Path) -> Result<std::path::PathBuf, UorustError> {
    if to.exists() {
        std::fs::remove_dir_all(to).map_err(|e| io(to, e))?;
    }
    std::fs::create_dir_all(to).map_err(|e| io(to, e))?;
    std::fs::copy(bundle.join("result.json"), to.join("result.json"))
        .map_err(|e| io(&bundle.join("result.json"), e))?;
    let packages = bundle.join("packages");
    for target in std::fs::read_dir(&packages).map_err(|e| io(&packages, e))? {
        let target = target.map_err(|e| io(&packages, e))?.path();
        for package in std::fs::read_dir(&target).map_err(|e| io(&target, e))? {
            let package = package.map_err(|e| io(&target, e))?.path();
            let rel = package.strip_prefix(bundle).unwrap_or(&package);
            let dest = to.join(rel).join("src");
            std::fs::create_dir_all(&dest).map_err(|e| io(&dest, e))?;
            let lib = package.join("src").join("lib.rs");
            std::fs::copy(&lib, dest.join("lib.rs")).map_err(|e| io(&lib, e))?;
            // The package's own manifest, whose edition a crate written
            // for the packages takes.
            let cargo = package.join("Cargo.toml");
            if cargo.is_file() {
                let to = to.join(rel).join("Cargo.toml");
                std::fs::copy(&cargo, to).map_err(|e| io(&cargo, e))?;
            }
        }
    }
    Ok(to.to_path_buf())
}

/// The diagnostics of a `lexlean` command result, each `code: message`.
fn diagnostics(result: &serde_json::Value) -> Vec<String> {
    result
        .get("diagnostics")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .map(|d| {
            format!(
                "{} {}",
                d.get("code").and_then(|c| c.as_str()).unwrap_or_default(),
                d.get("message")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
            )
        })
        .collect()
}

/// The first production root a diagnostic names, `<prefix><name>` in
/// backquotes, by its definition.
pub(crate) fn first_root(message: &str, prefix: &str) -> Option<String> {
    let at = message.find(&format!("`{prefix}"))? + 1 + prefix.len();
    let rest = &message[at..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// Run `lexlean <args> --diagnostic-format json` in `dir`: its command
/// result, whatever its exit.
fn lexlean(dir: &Path, args: &[&str]) -> Result<serde_json::Value, UorustError> {
    run_lexlean(Path::new(LEXLEAN), dir, args)
}

/// [`lexlean`], with the compiler's `program`.
fn run_lexlean(
    program: &Path,
    dir: &Path,
    args: &[&str],
) -> Result<serde_json::Value, UorustError> {
    lexlean_until(program, dir, args, None)?
        .ok_or_else(|| UorustError::Tool(format!("lexlean {}: no result", args.join(" "))))
}

/// [`lexlean`], stopped with every process it started when `deadline`
/// passes first: `None` then. The stop is coreutils' `timeout`, which runs
/// the command in its own process group and kills that whole group, the
/// `lean` lexlean runs among it, at the first whole second past `deadline`;
/// the `lake` and `lean` that left the group are killed by their build root.
fn lexlean_until(
    program: &Path,
    dir: &Path,
    args: &[&str],
    deadline: Option<std::time::Instant>,
) -> Result<Option<serde_json::Value>, UorustError> {
    let tool =
        |e: String| UorustError::Tool(format!("{} {}: {e}", program.display(), args.join(" ")));
    let mut command = match deadline {
        Some(d) => {
            let left = d.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let mut c = std::process::Command::new("timeout");
            c.args(["--signal=KILL", &format!("{}s", left.as_secs() + 1)])
                .arg(program);
            c
        }
        None => std::process::Command::new(program),
    };
    let run = command
        .args(args)
        .args(["--diagnostic-format", "json"])
        .current_dir(dir)
        .output()
        .map_err(|e| tool(e.to_string()))?;
    if deadline.is_some_and(|d| std::time::Instant::now() >= d) && !run.status.success() {
        // lake and lean leave the group: each names the build root.
        let _ = std::process::Command::new("pkill")
            .args(["-KILL", "-f"])
            .arg(dir.join(".lexlean").join(""))
            .status();
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&run.stdout);
    let last = text.lines().last().unwrap_or_default();
    serde_json::from_str(last)
        .map(Some)
        .map_err(|e| tool(format!("{e}: {}", String::from_utf8_lossy(&run.stderr))))
}

fn module_prefix(projection: &Path) -> Result<String, UorustError> {
    let config = projection.join("lexlean.toml");
    let text = std::fs::read_to_string(&config).map_err(|e| io(&config, e))?;
    text.lines()
        .find_map(|l| l.strip_prefix("module_prefix = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .ok_or_else(|| UorustError::Malformed {
            path: config,
            reason: "no module_prefix".to_string(),
        })
}

/// One top-level declaration of a module's text: a definition's name, when
/// it is one, and its lines.
struct Chunk {
    name: Option<String>,
    text: String,
}

/// A module's text cut into its top-level declarations, in order; joined
/// again they are the text.
fn chunks(source: &str) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    let mut open = false;
    for line in source.lines() {
        // A definition on one line, as the MIR path writes its translations.
        if let Some(inner) = line.strip_prefix("definition{name: ") {
            let name = inner.split(',').next().unwrap_or_default().to_string();
            out.push(Chunk {
                name: Some(name),
                text: format!("{line}\n"),
            });
            continue;
        }
        if line == "definition{" {
            open = true;
            out.push(Chunk {
                name: None,
                text: String::new(),
            });
        } else if !open {
            out.push(Chunk {
                name: None,
                text: String::new(),
            });
        }
        let Some(chunk) = out.last_mut() else {
            continue;
        };
        if open && chunk.name.is_none() {
            if let Some(name) = line
                .strip_prefix("  name: ")
                .and_then(|n| n.strip_suffix(','))
            {
                chunk.name = Some(name.to_string());
            }
        }
        chunk.text.push_str(line);
        chunk.text.push('\n');
        if line == "}" {
            open = false;
        }
    }
    out
}

/// The definitions of a projection's modules, by qualified name
/// (`Module::name`), with their texts.
pub(crate) struct Definitions(BTreeMap<String, String>);

impl Definitions {
    /// The definitions of `modules`, by module name with their texts.
    pub(crate) fn of(modules: &BTreeMap<String, String>) -> Definitions {
        let mut defs = BTreeMap::new();
        for (module, text) in modules {
            for c in chunks(text) {
                if let Some(name) = c.name {
                    defs.insert(format!("{module}::{name}"), c.text);
                }
            }
        }
        Definitions(defs)
    }

    /// A definition's text.
    pub(crate) fn text(&self, def: &str) -> Option<&str> {
        self.0.get(def).map(String::as_str)
    }

    /// A definition's closure: itself and every definition its body
    /// reaches.
    pub(crate) fn closure(&self, start: &str) -> BTreeSet<String> {
        let mut reached: BTreeSet<String> = BTreeSet::new();
        let mut work = vec![start.to_string()];
        while let Some(def) = work.pop() {
            let Some(text) = self.0.get(&def) else {
                continue;
            };
            if !reached.insert(def.clone()) {
                continue;
            }
            let module = def.split("::").next().unwrap_or_default();
            for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            {
                let key = if token.contains("::") {
                    token.to_string()
                } else {
                    format!("{module}::{token}")
                };
                if self.0.contains_key(&key) && !reached.contains(&key) {
                    work.push(key);
                }
            }
        }
        reached
    }
}

/// The modules' texts, by module name, with every root (a definition of
/// `Behavior`) a production root and every definition a root reaches
/// executable, the roots found, and the roots refused with their causes.
#[cfg(test)]
pub(crate) fn mark(
    modules: &BTreeMap<String, String>,
    roots: &BTreeSet<&str>,
) -> (
    BTreeMap<String, String>,
    BTreeSet<String>,
    BTreeMap<String, String>,
) {
    Profile::default().mark(modules, roots)
}

impl Profile {
    /// The modules' texts, by module name, with every root (a definition
    /// of `Behavior`) a production root with this profile's effects and
    /// target, and every definition a root reaches executable; the roots
    /// found; and the roots refused with their causes.
    pub(crate) fn mark(
        &self,
        modules: &BTreeMap<String, String>,
        roots: &BTreeSet<&str>,
    ) -> (
        BTreeMap<String, String>,
        BTreeSet<String>,
        BTreeMap<String, String>,
    ) {
        let cut: BTreeMap<&String, Vec<Chunk>> =
            modules.iter().map(|(m, text)| (m, chunks(text))).collect();
        let definitions = Definitions::of(modules);
        let defs = &definitions.0;
        let closure = |start: &str| definitions.closure(start);
        let mut reached: BTreeSet<String> = BTreeSet::new();
        let mut refused: BTreeMap<String, String> = BTreeMap::new();
        for root in roots {
            let own = closure(&format!("{BEHAVIOR}::{root}"));
            // SPEC §17.13: an executable definition takes no function.
            if let Some(hof) = own
                .iter()
                .find(|d| defs.get(*d).is_some_and(|t| higher_order(t)))
            {
                refused.insert(
                (*root).to_string(),
                format!("reaches {hof}, a definition with a function parameter, which no production root may"),
            );
                continue;
            }
            reached.extend(own);
        }
        let mut found = BTreeSet::new();
        let mut out = BTreeMap::new();
        for (module, chunks) in cut {
            let mut text = String::new();
            for c in chunks {
                let key = c.name.as_ref().map(|n| format!("{module}::{n}"));
                let Some(key) = key.filter(|k| reached.contains(k)) else {
                    text.push_str(&c.text);
                    continue;
                };
                let root = module == BEHAVIOR
                    && c.name
                        .as_deref()
                        .is_some_and(|n| roots.contains(n) && !refused.contains_key(n));
                let mut fields = Vec::new();
                if !c.text.contains("executable: true") {
                    fields.push("executable: true".to_string());
                }
                if root {
                    fields.push(format!(
                        "production: {{effects: [{}], targets: [{}]}}",
                        self.effects.join(", "),
                        self.target
                    ));
                    found.insert(key.trim_start_matches(&format!("{BEHAVIOR}::")).to_string());
                }
                let body = c
                    .text
                    .trim_end_matches('\n')
                    .trim_end_matches('}')
                    .trim_end();
                if fields.is_empty() {
                    text.push_str(&c.text);
                } else if c.text.lines().count() == 1 {
                    text.push_str(&format!("{body}, {}}}\n", fields.join(", ")));
                } else {
                    text.push_str(&format!("{body},\n  {}\n}}\n", fields.join(",\n  ")));
                }
            }
            out.insert(module.clone(), text);
        }
        (out, found, refused)
    }
}

/// Whether a definition's text takes a function: a parameter of a `Fn` type.
fn higher_order(text: &str) -> bool {
    let Some(at) = text.find("parameters: [") else {
        return false;
    };
    let rest = &text[at..];
    let end = rest.find("result:").unwrap_or(rest.len());
    rest[..end].contains("(Fn ")
}

/// The file of the production project that lists the roots it could not
/// export, by κ, with their causes.
pub const REFUSALS: &str = "refusals.toml";

fn toml_quoted(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

/// The module whose definitions are the lifted functions.
const BEHAVIOR: &str = "Behavior";

fn copy_tree(from: &Path, to: &Path) -> Result<(), UorustError> {
    std::fs::create_dir_all(to).map_err(|e| io(to, e))?;
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .map_err(|e| io(from, e))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        // The build root is the verifier's, never the projection's.
        if name == ".lexlean" || name == ".lake" {
            continue;
        }
        let target = to.join(&name);
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            std::fs::copy(&path, &target).map_err(|e| io(&path, e))?;
        }
    }
    Ok(())
}

/// One certified root of the manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certified {
    /// The Rust function's crate path.
    pub function: String,
    /// Its κ.
    pub kappa: String,
    /// The definition it exports.
    pub definition: String,
    /// The package's name.
    pub package: String,
    /// The SHA-256 of the package's `src/lib.rs`, as the bundle records it.
    pub source: String,
    /// The kernel's status of its certificates.
    pub certification: String,
    /// The closure κ its package was exported at ([`closure_kappas`]); a
    /// manifest written before closure κs has none, and its roots are built
    /// again by the next export.
    pub closure: Option<String>,
    /// The Rust edition its package's Cargo manifest declares; a root
    /// without one is built again rather than carried.
    pub edition: Option<String>,
}

/// The certified translator: the bundle it came from, its roots, and the
/// figures of the bootstrap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    /// The bundle's result id.
    pub result: String,
    /// The LexLean compiler's version.
    pub compiler: String,
    /// The project identity ([`project_identity`]) the export verified,
    /// when it verified one.
    pub project: Option<String>,
    /// The certified roots, by package.
    pub roots: Vec<Certified>,
    /// The figures, as [`Share`] computes them.
    pub share: Share,
    /// The roots the production project could not export, by κ, with
    /// their causes.
    pub refused: BTreeMap<String, String>,
    /// The build seconds the export spent per root it published, over all
    /// its passes and shards: what the next export sizes its shards by.
    pub seconds_per_root: Option<u64>,
}

/// The figures of the bootstrap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Share {
    /// Translator functions in the register.
    pub translator: usize,
    /// Those certified.
    pub certified: usize,
    /// Translator functions reachable from [`ENTRY`].
    pub reachable: usize,
    /// Those certified.
    pub reachable_certified: usize,
}

impl Manifest {
    /// The κ of every certified root.
    pub fn kappas(&self) -> BTreeSet<String> {
        self.roots.iter().map(|r| r.kappa.clone()).collect()
    }

    /// Read a manifest; an absent file is the empty manifest.
    pub fn read(path: &Path) -> Result<Manifest, UorustError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Manifest::default()),
            Err(e) => return Err(io(path, e)),
        };
        Manifest::parse(path, &text)
    }

    /// Parse a manifest's text.
    pub fn parse(path: &Path, text: &str) -> Result<Manifest, UorustError> {
        let bad = |reason: String| UorustError::Malformed {
            path: path.to_path_buf(),
            reason,
        };
        let doc: toml::Value = toml::from_str(text).map_err(|e| bad(format!("{e}")))?;
        if doc.get("schema").and_then(toml::Value::as_str) != Some(SCHEMA) {
            return Err(bad(format!("not a {SCHEMA} manifest")));
        }
        let text_of = |v: &toml::Value, k: &str| {
            v.get(k)
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| bad(format!("no {k}")))
        };
        let count = |k: &str| {
            doc.get("share")
                .and_then(|s| s.get(k))
                .and_then(toml::Value::as_integer)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| bad(format!("no share.{k}")))
        };
        let mut roots = Vec::new();
        for row in doc
            .get("root")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            roots.push(Certified {
                function: text_of(row, "function")?,
                kappa: text_of(row, "kappa")?,
                definition: text_of(row, "definition")?,
                package: text_of(row, "package")?,
                source: text_of(row, "source")?,
                certification: text_of(row, "certification")?,
                // An export that kept the key under its first name,
                // `identity`, is read as it wrote it.
                closure: ["closure", "identity"]
                    .iter()
                    .find_map(|k| row.get(*k).and_then(toml::Value::as_str))
                    .map(str::to_string),
                edition: row
                    .get("edition")
                    .and_then(toml::Value::as_str)
                    .map(str::to_string),
            });
        }
        let mut refused = BTreeMap::new();
        for row in doc
            .get("refused")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            refused.insert(text_of(row, "kappa")?, text_of(row, "cause")?);
        }
        Ok(Manifest {
            seconds_per_root: doc
                .get("timing")
                .and_then(|t| t.get("seconds_per_root"))
                .and_then(toml::Value::as_integer)
                .and_then(|n| u64::try_from(n).ok()),
            refused,
            result: text_of(&doc, "result")?,
            compiler: text_of(&doc, "compiler")?,
            project: doc
                .get("project")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            roots,
            share: Share {
                translator: count("translator")?,
                certified: count("certified")?,
                reachable: count("reachable")?,
                reachable_certified: count("reachable_certified")?,
            },
        })
    }

    /// The manifest's text, of the translator's own export.
    pub fn to_text(&self) -> String {
        self.text(&Profile::default())
    }

    /// The manifest's text, of an export under `profile`: the translator's
    /// own states no profile, any other states its own in `[profile]`.
    pub fn text(&self, profile: &Profile) -> String {
        let s = &self.share;
        let own = *profile == Profile::default();
        let mut out = String::new();
        out.push_str(match (own, profile.kind) {
            (true, _) => {
                "# The certified translator, written by `uorust certify sync` (just certified):\n\
                 # every lifted translator function exported by `lexlean production` and\n\
                 # kernel-checked, keyed by the κ of the Rust function it certifies.\n"
            }
            (false, Kind::Functions) => {
                "# The certified implementation, written by `uorust certify sync`: every\n\
                 # lifted function in scope exported by `lexlean production` and\n\
                 # kernel-checked, keyed by the κ of the Rust function it certifies.\n"
            }
            (false, Kind::Cases) => {
                "# The certified cases, written by `uorust certify sync`: every closed\n\
                 # assertion of the workspace's tests over a lifted function in scope,\n\
                 # exported by `lexlean production` and kernel-checked, keyed by the κ of\n\
                 # the definitions it runs.\n"
            }
        });
        out.push_str(&format!("schema = \"{SCHEMA}\"\n"));
        out.push_str(&format!("result = \"{}\"\n", self.result));
        out.push_str(&format!("compiler = \"{}\"\n", self.compiler));
        if let Some(project) = &self.project {
            out.push_str(&format!("project = \"{project}\"\n"));
        }
        if let Some(entry) = &profile.entry {
            out.push_str(&format!("entry = \"{entry}\"\n"));
        }
        if !own {
            out.push_str("\n# What the export is of.\n[profile]\n");
            out.push_str(&profile.rows());
        }
        out.push_str(if own {
            "\n# The bootstrap: the share of the translator that runs certified.\n"
        } else {
            "\n# The share that is certified: `translator` counts what is in scope.\n"
        });
        out.push_str("[share]\n");
        out.push_str(&format!("translator = {}\n", s.translator));
        out.push_str(&format!("certified = {}\n", s.certified));
        out.push_str(&format!("reachable = {}\n", s.reachable));
        out.push_str(&format!(
            "reachable_certified = {}\n",
            s.reachable_certified
        ));
        if let Some(n) = self.seconds_per_root {
            out.push_str(
                "\n# The build seconds per published root, which sizes the next export's shards.\n",
            );
            out.push_str(&format!("[timing]\nseconds_per_root = {n}\n"));
        }
        for r in &self.roots {
            out.push_str("\n[[root]]\n");
            out.push_str(&format!("function = \"{}\"\n", r.function));
            out.push_str(&format!("kappa = \"{}\"\n", r.kappa));
            out.push_str(&format!("definition = \"{}\"\n", r.definition));
            out.push_str(&format!("package = \"{}\"\n", r.package));
            out.push_str(&format!("source = \"{}\"\n", r.source));
            out.push_str(&format!("certification = \"{}\"\n", r.certification));
            if let Some(closure) = &r.closure {
                out.push_str(&format!("closure = \"{closure}\"\n"));
            }
            if let Some(edition) = &r.edition {
                out.push_str(&format!("edition = \"{edition}\"\n"));
            }
        }
        for (kappa, cause) in &self.refused {
            out.push_str("\n[[refused]]\n");
            out.push_str(&format!("kappa = \"{kappa}\"\n"));
            out.push_str(&format!("cause = {}\n", toml_quoted(cause)));
        }
        out
    }
}

/// The bundles [`production_build`] kept under `kept`, in the order it
/// built them: each directory `kept/<n>` that holds a `result.json`.
fn bundles(kept: &Path) -> Result<Vec<PathBuf>, UorustError> {
    let mut out: Vec<(usize, PathBuf)> = std::fs::read_dir(kept)
        .map_err(|e| io(kept, e))?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let n = e.file_name().to_str()?.parse::<usize>().ok()?;
            Some((n, e.path()))
        })
        .filter(|(_, p)| p.join("result.json").is_file())
        .collect();
    out.sort();
    Ok(out.into_iter().map(|(_, p)| p).collect())
}

/// What a sync takes in and where: the kept bundles, the directory of the
/// certified crate, the register its roots are current under, and what only
/// some exports have.
#[derive(Clone, Copy, Debug)]
pub struct Sync<'a> {
    /// What `certify production` kept, whole or by shard.
    pub kept: &'a Path,
    /// The certified crate's directory, which the caller names.
    pub dir: &'a Path,
    /// The register the roots are current under.
    pub register: &'a Register,
    /// The projection of that register: a cases export's roots are read
    /// from it.
    pub projection: Option<&'a Path>,
    /// The adapters' module that is written, when the workspace runs its
    /// certified packages in place of its functions.
    pub adapters: Option<&'a Path>,
    /// The workspace and its MIR dumps, for the adapters' types and for the
    /// share reachable from the profile's entry.
    pub mir: Option<(&'a Path, &'a Path)>,
}

/// Take the bundles kept under `kept` into `dir`: each certified root's
/// `src/lib.rs` as `dir/roots/<package>.rs` (checked against the SHA-256 its
/// bundle's result records), the crate that compiles them (with its Cargo
/// manifest, when the directory has none), and the manifest, with the share
/// over `register` and the call graph of `dumps`. A package is taken only
/// while its root's κ is the one it was exported at, so a function changed
/// since the export is uncertified until its package is rebuilt.
pub(crate) fn sync(
    profile: &Profile,
    sync: &Sync<'_>,
    roots: &[Root],
    dumps: Option<&Dumps>,
    compiler: &str,
) -> Result<Manifest, UorustError> {
    let Sync {
        kept,
        dir,
        register,
        adapters,
        ..
    } = *sync;
    let bad = |path: &Path, reason: String| UorustError::Malformed {
        path: path.to_path_buf(),
        reason,
    };
    // Refused before anything is written: not what `certify production`
    // keeps, which always holds the refusals and each bundle's result.json.
    let outputs = kept_roots(kept)?;
    if !outputs.iter().any(|k| k.join(REFUSALS).is_file()) {
        return Err(bad(
            kept,
            format!("no {REFUSALS} beside a bundle's result.json: not kept by certify production"),
        ));
    }
    let mut measured: Option<toml::Table> = None;
    let mut identified = toml::Table::new();
    let mut all = Vec::new();
    for k in &outputs {
        let kappas = k.join(KAPPAS);
        if let Ok(text) = std::fs::read_to_string(&kappas) {
            let table: toml::Table =
                toml::from_str(&text).map_err(|e| bad(&kappas, format!("{e}")))?;
            measured.get_or_insert_with(toml::Table::new).extend(table);
        }
        let ids = k.join(CLOSURES);
        if let Ok(text) = std::fs::read_to_string(&ids) {
            let table: toml::Table =
                toml::from_str(&text).map_err(|e| bad(&ids, format!("{e}")))?;
            identified.extend(table);
        }
        all.extend(bundles(k)?);
    }
    let mut results = Vec::new();
    for bundle in all {
        let path = bundle.join("result.json");
        let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        let result: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| bad(&path, e.to_string()))?;
        results.push((bundle, path, result));
    }
    let field = |v: &serde_json::Value, keys: &[&str]| {
        let mut at = v;
        for k in keys {
            at = at.get(k).unwrap_or(&serde_json::Value::Null);
        }
        at.as_str().unwrap_or_default().to_string()
    };
    let by_package: BTreeMap<String, &Root> = roots.iter().map(|r| (r.package(), r)).collect();
    let roots_dir = dir.join("roots");
    if roots_dir.is_dir() {
        std::fs::remove_dir_all(&roots_dir).map_err(|e| io(&roots_dir, e))?;
    }
    std::fs::create_dir_all(&roots_dir).map_err(|e| io(&roots_dir, e))?;
    let mut certified = Vec::new();
    for (bundle, path, result) in &results {
        let (bundle, path) = (bundle.as_path(), path.as_path());
        for root in result
            .get("roots")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
        {
            for output in root
                .get("outputs")
                .and_then(|o| o.as_array())
                .into_iter()
                .flatten()
            {
                let package = output.get("package").unwrap_or(&serde_json::Value::Null);
                let directory = field(package, &["directory"]);
                let name = directory.rsplit('/').next().unwrap_or_default().to_string();
                let Some(own) = by_package.get(&name) else {
                    if measured.is_some() {
                        continue;
                    }
                    return Err(bad(path, format!("{name} is no root of the register")));
                };
                // A function changed since the export is a new question.
                if measured.as_ref().is_some_and(|m| {
                    m.get(&name).and_then(toml::Value::as_str) != Some(own.kappa.as_str())
                }) {
                    continue;
                }
                let lib = format!("{directory}/src/lib.rs");
                let recorded = package
                    .get("files")
                    .and_then(|f| f.as_array())
                    .into_iter()
                    .flatten()
                    .find(|f| field(f, &["path"]) == lib)
                    .map(|f| field(f, &["sha256"]))
                    .ok_or_else(|| bad(path, format!("{lib} is not in the result")))?;
                let source_path = bundle.join(&lib);
                let bytes = std::fs::read(&source_path).map_err(|e| io(&source_path, e))?;
                let digest = crate::project::sha(&bytes);
                if digest != recorded {
                    return Err(bad(
                        &source_path,
                        format!("SHA-256 {digest}, the result records {recorded}"),
                    ));
                }
                let target = roots_dir.join(format!("{name}.rs"));
                std::fs::write(&target, &bytes).map_err(|e| io(&target, e))?;
                certified.push(Certified {
                    function: own.function.clone(),
                    kappa: own.kappa.clone(),
                    definition: own.definition.clone(),
                    closure: identified
                        .get(&name)
                        .and_then(toml::Value::as_str)
                        .map(str::to_string),
                    edition: package_edition(&bundle.join(&directory)),
                    package: name,
                    source: digest,
                    certification: field(output, &["certification", "status"]),
                });
            }
        }
    }
    let mut carried_results: Vec<String> = Vec::new();
    for k in &outputs {
        let list = k.join(CARRIED);
        let Ok(text) = std::fs::read_to_string(&list) else {
            continue;
        };
        let doc: toml::Value = toml::from_str(&text).map_err(|e| bad(&list, format!("{e}")))?;
        let rows = doc.get("carried").and_then(toml::Value::as_array);
        if rows.is_some_and(|r| !r.is_empty()) {
            carried_results.extend(
                doc.get("results")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_string),
            );
        }
        for row in rows.into_iter().flatten() {
            let text_of = |key: &str| {
                row.get(key)
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| bad(&list, format!("a carried package without {key}")))
            };
            let name = text_of("package")?;
            let Some(own) = by_package.get(&name) else {
                continue;
            };
            // As a built package: taken only while its function is the one
            // exported, and once.
            if measured.as_ref().is_some_and(|m| {
                m.get(&name).and_then(toml::Value::as_str) != Some(own.kappa.as_str())
            }) || certified.iter().any(|c| c.package == name)
            {
                continue;
            }
            let recorded = text_of("source")?;
            let source_path = k.join(CARRIED_DIR).join(format!("{name}.rs"));
            let bytes = std::fs::read(&source_path).map_err(|e| io(&source_path, e))?;
            let digest = crate::project::sha(&bytes);
            if digest != recorded {
                return Err(bad(
                    &source_path,
                    format!("SHA-256 {digest}, the prior manifest records {recorded}"),
                ));
            }
            let target = roots_dir.join(format!("{name}.rs"));
            std::fs::write(&target, &bytes).map_err(|e| io(&target, e))?;
            certified.push(Certified {
                function: own.function.clone(),
                kappa: own.kappa.clone(),
                definition: own.definition.clone(),
                package: name,
                source: digest,
                certification: text_of("certification")?,
                closure: Some(text_of("closure")?),
                edition: Some(text_of("edition")?),
            });
        }
    }
    certified.sort_by(|a, b| a.package.cmp(&b.package));
    // The edition the packages are written for, built or carried alike.
    let edition = certified.iter().find_map(|c| c.edition.clone());
    // The project every shard verified, when they state one and agree.
    let mut verified: BTreeSet<String> = BTreeSet::new();
    for k in &outputs {
        let path = k.join(PROJECT);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let table: toml::Table = toml::from_str(&text).map_err(|e| bad(&path, format!("{e}")))?;
        verified.extend(
            table
                .get("identity")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        );
    }
    let project = match verified.len() {
        1 => verified.pop_first(),
        _ => None,
    };
    let lib = profile.crate_source(&certified);
    let lib_path = dir.join("src").join("lib.rs");
    std::fs::create_dir_all(dir.join("src")).map_err(|e| io(dir, e))?;
    std::fs::write(&lib_path, lib).map_err(|e| io(&lib_path, e))?;
    // The crate's Cargo manifest is the caller's when the directory has
    // one (the translator's inherits its workspace's); else it is written,
    // a package of its own that any directory builds.
    let cargo = dir.join("Cargo.toml");
    if !cargo.exists() {
        let text = profile.cargo_manifest(edition.as_deref());
        std::fs::write(&cargo, text).map_err(|e| io(&cargo, e))?;
    }
    if let (Some(adapters), Some(dumps)) = (adapters, dumps) {
        let empty = BTreeMap::new();
        let mut wired = BTreeMap::new();
        for c in &certified {
            let source = std::fs::read_to_string(roots_dir.join(format!("{}.rs", c.package)))
                .map_err(|e| io(&roots_dir, e))?;
            let (krate, path) = c.function.split_once("::").unwrap_or(("", &c.function));
            let bodies = dumps.crates.get(krate).unwrap_or(&empty);
            let adapted = bodies
                .get(path)
                .and_then(|b| profile.adapter(c, b, &source));
            if let Some((name, code)) = adapted {
                wired.insert(name, code);
            }
        }
        let mut sites = BTreeMap::new();
        if let Some(src) = adapters.parent() {
            call_sites(src, adapters, &mut sites)?;
        }
        let text = adapters_source(&wired, &sites);
        std::fs::write(adapters, text).map_err(|e| io(adapters, e))?;
    }
    let kappas: BTreeSet<String> = certified.iter().map(|c| c.kappa.clone()).collect();
    let mut refused: BTreeMap<String, String> = BTreeMap::new();
    let (mut seconds, mut built) = (0u64, 0u64);
    for k in &outputs {
        let refusals = k.join(REFUSALS);
        let Ok(text) = std::fs::read_to_string(&refusals) else {
            continue;
        };
        let doc: toml::Value = toml::from_str(&text).map_err(|e| bad(&refusals, format!("{e}")))?;
        refused.extend(
            doc.get("refused")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| {
                    Some((
                        r.get("kappa")?.as_str()?.to_string(),
                        r.get("cause")?.as_str()?.to_string(),
                    ))
                }),
        );
        let timing: toml::Table = std::fs::read_to_string(k.join(TIMING))
            .ok()
            .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
            .unwrap_or_default();
        let count = |key: &str| {
            timing
                .get(key)
                .and_then(toml::Value::as_integer)
                .and_then(|n| u64::try_from(n).ok())
                .unwrap_or_default()
        };
        seconds += count("seconds");
        built += count("roots");
    }
    // A root a shard built is not refused by another shard's view of it.
    refused.retain(|kappa, _| !kappas.contains(kappa));
    let manifest = Manifest {
        refused,
        // The bundles this export published, then those the carried
        // packages were published in.
        result: {
            let mut ids: Vec<String> = results
                .iter()
                .map(|(_, _, r)| field(r, &["result_id"]))
                .collect();
            for id in carried_results {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            ids.join(" ")
        },
        // As `lexlean --version` states it, which the next export compares
        // its own against: a manifest of no bundle has it too.
        compiler: compiler.to_string(),
        project,
        seconds_per_root: seconds.checked_div(built),
        roots: certified,
        share: match profile.kind {
            Kind::Functions => profile.share(register, &kappas, dumps),
            // Of a cases export, the cases the projection states and those
            // whose package is current.
            Kind::Cases => Share {
                translator: roots.len(),
                certified: roots.iter().filter(|r| kappas.contains(&r.kappa)).count(),
                ..Share::default()
            },
        },
    };
    let manifest_path = dir.join("manifest.toml");
    std::fs::write(&manifest_path, manifest.text(profile)).map_err(|e| io(&manifest_path, e))?;
    Ok(manifest)
}

/// The edition a bundle's package declares in its Cargo manifest.
fn package_edition(package: &Path) -> Option<String> {
    let text = std::fs::read_to_string(package.join("Cargo.toml")).ok()?;
    let doc: toml::Value = toml::from_str(&text).ok()?;
    doc.get("package")?
        .get("edition")?
        .as_str()
        .map(str::to_string)
}

/// The profile every kept output under `kept` recorded: one export has one.
pub fn kept_profile(kept: &Path) -> Result<Profile, UorustError> {
    let mut found: Option<Profile> = None;
    for k in kept_roots(kept)? {
        let profile = Profile::kept(&k)?;
        if found.as_ref().is_some_and(|f| *f != profile) {
            return Err(UorustError::Malformed {
                path: k.join(PROFILE),
                reason: "the shards were exported under different profiles".to_string(),
            });
        }
        found = Some(profile);
    }
    Ok(found.unwrap_or_default())
}

/// [`sync`] of the bundles [`production_build`] kept, under the profile the
/// export recorded there, with the dumps read when the sync names them.
/// Returns the profile and the manifest.
pub fn sync_paths(s: &Sync<'_>) -> Result<(Profile, Manifest), UorustError> {
    let profile = kept_profile(s.kept)?;
    let roots = match (profile.kind, s.projection) {
        (Kind::Functions, _) => profile.roots(s.register),
        (Kind::Cases, Some(projection)) => profile.export_roots(projection, s.register)?,
        (Kind::Cases, None) => {
            return Err(UorustError::Tool(
                "a cases export is taken in against the projection its cases are read from: pass --projection"
                    .to_string(),
            ))
        }
    };
    let dumps = match s.mir {
        Some((workspace, mir)) => Some(crate::mir::load(workspace, mir)?),
        None => None,
    };
    let manifest = sync(&profile, s, &roots, dumps.as_ref(), &compiler_version()?)?;
    Ok((profile, manifest))
}

/// The adapters' module, `crates/uorust/src/certified.rs`: the switch
/// between the certified and the hand-written translator, the macro a
/// hand-written function runs its certified realization through, one adapter
/// per certified root whose types convert, and a stub answering `None` for
/// each call site whose root has no adapter at this commit, so that a root
/// leaving the certified set never breaks the build.
pub(crate) fn adapters_source(
    adapters: &BTreeMap<String, String>,
    sites: &BTreeMap<String, usize>,
) -> String {
    let mut out = String::from(ADAPTERS_HEAD);
    for a in adapters.values() {
        out.push('\n');
        out.push_str(a);
    }
    for (name, arity) in sites {
        if adapters.contains_key(name) {
            continue;
        }
        let params: Vec<String> = (0..*arity).map(|i| format!("A{i}: ?Sized")).collect();
        let args: Vec<String> = (0..*arity).map(|i| format!("_: &A{i}")).collect();
        out.push_str(&format!(
            "\n/// Not certified at this commit: the hand-written body runs.\npub(crate) fn {name}<{}R>({}) -> Option<R> {{\n    None\n}}\n",
            params.iter().map(|p| format!("{p}, ")).collect::<String>(),
            args.join(", ")
        ));
    }
    out.push_str(ADAPTERS_TESTS);
    out
}

/// The `certified!(adapter, args..)` call sites under `dir`, by adapter,
/// with their argument counts; the adapters' module itself is skipped.
fn call_sites(
    dir: &Path,
    adapters: &Path,
    out: &mut BTreeMap<String, usize>,
) -> Result<(), UorustError> {
    const SITE: &str = "certified!(";
    let entries = std::fs::read_dir(dir).map_err(|e| io(dir, e))?;
    let mut paths: Vec<PathBuf> = entries
        .map(|e| e.map(|e| e.path()).map_err(|e| io(dir, e)))
        .collect::<Result<_, UorustError>>()?;
    paths.sort();
    for path in paths {
        if path.is_dir() {
            call_sites(&path, adapters, out)?;
            continue;
        }
        let own = path.file_name().and_then(|n| n.to_str()) == Some("certify.rs");
        if path == adapters || own || path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        for (at, _) in text.match_indices(SITE) {
            let rest = &text[at + SITE.len()..];
            let mut depth = 1usize;
            let end = rest.char_indices().find_map(|(i, c)| {
                match c {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(i)
            });
            let Some(end) = end else { continue };
            let parts: Vec<String> = split_top_level(&rest[..end])
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            if let Some((name, args)) = parts.split_first() {
                out.insert(name.clone(), args.len());
            }
        }
    }
    Ok(())
}

const ADAPTERS_HEAD: &str = r#"//! The certified translator's adapters, written by `uorust certify sync`
//! (`just certified`): for each certified root whose parameter and result
//! types convert, a function over references to the hand-written one's
//! arguments that runs the certified package (crates/certified) on them. A
//! hand-written function runs its certified realization through
//! [`certified!`], its first statement; a site whose root this commit does
//! not certify has a stub that answers `None`, so the hand-written body runs.
//! `--translator hand` runs every hand-written body, the differential's second
//! path. A certified root that overflows its target's width stops with a
//! panic, which the differential sees as a disagreement.

// The adapters are written from the certified packages' signatures: an
// adapter is named after its root's Rust path and wraps its arguments as the
// package's `run` takes them.
#![allow(dead_code, non_snake_case, unused_parens)]

use std::sync::atomic::{AtomicBool, Ordering};

static HAND: AtomicBool = AtomicBool::new(false);

/// Run the hand-written translator in place of the certified one: the
/// differential's second path.
pub fn use_hand(hand: bool) {
    HAND.store(hand, Ordering::Relaxed);
}

/// Whether the hand-written translator runs.
pub(crate) fn hand() -> bool {
    HAND.load(Ordering::Relaxed)
}

/// `certified!(adapter, args...)`, a hand-written function's first
/// statement under `#[cfg(not(uorust_lift))]`: return what the adapter
/// answers, unless the hand-written translator runs. The lift's dump sets
/// `uorust_lift`, so the self register lifts the hand-written body alone.
macro_rules! certified {
    ($adapter:ident $(, $arg:expr)* $(,)?) => {
        if !$crate::certified::hand() {
            if let Some(v) = $crate::certified::$adapter($(&$arg),*) {
                return v;
            }
        }
    };
}
pub(crate) use certified;
"#;

const ADAPTERS_TESTS: &str = r#"
#[cfg(test)]
mod tests {
    // The switch the differential runs the hand-written translator through.
    #[test]
    fn use_hand_selects_the_hand_written_translator() {
        super::use_hand(true);
        assert!(super::hand());
        super::use_hand(false);
        assert!(!super::hand());
    }
}
"#;

/// The adapter of a certified root over its hand-written function's body:
/// none when a parameter or the result has no conversion.
#[cfg(test)]
pub(crate) fn adapter(root: &Certified, body: &IrBody, source: &str) -> Option<(String, String)> {
    Profile::default().adapter(root, body, source)
}

impl Profile {
    /// The adapter of a root certified under this profile over its function's
    /// body, calling its module of the profile's certified crate and named by
    /// the function's path in its own crate: none when a parameter or the
    /// result has no conversion.
    pub(crate) fn adapter(
        &self,
        root: &Certified,
        body: &IrBody,
        source: &str,
    ) -> Option<(String, String)> {
        let (params, result) = run_signature(source)?;
        if params.len() != body.arity {
            return None;
        }
        let m = format!("{}::{}", self.crate_name(), self.module(&root.package));
        let mut args = Vec::new();
        let mut converted = Vec::new();
        for (i, cert) in params.iter().enumerate() {
            let rust = unlifetimed(body.locals.get(i + 1)?);
            // Each argument comes by reference, so the hand-written body still
            // owns it when the root is not certified.
            args.push(format!("p{i}: &{rust}"));
            converted.push(if rust.starts_with('&') {
                to_cert(&format!("(*p{i})"), &rust, cert, &m)?
            } else {
                to_cert(&format!("p{i}"), &format!("&{rust}"), cert, &m)?
            });
        }
        let rust_result = unlifetimed(body.locals.first()?);
        let call = format!("{m}::run({})", converted.join(", "));
        let value = match result.strip_prefix("R<").and_then(|r| r.strip_suffix('>')) {
            Some(inner) => {
                let overflow = format!(
                "match {call} {{ Ok(v) => v, Err(_) => panic!(\"{}: the certified root overflowed\") }}",
                root.function
            );
                from_cert(&overflow, inner, &rust_result)?
            }
            None => from_cert(&call, &result, &rust_result)?,
        };
        let name = root
            .function
            .split_once("::")
            .map_or(root.function.as_str(), |(_, path)| path)
            .replace("::", "_");
        let code = format!(
        "/// `{}`, certified as `Behavior::{}`.\n#[allow(clippy::all, non_snake_case, unused_parens)]\npub(crate) fn {name}({}) -> Option<{rust_result}> {{\n    Some({value})\n}}\n",
        root.function,
        root.definition,
        args.join(", ")
    );
        Some((name, code))
    }
}

/// A type as MIR prints it, without its lifetimes.
fn unlifetimed(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            while chars
                .peek()
                .is_some_and(|n| n.is_alphanumeric() || *n == '_')
            {
                chars.next();
            }
            if chars.peek() == Some(&' ') {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The parameter and result types of a package's `run`.
fn run_signature(source: &str) -> Option<(Vec<String>, String)> {
    let at = source.find("pub fn run(")? + "pub fn run(".len();
    let rest = &source[at..];
    let close = rest.find(") -> ")?;
    let params: Vec<String> = split_top_level(&rest[..close])
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            p.split_once(':')
                .map(|(_, t)| t.trim().to_string())
                .unwrap_or_default()
        })
        .collect();
    let result = rest[close + ") -> ".len()..]
        .split('{')
        .next()?
        .trim()
        .to_string();
    Some((params, result))
}

fn split_top_level(text: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(last) = out.last_mut() {
            last.push(c);
        }
    }
    out
}

/// The generic argument of `outer<..>` in a type.
fn argument<'a>(text: &'a str, outer: &str) -> Option<&'a str> {
    text.strip_prefix(outer)?
        .strip_prefix('<')?
        .strip_suffix('>')
}

const NATURALS: &[&str] = &["u8", "u16", "u32", "u64", "usize"];

/// The expression converting the Rust value `e` of `rust` to the package's
/// `cert`.
fn to_cert(e: &str, rust: &str, cert: &str, m: &str) -> Option<String> {
    let rust = rust.trim();
    if let Some(inner) = rust.strip_prefix('&') {
        let inner = inner.trim_start_matches("mut ").trim();
        return match inner {
            "str" | "std::string::String" if cert == "Str" => Some(format!("{m}::Str::lit({e})")),
            n if NATURALS.contains(&n) && cert == "u64" => Some(format!(
                "u64::try_from(*{e}).expect(\"a natural of 64 bits\")"
            )),
            "bool" if cert == "bool" => Some(format!("*{e}")),
            _ => {
                let elem = inner
                    .strip_prefix('[')
                    .and_then(|t| t.strip_suffix(']'))
                    .or_else(|| argument(inner, "std::vec::Vec"))?;
                let item = argument(cert, "List")?;
                let each = to_cert("x", &format!("&{elem}"), item, m)?;
                Some(format!(
                    "{{ let mut l = {m}::List::nil(); for x in {e}.iter().rev() {{ l = {m}::List::cons({each}, l); }} l }}"
                ))
            }
        };
    }
    match rust {
        "std::string::String" if cert == "Str" => Some(format!("{m}::Str::lit(&{e})")),
        n if NATURALS.contains(&n) && cert == "u64" => Some(format!(
            "u64::try_from({e}).expect(\"a natural of 64 bits\")"
        )),
        "bool" if cert == "bool" => Some(e.to_string()),
        _ => {
            if let (Some(t), Some(c)) = (
                argument(rust, "std::option::Option"),
                argument(cert, "Option"),
            ) {
                let each = to_cert("x", t, c, m)?;
                return Some(format!("{e}.map(|x| {each})"));
            }
            let elem = argument(rust, "std::vec::Vec")?;
            let item = argument(cert, "List")?;
            let each = to_cert("x", elem, item, m)?;
            Some(format!(
                "{{ let mut l = {m}::List::nil(); for x in {e}.into_iter().rev() {{ l = {m}::List::cons({each}, l); }} l }}"
            ))
        }
    }
}

/// The expression converting the package's value `e` of `cert` to the Rust
/// `rust`, which owns its value.
fn from_cert(e: &str, cert: &str, rust: &str) -> Option<String> {
    let rust = rust.trim();
    match rust {
        "std::string::String" if cert == "Str" => Some(format!("{e}.text().to_string()")),
        n if NATURALS.contains(&n) && cert == "u64" => Some(format!(
            "{n}::try_from({e}).expect(\"a natural the hand-written type holds\")"
        )),
        "bool" if cert == "bool" => Some(e.to_string()),
        _ => {
            if let (Some(t), Some(c)) = (
                argument(rust, "std::option::Option"),
                argument(cert, "Option"),
            ) {
                let each = from_cert("x", c, t)?;
                return Some(format!("{e}.map(|x| {each})"));
            }
            let elem = argument(rust, "std::vec::Vec")?;
            let item = argument(cert, "List")?;
            let each = from_cert("h", item, elem)?;
            Some(format!(
                "{{ let mut v = Vec::new(); let mut c = {e}; while let Some((h, t)) = c.uncons() {{ v.push({each}); c = t; }} v }}"
            ))
        }
    }
}

impl Profile {
    /// The certified crate's `lib.rs`: one module per root, its file the
    /// package source byte for byte.
    pub(crate) fn crate_source(&self, roots: &[Certified]) -> String {
        let mut out = String::from(match (*self == Profile::default(), self.kind) {
            (true, _) => {
                "//! The certified translator: the Rust `lexlean production` exported from the\n\
                 //! lifted translator's definitions, one module per production root, each file\n\
                 //! the certified package's `src/lib.rs` byte for byte. Written by `uorust\n\
                 //! certify sync`; `crates/certified/manifest.toml` keys each by the κ of the\n\
                 //! Rust function it certifies.\n\n\
                 #![forbid(unsafe_code)]\n"
            }
            (false, Kind::Functions) => {
                "//! The certified implementation: the Rust `lexlean production` exported from\n\
                 //! the lifted definitions of the functions in scope, one module per\n\
                 //! production root, each file the certified package's `src/lib.rs` byte for\n\
                 //! byte. Written by `uorust certify sync`; `manifest.toml` keys each by the κ\n\
                 //! of the Rust function it certifies.\n\n\
                 #![forbid(unsafe_code)]\n"
            }
            (false, Kind::Cases) => {
                "//! The certified cases: the Rust `lexlean production` exported from the\n\
                 //! closed assertions of the workspace's tests, one module per production\n\
                 //! root, each file the certified package's `src/lib.rs` byte for byte.\n\
                 //! Written by `uorust certify sync`; `manifest.toml` keys each by the κ of\n\
                 //! the definitions it runs.\n\n\
                 #![forbid(unsafe_code)]\n"
            }
        });
        for r in roots {
            let named = match self.kind {
                Kind::Functions => format!("`{}`, certified as", r.function),
                Kind::Cases => format!("A case of `{}`:", r.function),
            };
            out.push_str(&format!(
                "\n/// {named} `Behavior::{}`.\n#[rustfmt::skip]\n#[allow(missing_docs, dead_code, clippy::all)]\n#[path = \"../roots/{}.rs\"]\npub mod {};\n",
                r.definition,
                r.package,
                self.module(&r.package)
            ));
        }
        out
    }

    /// The module of the certified crate a package is, by its name: the
    /// name without the profile's prefix.
    pub(crate) fn module<'a>(&self, package: &'a str) -> &'a str {
        package
            .strip_prefix(self.package.as_str())
            .unwrap_or(package)
    }

    /// The certified crate's name: the package prefix without the
    /// separator it ends with.
    pub(crate) fn crate_name(&self) -> String {
        self.package.trim_end_matches('_').to_string()
    }

    /// The Cargo manifest of a certified crate whose directory has none: a
    /// package and a workspace of its own, at the edition the bundle's
    /// packages declare, which builds wherever the directory is.
    pub(crate) fn cargo_manifest(&self, edition: Option<&str>) -> String {
        let edition = edition.map_or_else(String::new, |e| format!("edition = \"{e}\"\n"));
        format!(
            "# Written by `uorust certify sync` into a directory that had no Cargo\n\
             # manifest: the crate of the certified packages `manifest.toml` lists.\n\
             [package]\nname = \"{}\"\nversion = \"{PACKAGE_VERSION}\"\n{edition}publish = false\n\n\
             [lib]\npath = \"src/lib.rs\"\n\n[workspace]\n",
            self.crate_name()
        )
    }

    /// The figures of the bootstrap over `register`, a function being
    /// certified when its κ is in `certified`; the reachable ones are
    /// counted when the profile names an entry and the dumps are given.
    pub(crate) fn share(
        &self,
        register: &Register,
        certified: &BTreeSet<String>,
        dumps: Option<&Dumps>,
    ) -> Share {
        let reach = match (&self.entry, dumps) {
            (Some(entry), Some(dumps)) => reachable_from(entry, dumps),
            _ => BTreeSet::new(),
        };
        // (reachable, certified) of each function in scope; a root
        // certifies its κ only while the register lifts it.
        let rows: Vec<(bool, bool)> = register
            .rows
            .iter()
            .filter(|(_, r)| self.in_scope(&r.function))
            .map(|(k, r)| {
                (
                    reach.contains(&r.function),
                    certified.contains(k) && matches!(r.status, Status::Lifted(_)),
                )
            })
            .collect();
        Share {
            translator: rows.len(),
            certified: rows.iter().filter(|(_, c)| *c).count(),
            reachable: rows.iter().filter(|(r, _)| *r).count(),
            reachable_certified: rows.iter().filter(|(r, c)| *r && *c).count(),
        }
    }
}

/// The functions of its crate an entry reaches, by crate path, over the
/// calls, the closures and the function items of their MIR.
pub(crate) fn reachable_from(entry: &str, dumps: &Dumps) -> BTreeSet<String> {
    let krate = entry.split("::").next().unwrap_or_default();
    let empty = BTreeMap::new();
    let bodies = dumps.crates.get(krate).unwrap_or(&empty);
    let closures = dumps.closures.get(krate).unwrap_or(&empty);
    let prefix = format!("{krate}::");
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut work: Vec<&IrBody> = Vec::new();
    let mut closures_seen: BTreeSet<&str> = BTreeSet::new();
    if let Some(body) = bodies.get(entry.strip_prefix(&prefix).unwrap_or(entry)) {
        seen.insert(entry.to_string());
        work.push(body);
    }
    while let Some(body) = work.pop() {
        let (calls, made) = body_mentions(body);
        for call in calls {
            let path = callee_path(&call);
            if let Some(next) = bodies.get(&path) {
                if seen.insert(format!("{prefix}{path}")) {
                    work.push(next);
                }
            }
        }
        for ty in made {
            if let Some((key, next)) = closures.get_key_value(&ty) {
                if closures_seen.insert(key.as_str()) {
                    work.push(next);
                }
            }
        }
    }
    seen
}

/// A callee as MIR prints it, as the path its body is keyed by: generic
/// arguments erased, `<T as Trait>::m` the method of `T`.
pub(crate) fn callee_path(callee: &str) -> String {
    let erased = crate::mir::lower::erase(callee);
    if let Some(rest) = erased.strip_prefix('<') {
        if let Some((ty, tail)) = rest.split_once(" as ") {
            if let Some((_, method)) = tail.rsplit_once(">::") {
                return format!("{ty}::{method}");
            }
        }
    }
    erased
}

/// The callees, and the closures built, of a body.
fn body_mentions(body: &IrBody) -> (Vec<String>, Vec<String>) {
    let mut calls = Vec::new();
    let mut made = Vec::new();
    let operand = |o: &IrOperand, calls: &mut Vec<String>| {
        if let IrOperand::Constant(IrConstant::Path(p)) = o {
            calls.push(p.clone());
        }
    };
    for block in &body.blocks {
        for statement in &block.statements {
            if let IrStatement::Assign(_, rvalue) = statement {
                match rvalue {
                    IrRvalue::Use(o) | IrRvalue::Unary(_, o) | IrRvalue::Cast(_, o, _) => {
                        operand(o, &mut calls);
                    }
                    IrRvalue::Binary(_, a, b) => {
                        operand(a, &mut calls);
                        operand(b, &mut calls);
                    }
                    IrRvalue::Aggregate(kind, ops) => {
                        if let IrAggregate::Closure(ty) = kind {
                            made.push(ty.clone());
                        }
                        for (_, o) in ops {
                            operand(o, &mut calls);
                        }
                    }
                    _ => {}
                }
            }
        }
        if let IrTerminator::Call { callee, args, .. } = &block.terminator {
            calls.push(callee.clone());
            for a in args {
                operand(a, &mut calls);
            }
        }
    }
    (calls, made)
}

/// The domain tag of an output tree's κ.
pub const TREE: &str = "uorust/certified-output/1";

/// The κ of an output tree: SHA-256 over [`TREE`] and each file's path and
/// bytes, in path order, so two trees have one κ exactly when they are byte
/// for byte the same.
pub fn tree_kappa(dir: &Path) -> Result<String, UorustError> {
    let mut files = Vec::new();
    tree_files(dir, dir, &mut files)?;
    files.sort();
    let mut text = Vec::new();
    text.extend_from_slice(TREE.as_bytes());
    for (rel, path) in files {
        let bytes = std::fs::read(&path).map_err(|e| io(&path, e))?;
        text.extend_from_slice(format!("\n{rel}\n{}\n", bytes.len()).as_bytes());
        text.extend_from_slice(&bytes);
    }
    Ok(crate::project::sha(&text))
}

fn tree_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), UorustError> {
    for entry in std::fs::read_dir(dir).map_err(|e| io(dir, e))? {
        let path = entry.map_err(|e| io(dir, e))?.path();
        if path.is_dir() {
            tree_files(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            out.push((rel, path));
        }
    }
    Ok(())
}

/// The differential and the fixpoint over the outputs under `dir`: each
/// `<name>-certified` beside its `<name>-hand`. Returns one line per output,
/// `<name> <κ>`; refused, naming each, when a pair's κ differ or a pair is
/// incomplete.
pub fn agree(dir: &Path) -> Result<Vec<String>, UorustError> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(dir).map_err(|e| io(dir, e))? {
        let name = entry
            .map_err(|e| io(dir, e))?
            .file_name()
            .to_string_lossy()
            .into_owned();
        for suffix in ["-certified", "-hand"] {
            if let Some(base) = name.strip_suffix(suffix) {
                names.insert(base.to_string());
            }
        }
    }
    let mut lines = Vec::new();
    let mut disagree = Vec::new();
    for name in &names {
        let certified = dir.join(format!("{name}-certified"));
        let hand = dir.join(format!("{name}-hand"));
        if !certified.is_dir() || !hand.is_dir() {
            disagree.push(format!("{name}: one path wrote no output"));
            continue;
        }
        let (c, h) = (tree_kappa(&certified)?, tree_kappa(&hand)?);
        if c == h {
            lines.push(format!("{name} {c}"));
        } else {
            disagree.push(format!("{name}: certified κ {c}, hand-written κ {h}"));
        }
    }
    if names.is_empty() {
        disagree.push(format!("{}: no outputs", dir.display()));
    }
    if disagree.is_empty() {
        Ok(lines)
    } else {
        Err(UorustError::Disagree(disagree))
    }
}

/// The translator functions of `register` the manifest does not certify,
/// by function, each with its cause.
pub fn uncertified(register: &Register, manifest: &Manifest) -> BTreeMap<String, String> {
    Profile::default().uncertified(register, manifest)
}

/// The ratchet: the hand-written translator may only shrink. Refused when
/// more translator functions are uncertified under the head than under the
/// base, naming each that is uncertified under the head and not under the
/// base, with its cause. Returns the head's count.
pub fn ratchet(
    head: &Register,
    head_manifest: &Manifest,
    base: &Register,
    base_manifest: &Manifest,
) -> Result<usize, UorustError> {
    Profile::default().ratchet(head, head_manifest, base, base_manifest)
}

impl Profile {
    /// The functions of `register` in this profile's scope the manifest
    /// does not certify, by function, each with its cause.
    pub fn uncertified(
        &self,
        register: &Register,
        manifest: &Manifest,
    ) -> BTreeMap<String, String> {
        let certified = manifest.kappas();
        register
            .rows
            .iter()
            // A root certifies its κ only while the register lifts it.
            .filter(|(k, r)| {
                self.in_scope(&r.function)
                    && !(certified.contains(*k) && matches!(r.status, Status::Lifted(_)))
            })
            .map(|(k, r)| {
                let cause = match (&r.status, manifest.refused.get(k)) {
                    (Status::Lifted(d), Some(why)) => {
                        format!("lifted as {d}, not exported: {why}")
                    }
                    (Status::Lifted(d), None) => format!("lifted as {d}, not yet certified"),
                    (Status::Residue(reason, detail), _) => {
                        format!("residue ({reason}): {detail}")
                    }
                    (Status::Foreign(detail), _) => format!("foreign: {detail}"),
                    (Status::Effect(effect), _) => format!("effect: {effect}"),
                };
                (r.function.clone(), cause)
            })
            .collect()
    }

    /// The functions in scope the base certifies (a row of the function is
    /// lifted and its κ certified) that the head still has but lifts under
    /// no κ. A function the head lifts under a new κ was edited, and its
    /// certificate is owed to an export, so it is not lost; one the head no
    /// longer has was deleted.
    fn lost(&self, head: &Register, base: &Register, base_manifest: &Manifest) -> BTreeSet<String> {
        let certified = base_manifest.kappas();
        let rooted: BTreeSet<&str> = base
            .rows
            .iter()
            .filter(|(k, r)| {
                self.in_scope(&r.function)
                    && certified.contains(*k)
                    && matches!(r.status, Status::Lifted(_))
            })
            .map(|(_, r)| r.function.as_str())
            .collect();
        let mut present = BTreeSet::new();
        let mut lifted = BTreeSet::new();
        for r in head.rows.values() {
            present.insert(r.function.as_str());
            if matches!(r.status, Status::Lifted(_)) {
                lifted.insert(r.function.as_str());
            }
        }
        rooted
            .into_iter()
            .filter(|f| present.contains(f) && !lifted.contains(f))
            .map(String::from)
            .collect()
    }

    /// The ratchet over this profile's scope: what is not certified may
    /// only shrink. Refused first when a root the base certifies no longer
    /// lifts at the head (`lost`), however the count moves; then
    /// when more functions in scope are uncertified under the head than
    /// under the base, naming each that is uncertified under the head and
    /// not under the base, with its cause. Returns the head's count.
    pub fn ratchet(
        &self,
        head: &Register,
        head_manifest: &Manifest,
        base: &Register,
        base_manifest: &Manifest,
    ) -> Result<usize, UorustError> {
        let now = self.uncertified(head, head_manifest);
        let lost = self.lost(head, base, base_manifest);
        if !lost.is_empty() {
            // Not a growth an export can repay: the root no longer lifts.
            let mut rows = vec![format!(
                "{} certified roots of the parent no longer lift",
                lost.len()
            )];
            rows.extend(lost.iter().map(|f| {
                let cause = now.get(f).map_or("not lifted", String::as_str);
                format!("{f}: {cause}")
            }));
            return Err(UorustError::Grown(rows));
        }
        let before = self.uncertified(base, base_manifest);
        if now.len() <= before.len() {
            return Ok(now.len());
        }
        let mut rows = vec![format!(
            "{} uncertified {}, {} in the parent",
            now.len(),
            self.measured(),
            before.len()
        )];
        rows.extend(
            now.iter()
                .filter(|(f, _)| !before.contains_key(*f))
                .map(|(f, cause)| format!("{f}: {cause}")),
        );
        Err(UorustError::Grown(rows))
    }

    /// What the ratchet counts, in words: the translator's functions, or
    /// the functions in another workspace's scope.
    pub fn measured(&self) -> &'static str {
        if *self == Profile::default() {
            "translator functions"
        } else {
            "functions in scope"
        }
    }

    /// [`Profile::ratchet`] of a head measured under this profile against a
    /// base that was measured under `base_profile`, when its manifest
    /// states one: refused, before any count, when this scope leaves out a
    /// prefix the base measured, since a function taken out of the scope is
    /// hand-written and no longer counted.
    pub fn ratchet_from(
        &self,
        base_profile: Option<&Profile>,
        head: &Register,
        head_manifest: &Manifest,
        base: &Register,
        base_manifest: &Manifest,
    ) -> Result<usize, UorustError> {
        if self.kind != Kind::Functions {
            return Err(UorustError::Tool(
                "the ratchet measures functions: the manifest is of a cases export".to_string(),
            ));
        }
        let dropped = base_profile.map(|b| b.dropped_by(self)).unwrap_or_default();
        if !dropped.is_empty() {
            let mut rows = vec![format!(
                "the scope measures less than the base's: {} of its prefixes are left out",
                dropped.len()
            )];
            rows.extend(dropped.into_iter().map(|p| {
                let prefix = if p.is_empty() { "every function" } else { &p };
                format!("{prefix}: no longer in scope")
            }));
            return Err(UorustError::Grown(rows));
        }
        self.ratchet(head, head_manifest, base, base_manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(rows: &[(&str, &str, Status)]) -> Register {
        let mut r = Register::default();
        for (kappa, function, status) in rows {
            r.rows.insert(
                (*kappa).to_string(),
                crate::register::Row {
                    function: (*function).to_string(),
                    status: status.clone(),
                    divergence: None,
                },
            );
        }
        r
    }

    #[test]
    fn a_root_is_a_lifted_translator_function() {
        let r = register(&[
            (
                "a",
                "uorust::mir::coverage::binop",
                Status::Lifted("Behavior::binop".into()),
            ),
            (
                "b",
                "uorust::project::x",
                Status::Lifted("Behavior::x".into()),
            ),
            (
                "c",
                "uorust::lift::y",
                Status::Residue("mir".into(), "d".into()),
            ),
        ]);
        let roots = roots(&r);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].definition, "binop");
        assert_eq!(roots[0].package(), "uorust_certified_binop");
        assert_eq!(
            Root {
                definition: "mutTargetPart1".into(),
                ..roots[0].clone()
            }
            .package(),
            "uorust_certified_mut_target_part1"
        );
    }

    // Main's test of the closure κ (6d6b4e6), on the merged key: a root's
    // closure κ changes exactly with what its root reaches, and with the
    // compiler and the descriptors. A type no root reaches changes no root's
    // closure κ here (its package cannot differ, SPEC §24.7); it changes the
    // project's identity, without which no root is carried unverified.
    #[test]
    fn a_closure_kappa_changes_exactly_with_what_its_root_reaches() {
        let tmp = scratch("closure-kappa");
        let behavior = |c_body: &str, type_body: &str| {
            format!(
                "\\begin{{lexlean}}{{Behavior}}\n\\begin{{semanticmodule}}\ninductive{{\n  name: T,\n  constructors: [{type_body}]\n}}\n\ndefinition{{\n  name: a,\n  body: (b 1)\n}}\n\ndefinition{{\n  name: b,\n  body: (Prelude::p 1)\n}}\n\ndefinition{{name: c, parameters: [], body: {c_body}}}\n\\end{{semanticmodule}}\n\\end{{lexlean}}\n"
            )
        };
        let prelude = "\\begin{lexlean}{Prelude}\n\\begin{semanticmodule}\ndefinition{\n  name: p,\n  body: 1\n}\n\\end{semanticmodule}\n\\end{lexlean}\n";
        let register = register(&[
            ("k1", "meter::a", Status::Lifted("Behavior::a".into())),
            ("k2", "meter::b", Status::Lifted("Behavior::b".into())),
            ("k3", "meter::c", Status::Lifted("Behavior::c".into())),
        ]);
        let profile = keyed_profile();
        let keys = |name: &str, c_body: &str, type_body: &str, compiler: &str, runtime: &str| {
            let dir = tmp.join(name);
            keyed_projection(&dir, &behavior(c_body, type_body), prelude);
            let targets = keyed_targets(runtime);
            let input = Input {
                profile: &profile,
                projection: &dir,
                register: &register,
                targets: &targets,
            };
            let compiler = keyed_compiler(compiler);
            (
                closure_kappas(&input, &profile.roots(&register), &compiler).expect("closure κs"),
                project_identity(&input, &compiler).expect("a project identity"),
            )
        };
        let (base, project) = keys("base", "1", "x", "lexlean 1", "r1");
        let (c_changed, _) = keys("c", "2", "x", "lexlean 1", "r1");
        assert_eq!(base.len(), 3);
        assert_eq!(base["a"], c_changed["a"]);
        assert_eq!(base["b"], c_changed["b"]);
        assert_ne!(base["c"], c_changed["c"]);
        let (typed, typed_project) = keys("typed", "1", "y", "lexlean 1", "r1");
        assert_eq!(base, typed);
        assert_ne!(project, typed_project);
        // The compiler or the descriptors change every package.
        let (compiled, _) = keys("compiled", "1", "x", "lexlean 2", "r1");
        let (described, _) = keys("described", "1", "x", "lexlean 1", "r2");
        for r in ["a", "b", "c"] {
            assert_ne!(base[r], compiled[r]);
            assert_ne!(base[r], described[r]);
        }
    }

    #[test]
    fn marking_makes_the_roots_production_and_their_callees_executable() {
        let behavior = "\\begin{lexlean}{Behavior}\ndefinition{\n  name: a,\n  body: x\n}\n\ndefinition{\n  name: b,\n  body: (c (Prelude::p 1)),\n  axioms: []\n}\ndefinition{name: c, parameters: [], body: 1}\n";
        let prelude = "definition{\n  name: p,\n  body: (q 1)\n}\ndefinition{\n  name: q,\n  body: 1\n}\ndefinition{\n  name: r,\n  body: 1\n}\n";
        let modules: BTreeMap<String, String> = [
            ("Behavior".to_string(), behavior.to_string()),
            ("Prelude".to_string(), prelude.to_string()),
        ]
        .into_iter()
        .collect();
        let names: BTreeSet<&str> = ["b"].into_iter().collect();
        let (out, found, refused) = mark(&modules, &names);
        assert!(refused.is_empty());
        assert!(found.contains("b"));
        let b = &out["Behavior"];
        assert!(
            b.starts_with("\\begin{lexlean}{Behavior}\ndefinition{\n  name: a,\n  body: x\n}\n")
        );
        assert!(b.contains("  axioms: [],\n  executable: true,\n  production: {effects: [allocation, overflow, recursion], targets: [rust-std]}\n}\n"));
        assert!(b.contains("definition{name: c, parameters: [], body: 1, executable: true}\n"));
        let p = &out["Prelude"];
        assert!(p.contains("  body: (q 1),\n  executable: true\n}"));
        assert!(p.contains("  name: q,\n  body: 1,\n  executable: true\n}"));
        assert!(p.contains("  name: r,\n  body: 1\n}"));
    }

    #[test]
    fn an_adapter_converts_strings_lists_and_naturals() {
        let root = Certified {
            function: "uorust::mir::lower::implication".into(),
            kappa: "k".into(),
            definition: "implication".into(),
            package: "uorust_certified_implication".into(),
            source: "s".into(),
            certification: "kernel-checked".into(),
            closure: None,
            edition: None,
        };
        let body = IrBody {
            arity: 2,
            locals: vec![
                "std::string::String".into(),
                "&'a [std::string::String]".into(),
                "&str".into(),
            ],
            ..IrBody::default()
        };
        let source = "pub fn run(p0: List<Str>, p1: Str) -> Str {\n    f0(p0, p1)\n}\n";
        let (name, code) = adapter(&root, &body, source).expect("an adapter");
        assert_eq!(name, "mir_lower_implication");
        assert!(code.contains("pub(crate) fn mir_lower_implication(p0: &&[std::string::String], p1: &&str) -> Option<std::string::String>"), "{code}");
        assert!(code.contains("uorust_certified::implication::run("));
        assert!(code.contains(".text().to_string()"));
        // A type with no conversion has no adapter.
        let opaque = IrBody {
            arity: 1,
            locals: vec!["bool".into(), "&mir::lower::Cx<'_>".into()],
            ..IrBody::default()
        };
        assert_eq!(
            adapter(&root, &opaque, "pub fn run(p0: Adt3) -> bool {\n}\n"),
            None
        );
        // An overflowing root's result is unwrapped.
        let nat = IrBody {
            arity: 1,
            locals: vec!["u32".into(), "u32".into()],
            ..IrBody::default()
        };
        let (_, code) =
            adapter(&root, &nat, "pub fn run(p0: u64) -> R<u64> {\n}\n").expect("an adapter");
        assert!(code.contains("the certified root overflowed"));
    }

    // The planted defect of the differential: one byte apart is refused.
    #[test]
    fn the_differential_refuses_one_byte_apart() {
        let dir = std::env::temp_dir().join(format!("uorust-agree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, text) in [
            ("core-certified", "a"),
            ("core-hand", "a"),
            ("self-certified", "b"),
            ("self-hand", "c"),
        ] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(dir.join(name).join("frontier.toml"), text).unwrap();
        }
        match agree(&dir) {
            Err(UorustError::Disagree(rows)) => {
                assert_eq!(rows.len(), 1);
                assert!(rows[0].starts_with("self: certified κ"));
            }
            other => panic!("not refused: {other:?}"),
        }
        std::fs::write(dir.join("self-hand").join("frontier.toml"), "b").unwrap();
        let lines = agree(&dir).expect("agrees");
        assert_eq!(lines.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_diagnostic_names_its_root() {
        let m = "production root `Uorust.Behavior.exprIsFlat` is not eligible";
        assert_eq!(
            first_root(m, "Uorust.Behavior."),
            Some("exprIsFlat".to_string())
        );
        assert_eq!(
            first_root("root `Uorust.Prelude.x`", "Uorust.Behavior."),
            None
        );
    }

    #[test]
    fn a_trait_call_is_the_method_of_its_type() {
        assert_eq!(
            callee_path("<lift::Types as std::default::Default>::default"),
            "lift::Types::default"
        );
        assert_eq!(callee_path("mir::parse::bodies"), "mir::parse::bodies");
    }

    // The planted defect: a new translator function that does not lift, and a
    // certified one whose κ changed, grow the uncertified surface; the
    // ratchet refuses both, naming each with its cause.
    #[test]
    fn the_ratchet_refuses_a_grown_surface_naming_each_new_row() {
        let base = register(&[
            (
                "k1",
                "uorust::mir::lower::a",
                Status::Lifted("Behavior::a".into()),
            ),
            (
                "k2",
                "uorust::mir::lower::b",
                Status::Residue("mir".into(), "mir::Ty: x".into()),
            ),
        ]);
        let certified = Manifest {
            roots: vec![Certified {
                function: "uorust::mir::lower::a".into(),
                kappa: "k1".into(),
                definition: "a".into(),
                package: "uorust_certified_a".into(),
                source: "s".into(),
                certification: "kernel-checked".into(),
                closure: None,
                edition: None,
            }],
            ..Manifest::default()
        };
        assert_eq!(ratchet(&base, &certified, &base, &certified), Ok(1));
        let grown = register(&[
            (
                "k1",
                "uorust::mir::lower::a",
                Status::Lifted("Behavior::a".into()),
            ),
            (
                "k2",
                "uorust::mir::lower::b",
                Status::Residue("mir".into(), "mir::Ty: x".into()),
            ),
            (
                "k3",
                "uorust::mir::lower::c",
                Status::Residue("mir".into(), "mir::Rvalue::Use".into()),
            ),
        ]);
        match ratchet(&grown, &certified, &base, &certified) {
            Err(UorustError::Grown(rows)) => {
                assert_eq!(rows.len(), 2);
                assert!(rows[1].contains("uorust::mir::lower::c: residue (mir): mir::Rvalue::Use"));
            }
            other => panic!("not refused: {other:?}"),
        }
        let edited = register(&[
            (
                "k9",
                "uorust::mir::lower::a",
                Status::Lifted("Behavior::a".into()),
            ),
            (
                "k2",
                "uorust::mir::lower::b",
                Status::Residue("mir".into(), "mir::Ty: x".into()),
            ),
        ]);
        assert!(matches!(
            ratchet(&edited, &certified, &base, &certified),
            Err(UorustError::Grown(_))
        ));
        // Code outside the translator is not measured.
        let outside = register(&[
            (
                "k1",
                "uorust::mir::lower::a",
                Status::Lifted("Behavior::a".into()),
            ),
            (
                "k2",
                "uorust::mir::lower::b",
                Status::Residue("mir".into(), "mir::Ty: x".into()),
            ),
            (
                "k4",
                "uorust::project::d",
                Status::Residue("mir".into(), "e".into()),
            ),
        ]);
        assert_eq!(ratchet(&outside, &certified, &base, &certified), Ok(1));
    }

    #[test]
    fn the_manifest_round_trips() {
        let m = Manifest {
            result: "r".into(),
            compiler: "0.4.0".into(),
            project: Some("p".into()),
            roots: vec![Certified {
                function: "uorust::mir::coverage::binop".into(),
                kappa: "k".into(),
                definition: "binop".into(),
                package: "uorust_certified_binop".into(),
                source: "s".into(),
                certification: "kernel-checked".into(),
                closure: Some("c1".into()),
                edition: Some("2021".into()),
            }],
            share: Share {
                translator: 3,
                certified: 1,
                reachable: 2,
                reachable_certified: 1,
            },
            seconds_per_root: Some(9),
            refused: [(
                "k2".to_string(),
                "reaches Behavior::walk, a \"function\"".to_string(),
            )]
            .into_iter()
            .collect(),
        };
        let document = m.to_text();
        assert!(
            document.starts_with('#'),
            "the manifest includes a comment header"
        );
        assert_eq!(Manifest::parse(Path::new("m"), &document), Ok(m));
        // A text that is not a TOML document must still be rejected.
        assert!(Manifest::parse(Path::new("m"), "not a valid = [").is_err());
    }

    #[test]
    fn a_plan_sizes_its_shards_and_names_the_compiler_refusals() {
        let rows: Vec<(String, String, Status)> = (0..20)
            .map(|i| {
                (
                    format!("k{i}"),
                    format!("uorust::mir::f{i}"),
                    Status::Lifted(format!("Behavior::f{i}")),
                )
            })
            .collect();
        let reg = register(
            &rows
                .iter()
                .map(|(k, f, s)| (k.as_str(), f.as_str(), s.clone()))
                .collect::<Vec<_>>(),
        );
        let mut prior = Manifest::default();
        prior
            .refused
            .insert("k0".into(), "built alone: LLV7015 certificate B: x".into());
        prior.refused.insert("k1".into(), UNBUILT.into());
        // Nineteen to build, at most eight a shard however long the budget.
        let (shards, diagnose) = plan(&reg, &prior, u64::MAX / 2);
        assert_eq!((shards, diagnose), (3, vec!["f0".to_string()]));
        // A budget of one build of two roots: ten shards.
        prior.seconds_per_root = Some(1);
        assert_eq!(plan(&reg, &prior, 2 * BUILDS_PER_SHARD).0, 10);
    }

    #[test]
    fn the_kept_bundles_are_read_in_build_order() {
        let kept = std::env::temp_dir().join(format!("uorust-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&kept);
        for n in ["1", "2", "10", "notes"] {
            std::fs::create_dir_all(kept.join(n)).expect("a bundle directory");
            std::fs::write(kept.join(n).join("result.json"), "{}").expect("a result");
        }
        std::fs::create_dir_all(kept.join("3")).expect("an empty directory");
        let found = bundles(&kept).expect("the bundles");
        let _ = std::fs::remove_dir_all(&kept);
        let names: Vec<_> = found
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(str::to_string))
            .collect();
        assert_eq!(names, ["1", "2", "10"]);
    }

    // Class: the implementation produced from the model, for any workspace.

    /// A workspace that is not the translator's: a meter with two crates.
    fn meter() -> Profile {
        Profile {
            scope: vec!["meter::".to_string()],
            entry: None,
            package: "meter_certified_".to_string(),
            effects: vec!["overflow".to_string()],
            target: "rust-core".to_string(),
            kind: Kind::Functions,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uorust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    // The default of every step is the translator's own export, so its gate
    // reads as before: the constants, the committed manifest byte for byte,
    // and the committed crate of its roots byte for byte.
    #[test]
    fn the_default_profile_is_the_translators_own() {
        let own = Profile::default();
        assert_eq!(own.scope, SCOPE);
        assert_eq!(own.entry.as_deref(), Some(ENTRY));
        assert_eq!(own.package, PACKAGE);
        assert_eq!(own.effects, EFFECTS);
        assert_eq!(own.target, TARGET);
        assert_eq!(own.kind, Kind::Functions);
        assert_eq!(Profile::resolve(&Options::default()), Ok(own.clone()));
        assert!(SCOPE.iter().all(|s| in_scope(&format!("{s}f"))));
        let certified = Path::new(env!("CARGO_MANIFEST_DIR")).join("../certified");
        let path = certified.join("manifest.toml");
        let text = std::fs::read_to_string(&path).expect("the committed manifest");
        let manifest = Manifest::parse(&path, &text).expect("it parses");
        assert_eq!(manifest.to_text(), text);
        assert_eq!(manifest.text(&own), text);
        assert_eq!(Profile::recorded(&path, &text), Ok(own.clone()));
        let lib = std::fs::read_to_string(certified.join("src/lib.rs")).expect("its crate");
        assert_eq!(own.crate_source(&manifest.roots), lib);
    }

    // Step 1, the export. The planted defect: roots read through the
    // translator's constants give this workspace none.
    #[test]
    fn another_workspace_has_its_own_roots_packages_effects_and_target() {
        let r = register(&[
            ("a", "meter::read", Status::Lifted("Behavior::read".into())),
            (
                "b",
                "gauge::show",
                Status::Lifted("Behavior::showAll".into()),
            ),
            (
                "c",
                "meter::write",
                Status::Residue("form".into(), "d".into()),
            ),
        ]);
        assert!(roots(&r).is_empty());
        let meter = meter();
        let found = meter.roots(&r);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].function, "meter::read");
        assert_eq!(found[0].package(), "meter_certified_read");
        // Every function of the register, under the empty prefix.
        let every = Profile {
            scope: vec![String::new()],
            ..meter.clone()
        };
        let packages: Vec<String> = every.roots(&r).iter().map(Root::package).collect();
        assert_eq!(
            packages,
            ["meter_certified_read", "meter_certified_show_all"]
        );
        let modules: BTreeMap<String, String> = [
            (
                "Behavior".to_string(),
                "definition{\n  name: read,\n  body: 1\n}\n".to_string(),
            ),
            ("Prelude".to_string(), String::new()),
        ]
        .into_iter()
        .collect();
        let names: BTreeSet<&str> = ["read"].into_iter().collect();
        let (out, found, refused) = meter.mark(&modules, &names);
        assert!(refused.is_empty() && found.contains("read"));
        assert!(out["Behavior"].contains(
            "  executable: true,\n  production: {effects: [overflow], targets: [rust-core]}\n}\n"
        ));
        // An adapter calls the workspace's own certified crate and is named
        // by the function's path in its own crate.
        let certified = Certified {
            function: "meter::dial::read".into(),
            kappa: "a".into(),
            definition: "read".into(),
            package: "meter_certified_read".into(),
            source: "s".into(),
            certification: "kernel-checked".into(),
            closure: None,
            edition: None,
        };
        let body = IrBody {
            arity: 2,
            locals: vec!["u32".into(), "&[u32]".into(), "u32".into()],
            ..IrBody::default()
        };
        let source = "pub fn run(p0: List<u64>, p1: u64) -> R<u64> {\n}\n";
        let (name, code) = meter
            .adapter(&certified, &body, source)
            .expect("an adapter");
        assert_eq!(name, "dial_read");
        assert!(code.contains("pub(crate) fn dial_read(p0: &&[u32], p1: &u32) -> Option<u32>"));
        assert!(code.contains("meter_certified::read::run("), "{code}");
        // The request's descriptor is the profile's target's.
        let targets =
            r#"{"targets":[{"id":"rust-core","digest":"c0"},{"id":"rust-std","digest":"5d"}]}"#;
        assert_eq!(meter.descriptor(targets), Ok("c0".to_string()));
        assert_eq!(Profile::default().descriptor(targets), Ok("5d".to_string()));
        let none = Profile {
            target: "rust-wasm".to_string(),
            ..meter
        };
        assert!(matches!(
            none.descriptor(targets),
            Err(UorustError::Malformed { .. })
        ));
    }

    #[test]
    fn a_workspace_declares_its_profile_in_its_cargo_manifest() {
        let dir = scratch("profile").join("meter-ws");
        std::fs::create_dir_all(&dir).expect("the workspace");
        let cargo = dir.join("Cargo.toml");
        std::fs::write(&cargo, "[workspace]\nmembers = [\"meter\"]\n").expect("write");
        // What the workspace gives without a table: everything in scope,
        // packages named after its directory, every effect, the std target.
        let derived = Profile::of_workspace(&dir).expect("a profile");
        assert_eq!(derived.scope, [""]);
        assert!(derived.in_scope("meter::read") && derived.in_scope("gauge::show"));
        assert_eq!(derived.entry, None);
        assert_eq!(derived.package, "meter_ws_certified_");
        assert_eq!(derived.effects, EFFECTS);
        assert_eq!(derived.target, TARGET);
        std::fs::write(
            &cargo,
            "[workspace]\nmembers = [\"meter\"]\n\n[workspace.metadata.uorust.certify]\nscope = [\"meter::\"]\nentry = \"meter::run\"\neffects = [\"overflow\"]\ntarget = \"rust-core\"\n",
        )
        .expect("write");
        let declared = Profile::of_workspace(&dir).expect("a profile");
        assert_eq!(declared.scope, ["meter::"]);
        assert_eq!(declared.entry.as_deref(), Some("meter::run"));
        assert_eq!(declared.package, "meter_ws_certified_");
        assert_eq!(declared.effects, ["overflow"]);
        assert_eq!(declared.target, "rust-core");
        // An option replaces what the workspace declares.
        let options = Options {
            workspace: Some(dir.clone()),
            package: Some("m_".to_string()),
            cases: true,
            ..Options::default()
        };
        let resolved = Profile::resolve(&options).expect("a profile");
        assert_eq!(
            (resolved.package.as_str(), resolved.kind),
            ("m_", Kind::Cases)
        );
        assert_eq!(resolved.scope, ["meter::"]);
        // What an export records is read back whole, whatever the base.
        let table: toml::Value = toml::from_str(&resolved.rows()).expect("rows");
        assert_eq!(
            Profile::parse(&cargo, &table, Profile::default()),
            Ok(resolved)
        );
        for bad in [
            "effects = [\"io\"]\n",
            "roots = \"tests\"\n",
            "scopes = []\n",
            "package = \"Meter-\"\n",
            "package = \"9meter_\"\n",
            "scope = []\n",
        ] {
            std::fs::write(&cargo, format!("[package.metadata.uorust.certify]\n{bad}"))
                .expect("write");
            assert!(
                matches!(
                    Profile::of_workspace(&dir),
                    Err(UorustError::Malformed { .. })
                ),
                "{bad}"
            );
        }
        let _ = std::fs::remove_dir_all(dir.parent().expect("the scratch"));
    }

    /// What `certify production` keeps of one bundle of one package, under
    /// `kept`: the result, the package's source and Cargo manifest, the κ it
    /// was exported at, the refusals, the timing and the profile.
    fn kept_bundle(kept: &Path, profile: &Profile, root: &Root, source: &str) {
        let package = root.package();
        let directory = format!("packages/{}/{package}", profile.target);
        let bundle = kept.join("1");
        std::fs::create_dir_all(bundle.join(&directory).join("src")).expect("the package");
        std::fs::write(bundle.join(&directory).join("src/lib.rs"), source).expect("write");
        std::fs::write(
            bundle.join(&directory).join("Cargo.toml"),
            format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .expect("write");
        let result = serde_json::json!({
            "result_id": "r1",
            "roots": [{"outputs": [{
                "certification": {"status": "kernel-checked"},
                "package": {
                    "directory": directory,
                    "files": [{
                        "path": format!("{directory}/src/lib.rs"),
                        "sha256": crate::project::sha(source.as_bytes()),
                    }],
                },
            }]}],
        });
        std::fs::write(bundle.join("result.json"), result.to_string()).expect("write");
        std::fs::write(
            kept.join(KAPPAS),
            format!("{package} = \"{}\"\n", root.kappa),
        )
        .expect("write");
        std::fs::write(
            kept.join(REFUSALS),
            "[[refused]]\nkappa = \"k2\"\ncause = \"LLT4005 not eligible\"\n",
        )
        .expect("write");
        std::fs::write(kept.join(TIMING), "seconds = 12\nroots = 1\n").expect("write");
        std::fs::write(kept.join(PROFILE), profile.rows()).expect("write");
    }

    // Step 2, taking the bundle in. The planted defects: a package taken
    // whatever its function's κ, and a source that is not the one the
    // bundle's result records.
    #[test]
    fn a_sync_takes_the_packages_into_the_named_directory_keyed_by_kappa() {
        let tmp = scratch("sync");
        let (kept, dir) = (tmp.join("kept"), tmp.join("meter-certified"));
        let profile = Profile {
            target: TARGET.to_string(),
            ..meter()
        };
        let rows = |kappa: &'static str| {
            register(&[
                (
                    kappa,
                    "meter::read",
                    Status::Lifted("Behavior::read".into()),
                ),
                (
                    "k2",
                    "meter::scale",
                    Status::Lifted("Behavior::scale".into()),
                ),
                ("k3", "gauge::show", Status::Lifted("Behavior::show".into())),
            ])
        };
        let exported = rows("k1");
        let source = "pub fn run(p0: u64) -> u64 {\n    p0\n}\n";
        kept_bundle(&kept, &profile, &profile.roots(&exported)[0], source);
        assert_eq!(kept_profile(&kept), Ok(profile.clone()));
        let take = |register: &Register| {
            let s = Sync {
                kept: &kept,
                dir: &dir,
                register,
                projection: None,
                adapters: None,
                mir: None,
            };
            sync(&profile, &s, &profile.roots(register), None, "0.4.0")
        };
        let manifest = take(&exported).expect("a sync");
        assert_eq!(manifest.kappas(), ["k1".to_string()].into_iter().collect());
        assert_eq!(manifest.roots[0].function, "meter::read");
        assert_eq!(manifest.roots[0].package, "meter_certified_read");
        assert_eq!(manifest.refused["k2"], "LLT4005 not eligible");
        assert_eq!(manifest.seconds_per_root, Some(12));
        // Two functions in scope, one certified; the gauge is not measured.
        assert_eq!(
            (manifest.share.translator, manifest.share.certified),
            (2, 1)
        );
        let read = |rel: &str| std::fs::read_to_string(dir.join(rel)).expect(rel);
        assert_eq!(read("roots/meter_certified_read.rs"), source);
        assert!(read("src/lib.rs")
            .contains("#[path = \"../roots/meter_certified_read.rs\"]\npub mod read;\n"));
        let cargo = read("Cargo.toml");
        assert!(cargo.contains("name = \"meter_certified\"\n"), "{cargo}");
        assert!(cargo.contains("edition = \"2021\"\n") && cargo.contains("[workspace]\n"));
        // The manifest states the profile it was taken in under.
        let text = read("manifest.toml");
        let path = dir.join("manifest.toml");
        assert_eq!(Profile::recorded(&path, &text), Ok(profile.clone()));
        assert_eq!(Manifest::parse(&path, &text), Ok(manifest.clone()));
        assert!(!text.contains("entry = \"uorust"));
        // A caller's own Cargo manifest is kept.
        std::fs::write(dir.join("Cargo.toml"), "# the caller's\n").expect("write");
        // The function changed since the export: a new question, whose
        // package is not taken, so it is uncertified until rebuilt.
        let changed = rows("k9");
        let later = take(&changed).expect("a sync");
        assert!(later.roots.is_empty());
        assert!(!dir.join("roots/meter_certified_read.rs").exists());
        assert_eq!(read("Cargo.toml"), "# the caller's\n");
        assert_eq!(
            profile.uncertified(&changed, &later)["meter::read"],
            "lifted as Behavior::read, not yet certified"
        );
        assert_eq!(
            profile
                .uncertified(&exported, &manifest)
                .keys()
                .collect::<Vec<_>>(),
            ["meter::scale"]
        );
        // A source that is not the one the result records is refused.
        std::fs::write(
            kept.join("1/packages/rust-std/meter_certified_read/src/lib.rs"),
            "pub fn run(p0: u64) -> u64 {\n    0\n}\n",
        )
        .expect("plant");
        match take(&exported) {
            Err(UorustError::Malformed { reason, .. }) => {
                assert!(reason.contains("the result records"), "{reason}");
            }
            other => panic!("not refused: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(tmp);
    }

    // Step 3, the ratchet. The planted defect: a head measured through the
    // translator's scope sees none of this workspace's functions and passes.
    #[test]
    fn the_ratchet_holds_any_scope_and_a_scope_only_widens() {
        let meter = meter();
        let base = register(&[
            ("k1", "meter::read", Status::Lifted("Behavior::read".into())),
            (
                "k2",
                "meter::scale",
                Status::Residue("form".into(), "x".into()),
            ),
        ]);
        let certified = Manifest {
            roots: vec![Certified {
                function: "meter::read".into(),
                kappa: "k1".into(),
                definition: "read".into(),
                package: "meter_certified_read".into(),
                source: "s".into(),
                certification: "kernel-checked".into(),
                closure: None,
                edition: None,
            }],
            ..Manifest::default()
        };
        assert_eq!(meter.ratchet(&base, &certified, &base, &certified), Ok(1));
        let grown = register(&[
            ("k9", "meter::read", Status::Lifted("Behavior::read".into())),
            (
                "k2",
                "meter::scale",
                Status::Residue("form".into(), "x".into()),
            ),
            (
                "k3",
                "meter::sum",
                Status::Effect("Fs effect: fs::read".into()),
            ),
            (
                "k4",
                "gauge::show",
                Status::Residue("form".into(), "y".into()),
            ),
        ]);
        // The translator's scope measures nothing here.
        assert_eq!(ratchet(&grown, &certified, &base, &certified), Ok(0));
        match meter.ratchet(&grown, &certified, &base, &certified) {
            Err(UorustError::Grown(rows)) => assert_eq!(
                rows,
                [
                    "3 uncertified functions in scope, 1 in the parent",
                    "meter::read: lifted as Behavior::read, not yet certified",
                    "meter::sum: effect: Fs effect: fs::read"
                ]
            ),
            other => panic!("not refused: {other:?}"),
        }
        // A head that measures less than its base is refused before any
        // count; one that measures more is counted under its own scope.
        let wide = Profile {
            scope: vec![String::new()],
            ..meter.clone()
        };
        match meter.ratchet_from(Some(&wide), &base, &certified, &base, &certified) {
            Err(UorustError::Grown(rows)) => {
                assert_eq!(rows[1], "every function: no longer in scope");
            }
            other => panic!("not refused: {other:?}"),
        }
        assert_eq!(
            wide.ratchet_from(Some(&meter), &base, &certified, &base, &certified),
            Ok(1)
        );
        assert_eq!(
            meter.ratchet_from(None, &base, &certified, &base, &certified),
            Ok(1)
        );
        let cases = Profile {
            kind: Kind::Cases,
            ..meter
        };
        assert!(matches!(
            cases.ratchet_from(None, &base, &certified, &base, &certified),
            Err(UorustError::Tool(_))
        ));
    }

    fn certifying(roots: &[(&str, &str)]) -> Manifest {
        Manifest {
            roots: roots
                .iter()
                .map(|(function, kappa)| Certified {
                    function: (*function).into(),
                    kappa: (*kappa).into(),
                    definition: "d".into(),
                    package: "meter_certified_d".into(),
                    source: "s".into(),
                    certification: "kernel-checked".into(),
                    closure: None,
                    edition: None,
                })
                .collect(),
            ..Manifest::default()
        }
    }

    // GATE-12: a root lost to residue is not masked by roots gained
    // elsewhere. The count shrinks (2 uncertified at the base, 1 at the head),
    // which the count rule alone passes.
    #[test]
    fn a_root_that_no_longer_lifts_is_refused_even_when_the_count_shrinks() {
        let lifted = || Status::Lifted("Behavior::x".into());
        let base = register(&[
            ("kf", "meter::f", lifted()),
            ("kg", "meter::g", lifted()),
            ("kh", "meter::h", lifted()),
        ]);
        let base_manifest = certifying(&[("meter::f", "kf")]);
        let head = register(&[
            (
                "kf2",
                "meter::f",
                Status::Residue("form".into(), "x".into()),
            ),
            ("kg", "meter::g", lifted()),
            ("kh", "meter::h", lifted()),
        ]);
        let head_manifest = certifying(&[("meter::g", "kg"), ("meter::h", "kh")]);
        match meter().ratchet(&head, &head_manifest, &base, &base_manifest) {
            Err(UorustError::Grown(rows)) => assert_eq!(
                rows,
                [
                    "1 certified roots of the parent no longer lift",
                    "meter::f: residue (form): x"
                ]
            ),
            other => panic!("not refused: {other:?}"),
        }
        // Edited, not lost: lifted under a new κ, owed to an export.
        let edited = register(&[
            ("kf2", "meter::f", lifted()),
            ("kg", "meter::g", lifted()),
            ("kh", "meter::h", lifted()),
        ]);
        assert_eq!(
            meter().ratchet(&edited, &head_manifest, &base, &base_manifest),
            Ok(1)
        );
    }

    #[test]
    fn a_deleted_root_is_not_a_loss() {
        let lifted = || Status::Lifted("Behavior::x".into());
        let base = register(&[("kf", "meter::f", lifted()), ("kg", "meter::g", lifted())]);
        let base_manifest = certifying(&[("meter::f", "kf")]);
        let head = register(&[("kg", "meter::g", lifted())]);
        assert_eq!(
            meter().ratchet(&head, &Manifest::default(), &base, &base_manifest),
            Ok(1)
        );
    }

    // Step 4, the agreement's export: the roots of a cases export are the
    // cases the projection states, read from its modules.
    #[test]
    fn the_roots_of_a_cases_export_are_the_projections_cases() {
        let tmp = scratch("cases");
        std::fs::create_dir_all(tmp.join("src")).expect("the sources");
        std::fs::write(
            tmp.join("src/Behavior.lex.tex"),
            "definition{\n  name: read,\n  parameters: [n : Nat],\n  result: Nat,\n  body: n\n}\n\ndefinition{\n  name: acceptMeterReads0Root,\n  parameters: [],\n  result: Bool,\n  body: ((read 2) =? 2)\n}\n\ntheorem{\n  name: acceptMeterReads0RootHolds,\n  parameters: [],\n  statement: ((acceptMeterReads0Root) = true),\n  proof: decide{}\n}\n",
        )
        .expect("write");
        std::fs::write(tmp.join("src/Prelude.lex.tex"), "").expect("write");
        let r = register(&[("k1", "meter::read", Status::Lifted("Behavior::read".into()))]);
        let functions = meter();
        let cases = Profile {
            kind: Kind::Cases,
            ..functions.clone()
        };
        let of = |p: &Profile| -> Vec<String> {
            let roots = p.export_roots(&tmp, &r).expect("the roots");
            roots.iter().map(Root::package).collect()
        };
        assert_eq!(of(&functions), ["meter_certified_read"]);
        // A name longer than a crate's may be is shortened to the bound,
        // the same at every export and each root's alone.
        let long = |end: &str| Root {
            definition: format!(
                "acceptMeterReadsEveryRoomOfTheRosterInItsOwnOrderAndKeepsThem{end}"
            ),
            ..functions.roots(&r)[0].clone()
        };
        let (first, second) = (long("0Root").package(), long("1Root").package());
        assert_eq!((first.len(), second.len()), (CRATE_NAME, CRATE_NAME));
        assert!(
            first.starts_with("meter_certified_accept_meter_reads_every_room_") && first != second
        );
        assert_eq!(first, long("0Root").package());
        assert_eq!(of(&cases), ["meter_certified_accept_meter_reads0_root"]);
        let found = cases.export_roots(&tmp, &r).expect("the roots");
        assert_eq!(found[0].function, "meter::read");
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn a_refusals_class_is_its_cause_without_the_projects_names() {
        let admission = Admission {
            roots: 4,
            admitted: vec!["a".into()],
            refused: [
                ("k1", "LLT4005 phase production: production root `Meter.Behavior.read` is not eligible for target `rust-std`: effect mismatch: construct `prim append` realizes effect `allocation`, which the root does not admit (in `Meter.Behavior.read`, reached by Meter.Behavior.read; 1 violation(s) in total)"),
                ("k2", "LLT4005 phase production: production root `Meter.Behavior.scale` is not eligible for target `rust-std`: effect mismatch: construct `prim append` realizes effect `allocation`, which the root does not admit (in `Meter.Behavior.scaleLoop1`, reached by Meter.Behavior.scale -> Meter.Behavior.scaleLoop1; 2 violation(s) in total)"),
                ("k3", "reaches Behavior::walk, a definition with a function parameter, which no production root may"),
            ]
            .into_iter()
            .map(|(k, c)| (k.to_string(), c.to_string()))
            .collect(),
            prefix: "Meter".to_string(),
        };
        assert_eq!(
            admission.causes(),
            [
                ("LLT4005 phase production: production root `_` is not eligible for target `rust-std`: effect mismatch: construct `prim append` realizes effect `allocation`, which the root does not admit".to_string(), 2),
                ("reaches _ a definition with a function parameter, which no production root may".to_string(), 1),
            ]
        );
    }

    // Class: keyed. A certified root is never built twice: its export's
    // identity states everything its package is a function of, and an export
    // carries every root the prior manifest certified at its identity.

    const KEYED_BEHAVIOR: &str = "\\begin{lexlean}{Behavior}\n\\importmodule{Prelude}\n\\begin{semanticmodule}\nstructure{\n  name: Reading,\n  type_parameters: [],\n  parameters: [],\n  fields: [{name: value, type: Nat}]\n}\n\ndefinition{\n  name: helper,\n  parameters: [x : Nat],\n  result: Nat,\n  body: x\n}\n\ndefinition{\n  name: read,\n  parameters: [r : Reading],\n  result: Nat,\n  body: helper (Prelude::unit r.value)\n}\n\ndefinition{name: scale, parameters: [x : Nat], result: Nat, body: x}\n\ndefinition{name: spare, parameters: [x : Nat], result: Nat, body: x}\n\\end{semanticmodule}\n\\end{lexlean}\n";

    const KEYED_PRELUDE: &str = "\\begin{lexlean}{Prelude}\n\\begin{semanticmodule}\ndefinition{name: unit, parameters: [x : Nat], result: Nat, body: x}\n\\end{semanticmodule}\n\\end{lexlean}\n";

    const KEYED_COMPILER: &str = "lexlean 0.4.0\nlanguage 1.0\ncompiler-semantics aaaa\nlean-toolchain leanprover/lean4:v4.32.1\n";

    fn keyed_targets(runtime: &str) -> String {
        format!(
            "{{\"targets\":[{{\"id\":\"rust-std\",\"digest\":\"d1\",\"runtime\":\"{runtime}\"}},{{\"id\":\"rust-core\",\"digest\":\"d2\",\"runtime\":\"r2\"}}]}}"
        )
    }

    /// A projection of two roots, `read` (which reaches `helper`, the
    /// structure `Reading` and Prelude's `unit`) and `scale` (which reaches
    /// nothing), with the configuration files a build reads.
    fn keyed_projection(dir: &Path, behavior: &str, prelude: &str) {
        let write = |rel: &str, text: &str| {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, text).expect("a file");
        };
        write(
            "lexlean.toml",
            "spec = \"lexlean/project/1\"\nname = \"meter\"\nmodule_prefix = \"Meter\"\nlockfile = \"lexlean.lock\"\n\n[[lexicon_source]]\npackage = \"meter.words\"\nkind = \"path\"\npath = \"language/meter.words\"\n",
        );
        write(
            "lexlean.lock",
            "spec = \"lexlean/lock/2\"\n\n[[workspace_file]]\npath = \"lakefile.toml\"\nsha256 = \"x\"\n",
        );
        write("lakefile.toml", "name = \"Meter\"\n");
        write("language/meter.words/glossary.toml", "words = 1\n");
        write("frontier.toml", "# the register, which no build reads\n");
        write("src/Behavior.lex.tex", behavior);
        write("src/Prelude.lex.tex", prelude);
    }

    fn keyed_register() -> Register {
        register(&[
            ("k1", "meter::read", Status::Lifted("Behavior::read".into())),
            (
                "k2",
                "meter::scale",
                Status::Lifted("Behavior::scale".into()),
            ),
        ])
    }

    fn keyed_profile() -> Profile {
        Profile {
            target: TARGET.to_string(),
            ..meter()
        }
    }

    /// The identities of the two roots of a projection written with these
    /// texts, under this compiler and these descriptors.
    fn keyed_ids(
        tmp: &Path,
        name: &str,
        behavior: &str,
        prelude: &str,
        compiler: &str,
        targets: &str,
    ) -> BTreeMap<String, String> {
        let dir = tmp.join(name);
        keyed_projection(&dir, behavior, prelude);
        let (profile, register) = (keyed_profile(), keyed_register());
        let input = Input {
            profile: &profile,
            projection: &dir,
            register: &register,
            targets,
        };
        closure_kappas(&input, &profile.roots(&register), &keyed_compiler(compiler))
            .expect("identities")
    }

    /// A prior manifest that certifies each root at its identity, and the
    /// source directory that holds each package as a certified crate does.
    fn keyed_prior(sources: &Path, ids: &BTreeMap<String, String>) -> Manifest {
        let profile = keyed_profile();
        let mut roots = Vec::new();
        for root in profile.roots(&keyed_register()) {
            let bytes = format!("pub fn run() -> u64 {{ {} }}\n", root.definition.len());
            let path = sources.join("roots").join(format!("{}.rs", root.package()));
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("roots");
            std::fs::write(&path, &bytes).expect("a source");
            roots.push(Certified {
                function: root.function.clone(),
                kappa: root.kappa.clone(),
                definition: root.definition.clone(),
                package: root.package(),
                source: crate::project::sha(bytes.as_bytes()),
                certification: "kernel-checked".into(),
                closure: ids.get(&root.definition).cloned(),
                edition: Some("2021".into()),
            });
        }
        Manifest {
            result: "r-prior".into(),
            compiler: "0.4.0".into(),
            roots,
            seconds_per_root: Some(7),
            ..Manifest::default()
        }
    }

    // The planted defects: an export that builds the roots it carries runs
    // `lexlean` (absent here) and fails; a sync that drops the carried
    // packages certifies nothing; a manifest that loses the identity carries
    // nothing the next time.
    #[test]
    fn keyed_an_unchanged_root_is_carried_byte_for_byte_and_never_built() {
        let tmp = scratch("keyed-carry");
        let targets = keyed_targets("r1");
        let ids = keyed_ids(
            &tmp,
            "p",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &targets,
        );
        let sources = tmp.join("prior");
        let manifest = keyed_prior(&sources, &ids);
        let (profile, register) = (keyed_profile(), keyed_register());
        let projection = tmp.join("p");
        let input = Input {
            profile: &profile,
            projection: &projection,
            register: &register,
            targets: &targets,
        };
        let export = |manifest: &Manifest, sources: &[PathBuf], kept: &Path, verify: Verifier| {
            production_build(
                &input,
                &tmp.join("out"),
                kept,
                &Prior {
                    manifest,
                    sources,
                    verify,
                },
                &keyed_compiler(KEYED_COMPILER),
                Scope::default(),
            )
        };
        let kept = tmp.join("kept");
        // The prior export verified no project: the project is verified
        // once, before anything is carried.
        let done = export(
            &manifest,
            std::slice::from_ref(&sources),
            &kept,
            keyed_verifies,
        )
        .expect("an export");
        assert_eq!(
            done,
            Exported {
                carried: 2,
                built: 0,
                builds: 0,
                refused: 0
            }
        );
        let dir = tmp.join("certified");
        let synced = sync(
            &profile,
            &Sync {
                kept: &kept,
                dir: &dir,
                register: &register,
                projection: None,
                adapters: None,
                mir: None,
            },
            &profile.roots(&register),
            None,
            "0.4.0",
        )
        .expect("a sync");
        assert_eq!(
            synced.kappas(),
            ["k1", "k2"].map(String::from).into_iter().collect()
        );
        for root in &synced.roots {
            let prior = std::fs::read(sources.join("roots").join(format!("{}.rs", root.package)));
            let now = std::fs::read(dir.join("roots").join(format!("{}.rs", root.package)));
            assert_eq!(
                prior.expect("the prior source"),
                now.expect("the carried source")
            );
            assert_eq!(root.closure, ids.get(&root.definition).cloned());
        }
        assert_eq!(synced.result, "r-prior");
        // The figure the shards are sized by survives an export that built
        // nothing.
        assert_eq!(synced.seconds_per_root, Some(7));
        let text = std::fs::read_to_string(dir.join("manifest.toml")).expect("the manifest");
        assert_eq!(
            Manifest::parse(&dir.join("manifest.toml"), &text).as_ref(),
            Ok(&synced)
        );
        // A steady export: the synced manifest is the next prior, its crate
        // the next source, and again nothing is built.
        // It verifies nothing again: the manifest holds the project.
        assert!(synced.project.is_some());
        let again = export(
            &synced,
            std::slice::from_ref(&dir),
            &tmp.join("kept2"),
            keyed_never_verified,
        )
        .expect("a steady export");
        assert_eq!((again.carried, again.builds), (2, 0));
        // A source that is not the one the manifest records is not carried:
        // the export then builds it, and finds no compiler here.
        std::fs::write(
            sources.join("roots").join("meter_certified_scale.rs"),
            "pub fn run() -> u64 { 0 }\n",
        )
        .expect("plant");
        let found = carried(
            &profile.roots(&register),
            &ids,
            &Prior {
                manifest: &manifest,
                sources: std::slice::from_ref(&sources),
                verify: keyed_verifies,
            },
        )
        .expect("carried");
        assert_eq!(
            found
                .iter()
                .map(|c| c.definition.as_str())
                .collect::<Vec<_>>(),
            ["read"]
        );
        assert!(export(
            &manifest,
            std::slice::from_ref(&sources),
            &tmp.join("kept3"),
            keyed_verifies
        )
        .is_err());
        // A carried source changed after the export is refused by the sync.
        std::fs::write(kept.join(CARRIED_DIR).join("meter_certified_read.rs"), "x").expect("plant");
        let refused = sync(
            &profile,
            &Sync {
                kept: &kept,
                dir: &dir,
                register: &register,
                projection: None,
                adapters: None,
                mir: None,
            },
            &profile.roots(&register),
            None,
            "0.4.0",
        );
        match refused {
            Err(UorustError::Malformed { reason, .. }) => {
                assert!(reason.contains("the prior manifest records"), "{reason}");
            }
            other => panic!("not refused: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(tmp);
    }

    // The planted defects: an identity of the root's own declaration alone,
    // or a carry keyed by κ, carries `read` after its callee changed.
    #[test]
    fn keyed_a_change_in_a_roots_closure_builds_it_again_though_its_kappa_is_unchanged() {
        let tmp = scratch("keyed-closure");
        let targets = keyed_targets("r1");
        let at = |name: &str, behavior: &str, prelude: &str| {
            keyed_ids(&tmp, name, behavior, prelude, KEYED_COMPILER, &targets)
        };
        let base = at("base", KEYED_BEHAVIOR, KEYED_PRELUDE);
        let same = |ids: &BTreeMap<String, String>, def: &str| ids[def] == base[def];
        // The callee changes; the register (every κ) does not.
        let callee = at(
            "callee",
            &KEYED_BEHAVIOR.replace(
                "  body: x\n}\n\ndefinition{\n  name: read",
                "  body: 0\n}\n\ndefinition{\n  name: read",
            ),
            KEYED_PRELUDE,
        );
        assert!(!same(&callee, "read") && same(&callee, "scale"));
        // A type the root names, and a definition of another module it calls.
        let ty = at(
            "type",
            &KEYED_BEHAVIOR.replace("{name: value, type: Nat}", "{name: value, type: Int}"),
            KEYED_PRELUDE,
        );
        assert!(!same(&ty, "read") && same(&ty, "scale"));
        let prelude = at(
            "prelude",
            KEYED_BEHAVIOR,
            &KEYED_PRELUDE.replace("body: x}", "body: 1}"),
        );
        assert!(!same(&prelude, "read") && same(&prelude, "scale"));
        // What no root reaches, the order of declarations, blank lines and
        // the files no build reads change nothing.
        let spare = at(
            "spare",
            &KEYED_BEHAVIOR.replace(
                "name: spare, parameters: [x : Nat], result: Nat, body: x",
                "name: spare, parameters: [x : Nat], result: Nat, body: 2",
            ),
            KEYED_PRELUDE,
        );
        assert_eq!(spare, base);
        let moved = at(
            "moved",
            &KEYED_BEHAVIOR.replace(
                "\\begin{semanticmodule}\n",
                "\\begin{semanticmodule}\nstructure{\n  name: Other,\n  type_parameters: [],\n  parameters: [],\n  fields: []\n}\n\n\n",
            ),
            KEYED_PRELUDE,
        );
        assert_eq!(moved, base);
        std::fs::write(tmp.join("base").join("frontier.toml"), "# moved\n").expect("write");
        assert_eq!(at("base", KEYED_BEHAVIOR, KEYED_PRELUDE), base);
        // The plan counts what is left: the prior certified both at the base.
        let sources = tmp.join("prior");
        let manifest = keyed_prior(&sources, &base);
        let roots = keyed_profile().roots(&keyed_register());
        let prior = Prior {
            manifest: &manifest,
            sources: std::slice::from_ref(&sources),
            verify: keyed_verifies,
        };
        let (profile, register) = (keyed_profile(), keyed_register());
        let projection = tmp.join("callee");
        let input = Input {
            profile: &profile,
            projection: &projection,
            register: &register,
            targets: &targets,
        };
        let decided =
            carry(&input, &roots, &keyed_compiler(KEYED_COMPILER), &prior).expect("carry");
        assert_eq!(decided.ids, callee);
        let carried_now: BTreeSet<String> = decided
            .carried
            .iter()
            .map(|c| c.definition.clone())
            .collect();
        assert_eq!(carried_now, ["scale".to_string()].into_iter().collect());
        let plan = plan_keyed(&roots, Some(&decided), &manifest, u64::MAX / 2);
        assert_eq!(
            (plan.carried, plan.build, plan.refused, plan.shards),
            (Some(1), 1, 0, 1)
        );
        // Without identities nothing is known to be carried.
        let blind = plan_keyed(&roots, None, &manifest, u64::MAX / 2);
        assert_eq!((blind.carried, blind.build), (None, 2));
        // A manifest written before identities carries nothing.
        let mut old = manifest.clone();
        for r in &mut old.roots {
            r.closure = None;
        }
        let none = carried(
            &roots,
            &base,
            &Prior {
                manifest: &old,
                sources: &[sources],
                verify: keyed_verifies,
            },
        );
        assert_eq!(none.map(|c| c.len()), Ok(0));
        let _ = std::fs::remove_dir_all(tmp);
    }

    // The planted defect: an identity without the compiler's lines, the
    // descriptor or the project's files keeps every root carried.
    #[test]
    fn keyed_a_compiler_descriptor_or_configuration_change_builds_every_root_again() {
        let tmp = scratch("keyed-toolchain");
        let targets = keyed_targets("r1");
        let base = keyed_ids(
            &tmp,
            "base",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &targets,
        );
        let all_differ = |ids: &BTreeMap<String, String>| {
            assert_eq!(ids.len(), base.len());
            for (def, id) in ids {
                assert_ne!(&base[def], id, "{def} keeps its identity");
            }
        };
        // One version, another compiler semantics.
        let semantics = KEYED_COMPILER.replace("aaaa", "bbbb");
        assert_eq!(
            keyed_compiler(&semantics).version(),
            keyed_compiler(KEYED_COMPILER).version()
        );
        all_differ(&keyed_ids(
            &tmp,
            "c",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            &semantics,
            &targets,
        ));
        let toolchain = KEYED_COMPILER.replace("v4.32.1", "v4.33.0");
        all_differ(&keyed_ids(
            &tmp,
            "l",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            &toolchain,
            &targets,
        ));
        // The target's runtime, under one descriptor id.
        all_differ(&keyed_ids(
            &tmp,
            "t",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &keyed_targets("r9"),
        ));
        // Another target's descriptor is no input.
        let other = targets.replace("\"r2\"", "\"r8\"");
        assert_eq!(
            keyed_ids(
                &tmp,
                "o",
                KEYED_BEHAVIOR,
                KEYED_PRELUDE,
                KEYED_COMPILER,
                &other
            ),
            base
        );
        // The project's configuration, a lexicon it reads, a workspace file
        // its lock binds.
        for (rel, text) in [
            ("lexlean.toml", None),
            ("language/meter.words/glossary.toml", Some("words = 2\n")),
            ("lakefile.toml", Some("name = \"Meter2\"\n")),
        ] {
            let name = format!("f-{}", rel.replace('/', "-"));
            let dir = tmp.join(&name);
            keyed_projection(&dir, KEYED_BEHAVIOR, KEYED_PRELUDE);
            let path = dir.join(rel);
            let changed = match text {
                Some(t) => t.to_string(),
                None => {
                    std::fs::read_to_string(&path).expect("read") + "\n[limits]\nmax_ir_nodes = 1\n"
                }
            };
            std::fs::write(&path, changed).expect("write");
            let (profile, register) = (keyed_profile(), keyed_register());
            let input = Input {
                profile: &profile,
                projection: &dir,
                register: &register,
                targets: &targets,
            };
            all_differ(
                &closure_kappas(
                    &input,
                    &profile.roots(&register),
                    &keyed_compiler(KEYED_COMPILER),
                )
                .expect("identities"),
            );
        }
        // The effects the roots admit.
        let dir = tmp.join("base");
        let profile = Profile {
            effects: vec!["allocation".into()],
            ..keyed_profile()
        };
        let register = keyed_register();
        let input = Input {
            profile: &profile,
            projection: &dir,
            register: &register,
            targets: &targets,
        };
        all_differ(
            &closure_kappas(
                &input,
                &profile.roots(&register),
                &keyed_compiler(KEYED_COMPILER),
            )
            .expect("identities"),
        );
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn keyed_a_modules_declarations_are_read_by_name_whatever_their_kind_and_place() {
        let (frame, decls, order) = declarations(KEYED_BEHAVIOR);
        assert_eq!(order, ["Reading", "helper", "read", "scale", "spare"]);
        assert_eq!(
            decls.keys().map(String::as_str).collect::<Vec<_>>(),
            ["Reading", "helper", "read", "scale", "spare"]
        );
        assert_eq!(
            frame,
            [
                "\\begin{lexlean}{Behavior}",
                "\\importmodule{Prelude}",
                "\\begin{semanticmodule}",
                "\\end{semanticmodule}",
                "\\end{lexlean}"
            ]
        );
        assert!(decls["Reading"].starts_with("structure{\n") && decls["Reading"].ends_with("}\n"));
        assert_eq!(
            decls["scale"],
            "definition{name: scale, parameters: [x : Nat], result: Nat, body: x}\n"
        );
    }

    // keyed: the counterexamples of refute-keyed-6. Each identity input a
    // refuter found missing is in the identity now, with its test: a linked
    // package, the closure's relative order, the project a build verifies,
    // a carried package's edition, and the marking's refusals in the plan.

    fn keyed_verifies(dir: &Path) -> Result<Option<String>, UorustError> {
        assert!(dir.join("request.json").is_file(), "no production project");
        Ok(None)
    }

    // keyed2: the verification of a project nothing was built in marks one
    // root, so that its cost does not grow with the roots.
    fn keyed_verifies_one(dir: &Path) -> Result<Option<String>, UorustError> {
        let request = std::fs::read_to_string(dir.join("request.json")).expect("a request");
        let request: serde_json::Value = serde_json::from_str(&request).expect("json");
        assert_eq!(request["roots"].as_array().map(Vec::len), Some(1));
        Ok(None)
    }

    fn keyed_refuses(dir: &Path) -> Result<Option<String>, UorustError> {
        assert!(dir.join("request.json").is_file(), "no production project");
        Ok(Some(
            "LLV7002 Lean rejected `Meter.Behavior`: Tactic rfl failed".to_string(),
        ))
    }

    fn keyed_never_verified(dir: &Path) -> Result<Option<String>, UorustError> {
        panic!("verified {} again", dir.display())
    }

    /// The input of the base projection under `dir`, read with the
    /// compiler and descriptors of the other keyed tests.
    fn keyed_decide(dir: &Path, register: &Register, prior: &Prior<'_>) -> Carry {
        let profile = keyed_profile();
        let targets = keyed_targets("r1");
        let input = Input {
            profile: &profile,
            projection: dir,
            register,
            targets: &targets,
        };
        carry(
            &input,
            &profile.roots(register),
            &keyed_compiler(KEYED_COMPILER),
            prior,
        )
        .expect("carry")
    }

    fn keyed_project(dir: &Path) -> String {
        let (profile, register) = (keyed_profile(), keyed_register());
        let targets = keyed_targets("r1");
        let input = Input {
            profile: &profile,
            projection: dir,
            register: &register,
            targets: &targets,
        };
        project_identity(&input, &keyed_compiler(KEYED_COMPILER)).expect("a project identity")
    }

    // The planted defect: an identity without the linked packages' trees
    // carries a root whose package body changed.
    #[test]
    fn keyed_a_linked_packages_body_is_in_every_roots_identity() {
        let tmp = scratch("keyed-package");
        let targets = keyed_targets("r1");
        let ws = tmp.join("ws");
        let app = ws.join("app");
        let write = |path: &Path, text: &str| {
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, text).expect("a file");
        };
        let behavior = KEYED_BEHAVIOR.replace(
            "  body: helper (Prelude::unit r.value)\n",
            "  body: helper (PkgLib.Util::combine r.value)\n",
        );
        keyed_projection(&app, &behavior, KEYED_PRELUDE);
        let config = app.join("lexlean.toml");
        let text = std::fs::read_to_string(&config).expect("read");
        write(
            &config,
            &format!("{text}\n[[source_package]]\npackage = \"pkglib\"\nversion = \"1.0.0\"\nkind = \"path\"\npath = \"lib\"\n"),
        );
        write(
            &ws.join("lexlean-workspace.toml"),
            "spec = \"lexlean/workspace/1\"\nmembers = [\"app\", \"lib\", \"out/0\"]\n",
        );
        write(
            &ws.join("lib/lexlean.toml"),
            "spec = \"lexlean/project/1\"\nname = \"pkglib\"\nmodule_prefix = \"PkgLib\"\nsource_roots = [\"src\"]\n",
        );
        let util = ws.join("lib/src/Util.lex.tex");
        write(
            &util,
            "\\begin{lexlean}{Util}\n\\begin{semanticmodule}\ndefinition{name: combine, parameters: [x : Nat], result: Nat, body: x}\n\\end{semanticmodule}\n\\end{lexlean}\n",
        );
        let (profile, register) = (keyed_profile(), keyed_register());
        let ids = || {
            let input = Input {
                profile: &profile,
                projection: &app,
                register: &register,
                targets: &targets,
            };
            closure_kappas(
                &input,
                &profile.roots(&register),
                &keyed_compiler(KEYED_COMPILER),
            )
            .expect("identities")
        };
        let base = ids();
        let project = keyed_project(&app);
        let all_differ = |now: &BTreeMap<String, String>| {
            for (def, id) in now {
                assert_ne!(&base[def], id, "{def} keeps its identity");
            }
        };
        // The package's body changes: every root, and the project.
        std::fs::write(
            &util,
            std::fs::read_to_string(&util)
                .expect("read")
                .replace("body: x}", "body: 0}"),
        )
        .expect("plant");
        all_differ(&ids());
        assert_ne!(keyed_project(&app), project);
        // A file the language does not read of the package changes nothing.
        let restore = std::fs::read_to_string(&util)
            .expect("read")
            .replace("body: 0}", "body: x}");
        std::fs::write(&util, restore).expect("restore");
        assert_eq!(ids(), base);
        write(&ws.join("lib/notes.txt"), "not a source\n");
        assert_eq!(ids(), base);
        // The package's manifest and the workspace that grants it do.
        write(
            &ws.join("lib/lexlean.toml"),
            "spec = \"lexlean/project/1\"\nname = \"pkglib\"\nmodule_prefix = \"PkgLib\"\nsource_roots = [\"src\"]\n\n[limits]\nmax_ir_nodes = 1\n",
        );
        all_differ(&ids());
        let _ = std::fs::remove_dir_all(tmp);
    }

    // The planted defect: an identity without the closure's order carries a
    // root whose callee now follows it, which the language refuses.
    #[test]
    fn keyed_the_relative_order_of_a_roots_closure_is_in_its_identity() {
        let tmp = scratch("keyed-order");
        let targets = keyed_targets("r1");
        let at = |name: &str, behavior: &str| {
            keyed_ids(
                &tmp,
                name,
                behavior,
                KEYED_PRELUDE,
                KEYED_COMPILER,
                &targets,
            )
        };
        let base = at("base", KEYED_BEHAVIOR);
        let helper = "definition{\n  name: helper,\n  parameters: [x : Nat],\n  result: Nat,\n  body: x\n}\n\n";
        assert!(KEYED_BEHAVIOR.contains(helper));
        // `helper` moved below `read`, which calls it: the same texts.
        let moved = KEYED_BEHAVIOR.replace(helper, "").replace(
            "definition{name: scale,",
            &format!("{helper}definition{{name: scale,"),
        );
        let (_, before, _) = declarations(KEYED_BEHAVIOR);
        let (_, after, order) = declarations(&moved);
        assert_eq!(before, after);
        assert_eq!(order, ["Reading", "read", "helper", "scale", "spare"]);
        let now = at("moved", &moved);
        assert_ne!(now["read"], base["read"]);
        assert_eq!(now["scale"], base["scale"]);
        // A declaration the closure does not hold moving changes nothing.
        let spare = "definition{name: spare, parameters: [x : Nat], result: Nat, body: x}\n\n";
        let first = KEYED_BEHAVIOR.replace(spare, "").replace(
            "\\begin{semanticmodule}\n",
            &format!("\\begin{{semanticmodule}}\n{spare}"),
        );
        assert_eq!(at("spare", &first), base);
        let _ = std::fs::remove_dir_all(tmp);
    }

    // The planted defect: a carry that does not verify the project carries
    // both roots into a project whose theorem is false.
    #[test]
    fn keyed_a_root_is_carried_only_into_a_project_that_verifies() {
        let tmp = scratch("keyed-project");
        let targets = keyed_targets("r1");
        let base = keyed_ids(
            &tmp,
            "base",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &targets,
        );
        let sources = tmp.join("prior");
        let mut manifest = keyed_prior(&sources, &base);
        // A false theorem no root reaches: no root's identity moves, the
        // project's does.
        let theorem = KEYED_BEHAVIOR.replace(
            "\\end{semanticmodule}",
            "theorem{name: scaleIsZero, statement: ((scale 1) = 0), proof: reflexivity{}}\n\\end{semanticmodule}",
        );
        let ids = keyed_ids(
            &tmp,
            "false",
            &theorem,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &targets,
        );
        assert_eq!(ids, base);
        let (dir, register) = (tmp.join("false"), keyed_register());
        let project = keyed_project(&dir);
        assert_ne!(project, keyed_project(&tmp.join("base")));
        let decide = |manifest: &Manifest| {
            keyed_decide(
                &dir,
                &register,
                &Prior {
                    manifest,
                    sources: std::slice::from_ref(&sources),
                    verify: keyed_never_verified,
                },
            )
        };
        // The decision verifies nothing: both roots are candidates, and the
        // plan counts them carried, as an export that verifies carries them.
        let candidates = decide(&manifest);
        assert_eq!((candidates.carried.len(), candidates.verified), (2, false));
        let plan = plan_keyed(
            &keyed_profile().roots(&register),
            Some(&candidates),
            &manifest,
            u64::MAX / 2,
        );
        assert_eq!((plan.carried, plan.build), (Some(2), 0));
        // The project the prior export verified is known to verify.
        manifest.project = Some(project.clone());
        let known = decide(&manifest);
        assert_eq!((known.carried.len(), known.verified), (2, true));
        assert_eq!(known.project, project);
        // A module no root imports, and the project's lock, are verified
        // by a build: either moves the project; the register does not.
        let other = dir.join("src").join("Quality.lex.tex");
        std::fs::write(&other, "\\begin{lexlean}{Quality}\n\\end{lexlean}\n").expect("write");
        assert_ne!(keyed_project(&dir), project);
        std::fs::remove_file(&other).expect("remove");
        assert_eq!(keyed_project(&dir), project);
        let lock = dir.join("lexlean.lock");
        let locked = std::fs::read_to_string(&lock).expect("the lock");
        std::fs::write(&lock, format!("{locked}\n# relocked\n")).expect("write");
        assert_ne!(keyed_project(&dir), project);
        std::fs::write(&lock, locked).expect("restore");
        std::fs::write(dir.join("frontier.toml"), "# moved\n").expect("write");
        // The export, of a project no export verified.
        manifest.project = None;
        let profile = keyed_profile();
        let input = Input {
            profile: &profile,
            projection: &dir,
            register: &register,
            targets: &targets,
        };
        let export = |manifest: &Manifest, verify: Verifier| {
            production_build(
                &input,
                &tmp.join("out"),
                &tmp.join("kept"),
                &Prior {
                    manifest,
                    sources: std::slice::from_ref(&sources),
                    verify,
                },
                &keyed_compiler(KEYED_COMPILER),
                Scope::default(),
            )
        };
        // An export that builds nothing verifies the project once, with one
        // root marked; when it refuses, the carried roots are built (and no
        // compiler is found here), so nothing is published unverified.
        assert!(export(&manifest, keyed_refuses).is_err());
        let done = export(&manifest, keyed_verifies_one).expect("an export");
        assert_eq!((done.carried, done.builds), (2, 0));
        let kept = std::fs::read_to_string(tmp.join("kept").join(PROJECT)).expect("the project");
        assert_eq!(kept, format!("identity = \"{}\"\n", keyed_project(&dir)));
        // Verified by the prior export, it is not verified again.
        let mut verified = manifest.clone();
        verified.project = Some(keyed_project(&dir));
        let steady = export(&verified, keyed_never_verified).expect("a steady export");
        assert_eq!((steady.carried, steady.builds), (2, 0));
        let _ = std::fs::remove_dir_all(tmp);
    }

    // The planted defect: a sync that takes the edition from built bundles
    // only writes a crate of no edition (Rust 2015) for carried packages.
    #[test]
    fn keyed_an_all_carried_sync_writes_the_edition_its_packages_were_built_for() {
        let tmp = scratch("keyed-edition");
        let targets = keyed_targets("r1");
        let ids = keyed_ids(
            &tmp,
            "p",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &targets,
        );
        let sources = tmp.join("prior");
        let mut manifest = keyed_prior(&sources, &ids);
        let (profile, register) = (keyed_profile(), keyed_register());
        let projection = tmp.join("p");
        let input = Input {
            profile: &profile,
            projection: &projection,
            register: &register,
            targets: &targets,
        };
        let kept = tmp.join("kept");
        let done = production_build(
            &input,
            &tmp.join("out"),
            &kept,
            &Prior {
                manifest: &manifest,
                sources: std::slice::from_ref(&sources),
                verify: keyed_verifies,
            },
            &keyed_compiler(KEYED_COMPILER),
            Scope::default(),
        )
        .expect("an export");
        assert_eq!((done.carried, done.builds), (2, 0));
        let dir = tmp.join("crate");
        let synced = sync(
            &profile,
            &Sync {
                kept: &kept,
                dir: &dir,
                register: &register,
                projection: None,
                adapters: None,
                mir: None,
            },
            &profile.roots(&register),
            None,
            "0.4.0",
        )
        .expect("a sync");
        let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).expect("a Cargo manifest");
        assert!(cargo.contains("edition = \"2021\""), "{cargo}");
        assert!(synced
            .roots
            .iter()
            .all(|r| r.edition.as_deref() == Some("2021")));
        // A prior that does not record the edition carries nothing.
        for r in &mut manifest.roots {
            r.edition = None;
        }
        let none = carried(
            &profile.roots(&register),
            &ids,
            &Prior {
                manifest: &manifest,
                sources: std::slice::from_ref(&sources),
                verify: keyed_verifies,
            },
        );
        assert_eq!(none.map(|c| c.len()), Ok(0));
        let _ = std::fs::remove_dir_all(tmp);
    }

    // The planted defect: a plan or an export that counts only the prior
    // compiler's refusals misses the marking's.
    #[test]
    fn keyed_the_plan_counts_the_markings_refusals_as_the_export_does() {
        let tmp = scratch("keyed-marked");
        let targets = keyed_targets("r1");
        let behavior = KEYED_BEHAVIOR.replace(
            "\\end{semanticmodule}",
            "definition{name: applyBoth, parameters: [f : (Fn Nat Nat), x : Nat], result: Nat, body: f x}\n\\end{semanticmodule}",
        );
        let ids = keyed_ids(
            &tmp,
            "p",
            &behavior,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &targets,
        );
        let sources = tmp.join("prior");
        let manifest = keyed_prior(&sources, &ids);
        let mut register = keyed_register();
        register.rows.insert(
            "k3".to_string(),
            crate::register::Row {
                function: "meter::apply_both".to_string(),
                status: Status::Lifted("Behavior::applyBoth".into()),
                divergence: None,
            },
        );
        let profile = keyed_profile();
        let roots = profile.roots(&register);
        assert_eq!(roots.len(), 3);
        let projection = tmp.join("p");
        let prior = Prior {
            manifest: &manifest,
            sources: std::slice::from_ref(&sources),
            verify: keyed_verifies,
        };
        let decided = keyed_decide(&projection, &register, &prior);
        assert_eq!(
            decided
                .marked
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["applyBoth"]
        );
        let plan = plan_keyed(&roots, Some(&decided), &manifest, u64::MAX / 2);
        assert_eq!((plan.carried, plan.build, plan.refused), (Some(2), 0, 1));
        let input = Input {
            profile: &profile,
            projection: &projection,
            register: &register,
            targets: &targets,
        };
        let done = production_build(
            &input,
            &tmp.join("out"),
            &tmp.join("kept"),
            &prior,
            &keyed_compiler(KEYED_COMPILER),
            Scope::default(),
        )
        .expect("an export");
        assert_eq!(
            done,
            Exported {
                carried: 2,
                built: 0,
                builds: 0,
                refused: 1
            }
        );
        let _ = std::fs::remove_dir_all(tmp);
    }

    // keyed2: one key after the merge of main's closure κ. A manifest of
    // either past shape is read: the first keyed export's (`identity` rows)
    // is carried, since its key is this one under its first name; main's
    // (`closure` rows of its own tag and text) never equals this key, so
    // its roots are built once.
    #[test]
    fn keyed2_a_manifest_of_either_past_shape_is_read_and_only_this_key_carries() {
        let tmp = scratch("keyed2-shapes");
        let targets = keyed_targets("r1");
        let ids = keyed_ids(
            &tmp,
            "p",
            KEYED_BEHAVIOR,
            KEYED_PRELUDE,
            KEYED_COMPILER,
            &targets,
        );
        let sources = tmp.join("prior");
        let written = keyed_prior(&sources, &ids).to_text();
        assert_eq!(written.matches("\nclosure = ").count(), 2);
        let path = tmp.join("manifest.toml");
        let carry = |text: &str| {
            let manifest = Manifest::parse(&path, text).expect("a manifest of a past shape");
            carried(
                &keyed_profile().roots(&keyed_register()),
                &ids,
                &Prior {
                    manifest: &manifest,
                    sources: std::slice::from_ref(&sources),
                    verify: keyed_verifies,
                },
            )
            .expect("carried")
            .len()
        };
        assert_eq!(carry(&written), 2);
        let first = written.replace("\nclosure = ", "\nidentity = ");
        assert_eq!(carry(&first), 2);
        // Main's shape: each root's closure κ of main's own text.
        let mut main = written.clone();
        for id in ids.values() {
            let theirs =
                crate::project::sha(format!("uorust/certified-closure/1\n{id}").as_bytes());
            main = main.replace(id.as_str(), &theirs);
        }
        assert_eq!(carry(&main), 0);
        // Rewritten, a manifest keeps the key under this name only.
        let read = Manifest::parse(&path, &first).expect("a manifest");
        assert_eq!(read.to_text(), written);
        let _ = std::fs::remove_dir_all(tmp);
    }

    // Class keyed, the refuter's transitive case: a module that only another
    // module's import reaches.
    #[test]
    fn keyed_a_module_reached_through_another_modules_import_is_in_every_roots_identity() {
        let tmp = scratch("keyed-transitive");
        let targets = keyed_targets("r1");
        let behavior = KEYED_BEHAVIOR.replace(
            "\\importmodule{Prelude}\n",
            "\\importmodule{Prelude}\n\\importmodule{Helpers}\n",
        );
        assert_ne!(behavior, KEYED_BEHAVIOR);
        let ids = |name: &str, deep: &str, apart: &str| {
            let dir = tmp.join(name);
            keyed_projection(&dir, &behavior, KEYED_PRELUDE);
            let write = |module: &str, text: &str| {
                std::fs::write(dir.join("src").join(format!("{module}.lex.tex")), text)
                    .expect("a module");
            };
            write(
                "Helpers",
                "\\begin{lexlean}{Helpers}\n\\importmodule{Deep}\n\\end{lexlean}\n",
            );
            write("Deep", deep);
            write("Apart", apart);
            let (profile, register) = (keyed_profile(), keyed_register());
            let input = Input {
                profile: &profile,
                projection: &dir,
                register: &register,
                targets: &targets,
            };
            closure_kappas(
                &input,
                &profile.roots(&register),
                &keyed_compiler(KEYED_COMPILER),
            )
            .expect("identities")
        };
        // Deep imports Helpers back, so the walk must end on a cycle.
        let and = "\\begin{lexlean}{Deep}\n\\importmodule{Helpers}\ncore a b = (a && b)\n\\end{lexlean}\n";
        let base = ids("base", and, "one\n");
        assert!(!base.is_empty());
        assert_eq!(ids("again", and, "one\n"), base);
        // Helpers alone imports Deep: its body is in every root's identity.
        let or = and.replace("&&", "||");
        for (def, id) in &ids("or", &or, "one\n") {
            assert_ne!(
                &base[def], id,
                "{def} keeps its identity though Deep changed"
            );
        }
        // A module no import reaches is the project's identity's, not a root's.
        assert_eq!(ids("apart", and, "two\n"), base);
        let _ = std::fs::remove_dir_all(tmp);
    }

    // keyed2 (item 0): a test states its compiler. The stand-in is a
    // program that exists nowhere, so a build the test does not expect fails
    // to start wherever the suite runs, whether or not a real `lexlean` is
    // on the path; what the compiler states of itself is the test's own.
    fn keyed_compiler(stated: &str) -> Compiler {
        Compiler {
            stated: stated.to_string(),
            program: PathBuf::from("/nonexistent/uorust-test-stand-in/lexlean"),
        }
    }
}
