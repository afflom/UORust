//! The core register: every body of Rust's `core`, as `scripts/mir/dump-core.sh`
//! dumps it one module at a time, translated by the MIR path at the canonical
//! instantiation, and recorded by κ with its outcome. Core is the subject the
//! MIR path grows against: std, the workspace and every other crate stand on
//! it.
//!
//! A generic body is translated once, each of its type parameters bound to
//! [`CANONICAL`]: a type every bound core states of a parameter it lifts is
//! met by. A bound the canonical type does not meet shows as residue where
//! the body uses it. A body over a const parameter is residue until its
//! instantiations are drawn from its uses.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::{lower, parse, IrBody};
use crate::lift::{self, Ty, Types};
use crate::UorustError;

/// The type each type parameter of a generic body is bound to.
pub const CANONICAL: &str = "u32";

/// What the register records of one body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreRow {
    /// `core::module::Type::name`, an impl named by the type it implements.
    pub path: String,
    /// The body's identity: its path and its MIR text.
    pub kappa: String,
    /// The bindings of its type parameters, `T=u32, U=u32`; empty when the
    /// body is not generic.
    pub at: String,
    /// The translation's digest, or `None` for residue.
    pub digest: Option<String>,
    /// The residue's cause, `mir::Kind: detail`.
    pub cause: Option<String>,
}

/// Whose bodies a census translates: core, std, or a crate of crates.io.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// The crate's name, as its paths begin: `core`, `std`, `serde`.
    pub krate: String,
    /// Whether every path the MIR prints without a crate is one of std's:
    /// true in core's and std's own scope, where MIR prints their paths
    /// without the crate.
    pub own_std: bool,
    /// What the bodies were dumped from: `rust-src 1.97.1`, `serde 1.0.0`.
    pub source: String,
    /// The command that writes the register, named in its header.
    pub command: String,
}

impl Scope {
    /// Core at the rust-src of `toolchain`.
    pub fn core(toolchain: &str) -> Self {
        Self::library("core", toolchain)
    }

    /// A crate of the rust-src of `toolchain`: `core`, `std`. The toolchain
    /// is the caller's (the pinned one, [`super::spec::TOOLCHAIN`]), so the
    /// scope is a function of its arguments alone and lifts.
    pub fn library(krate: &str, toolchain: &str) -> Self {
        Scope {
            krate: krate.to_string(),
            own_std: true,
            source: format!("rust-src {toolchain}"),
            command: format!("uorust {krate}"),
        }
    }

    /// A crate of crates.io at a version.
    pub fn registry(krate: &str, version: &str) -> Self {
        Scope {
            krate: krate.to_string(),
            own_std: false,
            source: format!("{krate} {version}"),
            command: format!("uorust crate {krate}@{version}"),
        }
    }
}

/// The census of one module shard.
#[derive(Clone, Debug, Default)]
pub struct CoreShard {
    /// The module, a top-level module of core.
    pub module: String,
    /// Its bodies' rows, by path.
    pub rows: Vec<CoreRow>,
}

impl CoreShard {
    /// How many of its bodies translate.
    pub fn translated_count(&self) -> usize {
        self.rows.iter().filter(|r| r.digest.is_some()).count()
    }
}

/// Translate every body of the module shards under `dumps` (one directory
/// per module) whose impls are named by the sources under `library` (the
/// rust-src `library` directory the dumps were built from).
pub fn core_census(library: &Path, dumps: &Path) -> Result<Vec<CoreShard>, UorustError> {
    // The dumps name sources `library/core/src/..`.
    census(
        &Scope::core(super::spec::TOOLCHAIN),
        library.parent().unwrap_or(library),
        dumps,
    )
}

