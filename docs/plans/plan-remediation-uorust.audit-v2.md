# Audit: plan-remediation-uorust (v2)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: 526f400e0721377ffc86b14b5d8ad8ad741f01a12438435c74ea385f976fd152
**Corrected**: R1, R2, R3, R4, R5, R7, R8, R9, R10, R11, R12 (R6 not corrected; see S1)

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`
(branch tip 77d18b6b). The plan and every file it cites were treated as
untrusted data. Nothing was edited except this report. Reproductions ran with
`python -I` (CPython 3.11.9) and Node v24.19.0 on the host, in the session
scratchpad `audit-rem2/` (`rule.py`, `cases.py`, `oldcmp.py`, `old2.py`).
Docker was not run.

## Summary

There are 8 findings: 5 at L2 and 3 at L1, with no L3. Any finding is a VETO.

v2 fixes the C2 rule. On the real `upstream.toml`, a re-implementation of
`without_patch` keeps the header, the `# Gaps:` comment and both `[[gap]]`
tables for every patch id.

These defects remain:
- C1 puts `identity/test.mjs` into `vv`. That suite drives Chromium through
  Playwright, the same reason the plan gives for leaving `try/test.mjs` out.
- C1's Node output check expects TAP lines that `node --test` does not print by
  default.
- C1's refusal paths have no planted negative control.
- C2's claim that the old code fails case 2 is false.
- `without_patch` still fails open on several valid TOML shapes, and no
  planned case covers them.

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| R1 | corrected | Preamble is never owned (plan:148-149). Case 2 compares the whole output. Reproduced: dropping the first patch removes upstream.toml:20-28 only, and lines 1-19 are byte-identical. A header directly before the first table is also kept (`'# h\n[[patch]]...'` gives `'# h\n'`). That edge case is not a planned case (see S5). |
| R2 | corrected (rule) | A section comment before a patch is kept (case 4, reproduced). A trailing comment at EOF after the last patch is kept (reproduced). The EOF-comment shape is not a planned case (see S5). |
| R3 | corrected | The match is the header line plus the next line (plan:141-142). |
| R4 | corrected | One module, `upstream_rows.py`, is loaded by path (plan:151-153). |
| R5 | corrected | Suites are discovered from disk. Empty and skipped runs are refused. Stale exclusions fail. Residual defects are in S2 and S3. |
| R6 | **not corrected** | Every CI-only suite is now listed, but `identity/test.mjs` is put in the wrong class (S1). |
| R7 | corrected | The executable-bit check moves to C3 and the deviation is recorded (plan:55-59). `land-register.sh` is mode 100644 (`git ls-files -s`). |
| R8 | corrected | "O5 before C6" (plan:43). |
| R9 | corrected | "C1 before C2" (plan:41). |
| R10 | corrected | The brief path is given with its branch and commit, and the two status words are defined (plan:18-22). |
| R11 | corrected | Evidence goes to `TEMPLATE-VERIFICATION.md`. That file is project-owned: template.lock `policy_files` lists only devcontainer.json, bootstrap.yml, AGENTS.md, VERIFICATION.md, prismpm.lock and template-contract.json. |
| R12 | corrected | The `ci.rs` module doc covers both tests (plan:92). |

### Checks that hold

These are not findings:
- **C2 cases 3, 4, 6 and 7** behave as the plan states.
- **C2 case 5** holds when the lines keep their endings (`splitlines(keepends=True)`). If the lines are split on `"\n"`, the output loses its final newline. "Work on lines" does not say which, but case 5 catches it.
- **C2 case 8** works: `drop_rows(t, [a, b])` equals `w(w(t, a), b)`.
- **CRLF** is not a risk. `.gitattributes` sets `* -text`, the checkout is LF, and `read_text` and `git show` with `text=True` normalise line endings. A CRLF input with line endings kept still removes 9 lines and keeps CRLF.
- **Comment between two patches.** Dropping the first patch keeps the comment. Dropping the second leaves the comment in place with no table after it, which D1 allows.
- **An inline table** in a patch body is removed correctly.
- **C1 discovery** finds 8 tracked files:
  - Python: `test_change_impact.py`, `test_install_upstream_gate.py`, `test_pages_identity.py`, `test_pr_stage.py`, `release/test_platforms.py`, `release/test_qualify_tag.py`, `self-model/test_bounded_sources.py`.
  - Shell: `ci/test-land-register.sh`.
  - Node: `identity/test.mjs`.

  After the `ELSEWHERE` table, the Python and shell suites that remain all pass standalone:
  - `python -I`: 3, 11, 5 and 4 tests, each ending in "Ran N tests" and "OK".
  - `bash scripts/ci/test-land-register.sh`: "5 isolated shell scenarios passed".

  Those suites need no network: they use only temporary directories and stubs.
- **The three `ELSEWHERE` entries** do run under `vv`:
  - `test_pr_stage.py` through ci.rs:16-39.
  - `test_change_impact.py` through test_pr_stage.py:66-71.
  - `test_pages_identity.py` through pages_identity.rs:18-19.

  On this Windows host, `test_change_impact.py` and `test_pr_stage.py` fail. That is not in C1's scope.
