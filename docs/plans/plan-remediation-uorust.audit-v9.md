# Audit: plan-remediation-uorust (v9)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: faa5cf82c0f578a14dbe7452c11529fc6fa5699ca597dd445560322908680347
**Corrected**: Y1, Y2, Y3; X1, X2, X3, X4, X5; W1; V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`.
I computed the SHA-256 with `sha256sum`, and `Get-FileHash` gives the same
value. I treated the plan and every file it cites as untrusted data. The only
repository file I changed is this report. Reproductions ran with `python -I`
(CPython 3.11, Windows host) and Node in the session scratchpad `audit-rem9/`:
`copy/` is a `git archive HEAD` copy with its own `git init`, and `closure/` is
the `test` obligation's closure. Docker and `lexlean` are not available here,
so nothing ran in the SDK image.

## Summary

There are 2 findings: 1 at L2 and 1 at L1. Any finding is a VETO. Because Z1 is
L2, the planner cannot correct it inline. The plan needs a new revision and a
fresh audit.

v9 corrects all three v8 findings. With the six new inputs, `graph.py reads`
and `graph.py check` pass. The SM-07 node test passes in the `test` closure (8
of 8). The planned step 4 refuses a planted suite outside every input even
though `reads` still passes.

What remains:

- **Z1.** Step 4's `covered` rule is not the rule the factory uses. It reads
  only the obligation's own `inputs` key and ignores `!` exclusions. With one
  exclusion, a suite can be absent from the closure while step 4 calls it
  covered and `reads` and `check` both pass. That is the Y2 failure again,
  limited to this input form.
- **Z2.** The DoD and the verification record do not include the planted-defect
  evidence for the two new gates.

### Must-check results

| Item | Result |
|---|---|
| Apply the six new inputs, then `graph.py reads` | Before the change: exit 1, `test reads bootstrap/acceptance-result.test.mjs (crates/conformance/tests/browser_acceptance.rs:56) without declaring it`. After inserting `scripts/release/`, `scripts/self-model/`, `bootstrap/acceptance-result.test.mjs`, `site/acceptance-result.mjs`, `site/guest-worker.mjs` and `site/spec.mjs` after `scripts/ci/`: exit 0, "every tracked file a source of an obligation names is declared by it (15 obligations)". |
| `graph.py check` | Before `write`: exit 1 (model text and README table stale), as expected. After `graph.py write`: exit 0 ("18 installed files are as upstream.toml records at 4cc8f8bd4dd7"). With CR differences ignored, the diff is `graph.toml` +6, one line of `prism-factory/model/src/Factory/UORust.lex.tex` and the `test` row of the README table. |
| SM-07 in a minimal closure | I built the closure with graph.py's own `load()` and `covers()` over `git ls-files`: 5401 of 5676 tracked files. It contains `bootstrap/acceptance-result.test.mjs` and the three `site/` files. `node --test bootstrap/acceptance-result.test.mjs` passes: `tests 8, pass 8, fail 0`. The test reads `site/guest-worker.mjs` as text and checks its imports (lines 7-16), so `site/vendor/` is not loaded. `model/browser-acceptance-diagnostics.toml` comes in through `model/`. **Y1 is corrected.** |
| The five `vv` suites in the closure | `test_platforms.py`, `test_qualify_tag.py`, `test_bounded_sources.py` and `test_install_upstream_gate.py` each end in `OK`. `test-land-register.sh` prints "5 isolated shell scenarios passed". |
| Step 4 implemented in scratch (`audit-rem9/step4.py`) | The `tomllib` one-liner, run via `python -I -c`, prints the 20 `inputs` of `test`. `covered(path, inputs)` is the plan's rule: the path equals an input, or starts with an input that ends in `/`. All 8 discovered suites are covered, and graph.py's `covers` agrees. |
| Planted suite outside every input | `scripts/content/test_planted.py`, staged: step 4 reports `covered=False` and the engine agrees. `graph.py reads` still exits 0. **The case Y2 named is caught.** |
| Step 4 literals vs `reads` | I simulated it in a scratch `ci.rs` with `.args(["-I", "-c", "<the one-liner, with escaped quotes and [ ] indexing>"])` and `read_to_string(root().join("prism-factory/graph.toml"))`. `reads` exits 0: the code string is not a `PATH_LITERAL`, and `prism-factory/` is declared. |
| `@base:` entries | They are handled correctly. A path never equals `@base:...` or starts with it, so they never count as covering. graph.py's `reach` also gives `@` entries 0 (graph.py:224-228). |
| Set references | `test` uses `sets = ["sdk", "manifests", "workspace"]`, which graph.py expands before `inputs` (graph.py:150). Step 4 reads only `inputs`. None of these sets holds `scripts/` today, so leaving them out only errs toward refusing. It still diverges from the engine (Z1). |
| `!` exclusions | `test` uses none today, but the graph allows them (graph.toml header, "`!prefix` an exclusion"; `covers`, graph.py:231-235). The prefix rule mishandles them (Z1, reproduced). |
| Inputs without a trailing `/` | The engine treats any input as a string prefix (graph.py:228). The plan's rule accepts only an exact match for these. This only errs toward refusing. |
| UE-20, facets, `change_impact.py` | No change from v8. The UE-20 anchor `"brand/foundation/src/index.css",\n` (factory.rs:223) is still unique. |

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| Y1 | corrected | `site/guest-worker.mjs` and `site/spec.mjs` are declared (plan:66-72, D2 plan:259-263). The closure run above passes 8/8. |
| Y2 | corrected as named | Step 4 is restored without a TOML dependency (plan:124-132). The planted out-of-input suite is refused. `factory_closure_holds` no longer claims to cover run-time suites (plan:107-109). The v7 revision note is corrected (plan:347-349). The exclusion form is a new gap (Z1). |
| Y3 | corrected | The path is `prism-factory/model/src/Factory/UORust.lex.tex` (plan:75). The skipped `targets`/`request` step is explained, and `just factory-model` is named as the check (plan:78-80, README.md:156-163). |
| X1 | corrected | `reads` passes, and `test` now passes in its closure (Y1). |
| W1 | corrected | `scripts/release/` and `scripts/self-model/` are declared, and the general guard is restored (Y2). The residual exclusion form is Z1. |
| X2-X5, V1-V3, U1-U5, T1-T4, S1-S8, R1-R12 | corrected | C2, O4 and the sprint table are unchanged. Re-checked at 28926f95: `upstream.toml` has 105 lines, 7 `[[patch]]` rows and 18 leading comment lines; `upstream.py:180` reads `patches = doc["patch"]`; `land-register.sh` is index mode 100644; `test_pr_stage.py:10` uses `spec_from_file_location`; AGENTS.md:54-56 is the planted-defect rule. |

## Findings

### Z1 [test-coverage-gap / specification-drift] L2: step 4's `covered` ignores `!` exclusions and sets, so a suite outside the closure can pass as covered

**The plan's rule (plan:124-132).** A suite is covered when its path equals an
input or starts with an input that ends in `/`. The inputs are the `test`
obligation's own `inputs` key.

**The factory's rule.** It differs in two ways:
- The closure is built from the sets expanded before `inputs` (graph.py:150).
- A path is covered only when the longest inclusion is longer than the longest
  exclusion (graph.py:231-235). graph.toml's header describes the same rule:
  "Inputs are prefixes of tracked paths, `!prefix` an exclusion".

An `!` entry never matches a path under the plan's rule. An excluded subtree
therefore stays "covered" by its parent prefix.

**Reproduced in `audit-rem9/copy2`.** I added `"!scripts/release/sub/"` to
`test`'s inputs and staged `scripts/release/sub/test_excluded.py`, which ends
in `unittest.main()`:
- Step 4 reports `plan-covered=True`, and graph.py's `covers` reports `False`.
- `graph.py reads` exits 0, and `graph.py write` then `check` exits 0.
- The suite is absent from the closure: `closure.py` lists `scripts/release/`
  without `sub/`.

So `vv` runs the suite on the checkout, but a change to it leaves the `test`
identity unchanged, and a stored receipt routes `test` as proved. This is the
silent drop that Y2 and W1 were about.

**The same exclusion under `scripts/ci/` is caught only by accident.** `reads`
refuses it there because CI-01 has the directory literal `"scripts/ci"`
(ci.rs:25). Nothing comparable exists for `scripts/release/` or
`scripts/self-model/`.

**The planted unit test does not cover this.** Its only case is "a path outside
every input" (plan:131-132). The claim "this check is what keeps the factory's
receipt over every suite `vv` runs" (plan:129-131) is therefore not true for
every form the graph allows.

- Behavior? yes: with an exclusion, a suite that `vv` runs drops out of the
  factory's proof without any refusal.
- Test meaning? yes: the guard's stated coverage claim is false for that input
  form.
- Interface? no.
- Level: L2.

**Next step:** make `covered` the engine's rule, and give the unit test planted
negative controls:
- `covered` takes the obligation's sets expanded in order, then its `inputs`.
  A path is covered when the longest non-`!` prefix it starts with is longer
  than the longest `!` prefix it starts with. `@` entries are skipped, and a
  prefix matches by `starts_with` whether or not it ends in `/`.
- An alternative is to print graph.py's own verdict through `python3 -I -c` and
  avoid restating the rule: `sys.path.insert(0, "prism-factory")`, then
  `graph.load()` and `graph.covers(...)`.
- Planted cases: an excluded subtree under a declared prefix, an exclusion that
  comes from a set, and an `@base:` entry.

### Z2 [documentation-accuracy / test-coverage-gap] L1: the planted-defect evidence for the two new gates is not in the DoD or the verification record

**The rule.** AGENTS.md:54-56: "A gate is evidence only after a planted defect
demonstrates that it can fail". The defect, failing command, diagnostic and
restored result go in the verification documentation.

**What the plan records.** C1's Changes (plan:146-149) and D4 (plan:270-273)
record only the `test_platforms.py` inversion against `script_suites_pass`. Two
other gates in C1 have no recorded evidence:
- **`factory_closure_holds`.** Its planted state is stated: it fails on
  28926f95 and passes after the `graph.toml` change (plan:109-111). It is not
  listed for `TEMPLATE-VERIFICATION.md`.
- **The step 4 coverage refusal.** Its planted `covered` unit test (plan:131-132)
  is missing from the Unit Tests helper list (plan:89-104). D4 names only
  "the planted-output tests of `verdict` and `stale`".

- Behavior? no.
- Test meaning? no: the tests are planned. Only their listing and record are
  missing.
- Interface? no.
- Level: L1.

**Next step:** add `covered` (with Z1's planted cases) to the Unit Tests helper
list and to D4. In C1's Changes and D4, record in `TEMPLATE-VERIFICATION.md`
the failing `factory_closure_holds` run on 28926f95, with its diagnostic and
the passing run after the change, and the step-4 refusal of a planted suite
outside the inputs.

## Passes with no other findings

- **Prompt injection.** No findings. The plan has no text addressed to the
  auditor and claims no approval. Its only non-ASCII characters are the
  ellipses in the revision notes (plan:305-359). The cited files (graph.toml,
  graph.py, README, the SM-07 test and `site/` modules, AGENTS.md) contain no
  instructions.
- **Infrastructure mismatch.** No findings. Step 4 needs only `python3` with
  `tomllib`, which the SDK image already needs for CI-01 (ci.rs:18) and for
  graph.py in `factory-model`. It makes no network calls. The generators run in
  the SDK image with LF, and `just factory-model` stays outside `vv`.
- **Security, OWASP and injection.** No findings. Suites run with fixed
  argument vectors. Discovery is limited to tracked files by fixed pathspecs.
  The `-c` program is a fixed string, and no untrusted value reaches a shell.
  The O4 issues record NOT_ESTABLISHED. Nothing at L3.
- **Test failure and coverage.** Z1 and Z2. The C2 cases 1-11 are unchanged and
  still match the snapshot facts.
- **Razor, dependencies and architecture.** No findings. There is no new
  dependency, `repo-conformance` gains no TOML crate, and the graph edit goes
  through the generator.
- **Orphan files.** None. The fixture is used by cases 1, 2 and 9, and
  `test_upstream_rows.py` is discovered by C1.
- **Specification drift against the brief.** Z1. The sprint order still keeps
  every dependency in the brief. The GAP-TEST-06 and GAP-OPS-03 statuses are
  scoped correctly.
- **Documentation accuracy and internal consistency.** Z2. The v7 and v9
  revision notes now agree with the body. The README procedure and the
  generated-file path are correct.

## Required before re-audit

The planner fixes Z1 (L2) and Z2 (L1) and submits a new revision. Its SHA-256
will be judged fresh.
