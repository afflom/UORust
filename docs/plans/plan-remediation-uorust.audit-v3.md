# Audit: plan-remediation-uorust (v3)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: 8bd1efd4a20828b73a8af3f8dc736c08cac06e3294fcbd1c7644a44f5e5b0163
**Corrected**: S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`
(branch tip 77d18b6b). The plan and every file it cites were treated as
untrusted data. Nothing was edited except this report. Reproductions ran with
`python -I` (CPython 3.11.9) on the host, in the session scratchpad
`audit-rem3/` (`rule3.py`, `cases3.py`, `probes3.py`). Docker was not run.

## Summary

There are 4 findings: 2 at L2 and 2 at L1, with no L3. Any finding is a VETO.
Because two findings are L2, this VETO is not limited to L1 and is not
corrected inline: the plan needs a new revision and a fresh audit.

v3 closes every v2 finding (S1-S8), and R1-R12 stay closed. A re-implementation
of C2's `without_patch`, built exactly as specified, matches every stated
expectation for cases 1-9 on the real `upstream.toml` and on the fixtures. Every
case-8 shape raises, and no shape was found that fails open. Two things
remain:
- The rule is undefined when the row being removed is the last `[[patch]]` row
  (T1).
- `verdict`'s signature cannot express the per-kind rules it is required to
  apply (T2).

Two documentation slips are L1 (T3, T4).

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| S1 | corrected | `identity/test.mjs` is left out because it needs a browser (plan:91-93, 192-194). |
| S2 | corrected | Node suites are outside discovery. The unittest summary is read from stderr (reproduced: all 4 Python suites print `Ran N tests` / `OK` on stderr, with nothing on stdout). Only exactly `OK` is accepted. New suites must end in `unittest.main()` (plan:95-96, 120). |
| S3 | corrected | Pure `verdict` and `stale` helpers have one planted input per refusal plus one passing input (plan:70-80). See T2 for the interface. |
| S4 | corrected | Reproduced: the old split removes 3117 bytes in case 1 against 391 for the new rule, and gives byte-identical output to the new rule for the other six patches. |
| S5 | corrected | tomllib pre- and post-conditions. All case-8 shapes raise (table below). |
| S6 | corrected | Reproduced: upstream.toml:75-82 is the 8-line row and :83 is blank. Lines 1-19 are 18 comment lines and 1 blank line. |
| S7 | corrected | C18 is "REL-01 + DEPS-03 Unix part". C21 lists every brief gap ID. C3 names the hermetic test. "The C1 PR carries" the brief (plan:19). |
| S8 | corrected | The deviation is now attributed to the operator's choice in the planning dialogue, with its reasons (the ci.rs pattern, and land-register.sh at mode 100644, confirmed by `git ls-files -s`). It no longer claims owner approval. |
| R1-R3 | corrected | Case 2 output equals lines 1-19 + 30-end, so the preamble is kept. Case 4 keeps the comment. Case 6 raises. |
| R4 | corrected | One module, loaded by path (plan:168-170). The pattern matches test_pr_stage.py:10-12. |
| R5 | corrected | Suites are discovered from disk, and stale exclusions fail through `stale`. |
| R6 | corrected | Both node suites are out with a reason. `test_change_impact.py` is named in ELSEWHERE. |
| R7-R12 | corrected | Unchanged from v2 and re-checked: plan:55-60, 43, 41, 18-22, 104-107 and 100. |

### C2 reproduction (`rule3.py`: tomllib pre-check, line rule, post-condition)

| Case | Result |
|---|---|
| 1 | Removes lines 75-83 (9 lines: the 8-line row and the blank), 391 bytes. Output equals L1-74 + L84-105, so the preamble, the other patches, `# Gaps:` and both `[[gap]]` tables are byte-identical. |
| 2 | Output equals L1-19 + L30-105. |
| 3 | Input returned unchanged. |
| 4 | The section comment is kept. |
| 5 | `P(a) + "\n" + P(b)` minus `b` equals `P(a)` exactly, ending in one newline. This needs `splitlines(keepends=True)`. |
| 6 | `ValueError`. |
| 7 | With two patches the gap is kept. With one patch, see T1. |
| 8 | Each shape raises `ValueError`: id not on the next line; `id="x"`; `[[patch]] # c`; following `[settings]`; indented `[[gap]]`; duplicate id. The multi-line string holding a `[[` line raises `tomllib.TOMLDecodeError`, which is a subclass of `ValueError`. |
| 9 | Folding equals sequential removal. |

