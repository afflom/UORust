//! Projection: admitted facts to the files of a Prism project, byte for byte
//! the same for the same workspace.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::admit::Refusal;
use crate::facts::{Capability, FnFact, Package, SourceFile, Span, TargetKind, Workspace};
use crate::ingest::summary;
use crate::lex::{camel, pascal, string, string_list, Decl, Module};
use crate::lift::{self, Candidate, Enums, Residue};

/// What a projection is parameterised by beyond the workspace.
#[derive(Clone, Debug)]
pub struct Options {
    /// The project name; its PascalCase form is the LexLean module prefix.
    pub name: String,
    /// The control set's `lastModified`, `YYYY-MM-DDThh:mm:ssZ`.
    pub last_modified: String,
}

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

struct Ctx<'a> {
    ws: &'a Workspace,
    refusals: BTreeSet<Refusal>,
    residue: BTreeMap<String, (Span, Residue)>,
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

impl<'a> Ctx<'a> {
    fn package(&self, r: FnRef) -> &'a Package {
        &self.ws.packages[r.pkg]
    }

    fn file(&self, r: FnRef) -> &'a SourceFile {
        &self.ws.packages[r.pkg].files[r.file]
    }

    fn func(&self, r: FnRef) -> &'a FnFact {
        &self.file(r).fns[r.idx]
    }

    fn kind(&self, r: FnRef) -> &'a TargetKind {
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
pub(crate) fn project(ws: &Workspace, options: &Options) -> BTreeMap<String, Vec<u8>> {
    let mut ctx = Ctx {
        ws,
        refusals: BTreeSet::new(),
        residue: BTreeMap::new(),
        targets: BTreeMap::new(),
        components: Vec::new(),
        index: BTreeMap::new(),
        edges: BTreeMap::new(),
        calls: BTreeMap::new(),
        enum_routes: BTreeMap::new(),
    };
    for package in &ws.packages {
        for file in &package.files {
            if let Some(reason) = &file.parse_error {
                let line = reason
                    .split(':')
                    .nth(1)
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(1);
                ctx.refusals.insert(Refusal::Unparsable {
                    span: Span {
                        file: file.rel.clone(),
                        line,
                    },
                    reason: reason.clone(),
                });
            }
        }
    }
    components(&mut ctx);
    edges(&mut ctx);
    resolve(&mut ctx);
    route_enums(&mut ctx);

    let prefix = pascal(&[&options.name]);
    let mut modules = Vec::new();
    let arch = architecture(&ctx);
    let iface = interface(&mut ctx);
    let design = design(&mut ctx);
    let security = security(&ctx, options);
    let quality = quality(&mut ctx, iface.feature_count);
    let behavior = behavior(&mut ctx);
    let supply = supply(&ctx);
    modules.push(arch);
    modules.push(iface.module);
    modules.push(design);
    modules.push(security);
    modules.push(quality);
    modules.push(behavior);
    modules.push(supply);

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
         [limits]\nmax_file_bytes = 4194304\nmax_total_source_bytes = 67108864\n\
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
    files.insert("frontier.toml".into(), frontier(&ctx).into_bytes());
    files
}

pub(crate) fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn components(ctx: &mut Ctx<'_>) {
    let ws = ctx.ws;
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
            ctx.targets.insert((pkg, t), id.clone());
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
                    ctx.refusals.insert(Refusal::ExternalPath {
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
    for (i, (id, row)) in rows.into_iter().enumerate() {
        ctx.index.insert(id, i);
        ctx.components.push(row);
    }
}

fn edges(ctx: &mut Ctx<'_>) {
    let ws = ctx.ws;
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
                        Ctx::component_suffix(&from),
                        Ctx::component_suffix(&target_id)
                    ),
                );
                ctx.edges.insert(id, (from.clone(), target_id));
            }
        }
    }
}

/// Resolve every path call and unique method call to workspace functions.
fn resolve(ctx: &mut Ctx<'_>) {
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
                } else if first == "crate" || first == "self" || first == "super" {
                    candidates.retain(|r| {
                        r.pkg == caller.pkg && ctx.file(*r).target == ctx.file(caller).target
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
                    ctx.refusals.insert(Refusal::AmbiguousCall {
                        span: f.span.clone(),
                        callee: call.clone(),
                        candidates: names,
                    });
                }
            }
        }
        if !out.is_empty() {
            ctx.calls.insert(caller, out);
        }
    }
}

