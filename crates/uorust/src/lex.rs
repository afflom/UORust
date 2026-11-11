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
    Match {
        scrutinee: String,
        arms: Vec<(String, Expr)>,
    },
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
                for (pattern, arm) in arms {
                    out.push_str(&format!(" | {pattern} => {}", arm.flat()));
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
                for (i, (pattern, arm)) in arms.iter().enumerate() {
                    let head = format!("{}| {pattern} => ", " ".repeat(indent));
                    let last = if i + 1 == arms.len() { trail + 1 } else { 0 };
                    out.push('\n');
                    out.push_str(&head);
                    out.push_str(&arm.render_at(depth + 1, head.len(), last));
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
    Definition {
        name: String,
        parameters: Vec<(String, String)>,
        result: String,
        body: Expr,
        /// The Lean axioms the definition's elaboration depends on.
        axioms: Vec<String>,
        /// The parameter a structurally recursive definition descends on.
        recursive: Option<String>,
    },
    Theorem {
        name: String,
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
        Decl::Definition {
            name,
            parameters,
            result,
            body,
            axioms,
            recursive,
        } => {
            let ps = params(parameters);
            let mut flat = vec![
                format!("name: {name}"),
                format!("parameters: [{}]", ps.join(", ")),
                format!("result: {result}"),
                format!("body: {}", body.flat()),
            ];
            let trail = usize::from(recursive.is_some() || !axioms.is_empty());
            let mut broken = vec![
                format!("  name: {name}"),
                list("parameters", &ps, 2, true),
                format!("  result: {result}"),
                format!("  body: {}", body.render_at(0, 8, trail)),
            ];
            if !axioms.is_empty() {
                flat.push(format!("axioms: [{}]", axioms.join(", ")));
                broken.push(list("axioms", axioms, 2, recursive.is_some()));
            }
            if let Some(arg) = recursive {
                flat.push(format!("recursive_argument: {arg}"));
                broken.push(format!("  recursive_argument: {arg}"));
            }
            members(
                "definition",
                &flat,
                &broken,
                body.is_flat() && recursive.is_none(),
            )
        }
        Decl::Theorem {
            name,
            statement,
            proof,
            axioms,
        } => {
            let mut flat = vec![
                format!("name: {name}"),
                "parameters: []".to_string(),
                format!("statement: {statement}"),
                format!("proof: {proof}"),
            ];
            let mut broken = vec![
                format!("  name: {name}"),
                "  parameters: []".to_string(),
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