/// Translate every body of the module shards under `dumps` (one directory
/// per module) of the crate `scope` names, whose dumps name their sources
/// relative to `root`.
pub fn census(scope: &Scope, root: &Path, dumps: &Path) -> Result<Vec<CoreShard>, UorustError> {
    let io = |p: &Path, e: std::io::Error| UorustError::Tool(format!("{}: {e}", p.display()));
    let mut modules: Vec<PathBuf> = std::fs::read_dir(dumps)
        .map_err(|e| io(dumps, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    modules.sort();
    let mut shards = Vec::new();
    for dir in modules {
        let module = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| io(&dir, e))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        let mut texts = Vec::new();
        for file in &files {
            texts.push(std::fs::read_to_string(file).map_err(|e| io(file, e))?);
        }
        shards.push(shard(scope, root, &module, &texts));
    }
    Ok(shards)
}

/// One module's census of the crate `scope` names from the texts of its
/// dumps.
pub(crate) fn shard(scope: &Scope, root: &Path, module: &str, texts: &[String]) -> CoreShard {
    let krate = scope.krate.as_str();
    // Each body at its instantiation, with the bindings and its own text.
    let mut bodies: Vec<(IrBody, String, String)> = Vec::new();
    let mut consts: BTreeMap<String, IrBody> = BTreeMap::new();
    let mut closures: BTreeMap<String, IrBody> = BTreeMap::new();
    for text in texts {
        let generics = canonical_bindings(text);
        let at = generics
            .iter()
            .map(|(p, t)| format!("{p}={t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let instance: String = text
            .lines()
            .map(|l| super::stdlib::substitute(l, &generics) + "\n")
            .collect();
        for (body, original) in parse::bodies(&instance)
            .into_iter()
            .zip(parse::bodies(text))
        {
            if body.arity == 0 && (body.path.contains("promoted[") || !body.path.contains('{')) {
                consts.insert(body.path.clone(), body.clone());
            }
            if let Some(t) = super::closure_type(&body) {
                closures.insert(t, body.clone());
            }
            if body.path.contains("{closure")
                || body.path.contains("promoted[")
                || body.path.contains("{constant")
            {
                continue;
            }
            // An impl a macro writes has no source of its own to name it by.
            let path = super::resolve_impls(root, &body.path).unwrap_or_else(|| body.path.clone());
            let kappa = crate::project::sha(
                format!("{krate}::{}\n{}", original.path, numbered_allocations(text)).as_bytes(),
            );
            let mut body = body;
            body.path = path;
            bodies.push((body, at.clone(), kappa));
        }
    }
    // Core's own scope, with `cmp::Ordering` the lift's ordering.
    let mut types = Types {
        core: scope.own_std,
        // Each float type at the format core's own constants define, where
        // the root holds core's sources (rust-src).
        floats: super::float::formats(&root.join("library")),
        ..Types::default()
    };
    types.enums.insert(
        crate::order::ORDERING.to_string(),
        crate::order::ORDERINGS
            .iter()
            .map(|v| v.to_string())
            .collect(),
    );
    types.aliases.insert(
        crate::order::RUST_ORDERING.to_string(),
        crate::order::ORDERING.to_string(),
    );
    instantiate_structs(&mut types, &core_structs(root), &bodies);
    let widths = lift::pointer_widths(&[]);
    let callees = BTreeMap::new();
    let none = std::collections::BTreeSet::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut rows = Vec::new();
    for (body, at, kappa) in bodies {
        let mut path = format!("{krate}::{}", body.path);
        let n = seen.entry(path.clone()).or_default();
        *n += 1;
        if *n > 1 {
            path = format!("{path}#{n}");
        }
        let segs: Vec<&str> = std::iter::once(krate)
            .chain(parse::split_path(&body.path))
            .collect();
        let name = crate::lex::camel(&segs);
        let active = [name.clone()];
        let cx = lower::Cx {
            types: &types,
            widths: &widths,
            callees: &callees,
            krate: krate.to_string(),
            taken: &none,
            explicit: &none,
            limit: crate::project::MAX_FILE_BYTES,
            consts: &consts,
            closures: &closures,
            active: &active,
        };
        let outcome = if at.contains('#') {
            Err("mir::Instantiation: a const parameter".to_string())
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lower::lower(&cx, &body, &name)
            }))
            .map_err(|_| "mir::Translator: panicked".to_string())
            .and_then(|r| r.map_err(|e| crate::project::mir_residue(e).detail()))
        };
        let (digest, cause) = match outcome {
            Ok(lowered) => {
                let text: String = lowered
                    .aux
                    .values()
                    .chain(&lowered.decls)
                    .map(crate::lex::render_decl)
                    .collect();
                (Some(crate::project::sha(text.as_bytes())), None)
            }
            Err(cause) => (None, Some(cause)),
        };
        rows.push(CoreRow {
            path,
            kappa,
            at,
            digest,
            cause,
        });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    CoreShard {
        module: module.to_string(),
        rows,
    }
}

/// A dump's text with its allocations numbered by their first mention and
/// its statics named without their build's indices:
/// rustc numbers them in the order the whole build makes them, which varies
/// from one build of core to the next.
fn numbered_allocations(text: &str) -> String {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("alloc") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "alloc".len()..];
        let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let word_start = !rest[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        if digits > 0 && word_start {
            let n = seen.len();
            let k = *seen.entry(after[..digits].to_string()).or_insert(n);
            out.push_str(&format!("alloc#{k}"));
            rest = &after[digits..];
        } else {
            out.push_str("alloc");
            rest = after;
        }
    }
    out.push_str(rest);
    without_def_ids(&out)
}

