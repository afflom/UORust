# Audit: plan-remediation-uorust

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: 2434b2dcb46dff466557681c2de17839f8d8e4082409a8052b7c4a68eb9f03de

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`.
The plan was treated as untrusted data and was not edited. The Python
reproductions ran with `python -I` (CPython 3.11.9) in the session scratchpad
(`audit-rem/repro.py`, `audit-rem/edge.py`).

## Summary

The plan has 12 findings: 6 L2 and 6 L1. There are no L3 findings. Any finding
is a VETO.

The main defect is in C2. The block rule, as written, gives the header comment
of `upstream.toml` to the first `[[patch]]` table, so dropping that patch
deletes the header. It also deletes a section comment that sits right before a
patch row, and a comment at the end of the file. Read literally, the rule
matches nothing at all. The planned cases cannot catch these defects, and one
planned case contradicts the rule. C1 checks only exit status, uses a
hand-written list, and does not cover the CI-only Node suites, so its D1 claim
is not enforced. The plan also drops the brief's executable-bit check without
assigning it to any slice.

These claims were checked and hold, so they are not findings:
- Every file in the C1 `SUITES` list exists.
- Each of those suites passes standalone with `python -I <path>` (11, 5, 3 and
  4 tests), and `bash scripts/ci/test-land-register.sh` passes 5 of 5.
- `test-land-register.sh` stubs `git` and `python3` on its `PATH`
  (test-land-register.sh:10-26) and resolves its root from `$0` (:3), so it
  needs no network and works from any working directory.
- None of those suites runs under `vv` today. They run only in try-page.yml:62-78.
- `test_pr_stage.py` runs under `vv` through ci.rs:16-39, and
  `test_pages_identity.py` through pages_identity.rs:16-29.
- `-I` adds no script directory to `sys.path` (3.11: `sys.flags.safe_path` is
  True). The SDK's Python is at least 3.11, because `test_pages_identity.py:17`
  imports `tomllib` and already runs under `vv`.
- upstream.py:102-110 (`frame_pin`) and repin.py:45-50 (`drop_rows`) work as the
  plan describes.
- The old split removes exactly 3117 of the 8002 bytes of `upstream.toml`,
  including both `[[gap]]` tables.
- The package is named `repo-conformance` (crates/conformance/Cargo.toml:2).
- The meta-gate accepts a test name with no ID suffix: meta.rs:144-163 checks
  only names that end in a registered prefix plus two digits.
- `frontier.toml` records only library functions (`repo_conformance::meta::*`,
  `runner::*`), not integration tests.

Not verified: the tool inventory of the SDK image (GNU `sed -i`, `mktemp`,
`seq`). Docker was not run. Q2 covers this case.

## Findings

### R1 [test-failure] L2: C2's rule deletes the `upstream.toml` header; the planned cases miss it or contradict it
- Evidence: plan:108-111 says "the comment and blank lines at the end of a block
  that come right before the next table header belong to that next table". The
  preamble at upstream.toml:1-19 is made only of comment and blank lines, and it
  comes right before the first `[[patch]]` (:20). Under the rule it belongs to
  `prismpm-sdk-view-closure`.
- Reproduction (`repro.py`): dropping the first patch removes 1683 bytes, and
  the header is gone. Dropping every patch also removes the header.
- The case at plan:95-96 checks only "the second patch and everything after it",
  so it passes on this defect. The case at plan:98 ("leaves the header comment")
  fails against the rule as specified, so the plan contradicts itself.
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- Next step: state that the text before the first table is a preamble that no
  table owns. Make the drop-first case compare the whole output, header
  included. Add the edge case "first table directly after the header".

### R2 [test-failure] L2: the rule breaks D1 for section comments and comments at the end of the file
- Evidence: D1 (plan:144) says removal "never removes other tables or their
  comments".
- upstream.toml:84-89 is a section comment ("# Gaps: ..."). The policy there
  says a gap "becomes a `[[patch]]` row". If a patch row sits after that section
  comment, the rule hands the comment to that patch, and dropping the patch
  deletes it.
- `edge.py` case "section comment before a patch row" loses "# Gaps: section".
  Case "trailing comment EOF, drop last" deletes the trailing comment too.
- The planned cases (plan:92-99) include neither shape.
- Behavior? yes. Test meaning? yes. Interface? no. Level L2.
- Next step: define who owns a comment. Either a comment run that is separated
  from the next header by a blank line stays put, or only the single blank
  separator moves. Add both shapes as cases.

### R3 [specification-drift] L2: read literally, the rule drops nothing
- Evidence: plan:111 drops "the blocks whose first lines are `[[patch]]` and
  `id = "<id>"`". After the move at plan:109-110, every block from the second on
  starts with a blank or comment line, never with `[[patch]]`.
- `repro.py` ("literal" rows) removes 0 bytes for every id. Only matching the
  first non-comment lines works.
- Behavior? yes. Test meaning? no. Interface? no. Level L2.
- Next step: specify the match as "the block's table header line and the line
  that follows it".

### R4 [razor-overage] L1: the reason given for duplicating the rule is false
- Evidence: plan:106-107 says the scripts "cannot share a module" under `-I`.
  But `importlib.util.spec_from_file_location` works under `-I`.
  test_pr_stage.py:10 does this, and so does the plan's own test (plan:88-89).
- Both scripts are guarded by `if __name__ == "__main__"`
  (upstream.py `main()`, repin.py `sys.exit(main())`), so they can be imported
  without side effects.
- Either `repin.py` can load `upstream.without_patch`, or the rule can live once
  in a sibling file. Two copies of the rule can drift apart.
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### R5 [coverage-gap] L2: C1's gate accepts skipped, empty or unlisted suites
- Evidence: plan:59-66 checks only for a zero exit status from a hand-written
  `SUITES` table. On 3.11, a suite whose cases are all skipped, or that runs 0
  tests, exits 0. A new `scripts/**/test_*` file that nobody adds to the table
  is never run.
- So D1 (plan:135) is enforced only by prose (plan:73-74). AGENTS.md:54-58 says
  gates "must reject skipped, ignored, filtered, conditionally omitted ...
  results".
- Behavior? no. Test meaning? yes. Interface? no. Level L2.
- Next step: derive the inventory from the files on disk (a glob over `scripts/`,
  minus a recorded list of exclusions), fail on any file that is neither listed
  nor excluded, and refuse "Ran 0 tests", "skipped=" or a missing PASS count.

### R6 [coverage-gap] L2: D1 says "every script test suite", but Node suites stay CI-only
- Evidence: try-page.yml:79-82 runs `npm run test:try` (`try/test.mjs`) and
  `npm run test:identity` (`identity/test.mjs`). Neither runs under `vv`, and
  neither is in `SUITES` or the Q2 exclusions.
- The brief's GAP-TEST-06 (brief:175-178) is about CI-only tests in general.
- `test_change_impact.py` runs only transitively (test_pr_stage.py:66-71), and
  the plan does not mention it.
- Behavior? no. Test meaning? yes (D1 and the D3 status claim). Interface? no.
  Level L2.
- Next step: list every CI-only suite, in Python and Node, as either included or
  excluded with a reason.

### R7 [specification-drift] L2: the brief's executable-bit check is dropped without a new owner
- Evidence: brief:377-378 defines C1 as "`just scripts-test` in `vv` ... and
  check the executable bit of path-invoked scripts (Justfile only)".
- The plan replaces the Justfile recipe with a Rust test (plan:54-69) and never
  mentions the executable-bit check. C3 (plan:41) is described only as
  "OPS-01 phase A + OPS-02".
- Keeping the check out of C1 is correct: `scripts/ci/land-register.sh` is mode
  100644 (`git ls-files -s`), so the check would fail `vv` before C3 restores the
  bit. But the plan records neither the deviation nor where the check moves.
- Behavior? no. Test meaning? yes (a planned gate is lost). Interface? no.
  Level L2.
- Next step: assign the executable-bit check to C3, after the bit is restored,
  and record why C1 deviates from the brief.

### R8 [specification-drift] L1: C6 is missing the tag-ruleset prerequisite
- Evidence: brief:502-503 says "GOV-06 needs a tag ruleset (the qualifier runs
  from the tag itself)". The plan's C6 prerequisites (plan:41) are O3 and O4(b),
  with no O5.
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### R9 [orphan-file] L1: C2 depends on C1 but is listed with no prerequisite
- Evidence: plan:84 and plan:69 add `test_upstream_rows.py` to the `SUITES` table
  that C1 creates. The sprint table gives Sprint 1 the prerequisite "none"
  (plan:38), and the plan ships one PR per slice (plan:33).
- If C2 merges first, `test_upstream_rows.py` is not run by `vv`.
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### R10 [infrastructure-mismatch] L1: the brief path and its status words do not exist on the target branch
- Evidence: plan:18 cites `docs/audits/RESEARCH_BRIEF.md`, and the D3 steps
  (plan:137, 146, 154) edit it. `docs/audits/` does not exist on
  `remediation/sprint-1`. The file exists only on `audit/deep-audit-recon`.
- The status words "PARTIAL-RESOLVED" and "RESOLVED" are not used anywhere in
  the brief. Its statuses are CONFIRMED and PARTIAL (brief:21).
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### R11 [documentation-accuracy] L1: the plan records planted-defect evidence only in the PR
- Evidence: AGENTS.md:54-56 says to "Record the defect, failing command,
  diagnostic, and restored result in the project's verification documentation".
  A project-owned precedent exists: `PAGES-VERIFICATION.md`.
  (`VERIFICATION.md` is template-locked, per template.lock.)
- D4 of C1 (plan:139-140) and D4 of C2 (plan:147-148) record the evidence only
  "in the PR".
- Behavior? no. Test meaning? no. Interface? no. Level L1.

### R12 [documentation-accuracy] L1: the ci.rs module doc is not updated
- Evidence: crates/conformance/tests/ci.rs:1-3 describes the file as CI-01 only.
  The plan adds an unrelated test there (plan:54-55), and its doc changes cover
  only the new test (plan:73-74).
- Behavior? no. Test meaning? no. Interface? no. Level L1.

## Passes with no findings

- Prompt injection: neither the plan nor the brief contains text addressed to
  the auditor, and the plan has no non-ASCII or hidden characters.
- Security/OWASP: the suites run with fixed argument vectors and no shell
  interpolation of input. The O4 issues claim no owner approval (plan:128-129).
- Meta-gate and frontier rules: covered under Summary.

## Required before re-audit

Fix R1, R2, R3, R5, R6 and R7 in the plan. Address R4 and R8 through R12 or
record why not. Then submit a new plan revision; its new SHA-256 will be judged
fresh.
