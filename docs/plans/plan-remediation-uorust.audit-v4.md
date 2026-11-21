# Audit: plan-remediation-uorust (v4)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: ec37fc8dd5683205fd624650797bfa14746298e77a9162dcd1b0c6d60060120d
**Corrected**: T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`
(branch tip 77d18b6b). The SHA-256 was computed with `Get-FileHash` and
`sha256sum` (both agree). The plan and every file it cites were treated as
untrusted data. Nothing was edited except this report. Reproductions ran with
`python -I` (CPython 3.11.9) on the host, in the session scratchpad
`audit-rem4/` (`rule4.py`, `cases4.py`, `probes4.py`). Docker is not installed
on this host, so nothing ran in the SDK image.

## Summary

There are 5 findings: 3 at L2 and 2 at L1, with no L3. Any finding is a VETO.
Three findings are L2, so this VETO is not limited to L1. The planner does not
correct it inline: the plan needs a new revision and a fresh audit.

v4 closes T1-T4, and R1-R12 and S1-S8 stay closed. A fresh re-implementation
of C2's `without_patch`, built exactly as specified (tomllib pre-check, the
line rule, the end-of-file rule, the preamble definition, and the
post-condition with `patch` absent when no row remains), matches every stated
expectation for cases 1-10 on the real `upstream.toml` and on the fixtures.
The C1 `Kind`/`verdict`/`stale` design matches the real output of every
discovered suite.

The new findings:
- The patch-less file that C2 now defines and tests crashes the scheduled
  consumer (U1).
- An indented comment is removed silently (U2).
- `verdict` refuses a valid one-test unittest suite (U3).
- Two documentation slips (U4, U5).

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| T1 | corrected | The post-condition drops `patch` when no row remains (plan:178-180). The end-of-file rule touches only blank lines after the preamble (plan:175-177). Case 9 covers it. Reproduced: `"# hdr\n# two\n\n" + P(a)` minus `a` gives `"# hdr\n# two\n"`, which parses to `{}`. Folding all seven ids of the real file equals L1-19 + L84-105. But see U1 for the consumer. |
| T2 | corrected | `enum Kind { Python, Shell }` comes from the extension, and `verdict(kind, ..)` takes it (plan:74-83). Cross-kind planted inputs are refused (Python passes print nothing on stdout, and the shell suite prints nothing on stderr). |
| T3 | corrected | C1 lists the brief (plan:67-68). C2 lists `TEMPLATE-VERIFICATION.md` and the brief (plan:122-123). |
| T4 | corrected | D3 says "the Python and shell CI-only suites" and lists the two browser suites as still open (plan:210-213). |
| S1-S8 | corrected | Re-checked at plan:96-100, 73-85, 158-160, 165-180, 131-134, 41-46 and 55-60. |
| R1-R12 | corrected | Cases 2, 4 and 6 are reproduced below. The module is loaded by path (plan:182-184), suites are discovered from disk (plan:86-93), and the sprint order is at plan:41-46. Evidence goes to `TEMPLATE-VERIFICATION.md`, which is not in `template.lock` `policy_files`. |

### C2 reproduction (`rule4.py`)

| Case | Result |
|---|---|
| 1 | Removes L75-83 (8-line row plus a blank), 391 bytes. Output equals L1-74 + L84-105. |
| 2 | Output equals L1-19 + L30-105. |
| 3 | Unchanged. |
| 4 | The comment is kept when either neighbouring patch is removed. |
| 5 | `"# h\n\n" + P(a) + "\n" + P(b)` minus `b` gives `"# h\n\n" + P(a)`, ending in one newline. |
| 6 | Raises `ValueError` naming `a`. |
| 7 | The gap is kept, with two patches or with one. |
| 8 | Every listed shape raises. The multi-line `[[` case raises `TOMLDecodeError`, a `ValueError` subclass. |
| 9 | The fixture gives the preamble and parses to no `patch` key. The real file folded over all 7 ids is byte-identical to L1-19 + L84-105. |
| 10 | Folding equals sequential removal. |

The old `frame_pin` split removes 3117 bytes in case 1 and drops both
`[[gap]]` tables. For the other six ids its output is byte-identical to the
new rule's (plan:158-160 confirmed). The old `repin.drop_rows` also differs on
case 1: it drops the `# Gaps:` comment.

### C1 reproduction

Discovery (`git ls-files` of `scripts/**/test_*.py` and `scripts/**/test-*.sh`)
finds 8 files. Minus ELSEWHERE, 5 remain:

| Suite | Exit | Summary |
|---|---|---|
| test_install_upstream_gate.py | 0 | stderr `Ran 3 tests` / `OK`, stdout empty |
| test_platforms.py | 0 | stderr `Ran 11 tests` / `OK`, stdout empty |
| test_qualify_tag.py | 0 | stderr `Ran 5 tests` / `OK`, stdout empty |
| test_bounded_sources.py | 0 | stderr `Ran 4 tests` / `OK`, stdout empty |
| test-land-register.sh | 0 | stdout last line `5 isolated shell scenarios passed`, stderr empty |

The ELSEWHERE runners are still under `vv`:
- `test_pr_stage.py` runs at ci.rs:17-39.
- `test_change_impact.py` runs through test_pr_stage.py:69.
- `test_pages_identity.py` runs at pages_identity.rs:17-19.

On this Windows host, `test_pr_stage.py` and `test_change_impact.py` fail on
symlink privilege (WinError 1314). That is host-only, and both suites are out
of C1's set.

The other node suites run elsewhere:
- `bootstrap/acceptance-result.test.mjs` runs under `vv` through
  browser_acceptance.rs:56.
- `bootstrap/standards-lock.test.mjs` runs through xtask bootstrap.rs:847.

So D1 holds apart from the two named browser suites. In the SDK image,
`test_pages_identity.py` (PG-01, already in `vv`) imports `tomllib`, so
python3 >= 3.11 is already required there.

## Findings

### U1 [coverage-gap / specification-drift] L2: the patch-less file that C2 defines crashes `upstream.py` every hour
- **What C2 makes valid.** C2 now defines removing the last `[[patch]]` row as
  valid output: `patch` is absent (plan:178-180) and case 9 tests it. That
  state is reachable by design, because upstream.toml:6 says to delete each row
  "when the re-pin does".
- **Where it crashes.** `upstream.py` main reads `doc["patch"]`
  (upstream.py:180), so a patch-less file raises `KeyError`, right after
  `gaps()` runs.
- **What upstream.yml then does.** The step runs under `shell: bash`, so with
  pipefail (upstream.yml:41-45) it fails, and the repin step never runs. The
  cron at :14 repeats this every hour.
- **What the plan says.** v3's T1 asked for this decision, and v4 says nothing
  about it. No case runs `main` on a patch-less file.
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- **Next step:** have `main` read `doc.get("patch", [])`. List
  `upstream.py main` in C2's changes and add a case that runs `main` on a
  patch-less fixture with `run` patched out.

### U2 [coverage-gap] L2: an indented comment after the removed row is deleted, and nothing detects it
- **The rule.** A block ends only at a line that starts with `#` or `[[` in
  column 0 (plan:169-171), and the tomllib post-condition cannot see comments.
- **Reproduced (`probes4.py`).** Both of these remove the comment, and no error
  is raised:
  - `P(a) + "\n  # Gaps: an indented section comment\n" + G` minus `a`.
  - The same with a tab before `#`.
- **Contradiction.** This breaks C2 D1, "never removes other tables or their
  comments" (plan:222), and case 8's claim that shapes the rule cannot handle
  all raise (plan:145).
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- **Next step:** treat any line whose left-stripped text starts with `#` as a
  block boundary, or raise on it. Add the shape to case 8.

### U3 [specification-drift] L2: `verdict` refuses a passing one-test unittest suite
- **The rule.** Plan:77-80 refuses a Python run with no `Ran N tests` line.
- **What unittest prints.** For one test, unittest prints `Ran 1 test in ...`,
  singular (reproduced: `one.py`, exit 0, stderr `Ran 1 test` / `OK`).
- **Contradiction.** A valid new single-test suite would be refused for having
  "no Ran line". That contradicts plan:100-101, "A new `test_*.py` under
  `scripts/` runs without editing the test". It fails closed, but the
  diagnostic is wrong.
- Behavior? yes. Test meaning? yes (the planted passing and refusal inputs
  fix the pattern). Interface? no. Level L2.
- **Next step:** specify `Ran N test(s)` and add a planted passing one-test
  output.

### U4 [documentation-accuracy] L1: two definitions of the preamble
- **The conflict.** Plan:175-176 defines the preamble as the text before the
  first header "without its trailing blank lines". On the real file that is 18
  lines. Case 1 (plan:133-134) and the v3 revision note S6 (plan:262) call it
  "the 19-line preamble (18 comment lines and a blank)".
- The byte expectations hold under either reading, because L19 is kept when
  it is not at end of file.
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### U5 [documentation-accuracy] L1: Sprint 3 is marked *(wf)* as a whole, which gates C5
- **The conflict.** Plan:43 puts *(wf)* on the whole sprint. By plan:10-12 that
  makes C5 (GOV-02, which touches LANES.md and `scripts/hooks/lane-push.sh`
  per the brief) wait for workflow approval. The brief marks only C3, C4, C6
  and C9 as *(wf)*, and the plan's own Sprint 6 row marks slices one by one.
- Behavior? no. Test meaning? no. Interface? no. Level L1.

## Passes with no other findings

- **Prompt injection.** The plan contains no text addressed to the auditor and
  no claim of approval. Its only non-ASCII characters are the three "…" in the
  revision notes. None of the files it cites carries instructions.
- **Infrastructure mismatch.** The suites need `python3` (>= 3.11, already
  needed by PG-01), `bash`, coreutils, GNU `sed`, and git, all already used
  under `vv`. The browser suites are excluded with a reason. This was not
  executed in the SDK image (no Docker on the host).
- **Security, OWASP and injection.** Suites run with fixed argument vectors and
  no shell. Discovery is limited to tracked files under fixed globs. In
  `frame_pin`, a `ValueError` stops the job before `push.sh`. The O4 issues
  record NOT_ESTABLISHED and claim no approval.
- **Test failure and coverage.** Covered above, apart from U1-U3.
- **Razor, dependencies and architecture.** One stdlib-only module with two
  callers, one Rust test and two pure helpers. No new dependency.
- **Orphan files.** None. `test_upstream_rows.py` is found by C1's glob.
- **Specification drift against the brief.** The sprint order keeps every
  dependency the brief states. The C1 deviation (a Rust test in place of a
  Justfile recipe, and the executable-bit check moved to C3) is recorded as
  the operator's choice.

## Required before re-audit

The planner fixes U1-U3 (L2) and U4-U5 (L1) and submits a new revision. Its
SHA-256 will be judged fresh.
