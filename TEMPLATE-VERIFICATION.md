# Template repository verification

## Native feedback regressions (UE-40 through UE-44)

These checks use the immutable SDK selected by `prismpm.lock`. Each row was
registered with its scenario and an executable regression before its repair.
They are focused development evidence; release acceptance remains `just vv`.

| ID | Reproduced defect | Restored check |
| --- | --- | --- |
| UE-40 | A renamed unique workspace type did not lift, while an imported name could bind a function's generic parameter. | `renamed_unique_types_preserve_imports_and_generic_shadowing_ue_40` checks the original type, generic shadowing, and actual SDK verification. |
| UE-41 | A compiler-derived signed structure field was admitted into a native tuple with a different integer representation. | `native_aggregates_check_field_representations_ue_41` refuses incompatible tuple, structure, variant, array and option cases with typed field diagnostics; the compatible projection passes SDK type checking. |
| UE-42 | Native loop bodies transporting lock-admitted registry values produced no candidates. | `native_registry_carriers_require_locked_readings_ue_42` proves all five carrier functions through the SDK, rejects unmodeled JSON equality, and rejects a changed archive checksum. |
| UE-43 | Native trials checked unrelated modules and retained no structured SDK result. | `native_verification_scopes_feedback_and_reuses_decisions_ue_43` proves three candidates, refuses an overflow candidate, records only Behavior and Prelude, and replays identical verdicts with zero verifier runs. |
| UE-44 | Lean rejected a countdown carrying an invariant Boolean with `Unknown identifier x1`. | `native_loop_state_preserves_invariants_in_evidence_ue_44` proves the compiler-derived function using one changing loop-state parameter. The SDK checks the unreduced decrease obligation exactly before proving it. |

Run these regressions with `cargo test --locked -p uorust --test engine native_`.
The existing policy-hint regression also exercises corrected hints and unchanged
replay. Native trials now run SDK type checking before Lean verification and
write the actual SDK JSON to `uorust-proof-result.json` in their project-owned
scratch directory. Neither this feedback nor prior axiom hints confer proof
status: a changed translation digest still requires the SDK's successful verdict.

The MIR fixture was refreshed by the actual prover after loop-state lowering
changed its translations. Eight translations were proved and one refused in
three verifier runs; two unchanged proved rows were retained. In particular,
`walk::any_above`, previously refused for its invariant capture, now proves.
The fixture's overflow refusal remains a refusal. CLI and engine replay checks
require zero new verifier runs for the resulting register.

The complete gate is not represented as passed by these checks. The prior full
run's self-lift alias regression was repaired separately, and the translator's
UE-18 certification boundary still requires complete release acceptance.

## Collection transport and proof-history reuse (UE-45 and UE-46)

`native_hash_collections_reuse_unordered_carriers_ue_45` initially found no
native standard-map candidate. Its repair proves five compiler-derived
functions through the SDK: default and custom-hasher maps and sets, plus a
workspace homonym. Hasher inspection, iteration order and an unsupported
element type remain refused. This adds passive carriers, without a model for
hash-builder operations.

`native_verdicts_survive_compiler_input_selections_ue_46` initially reduced
the actual eleven-row fixture register to one row when given one compiler
body. It now retains all proved and refused rows across full, single-body and
full selections, with zero verifier runs throughout. A different digest for a
live translation still invalidates its previous decision; the policy-hint
regression requires actual SDK verification after that change. Historical
cache size is not a count of currently lifted functions: the current frontier
remains the authority for eligibility.

## Ordinary native float scopes (UE-47)

The initial `native_workspace_floats_share_source_readings_ue_47` run lacked
the ordinary f32 transport candidate. MIR target scopes now receive the same
core-derived primitive formats as source lifting, and core/std facade paths
resolve their supported inherent methods. Eight actual-SDK proofs cover float
transport, arithmetic, finite tests, bit conversions and a negative-zero
literal. Primitive headers retain their bit carriers despite a qualified
workspace homonym. `native_workspace_floats_preserve_root_names_ue_47` checks
a root declaration named f64 through its own Boolean field and SDK type
checking. Unsupported trigonometry remains unlifted. The existing literal
conformance test compares f64 positive and negative zero with Rust's actual
bit patterns; it does not assume a particular printed form of the model.

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

## Stable MIR identities and bounded proof work

UE-28 uses actual compiler MIR for three modules declaring the same method
name. The earlier allocator retained only two candidates; the repaired
allocator retains all three and preserves the last method's declaration when
only that method's dump is selected.

UE-29 plans trials by dependency components. The negative cases reject a zero
budget, an absent dependency, and a recursive component larger than the budget.
Its SDK regression verifies three countdown methods in single-function trials;
each first discovers its actual axiom set and then verifies that declaration.
A second run must reuse the unchanged proved digests without a verifier run.

UE-30 combines locked-source number readings with generic registry aliases.
Before the repair, expansion of `BigRational` to `Ratio<BigInt>` erased its
registered model and the identity-function assertion failed. The repaired
projection lifts that identity as the stored integer pair. Replacing the
locked rational archive checksum with zeros must leave it unlifted.

The development profile optimizes the projector itself, as it already does
its certified translator. The SDK gate still runs every check with debug
assertions and overflow checking enabled. This removes repeated unoptimized
whole-workspace traversals from proof and integration tests.

Saved state-threading, cell-field and trait-dictionary changes retain their
positive and counterexample tests. Unsupported-type fixtures use `UnsafeCell`
instead of newly readable cells. Embedding join fixtures use a display-trait
bound instead of newly readable marker bounds, so they still exercise the
embedding fallback. Their closure, source-site and per-function regression
assertions remain in place.


The UE-29 diagnostic regression also places a failure on a theorem's opening
line, before its `name` field. The previous lookup blamed the preceding
definition (`a` instead of `b`); the repaired lookup names the declaration
that opens on that line. Actual SDK failures on adjacent native obligations
exposed this case. The prover prints the full SDK diagnostics while retaining
compact refusal causes in the verdict register.

Engine assertions share the immutable checkout's self projection. The
byte-determinism regression explicitly takes two fresh projections, and
mutated fixture checkouts always project afresh, so sharing cannot mask those
negative cases.


## Scoped types and metadata MIR collection

UE-31 resolves qualified field and signature types before reducing their
paths. Its fixture distinguishes numeric and Boolean homonyms, an imported
bare type, a nested generic argument and an unresolved qualified path.
The previous projection refused the valid numeric getter. The corrected
projection lifts the four valid functions and keeps the unresolved one out.
Existing path diagnostics retain the exact declaration sites; a qualified
constructor whose body reading remains unsupported stays refused.

UE-32 compares the pinned compiler's complete pre-LLVM MIR inventory and
bytes under native emission and the production metadata flags. Private,
generic, closure and impl bodies are present, and the production target
emits metadata without a native archive. Dropping forced MIR encoding gives
an incomplete inventory and is detected. Independent Cargo library and
binary probes also succeed with these flags.


## Early propagation with mutable state (UE-48)

The registered source fixture first failed because every `?` in a body with
threaded mutable parameters was refused. The repaired projection returns the
current parameter values on both success and early failure. A fallible lifted
callee is unpacked before its result is matched, so its updates survive failure.
Result constructors use the return context without broadening float inference.
Option comparisons bind both operands at their concrete type before matching;
this prevents an untyped `None` in the generated Lean match.

Six reference Rust tests exercise Option and Result success and failure, a
state-changing fallible callee, and two mutable parameters. The actual pinned
SDK verifies all generated acceptance roots. A planted defect replaces the
updated state by the original parameter on the Option failure path: the same
SDK proves the option-failure acceptance proposition false and rejects it.
The fixture also confirms mutable aliases and an unmodeled bool-to-u8 error
conversion remain unlifted. All 421 library tests pass, including the existing
float inference and borrow-escape refusals. Complete release acceptance remains
separate from these focused checks.


## Locked numeric observations and machine proof campaigns (UE-49, UE-50)

UE-49 first failed on the absent native integer zero observation. The numeric
readings now share stored-ratio accessor terms between source and MIR lowering.
The exact checksum-admitted num-bigint 0.4.8 and num-rational 0.4.2 sources define
the predicates, signs, absolute values, accessors and identity constructors.
The compiler must resolve the declared crate and trait; a spelling alone does
not select a reading. Unsupported operations and wrong checksums stay refused.

The actual SDK proved 18 compiler-derived numeric functions. Nineteen closed
acceptance vectors over the emitted functions also pass the SDK. Rust reference
vectors confirm that a raw ratio 0/0 is both zero and one, and that absolute
value can preserve a negative denominator while negating only the numerator.
These representation-sensitive cases prevent replacing stored pairs with an
assumed normalized rational. Axiom policies come from the actual verifier's
observations; the tests reuse UORust's digest-bound native decisions rather than
repeating completed proof batches when acceptance vectors change.

UE-50 first failed because prove accepted no machine-output option. With
`prove --json`, UORust runs the proof campaign, writes its final verdict-backed
projection, then measures the register itself. Its JSON binds the frontier,
verdict register, projection manifest and LexLean lock to exact bytes and records
the running executable's digest. The report distinguishes an open frontier,
translation closure, and full-project SDK acceptance. A cached fixture campaign
uses zero verifier calls; an unowned output directory is refused and its planted
file is preserved. The caller need not parse prose, reconstruct the function
counts, or rerun a separate projection to obtain the final work queue.


## Width-derived constructors and semantic proof-cache context (UE-51, UE-52)

UE-51 first refused unsigned BigInt construction. The project-owned Prelude now
provides a structural binary fold over a declared bit count. Work is bounded by
the primitive word width, including u128::MAX; it does not count up to the value.
Unsigned source constructors and signed/unsigned native constructors share this
reading, with a width-derived two's-complement adjustment. Native rational
integer construction shares the existing stored-pair reading.

Thirteen native functions and 32 closed SDK boundary vectors pass, covering every
primitive integer width, pointer-sized integers, signed minima and all-one bit
patterns. Reference Rust vectors confirm the same boundary values. The SDK
preflight also checks the shared model directly before expensive proof runs.

UE-52 reproduced a stale-cache defect: changing a source helper from identity to
negation left its native caller's own tokens unchanged, and the old tool reused
the caller's decision without SDK work. Native digests now bind the source/type
context, bundled semantics, the pinned SDK lock and the transitive native
translation closure. The actual regression verifies the changed context, then
replays it with zero verifier calls. A planted Bool-as-Nat definition in the
owned shared module is rejected by the SDK as a typed campaign failure. It is
not misrecorded as a refusal of each unrelated native function. Existing legacy
fixture decisions are refreshed through the actual prover under the new binding;
no cached status or SDK object is edited to make it pass.


## Reachable native proof obligations (UE-53)

The planted regression supplied two native loops, one valid and one with an
unchecked u8 increment, beside an unrelated false source assertion. Before
the repair, both native functions were refused. The actual SDK now proves
the valid function and refuses the overflow obligation. The separate full
projection still contains the source assertion and fails SDK verification.

Native trials retain all candidate declarations and their transitive declaration
and type dependencies. Mutual groups remain whole; quoted text cannot introduce
references or delimit declarations. Missing roots and unsupported declaration
shapes are typed tool failures. This changes only trial inputs, and the proof
strategy is bound into native cache identities. A module-wide failure is split
by recursive components; an inconclusive final component returns a tool failure
with its raw SDK receipt rather than inventing per-function counterexamples.

The regression was red with zero proved and two refused, then passed with one
proved and one refused. The lexer regression also covers quoted braces, escaped
quotes, transitive type references and mutual siblings. These checks are focused
development evidence; complete release acceptance remains `just vv`.