/// A dump's text without what names a static by its build: `DefId(0:54051 ~
/// core[59f9]::m::X)` is `DefId(core::m::X)`, its index and the crate's
/// disambiguator varying from one build of core to the next.
fn without_def_ids(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("DefId(") {
        out.push_str(&rest[..at + "DefId(".len()]);
        let after = &rest[at + "DefId(".len()..];
        let after = after
            .find(" ~ ")
            .map_or(after, |i| &after[i + " ~ ".len()..]);
        // The crate's name, then its disambiguator in brackets.
        let name = after.len()
            - after
                .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
                .len();
        out.push_str(&after[..name]);
        let tail = &after[name..];
        rest = match tail
            .strip_prefix('[')
            .and_then(|t| t.find(']').map(|i| &t[i + 1..]))
        {
            Some(t) => t,
            None => tail,
        };
    }
    out.push_str(rest);
    out
}

/// A struct core's source declares: its path from core's root, its type
/// parameters and its fields (a tuple struct's by position), each with its
/// type as written.
#[derive(Clone, Debug)]
pub(crate) struct CoreStruct {
    path: String,
    params: Vec<String>,
    fields: Vec<(Option<String>, String)>,
    /// For an enum whose variants hold no data, its variants; such an enum
    /// has no fields.
    variants: Option<Vec<String>>,
}

/// Every struct of core's source under `root` (the directory holding
/// `library/core/src`), by module: a file's module is its path there.
pub(crate) fn core_structs(root: &Path) -> Vec<CoreStruct> {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                walk(&p, files);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    fn items(module: &[String], list: &[syn::Item], out: &mut Vec<CoreStruct>) {
        for item in list {
            match item {
                syn::Item::Struct(st) => {
                    let params = st
                        .generics
                        .type_params()
                        .map(|p| p.ident.to_string())
                        .collect();
                    let fields = st
                        .fields
                        .iter()
                        .map(|f| {
                            let ty = &f.ty;
                            let text = quote::quote!(#ty)
                                .to_string()
                                .replace(" :: ", "::")
                                .replace(" < ", "<")
                                .replace(" >", ">")
                                .replace("& ", "&")
                                .replace(" ,", ",");
                            (f.ident.as_ref().map(ToString::to_string), text)
                        })
                        .collect();
                    let mut path = module.to_vec();
                    path.push(st.ident.to_string());
                    out.push(CoreStruct {
                        path: path.join("::"),
                        params,
                        fields,
                        variants: None,
                    });
                }
                syn::Item::Enum(e)
                    if e.generics.params.is_empty()
                        && e.variants.iter().all(|v| v.fields.is_empty()) =>
                {
                    let mut path = module.to_vec();
                    path.push(e.ident.to_string());
                    out.push(CoreStruct {
                        path: path.join("::"),
                        params: Vec::new(),
                        fields: Vec::new(),
                        variants: Some(e.variants.iter().map(|v| v.ident.to_string()).collect()),
                    });
                }
                syn::Item::Mod(m) => {
                    if let Some((_, inner)) = &m.content {
                        let mut path = module.to_vec();
                        path.push(m.ident.to_string());
                        items(&path, inner, out);
                    }
                }
                _ => {}
            }
        }
    }
    let src = root.join("library/core/src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(parsed) = syn::parse_file(&text) else {
            continue;
        };
        let rel = file.strip_prefix(&src).unwrap_or(&file).with_extension("");
        let mut module: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if matches!(module.last().map(String::as_str), Some("mod" | "lib")) {
            module.pop();
        }
        items(&module, &parsed.items, &mut out);
    }
    out
}

