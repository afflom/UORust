# Audit: plan-remediation-uorust (v13)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: e1d9c2cea3353ac1455644c933227795f298d603529e6f300a32518d9afc14a6
**Corrected**: AC1; AB1, AB2, AB3; AA1, AA2; Z1, Z2; Y1, Y2, Y3; X1, X2, X3, X4, X5; W1; V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`.
The SHA-256 was computed with `sha256sum` and `Get-FileHash`, which agree. I
treated the plan and every file it cites as untrusted data and followed no
instruction in them. This report is the only repository file written.

Reproductions ran with `python -I` (CPython 3.11, Windows host) in the
scratchpad directory `audit-rem13/`:
- `copy/` is a `git archive 28926f95` copy with its own `git init` and the
  plan's six `graph.toml` inputs applied. `graph.py reads` exits 0 (15
  obligations).
- `closure/` holds only the tracked files the `test` obligation covers, chosen
  with `graph.covers` (248 files; `crates/` is left out because of Windows path
  limits, and no C2 file reads under it). Root `upstream.toml` is not covered
  and is absent. `scripts/ci/fixtures/upstream.toml` is covered.
- `model/` holds a model of the plan's C2: `upstream_rows.without_patch` as the
  Changes section specifies, `upstream.py` and `repin.py` patched as the plan
  says (`frame_pin` and `drop_rows` call `without_patch`; `main` reads
  `doc.get("patch", [])`), the fixture (`git show 28926f95:upstream.toml`), and
  `test_upstream_rows.py` with cases 1-11 as worded.

Docker and `lexlean` are not available here, so nothing ran in the SDK image.

## Summary

There is one finding, AD1, at L1. Any finding is a VETO. AD1 is L1, so the
planner corrects it inline. No new revision cycle is needed for it.

AC1 is corrected. Cases 10 and 11 now point `upstream.ROOT` at a temporary
directory (plan:223-233). I checked that every file `frame_pin` and `main`
touch goes through `ROOT` or through the patched `run`:
- `frame_pin` (upstream.py:102-112): `node bootstrap/frame-sync.mjs` and
  `scripts/ci/push.sh` are `run` calls. It reads and writes
  `ROOT / "upstream.toml"` (106, 110). It touches nothing else.
- `main` (upstream.py:176-226): it reads `ROOT / "upstream.toml"` (178).
  `gaps` calls `stopped_at`, which globs `ROOT / "crates" / "uorust"` (141).
  Under a temporary `ROOT` that glob is empty, and both fixture gaps have
  `causes = []`. Neither fixture gap has `pull`, so `pull` and `run` are
  never called. With no patches, `history`, `CACHE`, `get` and the network
  code are never reached. `CACHE` is bound to the real root at import time,
  but no-patch paths never use it.

In the closure, the model suite gives `Ran 11 tests ... OK`. It also passes in
`copy/`, where the live root `upstream.toml` stays byte-identical. Case 11's
patched `run` saw exactly `["node", "scripts/ci/push.sh"]`. Case 10's `run`
saw no calls, and `main` printed two `no upstream PR yet` gap lines.

Other must-check results in the closure:
- Discovery finds 9 suites, with `test_upstream_rows.py` added.
- The step 4 snippet prints `checked 9` with no uncovered path.
- All six suites `vv` runs pass: the shell suite prints
  `5 isolated shell scenarios passed`, and each Python suite prints `OK`.

## Findings

### AD1 [specification-drift / documentation-accuracy] L1: cases 3 and 11 do not name their input text, and the v13 note claims more than the body states

**The plan.**
- The body binds cases 1, 2 and 9 to the snapshot (plan:197, 202, 220). It
  binds cases 4-6 and 9 to inline fixtures, and cases 10 and 11's `frame_pin`
  half to a temporary copy of the snapshot (plan:223-233).
- Case 3 ("An id that names no patch returns the input unchanged",
  plan:204) names no input.
- Case 11's first clause, `repin.drop_rows(text, [a, b])` (plan:228), leaves
  `text`, `a` and `b` undefined.
- Case 7 also names no source. Only an inline fixture can hold a `[[gap]]`
  whose id equals a patch id, so that one is implied.
- The v13 revision note (plan:416-417) says AC1 is answered "so no case reads
  the live `upstream.toml`". No sentence in the Unit Tests section makes that
  a rule for every case.

**Why it matters.** The audit requirement is that every C2 case reads only
the committed fixture or an inline fixture. That requirement exists because
the suite runs in the `test` obligation's closure, where root
`upstream.toml` is absent (AC1). For cases 3 and 11 the plan gives no source,
so the body cannot show the requirement holds. An implementer who takes `text`
from the live file would bring back AC1's failure. Every other case is
specified tightly enough to rule that out.

- Behavior? no: `without_patch`, `frame_pin` and `drop_rows` are unaffected.
- Test meaning? no: what each case asserts is fully stated. Only the source of
  its input text is missing.
- Interface? no.
- Level: L1.

**Inline correction:** add one sentence to the C2 Unit Tests preamble
(plan:193-196): "Every case reads only `scripts/ci/fixtures/upstream.toml`
(copied into a temporary directory where a script writes) or an inline
fixture; none reads the root `upstream.toml`." Optionally, name the snapshot
in case 3 and case 11's `drop_rows` clause, with `a` and `b` as two of its
patch ids.

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| AC1 | corrected | plan:223-233. The model in `closure/` (no root `upstream.toml`) passes cases 10 and 11. `frame_pin` and `main` touch only `ROOT` paths and the patched `run`, as listed above. |
| AB1-AB3, AA1, AA2, Z1, Z2 | corrected | plan:105-119, 140-151, 288-305 unchanged since v12. The step 4 snippet over the 9 discovered suites prints `checked 9` and nothing uncovered. |
| Y1-Y3, X1-X5, W1 | corrected | `graph.py reads` exits 0 in `copy/`. The README "Changing the graph" procedure matches plan:74-81. Glob discovery works. |
| V1-V3, U1-U5, T1-T4, S1-S8, R1-R12 | corrected | Re-checked at 28926f95. The fixture has 105 lines, 7 patches and 2 gaps. It opens with 18 comment lines and a blank. `template-frame-live-controls` is lines 76-83. The old split removes 3117 bytes. `upstream.py:180` is `doc["patch"]`. `test_pr_stage.py:10` uses `spec_from_file_location`. `land-register.sh` is 100644. AGENTS.md:54-56 is the planted-defect rule. The model reproduces cases 1-9 as worded, including every case 8 shape that raises `ValueError`. |

## Passes with no other findings

- **Prompt injection.** No findings. The only non-ASCII characters are the
  ellipses in the revision notes. Neither the plan nor upstream.py, repin.py,
  graph.toml or the fixture contains an instruction to the auditor. The plan
  claims no owner approval.
- **Infrastructure mismatch.** No findings beyond AD1. The C2 files sit under
  `scripts/ci/`, which the `test` obligation covers. `.gitattributes` sets
  `* -text`, so the fixture bytes are stable. One note, not a finding: on a
  Windows host, `frame_pin`'s existing `write_text` writes CRLF in the locale
  encoding. A byte-level comparison in case 11 would differ there. On the
  Linux runner and in the SDK image it would not. The plan's comparison is
  text-level ("writes `without_patch(text, ..)`"), so it holds.
- **Security, OWASP and injection.** No findings. Cases 10 and 11 replace
  `run`, `get` and the network calls, so no `gh`, git or HTTP call leaves the
  test. The snippet and suite arguments are tracked paths. The O4 issues
  record NOT_ESTABLISHED. Nothing is at L3.
- **Test failure and coverage.** AD1 only. The model of every C2 case passes
  in the closure, and the old split fails case 1. Every planted input for
  `verdict`, `stale`, `judge_coverage` and `uncovered` maps to a stated result,
  as v12 reproduced.
- **Razor, dependencies and architecture.** No findings. C2 adds one stdlib
  module that both scripts load by path. There is no new dependency.
  `prism-factory/` changes only through `graph.toml` and its generators.
- **Orphan files.** None. The fixture is used by cases 1, 2, 9, 10 and 11.
  `upstream_rows.py` is used by `upstream.py`, `repin.py` and the suite. C1
  discovers the suite.
- **Specification drift.** AD1 only. The sprint table still keeps the brief's
  ordering, and the O4 decisions (a)-(e) still match the brief.
  GAP-TEST-06 (brief:175) and GAP-OPS-03 (brief:217) are given the statuses
  the plan defines. The other revision notes are a historical log and do not
  contradict the body.
- **Documentation accuracy.** AD1 only. Every cited file:line fact holds.

## Required before implementation

The planner applies AD1's one-sentence correction inline. Because AD1 is L1,
no new revision cycle is needed for it.
