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
}

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
/// {..}`, which a constant item's body is but for its keyword. A `static
/// mut` is not read: its value is not its initializer's once written.
pub(crate) fn static_body(text: &str) -> Option<IrBody> {
    let path = text
        .lines()
        .next()?
        .strip_prefix("// MIR for `")?
        .split_once('`')?
        .0;
    let head = text.lines().find_map(|l| l.strip_prefix("static "))?;
    if head.starts_with("mut ") {
        return None;
    }
    let constant = text.replacen(&format!("static {head}"), &format!("const {head}"), 1);
    let mut body = crate::mir::parse::bodies(&constant).into_iter().next()?;
    body.path = path.to_string();
    Some(body)
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
                program_key(link, &path, &kappa)
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
    crate::mir::parse::split_path(&erased)
        .last()
        .copied()
        .unwrap_or(&erased)
        .to_string()
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

/// An intrinsic by its name: rustc names an intrinsic by its item's name,
/// whatever crate's path a call prints it by.
fn intrinsic_name(path: &str) -> String {
    crate::mir::parse::split_path(path)
        .last()
        .copied()
        .unwrap_or(path)
        .to_string()
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

/// A LexLean name of the datatype, qualified by the semantics module.
fn mir_name(name: &str) -> String {
    format!("{MIR}::{name}")
}

/// `(ctor Mir::T.c a b)`.
fn mir_ctor(name: &str, args: &[String]) -> String {
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
fn repeat_parts(text: &str) -> Option<(&str, u64)> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (operand, count) = inner.rsplit_once(';')?;
    let count = count.trim();
    let count = count.split_once('_').map_or(count, |(n, _)| n);
    Some((operand.trim(), count.parse().ok()?))
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
fn int_ty(printed: &str, width: u32) -> Option<(u32, bool)> {
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
fn mir_string(text: &str) -> String {
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
            IrOperand::Constant(c) if self.item(c).is_some() => {
                (self.item_type)(&self.item(c).unwrap_or_default())
            }
            IrOperand::Constant(IrConstant::Other(text) | IrConstant::Path(text)) => {
                self.float_literal(text).map(|(name, _)| name)
            }
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
        let mut found = candidates.iter().filter(|(path, variants)| {
            (candidates.len() == 1 || same_item(&erased, path))
                && variants.iter().any(|(v, _)| v == variant)
        });
        let (_, variants) = found.next()?;
        if found.next().is_some() {
            return None;
        }
        variants.iter().find(|(v, _)| v == variant).map(|(_, d)| *d)
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
        if ty == "str" || ty.starts_with("dyn ") || (ty.starts_with('[') && array_ty(ty).is_none())
        {
            return Some(false);
        }
        if int_ty(ty, self.width).is_some()
            || matches!(ty, "bool" | "char" | "()")
            || array_ty(ty).is_some()
            || pointee(ty).is_some()
            || ty.starts_with("fn(")
            || ty.starts_with('(')
        {
            return Some(true);
        }
        let (name, args) = generic_parts(ty);
        let name = crate::mir::parse::split_path(name)
            .last()
            .copied()
            .unwrap_or(name);
        let [decl] = self.structs.get(name)?.as_slice() else {
            return None;
        };
        if decl.variants.is_some() || decl.data.is_some() {
            return Some(true);
        }
        // A struct is unsized where its last field is.
        let Some((_, last)) = decl.fields.last() else {
            return Some(true);
        };
        let bound: BTreeMap<String, String> = decl.params.iter().cloned().zip(args).collect();
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
            let (bits, signed) = int_ty(base, self.width)?;
            // Each value as its bits and as a key in its type's order: the
            // bits with the sign bit flipped, for a signed base.
            let mask = u128::MAX >> (u128::BITS - bits);
            let sign = if signed { 1u128 << (bits - 1) } else { 0 };
            let (first, last) = (sign, sign ^ mask);
            let value = |t: &str, open: u128| -> Option<u128> {
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
        if pointee(ty).is_some() || ty.starts_with("fn(") {
            let (size, assumed) =
                match pointee(ty).map(|t| (t, self.sized(t, layout_depth(self.structs)))) {
                    None | Some((_, Some(true))) => (thin, None),
                    Some((_, Some(false))) => (2 * thin, None),
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
            if found.1.len() != variants.len() {
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
            return self.rustc_enumeration(printed, l, tag);
        }
        let (adt, fields, l) = self.rustc_fields(ty)?;
        let mut terms = Vec::new();
        for (_, t) in &fields {
            let base = t
                .trim()
                .strip_prefix('(')
                .and_then(|r| r.split_once(") is "))
                .map(|(b, _)| b);
            let own = self.layouts.get(t).and_then(|(_, f)| f.scalar.as_ref());
            let range = own.or(l.scalar.as_ref().filter(|_| fields.len() == 1));
            let laid = match (base, range) {
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
                _ => self.layout(t, depth)?,
            };
            if laid.assumed.is_some() {
                return None;
            }
            terms.push(laid.term);
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
            .collect();
        let [(_, variants)] = found.as_slice() else {
            return None;
        };
        (variants.len() == tag.variants.len()).then_some(())?;
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
        if self.statics.contains(key) {
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
        let laid = self.layout(pointee(&ptr)?, layout_depth(self.structs))?;
        laid.assumed.is_none().then_some(laid.term)
    }

    /// `Callee.transmute` of a transmute from `from` to `to`, where both
    /// layouts are read and agree in size; a pointer whose size is assumed
    /// is allowed only where both sides hold one of the same pointee, so
    /// whatever its size, the two agree.
    fn transmute(&self, from: &str, to: &str) -> Option<String> {
        let depth = layout_depth(self.structs);
        let (a, b) = (self.layout(from, depth)?, self.layout(to, depth)?);
        if a.size != b.size || a.assumed != b.assumed {
            return None;
        }
        Some(mir_ctor("Callee.transmute", &[a.term, b.term]))
    }

    /// The key of the constant item a constant operand names, where the
    /// program has its body: the item itself, or the item of that name in
    /// the body its owner resolves to.
    fn item(&self, c: &IrConstant) -> Option<String> {
        let IrConstant::Other(text) = c else {
            return None;
        };
        if let Some(path) = static_path(text).filter(|p| self.statics.contains(*p)) {
            return Some(path.to_string());
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
        (1..segments.len().saturating_sub(1)).find_map(|i| {
            match (self.resolve)(&segments[i..].join("::"), &[], "") {
                Callee::Body(p) if named.ends_with(&format!("::{p}")) => Some(p),
                _ => None,
            }
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

    /// The value of type `ty` that allocation `alloc` holds at `offset`: an
    /// integer read at the target's byte order, an array of such values,
    /// or a reference to a sized value another allocation holds.
    fn read_cells(&self, alloc: &str, offset: usize, ty: &str, depth: usize) -> Option<String> {
        let depth = depth.checked_sub(1)?;
        let cells = self.cells.get(alloc)?;
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

    fn operand(&self, o: &IrOperand, e: &mut Embedded) -> String {
        use spec::MirOperand as O;
        let (kind, term) = match o {
            IrOperand::Copy(p) => (O::Copy, mir_ctor("Operand.copy", &[self.place(p, e)])),
            IrOperand::Move(p) => (O::Move, mir_ctor("Operand.move", &[self.place(p, e)])),
            IrOperand::Constant(c) if self.item(c).is_some() => {
                let key = self.item(c).unwrap_or_default();
                e.callees.insert(key.clone());
                let mut items = self.items.borrow_mut();
                let at = items.iter().position(|k| *k == key).unwrap_or_else(|| {
                    items.push(key.clone());
                    items.len() - 1
                });
                // A static's operand is a reference to it: the local past
                // the one its value is evaluated into, which borrows it.
                let slot: usize =
                    items[..at].iter().map(|k| self.slots(k)).sum::<usize>() + self.slots(&key) - 1;
                (
                    O::Constant,
                    mir_ctor(
                        "Operand.copy",
                        &[format!(
                            "(record {} {{base := {}, projection := {}}})",
                            mir_name("Place"),
                            self.base.get() + slot,
                            term_list(&[], &mir_name("Proj"))
                        )],
                    ),
                )
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
                let (a, b) = (self.operand(a, e), self.operand(b, e));
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
                        let d = variant
                            .as_deref()
                            .and_then(|v| self.discriminant(ty, v))
                            .unwrap_or_default();
                        if variant.is_some() && self.discriminant(ty, &v).is_none() {
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
                    IrAggregate::Other(k) => {
                        e.blocked.insert(k.path().to_string());
                        (*k, mir_ctor("Agg.other", &[mir_string(k.path())]))
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
            IrRvalue::Other(spec::MirRvalue::Repeat, text)
                if repeat_parts(text)
                    .is_some_and(|(o, _)| crate::mir::parse::operand(o).is_some()) =>
            {
                let (o, n) = repeat_parts(text).unwrap_or_default();
                let operand = crate::mir::parse::operand(o).map(|o| self.operand(&o, e));
                (
                    spec::MirRvalue::Repeat,
                    mir_ctor(
                        "Rvalue.repeat",
                        &[operand.unwrap_or_default(), n.to_string()],
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

    fn terminator(&self, t: &IrTerminator, body: &IrBody, e: &mut Embedded) -> String {
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
                            mir_ctor("Callee.body", &[mir_string(&p)])
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
                (
                    T::Call,
                    mir_ctor("Terminator.call", &[destination, callee, args, target]),
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
            IrTerminator::Other(k, text) => {
                e.blocked.insert(k.path().to_string());
                (
                    *k,
                    mir_ctor(
                        "Terminator.other",
                        &[mir_string(k.path()), mir_string(text)],
                    ),
                )
            }
        };
        e.uses.insert(kind.path().to_string());
        term
    }

    /// The body `b`, named `path` in the program, as a `Mir::Body` term.
    pub(crate) fn body(&self, b: &IrBody, path: &str) -> Embedded {
        let mut e = Embedded::default();
        self.items.borrow_mut().clear();
        self.owner.replace(b.path.clone());
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
            for next in b
                .blocks
                .get(at)
                .map(|k| k.terminator.successors())
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
                    self.terminator(&block.terminator, b, e)
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
                        mir_ctor("Callee.constant", &[mir_string(key)]),
                        term_list(&[], &mir_name("Operand")),
                        format!("(ctor Option.some [Nat] {after})"),
                    ],
                )
            ));
            if slots > 1 {
                let borrow = mir_ctor(
                    "Statement.assign",
                    &[
                        place(value + 1),
                        mir_ctor(
                            "Rvalue.borrow",
                            &[
                                mir_ctor(&kind_ctor(spec::MirBorrowKind::Shared.path()), &[]),
                                place(value),
                            ],
                        ),
                    ],
                );
                blocks.push(format!(
                    "(record {} {{statements := {}, terminator := {}}})",
                    mir_name("Block"),
                    term_list(&[borrow], &mir_name("Statement")),
                    mir_ctor("Terminator.jump", &[next.to_string()])
                ));
            }
        }
        e.term = format!(
            "(record {} {{path := {}, arity := {}, locals := {}, blocks := {}, entry := {entry}}})",
            mir_name("Body"),
            mir_string(path),
            b.arity,
            term_list(&locals, &mir_name("Ty")),
            term_list(&blocks, &mir_name("Block"))
        );
        e
    }
}

/// Each enum core's source declares, by its name's last segment, with its
/// variants' discriminants: those its declaration sets, else each
/// variant's index.
pub(crate) fn enums(decls: &crate::mir::core::CoreDecls) -> Enums {
    let mut out: Enums = BTreeMap::new();
    for s in &decls.structs {
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
         max_child_output_bytes = 16777216\nchild_timeout_ms = 300000\n",
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
    harness: Option<&Path>,
    observed: Option<&Path>,
) -> Result<EmbedCensus, UorustError> {
    let root = library.parent().unwrap_or(library);
    let (modules, census, program) = embed_census(krate, root, dumps, only)?;
    let mut extra = BTreeMap::new();
    let mut index = None;
    if let Some(observed) = observed {
        let text = read_text(observed)?;
        let (modules, checks) = differential(&program, observed, &text)?;
        extra = modules;
        index = Some(checks);
    }
    let mut files = embed_project(krate, &modules, extra, crate::project::MAX_FILE_BYTES);
    files.insert("census.toml".into(), census.render().into_bytes());
    if let Some(checks) = index {
        files.insert("differential.toml".into(), checks.into_bytes());
    }
    replace_dir(out, Some(&files))?;
    // A module none of whose bodies is sampled has no harness.
    if let Some(dir) = harness {
        let crate_files = (!program.samples.is_empty()).then(|| harness_crate(&program));
        replace_dir(dir, crate_files.as_ref())?;
    }
    Ok(census)
}

/// A file's text.
pub(crate) fn read_text(path: &Path) -> Result<String, UorustError> {
    std::fs::read_to_string(path).map_err(|e| crate::error::io(path, e))
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
fn replace_dir(dir: &Path, files: Option<&BTreeMap<String, Vec<u8>>>) -> Result<(), UorustError> {
    let io = crate::error::io;
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| io(dir, e))?;
    }
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
    src: &Path,
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
    if params.is_empty() || !stable_and_safe(&module_sources(src, module), name) {
        return None;
    }
    Some(Sample {
        key: key.to_string(),
        kappa: kappa.to_string(),
        function: format!("{ty}::{name}"),
        params,
        result,
        closure: Vec::new(),
    })
}

/// The source files of a top-level module: its directory's, or, for one
/// the crate root declares at a path (`#[path = "num/f32.rs"] pub mod
/// f32;`), that file.
fn module_sources(src: &Path, module: &str) -> Vec<std::path::PathBuf> {
    let own = src.join(module);
    if own.is_dir() {
        return dir_paths(&own).unwrap_or_default();
    }
    let root = read_text(&src.join("lib.rs")).unwrap_or_default();
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
    every: BTreeMap<String, (String, Embedded)>,
    samples: Vec<Sample>,
    /// The target's pointer width, `usize`'s.
    width: u32,
    /// Whether the target is big-endian.
    big_endian: bool,
}

/// The keys of the bodies `key` reaches, itself first, where the evaluator
/// defines each: `None` where one is not embedded, is blocked, calls an
/// intrinsic or leaves a callee to a dictionary.
fn closure(every: &BTreeMap<String, (String, Embedded)>, key: &str) -> Option<Vec<String>> {
    let mut seen = vec![key.to_string()];
    let mut at = 0;
    while let Some(k) = seen.get(at).cloned() {
        let (_, e) = every.get(&k)?;
        if !e.blocked.is_empty() || !e.bound.is_empty() || !e.intrinsics.is_empty() {
            return None;
        }
        for c in &e.callees {
            if !seen.contains(c) {
                seen.push(c.clone());
            }
        }
        at += 1;
    }
    Some(seen)
}

/// The harness crate: one call per case, under `catch_unwind`, each
/// printed as `<case> <result>` with an integer as its two's complement
/// bits, after a first line with the build's checks.
fn harness_crate(program: &Program) -> BTreeMap<String, Vec<u8>> {
    let mut main = String::from(
        "// Written by `uorust embed --harness`; do not edit.\n\
         #![allow(unused_parens)]\n\
         fn main() {\n    std::panic::set_hook(Box::new(|_| {}));\n    \
         let overflow = std::panic::catch_unwind(|| std::hint::black_box(u8::MAX) + 1).is_err();\n    \
         println!(\"checks {} {}\", cfg!(debug_assertions), overflow);\n",
    );
    let mut case = 0usize;
    for s in &program.samples {
        for inputs in s.cases() {
            let args: Vec<String> = s
                .params
                .iter()
                .zip(&inputs)
                .map(|((printed, scalar), x)| scalar.rust(printed, *x))
                .collect();
            let show = match s.result.1 {
                Sampled::Bool => "r.to_string()".to_string(),
                Sampled::Int(..) => {
                    format!("((r as {}) as u128).to_string()", unsigned(&s.result.0))
                }
                Sampled::Float(..) => "r.to_bits().to_string()".to_string(),
            };
            let _ = write!(
                main,
                "    match std::panic::catch_unwind(|| {}({})) {{\n        \
                 Ok(r) => println!(\"{case} {{}}\", {show}),\n        \
                 Err(_) => println!(\"{case} panic\"),\n    }}\n",
                s.function,
                args.join(", ")
            );
            case += 1;
        }
    }
    main.push_str("}\n");
    BTreeMap::from([
        (
            "Cargo.toml".to_string(),
            "[package]\nname = \"uorust-embed-harness\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n"
                .as_bytes()
                .to_vec(),
        ),
        ("src/main.rs".to_string(), main.into_bytes()),
    ])
}

/// The steps a run of a sample takes at most: each statement and
/// terminator of its closure, once per bit of its widest input and once
/// more, since core's loops over an integer iterate at most once per bit.
fn fuel(program: &Program, s: &Sample) -> usize {
    let steps: usize = s
        .closure
        .iter()
        .filter_map(|k| program.every.get(k))
        .map(|(_, e)| {
            e.term.matches("(record Mir::Block ").count()
                + e.term.matches("(ctor Mir::Statement.").count()
        })
        .sum();
    let bits = s
        .params
        .iter()
        .map(|(_, sc)| sc.bits() as usize)
        .max()
        .unwrap_or(1);
    steps * (bits + 1)
}

/// The most theorems one module of the differential check holds: Lean
/// decides each by evaluation, and a module is one child process that
/// `lexlean verify` stops at the project's `child_timeout_ms`.
const DIFFERENTIAL_THEOREMS: usize = 500;

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
    let mut parts: Vec<(Vec<String>, BTreeSet<String>, usize)> = vec![Default::default()];
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
            if *result == "panic" {
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
                "\n[[case]]\ntheorem = \"{theorem}\"\nbody = {}\nfunction = \"{}\"\ninputs = [{}]\nreturned = \"{result}\"\n",
                census_string(&s.key),
                s.function,
                inputs.iter().map(u128::to_string).collect::<Vec<_>>().join(", ")
            );
            theorems.push(format!(
                "theorem{{\n  name: {theorem},\n  parameters: [],\n  statement: ((Mir::evaluate {} (p{name}) {} {}) = {}),\n  proof: reflexivity{{}}\n}}",
                fuel(program, s),
                mir_string(&s.key),
                term_list(&args, &mir_name("Value")),
                mir_ctor("Outcome.returned", &[expected])
            ));
        }
        if theorems.is_empty() {
            continue;
        }
        if parts
            .last()
            .is_some_and(|(_, _, n)| *n > 0 && n + theorems.len() > DIFFERENTIAL_THEOREMS)
        {
            parts.push(Default::default());
        }
        let Some((decls, defined, count)) = parts.last_mut() else {
            continue;
        };
        *count += theorems.len();
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
            "definition{{\n  name: p{name},\n  parameters: [],\n  result: {},\n  body: (record {} {{bodies := {}, dictionary := (nil {}), ubChecks := {debug}, contractChecks := false, overflowChecks := {overflow}, pointerWidth := {}, bigEndian := {}}})\n}}",
            mir_name("Program"),
            mir_name("Program"),
            term_list(&bodies, &mir_name("Body")),
            mir_name("Binding"),
            program.width,
            program.big_endian
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

/// A body's κ, as the core register keys it: its crate path and its text,
/// with the build's allocation numbers renumbered.
pub(crate) fn body_kappa(krate: &str, path: &str, text: &str) -> String {
    crate::project::sha(
        format!(
            "{krate}::{path}\n{}",
            crate::mir::core::numbered_allocations(text)
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

/// The name a body has in the program: its path, or, where a macro's
/// impls share it, its path and its definition's name.
pub(crate) fn program_key(link: &crate::mir::link::CoreLink, path: &str, kappa: &str) -> String {
    if link.bodies.get(path).is_some_and(|b| b.len() > 1) {
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
    let erased = crate::mir::lower::erase(callee);
    let path = link.path(&erased);
    if let Some(p) = path.as_ref().filter(|p| link.decls.intrinsics.contains(*p)) {
        return Callee::Intrinsic(p.clone());
    }
    if let Some(p) = path.as_ref() {
        if let Some([_]) = link.bodies.get(p).map(Vec::as_slice) {
            return Callee::Body(p.clone());
        }
    }
    let Some(instance) = link.instance(callee, args, dest) else {
        return Callee::Bound(callee.to_string());
    };
    let Some(texts) = link.bodies.get(&instance.path) else {
        return Callee::Bound(callee.to_string());
    };
    if let [_] = texts.as_slice() {
        return Callee::Body(instance.path);
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
        (Some(t), None) => Callee::Body(program_key(
            link,
            &instance.path,
            &body_kappa(krate, &instance.path, t),
        )),
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
    let doc: toml::Table = text.parse().map_err(|e| bad(format!("{e}")))?;
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
fn embed_census(
    krate: &str,
    root: &Path,
    dumps: &Path,
    only: &[String],
) -> Result<(Vec<(String, Vec<(String, Embedded)>)>, EmbedCensus, Program), UorustError> {
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
            texts.push((name, read_text(file)?));
        }
        read.push((module, texts));
    }
    let link = crate::mir::link::CoreLink::new(
        crate::mir::core::core_decls(root),
        root,
        read.iter()
            .flat_map(|(_, texts)| texts.iter().map(|(n, t)| (n.as_str(), t))),
    );
    let enums = enums(&link.decls);
    let structs = structs(&link.decls);
    let layouts = crate::layouts::Index::new(crate::layouts::load(dumps)?);
    let resolve = |callee: &str, args: &[Option<String>], dest: &str| {
        resolve_callee(&link, krate, callee, args, dest)
    };
    let mut census = EmbedCensus::default();
    let mut modules = Vec::new();
    let mut every: BTreeMap<String, (String, Embedded)> = BTreeMap::new();
    let mut samples = Vec::new();
    let src = library_root(root).join(krate).join("src");
    let statics = statics(&link);
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
                item_type: &|key: &str| item_type(&link, key),
                layouts: &layouts,
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
                    None => program_key(&link, &b.path, &kappa),
                };
                let e = cx.body(&b, &key);
                if selected && has_row(&b.path) && !is_static {
                    census.add(&e);
                    if let Some(sample) = sample(root, &src, &b, &key, &kappa, width, &floats) {
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
    // A sample is checked where the evaluator defines its whole closure:
    // every body it reaches is embedded, uses no construct the evaluator
    // does not define, calls no intrinsic and leaves no callee to a
    // dictionary.
    samples.retain_mut(|s| match closure(&every, &s.key) {
        Some(c) => {
            s.closure = c;
            true
        }
        None => false,
    });
    let program = Program {
        every,
        samples,
        width,
        big_endian: big_endian.unwrap_or_default(),
    };
    Ok((modules, census, program))
}

#[cfg(test)]
mod tests {
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
        assert_eq!(repeat_parts("[const 0_u8; 32]"), Some(("const 0_u8", 32)));
        assert_eq!(repeat_parts("[move _3; 4_usize]"), Some(("move _3", 4)));
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
    }

    #[test]
    fn a_static_is_read_as_its_initializer_ue_19() {
        let text = "// MIR for `a::b::MAP` before ElaborateBoxDerefs\n\nstatic MAP: [u8; 2] = {\n    let mut _0: [u8; 2];\n\n    bb0: {\n        _0 = [const 1_u8, const 2_u8];\n        return;\n    }\n}\n";
        let body = static_body(text).expect("the initializer");
        assert_eq!(body.path, "a::b::MAP");
        assert_eq!(body.arity, 0);
        assert!(static_body(&text.replace("static MAP", "static mut MAP")).is_none());
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
