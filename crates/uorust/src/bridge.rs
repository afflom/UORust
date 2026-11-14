//! The bridge (UE-19): each translated body's shallow LexLean, the function
//! the MIR path lowers it to, proved a refinement of the evaluator run on
//! its deep embedding.
//!
//! For each translated κ whose signature the bridge encodes, a theorem
//! states that for every input the evaluator, run on the embedded body
//! with the fuel its blocks bound, returns the encoding of the shallow
//! function's result. The proof is the generator's, never the body's:
//! case analysis on each Boolean input, then the evaluator's own
//! reduction. `uorust bridge` verifies the project, sets each theorem Lean
//! refutes aside as `pending` (with the shape that stopped it, ranked in the
//! frontier), and keeps the rest, which verify; none is asserted and no
//! axiom is used.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::embed::{def_name, mir_ctor, mir_name, mir_string, term_list, Embedded, Program};
use crate::mir::core::CoreShard;
use crate::UorustError;

/// A translated body's shallow LexLean: its function, that function's
/// signature, and every declaration its translation renders, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shallow {
    /// The function the body lowers to.
    pub function: String,
    /// Its parameters, `(name, type)`.
    pub parameters: Vec<(String, String)>,
    /// Its result type.
    pub result: String,
    /// The translation's declarations, the function's among them, each
    /// `(name, rendered)`, in the order the translation renders them.
    pub decls: Vec<(String, String)>,
}

impl Shallow {
    /// The shallow LexLean of a body lowered to the function `name`.
    pub(crate) fn of(
        name: &str,
        lowered: &crate::mir::lower::Lowered,
        types: &crate::lift::Types,
    ) -> Option<Self> {
        let (parameters, result) = lowered.decls.iter().find_map(|d| match d {
            crate::lex::Decl::Definition {
                name: n,
                parameters,
                result,
                ..
            } if n == name => Some((parameters.clone(), result.clone())),
            _ => None,
        })?;
        // The types its declarations name, each after those its fields
        // name, as a project declares them.
        let mut typed = Vec::new();
        let mut emitted = BTreeSet::new();
        for t in &lowered.types {
            crate::project::type_decls(t, types, &BTreeSet::new(), &mut emitted, &mut typed);
        }
        let decls = typed
            .iter()
            .chain(lowered.aux.values())
            .chain(&lowered.decls)
            .map(|d| (d.name().to_string(), crate::lex::render_decl(d)))
            .collect();
        Some(Self {
            function: name.to_string(),
            parameters,
            result,
            decls,
        })
    }
}

/// Write each module's shallow translations under `dir`, `{module}.toml`:
/// for each translated row, its κ, its bindings, its function and that
/// function's signature, and its declarations.
pub fn write_shallow(dir: &Path, shards: &[CoreShard]) -> Result<(), UorustError> {
    std::fs::create_dir_all(dir).map_err(|e| crate::error::io(dir, e))?;
    for shard in shards {
        let mut rows = Vec::new();
        for row in &shard.rows {
            let Some(s) = &row.shallow else { continue };
            let mut t = toml::Table::new();
            t.insert("kappa".into(), row.kappa.clone().into());
            t.insert("at".into(), row.at.clone().into());
            t.insert("function".into(), s.function.clone().into());
            t.insert(
                "parameters".into(),
                toml::Value::Array(
                    s.parameters
                        .iter()
                        .map(|(n, ty)| {
                            toml::Value::Array(vec![n.clone().into(), ty.clone().into()])
                        })
                        .collect(),
                ),
            );
            t.insert("result".into(), s.result.clone().into());
            t.insert(
                "decl".into(),
                toml::Value::Array(
                    s.decls
                        .iter()
                        .map(|(n, text)| {
                            let mut d = toml::Table::new();
                            d.insert("name".into(), n.clone().into());
                            d.insert("lexlean".into(), text.clone().into());
                            toml::Value::Table(d)
                        })
                        .collect(),
                ),
            );
            rows.push(toml::Value::Table(t));
        }
        let mut doc = toml::Table::new();
        doc.insert("fn".into(), toml::Value::Array(rows));
        let text = format!(
            "# The shallow translations (written by `uorust core --lexlean`; do not\n\
             # edit): each translated body's κ, its function and its LexLean.\n\n{}",
            toml::to_string(&doc).map_err(|e| UorustError::Tool(e.to_string()))?
        );
        let file = dir.join(format!("{}.toml", shard.module));
        std::fs::write(&file, text).map_err(|e| crate::error::io(&file, e))?;
    }
    Ok(())
}

/// A value the bridge encodes: a scalar, or an enum of core's prelude over
/// encoded values, as both MIR's type and the shallow type name it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shape {
    Bool,
    Int(u32, bool),
    Char,
    Unit,
    /// A shared reference, which the shallow function takes as its
    /// referent.
    Ref(Box<Shape>),
    Option(Box<Shape>),
    Result(Box<Shape>, Box<Shape>),
}

impl Shape {
    /// The shape of a type as MIR prints it, at a pointer width of `width`.
    fn parse(printed: &str, width: u32) -> Option<Self> {
        let printed = printed.trim();
        match printed {
            "bool" => return Some(Self::Bool),
            "()" => return Some(Self::Unit),
            "char" => return Some(Self::Char),
            _ => {}
        }
        if let Some(inner) = printed.strip_prefix('&') {
            return (!inner.starts_with("mut "))
                .then(|| Self::parse(inner, width))
                .flatten()
                .map(|s| Self::Ref(Box::new(s)));
        }
        if let Some((bits, signed)) = crate::embed::int_ty(printed, width) {
            return Some(Self::Int(bits, signed));
        }
        let (head, args) = printed.strip_suffix('>')?.split_once('<')?;
        let args = top_level(args, ',');
        match (head.rsplit("::").next()?, &args[..]) {
            ("Option", [a]) => Some(Self::Option(Box::new(Self::parse(a, width)?))),
            ("Result", [a, e]) => Some(Self::Result(
                Box::new(Self::parse(a, width)?),
                Box::new(Self::parse(e, width)?),
            )),
            _ => None,
        }
    }

    /// The shallow type the MIR path lowers this shape to.
    fn shallow(&self) -> String {
        match self {
            Self::Bool => "Bool".into(),
            Self::Int(..) | Self::Char => "Nat".into(),
            Self::Unit => "Unit".into(),
            Self::Ref(a) => a.shallow(),
            Self::Option(a) => format!("(Option {})", a.shallow()),
            Self::Result(a, e) => format!("(Result {} {})", a.shallow(), e.shallow()),
        }
    }

