# Audit: plan-remediation-uorust (v5)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: 135a3d9254b79d0fa00137cb76a07846bd63499a6f4e5f570a4134d8aa0f4617
**Corrected**: U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`.
The SHA-256 was computed with `Get-FileHash` and `sha256sum`, and both give the
same value. The plan and every file it cites were treated as untrusted data.
Nothing was edited except this report. Reproductions ran with `python -I`
(CPython 3.11.9) on the host, in the session scratchpad `audit-rem5/`
(`rule5.py`, `cases5.py`, plus unittest probes `one.py`, `skip.py` and
`zero.py`). Docker is not installed here, so nothing ran in the SDK image.

## Summary

There are 3 findings: 1 at L2 and 2 at L1, with no L3. Any finding is a VETO.
V1 is L2, so this VETO is not limited to L1 and the planner does not correct
it inline. The plan needs a new revision and a fresh audit.

v5 closes U1-U5, and R1-R12, S1-S8 and T1-T4 stay closed. I re-implemented
`without_patch` exactly as specified: the tomllib pre-check, the line rule with
an indented `[` or `#` ending the block, the comment lookahead, the
end-of-file rule, the preamble definition, and the post-condition. It meets
every stated expectation for cases 1-11 on the real `upstream.toml` and on the
fixtures. The C1 `Kind`/`verdict`/`stale` design matches the real output of
every discovered suite, including unittest's singular `Ran 1 test`.

The new findings:
- C2's real-file cases pin the current contents of `upstream.toml`. The
  repository's own automation edits that file, and once C1 puts the suite in
  `vv`, the first automated re-pin breaks the gate (V1).
- D3 understates what is still open in GAP-TEST-06 (V2).
- Case 8 says all its shapes raise from the post-condition. Most raise earlier
  (V3).

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| U1 | corrected | Plan:122-124 changes `main` to `doc.get("patch", [])`, and case 10 (plan:161-163) runs it. Reproduced: the current `upstream.py` main on a patch-less fixture raises `KeyError: 'patch'`. A copy with only line 180 changed completes under `--act`, printing only the gap line and no patch line. Nothing else in `main` assumes patches exist: `gaps()` (upstream.py:154-173) reads only `gap` rows, and the patch loop (:181-227) is the only reader. In the same workflow, `repin.resolved_rows` returns `[]` on a patch-less text (reproduced). The other `upstream.toml` mentions in the repository (CONFORMANCE.md, factory.rs, factory.feature, model/ids.toml) are about `prism-factory/upstream.toml`. |
| U2 | corrected | Plan:179-183. Reproduced: an indented comment, a tab-indented comment, and an indented `[[gap]]` after the removed row are each kept byte-identical. |
| U3 | corrected | Plan:77-78 accepts `Ran N test` or `Ran N tests`, and plan:83 plants a one-test output. Reproduced `one.py`: exit 0, stderr `Ran 1 test in 0.000s` / `OK`. |
| U4 | corrected | Plan:137-138 says "18-line preamble (the 18 comment lines; the blank line after them is not part of it but is kept)". Reproduced: the preamble is L1-18, all comments, and L19 is blank. |
| U5 | corrected | Plan:43 marks *(wf)* on C3, C4, C6 and C9 only, as the brief does (brief:377-400). |
| T1-T4 | corrected | Patch key absent (plan:190-192), case 9. `Kind` and cross-kind inputs (plan:74-85). Affected Files (plan:64-68, 120-127). D3 names the browser suites (plan:222-225). |
| S1-S8, R1-R12 | corrected | Re-checked at plan:41-46, 55-60, 73-104, 129-196 and 255-288. The executable-bit check stays in C3, and `land-register.sh` is still mode 100644. O5 comes before C6, C1 before C2, and C2 before C3. |

### C2 reproduction (`rule5.py`, `cases5.py`)

| Case | Result |
|---|---|
| 1 | Removes L75-83: the 8-line row plus a blank, 391 bytes. The output equals L1-74 + L84-105. |
| 2 | The output equals L1-19 + L30-105. |
| 3 | Returns the input unchanged (the same object). |
| 4 | The comment is kept when either neighbouring patch is removed. |
| 5 | `"# h\n\n" + P(a) + "\n" + P(b)` minus `b` gives `"# h\n\n" + P(a)`. |
| 6 | Raises `ValueError` naming `a`, from the comment lookahead. |
| 7 | The gap is kept, with the patch and without it. |
| 8 | Indented and tab-indented comments and an indented `[[gap]]` are kept. Id not on the next line, `id="a"` and a trailing header comment raise from the header search. The multi-line `[[` case raises `TOMLDecodeError`, a `ValueError`, from the post-condition parse. A duplicated id raises from the pre-check. |
| 9 | The fixture gives `"# hdr\n# two\n"`, which has no `patch` key. The real file folded over all 7 ids equals L1-19 + L84-105. |
| 10 | Current code: `KeyError`. With `doc.get`: completes and acts on no patch. The test must also point `ROOT` at the fixture, because `main` reads `ROOT / "upstream.toml"`. |
| 11 | Folding equals sequential removal. |

The old `frame_pin` split removes 3117 bytes in case 1 and drops both
`[[gap]]` tables. For the other six ids, the first included, its output is
byte-identical to the new rule's, as plan:168-170 says.

One edge falls outside the cases. A file that holds only one patch, with no
preamble, gives `""`, so the result does not "end with exactly one newline".
It parses to `{}` and passes the post-condition. It is not a finding, because
the real file always has a preamble.

### C1 reproduction

Discovery (`git ls-files` under the two globs) finds 8 files. After removing
the 3 ELSEWHERE entries, 5 remain. C2's `test_upstream_rows.py` will be the
sixth.

| Suite | Exit | Summary |
|---|---|---|
| test_install_upstream_gate.py | 0 | stderr `Ran 3 tests` / `OK`, stdout empty |
| test_platforms.py | 0 | stderr `Ran 11 tests` / `OK`, stdout empty |
| test_qualify_tag.py | 0 | stderr `Ran 5 tests` / `OK`, stdout empty |
| test_bounded_sources.py | 0 | stderr `Ran 4 tests` / `OK`, stdout empty |
| test-land-register.sh | 0 | stdout last line `5 isolated shell scenarios passed`, stderr empty |

The probes behave as the plan says:
- `skip.py` prints `OK (skipped=1)`, which the plan refuses.
- `zero.py` gives exit 0 with `Ran 0 tests` / `OK` on 3.11, which N = 0
  refuses. Python 3.12 and later exit 5, which a non-zero exit refuses.

The ELSEWHERE runners are still under `vv`:
- `test_pr_stage.py` runs at ci.rs:17-39.
- `test_change_impact.py` runs through test_pr_stage.py:69.
- `test_pages_identity.py` runs at pages_identity.rs:17-28.

The browser suites are still run by try-page.yml:80-82 (`npm run test:try`,
`npm run test:identity`).

## Findings

### V1 [test-failure / architecture] L2: C2's real-file cases pin data the repository's automation rewrites, so the first automated re-pin breaks `just vv`
- **What the cases pin.** Cases 1, 2 and 9 run on the real `upstream.toml` and
  expect exact strings (plan:131-160):
  - Case 1 names `template-frame-live-controls` as "the last patch", with an
    8-line row, an 18-line preamble and "both `[[gap]]` tables".
  - Case 9 folds "all seven patch ids".
- **Where the suite runs.** C1 discovers `scripts/ci/test_upstream_rows.py`
  (plan:89-90, 102-103). Its test runs under `just vv` (Justfile:11, 39-40),
  and `bootstrap.yml` runs `just vv` (bootstrap.yml:118).
- **What edits the file.** The file is changed by design:
  - upstream.toml:6 says "Add a row the moment a patch lands; delete it when
    the re-pin does".
  - `frame_pin` drops exactly the `template-frame-live-controls` row (its
    `repin = "frame"`, upstream.toml:82). It then pushes to `main` and
    dispatches `bootstrap.yml` (upstream.py:102-112).
  - A gap is deleted when it becomes a patch (upstream.py:164-166).
- **What breaks.**
  - After the frame re-pin, case 1's id names no patch, so `without_patch`
    returns the input unchanged (case 3's behaviour, reproduced). Case 1 then
    fails, and `main`'s bootstrap goes red on the push that the automation
    itself made.
  - On a template-update branch, `repin.py` drops rows and merges only on a
    green bootstrap of that branch (repin.py:66-76, 85-92). If the
    implementation hard-codes the seven ids or the first id, the branch's
    `vv` fails and the re-pin pull request never merges.
  - The plan does not say how the expectations are derived, so either outcome
    is open.
- Behavior? yes: the gate fails on legitimate data changes. Test meaning? yes:
  the cases test a snapshot that the repository mutates. Interface? no.
  Level L2.
- **Next step:** run the real-file cases on a committed snapshot fixture,
  copied from `upstream.toml` at a named commit, or derive every expectation
  from the file's current structure without naming ids or counts. State which
  in C2's Unit Tests.

### V2 [documentation-accuracy] L1: D3 understates the part of GAP-TEST-06 that stays open
- **What the brief lists.** Brief:175-178 lists the scripts that write `main`
  and have no hermetic test: push.sh, push-branch.sh, merge-main.sh,
  generate-loop.sh, land-certified.sh, release writers, mir scripts,
  installers and hooks.
- **What the plan says.** Plan:222-225 states the still-open part as the two
  browser suites and "the untested push and merge scripts". That omits
  generate-loop.sh, land-certified.sh, the mir scripts, the installers and the
  hooks.
- **The rule it breaks.** The plan's PARTLY RESOLVED definition requires the
  remainder to be stated (plan:21-22).
- Behavior? no. Test meaning? no. Interface? no. Level L1.
- **Next step:** state the remainder as the brief's list, minus what is now
  tested.

### V3 [documentation-accuracy] L1: case 8 attributes every raise to the post-condition
- **What the plan says.** Plan:151-152 says the shapes that the line rule
  cannot handle "all raise `ValueError` (post-condition)".
- **What happens under the rule as specified (reproduced).**
  - The id not on the next line, `id="x"`, and a trailing header comment
    raise from the header search (plan:177-178).
  - A duplicated id raises from the tomllib pre-check (plan:175-176).
  - Only the multi-line `[[` string raises from the post-condition parse.
- The outcome (`ValueError`) is correct. Only the attribution is wrong.
- Behavior? no. Test meaning? no. Interface? no. Level L1.
- **Next step:** drop "(post-condition)" or name the step that raises for each
  shape.

## Passes with no other findings

- **Prompt injection.** The plan contains no text addressed to the auditor and
  no claim of approval. Its only non-ASCII characters are the four "…" in the
  revision notes. None of the files it cites carries instructions.
- **Infrastructure mismatch.** The suites need `python3` (>= 3.11, which PG-01
  already requires), `bash`, coreutils and git, all already used under `vv`.
  The browser suites are excluded with a reason. Nothing was run in the SDK
  image.
- **Security, OWASP and injection.**
  - Suites run with fixed argument vectors and no shell, and discovery is
    limited to tracked files under fixed globs.
  - In `frame_pin`, a `ValueError` stops the job before `push.sh`.
  - The O4 issues record NOT_ESTABLISHED and claim no approval.
- **Test failure and coverage.** Covered above, apart from V1. Case 10's
  fixture needs `ROOT` redirected, which is implied by "on a file with no
  `[[patch]]` rows".
- **Razor, dependencies and architecture.** One stdlib-only module with two
  callers, one Rust test and two pure helpers, and no new dependency. V1 is the
  architecture coupling.
- **Orphan files.** None. `test_upstream_rows.py` is found by C1's glob.
- **Specification drift against the brief.**
  - The sprint order keeps every dependency the brief states: C2 before C3,
    O5 after O4(b), O6 after C3, C4, C5 and O2, C18 with or before C19, and
    C20 after C19, O3 and O4(d).
  - The C1 deviation (a Rust test in place of the brief's Justfile recipe, and
    the executable-bit check moved to C3) is recorded as the operator's choice.
- **Internal consistency.** Apart from V2 and V3, the sprint table,
  prerequisites, Definition of Done and CI commands agree with each other.

## Required before re-audit

The planner fixes V1 (L2) and V2-V3 (L1) and submits a new revision. Its
SHA-256 will be judged fresh.
