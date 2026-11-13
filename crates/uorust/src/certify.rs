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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::io;
use crate::mir::ir::{IrAggregate, IrConstant, IrOperand, IrRvalue, IrStatement, IrTerminator};
use crate::mir::{Dumps, IrBody};
use crate::register::{Register, Status};
use crate::UorustError;

/// The modules of the translator, by their crate paths.
pub const SCOPE: &[&str] = &["uorust::mir::", "uorust::lift::", "uorust::lex::"];

/// The translate entry: the function that translates the bodies of one
/// module of a library, from their MIR text to their LexLean and κ rows.
pub const ENTRY: &str = "uorust::mir::core::core_shard";

/// The schema of the manifest [`sync`] writes.
pub const SCHEMA: &str = "uorust/certified/1";

/// The prefix of every certified package's name.
pub const PACKAGE: &str = "uorust_certified_";

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
}

impl Root {
    /// The certified package's name: [`PACKAGE`] and the definition in
    /// snake case.
    pub fn package(&self) -> String {
        let mut out = PACKAGE.to_string();
        for c in self.definition.chars() {
            if c.is_ascii_uppercase() {
                out.push('_');
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// The production roots of a register: every lifted translator function, by
/// definition.
pub fn roots(register: &Register) -> Vec<Root> {
    let mut out: Vec<Root> = register
        .rows
        .iter()
        .filter(|(_, row)| in_scope(&row.function))
        .filter_map(|(kappa, row)| match &row.status {
            Status::Lifted(definition) => Some(Root {
                function: row.function.clone(),
                kappa: kappa.clone(),
                definition: definition
                    .rsplit("::")
                    .next()
                    .unwrap_or(definition)
                    .to_string(),
            }),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.definition.cmp(&b.definition));
    out
}

/// Write the production project: the projection at `projection` with every
/// root's definition marked as a production root, and `request.json`, the
/// `lexlean/production-request/1` document naming one package per root at
/// the `rust-std` descriptor of `targets` (the output of `lexlean production
/// targets`). Returns the number of roots.
pub fn production(
    projection: &Path,
    out: &Path,
    register: &Register,
    targets: &str,
    compiler: &BTreeMap<String, String>,
) -> Result<usize, UorustError> {
    let bad = |path: &Path, reason: String| UorustError::Malformed {
        path: path.to_path_buf(),
        reason,
    };
    let descriptors: serde_json::Value =
        serde_json::from_str(targets).map_err(|e| bad(Path::new("targets.json"), e.to_string()))?;
    let digest = descriptors
        .get("targets")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
        .find(|t| t.get("id").and_then(|i| i.as_str()) == Some(TARGET))
        .and_then(|t| t.get("digest"))
        .and_then(|d| d.as_str())
        .ok_or_else(|| bad(Path::new("targets.json"), format!("no {TARGET} descriptor")))?
        .to_string();
    let prefix = module_prefix(projection)?;
    copy_tree(projection, out)?;
    let roots = roots(register);
    let names: BTreeSet<&str> = roots.iter().map(|r| r.definition.as_str()).collect();
    let mut modules = BTreeMap::new();
    for module in [BEHAVIOR, "Prelude"] {
        let path = out.join("src").join(format!("{module}.lex.tex"));
        let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        modules.insert(module.to_string(), text);
    }
    let exported: BTreeSet<&str> = names
        .iter()
        .copied()
        .filter(|n| !compiler.contains_key(*n))
        .collect();
    let (marked, found, mut refused) = mark(&modules, &exported);
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
    let mut entries: Vec<(String, serde_json::Value)> = roots
        .iter()
        .filter(|r| !refused.contains_key(&r.definition))
        .map(|r| {
            let root = format!("{prefix}.Behavior.{}", r.definition);
            let entry = serde_json::json!({
                "package": {"name": r.package(), "version": "0.1.0"},
                "root": root,
                "targets": [{"digest": digest, "id": TARGET}],
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
    Ok(roots.len() - refused.len())
}

/// Export the translator: write the production project and build it with
/// `lexlean production build`, refusing each root the compiler finds
/// ineligible with its diagnostic and building again until the build
/// publishes, then replay the bundle with `verify-artifact` and keep under
/// `kept` what [`sync`] reads of it: its result, its package sources and the
/// refusals. The project and the build may live on a scratch volume.
pub fn production_build(
    projection: &Path,
    out: &Path,
    register: &Register,
    targets: &str,
    kept: &Path,
) -> Result<(), UorustError> {
    let prefix = format!("{}.{BEHAVIOR}.", module_prefix(projection)?);
    let mut compiler: BTreeMap<String, String> = BTreeMap::new();
    loop {
        if out.exists() {
            std::fs::remove_dir_all(out).map_err(|e| io(out, e))?;
        }
        production(projection, out, register, targets, &compiler)?;
        lexlean(out, &["lock"])?;
        let result = lexlean(out, &["production", "build", "--request", "request.json"])?;
        if result.get("success").and_then(serde_json::Value::as_bool) == Some(true) {
            let bundle = result
                .get("artifacts")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|a| a.as_str())
                .map(|a| out.join(a))
                .ok_or_else(|| UorustError::Tool("lexlean production build: no bundle".into()))?;
            let replay = lexlean(
                out,
                &["production", "verify-artifact", &bundle.to_string_lossy()],
            )?;
            if replay.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
                return Err(UorustError::Tool(format!(
                    "lexlean production verify-artifact: {}",
                    diagnostics(&replay).join("; ")
                )));
            }
            keep(&bundle, kept)?;
            let refusals = out.join(REFUSALS);
            std::fs::copy(&refusals, kept.join(REFUSALS)).map_err(|e| io(&refusals, e))?;
            return Ok(());
        }
        let messages = diagnostics(&result);
        let named: Vec<(String, String)> = messages
            .iter()
            .filter_map(|m| first_root(m, &prefix).map(|r| (r, m.clone())))
            .filter(|(r, _)| !compiler.contains_key(r))
            .collect();
        if named.is_empty() {
            return Err(UorustError::Tool(format!(
                "lexlean production build: {}",
                messages.join("; ")
            )));
        }
        compiler.extend(named);
    }
}

/// Keep what [`sync`] reads of a bundle under `to`: its `result.json` and
/// every package's `src/lib.rs`, at their paths in the bundle.
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
    let run = std::process::Command::new("lexlean")
        .args(args)
        .args(["--diagnostic-format", "json"])
        .current_dir(dir)
        .output()
        .map_err(|e| UorustError::Tool(format!("lexlean {}: {e}", args.join(" "))))?;
    let text = String::from_utf8_lossy(&run.stdout);
    let last = text.lines().last().unwrap_or_default();
    serde_json::from_str(last).map_err(|e| {
        UorustError::Tool(format!(
            "lexlean {}: {e}: {}",
            args.join(" "),
            String::from_utf8_lossy(&run.stderr)
        ))
    })
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

/// The modules' texts, by module name, with every root (a definition of
/// `Behavior`) a production root and every definition a root reaches
/// executable, the roots found, and the roots refused with their causes.
pub(crate) fn mark(
    modules: &BTreeMap<String, String>,
    roots: &BTreeSet<&str>,
) -> (
    BTreeMap<String, String>,
    BTreeSet<String>,
    BTreeMap<String, String>,
) {
    let cut: BTreeMap<&String, Vec<Chunk>> =
        modules.iter().map(|(m, text)| (m, chunks(text))).collect();
    // Every definition by its qualified name, `Module::name`, with its text.
    let mut defs: BTreeMap<String, &str> = BTreeMap::new();
    for (module, chunks) in &cut {
        for c in chunks {
            if let Some(name) = &c.name {
                defs.insert(format!("{module}::{name}"), &c.text);
            }
        }
    }
    // Each root's closure: the definitions its body reaches.
    let refs = |def: &str| -> Vec<String> {
        let Some(text) = defs.get(def) else {
            return Vec::new();
        };
        let module = def.split("::").next().unwrap_or_default();
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .map(|token| {
                if token.contains("::") {
                    token.to_string()
                } else {
                    format!("{module}::{token}")
                }
            })
            .filter(|key| key != def && defs.contains_key(key))
            .collect()
    };
    let closure = |start: &str| {
        let mut reached: BTreeSet<String> = BTreeSet::new();
        let mut work = refs(start);
        work.push(start.to_string());
        while let Some(def) = work.pop() {
            let Some(text) = defs.get(&def) else {
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
                if defs.contains_key(&key) && !reached.contains(&key) {
                    work.push(key);
                }
            }
        }
        reached
    };
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
                    "production: {{effects: [{}], targets: [{TARGET}]}}",
                    EFFECTS.join(", ")
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
}

/// The certified translator: the bundle it came from, its roots, and the
/// figures of the bootstrap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    /// The bundle's result id.
    pub result: String,
    /// The LexLean compiler's version.
    pub compiler: String,
    /// The certified roots, by package.
    pub roots: Vec<Certified>,
    /// The figures, as [`Share`] computes them.
    pub share: Share,
    /// The roots the production project could not export, by κ, with
    /// their causes.
    pub refused: BTreeMap<String, String>,
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
        let doc: toml::Value = text.parse().map_err(|e| bad(format!("{e}")))?;
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
            refused,
            result: text_of(&doc, "result")?,
            compiler: text_of(&doc, "compiler")?,
            roots,
            share: Share {
                translator: count("translator")?,
                certified: count("certified")?,
                reachable: count("reachable")?,
                reachable_certified: count("reachable_certified")?,
            },
        })
    }

    /// The manifest's text.
    pub fn to_text(&self) -> String {
        let s = &self.share;
        let mut out = String::new();
        out.push_str(
            "# The certified translator, written by `uorust certify sync` (just certified):\n\
             # every lifted translator function exported by `lexlean production` and\n\
             # kernel-checked, keyed by the κ of the Rust function it certifies.\n",
        );
        out.push_str(&format!("schema = \"{SCHEMA}\"\n"));
        out.push_str(&format!("result = \"{}\"\n", self.result));
        out.push_str(&format!("compiler = \"{}\"\n", self.compiler));
        out.push_str(&format!("entry = \"{ENTRY}\"\n\n"));
        out.push_str("# The bootstrap: the share of the translator that runs certified.\n");
        out.push_str("[share]\n");
        out.push_str(&format!("translator = {}\n", s.translator));
        out.push_str(&format!("certified = {}\n", s.certified));
        out.push_str(&format!("reachable = {}\n", s.reachable));
        out.push_str(&format!(
            "reachable_certified = {}\n",
            s.reachable_certified
        ));
        for r in &self.roots {
            out.push_str("\n[[root]]\n");
            out.push_str(&format!("function = \"{}\"\n", r.function));
            out.push_str(&format!("kappa = \"{}\"\n", r.kappa));
            out.push_str(&format!("definition = \"{}\"\n", r.definition));
            out.push_str(&format!("package = \"{}\"\n", r.package));
            out.push_str(&format!("source = \"{}\"\n", r.source));
            out.push_str(&format!("certification = \"{}\"\n", r.certification));
        }
        for (kappa, cause) in &self.refused {
            out.push_str("\n[[refused]]\n");
            out.push_str(&format!("kappa = \"{kappa}\"\n"));
            out.push_str(&format!("cause = {}\n", toml_quoted(cause)));
        }
        out
    }
}

/// Take the bundle at `bundle` into `dir`: each certified root's
/// `src/lib.rs` as `dir/roots/<package>.rs` (checked against the SHA-256 the
/// bundle's result records), the crate that compiles them, and the manifest,
/// with the share over `register` and the call graph of `dumps`.
pub(crate) fn sync(
    bundle: &Path,
    production: &Path,
    dir: &Path,
    adapters: &Path,
    register: &Register,
    dumps: &Dumps,
) -> Result<Manifest, UorustError> {
    let bad = |path: &Path, reason: String| UorustError::Malformed {
        path: path.to_path_buf(),
        reason,
    };
    let path = bundle.join("result.json");
    let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
    let result: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| bad(&path, e.to_string()))?;
    let field = |v: &serde_json::Value, keys: &[&str]| {
        let mut at = v;
        for k in keys {
            at = at.get(k).unwrap_or(&serde_json::Value::Null);
        }
        at.as_str().unwrap_or_default().to_string()
    };
    let by_package: BTreeMap<String, Root> = roots(register)
        .into_iter()
        .map(|r| (r.package(), r))
        .collect();
    let roots_dir = dir.join("roots");
    if roots_dir.is_dir() {
        std::fs::remove_dir_all(&roots_dir).map_err(|e| io(&roots_dir, e))?;
    }
    std::fs::create_dir_all(&roots_dir).map_err(|e| io(&roots_dir, e))?;
    let mut certified = Vec::new();
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
                return Err(bad(&path, format!("{name} is no root of the register")));
            };
            let lib = format!("{directory}/src/lib.rs");
            let recorded = package
                .get("files")
                .and_then(|f| f.as_array())
                .into_iter()
                .flatten()
                .find(|f| field(f, &["path"]) == lib)
                .map(|f| field(f, &["sha256"]))
                .ok_or_else(|| bad(&path, format!("{lib} is not in the result")))?;
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
                package: name,
                source: digest,
                certification: field(output, &["certification", "status"]),
            });
        }
    }
    certified.sort_by(|a, b| a.package.cmp(&b.package));
    let lib = crate_source(&certified);
    let lib_path = dir.join("src").join("lib.rs");
    std::fs::create_dir_all(dir.join("src")).map_err(|e| io(dir, e))?;
    std::fs::write(&lib_path, lib).map_err(|e| io(&lib_path, e))?;
    let empty = BTreeMap::new();
    let bodies = dumps.crates.get("uorust").unwrap_or(&empty);
    let mut wired = BTreeMap::new();
    for c in &certified {
        let source = std::fs::read_to_string(roots_dir.join(format!("{}.rs", c.package)))
            .map_err(|e| io(&roots_dir, e))?;
        let path = c.function.strip_prefix("uorust::").unwrap_or(&c.function);
        if let Some((name, code)) = bodies.get(path).and_then(|b| adapter(c, b, &source)) {
            wired.insert(name, code);
        }
    }
    let mut sites = BTreeMap::new();
    if let Some(src) = adapters.parent() {
        call_sites(src, adapters, &mut sites)?;
    }
    let text = adapters_source(&wired, &sites);
    std::fs::write(adapters, text).map_err(|e| io(adapters, e))?;
    let kappas: BTreeSet<String> = certified.iter().map(|c| c.kappa.clone()).collect();
    let refusals = production.join(REFUSALS);
    let text = std::fs::read_to_string(&refusals).map_err(|e| io(&refusals, e))?;
    let doc: toml::Value = text.parse().map_err(|e| bad(&refusals, format!("{e}")))?;
    let refused: BTreeMap<String, String> = doc
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
    let manifest = Manifest {
        refused,
        result: field(&result, &["result_id"]),
        compiler: field(&result, &["compiler", "version"]),
        roots: certified,
        share: share(register, &kappas, dumps),
    };
    let manifest_path = dir.join("manifest.toml");
    std::fs::write(&manifest_path, manifest.to_text()).map_err(|e| io(&manifest_path, e))?;
    Ok(manifest)
}