    /// A name for the shape, a Lean identifier's suffix.
    fn mangle(&self) -> String {
        match self {
            Self::Bool => "Bool".into(),
            Self::Int(b, s) => format!("{}{b}", if *s { "I" } else { "U" }),
            Self::Char => "Char".into(),
            Self::Unit => "Unit".into(),
            Self::Ref(a) => format!("Ref{}", a.mangle()),
            Self::Option(a) => format!("Option{}", a.mangle()),
            Self::Result(a, e) => format!("Result{}{}", a.mangle(), e.mangle()),
        }
    }

    /// The encoding of the shallow term `t` as the evaluator's value, with
    /// the encoders it names added to `encoders`.
    fn encode(&self, t: &str, encoders: &mut BTreeMap<String, String>) -> String {
        match self {
            Self::Bool => mir_ctor("Value.boolean", &[t.to_string()]),
            Self::Int(bits, signed) => mir_ctor(
                "Value.int",
                &[bits.to_string(), signed.to_string(), t.to_string()],
            ),
            Self::Char => mir_ctor("Value.character", &[t.to_string()]),
            Self::Unit => mir_ctor("Value.tuple", &[format!("(nil {})", mir_name("Value"))]),
            Self::Ref(a) => mir_ctor("Value.reference", &[a.encode(t, encoders)]),
            Self::Option(_) | Self::Result(..) => {
                let name = format!("bridgeEnc{}", self.mangle());
                if !encoders.contains_key(&name) {
                    let def = self.encoder(&name, encoders);
                    encoders.insert(name.clone(), def);
                }
                format!("({name} {t})")
            }
        }
    }

    /// The definition of an enum shape's encoder, `name`: each variant the
    /// evaluator's value of it, by its discriminant.
    fn encoder(&self, name: &str, encoders: &mut BTreeMap<String, String>) -> String {
        let variant = |v: &str, d: u32, field: Option<String>| {
            let fields = match field {
                Some(f) => format!("(cons {f} (nil {}))", mir_name("Value")),
                None => format!("(nil {})", mir_name("Value")),
            };
            mir_ctor(
                "Value.adt",
                &[
                    format!("\"{v}\""),
                    format!(
                        "(record {} {{negative := false, magnitude := {d}}})",
                        mir_name("Signed")
                    ),
                    fields,
                ],
            )
        };
        let arms = match self {
            Self::Option(a) => format!(
                "    | Option.none => {}\n    | Option.some x => {}",
                variant("None", 0, None),
                variant("Some", 1, Some(a.encode("x", encoders)))
            ),
            Self::Result(a, e) => format!(
                "    | Result.ok x => {}\n    | Result.error x => {}",
                variant("Ok", 0, Some(a.encode("x", encoders))),
                variant("Err", 1, Some(e.encode("x", encoders)))
            ),
            _ => String::new(),
        };
        format!(
            "definition{{\n  name: {name},\n  parameters: [o : {}],\n  result: {},\n  body: (match o\n{arms})\n}}",
            self.shallow(),
            mir_name("Value")
        )
    }
}

/// `text` split at the `sep`s outside brackets.
fn top_level(text: &str, sep: char) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut depth = 0i32;
    for c in text.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            _ => {}
        }
        if c == sep && depth == 0 {
            out.push(String::new());
        } else if let Some(last) = out.last_mut() {
            last.push(c);
        }
    }
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

/// A printed type with the row's bindings (`T=u32, U=u32`) substituted for
/// its parameters' bare names.
fn substitute(printed: &str, bindings: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        out.push_str(
            bindings
                .get(word.as_str())
                .map_or(word.as_str(), String::as_str),
        );
        word.clear();
    };
    for c in printed.chars() {
        if c.is_alphanumeric() || c == '_' || c == ':' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// The keys of the bodies `key` reaches, itself first, where each is
/// embedded and the evaluator defines its every construct; else the first
/// reason one is not.
fn reach(every: &BTreeMap<String, (String, Embedded)>, key: &str) -> Bridgeable<Vec<String>> {
    let mut seen = vec![key.to_string()];
    let mut at = 0;
    while let Some(k) = seen.get(at).cloned() {
        let Some((_, e)) = every.get(&k) else {
            return Bridgeable::Pending("closure: a callee not embedded".into());
        };
        if let Some(b) = e.blocked.iter().next() {
            return Bridgeable::Pending(format!("closure: {b}"));
        }
        if !e.bound.is_empty() {
            return Bridgeable::Pending("closure: a callee left to a dictionary".into());
        }
        if e.term
            .contains(&mir_ctor("Generic.unknown", &[]).replace(')', ""))
            || (at == 0
                && e.term
                    .contains(&mir_ctor("Generic.slot", &[]).replace(')', "")))
        {
            return Bridgeable::Pending("closure: a generic no binding fixes".into());
        }
        for c in &e.callees {
            if !seen.contains(c) {
                seen.push(c.clone());
            }
        }
        at += 1;
    }
    Bridgeable::Ready(seen)
}

/// What the bridge makes of a body: what its theorem needs, or the shape
/// that leaves it pending.
enum Bridgeable<T> {
    Ready(T),
    Pending(String),
}

/// One translated body's place in the bridge: the declarations it adds to
/// its part, and its theorem.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Block {
    kappa: String,
    theorem: String,
    /// The declarations it needs, each `(name, text)`, which its part
    /// renders where a block first needs each.
    decls: Vec<(String, String)>,
    /// The text of its theorem.
    statement: String,
    /// The construct of its closure its proof is likeliest to stop at
    /// ([`shape`]), which a refuted theorem is pending at.
    #[serde(default)]
    shape: String,
}

/// A part of the bridge: one module's blocks.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct Part {
    /// Its number, which names its module and never changes.
    id: usize,
    blocks: Vec<Block>,
}

/// The bridge's state, which `uorust bridge` reads and prunes: its parts,
/// and each translated κ set aside, with the shape that stopped it.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct State {
    parts: Vec<Part>,
    pending: BTreeMap<String, String>,
}

/// The most theorems one part holds: a part is one Lean process, which
/// `lexlean verify` stops at the project's `child_timeout_ms`.
const PART_THEOREMS: usize = 8;

/// How long one part's Lean process may run: a reduction the evaluator
/// cannot finish on symbolic inputs is set aside, not waited on.
const PART_TIMEOUT_MS: u64 = 120_000;

/// The state file of a bridge's project.
const STATE: &str = "bridge.json";