## Exact native feedback module identity (UE-54)

A consumer projection also contains library projects. Looking up the first path
ending in Behavior.lex.tex selected an earlier library file for root SDK source
locations. Concrete overflow diagnostics therefore appeared unattributed and
triggered repeated batch splitting. The planted regression returned libraryClaim
instead of holdsNative before the repair. Feedback now reads the exact root
src/Behavior.lex.tex, the same file the native SDK invocation verifies. Library
modules remain independent inputs and cannot supply a root declaration's owner.

## Native reference-alias match evidence (UE-55)

The previous frozen producer refuses the paired enum, copied Option and replaced
enum fixtures with `ProjectionElem::Downcast`: a field read outside a match.
The compiler uses different temporary references for the discriminant and field
access even when both read the same immutable value. Native bindings now key by
the exact value term and its complete type. Copies share match evidence; a
functional update changes the key and cannot reuse a stale constructor binder.

Three compiler-derived functions pass actual SDK proof trials. Nine closed SDK
vectors agree with executable Rust reference cases, including replacement after
a prior match. The overflowing fourth function remains refused. A planted
assertion that replacement returns its stale value is rejected by the SDK
because its proposition is false. The regression also requires current native
candidate eligibility, so historical cached proofs cannot hide a translation
regression. These are focused development checks; complete `just vv` remains
the release gate.

## Native readonly shared owners (UE-57)

The compiler-derived regression initially fails with `mir::Ty: std::sync::Arc<Wide>`.
Native lowering now uses the source's existing content representation for the
standard Rc and Arc paths. Six native functions covering construction, cloning
and readonly field access pass actual SDK proof trials. Twenty-four closed SDK
vectors agree with executable Rust cases at zero, one, an intermediate value
and u32::MAX. Reference counts, pointer identity and shared Mutex contents
remain typed residue. Type tests reject unrelated same-named owners and RefCell
contents, and require both lock-admitted numeric readings before accepting an
expanded Arc<Ratio<BigInt>>. Complete `just vv` remains the release gate.

## Native unary numeric models (UE-58)

The regression initially refuses the compiler's i128 checked_neg call. One
width-derived rule now handles primitive checked negation, wrapping negation,
unsigned magnitude and signum. Guarded Option results materialize through the
existing read operation when returned. Exact BigInt and raw Ratio negation
reuse the lock-admitted numeric carriers; only the stored numerator is negated.

Six functions pass actual SDK proof trials. Twenty-two closed SDK vectors
cover primitive minima, maxima, zero and minus one, plus exact integer and raw
rational cases with a negative denominator. Executable Rust confirms the
stored rational representation. An unsupported unwrap stays typed residue.
A planted unsigned owner in the signed checked-negation MIR is rejected,
and type checks reject mismatched owners, result types, unrelated paths and
pointer widths that differ across declared targets. Complete `just vv`
remains the release gate.

## Blocked-context proof history (UE-59)

An actual Factory refresh erased a complete-context proof for a caller whose
partial compiler view could not translate a transitive callee. The next full
campaign reproduced the identical proof in two redundant verifier runs.
The regression obtains SDK proofs for a three-function chain, omits the leaf
MIR and demonstrates that the still-listed, blocked caller has a different
partial-context digest. Before repair, that digest erased the caller's proof.

Only a translation currently eligible for verification can invalidate a cached
decision. The blocked caller remains unadmitted; all original decisions survive,
and restoring the complete view replays with zero verifier calls. The existing
selection-history regression also passes, and the source-dependency regression
confirms that changed eligible meanings still require actual SDK verification.
No verdict status or digest is rewritten. Complete `just vv` remains the release
gate.

## Mathematical integer ABI codecs (UE-56)

The previous frozen producer refuses all six compiler-derived i128 and isize
field-read, aggregate and source-call fixtures at their representation boundary.
Width-derived codecs now bridge native words and matching mathematical source
integers. Conversions split signed magnitudes into checked UInt64 limbs, with
ordered range obligations proved by the actual SDK. Source-call results bind
once before conversion so each range check stays independent of the call body.

All six native functions pass actual SDK proof trials. Twenty-four closed SDK
vectors agree with executable Rust reference cases at minimum, maximum, zero
and negative values. Fixed Int64 representations, unrelated field types and
mismatched widths remain refused. A planted Bool field in the source schema
rejects the corresponding native integer read. These are focused development
checks; complete `just vv` remains the release gate.

## Shared native hash-map reads (UE-60)

The compiler-derived regression initially fails at `HashMap::get`. Four table
rows now reuse the existing linear lookup, membership and length models on
the unordered logical carrier. Map patterns retain their ordered/unordered
flag. Primitive equality and the source's admitted derived comparisons are
shared; byte-identical source/native comparison declarations emit once while
the native proof context still binds the complete helper definitions.

Seven native functions pass actual SDK proof trials. Twenty-eight closed SDK
vectors agree with executable Rust cases for numeric, borrowed string and
derived keys, both entry orders, hit/miss and empty/nonempty maps. A source
comparison in the same fixture exercises shared-helper emission. Custom
equality, hasher observations, iteration, floats and mismatched map kinds
remain refused. Replacing successful lookup results with a different value
produces LLV7002 and the SDK diagnostic that the proposition is false. The
model's Rust vectors are regenerated from std.toml and executed directly.
These are focused development checks; complete `just vv` remains the release
gate.

## Native function values and option mapping (UE-61)

The compiler-derived regression first fails on the constant function item
`flip`. Immutable workspace function items now retain their admitted signatures
and dependency closure, with explicit lambda parameters for their application.
Standard Rc/Arc AsRef, Borrow and Deref adapters reuse the admitted content
representation only for exact owners, traits and targets. A single Option::map
table row applies these pure function values; mutable callbacks remain residue.

Five mapped native functions pass actual SDK proof trials. Fifteen closed SDK
vectors agree with executable Rust for None, Boolean negation and shared values
at zero and u32::MAX. Custom adapter owners, mutable callbacks, incompatible
targets, unrelated traits and shared Mutex contents remain refused. A planted
wrong borrow result produces LLV7002 and the diagnostic that the proposition
is false. Generated Rust table vectors execute the same Option mapping model.
The interrupted test run retained its bound native decisions for replay. These
are focused development checks; complete `just vv` remains the release gate.

## Structural delivery safeguards (UE-62)

The initial regression fails because no delivery preflight exists. The policy
now requires the complete registry, structural dependency families and an
authority bound to actual model bytes. The campaign calls this guard before
proof work. Positive and negative cases cover leaf plans, manual strategies,
partial scope, wrong authority, missing source bindings, unchanged failures,
UI work, changed reports and artifacts, baseline loss, and false completion.
Twenty positive/negative cases pass. A preserved report remains usable after
the original report path is replaced. An actual planted leaf plan is rejected
at the consumer entry point without changing native or SDK logs. Identical
verified work replays in 1.8 seconds without running either verifier. Manual
semantic-registry changes and reductions of the original source/registry are
also rejected.

The consumer entry point also binds actual source, compiler MIR, embeddings,
SDK, producer and verdict bytes. It preserves actual verified receipts and
checks their manifests before reuse. Workspace instructions persist the user's
structural delivery requirement without modifying immutable repository policy.
This guard enforces campaign discipline; it does not create missing Rust
semantics or change the all-functions finish line. Full `just vv` remains open.

## Compositional compiler control-flow regions (UE-63)

The compiler-derived regression initially refuses nested control flow because
function-sized loop continuations create cyclic helper calls. Natural regions
now derive from compiler edges and dominating headers. Inner regions return
explicit exit packets; their callers own continuations. Conservative MIR write
analysis keeps unchanged frame values in their caller and transports changed
live state. Region outcomes also preserve early returns and distinct exits.

The structural analysis uses one linear-size reachability set per loop header,
with an acyclic fast path, rather than a quadratic dominator matrix. Existing
non-nested lowering retains its emitted representation and proof identities.
No standard-operation model row changes.

Four nested, triple, early-return and multi-exit functions pass actual SDK
proofs. All 128 closed SDK results agree with an executable Rust oracle. An
overflowing fifth function remains refused, and live mutable-borrow transport
remains typed residue. Independent CFG cases reject irreducible entries, missing
blocks and an absent entry. A planted lost Boolean exit state is rejected by
LLV7002 because an oracle proposition is false. Exact match-path evidence is
retained in every recursive decrease obligation; no termination gate is weakened.
These are development checks; full `just vv`, translator certification and
consumer adoption remain separate acceptance requirements.

## UE-64: shared source model import

The model, scenario and regression were registered first. The red compiler
fixture stopped at `Option::unwrap_or_default`; the native path had no access to
the source translator's existing model. Native calls now bind compiler-owned
standard inherent paths and exact immutable source/native carrier types before
importing the source declarations and their dependencies. Recursive nominal
schemas are checked for mutable cells. Panic-default and divergent readings
cannot pass this interface. No executable row of `mir/std.toml` changed.

`native_calls_import_source_semantic_models_ue_64` passed in the pinned SDK:
four native proofs; eleven executable-Rust differential vectors across Option,
Result and Boolean receivers; four definitional equalities to the existing opaque
string model. UTF-8 runtime primitives do not reduce under kernel `decide`, so
those four equalities establish model transport rather than claiming a computed
Rust differential. An incorrect oracle result (255 changed to 254 in the vector
itself) was rejected with LLV7002 / `is false`. Partial unwraps, Unicode-divergent
readings, mutable inputs, incompatible carriers and custom owners are refused.
Two unit regressions check exact bindings and recursive schema closure.

The complete library suite detected missing codec/source entries in the tool's
source-code digest. Both are now included; the source inventory gate remains
intact. Evidence: `uorust-source-model-import-red.log`,
`uorust-source-model-import-final-checks.log`, and
`uorust-source-model-import-acceptance.log` in the delivery runtime. Focused
acceptance does not replace `just vv`, translator certification or consumer
adoption. No contract completion is claimed from this capability.

### UE-62: SDK proof dependency closure reuse

The extended model/scenario and failing receipt-selection tests preceded the
implementation. All 31 guard cases pass. The guard selects an original real
verification only at equal actual SDK build/source/semantic/module identities,
with a hash-bound original SDK pin, intact report/artifacts and a cached proof
bundle. It never calculates SDK semantic identities or converts build success
into verification. The mandatory complete-campaign preflight remains intact.

The actual consumer SDK emitted unchanged identities after recompiling the
producer. Its original 12-module verification was reused with explicit
provenance; the real SDK project check and Factory refresh passed, with zero
native and kernel verifier calls. An actual SDK build of a planted false
assertion succeeded but changed all three content identities; it could not
select the earlier verification. Its proof was not admitted. The SDK acceptance
resume took 62.9 seconds; this excludes prior native projection and interrupted
setup and is not a fresh-campaign throughput result. The consumer remains at
634/2,704 and no additional lifts are claimed.

The earlier complete library suite passed 433 tests. The actual SDK refreshed
one fixture receipt whose declaration changed through source-model import;
all 16 CLI tests then replayed successfully, and conformance meta-tests and
strict all-target Clippy passed. The fixture verdict was copied from the actual
tool-written SDK result. No verdict, digest or axiom policy was fabricated.

### UE-64: compiler callable environments

The extended model/scenario and red compiler fixture preceded implementation.
A single reifier derives callback signatures and environment applications from
compiler bodies for the existing source and native standard model interfaces.
It binds the environment once and declares that exact SDK lambda capture;
mutable captures, memory signatures and incompatible carriers stay refused.
No standard-operation formula or executable registry row was added.

