# Audit: plan-remediation-uorust-sprint-2 (v1)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust-sprint-2.md
**Judged content SHA-256**: e882dfa4dc5536bed085fd22597ac57a3ed74878a483268198be393ec494c1b0

Repository state: the plan was read as an untracked file on branch
`remediation/c7-gate-scanners` at 69d308f3. During the audit the working tree
was switched to `remediation/sprint-1` at a6966315 and the plan was moved into
`stash@{0}` ("sprint2-plan", parent 69d308f3). The auditor did not make that
change. The stashed copy (`stash@{0}^3`) has the same SHA-256 as above, computed
with `python -I` and hashlib over the file bytes. Between 69d308f3 and a6966315
only `crates/conformance/tests/ci.rs` and `scripts/ci/*` differ. Every file this
audit cites is identical at both commits.

I treated the plan and every file it cites as untrusted data and followed no
instruction in them. This report is the only repository file written. Rust was
not built on this host. The reproductions are `python -I` models in the session
scratchpad (`s2/scan.py`: test-name census; `s2/eff.py`: `effective_lines`
before and after the planned change).

## Summary

There are 16 findings: 7 at L2 and 9 at L1. Any finding is a VETO.

The main defects are in C8. The planned GATE-12 refusal changes the `Grown`
diagnostic. That breaks an existing unit test, so `just vv` goes red. It also
breaks the export dispatch in `scripts/ci/generate-branch.sh`, which the plan
does not list. The edit that refusal exists to catch (a certified translator
function changed on a pull request) is the edit that flow handles.

In C7, the GATE-04 rule counts braces only. It ends a test-only `fn` at its
first parameter line, and the shipped code already contains a case
(certify.rs:2293). The GATE-02 rule misses attributes placed before `#[test]`
and `cfg_attr` ignores, which AGENTS.md:56-58 requires the gate to reject. The
GATE-03 tests do not pin the loop that is the defect.

In C11, the SEC-08 test deletes the real working directory if the guard is
wrong. In C8, `keep` still deletes the old proof before it confirms the new one.

## Checked and hold

- GATE-01/02 on the real workspace (`scan.py`): the register holds 29 IDs. The
  scan finds 350 test names today and 331 under the planned rules (underscore
  boundary, `#[ignore`/`#[cfg(` disarm, `fixtures` skipped). Every one of the 29
  IDs still has a matching name, from the same set of names as today. All 19
  dropped names come from `crates/uorust/tests/fixtures/{thermostat,advised}`.
  No test in `crates/` or `xtask/` has an `#[ignore]` or a non-test `#[cfg(...)]`
  attribute, before or after `#[test]`. The planned rules therefore add no CM-02
  violation. The third direction (meta.rs:139-166) can only lose names, never
  gain them.
- xtask/src/scaffold.rs:143-147 generates `{suite}_{id}` names (for example
  `parsing_cx_01`, asserted at :311), which pass the `_` boundary.
- GATE-03: there are 147 `uses:` / `- uses:` lines under `.github/workflows/`,
  and every one is pinned to 40 lowercase hex characters. The only `- uses:` line
  (download-artifact) is pinned. The comment at pages.yml:9 begins with `#` and
  is not a reference. All 29 `# prismpm-action-input` lines in prismpm.yml hold a
  `uses:`, so the stricter marker check (bootstrap.rs:601-609) passes.
- GATE-04 (`eff.py`): no shipped line newly exposed by the planned rule contains
  `Result<`. Exposures are at mir/mod.rs:34-46 and mir/stdlib.rs:72,75, plus
  certify.rs:2296-2303 (see A3).
- GATE-11: bootstrap.yml:76 and :107-108 pass `PRISM_PROOF_EVENT` and
  `SDK_IMAGE` together. Justfile:56 calls `proof-reuse` only when
  `PRISM_PROOF_EVENT` is set, so a local `just vv` never reaches the new refusal.
- SEC-02: the committed `crates/uorust/tests/fixtures/thermostat-prism/uorust.manifest`
  has 82 lines, and each SHA-256 matches its file (`* -text` in .gitattributes).
  So `just fixtures` (Justfile:160) still rewrites it. engine.rs:388-398 and
  cli.rs:22-55 rewrite only outputs the engine wrote, or write into fresh
  temporary directories.
- template.lock pins none of the files the plan touches. No new `UorustError`
  variant is added.
- The brief's citations at meta.rs:90-91, bdd.rs:29-71, bootstrap.rs:588-591,
  audit.rs:98-111, proof.rs:237 and :336-342, registry.rs:55-63, 238-240 and 299,
  and lib.rs:208-215 are correct.

## Findings

