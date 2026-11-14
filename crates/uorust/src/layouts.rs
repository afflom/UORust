//! Type layouts as rustc itself computes them (UE-19).
//!
//! `repr(Rust)` layout is unspecified, so the only layout of a struct, an
//! enum or a tuple the embedding may read is the one the pinned compiler
//! chose. The dump step (`scripts/mir/dump-lib.sh`) asks it: [`aliases`]
//! writes, into a copy of the library's sources, a type alias
//! `#[rustc_dump_layout(debug)] type __L_k = T;` for every type `T` of an
//! aggregate the dumps print, and for every field of one, in the module that
//! declares its head (where its private items are seen); the crate is
//! checked once more, and [`read`] parses each `layout_of(T) = Layout {..}`
//! the compiler prints into `layouts.json` beside the dumps, keyed by the
//! type as rustc prints it. An alias rustc refuses (a private item of
//! another module, a type parameter) is dropped and the crate checked
//! again, so no refusal is ever a layout.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::UorustError;

/// A scalar's primitive, as rustc's `Primitive` prints it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Prim {
    /// `Int(I8, false)`: its bytes and whether it is signed.
    Int { bytes: u64, signed: bool },
    /// `Float(F32)`: its bits.
    Float { bits: u64 },
    /// `Pointer(AddressSpace(0))`.
    Pointer,
}

/// A scalar of a layout: its primitive and its valid range (`valid_range`,
/// which wraps past the last value where its start is above its end).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Scalar {
    pub prim: Prim,
    pub start: u128,
    pub end: u128,
}

/// How an enum's variant is stored in its tag (`TagEncoding`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Encoding {
    /// The tag holds the discriminant.
    Direct,
    /// The variants `first..=last` are stored as `start + (v - first)` in
    /// the niche; `untagged` is every other value.
    Niche {
        untagged: usize,
        first: usize,
        last: usize,
        start: u128,
    },
}

/// One variant of an enum laid out with a tag: its size and its fields'
/// offsets, in declaration order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Variant {
    pub size: u64,
    pub offsets: Vec<u64>,
}

/// The tag of an enum of several variants (`Variants::Multiple`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tag {
    pub scalar: Scalar,
    /// The field of the enum's own layout the tag is: its offset is that
    /// field's.
    pub field: usize,
    pub encoding: Encoding,
    pub variants: Vec<Variant>,
}

/// A type's layout as rustc prints it: its size and alignment in bytes,
/// its scalar where it is one, its fields' offsets in declaration order,
/// and, for an enum of several variants, its tag; `single` is the variant
/// of an enum laid out as one (`Variants::Single`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TypeLayout {
    pub size: u64,
    pub align: u64,
    pub scalar: Option<Scalar>,
    pub offsets: Vec<u64>,
    pub single: Option<usize>,
    pub tag: Option<Tag>,
    /// The type of each field, as rustc prints it, where it was asked: a
    /// struct's or a tuple's fields, or each variant's of an enum.
    #[serde(default)]
    pub fields: Vec<Vec<Option<String>>>,
}

/// What the dump step keeps: each layout by the type as rustc prints it,
/// and each type it asked for by another text (a trimmed path) by the
/// type rustc printed for it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Kept {
    pub layouts: Layouts,
    pub names: BTreeMap<String, String>,
}

/// The layouts of a dump, by the type as rustc prints it.
pub(crate) type Layouts = BTreeMap<String, TypeLayout>;

/// The layouts of a dump, with each type found by its path's last segment
/// too: a field's type, as its declaration writes it (`UnsafeCell<u8>`),
/// is found by the one printed type it names (`cell::UnsafeCell<u8>`).
#[derive(Default)]
pub(crate) struct Index {
    kept: Kept,
    by_last: BTreeMap<String, Vec<String>>,
}

impl Index {
    pub(crate) fn new(kept: Kept) -> Self {
        let mut by_last: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for ty in kept.layouts.keys() {
            by_last.entry(last(ty)).or_default().push(ty.clone());
        }
        Self { kept, by_last }
    }

    /// The layout of `ty`, as MIR or a declaration prints it, with the
    /// type as rustc printed it.
    pub(crate) fn get(&self, ty: &str) -> Option<(&str, &TypeLayout)> {
        let ty = ty.trim();
        let ty = self.kept.names.get(ty).map_or(ty, String::as_str);
        if let Some((k, l)) = self.kept.layouts.get_key_value(ty) {
            return Some((k, l));
        }
        let found: Vec<&String> = self
            .by_last
            .get(&last(ty))?
            .iter()
            .filter(|k| same_type(k, ty))
            .collect();
        match found.as_slice() {
            [one] => self
                .kept
                .layouts
                .get_key_value(*one)
                .map(|(k, l)| (k.as_str(), l)),
            _ => None,
        }
    }
}

