//! The LexLean authoring notation, printed in the canonical layout `lexlean fmt` keeps.
//!
//! The printer only knows the declaration shapes the projection emits. A shape
//! is printed flat when the flat line fits the formatter's width and broken one
//! member per line otherwise; a `match` always breaks before each arm. The
//! layout is checked against `lexlean fmt --check` by the projection tests, so
//! a drift in the formatter shows up as a red test, never as a silent rewrite.

/// The formatter's line width.
const WIDTH: usize = 100;

/// An expression: a flat term, or a form whose `match` may break its arms
/// onto lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expr {
    /// A term printed on one line.
    Flat(String),
    /// `(match scrutinee | pattern => expr ...)`.
    Match { scrutinee: String, arms: Vec<Arm> },
    /// `(if condition then a else b)` where a branch may hold a match.
    If {
        condition: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
    /// `(let name : type := value; body)`.
    Let {
        name: String,
        ty: String,
        value: Box<Expr>,
        body: Box<Expr>,
    },
}

/// One arm of a `match`: its pattern and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Arm {
    pub(crate) pattern: String,
    pub(crate) body: Expr,
}

impl Expr {
    /// The structured form of a term held as text: a top-level `(match
    /// scrutinee | pattern => body ...)` as a [`Expr::Match`] (its arms'
    /// bodies read the same way), so the printer lays it out as `lexlean
    /// fmt` does; any other term flat.
    pub(crate) fn read(text: &str) -> Self {
        let flat = || Self::Flat(text.to_string());
        let Some(inner) = text
            .strip_prefix("(match ")
            .and_then(|t| t.strip_suffix(')'))
        else {
            return flat();
        };
        // The pieces of `inner` between its ` | ` at depth 0, outside strings;
        // `None` when its parentheses do not close inside it.
        let mut pieces = Vec::new();
        let (mut depth, mut start, mut quoted, mut escaped) = (0i32, 0, false, false);
        let bytes = inner.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if quoted {
                match b {
                    _ if escaped => escaped = false,
                    b'\\' => escaped = true,
                    b'"' => quoted = false,
                    _ => {}
                }
                continue;
            }
            match b {
                b'"' => quoted = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth < 0 {
                        return flat();
                    }
                }
                b'|' if depth == 0
                    && inner[..i].ends_with(' ')
                    && inner[i + 1..].starts_with(' ') =>
                {
                    pieces.push(&inner[start..i - 1]);
                    start = i + 2;
                }
                _ => {}
            }
        }
        if depth != 0 || quoted || pieces.is_empty() {
            return flat();
        }
        pieces.push(&inner[start..]);
        let scrutinee = pieces[0].to_string();
        let mut arms = Vec::new();
        for piece in &pieces[1..] {
            let Some((pattern, body)) = piece.split_once(" => ") else {
                return flat();
            };
            arms.push(Arm {
                pattern: pattern.to_string(),
                body: Self::read(body),
            });
        }
        Self::Match { scrutinee, arms }
    }
}

/// A piece of a compound expression: fixed text or a sub-expression.
enum Piece<'a> {
    Text(String),
    Sub(&'a Expr),
}

impl Expr {
    /// No `match` or `let` anywhere: a definition holding it may print flat.
    fn is_flat(&self) -> bool {
        match self {
            Self::Flat(_) => true,
            Self::Match { .. } | Self::Let { .. } => false,
            Self::If {
                condition,
                then,
                otherwise,
            } => condition.is_flat() && then.is_flat() && otherwise.is_flat(),
        }
    }

    /// The whole expression on one line.
    pub(crate) fn flat(&self) -> String {
        match self {
            Self::Flat(text) => text.clone(),
            Self::Match { scrutinee, arms } => {
                let mut out = format!("(match {scrutinee}");
                for arm in arms {
                    out.push_str(&format!(" | {} => {}", arm.pattern, arm.body.flat()));
                }
                out.push(')');
                out
            }
            _ => self
                .pieces()
                .iter()
                .map(|p| match p {
                    Piece::Text(t) => t.clone(),
                    Piece::Sub(e) => e.flat(),
                })
                .collect(),
        }
    }

