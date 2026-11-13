//! Projection: admitted facts to the files of a Prism project, byte for byte
//! the same for the same workspace.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::admit::Refusal;
use crate::facts::{Capability, FnFact, Package, SourceFile, Span, Syntax, TargetKind, Workspace};
use crate::ingest::summary;
use crate::lex::{pascal, string, string_list, Arm, Decl, Expr, Module};
use crate::lift::{self, Candidate, Residue, Ty, Types};

/// What a projection is parameterised by beyond the workspace.
#[derive(Clone, Debug)]
pub struct Options {
    /// The project name; its PascalCase form is the LexLean module prefix.
    pub name: String,
    /// The control set's `lastModified`, `YYYY-MM-DDThh:mm:ssZ`.
    pub last_modified: String,
    /// A RustSec advisory database checkout to check the locked packages
    /// against; without one, the frontier records that none was consulted.
    pub advisories: Option<std::path::PathBuf>,
    /// The MIR path of the lift, when it runs: see [`crate::mir`].
    pub mir: Option<crate::mir::MirOptions>,
}

/// The largest source file the projection's LexLean project admits.
pub(crate) const MAX_FILE_BYTES: usize = 4_194_304;

const ARCH: &str = "Architecture";
const IFACE: &str = "Interface";
const DESIGN: &str = "Design";
const SEC: &str = "Security";
const QUAL: &str = "Quality";
const BEHAVIOR: &str = "Behavior";
const SUPPLY: &str = "SupplyChain";

const BOOL: &str = "lexlean.std.bool@1.2.0";
const NAT: &str = "lexlean.std.nat@1.2.0";
const PARCH: &str = "prism.arch@1.0.0";
const PSEC: &str = "prism.sec@1.0.0";
const PQUAL: &str = "prism.qual@1.0.0";
const TITLES: &str = "uorust.titles@1.0.0";

/// A function in the workspace: package, file and position in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FnRef {
    pkg: usize,
    file: usize,
    idx: usize,
}

#[derive(Clone, Debug)]
struct Component {
    id: String,
    kind: &'static str,
    name: String,
    text: String,
}

/// What the projection derives from the workspace before it writes a facet:
/// the components, their edges, the resolved calls and the routed enums.
/// Built once, then only read.
struct Model {
    ws: Workspace,
    /// `(package, target index)` of a product target -> component ID.
    targets: BTreeMap<(usize, usize), String>,
    components: Vec<Component>,
    /// Component ID -> index.
    index: BTreeMap<String, usize>,
    /// Edge ID -> (from component ID, to component ID).
    edges: BTreeMap<String, (String, String)>,
    /// Resolved calls: caller -> callees.
    calls: BTreeMap<FnRef, BTreeSet<FnRef>>,
    /// The route the router admitted for each enum: `(package, file, enum)`.
    enum_routes: BTreeMap<(usize, usize, usize), &'static str>,
}

/// What the frontier records, as the facets find it.
#[derive(Default)]
struct Notes {
    refusals: BTreeSet<Refusal>,
    residue: BTreeMap<String, (Span, Residue)>,
    /// Functions that perform an operating-system effect, directly or
    /// through a function that does: the I/O shell, recorded with the effect
    /// instead of as residue.
    effects: BTreeMap<String, (Span, String)>,
    /// Functions at the boundary with a foreign crate: a parameter or the
    /// result is a type of a registry dependency that has no LexLean model.
    foreign: BTreeMap<String, (Span, String)>,
    /// Functions the lift took: the definition each lifted to, and where it
    /// parts from the Rust function, if anywhere.
    lifted: BTreeMap<String, (Span, String, Vec<String>)>,
}

impl Model {
    fn package(&self, r: FnRef) -> &Package {
        &self.ws.packages[r.pkg]
    }

    fn file(&self, r: FnRef) -> &SourceFile {
        &self.ws.packages[r.pkg].files[r.file]
    }

    fn func(&self, r: FnRef) -> &FnFact {
        &self.file(r).fns[r.idx]
    }

    fn kind(&self, r: FnRef) -> &TargetKind {
        let file = self.file(r);
        &self.package(r).targets[file.target].kind
    }

    fn crate_ident(&self, r: FnRef) -> String {
        let file = self.file(r);
        self.package(r).targets[file.target].name.replace('-', "_")
    }

    /// `crate::module::Type::name`.
    fn path(&self, r: FnRef) -> String {
        let file = self.file(r);
        let f = self.func(r);
        let mut parts = vec![self.crate_ident(r)];
        if !file.module.is_empty() {
            parts.push(file.module.clone());
        }
        if let Some(owner) = &f.owner {
            parts.push(owner.clone());
        }
        parts.push(f.name.clone());
        parts.join("::")
    }

    fn component_of(&self, r: FnRef) -> Option<&String> {
        self.targets.get(&(r.pkg, self.file(r).target))
    }

    /// A function the lift refused: residue, or a foreign row when what
    /// stopped it is a type of a registry dependency.
    fn record_residue(&self, notes: &mut Notes, r: FnRef, span: Span, reason: Residue) {
        let package = &self.ws.packages[r.pkg];
        if let Residue::Type(ty) = &reason {
            if let Some(dep) = foreign_crate(package, ty) {
                let detail = format!(
                    "takes or returns {ty}, a type of the {dep} crate with no LexLean model"
                );
                notes.foreign.insert(self.path(r), (span, detail));
                return;
            }
        }
        notes.residue.insert(self.path(r), (span, reason));
    }

    /// A function the lift did not take: an effect row when it reaches an
    /// effect, otherwise residue or a foreign row.
    fn stopped(
        &self,
        notes: &mut Notes,
        effectful: &BTreeMap<FnRef, String>,
        r: FnRef,
        reason: Residue,
    ) {
        let span = self.func(r).span.clone();
        match effectful.get(&r) {
            Some(effect) => {
                notes.effects.insert(self.path(r), (span, effect.clone()));
            }
            None => self.record_residue(notes, r, span, reason),
        }
    }

    fn product(&self, r: FnRef) -> bool {
        matches!(self.kind(r), TargetKind::Lib | TargetKind::Bin)
            && !self.func(r).in_test_module
            && !self.func(r).test
    }

    fn all_fns(&self) -> Vec<FnRef> {
        let mut out = Vec::new();
        for (pkg, package) in self.ws.packages.iter().enumerate() {
            for (file, source) in package.files.iter().enumerate() {
                for idx in 0..source.fns.len() {
                    out.push(FnRef { pkg, file, idx });
                }
            }
        }
        out
    }

    fn component_suffix(id: &str) -> &str {
        id.strip_prefix("Architecture::component").unwrap_or(id)
    }
}

fn qualified(module: &str, name: &str) -> String {
    format!("{module}::{name}")
}

/// Records of one type, sorted by ID bytes, each knowing its index.
fn indexed<T>(mut rows: Vec<(String, T)>) -> Vec<(String, usize, T)> {
    rows.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    rows.into_iter()
        .enumerate()
        .map(|(i, (id, row))| (id, i, row))
        .collect()
}

fn local(id: &str) -> &str {
    id.rsplit("::").next().unwrap_or(id)
}

fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The whole projection: path -> bytes.
pub(crate) fn project(
    ws: &Workspace,
    syntax: &Syntax,
    options: &Options,
    dumps: Option<&crate::mir::Dumps>,
) -> BTreeMap<String, Vec<u8>> {
    let mut notes = Notes::default();
    for package in &ws.packages {
        for file in &package.files {
            if let Some(reason) = &file.parse_error {
                let line = reason
                    .split(':')
                    .nth(1)
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(1);
                notes.refusals.insert(Refusal::Unparsable {
                    span: Span {
                        file: file.rel.clone(),
                        line,
                    },
                    reason: reason.clone(),
                });
            }
        }
    }
    let (targets, components, index, refused) = components(ws);
    notes.refusals.extend(refused);
    let mut ctx = Model {
        ws: ws.clone(),
        targets,
        components,
        index,
        edges: BTreeMap::new(),
        calls: BTreeMap::new(),
        enum_routes: BTreeMap::new(),
    };
    ctx.edges = edges(&ctx);
    let (calls, refused) = resolve(&ctx);
    ctx.calls = calls;
    notes.refusals.extend(refused);
    let (enum_routes, refused) = route_enums(&ctx);
    ctx.enum_routes = enum_routes;
    notes.refusals.extend(refused);
    let ctx = ctx;

    let prefix = pascal(&[&options.name]);
    let mut modules = Vec::new();
    let arch = architecture(&ctx);
    let (iface, refused) = interface(&ctx);
    notes.refusals.extend(refused);
    let (design, refused) = design(&ctx);
    notes.refusals.extend(refused);
    match &ws.advisories {
        None => {
            notes.refusals.insert(Refusal::AdvisoriesUnconsulted);
        }
        Some(db) => {
            for a in &db.hits {
                if !ws.has_deny && !ws.deny_ignores.contains_key(&a.id) {
                    notes.refusals.insert(Refusal::UnansweredAdvisory {
                        id: a.id.clone(),
                        package: a.package.clone(),
                        version: a.version.clone(),
                    });
                }
            }
        }
    }
    let security = security(&ctx, options);
    let (quality, refused) = quality(&ctx, iface.feature_count);
    notes.refusals.extend(refused);
    let mir = dumps.zip(options.mir.as_ref());
    let (behavior, candidates) = behavior(&ctx, syntax, &mut notes, mir);
    let supply = supply(&ctx);
    modules.push(arch);
    modules.push(iface.module);
    modules.push(design);
    modules.push(security);
    modules.push(quality);
    modules.push(behavior);
    modules.push(supply);
    // LexLean refuses a semantic module with no declaration (LLT4001), and a
    // workspace with nothing to lift (a fresh `cargo new`) leaves Behavior so.
    modules.retain(|m| !m.decls.is_empty());

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (path, bytes) in crate::assets::FIXED {
        files.insert((*path).to_string(), bytes.as_bytes().to_vec());
    }
    let mut entrypoints: Vec<String> = files
        .keys()
        .filter(|p| p.ends_with(".lex.tex"))
        .cloned()
        .collect();
    for module in &modules {
        entrypoints.push(module.path());
        files.insert(module.path(), module.render().into_bytes());
    }
    entrypoints.sort();
    let toml_name: String = options
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let lexlean = format!(
        "spec = \"lexlean/project/1\"\nname = \"{toml_name}\"\nlanguage = \"1.2\"\n\
         module_prefix = \"{prefix}\"\nsource_roots = [\"src\"]\nentrypoints = [{}]\n\
         build_root = \".lexlean\"\nlockfile = \"lexlean.lock\"\nlean_workspace = \".\"\n\
         lean_toolchain = \"leanprover/lean4:v4.32.1\"\n\n\
         [[lexicon_source]]\npackage = \"lexlean.std.nat\"\nkind = \"builtin\"\n\n\
         [[lexicon_source]]\npackage = \"prism.arch\"\nkind = \"path\"\npath = \"language/prism.arch\"\n\n\
         [[lexicon_source]]\npackage = \"prism.qual\"\nkind = \"path\"\npath = \"language/prism.qual\"\n\n\
         [[lexicon_source]]\npackage = \"prism.sec\"\nkind = \"path\"\npath = \"language/prism.sec\"\n\n\
         [[lexicon_source]]\npackage = \"uorust.titles\"\nkind = \"path\"\npath = \"language/uorust.titles\"\n\n\
         [limits]\nmax_file_bytes = {MAX_FILE_BYTES}\nmax_total_source_bytes = 67108864\n\
         max_primitive_atoms = 2000000\nmax_token_lattice_edges = 4000000\nmax_parse_states = 4000000\n\
         max_ir_nodes = 2000000\nmax_scope_depth = 1024\nmax_import_depth = 128\nmax_diagnostics = 256\n\
         max_child_output_bytes = 16777216\nchild_timeout_ms = 300000\n",
        entrypoints
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let lakefile = format!(
        "name = \"{prefix}\"\nversion = \"0.1.0\"\ndefaultTargets = [\"{prefix}\"]\n\n[[lean_lib]]\nname = \"{prefix}\"\n"
    );
    let manifest = format!(
        "{{\"version\":\"1.2.0\",\"packagesDir\":\".lake/packages\",\"packages\":[],\"name\":\"{prefix}\",\"lakeDir\":\".lake\",\"fixedToolchain\":false}}\n"
    );
    let toolchain = files.get("lean-toolchain").cloned().unwrap_or_default();
    let lock = format!(
        "spec = \"lexlean/lock/2\"\nlanguage = \"1.2\"\n\
         compiler_semantics = \"1427f8b629013648a480454d67202788f2303f5396f1d7c70a7c350ddab6f694\"\n\
         project_config_sha256 = \"{}\"\nlean_toolchain = \"leanprover/lean4:v4.32.1\"\n\n\
         [[workspace_file]]\npath = \"lake-manifest.json\"\nsha256 = \"{}\"\n\n\
         [[workspace_file]]\npath = \"lakefile.toml\"\nsha256 = \"{}\"\n\n\
         [[workspace_file]]\npath = \"lean-toolchain\"\nsha256 = \"{}\"\n\n{}",
        sha(lexlean.as_bytes()),
        sha(manifest.as_bytes()),
        sha(lakefile.as_bytes()),
        sha(&toolchain),
        crate::assets::LOCK_PACKAGES
    );
    let prismpm = format!(
        "spec = \"prismpm/project/1\"\nproject = \"{prefix}\"\nlexlean_project = \"lexlean.toml\"\n\
         build_root = \".prism\"\n\n[limits]\nmax_holo_bytes = 16777216\nmax_entities = 100000\n\
         max_diagnostics = 256\n"
    );
    files.insert("lexlean.toml".into(), lexlean.into_bytes());
    files.insert("lexlean.lock".into(), lock.into_bytes());
    files.insert("lakefile.toml".into(), lakefile.into_bytes());
    files.insert("lake-manifest.json".into(), manifest.into_bytes());
    files.insert("prismpm.toml".into(), prismpm.into_bytes());
    files.insert(".gitattributes".into(), b"* -text\n".to_vec());
    files.insert(
        ".gitignore".into(),
        b"/.lexlean/\n/.prism/\n/.lake/\n".to_vec(),
    );
    for (path, bytes) in register(&iface.register) {
        files.insert(path, bytes);
    }
    files.insert("supply/spdx.json".into(), spdx(ws, options).into_bytes());
    // The prover's trial run lists the translations it admitted on trial.
    if let Some(listing) = candidates {
        files.insert(crate::mir::CANDIDATES.into(), listing.into_bytes());
    }
    // The κ of every function of product code: the question its row answers.
    let kappas: BTreeMap<String, String> = ctx
        .all_fns()
        .into_iter()
        .filter(|&r| ctx.product(r))
        .map(|r| {
            let path = ctx.path(r);
            let kappa = crate::register::kappa(&path, &ctx.func(r).tokens);
            (path, kappa)
        })
        .collect();
    files.insert(
        "frontier.toml".into(),
        frontier(&notes, &kappas).into_bytes(),
    );
    files
}

pub(crate) fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The components: each product target and each third-party dependency, with
/// the component ID of each product target and each component's index.
#[allow(clippy::type_complexity)]
fn components(
    ws: &Workspace,
) -> (
    BTreeMap<(usize, usize), String>,
    Vec<Component>,
    BTreeMap<String, usize>,
    Vec<Refusal>,
) {
    let mut targets = BTreeMap::new();
    let mut refusals = Vec::new();
    let mut rows: Vec<(String, Component)> = Vec::new();
    let internal: BTreeSet<&str> = ws.packages.iter().map(|p| p.name.as_str()).collect();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (t, target) in package.targets.iter().enumerate() {
            let (kind, tag) = match target.kind {
                TargetKind::Lib => ("service", "Lib"),
                TargetKind::Bin => ("releaseBinary", "Bin"),
                _ => continue,
            };
            let root = package.files.iter().find(|f| f.rel == target.src);
            let doc = root.map(|f| summary(&f.doc)).unwrap_or_default();
            let text = if doc.is_empty() {
                package.description.clone()
            } else {
                doc
            };
            let id = qualified(ARCH, &format!("component{tag}{}", pascal(&[&target.name])));
            targets.insert((pkg, t), id.clone());
            rows.push((
                id.clone(),
                Component {
                    id,
                    kind,
                    name: target.name.clone(),
                    text,
                },
            ));
        }
        for dep in &package.deps {
            if dep.kind != "normal" || internal.contains(dep.name.as_str()) {
                continue;
            }
            if dep.path.is_some() {
                if !dep.inside {
                    refusals.push(Refusal::ExternalPath {
                        package: package.dir.clone(),
                        dependency: dep.name.clone(),
                    });
                }
                continue;
            }
            let id = qualified(ARCH, &format!("componentExt{}", pascal(&[&dep.name])));
            let version = ws
                .lock
                .iter()
                .filter(|l| l.name == dep.name)
                .map(|l| l.version.clone())
                .collect::<Vec<_>>()
                .join(", ");
            let text = if version.is_empty() {
                format!("Third-party crate {} from crates.io.", dep.name)
            } else {
                format!(
                    "Third-party crate {} {version}, locked in Cargo.lock.",
                    dep.name
                )
            };
            rows.push((
                id.clone(),
                Component {
                    id,
                    kind: "externalSystem",
                    name: dep.name.clone(),
                    text,
                },
            ));
        }
    }
    rows.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    rows.dedup_by(|a, b| a.0 == b.0);
    let mut index = BTreeMap::new();
    let mut components = Vec::new();
    for (i, (id, row)) in rows.into_iter().enumerate() {
        index.insert(id, i);
        components.push(row);
    }
    (targets, components, index, refusals)
}

fn edges(ctx: &Model) -> BTreeMap<String, (String, String)> {
    let ws = &ctx.ws;
    let mut edges = BTreeMap::new();
    let lib_of: BTreeMap<&str, String> = ws
        .packages
        .iter()
        .enumerate()
        .filter_map(|(pkg, p)| {
            p.targets
                .iter()
                .position(|t| t.kind == TargetKind::Lib)
                .and_then(|t| ctx.targets.get(&(pkg, t)))
                .map(|id| (p.name.as_str(), id.clone()))
        })
        .collect();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (t, target) in package.targets.iter().enumerate() {
            let Some(from) = ctx.targets.get(&(pkg, t)).cloned() else {
                continue;
            };
            let mut to = Vec::new();
            if target.kind == TargetKind::Bin {
                if let Some(own) = lib_of.get(package.name.as_str()) {
                    to.push(own.clone());
                }
            }
            for dep in &package.deps {
                if dep.kind != "normal" {
                    continue;
                }
                if let Some(lib) = lib_of.get(dep.name.as_str()) {
                    to.push(lib.clone());
                } else if dep.path.is_none() {
                    to.push(qualified(
                        ARCH,
                        &format!("componentExt{}", pascal(&[&dep.name])),
                    ));
                }
            }
            for target_id in to {
                if target_id == from || !ctx.index.contains_key(&target_id) {
                    continue;
                }
                let id = qualified(
                    ARCH,
                    &format!(
                        "edge{}To{}",
                        Model::component_suffix(&from),
                        Model::component_suffix(&target_id)
                    ),
                );
                edges.insert(id, (from.clone(), target_id));
            }
        }
    }
    edges
}