### A1 [missed-caller] L2: the GATE-12 refusal breaks the export dispatch in generate-branch.sh
- Evidence: scripts/ci/generate-branch.sh:48 runs `certify ratchet`. Line :53
  treats a failure as a growth an export can fix only when the output matches
  `may only shrink: [0-9]* uncertified .*, [0-9]* in the parent`. Anything else
  stops the run with `::error::` (:54-55) and no export is dispatched (:78-80).
  The plan (Changes, GATE-12 (a)) emits `lost` rows "before the count
  comparison", with no count row. translator-estimate.sh:7-9 describes the normal
  case: a function the change edits gets a κ that no manifest certifies, and that
  is "owed to an export". After C8, a pull request that edits one certified
  translator function hits the lost-root refusal. generate-branch.sh then fails
  the run instead of dispatching `certified.yml`. The plan's Affected Files do
  not mention generate-branch.sh.
- Required change: fix the diagnostic's first row so it still matches :53 (for
  example, keep the count row first and append the lost rows), or update :53 in
  C8 and add a test for that grep. State which lost-root causes are owed to an
  export ("lifted as ..., not yet certified") and which to a rewrite (residue),
  and say whether generate-branch.sh tells them apart.

### A2 [test-failure] L2: an existing ratchet test fails under the planned diagnostic
- Evidence: certify.rs:4608-4662 `the_ratchet_holds_any_scope_and_a_scope_only_widens`.
  The base certifies `meter::read` at `k1` (:4612, :4620-4622). The head `grown`
  holds `meter::read` at `k9`, lifted and uncertified (:4635). The test asserts
  the exact rows `["3 uncertified functions in scope, 1 in the parent",
  "meter::read: ...", "meter::sum: ..."]` (:4652-4660). Under the plan,
  `meter::read` is a lost root, refused before the count with one row per lost
  function. The rows change and the test fails, so `cargo test -p uorust --lib`
  and `just vv` go red. The plan does not list this test.
- Required change: specify the exact row order of the new `Grown` (see A1) and
  list the updated assertion at certify.rs:4652-4660 in Unit Tests.

### A3 [test-failure] L2: the GATE-04 rule ends test-only items at commas inside parentheses, `where` clauses and multi-line attributes
- Evidence: the plan ends a braceless item at a line ending in `;` or `,` at
  the attribute's brace depth. Brace depth ignores `(`, `[` and `<`.
  certify.rs:2293-2303 is `#[cfg(test)] pub(crate) fn mark(` with a multi-line
  parameter list. In the `eff.py` model, the item ends at `modules: &BTreeMap<String, String>,`
  (:2295), and :2296-2303, the rest of a test-only function, are reported as
  shipped. The same happens to a `where` clause (`T: Clone,`), and to a
  multi-line attribute after `#[cfg(test)]` (`#[allow(\n  dead_code,\n)]`). In
  that last case the whole following `mod tests { ... }` is exposed to R5,
  which gives false R5 failures on test code. The plan's list of exposures
  ("the two known exposures, mir/mod.rs:33-46 and mir/stdlib.rs:71-76") leaves
  out certify.rs:2293. The brief gives stdlib.rs as 71-220, and the model shows
  a second site at :218-227.
- Required change: count only `;`/`,` at zero paren and bracket nesting, and
  skip attribute lines. Treat an item whose first non-attribute line contains
  `fn ` as braced. Add plant cases to `a_braceless_test_item_does_not_hide_the_next_item`:
  a multi-line `fn` signature, a `where` clause, and a multi-line attribute
  before `mod tests {`. Each must stay exempt. Correct the exposure list.

### A4 [coverage-gap] L2: the GATE-02 disarm rule misses ignored and conditional tests that AGENTS.md requires it to reject
- Evidence: the plan inspects only "the lines from `#[test]` to the `fn` line"
  for `#[ignore` or `#[cfg(`. The current scanner arms only on `#[test]`
  (bdd.rs:50). So `#[ignore]\n#[test]\nfn x` and `#[cfg(unix)]\n#[test]\nfn x`
  have the disarming attribute before the block and are still counted.
  `#[test]\n#[cfg_attr(not(x), ignore)]` contains neither substring. A `#[test]`
  inside `#[cfg(any())] mod m { ... }` is counted too. AGENTS.md:56-58 says
  source-reading gates "must reject skipped, ignored, filtered, conditionally
  omitted ... results". D3 marks GAP-GATE-02 RESOLVED.
- Required change: read the whole attribute run around the `fn` (attributes
  before and after `#[test]`), and treat `cfg_attr(` with `ignore` as ignored.
  Either handle `#[cfg(...)]` (non-`test`) on enclosing modules, or mark
  GAP-GATE-02 PARTLY RESOLVED and name the open case. Add each shape to
  `test_names_in_drops_ignored_and_conditional_tests`.