    fn pieces(&self) -> Vec<Piece<'_>> {
        match self {
            Self::If {
                condition,
                then,
                otherwise,
            } => vec![
                Piece::Text("(if ".into()),
                Piece::Sub(condition),
                Piece::Text(" then ".into()),
                Piece::Sub(then),
                Piece::Text(" else ".into()),
                Piece::Sub(otherwise),
                Piece::Text(")".into()),
            ],
            Self::Let {
                name,
                ty,
                value,
                body,
            } => vec![
                Piece::Text(format!("(let {name} : {ty} := ")),
                Piece::Sub(value),
                Piece::Text("; ".into()),
                Piece::Sub(body),
                Piece::Text(")".into()),
            ],
            _ => Vec::new(),
        }
    }

    /// The text of the expression at nesting `depth`, starting at column
    /// `col` with `trail` characters after it on its last line. A `match`
    /// stays on one line when that line fits the width; otherwise each arm
    /// starts a line indented by `4 + 2 * depth`.
    pub(crate) fn render_at(&self, depth: usize, col: usize, trail: usize) -> String {
        let flat = self.flat();
        if col + flat.len() + trail <= WIDTH {
            return flat;
        }
        match self {
            Self::Flat(text) => text.clone(),
            Self::Match { scrutinee, arms } => {
                let indent = 4 + 2 * depth;
                let mut out = format!("(match {scrutinee}");
                for (i, arm) in arms.iter().enumerate() {
                    let head = format!("{}| {} => ", " ".repeat(indent), arm.pattern);
                    let last = if i + 1 == arms.len() { trail + 1 } else { 0 };
                    out.push('\n');
                    out.push_str(&head);
                    out.push_str(&arm.body.render_at(depth + 1, head.len(), last));
                }
                out.push(')');
                out
            }
            _ => {
                let pieces = self.pieces();
                let mut out = String::new();
                for (i, piece) in pieces.iter().enumerate() {
                    match piece {
                        Piece::Text(t) => out.push_str(t),
                        Piece::Sub(e) => {
                            let after: usize = pieces[i + 1..]
                                .iter()
                                .map(|p| match p {
                                    Piece::Text(t) => t.len(),
                                    Piece::Sub(e) => e.flat().len(),
                                })
                                .sum();
                            let at = out.rsplit('\n').next().map_or(0, str::len);
                            let column = if out.contains('\n') { at } else { col + at };
                            out.push_str(&e.render_at(depth, column, after + trail));
                        }
                    }
                }
                out
            }
        }
    }
}

/// One top-level declaration.
#[derive(Clone, Debug)]
pub(crate) enum Decl {
    Structure {
        name: String,
        fields: Vec<(String, String)>,
    },
    Inductive {
        name: String,
        constructors: Vec<String>,
    },
    /// An inductive whose constructors carry fields: `(constructor, field
    /// types)`.
    Data {
        name: String,
        constructors: Vec<(String, Vec<String>)>,
        /// The label of the mutual group the inductive is declared in.
        mutual: Option<String>,
    },
    Definition {
        name: String,
        parameters: Vec<(String, String)>,
        result: String,
        body: Expr,
        /// The Lean axioms the definition's elaboration depends on.
        axioms: Vec<String>,
        /// The parameter a structurally recursive definition descends on.
        recursive: Option<String>,
        /// A well-founded definition's measure and, for each recursive call
        /// in pre-order, the theorem stating that the call decreases it.
        termination: Option<(String, Vec<String>)>,
        /// The label of the mutual group the definition is declared in.
        mutual: Option<String>,
    },
    Theorem {
        name: String,
        parameters: Vec<(String, String)>,
        statement: String,
        proof: String,
        axioms: Vec<String>,
    },
}

