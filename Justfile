# `just vv` is the normative acceptance gate. Every other recipe is either one
# of its parts or a writer the gate checks the output of.
#
# Each check in `vv` runs exactly once. The parts are ordered so the cheapest
# failure is reported first: formatting needs no compiler, the repository gates
# compile only `xtask`, and the SDK's checks run before the workspace build.

default: vv

# The whole gate.
vv: fmt-check model template-check self-model-check lint test deny app uorust
    @echo "vv: the acceptance gate passed"

fmt-check:
    cargo fmt --all -- --check

# R1, R4, R5 and the bootstrap trust root: `check-model`, `audit-limits`,
# `audit-deferral` and `audit-bootstrap` (which includes
# `audit-dependency-updates`), in one `xtask` process.
model:
    cargo run -q --locked -p xtask -- validate

# The SDK's own checks of the contract and both locks. `template check` also
# regenerates `CONFORMANCE.md` in an isolated copy through `model-write`.
template-check:
    prismpm template check
    prismpm lock check

# Clippy at `-D warnings` over every target with every feature enabled. This is
# the one place an optional feature is compiled: a rename upstream of an
# optional dependency fails nowhere else until someone turns the flag on, and
# `--all-targets` because the tests behind a flag are code too. `test` compiles
# the default feature set.
lint:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# The workspace suite, which includes the `repo-conformance` meta-gate (R3,
# `CM-02`, `CM-03`).
test:
    cargo test --workspace --locked

# R6: nothing shipped depends on a dev-only crate, no wildcard version
# requirement, no advisory against anything in the tree. `cargo-deny` is
# supplied by the locked SDK.
deny:
    cargo deny --all-features check

# UORust's own Prism model: the route table as an application and every facet
# of UORust. The model must be canonical and locked. `prismpm verify` builds
# the holo, replaying its acceptance vectors in the compiled guest, then has the
# Lean kernel and leanchecker check every theorem (`SM-02`) and the generated
# code against the model. It runs the build itself, so a separate
# `prismpm build` would repeat it.
app:
    cd app && lexlean fmt --check && lexlean lock --check
    prismpm --project app verify

# UE-01, UE-07, UE-11, UE-12: the engine's projections, checked by the SDK.
# The committed fixture projection must be current and accepted; fresh
# projections of this workspace and of the advised fixture, whose locked
# smallvec has RustSec advisories, must be accepted too. `lexlean verify` elaborates both in
# Lean, so every lifted definition type-checks and every acceptance theorem is
# proved. The stamp is fixed so the bytes are.
uorust:
    cargo run -q --locked -p uorust -- check crates/uorust/tests/fixtures/thermostat --out crates/uorust/tests/fixtures/thermostat-prism --name thermostat --last-modified 2026-11-11T00:00:00Z
    cd crates/uorust/tests/fixtures/thermostat-prism && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check
    rm -rf target/uorust-self
    cargo run -q --locked -p uorust -- mir . --out target/mir
    cargo run -q --locked -p uorust -- project . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z {{mir}}
    cargo run -q --locked -p uorust -- check . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --register frontier.toml {{mir}}
    cd target/uorust-self && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check
    rm -rf target/uorust-advised
    cargo run -q --locked -p uorust -- project crates/uorust/tests/fixtures/advised --out target/uorust-advised --name advised --last-modified 2026-11-11T00:00:00Z --advisories crates/uorust/tests/fixtures/advised/advisory-db
    cd target/uorust-advised && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check

# The MIR path's inputs for this workspace: the dumps `uorust mir` writes and
# the verdict register `uorust prove` writes (UE-17).
mir := "--mir target/mir --verdicts lift-verdicts.toml"

# Rewrite this workspace's lift register, frontier.toml (UE-16): refused when a
# function it records as lifted is decided otherwise by the current source.
# The MIR path runs too: every translation the verdicts record proved lifts.
register:
    rm -rf target/uorust-self
    cargo run -q --locked -p uorust -- mir . --out target/mir
    cargo run -q --locked -p uorust -- project . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --register frontier.toml {{mir}}

# Decide every translation of the MIR path with `lexlean verify` and rewrite
# the verdict register, lift-verdicts.toml (UE-17): the translations whose
# obligations discharge are proved, the others refused with the verifier's
# diagnostic as their cause.
prove:
    cargo run -q --locked -p uorust -- mir . --out target/mir
    cargo run -q --locked -p uorust -- prove . --out target/uorust-prove --scratch target/mir-prove --name uorust --last-modified 2026-11-11T00:00:00Z {{mir}}

# Rewrite the core register, crates/uorust/core: every body of core's MIR at
# the pinned toolchain's rust-src, dumped one module shard at a time, then
# translated by the MIR path; refused when a body it records as translated is
# now residue.
core:
    scripts/mir/dump-core.sh target/core-mir
    cargo run -q --locked -p uorust -- core --mir target/core-mir --library target/rust-src-`sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml`/library --register crates/uorust/core

