//! The grep-shaped gates.
//!
//! These are crude on purpose. A gate that needs interpretation is a gate that
//! gets argued with; these ones read the source, find a token, and fail. Each
//! carries the rule it enforces in its failure message, because the point of a
//! red gate is to name the promise that was broken.

use std::path::{Path, PathBuf};

use crate::Fail;

/// The crates that ship: every crate under `crates/` that is not
/// `publish = false`. The rules below apply to those and not to the
/// dev-and-CI-only crates, which may use `std`, `alloc`, and floats freely.
///
/// Derived from the manifests rather than listed here. A list would be a second
/// place to remember, and the failure mode of a second place is that a crate
/// added to one is missing from the other --- silently, because a gate that
/// skips a crate cannot report on it. Reading `publish = false` asks the same
/// question `cargo publish` asks.
fn shipped(root: &Path) -> Result<Vec<String>, Fail> {
    let mut out = Vec::new();
    let dir = root.join("crates");
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        let manifest = path.join("Cargo.toml");
        let Ok(toml) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        if toml.lines().any(|l| l.trim() == "publish = false") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            out.push(name.to_string());
        }
    }
    out.sort();
    Ok(out)
}

struct Source {
    rel: String,
    text: String,
}

fn shipped_sources(root: &Path) -> Result<Vec<Source>, Fail> {
    let mut out = Vec::new();
    for name in shipped(root)? {
        let dir = root.join("crates").join(&name).join("src");
        if !dir.exists() {
            continue;
        }
        collect(&dir, root, &mut out)?;
    }
    Ok(out)
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<Source>) -> Result<(), Fail> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, root, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path)?;
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            out.push(Source { rel, text });
        }
    }
    Ok(())
}

/// Lines of `text` outside comments and outside `#[cfg(test)]` modules.
///
/// A rule about what the code *does* must not be tripped by a doc comment
/// explaining what it does not do, nor by a test that deliberately constructs
/// the forbidden thing to prove it is caught.
fn effective_lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut test: Option<TestItem> = None;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with("//") || line.starts_with("#!") {
            // A `#![deny(...)]` or a comment states policy; it never is the
            // behaviour the gate is looking for.
            continue;
        }
        let code = line.split("//").next().unwrap_or("");
        if test.is_none() && line.starts_with("#[cfg(test)]") {
            test = Some(TestItem::at(depth));
        }
        match test.as_mut() {
            Some(item) => {
                if item.read(code, &mut depth) {
                    test = None;
                }
            }
            None => {
                depth += Scan::new(code).map(|c| c.brace).sum::<i32>();
                if !code.trim().is_empty() {
                    out.push((i + 1, code));
                }
            }
        }
    }
    out
}

/// What a `#[cfg(test)]` attribute applies to, as far as the scan needs.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// A struct field or an enum variant: ends at its `,`.
    Field,
    /// A match arm: ends at its `,` unless it opened a block.
    Arm,
    /// Any other item: ends at its `;` or when its block closes.
    Item,
}

/// The `#[cfg(test)]` item being read: everything from the attribute to the
/// item's end is test code, and none of it is shipped.
struct TestItem {
    /// The brace depth at the attribute.
    depth: i32,
    /// Whether an attribute of the run is being read, and its bracket balance.
    in_attribute: bool,
    attribute: i32,
    /// Known once the first line after the attribute run is read.
    kind: Option<Kind>,
    /// Paren and bracket nesting inside the item.
    nest: i32,
    /// Whether the item has opened a brace.
    opened: bool,
}

impl TestItem {
    fn at(depth: i32) -> Self {
        TestItem {
            depth,
            in_attribute: false,
            attribute: 0,
            kind: None,
            nest: 0,
            opened: false,
        }
    }

    /// Read one line of the item; true when the item ends on it. `depth`
    /// tracks every brace of the line, including any after the item's end.
    fn read(&mut self, code: &str, depth: &mut i32) -> bool {
        let mut ended = false;
        for c in Scan::new(code) {
            *depth += c.brace;
            if ended || c.quoted {
                continue;
            }
            if self.kind.is_none() {
                if self.attribute == 0 && c.ch == '#' && code[c.at..].starts_with("#[") {
                    self.in_attribute = true;
                    continue;
                }
                if self.in_attribute {
                    self.attribute += i32::from(c.ch == '[') - i32::from(c.ch == ']');
                    self.in_attribute = self.attribute > 0;
                    continue;
                }
                if c.ch.is_whitespace() {
                    continue;
                }
                self.kind = Some(kind_of(&code[c.at..]));
            }
            ended = self.step(c.ch, *depth);
        }
        ended
    }

