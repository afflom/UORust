# Plan: remediation Sprint 2, gate and confinement fixes (C7, C8, C11)

**change_class**: hotfix

**doc_tier**: standard

**terms**: none introduced.

**boundaries**:
- limitations: Rust, shell and `.gitignore` only; no `.github/workflows/**`
  edit (none of these slices is *(wf)*). Template-locked files are not touched.
- non_goals: GATE-05..09 (Sprint 4, C10), the LIFT fixes (Sprint 5), any
  change to the translator's output.
- exclusions: generated code (`crates/certified/**`,
  `crates/uorust/src/certified.rs`, `prism-factory/**`, `CONFORMANCE.md`).

Parent plan: `docs/plans/plan-remediation-uorust.md` (v14, PASS), sprint table
row 2. Source of each gap: `docs/audits/RESEARCH_BRIEF.md`. Statuses RESOLVED
and PARTLY RESOLVED as the parent defines them. Repository is
Qortara-governed; seal NOT_ESTABLISHED.

Branches stack linearly so the appended `TEMPLATE-VERIFICATION.md` sections and
the brief's status edits never conflict: C7 on C2 (`remediation/c2-upstream-rows`),
C8 on C7, C11 on C8. One PR each, base `main` (`pr_stage.py` requires it).

Planted source text in every new test is a single-line, `\n`-escaped string
literal (as `crates/conformance/tests/factory.rs:185` writes it), and planted
test names carry no `_xx_NN` suffix, so no plant enters the workspace census
`bdd.rs` takes of itself (audit v1, A16).

## Open Questions

- Q1 (decided). GATE-12 has two halves. C8 closes (a): a root certified at the
  base that no longer lifts at the head is refused even when the count shrinks.
  A root whose κ changed (the function was edited) is owed to an export, as
  `scripts/hooks/translator-estimate.sh:5-9` and
  `scripts/ci/generate-branch.sh:50-57` already treat it, and stays under the
  count rule. When another root's gain masks it, the count does not grow and
  no export is dispatched on the pull request; `certified.yml` runs on a push
  to `main` that changes `crates/uorust/**` (among its paths,
  `certified.yml:21-31`), which an edit of a translator function always does,
  and exports it there (:85, :128).
  Half (b), a certified root whose Lean definition changed under an
  unchanged κ, is not a ratchet question: the root's `closure` and `source`
  change in the committed `crates/certified/manifest.toml` diff, and the export
  carries a root only when its closure and source bytes match
  (`carried`, certify.rs:1800-1843). Whether such a change must be refused is a
  policy question; the brief marks GAP-GATE-12 PARTLY RESOLVED with (b) open.
- Q2 (decided). SEC-08 asks for an ownership check before `remove_dir_all` of
  operator-given directories. A marker file would add bytes to committed
  generated trees (`embed::replace_dir` writes them), so C11 uses one guard:
  refuse to remove a directory that holds a `.git` entry (file or directory)
  or that is the current directory or one of its ancestors. It closes the
  destructive cases (`--out .`, `--out ~`, `--out /`, a checkout or worktree);
  it does not prove the engine wrote the directory. The brief says exactly
  that.
- Q3 (decided). GATE-02 inside one file handles attributes on the test and on
  enclosing inline modules. A test in a separate file that a non-test
  `#[cfg(...)] mod x;` gates is not resolved across files; no such declaration
  exists in `crates/` or `xtask/` today. The brief marks GAP-GATE-02 PARTLY
  RESOLVED with that case open.

## Sprint 2, Phase 1: scanners that stop refusing (C7: GATE-01..04)

### Affected Files

- `crates/conformance/src/meta.rs` - `names_id(test, slug) -> bool`, used by
  the R3 test check at :89-97.
- `crates/conformance/tests/bdd.rs` - `test_names_in(text)` split out of
  `workspace_test_names` (:29-71); the walk skips `fixtures` and `target`; the
  falsifiability test gains a plant.
- `xtask/src/bootstrap.rs` - `uses_reference(line)` and `floats(source)`; the
  workflow loop (:576-594) and the `# prismpm-action-input` check (:600-613)
  use them.
