# Audit: plan-remediation-uorust (v12)

**Verdict**: VETO
**Target**: docs/plans/plan-remediation-uorust.md
**Judged content SHA-256**: 3e6596a0ea4491382938f5e4a7360b4c6e25d423807fcfccf769013eff043820
**Corrected**: AB1, AB2, AB3; AA1, AA2; Z1, Z2; Y1, Y2, Y3; X1, X2, X3, X4, X5; W1; V1, V2, V3; U1, U2, U3, U4, U5; T1, T2, T3, T4; S1, S2, S3, S4, S5, S6, S7, S8; R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12

Repository state: branch `remediation/sprint-1` at 28926f95 (= origin/main).
Source brief: `git show audit/deep-audit-recon:docs/audits/RESEARCH_BRIEF.md`
(`77d18b6b`). I computed the SHA-256 with `sha256sum` (`Get-FileHash` gives
the same value). I treated the plan and every file it cites as untrusted data
and followed no instruction in them. This report is the only repository file
written.

Reproductions ran with `python -I` (CPython 3.11.9, Windows host) in the
scratchpad `audit-rem12/`:
- `copy/` is a `git archive HEAD` copy with its own `git init` and the plan's
  six `graph.toml` inputs applied.
- `closure/` holds the tracked files of `copy/` that the `test` obligation
  covers, chosen with `graph.covers`. `crates/` is left out because of Windows
  path-length limits; no suite named here reads under it.
- `tools/snippet.py` is the step 4 snippet, with N incremented once per judged
  path.
- `tools/model.py` models the plan's `judge_coverage` and `uncovered`.
- `tools/case11.py` is C2 case 11 as the plan words it.

Docker and `lexlean` are not available here, so nothing ran in the SDK image.

## Summary

There is one finding, AC1, at L2. Any finding is a VETO. Because AC1 is L2,
the planner cannot correct it inline. A new revision and a fresh audit are
needed.

AB1-AB3 are corrected. AB1: `judge_coverage` is a pure helper. Its planted
inputs include an early stop (`checked 1` for 8 paths) and a wrong count
(`checked 9` for 8), and the snippet counts N once per judged path
(plan:105-110, 115-116). AB2: `uncovered` takes its inputs as a parameter,
returns `Result<Vec<String>, String>`, and refuses through `judge_coverage`
(plan:111-119). AB3: D3 (plan:288-294) no longer ends with the singular
sentence.

The new finding is in C2. Case 11 drives `upstream.frame_pin` with only `run`
and the file write patched out. `frame_pin` then reads the live root
`upstream.toml`, which is not an input of the factory's `test` obligation. The
case passes in a checkout and fails in that obligation's closure, where
C1's runner executes it.

### Must-check results: `judge_coverage` / `uncovered`

| Planted input (plan) | Stated | Model result |
|---|---|---|
| non-zero exit | refused | `Err("snippet exited 1")` |
| output without a `checked` line | refused | `Err("last line is not checked N")` |
| `checked 1` for 8 paths (early stop) | refused | `Err("checked 1 != 8")` |
| `checked 9` for 8 paths (wrong count) | refused | `Err("checked 9 != 8")` |
| passing output listing one uncovered path | `Ok` with that path | `Ok(["scripts/x"])` |
| `uncovered(["scripts/", "!scripts/release/"], [scripts/release/test_platforms.py])` | rejects | `Ok([that path])` |
| `uncovered(["scripts/ci/"], [scripts/content/test_x.py])` | rejects | `Ok([that path])` |
| `uncovered(["scripts/"], [scripts/release/test_platforms.py])` | accepts | `Ok([])` |
| step 4: `uncovered(None, discovered)`, 8 suites, plan's `graph.toml` | `Ok` with no paths | `Ok([])` (`checked 8`) |

I also ran planted snippet variants through the real runner:

