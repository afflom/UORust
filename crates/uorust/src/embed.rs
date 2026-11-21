//! The deep embedding of MIR in LexLean (UE-19): every body a dump holds,
//! written as a value of the MIR datatype that `assets/embed/src/Mir.lex.tex`
//! declares, beside the one evaluator that module defines over it. The
//! embedding never refuses a body: what the parser reads structurally is
//! written as its construct, and what it keeps as text is written as the
//! `other` node of its kind, which the evaluator answers with
//! `Outcome.undefined` naming the kind. Types, generic arguments and calls
//! are data: a call is the body the linker resolves it to, or, where a
//! trait bound leaves the callee abstract, a name the caller's dictionary
//! (`Program.dictionary`) binds.
//!
//! Coverage is measured per construct, not per body. The constructs the
//! evaluator does not define are read from its own source: each is a
//! string it answers `Outcome.undefined` with, so the source stays the one
//! authority (R1). A body is blocked by every such construct it uses, and
//! the census ranks them by how many bodies they block: a finite queue,
//! since MIR's kinds are finite (`crate::mir::spec`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::mir::ir::{
    IrAggregate, IrBody, IrConstant, IrOperand, IrPlace, IrProjection, IrRvalue, IrStatement,
    IrTerminator,
};
use crate::mir::spec;
use crate::UorustError;

/// The MIR datatype and its evaluator.
pub(crate) const SEMANTICS: &str = include_str!("../assets/embed/src/Mir.lex.tex");

/// The evaluator's lemmas: its comparisons on bit patterns, which the
/// bridge's reductions meet, proved the comparisons of the values the
/// patterns encode. The embedding's project verifies them.
const LEMMAS: &str = include_str!("../assets/embed/src/MirLemmas.lex.tex");

/// The lexicon the embedding's titles are words of.
const LEXICON: &str = include_str!("../assets/embed/language/uorust.mir/lexicon.toml");

/// The lexicon's entries, by file name.
const ENTRIES: [(&str, &str); 3] = [
    (
        "bodies.toml",
        include_str!("../assets/embed/language/uorust.mir/entries/bodies.toml"),
    ),
    (
        "checks.toml",
        include_str!("../assets/embed/language/uorust.mir/entries/checks.toml"),
    ),
    (
        "semantics.toml",
        include_str!("../assets/embed/language/uorust.mir/entries/semantics.toml"),
    ),
];

/// The semantics module's name, which every body module imports and
/// qualifies the datatype's names by.
const MIR: &str = "Mir";

/// Each enum by its name's last segment: its paths, and per path its
/// variants with their discriminants.
pub(crate) type Enums = BTreeMap<String, Vec<(String, Vec<(String, i128)>)>>;

/// How a callee resolves, from the callee as printed, its arguments' types
/// and its destination's, where MIR prints them.
pub(crate) type Resolve<'a> = dyn Fn(&str, &[Option<String>], &str) -> Callee + 'a;

/// What an embedded body's construction reads beside the body: the target's
/// pointer width (`usize`'s and `isize`'s), each enum's variants and
/// discriminants, and how a call's callee resolves.
pub(crate) struct Cx<'a> {
    pub(crate) width: u32,
    /// Each enum by its name's last segment: its paths, and per path its
    /// variants with their discriminants.
    pub(crate) enums: &'a Enums,
    /// The path of the body a printed callee names, where one is linked.
    /// Its arguments: the callee as printed, its arguments' types and its
    /// destination's, where MIR prints them.
    pub(crate) resolve: &'a Resolve<'a>,
    /// The bytes of each allocation the dump prints (`allocN`), where every
    /// byte is initialized and none is a pointer ([`allocations`]).
    pub(crate) allocs: BTreeMap<String, Vec<u8>>,
    /// Whether the target stores an integer's most significant byte first,
    /// where the dumps record its data layout.
    pub(crate) big_endian: Option<bool>,
    /// The structs and enums core's source declares, by their name's last
    /// segment: a type's layout is read from its declaration where Rust
    /// fixes it (`#[repr(transparent)]`, a fieldless `#[repr(u8)]` enum).
    pub(crate) structs: &'a Structs,
    /// The constant items the body being embedded reads, in the order it
    /// first reads each: each is evaluated into a local of its own on entry.
    pub(crate) items: std::cell::RefCell<Vec<String>>,
    /// Each allocation the dump prints, where its bytes are read
    /// ([`allocation_cells`]).
    pub(crate) cells: BTreeMap<String, Vec<Cell>>,
    /// The first local past the body's own: the constant items' locals.
    pub(crate) base: std::cell::Cell<usize>,
    /// The block of the body being embedded that is being rendered: its
    /// calls' sites are its.
    pub(crate) block: std::cell::Cell<usize>,
    /// Each promoted and anonymous constant's key, by the stem of the file
    /// its dump is in ([`parts`]).
    pub(crate) parts: &'a BTreeMap<String, String>,
    /// The stem of the file the body being embedded is dumped to
    /// ([`dump_stem`]).
    pub(crate) stem: String,
    /// The path of the body being embedded, as its dump prints it.
    pub(crate) owner: std::cell::RefCell<String>,
    /// The paths of the statics whose initializers the dumps hold
    /// ([`statics`]).
    pub(crate) statics: &'a BTreeSet<String>,
    /// The IEEE 754 format of each float type core defines, by its name
    /// (`crate::mir::float::formats`).
    pub(crate) floats: &'a BTreeMap<String, crate::mir::float::Format>,
    /// The types of the locals of the body being embedded.
    pub(crate) locals: std::cell::RefCell<Vec<String>>,
    /// The type of the constant item a key names, where its dump prints it
    /// ([`item_type`]).
    pub(crate) item_type: &'a dyn Fn(&str) -> Option<String>,
    /// The layouts rustc chose for the aggregates the dumps print
    /// (`crate::layouts`), read where Rust does not fix a layout.
    pub(crate) layouts: &'a crate::layouts::Index,
    /// The slots of the body being embedded, in the order it first reads
    /// each.
    pub(crate) slots: std::cell::RefCell<Vec<Slot>>,
    /// The calls of the body being embedded.
    pub(crate) sites: std::cell::RefCell<Vec<Site>>,
    /// The body of a constant item no dump holds, from its declaration in
    /// the crate's source, by its path as MIR prints it.
    pub(crate) source_constant: &'a dyn Fn(&str) -> Option<IrBody>,
    /// Each allocation a dump names as a static's (`alloc7 (static: m::X)`),
    /// by name: the static's path, where one static the dumps hold ends so.
    pub(crate) static_allocs: &'a BTreeMap<String, String>,
    /// The extern statics the dumps name an allocation of (`alloc9 (extern
    /// static: m::environ)`), by path: the platform's, which the program
    /// leaves to its dictionary.
    pub(crate) externs: &'a BTreeSet<String>,
    /// The statics the program holds as its globals, in their order.
    pub(crate) globals: &'a [String],
    /// Every dump's allocations, by name ([`allocation_cells`]).
    pub(crate) every_cell: &'a BTreeMap<String, Vec<Cell>>,
    /// The text each constant item the body reads is printed by.
    pub(crate) item_texts: std::cell::RefCell<BTreeMap<String, String>>,
    /// The key of the body being embedded, which its calls' tokens hold.
    pub(crate) key: std::cell::RefCell<String>,
    /// The library's sources, which say whether a body tracks its caller
    /// and where its calls are ([`crate::mir::site`]).
    pub(crate) sources: &'a crate::mir::site::Sources,
    /// What the source declares of the body being embedded.
    pub(crate) declared: std::cell::RefCell<crate::mir::site::Declared>,
}

/// The fields of `core::panic::Location` as its declaration names them,
/// which `caller_location`'s value sets: the file, the line and the column.
const LOCATION_FIELDS: [&str; 3] = ["filename", "line", "col"];

/// The type of the constant item `key` names in `link`: its return
/// place's, as each of its dumps prints it, where they agree.
pub(crate) fn item_type(link: &crate::mir::link::CoreLink, key: &str) -> Option<String> {
    let path = key.split_once('#').map_or(key, |(p, _)| p);
    let mut types = link.bodies.get(path)?.iter().map(|t| {
        t.lines().find_map(|l| {
            let l = l.trim();
            let l = l
                .strip_prefix("let mut _0: ")
                .or_else(|| l.strip_prefix("let _0: "))?;
            l.strip_suffix(';').map(str::to_string)
        })
    });
    let first = types.next()??;
    types
        .all(|t| t.as_deref() == Some(first.as_str()))
        .then_some(first)
}

/// The initializer of the static a dump holds, as a constant's body by the
/// static's path: rustc dumps a static's initializer as `static NAME: T =
/// {..}`, which a constant item's body is but for its keyword: its initial
/// value, which the program holds as a global (see [`global_statics`]).
pub(crate) fn static_body(text: &str) -> Option<IrBody> {
    let path = text
        .lines()
        .next()?
        .strip_prefix("// MIR for `")?
        .split_once('`')?
        .0;
    let head = text.lines().find_map(|l| l.strip_prefix("static "))?;
    let constant = text.replacen(
        &format!("static {head}"),
        &format!("const {}", head.strip_prefix("mut ").unwrap_or(head)),
        1,
    );
    let mut body = crate::mir::parse::bodies(&constant).into_iter().next()?;
    body.path = path.to_string();
    Some(body)
}

/// A cast of a function item MIR prints by its bare path, `a::f::<T> as
/// fn(*mut u8) (PointerCoercion(ReifyFnPointer(Safe), Implicit))`: the
/// item's path and the cast's type.
fn fn_item_cast(text: &str) -> Option<(String, String)> {
    let open = text.strip_suffix(')')?.rfind(" (")?;
    let at = text[..open].rfind(" as ")?;
    let kind_text = &text[open + 2..text.len() - 1];
    if !kind_text.starts_with("PointerCoercion(ReifyFnPointer") {
        return None;
    }
    let path = text[..at].trim();
    let is_path = !path.is_empty()
        && !path.starts_with(['_', '&', '*', '('])
        && crate::mir::parse::operand(path).is_none();
    is_path.then(|| (path.to_string(), text[at + 4..open].to_string()))
}

/// The static MIR prints as `printed`, a path from the module of the body
/// `owner` (`ID`, `destructors::list::DTORS`): of the statics whose path
/// ends so, the one whose path shares the most leading segments with the
/// reader's, where one does.
fn nearest(statics: &BTreeSet<String>, printed: &str, owner: &str) -> Option<String> {
    let shared = |k: &str| {
        k.split("::")
            .zip(owner.split("::"))
            .take_while(|(a, b)| a == b)
            .count()
    };
    let suffix = format!("::{printed}");
    let found: Vec<&String> = statics
        .iter()
        .filter(|k| *k == printed || k.ends_with(&suffix))
        .collect();
    let best = found.iter().map(|k| shared(k)).max()?;
    match found
        .iter()
        .filter(|k| shared(k) == best)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [one] => Some((**one).clone()),
        _ => None,
    }
}

/// [`nearest`], or, for a static an impl's method declares, which MIR
/// prints through the impl's type (`pool::Pool::<T, F>::f::X`) and dumps
/// through the impl (`pool::<impl at ..>::f::X`), the static the longest
/// tail of that path past the type names.
fn nearest_static(statics: &BTreeSet<String>, printed: &str, owner: &str) -> Option<String> {
    if let Some(p) = nearest(statics, printed, owner) {
        return Some(p);
    }
    let segments = crate::mir::parse::split_path(printed);
    (1..segments.len().saturating_sub(1))
        .find_map(|n| nearest(statics, &segments[n..].join("::"), owner))
}

/// The path a dump's first line names its body by (`// MIR for `a::b``).
fn dumped_path(text: &str) -> Option<&str> {
    text.lines()
        .next()?
        .strip_prefix("// MIR for `")?
        .split_once('`')
        .map(|(p, _)| p)
}

/// The statics whose initializers `link`'s dumps hold, in order: each is
/// one global of the program, at its index here. The evaluator runs one
/// thread, so a static, `mut` or of interior mutability (`Atomic`,
/// `Cell`), is exactly one allocation every body reads and writes alike.
pub(crate) fn global_statics(link: &crate::mir::link::CoreLink) -> Vec<String> {
    statics(link).into_iter().collect()
}

/// The paths of the statics whose initializers `link`'s dumps hold.
pub(crate) fn statics(link: &crate::mir::link::CoreLink) -> BTreeSet<String> {
    link.bodies
        .iter()
        .filter(|(_, texts)| matches!(texts.as_slice(), [t] if static_body(t).is_some()))
        .map(|(p, _)| p.clone())
        .collect()
}

/// The static a constant names, `<static(DefId(0:1 ~ core[6af5]::a::B))>`,
/// by its path in its crate.
fn static_path(text: &str) -> Option<&str> {
    let inner = text.strip_prefix("<static(DefId(")?.strip_suffix("))>")?;
    let (_, path) = inner.split_once(" ~ ")?;
    Some(path.split_once("]::")?.1)
}

/// Whether a body is a part of the body that reads it: a promoted constant
/// or an anonymous one.
fn is_part(path: &str) -> bool {
    path.contains("promoted[") || path.contains("{constant#")
}

/// The stem of a dump's file name: rustc names a body's dump by its item's
/// path (`core.net-ip_addr-{impl#3}-is_broadcast`), then the pass's number
/// and name, and a part's by its owner's stem and its own last segment
/// (`-promoted[0]`, `-{constant#0}`).
pub(crate) fn dump_stem(name: &str) -> &str {
    let name = name
        .strip_suffix(".ElaborateBoxDerefs.before.mir")
        .unwrap_or(name);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// Each part's key in the program ([`is_part`]), by its dump's stem, of the
/// dumps `files` (name, text): its path, and where the dumps print several
/// parts by it (a macro's impls'), its path and definition's name, as
/// [`program_key`] keys a macro's bodies.
pub(crate) fn parts<'t>(
    krate: &str,
    link: &crate::mir::link::CoreLink,
    files: impl IntoIterator<Item = (&'t str, &'t str)>,
) -> BTreeMap<String, String> {
    let mut found: Vec<(String, String, String)> = Vec::new();
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for (name, text) in files {
        for b in crate::mir::parse::bodies(text)
            .into_iter()
            .filter(|b| is_part(&b.path))
        {
            *count.entry(b.path.clone()).or_default() += 1;
            let kappa = body_kappa(krate, &b.path, text);
            found.push((dump_stem(name).to_string(), b.path, kappa));
        }
    }
    found
        .into_iter()
        .map(|(stem, path, kappa)| {
            let key = if count.get(&path).is_some_and(|n| *n > 1) {
                format!("{path}#{}", def_name(&kappa))
            } else {
                program_key(link, &path, &path, &kappa)
            };
            (stem, key)
        })
        .collect()
}

/// Whether two paths of an item name the same one: one's segments end the
/// other's. MIR prints a path trimmed to its shortest unambiguous suffix
/// (`atomic::Ordering` for `sync::atomic::Ordering`), the source its whole.
pub(crate) fn same_item(a: &str, b: &str) -> bool {
    let (a, b) = (
        crate::mir::parse::split_path(a),
        crate::mir::parse::split_path(b),
    );
    let n = a.len().min(b.len());
    a[a.len() - n..] == b[b.len() - n..]
}

/// A path's last segment, its generic arguments erased.
fn last_segment(path: &str) -> String {
    let erased = crate::mir::lower::erase(path);
    let mut segments = crate::mir::parse::split_path(&erased);
    // A last segment in brackets is a function's generic arguments
    // (`sample::<impl RandomSource + ?Sized>`), not a segment of its path.
    while segments.len() > 1 && segments.last().is_some_and(|s| s.starts_with('<')) {
        segments.pop();
    }
    segments.last().copied().unwrap_or(&erased).to_string()
}

/// The structs and enums core's source declares, by their name's last
/// segment.
pub(crate) type Structs = BTreeMap<String, Vec<crate::mir::core::CoreStruct>>;

/// [`Structs`] of what `decls` declares.
pub(crate) fn structs(decls: &crate::mir::core::CoreDecls) -> Structs {
    let mut out: Structs = BTreeMap::new();
    for s in &decls.structs {
        out.entry(s.name().to_string()).or_default().push(s.clone());
    }
    out
}

/// A type's layout as the evaluator's `Mir::Layout` describes it: its
/// term, its size in bytes, and, where the size of a pointer in it is
/// assumed thin because its pointee is a parameter whose size is not known
/// here, that pointee.
#[derive(Clone, Debug)]
struct Laid {
    term: String,
    size: u64,
    assumed: Option<String>,
}

/// Whether a type as MIR prints it is a function pointer: `fn(u8) -> u8`,
/// `unsafe fn(..)`, `extern "C" fn(..)`, each a pointer to code (memchr
/// transmutes its cached `*mut ()` to an `unsafe fn`).
fn fn_pointer(ty: &str) -> bool {
    let ty = ty.strip_prefix("unsafe ").unwrap_or(ty);
    let ty = match ty.strip_prefix("extern \"") {
        Some(rest) => rest.split_once("\" ").map_or(rest, |(_, after)| after),
        None => ty,
    };
    ty.starts_with("fn(")
}

/// A type as MIR or the source prints it, its generic arguments split off:
/// `NonNull<T>` is `NonNull` and `[T]`.
pub(crate) fn generic_parts(ty: &str) -> (&str, Vec<String>) {
    let Some(open) = ty.find('<') else {
        return (ty, Vec::new());
    };
    let Some(inner) = ty[open + 1..].strip_suffix('>') else {
        return (ty, Vec::new());
    };
    let mut args = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                args.push(inner[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    args.push(inner[start..].trim().to_string());
    (&ty[..open], args)
}

/// The pointee of a pointer or reference type, with its lifetime dropped:
/// `&'a mut T` is `T`.
pub(crate) fn pointee(ty: &str) -> Option<&str> {
    // The source's types as `quote` prints them space a raw pointer's star.
    let rest = ty
        .strip_prefix("*const ")
        .or_else(|| ty.strip_prefix("*mut "))
        .or_else(|| ty.strip_prefix("* const "))
        .or_else(|| ty.strip_prefix("* mut "))
        .or_else(|| ty.strip_prefix('&'))?;
    let rest = match rest.strip_prefix('\'') {
        Some(r) => r.split_once(' ').map_or(r, |(_, t)| t),
        None => rest,
    };
    Some(rest.strip_prefix("mut ").unwrap_or(rest).trim())
}

/// How much of a structural walk a layout takes before it is refused: the
/// walk follows a type's declarations, which nest no deeper than core
/// declares types.
fn layout_depth(structs: &Structs) -> usize {
    structs.len()
}

/// The constant item a constant operand names: a promoted, an anonymous
/// constant or a named one, as MIR prints it, `X::promoted[0]`,
/// `X::{constant#1}`, `X::CHUNK_SIZE`.
fn item_suffix(text: &str) -> Option<(&str, &str)> {
    let (owner, last) = text.rsplit_once("::")?;
    let named = last
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && last.starts_with(|c: char| c.is_ascii_uppercase());
    (last.starts_with("promoted[") || last.starts_with("{constant#") || named)
        .then_some((owner, last))
}

/// How a call's callee is embedded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Callee {
    /// A body of the program, by its path.
    Body(String),
    /// An intrinsic, with no body of its own.
    Intrinsic(String),
    /// A callee a bound leaves abstract: the caller's dictionary binds it.
    Bound(String),
}

/// One embedded body: its term, the constructs it uses, those of them the
/// evaluator does not define, and the callees it leaves to a dictionary.
#[derive(Clone, Debug, Default)]
pub(crate) struct Embedded {
    pub(crate) term: String,
    pub(crate) uses: BTreeSet<String>,
    pub(crate) blocked: BTreeSet<String>,
    pub(crate) bound: BTreeSet<String>,
    /// The bodies of the program it calls or names, by key.
    pub(crate) callees: BTreeSet<String>,
    /// The intrinsics it calls, which have no body to evaluate.
    pub(crate) intrinsics: BTreeSet<String>,
    /// The types and counts it reads that its generic parameters leave to
    /// each instance: its frame's generics, which its caller supplies.
    pub(crate) slots: Vec<Slot>,
    /// Its calls of bodies of the program, each a token in its term that
    /// [`instances`] writes as the callee with its instance's generics.
    pub(crate) sites: Vec<Site>,
    /// Its key in the program.
    pub(crate) key: String,
    /// What one run of it costs the evaluator at most.
    pub(crate) cost: Cost,
}

/// The most one run of a body costs the evaluator (UE-19), from its loops'
/// bounds ([`crate::mir::trips`]): the differential check's fuel.
#[derive(Clone, Debug, Default)]
pub(crate) enum Cost {
    /// A body [`Cx::body`] did not embed, whose cost is not read.
    #[default]
    Unread,
    Bounded {
        /// Its own steps: each statement and each terminator, once for each
        /// run of its block, and its constant items' blocks once.
        steps: u128,
        /// The bodies it calls, each with the most runs of the block that
        /// calls it, and whether it is a constant item (evaluated once on
        /// entry, and its value pinned when it returns).
        calls: Vec<(String, u128, bool)>,
        /// The most fuel a step's own work takes past the step, a
        /// transmute's encoding and decoding: the evaluator bounds it by
        /// the fuel left.
        work: u128,
        /// The fuel pinning its value takes, as a constant item's, where
        /// its layout is read.
        pin: Option<u128>,
    },
    Unbounded(String),
}

/// What a body leaves to its instance: a type's layout or a count, as the
/// body prints it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Slot {
    Layout(String),
    Count(String),
}

/// A call of a body of the program: the token its term holds in the
/// callee's place, the callee's key, and the call as MIR prints it (the
/// callee, its arguments' types, its destination's), which the linker
/// binds the callee's generic parameters by.
#[derive(Clone, Debug, Default)]
pub(crate) struct Site {
    pub(crate) token: String,
    pub(crate) callee: String,
    pub(crate) text: String,
    pub(crate) args: Vec<Option<String>>,
    pub(crate) dest: String,
    /// Whether the callee is a constant item the body reads on entry, whose
    /// bindings are its trait's `Self` (`<T as Trait>::C`), or none for a
    /// part of the body, which shares the body's parameters.
    pub(crate) constant: bool,
    /// The body's block that makes the call, or none for a constant item,
    /// which the body evaluates once, on entry.
    pub(crate) block: Option<usize>,
}

/// The constructs the evaluator answers `Outcome.undefined` with, read
/// from its source: the string after each `Outcome.undefined`.
/// The intrinsics the evaluator defines, read from its source: each name
/// its table of intrinsics compares a callee's against.
pub(crate) fn defined_intrinsics() -> &'static BTreeSet<String> {
    static READ: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    READ.get_or_init(|| {
        let needle = "(prim equal name \"";
        SEMANTICS
            .match_indices(needle)
            .filter_map(|(at, _)| {
                let rest = &SEMANTICS[at + needle.len()..];
                rest.find('"').map(|end| rest[..end].to_string())
            })
            .collect()
    })
}

/// The float intrinsics the evaluator defines by their operation
/// (`floatIntrinsic`): an intrinsic named for one float type (`sqrtf32`,
/// `round_ties_even_f64`) is its operation at that type's format, as is a
/// generic one (`fabs::<f32>`).
pub(crate) fn defined_float_intrinsics() -> &'static BTreeSet<String> {
    static READ: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    READ.get_or_init(|| scraped("(prim equal op \""))
}

/// The intrinsics of a type argument's layout the evaluator defines
/// (`layoutQuery`).
pub(crate) fn defined_layout_queries() -> &'static BTreeSet<String> {
    static READ: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    READ.get_or_init(|| scraped("(prim equal query \""))
}

fn scraped(needle: &str) -> BTreeSet<String> {
    SEMANTICS
        .match_indices(needle)
        .filter_map(|(at, _)| {
            let rest = &SEMANTICS[at + needle.len()..];
            rest.find('"').map(|end| rest[..end].to_string())
        })
        .collect()
}

/// The layout query an intrinsic is: `align_of_val` of a sized value is
/// its type's `align_of`; any other intrinsic is its own name.
fn layout_query(path: &str) -> String {
    match intrinsic_name(path).as_str() {
        "align_of_val" => "align_of".to_string(),
        name => name.to_string(),
    }
}

/// An intrinsic by its name: rustc names an intrinsic by its item's name,
/// whatever crate's path a call prints it by.
fn intrinsic_name(path: &str) -> String {
    crate::mir::parse::split_path(path)
        .last()
        .copied()
        .unwrap_or(path)
        .to_string()
}

/// The intrinsics a type query answers (`QUERY::<T>`, `type_query`): a
/// count rustc's layout data gives, which the evaluator reads as one.
const TYPE_QUERIES: &[&str] = &[
    "field_offset",
    "assert_zero_valid",
    "assert_mem_uninitialized_valid",
];

/// The intrinsics whose effect is the platform's, outside the abstract
/// machine: `breakpoint`'s trap is a debugger's, and `va_arg` reads the
/// C ABI's variadic arguments, which the target's calling convention
/// lays out. Each is a call the program leaves to its dictionary, as a
/// foreign function is.
const PLATFORM_EFFECTS: &[&str] = &["breakpoint", "va_arg"];

/// The float intrinsics whose result the platform gives: each intrinsic
/// core declares without a body (`library/core/src/intrinsics/mod.rs`)
/// whose documentation names a stabilized version (`f32::cos`) that its
/// float module, std's or core's, documents under "Unspecified precision"
/// ("non-deterministic ... varies by platform"). Such a call is the
/// platform's math library's, which the evaluator takes as input.
pub(crate) fn platform_intrinsics(library: &Path) -> BTreeSet<String> {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).unwrap_or_default();
    let intrinsics = read(library.join("core/src/intrinsics/mod.rs"));
    let mut out = BTreeSet::new();
    for (name, doc) in documented_fns(&intrinsics) {
        let stable = doc
            .split("The stabilized version of this intrinsic is")
            .nth(1)
            .and_then(|r| r.split('`').nth(1))
            .and_then(|r| r.split_once("::"));
        let Some((ty, method)) = stable else {
            continue;
        };
        let unspecified = ["std", "core"].iter().any(|krate| {
            documented_fns(&read(library.join(format!("{krate}/src/num/{ty}.rs"))))
                .iter()
                .any(|(m, d)| m == method && d.contains("# Unspecified precision"))
        });
        if unspecified {
            out.insert(name);
        }
    }
    out
}

/// Each function a source declares, with the doc comment above it (its
/// attributes between), where it has one.
fn documented_fns(source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut doc = String::new();
    for line in source.lines() {
        let l = line.trim();
        if let Some(d) = l.strip_prefix("///") {
            doc.push_str(d);
            doc.push('\n');
            continue;
        }
        if l.starts_with("#[") {
            continue;
        }
        let head = [
            "pub const unsafe fn ",
            "pub unsafe fn ",
            "pub const fn ",
            "pub fn ",
        ]
        .iter()
        .find_map(|k| l.strip_prefix(k));
        if let Some(name) = head.and_then(|h| h.split(['(', '<']).next()) {
            if !doc.is_empty() {
                out.push((name.to_string(), std::mem::take(&mut doc)));
            }
        }
        doc.clear();
    }
    out
}

pub(crate) fn undefined_constructs() -> &'static BTreeSet<String> {
    static READ: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    READ.get_or_init(read_undefined)
}

fn read_undefined() -> BTreeSet<String> {
    let needle = "Outcome.undefined \"";
    SEMANTICS
        .match_indices(needle)
        .filter_map(|(at, _)| {
            let rest = &SEMANTICS[at + needle.len()..];
            rest.find('"').map(|end| rest[..end].to_string())
        })
        .collect()
}

/// The causes the evaluator answers a valid call of intrinsic `name` with
/// `Outcome.undefined` by, where its model does not yet place the case:
/// each `intrinsic a, b: why` it names `name` in.
fn intrinsic_gaps(name: &str) -> Vec<String> {
    undefined_constructs()
        .iter()
        .filter(|u| {
            u.strip_prefix("intrinsic ")
                .and_then(|r| r.split_once(": "))
                .is_some_and(|(names, _)| names.split(", ").any(|n| n == name))
        })
        .cloned()
        .collect()
}

/// `Option.none` of a call site: one whose location is not read.
fn no_span() -> String {
    format!("(ctor Option.none [{}])", mir_name("Span"))
}

/// `Option.some` of a call site: its file, line and column.
fn span_term(span: &crate::mir::ir::IrSpan) -> String {
    format!(
        "(ctor Option.some [{}] (record {} {{file := {}, line := {}, column := {}}}))",
        mir_name("Span"),
        mir_name("Span"),
        mir_string(&span.file),
        span.line,
        span.column
    )
}

/// A LexLean name of the datatype, qualified by the semantics module.
pub(crate) fn mir_name(name: &str) -> String {
    format!("{MIR}::{name}")
}

/// `(ctor Mir::T.c a b)`.
pub(crate) fn mir_ctor(name: &str, args: &[String]) -> String {
    if args.is_empty() {
        format!("(ctor {})", mir_name(name))
    } else {
        format!("(ctor {} {})", mir_name(name), args.join(" "))
    }
}

/// The constructor a kind of `spec` is in the datatype: `BinOp::AddUnchecked`
/// is `BinOp.addUnchecked`. `BorrowKind::Mut` is `mutable`, `mut` being a
/// Lean keyword.
pub(crate) fn kind_ctor(path: &str) -> String {
    let (ty, variant) = path.split_once("::").unwrap_or(("", path));
    let mut chars = variant.chars();
    let lower = match chars.next() {
        Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    };
    let lower = if lower == "mut" {
        "mutable".to_string()
    } else {
        lower
    };
    format!("{ty}.{lower}")
}

/// The operand and the count of a repeat as MIR prints it, `[const 0_u8;
/// 32]`, where the count is a literal.
fn repeat_parts(text: &str) -> Option<(&str, &str)> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (operand, count) = inner.rsplit_once(';')?;
    Some((operand.trim(), count.trim()))
}

/// The one type argument of a callee as MIR prints it, `size_of::<T>`.
fn type_argument(callee: &str) -> Option<String> {
    let (_, args) = generic_parts(callee.trim());
    match args.as_slice() {
        [t] => Some(t.clone()),
        _ => None,
    }
}

/// The metadata type `ptr_metadata::<T, M>` prints, `M`.
fn metadata_argument(callee: &str) -> Option<String> {
    let (_, args) = generic_parts(callee.trim());
    match args.as_slice() {
        [_, m] => Some(m.clone()),
        _ => None,
    }
}

/// A count as MIR prints a constant of `usize`, `4_usize`, or a bare `4`.
fn count_value(text: &str) -> Option<u64> {
    let count = text.trim();
    let count = count.split_once('_').map_or(count, |(n, _)| n);
    count.parse().ok()
}

