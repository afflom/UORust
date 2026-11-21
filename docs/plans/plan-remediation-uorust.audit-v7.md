# Audit: plan-remediation-uorust (v7)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: c07aaef1138f80b9748764f9e20c32b30fb8deeab4b87e698edd6f5e212a7853
**Corrected**: W1; V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`.
The SHA-256 was computed with `sha256sum`. The plan and every file it cites were
treated as untrusted data. Nothing in the repository was edited except this
report. Reproductions ran with `python -I` (CPython 3.11.9, Windows host) in
the session scratchpad `audit-rem7/copy/`, a `git archive HEAD` copy with its
own `git init`. Docker is not installed here, so nothing ran in the SDK image,
and `lexlean` (needed by `check.sh`) was not available.

## Summary

There are 5 findings: 4 at L2 and 1 at L1. Any finding is a VETO. Because
some findings are L2, the planner cannot correct them inline. The plan needs a
new revision and a fresh audit.

v7 corrects W1 in substance:
- `scripts/release/` and `scripts/self-model/` join the `test` obligation's
  inputs, and its `untouched` text is updated.
- The generated text is produced by `graph.py write`, the certified bundle is
  re-exported, and the files are listed in Affected Files.
- Step 4 of `script_suites_pass` fails on a suite outside the obligation's
  inputs.

Every earlier finding stays corrected.

The new findings come from following the change through the factory's other
gates:
- `graph.py reads` already refuses the `test` obligation at 28926f95 (X1).
- The plan never runs `graph.py reads`, and its new test can add refusals (X2).
- Step 4 has no stated way to read TOML in `repo-conformance`, and no stated
  coverage rule (X3).
- `graph.py write` run on a Windows host rewrites both generated files to CRLF,
  and the in-`vv` check accepts them (X4).
- The export step is named by reference to `check.sh`, which does not export
  (X5).

### Must-check results

| Item | Result |
|---|---|
| `graph.toml` `test` inputs gain `scripts/release/`, `scripts/self-model/` | Reproduced in the copy. The inputs array is at graph.toml:149-164, and it has no `!` exclusion under `scripts/`. All 5 suites left after `ELSEWHERE` then fall inside the closure. Everything they read outside their own directory is declared: `.github/workflows/release.yml` and `install.yml` through the `workspace` set's `.github/`, and `test-land-register.sh` reads only `scripts/ci/`. On the host, every suite passes with the summary `verdict` expects: `Ran 3/11/5/4 tests` + `OK`, and `5 isolated shell scenarios passed`. |
| `python3 -I prism-factory/graph.py write` is the right generator | Yes. graph.py:503-508 writes `model/src/Factory/UORust.lex.tex` (the `module_text`, which embeds every obligation's inputs) and the README section between the `graph:begin`/`graph:end` markers, after `validate`. It writes nothing else. In the copy (LF-normalized diff) it changed exactly the `test` row of README.md:103 and one line of UORust.lex.tex. `graph.py check` then passed. On this Windows host it also rewrote every line to CRLF (X4). |
| How `prism-factory/certified/` is re-exported | The documented procedure is prism-factory/README.md:155-163 ("Changing the graph"): `graph.py write`; `cd prism-factory/model && lexlean lock && lexlean verify --all`; `lexlean production build --request ../production-request.json`; `rm -rf ../certified && cp -r .lexlean/production/<result> ../certified`. `check.sh` (lines 19-38) rebuilds into `model/.lexlean/production/$result` and diffs that against the committed bundle. It never writes `certified/`. The bundle does embed the inputs: `certified/evidence/attestation.json`, `compiler-input.json`, `program/R*.json`, `crate/R*.rust-std.rs` and the certificates all contain `"scripts/ci/"`. So a re-export is required, as the plan says. The plan points to the step only through `check.sh` (X5). `model/lexlean.lock` pins only the workspace files and the `std` package tree, not the model's own sources, so `lexlean lock` should leave it, and its `upstream.toml` "relocked" SHA-256, unchanged. This could not be run here. |
| Is `just factory-model` in `just vv`? | No. `vv` (Justfile:11) is `fmt-check model template-check self-model-check lint test deny app uorust`. No workflow calls any factory recipe (`grep factory .github/workflows` is empty). `just factory-model` has to be run separately, and the plan lists it separately (plan:270-271). Correct. Only `graph.py check` runs inside `vv`, through UE-20 `factory_ue_20` (crates/conformance/tests/factory.rs:59-145). A stale certified bundle is therefore caught only by a manual `just factory-model`. |
| `graph.py reads` closure | Adding inputs can only widen coverage, so the change itself adds no refusal. But `reads` already fails on 28926f95 (X1), and the new test's literals can add failures (X2). |
| UE-20 `crates/conformance/tests/factory.rs` | Unaffected. `factory_ue_20` passes after `graph.py write`. The planted edit in `an_undeclared_read_..._ue_20` anchors on `"brand/foundation/src/index.css",\n` (factory.rs:222-226). That line stays unique if the two new entries go next to `scripts/ci/`, as the copy did. |
| Self-model facets citing the factory | None cite the obligation inputs. Only `model/ids.toml:185` and `features/suites/factory.feature` carry the UE-20 statement, which does not mention inputs. `self-model-check` does not declare `prism-factory/`. `scripts/ci/change_impact.py:70-96` reads graph.toml from the trusted base for an advisory report. The new inputs only add `test` as a consumer of `scripts/release/` and `scripts/self-model/` paths. `test_change_impact.py` uses its own fixture graph. |
| Factory host tests | `prism-factory/host/tests/core.rs` recomputes identities from `graph.py runs`, which is dynamic. Nothing there pins the `test` inputs. |
| Step 4 of `script_suites_pass` | Sound in intent, but see X3. It is effective only on a checkout. In a closure, discovery sees only the declared suites, so the check holds trivially. That is acceptable, because a plain `just vv` and `bootstrap.yml` run on the checkout. |

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| W1 | corrected | Plan:65-72 (inputs, `untouched` text, `graph.py write`, re-export, Affected Files), 106-108 (step 4 fails closed on the checkout), 236-237 (D2), 270-271 (`just factory-model`). Reproduced: with the two inputs added, all 5 discovered suites are covered and `graph.py check` passes. The naming of the export step is imprecise (X5), but the obligation is now planned. |
| V1-V3 | corrected | C2 is unchanged from v6. The snapshot at 28926f95 still has 18 leading comment lines, 7 `[[patch]]` rows and 105 lines. `scripts/ci/fixtures/` does not exist yet, which is correct because it is new. `upstream.py:180` still reads `doc["patch"]`. |
| U1-U5, T1-T4, S1-S8, R1-R12 | corrected | Re-checked against plan:39-60, 74-114, 129-197 and 254-319. `land-register.sh` is still index mode 100644, and the executable-bit check stays in C3. |

## Findings

### X1 [test-coverage-gap / specification-drift] L2: the `test` obligation already reads an undeclared suite, so `graph.py reads` refuses on 28926f95 and step 4's claim is false
- `crates/conformance/tests/browser_acceptance.rs:52-58` (SM-07, in `cargo test
  --workspace`, so in `vv`) runs `node --test bootstrap/acceptance-result.test.mjs`.
  That file imports `../site/acceptance-result.mjs` (line 5).
- Neither `bootstrap/` nor `site/` is in the `test` inputs, and `untouched` says
  "the site".
- Reproduced at 28926f95 in the copy: `python -I prism-factory/graph.py reads`
  exits 1 with `test reads bootstrap/acceptance-result.test.mjs
  (crates/conformance/tests/browser_acceptance.rs:56) without declaring it`.
  `factory.sh` calls `reads` before `routes`, `run` and `start` (factory.sh:44,
  81, 87, 99), so `just factory-plan` and `just factory` refuse on `main`
  today.
- In a `test` closure, SM-07's node run lacks both files and fails. The `test`
  obligation can therefore never pass in its closure.
- The brief does not record this; GAP-TEST-06 is about CI-only scripts.
- C1 edits exactly this inputs array, and step 4 states its purpose "so the
  factory's receipt covers every suite `vv` runs" (plan:108). SM-07 is a suite
  `vv` runs that the receipt does not cover, and step 4 checks only
  `scripts/**` suites.
- Reproduced: declaring `"bootstrap/"` and `"site/acceptance-result.mjs"` as
  well makes `graph.py reads` pass (15 obligations).
- Behavior? yes: the factory entry refuses to plan, and `test` fails in its
  closure. Test meaning? yes: the stated coverage claim does not hold.
  Interface? no. Level L2.
- **Next step:** in C1, either declare SM-07's reads in the `test` obligation
  (the copy shows the two entries that clear `reads`; check which other
  `bootstrap/` or `site/` files the node test loads at run time), or narrow
  step 4's claim to script suites and record the SM-07 closure gap as a new gap
  in the brief.

### X2 [infrastructure-mismatch / test-coverage-gap] L2: the plan never runs `graph.py reads`, and the new test's discovery literal can trip it
- `graph.py reads` is the factory's early refusal of a test target that names
  an undeclared tracked path (graph.py:194-212). It runs only from `factory.sh`.
  It is not part of `check`, `vv` or the plan's CI Commands (plan:266-272).
- `script_suites_pass` is a new test target in `crates/conformance/tests/` that
  must name `scripts` paths to discover suites (plan:89-90).
- Reproduced in the copy, with the plan's inputs plus X1's two entries:
  - A planted `.args(["ls-files", "-z", "--", "scripts/"])` in ci.rs is
    refused: `test reads scripts/ (crates/conformance/tests/ci.rs:43) without
    declaring it`. `declared` requires every tracked entry under `scripts/` to
    be covered, and `scripts/content/`, `scripts/mir/` and others are not.
  - Glob pathspecs (`"scripts/**/test_*.py"`) and
    `root().join("prism-factory/graph.toml")` are not refused.
