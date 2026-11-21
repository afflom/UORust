# Prism factory

This directory is the upstream Prism factory (PrismPM's `prism-factory/`),
installed with this tool's graph. It decides, for every obligation of the
tool's gate, its writers and its certified export, whether it is already
proved, must run, is refuted, or is blocked, and in what order to run what is
left. The decisions are the upstream's Prism model, checked by the Lean kernel
and exported by LexLean to certified Rust; the host only reads the tree, runs
what the plan routes to `observe`, and records receipts.

What is this tool's own is its graph and nothing else. `upstream.toml` names
the revision every other file is taken from and the SHA-256 of each; `graph.py
check` (inside the `factory-model` obligation) refuses a file that drifted, and
`graph.py upstream DIR` compares the component with upstream's at that revision.

## Layout

| Path | What it is | Upstream |
| --- | --- | --- |
| `model/src/Factory/Engine.lex.tex` | Obligations, identity, closures, routing, priority, and their theorems | identical |
| `std/` | LexLean's standard library (`Buffer`, `Sha256`, `Text`) the engine uses | identical (its lock is the SDK compiler's) |
| `model/src/Factory.lex.tex` | The roots `plan` and `reweigh` and their text protocol | derived by `graph.py derive`: upstream's definitions of the two roots, over this graph |
| `model/src/Factory/UORust.lex.tex` | This tool's graph and its theorems | written by `graph.py write` from `graph.toml` and the Justfile |
| `graph.toml` | The graph's one source: each obligation's recipe, kind, inputs and needs | the tool's |
| `certified/` | The production bundle of the two roots: certified Rust packages, certificates, attestation | exported here by the SDK's LexLean |
| `host/` | The thin host (`prism-factory` CLI): upstream's `request`, `plan`, `run`, `plan-request`, `reweigh` | upstream's code for those commands; `run` also takes the base and the closure repository from upstream's `forge.rs`, and adds the tool's two checks (a command stopped by a signal leaves no receipt; a work directory under a cargo or rustup configuration is refused) |
| `check.sh` | The `factory-model` obligation | the tool's, after upstream's |
| `factory.sh` | The entry: `routes`, `run`, `start`, `status`, and the host's commands; one run per store (a lock), the reads check before planning | the tool's, after upstream's |
| `sdk.sh` | Runs an obligation's command in the SDK image `prismpm.lock` selects | the tool's |
| `written.sh` | A writer's obligation: passes only when the writer changes nothing it writes | the tool's |

The release workflow, the cut and the forge's delegation of upstream
(`Workflow.lex.tex`, `Delegation.lex.tex`, `PrismPM.lex.tex`, `jobs/`, the
`actions-*` and `forge-*` commands) are PrismPM's own release and are not
installed.

## Identity

The identity is upstream's: the engine that computes it is byte for byte
upstream's, and the next three paragraphs are upstream's README text at the
recorded revision, verbatim. `host/tests/core.rs` holds the plan's identities
against Python's `hashlib` over the same text.

An obligation's identity is the SHA-256 (in hex) of:

```text
prism-factory/obligation/1
name <name>
kind hermetic|fetch
environment <environment>
run <run>
input <prefix>                 one line per declared input, in order (`!prefix` excludes, `@key` is a context)
needs <name> <identity>        one line per need, in order
closure <sha256 of the closure>
```

The closure is one line `file <mode>:<object> <path>` for each tracked entry
covered by the declared inputs, in Git tree order, then one line
`file context:<value> @<key>` for each context it declares. An entry is covered
when the longest inclusion it starts with is longer than the longest exclusion
(`!prefix`) it starts with; a context `@key` is reached only by an input that
starts with `@`. Changing a file,
an input, the command, the environment or any upstream identity changes the
identity. Nothing else does.

A closure is its own Git repository with one commit that tracks exactly the
closure, so `git ls-files` there lists what the identity covers. A ratchet
compares the tree with its *base*, the revision's first parent (for a pull
request's merge commit, the tip it merges into). The request carries the base
as `@base:PATH` entries, and an obligation reads only the ones it declares:
`model-audits` declares `@base:model/modules.toml`, so the module baseline
(rule 6 of `check-modules`) may not grow, and its identity changes only when
that manifest does. Its closure commit then has a parent holding exactly those
entries, named `origin/main` as a full checkout names the base.

