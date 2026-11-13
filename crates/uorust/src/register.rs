//! The lift register: every function of product code, keyed by the κ of its
//! question, with what the lift decided for it.
//!
//! A function's κ is the SHA-256 of [`DOMAIN`], its crate path and its
//! normalized token stream (signature and body as `syn` reads them, so
//! comments, doc comments and layout do not move it). The κ names the
//! question "does this function lift?", not the answer: a function whose
//! source changes is a new question with a new κ.
//!
//! A lifted κ is a proved claim. The ratchet is monotone, not a count: a
//! candidate register is admitted against the incumbent (the committed one)
//! exactly when every κ the incumbent records as lifted is still lifted, or
//! is absent because its source changed or it was removed ([`admit`]).
//! Residue, foreign and effect rows classify what is not lifted yet; none of
//! them is a budget. Their number is the distance to the requirement that
//! every function of product code lifts, an open claim the honesty report
//! states and no gate asserts.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::UorustError;

/// The domain tag every function κ starts with.
pub const DOMAIN: &str = "uorust/function/1";

/// The schema of a register file written apart from a projection.
pub const SCHEMA: &str = "uorust/register/1";

/// The κ of a function: SHA-256 over [`DOMAIN`], its crate path and its
/// normalized token stream, one line each, as lowercase hex.
pub fn kappa(path: &str, tokens: &str) -> String {
    let digest = Sha256::digest(kappa_preimage(path, tokens));
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The bytes [`kappa`] hashes: [`DOMAIN`], the path and the tokens, each
/// ended by a newline. The model decides the framing (`Kappa::kappaPreimage`
/// in app/src/Kappa.lex.tex, whose path is bytes that are never a newline):
/// the path and the tokens are each read back from the preimage
/// (`Kappa::thePathIsReadFromThePreimage`,
/// `Kappa::theTokensAreReadFromThePreimage`), so two functions with one
/// preimage are one function, and this adapter only hashes. A crate path is
/// `::`-joined identifiers and never holds a newline; one that did is the
/// one input the framing cannot tell apart
/// (`Kappa::aNewlineInThePathBreaksTheFraming`), so it is refused.
pub fn kappa_preimage(path: &str, tokens: &str) -> Vec<u8> {
    assert!(
        !path.contains('\n'),
        "a crate path holds a newline, which the κ framing cannot tell apart: {path:?}"
    );
    format!("{DOMAIN}\n{path}\n{tokens}\n").into_bytes()
}

/// What the lift decided for one function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// It lifted to this LexLean definition.
    Lifted(String),
    /// A pure function outside the lift's floor: the cause's code and detail.
    Residue(String, String),
    /// A parameter or the result is a registry crate's type: the detail names
    /// the crate and the type.
    Foreign(String),
    /// It performs or reaches an operating-system effect: the kind.
    Effect(String),
}

impl Status {
    /// The row table this status is written under.
    pub fn table(&self) -> &'static str {
        match self {
            Self::Lifted(_) => "lifted",
            Self::Residue(..) => "residue",
            Self::Foreign(_) => "foreign",
            Self::Effect(_) => "effect",
        }
    }
}

/// One function of the register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Its crate path, `crate::module::Type::name`.
    pub function: String,
    /// What the lift decided.
    pub status: Status,
    /// Where a lifted function's definition parts from the Rust function:
    /// a cause the lift states, such as a Unicode table LexLean does not
    /// bind. A row that is not lifted has none.
    pub divergence: Option<String>,
}

/// Every function of product code, by κ.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Register {
    /// Rows by κ.
    pub rows: BTreeMap<String, Row>,
}