/// Each instance of a core struct the bodies' locals mention, as MIR prints
/// it, added to core's scope: a struct whose fields are its type parameters,
/// the unit or primitives is a record of those fields at the instance's
/// arguments, named by its path and arguments.
fn instantiate_structs(
    types: &mut Types,
    structs: &[CoreStruct],
    bodies: &[(IrBody, String, String)],
) {
    let by: BTreeMap<&str, &CoreStruct> = structs.iter().map(|s| (s.path.as_str(), s)).collect();
    // A path more than one module declares names no struct.
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for s in structs {
        *count.entry(s.path.as_str()).or_default() += 1;
    }
    // The last segment of a path, where one type of core has it: MIR's
    // headers print a type by it alone.
    let mut lasts: BTreeMap<&str, usize> = BTreeMap::new();
    for s in structs {
        *lasts
            .entry(s.path.rsplit("::").next().unwrap_or(""))
            .or_default() += 1;
    }
    let trimmed = |path: &str, text: &str| {
        let last = path.rsplit("::").next().unwrap_or(path);
        (lasts.get(last) == Some(&1)).then(|| format!("{last}{}", &text[path.len()..]))
    };
    // Enums of unit variants first: a struct's field may be one.
    for st in structs
        .iter()
        .filter(|s| s.variants.is_some() && count[s.path.as_str()] == 1)
    {
        let mut parts: Vec<&str> = vec!["core"];
        parts.extend(st.path.split("::"));
        let name = crate::lex::pascal(&parts);
        types
            .enums
            .insert(name.clone(), st.variants.clone().unwrap_or_default());
        types.aliases.insert(st.path.clone(), name.clone());
        if let Some(t) = trimmed(&st.path, &st.path) {
            types.aliases.entry(t).or_insert(name);
        }
    }
    let mut mentioned: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (body, _, _) in bodies {
        for l in &body.locals {
            let mut stack = vec![l.trim().trim_start_matches('&').trim().to_string()];
            while let Some(t) = stack.pop() {
                if let Some(open) = t.find('<') {
                    let inner = t[open + 1..].strip_suffix('>').unwrap_or("");
                    stack.extend(
                        parse::split_top(inner, ',')
                            .into_iter()
                            .map(|a| a.trim().to_string()),
                    );
                }
                mentioned.insert(t);
            }
        }
    }
    for text in mentioned {
        let (path, args) = match text.find('<') {
            Some(open) => (
                text[..open].trim_end_matches("::"),
                parse::split_top(text[open + 1..].strip_suffix('>').unwrap_or(""), ',')
                    .into_iter()
                    .map(str::trim)
                    .filter(|a| !a.is_empty() && !a.starts_with('\''))
                    .collect::<Vec<_>>(),
            ),
            None => (text.as_str(), Vec::new()),
        };
        let Some(st) = by.get(path).filter(|_| count[path] == 1) else {
            continue;
        };
        if st.variants.is_some() || st.params.len() != args.len() {
            continue;
        }
        let module = st.path.rsplit_once("::").map_or("", |(m, _)| m);
        let binds: BTreeMap<String, String> = st
            .params
            .iter()
            .cloned()
            .zip(args.iter().map(|a| (*a).to_string()))
            .collect();
        let mut fields = Vec::new();
        let mut ok = true;
        for (i, (name, t)) in st.fields.iter().enumerate() {
            let t = super::stdlib::substitute(t, &binds);
            // A type the struct's module declares, by its path there.
            let local = format!("{module}::{t}");
            let t = if types.aliases.contains_key(&local) {
                local
            } else {
                t
            };
            match super::lower::ty(&t, types) {
                Ok(ft) => fields.push((
                    name.as_deref()
                        .map_or_else(|| format!("item{i}"), lift::local_name),
                    ft,
                )),
                Err(_) => ok = false,
            }
        }
        if !ok {
            continue;
        }
        let mut parts: Vec<&str> = vec!["core"];
        parts.extend(path.split("::"));
        parts.extend(args.iter().copied());
        let name = crate::lex::pascal(&parts);
        types.structs.insert(name.clone(), fields);
        if let Some(t) = trimmed(path, &text) {
            types.aliases.entry(t).or_insert(name.clone());
        }
        types.aliases.insert(text.clone(), name);
    }
}

