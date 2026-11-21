//! Projection: admitted facts to the files of a Prism project, byte for byte
//! the same for the same workspace.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::admit::Refusal;
use crate::facts::{Capability, FnFact, Package, SourceFile, Span, Syntax, TargetKind, Workspace};
use crate::ingest::summary;
use crate::lex::{pascal, string, string_list, Arm, Decl, Expr, Module};
use crate::lift::{self, Candidate, Residue, Ty, Types};
use crate::register::{queue_row, ranked, Register};
use crate::UorustError;

mod library;

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
    /// The limits file the workspace declares for its projection's LexLean
    /// and PrismPM projects (`--limits`); without one, the SDK scaffold's,
    /// and the frontier says which: see [`crate::limits`].
    pub limits: Option<std::path::PathBuf>,
    /// The records of `uorust embed --workspace` and the configurations
    /// the workspace declares: a function row that did not lift is joined
    /// to its body in each declared configuration, and is embedded-closed
    /// where they say so ([`crate::embedding`]).
    pub embedding: Option<Embedding>,
}

/// What `project --embedding` joins ([`crate::embedding`]).
#[derive(Clone, Debug)]
pub struct Embedding {
    /// The directory of the records `uorust embed --workspace` wrote (any
    /// depth under it).
    pub records: std::path::PathBuf,
    /// The configurations the workspace declares, each by its label
    /// ([`crate::shard::Configuration::label`]: `default features`, `all
    /// features`, ...). A row is embedded-closed only where the records of
    /// every one of them cover its crate; records of another configuration
    /// are not read.
    pub configurations: Vec<String>,
}

/// The largest source file of the projects the tool writes that take no
/// `--limits` (the embedding, the bridge, the core MIR path): the SDK
/// scaffold's `max_file_bytes`, which a test holds equal to the asset's.
pub(crate) const MAX_FILE_BYTES: usize = 4_194_304;

/// The projection's own register of what it did not admit or lift.
const FRONTIER: &str = "frontier.toml";

/// The projection's LexLean project file, which states its limits.
const LEXLEAN: &str = "lexlean.toml";

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

/// What name resolution names one call by (`Model::named`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Meant {
    /// This one function of product code.
    Fn(FnRef),
    /// Nothing of product code: a function outside the workspace, a
    /// constructor, a closure or a function the body itself declares.
    Outside,
    /// The places it is written differ, or the rules leave it open.
    Open,
}

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
    /// The projection's name, as `--name` gives it: the only name the
    /// projection writes for the workspace. The directory the workspace is
    /// checked out in is no fact of the workspace and enters no file, so
    /// two checkouts of one tree project byte for byte alike.
    name: String,
    /// The limits the projection's project files state, and where they
    /// came from.
    limits: crate::limits::Limits,
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
    /// Each call a function writes, as written, -> what the language's name
    /// resolution names at every place it is written: one function of
    /// product code, nothing of product code (a function outside the
    /// workspace, a constructor, a closure), or open where the places
    /// differ or the rules do not follow the call. Admission's reading,
    /// which the lift reads calls by too.
    named: BTreeMap<FnRef, BTreeMap<String, Meant>>,
    /// The route the router admitted for each enum: `(package, file, enum)`.
    enum_routes: BTreeMap<(usize, usize, usize), &'static str>,
    /// The bare names a function's module binds by a `use` to a type of a
    /// registry crate, each with the crate.
    registry: BTreeMap<FnRef, BTreeMap<String, String>>,
    /// The bare names each source file `(package, file)` binds, in every
    /// one of its modules, to a registered crate's type whose reading the
    /// package's lock admits, each with the type's path.
    registered: BTreeMap<(usize, usize), BTreeMap<String, String>>,
    /// The module each function stands in (`Model::module_of`).
    modules: BTreeMap<FnRef, String>,
    /// Per package, the registered crates' type paths it reads
    /// (`registered_paths`).
    readable: BTreeMap<usize, Vec<String>>,
    /// What each function's own module scope binds itself, and whether it
    /// may bring a trait from outside the workspace into scope
    /// (`names::Resolver::scope_binding`).
    scoped: BTreeMap<FnRef, (BTreeSet<String>, bool)>,
    /// Each trait of the workspace that may give a method to a type outside
    /// the workspace (`names::Resolver::foreign_impl_traits`), by name, with
    /// the names of the methods and associated functions it declares.
    trait_decls: BTreeMap<String, BTreeSet<String>>,
    /// The bare names each module, by package, file and inline module,
    /// binds by a `use` or a glob to an item of a registry crate, each with
    /// the path it names: the module that declares a type names its
    /// fields' types so, and the module of a function its signature's.
    registry_paths: BTreeMap<(usize, usize, String), BTreeMap<String, String>>,
    /// What each name several of a crate's types share names in each
    /// module that may write it, by package, file and inline module, as
    /// the language resolves it.
    type_readings: BTreeMap<(usize, usize, String), BTreeMap<String, crate::names::TypeReading>>,
    /// The public type aliases the registry crates the packages depend on
    /// declare at their roots, by path (`krate::Alias`), read from each
    /// crate's source at the version Cargo.lock pins.
    crate_aliases: BTreeMap<String, crate::ast::Alias>,
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
    /// The acceptance roots each package's tests yield, by package name: the
    /// closed Boolean definitions the Behavior module proves true.
    roots: BTreeMap<String, Vec<String>>,
    /// Every construct the types of each function's signature state, by
    /// the function's path: the constructs that stop it whichever the lift
    /// meets first.
    stops: BTreeMap<String, Vec<(String, String)>>,
    /// Each LexLean name of a workspace type with the site that declares
    /// it, and whether it is another name than the type's own: a chain
    /// names the owners of its places by these.
    lexed: BTreeMap<String, (Span, bool)>,
    /// The sites of each type construct of the queue, by construct.
    sites: BTreeMap<String, crate::register::Sites>,
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

    /// What the lift may read of a function's own module scope
    /// (`lift::Seen`): the names its `use` items bind (scope `scope` of its
    /// file), the names the module and the body bind themselves, whether a
    /// trait from outside the workspace may be in scope, and whether the
    /// package invokes a macro in item position.
    fn seen(&self, r: FnRef, scope: usize) -> lift::Seen {
        let file = self.file(r);
        let (local, outside_traits) = self
            .scoped
            .get(&r)
            .cloned()
            .unwrap_or_else(|| (BTreeSet::new(), true));
        let empty = crate::facts::ScopeFact::default();
        let at = file.scopes.get(scope).unwrap_or(&empty);
        // A trait is in scope where the scope declares it or a `use` binds
        // it; a `use` is matched by the last segment of its path, which
        // also covers `use path::Trait as _`, and a glob may bind any.
        let trait_methods = self
            .trait_decls
            .iter()
            .filter(|(t, _)| {
                at.glob
                    || at.traits.contains(t)
                    || at
                        .uses
                        .iter()
                        .any(|(n, p)| n == *t || p.rsplit("::").next() == Some(t.as_str()))
            })
            .flat_map(|(_, methods)| methods.iter().cloned())
            .collect();
        // A macro in item position makes items in the module it is invoked
        // in, which a glob or a `use` of a path inside the crate may bring
        // to another module.
        let internal = |p: &str| {
            let head = p.split("::").next().unwrap_or(p);
            matches!(head, "crate" | "self" | "super") || local.contains(head)
        };
        let package = &self.ws.packages[r.pkg];
        let package_macros = package.makes_items(r.file);
        let macro_items = package.file_makes_items(r.file)
            || (package_macros
                && (at.outward_glob(scope) || at.uses.iter().any(|(_, p)| internal(p))));
        lift::Seen {
            paths_open: macro_reachable(&self.ws.packages[r.pkg], r.file),
            trait_methods,
            uses: file
                .scopes
                .get(scope)
                .map(|s| s.uses.iter().cloned().collect())
                .unwrap_or_default(),
            local,
            outside_traits,
            macro_items,
        }
    }

    /// The module a function stands in, inside its target: its file's
    /// module and the inline modules around it in the file. Rust's path of
    /// the function runs through both. It is the name resolver's reading
    /// (`names::Resolver::module_path`), the one the calls are resolved in.
    fn module_of(&self, r: FnRef) -> String {
        self.modules
            .get(&r)
            .cloned()
            .unwrap_or_else(|| self.file(r).module.clone())
    }

    /// `crate::module::Type::name`; and `crate::module::<Type as
    /// Trait>::name` for a trait implementation's method whose type has
    /// another method of its name, which is how Rust names the two apart.
    /// The module is the one Rust names: the file's, then the inline
    /// modules around the function, so a function of `mod m { .. }` and one
    /// of its name beside it are two paths, two rows and two κ.
    fn path(&self, r: FnRef) -> String {
        let f = self.func(r);
        let mut parts = vec![self.crate_ident(r)];
        let module = self.module_of(r);
        if !module.is_empty() {
            parts.push(module);
        }
        if let Some(owner) = &f.owner {
            match &f.trait_ref {
                Some(named) if f.apart => parts.push(format!("<{owner} as {named}>")),
                _ => parts.push(owner.clone()),
            }
        }
        parts.push(f.name.clone());
        parts.join("::")
    }

    /// The registry crate a type names through a `use` of the function's
    /// module: `Value` where `use serde_json::Value;` binds it.
    fn bound_crate(&self, r: FnRef, ty: &str) -> Option<String> {
        let bound = self.registry.get(&r)?;
        ty.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .filter_map(|word| word.split("::").next())
            .find_map(|head| bound.get(head).cloned())
    }

    /// The registry crate whose item a leaf's head path names, among the
    /// registry dependencies of every package (the package that declares
    /// the type the leaf is a field of is the one that depends on it):
    /// `big` for `big::BigX` and `big::Ratio<i64>`. A borrow reads as what
    /// it borrows; a head the lift does not read (`RefCell<big::BigX>`) is
    /// the leaf whatever it holds.
    fn registry_crate(&self, leaf: &str) -> Option<String> {
        let (krate, _) = leaf_head(leaf).split_once("::")?;
        self.ws
            .packages
            .iter()
            .flat_map(|p| &p.deps)
            .filter(|d| d.path.is_none())
            .map(|d| d.name.replace('-', "_"))
            .find(|dep| dep == krate)
    }

    fn component_of(&self, r: FnRef) -> Option<&String> {
        self.targets.get(&(r.pkg, self.file(r).target))
    }

    /// The foreign row's detail of a refusal of `r` whose type is a
    /// registry dependency's, if it is one.
    fn foreign_detail(&self, r: FnRef, reason: &Residue) -> Option<String> {
        let Residue::Type(ty) = reason else {
            return None;
        };
        let dep = match chain_leaf(ty) {
            Some(leaf) => self.registry_crate(leaf),
            None => self
                .registry_crate(ty)
                .or_else(|| self.bound_crate(r, leaf_head(ty))),
        }?;
        Some(format!(
            "takes or returns {ty}, a type of the {dep} crate with no LexLean model"
        ))
    }

    /// The cause and construct a refusal of `r` is counted under in the
    /// register, as [`crate::register::Status::construct`] reads its row:
    /// a foreign row's, or the residue's code and detail.
    fn counted(&self, r: FnRef, reason: &Residue) -> (String, String) {
        match self.foreign_detail(r, reason) {
            Some(detail) => ("foreign".to_string(), detail),
            None => (reason.code().to_string(), reason.detail()),
        }
    }

    /// A function the lift refused: residue, or a foreign row when what
    /// stopped it is a type of a registry dependency.
    fn record_residue(&self, notes: &mut Notes, r: FnRef, span: Span, reason: Residue) {
        if let Some(detail) = self.foreign_detail(r, &reason) {
            // A path two functions project to (a name collision) keeps
            // one row: residue, wherever one of them is residue.
            if !notes.residue.contains_key(&self.path(r)) {
                notes.foreign.insert(self.path(r), (span, detail));
            }
            return;
        }
        notes.foreign.remove(&self.path(r));
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

/// The whole projection: path -> bytes. There is none where the function
/// rows it would write are not a lift register, or where a module it would
/// write is larger than the limit its own `lexlean.toml` states.
pub(crate) fn project(
    ws: &Workspace,
    syntax: &Syntax,
    options: &Options,
    dumps: Option<&crate::mir::Dumps>,
    source: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>, UorustError> {
    let mut notes = Notes::default();
    // Items under `cfg` predicates no build holds together are one fact:
    // the projection states the variant its configuration compiles.
    let (ws, syntax, variants) = crate::names::stated(ws, syntax);
    let (ws, syntax) = (&ws, &syntax);
    notes.refusals.extend(variants);
    let resolver = crate::names::Resolver::new(ws, syntax);
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
    let limits = match &options.limits {
        Some(path) => crate::limits::Limits::declared(path)?,
        None => crate::limits::Limits::sdk(),
    };
    let mut ctx = Model {
        name: options.name.clone(),
        limits,
        ws: ws.clone(),
        targets,
        components,
        index,
        edges: BTreeMap::new(),
        calls: BTreeMap::new(),
        named: BTreeMap::new(),
        enum_routes: BTreeMap::new(),
        registry: BTreeMap::new(),
        registered: BTreeMap::new(),
        modules: BTreeMap::new(),
        readable: BTreeMap::new(),
        scoped: BTreeMap::new(),
        trait_decls: BTreeMap::new(),
        registry_paths: BTreeMap::new(),
        type_readings: BTreeMap::new(),
        crate_aliases: BTreeMap::new(),
    };
    // A trait counts only where some `impl` may give its methods to a type
    // the lift reads as the standard library's or a registered crate's;
    // where a macro of a library or binary of the workspace may write an
    // `impl`, every trait (a test, bench or example target is a crate of
    // its own that nothing depends on).
    let foreign = resolver.foreign_impl_traits();
    let macros = ws.packages.iter().any(|p| {
        (0..p.files.len()).any(|i| {
            matches!(
                p.targets[p.files[i].target].kind,
                TargetKind::Lib | TargetKind::Bin
            ) && p.file_makes_items(i)
        })
    });
    for f in ws
        .packages
        .iter()
        .flat_map(|p| &p.files)
        .flat_map(|f| &f.fns)
    {
        if let (true, Some(owner)) = (f.in_trait, &f.owner) {
            if !macros && !foreign.contains(owner) {
                continue;
            }
            ctx.trait_decls
                .entry(owner.clone())
                .or_default()
                .insert(f.name.clone());
        }
    }
    ctx.scoped = ctx
        .all_fns()
        .into_iter()
        .map(|r| (r, resolver.scope_binding((r.pkg, r.file, r.idx))))
        .collect();
    ctx.modules = ctx
        .all_fns()
        .into_iter()
        .map(|r| (r, resolver.module_path((r.pkg, r.file, r.idx))))
        .collect();
    let registry_deps: BTreeSet<String> = ws
        .packages
        .iter()
        .flat_map(|p| &p.deps)
        .filter(|d| d.path.is_none())
        .map(|d| d.name.replace('-', "_"))
        .collect();
    let cargo_home = crate::registry::cargo_home();
    for krate in &registry_deps {
        // A crate whose source is not read declares no alias the lift
        // reads: a type it names stays a type of that crate.
        if let Some(aliases) = crate::registry::crate_aliases(&cargo_home, krate, &ws.lock) {
            ctx.crate_aliases.extend(aliases);
        }
    }
    for (pkg, package) in ws.packages.iter().enumerate() {
        let mut declared: BTreeMap<&str, usize> = BTreeMap::new();
        for file in &package.files {
            let names = file.enums.iter().map(|e| e.name.as_str());
            for name in names.chain(file.structs.iter().map(|st| st.name.as_str())) {
                *declared.entry(name).or_default() += 1;
            }
        }
        let shared: Vec<&str> = declared
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(name, _)| name)
            .collect();
        for (fi, file) in package.files.iter().enumerate() {
            for inline in syntax.names.inline_modules(&file.rel) {
                // Resolve qualified types before localization can erase the
                // module that distinguishes two declarations of one name.
                let mut pathed: BTreeSet<String> = file
                    .fns
                    .iter()
                    .filter(|f| syntax.names.fn_module(&file.rel, f.span.line, &f.name) == inline)
                    .filter_map(|f| f.owner.as_deref())
                    .map(|o| o.split('<').next().unwrap_or(o).trim())
                    .filter(|o| o.contains("::"))
                    .map(str::to_string)
                    .collect();
                let written = &syntax.files[&file.rel];
                for (fact, fields) in file.structs.iter().zip(&written.structs) {
                    if syntax
                        .names
                        .type_module(&file.rel, fact.span.line, &fact.name)
                        == inline
                    {
                        for (_, ty) in fields {
                            pathed.extend(ty.paths());
                        }
                    }
                }
                for (fact, variants) in file.enums.iter().zip(&written.enums) {
                    if syntax
                        .names
                        .type_module(&file.rel, fact.span.line, &fact.name)
                        == inline
                    {
                        for (_, fields) in variants {
                            for (_, ty) in fields {
                                pathed.extend(ty.paths());
                            }
                        }
                    }
                }
                for (fact, function) in file.fns.iter().zip(&written.fns) {
                    if syntax
                        .names
                        .fn_module(&file.rel, fact.span.line, &fact.name)
                        == inline
                    {
                        for argument in &function.signature.inputs {
                            if let crate::ast::FnArg::Typed(argument) = argument {
                                pathed.extend(argument.ty.paths());
                            }
                        }
                        if let Some(result) = &function.signature.output {
                            pathed.extend(result.paths());
                        }
                    }
                }
                for (_, alias, scope) in &written.aliases {
                    if file
                        .scopes
                        .get(*scope)
                        .is_some_and(|at| at.module == inline)
                    {
                        pathed.extend(
                            alias
                                .to
                                .paths()
                                .into_iter()
                                .filter(|p| !alias.params.contains(p)),
                        );
                    }
                }
                let read: BTreeMap<String, crate::names::TypeReading> = shared
                    .iter()
                    .map(|n| (*n).to_string())
                    .chain(pathed)
                    .map(|n| {
                        let reading = resolver.type_reading(pkg, fi, &inline, &n);
                        (n, reading)
                    })
                    .collect();
                if !read.is_empty() {
                    ctx.type_readings.insert((pkg, fi, inline), read);
                }
            }
        }
    }
    ctx.registry_paths = ws
        .packages
        .iter()
        .enumerate()
        .flat_map(|(pkg, package)| (0..package.files.len()).map(move |fi| (pkg, fi)))
        .flat_map(|(pkg, fi)| {
            let file = &ws.packages[pkg].files[fi];
            let written = bare_names(&syntax.files[&file.rel], file);
            let resolver = &resolver;
            syntax
                .names
                .inline_modules(&file.rel)
                .into_iter()
                .map(move |inline| {
                    // A `use` binds a name before a glob does.
                    let mut bound = resolver.inline_registry_paths(pkg, fi, &inline);
                    for (name, path) in resolver.glob_paths(pkg, fi, &inline, &written) {
                        bound.entry(name).or_insert(path);
                    }
                    ((pkg, fi, inline), bound)
                })
        })
        .filter(|(_, bound)| !bound.is_empty())
        .collect();
    ctx.registry = ctx
        .all_fns()
        .into_iter()
        .map(|r| (r, resolver.registry_types((r.pkg, r.file, r.idx))))
        .filter(|(_, bound)| !bound.is_empty())
        .collect();
    for (pkg, package) in ws.packages.iter().enumerate() {
        let read = registered_paths(ws, &resolver, pkg);
        if !ws.replaced.is_empty()
            && !crate::registered::of_package(package, &ws.lock, &[]).is_empty()
        {
            notes.refusals.insert(Refusal::SourceReplaced {
                config: ws.replaced.join(", "),
            });
        }
        if read.is_empty() {
            continue;
        }
        ctx.readable.insert(pkg, read.clone());
        for fi in 0..package.files.len() {
            let bound = resolver.registry_bindings(pkg, fi, &read);
            let bound = crate::registered::bindings(package, fi, bound);
            if !bound.is_empty() {
                ctx.registered.insert((pkg, fi), bound);
            }
        }
    }
    ctx.edges = edges(&ctx);
    let (calls, named, refused) = resolve(&ctx, &resolver);
    ctx.calls = calls;
    ctx.named = named;
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
    let mir = dumps.zip(options.mir.as_ref());
    let (behavior, candidates) = behavior(&ctx, syntax, &resolver, &mut notes, mir);
    let supply = supply(&ctx);
    // What a library package offers by Rust's own rule: its `pub` functions.
    let public: BTreeSet<String> = ctx
        .all_fns()
        .into_iter()
        .filter(|&r| ctx.func(r).public)
        .map(|r| ctx.path(r))
        .collect();
    let libraries = library::libraries(ws, &prefix, &behavior, &public, &mut notes);
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
    // The lift register of this run, read from the function rows the
    // frontier is about to carry: what the queue ranks and the Quality facet
    // measures is what a reader of the frontier finds.
    // The records are read against the workspace's source digest: a
    // record of another tree is refused.
    let records = match &options.embedding {
        Some(e) => Some(crate::embedding::read(
            &e.records,
            source.unwrap_or_default(),
        )?),
        None => None,
    };
    let embedded = match (&options.embedding, &records) {
        (Some(e), Some(records)) => {
            crate::embedding::closed(records, &e.configurations, &embedding_keys(&ctx))
        }
        _ => BTreeMap::new(),
    };
    let rows = function_rows(&notes, &kappas, &embedded);
    let lift = Register::parse(std::path::Path::new(FRONTIER), &rows)?;
    notes.sites = type_sites(&ctx, &notes);
    // Quality is written last of the facets: it measures what the others
    // admitted, refused and lifted.
    let (quality, refused) = quality(&ctx, iface.feature_count, &lift, &notes.refusals);
    notes.refusals.extend(refused);
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
    // The library profile's type belongs to the library projects alone: a
    // project with no library value states no library.
    let source = |name: &str| {
        Module {
            name: name.into(),
            glossaries: Vec::new(),
            imports: Vec::new(),
            title: "",
            decls: Vec::new(),
        }
        .path()
    };
    let (library_model, prelude) = (source(library::LIBRARY_MODEL), source(lift::PRELUDE));
    for (path, bytes) in crate::assets::FIXED {
        if *path != library_model {
            files.insert((*path).to_string(), bytes.as_bytes().to_vec());
        }
    }
    let mut entrypoints: Vec<String> = files
        .keys()
        .filter(|p| p.ends_with(MODULE_SUFFIX))
        .cloned()
        .collect();
    // Every file that is no generated module comes first: beside the modules
    // themselves, these are what can cite a record of one by its ID.
    for (path, bytes) in register(&iface.register) {
        files.insert(path, bytes);
    }
    files.insert("supply/spdx.json".into(), spdx(ws, options).into_bytes());
    // The prover's trial run lists the translations it admitted on trial.
    if let Some(listing) = candidates {
        files.insert(crate::mir::CANDIDATES.into(), listing.into_bytes());
    }
    // How the program holds each type, where the embedding's records read it.
    let rows = match &records {
        Some(records) => {
            rows + &holding_rows(&crate::embedding::holdings(records, &type_keys(&ctx)))
        }
        None => rows,
    };
    files.insert(
        FRONTIER.into(),
        frontier(&notes, &lift, &rows, &ctx.limits).into_bytes(),
    );
    let cited = |id: &str| {
        files
            .values()
            .any(|bytes| String::from_utf8_lossy(bytes).contains(id))
    };
    // Each module within the limit the project file below states, in parts
    // where it is beyond it and its declarations can be written apart.
    let written = sized(&modules, ctx.limits.max_file_bytes(), &cited);
    for (path, text) in written {
        entrypoints.push(path.clone());
        files.insert(path, text.into_bytes());
    }
    entrypoints.sort();
    let toolchain = files.get("lean-toolchain").cloned().unwrap_or_default();
    files.extend(config(
        &prefix,
        &options.name,
        &entrypoints,
        &toolchain,
        &ctx.limits,
    ));
    // Each library project beside the workspace project: the fixed files a
    // LexLean project of the engine's lexicon needs, its closure and its
    // value, each module within the same limit.
    for lib in &libraries {
        let dir = format!("{}/{}/", library::LIBRARIES, lib.package);
        let mut own: BTreeMap<String, Vec<u8>> = crate::assets::FIXED
            .iter()
            .filter(|(path, _)| {
                !path.starts_with("src/") || *path == library_model || *path == prelude
            })
            .map(|(path, bytes)| ((*path).to_string(), bytes.as_bytes().to_vec()))
            .collect();
        let mut entrypoints: Vec<String> = own
            .keys()
            .filter(|p| p.ends_with(MODULE_SUFFIX))
            .cloned()
            .collect();
        let modules = [lib.behavior.clone(), lib.value.clone()];
        for (path, text) in sized(&modules, ctx.limits.max_file_bytes(), &|_| false) {
            entrypoints.push(path.clone());
            own.insert(path, text.into_bytes());
        }
        entrypoints.sort();
        own.extend(config(
            &prefix,
            &lib.package,
            &entrypoints,
            &toolchain,
            &ctx.limits,
        ));
        files.extend(
            own.into_iter()
                .map(|(path, bytes)| (format!("{dir}{path}"), bytes)),
        );
    }
    within_limit(&files)?;
    Ok(files)
}

/// The project files of one LexLean and PrismPM project named `name` (in
/// TOML, each character outside ASCII letters and digits a hyphen) over
/// `entrypoints`, with the engine's lexicon sources and the Lean toolchain
/// pin `toolchain`, stating `limits`: the workspace project and each
/// library project are written by this one function.
fn config(
    prefix: &str,
    name: &str,
    entrypoints: &[String],
    toolchain: &[u8],
    limits: &crate::limits::Limits,
) -> Vec<(String, Vec<u8>)> {
    let toml_name: String = name
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
         {}",
        entrypoints
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", "),
        crate::limits::Limits::table_text(&limits.lexlean)
    );
    let lakefile = format!(
        "name = \"{prefix}\"\nversion = \"0.1.0\"\ndefaultTargets = [\"{prefix}\"]\n\n[[lean_lib]]\nname = \"{prefix}\"\n"
    );
    let manifest = format!(
        "{{\"version\":\"1.2.0\",\"packagesDir\":\".lake/packages\",\"packages\":[],\"name\":\"{prefix}\",\"lakeDir\":\".lake\",\"fixedToolchain\":false}}\n"
    );
    let lock = format!(
        "spec = \"lexlean/lock/2\"\nlanguage = \"1.2\"\n\
         compiler_semantics = \"{}\"\n\
         project_config_sha256 = \"{}\"\nlean_toolchain = \"leanprover/lean4:v4.32.1\"\n\n\
         [[workspace_file]]\npath = \"lake-manifest.json\"\nsha256 = \"{}\"\n\n\
         [[workspace_file]]\npath = \"lakefile.toml\"\nsha256 = \"{}\"\n\n\
         [[workspace_file]]\npath = \"lean-toolchain\"\nsha256 = \"{}\"\n\n{}",
        compiler_semantics(),
        sha(lexlean.as_bytes()),
        sha(manifest.as_bytes()),
        sha(lakefile.as_bytes()),
        sha(toolchain),
        crate::assets::LOCK_PACKAGES
    );
    let prismpm = format!(
        "spec = \"prismpm/project/1\"\nproject = \"{prefix}\"\nlexlean_project = \"lexlean.toml\"\n\
         build_root = \".prism\"\n\n{}",
        crate::limits::Limits::table_text(&limits.prismpm)
    );
    vec![
        (LEXLEAN.into(), lexlean.into_bytes()),
        ("lexlean.lock".into(), lock.into_bytes()),
        ("lakefile.toml".into(), lakefile.into_bytes()),
        ("lake-manifest.json".into(), manifest.into_bytes()),
        ("prismpm.toml".into(), prismpm.into_bytes()),
        (".gitattributes".into(), b"* -text\n".to_vec()),
        (
            ".gitignore".into(),
            b"/.lexlean/\n/.prism/\n/.lake/\n".to_vec(),
        ),
    ]
}

/// The LexLean compiler semantics a projection locks to: the one UORust's own
/// self model is locked to (`app/lexlean.lock`, which `lexlean lock` in the
/// locked SDK rewrites), so a projection follows the SDK's compiler without a
/// copied identity.
fn compiler_semantics() -> &'static str {
    const SELF_LOCK: &str = include_str!("../../../app/lexlean.lock");
    SELF_LOCK
        .lines()
        .find_map(|line| line.strip_prefix("compiler_semantics = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("app/lexlean.lock names its compiler semantics")
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

/// Resolve every call to workspace functions: by the language's own name
/// resolution where the written code decides it ([`crate::names`]), and by
/// name where those rules could not follow the call.
#[allow(clippy::type_complexity)]
fn resolve(
    ctx: &Model,
    resolver: &crate::names::Resolver<'_>,
) -> (
    BTreeMap<FnRef, BTreeSet<FnRef>>,
    BTreeMap<FnRef, BTreeMap<String, Meant>>,
    Vec<Refusal>,
) {
    use crate::names::Outcome;
    let mut calls = BTreeMap::new();
    let mut named: BTreeMap<FnRef, BTreeMap<String, Meant>> = BTreeMap::new();
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
    let fn_ref = |(pkg, file, idx): crate::names::FnId| FnRef { pkg, file, idx };
    for &caller in &all {
        let f = ctx.func(caller);
        let mut out = BTreeSet::new();
        let ruled: Vec<(String, Option<Outcome>)> =
            match resolver.calls((caller.pkg, caller.file, caller.idx)) {
                Some(list) => list.into_iter().map(|(c, o)| (c, Some(o))).collect(),
                None => f.calls.iter().map(|c| (c.clone(), None)).collect(),
            };
        for (call, outcome) in &ruled {
            // Product code's functions only, as by name below.
            let product = |fns: &[crate::names::FnId]| -> Vec<FnRef> {
                let mut v: Vec<FnRef> = fns.iter().map(|g| fn_ref(*g)).collect();
                v.retain(|r| ctx.product(*r));
                v
            };
            if outcome.is_some() {
                let one = match outcome {
                    Some(Outcome::Resolved(fns)) => match product(fns).as_slice() {
                        [only] => Meant::Fn(*only),
                        _ => Meant::Open,
                    },
                    Some(Outcome::Outside) => Meant::Outside,
                    _ => Meant::Open,
                };
                let reading = named.entry(caller).or_default();
                let agreed = match reading.get(call) {
                    Some(seen) if *seen != one => Meant::Open,
                    _ => one,
                };
                reading.insert(call.clone(), agreed);
            }
            match outcome {
                // One function, or the variants of one no build holds
                // together: the call reaches each.
                Some(Outcome::Resolved(fns)) => {
                    out.extend(product(fns));
                    continue;
                }
                Some(Outcome::Outside) => continue,
                // The written code leaves it open between these.
                Some(Outcome::Open(fns)) => {
                    match product(fns).as_slice() {
                        [] => {}
                        [only] => {
                            out.insert(*only);
                        }
                        many => {
                            let mut names: Vec<String> = many
                                .iter()
                                .map(|r| {
                                    format!("{} ({})", ctx.path(*r), ctx.func(*r).span.render())
                                })
                                .collect();
                            names.sort();
                            refusals.push(Refusal::AmbiguousCall {
                                span: f.span.clone(),
                                callee: call.clone(),
                                candidates: names,
                            });
                        }
                    }
                    continue;
                }
                Some(Outcome::Unknown) | None => {}
            }
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
    (calls, named, refusals)
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
            ctx.name,
            ws.packages.len()
        )
    } else {
        format!(
            "Maintain the {} workspace: {}.",
            ctx.name,
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
                format!("Listed as an author of the {} workspace.", ctx.name),
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
        let module = ctx.module_of(r);
        let parts = [
            package.name.as_str(),
            target.name.as_str(),
            module.as_str(),
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
            .entry((r.pkg, file.target, ctx.module_of(*r)))
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
        let module = ctx.module_of(*r);
        let path = if module.is_empty() {
            target.replace('-', "_")
        } else {
            format!("{}::{module}", target.replace('-', "_"))
        };
        by_module
            .entry((component.clone(), module, path))
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
        // A long list is balanced appends of short literals, so the
        // linker's JSON reader meets a shallow term however many sites a
        // control lists.
        let items: Vec<String> = r
            .evidence
            .iter()
            .map(|(kind, reference)| {
                record(
                    &format!("{tm}Evidence"),
                    &[
                        ("kind", format!("(ctor {tm}EvidenceKind.{kind})")),
                        ("reference", string(&truncate(reference, 256))),
                    ],
                )
            })
            .collect();
        let evidence = crate::embed::term_list(&items, &format!("{tm}Evidence"));
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

/// The Quality facet. Beside what it counts in the workspace, it states the
/// lift and the admission as measures of the model: the functions of product
/// code and those of them that lifted, from `lift` (the register this run
/// writes), and the open refusals, `admitted`'s and its own. Each is a
/// measured count under its own requirement, as every other measure here is;
/// none is a threshold the tool chose.
fn quality(
    ctx: &Model,
    features: usize,
    lift: &Register,
    admitted: &BTreeSet<Refusal>,
) -> (Module, Vec<Refusal>) {
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
    // The refusals the frontier will count: the other facets' and the ones
    // found above, each once.
    let open_refusals = admitted
        .iter()
        .chain(&refusals)
        .collect::<BTreeSet<_>>()
        .len();
    let lift = lift.totals();
    // (characteristic, subcharacteristic, measure, threshold)
    let mut measures: Vec<(&str, &str, &str, usize)> = vec![
        (
            "FunctionalSuitability",
            "FunctionalCompleteness",
            "AdmittedFeatures",
            features,
        ),
        (
            "FunctionalSuitability",
            "FunctionalCompleteness",
            "OpenRefusals",
            open_refusals,
        ),
        (
            "FunctionalSuitability",
            "FunctionalCompleteness",
            "ProductFunctions",
            lift.functions,
        ),
        (
            "FunctionalSuitability",
            "FunctionalCompleteness",
            "LiftedFunctions",
            lift.lifted,
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
    resolver: &crate::names::Resolver<'_>,
    notes: &mut Notes,
    mir: Option<(&crate::mir::Dumps, &crate::mir::MirOptions)>,
) -> (Module, Option<String>) {
    let ws = &ctx.ws;
    let mut decls = Vec::new();
    // Each crate's type names that two of its modules declare (an inline
    // module is a module), whether or not the lift reads both. A file reads
    // such a name only by its own module scope's declaration or `use`
    // (`overlays`), and a path written to one (`crate::inner::P`,
    // `super::P`) not at all: the lift reads a type path by its last segment
    // in the reader's scope, which names the type the path names only where
    // that scope happens to bind the name to it (`written_through_paths`).
    let shared: Vec<BTreeSet<String>> = ws
        .packages
        .iter()
        .map(|package| {
            let mut at: BTreeMap<&str, BTreeSet<(usize, String)>> = BTreeMap::new();
            for file in &package.files {
                if !matches!(
                    package.targets[file.target].kind,
                    TargetKind::Lib | TargetKind::Bin
                ) {
                    continue;
                }
                let enums = file.enums.iter().map(|e| (&e.name, e.scope));
                let structs = file.structs.iter().map(|st| (&st.name, st.scope));
                for (name, scope) in enums.chain(structs) {
                    let module = (file.target, file.item_module(scope));
                    at.entry(name.as_str()).or_default().insert(module);
                }
            }
            at.into_iter()
                .filter(|(_, modules)| modules.len() > 1)
                .map(|(name, _)| name.to_string())
                .collect()
        })
        .collect();
    // A type of product code is keyed by its full path in its crate: its
    // target, its module (an inline module among them) and its name. A key
    // declared twice (once per configuration) is refused; one name in two
    // modules or two crates is two types, each lifting under the shortest
    // name that tells it apart.
    type Key = (usize, String, String);
    /// A file and one of its module scopes.
    type Scope = (usize, usize);
    /// A struct's fields as its declaration writes them.
    type Written = Vec<(String, crate::ast::RustType)>;
    /// The lifted kinds of one crate's types, by key, each struct and data
    /// enum with the file and the inline module that declare it, which its
    /// fields are read in.
    #[derive(Default)]
    struct Declared {
        enums: BTreeMap<Key, Vec<String>>,
        structs: BTreeMap<Key, (Scope, Written)>,
        datas: BTreeMap<Key, (Scope, lift::RawVariants)>,
        spans: BTreeMap<Key, Span>,
        /// The names of the keys declared twice, which name no one type.
        twice: BTreeSet<String>,
    }
    let mut declared: Vec<Declared> = Vec::new();
    for package in &ws.packages {
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
                let inline = syntax.names.type_module(&file.rel, e.span.line, &e.name);
                let key = (
                    file.target,
                    crate::names::join(&file.module, inline),
                    e.name.clone(),
                );
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
                            let fs = fs.iter().map(|(n, t)| (n.clone(), t.clone())).collect();
                            (v.clone(), fs)
                        })
                        .collect();
                    d.datas.insert(key, ((fi, e.scope), variants));
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
                let inline = syntax.names.type_module(&file.rel, st.span.line, &st.name);
                let key = (
                    file.target,
                    crate::names::join(&file.module, inline),
                    st.name.clone(),
                );
                if d.spans.insert(key.clone(), st.span.clone()).is_some() {
                    clashing.insert(key);
                    notes.refusals.insert(Refusal::Collision {
                        span: st.span.clone(),
                        name: qualified(BEHAVIOR, &st.name),
                    });
                    continue;
                }
                let fields = fields.iter().map(|(n, t)| (n.clone(), t.clone())).collect();
                d.structs.insert(key, ((fi, st.scope), fields));
            }
        }
        for key in &clashing {
            d.enums.remove(key);
            d.structs.remove(key);
            d.datas.remove(key);
            d.twice.insert(key.2.clone());
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
    for (pkg, (d, package)) in declared.iter().zip(&ws.packages).enumerate() {
        let ks = keys(d);
        let mut in_crate: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, _, name) in &ks {
            *in_crate.entry(name.as_str()).or_default() += 1;
        }
        let named = ks
            .iter()
            .map(|key| {
                let (target, module, name) = key;
                // A name LexLean writes for a type of its own (`Prop`) is
                // shared with it as with another crate's type.
                let taken = lift::name_taken(name, !d.enums.contains_key(key));
                let apart = shared[pkg].contains(name);
                let lex = if everywhere[name] == 1 && !taken && !apart {
                    name.clone()
                } else if in_crate[name.as_str()] == 1 && !apart {
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
        ambiguous.push(shared[pkg].clone());
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
    // Where each LexLean name of a type is declared: a type construct names
    // the owners of its chain's places by these.
    for (d, named) in declared.iter().zip(&lexes) {
        for (key, lex) in named {
            if let Some(span) = d.spans.get(key) {
                notes
                    .lexed
                    .insert(lex.clone(), (span.clone(), *lex != key.2));
            }
        }
    }
    // Alias identities include their declaration scope. Their bodies are
    // normalized before expansion, so importing another homonym at a use
    // site cannot rebind an element introduced by the alias.
    let hex = |text: &str| {
        text.as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let alias_key = |pkg: usize, target: usize, module: &str, name: &str| {
        format!("@alias_{pkg}_{target}_{}_{}", hex(module), hex(name))
    };
    let frozen = |lex: &str| format!("{}{}", lift::FROZEN_TYPE_PREFIX, hex(lex));
    let frozen_names: BTreeMap<String, String> = lexes
        .iter()
        .flat_map(|named| named.values().map(|lex| (frozen(lex), lex.clone())))
        .collect();
    let mut alias_definitions = BTreeMap::new();
    let mut ambiguous_aliases = BTreeSet::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            for (name, alias, scope) in &syntax.files[&file.rel].aliases {
                let key = alias_key(pkg, file.target, &file.item_module(*scope), name);
                if alias_definitions
                    .insert(key.clone(), (pkg, fi, *scope, name.clone(), alias.clone()))
                    .is_some()
                {
                    ambiguous_aliases.insert(key);
                }
            }
        }
    }
    let declared_name = |reading: &crate::names::TypeReading| -> Option<String> {
        match reading {
            crate::names::TypeReading::Declared(pkg, target, module, name) => lexes
                .get(*pkg)
                .and_then(|named| named.get(&(*target, module.clone(), name.clone())))
                .cloned()
                .or_else(|| {
                    let key = alias_key(*pkg, *target, module, name);
                    alias_definitions.contains_key(&key).then_some(key)
                }),
            crate::names::TypeReading::Extern(path) => Some(path.clone()),
            crate::names::TypeReading::Open => None,
        }
    };
    // A resolved declaration is an identity, not a new bare identifier for
    // a later registry/glob overlay to resolve in a different scope.
    let frozen_declared_name = |reading: &crate::names::TypeReading| -> Option<String> {
        match reading {
            crate::names::TypeReading::Declared(p, t, m, n) => lexes
                .get(*p)
                .and_then(|named| named.get(&(*t, m.clone(), n.clone())))
                .map(|lex| frozen(lex))
                .or_else(|| declared_name(reading)),
            _ => None,
        }
    };
    let resolved_aliases: BTreeMap<String, crate::ast::Alias> = alias_definitions
        .iter()
        .map(|(key, (pkg, fi, scope, name, alias))| {
            let file = &ws.packages[*pkg].files[*fi];
            let inline = file
                .scopes
                .get(*scope)
                .map(|at| at.module.clone())
                .unwrap_or_default();
            let names: BTreeMap<String, String> = ctx
                .type_readings
                .get(&(*pkg, *fi, inline.clone()))
                .into_iter()
                .flatten()
                .filter(|(written, _)| !alias.params.contains(written))
                .filter_map(|(written, reading)| {
                    let to = frozen_declared_name(reading);
                    to.map(|to| (written.clone(), to))
                })
                .collect();
            let to = alias.to.renamed(&names).localized(&file.imports);
            let to = ctx
                .registry_paths
                .get(&(*pkg, *fi, inline))
                .map_or_else(|| to.clone(), |bound| to.renamed(bound));
            let to = if lift::PRIMITIVE_TYPES.contains(&name.as_str())
                || ambiguous_aliases.contains(key)
            {
                crate::ast::RustType {
                    text: if lift::PRIMITIVE_TYPES.contains(&name.as_str()) {
                        format!("{name}, a primitive type's name an alias takes")
                    } else {
                        format!("{name}, an alias declared more than once")
                    },
                    kind: crate::ast::TypeKind::Other,
                }
            } else {
                to
            };
            (
                key.clone(),
                crate::ast::Alias {
                    params: alias.params.clone(),
                    to,
                },
            )
        })
        .collect();
    let type_aliases: Vec<BTreeMap<String, crate::ast::Alias>> = ws
        .packages
        .iter()
        .enumerate()
        .map(|(pkg, _)| {
            let mut aliases = resolved_aliases.clone();
            // Preserve the existing conservative rule for primitive-name aliases.
            for (key, (owner, _, _, name, _)) in &alias_definitions {
                if *owner == pkg && lift::PRIMITIVE_TYPES.contains(&name.as_str()) {
                    aliases.insert(name.clone(), resolved_aliases[key].clone());
                }
            }
            aliases.extend(
                ctx.crate_aliases
                    .iter()
                    .filter(|(name, _)| {
                        !ctx.readable
                            .get(&pkg)
                            .is_some_and(|paths| paths.contains(*name))
                    })
                    .map(|(name, alias)| (name.clone(), alias.clone())),
            );
            aliases
        })
        .collect();
    // A chain of aliases that does not repeat one is at most as long as
    // the crate has aliases.
    let expanded = |pkg: usize, ty: &crate::ast::RustType| {
        ty.expanded(&type_aliases[pkg], type_aliases[pkg].len())
    };
    // A type as the file that writes it names it, a field's or a
    // signature's: a bare name its `use` binds to a registry crate's item
    // is that item's path, so the chain through a field ends at the
    // crate's type, by its crate, and an alias the crate declares reads as
    // what it stands for.
    let inline_of = |r: FnRef| {
        ctx.file(r)
            .scopes
            .get(ctx.func(r).scope)
            .map(|scope| scope.module.clone())
            .unwrap_or_default()
    };
    let declared_in = |pkg: usize, fi: usize, inline: &str, ty: &crate::ast::RustType| {
        let file = &ws.packages[pkg].files[fi];
        let ty = ty.localized(&file.imports);
        match ctx.registry_paths.get(&(pkg, fi, inline.to_string())) {
            Some(bound) => expanded(pkg, &ty.renamed(bound)),
            None => expanded(pkg, &ty),
        }
    };
    // Each module scope's reading of its crate's shared names: the type
    // its module declares or one of its own `use` items names, and
    // otherwise none. An inline module is a scope of its own: Rust gives it
    // neither the types nor the `use` bindings of the module around it, and
    // gives the module around it none of its own.
    let mut overlays: BTreeMap<(usize, usize, usize), BTreeMap<String, String>> = BTreeMap::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            for scope in 0..file.scopes.len().max(1) {
                let inline = file
                    .scopes
                    .get(scope)
                    .map(|s| s.module.clone())
                    .unwrap_or_default();
                let readings = ctx.type_readings.get(&(pkg, fi, inline));
                let paths = readings
                    .into_iter()
                    .flatten()
                    .filter(|(n, reading)| {
                        n.contains("::")
                            || matches!(reading, crate::names::TypeReading::Declared(..))
                    })
                    .map(|(n, _)| n);
                let mut names: BTreeMap<String, String> = ambiguous[pkg]
                    .iter()
                    .chain(paths)
                    .map(|n| {
                        let read = readings.and_then(|r| r.get(n)).and_then(|reading| {
                            frozen_declared_name(reading).or_else(|| declared_name(reading))
                        });
                        (n.clone(), read.unwrap_or_else(|| n.clone()))
                    })
                    .collect();
                if let Some(bound) = ctx.registered.get(&(pkg, fi)) {
                    names.extend(bound.clone());
                }
                overlays.insert((pkg, fi, scope), names);
            }
        }
    }
    let mut native_shapes = Vec::new();
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
            aliases.extend(frozen_names.clone());
            // The registered crates' types whose reading the package's lock
            // admits, each naming itself (`Types::registered`).
            for path in ctx.readable.get(&pkg).into_iter().flatten().cloned() {
                aliases.insert(path.clone(), path);
            }
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
            // A field's type is read in the module scope that declares it.
            let mut structs: Fields = d
                .structs
                .into_iter()
                .map(|(key, ((fi, scope), fs))| {
                    let names = &overlays[&(pkg, fi, scope)];
                    let inline = package.files[fi].scopes[scope].module.as_str();
                    let fs = fs
                        .into_iter()
                        .map(|(n, t)| (n, declared_in(pkg, fi, inline, &t.renamed(names))))
                        .collect();
                    (named[&key].clone(), fs)
                })
                .collect();
            let datas: Datas = d
                .datas
                .into_iter()
                .map(|(key, ((fi, scope), vs))| {
                    let names = &overlays[&(pkg, fi, scope)];
                    let inline = package.files[fi].scopes[scope].module.as_str();
                    let vs = vs
                        .into_iter()
                        .map(|(v, fs)| {
                            (
                                v,
                                fs.into_iter()
                                    .map(|(n, t)| {
                                        (n, declared_in(pkg, fi, inline, &t.renamed(names)))
                                    })
                                    .collect(),
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
                    let enums = file.enums.iter().map(|e| (&e.name, e.scope, &e.derives));
                    let structs = file
                        .structs
                        .iter()
                        .map(|st| (&st.name, st.scope, &st.derives));
                    enums
                        .chain(structs)
                        .filter_map(|(n, scope, ds)| {
                            let key = (file.target, file.item_module(scope), n.clone());
                            let lex = named.get(&key)?;
                            Some((lex.clone(), ds.iter().cloned().collect()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            // A name of the crate that reads as no one type is registered
            // with the reason, so that a row it stops states it: a key
            // declared twice, and a name several of the crate's types
            // share where a file does not say which it means.
            let unread = d
                .twice
                .iter()
                .map(|n| (n, lift::DECLARED_TWICE))
                .chain(ambiguous[pkg].iter().map(|n| (n, lift::SHARED_NAME)));
            for (name, why) in unread {
                let free = !enums.contains_key(name) && !datas.contains_key(name);
                if free && !structs.contains_key(name) {
                    structs.insert(name.clone(), vec![lift::unread_shape(why)]);
                }
            }
            native_shapes.push(structs.clone());
            Types {
                derives,
                source_aliases: std::sync::Arc::new(type_aliases[pkg].clone()),
                ..Types::with_data(enums, &datas, &structs, aliases)
            }
        })
        .collect();
    // Each module scope's types: its crate's, with the crate's shared names
    // read as the scope reads them.
    let file_scopes: BTreeMap<(usize, usize, usize), Types> = overlays
        .iter()
        .map(|(&at, names)| {
            let mut scope = scopes[at.0].clone();
            let inline = ws.packages[at.0].files[at.1]
                .scopes
                .get(at.2)
                .map(|scope| scope.module.clone())
                .unwrap_or_default();
            if let Some(bound) = ctx.registry_paths.get(&(at.0, at.1, inline)) {
                scope.aliases.extend(bound.clone());
            }
            scope.aliases.extend(names.clone());
            (at, scope)
        })
        .collect();
    // A function that writes a path to one of its crate's shared type names
    // reads that name as no type, in its signature and its body alike.
    let blind_of = |r: FnRef| -> BTreeSet<String> {
        let f = ctx.func(r);
        let mut out = written_through_paths(&f.tokens, &shared[r.pkg]);
        if let Some(owner) = &f.owner {
            out.extend(written_through_paths(owner, &shared[r.pkg]));
        }
        out
    };
    let fn_aliases = |r: FnRef| -> BTreeMap<String, String> {
        let at = (r.pkg, r.file, ctx.func(r).scope);
        let inline = inline_of(r);
        let mut names = ctx
            .registry_paths
            .get(&(r.pkg, r.file, inline))
            .cloned()
            .unwrap_or_default();
        names.extend(blinded(&overlays[&at], Some(&blind_of(r))));
        // A function's own type parameters shadow module-level imports.
        for parameter in &syntax.function(&ctx.file(r).rel, r.idx).signature.generics {
            if let crate::ast::GenericParam::Type(name) = parameter {
                names.remove(name);
            }
        }
        names
    };
    let fn_scopes: BTreeMap<FnRef, Types> = ctx
        .all_fns()
        .into_iter()
        .filter(|&r| !blind_of(r).is_empty())
        .map(|r| {
            let mut scope = file_scopes[&(r.pkg, r.file, ctx.func(r).scope)].clone();
            scope.aliases.extend(fn_aliases(r));
            (r, scope)
        })
        .collect();
    let types_of = |r: FnRef| -> &Types {
        fn_scopes
            .get(&r)
            .unwrap_or(&file_scopes[&(r.pkg, r.file, ctx.func(r).scope)])
    };
    // Each target's scope for the MIR path, which prints a type of the
    // crate by its full path: `project::Model`.
    let mut mir_scopes: BTreeMap<(usize, usize), Types> = BTreeMap::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for target in 0..package.targets.len() {
            let mut scope = scopes[pkg].clone();
            // Ordinary workspaces read the same primitive IEEE formats as
            // source lifting; core-library scopes already carry their own.
            for primitive in ["f32", "f64"] {
                let declared = lexes[pkg].keys().any(|(t, module, name)| {
                    *t == target && module.is_empty() && name == primitive
                });
                if !declared {
                    if let Some(format) = scope.float(primitive) {
                        scope.floats.insert(primitive.to_string(), format);
                    }
                }
            }
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
            crate::mir::views::bind(&mut scope, &native_shapes[pkg]);
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
                    type_parameters: Vec::new(),
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
    let sites = value_sites(ctx, syntax);
    let homonyms = homonym_fns(ctx, &sites);
    let reads = Reads {
        ctx,
        resolver,
        homonyms: &homonyms,
        sites: &sites,
    };
    // The method each type's impl of a trait of its crate gives, per crate:
    // what a call of a generic function passes for the trait's dictionary.
    let mut impl_index: Vec<BTreeMap<(String, String, String), String>> =
        ws.packages.iter().map(|_| BTreeMap::new()).collect();
    let mut scoped: Vec<(Vec<Candidate<'_>>, BTreeMap<String, FnRef>)> = ws
        .packages
        .iter()
        .map(|_| (Vec::new(), BTreeMap::new()))
        .collect();
    // How many functions of product code each path names, and how many
    // methods each type has of each name over its crate. Rust reads
    // `Type::name` and `value.name()` as the type's one method of that name,
    // an inherent one before a trait's.
    let mut pathed: BTreeMap<String, usize> = BTreeMap::new();
    let mut methods: BTreeMap<(usize, String, String), usize> = BTreeMap::new();
    for &r in all.iter().filter(|&&r| ctx.product(r)) {
        *pathed.entry(ctx.path(r)).or_default() += 1;
        let f = ctx.func(r);
        if let (Some(o), false) = (&f.owner, f.in_trait) {
            let owner = types_of(r).resolve(o);
            *methods.entry((r.pkg, owner, f.name.clone())).or_default() += 1;
        }
    }
    // The trait methods a call can mean, each with its crate, the key such a
    // call reads and its own key; and the candidates that give way.
    let mut reach: Vec<(usize, String, String, FnRef)> = Vec::new();
    let mut yielding: BTreeMap<(usize, String), u8> = BTreeMap::new();
    for &r in &all {
        let f = ctx.func(r);
        let types = types_of(r);
        if !ctx.product(r) {
            continue;
        }
        // Methods of a type the lift names lift with `self` as their first
        // parameter, inherent ones and a trait implementation's alike;
        // methods of other types stop, each with a row of its own that says
        // why its type is outside the lift.
        let known = |o: &String| {
            types.enums.contains_key(&types.resolve(o))
                || types.structs.contains_key(&types.resolve(o))
                || types.datas.contains_key(&types.resolve(o))
        };
        // Every type of the signature outside the lift stops the function,
        // whichever the lift meets first: the register holds them all, so
        // that a later register can show that one of them no longer stops
        // it ([`register::admit_parent`]).
        let stops = notes.stops.entry(ctx.path(r)).or_default();
        let fn_sig = &syntax.function(&ctx.file(r).rel, r.idx).signature;
        let written = fn_sig
            .inputs
            .iter()
            .filter_map(|i| match i {
                crate::ast::FnArg::Typed(t) => Some(&t.ty),
                crate::ast::FnArg::Receiver { .. } => None,
            })
            .chain(fn_sig.output.iter());
        for ty in written {
            let ty = expanded(r.pkg, &ty.localized(&ctx.file(r).imports));
            if let Err(reason @ Residue::Type(_)) = types.ty(&ty) {
                let counted = ctx.counted(r, &reason);
                if !stops.contains(&counted) {
                    stops.push(counted);
                }
            }
        }
        // A path Rust's own naming leaves to two trait methods (an impl or
        // a trait declared once per configuration) is one row, and names
        // neither of them.
        if (f.implements.is_some() || f.in_trait) && pathed[&ctx.path(r)] > 1 {
            ctx.stopped(notes, &effectful, r, Residue::Collision(ctx.path(r)));
            continue;
        }
        let fn_syntax = syntax.function(&ctx.file(r).rel, r.idx);
        // `Display::fmt` lifts as the function from the value to the text
        // it writes: `write!` into its formatter is that `format!`.
        let display = matches!(
            (&f.owner, &f.implements),
            (Some(o), Some(t)) if t == "Display" && f.name == "fmt" && !f.in_trait && known(o)
        );
        let method = match (&f.owner, &f.implements) {
            (None, _) => None,
            // A method a trait declares: its default body, where it is one
            // function for every implementing type.
            (Some(declarer), _) if f.in_trait => {
                let refusal = match &fn_syntax.block {
                    // No definition of its own: each implementing type's
                    // method is one, and a generic function over the trait
                    // takes it in its dictionary.
                    None => Some(Residue::Signature(format!(
                        "a method {declarer} declares without a body, which each implementing \
                         type gives: an entry of {declarer}'s dictionary, with no definition of \
                         its own"
                    ))),
                    Some(block) => default_refusal(ctx.file(r), f, declarer, block),
                };
                if let Some(reason) = refusal {
                    ctx.stopped(notes, &effectful, r, reason);
                    continue;
                }
                None
            }
            (Some(o), Some(_)) if display => Some(types.resolve(o)),
            (Some(o), implements) => {
                // A trait method over a registry crate's types, such as a
                // `syn` visitor, is the converter shell's: a foreign row.
                let foreign = implements.as_ref().and_then(|_| {
                    let sig = &fn_syntax.signature;
                    sig.inputs
                        .iter()
                        .filter_map(|i| match i {
                            crate::ast::FnArg::Typed(p) => Some(p.ty.text.clone()),
                            crate::ast::FnArg::Receiver { .. } => None,
                        })
                        .chain(sig.output.iter().map(|o| o.text.clone()))
                        .find(|ty| foreign_crate(&ctx.ws.packages[r.pkg], ty).is_some())
                });
                if let Some(ty) = foreign {
                    ctx.stopped(notes, &effectful, r, Residue::Type(ty));
                    continue;
                }
                // An impl with parameters of its own is one impl for each
                // type or constant that takes their place, and its methods
                // are generic as a function with parameters is.
                if implements.is_some() && !f.impl_params.is_empty() {
                    let reason = Residue::Signature(format!(
                        "a method of an impl generic in {}: one function for each type or \
                         constant that takes its place",
                        f.impl_params.join(", ")
                    ));
                    ctx.stopped(notes, &effectful, r, reason);
                    continue;
                }
                if known(o) {
                    Some(types.resolve(o))
                } else if foreign_crate(&ctx.ws.packages[r.pkg], o).is_some() {
                    ctx.stopped(notes, &effectful, r, Residue::Type(o.clone()));
                    continue;
                } else {
                    // The type and why the lift does not name it: a type
                    // the workspace registers by the chain down to its
                    // leaf, as a parameter of the type is, and any other by
                    // what the crate's source shows. The leaf decides the
                    // row: a registry crate's type there makes it foreign.
                    let reason = if types.outside.contains_key(&types.resolve(o)) {
                        types.unnamed(o)
                    } else {
                        let why =
                            unlifted_type(ctx, syntax, &file_scopes, &expanded, r.pkg, r.file, o);
                        Residue::Type(format!("{o}, {why}"))
                    };
                    ctx.stopped(notes, &effectful, r, reason);
                    continue;
                }
            }
        };
        let mut sig = fn_syntax.signature.clone();
        let signature_names = fn_aliases(r);
        for input in &mut sig.inputs {
            if let crate::ast::FnArg::Typed(t) = input {
                t.ty =
                    t.ty.renamed(&signature_names)
                        .localized(&ctx.file(r).imports);
            }
        }
        if let Some(t) = &mut sig.output {
            *t = t.renamed(&signature_names).localized(&ctx.file(r).imports);
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
        let site = (r.pkg, ctx.file(r).target, ctx.module_of(r), f.name.clone());
        // The key a call written `Type::name` or `value.name()` reads, where
        // this is a trait's method such a call can mean.
        let mut reached = None;
        let (rust, name) = match (&method, &f.owner, homonyms.get(&site)) {
            (Some(o), _, _) => {
                let plain = (
                    format!("{o}::{}", f.name),
                    lift::local_name(&format!("{o}_{}", f.name)),
                );
                let sole = methods[&(r.pkg, o.clone(), f.name.clone())] == 1;
                match &f.trait_ref {
                    None => plain,
                    // The text a value displays as keeps the key and the
                    // name it had: no call reads it, `write!` and `format!`
                    // do.
                    Some(_) if display && sole => plain,
                    // A trait's method lifts under a key no call writes,
                    // and each scope that sees the trait reads the call's
                    // key as it (`trait_in_scope`). Where its type has
                    // another method of the name, Rust names this one by
                    // its trait and no call is read as it. Nor where the
                    // trait is generic: it is implemented once per argument
                    // list (`From<A>` beside `From<B>` and core's own
                    // `From<T> for T`), and Rust picks among them by the
                    // types at the call. Its definition is named by its
                    // type and its trait whatever else its type has, so the
                    // name is one for each trait of a type and each type of
                    // a trait, and a method the type gains later leaves it.
                    Some(t) => {
                        if sole && !t.contains('<') {
                            reached = Some(plain.0);
                        }
                        (
                            format!("{o}::<{t}>::{}", f.name),
                            lift::local_name(&format!("{o}_{t}_{}", f.name)),
                        )
                    }
                }
            }
            // A default method lifts under its trait's name, and under a
            // key no call writes: a call means an implementing type's.
            (None, Some(declarer), _) => (
                format!("<{declarer}>::{}", f.name),
                lift::local_name(&format!("{declarer}_{}", f.name)),
            ),
            (None, None, Some((rust, lex))) => (rust.clone(), lex.clone()),
            (None, None, None) => (f.name.clone(), lift::local_name(&f.name)),
        };
        // What a call of the function by itself is written as: a free
        // function's name, a method's key, and a trait's method as its own
        // scope reads it.
        let called = match (&method, &f.owner, &reached) {
            (None, None, _) => f.name.clone(),
            (_, _, Some(plain)) if trait_in_scope(ctx, r, r) => plain.clone(),
            _ => rust.clone(),
        };
        if let Some(plain) = reached {
            reach.push((r.pkg, plain, rust.clone(), r));
        }
        // Who keeps a name two functions lift to, whichever the projection
        // reaches first: a free function before a type's own method
        // (`sample_cases` beside `Sample::cases`), and that before a
        // trait's method.
        let yields: u8 = if f.in_trait || (f.implements.is_some() && !display) {
            2
        } else {
            u8::from(method.is_some())
        };
        let owner = match (&method, &f.owner) {
            (Some(o), _) => Some(lift::Owner::Type {
                name: o,
                receiver: f.receiver.as_deref(),
            }),
            (None, Some(_)) => Some(lift::Owner::Implementor),
            (None, None) => None,
        };
        // A generic function reads its type parameters first.
        let generic = match generic_reading(ctx, syntax, &file_scopes, types, r, sig) {
            Ok(generic) => generic,
            Err(reason) => {
                ctx.stopped(notes, &effectful, r, reason);
                continue;
            }
        };
        let generic_scope = generic.as_ref().map(|g| {
            let mut scope = types.clone();
            scope.generic = g.reads.clone();
            scope
        });
        let read_sig = generic.as_ref().map(|g| {
            let mut read = sig.clone();
            read.generics.retain(
                |p| !matches!(p, crate::ast::GenericParam::Type(n) if g.reads.contains_key(n)),
            );
            read
        });
        let (types, sig) = match (&generic_scope, &read_sig) {
            (Some(t), Some(s)) => (t, s),
            _ => (types, sig),
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
            _ => types.signature(sig, owner.as_ref()),
        };
        // A Display impl's formatter is the text it writes, not a borrow
        // the definition threads.
        let threads = match formatter {
            Some(_) => Vec::new(),
            None => lift::threads(sig, owner.as_ref()),
        };
        match signature {
            Ok((params, result)) => {
                let holder = candidates
                    .iter()
                    .position(|c: &Candidate<'_>| c.name == name)
                    .filter(|&i| {
                        !names.contains_key(&rust)
                            && yielding
                                .get(&(r.pkg, candidates[i].rust.clone()))
                                .is_some_and(|held| yields < *held)
                    });
                if let Some(held) = holder.and_then(|i| names.remove(&candidates[i].rust)) {
                    // The name's holder gives way, and keeps its row.
                    candidates.retain(|c| c.name != name);
                    notes.refusals.insert(Refusal::Collision {
                        span: ctx.func(held).span.clone(),
                        name: qualified(BEHAVIOR, &name),
                    });
                    ctx.stopped(notes, &effectful, held, Residue::Collision(name.clone()));
                } else if names.contains_key(&rust)
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
                yielding.insert((r.pkg, rust.clone()), yields);
                names.insert(rust.clone(), r);
                // The trait an impl at its file's own scope implements, as
                // that scope reads the trait's path.
                if let (Some(o), Some(path), 0) = (&method, &f.trait_path, f.scope) {
                    if let Some((_, declarer)) = declared_trait(ctx, r.pkg, r.file, path) {
                        impl_index[r.pkg]
                            .insert((declarer, o.clone(), f.name.clone()), rust.clone());
                    }
                }
                candidates.push(Candidate {
                    generic,
                    rust,
                    name,
                    params,
                    result,
                    block,
                    imports: ctx.file(r).imports.clone(),
                    seen: ctx.seen(r, f.scope),
                    aliases: fn_aliases(r),
                    homonyms: reads.call_reading(r),
                    called,
                    formatter,
                    threads,
                });
            }
            Err(reason) => ctx.stopped(notes, &effectful, r, reason),
        }
    }
    // Constants lift as definitions of no parameters. A module that still
    // declares a constant twice holds two variants the configuration does
    // not decide between (`names::stated` has dropped every variant it
    // compiles out): neither is the constant, so neither lifts, and a
    // reader of the name binds none.
    let mut constant_scopes = BTreeMap::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            for (rust, _, _, scope, _, _) in &syntax.files[&file.rel].consts {
                if let Some((owner, _)) = rust.rsplit_once("::") {
                    constant_scopes
                        .entry((pkg, fi, *scope, owner.to_string()))
                        .or_insert_with(|| {
                            crate::mir::lower::constant_scope(
                                &file_scopes[&(pkg, fi, *scope)],
                                owner,
                            )
                        });
                }
            }
        }
    }
    let constant_types = |pkg, fi, scope, rust: &str| {
        rust.rsplit_once("::")
            .and_then(|(owner, _)| constant_scopes.get(&(pkg, fi, scope, owner.to_string())))
            .unwrap_or(&file_scopes[&(pkg, fi, scope)])
    };
    let mut declared: BTreeMap<FnSite, usize> = BTreeMap::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for file in &package.files {
            for (rust, _, _, scope, _, _) in syntax
                .files
                .get(&file.rel)
                .map_or(&[][..], |f| &f.consts[..])
            {
                let site = (pkg, file.target, file.item_module(*scope), rust.clone());
                *declared.entry(site).or_default() += 1;
            }
        }
    }
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            if !matches!(
                package.targets[file.target].kind,
                TargetKind::Lib | TargetKind::Bin
            ) {
                continue;
            }
            for (rust, ty, block, scope, tokens, _) in &syntax.files[&file.rel].consts {
                let site = (pkg, file.target, file.item_module(*scope), rust.clone());
                if declared.get(&site).is_some_and(|n| *n > 1) {
                    continue;
                }
                let blind = written_through_paths(tokens, &shared[pkg]);
                let base = constant_types(pkg, fi, *scope, rust);
                let mut aliases = blinded(&overlays[&(pkg, fi, *scope)], Some(&blind));
                let owner = rust.rsplit_once("::").map(|(owner, _)| owner);
                if owner.is_some() {
                    aliases.insert("Self".into(), base.resolve("Self"));
                }
                let mut blinded_types = None;
                if !blind.is_empty() {
                    let mut scope = base.clone();
                    scope.aliases.extend(aliases.clone());
                    blinded_types = Some(scope);
                }
                let types = blinded_types.as_ref().unwrap_or(base);
                let Ok(result) = types.ty(ty) else {
                    continue;
                };
                let (candidates, names) = &mut scoped[pkg];
                // A name two modules of the crate declare lifts under its
                // module's key and name, and each reader reads the one its
                // own module means (`Reads::scope_reading`).
                let (key, spelled) = match homonyms.get(&site) {
                    Some((key, lex)) => (key.clone(), lex.clone()),
                    None => (rust.clone(), rust.clone()),
                };
                let plain = lift::local_name(&spelled);
                // A constant spelled like a function, `REVERSE` beside
                // `reverse`, takes its kind as a suffix.
                let name = if candidates.iter().any(|c: &Candidate<'_>| c.name == plain) {
                    lift::local_name(&format!("{spelled}_CONST"))
                } else {
                    plain
                };
                if names.contains_key(&key)
                    || candidates
                        .iter()
                        .any(|c: &Candidate<'_>| c.rust == key || c.name == name)
                {
                    continue;
                }
                let mut imports = file.imports.clone();
                if let Some(owner) = owner {
                    imports.insert("Self".into(), owner.into());
                }
                candidates.push(Candidate {
                    generic: None,
                    rust: key,
                    name,
                    params: Vec::new(),
                    result,
                    block,
                    imports,
                    // A constant's scope is read as one that may see any
                    // trait: no method of its value is read as inherent.
                    seen: lift::Seen {
                        outside_traits: true,
                        macro_items: true,
                        paths_open: true,
                        ..lift::Seen::default()
                    },
                    aliases,
                    homonyms: reads.scope_reading(
                        (pkg, file.target, &file.item_module(*scope)),
                        &identifiers(tokens),
                    ),
                    called: rust.clone(),
                    formatter: None,
                    threads: Vec::new(),
                });
            }
        }
    }
    // Each candidate reads a call's key as the trait's method where its
    // scope sees the trait, and as no function where it does not. A
    // constant's value calls no trait's method.
    for (pkg, (candidates, names)) in scoped.iter_mut().enumerate() {
        for c in candidates.iter_mut() {
            let caller = names.get(&c.rust).copied();
            for (_, plain, key, imp) in reach.iter().filter(|(p, ..)| *p == pkg) {
                let seen = caller.is_some_and(|caller| trait_in_scope(ctx, caller, *imp));
                c.homonyms.insert(plain.clone(), seen.then(|| key.clone()));
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
            // A dictionary's parameter likewise.
            for e in c.generic.iter_mut().flat_map(|g| g.dictionary.iter_mut()) {
                let base = e.name.clone();
                let mut k = 1;
                while defined.contains(&e.name) || taken.contains(&e.name) {
                    k += 1;
                    e.name = format!("{base}Method{k}");
                }
                taken.insert(e.name.clone());
            }
        }
    }
    let mut lifted = Vec::new();
    let mut mentioned: BTreeSet<Ty> = BTreeSet::new();
    let mut ordered: BTreeSet<Ty> = BTreeSet::new();
    // Per crate: the lifted functions' signatures and their axioms.
    let mut results: Vec<(Sigs, Axioms)> = Vec::new();
    let mut model_sigs: Vec<Sigs> = Vec::new();
    // Every lifted function by its crate path, for the MIR path's calls.
    let mut callees: BTreeMap<String, crate::mir::lower::Callee> = BTreeMap::new();
    let every_name: BTreeSet<String> = scoped
        .iter()
        .flat_map(|(candidates, _)| candidates.iter().map(|c| c.name.clone()))
        .collect();
    for (((candidates, names), types), impls) in scoped.iter().zip(&scopes).zip(&impl_index) {
        let widths = lift::pointer_widths(&ctx.ws.targets);
        let mut types = types.clone();
        types.impls = impls.clone();
        let types = &types;
        let out = lift::lift_all(candidates, types, &widths, &every_name);
        for (rust, reason) in out.failed {
            if let Some(&r) = names.get(&rust) {
                let reason = match reason {
                    Residue::Call(callee) => {
                        Residue::Call(callee_path(ctx, &file_scopes, names, r, &callee))
                    }
                    other => other,
                };
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
                // A path has one row and one κ. Two functions share a path
                // where two traits give one type a method of one name (the
                // `fmt` of `Display` and of `Debug`): where the other did
                // not lift, the path's row stays the one that says so,
                // since the κ the register holds for the path may be
                // either function's.
                let path = ctx.path(r);
                if notes.residue.contains_key(&path)
                    || notes.foreign.contains_key(&path)
                    || notes.effects.contains_key(&path)
                {
                    continue;
                }
                notes.lifted.insert(
                    path,
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
                        generic: c.generic.clone(),
                        threads: c.threads.clone(),
                    },
                )
            })
            .collect();
        // Compiler calls reuse the source binder for type arguments and
        // coherent dictionaries rather than duplicating generic semantics.
        let exact: Sigs = sigs
            .iter()
            .filter(|(key, _)| out.divergences.get(*key).is_none_or(Vec::is_empty))
            .map(|(key, sig)| (key.clone(), sig.clone()))
            .collect();
        for c in candidates
            .iter()
            .filter(|c| c.generic.is_none() || exact.contains_key(&c.rust))
        {
            if let (true, Some(&r)) = (
                out.decls.iter().any(|d| d.name() == c.name),
                names.get(&c.rust),
            ) {
                callees.insert(
                    ctx.path(r),
                    crate::mir::lower::Callee {
                        name: c.name.clone(),
                        outcomes: false,
                        memory: false,
                        method: compiler_method(ctx, resolver, r),
                        params: c.params.iter().map(|(_, _, t)| t.clone()).collect(),
                        generic: c.generic.clone(),
                        // A definition that threads its `&mut` parameters
                        // returns its own result first, as a translated
                        // one does.
                        result: c.forward(),
                        threads: (!c.threads.is_empty())
                            .then(|| (c.result.clone(), c.threads.clone())),
                    },
                );
            }
        }
        lifted.extend(out.decls);
        model_sigs.push(exact);
        results.push((sigs, out.axioms));
    }
    let mut native_generics: BTreeMap<FnRef, lift::Generic> = scoped
        .iter()
        .flat_map(|(candidates, names)| {
            candidates
                .iter()
                .filter_map(|c| Some((*names.get(&c.rust)?, c.generic.clone()?)))
        })
        .collect();
    // A trait bound need not be interpreted to transport its parameter.
    // Unresolved parametric dispatch remains refused in native lowering.
    for r in ctx.all_fns() {
        if native_generics.contains_key(&r) {
            continue;
        }
        let sig = &syntax.function(&ctx.file(r).rel, r.idx).signature;
        if sig
            .generics
            .iter()
            .any(|p| matches!(p, crate::ast::GenericParam::Const(_)))
        {
            continue;
        }
        let names = generic_type_names(sig, types_of(r));
        if names.is_empty() {
            continue;
        }
        let parameters = sig
            .generics
            .iter()
            .filter_map(|p| match p {
                crate::ast::GenericParam::Type(n) => Some(names[n].clone()),
                _ => None,
            })
            .collect();
        native_generics.insert(
            r,
            lift::Generic {
                types: parameters,
                reads: names
                    .into_iter()
                    .map(|(rust, lex)| (rust, Ty::Param(lex)))
                    .collect(),
                dictionary: Vec::new(),
            },
        );
    }
    // Reuse actual, unambiguous source constants only after source lifting
    // established their exact definitions. The native path imports these
    // meanings; it neither evaluates a second initializer nor mistakes a
    // zero-parameter function for a value. Conflicting target identities
    // cannot acquire one meaning merely because rustc prints the same path.
    let mut constant_bindings: BTreeMap<String, Option<crate::mir::lower::Constant>> =
        BTreeMap::new();
    for (pkg, package) in ws.packages.iter().enumerate() {
        for (fi, file) in package.files.iter().enumerate() {
            for (rust, written, _, scope, _, _) in &syntax.files[&file.rel].consts {
                let module = file.item_module(*scope);
                let site = (pkg, file.target, module.clone(), rust.clone());
                let krate = package.targets[file.target].name.replace('-', "_");
                let path = if module.is_empty() {
                    format!("{krate}::{rust}")
                } else {
                    format!("{krate}::{module}::{rust}")
                };
                let binding = (|| {
                    if declared.get(&site) != Some(&1) {
                        return None;
                    }
                    let key = homonyms.get(&site).map_or(rust, |(key, _)| key);
                    let sig = model_sigs[pkg].get(key).filter(|sig| {
                        sig.params.is_empty() && sig.generic.is_none() && sig.threads.is_empty()
                    })?;
                    let types = constant_types(pkg, fi, *scope, rust);
                    if !crate::mir::lower::constant_interface(&written.text, &sig.result, types) {
                        return None;
                    }
                    Some(crate::mir::lower::Constant {
                        name: sig.name.clone(),
                        result: sig.result.clone(),
                    })
                })();
                constant_bindings
                    .entry(path)
                    .and_modify(|value| *value = None)
                    .or_insert(binding);
            }
        }
    }
    let constants = constant_bindings
        .into_iter()
        .filter_map(|(path, value)| Some((path, value?)))
        .collect();
    // The MIR path, for every function the surface lift left as residue.
    let mut listing = None;
    let mut translated = Vec::new();
    if let Some((dumps, options)) = mir {
        // A native body may call a source definition without embedding
        // that definition in its own text. Bind these shared meanings,
        // type schemas and imported assets before consulting old verdicts.
        let proof_context = sha(serde_json::json!({
            "spec":"uorust/native-proof-context/2",
            "proof_strategy":"reachable-declarations/1",
            "sdk":include_str!("../../../prismpm.lock"),
            "assets":crate::assets::FIXED,
            "source":lifted.iter().map(crate::lex::render_decl).collect::<Vec<_>>(),
            "types":format!("{mir_scopes:?}"),
            "limits":format!("{:?}",ctx.limits),
        })
        .to_string()
        .as_bytes());
        // The selected dictionary member appears in the translated term and
        // its dependency identity. Unused impl entries do not invalidate an
        // otherwise identical native proof context.
        for ((pkg, _), scope) in &mut mir_scopes {
            scope.impls = impl_index[*pkg].clone();
        }
        let mut taken = every_name.clone();
        taken.extend(lifted.iter().map(|d| d.name().to_string()));
        taken.extend(types.enums.keys().cloned());
        taken.extend(types.structs.keys().cloned());
        taken.extend(types.datas.keys().cloned());
        let out = mir_lift(
            ctx,
            resolver,
            notes,
            dumps,
            options,
            &mir_scopes,
            &model_sigs,
            &native_generics,
            &lifted,
            &types,
            &callees,
            &constants,
            &taken,
            &homonyms,
            &proof_context,
        );
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
    // The raw pointer and the memory, where a translation reaches memory:
    // the MIR path's own structures (see `mir_lift`'s scopes).
    let mut with_memory = types.clone();
    for (name, fields) in [
        (
            crate::mir::lower::POINTER.to_string(),
            crate::mir::lower::pointer_fields(),
        ),
        (
            crate::mir::memory::MEMORY.to_string(),
            crate::mir::memory::memory_fields(),
        ),
    ] {
        if mentioned.contains(&Ty::Named(name.clone())) {
            with_memory.structs.entry(name).or_insert(fields);
        }
    }
    for ty in &mentioned {
        type_decls(ty, &with_memory, &compared, &mut emitted, &mut decls);
    }
    // The derived orders the definitions compare by, after the types.
    if !ordered.is_empty() {
        decls.extend(crate::order::tests());
        decls.push(crate::order::reverse());
        decls.extend(crate::order::decls(&ordered, &types));
    }
    decls.extend(surface_dependency_order(lifted));
    // Source and native lowering can require the same shared comparison.
    // Retain differing declarations so the SDK still rejects a collision.
    let shared: BTreeMap<_, _> = decls
        .iter()
        .map(|d| (d.name().to_string(), crate::lex::render_decl(d)))
        .collect();
    decls.extend(
        translated
            .into_iter()
            .filter(|d| shared.get(d.name()) != Some(&crate::lex::render_decl(d))),
    );
    // Closed assertions of tests over lifted functions are theorems.
    let mut theorems = BTreeSet::new();
    for &r in &all {
        let f = ctx.func(r);
        if !f.test {
            continue;
        }
        // Assertions use the same resolved callees as their Rust scope.
        // Preserve the written path as the lookup key, including associated
        // owners and dependency aliases, and carry that callee's axioms.
        let mut accepted_sigs = BTreeMap::new();
        let mut accepted_axioms = BTreeMap::new();
        for (written, meant) in ctx.named.get(&r).into_iter().flatten() {
            let Meant::Fn(callee) = meant else {
                continue;
            };
            let Some((_, definition, _)) = notes.lifted.get(&ctx.path(*callee)) else {
                continue;
            };
            let Some(name) = definition.strip_prefix(&qualified(BEHAVIOR, "")) else {
                continue;
            };
            let (sigs, axioms) = &results[callee.pkg];
            if let Some((key, sig)) = sigs.iter().find(|(_, sig)| sig.name == name) {
                accepted_sigs.insert(written.clone(), sig.clone());
                accepted_axioms.insert(
                    written.clone(),
                    axioms.get(key).cloned().unwrap_or_default(),
                );
            }
        }
        for (i, (kind, args)) in syntax
            .function(&ctx.file(r).rel, r.idx)
            .asserts
            .iter()
            .enumerate()
        {
            if let Some(accepted) = lift::acceptance(kind, args, &accepted_sigs, types_of(r)) {
                let name = format!("accept{}{}", pascal(&[&ctx.package(r).name, &f.name]), i);
                if theorems.insert(name.clone()) {
                    let axioms = accepted_axioms
                        .get(&accepted.callee)
                        .cloned()
                        .unwrap_or_default();
                    // The acceptance root: the claim as a closed Boolean
                    // definition, which a native library lists and its
                    // generated package executes (SPEC 12.6).
                    let root = format!("{name}Root");
                    notes
                        .roots
                        .entry(ctx.package(r).name.clone())
                        .or_default()
                        .push(root.clone());
                    decls.push(Decl::Definition {
                        type_parameters: Vec::new(),
                        name: root.clone(),
                        parameters: Vec::new(),
                        result: "Bool".to_string(),
                        body: Expr::Flat(accepted.check),
                        axioms: axioms.clone(),
                        recursive: None,
                        termination: None,
                        mutual: None,
                    });
                    decls.push(Decl::Theorem {
                        type_parameters: Vec::new(),
                        name: format!("{root}Holds"),
                        parameters: Vec::new(),
                        statement: format!("(({root}) = true)"),
                        proof: "decide{}".to_string(),
                        axioms: axioms.clone(),
                    });
                    decls.push(Decl::Theorem {
                        type_parameters: Vec::new(),
                        name,
                        parameters: Vec::new(),
                        statement: accepted.statement,
                        proof: "decide{}".to_string(),
                        axioms,
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
            // Source carriers may name types imported by native lowering.
            // Order the merged graph, not just its independently built layers.
            decls: surface_dependency_order(decls),
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
    model_context_sha256: Option<String>,
}

/// The MIR path: every function the surface lift left as residue, whose
/// body the dump holds, translated from MIR. A translation is admitted when
/// the verdict register records it proved (or, on the prover's trial run,
/// when it has no verdict yet) and every translation it calls is admitted;
/// every other function's residue row names the MIR kind or the obligation
/// that stopped it.
/// Bind the implementation's identity through the same source resolver that
/// reads its types and traits; method spelling alone supplies no provider.
fn compiler_method(
    ctx: &Model,
    resolver: &crate::names::Resolver<'_>,
    r: FnRef,
) -> Option<crate::mir::identity::Method> {
    let f = ctx.func(r);
    let method = crate::mir::identity::Method::new(
        f.owner.as_ref()?,
        f.trait_ref.as_ref()?,
        &f.name,
        |text| resolver.source_type_identity((r.pkg, r.file, r.idx), text),
    )?;
    use quote::ToTokens;
    let Ok(signature) = syn::parse_str::<syn::Signature>(&f.signature) else {
        return Some(method);
    };
    let inputs = signature
        .inputs
        .iter()
        .map(|p| match p {
            syn::FnArg::Receiver(receiver) => match &receiver.kind {
                syn::ReceiverKind::Value => "Self".into(),
                syn::ReceiverKind::Reference(_, _, mutable) => if mutable.is_some() {
                    "&mut Self"
                } else {
                    "&Self"
                }
                .into(),
                syn::ReceiverKind::Typed(_, ty) => ty.to_token_stream().to_string(),
                _ => String::new(),
            },
            syn::FnArg::Typed(p) => p.ty.to_token_stream().to_string(),
        })
        .collect::<Vec<_>>();
    let result = match signature.output {
        syn::ReturnType::Default => "()".into(),
        syn::ReturnType::Type(_, t) => t.to_token_stream().to_string(),
    };
    Some(method.signature(&inputs, &result, |text| {
        resolver.source_type_identity((r.pkg, r.file, r.idx), text)
    }))
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn mir_lift(
    ctx: &Model,
    resolver: &crate::names::Resolver<'_>,
    notes: &mut Notes,
    dumps: &crate::mir::Dumps,
    options: &crate::mir::MirOptions,
    scopes: &BTreeMap<(usize, usize), Types>,
    source_sigs: &[Sigs],
    generics: &BTreeMap<FnRef, lift::Generic>,
    source_models: &[Decl],
    source_types: &Types,
    lifted: &BTreeMap<String, crate::mir::lower::Callee>,
    constants: &BTreeMap<String, crate::mir::lower::Constant>,
    taken: &BTreeSet<String>,
    homonyms: &BTreeMap<FnSite, (String, String)>,
    proof_context: &str,
) -> MirOut {
    use crate::mir::lower;
    use crate::mir::verdict::Decision;
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
    // MIR is a compilation artifact: its recorded compiler target owns its
    // ABI. Without that evidence, retain the conservative declared-target view.
    let layouts: Vec<crate::mir::layout::DataLayout> = if dumps.source_width().is_some() {
        dumps
            .layouts
            .iter()
            .filter(|l| Some(&l.target) == dumps.target.as_ref())
            .cloned()
            .collect()
    } else if !ctx.ws.targets.is_empty()
        && ctx
            .ws
            .targets
            .iter()
            .all(|t| dumps.layouts.iter().any(|l| &l.target == t))
    {
        dumps
            .layouts
            .iter()
            .filter(|l| ctx.ws.targets.contains(&l.target))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let reserved: BTreeSet<_> = source_types
        .structs
        .keys()
        .chain(source_types.enums.keys())
        .chain(source_types.datas.keys())
        .cloned()
        .collect();
    let scopes: BTreeMap<(usize, usize), Types> = scopes
        .iter()
        .map(|(&at, s)| {
            let mut scope = Types {
                closures: signatures.clone(),
                ..s.clone()
            };
            // Where the targets' layouts are known, a raw pointer and the
            // memory it points into are declared, as in core's scope.
            if !layouts.is_empty() {
                scope
                    .structs
                    .entry(lower::POINTER.to_string())
                    .or_insert_with(lower::pointer_fields);
                scope
                    .structs
                    .entry(crate::mir::memory::MEMORY.to_string())
                    .or_insert_with(crate::mir::memory::memory_fields);
            }
            let krate = ctx.ws.packages[at.0].targets[at.1].name.replace('-', "_");
            let mut requested: Vec<_> = dumps
                .crates
                .get(&krate)
                .into_iter()
                .flat_map(|bodies| bodies.values())
                .flat_map(|body| body.locals.iter().cloned())
                .collect();
            requested.extend(
                dumps
                    .closures
                    .get(&krate)
                    .into_iter()
                    .flat_map(|bodies| bodies.values())
                    .flat_map(|body| body.locals.iter().cloned()),
            );
            crate::mir::core::import_standard_schemas(&mut scope, &requested, &reserved);
            (at, scope)
        })
        .collect();
    let original_types = source_types;
    let mut effective_types = source_types.clone();
    for scope in scopes.values() {
        effective_types.structs.extend(scope.structs.clone());
        effective_types.enums.extend(scope.enums.clone());
        effective_types.datas.extend(scope.datas.clone());
        effective_types.borrowing.extend(scope.borrowing.clone());
    }
    let source_types = &effective_types;
    let generic_scopes: BTreeMap<FnRef, Types> = generics
        .iter()
        .map(|(&r, g)| {
            let mut scope = scopes[&(r.pkg, ctx.file(r).target)].clone();
            scope.generic = g.reads.clone();
            (r, scope)
        })
        .collect();
    let base_scope = |r: FnRef| {
        generic_scopes
            .get(&r)
            .unwrap_or(&scopes[&(r.pkg, ctx.file(r).target)])
    };
    let definition_keys = embedding_keys(ctx);
    let body_of = |r: FnRef| {
        let krate = ctx.crate_ident(r);
        let path = ctx.path(r);
        let prefix = format!("{krate}::");
        let local = path.strip_prefix(&prefix).unwrap_or(&path);
        dumps
            .crates
            .get(&krate)
            .and_then(|c| c.get(local))
            .or_else(|| {
                let mut key = definition_keys.get(&path)?.clone();
                if let Some((file, _, _)) = &mut key.impl_at {
                    file.clone_from(&ctx.file(r).rel);
                }
                dumps.definitions.get(&key)
            })
    };
    // Intern inferred source frames: equal constraints share one immutable type
    // environment instead of cloning the complete workspace for every body.
    let mut flow_environments = BTreeMap::new();
    let mut flow_frames = BTreeMap::new();
    let mut flow_errors = BTreeMap::new();
    for r in ctx
        .all_fns()
        .into_iter()
        .filter(|&r| ctx.product(r) && notes.residue.contains_key(&ctx.path(r)))
    {
        let Some(body) = body_of(r) else {
            continue;
        };
        let original = base_scope(r);
        let aliases = crate::mir::flow::aliases(body, original);
        let specialized =
            (!aliases.is_empty()).then(|| crate::mir::flow::specialize(original, &aliases));
        let base = specialized.as_ref().unwrap_or(original);
        match crate::mir::flow::infer(body, base) {
            Ok(inferred) if !inferred.is_empty() || !aliases.is_empty() => {
                let bindings = crate::mir::flow::bindings(body, base, &inferred);
                if bindings != original.generic || !aliases.is_empty() {
                    let key = (r.pkg, ctx.file(r).target, bindings, aliases);
                    flow_environments
                        .entry(key.clone())
                        .or_insert_with(|| Types {
                            generic: key.2.clone(),
                            ..base.clone()
                        });
                    flow_frames.insert(r, key);
                }
            }
            Err(reason) => {
                flow_errors.insert(r, reason);
            }
            _ => {}
        }
    }
    let scope = |r: FnRef| {
        flow_frames
            .get(&r)
            .and_then(|key| flow_environments.get(key))
            .unwrap_or_else(|| base_scope(r))
    };
    let widths = match (dumps.source_width(), dumps.target.as_ref()) {
        (Some(width), Some(target)) => vec![(width, vec![target.clone()])],
        _ => lift::pointer_widths(&ctx.ws.targets),
    };
    // Enums whose variants set their discriminants, by LexLean name.
    let mut explicit: BTreeSet<String> = BTreeSet::new();
    for (pkg, package) in ctx.ws.packages.iter().enumerate() {
        for file in &package.files {
            for e in file.enums.iter().filter(|e| e.explicit) {
                let module = file.item_module(e.scope);
                let printed = if module.is_empty() {
                    e.name.clone()
                } else {
                    format!("{module}::{}", e.name)
                };
                explicit.insert(scopes[&(pkg, file.target)].resolve(&printed));
            }
        }
    }
    // Reserve method homonyms from the source, independently of which MIR
    // files this invocation selected. A source identity qualifies every member
    // of an ambiguous family; adding another dump cannot steal a proved name.
    let base_name = |r: FnRef| {
        let f = ctx.func(r);
        let site = (r.pkg, ctx.file(r).target, ctx.module_of(r), f.name.clone());
        match (&f.owner, homonyms.get(&site)) {
            (Some(owner), _) => lift::local_name(&format!("{owner}_{}", f.name)),
            (None, Some((_, lex))) => lex.clone(),
            (None, None) => lift::local_name(&f.name),
        }
    };
    let mut name_counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in ctx.all_fns().into_iter().filter(|&r| ctx.product(r)) {
        *name_counts.entry(base_name(r)).or_default() += 1;
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
        let f = ctx.func(r);
        if let Some(reason) = flow_errors.get(&r) {
            notes
                .residue
                .insert(path, (f.span.clone(), mir_residue(reason.clone())));
            continue;
        }
        let body = body_of(r);
        let Some(body) = body else {
            continue;
        };
        let base = base_name(r);
        let name = if name_counts[&base] > 1 || names.contains(&base) {
            let kappa = crate::register::kappa(&path, &ctx.func(r).tokens);
            lift::local_name(&format!("{base}_{kappa}"))
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
                let mut generic = generics.get(&r).cloned().unwrap_or(lift::Generic {
                    types: Vec::new(),
                    reads: BTreeMap::new(),
                    dictionary: Vec::new(),
                });
                for parameter in params
                    .iter()
                    .chain(std::iter::once(&result))
                    .flat_map(Ty::parameters)
                {
                    if !generic.types.contains(&parameter) {
                        generic.types.push(parameter);
                    }
                }
                callees.insert(
                    path.clone(),
                    lower::Callee {
                        name: name.clone(),
                        outcomes: false,
                        memory: false,
                        method: compiler_method(ctx, resolver, r),
                        params,
                        generic: (!generic.types.is_empty() || !generic.dictionary.is_empty())
                            .then_some(generic),
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
    // Compiler-generated bodies have their own closed crate/target frame.
    // Registered source functions keep their existing provider and identity.
    let registered: BTreeSet<_> = ctx
        .all_fns()
        .into_iter()
        .filter_map(|r| {
            let mut key = definition_keys.get(&ctx.path(r))?.clone();
            if let Some((file, _, _)) = &mut key.impl_at {
                file.clone_from(&ctx.file(r).rel);
            }
            Some(key)
        })
        .collect();
    let no_consts = BTreeMap::new();
    let mut compiler_definitions = BTreeMap::new();
    let mut definition_frames = BTreeMap::new();
    let mut providers = Vec::new();
    for (key, definition) in &dumps.compiler_identities {
        if definition.parametric || registered.contains(key) {
            continue;
        }
        let Some(body) = dumps.definitions.get(key) else {
            continue;
        };
        let targets: Vec<_> = ctx
            .ws
            .packages
            .iter()
            .enumerate()
            .flat_map(|(pkg, p)| {
                p.targets.iter().enumerate().filter_map(move |(target, t)| {
                    (t.name.replace('-', "_") == key.krate).then_some((pkg, target))
                })
            })
            .collect();
        let [at] = targets.as_slice() else {
            continue;
        };
        let Some((method_name, owner, reference)) =
            crate::mir::identity::parts(&definition.identity)
        else {
            continue;
        };
        let Some(method) =
            crate::mir::identity::Method::new(&owner, &reference, &method_name, |text| {
                resolver.compiler_definition_type_identity(at.0, at.1, text)
            })
        else {
            continue;
        };
        let method = method.signature(&body.locals[1..=body.arity], &body.locals[0], |text| {
            resolver.compiler_definition_type_identity(at.0, at.1, text)
        });
        let identity = format!(
            "{}:{}:{}:{}",
            key.krate, body.path, definition.identity, definition.body_sha256
        );
        providers.push((*at, key.krate.clone(), body, identity, Some(method)));
    }
    // An admitted module path names a free function only in the target that
    // included it. Registered definitions retain their existing provider.
    for (pkg, package) in ctx.ws.packages.iter().enumerate() {
        for ((target, path), source_hash) in &package.path_functions {
            let krate = package.targets[*target].name.replace('-', "_");
            let target_count = ctx
                .ws
                .packages
                .iter()
                .flat_map(|p| &p.targets)
                .filter(|t| t.name.replace('-', "_") == krate)
                .count();
            let head = path.split("::").next().unwrap_or_default();
            if target_count != 1
                || matches!(head, "std" | "core" | "alloc")
                || package
                    .deps
                    .iter()
                    .any(|d| d.rename.as_ref().unwrap_or(&d.name).replace('-', "_") == head)
                || product.contains_key(&format!("{krate}::{path}"))
            {
                continue;
            }
            let Some(hash) = dumps.free_body_hashes.get(&(krate.clone(), path.clone())) else {
                continue;
            };
            let Some(body) = dumps.crates.get(&krate).and_then(|rows| rows.get(path)) else {
                continue;
            };
            if body.path != *path || !scopes.contains_key(&(pkg, *target)) {
                continue;
            }
            providers.push((
                (pkg, *target),
                krate.clone(),
                body,
                format!("{krate}:{path}:{source_hash}:{hash}"),
                None,
            ));
        }
    }
    let mut direct_paths = BTreeMap::new();
    for (at, krate, body, identity, method) in providers {
        let types = &scopes[&at];
        let signature: Result<_, Residue> = (|| {
            let params: Vec<_> = (1..=body.arity)
                .map(|i| {
                    let text = &body.locals[i];
                    lower::ty(lower::mut_target(text).unwrap_or(text), types)
                })
                .collect::<Result<_, Residue>>()?;
            let result = lower::ty(&body.locals[0], types)?;
            if params
                .iter()
                .chain(std::iter::once(&result))
                .any(|t| !t.parameters().is_empty())
            {
                return Err(Residue::Type("open compiler dependency interface".into()));
            }
            let positions: Vec<_> = lower::threaded(&body.locals, body.arity, false, types)
                .into_iter()
                .map(|i| i - 1)
                .collect();
            let rest = positions
                .iter()
                .map(|&i| params[i].clone())
                .collect::<Vec<_>>();
            let full = lower::threaded_result(&result, &rest, types);
            Ok((params, result, positions, full))
        })();
        let Ok((params, result, positions, full)) = signature else {
            continue;
        };
        let name = format!("compilerBody{}", sha(identity.as_bytes()));
        if !names.insert(name.clone()) {
            continue;
        }
        if method.is_none() {
            direct_paths.insert((at, body.path.as_str()), name.clone());
        }
        callees.insert(
            name.clone(),
            lower::Callee {
                name: name.clone(),
                outcomes: false,
                memory: false,
                method,
                params,
                result,
                generic: None,
                threads: (!positions.is_empty()).then_some((full, positions)),
            },
        );
        definition_frames.insert(name.clone(), (at, crate::mir::identity::references(body)));
        compiler_definitions.insert(
            name,
            lower::CompilerDefinition {
                body,
                types,
                source_sigs: &source_sigs[at.0],
                consts: dumps.consts.get(&krate).unwrap_or(&no_consts),
                krate,
            },
        );
    }
    // The source question owns its Rust generic scope. Compiler spellings
    // from another function must never supply that scope implicitly.
    let frame = |r: FnRef| crate::register::kappa(&ctx.path(r), &ctx.func(r).tokens);
    let mut requests: BTreeMap<String, (FnRef, BTreeSet<String>)> = todo
        .iter()
        .map(|(r, _, body)| (frame(*r), (*r, crate::mir::identity::references(body))))
        .collect();
    let mut owners: BTreeMap<crate::embedding::Key, Option<FnRef>> = BTreeMap::new();
    for r in ctx.all_fns().into_iter().filter(|r| ctx.product(*r)) {
        if let Some(mut key) = definition_keys.get(&ctx.path(r)).cloned() {
            if let Some((file, _, _)) = &mut key.impl_at {
                file.clone_from(&ctx.file(r).rel);
            }
            owners
                .entry(key)
                .and_modify(|held| *held = None)
                .or_insert(Some(r));
        }
    }
    let mut closure_scopes = BTreeMap::new();
    for (krate, closures) in &dumps.closures {
        for (closure, body) in closures {
            let parent = body.path.split("::{closure").next().unwrap_or(&body.path);
            let owner = crate::embedding::body_key(krate, parent)
                .and_then(|key| owners.get(&key).copied().flatten());
            if let Some(r) = owner {
                let key = frame(r);
                requests
                    .entry(key.clone())
                    .or_insert_with(|| (r, BTreeSet::new()))
                    .1
                    .extend(crate::mir::identity::references(body));
                closure_scopes.insert(closure.clone(), key);
            }
        }
    }
    let index = crate::mir::identity::Index::new(&callees);
    let mut compiler_bindings = crate::mir::identity::Bindings::new();
    let mut dictionary_frames = BTreeMap::new();
    for (scope, (r, requests)) in requests {
        let mut imports = BTreeMap::new();
        let mut dictionary = lower::DictionaryFrame {
            entries: generics
                .get(&r)
                .map(|g| g.dictionary.clone())
                .unwrap_or_default(),
            calls: BTreeMap::new(),
        };
        for request in &requests {
            let Some((method, owner, reference)) = crate::mir::identity::parts(request) else {
                continue;
            };
            let Ok(Ty::Param(parameter)) = lower::ty(&owner, base_scope(r)) else {
                continue;
            };
            let Some(trait_identity) =
                resolver.compiler_type_identity((r.pkg, r.file, r.idx), &reference)
            else {
                continue;
            };
            let matching: Vec<_> = dictionary
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.param == parameter
                        && entry.method == method
                        && resolver
                            .source_type_identity((r.pkg, r.file, r.idx), &entry.declarer)
                            .is_some_and(|source| source.matches(&trait_identity))
                })
                .map(|(i, _)| i)
                .collect();
            if let [entry] = matching.as_slice() {
                dictionary.calls.insert(request.clone(), *entry);
            }
        }
        if !dictionary.entries.is_empty() {
            dictionary_frames.insert(scope.clone(), dictionary);
        }

        for request in requests {
            if let Some(callee) = direct_paths
                .get(&((r.pkg, ctx.file(r).target), request.as_str()))
                .and_then(|n| callees.get(n))
                .or_else(|| {
                    index.resolve(&request, |text| {
                        resolver.compiler_type_identity((r.pkg, r.file, r.idx), text)
                    })
                })
            {
                imports.insert(request, callee.clone());
            }
        }
        compiler_bindings.insert(scope, imports);
    }
    for (scope, ((pkg, target), requests)) in definition_frames {
        let imports = requests
            .into_iter()
            .filter_map(|request| {
                let callee = direct_paths
                    .get(&((pkg, target), request.as_str()))
                    .and_then(|n| callees.get(n))
                    .or_else(|| {
                        index.resolve(&request, |text| {
                            resolver.compiler_definition_type_identity(pkg, target, text)
                        })
                    })?;
                Some((request, callee.clone()))
            })
            .collect();
        compiler_bindings.insert(scope, imports);
    }
    // The functions the MIR path translates, by LexLean name.
    let mut translations: BTreeMap<String, String> = todo
        .iter()
        .map(|(r, name, _)| (name.clone(), ctx.path(*r)))
        .collect();
    let mut outcome_names = BTreeMap::new();
    for (r, name, _) in &todo {
        let mut view = format!("nativeOutcome{}", sha(name.as_bytes()));
        while !names.insert(view.clone()) {
            view.push_str("Value");
        }
        translations.insert(view.clone(), ctx.path(*r));
        outcome_names.insert(name.clone(), view);
    }
    let dispatch_keys: BTreeMap<_, _> = callees
        .iter()
        .map(|(key, callee)| (callee.name.clone(), key.clone()))
        .collect();
    let dispatchers: BTreeMap<_, _> = scopes
        .keys()
        .map(|&(pkg, target)| {
            let index = &index;
            let keys = &dispatch_keys;
            (
                (pkg, target),
                move |callee: &str, inputs: &[Option<String>], result: &str| {
                    let selected = index.resolve_call(callee, inputs, result, |text| {
                        resolver.compiler_definition_type_identity(pkg, target, text)
                    })?;
                    keys.get(&selected.name).cloned()
                },
            )
        })
        .collect();
    let context = |r: FnRef| lower::Cx {
        types: scope(r),
        source_sigs: &source_sigs[r.pkg],
        widths: &widths,
        source_width: dumps.source_width(),
        callees: &callees,
        compiler_scope: frame(r),
        compiler_definitions: &compiler_definitions,
        compiler_bindings: &compiler_bindings,
        consumer_dispatch: Some(&dispatchers[&(r.pkg, ctx.file(r).target)]),
        dictionary_frames: &dictionary_frames,
        closure_scopes: &closure_scopes,
        constants,
        krate: ctx.crate_ident(r),
        taken: &names,
        explicit: &explicit,
        limit: ctx.limits.max_file_bytes(),
        consts: dumps.consts.get(&ctx.crate_ident(r)).unwrap_or(&no_consts),
        closures: &closures,
        active: &[],
        core: None,
        library: None,
        library_module: String::new(),
        layouts: &layouts,
        backward: false,
    };
    // Lower each body once, then propagate actual memory interfaces through
    // the same native dependency edges. Ordinary interfaces stay byte-identical.
    let translate = |r: FnRef,
                     name: &str,
                     body: &crate::mir::ir::IrBody,
                     cx: &lower::Cx<'_>|
     -> Result<Translation, Residue> {
        let path = ctx.path(r);
        let mut lowered = lower::lower(cx, body, name)?;
        if let Some(generic) = callees.get(&path).and_then(|c| c.generic.as_ref()) {
            crate::lex::bind_parameters(&mut lowered.decls, &mut lowered.aux, &generic.types, name);
        }
        Ok(Translation {
            r,
            path,
            kappa: crate::register::kappa(&ctx.path(r), &ctx.func(r).tokens),
            name: name.into(),
            lowered,
            digest: String::new(),
            model_context_sha256: None,
        })
    };
    let mut done: BTreeMap<String, Translation> = BTreeMap::new();
    let mut recoveries = BTreeMap::new();
    // Carry the successful representation into subsequent ABI and outcome
    // propagation. Only a real lowering residue requests another compiler body.
    for (r, name, body) in &mut todo {
        let cx = context(*r);
        let primary = translate(*r, name, body, &cx);
        let result = primary.or_else(|reason| {
            if let Some((alternate, primary_digest, runtime_digest)) =
                dumps.alternate(ctx.crate_ident(*r), body)
            {
                if let Ok(recovered) = translate(*r, name, alternate, &cx) {
                    recoveries.insert(
                        *r,
                        (
                            mir_residue(reason.clone()).detail(),
                            primary_digest,
                            runtime_digest,
                        ),
                    );
                    *body = alternate;
                    return Ok(recovered);
                }
            }
            Err(reason)
        });
        match result {
            Ok(translation) => {
                done.insert(name.clone(), translation);
            }
            Err(reason) => {
                notes.residue.insert(
                    ctx.path(*r),
                    (ctx.func(*r).span.clone(), mir_residue(reason)),
                );
            }
        }
    }
    // A successful local lowering can still leave a transitive call residue.
    // Compare compiler-provided dependency graphs before propagating ABIs.
    if !dumps.residual_bodies.is_empty() {
        let native = translations.keys().map(String::as_str).collect();
        let mut graph: BTreeMap<_, Vec<Vec<_>>> = done
            .iter()
            .map(|(name, t)| {
                (
                    name.as_str(),
                    vec![t.lowered.calls.iter().map(String::as_str).collect()],
                )
            })
            .collect();
        let closed = crate::mir::residual::closed_providers(&native, &graph);
        let mut alternatives = BTreeMap::new();
        for (r, name, body) in &todo {
            let Some(t) = done.get(name) else { continue };
            if closed.contains_key(name.as_str()) || recoveries.contains_key(r) {
                continue;
            }
            let Some(pair) = dumps.alternate(ctx.crate_ident(*r), body) else {
                continue;
            };
            let Ok(other) = translate(*r, name, &pair.0, &context(*r)) else {
                continue;
            };
            let Some(missing) = t
                .lowered
                .calls
                .iter()
                .find(|c| native.contains(c.as_str()) && !closed.contains_key(c.as_str()))
            else {
                continue;
            };
            alternatives.insert(name.clone(), (other, pair, translations[missing].clone()));
        }
        for (name, (t, _, _)) in &alternatives {
            graph
                .get_mut(name.as_str())
                .expect("known native translation")
                .push(t.lowered.calls.iter().map(String::as_str).collect());
        }
        let selected = crate::mir::residual::closed_providers(&native, &graph)
            .into_iter()
            .filter(|(_, provider)| *provider == 1)
            .map(|(name, _)| name.to_owned())
            .collect::<BTreeSet<_>>();
        for (r, name, body) in &mut todo {
            if selected.contains(name) {
                let (translation, (alternate, primary, runtime), missing) = alternatives
                    .remove(name)
                    .expect("selected compiler provider");
                recoveries.insert(*r,(format!("mir::TerminatorKind::Call: {missing}, whose translation is not admitted"),primary,runtime));
                *body = alternate;
                done.insert(name.clone(), translation);
            }
        }
    }
    let mut native_callees = callees.clone();
    let mut native_bindings = compiler_bindings.clone();
    loop {
        let mut changed = BTreeSet::new();
        for (name, translation) in &done {
            if !translation.lowered.abi.memory {
                continue;
            }
            for callee in native_callees.values_mut().chain(
                native_bindings
                    .values_mut()
                    .flat_map(|bindings| bindings.values_mut()),
            ) {
                if callee.name == *name
                    && callee.bind_memory(&translation.lowered.abi, scope(translation.r))
                {
                    changed.insert(name.clone());
                }
            }
        }
        if changed.is_empty() {
            break;
        }
        let consumers: BTreeSet<_> = done
            .iter()
            .filter(|(_, t)| t.lowered.calls.iter().any(|c| changed.contains(c)))
            .map(|(name, _)| name.clone())
            .collect();
        for (r, name, body) in todo.iter().filter(|(_, name, _)| consumers.contains(name)) {
            let mut cx = context(*r);
            cx.callees = &native_callees;
            cx.compiler_bindings = &native_bindings;
            match translate(*r, name, body, &cx) {
                Ok(translation) => {
                    done.insert(name.clone(), translation);
                }
                Err(reason) => {
                    done.remove(name);
                    notes.residue.insert(
                        ctx.path(*r),
                        (ctx.func(*r).span.clone(), mir_residue(reason)),
                    );
                }
            }
        }
    }
    // Bind transitive native dependencies as well. A lowering change can
    // alter a callee while its stable Rust name and its caller's text stay
    // the same. Sorted closures also cover recursive components.
    // Rebind after a changed semantic ABI, retaining exact normal receipts.
    for view_round in 0..2 {
        for translation in done.values_mut() {
            let text: String = translation
                .lowered
                .aux
                .values()
                .chain(&translation.lowered.decls)
                .map(crate::lex::render_decl)
                .collect();
            translation.digest = sha(text.as_bytes());
            translation.model_context_sha256 = None;
        }
        let digests: BTreeMap<String, String> = done
            .keys()
            .map(|name| {
                let mut pending = vec![name.clone()];
                let mut closure = BTreeSet::new();
                while let Some(next) = pending.pop() {
                    if let Some(body) = done.get(&next) {
                        if closure.insert(next) {
                            pending.extend(
                                body.lowered
                                    .calls
                                    .iter()
                                    .filter(|n| done.contains_key(*n))
                                    .cloned(),
                            );
                        }
                    }
                }
                let dependencies: BTreeMap<_, _> =
                    closure.iter().map(|n| (n, &done[n].digest)).collect();
                let digest = sha(
                    serde_json::json!({"context":proof_context,"dependencies":dependencies})
                        .to_string()
                        .as_bytes(),
                );
                (name.clone(), digest)
            })
            .collect();
        for (name, digest) in digests {
            done.get_mut(&name).expect("translation key").digest = digest;
        }
        let rooted = native_model_contexts(&done, source_models, source_types, lifted, &ctx.limits);
        for (name, model) in rooted {
            done.get_mut(&name)
                .expect("translation key")
                .model_context_sha256 = model;
        }
        if view_round == 1 {
            break;
        }
        let mut selected: BTreeSet<_> = todo
            .iter()
            .filter(|(_, name, body)| {
                let Some(t) = done.get(name) else {
                    return false;
                };
                t.lowered.library_outcomes
                    || body.blocks.iter().any(|block| {
                        matches!(
                            block.terminator,
                            crate::mir::ir::IrTerminator::Assert { .. }
                        )
                    }) && options
                        .verdicts
                        .matching(&t.kappa, &t.digest, t.model_context_sha256.as_deref())
                        .is_some_and(|v| matches!(v.status, Decision::Refused(_)))
            })
            .map(|(_, name, _)| name.clone())
            .collect();
        if selected.is_empty() {
            break;
        }
        // The same native dependency graph owns the changed ABI at every
        // consumer. A caller cannot observe a partial callee as a total one.
        loop {
            let consumers: Vec<_> = done
                .iter()
                .filter(|(name, t)| {
                    !selected.contains(*name)
                        && t.lowered
                            .calls
                            .iter()
                            .any(|callee| selected.contains(callee))
                })
                .map(|(name, _)| name.clone())
                .collect();
            if consumers.is_empty() {
                break;
            }
            selected.extend(consumers);
        }
        let mut outcome_callees = native_callees.clone();
        let mut outcome_bindings = native_bindings.clone();
        for (r, normal, _) in todo.iter().filter(|(_, name, _)| selected.contains(name)) {
            for callee in outcome_callees.values_mut().chain(
                outcome_bindings
                    .values_mut()
                    .flat_map(|bindings| bindings.values_mut()),
            ) {
                if callee.name == *normal {
                    callee.use_outcomes(&outcome_names[normal], scope(*r));
                }
            }
        }
        let mut changed = false;
        for (r, normal, body) in todo
            .iter()
            .filter(|(_, name, _)| selected.contains(name))
            .cloned()
        {
            let name = &outcome_names[&normal];
            let mut cx = context(r);
            cx.callees = &outcome_callees;
            cx.compiler_bindings = &outcome_bindings;
            let Ok(mut lowered) = lower::lower_outcomes(&cx, body, name) else {
                continue;
            };
            if let Some(generic) = callees.get(&ctx.path(r)).and_then(|c| c.generic.as_ref()) {
                crate::lex::bind_parameters(
                    &mut lowered.decls,
                    &mut lowered.aux,
                    &generic.types,
                    name,
                );
            }
            let mut translation = done.remove(&normal).expect("selected normal model");
            translation.name = name.clone();
            translation.lowered = lowered;
            translation.model_context_sha256 = None;
            translations.insert(name.clone(), translation.path.clone());
            done.insert(name.clone(), translation);
            changed = true;
        }
        if !changed {
            break;
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
        let verdict =
            options
                .verdicts
                .matching(&t.kappa, &t.digest, t.model_context_sha256.as_deref());
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
    // An owner may now lift through source code while its generated kernel
    // remains shared by native callers. Retain its observations as policy
    // hints, never as evidence admitting another function or changed model.
    let mut kernel_hints = BTreeMap::new();
    for row in options
        .verdicts
        .rows
        .values()
        .filter(|row| matches!(row.status, Decision::Proved))
    {
        for (name, policy) in &row.axioms {
            kernel_hints
                .entry(name.clone())
                .or_insert_with(|| policy.clone());
        }
    }
    let mut aux: BTreeMap<String, Decl> = BTreeMap::new();
    let mut body_decls = Vec::new();
    let mut listing = String::from("spec = \"uorust/mir-candidates/1\"\n");
    for n in &order {
        let t = &done[n];
        let learned = options
            .verdicts
            .matching(&t.kappa, &t.digest, t.model_context_sha256.as_deref())
            .map(|v| v.axioms.clone())
            .or_else(|| options.learned.get(&t.kappa).cloned())
            .unwrap_or_default();
        for (k, d) in &t.lowered.aux {
            insert_observed_aux(&mut aux, k, d, &learned, &kernel_hints);
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
        let dependencies: Vec<String> = t
            .lowered
            .calls
            .iter()
            .filter_map(|name| done.get(name))
            .map(|callee| toml_string(&callee.kappa))
            .collect();
        listing.push_str(&format!(
            "\n[[candidate]]\nkappa = {}\nfunction = {}\ndigest = {}\ntrial = {}\ndecls = [{}]\ndependencies = [{}]\n",
            toml_string(&t.kappa),
            toml_string(&t.path),
            toml_string(&t.digest),
            on_trial.contains(&t.name),
            decls.join(", "),
            dependencies.join(", ")
        ));
        if let Some((residue, primary, runtime)) = recoveries.get(&t.r) {
            listing.push_str(&format!(
                "primary_residue = {}\nprimary_mir_sha256 = {}\nruntime_mir_sha256 = {}\n",
                toml_string(residue),
                toml_string(primary),
                toml_string(runtime)
            ));
        }
        if let Some(model) = &t.model_context_sha256 {
            listing.push_str(&format!("model_context_sha256 = {}\n", toml_string(model)));
        }
        if let Some(cause) = causes.get(&t.name) {
            let span = ctx.func(t.r).span.clone();
            notes
                .residue
                .insert(t.path.clone(), (span, Residue::Mir(cause.clone())));
        }
    }
    let mut schema_declarations = Vec::new();
    let mut emitted_schemas = BTreeSet::new();
    for ty in &out.types {
        if let Ty::Named(name) = ty {
            if !original_types.structs.contains_key(name)
                && !original_types.enums.contains_key(name)
                && !original_types.datas.contains_key(name)
            {
                type_decls(
                    ty,
                    source_types,
                    &BTreeSet::new(),
                    &mut emitted_schemas,
                    &mut schema_declarations,
                );
            }
        }
    }
    let mut declarations = schema_declarations;
    declarations.extend(in_dependency_order(aux));
    declarations.extend(body_decls);
    out.decls.extend(surface_dependency_order(declarations));
    if options.trial != crate::mir::Trial::Off {
        out.listing = Some(listing);
    }
    out
}

/// The same declaration closure used for actual native verification, with
/// reached ABI/schema readings and immutable SDK assets bound alongside it.
/// This is a producer input identity, never a fabricated SDK identifier.
fn native_model_contexts(
    done: &BTreeMap<String, Translation>,
    source: &[Decl],
    types: &Types,
    callees: &BTreeMap<String, crate::mir::lower::Callee>,
    limits: &crate::limits::Limits,
) -> BTreeMap<String, Option<String>> {
    let mut schemas = Vec::new();
    let mut emitted = BTreeSet::new();
    let compared = types.enums.keys().cloned().collect();
    for ty in done.values().flat_map(|t| t.lowered.types.iter()) {
        type_decls(ty, types, &compared, &mut emitted, &mut schemas);
    }
    let mut declarations = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    for decl in schemas.iter().chain(source).chain(
        done.values()
            .flat_map(|t| t.lowered.aux.values().chain(&t.lowered.decls)),
    ) {
        let name = decl.name().to_string();
        let text = crate::lex::render_decl(decl);
        if declarations.get(&name).is_some_and(|old| old != &text) {
            conflicts.insert(name.clone());
        }
        declarations.entry(name).or_insert(text);
    }
    let model = format!(
        "\\begin{{semanticmodule}}\n{}\n\\end{{semanticmodule}}",
        declarations
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    let static_context = serde_json::json!({"spec":"uorust/rooted-native-model/1",
        "sdk":include_str!("../../../prismpm.lock"),"assets":crate::assets::FIXED,"limits":format!("{limits:?}")});
    let closure_graph = crate::lex::ProofClosureGraph::new(&model).ok();
    done.iter().map(|(name,t)| {
        let roots=t.lowered.aux.keys().cloned().chain(t.lowered.decls.iter().map(|d|d.name().to_string())).collect::<Vec<_>>();
        let identity=closure_graph.as_ref().and_then(|graph|graph.closure(&roots).ok()).and_then(|closure| {
            let named=|n:&str|closure.contains(&format!("name: {n},"));
            if conflicts.iter().any(|n|named(n)) {return None;}
            let mut pending=vec![name.clone()];let mut native=BTreeSet::new();let mut readings=BTreeSet::new();
            while let Some(next)=pending.pop() {
                if native.insert(next.clone()) {
                    let body=&done[&next];readings.extend(body.lowered.types.iter().cloned());
                    pending.extend(body.lowered.calls.iter().filter(|n|done.contains_key(*n)).cloned());
                }
            }
            let schemas: BTreeMap<_,_>=types.structs.keys().chain(types.enums.keys()).chain(types.datas.keys()).filter(|n|named(n)).map(|n| {
                (n,format!("{:?}",(types.structs.get(n),types.enums.get(n),types.datas.get(n),types.cells.get(n),types.derives.get(n),types.discriminants.get(n),types.valid.get(n))))
            }).collect();
            let signatures:BTreeMap<_,_>=callees.values().filter(|c|named(&c.name)).map(|c|(&c.name,format!("{c:?}"))).collect();
            Some(sha(serde_json::json!({"context":static_context,"model":closure,
                "native_readings":format!("{readings:?}"),"schemas":schemas,"source_signatures":signatures}).to_string().as_bytes()))
        });
        (name.clone(),identity)
    }).collect()
}

/// Order the merged declaration dependency graph across source files and
/// native compiler imports, keeping mutual type and definition groups intact.
fn surface_dependency_order(decls: Vec<Decl>) -> Vec<Decl> {
    let model = format!(
        "\\begin{{semanticmodule}}\n{}\n\\end{{semanticmodule}}",
        decls
            .iter()
            .map(crate::lex::render_decl)
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    let Some(references) = crate::lex::declaration_dependencies(&model) else {
        // Preserve every byte of an invalid declaration set for the SDK's
        // normal diagnostic; ordering cannot accept a malformed module.
        return decls;
    };
    let mut groups: BTreeMap<String, Vec<Decl>> = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut pending = Vec::new();
    for decl in decls {
        let key = match &decl {
            Decl::Definition {
                mutual: Some(group),
                ..
            }
            | Decl::Data {
                mutual: Some(group),
                ..
            } => format!("mutual:{group}"),
            _ => format!("definition:{}", decl.name()),
        };
        names.insert(decl.name().to_string(), key.clone());
        if !groups.contains_key(&key) {
            pending.push(key.clone());
        }
        groups.entry(key).or_default().push(decl);
    }
    let dependencies: BTreeMap<String, BTreeSet<String>> = groups
        .iter()
        .map(|(key, decls)| {
            let calls = decls
                .iter()
                .flat_map(|d| references[d.name()].iter().cloned())
                .filter_map(|name| names.get(&name).filter(|group| *group != key).cloned())
                .collect();
            (key.clone(), calls)
        })
        .collect();
    let mut placed = BTreeSet::new();
    let mut out = Vec::new();
    while !pending.is_empty() {
        let before = placed.len();
        let mut rest = Vec::new();
        for key in pending {
            if dependencies[&key].is_subset(&placed) {
                placed.insert(key.clone());
                out.extend(groups.remove(&key).expect("pending group"));
            } else {
                rest.push(key);
            }
        }
        if before == placed.len() {
            // A non-mutual cycle still reaches the SDK's normal refusal;
            // ordering cannot turn that cycle into an accepted definition.
            for key in rest {
                out.extend(groups.remove(&key).expect("pending group"));
            }
            break;
        }
        pending = rest;
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

/// A shared kernel has one declaration, while any matching owner may carry
/// its original SDK policy observation. Default metadata is not an observation.
fn insert_observed_aux(
    aux: &mut BTreeMap<String, Decl>,
    key: &str,
    decl: &Decl,
    learned: &BTreeMap<String, Vec<String>>,
    hints: &BTreeMap<String, Vec<String>>,
) {
    use std::collections::btree_map::Entry;
    match aux.entry(key.to_string()) {
        Entry::Vacant(entry) => {
            entry.insert(with_axioms(with_axioms(decl.clone(), hints), learned));
        }
        Entry::Occupied(mut entry) => {
            if !learned.contains_key(decl.name()) {
                return;
            }
            let empty = BTreeMap::from([(decl.name().to_string(), Vec::new())]);
            let original = crate::lex::render_decl(&with_axioms(entry.get().clone(), &empty));
            let incoming = crate::lex::render_decl(&with_axioms(decl.clone(), &empty));
            if original == incoming {
                entry.insert(with_axioms(decl.clone(), learned));
            }
        }
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
            type_parameters,
            parameters,
            result,
            body,
            recursive,
            termination,
            mutual,
            ..
        } => Decl::Definition {
            type_parameters,
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
            type_parameters,
            parameters,
            statement,
            proof,
            ..
        } => Decl::Theorem {
            type_parameters,
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
        type_parameters: Vec::new(),
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
            type_parameters: Vec::new(),
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
                type_parameters: Vec::new(),
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
            type_parameters: Vec::new(),
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
pub(crate) fn type_decls(
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
                type_parameters: Vec::new(),
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
                    type_parameters: Vec::new(),
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
        type_parameters: Vec::new(),
        name: ty.render(),
        fields: fields
            .iter()
            .map(|(f, t)| (f.clone(), t.render()))
            .collect(),
    });
}

fn supply(ctx: &Model) -> Module {
    let mut decls = vec![Decl::Structure {
        type_parameters: Vec::new(),
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

/// Whether the trait a method implements is in scope where `caller` stands:
/// Rust reads `Type::name` and `value.name()` as a trait's method there
/// alone. Three rules of the language decide it, and whatever they leave
/// open counts as out of scope, so a call is refused before it is misread:
/// the scope that names a trait by one segment has it in scope (it declares
/// the trait, imports it, or takes it from the prelude); a name the impl's
/// scope neither declares nor imports, with no glob that could bind it, is
/// the standard prelude's, in scope everywhere in a crate that keeps its
/// prelude; and a file's own scope has a trait it declares or imports by a
/// path that leads to the trait's declaration (`declared_trait`).
fn trait_in_scope(ctx: &Model, caller: FnRef, imp: FnRef) -> bool {
    let method = ctx.func(imp);
    let Some(path) = &method.trait_path else {
        return false;
    };
    let single = !path.contains("::");
    let from = ctx.func(caller);
    if single && caller.pkg == imp.pkg && caller.file == imp.file && from.scope == method.scope {
        return true;
    }
    let package = &ctx.ws.packages[imp.pkg];
    let home = &ctx.file(imp).scopes[method.scope];
    let bound = home.traits.contains(path) || home.uses.iter().any(|(n, _)| n == path);
    if single && !bound && !home.glob {
        return caller.pkg == imp.pkg && !package.files.iter().any(|f| f.no_prelude);
    }
    if caller.pkg != imp.pkg || from.scope != 0 || method.scope != 0 {
        return false;
    }
    let Some(named) = declared_trait(ctx, imp.pkg, imp.file, path) else {
        return false;
    };
    let here = ctx.file(caller);
    let (file, name) = &named;
    (*file == caller.file && here.scopes[0].traits.contains(name))
        || here.scopes[0].uses.iter().any(|(_, used)| {
            declared_trait(ctx, caller.pkg, caller.file, used).as_ref() == Some(&named)
        })
}

/// The trait a path names from a file's own scope: the file of the crate
/// that declares it, and its name there. The path is read as Rust reads it,
/// through the scope's own `use` bindings, from a module the scope declares
/// and from `crate`, `self` and `super`, and through each module that only
/// re-exports the name. A path
/// into another crate, and one Rust would read from a name this does not
/// see, names no trait here.
fn declared_trait(ctx: &Model, pkg: usize, fi: usize, path: &str) -> Option<(usize, String)> {
    let files = &ctx.ws.packages[pkg].files;
    let mut at = fi;
    let mut path = path.to_string();
    let mut seen: BTreeSet<(usize, String)> = BTreeSet::new();
    // Each step moves to another module's file or reads another binding, and
    // no step is taken twice.
    while seen.insert((at, path.clone())) {
        let file = &files[at];
        let scope = file.scopes.first()?;
        let mut segs: Vec<&str> = path.split("::").collect();
        let first = *segs.first()?;
        if first.is_empty() {
            return None;
        }
        if segs.len() == 1 && scope.traits.iter().any(|t| t == first) {
            return Some((at, first.to_string()));
        }
        let own = scope.mods.iter().any(|m| m == first);
        if !own && !matches!(first, "crate" | "self" | "super") {
            // The scope's own binding of the first name, read in its place.
            let (_, bound) = scope
                .uses
                .iter()
                .find(|(n, used)| n == first && *used != path)?;
            let rest = segs[1..].join("::");
            path = match rest.is_empty() {
                true => bound.clone(),
                false => format!("{bound}::{rest}"),
            };
            continue;
        }
        let mut module: Vec<&str> = file.module.split("::").filter(|s| !s.is_empty()).collect();
        match first {
            "crate" => {
                module.clear();
                segs.remove(0);
            }
            "self" => {
                segs.remove(0);
            }
            _ => {}
        }
        while segs.first() == Some(&"super") {
            module.pop()?;
            segs.remove(0);
        }
        let name = segs.pop()?;
        module.extend(segs);
        let module = module.join("::");
        at = files
            .iter()
            .position(|f| f.target == file.target && f.module == module)?;
        path = name.to_string();
    }
    None
}

/// Why a default method a trait declares is not one function for every
/// implementing type, if it is not: it calls a method through `self` or
/// `Self`, which is each implementing type's (the trait's own body of it is
/// one an implementation may replace), or reads `self` or names `Self`
/// otherwise. `None` for a body that does neither: it means the same for
/// every implementing type, and lifts as written.
fn default_refusal(
    file: &SourceFile,
    f: &FnFact,
    declarer: &str,
    block: &crate::ast::Block,
) -> Option<Residue> {
    let mut called = Vec::new();
    calls_through_self(block, &mut called);
    if let Some(method) = called.first() {
        let declared = file
            .fns
            .iter()
            .find(|g| g.in_trait && g.owner.as_deref() == Some(declarer) && &g.name == method);
        let whose = match declared {
            Some(g) if g.tokens.ends_with('}') => format!(
                "{declarer}::{method}, whose body in the trait an implementing type may replace"
            ),
            Some(_) => format!("{declarer}::{method}, which the trait declares without a body"),
            None => format!("{method}, which {declarer} does not declare"),
        };
        return Some(Residue::Signature(format!(
            "a default method of {declarer} that calls {whose}: it means each implementing \
             type's"
        )));
    }
    let words = |word: &str| {
        f.tokens
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| *w == word)
            .count()
    };
    if words("Self") > 0 {
        return Some(Residue::Signature(format!(
            "a default method of {declarer} that names Self, each implementing type"
        )));
    }
    // The receiver is the one `self` a signature writes.
    if words("self") > usize::from(f.receiver.is_some()) {
        return Some(Residue::Signature(format!(
            "a default method of {declarer} that reads self, each implementing type's value"
        )));
    }
    None
}

/// Bounds that ask nothing a body can call through a type parameter
/// beyond what the lift refuses anyway: the auto traits, which declare no
/// method, `Sized`, and `Copy` and `Clone`, whose one method a value of a
/// type parameter has only through its dictionary, and a dictionary holds
/// no method of them (`x.clone()` there stops the lift). Each is read so
/// only where the crate declares no trait of the name.
const MARKER_BOUNDS: [&str; 6] = ["Send", "Sync", "Sized", "Unpin", "Copy", "Clone"];

/// Names of methods that a trait of the prelude gives every type by a
/// blanket impl through a receiver taken by value (`Into` by `From<T> for
/// T`, and `TryInto` after it): on a receiver `x: &T`, Rust finds that
/// method at `&T` itself, before a bound's method of the name that takes
/// `self` one dereference further. The lift does not tell the receivers
/// apart here, so a call of such a name is refused whatever its receiver.
const BLANKET_METHODS: [&str; 2] = ["into", "try_into"];

/// How a function's type parameters read, or why they do not: `None`
/// where it has none (or is a trait's method, or has const parameters,
/// which keep the rows they have).
///
/// A type parameter reads as
/// - the function type of its one `Fn(A, ..) -> R` bound, as `impl Fn(A,
///   ..) -> R` in argument position is read (which is a type parameter by
///   another syntax): a call of it is that bound's `Fn::call`;
/// - otherwise a LexLean type parameter `'T`, with a dictionary of the
///   methods its body calls of each trait of the crate that bounds it (see
///   `lift::Generic`), where each such trait has no parameters,
///   supertraits, associated items or `unsafe`, is the one trait of its
///   name in the crate, is implemented only for types of the crate named
///   as themselves (`wrapped_impl`: no impl for a reference, a box or by a
///   blanket impl, which the lift would read as the type they wrap), and
///   the scope of the function brings no trait in by a glob or from
///   another crate, a path dependency of the workspace included
///   (`leaves_crate`: such a trait could give a method of the name at an
///   earlier step of Rust's method lookup than the bound's);
/// - with no bound but [`MARKER_BOUNDS`], a type parameter with no
///   dictionary: the body can only move its values.
///
/// Any other bound stops the function with a row that names it: a
/// registry crate's trait a foreign row of that crate, any other a
/// signature row.
fn generic_value_names(sig: &crate::ast::Signature) -> BTreeSet<String> {
    sig.inputs
        .iter()
        .filter_map(|i| match i {
            crate::ast::FnArg::Typed(p) => match &*p.pat {
                crate::ast::Pat::Ident(id) => Some(lift::local_name(&id.ident)),
                _ => None,
            },
            crate::ast::FnArg::Receiver { .. } => Some(lift::local_name("self")),
        })
        .collect()
}

fn generic_type_names(sig: &crate::ast::Signature, types: &Types) -> BTreeMap<String, String> {
    let value_names = generic_value_names(sig);
    let mut used = value_names;
    used.extend(types.structs.keys().cloned());
    used.extend(types.enums.keys().cloned());
    used.extend(types.datas.keys().cloned());
    used.extend(lift::BARE_TYPES.iter().map(|s| s.to_string()));
    used.extend([lift::PRELUDE.to_string(), BEHAVIOR.to_string()]);
    let mut names = BTreeMap::new();
    for parameter in &sig.generics {
        if let crate::ast::GenericParam::Type(name) = parameter {
            let mut lex = name.clone();
            while used.contains(&lex) {
                lex.push_str("Param");
            }
            used.insert(lex.clone());
            names.insert(name.clone(), lex);
        }
    }
    names
}

fn generic_reading(
    ctx: &Model,
    syntax: &Syntax,
    file_scopes: &BTreeMap<(usize, usize, usize), Types>,
    types: &Types,
    r: FnRef,
    sig: &crate::ast::Signature,
) -> Result<Option<lift::Generic>, Residue> {
    use crate::ast::GenericParam;
    let f = ctx.func(r);
    let params: Vec<&String> = sig
        .generics
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(n) => Some(n),
            _ => None,
        })
        .collect();
    let constant = sig
        .generics
        .iter()
        .any(|p| matches!(p, GenericParam::Const(_)));
    if params.is_empty() || constant || f.in_trait || f.implements.is_some() {
        return Ok(None);
    }
    if sig.asyncness || sig.unsafety || sig.variadic {
        return Ok(None);
    }
    let named = |what: String| Residue::Signature(format!("generic over {what}"));
    if let Some(clause) = sig.unread_where.first() {
        return Err(named(format!(
            "a where clause that bounds no type parameter of its own: {clause}"
        )));
    }
    let package = &ctx.ws.packages[r.pkg];
    let crate_traits: Vec<(usize, &crate::facts::TraitFact)> = package
        .files
        .iter()
        .enumerate()
        .flat_map(|(fi, file)| file.trait_decls.iter().map(move |t| (fi, t)))
        .collect();
    let declared = |name: &str| crate_traits.iter().filter(|(_, t)| t.name == name).count();
    let value_names = generic_value_names(sig);
    let type_names = generic_type_names(sig, types);
    let lex_name = |n: &str| type_names[n].clone();
    let mut reads: BTreeMap<String, Ty> = BTreeMap::new();
    let mut functions: Vec<(&String, &crate::ast::RustType)> = Vec::new();
    let mut traits: Vec<(&String, String)> = Vec::new();
    let mut order = Vec::new();
    for p in &params {
        let bounds: Vec<&crate::ast::Bound> = sig
            .bounds
            .iter()
            .filter(|b| &b.param == *p && !b.maybe)
            .collect();
        let mut function = None;
        let mut bounded = Vec::new();
        for b in &bounds {
            let path = &b.path;
            if let Some(f) = &b.function {
                if function.replace(f).is_some() {
                    return Err(named(format!("{p}: two Fn bounds")));
                }
                continue;
            }
            let plain = path.segments.len() == 1
                && matches!(path.segments[0].arguments, crate::ast::PathArguments::None);
            if plain && MARKER_BOUNDS.contains(&path.text.as_str()) && declared(&path.text) == 0 {
                continue;
            }
            let last_plain = path
                .segments
                .last()
                .is_some_and(|s| matches!(s.arguments, crate::ast::PathArguments::None));
            match declared_trait(ctx, r.pkg, r.file, &path.text) {
                Some((_, name)) if last_plain => bounded.push(name),
                Some(_) => {
                    return Err(named(format!(
                    "{p}: {}, a trait of the crate with parameters, one trait per argument list",
                    b.text
                )))
                }
                None if foreign_crate(package, &b.text).is_some()
                    || ctx.bound_crate(r, &b.text).is_some() =>
                {
                    return Err(Residue::Type(b.text.clone()))
                }
                None => {
                    return Err(named(format!(
                        "{p}: {}, a bound the lift reads no dictionary of",
                        b.text
                    )))
                }
            }
        }
        match (function, bounded.is_empty()) {
            (Some(f), true) => functions.push((p, f)),
            (Some(_), false) => {
                return Err(named(format!(
                    "{p}: an Fn bound beside a trait of the crate"
                )))
            }
            (None, _) => {
                let lex = lex_name(p);
                reads.insert((*p).clone(), Ty::Param(lex.clone()));
                order.push(lex);
                traits.extend(bounded.into_iter().map(|t| (*p, t)));
            }
        }
    }
    // A function type may name the other type parameters.
    let mut scope = types.clone();
    scope.generic = reads.clone();
    for (p, f) in functions {
        let t = scope.ty(f)?;
        reads.insert(p.clone(), t);
    }
    scope.generic = reads.clone();
    let mut dictionary = Vec::new();
    if !traits.is_empty() {
        // A bound is read as its file's own scope reads it
        // (`declared_trait`): an inline module's names are its own.
        if f.scope != 0 {
            return Err(named(format!(
                "{} in an inline module, whose names the bound's reading does not follow",
                params
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let here = &ctx.file(r).scopes[f.scope];
        let outside = here.glob
            || here
                .uses
                .iter()
                .any(|(_, path)| leaves_crate(package, path));
        if outside {
            return Err(named(format!(
                "{} in a scope that brings in names by a glob or from another crate: a trait \
                 among them could give a method of a bound's name first",
                params
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
    let mut taken: BTreeSet<String> = value_names.clone();
    for (p, name) in &traits {
        let Some(&(tf, fact)) = crate_traits.iter().find(|(_, t)| &t.name == name) else {
            return Err(named(format!(
                "{p}: {name}, which the crate does not declare"
            )));
        };
        let stop = if declared(name) > 1 {
            Some("a name two traits of the crate share")
        } else if !fact.params.is_empty() {
            Some("a trait with parameters, one trait per argument list")
        } else if !fact.supertraits.is_empty() {
            Some("a trait with supertraits, whose methods the dictionary does not hold")
        } else if !fact.associated.is_empty() {
            Some("a trait with associated types or constants")
        } else if fact.marked {
            Some("an unsafe trait")
        } else {
            None
        };
        if let Some(why) = stop {
            return Err(named(format!("{p}: {name}, {why}")));
        }
        if let Some(wrapped) = wrapped_impl(package, r.pkg, file_scopes, name) {
            return Err(named(format!(
                "{p}: {name}, which the crate implements for {wrapped}, a type the lift does \
                 not read as itself: a call through the bound could select that impl"
            )));
        }
        let file = &package.files[tf];
        let Ty::Param(lex) = &reads[*p] else {
            continue;
        };
        // A trait's method is read in the trait's scope, where the
        // function's type parameters name nothing, and `Self` is the one
        // the dictionary is of.
        let mut own = file_scopes
            .get(&(r.pkg, tf, fact.scope))
            .unwrap_or(types)
            .clone();
        own.generic = BTreeMap::from([("Self".to_string(), Ty::Param(lex.clone()))]);
        for (idx, m) in file.fns.iter().enumerate() {
            if !(m.in_trait && m.owner.as_deref() == Some(name.as_str()) && m.scope == fact.scope) {
                continue;
            }
            // The dictionary holds every method of the trait it can type,
            // so that a generic function passes on to a generic callee
            // what the callee's body calls; a method the body calls by
            // name must be one, and must mean the bound's.
            let called = f
                .calls
                .iter()
                .any(|c| c == &format!(".{}", m.name) || c.ends_with(&format!("::{}", m.name)));
            let msig = &syntax.function(&file.rel, idx).signature;
            let ty = match lift::entry_ty(&own, msig, &Ty::Param(lex.clone())) {
                Ok(ty) => ty,
                Err(Residue::Signature(what)) if called => {
                    return Err(named(format!("{p}: {name}::{}, {what}", m.name)))
                }
                Err(other) if called => return Err(other),
                Err(_) => continue,
            };
            if called && BLANKET_METHODS.contains(&m.name.as_str()) {
                return Err(named(format!(
                    "{p}: {name}::{}, a name the prelude's blanket impls give every type first",
                    m.name
                )));
            }
            let shared = crate_traits.iter().any(|(fi, t)| {
                t.name != *name
                    && package.files[*fi].fns.iter().any(|g| {
                        g.in_trait
                            && g.owner.as_deref() == Some(t.name.as_str())
                            && g.name == m.name
                    })
            });
            if called && shared {
                return Err(named(format!(
                    "{p}: {name}::{}, a method name another trait of the crate declares too",
                    m.name
                )));
            }
            let base = lift::local_name(&format!("{p}_{}", m.name));
            let mut entry_name = base.clone();
            let mut k = 1;
            while taken.contains(&entry_name) {
                k += 1;
                entry_name = format!("{base}{k}");
            }
            taken.insert(entry_name.clone());
            dictionary.push(lift::Entry {
                param: lex.clone(),
                declarer: name.clone(),
                method: m.name.clone(),
                name: entry_name,
                ty,
            });
        }
    }
    Ok(Some(lift::Generic {
        types: order,
        reads,
        dictionary,
    }))
}

/// The methods a body calls on `self` and the functions it calls as
/// `Self::name`, in source order.
fn calls_through_self(block: &crate::ast::Block, out: &mut Vec<String>) {
    use crate::ast::{RustExpr, Stmt, UnOp};
    fn is_self(e: &RustExpr) -> bool {
        match e {
            RustExpr::Path(p) => p.is_ident("self"),
            RustExpr::Paren(inner) => is_self(inner),
            RustExpr::Reference(r) => is_self(&r.expr),
            RustExpr::Unary(u) => matches!(u.op, UnOp::Deref) && is_self(&u.expr),
            _ => false,
        }
    }
    fn expr(e: &RustExpr, out: &mut Vec<String>) {
        match e {
            RustExpr::MethodCall(m) if is_self(&m.receiver) => out.push(m.method.clone()),
            RustExpr::Call(c) => {
                if let RustExpr::Path(p) = &*c.func {
                    if let [owner, name] = p.segments.as_slice() {
                        if owner.ident == "Self" {
                            out.push(name.ident.clone());
                        }
                    }
                }
            }
            RustExpr::Macro(m) => {
                for arg in m.args.iter().flat_map(|a| &a.items) {
                    expr(arg, out);
                }
            }
            _ => {}
        }
        let (exprs, blocks) = e.parts();
        for inner in exprs {
            expr(inner, out);
        }
        for inner in blocks {
            calls_through_self(inner, out);
        }
    }
    for stmt in &block.stmts {
        match stmt {
            Stmt::Local(l) => {
                if let Some(init) = &l.init {
                    expr(&init.expr, out);
                    if let Some(diverge) = &init.diverge {
                        expr(diverge, out);
                    }
                }
            }
            Stmt::Expr(e, _) => expr(e, out),
            Stmt::Macro(m) => {
                for arg in m.args.iter().flat_map(|a| &a.items) {
                    expr(arg, out);
                }
            }
            Stmt::Other => {}
        }
    }
}

/// A module of a crate: its package, its target, and its path in the
/// crate (the file's module, then the inline modules around).
type ModKey = (usize, usize, String);

/// What a name a type construct writes stands for where it is written.
enum Found {
    /// A type of the workspace, declared at these sites.
    Sites(Vec<String>),
    /// No type of the workspace: a primitive, the standard library's, an
    /// extern crate's, or a word of the chain's prose.
    Outside,
    /// Perhaps a type of the workspace, whose declaration the tool cannot
    /// place.
    Unknown,
}

/// Every module of the workspace with its bindings and its declarations,
/// to read the names a type construct writes as Rust reads them.
struct TypeIndex<'a> {
    ctx: &'a Model,
    /// Each module: the file and the scope of it that holds its items.
    modules: BTreeMap<ModKey, (usize, usize)>,
    /// The sites of each type a module declares, by module and name.
    decls: BTreeMap<(ModKey, String), Vec<String>>,
    /// The module each declaration site stands in.
    at: BTreeMap<String, ModKey>,
}

impl<'a> TypeIndex<'a> {
    fn new(ctx: &'a Model) -> Self {
        let mut index = TypeIndex {
            ctx,
            modules: BTreeMap::new(),
            decls: BTreeMap::new(),
            at: BTreeMap::new(),
        };
        for (pkg, package) in ctx.ws.packages.iter().enumerate() {
            for (fi, file) in package.files.iter().enumerate() {
                let module = |si: usize| -> ModKey {
                    let inline = file.scopes.get(si).map_or("", |s| s.module.as_str());
                    (pkg, file.target, join_path(&file.module, inline))
                };
                for si in 0..file.scopes.len() {
                    index.modules.entry(module(si)).or_insert((fi, si));
                }
                for (si, name, span) in &file.declared {
                    let sites = index.decls.entry((module(*si), name.clone())).or_default();
                    if !sites.contains(&span.render()) {
                        sites.push(span.render());
                    }
                    index.at.insert(span.render(), module(*si));
                }
            }
        }
        index
    }

    /// The module of the function `r`.
    fn module_of(&self, r: FnRef) -> ModKey {
        let file = self.ctx.file(r);
        let inline = file
            .scopes
            .get(self.ctx.func(r).scope)
            .map_or("", |s| s.module.as_str());
        (r.pkg, file.target, join_path(&file.module, inline))
    }

    /// What `path` names in `module` as Rust reads it: `crate::`, `self::`
    /// and `super::`, a child module, then the last segment as a type the
    /// module declares, or one a `use` of it binds (followed to its
    /// declaration, so a renaming `use` names the type it renames), or one
    /// a macro the module invokes writes into its arguments (the item it
    /// expands to is declared there). A first segment that is no module of
    /// the crate is an extern crate's or the standard library's. A name
    /// the module neither declares nor binds is the prelude's where it
    /// was written bare and no glob may bind it; where a glob may, or
    /// inside a module the path walked into, it is not placed.
    fn resolve(&self, module: &ModKey, path: &str, seen: &mut BTreeSet<(ModKey, String)>) -> Found {
        let segments: Vec<&str> = path.split("::").collect();
        let Some((last, prefix)) = segments.split_last() else {
            return Found::Outside;
        };
        let mut cur = module.clone();
        let mut walked = false;
        for (i, seg) in prefix.iter().enumerate() {
            match *seg {
                "crate" if i == 0 => cur.2 = String::new(),
                "self" if i == 0 => {}
                "super" if prefix[..i].iter().all(|s| *s == "super") => {
                    cur.2 = cur.2.rsplit_once("::").map_or("", |(up, _)| up).to_string();
                }
                seg => {
                    let child = (cur.0, cur.1, join_path(&cur.2, seg));
                    if self.modules.contains_key(&child) {
                        cur = child;
                    } else if i == 0
                        && self
                            .scope(&cur)
                            .is_some_and(|s| !s.uses.iter().any(|(n, _)| n == seg))
                    {
                        return Found::Outside;
                    } else {
                        return Found::Unknown;
                    }
                }
            }
            walked = true;
        }
        if !seen.insert((cur.clone(), (*last).to_string())) {
            return Found::Unknown;
        }
        if let Some(sites) = self.decls.get(&(cur.clone(), (*last).to_string())) {
            return Found::Sites(sites.clone());
        }
        let Some(scope) = self.scope(&cur) else {
            return Found::Unknown;
        };
        if let Some((_, bound)) = scope.uses.iter().find(|(n, _)| n == last) {
            return self.resolve(&cur, bound, seen);
        }
        let invoked: Vec<String> = scope
            .invoked
            .iter()
            .filter(|(idents, _)| idents.iter().any(|i| i == last))
            .map(|(_, span)| span.render())
            .collect();
        if !invoked.is_empty() {
            return Found::Sites(invoked);
        }
        if walked || scope.glob {
            Found::Unknown
        } else {
            Found::Outside
        }
    }

    fn scope(&self, module: &ModKey) -> Option<&'a crate::facts::ScopeFact> {
        let &(fi, si) = self.modules.get(module)?;
        self.ctx.ws.packages[module.0].files[fi].scopes.get(si)
    }
}

fn join_path(outer: &str, inner: &str) -> String {
    match (outer, inner) {
        ("", inner) => inner.to_string(),
        (outer, "") => outer.to_string(),
        (outer, inner) => format!("{outer}::{inner}"),
    }
}

/// The sites of each type construct of the queue: for every function a
/// type stops, each name its construct writes ([`crate::register::named_types`])
/// read where the chain writes it. The owner of a place is a type of the
/// workspace by its LexLean name, or by its path; a name after a place is
/// written in the declaration of that place's owner, and is read in its
/// module; a name before any place is written in the function's
/// signature, and is read in the function's module ([`TypeIndex::resolve`]),
/// or as a LexLean name the projection gave a type in place of its own. So
/// a type is never sited by another type's name: `RefCell` read where a
/// `use` binds the standard library's has no site, though the workspace
/// declares a `RefCell` elsewhere. A name the tool cannot place is listed
/// apart, unless it is one word that begins in lower case (a word of the
/// chain's prose, or a primitive: Rust's naming convention writes a type
/// otherwise).
fn type_sites(ctx: &Model, notes: &Notes) -> BTreeMap<String, crate::register::Sites> {
    let index = TypeIndex::new(ctx);
    let product: BTreeMap<String, FnRef> = ctx
        .all_fns()
        .into_iter()
        .filter(|&r| ctx.product(r))
        .map(|r| (ctx.path(r), r))
        .collect();
    let mut out: BTreeMap<String, crate::register::Sites> = BTreeMap::new();
    for (path, (_, reason)) in &notes.residue {
        let Residue::Type(_) = reason else {
            continue;
        };
        let construct = reason.detail();
        let (defined, unsited) = out.entry(construct.clone()).or_default();
        let Some(&r) = product.get(path) else {
            continue;
        };
        let mut context = index.module_of(r);
        for named in crate::register::named_types(&construct) {
            let mut seen = BTreeSet::new();
            let lexed = notes.lexed.get(named.name);
            let found = match lexed {
                Some((span, renamed)) if named.place || *renamed => {
                    Found::Sites(vec![span.render()])
                }
                _ => match index.resolve(&context, named.name, &mut seen) {
                    Found::Sites(sites) => Found::Sites(sites),
                    // `Enum::Variant.0`: the place's owner is the enum.
                    _ if named.place && named.name.contains("::") => {
                        let owner = named.name.rsplit_once("::").map_or("", |(o, _)| o);
                        index.resolve(&context, owner, &mut BTreeSet::new())
                    }
                    other => other,
                },
            };
            match found {
                Found::Sites(sites) => {
                    if named.place {
                        if let Some(module) = sites.first().and_then(|s| index.at.get(s)) {
                            context = module.clone();
                        }
                    }
                    for site in sites {
                        if !defined.contains(&site) {
                            defined.push(site);
                        }
                    }
                }
                Found::Unknown => {
                    let word = !named.name.contains("::")
                        && named.name.starts_with(|c: char| c.is_lowercase());
                    if !word && !unsited.iter().any(|u| u == named.name) {
                        unsited.push(named.name.to_string());
                    }
                }
                Found::Outside => {}
            }
        }
    }
    out
}

/// The callee a refusal `calls NAME` of `r` means, by its crate path, where
/// the projection can tell which function of `r`'s crate it is: every call
/// `r` writes whose last segment is NAME (`Alpha::make`, `Self::make`, or
/// `make` alone) reads, through the type its prefix names in `r`'s module
/// scope (its file's, or the inline module's it stands in), as
/// one key of the crate's candidates, and those keys are one function; or
/// `r` writes no such call and NAME is a candidate's key itself (a method
/// call's `Type::name`). Otherwise NAME as the lift wrote it: two callees of
/// one name (`Alpha::make` beside `Beta::make`) are then never one
/// construct of the register by their last segment alone.
fn callee_path(
    ctx: &Model,
    file_scopes: &BTreeMap<(usize, usize, usize), Types>,
    names: &BTreeMap<String, FnRef>,
    r: FnRef,
    callee: &str,
) -> String {
    let types = &file_scopes[&(r.pkg, r.file, ctx.func(r).scope)];
    let mut meant: BTreeSet<FnRef> = BTreeSet::new();
    let mut written = false;
    for call in &ctx.func(r).calls {
        let segments: Vec<&str> = call.split("::").collect();
        let key = match segments.as_slice() {
            [.., owner, last] if *last == callee => {
                let owner = match (*owner, &ctx.func(r).owner) {
                    ("Self", Some(own)) => own.as_str(),
                    (owner, _) => owner,
                };
                format!("{}::{last}", types.resolve(owner))
            }
            [last] if *last == callee => callee.to_string(),
            _ => continue,
        };
        written = true;
        match names.get(&key) {
            Some(&g) => {
                meant.insert(g);
            }
            None => return callee.to_string(),
        }
    }
    if !written {
        if let Some(&g) = names.get(callee) {
            meant.insert(g);
        }
    }
    match meant.into_iter().collect::<Vec<_>>().as_slice() {
        [g] => ctx.path(*g),
        _ => callee.to_string(),
    }
}

/// Why the lift does not name the type a method is of, as the file `fi` of
/// package `pkg` writes it: the shape it is declared in, or the first of
/// its fields whose type is outside the lift and, where that is another
/// type of the crate, why that one is, down to a type the crate does not
/// declare. Each type is explained once, so a family of types that hold
/// each other ends.
fn unlifted_type(
    ctx: &Model,
    syntax: &Syntax,
    scopes: &BTreeMap<(usize, usize, usize), Types>,
    expanded: &dyn Fn(usize, &crate::ast::RustType) -> crate::ast::RustType,
    pkg: usize,
    fi: usize,
    written: &str,
) -> String {
    let package = &ctx.ws.packages[pkg];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut next = Some((fi, written.to_string()));
    while let Some((from, ty)) = next.take() {
        // The name a declaration would carry: the path's last segment,
        // without its arguments.
        let head = ty.split('<').next().unwrap_or(&ty);
        let name = head.rsplit("::").next().unwrap_or(head).trim().to_string();
        let plain = name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty();
        if !plain || !seen.insert(name.clone()) {
            out.push(if plain {
                "which holds itself where no inductive type can".to_string()
            } else {
                "which is not a struct or an enum by name".to_string()
            });
            break;
        }
        if ty.contains('<') && out.is_empty() {
            out.push(
                "written with arguments, where a method's type is read by its name alone"
                    .to_string(),
            );
            break;
        }
        // Its declaration: the writing file's own, else the crate's first.
        let order = std::iter::once(from).chain((0..package.files.len()).filter(|i| *i != from));
        let mut found = None;
        for at in order {
            let file = &package.files[at];
            let read = &syntax.files[&file.rel];
            if let Some(shape) = file.unread.iter().find(|(n, _)| *n == name) {
                found = Some(Err(shape.1.to_string()));
            } else if let Some(i) = file.structs.iter().position(|st| st.name == name) {
                let fields = read.structs[i]
                    .iter()
                    .map(|(field, t)| (format!("field {field}"), t.clone()))
                    .collect::<Vec<_>>();
                found = Some(Ok((at, file.structs[i].scope, fields)));
            } else if let Some(i) = file.enums.iter().position(|e| e.name == name) {
                let fields = read.enums[i]
                    .iter()
                    .flat_map(|(variant, fields)| {
                        fields
                            .iter()
                            .map(move |(_, t)| (format!("variant {variant}"), t.clone()))
                    })
                    .collect::<Vec<_>>();
                found = Some(Ok((at, file.enums[i].scope, fields)));
            }
            if found.is_some() {
                break;
            }
        }
        match found {
            None => out.push("which this crate does not declare".to_string()),
            Some(Err(shape)) => out.push(format!("{shape}, which the lift does not read")),
            Some(Ok((at, declared, fields))) => {
                let scope = &scopes[&(pkg, at, declared)];
                let imports = &package.files[at].imports;
                let outside = fields.iter().find_map(|(part, t)| {
                    match scope.ty(&expanded(pkg, &t.localized(imports))) {
                        Err(Residue::Type(inner)) => Some((part, inner)),
                        _ => None,
                    }
                });
                match outside {
                    Some((part, inner)) => {
                        out.push(format!("whose {part} holds {inner}"));
                        next = Some((at, inner));
                    }
                    None => out.push(
                        "which the lift leaves out though each of its parts lifts: it has no \
                         part, it is declared twice, its name is one the projection keeps, or \
                         it holds itself where no inductive type can"
                            .to_string(),
                    ),
                }
            }
        }
    }
    out.join(", ")
}

/// Every item of product code's value namespace a body can name by one
/// identifier: each free function of product code and each constant of a
/// library or binary target, by package and name, at its site (`(pkg,
/// target, module, name)`, the module the language names it through).
fn value_sites(ctx: &Model, syntax: &Syntax) -> BTreeMap<(usize, String), BTreeSet<FnSite>> {
    let mut sites: BTreeMap<(usize, String), BTreeSet<FnSite>> = BTreeMap::new();
    for (pkg, package) in ctx.ws.packages.iter().enumerate() {
        for file in &package.files {
            if !matches!(
                package.targets[file.target].kind,
                TargetKind::Lib | TargetKind::Bin
            ) {
                continue;
            }
            let Some(read) = syntax.files.get(&file.rel) else {
                continue;
            };
            for (name, _, _, scope, _, _) in &read.consts {
                let site = (pkg, file.target, file.item_module(*scope), name.clone());
                sites.entry((pkg, name.clone())).or_default().insert(site);
            }
        }
    }
    for r in ctx.all_fns() {
        let f = ctx.func(r);
        if f.owner.is_some() || !ctx.product(r) {
            continue;
        }
        let file = ctx.file(r);
        let site = (r.pkg, file.target, ctx.module_of(r), f.name.clone());
        sites
            .entry((r.pkg, f.name.clone()))
            .or_default()
            .insert(site);
    }
    sites
}

/// The free functions of product code whose name two modules (or two
/// targets) of one crate share, `toml_string` in `project` and in
/// `certify`, each -> its `rust` key and the LexLean name it lifts under:
/// qualified by its module, and by its target where that is not the
/// library, so that no two of them can meet. A name only one module
/// declares keeps its own, and so its definition and every κ's verdict.
/// A constant of product code is a site of the same kind: Rust reads a
/// bare `N` as the constant its module declares or a `use` of it binds,
/// so constants of one name in two modules are homonyms as functions are
/// (`(pkg, target, module, name)` names one item of the value namespace).
fn homonym_fns(
    ctx: &Model,
    sites: &BTreeMap<(usize, String), BTreeSet<FnSite>>,
) -> BTreeMap<FnSite, (String, String)> {
    // A site's key and name by its target and module.
    let qualified = |site: &FnSite| {
        let package = &ctx.ws.packages[site.0];
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
        parts.push(site.3.clone());
        let rust = format!("{}#{}", site.1, parts.join("::"));
        (rust, lift::local_name(&parts.join("_")))
    };
    let mut out = BTreeMap::new();
    let mut single: Vec<FnSite> = Vec::new();
    for at in sites.values() {
        if at.len() < 2 {
            single.extend(at.iter().cloned());
            continue;
        }
        for site in at {
            let names = qualified(site);
            out.insert(site.clone(), names);
        }
    }
    // A function alone of its name, whose name reads as one a module gave
    // (`mir_memory_axioms` beside `axioms` of `mir::memory`), takes its
    // module's too: the names stay one per function whichever of the two
    // lifts, and whichever the projection reaches first.
    loop {
        let given: BTreeSet<(usize, String)> = out
            .iter()
            .map(|(site, (_, lex)): (&FnSite, &(String, String))| (site.0, lex.clone()))
            .collect();
        let (clashing, rest): (Vec<FnSite>, Vec<FnSite>) = single
            .into_iter()
            .partition(|site| given.contains(&(site.0, lift::local_name(&site.3))));
        single = rest;
        if clashing.is_empty() {
            break;
        }
        for site in clashing {
            let names = qualified(&site);
            out.insert(site, names);
        }
    }
    out
}

/// Whether an item a macro of the package makes in item position may be
/// what a name of file `fi` means: the file invokes such a macro (an
/// item's name is not hygienic, so it lands in the invoking module), or
/// the package does and a module of the file imports by a glob.
fn macro_reachable(package: &Package, fi: usize) -> bool {
    let file = &package.files[fi];
    package.file_makes_items(fi)
        || (package.makes_items(fi)
            && file
                .scopes
                .iter()
                .enumerate()
                .any(|(i, s)| s.outward_glob(i)))
}

/// The registered crates' type paths a package reads
/// ([`crate::registered::of_package`]), less those it cannot: a path whose
/// head (`num_bigint` of `num_bigint::BigInt`) a module, type, function or
/// `use` of the package binds itself, since a path written relative to a
/// module starts at what that module binds before it reaches a crate. (A
/// macro of the package may declare a module of the crate's name: where
/// it can be reached the lift reads no type by its path,
/// `Types::paths_open`.)
fn registered_paths(
    ws: &Workspace,
    resolver: &crate::names::Resolver<'_>,
    pkg: usize,
) -> Vec<String> {
    let package = &ws.packages[pkg];
    let mut read = crate::registered::of_package(package, &ws.lock, &ws.replaced);
    read.retain(|path| {
        let head = path.split("::").next().unwrap_or(path);
        !resolver.package_binds(pkg, head)
    });
    read
}

/// The identifiers of Rust tokens, string literals included (a format
/// string names its inline arguments): every name a body can write.
fn identifiers(tokens: &str) -> BTreeSet<String> {
    tokens
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.starts_with(|c: char| c.is_alphabetic() || c == '_'))
        .map(str::to_string)
        .collect()
}

/// How a body reads the names of its crate's value namespace: what the
/// candidate's `homonyms` holds, for every such name, not only those two
/// modules share. The lift reads a bare name, and a path call by its last
/// segment, as the crate's item of that key (`env.fns`), so a name the
/// language binds to anything else there must read as none: a function
/// of another module that the reader's scope does not reach, the item a
/// renaming `use` binds, a function outside the crate (`use
/// std::cmp::max`), a variant a glob imports, an item a macro invocation
/// declares, a function the configuration compiles out.
struct Reads<'a> {
    ctx: &'a Model,
    resolver: &'a crate::names::Resolver<'a>,
    homonyms: &'a BTreeMap<FnSite, (String, String)>,
    sites: &'a BTreeMap<(usize, String), BTreeSet<FnSite>>,
}

impl Reads<'_> {
    /// The key a site lifts under: its module's where two modules share
    /// its name, its own name otherwise; none for no site of product code.
    fn key(&self, site: &FnSite) -> Option<String> {
        if let Some((rust, _)) = self.homonyms.get(site) {
            return Some(rust.clone());
        }
        self.sites
            .get(&(site.0, site.3.clone()))
            .is_some_and(|at| at.contains(site))
            .then(|| site.3.clone())
    }

    /// How a function reads each value name its body writes. A call is
    /// read as the one function admission's name resolution names for
    /// every call of the body that ends in that name (the lift reads a
    /// call by its last segment, so `go()`, `inner::go()` and `super::go()`
    /// of one body are one name to it): a glob import's, an inline
    /// module's, `super`'s, a block `use`'s, and none where those calls
    /// mean two functions, a method, or nothing of product code (`max` of
    /// `use std::cmp::max`). A bare call resolution leaves open, and every
    /// name not called, reads as the module scope does (`scope_reading`).
    fn call_reading(&self, r: FnRef) -> BTreeMap<String, Option<String>> {
        let ctx = self.ctx;
        let f = ctx.func(r);
        let at = (r.pkg, ctx.file(r).target, ctx.module_of(r));
        let mut out = self.scope_reading((at.0, at.1, &at.2), &identifiers(&f.tokens));
        let mut meant: BTreeMap<String, BTreeSet<Option<String>>> = BTreeMap::new();
        for (call, callee) in ctx.named.get(&r).into_iter().flatten() {
            let name = call.rsplit("::").next().unwrap_or(call);
            let Some(scoped) = out.get(name) else {
                continue;
            };
            let reading = match callee {
                Meant::Fn(g) if ctx.func(*g).owner.is_none() => self.key(&(
                    g.pkg,
                    ctx.file(*g).target,
                    ctx.module_of(*g),
                    ctx.func(*g).name.clone(),
                )),
                Meant::Open if call == name => scoped.clone(),
                Meant::Fn(_) | Meant::Outside | Meant::Open => None,
            };
            meant.entry(name.to_string()).or_default().insert(reading);
        }
        for (name, readings) in meant {
            let one = match readings.len() {
                1 => readings.into_iter().next().flatten(),
                _ => None,
            };
            out.insert(name, one);
        }
        out
    }

    /// How module `module` of target `target` of package `pkg` reads each
    /// of `words` that names an item of its crate's value namespace: the
    /// key of the item Rust's name resolution binds the bare name to there
    /// (`names::Resolver::bare_value`: an item the module declares and its
    /// configuration compiles, a `use` binding, renaming ones included, a
    /// glob's item), or none where it binds none of the crate's items or
    /// the resolution cannot tell (an item-level macro invocation of the
    /// module). Rust gives an inline module none of the names of the
    /// module around it, so a function is read through its own module.
    fn scope_reading(
        &self,
        (pkg, target, module): (usize, usize, &str),
        words: &BTreeSet<String>,
    ) -> BTreeMap<String, Option<String>> {
        let mut out = BTreeMap::new();
        for name in words {
            if !self.sites.contains_key(&(pkg, name.clone())) {
                continue;
            }
            let bound = self
                .resolver
                .bare_value((pkg, target, module), name)
                .and_then(|(declared, item)| self.key(&(pkg, target, declared, item)));
            out.insert(name.clone(), bound);
        }
        out
    }
}

/// The names of `names` that `text` (Rust tokens, spaced or tidied) writes
/// behind a path separator: `P` of `crate::inner::P`, `super::P`,
/// `Vec<a::P>` or `a::P::new`. The lift reads a type path by its last
/// segment in the reader's own scope (`ast::Path::localized`, and an owner
/// of `Owner::f` by its name), so for a name two modules declare such a
/// path names the type the reader's scope binds, which need not be the one
/// the path leads to: a name found here is read as no type.
fn written_through_paths(text: &str, names: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("::") {
        rest = rest[at + 2..].trim_start();
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let word = &rest[..end];
        if names.contains(word) {
            out.insert(word.to_string());
        }
    }
    out
}

/// A scope's reading of its crate's shared type names with each of
/// `blind` read as no type: the name itself, which the crate's types
/// register as one several types share (`lift::SHARED_NAME`).
fn blinded(
    names: &BTreeMap<String, String>,
    blind: Option<&BTreeSet<String>>,
) -> BTreeMap<String, String> {
    let mut out = names.clone();
    for name in blind.into_iter().flatten() {
        out.insert(name.clone(), name.clone());
    }
    out
}

/// Whether a `use` path leaves the crate: its first segment names a
/// dependency of the package, a registry crate or a path one alike, or the
/// path starts at the extern prelude (`::`). The standard library's crates
/// are read by `BLANKET_METHODS`: no trait of theirs has a blanket impl
/// over every type with a by-value method beside `Into` and `TryInto`.
fn leaves_crate(package: &Package, path: &str) -> bool {
    if path.starts_with("::") {
        return true;
    }
    let head = path.split("::").next().unwrap_or(path);
    package
        .deps
        .iter()
        .any(|d| d.name.replace('-', "_") == head)
}

/// The first impl of the crate's trait `name` (matched by the trait path's
/// last segment, so that any trait of that name counts) whose self type is
/// not a type of the crate named by its own plain path at its file's own
/// scope: a reference, a `Box`, `Rc` or `Arc`, an alias, or a type
/// parameter of a blanket impl. The lift reads `&X`, `Box<X>`, `Rc<X>` and
/// `Arc<X>` as `X` and keys impls by the named type, so where such an impl
/// exists Rust's method probe (which tries the receiver's own type, then
/// its autoref, before it dereferences) and its trait selection at a call
/// (`T = &Square` with `impl Area for &Square`) can choose an impl the
/// dictionary does not hold. Where none exists, `X: Tr` is the only impl
/// a call or a probe through the wrapper can reach, since an impl of the
/// crate's trait for a wrapper can only be written in this crate or in a
/// dependent one, whose impls a body of this crate does not see.
fn wrapped_impl(
    package: &Package,
    pkg: usize,
    file_scopes: &BTreeMap<(usize, usize, usize), Types>,
    name: &str,
) -> Option<String> {
    package.files.iter().enumerate().find_map(|(fi, file)| {
        file.trait_impls
            .iter()
            .filter(|i| i.trait_path.rsplit("::").next() == Some(name))
            .find(|i| {
                let named = |types: &Types, plain: &str| {
                    let key = types.resolve(plain);
                    types.structs.contains_key(&key)
                        || types.enums.contains_key(&key)
                        || types.datas.contains_key(&key)
                };
                match (&i.plain, i.scope) {
                    (Some(plain), 0) => !file_scopes
                        .get(&(pkg, fi, 0))
                        .is_some_and(|t| named(t, plain)),
                    _ => true,
                }
            })
            .map(|i| i.self_ty.clone())
    })
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

/// The bare type names a file writes in its declarations and signatures,
/// less every name a parameter of an item of the file takes: a type
/// parameter is bound by its item, not by its module.
fn bare_names(file: &crate::facts::FileSyntax, facts: &SourceFile) -> BTreeSet<String> {
    fn walk(t: &crate::ast::RustType, out: &mut BTreeSet<String>) {
        use crate::ast::{GenericArgument, PathArguments, TypeKind};
        match &t.kind {
            TypeKind::Reference { elem, .. } => walk(elem, out),
            TypeKind::Slice(t) | TypeKind::Array(t, _) | TypeKind::Paren(t) => walk(t, out),
            TypeKind::Fn(ins, ret) => {
                ins.iter().for_each(|t| walk(t, out));
                walk(ret, out);
            }
            TypeKind::Tuple(ts) => ts.iter().for_each(|t| walk(t, out)),
            TypeKind::Path { qself, path } => {
                if let ([one], false) = (path.segments.as_slice(), qself) {
                    out.insert(one.ident.clone());
                }
                for segment in &path.segments {
                    if let PathArguments::AngleBracketed(args) = &segment.arguments {
                        for a in args {
                            if let GenericArgument::Type(t) = a {
                                walk(t, out);
                            }
                        }
                    }
                }
            }
            TypeKind::Other => {}
        }
    }
    let mut out = BTreeSet::new();
    let fields = file.structs.iter().flatten().map(|(_, t)| t);
    let variants = file
        .enums
        .iter()
        .flatten()
        .flat_map(|(_, fs)| fs.iter().map(|(_, t)| t));
    let aliases = file.aliases.iter().map(|(_, a, _)| &a.to);
    let signatures = file.fns.iter().flat_map(|f| {
        let sig = &f.signature;
        sig.inputs
            .iter()
            .filter_map(|i| match i {
                crate::ast::FnArg::Typed(p) => Some(&p.ty),
                crate::ast::FnArg::Receiver { .. } => None,
            })
            .chain(sig.output.iter())
    });
    for t in fields.chain(variants).chain(aliases).chain(signatures) {
        walk(t, &mut out);
    }
    for (_, alias, _) in &file.aliases {
        alias.params.iter().for_each(|p| {
            out.remove(p);
        });
    }
    for f in &file.fns {
        for p in &f.signature.generics {
            if let crate::ast::GenericParam::Type(n) | crate::ast::GenericParam::Const(n) = p {
                out.remove(n);
            }
        }
    }
    for f in &facts.fns {
        f.impl_params.iter().for_each(|p| {
            out.remove(p);
        });
    }
    out
}

/// The path a leaf's text is headed by, without its arguments: what the
/// lift read first and did not read. A borrow reads as what it borrows.
fn leaf_head(leaf: &str) -> &str {
    let mut head = leaf.trim_start();
    while let Some(rest) = head.strip_prefix('&') {
        let rest = rest.trim_start();
        head = rest.strip_prefix("mut ").unwrap_or(rest).trim_start();
    }
    let end = head
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(head.len());
    &head[..end]
}

/// The leaf of a type residue that is a chain through the fields of the
/// workspace's types (`Outer.inner: Inner::Some.0: big::BigX`): what follows
/// the last place, `Type.field` or `Type::Variant.field`, the chain passes
/// through. A residue that passes through no place has none.
fn chain_leaf(detail: &str) -> Option<&str> {
    let ident = |s: &str| {
        s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let place = |s: &str| {
        let Some((owner, field)) = s.split_once('.') else {
            return false;
        };
        let owner_ok = match owner.split_once("::") {
            Some((ty, variant)) => ident(ty) && ident(variant),
            None => ident(owner),
        };
        owner_ok
            && !field.is_empty()
            && field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let mut rest = detail;
    let mut through = false;
    while let Some((head, tail)) = rest.split_once(": ") {
        if !place(head) {
            break;
        }
        through = true;
        rest = tail;
    }
    through.then_some(rest)
}

/// How an effect reads after "which": `performs a Fs effect: std::fs::read`.
fn effect_phrase(effect: &str) -> String {
    match effect.strip_prefix("calls ") {
        Some(_) => "reaches an effect".to_string(),
        None => format!("performs a {effect}"),
    }
}

/// The table the frontier's count of refusals is written under: the stage
/// that refused them.
const ADMISSION: &str = "admission";

/// The unit the frontier's refusal queue counts in.
const REFUSALS: &str = "refusals";

/// The projection's `frontier.toml`: the counts and the ranked queues, then
/// every refusal and every function row. `lift` is the register of `rows`,
/// the function rows ([`function_rows`]).
fn frontier(notes: &Notes, lift: &Register, rows: &str, limits: &crate::limits::Limits) -> String {
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
         # unlifted in a later projection.\n\
         # The work queue comes first: [admission] counts the refusals and each\n\
         # [[reason]] row the refusals of one reason; [lift] counts the functions by\n\
         # table and each [[construct]] row the functions one cause and detail stops.\n\
         # Both queues are ranked, what stops the most first.\n\n\
         spec = \"uorust/frontier/1\"\n",
    );
    out.push_str(&limits.frontier_text());
    out.push_str(&refusal_queue(&notes.refusals));
    out.push_str(&lift.queue_text_with(Some(&notes.sites)));
    for r in &notes.refusals {
        out.push_str(&format!(
            "\n[[refusal]]\nstatus = \"open\"\nreason = \"{}\"\nroute = \"{}\"\nspan = {}\ndetail = {}\n",
            r.code(),
            r.route(),
            toml_string(&r.span()),
            toml_string(&r.detail())
        ));
    }
    out.push_str(rows);
    out
}

/// The refusals as the frontier counts and ranks them: the `[admission]`
/// table, then one `[[reason]]` row per reason a refusal carries, with the
/// route that refused it and the refusals of that reason, in the order and
/// the row shape of the lift's queue ([`ranked`], [`queue_row`]).
fn refusal_queue(refusals: &BTreeSet<Refusal>) -> String {
    let mut reasons: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for r in refusals {
        *reasons.entry((r.code(), r.route())).or_default() += 1;
    }
    let mut out = format!("\n[{ADMISSION}]\n{REFUSALS} = {}\n", refusals.len());
    for ((reason, route), count) in ranked(reasons) {
        out.push('\n');
        out.push_str(&queue_row(
            "reason",
            &[("reason", reason), ("route", route)],
            REFUSALS,
            count,
        ));
    }
    out
}

/// The key of the item each function of product code is, as the
/// compiler's dump names it ([`crate::embedding::Key`]), by the function's
/// path: its file's module and the inline modules around it, its impl by
/// the package-relative file and the line and column its header starts at,
/// a trait's provided method under its trait. A path two functions share keys none.
fn embedding_keys(ctx: &Model) -> BTreeMap<String, crate::embedding::Key> {
    let mut keys: BTreeMap<String, Option<crate::embedding::Key>> = BTreeMap::new();
    for r in ctx.all_fns().into_iter().filter(|&r| ctx.product(r)) {
        let (file, f) = (ctx.file(r), ctx.func(r));
        let mut module: Vec<String> = ctx
            .module_of(r)
            .split("::")
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .collect();
        let dir = &ctx.package(r).dir;
        let own = match dir.as_str() {
            "" | "." => file.rel.clone(),
            d => file
                .rel
                .strip_prefix(&format!("{d}/"))
                .unwrap_or(&file.rel)
                .to_string(),
        };
        let impl_at = match (&f.owner, f.in_trait) {
            (Some(trait_name), true) => {
                module.push(trait_name.clone());
                None
            }
            (Some(_), false) => f.impl_at.map(|(line, column)| (own, line, column)),
            (None, _) => None,
        };
        let key = crate::embedding::Key {
            krate: ctx.crate_ident(r),
            module,
            impl_at,
            name: f.name.clone(),
        };
        keys.entry(ctx.path(r))
            .and_modify(|k| *k = None)
            .or_insert(Some(key));
    }
    keys.into_iter()
        .filter_map(|(path, key)| Some((path, key?)))
        .collect()
}

/// The key of each struct and enum of a library or binary target, as the
/// compiler's dump prints it ([`crate::embedding::TypeKey`]), by the
/// type's path `crate::module::Type`.
fn type_keys(ctx: &Model) -> BTreeMap<String, crate::embedding::TypeKey> {
    let mut keys = BTreeMap::new();
    for package in &ctx.ws.packages {
        for file in &package.files {
            let target = &package.targets[file.target];
            if !matches!(target.kind, TargetKind::Lib | TargetKind::Bin) {
                continue;
            }
            let krate = target.name.replace('-', "_");
            let module: Vec<String> = file
                .module
                .split("::")
                .filter(|m| !m.is_empty())
                .map(str::to_string)
                .collect();
            let names = file
                .structs
                .iter()
                .map(|s| (&s.name, &s.fields))
                .chain(file.enums.iter().map(|e| (&e.name, &e.fields)));
            for (name, fields) in names {
                let mut path = vec![krate.clone()];
                path.extend(module.iter().cloned());
                path.push(name.clone());
                keys.insert(
                    path.join("::"),
                    crate::embedding::TypeKey {
                        krate: krate.clone(),
                        module: module.clone(),
                        name: name.clone(),
                        fields: fields.clone(),
                    },
                );
            }
        }
    }
    keys
}

/// The frontier's `[[holding]]` rows: each type of the workspace the
/// embedding's records hold, owned, shared or interior-mutable, with the
/// chains of wrappers it was found in.
fn holding_rows(holdings: &BTreeMap<String, crate::ownership::Held>) -> String {
    let mut out = String::new();
    for (path, held) in holdings {
        out.push_str(&format!(
            "\n[[holding]]\ntype = {}\nholding = \"{}\"\nwithin = [{}]\n",
            toml_string(path),
            held.holding.word(),
            held.within
                .iter()
                .map(|w| toml_string(w))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

/// The frontier's function rows: every function of product code under the
/// table of what the lift decided for it, keyed by its κ. A function that
/// did not lift and is `embedded`-closed is written under that table, with
/// where, in place of its residue, foreign or effect row.
fn function_rows(
    notes: &Notes,
    kappas: &BTreeMap<String, String>,
    embedded: &BTreeMap<String, Vec<crate::register::EmbeddedAt>>,
) -> String {
    let kappa = |path: &str| {
        kappas
            .get(path)
            .map(|k| format!("kappa = \"{k}\"\n"))
            .unwrap_or_default()
    };
    let stops = |path: &str| {
        crate::register::stops_line(notes.stops.get(path).map_or(&[][..], Vec::as_slice))
    };
    let mut out = String::new();
    let spans = notes
        .residue
        .iter()
        .map(|(p, (s, _))| (p, s))
        .chain(notes.foreign.iter().map(|(p, (s, _))| (p, s)))
        .chain(notes.effects.iter().map(|(p, (s, _))| (p, s)));
    for (path, span) in spans {
        if let Some(at) = embedded.get(path) {
            out.push_str(&format!(
                "\n[[{}]]\n{}function = {}\nspan = {}\n{}",
                crate::register::EMBEDDED,
                kappa(path),
                toml_string(path),
                toml_string(&span.render()),
                crate::register::embedded_fields(at)
            ));
        }
    }
    for (path, (span, reason)) in notes
        .residue
        .iter()
        .filter(|(p, _)| !embedded.contains_key(*p))
    {
        out.push_str(&format!(
            "\n[[residue]]\nstatus = \"open\"\n{}function = {}\nspan = {}\nreason = \"{}\"\ndetail = {}\n{}",
            kappa(path),
            toml_string(path),
            toml_string(&span.render()),
            reason.code(),
            toml_string(&reason.detail()),
            stops(path)
        ));
    }
    for (path, (span, detail)) in notes
        .foreign
        .iter()
        .filter(|(p, _)| !embedded.contains_key(*p))
    {
        out.push_str(&format!(
            "\n[[foreign]]\n{}function = {}\nspan = {}\ndetail = {}\n{}",
            kappa(path),
            toml_string(path),
            toml_string(&span.render()),
            toml_string(detail),
            stops(path)
        ));
    }
    for (path, (span, effect)) in notes
        .effects
        .iter()
        .filter(|(p, _)| !embedded.contains_key(*p))
    {
        out.push_str(&format!(
            "\n[[effect]]\n{}function = {}\nspan = {}\neffect = {}\n{}",
            kappa(path),
            toml_string(path),
            toml_string(&span.render()),
            toml_string(effect),
            stops(path)
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

/// Every module of the projection within the `max_file_bytes` its own
/// `lexlean.toml` states, which LexLean refuses a source file beyond
/// (LLS8002): the limit is read back from the project file the same run
/// writes, so the check and the stated limit cannot part. A module beyond it
/// is [`UorustError::ModuleTooLarge`], naming the first such module.
fn within_limit(files: &BTreeMap<String, Vec<u8>>) -> Result<(), UorustError> {
    let at = std::path::Path::new(LEXLEAN);
    let bad = |reason: String| UorustError::Malformed {
        path: at.to_path_buf(),
        reason,
    };
    let text = String::from_utf8_lossy(files.get(LEXLEAN).map_or(&[][..], Vec::as_slice));
    let config: toml::Value = text.parse().map_err(|e| bad(format!("{e}")))?;
    let limit = config
        .get("limits")
        .and_then(|limits| limits.get("max_file_bytes"))
        .and_then(toml::Value::as_integer)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| bad("it states no [limits] max_file_bytes".to_string()))?;
    match files
        .iter()
        .find(|(path, bytes)| path.ends_with(MODULE_SUFFIX) && bytes.len() > limit)
    {
        Some((path, bytes)) => Err(UorustError::ModuleTooLarge {
            module: path.clone(),
            bytes: bytes.len(),
            limit,
        }),
        None => Ok(()),
    }
}

/// What the path of a LexLean module's source ends with.
const MODULE_SUFFIX: &str = ".lex.tex";

/// The modules as they are written, each `(path, text)`: a module within
/// `limit` bytes whole, and a module beyond it as its [`parts`] where its
/// records can be written apart and nothing but the module itself cites one
/// (no other module, and no other file, which `cited` answers for the text
/// `Module::`). A module beyond the limit that cannot be parted is written
/// whole, and [`within_limit`] refuses the projection by its name and size.
fn sized(modules: &[Module], limit: usize, cited: &dyn Fn(&str) -> bool) -> Vec<(String, String)> {
    let texts: Vec<String> = modules.iter().map(Module::render).collect();
    let taken: BTreeSet<&str> = modules.iter().map(|m| m.name.as_str()).collect();
    let mut out = Vec::new();
    for (i, (module, text)) in modules.iter().zip(&texts).enumerate() {
        if text.len() > limit {
            let id = format!("{}::", module.name);
            let elsewhere = cited(&id)
                || texts
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && other.contains(&id));
            let parted = (!elsewhere).then(|| parts(module, limit, &taken)).flatten();
            if let Some(parted) = parted {
                out.extend(parted.iter().map(|part| (part.path(), part.render())));
                continue;
            }
        }
        out.push((module.path(), text.clone()));
    }
    out
}

/// The parts a module is written as when it is beyond `limit` bytes: its
/// declarations in their order, each part the longest run of them that
/// stays within the limit. The first part keeps the module's name and the
/// `n`th is named `{Module}{n}`, so a record's ID (`Module::name`, which
/// PrismPM reads from the module that declares it) names its part, and every
/// reference to a record of the module is rewritten to the part it is in.
///
/// Only a module of separate records is parted: every declaration a closed
/// value in which no word outside a string is another declaration's name
/// (the term is not read further, so a field or a constructor spelled like
/// one counts as naming it), and none carrying a canonical `index`, which
/// PrismPM derives from the order of the IDs a parting changes. `None` where
/// the module is not one, where one record alone is beyond the limit, or
/// where a part's name is `taken`.
fn parts(module: &Module, limit: usize, taken: &BTreeSet<&str>) -> Option<Vec<Module>> {
    let names: BTreeSet<&str> = module.decls.iter().map(Decl::name).collect();
    let mut records = Vec::new();
    for decl in &module.decls {
        let Decl::Definition {
            type_parameters,
            name,
            parameters,
            result,
            body: Expr::Flat(body),
            axioms,
            recursive: None,
            termination: None,
            mutual: None,
        } = decl
        else {
            return None;
        };
        let indexed = body.contains("{index := ") || body.contains(", index := ");
        if !parameters.is_empty()
            || !type_parameters.is_empty()
            || !axioms.is_empty()
            || indexed
            || words(body).any(|word| names.contains(word))
        {
            return None;
        }
        records.push((name, result, body));
    }
    let part = |decls: Vec<Decl>, name: String| Module {
        name,
        glossaries: module.glossaries.clone(),
        imports: module.imports.clone(),
        title: module.title,
        decls,
    };
    // The size of each record under the longest name a part can have: there
    // are no more parts than records, so no part's number is wider than
    // their count. A shorter name never prints a record larger, since a
    // line that fits stays flat.
    let widest = format!("{}{}", module.name, module.decls.len());
    let room = limit.checked_sub(part(Vec::new(), widest.clone()).render().len() + "\n".len())?;
    let between = "\n\n".len();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut used = 0;
    for (i, (name, result, body)) in records.iter().enumerate() {
        let widened = renamed(body, &module.name, &|n| {
            names.contains(n).then_some(widest.as_str())
        });
        let size = crate::lex::render_decl(&Decl::value(*name, *result, widened)).len();
        match groups.last_mut() {
            Some(group) if used + between + size <= room => {
                group.push(i);
                used += between + size;
            }
            _ if size <= room => {
                groups.push(vec![i]);
                used = size;
            }
            _ => return None,
        }
    }
    let part_names: Vec<String> = (0..groups.len())
        .map(|k| match k {
            0 => module.name.clone(),
            _ => format!("{}{}", module.name, k + 1),
        })
        .collect();
    if part_names[1..].iter().any(|n| taken.contains(n.as_str())) {
        return None;
    }
    let part_of: BTreeMap<&str, &str> = groups
        .iter()
        .zip(&part_names)
        .flat_map(|(group, part)| {
            group
                .iter()
                .map(|&i| (records[i].0.as_str(), part.as_str()))
        })
        .collect();
    Some(
        groups
            .iter()
            .zip(&part_names)
            .map(|(group, name)| {
                let decls = group
                    .iter()
                    .map(|&i| {
                        let (record, result, body) = records[i];
                        let body = renamed(body, &module.name, &|n| part_of.get(n).copied());
                        Decl::value(record, result, body)
                    })
                    .collect();
                part(decls, name.clone())
            })
            .collect(),
    )
}

/// `text` with every string that is an ID of `module` (`"Module::name"`)
/// naming the module `part_of` gives for its `name`; an ID it gives none for
/// stays as it is.
fn renamed<'a>(text: &str, module: &str, part_of: &dyn Fn(&str) -> Option<&'a str>) -> String {
    let opening = format!("\"{module}::");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&opening) {
        let after = &rest[at + opening.len()..];
        let name = &after[..after.find('"').unwrap_or(after.len())];
        out.push_str(&rest[..at]);
        match part_of(name) {
            Some(part) => out.push_str(&format!("\"{part}::")),
            None => out.push_str(&opening),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The words of a term outside its string literals: each run of letters,
/// digits and underscores, the shape of a declaration's name.
fn words(text: &str) -> impl Iterator<Item = &str> {
    let (mut quoted, mut escaped) = (false, false);
    text.split(move |c: char| {
        let inside = quoted;
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            _ => {}
        }
        inside || !(c.is_alphanumeric() || c == '_')
    })
    .filter(|word| !word.is_empty())
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
    #[test]
    fn shared_kernel_observations_survive_owner_order_ue_80() {
        let kernel = Decl::Theorem {
            name: "sharedKernel".into(),
            type_parameters: Vec::new(),
            parameters: Vec::new(),
            statement: "true = true".into(),
            proof: "decide{}".into(),
            axioms: vec!["propext".into()],
        };
        let observed = BTreeMap::from([(
            "sharedKernel".into(),
            vec![
                "Classical.choice".into(),
                "Quot.sound".into(),
                "propext".into(),
            ],
        )]);
        let empty = BTreeMap::new();
        for order in [[&empty, &observed], [&observed, &empty]] {
            let mut aux = BTreeMap::new();
            for hints in order {
                super::insert_observed_aux(&mut aux, "sharedKernel", &kernel, hints, &empty);
            }
            assert_eq!(
                crate::lex::render_decl(&aux["sharedKernel"]),
                crate::lex::render_decl(&super::with_axioms(kernel.clone(), &observed))
            );
        }
        let mut hinted = BTreeMap::new();
        super::insert_observed_aux(&mut hinted, "sharedKernel", &kernel, &empty, &observed);
        assert_eq!(
            crate::lex::render_decl(&hinted["sharedKernel"]),
            crate::lex::render_decl(&super::with_axioms(kernel.clone(), &observed))
        );
        let exact_empty = BTreeMap::from([("sharedKernel".into(), Vec::new())]);
        super::insert_observed_aux(
            &mut hinted,
            "sharedKernel",
            &kernel,
            &exact_empty,
            &observed,
        );
        assert_eq!(
            crate::lex::render_decl(&hinted["sharedKernel"]),
            crate::lex::render_decl(&super::with_axioms(kernel.clone(), &exact_empty))
        );
        let mut different = kernel.clone();
        if let Decl::Theorem { statement, .. } = &mut different {
            *statement = "false = false".into();
        }
        let mut aux = BTreeMap::new();
        super::insert_observed_aux(&mut aux, "sharedKernel", &different, &empty, &empty);
        super::insert_observed_aux(&mut aux, "sharedKernel", &kernel, &observed, &empty);
        assert_eq!(
            crate::lex::render_decl(&aux["sharedKernel"]),
            crate::lex::render_decl(&different)
        );
    }

    #[test]
    fn merged_dependencies_keep_mutual_types_together_ue_106() {
        let data = |name: &str, field: &str| Decl::Data {
            name: name.into(),
            type_parameters: Vec::new(),
            constructors: vec![("mk".into(), vec![field.into()])],
            mutual: Some("Nodes".into()),
        };
        let consumer = Decl::Structure {
            name: "Packet".into(),
            type_parameters: Vec::new(),
            fields: vec![("item".into(), "Left".into())],
        };
        let sorted = super::surface_dependency_order(vec![
            data("Left", "(Option Right)"),
            consumer,
            data("Right", "(List Left)"),
        ]);
        let names: Vec<_> = sorted.iter().map(Decl::name).collect();
        assert_eq!(names, ["Left", "Right", "Packet"]);
    }

    #[test]
    fn invalid_declaration_graphs_survive_ordering_ue_106() {
        let def = |name: &str, body: &str| Decl::Definition {
            type_parameters: Vec::new(),
            name: name.into(),
            parameters: Vec::new(),
            result: "Bool".into(),
            body: crate::lex::Expr::Flat(body.into()),
            axioms: Vec::new(),
            recursive: None,
            termination: None,
            mutual: None,
        };
        for input in [
            vec![def("left", "(right)"), def("right", "(left)")],
            vec![def("missing", "(absent)")],
            vec![def("collision", "true"), def("collision", "false")],
        ] {
            let before: Vec<_> = input.iter().map(crate::lex::render_decl).collect();
            let after: Vec<_> = super::surface_dependency_order(input)
                .iter()
                .map(crate::lex::render_decl)
                .collect();
            assert_eq!(before, after, "invalid graph must reach SDK unchanged");
        }
    }

    #[test]
    fn declaration_dependencies_include_function_values_ue_91() {
        use super::{surface_dependency_order, Decl};
        let def = |name: &str, body: &str| Decl::Definition {
            type_parameters: Vec::new(),
            name: name.into(),
            parameters: Vec::new(),
            result: "Bool".into(),
            body: crate::lex::Expr::Flat(body.into()),
            axioms: Vec::new(),
            recursive: None,
            termination: None,
            mutual: None,
        };
        let ordered = surface_dependency_order(vec![
            def("early", "(ref late)"),
            def("quoted", r#""(ref early)""#),
            def("late", "true"),
        ]);
        let names: Vec<_> = ordered.iter().map(Decl::name).collect();
        assert!(names.iter().position(|n| *n == "late") < names.iter().position(|n| *n == "early"));
        assert_eq!(names[0], "quoted");
    }

    #[test]
    fn surface_dependencies_cross_files_without_splitting_mutual_groups_ue_25() {
        let def = |name: &str, body: &str, mutual: Option<&str>| crate::lex::Decl::Definition {
            type_parameters: Vec::new(),
            name: name.into(),
            parameters: Vec::new(),
            result: "String".into(),
            body: crate::lex::Expr::Flat(body.into()),
            axioms: Vec::new(),
            recursive: None,
            termination: None,
            mutual: mutual.map(String::from),
        };
        let input = vec![
            def("consumer", "(later)", None),
            def("left", "(right)", Some("pair")),
            def("after", "(left)", None),
            def("right", "(base)", Some("pair")),
            def("literal", r#""(consumer) \" (later)""#, None),
            def("later", "(base)", None),
            def("base", r#""value""#, None),
        ];
        let ordered = super::surface_dependency_order(input);
        let names: Vec<_> = ordered.iter().map(crate::lex::Decl::name).collect();
        let position = |name| names.iter().position(|n| *n == name).unwrap();
        assert!(position("base") < position("later"));
        assert!(position("later") < position("consumer"));
        assert!(position("base") < position("right"));
        assert_eq!(position("left") + 1, position("right"));
        assert!(position("right") < position("after"));
        assert!(
            position("literal") < position("base"),
            "strings create no dependency"
        );
    }
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

    // Types outside the lift.

    /// A type construct of the work queue carries the sites that define the
    /// types it writes: the struct of the chain at the line of its name, in
    /// whichever file declares it, and nothing for a type of the standard
    /// library. Planted: a site read off by a line, or a chain read without
    /// its owner, fails here.
    #[test]
    fn register_a_type_construct_names_the_sites_that_define_it() {
        let frontier = frontier_of(
            "defined",
            &[
                (
                    "src/lib.rs",
                    "//! D.\n\npub mod holder;\n\nuse holder::Holder;\n\n/// Holds a holder.\npub struct Outer {\n    /// The holder.\n    pub held: Holder,\n}\n\n/// Reads it.\npub fn read(o: &Outer) -> u8 {\n    unsafe { *o.held.cell.get() }\n}\n\n/// Reads one by its path.\npub fn read_at(h: &holder::Holder) -> u8 {\n    unsafe { *h.cell.get() }\n}\n",
                ),
                (
                    "src/holder.rs",
                    "//! H.\n\n/// Holds a cell.\npub struct Holder {\n    /// The cell.\n    pub cell: std::cell::UnsafeCell<u8>,\n}\n",
                ),
            ],
        );
        let rows: std::collections::BTreeMap<&str, &str> = frontier
            .split("\n[[construct]]\n")
            .skip(1)
            .map(|row| row.split("\n\n").next().unwrap_or_default())
            .filter(|row| row.contains("\ncause = \"type\"\n"))
            .filter_map(|row| {
                let construct = row.lines().next()?.strip_prefix("construct = ")?;
                let defined = row.lines().find_map(|l| l.strip_prefix("defined = "))?;
                Some((construct, defined))
            })
            .collect();
        // The chain from the struct of this file down to the cell, and the
        // struct of the other file the path names: each site at the line of
        // the type's name, in the chain's order.
        let chained = rows
            .iter()
            .find(|(construct, _)| construct.contains("Outer.held"))
            .map(|(_, defined)| *defined);
        assert_eq!(
            chained,
            Some("[\"src/lib.rs:8\", \"src/holder.rs:4\"]"),
            "{frontier}"
        );
        let named = rows
            .iter()
            .find(|(construct, _)| !construct.contains("Outer") && construct.contains("Holder"))
            .map(|(_, defined)| *defined);
        assert_eq!(named, Some("[\"src/holder.rs:4\"]"), "{frontier}");
        // The register reads such a frontier as it reads any other.
        let lift = crate::register::Register::read(Path::new(FRONTIER), &frontier);
        assert!(lift.is_ok(), "{lift:?}");
    }

    /// The register of a one-crate workspace of these files, as a
    /// projection's frontier states it.
    fn register_kai_of(name: &str, files: &[(&str, &str)]) -> crate::register::Register {
        let frontier = frontier_of(name, files);
        crate::register::Register::read(Path::new(FRONTIER), &frontier).expect("a register")
    }

    /// The refuter's first case of the law per function: only P changes,
    /// and `uses_pq` (its tokens the same) is now counted under P's field
    /// while Q's still stops it, though Q's stops no other function. The
    /// register records every construct of its signature, so the move is
    /// refused. Planted: a register that keeps only the first construct
    /// (`stops` empty) is refused for want of evidence, never admitted.
    #[test]
    fn register_kai_a_move_while_the_parents_construct_still_applies_is_refused() {
        let parent = register_kai_of("kaitwo", &[("src/lib.rs", W2P)]);
        let candidate = register_kai_of("kaitwo", &[("src/lib.rs", W2C)]);
        let refused = crate::register::admit_parent(&parent, &candidate);
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(
            refused[0].starts_with("kaitwo::uses_pq ")
                && refused[0].contains("Q.c: UnsafeCell<u8>"),
            "{refused:?}"
        );
        let row = candidate
            .rows
            .values()
            .find(|r| r.function == "kaitwo::uses_pq")
            .expect("its row");
        let stops: Vec<&str> = row
            .stops
            .iter()
            .flatten()
            .map(|(_, c)| c.as_str())
            .collect();
        assert_eq!(
            stops,
            [
                "type outside the lift: P.v: UnsafeCell<u8>",
                "type outside the lift: Q.c: UnsafeCell<u8>"
            ]
        );
    }

    /// The refuter's second case: `Alpha::make` lifts, and `fa` (its tokens
    /// the same) moves on to its next call, while `Beta::make`, of the same
    /// last segment, is still residue. A call construct names its callee by
    /// its path, so the two are two constructs and the advance is admitted.
    #[test]
    fn register_kai_a_callee_that_lifts_lets_its_caller_move_on() {
        let parent = register_kai_of("kaisix", &[("src/lib.rs", W6P)]);
        let candidate = register_kai_of("kaisix", &[("src/lib.rs", W6C)]);
        let construct = |r: &crate::register::Register, f: &str| {
            r.rows
                .values()
                .find(|row| row.function == f)
                .and_then(|row| row.status.construct().map(|(_, c)| c.to_string()))
        };
        assert_eq!(
            construct(&parent, "kaisix::fa").as_deref(),
            Some("calls kaisix::Alpha::make, which is not lifted")
        );
        assert_eq!(
            construct(&parent, "kaisix::fb").as_deref(),
            Some("calls kaisix::Beta::make, which is not lifted")
        );
        assert_eq!(
            construct(&candidate, "kaisix::fa").as_deref(),
            Some("calls kaisix::k, which is not lifted")
        );
        let refused = crate::register::admit_parent(&parent, &candidate);
        assert!(refused.is_empty(), "{refused:?}");
    }

    /// The refuter's third case: an inline module adds a homonym of a
    /// lifted root function. Each function keeps a row of its own path,
    /// so the root's κ stays lifted and the register counts both.
    #[test]
    fn register_kai_an_inline_homonym_is_a_row_of_its_own() {
        let parent = register_kai_of("kaieight", &[("src/lib.rs", W8P)]);
        let candidate = register_kai_of("kaieight", &[("src/lib.rs", W8C)]);
        let functions: Vec<&str> = candidate
            .rows
            .values()
            .map(|r| r.function.as_str())
            .collect();
        assert_eq!(functions.len(), 4, "{functions:?}");
        assert!(functions.contains(&"kaieight::m::h"), "{functions:?}");
        for (kappa, row) in &parent.rows {
            // The root `h` now shares its name with `m::h`, an item of its
            // own, so its definition takes the qualified name (lane amb2's
            // naming of homonyms); it is lifted as before.
            let expected = match &row.status {
                crate::register::Status::Lifted(d) if d == "Behavior::h" => {
                    crate::register::Status::Lifted("Behavior::kaieightH".into())
                }
                other => other.clone(),
            };
            assert_eq!(
                candidate.rows.get(kappa).map(|r| &r.status),
                Some(&expected)
            );
        }
        assert!(crate::register::admit_parent(&parent, &candidate).is_empty());
    }

    /// The refuter's sites: each type construct names the declarations its
    /// chain means, read where the chain writes each name. Two homonyms of
    /// two inline modules each have their own; std's `UnsafeCell` has none,
    /// though the workspace declares a `UnsafeCell` elsewhere; a renaming
    /// `use` leads to the type it renames; a type a macro invocation writes
    /// is sited at the invocation; a type the projection renamed is sited
    /// by its new name.
    #[test]
    fn register_kai_a_type_construct_is_sited_where_its_names_are_written() {
        let sites = |frontier: &str| -> std::collections::BTreeMap<String, (String, String)> {
            frontier
                .split("\n[[construct]]\n")
                .skip(1)
                .map(|row| row.split("\n\n").next().unwrap_or_default())
                .filter(|row| row.contains("\ncause = \"type\"\n"))
                .filter_map(|row| {
                    let line = |key: &str| {
                        row.lines()
                            .find_map(|l| l.strip_prefix(key))
                            .map(str::to_string)
                    };
                    let construct = line("construct = ")?;
                    Some((construct, (line("defined = ")?, line("unsited = ")?)))
                })
                .collect()
        };
        let one = sites(&frontier_of("kaisites", &[("src/lib.rs", W1)]));
        let at = |construct: &str| {
            one.get(&format!("\"type outside the lift: {construct}\""))
                .cloned()
                .unwrap_or_else(|| panic!("{construct} in {one:?}"))
        };
        let none = "[]".to_string();
        assert_eq!(
            at("KaisitesANode.c: UnsafeCell<u8>"),
            ("[\"src/lib.rs:25\"]".into(), none.clone())
        );
        assert_eq!(
            at("KaisitesBNode.c: UnsafeCell<u8>"),
            ("[\"src/lib.rs:34\"]".into(), none.clone())
        );
        assert_eq!(
            at("Q.c: UnsafeCell<u8>"),
            ("[\"src/lib.rs:11\"]".into(), none.clone())
        );
        assert_eq!(
            at("Deep.c: UnsafeCell<u16>"),
            ("[\"src/lib.rs:75\"]".into(), none.clone())
        );
        assert_eq!(at("Made"), ("[\"src/lib.rs:65\"]".into(), none.clone()));
        let four = sites(&frontier_of(
            "kaifour",
            &[("src/lib.rs", W4L), ("src/a.rs", W4A), ("src/b.rs", W4B)],
        ));
        let renamed: Vec<&(String, String)> = four
            .iter()
            .filter(|(c, _)| c.contains("Node.c"))
            .map(|(_, s)| s)
            .collect();
        assert_eq!(renamed.len(), 2, "{four:?}");
        assert!(
            renamed.contains(&&("[\"src/a.rs:6\"]".to_string(), none.clone())),
            "{four:?}"
        );
        assert!(
            renamed.contains(&&("[\"src/b.rs:5\"]".to_string(), none)),
            "{four:?}"
        );
    }

    /// A synthetic crate of the refuter of the register lane.
    const W2P: &str = r##"//! Synthetic two.
use std::cell::UnsafeCell;

/// P.
pub struct P {
    /// v.
    pub v: u8,
}

/// Q: outside the lift.
pub struct Q {
    /// c.
    pub c: UnsafeCell<u8>,
}

/// Uses P then Q.
pub fn uses_pq(p: &P, q: &Q) -> u8 {
    let _ = p;
    *q.c.borrow()
}

/// Other: outside.
pub struct Other {
    /// c.
    pub c: UnsafeCell<i8>,
}

/// a.
pub mod a {
    use std::cell::UnsafeCell;
    /// Node a.
    pub struct Node {
        /// c.
        pub c: UnsafeCell<u8>,
    }
    /// Uses Node, then Other.
    pub fn fa(n: &Node, o: &super::Other) -> u8 {
        let _ = n;
        *o.c.borrow() as u8
    }
}
/// b.
pub mod b {
    use std::cell::UnsafeCell;
    /// Node b.
    pub struct Node {
        /// c.
        pub c: UnsafeCell<u8>,
    }
    /// Uses Node.
    pub fn fb(n: &Node) -> u8 {
        *n.c.borrow()
    }
}

/// One
pub fn ident(x: u64) -> u64 {
    x
}
"##;

    /// A synthetic crate of the refuter of the register lane.
    const W2C: &str = r##"//! Synthetic two.
use std::cell::UnsafeCell;

/// P.
pub struct P {
    /// v.
    pub v: UnsafeCell<u8>,
}

/// Q: outside the lift.
pub struct Q {
    /// c.
    pub c: UnsafeCell<u8>,
}

/// Uses P then Q.
pub fn uses_pq(p: &P, q: &Q) -> u8 {
    let _ = p;
    *q.c.borrow()
}

/// Other: outside.
pub struct Other {
    /// c.
    pub c: UnsafeCell<i8>,
}

/// a.
pub mod a {
    use std::cell::UnsafeCell;
    /// Node a.
    pub struct Node {
        /// c.
        pub c: u8,
    }
    /// Uses Node, then Other.
    pub fn fa(n: &Node, o: &super::Other) -> u8 {
        let _ = n;
        *o.c.borrow() as u8
    }
}
/// b.
pub mod b {
    use std::cell::UnsafeCell;
    /// Node b.
    pub struct Node {
        /// c.
        pub c: UnsafeCell<u8>,
    }
    /// Uses Node.
    pub fn fb(n: &Node) -> u8 {
        *n.c.borrow()
    }
}

/// Two
pub fn ident(x: u64) -> u64 {
    x
}
"##;

    /// A synthetic crate of the refuter of the register lane.
    const W6P: &str = r##"//! Synthetic six.

/// Alpha.
pub struct Alpha {
    /// v.
    pub v: u8,
}

impl Alpha {
    /// make of Alpha.
    pub fn make(x: u8) -> u8 {
        let mut y = x;
        while y > 3 {
            y -= 1;
        }
        y
    }
}

/// Beta.
pub struct Beta {
    /// v.
    pub v: u8,
}

impl Beta {
    /// make of Beta.
    pub fn make(x: u8) -> u8 {
        let mut y = x;
        while y > 3 {
            y -= 1;
        }
        y
    }
}

/// Residue k.
pub fn k(x: u8) -> u8 {
    let mut y = x;
    while y > 5 {
        y -= 1;
    }
    y
}

/// Calls Alpha::make, then k.
pub fn fa(x: u8) -> u8 {
    let y = Alpha::make(x);
    k(y)
}

/// Calls Beta::make.
pub fn fb(x: u8) -> u8 {
    let y = Beta::make(x);
    y
}
"##;

    /// A synthetic crate of the refuter of the register lane.
    const W6C: &str = r##"//! Synthetic six.

/// Alpha.
pub struct Alpha {
    /// v.
    pub v: u8,
}

impl Alpha {
    /// make of Alpha.
    pub fn make(x: u8) -> u8 {
        if x > 3 { 3 } else { x }
    }
}

/// Beta.
pub struct Beta {
    /// v.
    pub v: u8,
}

impl Beta {
    /// make of Beta.
    pub fn make(x: u8) -> u8 {
        let mut y = x;
        while y > 3 {
            y -= 1;
        }
        y
    }
}

/// Residue k.
pub fn k(x: u8) -> u8 {
    let mut y = x;
    while y > 5 {
        y -= 1;
    }
    y
}

/// Calls Alpha::make, then k.
pub fn fa(x: u8) -> u8 {
    let y = Alpha::make(x);
    k(y)
}

/// Calls Beta::make.
pub fn fb(x: u8) -> u8 {
    let y = Beta::make(x);
    y
}
"##;

    /// A synthetic crate of the refuter of the register lane.
    const W8P: &str = r##"//! Synthetic eight.

/// P.
pub struct P {
    /// v.
    pub v: u8,
}

/// Lifted: reads v.
pub fn getv(p: &P) -> u8 {
    p.v
}

/// Lifted: identity.
pub fn h(x: u8) -> u8 {
    x
}

"##;

    /// A synthetic crate of the refuter of the register lane.
    const W8C: &str = r##"//! Synthetic eight.

/// P.
pub struct P {
    /// v.
    pub v: u8,
}

/// Lifted: reads v.
pub fn getv(p: &P) -> u8 {
    p.v
}

/// Lifted: identity.
pub fn h(x: u8) -> u8 {
    x
}

/// An inline module with a homonym of h.
pub mod m {
    /// h of m.
    pub fn h(x: u8) -> u8 {
        let mut y = x;
        while y > 3 {
            y -= 1;
        }
        y
    }
}

/// Calls the root h.
pub fn calls_root(x: u8) -> u8 {
    h(x)
}
"##;

    /// A synthetic crate of the refuter of the register lane.
    const W1: &str = r##"//! Synthetic.
use std::cell::UnsafeCell;

/// P: liftable in v1.
pub struct P {
    /// v.
    pub v: u8,
}

/// Q: outside the lift.
pub struct Q {
    /// c.
    pub c: UnsafeCell<u8>,
}

/// Uses P then Q.
pub fn uses_pq(p: &P, q: &Q) -> u8 {
    p.v + *q.c.borrow()
}

/// Inline modules with homonym types.
pub mod a {
    use std::cell::UnsafeCell;
    /// Node a.
    pub struct Node {
        /// c.
        pub c: UnsafeCell<u8>,
    }
}
/// b.
pub mod b {
    use std::cell::UnsafeCell;
    /// Node b.
    pub struct Node {
        /// c.
        pub c: UnsafeCell<u8>,
    }
}

/// Other: outside.
pub struct Other {
    /// c.
    pub c: UnsafeCell<i8>,
}

/// Uses a::Node, then Other.
pub fn fa(n: &a::Node, o: &Other) -> u8 {
    *n.c.borrow() + (*o.c.borrow() as u8)
}

/// Uses b::Node.
pub fn fb(n: &b::Node) -> u8 {
    *n.c.borrow()
}

macro_rules! mk {
    ($n:ident) => {
        /// made.
        pub struct $n {
            /// c.
            pub c: UnsafeCell<u8>,
        }
    };
}
mk!(Made);

/// Uses Made.
pub fn fm(m: &Made) -> u8 {
    *m.c.borrow()
}

mod inner {
    use std::cell::UnsafeCell;
    /// Deep.
    pub struct Deep {
        /// c.
        pub c: UnsafeCell<u16>,
    }
}
pub use inner::Deep as Alias;

/// Uses the alias.
pub fn fr(x: &Alias) -> u16 {
    *x.c.borrow()
}

/// Generic.
pub struct Wrap<T> {
    /// v.
    pub v: T,
}

/// Uses Wrap.
pub fn fw(w: &Wrap<u8>) -> u8 {
    w.v
}

/// Twin under x.
#[cfg(feature = "x")]
pub struct Twin {
    /// c.
    pub c: UnsafeCell<u8>,
}
/// Twin otherwise.
#[cfg(not(feature = "x"))]
pub struct Twin {
    /// c.
    pub c: std::cell::Cell<u8>,
}

/// Uses Twin.
pub fn ft(t: &Twin) -> u8 {
    #[cfg(feature = "x")]
    return *t.c.borrow();
    #[cfg(not(feature = "x"))]
    return t.c.get();
}

/// Unrelated module that declares its own UnsafeCell.
pub mod mine {
    /// A homonym of std's.
    pub struct UnsafeCell {
        /// v.
        pub v: u8,
    }
}

"##;

    /// A synthetic crate of the refuter of the register lane.
    const W4L: &str = r##"//! Synthetic four.
use std::cell::UnsafeCell;

pub mod a;
pub mod b;

/// Other: outside.
pub struct Other {
    /// c.
    pub c: UnsafeCell<i8>,
}
"##;

    /// A synthetic crate of the refuter of the register lane.
    const W4A: &str = r##"//! a.
#[allow(unused_imports)]
use std::cell::UnsafeCell;

/// Node of a.
pub struct Node {
    /// c.
    pub c: UnsafeCell<u8>,
}

/// Uses Node, then Other.
pub fn fa(n: &Node, o: &crate::Other) -> u8 {
    let _ = n;
    *o.c.borrow() as u8
}
"##;

    /// A synthetic crate of the refuter of the register lane.
    const W4B: &str = r##"//! b.
use std::cell::UnsafeCell;

/// Node of b.
pub struct Node {
    /// c.
    pub c: UnsafeCell<u8>,
}

/// Uses Node.
pub fn fb(n: &Node) -> u8 {
    *n.c.borrow()
}
"##;

    /// The frontier of a one-crate workspace of these files, written under
    /// the system's temporary directory and projected.
    fn frontier_of(name: &str, files: &[(&str, &str)]) -> String {
        let dir = std::env::temp_dir().join(format!("uorust-lib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let manifest =
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n");
        for (path, body) in [("Cargo.toml", manifest.as_str())].iter().chain(files) {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, body).expect("a file");
        }
        let options = super::Options {
            name: name.to_string(),
            last_modified: "2026-11-11T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let projected = crate::project_workspace(&dir, &options).expect("a projection");
        let _ = std::fs::remove_dir_all(&dir);
        String::from_utf8(projected["frontier.toml"].clone()).expect("text")
    }

    /// The rows of a frontier that are of this function: each one's table
    /// and the line of it that starts with this key.
    fn rows_of(frontier: &str, function: &str, key: &str) -> Vec<(String, String)> {
        frontier
            .split("\n[[")
            .filter(|row| row.contains(&format!("\nfunction = \"{function}\"\n")))
            .map(|row| {
                let table = row.split("]]").next().unwrap_or_default().to_string();
                let line = row
                    .lines()
                    .find_map(|l| l.strip_prefix(&format!("{key} = ")))
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string();
                (table, line)
            })
            .collect()
    }

    const NAMES: &[(&str, &str)] = &[
        (
            "src/lib.rs",
            "//! Names that read as one type, as none, or as a leaf.\n\
             pub mod first;\npub mod reader;\npub mod second;\npub mod teller;\n\n\
             /// One type for each build configuration.\n#[cfg(unix)]\n\
             pub struct Handle {\n    /// A descriptor.\n    pub fd: u32,\n}\n\n\
             /// One type for each build configuration.\n#[cfg(not(unix))]\n\
             pub struct Handle {\n    /// A raw handle.\n    pub raw: u64,\n}\n\n\
             /// Stopped by a name two declarations share.\n\
             pub fn opened(h: &Handle) -> bool {\n    let _ = h;\n    true\n}\n\n\
             /// A cell.\npub struct Leaf {\n    /// The cell.\n    pub x: UnsafeCell<u8>,\n}\n\n\
             /// Holds the leaf.\npub struct Mid {\n    /// A count.\n    pub n: u32,\n    \
             /// The leaf.\n    pub leaf: Leaf,\n}\n\n\
             impl Mid {\n    /// Stopped by the type it is a method of.\n    \
             pub fn count(&self) -> u32 {\n        self.n\n    }\n}\n\n\
             /// A type named as LexLean names one of its own.\n\
             pub struct Prop {\n    /// A power.\n    pub power: u8,\n}\n\n\
             /// Reads it.\npub fn power(p: &Prop) -> u8 {\n    p.power\n}\n\n\
             /// Two traits give one type a method of one name.\n\
             pub struct Label {\n    /// The text.\n    pub text: String,\n}\n\n\
             impl std::fmt::Display for Label {\n    \
             fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        \
             write!(f, \"{}\", self.text)\n    }\n}\n\n\
             impl std::fmt::Debug for Label {\n    \
             fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        \
             f.write_str(\"label\")\n    }\n}\n\n\
             /// One function for each build configuration.\n#[cfg(unix)]\n\
             pub fn width() -> u32 {\n    1\n}\n\n\
             /// One function for each build configuration.\n#[cfg(not(unix))]\n\
             pub fn width(scale: UnsafeCell<u8>) -> u32 {\n    let _ = scale;\n    2\n}\n",
        ),
        (
            "src/first.rs",
            "//! The first of two modules that declare a type of one name.\n\n\
             /// A shape with sides.\npub struct Shape {\n    /// The sides.\n    pub sides: u32,\n}\n\n\
             /// Reads the shape its own module declares.\n\
             pub fn sides(s: &Shape) -> u32 {\n    s.sides\n}\n",
        ),
        (
            "src/second.rs",
            "//! The second of two modules that declare a type of one name.\n\n\
             /// A shape with a radius.\npub struct Shape {\n    /// The radius.\n    pub radius: u32,\n}\n\n\
             /// Reads the shape its own module declares.\n\
             pub fn radius(s: &Shape) -> u32 {\n    s.radius\n}\n",
        ),
        (
            "src/teller.rs",
            "//! A file that says which of the two shapes it means.\n\n\
             use crate::second::Shape;\n\n\
             /// Reads the shape its import names.\n\
             pub fn round(s: &Shape) -> u32 {\n    s.radius\n}\n",
        ),
        (
            "src/reader.rs",
            "//! A file that imports one of the two shapes by a glob.\n\n\
             use crate::first::*;\n\n\
             /// Reads the shape its glob imports.\n\
             pub fn corners(s: &Shape) -> u32 {\n    s.sides\n}\n",
        ),
    ];

    #[test]
    fn a_name_that_reads_as_no_one_type_states_why() {
        let frontier = frontier_of("names", NAMES);
        let detail = |function: &str| rows_of(&frontier, function, "detail");
        let residue = |why: String| vec![("residue".to_string(), why)];
        // One name, two declarations in one module: neither is the type.
        assert_eq!(
            detail("names::opened"),
            residue(format!(
                "type outside the lift: Handle: {}",
                crate::lift::DECLARED_TWICE
            ))
        );
        // One name, two modules: the module that declares one, the file
        // that imports one and the file whose glob imports one read it.
        for told in [
            "names::first::sides",
            "names::second::radius",
            "names::teller::round",
            "names::reader::corners",
        ] {
            let rows = rows_of(&frontier, told, "definition");
            assert_eq!(rows.len(), 1, "{told}: {rows:?}");
            assert_eq!(rows[0].0, "lifted", "{told}");
        }
    }

    #[test]
    fn a_method_of_a_type_outside_the_lift_names_the_leaf() {
        let frontier = frontier_of("owners", NAMES);
        assert_eq!(
            rows_of(&frontier, "owners::Mid::count", "detail"),
            vec![(
                "residue".to_string(),
                "type outside the lift: Mid.leaf: Leaf.x: UnsafeCell<u8>".to_string()
            )]
        );
    }

    #[test]
    fn a_type_named_as_lexlean_names_its_own_lifts_under_its_crate() {
        let frontier = frontier_of("taken", NAMES);
        let rows = rows_of(&frontier, "taken::power", "definition");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "lifted");
    }

    #[test]
    fn two_functions_of_one_path_share_one_row() {
        let frontier = frontier_of("paths", NAMES);
        let rows = rows_of(&frontier, "paths::width", "detail");
        assert_eq!(rows.len(), 1, "{rows:?}");
        // The row is the one that claims least: the function not lifted.
        assert_eq!(rows[0].0, "residue");
        // Two traits' methods of one name on one type are two paths, each
        // with its own row.
        for path in [
            "paths::<Label as Display>::fmt",
            "paths::<Label as Debug>::fmt",
        ] {
            let rows = rows_of(&frontier, path, "function");
            assert_eq!(rows.len(), 1, "{path}: {rows:?}");
        }
    }

    // Functions refused by their signature or misread by where they stand:
    // a trait implementation's method, a default method, a method of a type
    // outside the lift, and a module declared out of line under a test cfg.
    // Each projects a crate written for the test.

    /// The frontier and the behaviour module of the crate `name` of `files`.
    fn projected(name: &str, files: &[(&str, &str)]) -> (String, String) {
        let dir = std::env::temp_dir().join(format!("uorust-stand-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let manifest =
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        let manifest = ("Cargo.toml", manifest.as_str());
        for (path, body) in std::iter::once(&manifest).chain(files) {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(path, body).expect("write");
        }
        let options = crate::Options {
            name: name.to_string(),
            last_modified: "2026-11-11T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let out = crate::project_workspace(&dir, &options).expect("projects");
        let _ = std::fs::remove_dir_all(&dir);
        // A crate none of whose functions lifts has no behaviour module.
        let text = |path: &str| {
            String::from_utf8(out.get(path).cloned().unwrap_or_default()).expect("utf-8")
        };
        (text("frontier.toml"), text("src/Behavior.lex.tex"))
    }

    #[test]
    fn qualified_fields_keep_their_scope_ue_31() {
        let (frontier, behavior) = projected(
            "pathed",
            &[(
                "src/lib.rs",
                r#"//! Scoped type paths.
/// A.
pub mod a {
    /// Numeric declaration.
    pub struct Declared { pub left: u8 }
}
/// B.
pub mod b {
    /// Boolean declaration.
    pub struct Declared { pub right: bool }
}
/// Holder scope.
pub mod inner {
    use crate::b::Declared;
    /// Both written bindings.
    pub struct Holder {
        pub number: crate::a::Declared,
        pub flag: Declared,
    }
    /// Numeric field.
    pub fn number(h: &Holder) -> u8 { h.number.left }
    /// Boolean field.
    pub fn flag(h: &Holder) -> bool { h.flag.right }
}
/// Qualified signature.
pub fn direct(x: &crate::a::Declared) -> u8 { x.left }
/// Nested qualified type.
pub fn nested(xs: Vec<crate::a::Declared>) -> Vec<crate::a::Declared> { xs }
/// An unresolved qualified type must not borrow a bare name.
pub fn unresolved(x: &crate::missing::Declared) -> u8 { let _ = x; 0 }
"#,
            )],
        );
        for function in [
            "pathed::inner::number",
            "pathed::inner::flag",
            "pathed::direct",
            "pathed::nested",
        ] {
            let row = row(&frontier, function);
            assert!(row.starts_with("[[lifted]]"), "{row}");
        }
        assert!(!row(&frontier, "pathed::unresolved").starts_with("[[lifted]]"));
        assert!(behavior.contains("result: Bool"), "{behavior}");
        assert!(behavior.contains("result: Nat"), "{behavior}");
    }

    /// The register row of a function: its kind and its lines.
    fn row(frontier: &str, function: &str) -> String {
        let needle = format!("function = \"{function}\"\n");
        let rows: Vec<&str> = frontier
            .split("\n\n")
            .filter(|row| row.contains(&needle))
            .collect();
        assert_eq!(rows.len(), 1, "{function} has one row:\n{frontier}");
        rows[0].to_string()
    }

    /// One definition of the behaviour module, flattened to a line.
    fn definition(behavior: &str, name: &str) -> String {
        let found = behavior
            .split("definition{")
            .find(|d| d.trim_start().starts_with(&format!("name: {name},")))
            .unwrap_or_else(|| panic!("{name} is defined:\n{behavior}"));
        found.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn a_trait_implementations_method_lifts_as_a_function_of_its_type_ue_15() {
        let (frontier, behavior) = projected(
            "shapes",
            &[(
                "src/lib.rs",
                "//! Shapes.\n\
                 /// Something with an area.\n\
                 pub trait Shape {\n    /// The area.\n    fn area(&self) -> u32;\n}\n\
                 /// Something with a label.\n\
                 pub trait Label {\n    /// An area.\n    fn area(&self) -> u32;\n    \
                 /// The label.\n    fn label(&self) -> String;\n}\n\
                 /// A square.\npub struct Square {\n    /// Its side.\n    pub side: u32,\n}\n\
                 /// A rectangle.\npub struct Rect {\n    /// Its width.\n    pub w: u32,\n    \
                 /// Its height.\n    pub h: u32,\n}\n\
                 impl Shape for Square {\n    fn area(&self) -> u32 {\n        \
                 self.side.saturating_mul(self.side)\n    }\n}\n\
                 impl Shape for Rect {\n    fn area(&self) -> u32 {\n        \
                 self.w.saturating_mul(self.h)\n    }\n}\n\
                 impl Label for Rect {\n    fn area(&self) -> u32 {\n        0\n    }\n    \
                 fn label(&self) -> String {\n        \"rect\".to_string()\n    }\n}\n\
                 impl Default for Square {\n    fn default() -> Self {\n        \
                 Square { side: 1 }\n    }\n}\n\
                 impl From<u32> for Rect {\n    fn from(side: u32) -> Self {\n        \
                 Rect { w: side, h: side }\n    }\n}\n\
                 impl From<bool> for Rect {\n    fn from(whole: bool) -> Self {\n        \
                 if whole {\n            Rect { w: 1, h: 1 }\n        } else {\n            \
                 Rect { w: 0, h: 0 }\n        }\n    }\n}\n\
                 impl std::fmt::Display for Rect {\n    \
                 fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        \
                 write!(f, \"rect\")\n    }\n}\n\
                 /// The area of a square.\npub fn square_area(s: &Square) -> u32 {\n    \
                 s.area()\n}\n\
                 /// The unit square.\npub fn unit() -> Square {\n    Square::default()\n}\n\
                 /// A rectangle's label.\npub fn label_of(r: &Rect) -> String {\n    \
                 r.label()\n}\n\
                 /// A rectangle of a side.\npub fn of_side(side: u32) -> Rect {\n    \
                 Rect::from(side)\n}\n",
            )],
        );
        // A function of the implementing type, with `self` first; one name
        // for each type of a trait and each trait of a type.
        for (name, needle) in [
            (
                "squareShapeArea",
                "parameters: [selfValue : Square], result: Nat",
            ),
            (
                "rectShapeArea",
                "parameters: [selfValue : Rect], result: Nat",
            ),
            (
                "rectLabelArea",
                "parameters: [selfValue : Rect], result: Nat, body: 0",
            ),
            (
                "rectLabelLabel",
                "parameters: [selfValue : Rect], result: String",
            ),
            ("squareDefaultDefault", "parameters: [], result: Square"),
            ("rectFromU32From", "parameters: [side : Nat], result: Rect"),
            (
                "rectFromBoolFrom",
                "parameters: [whole : Bool], result: Rect",
            ),
            // The text a value displays as keeps the name it had.
            ("rectFmt", "parameters: [selfValue : Rect], result: String"),
            // A call written on the value or the type means the type's one
            // method of the name.
            ("squareArea", "body: (squareShapeArea s)"),
            ("unit", "body: (squareDefaultDefault)"),
            ("labelOf", "body: (rectLabelLabel r)"),
        ] {
            let found = definition(&behavior, name);
            assert!(found.contains(needle), "{name}: {found}");
        }
        // Rust's own name for each: `Type::name` where the type has one
        // method of the name, `<Type as Trait>::name` where it has several.
        for (function, lifted) in [
            ("shapes::Square::area", "Behavior::squareShapeArea"),
            ("shapes::<Rect as Shape>::area", "Behavior::rectShapeArea"),
            ("shapes::<Rect as Label>::area", "Behavior::rectLabelArea"),
            ("shapes::Rect::label", "Behavior::rectLabelLabel"),
            ("shapes::Square::default", "Behavior::squareDefaultDefault"),
            (
                "shapes::<Rect as From<u32>>::from",
                "Behavior::rectFromU32From",
            ),
            (
                "shapes::<Rect as From<bool>>::from",
                "Behavior::rectFromBoolFrom",
            ),
            ("shapes::Rect::fmt", "Behavior::rectFmt"),
        ] {
            let found = row(&frontier, function);
            assert!(
                found.starts_with("[[lifted]]")
                    && found.contains(&format!("definition = \"{lifted}\"")),
                "{found}"
            );
        }
        // A generic trait is implemented once per argument list, and the
        // types at the call pick among them: no call is read as one.
        let found = row(&frontier, "shapes::of_side");
        assert!(found.contains("reason = \"call\""), "{found}");
        // A method the trait declares without a body is no one function.
        let found = row(&frontier, "shapes::Shape::area");
        assert!(
            found.contains(
                "detail = \"signature outside the lift: a method Shape declares without a \
                 body, which each implementing type gives: an entry of Shape's dictionary, with \
                 no definition of its own\""
            ),
            "{found}"
        );
    }

    #[test]
    fn a_call_means_a_traits_method_only_where_the_trait_is_in_scope() {
        let (frontier, behavior) = projected(
            "scopes",
            &[
                (
                    "src/lib.rs",
                    "//! Scopes.\npub mod far;\npub mod near;\npub mod shapes;\n\
                     pub use shapes::Label;\n",
                ),
                (
                    "src/shapes.rs",
                    "//! A square and its traits.\n\
                     /// A square.\n#[derive(Clone)]\npub struct Square {\n    \
                     /// Its side.\n    pub side: u32,\n}\n\
                     /// Something that gives a fresh square.\n\
                     pub trait Fresh {\n    /// A fresh square.\n    \
                     fn clone(&self) -> Square;\n}\n\
                     impl Fresh for Square {\n    fn clone(&self) -> Square {\n        \
                     Square { side: 0 }\n    }\n}\n\
                     /// Something with a label.\n\
                     pub trait Label {\n    /// The label.\n    fn label(&self) -> String;\n}\n\
                     impl Label for Square {\n    fn label(&self) -> String {\n        \
                     \"square\".to_string()\n    }\n}\n\
                     /// The label, where the trait is declared.\n\
                     pub fn here(s: &Square) -> String {\n    s.label()\n}\n",
                ),
                (
                    "src/far.rs",
                    "//! Where `Fresh` is not in scope.\nuse crate::shapes::Square;\n\
                     /// The square again, as `Clone` gives it.\n\
                     pub fn again(s: &Square) -> Square {\n    s.clone()\n}\n",
                ),
                (
                    "src/near.rs",
                    "//! Where the traits are imported.\n\
                     use crate::shapes::{Fresh, Square};\nuse crate::Label as _;\n\
                     /// A fresh square, named by its trait.\n\
                     pub fn fresh(s: &Square) -> Square {\n    Fresh::clone(s)\n}\n\
                     /// The label, through a trait the crate root re-exports.\n\
                     pub fn label_of(s: &Square) -> String {\n    s.label()\n}\n\
                     /// A module with a scope of its own.\n\
                     pub mod inner {\n    use crate::shapes::Square;\n    \
                     /// The square again: this scope does not import `Fresh`.\n    \
                     pub fn inner_again(s: &Square) -> Square {\n        s.clone()\n    }\n}\n",
                ),
            ],
        );
        // The trait's method lifts wherever it is called from.
        let found = row(&frontier, "scopes::shapes::Square::clone");
        assert!(
            found.contains("definition = \"Behavior::squareFreshClone\""),
            "{found}"
        );
        for (name, needle) in [
            // Out of the trait's scope `.clone()` is `Clone`'s: the value.
            ("again", "body: s}"),
            ("innerAgain", "body: s}"),
            // In its scope a call is the trait's method: where the trait is
            // declared, and where a `use` leads to its declaration.
            ("here", "body: (squareLabelLabel s)"),
            ("labelOf", "body: (squareLabelLabel s)"),
        ] {
            let found = definition(&behavior, name);
            assert!(found.contains(needle), "{name}: {found}");
        }
    }

    #[test]
    fn a_traits_method_gives_way_to_the_function_that_holds_its_name() {
        let (frontier, _) = projected(
            "yields",
            &[(
                "src/lib.rs",
                "//! Names.\n\
                 /// Something with an area.\n\
                 pub trait Shape {\n    /// The area.\n    fn area(&self) -> u32;\n}\n\
                 /// A square.\npub struct Square {\n    /// Its side.\n    pub side: u32,\n}\n\
                 /// A rectangle.\npub struct Rect {\n    /// Its width.\n    pub w: u32,\n}\n\
                 /// Spelled as the method of `Rect` below lifts.\n\
                 pub fn rect_shape_area(n: u32) -> u32 {\n    n\n}\n\
                 impl Shape for Rect {\n    fn area(&self) -> u32 {\n        self.w\n    }\n}\n\
                 impl Shape for Square {\n    fn area(&self) -> u32 {\n        self.side\n    }\n}\n\
                 /// Spelled as the method of `Square` above lifts.\n\
                 pub fn square_shape_area(n: u32) -> u32 {\n    n\n}\n",
            )],
        );
        // Before the method or after it, the function keeps the name.
        for (function, method) in [
            ("yields::rect_shape_area", "yields::Rect::area"),
            ("yields::square_shape_area", "yields::Square::area"),
        ] {
            let kept = row(&frontier, function);
            assert!(kept.starts_with("[[lifted]]"), "{kept}");
            let gave = row(&frontier, method);
            assert!(
                gave.starts_with("[[residue]]") && gave.contains("reason = \"collision\""),
                "{gave}"
            );
        }
    }

    #[test]
    fn a_default_method_lifts_where_its_body_is_one_for_every_implementing_type() {
        let (frontier, behavior) = projected(
            "defaults",
            &[(
                "src/lib.rs",
                "//! Defaults.\n\
                 /// Something with an area.\n\
                 pub trait Shape {\n    /// The area.\n    fn area(&self) -> u32;\n    \
                 /// The corners of a shape that does not say.\n    \
                 fn corners(&self) -> u32 {\n        4\n    }\n    \
                 /// The area doubled.\n    \
                 fn doubled(&self) -> u32 {\n        self.area().saturating_mul(2)\n    }\n    \
                 /// The corners doubled.\n    \
                 fn corners_doubled(&self) -> u32 {\n        \
                 self.corners().saturating_mul(2)\n    }\n    \
                 /// The shape itself.\n    fn same(&self) -> &Self {\n        self\n    }\n    \
                 /// A hook that does nothing.\n    fn visit(&mut self, _n: u32) {}\n}\n\
                 /// Spelled like a default method.\n\
                 pub fn corners() -> u32 {\n    3\n}\n\
                 /// A trait whose default method calls a function of that spelling.\n\
                 pub trait Count {\n    /// The count.\n    \
                 fn count(&self) -> u32 {\n        corners()\n    }\n}\n",
            )],
        );
        // One function for every implementing type takes no value of one.
        for (name, needle) in [
            ("shapeCorners", "parameters: [], result: Nat, body: 4}"),
            ("shapeVisit", "parameters: [n : Nat], result: Unit"),
            // A free function of a method's spelling is the free function.
            (
                "countCount",
                "parameters: [], result: Nat, body: (corners)}",
            ),
        ] {
            let found = definition(&behavior, name);
            assert!(found.contains(needle), "{name}: {found}");
        }
        // What it uses of an implementing type keeps it out, by name.
        for (function, detail) in [
            (
                "defaults::Shape::doubled",
                "a default method of Shape that calls Shape::area, which the trait declares \
                 without a body: it means each implementing type's",
            ),
            (
                "defaults::Shape::corners_doubled",
                "a default method of Shape that calls Shape::corners, whose body in the trait \
                 an implementing type may replace: it means each implementing type's",
            ),
            (
                "defaults::Shape::same",
                "a default method of Shape that names Self, each implementing type",
            ),
        ] {
            let found = row(&frontier, function);
            assert!(
                found.contains("reason = \"signature\"")
                    && found.contains(&format!(
                        "detail = \"signature outside the lift: {detail}\""
                    )),
                "{found}"
            );
        }
    }

    #[test]
    fn a_method_outside_the_lift_names_its_type_and_why() {
        let (frontier, _) = projected(
            "reasons",
            &[(
                "src/lib.rs",
                "//! Reasons.\n\
                 /// A circle.\npub struct Circle(pub u32);\n\
                 /// A blob.\npub struct Blob {\n    /// Its weight.\n    pub weight: UnsafeCell<u8>,\n}\n\
                 /// A bag.\npub struct Bag {\n    /// Its blob.\n    pub blob: Blob,\n}\n\
                 /// A square.\npub struct Square {\n    /// Its side.\n    pub side: u32,\n}\n\
                 impl Circle {\n    /// The radius.\n    \
                 pub fn radius(&self) -> u32 {\n        self.0\n    }\n}\n\
                 impl Bag {\n    /// How many blobs.\n    \
                 pub fn count(&self) -> u32 {\n        1\n    }\n}\n\
                 impl Square {\n    /// Grow.\n    pub fn grow(&mut self, by: u32) {\n        \
                 self.side = self.side.saturating_add(by);\n    }\n}\n\
                 /// Something that scales by a factor of some type.\n\
                 pub trait Scale<T> {\n    /// The side scaled.\n    \
                 fn scale(&self, by: T) -> u32;\n}\n\
                 impl<T: Into<u32>> Scale<T> for Square {\n    \
                 fn scale(&self, by: T) -> u32 {\n        \
                 self.side.saturating_mul(by.into())\n    }\n}\n",
            )],
        );
        // A tuple struct's fields are read by position, so its method
        // lifts; a method that changes its receiver gives back its new
        // value.
        let grow = row(&frontier, "reasons::Square::grow");
        assert!(grow.starts_with("[[lifted]]"), "{grow}");
        let radius = row(&frontier, "reasons::Circle::radius");
        assert!(
            radius.starts_with("[[lifted]]")
                && radius.contains("definition = \"Behavior::circleRadius\""),
            "{radius}"
        );
        for (function, reason, detail) in [
            (
                "reasons::Bag::count",
                "type",
                "type outside the lift: Bag.blob: Blob.weight: UnsafeCell<u8>",
            ),
            (
                "reasons::Square::scale",
                "signature",
                "signature outside the lift: a method of an impl generic in T: one function \
                 for each type or constant that takes its place",
            ),
        ] {
            let found = row(&frontier, function);
            assert!(
                found.starts_with("[[residue]]")
                    && found.contains(&format!("reason = \"{reason}\""))
                    && found.contains(&format!("detail = \"{detail}\"")),
                "{found}"
            );
        }
    }

    #[test]
    fn a_module_declared_out_of_line_under_a_test_cfg_is_no_product_code() {
        let (frontier, _) = projected(
            "marks",
            &[
                (
                    "src/lib.rs",
                    "//! Marks.\n#[cfg(test)]\nmod tests;\n\
                     #[cfg(any(test, feature = \"x\"))]\nmod shared;\n\
                     /// Twice.\npub fn twice(n: u32) -> u32 {\n    n.saturating_mul(2)\n}\n",
                ),
                (
                    "src/tests.rs",
                    "mod helpers;\n\nfn four() -> u32 {\n    helpers::helper(super::twice(2))\n}\n",
                ),
                (
                    "src/tests/helpers.rs",
                    "pub fn helper(n: u32) -> u32 {\n    n\n}\n",
                ),
                (
                    "src/shared.rs",
                    "/// In a build with the feature too.\n\
                     pub fn shared_fn(n: u32) -> u32 {\n    n\n}\n",
                ),
            ],
        );
        // The crate's own function, and the module a build without tests
        // can hold, have their rows; the test tree has none.
        row(&frontier, "marks::twice");
        row(&frontier, "marks::shared::shared_fn");
        for helper in ["four", "helper"] {
            assert!(
                !frontier.contains(&format!("::{helper}\"")),
                "{helper} is test code:\n{frontier}"
            );
        }
    }

    #[test]
    fn two_traits_methods_of_one_name_keep_a_row_each() {
        let (frontier, _) = projected(
            "texts",
            &[(
                "src/lib.rs",
                "//! Texts.\n\
                 /// A point.\npub struct Point {\n    /// Its place.\n    pub x: u32,\n}\n\
                 impl std::fmt::Display for Point {\n    \
                 fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        \
                 write!(f, \"point\")\n    }\n}\n\
                 impl std::fmt::Debug for Point {\n    \
                 fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        \
                 write!(f, \"Point\")\n    }\n}\n",
            )],
        );
        let shown = row(&frontier, "texts::<Point as Display>::fmt");
        assert!(
            shown.contains("definition = \"Behavior::pointDisplayFmt\""),
            "{shown}"
        );
        let debugged = row(&frontier, "texts::<Point as Debug>::fmt");
        assert!(debugged.starts_with("[[residue]]"), "{debugged}");
        // One row per function and per κ: the frontier is a register.
        let read =
            crate::register::Register::parse(std::path::Path::new("frontier.toml"), &frontier);
        assert!(read.is_ok(), "{:?}", read.err().map(|e| e.to_string()));
    }

    // The register as the work queue: the frontier's ranked refusals and
    // lift queue, the lift as Quality measures, and the size of every
    // module against the limit the projection states.

    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use super::{
        frontier, function_rows, refusal_queue, within_limit, Notes, Refusal, Register, Residue,
        Span, UorustError, FRONTIER, LEXLEAN, MAX_FILE_BYTES,
    };

    fn at(line: usize) -> Span {
        Span {
            file: "src/lib.rs".into(),
            line,
        }
    }

    /// A one-package workspace with no `rust-version`, written fresh: `over`
    /// lifts, and `next` and `after` stop at the same arithmetic.
    fn workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uorust-queue-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("a scratch workspace");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"queue\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .expect("a manifest");
        std::fs::write(
            dir.join("src/lib.rs"),
            "//! Q.\n\n/// Over one.\npub fn over(x: u8) -> bool {\n    x > 1\n}\n\n\
             /// One more.\npub fn next(x: u8) -> u8 {\n    x + 1\n}\n\n\
             /// One more again.\npub fn after(x: u8) -> u8 {\n    x + 1\n}\n",
        )
        .expect("a library");
        dir
    }

    fn projected_files(name: &str) -> BTreeMap<String, Vec<u8>> {
        let dir = workspace(name);
        let options = super::Options {
            name: "queue".into(),
            last_modified: "2026-11-11T00:00:00Z".into(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let files = crate::project_workspace(&dir, &options).expect("projects");
        let _ = std::fs::remove_dir_all(dir);
        files
    }

    fn toml_of(files: &BTreeMap<String, Vec<u8>>, path: &str) -> toml::Value {
        String::from_utf8_lossy(&files[path])
            .parse()
            .expect("the file is TOML")
    }

    fn count(value: &toml::Value) -> usize {
        usize::try_from(value.as_integer().expect("a count")).expect("a count")
    }

    /// The `(requirementIndex, threshold)` of the Quality measure `name`.
    fn measure(quality: &str, name: &str) -> (usize, usize) {
        let from = quality
            .find(&format!("name: measure{name},"))
            .unwrap_or_else(|| panic!("no measure{name} in:\n{quality}"));
        let record = &quality[from..from + quality[from..].find("\n}").expect("a record")];
        let field = |key: &str| {
            let rest = &record[record.find(&format!("{key} := ")).expect("a field") + key.len()..];
            rest.trim_start_matches(" := ")
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<usize>()
                .expect("a number")
        };
        (field("requirementIndex"), field("threshold"))
    }

    /// The `index` of the Quality requirement `name`.
    fn requirement(quality: &str, name: &str) -> usize {
        let from = quality
            .find(&format!("name: requirement{name},"))
            .unwrap_or_else(|| panic!("no requirement{name} in:\n{quality}"));
        let rest = &quality[from + quality[from..].find("{index := ").expect("an index")..];
        rest.trim_start_matches("{index := ")
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("a number")
    }

    #[test]
    fn the_frontier_ranks_its_refusals_by_reason() {
        let collision = |line: usize| Refusal::Collision {
            span: at(line),
            name: format!("n{line}"),
        };
        let unaccepted = |line: usize| Refusal::Unaccepted {
            span: at(line),
            feature: format!("f{line}"),
        };
        let refusals: BTreeSet<Refusal> = [
            Refusal::AdvisoriesUnconsulted,
            Refusal::NoMsrv {
                package: "p".into(),
            },
            unaccepted(1),
            unaccepted(2),
            collision(3),
            collision(4),
            collision(5),
        ]
        .into_iter()
        .collect();
        let doc: toml::Value = refusal_queue(&refusals).parse().expect("TOML");
        assert_eq!(count(&doc["admission"]["refusals"]), refusals.len());
        let rows: Vec<(&str, &str, usize)> = doc["reason"]
            .as_array()
            .expect("the queue")
            .iter()
            .map(|r| {
                (
                    r["reason"].as_str().expect("a reason"),
                    r["route"].as_str().expect("a route"),
                    count(&r["refusals"]),
                )
            })
            .collect();
        // The most refusals first; as many, by reason.
        assert_eq!(
            rows,
            [
                ("name-collision", "UR-R13", 3),
                ("feature-without-acceptance", "UR-R10", 2),
                ("advisories-unconsulted", "UR-R21", 1),
                ("no-msrv", "UR-R24", 1),
            ]
        );
        assert_eq!(rows.iter().map(|r| r.2).sum::<usize>(), refusals.len());
        // No refusal: the count is stated, and the queue is empty.
        let none: toml::Value = refusal_queue(&BTreeSet::new()).parse().expect("TOML");
        assert_eq!(count(&none["admission"]["refusals"]), 0);
        assert!(none.get("reason").is_none());
    }

    #[test]
    fn the_frontier_carries_the_queue_of_its_own_rows() {
        let mut notes = Notes::default();
        notes.refusals.insert(Refusal::AdvisoriesUnconsulted);
        for (i, name) in ["c::t1", "c::t2"].into_iter().enumerate() {
            let reason = Residue::Type("Wide".into());
            notes.residue.insert(name.into(), (at(i + 1), reason));
        }
        let form = Residue::Form("a\tloop".into());
        notes.residue.insert("c::g".into(), (at(3), form));
        notes
            .effects
            .insert("c::e".into(), (at(4), "Fs effect: std::fs::read".into()));
        notes.lifted.insert(
            "c::f".into(),
            (at(5), "Behavior::f".into(), vec!["ASCII only".into()]),
        );
        let kappas: BTreeMap<String, String> = ["c::t1", "c::t2", "c::g", "c::e", "c::f"]
            .into_iter()
            .map(|path| (path.to_string(), crate::register::kappa(path, "fn f () {}")))
            .collect();
        let rows = function_rows(&notes, &kappas, &BTreeMap::new());
        let lift = Register::parse(Path::new(FRONTIER), &rows).expect("the rows are a register");
        let text = frontier(&notes, &lift, &rows, &crate::limits::Limits::sdk());
        let doc: toml::Value = text.parse().expect("the frontier is TOML");
        let totals: Vec<usize> = ["functions", "lifted", "residue", "foreign", "effect"]
            .iter()
            .map(|key| count(&doc["lift"][key]))
            .collect();
        assert_eq!(totals, [5, 1, 3, 0, 1]);
        let queue: Vec<(&str, &str, usize)> = doc["construct"]
            .as_array()
            .expect("the queue")
            .iter()
            .map(|r| {
                (
                    r["cause"].as_str().expect("a cause"),
                    r["construct"].as_str().expect("a construct"),
                    count(&r["functions"]),
                )
            })
            .collect();
        // A construct is the detail as a reader of the rows finds it: the
        // tab the frontier writes as a space is a space in the queue too.
        assert_eq!(
            queue,
            [
                ("type", "type outside the lift: Wide", 2),
                ("effect", "Fs effect: std::fs::read", 1),
                ("form", "expression form outside the lift: a loop", 1),
            ]
        );
        assert_eq!(count(&doc["admission"]["refusals"]), 1);
        assert_eq!(doc["refusal"].as_array().map(Vec::len), Some(1));
        // The frontier is the register it was written from, queue and all.
        assert_eq!(Register::parse(Path::new(FRONTIER), &text), Ok(lift));
        // Rows that are not a register give no projection: a function with
        // no κ has no row to key.
        assert!(matches!(
            Register::parse(
                Path::new(FRONTIER),
                &function_rows(&notes, &BTreeMap::new(), &BTreeMap::new())
            ),
            Err(UorustError::Malformed { .. })
        ));
    }

    /// The name given with `--name` is the only name the projection writes:
    /// one tree checked out under two directory names projects to the same
    /// bytes, and the directory's name is in no file.
    #[test]
    fn projection_name_one_tree_under_two_directory_names_projects_alike() {
        let alpha = projected_files("alpha");
        let beta = projected_files("beta");
        let differ: Vec<&String> = alpha
            .keys()
            .chain(beta.keys())
            .filter(|path| alpha.get(*path) != beta.get(*path))
            .collect();
        assert!(differ.is_empty(), "these files differ: {differ:?}");
        for (path, bytes) in &alpha {
            let text = String::from_utf8_lossy(bytes);
            assert!(
                !text.contains("uorust-queue-alpha"),
                "{path} names the workspace's directory"
            );
        }
        let arch = String::from_utf8_lossy(&alpha["src/Architecture.lex.tex"]);
        assert!(arch.contains("Maintain the queue workspace"), "{arch}");
    }

    /// The queue workspace projected with the limits file `limits` states
    /// (none: the SDK scaffold's).
    fn limits_projection(
        tag: &str,
        limits: Option<&str>,
    ) -> Result<BTreeMap<String, Vec<u8>>, UorustError> {
        let dir = workspace(&format!("limits-{tag}"));
        let file = limits.map(|text| {
            let path = dir.with_extension("limits.toml");
            std::fs::write(&path, text).expect("a limits file");
            path
        });
        let options = super::Options {
            name: "queue".into(),
            last_modified: "2026-11-11T00:00:00Z".into(),
            advisories: None,
            mir: None,
            limits: file.clone(),
            embedding: None,
        };
        let files = crate::project_workspace(&dir, &options);
        let _ = std::fs::remove_dir_all(dir);
        if let Some(path) = file {
            let _ = std::fs::remove_file(path);
        }
        files
    }

    /// The scaffold's limits as a limits file, each value through `value`.
    fn limits_declared(value: impl Fn(&str, u64) -> u64) -> String {
        let sdk = crate::limits::Limits::sdk();
        let table = |name: &str, rows: &[(String, u64)]| {
            let mut out = format!("[{name}]\n");
            for (key, v) in rows {
                out.push_str(&format!("{key} = {}\n", value(key, *v)));
            }
            out
        };
        format!(
            "{}\n{}",
            table("lexlean", &sdk.lexlean),
            table("prismpm", &sdk.prismpm)
        )
    }

    #[test]
    fn limits_without_a_file_the_projection_states_the_scaffold_and_says_so() {
        let files = limits_projection("sdk", None).expect("projects");
        let sdk = crate::limits::Limits::sdk();
        let lexlean = String::from_utf8_lossy(&files[LEXLEAN]).to_string();
        let prismpm = String::from_utf8_lossy(&files["prismpm.toml"]).to_string();
        assert!(
            lexlean.ends_with(&crate::limits::Limits::table_text(&sdk.lexlean)),
            "{lexlean}"
        );
        assert!(
            prismpm.ends_with(&crate::limits::Limits::table_text(&sdk.prismpm)),
            "{prismpm}"
        );
        let limits = toml_of(&files, FRONTIER)["limits"].clone();
        assert_eq!(limits["source"].as_str(), Some("sdk-scaffold"), "{limits}");
        assert!(limits.get("lexlean_sha256").is_some(), "{limits}");
    }

    #[test]
    fn limits_a_declared_file_is_what_the_projection_states() {
        let declared = limits_declared(|key, v| match key {
            "max_ir_nodes" | "max_entities" => v * 2,
            _ => v,
        });
        let files = limits_projection("declared", Some(&declared)).expect("projects");
        let sdk = crate::limits::Limits::sdk();
        let doubled = |rows: &[(String, u64)], key: &str| {
            let v = rows.iter().find(|(k, _)| k == key).expect("a key").1;
            format!("\n{key} = {}\n", v * 2)
        };
        let lexlean = String::from_utf8_lossy(&files[LEXLEAN]).to_string();
        let prismpm = String::from_utf8_lossy(&files["prismpm.toml"]).to_string();
        assert!(
            lexlean.contains(&doubled(&sdk.lexlean, "max_ir_nodes")),
            "{lexlean}"
        );
        assert!(
            prismpm.contains(&doubled(&sdk.prismpm, "max_entities")),
            "{prismpm}"
        );
        let limits = toml_of(&files, FRONTIER)["limits"].clone();
        assert_eq!(limits["source"].as_str(), Some("declared"), "{limits}");
        assert_eq!(
            limits["sha256"].as_str(),
            Some(super::sha(declared.as_bytes()).as_str()),
            "{limits}"
        );
        // The lock pins the project file, so it moves with the limits too.
        let default = limits_projection("default", None).expect("projects");
        assert_ne!(files["lexlean.lock"], default["lexlean.lock"]);
    }

    #[test]
    fn limits_a_declared_max_file_bytes_is_the_limit_the_modules_are_held_to() {
        let declared = limits_declared(|key, v| if key == "max_file_bytes" { 1 } else { v });
        match limits_projection("tiny", Some(&declared)) {
            Err(UorustError::ModuleTooLarge { limit, .. }) => assert_eq!(limit, 1),
            other => panic!(
                "not held to the declared limit: {:?}",
                other.map(|f| f.len())
            ),
        }
        let refused = limits_projection("refused", Some("[lexlean]\n"));
        assert!(
            matches!(refused, Err(UorustError::Limits { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn quality_states_the_lift_and_the_open_refusals_as_measures() {
        let files = projected_files("quality");
        let frontier = toml_of(&files, FRONTIER);
        let quality = String::from_utf8_lossy(&files["src/Quality.lex.tex"]).to_string();
        // The register the same run writes.
        let totals = crate::lift_register(&files).expect("a register").totals();
        assert_eq!((totals.functions, totals.lifted, totals.residue), (3, 1, 2));
        let refusals = frontier["refusal"].as_array().expect("refusals").len();
        // Quality's own refusal (no `rust-version`) is one of them.
        assert!(frontier["refusal"]
            .as_array()
            .expect("refusals")
            .iter()
            .any(|r| r["reason"].as_str() == Some("no-msrv")));
        assert_eq!(count(&frontier["admission"]["refusals"]), refusals);
        assert_eq!(count(&frontier["lift"]["functions"]), totals.functions);
        assert_eq!(count(&frontier["lift"]["lifted"]), totals.lifted);
        for (name, measured) in [
            ("ProductFunctions", totals.functions),
            ("LiftedFunctions", totals.lifted),
            ("OpenRefusals", refusals),
        ] {
            let (of, threshold) = measure(&quality, name);
            assert_eq!(threshold, measured, "measure{name}");
            // Built as every other measure is: under its own requirement.
            assert_eq!(of, requirement(&quality, name), "measure{name}");
        }
        // The measure this one is modelled on is still built the same way.
        let (of, _) = measure(&quality, "AdmittedFeatures");
        assert_eq!(of, requirement(&quality, "AdmittedFeatures"));
        // The queue of the two functions one construct stops.
        let first = &frontier["construct"].as_array().expect("the queue")[0];
        assert_eq!(first["cause"].as_str(), Some("arithmetic"));
        assert_eq!(count(&first["functions"]), 2);
    }

    #[test]
    fn a_module_beyond_the_stated_limit_is_a_typed_error() {
        let project = |limit: usize, module: usize| -> BTreeMap<String, Vec<u8>> {
            [
                (
                    LEXLEAN.to_string(),
                    format!("[limits]\nmax_file_bytes = {limit}\n").into_bytes(),
                ),
                ("src/Design.lex.tex".to_string(), vec![b' '; module]),
                // No module: LexLean does not read it as a source file.
                (FRONTIER.to_string(), vec![b'#'; limit + 1]),
            ]
            .into_iter()
            .collect()
        };
        let limit = 64;
        // At the limit a module is admitted; one byte beyond it is refused.
        assert_eq!(within_limit(&project(limit, limit)), Ok(()));
        let refused = within_limit(&project(limit, limit + 1)).expect_err("beyond the limit");
        assert_eq!(
            refused,
            UorustError::ModuleTooLarge {
                module: "src/Design.lex.tex".into(),
                bytes: limit + 1,
                limit,
            }
        );
        let said = refused.to_string();
        assert!(
            said.contains("src/Design.lex.tex") && said.contains("65 bytes") && said.contains("64"),
            "{said}"
        );
        // A project file that states no limit is not one to check against.
        let mut silent = project(limit, limit);
        silent.insert(LEXLEAN.to_string(), b"[limits]\n".to_vec());
        assert!(matches!(
            within_limit(&silent),
            Err(UorustError::Malformed { .. })
        ));
    }

    /// The limit checked is the one the projection's own `lexlean.toml`
    /// states: a projection is within it, and the same files under a lower
    /// stated limit are refused at their largest module.
    #[test]
    fn a_projection_is_checked_against_the_limit_it_states() {
        let mut files = projected_files("limit");
        let stated = format!("max_file_bytes = {MAX_FILE_BYTES}\n");
        let config = String::from_utf8_lossy(&files[LEXLEAN]).to_string();
        assert!(config.contains(&stated), "{config}");
        assert_eq!(within_limit(&files), Ok(()));
        // The first of the largest, in the order the check reads them.
        let (largest, bytes) = files
            .iter()
            .rev()
            .filter(|(path, _)| path.ends_with(".lex.tex"))
            .map(|(path, bytes)| (path.clone(), bytes.len()))
            .max_by_key(|(_, bytes)| *bytes)
            .expect("a module");
        let lower = bytes - 1;
        files.insert(
            LEXLEAN.to_string(),
            config
                .replace(&stated, &format!("max_file_bytes = {lower}\n"))
                .into_bytes(),
        );
        assert_eq!(
            within_limit(&files),
            Err(UorustError::ModuleTooLarge {
                module: largest,
                bytes,
                limit: lower,
            })
        );
    }

    use super::{parts, record, sized, string, Decl, Module};

    /// A module of separate records in the shape of a design facet: a view,
    /// `n` processes, and `n` things that each cite the view and a process.
    fn design_like(n: usize) -> Module {
        let t = "Foundation.Arch::";
        let id = |name: &str| string(&format!("Design::{name}"));
        let mut decls = vec![Decl::value(
            "opmView",
            format!("{t}DesignView"),
            record(&format!("{t}DesignView"), &[("title", string("Processes"))]),
        )];
        for i in 0..n {
            decls.push(Decl::value(
                format!("process{i}"),
                format!("{t}SystemProcess"),
                record(&format!("{t}SystemProcess"), &[("owner", string("c"))]),
            ));
        }
        for i in 0..n {
            decls.push(Decl::value(
                format!("thing{i}"),
                format!("{t}OpmThing"),
                record(
                    &format!("{t}OpmThing"),
                    &[
                        ("view", id("opmView")),
                        ("subject", id(&format!("process{i}"))),
                    ],
                ),
            ));
        }
        Module {
            name: "Design".into(),
            glossaries: vec![super::BOOL],
            imports: vec!["Foundation.Arch".into()],
            title: "design view",
            decls,
        }
    }

    /// The names a module text declares, in order.
    fn declared(text: &str) -> Vec<String> {
        text.split("name: ")
            .skip(1)
            .map(|rest| rest.split(',').next().expect("a name").to_string())
            .collect()
    }

    /// Every ID a module text cites: each string of the form `"Design..::name"`.
    fn cited_ids(text: &str) -> Vec<String> {
        text.split('"')
            .filter(|piece| piece.starts_with("Design") && piece.contains("::"))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn a_module_of_separate_records_beyond_the_limit_is_written_in_parts() {
        let module = design_like(40);
        let whole = module.render();
        let nothing = |_: &str| false;
        // Within the limit it is written whole, byte for byte.
        assert_eq!(
            sized(std::slice::from_ref(&module), whole.len(), &nothing),
            [(module.path(), whole.clone())]
        );
        // A third of its size holds no more than a third of it.
        let limit = whole.len() / 3;
        let written = sized(std::slice::from_ref(&module), limit, &nothing);
        assert!(written.len() > 3, "{} parts", written.len());
        let mut ids = BTreeSet::new();
        let mut order = Vec::new();
        for (k, (path, text)) in written.iter().enumerate() {
            // Each part is a module of its own, within the limit, named in turn.
            let name = match k {
                0 => "Design".to_string(),
                _ => format!("Design{}", k + 1),
            };
            assert_eq!(path, &format!("src/{name}.lex.tex"));
            assert!(text.starts_with(&format!("\\begin{{lexlean}}{{{name}}}\n")));
            assert!(text.len() <= limit, "{path}: {} > {limit}", text.len());
            for declared in declared(text) {
                assert!(
                    ids.insert(format!("{name}::{declared}")),
                    "{declared} twice"
                );
                order.push(declared);
            }
        }
        // Every record is written once, in its order.
        assert_eq!(order, declared(&whole));
        // Every reference names the part its record is in, and some reach
        // back into the first part and some into a later one.
        let cited: Vec<String> = written.iter().flat_map(|(_, t)| cited_ids(t)).collect();
        assert_eq!(cited.len(), cited_ids(&whole).len());
        for id in &cited {
            assert!(ids.contains(id), "{id} names no record");
        }
        let last = &written[written.len() - 1].1;
        assert!(last.contains("\"Design::opmView\""), "{last}");
        assert!(cited.iter().any(|id| !id.starts_with("Design::")));
        // The parts are the same on a second run.
        assert_eq!(
            written,
            sized(std::slice::from_ref(&module), limit, &nothing)
        );
    }

    /// What is not a module of separate records, or is cited from outside
    /// itself, is written whole, and the size check refuses the projection.
    #[test]
    fn a_module_that_cannot_be_parted_is_left_to_the_typed_error() {
        let module = design_like(40);
        let whole = module.render();
        let limit = whole.len() / 3;
        let taken = BTreeSet::new();
        assert!(parts(&module, limit, &taken).is_some());
        let unparted = [(module.path(), whole.clone())];
        // Another file cites one of its records.
        let elsewhere = |id: &str| id == "Design::";
        assert_eq!(
            sized(std::slice::from_ref(&module), limit, &elsewhere),
            unparted
        );
        // Another module does.
        let citing = Module {
            name: "Interface".into(),
            decls: vec![Decl::value("journey", "String", string("Design::opmView"))],
            ..design_like(0)
        };
        let both = [module.clone(), citing];
        assert_eq!(sized(&both, limit, &|_| false)[0], unparted[0]);
        // A record with a canonical index, whose order its ID decides.
        let mut indexed = design_like(40);
        indexed.decls[0] = Decl::value("opmView", "T", record("T", &[("index", "0".into())]));
        assert!(parts(&indexed, limit, &taken).is_none());
        // A declaration that names another outside a string, or takes a
        // parameter, is no separate record.
        let mut naming = design_like(40);
        naming.decls[1] = Decl::value("process0", "T", "opmView".to_string());
        assert!(parts(&naming, limit, &taken).is_none());
        let mut taking = design_like(40);
        if let Decl::Definition { parameters, .. } = &mut taking.decls[1] {
            parameters.push(("x".into(), "Nat".into()));
        }
        assert!(parts(&taking, limit, &taken).is_none());
        // One record alone beyond the limit, and a part's name already a
        // module's.
        let one = crate::lex::render_decl(&module.decls[0]).len();
        assert!(parts(&module, one, &taken).is_none());
        assert!(parts(&module, limit, &BTreeSet::from(["Design2"])).is_none());
        // Left whole, it is the typed error's.
        let files: BTreeMap<String, Vec<u8>> = [
            (
                LEXLEAN.to_string(),
                format!("[limits]\nmax_file_bytes = {limit}\n").into_bytes(),
            ),
            (module.path(), whole.clone().into_bytes()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            within_limit(&files),
            Err(UorustError::ModuleTooLarge {
                module: "src/Design.lex.tex".into(),
                bytes: whole.len(),
                limit,
            })
        );
    }

    // Name resolution: admission resolves names as the language does.

    /// The frontier of a one-library workspace with `source` as its `lib.rs`.
    fn resolved_frontier(tag: &str, manifest_extra: &str, source: &str) -> String {
        let dir = std::env::temp_dir().join(format!("uorust-resolve-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"w\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
                 rust-version = \"1.70\"\n{manifest_extra}"
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), source).unwrap();
        let options = super::Options {
            name: "w".to_string(),
            last_modified: "2026-11-15T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let files = crate::project_workspace(&dir, &options).expect("a projection");
        let _ = std::fs::remove_dir_all(&dir);
        String::from_utf8(files["frontier.toml"].clone()).unwrap()
    }

    #[test]
    fn a_call_resolves_in_its_nearest_module_before_it_is_ambiguous() {
        let frontier = resolved_frontier(
            "near",
            "",
            "pub mod left {\n pub fn near() -> u8 { 3 }\n pub fn here() -> u8 { near() }\n}\n\
             pub mod right {\n pub fn near() -> u8 { 4 }\n}\n",
        );
        assert!(!frontier.contains("ambiguous-call"), "{frontier}");
    }

    #[test]
    fn a_registry_type_a_use_binds_is_a_foreign_row() {
        let frontier = resolved_frontier(
            "foreign",
            "\n[dependencies]\nserde_json = \"1\"\n",
            "use serde_json::Value;\npub fn null(v: Value) -> bool { v.is_null() }\n",
        );
        assert!(
            frontier.contains("takes or returns serde_json::Value, a type of the serde_json crate"),
            "{frontier}"
        );
    }

    #[test]
    fn complementary_cfg_variants_are_one_fact_not_a_collision() {
        let frontier = resolved_frontier(
            "variant",
            "\n[features]\nwide = []\n",
            "#[cfg(feature = \"wide\")]\npub fn variant() -> u8 { 1 }\n\
             #[cfg(not(feature = \"wide\"))]\npub fn variant() -> u8 { 2 }\n",
        );
        assert!(!frontier.contains("name-collision"), "{frontier}");
        assert!(frontier.contains("reason = \"cfg-variant\""), "{frontier}");
        assert!(
            frontier.contains("w::variant has a variant under cfg(feature = \\\"wide\\\")"),
            "{frontier}"
        );
    }

    // One name per function, whichever lifts and whichever is reached first.

    #[test]
    fn a_name_a_module_gave_is_not_taken_by_a_function_alone_of_its_name() {
        // `axioms` of two modules are named by their modules; `mir_memory_axioms`,
        // alone of its name and in a file read before `mir/memory.rs`, would
        // read as the second of them.
        let (frontier, _) = projected(
            "given",
            &[
                (
                    "src/lib.rs",
                    "//! Given names.\npub mod adapters;\npub mod mir;\npub mod order;\n",
                ),
                (
                    "src/adapters.rs",
                    "//! Adapters.\n/// Outside the lift by its body.\n\
                     pub fn mir_memory_axioms(n: u32) -> u32 {\n    let mut m = n;\n    \
                     while m > 9 {\n        m /= 2;\n    }\n    m\n}\n",
                ),
                ("src/mir/mod.rs", "//! Mir.\npub mod memory;\n"),
                (
                    "src/mir/memory.rs",
                    "//! Memory.\n/// Two.\npub fn axioms() -> u32 {\n    2\n}\n",
                ),
                (
                    "src/order.rs",
                    "//! Order.\n/// One.\npub fn axioms() -> u32 {\n    1\n}\n",
                ),
            ],
        );
        let kept = row(&frontier, "given::mir::memory::axioms");
        assert!(
            kept.starts_with("[[lifted]]")
                && kept.contains("definition = \"Behavior::mirMemoryAxioms\""),
            "{kept}"
        );
        let other = row(&frontier, "given::adapters::mir_memory_axioms");
        assert!(
            !other.contains("reason = \"collision\"") && !other.contains("mirMemoryAxioms\""),
            "{other}"
        );
        assert!(!frontier.contains("more than one fact"), "{frontier}");
    }

    #[test]
    fn a_free_function_keeps_the_name_a_method_reached_first_also_lifts_to() {
        let (frontier, _) = projected(
            "kept",
            &[
                (
                    "src/lib.rs",
                    "//! Kept names.\npub mod embed;\npub mod registry;\n",
                ),
                (
                    "src/embed.rs",
                    "//! Embed.\n/// A sample.\npub struct Sample {\n    /// Its size.\n    \
                     pub n: u32,\n}\nimpl Sample {\n    /// Its cases.\n    \
                     pub fn cases(&self) -> u32 {\n        self.n\n    }\n}\n",
                ),
                (
                    "src/registry.rs",
                    "//! Registry.\n/// The cases of a sample.\n\
                     pub fn sample_cases(n: u32) -> u32 {\n    n\n}\n",
                ),
            ],
        );
        let free = row(&frontier, "kept::registry::sample_cases");
        assert!(
            free.starts_with("[[lifted]]")
                && free.contains("definition = \"Behavior::sampleCases\""),
            "{free}"
        );
        let method = row(&frontier, "kept::embed::Sample::cases");
        assert!(
            method.starts_with("[[residue]]") && method.contains("reason = \"collision\""),
            "{method}"
        );
    }

    // An item of an inline module stands in that module: Rust names it
    // through the inline module's path, and two items of one name are one
    // item only where that path is the same (lane amb2).

    #[test]
    fn inline_a_call_reads_the_function_its_own_inline_module_declares() {
        // `inner::go` shadows the glob's `go`; the crate root has a `go` of
        // its own. Rust: `inner::run()` is 4.
        let (frontier, behavior) = projected(
            "amb",
            &[
                (
                    "src/lib.rs",
                    "//! Shadowed.\npub mod a;\npub mod b;\n/// Three.\n\
                     pub fn go() -> u8 {\n    3\n}\n/// Inner.\npub mod inner {\n    \
                     #[allow(unused_imports)]\n    use crate::a::*;\n    /// Four.\n    \
                     pub fn go() -> u8 {\n        4\n    }\n    /// Calls the inner one.\n    \
                     pub fn run() -> u8 {\n        go()\n    }\n}\n",
                ),
                (
                    "src/a.rs",
                    "//! A.\n/// One.\npub fn go() -> u8 {\n    1\n}\n",
                ),
                (
                    "src/b.rs",
                    "//! B.\n/// Two.\npub fn go() -> u8 {\n    2\n}\n",
                ),
            ],
        );
        let inner = row(&frontier, "amb::inner::go");
        assert!(
            inner.starts_with("[[lifted]]") && inner.contains("definition = \"Behavior::innerGo\""),
            "{inner}"
        );
        let root = row(&frontier, "amb::go");
        assert!(
            root.starts_with("[[lifted]]") && root.contains("definition = \"Behavior::ambGo\""),
            "{root}"
        );
        let run = definition(&behavior, "run");
        assert!(run.contains("body: (innerGo)"), "{run}");
        assert!(
            definition(&behavior, "innerGo").contains("body: 4"),
            "{behavior}"
        );
    }

    #[test]
    fn inline_functions_of_one_name_in_nested_modules_are_three_items() {
        let (frontier, behavior) = projected(
            "nest",
            &[(
                "src/lib.rs",
                "//! Nested.\n/// Three.\npub fn go() -> u8 {\n    3\n}\n/// Root's.\n\
                 pub fn top() -> u8 {\n    go()\n}\n/// Outer.\npub mod outer {\n    \
                 /// Five.\n    pub fn go() -> u8 {\n        5\n    }\n    /// Outer's.\n    \
                 pub fn mid() -> u8 {\n        go()\n    }\n    /// Deep.\n    \
                 pub mod deep {\n        /// Six.\n        pub fn go() -> u8 {\n            \
                 6\n        }\n        /// Deep's.\n        pub fn low() -> u8 {\n            \
                 go()\n        }\n    }\n}\n",
            )],
        );
        for (function, lex) in [
            ("nest::go", "nestGo"),
            ("nest::outer::go", "outerGo"),
            ("nest::outer::deep::go", "outerDeepGo"),
        ] {
            let kept = row(&frontier, function);
            assert!(
                kept.starts_with("[[lifted]]")
                    && kept.contains(&format!("definition = \"Behavior::{lex}\"")),
                "{kept}"
            );
        }
        for (caller, callee) in [
            ("top", "nestGo"),
            ("mid", "outerGo"),
            ("low", "outerDeepGo"),
        ] {
            let read = definition(&behavior, caller);
            assert!(read.contains(&format!("body: ({callee})")), "{read}");
        }
    }

    #[test]
    fn inline_a_function_of_an_inline_module_in_a_file_module_is_its_own() {
        let (frontier, behavior) = projected(
            "filed",
            &[
                ("src/lib.rs", "//! Filed.\npub mod a;\npub mod b;\n"),
                (
                    "src/a.rs",
                    "//! A.\n/// One.\npub fn go() -> u8 {\n    1\n}\n/// A's.\n\
                     pub fn here() -> u8 {\n    go()\n}\n/// Inner.\npub mod inner {\n    \
                     /// Eight.\n    pub fn go() -> u8 {\n        8\n    }\n    \
                     /// Inner's.\n    pub fn run() -> u8 {\n        go()\n    }\n}\n",
                ),
                (
                    "src/b.rs",
                    "//! B.\n/// Two.\npub fn go() -> u8 {\n    2\n}\n/// B's.\n\
                     pub fn there() -> u8 {\n    go()\n}\n",
                ),
            ],
        );
        let inner = row(&frontier, "filed::a::inner::go");
        assert!(
            inner.starts_with("[[lifted]]")
                && inner.contains("definition = \"Behavior::aInnerGo\""),
            "{inner}"
        );
        for (caller, callee) in [("here", "aGo"), ("run", "aInnerGo"), ("there", "bGo")] {
            let read = definition(&behavior, caller);
            assert!(read.contains(&format!("body: ({callee})")), "{read}");
        }
    }

    #[test]
    fn inline_a_type_an_inline_module_declares_is_not_its_files_module_type() {
        // The crate root imports `a::E`; `inner` declares an `E` of its own.
        // Rust: the root's `pick` is `a::E::B`, `inner::pick` is
        // `inner::E::B`.
        let (frontier, behavior) = projected(
            "enums",
            &[
                (
                    "src/lib.rs",
                    "//! Enums.\npub mod a;\nuse crate::a::E;\n/// Root's.\n\
                     pub fn pick() -> E {\n    E::B\n}\n/// Inner.\npub mod inner {\n    \
                     /// Reversed.\n    pub enum E {\n        /// First.\n        B,\n        \
                     /// Second.\n        A,\n        /// Third.\n        C,\n    }\n    \
                     /// Inner's.\n    pub fn pick() -> E {\n        E::B\n    }\n}\n",
                ),
                (
                    "src/a.rs",
                    "//! A.\n/// In order.\npub enum E {\n    /// First.\n    A,\n    \
                     /// Second.\n    B,\n}\n",
                ),
            ],
        );
        for function in ["enums::pick", "enums::inner::pick"] {
            let kept = row(&frontier, function);
            assert!(kept.starts_with("[[lifted]]"), "{kept}");
        }
        let root = definition(&behavior, "enumsPick");
        assert!(root.contains("result: EnumsAE,"), "{root}");
        let inner = definition(&behavior, "innerPick");
        assert!(inner.contains("result: EnumsInnerE,"), "{inner}");
    }

    #[test]
    fn inline_a_type_written_through_a_module_path_is_not_the_readers_own() {
        // Qualified signature types keep the declaration their path names.
        // The constructor in the body still requires its separate reading.
        let (frontier, behavior) = projected(
            "paths",
            &[(
                "src/lib.rs",
                "//! Paths.\n/// Root's.\npub struct P {\n    /// Value.\n    pub v: u8,\n}\n\
                 /// Own.\npub fn mine(p: P) -> u8 {\n    p.v\n}\n\
                 /// Inner's.\npub fn theirs(p: crate::inner::P) -> u8 {\n    p.v\n}\n\
                 /// Inner's, made.\npub fn make() -> crate::inner::P {\n    \
                 crate::inner::P { v: 5 }\n}\n/// Inner.\npub mod inner {\n    \
                 /// Inner's.\n    pub struct P {\n        /// Value.\n        pub v: u8,\n    }\n    \
                 /// Root's.\n    pub fn back(p: super::P) -> u8 {\n        p.v\n    }\n}\n",
            )],
        );
        let kept = row(&frontier, "paths::mine");
        assert!(kept.starts_with("[[lifted]]"), "{kept}");
        for (function, declaration, ty) in [
            ("paths::theirs", "theirs", "PathsInnerP"),
            ("paths::inner::back", "back", "PathsP"),
        ] {
            let kept = row(&frontier, function);
            assert!(kept.starts_with("[[lifted]]"), "{kept}");
            let emitted = definition(&behavior, declaration);
            assert!(
                emitted.contains(&format!("parameters: [p : {ty}]")),
                "{emitted}"
            );
        }
        let made = row(&frontier, "paths::make");
        assert!(!made.starts_with("[[lifted]]"), "{made}");
    }

    #[test]
    fn inline_a_constant_is_the_one_its_own_module_declares() {
        // Rust: `outer()` 3, `inner::run()` 4, `a::fa()` 1, `b::fb()` 2.
        let (frontier, behavior) = projected(
            "konst",
            &[
                (
                    "src/lib.rs",
                    "//! Constants.\npub mod a;\npub mod b;\n/// Three.\npub const N: u8 = 3;\n\
                     /// Root's.\npub fn outer() -> u8 {\n    N\n}\n/// Inner.\npub mod inner {\n    \
                     /// Four.\n    pub const N: u8 = 4;\n    /// Inner's.\n    \
                     pub fn run() -> u8 {\n        N\n    }\n}\n",
                ),
                (
                    "src/a.rs",
                    "//! A.\n/// One.\npub const N: u8 = 1;\n/// A's.\n\
                     pub fn fa() -> u8 {\n    N\n}\n",
                ),
                (
                    "src/b.rs",
                    "//! B.\n/// Two.\npub const N: u8 = 2;\n/// B's.\n\
                     pub fn fb() -> u8 {\n    N\n}\n",
                ),
            ],
        );
        for function in [
            "konst::outer",
            "konst::inner::run",
            "konst::a::fa",
            "konst::b::fb",
        ] {
            let kept = row(&frontier, function);
            assert!(kept.starts_with("[[lifted]]"), "{kept}");
        }
        for (reader, constant, value) in [
            ("outer", "konstN", 3),
            ("run", "innerN", 4),
            ("fa", "aN", 1),
            ("fb", "bN", 2),
        ] {
            let read = definition(&behavior, reader);
            assert!(read.contains(&format!("body: ({constant})")), "{read}");
            let stated = definition(&behavior, constant);
            assert!(stated.contains(&format!("body: {value}")), "{stated}");
        }
    }

    #[test]
    fn inline_a_call_through_a_module_path_reads_the_function_the_path_names() {
        // Rust: `down()` 4, `top()` 1, `inner::up()` 3, `inner::me()` 4;
        // `both()` calls two functions named `go`, which the lift reads by
        // one name, so it is not lifted.
        let (frontier, behavior) = projected(
            "paths",
            &[
                (
                    "src/lib.rs",
                    "//! Paths.\npub mod a;\n/// Three.\npub fn go() -> u8 {\n    3\n}\n\
                     /// Down.\npub fn down() -> u8 {\n    inner::go()\n}\n/// Over.\n\
                     pub fn top() -> u8 {\n    a::go()\n}\n/// Both.\n\
                     pub fn both() -> u8 {\n    go() + inner::go()\n}\n/// Inner.\n\
                     pub mod inner {\n    /// Four.\n    pub fn go() -> u8 {\n        4\n    }\n    \
                     /// Up.\n    pub fn up() -> u8 {\n        super::go()\n    }\n    \
                     /// Me.\n    pub fn me() -> u8 {\n        self::go()\n    }\n}\n",
                ),
                ("src/a.rs", "//! A.\n/// One.\npub fn go() -> u8 {\n    1\n}\n"),
            ],
        );
        for (caller, callee) in [
            ("down", "innerGo"),
            ("top", "aGo"),
            ("up", "pathsGo"),
            ("me", "innerGo"),
        ] {
            let read = definition(&behavior, caller);
            assert!(read.contains(&format!("body: ({callee})")), "{read}");
        }
        let both = row(&frontier, "paths::both");
        assert!(both.starts_with("[[residue]]"), "{both}");
    }

    #[test]
    fn inline_tests_of_one_name_in_two_inline_modules_are_two_acceptance_rows() {
        let (frontier, _) = projected(
            "twice",
            &[(
                "src/lib.rs",
                "//! Twice.\n/// One.\npub fn one() -> u8 {\n    1\n}\n#[cfg(test)]\n\
                 mod first {\n    #[test]\n    fn same() {\n        \
                 assert_eq!(super::one(), 1);\n    }\n}\n#[cfg(test)]\nmod second {\n    \
                 #[test]\n    fn same() {\n        assert_eq!(super::one(), 1);\n    }\n}\n",
            )],
        );
        assert!(!frontier.contains("name-collision"), "{frontier}");
    }

    // A body reads a bare name of its crate's value namespace as Rust's name
    // resolution binds it in the body's own module, whether or not another
    // module declares the name too (lane amb2's repair; the refuter's
    // counterexamples r06, r07, r08, r11, r14, r15, r23).

    #[test]
    fn amb2_a_constant_the_configuration_compiles_out_is_not_the_constant() {
        // Rust: `run3()` is 5; the `K` under `cfg(any())` is in no build.
        let (frontier, behavior) = projected(
            "twin",
            &[(
                "src/lib.rs",
                "//! Twins.\n/// Nine.\n#[cfg(any())]\npub const K: u8 = 9;\n/// Five.\n\
                 #[cfg(all())]\npub const K: u8 = 5;\n/// Reads K.\n\
                 pub fn run3() -> u8 {\n    K\n}\n",
            )],
        );
        assert!(
            frontier.contains("reason = \"cfg-variant\"") && frontier.contains("twin::K"),
            "{frontier}"
        );
        assert!(definition(&behavior, "k").contains("body: 5"), "{behavior}");
        assert!(
            definition(&behavior, "run3").contains("body: (k)"),
            "{behavior}"
        );
    }

    #[test]
    fn amb2_a_function_the_configuration_compiles_out_does_not_shadow_a_glob() {
        // Rust: `inner::run()` is 1, the glob's `a::go`.
        let (_, behavior) = projected(
            "shade",
            &[
                (
                    "src/lib.rs",
                    "//! Shade.\npub mod a;\npub mod b;\n/// Inner.\npub mod inner {\n    \
                     #[allow(unused_imports)]\n    use crate::a::*;\n    /// Nine.\n    \
                     #[cfg(any())]\n    pub fn go() -> u8 {\n        9\n    }\n    \
                     /// Calls the glob's.\n    pub fn run() -> u8 {\n        go()\n    }\n}\n",
                ),
                (
                    "src/a.rs",
                    "//! A.\n/// One.\npub fn go() -> u8 {\n    1\n}\n",
                ),
                (
                    "src/b.rs",
                    "//! B.\n/// Two.\npub fn go() -> u8 {\n    2\n}\n",
                ),
            ],
        );
        let run = definition(&behavior, "run");
        assert!(run.contains("body: (aGo)"), "{run}");
    }

    #[test]
    fn amb2_a_module_that_invokes_a_macro_binds_no_name_of_another_module() {
        // Rust: `inner::run()` is 4 and `other::run()` 6, each the macro's.
        let (frontier, behavior) = projected(
            "made",
            &[(
                "src/lib.rs",
                "//! Made.\nmacro_rules! make {\n    ($n:ident, $v:expr) => {\n        \
                 pub fn $n() -> u8 {\n            $v\n        }\n    };\n}\n/// Three.\n\
                 pub fn go() -> u8 {\n    3\n}\n/// Inner.\npub mod inner {\n    \
                 make!(go, 4);\n    /// Calls the macro's.\n    pub fn run() -> u8 {\n        \
                 go()\n    }\n}\n/// Three.\npub fn solo() -> u8 {\n    3\n}\n/// Other.\n\
                 pub mod other {\n    make!(solo, 6);\n    /// Calls the macro's.\n    \
                 pub fn run() -> u8 {\n        solo()\n    }\n}\n",
            )],
        );
        for function in ["made::inner::run", "made::other::run"] {
            let read = row(&frontier, function);
            assert!(!read.starts_with("[[lifted]]"), "{read}");
        }
        assert!(!behavior.contains("body: (go)"), "{behavior}");
        assert!(!behavior.contains("body: (solo)"), "{behavior}");
    }

    #[test]
    fn amb2_a_variant_a_glob_imports_is_not_a_function_of_its_name() {
        // Rust: `inner::halt()` is `E::Stop`, not the crate's `fn Stop`.
        let (_, behavior) = projected(
            "vary",
            &[(
                "src/lib.rs",
                "//! Variants.\n/// E.\n#[derive(Debug, PartialEq, Clone, Copy)]\n\
                 pub enum E {\n    /// Go.\n    Go(u8),\n    /// Stop.\n    Stop,\n}\n\
                 /// A function named like a variant.\n#[allow(non_snake_case)]\n\
                 pub fn Stop() -> E {\n    E::Go(0)\n}\n/// Inner.\npub mod inner {\n    \
                 use super::E;\n    use super::E::*;\n    /// The variant.\n    \
                 pub fn halt() -> E {\n        Stop\n    }\n}\n",
            )],
        );
        assert!(!behavior.contains("body: (stop)"), "{behavior}");
    }

    #[test]
    fn amb2_a_renaming_use_binds_the_item_its_path_names() {
        // Rust: `inner::run()` is 1 (`a::go`), `helper()` 3.
        let (_, behavior) = projected(
            "named",
            &[
                (
                    "src/lib.rs",
                    "//! Renamed.\npub mod a;\n/// Three.\npub fn helper() -> u8 {\n    3\n}\n\
                     /// Inner.\npub mod inner {\n    use crate::a::go as helper;\n    \
                     /// Calls a::go.\n    pub fn run() -> u8 {\n        helper()\n    }\n}\n",
                ),
                (
                    "src/a.rs",
                    "//! A.\n/// One.\npub fn go() -> u8 {\n    1\n}\n",
                ),
            ],
        );
        let run = definition(&behavior, "run");
        assert!(run.contains("body: (go)"), "{run}");
        assert!(
            definition(&behavior, "go").contains("body: 1"),
            "{behavior}"
        );
    }

    #[test]
    fn amb2_a_use_of_a_path_outside_the_crate_is_no_function_of_the_crate() {
        // Rust: `inner::run()` and `run2()` are 9 (`std::cmp::max`), `run()`
        // is 1 (the crate's `max`).
        let (frontier, behavior) = projected(
            "most",
            &[(
                "src/lib.rs",
                "//! Outside.\n/// The first.\npub fn max(a: u8, _b: u8) -> u8 {\n    a\n}\n\
                 /// Inner.\npub mod inner {\n    use std::cmp::max;\n    /// The std one.\n    \
                 pub fn run() -> u8 {\n        max(1, 9)\n    }\n}\n/// The crate's.\n\
                 pub fn run() -> u8 {\n    max(1, 9)\n}\n/// The std one, by a block use.\n\
                 pub fn run2() -> u8 {\n    use std::cmp::max;\n    max(1, 9)\n}\n",
            )],
        );
        for function in ["most::inner::run", "most::run2"] {
            let read = row(&frontier, function);
            if read.starts_with("[[lifted]]") {
                let name = read
                    .split("definition = \"Behavior::")
                    .nth(1)
                    .and_then(|d| d.split('"').next())
                    .expect("a definition");
                assert!(
                    !definition(&behavior, name).contains("(max 1 9)"),
                    "{behavior}"
                );
            }
        }
        assert!(
            definition(&behavior, "mostRun").contains("body: (max 1 9)"),
            "{behavior}"
        );
    }

    #[test]
    fn amb2_a_module_the_configuration_compiles_out_reads_its_own_functions() {
        // Under `feature = "x"` the module is built whole: `run()` calls its
        // own `helper`, not anything the outside glob could bring.
        let (_, behavior) = projected(
            "gated",
            &[(
                "src/lib.rs",
                "//! Gated.\n/// Gated.\n#[cfg(feature = \"x\")]\npub mod m {\n    \
                 #[allow(unused_imports)]\n    use std::cmp::*;\n    /// One.\n    \
                 pub fn helper() -> u8 {\n        1\n    }\n    /// Calls helper.\n    \
                 pub fn run() -> u8 {\n        helper()\n    }\n}\n",
            )],
        );
        let run = definition(&behavior, "run");
        assert!(run.contains("body: (helper)"), "{run}");
    }

    // Class: the library route (one library project per library package).

    /// The projection of a workspace of library packages, each `(name, lib
    /// source, test source)`, every package with a description and a
    /// repository so that only roots decide its profile.
    fn library_route_projection(
        tag: &str,
        packages: &[(&str, &str, &str)],
    ) -> BTreeMap<String, Vec<u8>> {
        let dir =
            std::env::temp_dir().join(format!("uorust-libroute-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let members: Vec<String> = packages
            .iter()
            .map(|(n, _, _)| format!("\"{n}\""))
            .collect();
        let mut files = vec![(
            "Cargo.toml".to_string(),
            format!(
                "[workspace]\nmembers = [{}]\nresolver = \"2\"\n",
                members.join(", ")
            ),
        )];
        for (name, lib, test) in packages {
            files.push((
                format!("{name}/Cargo.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
                     rust-version = \"1.70\"\ndescription = \"The {name} part.\"\n\
                     repository = \"https://example.org/{name}\"\n"
                ),
            ));
            files.push((format!("{name}/src/lib.rs"), (*lib).to_string()));
            if !test.is_empty() {
                files.push((format!("{name}/tests/claims.rs"), (*test).to_string()));
            }
        }
        for (path, body) in &files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, body).expect("a file");
        }
        let options = super::Options {
            name: "w".to_string(),
            last_modified: "2026-11-11T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let projected = crate::project_workspace(&dir, &options).expect("a projection");
        let _ = std::fs::remove_dir_all(&dir);
        projected
    }

    fn library_route_text(files: &BTreeMap<String, Vec<u8>>, path: &str) -> String {
        String::from_utf8(files.get(path).cloned().unwrap_or_default()).expect("text")
    }

    #[test]
    fn library_route_each_library_package_is_a_project_of_its_own_closure() {
        let files = library_route_projection(
            "two",
            &[
                (
                    "alpha",
                    "//! Alpha.\n\n/// Big.\npub fn big(n: u8) -> bool {\n    n > 3\n}\n",
                    "#[test]\nfn big_is_big() {\n    assert!(alpha::big(5));\n}\n",
                ),
                (
                    "beta",
                    "//! Beta.\n\n/// Small.\npub fn small(n: u8) -> bool {\n    n < 3\n}\n",
                    "#[test]\nfn small_is_small() {\n    assert!(beta::small(1));\n}\n",
                ),
            ],
        );
        let frontier = library_route_text(&files, "frontier.toml");
        assert!(!frontier.contains("undefined-slot"), "{frontier}");
        // The workspace project states no library; each package's does.
        assert!(!files.contains_key("src/Library.lex.tex"));
        assert!(!files.contains_key("src/Foundation/Library/V1/Model.lex.tex"));
        assert!(!library_route_text(&files, "lexlean.toml").contains("Library"));
        for (name, own, other) in [("alpha", "big", "small"), ("beta", "small", "big")] {
            let dir = format!("library/{name}");
            let value = library_route_text(&files, &format!("{dir}/src/Library.lex.tex"));
            assert!(
                value.contains(&format!("cargoName := \"{name}\"")),
                "{value}"
            );
            assert!(value.contains(&format!("\"W.Behavior.{own}\"")), "{value}");
            assert!(
                !value.contains(&format!("\"W.Behavior.{other}\"")),
                "{value}"
            );
            let behavior = library_route_text(&files, &format!("{dir}/src/Behavior.lex.tex"));
            assert!(behavior.contains(&format!("name: {own},")), "{behavior}");
            assert!(!behavior.contains(&format!("name: {other},")), "{behavior}");
            for file in [
                "prismpm.toml",
                "lexlean.lock",
                "lakefile.toml",
                "lean-toolchain",
            ] {
                assert!(files.contains_key(&format!("{dir}/{file}")), "{dir}/{file}");
            }
            let config = library_route_text(&files, &format!("{dir}/lexlean.toml"));
            assert!(config.contains("src/Library.lex.tex"), "{config}");
            assert!(!config.contains("src/Architecture.lex.tex"), "{config}");
        }
    }

    #[test]
    fn library_route_a_package_with_no_repository_is_refused_naming_the_field() {
        let dir =
            std::env::temp_dir().join(format!("uorust-libroute-norepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("a workspace");
        std::fs::create_dir_all(dir.join("tests")).expect("a workspace");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"eta\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             rust-version = \"1.70\"\ndescription = \"The eta part.\"\n\n[workspace]\n",
        )
        .expect("a manifest");
        std::fs::write(
            dir.join("src/lib.rs"),
            "//! Eta.\n\n/// Big.\npub fn big(n: u8) -> bool {\n    n > 3\n}\n",
        )
        .expect("a library");
        std::fs::write(
            dir.join("tests/claims.rs"),
            "#[test]\nfn big_is_big() {\n    assert!(eta::big(5));\n}\n",
        )
        .expect("a test");
        let options = super::Options {
            name: "w".to_string(),
            last_modified: "2026-11-11T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let files = crate::project_workspace(&dir, &options).expect("a projection");
        let _ = std::fs::remove_dir_all(&dir);
        let frontier = library_route_text(&files, "frontier.toml");
        assert!(
            frontier.contains(
                "its Cargo.toml states no `package.repository`; the SDK's native-library \
                 profile requires cargoRepository"
            ),
            "{frontier}"
        );
        assert!(
            frontier.contains("states no `package.homepage` or `package.repository`"),
            "{frontier}"
        );
        assert!(
            !frontier.contains("requires cargoDescription"),
            "{frontier}"
        );
        assert!(!files.keys().any(|p| p.starts_with("library/")));
    }

    #[test]
    fn library_route_a_package_with_no_claim_says_which_root_list_is_empty() {
        let files = library_route_projection(
            "noclaim",
            &[(
                "gamma",
                "//! Gamma.\n\n/// Big.\npub fn big(n: u8) -> bool {\n    n > 3\n}\n",
                "",
            )],
        );
        let frontier = library_route_text(&files, "frontier.toml");
        assert!(
            frontier.contains(
                "the library profile's acceptanceRoots is undefined: its tests state no \
                 closed claim over its lifted functions"
            ),
            "{frontier}"
        );
        assert!(!files.keys().any(|p| p.starts_with("library/")));
    }

    #[test]
    fn library_route_a_refused_root_is_a_row_and_the_rest_is_kept() {
        let files = library_route_projection(
            "refused",
            &[(
                "delta",
                "//! Delta.\n\n/// Big.\npub fn big(n: u8) -> bool {\n    n > 3\n}\n\n\
                 /// Applied.\npub fn applied(f: &dyn Fn(u8) -> u8, n: u8) -> u8 {\n    f(n)\n}\n",
                "#[test]\nfn big_is_big() {\n    assert!(delta::big(5));\n}\n",
            )],
        );
        let frontier = library_route_text(&files, "frontier.toml");
        let value = library_route_text(&files, "library/delta/src/Library.lex.tex");
        assert!(value.contains("\"W.Behavior.big\""), "{value}\n{frontier}");
        assert!(!value.contains("\"W.Behavior.applied\""), "{value}");
        assert!(frontier.contains("reason = \"library-root\""), "{frontier}");
        assert!(
            frontier.contains("does not take the root applied: it reaches Behavior::applied"),
            "{frontier}"
        );
    }

    /// The projection of a workspace written from `files` (path, text) under
    /// `dir`, named `w`.
    fn library_route_written(dir: &Path, files: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
        let _ = std::fs::remove_dir_all(dir);
        for (path, body) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, body).expect("a file");
        }
        let options = super::Options {
            name: "w".to_string(),
            last_modified: "2026-11-11T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let projected = crate::project_workspace(dir, &options).expect("a projection");
        let _ = std::fs::remove_dir_all(dir);
        projected
    }

    /// A package manifest of the library route's tests, with `extra` lines.
    fn library_route_manifest(name: &str, extra: &str) -> String {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             rust-version = \"1.70\"\ndescription = \"The {name} part.\"\n\
             repository = \"https://example.org/{name}\"\n{extra}"
        )
    }

    /// Two packages whose libraries share one crate name (Cargo builds them,
    /// warning of an output filename collision) are two libraries: each
    /// exports only the functions its own files define, and only the claims
    /// over them (a test of `twin-b` calling `twin::left` does not compile).
    #[test]
    fn library_route_two_packages_of_one_library_name_are_told_apart() {
        let dir =
            std::env::temp_dir().join(format!("uorust-libroute-twins-{}", std::process::id()));
        let a = library_route_manifest("twin-a", "\n[lib]\nname = \"twin\"\n");
        let b = library_route_manifest("twin-b", "\n[lib]\nname = \"twin\"\n");
        let files = library_route_written(
            &dir,
            &[
                (
                    "Cargo.toml",
                    "[workspace]\nmembers = [\"twin-a\", \"twin-b\"]\nresolver = \"2\"\n",
                ),
                ("twin-a/Cargo.toml", &a),
                (
                    "twin-a/src/lib.rs",
                    "//! Twin a.\n\n/// Left.\npub fn left(n: u8) -> bool {\n    n < 10\n}\n",
                ),
                (
                    "twin-a/tests/claims.rs",
                    "#[test]\nfn left_holds() {\n    assert!(twin::left(1));\n}\n",
                ),
                ("twin-b/Cargo.toml", &b),
                (
                    "twin-b/src/lib.rs",
                    "//! Twin b.\n\n/// Right.\npub fn right(n: u8) -> bool {\n    n > 10\n}\n",
                ),
                (
                    "twin-b/tests/claims.rs",
                    "#[test]\nfn right_holds() {\n    assert!(twin::right(20));\n}\n",
                ),
            ],
        );
        for (package, own, other, claim) in [
            ("twin-a", "left", "right", "acceptTwinBRightHolds"),
            ("twin-b", "right", "left", "acceptTwinALeftHolds"),
        ] {
            let dir = format!("library/{package}");
            let value = library_route_text(&files, &format!("{dir}/src/Library.lex.tex"));
            assert!(value.contains(&format!("\"W.Behavior.{own}\"")), "{value}");
            assert!(
                !value.contains(&format!("\"W.Behavior.{other}\"")),
                "{value}"
            );
            assert!(!value.contains(claim), "{value}");
            let behavior = library_route_text(&files, &format!("{dir}/src/Behavior.lex.tex"));
            assert!(!behavior.contains(&format!("name: {other},")), "{behavior}");
        }
    }

    /// A binary's functions are in no library: not those of the package's
    /// own `src/main.rs` (the binary takes the library's crate name), nor
    /// those of another package's `[[bin]]` named like the library.
    #[test]
    fn library_route_a_binary_named_like_a_library_adds_nothing_to_it() {
        let dir = std::env::temp_dir().join(format!("uorust-libroute-bins-{}", std::process::id()));
        let mu = library_route_manifest("mu", "");
        let kappa = library_route_manifest("kappa", "");
        let tool = library_route_manifest(
            "kappa-tool",
            "\n[[bin]]\nname = \"kappa\"\npath = \"src/main.rs\"\n",
        );
        let files = library_route_written(
            &dir,
            &[
                (
                    "Cargo.toml",
                    "[workspace]\nmembers = [\"mu\", \"kappa\", \"kappa-tool\"]\n\
                     resolver = \"2\"\n",
                ),
                ("mu/Cargo.toml", &mu),
                (
                    "mu/src/lib.rs",
                    "//! Mu.\n\n/// Low.\npub fn low(n: u8) -> bool {\n    n < 4\n}\n",
                ),
                (
                    "mu/src/main.rs",
                    "//! Mu tool.\n\n/// Wide.\npub fn wide(n: u8) -> bool {\n    n > 100\n}\n\n\
                     fn main() {\n    println!(\"{}\", wide(200));\n}\n",
                ),
                (
                    "mu/tests/claims.rs",
                    "#[test]\nfn low_holds() {\n    assert!(mu::low(1));\n}\n",
                ),
                ("kappa/Cargo.toml", &kappa),
                (
                    "kappa/src/lib.rs",
                    "//! Kappa.\n\n/// One.\npub fn one(n: u8) -> bool {\n    n == 1\n}\n",
                ),
                (
                    "kappa/tests/claims.rs",
                    "#[test]\nfn one_holds() {\n    assert!(kappa::one(1));\n}\n",
                ),
                ("kappa-tool/Cargo.toml", &tool),
                (
                    "kappa-tool/src/main.rs",
                    "//! Kappa tool.\n\n/// Two.\npub fn two(n: u8) -> bool {\n    n == 2\n}\n\n\
                     fn main() {\n    println!(\"{}\", two(2));\n}\n",
                ),
            ],
        );
        assert!(!files.keys().any(|p| p.starts_with("library/kappa-tool/")));
        for (package, own, other) in [("mu", "low", "wide"), ("kappa", "one", "two")] {
            let dir = format!("library/{package}");
            let value = library_route_text(&files, &format!("{dir}/src/Library.lex.tex"));
            assert!(value.contains(&format!("\"W.Behavior.{own}\"")), "{value}");
            assert!(
                !value.contains(&format!("\"W.Behavior.{other}\"")),
                "{value}"
            );
            let behavior = library_route_text(&files, &format!("{dir}/src/Behavior.lex.tex"));
            assert!(!behavior.contains(&format!("name: {other},")), "{behavior}");
        }
    }

    /// A package whose repository is an `http` URL (Cargo takes it; the
    /// SDK's profile admits only `https`) gets no library project, which
    /// `prismpm check` would refuse, but a row naming the field; so does a
    /// pre-release version.
    #[test]
    fn library_route_a_value_the_profile_does_not_admit_is_refused_naming_the_field() {
        let dir = std::env::temp_dir().join(format!("uorust-libroute-http-{}", std::process::id()));
        let zeta = "[package]\nname = \"zeta\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
                    rust-version = \"1.70\"\ndescription = \"The zeta part.\"\n\
                    repository = \"http://example.org/zeta\"\n";
        let iota = library_route_manifest("iota", "").replace("0.1.0", "0.1.0-alpha.1");
        let files = library_route_written(
            &dir,
            &[
                (
                    "Cargo.toml",
                    "[workspace]\nmembers = [\"zeta\", \"iota\"]\nresolver = \"2\"\n",
                ),
                ("zeta/Cargo.toml", zeta),
                (
                    "zeta/src/lib.rs",
                    "//! Zeta.\n\n/// Zero.\npub fn zero(n: u8) -> bool {\n    n == 0\n}\n",
                ),
                (
                    "zeta/tests/claims.rs",
                    "#[test]\nfn zero_holds() {\n    assert!(zeta::zero(0));\n}\n",
                ),
                ("iota/Cargo.toml", &iota),
                (
                    "iota/src/lib.rs",
                    "//! Iota.\n\n/// One.\npub fn one(n: u8) -> bool {\n    n == 1\n}\n",
                ),
                (
                    "iota/tests/claims.rs",
                    "#[test]\nfn one_holds() {\n    assert!(iota::one(1));\n}\n",
                ),
            ],
        );
        let frontier = library_route_text(&files, "frontier.toml");
        assert!(
            frontier.contains(
                "its Cargo.toml states `package.repository` as a value no library project \
                 can hold as cargoRepository: the SDK admits"
            ),
            "{frontier}"
        );
        assert!(
            frontier.contains(
                "its Cargo.toml states `package.homepage` or `package.repository` as a value"
            ),
            "{frontier}"
        );
        assert!(
            frontier.contains(
                "its Cargo.toml states `package.version` as a value no library project \
                 can hold as cargoVersion: the SDK admits"
            ),
            "{frontier}"
        );
        assert!(
            !files.keys().any(|p| p.starts_with("library/")),
            "{frontier}"
        );
    }

    /// A workspace whose member lies outside its root (`members =
    /// ["../member"]`, which Cargo builds) projects to the same bytes
    /// wherever the two directories lie: a member's paths are written
    /// relative to the root, never absolute.
    #[test]
    fn projection_name_a_member_outside_the_root_projects_alike_anywhere() {
        let base = std::env::temp_dir().join(format!("uorust-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let files = [
            (
                "root/Cargo.toml",
                "[workspace]\nmembers = [\"../member\"]\nresolver = \"2\"\n",
            ),
            (
                "member/Cargo.toml",
                "[package]\nname = \"omega\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
                 rust-version = \"1.70\"\nworkspace = \"../root\"\n",
            ),
            (
                "member/src/lib.rs",
                "//! Omega.\n\n/// Last.\npub fn last(n: u8) -> bool {\n    n == 255\n}\n",
            ),
            (
                "member/tests/claims.rs",
                "#[test]\nfn last_holds() {\n    assert!(omega::last(255));\n}\n",
            ),
        ];
        let options = super::Options {
            name: "w".to_string(),
            last_modified: "2026-11-11T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let mut projections = Vec::new();
        for place in ["here", "elsewhere/other-place-ZZ"] {
            let dir = base.join(place);
            for (path, body) in files {
                let path = dir.join(path);
                std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
                std::fs::write(path, body).expect("a file");
            }
            projections
                .push(crate::project_workspace(&dir.join("root"), &options).expect("a projection"));
        }
        let _ = std::fs::remove_dir_all(&base);
        let (here, there) = (&projections[0], &projections[1]);
        let differ: Vec<&String> = here
            .keys()
            .chain(there.keys())
            .filter(|path| here.get(*path) != there.get(*path))
            .collect();
        assert!(differ.is_empty(), "these files differ: {differ:?}");
        let base_text = base.to_string_lossy().to_string();
        for (path, bytes) in here {
            let text = String::from_utf8_lossy(bytes);
            assert!(
                !text.contains(&base_text),
                "{path} names where the workspace lies"
            );
        }
        let frontier = String::from_utf8_lossy(&here["frontier.toml"]);
        assert!(
            frontier.contains("\"../member/src/lib.rs:4\""),
            "{frontier}"
        );
    }
    // Class: calls the language resolves by type (kai, round 3): a
    // function's inline module is part of its path.

    /// The projection of a one-library workspace with `source` as its
    /// `lib.rs`: its frontier and its interface module.
    fn typed_call_projection(tag: &str, source: &str) -> (String, String) {
        let dir = std::env::temp_dir().join(format!("uorust-typed-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"w\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             rust-version = \"1.70\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), source).unwrap();
        let options = super::Options {
            name: "w".to_string(),
            last_modified: "2026-11-15T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let files = crate::project_workspace(&dir, &options).expect("a projection");
        let _ = std::fs::remove_dir_all(&dir);
        let text = |suffix: &str| {
            files
                .iter()
                .find(|(k, _)| k.ends_with(suffix))
                .map(|(_, v)| String::from_utf8(v.clone()).unwrap())
                .unwrap_or_default()
        };
        (text("frontier.toml"), text("Interface.lex.tex"))
    }

    #[test]
    fn functions_of_one_name_in_two_inline_modules_have_two_paths() {
        let (frontier, interface) = typed_call_projection(
            "inline",
            "pub mod outer {\n pub mod left {\n  pub fn pick() -> u8 { 1 }\n }\n \
             pub mod right {\n  pub fn pick() -> u8 { 2 }\n }\n}\n",
        );
        assert!(!frontier.contains("name-collision"), "{frontier}");
        for want in ["callWOuterLeftPick", "callWOuterRightPick"] {
            assert!(interface.contains(want), "{want} not in {interface}");
        }
    }

    // Class: refuted readings (repair of round 4's confirmed counterexamples).

    /// The frontier and the behavior module of a one-file package.
    fn refuted_projection(tag: &str, source: &str) -> (String, String) {
        let dir = std::env::temp_dir().join(format!("uorust-refuted-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"w\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             rust-version = \"1.70\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), source).unwrap();
        let options = super::Options {
            name: "w".to_string(),
            last_modified: "2026-11-15T00:00:00Z".to_string(),
            advisories: None,
            mir: None,
            limits: None,
            embedding: None,
        };
        let files = crate::project_workspace(&dir, &options).expect("a projection");
        let _ = std::fs::remove_dir_all(&dir);
        let text = |suffix: &str| {
            files
                .iter()
                .find(|(k, _)| k.ends_with(suffix))
                .map(|(_, v)| String::from_utf8(v.clone()).unwrap())
                .unwrap_or_default()
        };
        (text("frontier.toml"), text("Behavior.lex.tex"))
    }

    #[test]
    fn an_inline_module_reads_a_shared_name_by_its_own_use_alone() {
        let (_, behavior) = refuted_projection(
            "homonym-use",
            "mod left {\n pub fn pick() -> u8 { 1 }\n}\n\
             mod right {\n pub fn pick() -> u8 { 2 }\n}\n\
             use right::pick;\n\
             pub fn top() -> u8 { pick() }\n\
             pub mod inner {\n use crate::left::pick;\n pub fn caller() -> u8 { pick() }\n}\n",
        );
        let top = behavior
            .lines()
            .find(|l| l.contains("name: top,"))
            .unwrap_or_default();
        assert!(top.contains("(rightPick)"), "{behavior}");
        let caller = behavior
            .lines()
            .find(|l| l.contains("name: caller,"))
            .unwrap_or_default();
        assert!(caller.contains("(leftPick)"), "{behavior}");
    }

    #[test]
    fn an_alias_that_takes_a_primitive_name_is_read_nowhere() {
        let (frontier, behavior) = refuted_projection(
            "primitive-alias",
            "pub mod narrow {\n #[allow(non_camel_case_types)]\n pub type f64 = f32;\n \
             pub fn small(x: f64) -> bool { x > 0.5 }\n}\n\
             pub fn wide(x: f64) -> bool { x > 0.5 }\n",
        );
        assert!(!behavior.contains("8 23"), "{behavior}");
        assert!(
            frontier.contains("a primitive type's name an alias takes"),
            "{frontier}"
        );
    }

    // Class: a trait is a dictionary (kai, round 4): a function generic
    // over `T: Trait` takes the methods the impl for `T` gives, and each
    // call passes the impl's methods of the type its arguments fix.

    const DICTIONARY_CRATE: &str = "//! Areas through a trait.\n\
        /// Something with an area.\n\
        pub trait Area {\n    /// The area.\n    fn area(&self) -> u32;\n    \
        /// The area scaled.\n    fn scaled(&self, k: u32) -> u32;\n}\n\
        /// Something with a name and an area.\n\
        pub trait Named: Area {\n    /// The name.\n    fn name(&self) -> u32;\n}\n\
        /// Something that counts.\n\
        pub trait Counter {\n    /// One more.\n    fn bump(&mut self) -> u32;\n    \
        /// Into a number.\n    fn into(self) -> u32;\n}\n\
        /// A square.\npub struct Square {\n    /// Its side.\n    pub side: u32,\n}\n\
        /// A rectangle.\npub struct Rect {\n    /// Its width.\n    pub w: u32,\n    \
        /// Its height.\n    pub h: u32,\n}\n\
        impl Area for Square {\n    fn area(&self) -> u32 {\n        \
        self.side.saturating_mul(self.side)\n    }\n    \
        fn scaled(&self, k: u32) -> u32 {\n        k.saturating_mul(self.side)\n    }\n}\n\
        impl Area for Rect {\n    fn area(&self) -> u32 {\n        \
        self.w.saturating_mul(self.h)\n    }\n    \
        fn scaled(&self, k: u32) -> u32 {\n        k.saturating_mul(self.w)\n    }\n}\n\
        /// Twice the area.\npub fn twice<T: Area>(s: &T) -> u32 {\n    \
        s.area().saturating_add(s.area())\n}\n\
        /// One area and another scaled.\n\
        pub fn summed<T>(a: &T, b: &T) -> u32\nwhere\n    T: Area,\n{\n    \
        T::area(a).saturating_add(b.scaled(2))\n}\n\
        /// Twice, passed on.\npub fn passed<U: Area>(u: &U) -> u32 {\n    twice(u)\n}\n\
        /// Twice a square.\npub fn square_twice(s: &Square) -> u32 {\n    twice(s)\n}\n\
        /// Twice a rectangle, passed on.\npub fn rect_passed(r: &Rect) -> u32 {\n    \
        passed(r)\n}\n\
        /// Two rectangles.\npub fn rects_summed(a: &Rect, b: &Rect) -> u32 {\n    \
        summed(a, b)\n}\n\
        /// The value itself.\npub fn keep<T>(x: T) -> T {\n    x\n}\n\
        /// A number kept.\npub fn kept(n: u32) -> u32 {\n    keep(n)\n}\n\
        /// A function applied.\n\
        pub fn apply<F: Fn(u32) -> u32>(f: F, x: u32) -> u32 {\n    f(x)\n}\n\
        /// One more.\npub fn applied(x: u32) -> u32 {\n    \
        apply(|y| y.saturating_add(1), x)\n}\n\
        /// A boxed function that may cross threads.\n\
        pub fn boxed(f: &(dyn Fn(u32) -> u32 + Send + Sync), x: u32) -> u32 {\n    f(x)\n}\n\
        /// A clone through a marker bound.\n\
        pub fn copied<T: Clone>(x: &T) -> T {\n    x.clone()\n}\n\
        /// A name through a supertrait.\n\
        pub fn named<N: Named>(n: &N) -> u32 {\n    n.name()\n}\n\
        /// Text the standard library shows.\n\
        pub fn shown<D: std::fmt::Display>(d: D) -> String {\n    d.to_string()\n}\n\
        /// A count that changes its counter.\n\
        pub fn bumped<C: Counter>(c: C) -> u32 {\n    let mut c = c;\n    c.bump()\n}\n\
        /// A counter as a number.\n\
        pub fn number<C: Counter>(c: C) -> u32 {\n    c.into()\n}\n\
        /// An inline module.\npub mod inner {\n    use super::Area;\n    \
        /// Twice, inside.\n    pub fn twice_inner<T: Area>(s: &T) -> u32 {\n        \
        s.area()\n    }\n}\n";

    /// The frontier and behaviour module of [`DICTIONARY_CRATE`], with
    /// `glob` written after its inner doc line.
    fn dictionary_projected(name: &str, glob: &str) -> (String, String) {
        let (head, rest) = DICTIONARY_CRATE.split_once('\n').expect("a doc line");
        let source = format!("{head}\n{glob}{rest}");
        projected(name, &[("src/lib.rs", source.as_str())])
    }

    #[test]
    fn a_generic_function_takes_the_dictionary_of_its_bound() {
        let (frontier, behavior) = dictionary_projected("dict", "");
        for (name, needle) in [
            (
                "twice",
                "type_parameters: [T], parameters: [tArea : (Fn ('T) Nat), \
                 tScaled : (Fn ('T Nat) Nat), s : 'T], result: Nat",
            ),
            ("twice", "((apply tArea s) + (apply tArea s))"),
            ("summed", "((apply tArea a) + (apply tScaled b 2))"),
            // A type parameter of the caller passes on its own dictionary.
            ("passed", "body: (twice ['U] uArea uScaled u)"),
            // A type of the crate passes its impl's methods.
            (
                "squareTwice",
                "body: (twice [Square] (ref squareAreaArea) (ref squareAreaScaled) s)",
            ),
            (
                "rectPassed",
                "body: (passed [Rect] (ref rectAreaArea) (ref rectAreaScaled) r)",
            ),
            (
                "rectsSummed",
                "body: (summed [Rect] (ref rectAreaArea) (ref rectAreaScaled) a b)",
            ),
            // No bound: the body can only move the value.
            (
                "keep",
                "type_parameters: [T], parameters: [x : 'T], result: 'T, body: x",
            ),
            ("kept", "body: (keep [Nat] n)"),
            // An `Fn` bound is the function type it names.
            ("applyValue", "parameters: [f : (Fn (Nat) Nat), x : Nat]"),
            ("applied", "body: (applyValue (fun (y : Nat) =>"),
            // Auto traits beside `Fn` in a trait object declare nothing.
            ("boxed", "parameters: [f : (Fn (Nat) Nat), x : Nat]"),
        ] {
            assert!(
                definition(&behavior, name).contains(needle),
                "{name} lacks {needle}:\n{}\n{frontier}",
                definition(&behavior, name)
            );
        }
    }

    #[test]
    fn a_bound_no_dictionary_reads_keeps_a_row_naming_it() {
        let (frontier, _) = dictionary_projected("rows", "");
        for (function, detail) in [
            (
                "rows::copied",
                "a method clone of a value of a type parameter, which its dictionary does not give",
            ),
            (
                "rows::named",
                "generic over N: Named, a trait with supertraits",
            ),
            (
                "rows::shown",
                "generic over D: std::fmt::Display, a bound the lift reads no dictionary of",
            ),
            (
                "rows::bumped",
                "generic over C: Counter::bump, a trait method that takes &mut self",
            ),
            (
                "rows::inner::twice_inner",
                "generic over T in an inline module",
            ),
            (
                "rows::number",
                "generic over C: Counter::into, a name the prelude's blanket impls give every \
                 type first",
            ),
        ] {
            let found = row(&frontier, function);
            assert!(found.contains(detail), "{function}: {found}");
        }
        // A glob import could bring in a trait whose method of the name
        // Rust finds first.
        let (frontier, _) = dictionary_projected("globbed", "use std::collections::*;\n");
        let found = row(&frontier, "globbed::twice");
        assert!(
            found.contains("generic over T in a scope that brings in names by a glob"),
            "{found}"
        );
    }

    // Class: a trait is a dictionary (kai, round 4, repair): what Rust's
    // method probe and trait selection can reach beside the bound's impl
    // for the named type stops the reading.

    /// A crate with a trait `Area`, a `Square` that implements it, a
    /// function generic over `T: Area`, and `extra` written after them.
    fn dictionary_repair_crate(extra: &str) -> String {
        format!(
            "//! Areas.\n\
             /// Something with an area.\n\
             pub trait Area {{\n    /// The area.\n    fn area(&self) -> u32;\n}}\n\
             /// A square.\npub struct Square {{\n    /// Its side.\n    pub side: u32,\n}}\n\
             impl Area for Square {{\n    fn area(&self) -> u32 {{\n        self.side\n    }}\n}}\n\
             /// The area of a value.\n\
             pub fn by_val<T: Area>(x: T) -> u32 {{\n    x.area()\n}}\n\
             /// A square's area.\npub fn square(s: Square) -> u32 {{\n    by_val(s)\n}}\n\
             {extra}"
        )
    }

    #[test]
    fn dictionary_repair_an_impl_for_a_wrapper_stops_the_bound() {
        // Each impl makes Rust select another impl than `Square`'s at a
        // call or a probe the lift reads as `Square`.
        for (name, extra) in [
            (
                "boxed",
                "impl<X: Area> Area for Box<X> {\n    fn area(&self) -> u32 {\n        1000\n    \
                 }\n}\n/// Boxed.\npub fn boxed_area<T: Area>(b: Box<T>) -> u32 {\n    \
                 b.area()\n}\n",
            ),
            (
                "byref",
                "impl Area for &Square {\n    fn area(&self) -> u32 {\n        77\n    }\n}\n\
                 /// Through a reference.\npub fn via_ref(s: &Square) -> u32 {\n    \
                 by_val(s)\n}\n",
            ),
            (
                "blanket",
                "impl<X: Area> Area for &X {\n    fn area(&self) -> u32 {\n        500\n    \
                 }\n}\n/// Through a reference.\npub fn via_ref(s: &Square) -> u32 {\n    \
                 by_val(s)\n}\n",
            ),
        ] {
            let source = dictionary_repair_crate(extra);
            let (frontier, behavior) = projected(name, &[("src/lib.rs", source.as_str())]);
            let found = row(&frontier, &format!("{name}::by_val"));
            assert!(
                found.contains("generic over T: Area, which the crate implements for"),
                "{name}: {found}"
            );
            assert!(!behavior.contains("(byVal [Square]"), "{name}:\n{behavior}");
        }
        // Without such an impl the reading holds.
        let source = dictionary_repair_crate("");
        let (frontier, behavior) = projected("plain", &[("src/lib.rs", source.as_str())]);
        assert!(
            definition(&behavior, "square").contains("(byVal [Square] (ref squareAreaArea) s)"),
            "{frontier}\n{behavior}"
        );
    }

    #[test]
    fn only_a_glob_that_may_leave_its_file_brings_a_macros_items() {
        let scope = |globs: &[&str]| crate::facts::ScopeFact {
            glob: !globs.is_empty(),
            globs: globs.iter().map(|g| g.to_string()).collect(),
            ..crate::facts::ScopeFact::default()
        };
        assert!(!scope(&["super"]).outward_glob(1));
        assert!(!scope(&[]).outward_glob(0));
        // The file's own scope: `super` is another file's module.
        assert!(scope(&["super"]).outward_glob(0));
        assert!(scope(&["crate::other"]).outward_glob(1));
        assert!(scope(&["super", "other"]).outward_glob(1));
    }

    // What a type name means: the leaf of a chain is named as the file
    // that declares the last type of the chain writes it.

    /// The row the frontier writes for `function`: its table and detail.
    fn type_names_row(frontier: &str, function: &str) -> String {
        let at = frontier
            .find(&format!("function = \"{function}\""))
            .unwrap_or_else(|| panic!("no row of {function}: {frontier}"));
        let start = frontier[..at].rfind("[[").unwrap();
        let end = frontier[at..]
            .find("\n\n")
            .map_or(frontier.len(), |e| at + e);
        frontier[start..end].to_string()
    }

    #[test]
    fn type_names_a_chain_ending_at_a_registry_type_is_a_foreign_row() {
        let frontier = resolved_frontier(
            "leaf-foreign",
            "\n[dependencies]\nbignum = \"1\"\nwide = \"1\"\n",
            "pub mod num {\n use wide::prelude::*;\n use bignum::BigX;\n\
             pub struct Ratio { pub q: BigX }\n}\n\
             pub mod calc {\n use crate::num::Ratio;\n\
             pub fn keep(r: Ratio) -> Ratio { r }\n}\n",
        );
        let row = type_names_row(&frontier, "w::calc::keep");
        assert!(row.starts_with("[[foreign]]"), "{row}");
        assert!(
            row.contains("Ratio.q: bignum::BigX, a type of the bignum crate"),
            "{row}"
        );
    }

    #[test]
    fn type_names_a_head_the_lift_does_not_read_is_the_leaf() {
        let frontier = resolved_frontier(
            "leaf-head",
            "\n[dependencies]\nbignum = \"1\"\n",
            "use std::cell::UnsafeCell;\nuse bignum::BigX;\n\
             pub struct Held { pub c: UnsafeCell<BigX> }\n\
             pub fn keep(h: Held) -> Held { h }\n",
        );
        let row = type_names_row(&frontier, "w::keep");
        assert!(row.starts_with("[[residue]]"), "{row}");
        assert!(row.contains("Held.c: UnsafeCell<bignum::BigX>"), "{row}");
    }

    #[test]
    fn type_names_a_registry_map_a_use_binds_is_not_the_standard_one() {
        let frontier = resolved_frontier(
            "leaf-map",
            "\n[dependencies]\nfastmap = \"1\"\n",
            "use fastmap::HashMap;\n\
             pub struct Index { pub at: HashMap<u8, u8> }\n\
             pub fn size(i: &Index) -> usize { i.at.len() }\n",
        );
        let row = type_names_row(&frontier, "w::size");
        assert!(row.starts_with("[[foreign]]"), "{row}");
        assert!(row.contains("a type of the fastmap crate"), "{row}");
    }

    #[test]
    fn type_names_types_of_two_inline_modules_of_a_file_are_two_types() {
        let frontier = resolved_frontier(
            "inline-types",
            "",
            "pub struct Shape { pub w: u8 }\npub fn wide(s: Shape) -> u8 { s.w }\n\
             pub mod ext {\n pub struct Shape { pub h: u8 }\n\
             pub fn tall(s: Shape) -> u8 { s.h }\n}\n",
        );
        assert!(!frontier.contains("declared twice"), "{frontier}");
        assert!(type_names_row(&frontier, "w::wide").starts_with("[[lifted]]"));
        assert!(type_names_row(&frontier, "w::ext::tall").starts_with("[[lifted]]"));
    }

    #[test]
    fn type_names_a_shared_name_reads_as_what_the_module_binds() {
        let frontier = resolved_frontier(
            "shared-glob",
            "",
            "pub mod a {\n pub struct Op { pub k: u8 }\n}\n\
             pub mod b {\n pub struct Op { pub j: u8 }\n}\n\
             pub mod c {\n use crate::b::*;\n pub fn j(o: Op) -> u8 { o.j }\n}\n\
             pub mod d {\n use std::cell::UnsafeCell;\n\
             pub struct Count { pub n: UnsafeCell<u8> }\n\
             pub fn keep(c: Count) -> Count { c }\n}\n\
             pub mod e {\n pub struct UnsafeCell { pub v: u8 }\n}\n\
             pub mod f {\n pub struct UnsafeCell { pub u: u8 }\n}\n",
        );
        assert!(
            type_names_row(&frontier, "w::c::j").starts_with("[[lifted]]"),
            "{frontier}"
        );
        let row = type_names_row(&frontier, "w::d::keep");
        assert!(row.contains("Count.n: std::cell::UnsafeCell<u8>"), "{row}");
        assert!(!frontier.contains("one of several types"), "{frontier}");
    }

    #[test]
    fn type_names_a_name_two_globs_of_outside_crates_may_bind_stays_open() {
        let frontier = resolved_frontier(
            "shared-open",
            "\n[dependencies]\nwide = \"1\"\nthin = \"1\"\n",
            "pub mod a {\n pub struct Op { pub k: u8 }\n}\n\
             pub mod b {\n pub struct Op { pub j: u8 }\n}\n\
             pub mod c {\n use wide::*;\n use thin::*;\n pub struct Holder { pub o: Op }\n\
             pub fn keep(h: Holder) -> Holder { h }\n}\n",
        );
        let row = type_names_row(&frontier, "w::c::keep");
        assert!(row.contains(crate::lift::SHARED_NAME), "{row}");
    }

    #[test]
    fn type_names_a_standard_map_with_a_hasher_reads_as_the_unordered_map() {
        let frontier = resolved_frontier(
            "hasher-map",
            "",
            "use std::collections::{HashMap, HashSet};\nuse std::hash::BuildHasherDefault;\n\
             pub struct Mix { pub state: u64 }\n\
             pub struct Index {\n pub at: HashMap<u8, u8, BuildHasherDefault<Mix>>,\n\
             pub seen: HashSet<u8, BuildHasherDefault<Mix>>,\n}\n\
             pub fn has(i: &Index, k: u8) -> bool { i.at.contains_key(&k) }\n\
             pub fn saw(i: &Index, k: u8) -> bool { i.seen.contains(&k) }\n\
             pub fn keys(i: &Index) -> Vec<u8> { i.at.keys().cloned().collect() }\n",
        );
        assert!(
            type_names_row(&frontier, "w::has").starts_with("[[lifted]]"),
            "{frontier}"
        );
        assert!(
            type_names_row(&frontier, "w::saw").starts_with("[[lifted]]"),
            "{frontier}"
        );
        let keys = type_names_row(&frontier, "w::keys");
        assert!(keys.contains("whose order is unspecified"), "{keys}");
    }

    #[test]
    fn qualified_declaration_identity_survives_registry_globs_ue_113() {
        let frontier = resolved_frontier(
            "resolved-glob-identity",
            "\n[dependencies]\nbits = \"1\"\n",
            "pub mod defs {pub enum Policy { First, Second }}\n\
             pub mod parent {pub use crate::defs::Policy;\n\
              pub mod wire { use bits::prelude::*;\n\
               pub struct Input {pub policy:super::Policy}\n\
               pub fn keep(value:Input)->Input {value}\n\
               pub struct Foreign {pub policy:Policy}\n\
               pub fn foreign(value:Foreign)->Foreign {value}\n\
               pub struct Missing {pub policy:super::missing::Policy}\n\
               pub fn missing(value:Missing)->Missing {value}\n\
              }\n}\n",
        );
        assert!(
            type_names_row(&frontier, "w::parent::wire::keep").starts_with("[[lifted]]"),
            "{frontier}"
        );
        assert!(
            type_names_row(&frontier, "w::parent::wire::foreign").contains("bits::prelude::Policy"),
            "{frontier}"
        );
        assert!(
            !type_names_row(&frontier, "w::parent::wire::missing").starts_with("[[lifted]]"),
            "{frontier}"
        );
    }

    #[test]
    fn type_names_a_name_only_a_registry_glob_binds_is_that_crates() {
        let frontier = resolved_frontier(
            "glob-leaf",
            "\n[dependencies]\nbits = \"1\"\n",
            "pub mod store {\n use bits::prelude::*;\n\
             pub struct Map { pub bits: BitBox, pub n: Vec<u8> }\n}\n\
             pub mod read {\n use crate::store::Map;\n pub fn keep(m: Map) -> Map { m }\n}\n\
             pub struct Shape { pub w: u8 }\npub struct Holder { pub s: Shape }\n\
             pub fn width(h: &Holder) -> u8 { h.s.w }\n\
             pub mod ext {\n use bits::prelude::*;\n pub fn lone(b: BitBox) -> BitBox { b }\n}\n",
        );
        let row = type_names_row(&frontier, "w::read::keep");
        assert!(
            row.contains("Map.bits: bits::prelude::BitBox, a type of the bits crate"),
            "{row}"
        );
        // The glob of an inline module binds names in that module only.
        assert!(
            type_names_row(&frontier, "w::width").starts_with("[[lifted]]"),
            "{frontier}"
        );
        assert!(type_names_row(&frontier, "w::ext::lone").contains("a type of the bits crate"));
    }

    #[test]
    fn type_names_a_self_type_written_by_a_path_reads_as_the_type_it_names() {
        let frontier = resolved_frontier(
            "owner-path",
            "",
            "pub struct Shape { pub w: u8 }\n\
             pub mod ext {\n impl super::Shape {\n  pub fn width(&self) -> u8 { self.w }\n }\n}\n",
        );
        let at = frontier.find("::width\"").expect("a row of width");
        let start = frontier[..at].rfind("[[").unwrap();
        assert!(frontier[start..].starts_with("[[lifted]]"), "{frontier}");
    }

    #[test]
    fn dictionary_repair_a_path_dependency_trait_in_scope_stops_the_bound() {
        // `loud` gives every type a by-value `area`, which Rust finds on
        // `x: T` before the bound's `area(&self)`.
        let source = dictionary_repair_crate("");
        let (head, rest) = source.split_once('\n').expect("a doc line");
        let sib = format!("{head}\nuse loud::Loud;\n{rest}");
        let (frontier, behavior) = projected(
            "sib",
            &[
                (
                    "Cargo.toml",
                    "[package]\nname = \"sib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
                     [dependencies]\nloud = { path = \"loud\" }\n\n[workspace]\n\
                     members = [\".\", \"loud\"]\n",
                ),
                ("src/lib.rs", sib.as_str()),
                (
                    "loud/Cargo.toml",
                    "[package]\nname = \"loud\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
                ),
                (
                    "loud/src/lib.rs",
                    "//! Loud.\n/// Loud.\npub trait Loud {\n    /// A number.\n    \
                     fn area(self) -> u32;\n}\n\
                     impl<X> Loud for X {\n    fn area(self) -> u32 {\n        999\n    }\n}\n",
                ),
            ],
        );
        let found = row(&frontier, "sib::by_val");
        assert!(
            found.contains(
                "generic over T in a scope that brings in names by a glob or from another crate"
            ),
            "{found}"
        );
        assert!(
            !behavior.contains("(sibByVal [") && !behavior.contains("(byVal ["),
            "{behavior}"
        );
    }

    #[test]
    fn dictionary_repair_a_literal_takes_the_width_the_other_argument_fixes() {
        let source = "//! Widths.\n/// The first of two.\n\
             pub fn first<T>(a: T, b: T) -> T {\n    let _ = b;\n    a\n}\n\
             /// A literal first, a byte second.\n\
             pub fn literal_first(x: u8) -> u8 {\n    first(200, x).wrapping_add(100)\n}\n\
             /// A byte first.\n\
             pub fn byte_first(x: u8) -> u8 {\n    first(x, 7).wrapping_add(100)\n}\n\
             /// Literals alone.\n\
             pub fn literals() -> u8 {\n    first(200, 7)\n}\n";
        let (frontier, behavior) = projected("wid", &[("src/lib.rs", source)]);
        let byte = definition(&behavior, "byteFirst");
        // Rust makes `200` a `u8`: the sum wraps at 2^8, as `byte_first`'s.
        let literal = definition(&behavior, "literalFirst");
        assert!(literal.contains(" 256 "), "{literal}\n{byte}\n{frontier}");
        assert!(!literal.contains("4294967296"), "{literal}");
        assert!(literal.contains("[Nat] 200 x"), "{literal}");
        // A type parameter only literals fix takes the type of the context.
        let found = row(&frontier, "wid::literals");
        assert!(
            found.contains("whose type parameter only an unsuffixed number literal fixes"),
            "{found}"
        );
    }

    // Mutation as state passing: a method that changes its receiver is the
    // function from the old value to its result and the new value, and a
    // caller rebinds the place it lent.

    /// The tally workspace, projected under its own directory `dir` (tests
    /// run at once, each in a directory of its own).
    fn mutation_projected(dir: &str) -> (String, String) {
        projected(
            dir,
            &[(
                "src/lib.rs",
                "//! Tallies.\n\
                 /// A tally.\npub struct Tally {\n    /// Its count.\n    pub count: u32,\n    \
                 /// Its marks.\n    pub marks: Vec<u32>,\n}\n\
                 /// A board.\npub struct Board {\n    /// Its tally.\n    pub tally: Tally,\n    \
                 /// Whether it was touched.\n    pub touched: bool,\n}\n\
                 impl Tally {\n    \
                 /// Count one more.\n    pub fn tick(&mut self) -> u32 {\n        \
                 self.count = self.count.saturating_add(1);\n        self.count\n    }\n    \
                 /// Mark a value.\n    pub fn mark(&mut self, m: u32) {\n        \
                 self.marks.push(m);\n    }\n    \
                 /// The tally with a count.\n    pub fn with(mut self, count: u32) -> Self {\n        \
                 self.count = count;\n        self\n    }\n    \
                 /// Itself, borrowed again.\n    pub fn again(&mut self) -> u32 {\n        \
                 let other = self;\n        other.count\n    }\n}\n\
                 impl Board {\n    /// Touch the board.\n    pub fn touch(&mut self) {\n        \
                 self.tally.mark(7);\n        self.touched = true;\n    }\n}\n\
                 /// Push onto a list.\npub fn push_on(out: &mut Vec<u32>, x: u32) {\n    \
                 out.push(x);\n}\n\
                 /// Two ticks of a fresh tally.\npub fn two() -> u32 {\n    \
                 let mut t = Tally { count: 0, marks: Vec::new() };\n    t.tick();\n    \
                 let n = t.tick();\n    let mut v = Vec::new();\n    push_on(&mut v, n);\n    \
                 t.count.saturating_add(v.len() as u32)\n}\n\
                 /// A tick as an operand.\npub fn inside(t: Tally) -> u32 {\n    \
                 let mut t = t;\n    t.tick().saturating_add(1)\n}\n",
            )],
        )
    }

    #[test]
    fn mutation_a_method_and_its_callers_thread_the_receiver() {
        let (frontier, behavior) = mutation_projected("tally");
        for function in [
            "tally::Tally::tick",
            "tally::Tally::mark",
            "tally::Tally::with",
            "tally::Board::touch",
            "tally::push_on",
            "tally::two",
        ] {
            let found = row(&frontier, function);
            assert!(found.starts_with("[[lifted]]"), "{found}");
        }
        // The new value after the result.
        let tick = definition(&behavior, "tallyTick");
        assert!(tick.contains("result: TupleNatTally"), "{tick}");
        assert!(
            tick.contains("(record TupleNatTally {item0 := threaded, item1 := selfValue1})"),
            "{tick}"
        );
        // A field's method writes the field back.
        let touch = definition(&behavior, "boardTouch");
        assert!(
            touch.contains("(tallyMark (project selfValue tally) 7)")
                && touch.contains("tally := (project threaded1 item1)"),
            "{touch}"
        );
        // A caller reads the receiver as each call leaves it.
        let two = definition(&behavior, "two");
        assert!(
            two.contains("(tallyTick t)")
                && two.contains("(let t1 : Tally := (project threaded1 item1)")
                && two.contains("(tallyTick t1)")
                && two.contains("(let n : Nat := (project threaded2 item0)")
                && two.contains("(pushOn v n)")
                && two.contains("(project t2 count)"),
            "{two}"
        );
        // An owned receiver is the method's own local.
        let with = definition(&behavior, "tallyWith");
        assert!(with.contains("result: Tally,"), "{with}");
    }

    #[test]
    fn mutation_a_change_nothing_writes_back_keeps_a_row() {
        let (frontier, _) = mutation_projected("tallies");
        for (function, detail) in [
            (
                "tallies::inside",
                "expression form outside the lift: a call of Tally::tick, which changes what it \
                 borrows mutably, as an operand",
            ),
            (
                "tallies::Tally::again",
                "expression form outside the lift: self used as itself, a second borrow of the \
                 place it lends, which the lift would read as a copy",
            ),
        ] {
            let found = row(&frontier, function);
            assert!(
                found.starts_with("[[residue]]")
                    && found.contains(&format!("detail = \"{detail}\"")),
                "{found}"
            );
        }
    }
}
