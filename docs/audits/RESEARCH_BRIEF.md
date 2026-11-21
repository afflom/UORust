# UORust deep-audit research brief

Recon half of `/qor-deep-audit` over the hand-written codebase at `c4ed21bf`
(origin/main, 2026-11-21). Read-only: no code was changed. Generated code is out
of scope (`crates/certified/**`, `crates/uorust/src/certified.rs`,
`prism-factory/**`, `CONFORMANCE.md`, template-owned `bootstrap/render*`,
frame-owned `site/*`). This is a plain document: no Qor-logic ledger; the
repository is Qortara-governed and its seal stays NOT_ESTABLISHED.

Status column: `UNVERIFIED` until a verification round marks it `CONFIRMED`,
`REFUTED` or `PARTIAL`. Severity: CRITICAL / MAJOR / MINOR / OBS. Effort: S
(< 1 h), M (half day), L (days). "Blocks" = blocks a trustworthy release.

## Executive summary

Recon base `c4ed21bf`; origin/main has since moved to `28926f95` (PR #43), which
shifted some `lift.rs` line references by up to ~40 lines.

Five subsystem reconnaissances produced about 128 raw findings, consolidated
into 56 gaps; verification added 6 more. After two verification rounds there
are **62 gaps: 5 CRITICAL, 27 MAJOR, 23 MINOR, 7 OBS; 43 CONFIRMED, 19 PARTIAL,
0 REFUTED** (several sub-claims and many line numbers were corrected).

Top five:
1. **GAP-GOV-06 / GAP-GOV-01** Anyone with write access can publish an
   attested release from an unmerged commit (self-qualifying tag, no tag
   ruleset, no `release` environment); governance is declared, not enforced (no
   required checks, 0 approvals, no CODEOWNERS, always-bypass actors).
2. **GAP-SEC-01 / GAP-SEC-12** No working private vulnerability channel, and
   secret scanning, push protection and Dependabot alerts are all off.
3. **GAP-LIFT-01** `.collect()` into a set or map lifts as the unchanged list:
   8 lifted functions carry proved claims with wrong set semantics, 3 of them
   certified roots (`lift::captures` is wrong). No production behaviour depends
   on them.
4. **GAP-OPS-01/02/03, GAP-OPS-07, GAP-REL-01** Operations that report success
   while failing: bot pushes vs the PR ruleset (push.sh exits 0), core/std
   register workflows dead since a lost executable bit, `upstream.py` will drop
   the `[[gap]]` tables, the crate was never published to crates.io, and the
   Windows release has not built since v0.3.0.
5. **GAP-GATE-01..04** Acceptance gates that can silently stop refusing.

The single largest cause of blocked improvement is **GAP-TEST-***: most
behaviour is not pinned against base bytes, which is why a behaviour-preserving
refactor admitted only 6 of 31 candidates (PR #43, merged as `28926f95`).

## Constraints that bound every remediation

- **K-1 κ register.** `frontier.toml` keys every product function by
  κ = sha256(domain, crate path, tokens) (`crates/uorust/src/register.rs:55-77`);
  any token edit needs `just register`; renames/moves change κ.
- **K-2 UE-16 blind spot.** A lifted κ that disappears is not refused
  (`register.rs:537-563`); lifted-status loss must be reviewed in the diff.
- **K-3 UE-18 ratchet.** Uncertified `uorust::lift::` functions may only shrink
  (`crates/conformance/tests/ratchet.rs:104-108`, `certify.rs:3786-3810`): a new
  helper or an edit to a certified function (160 roots in
  `crates/certified/manifest.toml`) is refused until re-certified.
- **K-4 certified dispatch.** Seven functions run the certified package in
  production (`certified.rs:36-45`); editing their hand bodies has no effect
  until `just certified`.
- **K-5 template byte-binding.** `.devcontainer/devcontainer.json`,
  `bootstrap.yml`, `AGENTS.md`, `VERIFICATION.md`, `prismpm.lock`,
  `template-contract.json`, `template.lock` are pinned by `template.lock`;
  frame files by `frame.lock`. Fix those upstream.
- **K-6 matched literals.** xtask matches exact workflow strings
  (`xtask/src/bootstrap.rs:78-121, 145-149, 192-203, 400-422, 585-652`).
- **K-7 verdict digests.** MIR lowering text changes need Lean re-proof
  (`lift-verdicts.toml`).
- **K-8 base pins.** Only UE-01 (thermostat fixture), UE-17 digests,
  CONFORMANCE.md and unit/engine assertions pin base bytes.

## Gaps by category

### GOV — governance and enforcement

- **GAP-GOV-01** CRITICAL, M. GOVERNANCE.md:7 requires `just vv` and owner
  approval; live ruleset has no required checks, 0 approvals, admin/team bypass;
  no `.github/CODEOWNERS`; README.md:240 says "Enforced on PRs". CONFIRMED (R1).
- **GAP-GOV-02** MAJOR, M. LANES.md:51-57 and `scripts/hooks/lane-push.sh:24`
  push lanes straight to `main`, contradicting GOVERNANCE.md. CONFIRMED (R1).
- **GAP-GOV-03** MAJOR, M. Release jobs (release.yml:89,165,240,271) have no
  protected environment; no `release` environment exists; release is fully
  automatic, contradicting VERIFICATION.md:82. CONFIRMED (R1).
- **GAP-GOV-04** MAJOR, M. `ratchet`, `core-gate`, `certified-gate`, `content`
  are outside `just vv` (Justfile:11, 242-272); the ratchet runs only after
  merge (certified.yml:21-32), contradicting VERIFICATION.md:80. CONFIRMED (R1).
- **GAP-GOV-05** MINOR, S. Qortara checks required by AGENTS.md:72-73 run
  nowhere (no Justfile/CI step). CONFIRMED (R2).

### SEC — security and trust boundaries

- **GAP-SEC-01** CRITICAL, S. SECURITY.md:4 channel is the pre-transfer owner;
  private vulnerability reporting disabled; single triager (SECURITY.md:7).
  CONFIRMED (R1).
- **GAP-SEC-02** MINOR, S. `crates/uorust/src/lib.rs:208-215` joins manifest
  paths to `out` unchecked; manifest presence alone proves ownership; SHA
  ignored; deletes outside `out`. CONFIRMED (R1).
- **GAP-SEC-03** MAJOR, M. crates.yml builds top crates.io crates with the
  docker socket mounted; `scripts/mir/top-crates.py:28-33` unvalidated; merge
  job `merge-multiple` lets a shard overwrite another's registers; pushed to
  `main`. CONFIRMED (R1).
- **GAP-SEC-04** MAJOR, M. pages.yml job holding `id-token`/`attestations`
  runs unlocked `npm install --global` (pages.yml:37-38, 141-159). CONFIRMED (R1).
- **GAP-SEC-05** MINOR, M. GH_TOKEN (contents/actions write) in the env of steps
  running build code with the docker socket (`scripts/ci/sdk.sh:102,105`,
  generate.yml:95-112). PARTIAL (R1).
- **GAP-SEC-06** MAJOR, S. `uorust crate` / `certify agree --cases` run untrusted
  build scripts and code unsandboxed, without timeouts
  (`registry.rs:156-177, 486-500`); `scripts/self-model/facets.py:544` control
  text omits them. CONFIRMED (R1).
- **GAP-SEC-07** MINOR, S. `registry.rs:55-63, 238-240, 299` spec version path
  traversal reaches `remove_dir_all` outside the temp dir. CONFIRMED (R2).
- **GAP-SEC-08** MINOR, M. `remove_dir_all` of operator-given dirs without an
  ownership check (certify.rs:349, 475, 718, 1085, 2053, 2780; embed.rs:4207).
  CONFIRMED (R2).
- **GAP-SEC-09** OBS, S. Credentials on argv without `::add-mask::`;
  upstream.yml persists them in `.git/config` (`scripts/ci/push.sh:45-60`).
  PARTIAL (R2).
- **GAP-SEC-10** MINOR, M. Supply chain: SDK in a personal namespace
  (prismpm.lock), crates.io checksum same-origin and unrecorded, installer sums
  same-origin, attestation opt-in, `wget` without `--https-only`, Dependabot
  security updates and secret scanning disabled. CONFIRMED (R2).
- **GAP-SEC-11** MINOR, S. Artifact provenance: `scripts/mir/fetch-mir.sh:32-33`
  takes any branch/repo artifact; `scripts/ci/repin.py:285-290` picks `prs[0]`
  without author filter. PARTIAL (R2).

### GATE — acceptance gate correctness

- **GAP-GATE-01** MAJOR, S. `crates/conformance/src/meta.rs:90-91`
  `ends_with(slug)` without `_` boundary. CONFIRMED (R1). **RESOLVED** (Sprint 2, C7):
  `names_id` (meta.rs:63) requires the `_` boundary; `names_id_needs_an_underscore_boundary`
  and the `aue_01` plant in `the_meta_gate_is_falsifiable_cm_02` (bdd.rs).
- **GAP-GATE-02** MAJOR, M. `crates/conformance/tests/bdd.rs:29-71` counts
  `#[ignore]`, `#[cfg(any())]` and fixture tests (AGENTS.md:56-57). CONFIRMED (R1). **PARTLY RESOLVED** (Sprint 2,
  C7): `test_names_in` (bdd.rs:66) drops ignored, `cfg_attr`-ignored and
  conditionally compiled tests, tests in `cfg`-gated inline modules and files,
  and the walk skips `fixtures/`; tested by
  `test_names_in_drops_ignored_and_conditional_tests` and
  `fixture_tests_are_not_counted`. Still open: a test in a separate file gated
  by `#[cfg(x)] mod m;` (none exists today).
- **GAP-GATE-03** MAJOR, S. `xtask/src/bootstrap.rs:588-591` floating-action
  check misses `- uses:` lines. CONFIRMED (R1). **RESOLVED** (Sprint 2, C7):
  `uses_reference` and `floats` (bootstrap.rs:376-392) read both forms; a
  marker line with no `uses:` is no longer pinned;
  `floating_workflow_input_plants_are_rejected`.
- **GAP-GATE-04** MAJOR, S. `xtask/src/audit.rs:98-111` braceless
  `#[cfg(test)]` item exempts the next shipped item from R5. CONFIRMED (R1).
  **RESOLVED** (Sprint 2, C7): `effective_lines` (audit.rs:84) ends each test
  item at its terminator by kind (`kind_of`, audit.rs:206);
  `a_braceless_test_item_does_not_hide_the_next_item`.
- **GAP-GATE-05** MINOR, M. CM-01..03 claimed only by test names, not
  registered (meta.rs:130-135 names this a defect). PARTIAL (R1).
- **GAP-GATE-06** MAJOR, M. Documented R2 check (no test asserts an open claim)
  does not exist; ledger open claims never scanned; substring ID match
  (meta.rs:5-6, 169-202; lib.rs:13-18). CONFIRMED (R1).
- **GAP-GATE-07** MINOR, M. Bootstrap predicate holes: permissions prefix match,
  job-level permissions unchecked, `- if:` skips, 6-space step split, open
  devcontainer predicate (bootstrap.rs:160-189, 527-561). Mitigated by K-5.
  CONFIRMED (R2).
- **GAP-GATE-08** MINOR, S. R5 sanction by substring; three dead sanctioned
  names (`audit.rs:134-160`). CONFIRMED (R2).
- **GAP-GATE-09** MINOR, M. `check_ids` never checks ID format; scenario
  suite/statement/uniqueness unchecked; ledger duplicate IDs and feature paths
  unresolved (`crates/model/src/lib.rs:76-113`, registry.rs:52-63). PARTIAL (R2).
- **GAP-GATE-10** MINOR, S. Missing base manifest makes the certify ratchet pass
  (`certify.rs:2498-2502`, `certify/profile.rs:287-292`). CONFIRMED (R2).
  **RESOLVED** (Sprint 2, C8): `certify ratchet` refuses an absent `--manifest`
  or `--base-manifest` by name (main.rs, `Certify::Ratchet`), and
  `scripts/hooks/ratchet-staged.sh` empties its directory and passes an absent
  manifest through; `ratchet_refuses_an_absent_manifest` (tests/certify.rs).
- **GAP-GATE-11** MINOR, S. `xtask/src/proof.rs`: `SDK_IMAGE` defaults empty
  (fail-open, :237); `keep()` deletes before confirming (:336-342). CONFIRMED (R2).
  **RESOLVED** (Sprint 2, C8): `reuse_with` refuses an empty SDK image;
  `install` stages, swaps, confirms and restores the earlier proof on failure;
  `reuse_needs_an_sdk_image`, `keep_loses_nothing_when_a_copy_fails`,
  `keep_restores_the_old_proof_when_the_new_one_does_not_read_back`.

### TEST — behaviour or refusals with no pin

- **GAP-TEST-01** MAJOR, L. Gate refusals without negative tests: meta-gate
  (meta.rs:69-217 except "no test name"), 11 `ModelError::Inconsistent`
  refusals (lib.rs:93-153), check-model stale CONFORMANCE (xtask main.rs:90-97),
  `audit_deferral` (audit.rs:189-291), bootstrap `audit()` /
  `audit_policy_files` (bootstrap.rs:209-656). AGENTS.md:15 R5. PARTIAL (R1).
- **GAP-TEST-02** MAJOR, L. Translator AST paths unpinned: list combinators
  (lift.rs:6376-6952), Option/Result combinators (:5423-5643), text methods
  (:3945-4516), integer arms (:4520-4972), `cast_value` (:2819-2864),
  comparisons/format (:2971-3269), `updated` (:7973-8170), cross-candidate
  (:10602-10957). PARTIAL (R1).
- **GAP-TEST-03** MAJOR, L. MIR lowering unpinned beyond 11 digests: `binary_on`,
  `int_intrinsic`, memory/pointers (mir/lower.rs:4610-8490); `mir/link.rs`,
  `intrinsic.rs`, `ir.rs`, `spec.rs` have 0 tests. PARTIAL (R1).
- **GAP-TEST-04** MAJOR, M. No byte golden for `embed_project`
  (embed.rs:4092), bridge project/verify, `layouts.json`, certify manifest render
  (CRATE-22..27). PARTIAL (R1).
- **GAP-TEST-05** MINOR, M. Error Display texts and CLI messages unpinned
  (error.rs:69-122; main.rs:564-1053). PARTIAL (R2).
- **GAP-TEST-06** MAJOR, L. CI scripts that write `main` have no hermetic test:
  push.sh, push-branch.sh, merge-main.sh, generate-loop.sh, land-certified.sh,
  release writers, mir scripts, installers, hooks; several tests run only in CI,
  not `just vv`. CONFIRMED (R1). **PARTLY RESOLVED** (Sprint 1, C1): every
  Python and shell suite under `scripts/` now runs under `vv`
  (`crates/conformance/tests/ci.rs` `script_suites_pass`), inside the factory's
  `test` obligation (`prism-factory/graph.toml`); still open: the browser suites
  `try/test.mjs` and `identity/test.mjs` (CI-only), and no hermetic test yet for
  push.sh, push-branch.sh, merge-main.sh, generate-loop.sh, land-certified.sh,
  release writers, mir scripts, installers and hooks.
- **GAP-TEST-07** MINOR, S. UE-01's "nothing stale lingers" is a tautology; extra
  files in the fixture go unnoticed (`lib.rs:232-247`, engine.rs:165-167).
  CONFIRMED (R2).
- **GAP-TEST-08** OBS, S. `HAND` global flipped by a test while others run in
  parallel (`certified.rs:19-30, 386-392`). CONFIRMED (R2).

### LIFT — translator correctness (latent wrong claims)

- **GAP-LIFT-01** CRITICAL, L. `.collect()` into Set/Map returns the list unchanged;
  `Ty::same` compares rendered text (lift.rs:6419-6439, 235-237). K-3/K-1.
  CONFIRMED (R1).
- **GAP-LIFT-02** MAJOR, S. `str::is_ascii()` lifts to `textIsAscii`, which tests
  the first character only (lift.rs:4170-4173; `assets/project/src/Prelude.lex.tex:423-427`).
  CONFIRMED (R1).
- **GAP-LIFT-03** MAJOR, L. `isize << n` admitted without a range obligation
  (`mir/lower.rs:4873-4898`). K-7. CONFIRMED (R1).
- **GAP-LIFT-04** MAJOR, S. `unwrap_or_default` on compound Named types and
  `Char` (lift.rs:5290, 5625-5632). CONFIRMED (R2).
- **GAP-LIFT-05** MINOR, L. `pointer_width` ignores x32/ilp32 targets
  (lift.rs:2035-2087; certified, K-4). CONFIRMED (R2).
- **GAP-LIFT-06** MINOR, M. `->` miscounted by splitters: bridge.rs:310,
  layouts.rs:579, embed.rs:404-426 (`generic_parts`, 18 callers),
  certify.rs:3316-3354; fails closed (lost coverage). Fix re-stamps registers.
  PARTIAL (R2).
- **GAP-LIFT-07** OBS, S. Misleading residue texts (lift.rs:92-96, 3049,
  3103, 4219, 4326, 4849, 4874). PARTIAL (R2).
- **GAP-LIFT-08** OBS, M. Certified adapters without dispatch sites hidden by
  `#![allow(dead_code)]` (certified.rs:47-376). CONFIRMED (R2).

### OPS — CI health and operations

- **GAP-OPS-01** MAJOR, L. At least seven workflows push to `main` as the bot
  (generate.yml, core.yml, std.yml, crates.yml, certified.yml, release-bump.yml,
  translator-pin.yml); the PR-required ruleset (since 2026-11-20 19:46) likely
  rejects them; `scripts/ci/push.sh:53-55` exits 0 on rejection; release.yml
  formula push retries unbounded. PARTIAL (R1).
- **GAP-OPS-02** MAJOR, S. core.yml, std.yml, crates.yml failed every run since
  2026-11-19 (artifact `digest-mismatch`, `bitvec@1.1.1`). PARTIAL (R1).
- **GAP-OPS-03** MAJOR, S. `scripts/ci/upstream.py:106-112` drops the `[[gap]]`
  tables when it removes the last `[[patch]]` (upstream.toml:75-110).
  CONFIRMED (R1). **RESOLVED** (Sprint 1, C2): `scripts/ci/upstream_rows.py`
  `without_patch`, used by `upstream.py` `frame_pin` and `repin.py`
  `drop_rows`; `upstream.py main` reads `doc.get("patch", [])`; proved by
  `scripts/ci/test_upstream_rows.py` (12 cases on a committed snapshot).
- **GAP-OPS-04** MINOR, S. Swallowed failures: land-certified.sh:119-121 exit 0;
  generate-loop.sh:165-168 calls every failure WCAG; layouts `--check` status
  ignored (layouts.rs:1160-1168); harness clears differential silently
  (registry.rs:558-570); `compile_imports` ignores status (bridge.rs:1379-1395);
  `lexlean --version` unchecked (certify.rs:1186-1196). PARTIAL (R2).
- **GAP-OPS-05** MINOR, M. Spawns without timeouts (bridge, registry curl/cargo,
  cases, diagnose) and `pkill -f` regex (certify.rs:2156-2163). CONFIRMED (R2).
- **GAP-OPS-06** MINOR, S. Unreviewed hourly pins of external mutable branches to
  `main` (brand, website authority). CONFIRMED (R2).

### DOCS — documentation and architecture accuracy

- **GAP-DOCS-01** MAJOR, M. Architecture register empty (docs/README.md);
  the real model is `app/src/Architecture.lex.tex`, which omits MIR, lift,
  certified translator and gate crates. CONFIRMED (R1).
- **GAP-DOCS-02** MAJOR, M. Stale `afflom` identity in Cargo.toml:12-13,
  install.sh/ps1, Formula, flake.nix, install.yml, release.yml (Pages URLs do not
  redirect). PARTIAL (R1).
- **GAP-DOCS-03** MINOR, S. Doc/code mismatches: codegen.rs:25, 36-42; xtask
  main.rs:39-51, 69-70; features/suites/README.md; bdd.rs:169-171; CM-02 message
  whitespace meta.rs:163; try/index.html:93, 99; TEMPLATE-VERIFICATION.md lacks
  command/diagnostic/restore evidence. PARTIAL (R2).
- **GAP-DOCS-04** OBS, S. Proof reuse makes "vv is the complete gate"
  conditional (Justfile:56; bootstrap.yml:41-61). CONFIRMED (R2).

### DEPS — dependencies and toolchain

- **GAP-DEPS-01** MINOR, S. MSRV 1.89 claimed (Cargo.toml:9, clippy.toml:9,
  README) but never built. CONFIRMED (R2).
- **GAP-DEPS-02** OBS, S. External deps repeated per crate, not in
  `[workspace.dependencies]`. PARTIAL (R2).
- **GAP-DEPS-03** OBS, S. No flake.lock; Nix uses nixpkgs rustc; unused
  `prismpm.yml`; Unix-only crate (unconditional `std::os::unix::fs::symlink`,
  registry.rs:369-378). PARTIAL (R2).

### NEW — gaps found during verification (Round 2)

- **GAP-GOV-06** CRITICAL, M. Self-qualifying tag release: `bootstrap.yml`
  runs on any tag (:5-7) and `scripts/release/qualify_tag.py:28-32` accepts a
  passing bootstrap run on the tag itself; there is no tag ruleset and no
  `release` environment, so anyone with write access (or a stolen writer token)
  can tag an unmerged commit and `release.yml` publishes it with an attestation
  (:87-89, :165, :271) and pushes the Homebrew formula. CONFIRMED (R2).
- **GAP-GOV-07** MINOR, S. `can_approve_pull_request_reviews` is true
  (live `actions/permissions/workflow`); latent until GOV-01 requires
  approvals, then a self-approval route. Fix with GOV-01. CONFIRMED (R2).
- **GAP-SEC-12** MAJOR, S. Secret scanning, push protection, validity checks,
  Dependabot alerts and security updates are all disabled on a public repo
  (live `security_and_analysis`, `vulnerability-alerts` 404). CONFIRMED (R2).
- **GAP-REL-01** MAJOR, S. The crate is Unix-only (`std::os::unix::fs::symlink`
  without `cfg`, `crates/uorust/src/registry.rs:369,372,378,890`), so the
  Windows release leg has failed since v0.3.0 (`E0433 cannot find unix in os`)
  and is allowed to fail (release.yml:11); `install.ps1` (default
  `releases/latest/download`) is broken while release notes advertise it.
  v0.1.0-v0.2.0 shipped an unsigned Windows zip; whether the owner's
  code-signing policy covers this repository is an owner decision.
  CONFIRMED (R2).
- **GAP-OPS-07** CRITICAL, S. `CARGO_REGISTRY_TOKEN` exists in no repo,
  environment or visible org secret; the crates job skips by design but shows
  green (release.yml:222, 243-244); `uorust` has never been published
  (crates.io 404) while release notes advertise `cargo binstall uorust`.
  CONFIRMED (R2).
- **GAP-GATE-12** MAJOR, M. The UE-18 ratchet compares counts only
  (`certify.rs:3795`) and checks κ membership, not root definition hashes
  (`certify.rs:3755-3760`): a lost root can be masked by one gained elsewhere,
  and a certified root whose Lean definition changed without κ change goes
  unnoticed. CONFIRMED (R2). **PARTLY RESOLVED** (Sprint 2, C8): (a) a root the
  base certifies that no longer lifts at the head is refused before the count
  (`Profile::lost`, certify.rs);
  `a_root_that_no_longer_lifts_is_refused_even_when_the_count_shrinks`. Still
  open: (b) a definition change under an unchanged κ (policy question; the
  manifest diff shows it).

## Summary matrix

| ID | Cat | Sev | Effort | Blocks | Status |
|---|---|---|---|---|---|
| GAP-GOV-01 | GOV | CRITICAL | M | yes | CONFIRMED |
| GAP-GOV-06 | GOV | CRITICAL | M | yes | CONFIRMED |
| GAP-LIFT-01 | LIFT | CRITICAL | L | yes | CONFIRMED |
| GAP-SEC-01 | SEC | CRITICAL | S | yes | CONFIRMED |
| GAP-DOCS-01 | DOCS | MAJOR | M | no | CONFIRMED |
| GAP-DOCS-02 | DOCS | MAJOR | M | yes | PARTIAL |
| GAP-GATE-01 | GATE | MAJOR | S | yes | RESOLVED |
| GAP-GATE-02 | GATE | MAJOR | M | yes | PARTLY RESOLVED |
| GAP-GATE-03 | GATE | MAJOR | S | yes | RESOLVED |
| GAP-GATE-04 | GATE | MAJOR | S | yes | RESOLVED |
| GAP-GATE-06 | GATE | MAJOR | M | no | CONFIRMED |
| GAP-GOV-02 | GOV | MAJOR | M | no | CONFIRMED |
| GAP-GOV-03 | GOV | MAJOR | M | yes | CONFIRMED |
| GAP-GOV-04 | GOV | MAJOR | M | no | CONFIRMED |
| GAP-LIFT-02 | LIFT | MAJOR | S | no | CONFIRMED |
| GAP-LIFT-03 | LIFT | MAJOR | L | yes | CONFIRMED |
| GAP-LIFT-04 | LIFT | MAJOR | S | no | CONFIRMED |
| GAP-OPS-01 | OPS | MAJOR | L | yes | PARTIAL |
| GAP-OPS-02 | OPS | MAJOR | S | yes | PARTIAL |
| GAP-OPS-03 | OPS | MAJOR | S | yes | RESOLVED |
| GAP-OPS-07 | OPS | CRITICAL | S | yes | CONFIRMED |
| GAP-REL-01 | REL | MAJOR | S | yes | CONFIRMED |
| GAP-SEC-03 | SEC | MAJOR | M | yes | CONFIRMED |
| GAP-SEC-04 | SEC | MAJOR | M | yes | CONFIRMED |
| GAP-SEC-06 | SEC | MAJOR | S | no | CONFIRMED |
| GAP-SEC-12 | SEC | MAJOR | S | yes | CONFIRMED |
| GAP-TEST-01 | TEST | MAJOR | L | no | PARTIAL |
| GAP-TEST-02 | TEST | MAJOR | L | no | PARTIAL |
| GAP-TEST-03 | TEST | MAJOR | L | no | PARTIAL |
| GAP-TEST-04 | TEST | MAJOR | M | no | PARTIAL |
| GAP-TEST-06 | TEST | MAJOR | L | no | PARTLY RESOLVED |
| GAP-DEPS-01 | DEPS | MINOR | S | no | CONFIRMED |
| GAP-DOCS-03 | DOCS | MINOR | S | no | PARTIAL |
| GAP-GATE-05 | GATE | MINOR | M | no | PARTIAL |
| GAP-GATE-07 | GATE | MINOR | M | no | CONFIRMED |
| GAP-GATE-08 | GATE | MINOR | S | no | CONFIRMED |
| GAP-GATE-09 | GATE | MINOR | M | no | PARTIAL |
| GAP-GATE-10 | GATE | MINOR | S | no | RESOLVED |
| GAP-GATE-11 | GATE | MINOR | S | no | RESOLVED |
| GAP-GATE-12 | GATE | MAJOR | M | no | PARTLY RESOLVED |
| GAP-GOV-05 | GOV | MINOR | S | no | CONFIRMED |
| GAP-GOV-07 | GOV | MINOR | S | no | CONFIRMED |
| GAP-LIFT-05 | LIFT | MINOR | L | no | CONFIRMED |
| GAP-LIFT-06 | LIFT | MINOR | M | no | PARTIAL |
| GAP-OPS-04 | OPS | MINOR | S | no | PARTIAL |
| GAP-OPS-05 | OPS | MINOR | M | no | CONFIRMED |
| GAP-OPS-06 | OPS | MINOR | S | no | CONFIRMED |
| GAP-SEC-02 | SEC | MINOR | S | no | CONFIRMED |
| GAP-SEC-05 | SEC | MINOR | M | no | PARTIAL |
| GAP-SEC-07 | SEC | MINOR | S | no | CONFIRMED |
| GAP-SEC-08 | SEC | MINOR | M | no | CONFIRMED |
| GAP-SEC-10 | SEC | MINOR | M | no | CONFIRMED |
| GAP-SEC-11 | SEC | MINOR | S | no | PARTIAL |
| GAP-TEST-05 | TEST | MINOR | M | no | PARTIAL |
| GAP-TEST-07 | TEST | MINOR | S | no | CONFIRMED |
| GAP-DEPS-02 | DEPS | OBS | S | no | PARTIAL |
| GAP-DEPS-03 | DEPS | OBS | S | no | PARTIAL |
| GAP-DOCS-04 | DOCS | OBS | S | no | CONFIRMED |
| GAP-LIFT-07 | LIFT | OBS | S | no | PARTIAL |
| GAP-LIFT-08 | LIFT | OBS | M | no | CONFIRMED |
| GAP-SEC-09 | SEC | OBS | S | no | PARTIAL |
| GAP-TEST-08 | TEST | OBS | S | no | CONFIRMED |

## Sprint plan (revised in Round 3; one PR per code slice)

Slices marked *(wf)* edit `.github/workflows/**` and need an owned issue and
owner approval. Template-locked files change upstream only (K-5).

**Owner-only, no code**
- **O1** SEC-01, SEC-12, SEC-10: enable private vulnerability reporting, secret
  scanning with push protection, Dependabot alerts and security updates.
- **O2** GOV-07: set `can_approve_pull_request_reviews` to false.
- **O3** Create a reviewed `release` environment (policy allows `main` and
  `v*`) before any PR references it.
- **O4** Decide and record as open owner actions: (a) the bot-write route
  (GitHub App, or generated outputs off `main`; an Actions bypass would defeat
  GOV-01); (b) who may create tags and whether `release-tag.yml` stays;
  (c) Windows: sign or drop the leg; (d) reserve `uorust` and
  `uorust-certified` on crates.io or withdraw the claim (OPS-07 interim);
  (e) GOV-05, blocked on the template contract re-pin. Recorded (Sprint 1) as
  open owner-action issues: (a) #47, (b) #48, (c) #49, (d) #50, (e) #51.
- **O5** `v*` tag ruleset, after O4(b).
- **O6** Main ruleset: required `acceptance / ubuntu-24.04`, 1 approval,
  CODEOWNERS, no always-bypass. Only after C3, C4, C5 and O2.

**Code slices (in order)**
- **C1** `just scripts-test` in `vv`: run the existing script tests and check
  the executable bit of path-invoked scripts (Justfile only).
- **C2** OPS-03: fix `upstream.py frame_pin` with a test (before C3).
- **C3** *(wf)* OPS-01 phase A + OPS-02: `push.sh` fails on rejection; bound the
  formula loop; restore `land-register.sh` executable bit; hermetic test.
- **C4** *(wf)* OPS-01 phase B: route all nine main writers per O4(a).
- **C5** GOV-02: lanes through PRs (before O6 removes bypass).
- **C6** *(wf)* GOV-06 + GOV-03: run `qualify_tag.py` from `main`, require the
  tag commit on `main`, `environment: release` on publish/crates/formula,
  installer `--signer-workflow`.
- **C7** GATE-01..04, one planted test each, `just register`.
- **C8** GATE-10, GATE-11, GATE-12 (per-root definition check).
- **C9** *(wf)* GOV-04: ratchet, certified-gate, core-gate before merge.
- **C10** TEST-01, then GATE-05, 06, 08, 09.
- **C11** SEC-02, SEC-07, SEC-08 with victim-file tests.
- **C12** TEST-02..05 pins, test-only code (non-test helpers in `uorust::lift`
  trip UE-18).
- **C13** LIFT-01 with re-certification in the same PR (needs C8, C9, C12).
- **C14** LIFT-02, LIFT-04 (moves the UE-01 fixture via its Prelude copy).
- **C15** LIFT-03 (`retracted.toml` rows with witness tests, `just prove`).
- **C16** LIFT-05..08 (LIFT-05 needs certified re-export).
- **C17** *(wf)* SEC-03, 04, 05, 09, 11.
- **C18** REL-01 + DEPS-03 Unix part, after O4(c).
- **C19** *(wf)* DOCS-02 identity, with or after C18; separate download repo
  from per-version signer in `install.sh`; do not touch the thermostat fixture.
- **C20** OPS-07: publish 0.4.1+ after C19, O3, O4(d).
- **C21** DOCS-01, DOCS-03, SEC-06 control texts (same `facets.py`), DEPS-01/02,
  OPS-04/05/06, GATE-07.

**Upstream (template):** AGENTS.md (GOV-05); VERIFICATION.md wording
(GOV-03/04, DOCS-04); `bootstrap.yml` tag trigger with xtask's TRIGGERS
literal; SDK image namespace (SEC-10, DOCS-02); root layout
(UOR-Foundation/template#104).

**Unblocks the refactor pass:** C1, C7, C8, C9, C10, C12; run C13-C16 before
any `lift.rs` refactor and keep the refactor off certified roots.

## Verification log

- Round 0 (reachability probe, `qor-logic scripts reachability_probe`):
  Python surfaces `scripts/ci/upstream.py::frame_pin`, `scripts/ci/repin.py::main`,
  `scripts/mir/ledger.py::version_order` all returned WARN (no `tests/`, no
  `pyproject.toml`, no importing caller; `ledger.py` runs `sys.argv[1]` at
  import). These are shape mismatches: the scripts are CLI files invoked by path
  from workflows, not importable packages, so the probe cannot see their
  callers. Rust surfaces: SKIP (probe is Python-only; Phase 75
  missing-prerequisite tolerance). Reachability is established from workflow
  and Justfile invocation evidence in Round 1 instead.
- Round 1 (29 CRITICAL/MAJOR gaps, four independent verifiers, read-only;
  live settings read 2026-11-21 ~04:45Z): 21 CONFIRMED, 8 PARTIAL, 0 REFUTED.
  Corrections:
  - **GAP-LIFT-01 raised to CRITICAL and is live, not latent.** Two lifted
    product functions carry wrong set semantics as proved claims:
    `repo_conformance::runner::SuiteReport::ids` (conformance/src/runner.rs:95,
    frontier.toml:9469-9472) and `uorust::certify::Manifest::kappas`
    (certify.rs:2493). `comb_collect` is at lift.rs:6459-6479.
  - **GAP-LIFT-03 is live:** rust-toolchain.toml declares 32-bit targets
    (wasm32, thumbv7em) and `core/num.toml` marks the `isize` shl family
    translated (lower.rs:4894).
  - **GAP-OPS-02 root cause corrected:** core.yml/std.yml fail with
    `scripts/ci/land-register.sh: Permission denied` (exit 126); the file lost
    its executable bit in commit 83748914 (PR #24). crates.yml fails separately
    (crates.io 429s, per-crate exit 101) since at least 2026-11-16.
  - **GAP-OPS-01:** eight workflows push to `main` (seven via push.sh plus
    release.yml:311); push.sh also exits 0 after five failed rebases (:90-92);
    the formula loop is bounded by the 15-minute job timeout. A rejection is
    predicted, not yet observed (no post-ruleset run had changes to push).
  - **GAP-SEC-02 lowered to MINOR:** needs a reviewed fixture commit or
    output-dir write; CI blast radius is the checkout.
  - **GAP-SEC-05 lowered to MINOR:** the token is not passed into the
    container; reachable only through the read-write checkout or docker socket.
  - **GAP-SEC-06:** a second false threat-model control,
    `scripts/self-model/facets.py:545` controlOffline (registry.rs uses curl;
    `cargo rustc` without `--offline`).
  - **GAP-GATE-04:** the mechanism already hides shipped lines today
    (`crates/uorust/src/mir/mod.rs:33-46`, `mir/stdlib.rs:71-220`).
  - **GAP-GATE-05 lowered to MINOR:** CM IDs are documented elsewhere; the
    real gap is that unregistered prefixes are invisible.
  - **GAP-TEST-01:** 12 (not 11) untested `Inconsistent` refusals; the
    bootstrap helper predicates do have 15 planted tests, `audit()` has none;
    controlling rule is AGENTS.md:54-58.
  - **GAP-TEST-04 refuted for the certify manifest** (byte round-trip at
    certify.rs:4289-4309); stands for embed, bridge, layouts.
  - **GAP-DOCS-02:** README `afflom` references are intentional (v0.4.0 signer);
    also stale: SECURITY.md:4, model/experience.toml, app/src/Users.lex.tex:14,
    app/src/Release.lex.tex:56, scripts/release/formula.py:8-9.
  - Line-number corrections: README :248; LANES.md :21-28; VERIFICATION.md :13
    and :15; sdk.sh is 31 lines (docker run :19-31).
  New candidate gaps for Round 2: secret scanning/push protection/Dependabot
  security updates disabled; `CARGO_REGISTRY_TOKEN` absent from repo and org
  secrets while the crates job reports success; Windows release binary has no
  code-signing step; Actions may approve PRs.
- Round 2 (three verifiers: 27 MINOR/OBS gaps, four new candidates, and an
  architecture/security analysis; read-only, live settings read 2026-11-21):
  all 27 checked, 16 CONFIRMED, 11 PARTIAL, 0 REFUTED. Severity changes:
  GAP-LIFT-04 MINOR→MAJOR (silent wrong default value); GAP-SEC-09, GAP-LIFT-07,
  GAP-TEST-08, GAP-DEPS-03 (non-Unix part) lowered to OBS; GAP-OPS-06 OBS→MINOR.
  Six new gaps (GOV-06, GOV-07, SEC-12, REL-01, OPS-07, GATE-12).
  Architecture findings:
  - GAP-LIFT-01 blast radius: `SuiteReport::ids`, `Manifest::kappas`,
    `certify::cases::covered`, `Profile::uncertified`, `certify::uncertified`
    (wrong), `lift::captures` (wrong, certified root, manifest.toml:212),
    `lift::named_in` and `Acc::all_axioms` (correct by luck, certified roots).
    Thermostat golden (UE-01), UE-17 verdicts and production are unaffected.
    Recommended fix: lower sorted-set/map collects through the existing sorted
    insert, refuse `HashSet`/`HashMap`, tighten `Ty::same` by collection kind
    only; requires `just register` and `just certified` in the same change
    (`comb_collect` and `Ty::same` are certified roots; K-1, K-3).
  - GAP-LIFT-03 blast radius: about 10 `isize` shift functions in
    `crates/uorust/core/num.toml` plus an estimated 48-60 `Shl`/`ShlAssign` rows
    in `core/ops.toml`. Recommended fix: refuse like `usize`; each regressed row
    needs a `crates/uorust/core/retracted.toml` entry with a witness test
    (queue.rs:54-61 refuses translated→residue). An obligation would add a
    precondition Rust does not have.
  - The certified manifest has 167 roots (not 160).
  - Smallest change set closing the most risk: owner settings (required checks,
    1 approval, CODEOWNERS, remove always-bypass, `v*` tag ruleset, reviewed
    `release` environment, `can_approve_pull_request_reviews=false`);
    `qualify_tag.py` accepts only commits on `main`; `pages.yml` installs from
    the lockfile with attestation in a separate job; per-shard crate artifacts
    without `merge-multiple`; confine `write` manifest paths.
- Round 3 (blast radius, hidden dependencies, sprint ordering; read-only):
  dependencies checked against the files. Key ones: OPS-01 phase A must land
  with or before OPS-02 (restoring the bit makes core/std attempt bot pushes,
  which push.sh would mask); OPS-03 before OPS-01 (upstream.yml is a ninth main
  writer); GOV-07 before GOV-01 approvals; GOV-02 before removing bypass;
  GOV-06 needs a tag ruleset (the qualifier runs from the tag itself);
  the `release` environment must exist before code references it; DOCS-02
  before the first crates.io publish (metadata is immutable) and with REL-01
  (install.yml's Windows leg); LIFT-01 needs GATE-12 and GOV-04 first.
  Severity/effort changes: OPS-07 MAJOR→CRITICAL (unclaimed crate name
  advertised and installed by `install.yml`); GATE-12 MINOR→MAJOR; efforts
  revised for GOV-01, GOV-06, OPS-01, OPS-02, REL-01, LIFT-01, LIFT-03, LIFT-05.
  Refuted sub-claim: DEPS-03 "unused `prismpm.yml`" (`audit_bootstrap` reads
  about 20 literals from it). Simulated: the GATE-01..04 fixes newly fail no
  existing code.

Already refuted during recon: CONF-25 "origin/main deleted docs/plans" (main
never had it).
