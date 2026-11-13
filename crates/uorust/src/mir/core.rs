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
use crate::lift::{self, Types};
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
    // The pointer width the dumps were built for, from the `target.toml`
    // that `scripts/mir/dump-lib.sh` writes beside them (the target triple
    // and its `target_pointer_width` as rustc prints it); every width a
    // target may have when there is none.
    let file = dumps.join("target.toml");
    let mut layouts = Vec::new();
    let widths = match std::fs::read_to_string(&file) {
        Err(_) => lift::pointer_widths(&[]),
        Ok(text) => {
            let bad = |reason: String| UorustError::Malformed {
                path: file.clone(),
                reason,
            };
            let doc: toml::Table = text.parse().map_err(|e| bad(format!("{e}")))?;
            let triple = doc
                .get("triple")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| bad("no triple".into()))?;
            let width = doc
                .get("pointer_width")
                .and_then(toml::Value::as_integer)
                .and_then(|w| u32::try_from(w).ok())
                .ok_or_else(|| bad("no pointer_width".into()))?;
            // Its data layout, which `dump-lib.sh` records beside them.
            layouts = super::layout::from_target_toml(&doc).map_err(|r| bad(r.to_string()))?;
            vec![(width, vec![triple.to_string()])]
        }
    };
    let mut read = Vec::new();
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
        read.push((module, texts));
    }
    // Every module's bodies, which each module's calls name.
    let link = super::link::CoreLink::new(
        core_decls(root),
        root,
        read.iter().flat_map(|(_, texts)| texts),
    );
    Ok(read
        .iter()
        .map(|(module, texts)| shard(scope, root, module, texts, &widths, &layouts, &link))
        .collect())
}

/// One module's census of the crate `scope` names from the texts of its
/// dumps, at the pointer widths they were built for, its calls into the
/// crate those of `link`'s bodies.
pub(crate) fn shard(
    scope: &Scope,
    root: &Path,
    module: &str,
    texts: &[String],
    widths: &[(u32, Vec<String>)],
    layouts: &[super::layout::DataLayout],
    link: &super::link::CoreLink,
) -> CoreShard {
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
                // A constant item's dump names it by its full path (`//
                // MIR for `fmt::flags::WIDTH_FLAG``), which its uses print,
                // where its own text names it by its name alone.
                let full = text
                    .lines()
                    .next()
                    .and_then(|l| l.strip_prefix("// MIR for `"))
                    .and_then(|r| r.split_once('`'))
                    .map(|(p, _)| p);
                if let Some(full) = full.filter(|f| f.ends_with(&format!("::{}", body.path))) {
                    consts
                        .entry(full.to_string())
                        .or_insert_with(|| body.clone());
                }
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
    let explicit = instantiate_structs(&mut types, &link.decls, &bodies);
    // A raw pointer: an address, its provenance and its metadata.
    types
        .structs
        .insert(lower::POINTER.to_string(), lower::pointer_fields());
    // The memory raw pointers reach (see `super::memory`).
    types.structs.insert(
        super::memory::MEMORY.to_string(),
        super::memory::memory_fields(),
    );
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
            widths,
            callees: &callees,
            krate: krate.to_string(),
            taken: &none,
            explicit: &explicit,
            limit: crate::project::MAX_FILE_BYTES,
            consts: &consts,
            closures: &closures,
            active: &active,
            core: Some(link),
            layouts,
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
    pub(crate) path: String,
    params: Vec<String>,
    fields: Vec<(Option<String>, String)>,
    /// For an enum whose variants hold no data, its variants; such an enum
    /// has no fields.
    variants: Option<Vec<String>>,
    /// Whether such an enum's variants set their own discriminants.
    explicit: bool,
}

impl CoreStruct {
    /// Its name, the last segment of its path.
    pub(crate) fn name(&self) -> &str {
        self.path.rsplit("::").next().unwrap_or(&self.path)
    }
}

/// What core's source declares that names its types: its structs and enums
/// of unit variants, and each module's `use` names, by module and name, to
/// the path each names from core's root.
#[derive(Clone, Debug, Default)]
pub(crate) struct CoreDecls {
    pub(crate) structs: Vec<CoreStruct>,
    uses: BTreeMap<(String, String), String>,
    /// Each free function's generic parameters, types and constants in the
    /// order its signature declares them, by its path from core's root.
    pub(crate) generics: BTreeMap<String, Vec<String>>,
    /// The intrinsics core declares without a body of their own, by path.
    pub(crate) intrinsics: std::collections::BTreeSet<String>,
    /// The traits core declares, by path: a field's trait object names one.
    traits: std::collections::BTreeSet<String>,
}