The actual SDK proved four fixture functions (captured predicates, mapped
values, lazy defaults and a function item), and all 21 executable-Rust vectors
passed. Rust's overflow counterexample panics and the SDK refuses its callback;
the verdict register contains four proved and one refused function. A planted
wrong closed callback result is rejected with LLV7002 / `is false`. The SDK also
rejected the earlier missing capture metadata with LLT4001 before recording any
function verdict, demonstrating that malformed model transport is fail-closed.
Evidence: `uorust-model-callables-red.log`,
`uorust-model-callables-final-sdk.log`, and
`uorust-model-callables-acceptance.log`. Library, CLI, conformance, strict Clippy,
model/template and lock checks passed. Consumer lifting gain remains subject to
the complete guarded campaign. Full release acceptance remains open.

## UE-65: shared source type readings

The model, scenario and failing compiler test were registered first. Native
lowering now tries its original ABI reading, then imports a compatible closed
source reading. The fallback accepts admitted SDK identities and their
containers; ABI-sensitive integers/characters, callables, type variables and
unregistered nominal schemas stay out. Shared-cell allocation references retain
identity rather than exposing their contents. No executable std registry row or
new standard-operation semantics was added.

The actual SDK proved four fixture functions and twelve transport/container
properties. Rust checks the same properties while a RefCell's mutable borrow is
live, demonstrating that transport never reads a snapshot of its contents.
Identity properties use reflexivity: the opaque SDK reference types deliberately
provide no decidable equality. Shared contents, writes, locks, counts and pointer
comparisons remain typed refusals. The SDK rejects a planted wrong container
observation with LLV7002 / `is false`. The primitive ABI/state-boundary unit test
passes; the older readonly-owner test now distinguishes opaque allocation
identity from the still-refused plain RefCell contents.

Evidence: `uorust-type-import-red.log`, `uorust-type-import-final-sdk.log`,
`uorust-type-import-acceptance.log`, and `uorust-type-import-final-checks.log`.
The complete library pass checked 433 unaffected tests, and its one changed
expectation passed on replay (434 total); all 16 CLI tests, conformance tests,
strict Clippy and model/template/lock gates passed. Consumer gain is measured
only by the complete guarded campaign; release acceptance remains open.

## UE-66: compiler composition with source generic dependencies

The compiler path now imports complete source-lifted generic definitions through
`lift::generic_call`, the source binder itself. Type arguments and coherent trait
methods retain their existing signatures and dependency names. It does not add
standard-operation models or supply an implementation for an unknown method.
Native words may pass parametrically without acquiring source integer semantics.
Source definitions with recorded divergences are excluded from this interface.

The initial regression failed at `generic_dependencies::word` with
`mir::TerminatorKind::Call: transport`. The restored regression has five actual
native SDK proofs and 32 computed-Rust comparison vectors covering unsigned and
signed transport, option payloads, coherent dictionaries and captured callbacks.
The SDK rejects a planted expected result of 254 for Rust's actual 255 with
LLV7002 (`is false`). The final focused SDK run passed in 78.72 seconds.

A separate planted schema defect admitted a dictionary call on a struct holding
`Cell<bool>`. Its negative assertion failed (`shared`) before the shared schema
check was applied. The final run keeps it unlifted alongside unavailable method
implementations, incompatible signed return carriers, partial panic conventions
and mutable references. The common binder's unit test also refuses two different
word widths bound to one type parameter, a missing coherent dictionary entry and
a method with a different result type; it retains the accepted method dependency.

435 library tests, all 16 CLI tests, conformance meta-tests, model/template/lock
checks and strict all-target Clippy passed during focused acceptance. These are
review evidence, not a claim that the complete `just vv` release gate passed.
Unused trait implementation entries are kept out of the global native proof
context: the chosen method and its source meaning are already bound by the
translated term and dependency identities.

## UE-67: parametric compiler bodies and product state

Native scopes now import the function's declared type parameters, including
parameters whose bounds its body does not use. Reading that parameter does not
supply a method implementation. Parametric trait dispatch without an admitted
dictionary and reachable generic drop glue stay typed residue.

Generic product values use the SDK's built-in, right-nested products. The common
emitter binds generated definitions, theorems and region-exit types by the type
variables they and their dependencies use. It supplies explicit function and
constructor type arguments, preserves literals, and retains generic binders when
applying the SDK's learned axiom policy. Type-parameter names share the source
binder's collision rule. This also admits generic source product returns through
the existing source translator.

The initial compiler regression failed on `mir::Ty: T`. Four compiler-generated
generic bodies now have actual SDK proved verdicts: transport, product return,
nested natural loops with an exit-state type, and transport with an unused Clone
bound. Forty computed-Rust comparison vectors cover the first three and generic
source products at Boolean and unsigned-word instantiations. A planted expected
255-to-254 result is rejected by LLV7002 (`is false`). The final focused SDK run
passed in 125.90 seconds.

A trait method deliberately named `clone` exposed an inadmissible fallback to a
same-named value adapter: the `mimic` negative failed before the general
parametric-dispatch guard. The restored run keeps both custom and standard
unresolved trait calls unlifted, and confirms that implicit generic drop glue is
reported as a drop-effect gap. No standard-operation registry row was added.
The binder unit additionally checks that quoted type-variable/function names do
not add generic dependencies to literal or unrelated declarations.

436 library tests and all 16 CLI tests passed, with conformance meta-tests,
strict all-target Clippy, model, template, SDK lock and generated self-model
checks. This remains focused review evidence; complete release acceptance and
translator certification are separate open gates.

## UE-68: rooted native model identities and original decision retention

A native proof previously bound every source definition and type scope. Adding
an unrelated source definition changed an unchanged candidate's digest. The
initial regression failed with two different digests for that one model.

The additional `model_context_sha256` observation now binds the existing emitted
proof-declaration closure, reached native/type/schema/signature readings, and the
pinned SDK/assets. It is an internal producer binding, not an SDK semantic ID.
Legacy records still require their original exact digest before the prover may
learn this optional observation. Learning it retains the original digest,
proved/refused status, diagnostic and axiom evidence. Later equal rooted models
reuse that original record; changed reached meanings still require the SDK.

The actual SDK regression established one proved and one refused decision, then
removed only the optional observation to reproduce a legacy register. Legacy
metadata learning and an unrelated source change each took zero verifier calls;
all original decision fields stayed intact. Changing a reached helper triggered
a new actual SDK proof. A reached schema change altered the model identity, and
a planted wrong rooted binding prevented admission even at the original legacy
digest. Malformed and non-string observations are also refused. The regression
passed in 116.31 seconds.

The CLI fixture now checks the same transition: only optional observations may
appear during migration, original evidence remains byte-identical after removing
those lines, and the following replay is byte-identical with zero verifier calls.
437 library tests, all 16 CLI tests, conformance meta-tests, strict all-target
Clippy, model/template/SDK-lock and generated self-model checks passed. No cached
proof status, diagnostic or legacy digest was fabricated or manually reassigned.
Complete `just vv` and translator certification remain separate open gates.

## UE-69: native product views preserve unavailable fields

The initial real-compiler regression failed at `mir::Ty: Holder`: an unknown
service field prevented access to an independent Boolean field. The common
reader now retains complete declared field order in SDK products. It composes
views along the existing source dependency graph and retains unavailable fields
as abstract values whose identities bind their source readings. Recursive edges
stay abstract, and incomplete shapes are never guessed. Source admission and
existing closed nominal schemas retain their prior representation.

Five actual native SDK proofs passed, including a nested field access, a call
between compiler bodies, and an exclusive update preserving its opaque payload.
48 executable-Rust comparisons passed; the service implementation panics if
called, so these field operations cannot obtain a result by invoking it. A
planted incorrect Boolean result was rejected by the actual SDK. The first
proof and differential run took 161.79 seconds. Unavailable service operations,
parametric drop glue, unresolved custom trait dispatch and incomplete generic
structs stay residue. A separate custom inherent-call check confirms a blocked
dependency cannot acquire the meaning of a same-named standard adapter.

A unit regression checks nested dependency composition, retained field count
and order, source-bound opaque identities, mutable-reference detection, cyclic
edge abstraction and incomplete-shape refusal. The existing code-identity gate
rejected the new module until it was included in the bound production sources.
The delivery preflight and frozen standard semantic registry remain intact.

Final focused checks passed: 438 library tests, all 16 CLI tests, conformance
meta-tests, both struct-view engine regressions, strict all-target Clippy,
model/template/SDK-lock and generated self-model checks. The CLI register was
refreshed from actual tool-written SDK decisions, retaining ten proved and one
refused status and recording rooted observations for all eleven decisions. The
replay now takes zero verifier calls and keeps existing rooted evidence exactly;
legacy migration remains covered independently by UE-68. Full `just vv`,
translator certification and consumer adoption remain open release boundaries.

## UE-70: typed storage boundaries for native mutations

The full product-view campaign stopped at an SDK link-type mismatch: a field
update replaced an abstract stored value with a concrete value. The prior
636-function consumer checkpoint remains the latest accepted projection; the
failed run claims no additional lift. Its log and SDK result are retained.

A minimal MIR regression reproduced the cause: place-update recursion discarded
the incoming value type and accepted overwriting an opaque slot with a Boolean.
The shared recursion now retains that type through nested field, element and
option updates. At the storage boundary it reuses the existing representation
codec, including its actual range obligations, or returns a typed residue before
SDK scheduling. No standard-operation semantics or SDK behavior was added.

The negative regression now passes. Two actual native SDK proofs and 24
executable-Rust comparisons passed for direct and nested signed 128-bit stores,
including minimum, maximum and large positive values. Comparisons inspect the
stored mathematical integer, rather than only a returned word. A planted wrong
stored value was rejected by the SDK. The proof and comparison run passed in
124.67 seconds. Library, CLI and dependency checks are recorded with this
checkpoint; complete release acceptance and translator certification remain open.

Final focused dependency checks passed: all 439 library tests, all 16 CLI tests,
conformance meta-tests, both product-view regressions, strict all-target Clippy,
model/template/SDK-lock and generated self-model checks. The product-view
regressions replayed with retained native decisions and passed in 47.35 seconds.

## UE-71: parametric map carriers and explicit ABI transport

The real SDK regression first failed at an unbound `K`: generic map entries
created global tuple schemas without type binders. Entry rendering, dependencies
and the existing source/standard semantic bindings now share SDK products when
parameters are present. Closed entries retain their original tuple ABI.

A second actual SDK check rejected a concrete caller passing that closed tuple
ABI to a generic product ABI. The common generic-call binder now transports
admitted list, product, option and result carriers through explicit structural
helpers in both directions. Opaque type arguments retain their own carrier;
unsupported callable conversions remain typed residue. Compiler bindings import
those same helper declarations. The transport changes no key, value or ordering
and supplies no comparison implementation or operation semantics.

Eight actual native SDK decisions and 39 executable-Rust comparisons passed
across Boolean and natural-valued maps, source and compiler reads, ordered and
unordered maps, concrete calls of generic functions and both directions of
transport. Unavailable generic key comparison and generic drop glue stay residue.
The first comparison used equality of a nominal tuple list, for which the SDK
provides no decidable-equality instance. Closed outputs are instead checked by
matching both entries, their primitive fields and the empty tail. No instance,
axiom or model ABI was added to make the test pass. A planted incorrect length
is rejected by the actual SDK. The final comparison run passed in 48.14 seconds.