That paragraph speaks of upstream's own graph. In this tool's graph the
ratchet is UE-18 (`crates/conformance/tests/ratchet.rs`), and `test` declares
the two files of the parent it compares, `@base:frontier.toml` and
`@base:crates/certified/manifest.toml`. So two commits with one tree and
different parents are two identities of `test`. The host takes the base and
the closure repository from upstream's `forge.rs`, and runs every obligation
in such a closure. In a closure, a missing base fails UE-18; it is not
skipped.

## The graph

Every obligation runs on the host (`environment host`): an obligation that
needs the SDK runs its recipe through `sdk.sh`, which reads the image from
`prismpm.lock`, and both are declared inputs, so the image's digest is part of
the identity. A compiling obligation fetches locked crates, so it is a `fetch`
obligation as upstream's are: a failure is observed again, never refuted.

<!-- graph:begin (graph.py write) -->
| Obligation | Kind | Needs | Reads | Not touched by |
| --- | --- | --- | --- | --- |
| `factory-model` | hermetic | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `prism-factory/`, `Justfile`, `rust-toolchain.toml` | anything outside prism-factory/, the Justfile, the toolchain file and the SDK lock |
| `factory-upstream` | fetch | nothing | `prism-factory/` | anything outside prism-factory/ |
| `fmt-check` | hermetic | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `!crates/uorust/core/`, `!crates/uorust/std/`, `!crates/uorust/crates/`, `!crates/uorust/ledger/`, `!crates/uorust/tests/fixtures/`, `crates/uorust/tests/fixtures/nist-sha256/`, `xtask/`, `app/lexlean.lock`, `.github/dependabot.yml`, `.github/workflows/bootstrap.yml`, `.github/workflows/prismpm.yml`, `.github/workflows/template-update.yml`, `rustfmt.toml` | the model, the app, the registers, the fixtures, scripts, the site, the documents |
| `model` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `.devcontainer/`, `.github/`, `AGENTS.md`, `CONFORMANCE.md`, `README.md`, `VERIFICATION.md`, `template.lock`, `template-contract.json`, `crates/`, `xtask/`, `model/`, `LANES.md`, `TEMPLATE-CONTRACT.md`, `TEMPLATE-VERIFICATION.md` | the app, scripts, the site, the brand, frontier.toml, lift-verdicts.toml |
| `template-check` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `.devcontainer/`, `.github/`, `AGENTS.md`, `CONFORMANCE.md`, `README.md`, `VERIFICATION.md`, `template.lock`, `template-contract.json`, `crates/`, `xtask/`, `model/`, `Justfile` | the app, scripts, the site, the brand, frontier.toml, lift-verdicts.toml |
| `self-model-check` | hermetic | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `model/`, `app/`, `brand/`, `scripts/self-model/`, `scripts/experience/`, `scripts/mir/ledger.py`, `crates/uorust/src/`, `crates/uorust/core/frontier.toml`, `crates/uorust/std/frontier.toml`, `crates/uorust/crates/`, `crates/certified/manifest.toml`, `frontier.toml`, `site/app.json`, `.github/workflows/`, `Cargo.toml`, `README.md` | the other crates, the fixtures, xtask/, the lock files, lift-verdicts.toml |
| `lint` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `!crates/uorust/core/`, `!crates/uorust/std/`, `!crates/uorust/crates/`, `!crates/uorust/ledger/`, `!crates/uorust/tests/fixtures/`, `crates/uorust/tests/fixtures/nist-sha256/`, `xtask/`, `app/lexlean.lock`, `.github/dependabot.yml`, `.github/workflows/bootstrap.yml`, `.github/workflows/prismpm.yml`, `.github/workflows/template-update.yml`, `clippy.toml` | the model, the app, the registers, the fixtures, scripts, the site, the documents |
| `test` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `xtask/`, `frontier.toml`, `lift-verdicts.toml`, `app/lexlean.lock`, `.github/`, `model/`, `features/`, `app/`, `Justfile`, `CONFORMANCE.md`, `README.md`, `VERIFICATION.md`, `ANALYSIS.md`, `scripts/ci/`, `scripts/hooks/passes.py`, `scripts/release/`, `scripts/self-model/`, `bootstrap/acceptance-result.test.mjs`, `site/acceptance-result.mjs`, `site/guest-worker.mjs`, `site/spec.mjs`, `prism-factory/`, `brand/foundation/src/index.css`, `@base:frontier.toml`, `@base:crates/certified/manifest.toml` | the site other than the three modules SM-07 loads, the brand other than its foundation stylesheet, scripts other than scripts/ci/, scripts/release/, scripts/self-model/ and scripts/hooks/passes.py, the documents other than CONFORMANCE.md, README.md, VERIFICATION.md and ANALYSIS.md, the parent's files other than its register and manifest |
| `deny` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `!crates/uorust/core/`, `!crates/uorust/std/`, `!crates/uorust/crates/`, `!crates/uorust/ledger/`, `!crates/uorust/tests/fixtures/`, `crates/uorust/tests/fixtures/nist-sha256/`, `xtask/`, `app/lexlean.lock`, `.github/dependabot.yml`, `.github/workflows/bootstrap.yml`, `.github/workflows/prismpm.yml`, `.github/workflows/template-update.yml`, `deny.toml` | the model, the app, the registers, the fixtures, scripts, the site, the documents |
| `app` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `!crates/uorust/core/`, `!crates/uorust/std/`, `!crates/uorust/crates/`, `!crates/uorust/ledger/`, `!crates/uorust/tests/fixtures/`, `crates/uorust/tests/fixtures/nist-sha256/`, `xtask/`, `app/lexlean.lock`, `.github/dependabot.yml`, `.github/workflows/bootstrap.yml`, `.github/workflows/prismpm.yml`, `.github/workflows/template-update.yml`, `app/`, `Justfile` | the site, registers, non-workspace scripts and documents; app, Rust and recipe inputs invalidate this proof |
| `uorust` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `xtask/`, `frontier.toml`, `lift-verdicts.toml`, `app/lexlean.lock`, `.github/` | the model, the app, scripts, the site, the documents |
| `vv` | hermetic | `fmt-check`, `model`, `template-check`, `self-model-check`, `lint`, `test`, `deny`, `app`, `uorust` | `Justfile` | everything its parts leave untouched, and the Justfile's other recipes only through its own input |
| `prove-fixture` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `!crates/uorust/core/`, `!crates/uorust/std/`, `!crates/uorust/crates/`, `!crates/uorust/ledger/`, `!crates/uorust/tests/fixtures/`, `crates/uorust/tests/fixtures/nist-sha256/`, `xtask/`, `app/lexlean.lock`, `.github/dependabot.yml`, `.github/workflows/bootstrap.yml`, `.github/workflows/prismpm.yml`, `.github/workflows/template-update.yml`, `crates/uorust/tests/fixtures/mir-path/`, `prism-factory/written.sh` | the model, the app, the other fixtures, the registers, scripts, the site, the documents |
| `prove` | fetch | nothing | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `xtask/`, `frontier.toml`, `lift-verdicts.toml`, `app/lexlean.lock`, `.github/`, `prism-factory/written.sh` | the model, the app, scripts, the site, the documents |
| `certified` | fetch | `prove` | `prismpm.lock`, `prism-factory/sdk.sh`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, `crates/`, `xtask/`, `frontier.toml`, `lift-verdicts.toml`, `app/lexlean.lock`, `.github/`, `Justfile`, `prism-factory/written.sh` | the model, the app, scripts, the site, the documents |
<!-- graph:end -->