    /// One character of the item's body, after `depth` has counted it.
    fn step(&mut self, ch: char, depth: i32) -> bool {
        match ch {
            '(' | '[' => self.nest += 1,
            ')' | ']' => self.nest -= 1,
            '{' => self.opened = true,
            '}' if depth < self.depth || (self.opened && depth == self.depth) => return true,
            ';' if depth == self.depth && self.nest == 0 => return true,
            ',' if depth == self.depth && self.nest == 0 => {
                return match self.kind {
                    Some(Kind::Field) => true,
                    Some(Kind::Arm) => !self.opened,
                    _ => false,
                };
            }
            _ => {}
        }
        false
    }
}

/// The kind of item whose text starts `rest`.
fn kind_of(rest: &str) -> Kind {
    let mut rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix("pub(") {
        rest = after.split_once(')').map_or(after, |(_, r)| r).trim_start();
    } else if let Some(after) = rest.strip_prefix("pub ") {
        rest = after.trim_start();
    }
    if rest.contains("=>") {
        return Kind::Arm;
    }
    let word: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    const ITEMS: [&str; 16] = [
        "use",
        "const",
        "static",
        "type",
        "mod",
        "struct",
        "enum",
        "union",
        "trait",
        "impl",
        "fn",
        "extern",
        "default",
        "async",
        "unsafe",
        "macro_rules",
    ];
    if word.is_empty() || ITEMS.contains(&word.as_str()) {
        return Kind::Item;
    }
    let after = rest[word.len()..].trim_start();
    if after.starts_with('!') || after.starts_with("::") {
        return Kind::Item;
    }
    if after.starts_with(':')
        || after.is_empty()
        || ['(', '{', '=', ','].iter().any(|c| after.starts_with(*c))
    {
        return Kind::Field;
    }
    Kind::Item
}

/// One character of a line, with whether it sits in a string or character
/// literal and the brace depth change it makes.
struct Char {
    at: usize,
    ch: char,
    quoted: bool,
    brace: i32,
}

/// The characters of a line, literal-aware: a string literal, and a
/// character literal of a brace (`'{'`), are quoted.
struct Scan<'a> {
    chars: std::str::CharIndices<'a>,
    text: &'a str,
    string: bool,
    escaped: bool,
    /// Characters still inside a character literal.
    literal: u8,
}

impl<'a> Scan<'a> {
    fn new(text: &'a str) -> Self {
        Scan {
            chars: text.char_indices(),
            text,
            string: false,
            escaped: false,
            literal: 0,
        }
    }
}

impl Iterator for Scan<'_> {
    type Item = Char;

    fn next(&mut self) -> Option<Char> {
        let (at, ch) = self.chars.next()?;
        let quoted = if self.literal > 0 {
            self.literal -= 1;
            true
        } else if self.string {
            match (self.escaped, ch) {
                (true, _) => self.escaped = false,
                (false, '\\') => self.escaped = true,
                (false, '"') => self.string = false,
                _ => {}
            }
            true
        } else if ch == '"' {
            self.string = true;
            true
        } else if ["'{'", "'}'"]
            .iter()
            .any(|l| self.text[at..].starts_with(l))
        {
            self.literal = 2;
            true
        } else {
            false
        };
        let brace = match (quoted, ch) {
            (false, '{') => 1,
            (false, '}') => -1,
            _ => 0,
        };
        Some(Char {
            at,
            ch,
            quoted,
            brace,
        })
    }
}

/// The only error types a shipped crate may name, plus the declaration check at
/// a declared boundary, which is not the operation failing.
///
/// `UorustError` is the engine's: it reports only that no projection exists
/// (the workspace cannot be read, the output directory is not the engine's, the
/// committed projection is stale), never a bound of the engine's own choosing.
/// An ambiguity in the workspace is not an error at all: admission records it
/// as a typed refusal in `frontier.toml`. Every variant is typed, modeled
/// (`UE-06`) and covered by a negative test in `crates/uorust/tests`, so the
/// addition keeps R5 instead of loosening it; the tests below prove the gate
/// still rejects every other error type.
///
/// `Residue` is the lift's: a lift step's `Result<T, Residue>` reports only
/// that no LexLean definition of the function exists in the fragment the
/// engine reads, with the cause. It is not a failure of the projection: the
/// residue is recorded, typed, in `frontier.toml` (`UE-16`), and each cause
/// is a form or type the lift does not yet read, never a bound of the
/// engine's own choosing.
const SANCTIONED: [&str; 5] = [
    "NotAProduct",
    "ObservedBound",
    "KappaError",
    "UorustError",
    "Residue",
];