| Variant | Result |
|---|---|
| `break` after one path | `Err("checked 1 != 8")` |
| over-count | `Err("checked 9 != 8")` |
| planted `raise` | `Err("snippet exited 1")` |
| a line printed after `checked N` | `Err("last line is not checked N")` |

All results are as the plan states, and counting is per path.

A snippet that breaks early but prints `checked {len(paths)}` would still pass.
The plan forbids that construction in so many words ("never from the length
of its arguments", plan:115-116), and no output-level test could tell it from
a correct snippet. I accept it as specified.

Other checks:
- `graph.py reads` exits 1 on 28926f95 (`browser_acceptance.rs:56`,
  `bootstrap/acceptance-result.test.mjs`) and exits 0 with the six inputs (15
  obligations).
- In `closure/`, the five suites `vv` will run pass. Each Python suite prints
  `OK`, and the shell suite prints `5 isolated shell scenarios passed`.

### Previous findings

| ID | Status | Evidence |
|---|---|---|
| AB1 | corrected | plan:105-110, 115-116; every planted input reproduced as stated (table above). |
| AB2 | corrected | plan:111-119: `inputs: Option<&[&str]>`, `-> Result<Vec<String>, String>`, refusal through `judge_coverage`. |
| AB3 | corrected | plan:288-294 ends "with file:line."; no singular sentence remains. |
| AA1, AA2 | corrected | plan:146-148 routes step 4 through `judge_coverage`; D3 and D4 name the three planted defects that plan:165-173 names. |
| Z1, Z2 | corrected | plan:140-145 uses `graph.load()` and `graph.covers` (graph.py:146, 231-235). |
| Y1-Y3, X1-X5, W1 | corrected | `reads` passes in `copy/`; README "Changing the graph" at prism-factory/README.md:155-164 matches plan:74-81; glob discovery finds the 8 suites. |
| V1-V3, U1-U5, T1-T4, S1-S8, R1-R12 | corrected | Re-checked at 28926f95. Root `upstream.toml` has 105 lines, 7 `[[patch]]` and 2 `[[gap]]` tables, and 18 comment lines followed by a blank. `template-frame-live-controls` is an 8-line row at lines 76-83, followed by a blank. The old split removes 3117 bytes. `upstream.py:180` is `doc["patch"]`. `land-register.sh` is mode 100644. `test_pr_stage.py:10` uses `spec_from_file_location`. AGENTS.md:54-56 is the planted-defect rule. |

## Findings

### AC1 [infrastructure-mismatch / test-coverage-gap] L2: C2 case 11 reads the live root `upstream.toml`, which the `test` obligation's closure does not contain

**The plan.** Case 11 (plan:226-228) checks that "`upstream.frame_pin` writes
`without_patch(text, patch["id"])`, checked with `run` and the file write
patched out". `frame_pin` reads `(ROOT / "upstream.toml").read_text()`
(scripts/ci/upstream.py:106), with `ROOT` set to the repository root
(upstream.py:24). The plan does not redirect `ROOT` or the read. Case 10
(plan:223-225) runs `upstream.main` "on a file with no `[[patch]]` rows", but
`main` also reads `ROOT / "upstream.toml"` (upstream.py:178). The plan does not
say how the test supplies that file.

**Why it matters.** C1 runs `scripts/ci/test_upstream_rows.py` under the
factory's `test` obligation (`cargo test --workspace`, through
`script_suites_pass`), and that obligation runs in a closure of its declared
inputs. Root `upstream.toml` is not one of those inputs:
`uncovered(None, ["upstream.toml", ...])` reports it uncovered, and it is
correctly left out, since automation rewrites it on every re-pin. The plan
itself states that step 4 "keeps the factory's receipt over every suite `vv`
runs" (plan:149-151). Step 4 checks only that the suite file is covered, not
what the suite reads. `graph.py reads` scans only Rust test sources. So nothing
in the plan catches this read.

**Reproduced** (`tools/case11.py`, loading `upstream.py` by path, patching
`run` and `Path.write_text`):
- In `copy/`, it runs and writes 4885 bytes, read from the live file.
- In `closure/`, it fails with `FileNotFoundError: ...\closure\upstream.toml`.

