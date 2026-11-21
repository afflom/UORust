# Plan: remediate the UORust deep-audit gaps

**change_class**: feature

**doc_tier**: standard

**terms**: none introduced.

**boundaries**:
- limitations: Owner-only actions (O1-O6) are recorded, not performed. Slices
  that edit `.github/workflows/**` (marked *(wf)*) start only with an owned
  issue and owner approval. Template-locked files change upstream only.
- non_goals: No root-layout reorganization (UOR-Foundation/template#104 decides
  that first). No refactor pass until its unblocking slices land.
- exclusions: Generated code (`crates/certified/**`, `crates/uorust/src/certified.rs`,
  `prism-factory/**`, `CONFORMANCE.md`) changes only through its generators.

Source: `docs/audits/RESEARCH_BRIEF.md` at branch `audit/deep-audit-recon`,
commit `77d18b6b` (verified, 62 gaps, revised sprint plan). The C1 PR carries
that file into `main` so gap statuses can be updated. Statuses used:
**RESOLVED** (closed, with file:line and a passing test) and **PARTLY
RESOLVED** (the named part closed, the rest still open, both stated). Repository is Qortara-governed; seal NOT_ESTABLISHED; this plan and its
audit are host documents, not `.qortara/` or Qor-logic records.

## Open Questions

- Q1. O4 owner decisions (a)-(e) gate Sprint 3 onward; Sprint 1 and 2 do not
  depend on them.
- Q2. If a script suite needs the network or a tool absent from the SDK image,
  C1 leaves it out of `vv` with the reason recorded in the test's doc comment
  (no suite is skipped silently).

## Sprint plan

Ordered: release blockers, then severity, then effort; prerequisites in
brackets. One PR per slice; draft until `just vv` passes in the pinned SDK
image (uor-render VM).

| Sprint | Slices | Prerequisites |
|---|---|---|
| 1 | C1, then C2; owner-action issues for O4(a)-(e); O1-O3 listed for the owner | C1 before C2 (C2's suite runs through C1's discovery) |
| 2 | C7 (GATE-01..04), C8 (GATE-10..12), C11 (SEC-02, 07, 08) | C1 |
| 3 | C3 *(wf)* (OPS-01 phase A + OPS-02, the executable-bit check on scripts invoked by path, and a hermetic test against a bare remote that rejects pushes), C4 *(wf)* (OPS-01 phase B), C5 (GOV-02), C6 *(wf)* (GOV-06 + GOV-03), C9 *(wf)* (GOV-04) | C2 before C3; O4(a) before C4; O3, O4(b) and O5 before C6 |
| 4 | C10 (TEST-01, GATE-05/06/08/09), C12 (TEST-02..05 pins) | C7 |
| 5 | C13 (LIFT-01 + re-certify), C14 (LIFT-02, 04), C15 (LIFT-03), C16 (LIFT-05..08) | C8, C9, C12 |
| 6 | C17 *(wf)* (SEC-03/04/05/09/11), C18 (REL-01 + DEPS-03 Unix part), C19 *(wf)* (DOCS-02), C20 (OPS-07), C21 (DOCS-01, DOCS-03, SEC-06 control texts, DEPS-01/02, OPS-04/05/06, GATE-07) | O4(c) before C18; C18 with or before C19; C19, O3, O4(d) before C20 |

Owner: O1 (private reporting, secret scanning, push protection, Dependabot),
O2 (`can_approve_pull_request_reviews=false`), O3 (reviewed `release`
environment), O5 (`v*` tag ruleset, after O4(b)), O6 (main ruleset: required
check, 1 approval, CODEOWNERS, no always-bypass; after C3, C4, C5, O2).

## Sprint 1, Phase 1: every script suite inside `just vv` (C1)

Deviation from the brief, recorded: the brief's C1 was a Justfile recipe with an
executable-bit check. The operator chose a Rust test during the planning
dialogue on 2026-11-21 (the existing pattern in
`crates/conformance/tests/ci.rs`), and the executable-bit check moves to C3,
because `scripts/ci/land-register.sh` is mode 100644 and must stay so until
`push.sh` stops masking rejected pushes (brief, Round 3).

### Affected Files