/// The last segment of a type's head, its arguments erased: a tuple's is
/// empty.
fn last(ty: &str) -> String {
    let (head, _) = crate::embed::generic_parts(ty.trim());
    crate::mir::parse::split_path(head)
        .last()
        .copied()
        .unwrap_or(head)
        .to_string()
}

/// Whether two types name the same: their heads the same item, their
/// arguments the same types, lifetimes aside.
fn same_type(a: &str, b: &str) -> bool {
    let strip = |t: &str| -> String {
        let t = t.trim();
        let t = t.strip_prefix("crate::").unwrap_or(t);
        t.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let (a, b) = (strip(a), strip(b));
    if a == b {
        return true;
    }
    if a.starts_with('\'') && b.starts_with('\'') {
        return true;
    }
    let ((ha, aa), (hb, ab)) = (
        crate::embed::generic_parts(&a),
        crate::embed::generic_parts(&b),
    );
    if aa.is_empty() && ab.is_empty() {
        return ha
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            && hb
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            && crate::embed::same_item(ha, hb);
    }
    aa.len() == ab.len()
        && crate::embed::same_item(ha, hb)
        && aa.iter().zip(&ab).all(|(x, y)| same_type(x, y))
}

/// The file beside the dumps the layouts are kept in.
pub(crate) const FILE: &str = "layouts.json";

/// The layouts beside `dumps`, none where the dump step wrote none.
pub(crate) fn load(dumps: &Path) -> Result<Kept, UorustError> {
    let file = dumps.join(FILE);
    if !file.exists() {
        return Ok(Kept::default());
    }
    let text = crate::embed::read_text(&file)?;
    serde_json::from_str(&text).map_err(|e| crate::error::io(&file, std::io::Error::other(e)))
}

/// A node of a value's `Debug` output: `Name { k: v, .. }`, `Name(v, ..)`,
/// `[v, ..]`, or a word (`4 bytes`, `0..=255`, `false`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Word(String),
    Record(String, Vec<(String, Node)>),
    Call(String, Vec<Node>),
    List(Vec<Node>),
}

impl Node {
    fn field(&self, key: &str) -> Option<&Node> {
        match self {
            Self::Record(_, fs) => fs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Word(w) | Self::Record(w, _) | Self::Call(w, _) => w,
            Self::List(_) => "",
        }
    }

    fn args(&self) -> &[Node] {
        match self {
            Self::Call(_, a) | Self::List(a) => a,
            _ => &[],
        }
    }

    fn word(&self) -> Option<&str> {
        match self {
            Self::Word(w) => Some(w),
            _ => None,
        }
    }
}

/// Parse `Debug` output into a [`Node`].
fn parse(text: &str) -> Option<Node> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let node = value(&chars, &mut i)?;
    skip(&chars, &mut i);
    (i == chars.len()).then_some(node)
}

fn skip(c: &[char], i: &mut usize) {
    while c.get(*i).is_some_and(|c| c.is_whitespace()) {
        *i += 1;
    }
}

fn word(c: &[char], i: &mut usize) -> String {
    let start = *i;
    while c.get(*i).is_some_and(|c| !"{}()[],:".contains(*c)) {
        *i += 1;
    }
    c[start..*i].iter().collect::<String>().trim().to_string()
}

fn value(c: &[char], i: &mut usize) -> Option<Node> {
    skip(c, i);
    if c.get(*i) == Some(&'[') {
        *i += 1;
        return Some(Node::List(values(c, i, ']')?));
    }
    let name = word(c, i);
    skip(c, i);
    match c.get(*i) {
        Some('{') => {
            *i += 1;
            let mut fields = Vec::new();
            loop {
                skip(c, i);
                if c.get(*i) == Some(&'}') {
                    *i += 1;
                    return Some(Node::Record(name, fields));
                }
                let key = word(c, i);
                skip(c, i);
                (c.get(*i) == Some(&':')).then_some(())?;
                *i += 1;
                fields.push((key, value(c, i)?));
                skip(c, i);
                if c.get(*i) == Some(&',') {
                    *i += 1;
                }
            }
        }
        Some('(') => {
            *i += 1;
            Some(Node::Call(name, values(c, i, ')')?))
        }
        _ => (!name.is_empty()).then_some(Node::Word(name)),
    }
}