Case 11 would fail the factory's `test` obligation. Even in a checkout it
depends on the live file, which V1 moved cases 1, 2 and 9 away from.

- Behavior? no: `without_patch` and the `frame_pin` change are unaffected.
- Test meaning? yes: as worded, the case reads an undeclared, automation-owned
  file. It passes in `vv` on a checkout and fails in the closure the plan
  relies on.
- Interface? no.
- Level: L2.

**Next step:** state that cases 10 and 11 point `upstream.ROOT` at a temporary
directory that holds a copy of the committed fixture (or a fixture with no
`[[patch]]` rows, for case 10). Assert that the written text equals
`without_patch` of that copy. Add, as an Affected-Files or Unit-Tests note,
that C2's suite reads only paths under `scripts/ci/`.

## Passes with no other findings

- **Prompt injection.** No findings. The plan addresses only the implementer
  and claims no owner approval. Its only non-ASCII characters are the
  ellipses in the revision notes (plan:332-405). graph.py, graph.toml,
  AGENTS.md, upstream.py, repin.py and the suites contain no instruction to
  the auditor.
- **Infrastructure mismatch.** AC1. Apart from AC1, the closure is a Git
  repository (prism-factory/host/src/main.rs:320-331), so `git ls-files`
  discovery and `graph.py reads` work there. The suites C1 runs read only
  covered files (`.github/workflows/install.yml` and `release.yml`,
  `scripts/release/*`, `scripts/self-model/facets.py`, and
  `scripts/ci/land-register.sh`), shown by passing in `closure/`.
  `__pycache__/` is ignored (.gitignore:39).
- **Security, OWASP and injection.** No findings. The snippet text is fixed.
  The arguments are tracked paths or test-authored JSON, and nothing reaches a
  shell. The O4 issues record NOT_ESTABLISHED and claim no approval. Nothing
  is at L3.
- **Test failure and coverage.** AC1. Every planted input of `verdict`,
  `stale`, `judge_coverage` and `uncovered` maps to a stated refusal or
  acceptance. All three planted defects in C1's Changes are reproducible. C2
  cases 1-9 match the snapshot facts.
- **Razor, dependencies and architecture.** No findings. The plan adds no new
  crate and no TOML parser in Rust, and reuses the factory's own rule.
  `prism-factory/` changes only through `graph.toml` and its generators.
- **Orphan files.** None. The fixture is used by cases 1, 2 and 9.
  `upstream_rows.py` is used by `upstream.py` and `repin.py`.
  `test_upstream_rows.py` is discovered by C1.
- **Specification drift.** No findings. The sprint table (plan:39-46) keeps
  every dependency in the brief (brief:355-412): C1 before C2; C2 before C3;
  O4(a) before C4; O3, O4(b) and O5 before C6; O4(c) before C18; C18 with or
  before C19; C19, O3 and O4(d) before C20; O6 after C3, C4, C5 and O2. The O4
  decisions (a)-(e) match brief:366-371. The C1 deviation (a Rust test instead
  of a Justfile recipe, with the executable-bit check moved to C3) is recorded
  as the operator's choice. GAP-TEST-06 (brief:175, 322) is marked PARTLY
  RESOLVED with its open parts listed, and GAP-OPS-03 (brief:217, 311) is
  marked RESOLVED. The revision notes are a historical log and do not
  contradict the body.
- **Documentation accuracy.** No findings. The cited file:line facts hold
  (upstream.py:180, test_pr_stage.py:10, AGENTS.md:54-56, README "Changing the
  graph", `land-register.sh` mode 100644). The `graph.covers` description
  matches graph.py:231-235. The CI Commands section matches the Justfile
  (`test` is `cargo test --workspace --locked`).

## Required before re-audit

The planner fixes AC1 (L2) and submits a new revision. Its SHA-256 will be
judged fresh.