fn record(ty: &str, fields: &[(&str, String)]) -> String {
    let fields: Vec<String> = fields.iter().map(|(k, v)| format!("{k} := {v}")).collect();
    format!("(record {ty} {{{}}})", fields.join(", "))
}

fn architecture(ctx: &Ctx<'_>) -> Module {
    let ws = ctx.ws;
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
fn audience(ctx: &Ctx<'_>) -> (Named, Named, Viewpoints) {
    let ws = ctx.ws;
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
    let concerns = vec![
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

fn audience_index(ctx: &Ctx<'_>) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
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
fn interface_fns(ctx: &Ctx<'_>) -> Vec<FnRef> {
    ctx.all_fns()
        .into_iter()
        .filter(|r| {
            let f = ctx.func(*r);
            ctx.product(*r) && f.public && ctx.kind(*r) == &TargetKind::Lib
        })
        .collect()
}

fn call_name(ctx: &Ctx<'_>, r: FnRef) -> String {
    let path = ctx.path(r);
    let parts: Vec<&str> = path.split("::").collect();
    qualified(IFACE, &format!("call{}", pascal(&parts)))
}

fn interface(ctx: &mut Ctx<'_>) -> Interface {
    let ws = ctx.ws;
    let mut decls: Vec<Decl> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut push = |ctx: &mut Ctx<'_>, decl: Decl, span: &Span| -> bool {
        if seen.insert(decl.name().to_string()) {
            decls.push(decl);
            true
        } else {
            ctx.refusals.insert(Refusal::Collision {
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
            ctx,
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
            ctx,
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
            ctx,
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
                let call = format!("call{}Main", pascal(&[bin]));
                let text = if doc.is_empty() {
                    format!("{bin}: the binary's entry point.")
                } else {
                    format!("{bin}: {doc}")
                };
                if !push(
                    ctx,
                    Decl::value(call.clone(), "String", string(&text)),
                    &span,
                ) {
                    continue;
                }
                let acceptance: Vec<String> = package_tests
                    .iter()
                    .filter(|(r, _)| ctx.func(*r).literals.iter().any(|l| l.contains(&exe)))
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
                    let call = format!("call{}{}", pascal(&[bin]), pascal(&[&variant.name]));
                    let doc = summary(&variant.doc);
                    let text = if doc.is_empty() {
                        format!("{bin} {command}")
                    } else {
                        format!("{bin} {command}: {doc}")
                    };
                    if !push(
                        ctx,
                        Decl::value(call.clone(), "String", string(&text)),
                        &span,
                    ) {
                        continue;
                    }
                    let acceptance: Vec<String> = package_tests
                        .iter()
                        .filter(|(r, _)| ctx.func(*r).literals.contains(&command))
                        .map(|(_, row)| row.clone())
                        .collect();
                    features.push(Feature {
                        id: qualified(
                            IFACE,
                            &format!("feature{}{}", pascal(&[bin]), pascal(&[&variant.name])),
                        ),
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
    // Examples are acceptance rows of the crate's library features.
    for package in &ws.packages {
        for target in package
            .targets
            .iter()
            .filter(|t| t.kind == TargetKind::Example)
        {
            let name = format!("acExample{}", pascal(&[&package.name, &target.name]));
            let span = Span {
                file: target.src.clone(),
                line: 1,
            };
            let text = format!("The example {} at {} builds.", target.name, target.src);
            push(ctx, Decl::value(name, "String", string(&text)), &span);
        }
    }
    // Admission: a feature without acceptance is refused, not emitted.
    let mut admitted = Vec::new();
    for feature in features {
        if feature.acceptance.is_empty() {
            ctx.refusals.insert(Refusal::Unaccepted {
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
    Interface {
        feature_count: admitted.len(),
        module: Module {
            name: IFACE.into(),
            glossaries: vec![BOOL, NAT, PARCH],
            imports: vec!["Foundation.Arch".into()],
            title: "feature",
            decls,
        },
        register,
    }
}

/// Unit-variant enums of product code named for a state, or with transitions.
/// Ask the router for every enum of product code: `error-enum` when it is named
/// for an error or implements `Error`, `state-enum` when it is fieldless and
/// named for a state or matched in transitions. An enum with both tags is
/// refused as ambiguous and becomes neither record.
fn route_enums(ctx: &mut Ctx<'_>) {
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
                match crate::route::classify(tags) {
                    Some(crate::route::Routing::Routed(route)) => {
                        ctx.enum_routes.insert((pkg, fi, ei), route.id);
                    }
                    Some(crate::route::Routing::Ambiguous(a, b)) => {
                        ctx.refusals.insert(Refusal::AmbiguousFact {
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
}

fn state_enums(ctx: &Ctx<'_>) -> Vec<(usize, usize, usize)> {
    ctx.enum_routes
        .iter()
        .filter(|(_, route)| **route == "UR-R26")
        .map(|(key, _)| *key)
        .collect()
}

fn design(ctx: &mut Ctx<'_>) -> Module {
    let t = "Foundation.Arch::";
    let mut decls = Vec::new();
    let mut processes: BTreeMap<FnRef, String> = BTreeMap::new();
    let process_id = |ctx: &Ctx<'_>, r: FnRef| {
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
            ctx.refusals.insert(Refusal::NoInitialState {
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
            format!("uml{}", Ctx::component_suffix(&c.id)),
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
                format!("lifeline{base}{}", Ctx::component_suffix(lifeline)),
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
        let suffix = Ctx::component_suffix(component).to_string();
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
    let mut names = BTreeSet::new();
    decls.retain(|d| names.insert(d.name().to_string()));
    Module {
        name: DESIGN.into(),
        glossaries: vec![BOOL, NAT, PARCH],
        imports: vec!["Foundation.Arch".into()],
        title: "design view",
        decls,
    }
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

fn threats(ctx: &Ctx<'_>) -> Vec<Threat> {
    let ws = ctx.ws;
    let mut out = Vec::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (t, target) in package.targets.iter().enumerate() {
            let Some(component) = ctx.targets.get(&(pkg, t)) else {
                continue;
            };
            let suffix = Ctx::component_suffix(component).to_string();
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
        });
    }
    out
}

fn security(ctx: &Ctx<'_>, options: &Options) -> Module {
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
    let controls = indexed(rows.iter().map(|r| (control_id(r), r)).collect());
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
        let gated = r.evidence.iter().any(|(k, _)| *k == "gate");
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
        collapsed.chars().take(limit - 3).collect::<String>() + "..."
    }
}

fn quality(ctx: &mut Ctx<'_>, features: usize) -> Module {
    let ws = ctx.ws;
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
                ctx.refusals.insert(Refusal::NoMsrv {
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
    Module {
        name: QUAL.into(),
        glossaries: vec![BOOL, NAT, PQUAL],
        imports: vec!["Foundation.Qual".into()],
        title: "quality measure",
        decls,
    }
}

fn behavior(ctx: &mut Ctx<'_>) -> Module {
    let ws = ctx.ws;
    let mut decls = Vec::new();
    // Unit-variant enums of product code, by name; a name two crates share is refused.
    let mut enums: Enums = Enums::new();
    let mut enum_spans: BTreeMap<String, Span> = BTreeMap::new();
    let mut clashing: BTreeSet<String> = BTreeSet::new();
    for package in &ws.packages {
        for file in &package.files {
            if !matches!(
                package.targets[file.target].kind,
                TargetKind::Lib | TargetKind::Bin
            ) {
                continue;
            }
            for e in &file.enums {
                if e.variants.is_empty() || !e.variants.iter().all(|v| v.unit) {
                    continue;
                }
                if enums.contains_key(&e.name) {
                    clashing.insert(e.name.clone());
                    ctx.refusals.insert(Refusal::Collision {
                        span: e.span.clone(),
                        name: qualified(BEHAVIOR, &e.name),
                    });
                    continue;
                }
                enums.insert(
                    e.name.clone(),
                    e.variants.iter().map(|v| v.name.clone()).collect(),
                );
                enum_spans.insert(e.name.clone(), e.span.clone());
            }
        }
    }
    for name in &clashing {
        enums.remove(name);
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
                    ctx.refusals.insert(Refusal::Collision {
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
    let mut candidates = Vec::new();
    let mut names: BTreeMap<String, FnRef> = BTreeMap::new();
    for &r in &all {
        let f = ctx.func(r);
        if !ctx.product(r) || f.owner.is_some() || f.name == "main" {
            continue;
        }
        let Some(sig) = &f.item else { continue };
        let Some(block) = &f.block else { continue };
        let name = camel(&[&f.name]);
        match lift::signature(sig, &enums) {
            std::ops::ControlFlow::Continue((params, result)) => {
                if names.contains_key(&f.name) {
                    ctx.refusals.insert(Refusal::Collision {
                        span: f.span.clone(),
                        name: qualified(BEHAVIOR, &name),
                    });
                    continue;
                }
                names.insert(f.name.clone(), r);
                candidates.push(Candidate {
                    rust: f.name.clone(),
                    name,
                    params,
                    result,
                    block,
                });
            }
            std::ops::ControlFlow::Break(reason) => {
                ctx.residue.insert(ctx.path(r), (f.span.clone(), reason));
            }
        }
    }
    let (lifted, failed) = lift::lift_all(&candidates, &enums);
    for (rust, reason) in failed {
        if let Some(r) = names.get(&rust) {
            ctx.residue
                .insert(ctx.path(*r), (ctx.func(*r).span.clone(), reason));
        }
    }
    // The inductives the lifted definitions name.
    let used: BTreeSet<String> = lifted
        .iter()
        .flat_map(|d| match d {
            Decl::Definition {
                parameters, result, ..
            } => parameters
                .iter()
                .map(|(_, t)| t.clone())
                .chain(std::iter::once(result.clone()))
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .filter(|t| enums.contains_key(t))
        .collect();
    for name in &used {
        decls.push(Decl::Inductive {
            name: name.clone(),
            constructors: enums[name].clone(),
        });
    }
    let results: BTreeMap<String, (String, String)> = lifted
        .iter()
        .filter_map(|d| match d {
            Decl::Definition { name, result, .. } => candidates
                .iter()
                .find(|c| &c.name == name)
                .map(|c| (c.rust.clone(), (name.clone(), result.clone()))),
            _ => None,
        })
        .collect();
    decls.extend(lifted);
    // Closed assertions of tests over lifted functions are theorems.
    let mut theorems = BTreeSet::new();
    for &r in &all {
        let f = ctx.func(r);
        if !f.test {
            continue;
        }
        for (i, (kind, args)) in f.asserts.iter().enumerate() {
            if let Some(statement) = lift::acceptance(kind, args, &results, &enums) {
                let name = format!("accept{}{}", pascal(&[&ctx.package(r).name, &f.name]), i);
                if theorems.insert(name.clone()) {
                    decls.push(Decl::Theorem {
                        name,
                        statement,
                        proof: "decide{}".to_string(),
                    });
                }
            }
        }
    }
    Module {
        name: BEHAVIOR.into(),
        glossaries: vec![BOOL, NAT],
        imports: Vec::new(),
        title: "Boolean",
        decls,
    }
}

fn supply(ctx: &Ctx<'_>) -> Module {
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
        glossaries: vec![BOOL, NAT],
        imports: Vec::new(),
        title: "Boolean",
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

fn frontier(ctx: &Ctx<'_>) -> String {
    let mut out = String::from(
        "# frontier.toml: what uorust did not admit or lift, each with a typed reason.\n\
         # A refusal is an open claim: the router proposed it and admission would\n\
         # not decide it. A residue row is a function outside the lift's floor.\n\n\
         spec = \"uorust/frontier/1\"\n",
    );
    for r in &ctx.refusals {
        out.push_str(&format!(
            "\n[[refusal]]\nstatus = \"open\"\nreason = \"{}\"\nroute = \"{}\"\nspan = {}\ndetail = {}\n",
            r.code(),
            r.route(),
            toml_string(&r.span()),
            toml_string(&r.detail())
        ));
    }
    for (path, (span, reason)) in &ctx.residue {
        out.push_str(&format!(
            "\n[[residue]]\nstatus = \"open\"\nfunction = {}\nspan = {}\nreason = \"{}\"\ndetail = {}\n",
            toml_string(path),
            toml_string(&span.render()),
            reason.code(),
            toml_string(&reason.detail())
        ));
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