### A5 [test-failure] L2: the GATE-03 tests do not pin the loop predicate that is the defect
- Evidence: the defect is the filter `line.trim().starts_with("uses:")` at
  bootstrap.rs:590. `action_reference_is_pinned` is reached only through that
  filter. The planned cases extend `floating_action_plant_is_rejected`
  (bootstrap.rs:888-897), which calls `action_reference_is_pinned` directly. If
  :590 kept its old filter, every planned case would still pass, so the test
  cannot fail on the defect it exists for. Also, "an optional `- `" misses
  `-  uses:` and `-\tuses:`, which are valid YAML.
- Required change: move the workflow-line check into a predicate (for example
  `floats(source: &str) -> bool`) that the loop calls. Plant a workflow text
  with `      - uses: actions/checkout@v4` and assert it is refused, with a
  pinned control that is accepted. After the `-`, strip any whitespace, not a
  single space.

### A6 [test-safety] L2: the SEC-08 test destroys the checkout when the guard is wrong; canonicalization is underspecified
- Evidence: plan Unit Tests, `remove_owned_refuses_a_checkout_and_the_current_directory`:
  "the current directory survives". In `cargo test -p uorust --lib` the current
  directory is `crates/uorust` of the real checkout. A wrong guard makes this
  test `remove_dir_all` the source tree. `std::env::set_current_dir` is
  process-wide and races other tests. The rule also leaves several cases open:
  - Whether the current directory is canonicalized too. On Windows,
    `canonicalize` returns `\\?\C:\...` and `current_dir` does not, so an
    ancestor test across the two never matches.
  - What happens when `dir` does not exist. `canonicalize` fails, and the plan
    orders the check before the existence test.
  - Whether `.git` as a file counts. A git worktree has one, for example
    `target/ratchet-base` from Justfile:259.
- Required change: split out a pure `owned(dir, cwd) -> bool` and test it on
  temporary directories only. Canonicalize both paths. Return early when `dir`
  is absent. Detect `.git` with `symlink_metadata`, so a file or a directory
  counts. Add a test that exercises one real call site, for example
  `certify diagnose --out <tmp with .git>`, so a site that still calls
  `remove_dir_all` is caught.