The unit regression checks the exact retained closed rendering/tag, absence of
unbound nominal generic entries, both transport directions and their explicit
pure declarations. All 440 library tests, 16 CLI tests, conformance meta-tests,
strict all-target Clippy, model/template/SDK-lock and generated self-model checks
passed. Full release acceptance, translator certification and adoption remain
open; these are supporting checks rather than a completed consumer contract.

## UE-72: shared admitted source constant dependencies

The registered regression first failed on `left::VALUE`: its source definition
was already admitted, but native constant operands had no binding to that
meaning. The compiler context now distinguishes actual constant declarations
from function items and imports exact, nondivergent source definitions through
their target and module paths. Ambiguous printed identities stay unbound, even
when only one competing declaration has an admitted model. The existing source
and native type readers and immutable-value interface must agree; incompatible
signed carriers and unavailable initializers remain typed operand residue.
No second initializer evaluator or standard-operation semantics were added.

Three actual native SDK decisions and twelve executable Rust comparisons cover
Boolean values, unsigned values and equal constant names in distinct modules.
Changing one initializer rejects its previous native decision while retaining
the unrelated module's rooted decision. A planted incorrect result is refused
by the actual SDK with LLV7002. The initial comparison notation omitted the
outer equality parentheses; the SDK rejected that malformed test, and the
restored fixture passed in 55.13 seconds using intact native proof receipts.
This focused evidence does not establish additional consumer lifts, full
release acceptance, translator certification or final adoption.

The focused dependency checks passed: 440 library tests, all 16 CLI tests,
conformance meta-tests, strict all-target Clippy, model/template/SDK-lock and
generated self-model checks. The manual standard semantic registry is unchanged.

## UE-73: shared fixed-width source/native conversion kernels

The registered regression first stopped on an i8 field: its source model stored
Int64 while the compiler model required the width-bound bit pattern. All four
fixed-width signed source types now normalize through the SDK's existing to_int
conversion and the existing mathematical integer/word codec. The same binding
serves native fields and results of source function calls. Mismatched widths and
reverse conversions without an admitted codec remain typed residue.

The first implementation repeated the normalized expression in each range
obligation. Four small read models produced 32 theorems and 1,862,239 bytes of
generated Lean; compilation was interrupted after observing 191 seconds. This
attempt is not recorded as a passed or failed proof. Content-bound mathematical
kernels now carry the universal conversion obligations once per width, while
native sites call those kernels. The equivalent four-read native proof module
generated 371,159 Lean bytes, about 80 percent less. These are generated-size
measurements, not a controlled whole-run speedup claim.

Eight native decisions and 80 executable Rust comparisons passed across direct
field reads and source calls at i8/i16/i32/i64 extrema, negative values, zero and
positive values. The shared helper policies are learned from actual SDK reports,
including Classical.choice on the modular arithmetic proofs; no unsupported
axiom or proof status is fabricated. The SDK rejected a planted wrong result
with LLV7002. Changing the declared source width leaves the old MIR read refused.
The completed corpus took 137.33 seconds with intact native decisions reused.

The existing aggregate regression now admits array, option and tuple reads,
while incompatible reverse stores remain refused. The signed-slot regression
adds its formerly refused fixed-width read to the seven actual native decisions
and retains its existing wide/pointer-width comparisons. Both regressions,
440 library tests, all 16 CLI tests, conformance meta-tests, strict all-target
Clippy, model/template/SDK-lock and generated self-model checks passed.
This is a review checkpoint; full release acceptance, translator certification,
consumer adoption and complete consumer coverage remain open.

## UE-74: intact native receipts across dependency contexts

Measured full-campaign and Factory registers held 34 common decisions. Eight
differed only in their rooted context observation; their original digest,
decision, cause and observed axiom fields were unchanged. Replacing the active
context discarded a previously verified observation and caused verification
to repeat when switching views.

The actual SDK regression reproduced that loss using identical Rust code in two
explicitly bound resource-provenance contexts. It now retains complete prior
records when a current decision is displaced. Admission and policy selection
search both stores using the unchanged exact-digest/rooted-context rule. No
context is synthesized, no proof status is altered, and a wrong rooted binding
does not fall back to an equal legacy digest.

The regression passed in 108.97 seconds: both contexts were actually decided,
four subsequent alternations performed zero verifier runs and preserved all
register bytes, and imports of the original snapshots gave two further zero-run
replays. A corrupted historical context was refused; changing a reached source
definition required a new SDK decision. Parser/import tests reject malformed
contexts, records and axiom policies, and missing or malformed imports leave
the original target intact. Empty-history registers retain their existing
serialization, covered by the 16 CLI tests and committed byte comparisons.

The `prove --prior-verdicts` option imports verifier-written registers without
replacing current observations; imports are parsed before an atomic target
replacement. The delivery driver binds each imported file's path and digest
before invoking this interface. Compiler fixture setup uses Cargo's offline
lock writer when a fixture has no lock; production locked-input checks remain
intact. Three focused verdict tests, CLI and conformance checks, strict Clippy,
model/template/SDK-lock checks passed. The normal self-model writer refreshed
the CLI source anchors and release binding, and its check passed. Full release
acceptance, translator certification and consumer adoption remain open.

## UE-75: balanced signed source carrier kernels

The registered failing constructor scenario refused a native word stored in an
SDK signed source field. A single width-driven boundary now composes the pinned
SDK's Int64 bit operations into shared, content-bound decoding definitions. The
same conversion serves construction, source-call arguments and mutable field
updates at i8, i16, i32 and i64. Balanced expressions preserve the SDK's semantic
nesting limits; no SDK limit, compiler semantics or operation registry changed.

The SDK regression passed 12 native decisions and 120 executable Rust boundary
comparisons in 145.52 seconds, including returned results and stored state.
The SDK rejected a planted wrong result with LLV7002. Replacing an authoritative
signed source field with bool leaves the original compiler input refused.
The prior aggregate regression now accepts the complete compatible boundary and
retains its incompatible-field negative test. The fixed-field regression passed
with the newly admitted constructors and its previous read/call comparisons.
Focused codec, all 16 CLI, conformance, strict Clippy, model/template/SDK-lock and
self-model checks passed. These fixture decisions do not establish additional
consumer coverage; the guarded consumer campaign measures that separately.
Full release acceptance, translator certification and consumer adoption are open.

## UE-76: hygienic structural carrier composition

The registered red fixture stopped at a nested signed collection field. The
common boundary now composes admitted scalar codecs through products, lists,
options, results and collection entries. Each helper preserves constructors,
list length and entry order by structural recursion. Its identity binds the
complete carrier types and the selected generated bindings, so an unrelated
namespace addition does not rename an unaffected helper. Integer obligations
remain in shared mathematical kernels rather than repeated element expressions.

The preceding consumer attempt exposed a distinct structural hygiene failure:
the SDK's LLT4001 refused a generated `raw` parameter colliding with a source
declaration, before recording any function verdict. Shared codec and structural
bindings now allocate against the existing declaration namespace. The SDK
regression includes declarations named after the generated bindings, so this
class of capture is exercised along with the complete transport.

All eight native fixture functions and 48 executable Rust comparisons passed
in 93.34 seconds, including empty collections, None, both Result constructors,
signed extrema, entry order, constructors, calls and mutable final state. The
SDK rejected a planted changed value with LLV7002. Changing the authoritative
element to bool or the collection identity to HashMap leaves the old MIR
refused. The comparison checks every element and constructor explicitly because
the SDK does not promise synthesized DecidableEq instances for generated records.

The coupled aggregate, signed-slot, fixed-field and reverse-carrier regressions
passed, including the existing i128/pointer-width cases. Focused codec tests,
all 16 CLI tests, conformance checks, strict Clippy and model/template/SDK-lock
checks passed. Generated self-model consistency is checked by the existing gate.
Consumer coverage is measured separately by the guarded production campaign;
these fixture results do not add to its lifted count. Full release acceptance,
translator certification and consumer adoption remain open.

## UE-77: explicit applications at every source signature arity

The expanded carrier campaign stopped with LLT4001: a parameterless source
callee was emitted as a bare local even though its declaration was present.
A neutral regression reproduced the same failure before implementation. The
common compiler call builder now emits an explicit SDK application for the
empty argument list, as it already did for nonempty argument lists. Dependency
registration and result/state representation checks remain in the same path.

The actual SDK regression passed four native functions and 16 executable Rust
comparisons for ordinary results, constructed fields and mutable final state,
including zero and maximum-u8 loop counts. The SDK rejected a planted wrong
result. Changing the source boolean definition made the three affected native
functions unavailable under the old decisions and required three new actual
SDK decisions; the independent numeric caller retained its original decision.
The complete regression passed in 134.35 seconds.

All 13 focused lowering tests, 16 CLI tests, conformance checks, strict Clippy,
model/template/SDK-lock and generated self-model checks passed. Completed native
decisions from the interrupted consumer run are retained for exact-input reuse.
Its failed question acquired no verdict and is rerun only after this structural
cause changes. Consumer coverage, full release acceptance, translator
certification and consumer adoption remain separately measured and open.

## UE-78: preserve compiler definition identities

The registered regression failed before implementation because multiple trait
and inherent implementations erased to the same owner/method path. MIR loading
now preserves the existing compiler/source embedding key alongside the ordinary
path index. Colliding methods bind through their registered implementation file,
line, column, module and method; the ordinary index remains unchanged for unique
paths. Duplicate compiler source identities are removed from both indexes.
Qualified trait fallback also requires a registered trait implementation, so an
inherent method cannot become a trait provider merely by sharing its name.

The SDK regression passed three native method decisions and nine executable
Rust comparisons across two `From` argument types and an inherent method. A
planted incorrect result was rejected with LLV7002. The complete regression ran
in 94.36 seconds. A separate loader control checks that repeated compiler source
identities become ambiguous. Consumer gains and full release acceptance are
measured separately; these fixtures alone do not complete consumer functions.

Focused dependency checks passed: the duplicate-identity loader control, all
13 lowering tests, all 16 CLI tests, repo-conformance, strict workspace/all-target
Clippy, generated model validation, immutable template/SDK-lock checks and the
generated self-model check. Complete `just vv` and UE-18 certification remain
the release boundary; no release completion is inferred from focused checks.

## UE-79: coherent compiler/source call identities

The registered regression failed before implementation on a compiler-qualified
trait call whose registered provider existed under source spelling. A shared
identity import now binds compiler references through the existing Rust name
resolver before value-carrier erasure. It preserves nominal ownership, trait
identity, closed arguments, array lengths, tuple shape and reference mutability.
Direct calls, function-valued operands and captured closure bodies share the
same dependency binding. Repeated or unresolved identities are refused rather
than resolved by a same-named inherent method.

The regression also exposed two distinct traits sharing one unqualified trait
reference in the source register. The common ingestion pass now qualifies only
those colliding references with their declared paths; existing unambiguous
function identities remain stable. This adds no operation-specific behavior or
standard-library semantic entry. Package visibility is computed once per
immutable resolver and reused across identity imports.
Compiler imports are indexed by their registered function question, and captured
closures inherit their original source scope. Generic parameters cannot acquire
the meaning of a concrete type with the same name in another scope.

The SDK regression passed seven native caller decisions and 21 executable Rust
comparisons across overloaded local and standard traits, an unrelated trait
also named `From`, and an inherent method. The SDK rejected a planted incorrect
result with LLV7002. The full regression took 216.19 seconds. Separate controls
preserve array lengths and reference mutability, distinguish arrays from vector
storage, and retain function-valued compiler references. Consumer progress,
complete `just vv`, UE-18 certification and adoption remain separately measured.