## Running it

```sh
just factory-plan            # each obligation's route and how many are left (prism-factory/factory.sh routes)
just factory                 # plan and run to the fixed point; `just factory NAME...` runs only those
just factory-start           # the same, detached; its log and status are written beside the receipts
just factory-status          # `running PID TIME`; `done CODE …` or `interrupted SIGNAL …`, then the counts
                             # (established N proved N refuted N blocked N observe N); `stale …` when the run's process is gone
```

`run` copies each obligation's closure, and nothing else, into a fresh
directory outside any repository (`FACTORY_WORK`) and runs it there, so a
tracked file the obligation does not declare is absent there and a read of it
fails. Three more reads would escape a closure, and each is closed:

- Cargo reads `.cargo/config.toml` in every ancestor of the directory it runs
  in, and rustup reads the nearest `rust-toolchain.toml`. The host refuses a
  `FACTORY_WORK` that has such a file in an ancestor, before anything runs.
  `factory-model` declares the workspace's `rust-toolchain.toml`, as every
  compiling obligation does.
- Cargo reads its home's `config.toml`. Inside the SDK, `sdk.sh` gives the
  command a cargo home of its own. It holds links to the image's registry and
  git caches and installed tools, but no configuration and no credentials.
- A source names a tracked file at run time, or embeds one
  (`include_str!`). `factory.sh` refuses to plan when such a file is not
  declared by the obligation that compiles or runs the source
  (`graph.py reads`).

