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
    cargo run -q --locked -p uorust -- project . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z
    cargo run -q --locked -p uorust -- check . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --register frontier.toml
    cd target/uorust-self && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check
    rm -rf target/uorust-advised
    cargo run -q --locked -p uorust -- project crates/uorust/tests/fixtures/advised --out target/uorust-advised --name advised --last-modified 2026-11-11T00:00:00Z --advisories crates/uorust/tests/fixtures/advised/advisory-db
    cd target/uorust-advised && lexlean fmt --check && lexlean lock --check && lexlean verify && prismpm check

# Rewrite this workspace's lift register, frontier.toml (UE-16): refused when a
# function it records as lifted is decided otherwise by the current source.
register:
    rm -rf target/uorust-self
    cargo run -q --locked -p uorust -- project . --out target/uorust-self --name uorust --last-modified 2026-11-11T00:00:00Z --register frontier.toml

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

# Every writer, in the order their outputs feed each other: the self model and
# the fixture projection from the sources, the lift register from the
# workspace, then CONFORMANCE.md from the register. `generate.yml` runs this on
# every push to main and commits what changed, so no generated file is stale.
generate: self-model-write fixtures register model-write

# The content standard (scripts/content/standard.json) on an assembled site.
content site="site":
    node scripts/content/enrich.mjs {{site}}
    node scripts/content/check.mjs {{site}}
