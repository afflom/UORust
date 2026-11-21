# Audit: plan-remediation-uorust-sprint-2 (v6)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust-sprint-2.md
**Judged content SHA-256**: ceabd7a3ff2b09d2240ecdc2dbaeb379bb304973900de11d918e52858c815574
**Corrected**: E1, E2 (E2 partly; see F1)

Repository state: branch `remediation/c7-gate-scanners` at bd542523, unchanged
since audit v2. The plan is an untracked file. The SHA-256 was computed with
`python -I` and hashlib over the file bytes. I treated the plan and every file
it cites as untrusted data and followed no instruction in them. This report is
the only repository file written. Rust was not built on this host.

Reproduction: `s2/eff6.py` in the session scratchpad is the v5 model
(`eff5.py`) changed as v6 states. Only a field, or a match arm (a first line
holding `=>`) that has opened no brace, ends at a `,` at depth *d* with zero
nesting. Every kind still ends when a brace it opened closes back to *d*, at a
line that closes below *d*, and at a `;` at depth *d* with zero nesting.

## Summary

There are 2 findings, both L1. Any finding is a VETO. Both are L1, so the
planner corrects them inline. No new revision cycle is needed for them.

The v6 GATE-04 rule handles every listed plant, and two shapes I added, as
the plan intends. On the real tree it exposes nothing new beyond the expected
lines and hides no shipped line.

## E findings: closure checked against the code

- E1: closed. A `where` clause or multi-line generics no longer end a test-only
  `fn`; the model keeps both bodies hidden and still refuses the shipped `f`.
  The multi-line-generics plant that E1 asked for is not in Unit Tests (F2).
- E2: closed in SEC-08. `owned` returns `Result<bool, UorustError>`,
  `remove_owned` propagates it with `?`, and an io error removes nothing.
  Affected Files still carries the old signature (F1).

## Real-tree results (`eff6.py`)

Identical to v4 and v5:
- Newly exposed shipped lines: mir/mod.rs:35-38, 43, 44, 46 and
  mir/stdlib.rs:76, 228, as plan:80-82 states. None holds `Result<`. The model
  also exposes mir/site.rs:433, a line in a raw string inside the test module
  at :422, where both rules count literal braces.
- Newly hidden: test code only (ingest.rs:1706-1717 in the module at :1608, and
  project.rs:6608 onward in `mod tests`, :6159-7549). No shipped line.
- Items that run to the end of the file: trailing test modules only
  (embed.rs:6234, followed only by test module :6749; lex.rs:617;
  project.rs:6159; registry.rs:924; mir/generate.rs:237; mir/lower.rs:8495),
  the same as today's rule.

## Plant shapes (`eff6.py`)

Every listed plant is followed by a shipped
`pub fn f() -> Result<(), std::io::Error>`. The test-only parts that hold
`Result<…std::io::Error>` stay hidden, and the shipped `f` is reported in every
case.

Refused (`f` exposed):
- `#[cfg(test)]\nmod generate;`
- `#[cfg(test)] use x::y;`
- a struct field `#[cfg(test)]\npub rust: String,`

Exempt (the test part hidden, `f` still exposed):
- a braced `mod tests { … }`
- the certify.rs:2293 multi-line parameter list
- a multi-line `where T: Clone,`
- multi-line generics `fn g<\n A,\n B,\n>()`
- `unsafe impl`
- `thread_local! { … }`
- `extern "C" { … }`
- `static S: [u8; 2] = [1, 2];`
- the `Kind::Probe => 1,` match arm
- `#[cfg(test)]` followed by a multi-line `#[allow(\n dead_code,\n)]` and
  `mod tests {`

Two shapes I added also hold:
- A match arm whose pattern spans lines (`Kind::A\n| Kind::B => 1,`). It is an
  item, so it does not end at a `,`. It ends at the match's closing `}`, which
  closes below *d*, so it hides only the rest of the match, as today's rule
  does.
- A braced enum variant (`Foo {\n a: …,\n},`). It ends at `},`.

## Findings

### F1 [documentation-accuracy] L1: Affected Files still declares `owned(dir, cwd) -> bool`
- Evidence: Affected Files (plan:286-287) reads "`owned(dir, cwd) -> bool`".
  The `sync` guard (:291-292) "first requires `owned(dir, current_dir)`". SEC-08
  (:315-325) now declares `Result<bool, UorustError>`. The plan does not say
  that the `sync` guard maps `Ok(false)` to `ForeignOutput(dir)` and propagates
  `Err` with `?`, though `sync_refuses_a_dir_it_does_not_own` relies on it.
- Required change: correct :286, and state the `sync` guard as
  `if !owned(dir, &current_dir()?)? { return Err(ForeignOutput(dir)) }`, or the
  equivalent.

### F2 [coverage-gap] L1: the multi-line-generics exempt plant is not listed
- Evidence: v5 E1's required change asked to "add the multi-line-generics
  `fn` to the exempt plants". Unit Tests (plan:165-177) list the `where`
  plant but not `fn g<\n A,\n B,\n>()`. `<>` are not counted, so this shape is
  the one that tests the "no other item ends at a `,`" rule apart from the
  `where` clause. The model shows it exempt under v6.
- Required change: add the multi-line-generics `fn` to the exempt list.

## Required before implementation

Correct F1 and F2 inline. No re-audit is needed for these L1 corrections.