/// Write the bridge's project to `out`: for each translated κ of
/// `modules` whose shallow translation `shallow` holds, its theorem, or
/// the reason it has none.
pub(crate) fn project(
    krate: &str,
    modules: &[(String, Vec<(String, Embedded)>)],
    program: &Program,
    shallow: &Path,
    out: &Path,
) -> Result<(), UorustError> {
    let mut state = State::default();
    // The parts being filled, each with the declarations it defines and its
    // size.
    let mut open: Vec<(Part, BTreeMap<String, String>, usize)> = Vec::new();
    let budget = crate::project::MAX_FILE_BYTES / 2;
    for (module, _) in modules {
        let file = shallow.join(format!("{module}.toml"));
        if !file.exists() {
            continue;
        }
        let doc: toml::Table =
            crate::embed::read_text(&file)?
                .parse()
                .map_err(|e| UorustError::Malformed {
                    path: file.clone(),
                    reason: format!("{e}"),
                })?;
        let rows = doc
            .get("fn")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for row in rows {
            let field = |k: &str| {
                row.get(k)
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let kappa = field("kappa");
            if state.pending.contains_key(&kappa)
                || open
                    .iter()
                    .any(|(p, _, _)| p.blocks.iter().any(|b| b.kappa == kappa))
            {
                continue;
            }
            let pairs = |k: &str| -> Vec<(String, String)> {
                row.get(k)
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let p = p.as_array()?;
                        Some((
                            p.first()?.as_str()?.to_string(),
                            p.get(1)?.as_str()?.to_string(),
                        ))
                    })
                    .collect()
            };
            let decls: Vec<(String, String)> = row
                .get("decl")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|d| {
                    Some((
                        d.get("name")?.as_str()?.to_string(),
                        d.get("lexlean")?.as_str()?.to_string(),
                    ))
                })
                .collect();
            let shallow = Shallow {
                function: field("function"),
                parameters: pairs("parameters"),
                result: field("result"),
                decls,
            };
            match block(program, &kappa, &field("at"), &shallow) {
                Bridgeable::Pending(reason) => {
                    state.pending.insert(kappa, reason);
                }
                Bridgeable::Ready((theorem, statement, needed)) => {
                    // The first part with room that defines none of its
                    // names otherwise (a macro's impls share a name), else
                    // a new one.
                    let size: usize =
                        needed.iter().map(|(_, t)| t.len()).sum::<usize>() + statement.len();
                    let fits = |(p, defined, used): &(Part, BTreeMap<String, String>, usize)| {
                        p.blocks.len() < PART_THEOREMS
                            && used + size <= budget
                            && !needed
                                .iter()
                                .any(|(n, t)| defined.get(n).is_some_and(|d| d != t))
                    };
                    let at = match open.iter().position(fits) {
                        Some(at) => at,
                        None => {
                            let id = open.len();
                            open.push((
                                Part {
                                    id,
                                    blocks: Vec::new(),
                                },
                                BTreeMap::new(),
                                0,
                            ));
                            open.len() - 1
                        }
                    };
                    let (part, defined, part_size) = &mut open[at];
                    for (n, t) in &needed {
                        if !defined.contains_key(n) {
                            *part_size += t.len();
                            defined.insert(n.clone(), t.clone());
                        }
                    }
                    *part_size += statement.len();
                    let shape = shape(program, &kappa);
                    part.blocks.push(Block {
                        kappa,
                        theorem,
                        decls: needed,
                        statement,
                        shape,
                    });
                }
            }
        }
    }
    state.parts = open.into_iter().map(|(p, _, _)| p).collect();
    let mut files = crate::embed::embed_project(
        krate,
        &[],
        render_parts(&state),
        crate::project::MAX_FILE_BYTES,
    );
    files.insert(
        STATE.into(),
        serde_json::to_vec(&state).map_err(|e| UorustError::Tool(e.to_string()))?,
    );
    crate::embed::replace_dir(out, Some(&files))
}

/// A theorem's name, its text, and the declarations it needs, each
/// `(name, text)`.
type Theorem = (String, String, Vec<(String, String)>);

/// A translated body's theorem, its statement's text and the declarations
/// it needs, each `(name, text)`; else why it has none.
fn block(program: &Program, kappa: &str, at: &str, shallow: &Shallow) -> Bridgeable<Theorem> {
    let Some((key, locals)) = program.signatures.get(kappa) else {
        return Bridgeable::Pending("unembedded".into());
    };
    let bindings: BTreeMap<String, String> = top_level(at, ',')
        .into_iter()
        .filter_map(|b| {
            let (p, t) = b.split_once('=')?;
            Some((p.trim().to_string(), t.trim().to_string()))
        })
        .collect();
    let shape = |printed: &str, declared: &str| -> Option<Shape> {
        Shape::parse(&substitute(printed, &bindings), program.width)
            .filter(|s| s.shallow() == declared)
    };
    if shallow.parameters.len() + 1 != locals.len() {
        return Bridgeable::Pending("signature: an arity the body does not have".into());
    }
    let Some(result) = shape(&locals[0], &shallow.result) else {
        return Bridgeable::Pending(format!("signature: {}", head(&shallow.result)));
    };
    let mut params: Vec<Shape> = Vec::new();
    for ((_, ty), printed) in shallow.parameters.iter().zip(&locals[1..]) {
        match shape(printed, ty) {
            Some(s) => params.push(s),
            None => return Bridgeable::Pending(format!("signature: {}", head(ty))),
        }
    }
    let closure = match reach(&program.every, key) {
        Bridgeable::Ready(c) => c,
        Bridgeable::Pending(why) => return Bridgeable::Pending(why),
    };
    let mut encoders = BTreeMap::new();
    let args: Vec<String> = shallow
        .parameters
        .iter()
        .zip(&params)
        .map(|((n, _), s)| s.encode(n, &mut encoders))
        .collect();
    let call = if shallow.parameters.is_empty() {
        format!("({})", shallow.function)
    } else {
        let names: Vec<&str> = shallow.parameters.iter().map(|(n, _)| n.as_str()).collect();
        format!("({} {})", shallow.function, names.join(" "))
    };
    let expected = result.encode(&call, &mut encoders);
    let name = def_name(kappa);
    let mut needed: Vec<(String, String)> = shallow.decls.clone();
    let mut bodies = Vec::new();
    let mut steps = 0usize;
    for k in &closure {
        let Some((kappa, e)) = program.every.get(k) else {
            continue;
        };
        steps += e.term.matches("(record Mir::Block ").count()
            + e.term.matches("(ctor Mir::Statement.").count();
        let c = format!("c{}", def_name(kappa));
        needed.push((
            c.clone(),
            format!(
                "definition{{\n  name: {c},\n  parameters: [],\n  result: {},\n  body: {}\n}}",
                mir_name("Body"),
                e.term
            ),
        ));
        bodies.push(format!("({c})"));
    }
    // The shallow function computes at the release build's checks: core's
    // distributed build runs neither UB nor overflow checks.
    needed.push((
        format!("p{name}"),
        format!(
            "definition{{\n  name: p{name},\n  parameters: [],\n  result: {},\n  body: (record {} {{bodies := {}, dictionary := (nil {}), ubChecks := false, contractChecks := false, overflowChecks := false, pointerWidth := {}, bigEndian := {}}})\n}}",
            mir_name("Program"),
            mir_name("Program"),
            term_list(&bodies, &mir_name("Body")),
            mir_name("Binding"),
            program.width,
            program.big_endian
        ),
    ));
    needed.extend(encoders);
    let bits = params
        .iter()
        .map(|s| match s {
            Shape::Int(b, _) => *b as usize,
            _ => 1,
        })
        .max()
        .unwrap_or(1);
    let fuel = steps.max(1) * (bits + 1);
    let theorem = format!("r{name}");
    let parameters: Vec<String> = shallow
        .parameters
        .iter()
        .map(|(n, t)| format!("{n} : {t}"))
        .collect();
    let booleans: Vec<&str> = shallow
        .parameters
        .iter()
        .zip(&params)
        .filter(|(_, s)| **s == Shape::Bool)
        .map(|((n, _), _)| n.as_str())
        .collect();
    let statement = format!(
        "theorem{{\n  name: {theorem},\n  parameters: [{}],\n  statement: (({} {fuel} (p{name}) {} {}) = {}),\n  proof: {}\n}}",
        parameters.join(", "),
        mir_name("evaluate"),
        mir_string(key),
        term_list(&args, &mir_name("Value")),
        mir_ctor("Outcome.returned", &[expected]),
        by_cases(&booleans, "reflexivity{}")
    );
    Bridgeable::Ready((theorem, statement, needed))
}

