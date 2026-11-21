# Template repository verification

This repository's `just vv` instantiates the universal verification policy in
`VERIFICATION.md` for the template itself, which is a Prism application: the
template contract checker in `app/`.

| `just` recipe | Enforces | ID classes |
| --- | --- | --- |
| `just fmt-check` | the diff is reviewable | --- |
| `just model` | R1, R4, R5 and the bootstrap trust root, in one `xtask validate` | `CM-01` |
| `just template-check` | immutable SDK selection and template drift, checked by the SDK | --- |
| `just lint` | Clippy at `-D warnings`, every target, every optional feature | --- |
| `just test` | the workspace suite, including the R3 meta-gate | `CM-02`, `CM-03` |
| `just deny` | R6 over the dependency graph | --- |
| `just app` | the Prism application: LexLean format and lock, then `prismpm verify`, which builds the holo and checks it | `SM-01`, `SM-02` |

Each check runs once per `just vv`. `just bdd` runs only the meta-gate while a
scenario is being written; `just test` already includes it.

The register holds UORust's claims (`SM-01` to `SM-04` for its own model,
and the engine's rows), so the meta-gate checks real rows, real scenarios and
real tests. It becomes an error as soon as an ID, scenario, or test is added
without the other two.

## Planted defects

| Gate | Planted defect | Result |
| --- | --- | --- |
| `check-model` | `CONFORMANCE.md` disagrees with the register | rejected |
| `audit-deferral` | a deferral marker in a crate and in the gate's own source | both rejected |
| honesty meta-gate | an ID with no test | rejected |
| `self-model-check` | a hand edit of a generated module (a stakeholder renamed in `app/src/Architecture.lex.tex`) | rejected |
| `self_model_sm_01` | a route dropped from `app/src/Routes.lex.tex`, or a vector of `model/routes.toml` missing from the application | both rejected |
| `prismpm verify` | an acceptance vector set without the exact guest-allocation boundary | rejected (PP2009) |
| UE-18 ratchet (`the_hand_written_translator_only_shrinks_ue_18`, `just ratchet`, the pre-commit hook) | a register with one more uncertified translator function than its parent's (`a_grown_translator_is_refused_ue_18`; in `certify`, a new residue function and a certified function whose κ changed) | rejected, naming `uorust::mir::lower::planted: residue (mir): mir::Rvalue::Use` |
| UE-18 differential and fixpoint (`certify agree`, `just certified-gate`) | a hand-written output one byte apart from the certified one's (`the_differential_refuses_one_byte_apart`) | rejected, naming both κ |
| UE-18 certified sources (`certify sync`) | a package source whose SHA-256 is not the one the bundle's result records | rejected |
| UE-18 certified dispatch (`certified!`, `--cfg=uorust_lift`) | a hand-written body whose certified dispatch the lift reads (`the_lift_reads_no_certified_dispatch`) | rejected: the dispatch statement is left out of the lifted syntax |
| `audit-bootstrap` | a floating/copied action, mutable SDK tag, changed universal file, narrowed boundary, shallow/credential-retaining checkout, copied project renderer, omitted standards lock, acceptance bypass, deploy-job signing privilege, incomplete release lifecycle, or a bootstrap that drops a native architecture from main or tags, cancels or replaces a main run, or skips runs by path, condition or matrix exclusion | all rejected |

`audit-deferral` reads every crate and `xtask`, including itself. Its token
construction therefore cannot exempt the very gate in which a deferral could
otherwise be hidden.

## One pull-request run, complete native coverage on main and tags

