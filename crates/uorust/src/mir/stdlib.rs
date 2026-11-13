//! The standard library on the MIR path, from one table keyed by std path
//! (`crates/uorust/mir/std.toml`).
//!
//! A std operation is translated by the same machinery as the workspace's
//! own code where its MIR reaches values: std's own bodies at the pinned
//! toolchain (`crates/uorust/mir/std.mir`, vendored by
//! `scripts/mir/fetch-std.sh`) are instantiated at a call's types and
//! lowered like any body. Where std bottoms out in intrinsics or unsafe
//! raw-pointer code, the table gives the operation's model on the lift's
//! types once, for every call site, with the vectors on which a test runs
//! std itself and the projection proves the model.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::lex::{Decl, Expr};
use crate::lift::{self, Ty};

/// The table.
const TABLE: &str = include_str!("../../mir/std.toml");
/// std's own MIR for the items the table lifts, at the pinned toolchain.
const BODIES: &str = include_str!("../../mir/std.mir");

/// A type pattern over the lift's types.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StdPattern {
    Bool,
    Str,
    Char,
    Usize,
    Nat,
    /// A string or a character.
    Text,
    Unit,
    Var(String),
    List(Box<StdPattern>),
    Option(Box<StdPattern>),
    Set(Box<StdPattern>),
    Map(Box<StdPattern>, Box<StdPattern>),
    /// `result<P, Q>`: a `Result` of a `P` or a `Q`.
    Outcome(Box<StdPattern>, Box<StdPattern>),
    /// `u8`, `u16`, ..: an unsigned integer of that width.
    Fixed(u32),
    /// `named<fmt::Error>`: a struct the crate's scope declares, by its
    /// path from the crate's root, under the name the scope lifts it as.
    Named(String),
    /// `fn<P.., R>`: a pure function of the `P`s to an `R`.
    Function(Vec<StdPattern>, Box<StdPattern>),
    /// A tuple of these, `tuple<P, Q, ..>`; `cursor<P>` is a list of `P`
    /// and the two positions an iterator over it has reached from its
    /// front and its back, `tuple<list<P>, usize, usize>`: a slice
    /// iterator's `ptr` and `end` as indices into the slice.
    Tuple(Vec<StdPattern>),
}

/// Bindings of a pattern's variables.
pub(crate) type PatternBinds = BTreeMap<String, Ty>;

/// A modelled operation.
#[derive(Clone, Debug)]
pub(crate) struct StdModel {
    pub(crate) path: String,
    args: Vec<StdPattern>,
    result: StdPattern,
    term: String,
    update: BTreeMap<usize, String>,
    /// For an operation whose result is an `Option`: the condition under
    /// which it is `Some`, and then `term` is the value it holds.
    guard: Option<String>,
    /// The Rust expression std evaluates on a vector, `{0}`.. its arguments.
    #[cfg(test)]
    pub(crate) rust: String,
    /// Each vector's arguments and std's result, as Rust literals.
    #[cfg(test)]
    pub(crate) vectors: Vec<Vec<String>>,
}

/// An auxiliary definition a model names, before instantiation.
#[derive(Clone, Debug)]
struct StdAux {
    params: Vec<String>,
    result: String,
    recursive: Option<String>,
    body: String,
}

/// The parsed table.
pub(crate) struct StdTable {
    pub(crate) lift: Vec<String>,
    /// The crate's own types the models build values of, as MIR prints
    /// them: a library's scope instantiates each.
    pub(crate) instantiate: Vec<String>,
    types: Vec<(String, StdPattern)>,
    pub(crate) models: Vec<StdModel>,
    aux: BTreeMap<String, StdAux>,
    /// std's bodies by [`key`].
    bodies: BTreeMap<String, String>,
}