/// The operators of MIR a proof by the evaluator's reduction is likeliest
/// to stop at on symbolic inputs, hardest first.
const OPERATORS: &[&str] = &[
    "mul", "div", "rem", "shl", "shr", "sub", "add", "bitAnd", "bitOr", "bitXor", "cmp", "lt",
    "le", "gt", "ge", "eq", "ne",
];

/// The construct of a body's closure a proof by reduction is likeliest to
/// stop at: a float operation, a call, an operator (the hardest its
/// closure applies), a cast, a switch on its inputs, else none.
fn shape(program: &Program, kappa: &str) -> String {
    let Some((key, _)) = program.signatures.get(kappa) else {
        return "unembedded".into();
    };
    let keys = match reach(&program.every, key) {
        Bridgeable::Ready(keys) => keys,
        Bridgeable::Pending(_) => Vec::new(),
    };
    let terms: Vec<&str> = keys
        .iter()
        .filter_map(|k| program.every.get(k))
        .map(|(_, e)| e.term.as_str())
        .collect();
    let has = |needle: &str| terms.iter().any(|t| t.contains(needle));
    if has("Rvalue.float") {
        return "float".into();
    }
    if has("(ctor Mir::Terminator.call ") {
        return "call".into();
    }
    if let Some(op) = OPERATORS
        .iter()
        .find(|op| has(&format!("(ctor Mir::BinOp.{op})")))
    {
        return format!("binary {op}");
    }
    if has("(ctor Mir::Rvalue.cast ") {
        return "cast".into();
    }
    if has("(ctor Mir::Terminator.switch ") {
        return "switch".into();
    }
    "straight".into()
}

/// A shallow type's head: `Nat`, `Option`, a structure's name.
fn head(ty: &str) -> String {
    ty.trim_start_matches('(')
        .split([' ', ')'])
        .next()
        .unwrap_or(ty)
        .to_string()
}

/// `leaf` under case analysis on each of the Boolean locals `vars`.
fn by_cases(vars: &[&str], leaf: &str) -> String {
    match vars.split_first() {
        None => leaf.to_string(),
        Some((v, rest)) => {
            let inner = by_cases(rest, leaf);
            format!(
                "cases{{branches: [{{proof: {inner}, binders: [], constructor: \"false\"}}, {{proof: {inner}, binders: [], constructor: \"true\"}}], scrutinee: {v}}}"
            )
        }
    }
}

/// The module of the bridge's part numbered `k`: zero-padded, so that the
/// modules' order is the parts'.
fn part_module(k: usize) -> String {
    format!("BridgePart{k:06}")
}

/// Each nonempty part's module, by its source path, and for each the line
/// each block's declarations and its theorem begin at.
fn render_parts(state: &State) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for p in &state.parts {
        if p.blocks.is_empty() {
            continue;
        }
        let (text, _) = render_part(p);
        files.insert(
            format!("src/{}.lex.tex", part_module(p.id)),
            text.into_bytes(),
        );
    }
    files
}

/// The lines a part's module gives each declaration and each theorem.
#[derive(Default)]
struct Spans {
    /// Each declaration's name and its lines `[start, end)`.
    decls: Vec<(String, usize, usize)>,
    /// Each block's theorem's lines `[start, end)`.
    theorems: Vec<(usize, usize)>,
}

/// A part's module, each declaration rendered where a block first needs
/// it, and the lines each declaration and each theorem span.
fn render_part(p: &Part) -> (String, Spans) {
    let mut text = format!(
        "\\begin{{lexlean}}{{{}}}\n\\useglossary{{lexlean.std.bool@1.2.0}}\n\
         \\useglossary{{lexlean.std.nat@1.2.0}}\n\\useglossary{{uorust.mir@1.0.0}}\n\
         \\importmodule{{Mir}}\n\\importmodule{{Prelude}}\n\\title{{Embedded MIR checks}}\n\\begin{{semanticmodule}}\n",
        part_module(p.id)
    );
    let line = |t: &str| t.matches('\n').count() + 1;
    let mut spans = Spans::default();
    let mut emitted: BTreeSet<&str> = BTreeSet::new();
    for b in &p.blocks {
        for (n, d) in &b.decls {
            if emitted.insert(n) {
                let start = line(&text);
                text.push_str(d);
                text.push_str("\n\n");
                spans.decls.push((n.clone(), start, line(&text)));
            }
        }
        let start = line(&text);
        text.push_str(&b.statement);
        text.push_str("\n\n");
        spans.theorems.push((start, line(&text)));
    }
    text.push_str("\\end{semanticmodule}\n\\end{lexlean}\n");
    (text, spans)
}

/// What `uorust bridge` measured: the translated bodies bridged, and those
/// pending.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bridged {
    /// The bodies whose theorem verifies.
    pub bridged: usize,
    /// The translated bodies with none.
    pub pending: usize,
}

/// The most verifier runs `uorust bridge` makes before it refuses: each
/// sets aside at least one theorem, and the link phase reports one lack a
/// run.
const ROUNDS: usize = 100_000;

/// The diagnostic of LexLean's link phase, which names what a module lacks.
const LINK: &str = "LLT4001";