fn values(c: &[char], i: &mut usize, close: char) -> Option<Vec<Node>> {
    let mut out = Vec::new();
    loop {
        skip(c, i);
        if c.get(*i) == Some(&close) {
            *i += 1;
            return Some(out);
        }
        out.push(value(c, i)?);
        skip(c, i);
        if c.get(*i) == Some(&',') {
            *i += 1;
        }
    }
}

/// `Size(4 bytes)` as its bytes.
fn size(n: &Node) -> Option<u64> {
    let [w] = n.args() else { return None };
    w.word()?.strip_suffix(" bytes")?.parse().ok()
}

/// `0..=255` as its ends.
fn range(n: &Node) -> Option<(u128, u128)> {
    let (a, b) = n.word()?.split_once("..=")?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn prim(n: &Node) -> Option<Prim> {
    match (n.name(), n.args()) {
        ("Int", [width, signed]) => {
            let bits: u64 = width.word()?.strip_prefix('I')?.parse().ok()?;
            Some(Prim::Int {
                bytes: bits / u64::from(u8::BITS),
                signed: signed.word()? == "true",
            })
        }
        ("Float", [width]) => Some(Prim::Float {
            bits: width.word()?.strip_prefix('F')?.parse().ok()?,
        }),
        ("Pointer", _) => Some(Prim::Pointer),
        _ => None,
    }
}

/// `Initialized { value, valid_range }` (a `Union` scalar has no range and
/// is not read).
fn scalar_of(n: &Node) -> Option<Scalar> {
    let (start, end) = range(n.field("valid_range")?)?;
    Some(Scalar {
        prim: prim(n.field("value")?)?,
        start,
        end,
    })
}

fn offsets_of(n: &Node) -> Option<Vec<u64>> {
    match n {
        Node::List(items) => items.iter().map(size).collect(),
        _ => None,
    }
}

fn nat(n: &Node) -> Option<usize> {
    n.word()?.parse().ok()
}

/// A `Layout { .. }` as a [`TypeLayout`], where rustc lays the type out
/// with fields at offsets (`FieldsShape::Arbitrary`) or none
/// (`Primitive`).
fn type_layout(n: &Node) -> Option<TypeLayout> {
    let fields = n.field("fields")?;
    let offsets = match fields.name() {
        "Arbitrary" => offsets_of(fields.field("offsets")?)?,
        "Primitive" => Vec::new(),
        _ => return None,
    };
    let repr = n.field("backend_repr")?;
    let scalar = match (repr.name(), repr.args()) {
        ("Scalar", [s]) => scalar_of(s),
        _ => None,
    };
    let variants = n.field("variants")?;
    let (single, tag) = match variants.name() {
        "Single" => (Some(nat(variants.field("index")?)?), None),
        "Multiple" => {
            let enc = variants.field("tag_encoding")?;
            let encoding = match enc.name() {
                "Direct" => Encoding::Direct,
                "Niche" => {
                    let (first, last) = range(enc.field("niche_variants")?)?;
                    Encoding::Niche {
                        untagged: nat(enc.field("untagged_variant")?)?,
                        first: usize::try_from(first).ok()?,
                        last: usize::try_from(last).ok()?,
                        start: enc.field("niche_start")?.word()?.parse().ok()?,
                    }
                }
                _ => return None,
            };
            let vs = match variants.field("variants")? {
                Node::List(vs) => vs
                    .iter()
                    .map(|v| {
                        Some(Variant {
                            size: size(v.field("size")?)?,
                            offsets: offsets_of(v.field("field_offsets")?)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
                _ => return None,
            };
            let tag = Tag {
                scalar: scalar_of(variants.field("tag")?)?,
                field: nat(variants.field("tag_field")?)?,
                encoding,
                variants: vs,
            };
            (None, Some(tag))
        }
        _ => return None,
    };
    let align = n.field("align")?.field("abi")?;
    Some(TypeLayout {
        size: size(n.field("size")?)?,
        align: size(align)?,
        scalar,
        offsets,
        single,
        tag,
        fields: Vec::new(),
    })
}

/// The errors that follow from another (a layout rustc does not compute
/// while the crate has errors, a method two broken impls both give): an
/// alias they are reported at is asked again once the others are dropped.
const DEFERRED: &[&str] = &["error: the type has an unknown layout", "error[E0034]"];

/// What one check of the patched crate printed: each layout by the type
/// rustc names, and each line an error other than a layout is reported at.
#[allow(clippy::type_complexity)]
fn diagnostics(
    stderr: &str,
) -> (
    BTreeMap<(PathBuf, usize), (String, TypeLayout)>,
    BTreeSet<(PathBuf, usize)>,
) {
    let mut layouts = BTreeMap::new();
    let mut refused = BTreeSet::new();
    let lines: Vec<&str> = stderr.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if !line.starts_with("error") {
            continue;
        }
        let head = line
            .strip_prefix("error: layout_of(")
            .and_then(|r| r.strip_suffix(") = Layout {"));
        let start = i;
        while i < lines.len() && !lines[i].trim_start().starts_with("--> ") {
            if lines[i].starts_with("error") {
                break;
            }
            i += 1;
        }
        let at = lines
            .get(i)
            .and_then(|l| l.trim_start().strip_prefix("--> "));
        let place = |at: &str| {
            let mut parts = at.rsplitn(3, ':');
            let (_, line, file) = (parts.next(), parts.next(), parts.next());
            Some((PathBuf::from(file?), line?.parse::<usize>().ok()?))
        };
        match (head, at) {
            (Some(ty), Some(at)) => {
                let body = format!("Layout {{\n{}", unwrap_ranges(&lines[start..i].join("\n")));
                if let (Some(l), Some(p)) = (parse(&body).as_ref().and_then(type_layout), place(at))
                {
                    layouts.insert(p, (ty.to_string(), l));
                }
            }
            // A layout rustc could not compute while other aliases were
            // refused (a pattern type's range is a constant it then does
            // not evaluate) is asked again once they are dropped.
            (None, Some(_)) if DEFERRED.iter().any(|d| line.starts_with(d)) => {}
            (None, Some(at)) => {
                refused.extend(place(at));
            }
            _ => {}
        }
    }
    (layouts, refused)
}

/// A wrapping range as rustc prints it, `(..=1) | (255..)`, written as
/// `255..=1`: a `valid_range` whose start is above its end wraps.
fn unwrap_ranges(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("(..=") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + "(..=".len()..];
        let parsed = tail.split_once(") | (").and_then(|(end, more)| {
            let (start, after) = more.split_once("..)")?;
            Some((end, start, after))
        });
        match parsed {
            Some((end, start, after)) => {
                out.push_str(&format!("{start}..={end}"));
                rest = after;
            }
            None => {
                out.push_str("(..=");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The aggregates a type printed by MIR holds, itself among them: each
/// tuple and each path that is not a primitive, as printed.
fn aggregates(ty: &str, out: &mut BTreeSet<String>) {
    let ty = ty.trim();
    if ty.is_empty() || ty.contains('{') || ty.contains("dyn ") || ty.contains("impl ") {
        return;
    }
    if let Some(inner) = crate::embed::pointee(ty) {
        return aggregates(inner, out);
    }
    if let Some(inner) = ty.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        let elem = top_split(inner, ';');
        return aggregates(&elem[0], out);
    }
    if ty.starts_with('(') && ty.ends_with(')') && !ty.contains(") is ") {
        let elems = top_split(&ty[1..ty.len() - 1], ',');
        let elems: Vec<&String> = elems.iter().filter(|e| !e.trim().is_empty()).collect();
        if !elems.is_empty() {
            out.insert(ty.to_string());
        }
        for e in elems {
            aggregates(e, out);
        }
        return;
    }
    if ty.starts_with('<') || ty.starts_with("fn(") || ty.starts_with("unsafe ") || ty == "!" {
        return;
    }
    let (head, args) = crate::embed::generic_parts(ty);
    if !head.contains("::") {
        return;
    }
    out.insert(ty.to_string());
    for a in args {
        aggregates(&a, out);
    }
}

/// `text` split at `sep` where no bracket is open.
fn top_split(text: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(text[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(text[start..].to_string());
    out
}

/// The crates of the library a path of another crate starts with.
const EXTERN: &[&str] = &["core", "alloc", "std"];

/// A type as MIR prints it written as source in the crate: its crate's own
/// paths from `crate::`, every lifetime `'static`.
fn source(ty: &str, krate: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = ty.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push_str("'static");
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let after = start >= 2 && chars[start - 1] == ':' && chars[start - 2] == ':';
            let pathy = chars.get(i) == Some(&':') && chars.get(i + 1) == Some(&':');
            if !after && pathy && word != "crate" && !EXTERN.contains(&word.as_str()) {
                out.push_str("crate::");
            } else if word == krate && !after && pathy {
                out.push_str("crate");
                continue;
            }
            out.push_str(&word);
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The file of the module `module` (a path of `src`'s modules), or of the
/// nearest module above it that has one.
fn module_file(src: &Path, module: &[&str]) -> PathBuf {
    for n in (1..=module.len()).rev() {
        let dir: PathBuf = module[..n].iter().collect();
        for f in [
            src.join(&dir).with_extension("rs"),
            src.join(&dir).join("mod.rs"),
        ] {
            if f.is_file() {
                return f;
            }
        }
    }
    src.join("lib.rs")
}

/// The types of the dumps' aggregates: every local's, every constant's
/// and every argument's, and theirs in turn.
fn dumped_types(dumps: &Path) -> Result<BTreeSet<String>, UorustError> {
    let mut found = BTreeSet::new();
    for dir in crate::embed::dir_paths(dumps)?
        .into_iter()
        .filter(|p| p.is_dir())
    {
        for file in crate::embed::dir_paths(&dir)? {
            let text = crate::embed::read_text(&file)?;
            for line in text.lines() {
                let l = line.trim();
                if let Some(decl) = l.strip_prefix("let ") {
                    let decl = decl.strip_prefix("mut ").unwrap_or(decl);
                    if let Some((_, ty)) = decl.split_once(": ") {
                        aggregates(ty.trim_end_matches(';'), &mut found);
                    }
                }
                let mut rest = l;
                while let Some(at) = rest.find("{alloc") {
                    rest = &rest[at..];
                    let Some(end) = rest.find('}') else { break };
                    if let Some((_, ty)) = rest[..end].split_once(": ") {
                        aggregates(ty, &mut found);
                    }
                    rest = &rest[end..];
                }
            }
        }
    }
    Ok(found)
}

/// What an alias asks rustc: the layout of a type, as MIR prints it, or
/// the type of a field (of a variant) of a type whose layout it gave.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ask {
    Type(String),
    Field(String, usize, usize),
}

/// The associated types the crate's traits declare: each name, by the
/// paths of the traits that declare one of that name.
fn associated(src: &Path) -> BTreeMap<String, BTreeSet<String>> {
    // Read from the text, not a parse: a file of the library may use
    // syntax no parser outside rustc reads. Each `trait Name .. {`, and
    // each `type Item` its braces hold at their first depth.
    fn traits(text: &str, module: &str, out: &mut BTreeMap<String, BTreeSet<String>>) {
        let ident = |t: &str| -> String {
            t.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect()
        };
        // Comments set aside: their prose names traits too.
        let code: String = text
            .lines()
            .map(|l| l.split_once("//").map_or(l, |(c, _)| c))
            .collect::<Vec<_>>()
            .join("\n");
        let mut rest = code.as_str();
        while let Some(at) = rest.find("trait ") {
            let before = rest[..at].chars().last();
            rest = &rest[at + "trait ".len()..];
            let name = ident(rest);
            let upper = name.starts_with(|c: char| c.is_uppercase());
            if !upper || before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let Some(open) = rest.find(['{', ';']) else {
                break;
            };
            if rest[open..].starts_with(';') {
                continue;
            }
            let mut depth = 0usize;
            let body = &rest[open..];
            for (k, c) in body.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    't' if depth == 1 && body[k..].starts_with("type ") => {
                        let prev = body[..k].chars().last();
                        if prev.is_none_or(|p| !(p.is_alphanumeric() || p == '_')) {
                            let item = ident(&body[k + "type ".len()..]);
                            if !item.is_empty() {
                                out.entry(item)
                                    .or_default()
                                    .insert(format!("{module}::{name}"));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        for p in crate::embed::dir_paths(dir).unwrap_or_default() {
            if p.is_dir() {
                walk(&p, files);
            } else if p.extension().is_some_and(|e| e == "rs") {
                files.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(src, &mut files);
    let mut out = BTreeMap::new();
    for file in files {
        let Ok(rel) = file.strip_prefix(src) else {
            continue;
        };
        let mut segs: Vec<String> = rel
            .with_extension("")
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        if segs.last().is_some_and(|l| l == "mod" || l == "lib") {
            segs.pop();
        }
        let module = std::iter::once("crate".to_string())
            .chain(segs)
            .collect::<Vec<_>>()
            .join("::");
        if let Ok(text) = std::fs::read_to_string(&file) {
            traits(&text, &module, &mut out);
        }
    }
    out
}

/// Every lifetime of `text` as `'static`: an alias names no lifetime of
/// its own.
fn static_lifetimes(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' && chars.peek().is_some_and(|n| n.is_alphabetic() || *n == '_') {
            while chars
                .peek()
                .is_some_and(|n| n.is_alphanumeric() || *n == '_')
            {
                chars.next();
            }
            out.push_str("'static");
        } else {
            out.push(c);
        }
    }
    out
}

/// A field's type as its declaration writes it, as source of an alias in
/// the declaration's module: its parameters substituted, a pattern type
/// as the `pattern_type!` that writes it, and an associated type of a
/// parameter (`T::Storage`) by the one trait that declares one of its
/// name.
fn field_text(
    field: &str,
    bound: &BTreeMap<String, String>,
    assoc: &BTreeMap<String, BTreeSet<String>>,
) -> String {
    let mut t = field.trim().to_string();
    if let Some((base, pat)) = t.strip_prefix('(').and_then(|r| r.split_once(") is ")) {
        t = format!("pattern_type!({base} is {pat})");
    }
    for p in bound.keys() {
        let prefix = format!("{p}::");
        let mut out = String::new();
        let mut rest = t.as_str();
        while let Some(at) = rest.find(&prefix) {
            let before = rest[..at].chars().last();
            let after = &rest[at + prefix.len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            out.push_str(&rest[..at]);
            let word = before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':'));
            match assoc.get(&name).filter(|ts| ts.len() == 1 && word) {
                Some(ts) => {
                    let tr = ts.iter().next().map(String::as_str).unwrap_or_default();
                    out.push_str(&format!("<{p} as {tr}>::{name}"));
                    rest = &after[name.len()..];
                }
                None => {
                    out.push_str(&prefix);
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        t = out;
    }
    static_lifetimes(&crate::mir::stdlib::substitute(&t, bound))
}

/// The module a type's path, as rustc prints it, is declared in, as a
/// path of the crate's modules; another crate's type is seen from the
/// crate's root.
fn home_of(printed: &str, krate: &str) -> Vec<String> {
    if printed.starts_with('(') {
        return Vec::new();
    }
    let (head, _) = crate::embed::generic_parts(printed);
    let segs = crate::mir::parse::split_path(head);
    let segs = match segs.first() {
        Some(&first) if first == krate => &segs[1..],
        Some(&first) if EXTERN.contains(&first) => &[],
        _ => &segs[..],
    };
    segs[..segs.len().saturating_sub(1)]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Ask rustc for the layout of every aggregate the dumps print and of
/// every field of one, in `copy`, a copy of the `library` directory the
/// dumps were built from, for the crate `krate`: `check` is the shell
/// command that checks the copy. Each round writes its aliases, checks the
/// copy, drops each alias rustc refused and checks again until it refuses
/// none; the next round asks for the fields of the types this one laid
/// out. The layouts are kept in `dumps`' [`FILE`]; returns how many.
pub fn run(krate: &str, dumps: &Path, copy: &Path, check: &str) -> Result<usize, UorustError> {
    let io = crate::error::io;
    let copy = &std::fs::canonicalize(copy).map_err(|e| io(copy, e))?;
    let decls = crate::mir::core::core_decls(copy.parent().unwrap_or(copy));
    let structs = crate::embed::structs(&decls);
    let src = copy.join(krate).join("src");
    let assoc = associated(&src);
    // A trimmed path (a constant's type prints so) resolves to the one
    // declaration of that name.
    let resolve = |head: &str| -> Option<&crate::mir::core::CoreStruct> {
        let last = crate::mir::parse::split_path(head).last().copied()?;
        let found: Vec<_> = structs
            .get(last)?
            .iter()
            .filter(|d| crate::embed::same_item(head, &d.path))
            .collect();
        match found.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    };
    let ask_type = |ty: &str| -> (Ask, Vec<String>, String) {
        let (head, _) = crate::embed::generic_parts(ty);
        let full = match resolve(head) {
            Some(d) if !head.contains("::") && !ty.starts_with('(') => d.path.clone(),
            _ => head.to_string(),
        };
        let full = format!("{full}{}", &ty[head.len()..]);
        (
            Ask::Type(ty.to_string()),
            home_of(&full, krate),
            source(&full, krate),
        )
    };
    let mut kept = Kept::default();
    let mut asked: BTreeSet<Ask> = BTreeSet::new();
    let mut expanded: BTreeSet<String> = BTreeSet::new();
    let mut pristine: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut next: Vec<(Ask, Vec<String>, String)> =
        dumped_types(dumps)?.iter().map(|t| ask_type(t)).collect();
    while !next.is_empty() {
        // A text that is no type's (a parse error stops the whole check)
        // is asked nothing.
        let mut live: Vec<(Ask, PathBuf, String)> = std::mem::take(&mut next)
            .into_iter()
            .filter(|(a, _, t)| syn::parse_str::<syn::Type>(t).is_ok() && asked.insert(a.clone()))
            .map(|(a, home, t)| {
                let refs: Vec<&str> = home.iter().map(String::as_str).collect();
                (a, module_file(&src, &refs), t)
            })
            .collect();
        let answers = loop {
            let mut by_file: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
            for (i, (_, f, _)) in live.iter().enumerate() {
                by_file.entry(f.clone()).or_default().push(i);
            }
            for f in by_file.keys() {
                if !pristine.contains_key(f) {
                    pristine.insert(f.clone(), crate::embed::read_text(f)?);
                }
            }
            let mut at: BTreeMap<(PathBuf, usize), usize> = BTreeMap::new();
            for (f, text) in &pristine {
                let mut body = text.clone();
                if !body.ends_with('\n') {
                    body.push('\n');
                }
                let mut line = body.lines().count();
                for &i in by_file.get(f).into_iter().flatten() {
                    line += 1;
                    body.push_str(&format!(
                        "#[rustc_dump_layout(debug)] type __L_{i} = {};\n",
                        live[i].2
                    ));
                    at.insert((f.clone(), line), i);
                }
                std::fs::write(f, body).map_err(|e| io(f, e))?;
            }
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(check)
                .output()
                .map_err(|e| io(Path::new(check), e))?;
            let stderr = String::from_utf8_lossy(&output.stderr);
            let (layouts, refused) = diagnostics(&stderr);
            let bad: BTreeSet<usize> = refused.iter().filter_map(|p| at.get(p)).copied().collect();
            if bad.is_empty() {
                // An error at no alias's line asks nothing of the layouts:
                // it is named, and what rustc laid out is kept.
                if let Some((f, n)) = refused.iter().next() {
                    eprintln!("layouts: rustc reported an error at {}:{n}", f.display());
                }
                break layouts
                    .into_iter()
                    .filter_map(|(p, v)| Some((*at.get(&p)?, v)))
                    .collect::<Vec<_>>();
            }
            eprintln!(
                "layouts: rustc refused {} of {} aliases",
                bad.len(),
                live.len()
            );
            live = live
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !bad.contains(i))
                .map(|(_, x)| x)
                .collect();
        };
        for (f, text) in &pristine {
            std::fs::write(f, text).map_err(|e| io(f, e))?;
        }
        for (i, (printed, layout)) in answers {
            kept.layouts.entry(printed.clone()).or_insert(layout);
            match &live[i].0 {
                Ask::Type(t) if *t != printed => {
                    kept.names.insert(t.clone(), printed);
                }
                Ask::Type(_) => {}
                Ask::Field(p, v, f) => {
                    if let Some(slot) = kept
                        .layouts
                        .get_mut(p)
                        .and_then(|l| l.fields.get_mut(*v))
                        .and_then(|fs| fs.get_mut(*f))
                    {
                        *slot = Some(printed);
                    }
                }
            }
        }
        // The next round: each field of each type laid out and not yet
        // asked of, a tuple's elements as it prints them.
        let fresh: Vec<String> = kept
            .layouts
            .keys()
            .filter(|p| !expanded.contains(*p))
            .cloned()
            .collect();
        for p in fresh {
            expanded.insert(p.clone());
            if let Some(inner) = p.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
                let elems: Vec<String> = top_split(inner, ',')
                    .into_iter()
                    .map(|e| e.trim().to_string())
                    .filter(|e| !e.is_empty())
                    .collect();
                for e in &elems {
                    let mut inner = BTreeSet::new();
                    aggregates(e, &mut inner);
                    next.extend(inner.iter().map(|t| ask_type(t)));
                }
                if let Some(l) = kept.layouts.get_mut(&p) {
                    l.fields = vec![elems.into_iter().map(Some).collect()];
                }
                continue;
            }
            let (head, args) = crate::embed::generic_parts(&p);
            let Some(decl) = resolve(head) else { continue };
            // The declarations name no lifetime parameter; `Self` is the type.
            let srcargs: Vec<String> = args
                .iter()
                .filter(|a| !a.starts_with('\''))
                .map(|a| source(a, krate))
                .collect();
            let mut bound: BTreeMap<String, String> =
                decl.params.iter().cloned().zip(srcargs).collect();
            bound.insert("Self".to_string(), source(&p, krate));
            let variants: Vec<Vec<String>> = match &decl.data {
                Some(vs) => vs
                    .iter()
                    .map(|(_, fs)| fs.iter().map(|(_, t)| t.clone()).collect())
                    .collect(),
                None if decl.variants.is_some() => Vec::new(),
                None => vec![decl.fields.iter().map(|(_, t)| t.clone()).collect()],
            };
            let home = home_of(&p, krate);
            if let Some(l) = kept.layouts.get_mut(&p) {
                l.fields = variants.iter().map(|fs| vec![None; fs.len()]).collect();
            }
            for (v, fs) in variants.iter().enumerate() {
                for (f, t) in fs.iter().enumerate() {
                    let text = field_text(t, &bound, &assoc);
                    next.push((Ask::Field(p.clone(), v, f), home.clone(), text));
                }
            }
        }
    }
    let file = dumps.join(FILE);
    let json = serde_json::to_string(&kept).unwrap_or_default();
    std::fs::write(&file, json).map_err(|e| io(&file, e))?;
    Ok(kept.layouts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDERING: &str = "error: layout_of(sync::atomic::Ordering) = Layout {
           size: Size(1 bytes),
           align: AbiAlign {
               abi: Align(1 bytes),
           },
           backend_repr: Scalar(
               Initialized {
                   value: Int(
                       I8,
                       false,
                   ),
                   valid_range: 0..=4,
               },
           ),
           fields: Arbitrary {
               offsets: [
                   Size(0 bytes),
               ],
               in_memory_order: [
                   0,
               ],
           },
           largest_niche: None,
           uninhabited: false,
           variants: Multiple {
               tag: Initialized {
                   value: Int(
                       I8,
                       false,
                   ),
                   valid_range: 0..=4,
               },
               tag_encoding: Direct,
               tag_field: 0,
               variants: [
                   VariantLayout {
                       size: Size(1 bytes),
                       backend_repr: Memory {
                           sized: true,
                       },
                       field_offsets: [],
                       fields_in_memory_order: [],
                       largest_niche: None,
                       uninhabited: false,
                   },
               ],
           },
           max_repr_align: None,
           unadjusted_abi_align: Align(1 bytes),
           randomization_seed: 4979357733989443988,
       }
   --> /x/core/src/lib.rs:404:29
    |
error[E0603]: module `rt` is private
   --> /x/core/src/lib.rs:407:54
";

    #[test]
    fn reads_a_layout_and_a_refusal() {
        let (layouts, refused) = diagnostics(ORDERING);
        let (ty, l) = &layouts[&(PathBuf::from("/x/core/src/lib.rs"), 404)];
        assert_eq!(ty, "sync::atomic::Ordering");
        assert_eq!((l.size, l.align, l.offsets.clone()), (1, 1, vec![0]));
        let tag = l
            .tag
            .as_ref()
            .map(|t| (t.encoding.clone(), t.variants.len(), t.scalar.end));
        assert_eq!(tag, Some((Encoding::Direct, 1, 4)));
        assert!(refused.contains(&(PathBuf::from("/x/core/src/lib.rs"), 407)));
        assert_eq!(refused.len(), 1);
    }

    #[test]
    fn writes_a_printed_type_as_source() {
        assert_eq!(
            source("fmt::rt::Argument<'_>", "core"),
            "crate::fmt::rt::Argument<'static>"
        );
        assert_eq!(
            source("(u64, core::cmp::Ordering)", "std"),
            "(u64, core::cmp::Ordering)"
        );
        let mut found = BTreeSet::new();
        aggregates("&[(u64, i16, i16); 81]", &mut found);
        assert_eq!(
            found.into_iter().collect::<Vec<_>>(),
            vec!["(u64, i16, i16)"]
        );
    }
}