- `crates/conformance/tests/ci.rs` - one new test and an updated module doc.
- `prism-factory/graph.toml` - the `test` obligation's `inputs` gain
  `"scripts/release/"` and `"scripts/self-model/"` (audit v6, W1), and the four
  files the SM-07 browser-acceptance test already loads but the obligation does
  not declare: `"bootstrap/acceptance-result.test.mjs"`,
  `"site/acceptance-result.mjs"`, `"site/guest-worker.mjs"` and
  `"site/spec.mjs"` (audit v7, X1, and audit v8, Y1: `graph.py reads` refuses
  on `28926f95` today, and in a minimal closure `node --test` fails without the
  last two); its comment and `untouched` text are updated.
- Regenerated in the pinned SDK image (LF line endings; audit v7, X4) by the
  procedure in `prism-factory/README.md`, "Changing the graph": `graph.py write`
  (rewrites `prism-factory/model/src/Factory/UORust.lex.tex` and the README
  table between its markers), then `lexlean lock`, `lexlean verify --all`,
  `lexlean production build`, and copying the result into
  `prism-factory/certified/`. The README's `targets` and `graph.py request`
  step is skipped because it applies only when the SDK's compiler changed, which
  this slice does not do; `just factory-model` refuses a stale request.
  Nothing in `prism-factory/` is edited by hand except `graph.toml`.
- `TEMPLATE-VERIFICATION.md` - a section recording the planted-defect evidence
  (AGENTS.md:54-56).
- `docs/audits/RESEARCH_BRIEF.md` - brought into `main` from
  `audit/deep-audit-recon` (`77d18b6b`); GAP-TEST-06 marked PARTLY RESOLVED.

### Unit Tests