fn parse_pat(text: &str) -> Option<StdPattern> {
    let t = text.trim();
    if let Some(v) = t.strip_prefix('$') {
        return Some(StdPattern::Var(v.to_string()));
    }
    let (head, inner) = match t.find('<') {
        Some(at) => (&t[..at], Some(t[at + 1..].strip_suffix('>')?)),
        None => (t, None),
    };
    if let ("named", Some(path)) = (head, inner) {
        let parts: Vec<&str> = std::iter::once("core").chain(path.split("::")).collect();
        return Some(StdPattern::Named(crate::lex::pascal(&parts)));
    }
    let args: Vec<StdPattern> = match inner {
        Some(i) => super::parse::split_top(i, ',')
            .into_iter()
            .map(parse_pat)
            .collect::<Option<_>>()?,
        None => Vec::new(),
    };
    let one = |args: &[StdPattern]| match args {
        [a] => Some(Box::new(a.clone())),
        _ => None,
    };
    Some(match (head, args.as_slice()) {
        ("bool", []) => StdPattern::Bool,
        ("str", []) => StdPattern::Str,
        ("char", []) => StdPattern::Char,
        ("usize", []) => StdPattern::Usize,
        ("nat", []) => StdPattern::Nat,
        ("text", []) => StdPattern::Text,
        ("unit", []) => StdPattern::Unit,
        ("u8", []) => StdPattern::Fixed(8),
        ("u16", []) => StdPattern::Fixed(16),
        ("u32", []) => StdPattern::Fixed(32),
        ("u64", []) => StdPattern::Fixed(64),
        ("u128", []) => StdPattern::Fixed(128),
        ("fn", [ins @ .., out]) => StdPattern::Function(ins.to_vec(), Box::new(out.clone())),
        ("list", _) => StdPattern::List(one(&args)?),
        ("option", _) => StdPattern::Option(one(&args)?),
        ("set", _) => StdPattern::Set(one(&args)?),
        ("map", [k, v]) => StdPattern::Map(Box::new(k.clone()), Box::new(v.clone())),
        ("result", [t, e]) => StdPattern::Outcome(Box::new(t.clone()), Box::new(e.clone())),
        ("tuple", [_, _, ..]) => StdPattern::Tuple(args),
        ("cursor", [p]) => StdPattern::Tuple(vec![
            StdPattern::List(Box::new(p.clone())),
            StdPattern::Usize,
            StdPattern::Usize,
        ]),
        _ => return None,
    })
}

/// The table, parsed once. A malformed table is a build defect the tests
/// report; here it is an empty table.
pub(crate) fn table() -> &'static StdTable {
    static T: OnceLock<StdTable> = OnceLock::new();
    T.get_or_init(|| {
        parse_table(TABLE, BODIES).unwrap_or_else(|| StdTable {
            lift: Vec::new(),
            instantiate: Vec::new(),
            types: Vec::new(),
            models: Vec::new(),
            aux: BTreeMap::new(),
            bodies: BTreeMap::new(),
        })
    })
}

/// The table, or `None` where a row lacks a field or a pattern does not
/// read.
fn parse_table(text: &str, bodies: &str) -> Option<StdTable> {
    let doc: toml::Value = text.parse().ok()?;
    let strs = |v: Option<&toml::Value>| -> Vec<String> {
        v.and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_str)
            .map(str::to_string)
            .collect()
    };
    let field = |row: &toml::Value, n: &str| -> Option<String> {
        row.get(n).and_then(toml::Value::as_str).map(str::to_string)
    };
    let pat = |s: &str| parse_pat(s);
    let mut types = Vec::new();
    for row in doc
        .get("type")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        types.push((field(row, "path")?, pat(&field(row, "model")?)?));
    }
    let mut models = Vec::new();
    for row in doc
        .get("model")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut update = BTreeMap::new();
        if let Some(u) = row.get("update").and_then(toml::Value::as_table) {
            for (k, v) in u {
                let i: usize = k.parse().ok()?;
                update.insert(i, v.as_str().unwrap_or_default().to_string());
            }
        }
        models.push(StdModel {
            path: field(row, "path")?,
            args: strs(row.get("args"))
                .iter()
                .map(|a| pat(a))
                .collect::<Option<_>>()?,
            result: pat(&field(row, "result")?)?,
            term: field(row, "term")?,
            update,
            guard: field(row, "guard"),
            #[cfg(test)]
            rust: field(row, "rust")?,
            #[cfg(test)]
            vectors: row
                .get("vectors")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .map(|v| strs(Some(v)))
                .collect(),
        });
    }
    let mut aux = BTreeMap::new();
    for (name, row) in doc
        .get("aux")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
    {
        aux.insert(
            name.clone(),
            StdAux {
                params: strs(row.get("params")),
                result: field(row, "result")?,
                recursive: row
                    .get("recursive")
                    .and_then(toml::Value::as_str)
                    .map(str::to_string),
                body: field(row, "body")?,
            },
        );
    }
    Some(StdTable {
        lift: strs(doc.get("lift")),
        instantiate: strs(doc.get("instantiate")),
        types,
        models,
        aux,
        bodies: split_bodies(bodies),
    })
}

/// std.mir's bodies, each from its `fn` line to its closing brace, by
/// [`key`] of the path it defines.
fn split_bodies(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("fn ") {
            let path = head.split('(').next().unwrap_or(head);
            current = Some((key(path), String::new()));
        }
        if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
            if line == "}" {
                if let Some((k, b)) = current.take() {
                    out.insert(k, b);
                }
            }
        }
    }
    out
}