/// Resolve every path call and unique method call to workspace functions.
#[allow(clippy::type_complexity)]
fn resolve(ctx: &Model) -> (BTreeMap<FnRef, BTreeSet<FnRef>>, Vec<Refusal>) {
    let mut calls = BTreeMap::new();
    let mut refusals = Vec::new();
    let all = ctx.all_fns();
    let mut by_name: BTreeMap<&str, Vec<FnRef>> = BTreeMap::new();
    for &r in &all {
        if ctx.product(r) {
            by_name
                .entry(ctx.func(r).name.as_str())
                .or_default()
                .push(r);
        }
    }
    let crate_idents: BTreeMap<String, usize> = ctx
        .ws
        .packages
        .iter()
        .enumerate()
        .flat_map(|(pkg, p)| {
            p.targets
                .iter()
                .filter(|t| t.kind == TargetKind::Lib)
                .map(move |t| (t.name.replace('-', "_"), pkg))
        })
        .collect();
    for &caller in &all {
        let f = ctx.func(caller);
        let mut out = BTreeSet::new();
        for call in &f.calls {
            if let Some(method) = call.strip_prefix('.') {
                let candidates: Vec<FnRef> = by_name
                    .get(method)
                    .map(|v| {
                        v.iter()
                            .copied()
                            .filter(|r| ctx.func(*r).owner.is_some())
                            .collect()
                    })
                    .unwrap_or_default();
                // A method a trait declares, with every other candidate an
                // implementation of that trait, is a call of the trait's
                // interface.
                let declared: Vec<FnRef> = candidates
                    .iter()
                    .copied()
                    .filter(|r| ctx.func(*r).in_trait)
                    .collect();
                if let [decl] = declared.as_slice() {
                    let name = ctx.func(*decl).owner.clone();
                    let through = candidates
                        .iter()
                        .all(|r| r == decl || ctx.func(*r).implements == name);
                    if through {
                        out.insert(*decl);
                        continue;
                    }
                }
                if let [only] = candidates.as_slice() {
                    out.insert(*only);
                }
                continue;
            }
            let imports = &ctx.file(caller).imports;
            let expanded = match call.split_once("::") {
                Some((head, rest)) => imports.get(head).map(|p| format!("{p}::{rest}")),
                None => imports.get(call.as_str()).cloned(),
            };
            let call = expanded.as_ref().unwrap_or(call);
            let segments: Vec<&str> = call.split("::").collect();
            let Some(name) = segments.last() else {
                continue;
            };
            let Some(found) = by_name.get(name) else {
                continue;
            };
            let mut candidates: Vec<FnRef> = found.clone();
            if segments.len() >= 2 {
                let first = segments[0];
                let owner = segments[segments.len() - 2];
                if let Some(&pkg) = crate_idents.get(first) {
                    candidates.retain(|r| r.pkg == pkg && ctx.kind(*r) == &TargetKind::Lib);
                } else if first.starts_with(|c: char| c.is_ascii_lowercase()) {
                    // `crate::`, `self::`, `super::` or a local module path:
                    // only a function of the caller's own target whose module
                    // ends with that path. Anything else (`std::fs::read`, an
                    // external crate) is not a workspace call.
                    let module: Vec<&str> = segments[..segments.len() - 1]
                        .iter()
                        .copied()
                        .filter(|s| !matches!(*s, "crate" | "self" | "super"))
                        .filter(|s| !s.starts_with(|c: char| c.is_ascii_uppercase()))
                        .collect();
                    let module = module.join("::");
                    candidates.retain(|r| {
                        r.pkg == caller.pkg
                            && ctx.file(*r).target == ctx.file(caller).target
                            && (module.is_empty() || ctx.file(*r).module.ends_with(&module))
                    });
                }
                if owner == "Self" {
                    let own = f.owner.clone();
                    candidates.retain(|r| ctx.func(*r).owner == own);
                } else if owner.starts_with(|c: char| c.is_ascii_uppercase()) {
                    candidates.retain(|r| {
                        ctx.func(*r)
                            .owner
                            .as_deref()
                            .is_some_and(|o| o.split('<').next() == Some(owner))
                    });
                } else {
                    candidates.retain(|r| ctx.func(*r).owner.is_none());
                }
                if candidates.len() > 1 {
                    let module = segments[..segments.len() - 1]
                        .iter()
                        .skip(usize::from(crate_idents.contains_key(first)))
                        .filter(|s| !matches!(**s, "crate" | "self" | "super"))
                        .filter(|s| !s.starts_with(|c: char| c.is_ascii_uppercase()))
                        .copied()
                        .collect::<Vec<_>>()
                        .join("::");
                    let narrowed: Vec<FnRef> = candidates
                        .iter()
                        .copied()
                        .filter(|r| ctx.file(*r).module.ends_with(&module))
                        .collect();
                    if !narrowed.is_empty() {
                        candidates = narrowed;
                    }
                }
            } else {
                candidates.retain(|r| ctx.func(*r).owner.is_none());
                let same_file: Vec<FnRef> = candidates
                    .iter()
                    .copied()
                    .filter(|r| r.pkg == caller.pkg && r.file == caller.file)
                    .collect();
                if !same_file.is_empty() {
                    candidates = same_file;
                } else {
                    candidates.retain(|r| {
                        r.pkg == caller.pkg && ctx.file(*r).target == ctx.file(caller).target
                    });
                }
            }
            match candidates.as_slice() {
                [] => {}
                [only] => {
                    out.insert(*only);
                }
                many => {
                    let mut names: Vec<String> = many
                        .iter()
                        .map(|r| format!("{} ({})", ctx.path(*r), ctx.func(*r).span.render()))
                        .collect();
                    names.sort();
                    refusals.push(Refusal::AmbiguousCall {
                        span: f.span.clone(),
                        callee: call.clone(),
                        candidates: names,
                    });
                }
            }
        }
        if !out.is_empty() {
            calls.insert(caller, out);
        }
    }
    (calls, refusals)
}

fn record(ty: &str, fields: &[(&str, String)]) -> String {
    let fields: Vec<String> = fields.iter().map(|(k, v)| format!("{k} := {v}")).collect();
    format!("(record {ty} {{{}}})", fields.join(", "))
}

fn architecture(ctx: &Model) -> Module {
    let ws = &ctx.ws;
    let t = "Foundation.Arch::";
    let mut decls = Vec::new();
    for (i, c) in ctx.components.iter().enumerate() {
        decls.push(Decl::value(
            local(&c.id),
            format!("{t}Component"),
            record(
                &format!("{t}Component"),
                &[
                    ("index", i.to_string()),
                    ("kind", format!("(ctor {t}ComponentKind.{})", c.kind)),
                    ("name", string(&c.name)),
                    ("text", string(&c.text)),
                ],
            ),
        ));
    }
    for (id, i, (from, to)) in indexed(
        ctx.edges
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    ) {
        decls.push(Decl::value(
            local(&id),
            format!("{t}Edge"),
            record(
                &format!("{t}Edge"),
                &[
                    ("index", i.to_string()),
                    ("fromIndex", ctx.index[&from].to_string()),
                    ("toIndex", ctx.index[&to].to_string()),
                    ("kind", format!("(ctor {t}EdgeKind.controlFlow)")),
                ],
            ),
        ));
    }
    let (stakeholders, concerns, viewpoints) = audience(ctx);
    for (id, i, (name, text)) in indexed(stakeholders) {
        decls.push(Decl::value(
            local(&id),
            format!("{t}Stakeholder"),
            record(
                &format!("{t}Stakeholder"),
                &[
                    ("index", i.to_string()),
                    ("name", string(&name)),
                    ("text", string(&text)),
                ],
            ),
        ));
    }
    let concern_rows = indexed(concerns);
    for (id, i, (name, text)) in &concern_rows {
        decls.push(Decl::value(
            local(id),
            format!("{t}Concern"),
            record(
                &format!("{t}Concern"),
                &[
                    ("index", i.to_string()),
                    ("name", string(name)),
                    ("text", string(text)),
                ],
            ),
        ));
    }
    let (stakeholder_index, concern_index) = audience_index(ctx);
    let viewpoint_rows = indexed(viewpoints);
    for (id, i, (stakeholder, concern, model)) in &viewpoint_rows {
        decls.push(Decl::value(
            local(id),
            format!("{t}Viewpoint"),
            record(
                &format!("{t}Viewpoint"),
                &[
                    ("index", i.to_string()),
                    (
                        "stakeholderIndex",
                        stakeholder_index[stakeholder].to_string(),
                    ),
                    ("concernIndex", concern_index[concern].to_string()),
                    ("modelKindIndex", model.to_string()),
                ],
            ),
        ));
    }
    for (id, i, (_, _, model)) in &viewpoint_rows {
        let view = id.replace("::viewpoint", "::view");
        decls.push(Decl::value(
            local(&view),
            format!("{t}View"),
            record(
                &format!("{t}View"),
                &[
                    ("index", i.to_string()),
                    ("viewpointIndex", i.to_string()),
                    ("modelKindIndex", model.to_string()),
                ],
            ),
        ));
    }
    for (name, index, id) in [
        ("standardQuality", 0, "ISO-25010-2023"),
        ("standardRiskManagement", 1, "ISO-27005-2022"),
        ("standardApplicationSecurity", 2, "ISO-27034-1-2011"),
        ("standardSecurityControls", 3, "ISO-27034-5-2017"),
        ("standardArchitectureDescription", 4, "ISO-42010-2022"),
    ] {
        let edition = id.rsplit('-').next().unwrap_or_default();
        decls.push(Decl::value(
            name,
            format!("{t}StandardReference"),
            record(
                &format!("{t}StandardReference"),
                &[
                    ("standardIndex", index.to_string()),
                    ("editionIndex", "0".to_string()),
                    ("standardId", string(id)),
                    ("edition", string(edition)),
                ],
            ),
        ));
    }
    let _ = ws;
    Module {
        name: ARCH.into(),
        glossaries: vec![BOOL, NAT, PARCH],
        imports: vec!["Foundation.Arch".into()],
        title: "architectural component",
        decls,
    }
}

type Named = Vec<(String, (String, String))>;
type Viewpoints = Vec<(String, (String, String, usize))>;

fn has_kind(ws: &Workspace, kind: TargetKind) -> bool {
    ws.packages
        .iter()
        .any(|p| p.targets.iter().any(|t| t.kind == kind))
}

/// Stakeholders, concerns and viewpoints, each derived from what the workspace has.
fn audience(ctx: &Model) -> (Named, Named, Viewpoints) {
    let ws = &ctx.ws;
    let a = |n: &str| qualified(ARCH, n);
    let mut authors: Vec<String> = ws.packages.iter().flat_map(|p| p.authors.clone()).collect();
    authors.sort();
    authors.dedup();
    let maintainers = if authors.is_empty() {
        format!(
            "Maintain the {} workspace and its {} packages.",
            ws.name,
            ws.packages.len()
        )
    } else {
        format!(
            "Maintain the {} workspace: {}.",
            ws.name,
            authors.join(", ")
        )
    };
    let mut stakeholders = vec![
        (
            a("stakeholderMaintainers"),
            ("Maintainers".to_string(), maintainers),
        ),
        (
            a("stakeholderSecurityReviewers"),
            (
                "Security reviewers".to_string(),
                "Judge what the code can reach beyond computation and what the build trusts."
                    .to_string(),
            ),
        ),
        (
            a("stakeholderQualityReviewers"),
            (
                "Quality reviewers".to_string(),
                "Judge documentation, tests, lints and compatibility.".to_string(),
            ),
        ),
    ];
    if has_kind(ws, TargetKind::Bin) {
        stakeholders.push((
            a("stakeholderCliUsers"),
            (
                "Command-line users".to_string(),
                "Run the workspace's binaries.".to_string(),
            ),
        ));
    }
    if has_kind(ws, TargetKind::Lib) {
        stakeholders.push((
            a("stakeholderLibraryUsers"),
            (
                "Library users".to_string(),
                "Call the public interface of the workspace's libraries.".to_string(),
            ),
        ));
    }
    let description = ws
        .packages
        .iter()
        .map(|p| p.description.clone())
        .filter(|d| !d.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    // UR-R04: every package author is a stakeholder.
    for author in &authors {
        let name = author
            .split('<')
            .next()
            .unwrap_or(author)
            .trim()
            .to_string();
        stakeholders.push((
            a(&format!("stakeholderAuthor{}", pascal(&[&name]))),
            (
                name,
                format!("Listed as an author of the {} workspace.", ws.name),
            ),
        ));
    }
    let mut concerns = vec![
        (
            a("concernFunctionality"),
            (
                "Functionality".to_string(),
                if description.is_empty() {
                    "What the public interface does and how it is accepted.".to_string()
                } else {
                    description
                },
            ),
        ),
        (
            a("concernSecurity"),
            (
                "Security".to_string(),
                "Files, network, processes, environment and unsafe code the targets reach."
                    .to_string(),
            ),
        ),
        (
            a("concernQuality"),
            (
                "Quality".to_string(),
                "Documentation coverage, tests, lint levels and the minimum supported Rust."
                    .to_string(),
            ),
        ),
        (
            a("concernSupplyChain"),
            (
                "Supply chain".to_string(),
                format!(
                    "The {} packages Cargo.lock pins and their sources.",
                    ws.lock.len()
                ),
            ),
        ),
    ];
    // UR-R03: a crate's doc comment states the concern the crate answers.
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (t, target) in package.targets.iter().enumerate() {
            let Some(component) = ctx.targets.get(&(pkg, t)) else {
                continue;
            };
            let doc = package
                .files
                .iter()
                .find(|f| f.rel == target.src)
                .map(|f| summary(&f.doc))
                .unwrap_or_default();
            if !doc.is_empty() {
                concerns.push((
                    a(&format!(
                        "concernCrate{}",
                        Model::component_suffix(component)
                    )),
                    (target.name.clone(), doc),
                ));
            }
        }
    }
    let user = if has_kind(ws, TargetKind::Bin) {
        a("stakeholderCliUsers")
    } else if has_kind(ws, TargetKind::Lib) {
        a("stakeholderLibraryUsers")
    } else {
        a("stakeholderMaintainers")
    };
    let viewpoints = vec![
        (
            a("viewpointMaintenance"),
            (a("stakeholderMaintainers"), a("concernFunctionality"), 0),
        ),
        (a("viewpointUsage"), (user, a("concernFunctionality"), 0)),
        (
            a("viewpointQuality"),
            (a("stakeholderQualityReviewers"), a("concernQuality"), 1),
        ),
        (
            a("viewpointSecurity"),
            (a("stakeholderSecurityReviewers"), a("concernSecurity"), 2),
        ),
        (
            a("viewpointSupplyChain"),
            (
                a("stakeholderSecurityReviewers"),
                a("concernSupplyChain"),
                2,
            ),
        ),
    ];
    (stakeholders, concerns, viewpoints)
}

fn audience_index(ctx: &Model) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
    let (s, c, _) = audience(ctx);
    (
        indexed(s).into_iter().map(|(id, i, _)| (id, i)).collect(),
        indexed(c).into_iter().map(|(id, i, _)| (id, i)).collect(),
    )
}

struct Interface {
    module: Module,
    feature_count: usize,
    register: Vec<RegisterRow>,
}

struct RegisterRow {
    id: String,
    suite: String,
    record: String,
    statement: String,
    calls: Vec<String>,
    acceptance: Vec<String>,
}

struct Feature {
    id: String,
    statement: String,
    calls: Vec<String>,
    acceptance: Vec<String>,
    span: Span,
    suite: String,
    journey: Option<(String, String)>,
}

/// Public functions of a library, in their order of declaration.
fn interface_fns(ctx: &Model) -> Vec<FnRef> {
    ctx.all_fns()
        .into_iter()
        .filter(|r| {
            let f = ctx.func(*r);
            ctx.product(*r) && f.public && ctx.kind(*r) == &TargetKind::Lib
        })
        .collect()
}

fn call_name(ctx: &Model, r: FnRef) -> String {
    let path = ctx.path(r);
    let parts: Vec<&str> = path.split("::").collect();
    qualified(IFACE, &format!("call{}", pascal(&parts)))
}

/// Whether a string literal of `r`, or of a function `r` reaches through
/// resolved calls, satisfies `pred`: a test that runs the binary through a
/// helper still runs it.
fn reaches_literal(ctx: &Model, r: FnRef, pred: impl Fn(&String) -> bool) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack = vec![r];
    while let Some(f) = stack.pop() {
        if !seen.insert(f) {
            continue;
        }
        if ctx.func(f).literals.iter().any(&pred) {
            return true;
        }
        stack.extend(ctx.calls.get(&f).into_iter().flatten().copied());
        // Helpers of a test file are not workspace functions the resolver
        // admits; a bare call to one in the same file still reaches it.
        let file = ctx.file(f);
        for call in &ctx.func(f).calls {
            for (idx, g) in file.fns.iter().enumerate() {
                if g.owner.is_none() && &g.name == call {
                    stack.push(FnRef { idx, ..f });
                }
            }
        }
    }
    false
}