impl Register {
    /// Read the function rows of a `frontier.toml` or a register file. Every
    /// row carries a κ that appears once, every function appears once, and
    /// every row that is not lifted is typed: a residue row its reason and
    /// detail, a foreign row the crate of its type, an effect row its effect.
    /// A register that is not one is [`UorustError::Malformed`] at `path`.
    pub fn parse(path: &Path, text: &str) -> Result<Register, UorustError> {
        let bad = |reason: String| UorustError::Malformed {
            path: path.to_path_buf(),
            reason,
        };
        let doc: toml::Value = text.parse().map_err(|e| bad(format!("{e}")))?;
        let mut register = Register::default();
        let mut functions = BTreeMap::new();
        for table in ["lifted", "residue", "foreign", "effect"] {
            let rows = doc.get(table).and_then(toml::Value::as_array);
            for row in rows.into_iter().flatten() {
                let field = |name: &str| {
                    row.get(name)
                        .and_then(toml::Value::as_str)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string)
                        .ok_or_else(|| bad(format!("a {table} row without {name}: {row}")))
                };
                let kappa = field("kappa")?;
                let function = field("function")?;
                let status = match table {
                    "lifted" => Status::Lifted(field("definition")?),
                    "residue" => Status::Residue(field("reason")?, field("detail")?),
                    "foreign" => {
                        let detail = field("detail")?;
                        if !detail.contains(" crate with no LexLean model") {
                            return Err(bad(format!("a foreign row names no crate: {row}")));
                        }
                        Status::Foreign(detail)
                    }
                    _ => Status::Effect(field("effect")?),
                };
                if kappa.len() != 64 || !kappa.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(bad(format!("{function}: κ {kappa} is not a SHA-256")));
                }
                if functions.insert(function.clone(), ()).is_some() {
                    return Err(bad(format!("{function} has more than one row")));
                }
                let divergence = match row.get("divergence") {
                    None => None,
                    Some(_) if table != "lifted" => {
                        return Err(bad(format!("a {table} row with a divergence: {row}")));
                    }
                    Some(_) => Some(field("divergence")?),
                };
                let new = Row {
                    function,
                    status,
                    divergence,
                };
                if register.rows.insert(kappa.clone(), new).is_some() {
                    return Err(bad(format!("κ {kappa} has more than one row")));
                }
            }
        }
        Ok(register)
    }

    /// How many functions lifted.
    pub fn lifted(&self) -> usize {
        self.rows
            .values()
            .filter(|r| matches!(r.status, Status::Lifted(_)))
            .count()
    }

    /// How many functions did not lift yet: the observation of the measure
    /// whose requirement is that every function of product code lifts. The
    /// rows the two counts take part the register, so they sum to its
    /// length; each is counted, so neither is a difference that could
    /// underflow.
    pub fn unlifted(&self) -> usize {
        self.rows
            .values()
            .filter(|r| !matches!(r.status, Status::Lifted(_)))
            .count()
    }

    /// The honesty report: how many functions lifted, and every row that
    /// did not by its cause (a residue row of the MIR path by the MIR kind
    /// or obligation that stopped it), as comment lines.
    pub fn honesty(&self) -> String {
        let mut by: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        let (mut foreign, mut effect) = (0usize, 0usize);
        let mut mir = false;
        for row in self.rows.values() {
            match &row.status {
                Status::Lifted(_) => {}
                Status::Foreign(_) => foreign += 1,
                Status::Effect(_) => effect += 1,
                Status::Residue(reason, detail) => {
                    let (cause, rest) = match detail.strip_prefix("mir::") {
                        Some(d) => {
                            mir = true;
                            match d.split_once(": ") {
                                Some((kind, rest)) => (format!("mir::{kind}"), rest.to_string()),
                                None => (format!("mir::{d}"), String::new()),
                            }
                        }
                        None => (reason.clone(), detail.clone()),
                    };
                    *by.entry(cause).or_default().entry(rest).or_default() += 1;
                }
            }
        }
        let residue: usize = by.values().flat_map(BTreeMap::values).sum();
        let mut out = format!(
            "# Honesty report: {} of {} functions of product code are lifted; {residue}\n\
             # are residue, {foreign} foreign and {effect} effect rows.\n",
            self.lifted(),
            self.rows.len()
        );
        if residue > 0 {
            out.push_str("# Residue by cause:\n");
        }
        for (cause, details) in &by {
            let n: usize = details.values().sum();
            out.push_str(&format!("#   {cause}: {n}\n"));
            if cause.starts_with("mir::") {
                for (detail, k) in details.iter().filter(|(d, _)| !d.is_empty()) {
                    out.push_str(&format!("#     {k} {detail}\n"));
                }
            }
        }
        if mir {
            out.push_str(&crate::mir::coverage::report());
        }
        out.push('\n');
        out
    }

    /// The register file: one row per function, by table and then function,
    /// without spans, so that only a function's own source moves its row.
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "# The lift register, written by uorust: every function of product code by\n\
             # the κ of its question (crate path and normalized tokens). A lifted κ is a\n\
             # proved claim; a later projection that decides it unlifted is refused.\n\n\
             spec = \"{SCHEMA}\"\n"
        );
        out.insert_str(out.find("spec = ").unwrap_or(0), &self.honesty());
        let mut rows: Vec<(&String, &Row)> = self.rows.iter().collect();
        rows.sort_by(|a, b| a.1.function.cmp(&b.1.function));
        for table in ["residue", "foreign", "effect", "lifted"] {
            for (kappa, row) in &rows {
                if row.status.table() != table {
                    continue;
                }
                out.push_str(&format!(
                    "\n[[{table}]]\nkappa = \"{kappa}\"\nfunction = {}\n",
                    quoted(&row.function)
                ));
                match &row.status {
                    Status::Lifted(d) => out.push_str(&format!("definition = {}\n", quoted(d))),
                    Status::Residue(reason, detail) => out.push_str(&format!(
                        "reason = {}\ndetail = {}\n",
                        quoted(reason),
                        quoted(detail)
                    )),
                    Status::Foreign(detail) => {
                        out.push_str(&format!("detail = {}\n", quoted(detail)));
                    }
                    Status::Effect(effect) => {
                        out.push_str(&format!("effect = {}\n", quoted(effect)));
                    }
                }
                if let Some(divergence) = &row.divergence {
                    out.push_str(&format!("divergence = {}\n", quoted(divergence)));
                }
            }
        }
        out
    }
}