- `xtask/src/audit.rs` - `effective_lines` (:84-114) ends a braceless
  `#[cfg(test)]` item at its terminator.
- `TEMPLATE-VERIFICATION.md` - section "Gate scanners that stopped refusing (GATE-01..04)".
- `docs/audits/RESEARCH_BRIEF.md` - GATE-01..04 statuses.

The GATE-04 fix exposes shipped lines the old rule hid: `mir/mod.rs:35-38`,
`:43`, `:44`, `:46` and `mir/stdlib.rs:76`, `:228` (audit v2 model of these
rules). None holds `Result<`.
If `cargo xtask audit` reports a newly exposed line, the slice stops for a
decision rather than editing it.

### Changes

- **GATE-01.** `names_id(t, slug)`: with `t` taken after its last `::`,
  `t == slug || t.ends_with(&format!("_{slug}"))`. `due_1` no longer
  discharges `UE-1`.
- **GATE-02.** `test_names_in(text) -> BTreeSet<String>` scans line by line,
  tracking brace depth and the run of attribute lines before each item. A run
  is the lines from one that starts with `#[` through the line where its
  brackets balance, then any further such attributes; comment and blank lines
  inside a run continue it, and any other line ends it:
  - An attribute run that contains `#[test]` and ends at a `fn` line names a
    test, unless the run holds `#[ignore`, a `cfg_attr(` whose text contains
    `ignore`, or a `#[cfg(` other than `#[cfg(test)]`.
  - An attribute run holding a `#[cfg(` other than `#[cfg(test)]` before a
    `mod name {` line skips every test until that module's brace closes.
  - A file whose inner attributes (`#![cfg(`…) hold a non-test `cfg` yields none.
  `workspace_test_names` skips directories named `fixtures` or `target`:
  fixture crates are not workspace members and never run under `vv`. Audit v1
  computed the effect on the workspace: 350 names to 331, all 19 dropped from
  `crates/uorust/tests/fixtures/`, every one of the 29 registered IDs still
  matched.
- **GATE-03.** `uses_reference(line) -> Option<&str>`: trim leading
  whitespace, strip an optional `-` and any whitespace after it, then require
  `uses:`. `floats(source) -> bool` is the whole workflow predicate the loop
  calls: `ubuntu-latest`, `@main`, `@master`, or a line where `uses_reference`
  is `Some` and the reference is not pinned. `action_reference_is_pinned`
  reads through `uses_reference` and returns false when there is none, so the
  marker check no longer passes a marker line without `uses:` (it returns
  true today). Audit v1: all 147 `uses:` lines and all 29 marker lines are
  pinned today.