fn interface(ctx: &Model) -> (Interface, Vec<Refusal>) {
    let ws = &ctx.ws;
    let mut refusals = Vec::new();
    let mut decls: Vec<Decl> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut push = |refusals: &mut Vec<Refusal>, decl: Decl, span: &Span| -> bool {
        if seen.insert(decl.name().to_string()) {
            decls.push(decl);
            true
        } else {
            refusals.push(Refusal::Collision {
                span: span.clone(),
                name: qualified(IFACE, decl.name()),
            });
            false
        }
    };
    // Acceptance rows: tests, doctests, examples.
    let mut tests: Vec<(FnRef, String)> = Vec::new();
    for r in ctx.all_fns() {
        let f = ctx.func(r);
        if !f.test {
            continue;
        }
        let file = ctx.file(r);
        let package = ctx.package(r);
        let target = &package.targets[file.target];
        let parts = [
            package.name.as_str(),
            target.name.as_str(),
            file.module.as_str(),
            f.name.as_str(),
        ];
        let name = format!("ac{}", pascal(&parts));
        let doc = summary(&f.doc);
        let text = if doc.is_empty() {
            format!("{} at {} passes.", f.name, f.span.render())
        } else {
            format!("{} at {}: {doc}", f.name, f.span.render())
        };
        if push(
            &mut refusals,
            Decl::value(name.clone(), "String", string(&text)),
            &f.span,
        ) {
            tests.push((r, qualified(IFACE, &name)));
        }
    }
    let mut doctests: BTreeMap<FnRef, String> = BTreeMap::new();
    for r in interface_fns(ctx) {
        let f = ctx.func(r);
        if f.doctests == 0 {
            continue;
        }
        let path = ctx.path(r);
        let parts: Vec<&str> = path.split("::").collect();
        let name = format!("acDoc{}", pascal(&parts));
        let text = format!(
            "The {} doctest(s) of {path} at {} pass.",
            f.doctests,
            f.span.render()
        );
        if push(
            &mut refusals,
            Decl::value(name.clone(), "String", string(&text)),
            &f.span,
        ) {
            doctests.insert(r, qualified(IFACE, &name));
        }
    }
    // Calls.
    let mut call_of: BTreeMap<FnRef, String> = BTreeMap::new();
    for r in interface_fns(ctx) {
        let f = ctx.func(r);
        let id = call_name(ctx, r);
        let text = format!("{}: {}", ctx.path(r), f.signature);
        if push(
            &mut refusals,
            Decl::value(local(&id), "String", string(&text)),
            &f.span,
        ) {
            call_of.insert(r, id);
        }
    }
    let mut features: Vec<Feature> = Vec::new();
    // Library modules with public functions.
    let mut by_module: BTreeMap<(usize, usize, String), Vec<FnRef>> = BTreeMap::new();
    for r in call_of.keys() {
        let file = ctx.file(*r);
        by_module
            .entry((r.pkg, file.target, file.module.clone()))
            .or_default()
            .push(*r);
    }
    for ((pkg, target, module), fns) in &by_module {
        let package = &ws.packages[*pkg];
        let crate_name = package.targets[*target].name.replace('-', "_");
        let module_file = package
            .files
            .iter()
            .find(|f| f.target == *target && &f.module == module);
        let doc = module_file.map(|f| summary(&f.doc)).unwrap_or_default();
        let path = if module.is_empty() {
            crate_name.clone()
        } else {
            format!("{crate_name}::{module}")
        };
        let names: Vec<String> = fns.iter().map(|r| ctx.func(*r).name.clone()).collect();
        let statement = if doc.is_empty() {
            format!("{path} offers {}.", names.join(", "))
        } else {
            format!("{path}: {doc}")
        };
        let mut acceptance: BTreeSet<String> = BTreeSet::new();
        let set: BTreeSet<FnRef> = fns.iter().copied().collect();
        for (test, row) in &tests {
            let reaches = ctx.calls.get(test).is_some_and(|c| !c.is_disjoint(&set));
            if reaches {
                acceptance.insert(row.clone());
            }
        }
        for r in fns {
            if let Some(row) = doctests.get(r) {
                acceptance.insert(row.clone());
            }
        }
        let module_parts: Vec<&str> = module.split("::").collect();
        let mut parts = vec![crate_name.as_str()];
        parts.extend(module_parts.iter().filter(|p| !p.is_empty()));
        features.push(Feature {
            id: qualified(IFACE, &format!("feature{}", pascal(&parts))),
            statement,
            calls: fns.iter().map(|r| call_of[r].clone()).collect(),
            acceptance: acceptance.into_iter().collect(),
            span: ctx.func(fns[0]).span.clone(),
            suite: package.name.clone(),
            journey: Some((
                qualified(ARCH, "stakeholderLibraryUsers"),
                qualified(IFACE, &format!("journeyLibrary{}", pascal(&[&crate_name]))),
            )),
        });
    }
    // Binaries: subcommands, or the entry point.
    for (pkg, package) in ws.packages.iter().enumerate() {
        for target in package.targets.iter().filter(|t| t.kind == TargetKind::Bin) {
            let bin = &target.name;
            let journey = Some((
                qualified(ARCH, "stakeholderCliUsers"),
                qualified(IFACE, &format!("journeyCli{}", pascal(&[bin]))),
            ));
            let package_tests: Vec<&(FnRef, String)> =
                tests.iter().filter(|(r, _)| r.pkg == pkg).collect();
            let exe = format!("CARGO_BIN_EXE_{bin}");
            let mut subcommands = Vec::new();
            for file in &package.files {
                let in_bin = package.targets[file.target].name == *bin
                    || package.targets[file.target].kind == TargetKind::Lib;
                if !in_bin {
                    continue;
                }
                for e in &file.enums {
                    if e.derives.iter().any(|d| d == "Subcommand") {
                        for v in &e.variants {
                            subcommands.push((v.clone(), e.span.clone()));
                        }
                    }
                }
            }
            if subcommands.is_empty() {
                let main = package.files.iter().find(|f| f.rel == target.src);
                let doc = main.map(|f| summary(&f.doc)).unwrap_or_default();
                let span = main
                    .and_then(|f| f.fns.iter().find(|x| x.name == "main"))
                    .map(|x| x.span.clone())
                    .unwrap_or(Span {
                        file: target.src.clone(),
                        line: 1,
                    });
                let call = format!("callCli{}Main", pascal(&[bin]));
                let text = if doc.is_empty() {
                    format!("{bin}: the binary's entry point.")
                } else {
                    format!("{bin}: {doc}")
                };
                if !push(
                    &mut refusals,
                    Decl::value(call.clone(), "String", string(&text)),
                    &span,
                ) {
                    continue;
                }
                let acceptance: Vec<String> = package_tests
                    .iter()
                    .filter(|(r, _)| reaches_literal(ctx, *r, |l| l.contains(&exe)))
                    .map(|(_, row)| row.clone())
                    .collect();
                features.push(Feature {
                    id: qualified(IFACE, &format!("feature{}Main", pascal(&[bin]))),
                    statement: text,
                    calls: vec![qualified(IFACE, &call)],
                    acceptance,
                    span,
                    suite: package.name.clone(),
                    journey: journey.clone(),
                });
            } else {
                for (variant, span) in subcommands {
                    let command = kebab(&variant.name);
                    let call = format!("callCli{}{}", pascal(&[bin]), pascal(&[&variant.name]));
                    let doc = summary(&variant.doc);
                    let text = if doc.is_empty() {
                        format!("{bin} {command}")
                    } else {
                        format!("{bin} {command}: {doc}")
                    };
                    if !push(
                        &mut refusals,
                        Decl::value(call.clone(), "String", string(&text)),
                        &span,
                    ) {
                        continue;
                    }
                    let acceptance: Vec<String> = package_tests
                        .iter()
                        .filter(|(r, _)| reaches_literal(ctx, *r, |l| *l == command))
                        .map(|(_, row)| row.clone())
                        .collect();
                    // A subcommand named as a library module of the same
                    // crate is told apart from that module's feature.
                    let mut id = qualified(
                        IFACE,
                        &format!("feature{}{}", pascal(&[bin]), pascal(&[&variant.name])),
                    );
                    if features.iter().any(|f| f.id == id) {
                        id = qualified(
                            IFACE,
                            &format!(
                                "feature{}Command{}",
                                pascal(&[bin]),
                                pascal(&[&variant.name])
                            ),
                        );
                    }
                    features.push(Feature {
                        id,
                        statement: text,
                        calls: vec![qualified(IFACE, &call)],
                        acceptance,
                        span,
                        suite: package.name.clone(),
                        journey: journey.clone(),
                    });
                }
            }
        }
    }
    // Admission: a feature without acceptance is refused, not emitted.
    let mut admitted = Vec::new();
    for feature in features {
        if feature.acceptance.is_empty() {
            refusals.push(Refusal::Unaccepted {
                span: feature.span.clone(),
                feature: feature.id.clone(),
            });
            continue;
        }
        admitted.push(feature);
    }
    let t = "Foundation.Arch::";
    let mut journeys: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for feature in &admitted {
        decls.push(Decl::value(
            local(&feature.id),
            format!("{t}Feature"),
            record(
                &format!("{t}Feature"),
                &[
                    ("statement", string(&feature.statement)),
                    ("calls", string_list(&feature.calls)),
                    ("acceptance", string_list(&feature.acceptance)),
                ],
            ),
        ));
        if let Some((actor, journey)) = &feature.journey {
            journeys
                .entry(journey.clone())
                .or_insert_with(|| (actor.clone(), Vec::new()))
                .1
                .push(feature.id.clone());
        }
    }
    // UR-R09: an example is a journey through the features its main calls.
    let feature_of: BTreeMap<&String, &String> = admitted
        .iter()
        .flat_map(|f| f.calls.iter().map(move |c| (c, &f.id)))
        .collect();
    for r in ctx.all_fns() {
        let f = ctx.func(r);
        if f.name != "main" || ctx.kind(r) != &TargetKind::Example {
            continue;
        }
        let file = ctx.file(r);
        let package = ctx.package(r);
        let mut callees: Vec<FnRef> = ctx.calls.get(&r).into_iter().flatten().copied().collect();
        callees.sort_by_key(|c| (ctx.func(*c).span.clone(), *c));
        let mut steps: Vec<String> = Vec::new();
        for callee in callees {
            if let Some(feature) = call_of.get(&callee).and_then(|c| feature_of.get(c)) {
                if steps.last() != Some(*feature) {
                    steps.push((*feature).clone());
                }
            }
        }
        if steps.is_empty() {
            continue;
        }
        let target = &package.targets[file.target].name;
        journeys.insert(
            qualified(
                IFACE,
                &format!("journeyExample{}", pascal(&[&package.name, target])),
            ),
            (qualified(ARCH, "stakeholderLibraryUsers"), steps),
        );
    }
    for (journey, (actor, steps)) in &journeys {
        decls.push(Decl::value(
            local(journey),
            format!("{t}Journey"),
            record(
                &format!("{t}Journey"),
                &[("actor", string(actor)), ("steps", string_list(steps))],
            ),
        ));
    }
    let mut sorted: Vec<&Feature> = admitted.iter().collect();
    sorted.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
    let width = sorted.len().to_string().len().max(2);
    let register = sorted
        .iter()
        .enumerate()
        .map(|(i, f)| RegisterRow {
            id: format!("FT-{:0width$}", i + 1),
            suite: f.suite.clone(),
            record: f.id.clone(),
            statement: f.statement.clone(),
            calls: f.calls.clone(),
            acceptance: f.acceptance.clone(),
        })
        .collect();
    let interface = Interface {
        feature_count: admitted.len(),
        module: Module {
            name: IFACE.into(),
            glossaries: vec![BOOL, NAT, PARCH],
            imports: vec!["Foundation.Arch".into()],
            title: "feature",
            decls,
        },
        register,
    };
    (interface, refusals)
}