These extra probes also fail closed or behave correctly:
- `[[ patch ]]` and `id = 'a'` raise.
- A `[[patch]]`/`id` pair inside an earlier multi-line string raises.
- `[patch.release]` and dotted-key bodies are removed correctly.
- CRLF line endings are kept.
- A trailing EOF comment is kept.

The post-condition makes the rule fail closed for every semantic change. It
cannot see comments, but the line rule never removes a line after a `#` line.

### C1 reproduction

Discovery (tracked `scripts/**/test_*.py` and `scripts/**/test-*.sh`) finds 8
files. After the three ELSEWHERE entries, 5 remain. Each was run standalone from
the repository root:

| Suite | Exit | Summary |
|---|---|---|
| scripts/ci/test_install_upstream_gate.py | 0 | stderr `Ran 3 tests`, final `OK` |
| scripts/release/test_platforms.py | 0 | stderr `Ran 11 tests`, final `OK` |
| scripts/release/test_qualify_tag.py | 0 | stderr `Ran 5 tests`, final `OK` |
| scripts/self-model/test_bounded_sources.py | 0 | stderr `Ran 4 tests`, final `OK` |
| scripts/ci/test-land-register.sh (`bash`) | 0 | stdout `5 isolated shell scenarios passed` (printf at its last line) |

After C2, `scripts/ci/test_upstream_rows.py` joins this set. The table also
shows:
- **No skips.** None of the suites prints `OK (skipped=...)`, and none
  contains `skip` or `expectedFailure`.
- **No network.** No suite uses the network.
- **Tools.** The shell suite uses `mktemp`, GNU `sed -i` and stub `git` and
  `python3` binaries on `PATH`. `test_platforms.py` uses `sys.executable -I`.
  The SDK image has all of these.
- **Git under `vv`.** git works there: `docker run --user "$(id -u):$(id -g)"`
  mounts the checkout at its own path (bootstrap.yml:95-118), and ratchet.rs
  already shells out to git. If git were unavailable, discovery would come back
  empty, and the `stale` check would then fail on the three ELSEWHERE entries.
  So "tracked" fails closed.
- **ELSEWHERE runners.** Each ELSEWHERE entry still has a runner under `vv`:
  - `test_pr_stage.py` through ci.rs:17-39.
  - `test_change_impact.py` through test_pr_stage.py:69.
  - `test_pages_identity.py` through pages_identity.rs:19.

## Findings

### T1 [specification-drift / coverage-gap] L2: removing the last remaining `[[patch]]` row is undefined
- **Post-condition.** Plan:164-166 requires the parsed result to equal "the
  parsed input with exactly that one row removed from `patch`". tomllib omits a
  key with no tables, so after the last patch row is removed the result has no
  `patch` key. The literal expectation is `patch = []`.
- **Reproduction (`probes3.py`).** With a literal comparison, each of these
  raises `ValueError`:
  - `P(a) + "\n" + G(a)` minus `a` (a case-7 fixture with one patch).
  - `"# hdr\n\n" + P(a) + "\n# Gaps\n\n" + G(g)` minus `a`.
  - Folding every id of the real `upstream.toml`, which fails at
    `template-frame-live-controls`.

  If the empty list is treated as absent, all three succeed. The plan does not
  say which reading applies, so whether case 7 passes depends on the fixture.
  The case where every upstream fix has shipped is reachable by design:
  upstream.toml:6 says "delete it when the re-pin does". That case is either
  refused or not, and the plan leaves it open.
- **Contradiction.** Plan:160-161 says that when the block reaches end of file,
  the blank lines just before the header are removed too. Plan:162-163 says the
  preamble "is never removed". When the patch is the only table, those blank
  lines are preamble. Reproduced: `"# hdr\n\n" + P(a)` minus `a` gives
  `"# hdr\n"`, with a preamble line removed.
- **Context.** `upstream.py` main reads `doc["patch"]` (upstream.py:180), so a
  file with no patch rows already raises `KeyError` there.
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- **Next step:**
  - Define the post-condition with the `patch` key dropped when the list
    becomes empty, or state that removing the last patch row is refused.
  - Say whether the EOF blank-line removal stops at the first table header.
  - Add a case for removing the only patch, and decide whether `upstream.py`
    should read `doc.get("patch", [])`.