/// A subslice projection as MIR prints it, `[1:]`, `[:-1]`, `[1:-2]` (from
/// the end) or `[1..3]` (of an array): its first element, its end (counted
/// from the end where so), and whether it is.
fn subslice(text: &str) -> Option<(u64, u64, bool)> {
    let t = text.trim();
    let t = t
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(t);
    let num = |n: &str| -> Option<u64> {
        let n = n.trim();
        if n.is_empty() {
            Some(0)
        } else {
            n.parse().ok()
        }
    };
    if let Some((a, b)) = t.split_once("..") {
        return Some((num(a)?, b.trim().parse().ok()?, false));
    }
    let (a, b) = t.split_once(':')?;
    let b = b.trim();
    let to = match b.strip_prefix('-') {
        Some(n) => n.parse().ok()?,
        None if b.is_empty() => 0,
        None => return None,
    };
    Some((num(a)?, to, true))
}

/// The element type and length of an array type as MIR prints it,
/// `[u32; 10]`.
/// Whether a type MIR prints is an array, of a count it prints or a
/// parameter's (`[u8; N]`), and so sized: its brackets hold a `;` at no
/// depth of its element's brackets.
fn is_array(ty: &str) -> bool {
    let Some(inner) = ty
        .trim()
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
    else {
        return false;
    };
    let mut depth = 0usize;
    for c in inner.chars() {
        match c {
            '[' | '(' | '<' => depth += 1,
            ']' | ')' | '>' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn array_ty(ty: &str) -> Option<(&str, usize)> {
    let inner = ty.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (elem, count) = inner.rsplit_once(';')?;
    Some((elem.trim(), count.trim().parse().ok()?))
}

/// The bytes of a byte string literal as MIR prints it, `b"a\x00\n"`.
fn byte_string(text: &str) -> Option<Vec<u8>> {
    let inner = text.strip_prefix("b\"")?.strip_suffix('"')?;
    let mut out = Vec::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next()? {
            'n' => out.push(b'\n'),
            'r' => out.push(b'\r'),
            't' => out.push(b'\t'),
            '0' => out.push(0),
            '\\' => out.push(b'\\'),
            '\'' => out.push(b'\''),
            '"' => out.push(b'"'),
            'x' => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).ok()?);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The bytes of each allocation a dump prints after its bodies, `alloc12
/// (size: 3, align: 1) { 61 62 63 │ abc }`, where every byte is
/// initialized and none is part of a pointer.
pub(crate) fn allocations(text: &str) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some((name, _)) = line.split_once(" (") else {
            continue;
        };
        if !name.starts_with("alloc") || !line.trim_end().ends_with('{') {
            continue;
        }
        let mut bytes = Some(Vec::new());
        for row in lines.by_ref() {
            if row.trim() == "}" {
                break;
            }
            // `0x10 │ 01 02 … │ ascii` or `01 02 … │ ascii`.
            let parts: Vec<&str> = row.split('│').collect();
            let hex = match parts.as_slice() {
                [_, hex, _] | [hex, _] => *hex,
                _ => "",
            };
            for token in hex.split_whitespace() {
                match (u8::from_str_radix(token, 16), bytes.as_mut()) {
                    (Ok(b), Some(v)) if token.len() == 2 => v.push(b),
                    _ => bytes = None,
                }
            }
        }
        if let Some(bytes) = bytes {
            out.insert(name.to_string(), bytes);
        }
    }
    out
}

/// A byte of an allocation as a dump prints it: initialized, or one of
/// the bytes of a pointer to another allocation (`╾──alloc7──╼`), by that
/// allocation's name and the byte's index in the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cell {
    Byte(u8),
    Pointer(String, usize),
}

/// The bytes of each allocation a dump prints, where every byte is
/// initialized or part of a pointer to another allocation at offset zero:
/// a pointer token spans the pointer's bytes, `width` bits.
pub(crate) fn allocation_cells(text: &str, width: u32) -> BTreeMap<String, Vec<Cell>> {
    let bytes = usize::try_from(width / 8).unwrap_or_default();
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some((name, _)) = line.split_once(" (") else {
            continue;
        };
        if !name.starts_with("alloc") || !line.trim_end().ends_with('{') {
            continue;
        }
        let mut cells = Some(Vec::new());
        for row in lines.by_ref() {
            if row.trim() == "}" {
                break;
            }
            let parts: Vec<&str> = row.split('│').collect();
            let hex = match parts.as_slice() {
                [_, hex, _] | [hex, _] => *hex,
                _ => "",
            };
            for token in hex.split_whitespace() {
                let Some(v) = cells.as_mut() else { break };
                if let Ok(b) = u8::from_str_radix(token, 16) {
                    if token.len() == 2 {
                        v.push(Cell::Byte(b));
                        continue;
                    }
                }
                // `╾───alloc12<imm>────╼`: a pointer to the start of alloc12.
                let target = token
                    .trim_matches(|c| c == '╾' || c == '╼' || c == '─')
                    .split('<')
                    .next()
                    .unwrap_or_default();
                if target.starts_with("alloc")
                    && target[5..].chars().all(|c| c.is_ascii_digit())
                    && target.len() > 5
                {
                    v.extend((0..bytes).map(|i| Cell::Pointer(target.to_string(), i)));
                } else {
                    cells = None;
                }
            }
        }
        if let Some(cells) = cells {
            out.insert(name.to_string(), cells);
        }
    }
    out
}

/// A list term of items of type `ty`. A long list is the `append` of its
/// halves, down to runs no longer than its length's bit length, so neither
/// a run nor the appends nest deeper than about that however long it
/// grows: the linker reads a term to a bounded depth.
pub(crate) fn term_list(items: &[String], ty: &str) -> String {
    let run = (usize::BITS - items.len().leading_zeros()) as usize;
    halves(items, ty, run.max(1))
}

/// [`term_list`] of `items`, its runs at most `run` long.
fn halves(items: &[String], ty: &str, run: usize) -> String {
    if items.len() <= run {
        let mut out = format!("(nil {ty})");
        for item in items.iter().rev() {
            out = format!("(cons {item} {out})");
        }
        return out;
    }
    let (left, right) = items.split_at(items.len() / 2);
    format!(
        "(prim append {} {} : (List {ty}))",
        halves(left, ty, run),
        halves(right, ty, run)
    )
}

/// The width and signedness of an integer type as MIR prints it: `u8`,
/// `i128`, `usize` (the target's pointer width).
pub(crate) fn int_ty(printed: &str, width: u32) -> Option<(u32, bool)> {
    match printed {
        "usize" => return Some((width, false)),
        "isize" => return Some((width, true)),
        _ => {}
    }
    let signed = match printed.chars().next()? {
        'u' => false,
        'i' => true,
        _ => return None,
    };
    printed[1..]
        .parse::<u32>()
        .ok()
        .filter(|b| *b > 0)
        .map(|b| (b, signed))
}

/// An `asm!` terminator as a call the dictionary answers.
#[derive(Clone, Debug, PartialEq)]
struct InlineAsm {
    /// The assembly with its operands' kinds and registers and its
    /// options, the operands themselves left out: `asm!("syscall",
    /// inlateout("ax"), in("di"), lateout("cx"), options(NOSTACK))`.
    name: String,
    /// The operands it reads, in order.
    inputs: Vec<IrOperand>,
    /// The one place it writes.
    output: IrPlace,
    /// Where control goes after it (none for `noreturn`).
    target: Option<usize>,
}

/// The `asm!` terminator `text` prints, where its operands are registers'
/// inputs and outputs and it writes one place: a `const` or `sym` operand,
/// or several outputs, is none.
fn inline_asm(text: &str) -> Option<InlineAsm> {
    let (call, edges) = match text.rfind(" -> ") {
        Some(at) => (&text[..at], Some(&text[at + 4..])),
        None => (text, None),
    };
    let inner = call.trim().strip_prefix("asm!(")?.strip_suffix(')')?;
    // `=>`, an inout's output, is no closing bracket to the split.
    const INTO: &str = " \u{21d2} ";
    let inner = inner.replace(" => ", INTO);
    let mut name = Vec::new();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut noreturn = false;
    for part in crate::mir::parse::split_top(&inner, ',') {
        let part = part.trim();
        if part.starts_with('"') || part.starts_with("options(") || part.starts_with("clobber_abi(")
        {
            noreturn |= part.starts_with("options(") && part.contains("NORETURN");
            name.push(part.to_string());
            continue;
        }
        let (spec, rest) = part.split_once(") ")?;
        let (kind, _) = spec.split_once('(')?;
        name.push(format!("{spec})"));
        let (read, written) = match kind {
            "in" => (Some(rest), None),
            "out" | "lateout" => (None, Some(rest)),
            "inout" | "inlateout" => match rest.split_once(INTO) {
                Some((i, o)) => (Some(i), Some(o)),
                None => (Some(rest), Some(rest)),
            },
            _ => return None,
        };
        if let Some(i) = read {
            inputs.push(crate::mir::parse::operand(i.trim())?);
        }
        match written.map(str::trim) {
            Some("_") | None => {}
            Some(o) => outputs.push(crate::mir::parse::place(o)?),
        }
    }
    let [output] = <[IrPlace; 1]>::try_from(outputs).ok()?;
    let target = match edges {
        _ if noreturn => None,
        Some(edges) => {
            let inner = edges.trim().strip_prefix('[')?.strip_suffix(']')?;
            let ret = crate::mir::parse::split_top(inner, ',')
                .into_iter()
                .find_map(|e| e.trim().strip_prefix("return: bb")?.parse().ok());
            Some(ret.or_else(|| edges.trim().strip_prefix("bb")?.parse().ok())?)
        }
        None => None,
    };
    Some(InlineAsm {
        name: format!("asm!({})", name.join(", ")),
        inputs,
        output,
        target,
    })
}

/// The integer a constant of a pattern type prints, `65536_i32 is (..)`:
/// its value, width and signedness, its base's.
fn pattern_int(text: &str, width: u32) -> Option<(i128, u32, bool)> {
    let (literal, _) = text.split_once(" is ")?;
    let (n, suffix) = literal.trim().rsplit_once('_')?;
    let (bits, signed) = int_ty(suffix, width)?;
    Some((n.parse().ok()?, bits, signed))
}

/// The two's complement bits of `n` at `bits`.
fn repr(n: i128, bits: u32) -> u128 {
    // `n as u128` is n's two's complement at 128 bits; the low `bits` of
    // it are n's at `bits`.
    let full = n as u128;
    match 1u128.checked_shl(bits) {
        Some(m) if bits < u128::BITS => full & (m - 1),
        _ => full,
    }
}

/// `Mir::Signed`.
fn signed(n: i128) -> String {
    format!(
        "(record {} {{negative := {}, magnitude := {}}})",
        mir_name("Signed"),
        n < 0,
        n.unsigned_abs()
    )
}