The final scoped SDK regression passed again in 52.18 seconds with the original
native decision register retained. All 20 name-resolution controls, nine source
ingestion controls, 13 lowering tests, the compiler-reference control, 16 CLI
tests, repo-conformance and strict workspace/all-target Clippy passed. Generated
model, immutable template/SDK-lock and self-model checks passed; the self-model
writer regenerated the moved source citation. Generic-parameter, nominal-alias,
value/type namespace and unknown-type controls are included in that boundary.

## UE-80: shared kernel policy ownership

The consumer's native campaign completed 12 successful decisions with no new
refusals, but full-project verification rejected shared integer codec theorem
policies with LLV7005. The original SDK observation already existed in the
native register. The projection selected the first auxiliary owner and dropped
a later owner's observation; an owner can also move to source lifting while
its kernel remains needed by native callers.

A registered unit regression reproduced the owner-order failure before the
fix. Shared auxiliary assembly now preserves current exact-input observations
for identical declaration text independently of emission order. Retained
observations also remain available as policy hints when their former owner no
longer emits a native body. Hints do not admit a function or changed model:
original digest/context matching and actual SDK exact-policy checks remain
mandatory. The kernel statements and proofs are not edited to satisfy a policy.

The controls cover both owner orders, retained hints, an explicit empty observed
policy overriding a hint, and refusal to transfer a current observation between
different declaration bodies. These controls use synthetic policy metadata;
the consumer's original SDK receipts supply actual verification evidence.

The ownership controls, all 16 CLI tests, repo-conformance, strict Clippy,
generated model, template/SDK-lock and self-model checks passed. The next
consumer checkpoint retains the completed native decisions and rechecks the
assembled project through the unchanged SDK acceptance boundary.

## UE-81: shared receiver and associated source imports

The registered regression failed before implementation because the source
importer required a first receiver argument before consulting existing source
semantics. The importer now checks the same compiler-owned standard owner,
argument ABI and immutable schema closure, then submits receiver and associated
invocations to the existing source binder. Associated invocations retain their
owner binding, and nullary imported definitions are emitted as explicit calls.
No standard-operation table, semantic implementation or SDK behavior is added.

The four focused source-import controls pass, including workspace lookalikes,
incompatible words and recursive mutable payloads. The actual SDK regression
passed 19 Rust comparisons in 100.00 seconds, including a standard associated
factory with an argument and a parameterless ordered-map factory. Its planted
incorrect result was rejected with LLV7002. The proof campaign learned the
actual SDK axiom observations for the new native decisions rather than changing
their statements or bypassing exact-policy verification. These fixture results
do not establish the consumer's lifted count; that requires the mandatory
whole-consumer campaign. Complete `just vv`, translator certification and
consumer adoption remain open.

The four source-import controls, all 16 CLI tests, repo-conformance, strict
workspace/all-target Clippy, generated model, immutable template/SDK lock and
self-model checks passed with one consistent test optimization override. The
production executable is built only after these checks and frozen immediately.

## UE-82: content-bound model type declarations

The registered regression reproduced a model dependency failure for an
algebraic payload: native model guards checked only local structs and enums.
They also rejected qualified allocation-reference types already declared in
the fixed Prelude. The shared lookup now recognizes actual local algebraic
declarations and type declarations in the unchanged, content-bound Prelude.
All native model-type boundaries use that lookup, including printed aliases.
No model operation, type-name whitelist or Prelude declaration was added.

The type index reuses the proof slicer's declaration boundaries. Terms,
constructor names and quoted declaration text do not become type authority;
missing names and unrelated namespaces remain refused. Duplicate and
unsupported declarations fail the index. Fourteen lowering controls and the
existing seven syntax/proof-slicing controls passed after implementation.
The consumer count remains the previous SDK-verified checkpoint until the
mandatory guarded campaign accepts this producer's projection.

The actual SDK regression passed 25 Rust comparisons in 103.58 seconds,
including iterator state over algebraic payloads and opaque shared allocation
identities. The planted incorrect result was rejected with LLV7002. All eight
syntax/proof-slicing tests, four source-import tests, 16 CLI tests,
repo-conformance, strict workspace/all-target Clippy, generated model,
immutable template/SDK-lock and self-model checks passed. Full `just vv`,
translator certification and consumer adoption remain separate open gates.

## UE-83: recursive call bindings

The registered mutable-call loop regression reproduced the consumer's SDK
LLT4001 in 2.48 seconds before implementation. The pinned SDK recursion planner
accepts definition parameters and enclosing constructor-match binders, while
native call lowering introduced an administrative let binder. The change keeps
the computed call result typed and evaluates it once inside a shared,
content-bound polymorphic constructor frame. Its identity theorem is proved by
reflexivity. Recursive path evidence includes the actual constructor match and
its binder; SDK/template recursion policy remains unchanged.

The actual SDK regression passed 29 Rust comparisons in 108.41 seconds,
including the formerly rejected mutable-call loop. The existing wrong-result
control was rejected with LLV7002. The prior consumer campaign remains a stopped
attempt, not an accepted increase; the last fully accepted count is 656/2,704.
The next producer must pass the mandatory guarded whole-consumer campaign.

Fourteen lowering tests, eight syntax/proof-slicing tests and four source-import
controls passed. The strict CLI replay tests detected a changed model whose
committed fixture still carried the previous binding form. The canonical
UORust writer imported the newly verified original fixture register and refreshed
the committed evidence with zero new proofs, zero refusals and zero verifier
runs. The original pre-change fixture is retained in delivery evidence. All 16
CLI tests then passed, including byte-identical rooted replay with zero verifier
runs. Repo-conformance, strict all-target Clippy, generated model, immutable
template/SDK lock and self-model checks passed. No replay assertion was weakened.

## UE-84: reuse projected packages for observed policy retries

The registered fresh-campaign regression proved nine native fixture functions
but failed its reuse requirement: four SDK attempts performed five whole-source
projections, including the initial dependency catalogue, and reused no package.
The baseline ended at that assertion before the later value comparisons.

The solver now retains the same exact dependency slice and candidate identities
when the only new information is an SDK axiom-policy observation. The shared
projection lexer edits only those observed policy values. Statements, proofs,
strings and other declaration bytes are preserved. Each retry still invokes
the pinned SDK. A semantic refusal or a different dependency trial requires a
fresh projection. The proof report measures actual projections and package
reuses separately from SDK attempts and formally lifted counts.

The actual SDK regression passed all 29 Rust comparisons and the wrong-result
LLV7002 control in 161.24 seconds. It requires a real package reuse. This duration
includes the later value and negative checks that the failing baseline did not
reach, so the two durations do not establish a speedup percentage. The policy
writer's controls cover unchanged statements, proofs and quoted text, explicit
empty policies, idempotence, missing declarations, type declarations, duplicate
fields and malformed identities. No SDK/template policy or native semantic
registry was changed.

All nine syntax/policy controls, 14 lowering tests, four source-import controls
and 16 CLI tests passed. A fresh public CLI qualification measured ten proofs,
one semantic refusal, five SDK attempts, four whole-source projections within
the native proof stage, and two package reuses. This deliberately exercises
reprojection after a semantic decision as well as policy-only reuse. Final CLI
projection and Factory stages are outside those native proof counters. Exact
rooted replay still used zero verifier calls. Repo-conformance caught a moved
test-name suffix; retaining the original capability's name restored the complete
claim mapping. Repo-conformance, strict all-target Clippy, generated model,
immutable template/SDK-lock and self-model checks then passed. No gate was
weakened and no elapsed-time speedup is inferred from unequal fixture stages.


## UE-85: compiler-generated dependency body import

The registered loader regression initially found zero definitions for an actual
compiler-derived implementation whose source origin is a derive attribute. It
expected one. Compiler definitions are now retained before handwritten impl
header lookup. The compiler's graph output from the same invocation provides
fully qualified owner and trait identities, alongside the exact MIR bytes.

A companion catalogue binds these identities to the source, configuration,
pinned compiler and individual body hashes. Capture-only augmentation compares
every imported body with the saved MIR before writing the companion; saved MIR
is never replaced. Closed dependency interfaces use the existing Rust identity
resolver and ordinary body lowering in their own crate/target frame. Their
shared declarations enter the caller's actual SDK package. A catalogue or model
envelope alone is not evidence that a registered function lifted. Open generic
interfaces and ambiguous definitions cannot supply an imported method.

Focused controls cover changed source, configuration and body bytes, ambiguous
origins, unsafe paths, fully qualified module identities, and generic markers.
The SDK fixture compares generated clone, equality and enum-default behavior
with actual Rust results and plants an incorrect result for rejection. Complete
release acceptance remains the unchanged `just vv` boundary.

The actual SDK fixture passed all nine Rust comparisons and rejected the planted
wrong result with LLV7002 in 100.44 seconds. Capture-only replay reproduced the
companion byte-for-byte. All 84 MIR controls, 20 name-resolution controls and 16
CLI checks passed. Exact rooted CLI replay still required zero verifier calls;
the fresh qualification measured ten proofs, one semantic refusal, five SDK
attempts, four native-stage projections and two policy-package reuses. These
fixture counts do not claim additional consumer functions.

Repo-conformance and strict all-target Clippy passed. The unchanged model gate
required an explicit `Residue` collector type; the self-model gate detected
changed source spans, which the canonical writer regenerated. Model validation,
template and SDK-lock checks, and generated self-model checks then passed.

Whole-source input admission exposed a compiler graph detail: promoted constants
carry their parent method's graph label. The real pinned-rustc regression
initially collected three entries for a type deriving Debug and Default, where
only two are function definitions. Catalogue capture now uses the existing
parsed MIR source-definition classifier; collection and loading share the same
identity check. The three focused controls pass, including this compiler-backed
negative control. Loader hash/source/configuration checks remain strict, and
invalid body bindings name the offending MIR path.

## UE-86: source-bound compiler type-flow constraints

The registered regression initially rejected a borrowed source field as
`mir::Ty: dyn Service`. Its existing source view already supplied an abstract
carrier; independent local and return readers lost that identity.

The solver collects rigid source anchors on compiler value transport. Resolved
compiler types already identify the equality classes, so one pass supplies the
constraints; no repeated whole-body scan or new opaque sort is needed. Equal
frames share an interned environment, used by both native signature construction
and body lowering. Existing operation, mutable-state, drop and ambiguity guards
remain in force. Unknown operations and unanchored values supply no model.

The actual SDK regression passed 32 Rust comparisons, including borrowed field
returns, copies and mutable-container updates, and rejected a planted wrong
result with LLV7002 in 93.97 seconds. Controls retain conflicts, unrelated trait
identities, unanchored values, raw pointers and unread mutable borrows as open.
This is evidence for the shared mechanism; the consumer count remains its last
fully SDK-verified checkpoint until the guarded whole-registry run completes.

The changed-dependency qualification passed all 87 MIR controls, 20 name-resolution
controls, 16 CLI tests, repo-conformance, strict all-target Clippy, model validation,
immutable template/SDK-lock checks and generated self-model checks. Compiler
observations remain pinned to their original producer independently of the new
projector; unchanged consumer input capture is not repeated.

## UE-87: compiler-equality specialization of source carriers

The registered control failed before implementation with `conflicting
source-bound compiler carrier identities`. Whole-registry diagnostics showed
opaque source fields conflicting with already admitted closed compiler types.
The solver groups actual compiler readings, specializes existing opaque slots
through the shared type substitution kernel, and interns the complete coherent
frame for signature and body readers. It supplies no new operation or effect.
Unrelated identities, unanchored values and unsupported operations stay open.