/// Unit-variant enums of product code named for a state, or with transitions.
/// Ask the router for every enum of product code: `error-enum` when it is named
/// for an error or implements `Error`, `state-enum` when it is fieldless and
/// named for a state or matched in transitions. An enum with both tags is
/// refused as ambiguous and becomes neither record.
#[allow(clippy::type_complexity)]
fn route_enums(ctx: &Model) -> (BTreeMap<(usize, usize, usize), &'static str>, Vec<Refusal>) {
    let mut routes = BTreeMap::new();
    let mut refusals = Vec::new();
    let transitions: BTreeSet<String> = ctx
        .all_fns()
        .into_iter()
        .filter(|r| ctx.product(*r))
        .flat_map(|r| {
            ctx.func(r)
                .transitions
                .iter()
                .map(|t| t.0.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    for (pkg, package) in ctx.ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            if !matches!(
                package.targets[file.target].kind,
                TargetKind::Lib | TargetKind::Bin
            ) {
                continue;
            }
            for (ei, e) in file.enums.iter().enumerate() {
                let unit = e.variants.len() >= 2 && e.variants.iter().all(|v| v.unit);
                let named = ["State", "Mode", "Phase", "Status", "Stage"]
                    .iter()
                    .any(|s| e.name.ends_with(s));
                let cli = e
                    .derives
                    .iter()
                    .any(|d| matches!(d.as_str(), "Subcommand" | "ValueEnum" | "Parser"));
                let mut tags = Vec::new();
                if !e.variants.is_empty()
                    && (e.name.ends_with("Error") || file.error_impls.contains(&e.name))
                {
                    tags.push("error-enum");
                }
                if unit && !cli && (named || transitions.contains(&e.name)) {
                    tags.push("state-enum");
                }
                match crate::route::classify(&tags) {
                    Some(crate::route::Routing::Routed(route)) => {
                        routes.insert((pkg, fi, ei), route.id);
                    }
                    Some(crate::route::Routing::Ambiguous(a, b)) => {
                        refusals.push(Refusal::AmbiguousFact {
                            span: e.span.clone(),
                            fact: e.name.clone(),
                            tags: vec![a, b],
                        });
                    }
                    Some(crate::route::Routing::Unknown(_)) | None => {}
                }
            }
        }
    }
    (routes, refusals)
}

fn state_enums(ctx: &Model) -> Vec<(usize, usize, usize)> {
    ctx.enum_routes
        .iter()
        .filter(|(_, route)| **route == "UR-R26")
        .map(|(key, _)| *key)
        .collect()
}

fn design(ctx: &Model) -> (Module, Vec<Refusal>) {
    let mut refusals = Vec::new();
    let t = "Foundation.Arch::";
    let mut decls = Vec::new();
    let mut processes: BTreeMap<FnRef, String> = BTreeMap::new();
    let process_id = |ctx: &Model, r: FnRef| {
        let path = ctx.path(r);
        let parts: Vec<&str> = path.split("::").collect();
        qualified(DESIGN, &format!("process{}", pascal(&parts)))
    };
    for r in interface_fns(ctx) {
        processes.insert(r, process_id(ctx, r));
    }
    for r in ctx.all_fns() {
        let f = ctx.func(r);
        if ctx.product(r)
            && (f.name == "main" && f.owner.is_none() && ctx.kind(r) == &TargetKind::Bin
                || !f.transitions.is_empty())
        {
            processes.insert(r, process_id(ctx, r));
        }
    }
    let mut process_names: BTreeSet<String> = BTreeSet::new();
    processes.retain(|r, id| ctx.component_of(*r).is_some() && process_names.insert(id.clone()));
    for (r, id) in &processes {
        let owner = ctx.component_of(*r).cloned().unwrap_or_default();
        decls.push(Decl::value(
            local(id),
            format!("{t}SystemProcess"),
            record(&format!("{t}SystemProcess"), &[("owner", string(&owner))]),
        ));
    }
    // States.
    let mut machines = Vec::new();
    for (pkg, fi, ei) in state_enums(ctx) {
        let package = &ctx.ws.packages[pkg];
        let file = &package.files[fi];
        let e = &file.enums[ei];
        let Some(owner) = ctx.targets.get(&(pkg, file.target)).cloned() else {
            continue;
        };
        let base = pascal(&[&package.targets[file.target].name, &e.name]);
        let states: Vec<(String, String)> = e
            .variants
            .iter()
            .map(|v| {
                (
                    v.name.clone(),
                    qualified(DESIGN, &format!("state{base}{}", pascal(&[&v.name]))),
                )
            })
            .collect();
        for (_, id) in &states {
            decls.push(Decl::value(
                local(id),
                format!("{t}SystemState"),
                record(&format!("{t}SystemState"), &[("owner", string(&owner))]),
            ));
        }
        // Transitions from fns of the same component.
        let mut transitions = Vec::new();
        for (r, process) in &processes {
            if ctx.component_of(*r) != Some(&owner) {
                continue;
            }
            for (enum_name, from, to) in &ctx.func(*r).transitions {
                if enum_name == &e.name {
                    transitions.push((
                        ctx.func(*r).span.clone(),
                        from.clone(),
                        to.clone(),
                        process.clone(),
                    ));
                }
            }
        }
        transitions.sort();
        transitions.dedup();
        let initial = if let Some(v) = e.variants.iter().find(|v| v.default) {
            Some(v.name.clone())
        } else {
            let entered: BTreeSet<&String> = transitions.iter().map(|t| &t.2).collect();
            let sources: Vec<&String> = e
                .variants
                .iter()
                .map(|v| &v.name)
                .filter(|v| !entered.contains(v))
                .collect();
            match sources.as_slice() {
                [only] if !transitions.is_empty() => Some((*only).clone()),
                _ => None,
            }
        };
        if initial.is_none() {
            refusals.push(Refusal::NoInitialState {
                span: e.span.clone(),
                machine: format!("{}::{}", package.targets[file.target].name, e.name),
            });
        }
        machines.push((base, e.name.clone(), owner, states, transitions, initial));
    }
    // UML component view.
    let comp_view = qualified(DESIGN, "umlComponents");
    decls.push(Decl::value(
        local(&comp_view),
        format!("{t}DesignView"),
        record(
            &format!("{t}DesignView"),
            &[
                ("title", string("Components and dependencies")),
                (
                    "viewpoint",
                    string(&qualified(ARCH, "viewpointMaintenance")),
                ),
                ("diagram", format!("(ctor {t}DesignDiagram.umlComponent)")),
            ],
        ),
    ));
    for c in &ctx.components {
        decls.push(Decl::value(
            format!("uml{}", Model::component_suffix(&c.id)),
            format!("{t}UmlComponent"),
            record(
                &format!("{t}UmlComponent"),
                &[("view", string(&comp_view)), ("component", string(&c.id))],
            ),
        ));
    }
    for id in ctx.edges.keys() {
        decls.push(Decl::value(
            format!("umlConnector{}", local(id).trim_start_matches("edge")),
            format!("{t}UmlConnector"),
            record(
                &format!("{t}UmlConnector"),
                &[("view", string(&comp_view)), ("edge", string(id))],
            ),
        ));
    }
    let connected = |a: &str, b: &str| ctx.edges.values().any(|(f, t)| f == a && t == b);
    // Sequence views: each bin main's calls into other components.
    for (r, process) in &processes {
        let f = ctx.func(*r);
        if f.name != "main" || ctx.kind(*r) != &TargetKind::Bin {
            continue;
        }
        let Some(bin) = ctx.component_of(*r).cloned() else {
            continue;
        };
        let mut messages = Vec::new();
        let mut lifelines = vec![bin.clone()];
        let mut order: Vec<FnRef> = ctx.calls.get(r).into_iter().flatten().copied().collect();
        order.sort_by_key(|c| (ctx.func(*c).span.clone(), *c));
        for callee in order {
            let Some(target) = ctx.component_of(callee).cloned() else {
                continue;
            };
            let Some(callee_process) = processes.get(&callee) else {
                continue;
            };
            if target == bin || !connected(&bin, &target) {
                continue;
            }
            if !lifelines.contains(&target) {
                lifelines.push(target.clone());
            }
            messages.push((target, callee_process.clone()));
        }
        if messages.is_empty() {
            continue;
        }
        let base = local(process).trim_start_matches("process").to_string();
        let view = qualified(DESIGN, &format!("sequence{base}"));
        decls.push(Decl::value(
            local(&view),
            format!("{t}DesignView"),
            record(
                &format!("{t}DesignView"),
                &[
                    ("title", string(&format!("{} calls", ctx.path(*r)))),
                    ("viewpoint", string(&qualified(ARCH, "viewpointUsage"))),
                    ("diagram", format!("(ctor {t}DesignDiagram.umlSequence)")),
                ],
            ),
        ));
        for (i, lifeline) in lifelines.iter().enumerate() {
            decls.push(Decl::value(
                format!("lifeline{base}{}", Model::component_suffix(lifeline)),
                format!("{t}UmlLifeline"),
                record(
                    &format!("{t}UmlLifeline"),
                    &[
                        ("view", string(&view)),
                        ("component", string(lifeline)),
                        ("order", i.to_string()),
                    ],
                ),
            ));
        }
        for (i, (target, callee_process)) in messages.iter().enumerate() {
            decls.push(Decl::value(
                format!("message{base}{i}"),
                format!("{t}UmlMessage"),
                record(
                    &format!("{t}UmlMessage"),
                    &[
                        ("view", string(&view)),
                        ("kind", format!("(ctor {t}UmlMessageKind.synchronous)")),
                        ("order", i.to_string()),
                        ("source", string(&bin)),
                        ("target", string(target)),
                        ("process", string(callee_process)),
                    ],
                ),
            ));
        }
    }
    // State machines.
    for (base, name, _owner, states, transitions, initial) in &machines {
        let view = qualified(DESIGN, &format!("machine{base}"));
        decls.push(Decl::value(
            local(&view),
            format!("{t}DesignView"),
            record(
                &format!("{t}DesignView"),
                &[
                    ("title", string(&format!("{name} states"))),
                    (
                        "viewpoint",
                        string(&qualified(ARCH, "viewpointMaintenance")),
                    ),
                    (
                        "diagram",
                        format!("(ctor {t}DesignDiagram.umlStateMachine)"),
                    ),
                ],
            ),
        ));
        for (variant, id) in states {
            decls.push(Decl::value(
                format!("uml{}", local(id)),
                format!("{t}UmlState"),
                record(
                    &format!("{t}UmlState"),
                    &[
                        ("view", string(&view)),
                        ("state", string(id)),
                        ("initial", (initial.as_ref() == Some(variant)).to_string()),
                    ],
                ),
            ));
        }
        let state_of: BTreeMap<&String, &String> = states.iter().map(|(v, id)| (v, id)).collect();
        for (i, (_, from, to, process)) in transitions.iter().enumerate() {
            let (Some(source), Some(target)) = (state_of.get(from), state_of.get(to)) else {
                continue;
            };
            decls.push(Decl::value(
                format!("transition{base}{i}"),
                format!("{t}UmlTransition"),
                record(
                    &format!("{t}UmlTransition"),
                    &[
                        ("view", string(&view)),
                        ("order", i.to_string()),
                        ("source", string(source)),
                        ("target", string(target)),
                        ("trigger", string(process)),
                    ],
                ),
            ));
        }
    }
    // OPM: each component with processes, its processes, invocations among them
    // and the states of its machines.
    let mut by_component: BTreeMap<String, Vec<(FnRef, String)>> = BTreeMap::new();
    for (r, id) in &processes {
        if let Some(c) = ctx.component_of(*r) {
            by_component
                .entry(c.clone())
                .or_default()
                .push((*r, id.clone()));
        }
    }
    for (component, procs) in &by_component {
        let suffix = Model::component_suffix(component).to_string();
        let view = qualified(DESIGN, &format!("opm{suffix}"));
        decls.push(Decl::value(
            local(&view),
            format!("{t}DesignView"),
            record(
                &format!("{t}DesignView"),
                &[
                    (
                        "title",
                        string(&format!(
                            "{} processes",
                            ctx.components[ctx.index[component]].name
                        )),
                    ),
                    (
                        "viewpoint",
                        string(&qualified(ARCH, "viewpointMaintenance")),
                    ),
                    (
                        "diagram",
                        format!("(ctor {t}DesignDiagram.opmObjectProcess)"),
                    ),
                ],
            ),
        ));
        decls.push(Decl::value(
            format!("opm{suffix}Object"),
            format!("{t}OpmThing"),
            record(
                &format!("{t}OpmThing"),
                &[
                    ("view", string(&view)),
                    ("kind", format!("(ctor {t}OpmThingKind.object)")),
                    ("subject", string(component)),
                ],
            ),
        ));
        let in_view: BTreeSet<FnRef> = procs.iter().map(|(r, _)| *r).collect();
        for (r, id) in procs {
            let p = local(id).trim_start_matches("process");
            decls.push(Decl::value(
                format!("opm{suffix}Thing{p}"),
                format!("{t}OpmThing"),
                record(
                    &format!("{t}OpmThing"),
                    &[
                        ("view", string(&view)),
                        ("kind", format!("(ctor {t}OpmThingKind.process)")),
                        ("subject", string(id)),
                    ],
                ),
            ));
            decls.push(Decl::value(
                format!("opm{suffix}Instrument{p}"),
                format!("{t}OpmLink"),
                record(
                    &format!("{t}OpmLink"),
                    &[
                        ("view", string(&view)),
                        ("kind", format!("(ctor {t}OpmLinkKind.instrument)")),
                        ("source", string(component)),
                        ("target", string(id)),
                    ],
                ),
            ));
            for callee in ctx.calls.get(r).into_iter().flatten() {
                if !in_view.contains(callee) || callee == r {
                    continue;
                }
                let q = local(&processes[callee])
                    .trim_start_matches("process")
                    .to_string();
                decls.push(Decl::value(
                    format!("opm{suffix}Invokes{p}{q}"),
                    format!("{t}OpmLink"),
                    record(
                        &format!("{t}OpmLink"),
                        &[
                            ("view", string(&view)),
                            ("kind", format!("(ctor {t}OpmLinkKind.invocation)")),
                            ("source", string(id)),
                            ("target", string(&processes[callee])),
                        ],
                    ),
                ));
            }
        }
        for (_, _, owner, states, _, _) in &machines {
            if owner != component {
                continue;
            }
            for (_, state) in states {
                decls.push(Decl::value(
                    format!("opm{suffix}{}", pascal(&[local(state)])),
                    format!("{t}OpmState"),
                    record(
                        &format!("{t}OpmState"),
                        &[
                            ("view", string(&view)),
                            ("thing", string(component)),
                            ("state", string(state)),
                        ],
                    ),
                ));
            }
        }
    }
    decls.extend(module_opm(ctx, &processes));
    let mut names = BTreeSet::new();
    decls.retain(|d| names.insert(d.name().to_string()));
    let module = Module {
        name: DESIGN.into(),
        glossaries: vec![BOOL, NAT, PARCH],
        imports: vec!["Foundation.Arch".into()],
        title: "design view",
        decls,
    };
    (module, refusals)
}

/// OPM per module (UR-R28): the module's component is the object, its
/// processes are the module's functions, each invocation inside the module is
/// a link and each process zooms into the processes it calls there.
fn module_opm(ctx: &Model, processes: &BTreeMap<FnRef, String>) -> Vec<Decl> {
    let t = "Foundation.Arch::";
    let mut by_module: BTreeMap<(String, String, String), Vec<FnRef>> = BTreeMap::new();
    for r in processes.keys() {
        let Some(component) = ctx.component_of(*r) else {
            continue;
        };
        let file = ctx.file(*r);
        let target = &ctx.package(*r).targets[file.target].name;
        let path = if file.module.is_empty() {
            target.replace('-', "_")
        } else {
            format!("{}::{}", target.replace('-', "_"), file.module)
        };
        by_module
            .entry((component.clone(), file.module.clone(), path))
            .or_default()
            .push(*r);
    }
    let mut decls = Vec::new();
    for ((component, module, path), procs) in &by_module {
        let parts: Vec<&str> = module.split("::").filter(|p| !p.is_empty()).collect();
        let suffix = format!("{}{}", Model::component_suffix(component), pascal(&parts));
        let view = qualified(DESIGN, &format!("opmModule{suffix}"));
        let name = local(&view).to_string();
        decls.push(Decl::value(
            name.clone(),
            format!("{t}DesignView"),
            record(
                &format!("{t}DesignView"),
                &[
                    ("title", string(&format!("{path} processes"))),
                    (
                        "viewpoint",
                        string(&qualified(ARCH, "viewpointMaintenance")),
                    ),
                    (
                        "diagram",
                        format!("(ctor {t}DesignDiagram.opmObjectProcess)"),
                    ),
                ],
            ),
        ));
        decls.push(Decl::value(
            format!("{name}Object"),
            format!("{t}OpmThing"),
            record(
                &format!("{t}OpmThing"),
                &[
                    ("view", string(&view)),
                    ("kind", format!("(ctor {t}OpmThingKind.object)")),
                    ("subject", string(component)),
                ],
            ),
        ));
        let in_view: BTreeSet<FnRef> = procs.iter().copied().collect();
        let short = |r: &FnRef| {
            local(&processes[r])
                .trim_start_matches("process")
                .to_string()
        };
        let mut parent: BTreeMap<FnRef, FnRef> = BTreeMap::new();
        for r in procs {
            let id = &processes[r];
            let p = short(r);
            decls.push(Decl::value(
                format!("{name}Thing{p}"),
                format!("{t}OpmThing"),
                record(
                    &format!("{t}OpmThing"),
                    &[
                        ("view", string(&view)),
                        ("kind", format!("(ctor {t}OpmThingKind.process)")),
                        ("subject", string(id)),
                    ],
                ),
            ));
            decls.push(Decl::value(
                format!("{name}Instrument{p}"),
                format!("{t}OpmLink"),
                record(
                    &format!("{t}OpmLink"),
                    &[
                        ("view", string(&view)),
                        ("kind", format!("(ctor {t}OpmLinkKind.instrument)")),
                        ("source", string(component)),
                        ("target", string(id)),
                    ],
                ),
            ));
            let mut order = 0usize;
            for callee in ctx.calls.get(r).into_iter().flatten() {
                if !in_view.contains(callee) || callee == r {
                    continue;
                }
                let q = short(callee);
                decls.push(Decl::value(
                    format!("{name}Invokes{p}{q}"),
                    format!("{t}OpmLink"),
                    record(
                        &format!("{t}OpmLink"),
                        &[
                            ("view", string(&view)),
                            ("kind", format!("(ctor {t}OpmLinkKind.invocation)")),
                            ("source", string(id)),
                            ("target", string(&processes[callee])),
                        ],
                    ),
                ));
                // A process zooms under one parent only, its first caller in
                // declaration order, and never under its own descendant.
                let mut ancestor = Some(*r);
                let mut cycle = false;
                while let Some(a) = ancestor {
                    cycle |= a == *callee;
                    ancestor = parent.get(&a).copied().filter(|_| !cycle);
                }
                if cycle || parent.contains_key(callee) {
                    continue;
                }
                parent.insert(*callee, *r);
                decls.push(Decl::value(
                    format!("{name}Zoom{p}{q}"),
                    format!("{t}OpmZoom"),
                    record(
                        &format!("{t}OpmZoom"),
                        &[
                            ("view", string(&view)),
                            ("process", string(id)),
                            ("subprocess", string(&processes[callee])),
                            ("order", order.to_string()),
                        ],
                    ),
                ));
                order += 1;
            }
        }
    }
    decls
}

struct Threat {
    asset: String,
    asset_name: String,
    asset_text: String,
    component: String,
    threat: String,
    threat_name: String,
    threat_text: String,
    stride: &'static str,
    likelihood: &'static str,
    impact: &'static str,
    control_name: String,
    control_text: String,
    evidence: Vec<(&'static str, String)>,
    /// Whether the control's verification passes today.
    passing: bool,
    /// The rationale of an accepted risk, which has no control.
    accepted: Option<String>,
}

fn capability_threat(
    cap: Capability,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    // (asset noun, threat sentence, STRIDE, likelihood, impact)
    match cap {
        Capability::Fs => (
            "Files it reads and writes",
            "A path or file content an attacker controls alters what is read or overwritten.",
            "tampering",
            "medium",
            "major",
        ),
        Capability::Net => (
            "Network connections it opens",
            "Data sent or received over the network is observed or redirected.",
            "informationDisclosure",
            "medium",
            "major",
        ),
        Capability::Process => (
            "Processes it starts",
            "A started process runs with arguments or a program an attacker chose.",
            "elevationOfPrivilege",
            "medium",
            "critical",
        ),
        Capability::Env => (
            "Environment and arguments it reads",
            "A variable or argument an attacker sets impersonates the configuration the operator meant.",
            "spoofing",
            "low",
            "major",
        ),
        Capability::Unsafe => (
            "Memory it manages with unsafe code",
            "An unsafe block breaks an invariant and corrupts memory.",
            "elevationOfPrivilege",
            "low",
            "critical",
        ),
        Capability::Ffi => (
            "Foreign functions it calls",
            "A foreign function violates the contract its declaration states.",
            "elevationOfPrivilege",
            "low",
            "critical",
        ),
    }
}

fn threats(ctx: &Model) -> Vec<Threat> {
    let ws = &ctx.ws;
    let mut out = Vec::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (t, target) in package.targets.iter().enumerate() {
            let Some(component) = ctx.targets.get(&(pkg, t)) else {
                continue;
            };
            let suffix = Model::component_suffix(component).to_string();
            let files: Vec<&SourceFile> = package.files.iter().filter(|f| f.target == t).collect();
            let mut sites: BTreeMap<Capability, Vec<String>> = BTreeMap::new();
            for file in &files {
                for (cap, span, what) in &file.capabilities {
                    sites
                        .entry(*cap)
                        .or_default()
                        .push(format!("{} {what}", span.render()));
                }
            }
            // Every target answers for memory safety, unsafe code or not.
            let forbids = files
                .iter()
                .any(|f| f.rel == target.src && f.forbids_unsafe)
                || package
                    .lints
                    .get("rust.unsafe_code")
                    .is_some_and(|l| l == "forbid");
            if !sites.contains_key(&Capability::Unsafe) {
                let evidence = if forbids {
                    let where_ = if files
                        .iter()
                        .any(|f| f.rel == target.src && f.forbids_unsafe)
                    {
                        format!("#![forbid(unsafe_code)] in {}", target.src)
                    } else {
                        format!(
                            "[lints.rust] unsafe_code = forbid in {}/Cargo.toml",
                            package.dir
                        )
                    };
                    vec![("gate", where_)]
                } else {
                    vec![(
                        "review",
                        format!(
                            "uorust found no unsafe block, unsafe fn or unsafe impl in {}",
                            target.name
                        ),
                    )]
                };
                out.push(Threat {
                    asset: qualified(SEC, &format!("asset{suffix}Memory")),
                    asset_name: format!("{} memory", target.name),
                    asset_text: format!("The memory of {} while it runs.", target.name),
                    component: component.clone(),
                    threat: qualified(SEC, &format!("threat{suffix}Memory")),
                    threat_name: format!("Memory corruption in {}", target.name),
                    threat_text: "Unsafe code breaks an invariant the borrow checker would have kept."
                        .to_string(),
                    stride: "elevationOfPrivilege",
                    likelihood: "low",
                    impact: "critical",
                    control_name: if forbids {
                        "Unsafe code forbidden".to_string()
                    } else {
                        "Safe Rust only".to_string()
                    },
                    control_text: format!(
                        "{} is checked by the borrow checker throughout; no unsafe code is admitted.",
                        target.name
                    ),
                    evidence,
                    passing: true,
                    accepted: None,
                });
            }
            for (cap, list) in sites {
                let (noun, sentence, stride, likelihood, impact) = capability_threat(cap);
                let key = cap.key();
                out.push(Threat {
                    asset: qualified(SEC, &format!("asset{suffix}{key}")),
                    asset_name: format!("{}: {}", target.name, noun.to_lowercase()),
                    asset_text: format!("{noun} in {}, at {} site(s).", target.name, list.len()),
                    component: component.clone(),
                    threat: qualified(SEC, &format!("threat{suffix}{key}")),
                    threat_name: format!("{} misuse in {}", key, target.name),
                    threat_text: sentence.to_string(),
                    stride,
                    likelihood,
                    impact,
                    control_name: format!("Reviewed {} sites", key.to_lowercase()),
                    control_text: format!(
                        "Each {} site of {} is listed with its span for review.",
                        key.to_lowercase(),
                        target.name
                    ),
                    evidence: list.into_iter().map(|s| ("review", s)).collect(),
                    passing: true,
                    accepted: None,
                });
            }
        }
    }
    if let Some(first) = ctx.components.iter().find(|c| c.kind != "externalSystem") {
        let mut evidence = vec![(
            "gate",
            format!(
                "Cargo.lock pins {} packages, {} with a registry checksum",
                ws.lock.len(),
                ws.lock.iter().filter(|l| !l.checksum.is_empty()).count()
            ),
        )];
        if ws.has_deny {
            evidence.push(("gate", "cargo deny check: deny.toml".to_string()));
        }
        out.push(Threat {
            asset: qualified(SEC, "assetLockedDependencies"),
            asset_name: "Locked dependency graph".to_string(),
            asset_text: format!("The {} packages Cargo.lock pins.", ws.lock.len()),
            component: first.id.clone(),
            threat: qualified(SEC, "threatDependencySubstitution"),
            threat_name: "Dependency substitution".to_string(),
            threat_text: "A dependency is replaced by a package with the same name and different code."
                .to_string(),
            stride: "tampering",
            likelihood: "low",
            impact: "critical",
            control_name: "Locked and checked dependencies".to_string(),
            control_text: "Every dependency is pinned with its checksum and checked against advisories and licences."
                .to_string(),
            evidence,
            passing: true,
            accepted: None,
        });
        out.extend(advisory_threats(ctx, &first.id));
    }
    out
}

/// UR-R21: each advisory that affects a locked package is a risk on the
/// supply chain, answered by the advisory gate or accepted where `deny.toml`
/// ignores it.
fn advisory_threats(ctx: &Model, first: &str) -> Vec<Threat> {
    let ws = &ctx.ws;
    let mut out = Vec::new();
    let Some(db) = &ws.advisories else {
        return out;
    };
    for a in &db.hits {
        let key: String = format!("{} v{}", a.id, a.version.replace('.', "p"))
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let suffix = pascal(&[&key]);
        let component = ctx
            .components
            .iter()
            .find(|c| c.kind == "externalSystem" && c.name == a.package)
            .map_or_else(|| first.to_string(), |c| c.id.clone());
        let has = |c: &str| a.categories.iter().any(|x| x == c);
        let stride =
            if has("code-execution") || has("memory-corruption") || has("privilege-escalation") {
                "elevationOfPrivilege"
            } else if has("denial-of-service") {
                "denialOfService"
            } else if has("memory-exposure") || has("crypto-failure") {
                "informationDisclosure"
            } else {
                "tampering"
            };
        let (likelihood, impact) = match (&a.informational, stride) {
            (Some(_), _) => ("low", "minor"),
            (None, "elevationOfPrivilege") => ("medium", "critical"),
            (None, _) => ("medium", "major"),
        };
        let patched = if a.patched.is_empty() {
            "no patched release".to_string()
        } else {
            format!("patched in {}", a.patched.join(", "))
        };
        let accepted = ws.deny_ignores.get(&a.id).map(|reason| {
            if reason.is_empty() {
                format!("deny.toml ignores {} with no reason given", a.id)
            } else {
                format!("deny.toml ignores {}: {reason}", a.id)
            }
        });
        if accepted.is_none() && !ws.has_deny {
            continue;
        }
        let title = if a.title.is_empty() {
            a.id.clone()
        } else {
            a.title.clone()
        };
        out.push(Threat {
            asset: qualified(SEC, &format!("assetAdvisory{suffix}")),
            asset_name: format!("{} {}", a.package, a.version),
            asset_text: format!("The locked package {} {}.", a.package, a.version),
            component,
            threat: qualified(SEC, &format!("threatAdvisory{suffix}")),
            threat_name: format!("{}: {title}", a.id),
            threat_text: format!(
                "{} ({}) affects {} {}; {patched}.",
                a.id,
                if a.date.is_empty() {
                    "undated"
                } else {
                    &a.date
                },
                a.package,
                a.version
            ),
            stride,
            likelihood,
            impact,
            control_name: format!("Advisory gate for {}", a.id),
            control_text: format!(
                "cargo deny check advisories fails while {} affects {} {}.",
                a.id, a.package, a.version
            ),
            evidence: vec![(
                "gate",
                format!(
                    "cargo deny check advisories: deny.toml; advisory database {}",
                    db.revision
                ),
            )],
            passing: false,
            accepted,
        });
    }
    out
}

fn security(ctx: &Model, options: &Options) -> Module {
    let s = "Foundation.Sec::";
    let tm = "Production.ThreatModel::";
    let rows = threats(ctx);
    let mut decls = Vec::new();
    let assets = indexed(rows.iter().map(|r| (r.asset.clone(), r)).collect());
    for (id, i, r) in &assets {
        decls.push(Decl::value(
            local(id),
            format!("{s}Asset"),
            record(
                &format!("{s}Asset"),
                &[
                    ("index", i.to_string()),
                    ("componentIndex", ctx.index[&r.component].to_string()),
                ],
            ),
        ));
    }
    let asset_index: BTreeMap<&String, usize> = assets.iter().map(|(id, i, _)| (id, *i)).collect();
    let threat_rows = indexed(rows.iter().map(|r| (r.threat.clone(), r)).collect());
    for (id, i, _) in &threat_rows {
        decls.push(Decl::value(
            local(id),
            format!("{s}Threat"),
            record(&format!("{s}Threat"), &[("index", i.to_string())]),
        ));
    }
    let threat_index: BTreeMap<&String, usize> =
        threat_rows.iter().map(|(id, i, _)| (id, *i)).collect();
    let risk_id = |r: &Threat| r.threat.replace("::threat", "::risk");
    let control_id = |r: &Threat| r.threat.replace("::threat", "::control");
    let risks = indexed(rows.iter().map(|r| (risk_id(r), r)).collect());
    for (id, i, r) in &risks {
        decls.push(Decl::value(
            local(id),
            format!("{s}Risk"),
            record(
                &format!("{s}Risk"),
                &[
                    ("index", i.to_string()),
                    ("assetIndex", asset_index[&r.asset].to_string()),
                    ("threatIndex", threat_index[&r.threat].to_string()),
                    (
                        "likelihood",
                        format!("(ctor {s}Likelihood.{})", r.likelihood),
                    ),
                    ("impact", format!("(ctor {s}Impact.{})", r.impact)),
                ],
            ),
        ));
    }
    let risk_index: BTreeMap<String, usize> =
        risks.iter().map(|(id, i, _)| (id.clone(), *i)).collect();
    let controls = indexed(
        rows.iter()
            .filter(|r| r.accepted.is_none())
            .map(|r| (control_id(r), r))
            .collect(),
    );
    for (id, i, r) in &controls {
        decls.push(Decl::value(
            local(id),
            format!("{s}ApplicationSecurityControl"),
            record(
                &format!("{s}ApplicationSecurityControl"),
                &[
                    ("index", i.to_string()),
                    ("riskIndex", risk_index[&risk_id(r)].to_string()),
                ],
            ),
        ));
    }
    for (id, i, _) in &controls {
        decls.push(Decl::value(
            local(&id.replace("::control", "::activity")),
            format!("{s}SecurityActivity"),
            record(
                &format!("{s}SecurityActivity"),
                &[("index", i.to_string()), ("controlIndex", i.to_string())],
            ),
        ));
    }
    for (id, i, r) in &controls {
        let gated = r.passing && r.evidence.iter().any(|(k, _)| *k == "gate");
        decls.push(Decl::value(
            local(&id.replace("::control", "::measurement")),
            format!("{s}VerificationMeasurement"),
            record(
                &format!("{s}VerificationMeasurement"),
                &[
                    ("index", i.to_string()),
                    ("controlIndex", i.to_string()),
                    ("passed", gated.to_string()),
                ],
            ),
        ));
    }
    // Production.ThreatModel rows.
    let version = ctx
        .ws
        .packages
        .iter()
        .map(|p| p.version.clone())
        .next()
        .unwrap_or_else(|| "0.0.0".to_string());
    decls.push(Decl::value(
        "controlSet",
        format!("{tm}ControlSet"),
        record(
            &format!("{tm}ControlSet"),
            &[
                ("title", string(&format!("{} security controls", options.name))),
                (
                    "text",
                    string(&format!(
                        "Controls uorust derived from the {} workspace: memory safety, capability sites and the locked dependency graph.",
                        options.name
                    )),
                ),
                ("version", string(&version)),
                ("lastModified", string(&options.last_modified)),
            ],
        ),
    ));
    for (id, _, r) in &assets {
        decls.push(Decl::value(
            format!("modeled{}", pascal(&[local(id)])),
            format!("{tm}ModeledAsset"),
            record(
                &format!("{tm}ModeledAsset"),
                &[
                    ("asset", string(id)),
                    ("name", string(&r.asset_name)),
                    ("text", string(&r.asset_text)),
                ],
            ),
        ));
    }
    for (id, _, r) in &threat_rows {
        decls.push(Decl::value(
            format!("modeled{}", pascal(&[local(id)])),
            format!("{tm}ModeledThreat"),
            record(
                &format!("{tm}ModeledThreat"),
                &[
                    ("threat", string(id)),
                    ("name", string(&r.threat_name)),
                    ("text", string(&r.threat_text)),
                    ("category", format!("(ctor {tm}Stride.{})", r.stride)),
                ],
            ),
        ));
    }
    for (id, _, r) in &controls {
        let mut evidence = format!("(nil {tm}Evidence)");
        for (kind, reference) in r.evidence.iter().rev() {
            let item = record(
                &format!("{tm}Evidence"),
                &[
                    ("kind", format!("(ctor {tm}EvidenceKind.{kind})")),
                    ("reference", string(&truncate(reference, 256))),
                ],
            );
            evidence = format!("(cons {item} {evidence})");
        }
        decls.push(Decl::value(
            format!("modeled{}", pascal(&[local(id)])),
            format!("{tm}ModeledControl"),
            record(
                &format!("{tm}ModeledControl"),
                &[
                    ("control", string(id)),
                    ("name", string(&r.control_name)),
                    ("text", string(&r.control_text)),
                    ("evidence", evidence),
                ],
            ),
        ));
    }
    for (id, _, r) in &threat_rows {
        if let Some(rationale) = &r.accepted {
            decls.push(Decl::value(
                format!("accepted{}", pascal(&[local(id)])),
                format!("{tm}AcceptedRisk"),
                record(
                    &format!("{tm}AcceptedRisk"),
                    &[
                        ("threat", string(id)),
                        ("rationale", string(&truncate(rationale, 256))),
                    ],
                ),
            ));
            continue;
        }
        decls.push(Decl::value(
            format!("mapping{}", pascal(&[local(id)])),
            format!("{tm}ThreatMapping"),
            record(
                &format!("{tm}ThreatMapping"),
                &[
                    ("threat", string(id)),
                    ("controls", string_list(&[control_id(r)])),
                ],
            ),
        ));
    }
    Module {
        name: SEC.into(),
        glossaries: vec![BOOL, NAT, PARCH, PSEC],
        imports: vec!["Foundation.Sec".into(), "Production.ThreatModel".into()],
        title: "security threat",
        decls,
    }
}

fn truncate(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        collapsed
    } else {
        // The ellipsis takes three of the limit; a limit below three keeps
        // no character of the text, as `saturating_sub` says.
        collapsed
            .chars()
            .take(limit.saturating_sub(3))
            .collect::<String>()
            + "..."
    }
}