- The plan constrains neither the literals nor the check, so a natural
  implementation passes `just vv` and breaks `just factory`.
- Behavior? yes: the factory entry refuses to plan. Test meaning? no.
  Interface? no. Level L2.
- **Next step:** add `python3 -I prism-factory/graph.py reads` to C1's CI
  Commands and DoD (after X1 makes it pass on the base). State that discovery
  names only declared paths or glob pathspecs, for example `git ls-files -z --
  'scripts/**/test_*.py' 'scripts/**/test-*.sh'`.

### X3 [razor-overage / dependency / specification-drift] L2: step 4 reads graph.toml with no stated parser and no stated coverage rule
- Step 4 asserts that each suite "lies under an input of the factory's `test`
  obligation, read from `prism-factory/graph.toml`" (plan:106-108).
- `repo-conformance` has no TOML parser. Its dependencies are `repo-model` and,
  for dev, `uorust` (crates/conformance/Cargo.toml:10-19; Cargo.lock:237-242).
  `repo-model` does not re-export `toml` (crates/model/src/lib.rs:17).
- Parsing therefore needs one of these, and the plan names none of them:
  - a new dev-dependency, which changes `crates/conformance/Cargo.toml` and
    the `repo-conformance` entry in `Cargo.lock` (neither is in Affected Files);
  - ad hoc text matching of graph.toml;
  - running `graph.py runs`, which prints sets already expanded.