A receipt is written as each obligation ends. A command stopped by a signal
(an OOM kill, a killed session; `bash` reports these as exit 128 + N for
HUP, INT, KILL and TERM) reached no verdict, so it leaves no receipt and the
run stops. An interrupted run therefore loses only the obligation in flight,
and the next run runs that obligation again. A second run on the same tree
runs nothing. `run` and `start` take an exclusive lock on the store
(`flock` on `FACTORY_RECEIPTS/lock`) before they read or write the status, and
the detached run holds it until its last process ends. So a second start is
refused while one is in progress, and two runs never share closures or a log.
The receipts live in `FACTORY_RECEIPTS`, a directory that outlives the
worktree and every target directory. `factory.sh keep BRANCH` would add them
to a receipts branch of the forge, upstream's store; it pushes, so it runs
only where the repository's owner grants that push.

## Changing the graph

```sh
python3 prism-factory/graph.py write                     # graph.toml and the Justfile -> UORust.lex.tex and the table
cd prism-factory/model && lexlean lock && lexlean verify --all
lexlean production targets --diagnostic-format json > /tmp/targets.json
python3 ../graph.py request /tmp/targets.json            # only when the SDK's compiler changed
lexlean production build --request ../production-request.json
rm -rf ../certified && cp -r .lexlean/production/<result> ../certified
```

The graph is compiled into the certified `plan` root, as upstream's is, so a
change of an obligation's recipe, inputs or needs is a new export.

## Limits

- Obligations run with the host's network and user, as upstream's `host`
  obligations do. The closure is enforced; network and uid isolation are not.
  A compiling obligation reads the SDK container's cargo cache, which is why it
  is a `fetch` obligation.
- A receipt is trusted because of who can write the receipt store. A receipt
  established on a workstation counts on the forge only when a listed
  maintainer signed it for exactly its identity (upstream's delegation model).
  No signing key exists here and none is made, and the forge's workflows keep
  no receipts, so a forge run would establish every obligation its event asks
  for again. What is carried by content regardless of receipts is carried
  there too: the certified packages in `crates/certified/` and their export
  identities (`uorust certify production --carry`).
- No workflow calls this entry yet. The forge still establishes, on its own and
  whole, what the plan would route:
  - `bootstrap.yml` runs `just vv`, the whole gate, on every pull request,
    every push to `main` and every tag.
  - `generate.yml` runs every writer on each push to `main` and hourly
    (cron).
  - `certified.yml` decides its shards with `just certified-plan`, outside the
    factory.
  - `core.yml` and `crates.yml` run daily (cron).

  On the forge, with no kept receipts and no signed ones, the plan would leave
  every obligation, so routing these workflows through the entry would change
  what they decide, not what they run. Routing them waits on the receipts
  branch and a maintainer's signing key, and no trigger is added here.
- Rustup's directory overrides (`rustup override set`, kept in its home) and
  the toolchains installed in the SDK container are not declared. The image is
  pinned by digest, and the channel by each compiling obligation's
  `rust-toolchain.toml`. An override set by hand inside the container is a
  change of the place that no identity sees.
- A process the run started inherits the store's lock. An obligation's command
  left running after its host died (an orphan) therefore keeps a new start
  refused until it ends. Its result is not recorded.