/// The κ the incumbent records as lifted that the candidate decides
/// otherwise, each with both decisions: empty exactly when the candidate is
/// admitted. A κ the candidate does not hold is a question no longer asked
/// (its function changed or went away), not a regression.
pub fn admit(incumbent: &Register, candidate: &Register) -> Vec<String> {
    incumbent
        .rows
        .iter()
        .filter(|(_, row)| matches!(row.status, Status::Lifted(_)))
        .filter_map(|(kappa, row)| {
            let now = candidate.rows.get(kappa)?;
            match &now.status {
                // A κ lifted exactly stays exact: a divergence is a weaker
                // claim.
                Status::Lifted(_) if row.divergence.is_none() && now.divergence.is_some() => {
                    Some(format!(
                        "{} (κ {kappa}) was lifted exactly and now diverges: {}",
                        row.function,
                        now.divergence.as_deref().unwrap_or_default()
                    ))
                }
                Status::Lifted(_) => None,
                other => Some(format!(
                    "{} (κ {kappa}) was lifted and is now {}",
                    row.function,
                    describe(other)
                )),
            }
        })
        .collect()
}

fn describe(status: &Status) -> String {
    match status {
        Status::Lifted(d) => format!("lifted to {d}"),
        Status::Residue(_, detail) => format!("residue: {detail}"),
        Status::Foreign(detail) => format!("foreign: {detail}"),
        Status::Effect(effect) => format!("an effect: {effect}"),
    }
}