- `crates/conformance/tests/ci.rs`, pure helpers tested first with planted
  outputs (negative controls for every refusal):
  - `enum Kind { Python, Shell }`, chosen from the file extension (`.py`, `.sh`).
  - `verdict(kind, status, stdout, stderr) -> Result<usize, String>` returns the
    test count of a passing suite and refuses, each with its own message: a
    non-zero exit; for `Python` (summary read from stderr) no line matching
    `Ran N test` or `Ran N tests` (unittest prints the singular for one test),
    N = 0, or a final summary other than exactly `OK` (so
    `OK (skipped=1)`, `OK (expected failures=1)` and `FAILED` are refused); for
    `Shell` (summary read from stdout) no `N isolated shell scenarios passed`
    line or N = 0. Planted inputs: one per refusal, one passing case per kind, a
    passing one-test Python output (`Ran 1 test`),
    and a passing Python output judged as `Shell` and a passing shell output
    judged as `Python` (both refused).
  - `stale(elsewhere, discovered) -> Vec<&str>` returns `ELSEWHERE` entries that
    are no longer discovered; planted: a table naming a missing file.
  - `judge_coverage(status, stdout, expected: usize) -> Result<Vec<String>, String>`
    reads the snippet's result: `Ok` with the uncovered paths, or `Err` when the
    exit is non-zero, the last line is not `checked N`, or N differs from
    `expected`. Planted inputs (audit v11, AB1): a non-zero exit, output without
    a `checked` line, `checked 1` for 8 paths (an early stop), `checked 9` for 8
    paths (a wrong count), and a passing output listing one uncovered path.
  - `uncovered(inputs: Option<&[&str]>, paths: &[&str]) -> Result<Vec<String>, String>`
    runs the snippet (with `--inputs` JSON when `inputs` is given, otherwise the
    `test` obligation's inputs from `graph.load()`) and returns
    `judge_coverage(..)` of its run (audit v11, AB2). The snippet counts N by
    incrementing once per path it judges, never from the length of its
    arguments. Planted: `["scripts/", "!scripts/release/"]` rejects
    `scripts/release/test_platforms.py`, `["scripts/ci/"]` rejects
    `scripts/content/test_x.py`, and `["scripts/"]` accepts
    `scripts/release/test_platforms.py`.
- `crates/conformance/tests/ci.rs::factory_closure_holds`: runs
  `python3 -I prism-factory/graph.py reads` from the repository root and
  asserts a zero exit, so `vv` refuses any obligation whose source names a
  tracked file it does not declare. Suites found at run time are covered by
  `script_suites_pass` step 4, not by this test. It
  fails on `28926f95` today (X1) and passes once `graph.toml` declares the
  files above. No TOML parser is added to `repo-conformance` (audit v7, X3).
- `crates/conformance/tests/ci.rs::script_suites_pass`:
  1. Discovers suites from disk with `git ls-files` glob pathspecs
     (`:(glob)scripts/**/test_*.py`, `:(glob)scripts/**/test-*.sh`), never a
     bare path literal, so `graph.py reads` (which scans source for literals)
     sees no undeclared read (audit v7, X2).
  2. Excludes only names in an `ELSEWHERE` table, each with its reason:
     `scripts/ci/test_pr_stage.py` (run by CI-01),
     `scripts/ci/test_change_impact.py` (loaded by CI-01's suite),
     `scripts/ci/test_pages_identity.py` (run by PG-01), and asserts
     `stale(ELSEWHERE, discovered)` is empty.
  3. Runs each suite from the repository root (`python3 -I <path>` or
     `bash <path>`, by `Kind`) and asserts `verdict(kind, ..)` is `Ok`.
  4. Asserts every discovered suite is covered by the factory's `test`
     obligation using the factory's own rule, not a copy of it (audit v9, Z1):
     a `python3 -I -c` snippet loads `prism-factory/graph.py` by path with
     `importlib.util.spec_from_file_location`, takes the `test` obligation's
     inputs from `graph.load()` (sets expanded), and prints every path that
     `graph.covers(inputs, path)` rejects (longest inclusion must beat longest
     exclusion, so `!` exclusions count), then a final line `checked N`.
     Step 4 calls `uncovered(None, discovered)` and requires `Ok` with no
     paths, so a crash, an early stop or a wrong count fails through
     `judge_coverage` rather than reading as "all covered" (audit v10, AA1);
     it fails naming each uncovered suite. `graph.py reads` cannot see suites found at run time (audit v8,
     Y2), so this is what keeps the factory's receipt over every suite `vv`
     runs.
  5. Its doc comment names the node suites left out and why: `try/test.mjs` and
     `identity/test.mjs` drive Chromium through Playwright, which the SDK image
     used by `vv` does not carry; both stay in `try-page.yml`.

  A new `test_*.py` under `scripts/` runs without editing the test; it must end
  in `unittest.main()`, or `verdict` refuses it for printing no `Ran` line.

### Changes

- The module doc of `ci.rs` describes both tests (CI-01 and the suite runner).
- No test name carries a register ID suffix: the meta-gate's third-direction
  check applies only to registered prefixes, and integration tests are not
  product functions in `frontier.toml`.
- `TEMPLATE-VERIFICATION.md` records, for each new check, the planted defect,
  the failing command, its diagnostic and the restored passing result
  (AGENTS.md:54-56; audit v9, Z2):
  - `script_suites_pass`: one assertion in `scripts/release/test_platforms.py`
    inverted;
  - `factory_closure_holds`: `graph.toml` before this slice (the SM-07 files
    undeclared), which `graph.py reads` refuses;
  - step 4: `!scripts/release/` added to the `test` obligation's inputs in a
    scratch commit, which reports the release suites uncovered.

## Sprint 1, Phase 2: removing a patch row removes only that row (C2)

### Affected Files

- `scripts/ci/test_upstream_rows.py` - new unit tests (first).
- `scripts/ci/fixtures/upstream.toml` - a committed snapshot of `upstream.toml`
  at `28926f95`, so the tests do not change when automation re-pins the live
  file (audit v5, V1).
- `scripts/ci/upstream_rows.py` - new module holding the one rule.
- `scripts/ci/upstream.py` - `frame_pin` uses `upstream_rows.without_patch`;
  `main` reads `doc.get("patch", [])` (was `doc["patch"]`, `upstream.py:180`),
  so a file with no `[[patch]]` rows left runs instead of raising `KeyError`.
- `scripts/ci/repin.py` - `drop_rows` uses `upstream_rows.without_patch`.
- `TEMPLATE-VERIFICATION.md` - the old split failing case 1, recorded.
- `docs/audits/RESEARCH_BRIEF.md` - GAP-OPS-03 marked RESOLVED.

### Unit Tests

`scripts/ci/test_upstream_rows.py` (ends in `unittest.main()`) loads
`upstream_rows.py`, `upstream.py` and `repin.py` with
`importlib.util.spec_from_file_location` (as `test_pr_stage.py:10` does) and
checks each case against an exact expected string or exception. Every case
reads only `scripts/ci/fixtures/upstream.toml` (copied to a temporary directory
wherever a script writes) or an inline fixture; none reads the live root
`upstream.toml`.

1. On the snapshot `scripts/ci/fixtures/upstream.toml`: removing `template-frame-live-controls`, the
   last patch, removes exactly its 8-line row and the blank line after it; the
   18-line preamble (the 18 comment lines; the blank line after them is not
   part of it but is kept), every other patch, the
   `# Gaps:` comment and both `[[gap]]` tables are byte-identical.
2. Removing the first patch on the snapshot leaves the preamble and everything
   else byte-identical.
3. An id that names no patch returns the input unchanged.
4. A fixture where a section comment sits directly before a patch row: removing
   that patch keeps the comment.
5. A fixture where the patch is the last table and the file ends after it: the
   blank line before the removed patch goes too, so the file ends with one
   newline.
6. A comment line inside a patch body raises `ValueError` naming the id.
7. A `[[gap]]` whose `id` equals the patch id is never removed.
8. An indented comment or an indented `[[gap]]` right after the removed row is
   kept byte-identical. Shapes the line rule cannot handle all raise
   `ValueError`, from the header search, the pre-check, or the post-condition:
   the id not on the line after the header, `id="x"` spacing, a trailing comment
   on the header, a multi-line string holding a `[[` line, and a duplicated patch
   id.
9. Removing the only remaining `[[patch]]` row: on a fixture with a preamble and
   one patch, the result is the preamble ending in exactly one newline and
   parses with no `patch` key; on the snapshot, `drop_rows` over all seven
   patch ids leaves the preamble, the `# Gaps:` comment and both `[[gap]]`
   tables byte-identical.
10. `upstream.main` on a file with no `[[patch]]` rows completes without
    `KeyError` and acts on no patch: the test points `upstream.ROOT` at a
    temporary directory whose `upstream.toml` is the committed fixture with every
    patch removed by `without_patch`, and patches out `run`, `get` and the
    network calls.
11. `repin.drop_rows(text, [a, b])` equals removing `a` then `b`, and
    `upstream.frame_pin` writes `without_patch(text, patch["id"])`: the test
    points `upstream.ROOT` at a temporary directory holding a copy of the
    committed fixture and patches out `run`, so it never reads the live root
    `upstream.toml` (which is outside the factory's `test` obligation; audit v12,
    AC1).

The old split fails case 1 (it removes 3117 bytes, including both `[[gap]]`
tables); it gives the same output as the new rule for every other patch, so
case 2 guards the first draft of this rule (audit v1, R1), not the old code.

### Changes

`upstream_rows.without_patch(text: str, patch_id: str) -> str`:
- Parse `text` with `tomllib`. If no `[[patch]]` row has `id == patch_id`,
  return `text` unchanged; if more than one does, raise `ValueError`.
- Work on lines. Find the header line exactly `[[patch]]` whose next line is
  exactly `id = "<patch_id>"`; if there is none, raise `ValueError`.
- The block ends at the first later line whose text, after leading whitespace,
  starts with `[` or `#`, or at end of file, so indented comments and table
  headers are never swallowed. If such a comment line is followed, after comment
  and blank lines, by a key line rather than a header or end of file, raise
  `ValueError`.
- Remove the lines from the header to the block end. If the block reached end
  of file, remove the blank lines left at the end of the result, so it ends with
  exactly one newline.
- The preamble is the text before the first table header, without its trailing
  blank lines; no table owns it and it is never removed. Only blank lines after
  it can go, under the end-of-file rule.
- Post-condition: parse the result with `tomllib`; it must equal the parsed
  input with exactly that one row removed from `patch` (and the `patch` key
  absent when no row remains), everything else equal, or raise `ValueError`.

Both scripts load the module by path relative to their own file with
`importlib.util.spec_from_file_location`, which works under `python3 -I`
(audit v1, R4).

## Sprint 1, Phase 3: owner actions (O4)

### Affected Files

- none in the repository; five GitHub issues on UOR-Foundation/UORust.

### Changes

- One issue per decision, each citing its gap IDs and the brief: (a) bot-write
  route to `main` (OPS-01, GOV-01, GOV-07); (b) who may create release tags and
  whether `release-tag.yml` stays (GOV-06); (c) Windows release: sign or drop
  the leg (REL-01); (d) reserve `uorust` and `uorust-certified` on crates.io or
  withdraw the claim (OPS-07); (e) AGENTS.md Qortara-check wording, blocked on
  the template contract re-pin (GOV-05). Issue text records the policy as
  NOT_ESTABLISHED and claims no owner approval.

## Definition of Done

### Deliverable: script suites in `vv` (C1)

- **D1**: every Python and shell script test suite runs under `just vv`; the
  node suites `try/test.mjs` and `identity/test.mjs` stay in `try-page.yml`
  (browser), with the reason recorded.
- **D2**: `script_suites_pass` and `factory_closure_holds` in
  `crates/conformance/tests/ci.rs`; the factory `test` obligation in
  `prism-factory/graph.toml` declares every suite's directory and the four SM-07
  files, with its generated text and certified bundle regenerated in the SDK
  image.
- **D3**: `TEMPLATE-VERIFICATION.md` records all three planted defects
  (`script_suites_pass`, `factory_closure_holds`, step 4) with command,
  diagnostic and restored result; the brief marks GAP-TEST-06 PARTLY RESOLVED (the Python and shell CI-only
  suites now in `vv`; still open: the browser suites `try/test.mjs` and
  `identity/test.mjs`, CI-only; and the untested push, merge, generate-loop and
  land-certified scripts, release writers, MIR scripts, installers and hooks)
  with file:line.
- **D4**: `cargo test -p repo-conformance --test ci` passes in the SDK image,
  including the planted-output tests of `verdict`, `stale`, `judge_coverage`
  and `uncovered`;
  each of the three planted defects makes its check fail (`script_suites_pass`,
  `factory_closure_holds`, `script_suites_pass` step 4), and each passes after
  the restore.

### Deliverable: patch-row removal keeps gaps (C2)

- **D1**: removing a `[[patch]]` row never removes other tables or their comments.
- **D2**: `without_patch` in `scripts/ci/upstream_rows.py`, used by `frame_pin`
  (`scripts/ci/upstream.py`) and `drop_rows` (`scripts/ci/repin.py`).
- **D3**: brief marks GAP-OPS-03 RESOLVED with file:line.
- **D4**: `scripts/ci/test_upstream_rows.py` cases 1-11 pass; case 1 fails
  against the old split (run once before the fix, recorded in
  `TEMPLATE-VERIFICATION.md`).

### Deliverable: owner-action issues (O4)

- **D1**: each O4 decision has an open issue.
- **D2**: five issue URLs.
- **D3**: brief's sprint plan links them.
- **D4.d**: no executable behavior. **Follow-up phase**: Sprint 3 consumes them.

## CI Commands

- `python3 -I scripts/ci/test_upstream_rows.py` - C2 unit tests (host and SDK).
- `cargo test -p repo-conformance --test ci` - C1 suites (SDK image).
- `python3 -I prism-factory/graph.py reads` - every obligation declares what
  it reads (also run by `factory_closure_holds` under `vv`).
- `just factory-model` - the factory graph, generated text and certified bundle
  agree after the `graph.toml` change (SDK image; not part of `vv`).
- `just vv` - full acceptance gate in the pinned SDK image, as `bootstrap.yml` runs it.

## Revision

v2 answers `docs/plans/plan-remediation-uorust.audit-v1.md` (VETO on
`2434b2dc…`): R1-R3 (rule redefined on lines from the header; the preamble is
never owned; a comment before a patch is kept; an inner comment fails closed;
cases 2, 4, 5, 6 added), R4 (one module loaded by path), R5 (suites discovered
from disk; empty or skipped runs refused; stale exclusions fail), R6
(`identity/test.mjs` included; `try/test.mjs` excluded with reason;
`test_change_impact.py` named), R7 (executable-bit check assigned to C3;
deviation recorded), R8 (O5 before C6), R9 (C1 before C2), R10 (brief path and
statuses defined), R11 (evidence in `TEMPLATE-VERIFICATION.md`), R12 (`ci.rs`
module doc).

v3 answers `docs/plans/plan-remediation-uorust.audit-v2.md` (VETO on
`526f400e…`): S1 (`identity/test.mjs` excluded, browser), S2 (node suites out
of discovery; unittest summary read from stderr; only exactly `OK` accepted;
new suites must end in `unittest.main()`), S3 (pure `verdict` and `stale`
helpers with planted outputs for every refusal), S4 (old split fails case 1
only; case 2 guards the v1 draft), S5 (tomllib pre- and post-condition; all
listed shapes raise; case 8), S6 (8-line row; preamble line count, corrected to 18 in v5), S7 (C3 hermetic test, C18 DEPS-03, C21 gap IDs; the C1 PR
carries the brief), S8 (the choice is cited as the operator's, made in the
planning dialogue).

v4 answers `docs/plans/plan-remediation-uorust.audit-v3.md` (VETO on
`8bd1efd4…`): T1 (removing the last remaining patch defined: `patch` key absent;
end-of-file blank rule no longer touches the preamble; case 9), T2 (`Kind`
parameter; cross-kind planted outputs), T3 (brief and `TEMPLATE-VERIFICATION.md`
in Affected Files), T4 (D3 names the still-open browser suites).

v5 answers `docs/plans/plan-remediation-uorust.audit-v4.md` (VETO on
`ec37fc8d…`): U1 (`upstream.py main` reads `doc.get("patch", [])`; case 10),
U2 (block ends at an indented `[` or `#` too; case 8 keeps indented comments
and headers), U3 (`Ran N test` singular accepted; planted one-test output), U4
(18-line preamble), U5 (*(wf)* marked per slice: C3, C4, C6, C9).

v6 answers `docs/plans/plan-remediation-uorust.audit-v5.md` (VETO on
`135a3d92…`): V1 (cases 1, 2, 9 run on the committed snapshot
`scripts/ci/fixtures/upstream.toml`, not the live file automation rewrites), V2
(D3 lists every still-open part of GAP-TEST-06), V3 (case 8 names where each
shape raises).

v7 answers `docs/plans/plan-remediation-uorust.audit-v6.md` (VETO on
`33cf8ca9…`): W1 (the factory `test` obligation declares `scripts/release/` and
`scripts/self-model/`; generated text and the certified bundle are regenerated
by the factory's generators; `script_suites_pass` fails on a suite outside the
obligation's inputs; `just factory-model` added to CI commands; the in-test coverage check
  described here was dropped in v8 and restored in v9 without a TOML
  dependency).

v8 answers `docs/plans/plan-remediation-uorust.audit-v7.md` (VETO on
`c07aaef1…`): X1 (the SM-07 files declared; the pre-existing `reads` refusal
recorded), X2 (`graph.py reads` runs under `vv` as `factory_closure_holds`;
discovery uses glob pathspecs), X3 (no TOML parsing in the Rust test; the
closure check is the factory's own tool), X4 (generators run in the SDK image),
X5 (the README's "Changing the graph" procedure named).

v9 answers `docs/plans/plan-remediation-uorust.audit-v8.md` (VETO on
`25af74da…`): Y1 (`site/guest-worker.mjs` and `site/spec.mjs` declared too),
Y2 (`script_suites_pass` step 4 checks each discovered suite against the `test`
obligation's inputs, read with `tomllib` via `python3 -I -c`; `covered` helper
planted; `factory_closure_holds` no longer claims to cover run-time suites), Y3
(`Factory/UORust.lex.tex` path; the skipped `request` step explained).

v10 answers `docs/plans/plan-remediation-uorust.audit-v9.md` (VETO on
`faa5cf82…`): Z1 (step 4 uses `graph.load()` and `graph.covers` from the
factory itself, so sets and `!` exclusions follow the engine's rule; planted
`uncovered` cases include an exclusion), Z2 (planted-defect evidence for
`factory_closure_holds` and step 4 recorded in `TEMPLATE-VERIFICATION.md`;
`uncovered` listed in Unit Tests and D4).

v11 answers `docs/plans/plan-remediation-uorust.audit-v10.md` (VETO on
`e4535b74…`): AA1 (the snippet ends with `checked N`; a non-zero exit or a
missing or short count is refused; planted crash case), AA2 (D3 and D4 name all
three planted defects).

v12 answers `docs/plans/plan-remediation-uorust.audit-v11.md` (VETO on
`da7ec68d…`): AB1 (pure `judge_coverage` with planted early-stop and
wrong-count outputs; N counted per judged path), AB2 (`uncovered` returns
`Result`, its inputs are a parameter, refusal goes through `judge_coverage`),
AB3 (D3's trailing singular sentence removed).

v13 answers `docs/plans/plan-remediation-uorust.audit-v12.md` (VETO on
`3e6596a0…`): AC1 (cases 10 and 11 point `upstream.ROOT` at a temporary copy
of the committed fixture, so no case reads the live `upstream.toml`).

v14 corrects inline the all-L1 VETO `docs/plans/plan-remediation-uorust.audit-v13.md`
(on `e1d9c2ce…`): AD1 (the C2 Unit Tests preamble states that every case reads
only the committed fixture or an inline fixture).