/// A std path without its crate and with each `<impl X>` named by `X`:
/// `core::str::<impl str>::trim` and `std::str::<impl str>::trim` are both
/// `str::str::trim`. The facade crate and the crate defining an item print
/// one item under both.
pub(crate) fn key(path: &str) -> String {
    let segs = super::parse::split_path(path);
    let rest = match segs.first() {
        Some(&("core" | "alloc" | "std")) => &segs[1..],
        _ => &segs[..],
    };
    rest.iter()
        .map(|s| {
            if let Some(t) = s.strip_prefix("<impl ").and_then(|r| r.strip_suffix('>')) {
                // The impl on slices of any item type.
                return if t.starts_with('[') {
                    "[]".to_string()
                } else {
                    t.split('<').next().unwrap_or(t).to_string()
                };
            }
            // `<X as Trait>`: by X's module and name and the trait's name,
            // so a type std re-exports at a shorter path than the module
            // that defines it (`std::slice::Iter`, `core::slice::iter::Iter`
            // inside core), and an impl on slices of any item, key alike.
            match s
                .strip_prefix('<')
                .and_then(|r| r.strip_suffix('>'))
                .and_then(|r| r.split_once(" as "))
            {
                Some((on, tr)) => {
                    let tr = super::parse::split_path(tr);
                    format!(
                        "<{} as {}>",
                        canon(on),
                        tr.last().copied().unwrap_or_default()
                    )
                }
                None => (*s).to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}

/// A std type's path by its first module and its name, references and
/// slices kept: `std::slice::Iter`, `core::slice::iter::Iter` and
/// `slice::iter::Iter` (as core prints its own) are all `slice::Iter`.
pub(crate) fn canon(path: &str) -> String {
    let t = path.trim();
    let bare = t.trim_start_matches('&');
    let (refs, bare) = match bare.strip_prefix("mut ") {
        Some(b) => (format!("{}mut ", &t[..t.len() - bare.len()]), b),
        None => (t[..t.len() - bare.len()].to_string(), bare),
    };
    // A trait object keys by its trait: `dyn core::fmt::Write` and `dyn
    // fmt::Write` alike.
    let (refs, bare) = match bare.strip_prefix("dyn ") {
        Some(b) => (format!("{refs}dyn "), b),
        None => (refs, bare),
    };
    if bare.starts_with('[') {
        return format!("{refs}[]");
    }
    let segs = super::parse::split_path(bare);
    let segs: &[&str] = match segs.first() {
        Some(&("core" | "alloc" | "std")) => &segs[1..],
        _ => &segs[..],
    };
    match segs {
        [first, .., last] => format!("{refs}{first}::{last}"),
        _ => format!("{refs}{}", segs.join("::")),
    }
}

/// Bind `p` against `t`.
fn bind(p: &StdPattern, t: &Ty, binds: &mut PatternBinds) -> bool {
    match (p, t) {
        (StdPattern::Bool, Ty::Bool)
        | (StdPattern::Str | StdPattern::Text, Ty::Str)
        | (StdPattern::Char | StdPattern::Text, Ty::Char)
        | (StdPattern::Usize, Ty::Nat(None))
        | (StdPattern::Nat, Ty::Nat(_))
        | (StdPattern::Unit, Ty::Unit) => true,
        (StdPattern::Var(v), t) => match binds.get(v) {
            Some(b) => b.render() == t.render(),
            None => {
                binds.insert(v.clone(), t.clone());
                true
            }
        },
        (StdPattern::List(p), Ty::List(t))
        | (StdPattern::Option(p), Ty::Option(t))
        | (StdPattern::Set(p), Ty::Set(t, true)) => bind(p, t, binds),
        (StdPattern::Map(pk, pv), Ty::Map(k, v, true)) => bind(pk, k, binds) && bind(pv, v, binds),
        (StdPattern::Outcome(pt, pe), Ty::Outcome(t, e)) => {
            bind(pt, t, binds) && bind(pe, e, binds)
        }
        (StdPattern::Fixed(b), Ty::Nat(Some(w))) => b == w,
        (StdPattern::Named(n), Ty::Named(m)) => n == m,
        (StdPattern::Function(ps, pr), Ty::Fn(ts, r)) if ps.len() == ts.len() => {
            ps.iter().zip(ts).all(|(p, t)| bind(p, t, binds)) && bind(pr, r, binds)
        }
        (StdPattern::Tuple(ps), Ty::Tuple(ts)) if ps.len() == ts.len() => {
            ps.iter().zip(ts).all(|(p, t)| bind(p, t, binds))
        }
        _ => false,
    }
}

/// The type a pattern names under `binds`.
fn build(p: &StdPattern, binds: &PatternBinds) -> Option<Ty> {
    Some(match p {
        StdPattern::Bool => Ty::Bool,
        StdPattern::Str => Ty::Str,
        StdPattern::Char => Ty::Char,
        StdPattern::Usize => Ty::Nat(None),
        StdPattern::Unit => Ty::Unit,
        StdPattern::Nat | StdPattern::Text => return None,
        StdPattern::Var(v) => binds.get(v)?.clone(),
        StdPattern::List(p) => Ty::List(Box::new(build(p, binds)?)),
        StdPattern::Option(p) => Ty::Option(Box::new(build(p, binds)?)),
        StdPattern::Set(p) => Ty::Set(Box::new(build(p, binds)?), true),
        StdPattern::Map(k, v) => {
            Ty::Map(Box::new(build(k, binds)?), Box::new(build(v, binds)?), true)
        }
        StdPattern::Outcome(t, e) => {
            Ty::Outcome(Box::new(build(t, binds)?), Box::new(build(e, binds)?))
        }
        StdPattern::Fixed(b) => Ty::Nat(Some(*b)),
        StdPattern::Named(n) => Ty::Named(n.clone()),
        StdPattern::Function(ps, r) => Ty::Fn(
            ps.iter().map(|p| build(p, binds)).collect::<Option<_>>()?,
            Box::new(build(r, binds)?),
        ),
        StdPattern::Tuple(ps) => {
            Ty::Tuple(ps.iter().map(|p| build(p, binds)).collect::<Option<_>>()?)
        }
    })
}

/// The variables of a pattern, in order.
fn vars(p: &StdPattern, out: &mut Vec<String>) {
    match p {
        StdPattern::Var(v) if !out.contains(v) => out.push(v.clone()),
        StdPattern::List(p) | StdPattern::Option(p) | StdPattern::Set(p) => vars(p, out),
        StdPattern::Map(k, v) | StdPattern::Outcome(k, v) => {
            vars(k, out);
            vars(v, out);
        }
        StdPattern::Tuple(ps) => {
            for p in ps {
                vars(p, out);
            }
        }
        StdPattern::Function(ps, r) => {
            for p in ps {
                vars(p, out);
            }
            vars(r, out);
        }
        _ => {}
    }
}

/// The lift's type for a std type the table models, by the path MIR prints
/// and its type arguments.
pub(crate) fn model_type(path: &str, args: &[Ty]) -> Option<Ty> {
    // A type core prints by its name alone (`Arguments<'_>`, in scope
    // where it is used): the one model of that name.
    let bare = |p: &str| !path.contains("::") && p.rsplit("::").next() == Some(path);
    let mut named = table().types.iter().filter(|(p, _)| bare(p));
    let (_, pat) = match table().types.iter().find(|(p, _)| canon(p) == canon(path)) {
        Some(m) => m,
        None => match (named.next(), named.next()) {
            (Some(m), None) => m,
            _ => return None,
        },
    };
    let mut names = Vec::new();
    vars(pat, &mut names);
    if names.len() != args.len() {
        return None;
    }
    let binds: PatternBinds = names.into_iter().zip(args.iter().cloned()).collect();
    build(pat, &binds)
}

/// An applied model: its value, the new values of its `&mut` arguments, the
/// auxiliary definitions it names and the types it mentions.
#[derive(Clone, Debug, Default)]
pub(crate) struct Applied {
    pub(crate) term: String,
    pub(crate) updates: BTreeMap<usize, String>,
    pub(crate) aux: Vec<Decl>,
    pub(crate) mentions: Vec<Ty>,
    /// Where the result is an `Option` the model guards: the condition
    /// under which it is `Some`, `term` then being the value it holds.
    pub(crate) guard: Option<String>,
}

/// A value of each type, where the lift has one: what a model writes for an
/// item it reads where its guard has excluded the case.
pub(crate) type Defaults<'a> = &'a dyn Fn(&Ty) -> Option<String>;

/// The model of the std operation `callee` (generic arguments erased) at
/// these argument and result types.
pub(crate) fn apply(
    callee: &str,
    args: &[(String, Ty)],
    dt: &Ty,
    defaults: Defaults<'_>,
) -> Option<Applied> {
    let k = key(callee);
    // Core prints its own items by their shortest path in scope
    // (`Arguments::from_str`): the one model whose path ends it.
    let tail = format!("::{k}");
    let mut ends = table()
        .models
        .iter()
        .filter(|m| key(&m.path).ends_with(&tail));
    let model = match table().models.iter().find(|m| key(&m.path) == k) {
        Some(m) => m,
        None => match (ends.next(), ends.next()) {
            (Some(m), None) if k.contains("::") && !k.starts_with('<') => m,
            _ => return None,
        },
    };
    if model.args.len() != args.len() {
        return None;
    }
    let mut binds = PatternBinds::new();
    for (p, (_, t)) in model.args.iter().zip(args) {
        if !bind(p, t, &mut binds) {
            return None;
        }
    }
    if !bind(&model.result, dt, &mut binds) {
        return None;
    }
    let terms: Vec<String> = args.iter().map(|(v, _)| v.clone()).collect();
    let mut out = Applied {
        mentions: binds.values().cloned().collect(),
        ..Applied::default()
    };
    let r = Renderer {
        args: &terms,
        binds: &binds,
        defaults,
    };
    out.guard = match &model.guard {
        Some(g) => Some(r.render(g, None, None, &mut out.aux)?),
        None => None,
    };
    out.term = r.render(&model.term, None, out.guard.as_deref(), &mut out.aux)?;
    for (i, u) in &model.update {
        let v = r.render(u, None, out.guard.as_deref(), &mut out.aux)?;
        out.updates.insert(*i, v);
    }
    for t in binds.values() {
        out.mentions.extend(t.mentions());
    }
    if let Ty::Tuple(_) = dt {
        out.mentions.push(dt.clone());
    }
    for (_, t) in args {
        if let Ty::Tuple(_) = t {
            out.mentions.push(t.clone());
        }
    }
    if let (Some(k), Some(v)) = (binds.get("k"), binds.get("v")) {
        out.mentions.push(Ty::Tuple(vec![k.clone(), v.clone()]));
    }
    Some(out)
}

/// The text inside the innermost `{...}` that holds no other, with its span.
fn innermost(text: &str) -> Option<(usize, usize)> {
    let mut open = None;
    for (i, c) in text.char_indices() {
        match c {
            '{' => open = Some(i),
            '}' => return open.map(|o| (o, i)),
            _ => {}
        }
    }
    None
}

/// Split at spaces outside parentheses.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i64;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c == ' ' && depth == 0 {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// What a template renders against: the call's argument terms, the bound
/// type variables, and a value of each type.
struct Renderer<'a> {
    args: &'a [String],
    binds: &'a PatternBinds,
    defaults: Defaults<'a>,
}

impl Renderer<'_> {
    /// Render a template: `None` where a placeholder has no meaning at these
    /// types (an order the lift does not model, an unbound variable).
    fn render(
        &self,
        template: &str,
        own: Option<&str>,
        guard: Option<&str>,
        aux: &mut Vec<Decl>,
    ) -> Option<String> {
        let binds = self.binds;
        let mut text = template.to_string();
        while let Some((open, close)) = innermost(&text) {
            let inner = text[open + 1..close].to_string();
            let w = words(&inner);
            let ty = |v: &str| binds.get(v.strip_prefix('$')?);
            let entry = || {
                Some(Ty::Tuple(vec![
                    binds.get("k")?.clone(),
                    binds.get("v")?.clone(),
                ]))
            };
            let value = match w.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                [n] if n.chars().all(|c| c.is_ascii_digit()) => {
                    self.args.get(n.parse::<usize>().ok()?)?.clone()
                }
                ["ty", v] => ty(v)?.render(),
                ["prelude", f] => format!("{}::{f}", lift::PRELUDE),
                ["self"] => own?.to_string(),
                ["guard"] => guard?.to_string(),
                ["entry"] => entry()?.render(),
                ["key", x] => lift::tuple_item(&entry()?, x, 0),
                ["value", x] => lift::tuple_item(&entry()?, x, 1),
                ["pair", a, b] => {
                    lift::tuple_value(&entry()?, &[(*a).to_string(), (*b).to_string()])
                }
                ["default", v] => (self.defaults)(ty(v)?)?,
                // A tuple of the type the pattern names, from its items.
                ["make", pattern, items @ ..] => {
                    let t = build(&parse_pat(pattern)?, binds)?;
                    let items: Vec<String> = items.iter().map(|i| (*i).to_string()).collect();
                    match &t {
                        Ty::Tuple(ts) if ts.len() == items.len() => lift::tuple_value(&t, &items),
                        _ => return None,
                    }
                }
                ["item", i, x] => format!("(project {x} item{})", i.parse::<usize>().ok()?),
                ["eq", v, a, b] => match ty(v)? {
                    Ty::Str | Ty::Char | Ty::Bool => format!("(prim equal {a} {b} : Bool)"),
                    Ty::Nat(_) => format!("({a} =? {b})"),
                    _ => return None,
                },
                ["lt", v, a, b] => match ty(v)? {
                    Ty::Str | Ty::Char => format!("(prim less_than {a} {b} : Bool)"),
                    Ty::Nat(_) => format!("({a} <? {b})"),
                    _ => return None,
                },
                ["aux", n] => self.instantiate(n, aux)?,
                _ => return None,
            };
            // A rendered record literal's braces are not placeholders.
            let value = value.replace('{', "\u{e000}").replace('}', "\u{e001}");
            text.replace_range(open..=close, &value);
        }
        Some(text.replace('\u{e000}', "{").replace('\u{e001}', "}"))
    }

    /// The auxiliary definition `n` at the bound types, added to `aux`: its
    /// name.
    fn instantiate(&self, n: &str, aux: &mut Vec<Decl>) -> Option<String> {
        let binds = self.binds;
        let a = table().aux.get(n)?;
        let all = format!("{} {} {}", a.params.join(" "), a.result, a.body);
        let tags: String = binds
            .iter()
            .filter(|(v, _)| {
                all.contains(&format!("${v}"))
                    || (matches!(v.as_str(), "k" | "v") && all.contains("{entry}"))
            })
            .map(|(_, t)| t.tag())
            .collect();
        let name = format!("{}{tags}", crate::lex::camel(&["std", n]));
        if aux.iter().any(|d| d.name() == name) {
            return Some(name);
        }
        // An auxiliary definition reads only its own parameters.
        let own = Renderer {
            args: &[],
            binds,
            defaults: self.defaults,
        };
        let mut params = Vec::new();
        for p in &a.params {
            let (pn, pt) = p.split_once(" : ")?;
            params.push((pn.to_string(), own.render(pt, Some(&name), None, aux)?));
        }
        let result = own.render(&a.result, Some(&name), None, aux)?;
        let body = own.render(&a.body, Some(&name), None, aux)?;
        aux.push(Decl::Definition {
            name: name.clone(),
            parameters: params,
            result,
            body: Expr::Flat(body),
            axioms: Vec::new(),
            recursive: a.recursive.clone(),
            termination: None,
            mutual: None,
        });
        Some(name)
    }
}