/// Verify the bridge's project at `dir`: each theorem Lean refutes, and
/// each block whose declarations it refutes, with every later block of its
/// part that names one of them, is set aside as pending, and the project
/// verified again, until it verifies. Then `bridge.toml` indexes each
/// theorem and pending κ, and each translated row of the module files
/// under `register` is stamped `bridged` or `pending`.
pub fn verify(dir: &Path, register: Option<&Path>) -> Result<Bridged, UorustError> {
    let state_file = dir.join(STATE);
    let mut state: State =
        serde_json::from_str(&crate::embed::read_text(&state_file)?).map_err(|e| {
            UorustError::Malformed {
                path: state_file.clone(),
                reason: e.to_string(),
            }
        })?;
    let run = |args: &[&str]| {
        std::process::Command::new("lexlean")
            .args(args)
            .current_dir(dir)
            .output()
            .map_err(|e| UorustError::Tool(format!("lexlean: {e}")))
    };
    // The screen: every part elaborated at once, each theorem's reduction
    // bounded, so that what Lean refutes is set aside in one pass, before
    // LexLean's own verification, which stops at a module's first failure.
    for pass in 0.. {
        write_parts(dir, &state)?;
        let refuted = screen(dir, &state)?;
        let n = apply(&mut state, &refuted);
        eprintln!("bridge: screen {pass}: {n} theorems set aside");
        std::fs::write(
            &state_file,
            serde_json::to_vec(&state).map_err(|e| UorustError::Tool(e.to_string()))?,
        )
        .map_err(|e| crate::error::io(&state_file, e))?;
        if n == 0 {
            break;
        }
    }
    // A round verifies the parts not yet confirmed. LexLean verifies its
    // modules in order and stops at the first that fails, every one before
    // it verified; that one is pruned. When every part is confirmed, the
    // whole project is verified once more, which is the bridge's check.
    let mut confirmed: BTreeSet<usize> = BTreeSet::new();
    for round in 0.. {
        write_parts(dir, &state)?;
        let lock = run(&["lock"])?;
        if !lock.status.success() {
            return Err(UorustError::Tool(format!(
                "lexlean lock: {}",
                String::from_utf8_lossy(&lock.stderr)
            )));
        }
        let open: Vec<usize> = state
            .parts
            .iter()
            .map(|p| p.id)
            .filter(|id| !confirmed.contains(id))
            .collect();
        let mut args = vec![
            "verify".to_string(),
            "--diagnostic-format".into(),
            "json".into(),
        ];
        if !open.is_empty() {
            args.push("src/Mir.lex.tex".into());
            args.push(format!("src/{}.lex.tex", crate::lift::PRELUDE));
            args.extend(
                open.iter()
                    .map(|id| format!("src/{}.lex.tex", part_module(*id))),
            );
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run(&args)?;
        if out.status.success() {
            if open.is_empty() {
                break;
            }
            confirmed.extend(open);
            continue;
        }
        let result: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| {
            UorustError::Tool(format!(
                "lexlean verify: {e}: {}",
                String::from_utf8_lossy(&out.stderr)
            ))
        })?;
        let named = failing(&state, &result);
        let before: BTreeMap<usize, Vec<String>> = state
            .parts
            .iter()
            .map(|p| (p.id, p.blocks.iter().map(|b| b.kappa.clone()).collect()))
            .collect();
        let refuted = prune(&mut state, &result);
        if refuted == 0 || round + 1 >= ROUNDS {
            return Err(UorustError::Tool(format!(
                "the bridge does not verify after {} rounds: {}",
                round + 1,
                result
            )));
        }
        // The parts before the one that failed verified; a part the
        // pruning changed is verified again.
        let linked = !result.to_string().contains(&format!("\"{LINK}\""));
        if let Some(first) = named.iter().min().filter(|_| linked) {
            confirmed.extend(open.iter().filter(|id| *id < first));
        }
        for p in &state.parts {
            let now: Vec<String> = p.blocks.iter().map(|b| b.kappa.clone()).collect();
            if before.get(&p.id) != Some(&now) {
                confirmed.remove(&p.id);
            }
        }
        eprintln!("bridge: round {round}: {refuted} theorems set aside");
    }
    let theorems: Vec<(&str, &str, usize)> = state
        .parts
        .iter()
        .flat_map(|p| {
            p.blocks
                .iter()
                .filter(|b| !b.statement.is_empty())
                .map(move |b| (b.kappa.as_str(), b.theorem.as_str(), p.id))
        })
        .collect();
    let mut index = String::from(
        "# The bridge (written by `uorust bridge`; do not edit): each translated\n\
         # body's theorem that the evaluator refines its shallow translation, as\n\
         # Lean verified it, and each pending with the shape that stopped it.\n",
    );
    for (kappa, theorem, k) in &theorems {
        let _ = write!(
            index,
            "\n[[bridged]]\nkappa = \"{kappa}\"\ntheorem = \"{theorem}\"\nmodule = \"{}\"\n",
            part_module(*k)
        );
    }
    for (kappa, reason) in &state.pending {
        let _ = write!(
            index,
            "\n[[pending]]\nkappa = \"{kappa}\"\nshape = {}\n",
            toml::Value::String(reason.clone())
        );
    }
    let file = dir.join("bridge.toml");
    std::fs::write(&file, index).map_err(|e| crate::error::io(&file, e))?;
    let proved: BTreeSet<&str> = theorems.iter().map(|(k, _, _)| *k).collect();
    if let Some(register) = register {
        for file in crate::embed::dir_paths(register)? {
            if file.extension().is_some_and(|x| x == "toml") {
                let text = stamp(&crate::embed::read_text(&file)?, &proved, &state.pending);
                std::fs::write(&file, text).map_err(|e| crate::error::io(&file, e))?;
            }
        }
    }
    Ok(Bridged {
        bridged: proved.len(),
        pending: state.pending.len(),
    })
}

