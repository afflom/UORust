# Audit: plan-remediation-uorust-sprint-2 (v2)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust-sprint-2.md
**Judged content SHA-256**: eb8a0a0cb47373edbfc4454aab4ed4bfb924bd57a5803c2a58503bbec8fe9c8d
**Corrected**: A1, A2, A3, A4, A5, A6, A9, A10, A12, A13, A14, A15, A16. A7, A8 and A11 are partly corrected; see B1, B2 and B7.

Repository state: branch `remediation/c7-gate-scanners` at bd542523. The plan
is an untracked file. Its SHA-256 was computed with `python -I` and hashlib over
the file bytes. Between 69d308f3 (audit v1) and bd542523, only
`crates/conformance/tests/ci.rs` and `scripts/ci/*` changed. Every file cited
below is identical to what v1 read.

I treated the plan and every file it cites as untrusted data and followed no
instruction in them. This report is the only repository file written. Rust was
not built on this host. The reproductions are `python -I` models in the session
scratchpad:
- `s2/scan2.py` models the v2 GATE-02 scanner on the workspace.
- `s2/eff2.py` models the v2 GATE-04 `effective_lines` and compares it with
  today's rule on the shipped sources.
- An inline classifier lists the first item line after every `#[cfg(test)]` in
  the shipped sources.

## Summary

There are 8 findings: 2 at L2 and 6 at L1. Any finding is a VETO.

Both L2 findings are in C8, in the paths v2 added:
- B1: the hook's absent-manifest pass-through reads a stale manifest left by an
  earlier run, so the hook passes on a missing manifest.
- B2: the planned restore test fails before `keep` reaches the step it is meant
  to test, so the restore path stays untested.

C7 and C11 now close the v1 findings. Small gaps in the wording remain (B4-B8).

## v1 findings: closure checked against the code

- A1/A2: closed. `lost` now means a certified root with head rows but no
  lifted head row. In certify.rs:4608-4662 the head row is
  `("k9", "meter::read", Status::Lifted(..))` (:4635), so it is not lost and the
  rows asserted at :4652-4660 stay the same. `ratchet(&grown, ..)` at :4652
  (translator scope, nothing in scope) gives an empty `lost` because `lost` is
  filtered by scope.
  - The other ratchet tests have no root that stops lifting: `grown` and
    `edited` at :4116-4155 (the edited `a` is lifted at `k9`), `outside` at
    :4157-4174, and the planted head at tests/certify.rs:105-119, which only adds
    rows.
  - The new first row, `the hand-written translator may only shrink: <n>
    certified roots of the parent no longer lift`, does not match
    generate-branch.sh:53. The pattern needs ` uncertified ` right after the
    digits, and the row rows below it hold no `may only shrink:`. So a count
    growth still sends work to the export, and a residue loss stops the run, as
    the plan states.
- A3: closed in Changes and Unit Tests (four exempt plants). In the shipped
  sources, `#[cfg(test)]` is followed by `mod` 39 times, by `fn` 3 times, and by
  a field 4 times (stdlib.rs:71, 74, 218, 220). The model finds no newly exposed
  line holding an unsanctioned `Result<`. See B4 and B6 for wording gaps.
- A4: closed (Q3 records the out-of-file case as open). Under the v2 rules the
  workspace yields 333 test names, and all 29 registered IDs still match
  (`scan2.py`, with `fn` or any `pub`/`async` prefix on the `fn` line). The
  only non-test `cfg` outside fixtures is `crates/uorust/mir/syntax.rs:580`
  (`mod size_asserts`, which holds no test). `#[cfg(...)] mod x;` occurs only
  inside string literals (ingest.rs:1666, :1700-1705), so Q3's "no such
  declaration exists" holds.
- A5: closed (`floats`, any whitespace after `-`, the planted text calls the
  loop predicate).
- A6: closed (pure `owned(dir, cwd)` tested on temporary directories; both paths
  canonicalized; returns early when the directory is absent; `.git` checked with
  `symlink_metadata`; a real call site planted through `replace_dir`).