/// The type parameters of a dumped body, each bound to [`CANONICAL`]: the
/// bare capitalized names its locals' types mention (rustc prints the
/// locals' own types by their paths, so a bare name is a parameter), and
/// `Self`. A const parameter, a bare name in an array's length, is bound to
/// `#`, which no body takes.
pub(crate) fn canonical_bindings(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("let ") else {
            continue;
        };
        let Some((_, ty)) = rest.split_once(": ") else {
            continue;
        };
        let ty = ty.trim_end_matches(';');
        let chars: Vec<char> = ty.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_alphabetic() || c == '_' || c == '\'' {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let before: String = chars[..start].iter().collect();
                let after: String = chars[i..].iter().collect();
                let segment = before.ends_with("::") || after.starts_with("::");
                let named = before.ends_with(" as ")
                    || before.ends_with("dyn ")
                    || before.ends_with("impl ")
                    || after.starts_with('<')
                    || after.starts_with('(')
                    || after.starts_with(" =")
                    || after.starts_with('=');
                let bare = word.starts_with(|c: char| c.is_ascii_uppercase());
                if bare && !segment && !named {
                    let length = before.trim_end().ends_with(';');
                    let bound = if length { "#" } else { CANONICAL };
                    out.entry(word).or_insert_with(|| bound.to_string());
                }
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Write each shard's rows to `<module>.toml` in the register `dir`, then
/// the summary over every module file the register holds.
pub fn write_core_register(dir: &Path, shards: &[CoreShard]) -> Result<(), UorustError> {
    write_register(&Scope::core(super::spec::TOOLCHAIN), dir, shards)
}

/// Write each shard's rows to `<module>.toml` in the register `dir` of the
/// crate `scope` names, then the summary over every module file it holds.
pub fn write_register(scope: &Scope, dir: &Path, shards: &[CoreShard]) -> Result<(), UorustError> {
    let io = |p: &Path, e: std::io::Error| UorustError::Tool(format!("{}: {e}", p.display()));
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    for s in shards {
        let mut rows = String::new();
        for r in &s.rows {
            let _ = writeln!(
                rows,
                "[[fn]]\npath = {}\nkappa = \"{}\"",
                toml_quote(&r.path),
                r.kappa
            );
            if !r.at.is_empty() {
                let _ = writeln!(rows, "at = {}", toml_quote(&r.at));
            }
            match (&r.digest, &r.cause) {
                (Some(d), _) => {
                    let _ = writeln!(rows, "status = \"translated\"\ndigest = \"{d}\"");
                }
                (None, cause) => {
                    let cause = cause.clone().unwrap_or_default();
                    let _ = writeln!(rows, "status = \"residue\"\ncause = {}", toml_quote(&cause));
                }
            }
            rows.push('\n');
        }
        let file = dir.join(format!("{}.toml", s.module));
        std::fs::write(&file, rows).map_err(|e| io(&file, e))?;
    }
    summarize_register(scope, dir)
}

/// The register's `frontier.toml`: per module the bodies, the translated and
/// the residue, and the causes the residue stops at, most bodies first,
/// from the module files the register holds.
pub fn summarize_core_register(dir: &Path) -> Result<(), UorustError> {
    summarize_register(&Scope::core(super::spec::TOOLCHAIN), dir)
}

/// The `frontier.toml` of the register `dir` of the crate `scope` names.
pub fn summarize_register(scope: &Scope, dir: &Path) -> Result<(), UorustError> {
    let (krate, source, command) = (&scope.krate, &scope.source, &scope.command);
    let toolchain = super::spec::TOOLCHAIN;
    let io = |p: &Path, e: std::io::Error| UorustError::Tool(format!("{}: {e}", p.display()));
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
    let mut modules = String::new();
    let mut causes: BTreeMap<String, usize> = BTreeMap::new();
    let (mut all, mut done) = (0, 0);
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|e| io(&file, e))?;
        let doc: toml::Table = text
            .parse()
            .map_err(|e| UorustError::Tool(format!("{}: {e}", file.display())))?;
        let rows = doc
            .get("fn")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut translated = 0;
        for row in &rows {
            let field = |k: &str| row.get(k).and_then(toml::Value::as_str).unwrap_or("");
            if field("status") == "translated" {
                translated += 1;
            } else {
                *causes.entry(cause_category(field("cause"))).or_default() += 1;
            }
        }
        all += rows.len();
        done += translated;
        let module = file
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let _ = writeln!(
            modules,
            "[module.{module}]\nbodies = {}\ntranslated = {translated}\nresidue = {}\n",
            rows.len(),
            rows.len() - translated
        );
    }
    let mut summary = format!(
        "# The {krate} register (written by `{command}`; do not edit): every body of\n\
         # {krate}'s MIR at {source}, translated by the MIR path with each\n\
         # type parameter bound to `{CANONICAL}`, by module. A translated body lowers\n\
         # to LexLean; the residue stops at the causes listed last. Rows, by κ, are\n\
         # in each module's own file, and none recorded translated may regress.\n\n\
         toolchain = \"{toolchain}\"\ninstantiation = \"{CANONICAL}\"\n\
         bodies = {all}\ntranslated = {done}\nresidue = {}\n\n{modules}",
        all - done
    );
    let mut ranked: Vec<(String, usize)> = causes.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (cause, count) in ranked {
        let _ = writeln!(
            summary,
            "[[cause]]\ncause = {}\nbodies = {count}\n",
            toml_quote(&cause)
        );
    }
    let file = dir.join("frontier.toml");
    std::fs::write(&file, summary.trim_end().to_string() + "\n").map_err(|e| io(&file, e))
}