/// R5: no arbitrary limitation. Every bound is a property of the caller's
/// chosen instantiation, never of the code.
///
/// Concretely: no shipped crate may return an error the model does not
/// sanction. The absence of a class of error is checked here --- the absence of
/// negative testing is only honest if there is nothing to test negatively.
pub fn audit_limits(root: &Path) -> Result<(), Fail> {
    let sources = shipped_sources(root)?;
    let sanctioned = SANCTIONED;

    let mut violations = Vec::new();
    for src in &sources {
        for (line_no, line) in effective_lines(&src.text) {
            let Some(pos) = line.find("Result<") else {
                continue;
            };
            let tail = &line[pos..];
            if sanctioned.iter().any(|s| tail.contains(s)) {
                continue;
            }
            violations.push(format!("{}:{line_no}:{}", src.rel, line.trim()));
        }
    }
    if !violations.is_empty() {
        return Err(format!(
            "R5: every bound is derived from declared parameters and is a \
             property of the caller's chosen instantiation. The only reportable \
             condition is that the requested object does not exist, reported at view \
             construction. A `Result` over anything else is a limitation the model does \
             not sanction.\n\n{}",
            violations.join("\n")
        )
        .into());
    }
    println!("audit-limits: no shipped crate returns an unsanctioned error (R5)");
    Ok(())
}

/// R4: nothing is deferred. No deferral marker, no stub, no placeholder
/// document section, no capability behind a flag that turns it off.
///
/// The markers are spelled in halves. This gate reads every crate *and*
/// `xtask`, which means it reads this file, and a list of forbidden tokens
/// written out in full is a list that matches itself --- the gate would fail on
/// its own definition, for ever. The alternative is exempting this file, and an
/// exemption is a hole: a real deferral parked in the gate would then be the one
/// place nothing looks. Split, the gate scans itself like anything else.
pub fn audit_deferral(root: &Path) -> Result<(), Fail> {
    let markers = [
        concat!("TO", "DO"),
        concat!("FIX", "ME"),
        concat!("XX", "X"),
        concat!("unimplemented", "!"),
        concat!("to", "do!"),
        concat!("for ", "now"),
        concat!("later ", "version"),
    ];
    let mut violations = Vec::new();

    // Every crate, not only the shipped ones, and `xtask` with them: R4 is a
    // promise about the repository, and a deferral parked in a gate is the same
    // deferral as one parked in shipped code.
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in [root.join("crates"), root.join("xtask")] {
        if dir.exists() {
            gather_all(&dir, &mut files)?;
        }
    }
    // Every Markdown file at the root, discovered rather than listed. A list
    // here would go stale the first time a document was added or renamed, and
    // the failure would be silent: a document nobody scans reports nothing.
    for entry in std::fs::read_dir(root)? {
        let p = entry?.path();
        if p.extension().is_some_and(|e| e == "md") {
            files.push(p);
        }
    }

    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        for (i, line) in text.lines().enumerate() {
            for marker in markers {
                if !line.contains(marker) {
                    continue;
                }
                // A backticked marker is a *mention*, not a use: the
                // documentation has to be able to name what the gate catches
                // without tripping it. Anything outside code spans is real.
                if outside_code_spans(line, marker) {
                    violations.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                }
            }
        }
    }

    if !violations.is_empty() {
        return Err(format!(
            "R4: nothing is deferred. None of {} may appear, and no stub, no \
             placeholder section, and no capability behind a flag that turns it \
             off. Every capability ships in the one release.\n\n{}",
            markers.join(", "),
            violations.join("\n")
        )
        .into());
    }
    println!("audit-deferral: nothing is deferred (R4)");
    Ok(())
}

/// Does `marker` occur in `line` outside every backtick-delimited span?
fn outside_code_spans(line: &str, marker: &str) -> bool {
    let mut rest = line;
    let mut at = 0usize;
    while let Some(pos) = rest.find(marker) {
        let absolute = at + pos;
        // An odd number of backticks before this occurrence means it is inside
        // a span.
        if line[..absolute].matches('`').count().is_multiple_of(2) {
            return true;
        }
        at = absolute + marker.len();
        rest = &line[at..];
    }
    false
}