/// An instance of a std body: the item's [`key`], its MIR text at the
/// call's types, and each generic parameter with the type it binds.
pub(crate) struct Instance {
    pub(crate) path: String,
    pub(crate) text: String,
    pub(crate) bound: Vec<(String, String)>,
}

/// std's own body of `callee` as MIR text, with its generic parameters
/// instantiated at the types MIR prints for the call's arguments and
/// destination; `None` when the table does not lift it or a parameter does
/// not bind. A qualified call `<X as Trait>::m` with no body of its own is
/// the trait's provided method with `Self` as `X`.
pub(crate) fn instance(callee: &str, arg_texts: &[Option<String>], dest: &str) -> Option<Instance> {
    let t = table();
    let mut generics: BTreeMap<String, String> = BTreeMap::new();
    let k = key(&super::lower::erase(callee));
    let (k, text) = match t
        .bodies
        .get(&k)
        .map(|b| (k.clone(), b))
        .or_else(|| implementor(&k))
    {
        Some((k, b)) => (k, b),
        None => {
            let inner = callee.strip_prefix('<')?;
            let (self_ty, rest) = inner.split_once(" as ")?;
            let (trait_path, method) = rest.split_once(">::")?;
            let k = format!("{}::{method}", key(&super::lower::erase(trait_path)));
            generics.insert("Self".into(), self_ty.to_string());
            (k.clone(), t.bodies.get(&k)?)
        }
    };
    if !t.lift.iter().any(|l| key(l) == k) {
        return None;
    }
    let head = text.lines().next()?.strip_prefix("fn ")?;
    let open = head.find('(')?;
    let close = head.rfind(") -> ")?;
    let params = super::parse::split_top(&head[open + 1..close], ',');
    let ret = head[close + ") -> ".len()..].trim_end_matches(" {").trim();
    if params.len() != arg_texts.len() {
        return None;
    }
    for (p, a) in params.iter().zip(arg_texts) {
        let (_, pt) = p.split_once(": ")?;
        if let Some(a) = a {
            unify(pt, a, &mut generics)?;
        }
    }
    unify(ret, dest, &mut generics)?;
    let mut out = String::new();
    for line in text.lines() {
        out.push_str(&substitute(line, &generics));
        out.push('\n');
    }
    Some(Instance {
        path: k,
        text: out,
        bound: generics.into_iter().collect(),
    })
}