/// Write the parts and the entrypoints that name them.
fn write_parts(dir: &Path, state: &State) -> Result<(), UorustError> {
    let src = dir.join("src");
    for file in crate::embed::dir_paths(&src)? {
        let stem = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if stem.starts_with("BridgePart") {
            std::fs::remove_file(&file).map_err(|e| crate::error::io(&file, e))?;
        }
    }
    let parts = render_parts(state);
    for (path, bytes) in &parts {
        let file = dir.join(path);
        std::fs::write(&file, bytes).map_err(|e| crate::error::io(&file, e))?;
    }
    let config = dir.join("lexlean.toml");
    let text = crate::embed::read_text(&config)?;
    let mut entries: Vec<String> = vec![
        format!("src/{}.lex.tex", "Mir"),
        format!("src/{}.lex.tex", crate::lift::PRELUDE),
    ];
    entries.extend(parts.keys().cloned());
    entries.sort();
    let line = format!(
        "entrypoints = [{}]",
        entries
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let text: String = text
        .lines()
        .map(|l| {
            if l.starts_with("entrypoints = ") {
                line.clone()
            } else if l.starts_with("child_timeout_ms = ") {
                format!("child_timeout_ms = {PART_TIMEOUT_MS}")
            } else {
                l.to_string()
            }
        })
        .map(|l| l + "\n")
        .collect();
    std::fs::write(&config, text).map_err(|e| crate::error::io(&config, e))
}

/// How many heartbeats the screen gives one theorem's elaboration: what
/// does not reduce within them is set aside as pending.
const SCREEN_HEARTBEATS: u64 = 200_000;

/// LexLean's runtime library, a LexLean project of the locked SDK, whose
/// modules a generated module imports.
const RUNTIME: &str =
    "/opt/prismpm/share/conformance-root/vendor/lexlean/language/runtime-1.2/library";

/// Copy the directory `from` to `to`, every file writable.
fn copy_tree(from: &Path, to: &Path) -> Result<(), UorustError> {
    std::fs::create_dir_all(to).map_err(|e| crate::error::io(to, e))?;
    for entry in crate::embed::dir_paths(from)? {
        let name = entry.file_name().unwrap_or_default();
        if name == ".lexlean" {
            continue;
        }
        let target = to.join(name);
        if entry.is_dir() {
            copy_tree(&entry, &target)?;
        } else {
            let bytes = std::fs::read(&entry).map_err(|e| crate::error::io(&entry, e))?;
            std::fs::write(&target, bytes).map_err(|e| crate::error::io(&target, e))?;
        }
    }
    Ok(())
}

/// How long the screen lets one part's Lean process run.
const SCREEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Elaborate every part with Lean directly, against the imports LexLean
/// verifies, each theorem bounded by [`SCREEN_HEARTBEATS`]: what each part
/// refutes, by its Lean's error lines; a link failure, by LexLean's.
fn screen(dir: &Path, state: &State) -> Result<Refuted, UorustError> {
    let dir = &std::path::absolute(dir).map_err(|e| crate::error::io(dir, e))?;
    let lexlean = |args: &[&str]| -> Result<serde_json::Value, UorustError> {
        let out = std::process::Command::new("lexlean")
            .args(args)
            .current_dir(dir)
            .output()
            .map_err(|e| UorustError::Tool(format!("lexlean: {e}")))?;
        serde_json::from_slice(&out.stdout).map_err(|e| {
            UorustError::Tool(format!(
                "lexlean {}: {e}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            ))
        })
    };
    let artifact = |r: &serde_json::Value| {
        r.get("artifacts")
            .and_then(serde_json::Value::as_array)
            .and_then(|a| a.first())
            .and_then(serde_json::Value::as_str)
            .map(|a| dir.join(a))
    };
    let tool = |what: &str, r: &serde_json::Value| UorustError::Tool(format!("{what}: {r}"));
    let locked = std::process::Command::new("lexlean")
        .arg("lock")
        .current_dir(dir)
        .output()
        .map_err(|e| UorustError::Tool(format!("lexlean: {e}")))?;
    if !locked.status.success() {
        return Err(UorustError::Tool(format!(
            "lexlean lock: {}",
            String::from_utf8_lossy(&locked.stderr)
        )));
    }
    let built = lexlean(&["build", "--diagnostic-format", "json"])?;
    if built.get("success").and_then(serde_json::Value::as_bool) == Some(false)
        || built.get("exit_code").and_then(serde_json::Value::as_i64) != Some(0)
    {
        let r = refuted_by(state, &built);
        if r.lost.is_empty()
            && r.unavailable.is_empty()
            && r.whole.is_empty()
            && r.theorems.is_empty()
        {
            return Err(tool("lexlean build", &built));
        }
        return Ok(r);
    }
    let modules = artifact(&built)
        .ok_or_else(|| tool("lexlean build", &built))?
        .join("modules");
    // The imports, compiled here: LexLean's runtime, as its own project
    // builds it, then the project's modules but the parts, in the order
    // their imports give.
    let work = dir.join(".lexlean").join("screen");
    crate::embed::replace_dir(&work, Some(&BTreeMap::new()))?;
    let runtime = work.join("runtime");
    copy_tree(Path::new(RUNTIME), &runtime)?;
    let run_in = |at: &Path, args: &[&str]| -> Result<serde_json::Value, UorustError> {
        let out = std::process::Command::new("lexlean")
            .args(args)
            .current_dir(at)
            .output()
            .map_err(|e| UorustError::Tool(format!("lexlean: {e}")))?;
        Ok(serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
    };
    run_in(&runtime, &["lock", "--diagnostic-format", "json"])?;
    let rt = run_in(&runtime, &["build", "--all", "--diagnostic-format", "json"])?;
    let rt_modules = rt
        .get("artifacts")
        .and_then(serde_json::Value::as_array)
        .and_then(|a| a.first())
        .and_then(serde_json::Value::as_str)
        .map(|a| runtime.join(a).join("modules"))
        .ok_or_else(|| tool("lexlean build (runtime)", &rt))?;
    let src = work.join("src");
    let oleans = work.join("oleans");
    let mut imports: BTreeMap<String, (std::path::PathBuf, Vec<String>)> = BTreeMap::new();
    for root in [&rt_modules, &modules] {
        for package in crate::embed::dir_paths(root)? {
            let Some(prefix) = package
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if !package.is_dir() {
                continue;
            }
            for file in crate::embed::dir_paths(&package)? {
                let Some(stem) = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".lean"))
                else {
                    continue;
                };
                if stem.starts_with("BridgePart") {
                    continue;
                }
                let text = crate::embed::read_text(&file)?;
                let to = src.join(&prefix).join(format!("{stem}.lean"));
                std::fs::create_dir_all(src.join(&prefix))
                    .map_err(|e| crate::error::io(&src, e))?;
                std::fs::write(&to, &text).map_err(|e| crate::error::io(&to, e))?;
                let deps = text
                    .lines()
                    .filter_map(|l| {
                        l.strip_prefix("public import ")
                            .or_else(|| l.strip_prefix("import "))
                    })
                    .map(|m| m.trim().to_string())
                    .collect();
                imports.insert(format!("{prefix}.{stem}"), (to, deps));
            }
        }
    }
    let mut done: BTreeSet<String> = BTreeSet::new();
    while done.len() < imports.len() {
        let ready: Vec<String> = imports
            .iter()
            .filter(|(m, (_, deps))| {
                !done.contains(*m)
                    && deps
                        .iter()
                        .all(|d| done.contains(d) || !imports.contains_key(d))
            })
            .map(|(m, _)| m.clone())
            .collect();
        if ready.is_empty() {
            return Err(UorustError::Tool(
                "the screen's imports form a cycle".into(),
            ));
        }
        for m in ready {
            let (file, _) = &imports[&m];
            let olean = oleans.join(format!("{}.olean", m.replace('.', "/")));
            if let Some(parent) = olean.parent() {
                std::fs::create_dir_all(parent).map_err(|e| crate::error::io(parent, e))?;
            }
            let out = std::process::Command::new("lean")
                .arg("-R")
                .arg(&src)
                .arg("-o")
                .arg(&olean)
                .arg(file)
                .current_dir(dir)
                .env("LEAN_PATH", &oleans)
                .output()
                .map_err(|e| UorustError::Tool(format!("lean: {e}")))?;
            if !out.status.success() {
                return Err(UorustError::Tool(format!(
                    "lean {m}: {}",
                    String::from_utf8_lossy(&out.stdout)
                )));
            }
            done.insert(m);
        }
    }
    let mut jobs = Vec::new();
    for (k, p) in state.parts.iter().enumerate() {
        let module = part_module(p.id);
        let lean = modules.join("UorustEmbed").join(format!("{module}.lean"));
        let text = crate::embed::read_text(&lean)?;
        let bounded: String = text
            .lines()
            .map(|l| {
                if l.starts_with("set_option maxHeartbeats ") {
                    format!("set_option maxHeartbeats {SCREEN_HEARTBEATS}\n")
                } else {
                    format!("{l}\n")
                }
            })
            .collect();
        let file = work.join(format!("{module}.lean"));
        std::fs::write(&file, &bounded).map_err(|e| crate::error::io(&file, e))?;
        jobs.push((k, file, bounded));
    }
    let width = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let queue = std::sync::Mutex::new(jobs.into_iter());
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..width {
            scope.spawn(|| loop {
                let job = queue.lock().ok().and_then(|mut q| q.next());
                let Some((k, file, text)) = job else { break };
                let outcome = elaborate(dir, &oleans, &file);
                if let Ok(mut r) = results.lock() {
                    r.push((k, text, outcome));
                }
            });
        }
    });
    let mut refuted = Refuted {
        screened: true,
        ..Refuted::default()
    };
    for (k, text, outcome) in results.into_inner().unwrap_or_default() {
        let Some(errors) = outcome? else {
            refuted.whole.insert(k);
            continue;
        };
        // Each error at the declaration its line falls in.
        let starts: Vec<(usize, bool, String)> = text
            .lines()
            .enumerate()
            .filter_map(|(i, l)| {
                let mut words = l.split_whitespace().skip_while(|w| {
                    w.starts_with('@') || *w == "public" || *w == "private" || *w == "noncomputable"
                });
                let kind = words.next()?;
                let name = words.next()?;
                matches!(
                    kind,
                    "def" | "theorem" | "structure" | "inductive" | "abbrev"
                )
                .then(|| (i + 1, kind == "theorem", name.to_string()))
            })
            .collect();
        let blocks = &state.parts[k].blocks;
        for line in errors {
            let Some((_, theorem, name)) = starts.iter().rev().find(|(l, _, _)| *l <= line) else {
                refuted.whole.insert(k);
                continue;
            };
            match blocks.iter().position(|b| *theorem && &b.theorem == name) {
                Some(i) => {
                    refuted.theorems.insert((k, i));
                }
                None if *theorem => {
                    refuted.whole.insert(k);
                }
                None => {
                    refuted.lost.insert(name.clone());
                }
            }
        }
    }
    Ok(refuted)
}