/// A LexLean string literal of `text`, every character kept: a quote and a
/// backslash escaped, and a control character and `%` (which LexLean's TeX
/// surface reserves) written as their `\\u` escape.
pub(crate) fn mir_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c == '%' || c.is_control() => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{u:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Cx<'_> {
    fn ty(&self, printed: &str) -> String {
        let printed = printed.trim();
        if let Some((bits, s)) = int_ty(printed, self.width) {
            return mir_ctor("Ty.int", &[bits.to_string(), s.to_string()]);
        }
        match printed {
            "bool" => mir_ctor("Ty.boolean", &[]),
            "char" => mir_ctor("Ty.character", &[]),
            "()" => mir_ctor("Ty.unit", &[]),
            _ => match self.floats.get(printed) {
                Some(f) => mir_ctor("Ty.float", &[f.exp.to_string(), f.frac.to_string()]),
                None => mir_ctor("Ty.other", &[mir_string(printed)]),
            },
        }
    }

    /// The source and target `Ty` of a cast of `o` to `ty` between numbers,
    /// one of them a float.
    fn numeric(&self, o: &IrOperand, ty: &str) -> Option<(String, String)> {
        let source = self.operand_ty(o)?;
        let number = |t: &str| self.float(t).is_some() || int_ty(t.trim(), self.width).is_some();
        (number(&source)
            && number(ty)
            && (self.float(&source).is_some() || self.float(ty).is_some()))
        .then(|| (self.ty(&source), self.ty(ty)))
    }

    /// The format of a float type, as MIR prints it.
    fn float(&self, ty: &str) -> Option<crate::mir::float::Format> {
        self.floats.get(ty.trim()).copied()
    }

    /// A call of the float intrinsic `path` on `args`: the format of its
    /// first argument's float type, and its operation, the intrinsic's name
    /// less that type's suffix, where the evaluator defines it.
    fn float_intrinsic(
        &self,
        path: &str,
        args: &[IrOperand],
    ) -> Option<(crate::mir::float::Format, String)> {
        let ty = self.operand_ty(args.first()?)?;
        let format = self.float(&ty)?;
        let name = intrinsic_name(path);
        let op = name
            .strip_suffix(ty.trim())
            .map_or(name.as_str(), |s| s.trim_end_matches('_'));
        defined_float_intrinsics()
            .contains(op)
            .then(|| (format, op.to_string()))
    }

    /// The type MIR prints of an operand, where it does: a local's, each
    /// projection's (a field's type, a pointer's pointee, an array's or a
    /// slice's element), or a literal's suffix.
    fn operand_ty(&self, o: &IrOperand) -> Option<String> {
        match o {
            IrOperand::Copy(p) | IrOperand::Move(p) => {
                let mut ty = self.locals.borrow().get(p.local)?.clone();
                for step in &p.projection {
                    ty = match step {
                        IrProjection::Field(_, t) => t.clone(),
                        IrProjection::Deref => pointee(&ty)?.to_string(),
                        IrProjection::Index(_) | IrProjection::ConstantIndex(..) => {
                            match array_ty(&ty) {
                                Some((elem, _)) => elem.to_string(),
                                None => ty.strip_prefix('[')?.strip_suffix(']')?.to_string(),
                            }
                        }
                        IrProjection::Downcast(_) => ty,
                        IrProjection::Other(..) => return None,
                    };
                }
                Some(ty)
            }
            IrOperand::Constant(IrConstant::Int(_, suffix)) => Some(suffix.clone()),
            // A constant of a pattern type is of `(base) is pattern`.
            IrOperand::Constant(IrConstant::Other(text))
                if pattern_int(text, self.width).is_some() =>
            {
                let (literal, pattern) = text.split_once(" is ")?;
                let (_, base) = literal.trim().rsplit_once('_')?;
                Some(format!("({base}) is {pattern}"))
            }
            IrOperand::Constant(c) if self.item(c).is_some() => {
                (self.item_type)(&self.item(c).unwrap_or_default())
            }
            IrOperand::Constant(IrConstant::Other(text) | IrConstant::Path(text)) => self
                .float_literal(text)
                .map(|(name, _)| name)
                // A trait's constant item, `<Self as Trait>::C`: the type
                // the trait declares it of.
                .or_else(|| {
                    text.starts_with('<')
                        .then(|| (self.item_type)(text))
                        .flatten()
                }),
            _ => None,
        }
    }

    /// A float literal as MIR prints it with its type's suffix (`1.0E+4f32`,
    /// `-0f64`, `+Inf_f16`), or a constant IEEE 754 fixes, by core's name
    /// for it (`f32::<impl f32>::NAN`): the type, and the prelude's term for
    /// its bits.
    fn float_literal(&self, text: &str) -> Option<(String, String)> {
        self.floats.iter().find_map(|(name, f)| {
            let lit = text
                .strip_suffix(name.as_str())
                .map(|l| l.strip_suffix('_').unwrap_or(l));
            let named = text.strip_prefix(&format!("{name}::<impl {name}>::"));
            let term = lit
                .or(named)
                .and_then(|l| crate::mir::float::literal(*f, l))?;
            Some((name.clone(), term))
        })
    }

    /// The discriminant of `variant` of the enum MIR prints as `ty`, where
    /// core declares one enum by that name with that variant.
    fn discriminant(&self, ty: &str, variant: &str) -> Option<i128> {
        let erased = crate::mir::lower::erase(ty);
        let last = crate::mir::parse::split_path(&erased)
            .last()
            .copied()
            .unwrap_or(&erased)
            .trim_end_matches("::")
            .to_string();
        let candidates = self.enums.get(&last)?;
        let has = |vs: &Vec<(String, i128)>| vs.iter().any(|(v, _)| v == variant);
        // The enum of exactly the printed path (a crate prints its own
        // items by their paths from its root), else each whose path the
        // printed one ends alike with, else, for a path through a module
        // no source declares (one a build script writes), the enums of
        // that name its crate declares; each where they give the variant's
        // discriminant alike.
        let exact: Vec<_> = candidates
            .iter()
            .filter(|(path, vs)| *path == erased && has(vs))
            .collect();
        let alike: Vec<_> = candidates
            .iter()
            .filter(|(path, vs)| (candidates.len() == 1 || same_item(&erased, path)) && has(vs))
            .collect();
        let first = crate::mir::parse::split_path(&erased)
            .first()
            .copied()
            .unwrap_or_default();
        let owned: Vec<_> = candidates
            .iter()
            .filter(|(path, vs)| {
                crate::mir::parse::split_path(path).first().copied() == Some(first) && has(vs)
            })
            .collect();
        // Else every enum of the name that has the variant.
        let only: Vec<_> = candidates.iter().filter(|(_, vs)| has(vs)).collect();
        let chosen = if !exact.is_empty() {
            exact
        } else if !alike.is_empty() {
            alike
        } else if !owned.is_empty() {
            owned
        } else if !only.is_empty() {
            only
        } else {
            return None;
        };
        let mut found = chosen.into_iter();
        // Where several enums could be the one MIR prints (cfg'd twins, a
        // path printed from the reader's module), the variant's
        // discriminant, where every one of them gives it alike.
        let (_, variants) = found.next()?;
        let d = variants
            .iter()
            .find(|(v, _)| v == variant)
            .map(|(_, d)| *d)?;
        found
            .all(|(_, vs)| vs.iter().find(|(v, _)| v == variant).map(|(_, d)| *d) == Some(d))
            .then_some(d)
    }

    /// The value of a variant's discriminant expression, the anonymous
    /// constant `E::V::{constant#0}`: the discriminant the enum's source
    /// gives `V`.
    fn explicit_discriminant(&self, text: &str) -> Option<i128> {
        match crate::mir::parse::split_path(text).as_slice() {
            [.., ty, v, last] if last.starts_with("{constant#") => self.discriminant(ty, v),
            _ => None,
        }
    }

    /// The variant and discriminant of a unit variant, or a unit struct
    /// (no variant, discriminant zero), that `text` names as a path.
    fn unit_adt(&self, text: &str) -> Option<(String, i128)> {
        let erased = crate::mir::lower::erase(text);
        if !erased
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
        {
            return None;
        }
        let segments = crate::mir::parse::split_path(text);
        if let [.., ty, v] = segments.as_slice() {
            if let Some(d) = self.discriminant(ty, v) {
                return Some(((*v).to_string(), d));
            }
        }
        let last = crate::mir::parse::split_path(&erased).last().copied()?;
        last.starts_with(|c: char| c.is_ascii_uppercase())
            .then(|| (String::new(), 0))
    }

    /// Whether values of `ty` have a size known at compile time: `None`
    /// where `ty` is a parameter, or names one, whose bounds are not read.
    fn sized(&self, ty: &str, depth: usize) -> Option<bool> {
        let ty = ty.trim();
        if depth == 0 {
            return None;
        }
        if ty == "str" || ty.starts_with("dyn ") || (ty.starts_with('[') && !is_array(ty)) {
            return Some(false);
        }
        if int_ty(ty, self.width).is_some()
            || self.float(ty).is_some()
            || matches!(ty, "bool" | "char" | "()")
            || is_array(ty)
            || pointee(ty).is_some()
            || fn_pointer(ty)
            || ty.starts_with('(')
        {
            return Some(true);
        }
        let (name, args) = generic_parts(ty);
        let name = crate::mir::parse::split_path(name)
            .last()
            .copied()
            .unwrap_or(name);
        // Of the declarations of that name, the one of the path MIR prints
        // (`ty::Type`, not `token::Type` or core's `mem::type_info::Type`).
        // A path a re-export prints (`core::ffi::CStr` of `ffi::c_str::CStr`)
        // is the one declaration of its name.
        let (head, _) = generic_parts(ty);
        let named = self.structs.get(name).map_or(&[][..], Vec::as_slice);
        let found: Vec<_> = named.iter().filter(|d| same_item(head, &d.path)).collect();
        let decl = match (found.as_slice(), named) {
            ([one], _) => *one,
            ([], [one]) => one,
            // Where no path tells them apart, an enum (or a type of data)
            // is sized whichever of them it is.
            ([], [_, ..])
                if named
                    .iter()
                    .all(|d| d.variants.is_some() || d.data.is_some()) =>
            {
                return Some(true);
            }
            _ => return None,
        };
        if decl.variants.is_some() || decl.data.is_some() {
            return Some(true);
        }
        // A struct is unsized where its last field is.
        let Some((_, last)) = decl.fields.last() else {
            return Some(true);
        };
        let bound: BTreeMap<String, String> = decl.params.iter().cloned().zip(args).collect();
        // A type parameter MIR prints no argument for (`Box<T>`'s defaulted
        // allocator) is `Sized` unless the declaration bounds it `?Sized`.
        let field = last.trim();
        if decl.params.iter().any(|p| p == field) && !bound.contains_key(field) {
            return Some(!decl.unsized_params.iter().any(|p| p == field));
        }
        self.sized(&crate::mir::stdlib::substitute(last, &bound), depth - 1)
    }

    /// The layout of `ty`, where Rust fixes it and the dumps' data layout
    /// gives each primitive's: an integer's, `bool`'s and `char`'s sizes,
    /// a pointer's (twice a thin one's where its pointee is unsized: its
    /// metadata is a `usize` or a vtable's pointer), an array's (its
    /// element's stride, its length), a `#[repr(transparent)]` struct's
    /// (its one field of non-zero size, at offset zero) and a fieldless
    /// enum's of an integer representation (its tag, of that integer).
    fn layout(&self, ty: &str, depth: usize) -> Option<Laid> {
        let ty = ty.trim();
        let depth = depth.checked_sub(1)?;
        // A type rustc finds uninhabited has no layout of the table: every
        // layout the evaluator is given is of a type some value is of.
        if self.layouts.get(ty).is_some_and(|(_, l)| l.uninhabited) {
            return None;
        }
        let laid = |term: String, size: u64| Laid {
            term,
            size,
            assumed: None,
        };
        let bytes = |bits: u32| u64::from(bits / 8);
        if let Some((bits, signed)) = int_ty(ty, self.width) {
            if bits > 8 && self.big_endian.is_none() {
                return None;
            }
            return Some(laid(
                mir_ctor("Layout.int", &[bytes(bits).to_string(), signed.to_string()]),
                bytes(bits),
            ));
        }
        // A float is laid out as the unsigned integer of its width, its bits.
        if let Some(f) = self.float(ty) {
            let bits = 1 + f.exp + f.frac;
            self.big_endian?;
            return Some(laid(
                mir_ctor("Layout.int", &[bytes(bits).to_string(), false.to_string()]),
                bytes(bits),
            ));
        }
        match ty {
            // The Reference ("Type layout"): `bool` is one byte, `char` four.
            "bool" => return Some(laid(mir_ctor("Layout.boolean", &[]), 1)),
            "char" => {
                return (self.big_endian.is_some())
                    .then(|| laid(mir_ctor("Layout.character", &[]), bytes(u32::BITS)));
            }
            _ => {}
        }
        // A pattern type, `(u32) is 1..`, is laid out as its base type and
        // valid only in its range, as rustc's layout's `valid_range` makes
        // it: a value outside it is undefined. The range is the pattern's,
        // read from the type, on the base's bits (its two's complement for
        // a signed base), and wraps past the last value where its start is
        // above its end, as a `valid_range` does: `(i8) is ..=-1 | 1..` is
        // `1..=255`. An open end is the base type's first or last value.
        if let Some((base, pattern)) = ty.strip_prefix('(').and_then(|t| t.split_once(") is ")) {
            // A `char` base is its scalar value's bits, `u32`'s.
            let (bits, signed) = match base.trim() {
                "char" => (u32::BITS, false),
                b => int_ty(b, self.width)?,
            };
            // Each value as its bits and as a key in its type's order: the
            // bits with the sign bit flipped, for a signed base.
            let mask = u128::MAX >> (u128::BITS - bits);
            let sign = if signed { 1u128 << (bits - 1) } else { 0 };
            let (first, last) = (sign, sign ^ mask);
            let value = |t: &str, open: u128| -> Option<u128> {
                // A `char` literal, `'\u{1}'` or `'a'`: its scalar value.
                if let Some(c) = t
                    .trim()
                    .strip_prefix('\'')
                    .and_then(|c| c.strip_suffix('\''))
                {
                    let code = match c.strip_prefix("\\u{").and_then(|h| h.strip_suffix('}')) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => {
                            let mut cs = c.chars();
                            match (cs.next(), cs.next()) {
                                (Some(one), None) => u32::from(one),
                                _ => return None,
                            }
                        }
                    };
                    return Some(u128::from(code) ^ sign);
                }
                let t = t.trim().replace('_', "");
                // `i32::MIN`, `i32::MAX`: the base's first and last values.
                if t.is_empty() || t == format!("{base}::MIN") || t == format!("{base}::MAX") {
                    return Some(if t.ends_with("MAX") { last } else { open });
                }
                let bits_of = match t.strip_prefix('-') {
                    Some(m) => {
                        let m = m.parse::<u128>().ok()?;
                        (signed && m <= sign).then(|| m.wrapping_neg() & mask)?
                    }
                    None => {
                        let n = t.parse::<u128>().ok()?;
                        (n <= mask >> u32::from(signed)).then_some(n)?
                    }
                };
                Some(bits_of ^ sign)
            };
            // One alternative: the keys of its first and last values.
            let interval = |alt: &str| -> Option<(u128, u128)> {
                match alt.split_once("..=") {
                    Some((a, b)) => Some((value(a, first)?, value(b, last)?)),
                    None => Some((value(alt.trim().strip_suffix("..")?, first)?, last)),
                }
            };
            // rustc prints an or-pattern in parentheses.
            let pattern = pattern
                .trim()
                .strip_prefix('(')
                .and_then(|p| p.strip_suffix(')'))
                .unwrap_or(pattern);
            let alts: Vec<(u128, u128)> =
                pattern.split('|').map(interval).collect::<Option<_>>()?;
            let (start, end) = match alts.as_slice() {
                [(a, b)] if a <= b => (*a, *b),
                // `..=a | b..`: from b up past the last value round to a.
                [(lo, a), (b, hi)] if *lo == first && *hi == last && a < b => (*b, *a),
                _ => return None,
            };
            let (low, high) = (start ^ sign, end ^ sign);
            let inner = self.layout(base, depth)?;
            return Some(Laid {
                term: mir_ctor(
                    "Layout.ranged",
                    &[inner.term, low.to_string(), high.to_string()],
                ),
                size: inner.size,
                assumed: None,
            });
        }
        // A pointer that is not null, `pattern_type!(*const T is !null)` as
        // `NonNull` declares its field: a pointer whose bytes are one
        // pointer's, which the encoding's pointers alone are, is never null,
        // so it is laid out as the pointer.
        if let Some(inner) = ty
            .strip_prefix("crate::")
            .unwrap_or(ty)
            .strip_prefix("pattern_type ! (")
            .and_then(|t| t.strip_suffix(" is ! null)"))
        {
            return pointee(inner).and_then(|_| self.layout(inner, depth));
        }
        let thin = bytes(self.width);
        if pointee(ty).is_some() || fn_pointer(ty) {
            let (size, assumed) =
                match pointee(ty).map(|t| (t, self.sized(t, layout_depth(self.structs)))) {
                    None | Some((_, Some(true))) => (thin, None),
                    Some((_, Some(false))) => (2 * thin, None),
                    // A pointee rustc lays out that is no slice, `str` or
                    // trait object (an extern type, `VTable`): thin.
                    Some((t, None))
                        if self.layouts.get(t).is_some()
                            && !(t.starts_with('[') || t == "str" || t.starts_with("dyn ")) =>
                    {
                        (thin, None)
                    }
                    Some((t, None)) => (thin, Some(t.to_string())),
                };
            return Some(Laid {
                term: mir_ctor("Layout.pointer", &[size.to_string()]),
                size,
                assumed,
            });
        }
        if let Some((elem, n)) = array_ty(ty) {
            let e = self.layout(elem, depth)?;
            if e.assumed.is_some() {
                return None;
            }
            let n = u64::try_from(n).ok()?;
            return Some(laid(
                mir_ctor("Layout.array", &[e.term, n.to_string(), e.size.to_string()]),
                e.size.checked_mul(n)?,
            ));
        }
        if let Some(laid) = self.rustc_layout(ty, depth) {
            return Some(laid);
        }
        let (name, args) = generic_parts(ty);
        let name = crate::mir::parse::split_path(name)
            .last()
            .copied()
            .unwrap_or(name);
        let [decl] = self.structs.get(name)?.as_slice() else {
            return None;
        };
        if let Some(variants) = &decl.variants {
            let repr = decl.repr.iter().find_map(|r| int_ty(r, self.width))?;
            if repr.0 > 8 && self.big_endian.is_none() {
                return None;
            }
            let found = self
                .enums
                .get(name)?
                .iter()
                .find(|(p, _)| *p == decl.path)?;
            // An enum of no variant is uninhabited: no layout of the table.
            if found.1.len() != variants.len() || variants.is_empty() {
                return None;
            }
            let names: Vec<String> = found.1.iter().map(|(v, _)| mir_string(v)).collect();
            let discs: Vec<String> = found.1.iter().map(|(_, d)| signed(*d)).collect();
            return Some(laid(
                mir_ctor(
                    "Layout.enumeration",
                    &[
                        bytes(repr.0).to_string(),
                        repr.1.to_string(),
                        term_list(&names, "String"),
                        term_list(&discs, &mir_name("Signed")),
                    ],
                ),
                bytes(repr.0),
            ));
        }
        if decl.data.is_some() || !decl.repr.iter().any(|r| r == "transparent") {
            return None;
        }
        let bound: BTreeMap<String, String> = decl.params.iter().cloned().zip(args).collect();
        let fields: Vec<Laid> = decl
            .fields
            .iter()
            .map(|(_, t)| self.layout(&crate::mir::stdlib::substitute(t, &bound), depth))
            .collect::<Option<_>>()?;
        // `#[repr(transparent)]`: every field but one is of size zero, and
        // the struct is laid out as that one.
        let wide: Vec<&Laid> = fields.iter().filter(|f| f.size > 0).collect();
        let (size, assumed) = match wide.as_slice() {
            [] => (0, None),
            [one] => (one.size, one.assumed.clone()),
            _ => return None,
        };
        let offsets: Vec<String> = fields.iter().map(|_| "0".to_string()).collect();
        let terms: Vec<String> = fields.into_iter().map(|f| f.term).collect();
        Some(Laid {
            term: mir_ctor(
                "Layout.record",
                &[
                    size.to_string(),
                    "true".to_string(),
                    term_list(&offsets, "Nat"),
                    term_list(&terms, &mir_name("Layout")),
                ],
            ),
            size,
            assumed,
        })
    }

    /// The fields of a tuple or a struct rustc lays out (`crate::layouts`),
    /// in declaration order, each at its offset with its type as rustc
    /// prints it; with them, whether it is a struct, and its layout.
    #[allow(clippy::type_complexity)]
    fn rustc_fields(
        &self,
        ty: &str,
    ) -> Option<(bool, Vec<(u64, String)>, &crate::layouts::TypeLayout)> {
        let (printed, l) = self.layouts.get(ty)?;
        if l.tag.is_some() {
            return None;
        }
        let [fields] = l.fields.as_slice() else {
            return None;
        };
        (fields.len() == l.offsets.len()).then_some(())?;
        let fields = l
            .offsets
            .iter()
            .zip(fields)
            .map(|(o, f)| Some((*o, f.clone()?)))
            .collect::<Option<Vec<_>>>()?;
        Some((!printed.starts_with('('), fields, l))
    }

    /// The layout of an aggregate as rustc lays it out (`crate::layouts`):
    /// a tuple or a struct as a record of its fields at rustc's offsets, a
    /// field of a pattern type as its base valid in the range rustc gives
    /// the field (or, its one field, the struct); an enum whose variants
    /// hold no data and whose tag is its discriminant as an enumeration of
    /// that tag.
    fn rustc_layout(&self, ty: &str, depth: usize) -> Option<Laid> {
        let (printed, l) = self.layouts.get(ty)?;
        if let Some(tag) = &l.tag {
            return self
                .rustc_enumeration(printed, l, tag)
                .or_else(|| self.rustc_tagged(printed, l, tag, depth));
        }
        let (adt, fields, l) = self.rustc_fields(ty)?;
        let mut terms = Vec::new();
        for (_, t) in &fields {
            let outer = l.scalar.as_ref().filter(|_| fields.len() == 1);
            terms.push(self.rustc_field(t, outer, depth)?.term);
        }
        let offsets: Vec<String> = fields.iter().map(|(o, _)| o.to_string()).collect();
        Some(Laid {
            term: mir_ctor(
                "Layout.record",
                &[
                    l.size.to_string(),
                    adt.to_string(),
                    term_list(&offsets, "Nat"),
                    term_list(&terms, &mir_name("Layout")),
                ],
            ),
            size: l.size,
            assumed: None,
        })
    }

    /// A field of an aggregate rustc lays out, of the type rustc prints:
    /// a pattern type as its base valid in the range rustc gives it (or,
    /// where it is its aggregate's one field, the aggregate's, `outer`).
    fn rustc_field(
        &self,
        t: &str,
        outer: Option<&crate::layouts::Scalar>,
        depth: usize,
    ) -> Option<Laid> {
        let base = t
            .trim()
            .strip_prefix('(')
            .and_then(|r| r.split_once(") is "))
            .map(|(b, _)| b);
        let own = self.layouts.get(t).and_then(|(_, f)| f.scalar.as_ref());
        let laid = match (base, own.or(outer)) {
            (Some(b), Some(s)) => {
                let inner = self.layout(b, depth)?;
                Laid {
                    term: mir_ctor(
                        "Layout.ranged",
                        &[inner.term, s.start.to_string(), s.end.to_string()],
                    ),
                    size: inner.size,
                    assumed: None,
                }
            }
            // A pointer that is not null and has no scalar of its own (a
            // wide one, `(*const str) is !null`, as `NonNull<str>` lays out
            // its field): laid out as the pointer, as `pattern_type!(*const
            // T is !null)` is ([`Self::layout`]).
            (Some(b), None) if pointee(b).is_some() && t.trim().ends_with(") is !null") => {
                self.layout(b, depth)?
            }
            _ => self.layout(t, depth)?,
        };
        laid.assumed.is_none().then_some(laid)
    }

    /// The variants an enum rustc lays out with a tag declares, by name
    /// and discriminant, in order: of a declaration for each target, the
    /// one whose variants are those rustc laid out.
    fn rustc_variants(&self, ty: &str, count: usize) -> Option<&Vec<(String, i128)>> {
        let (name, _) = generic_parts(ty);
        let last = crate::mir::parse::split_path(name)
            .last()
            .copied()
            .unwrap_or(name);
        let found: Vec<_> = self
            .enums
            .get(last)?
            .iter()
            .filter(|(p, vs)| same_item(name, p) && vs.len() == count)
            .collect();
        match found.as_slice() {
            [(_, variants)] => Some(variants),
            _ => None,
        }
    }

    /// An enum whose variants hold data, as rustc lays it out: its tag's
    /// offset, width and encoding (the discriminant itself, or a niche in
    /// the untagged variant's fields), and each variant as a record of its
    /// fields at rustc's offsets, as `Layout.tagged`.
    fn rustc_tagged(
        &self,
        ty: &str,
        l: &crate::layouts::TypeLayout,
        tag: &crate::layouts::Tag,
        depth: usize,
    ) -> Option<Laid> {
        use crate::layouts::{Encoding, Prim};
        let (bytes, is_signed) = match tag.scalar.prim {
            Prim::Int { bytes, signed } => (bytes, signed),
            // A pointer's niche: its null, which no pointer's bytes are.
            Prim::Pointer if matches!(tag.encoding, Encoding::Niche { .. }) => {
                (u64::from(self.width / 8), false)
            }
            _ => return None,
        };
        if bytes > 1 && self.big_endian.is_none() {
            return None;
        }
        let at = *l.offsets.get(tag.field)?;
        let variants = self.rustc_variants(ty, tag.variants.len())?;
        (l.fields.len() == tag.variants.len()).then_some(())?;
        let mut records = Vec::new();
        for (v, fields) in tag.variants.iter().zip(&l.fields) {
            (v.offsets.len() == fields.len()).then_some(())?;
            let mut terms = Vec::new();
            for f in fields {
                terms.push(self.rustc_field(f.as_deref()?, None, depth)?.term);
            }
            let offsets: Vec<String> = v.offsets.iter().map(u64::to_string).collect();
            records.push(mir_ctor(
                "Layout.record",
                &[
                    v.size.to_string(),
                    true.to_string(),
                    term_list(&offsets, "Nat"),
                    term_list(&terms, &mir_name("Layout")),
                ],
            ));
        }
        let (niche, untagged, first, last, start) = match tag.encoding {
            Encoding::Direct => (false, 0, 0, 0, 0),
            Encoding::Niche {
                untagged,
                first,
                last,
                start,
            } => (true, untagged, first, last, start),
        };
        let names: Vec<String> = variants.iter().map(|(v, _)| mir_string(v)).collect();
        let discs: Vec<String> = variants.iter().map(|(_, d)| signed(*d)).collect();
        Some(Laid {
            term: mir_ctor(
                "Layout.tagged",
                &[
                    l.size.to_string(),
                    at.to_string(),
                    bytes.to_string(),
                    is_signed.to_string(),
                    niche.to_string(),
                    untagged.to_string(),
                    first.to_string(),
                    last.to_string(),
                    start.to_string(),
                    term_list(&names, "String"),
                    term_list(&discs, &mir_name("Signed")),
                    term_list(&records, &mir_name("Layout")),
                ],
            ),
            size: l.size,
            assumed: None,
        })
    }

    /// An enum whose variants hold no data, its tag the whole of it and
    /// its discriminant, as `Layout.enumeration`.
    fn rustc_enumeration(
        &self,
        ty: &str,
        l: &crate::layouts::TypeLayout,
        tag: &crate::layouts::Tag,
    ) -> Option<Laid> {
        let crate::layouts::Prim::Int {
            bytes,
            signed: is_signed,
        } = tag.scalar.prim
        else {
            return None;
        };
        let fieldless = tag.variants.iter().all(|v| v.offsets.is_empty());
        let direct = tag.encoding == crate::layouts::Encoding::Direct;
        (fieldless && direct && bytes == l.size && l.offsets.get(tag.field) == Some(&0))
            .then_some(())?;
        let (name, _) = generic_parts(ty);
        let last = crate::mir::parse::split_path(name)
            .last()
            .copied()
            .unwrap_or(name);
        let found: Vec<_> = self
            .enums
            .get(last)?
            .iter()
            .filter(|(p, _)| same_item(name, p))
            // A declaration for each target (`#[cfg(target_pointer_width)]`):
            // the one whose variants are those rustc laid out.
            .filter(|(_, vs)| vs.len() == tag.variants.len())
            .collect();
        let [(_, variants)] = found.as_slice() else {
            return None;
        };
        if bytes > 1 && self.big_endian.is_none() {
            return None;
        }
        let names: Vec<String> = variants.iter().map(|(v, _)| mir_string(v)).collect();
        let discs: Vec<String> = variants.iter().map(|(_, d)| signed(*d)).collect();
        Some(Laid {
            term: mir_ctor(
                "Layout.enumeration",
                &[
                    bytes.to_string(),
                    is_signed.to_string(),
                    term_list(&names, "String"),
                    term_list(&discs, &mir_name("Signed")),
                ],
            ),
            size: bytes,
            assumed: None,
        })
    }

    /// The locals a constant item the body reads takes: its value's, and a
    /// static's the reference to it.
    fn slots(&self, key: &str) -> usize {
        if self.statics.contains(key) || self.externs.contains(key) {
            2
        } else {
            1
        }
    }

    /// The layout of the element a `write_bytes` call writes, its first
    /// argument's pointee, where it is read and no pointer's size in it is
    /// assumed.
    fn written_element(&self, body: &IrBody, args: &[IrOperand]) -> Option<String> {
        let ptr = arg_type(body, args.first()?)?;
        self.generic_layout(pointee(&ptr)?)
    }

    /// `Callee.transmute` of a transmute from `from` to `to`, where both
    /// layouts are read and agree in size, or a side's is its instance's:
    /// the evaluator compares the instance's sizes when it is called.
    fn transmute(&self, from: &str, to: &str) -> Option<String> {
        let (a, b) = self.transmute_layouts(from, to)?;
        Some(mir_ctor("Callee.transmute", &[a, b]))
    }

    /// The `Generic`s of a transmute's source and target layouts: the
    /// layouts themselves where both are read and alike in size, else the
    /// generic layouts the caller supplies.
    fn transmute_layouts(&self, from: &str, to: &str) -> Option<(String, String)> {
        let depth = layout_depth(self.structs);
        if let (Some(a), Some(b)) = (self.layout(from, depth), self.layout(to, depth)) {
            if a.assumed.is_none() && b.assumed.is_none() {
                return (a.size == b.size).then(|| {
                    (
                        mir_ctor("Generic.layout", &[a.term]),
                        mir_ctor("Generic.layout", &[b.term]),
                    )
                });
            }
        }
        Some((self.generic_layout(from)?, self.generic_layout(to)?))
    }

    /// The token of the call that evaluates the constant item `key` on
    /// entry: [`instances`] writes it as `Callee.constant` with the
    /// generics of the instance the body reads.
    fn constant_site(&self, key: &str) -> String {
        let token = format!(
            "\u{1}{}#{}#{key}\u{1}",
            self.key.borrow(),
            self.sites.borrow().len()
        );
        let text = self
            .item_texts
            .borrow()
            .get(key)
            .cloned()
            .unwrap_or_default();
        self.sites.borrow_mut().push(Site {
            token: token.clone(),
            callee: key.to_string(),
            text,
            args: Vec::new(),
            dest: String::new(),
            constant: true,
            block: None,
        });
        token
    }

    /// The generic parameters a type as MIR prints it names: the words
    /// that are no path's segment, no primitive and no type core declares.
    /// MIR prints a declared type by its path and a parameter bare.
    fn parameters(&self, ty: &str) -> BTreeSet<String> {
        let chars: Vec<char> = ty.chars().collect();
        let mut out = BTreeSet::new();
        let mut i = 0;
        while i < chars.len() {
            if !(chars[i].is_ascii_alphabetic() || chars[i] == '_') {
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let after_path = start >= 2 && chars[start - 1] == ':' && chars[start - 2] == ':';
            let before_path = chars.get(i) == Some(&':') && chars.get(i + 1) == Some(&':');
            let lifetime = start >= 1 && chars[start - 1] == '\'';
            let primitive = int_ty(&word, self.width).is_some()
                || self.float(&word).is_some()
                || matches!(
                    word.as_str(),
                    "bool" | "char" | "str" | "mut" | "const" | "dyn" | "is" | "null"
                );
            if !after_path
                && !before_path
                && !lifetime
                && !primitive
                && word.starts_with(|c: char| c.is_ascii_uppercase())
                && !self.structs.contains_key(&word)
                && !self.enums.contains_key(&word)
            {
                out.insert(word);
            }
        }
        out
    }

    /// The slot of the body being embedded that holds `slot`, its index
    /// the order the body first reads each.
    fn slot(&self, slot: Slot) -> String {
        let mut slots = self.slots.borrow_mut();
        let at = match slots.iter().position(|s| *s == slot) {
            Some(at) => at,
            None => {
                slots.push(slot);
                slots.len() - 1
            }
        };
        mir_ctor("Generic.slot", &[at.to_string()])
    }

    /// The `Generic` of the layout of `ty`: its layout, where it is read
    /// and assumes no pointer's size, or, where `ty` names a generic
    /// parameter, the slot its instance's fills.
    /// `Callee.callerLocation` of a call of `caller_location` whose
    /// destination is of type `dest` (`&Location<'_>`): `Location`'s layout
    /// as rustc lays it out, and the indices of the fields
    /// [`LOCATION_FIELDS`] names in its declaration.
    fn caller_location(&self, dest: &str) -> Option<String> {
        let ty = dest.trim().strip_prefix('&')?.trim();
        let ty = ty
            .strip_prefix("'static ")
            .or_else(|| ty.strip_prefix("'_ "))
            .unwrap_or(ty);
        let laid = self.layout(ty, layout_depth(self.structs))?;
        laid.assumed.is_none().then_some(())?;
        let (name, _) = generic_parts(ty);
        let last = crate::mir::parse::split_path(name)
            .last()
            .copied()
            .unwrap_or(name);
        let [decl] = self.structs.get(last)?.as_slice() else {
            return None;
        };
        let at = |field: &str| {
            decl.fields
                .iter()
                .position(|(n, _)| n.as_deref() == Some(field))
                .map(|i| i.to_string())
        };
        let mut args = vec![laid.term];
        for field in LOCATION_FIELDS {
            args.push(at(field)?);
        }
        Some(mir_ctor("Callee.callerLocation", &args))
    }

    fn generic_layout(&self, ty: &str) -> Option<String> {
        if let Some(g) = self.fixed_layout(ty) {
            return Some(g);
        }
        (!self.parameters(ty).is_empty()).then(|| self.slot(Slot::Layout(ty.trim().to_string())))
    }

    /// The `Generic` of a type whose layout is read and assumes no
    /// pointer's size: with its alignment where rustc's layout of it gives
    /// one (`Generic.aligned`).
    pub(crate) fn fixed_layout(&self, ty: &str) -> Option<String> {
        let metadata = self.pointee_metadata(ty);
        let ty = metadata.as_deref().unwrap_or(ty);
        let l = self.layout(ty, layout_depth(self.structs))?;
        l.assumed.is_none().then_some(())?;
        Some(match self.layouts.get(ty.trim()) {
            Some((_, rustc)) => mir_ctor("Generic.aligned", &[l.term, rustc.align.to_string()]),
            None => mir_ctor("Generic.layout", &[l.term]),
        })
    }

    /// The type `<X as Pointee>::Metadata` is, as rustc's built-in impl of
    /// `Pointee` gives it: `()` of a sized `X`, `usize` of a slice or
    /// `str`. `None` of any other type, or one whose size is not read.
    fn pointee_metadata(&self, ty: &str) -> Option<String> {
        let rest = ty.trim().strip_prefix('<')?;
        let (x, tail) = rest.rsplit_once(" as ")?;
        let tail = tail.strip_suffix(">::Metadata")?;
        (crate::mir::parse::split_path(tail).last() == Some(&"Pointee")).then_some(())?;
        let x = x.trim();
        if x == "str" || (x.starts_with('[') && !x.contains(';')) {
            return Some("usize".to_string());
        }
        self.sized(x, layout_depth(self.structs))?
            .then(|| "()".to_string())
    }

    /// The `Generic` of a count MIR prints (`4_usize`, or a const
    /// parameter `N`).
    fn generic_count(&self, text: &str) -> Option<String> {
        match self.count_of(text) {
            Some(n) => Some(mir_ctor("Generic.count", &[n.to_string()])),
            None => (!self.parameters(text).is_empty())
                .then(|| self.slot(Slot::Count(text.trim().to_string()))),
        }
    }

    /// A count as MIR prints it, or the answer rustc's layout data gives
    /// a type query of a type laid out (`field_offset::<F>`).
    pub(crate) fn count_of(&self, text: &str) -> Option<u64> {
        count_value(text).or_else(|| self.type_query(text))
    }

    /// A query of a type that rustc answers from the type's layout, as
    /// `QUERY::<T>`: `field_offset` of a `FieldRepresentingType<B, V, F>`,
    /// the offset rustc lays field `F` of variant `V` of `B` at; and
    /// `assert_zero_valid` and `assert_mem_uninitialized_valid`, 1 where
    /// rustc's (lax) check lets `T` be filled with zero bytes, or with
    /// `0x01` bytes, and 0 where it does not.
    fn type_query(&self, text: &str) -> Option<u64> {
        let (query, ty) = text.trim().split_once("::<")?;
        let ty = ty.strip_suffix('>')?.trim();
        match query.trim() {
            "field_offset" => self.field_offset(ty),
            "assert_zero_valid" => self.raw_init_valid(ty, 0, &mut Vec::new()).map(u64::from),
            "assert_mem_uninitialized_valid" => {
                self.raw_init_valid(ty, 1, &mut Vec::new()).map(u64::from)
            }
            _ => None,
        }
    }

    /// The offset rustc lays the field a `FieldRepresentingType<B, V, F>`
    /// names at: field `F` of `B`, of its variant `V` where `B` has a tag.
    fn field_offset(&self, ty: &str) -> Option<u64> {
        let (name, args) = generic_parts(ty);
        let last = crate::mir::parse::split_path(name).last().copied()?;
        let [base, variant, field] = args.as_slice() else {
            return None;
        };
        (last == "FieldRepresentingType").then_some(())?;
        let (variant, field) = (count_value(variant)?, count_value(field)?);
        let (_, l) = self.layouts.get(base)?;
        let offsets = match &l.tag {
            Some(tag) => &tag.variants.get(usize::try_from(variant).ok()?)?.offsets,
            None => {
                (variant == 0).then_some(())?;
                &l.offsets
            }
        };
        offsets.get(usize::try_from(field).ok()?).copied()
    }

    /// Whether rustc's lax check (`might_permit_raw_init_lax`) lets a `ty`
    /// be every byte `fill`: its scalar's valid range holds the bytes, a
    /// reference's pointee is a byte-aligned zero-sized type (no fill
    /// makes a reference otherwise), and each field of its one variant
    /// passes too. `None` where rustc's layout of a type it reads is not
    /// in the dump.
    fn raw_init_valid(&self, ty: &str, fill: u8, within: &mut Vec<String>) -> Option<bool> {
        // A type holds no value of itself: one met again inside itself is
        // a layout this does not read.
        let (printed, l) = self.layouts.get(ty)?;
        if within.iter().any(|t| t == printed) {
            return None;
        }
        within.push(printed.to_string());
        let valid = self.raw_init_fields(ty, l, fill, within);
        within.pop();
        valid
    }

    /// [`Self::raw_init_valid`] of `ty`, laid out as `l`, inside `within`.
    fn raw_init_fields(
        &self,
        ty: &str,
        l: &crate::layouts::TypeLayout,
        fill: u8,
        within: &mut Vec<String>,
    ) -> Option<bool> {
        if l.uninhabited {
            return Some(false);
        }
        if let Some(s) = &l.scalar {
            let size = match s.prim {
                crate::layouts::Prim::Int { bytes, .. } => bytes,
                crate::layouts::Prim::Float { bits } => bits / 8,
                crate::layouts::Prim::Pointer => u64::from(self.width) / 8,
            };
            let value = (0..size).fold(0u128, |v, _| (v << 8) | u128::from(fill));
            let holds = if s.start <= s.end {
                s.start <= value && value <= s.end
            } else {
                value >= s.start || value <= s.end
            };
            if !holds {
                return Some(false);
            }
        }
        if let Some(target) = ty.trim().strip_prefix('&') {
            if fill == 0 {
                return Some(false);
            }
            let target = pointee(&format!("&{target}"))?.to_string();
            let (_, p) = self.layouts.get(&target)?;
            if p.align > 1 || p.size > 0 {
                return Some(false);
            }
        }
        if l.tag.is_some() || l.offsets.is_empty() {
            return Some(true);
        }
        let fields = l.fields.first()?;
        (fields.len() == l.offsets.len()).then_some(())?;
        for f in fields {
            if !self.raw_init_valid(f.as_deref()?, fill, within)? {
                return Some(false);
            }
        }
        Some(true)
    }

    /// The value of a named constant of a primitive type (`HAS_PREFIXES`)
    /// whose body, its dump's or its declaration's, is one literal.
    fn named_scalar(&self, c: &IrConstant) -> Option<i128> {
        let IrConstant::Path(text) = c else {
            return None;
        };
        let body = (self.source_constant)(text)?;
        let [block] = body.blocks.as_slice() else {
            return None;
        };
        block.statements.iter().find_map(|s| match s {
            IrStatement::Assign(q, IrRvalue::Use(IrOperand::Constant(c)))
                if q.local == 0 && q.projection.is_empty() =>
            {
                match c {
                    IrConstant::Bool(b) => Some(i128::from(*b)),
                    IrConstant::Int(n, _) if *n >= 0 => Some(*n),
                    _ => None,
                }
            }
            _ => None,
        })
    }

    /// The key of the constant item a constant operand names, where the
    /// program has its body: the item itself, or the item of that name in
    /// the body its owner resolves to.
    fn item(&self, c: &IrConstant) -> Option<String> {
        let text = match c {
            IrConstant::Other(text) => text,
            // A named constant MIR prints as a path (`time::MILLIS_PER_SEC`),
            // where it is no variant: the body its dump holds, or the one
            // its declaration in the crate's source gives.
            IrConstant::Path(text)
                if item_suffix(text).is_some_and(|(_, last)| !is_part(last))
                    && !matches!(
                        crate::mir::parse::split_path(text).as_slice(),
                        [.., ty, v] if self.discriminant(ty, v).is_some()
                    ) =>
            {
                if let Callee::Body(p) = (self.resolve)(text, &[], "") {
                    return Some(p);
                }
                return (self.source_constant)(text).map(|_| text.clone());
            }
            _ => return None,
        };
        if let Some(path) = static_path(text).filter(|p| self.statics.contains(*p)) {
            return Some(path.to_string());
        }
        // A reference to a static, `{alloc7: &T}` of `alloc7 (static:
        // m::X)`: the static itself, whether or not a dump prints its
        // bytes, since a write through the reference is the static's.
        if let Some(path) = text
            .strip_prefix('{')
            .and_then(|t| t.split_once(": &"))
            .and_then(|(a, _)| self.static_allocs.get(a))
        {
            return Some(path.clone());
        }
        // A pointer to an extern static, `{alloc9: *const T}`: the static.
        if let Some(path) = text
            .strip_prefix('{')
            .and_then(|t| t.split_once(": *"))
            .and_then(|(a, _)| self.static_allocs.get(a))
            .filter(|p| self.externs.contains(*p) || self.globals.contains(*p))
        {
            return Some(path.clone());
        }
        let (owner, last) = item_suffix(text)?;
        // A promoted or anonymous constant is a part of the body that
        // reads it, which MIR prints by its own path (an inherent impl's by
        // its span, a trait's default method's by the trait) and the
        // constant by the reader's view of that path: the part its dump
        // names after the reader's, where the two paths end alike.
        if is_part(last) && last_segment(owner) == last_segment(&self.owner.borrow()) {
            if let Some(key) = self.parts.get(&format!("{}-{last}", self.stem)) {
                return Some(key.clone());
            }
        }
        if let Callee::Body(p) = (self.resolve)(text, &[], "") {
            return Some(p);
        }
        let named = match (self.resolve)(owner, &[], "") {
            Callee::Body(o) => {
                format!(
                    "{}::{last}",
                    o.split_once('#').map_or(o.as_str(), |(p, _)| p)
                )
            }
            _ => text.clone(),
        };
        if let Callee::Body(p) = (self.resolve)(&named, &[], "") {
            return Some(p);
        }
        // rustc heads a constant's dump by its path's last segments
        // (`const SINGLETONS0U::promoted[0]`), so its body is had by the
        // longest tail of the path that names one.
        let segments = crate::mir::parse::split_path(&named);
        (1..segments.len().saturating_sub(1))
            .find_map(
                |i| match (self.resolve)(&segments[i..].join("::"), &[], "") {
                    Callee::Body(p) if named.ends_with(&format!("::{p}")) => Some(p),
                    _ => None,
                },
            )
            .or_else(|| {
                (!is_part(last))
                    .then(|| (self.source_constant)(text).map(|_| text.clone()))
                    .flatten()
            })
    }

    fn place(&self, p: &IrPlace, e: &mut Embedded) -> String {
        let steps: Vec<String> = p
            .projection
            .iter()
            .map(|step| {
                use spec::MirProjectionElem as P;
                let (kind, term) = match step {
                    IrProjection::Deref => (P::Deref, mir_ctor("Proj.deref", &[])),
                    IrProjection::Field(i, _) => {
                        (P::Field, mir_ctor("Proj.field", &[i.to_string()]))
                    }
                    IrProjection::Index(l) => (P::Index, mir_ctor("Proj.index", &[l.to_string()])),
                    IrProjection::ConstantIndex(n, back) => (
                        P::ConstantIndex,
                        mir_ctor("Proj.constantIndex", &[n.to_string(), back.to_string()]),
                    ),
                    IrProjection::Downcast(v) => {
                        (P::Downcast, mir_ctor("Proj.downcast", &[mir_string(v)]))
                    }
                    IrProjection::Other(P::Subslice, text) if subslice(text).is_some() => {
                        let (from, to, from_end) = subslice(text).unwrap_or_default();
                        (
                            P::Subslice,
                            mir_ctor(
                                "Proj.subslice",
                                &[from.to_string(), to.to_string(), from_end.to_string()],
                            ),
                        )
                    }
                    IrProjection::Other(k, text) => {
                        e.blocked.insert(k.path().to_string());
                        (
                            *k,
                            mir_ctor("Proj.other", &[mir_string(k.path()), mir_string(text)]),
                        )
                    }
                };
                e.uses.insert(kind.path().to_string());
                term
            })
            .collect();
        format!(
            "(record {} {{base := {}, projection := {}}})",
            mir_name("Place"),
            p.local,
            term_list(&steps, &mir_name("Proj"))
        )
    }

    fn constant(&self, c: &IrConstant, e: &mut Embedded) -> String {
        match c {
            IrConstant::Other(text) | IrConstant::Path(text)
                if self.float_literal(text).is_some() =>
            {
                let (name, term) = self.float_literal(text).unwrap_or_default();
                let bits = self.float(&name).map_or(0, |f| 1 + f.exp + f.frac);
                mir_ctor("Const.int", &[bits.to_string(), false.to_string(), term])
            }
            IrConstant::Int(n, suffix) => match int_ty(suffix, self.width) {
                Some((bits, s)) => mir_ctor(
                    "Const.int",
                    &[bits.to_string(), s.to_string(), repr(*n, bits).to_string()],
                ),
                None => self.unread(c, e),
            },
            // A constant of a pattern type, `65536_i32 is (i32::MIN..=-1 | 1..)`,
            // is its base's value, which rustc's evaluation keeps in the
            // pattern; the type is laid out as its base.
            IrConstant::Other(text) if pattern_int(text, self.width).is_some() => {
                let (n, bits, s) = pattern_int(text, self.width).unwrap_or_default();
                mir_ctor(
                    "Const.int",
                    &[bits.to_string(), s.to_string(), repr(n, bits).to_string()],
                )
            }
            IrConstant::Bool(b) => mir_ctor("Const.boolean", &[b.to_string()]),
            IrConstant::Str(s) => mir_ctor("Const.text", &[mir_string(s)]),
            IrConstant::Char(ch) => mir_ctor("Const.character", &[u32::from(*ch).to_string()]),
            IrConstant::Unit => mir_ctor("Const.unit", &[]),
            IrConstant::Path(p) => {
                let segments = crate::mir::parse::split_path(p);
                let variant = match segments.as_slice() {
                    [.., ty, v] => self.discriminant(ty, v).map(|d| ((*v).to_string(), d)),
                    _ => None,
                };
                match variant {
                    Some((v, d)) => mir_ctor("Const.variant", &[mir_string(&v), signed(d)]),
                    None => match (self.resolve)(p, &[], "") {
                        Callee::Body(path) => {
                            e.callees.insert(path.clone());
                            mir_ctor("Const.function", &[mir_string(&path)])
                        }
                        Callee::Intrinsic(path) => {
                            let path = intrinsic_name(&path);
                            if !defined_intrinsics().contains(&path) {
                                e.intrinsics.insert(path.clone());
                                e.blocked.insert(format!("intrinsic {path}"));
                            }
                            mir_ctor("Const.function", &[mir_string(&path)])
                        }
                        Callee::Bound(name) => {
                            e.bound.insert(name.clone());
                            mir_ctor("Const.function", &[mir_string(&name)])
                        }
                    },
                }
            }
            IrConstant::Other(text) => match self.reference_constant(text) {
                Some(value) => mir_ctor("Const.reference", &[value]),
                None => self.unread(c, e),
            },
        }
    }

    /// The value a reference constant points to: a byte string's bytes,
    /// or an allocation of an integer array (or array of them) read at
    /// the target's byte order.
    fn reference_constant(&self, text: &str) -> Option<String> {
        if let Some(bytes) = byte_string(text) {
            return Some(self.array_value(&bytes, 8, false, 1));
        }
        if let Some(value) = self.slice_constant(text) {
            return Some(value);
        }
        if let Some(value) = text
            .strip_prefix('{')
            .and_then(|t| t.strip_suffix('}'))
            .and_then(|t| t.split_once(": &"))
            .and_then(|(alloc, ty)| self.read_cells(alloc, 0, ty, layout_depth(self.structs)))
        {
            return Some(value);
        }
        let (alloc, ty) = text
            .strip_prefix('{')?
            .strip_suffix('}')?
            .split_once(": &")?;
        let bytes = self.allocs.get(alloc)?;
        let (elem, count) = array_ty(ty)?;
        let (rows, inner) = match array_ty(elem) {
            Some((inner, n)) => (Some(count), (inner, n)),
            None => (None, (elem, count)),
        };
        let (bits, signed) = int_ty(inner.0, self.width)?;
        let size = usize::try_from(bits / 8).ok()?;
        if bytes.len() != size * inner.1 * rows.unwrap_or(1)
            || (size > 1 && self.big_endian.is_none())
        {
            return None;
        }
        match rows {
            None => Some(self.array_value(bytes, bits, signed, size)),
            Some(rows) => {
                let row = size * inner.1;
                let items: Vec<String> = (0..rows)
                    .map(|r| self.array_value(&bytes[r * row..(r + 1) * row], bits, signed, size))
                    .collect();
                Some(mir_ctor(
                    "Value.tuple",
                    &[term_list(&items, &mir_name("Value"))],
                ))
            }
        }
    }

    /// The value a slice constant points to, `Slice { alloc_id: alloc7,
    /// meta: 5 }: &T`: `meta` integers its allocation's bytes hold, where
    /// `T` is a slice of them, or a struct whose one field rustc's layout
    /// prints as one (`CStr`'s `[c_char]`, `[i8]` at the target).
    fn slice_constant(&self, text: &str) -> Option<String> {
        let rest = text.strip_prefix("Slice { alloc_id: ")?;
        let (alloc, rest) = rest.split_once(", meta: ")?;
        let (meta, ty) = rest.split_once(" }: &")?;
        let count: usize = meta.trim().parse().ok()?;
        let bytes = self.allocs.get(alloc.trim())?;
        let elements = |t: &str| -> Option<String> {
            let elem = t.trim().strip_prefix('[')?.strip_suffix(']')?;
            if array_ty(t.trim()).is_some() {
                return None;
            }
            let (bits, signed) = int_ty(elem.trim(), self.width)?;
            let size = usize::try_from(bits / 8).ok()?;
            (bytes.len() == size * count && (size == 1 || self.big_endian.is_some()))
                .then(|| self.array_value(bytes, bits, signed, size))
        };
        let ty = ty.trim();
        if let Some(v) = elements(ty) {
            return Some(v);
        }
        let (adt, fields, _) = self.rustc_fields(ty)?;
        match fields.as_slice() {
            [(0, field)] if adt => {
                let inner = elements(field)?;
                Some(mir_ctor(
                    "Value.adt",
                    &[
                        mir_string(""),
                        signed(0),
                        term_list(&[inner], &mir_name("Value")),
                    ],
                ))
            }
            _ => None,
        }
    }

    /// The value of type `ty` that allocation `alloc` holds at `offset`: an
    /// integer read at the target's byte order, an array of such values,
    /// or a reference to a sized value another allocation holds.
    fn read_cells(&self, alloc: &str, offset: usize, ty: &str, depth: usize) -> Option<String> {
        let depth = depth.checked_sub(1)?;
        // An allocation another dump prints (a constant's part reads its
        // owner's): rustc numbers a crate's allocations once.
        let cells = self
            .cells
            .get(alloc)
            .or_else(|| self.every_cell.get(alloc))?;
        let ty = ty.trim();
        if let Some((bits, signed)) = int_ty(ty, self.width) {
            let size = usize::try_from(bits / 8).ok()?;
            let bytes: Vec<u8> = cells
                .get(offset..offset + size)?
                .iter()
                .map(|c| match c {
                    Cell::Byte(b) => Some(*b),
                    Cell::Pointer(..) => None,
                })
                .collect::<Option<_>>()?;
            if size > 1 && self.big_endian.is_none() {
                return None;
            }
            return Some(self.int_value(&bytes, bits, signed));
        }
        if let Some((elem, n)) = array_ty(ty) {
            let stride =
                usize::try_from(self.layout(elem, layout_depth(self.structs))?.size).ok()?;
            let items: Vec<String> = (0..n)
                .map(|i| self.read_cells(alloc, offset + i * stride, elem, depth))
                .collect::<Option<_>>()?;
            return Some(mir_ctor(
                "Value.tuple",
                &[term_list(&items, &mir_name("Value"))],
            ));
        }
        // A tuple or a struct rustc lays out: each field read at its
        // offset.
        if let Some((adt, fields, _)) = self.rustc_fields(ty) {
            let items: Vec<String> = fields
                .iter()
                .map(|(o, t)| {
                    let at = offset.checked_add(usize::try_from(*o).ok()?)?;
                    self.read_cells(alloc, at, t, depth)
                })
                .collect::<Option<_>>()?;
            let list = term_list(&items, &mir_name("Value"));
            return Some(if adt {
                mir_ctor("Value.adt", &[mir_string(""), signed(0), list])
            } else {
                mir_ctor("Value.tuple", &[list])
            });
        }
        let inner = ty.strip_prefix('&')?;
        if inner.starts_with("mut ") || self.sized(inner, layout_depth(self.structs)) != Some(true)
        {
            return None;
        }
        let width = usize::try_from(self.width / 8).ok()?;
        let target = match cells.get(offset..offset + width)? {
            [Cell::Pointer(t, 0), rest @ ..]
                if rest
                    .iter()
                    .enumerate()
                    .all(|(i, c)| *c == Cell::Pointer(t.clone(), i + 1)) =>
            {
                t.clone()
            }
            _ => return None,
        };
        let value = self.read_cells(&target, 0, inner, depth)?;
        Some(mir_ctor("Value.reference", &[value]))
    }

    /// `Value.tuple` of the integers `bytes` holds, each `size` bytes.
    fn array_value(&self, bytes: &[u8], bits: u32, signed: bool, size: usize) -> String {
        let items: Vec<String> = bytes
            .chunks(size)
            .map(|chunk| self.int_value(chunk, bits, signed))
            .collect();
        mir_ctor("Value.tuple", &[term_list(&items, &mir_name("Value"))])
    }

    /// `Value.int` of the integer `chunk` holds at the target's byte order.
    fn int_value(&self, chunk: &[u8], bits: u32, signed: bool) -> String {
        let fold = |n: u128, b: &u8| (n << 8) | u128::from(*b);
        let n = if self.big_endian == Some(true) {
            chunk.iter().fold(0, fold)
        } else {
            chunk.iter().rev().fold(0, fold)
        };
        mir_ctor(
            "Value.int",
            &[bits.to_string(), signed.to_string(), n.to_string()],
        )
    }

    /// A constant the evaluator does not read: the construct it answers
    /// with, named as it names it.
    fn unread(&self, c: &IrConstant, e: &mut Embedded) -> String {
        let text = match c {
            IrConstant::Other(t) => t.clone(),
            IrConstant::Int(n, s) => format!("{n}_{s}"),
            _ => String::new(),
        };
        e.blocked
            .insert("Operand::Constant of a constant the embedding does not read".to_string());
        mir_ctor("Const.other", &[mir_string(&text)])
    }

    /// The operand that reads the constant item `key`, which the body
    /// evaluates on entry into a local of its own: that local, or, for a
    /// static, the local past it, which borrows it.
    fn item_operand(&self, key: &str, text: Option<&str>, e: &mut Embedded) -> String {
        if self.externs.contains(key) {
            e.bound.insert(key.to_string());
        } else {
            e.callees.insert(key.to_string());
        }
        if let Some(t) = text {
            self.item_texts
                .borrow_mut()
                .entry(key.to_string())
                .or_insert_with(|| t.to_string());
        }
        let mut items = self.items.borrow_mut();
        let at = items.iter().position(|k| k == key).unwrap_or_else(|| {
            items.push(key.to_string());
            items.len() - 1
        });
        let slot: usize =
            items[..at].iter().map(|k| self.slots(k)).sum::<usize>() + self.slots(key) - 1;
        mir_ctor(
            "Operand.copy",
            &[format!(
                "(record {} {{base := {}, projection := {}}})",
                mir_name("Place"),
                self.base.get() + slot,
                term_list(&[], &mir_name("Proj"))
            )],
        )
    }

    /// The static a thread-local reference names (`&/*tls*/ ID`, MIR's
    /// `Rvalue::ThreadLocalRef`), which MIR prints by its path from the
    /// reading body's module: the static whose path ends so, nearest the
    /// reader's. The evaluator runs one thread, so the thread's instance
    /// of a thread-local is the static its initializer gives.
    fn thread_local(&self, text: &str) -> Option<String> {
        let printed = text.strip_prefix("&/*tls*/ ")?.trim();
        nearest(self.statics, printed, &self.owner.borrow())
    }

    fn operand(&self, o: &IrOperand, e: &mut Embedded) -> String {
        use spec::MirOperand as O;
        let (kind, term) = match o {
            IrOperand::Copy(p) => (O::Copy, mir_ctor("Operand.copy", &[self.place(p, e)])),
            IrOperand::Move(p) => (O::Move, mir_ctor("Operand.move", &[self.place(p, e)])),
            IrOperand::Constant(c) if self.item(c).is_some() => {
                let key = self.item(c).unwrap_or_default();
                let text = match c {
                    IrConstant::Other(t) | IrConstant::Path(t) => Some(t.as_str()),
                    _ => None,
                };
                (O::Constant, self.item_operand(&key, text, e))
            }
            IrOperand::Constant(c) => (
                O::Constant,
                mir_ctor("Operand.constant", &[self.constant(c, e)]),
            ),
            IrOperand::Check(k) => {
                e.uses.insert(k.path().to_string());
                (
                    O::RuntimeChecks,
                    mir_ctor("Operand.check", &[mir_ctor(&kind_ctor(k.path()), &[])]),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    fn operands<'o>(
        &self,
        ops: impl IntoIterator<Item = &'o IrOperand>,
        e: &mut Embedded,
    ) -> String {
        let terms: Vec<String> = ops.into_iter().map(|o| self.operand(o, e)).collect();
        term_list(&terms, &mir_name("Operand"))
    }

    /// An rvalue assigned to a place of type `dest`.
    fn rvalue(&self, r: &IrRvalue, dest: Option<&str>, e: &mut Embedded) -> String {
        use spec::MirRvalue as R;
        let undefined = undefined_constructs();
        let gap = |label: &str, e: &mut Embedded| {
            if undefined.contains(label) {
                e.blocked.insert(label.to_string());
            }
        };
        let (kind, term) = match r {
            IrRvalue::Use(o) => (R::Use, mir_ctor("Rvalue.value", &[self.operand(o, e)])),
            IrRvalue::Ref(k, p) => {
                e.uses.insert(k.path().to_string());
                (
                    R::Ref,
                    mir_ctor(
                        "Rvalue.borrow",
                        &[mir_ctor(&kind_ctor(k.path()), &[]), self.place(p, e)],
                    ),
                )
            }
            IrRvalue::CopyForDeref(p) => (
                R::CopyForDeref,
                mir_ctor("Rvalue.copyForDeref", &[self.place(p, e)]),
            ),
            // An operator on floats is the prelude's, at the operands'
            // format: MIR's operands of one are of one type.
            IrRvalue::Binary(op, a, b)
                if self
                    .operand_ty(a)
                    .or_else(|| self.operand_ty(b))
                    .and_then(|t| self.float(&t))
                    .is_some() =>
            {
                e.uses.insert(op.path().to_string());
                let f = self
                    .operand_ty(a)
                    .or_else(|| self.operand_ty(b))
                    .and_then(|t| self.float(&t))
                    .unwrap_or(crate::mir::float::Format { exp: 0, frac: 0 });
                use spec::MirBinOp as B;
                if !matches!(
                    op,
                    B::Add
                        | B::Sub
                        | B::Mul
                        | B::Div
                        | B::Rem
                        | B::Eq
                        | B::Lt
                        | B::Le
                        | B::Ne
                        | B::Ge
                        | B::Gt
                ) {
                    gap("BinOp on floats other than arithmetic or a comparison", e);
                }
                let (a, b) = (self.operand(a, e), self.operand(b, e));
                (
                    R::BinaryOp,
                    mir_ctor(
                        "Rvalue.floatBinary",
                        &[
                            f.exp.to_string(),
                            f.frac.to_string(),
                            mir_ctor(&kind_ctor(op.path()), &[]),
                            a,
                            b,
                        ],
                    ),
                )
            }
            IrRvalue::Unary(spec::MirUnOp::Neg, a)
                if self.operand_ty(a).and_then(|t| self.float(&t)).is_some() =>
            {
                e.uses.insert(spec::MirUnOp::Neg.path().to_string());
                let f = self
                    .operand_ty(a)
                    .and_then(|t| self.float(&t))
                    .unwrap_or(crate::mir::float::Format { exp: 0, frac: 0 });
                (
                    R::UnaryOp,
                    mir_ctor(
                        "Rvalue.floatNeg",
                        &[f.exp.to_string(), f.frac.to_string(), self.operand(a, e)],
                    ),
                )
            }
            IrRvalue::Binary(op, a, b) => {
                e.uses.insert(op.path().to_string());
                gap(op.path(), e);
                // A variant's discriminant expression, an anonymous constant
                // no dump holds, is the discriminant the enum's source
                // gives it, at the type of the operand beside it.
                let at = |o: &IrOperand, other: &IrOperand| -> IrOperand {
                    match (o, self.operand_ty(other)) {
                        (IrOperand::Constant(IrConstant::Other(t)), Some(ty))
                            if int_ty(ty.trim(), self.width).is_some() =>
                        {
                            self.explicit_discriminant(t).map_or_else(
                                || o.clone(),
                                |d| IrOperand::Constant(IrConstant::Int(d, ty.trim().to_string())),
                            )
                        }
                        _ => o.clone(),
                    }
                };
                let (a, b) = (at(a, b), at(b, a));
                let (a, b) = (self.operand(&a, e), self.operand(&b, e));
                (
                    R::BinaryOp,
                    mir_ctor(
                        "Rvalue.binary",
                        &[mir_ctor(&kind_ctor(op.path()), &[]), a, b],
                    ),
                )
            }
            IrRvalue::Unary(op, a) => {
                e.uses.insert(op.path().to_string());
                gap(op.path(), e);
                (
                    R::UnaryOp,
                    mir_ctor(
                        "Rvalue.unary",
                        &[mir_ctor(&kind_ctor(op.path()), &[]), self.operand(a, e)],
                    ),
                )
            }
            IrRvalue::Discriminant(p) => {
                let ty = self.ty(dest.unwrap_or_default());
                if ty.contains("Ty.other") {
                    gap(
                        "Rvalue::Discriminant at a type the embedding does not read",
                        e,
                    );
                }
                (
                    R::Discriminant,
                    mir_ctor("Rvalue.discriminant", &[self.place(p, e), ty]),
                )
            }
            IrRvalue::Aggregate(a, ops) => {
                use spec::MirAggregateKind as A;
                let (k, agg) = match a {
                    IrAggregate::Tuple => (A::Tuple, mir_ctor("Agg.tuple", &[])),
                    IrAggregate::Array => (A::Array, mir_ctor("Agg.array", &[])),
                    IrAggregate::Adt(ty, variant) => {
                        let v = variant.clone().unwrap_or_default();
                        // An enum MIR names by its bare name where several
                        // share it is the one the local it builds is of.
                        let named = |t: &str| last_segment(t) == last_segment(ty);
                        let at = |v: &str| {
                            self.discriminant(ty, v).or_else(|| {
                                dest.filter(|t| named(t))
                                    .and_then(|t| self.discriminant(t, v))
                            })
                        };
                        let d = variant.as_deref().and_then(at).unwrap_or_default();
                        if variant.is_some() && at(&v).is_none() {
                            e.blocked.insert(
                                "AggregateKind::Adt of an enum whose discriminants the crate's source does not give"
                                    .to_string(),
                            );
                        }
                        (A::Adt, mir_ctor("Agg.adt", &[mir_string(&v), signed(d)]))
                    }
                    IrAggregate::Closure(t) => {
                        (A::Closure, mir_ctor("Agg.closure", &[mir_string(t)]))
                    }
                    // A coroutine is built unresumed: rustc's variant 0
                    // of its state (`CoroutineArgs::UNRESUMED`), holding
                    // its upvars.
                    IrAggregate::Coroutine(t) => {
                        (A::Coroutine, mir_ctor("Agg.coroutine", &[mir_string(t)]))
                    }
                };
                e.uses.insert(k.path().to_string());
                (
                    R::Aggregate,
                    mir_ctor(
                        "Rvalue.aggregate",
                        &[agg, self.operands(ops.iter().map(|(_, o)| o), e)],
                    ),
                )
            }
            // A cast from or to a float, between the types the operand's
            // and the cast's print.
            IrRvalue::Cast(k, o, ty)
                if matches!(
                    k,
                    spec::MirCastKind::FloatToInt
                        | spec::MirCastKind::IntToFloat
                        | spec::MirCastKind::FloatToFloat
                ) && self.numeric(o, ty).is_some() =>
            {
                e.uses.insert(k.path().to_string());
                let (source, target) = self.numeric(o, ty).unwrap_or_default();
                (
                    R::Cast,
                    mir_ctor("Rvalue.numeric", &[source, target, self.operand(o, e)]),
                )
            }
            // A transmute: the operand's bytes at its type's layout read
            // at the target's, as the `transmute` intrinsic is.
            IrRvalue::Cast(spec::MirCastKind::Transmute, o, ty)
                if self
                    .operand_ty(o)
                    .and_then(|f| self.transmute_layouts(&f, ty))
                    .is_some() =>
            {
                e.uses
                    .insert(spec::MirCastKind::Transmute.path().to_string());
                let (from, to) = self
                    .operand_ty(o)
                    .and_then(|f| self.transmute_layouts(&f, ty))
                    .unwrap_or_default();
                (
                    R::Cast,
                    mir_ctor("Rvalue.transmute", &[from, to, self.operand(o, e)]),
                )
            }
            IrRvalue::Cast(k, o, ty) => {
                e.uses.insert(k.path().to_string());
                gap(k.path(), e);
                let ty = self.ty(ty);
                if *k == spec::MirCastKind::IntToInt && ty.contains("Ty.other") {
                    gap("CastKind::IntToInt to a type that is not an integer", e);
                }
                (
                    R::Cast,
                    mir_ctor(
                        "Rvalue.cast",
                        &[mir_ctor(&kind_ctor(k.path()), &[]), self.operand(o, e), ty],
                    ),
                )
            }
            IrRvalue::RawPtr(_, p) | IrRvalue::MetadataPtr(p) => {
                (R::RawPtr, mir_ctor("Rvalue.rawPtr", &[self.place(p, e)]))
            }
            // A unit variant or unit struct whose generic arguments the
            // parser does not read as a path (`Option::<&[T]>::None`).
            IrRvalue::Other(spec::MirRvalue::Use, text) if self.unit_adt(text).is_some() => {
                let (v, d) = self.unit_adt(text).unwrap_or_default();
                e.uses
                    .insert(spec::MirAggregateKind::Adt.path().to_string());
                (
                    R::Aggregate,
                    mir_ctor(
                        "Rvalue.aggregate",
                        &[
                            mir_ctor("Agg.adt", &[mir_string(&v), signed(d)]),
                            term_list(&[], &mir_name("Operand")),
                        ],
                    ),
                )
            }
            // `[x; n]`: the operand and the count.
            // A function item reified to a pointer, which MIR prints by its
            // path bare (`f::<T> as fn(*mut u8) (PointerCoercion(..))`):
            // the cast of the item's constant.
            IrRvalue::Other(R::Cast, text) if fn_item_cast(text).is_some() => {
                let (path, ty) = fn_item_cast(text).unwrap_or_default();
                let cast = IrRvalue::Cast(
                    spec::MirCastKind::PointerCoercion,
                    IrOperand::Constant(IrConstant::Path(path)),
                    ty,
                );
                return self.rvalue(&cast, dest, e);
            }
            // A thread-local's reference: the static's, as an operand
            // naming the static reads it.
            IrRvalue::Other(R::Ref, text) if self.thread_local(text).is_some() => {
                let key = self.thread_local(text).unwrap_or_default();
                e.uses
                    .insert(spec::MirRvalue::ThreadLocalRef.path().to_string());
                (
                    R::ThreadLocalRef,
                    mir_ctor("Rvalue.value", &[self.item_operand(&key, None, e)]),
                )
            }
            IrRvalue::Other(spec::MirRvalue::Repeat, text)
                if repeat_parts(text).is_some_and(|(o, n)| {
                    crate::mir::parse::operand(o).is_some() && self.generic_count(n).is_some()
                }) =>
            {
                let (o, n) = repeat_parts(text).unwrap_or_default();
                let operand = crate::mir::parse::operand(o).map(|o| self.operand(&o, e));
                (
                    spec::MirRvalue::Repeat,
                    mir_ctor(
                        "Rvalue.repeat",
                        &[
                            operand.unwrap_or_default(),
                            self.generic_count(n).unwrap_or_default(),
                        ],
                    ),
                )
            }
            IrRvalue::Other(k, text) => {
                e.blocked.insert(k.path().to_string());
                (
                    *k,
                    mir_ctor("Rvalue.other", &[mir_string(k.path()), mir_string(text)]),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    fn statement(&self, s: &IrStatement, body: &IrBody, e: &mut Embedded) -> String {
        use spec::MirStatementKind as S;
        let (kind, term) = match s {
            IrStatement::Assign(p, r) => {
                let dest = p
                    .projection
                    .is_empty()
                    .then(|| body.locals.get(p.local).map(String::as_str))
                    .flatten();
                let place = self.place(p, e);
                (
                    S::Assign,
                    mir_ctor("Statement.assign", &[place, self.rvalue(r, dest, e)]),
                )
            }
            IrStatement::Marker(k) => (*k, mir_ctor("Statement.marker", &[mir_string(k.path())])),
            IrStatement::Assume(o) => {
                e.uses
                    .insert(spec::MirNonDivergingIntrinsic::Assume.path().to_string());
                (
                    S::Intrinsic,
                    mir_ctor("Statement.assume", &[self.operand(o, e)]),
                )
            }
            IrStatement::Other(k, text) => {
                e.blocked.insert(k.path().to_string());
                (
                    *k,
                    mir_ctor("Statement.other", &[mir_string(k.path()), mir_string(text)]),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    fn terminator(
        &self,
        t: &IrTerminator,
        span: Option<&crate::mir::ir::IrSpan>,
        body: &IrBody,
        e: &mut Embedded,
    ) -> String {
        use spec::MirTerminatorKind as T;
        let (kind, term) = match t {
            IrTerminator::Goto(b) => (T::Goto, mir_ctor("Terminator.jump", &[b.to_string()])),
            IrTerminator::False(k, b) => (*k, mir_ctor("Terminator.jump", &[b.to_string()])),
            IrTerminator::Switch(o, arms, otherwise) => {
                let arms: Vec<String> = arms
                    .iter()
                    .map(|(v, b)| {
                        // A switch's values are printed as the bits of the
                        // operand's type, which are never negative.
                        format!(
                            "(record {} {{value := {}, target := {b}}})",
                            mir_name("Arm"),
                            repr(*v, u128::BITS)
                        )
                    })
                    .collect();
                (
                    T::SwitchInt,
                    mir_ctor(
                        "Terminator.switch",
                        &[
                            self.operand(o, e),
                            term_list(&arms, &mir_name("Arm")),
                            otherwise.to_string(),
                        ],
                    ),
                )
            }
            IrTerminator::Return => (T::Return, mir_ctor("Terminator.exit", &[])),
            IrTerminator::Unreachable => (T::Unreachable, mir_ctor("Terminator.unreachable", &[])),
            IrTerminator::Drop(b) => (T::Drop, mir_ctor("Terminator.drop", &[b.to_string()])),
            IrTerminator::Call {
                destination,
                callee,
                args,
                target,
            } => {
                let mut typed: Option<String> = None;
                let callee = match crate::mir::parse::operand(callee) {
                    Some(o @ (IrOperand::Copy(_) | IrOperand::Move(_))) => {
                        mir_ctor("Callee.operand", &[self.operand(&o, e)])
                    }
                    _ => match (self.resolve)(
                        callee,
                        &args.iter().map(|a| arg_type(body, a)).collect::<Vec<_>>(),
                        &place_type(body, destination).unwrap_or_default(),
                    ) {
                        Callee::Body(p) => {
                            e.callees.insert(p.clone());
                            let token = format!(
                                "\u{1}{}#{}#{p}#{callee}\u{1}",
                                self.key.borrow(),
                                self.sites.borrow().len()
                            );
                            self.sites.borrow_mut().push(Site {
                                token: token.clone(),
                                callee: p,
                                text: callee.clone(),
                                args: args.iter().map(|a| arg_type(body, a)).collect(),
                                dest: place_type(body, destination).unwrap_or_default(),
                                constant: false,
                                block: Some(self.block.get()),
                            });
                            token
                        }
                        // `caller_location`: a reference to the `Location` of
                        // the call the frame's caller passes, laid out as
                        // rustc lays out `Location`, its fields found by
                        // their declared names.
                        Callee::Intrinsic(p)
                            if intrinsic_name(&p) == "caller_location"
                                && place_type(body, destination)
                                    .and_then(|t| self.caller_location(&t))
                                    .is_some() =>
                        {
                            place_type(body, destination)
                                .and_then(|t| self.caller_location(&t))
                                .unwrap_or_default()
                        }
                        // `float_to_int_unchecked`, from its argument's float
                        // type to its destination's integer type.
                        Callee::Intrinsic(p)
                            if intrinsic_name(&p) == "float_to_int_unchecked"
                                && args
                                    .first()
                                    .and_then(|a| self.operand_ty(a))
                                    .zip(place_type(body, destination))
                                    .is_some_and(|(f, t)| {
                                        self.float(&f).is_some()
                                            && int_ty(t.trim(), self.width).is_some()
                                    }) =>
                        {
                            let from = args
                                .first()
                                .and_then(|a| self.operand_ty(a))
                                .unwrap_or_default();
                            let to = place_type(body, destination).unwrap_or_default();
                            mir_ctor("Callee.convert", &[self.ty(&from), self.ty(&to)])
                        }
                        // A float intrinsic: its operation at its first
                        // argument's float format.
                        Callee::Intrinsic(p) if self.float_intrinsic(&p, args).is_some() => {
                            let (format, op) = self.float_intrinsic(&p, args).unwrap_or_default();
                            mir_ctor(
                                "Callee.float",
                                &[
                                    format.exp.to_string(),
                                    format.frac.to_string(),
                                    mir_string(&op),
                                ],
                            )
                        }
                        Callee::Intrinsic(p)
                            if matches!(
                                intrinsic_name(&p).as_str(),
                                "transmute" | "transmute_unchecked"
                            ) =>
                        {
                            let from = args.first().and_then(|a| arg_type(body, a));
                            let to = place_type(body, destination);
                            match from.zip(to).and_then(|(f, t)| self.transmute(&f, &t)) {
                                Some(term) => term,
                                None => {
                                    e.blocked.insert(
                                        "intrinsic transmute of a type whose layout the embedding does not read"
                                            .to_string(),
                                    );
                                    mir_ctor("Callee.intrinsic", &[mir_string(&intrinsic_name(&p))])
                                }
                            }
                        }
                        // `write_bytes::<T>`: its element's layout, from the
                        // type argument the call prints.
                        // `size_of::<T>`: its type argument's size, read
                        // from the type argument the call prints; and
                        // `size_of_val::<T>` of a `T` laid out, which is
                        // sized, whose value's size is its type's.
                        Callee::Intrinsic(p)
                            if matches!(intrinsic_name(&p).as_str(), "size_of" | "size_of_val")
                                && type_argument(callee)
                                    .and_then(|t| self.generic_layout(&t))
                                    .is_some() =>
                        {
                            let t = type_argument(callee).unwrap_or_default();
                            mir_ctor(
                                "Callee.sizeOf",
                                &[self.generic_layout(&t).unwrap_or_default()],
                            )
                        }
                        // `field_offset::<F>`, `assert_zero_valid::<T>`: a
                        // count rustc's layout data gives of the type
                        // argument, or the slot its instance's fills.
                        Callee::Intrinsic(p)
                            if TYPE_QUERIES.contains(&intrinsic_name(&p).as_str())
                                && defined_layout_queries().contains(&intrinsic_name(&p))
                                && type_argument(callee)
                                    .and_then(|t| {
                                        self.generic_count(&format!(
                                            "{}::<{t}>",
                                            intrinsic_name(&p)
                                        ))
                                    })
                                    .is_some() =>
                        {
                            let p = intrinsic_name(&p);
                            let t = type_argument(callee).unwrap_or_default();
                            mir_ctor(
                                "Callee.layoutQuery",
                                &[
                                    mir_string(&p),
                                    self.generic_count(&format!("{p}::<{t}>"))
                                        .unwrap_or_default(),
                                ],
                            )
                        }
                        // `align_of::<T>`, `assert_inhabited::<T>`: a query
                        // of its type argument's layout.
                        // `align_of_val::<T>` of a `T` laid out, as
                        // `align_of::<T>`, as `size_of_val` is `size_of`.
                        Callee::Intrinsic(p)
                            if defined_layout_queries().contains(&layout_query(&p))
                                && type_argument(callee)
                                    .and_then(|t| self.generic_layout(&t))
                                    .is_some() =>
                        {
                            let t = type_argument(callee).unwrap_or_default();
                            mir_ctor(
                                "Callee.layoutQuery",
                                &[
                                    mir_string(&layout_query(&p)),
                                    self.generic_layout(&t).unwrap_or_default(),
                                ],
                            )
                        }
                        // `raw_eq::<T>`: its two values' bytes at `T`'s
                        // layout, read from the type argument the call
                        // prints.
                        Callee::Intrinsic(p)
                            if intrinsic_name(&p) == "raw_eq"
                                && type_argument(callee)
                                    .and_then(|t| self.generic_layout(&t))
                                    .is_some() =>
                        {
                            let t = type_argument(callee).unwrap_or_default();
                            mir_ctor(
                                "Callee.rawEq",
                                &[self.generic_layout(&t).unwrap_or_default()],
                            )
                        }
                        // `ptr_metadata::<T, M>`: the pointer's metadata,
                        // of `M`'s layout: `()` of a thin pointer, or a
                        // slice's length.
                        Callee::Intrinsic(p)
                            if intrinsic_name(&p) == "ptr_metadata"
                                && metadata_argument(callee)
                                    .and_then(|m| self.generic_layout(&m))
                                    .is_some() =>
                        {
                            let m = metadata_argument(callee).unwrap_or_default();
                            mir_ctor(
                                "Callee.metadata",
                                &[self.generic_layout(&m).unwrap_or_default()],
                            )
                        }
                        Callee::Intrinsic(p)
                            if intrinsic_name(&p) == "write_bytes"
                                && self.written_element(body, args).is_some() =>
                        {
                            let element = self.written_element(body, args).unwrap_or_default();
                            mir_ctor("Callee.writeBytes", &[element])
                        }
                        Callee::Intrinsic(p) => {
                            let p = intrinsic_name(&p);
                            if !defined_intrinsics().contains(&p) {
                                e.intrinsics.insert(p.clone());
                                e.blocked.insert(format!("intrinsic {p}"));
                            }
                            // A case of a valid call the memory model does
                            // not yet place blocks the call, by its cause.
                            for gap in intrinsic_gaps(&p) {
                                e.blocked.insert(gap);
                            }
                            typed = Some(p.clone());
                            mir_ctor("Callee.intrinsic", &[mir_string(&p)])
                        }
                        Callee::Bound(n) => {
                            e.bound.insert(n.clone());
                            mir_ctor("Callee.bound", &[mir_string(&n)])
                        }
                    },
                };
                // An intrinsic whose result's type its arguments do not
                // give (`discriminant_value`) is passed it last, as an
                // integer of that type.
                let mut args = args.clone();
                let dest_ty = place_type(body, destination).unwrap_or_default();
                if typed.as_deref() == Some("discriminant_value")
                    && int_ty(&dest_ty, self.width).is_some()
                {
                    args.push(IrOperand::Constant(IrConstant::Int(0, dest_ty)));
                }
                let target = match target {
                    Some(b) => format!("(ctor Option.some [Nat] {b})"),
                    None => "(ctor Option.none [Nat])".to_string(),
                };
                let destination = self.place(destination, e);
                let args = self.operands(&args, e);
                let site = span
                    .filter(|s| self.declared.borrow().reads(s))
                    .map_or_else(no_span, span_term);
                (
                    T::Call,
                    mir_ctor(
                        "Terminator.call",
                        &[destination, callee, args, target, site],
                    ),
                )
            }
            IrTerminator::Assert {
                condition,
                expected,
                kind,
                target,
            } => {
                e.uses.insert(kind.path().to_string());
                (
                    T::Assert,
                    mir_ctor(
                        "Terminator.assert",
                        &[
                            self.operand(condition, e),
                            expected.to_string(),
                            mir_ctor(&kind_ctor(kind.path()), &[]),
                            target.to_string(),
                        ],
                    ),
                )
            }
            // Inline assembly is the platform's, outside the abstract
            // machine: a call the program leaves to its dictionary, as a
            // foreign function and `va_arg` are, named by the assembly and
            // its operands' registers, with its inputs as arguments and its
            // one output as the result; it stops unlinked without it.
            IrTerminator::Other(k, text) => match (k, inline_asm(text)) {
                // Inline assembly is the platform's, outside the abstract
                // machine: a call the program leaves to its dictionary, as
                // a foreign function and `va_arg` are, named by the
                // assembly and its operands' registers, with its inputs as
                // arguments and its one output as the result; it stops
                // unlinked without it.
                (spec::MirTerminatorKind::InlineAsm, Some(asm)) => {
                    e.bound.insert(asm.name.clone());
                    let target = match asm.target {
                        Some(b) => format!("(ctor Option.some [Nat] {b})"),
                        None => "(ctor Option.none [Nat])".to_string(),
                    };
                    let destination = self.place(&asm.output, e);
                    let args = self.operands(&asm.inputs, e);
                    (
                        T::Call,
                        mir_ctor(
                            "Terminator.call",
                            &[
                                destination,
                                mir_ctor("Callee.bound", &[mir_string(&asm.name)]),
                                args,
                                target,
                                no_span(),
                            ],
                        ),
                    )
                }
                _ => {
                    e.blocked.insert(k.path().to_string());
                    (
                        *k,
                        mir_ctor(
                            "Terminator.other",
                            &[mir_string(k.path()), mir_string(text)],
                        ),
                    )
                }
            },
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    /// The body `b`, named `path` in the program, as a `Mir::Body` term.
    /// What one run of `b` costs the evaluator at most ([`Cost`]): its
    /// blocks control `reached` each run at most its loops' bounds, its
    /// constant `items` evaluated once, its `sites`' callees called from
    /// their blocks, its rendered `blocks` read for the transmutes in them.
    fn cost(
        &self,
        b: &IrBody,
        reached: &BTreeSet<usize>,
        items: &[String],
        sites: &[Site],
        blocks: &[String],
    ) -> Cost {
        let runs = match crate::mir::trips::runs(b, reached, self.width) {
            crate::mir::trips::Runs::Bounded(r) => r,
            crate::mir::trips::Runs::Unbounded(why) => return Cost::Unbounded(why),
        };
        let pointer_call = reached.iter().any(|&i| {
            matches!(&b.blocks[i].terminator, IrTerminator::Call { callee, .. }
                if matches!(crate::mir::parse::operand(callee), Some(IrOperand::Copy(_) | IrOperand::Move(_))))
        });
        if pointer_call {
            return Cost::Unbounded("calls a function pointer, whose callee bounds no loop".into());
        }
        let run = |i: usize| if reached.contains(&i) { runs[i] } else { 0 };
        // A constant item's block is one step, its call; a static's borrow
        // is a block of a statement and a jump past it.
        let entry: u128 = items
            .iter()
            .map(|k| if self.slots(k) > 1 { 3 } else { 1 })
            .sum();
        let steps = (0..b.blocks.len())
            .map(|i| run(i).saturating_mul(b.blocks[i].statements.len() as u128 + 1))
            .fold(entry, u128::saturating_add);
        let calls = sites
            .iter()
            .map(|s| (s.callee.clone(), s.block.map_or(1, run), s.constant))
            .collect();
        let mut work = 0u128;
        for block in blocks {
            for name in [
                "Rvalue.transmute",
                "Callee.transmute",
                "Callee.rawEq",
                "Callee.writeBytes",
            ] {
                let needle = format!("(ctor {} ", mir_name(name));
                for (at, _) in block.match_indices(&needle) {
                    match transmute_work(&block[at..]) {
                        Some(w) => work = work.max(w),
                        None => {
                            return Cost::Unbounded(
                                "transmutes at a layout its instance supplies, whose size no bound reads"
                                    .into(),
                            )
                        }
                    }
                }
            }
        }
        Cost::Bounded {
            steps,
            calls,
            work,
            pin: b.locals.first().and_then(|t| self.pin_work(t)),
        }
    }

    /// The fuel pinning a value of `ty` takes ([`Cost`]): a step for each
    /// value in it and one more to rebuild each value it holds others in,
    /// and one to end. A pointer in it is followed, so its pointee's size
    /// is read where the value is a reference to a value of no pointer.
    fn pin_work(&self, ty: &str) -> Option<u128> {
        let depth = layout_depth(self.structs);
        let nodes = |ty: &str| -> Option<u128> {
            let laid = self.layout(ty, depth).filter(|l| l.assumed.is_none())?;
            if laid.term.contains(&mir_name("Layout.pointer")) {
                return None;
            }
            layout_pieces(&sexp(&laid.term)?)
        };
        let count = match ty.strip_prefix("&mut ").or_else(|| ty.strip_prefix('&')) {
            Some(inner) => nodes(inner.trim_start_matches("'static "))?.checked_add(1)?,
            None => nodes(ty)?,
        };
        count.checked_mul(2)?.checked_add(1)
    }

    pub(crate) fn body(&self, b: &IrBody, path: &str) -> Embedded {
        let mut e = Embedded::default();
        self.items.borrow_mut().clear();
        self.item_texts.borrow_mut().clear();
        self.slots.borrow_mut().clear();
        self.sites.borrow_mut().clear();
        self.key.replace(path.to_string());
        self.owner.replace(b.path.clone());
        self.declared.replace(self.sources.declared(b));
        self.locals.replace(b.locals.clone());
        self.base.set(b.locals.len());
        let mut locals: Vec<String> = b.locals.iter().map(|t| self.ty(t)).collect();
        // The blocks control reaches from the entry without unwinding: the
        // evaluator ends a run at a panic, so a cleanup block is never
        // evaluated, and what it uses blocks nothing.
        // MIR prints a call's only edge bare, so a diverging call's unwind
        // edge reads as its return: a block every path from which resumes
        // unwinding (or terminates it) is a cleanup block, and is not followed.
        let resumes = |t: &IrTerminator| matches!(t, IrTerminator::Other(k, _) if matches!(k, spec::MirTerminatorKind::UnwindResume | spec::MirTerminatorKind::UnwindTerminate));
        let mut cleanup: BTreeSet<usize> = BTreeSet::new();
        loop {
            let before = cleanup.len();
            for (i, block) in b.blocks.iter().enumerate() {
                let next = block.terminator.successors();
                if resumes(&block.terminator)
                    || (!next.is_empty()
                        && matches!(
                            block.terminator,
                            IrTerminator::Goto(_) | IrTerminator::Drop(_)
                        )
                        && next.iter().all(|n| cleanup.contains(n)))
                {
                    cleanup.insert(i);
                }
            }
            if cleanup.len() == before {
                break;
            }
        }
        let mut reached = BTreeSet::from([0]);
        let mut work = vec![0];
        while let Some(at) = work.pop() {
            let named = |c: &IrConstant| self.named_scalar(c);
            for next in b
                .blocks
                .get(at)
                .map(|block| live_successors(block, &named))
                .unwrap_or_default()
            {
                if !cleanup.contains(&next) && reached.insert(next) {
                    work.push(next);
                }
            }
        }
        let blocks: Vec<String> = b
            .blocks
            .iter()
            .enumerate()
            .map(|(i, block)| {
                self.block.set(i);
                let mut cleanup = Embedded::default();
                let e = if reached.contains(&i) {
                    &mut e
                } else {
                    &mut cleanup
                };
                let statements: Vec<String> = block
                    .statements
                    .iter()
                    .map(|s| self.statement(s, b, e))
                    .collect();
                format!(
                    "(record {} {{statements := {}, terminator := {}}})",
                    mir_name("Block"),
                    term_list(&statements, &mir_name("Statement")),
                    self.terminator(&block.terminator, block.span.as_ref(), b, e)
                )
            })
            .collect();
        // Each constant item the body reads is evaluated on entry, in a
        // block of its own past the body's, into a local of its own: rustc
        // evaluates every constant a body requires before it runs.
        let mut blocks = blocks;
        // A static is evaluated so too, and borrowed into the local past
        // its value's, in a block of its own after its call's.
        let items = self.items.borrow().clone();
        let entry = if items.is_empty() { 0 } else { blocks.len() };
        let total: usize = items.iter().map(|k| self.slots(k)).sum();
        let place = |local: usize| {
            format!(
                "(record {} {{base := {local}, projection := {}}})",
                mir_name("Place"),
                term_list(&[], &mir_name("Proj"))
            )
        };
        let mut at = 0;
        for key in &items {
            let slots = self.slots(key);
            let value = b.locals.len() + at;
            at += slots;
            let next = if at < total { entry + at } else { 0 };
            for _ in 0..slots {
                locals.push(mir_ctor("Ty.other", &[mir_string("const")]));
            }
            let after = if slots > 1 { entry + at - 1 } else { next };
            blocks.push(format!(
                "(record {} {{statements := {}, terminator := {}}})",
                mir_name("Block"),
                term_list(&[], &mir_name("Statement")),
                mir_ctor(
                    "Terminator.call",
                    &[
                        place(value),
                        // An extern static's value is the platform's,
                        // which the dictionary binds by its path.
                        if self.externs.contains(key) {
                            mir_ctor("Callee.bound", &[mir_string(key)])
                        } else {
                            self.constant_site(key)
                        },
                        term_list(&[], &mir_name("Operand")),
                        format!("(ctor Option.some [Nat] {after})"),
                        no_span(),
                    ],
                )
            ));
            if slots > 1 {
                // A static is the program's global: its initial value is
                // written there once, and the body reads it through a
                // pointer to that global. An extern static is read through
                // a pointer to the platform's value.
                let borrow = match self.globals.iter().position(|g| g == key) {
                    Some(index) => mir_ctor(
                        "Statement.global",
                        &[index.to_string(), place(value), place(value + 1)],
                    ),
                    None => mir_ctor(
                        "Statement.assign",
                        &[place(value + 1), mir_ctor("Rvalue.rawPtr", &[place(value)])],
                    ),
                };
                blocks.push(format!(
                    "(record {} {{statements := {}, terminator := {}}})",
                    mir_name("Block"),
                    term_list(&[borrow], &mir_name("Statement")),
                    mir_ctor("Terminator.jump", &[next.to_string()])
                ));
            }
        }
        e.slots = self.slots.take();
        e.key = path.to_string();
        e.sites = self.sites.take();
        e.cost = self.cost(b, &reached, &items, &e.sites, &blocks);
        e.term = format!(
            "(record {} {{path := {}, arity := {}, locals := {}, blocks := {}, entry := {entry}, tracked := {}}})",
            mir_name("Body"),
            mir_string(path),
            b.arity,
            term_list(&locals, &mir_name("Ty")),
            term_list(&blocks, &mir_name("Block")),
            match self.declared.borrow().tracked {
                Some(t) => format!("(ctor Option.some [Bool] {t})"),
                None => "(ctor Option.none [Bool])".to_string(),
            }
        );
        e
    }
}

/// Each enum core's source declares, by its name's last segment, with its
/// variants' discriminants: those its declaration sets, else each
/// variant's index.
pub(crate) fn enums(decls: &crate::mir::core::CoreDecls) -> Enums {
    let mut out: Enums = BTreeMap::new();
    for s in decls.structs.iter().chain(&decls.locals) {
        let names: Vec<String> = match (&s.variants, &s.data) {
            (Some(v), _) => v.clone(),
            (None, Some(d)) => d.iter().map(|(v, _)| v.clone()).collect(),
            (None, None) => continue,
        };
        let values: Vec<i128> = match &s.values {
            Some(vs) if vs.len() == names.len() => vs
                .iter()
                .map(|v| crate::mir::core::discriminant(*v, *v < 0))
                .collect(),
            _ => (0..).take(names.len()).collect(),
        };
        let last = crate::mir::parse::split_path(&s.path)
            .last()
            .copied()
            .unwrap_or(&s.path)
            .to_string();
        out.entry(last)
            .or_default()
            .push((s.path.clone(), names.into_iter().zip(values).collect()));
    }
    out
}

/// The crates a library crate's root mounts as modules of its own, by
/// `#[path = "../../<crate>/src/lib.rs"] mod <module>;`: each module's
/// name and the crate's directory under `library`.
fn mounted_crates(root: &Path, krate: &str) -> Vec<(String, String)> {
    let package = library_root(root).join(krate);
    let lib = crate::layouts::library_file(&package).map(|l| package.join(l));
    let text = lib.and_then(|l| read_text(&l)).unwrap_or_default();
    let mut out = Vec::new();
    let mut mounted: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#[path = \"../../") {
            mounted = rest
                .strip_suffix("/src/lib.rs\"]")
                .filter(|c| !c.contains('/'))
                .map(str::to_string);
            continue;
        }
        if line.starts_with("#[") {
            continue;
        }
        if let Some(c) = mounted.take() {
            let module = line
                .trim_start_matches("pub ")
                .strip_prefix("mod ")
                .and_then(|m| m.strip_suffix(';'));
            if let Some(m) = module {
                out.push((m.to_string(), c));
            }
        }
    }
    out
}

/// The name of a body's definition: `b` and the first half of its κ.
pub(crate) fn def_name(kappa: &str) -> String {
    format!("b{}", &kappa[..kappa.len() / 2])
}

/// What `uorust embed` writes and measures.
#[derive(Clone, Debug, Default)]
pub struct EmbedCensus {
    /// The bodies embedded.
    pub bodies: usize,
    /// Those whose every construct the evaluator defines.
    pub evaluable: usize,
    /// For each construct, how many bodies use it.
    pub uses: BTreeMap<String, usize>,
    /// For each construct the evaluator does not define, how many bodies it
    /// blocks.
    pub blocked: BTreeMap<String, usize>,
    /// How many bodies leave a callee to a dictionary.
    pub bound: usize,
}

impl EmbedCensus {
    fn add(&mut self, e: &Embedded) {
        self.bodies += 1;
        if e.blocked.is_empty() {
            self.evaluable += 1;
        }
        for u in &e.uses {
            *self.uses.entry(u.clone()).or_default() += 1;
        }
        for b in &e.blocked {
            *self.blocked.entry(b.clone()).or_default() += 1;
        }
        if !e.bound.is_empty() {
            self.bound += 1;
        }
    }

    /// The census as TOML: the counts, then each blocking construct, most
    /// bodies first, then each construct's uses.
    pub fn render(&self) -> String {
        let mut out = format!(
            "# The embedding's census (written by `uorust embed`; do not edit): every\n\
             # body embedded, those whose every construct the evaluator defines, and\n\
             # the constructs it does not, by the bodies each blocks.\n\n\
             bodies = {}\nevaluable = {}\nbound = {}\n",
            self.bodies, self.evaluable, self.bound
        );
        let mut ranked: Vec<(&String, &usize)> = self.blocked.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (c, n) in ranked {
            let _ = write!(
                out,
                "\n[[blocked]]\nconstruct = {}\nbodies = {n}\n",
                census_string(c)
            );
        }
        for (c, n) in &self.uses {
            let _ = write!(
                out,
                "\n[[construct]]\nconstruct = {}\nbodies = {n}\n",
                census_string(c)
            );
        }
        out
    }
}

fn census_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// The LexLean project of the embedding: `Mir` and, for each module, its
/// bodies in parts of at most `budget` bytes, each a definition named by
/// its κ ([`def_name`]), and the `extra` modules, each an entrypoint.
/// The crate's modules sit under `{Crate}Mir`, a segment no crate name
/// can make a Lean namespace.
pub(crate) fn embed_project(
    krate: &str,
    modules: &[(String, Vec<(String, Embedded)>)],
    extra: BTreeMap<String, Vec<u8>>,
    budget: usize,
) -> BTreeMap<String, Vec<u8>> {
    let mut entrypoints: Vec<String> = extra.keys().cloned().collect();
    let mut files: BTreeMap<String, Vec<u8>> = extra;
    files.insert(format!("src/{MIR}.lex.tex"), SEMANTICS.as_bytes().to_vec());
    // The evaluator's floats are the prelude's one IEEE 754 model, which
    // the engine's own project carries with the glossary its titles are
    // words of.
    files.insert(
        format!("src/{}.lex.tex", crate::lift::PRELUDE),
        crate::lift::PRELUDE_SOURCE.as_bytes().to_vec(),
    );
    for (path, text) in crate::assets::FIXED
        .iter()
        .filter(|(p, _)| p.starts_with("language/uorust.titles/"))
    {
        files.insert((*path).to_string(), text.as_bytes().to_vec());
    }
    files.insert(
        "language/uorust.mir/lexicon.toml".into(),
        LEXICON.as_bytes().to_vec(),
    );
    for (name, text) in ENTRIES {
        files.insert(
            format!("language/uorust.mir/entries/{name}"),
            text.as_bytes().to_vec(),
        );
    }
    entrypoints.push(format!("src/{MIR}.lex.tex"));
    entrypoints.push(format!("src/{}.lex.tex", crate::lift::PRELUDE));
    let crate_pascal = format!("{}{MIR}", crate::lex::pascal(&[krate]));
    for (module, bodies) in modules {
        let pascal = crate::lex::pascal(&[module]);
        let mut parts: Vec<Vec<String>> = vec![Vec::new()];
        // A part's file is its definitions and the module's frame around
        // them, whose name's number is at most the bodies' count.
        let frame = |k: usize, decls: &str| {
            format!(
                "\\begin{{lexlean}}{{{crate_pascal}.{pascal}Part{k}}}\n\\useglossary{{lexlean.std.bool@1.2.0}}\n\
                 \\useglossary{{lexlean.std.nat@1.2.0}}\n\\useglossary{{uorust.mir@1.0.0}}\n\
                 \\importmodule{{{MIR}}}\n\\importmodule{{Prelude}}\n\\title{{Embedded MIR bodies}}\n\\begin{{semanticmodule}}\n{decls}\n\
                 \\end{{semanticmodule}}\n\\end{{lexlean}}\n"
            )
        };
        let overhead = frame(bodies.len(), "").len();
        let mut size = overhead;
        for (kappa, e) in bodies {
            let decl = format!(
                "definition{{\n  name: {},\n  parameters: [],\n  result: {},\n  body: {}\n}}",
                def_name(kappa),
                mir_name("Body"),
                e.term
            );
            if size + decl.len() > budget && !parts.last().is_some_and(Vec::is_empty) {
                parts.push(Vec::new());
                size = overhead;
            }
            size += decl.len() + 2;
            if let Some(last) = parts.last_mut() {
                last.push(decl);
            }
        }
        for (k, decls) in parts.iter().enumerate().filter(|(_, d)| !d.is_empty()) {
            let path = format!("src/{crate_pascal}/{pascal}Part{k}.lex.tex");
            let text = frame(k, &decls.join("\n\n"));
            entrypoints.push(path.clone());
            files.insert(path, text.into_bytes());
        }
    }
    entrypoints.sort();
    let toolchain = include_str!("../assets/project/lean-toolchain");
    let prefix = "UorustEmbed";
    let lexlean = format!(
        "spec = \"lexlean/project/1\"\nname = \"uorust-embed\"\nlanguage = \"1.2\"\n\
         module_prefix = \"{prefix}\"\nsource_roots = [\"src\"]\nentrypoints = [{}]\n\
         build_root = \".lexlean\"\nlockfile = \"lexlean.lock\"\nlean_workspace = \".\"\n\
         lean_toolchain = \"{}\"\n\n\
         [[lexicon_source]]\npackage = \"lexlean.std.nat\"\nkind = \"builtin\"\n\n\
         [[lexicon_source]]\npackage = \"uorust.mir\"\nkind = \"path\"\npath = \"language/uorust.mir\"\n\n\
         [[lexicon_source]]\npackage = \"uorust.titles\"\nkind = \"path\"\npath = \"language/uorust.titles\"\n\n\
         [limits]\nmax_file_bytes = {budget}\nmax_total_source_bytes = 67108864\n\
         max_primitive_atoms = 2000000\nmax_token_lattice_edges = 4000000\nmax_parse_states = 4000000\n\
         max_ir_nodes = 2000000\nmax_scope_depth = 1024\nmax_import_depth = 128\nmax_diagnostics = 256\n\
         max_child_output_bytes = 16777216\nchild_timeout_ms = {CHILD_TIMEOUT_MS}\n",
        entrypoints
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", "),
        toolchain.trim(),
    );
    files.insert("lexlean.toml".into(), lexlean.into_bytes());
    files.insert(
        "lakefile.toml".into(),
        format!("name = \"{prefix}\"\nversion = \"0.1.0\"\ndefaultTargets = [\"{prefix}\"]\n\n[[lean_lib]]\nname = \"{prefix}\"\n").into_bytes(),
    );
    files.insert(
        "lake-manifest.json".into(),
        format!("{{\"version\":\"1.2.0\",\"packagesDir\":\".lake/packages\",\"packages\":[],\"name\":\"{prefix}\",\"lakeDir\":\".lake\",\"fixedToolchain\":false}}\n").into_bytes(),
    );
    files.insert("lean-toolchain".into(), toolchain.as_bytes().to_vec());
    files
}

/// Embed every body of the module shards under `dumps` (one directory per
/// module; `only`, when nonempty, the modules to embed) whose sources are
/// under `library` (rust-src's `library`), and write the LexLean project to
/// `out` with the census, `census.toml`.
///
/// The differential check is two runs. With `harness`, a Rust crate is
/// written there that calls each sampled body's function, as rustc
/// compiles core, on inputs drawn from its types' widths, and prints what
/// each returns. With `observed`, that output, the project gains the
/// modules `DifferentialPart{k}`: for each input a theorem that the evaluator,
/// run on the embedded body, returns what rustc's build returned, which
/// `lexlean verify` decides, and `differential.toml` names each theorem's
/// body and inputs.
pub fn embed(
    krate: &str,
    library: &Path,
    dumps: &Path,
    out: &Path,
    only: &[String],
    outputs: &Outputs<'_>,
) -> Result<EmbedCensus, UorustError> {
    embed_with(krate, &[], library, dumps, out, only, outputs)
}

/// [`embed`] for a crate that builds on the crates `builds_on` as well as
/// core and alloc, each a directory of `library` by its name (a crate of
/// crates.io and its dependencies): its bodies make values of their enums
/// and structs, read from their declarations as core's are.
pub fn embed_with(
    krate: &str,
    builds_on: &[String],
    library: &Path,
    dumps: &Path,
    out: &Path,
    only: &[String],
    outputs: &Outputs<'_>,
) -> Result<EmbedCensus, UorustError> {
    let Outputs {
        harness,
        observed,
        register,
        bridge,
        differential: alone,
        link,
    } = *outputs;
    let root = library.parent().unwrap_or(library);
    // Each library linked with those before it: alloc with core.
    let mut linked: Vec<Linked> = Vec::new();
    // A library that does not link is named and left out: the crate's
    // calls into it stay its dictionary's, as they are unlinked.
    for (lib, dir) in link {
        match link_library(lib, root, dir, &linked) {
            Ok(l) => linked.push(l),
            Err(e) => eprintln!("embed: {lib} is not linked: {e}"),
        }
    }
    let (modules, census, program, _) = embed_linked(krate, builds_on, root, dumps, only, &linked)?;
    if let Some(dir) = register {
        for (module, bodies) in &modules {
            let file = dir.join(format!("{module}.toml"));
            if file.exists() {
                let by_kappa: BTreeMap<&str, &Embedded> =
                    bodies.iter().map(|(k, e)| (k.as_str(), e)).collect();
                let text = stamp_rows(&read_text(&file)?, &by_kappa);
                std::fs::write(&file, text).map_err(|e| crate::error::io(&file, e))?;
            }
        }
    }
    let mut extra = BTreeMap::new();
    let mut index = None;
    if let Some(observed) = observed {
        let text = read_text(observed)?;
        let (modules, checks) = differential(&program, observed, &text)?;
        extra = modules;
        index = Some(checks);
    }
    extra.insert("src/MirLemmas.lex.tex".into(), LEMMAS.as_bytes().to_vec());
    // The differential check's modules on their own: each holds the
    // bodies its theorems run, so a project of them and the evaluator
    // verifies the check apart from the rest of the embedding.
    if let (Some(dir), Some(checks)) = (alone, index.as_ref()) {
        let mut files = embed_project(krate, &[], extra.clone(), crate::project::MAX_FILE_BYTES);
        files.insert("differential.toml".into(), checks.clone().into_bytes());
        replace_dir(dir, Some(&files))?;
    }
    let mut files = embed_project(krate, &modules, extra, crate::project::MAX_FILE_BYTES);
    files.insert("census.toml".into(), census.render().into_bytes());
    if let Some(checks) = index {
        files.insert("differential.toml".into(), checks.into_bytes());
    }
    replace_dir(out, Some(&files))?;
    if let Some((shallow, dir)) = bridge {
        crate::bridge::project(krate, &modules, &program, shallow, dir)?;
    }
    // A module none of whose bodies is sampled has no harness.
    if let Some(dir) = harness {
        let of_crates_io = !["core", "alloc", "std"].contains(&krate);
        let crate_files = (!program.samples.is_empty())
            .then(|| harness_crate(&program, of_crates_io.then_some(krate)));
        replace_dir(dir, crate_files.as_ref())?;
    }
    Ok(census)
}

/// What `uorust embed` writes besides its project, where it is asked:
/// the differential check's harness crate, the theorems of the harness's
/// output, and each row's embedding on a register's module files.
#[derive(Clone, Copy, Debug, Default)]
pub struct Outputs<'a> {
    /// The harness crate's directory.
    pub harness: Option<&'a Path>,
    /// The harness's output.
    pub observed: Option<&'a Path>,
    /// The register's module files to stamp.
    pub register: Option<&'a Path>,
    /// The shallow translations `uorust core --lexlean` wrote, and the
    /// directory the bridge's project is written to (`crate::bridge`).
    pub bridge: Option<(&'a Path, &'a Path)>,
    /// Where the differential check's modules are written as a project of
    /// their own, beside the evaluator, with their index.
    pub differential: Option<&'a Path>,
    /// Library crates (core, alloc) to link into the program, each with
    /// its dumps: a call into one names its body ([`link_library`]).
    pub link: &'a [(&'a str, &'a Path)],
}

/// A register's module file with each row's embedding stamped on it by
/// its κ: the digest of the body's term (`embedded`) and the constructs
/// the evaluator does not define that it uses (`blocked`), in place of
/// whatever the row recorded. A row whose κ the embedding has no body of
/// records none.
pub(crate) fn stamp_rows(text: &str, by_kappa: &BTreeMap<&str, &Embedded>) -> String {
    let mut out = String::new();
    let mut kappa: Option<String> = None;
    let mut pending = false;
    let stamp = |out: &mut String, kappa: &Option<String>| {
        if let Some(e) = kappa.as_deref().and_then(|k| by_kappa.get(k)) {
            let _ = writeln!(
                out,
                "embedded = \"{}\"",
                crate::project::sha(e.term.as_bytes())
            );
            if !e.blocked.is_empty() {
                let quoted: Vec<String> = e.blocked.iter().map(|b| census_string(b)).collect();
                let _ = writeln!(out, "blocked = [{}]", quoted.join(", "));
            }
        }
    };
    for line in text.lines() {
        if line.starts_with("embedded = ") || line.starts_with("blocked = ") {
            continue;
        }
        // The stamp follows a row's κ and its span, as the register writes
        // them (`write_register`).
        if pending && !line.starts_with("at = ") {
            stamp(&mut out, &kappa);
            pending = false;
        }
        if line == "[[fn]]" {
            kappa = None;
        }
        if let Some(k) = line
            .strip_prefix("kappa = \"")
            .and_then(|k| k.strip_suffix('"'))
        {
            kappa = Some(k.to_string());
            pending = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    if pending {
        stamp(&mut out, &kappa);
    }
    out
}

/// A file's text.
pub(crate) fn read_text(path: &Path) -> Result<String, UorustError> {
    std::fs::read_to_string(path).map_err(|e| crate::error::io(path, e))
}

/// A dump's text, with the spans it prints ([`crate::mir::parse::dumped`]).
pub(crate) fn read_dump(path: &Path) -> Result<String, UorustError> {
    Ok(crate::mir::parse::dumped(&read_text(path)?))
}

/// A dump's text as rustc prints it without spans
/// ([`crate::mir::parse::unspanned`]).
pub(crate) fn read_plain_dump(path: &Path) -> Result<String, UorustError> {
    Ok(crate::mir::parse::unspanned(&read_dump(path)?))
}

/// The paths a directory holds, sorted.
pub(crate) fn dir_paths(dir: &Path) -> Result<Vec<std::path::PathBuf>, UorustError> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| crate::error::io(dir, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    paths.sort();
    Ok(paths)
}

/// Remove `dir` and, given `files`, write them there in its place.
pub(crate) fn replace_dir(
    dir: &Path,
    files: Option<&BTreeMap<String, Vec<u8>>>,
) -> Result<(), UorustError> {
    let io = crate::error::io;
    crate::error::remove_owned(dir)?;
    for (path, bytes) in files.into_iter().flatten() {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        std::fs::write(&file, bytes).map_err(|e| io(&file, e))?;
    }
    Ok(())
}

/// The rust-src `library` directory under the root the dumps name their
/// sources from.
fn library_root(root: &Path) -> std::path::PathBuf {
    root.join("library")
}

/// A scalar type of a sampled signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sampled {
    Int(u32, bool),
    Bool,
    /// A float of the IEEE 754 format of these exponent and fraction
    /// widths, drawn and compared as its bits: a NaN is the NaN its bits
    /// are.
    Float(u32, u32),
}

/// The float types a harness names on the stable compiler.
const STABLE_FLOATS: &[&str] = &["f32", "f64"];

impl Sampled {
    fn of(
        printed: &str,
        width: u32,
        floats: &BTreeMap<String, crate::mir::float::Format>,
    ) -> Option<Self> {
        if printed == "bool" {
            return Some(Self::Bool);
        }
        if let Some(f) = floats
            .get(printed)
            .filter(|_| STABLE_FLOATS.contains(&printed))
        {
            return Some(Self::Float(f.exp, f.frac));
        }
        int_ty(printed, width).map(|(b, s)| Self::Int(b, s))
    }

    /// Its width in bits.
    fn bits(self) -> u32 {
        match self {
            Self::Int(b, _) => b,
            Self::Bool => 1,
            Self::Float(e, m) => 1 + e + m,
        }
    }

    /// The inputs a check draws: the values at the edges of the type's
    /// range and its width, as two's complement bits.
    fn inputs(self) -> Vec<u128> {
        match self {
            Self::Bool => vec![0, 1],
            Self::Int(bits, signed) => {
                let top = 1u128 << (bits - 1);
                let max = top - 1 + top;
                let mut v = vec![0, 1, u128::from(bits - 1), top, top - 1, max];
                if !signed {
                    v.push(2);
                }
                v.sort_unstable();
                v.dedup();
                v
            }
            // Zero and its negation, one, the least subnormal, the greatest
            // finite, infinity and the quiet NaN, each of either sign.
            Self::Float(e, m) => {
                let sign = 1u128 << (e + m);
                let inf = ((1u128 << e) - 1) << m;
                let one = ((1u128 << (e - 1)) - 1) << m;
                let nan = inf | (1u128 << (m - 1));
                let mut v: Vec<u128> = [0, one, 1, inf - 1, inf, nan]
                    .iter()
                    .flat_map(|x| [*x, x | sign])
                    .collect();
                v.sort_unstable();
                v.dedup();
                v
            }
        }
    }

    /// `x` as a Rust expression of the type.
    fn rust(self, printed: &str, x: u128) -> String {
        match self {
            Self::Bool => (x == 1).to_string(),
            Self::Int(_, false) => format!("{x}{printed}"),
            Self::Int(..) => format!("({x}{} as {printed})", unsigned(printed)),
            Self::Float(..) => format!("{printed}::from_bits({x})"),
        }
    }

    /// `x` as a `Mir::Value` term.
    fn value(self, x: u128) -> String {
        match self {
            Self::Bool => mir_ctor("Value.boolean", &[(x == 1).to_string()]),
            Self::Int(bits, signed) => mir_ctor(
                "Value.int",
                &[bits.to_string(), signed.to_string(), x.to_string()],
            ),
            // A float's value is its bits (the evaluator's `floatValue`).
            Self::Float(..) => mir_ctor(
                "Value.int",
                &[self.bits().to_string(), false.to_string(), x.to_string()],
            ),
        }
    }
}

/// The unsigned integer type of a signed one's width: `i32` is `u32`'s.
fn unsigned(printed: &str) -> String {
    match printed.strip_prefix('i') {
        Some(rest) => format!("u{rest}"),
        None => printed.to_string(),
    }
}

/// A body the differential check runs: its key in the program, its
/// definition's name, the function rustc's build calls it by, its
/// parameters' and result's types as printed and read, and the bodies it
/// reaches.
#[derive(Clone, Debug)]
pub(crate) struct Sample {
    key: String,
    kappa: String,
    function: String,
    params: Vec<(String, Sampled)>,
    result: (String, Sampled),
    closure: Vec<String>,
    /// The fuel a run of it takes at most ([`derived_fuel`]).
    fuel: u128,
    /// For a body of a crate of crates.io, the module its call is written
    /// in: a harness outside the crate calls it through a wrapper there
    /// ([`harness_crate`]), where its private items are seen.
    home: Option<Vec<String>>,
    /// For a method of an impl, the file the impl is written in, from the
    /// crate's root, its wrapper's home in place of a module's file.
    file: Option<String>,
}

impl Sample {
    /// Each case's inputs: the `i`th case takes each parameter's
    /// `(i + j)`th input, cycling, for as many cases as the parameter with
    /// the most inputs has.
    fn cases(&self) -> Vec<Vec<u128>> {
        let inputs: Vec<Vec<u128>> = self.params.iter().map(|(_, s)| s.inputs()).collect();
        let n = inputs.iter().map(Vec::len).max().unwrap_or(0);
        (0..n)
            .map(|i| {
                inputs
                    .iter()
                    .enumerate()
                    .map(|(j, v)| v[(i + j) % v.len()])
                    .collect()
            })
            .collect()
    }
}

/// The body `b` as a sample, where it is a stable, safe method of a
/// primitive integer type that core's source under `num` declares, over
/// scalars.
fn sample(
    root: &Path,
    lib: &Path,
    b: &IrBody,
    key: &str,
    kappa: &str,
    width: u32,
    floats: &BTreeMap<String, crate::mir::float::Format>,
) -> Option<Sample> {
    let display = crate::mir::resolve_impls(root, &b.path).unwrap_or_else(|| b.path.clone());
    let [module, ty, name] = crate::mir::parse::split_path(&display)[..] else {
        return None;
    };
    (int_ty(ty, width).is_some() || floats.contains_key(ty)).then_some(())?;
    let scalar = |i: usize| {
        let t = b.locals.get(i)?;
        Some((t.clone(), Sampled::of(t, width, floats)?))
    };
    let params: Vec<(String, Sampled)> = (1..=b.arity).map(scalar).collect::<Option<_>>()?;
    let result = scalar(0)?;
    if params.is_empty() || !stable_and_safe(&module_sources(lib, module), name) {
        return None;
    }
    Some(Sample {
        key: key.to_string(),
        kappa: kappa.to_string(),
        function: format!("{ty}::{name}"),
        params,
        result,
        closure: Vec::new(),
        fuel: 0,
        home: None,
        file: None,
    })
}

/// The body `b` of a crate of crates.io, dumped to the file of stem
/// `stem`, as a sample, where it is a safe function of the crate over
/// scalars: an item its module path names from `crate::`, which a wrapper
/// the harness writes in the module that declares it calls (a wrapper
/// rustc refuses is left out). MIR prints a path trimmed to what tells it
/// apart (`has_zero_byte` for `arch::all::memchr::has_zero_byte`); its dump's
/// file is named by the whole of it, `<crate>.arch-all-memchr-has_zero_byte`,
/// a method's with its impl's `{impl#k}`, which no source path names.
#[allow(clippy::too_many_arguments)]
fn crate_sample(
    stem: &str,
    package: &Path,
    lib: &Path,
    b: &IrBody,
    key: &str,
    kappa: &str,
    width: u32,
    floats: &BTreeMap<String, crate::mir::float::Format>,
) -> Option<Sample> {
    let segments: Vec<&str> = stem.split_once('.')?.1.split('-').collect();
    if let Some(method) = method_sample(package, b, key, kappa, width, floats) {
        return Some(method);
    }
    let ident = |s: &&str| {
        s.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
            && s.chars().all(|c| c.is_alphanumeric() || c == '_')
            && *s != "_"
    };
    if segments.is_empty() || !segments.iter().all(ident) {
        return None;
    }
    let (name, above) = segments.split_last()?;
    if crate::mir::parse::split_path(&b.path).last() != Some(name) {
        return None;
    }
    let scalar = |i: usize| {
        let t = b.locals.get(i)?;
        Some((t.clone(), Sampled::of(t, width, floats)?))
    };
    let params: Vec<(String, Sampled)> = (1..=b.arity).map(scalar).collect::<Option<_>>()?;
    let result = scalar(0)?;
    if params.is_empty() {
        return None;
    }
    // Declared in its module's file, and not `unsafe`: a harness calls it
    // on any input.
    let file = read_text(&crate::layouts::module_file(lib, above)).ok()?;
    let decl = format!("fn {name}(");
    if !file.contains(&decl) || file.contains(&format!("unsafe fn {name}(")) {
        return None;
    }
    Some(Sample {
        key: key.to_string(),
        kappa: kappa.to_string(),
        function: format!("crate::{}", segments.join("::")),
        params,
        result,
        closure: Vec::new(),
        fuel: 0,
        home: Some(above.iter().map(|s| s.to_string()).collect()),
        file: None,
    })
}

/// The scalars of `b`'s signature, its parameters' and its result's, where
/// each is one and it takes one at least.
/// A sampled signature: its parameters' types and scalars, and its
/// result's.
type Signature = (Vec<(String, Sampled)>, (String, Sampled));

fn scalar_signature(
    b: &IrBody,
    width: u32,
    floats: &BTreeMap<String, crate::mir::float::Format>,
) -> Option<Signature> {
    let scalar = |i: usize| {
        let t = b.locals.get(i)?;
        Some((t.clone(), Sampled::of(t, width, floats)?))
    };
    let params: Vec<(String, Sampled)> = (1..=b.arity).map(scalar).collect::<Option<_>>()?;
    let result = scalar(0)?;
    (!params.is_empty()).then_some((params, result))
}

/// A method of one of the crate's impls as a sample (`{impl#k}` in its
/// dump's name), where the impl's header, which MIR names by its span
/// (`<impl at src/lib.rs:40:1: 40:9>::f`), is no generic one: `impl T`
/// calls it `T::f`, `impl Tr for T` calls it `<T as Tr>::f`, written in the
/// impl's own file, where the names its header writes are seen. A
/// derive's span (its trait's name alone) is no header.
fn method_sample(
    root: &Path,
    b: &IrBody,
    key: &str,
    kappa: &str,
    width: u32,
    floats: &BTreeMap<String, crate::mir::float::Format>,
) -> Option<Sample> {
    let at = b.path.find("<impl at ")?;
    let rest = &b.path[at + "<impl at ".len()..];
    let (span, name) = rest.split_once(">::")?;
    if name.contains("::") || name.contains('<') || name.contains('{') {
        return None;
    }
    let (params, result) = scalar_signature(b, width, floats)?;
    let (file, header) = span_text(root, span)?;
    let header = header.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = header.strip_prefix("impl ")?;
    if body.starts_with('<') || body.starts_with('!') || body.contains(" where ") {
        return None;
    }
    let callee = match body.split_once(" for ") {
        Some((tr, ty)) => format!("<{ty} as {tr}>::{name}"),
        None => format!("{body}::{name}"),
    };
    let text = read_text(&root.join(&file)).ok()?;
    if !text.contains(&format!("fn {name}(")) || text.contains(&format!("unsafe fn {name}(")) {
        return None;
    }
    Some(Sample {
        key: key.to_string(),
        kappa: kappa.to_string(),
        function: callee,
        params,
        result,
        closure: Vec::new(),
        fuel: 0,
        home: Some(Vec::new()),
        file: Some(file),
    })
}

/// The file a span (`src/lib.rs:40:1: 41:9`, lines and columns from one)
/// names, from `root`, and the text it spans.
fn span_text(root: &Path, span: &str) -> Option<(String, String)> {
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
    let source = read_text(&root.join(file)).ok()?;
    let lines: Vec<&str> = source.lines().collect();
    let mut text = String::new();
    for l in l1..=l2 {
        let chars: Vec<char> = lines.get(l.checked_sub(1)?)?.chars().collect();
        let from = if l == l1 { c1.saturating_sub(1) } else { 0 };
        let to = if l == l2 {
            c2.saturating_sub(1).min(chars.len())
        } else {
            chars.len()
        };
        text.extend(chars.get(from..to)?);
        text.push(' ');
    }
    Some((file.to_string(), text))
}

/// The source files of a top-level module: its directory's, or, for one
/// the crate root declares at a path (`#[path = "num/f32.rs"] pub mod
/// f32;`), that file.
fn module_sources(lib: &Path, module: &str) -> Vec<std::path::PathBuf> {
    let src = lib.parent().unwrap_or(lib);
    let own = src.join(module);
    if own.is_dir() {
        return dir_paths(&own).unwrap_or_default();
    }
    let root = read_text(lib).unwrap_or_default();
    let mut path: Option<&str> = None;
    for line in root.lines() {
        let t = line.trim();
        if let Some(p) = t
            .strip_prefix("#[path = \"")
            .and_then(|r| r.strip_suffix("\"]"))
        {
            path = Some(p);
            continue;
        }
        let declared = t
            .split_once("mod ")
            .is_some_and(|(vis, rest)| !vis.contains('{') && rest == format!("{module};"));
        if declared {
            if let Some(p) = path {
                return vec![src.join(p)];
            }
        }
        if !t.starts_with('#') {
            path = None;
        }
    }
    Vec::new()
}

/// Whether every declaration of `fn name` in `entries`, a module's
/// sources, is
/// stable and safe: its attributes say `#[stable(` and not `#[unstable(`,
/// and it is no `unsafe fn`. A harness on the stable toolchain can call it,
/// and on any input.
fn stable_and_safe(entries: &[std::path::PathBuf], name: &str) -> bool {
    let decl = format!("fn {name}(");
    let mut found = false;
    for entry in entries {
        let Ok(text) = read_text(entry) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains(&decl) {
                continue;
            }
            found = true;
            if line.contains("unsafe fn") {
                return false;
            }
            // Its attributes and doc comment: the lines above it up to the
            // end of the item before.
            let mut attrs = String::new();
            for above in lines[..i].iter().rev() {
                let t = above.trim();
                if t.is_empty() || t.ends_with('}') || t.ends_with(';') {
                    break;
                }
                attrs.push_str(t);
            }
            if !attrs.contains("#[stable(") || attrs.contains("#[unstable(") {
                return false;
            }
        }
    }
    found
}

/// Every body the program embeds, by key, with its κ, and the samples.
pub(crate) struct Program {
    pub(crate) every: BTreeMap<String, (String, Embedded)>,
    samples: Vec<Sample>,
    /// The target's pointer width, `usize`'s.
    pub(crate) width: u32,
    /// Whether the target is big-endian.
    pub(crate) big_endian: bool,
    /// Each selected row's key in the program and its locals' types as
    /// MIR prints them, the return place's first, by κ: what the bridge
    /// (`crate::bridge`) reads a body's signature from.
    pub(crate) signatures: BTreeMap<String, (String, Vec<String>)>,
    /// How many globals the program holds: its `static mut`s.
    pub(crate) globals: usize,
    /// The types of the instances the calls reach whose layouts are not
    /// read: the layouts the dump step asks rustc for.
    unread: BTreeSet<String>,
}

/// What a sample reaches: every body, which the evaluator defines, or
/// why it does not.
enum Reach {
    Closed(Vec<String>),
    Open(String),
}

/// The keys of the bodies `key` reaches, itself first, where the evaluator
/// defines each: else why not, by the first body that stops it, which is
/// not embedded, is blocked, calls an intrinsic, leaves a callee to a
/// dictionary, or reaches an instance whose generics no binding fixes
/// (`Generic.unknown`, and the root's own slots, which a sample's call
/// supplies none of).
/// A term read as an s-expression: an atom, or a parenthesized list.
#[derive(Debug, PartialEq, Eq)]
enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

/// The s-expression `text` starts with.
fn sexp(text: &str) -> Option<Sexp> {
    fn one(chars: &[char], i: &mut usize) -> Option<Sexp> {
        while chars.get(*i).is_some_and(|c| c.is_whitespace()) {
            *i += 1;
        }
        match chars.get(*i)? {
            '(' => {
                *i += 1;
                let mut items = Vec::new();
                loop {
                    while chars.get(*i).is_some_and(|c| c.is_whitespace()) {
                        *i += 1;
                    }
                    if *chars.get(*i)? == ')' {
                        *i += 1;
                        return Some(Sexp::List(items));
                    }
                    items.push(one(chars, i)?);
                }
            }
            ')' => None,
            '"' => {
                let start = *i;
                *i += 1;
                while *chars.get(*i)? != '"' {
                    *i += if chars[*i] == '\\' { 2 } else { 1 };
                }
                *i += 1;
                Some(Sexp::Atom(chars[start..*i].iter().collect()))
            }
            _ => {
                let start = *i;
                while chars
                    .get(*i)
                    .is_some_and(|c| !c.is_whitespace() && *c != '(' && *c != ')')
                {
                    *i += 1;
                }
                Some(Sexp::Atom(chars[start..*i].iter().collect()))
            }
        }
    }
    let chars: Vec<char> = text.chars().collect();
    one(&chars, &mut 0)
}

/// A constructor term's name and arguments: `(ctor Mir::T.c a b)`.
fn ctor_of(s: &Sexp) -> Option<(&str, &[Sexp])> {
    match s {
        Sexp::List(items) => match items.as_slice() {
            [Sexp::Atom(c), Sexp::Atom(name), args @ ..] if c == "ctor" => Some((name, args)),
            _ => None,
        },
        Sexp::Atom(_) => None,
    }
}

/// The layouts a list term of layouts holds, however its runs are joined.
fn layouts_in(s: &Sexp) -> Vec<&Sexp> {
    if ctor_of(s).is_some_and(|(n, _)| n.starts_with(&mir_name("Layout."))) {
        return vec![s];
    }
    match s {
        Sexp::List(items) => items.iter().flat_map(layouts_in).collect(),
        Sexp::Atom(_) => Vec::new(),
    }
}

/// The pieces of a value a layout encodes, as the evaluator's encoding and
/// decoding take them, one task each: the layout itself, and each piece
/// of each value it holds, an array's elements each, a tagged enum's
/// largest variant's.
fn layout_pieces(s: &Sexp) -> Option<u128> {
    let (name, args) = ctor_of(s)?;
    let kind = name.strip_prefix(&format!("{}.", mir_name("Layout")))?;
    let below = match kind {
        "array" => {
            let Sexp::Atom(n) = args.get(1)? else {
                return None;
            };
            layout_pieces(args.first()?)?.checked_mul(n.parse().ok()?)?
        }
        "record" => layouts_in(args.get(3)?)
            .into_iter()
            .map(layout_pieces)
            .try_fold(0u128, |a, p| a.checked_add(p?))?,
        "tagged" => layouts_in(args.get(11)?)
            .into_iter()
            .map(layout_pieces)
            .try_fold(0u128, |a, p| Some(a.max(p?)))?,
        "ranged" => layout_pieces(args.first()?)?,
        _ => 0,
    };
    below.checked_add(1)
}

/// The bytes a layout spans, as its term states them.
fn layout_size(s: &Sexp) -> Option<u128> {
    let (name, args) = ctor_of(s)?;
    let nat = |i: usize| match args.get(i) {
        Some(Sexp::Atom(n)) => n.parse::<u128>().ok(),
        _ => None,
    };
    match name.strip_prefix(&format!("{}.", mir_name("Layout")))? {
        "boolean" => Some(1),
        "character" => Some(4),
        "array" => nat(1)?.checked_mul(nat(2)?),
        "ranged" => layout_size(args.first()?),
        _ => nat(0),
    }
}

/// The fuel a step's own work takes past it, `text` starting with the
/// term of what does it, each part having the fuel left: a transmute's
/// source's encoding (a task for each piece and one to end) or its
/// target's decoding (two for each, the piece and the value built of it
/// or its range checked, and one to end), whichever is more; a `raw_eq`'s
/// encoding of its layout; a `write_bytes`'s transmute of its element's
/// bytes to the element. None where a layout is its instance's.
fn transmute_work(text: &str) -> Option<u128> {
    let term = sexp(text)?;
    let (name, args) = ctor_of(&term)?;
    fn layout(g: &Sexp) -> Option<&Sexp> {
        let (name, args) = ctor_of(g)?;
        (name == mir_name("Generic.layout") || name == mir_name("Generic.aligned"))
            .then(|| args.first())
            .flatten()
    }
    let encode = |l: &Sexp| layout_pieces(l)?.checked_add(1);
    let decode = |l: &Sexp| layout_pieces(l)?.checked_mul(2)?.checked_add(1);
    if name == mir_name("Callee.rawEq") {
        return encode(layout(args.first()?)?);
    }
    if name == mir_name("Callee.writeBytes") {
        let element = layout(args.first()?)?;
        // The element's bytes, an array of a piece for each, and its end.
        let bytes = layout_size(element)?.checked_add(2)?;
        return Some(bytes.max(decode(element)?));
    }
    let from = encode(layout(args.first()?)?)?;
    let to = decode(layout(args.get(1)?)?)?;
    Some(from.max(to))
}

/// The fuel a run of the body `key` takes at most: its steps, each
/// callee's for each of its calls, and the most fuel a step's own work or
/// a constant's pinning takes past it, which the evaluator bounds by the
/// fuel left; or why no bound is read.
fn derived_fuel(every: &BTreeMap<String, (String, Embedded)>, key: &str) -> Fuel {
    fn go(
        every: &BTreeMap<String, (String, Embedded)>,
        key: &str,
        open: &mut Vec<String>,
        done: &mut BTreeMap<String, (u128, u128)>,
    ) -> Fuel {
        if let Some(&(steps, work)) = done.get(key) {
            return Fuel::Bounded(steps, work);
        }
        if open.iter().any(|k| k == key) {
            if std::env::var_os("UORUST_DIFFERENTIAL_DETAIL").is_some() {
                eprintln!("differential: a call of {key} reaches it again");
            }
            return Fuel::Unbounded(
                "reaches a body that calls itself, which no loop bound bounds".into(),
            );
        }
        let Some((_, e)) = every.get(key) else {
            return Fuel::Unbounded("reaches a body not embedded".into());
        };
        let (own, calls, mine, _) = match &e.cost {
            Cost::Bounded {
                steps,
                calls,
                work,
                pin,
            } => (*steps, calls, *work, pin),
            Cost::Unread => return Fuel::Unbounded("reaches a body whose cost is not read".into()),
            Cost::Unbounded(why) => {
                if std::env::var_os("UORUST_DIFFERENTIAL_DETAIL").is_some() {
                    eprintln!("differential: {key}: {why}");
                }
                // The reason, its block aside, counts the samples it leaves out.
                let why = match why.split_once(" (at bb") {
                    Some((head, tail)) => {
                        format!("{head}{}", tail.split_once(')').map_or("", |(_, t)| t))
                    }
                    None => why.clone(),
                };
                return Fuel::Unbounded(format!("reaches a body where {why}"));
            }
        };
        open.push(key.to_string());
        let (mut steps, mut work) = (own, mine);
        for (callee, runs, constant) in calls {
            if *runs == 0 {
                continue;
            }
            let (s, w) = match go(every, callee, open, done) {
                Fuel::Bounded(s, w) => (s, w),
                unbounded => return unbounded,
            };
            steps = steps.saturating_add(runs.saturating_mul(s));
            work = work.max(w);
            if *constant {
                let pin = every.get(callee).and_then(|(_, c)| match &c.cost {
                    Cost::Bounded { pin, .. } => *pin,
                    _ => None,
                });
                match pin {
                    Some(p) => work = work.max(p),
                    None => {
                        return Fuel::Unbounded(format!(
                            "reads the constant item {callee}, whose value's size no layout read bounds"
                        ))
                    }
                }
            }
        }
        open.pop();
        done.insert(key.to_string(), (steps, work));
        Fuel::Bounded(steps, work)
    }
    match go(every, key, &mut Vec::new(), &mut BTreeMap::new()) {
        Fuel::Bounded(steps, work) => Fuel::Bounded(steps.saturating_add(work), 0),
        unbounded => unbounded,
    }
}

/// [`derived_fuel`]'s answer: the steps and the work past them, or why
/// no bound is read.
enum Fuel {
    Bounded(u128, u128),
    Unbounded(String),
}

fn closure(every: &BTreeMap<String, (String, Embedded)>, key: &str) -> Reach {
    let mut seen = vec![key.to_string()];
    let mut at = 0;
    while let Some(k) = seen.get(at).cloned() {
        let Some((_, e)) = every.get(&k) else {
            return Reach::Open("reaches a body not embedded".into());
        };
        if let Some(b) = e.blocked.iter().next() {
            return Reach::Open(format!("reaches a body blocked by {b}"));
        }
        if let Some(b) = e.bound.iter().next() {
            // A dictionary's name, its generic arguments and the
            // instance's impl aside: the kind of callee left.
            let kind =
                if b.starts_with("core::") || b.starts_with("alloc::") || b.starts_with("std::") {
                    b.split("::").next().unwrap_or_default().to_string()
                } else {
                    "the crate".to_string()
                };
            if std::env::var_os("UORUST_DIFFERENTIAL_DETAIL").is_some() {
                eprintln!("differential: {key} reaches {k}, which leaves {b} to a dictionary");
            }
            return Reach::Open(format!("leaves a callee of {kind} to a dictionary"));
        }
        if !e.intrinsics.is_empty() {
            return Reach::Open("calls an intrinsic".into());
        }
        if e.term
            .contains(&mir_ctor("Generic.unknown", &[]).replace(')', ""))
        {
            return Reach::Open("reaches an instance no binding fixes".into());
        }
        if at == 0
            && e.term
                .contains(&mir_ctor("Generic.slot", &[]).replace(')', ""))
        {
            return Reach::Open("is generic itself".into());
        }
        for c in &e.callees {
            if !seen.contains(c) {
                seen.push(c.clone());
            }
        }
        at += 1;
    }
    Reach::Closed(seen)
}

/// The harness crate: one call per case, under `catch_unwind`, each
/// printed as `<case> <result>` with an integer as its two's complement
/// bits, after a first line with the build's checks.
///
/// For a crate of crates.io (`krate`), whose functions are mostly private,
/// each sample is called through a wrapper `__uorust_differential_<k>`,
/// unmangled, that `wrappers.json` asks to be written into the crate's
/// copy at `../harness-crate` (beside the harness, outside its workspace,
/// as a crate that declares a workspace of its own must be), in the module
/// of its function, where its private items
/// are seen; the harness depends on that copy. Each sample's calls stand
/// between `// sample <k>` and `// unbuilt <k>`, and the lines from there to
/// `// end <k>`, commented out, print each of its cases `unbuilt`: where
/// rustc refuses the wrapper, those are the lines kept
/// ([`crate::registry`]).
fn harness_crate(program: &Program, krate: Option<&str>) -> BTreeMap<String, Vec<u8>> {
    let mut main = String::from(
        "// Written by `uorust embed --harness`; do not edit.\n\
         #![allow(unused_parens)]\n",
    );
    if let Some(k) = krate {
        let _ = writeln!(main, "use {} as _;", k.replace('-', "_"));
    }
    main.push_str(
        "fn main() {\n    std::panic::set_hook(Box::new(|_| {}));\n    \
         let overflow = std::panic::catch_unwind(|| std::hint::black_box(u8::MAX) + 1).is_err();\n    \
         println!(\"checks {} {}\", cfg!(debug_assertions), overflow);\n",
    );
    let mut wrappers = Vec::new();
    let mut case = 0usize;
    for (k, s) in program.samples.iter().enumerate() {
        let show = match s.result.1 {
            Sampled::Bool => "r.to_string()".to_string(),
            Sampled::Int(..) => {
                format!("((r as {}) as u128).to_string()", unsigned(&s.result.0))
            }
            Sampled::Float(..) => "r.to_bits().to_string()".to_string(),
        };
        let callee = match (&s.home, krate) {
            (Some(home), Some(_)) => {
                let wrapper = format!("__uorust_differential_{k}");
                let names: Vec<String> = (0..s.params.len()).map(|i| format!("a{i}")).collect();
                let typed: Vec<String> = names
                    .iter()
                    .zip(&s.params)
                    .map(|(n, (t, _))| format!("{n}: {t}"))
                    .collect();
                let signature = format!("fn {wrapper}({}) -> {}", typed.join(", "), s.result.0);
                wrappers.push(serde_json::json!({
                    "sample": k,
                    "module": home,
                    "file": s.file,
                    "code": format!(
                        "#[doc(hidden)] #[allow(warnings)] #[unsafe(no_mangle)] pub extern \"Rust\" {signature} {{ {}({}) }}",
                        s.function,
                        names.join(", ")
                    ),
                }));
                let _ = writeln!(
                    main,
                    "    // sample {k}\n    extern \"Rust\" {{ {signature}; }}"
                );
                wrapper
            }
            _ => s.function.clone(),
        };
        let wrapped = krate.is_some() && s.home.is_some();
        let first = case;
        for inputs in s.cases() {
            let args: Vec<String> = s
                .params
                .iter()
                .zip(&inputs)
                .map(|((printed, scalar), x)| scalar.rust(printed, *x))
                .collect();
            let call = if wrapped {
                format!("unsafe {{ {callee}({}) }}", args.join(", "))
            } else {
                format!("{callee}({})", args.join(", "))
            };
            let _ = write!(
                main,
                "    match std::panic::catch_unwind(|| {call}) {{\n        \
                 Ok(r) => println!(\"{case} {{}}\", {show}),\n        \
                 Err(_) => println!(\"{case} panic\"),\n    }}\n",
            );
            case += 1;
        }
        if wrapped {
            let _ = writeln!(main, "    // unbuilt {k}");
            for c in first..case {
                let _ = writeln!(main, "    // println!(\"{c} unbuilt\");");
            }
            let _ = writeln!(main, "    // end {k}");
        }
    }
    main.push_str("}\n");
    let dependency = match krate {
        Some(k) => format!("\n[dependencies]\n{k} = {{ path = \"../harness-crate\" }}\n"),
        None => String::new(),
    };
    let mut files = BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            format!("[package]\nname = \"uorust-embed-harness\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n{dependency}\n[workspace]\n")
                .into_bytes(),
        ),
        ("src/main.rs".to_string(), main.into_bytes()),
    ]);
    if krate.is_some() {
        files.insert(
            "wrappers.json".to_string(),
            serde_json::to_vec_pretty(&wrappers).unwrap_or_default(),
        );
    }
    files
}

/// How long `lexlean verify` lets one child process, one module, run
/// (the project's `child_timeout_ms`).
const CHILD_TIMEOUT_MS: usize = 300_000;

/// A measurement of how much the differential check's evaluation costs:
/// `MEASURED_FUEL` units of fuel, over a module's theorems, verified
/// in one child within `MEASURED_MS` (core::num's `DifferentialPart3` of
/// 495 theorems, the largest of its parts to verify within the timeout,
/// on the locked SDK's Lean v4.32.1; its parts of 1,124,627 fuel and more
/// did not).
pub(crate) const MEASURED_FUEL: usize = 664_506;
pub(crate) const MEASURED_MS: usize = 300_000;

/// The same measurement in units of [`derived_fuel`]. [`MEASURED_FUEL`]
/// counts the estimate the check ran on before: each statement and block
/// of a sample's closure, once per bit of its widest input and once more.
/// Over core::num's samples, the derived fuel is never less than that
/// estimate divided by [`ESTIMATE_PER_DERIVED`] (a body of no loop and no
/// call over a `u128`, its steps charged 129 times), so the part measured
/// held at least this much derived fuel.
const MEASURED_DERIVED_FUEL: u128 = MEASURED_FUEL as u128 / ESTIMATE_PER_DERIVED;

/// The most that estimate charged a step: once per bit of the widest
/// input a sample takes, a `u128`, and once more.
const ESTIMATE_PER_DERIVED: u128 = u128::BITS as u128 + 1;

/// The fuel one module of the differential check holds at most: Lean
/// decides each theorem by evaluation, at a cost the theorem's fuel
/// bounds, and a module is one child process that `lexlean verify` stops
/// at [`CHILD_TIMEOUT_MS`]; so a module's budget is what that timeout
/// verifies at the measured cost per unit of fuel.
fn differential_budget() -> u128 {
    (CHILD_TIMEOUT_MS as u128).saturating_mul(MEASURED_DERIVED_FUEL) / MEASURED_MS as u128
}

/// The modules `DifferentialPart{k}`, each with the bodies its theorems
/// run, by path, and their index, from the harness's output.
fn differential(
    program: &Program,
    file: &Path,
    observed: &str,
) -> Result<(BTreeMap<String, Vec<u8>>, String), UorustError> {
    let bad = |reason: String| UorustError::Malformed {
        path: file.to_path_buf(),
        reason,
    };
    let mut lines = observed.lines();
    let checks: Vec<&str> = lines
        .next()
        .and_then(|l| l.strip_prefix("checks "))
        .ok_or_else(|| bad("no checks line".into()))?
        .split(' ')
        .collect();
    let [debug, overflow] = checks[..] else {
        return Err(bad("a checks line of other than two flags".into()));
    };
    let results: BTreeMap<usize, &str> = lines
        .filter_map(|l| {
            let (n, r) = l.split_once(' ')?;
            Some((n.parse().ok()?, r))
        })
        .collect();
    let mut parts: Vec<(Vec<String>, BTreeSet<String>, u128)> = vec![Default::default()];
    let mut index = String::from(
        "# The differential check (written by `uorust embed --observed`; do not edit):\n\
         # each theorem of Differential, the body it runs, its inputs and what\n\
         # rustc's build returned.\n",
    );
    let mut case = 0usize;
    for s in &program.samples {
        let cases = s.cases();
        let first = case;
        case += cases.len();
        let name = def_name(&s.kappa);
        let mut theorems = Vec::new();
        for (i, inputs) in cases.iter().enumerate() {
            let Some(result) = results.get(&(first + i)) else {
                return Err(bad(format!("no result for case {}", first + i)));
            };
            // A panic is no value to compare, and an unbuilt case (a
            // wrapper rustc refused) no run.
            if *result == "panic" || *result == "unbuilt" {
                continue;
            }
            let expected = match s.result.1 {
                Sampled::Bool => s.result.1.value(u128::from(*result == "true")),
                Sampled::Int(..) | Sampled::Float(..) => s.result.1.value(
                    result
                        .parse()
                        .map_err(|_| bad(format!("case {}: {result}", first + i)))?,
                ),
            };
            let args: Vec<String> = s
                .params
                .iter()
                .zip(inputs)
                .map(|((_, sc), x)| sc.value(*x))
                .collect();
            let theorem = format!("t{name}x{i}");
            let _ = write!(
                index,
                "\n[[case]]\ntheorem = \"{theorem}\"\nbody = {}\nkappa = \"{}\"\nfunction = \"{}\"\ninputs = [{}]\nreturned = \"{result}\"\n",
                census_string(&s.key),
                s.kappa,
                s.function,
                inputs.iter().map(u128::to_string).collect::<Vec<_>>().join(", ")
            );
            theorems.push(format!(
                "theorem{{\n  name: {theorem},\n  parameters: [],\n  statement: ((Mir::evaluate {} (p{name}) {} {}) = {}),\n  proof: reflexivity{{}}\n}}",
                s.fuel,
                mir_string(&s.key),
                term_list(&args, &mir_name("Value")),
                mir_ctor("Outcome.returned", &[expected])
            ));
        }
        if theorems.is_empty() {
            continue;
        }
        // A sample's theorems stay in one module, which a sample whose
        // fuel is past the budget has to itself. A call finds its callee
        // by comparing paths along the program's bodies, so a step costs
        // up to the closure's size: the measurement was of samples of one
        // body.
        let cost = (theorems.len() as u128)
            .saturating_mul(s.fuel)
            .saturating_mul(s.closure.len().max(1) as u128);
        if parts
            .last()
            .is_some_and(|(_, _, n)| *n > 0 && n.saturating_add(cost) > differential_budget())
        {
            parts.push(Default::default());
        }
        let Some((decls, defined, spent)) = parts.last_mut() else {
            continue;
        };
        *spent = spent.saturating_add(cost);
        let mut bodies = Vec::new();
        for k in &s.closure {
            let Some((kappa, e)) = program.every.get(k) else {
                continue;
            };
            let c = format!("c{}", def_name(kappa));
            if defined.insert(c.clone()) {
                decls.push(format!(
                    "definition{{\n  name: {c},\n  parameters: [],\n  result: {},\n  body: {}\n}}",
                    mir_name("Body"),
                    e.term
                ));
            }
            bodies.push(format!("({c})"));
        }
        // Contracts are checked only under `-Zcontract-checks`, which no
        // build of core here passes.
        decls.push(format!(
            "definition{{\n  name: p{name},\n  parameters: [],\n  result: {},\n  body: (record {} {{bodies := {}, dictionary := (nil {}), ubChecks := {debug}, contractChecks := false, overflowChecks := {overflow}, pointerWidth := {}, bigEndian := {}, globals := {}}})\n}}",
            mir_name("Program"),
            mir_name("Program"),
            term_list(&bodies, &mir_name("Body")),
            mir_name("Binding"),
            program.width,
            program.big_endian,
            program.globals
        ));
        decls.extend(theorems);
    }
    let mut modules = BTreeMap::new();
    for (k, (decls, _, _)) in parts.iter().enumerate().filter(|(_, p)| p.2 > 0) {
        let module = format!(
            "\\begin{{lexlean}}{{DifferentialPart{k}}}\n\\useglossary{{lexlean.std.bool@1.2.0}}\n\
         \\useglossary{{lexlean.std.nat@1.2.0}}\n\\useglossary{{uorust.mir@1.0.0}}\n\
         \\importmodule{{{MIR}}}\n\\importmodule{{Prelude}}\n\\title{{Embedded MIR checks}}\n\\begin{{semanticmodule}}\n{}\n\
         \\end{{semanticmodule}}\n\\end{{lexlean}}\n",
            decls.join("\n\n")
        );
        modules.insert(
            format!("src/DifferentialPart{k}.lex.tex"),
            module.into_bytes(),
        );
    }
    Ok((modules, index))
}

/// A body's κ, as the core register keys it: its crate path and its text
/// as rustc prints it without spans, with the build's allocation numbers
/// renumbered.
pub(crate) fn body_kappa(krate: &str, path: &str, text: &str) -> String {
    crate::project::sha(
        format!(
            "{krate}::{path}\n{}",
            crate::mir::core::numbered_allocations(&crate::mir::parse::unspanned(text))
        )
        .as_bytes(),
    )
}

/// Whether a body has a row of its own in the register: closures,
/// promoted constants and anonymous constants are parts of the bodies
/// that use them.
pub(crate) fn has_row(path: &str) -> bool {
    !(path.contains("{closure") || path.contains("promoted[") || path.contains("{constant"))
}

/// The blocks control may go to next from `block`: a `switchInt` of a
/// local the block last sets to a constant (a `cfg!` or `const` condition,
/// `_5 = const false; switchInt(move _5)`) goes only to that constant's
/// arm, and every other terminator to its successors.
fn live_successors(
    block: &crate::mir::ir::IrBlock,
    named: &dyn Fn(&IrConstant) -> Option<i128>,
) -> Vec<usize> {
    let IrTerminator::Switch(IrOperand::Copy(p) | IrOperand::Move(p), arms, otherwise) =
        &block.terminator
    else {
        return block.terminator.successors();
    };
    let set = block.statements.iter().rev().find_map(|s| match s {
        IrStatement::Assign(q, r) if q.local == p.local => Some((q, r)),
        _ => None,
    });
    let value = match set {
        Some((q, IrRvalue::Use(IrOperand::Constant(c))))
            if q.projection.is_empty() && p.projection.is_empty() =>
        {
            match c {
                IrConstant::Bool(b) => Some(i128::from(*b)),
                IrConstant::Int(n, _) if *n >= 0 => Some(*n),
                c => named(c),
            }
        }
        _ => None,
    };
    match value {
        Some(v) => vec![arms
            .iter()
            .find(|(a, _)| *a == v)
            .map_or(*otherwise, |(_, t)| *t)],
        None => block.terminator.successors(),
    }
}

/// The type MIR prints of a place that is a local, unprojected.
fn place_type(body: &IrBody, p: &IrPlace) -> Option<String> {
    p.projection
        .is_empty()
        .then(|| body.locals.get(p.local).cloned())
        .flatten()
}

/// The type MIR prints of an argument, where it does: a local's, or a
/// literal's suffix.
fn arg_type(body: &IrBody, o: &IrOperand) -> Option<String> {
    match o {
        IrOperand::Copy(p) | IrOperand::Move(p) => place_type(body, p),
        IrOperand::Constant(IrConstant::Int(_, suffix)) => Some(suffix.clone()),
        IrOperand::Constant(IrConstant::Bool(_)) => Some("bool".into()),
        IrOperand::Constant(IrConstant::Char(_)) => Some("char".into()),
        _ => None,
    }
}

/// The name a body has in the program: its path as its `fn` line prints
/// it, or, where a macro's impls share the path its dump names them by
/// (`dumped`, which the linker keys them by), that path and its
/// definition's name.
pub(crate) fn program_key(
    link: &crate::mir::link::CoreLink,
    dumped: &str,
    path: &str,
    kappa: &str,
) -> String {
    let shared = |p: &str| link.bodies.get(p).is_some_and(|b| b.len() > 1);
    if shared(dumped) || shared(path) {
        format!("{path}#{}", def_name(kappa))
    } else {
        path.to_string()
    }
}

/// How the linker resolves a printed callee with arguments and a
/// destination of the types MIR prints: the one body of `krate` it names
/// (of a macro's impls, the one whose signature the call's types
/// instantiate), an intrinsic, or, where it names none, a name for the
/// dictionary: a trait's method at an abstract type.
pub(crate) fn resolve_callee(
    link: &crate::mir::link::CoreLink,
    krate: &str,
    callee: &str,
    args: &[Option<String>],
    dest: &str,
) -> Callee {
    // A body's key, as the program's loop over the dumps keys it: by the
    // path its `fn` line prints (rustc trims it, `arith::` for
    // `ops::arith::`), where the linker names it by its dump's.
    let key = |dumped: &str, text: &str| {
        let printed = text
            .lines()
            .find_map(|l| l.strip_prefix("fn "))
            .map_or(dumped, |h| &h[..h.find('(').unwrap_or(h.len())]);
        program_key(link, dumped, printed, &body_kappa(krate, printed, text))
    };
    let erased = crate::mir::lower::erase(callee);
    let path = link.path(&erased);
    if let Some(p) = path.as_ref().filter(|p| link.decls.intrinsics.contains(*p)) {
        return Callee::Intrinsic(p.clone());
    }
    if let Some(p) = path.as_ref() {
        if let Some([t]) = link.bodies.get(p).map(Vec::as_slice) {
            return Callee::Body(key(p, t));
        }
    }
    let Some(instance) = link.instance(callee, args, dest) else {
        return Callee::Bound(callee.to_string());
    };
    let Some(texts) = link.bodies.get(&instance.path) else {
        return Callee::Bound(callee.to_string());
    };
    if let [t] = texts.as_slice() {
        return Callee::Body(key(&instance.path, t));
    }
    // Of a macro's impls, the one whose signature, at the instance's
    // bindings, is the instance's.
    let generics: BTreeMap<String, String> = instance
        .bound
        .iter()
        .filter(|(k, _)| k != "Self")
        .cloned()
        .collect();
    let head = |t: &str| t.lines().find(|l| l.starts_with("fn ")).map(str::to_string);
    let want = head(&instance.text);
    let mut hits = texts
        .iter()
        .filter(|t| head(t).map(|l| crate::mir::stdlib::substitute(&l, &generics)) == want);
    match (hits.next(), hits.next()) {
        (Some(t), None) => Callee::Body(key(&instance.path, t)),
        _ => Callee::Bound(callee.to_string()),
    }
}

/// The pointer width the dumps under `dumps` were built for, and whether
/// the target is big-endian, from the `target.toml` beside them.
fn dump_target(dumps: &Path) -> Result<(u32, Option<bool>), UorustError> {
    let file = dumps.join("target.toml");
    let bad = |reason: String| UorustError::Malformed {
        path: file.clone(),
        reason,
    };
    let text = read_text(&file).map_err(|e| bad(e.to_string()))?;
    let doc: toml::Table = toml::from_str(&text).map_err(|e| bad(format!("{e}")))?;
    let width = doc
        .get("pointer_width")
        .and_then(toml::Value::as_integer)
        .and_then(|w| u32::try_from(w).ok())
        .ok_or_else(|| bad("no pointer_width".into()))?;
    let big_endian = crate::mir::layout::from_target_toml(&doc)
        .ok()
        .and_then(|layouts| layouts.first().map(|l| l.big_endian));
    Ok((width, big_endian))
}

/// Every module's bodies, embedded, by module and κ, with the census of
/// the modules `only` names (every module when empty). Every module's
/// dumps are linked, so a call into another module names its body.
#[allow(clippy::type_complexity)]
/// The types of the instances the bodies under `dumps` call whose layouts
/// the embedding does not read (from `dumps`' own `layouts.json`, where it
/// has one): what the dump step asks rustc to lay out, beside the types
/// the dumps print. `root` holds the crate's sources, `library/<krate>/src`.
pub(crate) fn instance_types(
    krate: &str,
    root: &Path,
    dumps: &Path,
) -> Result<BTreeSet<String>, UorustError> {
    Ok(embed_census(krate, &[], root, dumps, &[])?.2.unread)
}

/// How deep a type MIR prints nests: its brackets' depth.
fn nesting(ty: &str) -> usize {
    let (mut depth, mut most) = (0usize, 0usize);
    for c in ty.chars() {
        match c {
            '<' | '[' | '(' => {
                depth += 1;
                most = most.max(depth);
            }
            '>' | ']' | ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    most
}

/// Each call's callee, by its token: the body, with the generics of the
/// instance the call names. A body's slots are its own (what it reads
/// that its generic parameters leave open) and, transitively, those its
/// calls leave open: a callee's slot at the call's bindings is a layout or
/// a count where the binding fixes it, and else a slot of the caller,
/// where it names the caller's own parameters. The slots grow to a
/// fixpoint, bounded by the deepest type the program prints: no instance
/// nests deeper. A slot no binding fixes is `Generic.unknown`, which the
/// evaluator answers as undefined.
/// Each call's token written as its callee at its instance's generics,
/// the layouts no instance reads, and each body's slots, its own and those
/// its callees leave to it. A call into a library linked into the program
/// (`linked`, by name) is bound by that library's link.
fn instances(
    cx: &Cx<'_>,
    link: &crate::mir::link::CoreLink,
    linked: &[(&str, &crate::mir::link::CoreLink)],
    every: &BTreeMap<String, (String, Embedded)>,
    others: &[&Embedded],
) -> Instances {
    let mut unread = BTreeSet::new();
    let deepest = link
        .locals
        .iter()
        .map(|t| nesting(t))
        .max()
        .unwrap_or_default();
    let mut slots: BTreeMap<String, Vec<Slot>> = every
        .iter()
        .map(|(k, (_, e))| (k.clone(), e.slots.clone()))
        .collect();
    let mut bindings: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut bind = |site: &Site| -> BTreeMap<String, String> {
        bindings
            .entry(site.token.clone())
            .or_insert_with(|| {
                // A call into a linked library is bound as its link binds
                // the call, its name aside.
                let (link, text) = linked
                    .iter()
                    .find_map(|(k, l)| {
                        let rest = site.text.strip_prefix(&format!("{k}::"))?;
                        site.callee
                            .starts_with(&format!("{k}::"))
                            .then_some((*l, rest))
                    })
                    .unwrap_or((link, site.text.as_str()));
                let bound = if !site.constant {
                    link.instance(text, &site.args, &site.dest).map(|i| i.bound)
                } else if text.starts_with('<') {
                    link.constant(text).map(|i| i.bound)
                } else {
                    None
                };
                bound
                    .map(|b| b.into_iter().filter(|(k, v)| k != v).collect())
                    .unwrap_or_default()
            })
            .clone()
    };
    // A callee's slot at a call's bindings: its generic, or the caller's
    // slot it is.
    enum At {
        Fixed(String),
        Caller(Slot),
        Unknown(String),
    }
    let at = |slot: &Slot, bound: &BTreeMap<String, String>| -> At {
        let (text, layout) = match slot {
            Slot::Layout(t) => (crate::mir::stdlib::substitute(t, bound), true),
            Slot::Count(t) => (crate::mir::stdlib::substitute(t, bound), false),
        };
        let fixed = if layout {
            cx.fixed_layout(&text)
        } else {
            cx.count_of(&text)
                .map(|n| mir_ctor("Generic.count", &[n.to_string()]))
        };
        match fixed {
            Some(g) => At::Fixed(g),
            None if !cx.parameters(&text).is_empty() && nesting(&text) <= deepest => {
                At::Caller(if layout {
                    Slot::Layout(text)
                } else {
                    Slot::Count(text)
                })
            }
            None => At::Unknown(text),
        }
    };
    loop {
        let mut grown = false;
        for (key, (_, e)) in every.iter() {
            for site in &e.sites {
                let open = slots.get(&site.callee).cloned().unwrap_or_default();
                if open.is_empty() {
                    continue;
                }
                let bound = bind(site);
                for slot in &open {
                    if let At::Caller(s) = at(slot, &bound) {
                        let mine = slots.entry(key.clone()).or_default();
                        if !mine.contains(&s) {
                            mine.push(s);
                            grown = true;
                        }
                    }
                }
            }
        }
        if !grown {
            break;
        }
    }
    let mut out = BTreeMap::new();
    let all = every.values().map(|(_, e)| e).chain(others.iter().copied());
    for e in all {
        let mine = slots.get(&e.key).cloned().unwrap_or_default();
        for site in &e.sites {
            let open = slots.get(&site.callee).cloned().unwrap_or_default();
            let term = if open.is_empty() && site.constant {
                mir_ctor(
                    "Callee.constant",
                    &[
                        mir_string(&site.callee),
                        term_list(&[], &mir_name("Generic")),
                    ],
                )
            } else if open.is_empty() {
                mir_ctor("Callee.body", &[mir_string(&site.callee)])
            } else {
                let bound = bind(site);
                let generics: Vec<String> = open
                    .iter()
                    .map(|slot| match at(slot, &bound) {
                        At::Fixed(g) => g,
                        At::Caller(s) => mir_ctor(
                            "Generic.slot",
                            &[mine
                                .iter()
                                .position(|m| *m == s)
                                .unwrap_or_default()
                                .to_string()],
                        ),
                        At::Unknown(t) => {
                            if matches!(slot, Slot::Layout(_)) {
                                unread.insert(t.clone());
                            }
                            mir_ctor("Generic.unknown", &[mir_string(&t)])
                        }
                    })
                    .collect();
                mir_ctor(
                    if site.constant {
                        "Callee.constant"
                    } else {
                        "Callee.instance"
                    },
                    &[
                        mir_string(&site.callee),
                        term_list(&generics, &mir_name("Generic")),
                    ],
                )
            };
            out.insert(site.token.clone(), term);
        }
    }
    (out, unread, slots)
}

/// [`instances`]' answer.
type Instances = (
    BTreeMap<String, String>,
    BTreeSet<String>,
    BTreeMap<String, Vec<Slot>>,
);

/// A body's term with each call's token written as its callee.
fn write_callees(term: &str, callees: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(term.len());
    let mut rest = term;
    while let Some(at) = rest.find('\u{1}') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find('\u{1}').unwrap_or(after.len());
        let token = &rest[at..at + end + 2];
        out.push_str(callees.get(token).map_or(token, String::as_str));
        rest = &after[(end + 1).min(after.len())..];
    }
    out.push_str(rest);
    out
}

/// Each module's embedded bodies, by κ.
type Modules = Vec<(String, Vec<(String, Embedded)>)>;

fn embed_census(
    krate: &str,
    builds_on: &[String],
    root: &Path,
    dumps: &Path,
    only: &[String],
) -> Result<(Modules, EmbedCensus, Program), UorustError> {
    let (modules, census, program, _) = embed_linked(krate, builds_on, root, dumps, only, &[])?;
    Ok((modules, census, program))
}

/// A library crate's program linked into a crate's: its link, which
/// resolves the calls the crate makes into it, and its bodies, keyed and
/// naming each other under the library's name (`core::num::<impl
/// usize>::wrapping_sub`), as the crate's calls print them.
pub(crate) struct Linked {
    krate: String,
    link: crate::mir::link::CoreLink,
    every: BTreeMap<String, (String, Embedded)>,
}

/// The program of the library crate `krate` (core, alloc), dumped under
/// `dumps`, to link into a crate's ([`Linked`]), with the libraries it
/// builds on (`earlier`: alloc's core) linked into it: every body of its
/// own embedded, its key and every key of its own its terms name prefixed
/// with the crate's name.
pub(crate) fn link_library(
    krate: &str,
    root: &Path,
    dumps: &Path,
    earlier: &[Linked],
) -> Result<Linked, UorustError> {
    // No module of it is the census's: its bodies are the program's alone.
    let none = ["\u{0}".to_string()];
    let (_, _, program, link) = embed_linked(krate, &[], root, dumps, &none, earlier)?;
    let theirs = |k: &str| earlier.iter().any(|l| l.every.contains_key(k));
    let own: Vec<(String, (String, Embedded))> = program
        .every
        .into_iter()
        .filter(|(k, _)| !theirs(k))
        .collect();
    let renamed: BTreeMap<String, String> = own
        .iter()
        .map(|(k, _)| (mir_string(k), mir_string(&format!("{krate}::{k}"))))
        .collect();
    let mine: BTreeSet<String> = own.iter().map(|(k, _)| k.clone()).collect();
    let every = own
        .into_iter()
        .map(|(k, (kappa, mut e))| {
            e.term = rename_strings(&e.term, &renamed);
            e.callees = e
                .callees
                .iter()
                .map(|c| {
                    if mine.contains(c) {
                        format!("{krate}::{c}")
                    } else {
                        c.clone()
                    }
                })
                .collect();
            if let Cost::Bounded { calls, .. } = &mut e.cost {
                for (c, _, _) in calls.iter_mut() {
                    if mine.contains(c.as_str()) {
                        *c = format!("{krate}::{c}");
                    }
                }
            }
            e.key = format!("{krate}::{}", e.key);
            // Its calls are written: no site is left to bind.
            e.sites.clear();
            (format!("{krate}::{k}"), (kappa, e))
        })
        .collect();
    Ok(Linked {
        krate: krate.to_string(),
        link,
        every,
    })
}

/// `term` with each string literal `renamed` names (as [`mir_string`]
/// writes it) replaced.
fn rename_strings(term: &str, renamed: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(term.len());
    let bytes = term.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            let next = term[i..].find('"').map_or(term.len(), |n| i + n);
            out.push_str(&term[i..next]);
            i = next;
            continue;
        }
        let mut j = i + 1;
        while j < bytes.len() && bytes[j] != b'"' {
            j += if bytes[j] == b'\\' { 2 } else { 1 };
        }
        let end = (j + 1).min(bytes.len());
        let literal = &term[i..end];
        // Only where a term names a body: an intrinsic's name, a text or a
        // dictionary's name that a key spells is no key.
        let before = term[..i].trim_end();
        let names_body = before.ends_with("path :=")
            || [
                "Callee.body",
                "Callee.instance",
                "Callee.constant",
                "Const.function",
            ]
            .iter()
            .any(|c| before.ends_with(c));
        out.push_str(
            renamed
                .get(literal)
                .filter(|_| names_body)
                .map_or(literal, String::as_str),
        );
        i = end;
    }
    out
}

/// [`embed_census`] with the library crates `linked` linked into the
/// program: a call the crate's own link leaves unresolved that names one
/// of them (`core::...`) is resolved by its link to its body, under its
/// name; its link is returned too.
fn embed_linked(
    krate: &str,
    builds_on: &[String],
    root: &Path,
    dumps: &Path,
    only: &[String],
    linked: &[Linked],
) -> Result<(Modules, EmbedCensus, Program, crate::mir::link::CoreLink), UorustError> {
    let (width, big_endian) = dump_target(dumps)?;
    let dirs: Vec<std::path::PathBuf> = dir_paths(dumps)?
        .into_iter()
        .filter(|p| p.is_dir())
        .collect();
    let mut read: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for dir in dirs {
        let module = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let files = dir_paths(&dir)?;
        let mut texts = Vec::new();
        for file in &files {
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            texts.push((name, read_dump(file)?));
        }
        read.push((module, texts));
    }
    let link = crate::mir::link::CoreLink::new(
        // A crate's own declarations with those of the crates it builds on:
        // its bodies make values of their enums.
        if krate == "core" {
            crate::mir::core::core_decls(root)
        } else {
            let mut krates = vec!["core", "alloc", krate];
            krates.extend(builds_on.iter().map(String::as_str));
            crate::mir::core::library_decls(root, &krates)
        },
        root,
        read.iter()
            .flat_map(|(_, texts)| texts.iter().map(|(n, t)| (n.as_str(), t))),
    );
    // The enums of the crates it builds on are theirs by their crate's
    // name (`hashbrown::TryReserveError`), as its bodies print them, and
    // not by a path of its own.
    // So are core's and alloc's (`core::io::ErrorKind`, not serde_json's
    // own `io::core::ErrorKind`), for such a crate.
    let mut enums = if builds_on.is_empty() {
        enums(&link.decls)
    } else {
        enums(&crate::mir::core::library_decls(root, &[krate]))
    };
    let library = ["core", "alloc"].map(String::from);
    let theirs: &[String] = if builds_on.is_empty() { &[] } else { &library };
    for dep in theirs.iter().chain(builds_on) {
        let decls = crate::mir::core::library_decls(root, &[dep.as_str()]);
        let mut by_path: BTreeMap<String, Vec<(String, i128)>> =
            self::enums(&decls).into_values().flatten().collect();
        // An enum by each path a `use` re-exports it at too
        // (`aho_corasick::MatchKind` of `util::search::MatchKind`), a
        // re-export of a re-export as well.
        // Each round adds a path no round had, of finitely many uses.
        loop {
            let more: Vec<(String, Vec<(String, i128)>)> = decls
                .uses()
                .filter_map(|(module, name, target)| {
                    let at = if module.is_empty() {
                        name.to_string()
                    } else {
                        format!("{module}::{name}")
                    };
                    let variants = by_path.get(target)?;
                    (!by_path.contains_key(&at)).then(|| (at, variants.clone()))
                })
                .collect();
            if more.is_empty() {
                break;
            }
            by_path.extend(more);
        }
        for (path, variants) in by_path {
            enums
                .entry(last_segment(&path))
                .or_default()
                .push((format!("{dep}::{path}"), variants));
        }
    }
    // The enums of a crate the crate's source mounts as a module of its own
    // (std's `#[path = "../../backtrace/src/lib.rs"] mod backtrace_rs;`),
    // by their paths under that module.
    for (module, mounted) in mounted_crates(root, krate) {
        let decls = crate::mir::core::library_decls(root, &[mounted.as_str()]);
        for (last, found) in self::enums(&decls) {
            let entry = enums.entry(last).or_default();
            entry.extend(
                found
                    .into_iter()
                    .map(|(path, variants)| (format!("{module}::{path}"), variants)),
            );
        }
    }
    let structs = structs(&link.decls);
    let layouts = crate::layouts::Index::new(crate::layouts::load(dumps)?);
    // A float intrinsic whose result the platform gives is a call the
    // program leaves to its dictionary, as a foreign function is: the
    // evaluator is given the platform's, and stops unlinked without it.
    let platform = platform_intrinsics(&library_root(root));
    let into_linked = |callee: &str, args: &[Option<String>], dest: &str| {
        linked.iter().find_map(|l| {
            let rest = callee.strip_prefix(&format!("{}::", l.krate))?;
            match resolve_callee(&l.link, &l.krate, rest, args, dest) {
                Callee::Body(k) => Some(Callee::Body(format!("{}::{k}", l.krate))),
                Callee::Intrinsic(p) => Some(Callee::Intrinsic(p)),
                Callee::Bound(_) => None,
            }
        })
    };
    let resolve = |callee: &str, args: &[Option<String>], dest: &str| match match resolve_callee(
        &link, krate, callee, args, dest,
    ) {
        Callee::Bound(name) => into_linked(callee, args, dest).unwrap_or(Callee::Bound(name)),
        c => c,
    } {
        Callee::Intrinsic(p)
            if (platform.contains(&intrinsic_name(&p))
                || PLATFORM_EFFECTS.contains(&intrinsic_name(&p).as_str()))
                && !defined_intrinsics().contains(&intrinsic_name(&p)) =>
        {
            Callee::Bound(intrinsic_name(&p))
        }
        c => c,
    };
    let mut census = EmbedCensus::default();
    // A constant item no dump holds, as its declaration in the crate's
    // source gives it (`CoreLink::constant`), with its text.
    let declared = |printed: &str| -> Option<(IrBody, String)> {
        // A constant item of a linked library (`core::num::<impl
        // usize>::MAX`), as its own source declares it.
        let instance = link.constant(printed).or_else(|| {
            linked.iter().find_map(|l| {
                l.link
                    .constant(printed.strip_prefix(&format!("{}::", l.krate))?)
            })
        })?;
        let mut b = crate::mir::parse::bodies(&instance.text)
            .into_iter()
            .next()?;
        b.path = printed.to_string();
        Some((b, instance.text))
    };
    let source_constant = |printed: &str| declared(printed).map(|(b, _)| b);
    let typed = |key: &str| {
        item_type(&link, key)
            .or_else(|| source_constant(key)?.locals.first().cloned())
            .or_else(|| link.trait_constant_type(key))
    };
    let mut modules = Vec::new();
    let mut every: BTreeMap<String, (String, Embedded)> = BTreeMap::new();
    let mut samples = Vec::new();
    let mut signatures = BTreeMap::new();
    let package = library_root(root).join(krate);
    let lib = package.join(crate::layouts::library_file(&package)?);
    let sources = crate::mir::site::Sources::new(root);
    // Core's, alloc's and std's samples are their primitives' methods; a
    // crate of crates.io's are its own functions over scalars.
    let of_library = ["core", "alloc", "std"].contains(&krate);
    let statics = statics(&link);
    let globals = global_statics(&link);
    let static_allocs: BTreeMap<String, String> = read
        .iter()
        .flat_map(|(_, texts)| texts.iter())
        .flat_map(|(_, t)| {
            let owner = dumped_path(t).unwrap_or_default();
            t.lines().map(move |l| (owner, l))
        })
        .filter_map(|(owner, l)| {
            if let Some((alloc, rest)) = l.split_once(" (extern static: ") {
                let named = rest.split([',', ')']).next()?.trim();
                return Some((alloc.trim().to_string(), named.to_string()));
            }
            let (alloc, rest) = l.split_once(" (static: ")?;
            // The path ends at the allocation's size, past any comma of
            // its generic arguments (`Pool::<T, F>::f::X`).
            let named = rest
                .split_once(", size: ")
                .map_or_else(|| rest.split([',', ')']).next(), |(n, _)| Some(n))?
                .trim();
            nearest_static(&statics, named, owner).map(|p| (alloc.trim().to_string(), p))
        })
        .collect();
    let externs: BTreeSet<String> = read
        .iter()
        .flat_map(|(_, texts)| texts.iter())
        .flat_map(|(_, t)| t.lines())
        .filter_map(|l| {
            let rest = l.split_once(" (extern static: ")?.1;
            Some(rest.split([',', ')']).next()?.trim().to_string())
        })
        .collect();
    let every_cell: BTreeMap<String, Vec<Cell>> = read
        .iter()
        .flat_map(|(_, texts)| texts.iter())
        .flat_map(|(_, t)| allocation_cells(t, width))
        .collect();
    let floats = crate::mir::float::formats(&library_root(root));
    let parts = parts(
        krate,
        &link,
        read.iter()
            .flat_map(|(_, texts)| texts.iter().map(|(n, t)| (n.as_str(), t.as_str()))),
    );
    for (module, texts) in &read {
        let selected = only.is_empty() || only.contains(module);
        let mut bodies = Vec::new();
        for (name, text) in texts {
            let stem = dump_stem(name);
            let cx = Cx {
                width,
                enums: &enums,
                resolve: &resolve,
                allocs: allocations(text),
                big_endian,
                structs: &structs,
                cells: allocation_cells(text, width),
                items: Default::default(),
                base: Default::default(),
                parts: &parts,
                stem: stem.to_string(),
                owner: Default::default(),
                statics: &statics,
                floats: &floats,
                locals: Default::default(),
                item_type: &typed,
                layouts: &layouts,
                source_constant: &source_constant,
                every_cell: &every_cell,
                static_allocs: &static_allocs,
                externs: &externs,
                globals: &globals,
                slots: Default::default(),
                sites: Default::default(),
                item_texts: Default::default(),
                key: Default::default(),
                block: Default::default(),
                sources: &sources,
                declared: Default::default(),
            };
            // A static's initializer is a body of the program, read where a
            // body reads the static, and no row of the census.
            let initializer = static_body(text).filter(|b| statics.contains(&b.path));
            let is_static = initializer.is_some();
            for b in crate::mir::parse::bodies(text)
                .into_iter()
                .chain(initializer)
            {
                let kappa = body_kappa(krate, &b.path, text);
                let key = match parts.get(stem).filter(|_| is_part(&b.path)) {
                    Some(key) => key.clone(),
                    None => {
                        program_key(&link, dumped_path(text).unwrap_or(&b.path), &b.path, &kappa)
                    }
                };
                let e = cx.body(&b, &key);
                if selected && has_row(&b.path) && !is_static {
                    census.add(&e);
                    signatures.insert(
                        kappa.clone(),
                        (
                            key.clone(),
                            b.locals.iter().take(b.arity + 1).cloned().collect(),
                        ),
                    );
                    let sampled = if of_library {
                        sample(root, &lib, &b, &key, &kappa, width, &floats)
                    } else {
                        crate_sample(stem, &package, &lib, &b, &key, &kappa, width, &floats)
                    };
                    if let Some(sample) = sampled {
                        samples.push(sample);
                    }
                }
                every.insert(key, (kappa.clone(), e.clone()));
                if selected {
                    bodies.push((kappa, e));
                }
            }
        }
        if !selected {
            continue;
        }
        // Two bodies of one κ (a macro's impls of one text) are one
        // definition.
        bodies.sort_by(|a, b| a.0.cmp(&b.0));
        bodies.dedup_by(|a, b| a.0 == b.0);
        modules.push((module.clone(), bodies));
    }
    // The linked libraries' bodies, which the crate's calls into them
    // name.
    for l in linked {
        for (k, v) in &l.every {
            every.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    let unread = {
        let cx = Cx {
            width,
            enums: &enums,
            resolve: &resolve,
            allocs: BTreeMap::new(),
            big_endian,
            structs: &structs,
            cells: BTreeMap::new(),
            items: Default::default(),
            base: Default::default(),
            parts: &parts,
            stem: String::new(),
            owner: Default::default(),
            statics: &statics,
            floats: &floats,
            locals: Default::default(),
            item_type: &typed,
            layouts: &layouts,
            source_constant: &source_constant,
            every_cell: &every_cell,
            static_allocs: &static_allocs,
            externs: &externs,
            globals: &globals,
            slots: Default::default(),
            sites: Default::default(),
            item_texts: Default::default(),
            key: Default::default(),
            block: Default::default(),
            sources: &sources,
            declared: Default::default(),
        };
        // The constant items the bodies read that no dump holds: each a
        // body of the program, as its declaration gives it.
        let wanted: BTreeSet<String> = every
            .values()
            .flat_map(|(_, e)| e.callees.iter())
            .filter(|k| !every.contains_key(*k))
            .cloned()
            .collect();
        for key in wanted {
            if let Some((b, text)) = declared(&key) {
                let e = cx.body(&b, &key);
                every.insert(key.clone(), (body_kappa(krate, &key, &text), e));
            }
        }
        let others: Vec<&Embedded> = modules
            .iter()
            .flat_map(|(_, bodies)| bodies.iter().map(|(_, e)| e))
            .collect();
        let links: Vec<(&str, &crate::mir::link::CoreLink)> =
            linked.iter().map(|l| (l.krate.as_str(), &l.link)).collect();
        let (callees, found, slots) = instances(&cx, &link, &links, &every, &others);
        for (k, (_, e)) in every.iter_mut() {
            e.term = write_callees(&e.term, &callees);
            // Its slots with those its callees leave it: a caller in a
            // program this one is linked into supplies them all.
            if let Some(s) = slots.get(k) {
                e.slots = s.clone();
            }
        }
        for (_, bodies) in &mut modules {
            for (_, e) in bodies.iter_mut() {
                e.term = write_callees(&e.term, &callees);
            }
        }
        found
    };
    // A sample is checked where the evaluator defines its whole closure:
    // every body it reaches is embedded, uses no construct the evaluator
    // does not define, calls no intrinsic and leaves no callee to a
    // dictionary.
    let mut left: BTreeMap<String, usize> = BTreeMap::new();
    let sampled = samples.len();
    samples.retain_mut(|s| match closure(&every, &s.key) {
        Reach::Closed(c) => match derived_fuel(&every, &s.key) {
            Fuel::Bounded(fuel, _) => {
                s.closure = c;
                s.fuel = fuel;
                true
            }
            Fuel::Unbounded(why) => {
                *left.entry(why).or_default() += 1;
                false
            }
        },
        Reach::Open(why) => {
            *left.entry(why).or_default() += 1;
            false
        }
    });
    if sampled > 0 {
        eprintln!(
            "differential: {} of {sampled} sampled bodies checked{}",
            samples.len(),
            left.iter()
                .map(|(why, n)| format!("; {n} left out: {why}"))
                .collect::<String>()
        );
    }
    let program = Program {
        every,
        samples,
        width,
        big_endian: big_endian.unwrap_or_default(),
        unread,
        signatures,
        globals: globals.len(),
    };
    Ok((modules, census, program, link))
}

#[cfg(test)]
mod tests {
    // SEC-08: the one function every embedding and bridge write goes through
    // never removes a checkout.
    #[test]
    fn replace_dir_refuses_a_checkout() {
        let dir = std::env::temp_dir().join(format!("uorust-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).expect("checkout");
        assert_eq!(
            super::replace_dir(&dir, None),
            Err(crate::error::UorustError::ForeignOutput(dir.clone()))
        );
        assert!(dir.join(".git").is_dir());
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// The embedding stamps each row by its κ, after its span, in place of
    /// what the row recorded, and leaves a row it has no body of unstamped.
    #[test]
    fn inline_assembly_is_a_call_its_dictionary_answers() {
        let asm = inline_asm("asm!(\"syscall\", inlateout(\"ax\") move _9 => _7, in(\"di\") move _11, lateout(\"cx\") _, options(PRESERVES_FLAGS | NOSTACK)) -> [return: bb1, unwind unreachable]")
            .expect("a syscall");
        assert_eq!(
            asm.name,
            "asm!(\"syscall\", inlateout(\"ax\"), in(\"di\"), lateout(\"cx\"), options(PRESERVES_FLAGS | NOSTACK))"
        );
        assert_eq!(asm.inputs.len(), 2);
        assert_eq!(asm.output.local, 7);
        assert_eq!(asm.target, Some(1));
        assert!(inline_asm("asm!(\"\", const 1_i32, out(reg) _1) -> bb1").is_none());
        assert!(inline_asm("asm!(\"\", out(reg) _1, out(reg) _2) -> bb1").is_none());
    }

    #[test]
    fn a_pattern_constant_is_its_base_integer() {
        assert_eq!(
            pattern_int("65536_i32 is (i32::MIN..=-1 | 1..)", 64),
            Some((65536, 32, true))
        );
        assert_eq!(pattern_int("1_usize is 1..", 64), Some((1, 64, false)));
        assert_eq!(pattern_int("65536_i32", 64), None);
    }

    #[test]
    fn an_array_of_a_parameter_count_is_sized() {
        assert!(super::is_array("[u8; N]"));
        assert!(super::is_array("[[u8; 2]; 3]"));
        assert!(!super::is_array("[[u8; 2]]"));
        assert!(!super::is_array("[u8]"));
    }

    #[test]
    fn stamps_each_row_by_its_kappa() {
        let blocked = super::Embedded {
            term: "t".into(),
            blocked: ["intrinsic fabs".to_string()].into(),
            ..Default::default()
        };
        let evaluable = super::Embedded {
            term: "u".into(),
            ..Default::default()
        };
        let by_kappa = [("a", &blocked), ("b", &evaluable)].into();
        let text = "[[fn]]\npath = \"core::a\"\nkappa = \"a\"\nat = \"x\"\nembedded = \"old\"\nblocked = [\"CastKind::IntToFloat\"]\nstatus = \"residue\"\ncause = \"c\"\n\n[[fn]]\npath = \"core::b\"\nkappa = \"b\"\nstatus = \"translated\"\ndigest = \"d\"\n\n[[fn]]\npath = \"core::c\"\nkappa = \"c\"\nembedded = \"old\"\nstatus = \"translated\"\ndigest = \"d\"\n";
        let (t, u) = (crate::project::sha(b"t"), crate::project::sha(b"u"));
        assert_eq!(
            super::stamp_rows(text, &by_kappa),
            format!("[[fn]]\npath = \"core::a\"\nkappa = \"a\"\nat = \"x\"\nembedded = \"{t}\"\nblocked = [\"intrinsic fabs\"]\nstatus = \"residue\"\ncause = \"c\"\n\n[[fn]]\npath = \"core::b\"\nkappa = \"b\"\nembedded = \"{u}\"\nstatus = \"translated\"\ndigest = \"d\"\n\n[[fn]]\npath = \"core::c\"\nkappa = \"c\"\nstatus = \"translated\"\ndigest = \"d\"\n")
        );
    }

    #[test]
    fn reads_a_subslice() {
        assert_eq!(super::subslice("1:"), Some((1, 0, true)));
        assert_eq!(super::subslice(":-1"), Some((0, 1, true)));
        assert_eq!(super::subslice("2:-3"), Some((2, 3, true)));
        assert_eq!(super::subslice("1..3"), Some((1, 3, false)));
        assert_eq!(super::subslice("x"), None);
    }

    #[test]
    fn names_an_item_by_a_trimmed_path() {
        assert!(super::same_item(
            "atomic::Ordering",
            "sync::atomic::Ordering"
        ));
        assert!(!super::same_item("cmp::Ordering", "sync::atomic::Ordering"));
    }

    use super::*;

    /// A function pointer is one however its ABI and safety print.
    #[test]
    fn a_function_pointer_is_one_however_it_prints() {
        for ty in [
            "fn(u8) -> u8",
            "unsafe fn(u8, *const u8) -> Option<*const u8>",
            "extern \"C\" fn()",
            "unsafe extern \"C\" fn(i32)",
        ] {
            assert!(fn_pointer(ty), "{ty}");
        }
        assert!(!fn_pointer("unsafe_cell::UnsafeCell<u8>"));
    }

    /// A transmute's work is read from its layouts' terms: a `[u8; 4]`
    /// to a `u32` encodes the array and its four bytes, and decodes one
    /// integer.
    #[test]
    fn a_transmutes_work_is_read_from_its_layouts() {
        let byte = mir_ctor("Layout.int", &["1".into(), "false".into()]);
        let array = mir_ctor("Layout.array", &[byte, "4".into(), "1".into()]);
        let word = mir_ctor("Layout.int", &["4".into(), "false".into()]);
        let term = mir_ctor(
            "Rvalue.transmute",
            &[
                mir_ctor("Generic.layout", std::slice::from_ref(&array)),
                mir_ctor("Generic.layout", &[word]),
                "x".into(),
            ],
        );
        assert_eq!(layout_pieces(&sexp(&array).unwrap()), Some(5));
        assert_eq!(layout_size(&sexp(&array).unwrap()), Some(4));
        assert_eq!(transmute_work(&term), Some(6));
        let slot = mir_ctor("Callee.rawEq", &[mir_ctor("Generic.slot", &["0".into()])]);
        assert_eq!(transmute_work(&slot), None);
    }

    /// A sample's fuel is its steps and each callee's for each call, and
    /// a body that calls itself has none.
    #[test]
    fn a_samples_fuel_counts_each_call_of_each_callee() {
        let body = |calls: Vec<(String, u128, bool)>| Embedded {
            cost: Cost::Bounded {
                steps: 5,
                calls,
                work: 0,
                pin: Some(3),
            },
            ..Default::default()
        };
        let mut every = BTreeMap::from([
            (
                "f".to_string(),
                (
                    String::new(),
                    body(vec![("g".into(), 4, false), ("c".into(), 1, true)]),
                ),
            ),
            ("g".to_string(), (String::new(), body(Vec::new()))),
            ("c".to_string(), (String::new(), body(Vec::new()))),
        ]);
        // 5 own, 4 runs of g's 5, c's 5, and c's value pinned past them.
        assert!(matches!(derived_fuel(&every, "f"), Fuel::Bounded(33, 0)));
        every.insert(
            "g".into(),
            (String::new(), body(vec![("f".into(), 1, false)])),
        );
        assert!(matches!(derived_fuel(&every, "f"), Fuel::Unbounded(_)));
    }

    /// A linked library's keys are renamed in its terms as whole string
    /// literals: a literal that only holds a key, or is another's, is not.
    #[test]
    fn a_linked_librarys_keys_are_renamed_as_whole_literals() {
        let renamed = BTreeMap::from([(mir_string("num::f"), mir_string("core::num::f"))]);
        let term = format!(
            "(ctor Callee.body {}) (ctor X {}) (ctor Y {}) (ctor Callee.intrinsic {})",
            mir_string("num::f"),
            mir_string("num::f::g"),
            mir_string("q\"num::f"),
            mir_string("num::f")
        );
        assert_eq!(
            rename_strings(&term, &renamed),
            format!(
                "(ctor Callee.body {}) (ctor X {}) (ctor Y {}) (ctor Callee.intrinsic {})",
                mir_string("core::num::f"),
                mir_string("num::f::g"),
                mir_string("q\"num::f"),
                mir_string("num::f")
            )
        );
    }

    /// An impl's header is read from the span MIR names it by.
    #[test]
    fn an_impls_header_is_read_from_its_span() {
        let dir = std::env::temp_dir().join(format!("uorust-span-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "struct A;\nimpl Tr for\n    u32 {\n}\n",
        )
        .unwrap();
        let (file, text) = span_text(&dir, "src/lib.rs:2:1: 3:8").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(file, "src/lib.rs");
        assert_eq!(
            text.split_whitespace().collect::<Vec<_>>(),
            ["impl", "Tr", "for", "u32"]
        );
    }

    /// The constructors of the inductive `name` in the semantics.
    fn constructors(name: &str) -> Vec<String> {
        let head = format!("inductive{{\n  name: {name},");
        let at = SEMANTICS.find(&head).expect("the inductive");
        let rest = &SEMANTICS[at..];
        let end = rest.find("\n}\n").expect("its end");
        rest[..end]
            .match_indices("{name: ")
            .map(|(i, _)| {
                let r = &rest[i + "{name: ".len()..];
                r[..r.find(',').expect("a name")].to_string()
            })
            .collect()
    }

    #[test]
    fn the_datatype_kinds_are_rustcs_kinds_ue_19() {
        let check = |name: &str, paths: Vec<&str>| {
            let want: Vec<String> = paths
                .iter()
                .map(|p| kind_ctor(p).split_once('.').expect("a kind").1.to_string())
                .collect();
            assert_eq!(constructors(name), want, "{name}");
        };
        check(
            "BinOp",
            spec::MirBinOp::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "UnOp",
            spec::MirUnOp::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "CastKind",
            spec::MirCastKind::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "BorrowKind",
            spec::MirBorrowKind::ALL.iter().map(|k| k.path()).collect(),
        );
        check(
            "RuntimeChecks",
            spec::MirRuntimeChecks::ALL
                .iter()
                .map(|k| k.path())
                .collect(),
        );
        check(
            "AssertKind",
            spec::MirAssertKind::ALL.iter().map(|k| k.path()).collect(),
        );
    }

    #[test]
    fn the_undefined_constructs_are_read_from_the_evaluator_ue_19() {
        let undefined = undefined_constructs();
        assert!(undefined.contains("BinOp::Offset"));
        assert!(undefined.contains("CastKind::Transmute"));
        assert!(!undefined.contains("BinOp::Add"));
    }

    #[test]
    fn a_long_list_nests_no_deeper_than_its_bit_length_ue_19() {
        let items: Vec<String> = (0..10_000).map(|i| i.to_string()).collect();
        let term = term_list(&items, "Nat");
        let (mut depth, mut deepest) = (0usize, 0usize);
        for c in term.chars() {
            match c {
                '(' => {
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                ')' => depth -= 1,
                _ => {}
            }
        }
        let bits = (usize::BITS - items.len().leading_zeros()) as usize;
        assert!(deepest <= 2 * (bits + 1), "{deepest}");
        assert_eq!(term.matches("(cons ").count(), items.len());
    }

    #[test]
    fn byte_strings_allocations_and_repeats_are_read_ue_19() {
        assert_eq!(
            byte_string(r#"b"a\x00\n\"""#),
            Some(vec![b'a', 0, b'\n', b'"'])
        );
        assert_eq!(byte_string("\"a\""), None);
        let text = "fn f() -> () {\n}\n\nalloc7 (static: T, size: 4, align: 2) {\n    0x0 \u{2502} 01 00 02 00 \u{2502} ....\n}\n\nalloc8 (size: 2, align: 1) {\n    __ 01 \u{2502} .\n}\n";
        let allocs = allocations(text);
        assert_eq!(allocs.get("alloc7"), Some(&vec![1, 0, 2, 0]));
        assert!(!allocs.contains_key("alloc8"));
        assert_eq!(repeat_parts("[const 0_u8; 32]"), Some(("const 0_u8", "32")));
        assert_eq!(
            repeat_parts("[move _3; 4_usize]"),
            Some(("move _3", "4_usize"))
        );
        assert_eq!(count_value("4_usize"), Some(4));
        assert_eq!(count_value("N"), None);
    }

    #[test]
    fn an_allocations_pointers_and_a_constants_item_are_read_ue_19() {
        let text = "alloc2 (static: X, size: 8, align: 8) {\n    \u{257e}\u{2500}\u{2500}alloc1\u{2500}\u{2500}\u{257c} \u{2502} \u{257e}\u{2500}\u{257c}\n}\n\nalloc1 (size: 2, align: 1) {\n    61 62 \u{2502} ab\n}\n";
        let cells = allocation_cells(text, 64);
        assert_eq!(cells["alloc1"], vec![Cell::Byte(0x61), Cell::Byte(0x62)]);
        assert_eq!(cells["alloc2"].len(), 8);
        assert_eq!(cells["alloc2"][7], Cell::Pointer("alloc1".into(), 7));
        assert_eq!(
            item_suffix("a::f::promoted[0]"),
            Some(("a::f", "promoted[0]"))
        );
        assert_eq!(
            item_suffix("a::f::{constant#1}"),
            Some(("a::f", "{constant#1}"))
        );
        assert_eq!(
            item_suffix("a::f::CHUNK_SIZE"),
            Some(("a::f", "CHUNK_SIZE"))
        );
        assert_eq!(item_suffix("a::f::g"), None);
        assert_eq!(
            generic_parts("NonNull<[T; N], U>"),
            ("NonNull", vec!["[T; N]".to_string(), "U".to_string()])
        );
        assert_eq!(pointee("&'a mut T"), Some("T"));
        assert_eq!(pointee("* const T"), Some("T"));
        let structs = Structs::new();
        let enums = Enums::new();
        let floats = BTreeMap::from([(
            "f32".to_string(),
            crate::mir::float::Format { exp: 8, frac: 23 },
        )]);
        let resolve = |_: &str, _: &[Option<String>], _: &str| Callee::Bound(String::new());
        let sources = crate::mir::site::Sources::new(Path::new(""));
        let cx = Cx {
            width: 64,
            enums: &enums,
            resolve: &resolve,
            allocs: BTreeMap::new(),
            big_endian: Some(false),
            structs: &structs,
            cells: BTreeMap::new(),
            items: Default::default(),
            base: Default::default(),
            parts: &BTreeMap::new(),
            stem: String::new(),
            owner: Default::default(),
            statics: &BTreeSet::new(),
            floats: &floats,
            locals: Default::default(),
            item_type: &|_: &str| None,
            layouts: &Default::default(),
            source_constant: &|_| None,
            every_cell: &BTreeMap::new(),
            static_allocs: &BTreeMap::new(),
            externs: &BTreeSet::new(),
            globals: &[],
            slots: Default::default(),
            sites: Default::default(),
            item_texts: Default::default(),
            key: Default::default(),
            block: Default::default(),
            sources: &sources,
            declared: Default::default(),
        };
        assert_eq!(cx.ty("f32"), "(ctor Mir::Ty.float 8 23)");
        assert_eq!(
            cx.float_literal("+Inf_f32").map(|l| l.0),
            Some("f32".into())
        );
        assert!(cx.float_literal("f32::<impl f32>::NAN").is_some());
        assert!(cx.float_literal("1.0E+4f32").is_some());
        assert!(cx.float_literal("1.0E+4u32").is_none());
        let ranged = |t: &str| cx.layout(t, 4).map(|l| l.term);
        assert!(ranged("(u32) is 1..")
            .is_some_and(|t| t.contains("Layout.ranged") && t.contains(" 1 4294967295)")));
        assert!(ranged("(u32) is 0..=999999999").is_some_and(|t| t.contains(" 0 999999999)")));
        assert!(ranged("(i8) is (i8::MIN..=-1 | 1..)").is_some_and(|t| t.contains(" 1 255)")));
        assert!(ranged("(i8) is (i8::MIN..=-2 | 0..)").is_some_and(|t| t.contains(" 0 254)")));
        assert!(ranged("(i8) is -5..=5").is_some_and(|t| t.contains(" 251 5)")));
        assert_eq!(ranged("(u8) is 5..=1"), None);
        // A type query reads rustc's layouts: a field's offset, and
        // whether zero bytes, or 0x01 bytes, are a valid value.
        use crate::layouts::{Kept, Prim, Scalar, TypeLayout};
        let scalar = |bytes, end| TypeLayout {
            size: bytes,
            align: bytes,
            scalar: Some(Scalar {
                prim: Prim::Int {
                    bytes,
                    signed: false,
                },
                start: 0,
                end,
            }),
            offsets: Vec::new(),
            single: None,
            tag: None,
            fields: Vec::new(),
            uninhabited: false,
        };
        let pair = TypeLayout {
            size: 8,
            align: 4,
            scalar: None,
            offsets: vec![4, 0],
            single: None,
            tag: None,
            fields: vec![vec![Some("bool".into()), Some("char".into())]],
            uninhabited: false,
        };
        let kept = Kept {
            layouts: BTreeMap::from([
                ("bool".to_string(), scalar(1, 1)),
                ("char".to_string(), scalar(4, 0x10FFFF)),
                ("Pair".to_string(), pair),
            ]),
            names: BTreeMap::new(),
        };
        let index = crate::layouts::Index::new(kept);
        let cx = Cx {
            layouts: &index,
            ..cx
        };
        assert_eq!(
            cx.count_of("field_offset::<FieldRepresentingType<Pair, 0_u32, 1_u32>>"),
            Some(0)
        );
        assert_eq!(
            cx.count_of("field_offset::<FieldRepresentingType<Pair, 1_u32, 1_u32>>"),
            None
        );
        assert_eq!(cx.count_of("assert_zero_valid::<Pair>"), Some(1));
        assert_eq!(
            cx.count_of("assert_mem_uninitialized_valid::<bool>"),
            Some(1)
        );
        assert_eq!(
            cx.count_of("assert_mem_uninitialized_valid::<Pair>"),
            Some(0)
        );
        assert_eq!(cx.count_of("assert_zero_valid::<&Pair>"), None);
        assert_eq!(cx.count_of("assert_zero_valid::<Unknown>"), None);
        let metadata =
            |t: &str| cx.pointee_metadata(&format!("<{t} as ptr::metadata::Pointee>::Metadata"));
        assert_eq!(metadata("[u8]").as_deref(), Some("usize"));
        assert_eq!(metadata("str").as_deref(), Some("usize"));
        assert_eq!(cx.pointee_metadata("<[u8] as Other>::Metadata"), None);
        assert!(intrinsic_gaps("aggregate_raw_ptr")
            .iter()
            .any(|g| g.contains("subrange")));
        assert_eq!(intrinsic_gaps("vtable_align").len(), 1);
        assert!(intrinsic_gaps("copy").is_empty());
    }

    #[test]
    fn a_part_is_named_by_its_owners_dump_ue_19() {
        assert_eq!(
            dump_stem("core.net-ip_addr-{impl#3}-is_broadcast-promoted[0].2-2-009.ElaborateBoxDerefs.before.mir"),
            "core.net-ip_addr-{impl#3}-is_broadcast-promoted[0]"
        );
        assert!(is_part("a::f::promoted[1]") && is_part("a::f::{constant#0}"));
        assert!(!is_part("a::f::{closure#0}"));
        assert_eq!(last_segment("<Self as I>::all::<F>"), "all");
        assert_eq!(
            last_segment("<R as D<bool>>::sample::<impl RandomSource + ?Sized>"),
            "sample"
        );
    }

    #[test]
    fn a_static_is_read_as_its_initializer_ue_19() {
        let text = "// MIR for `a::b::MAP` before ElaborateBoxDerefs\n\nstatic MAP: [u8; 2] = {\n    let mut _0: [u8; 2];\n\n    bb0: {\n        _0 = [const 1_u8, const 2_u8];\n        return;\n    }\n}\n";
        let body = static_body(text).expect("the initializer");
        assert_eq!(body.path, "a::b::MAP");
        assert_eq!(body.arity, 0);
        // A `static mut`'s initializer is its global's initial value.
        let mutable = text.replace("static MAP", "static mut MAP");
        assert_eq!(
            static_body(&mutable).map(|b| b.path),
            Some("a::b::MAP".to_string())
        );
        assert_eq!(
            static_path("<static(DefId(0:54115 ~ core[6af5]::a::b::MAP))>"),
            Some("a::b::MAP")
        );
    }

    #[test]
    fn a_harness_output_without_its_checks_is_refused_ue_19() {
        let program = Program {
            every: BTreeMap::new(),
            samples: Vec::new(),
            width: usize::BITS,
            big_endian: false,
            unread: BTreeSet::new(),
            signatures: BTreeMap::new(),
            globals: 0,
        };
        let file = Path::new("observed.txt");
        assert!(matches!(
            differential(&program, file, "0 1\n"),
            Err(UorustError::Malformed { .. })
        ));
        assert!(differential(&program, file, "checks false true\n").is_ok());
    }

    #[test]
    fn a_string_keeps_every_character_ue_19() {
        assert_eq!(mir_string("a%b"), "\"a\\u0025b\"");
        assert_eq!(mir_string("q\"\\\n"), "\"q\\\"\\\\\\u000a\"");
    }

    #[test]
    fn an_integer_constant_is_its_twos_complement_ue_19() {
        assert_eq!(repr(-1, 8), 255);
        assert_eq!(repr(5, 32), 5);
        assert_eq!(repr(-1, 128), u128::MAX);
        assert_eq!(int_ty("usize", 64), Some((64, false)));
        assert_eq!(int_ty("i16", 64), Some((16, true)));
        assert_eq!(int_ty("u", 64), None);
    }
}

#[cfg(test)]
mod platform_tests {
    /// An intrinsic is the platform's where its stabilized version is
    /// documented under "Unspecified precision", and not otherwise.
    #[test]
    fn platform_intrinsics_read_their_stabilized_docs() {
        let dir = std::env::temp_dir().join(format!("uorust-platform-{}", std::process::id()));
        let w = |p: &str, t: &str| {
            let p = dir.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        w(
            "core/src/intrinsics/mod.rs",
            "/// Sine.\n///\n/// The stabilized version of this intrinsic is\n/// [`f32::sin`](x)\n#[rustc_intrinsic]\npub fn sinf32(x: f32) -> f32;\n/// Root.\n///\n/// The stabilized version of this intrinsic is\n/// [`f32::sqrt`](x)\n#[rustc_intrinsic]\npub fn sqrtf32(x: f32) -> f32;\n",
        );
        w(
            "std/src/num/f32.rs",
            "    /// # Unspecified precision\n    #[inline]\n    pub fn sin(self) -> f32 {}\n    /// Exact.\n    pub fn sqrt(self) -> f32 {}\n",
        );
        let got = super::platform_intrinsics(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            vec!["sinf32".to_string()]
        );
    }
}