fn quoted(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(function: &str, status: Status) -> (String, Row) {
        (
            kappa(function, "fn f () {}"),
            Row {
                function: function.into(),
                status,
                divergence: None,
            },
        )
    }

    #[test]
    fn a_lifted_kappa_that_turns_unlifted_is_refused() {
        let lifted = row("c::f", Status::Lifted("Behavior::f".into()));
        let other = row("c::g", Status::Residue("form".into(), "a loop".into()));
        let incumbent = Register {
            rows: [lifted.clone(), other.clone()].into_iter().collect(),
        };
        // Still lifted, and the residue row lifted too: admitted.
        let better = Register {
            rows: [
                lifted.clone(),
                (
                    other.0.clone(),
                    Row {
                        function: "c::g".into(),
                        status: Status::Lifted("Behavior::g".into()),
                        divergence: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        assert!(admit(&incumbent, &better).is_empty());
        // The lifted function's source changed: a new question, admitted.
        let changed = Register {
            rows: [other.clone()].into_iter().collect(),
        };
        assert!(admit(&incumbent, &changed).is_empty());
        // The same κ decided as residue: refused, naming it.
        let worse = Register {
            rows: [
                (
                    lifted.0.clone(),
                    Row {
                        function: "c::f".into(),
                        status: Status::Residue("call".into(), "calls g".into()),
                        divergence: None,
                    },
                ),
                other,
            ]
            .into_iter()
            .collect(),
        };
        let refused = admit(&incumbent, &worse);
        assert_eq!(refused.len(), 1);
        assert!(refused[0].contains(&lifted.0), "{refused:?}");
        // The same κ lifted, but now with a divergence: a weaker claim,
        // refused; a κ that diverged and now lifts exactly is admitted.
        let diverging = |r: &(String, Row)| {
            let mut r = r.clone();
            r.1.divergence = Some("no Unicode case table".into());
            r
        };
        let weaker = Register {
            rows: [diverging(&lifted)].into_iter().collect(),
        };
        let refused = admit(&incumbent, &weaker);
        assert_eq!(refused.len(), 1);
        assert!(refused[0].contains("now diverges"), "{refused:?}");
        assert!(admit(&weaker, &incumbent).is_empty());
    }

    #[test]
    fn the_register_round_trips_and_rows_are_typed() {
        let register = Register {
            rows: [
                row("c::f", Status::Lifted("Behavior::f".into())),
                {
                    let (k, mut r) = row("c::e", Status::Lifted("Behavior::e".into()));
                    r.divergence = Some("lowers ASCII letters only".into());
                    (k, r)
                },
                row("c::g", Status::Effect("Fs effect: std::fs::read".into())),
                row(
                    "c::h",
                    Status::Foreign(
                        "takes or returns syn::Expr, a type of the syn crate with no LexLean model"
                            .into(),
                    ),
                ),
                row("c::i", Status::Residue("form".into(), "a \"loop\"".into())),
            ]
            .into_iter()
            .collect(),
        };
        let at = Path::new("frontier.toml");
        assert_eq!(
            Register::parse(at, &register.to_text()),
            Ok(register.clone())
        );
        assert_eq!((register.lifted(), register.unlifted()), (2, 3));
        assert_eq!(register.lifted() + register.unlifted(), register.rows.len());
        let stray = "[[residue]]\nkappa = \"00\"\nfunction = \"c::f\"\nreason = \"form\"\ndetail = \"d\"\ndivergence = \"x\"\n";
        assert!(Register::parse(at, stray).is_err());
        let untyped = "[[residue]]\nkappa = \"00\"\nfunction = \"c::f\"\n";
        assert!(Register::parse(at, untyped).is_err());
        let twice = format!(
            "{}\n[[lifted]]\nkappa = \"{}\"\nfunction = \"c::f\"\ndefinition = \"D\"\n",
            register.to_text(),
            kappa("c::z", "")
        );
        assert!(
            Register::parse(at, &twice).is_err_and(|e| e.to_string().contains("more than one row"))
        );
    }

    #[test]
    fn the_preimage_is_the_models_fixture() {
        // Kappa::kappaPreimageFixture, which Kappa::theFixtureIsThePreimage
        // proves is the model's preimage of these two inputs.
        let model = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/Kappa.lex.tex"),
        )
        .expect("Kappa.lex.tex");
        let at = model
            .find("name: kappaPreimageFixture,")
            .expect("the fixture");
        let body = &model[at..model[at..].find("\n}").map_or(model.len(), |e| at + e)];
        let mut bytes = Vec::new();
        for piece in body.split("(ctor Symbol.").skip(1) {
            if piece.starts_with("newline") {
                bytes.push(b'\n');
            } else {
                let n = piece
                    .trim_start_matches("byte ")
                    .split(')')
                    .next()
                    .expect("a byte");
                bytes.push(n.trim().parse::<u8>().expect("a byte"));
            }
        }
        assert_eq!(kappa_preimage("c::f", "fn f () {}"), bytes);
    }

    #[test]
    #[should_panic(expected = "newline")]
    fn a_path_with_a_newline_is_refused() {
        let _ = kappa_preimage("c::f\ng", "x");
    }

    #[test]
    fn kappa_depends_on_path_and_tokens() {
        assert_ne!(kappa("c::f", "fn f () {}"), kappa("c::g", "fn f () {}"));
        assert_ne!(kappa("c::f", "fn f () {}"), kappa("c::f", "fn f () { 1 }"));
        assert_eq!(kappa("c::f", "x").len(), 64);
    }
}