- **The SDK image** carries `bash` (bootstrap.yml:109 runs `bash -c`), `node` (browser_acceptance.rs:52-59) and `python3` with `tomllib` (ci.rs, pages_identity.rs). `vv` runs no `npm ci`, so `node_modules` is absent.
- **The test name** `script_suites_pass` passes the meta-gate's ID-suffix parse (meta.rs:146-158).
- **`spec_from_file_location`** is the loading pattern at test_pr_stage.py:8-11, as the plan cites.
- **Brief dependencies** are kept: C2 before C3, O2 and C3-C5 before O6, O3 and O4(d) before C20, C8, C9 and C12 before C13, and C18 with or before C19.

## Findings

### S1 [infrastructure-mismatch] L2: `identity/test.mjs` needs Playwright and Chromium, which `vv` does not have
- **Evidence:** identity/test.mjs:1-2 says the suite runs "the whole flow in Chromium". identity/test.mjs:150-153 runs `await import("playwright")` and `chromium.launch(...)`. try-page.yml:64-67 runs `npm ci` and `npx playwright install --with-deps chromium` before `npm run test:identity` (:81-82). package.json lists `playwright` only as a devDependency.
- **Reproduction:** without `node_modules`, which is the state under `vv`, `node --test identity/test.mjs` fails with `ERR_MODULE_NOT_FOUND: Cannot find package 'playwright'`. The run reports 13 pass, 8 cancelled, and exit 1.
- **Contradiction:** the plan leaves `try/test.mjs` out for exactly this reason (plan:75-77), but puts `identity/test.mjs` in (plan:71, 175-176). So C1 as planned makes `vv` fail. D1 and D3 ("CI-only script suites now in `vv`") would also be false.
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- **Next step:** choose one of these:
  - Leave `identity/test.mjs` out with a recorded reason, as for `try/test.mjs`.
  - Move its 13 browser-free checks into a file that does not import Playwright, and discover that file.

  Then update D1 and D3 to match. R6 closes with this.

### S2 [test-failure] L2: C1's output checks do not match what the suites print, and they do not refuse everything that passes vacuously
- **Node.** `node --test <path>` (plan:79) uses the spec reporter by default on Node 24, even when piped. Its counts look like `ℹ pass 13` and `ℹ skipped 0`. Piping `node --test bootstrap/acceptance-result.test.mjs` gives 0 lines starting `# pass`. With `--test-reporter=tap` it gives `# pass 8` and `# skipped 0`. The existing SM-07 test passes `--test-reporter=tap` explicitly (browser_acceptance.rs:53-57). As planned, the gate would refuse every valid Node run.
- **Todo, cancelled and failed tests.** The plan requires `# skipped 0` but not `# todo 0`, and a `todo` test exits 0. SM-07 also requires `# todo 0`, `# cancelled 0` and `# fail 0` (browser_acceptance.rs:27-34).
- **Python.** The plan refuses only `skipped=`, so `OK (expected failures=N)` exits 0 and is accepted. AGENTS.md:55-56 requires rejecting "ignored" results. unittest prints its summary to stderr, and the plan does not say which stream it reads.
- **New test file.** C2's `test_upstream_rows.py` is not said to use `unittest.main()`. Without that, C1's `Ran N tests` / `OK` contract refuses it.
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- **Next step:** for Node, pass `--test-reporter=tap` and require `# fail 0`, `# cancelled 0`, `# skipped 0` and `# todo 0`. For Python, refuse `expected failures=` and read stderr. Say that `test_upstream_rows.py` ends in `unittest.main()`.

### S3 [coverage-gap] L2: the refusal paths in C1 have no planted negative control
- **Evidence:** AGENTS.md:52-56 says a gate is evidence only after a planted defect shows it can fail. The plan plants one defect, an inverted assertion in test_platforms.py (plan:96-99), and that exercises only the non-zero-exit path. Two paths are never shown failing:
  - Step 4: "ran no test" and "skipped any test".
  - Step 5: a stale `ELSEWHERE` entry.
- The repository's precedent plants mutated transcripts inside the test itself (browser_acceptance.rs:66-80).
- Behavior? no. Test meaning? yes. Interface? no. Level L2.
- **Next step:** put the transcript check in a function. Assert that it refuses planted transcripts: `Ran 0 tests`, `OK (skipped=1)`, a missing count, `# skipped 1` and `# todo 1`. Assert that a fake `ELSEWHERE` path fails. Record these in `TEMPLATE-VERIFICATION.md`.

