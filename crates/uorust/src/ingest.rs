//! Ingest: `cargo metadata`, the manifests, `Cargo.lock` and every source file,
//! parsed with `syn`, into [`facts`](crate::facts) with spans.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use quote::ToTokens;
use serde_json::Value;
use syn::punctuated::Punctuated;
use syn::visit::Visit;

use crate::error::{io, UorustError};
use crate::facts::{
    Advisories, Advisory, Capability, Dep, EnumFact, FnFact, LockedPackage, Package, SourceFile,
    Span, StructFact, Target, TargetKind, Variant, Workspace,
};

/// Read a workspace from its root directory.
pub(crate) fn workspace(root: &Path, advisory_db: Option<&Path>) -> Result<Workspace, UorustError> {
    let root = root.canonicalize().map_err(|e| io(root.to_path_buf(), e))?;
    let metadata = metadata(&root)?;
    let root_manifest = read_toml(&root.join("Cargo.toml"))?;
    let workspace_lints = lints_of(root_manifest.get("workspace").and_then(|w| w.get("lints")));
    let members: Vec<&Value> = metadata
        .get("packages")
        .and_then(Value::as_array)
        .map(|p| p.iter().collect())
        .unwrap_or_default();
    let mut packages = Vec::new();
    for member in members {
        packages.push(package(&root, member, &workspace_lints)?);
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let lock = lock(&root.join("Cargo.lock"))?;
    let deny = root.join("deny.toml");
    let deny_ignores = if deny.is_file() {
        ignores(&read_toml(&deny)?)
    } else {
        BTreeMap::new()
    };
    let advisories = match advisory_db {
        Some(db) => Some(advisories(db, &lock)?),
        None => None,
    };
    Ok(Workspace {
        name,
        lock,
        has_deny: deny.is_file(),
        deny_ignores,
        advisories,
        packages,
    })
}

/// `[advisories] ignore` of `deny.toml`: bare IDs, or `{ id, reason }`.
fn ignores(deny: &toml::Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let list = deny
        .get("advisories")
        .and_then(|a| a.get("ignore"))
        .and_then(toml::Value::as_array);
    for item in list.into_iter().flatten() {
        match item {
            toml::Value::String(id) => {
                out.insert(id.clone(), String::new());
            }
            toml::Value::Table(t) => {
                if let Some(id) = t.get("id").and_then(toml::Value::as_str) {
                    let reason = t.get("reason").and_then(toml::Value::as_str).unwrap_or("");
                    out.insert(id.to_string(), reason.to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// The advisories of a RustSec advisory database checkout (`crates/<name>/
/// RUSTSEC-*.md`) that affect a locked registry package's version.
fn advisories(db: &Path, lock: &[LockedPackage]) -> Result<Advisories, UorustError> {
    let crates = db.join("crates");
    if !crates.is_dir() {
        return Err(UorustError::Malformed {
            path: db.to_path_buf(),
            reason: "not a RustSec advisory database: no crates directory".to_string(),
        });
    }
    // The checkout's commit, when the database is a repository of its own;
    // a directory inside another repository has no revision of its own.
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(db)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let own_repo = git(&["rev-parse", "--show-toplevel"])
        .and_then(|top| PathBuf::from(top).canonicalize().ok())
        .is_some_and(|top| db.canonicalize().is_ok_and(|d| d == top));
    let revision = own_repo
        .then(|| git(&["rev-parse", "HEAD"]))
        .flatten()
        .unwrap_or_else(|| "unversioned".to_string());
    let mut hits = Vec::new();
    for package in lock.iter().filter(|p| p.source.starts_with("registry+")) {
        let dir = crates.join(&package.name);
        if !dir.is_dir() {
            continue;
        }
        let Ok(version) = semver::Version::parse(&package.version) else {
            continue;
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| io(dir.clone(), e))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect();
        files.sort();
        for file in files {
            let text = std::fs::read_to_string(&file).map_err(|e| io(file.clone(), e))?;
            if let Some(advisory) = advisory(&file, &text, &package.name, &version)? {
                hits.push(advisory);
            }
        }
    }
    hits.sort_by(|a, b| (&a.id, &a.package, &a.version).cmp(&(&b.id, &b.package, &b.version)));
    Ok(Advisories { revision, hits })
}

/// One advisory file, when it applies to `version`.
fn advisory(
    file: &Path,
    text: &str,
    package: &str,
    version: &semver::Version,
) -> Result<Option<Advisory>, UorustError> {
    let malformed = |reason: &str| UorustError::Malformed {
        path: file.to_path_buf(),
        reason: reason.to_string(),
    };
    let body = text
        .strip_prefix("```toml")
        .ok_or_else(|| malformed("no TOML front matter"))?;
    let (front, rest) = body
        .split_once("\n```")
        .ok_or_else(|| malformed("unterminated TOML front matter"))?;
    let front: toml::Value = toml::from_str(front).map_err(|e| malformed(&e.to_string()))?;
    let section = front
        .get("advisory")
        .ok_or_else(|| malformed("no [advisory] table"))?;
    let text_of = |key: &str| {
        section
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_string)
    };
    if section.get("withdrawn").is_some() || text_of("package").as_deref() != Some(package) {
        return Ok(None);
    }
    let reqs = |key: &str| -> Vec<String> {
        front
            .get("versions")
            .and_then(|v| v.get(key))
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let patched = reqs("patched");
    let unaffected = reqs("unaffected");
    let spared = patched
        .iter()
        .chain(&unaffected)
        .any(|r| semver::VersionReq::parse(r).is_ok_and(|req| req.matches(version)));
    if spared {
        return Ok(None);
    }
    let title = rest
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(Some(Advisory {
        id: text_of("id").ok_or_else(|| malformed("no advisory id"))?,
        package: package.to_string(),
        version: version.to_string(),
        title,
        date: text_of("date").unwrap_or_default(),
        informational: text_of("informational"),
        categories: section
            .get("categories")
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        patched,
    }))
}

fn metadata(root: &Path) -> Result<Value, UorustError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| UorustError::CargoMetadata(e.to_string()))?;
    if !output.status.success() {
        return Err(UorustError::CargoMetadata(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| UorustError::Malformed {
        path: root.join("Cargo.toml"),
        reason: e.to_string(),
    })
}

fn read_toml(path: &Path) -> Result<toml::Value, UorustError> {
    let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
    text.parse::<toml::Value>()
        .map_err(|e| UorustError::Malformed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
}

fn lints_of(table: Option<&toml::Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(groups) = table.and_then(toml::Value::as_table) else {
        return out;
    };
    for (group, lints) in groups {
        let Some(lints) = lints.as_table() else {
            continue;
        };
        for (lint, level) in lints {
            let level = level
                .as_str()
                .map(str::to_string)
                .or_else(|| {
                    level
                        .get("level")
                        .and_then(toml::Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            out.insert(format!("{group}.{lint}"), level);
        }
    }
    out
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn package(
    root: &Path,
    meta: &Value,
    workspace_lints: &BTreeMap<String, String>,
) -> Result<Package, UorustError> {
    let manifest = PathBuf::from(text(meta, "manifest_path"));
    let dir_abs = manifest.parent().unwrap_or(root).to_path_buf();
    let dir = rel(root, &dir_abs);
    let raw = read_toml(&manifest)?;
    let lints = match raw.get("lints") {
        Some(table) if table.get("workspace").and_then(toml::Value::as_bool) == Some(true) => {
            workspace_lints.clone()
        }
        other => lints_of(other),
    };
    let mut targets = Vec::new();
    for target in meta
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kinds: Vec<&str> = target
            .get("kind")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let kind = if kinds.iter().any(|k| {
            matches!(
                *k,
                "lib" | "rlib" | "cdylib" | "staticlib" | "dylib" | "proc-macro"
            )
        }) {
            TargetKind::Lib
        } else if kinds.contains(&"bin") {
            TargetKind::Bin
        } else if kinds.contains(&"test") {
            TargetKind::Test
        } else if kinds.contains(&"example") {
            TargetKind::Example
        } else if kinds.contains(&"bench") {
            TargetKind::Bench
        } else {
            continue;
        };
        targets.push(Target {
            name: text(target, "name"),
            kind,
            src: rel(root, Path::new(&text(target, "src_path"))),
        });
    }
    targets
        .sort_by(|a, b| (a.src.as_str(), a.name.as_str()).cmp(&(b.src.as_str(), b.name.as_str())));
    let mut deps = Vec::new();
    for dep in meta
        .get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let path = dep.get("path").and_then(Value::as_str).map(PathBuf::from);
        let inside = path.as_ref().is_some_and(|p| p.starts_with(root));
        deps.push(Dep {
            name: text(dep, "name"),
            kind: dep
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("normal")
                .to_string(),
            path: path.map(|p| rel(root, &p)),
            inside,
        });
    }
    deps.sort_by(|a, b| {
        (a.name.as_str(), a.kind.as_str()).cmp(&(b.name.as_str(), b.kind.as_str()))
    });
    let mut files = Vec::new();
    let mut paths = Vec::new();
    for sub in ["src", "tests", "examples", "benches"] {
        collect_rs(&dir_abs.join(sub), &mut paths)?;
    }
    paths.sort();
    for path in paths {
        let rel_path = rel(root, &path);
        let Some((target, module)) = assign(&rel_path, &dir, &targets) else {
            continue;
        };
        files.push(source(
            &path,
            rel_path,
            target,
            module,
            &targets[target].kind,
        )?);
    }
    Ok(Package {
        name: text(meta, "name"),
        version: text(meta, "version"),
        description: text(meta, "description"),
        license: text(meta, "license"),
        rust_version: meta
            .get("rust_version")
            .and_then(Value::as_str)
            .map(str::to_string),
        authors: meta
            .get("authors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        dir,
        targets,
        deps,
        lints,
        files,
    })
}

/// Every `.rs` file under `dir`, not descending into a nested package or a
/// build directory.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), UorustError> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| io(dir, e))?;
    for entry in entries {
        let path = entry.map_err(|e| io(dir, e))?.path();
        if path.is_dir() {
            if path.join("Cargo.toml").exists()
                || path
                    .file_name()
                    .is_some_and(|n| n == "target" || n == "fixtures")
            {
                continue;
            }
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// The target a file belongs to and its module path inside that target.
fn assign(file: &str, dir: &str, targets: &[Target]) -> Option<(usize, String)> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let local = file.strip_prefix(&prefix)?;
    if let Some(index) = targets.iter().position(|t| t.src == file) {
        return Some((index, String::new()));
    }
    let parent_of = |t: &Target| {
        t.src
            .rsplit_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default()
    };
    // A file in a target root's own directory tree, the most specific root first.
    let mut best: Option<(usize, usize)> = None;
    for (index, target) in targets.iter().enumerate() {
        let base = parent_of(target);
        let in_tree = file.starts_with(&format!("{base}/"));
        if !in_tree {
            continue;
        }
        let shared_src = local.starts_with("src/") && base == format!("{prefix}src");
        // In `src/`, a bin root shares the tree with the library: the library owns it.
        let rank = base.len() * 4
            + match (&target.kind, shared_src) {
                (TargetKind::Lib, _) => 3,
                (TargetKind::Bin, true) => 0,
                _ => 2,
            };
        if best.is_none_or(|(_, r)| rank > r) {
            best = Some((index, rank));
        }
    }
    let (index, _) = best?;
    let base = parent_of(&targets[index]);
    let inner = file.strip_prefix(&format!("{base}/")).unwrap_or(file);
    let inner = inner.strip_suffix(".rs").unwrap_or(inner);
    let inner = inner.strip_suffix("/mod").unwrap_or(inner);
    Some((index, inner.replace('/', "::")))
}

fn lock(path: &Path) -> Result<Vec<LockedPackage>, UorustError> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let value = read_toml(path)?;
    let mut out: Vec<LockedPackage> = value
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| {
            let field = |k: &str| {
                p.get(k)
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            LockedPackage {
                name: field("name"),
                version: field("version"),
                source: field("source"),
                checksum: field("checksum"),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        (a.name.as_str(), a.version.as_str()).cmp(&(b.name.as_str(), b.version.as_str()))
    });
    Ok(out)
}

fn source(
    path: &Path,
    rel: String,
    target: usize,
    module: String,
    kind: &TargetKind,
) -> Result<SourceFile, UorustError> {
    let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
    let mut file = SourceFile {
        rel: rel.clone(),
        target,
        module,
        ..SourceFile::default()
    };
    let parsed = match syn::parse_file(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            let at = error.span().start();
            file.parse_error = Some(format!("{}:{}: {error}", rel, at.line));
            return Ok(file);
        }
    };
    file.doc = doc_of(&parsed.attrs);
    file.forbids_unsafe = parsed.attrs.iter().any(|a| {
        a.path().is_ident("forbid") && a.to_token_stream().to_string().contains("unsafe_code")
    });
    let product = matches!(kind, TargetKind::Lib | TargetKind::Bin);
    let lib = matches!(kind, TargetKind::Lib);
    let test_target = matches!(kind, TargetKind::Test);
    items(
        &parsed.items,
        &rel,
        None,
        test_target,
        product,
        lib,
        &mut file,
    );
    file.capabilities.sort();
    file.capabilities.dedup();
    Ok(file)
}

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.path().is_ident("cfg") && a.to_token_stream().to_string().contains("test"))
}

fn doc_of(attrs: &[syn::Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            {
                lines.push(s.value().trim().to_string());
            }
        }
    }
    lines.join("\n")
}

/// The first paragraph of a doc comment, on one line, without Markdown links.
pub(crate) fn summary(doc: &str) -> String {
    let para: Vec<&str> = doc
        .lines()
        .take_while(|l| !l.trim().is_empty())
        .map(str::trim)
        .collect();
    para.join(" ")
        .replace(['`', '[', ']'], "")
        .trim()
        .to_string()
}

fn doctests(doc: &str) -> usize {
    let mut count = 0;
    let mut open = false;
    for line in doc.lines() {
        let Some(info) = line.trim().strip_prefix("```") else {
            continue;
        };
        if open {
            open = false;
            continue;
        }
        open = true;
        let info = info.trim();
        if info.is_empty()
            || info
                .split(',')
                .all(|t| matches!(t.trim(), "rust" | "no_run" | "should_panic"))
        {
            count += 1;
        }
    }
    count
}

fn derives(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        if let Ok(paths) =
            attr.parse_args_with(Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        {
            for path in paths {
                if let Some(seg) = path.segments.last() {
                    out.push(seg.ident.to_string());
                }
            }
        }
    }
    out
}

fn line_of(span: proc_macro2::Span) -> usize {
    span.start().line
}

/// Collapse the token spacing of printed Rust to the way it is written.
pub(crate) fn tidy(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in [
        (" :: ", "::"),
        (":: ", "::"),
        (" ::", "::"),
        (" ,", ","),
        ("( ", "("),
        (" )", ")"),
        ("& ", "&"),
        (" <", "<"),
        ("< ", "<"),
        (" >", ">"),
        ("[ ", "["),
        (" ]", "]"),
        (" ;", ";"),
        (" :", ":"),
        ("- >", "->"),
        ("& 'static", "&'static"),
        ("' ", "'"),
        (" (", "("),
        ("->", " -> "),
        ("  ", " "),
    ] {
        out = out.replace(from, to);
    }
    out.trim().to_string()
}

#[allow(clippy::too_many_arguments)]
fn function(
    sig: &syn::Signature,
    block: Option<&syn::Block>,
    attrs: &[syn::Attribute],
    public: bool,
    owner: Option<String>,
    rel: &str,
    in_test: bool,
    lib: bool,
) -> FnFact {
    let doc = doc_of(attrs);
    let mut body = BodyScan::default();
    if let Some(block) = block {
        body.visit_block(block);
    }
    let test = attrs
        .iter()
        .any(|a| a.path().segments.last().is_some_and(|s| s.ident == "test"));
    FnFact {
        name: sig.ident.to_string(),
        owner,
        public,
        test,
        in_test_module: in_test,
        span: Span {
            file: rel.to_string(),
            line: line_of(sig.ident.span()),
        },
        doctests: if lib && public { doctests(&doc) } else { 0 },
        doc,
        signature: tidy(&sig.to_token_stream().to_string()),
        calls: body.calls,
        literals: body.literals,
        asserts: body.asserts,
        transitions: body.transitions,
        item: Some(sig.clone()),
        block: block.cloned(),
        in_trait: false,
        implements: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn items(
    list: &[syn::Item],
    rel: &str,
    _owner: Option<String>,
    in_test: bool,
    product: bool,
    lib: bool,
    file: &mut SourceFile,
) {
    for item in list {
        match item {
            syn::Item::Fn(f) => {
                let public = matches!(f.vis, syn::Visibility::Public(_));
                file.fns.push(function(
                    &f.sig,
                    Some(&f.block),
                    &f.attrs,
                    public,
                    None,
                    rel,
                    in_test,
                    lib,
                ));
            }
            syn::Item::Impl(imp) => {
                let self_ty = tidy(&imp.self_ty.to_token_stream().to_string());
                if let Some((path, _)) = &imp.trait_ {
                    if path.segments.last().is_some_and(|s| s.ident == "Error") {
                        file.error_impls.push(self_ty.clone());
                    }
                }
                let implements = imp
                    .trait_
                    .as_ref()
                    .and_then(|(path, _)| path.segments.last())
                    .map(|s| s.ident.to_string());
                for member in &imp.items {
                    if let syn::ImplItem::Fn(f) = member {
                        let public = matches!(f.vis, syn::Visibility::Public(_));
                        let mut fact = function(
                            &f.sig,
                            Some(&f.block),
                            &f.attrs,
                            public,
                            Some(self_ty.clone()),
                            rel,
                            in_test,
                            lib,
                        );
                        fact.implements.clone_from(&implements);
                        file.fns.push(fact);
                    }
                }
            }
            syn::Item::Trait(t) => {
                let public = matches!(t.vis, syn::Visibility::Public(_));
                for member in &t.items {
                    if let syn::TraitItem::Fn(f) = member {
                        let mut fact = function(
                            &f.sig,
                            f.default.as_ref(),
                            &f.attrs,
                            public,
                            Some(t.ident.to_string()),
                            rel,
                            in_test,
                            lib,
                        );
                        fact.in_trait = true;
                        file.fns.push(fact);
                    }
                }
            }
            syn::Item::Enum(e) => file.enums.push(EnumFact {
                name: e.ident.to_string(),
                span: Span {
                    file: rel.to_string(),
                    line: line_of(e.ident.span()),
                },
                derives: derives(&e.attrs),
                variants: e
                    .variants
                    .iter()
                    .map(|v| Variant {
                        name: v.ident.to_string(),
                        unit: matches!(v.fields, syn::Fields::Unit),
                        doc: doc_of(&v.attrs),
                        default: v.attrs.iter().any(|a| a.path().is_ident("default")),
                    })
                    .collect(),
            }),
            syn::Item::Struct(st) if st.generics.params.is_empty() => {
                if let syn::Fields::Named(named) = &st.fields {
                    file.structs.push(StructFact {
                        name: st.ident.to_string(),
                        span: Span {
                            file: rel.to_string(),
                            line: line_of(st.ident.span()),
                        },
                        fields: named
                            .named
                            .iter()
                            .filter_map(|f| Some((f.ident.as_ref()?.to_string(), f.ty.clone())))
                            .collect(),
                    });
                }
            }
            syn::Item::Use(u) => {
                let mut out = Vec::new();
                use_paths(&u.tree, &mut Vec::new(), &mut out);
                for path in out {
                    if let Some(last) = path.last() {
                        file.imports.insert(last.clone(), path.join("::"));
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    let test = in_test || is_cfg_test(&m.attrs);
                    items(inner, rel, None, test, product, lib, file);
                }
            }
            _ => {}
        }
        if product && !in_test && !matches!(item, syn::Item::Mod(_)) {
            let mut scan = CapabilityScan {
                rel,
                found: Vec::new(),
            };
            scan.visit_item(item);
            file.capabilities.extend(scan.found);
        }
    }
}

/// Calls, literals, assertions and state transitions in one body.
#[derive(Default)]
struct BodyScan {
    calls: Vec<String>,
    literals: Vec<String>,
    asserts: Vec<(String, Vec<syn::Expr>)>,
    transitions: Vec<(String, String, String)>,
}

fn enum_path(path: &syn::Path) -> Option<(String, String)> {
    let n = path.segments.len();
    if n < 2 {
        return None;
    }
    Some((
        path.segments[n - 2].ident.to_string(),
        path.segments[n - 1].ident.to_string(),
    ))
}

/// Every enum path an arm body can evaluate to, through `if`/`else` branches.
fn arm_targets<'e>(expr: &'e syn::Expr, out: &mut Vec<&'e syn::Path>) {
    match expr {
        syn::Expr::If(i) => {
            if let [syn::Stmt::Expr(e, None)] = i.then_branch.stmts.as_slice() {
                arm_targets(e, out);
            }
            if let Some((_, e)) = &i.else_branch {
                arm_targets(e, out);
            }
        }
        syn::Expr::Block(b) if b.block.stmts.len() == 1 => {
            if let syn::Stmt::Expr(e, None) = &b.block.stmts[0] {
                arm_targets(e, out);
            }
        }
        other => {
            if let Some(path) = arm_target(other) {
                out.push(path);
            }
        }
    }
}

fn arm_target(expr: &syn::Expr) -> Option<&syn::Path> {
    match expr {
        syn::Expr::Path(p) => Some(&p.path),
        syn::Expr::Block(b) if b.block.stmts.len() == 1 => match &b.block.stmts[0] {
            syn::Stmt::Expr(e, None) => arm_target(e),
            _ => None,
        },
        syn::Expr::Call(c) if c.args.len() == 1 => match &*c.func {
            syn::Expr::Path(p) if p.path.is_ident("Some") || p.path.is_ident("Ok") => {
                arm_target(&c.args[0])
            }
            _ => None,
        },
        syn::Expr::Paren(p) => arm_target(&p.expr),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for BodyScan {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*node.func {
            self.calls.push(tidy(&p.path.to_token_stream().to_string()));
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.calls.push(format!(".{}", node.method));
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_lit_str(&mut self, node: &'ast syn::LitStr) {
        self.literals.push(node.value());
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        let name = node
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if let Ok(args) =
            node.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        {
            if matches!(name.as_str(), "assert" | "assert_eq" | "assert_ne") {
                self.asserts
                    .push((name.clone(), args.iter().cloned().collect()));
            }
            for arg in &args {
                self.visit_expr(arg);
            }
        }
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        for arm in &node.arms {
            let from = match &arm.pat {
                syn::Pat::Path(p) => enum_path(&p.path),
                _ => None,
            };
            let mut targets = Vec::new();
            arm_targets(&arm.body, &mut targets);
            if let Some((e1, a)) = from {
                for (e2, b) in targets.into_iter().filter_map(enum_path) {
                    if e1 == e2 {
                        self.transitions.push((e1.clone(), a.clone(), b));
                    }
                }
            }
        }
        syn::visit::visit_expr_match(self, node);
    }
}

/// Effects a product item reaches, by the paths it names.
struct CapabilityScan<'a> {
    rel: &'a str,
    found: Vec<(Capability, Span, String)>,
}

impl CapabilityScan<'_> {
    fn note(&mut self, cap: Capability, span: proc_macro2::Span, what: String) {
        self.found.push((
            cap,
            Span {
                file: self.rel.to_string(),
                line: line_of(span),
            },
            what,
        ));
    }

    fn classify(segments: &[String]) -> Option<Capability> {
        let tail: Vec<&str> = if segments.first().is_some_and(|s| s == "std") {
            segments[1..].iter().map(String::as_str).collect()
        } else {
            segments.iter().map(String::as_str).collect()
        };
        match tail.first().copied() {
            Some("fs") | Some("File") | Some("OpenOptions") => Some(Capability::Fs),
            Some("net") | Some("TcpStream") | Some("TcpListener") | Some("UdpSocket") => {
                Some(Capability::Net)
            }
            Some("process") | Some("Command") => Some(Capability::Process),
            Some("env") => Some(Capability::Env),
            _ => None,
        }
    }
}

fn use_paths(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            use_paths(&p.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(n) => {
            let mut path = prefix.clone();
            path.push(n.ident.to_string());
            out.push(path);
        }
        syn::UseTree::Rename(r) => {
            let mut path = prefix.clone();
            path.push(r.ident.to_string());
            out.push(path);
        }
        syn::UseTree::Glob(_) => out.push(prefix.clone()),
        syn::UseTree::Group(g) => {
            for item in &g.items {
                use_paths(item, prefix, out);
            }
        }
    }
}

impl<'ast> Visit<'ast> for CapabilityScan<'_> {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let mut out = Vec::new();
        use_paths(&node.tree, &mut Vec::new(), &mut out);
        for path in out {
            if path.first().is_some_and(|s| s == "std") {
                if let Some(cap) = Self::classify(&path) {
                    self.note(cap, node.use_token.span, path.join("::"));
                }
            }
        }
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        let segments: Vec<String> = node.segments.iter().map(|s| s.ident.to_string()).collect();
        if segments.len() >= 2 {
            if let Some(cap) = Self::classify(&segments) {
                let span = node.segments[0].ident.span();
                self.note(cap, span, segments.join("::"));
            }
        }
        syn::visit::visit_path(self, node);
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.note(
            Capability::Unsafe,
            node.unsafe_token.span,
            "unsafe block".to_string(),
        );
        syn::visit::visit_expr_unsafe(self, node);
    }

    fn visit_signature(&mut self, node: &'ast syn::Signature) {
        if let syn::Safety::Unsafe(token) = node.safety {
            self.note(
                Capability::Unsafe,
                token.span,
                format!("unsafe fn {}", node.ident),
            );
        }
        syn::visit::visit_signature(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if let Some(token) = node.unsafety {
            self.note(Capability::Unsafe, token.span, "unsafe impl".to_string());
        }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        self.note(
            Capability::Ffi,
            node.abi.extern_token.span,
            "extern block".to_string(),
        );
        syn::visit::visit_item_foreign_mod(self, node);
    }
}