/// Run Lean on `file` against `oleans`: the lines of its errors, or `None`
/// where it outran [`SCREEN_TIMEOUT`] or stopped without one.
fn elaborate(dir: &Path, oleans: &Path, file: &Path) -> Result<Option<Vec<usize>>, UorustError> {
    let mut child = std::process::Command::new("lean")
        .arg(file)
        .current_dir(dir)
        .env("LEAN_PATH", oleans)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| UorustError::Tool(format!("lean: {e}")))?;
    let out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut o) = out {
            let _ = std::io::Read::read_to_string(&mut o, &mut text);
        }
        text
    });
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() > SCREEN_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
            Err(e) => return Err(UorustError::Tool(format!("lean: {e}"))),
        }
    };
    let text = reader.join().unwrap_or_default();
    let Some(status) = status else {
        return Ok(None);
    };
    let name = file.to_string_lossy();
    let lines: Vec<usize> = text
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix(name.as_ref())?.strip_prefix(':')?;
            let (line, rest) = rest.split_once(':')?;
            let (_, rest) = rest.split_once(':')?;
            rest.trim_start()
                .starts_with("error")
                .then(|| line.parse().ok())
                .flatten()
        })
        .collect();
    Ok((status.success() || !lines.is_empty()).then_some(lines))
}

/// The parts a failed verification names.
fn failing(state: &State, result: &serde_json::Value) -> Vec<usize> {
    let text = result.to_string();
    state
        .parts
        .iter()
        .map(|p| p.id)
        .filter(|id| text.contains(&part_module(*id)))
        .collect()
}

/// Set aside what a failed verification refutes; returns how many blocks.
///
/// A refuted theorem is its block's; a refuted declaration refutes every
/// block that needs it, in every part; what the link phase names as
/// lacking refutes every block that names it; a part Lean stops without a
/// line (its child's timeout) is split into a part per block, and a block
/// alone in such a part is pending at `timeout`.
fn prune(state: &mut State, result: &serde_json::Value) -> usize {
    let refuted = refuted_by(state, result);
    apply(state, &refuted)
}

/// What a verification refutes: declarations by name, theorems by their
/// part's position and their own, the parts it stopped as a whole, and the
/// names the link phase lacks.
#[derive(Default)]
struct Refuted {
    /// Whether the screen refuted it, where a part stopped as a whole is
    /// pending, not split: its Lean outran the screen's bound.
    screened: bool,
    lost: BTreeSet<String>,
    theorems: BTreeSet<(usize, usize)>,
    whole: BTreeSet<usize>,
    unavailable: BTreeSet<String>,
}