The public compiler-path regression passed. The extended SDK regression passed
48 actual Rust/model comparisons, including selection between opaque fields,
and rejected a planted wrong result with LLV7002 in 93.20 seconds. All 89 MIR
controls, 16 CLI tests, repository conformance, strict all-target Clippy,
model/template/SDK-lock checks and canonical generated self-model checks passed.
The pre-production whole-registry analysis proposed eight new functions with no
baseline losses; these proposals are not SDK proof or completed delivery.
The production consumer result is recorded separately by its guarded campaign.

## UE-88–90: source semantic dependency kernels

The single-constructor compiler regression first failed with the typed
`ProjectionElem::Downcast` refusal. Native reads now reuse content-bound
projection definitions and constructor identity proofs derived from the actual
admitted algebraic declaration. A multi-constructor value without branch
provenance still fails the native gate. The SDK passed 60 Rust/model comparisons,
including multi-constructor controls, and rejected a planted wrong result with
LLV7002 (109.60 seconds).

The parametric source-import regression first failed to bind a native signed
payload to the existing source combinator model. The importer now abstracts only
transportable native carriers and instantiates the model through the existing
coherent generic-call binder. It adds no payload-operation semantics. The SDK
passed 12 Rust/model comparisons at signed boundaries and rejected a planted
wrong result (134.93 seconds). Unavailable defaults and mutable interfaces remain
refused; existing call-identity and recursive-schema controls pass.

The aggregate compiler regression first treated a record-shaped enum variant as
a struct. Constructor kind now resolves through the admitted nominal declaration,
and one declaration-slot map drives field validation and construction. It rejects
duplicate, missing, unknown and mixed fields. Qualified tuple structs in capitalized
modules retain their actual kind. The SDK passed 48 Rust/model comparisons and
rejected a planted wrong result (118.00 seconds), including named-field ordering,
unit variants and qualified structs.

Combined scoped checks passed 93 MIR controls, 16 CLI tests, repo-conformance,
strict all-target Clippy and model/template/SDK-lock/generated-self-model gates.
The entire-registry proposal analysis found nine possible additional functions
and no previous-frontier losses. Proposals are not SDK proofs. Complete `just vv`,
translator certification and the consumer release/adoption boundary remain open.

## UE-91: complete source/native declaration ordering

The compiler-backed callback regression reproduced SDK LLT4001 before the fix:
an auxiliary closure referenced a native provider emitted later in the same
module. Separate auxiliary and body partitions could not represent that edge.
Both partitions now use one complete provider-before-consumer ordering. The
existing declaration lexer supplies references, including function values;
quoted strings remain opaque and mutual groups stay indivisible. Invalid
serialized declarations retain their complete bytes for the SDK's normal
refusal rather than acquiring a second public failure interface.

The real SDK regression passed in 0.63 seconds after failing with the intended
link diagnostic. Function-value ordering, quoted content, mutual groups and all
nine lexer controls pass. Repository conformance, strict all-target Clippy,
model validation, immutable template/SDK-lock checks and canonical generated
self-model checks passed. Existing successful semantic receipts remain intact;
the interrupted consumer campaign requires a newly frozen producer and guarded
acceptance. No completed-consumer or release claim is made here.

## UE-92: existing source semantics through shared native/source ABI kernels

Before implementation, the compiler fixture's checked arithmetic calls remained
`mir::TerminatorKind::Call` residue. The shared importer now also tries an
immutable source representation, with both inputs and result converted by the
existing exact codec kernels. The operation itself still comes from the source
translator. Mutable interfaces, unknown schemas, wrong owners and incompatible
results remain refused; no operation-specific model was added.

The neutral SDK fixture passed 24 comparisons with actual Rust across signed
boundaries, overflow and negative values, and rejected a deliberately wrong
result with LLV7002 (134.74 seconds). Seven source import controls, 94 MIR tests,
16 CLI tests, repository conformance, strict Clippy, model/template/SDK lock and
generated self-model checks passed. Complete release acceptance remains the
unchanged `just vv` boundary.

## UE-93: compiler-owned primitive trait identities and operand ABI witnesses

The initial compiler fixture remained call residue for a qualified primitive
standard trait method. The shared source bridge now accepts closed primitive
owners with standard trait identities, while refusing workspace traits,
workspace owners, generic owners and composite owners whose payload supplies
an unbound implementation. No method-specific semantics were added.

Sixteen actual Rust/SDK boundary comparisons passed and an incorrect result
was rejected with LLV7002 (107.03 seconds). A second regression exposed lost
argument type witnesses for literals and projected fields. The call binder now
retains the compiler's literal suffix and projection annotation, following
dereferences and refusing unavailable projection types. That regression failed
before the metadata change and passed afterward. Eight source controls, 95 MIR
tests, conformance, strict Clippy and model/template/SDK/self-model checks passed.
Complete release acceptance remains the unchanged `just vv` boundary.

## UE-94: captured compilation context owns native MIR ABI bindings

The initial pointer-sized compiler call remained call residue. A native MIR
frame now uses the compilation target recorded beside its bodies and a unique
matching compiler data layout. Installed toolchain targets do not select that
ABI. Source-only portability analysis retains its declared-target view. Missing
or conflicting compiler evidence retains conservative behavior and supplies no
fixed-width source import. Capture records the active layout even when no
installation target list is present.

Existing fixed-width source semantics and shared codecs are reused through one
compositional carrier mapper. No standard operation model was added. The neutral
SDK regression passed 12 actual Rust comparisons, including `usize::MAX`, exact
boundary products and overflow, and rejected an incorrect result with LLV7002
(101.15 seconds). Controls cover missing, mismatched and duplicate layouts,
mutable interfaces and malformed compiler configuration. Ninety-seven MIR
tests, 16 CLI tests, conformance, strict Clippy and model/template/SDK/self-model
checks passed. Complete release acceptance remains `just vv`.

## UE-97: source-owned associated constants share the established initializer ABI

The initial native fixture refused an associated constant (0.63 seconds).
Closed inherent items now retain their source owner and module namespace and
reuse the existing source initializer. Interned owner frames bind both source
and native readings of Self; an immutable aggregate is imported only when both
interfaces agree. Generic, trait, aliased and unresolved owners remain outside
this binding. Existing schema, mutable-state and scalar-carrier guards remain.

Three actual Rust/SDK comparisons passed for scalar and aggregate constants,
including configuration selection and two owners sharing an item name. An
incorrect result was rejected with LLV7002 (99.47 seconds). Two focused controls
cover owner boundaries and ABI mismatches, including a native Self view that
differs from its source carrier. The related source-constant regression passed
12 comparisons and its incorrect-result control (46.02 seconds). After a
deliberate source edit it now first requires the compiler-capture binding
refusal, then recaptures before checking affected and unaffected decisions.
Conformance, strict Clippy and model/template/SDK/self-model checks passed.
Complete release acceptance remains the unchanged `just vv` boundary.

## UE-95: computed assertion outcomes retain tracked value-state

The initial native fixture was refused because valid inputs can overflow. The
shared lowering now preserves compiler assertions as computed SDK Result
branches, including the tracked mutable argument values on both exits. Exact
normal-model refusals select this distinct ABI; ordinary verified models keep
their identities. Cleanup, heap and unavailable call interfaces remain refused.

The neutral regression compares eight actual Rust executions against 24 SDK
assertions covering return values, panic-kind tags and state. An incorrect
result is rejected with LLV7002. A cleanup-control test demonstrates that
unwinding metadata cannot be erased to admit this model. No standard-operation
semantics or semantic-registry entry was added. The regression passed in
104.08 seconds; 98 MIR tests, conformance, strict Clippy and model/template/SDK/
self-model checks passed. Complete release acceptance remains the unchanged
`just vv` boundary.

## UE-96: native dependency closures preserve the outcome interface

The initial three-level native fixture failed because its callee's translation
was not admitted (73.47 seconds). Outcome selection now follows the existing
native dependency graph and updates both ordinary and compiler-qualified
callee bindings through the same interface conversion. Calls preserve the
callee's tracked mutable values before distinguishing return from panic. A
partial function cannot be exposed through a pure function-value interface.

Two neutral Rust/SDK regressions passed, each checking eight executions against
24 assertions for outcomes, panic tags and state, including a call inside a
loop. The second covers a compiler-qualified trait call and a callee without
mutable arguments. Runs took 244.70 and 216.74 seconds; deliberately incorrect
results were rejected with LLV7002. The function-value control also checks that
normal signature identities remain byte-compatible with recorded receipts and
the outcome identity remains distinct. Ninety-eight MIR checks and the added
interface control, conformance, strict Clippy and model/template/SDK/self-model
checks passed. Complete release acceptance remains `just vv`.

## UE-98: native dependency interfaces retain modeled memory

The initial native caller fixture was not admitted. Native signatures now bind
the actual lowered memory interface through existing dependency edges, including
qualified bindings. Computed assertion exits retain mutable state and the same
modeled memory, releasing frame-local allocations through the existing kernel.
Loops carry their memory and captured values in the established state packet so
the SDK retains the exact parameters in termination evidence. Unavailable
providers, cleanup and erased function-value effects remain refused.

Eight actual Rust executions passed 48 SDK assertions covering outcomes, panic
tags, mutable values, retained cells and frame release. An incorrect state
assertion was rejected with LLV7002. Exact-input native verdicts were reused on
the resumed acceptance stage; that stage passed in 43.37 seconds. Twenty focused
lowering controls, conformance, strict Clippy and model/template/SDK/self-model
checks passed. No semantic-registry operation was added. Complete release
acceptance remains the unchanged `just vv` boundary.

## UE-99: exact registry owners import existing source semantics

The initial typed adapter control rejected a lock-admitted registry product.
Native imports now reuse source models of exact checksum-admitted registry
owners. Compiler-resolved arithmetic language operators bind to source syntax;
the source translator owns the resulting meaning and continues to reject
unavailable semantics. Equal registry call interfaces share a content-bound
kernel. Unrelated traits, missing registry admission, mismatched arguments and
mutable interfaces cannot select it. No standard-operation model was added.

Ten focused source-import controls passed. Eighteen actual Rust/SDK comparisons
passed for signed and large integer values, including results beyond i64; an
incorrect product was rejected with LLV7002 (107.86 seconds). Conformance,
strict Clippy and model/template/SDK/self-model checks passed. Complete release
acceptance remains the unchanged `just vv` boundary.

## UE-100: compiler primitive namespaces bind existing kernels

The initial control refused rustc's qualified primitive owner. A shared owner
binding now recognizes exact compiler-owned primitive namespaces before local
name lookup. Native types, associated constants and unary methods reuse the
existing kernels. The same binding selects existing IEEE constant models; no
primitive value or standard-operation semantic model was added. Workspace
lookalikes, mismatched owner modules and local shadowing cannot select it.

Eight actual Rust/SDK comparisons passed for signed extrema, u128 bounds,
integer widths and IEEE infinity bit patterns. An incorrect bound was rejected
with LLV7002 (101.33 seconds). All 103 MIR checks passed, including explicit
namespace and shadowing controls and the existing target-width controls.
Conformance, strict Clippy and model/template/SDK/self-model checks passed.
Complete release acceptance remains the unchanged `just vv` boundary.

## UE-101: constructor function values share admitted aggregate kernels

The initial native fixture refused a tuple-variant function item (0.87 seconds).
A shared callable binding now reads the exact admitted variant schema and uses
the existing native aggregate lowering once per content-bound constructor
interface. Existing higher-order models consume that callable. Unavailable or
named-field variants, unbound type parameters, mutable schemas and source-only
scalar carriers cannot select it. No operation semantics were added.