# Fetch into target/core-mir the MIR CI dumped of core, under the current
# key (toolchain κ, dump-script digest): a local dump can differ from CI's,
# and then a local census cannot see the regressions CI's refuses. A module CI
# has no dump of under the key is dumped here instead.
core-mir:
    scripts/mir/fetch-mir.sh core target/core-mir

# The core register's ratchet on CI's own MIR: fetch it (`core-mir`),
# translate every module against a copy of the committed register and fail
# when a body it records as translated is residue. The committed register is
# left as it is. Extra arguments go to cargo (a lane's `--config` flags).
core-gate *cargo:
    scripts/mir/core-gate.sh {{cargo}}

# Rewrite the std register, crates/uorust/std, as `core` does core's.
std:
    scripts/mir/dump-lib.sh std target/std-mir
    cargo run -q --locked -p uorust -- std --mir target/std-mir --library target/rust-src-`sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml`/library --register crates/uorust/std

# Rewrite the register of a crate of crates.io, crates/uorust/crates/<name>:
#
#     just crate itoa@1.0.15
crate spec:
    cargo run -q --locked -p uorust -- crate {{quote(spec)}}

# Only the meta-gate, while iterating on a scenario. `test` already runs it.
bdd:
    cargo test --locked -p repo-conformance

fmt:
    cargo fmt --all

# Regenerate everything the model owns: CONFORMANCE.md.
model-write:
    cargo run -q --locked -p xtask -- check-model --write

# R3's first step in one command: a register row, its scenario, a failing test
# named for the ID, and the regenerated CONFORMANCE.md.
#
#     just new-capability CX-01 build parsing "A parsed document round-trips"
new-capability id level suite statement:
    cargo run -q --locked -p xtask -- new-capability {{quote(id)}} {{quote(level)}} {{quote(suite)}} {{quote(statement)}}

# UORust's self model is generated: model/routes.toml and
# scripts/self-model/facets.py write the route table, its vectors and the facet
# modules of app/. The check regenerates them in a scratch copy and compares
# byte for byte; the writer regenerates in place, locks, formats, and rebinds
# the System release digest that `prismpm check` reports (PP2101).
self-model-check:
    scripts/self-model/check.sh

self-model-write:
    scripts/self-model/write.sh

# Rewrite the committed fixture projection that `uorust` checks (UE-01): the
# same command line as the check, with `project` in place of `check`.
fixtures:
    cargo run -q --locked -p uorust -- project crates/uorust/tests/fixtures/thermostat --out crates/uorust/tests/fixtures/thermostat-prism --name thermostat --last-modified 2026-11-11T00:00:00Z

# Decide every translation of the MIR path's fixture with `lexlean verify` and
# rewrite its verdict register, crates/uorust/tests/fixtures/mir-path's
# lift-verdicts.toml, which the tests decide the fixture on without a verifier.
prove-fixture:
    cargo run -q --locked -p uorust -- mir crates/uorust/tests/fixtures/mir-path --out target/mir-fixture
    cargo run -q --locked -p uorust -- prove crates/uorust/tests/fixtures/mir-path --out target/mir-fixture-prove --scratch target/mir-fixture-scratch --name walk --last-modified 2026-11-11T00:00:00Z --mir target/mir-fixture --verdicts crates/uorust/tests/fixtures/mir-path/lift-verdicts.toml

# Every writer, in the order their outputs feed each other: rustfmt first,
# since the self model records source line numbers, then the self model and
# the fixture projection from the sources, the MIR path's verdicts (the
# fixture's and the workspace's) and the lift register from the workspace,
# then CONFORMANCE.md from the register. `generate.yml` runs this on every
# push to main and hourly, and commits what changed, so no generated file is
# stale.
generate: fmt self-model-write fixtures prove-fixture prove register model-write

# The certified translator (loop step 3): the self projection's lifted
# translator functions (uorust::mir, lift, lex) exported through `lexlean
# production` (SPEC §24.7) as kernel-checked Rust, each root the compiler finds
# ineligible refused with its diagnostic, the bundle replayed with
# `verify-artifact`, then taken into crates/certified: every package source
# byte for byte as the bundle records it, and manifest.toml keying each root by
# the κ of the Rust function it certifies, with the bootstrap's share.
certified:
    rm -rf target/uorust-self target/uorust-certify target/uorust-certified
    cargo run -q --locked -p uorust -- mir . --out target/mir
    cargo run -q --locked -p uorust -- project . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --register frontier.toml {{mir}}
    lexlean production targets --diagnostic-format json >crates/certified/targets.json
    cargo run -q --locked -p uorust -- certify production --projection target/uorust-self --register frontier.toml --targets crates/certified/targets.json --out target/uorust-certify --kept target/uorust-certified
    cargo run -q --locked -p uorust -- certify sync --kept target/uorust-certified --register frontier.toml --mir target/mir --workspace . --dir crates/certified --adapters crates/uorust/src/certified.rs
    rustfmt --edition 2021 crates/uorust/src/certified.rs