- "Lies under an input" is also undefined against the engine's rule:
  - The engine's rule is `covers`: the longest inclusion against the longest
    `!` exclusion (graph.py:220-230; identity.py:15-17). Expanding `sets` is
    part of the input list (graph.py:142).
  - A raw prefix match on the `inputs` array ignores both rules.
  - It agrees today only because no exclusion touches `scripts/`.
- Behavior? no. Test meaning? yes: what step 4 proves depends on the unstated
  rule. Interface? yes: the crate's dependency set may change. Level L2.
- **Next step:** name the mechanism, preferably `python3 -I
  prism-factory/graph.py runs` (the expanded inputs, from the factory's own
  reader), with coverage decided by the `covers` rule. If a dependency is
  added instead, list `crates/conformance/Cargo.toml` and `Cargo.lock` in
  Affected Files. Add a planted negative control: a suite in a directory that
  is not declared, and one that is excluded with `!`.

### X4 [infrastructure-mismatch] L2: `graph.py write` on a Windows host writes CRLF, and `graph.py check` accepts it
- graph.py:506-507 calls `write_text(..., encoding="utf-8")` with no
  `newline`.
- Reproduced on this host (CPython 3.11.9, Windows): `graph.py write` rewrote
  every line of `prism-factory/README.md` (diff stat 408) and `UORust.lex.tex`
  (diff stat 188) with CRLF terminators (`file`: "with CRLF line terminators"). The real
  change is 2 lines.