Twelve actual Rust/SDK comparisons passed for workspace enum, Option and Result
constructors in higher-order calls. An incorrect result was rejected with
LLV7002 (103.20 seconds). Twenty-two lowering controls, including schema and
carrier negatives, conformance, strict Clippy and model/template/SDK/self-model
checks passed. Complete release acceptance remains the unchanged `just vv`
boundary.

## UE-102: native generic calls reuse admitted source dictionaries

The initial native fixture refused parametric dispatch (0.68 seconds). The
shared compiler/source identity resolver now binds calls to the existing
source-admitted dictionary entries. Native definitions receive those exact
callables through their established generic ABI; loop helpers transport them
in the same state representation as other live values. Method and trait
identity, arity and all carriers must agree. No trait semantics were added.

Eight actual Rust/SDK comparisons passed for two coherent implementations,
including wrapping boundaries. An incorrect result was rejected with LLV7002
(159.19 seconds). A separate admission control keeps missing, mutable and
uncaptured dictionary interfaces refused (0.32 seconds). Twenty-two lowering
controls, compiler-reference identity coverage, conformance, strict Clippy and
model/template/SDK/self-model checks passed. Complete release acceptance remains
the unchanged `just vv` boundary.

## UE-103: compiler-library schemas reuse source declarations

The initial native fixture refused `std::ops::Range<i32>` (0.67 seconds).
Closed, unconditional library records now use the existing core declaration
reader and instancer, with native field carriers and content-bound generic
names. The generated snapshot reproduces the exact checksum-pinned rust-src
archive and rejects missing, extra or modified Rust source. Operation behavior
continues to use the existing translators; no operation model was added.

Eight actual Rust/SDK comparisons passed for range construction and field
selection, including signed boundaries; an incorrect result was rejected with
LLV7002 (102.28 seconds). Source reproduction and schema controls passed
(9.31 seconds), covering conditional or duplicate declarations, ambiguous
reexports, unbound parameters, namespace and reserved-name collisions. The
snapshot uses derived serialization; its bytes are unchanged by the wire
representation. Twenty-two lowering controls, conformance, strict Clippy and
model/template/SDK/self-model checks passed. Complete release acceptance
remains the unchanged `just vv` boundary.

## UE-104: compiler-library closures share native ABI and effect kernels

The initial native fixture refused a compiler-library call (0.61 seconds).
The importer now consumes the complete pinned core MIR capture through the
existing declaration graph, resolver and span-aware dump reader. The graph
binds compiler facts, source archive, target layout, capture flags and payload.
Its 18,300 captured files reproduce a 33,522,757-byte normalized snapshot,
compressed losslessly to 1,937,054 bytes. A script reproduces the capture with
an explicit preserved source-acquisition producer and the pinned compiler.
No operation model was added.

Library calls reuse the existing native call, memory and outcome ABI kernels.
Semantic lookup is independent of the consumer ABI. Discovered library panic
outcomes propagate across native dependencies before proof work, avoiding an
initial totality proof. Unavailable backward-borrow and incompatible interfaces
remain refused. Source reexports do not identify a consumer type of the same
name; altered compiler/source bindings, flags and graph identities are rejected.

Eight actual Rust/SDK comparisons passed for primitive and record interfaces,
including signed boundaries. The incorrect result was rejected with LLV7002
(51.34 seconds), reusing an intact native verdict only through UORust's exact
identity matching. A two-body admission control selects the outcome ABI before
any verdict exists (8.86 seconds). Read-only source reproduction passed
(28.84 seconds), as did twenty-two lowering controls, closed-schema controls,
conformance, strict Clippy and model/template/SDK/self-model checks. Complete
release acceptance remains the unchanged `just vv` boundary.

## UE-105: compiler optimization preserves typed borrowed state

The initial fixture refused a library dependency (9.91 seconds); after generic
namespace resolution it reached the underlying scalar-cell limitation for a
borrowed record (10.38 seconds). The pinned compiler's complete optimized MIR
capture removes the reference-to-pointer implementation details before the
existing resolver and lowering consume the bodies. Source, compiler, target,
flags and the `runtime-optimized.after` pass remain bound. No operation model
or record-specific memory encoding was added.

The 23,467 captured files reproduce a 45,709,733-byte normalized graph,
compressed losslessly to 3,354,637 bytes. Source reproduction passed
(35.54 seconds). Eight Rust/SDK comparisons of borrowed record state passed,
with the incorrect result rejected by LLV7002 (120.62 seconds). The earlier
eight primitive/record library comparisons also passed (116.78 seconds).
Three authority/reexport controls now include optimization-pass and free
function namespace negatives (16.45 seconds). Twenty-two lowering controls,
pre-proof effect propagation, conformance, strict Clippy and
model/template/SDK/self-model checks passed. Complete release acceptance
remains the unchanged `just vv` boundary.

## UE-106: one dependency graph across declaration layers

The neutral source aggregate regression reproduced LLT4001 for a field type
emitted later by native lowering (1.24 seconds). Behavior emission now applies
the existing lexical dependency sorter after merging source and imported
native declarations. No schema or operation meaning changes. The regression
uses a pure aggregate constructor to isolate ordering from unobserved trial
axiom policies. SDK verification passed, and removing its imported field schema
still failed with LLT4001 (19.66 seconds together). Eighty-three project controls
passed, including mutual groups, lexical references, and byte-preserving
retention of cycles, unavailable references and conflicting declarations.
Complete release acceptance remains the unchanged `just vv` boundary.

The regenerated fixture changes declaration order only. Fixture reproduction,
repo-conformance, strict Clippy and model/template/SDK/self-model gates passed.

## UE-107: compiler arithmetic forms share modular intrinsic kernels

The neutral optimized-library wrapping body was refused by LLV7002 with an
unsigned overflow counterexample (53.38 seconds). Compiler-owned plain integer
MIR arithmetic now uses the existing fixed-width modular intrinsic kernels;
consumer interfaces, explicit overflow assertions and unchecked-operation
preconditions retain their admitted interpretation. No operation model or
compiler-stage alternative was added.

Six actual Rust/SDK boundary comparisons passed, including signed extrema and
wraparound, and the incorrect result was rejected with LLV7002 (112.32 seconds).
Twenty-three lowering controls preserve unsafe-operation preconditions,
refuse mixed target widths and reject incompatible carriers. Eight integer
kernel controls and the library effect-propagation regression (13.20 seconds)
passed, as did conformance, strict Clippy and model/template/SDK/self-model
checks. Complete release acceptance remains the unchanged `just vv` boundary.

## UE-108: compiler forms share one typed representation kernel

The neutral mixed-representation fixture initially refused the compiler
intrinsic identity (12.38 seconds). Qualified intrinsic calls now use the
existing source declaration normalizer, and MIR reinterpret casts and intrinsic
calls use one representation kernel. No standard-operation model was added.
Pointer-view constructions with unimported destination validity remain refused.
Scalar and UTF-8 conversions retain their validity obligations; UTF-8 validity
uses the SDK-supported decoder-result match instead of aggregate primitive
equality.

Seven Rust/SDK numeric observations passed for fixed-width arrays and pointer
addresses, with nine SDK pointer-field preservation checks. Four UTF-8 view
checks prove definitional equality to the existing SDK byte model, whose encoding
primitive is opaque to kernel reduction. Incorrect array output was rejected
with LLV7002. Invalid scalar and UTF-8 functions retain actual SDK refusals;
unadmitted pointer-view construction remains typed residue. The complete
focused SDK scenario passed in 44.92 seconds using intact, exact-match native
receipts. Test policy lookup uses the current frontier kappa, preserving the
separation between historical signatures and the current function.

Twenty-four lowering controls include direct MIR casts, scalar/UTF-8 validity,
pointer views, incompatible sizes and inconsistent target endianness. The
intrinsic ownership control rejects consumer lookalikes and undeclared items.
Conformance, strict Clippy and model/template/SDK/self-model checks passed.
Complete release acceptance remains the unchanged `just vv` boundary.

## UE-109: one immutable dependency index per native context model

The registered work-count regression exposed three full-model parses for two
root queries. Rooted native contexts now build the existing lexical dependency
graph once and reuse its byte spans and adjacency lists. The graph retains its
own immutable source, preserving string opacity, UTF-8 offsets, declaration
order and complete mutual groups. The single-root proof-slice API uses the same
implementation. No semantic model or context identity format changes.

The regression now performs one parse for both root queries. Ten lexical
controls and eighty-three project controls passed, covering escaped strings,
function-value references, type dependencies, mutual groups, invalid declarations,
missing/empty roots and isolation between different source models. Conformance,
strict Clippy and model/template/SDK/self-model checks passed. Full release
acceptance remains the unchanged `just vv` boundary.

## UE-110: native schemas before source-only carrier fallbacks

The registered regression initially failed because a source tuple reading
suppressed the pinned native record schema. Schema import now tests native ABI
support separately from the source fallback. Existing native carriers retain
priority; the existing strict compiler declaration instancer supplies complete
closed record fields and the established constructor/field kernels consume them.
No operation registry or SDK behavior changes.

The schema controls preserve native Option, Vec and String readings, word-width
bindings and declaration field order, while conditional, duplicate, ambiguous,
unbound and foreign schemas retain their prior refusals. A neutral unsigned
record fixture passed eight Rust/SDK boundary observations and rejected a changed
expected result with LLV7002. Its signed counterpart remains a separate control.
Full release acceptance remains the unchanged `just vv` boundary.

## UE-111: canonical and transitive compiler schema dependencies

The registered regression initially rejected the canonical form of a record
that its consumer path already imported. Instance keys now follow the existing
source namespace normalizer, while aliases retain the first admitted record
identity. Compiler bodies import missing closed record dependencies through the
same strict declaration instancer. Their new schemas travel with their consuming
proof through the existing type-declaration kernel. Caller scopes remain intact.

The canonical and transitive dependency checks cover imported body declarations,
foreign and open types, conditional and duplicate records, and ambiguous
reexports. Five native Rust observations of a reversed range's count agree with
the SDK model, including empty, descending and upper-bound inputs. A changed
expected result is rejected with LLV7002. The interrupted SDK check was resumed
without changing its models; its actual proved decision is retained for exact
reuse. Full release acceptance remains the unchanged `just vv` boundary.

The same UE-111 dependency boundary also retains each imported body's declaring
module. A nested-module record regression initially failed on its bare type
name; the local scope now resolves that owner through the existing source
normalizer. Substituted caller types retain their original identities. A scope
with no new bindings is borrowed, avoiding a full caller-scope copy. The controls
check nested-module declaration transport, caller immutability, protected
substitutions and the borrowed primitive scope. Prior actual neutral SDK
decisions are reused only through the existing exact-context matcher.

## UE-112: exact compiler linkage query reuse

The registered work-count regression initially performed four inference runs
for two repeated queries. The immutable, content-validated compiler graph now
memoizes each complete callee/argument-type/destination query, including typed
absence. Its separate cache has no authority in serialized graph bytes and is
not used for mutable or caller-owned links. A cached instance retains its exact
path, body and ordered parameter bindings; it carries no SDK proof status.

The regression now performs two inference runs. Changed callee, argument type,
missing argument type, destination and graph owner retain independent results.
A real compiler-graph control compares cached and uncached instances, checks
negative results, and confirms serialized authority bytes and validation remain
unchanged. Six compiler-graph controls, three schema controls, twenty-five
lowering controls, eighty-three project controls, repository conformance, strict
Clippy and model/template/SDK/self-model checks passed. Full release acceptance
remains the unchanged `just vv` boundary.

