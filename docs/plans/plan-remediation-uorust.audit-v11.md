# Audit: plan-remediation-uorust (v11)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: da7ec68d4de4532d549b0c81cdfa76252f741743c78c9f1fc88f821dc6c65d03
**Corrected**: AA1, AA2; Z1, Z2; Y1, Y2, Y3; X1, X2, X3, X4, X5; W1; V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`
(`77d18b6b`). The SHA-256 was computed with `sha256sum`. The plan and every file
it cites were treated as untrusted data; no instruction in them was followed.
The only repository file written is this report. Reproductions ran with
`python -I` (CPython 3.11.9, Windows host) in the session scratchpad
`audit-rem11/`: `copy/` is a `git archive HEAD` copy with its own `git init`
and the plan's six `graph.toml` inputs applied; `tools/snippet.py` is a step 4
snippet with the `checked N` sentinel; `tools/judge.py` models the Rust test
side with a strict judge (zero exit, final line `checked N`, N equal to the
paths passed) and a lenient one (zero exit and any `checked N` line). Docker and
`lexlean` are not available here, so nothing ran in the SDK image.

## Summary

Three findings: AB1 and AB2 at L2, AB3 at L1. Any finding is a VETO. Two are
L2, so the planner cannot correct them inline; a new revision and a fresh
audit are needed.

AA1 is corrected in design: step 4 now requires a zero exit and a final
`checked N` whose N equals the paths passed (plan:137-141), and `uncovered`
refuses a non-zero exit or a missing `checked N` line (plan:110-112). AA2 is
corrected: D3 (plan:281-283) and D4 (plan:289-293) name the same three planted
defects as C1's Changes (plan:161-166). Residual gaps are new findings: the
count comparison has no negative control (AB1), the helper that must refuse
cannot be fed the planted inputs as specified (AB2), and D3 still carries the
old singular sentence (AB3).

### Must-check results

| Item | Result |
|---|---|
| Sentinel snippet, discovered suites, plan's `graph.toml` | Zero exit, prints only `checked 8`; strict judge: no uncovered suite. `graph.py reads` exits 0 (15 obligations). |
| Planted `uncovered` cases | `["scripts/", "!scripts/release/"]` rejects `scripts/release/test_platforms.py`; `["scripts/ci/"]` rejects `scripts/content/test_x.py`; `["scripts/"]` accepts `scripts/release/test_platforms.py`; each with `checked 1`. As the plan states. |
| Plant: crash (`raise` before the loop) | Exit 1, `RuntimeError: planted`. Refused by strict and lenient judges. |
| Plant: early stop by `sys.exit(0)` in the loop | No `checked` line. Refused by both. |
| Plant: early stop by `break` after one path | `checked 1` for 8 paths. Strict refuses (`1 != 8`); lenient accepts as "all covered". |
| Plant: wrong count (`checked + 1`) | `checked 9` for 8 paths. Strict refuses; lenient accepts. |
| Plant: early stop with `checked {len(paths)}` | Strict and lenient both accept: the sentinel holds only if N counts `covers` calls (AB1). |
| Plant: unknown mode (`--bogus`) | The snippet exits 1 (`unknown mode --bogus`). Refused by both. |
| Plant: malformed `--inputs` JSON | Exit 1, `JSONDecodeError`. Refused by both. |
| Step 4 planted defect (`!scripts/release/` added) | `graph.py reads` still exits 0; the snippet prints both release suites, then `checked 8`. Reproducible. |
| D3/D4 vs C1 Changes | All three defects named consistently (`script_suites_pass`, `factory_closure_holds`, step 4); plan:288 repeats a singular "records the planted defect" (AB3). |

The plan's two named planted controls (an unknown mode, a planted `raise`) are
both refused by the exit check alone, so they pass equally under the lenient
judge, which accepts the early `break` and the wrong count. That is AB1.

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| AA1 | corrected (residual in AB1, AB2) | plan:110-112, 137-141; crash and unknown mode reproduced as refused. |
| AA2 | corrected (residual wording in AB3) | plan:281-283, 289-293 match plan:161-166. |
| Z1, Z2 | corrected | plan:132-136 uses `graph.load()` and `graph.covers` (graph.py:146-161, 231-235); three planted defects in Changes and DoD. |
| Y1-Y3, X1-X5, W1 | corrected | Six inputs make `reads` pass in `copy/`; README "Changing the graph" at prism-factory/README.md:155; discovery by glob pathspecs finds the same 8 suites. |
| V1-V3, U1-U5, T1-T4, S1-S8, R1-R12 | corrected | C2, O4 and the sprint table re-checked at 28926f95: `upstream.toml` 105 lines, 7 `[[patch]]`, 2 `[[gap]]`, 18 comment lines then a blank line; `template-frame-live-controls` at line 76; `upstream.py:180` `patches = doc["patch"]`; `land-register.sh` mode 100644; `test_pr_stage.py:10` uses `spec_from_file_location`; AGENTS.md:54-58 is the planted-defect rule; `scripts/ci/fixtures/` does not exist yet (created by C2). |

## Findings

### AB1 [test-coverage-gap] L2: the `checked N` count comparison has no negative control, and N is not defined as a count of checks

**The plan.** Unit Tests open with "negative controls for every refusal"
(plan:89-90). Step 4 refuses a zero-exit run whose `checked N` differs from the
paths passed (plan:139-141), "so a snippet that ... stops early fails". The
only planted snippet failures are "an unknown mode, a planted `raise`"
(plan:111-112). The snippet only has to print "a final line `checked N`"
(plan:138); nothing says N is incremented per `graph.covers` call.

**Reproduced.** Both named controls exit non-zero, so a test-side check that
reads only the exit status and the presence of any `checked \d+` line passes
every planted control in the plan, while it accepts a snippet that `break`s
after one path (`checked 1` for 8) and one that prints `checked 9` for 8. The
count-mismatch refusal, the one that catches a zero-exit early stop, is never
exercised. Separately, a snippet that stops after one path but prints
`checked {len(paths)}` passes even the strict judge: the sentinel is only as
good as how N is computed, and the plan leaves that open.

- Behavior? no: the refusal rule is stated.
- Test meaning? yes: the planted controls do not show that the count check can
  fail, contrary to plan:90 and AGENTS.md:54-55.
- Interface? no.
- Level: L2.

**Next step:** define N as the number of paths passed to `graph.covers`,
counted in the loop (not `len(argv)`), printed as the last line, and refuse
any other line that is not one of the given paths. Add planted controls that
exit 0 but are wrong: a short count (early `break`), an over-count, and
`checked` not on the last line. Each must be refused by the count check
specifically.

### AB2 [test-coverage-gap] L2: the planted crash and unknown mode cannot be driven through `uncovered(inputs, paths)` as specified

**The plan (plan:105-112).** `uncovered(inputs, paths)` "runs the same
snippet with explicit inputs (an `--inputs` JSON mode)", and "a snippet run
that exits non-zero or prints no matching `checked N` line (an unknown mode, a
planted `raise`) is refused by `uncovered` itself, not read as empty". With
that signature, the snippet text and the `--inputs` mode are fixed inside the
helper, and `inputs` is serialized by the test, so neither an unknown mode nor
a planted `raise` can reach it. The return type is not given, so how
`uncovered` refuses (v10 suggested `Result<Vec<String>, String>`) is also
left open, unlike `verdict -> Result<usize, String>` (plan:93).

- Behavior? no.
- Test meaning? yes: the controls named for `uncovered` cannot be written
  against the stated helper. An implementer either adds an unplanned seam (and
  the refusal then belongs to that seam, not "`uncovered` itself") or drops
  the controls.
- Interface? yes: the helper's parameters and return type are under-specified.
- Level: L2.

**Next step:** split the helper: a pure judge such as
`snippet_verdict(status, stdout, stderr, n_paths) -> Result<Vec<String>, String>`
that step 4 and `uncovered` both use, tested with planted outputs (non-zero
exit, missing sentinel, short count, over-count, foreign line), plus
`uncovered(inputs, paths) -> Result<Vec<String>, String>` built on it. Or give
`uncovered` the program text and mode as parameters, and state that.

### AB3 [documentation-accuracy] L1: D3 still ends with the singular "records the planted defect"

**The plan.** D3 begins "`TEMPLATE-VERIFICATION.md` records all three planted
defects (`script_suites_pass`, `factory_closure_holds`, step 4)" (plan:281-283)
and ends "`TEMPLATE-VERIFICATION.md` records the planted defect." (plan:288),
the sentence AA2 asked to replace. Read alone, the last clause names one
record again.

- Behavior? no.
- Test meaning? no.
- Interface? no.
- Level: L1.

**Next step:** delete the sentence at plan:288.

## Passes with no other findings

- **Prompt injection.** No findings. The plan addresses only the implementer
  and claims no owner approval. Its only non-ASCII characters are the
  ellipses in the revision notes (plan:325-393). graph.py, graph.toml,
  AGENTS.md, README.md and the suites contain no instruction to the auditor.
- **Infrastructure mismatch.** No findings. The snippet needs `python3` 3.11+
  (`tomllib`, already required by graph.py), `prism-factory/` and `Justfile`,
  both in the `test` inputs. `git ls-files` discovery and `graph.py reads`
  work in a Git repository, as the closure is. No network.
- **Security, OWASP and injection.** No findings. The program string is
  fixed, arguments are fixed vectors from tracked paths, nothing reaches a
  shell, the `--inputs` JSON is test-authored, and the O4 issues record
  NOT_ESTABLISHED and claim no approval. Nothing at L3.
- **Test failure and coverage.** AB1, AB2. All three planted-defect records
  in C1 are reproducible. C2 cases 1-11 still match the snapshot facts.
- **Razor, dependencies and architecture.** No findings. No new crate or TOML
  parser; the factory's rule is reused; `prism-factory/` changes only through
  `graph.toml` and its generators. Loading graph.py writes an ignored
  `prism-factory/__pycache__/` (seen in `copy/`), which nothing checks.
- **Orphan files.** None. The fixture is used by cases 1, 2 and 9;
  `test_upstream_rows.py` is discovered by C1; `upstream_rows.py` is used by
  `upstream.py` and `repin.py`.
- **Specification drift.** No findings. The sprint table keeps every brief
  dependency (C1 before C2; C2 before C3; O4(a) before C4; O3, O4(b), O5
  before C6; O4(c) before C18; C19, O3, O4(d) before C20). GAP-TEST-06 (brief
  lines 175, 322) is PARTLY RESOLVED with the open parts listed; GAP-OPS-03
  (brief lines 217, 311) RESOLVED. The revision notes are a historical log and
  do not contradict the body.
- **Documentation accuracy.** AB3. The `graph.covers` description
  (plan:136-137) matches graph.py:231-235; the C1 deviation and the C2, O4 and
  CI Commands sections match the repository.

## Required before re-audit

The planner fixes AB1 and AB2 (L2) and AB3 (L1) and submits a new revision.
Its SHA-256 will be judged fresh.