- `graph.py check` then passed, because `read_text` translates newlines on
  read (graph.py:512-515). UE-20 inside `vv` passes too.
- `.gitattributes` is `* -text`, so the CRLF bytes would be committed as-is.
  They would change the closure, and thus the identity, of every obligation
  that declares `prism-factory/`.
- Only the certified re-export and `just factory-model` might notice, and
  neither is in `vv`.
- The plan names the generator (plan:69) but not where it runs. It says "SDK
  image" only for `just factory-model` and `vv`. This repository's working copy
  is on a Windows host.
- Behavior? yes: the committed generated files differ from what the generator
  writes on Linux. Test meaning? no. Interface? no. Level L2.
- **Next step:** state that `graph.py write`, like the re-export, runs in the
  pinned SDK image (Linux), and add an LF check of the two generated files to
  C1's verification. Fixing graph.py's `newline` is a separate generator change
  and outside C1.

### X5 [documentation-accuracy] L1: the export step is named through `check.sh`, which does not export
- Plan:70-72 says the bundle "is re-exported by the export step that
  `prism-factory/check.sh` replays".
- `check.sh` rebuilds into `prism-factory/model/.lexlean/production/$result`
  and compares (check.sh:21-38). Nothing in it writes `prism-factory/certified/`.
- The repository's export procedure is prism-factory/README.md:155-163:
  `lexlean lock && lexlean verify --all` in `prism-factory/model`, then
  `lexlean production build --request ../production-request.json`, then
  `rm -rf ../certified && cp -r .lexlean/production/<result> ../certified`.
  It runs where the SDK's `lexlean` is.
- Behavior? no. Test meaning? no. Interface? no. Level L1.
- **Next step:** cite README.md "Changing the graph" by name and list its
  commands and environment in C1's Affected Files or Changes.

## Passes with no other findings

- **Prompt injection.** The plan has no text addressed to the auditor and
  claims no approval. Its only non-ASCII characters are the six "…" in the
  revision notes. The cited files contain no instructions.
- **Infrastructure mismatch.** X2 and X4. The suites need `python3`, `bash`
  and git, as v6 established. `just factory-model` is correctly kept separate
  from `vv`.
- **Security, OWASP and injection.** No change since v6. The suites run with
  fixed argument vectors, discovery is limited to tracked files, and the O4
  issues record NOT_ESTABLISHED and claim no approval. Nothing at L3.
- **Test failure and coverage.** X1, X2 and X3. C2 cases 1-11 are unchanged
  and still match the snapshot.
- **Razor, dependencies and architecture.** X3. Otherwise there is one
  stdlib-only module, one Rust test with two pure helpers, and one graph edit
  through the generator.
- **Orphan files.** None. The fixture is read by cases 1, 2 and 9, and
  `test_upstream_rows.py` is discovered by C1.
- **Specification drift against the brief.** X1, which is an unrecorded gap.
  The sprint order still keeps every dependency in the brief.
- **Documentation accuracy and internal consistency.** X5. The sprint table,
  DoD, CI commands and revision notes agree with each other.

## Required before re-audit

The planner fixes X1-X4 (L2) and X5 (L1) and submits a new revision. Its
SHA-256 will be judged fresh.