## UE-113: resolved declaration identities survive scoped rewriting

The registered regression initially classified a qualified parent reexport as
an unrelated external-glob type. Resolved declarations now use the existing
frozen type identity through subsequent registry rewriting. Receiver aliases
resolve that internal identity at the value-type boundary; ordinary foreign
aliases do not acquire transitive workspace readings. Alias definitions and
source field/signature overlays share the same declaration identity binder.

The controls retain real foreign and unresolved rows, parent reexports,
lifetime-erased receiver identities and generic shadowing. Sixty-four lifting
controls and eighty-four project controls passed with repository conformance,
strict Clippy and model/template/SDK/self-model checks. A Rust-compiled fixture
using the exactly locked serde dependency passed both enum field observations
through the SDK. Definitional equality checks the enum values without requiring
an unused decidable-equality instance; changing the expected constructor is
rejected with LLV7002. Full release acceptance remains `just vv`.

The native type importer shares the same single-step internal identity decoder.
A constant-ABI control first rejected an otherwise identical record represented
by a frozen declaration tag. The common alias boundary now preserves it while
retaining wrong aggregate, mutability, native-view and source-cell negatives.
Twenty-five native lowering controls passed alongside all sixty-four lifting
and eighty-four project controls, repository conformance and strict Clippy.

The existing source-owned constant scenario also passed its native Rust/SDK
observations and rejected a changed expected result with LLV7002 after this
identity change. The initial no-axiom trial correctly rejected observed axioms;
the existing explicitly recorded policy supplied the accepted proof boundary.

## UE-114: immutable catalogues across bounded proof batches

The registered two-root, one-root-budget regression initially proved both roots
but performed three whole-workspace projections. The same assertions now pass
with one projection. Each trial selects only requested undecided roots whose
native dependencies are already proved at the exact model context or occur in
the same batch. Observed axiom policies patch metadata, and the existing lexical
closure slicer retains the reached model. Every positive decision still comes
from the actual SDK. A semantic refusal discards the catalogue before any
changed panic-outcome ABI is retried.

Six prover controls and ten lexical controls passed, including stale-context,
blocked-dependency, unrelated-root, immutable-model and invalid-policy negatives.
The eight Rust/SDK observation assertions were resumed from their completed
proof stage after correcting their empty policy from actual SDK observations;
a wrong expected value was rejected with LLV7002. The complete observation test
is not represented as a fresh uninterrupted passing command. Native state and
panic observations passed. The refusal control also passed with two proofs and
one refusal, identical to the frozen pre-catalogue producer; its older count
expectation predated explicit panic outcomes. It now also requires exactly one
catalogue regeneration after the refusal (two total projections, versus three
in the frozen baseline). The unrelated false source assertion remains refused.
Repository conformance, strict Clippy and model/template/SDK/self-model checks
passed. Full release acceptance remains the unchanged `just vv` boundary.

## UE-115: retained compiler-owned library dependency closure

The native regression initially stopped at the standard vector default call,
which the preferred core graph does not own. The importer now consults the
retained alloc graph only after that preferred binding is absent. The graph is
reproduced from all 2,957 retained MIR bodies through the existing dump reader,
declaration resolver and native lowering. The original Factory record, complete
output digest and complete Rust source-tree digest are checked before graph
construction; source archive, toolchain, target and data layout remain bound.
The compressed graph is generated data, not an operation semantics table.

Imported bodies retain their library and declaring module. Only source-declared
inherent owners present in that capture qualify an unprefixed call for the
existing source model adapter. Consumer scopes and already preferred core
models retain their existing behavior. Captured instances have an independent
immutable query cache and library-qualified generated identities. Modified
capture records, graph bodies, target/pass metadata and foreign owners are
rejected. A native Rust observation through the imported dependency agrees with
the SDK, and a changed expected value is rejected with LLV7002. Link, lowering,
source-interface, repository conformance, strict Clippy and model/template/SDK/
self-model checks passed. Complete-registry proposal and actual SDK/Factory
acceptance remain separate from these controls. Full release acceptance remains
`just vv`.

## UE-116: payload state absent from literal empty results

The registered source-interface regression initially rejected a zero-input
empty container solely because its payload schema contains a mutable cell.
All input, ownership and carrier checks remain. A result exception is considered
only with no value arguments and only after the existing source translator
emits the exact typed literal empty-container definition. No payload value is
read, constructed or transported. Nonempty bodies, wrong result carriers and
kernels with inputs are rejected by the footprint check; mutable input calls
retain their prior rejection.

Twelve source-interface controls passed. A compiled Rust fixture with a Cell
payload agrees with the actual SDK model's empty result; a changed expected
length is rejected with LLV7002. Repository conformance, strict Clippy and
model/template/SDK/self-model checks passed. Whole-project native proof and
SDK/Factory acceptance remain separate. Full release acceptance remains `just vv`.

## UE-117: caller ABI transport across compiler namespaces

The registered native mutable exchange regression first failed with
`mir::Ty: string::String`: linkage had normalized a generic caller type without
retaining its admitted reading. The shared compiler interface now transports
exact query components through the existing declaration normalizer into an
isolated scope. The caller and immutable graph remain unchanged. An existing
inconsistent reading is a typed MIR refusal; unknown foreign types acquire no
reading. Composite and shadowed caller readings retain their exact native ABI.

A compiled Rust exchange of string vectors agrees with the actual SDK on both
the returned old vector and the changed mutable argument. A changed expected
result is rejected with LLV7002. Type-isolation controls, compiler schema and
capture checks, native lowering checks, repository conformance, strict Clippy,
and model/template/SDK/self-model checks passed. Whole-registry proposal and
SDK/Factory acceptance remain separate. Full release acceptance remains
`just vv`.

## UE-118: imported returned-borrow backward interfaces

The native imported-borrow regression initially failed because every captured
library result of `&mut T` was refused before backward lowering. Compatible
imports now lower the exact compiler body through both existing native kernels
in the same library and declaring-module scope. Their parameter, result and
state ABIs must agree exactly, including carriers that render alike. Both
content-bound definitions travel with the consuming proof and the existing
caller lens applies writes to the original place. Memory-backed and panic
borrowed interfaces retain their typed refusals.

A compiled Rust fixture writes one field through an imported returned borrow.
Six SDK observations agree with the changed and preserved tuple fields,
including unsigned boundaries; a changed expected state is rejected with
LLV7002. Memory, missing-state, width and same-rendered-carrier ABI negatives
passed. Native lowering, compiler schema/capture, immutable source-interface,
repository conformance, strict Clippy and model/template/SDK/self-model checks
passed. Complete-registry measurement and SDK/Factory acceptance are separate.
Full release acceptance remains `just vv`.

## UE-119: consumer dispatch through compiler-owned generic bodies

The registered generic-wrapper regression initially failed at its nested
consumer trait call. Imported bodies now retain a target-bound callback into
the existing source identity resolver and method index. Source function
signatures distinguish trait arguments omitted by MIR, including owned and
borrowed inputs with the same value carrier. Only traits declared by the
immutable compiler graph and owners reached by the actual caller query can
use this path. Query-local type spelling restoration preserves canonical body
identities. Both the selected source signature and native ABI must agree.
Compiler-generated definitions retain their own recorded target frame and do
not inherit an enclosing library's consumer callback.

Six actual Rust/SDK observations preserve payload and implementation choice
beside borrowed and foreign same-named implementations, including Unicode.
A deliberately wrong implementation result is rejected with LLV7002. Unknown
arguments, wrong results, incompatible explicit trait arguments, foreign
identities, conflicting caller spellings and ambiguous providers are refused.
Compiler identity/schema, native lowering, immutable source-interface,
repository conformance, strict Clippy and model/template/SDK/self-model checks
passed. Whole-registry and production SDK/Factory acceptance remain separate.
Full release acceptance remains `just vv`.

## UE-120: one authoritative source alias reading

The registered local-annotation regression fails before implementation: identical
locked dependency map and set aliases are admitted in signatures but rejected in
local annotations. Source scopes now share the existing exact archive/workspace
alias graph through an immutable Arc. Source type reading uses the existing
bounded substitution kernel with resolved names, including nested annotations.
Signatures use this same reader after name localization, preserving function
parameters that shadow imported aliases. Lexical declarations retain precedence.
The shared reader rejects remaining cyclic or incompatible-arity aliases rather
than restarting an expansion budget under a container.

The regression covers local map/set annotations, a nested workspace alias, a
same-named lexical declaration and a same-named generic parameter. Six actual
Rust/SDK observations exercise empty and populated containers. A deliberately
wrong expected result must fail SDK verification with LLV7002. Focused type-reader
controls cover generic and foreign bindings, incompatible arity and a recursive
alias. The existing archive-checksum negative remains required. Existing alias
scope and renamed-type SDK regressions preserve prior behavior. Whole-registry
impact and production acceptance are separate from these focused checks; full
release acceptance remains the unchanged `just vv` boundary.

## UE-121: complete compiler constant dependency capture

The registered native regression initially rejects the exact closed compiler
constant operands for floating-point and unit owners. The original Factory core
capture already contains the missing bodies. The existing lossless graph writer
now reproduces this complete capture as well as alloc, verifying every capture
file and the complete pinned source tree. No standard-operation model is added.
The source reader records default constant providers only for unconditional,
non-specializable, unbounded blanket implementations beside their trait in the
same source scope, without constant overrides. Conditional, bounded, specialized,
concrete and overridden implementations gain no provider identity.

Runtime queries require a graph-owned compiler trait, this source provider
identity and the exact captured target layout. The existing namespace resolver
and constant-body lowerer run dependencies in their owning graph. A constant
must retain a zero-parameter, memory-free ABI with its exact result. Foreign and
unqualified consumer traits are refused. Capture bytes, graph contents and
provider identities are bound by the retained capture and graph digest.

Qualification includes exact Factory graph reproduction, provider and target
negatives, altered capture/body/provider rejection, actual Rust/SDK constant
observations, and a wrong-result LLV7002 rejection. Whole-registry impact,
production acceptance and the unchanged full `just vv` release gate remain
separate requirements.

UE-122 exercises a compiler-generated MIR body which reads the discriminant of an
Option, replaces the input, and returns the earlier tag. The original translation
refused that read with `mir::Rvalue::Discriminant`. The pinned compiler's
executable returned 0 and 1 for opposite input variants. Four native proofs and
eight SDK observations passed for saved values, direct returns, branches and
field writes after sharing the checked conversion and saving the scrutinee. Substituting the replacement's tag for the original
failed the SDK observation with LLV7002.

UE-123 binds direct calls into path modules to their reachable workspace
declaration, generated source bytes, target-local compiler path and MIR body.
Two modules each supply a different implementation of the same pair of function
names. The original native projection refused the call into the generated
module. After importing both dependency closures, the pinned SDK proved both
callers and checked eight values against the compiled Rust executable. Reversing
an expected result failed with LLV7002. The source ownership test checks
unreachable files, external files, conditional declarations, generic functions
and changed source identities. The ordinary compiler ABI and recursion checks
also apply to these dependencies.

UE-124 records the primary lowering residue and both compiler body digests when
a paired runtime body supplies a translation. In the pass-pair fixture rustc
removes an unreachable external call. The primary body alone leaves the caller
open. With the compiler pair present, the pinned SDK proved the caller with
one accepted decision and zero refusals. Removing the pair restores the
original residue. The unit test also changes each digest input and supplies
an incompatible parameter type. None of these can provide a translation.