/// What LexLean's diagnostics refute.
fn refuted_by(state: &State, result: &serde_json::Value) -> Refuted {
    let diagnostics = result
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rendered: Vec<Spans> = state.parts.iter().map(|p| render_part(p).1).collect();
    // The declarations refuted, by name; each part's theorems refuted; the
    // parts stopped as a whole.
    let mut lost: BTreeSet<String> = BTreeSet::new();
    let mut refuted: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut whole: BTreeSet<usize> = BTreeSet::new();
    let mut unavailable: BTreeSet<String> = BTreeSet::new();
    for d in &diagnostics {
        if d.get("severity").and_then(serde_json::Value::as_str) != Some("error") {
            continue;
        }
        let primary = d.get("primary");
        let path = primary
            .and_then(|p| p.get("path"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let message = d
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let part = (0..state.parts.len()).find(|k| {
            let module = part_module(state.parts[*k].id);
            path == format!("src/{module}.lex.tex") || message.contains(&format!(".{module}"))
        });
        let Some(k) = part else { continue };
        // The link phase names what it lacks, over the whole module.
        if d.get("code").and_then(serde_json::Value::as_str) == Some(LINK) {
            let named: BTreeSet<String> = message
                .split('`')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect();
            let hit = state
                .parts
                .iter()
                .any(|p| p.blocks.iter().any(|b| b.names(&named)));
            if hit {
                unavailable.extend(named);
            } else {
                whole.insert(k);
            }
            continue;
        }
        let line = primary
            .and_then(|p| p.get("line_start"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|l| usize::try_from(l).ok())
            .filter(|_| path.ends_with(".lex.tex"));
        let Some(l) = line else {
            whole.insert(k);
            continue;
        };
        let spans = &rendered[k];
        if let Some((n, _, _)) = spans.decls.iter().find(|(_, a, b)| (*a..*b).contains(&l)) {
            lost.insert(n.clone());
        } else if let Some(i) = spans
            .theorems
            .iter()
            .position(|(a, b)| (*a..*b).contains(&l))
        {
            refuted.insert((k, i));
        } else {
            whole.insert(k);
        }
    }
    Refuted {
        screened: false,
        lost,
        theorems: refuted,
        whole,
        unavailable,
    }
}

/// Set aside what `r` refutes; returns how many blocks.
fn apply(state: &mut State, r: &Refuted) -> usize {
    let Refuted {
        lost,
        theorems: refuted,
        whole,
        unavailable,
        ..
    } = r;
    let mut count = 0;
    let mut split = Vec::new();
    let mut next = state.parts.iter().map(|p| p.id + 1).max().unwrap_or(0);
    for (k, part) in state.parts.iter_mut().enumerate() {
        let blocks = std::mem::take(&mut part.blocks);
        if whole.contains(&k) && blocks.len() > 1 && !r.screened {
            count += blocks.len();
            for b in blocks {
                split.push(Part {
                    id: next,
                    blocks: vec![b],
                });
                next += 1;
            }
            continue;
        }
        for (i, b) in blocks.into_iter().enumerate() {
            let why = if whole.contains(&k) {
                Some(format!("timeout: {}", b.shape))
            } else if b.decls.iter().any(|(n, _)| lost.contains(n)) || b.names(unavailable) {
                Some("definition".to_string())
            } else if refuted.contains(&(k, i)) {
                Some(format!("proof: {}", b.shape))
            } else {
                None
            };
            match why {
                None => part.blocks.push(b),
                Some(why) => {
                    count += 1;
                    state.pending.insert(b.kappa, why);
                }
            }
        }
    }
    state.parts.extend(split);
    state.parts.retain(|p| !p.blocks.is_empty());
    count
}

impl Block {
    /// Whether its declarations or theorem name one of `names`.
    fn names(&self, names: &BTreeSet<String>) -> bool {
        !names.is_empty()
            && std::iter::once(self.statement.as_str())
                .chain(self.decls.iter().map(|(_, t)| t.as_str()))
                .any(|t| {
                    t.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .any(|w| names.contains(w))
                })
    }
}

/// A register's module file with each translated row stamped by its κ:
/// `bridged = "proved"` where its theorem verifies, else
/// `bridged = "pending"` with the shape that stopped it.
fn stamp(text: &str, proved: &BTreeSet<&str>, pending: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut kappa = String::new();
    for line in text.lines() {
        if line.starts_with("bridged = ") || line.starts_with("pending = ") {
            continue;
        }
        if line == "[[fn]]" {
            kappa.clear();
        }
        if let Some(k) = line
            .strip_prefix("kappa = \"")
            .and_then(|k| k.strip_suffix('"'))
        {
            kappa = k.to_string();
        }
        out.push_str(line);
        out.push('\n');
        if line == "status = \"translated\"" {
            if proved.contains(kappa.as_str()) {
                out.push_str("bridged = \"proved\"\n");
            } else {
                let why = pending.get(&kappa).map_or("unembedded", String::as_str);
                let _ = writeln!(
                    out,
                    "bridged = \"pending\"\npending = {}",
                    toml::Value::String(why.to_string())
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MIR's printed types read as the shapes the shallow types name: a
    /// reference is its referent, `Option<&u8>` an enum over it (UE-19).
    #[test]
    fn a_signature_reads_as_the_shallow_types_ue_19() {
        let at: BTreeMap<String, String> = [("T".to_string(), "u32".to_string())].into();
        let parse = |p: &str| Shape::parse(&substitute(p, &at), 64);
        assert_eq!(
            parse("&i16"),
            Some(Shape::Ref(Box::new(Shape::Int(16, true))))
        );
        assert_eq!(parse("usize"), Some(Shape::Int(64, false)));
        assert_eq!(
            parse("option::Option<T>").map(|s| s.shallow()),
            Some("(Option Nat)".to_string())
        );
        assert_eq!(
            parse("result::Result<(), T>").map(|s| s.shallow()),
            Some("(Result Unit Nat)".to_string())
        );
        assert_eq!(parse("&mut u8"), None);
        assert_eq!(parse("Wrapping<u8>"), None);
    }

    /// Each translated row is stamped by its κ, proved or pending with its
    /// shape, in place of what it recorded; residue is not (UE-19).
    #[test]
    fn the_register_records_each_translated_row_bridged_or_pending_ue_19() {
        let text = "[[fn]]\nkappa = \"a\"\nstatus = \"translated\"\nbridged = \"pending\"\npending = \"proof\"\ndigest = \"d\"\n\n\
                    [[fn]]\nkappa = \"b\"\nstatus = \"translated\"\n\n[[fn]]\nkappa = \"c\"\nstatus = \"residue\"\n";
        let proved: BTreeSet<&str> = ["a"].into();
        let pending: BTreeMap<String, String> =
            [("b".to_string(), "signature: Nat".to_string())].into();
        let out = stamp(text, &proved, &pending);
        assert!(
            out.contains("kappa = \"a\"\nstatus = \"translated\"\nbridged = \"proved\"\ndigest")
        );
        assert!(out.contains("kappa = \"b\"\nstatus = \"translated\"\nbridged = \"pending\"\npending = \"signature: Nat\"\n"));
        assert_eq!(out.matches("bridged").count(), 2);
    }

    /// A Boolean input is split into its two constructors before the
    /// evaluator's reduction closes each case (UE-19).
    #[test]
    fn each_boolean_input_is_a_case_ue_19() {
        assert_eq!(by_cases(&[], "reflexivity{}"), "reflexivity{}");
        let one = by_cases(&["b"], "reflexivity{}");
        assert!(one.starts_with(
            "cases{branches: [{proof: reflexivity{}, binders: [], constructor: \"false\"}"
        ));
        assert!(one.ends_with("scrutinee: b}"));
    }
}
