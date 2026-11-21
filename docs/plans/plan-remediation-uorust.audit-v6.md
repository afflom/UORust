# Audit: plan-remediation-uorust (v6)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: 33cf8ca904243d8df567aa1063f8dccc7b38c952bd9fdc368f36522fef55fc7b
**Corrected**: V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`.
The SHA-256 was computed with `sha256sum`. The plan and every file it cites were
treated as untrusted data. Nothing was edited except this report. Reproductions
ran with `python -I` (CPython 3.11) in the session scratchpad `audit-rem6/`
(`rule6.py`, an independent implementation of the plan's `without_patch`;
`cases6.py`, `more6.py`; `snapshot.toml` = `git show 28926f95:upstream.toml`).
Docker is not installed here, so nothing ran in the SDK image.

## Summary

There is 1 finding, at L2. Any finding is a VETO. Because W1 is L2, the planner
does not correct it inline. The plan needs a new revision and a fresh audit.

v6 closes V1-V3, and every earlier finding stays closed. The snapshot approach
works. The fixture is reachable by every gate that needs it, no gate refuses
it, and C1's discovery does not treat it as a suite. Cases 1, 2 and 9 no longer
read the live `upstream.toml`, so automated re-pins cannot break them.

The new finding is about C1 and the repository's factory, not the snapshot.
C1's suite runner discovers suites from the tracked tree. The factory's `test`
obligation runs `cargo test --workspace` in a closure that holds only its
declared inputs, and those inputs leave out three of the five suites C1 adds
(W1).

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| V1 | corrected | Plan:120-121 and 133-158. Cases 1, 2 and 9 run on `scripts/ci/fixtures/upstream.toml`, a copy of `upstream.toml` at 28926f95. Details in "Snapshot checks" below. |
| V2 | corrected | Plan:222-226 lists the still-open part as the browser suites plus "the untested push, merge, generate-loop and land-certified scripts, release writers, MIR scripts, installers and hooks". That matches brief:175-178 (push.sh and push-branch.sh fall under "push", merge-main.sh under "merge"). |
| V3 | corrected | Plan:150-154 says the shapes raise "from the header search, the pre-check, or the post-condition". Reproduced: id not on the next line, `id="a"` and a trailing header comment raise from the header search. A duplicated id raises from the pre-check. The multi-line `[[` string raises `TOMLDecodeError` (a `ValueError`) from the post-condition parse. |
| U1-U5 | corrected | `doc.get("patch", [])` and case 10 (plan:122-124, 161-163). Indented `[`/`#` end the block (plan:179-183). `Ran N test` singular (plan:77-78, 83). 18-line preamble: lines 1-18 of the snapshot are comments, line 19 is blank, line 20 is the first header. *(wf)* on C3, C4, C6 and C9 only (plan:43). |
| T1-T4 | corrected | Patch key absent (plan:191-193) and case 9. `Kind` with cross-kind inputs (plan:74-85). Affected Files (plan:64-68, 118-127). D3 names the browser suites. |
| S1-S8, R1-R12 | corrected | Re-checked at plan:39-46, 52-60, 73-104, 129-197 and 254-298. `land-register.sh` is still mode 100644, and the executable-bit check stays in C3. O5 comes before C6, C1 before C2, and C2 before C3. |

### Snapshot checks (the must-check items)

- **The xtask `audit_deferral` gate.** `audit_deferral` (xtask/src/audit.rs:189-255)
  reads `crates/`, `xtask/` (`.rs`, `.md` and `.toml` files, through
  `gather_all`, audit.rs:275-293) and the `*.md` files at the root. It never
  reads `scripts/`, so the fixture is out of its scope. The root
  `TEMPLATE-VERIFICATION.md`, which C1 and C2 edit, is in scope. The text the
  implementer writes there must not contain the markers, but the plan proposes
  no such text.
- **Other gates over `scripts/` or fixtures.** None of them refuses or misreads
  the fixture:
  - `template.lock` and `template-contract.json` name no `scripts/` path.
  - The `self-model-check` inputs are `scripts/self-model/`,
    `scripts/experience/` and `scripts/mir/ledger.py`.
  - `uorust project` reads `crates/` and `xtask/`.
  - Every `upstream.toml` reader is path-specific: upstream.py:106, 178 and
    repin.py:65-69 read the root file, and prism-factory/upstream.sh:7-8 and
    graph.py:40 read `prism-factory/upstream.toml`.
  - `.gitattributes` has `* -text`, so the fixture keeps LF on Windows hosts too.
  - The factory `test` obligation declares `scripts/ci/` (graph.toml:159), so
    the fixture, `upstream.py`, `repin.py` and the new `upstream_rows.py` are
    all inside its closure.
- **C1 discovery.** The globs `scripts/**/test_*.py` and `scripts/**/test-*.sh`
  do not match `scripts/ci/fixtures/upstream.toml`. The existing CI-01
  `unittest discover -s scripts/ci -p test_pr_stage.py` (ci.rs:17-39) matches
  only its own pattern.
- **Cases 1, 2 and 9 when automation rewrites the live file.**
  - `frame_pin` (upstream.py:106-110) and `repin.py:69` write only
    `ROOT/upstream.toml`.
  - The cases read only the committed fixture, so their exact expectations
    are fixed.
  - Case 10 must redirect `ROOT`, because `main` reads `ROOT / "upstream.toml"`.
    "On a file with no `[[patch]]` rows" implies that.
  - Case 11 does not say which text `frame_pin` reads. On the live file the
    equality still holds after a re-pin (`without_patch` returns the input
    unchanged), so it cannot break the gate. Not a finding.

### C2 rule re-run on the snapshot (cases 1-11)

| Case | Result |
|---|---|
| 1 | `template-frame-live-controls` (the 7th and last patch) removes lines 75-83, which are the 8-line row plus the blank after it (391 bytes). The output equals lines 1-74 followed by lines 84-105, so the preamble, the other patches, `# Gaps:` and both `[[gap]]` tables are byte-identical. |
| 2 | First patch `prismpm-sdk-view-closure`: the output equals lines 1-19 followed by lines 30-105. |
| 3 | An unknown id returns the same object. |
| 4 | A section comment before the removed patch is kept. |
| 5 | Removing the last table leaves `"# h\n\n" + P(a)`, which ends with one newline. |
| 6 | Raises `ValueError` naming `a` (comment lookahead). |
| 7 | A `[[gap]]` with the patch's id is kept. |
| 8 | An indented comment and an indented `[[gap]]` are kept byte-identical. Every unhandled shape raises, at the step V3 now names. |
| 9 | The one-patch fixture gives `"# h\n# two\n"`, which has no `patch` key. Folding all 7 snapshot ids gives lines 1-19 followed by lines 84-105, with no `patch` key. |
| 10, 11 | Unchanged from v5 (no rule change). `doc.get` removes the `KeyError`. Folding equals sequential removal. |

The old split on the snapshot removes 3117 bytes in case 1 and drops both
`[[gap]]` tables. For each of the other six ids, its output is byte-identical
to the new rule's (plan:166-169).

## Findings

### W1 [infrastructure-mismatch / test-coverage-gap] L2: C1's suites lie outside the factory `test` obligation's declared inputs, so in its closure `script_suites_pass` runs only some of them and still passes
- **What C1 discovers.** `script_suites_pass` discovers "every tracked
  `scripts/**/test_*.py` and `scripts/**/test-*.sh`" (plan:89-90). Of the 5
  suites left after the exclusion table, 3 lie outside `scripts/ci/`:
  - `scripts/release/test_platforms.py`, which loads `platforms.py`
  - `scripts/release/test_qualify_tag.py`, which loads `qualify_tag.py`
  - `scripts/self-model/test_bounded_sources.py`, which reads `facets.py`
- **What the obligation declares.** The repository's factory gives `cargo test
  --workspace` the obligation `test` (prism-factory/graph.toml:140-165). Its
  inputs list only `scripts/ci/` and `scripts/hooks/passes.py` from
  `scripts/`. Its `untouched` text states that "scripts other than scripts/ci/
  and scripts/hooks/passes.py" leave its identity unchanged. Each obligation
  runs in a closure that is its own repository and holds only the declared
  entries (prism-factory/README.md; check.sh:13).
- **What happens in the closure.**
  - `git ls-files` lists none of the three suites.
  - The `ELSEWHERE` entries all live under `scripts/ci/`, so
    `stale(ELSEWHERE, discovered)` stays empty.
  - The test therefore passes on three suites instead of six.
  - This is exactly the case graph.toml:198-213 and factory.sh:40-43 warn
    about: a source that tolerates a missing file "pass[es] on other inputs
    than the identity covers".
  - `graph.py reads` is syntactic: it matches string literals passed to path
    calls (graph.py:214-218). Paths discovered at run time never reach it, so
    nothing refuses the graph before planning.
- **The consequences.**
  - A change to `scripts/release/` or `scripts/self-model/` leaves the `test`
    identity unchanged, so a stored receipt routes `test` (and `vv`) as
    proved.
  - The plan's own planted defect, an inverted assertion in
    `scripts/release/test_platforms.py` (plan:112-114, D4), would not fail the
    factory's `test` obligation.
  - AGENTS.md:54-57 requires gates to reject "conditionally omitted" results.
  - D1 ("every Python and shell script test suite runs") holds under a plain
    `just vv` but not under the factory's `vv` obligation.
- **What the plan omits.** It names no change to:
  - `prism-factory/graph.toml`.
  - The outputs `graph.py write` generates from it: the model text
    `prism-factory/model/src/Factory/UORust.lex.tex` and the README table at
    `prism-factory/README.md:103`. `graph.py check` refuses either when stale.
  - The certified bundle. It embeds the input list:
    `prism-factory/certified/evidence/attestation.json` carries `"scripts/ci/"`,
    and check.sh:24-38 requires a fresh export to equal the committed bundle.

  The plan's exclusions (plan:13-14) allow `prism-factory/**` to change only
  through its generators, so this work has to be planned explicitly.
- Behavior? yes: the factory proves `test` on inputs its identity does not
  cover. Test meaning? yes: the suite runner silently runs a subset in the
  closure. Interface? no. Level L2.
- **Next step:** give C1 the factory change. Add `scripts/release/` and
  `scripts/self-model/` (or every directory discovery can reach) to the `test`
  obligation's inputs and correct its `untouched` text. Regenerate the model
  text and README table with `graph.py write`, re-export
  `prism-factory/certified` through the factory's own path, and list all of
  these in C1's Affected Files.

  Also make `script_suites_pass` fail closed in a closure. One way: check that
  every directory holding a discovered suite on the checkout is declared, for
  example as a planted check against `graph.py`'s inputs. Or state why the
  declared inputs alone suffice.

## Passes with no other findings

- **Prompt injection.** The plan has no text addressed to the auditor and claims
  no approval. Its only non-ASCII characters are the five "…" in the revision
  notes. The cited files contain no instructions.
- **Infrastructure mismatch.** Apart from W1, the suites need `python3` (3.11 or
  later), `bash`, coreutils and git, all already used under `vv`. `-I` is
  compatible because every suite and both scripts load their modules with
  `spec_from_file_location`. The browser suites are excluded with a reason.
- **Security, OWASP and injection.**
  - Suites run with fixed argument vectors and no shell.
  - Discovery is limited to tracked files under fixed globs.
  - A `ValueError` in `frame_pin` stops the job before `push.sh`.
  - The O4 issues record NOT_ESTABLISHED and claim no approval.
- **Test failure and coverage.** Covered by W1. Cases 1-11 reproduce as the plan
  states.
- **Razor, dependencies and architecture.** One stdlib-only module with two
  callers, one Rust test with two pure helpers, and no new dependency.
- **Orphan files.** None. `test_upstream_rows.py` is discovered by C1, and the
  fixture is read by cases 1, 2 and 9.
- **Specification drift against the brief.**
  - The sprint order keeps every dependency in the brief (brief:370-402): C2
    before C3, O5 after O4(b), O6 after C3, C4, C5 and O2, C18 with or before
    C19, and C20 after C19, O3 and O4(d).
  - The C1 deviation is recorded as the operator's choice.
- **Documentation accuracy and internal consistency.** The sprint table,
  prerequisites, Definition of Done, CI commands and revision notes agree with
  each other. The byte and line counts match the snapshot.

## Required before re-audit

The planner fixes W1 (L2) and submits a new revision. Its SHA-256 will be
judged fresh.