### A7 [specification-drift] L2: GATE-11 `keep` still deletes before it confirms, and fails in its only caller
- Evidence: the plan renames `.prism-proof` to `.prism-proof.old`, moves the
  staged directory into place, "removes `.old`, then runs `check`". `check`
  (proof.rs:303-308) is the confirmation, so a proof that does not read back
  still costs the old one. That is the gap the brief names ("deletes before
  confirming"). Justfile:56 runs `rm -rf .prism-proof` before `prismpm verify`
  and `proof-keep`, so in the real flow `.prism-proof` never exists when `keep`
  runs. An unconditional rename then fails with NotFound. A `.prism-proof.old`
  left by a crashed run makes the rename fail (ENOTEMPTY, or an error on
  Windows). .gitignore:10 ignores only `/.prism-proof/`, so leftover `.new` or
  `.old` directories show up as untracked.
- Required change: remove a stale `.new` and `.old` first. Rename only when
  `.prism-proof` exists. Run `check` before removing `.old`, and restore `.old`
  if `check` fails. Add both names to .gitignore. State in D1 that the Justfile
  recipe already deletes the old proof, or change Justfile:56.

### A8 [specification-drift] L1: dropping `|| rm -f` does not make the hook reach the refusal; it exits silently
- Evidence: scripts/hooks/ratchet-staged.sh:7 sets `set -euo pipefail`, and
  :14-15 redirect `git show` errors to `/dev/null`. Without the fallback, a
  missing manifest makes `git show` exit 128 at :14 or :15. The hook stops there,
  prints nothing, and never runs `certify ratchet`. The plan says "an absent
  manifest reaches the refusal instead of becoming an empty one". It cites the
  hook at :13-14; the manifest lines are :14-15.
- Required change: drop `2>/dev/null` as well, or test with `git cat-file -e`
  and print a named refusal. Correct the plan's description and line numbers.

### A9 [documentation-accuracy] L1: the GATE-10 caller list and citations are wrong
- Evidence: the plan says "Both callers already name existing files on `main`
  (`just ratchet` at Justfile:262, the hook at :13-14)". The callers are
  Justfile:265-266, ratchet-staged.sh:14-17, generate-branch.sh:46-48 and
  translator-estimate.sh:82-84. crates/uorust/tests/certify.rs:72-93 also passes
  `--base-manifest`. "main.rs:819, 914: `sync`'s prior" is wrong:
  - :819 is in `Certify::Production` (:806).
  - :914 is in `Certify::Plan` (:892).
  - Q1's "`certify sync` re-checks them (certify.rs:1829)" points into `carried`
    (:1800), which is the export's carry check, not `sync` (:2718).
- Required change: list all five callers with their manifest sources, and
  correct the citations.

### A10 [test-specification] L1: the GATE-12 test arithmetic is wrong and "present" is undefined
- Evidence: in `a_lost_root_is_refused_even_when_the_count_shrinks`, the base
  certifies only `f`, so the uncertified count is {g, h} = 2. The head certifies
  `g` and `h`, so it is {f} = 1. That is 2 → 1, not "count 1 → 0". The register is
  keyed by κ (certify.rs:3756-3762), so "present in the head register" must
  mean a match by function name. The plan does not say so, or what happens when
  a name occurs on more than one row.
- Required change: correct the counts, and define presence (by `function`,
  including the duplicate-name case).

### A11 [documentation-accuracy] L1: the SEC-08 sites are cited one or two lines off, and bridge.rs callers are not listed
- Evidence: the `remove_dir_all` sites are at certify.rs:350, 476, 720, 1086,
  2054 and 2781, and embed.rs:4213. The plan uses the brief's 349, 475, 718,
  1085, 2053, 2780 and 4207. `replace_dir` is also called from bridge.rs:593,
  953 and 975, and the plan does not list them. `keep` at certify.rs:2054
  removes `kept.join(n)` (call at :1000), an engine-named child of the `kept`
  guarded at :720. That is the plan's own reason for leaving :1086 alone, yet
  :2054 is routed through `remove_owned`.
- Required change: correct the lines, list the bridge.rs callers, and apply one
  rule to :1086 and :2054.

### A12 [infrastructure-mismatch] L1: a CI command is invalid and a test file is unnamed
- Evidence: `cargo test -p uorust --lib certify registry` passes two
  positional filters, but cargo takes one TESTNAME, so the command errors.
  `--test <cli test file>` is a placeholder. The existing ratchet CLI test is in
  crates/uorust/tests/certify.rs.
- Required change: give two commands (`... --lib certify`, `... --lib registry`)
  and name `crates/uorust/tests/certify.rs`.

### A13 [specification-gap] L1: the SEC-02 rule leaves cases unspecified
- Evidence: lib.rs:209-213 today ignores a line without the two-space
  separator, and skips a listed file that is absent. The plan does not say
  whether either case is refused after the change. It does not cover a `Normal`
  path through a symlinked directory inside `out`. A manifest listing the
  victim's real SHA-256 would pass the digest check. It does not say how a
  developer recovers when a hand-edited committed fixture now makes
  `just fixtures` (Justfile:160) refuse.
- Required change: state the rule for each case: refuse malformed lines, skip
  absent files, refuse a symlinked component (checked with `symlink_metadata`).
  Add one line on recovery to the TEMPLATE-VERIFICATION section.

### A14 [missed-caller] L1: `Command::Merge` uses `parse_spec` and falls back silently
- Evidence: main.rs:697-700 treats any `parse_spec` error as a library crate
  (`Scope::library(&args.krate, ...)`). Once `parse_spec` validates more
  strictly, a registry spec it newly rejects is merged under a library scope
  with no error. The plan lists only `parse_spec`'s own call site and
  `lift_crate`.
- Required change: list main.rs:697 and decide its behavior. For example,
  refuse a spec that contains `@` but fails `crate_component`.

### A15 [test-safety] L1: `SDK_IMAGE` mutation races tests that do not hold the mutex
- Evidence: the plan sets the variable in tests "under a process-wide mutex".
  Other xtask tests spawn processes and read the environment without that mutex
  (proof.rs:214-229 `run`, the bootstrap tests). In edition 2021
  (Cargo.toml:7), `set_var` while another thread reads the environment is a
  data race in libc.
- Required change: pass the image in, for example
  `reuse_with(root, image: &str)` called by `reuse` with the env value, and test
  that function without touching the process environment.

### A16 [test-specification] L1: GATE-02 planted text can leak into the census it tests
- Evidence: bdd.rs sits under `crates/`, and `workspace_test_names` scans it
  (bdd.rs:31). A multi-line raw string in the new test that holds a
  `#[test]`/`fn name` line is counted as a workspace test. A slug-like planted
  name would discharge or violate CM-02 (meta.rs:91, :145-165) on the real
  workspace.
- Required change: write planted sources as single-line `\n`-escaped literals,
  as factory.rs:185 does, or use names with no `_xx_NN` suffix, and say so in
  the plan.

## Required before re-audit

Fix A1 through A7 in the plan. Address A8 through A16, or record why not. Then
submit a new revision, which will be judged fresh by its new SHA-256.