impl Decl {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Structure { name, .. }
            | Self::Inductive { name, .. }
            | Self::Data { name, .. }
            | Self::Definition { name, .. }
            | Self::Theorem { name, .. } => name,
        }
    }

    /// A record definition with no parameters.
    pub(crate) fn value(name: impl Into<String>, result: impl Into<String>, body: String) -> Self {
        Self::Definition {
            name: name.into(),
            parameters: Vec::new(),
            result: result.into(),
            body: Expr::Flat(body),
            axioms: Vec::new(),
            recursive: None,
            termination: None,
            mutual: None,
        }
    }
}

/// `[a, b]` flat, or one item per line under `indent`.
fn list(key: &str, items: &[String], indent: usize, comma: bool) -> String {
    let flat = format!("{}{key}: [{}]", " ".repeat(indent), items.join(", "));
    if flat.len() + usize::from(comma) <= WIDTH || items.is_empty() {
        return flat;
    }
    let inner = " ".repeat(indent + 2);
    let mut out = format!("{}{key}: [\n", " ".repeat(indent));
    out.push_str(
        &items
            .iter()
            .map(|item| format!("{inner}{item}"))
            .collect::<Vec<_>>()
            .join(",\n"),
    );
    out.push('\n');
    out.push_str(&" ".repeat(indent));
    out.push(']');
    out
}

/// The members of a declaration, flat when the whole line fits.
fn members(
    kind: &str,
    flat_members: &[String],
    broken_members: &[String],
    flat_ok: bool,
) -> String {
    let flat = format!("{kind}{{{}}}", flat_members.join(", "));
    if flat_ok && flat.len() <= WIDTH {
        return flat;
    }
    format!("{kind}{{\n{}\n}}", broken_members.join(",\n"))
}

fn params(parameters: &[(String, String)]) -> Vec<String> {
    parameters
        .iter()
        .map(|(name, ty)| format!("{name} : {ty}"))
        .collect()
}