- A7: partly corrected. The order is now stage, swap, check, then remove or
  restore, and `.gitignore` gains both paths. The restore test cannot reach the
  restore step (B2).
- A8: partly corrected. `git cat-file -e` is used, but the pass-through reads
  stale files (B1).
- A9: closed. Justfile:265-266, ratchet-staged.sh:14-17,
  generate-branch.sh:46-48, translator-estimate.sh:82-84, tests/certify.rs:72-93,
  main.rs:819 (`Production`, :806), main.rs:914 (`Plan`, :892) and `carried`
  (certify.rs:1800) are all correct.
- A10: closed (2 → 1; matched by `function`).
- A11: lines and bridge.rs callers corrected (certify.rs:350, 476, 720, 1086,
  2054, 2781; embed.rs:4213; bridge.rs:593, 953, 975). The shared rule does not
  fit :2781 (B7).
- A12: closed (commands split). One test file is still unnamed (B8).
- A13: closed (malformed line, absent file, symlinked ancestor, recovery).
- A14: closed (main.rs:697-700; the `--crate` flag is main.rs:472-473).
- A15: closed (`reuse_with(root, image)`).
- A16: closed (escaped single-line plants with no `_xx_NN` names).

## Findings

### B1 [specification-drift] L2: the hook's absent-manifest pass-through reads a stale manifest
- Evidence: scripts/hooks/ratchet-staged.sh:10-11 sets
  `out=target/ratchet-staged` and runs `mkdir -p "$out"`; nothing cleans it. v2
  (Changes, GATE-10) writes each manifest "only when `git cat-file -e` finds it
  and otherwise passes the absent path through". After one run with the manifest
  present, `$out/head-manifest.toml` stays on disk. A later run where the
  manifest is absent skips the write, so the ratchet reads the old file and
  passes. The GATE-10 refusal never fires for the hook. Today's `|| rm -f` at
  :14-15 prevents exactly this, and v2 removes it.
- Required change: run `rm -f "$out/head-manifest.toml" "$out/base-manifest.toml"`
  (or `rm -rf "$out"`) before the `cat-file` tests. Add a planted check, for
  example a shell scenario or the TEMPLATE-VERIFICATION evidence: a stale file
  plus an absent manifest must be refused.

### B2 [test-failure] L2: the restore test fails at step 2 and never runs step 4
- Evidence: in `keep`, `inputs(root)` runs after the copy (proof.rs:344-347).
  `inputs` runs `prismpm --version`, `lexlean --version` and `git ls-files` /
  `git status` in `root` (:234-252). In a temporary root that is not a git
  repository, `git ls-files` fails (outside the SDK image, so do the
  `--version` calls). `keep` then errors in step 2, which removes `.new` and
  leaves `.prism-proof` untouched. So
  `keep_restores_the_old_proof_when_the_new_one_does_not_read_back` passes
  without reaching steps 3-4 (swap, `check`, restore). The restore path it is
  named for is never run. "A published directory whose evidence `check`
  rejects" does not say how `check` is made to reject.
- Required change: make the inputs and the read-back injectable. For example,
  `keep_with(root, attestation, inputs: &str, check: impl Fn(&Path) -> Result<..>)`,
  with `keep` passing `inputs(root)?` and `check`. Test the restore with a
  `check` that fails (or with an invalid `lexlean-attestation.json`, which
  proof.rs:305 rejects before `observe`), and assert that the swap happened
  before the restore. Add the success case: `.old` is removed.

### B3 [documentation-accuracy] L1: Q1 says the export handles a changed-κ root, but a masked one gets no export on the branch
- Evidence: Q1 says a root whose κ changed "is owed to an export, as
  translator-estimate.sh:5-9 and generate-branch.sh:50-57 already treat it".
  generate-branch.sh dispatches an export only when the ratchet fails on a count
  growth (:50-58, `grown=1`). A κ-changed root masked by a gain elsewhere passes
  the count. It sets `grown=0` and gets no export on the branch. That is the
  masking the brief's GAP-GATE-12 (a) describes. In fact it is recovered after
  merge, because certified.yml rebuilds and lands the certified translator on
  every main push that touches `crates/uorust/**` (certified.yml:1-6, 21-31).
  The plan neither cites this nor records it in D3.