/// [`sync`] of the bundle [`build`] kept at `kept`, with the dumps read from
/// `mir` of the workspace at `workspace`.
pub fn sync_paths(
    kept: &Path,
    dir: &Path,
    adapters: &Path,
    register: &Register,
    workspace: &Path,
    mir: &Path,
) -> Result<Manifest, UorustError> {
    let dumps = crate::mir::load(workspace, mir)?;
    sync(kept, kept, dir, adapters, register, &dumps)
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

#![allow(dead_code)]

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
pub(crate) fn adapter(root: &Certified, body: &IrBody, source: &str) -> Option<(String, String)> {
    let (params, result) = run_signature(source)?;
    if params.len() != body.arity {
        return None;
    }
    let m = format!(
        "uorust_certified::{}",
        root.package.strip_prefix(PACKAGE).unwrap_or(&root.package)
    );
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
        .strip_prefix("uorust::")
        .unwrap_or(&root.function)
        .replace("::", "_");
    let code = format!(
        "/// `{}`, certified as `Behavior::{}`.\n#[allow(clippy::all)]\npub(crate) fn {name}({}) -> Option<{rust_result}> {{\n    Some({value})\n}}\n",
        root.function,
        root.definition,
        args.join(", ")
    );
    Some((name, code))
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

/// The certified crate's `lib.rs`: one module per root, its file the
/// package source byte for byte.
pub(crate) fn crate_source(roots: &[Certified]) -> String {
    let mut out = String::from(
        "//! The certified translator: the Rust `lexlean production` exported from the\n\
         //! lifted translator's definitions, one module per production root, each file\n\
         //! the certified package's `src/lib.rs` byte for byte. Written by `uorust\n\
         //! certify sync`; `crates/certified/manifest.toml` keys each by the κ of the\n\
         //! Rust function it certifies.\n\n\
         #![forbid(unsafe_code)]\n",
    );
    for r in roots {
        out.push_str(&format!(
            "\n/// `{}`, certified as `Behavior::{}`.\n#[rustfmt::skip]\n#[allow(missing_docs, dead_code, clippy::all)]\n#[path = \"../roots/{}.rs\"]\npub mod {};\n",
            r.function,
            r.definition,
            r.package,
            r.package.strip_prefix(PACKAGE).unwrap_or(&r.package)
        ));
    }
    out
}

/// The figures of the bootstrap over `register`, a function being certified
/// when its κ is in `certified`.
pub(crate) fn share(register: &Register, certified: &BTreeSet<String>, dumps: &Dumps) -> Share {
    let reach = reachable(dumps);
    // (reachable, certified) of each translator function; a root certifies
    // its κ only while the register lifts it.
    let rows: Vec<(bool, bool)> = register
        .rows
        .iter()
        .filter(|(_, r)| in_scope(&r.function))
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

/// The functions of the `uorust` crate the translate entry reaches, by crate
/// path, over the calls, the closures and the function items of their MIR.
pub(crate) fn reachable(dumps: &Dumps) -> BTreeSet<String> {
    let krate = ENTRY.split("::").next().unwrap_or_default();
    let empty = BTreeMap::new();
    let bodies = dumps.crates.get(krate).unwrap_or(&empty);
    let closures = dumps.closures.get(krate).unwrap_or(&empty);
    let prefix = format!("{krate}::");
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut work: Vec<&IrBody> = Vec::new();
    let mut closures_seen: BTreeSet<&str> = BTreeSet::new();
    if let Some(entry) = bodies.get(ENTRY.strip_prefix(&prefix).unwrap_or(ENTRY)) {
        seen.insert(ENTRY.to_string());
        work.push(entry);
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
    let certified = manifest.kappas();
    register
        .rows
        .iter()
        // A root certifies its κ only while the register lifts it.
        .filter(|(k, r)| {
            in_scope(&r.function)
                && !(certified.contains(*k) && matches!(r.status, Status::Lifted(_)))
        })
        .map(|(k, r)| {
            let cause = match (&r.status, manifest.refused.get(k)) {
                (Status::Lifted(d), Some(why)) => format!("lifted as {d}, not exported: {why}"),
                (Status::Lifted(d), None) => format!("lifted as {d}, not yet certified"),
                (Status::Residue(reason, detail), _) => format!("residue ({reason}): {detail}"),
                (Status::Foreign(detail), _) => format!("foreign: {detail}"),
                (Status::Effect(effect), _) => format!("effect: {effect}"),
            };
            (r.function.clone(), cause)
        })
        .collect()
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
    let now = uncertified(head, head_manifest);
    let before = uncertified(base, base_manifest);
    if now.len() <= before.len() {
        return Ok(now.len());
    }
    let mut rows = vec![format!(
        "{} uncertified translator functions, {} in the parent",
        now.len(),
        before.len()
    )];
    rows.extend(
        now.iter()
            .filter(|(f, _)| !before.contains_key(*f))
            .map(|(f, cause)| format!("{f}: {cause}")),
    );
    Err(UorustError::Grown(rows))
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
            roots: vec![Certified {
                function: "uorust::mir::coverage::binop".into(),
                kappa: "k".into(),
                definition: "binop".into(),
                package: "uorust_certified_binop".into(),
                source: "s".into(),
                certification: "kernel-checked".into(),
            }],
            share: Share {
                translator: 3,
                certified: 1,
                reachable: 2,
                reachable_certified: 1,
            },
            refused: [(
                "k2".to_string(),
                "reaches Behavior::walk, a \"function\"".to_string(),
            )]
            .into_iter()
            .collect(),
        };
        assert_eq!(Manifest::parse(Path::new("m"), &m.to_text()), Ok(m));
    }
}