pub(crate) fn render_decl(decl: &Decl) -> String {
    match decl {
        Decl::Structure { name, fields } => {
            let items: Vec<String> = fields
                .iter()
                .map(|(field, ty)| format!("{{name: {field}, type: {ty}}}"))
                .collect();
            let head = [
                format!("name: {name}"),
                "type_parameters: []".to_string(),
                "parameters: []".to_string(),
            ];
            let mut flat = head.to_vec();
            flat.push(format!("fields: [{}]", items.join(", ")));
            let mut broken: Vec<String> = head.iter().map(|m| format!("  {m}")).collect();
            broken.push(list("fields", &items, 2, false));
            members("structure", &flat, &broken, true)
        }
        Decl::Inductive { name, constructors } => {
            let items: Vec<String> = constructors
                .iter()
                .map(|ctor| format!("{{name: {ctor}, fields: []}}"))
                .collect();
            let head = [
                format!("name: {name}"),
                "type_parameters: []".to_string(),
                "parameters: []".to_string(),
            ];
            let mut flat = head.to_vec();
            flat.push(format!("constructors: [{}]", items.join(", ")));
            let mut broken: Vec<String> = head.iter().map(|m| format!("  {m}")).collect();
            broken.push(list("constructors", &items, 2, false));
            members("inductive", &flat, &broken, true)
        }
        Decl::Data {
            name,
            constructors,
            mutual,
        } => {
            let items: Vec<String> = constructors
                .iter()
                .map(|(ctor, fields)| format!("{{name: {ctor}, fields: [{}]}}", fields.join(", ")))
                .collect();
            // A constructor too long for its line is broken one member per
            // line, its fields one per line.
            let lines: Vec<String> = constructors
                .iter()
                .zip(&items)
                .enumerate()
                .map(|(i, ((ctor, fields), item))| {
                    let comma = i + 1 < items.len();
                    if 4 + item.len() + usize::from(comma) <= WIDTH {
                        item.clone()
                    } else {
                        format!(
                            "{{\n      name: {ctor},\n{}\n    }}",
                            list("fields", fields, 6, false)
                        )
                    }
                })
                .collect();
            let head = [
                format!("name: {name}"),
                "type_parameters: []".to_string(),
                "parameters: []".to_string(),
            ];
            let mut flat = head.to_vec();
            flat.push(format!("constructors: [{}]", items.join(", ")));
            let mut broken: Vec<String> = head.iter().map(|m| format!("  {m}")).collect();
            broken.push(list("constructors", &lines, 2, mutual.is_some()));
            if let Some(label) = mutual {
                flat.push(format!("mutual: {label}"));
                broken.push(format!("  mutual: {label}"));
            }
            members("inductive", &flat, &broken, true)
        }
        Decl::Definition {
            name,
            parameters,
            result,
            body,
            axioms,
            recursive,
            termination,
            mutual,
        } => {
            let ps = params(parameters);
            let mut flat = vec![
                format!("name: {name}"),
                format!("parameters: [{}]", ps.join(", ")),
                format!("result: {result}"),
                format!("body: {}", body.flat()),
            ];
            let more = recursive.is_some() || termination.is_some() || mutual.is_some();
            let trail = usize::from(more || !axioms.is_empty());
            let mut broken = vec![
                format!("  name: {name}"),
                list("parameters", &ps, 2, true),
                format!("  result: {result}"),
                format!("  body: {}", body.render_at(0, 8, trail)),
            ];
            if !axioms.is_empty() {
                flat.push(format!("axioms: [{}]", axioms.join(", ")));
                broken.push(list("axioms", axioms, 2, more));
            }
            if let Some(label) = mutual {
                flat.push(format!("mutual: {label}"));
                broken.push(format!("  mutual: {label}"));
            }
            if let Some(arg) = recursive {
                flat.push(format!("recursive_argument: {arg}"));
                broken.push(format!("  recursive_argument: {arg}"));
            }
            if let Some((measure, evidence)) = termination {
                let clause = format!(
                    "termination: {{evidence: [{}], measure: {measure}}}",
                    evidence.join(", ")
                );
                flat.push(clause.clone());
                if 2 + clause.len() <= WIDTH {
                    broken.push(format!("  {clause}"));
                } else {
                    broken.push(format!(
                        "  termination: {{\n{},\n    measure: {measure}\n  }}",
                        list("evidence", evidence, 4, true)
                    ));
                }
            }
            members("definition", &flat, &broken, body.is_flat() && !more)
        }
        Decl::Theorem {
            name,
            parameters,
            statement,
            proof,
            axioms,
        } => {
            let ps = params(parameters);
            let mut flat = vec![
                format!("name: {name}"),
                format!("parameters: [{}]", ps.join(", ")),
                format!("statement: {statement}"),
                format!("proof: {proof}"),
            ];
            let mut broken = vec![
                format!("  name: {name}"),
                list("parameters", &ps, 2, true),
                format!("  statement: {statement}"),
                format!("  proof: {proof}"),
            ];
            if !axioms.is_empty() {
                flat.push(format!("axioms: [{}]", axioms.join(", ")));
                broken.push(list("axioms", axioms, 2, false));
            }
            members("theorem", &flat, &broken, true)
        }
    }
}

/// A whole module.
#[derive(Clone, Debug)]
pub(crate) struct Module {
    pub(crate) name: String,
    pub(crate) glossaries: Vec<&'static str>,
    pub(crate) imports: Vec<String>,
    pub(crate) title: &'static str,
    pub(crate) decls: Vec<Decl>,
}