Every instance inherits this bootstrap. Before this change, each pull request
and each main push ran the complete `just vv` on both native runners, and
nothing cancelled a superseded pull-request run. Those runner minutes are
shared across the organization (UOR-Foundation/PrismPM#187). Repeating a
deterministic gate on the same inputs adds time, not evidence
(UOR-Foundation/PrismPM#210, UOR-Foundation/prism-code#25).

A pull request now runs the complete gate once, natively on amd64, and a newer
push to the same pull request cancels its older run. Every main push and every
tag runs the complete gate on both native architectures. Each non-pull-request
run has a concurrency group of its own. GitHub keeps at most one pending run
per group, so a shared main group would replace a queued commit's run before
it produced a verdict. The pull request's arm64 evidence therefore arrives on
the merge commit, before any tag of it publishes.

`native_coverage_is_complete`, part of `audit-bootstrap`, keeps that saving
sound. It requires the main and tag triggers, one of exactly two runner
matrices, and either no concurrency or exactly the pull-request-only
cancellation. The two admissible matrices are both architectures on every
event, or amd64 on pull requests and both elsewhere. The gate also rejects job
conditions, `continue-on-error`, matrix inclusions or exclusions, and path,
branch or tag filters.

The new test, `dropped_native_coverage_plant_is_rejected`, failed against the
previous workflow bytes: `assertion failed: native_coverage_is_complete(bootstrap)`.
It passes against the changed workflow, and rejects eight planted defects:
- arm64 removed from main and tags;
- the two matrix legs swapped;
- the tag trigger removed;
- unconditional cancellation;
- one shared main group;
- a `paths-ignore` filter;
- a job condition that skips arm64;
- a matrix exclusion of arm64.

Running both architectures on every event without concurrency stays
admissible, because it is stronger than the policy.

All 15 `xtask` tests, including both invoked Node cases, passed with the pinned
Rust 1.97.1 on `x86_64-pc-windows-gnu`, locked. Formatting passed, and
all-target Clippy with warnings denied passed for `xtask`. These ran outside
the SDK image, because no Docker host was available. Workspace Clippy on that
target also reports `result_large_err` in `repo-model` on unchanged `main`; it
is not part of this change. The change's own pull-request run on GitHub
evaluated both expressions and started exactly one job,
`acceptance / ubuntu-24.04`. That job then stopped before the gate, as every
template run does, because the template commits no `prismpm.lock`. The main,
tag and cancellation behavior have not yet run on a hosted runner.

## Platform-indexed SDK bootstrap

The `/2` SDK lock renderer delegates index and inventory validation to the
SDK-owned `platform-lock.mjs` helper. Transport extracts inventory and standards
files from each exact `linux/amd64` and `linux/arm64` image child using Docker
without executing the foreign architecture. The template native audit compares
the detected platform's complete inventory and bytes; the legacy `/1` branch
keeps its original exact comparison.

All 10 template `xtask` tests and all-target Clippy passed, locked and offline,
inside the existing PrismPM SDK as UID 1000. The new negative test rejects a
swapped native inventory, missing platform, and legacy architecture drift.
Shell and Node syntax checks passed. An isolated ignored Cargo target was used
because older root-owned fingerprints prevented reuse of the default target;
those unrelated artifacts were not changed. These focused tests do not claim
the template's full `just vv` has accepted an unpublished candidate SDK. A real
immutable multi-platform candidate and policy-input commit are still required
before generating and committing the new locks.

The initial standards binding adds two executable Node cases, also invoked
by the ordinary `xtask` test suite: absent and matching locks preserve the
exact supplied bytes; a differing existing lock is rejected without changing
it. The complete 11-test `xtask` suite, all-target Clippy and shell/Node syntax
checks passed inside the SDK as UID 1000. This is byte-binding evidence, not a
claim that the project has adopted or conforms to every referenced standard.

## Private Buildx state with read-only credentials

Foundry bootstrap run `35104180230` failed on both runner architectures before
the repository gate: Buildx tried to create `.docker/buildx` inside the
read-only credentials mount. The exact amd64 SDK candidate
`ghcr.io/uor-foundation/prismpm-sdk-candidate@sha256:c2e0e50437e13d9b2e382d3af4ae7a962b469721b9b80f14f215d9254e8ed78f`
reproduced `mkdir /tmp/prismpm-home/.docker/buildx: read-only file system`
under UID 1000, with network disabled, an empty test-only credentials file
mounted read-only, the socket's group, and the same private `/tmp` tmpfs.
No owner credentials were read or mounted.

With `BUILDX_CONFIG=/tmp/prismpm-buildx` and `umask 077`, actual `docker buildx
version` and `docker buildx inspect --bootstrap` passed. The state directory
was mode 0700, credentials stayed read-only and byte-identical, and no state
was created inside the credential directory. The default Docker driver was
inspected; no builder was created, selected, or removed. The disposable
container's state vanished when it exited. This is native amd64 execution,
not a claim that the updated hosted ARM job has already passed.

The native guard's planted missing-state regression failed before the fix.
It now rejects missing/duplicate configuration, paths inside either the
credentials directory or its parent, missing tmpfs and permissive creation.
An additional real-workflow mutation pair reproduced that the initial guard
accepted an appended `umask 022` before Buildx and a `umask 077` moved after
inspection. Both now fail: the scoped container-script check requires exactly
one umask directive, exactly 077, before its first Buildx command. The final
12-test xtask suite, both invoked Node tests, formatting and Clippy passed
again; the actual Docker workflow was already correct and remained unchanged.
All 20 workspace Rust tests (including all 12 xtask tests), both invoked
Node tests, locked offline Clippy, all-features/all-target checks, formatting,
model comparison and YAML/embedded-shell syntax passed under that exact SDK.
The YAML check used the SDK's existing `js-yaml`; no tooling was installed.
Raw RED/GREEN logs are retained under `target/template-buildx-*`.

The template remains unbound: these component checks do not replace its
required complete `just vv`, accept a production SDK, or waive universal
policy. Downstream repositories must receive the changed workflow and policy
through their normal reviewed template update.

## SDK-owned dependency maintenance

Dependabot uses canonical JSON-form YAML, checked structurally by
`audit-dependency-updates` within `audit-bootstrap`. Duplicate keys, malformed
values, missing or broader exclusions, and disabled maintenance are rejected.
Only the SHA-pinned `UOR-Foundation/PrismPM/action` and byte-bound
`.github/workflows/bootstrap.yml` use the reviewed SDK/template update flow.
Other Actions and Cargo dependencies retain their weekly schedules and groups;
both now request Conventional Commit prefixes.

GitHub documents [dependency ignores and path exclusions](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference).
The upstream [SHA-pinned subpath parser](https://github.com/dependabot/dependabot-core/blob/dc25739e082c6d97399b476ad4eb06b5559895e7/github_actions/lib/dependabot/github_actions/file_parser.rb#L148)
confirms the exact action dependency name. The existing SDK's `js-yaml` 4.3.2
independently parsed the configuration and preserved the prior schedules and
groups; this inspection is not a new SDK API or general YAML conformance claim.

The previous configuration failed the new gate. A canonical broad `*` ignore
also failed the actual command; restoring the exact action passed. Negative
tests cover omitted/wrong/extra exclusions, disabled or duplicate ecosystems,
removed Cargo coverage, altered scheduling, and duplicate JSON keys.
The byte-wise SHA formatter preserves exact empty-string and `abc` digests
with unchanged sha2 0.10.9; the same correction fixes sha2 0.11 compilation in
the separately verified Foundry dependency update. No dependency was changed
here. Logs are retained under `target/template-dependabot-*` and
`target/template-maintenance-*`.

In the exact native AMD64 SDK above, all 22 workspace Rust tests and both
invoked Node tests, formatting, Clippy, all-feature/all-target compilation,
model/limit/deferral audits and dependency checks passed. Full `just vv`
passes the new policy check, then rejects the template's existing absent
`prismpm.lock`. No universal policy bytes changed or release gate was waived.

## Each check once, and the R3 scaffold

`just vv` used to run `check-model` and `audit-bootstrap` twice (once in
`template-check`, again inside `xtask validate`), the `repo-conformance` tests
twice (`test`, then `bdd`), and the workspace through two compilers that differ
only in features (`lint` at the default features, then `features`, a
`cargo check --all-features`). Repeating a deterministic check on the same
inputs adds time, not evidence. Now `model` is the one `xtask validate`,
`template-check` is only the SDK's two checks, `lint` is Clippy with
`--all-features` (so an optional feature is still compiled, with its tests, and
now also linted), and `test` compiles the default features. Every Cargo
invocation passes `--locked`, so no part of the gate can resolve a dependency
the lock does not name (R6). The `cargo vv` alias, which ran `xtask validate`
and so was not the gate its name claimed, is removed.

`cargo xtask new-capability <ID> <level> <suite> <statement>` (`just
new-capability`) writes the register row, the tagged scenario and a test named
`<suite>_<id>` that fails until the capability is built, then regenerates
`CONFORMANCE.md`. Its tests run the honesty meta-gate on a scratch copy of the
register: two scaffolded capabilities leave it clean, and a lowercase ID, a
missing hyphen or number, an unknown level, an uppercase or path-escaping
suite, an empty or two-line statement, too few arguments and a duplicate ID
are all refused with the register byte-identical and no suite file written.

On a copy of this repository, `just new-capability CX-01 build parsing ...`
produced files that passed `cargo fmt --check` and all-feature Clippy, and
`cargo test` then failed exactly at `parsing_cx_01`, as R3 requires. The 18
`xtask` tests, all-target all-feature Clippy, formatting and `audit-deferral`
passed with the pinned Rust 1.97.1 on `x86_64-unknown-linux-gnu`, locked,
outside the SDK image; the SDK checks run in the pull request's bootstrap job.

## UORust's route table

`app/` is UORust's own Prism model. Its application, modeled in
`app/src/Routes.lex.tex` on the `prismpm/text-application/3` profile with a
workbench (`app/src/Guide.lex.tex`, `app/src/Frame.lex.tex`), is the route
table of `model/routes.toml`: a request is one Rust fact's kinds, one per line,
and the answer is the one route they all name, `refused unknown-fact`,
`refused ambiguous`, or `error` for an empty, oversized, non-UTF-8 or
blank-line request. The theorems `routeTableIsTotal`, `routeFactsAreDistinct`,
`routeTableRefusesAmbiguity` and `routeIndexesArePositions` are decided over
the whole table. `self_model_sm_01` and `self_model_sm_02` read the model as
text and hold the table, `routeOf`, the acceptance vectors and the theorem
statements to `model/routes.toml`.

Constraints met on the way, each a refusal of the pinned SDK: a request of
exactly `guestAllocationMaximum` bytes must be a vector (PP2009); a text/3
command line needs an outcome named `error`, and the workbench purpose must
equal its description; generated code refuses a `List` used as an
intermediate value and a static list constant on the runtime path, so the
table is answered by an `if` chain (`routeOf`) and the list (`routes`) serves
the theorems; generated Rust has one namespace, so no two definitions share a
name across modules; and every definition states its exact axioms.

Locally, in the locked SDK: `lexlean verify --all` verifies all 19 modules,
including every decide theorem, and `prismpm --project app check` passes with
every facet (PP2012, PP2014 to PP2022, PP2101). `prismpm --project app verify`
builds and replays the holo and then stops in the SDK's pinned Hologram
oracle (`PP5301: attached portable View asset closure/order is not
canonical`): the oracle expects a View of exactly `app.css`, `app.js` and
`index.html`, and the SDK's own build attaches the brand files beside them.

## Prism accounts: the remembered passkey

The sign-in page (`identity/id.mjs`) remembers which passkey this device last
signed in with, so the browser asks for that one and skips its chooser, and
it names the device's authenticator ("Secured by Windows Hello"). The hint
must never decide that a device has no passkey: if it could, someone with an
account whose remembered passkey was removed would be sent to Create account
and end up with a second account.

| Gate | Planted defect | Result |
| --- | --- | --- |
| `npm run test:identity`, "a remembered passkey that is gone asks for any passkey, never straight to a new account" | the branch that forgets a remembered passkey which did not answer, removed from `run()` | rejected: `locator.waitFor: Timeout 10000ms exceeded` waiting for `#go-label` to read `Sign in`; restored, 20 of 20 pass |
| `npm run test:identity`, "a click the bar is redrawn under still opens the panel" | the frame before this fix, which opened the panel it was clicked on even after the bar was redrawn (found on holo-search: a first click right after load did nothing) | rejected: `not ok 2`, waiting for `#account-panel:popover-open`; fixed, 21 of 21 pass |
| same test, after the account panel became a centred dialog and the button reads "Continue with Windows Hello" | the same branch removed again | rejected: the button read `Create account` where `Continue with Windows Hello` was required; restored, 21 of 21 pass |

## The workspace page

The standard page became the workspace: a header row, the app's surface
beside a rail for its result, actions and history, and a spec that opens
beside the window (the frame's window and bar narrow by `--prism-aside`)
and loads only when opened. Plain dark replaced immersive as the default
theme. The page gates were run on three apps built with these files, as
`frame-sync.mjs --pin` gives them (Leap year, Luhn check, Legal chess, each
with its own holo and LexLean source):

| Gate | Result |
| --- | --- |
| `node bootstrap/screens.mjs site screens`, six size classes in three themes, the spec open and closed | every class passes in every theme, all three apps |
| `lhci autorun`, three runs each | 100 in all four categories, CLS 0, LCP about 1.21 s; 105 KiB (Leap year, Luhn) and 122,701 of 122,880 bytes (Legal chess, whose live page was already within 1 KiB of the budget) |
| `npm run test:identity`, `npm run test:try` | 21 of 21, 1 of 1 |

The screens gate's new check, that the workspace fits the screen, was first
written as "the page does not scroll". A planted defect showed it could not
fail: the page hides its overflow, so a workspace taller than the screen is
cut off rather than scrolled.

| Gate | Planted defect | Result |
| --- | --- | --- |
| `screens.mjs`, the workspace fits the screen (scroll only) | `main` made 4 rem taller than the screen | passed: the defect went unseen |
| same, checking that `main` ends inside the screen and holds its content | the same 4 rem | rejected in all 18 cases: `the workspace does not fit the screen: the page would scroll or cut it off`; restored, the three apps pass |

Lighthouse found two more before the change was done: the display's type
shrank once the script set its width (CLS 0.009 on Luhn check, fixed by
setting the first width in the hashed stylesheet), and `app.mjs` crossed the
unminified-JavaScript allowance (2,458 bytes against 2,048, fixed by moving
the spec into `site/spec.mjs`, loaded on first open, and shortening comments).

## A newer frame may add a file

`frame-sync.mjs --pin` judged a target commit's lock by this checkout's own
frame list, so the first frame to add files (`site/spec.css`, `site/spec.mjs`)
could not be pinned by any app that had an older script. The target is now
judged by its own frame: its `frame-sync.mjs`, matched to its lock, names the
files, read as text and never run, and each must sit under `bootstrap/` or
`site/` (or be `lighthouserc.json`). `--check` is unchanged. Run against real
commits and against mirrors served with `PRISM_TEMPLATE_RAW`:

| Case | Result |
| --- | --- |
| the old logic with the old fourteen-file list pins `128d80d` | refused: `frame.lock at 128d80d… is not a prism/frame-lock/1 lock of this frame` (the defect) |
| the new logic with the old list pins `128d80d` | pinned; `--check` then passes, 16 files |
| the new logic pins back to `76bdeb0` | pinned; `site/spec.css` and `site/spec.mjs` reported as no longer part of the frame |
| a mirror serves `site/app.mjs` with one line added | refused: its digest against the lock's; nothing written |
| a mirror's lock names a file its frame does not | refused: the lock's list against the frame's; nothing written |
| a mirror's frame and lock both add `.github/workflows/x.yml` | refused: `names files outside the page`; nothing written |
| an untouched mirror | pinned |

## Browser acceptance and authoritative frame provenance (SM-07)

The site frame is byte-locked to an immutable `UOR-Foundation/template` source
commit in `frame.lock`; the matching source pins the acceptance-result validator
alongside its Worker and page entrypoints. The `SM-07` suite checks that failed
vector identities remain visible, invalid results are refused with three stable
codes declared in `model/browser-acceptance-diagnostics.toml`, and private
request buffers are not forwarded to the display. Its Rust conformance test
invokes eight Node cases through the complete `just vv` workspace gate.

The Pages workflow must still complete on merged main to establish deployment.

### SM-07 planted-defect and restored control

The non-vacuity oracle is the `require_complete_tap` function in
`crates/conformance/tests/browser_acceptance.rs`. The same test first
executes the real eight-case Node acceptance suite, then plants two defects
**in its actual TAP output** before testing the unmodified result:

| Plant | Input mutation | Required rejection |
| --- | --- | --- |
| Skipped tests | Replace the real `# pass 8` and `# skipped 0` summaries with `# pass 0` and `# skipped 8` | `SM-07: invalid TAP summary for pass` |
| Missing required case | Rename the actual `worker preserves a zero-failures result` subtest | `SM-07: required test identity was skipped, removed, renamed or repeated` |
| Restored control | Use the unmodified Node test transcript | Exactly eight named tests; `tests 8`, `pass 8`, `fail 0`, `cancelled 0`, `skipped 0`, `todo 0`; `require_complete_tap` accepts |

**Reproduction/acceptance command:** `cargo test --workspace --locked`,
invoked by the locked-SDK `just vv` gate. The two planted cases are expected
to return `Err` and the restored input must return `Ok`; the Rust test
fails if any negative plant is accidentally accepted or if the clean
transcript is rejected. This is an embedded negative-control check rather
than a separate intentionally red PR workflow. The exact-head hosted
bootstrap run is the execution receipt; until that run passes, these
remain source-level expected results, **not verified hosted outcomes**.

## Script suites inside the acceptance gate

Every Python and shell test suite under `scripts/` now runs under `just vv`
through `script_suites_pass` in `crates/conformance/tests/ci.rs`. Suites are
discovered with `git ls-files` glob pathspecs, so a new suite runs without
editing the test. A suite is refused when it exits non-zero, runs no test,
skips one or reports an expected failure (`verdict`), and every discovered suite
must lie inside the factory's `test` obligation, judged by the factory's own
`graph.covers` (`uncovered`). `factory_closure_holds` runs
`prism-factory/graph.py reads`, which refused on `28926f95` because SM-07 reads
`bootstrap/acceptance-result.test.mjs` without the obligation declaring it; the
obligation now declares that file, the three site modules it loads,
`scripts/release/` and `scripts/self-model/`, and the factory's text and
certified bundle were regenerated in the SDK image by `prism-factory/README.md`,
"Changing the graph" (`just factory-model` passes).

The node suites `try/test.mjs` and `identity/test.mjs` drive Chromium through
Playwright, which the SDK image does not carry; they stay in `try-page.yml`.

### Planted defects and restored controls

Run in the pinned SDK image, each with
`cargo test -q --locked -p repo-conformance --test ci <test>`:

| Plant | Check | Result with the plant | Restored |
| --- | --- | --- | --- |
| First `assertEqual` in `scripts/release/test_platforms.py` inverted | `script_suites_pass` | refused: `scripts/release/test_platforms.py: the suite exited non-zero` (`FAILED (failures=1)`) | passes |
| `prism-factory/graph.toml` as on `28926f95` | `factory_closure_holds` | refused: `test reads bootstrap/acceptance-result.test.mjs (crates/conformance/tests/browser_acceptance.rs:56) without declaring it` | passes |
| `"!scripts/release/"` added to the `test` obligation's inputs | `script_suites_pass` (coverage step) | refused: `suites outside the factory test obligation` | passes |

The planted-output unit tests `verdict_accepts_only_a_suite_that_ran_and_passed`,
`stale_names_exclusions_that_no_longer_exist`,
`judge_coverage_refuses_a_run_it_cannot_trust` and
`uncovered_follows_the_factory_rule` cover each refusal without a live failure:
a non-zero exit, no or zero `Ran` count, `OK (skipped=1)`,
`OK (expected failures=1)`, a cross-kind summary, a stale exclusion, a coverage
run that stops early (`checked 1` for 8) or over-counts (`checked 9` for 8), and
an `!` exclusion.

### Review follow-up: suites run elsewhere, and the closure's view

Two gaps found in review are closed. First, the three suites `vv` runs through
another test were held only to their exit status. CI-01 now judges
`test_pr_stage.py` through `verdict`, and that suite's nested run of
`test_change_impact.py` checks the `Ran` count and an exact `OK` summary.
`test_pages_identity.py` already refuses an incomplete run itself
(`complete_result`), as `test_obligation_plan.py` (CI-02, from `main`) does. Second, the factory runs the `test` obligation in a
closure that holds only its inputs, where a suite outside them was absent
rather than refused. `scripts/ci/suites.txt` lists every suite and lies in an
input; `script_suites_pass` refuses the list when it and the tree disagree
(`inventory_diff`, with a planted-output unit test).

| Plant | Check | Result with the plant | Restored |
| --- | --- | --- | --- |
| `scripts/release/` removed from the index, as in a closure without it | `script_suites_pass` | refused: `listed, not in this tree: scripts/release/test_platforms.py`, `…/test_qualify_tag.py` | passes |
| `scripts/release/test_platforms.py` dropped from `scripts/ci/suites.txt` | `script_suites_pass` | refused: `in this tree, not listed in scripts/ci/suites.txt: scripts/release/test_platforms.py` | passes |
| One test in `scripts/ci/test_change_impact.py` skipped | `trusted_change_impact_classification_ci_01` | refused: `AssertionError: 'OK (skipped=1)' != 'OK'` (nested check) | passes |
| One test in `scripts/ci/test_pr_stage.py` skipped | `trusted_change_impact_classification_ci_01` | refused: `summary Some("OK (skipped=1)") is not exactly OK` | passes |

## Removing an upstream.toml patch row keeps the gaps (OPS-03)

`scripts/ci/upstream.py` `frame_pin` split `upstream.toml` on `\n[[patch]]\n`
and dropped the matching chunk, so removing the last patch row also removed the
comment and both `[[gap]]` tables after it (3117 bytes of the file at
`28926f95`), and `scripts/ci/repin.py` `drop_rows` dropped a comment that
precedes the next table. Both now call `scripts/ci/upstream_rows.py`
`without_patch`, which removes a row by lines from its header, keeps the
preamble and every comment that precedes a table, and refuses (`ValueError`)
any shape its line rule cannot handle, checked by parsing the result with
`tomllib`. `upstream.py main` reads `doc.get("patch", [])`, so a file with no
patch rows left runs.

`scripts/ci/test_upstream_rows.py` (12 cases, run by `script_suites_pass`)
reads only the committed snapshot `scripts/ci/fixtures/upstream.toml` or inline
fixtures. Planted defect: with `without_patch` replaced by the old split, the
suite fails 7 of its cases, among them
`test_last_patch_keeps_gaps_and_their_comment` (the gaps lost),
`test_only_remaining_patch` and `test_main_with_no_patches_acts_on_none`
(`KeyError: 'patch'`); with the new rule all 12 pass
(`python3 -I scripts/ci/test_upstream_rows.py`: `Ran 12 tests`, `OK`).

## Gate scanners that stopped refusing (GATE-01..04)

Four source-reading gates could pass on the defect they exist to catch.
`names_id` (`crates/conformance/src/meta.rs`) needs the `_` boundary before an
ID's slug, so `due_1` no longer discharges `UE-1`. `test_names_in`
(`crates/conformance/tests/bdd.rs`) no longer counts ignored, `cfg_attr`-ignored
or conditionally compiled tests, tests in a `cfg`-gated inline module or file,
or tests in fixture crates; a module gated in another file is not followed
(GAP-GATE-02 stays partly open for that case). `uses_reference` and `floats`
(`xtask/src/bootstrap.rs`) read the list-item form `- uses:` with any
whitespace after the dash. `effective_lines` (`xtask/src/audit.rs`) ends a
`#[cfg(test)]` item at its own terminator by kind (field, match arm or item),
so `#[cfg(test)] mod generate;` no longer hides the shipped code after it.
On the real tree the census drops 19 fixture-crate names and every registered
ID still has its test; no newly exposed shipped line names an unsanctioned
error; every workflow action is pinned.

Run in the pinned SDK image (`cargo xtask validate` passes on the real tree):

| Plant | Check | Result with the plant | Restored |
| --- | --- | --- | --- |
| `names_id` reverted to `name.ends_with(slug)` | `cargo test -p repo-conformance --test bdd the_meta_gate_is_falsifiable_cm_02` | refused: `SM-01 must be reported missing with only an unbounded name` | passes |
| `#[ignore]` on `trusted_factory_obligation_plan_ci_02`, CI-02's only test | `... --test bdd every_id_has_a_scenario_and_a_test_cm_02` | refused: `CM-02: CI-02 is registered but no test name is ci_02 or ends in _ci_02` | passes |
| `- uses: actions/download-artifact@v4` in `certified.yml` | `cargo xtask audit-bootstrap` | refused: `certified.yml contains a floating workflow input` | passes |
| `#[cfg(test)] mod planted;` then `pub fn planted_limit() -> Result<(), String>` in `crates/uorust/src/lib.rs` | `cargo xtask audit-limits` | refused: `R5: ... crates/uorust/src/lib.rs:265:pub fn planted_limit() -> Result<(), String> {` | passes |

The planted-source unit tests cover each rule without a live failure:
`names_id_needs_an_underscore_boundary`,
`test_names_in_drops_ignored_and_conditional_tests` (twelve shapes),
`fixture_tests_are_not_counted`, `floating_workflow_input_plants_are_rejected`
(four step forms, tag and pin) and
`a_braceless_test_item_does_not_hide_the_next_item` (three braceless shapes
refused; ten test-only shapes, among them a multi-line signature, a `where`
clause, multi-line generics, `thread_local!`, `extern "C"` and a match arm,
kept exempt with the shipped item after each still refused).

## Ratchets that passed on missing or masked input (GATE-10..12)

`certify ratchet` now refuses an absent `--manifest` or `--base-manifest` by
name instead of reading it as a manifest that certifies nothing (which passed
every head), and `scripts/hooks/ratchet-staged.sh` empties its directory first
and passes an absent manifest through to that refusal. The ratchet refuses,
before its count, a root the base certifies that no longer lifts at the head
(`Profile::lost` in `crates/uorust/src/certify.rs`), so a lost root can no
longer be masked by one gained elsewhere; a function lifted under a new κ is
an edit owed to an export and stays under the count rule, and a deleted
function is not a loss. The other half of GAP-GATE-12, a definition changed
under an unchanged κ, stays open. `cargo xtask proof-reuse` refuses a run that
names no SDK image, and `proof-keep` stages the new proof, swaps it in,
confirms it and restores the earlier one on any failure (`install` in
`xtask/src/proof.rs`).

Run in the pinned SDK image:

| Plant | Check | Result with the plant | Restored |
| --- | --- | --- | --- |
| `crates/certified/manifest.toml` removed from the index | `scripts/hooks/ratchet-staged.sh` | refused: `certify ratchet: target/ratchet-staged/head-manifest.toml does not exist; a ratchet against an absent manifest measures nothing` | `507 translator functions uncertified, none more than in the base` |
| The lost-root refusal disabled | `cargo test -p uorust --lib a_root_that_no_longer_lifts` | refused: `not refused: Ok(1)` (2 uncertified at the base, 1 at the head) | passes |
| The empty-image check disabled | `cargo test -p xtask reuse_needs_an_sdk_image` | refused | passes |
| The old order: the kept proof removed before staging | `cargo test -p xtask keep_` | both keep tests refused: `kept proof: ... NotFound` | passes |

`ratchet_refuses_an_absent_manifest` (`crates/uorust/tests/certify.rs`) runs
the binary with each manifest missing in turn, and `a_deleted_root_is_not_a_loss`
and the unchanged `the_ratchet_holds_any_scope_and_a_scope_only_widens` hold
the rules the export dispatch in `scripts/ci/generate-branch.sh` relies on.
