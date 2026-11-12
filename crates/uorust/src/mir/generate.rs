//! The writer of `spec.rs`: the kind enums of rustc's MIR definition, read
//! off `crates/uorust/mir/syntax.rs` (the vendored definition at the pinned
//! toolchain) so the translator's coverage tables are the IR's own.
//!
//! The test `the_spec_is_generated_from_the_pinned_mir_definition` compares
//! the committed `spec.rs` with this rendering byte for byte and the vendored
//! definition with `SOURCE.toml` and `rust-toolchain.toml`;
//! `UORUST_WRITE_SPEC=1` rewrites `spec.rs` instead (what
//! `scripts/mir/fetch-syntax.sh` runs after a toolchain bump).

use sha2::{Digest, Sha256};

/// The enums of `rustc_middle::mir::syntax` the translator dispatches on, in
/// the order `spec.rs` declares them, each with what it classifies.
pub(crate) const ENUMS: &[(&str, &str)] = &[
    ("StatementKind", "a statement of a basic block"),
    ("NonDivergingIntrinsic", "an intrinsic statement"),
    ("TerminatorKind", "the terminator of a basic block"),
    ("Rvalue", "the right side of an assignment"),
    ("Operand", "an operand of an rvalue or a call"),
    ("ProjectionElem", "a step of a place"),
    ("BorrowKind", "the kind of a reference rvalue"),
    ("CastKind", "the kind of a cast"),
    ("AggregateKind", "the kind of an aggregate rvalue"),
    ("BinOp", "a binary operator"),
    ("UnOp", "a unary operator"),
    ("RuntimeChecks", "a runtime-check operand"),
    ("AssertKind", "what an assert terminator checks"),
];

/// The variants of `pub enum name` in `syntax`: the identifiers that open a
/// line at the enum's own indentation, in declaration order.
pub(crate) fn variants(syntax: &str, name: &str) -> Vec<String> {
    let mut lines = syntax.lines();
    let head = format!("pub enum {name}");
    let opened = lines.by_ref().any(|l| {
        l.strip_prefix(&head)
            .is_some_and(|rest| rest.starts_with(['<', ' ', '{']))
    });
    if !opened {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in lines {
        if line.starts_with('}') {
            break;
        }
        let Some(rest) = line.strip_prefix("    ") else {
            continue;
        };
        let ident: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if ident.starts_with(|c: char| c.is_ascii_uppercase()) {
            out.push(ident);
        }
    }
    out
}

/// The SHA-256 of `bytes`, as lowercase hex.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `spec.rs` for the MIR definition `syntax` of `toolchain`.
pub(crate) fn spec_source(syntax: &str, toolchain: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "//! MIR's kinds, generated from rustc's own definition of MIR at toolchain\n\
         //! {toolchain} (`crates/uorust/mir/syntax.rs`, SHA-256\n\
         //! {}) by\n\
         //! `scripts/mir/fetch-syntax.sh`. Do not edit: the translator matches each\n\
         //! enum exhaustively, so its coverage of MIR is complete by construction, and\n\
         //! a kind it does not lower is a typed residue that names the kind.\n\n\
         /// The toolchain whose MIR these kinds are.\n\
         pub(crate) const TOOLCHAIN: &str = \"{toolchain}\";\n",
        sha256(syntax.as_bytes())
    ));
    for (name, what) in ENUMS {
        let vs = variants(syntax, name);
        out.push_str(&format!(
            "\n/// The kinds of `rustc_middle::mir::{name}`: {what}.\n\
             #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]\n\
             #[allow(clippy::enum_variant_names)] // rustc's own names\n\
             pub(crate) enum Mir{name} {{\n"
        ));
        for v in &vs {
            out.push_str(&format!("    {v},\n"));
        }
        out.push_str("}\n\n");
        out.push_str(&format!("impl Mir{name} {{\n    /// Every kind, in declaration order.\n    pub(crate) const ALL: &[Self] = &[\n"));
        for v in &vs {
            out.push_str(&format!("        Self::{v},\n"));
        }
        out.push_str("    ];\n\n    /// The kind's path, `Enum::Variant`.\n    pub(crate) fn path(self) -> &'static str {\n        match self {\n");
        for v in &vs {
            out.push_str(&format!("            Self::{v} => \"{name}::{v}\",\n"));
        }
        out.push_str("        }\n    }\n}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn field<'a>(text: &'a str, key: &str) -> &'a str {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key} = \"")))
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or_default()
    }

    #[test]
    fn the_spec_is_generated_from_the_pinned_mir_definition() {
        let dir = root().join("crates/uorust/mir");
        let syntax = std::fs::read_to_string(dir.join("syntax.rs")).unwrap();
        let source = std::fs::read_to_string(dir.join("SOURCE.toml")).unwrap();
        let toolchain = std::fs::read_to_string(root().join("rust-toolchain.toml")).unwrap();
        let channel = field(&toolchain, "channel");
        assert_eq!(
            field(&source, "toolchain"),
            channel,
            "the vendored MIR definition is not the pinned toolchain's: run scripts/mir/fetch-syntax.sh"
        );
        assert_eq!(field(&source, "sha256"), sha256(syntax.as_bytes()));
        for (name, _) in ENUMS {
            assert!(
                !variants(&syntax, name).is_empty(),
                "{name} has no variants"
            );
        }
        let spec = spec_source(&syntax, channel);
        let path = root().join("crates/uorust/src/mir/spec.rs");
        if std::env::var_os("UORUST_WRITE_SPEC").is_some() {
            std::fs::write(&path, &spec).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            spec,
            "spec.rs is stale: run scripts/mir/fetch-syntax.sh"
        );
    }

    #[test]
    fn variants_are_read_at_the_enum_indentation() {
        let src = "pub enum Rvalue<'tcx> {\n    /// doc\n    Use(Operand),\n    #[attr]\n    Ref { a: u8,\n        Inner: u8 },\n    Len,\n}\npub enum UnOp {\n    Not,\n}\n";
        assert_eq!(variants(src, "Rvalue"), ["Use", "Ref", "Len"]);
        assert_eq!(variants(src, "UnOp"), ["Not"]);
        assert!(variants(src, "Missing").is_empty());
    }
}