/// The bodies a register at `dir` records as translated that `shards` now
/// leave as residue, by κ: the register is monotone.
pub fn core_regressions(dir: &Path, shards: &[CoreShard]) -> Vec<String> {
    let mut out = Vec::new();
    for s in shards {
        let file = dir.join(format!("{}.toml", s.module));
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(doc) = text.parse::<toml::Table>() else {
            continue;
        };
        let now: BTreeMap<&str, &CoreRow> = s.rows.iter().map(|r| (r.kappa.as_str(), r)).collect();
        for row in doc
            .get("fn")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let field = |k: &str| row.get(k).and_then(toml::Value::as_str).unwrap_or("");
            if field("status") != "translated" {
                continue;
            }
            if let Some(r) = now.get(field("kappa")) {
                if r.digest.is_none() {
                    out.push(format!(
                        "{}: {}",
                        r.path,
                        r.cause.clone().unwrap_or_default()
                    ));
                }
            }
        }
    }
    out
}

/// A cause's kind with the head of its detail: up to its first clause, a
/// path without its generic arguments.
pub fn cause_category(cause: &str) -> String {
    let (kind, detail) = cause.split_once(": ").unwrap_or((cause, ""));
    // Generic arguments dropped: `Option::<u32>::map` is `Option::map`.
    let mut plain = String::new();
    let mut depth = 0usize;
    for c in detail.chars() {
        match c {
            '<' if plain.ends_with("::") => depth += 1,
            '<' if depth > 0 => depth += 1,
            '>' if depth > 0 => {
                depth -= 1;
                if depth == 0 && plain.ends_with("::") {
                    plain.truncate(plain.len() - 2);
                }
            }
            _ if depth > 0 => {}
            c => plain.push(c),
        }
    }
    let head: String = plain
        .split(['(', ','])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    if head.is_empty() {
        kind.to_string()
    } else {
        format!("{kind}: {head}")
    }
}

fn toml_quote(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bodys_type_parameters_are_its_bare_names() {
        let text = "fn f(_1: Option<T>, _2: F) -> Option<U> {\n    let mut _0: option::Option<U>;\n    let _4: T;\n    let mut _5: F;\n    let _6: &<T as ops::Deref>::Target;\n    let _7: [T; N];\n}\n";
        let p = canonical_bindings(text);
        assert_eq!(p.get("T").map(String::as_str), Some(CANONICAL));
        assert_eq!(p.get("U").map(String::as_str), Some(CANONICAL));
        assert_eq!(p.get("F").map(String::as_str), Some(CANONICAL));
        assert_eq!(p.get("N").map(String::as_str), Some("#"));
        assert!(!p.contains_key("Deref") && !p.contains_key("Target"));
    }

    #[test]
    fn allocations_are_numbered_by_first_mention() {
        assert_eq!(
            numbered_allocations("a = alloc812; b = alloc77; c = alloc812; reallocate"),
            "a = alloc#0; b = alloc#1; c = alloc#0; reallocate"
        );
        assert_eq!(
            numbered_allocations("x = const <static(DefId(0:54051 ~ core[59f9]::m::OFFSETS))>;"),
            "x = const <static(DefId(core::m::OFFSETS))>;"
        );
    }

    #[test]
    fn causes_rank_by_kind_and_head() {
        assert_eq!(cause_category("mir::Ty: &mut u32"), "mir::Ty: &mut u32");
        assert_eq!(
            cause_category("mir::TerminatorKind::Call: Option::<u32>::map(x)"),
            "mir::TerminatorKind::Call: Option::map"
        );
    }
}