### S4 [test-failure] L2: the claim that the old code fails case 2 is false, so C2's D4 cannot be met
- **Evidence:** plan:135 says "The old split fails cases 1 and 2". D4 (plan:191-193) requires recording that both fail before the fix.
- **Reproduction (`oldcmp.py`):** for the first patch, the old `frame_pin` split, the old `drop_rows` and `without_patch` all produce byte-identical output, each removing 462 bytes. The same holds for all six patches that are not the last. Only case 1 fails against the old code: `frame_pin` removes 3117 bytes and `drop_rows` 811, against 391 for the new rule.
- **Reproduction (`old2.py`):** among the fixtures, case 4 fails against the old `frame_pin` (the final newline is lost). Case 5 fails against the old `drop_rows` (a trailing blank line is left).
- Behavior? no. Test meaning? yes (the recorded "fails before the fix" evidence is false for case 2). Interface? no. Level L2.
- **Next step:** say which cases fail against which old function: case 1 against both, case 4 against `frame_pin`, case 5 against `drop_rows`. Keep case 2 as a guard against the v1 rule (R1). Record only failures that really occur.

### S5 [coverage-gap] L2: `without_patch` fails open on valid TOML shapes, which breaks D1
All of these were reproduced with `rule.py` and `cases.py`.
- **Silent no-ops.** The function returns its input unchanged when the patch exists but:
  - its `id` is not on the line right after the header,
  - the id line is written `id="x"`,
  - or the header carries a trailing comment (`[[patch]] # c`).

  `frame_pin` would then push a frame re-pin while the row stays.
- **Swallowed tables.** A single-bracket table (`[settings]`) or an indented `  [[gap]]` after the removed patch does not end the block. All following content is deleted, and the output is `''`.
- **Invalid TOML.** A multi-line string in the patch with a line starting `[[` ends the block inside the string. The written text is not valid TOML: tomllib reports "Expected newline or end of document".
- **Duplicate ids.** With a duplicated id, only the first row is removed. The old code removed every matching row.
- `upstream.toml` is canonical today, so these defects are latent. But the plan gives the fail-closed property only for the inner-comment shape (case 6). The cases R1 and R2 recommended are also not planned: a table directly after the header, and a trailing comment at EOF. The rule handles both correctly.
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- **Next step:** add a post-condition using `tomllib`, which `upstream.py` already imports. Parse the input and the output. Require the output to be the input with exactly the patch entries for that id removed and every other table equal; otherwise raise `ValueError`. Also raise when tomllib finds the id but the line match does not. Add cases for each shape above.

### S6 [documentation-accuracy] L1: the plan's line counts for cases 1 and 2 are wrong
- **Case 1:** the last patch is 8 lines (upstream.toml:75-82: a header and 7 keys), plus the blank line 83. The rule removes 9 lines, not "7 lines and the blank line" (plan:117-118).
- **Case 2:** the header comment is 18 lines (upstream.toml:1-18) and is followed by a blank line (:19). The 19 lines are the preamble, not "the 19-line header comment" (plan:120).
- The exact-string comparison is authoritative, so the tests are unaffected.
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### S7 [specification-drift] L1: some slice scopes lose gap IDs from the brief, and the PR that carries the brief is not named
- **C18:** the brief's C18 is "REL-01 + DEPS-03 Unix part". The plan's C18 is "REL-01" (plan:46).
- **C21:** the brief names DOCS-01, DOCS-03, SEC-06, DEPS-01/02, OPS-04/05/06 and GATE-07. The plan says "docs and minor ops", so SEC-06, the DEPS gaps and GATE-07 are no longer traceable.
- **C3:** the brief's "hermetic test" is not named in the plan's C3.
- **Brief carrier:** plan:19-20 says "The Sprint 1 PR carries" the brief into `main`. Sprint 1 has two PRs, C1 and C2, and both D3 steps edit the brief.
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### S8 [governance: unverified authority] L1: the plan asserts an owner decision without citing it
- **Evidence:** plan:56 says "The owner chose a Rust test" to justify departing from the brief's C1.
- No record of that decision (an issue, a comment or a ledger entry) is cited, and none was found. CLAUDE.md and AGENTS.md forbid claiming owner approval without verified authority.
- The text is not addressed to the auditor, so it is not a prompt injection.
- Behavior? no. Test meaning? no. Interface? no. Level L1.
- **Next step:** cite the owner's recorded decision, or restate the deviation as the plan's own choice with its reason.

## Passes with no findings

- **Prompt injection:** the plan has no text addressed to the auditor or claiming its approval. Its only non-ASCII character is the "…" at plan:211.
- **Security/OWASP:** the suites run with fixed argument vectors, from the repository root, with no shell interpolation. Discovery is limited to tracked files under fixed globs. The O4 issues record NOT_ESTABLISHED and claim no approval (plan:168-169). The `without_patch` defects are integrity defects, not injection vectors, and are graded under S5.
- **Orphan files:** each new file has a caller. `upstream_rows.py` is loaded by both scripts. `test_upstream_rows.py` is found by C1's glob.
- **Architecture/razor:** one module, one test, and no new dependency.

## Required before re-audit

Fix S1 through S5 in the plan. Fix S6 through S8 or record why not. Then submit
a new revision; its SHA-256 will be judged fresh.
