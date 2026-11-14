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
    /// The digest of the body's embedding in LexLean (`uorust embed`),
    /// which every body has.
    pub embedded: Option<String>,
    /// The constructs its embedding uses that the evaluator does not
    /// define.
    pub blocked: Vec<String>,
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
        &[],
    )
}

/// Translate every body of the module shards under `dumps` (one directory
/// per module) of the crate `scope` names, whose dumps name their sources
/// relative to `root`.
///
/// Every module's dumps are linked, so a call into another module is that
/// module's body; with `only` nonempty, only the modules it names are
/// translated (a CI shard's), and their rows are those of the whole census.
pub fn census(
    scope: &Scope,
    root: &Path,
    dumps: &Path,
    only: &[String],
) -> Result<Vec<CoreShard>, UorustError> {
    // A library's declarations are read from its sources: without them
    // every type the bodies name is unknown, so the census refuses.
    if scope.own_std {
        crate::embed::dir_paths(&root.join("library").join(&scope.krate).join("src"))?;
    }
    let modules: Vec<PathBuf> = crate::embed::dir_paths(dumps)?
        .into_iter()
        .filter(|p| p.is_dir())
        .collect();
    // The pointer width the dumps were built for, from the `target.toml`
    // that `scripts/mir/dump-lib.sh` writes beside them (the target triple
    // and its `target_pointer_width` as rustc prints it); every width a
    // target may have when there is none.
    let file = dumps.join("target.toml");
    let mut layouts = Vec::new();
    let widths = match crate::embed::read_text(&file) {
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
            layouts = super::layout::from_target_toml(&doc)
                .map_err(|r| bad(super::layout::reason(&r)))?;
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
        let files = crate::embed::dir_paths(&dir)?;
        let mut dumps = Vec::new();
        for file in &files {
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            dumps.push((name, crate::embed::read_text(file)?));
        }
        read.push((module, dumps));
    }
    // Every module's bodies, which each module's calls name, with the
    // names of the files they are dumped to.
    let link = super::link::CoreLink::new(
        core_decls(root),
        root,
        read.iter()
            .flat_map(|(_, dumps)| dumps.iter().map(|(n, t)| (n.as_str(), t))),
    );
    Ok(read
        .iter()
        .filter(|(module, _)| only.is_empty() || only.contains(module))
        .map(|(module, dumps)| shard(scope, root, module, dumps, &widths, &layouts, &link))
        .collect())
}