- **GATE-04.** `effective_lines` keeps brace depth and adds paren/bracket
  nesting, both counted per character. After a `#[cfg(test)]` line, the
  attribute run that follows (as GATE-02 defines it: through the line where
  each attribute's brackets balance, comment and blank lines included) is part
  of the item. The item's first line after the run decides its kind, after
  visibility (`pub`, `pub(...)`) is stripped:
  - *field*: what remains is `ident:` not followed by a second `:` (a struct
    field; `ident::` is a path, not a field) or an identifier
    followed by `(`, `{`, `=`, `,` or the end of the line (an enum variant),
    and is none of the item keywords below;
  - otherwise *item*: `use`, `const`, `static`, `type`, `mod`, `struct`,
    `enum`, `union`, `trait`, `impl`, `fn` (after any of `default`, `const`,
    `async`, `unsafe`, `extern "…"`), `extern` blocks, `macro_rules!`, and
    any macro invocation (`thread_local! { … }`).
  With *d* the brace depth at the attribute, every kind ends when a brace it
  opened closes back to *d* (as today's rule ends a braced item,
  audit.rs:108-111), at a line that closes below *d*, and at a `;` at depth
  *d* and zero paren/bracket nesting (`mod generate;`, `struct S;`,
  `const X: [u8; 3] = …;`). A field, and a match arm (a first line holding
  `=>`, such as `Kind::Probe => 1,`) that has opened no brace, also end at a
  `,` at depth *d* and zero nesting. No other item ends at a `,`, so a
  test-only `fn` with a multi-line `where` clause or generics runs to its
  body's closing brace.

### Unit Tests

Every test runs the unit on text holding the defect and asserts the refusal,
and on the corrected text and asserts acceptance.

- `meta.rs` `names_id_needs_an_underscore_boundary`: `("due_1","ue_1")` false;
  `("ue_1","ue_1")`, `("x_ue_1","ue_1")`, `("m::x_ue_1","ue_1")` true.
- `bdd.rs` `the_meta_gate_is_falsifiable_cm_02` gains a plant: a list of every
  registered slug prefixed by `a` with no `_` (e.g. `aue_01`) fails with a
  CM-02 missing-test violation for each ID.
- `bdd.rs` `test_names_in_drops_ignored_and_conditional_tests`: one text with a
  plain test (counted) and, each not counted: `#[ignore]` after `#[test]`,
  `#[ignore]` before `#[test]`, `#[cfg(unix)]` before `#[test]`,
  `#[cfg_attr(not(x), ignore)]`, `#[cfg(any())]` after `#[test]`, a test
  inside `#[cfg(any())] mod m { … }`, `#[ignore]` then `// reason` then
  `#[test]`, a multi-line `#[cfg_attr(\n not(x),\n ignore\n)]`, and a test
  after the module closes (counted). A second text with `#![cfg(any())]` yields none.
- `bdd.rs` `fixture_tests_are_not_counted`: a temp tree with
  `crates/a/tests/t.rs` and `crates/a/tests/fixtures/f/src/lib.rs`, one test
  each, yields only the first.
- `bootstrap.rs` `floating_workflow_input_plants_are_rejected`: `floats` on a
  workflow text with `      - uses: actions/checkout@v4`, with `-  uses:`
  (two spaces), and with `-\tuses:` is true; the same with a 40-hex pin is
  false. `action_reference_is_pinned` on a marker line with no `uses:` is
  false. The existing `floating_action_plant_is_rejected` stays.
- `audit.rs` `a_braceless_test_item_does_not_hide_the_next_item`: each of
  `#[cfg(test)]\nmod generate;`, `#[cfg(test)] use x::y;`, and a struct field
  `#[cfg(test)]\npub rust: String,`, followed by a shipped
  `pub fn f() -> Result<(), std::io::Error>`, makes `audit_limits` refuse `f`.
  Each of these stays exempt, with a shipped `f` after it still refused: a
  braced `mod tests { … }` holding the same signature; a test-only `fn` whose
  parameter list spans lines (the certify.rs:2293 shape); a test-only `fn`
  with a multi-line `where T: Clone,` clause; a test-only `fn` whose generics
  span lines (`fn g<\n A: Clone,\n B,\n>() {`); a test-only `unsafe impl`
  block; a test-only `thread_local! { … }`; a test-only `extern "C" { … }`
  block; a test-only `static S: [u8; 2] = [1, 2];`; a test-only match arm
  `Kind::Probe => 1,` inside a shipped `match` (the shipped `f` after the
  enclosing function is refused); and `#[cfg(test)]` followed by a
  multi-line `#[allow(\n dead_code,\n)]` and `mod tests {`.

## Sprint 2, Phase 2: ratchets that pass on missing or masked input (C8: GATE-10..12)

### Affected Files

- `crates/uorust/src/main.rs` - the `Certify::Ratchet` arm (:1019-1043)
  refuses an absent `--manifest` or `--base-manifest` file.
- `crates/uorust/src/certify.rs` - `Profile::ratchet` (:3787-3810) refuses a
  root that no longer lifts.
- `scripts/hooks/ratchet-staged.sh` - :10-11 become `rm -rf "$out" && mkdir -p
  "$out"`, so no file from an earlier run is read; :14-15 test with
  `git cat-file -e`, so an absent manifest reaches the ratchet's named refusal.
- `xtask/src/proof.rs` - `reuse` refuses without an SDK image; `keep` stages
  and confirms before it removes the old proof.
- `.gitignore` - `/.prism-proof.new/` and `/.prism-proof.old/` beside
  `/.prism-proof/` (:10).
- `TEMPLATE-VERIFICATION.md`, `docs/audits/RESEARCH_BRIEF.md`.

Callers of `certify ratchet`, each naming manifests that exist on `main`:
`Justfile:265-266` (`just ratchet`: the work tree's and the base worktree's),
`scripts/hooks/ratchet-staged.sh:14-17` (index and `HEAD`),
`scripts/ci/generate-branch.sh:46-48` (work tree and `main`),
`scripts/hooks/translator-estimate.sh:82-84` (work tree and the base tree),
and `crates/uorust/tests/certify.rs:72-93` (a temp manifest).

### Changes

- **GATE-10.** In the `Certify::Ratchet` arm, before anything is read, each of
  `manifest` and `base_manifest` must be a file; otherwise
  `UorustError::Tool("certify ratchet: <path> does not exist; a ratchet against
  an absent manifest measures nothing")`. `Manifest::read`'s absent-is-empty
  rule stays for its other callers (`Certify::Production` at main.rs:819 and
  `Certify::Plan` at :914, the prior manifest of a first export).
  `ratchet-staged.sh` empties its directory first, writes each manifest only
  when `git cat-file -e` finds it, and otherwise passes the absent path
  through, so the refusal names it.
- **GATE-12 (a).** In `Profile::ratchet`, `lost` is every function name in
  scope that is certified under the base (some base row with that `function`
  is lifted and its κ is in the base manifest), has at least one head row, and
  has no head row that is lifted. Rows are matched by `function`; a function
  that is lifted at the head under any κ is not lost, because a changed κ is
  owed to an export. A non-empty `lost` is refused before the count rule with
  `UorustError::Grown` whose first row is
  `"<n> certified roots of the parent no longer lift"`, then one row per
  function, `"<function>: <head cause>"`. That first row does not match
  `generate-branch.sh:53`'s `[0-9]* uncertified .*, [0-9]* in the parent`, so
  the run stops with its `::error::` instead of dispatching an export that
  cannot fix a residue. A count growth keeps today's rows unchanged, so
  `generate-branch.sh` and the existing assertion at certify.rs:4652-4660
  (where `meter::read` is lifted under a new κ, not lost) are unaffected.
- **GATE-11.** `reuse_with(root, image: &str)` holds the logic; `reuse` calls
  it with `SDK_IMAGE` (empty when unset). An empty image is refused ("no SDK
  image named, so no kept proof can bind this run") before `check`. `inputs`
  keeps `unwrap_or_default`, so a proof kept outside the SDK records an empty
  image and is never reused. `keep(root, attestation)` checks the ID, computes
  `inputs(root)` before touching anything, and calls
  `install(root, attestation, inputs: &str, confirm: impl Fn(&Path) -> Result<String, Fail>)`
  with `check` as `confirm`. `install`:
  1. removes stale `.prism-proof.new` and `.prism-proof.old`;
  2. copies the evidence into `.prism-proof.new` (a failure removes it and
     leaves `.prism-proof` untouched);
  3. renames `.prism-proof` to `.prism-proof.old` if it exists, then
     `.prism-proof.new` to `.prism-proof`;
  4. runs `confirm(root)`; on failure removes `.prism-proof`, renames `.old` back if
     present, and returns the error; on success removes `.old`.
  `Justfile:56` already runs `rm -rf .prism-proof` before `prismpm verify`, so
  in the `app` recipe there is no old proof to keep; `keep` no longer depends
  on that.

### Unit Tests

- `certify.rs` `a_root_that_no_longer_lifts_is_refused_even_when_the_count_shrinks`:
  base register lifts `f`, `g`, `h`; the base manifest certifies `f`
  (uncertified at the base: `g`, `h`, so 2). Head: `f` is residue, `g` and `h`
  lifted and certified (uncertified at the head: `f`, so 1; today this
  passes). Refused, first row `1 certified roots of the parent no longer lift`,
  then `f: residue (...)`. Control: head `f` lifted under a new κ and
  uncertified, `g`, `h` certified, passes with count 1.
- `certify.rs` `a_deleted_root_is_not_a_loss`: head register without `f`,
  manifest without `f`: passes.
- `certify.rs` the existing `the_ratchet_holds_any_scope_and_a_scope_only_widens`
  (:4608-4662) passes unchanged.
- `crates/uorust/tests/certify.rs` `ratchet_refuses_an_absent_manifest`: the
  binary with a missing `--base-manifest`, then a missing `--manifest`, fails
  with the named message; with both present it passes.
- `proof.rs` `reuse_needs_an_sdk_image`: `reuse_with(root, "")` on a temp root
  with no kept proof errors with the image message, not the missing-proof one.
- `proof.rs` `keep_loses_nothing_when_a_copy_fails`: a temp root with a kept
  proof and a published directory missing one `EVIDENCE` file: `install` (with
  fixed inputs text and a `confirm` that succeeds) errors,
  `.prism-proof` is byte-identical, and neither `.new` nor `.old` remains.
- `proof.rs` `keep_restores_the_old_proof_when_the_new_one_does_not_read_back`:
  a complete published directory and a `confirm` that fails: `install` errors,
  the old `.prism-proof` is restored byte-identical, and no `.new` or `.old`
  remains. Control: a `confirm` that succeeds leaves the new proof and no
  `.old`.

## Sprint 2, Phase 3: paths that escape their directory (C11: SEC-02, SEC-07, SEC-08)

### Affected Files

- `crates/uorust/src/lib.rs` - `write` (:200-227) validates every manifest line
  before removing anything.
- `crates/uorust/src/registry.rs` - `crate_component(name, version)`;
  `parse_spec` (:55-63) and `lift_crate` (:231-240) call it.
- `crates/uorust/src/main.rs` - `Command::Merge` (:697-700) refuses a spec
  holding `@` that `parse_spec` refuses, instead of falling back to a library
  scope.
- `crates/uorust/src/error.rs` - `owned(dir, cwd) -> Result<bool, UorustError>` and
  `remove_owned(dir)`.
- Guarded sites: certify.rs:350 (`diagnose`, `--out`), :476 (`admit_roots`,
  `out`), :720 (`production_build`, `out` and `kept`), embed.rs:4213
  (`replace_dir`, which embed.rs:4085, 4092, 4101 and bridge.rs:593, 953, 975
  call), and certify.rs:2781 (`sync` removes `roots` under its `--dir`: it
  first requires `owned(dir, current_dir)?` to be true, `Ok(false)` becoming
  `ForeignOutput(dir)` and an io error passing up, so `--dir ~` is refused before
  `~/roots` is touched). Left as they are, with a comment naming the guarded parent: :1086
  (`build_batch`, a child of `out`), :2054 (`keep`, `kept.join(n)`, a child of
  the `kept` guarded at :720), registry.rs:239 and :299 (`work`, under the temp directory, built
  from validated components).
- `TEMPLATE-VERIFICATION.md`, `docs/audits/RESEARCH_BRIEF.md`.

### Changes

- **SEC-02.** In `write`, every line of `uorust.manifest` must be
  `<64 hex>  <path>`; any other line is `ForeignOutput(out)`. The path must be
  relative with only `Normal` components, and no ancestor of the listed file
  inside `out` may be a symlink (`symlink_metadata`); otherwise
  `ForeignOutput(out)`. A listed file that is absent is skipped, as today. A
  listed file whose SHA-256 differs from its line is not the engine's output:
  `ForeignOutput(out)`. All lines are checked before any file is removed.
  The committed fixture `crates/uorust/tests/fixtures/thermostat-prism/`
  matches its manifest (audit v1), so `just fixtures` still rewrites it; a
  hand-edited projection is recovered with `git checkout` of that directory or
  by removing it.
- **SEC-07.** `crate_component(name, version)`: name matches
  `[A-Za-z][A-Za-z0-9_-]{0,63}`; version only `[0-9A-Za-z.+-]`, not `.` or
  `..`. Refusal: the existing `tool("uorust crate", …)`.
- **SEC-08.** `owned(dir, cwd) -> Result<bool, UorustError>`: `Ok(true)` when `dir` does not exist
  (nothing there to destroy; `sync` creates its `--dir` at certify.rs:2934,
  and the `sync` tests at :4931 and :5633 pass a directory not yet created);
  otherwise `Ok(false)` when `dir` holds `.git` (`symlink_metadata`, file or
  directory), or when the canonical `dir` equals or is an ancestor of the
  canonical `cwd`. `remove_owned(dir)`: returns `Ok`
  when `dir` does not exist; otherwise refuses with `ForeignOutput(dir)` unless
  `owned(dir, current_dir)?` is true, then `remove_dir_all`. When the current
  directory cannot be read (in `remove_owned`), or either path cannot be
  canonicalized (in `owned`), the io error is returned as
  `error::io(path, e)` and nothing is removed, so the guard fails closed. No new `UorustError`
  variant, so UE-06 and R5 are unchanged.

### Unit Tests

- `lib.rs` `write_refuses_a_manifest_path_outside_out`: `uorust.manifest`
  lists a matching SHA with `../victim`; `victim` beside `out` survives and
  `write` returns `ForeignOutput`. The same for an absolute path, a malformed
  line, and a path through a symlinked directory inside `out` (Unix only).
- `lib.rs` `write_refuses_a_listed_file_it_did_not_write`: a listed file with a
  different SHA survives; `ForeignOutput`. Control: a matching SHA is replaced;
  an absent listed file is skipped.
- `registry.rs` `spec_refuses_a_path_in_the_version`: `serde@../../x`,
  `serde@1/2`, `../x@1`, `serde@..` refused; `serde@1.0.219` and
  `serde_json@1.0.0-rc.1+build` accepted.
- `error.rs` `owned_refuses_a_checkout_and_the_working_directory`: on temp
  directories only: a dir with a `.git` directory, a dir with a `.git` file,
  `cwd` itself and `cwd`'s parent are not owned; a sibling of `cwd` and a
  path that does not exist are owned.
  The process's current directory is never changed or removed.
- `embed.rs` `replace_dir_refuses_a_checkout`: `replace_dir` on a temp dir
  holding `.git` returns `ForeignOutput` and the dir and its `.git` survive.
- `certify.rs` `sync_refuses_a_dir_it_does_not_own`: `sync` with `dir` a
  temp directory holding `.git` and a `roots/` child returns `ForeignOutput`
  before reading anything else, and `roots/` survives (the guard is the first
  statement of `sync`).
- `crates/uorust/tests/cli.rs` `merge_refuses_a_bad_registry_spec`:
  `uorust merge --crate serde@../x …` fails naming the spec.

## Definition of Done

### Deliverable: gate scanners (C7)

- **D1**: the CM-02 test match, the test-name census, the floating-action
  check and the R5 test exemption each refuse the defect the brief names.
- **D2**: `names_id`, `test_names_in`, `uses_reference`, `floats` and
  `effective_lines` as above.
- **D3**: `TEMPLATE-VERIFICATION.md` records one planted defect per gap with
  command, diagnostic and restored result; the brief marks GAP-GATE-01, 03 and
  04 RESOLVED and GAP-GATE-02 PARTLY RESOLVED (Q3), with file:line.
- **D4**: the unit tests above pass; `cargo test -p repo-conformance --test bdd`
  and `cargo test -p xtask` pass on the real workspace.

### Deliverable: ratchets (C8)

- **D1**: the certify ratchet refuses an absent manifest and a certified root
  that no longer lifts; a proof kept outside the SDK is never reused; a failed
  keep loses no proof.
- **D2**: as Changes.
- **D3**: evidence recorded; GAP-GATE-10 and GAP-GATE-11 RESOLVED, GAP-GATE-12
  PARTLY RESOLVED ((a) closed, (b) open per Q1).
- **D4**: the unit and CLI tests above pass; `just ratchet` passes on the C8
  head against its parent.

### Deliverable: confinement (C11)

- **D1**: no manifest line, crate spec or operator-given directory makes the
  engine delete outside what it owns or inside a checkout.
- **D2**: as Changes.
- **D3**: evidence recorded; GAP-SEC-02 and GAP-SEC-07 RESOLVED; GAP-SEC-08
  RESOLVED for the guard Q2 defines, stating that ownership is not proven.
- **D4**: the unit and CLI tests above pass.

## CI Commands

- `cargo test -p repo-conformance --test bdd` - C7 meta-gate (SDK image).
- `cargo test -p xtask` - C7 scanners and C8 proof tests (SDK image).
- `cargo test -p uorust --lib certify` - C8 ratchet (SDK image).
- `cargo test -p uorust --lib registry`, `cargo test -p uorust --lib write`,
  `cargo test -p uorust --lib owned`, `cargo test -p uorust --lib replace_dir` - C11.
- `cargo test -p uorust --test certify` - C8 CLI test (SDK image).
- `cargo test -p uorust --test cli` - C11 CLI test (SDK image).
- `just ratchet` - C8 on its own parent (SDK image).
- `just vv` - full gate in the pinned SDK image, as `bootstrap.yml` runs it.

## Revision

v2 answers `docs/plans/plan-remediation-uorust-sprint-2.audit-v1.md` (VETO on
`e882dfa4…`): A1-A2 (the refusal is a residue loss, not a κ change; first row
distinct from the count row; count rows unchanged), A3 (paren/bracket nesting,
attribute runs, braced versus field items; four exempt plants), A4 (attributes
on either side of `#[test]`, `cfg_attr` ignore, inline `cfg` modules, inner
`cfg`; out-of-file `cfg` modules open, Q3), A5 (`floats` predicate planted;
any whitespace after `-`), A6 (pure `owned` on temp dirs; both paths
canonical; absent dir returns early; `.git` file or directory; a real call
site planted), A7 (stage, swap, check, then remove or restore; `.gitignore`),
A8 (`git cat-file -e`), A9 (five callers, corrected citations), A10 (counts
2 → 1; presence by `function`), A11 (lines corrected, bridge callers, one rule
for children of a guarded directory), A12 (commands split, test file named),
A13 (malformed, absent, symlink, recovery), A14 (`Command::Merge`), A15
(`reuse_with`), A16 (escaped single-line plants).

v3 answers `docs/plans/plan-remediation-uorust-sprint-2.audit-v2.md` (VETO on
`eb8a0a0c…`): B1 (the hook empties its directory), B2 (`install` with injected
inputs and `confirm`; both keep tests reach their step), B3 (a masked κ change
is exported on `main` by `certified.yml`), B4 (attribute continuation lines,
depth *d* for fields, qualifiers stripped; `unsafe impl` plant), B5 (comments
and blanks continue a run; two plants), B6 (exposure list from these rules),
B7 (`sync`'s `--dir` guarded before `roots` is removed), B8 (`cli.rs`).

v4 answers `docs/plans/plan-remediation-uorust-sprint-2.audit-v3.md` (VETO on
`3081d1dc…`): C1 (every kind ends when a brace it opened closes back to *d*;
`thread_local!` and `extern "C"` plants), C2 (an absent `dir` is owned; the
`sync` tests and a first sync unaffected), C3 (`certified.yml`'s path filter
cited), C4 (field defined positively; qualifiers only before `fn`; `static`
plant).

v5 answers `docs/plans/plan-remediation-uorust-sprint-2.audit-v4.md` (VETO on
`182d94ed…`): D1 (every kind ends at a line closing below *d*; a braceless
item ends at a top-level `,`; `ident::` is not a field; match-arm plant), D2
(an unreadable or uncanonicalizable path returns the io error and removes
nothing).

v6 answers `docs/plans/plan-remediation-uorust-sprint-2.audit-v5.md` (VETO on
`9a5cab41…`): E1 (only fields and match arms end at a `,`; the `where` plant
stays exempt), E2 (`owned` returns `Result<bool, UorustError>`).

v6 then takes `docs/plans/plan-remediation-uorust-sprint-2.audit-v6.md`'s two
L1 findings inline (VETO on `ceabd7a3…`, no new round required): F1 (Affected
Files signature; `sync` maps `Ok(false)` to `ForeignOutput`), F2 (multi-line
generics plant).