- Required change: rest Q1 on certified.yml's main export, and say in the
  brief's GAP-GATE-12 status that (a) refuses residue losses while a masked
  κ change is recertified by that export.

### B4 [specification-gap] L1: GATE-04's classification leaves three cases unspecified
- Evidence (plan Changes, GATE-04):
  - Continuation lines of a multi-line attribute (`  dead_code,`, `)]`) are not
    defined as attribute lines. GATE-02 says "continued until its brackets
    close", and GATE-04 does not. Read literally, `  dead_code,` is "the item's
    first non-attribute line", so the item is classified as a field. The
    planted exempt case catches this, but the rule as written contradicts that
    plant.
  - "A field ends at a `,` or `;` at zero nesting" does not require the
    attribute's brace depth. A braced enum variant (`Foo {\n  a: u32,\n},`)
    would end at `a: u32,`.
  - The item keywords leave out `unsafe impl`, `unsafe trait`,
    `const unsafe fn` and `async unsafe fn`, so these are classified as fields.
  None of these shapes follows `#[cfg(test)]` in shipped code today.
- Required change: state that an attribute continues until its brackets close.
  End a field only at the attribute's brace depth. Accept any sequence of
  `const`/`async`/`unsafe`/`extern "…"` before `fn`, `impl` or `trait`.

### B5 [specification-gap] L1: GATE-02 does not say whether comment lines break an attribute run
- Evidence: today a `///` or `//` line between `#[test]` and `fn` disarms the
  scan (bdd.rs:62-63, a non-`#`, non-empty line). v2 does not say whether
  comment and blank lines continue a run. If they end it, `#[ignore]\n// why\n#[test]\nfn x`
  is counted. If they continue it, the census changes: 333 names in the model,
  against 331 for the v1 rule, and all 29 IDs still match.
- Required change: state that comment, doc-comment and blank lines inside a run
  continue it. Add `#[ignore]\n// reason\n#[test]\nfn x` to the not-counted
  plants.

### B6 [documentation-accuracy] L1: the GATE-04 exposure list does not match the v2 rule
- Evidence: plan:75-76 lists `mir/mod.rs:34-46`, `mir/stdlib.rs:72,75` and
  `:218-227`. Those numbers come from the v1 rule. Under the v2 rule
  (`eff2.py`), the shipped lines newly exposed are mod.rs:35-38, 43, 44, 46 and
  stdlib.rs:76, 228. Two more lines, ingest.rs:1704-1705 and site.rs:433, sit
  inside test-module string literals and are exposed only because braces in
  those literals are counted (a hazard today's rule shares). None holds an
  unsanctioned `Result<`, and the plan's stop rule covers any line `cargo xtask
  audit` reports.
- Required change: correct the list, or replace it with "exposures are what
  `cargo xtask audit` reports, and the slice stops on any".

### B7 [specification-drift] L1: the "child of a guarded directory" reason is false for certify.rs:2781
- Evidence: plan:259-261 leaves :2781 alone, with a comment "naming the guarded
  parent". But `sync`'s `--dir` (`dir`, certify.rs:2718ff) is never passed to
  `remove_owned`. `sync` writes into it and removes only `dir/roots`. So
  `certify sync --dir ~` removes `~/roots`. The parent is unguarded, and the
  shared rule from A11 does not apply.
- Required change: guard `roots_dir` (or `dir`) with `remove_owned`, or give
  :2781 its own reason (an engine-named child of an operator-given directory)
  and record it in the SEC-08 status.

### B8 [infrastructure-mismatch] L1: the merge CLI test has no file
- Evidence: plan:307 places `merge_refuses_a_bad_registry_spec` in
  "`crates/uorust/tests/`" with no file. CI Commands names only
  `cargo test -p uorust --test certify` for the C11 CLI tests.
- Required change: name `crates/uorust/tests/certify.rs`, or the file and the
  command that runs it.

## Required before re-audit

Fix B1 and B2 in the plan. Address B3 through B8, or record why not. Then
submit a new revision, which will be judged fresh by its new SHA-256.