/// One module's census of the crate `scope` names from its dumps (each
/// file's name and text), at the pointer widths they were built for, its
/// calls into the crate those of `link`'s bodies.
pub(crate) fn shard(
    scope: &Scope,
    root: &Path,
    module: &str,
    dumps: &[(String, String)],
    widths: &[(u32, Vec<String>)],
    layouts: &[super::layout::DataLayout],
    link: &super::link::CoreLink,
) -> CoreShard {
    let krate = scope.krate.as_str();
    // Each body at its instantiation, with the bindings and its own text.
    let mut bodies: Vec<(IrBody, String, String)> = Vec::new();
    // Each body's embedding: its digest and the constructs that block it.
    let mut embeddings: Vec<(String, Vec<String>)> = Vec::new();
    let enums = crate::embed::enums(&link.decls);
    let structs = crate::embed::structs(&link.decls);
    let resolve = |callee: &str, args: &[Option<String>], dest: &str| {
        crate::embed::resolve_callee(link, krate, callee, args, dest)
    };
    let width = widths.first().map_or(usize::BITS, |(w, _)| *w);
    let big_endian = layouts.first().map(|l| l.big_endian);
    // Each body's instance at its `Fn`-bounded parameters' functions, where
    // it has any: lowered first, the canonical instance where it does not
    // lower.
    let mut variants: Vec<Option<(IrBody, String)>> = Vec::new();
    let mut consts: BTreeMap<String, IrBody> = BTreeMap::new();
    let mut closures: BTreeMap<String, IrBody> = BTreeMap::new();
    let parts = crate::embed::parts(
        krate,
        link,
        dumps.iter().map(|(n, t)| (n.as_str(), t.as_str())),
    );
    let statics = crate::embed::statics(link);
    for (name, text) in dumps {
        let embedder = crate::embed::Cx {
            width,
            enums: &enums,
            resolve: &resolve,
            allocs: crate::embed::allocations(text),
            big_endian,
            structs: &structs,
            cells: crate::embed::allocation_cells(text, width),
            items: Default::default(),
            base: Default::default(),
            parts: &parts,
            stem: crate::embed::dump_stem(name).to_string(),
            owner: Default::default(),
            statics: &statics,
        };
        // The generic parameters core's source declares for the dump's
        // function, which its signature prints by their bare names among
        // the bare names of its types.
        let printed = text
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// MIR for `"))
            .and_then(|l| l.split_once('`'))
            .map(|(p, _)| p);
        let resolved = printed.and_then(|p| super::resolve_impls(root, p));
        // The const parameters of the impls the path names by their spans.
        let impl_consts = printed
            .map(|p| impl_const_types(root, p))
            .unwrap_or_default();
        let declared = resolved
            .as_deref()
            .map(|p| link.decls.parameters(p))
            .unwrap_or_default();
        let generics = canonical_bindings(text, &|w| declared.contains(w), &|w| {
            resolved
                .as_deref()
                .and_then(|p| link.decls.const_type(p, w))
                .or_else(|| {
                    let written = printed.and_then(|p| written_impls(root, p))?;
                    link.decls.const_type(&written, w)
                })
                .or_else(|| impl_consts.get(w).cloned())
        });
        // A parameter bounded by `Fn` or `FnOnce` is the function its bound
        // states, at the other parameters' bindings: the body lifts for
        // every function of that type, and `<F as FnOnce<A>>::Output` is
        // the bound's result.
        let mut fn_generics = generics.clone();
        let mut outputs: Vec<(String, String)> = Vec::new();
        if let Some(fns) = resolved
            .as_deref()
            .and_then(|p| link.decls.fn_bounds.get(p))
        {
            for (p, bound) in fns {
                if generics.get(p).map(String::as_str) == Some(CANONICAL) {
                    let bound = super::stdlib::substitute(bound, &generics);
                    if let Some((_, out)) = bound.rsplit_once(" -> ") {
                        outputs.push((p.clone(), out.to_string()));
                    }
                    fn_generics.insert(p.clone(), bound);
                }
            }
        }
        let at = generics
            .iter()
            .map(|(p, t)| format!("{p}={t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let instance: String = text
            .lines()
            .map(|l| super::stdlib::substitute(&opaque_projections(l, &generics), &generics) + "\n")
            .collect();
        // The body at its `Fn`-bounded parameters' functions, where it has
        // any, which is lowered first.
        let fn_at = fn_generics
            .iter()
            .map(|(p, t)| format!("{p}={t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let fn_instance: Option<String> = (!outputs.is_empty()).then(|| {
            text.lines()
                .map(|l| {
                    // `<F as FnOnce<A>>::Output` of a function parameter:
                    // its bound's result.
                    let mut l = l.to_string();
                    for (p, out) in &outputs {
                        let open = format!("<{p} as ");
                        while let Some(at) = l.find(&open) {
                            let mut depth = 0i64;
                            let mut end = None;
                            let cs: Vec<(usize, char)> = l[at..].char_indices().collect();
                            for (j, (i, c)) in cs.iter().enumerate() {
                                match c {
                                    '<' => depth += 1,
                                    '>' if j > 0 && cs[j - 1].1 == '-' => {}
                                    '>' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            end = Some(at + i + 1);
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            match end.filter(|e| l[*e..].starts_with("::Output")) {
                                Some(e) => l.replace_range(at..e + "::Output".len(), out),
                                None => break,
                            }
                        }
                    }
                    super::stdlib::substitute(&opaque_projections(&l, &fn_generics), &fn_generics)
                        + "\n"
                })
                .collect()
        });
        let mut fn_bodies = fn_instance
            .as_deref()
            .map(parse::bodies)
            .unwrap_or_default()
            .into_iter();
        for (body, original) in parse::bodies(&instance)
            .into_iter()
            .zip(parse::bodies(text))
        {
            let fn_body = fn_bodies.next();
            // A constant: a promoted one, a constant item, or an anonymous
            // constant (`u16::{constant#0}`, an expression in a const
            // context of its function's body, which its uses name).
            if body.arity == 0
                && (body.path.contains("promoted[")
                    || body.path.contains("{constant#")
                    || !body.path.contains('{'))
            {
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
            let embedded = embedder.body(
                &original,
                &crate::embed::program_key(
                    link,
                    &original.path,
                    &crate::embed::body_kappa(krate, &original.path, text),
                ),
            );
            embeddings.push((
                crate::project::sha(embedded.term.as_bytes()),
                embedded.blocked.into_iter().collect(),
            ));
            let mut body = body;
            body.path = path.clone();
            variants.push(fn_body.map(|mut b| {
                b.path = path;
                (b, fn_at.clone())
            }));
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
    // A closure's type is the struct of the values it captures, as the
    // crate path reads it.
    types.closures = closures
        .iter()
        .filter(|(_, b)| b.upvars.iter().all(Option::is_some))
        .map(|(t, b)| {
            let captures = b.upvars.iter().flatten().map(|(_, t)| t.clone()).collect();
            (t.clone(), captures)
        })
        .collect();
    // The structs the bodies name, and those their instances at their
    // `Fn`-bounded parameters' functions name (`FromFn<impl Fn() -> T>`).
    let scanned: Vec<(IrBody, String, String)> = bodies
        .iter()
        .cloned()
        .chain(
            variants
                .iter()
                .flatten()
                .map(|(b, a)| (b.clone(), a.clone(), String::new())),
        )
        .collect();
    let mut explicit = instantiate_structs(&mut types, link, &scanned, &closures);
    // The discriminants an enum's declarations set (`cmp::Ordering`'s -1,
    // 0 and 1), where those whose variants the lift's enum holds agree on
    // them: a switch reads them, and so does a discriminant read as a
    // value. One whose declarations disagree, or set them by no literal,
    // is refused.
    let enums: Vec<(String, Vec<String>)> = types
        .enums
        .iter()
        .map(|(n, vs)| (n.clone(), vs.clone()))
        .collect();
    for (n, vs) in enums {
        let mut found = link
            .decls
            .structs
            .iter()
            .filter(|s| s.explicit && s.variants.as_ref() == Some(&vs))
            .map(|s| s.values.clone());
        let Some(first) = found.next() else {
            continue;
        };
        let agreed = found.all(|v| v == first);
        match first {
            Some(v) if agreed && v.len() == vs.len() => {
                types.discriminants.insert(n.clone(), v.clone());
                explicit.remove(&n);
            }
            _ => {
                explicit.insert(n);
            }
        }
    }
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
    for (((body, at, kappa), variant), (embedded, blocked)) in
        bodies.into_iter().zip(variants).zip(embeddings)
    {
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
            backward: false,
        };
        let lowered = |b: &IrBody| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lower::lower(&cx, b, &name)))
                .map_err(|_| "mir::Translator: panicked".to_string())
                .and_then(|r| r.map_err(|e| crate::project::mir_residue(e).detail()))
        };
        // The instance at its parameters' functions where it lowers, else
        // the canonical one; where neither does, the cause is the former's,
        // which got further than an instance its bounds refuse.
        let mut at = at;
        let outcome = match variant.map(|(b, a)| (lowered(&b), a)) {
            Some((Ok(l), a)) => {
                at = a;
                Ok(l)
            }
            Some((Err(e), a)) => lowered(&body).map_err(|_| {
                at = a;
                e
            }),
            None => lowered(&body),
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
            embedded: Some(embedded),
            blocked,
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
pub(crate) fn numbered_allocations(text: &str) -> String {
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
    pub(crate) params: Vec<String>,
    pub(crate) fields: Vec<(Option<String>, String)>,
    /// For an enum whose variants hold no data, its variants; such an enum
    /// has no fields.
    pub(crate) variants: Option<Vec<String>>,
    /// For an enum whose variants hold data, each variant and its fields.
    #[allow(clippy::type_complexity)]
    pub(crate) data: Option<Vec<(String, Vec<(Option<String>, String)>)>>,
    /// Whether such an enum's variants set their own discriminants.
    pub(crate) explicit: bool,
    /// Such an enum's discriminants, in its variants' order, where each is
    /// a literal or follows one.
    /// Each is held as its low 64 bits, two's complement, as `discriminant`
    /// reads it back: the workspace's own lift holds no wider signed
    /// integer, and `1 << 63` of a `usize` enum is past `i64`'s range.
    pub(crate) values: Option<Vec<i64>>,
    /// The representations its `#[repr(..)]` attributes name (`transparent`,
    /// `u8`, `C`), as written.
    pub(crate) repr: Vec<String>,
}

/// A discriminant `CoreStruct::values` holds, read at an integer type of
/// the given signedness.
pub(crate) fn discriminant(held: i64, signed: bool) -> i128 {
    if signed {
        i128::from(held)
    } else {
        i128::from(held.cast_unsigned())
    }
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
    /// The unions core declares (`MaybeUninit`), by path: a path a `use`
    /// names one by resolves to it, though no record is made of it here.
    unions: std::collections::BTreeSet<String>,
    /// Each type alias core declares, by path: the module it is written
    /// in, its generic parameters in order and the type it names.
    aliases: BTreeMap<String, (String, Vec<String>, String)>,
    /// The generic parameters in scope in each method of an impl, its
    /// impl's and its own, by the method's path from core's root through
    /// its type's name (several impls of one type's methods of one name
    /// together).
    pub(crate) methods: BTreeMap<String, std::collections::BTreeSet<String>>,
    /// The type of each const parameter in scope in a function or of a
    /// struct, by its path, as [`CoreDecls::parameters`] finds them.
    consts: BTreeMap<String, BTreeMap<String, String>>,
    /// Each associated type an impl of a trait sets, by the type it is for
    /// (as written), the trait's name and the associated type's: the
    /// module the impl is written in and the type it sets.
    assoc: BTreeMap<(String, String, String), (String, String)>,
    /// Each constant item's type and expression, as written, by its path
    /// from core's root: rustc dumps the MIR of only some (`pub` ones), so
    /// a use of another reads its value from the source.
    pub(crate) items: BTreeMap<String, (String, String)>,
    /// Each type parameter a function's generics bound by `Fn` or `FnOnce`,
    /// by the function's path as [`CoreDecls::parameters`] takes it: the
    /// bound as a type, `impl FnOnce(A) -> R`.
    pub(crate) fn_bounds: BTreeMap<String, BTreeMap<String, String>>,
    /// Each struct's or enum's type parameters' defaults, in order, by its
    /// name (`ControlFlow`: `[None, Some("()")]`): MIR prints a type
    /// without the trailing arguments its defaults give. A name two
    /// declarations give other defaults has none.
    pub(crate) defaults: BTreeMap<String, Option<Vec<Option<String>>>>,
    /// Each trait's type parameters, in order, by its path: a call of its
    /// default method, `<X as Trait<A>>::m`, binds them.
    pub(crate) trait_params: BTreeMap<String, Vec<String>>,
}

impl CoreDecls {
    /// The type core's source gives the const parameter `name` in scope in
    /// the function at `path`: its own, its impl's or its type's.
    pub(crate) fn const_type(&self, path: &str, name: &str) -> Option<String> {
        let parent = path.rsplit_once("::").map(|(t, _)| t);
        [Some(path), parent]
            .into_iter()
            .flatten()
            .find_map(|p| self.consts.get(p)?.get(name).cloned())
    }

    /// The generic parameters in scope in the function at `path` from
    /// core's root: a free function's own, or a method's, its impl's and
    /// those of the type it is a method of.
    pub(crate) fn parameters(&self, path: &str) -> std::collections::BTreeSet<String> {
        let mut out: std::collections::BTreeSet<String> = self
            .generics
            .get(path)
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        out.extend(self.methods.get(path).into_iter().flatten().cloned());
        if let Some((ty, _)) = path.rsplit_once("::") {
            for st in self.structs.iter().filter(|s| s.path == ty) {
                out.extend(st.params.iter().cloned());
            }
        }
        out
    }
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

/// The const parameters of each impl the dumped path `path` names by its
/// span (`<impl at file:l:c: l:c>`), with their types, read from the impl's
/// header in its source.
fn impl_const_types(root: &Path, path: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = path;
    while let Some(at) = rest.find("<impl at ") {
        let after = &rest[at + "<impl at ".len()..];
        let Some(end) = after.find('>') else {
            break;
        };
        let header = super::impl_header(root, &after[..end]);
        if let Some(im) = header.and_then(|h| {
            syn::parse_str::<syn::ItemImpl>(&format!("{} {{}}", without_constness(&h))).ok()
        }) {
            out.extend(const_types(&im.generics));
        }
        rest = &after[end + 1..];
    }
    out
}

/// An impl's header with what syn does not read left out: the `const`
/// of a const trait impl (`impl<T, const N: usize> const TryFrom<&[T]>
/// for [T; N]`) and its where clause, whose bounds may be `[const]`. Its
/// generics, which are what is read of it, stay as written.
fn without_constness(header: &str) -> String {
    let head = header.split(" where ").next().unwrap_or(header).trim_end();
    let Some(rest) = head.trim_start().strip_prefix("impl") else {
        return head.to_string();
    };
    // The generics' closing `>`, at depth zero.
    let mut depth = 0usize;
    let mut cut = 0;
    if rest.starts_with('<') {
        let chars: Vec<char> = rest.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            match c {
                '<' => depth += 1,
                '>' if i > 0 && chars[i - 1] != '-' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        cut = chars[..=i].iter().map(|c| c.len_utf8()).sum();
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let (generics, after) = rest.split_at(cut);
    let after = after.trim_start();
    let after = after.strip_prefix("const ").unwrap_or(after);
    format!("impl{generics} {after}")
}

/// An impl's type as its source writes it, without spaces: `[T]`.
fn self_type_text(ty: &syn::Type) -> String {
    quote::quote!(#ty).to_string().replace(' ', "")
}

/// The dumped path `path` with each impl it names by its span written as
/// the impl's type, as [`self_type_text`] writes it: `slice::[T]::as_array`.
fn written_impls(root: &Path, path: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(at) = rest.find("<impl at ") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "<impl at ".len()..];
        let end = after.find('>')?;
        let header = super::impl_header(root, &after[..end])?;
        let im = syn::parse_str::<syn::ItemImpl>(&format!("{header} {{}}")).ok()?;
        out.push_str(&self_type_text(&im.self_ty));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// A signature's const parameters, each with its type as written.
fn const_types(g: &syn::Generics) -> BTreeMap<String, String> {
    g.const_params()
        .map(|c| {
            let ty = &c.ty;
            (
                c.ident.to_string(),
                quote::quote!(#ty).to_string().replace(' ', ""),
            )
        })
        .collect()
}

/// A function's generic parameters in the order MIR prints its type
/// arguments: those its signature declares, then one for each argument
/// written `impl Trait`, which rustc adds after them.
fn signature_generic_names(sig: &syn::Signature) -> Vec<String> {
    let mut out = generic_names(&sig.generics);
    for input in &sig.inputs {
        if let syn::FnArg::Typed(t) = input {
            let ty = &t.ty;
            let text = quote::quote!(#ty).to_string();
            if let Some(at) = text.find("impl ") {
                out.push(
                    text[at..]
                        .trim_end_matches([')', ']', '>', ' '])
                        .to_string(),
                );
            }
        }
    }
    out
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
        let Ok(entries) = crate::embed::dir_paths(dir) else {
            return;
        };
        for p in entries {
            if p.is_dir() {
                walk(&p, files);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    fn items(module: &[String], list: &[syn::Item], out: &mut CoreDecls) {
        // The representations the `#[repr(..)]` attributes of an item name:
        // each name at the list's top level (`repr(C, align(8))`), `align`'s
        // argument not being one.
        let reprs = |attrs: &[syn::Attribute]| {
            let mut out = Vec::new();
            for a in attrs {
                if let syn::Meta::List(l) = &a.meta {
                    if l.path.is_ident("repr") {
                        for tree in l.tokens.clone() {
                            if let proc_macro2::TokenTree::Ident(i) = tree {
                                out.push(i.to_string());
                            }
                        }
                    }
                }
            }
            out
        };
        let at = |name: String| {
            let mut path = module.to_vec();
            path.push(name);
            path.join("::")
        };
        // A type as MIR prints it: a path's segments joined without spaces,
        // a pattern type as `(T) is P`.
        let text = |ty: &syn::Type| {
            let t = quote::quote!(#ty)
                .to_string()
                .replace(" :: ", "::")
                .replace(" < ", "<")
                .replace(" >", ">")
                .replace("& ", "&")
                .replace(" ,", ",")
                .replace(" ;", ";");
            match t
                .strip_prefix("pattern_type ! (")
                .and_then(|r| r.strip_suffix(')'))
            {
                Some(inner) => match inner.split_once(" is ") {
                    Some((base, pat)) => format!("({base}) is {}", pat.replace(' ', "")),
                    None => t,
                },
                None => t,
            }
        };
        // Each type parameter bounded by `Fn`, `FnMut` or `FnOnce` of the
        // generics in scope, and the bound, `impl FnOnce(A) -> R`: the
        // parameter is a function, its one method the function itself. A
        // `FnMut` one is bound to `impl Fn(A) -> R`, the functions among
        // its types (every `Fn` is a `FnMut`, whose `call_mut` changes no
        // environment), which its row's instance states.
        let fn_bounds = |scopes: &[&syn::Generics]| -> BTreeMap<String, String> {
            let mut found = BTreeMap::new();
            let mut bound = |p: String, b: &syn::TypeParamBound| {
                let syn::TypeParamBound::Trait(tb) = b else {
                    return;
                };
                let Some(last) = tb.path.segments.last() else {
                    return;
                };
                let name = last.ident.to_string();
                let syn::PathArguments::Parenthesized(args) = &last.arguments else {
                    return;
                };
                let name = match name.as_str() {
                    "Fn" | "FnMut" => "Fn",
                    "FnOnce" => "FnOnce",
                    _ => return,
                };
                let inputs: Vec<String> = args.inputs.iter().map(|a| text(&a.ty)).collect();
                let output = match &args.output {
                    syn::ReturnType::Default => "()".to_string(),
                    syn::ReturnType::Type(_, t) => text(t),
                };
                found.insert(p, format!("impl {name}({}) -> {output}", inputs.join(", ")));
            };
            for g in scopes {
                for param in &g.params {
                    if let syn::GenericParam::Type(t) = param {
                        for b in &t.bounds {
                            bound(t.ident.to_string(), b);
                        }
                    }
                }
                for pred in g.where_clause.iter().flat_map(|w| &w.predicates) {
                    if let syn::WherePredicate::Type(pt) = pred {
                        let p = text(&pt.bounded_ty);
                        for b in &pt.bounds {
                            bound(p.clone(), b);
                        }
                    }
                }
            }
            found
        };
        for item in list {
            match item {
                syn::Item::Const(c) => {
                    let e = &c.expr;
                    out.items.insert(
                        at(c.ident.to_string()),
                        (text(&c.ty), quote::quote!(#e).to_string()),
                    );
                }
                syn::Item::Struct(st) => {
                    let ds: Vec<Option<String>> = st
                        .generics
                        .type_params()
                        .map(|t| t.default.as_ref().map(|(_, d)| text(d)))
                        .collect();
                    if ds.iter().any(Option::is_some) {
                        let entry = out.defaults.entry(st.ident.to_string());
                        let was = entry.or_insert_with(|| Some(ds.clone()));
                        if was.as_ref() != Some(&ds) {
                            *was = None;
                        }
                    }
                    let params = generic_names(&st.generics);
                    out.consts
                        .entry(at(st.ident.to_string()))
                        .or_default()
                        .extend(const_types(&st.generics));
                    let fields = st
                        .fields
                        .iter()
                        .map(|f| (f.ident.as_ref().map(ToString::to_string), text(&f.ty)))
                        .collect();
                    out.structs.push(CoreStruct {
                        path: at(st.ident.to_string()),
                        params,
                        fields,
                        variants: None,
                        data: None,
                        explicit: false,
                        values: None,
                        repr: reprs(&st.attrs),
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
                        data: None,
                        explicit: e.variants.iter().any(|v| v.discriminant.is_some()),
                        // Each variant's discriminant, as Rust assigns them:
                        // the literal it sets, or one past the previous.
                        values: {
                            let mut next = Some(0i128);
                            let mut out = Vec::new();
                            for v in &e.variants {
                                let set = v.discriminant.as_ref().map(|(_, x)| {
                                    let text = quote::quote!(#x).to_string().replace(' ', "");
                                    let (neg, digits) = match text.strip_prefix('-') {
                                        Some(d) => (true, d.to_string()),
                                        None => (false, text),
                                    };
                                    let digits = digits.replace('_', "");
                                    // `1 << k`, as an enum of powers sets.
                                    let shifted = digits.split_once("<<").and_then(|(a, b)| {
                                        let (a, b) =
                                            (a.parse::<i128>().ok()?, b.parse::<u32>().ok()?);
                                        a.checked_shl(b).filter(|v| v >> b == a)
                                    });
                                    let n = if shifted.is_some() {
                                        shifted
                                    } else if let Some(h) = digits.strip_prefix("0x") {
                                        i128::from_str_radix(h, 16).ok()
                                    } else if let Some(b) = digits.strip_prefix("0b") {
                                        i128::from_str_radix(b, 2).ok()
                                    } else if let Some(o) = digits.strip_prefix("0o") {
                                        i128::from_str_radix(o, 8).ok()
                                    } else {
                                        digits.parse::<i128>().ok()
                                    };
                                    n.map(|n| if neg { -n } else { n })
                                });
                                let value = match set {
                                    Some(n) => n,
                                    None => next,
                                };
                                out.push(value);
                                next = value.and_then(|n| n.checked_add(1));
                            }
                            out.into_iter()
                                .map(|v| {
                                    v.and_then(|n| {
                                        i64::try_from(n)
                                            .ok()
                                            .or_else(|| u64::try_from(n).ok().map(u64::cast_signed))
                                    })
                                })
                                .collect::<Option<Vec<i64>>>()
                        },
                        repr: reprs(&e.attrs),
                    });
                }
                syn::Item::Trait(t) => {
                    out.traits.insert(at(t.ident.to_string()));
                    out.trait_params
                        .insert(at(t.ident.to_string()), generic_names(&t.generics));
                }
                syn::Item::Union(u) => {
                    out.unions.insert(at(u.ident.to_string()));
                }
                syn::Item::Type(t) => {
                    out.aliases.insert(
                        at(t.ident.to_string()),
                        (module.join("::"), generic_names(&t.generics), text(&t.ty)),
                    );
                }
                // An enum whose variants hold data, over its parameters.
                syn::Item::Enum(e) => {
                    let ds: Vec<Option<String>> = e
                        .generics
                        .type_params()
                        .map(|t| t.default.as_ref().map(|(_, d)| text(d)))
                        .collect();
                    if ds.iter().any(Option::is_some) {
                        let entry = out.defaults.entry(e.ident.to_string());
                        let was = entry.or_insert_with(|| Some(ds.clone()));
                        if was.as_ref() != Some(&ds) {
                            *was = None;
                        }
                    }
                    let data = e
                        .variants
                        .iter()
                        .map(|v| {
                            let fields = v
                                .fields
                                .iter()
                                .map(|f| (f.ident.as_ref().map(ToString::to_string), text(&f.ty)))
                                .collect();
                            (v.ident.to_string(), fields)
                        })
                        .collect();
                    out.structs.push(CoreStruct {
                        path: at(e.ident.to_string()),
                        params: generic_names(&e.generics),
                        fields: Vec::new(),
                        variants: None,
                        data: Some(data),
                        explicit: e.variants.iter().any(|v| v.discriminant.is_some()),
                        values: None,
                        repr: reprs(&e.attrs),
                    });
                }
                // An impl: its methods' generic parameters, and the
                // associated types an impl of a trait sets.
                syn::Item::Impl(im) => {
                    let ty = match &*im.self_ty {
                        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
                        _ => None,
                    };
                    // A method's const parameters, by its impl's type as
                    // written where no path names it (`[T]::as_array`).
                    let written = self_type_text(&im.self_ty);
                    for it in &im.items {
                        if let syn::ImplItem::Fn(f) = it {
                            let consts = out
                                .consts
                                .entry(at(format!(
                                    "{}::{}",
                                    ty.as_deref().unwrap_or(&written),
                                    f.sig.ident
                                )))
                                .or_default();
                            consts.extend(const_types(&im.generics));
                            consts.extend(const_types(&f.sig.generics));
                        }
                        if let (syn::ImplItem::Fn(f), Some(ty)) = (it, &ty) {
                            let path = at(format!("{ty}::{}", f.sig.ident));
                            // An inherent impl's method, by the impl's type:
                            // its own generic parameters, which a call's
                            // turbofish binds.
                            if im.trait_.is_none() {
                                out.generics
                                    .entry(path.clone())
                                    .or_insert_with(|| signature_generic_names(&f.sig));
                            }
                            let fns = fn_bounds(&[&im.generics, &f.sig.generics]);
                            if !fns.is_empty() {
                                out.fn_bounds.entry(path.clone()).or_default().extend(fns);
                            }
                            out.methods.entry(path).or_default().extend(
                                generic_names(&im.generics)
                                    .into_iter()
                                    .chain(generic_names(&f.sig.generics)),
                            );
                        }
                    }
                    let Some((tr, _)) = &im.trait_ else {
                        continue;
                    };
                    let Some(tname) = tr.segments.last().map(|s| s.ident.to_string()) else {
                        continue;
                    };
                    for it in &im.items {
                        if let syn::ImplItem::Type(t) = it {
                            out.assoc.insert(
                                (text(&im.self_ty), tname.clone(), t.ident.to_string()),
                                (module.join("::"), text(&t.ty)),
                            );
                        }
                    }
                }
                syn::Item::Fn(f) => {
                    out.consts
                        .entry(at(f.sig.ident.to_string()))
                        .or_default()
                        .extend(const_types(&f.sig.generics));
                    let fns = fn_bounds(&[&f.sig.generics]);
                    if !fns.is_empty() {
                        out.fn_bounds.insert(at(f.sig.ident.to_string()), fns);
                    }
                    out.generics
                        .insert(at(f.sig.ident.to_string()), signature_generic_names(&f.sig));
                }
                // A function without a body, as core declares an intrinsic.
                syn::Item::Verbatim(tokens) => {
                    if let Ok(f) = syn::parse2::<syn::ForeignItemFn>(tokens.clone()) {
                        let path = at(f.sig.ident.to_string());
                        if f.attrs.iter().any(|a| a.path().is_ident("rustc_intrinsic")) {
                            out.intrinsics.insert(path.clone());
                        }
                        out.consts
                            .entry(path.clone())
                            .or_default()
                            .extend(const_types(&f.sig.generics));
                        out.generics.insert(path, signature_generic_names(&f.sig));
                    }
                    // A `const trait`, which syn reads as tokens: the trait
                    // with `const` left out.
                    let ts: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
                    let word = |t: &proc_macro2::TokenTree, s: &str| matches!(t, proc_macro2::TokenTree::Ident(i) if i == s);
                    let at_const = ts
                        .windows(2)
                        .position(|w| word(&w[0], "const") && word(&w[1], "trait"));
                    if let Some(c) = at_const {
                        let rest: proc_macro2::TokenStream = ts
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i != c)
                            .map(|(_, t)| t.clone())
                            .collect();
                        if let Ok(t) = syn::parse2::<syn::ItemTrait>(rest) {
                            out.traits.insert(at(t.ident.to_string()));
                            out.trait_params
                                .insert(at(t.ident.to_string()), generic_names(&t.generics));
                        }
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
    // Every file first, for the macros each declares; each file's items
    // are then read with its invocations of them expanded.
    let mut read: Vec<(Vec<String>, Vec<syn::Item>)> = Vec::new();
    let mut macros = crate::expand::Macros::default();
    for file in files {
        let Ok(text) = crate::embed::read_text(&file) else {
            continue;
        };
        let Ok(parsed) = syn::parse_file(&text).or_else(|_| {
            let tokens: proc_macro2::TokenStream = text.parse()?;
            syn::parse2::<syn::File>(without_const_traits(tokens))
        }) else {
            continue;
        };
        macros.collect(&parsed.items);
        let rel = file.strip_prefix(&src).unwrap_or(&file).with_extension("");
        let mut module: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if matches!(module.last().map(String::as_str), Some("mod" | "lib")) {
            module.pop();
        }
        read.push((module, parsed.items));
    }
    let reread = |tokens: proc_macro2::TokenStream| {
        syn::parse2::<syn::File>(tokens.clone())
            .or_else(|_| syn::parse2::<syn::File>(without_const_traits(tokens)))
            .ok()
            .map(|f| f.items)
    };
    for (module, parsed) in read {
        items(&module, &macros.expand(parsed, &reread), &mut out);
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
        if self.by.contains_key(path)
            || self.decls.traits.contains(path)
            || self.decls.unions.contains(path)
            || self.decls.aliases.contains_key(path)
        {
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
        // `crate::`, `self::` and `super::` as Rust reads them.
        let mut at: Vec<&str> = module.split("::").filter(|s| !s.is_empty()).collect();
        let mut rest_path = path;
        let mut relative = false;
        loop {
            if let Some(r) = rest_path.strip_prefix("crate::") {
                at.clear();
                rest_path = r;
            } else if let Some(r) = rest_path.strip_prefix("self::") {
                rest_path = r;
            } else if let Some(r) = rest_path.strip_prefix("super::") {
                at.pop();
                rest_path = r;
            } else {
                break;
            }
            relative = true;
        }
        if relative {
            return self.resolve(&at.join("::"), rest_path);
        }
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
    /// declared type written from core's root, and each type alias written
    /// as the type it names at its arguments (`depth` aliases deep).
    fn rooted(&self, module: &str, text: &str, depth: usize) -> String {
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
                let path = self.resolve(module, &word).unwrap_or(word);
                let Some((at, params, target)) =
                    self.decls.aliases.get(&path).filter(|_| depth < 16)
                else {
                    out.push_str(&path);
                    continue;
                };
                // The alias's arguments, each written from core's root.
                let mut args = Vec::new();
                if chars.get(i) == Some(&'<') {
                    let mut level = 0usize;
                    let open = i;
                    while i < chars.len() {
                        match chars[i] {
                            '<' => level += 1,
                            '>' if chars[i - 1] != '-' => {
                                level -= 1;
                                if level == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    let inner: String = chars[open + 1..i.min(chars.len())].iter().collect();
                    i += 1;
                    args = parse::split_top(&inner, ',')
                        .into_iter()
                        .map(str::trim)
                        .filter(|a| !a.is_empty() && !a.starts_with('\''))
                        .map(|a| self.rooted(module, a, depth + 1))
                        .collect();
                }
                if args.len() != params.len() {
                    out.push_str(&path);
                    continue;
                }
                let binds: BTreeMap<String, String> = params.iter().cloned().zip(args).collect();
                let named = super::stdlib::substitute(target, &binds);
                out.push_str(&self.rooted(at, &named, depth + 1));
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
        // A field's type at the instance's arguments, from core's root: an
        // associated type of an argument (`T::NonZeroInner`) is the type
        // the one impl that sets it for that argument sets, read in the
        // impl's module.
        let field = |cx: &mut Self, t: &str, types: &mut Types| -> Option<lift::Ty> {
            // `T::Assoc`, which substitution leaves as a path, with `T`
            // bound first.
            let t = match t.split_once("::") {
                Some((p, assoc)) if binds.contains_key(p) && !assoc.contains("::") => {
                    format!("{}::{assoc}", binds[p])
                }
                _ => super::stdlib::substitute(t, &binds),
            };
            let projected = t
                .strip_prefix('<')
                .and_then(|r| r.split_once(" as "))
                .and_then(|(base, rest)| {
                    Some((base.to_string(), rest.rsplit_once(">::")?.1.to_string()))
                })
                .or_else(|| {
                    t.rsplit_once("::")
                        .map(|(b, a)| (b.to_string(), a.to_string()))
                });
            let set: Vec<String> = projected
                .map(|(base, assoc)| {
                    cx.decls
                        .assoc
                        .iter()
                        // A generic associated type is set by its name:
                        // `P::Searcher<'a>` is the impl's `type Searcher<'a>`.
                        .filter(|((ty, _, a), _)| {
                            *ty == base && a.as_str() == assoc.split('<').next().unwrap_or(&assoc)
                        })
                        .map(|(_, (m, to))| cx.rooted(m, to, 0))
                        .collect()
                })
                .unwrap_or_default();
            let t = match set.as_slice() {
                [one] => one.clone(),
                _ => cx.rooted(module, &t, 0),
            };
            cx.mention(&t, types);
            super::lower::ty(&t, types).ok()
        };
        parts.extend(args.iter().copied());
        let name = crate::lex::pascal(&parts);
        let text = if args.is_empty() {
            path.to_string()
        } else {
            format!("{path}<{}>", args.join(", "))
        };
        // An enum the lift has its own type for, `Option` or `Result`,
        // keeps that type.
        if st.data.is_some() && super::lower::ty(&text, types).is_ok() {
            return None;
        }
        if let Some(data) = &st.data {
            let mut variants = Vec::new();
            for (v, fs) in data {
                let mut lifted = Vec::new();
                for (f, t) in fs {
                    lifted.push((f.clone(), field(self, t, types)?));
                }
                variants.push((v.clone(), lifted));
            }
            if st.explicit {
                self.explicit.insert(name.clone());
            }
            types.datas.insert(name.clone(), variants);
            types.aliases.insert(text, name.clone());
            self.done.insert(key, Some(name.clone()));
            return Some(name);
        }
        let mut fields = Vec::new();
        let mut lent = Vec::new();
        for (i, (fname, t)) in st.fields.iter().enumerate() {
            let ft = field(self, t, types)?;
            if super::lower::mut_target(t).is_some() {
                lent.push((
                    i,
                    self.rooted(module, &super::stdlib::substitute(t, &binds), 0),
                ));
            }
            fields.push((
                fname
                    .as_deref()
                    .map_or_else(|| format!("item{i}"), lift::local_name),
                ft,
            ));
        }
        // A struct of one pattern-typed field holds the values its pattern
        // admits.
        if let [(_, raw)] = st.fields.as_slice() {
            if let Some((base, pat)) = raw.strip_prefix('(').and_then(|r| r.split_once(") is ")) {
                types
                    .valid
                    .insert(name.clone(), (base.to_string(), pat.to_string()));
            }
        }
        types.structs.insert(name.clone(), fields);
        if !lent.is_empty() {
            types.borrowing.insert(name.clone(), lent);
        }
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
                types
                    .aliases
                    .entry(super::lower::without_lifetimes(&c))
                    .or_insert_with(|| n.clone());
                types.aliases.entry(c.clone()).or_insert(n);
            }
        }
    }
}

/// Each instance of a core struct the bodies' (and closures') locals mention, as MIR prints
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
    link: &super::link::CoreLink,
    bodies: &[(IrBody, String, String)],
    closures: &BTreeMap<String, IrBody>,
) -> std::collections::BTreeSet<String> {
    let decls = &link.decls;
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
    for body in closures.values() {
        mentioned.extend(body.locals.iter().map(String::as_str));
    }
    // And those of every body of core's, which a call into another
    // module's body lowers with.
    mentioned.extend(link.locals.iter().map(String::as_str));
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
    // Each associated type an impl sets to a struct of core's, under the
    // projection that names it, `<X as Trait>::Assoc` (see
    // `lower::ty`): an instance's locals name `<char as
    // str::pattern::Pattern>::Searcher<'_>`, `CharSearcher`.
    for ((x, tr, a), (m, to)) in decls.assoc.clone() {
        let target = cx.rooted(&m, &to, 0);
        cx.mention(&target, types);
        if let Some(n) = types.aliases.get(&target).cloned() {
            types
                .aliases
                .entry(format!("<{x} as {tr}>::{a}"))
                .or_insert(n);
        }
    }
    cx.explicit
}

/// The type parameters of a dumped body, each bound to [`CANONICAL`]: the
/// bare capitalized names its locals' types mention (rustc prints those
/// types by their paths, so a bare name is a parameter), `Self`, and those
/// of its signature's types that `declared` holds (the signature prints a
/// type by its bare name too). A const parameter is bound to `#`: a bare
/// name in an array's length, or one a type mentions that the body also
/// reads as a value, `const N` (a constant item is never a type's bare
/// name).
pub(crate) fn canonical_bindings(
    text: &str,
    declared: &dyn Fn(&str) -> bool,
    const_type: &dyn Fn(&str) -> Option<String>,
) -> BTreeMap<String, String> {
    // Each bare name in the types, with whether one mention is a length.
    let mut names: BTreeMap<String, bool> = BTreeMap::new();
    for (signature, ty) in signature_and_local_types(text) {
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
                if bare && !segment && !named && (!signature || declared(&word)) {
                    let length = before.trim_end().ends_with(';');
                    *names.entry(word).or_default() |= length;
                }
            } else {
                i += 1;
            }
        }
    }
    // The names the body reads as values: `const N`, not `*const T`.
    let mut values = std::collections::BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("const ") {
        let pointer = rest[..at].ends_with('*');
        let after = &rest[at + "const ".len()..];
        let word: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let path = after[word.len()..].starts_with([':', '<', '(']);
        if !pointer && !path && !word.is_empty() {
            values.insert(word);
        }
        rest = after;
    }
    names
        .into_iter()
        .map(|(word, length)| {
            let bound = if length || values.contains(&word) {
                const_marker(&word, const_type(&word).as_deref())
            } else {
                CANONICAL.to_string()
            };
            (word, bound)
        })
        .collect()
}

/// `line` with each associated type of a type parameter, `<I as
/// Iterator>::Item` (which rustc leaves unnormalized only where the
/// body's bounds do not fix it), written as the parameter itself: within
/// the body it is a type of its own about which only its bounds say
/// anything, as the parameter is, so the census instantiates it at the
/// parameter's binding too. Nested projections fold from the inside.
fn opaque_projections(line: &str, params: &BTreeMap<String, String>) -> String {
    let mut out = line.to_string();
    'again: loop {
        let mut from = 0;
        while let Some(at) = out[from..].find('<').map(|i| i + from) {
            from = at + 1;
            let rest = &out[at + 1..];
            let word: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !params.contains_key(&word) || !rest[word.len()..].starts_with(" as ") {
                continue;
            }
            // The `>` closing this `<`, then `::Name`.
            let mut depth = 0i64;
            let mut close = None;
            let mut prev = ' ';
            for (i, c) in out[at..].char_indices() {
                match c {
                    '<' => depth += 1,
                    '>' if prev != '-' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(at + i);
                            break;
                        }
                    }
                    _ => {}
                }
                prev = c;
            }
            let Some(close) = close else {
                continue;
            };
            let Some(after) = out[close + 1..].strip_prefix("::") else {
                continue;
            };
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            // An associated type, `Item`: not a method, `next(..)`, nor
            // a constant, `MAX`.
            let ty = name.starts_with(|c: char| c.is_ascii_uppercase())
                && name.chars().any(|c| c.is_ascii_lowercase());
            let next = after[name.len()..].chars().next();
            if !ty || matches!(next, Some('(' | ':')) {
                continue;
            }
            // A pointee's `Metadata` is fixed by core for every type (the
            // unit for a sized one, a slice's length), never opaque: the
            // lowering reads it (see `lower::ty`).
            if name == "Metadata" && out[at..close].ends_with("Pointee") {
                continue;
            }
            let end = close + 1 + "::".len() + name.len();
            out.replace_range(at..end, &word);
            continue 'again;
        }
        return out;
    }
}

/// The census's binding of the const parameter `name` of type `ty`:
/// `#usize_N`, which no body takes as a type and a lowered body reads as
/// a parameter of that type (`#_N` where the source gives no type).
pub(crate) fn const_marker(name: &str, ty: Option<&str>) -> String {
    format!("#{}_{name}", ty.unwrap_or(""))
}

/// The const parameter a marker [`const_marker`] wrote stands for: its
/// type, where known, and its name.
pub(crate) fn const_of_marker(marker: &str) -> Option<(Option<&str>, &str)> {
    let (ty, name) = marker.strip_prefix('#')?.split_once('_')?;
    Some(((!ty.is_empty()).then_some(ty), name))
}

/// The texts of a dumped body's types, each with whether its `fn` line
/// writes it: its parameters' and its return type's, and each local's.
fn signature_and_local_types(text: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("let ") {
            if let Some((_, ty)) = rest.split_once(": ") {
                out.push((false, ty.trim_end_matches(';')));
            }
            continue;
        }
        if !line.starts_with("fn ") {
            continue;
        }
        let line = line.trim_end_matches('{').trim_end();
        let (head, ret) = line.rsplit_once(" -> ").unwrap_or((line, ""));
        if !ret.is_empty() {
            out.push((true, ret));
        }
        let Some(open) = head.find("(_") else {
            continue;
        };
        let Some(params) = head[open + 1..].strip_suffix(')') else {
            continue;
        };
        for p in parse::split_top(params, ',') {
            if let Some((_, ty)) = p.split_once(": ") {
                out.push((true, ty.trim()));
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
            if let Some(e) = &r.embedded {
                let _ = writeln!(rows, "embedded = \"{e}\"");
            }
            if !r.blocked.is_empty() {
                let quoted: Vec<String> = r.blocked.iter().map(|b| toml_quote(b)).collect();
                let _ = writeln!(rows, "blocked = [{}]", quoted.join(", "));
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
    let files: Vec<PathBuf> = crate::embed::dir_paths(dir)?
        .into_iter()
        .filter(|p| {
            p.extension().is_some_and(|x| x == "toml")
                && p.file_stem()
                    .is_some_and(|n| n != "frontier" && n != crate::queue::RETRACTED)
        })
        .collect();
    let mut modules = String::new();
    let mut causes: BTreeMap<String, usize> = BTreeMap::new();
    let (mut all, mut done) = (0, 0);
    // The embedding's census: the bodies embedded, those no construct
    // blocks, and the bodies each construct the evaluator does not define
    // blocks.
    let (mut embedded, mut evaluable) = (0, 0);
    let mut blocking: BTreeMap<String, usize> = BTreeMap::new();
    for file in files {
        let text = crate::embed::read_text(&file)?;
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
            if !field("embedded").is_empty() {
                embedded += 1;
                let blocked = row
                    .get("blocked")
                    .and_then(toml::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if blocked.is_empty() {
                    evaluable += 1;
                }
                for b in blocked.iter().filter_map(toml::Value::as_str) {
                    *blocking.entry(b.to_string()).or_default() += 1;
                }
            }
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
    // The embedding's count and its queue: every construct the evaluator
    // does not define, by the bodies it blocks.
    if embedded > 0 {
        let _ = writeln!(
            summary,
            "[embedding]\nembedded = {embedded}\nevaluable = {evaluable}\n"
        );
        let mut queue: Vec<(String, usize)> = blocking.into_iter().collect();
        queue.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (construct, count) in queue {
            let _ = writeln!(
                summary,
                "[[construct]]\nconstruct = {}\nbodies = {count}\n",
                toml_quote(&construct)
            );
        }
    }
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
/// leave as residue, by κ: the register is monotone but for a body its
/// `retracted.toml` lists by κ and path.
pub fn core_regressions(dir: &Path, shards: &[CoreShard]) -> Vec<String> {
    let mut out = Vec::new();
    let retracted = match crate::queue::retractions(dir) {
        Ok(r) => r,
        Err(e) => return vec![e.to_string()],
    };
    for s in shards {
        let file = dir.join(format!("{}.toml", s.module));
        let Ok(text) = crate::embed::read_text(&file) else {
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
                let listed = retracted
                    .iter()
                    .any(|x| x.kappa == r.kappa && x.path == field("path"));
                if r.digest.is_none() && !listed {
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
        let p = canonical_bindings(text, &|_| false, &|_| None);
        // The signature's own names, and a const parameter the body reads.
        let header = "fn g(_1: V, _2: *const W, _3: &Layout) -> IntoIter<V, M> {\n    let mut _0: IntoIter<V, M>;\n    _3 = Lt(copy _2, const M);\n    _4 = const MAX;\n}\n";
        let q = canonical_bindings(header, &|w| ["V", "W", "M"].contains(&w), &|w| {
            (w == "M").then(|| "usize".to_string())
        });
        assert!(!q.contains_key("Layout"));
        assert_eq!(q.get("V").map(String::as_str), Some(CANONICAL));
        assert_eq!(q.get("W").map(String::as_str), Some(CANONICAL));
        assert_eq!(q.get("M").map(String::as_str), Some("#usize_M"));
        assert!(!q.contains_key("MAX") && !q.contains_key("IntoIter"));
        assert_eq!(p.get("T").map(String::as_str), Some(CANONICAL));
        assert_eq!(p.get("U").map(String::as_str), Some(CANONICAL));
        assert_eq!(p.get("F").map(String::as_str), Some(CANONICAL));
        assert_eq!(p.get("N").map(String::as_str), Some("#_N"));
        assert!(!p.contains_key("Deref") && !p.contains_key("Target"));
    }

    #[test]
    fn a_const_trait_impls_const_parameters_have_their_types() {
        let header = "impl<T, U, const N: usize> const PartialEq<[U; N]> for &mut [T] where T: [const] PartialEq<U>, ";
        let im: syn::ItemImpl =
            syn::parse_str(&format!("{} {{}}", without_constness(header))).unwrap();
        assert_eq!(
            const_types(&im.generics).get("N").map(String::as_str),
            Some("usize")
        );
    }

    #[test]
    fn a_parameters_associated_types_are_the_parameter() {
        let p: BTreeMap<String, String> = [("I".to_string(), CANONICAL.to_string())].into();
        assert_eq!(
            opaque_projections("let _3: Option<<I as iter::Iterator>::Item>;", &p),
            "let _3: Option<I>;"
        );
        assert_eq!(
            opaque_projections(
                "let _4: <<I as Try>::Residual as Residual<u8>>::TryType;",
                &p
            ),
            "let _4: I;"
        );
        // A method or a constant of the parameter's trait stays.
        let call = "_5 = <I as Iterator>::next(move _6) -> [return: bb1, unwind continue];";
        assert_eq!(opaque_projections(call, &p), call);
        assert_eq!(
            opaque_projections("_7 = const <I as Bits>::MAX;", &p),
            "_7 = const <I as Bits>::MAX;"
        );
        // Another type's associated type stays.
        let other = "let _8: <u8 as Iterator>::Item;";
        assert_eq!(opaque_projections(other, &p), other);
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