impl Module {
    pub(crate) fn render(&self) -> String {
        let mut out = format!("\\begin{{lexlean}}{{{}}}\n", self.name);
        for glossary in &self.glossaries {
            out.push_str(&format!("\\useglossary{{{glossary}}}\n"));
        }
        for import in &self.imports {
            out.push_str(&format!("\\importmodule{{{import}}}\n"));
        }
        out.push_str(&format!("\\title{{{}}}\n", self.title));
        out.push_str("\\begin{semanticmodule}\n");
        let decls: Vec<String> = self.decls.iter().map(render_decl).collect();
        out.push_str(&decls.join("\n\n"));
        if !decls.is_empty() {
            out.push('\n');
        }
        out.push_str("\\end{semanticmodule}\n\\end{lexlean}\n");
        out
    }

    /// The path of the module's source under `src/`.
    pub(crate) fn path(&self) -> String {
        format!("src/{}.lex.tex", self.name.replace('.', "/"))
    }
}

/// A LexLean string literal. Control characters become spaces and runs of
/// whitespace collapse, because a record name or text is one bounded line.
pub(crate) fn string(text: &str) -> String {
    format!("\"{}\"", escape(text))
}

pub(crate) fn escape(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(collapsed.len());
    for c in collapsed.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// `(cons "a" (cons "b" (nil String)))`.
pub(crate) fn string_list(items: &[String]) -> String {
    let mut out = "(nil String)".to_string();
    for item in items.iter().rev() {
        out = format!("(cons {} {out})", string(item));
    }
    out
}

/// `lowerCamel` from any Rust or Cargo name: `thermo-core::should_heat` ->
/// `thermoCoreShouldHeat`.
pub(crate) fn camel(parts: &[&str]) -> String {
    let pascal = pascal(parts);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// `PascalCase` from any Rust or Cargo name.
pub(crate) fn pascal(parts: &[&str]) -> String {
    let mut out = String::new();
    for part in parts {
        let mut upper = true;
        let mut prev_lower = false;
        for c in part.chars() {
            if !c.is_ascii_alphanumeric() {
                upper = true;
                prev_lower = false;
                continue;
            }
            if upper {
                out.push(c.to_ascii_uppercase());
                upper = false;
            } else if c.is_ascii_uppercase() && !prev_lower {
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'N');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_camel_cased_from_rust_and_cargo_names() {
        assert_eq!(
            camel(&["thermo-core", "should_heat"]),
            "thermoCoreShouldHeat"
        );
        assert_eq!(pascal(&["UorustError"]), "UorustError");
        assert_eq!(pascal(&["HTTPServer"]), "Httpserver");
        assert_eq!(pascal(&["2d"]), "N2d");
    }

    #[test]
    fn strings_are_escaped_and_collapsed() {
        assert_eq!(string("a \"b\"\n\tc\\"), "\"a \\\"b\\\" c\\\\\"");
    }

    #[test]
    fn a_match_held_as_text_is_read_into_its_arms() {
        let text = "(match (prim index xs i : (Option T)) | Option.none => (record T {a := \"|\"}) | Option.some x => (match x | T.mk y => y))";
        let Expr::Match { scrutinee, arms } = Expr::read(text) else {
            panic!("not read as a match");
        };
        assert_eq!(scrutinee, "(prim index xs i : (Option T))");
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].pattern, "Option.none");
        assert_eq!(arms[0].body, Expr::Flat("(record T {a := \"|\"})".into()));
        assert!(matches!(arms[1].body, Expr::Match { .. }));
        assert_eq!(Expr::read(text).flat(), text);
        assert_eq!(Expr::read("(f x)"), Expr::Flat("(f x)".into()));
    }

    #[test]
    fn a_short_definition_is_flat_and_a_long_one_breaks() {
        let short = Decl::value("callA", "String", string("a"));
        assert_eq!(
            render_decl(&short),
            "definition{name: callA, parameters: [], result: String, body: \"a\"}"
        );
        let long = Decl::value("callA", "String", string(&"x".repeat(80)));
        assert!(render_decl(&long).starts_with("definition{\n  name: callA,\n"));
    }
}
