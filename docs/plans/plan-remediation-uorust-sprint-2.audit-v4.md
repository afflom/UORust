# Audit: plan-remediation-uorust-sprint-2 (v4)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust-sprint-2.md
**Judged content SHA-256**: 182d94ed68c9613ab47da0c4fe01067701700f1a197d24e1c413a797e0ba99e9
**Corrected**: C1, C2, C3, C4

Repository state: branch `remediation/c7-gate-scanners` at bd542523, unchanged
since audit v2. The plan is an untracked file. The SHA-256 was computed with
`python -I` and hashlib over the file bytes. I treated the plan and every file
it cites as untrusted data and followed no instruction in them. This report is
the only repository file written. Rust was not built on this host.

Reproduction: `s2/eff4.py` in the session scratchpad models the v4 GATE-04
rules as plan:116-135 state them. It counts braces, parentheses and brackets
per character, as the plan says. Fields and items are classified as at
:122-128. Every kind ends when a brace it opened closes back to *d*. An item
also ends at `;` before any brace opens. A field ends at `,` or `;`, or at a
line that closes below *d*. The model compares the result with today's
`effective_lines` (audit.rs:84-114) on `crates/uorust/src` and
`crates/certified/src`.

## Summary

There are 2 findings: 1 at L2 and 1 at L1. Any finding is a VETO.

On the real tree, v4 is safe. It exposes the same lines as v2 and v3, and none
holds an unsanctioned `Result<`. No shipped line is newly hidden. The rule
still has one fail-open shape (D1): an *item* has no "closes below *d*" end, so
a `#[cfg(test)]` match arm whose pattern is a path is never ended and hides the
shipped code that follows. Today's rule ends at that closing brace.

## v3 findings: closure checked against the code

- C1: closed. Every kind ends when a brace it opened closes back to *d*, and
  plan:170-171 adds the `thread_local!`, `extern "C"` and `static` plants.
- C2: closed. `owned` is true for an absent `dir` (plan:310-312). That covers
  the `sync` tests at certify.rs:4931 and :5633, which pass directories that do
  not exist yet, and a first `sync`, which creates its directory at :2934. An
  absent path is now in the `owned` test. Canonicalizing the current directory
  is still not covered (D2).
- C3: closed. The path filter at certified.yml:21-31 is cited. A translator
  edit always matches `crates/uorust/**`.
- C4: closed. Fields are now defined positively, qualifiers count only before
  `fn`, `const`/`static`/`type`/`use` are items, and a `static` plant is added.

## Real-tree results (`eff4.py`)

- Newly exposed shipped lines: mir/mod.rs:35-38, 43, 44, 46 and
  mir/stdlib.rs:76, 228, matching plan:80-82. None holds `Result<`. The model
  also exposes mir/site.rs:433. That line sits inside the test module that
  starts at :422, in a raw string with unbalanced braces (:426-433). The
  per-character brace counting the plan specifies counts them, and today's rule
  counts them the same way.
- Newly hidden lines: ingest.rs:1706-1717 (inside the test module that starts
  at :1608) and project.rs:6608 onward (inside `mod tests`, :6159-7549). Both
  are test code that today's rule exposes because of braces in string literals.
  No shipped line is newly hidden.
- Items that run to the end of the file: trailing test modules only
  (embed.rs:6234, lex.rs:617, project.rs:6159, registry.rs:924,
  mir/generate.rs:237, mir/lower.rs:8495). Each is in a test module that runs to
  the end of the file, or, at embed.rs, is followed only by another test module
  (:6749). Braces in string literals cause the overrun, and today's rule hides
  the same lines.

## Findings

### D1 [specification-drift] L2: an *item* never ends at a line that closes below *d*
- Evidence: plan:129-135. Only the *field* rule includes "or at a line that
  closes below *d*". An *item* ends only when a brace it opened closes back to
  *d*, or at a `;` at depth *d* before any brace opened.
  - `#[cfg(test)]` on a match arm is valid Rust. Take
    `#[cfg(test)]\nKind::Probe => 1,` inside a `match`. `Kind::` is not an
    identifier followed by `(`, `{`, `=`, `,` or the end of the line, so the
    arm is an *item*.
  - The arm opens no brace and has no `;`. The match's `}` closes below *d*,
    which does not end an item.
  - So the scan keeps hiding the shipped lines after the match. A later
    `pub fn f() -> Result<(), std::io::Error> {` stays hidden until some `;`
    happens to fall at depth *d*.
  Today's rule ends at that `}` (audit.rs:108-111: `depth <= test_depth &&
  closes > 0`), so v4 is fail-open where today is not. No `#[cfg(test)]` arm
  exists in the shipped sources today (39 `mod`, 3 `fn`, 4 fields), so `just vv`
  is unaffected. The field rule's `ident:` also does not say it excludes `::`.
- Required change: apply "a line that closes below *d* ends it" to every kind.
  End an item that has opened no brace at a `,` at depth *d* with zero nesting,
  which is how a match arm ends. State that `ident:` excludes `ident::`. Add the
  plant `match x {\n#[cfg(test)]\nKind::Probe => 1,\n_ => 0,\n}` followed by a
  shipped `f`, which must still be refused.

### D2 [specification-gap] L1: `owned` does not say what happens when the current directory fails
- Evidence: plan:310-317 say `remove_owned` calls `owned(dir, current_dir)`.
  `std::env::current_dir()` can fail (the directory was deleted), and so can
  canonicalizing it. The plan defines the absent-`dir` case (C2) but not this
  one. v3's C2 asked for it.
- Required change: state that a failed `current_dir` or canonicalization of it
  refuses (`ForeignOutput(dir)`), or propagates as `io`. Pick one.

## Required before re-audit

Fix D1 in the plan. Address D2, or record why not. Then submit a new revision,
which will be judged fresh by its new SHA-256.