/// Each name a `use` tree binds in `module`, with the path from core's root
/// it names: `crate::`, `core::`, `self::` and `super::` as Rust reads
/// them, and any other path relative to the module.
fn use_names(
    module: &[String],
    prefix: &[String],
    tree: &syn::UseTree,
    out: &mut Vec<(String, String)>,
) {
    let with = |s: String| {
        let mut p = prefix.to_vec();
        p.push(s);
        p
    };
    let use_target = |module: &[String], path: &[String]| -> Vec<String> {
        let mut at: Vec<String> = module.to_vec();
        let mut rest = path;
        match rest.first().map(String::as_str) {
            Some("crate" | "core") => {
                at.clear();
                rest = &rest[1..];
            }
            Some("self") => rest = &rest[1..],
            _ => {}
        }
        while rest.first().map(String::as_str) == Some("super") {
            at.pop();
            rest = &rest[1..];
        }
        at.extend(rest.iter().cloned());
        at
    };
    match tree {
        syn::UseTree::Path(p) => use_names(module, &with(p.ident.to_string()), &p.tree, out),
        syn::UseTree::Name(n) if n.ident == "self" => {
            if let Some(last) = prefix.last() {
                out.push((last.clone(), use_target(module, prefix).join("::")));
            }
        }
        syn::UseTree::Name(n) => out.push((
            n.ident.to_string(),
            use_target(module, &with(n.ident.to_string())).join("::"),
        )),
        syn::UseTree::Rename(r) => out.push((
            r.rename.to_string(),
            use_target(module, &with(r.ident.to_string())).join("::"),
        )),
        syn::UseTree::Group(g) => {
            for t in &g.items {
                use_names(module, prefix, t, out);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

/// A signature's generic parameters, types and constants, in order.
fn generic_names(g: &syn::Generics) -> Vec<String> {
    g.params
        .iter()
        .filter_map(|p| match p {
            syn::GenericParam::Type(t) => Some(t.ident.to_string()),
            syn::GenericParam::Const(c) => Some(c.ident.to_string()),
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect()
}

/// Every struct and enum of unit variants of core's source under `root` (the
/// directory holding `library/core/src`), and every module's `use` names: a
/// file's module is its path there.
pub(crate) fn core_decls(root: &Path) -> CoreDecls {
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
    fn items(module: &[String], list: &[syn::Item], out: &mut CoreDecls) {
        let at = |name: String| {
            let mut path = module.to_vec();
            path.push(name);
            path.join("::")
        };
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
                            // The type as MIR prints it: a path's segments
                            // joined without spaces.
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
                    out.structs.push(CoreStruct {
                        path: at(st.ident.to_string()),
                        params,
                        fields,
                        variants: None,
                        explicit: false,
                    });
                }
                syn::Item::Enum(e)
                    if e.generics.params.is_empty()
                        && e.variants.iter().all(|v| v.fields.is_empty()) =>
                {
                    out.structs.push(CoreStruct {
                        path: at(e.ident.to_string()),
                        params: Vec::new(),
                        fields: Vec::new(),
                        variants: Some(e.variants.iter().map(|v| v.ident.to_string()).collect()),
                        explicit: e.variants.iter().any(|v| v.discriminant.is_some()),
                    });
                }
                // An inherent impl's methods, by the impl's type: their
                // own generic parameters, which a call's turbofish binds.
                syn::Item::Impl(imp) if imp.trait_.is_none() => {
                    if let syn::Type::Path(tp) = &*imp.self_ty {
                        if let Some(seg) = tp.path.segments.last() {
                            for it in &imp.items {
                                if let syn::ImplItem::Fn(f) = it {
                                    out.generics
                                        .entry(at(format!("{}::{}", seg.ident, f.sig.ident)))
                                        .or_insert_with(|| generic_names(&f.sig.generics));
                                }
                            }
                        }
                    }
                }
                syn::Item::Trait(t) => {
                    out.traits.insert(at(t.ident.to_string()));
                }
                syn::Item::Fn(f) => {
                    out.generics
                        .insert(at(f.sig.ident.to_string()), generic_names(&f.sig.generics));
                }
                // A function without a body, as core declares an intrinsic.
                syn::Item::Verbatim(tokens) => {
                    if let Ok(f) = syn::parse2::<syn::ForeignItemFn>(tokens.clone()) {
                        let path = at(f.sig.ident.to_string());
                        if f.attrs.iter().any(|a| a.path().is_ident("rustc_intrinsic")) {
                            out.intrinsics.insert(path.clone());
                        }
                        out.generics.insert(path, generic_names(&f.sig.generics));
                    }
                }
                syn::Item::Use(u) => {
                    let mut names = Vec::new();
                    use_names(module, &[], &u.tree, &mut names);
                    for (name, target) in names {
                        out.uses.insert((module.join("::"), name), target);
                    }
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
    let mut out = CoreDecls::default();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(parsed) = syn::parse_file(&text).or_else(|_| {
            let tokens: proc_macro2::TokenStream = text.parse()?;
            syn::parse2::<syn::File>(without_const_traits(tokens))
        }) else {
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

/// `tokens` without the const-trait syntax core is written in and syn does
/// not read: the `const` of `impl const Trait` and `impl<..> const Trait`,
/// and the `const`, `[const]` and `~const` of a bound. What is left declares the
/// same items.
fn without_const_traits(tokens: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    use proc_macro2::{Group, TokenTree};
    let is = |t: &TokenTree, s: &str| matches!(t, TokenTree::Ident(i) if i == s);
    let punct = |t: &TokenTree, c: char| matches!(t, TokenTree::Punct(p) if p.as_char() == c);
    let mut out: Vec<TokenTree> = Vec::new();
    // Within an impl's generics, the depth of their angle brackets.
    let mut generics: Option<usize> = None;
    let mut after_impl = false;
    let mut iter = tokens.into_iter().peekable();
    while let Some(t) = iter.next() {
        if after_impl && is(&t, "const") {
            after_impl = false;
            continue;
        }
        // A bound `T: const Trait` or `A + const Trait`.
        if is(&t, "const") && out.last().is_some_and(|l| punct(l, ':') || punct(l, '+')) {
            continue;
        }
        if punct(&t, '~') && iter.peek().is_some_and(|n| is(n, "const")) {
            iter.next();
            continue;
        }
        if let TokenTree::Group(g) = &t {
            let inner: Vec<TokenTree> = g.stream().into_iter().collect();
            if g.delimiter() == proc_macro2::Delimiter::Bracket
                && inner.len() == 1
                && is(&inner[0], "const")
            {
                continue;
            }
            let mut ng = Group::new(g.delimiter(), without_const_traits(g.stream()));
            ng.set_span(g.span());
            out.push(TokenTree::Group(ng));
            after_impl = false;
            continue;
        }
        // The `>` of `->` closes no bracket.
        let arrow = out.last().is_some_and(|l| punct(l, '-'));
        match generics {
            _ if arrow => {}
            Some(d) if punct(&t, '<') => generics = Some(d + 1),
            Some(1) if punct(&t, '>') => {
                generics = None;
                after_impl = true;
                out.push(t);
                continue;
            }
            Some(d) if punct(&t, '>') => generics = Some(d - 1),
            _ => {}
        }
        if generics.is_none() {
            after_impl = is(&t, "impl");
            if after_impl && iter.peek().is_some_and(|n| punct(n, '<')) {
                out.push(t);
                out.push(iter.next().expect("peeked"));
                generics = Some(1);
                after_impl = false;
                continue;
            }
        }
        out.push(t);
    }
    out.into_iter().collect()
}

/// The types a type as MIR prints it is built of, itself first: the
/// referents of its references, the elements of its arrays and slices, the
/// fields of its tuples and the arguments of its paths.
fn components(text: &str, out: &mut Vec<String>) {
    let mut t = text.trim();
    while let Some(r) = t
        .strip_prefix('&')
        .or_else(|| t.strip_prefix("*const "))
        .or_else(|| t.strip_prefix("*mut "))
    {
        t = r.trim_start();
        if let Some(r) = t.strip_prefix('\'') {
            t = r.split_once(' ').map_or("", |(_, r)| r);
        }
        t = t.strip_prefix("mut ").unwrap_or(t).trim_start();
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        components(parse::split_top(inner, ';')[0], out);
        return;
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        for p in parse::split_top(inner, ',') {
            components(p, out);
        }
        return;
    }
    if t.is_empty() {
        return;
    }
    out.push(t.to_string());
    if let Some(open) = t.find('<') {
        for a in parse::split_top(t[open + 1..].strip_suffix('>').unwrap_or(""), ',') {
            components(a, out);
        }
    }
}

/// Core's declared types in a body's scope, instantiated as its bodies
/// mention them.
struct Instances<'a> {
    decls: &'a CoreDecls,
    /// Each declared path, where one declaration (or one enum declared once
    /// for each configuration) has it.
    by: BTreeMap<String, CoreStruct>,
    /// Each instance's lifted name, or `None` where it does not lift, by its
    /// path from core's root and arguments.
    done: BTreeMap<String, Option<String>>,
    explicit: std::collections::BTreeSet<String>,
}

impl Instances<'_> {
    /// The declared path `path` names from core's root, through the `use`
    /// names that re-export it.
    fn canonical(&self, path: &str, depth: usize) -> Option<String> {
        if self.by.contains_key(path) || self.decls.traits.contains(path) {
            return Some(path.to_string());
        }
        let (module, name) = path.rsplit_once("::").unwrap_or(("", path));
        let target = self
            .decls
            .uses
            .get(&(module.to_string(), name.to_string()))?;
        (depth < 16 && target != path).then(|| self.canonical(target, depth + 1))?
    }

    /// The declared path `path`, written in `module`, names: through the
    /// module's `use` name for its first segment, else from the module or
    /// from core's root.
    fn resolve(&self, module: &str, path: &str) -> Option<String> {
        let (head, rest) = path
            .split_once("::")
            .map_or((path, None), |(h, r)| (h, Some(r)));
        if let Some(t) = self.decls.uses.get(&(module.to_string(), head.to_string())) {
            let full = rest.map_or_else(|| t.clone(), |r| format!("{t}::{r}"));
            return self.canonical(&full, 0);
        }
        let local = if module.is_empty() {
            path.to_string()
        } else {
            format!("{module}::{path}")
        };
        self.canonical(&local, 0)
            .or_else(|| self.canonical(path, 0))
    }

    /// The declared path a path as MIR prints it names: from core's root,
    /// or the one declared path it ends.
    fn printed(&self, path: &str) -> Option<String> {
        if let Some(p) = self.canonical(path, 0) {
            return Some(p);
        }
        let tail = format!("::{path}");
        let mut ends = self.by.keys().filter(|p| p.ends_with(&tail));
        match (ends.next(), ends.next()) {
            (Some(p), None) => Some(p.clone()),
            _ => None,
        }
    }

    /// `text`, a type written in `module`, with each path that names a
    /// declared type written from core's root.
    fn rooted(&self, module: &str, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if (c.is_ascii_alphabetic() || c == '_') && !(i > 0 && chars[i - 1] == '\'') {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric()
                        || chars[i] == '_'
                        || (chars[i] == ':'
                            && chars.get(i + 1) == Some(&':')
                            && chars
                                .get(i + 2)
                                .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')))
                {
                    i += if chars[i] == ':' { 2 } else { 1 };
                }
                let word: String = chars[start..i].iter().collect();
                out.push_str(&self.resolve(module, &word).unwrap_or(word));
            } else {
                out.push(c);
                i += 1;
            }
        }
        out
    }

    /// The lifted name of the declared type `path` at `args`, each of its
    /// fields' types lifted first.
    fn instance(&mut self, path: &str, args: &[&str], types: &mut Types) -> Option<String> {
        let key = format!("{path}<{}>", args.join(","));
        if let Some(n) = self.done.get(&key) {
            return n.clone();
        }
        self.done.insert(key.clone(), None);
        // A type the std table models (a lang item core builds of raw
        // pointers, as `fmt::Arguments`) is the table's.
        if super::stdlib::model_type(&format!("core::{path}"), &[]).is_some() {
            return None;
        }
        let st = self.by.get(path)?.clone();
        // An enum the scope lifts as its own, of the same variants, keeps
        // its lift.
        let own = types.resolve(st.name());
        if types
            .enums
            .get(&own)
            .is_some_and(|vs| Some(vs) == st.variants.as_ref())
        {
            self.done.insert(key, Some(own.clone()));
            return Some(own);
        }
        let mut parts: Vec<&str> = vec!["core"];
        parts.extend(path.split("::"));
        if st.variants.is_some() {
            let name = crate::lex::pascal(&parts);
            if st.explicit {
                self.explicit.insert(name.clone());
            }
            types
                .enums
                .insert(name.clone(), st.variants.clone().unwrap_or_default());
            types.aliases.insert(path.to_string(), name.clone());
            self.done.insert(key, Some(name.clone()));
            return Some(name);
        }
        if st.params.len() != args.len() {
            return None;
        }
        let module = path.rsplit_once("::").map_or("", |(m, _)| m);
        let binds: BTreeMap<String, String> = st
            .params
            .iter()
            .cloned()
            .zip(args.iter().map(|a| (*a).to_string()))
            .collect();
        let mut fields = Vec::new();
        for (i, (name, t)) in st.fields.iter().enumerate() {
            let t = self.rooted(module, &super::stdlib::substitute(t, &binds));
            self.mention(&t, types);
            let ft = super::lower::ty(&t, types).ok()?;
            fields.push((
                name.as_deref()
                    .map_or_else(|| format!("item{i}"), lift::local_name),
                ft,
            ));
        }
        parts.extend(args.iter().copied());
        let name = crate::lex::pascal(&parts);
        types.structs.insert(name.clone(), fields);
        let text = if args.is_empty() {
            path.to_string()
        } else {
            format!("{path}<{}>", args.join(", "))
        };
        types.aliases.insert(text, name.clone());
        self.done.insert(key, Some(name.clone()));
        Some(name)
    }

    /// Each declared type `text` (as MIR prints it) is built of, lifted and
    /// named as it is printed.
    fn mention(&mut self, text: &str, types: &mut Types) {
        let mut parts = Vec::new();
        components(text, &mut parts);
        // Arguments before the types they are arguments of.
        for c in parts.into_iter().rev() {
            // Its path and its type arguments, lifetimes left out.
            let (path, args) = match c.find('<') {
                Some(open) => (
                    c[..open].trim_end_matches("::"),
                    parse::split_top(c[open + 1..].strip_suffix('>').unwrap_or(""), ',')
                        .into_iter()
                        .map(str::trim)
                        .filter(|a| !a.is_empty() && !a.starts_with('\''))
                        .collect(),
                ),
                None => (c.as_str(), Vec::new()),
            };
            let Some(declared) = self.printed(path) else {
                continue;
            };
            if let Some(n) = self.instance(&declared, &args, types) {
                types.aliases.entry(c.clone()).or_insert(n);
            }
        }
    }
}

/// Each instance of a core struct the bodies' locals mention, as MIR prints
/// it, added to core's scope: a struct whose fields lift is a record of
/// those fields at the instance's arguments, named by its path and
/// arguments, each field's type read in the struct's module through its
/// `use` names. An enum of unit variants core declares once for each
/// configuration (as it does by pointer width) is one enum of all its
/// declarations' variants; a path two other declarations have names
/// neither. The enums whose variants set their own discriminants are
/// returned: a switch on one is residue.
fn instantiate_structs(
    types: &mut Types,
    decls: &CoreDecls,
    bodies: &[(IrBody, String, String)],
) -> std::collections::BTreeSet<String> {
    let mut groups: BTreeMap<&str, Vec<&CoreStruct>> = BTreeMap::new();
    for s in &decls.structs {
        groups.entry(s.path.as_str()).or_default().push(s);
    }
    let mut by = BTreeMap::new();
    for (path, group) in groups {
        let one = match group.as_slice() {
            [one] => (*one).clone(),
            all if all.iter().all(|s| s.variants.is_some()) => {
                let mut vs: Vec<String> = Vec::new();
                for v in all.iter().flat_map(|s| s.variants.iter().flatten()) {
                    if !vs.contains(v) {
                        vs.push(v.clone());
                    }
                }
                CoreStruct {
                    variants: Some(vs),
                    explicit: all.iter().any(|s| s.explicit),
                    ..all[0].clone()
                }
            }
            _ => continue,
        };
        by.insert(path.to_string(), one);
    }
    let mut cx = Instances {
        decls,
        by,
        done: BTreeMap::new(),
        explicit: std::collections::BTreeSet::new(),
    };
    let mut mentioned: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for (body, _, _) in bodies {
        mentioned.extend(body.locals.iter().map(String::as_str));
    }
    // The types the std table's models build values of.
    mentioned.extend(
        super::stdlib::table()
            .instantiate
            .iter()
            .map(String::as_str),
    );
    for text in mentioned {
        cx.mention(text, types);
    }
    cx.explicit
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