fn gather_all(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Fail> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            gather_all(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|e| e == "rs" || e == "md" || e == "toml")
        {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch repository with one shipped crate. Each call gets its own
    /// directory, so tests running in parallel never share one.
    fn shipped_crate(source: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("audit-limits-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("crates/shipped/src");
        std::fs::create_dir_all(&src).expect("temp crate");
        std::fs::write(
            root.join("crates/shipped/Cargo.toml"),
            "[package]\nname = \"shipped\"\n",
        )
        .expect("manifest");
        std::fs::write(src.join("lib.rs"), source).expect("source");
        root
    }

    #[test]
    fn the_engine_error_is_sanctioned() {
        let root = shipped_crate("pub fn f() -> Result<(), UorustError> { Ok(()) }\n");
        assert!(audit_limits(&root).is_ok());
    }

    #[test]
    fn the_lift_residue_is_sanctioned() {
        let root = shipped_crate("pub type Lift<T> = Result<T, Residue>;\n");
        assert!(audit_limits(&root).is_ok());
    }

    #[test]
    fn any_other_error_type_is_still_refused() {
        for other in [
            "std::io::Error",
            "String",
            "Box<dyn std::error::Error>",
            "Uorust",
            "Resid",
        ] {
            let root = shipped_crate(&format!("pub fn f() -> Result<(), {other}> {{ Ok(()) }}\n"));
            let error = audit_limits(&root).expect_err(other).to_string();
            assert!(error.contains("R5"), "{other}: {error}");
        }
    }

    #[test]
    fn a_publish_false_crate_is_not_audited() {
        let root = shipped_crate("pub fn f() -> Result<(), String> { Ok(()) }\n");
        std::fs::write(
            root.join("crates/shipped/Cargo.toml"),
            "[package]\nname = \"shipped\"\npublish = false\n",
        )
        .expect("manifest");
        assert!(audit_limits(&root).is_ok());
    }

    /// GATE-04: a braceless `#[cfg(test)]` item hides only itself. Test code
    /// names `std::fmt::Error` and the shipped `f` after it names
    /// `std::io::Error`, so the refusal shows which lines the gate read.
    #[test]
    fn a_braceless_test_item_does_not_hide_the_next_item() {
        let shipped = "pub fn f() -> Result<(), std::io::Error> { Ok(()) }\n";
        let braceless = [
            "#[cfg(test)]\nmod generate;\n",
            "#[cfg(test)] use x::y;\n",
            "pub struct S {\n    #[cfg(test)]\n    pub rust: String,\n    pub a: u8,\n}\n",
        ];
        let exempt = [
            "#[cfg(test)]\nmod tests {\n    fn g() -> Result<(), std::fmt::Error> { Ok(()) }\n}\n",
            "#[cfg(test)]\npub(crate) fn mark(\n    a: u8,\n    b: u8,\n) -> Result<(), std::fmt::Error> {\n    let r: Result<(), std::fmt::Error> = Ok(());\n    r\n}\n",
            "#[cfg(test)]\nfn w<T>(t: T) -> Result<T, std::fmt::Error>\nwhere\n    T: Clone,\n{\n    let r: Result<T, std::fmt::Error> = Ok(t);\n    r\n}\n",
            "#[cfg(test)]\nfn g<\n    A: Clone,\n    B,\n>() {\n    let r: Result<(), std::fmt::Error> = Ok(());\n    r.ok();\n}\n",
            "#[cfg(test)]\nunsafe impl Send for S {\n}\n#[cfg(test)]\nimpl S {\n    fn h() -> Result<(), std::fmt::Error> { Ok(()) }\n}\n",
            "#[cfg(test)]\nthread_local! {\n    static T: Result<(), std::fmt::Error> = Ok(());\n}\n",
            "#[cfg(test)]\nextern \"C\" {\n    fn e(x: u8) -> u8;\n}\n#[cfg(test)]\nconst K: Result<(), std::fmt::Error> = Ok(());\n",
            "#[cfg(test)]\nstatic S2: [u8; 2] = [1, 2];\n#[cfg(test)]\nstatic R: Result<(), std::fmt::Error> = Ok(());\n",
            "pub fn m(k: u8) -> u8 {\n    match k {\n        #[cfg(test)]\n        Kind::Probe => 1,\n        _ => 0,\n    }\n}\n#[cfg(test)]\nfn p() -> Result<(), std::fmt::Error> { Ok(()) }\n",
            "#[cfg(test)]\n#[allow(\n    dead_code,\n)]\nmod tests {\n    fn g() -> Result<(), std::fmt::Error> { Ok(()) }\n}\n",
        ];
        for plant in braceless.iter().chain(exempt.iter()) {
            let root = shipped_crate(&format!("{plant}{shipped}"));
            let error = audit_limits(&root)
                .expect_err("the shipped f is read")
                .to_string();
            assert!(
                error.contains("std::io::Error"),
                "{plant:?}: f must be refused: {error}"
            );
            assert!(
                !error.contains("std::fmt::Error"),
                "{plant:?}: test code must stay exempt: {error}"
            );
        }
    }
}
