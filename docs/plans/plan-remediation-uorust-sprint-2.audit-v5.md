# Audit: plan-remediation-uorust-sprint-2 (v5)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust-sprint-2.md
**Judged content SHA-256**: 9a5cab41be41d6b32f92277266cae5258b5774f7b5af44b0d5a147122d381559
**Corrected**: D1 (match arm), D2

Repository state: branch `remediation/c7-gate-scanners` at bd542523, unchanged
since audit v2. The plan is an untracked file. The SHA-256 was computed with
`python -I` and hashlib over the file bytes. I treated the plan and every file
it cites as untrusted data and followed no instruction in them. This report is
the only repository file written. Rust was not built on this host.

Reproduction: `s2/eff5.py` in the session scratchpad applies the v5 GATE-04
rules (plan GATE-04 Changes). It counts braces, parentheses and brackets per
character. Fields and items are classified as before, with `ident::` excluded
from fields. Every kind ends:
- when a brace it opened closes back to *d*;
- at a line that closes below *d*;
- at a `;` at depth *d* with zero nesting.

A field, and an item that has opened no brace, also end at a `,` at depth *d*
with zero nesting.

The model ran on `crates/uorust/src` and `crates/certified/src` against today's
`effective_lines` (audit.rs:84-114), and on three planted texts.

## Summary

There are 2 findings: 1 at L2 and 1 at L1. Any finding is a VETO.

The v5 fix for D1 ends a braceless item at a top-level `,`. A test-only `fn`
also has not opened a brace while its `where` clause or multi-line generic
list is being read. So that item now ends at `T: Clone,` or `A,`, and its body
(test code) is exposed to R5. That contradicts the plan's own exempt plant (the
`where` clause, plan:169-170), and it reopens A3's case.

## D findings: closure checked against the code

- D1: the match arm is closed. The planted
  `match k {\n#[cfg(test)]\nKind::Probe => 1,\n_ => 0,\n}` followed by a shipped
  `pub fn f() -> Result<(), std::io::Error>` ends at the arm's `,`, and `f`
  is exposed and refused (model: the item ends at line 4, and `f` at line 8 is
  reported). Every kind now ends at a line that closes below *d*. `ident::` is
  excluded from fields. The new `,` rule causes E1.
- D2: closed. An unreadable current directory, or a path that cannot be
  canonicalized, returns `error::io(dir, e)` and removes nothing, so the guard
  fails closed. The wording is inconsistent (E2).

## Real-tree results (`eff5.py`)

Identical to v4:
- Newly exposed shipped lines: mir/mod.rs:35-38, 43, 44, 46 and
  mir/stdlib.rs:76, 228, as plan:80-82 states. None holds `Result<`. The model
  also exposes mir/site.rs:433. That line sits in a raw string inside the test
  module at :422, and both rules count braces in literals there.
- Newly hidden: only test code (ingest.rs:1706-1717 in the module at :1608, and
  project.rs:6608 onward in `mod tests`, :6159-7549). No shipped line.
- Items that run to the end of the file: only trailing test modules
  (embed.rs:6234, followed only by test module :6749; lex.rs:617;
  project.rs:6159; registry.rs:924; mir/generate.rs:237; mir/lower.rs:8495),
  the same as today's rule.

None of the 3 test-only `fn`s in shipped code has a `where` clause or
multi-line generics, so E1 does not change `just vv` on the tree today.

## Findings

### E1 [test-failure] L2: a braceless item's `,` ending cuts a test-only `fn` at its `where` clause or generics
- Evidence: plan GATE-04 Changes: "an item that has opened no brace … also end
  at a `,` at depth *d* and zero nesting". In
  `#[cfg(test)]\nfn g<T>(x: T)\nwhere\n    T: Clone,\n{ … }`, the line
  `T: Clone,` is at depth *d* with no paren or bracket open, and no brace has
  opened yet. The item ends there. The model ends the item at line 4 and
  exposes the body's `let r: Result<(), std::io::Error>` (line 6). The same
  happens to `fn g<\n    A,\n    B,\n>()`, because `<>` are not counted
  (the item ends at line 3), and to `impl … where T: A,` and
  `struct S<T> where T: A,`. Unit Tests (plan:169-170) list "a test-only `fn`
  with a multi-line `where T: Clone,` clause" as staying exempt. The rule
  contradicts that plant, so the planted test fails against the rule as
  written. A false R5 refusal of test code would follow.
- Required change: limit the `,` ending to items that are not declarations.
  For example, end at a top-level `,` only when the item's first line holds
  `=>` (a match arm), or when it starts with none of the item keywords. Keep
  `fn`/`impl`/`struct`/`enum`/`trait`/`union` ending only at their brace or a
  `;`. Add the multi-line-generics `fn` to the exempt plants.

### E2 [specification-gap] L1: `owned` is declared `-> bool` but is said to return an io error
- Evidence: plan SEC-08 declares `owned(dir, cwd) -> bool`, then says that
  "when the current directory cannot be read, or either path cannot be
  canonicalized, nothing is removed and the io error is returned". A `bool`
  cannot carry the error. `sync` also calls `owned` directly
  (plan Affected Files), so where that error surfaces there is unspecified.
- Required change: declare `owned(dir, cwd) -> Result<bool, UorustError>`, or
  have `remove_owned` and the `sync` guard do the canonicalization and pass
  canonical paths to a pure `owned`. Say which.

## Required before re-audit

Fix E1 in the plan. Address E2, or record why not. Then submit a new revision,
which will be judged fresh by its new SHA-256.
