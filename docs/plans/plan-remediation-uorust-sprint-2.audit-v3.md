# Audit: plan-remediation-uorust-sprint-2 (v3)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust-sprint-2.md
**Judged content SHA-256**: 3081d1dc9153155003a98140a0c9399398843640211f1353fc4ad06fb2eeb021
**Corrected**: B1, B2, B3, B4, B5, B6, B8. B7 is corrected in intent, but its guard introduces C2.

Repository state: branch `remediation/c7-gate-scanners` at bd542523, unchanged
since audit v2. The plan is an untracked file. The SHA-256 was computed with
`python -I` and hashlib over the file bytes. I treated the plan and every file
it cites as untrusted data and followed no instruction in them. This report is
the only repository file written. Rust was not built on this host.

## Summary

There are 4 findings: 2 at L2 and 2 at L1. Any finding is a VETO.

- C1 (L2): the v3 GATE-04 rule turns a module-level brace-block macro item, and
  an `extern "…" {}` block, into a field. A field never ends at its closing
  brace, so the scan hides the shipped code after it. Today's rule ends at that
  brace (audit.rs:109-111), so this is new fail-open behaviour.
- C2 (L2): the new B7 guard runs `owned(dir, cwd)` first in `sync`. Two
  existing `sync` tests pass a `dir` that does not exist yet, and `owned` does
  not say what an absent directory means.

## v2 findings: closure checked against the code

- B1: closed. ratchet-staged.sh:10-11 becomes `rm -rf "$out" && mkdir -p "$out"`,
  run after `cd "$root"` (:9), so no earlier manifest survives.
- B2: closed.
  - `keep` computes `inputs(root)` before it touches anything. `install`
    receives the inputs text and a `confirm` callback.
  - `confirm: impl Fn(&Path) -> Result<String, Fail>` matches
    `check(root: &Path) -> Result<String, Fail>` (proof.rs:303).
  - Both tests now reach their steps. The copy failure happens in step 2. The
    restore runs in step 4 with a failing `confirm`. A control with a
    succeeding `confirm` covers removing `.old`.
- B3: closed in substance. A masked κ change is exported on `main`. The
  wording is off (C3).
- B4: closed for continuation lines, depth *d* for fields, and `unsafe impl`
  (with a plant). The qualifier stripping that answers B4 brings in C1 and C4.
- B5: closed. Comment and blank lines continue a run, with two plants. The v2
  model `s2/scan2.py`, which already treats comments as transparent and handles
  multi-line attributes, yields 333 names. All 29 registered IDs still match.
- B6: closed. plan:78-80 now matches the v2 model: mod.rs:35-38, 43, 44, 46 and
  stdlib.rs:76, 228.
- B7: the guard now covers `sync`'s `--dir`, and
  `sync_refuses_a_dir_it_does_not_own` is planted. See C2.
- B8: closed. The merge test is in `crates/uorust/tests/cli.rs`, run by
  `cargo test -p uorust --test cli`.

Checked and hold:
- Every `#[cfg(test)]` in the shipped sources is followed by `mod` (39), `fn`
  (3) or a field (4). None is followed by a macro item, an `extern` block, or a
  `const` or `static` item. So C1 and C4 do not change what `just vv` does on
  the tree today.
- certified.yml:128 runs `just certified-project && just certified-export`.
  :85 runs `just certified-plan`.

## Findings

### C1 [specification-drift] L2: a brace-block macro item or `extern "…" {}` block under `#[cfg(test)]` hides the shipped code after it
- Evidence: plan:118-128. After stripping qualifiers, an item is braced only
  when it starts with `fn`, `impl`, `struct`, `enum`, `union`, `trait`, `mod`,
  `extern` or `macro_rules!`. Anything else is a field. A field ends only at a
  `,` or `;` at depth *d* with zero nesting, or at a line that closes below *d*.
  - `#[cfg(test)]\nthread_local! {\n    static X: u8 = 0;\n}` (and any
    `name! { … }` item) is a field. Its inner `;` is at depth *d*+1, and its
    closing `}` returns to *d* without going below. The item never ends.
    Everything after it stays hidden until the next `;` or `,` at depth *d*,
    for example a later shipped `pub fn f() -> Result<(), std::io::Error> { … }`.
  - Because `extern "…"` is stripped as a qualifier, `#[cfg(test)]\nextern "C" {`
    leaves `{`, which is classified as a field, with the same result.
  Today's rule ends either item at its closing brace (audit.rs:108-111). v3 is
  therefore weaker than today for these shapes. No such shape exists in the tree
  today.
- Required change: end any item, braced or field, when a brace it opened at
  zero paren/bracket nesting closes back to *d*. That still lets
  `vectors: row.map(|v| { … }).collect(),` end at its `,`, because that brace
  opens inside parentheses. Strip `extern "…"` only when `fn` follows it. Add
  plants for a multi-line `thread_local! { … }` and an `extern "C" { … }` block
  under `#[cfg(test)]`, each followed by a shipped `f` that must still be
  refused.

### C2 [test-failure] L2: the `sync` guard refuses a `--dir` that does not exist yet, which existing tests pass
- Evidence: plan:280-282 and :329-332 make `owned(dir, current_dir)` the first
  statement of `sync`. `owned` (plan:304-306) compares canonical paths.
  `canonicalize` fails on a path that does not exist, and the plan defines no
  absent case for `owned`; only `remove_owned` returns `Ok` when the directory is
  absent. `sync` creates its directory itself (certify.rs:2934). The existing
  tests call `sync` with fresh, absent directories: `tmp.join("certified")`
  (certify.rs:4931-4945) and `tmp.join("crate")` (:5633-5645). If absent means
  "not owned", those tests fail and `just vv` goes red. If `owned` propagates the
  canonicalize error, `sync` errors. A first sync into a new `--dir` breaks the
  same way.
- Required change: define `owned` for an absent `dir` as owned, since an absent
  path cannot hold `.git` or be an ancestor of the current directory. Do the
  same if canonicalizing the current directory fails. Name certify.rs:4931 and
  :5633 among the tests that must pass unchanged, and add an absent-`dir` case
  to `owned_refuses_a_checkout_and_the_working_directory`.

### C3 [documentation-accuracy] L1: Q1 says `certified.yml` runs on every push to `main`
- Evidence: Q1 (plan:39-40) says "`certified.yml` runs on every push to `main`
  (`certified.yml:22-23`) and exports it there (:85, :128)". The trigger is
  filtered by path (certified.yml:23-31). The masked case always edits
  `crates/uorust/**`, so it is still covered. :85 is the plan step
  (`just certified-plan`); the export itself runs at :128.
- Required change: say "on every push to `main` that changes
  `crates/uorust/**` (certified.yml:21-31)", and cite :128 for the export.

### C4 [specification-gap] L1: `const` is both a stripped qualifier and an item kind
- Evidence: plan:119-120 strips `const` as a qualifier in any order, and
  plan:129-130 lists `const` items as ending at their `;`. Stripping
  `const X: T = …;` leaves `X: T = …;`, which is a field under :127-128. That
  gives the same result for a plain `const`. But it means the item-kind rule at
  :129 is never reached for `const`, and `const _: () = { … };` passes through
  the field path, where C1 applies. `static` is never stripped and is not in the
  braced list, so it too is a field.
- Required change: strip `const`, `async`, `unsafe` and `default` only when
  `fn`, `impl` or `trait` follows. Classify `const`, `static`, `type` and `use`
  before stripping.

## Required before re-audit

Fix C1 and C2 in the plan. Address C3 and C4, or record why not. Then submit a
new revision, which will be judged fresh by its new SHA-256.