# The ratchet (loop step 3): the hand-written translator may only shrink.
# The head's register and the base's are both generated from their sources
# (the base's in a worktree of `base`), so a stale committed register can
# neither hide a growth nor fake one; each root is certified by the manifest of
# its own commit. Refused, naming each new uncertified function and its cause,
# when more translator functions are uncertified than at the base. No bypass.
ratchet base="HEAD~1":
    #!/usr/bin/env bash
    set -euo pipefail
    rm -rf target/ratchet && mkdir -p target/ratchet
    git worktree remove --force target/ratchet-base 2>/dev/null || true
    git worktree add --detach target/ratchet-base {{base}}
    trap 'git worktree remove --force target/ratchet-base' EXIT
    cargo run -q --locked -p uorust -- mir . --out target/mir
    cp frontier.toml target/ratchet/head.toml
    cargo run -q --locked -p uorust -- project . --out target/ratchet/head --name uorust --last-modified 2026-11-11T00:00:00Z --register target/ratchet/head.toml {{mir}}
    if [[ -f target/ratchet-base/frontier.toml ]]; then
      (cd target/ratchet-base && CARGO_TARGET_DIR=../../target cargo run -q --locked -p uorust -- mir . --out ../ratchet/base-mir)
      cp target/ratchet-base/frontier.toml target/ratchet/base.toml
      (cd target/ratchet-base && CARGO_TARGET_DIR=../../target cargo run -q --locked -p uorust -- project . --out ../ratchet/base --name uorust --last-modified 2026-11-11T00:00:00Z --register ../ratchet/base.toml --mir ../ratchet/base-mir --verdicts lift-verdicts.toml)
    else
      cp target/ratchet/head.toml target/ratchet/base.toml
    fi
    base_manifest=target/ratchet-base/crates/certified/manifest.toml
    cargo run -q --locked -p uorust -- certify ratchet --register target/ratchet/head.toml --manifest crates/certified/manifest.toml --base-register target/ratchet/base.toml --base-manifest "$base_manifest"

# The ratchet before a commit, on what is staged: the index's register and
# manifest against HEAD's (scripts/hooks/ratchet-staged.sh, which the
# pre-commit hook runs; `just hooks` installs it). CI's `ratchet` decides on registers generated from both commits.
ratchet-staged *cargo:
    scripts/hooks/ratchet-staged.sh {{cargo}}

# The differential and the fixpoint (loop step 4). Every body of core runs
# through the certified translator and through the hand-written one
# (`--translator hand`), and the translator translates its own MIR both ways:
# stage 2 is the self projection and register by the hand-written translator,
# stage 3 by the certified one. `certify agree` computes each output's κ (over
# every file's path and bytes) and refuses any pair whose κ differ; the κ it
# prints land in {{out}}/kappa.txt, which certified.yml compares across runs
# and architectures. An exit code is never the verdict: the κ are.
certified-gate out="target/certified-gate":
    #!/usr/bin/env bash
    set -euo pipefail
    rm -rf {{out}} && mkdir -p {{out}}
    [[ -d target/core-mir ]] || scripts/mir/dump-core.sh target/core-mir
    library=target/rust-src-`sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml`/library
    cargo run -q --locked -p uorust -- mir . --out target/mir
    for t in certified hand; do
      cp -r crates/uorust/core {{out}}/core-$t
      cargo run -q --locked -p uorust -- --translator $t core --mir target/core-mir --library "$library" --register {{out}}/core-$t
      mkdir -p {{out}}/register-$t && cp frontier.toml {{out}}/register-$t/frontier.toml
      cargo run -q --locked -p uorust -- --translator $t project . --out {{out}}/self-$t --name uorust --last-modified 2026-11-11T00:00:00Z --register {{out}}/register-$t/frontier.toml {{mir}}
    done
    cargo run -q --locked -p uorust -- certify agree --dir {{out}} >{{out}}/kappa.txt
    cat {{out}}/kappa.txt

# Install the repository's hooks: the pre-commit ratchet, and the pre-push
# gate (rustfmt, xtask's R1/R4/R5 gates, and `core-gate` when the push
# touches the MIR path).
hooks:
    git config core.hooksPath scripts/hooks

# The content standard (scripts/content/standard.json) on an assembled site.
content site="site":
    node scripts/content/enrich.mjs {{site}}
    node scripts/content/check.mjs {{site}}
