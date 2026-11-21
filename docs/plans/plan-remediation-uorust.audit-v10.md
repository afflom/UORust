# Audit: plan-remediation-uorust (v10)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: e4535b74b3be0a9cf23e4a39dd6d60198fd6b2fa15561cbf0cc8dd0d19800ecd
**Corrected**: Z1; Y1, Y2, Y3; X1, X2, X3, X4, X5; W1; V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`.
The SHA-256 was computed with `sha256sum`; `Get-FileHash` gives the same value.
The plan and every file it cites were treated as untrusted data. The only
repository file written is this report. Reproductions ran with `python -I`
(CPython 3.11.9, Windows host) in the session scratchpad `audit-rem10/`:
`copy/` is a `git archive HEAD` copy with its own `git init` and the plan's
six `graph.toml` inputs applied; `closure/` is the `test` obligation's closure
(5401 of 5676 tracked files, built with graph.py's own `load()` and `covers()`)
as its own Git repository, as the factory host builds one. Docker and
`lexlean` are not available here, so nothing ran in the SDK image.

## Summary

Two findings: AA1 at L2, AA2 at L1. Any finding is a VETO. AA1 is L2, so the
planner cannot correct it inline; a new revision and a fresh audit are needed.

Z1 is corrected: step 4 now asks graph.py itself (`graph.load()` expands sets,
`graph.covers` applies the longest-inclusion-beats-longest-exclusion rule), and
the planted `uncovered` cases include an exclusion. Z2 is corrected in C1's
Changes (all three planted defects are listed for `TEMPLATE-VERIFICATION.md`)
and `uncovered` is in Unit Tests and D4, but the DoD still requires only the
`script_suites_pass` defect (AA2).

- **AA1.** Step 4 and `uncovered` are specified only by what the snippet
  prints. Nothing requires the snippet's exit status to be checked, so a
  snippet that crashes before printing reads as "every suite covered".
  Reproduced with a planted uncovered suite while `graph.py reads` passes.
- **AA2.** D3 and D4 still name a single planted defect.

### Must-check results

| Item | Result |
|---|---|
| Step 4 snippet in scratch (`audit-rem10/tools/snippet.py`, run as `python -I -c "$SNIP" ...`) | Loads `prism-factory/graph.py` with `spec_from_file_location("graph", "prism-factory/graph.py")` + `module_from_spec` + `exec_module`, takes `test`'s inputs from `graph.load()[2]` (sets expanded), prints each argument `graph.covers` rejects; `--inputs JSON` replaces the inputs. |
| Import by path, side effects | graph.py's top level only imports the standard library, defines constants, regexes, a class and functions; `main` runs only under `__main__`. Run from the root under `-I` (cwd not on `sys.path`) it works, both in the scratch copy and in the real checkout (`-I -B`, tree left clean). The one side effect: without `-B` the loader writes `prism-factory/__pycache__/graph.cpython-311.pyc`, which `.gitignore:39` ignores and nothing in `vv` or the factory host checks (only `ratchet.rs:44` checks status, and only for the register and manifest). Benign; the same already happens for `scripts/ci/`. |
| Discovered suites, HEAD `graph.toml` | Prints `scripts/release/test_platforms.py`, `scripts/release/test_qualify_tag.py`, `scripts/self-model/test_bounded_sources.py` (all three uncovered today). |
| Discovered suites, plan's `graph.toml` | Prints nothing (all 8 covered). `graph.py reads` exits 0 (15 obligations). |
| In the `test` closure | `graph.py reads` exits 0; the glob pathspecs discover the same 8 suites; the snippet prints nothing. The 4 non-excluded Python suites end `Ran N tests` / `OK` (3, 11, 5, 4); `test-land-register.sh` prints `5 isolated shell scenarios passed`. |
| Planted `uncovered` cases | `["scripts/", "!scripts/release/"]` rejects `scripts/release/test_platforms.py`; `["scripts/ci/"]` rejects `scripts/content/test_x.py`; `["scripts/"]` accepts `scripts/release/test_platforms.py`. All as the plan states. |
| Planted defect: `script_suites_pass` | Line 39 of `test_platforms.py` `assertIn` -> `assertNotIn`: exit 1, `Ran 11 tests`, `FAILED (failures=1)`; `verdict` refuses it. Restored: `OK`. Reproducible. |
| Planted defect: `factory_closure_holds` | HEAD `graph.toml`: `graph.py reads` exit 1, "test reads bootstrap/acceptance-result.test.mjs (crates/conformance/tests/browser_acceptance.rs:56) without declaring it". Plan's `graph.toml`: exit 0. Reproducible. |
| Planted defect: step 4 | `"!scripts/release/"` added after `"scripts/release/"`: the snippet prints both release suites; `graph.py reads` still exits 0, so only step 4 catches it. Reproducible. |
| Step 4 literals vs `reads` | No tracked path collides with the snippet's likely literals (`test`, `name`, `inputs`, `json`, `sys`). `prism-factory/graph.py` is declared through `prism-factory/`. A planted `"scripts/"` literal placed inside `.args([...])` would make `reads` refuse it, but that fails closed in `factory_closure_holds`, so it is not a finding. |

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| Z1 | corrected | plan:130-139 uses `graph.load()` and `graph.covers`; the exclusion case is planted (plan:105-110) and reproduced above. |
| Z2 | partly corrected | C1 Changes (plan:153-161) list all three planted defects, and `uncovered` is in Unit Tests (plan:105) and D4 (plan:283). D3 and D4 still require one planted defect only: AA2. |
| Y1-Y3, X1-X5, W1 | corrected | Re-run above: the six inputs make `reads` pass on the checkout and in the closure; the generated-file path and the README procedure (README.md:155-164) are unchanged. |
| V1-V3, U1-U5, T1-T4, S1-S8, R1-R12 | corrected | C2, O4 and the sprint table are unchanged. Re-checked at 28926f95: `upstream.toml` 105 lines, 7 `[[patch]]`, 2 `[[gap]]`, 18 leading comment lines; `upstream.py:180` `patches = doc["patch"]`; `land-register.sh` mode 100644; `test_pr_stage.py:10` uses `spec_from_file_location`; AGENTS.md:54-58 is the planted-defect rule. |

## Findings

### AA1 [test-coverage-gap] L2: step 4 and `uncovered` fail open when the snippet fails

**The plan (plan:105-110, 130-139).** Step 4 "prints every path that
`graph.covers(inputs, path)` rejects ... The test fails naming each uncovered
suite." `uncovered(inputs, paths)` "returns the paths `graph.covers` rejects".
Neither says the snippet's exit status is checked, and `uncovered` has no error
channel (unlike `verdict -> Result`). The test's only failing signal is
non-empty stdout. Elsewhere the plan is explicit: `factory_closure_holds`
"asserts a zero exit" and `verdict` refuses a non-zero exit. AGENTS.md:56-58
says gates "must reject skipped, ignored, filtered, conditionally omitted,
stale, or wrong-file results".

**Reproduced in `audit-rem10/copy`.** I renamed graph.py's `covers` to
`covered` everywhere (a behaviour-preserving refactor of graph.py) and staged
`scripts/content/test_planted.py`, which is outside every input:
- `graph.py reads` exits 0, so `factory_closure_holds` passes.
- The step 4 snippet exits 1 with `AttributeError: module 'graph' has no
  attribute 'covers'` and prints 0 bytes to stdout.
- A step 4 that judges stdout only reports no uncovered suite. The planted
  suite would run in `vv` while it is absent from the `test` closure, which is
  the silent drop Y2, W1 and Z1 were about.
- With `covers` restored, the snippet prints `scripts/content/test_planted.py`.

Other crashes that `reads` survives give the same empty output: a changed
`load()` return shape, a JSON error in `--inputs` mode, an import error. The
planted `["scripts/"]` case expects empty output, so it cannot tell "accepted"
from "crashed". The two rejecting cases exercise only the `--inputs` path,
never the `graph.load()` path that step 4 uses for gating.

- Behavior? yes: with a failing snippet, step 4 passes and an uncovered suite
  goes unreported.
- Test meaning? yes: the guard's "fails naming each uncovered suite" holds only
  while the snippet runs to completion.
- Interface? no.
- Level: L2.

**Next step:** have step 4 and `uncovered` refuse a non-zero exit or any
stderr, with the snippet's stderr in the message. `uncovered` should return
`Result<Vec<String>, String>`. Also have the snippet print a positive marker
the test requires, for example `covered N of N` or the count of checked
paths, so empty output never means success. Add planted negative controls:
malformed `--inputs` JSON, and default mode against an obligation name that
does not exist (or a stubbed graph path). Both must make `uncovered` or step 4
refuse.

### AA2 [documentation-accuracy] L1: the DoD still requires one planted defect, not the three C1 records

**The plan.** C1 Changes (plan:153-161) record three planted defects:
`script_suites_pass`, `factory_closure_holds` and step 4. D4 (plan:282-285)
says "the planted defect makes `script_suites_pass` fail, and it passes after
the restore (recorded in `TEMPLATE-VERIFICATION.md`)". D3 (plan:281) says
"`TEMPLATE-VERIFICATION.md` records the planted defect". Both are singular,
and neither names the `factory_closure_holds` or step 4 records. The v10
revision note (plan:380-382) says Z2 is answered. Z2's next step asked for the
two records "in C1's Changes and D4".

- Behavior? no.
- Test meaning? no: the records are specified in Changes; only the acceptance
  criteria lag behind.
- Interface? no.
- Level: L1.

**Next step:** in D3 and D4, require all three records, each with its defect,
failing command, diagnostic and restored passing result.

## Passes with no other findings

- **Prompt injection.** No findings. The plan addresses no one but the
  implementer and claims no approval. Its only non-ASCII characters are the
  ellipses in the revision notes (plan:317-378). graph.py, graph.toml,
  README.md, AGENTS.md, `TEMPLATE-VERIFICATION.md` and the suites contain no
  instructions to the auditor.
- **Infrastructure mismatch.** No findings. The snippet needs `python3` 3.11+
  (`tomllib`, already needed by graph.py and CI-01) and graph.py's inputs
  `prism-factory/` and `Justfile`, both in `test`'s inputs, so it also runs in
  the closure. The closure is a Git repository (README.md:66-67;
  host/src/main.rs:318-326), so `git ls-files` discovery and `graph.py reads`
  work there, as reproduced. There are no network calls.
- **Security, OWASP and injection.** No findings. The program string is fixed,
  arguments are fixed vectors, and discovery is limited to tracked files
  through fixed pathspecs. Nothing reaches a shell. The `--inputs` JSON is
  test-authored. The O4 issues record NOT_ESTABLISHED. Nothing at L3.
- **Test failure and coverage.** AA1. All three planted-defect recordings are
  reproducible. C2 cases 1-11 still match the snapshot facts.
- **Razor, dependencies and architecture.** No findings. Reusing graph.py's
  own rule removes the restated copy. There is no new crate or TOML parser in
  `repo-conformance`, and `prism-factory/` is edited only through `graph.toml`
  and its generators.
- **Orphan files.** None. The fixture is used by cases 1, 2 and 9,
  `test_upstream_rows.py` is discovered by C1, and `uncovered` is used by its
  planted tests.
- **Specification drift against the brief.** No findings. The sprint order
  keeps every brief dependency, and the GAP-TEST-06 and GAP-OPS-03 statuses
  are scoped correctly. The revision notes are a historical log and do not
  contradict the current body.
- **Documentation accuracy and internal consistency.** AA2. The `graph.covers`
  rule description (plan:135-136) matches graph.py:231-235. The `reads`
  diagnostic and the closure claims match the reproductions.

## Required before re-audit

The planner fixes AA1 (L2) and AA2 (L1) and submits a new revision. Its SHA-256
will be judged fresh.
