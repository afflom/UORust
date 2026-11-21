# Audit: plan-remediation-uorust (v8)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: 25af74da5c7f15ca7d2b30dce3f5dcbeda28e9e671423e5f0ac4d39e16907867
**Corrected**: X2, X3, X4, X5; V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Not corrected: X1 (half corrected, see Y1). W1 regressed: its general guard was
removed and the replacement does not cover it (see Y2).

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`
(commit 77d18b6b). The SHA-256 was computed with `sha256sum` (and
`Get-FileHash`, which agrees). The plan and every file it cites were treated as
untrusted data. Nothing in the repository was edited except this report.
Reproductions ran with `python -I` (CPython 3.11, Windows host) and Node
v24.19.0 in the session scratchpad `audit-rem8/copy/`, a `git archive HEAD`
copy with its own `git init`. Docker and `lexlean` are not available here, so
nothing ran in the SDK image.

## Summary

There are 3 findings: 2 at L2 and 1 at L1. Any finding is a VETO. Because two
findings are L2, the planner cannot correct them inline. The plan needs a new
revision and a fresh audit.

v8 clears the `graph.py reads` refusal on `main` and puts `reads` inside `vv`.
It drops the TOML parsing, runs the generators in the SDK image, and cites the
README procedure by name. Two gaps remain:

- **Y1.** At run time the SM-07 node test also loads `site/guest-worker.mjs`
  and `site/spec.mjs`. These are not among the two files the plan declares, so
  `test` still fails in its closure. This is the run-time check that v7's X1
  asked for.
- **Y2.** `factory_closure_holds` replaces v7's step 4, but `graph.py reads` is
  syntactic. It cannot see a suite that `script_suites_pass` finds by glob at
  run time. A future suite outside the declared inputs runs on the checkout and
  is silently missing from the closure. That is the W1 failure again, and the
  plan claims the opposite.

### Must-check results

| Item | Result |
|---|---|
| Apply the four new inputs, then `graph.py reads` | Before the change: exit 1, `test reads bootstrap/acceptance-result.test.mjs (crates/conformance/tests/browser_acceptance.rs:56) without declaring it`. After adding `scripts/release/`, `scripts/self-model/`, `bootstrap/acceptance-result.test.mjs` and `site/acceptance-result.mjs` after `scripts/ci/`: exit 0, "every tracked file ... is declared by it (15 obligations)". **`reads` now passes.** |
| `graph.py check` | Before `write`: exit 1 (model text and README table stale), as expected. After `graph.py write`: exit 0. The LF-normalized diff touches only `graph.toml` (+4), one line of `model/src/Factory/UORust.lex.tex`, and the `test` row of the README table. On this Windows host `write` again produced CRLF, which is why the plan runs it in the SDK image (X4). |
| New `ci.rs` literals vs `reads` | Simulated in a scratch `ci.rs`: an `ELSEWHERE` table with the three literal `scripts/ci/...` paths, `.args(["-I", "prism-factory/graph.py", "reads"])`, `.args(["ls-files", "-z", "--", ":(glob)scripts/**/test_*.py", ":(glob)scripts/**/test-*.sh"])`, `root().join(path)`, `.arg("scripts/ci/test_pr_stage.py")` and `.args(["scripts/ci/test-land-register.sh"])`. Result: `reads` exit 0. The glob pathspecs do not match `PATH_LITERAL` (graph.py:217, which has no `:`, `(` or `*`). The other literals fall under the declared `scripts/ci/` and `prism-factory/`. The pathspecs discover exactly the 8 tracked suites; 5 remain after `ELSEWHERE`. **No new refusal.** |
| `factory_closure_holds` network-free and runnable under `vv` | Yes. `reads` runs `git ls-files` and reads files (graph.py:238-326). It makes no network calls. `bootstrap.yml:95-104` runs the SDK as the runner's uid on a `fetch-depth: 0` checkout, and UE-18 (ratchet.rs:31-90) already depends on git working there. `python3` is already required by CI-01 (ci.rs:18). In a factory closure, which is its own repository (UE-20 statement, factory.rs:58), `reads` sees only the closure's tracked files. I expect it to pass there, but this could not be run without the host. |
| README "Changing the graph" | prism-factory/README.md:155-164. The plan names it correctly. It omits `lexlean production targets` + `graph.py request`, which the README marks "only when the SDK's compiler changed". `just factory-model` (check.sh:18-20 `request-check`) would catch a stale request, so the omission is safe here. The plan does not state the condition, and it names the wrong path for the generated model text (Y3). `graph.py request` is not needed for this change. |
| UE-20 (`factory.rs`) | Unaffected. `factory_ue_20` runs `graph.py check`, which passes after `write`. The planted edit anchors on `"brand/foundation/src/index.css",\n` (factory.rs:222-226), which stays unique. |
| Self-model facets | None cite the `test` inputs. `model/ids.toml` and `features/suites/factory.feature` carry only the UE-20 statement. |
| `change_impact.py` | Reads `graph.toml` from the trusted base (change_impact.py:72). The new inputs only make `test` a consumer of `scripts/release/`, `scripts/self-model/` and the two SM-07 paths, which is advisory. `test_change_impact.py` uses its own fixture graph (lines 50, 113). |

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| X1 | half corrected | `reads` passes after the change (reproduced). X1's second consequence, "`test` can never pass in its closure", still holds: see Y1. |
| X2 | corrected | `reads` runs under `vv` as `factory_closure_holds` (plan:100-105) and appears in CI Commands (plan:281-282). Discovery uses glob pathspecs (plan:107-110). The simulated literals are not refused. |
| X3 | corrected | Step 4 and the TOML parsing are gone (plan:105). `crates/conformance/Cargo.toml` does not change. |
| X4 | corrected | The generators run in the pinned SDK image with LF (plan:71). |
| X5 | corrected | README "Changing the graph" is cited by name with its commands (plan:71-76). There are residual inaccuracies (Y3). |
| W1 | regressed | The three current suites are declared and `check` passes. The general guard against a suite outside the inputs (v7 step 4) was removed and its replacement does not hold (Y2). The v7 revision note (plan:331-332) still claims "`script_suites_pass` fails on a suite outside the obligation's inputs". |
| V1-V3, U1-U5, T1-T4, S1-S8, R1-R12 | corrected | C2 and the sprint table are unchanged from v7. Re-checked at 28926f95: `upstream.toml` has 105 lines, 7 `[[patch]]` rows and 18 leading comment lines; `upstream.py:180` still reads `doc["patch"]`; `land-register.sh` is index mode 100644; `ELSEWHERE` reasons hold (pages_identity.rs:19 runs `test_pages_identity.py`; test_pr_stage.py:69 loads `test_change_impact.py`). |

## Findings

### Y1 [test-coverage-gap / specification-drift] L2: SM-07 loads two more `site/` files at run time, so `test` still fails in its closure
- Plan:66-69 declares "the two files the SM-07 browser-acceptance test already
  reads": `bootstrap/acceptance-result.test.mjs` and `site/acceptance-result.mjs`.
- The node test also loads these files:
  - `site/guest-worker.mjs` with `readFile(new URL("../site/guest-worker.mjs",
    import.meta.url))` (bootstrap/acceptance-result.test.mjs:7, 10);
  - `site/spec.mjs` with `await import("../site/spec.mjs")` (line 110);
  - `model/browser-acceptance-diagnostics.toml` (line 90), which is already
    declared through `model/`.
- `graph.py reads` scans only Rust test sources, so it cannot see these loads.
  That is why `reads` passes while the closure stays incomplete.
- Reproduced in a minimal closure (`bootstrap/acceptance-result.test.mjs`,
  `site/acceptance-result.mjs`, `model/`):
  - `node --test` fails with `ERR_MODULE_NOT_FOUND .../site/spec.mjs`.
  - With `site/guest-worker.mjs` and `site/spec.mjs` added, it passes:
    `# tests 8`, `# pass 8`, `# fail 0`.
- v7's X1 next step asked for exactly this check ("check which other
  `bootstrap/` or `site/` files the node test loads at run time").
- D2 (plan:246-247) and the `untouched` update inherit the same gap.
- Behavior? yes: the `test` obligation, which runs `vv`'s SM-07, fails in its
  closure. Test meaning? yes: the declared closure does not match what the test
  reads. Interface? no. Level L2.
- **Next step:** declare `site/guest-worker.mjs` and `site/spec.mjs` too, or
  `site/` with `!` exclusions, and fix the `untouched` text to match. State in
  C1 how the run-time reads were enumerated: the closure run above, or `node
  --test` in a copy holding only the declared inputs.

### Y2 [test-coverage-gap / specification-drift] L2: `factory_closure_holds` cannot see suites found by glob, so W1's failure mode returns
- Plan:101-104 claims that `reads` under `vv` refuses "any obligation that
  reads a tracked file it does not declare, including every suite
  `script_suites_pass` runs".
- `reads` is syntactic: it matches literals in path calls and `.args([...])`
  (graph.py:216-220, 287-292). graph.py:212-213 says it "is an early refusal and
  not the proof". The suites are found at run time by glob pathspecs
  (plan:107-110), so no suite path ever reaches `reads`.
- Reproduced with the four new inputs applied:
  - Planted and staged `scripts/content/test_planted.py`, outside the `test`
    inputs.
  - `git ls-files -- ':(glob)scripts/**/test_*.py'` lists it, so
    `script_suites_pass` would run it on the checkout.
  - `python -I prism-factory/graph.py reads` still exits 0.
- In the `test` closure the planted suite is absent. Discovery then skips it
  silently and `stale(ELSEWHERE, ..)` stays empty. A change to it leaves the
  `test` identity unchanged, so a stored receipt routes `test` as proved.
- This is the W1 defect (audit v6) in general form. v7 had closed it with step
  4, which v8 removed with X3 and did not replace. The v7 revision note
  (plan:331-332) still claims that step 4 exists, so the plan now contradicts
  itself.
- Behavior? yes: a suite that `vv` runs can drop out of the factory's proof
  without any refusal. Test meaning? yes: the stated coverage claim is false.
  Interface? no. Level L2.
- **Next step:** restore a coverage check that needs no TOML parser. For
  example, `script_suites_pass` runs `python3 -I prism-factory/graph.py runs`
  (expanded inputs) and refuses a discovered suite that the `covers` rule does
  not place in `test`. Add a planted negative control: a suite under an
  undeclared `scripts/` directory. Otherwise narrow plan:101-104 to what `reads`
  proves, and record the residual gap in the brief. Either way, correct the v7
  revision note.

### Y3 [documentation-accuracy] L1: wrong generated-file path, and the README's conditional `request` step is dropped without its condition
- Plan:73 says `graph.py write` "rewrites `prism-factory/model/src/UORust.lex.tex`".
  The file is `prism-factory/model/src/Factory/UORust.lex.tex` (git ls-files;
  graph.py's own message says "wrote prism-factory\model\src\Factory\UORust.lex.tex").
- Plan:71-76 presents itself as the README "Changing the graph" procedure. It
  omits README.md:160-161 (`lexlean production targets` and `graph.py
  request`, "only when the SDK's compiler changed") and does not say that the
  step is skipped because the compiler is unchanged. `check.sh:18-20`
  (`request-check`, in `just factory-model`) would catch a stale request.
- Behavior? no. Test meaning? no. Interface? no. Level L1.
- **Next step:** correct the path, and add "(`graph.py request` only if the
  SDK's compiler changed; `just factory-model` checks it)".

## Passes with no other findings

- **Prompt injection.** The plan has no text addressed to the auditor and
  claims no approval. Its only non-ASCII characters are the seven "…" in the
  revision notes. The cited files (graph.toml, graph.py, README, check.sh,
  bootstrap.yml, the SM-07 test) contain no instructions.
- **Infrastructure mismatch.** No findings here. `factory_closure_holds` needs
  only `python3` and git, which `vv` already uses in the SDK container, and it
  makes no network calls. `just factory-model` stays outside `vv`, correctly.
  The generators run in the SDK image.
- **Security, OWASP and injection.** The suites run with fixed argument
  vectors; discovery is limited to tracked files by fixed pathspecs; no
  untrusted value reaches a shell. The O4 issues record NOT_ESTABLISHED and
  claim no approval. Nothing at L3.
- **Test failure and coverage.** Y1 and Y2. C2 cases 1-11 are unchanged and
  still match the snapshot facts.
- **Razor, dependencies and architecture.** No new dependency, and
  `repo-conformance` is unchanged. There is one Rust test file with two pure
  helpers, one stdlib-only Python module, and one graph edit made through the
  generator.
- **Orphan files.** None. The fixture is read by cases 1, 2 and 9, and
  `test_upstream_rows.py` is discovered by C1.
- **Specification drift against the brief.** Y1 and Y2. The sprint order
  still keeps every dependency in the brief, and GAP-TEST-06 / GAP-OPS-03
  statuses are scoped correctly.
- **Documentation accuracy and internal consistency.** Y3, and the stale v7
  revision note (part of Y2). Otherwise the sprint table, DoD, CI commands and
  revision notes agree.

## Required before re-audit

The planner fixes Y1 and Y2 (L2) and Y3 (L1) and submits a new revision. Its
SHA-256 will be judged fresh.