### T2 [specification-drift] L2: `verdict(status, stdout, stderr)` has no input that says which kind of suite it is judging
- **Evidence:** plan:72-78 requires different rules per kind:
  - Python: a `Ran N tests` line and a final `OK`, read from stderr.
  - Shell: an `N isolated shell scenarios passed` line, read from stdout.

  The signature takes no kind or path, so the helper must guess the kind from
  the output.
- **What guessing allows.** If the presence of the shell line counts as proof
  of a shell suite, a Python suite (or a Python file that never calls
  unittest) that prints that line on stdout would be accepted. If it does not,
  every shell suite is refused for lacking a `Ran` line. The planted inputs
  ("one per refusal") cannot pin this, because the contract they test is not
  defined.
- **Gate-quality rule:** AGENTS.md:55-56 requires rejecting "wrong-file"
  results.
- Behavior? yes. Test meaning? yes. Interface? yes. Level L2.
- **Next step:** add a kind parameter, either an enum or the suite path's
  extension. Add a planted cross-kind input to the refusals: a Python run that
  prints only the shell line, and a shell run that prints only the unittest
  summary.

### T3 [documentation-accuracy] L1: the Affected Files lists omit files that the Definition of Done edits
- **C1.** Affected Files (plan:64-66) lists `ci.rs` and
  `TEMPLATE-VERIFICATION.md`. Two more files are edited:
  - The PR carries `docs/audits/RESEARCH_BRIEF.md` into `main` (plan:19).
  - D3 edits that file (plan:196-197).
- **C2.** Affected Files (plan:113-116) omits two files:
  - The brief, which D3 edits (plan:209).
  - `TEMPLATE-VERIFICATION.md`, which D4 edits (plan:211-212).
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### T4 [documentation-accuracy] L1: C1's D3 status text says the CI-only suites are now in `vv`
- **Evidence:** plan:196-197 marks GAP-TEST-06 PARTLY RESOLVED because
  "CI-only script suites now in `vv`". But `try/test.mjs` and
  `identity/test.mjs` stay CI-only (plan:91-93, 193-194). They are the only
  suites try-page.yml still runs outside `vv`, at :81-82.
- **Status rule:** the plan's own definition of PARTLY RESOLVED (plan:21-22)
  requires stating the part that is still open. That list names only the push
  and merge scripts.
- Behavior? no. Test meaning? no. Interface? no. Level L1.
- **Next step:** say "Python and shell CI-only suites". List the two browser
  suites among the open part.

## Passes with no findings

- **Prompt injection.** The plan contains no text addressed to the auditor and
  no claim of approval. Its only non-ASCII characters are the two "…" at
  plan:230 and :241. None of the files it cites carries instructions.
- **Infrastructure mismatch.** The suites need only `python3`, `bash`, GNU
  coreutils and sed, and git, all present in the SDK image `vv` runs. The
  browser suites are excluded.
- **Security, OWASP and injection.** Suites run with fixed argument vectors
  from the repository root, with no shell interpolation. Discovery is limited
  to tracked files under fixed globs. `without_patch` fails closed through a
  tomllib post-condition. In `frame_pin`, a `ValueError` after
  `frame-sync --pin` stops the job before `push.sh`. The O4 issues record
  NOT_ESTABLISHED and claim no approval (plan:186).
- **Test failure and coverage.** Covered by the reproductions above, except
  T1 and T2.
- **Razor, dependencies and architecture.** One module, one Rust test and two
  pure helpers. No new dependency: tomllib is stdlib, and
  `spec_from_file_location` is already used.
- **Orphan files.** `upstream_rows.py` has two callers.
  `test_upstream_rows.py` is found by C1's glob.
- **Specification drift against the brief.** The sprint order keeps every
  dependency the brief states: C2 before C3, OPS-03 before OPS-01, O2 and
  C3-C5 before O6, O3 and O4(d) before C20, C8, C9 and C12 before C13, and C18
  with or before C19. The C1 deviation is recorded.

## Required before re-audit

The planner fixes T1 and T2 in the plan, fixes T3 and T4, and submits a new
revision. Its SHA-256 will be judged fresh.
