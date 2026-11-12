//! The verdict register: what the verifier decided for each translation.
//!
//! A translation is a candidate until `lexlean verify` has elaborated its
//! definitions and proved its theorems (the obligations of its asserts,
//! panics and arithmetic, and the decrease of each loop's measure). The
//! register keeps, by the κ of the Rust function and the SHA-256 of the
//! translation's text, whether it was proved, or refused and why, and the
//! exact axioms each declaration elaborated with as the verifier observed
//! them. A projection admits a translation only on a `proved` row of the same
//! digest, so the gate never meets an obligation that did not discharge, and
//! a translation that changes is a new question. `uorust prove` writes it.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::io;
use crate::UorustError;

/// The register's schema.
pub const SCHEMA: &str = "uorust/verdicts/1";

/// What the verifier decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Every declaration elaborated and every theorem was proved.
    Proved,
    /// The verifier refused it: the diagnostic code and message.
    Refused(String),
}

/// One translation's verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// The Rust function's path.
    pub function: String,
    /// The SHA-256 of the translation's text.
    pub digest: String,
    /// The decision.
    pub status: Decision,
    /// The axioms each declaration elaborated with, where the verifier
    /// reported them.
    pub axioms: BTreeMap<String, Vec<String>>,
}

/// Every verdict, by the κ of its function.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdicts {
    /// Rows by κ.
    pub rows: BTreeMap<String, Verdict>,
}

impl Verdicts {
    /// Read the register at `path`; an absent file is an empty register.
    pub fn read(path: &Path) -> Result<Verdicts, UorustError> {
        if !path.is_file() {
            return Ok(Verdicts::default());
        }
        let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
        Self::parse(path, &text)
    }

    /// Parse a register's text.
    pub fn parse(path: &Path, text: &str) -> Result<Verdicts, UorustError> {
        let bad = |reason: String| UorustError::Malformed {
            path: path.to_path_buf(),
            reason,
        };
        let doc: toml::Value = text.parse().map_err(|e| bad(format!("{e}")))?;
        let mut out = Verdicts::default();
        for row in doc
            .get("verdict")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let field = |name: &str| {
                row.get(name)
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| bad(format!("a verdict without {name}")))
            };
            let status = match field("status")?.as_str() {
                "proved" => Decision::Proved,
                "refused" => Decision::Refused(field("cause")?),
                other => return Err(bad(format!("a verdict of status {other}"))),
            };
            let mut axioms = BTreeMap::new();
            if let Some(table) = row.get("axioms").and_then(toml::Value::as_table) {
                for (decl, list) in table {
                    let list = list
                        .as_array()
                        .ok_or_else(|| bad(format!("axioms of {decl} are not a list")))?;
                    axioms.insert(
                        decl.clone(),
                        list.iter()
                            .filter_map(toml::Value::as_str)
                            .map(str::to_string)
                            .collect(),
                    );
                }
            }
            out.rows.insert(
                field("kappa")?,
                Verdict {
                    function: field("function")?,
                    digest: field("digest")?,
                    status,
                    axioms,
                },
            );
        }
        Ok(out)
    }

    /// The register's text: rows by κ, the same bytes for the same rows.
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "# The verdict register of the MIR path, written by `uorust prove`: for each\n\
             # translated function (by the κ of its question and the SHA-256 of its\n\
             # translation), whether `lexlean verify` proved every theorem of the\n\
             # translation, and the axioms each declaration elaborated with. A\n\
             # projection admits a translation only on a proved row of its digest.\n\n\
             spec = \"{SCHEMA}\"\n"
        );
        for (kappa, v) in &self.rows {
            out.push_str(&format!(
                "\n[[verdict]]\nkappa = {}\nfunction = {}\ndigest = {}\n",
                verdict_quoted(kappa),
                verdict_quoted(&v.function),
                verdict_quoted(&v.digest)
            ));
            match &v.status {
                Decision::Proved => out.push_str("status = \"proved\"\n"),
                Decision::Refused(cause) => {
                    out.push_str(&format!(
                        "status = \"refused\"\ncause = {}\n",
                        verdict_quoted(cause)
                    ));
                }
            }
            if !v.axioms.is_empty() {
                let parts: Vec<String> = v
                    .axioms
                    .iter()
                    .map(|(d, list)| {
                        let items: Vec<String> = list.iter().map(|a| verdict_quoted(a)).collect();
                        format!("{d} = [{}]", items.join(", "))
                    })
                    .collect();
                out.push_str(&format!("axioms = {{ {} }}\n", parts.join(", ")));
            }
        }
        out
    }
}

fn verdict_quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_round_trip() {
        let mut v = Verdicts::default();
        v.rows.insert(
            "k1".into(),
            Verdict {
                function: "c::f".into(),
                digest: "d".into(),
                status: Decision::Proved,
                axioms: [("holds_f_0".to_string(), vec!["propext".to_string()])]
                    .into_iter()
                    .collect(),
            },
        );
        v.rows.insert(
            "k2".into(),
            Verdict {
                function: "c::g".into(),
                digest: "e".into(),
                status: Decision::Refused("LLV7001: \"omega\" failed".into()),
                axioms: BTreeMap::new(),
            },
        );
        let text = v.to_text();
        assert_eq!(Verdicts::parse(Path::new("v.toml"), &text).unwrap(), v);
    }
}