fn quality(ctx: &Model, features: usize) -> (Module, Vec<Refusal>) {
    let mut refusals = Vec::new();
    let ws = &ctx.ws;
    let q = "Foundation.Qual::";
    let fns = ctx.all_fns();
    let public: Vec<FnRef> = fns
        .iter()
        .copied()
        .filter(|r| ctx.product(*r) && ctx.func(*r).public)
        .collect();
    let documented = public
        .iter()
        .filter(|r| !ctx.func(**r).doc.is_empty())
        .count();
    let tests = fns.iter().filter(|r| ctx.func(**r).test).count();
    let doctests: usize = fns.iter().map(|r| ctx.func(*r).doctests).sum();
    let failures: usize = ws
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .flat_map(|f| {
            f.enums
                .iter()
                .filter(|e| e.name.ends_with("Error") || f.error_impls.contains(&e.name))
                .map(|e| e.variants.len())
                .collect::<Vec<_>>()
        })
        .sum();
    let mut lints: BTreeSet<String> = BTreeSet::new();
    for p in &ws.packages {
        for (lint, level) in &p.lints {
            if level == "deny" || level == "forbid" {
                lints.insert(lint.clone());
            }
        }
    }
    let unsafe_sites: usize = ws
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .map(|f| {
            f.capabilities
                .iter()
                .filter(|c| matches!(c.0, Capability::Unsafe | Capability::Ffi))
                .count()
        })
        .sum();
    let benches = ws
        .packages
        .iter()
        .flat_map(|p| p.targets.iter())
        .filter(|t| t.kind == TargetKind::Bench)
        .count();
    let mut msrv: Option<usize> = None;
    for p in &ws.packages {
        match p
            .rust_version
            .as_deref()
            .and_then(|v| v.split('.').nth(1))
            .and_then(|m| m.parse().ok())
        {
            Some(minor) => msrv = Some(msrv.map_or(minor, |m: usize| m.max(minor))),
            None => {
                refusals.push(Refusal::NoMsrv {
                    package: p.dir.clone(),
                });
            }
        }
    }
    let crates = ws.packages.len();
    // (characteristic, subcharacteristic, measure, threshold)
    let mut measures: Vec<(&str, &str, &str, usize)> = vec![
        (
            "FunctionalSuitability",
            "FunctionalCompleteness",
            "AdmittedFeatures",
            features,
        ),
        (
            "Maintainability",
            "Analysability",
            "DocumentedPublicItems",
            documented,
        ),
        ("Maintainability", "Modularity", "WorkspacePackages", crates),
        ("Maintainability", "Testability", "Tests", tests + doctests),
        (
            "Reliability",
            "Faultlessness",
            "TypedFailureVariants",
            failures,
        ),
        ("Reliability", "Faultlessness", "DeniedLints", lints.len()),
        (
            "Security",
            "Integrity",
            "UnsafeAndForeignSites",
            unsafe_sites,
        ),
    ];
    // UR-R23: each denied or forbidden lint is a requirement, measured by the
    // count of lints the workspace denies.
    let lint_names: Vec<String> = lints.iter().map(|l| pascal(&[l])).collect();
    if let Some(minor) = msrv {
        measures.push(("Flexibility", "Adaptability", "MinimumRustMinor", minor));
    }
    if benches > 0 {
        measures.push((
            "PerformanceEfficiency",
            "TimeBehaviour",
            "Benchmarks",
            benches,
        ));
    }
    let chars = indexed(
        measures
            .iter()
            .map(|m| (qualified(QUAL, &format!("characteristic{}", m.0)), ()))
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect(),
    );
    let char_index: BTreeMap<String, usize> =
        chars.iter().map(|(id, i, _)| (id.clone(), *i)).collect();
    let subs = indexed(
        measures
            .iter()
            .map(|m| {
                (
                    qualified(QUAL, &format!("subcharacteristic{}", m.1)),
                    qualified(QUAL, &format!("characteristic{}", m.0)),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect(),
    );
    let sub_index: BTreeMap<String, usize> =
        subs.iter().map(|(id, i, _)| (id.clone(), *i)).collect();
    let reqs = indexed(
        measures
            .iter()
            .map(|m| {
                (
                    qualified(QUAL, &format!("requirement{}", m.2)),
                    qualified(QUAL, &format!("subcharacteristic{}", m.1)),
                )
            })
            .chain(lint_names.iter().map(|l| {
                (
                    qualified(QUAL, &format!("requirementLint{l}")),
                    qualified(QUAL, "subcharacteristicFaultlessness"),
                )
            }))
            .collect(),
    );
    let req_index: BTreeMap<String, usize> =
        reqs.iter().map(|(id, i, _)| (id.clone(), *i)).collect();
    let meas = indexed(
        measures
            .iter()
            .map(|m| {
                (
                    qualified(QUAL, &format!("measure{}", m.2)),
                    (qualified(QUAL, &format!("requirement{}", m.2)), m.3),
                )
            })
            .collect(),
    );
    let mut decls = Vec::new();
    for (id, i, _) in &chars {
        decls.push(Decl::value(
            local(id),
            format!("{q}ProductQualityCharacteristic"),
            record(
                &format!("{q}ProductQualityCharacteristic"),
                &[("index", i.to_string())],
            ),
        ));
    }
    for (id, i, c) in &subs {
        decls.push(Decl::value(
            local(id),
            format!("{q}QualitySubcharacteristic"),
            record(
                &format!("{q}QualitySubcharacteristic"),
                &[
                    ("index", i.to_string()),
                    ("characteristicIndex", char_index[c].to_string()),
                ],
            ),
        ));
    }
    for (id, i, s) in &reqs {
        decls.push(Decl::value(
            local(id),
            format!("{q}QualityRequirement"),
            record(
                &format!("{q}QualityRequirement"),
                &[
                    ("index", i.to_string()),
                    ("subcharacteristicIndex", sub_index[s].to_string()),
                ],
            ),
        ));
    }
    for (id, i, (r, threshold)) in &meas {
        decls.push(Decl::value(
            local(id),
            format!("{q}QualityMeasure"),
            record(
                &format!("{q}QualityMeasure"),
                &[
                    ("index", i.to_string()),
                    ("requirementIndex", req_index[r].to_string()),
                    ("threshold", threshold.to_string()),
                ],
            ),
        ));
    }
    let module = Module {
        name: QUAL.into(),
        glossaries: vec![BOOL, NAT, PQUAL],
        imports: vec!["Foundation.Qual".into()],
        title: "quality measure",
        decls,
    };
    (module, refusals)
}

fn behavior(
    ctx: &Model,
    syntax: &Syntax,
    notes: &mut Notes,
    mir: Option<(&crate::mir::Dumps, &crate::mir::MirOptions)>,
) -> (Module, Option<String>) {
    let ws = &ctx.ws;
    let mut decls = Vec::new();
    // Each crate's type aliases, which its types and signatures are read
    // through. An alias spelled like a type the crate declares names one
    // thing in its module and another elsewhere: it is not read.
    let type_aliases: Vec<BTreeMap<String, crate::ast::Alias>> = ws
        .packages
        .iter()
        .map(|package| {
            let declared: BTreeSet<&str> = package
                .files
                .iter()
                .flat_map(|file| {
                    file.enums
                        .iter()
                        .map(|e| e.name.as_str())
                        .chain(file.structs.iter().map(|s| s.name.as_str()))
                })
                .collect();
            package
                .files
                .iter()
                .flat_map(|file| {
                    syntax.files[&file.rel]
                        .aliases
                        .iter()
                        .filter(|(n, _)| !declared.contains(n.as_str()))
                        .map(|(n, a)| {
                            let to = a.to.localized(&file.imports);
                            let alias = crate::ast::Alias {
                                params: a.params.clone(),
                                to,
                            };
                            (n.clone(), alias)
                        })
                })
                .collect()
        })
        .collect();
    // A chain of aliases that does not repeat one is at most as long as
    // the crate has aliases.
    let expanded = |pkg: usize, ty: &crate::ast::RustType| {
        ty.expanded(&type_aliases[pkg], type_aliases[pkg].len())
    };
    // A type of product code is keyed by its full path in its crate: its
    // target, its module and its name. A key declared twice (once per
    // configuration) is refused; one name in two modules or two crates is
    // two types, each lifting under the shortest name that tells it apart.
    type Key = (usize, String, String);
    /// The lifted kinds of one crate's types, by key, each struct and data
    /// enum with the file that declares it, which its fields are read in.
    #[derive(Default)]
    struct Declared {
        enums: BTreeMap<Key, Vec<String>>,
        structs: BTreeMap<Key, (usize, Vec<(String, crate::ast::RustType)>)>,
        datas: BTreeMap<Key, (usize, lift::RawVariants)>,
        spans: BTreeMap<Key, Span>,
    }
    let mut declared: Vec<Declared> = Vec::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        let mut d = Declared::default();
        let mut clashing: BTreeSet<Key> = BTreeSet::new();
        let product = |file: &crate::facts::SourceFile| {
            matches!(
                package.targets[file.target].kind,
                TargetKind::Lib | TargetKind::Bin
            )
        };
        for (fi, file) in package.files.iter().enumerate() {
            if !product(file) {
                continue;
            }
            for (e, payload) in file.enums.iter().zip(&syntax.files[&file.rel].enums) {
                // An enum whose variants carry data is an inductive with
                // fields, where its syntax reads.
                let data = !e.variants.iter().all(|v| v.unit);
                if e.variants.is_empty() || (data && payload.is_empty()) {
                    continue;
                }
                let key = (file.target, file.module.clone(), e.name.clone());
                if d.spans.insert(key.clone(), e.span.clone()).is_some() {
                    clashing.insert(key);
                    notes.refusals.insert(Refusal::Collision {
                        span: e.span.clone(),
                        name: qualified(BEHAVIOR, &e.name),
                    });
                    continue;
                }
                if data {
                    let variants = payload
                        .iter()
                        .map(|(v, fs)| {
                            let fs = fs
                                .iter()
                                .map(|(n, t)| {
                                    (n.clone(), expanded(pkg, &t.localized(&file.imports)))
                                })
                                .collect();
                            (v.clone(), fs)
                        })
                        .collect();
                    d.datas.insert(key, (fi, variants));
                } else {
                    d.enums
                        .insert(key, e.variants.iter().map(|v| v.name.clone()).collect());
                }
            }
        }
        for (fi, file) in package.files.iter().enumerate() {
            if !product(file) {
                continue;
            }
            for (st, fields) in file.structs.iter().zip(&syntax.files[&file.rel].structs) {
                let key = (file.target, file.module.clone(), st.name.clone());
                if d.spans.insert(key.clone(), st.span.clone()).is_some() {
                    clashing.insert(key);
                    notes.refusals.insert(Refusal::Collision {
                        span: st.span.clone(),
                        name: qualified(BEHAVIOR, &st.name),
                    });
                    continue;
                }
                let fields = fields
                    .iter()
                    .map(|(n, t)| (n.clone(), expanded(pkg, &t.localized(&file.imports))))
                    .collect();
                d.structs.insert(key, (fi, fields));
            }
        }
        for key in &clashing {
            d.enums.remove(key);
            d.structs.remove(key);
            d.datas.remove(key);
        }
        declared.push(d);
    }
    let keys = |d: &Declared| -> Vec<Key> {
        d.enums
            .keys()
            .chain(d.structs.keys())
            .chain(d.datas.keys())
            .cloned()
            .collect()
    };
    // A name one type of the workspace has lifts as itself; one a crate's
    // type shares with another crate's, qualified by the crate; one two of
    // a crate's types share, by the crate, the target where it is not the
    // library, and the module.
    let mut everywhere: BTreeMap<String, usize> = BTreeMap::new();
    for d in &declared {
        for (_, _, name) in keys(d) {
            *everywhere.entry(name).or_default() += 1;
        }
    }
    let mut lexes: Vec<BTreeMap<Key, String>> = Vec::new();
    // Each crate's names that two of its types share: a file reads one by
    // its own module's declaration or its import of it.
    let mut ambiguous: Vec<BTreeSet<String>> = Vec::new();
    for (d, package) in declared.iter().zip(&ws.packages) {
        let ks = keys(d);
        let mut in_crate: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, _, name) in &ks {
            *in_crate.entry(name.as_str()).or_default() += 1;
        }
        let named = ks
            .iter()
            .map(|key| {
                let (target, module, name) = key;
                let lex = if everywhere[name] == 1 {
                    name.clone()
                } else if in_crate[name.as_str()] == 1 {
                    format!("{}{name}", pascal(&[&package.name]))
                } else {
                    let mut parts: Vec<&str> = vec![&package.name];
                    let t = &package.targets[*target];
                    if !matches!(t.kind, TargetKind::Lib) {
                        parts.push(&t.name);
                    }
                    parts.extend(module.split("::").filter(|s| !s.is_empty()));
                    format!("{}{name}", pascal(&parts))
                };
                (key.clone(), lex)
            })
            .collect();
        lexes.push(named);
        ambiguous.push(
            in_crate
                .into_iter()
                .filter(|(_, n)| *n > 1)
                .map(|(name, _)| name.to_string())
                .collect(),
        );
    }
    // Two keys whose qualified names meet name neither.
    let mut lex_count: BTreeMap<String, usize> = BTreeMap::new();
    for lex in lexes.iter().flat_map(BTreeMap::values) {
        *lex_count.entry(lex.clone()).or_default() += 1;
    }
    for (d, named) in declared.iter_mut().zip(&mut lexes) {
        let met: Vec<Key> = named
            .iter()
            .filter(|(_, lex)| lex_count[*lex] > 1)
            .map(|(key, _)| key.clone())
            .collect();
        for key in met {
            if let Some(span) = d.spans.get(&key) {
                notes.refusals.insert(Refusal::Collision {
                    span: span.clone(),
                    name: qualified(BEHAVIOR, &named[&key]),
                });
            }
            named.remove(&key);
            d.enums.remove(&key);
            d.structs.remove(&key);
            d.datas.remove(&key);
        }
    }
    // Each file's reading of its crate's shared names: the type its module
    // declares or its `use` names, and otherwise none.
    let mut overlays: BTreeMap<(usize, usize), BTreeMap<String, String>> = BTreeMap::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            let mut names: BTreeMap<String, String> = ambiguous[pkg]
                .iter()
                .map(|n| (n.clone(), n.clone()))
                .collect();
            let module: Vec<&str> = file.module.split("::").filter(|s| !s.is_empty()).collect();
            for (local, path) in &file.imports {
                if !names.contains_key(local) {
                    continue;
                }
                // `crate::`, `self::` and `super::` as Rust reads them, any
                // other path from the file's module.
                let mut at = module.clone();
                let mut segs: Vec<&str> = path.split("::").collect();
                match segs.first() {
                    Some(&"crate") => {
                        at.clear();
                        segs.remove(0);
                    }
                    Some(&"self") => {
                        segs.remove(0);
                    }
                    _ => {}
                }
                while segs.first() == Some(&"super") {
                    at.pop();
                    segs.remove(0);
                }
                let Some((name, prefix)) = segs.split_last() else {
                    continue;
                };
                at.extend(prefix);
                let key = (file.target, at.join("::"), name.to_string());
                if let Some(lex) = lexes[pkg].get(&key) {
                    names.insert(local.clone(), lex.clone());
                }
            }
            for ((target, m, name), lex) in &lexes[pkg] {
                if *target == file.target && *m == file.module && names.contains_key(name) {
                    names.insert(name.clone(), lex.clone());
                }
            }
            overlays.insert((pkg, fi), names);
        }
    }
    let scopes: Vec<Types> = declared
        .into_iter()
        .enumerate()
        .map(|(pkg, d)| {
            let package = &ws.packages[pkg];
            let named = &lexes[pkg];
            // A name no other of the crate's types shares reads as its type
            // in every file.
            let mut aliases: BTreeMap<String, String> = named
                .iter()
                .filter(|((_, _, name), lex)| *lex != name && !ambiguous[pkg].contains(name))
                .map(|((_, _, name), lex)| (name.clone(), lex.clone()))
                .collect();
            let mut enums: Variants = d
                .enums
                .into_iter()
                .map(|(key, vs)| (named[&key].clone(), vs))
                .collect();
            // `std::cmp::Ordering`, which derived orders result in, unless
            // a crate declares a type of its name.
            let rust = crate::order::RUST_ORDERING;
            if !everywhere.contains_key(rust) && !everywhere.contains_key(crate::order::ORDERING) {
                enums.insert(
                    crate::order::ORDERING.to_string(),
                    crate::order::ORDERINGS
                        .iter()
                        .map(|v| v.to_string())
                        .collect(),
                );
                aliases.insert(rust.to_string(), crate::order::ORDERING.to_string());
            }
            // A field's type is read in the file that declares it.
            let structs: Fields = d
                .structs
                .into_iter()
                .map(|(key, (fi, fs))| {
                    let names = &overlays[&(pkg, fi)];
                    let fs = fs.into_iter().map(|(n, t)| (n, t.renamed(names))).collect();
                    (named[&key].clone(), fs)
                })
                .collect();
            let datas: Datas = d
                .datas
                .into_iter()
                .map(|(key, (fi, vs))| {
                    let names = &overlays[&(pkg, fi)];
                    let vs = vs
                        .into_iter()
                        .map(|(v, fs)| {
                            (
                                v,
                                fs.into_iter().map(|(n, t)| (n, t.renamed(names))).collect(),
                            )
                        })
                        .collect();
                    (named[&key].clone(), vs)
                })
                .collect();
            // The traits each lifted type derives, which say where its
            // order and equality are the structural ones.
            let derives: BTreeMap<String, BTreeSet<String>> = package
                .files
                .iter()
                .flat_map(|file| {
                    let enums = file.enums.iter().map(|e| (&e.name, &e.derives));
                    let structs = file.structs.iter().map(|st| (&st.name, &st.derives));
                    enums
                        .chain(structs)
                        .filter_map(|(n, ds)| {
                            let lex = named.get(&(file.target, file.module.clone(), n.clone()))?;
                            Some((lex.clone(), ds.iter().cloned().collect()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            Types {
                derives,
                ..Types::with_data(enums, &datas, &structs, aliases)
            }
        })
        .collect();
    // Each file's scope: its crate's, with the crate's shared names read as
    // the file reads them.
    let file_scopes: BTreeMap<(usize, usize), Types> = overlays
        .iter()
        .map(|(&(pkg, fi), names)| {
            let mut scope = scopes[pkg].clone();
            scope.aliases.extend(names.clone());
            ((pkg, fi), scope)
        })
        .collect();
    // Each target's scope for the MIR path, which prints a type of the
    // crate by its full path: `project::Model`.
    let mut mir_scopes: BTreeMap<(usize, usize), Types> = BTreeMap::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for target in 0..package.targets.len() {
            let mut scope = scopes[pkg].clone();
            for ((t, module, name), lex) in &lexes[pkg] {
                if *t == target {
                    let printed = if module.is_empty() {
                        name.clone()
                    } else {
                        format!("{module}::{name}")
                    };
                    scope.aliases.insert(printed, lex.clone());
                }
            }
            mir_scopes.insert((pkg, target), scope);
        }
    }
    // Every crate's lifted types, for the declarations they emit.
    let mut types = Types::default();
    for scope in &scopes {
        types.enums.extend(scope.enums.clone());
        types.structs.extend(scope.structs.clone());
        types.datas.extend(scope.datas.clone());
        types.derives.extend(scope.derives.clone());
        for family in &scope.families {
            if !types.families.contains(family) {
                types.families.push(family.clone());
            }
        }
    }
    // Typed failures: every error enum's variants, as an inductive.
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            if !matches!(
                package.targets[file.target].kind,
                TargetKind::Lib | TargetKind::Bin
            ) {
                continue;
            }
            for (ei, e) in file.enums.iter().enumerate() {
                if ctx.enum_routes.get(&(pkg, fi, ei)) != Some(&"UR-R12") {
                    continue;
                }
                let name = format!("{}Failure", pascal(&[&e.name]));
                if decls.iter().any(|d: &Decl| d.name() == name) {
                    notes.refusals.insert(Refusal::Collision {
                        span: e.span.clone(),
                        name: qualified(BEHAVIOR, &name),
                    });
                    continue;
                }
                decls.push(Decl::Inductive {
                    name,
                    constructors: e.variants.iter().map(|v| v.name.clone()).collect(),
                });
            }
        }
    }
    let all = ctx.all_fns();
    // The I/O shell: a function that performs an effect, or calls one that does.
    let mut effectful: BTreeMap<FnRef, String> = BTreeMap::new();
    for &r in &all {
        if let Some((cap, op)) = &ctx.func(r).effect {
            effectful.insert(r, format!("{} effect: {op}", cap.key()));
        }
    }
    loop {
        let mut grew = false;
        for &r in &all {
            if effectful.contains_key(&r) {
                continue;
            }
            let via = ctx
                .calls
                .get(&r)
                .and_then(|callees| callees.iter().find(|c| effectful.contains_key(c)));
            if let Some(&callee) = via {
                let reason = format!(
                    "calls {}, which {}",
                    ctx.path(callee),
                    effect_phrase(&effectful[&callee])
                );
                effectful.insert(r, reason);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    // Each crate lifts on its own: its types, and calls among its functions.
    // A function name two crates share lifts qualified by the crate's name.
    // A free function's name two modules of a crate share lifts qualified
    // by its module.
    let homonyms = homonym_fns(ctx);
    let mut scoped: Vec<(Vec<Candidate<'_>>, BTreeMap<String, FnRef>)> = ws
        .packages
        .iter()
        .map(|_| (Vec::new(), BTreeMap::new()))
        .collect();
    for &r in &all {
        let f = ctx.func(r);
        let types = &file_scopes[&(r.pkg, r.file)];
        if !ctx.product(r) {
            continue;
        }
        // Inherent methods of a type the lift names lift with `self` as their
        // first parameter; trait methods and methods of other types stop, each
        // with a row of its own.
        let known = |o: &String| {
            types.enums.contains_key(&types.resolve(o))
                || types.structs.contains_key(&types.resolve(o))
                || types.datas.contains_key(&types.resolve(o))
        };
        // `Display::fmt` lifts as the function from the value to the text
        // it writes: `write!` into its formatter is that `format!`.
        let display = matches!(
            (&f.owner, &f.implements),
            (Some(o), Some(t)) if t == "Display" && f.name == "fmt" && !f.in_trait && known(o)
        );
        let method = match (&f.owner, &f.implements) {
            (None, _) => None,
            (Some(o), Some(_)) if display => Some(types.resolve(o)),
            (Some(_), _) if f.in_trait => {
                let reason = Residue::Signature("a method a trait declares".to_string());
                ctx.stopped(notes, &effectful, r, reason);
                continue;
            }
            // A trait method over a registry crate's types, such as a `syn`
            // visitor, is the converter shell's: a foreign row.
            (Some(_), Some(t)) => {
                let sig = &syntax.function(&ctx.file(r).rel, r.idx).signature;
                let foreign = sig
                    .inputs
                    .iter()
                    .filter_map(|i| match i {
                        crate::ast::FnArg::Typed(p) => Some(p.ty.text.clone()),
                        crate::ast::FnArg::Receiver { .. } => None,
                    })
                    .chain(sig.output.iter().map(|o| o.text.clone()))
                    .find(|ty| foreign_crate(&ctx.ws.packages[r.pkg], ty).is_some());
                let reason = match foreign {
                    Some(ty) => Residue::Type(ty),
                    None => Residue::Signature(format!("implements the trait {t}")),
                };
                ctx.stopped(notes, &effectful, r, reason);
                continue;
            }
            (Some(o), None)
                if types.enums.contains_key(&types.resolve(o))
                    || types.structs.contains_key(&types.resolve(o))
                    || types.datas.contains_key(&types.resolve(o)) =>
            {
                Some(types.resolve(o))
            }
            (Some(o), None) => {
                ctx.stopped(notes, &effectful, r, Residue::Type(o.clone()));
                continue;
            }
        };
        let fn_syntax = syntax.function(&ctx.file(r).rel, r.idx);
        let mut sig = fn_syntax.signature.clone();
        for input in &mut sig.inputs {
            if let crate::ast::FnArg::Typed(t) = input {
                t.ty = expanded(r.pkg, &t.ty.localized(&ctx.file(r).imports));
            }
        }
        if let Some(t) = &mut sig.output {
            *t = expanded(r.pkg, &t.localized(&ctx.file(r).imports));
        }
        let formatter = match sig.inputs.as_slice() {
            [crate::ast::FnArg::Receiver { plain: true }, crate::ast::FnArg::Typed(t)]
                if display =>
            {
                match &*t.pat {
                    crate::ast::Pat::Ident(i) => Some(i.ident.clone()),
                    _ => None,
                }
            }
            _ => None,
        };
        let sig = &sig;
        let Some(block) = &fn_syntax.block else {
            continue;
        };
        let site = (
            r.pkg,
            ctx.file(r).target,
            ctx.file(r).module.clone(),
            f.name.clone(),
        );
        let (rust, name) = match (&method, homonyms.get(&site)) {
            (Some(o), _) => (
                format!("{o}::{}", f.name),
                lift::local_name(&format!("{o}_{}", f.name)),
            ),
            (None, Some((rust, lex))) => (rust.clone(), lex.clone()),
            (None, None) => (f.name.clone(), lift::local_name(&f.name)),
        };
        let called = match &method {
            Some(_) => rust.clone(),
            None => f.name.clone(),
        };
        let (candidates, names) = &mut scoped[r.pkg];
        let signature = match (&formatter, &method) {
            (Some(_), Some(o)) => Ok((
                vec![(
                    "self".to_string(),
                    lift::local_name("self"),
                    lift::Ty::Named(o.clone()),
                )],
                lift::Ty::Str,
            )),
            (None, _) if display => Err(Residue::Signature(
                "a Display impl whose formatter is not a plain parameter".to_string(),
            )),
            _ => types.signature(sig, method.as_deref()),
        };
        match signature {
            Ok((params, result)) => {
                if names.contains_key(&rust)
                    || candidates.iter().any(|c: &Candidate<'_>| c.name == name)
                {
                    notes.refusals.insert(Refusal::Collision {
                        span: f.span.clone(),
                        name: qualified(BEHAVIOR, &name),
                    });
                    // The register keeps a row for it all the same.
                    ctx.stopped(notes, &effectful, r, Residue::Collision(name));
                    continue;
                }
                names.insert(rust.clone(), r);
                candidates.push(Candidate {
                    rust,
                    name,
                    params,
                    result,
                    block,
                    imports: ctx.file(r).imports.clone(),
                    aliases: overlays[&(r.pkg, r.file)].clone(),
                    homonyms: homonym_reading(ctx, &homonyms, r.pkg, r.file),
                    called,
                    formatter,
                });
            }
            Err(reason) => ctx.stopped(notes, &effectful, r, reason),
        }
    }
    // Constants lift as definitions of no parameters.
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            let types = &file_scopes[&(pkg, fi)];
            if !matches!(
                package.targets[file.target].kind,
                TargetKind::Lib | TargetKind::Bin
            ) {
                continue;
            }
            for (rust, ty, block) in &syntax.files[&file.rel].consts {
                let Ok(result) = types.ty(ty) else {
                    continue;
                };
                let (candidates, names) = &mut scoped[pkg];
                // A constant spelled like a function, `REVERSE` beside
                // `reverse`, takes its kind as a suffix.
                let plain = lift::local_name(rust);
                let name = if candidates.iter().any(|c: &Candidate<'_>| c.name == plain) {
                    lift::local_name(&format!("{rust}_CONST"))
                } else {
                    plain
                };
                if names.contains_key(rust)
                    || candidates
                        .iter()
                        .any(|c: &Candidate<'_>| c.rust == *rust || c.name == name)
                {
                    continue;
                }
                candidates.push(Candidate {
                    rust: rust.clone(),
                    name,
                    params: Vec::new(),
                    result,
                    block,
                    imports: file.imports.clone(),
                    aliases: overlays[&(pkg, fi)].clone(),
                    homonyms: homonym_reading(ctx, &homonyms, pkg, fi),
                    called: rust.clone(),
                    formatter: None,
                });
            }
        }
    }
    let mut shared: BTreeMap<String, usize> = BTreeMap::new();
    for (candidates, _) in &scoped {
        for c in candidates {
            *shared.entry(c.name.clone()).or_default() += 1;
        }
    }
    for ((candidates, _), package) in scoped.iter_mut().zip(&ws.packages) {
        for c in candidates.iter_mut() {
            if shared[&c.name] > 1 {
                c.name = lift::local_name(&format!("{}_{}", package.name, c.name));
            }
        }
    }
    // A parameter may not be spelled like a definition: generated Lean would
    // capture the definition.
    let defined: BTreeSet<String> = scoped
        .iter()
        .flat_map(|(candidates, _)| candidates.iter().map(|c| c.name.clone()))
        .collect();
    for (candidates, _) in &mut scoped {
        for c in candidates.iter_mut() {
            let mut taken: BTreeSet<String> = c.params.iter().map(|(_, n, _)| n.clone()).collect();
            for (_, n, _) in &mut c.params {
                if defined.contains(n.as_str()) {
                    let mut k = 1;
                    let renamed = loop {
                        let candidate = format!(
                            "{n}Arg{}",
                            if k == 1 { String::new() } else { k.to_string() }
                        );
                        if !defined.contains(&candidate) && !taken.contains(&candidate) {
                            break candidate;
                        }
                        k += 1;
                    };
                    taken.insert(renamed.clone());
                    *n = renamed;
                }
            }
        }
    }
    let mut lifted = Vec::new();
    let mut mentioned: BTreeSet<Ty> = BTreeSet::new();
    let mut ordered: BTreeSet<Ty> = BTreeSet::new();
    // Per crate: the lifted functions' signatures and their axioms.
    let mut results: Vec<(Sigs, Axioms)> = Vec::new();
    // Every lifted function by its crate path, for the MIR path's calls.
    let mut callees: BTreeMap<String, crate::mir::lower::Callee> = BTreeMap::new();
    let every_name: BTreeSet<String> = scoped
        .iter()
        .flat_map(|(candidates, _)| candidates.iter().map(|c| c.name.clone()))
        .collect();
    for ((candidates, names), types) in scoped.iter().zip(&scopes) {
        let widths = lift::pointer_widths(&ctx.ws.targets);
        let out = lift::lift_all(candidates, types, &widths, &every_name);
        for (rust, reason) in out.failed {
            if let Some(&r) = names.get(&rust) {
                ctx.stopped(notes, &effectful, r, reason);
            }
        }
        mentioned.extend(out.types);
        ordered.extend(out.ordered);
        for c in candidates {
            // A constant has no function row of its own.
            if let (true, Some(&r)) = (
                out.decls.iter().any(|d| d.name() == c.name),
                names.get(&c.rust),
            ) {
                notes.lifted.insert(
                    ctx.path(r),
                    (
                        ctx.func(r).span.clone(),
                        qualified(BEHAVIOR, &c.name),
                        out.divergences.get(&c.rust).cloned().unwrap_or_default(),
                    ),
                );
            }
        }
        // A closed instance is a theorem only where the kernel can compute it.
        let sigs: BTreeMap<String, lift::Sig> = candidates
            .iter()
            .filter(|c| out.decls.iter().any(|d| d.name() == c.name))
            .filter(|c| !out.opaque.contains(&c.rust))
            .map(|c| {
                (
                    c.rust.clone(),
                    lift::Sig {
                        name: c.name.clone(),
                        params: c.params.iter().map(|(_, _, t)| t.clone()).collect(),
                        result: c.result.clone(),
                    },
                )
            })
            .collect();
        for c in candidates {
            if let (true, Some(&r)) = (
                out.decls.iter().any(|d| d.name() == c.name),
                names.get(&c.rust),
            ) {
                callees.insert(
                    ctx.path(r),
                    crate::mir::lower::Callee {
                        name: c.name.clone(),
                        params: c.params.iter().map(|(_, _, t)| t.clone()).collect(),
                        result: c.result.clone(),
                        threads: None,
                    },
                );
            }
        }
        lifted.extend(out.decls);
        results.push((sigs, out.axioms));
    }
    // The MIR path, for every function the surface lift left as residue.
    let mut listing = None;
    let mut translated = Vec::new();
    if let Some((dumps, options)) = mir {
        let mut taken = every_name.clone();
        taken.extend(lifted.iter().map(|d| d.name().to_string()));
        taken.extend(types.enums.keys().cloned());
        taken.extend(types.structs.keys().cloned());
        taken.extend(types.datas.keys().cloned());
        let out = mir_lift(ctx, notes, dumps, options, &mir_scopes, &callees, &taken);
        mentioned.extend(out.types);
        translated = out.decls;
        listing = out.listing;
    }
    // The enums, structs and tuples the lifted definitions name, each after
    // the types its fields name.
    let mut rendered: Vec<String> = lifted.iter().map(crate::lex::render_decl).collect();
    rendered.extend(translated.iter().map(crate::lex::render_decl));
    let compared: BTreeSet<String> = types
        .enums
        .keys()
        .filter(|n| {
            let call = format!("({} ", lift::equality_name(n));
            rendered.iter().any(|d| d.contains(&call))
        })
        .cloned()
        .collect();
    let mut emitted: BTreeSet<Ty> = BTreeSet::new();
    for ty in &mentioned {
        type_decls(ty, &types, &compared, &mut emitted, &mut decls);
    }
    // The derived orders the definitions compare by, after the types.
    if !ordered.is_empty() {
        decls.extend(crate::order::tests());
        decls.push(crate::order::reverse());
        decls.extend(crate::order::decls(&ordered, &types));
    }
    decls.extend(lifted);
    decls.extend(translated);
    // Closed assertions of tests over lifted functions are theorems.
    let mut theorems = BTreeSet::new();
    for &r in &all {
        let f = ctx.func(r);
        if !f.test {
            continue;
        }
        let (sigs, axioms) = &results[r.pkg];
        for (i, (kind, args)) in syntax
            .function(&ctx.file(r).rel, r.idx)
            .asserts
            .iter()
            .enumerate()
        {
            if let Some((statement, callee)) =
                lift::acceptance(kind, args, sigs, &file_scopes[&(r.pkg, r.file)])
            {
                let name = format!("accept{}{}", pascal(&[&ctx.package(r).name, &f.name]), i);
                if theorems.insert(name.clone()) {
                    decls.push(Decl::Theorem {
                        name,
                        parameters: Vec::new(),
                        statement,
                        proof: "decide{}".to_string(),
                        axioms: axioms.get(&callee).cloned().unwrap_or_default(),
                    });
                }
            }
        }
    }
    (
        Module {
            name: BEHAVIOR.into(),
            glossaries: vec![BOOL, NAT, TITLES],
            imports: vec![lift::PRELUDE.into()],
            title: "Lifted Rust behavior",
            decls,
        },
        listing,
    )
}

/// What the MIR path adds to the behavior module.
struct MirOut {
    decls: Vec<Decl>,
    types: BTreeSet<Ty>,
    /// The trial run's listing of the translations it admitted on trial.
    listing: Option<String>,
}

/// One function's translation.
struct Translation {
    r: FnRef,
    path: String,
    kappa: String,
    name: String,
    lowered: crate::mir::lower::Lowered,
    digest: String,
}

/// The MIR path: every function the surface lift left as residue, whose
/// body the dump holds, translated from MIR. A translation is admitted when
/// the verdict register records it proved (or, on the prover's trial run,
/// when it has no verdict yet) and every translation it calls is admitted;
/// every other function's residue row names the MIR kind or the obligation
/// that stopped it.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn mir_lift(
    ctx: &Model,
    notes: &mut Notes,
    dumps: &crate::mir::Dumps,
    options: &crate::mir::MirOptions,
    scopes: &BTreeMap<(usize, usize), Types>,
    lifted: &BTreeMap<String, crate::mir::lower::Callee>,
    taken: &BTreeSet<String>,
) -> MirOut {
    use crate::mir::lower;
    use crate::mir::verdict::Decision;
    let homonyms = homonym_fns(ctx);
    // Every crate's closures, by the type MIR prints for each: a closure's
    // type is the struct of the values it captures.
    let closures: BTreeMap<String, crate::mir::IrBody> = dumps
        .closures
        .values()
        .flat_map(|c| c.iter().map(|(t, b)| (t.clone(), b.clone())))
        .collect();
    let signatures: BTreeMap<String, Vec<String>> = closures
        .iter()
        .filter(|(_, b)| b.upvars.iter().all(Option::is_some))
        .map(|(t, b)| {
            let captures = b.upvars.iter().flatten().map(|(_, t)| t.clone()).collect();
            (t.clone(), captures)
        })
        .collect();
    let scopes: BTreeMap<(usize, usize), Types> = scopes
        .iter()
        .map(|(&at, s)| {
            let scope = Types {
                closures: signatures.clone(),
                ..s.clone()
            };
            (at, scope)
        })
        .collect();
    let scope = |r: FnRef| &scopes[&(r.pkg, ctx.file(r).target)];
    let widths = lift::pointer_widths(&ctx.ws.targets);
    // Enums whose variants set their discriminants, by LexLean name.
    let mut explicit: BTreeSet<String> = BTreeSet::new();
    for (pkg, package) in ctx.ws.packages.iter().enumerate() {
        for file in &package.files {
            for e in file.enums.iter().filter(|e| e.explicit) {
                let printed = if file.module.is_empty() {
                    e.name.clone()
                } else {
                    format!("{}::{}", file.module, e.name)
                };
                explicit.insert(scopes[&(pkg, file.target)].resolve(&printed));
            }
        }
    }
    let product: BTreeMap<String, FnRef> = ctx
        .all_fns()
        .into_iter()
        .filter(|&r| ctx.product(r))
        .map(|r| (ctx.path(r), r))
        .collect();
    // The residue functions whose bodies the dump holds, with their names
    // and signatures.
    let mut todo: Vec<(FnRef, String, &crate::mir::IrBody)> = Vec::new();
    let mut callees = lifted.clone();
    let mut names: BTreeSet<String> = taken.clone();
    let residue: Vec<String> = notes.residue.keys().cloned().collect();
    for path in residue {
        let Some(&r) = product.get(&path) else {
            continue;
        };
        let krate = ctx.crate_ident(r);
        let local = path
            .strip_prefix(&format!("{krate}::"))
            .unwrap_or(&path)
            .to_string();
        let Some(body) = dumps.crates.get(&krate).and_then(|c| c.get(&local)) else {
            continue;
        };
        let f = ctx.func(r);
        let site = (
            r.pkg,
            ctx.file(r).target,
            ctx.file(r).module.clone(),
            f.name.clone(),
        );
        let base = match (&f.owner, homonyms.get(&site)) {
            (Some(o), _) => lift::local_name(&format!("{o}_{}", f.name)),
            (None, Some((_, lex))) => lex.clone(),
            (None, None) => lift::local_name(&f.name),
        };
        let name = if names.contains(&base) {
            lift::local_name(&format!("{}_{base}", ctx.package(r).name))
        } else {
            base
        };
        if !names.insert(name.clone()) {
            let span = f.span.clone();
            notes.residue.insert(path, (span, Residue::Collision(name)));
            continue;
        }
        let types = scope(r);
        // The lift's signature: its parameters (a `&mut T` as the `T` it
        // borrows), its forward result (`_0`'s type), the positions of its
        // threaded parameters (`lower::threaded`), and the definition's
        // result.
        let sig: Result<(Vec<Ty>, Ty, Vec<usize>, Ty), Residue> = (|| {
            let mut params = Vec::new();
            for l in 1..=body.arity {
                let text = &body.locals[l];
                params.push(lower::ty(lower::mut_target(text).unwrap_or(text), types)?);
            }
            let ret = lower::ty(&body.locals[0], types)?;
            let closure = crate::mir::closure_type(body).is_some();
            let positions: Vec<usize> = lower::threaded(&body.locals, body.arity, closure, types)
                .into_iter()
                .map(|l| l - 1)
                .collect();
            let rest: Vec<Ty> = positions.iter().map(|&i| params[i].clone()).collect();
            let full = lower::threaded_result(&ret, &rest, types);
            Ok((params, ret, positions, full))
        })();
        match sig {
            Ok((params, result, positions, full)) => {
                callees.insert(
                    path.clone(),
                    lower::Callee {
                        name: name.clone(),
                        params,
                        result,
                        threads: (!positions.is_empty()).then_some((full, positions)),
                    },
                );
                todo.push((r, name, body));
            }
            Err(reason) => {
                let span = f.span.clone();
                notes.residue.insert(path, (span, mir_residue(reason)));
            }
        }
    }
    // The functions the MIR path translates, by LexLean name.
    let translations: BTreeMap<String, String> = todo
        .iter()
        .map(|(r, name, _)| (name.clone(), ctx.path(*r)))
        .collect();
    // Translate each.
    let no_consts = BTreeMap::new();
    let mut done: BTreeMap<String, Translation> = BTreeMap::new();
    for (r, name, body) in todo {
        let path = ctx.path(r);
        let cx = lower::Cx {
            types: scope(r),
            widths: &widths,
            callees: &callees,
            krate: ctx.crate_ident(r),
            taken: &names,
            explicit: &explicit,
            limit: MAX_FILE_BYTES,
            consts: dumps.consts.get(&ctx.crate_ident(r)).unwrap_or(&no_consts),
            closures: &closures,
            active: &[],
            core: None,
            layouts: &[],
        };
        match lower::lower(&cx, body, &name) {
            Ok(lowered) => {
                let text: String = lowered
                    .aux
                    .values()
                    .chain(&lowered.decls)
                    .map(crate::lex::render_decl)
                    .collect();
                let digest = sha(text.as_bytes());
                let kappa = crate::register::kappa(&path, &ctx.func(r).tokens);
                done.insert(
                    name.clone(),
                    Translation {
                        r,
                        path,
                        kappa,
                        name,
                        lowered,
                        digest,
                    },
                );
            }
            Err(reason) => {
                let span = ctx.func(r).span.clone();
                notes.residue.insert(path, (span, mir_residue(reason)));
            }
        }
    }
    // Admission: a verdict of the digest, or the trial; then every
    // translation a translation calls must be admitted too.
    let mut admitted: BTreeMap<String, bool> = BTreeMap::new();
    let mut causes: BTreeMap<String, String> = BTreeMap::new();
    let mut on_trial: BTreeSet<String> = BTreeSet::new();
    // A translation that calls one the path could not translate, directly
    // or through others, is never admitted, whatever its verdict: that
    // callee is its cause, and it is not put on trial.
    let mut blocked: BTreeMap<String, String> = BTreeMap::new();
    loop {
        let mut changed = false;
        for t in done.values() {
            if blocked.contains_key(&t.name) {
                continue;
            }
            let stopped = t.lowered.calls.iter().find(|c| {
                translations.contains_key(*c)
                    && (!done.contains_key(*c) || blocked.contains_key(*c))
            });
            if let Some(c) = stopped {
                blocked.insert(t.name.clone(), translations[c].clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for t in done.values() {
        if let Some(callee) = blocked.get(&t.name) {
            causes.insert(
                t.name.clone(),
                format!("TerminatorKind::Call: {callee}, whose translation is not admitted"),
            );
            admitted.insert(t.name.clone(), false);
            continue;
        }
        let verdict = options
            .verdicts
            .rows
            .get(&t.kappa)
            .filter(|v| v.digest == t.digest);
        let ok = match verdict.map(|v| &v.status) {
            Some(Decision::Proved) => true,
            Some(Decision::Refused(cause)) => {
                causes.insert(t.name.clone(), format!("Verdict: {cause}"));
                false
            }
            None if options.trial.admits(&t.kappa) => {
                on_trial.insert(t.name.clone());
                true
            }
            None => {
                causes.insert(
                    t.name.clone(),
                    "Verdict: a translation the verifier has not decided yet".to_string(),
                );
                false
            }
        };
        admitted.insert(t.name.clone(), ok);
    }
    loop {
        let mut changed = false;
        for t in done.values() {
            if !admitted[&t.name] {
                continue;
            }
            let stopped = t
                .lowered
                .calls
                .iter()
                .find(|c| translations.contains_key(*c) && admitted.get(*c) != Some(&true));
            if let Some(c) = stopped {
                let callee = translations[c].clone();
                causes.insert(
                    t.name.clone(),
                    format!("TerminatorKind::Call: {callee}, whose translation is not admitted"),
                );
                admitted.insert(t.name.clone(), false);
                on_trial.remove(&t.name);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // The admitted translations, each after the translations it calls, with
    // the axioms the verifier observed.
    let mut order: Vec<String> = Vec::new();
    let mut placed: BTreeSet<String> = BTreeSet::new();
    let names_in: Vec<String> = done.keys().cloned().collect();
    for n in &names_in {
        place_after_callees(n, &done, &admitted, &mut placed, &mut order);
    }
    let mut out = MirOut {
        decls: Vec::new(),
        types: BTreeSet::new(),
        listing: None,
    };
    let mut aux: BTreeMap<String, Decl> = BTreeMap::new();
    let mut body_decls = Vec::new();
    let mut listing = String::from("spec = \"uorust/mir-candidates/1\"\n");
    for n in &order {
        let t = &done[n];
        let learned = options
            .verdicts
            .rows
            .get(&t.kappa)
            .filter(|v| v.digest == t.digest)
            .map(|v| v.axioms.clone())
            .or_else(|| options.learned.get(&t.kappa).cloned())
            .unwrap_or_default();
        for (k, d) in &t.lowered.aux {
            aux.entry(k.clone())
                .or_insert_with(|| with_axioms(d.clone(), &learned));
        }
        for d in &t.lowered.decls {
            body_decls.push(with_axioms(d.clone(), &learned));
        }
        out.types.extend(t.lowered.types.iter().cloned());
        notes.residue.remove(&t.path);
        notes.lifted.insert(
            t.path.clone(),
            (
                ctx.func(t.r).span.clone(),
                qualified(BEHAVIOR, &t.name),
                Vec::new(),
            ),
        );
    }
    for t in done.values() {
        let decls: Vec<String> = t
            .lowered
            .aux
            .keys()
            .cloned()
            .chain(t.lowered.decls.iter().map(|d| d.name().to_string()))
            .map(|d| toml_string(&d))
            .collect();
        listing.push_str(&format!(
            "\n[[candidate]]\nkappa = {}\nfunction = {}\ndigest = {}\ntrial = {}\ndecls = [{}]\n",
            toml_string(&t.kappa),
            toml_string(&t.path),
            toml_string(&t.digest),
            on_trial.contains(&t.name),
            decls.join(", ")
        ));
        if let Some(cause) = causes.get(&t.name) {
            let span = ctx.func(t.r).span.clone();
            notes
                .residue
                .insert(t.path.clone(), (span, Residue::Mir(cause.clone())));
        }
    }
    out.decls.extend(in_dependency_order(aux));
    out.decls.extend(body_decls);
    if options.trial != crate::mir::Trial::Off {
        out.listing = Some(listing);
    }
    out
}

/// Auxiliary declarations, each after every other one its text names (a
/// helper after the definitions it applies, a theorem after the
/// definitions it states), in name order otherwise.
fn in_dependency_order(aux: BTreeMap<String, Decl>) -> Vec<Decl> {
    let texts: BTreeMap<String, String> = aux
        .iter()
        .map(|(n, d)| (n.clone(), crate::lex::render_decl(d)))
        .collect();
    let names: Vec<String> = aux.keys().cloned().collect();
    let mentions = |text: &str, name: &str| {
        text.match_indices(name).any(|(i, _)| {
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            !word(text[..i].chars().next_back()) && !word(text[i + name.len()..].chars().next())
        })
    };
    let mut placed: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<String> = Vec::new();
    fn visit(
        n: &str,
        names: &[String],
        texts: &BTreeMap<String, String>,
        mentions: &dyn Fn(&str, &str) -> bool,
        placed: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if !placed.insert(n.to_string()) {
            return;
        }
        for m in names {
            if m != n && mentions(&texts[n], m) {
                visit(m, names, texts, mentions, placed, order);
            }
        }
        order.push(n.to_string());
    }
    for n in &names {
        visit(n, &names, &texts, &mentions, &mut placed, &mut order);
    }
    let mut aux = aux;
    order.into_iter().filter_map(|n| aux.remove(&n)).collect()
}

/// A translation's residue: a type the lift does not name is a MIR type
/// residue too.
pub(crate) fn mir_residue(reason: Residue) -> Residue {
    match reason {
        Residue::Mir(_) => reason,
        Residue::Type(t) => Residue::Mir(format!("Ty: {t}")),
        other => Residue::Mir(format!("Body: {}", other.detail())),
    }
}

/// `decl` with the axioms the verifier observed for it, where it reported
/// them.
fn with_axioms(decl: Decl, learned: &BTreeMap<String, Vec<String>>) -> Decl {
    let Some(observed) = learned.get(decl.name()) else {
        return decl;
    };
    match decl {
        Decl::Definition {
            name,
            parameters,
            result,
            body,
            recursive,
            termination,
            mutual,
            ..
        } => Decl::Definition {
            name,
            parameters,
            result,
            body,
            axioms: observed.clone(),
            recursive,
            termination,
            mutual,
        },
        Decl::Theorem {
            name,
            parameters,
            statement,
            proof,
            ..
        } => Decl::Theorem {
            name,
            parameters,
            statement,
            proof,
            axioms: observed.clone(),
        },
        other => other,
    }
}

/// Place translation `n` after every admitted translation it calls.
fn place_after_callees(
    n: &str,
    done: &BTreeMap<String, Translation>,
    admitted: &BTreeMap<String, bool>,
    placed: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) {
    if !admitted.get(n).copied().unwrap_or(false) || !placed.insert(n.to_string()) {
        return;
    }
    for c in &done[n].lowered.calls {
        if done.contains_key(c) {
            place_after_callees(c, done, admitted, placed, order);
        }
    }
    order.push(n.to_string());
}

/// The definition deciding equality of a unit-variant enum's values: a
/// match on both.
fn enum_equality(name: &str, variants: &[String]) -> Decl {
    let arms = |left: &String| {
        variants
            .iter()
            .map(|v| Arm {
                pattern: format!("{name}.{v}"),
                body: Expr::Flat((v == left).to_string()),
            })
            .collect()
    };
    Decl::Definition {
        name: lift::equality_name(name),
        parameters: vec![("left".into(), name.into()), ("right".into(), name.into())],
        result: "Bool".into(),
        body: Expr::Match {
            scrutinee: "left".into(),
            arms: variants
                .iter()
                .map(|v| Arm {
                    pattern: format!("{name}.{v}"),
                    body: Expr::Match {
                        scrutinee: "right".into(),
                        arms: arms(v),
                    },
                })
                .collect(),
        },
        axioms: Vec::new(),
        recursive: None,
        termination: None,
        mutual: None,
    }
}

/// Unit-variant enums by name, with their variants.
type Variants = BTreeMap<String, Vec<String>>;
/// Structs by name, with their named fields.
type Fields = BTreeMap<String, Vec<(String, crate::ast::RustType)>>;
/// Data-carrying enum name -> its variants' fields.
type Datas = BTreeMap<String, lift::RawVariants>;
/// A crate's lifted functions, by Rust name.
type Sigs = BTreeMap<String, lift::Sig>;
/// The Lean axioms of each lifted function, by Rust name.
type Axioms = BTreeMap<String, Vec<String>>;

/// A recursive family, after the types its members name: one inductive per
/// member (one mutual group when there are several, a struct as an
/// inductive of one constructor `mk`), the accessors of the structs' fields,
/// and, for a nested family, the mutual group of its size functions that
/// well-founded recursion over it measures by.
fn family_decls(
    family: &BTreeSet<String>,
    types: &Types,
    compared: &BTreeSet<String>,
    emitted: &mut BTreeSet<Ty>,
    decls: &mut Vec<Decl>,
) {
    for m in family {
        emitted.insert(Ty::Named(m.clone()));
    }
    for m in family {
        for t in types.member_fields(m) {
            let inner = t.mentions();
            for dep in &inner {
                type_decls(dep, types, compared, emitted, decls);
            }
        }
    }
    let first = family.iter().next().cloned().unwrap_or_default();
    let label = (family.len() > 1).then(|| format!("{first}Group"));
    // Each member's constructors: name and field types.
    let ctors = |m: &String| -> Vec<(String, Vec<Ty>)> {
        match types.datas.get(m) {
            Some(variants) => variants
                .iter()
                .map(|(v, fs)| (v.clone(), fs.iter().map(|(_, t)| t.clone()).collect()))
                .collect(),
            None => vec![(
                "mk".to_string(),
                types.structs[m].iter().map(|(_, t)| t.clone()).collect(),
            )],
        }
    };
    for m in family {
        decls.push(Decl::Data {
            name: m.clone(),
            constructors: ctors(m)
                .into_iter()
                .map(|(c, fs)| (c, fs.iter().map(Ty::render).collect()))
                .collect(),
            mutual: label.clone(),
        });
    }
    for m in family.iter().filter(|m| types.boxed(m)) {
        let fields = &types.structs[m];
        let binders: Vec<String> = (0..fields.len()).map(|i| format!("x{i}")).collect();
        for (i, (f, t)) in fields.iter().enumerate() {
            decls.push(Decl::Definition {
                name: Types::accessor(m, f),
                parameters: vec![("value".into(), m.clone())],
                result: t.render(),
                body: Expr::Match {
                    scrutinee: "value".into(),
                    arms: vec![Arm {
                        pattern: format!("{m}.mk {}", binders.join(" ")),
                        body: Expr::Flat(binders[i].clone()),
                    }],
                },
                axioms: Vec::new(),
                recursive: None,
                termination: None,
                mutual: None,
            });
        }
    }
    if !types.nested(&first) || !types.sizable(family) {
        return;
    }
    // The types the sizes range over: the members, then the lists and
    // options of members the members hold.
    let domains = types.size_domains(family);
    let size_label = format!("SizeOf{first}");
    let measure = |t: &Ty, v: &str| types.size_name(t, family).map(|f| format!("({f} {v})"));
    let sum = |base: &str, parts: Vec<String>| {
        parts
            .into_iter()
            .fold(base.to_string(), |acc, p| format!("({acc} + {p})"))
    };
    for d in &domains {
        let arms: Vec<Arm> = match d {
            Ty::Named(m) => ctors(m)
                .into_iter()
                .map(|(c, fs)| {
                    let names: Vec<String> = (0..fs.len()).map(|i| format!("x{i}")).collect();
                    let parts = fs
                        .iter()
                        .zip(&names)
                        .filter_map(|(t, v)| measure(t, v))
                        .collect();
                    Arm {
                        pattern: format!(
                            "{m}.{c}{}",
                            names.iter().map(|v| format!(" {v}")).collect::<String>()
                        ),
                        body: Expr::Flat(sum("1", parts)),
                    }
                })
                .collect(),
            Ty::List(t) => vec![
                Arm {
                    pattern: "List.nil".into(),
                    body: Expr::Flat("0".into()),
                },
                Arm {
                    pattern: "List.cons x0 x1".into(),
                    body: Expr::Flat(sum(
                        "1",
                        vec![
                            measure(t, "x0").unwrap_or_default(),
                            measure(d, "x1").unwrap_or_default(),
                        ],
                    )),
                },
            ],
            Ty::Option(t) => vec![
                Arm {
                    pattern: "Option.none".into(),
                    body: Expr::Flat("0".into()),
                },
                Arm {
                    pattern: "Option.some x0".into(),
                    body: Expr::Flat(sum("1", vec![measure(t, "x0").unwrap_or_default()])),
                },
            ],
            _ => continue,
        };
        decls.push(Decl::Definition {
            name: types.size_name(d, family).unwrap_or_default(),
            parameters: vec![("value".into(), d.render())],
            result: "Nat".into(),
            body: Expr::Match {
                scrutinee: "value".into(),
                arms,
            },
            axioms: Vec::new(),
            recursive: Some("value".into()),
            termination: None,
            mutual: Some(size_label.clone()),
        });
    }
}

/// The declaration of a named type or tuple, after those of the types its
/// fields name.
fn type_decls(
    ty: &Ty,
    types: &Types,
    compared: &BTreeSet<String>,
    emitted: &mut BTreeSet<Ty>,
    decls: &mut Vec<Decl>,
) {
    if !emitted.insert(ty.clone()) {
        return;
    }
    if let Ty::Named(n) = ty {
        if let Some(family) = types.family_of(n) {
            if family.len() > 1 || types.boxed(n) || types.nested(n) {
                family_decls(&family.clone(), types, compared, emitted, decls);
                return;
            }
        }
        if let Some(variants) = types.datas.get(n) {
            for (_, fields) in variants {
                for (_, t) in fields {
                    let inner = t.mentions();
                    for dep in &inner {
                        type_decls(dep, types, compared, emitted, decls);
                    }
                }
            }
            decls.push(Decl::Data {
                name: n.clone(),
                constructors: variants
                    .iter()
                    .map(|(v, fields)| {
                        (v.clone(), fields.iter().map(|(_, t)| t.render()).collect())
                    })
                    .collect(),
                mutual: None,
            });
            return;
        }
    }
    let fields: Vec<(String, Ty)> = match ty {
        Ty::Named(n) => match (types.enums.get(n), types.structs.get(n)) {
            (Some(variants), _) => {
                decls.push(Decl::Inductive {
                    name: n.clone(),
                    constructors: variants.clone(),
                });
                if compared.contains(n) {
                    decls.push(enum_equality(n, variants));
                }
                return;
            }
            (None, Some(fields)) => fields.clone(),
            (None, None) => return,
        },
        Ty::Tuple(ts) => ts
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("item{i}"), t.clone()))
            .collect(),
        _ => return,
    };
    for (_, t) in &fields {
        let inner = t.mentions();
        for dep in &inner {
            type_decls(dep, types, compared, emitted, decls);
        }
    }
    // Widths and characters render alike: one structure serves them all.
    if decls.iter().any(|d| d.name() == ty.render()) {
        return;
    }
    decls.push(Decl::Structure {
        name: ty.render(),
        fields: fields
            .iter()
            .map(|(f, t)| (f.clone(), t.render()))
            .collect(),
    });
}

fn supply(ctx: &Model) -> Module {
    let mut decls = vec![Decl::Structure {
        name: "LockedPackage".into(),
        fields: vec![
            ("name".into(), "String".into()),
            ("version".into(), "String".into()),
            ("source".into(), "String".into()),
            ("checksum".into(), "String".into()),
        ],
    }];
    let mut seen = BTreeSet::new();
    for l in &ctx.ws.lock {
        let name = format!(
            "package{}",
            pascal(&[&l.name, &l.version.replace('.', "_")])
        );
        if !seen.insert(name.clone()) {
            continue;
        }
        decls.push(Decl::value(
            name,
            "LockedPackage",
            record(
                "LockedPackage",
                &[
                    ("name", string(&l.name)),
                    ("version", string(&l.version)),
                    ("source", string(&l.source)),
                    ("checksum", string(&l.checksum)),
                ],
            ),
        ));
    }
    Module {
        name: SUPPLY.into(),
        glossaries: vec![BOOL, NAT, TITLES],
        imports: Vec::new(),
        title: "Locked supply chain",
        decls,
    }
}

fn register(rows: &[RegisterRow]) -> Vec<(String, Vec<u8>)> {
    let mut ids = String::from(
        "# model/ids.toml: one row per admitted feature, generated by uorust.\n\nspec = \"template/1\"\n",
    );
    let mut suites: BTreeMap<&str, Vec<&RegisterRow>> = BTreeMap::new();
    for row in rows {
        ids.push_str(&format!(
            "\n[[id]]\nid = \"{}\"\nlevel = \"build\"\nsuite = \"{}\"\nrecord = \"{}\"\nstatement = {}\n",
            row.id,
            row.suite,
            row.record,
            string(&row.statement)
        ));
        suites.entry(row.suite.as_str()).or_default().push(row);
    }
    let mut out = vec![("model/ids.toml".to_string(), ids.into_bytes())];
    for (suite, rows) in suites {
        let mut text = format!(
            "Feature: {suite}\n\n  The features uorust admitted for the {suite} package, each with the\n  interface calls that realize it and the acceptance rows that accept it.\n"
        );
        for row in rows {
            text.push_str(&format!(
                "\n  @{} @build\n  Scenario: {}\n    Given the record {}\n",
                row.id,
                crate::lex::escape(&row.statement),
                row.record
            ));
            for (i, call) in row.calls.iter().enumerate() {
                let word = if i == 0 { "When" } else { "And" };
                text.push_str(&format!("    {word} the interface call {call} is made\n"));
            }
            for (i, row) in row.acceptance.iter().enumerate() {
                let word = if i == 0 { "Then" } else { "And" };
                text.push_str(&format!("    {word} the acceptance row {row} holds\n"));
            }
        }
        out.push((
            format!("features/suites/{suite}.feature"),
            text.into_bytes(),
        ));
    }
    out
}

fn toml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A free function of product code by where it is declared: package,
/// target, module and name.
type FnSite = (usize, usize, String, String);

/// The free functions of product code whose name two modules (or two
/// targets) of one crate share, `toml_string` in `project` and in
/// `certify`, each -> its `rust` key and the LexLean name it lifts under:
/// qualified by its module, and by its target where that is not the
/// library, so that no two of them can meet. A name only one module
/// declares keeps its own, and so its definition and every κ's verdict.
fn homonym_fns(ctx: &Model) -> BTreeMap<FnSite, (String, String)> {
    let mut sites: BTreeMap<(usize, String), BTreeSet<FnSite>> = BTreeMap::new();
    for r in ctx.all_fns() {
        let f = ctx.func(r);
        if f.owner.is_some() || !ctx.product(r) {
            continue;
        }
        let file = ctx.file(r);
        let site = (r.pkg, file.target, file.module.clone(), f.name.clone());
        sites
            .entry((r.pkg, f.name.clone()))
            .or_default()
            .insert(site);
    }
    let mut out = BTreeMap::new();
    for ((pkg, name), at) in sites {
        if at.len() < 2 {
            continue;
        }
        let package = &ctx.ws.packages[pkg];
        for site in at {
            let target = &package.targets[site.1];
            let mut parts: Vec<String> = Vec::new();
            if !matches!(target.kind, TargetKind::Lib) {
                parts.push(target.name.replace('-', "_"));
            }
            parts.extend(
                site.2
                    .split("::")
                    .filter(|m| !m.is_empty())
                    .map(str::to_string),
            );
            if parts.is_empty() {
                parts.push(package.name.replace('-', "_"));
            }
            parts.push(name.clone());
            let rust = format!("{}#{}", site.1, parts.join("::"));
            let lex = lift::local_name(&parts.join("_"));
            out.insert(site, (rust, lex));
        }
    }
    out
}

/// How a file reads each name its crate's free functions share: the
/// function its module declares, else the one its `use` names, else none.
fn homonym_reading(
    ctx: &Model,
    homonyms: &BTreeMap<FnSite, (String, String)>,
    pkg: usize,
    fi: usize,
) -> BTreeMap<String, Option<String>> {
    let file = &ctx.ws.packages[pkg].files[fi];
    let mut out = BTreeMap::new();
    for ((p, target, module, name), (rust, _)) in homonyms {
        if *p != pkg {
            continue;
        }
        let entry = out.entry(name.clone()).or_insert(None);
        if *target != file.target {
            continue;
        }
        if *module == file.module {
            *entry = Some(rust.clone());
            continue;
        }
        let Some(path) = file.imports.get(name) else {
            continue;
        };
        // `crate::`, `self::` and `super::` as Rust reads them, any other
        // path from the file's module.
        let mut at: Vec<&str> = file.module.split("::").filter(|s| !s.is_empty()).collect();
        let mut segs: Vec<&str> = path.split("::").collect();
        match segs.first() {
            Some(&"crate") => {
                at.clear();
                segs.remove(0);
            }
            Some(&"self") => {
                segs.remove(0);
            }
            _ => {}
        }
        while segs.first() == Some(&"super") {
            at.pop();
            segs.remove(0);
        }
        if let Some((last, prefix)) = segs.split_last() {
            at.extend(prefix);
            if *last == name.as_str() && at.join("::") == *module && entry.is_none() {
                *entry = Some(rust.clone());
            }
        }
    }
    out
}

/// The registry dependency a type the lift refused belongs to: `syn` for
/// `syn::Expr`, `&mut toml::Value` or `Option<serde_json::Value>`.
fn foreign_crate(package: &Package, ty: &str) -> Option<String> {
    let deps: BTreeSet<String> = package
        .deps
        .iter()
        .filter(|d| d.path.is_none())
        .map(|d| d.name.replace('-', "_"))
        .collect();
    ty.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .filter_map(|word| word.split_once("::").map(|(head, _)| head.to_string()))
        .find(|head| deps.contains(head))
}

/// How an effect reads after "which": `performs a Fs effect: std::fs::read`.
fn effect_phrase(effect: &str) -> String {
    match effect.strip_prefix("calls ") {
        Some(_) => "reaches an effect".to_string(),
        None => format!("performs a {effect}"),
    }
}

fn frontier(notes: &Notes, kappas: &BTreeMap<String, String>) -> String {
    let kappa = |path: &str| {
        kappas
            .get(path)
            .map(|k| format!("kappa = \"{k}\"\n"))
            .unwrap_or_default()
    };
    let mut out = String::from(
        "# frontier.toml: what uorust did not admit or lift, each with a typed reason.\n\
         # A refusal is an open claim: the router proposed it and admission would\n\
         # not decide it. A residue row is a pure function outside the lift's floor.\n\
         # An effect row is the I/O shell: a function that performs an operating-system\n\
         # effect (file system, process, environment, network), or calls one that does;\n\
         # the security facet records each effect as a capability of its crate.\n\
         # A foreign row is a function whose parameter or result is a type of a registry\n\
         # dependency (syn, toml, serde_json, ...) that has no LexLean model.\n\
         # A lifted row names the definition a function lifted to and, where that\n\
         # definition parts from the Rust function, its divergence. Every function of\n\
         # product code has exactly one residue, foreign, effect or lifted row, keyed\n\
         # by the κ of its question (crate path and tokens): a lifted κ never turns\n\
         # unlifted in a later projection.\n\n\
         spec = \"uorust/frontier/1\"\n",
    );
    for r in &notes.refusals {
        out.push_str(&format!(
            "\n[[refusal]]\nstatus = \"open\"\nreason = \"{}\"\nroute = \"{}\"\nspan = {}\ndetail = {}\n",
            r.code(),
            r.route(),
            toml_string(&r.span()),
            toml_string(&r.detail())
        ));
    }
    for (path, (span, reason)) in &notes.residue {
        out.push_str(&format!(
            "\n[[residue]]\nstatus = \"open\"\n{}function = {}\nspan = {}\nreason = \"{}\"\ndetail = {}\n",
            kappa(path),
            toml_string(path),
            toml_string(&span.render()),
            reason.code(),
            toml_string(&reason.detail())
        ));
    }
    for (path, (span, detail)) in &notes.foreign {
        out.push_str(&format!(
            "\n[[foreign]]\n{}function = {}\nspan = {}\ndetail = {}\n",
            kappa(path),
            toml_string(path),
            toml_string(&span.render()),
            toml_string(detail)
        ));
    }
    for (path, (span, effect)) in &notes.effects {
        out.push_str(&format!(
            "\n[[effect]]\n{}function = {}\nspan = {}\neffect = {}\n",
            kappa(path),
            toml_string(path),
            toml_string(&span.render()),
            toml_string(effect)
        ));
    }
    for (path, (span, definition, divergence)) in &notes.lifted {
        out.push_str(&format!(
            "\n[[lifted]]\n{}function = {}\nspan = {}\ndefinition = {}\n",
            kappa(path),
            toml_string(path),
            toml_string(&span.render()),
            toml_string(definition)
        ));
        if !divergence.is_empty() {
            out.push_str(&format!(
                "divergence = {}\n",
                toml_string(&divergence.join("; "))
            ));
        }
    }
    out
}

fn spdx(ws: &Workspace, options: &Options) -> String {
    let id = |name: &str, version: &str| {
        let raw = format!("SPDXRef-Package-{name}-{version}");
        raw.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>()
    };
    let internal: BTreeMap<&str, &Package> =
        ws.packages.iter().map(|p| (p.name.as_str(), p)).collect();
    let mut packages = Vec::new();
    for l in &ws.lock {
        let license = internal
            .get(l.name.as_str())
            .map(|p| p.license.clone())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "NOASSERTION".to_string());
        let mut package = serde_json::json!({
            "SPDXID": id(&l.name, &l.version),
            "name": l.name,
            "versionInfo": l.version,
            "downloadLocation": if l.source.starts_with("registry+") {
                format!("https://crates.io/api/v1/crates/{}/{}/download", l.name, l.version)
            } else {
                "NOASSERTION".to_string()
            },
            "licenseConcluded": license,
            "filesAnalyzed": false,
        });
        if !l.checksum.is_empty() {
            package["checksums"] =
                serde_json::json!([{"algorithm": "SHA256", "checksumValue": l.checksum}]);
        }
        packages.push(package);
    }
    let lock_digest = sha(ws
        .lock
        .iter()
        .map(|l| format!("{} {} {}\n", l.name, l.version, l.checksum))
        .collect::<String>()
        .as_bytes());
    let doc = serde_json::json!({
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": options.name,
        "documentNamespace": format!("https://uor.foundation/spdx/{}/{}", options.name, lock_digest),
        "creationInfo": {"created": options.last_modified, "creators": ["Tool: uorust"]},
        "packages": packages,
    });
    let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::sha;

    /// NIST CAVP SHA-256 short byte-oriented known answers, the response
    /// file UOR-Foundation/compression locks at `standards/nist-sha256`.
    const SHORT: &str = include_str!("../tests/fixtures/nist-sha256/SHA256ShortMsg.rsp");

    fn vectors() -> Vec<(Vec<u8>, String)> {
        let mut out = Vec::new();
        let mut len = 0;
        let mut msg = Vec::new();
        for line in SHORT.lines() {
            if let Some(v) = line.strip_prefix("Len = ") {
                len = v.parse::<usize>().expect("a bit length") / 8;
            } else if let Some(v) = line.strip_prefix("Msg = ") {
                msg = (0..v.len() / 2)
                    .map(|i| u8::from_str_radix(&v[2 * i..2 * i + 2], 16).expect("hex"))
                    .take(len)
                    .collect();
            } else if let Some(v) = line.strip_prefix("MD = ") {
                out.push((std::mem::take(&mut msg), v.to_string()));
            }
        }
        out
    }

    #[test]
    fn sha_meets_every_nist_short_vector() {
        let vectors = vectors();
        assert_eq!(vectors.len(), 65);
        for (msg, md) in vectors {
            assert_eq!(sha(&msg), md);
        }
    }

    /// The vectors at the bounds of SHA-256 padding, which the lift proves of
    /// the prelude's digest: empty, one to three bytes, the longest message
    /// whose length fits its last block, the shortest that does not, a
    /// block less a byte and a whole block. The literals agree with the
    /// response file because both agree with `sha`.
    #[test]
    fn sha_meets_the_nist_vectors_at_the_padding_bounds() {
        assert_eq!(
            sha(&[]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha(&[0xd3]),
            "28969cdfa74a12c82f3bad960b0b000aca2ac329deea5c2328ebc6f2ba9802c1"
        );
        assert_eq!(
            sha(&[0x11, 0xaf]),
            "5ca7133fa735326081558ac312c620eeca9970d1e70a4b95533d956f072d1f98"
        );
        assert_eq!(
            sha(&[0xb4, 0x19, 0x0e]),
            "dff2e73091f6c05e528896c4c831b9448653dc2ff043528f6769437bc7b975c2"
        );
        assert_eq!(
            sha(&[
                0x3e, 0xbf, 0xb0, 0x6d, 0xb8, 0xc3, 0x8d, 0x5b, 0xa0, 0x37, 0xf1, 0x36, 0x3e, 0x11,
                0x85, 0x50, 0xaa, 0xd9, 0x46, 0x06, 0xe2, 0x68, 0x35, 0xa0, 0x1a, 0xf0, 0x50, 0x78,
                0x53, 0x3c, 0xc2, 0x5f, 0x2f, 0x39, 0x57, 0x3c, 0x04, 0xb6, 0x32, 0xf6, 0x2f, 0x68,
                0xc2, 0x94, 0xab, 0x31, 0xf2, 0xa3, 0xe2, 0xa1, 0xa0, 0xd8, 0xc2, 0xbe, 0x51
            ]),
            "6595a2ef537a69ba8583dfbf7f5bec0ab1f93ce4c8ee1916eff44a93af5749c4"
        );
        assert_eq!(
            sha(&[
                0x2d, 0x52, 0x44, 0x7d, 0x12, 0x44, 0xd2, 0xeb, 0xc2, 0x86, 0x50, 0xe7, 0xb0, 0x56,
                0x54, 0xba, 0xd3, 0x5b, 0x3a, 0x68, 0xee, 0xdc, 0x7f, 0x85, 0x15, 0x30, 0x6b, 0x49,
                0x6d, 0x75, 0xf3, 0xe7, 0x33, 0x85, 0xdd, 0x1b, 0x00, 0x26, 0x25, 0x02, 0x4b, 0x81,
                0xa0, 0x2f, 0x2f, 0xd6, 0xdf, 0xfb, 0x6e, 0x6d, 0x56, 0x1c, 0xb7, 0xd0, 0xbd, 0x7a
            ]),
            "cfb88d6faf2de3a69d36195acec2e255e2af2b7d933997f348e09f6ce5758360"
        );
        assert_eq!(
            sha(&[
                0xe2, 0xf7, 0x6e, 0x97, 0x60, 0x6a, 0x87, 0x2e, 0x31, 0x74, 0x39, 0xf1, 0xa0, 0x3f,
                0xcd, 0x92, 0xe6, 0x32, 0xe5, 0xbd, 0x4e, 0x7c, 0xbc, 0x4e, 0x97, 0xf1, 0xaf, 0xc1,
                0x9a, 0x16, 0xfd, 0xe9, 0x2d, 0x77, 0xcb, 0xe5, 0x46, 0x41, 0x6b, 0x51, 0x64, 0x0c,
                0xdd, 0xb9, 0x2a, 0xf9, 0x96, 0x53, 0x4d, 0xfd, 0x81, 0xed, 0xb1, 0x7c, 0x44, 0x24,
                0xcf, 0x1a, 0xc4, 0xd7, 0x5a, 0xce, 0xeb
            ]),
            "18041bd4665083001fba8c5411d2d748e8abbfdcdfd9218cb02b68a78e7d4c23"
        );
        assert_eq!(
            sha(&[
                0x5a, 0x86, 0xb7, 0x37, 0xea, 0xea, 0x8e, 0xe9, 0x76, 0xa0, 0xa2, 0x4d, 0xa6, 0x3e,
                0x7e, 0xd7, 0xee, 0xfa, 0xd1, 0x8a, 0x10, 0x1c, 0x12, 0x11, 0xe2, 0xb3, 0x65, 0x0c,
                0x51, 0x87, 0xc2, 0xa8, 0xa6, 0x50, 0x54, 0x72, 0x08, 0x25, 0x1f, 0x6d, 0x42, 0x37,
                0xe6, 0x61, 0xc7, 0xbf, 0x4c, 0x77, 0xf3, 0x35, 0x39, 0x03, 0x94, 0xc3, 0x7f, 0xa1,
                0xa9, 0xf9, 0xbe, 0x83, 0x6a, 0xc2, 0x85, 0x09
            ]),
            "42e61e174fbb3897d6dd6cef3dd2802fe67b331953b06114a65c772859dfc1aa"
        );
        for (msg, md) in vectors() {
            if [0, 1, 2, 3, 55, 56, 63, 64].contains(&msg.len()) {
                assert_eq!(sha(&msg), md);
            }
        }
    }
}