/// The body of `<X as Trait>::m` that X's own impl of the trait gives, by
/// the module X is in and X's name: `<std::slice::Iter as Iterator>::any`
/// is `core::slice::iter::<impl Iter>::any` (std re-exports a type at a
/// shorter path than the module that defines it).
fn implementor(k: &str) -> Option<(String, &'static String)> {
    let inner = k.strip_prefix('<')?;
    let (on, rest) = inner.split_once(" as ")?;
    let (_, method) = rest.split_once(">::")?;
    let segs = super::parse::split_path(on.trim_start_matches('&'));
    let segs: &[&str] = match segs.first() {
        Some(&("core" | "alloc" | "std")) => &segs[1..],
        _ => &segs[..],
    };
    let (module, name) = (segs.first()?, segs.last()?);
    if segs.len() < 2 {
        return None;
    }
    table().bodies.iter().find_map(|(bk, b)| {
        let bs = super::parse::split_path(bk);
        let n = bs.len();
        (n >= 3 && bs[0] == *module && bs[n - 2] == *name && bs[n - 1] == method)
            .then(|| (bk.clone(), b))
    })
}

const PRIMITIVES: &[&str] = &[
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64", "!",
];

/// A type's text without its lifetimes: `&'a T` is `&T`.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text.trim();
    while let Some(at) = rest.find('\'') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        rest = after[end..].trim_start_matches([' ', ',']);
        if out.ends_with('<') && rest.starts_with('>') {
            out.pop();
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Unify a std parameter's type text with a call site's: a bare name that
/// is no primitive is a generic parameter and binds the call site's text.
pub(crate) fn unify(
    pattern: &str,
    actual: &str,
    generics: &mut BTreeMap<String, String>,
) -> Option<()> {
    let (p, a) = (plain(pattern), plain(actual));
    let bare = !p.is_empty()
        && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && p.starts_with(|c: char| c.is_ascii_uppercase())
        && !PRIMITIVES.contains(&p.as_str());
    // A type printed with or without its path is one type: `Option<u16>`
    // is `option::Option<u16>`.
    let unpathed = |t: &str| -> String {
        let mut out = String::new();
        for (i, part) in t.split("::").enumerate() {
            if i > 0 {
                // The segment before `::` was a path's, not the type's.
                let keep = out
                    .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .map_or(0, |at| at + 1);
                out.truncate(keep);
            }
            out.push_str(part);
        }
        out
    };
    // A projection `<X as Trait>::Assoc` names a type the link does not
    // normalize: it matches any type and binds none.
    if p.starts_with('<') && p.contains(" as ") {
        return Some(());
    }
    // An argument-position `impl Trait` is a parameter of its own, which no
    // name binds: it matches any type.
    if p.starts_with("impl ") {
        return Some(());
    }
    if bare {
        return match generics.get(&p) {
            Some(b) if unpathed(&plain(b)) != unpathed(&a) => None,
            Some(_) => Some(()),
            None => {
                generics.insert(p, a);
                Some(())
            }
        };
    }
    if let (Some(p), Some(a)) = (p.strip_prefix('&'), a.strip_prefix('&')) {
        // A shared and a mutable borrow are two types.
        let (pm, am) = (p.strip_prefix("mut "), a.strip_prefix("mut "));
        if pm.is_some() != am.is_some() {
            return None;
        }
        return unify(pm.unwrap_or(p), am.unwrap_or(a), generics);
    }
    // A raw pointer, a slice, an array or a tuple: its parts, in order.
    for k in ["*const ", "*mut "] {
        if let (Some(p), Some(a)) = (p.strip_prefix(k), a.strip_prefix(k)) {
            return unify(p, a, generics);
        }
    }
    for (open, close, sep) in [('[', ']', ';'), ('(', ')', ',')] {
        let inner = |t: &str| {
            t.strip_prefix(open)
                .and_then(|r| r.strip_suffix(close))
                .map(str::to_string)
        };
        if let (Some(p), Some(a)) = (inner(&p), inner(&a)) {
            let (ps, as_) = (
                super::parse::split_top(&p, sep),
                super::parse::split_top(&a, sep),
            );
            let ps: Vec<&str> = ps
                .iter()
                .map(|x| x.trim())
                .filter(|x| !x.is_empty())
                .collect();
            let as_: Vec<&str> = as_
                .iter()
                .map(|x| x.trim())
                .filter(|x| !x.is_empty())
                .collect();
            if ps.len() != as_.len() {
                return None;
            }
            for (x, y) in ps.iter().zip(&as_) {
                unify(x, y, generics)?;
            }
            return Some(());
        }
    }
    let split = |t: &str| -> (String, Vec<String>) {
        let segs = super::parse::split_path(t);
        let last = segs.last().copied().unwrap_or(t);
        match last.find('<') {
            Some(at) => (
                last[..at].trim_end_matches("::").to_string(),
                super::parse::split_top(last[at + 1..].strip_suffix('>').unwrap_or(""), ',')
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
            None => (last.to_string(), Vec::new()),
        }
    };
    let ((pn, pa), (an, aa)) = (split(&p), split(&a));
    // MIR prints a type without the trailing arguments its defaults give
    // (`ControlFlow<B>` for `ControlFlow<B, ()>`): the arguments printed
    // are the first ones.
    if pn != an || pa.len() < aa.len() {
        return None;
    }
    for (x, y) in pa.iter().zip(&aa) {
        unify(x, y, generics)?;
    }
    Some(())
}

/// Replace each generic parameter, as a whole word that is not a path's
/// segment, by its binding.
pub(crate) fn substitute(line: &str, generics: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let after_path = start >= 2 && chars[start - 1] == ':' && chars[start - 2] == ':';
            let before_path = chars.get(i) == Some(&':') && chars.get(i + 1) == Some(&':');
            let local = start >= 1 && chars[start - 1] == '_';
            match generics.get(&word) {
                Some(b) if !after_path && !before_path && !local => out.push_str(b),
                _ => out.push_str(&word),
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses_and_names_only_vendored_bodies() {
        let t = parse_table(TABLE, BODIES).expect("std.toml parses");
        assert!(!t.models.is_empty());
        for l in &t.lift {
            assert!(
                t.bodies.contains_key(&key(l)),
                "{l} is lifted but std.mir has no body of it: run scripts/mir/fetch-std.sh"
            );
        }
        for m in &t.models {
            assert!(!m.vectors.is_empty(), "{} has no vectors", m.path);
            for v in &m.vectors {
                assert_eq!(v.len(), m.args.len() + 1, "{}: a vector's arity", m.path);
            }
        }
    }

    #[test]
    fn std_paths_key_without_their_crate() {
        assert_eq!(key("core::str::<impl str>::trim"), "str::str::trim");
        assert_eq!(key("std::str::<impl str>::trim"), "str::str::trim");
        assert_eq!(
            key("std::option::Option::unwrap_or"),
            "option::Option::unwrap_or"
        );
        assert_eq!(
            key("core::slice::<impl [lift::Ty]>::contains"),
            "slice::[]::contains"
        );
        assert_eq!(
            key("<&[ast::Stmt] as std::iter::IntoIterator>::into_iter"),
            key("<&[T] as std::iter::IntoIterator>::into_iter")
        );
    }

    #[test]
    fn models_apply_at_their_types() {
        let a = apply(
            "core::str::<impl str>::trim",
            &[("s".into(), Ty::Str)],
            &Ty::Str,
            &|_| None,
        )
        .expect("trim");
        assert_eq!(a.term, "(Prelude::textTrim s)");
        assert!(apply(
            "core::str::<impl str>::trim",
            &[("s".into(), Ty::Bool)],
            &Ty::Str,
            &|_| None,
        )
        .is_none());
        let set = Ty::Set(Box::new(Ty::Str), true);
        let ins = apply(
            "std::collections::BTreeSet::insert",
            &[("s".into(), set), ("x".into(), Ty::Str)],
            &Ty::Bool,
            &|_| None,
        )
        .expect("insert");
        assert_eq!(ins.updates[&0], "(stdInsertString s x)");
        assert!(ins.aux.iter().any(|d| d.name() == "stdMemberString"));
        assert_eq!(
            model_type("std::collections::BTreeSet", &[Ty::Str]),
            Some(Ty::Set(Box::new(Ty::Str), true))
        );
    }

    #[test]
    fn generic_parameters_bind_to_the_call_sites_types() {
        let mut g = BTreeMap::new();
        assert!(unify("Option<T>", "std::option::Option<u32>", &mut g).is_some());
        assert_eq!(g["T"], "u32");
        assert!(unify("&'a T", "&u32", &mut g).is_some());
        assert!(unify("T", "u8", &mut g).is_none());
        assert_eq!(
            substitute(
                "fn f(_1: Option<T>) -> T { <T as clone::Clone>::clone }",
                &g
            ),
            "fn f(_1: Option<u32>) -> u32 { <u32 as clone::Clone>::clone }"
        );
    }
}
