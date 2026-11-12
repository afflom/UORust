<p align="center">
  <a href="https://afflom.github.io/UORust/">Live route table</a>
  ·
  <a href="model/routes.toml">Route table source</a>
  ·
  <a href="CONFORMANCE.md">Conformance register</a>
  ·
  <a href="https://github.com/UOR-Foundation/PrismPM">PrismPM</a>
  ·
  <a href="https://github.com/UOR-Foundation/LexLean">LexLean</a>
  ·
  <a href="AGENTS.md">Agent rules</a>
</p>

<p align="center">
  <a href="https://github.com/afflom/uorust/actions/workflows/bootstrap.yml"><img src="https://github.com/afflom/uorust/actions/workflows/bootstrap.yml/badge.svg?branch=main" alt="bootstrap"></a>
  <a href="https://github.com/afflom/uorust/actions/workflows/pages.yml"><img src="https://github.com/afflom/uorust/actions/workflows/pages.yml/badge.svg?branch=main" alt="pages"></a>
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License: MIT OR Apache-2.0">
</p>

# UORust

UORust reads any Cargo workspace and writes a complete
[Prism](https://github.com/UOR-Foundation/PrismPM) model of it: architecture,
design views, features and journeys, behavior, security with a threat model,
quality, and the release's bill of materials. The model is a LexLean project
that the locked PrismPM SDK checks, so a Rust codebase gets a formal,
reviewable description without anyone writing it by hand.

UORust is itself modeled in every Prism facet, in [`app/`](app), and that
model is verified by the same SDK. Its application is UORust's route table:
the function that decides which Prism record each kind of Rust fact becomes.
The [Pages site](https://afflom.github.io/UORust/) runs that verified table in
the browser.

## How it works

```
cargo metadata, Cargo.lock, syn  ->  facts with source spans
facts                           ->  router (model/routes.toml)
routed facts                    ->  admission: complete records, or typed refusals as open claims
admitted records                ->  emitter: prismpm.toml, lexlean.toml, one module per facet, register
```

The pipeline is the [Foundry](https://github.com/UOR-Foundation/foundry)
pattern: proposals from a router, admission by a typed rule, and a
byte-checked projection. Nothing is defaulted. A fact whose kinds name two
different routes is refused as ambiguous; a kind no route names is refused as
unknown; each refusal becomes an open claim against the fact's source span.
Re-running on the same workspace writes byte-identical files.

## Install it

Every channel installs the same `uorust` binary that a release of
[`.github/workflows/release.yml`](.github/workflows/release.yml) builds, or
builds it from the locked sources. CI installs through each one and runs
`uorust --version`
([`.github/workflows/install.yml`](.github/workflows/install.yml)).

| Channel | Command | What is checked |
| --- | --- | --- |
| Install script (Linux, macOS) | `curl -fsSL https://afflom.github.io/UORust/install.sh \| sh` | SHA-256 against the release's `SHA256SUMS`; with `UORUST_ATTESTATION=1`, also the build provenance |
| Install script (Windows) | `powershell -ExecutionPolicy Bypass -c "irm https://afflom.github.io/UORust/install.ps1 \| iex"` | the same |
| cargo-binstall | `cargo binstall uorust` | the release archive `[package.metadata.binstall]` names |
| Homebrew | `brew tap afflom/uorust https://github.com/afflom/UORust` then `brew install afflom/uorust/uorust` | the formula's `sha256`, written from `SHA256SUMS` |
| Nix | `nix run github:afflom/UORust -- --version` or `nix profile install github:afflom/UORust` | builds `main` from `Cargo.lock` at a pinned nixpkgs |
| crates.io | `cargo install --locked uorust` | builds from crates.io with the published `Cargo.lock` |
| From a tag | `cargo install --locked --git https://github.com/afflom/UORust --tag v0.1.1 uorust` | builds that tag with its `Cargo.lock` |

The install script takes `UORUST_VERSION` (a release such as `0.1.1`; the
newest by default) and `UORUST_INSTALL_DIR` (default `~/.local/bin`; on Windows
`%LOCALAPPDATA%\Programs\uorust\bin`, which it adds to the user `PATH`). Read it
before running it: [`install.sh`](install.sh), [`install.ps1`](install.ps1).
`cargo binstall uorust` and `cargo install --locked uorust` work once the crate
is on crates.io; until then use `cargo binstall --git
https://github.com/afflom/UORust uorust`, or install from a tag.

### Release targets

Each release carries one archive per target, its `.sha256`, and `SHA256SUMS`
listing them all. The table is the build matrix of `release.yml`, written by
`scripts/self-model/release.py`:

<!-- release-targets:begin (scripts/self-model/release.py) -->
| Target | Archive | Built on | Release waits for it |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `uorust-x86_64-unknown-linux-gnu.tar.gz` | `ubuntu-24.04` | yes |
| `aarch64-unknown-linux-gnu` | `uorust-aarch64-unknown-linux-gnu.tar.gz` | `ubuntu-24.04-arm` | yes |
| `x86_64-apple-darwin` | `uorust-x86_64-apple-darwin.tar.gz` | `macos-15` | yes |
| `aarch64-apple-darwin` | `uorust-aarch64-apple-darwin.tar.gz` | `macos-15` | yes |
| `x86_64-pc-windows-msvc` | `uorust-x86_64-pc-windows-msvc.zip` | `windows-2025` | no |
<!-- release-targets:end -->

The release waits for every target marked yes; one marked no is attached when
it builds.

### Verify a download by hand

```sh
gh release download v0.1.1 --repo afflom/UORust --pattern 'uorust-x86_64-unknown-linux-gnu.tar.gz' --pattern SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify uorust-x86_64-unknown-linux-gnu.tar.gz --repo afflom/UORust \
  --signer-workflow afflom/UORust/.github/workflows/release.yml
tar -xzf uorust-x86_64-unknown-linux-gnu.tar.gz
./uorust-x86_64-unknown-linux-gnu/uorust --version
```

`gh attestation verify` checks that the archive's SHA-256 is the subject of a
build-provenance attestation signed, through GitHub's OIDC issuer, by this
repository's release workflow. On macOS use `shasum -a 256 --check` in place
of `sha256sum --check`.

## Use it

```sh
cargo run --locked -p uorust -- project path/to/workspace --out path/to/prism-project
cargo run --locked -p uorust -- check   path/to/workspace --out path/to/prism-project
cargo run --locked -p uorust -- routes
```

`project` writes the Prism project, `check` confirms a directory holds exactly
the current projection, and `routes` prints the route table. `project` and
`check` run `cargo metadata --offline` on the workspace (so `cargo` must be on
`PATH`, or named by `CARGO`) and, when `--last-modified` is absent, `git log`
for the HEAD commit time. Check the result
with the locked SDK: `prismpm --project path/to/prism-project check`.

The verified route table also ships on its own. `prismpm --project app build`
writes static `uorust-routes` programs for `x86_64` and `aarch64` Linux beside
the holo, generated from the model with no handwritten dispatch:

```sh
printf 'error-enum' | uorust-routes route              # route UR-R12 ... exit 0
printf 'error-enum\nstate-enum' | uorust-routes route  # refused ambiguous ... exit 1
```

## The route table

[`model/routes.toml`](model/routes.toml) is the one source both
implementations read: the Rust router in `crates/uorust` and the LexLean model
[`app/src/Routes.lex.tex`](app/src/Routes.lex.tex). Both replay the same
acceptance vectors, and the model's theorems, decided by the Lean kernel in
`prismpm verify`, state that every kind routes to its own row, that no two
rows share a kind, and that any two kinds naming different routes are refused.

| Route | Rust fact | Prism record | Facet |
| --- | --- | --- | --- |
| `UR-R01` | `crate-bin-target` | `Foundation.Arch::Component` | arch |
| `UR-R02` | `crate-lib-target` | `Foundation.Arch::Component` | arch |
| `UR-R03` | `crate-doc` | `Foundation.Arch::Concern` | arch |
| `UR-R04` | `package-author` | `Foundation.Arch::Stakeholder` | arch |
| `UR-R05` | `dependency-edge` | `Foundation.Arch::Edge` | arch |
| `UR-R06` | `clap-subcommand` | `Foundation.Arch::Feature` | features |
| `UR-R07` | `pub-fn` | `Feature.calls` | features |
| `UR-R08` | `pub-trait` | `Feature.calls` | features |
| `UR-R09` | `example-target` | `Foundation.Arch::Journey` | features |
| `UR-R10` | `test-fn` | `Feature.acceptance` | features |
| `UR-R11` | `doctest` | `Feature.acceptance` | features |
| `UR-R12` | `error-enum` | `LexLean.inductive` | behavior |
| `UR-R13` | `fn-body` | `LexLean.definition` | behavior |
| `UR-R14` | `unsafe-block` | `Foundation.Sec::Threat` | sec |
| `UR-R15` | `ffi-extern` | `Foundation.Sec::Threat` | sec |
| `UR-R16` | `fs-use` | `Foundation.Sec::Asset` | sec |
| `UR-R17` | `net-use` | `Foundation.Sec::Asset` | sec |
| `UR-R18` | `process-use` | `Foundation.Sec::Asset` | sec |
| `UR-R19` | `env-use` | `Foundation.Sec::Asset` | sec |
| `UR-R20` | `forbid-unsafe` | `Foundation.Sec::ApplicationSecurityControl` | sec |
| `UR-R21` | `advisory` | `Foundation.Sec::Risk` | sec |
| `UR-R22` | `lock-package` | `System.SpdxPackage` | release |
| `UR-R23` | `lint-config` | `Foundation.Qual::QualityRequirement` | qual |
| `UR-R24` | `rust-version` | `Foundation.Qual::QualityRequirement` | qual |
| `UR-R25` | `bench-target` | `Foundation.Qual::QualityMeasure` | qual |
| `UR-R26` | `state-enum` | `Foundation.Arch::UmlState` | design |
| `UR-R27` | `call-edge` | `Foundation.Arch::UmlMessage` | design |
| `UR-R28` | `module` | `Foundation.Arch::OpmThing` | design |

A request is one fact's kinds, one per line. The answer is
`route ID / fact KIND / record RECORD / facet FACET`, or
`refused unknown-fact`, `refused ambiguous`, or `error REASON` for an empty,
oversized, non-UTF-8 or blank-line request.

## UORust's own model

| Facet | Where | Records |
| --- | --- | --- |
| Architecture (ISO/IEC/IEEE 42010) | [`Architecture.lex.tex`](app/src/Architecture.lex.tex) | 5 stakeholders, 7 concerns, 14 components (CLI release binary, ingest, router, admission, emitter, route-table guest, route CLI adapter, Pages static site, and the subject workspace, Cargo, crates.io, rustc, PrismPM and GitHub Actions as external systems), 20 edges, 7 viewpoints, 7 views |
| Design views (OPM, UML) | [`Design.lex.tex`](app/src/Design.lex.tex) | 4 views: the OPM projection process with its zoom, the UML component view, the ingest, route, admit and emit sequence, and the state machine of a fact's admission |
| Features and journeys | [`Features.lex.tex`](app/src/Features.lex.tex) | 10 features, each naming its interface calls and acceptance rows; 5 journeys, one per stakeholder |
| Behavior | [`Routes.lex.tex`](app/src/Routes.lex.tex) | the route table, its dispatch, 7 decide theorems, 46 acceptance vectors, and the request limit derived from the table (the length of a request naming every kind once) |
| Security and threat model (ISO 27034, 27005, STRIDE) | [`Security.lex.tex`](app/src/Security.lex.tex) | 6 assets, 11 threats and risks, 13 controls with gate, proof or review evidence, threat mappings and one accepted risk |
| Quality (ISO/IEC 25010:2023) | [`Quality.lex.tex`](app/src/Quality.lex.tex) | 6 characteristics, 10 subcharacteristics, 12 requirements with measures: route coverage, vector coverage, self-model drift, byte-identical re-runs, run time, request bound, registered tests, Clippy warnings, MSRV, static CLI, no unsafe code, every function lifted. No threshold is chosen: each is counted from the route table, read from the manifest, the model's own request limit, a comparator measurement pinned in [`model/comparators.toml`](model/comparators.toml), or 0 for a count of defects, with an excess theorem where it is measured |
| Release and distribution | [`Release.lex.tex`](app/src/Release.lex.tex) | the release targets of `release.yml` with their archives and whether the release waits for each, the release artifacts (archives, `.sha256` files, `SHA256SUMS`), the distribution channels `install.yml` proves (each with its command, verification and the `release.yml` job it installs from), and a theorem that the release version is the system model's product version; written by `scripts/self-model/release.py`, every record cited with its source span and κ in `Release::sourcesOfRelease` |
| Workbench view | [`Guide.lex.tex`](app/src/Guide.lex.tex), [`Frame.lex.tex`](app/src/Frame.lex.tex) | the page's operations, examples, refusal hints and the route-table guide |

The component, feature, acceptance and evidence text that UORust's code or
register states is read from it by `scripts/self-model/facets.py`, not
restated: each such record is listed with its source path, lines and κ (the
SHA-256 of the span) in `Architecture::sourcesOfComponents`,
`Features::sourcesOfFeatures`, `Security::sourcesOfControls` and
`Quality::sourcesOfMeasures`, so `just self-model-check` fails when a cited
source changes until the model is regenerated.

`prismpm --project app check` checks every facet, including the design views
(PP2012), features and journeys (PP2014 to PP2018) and the threat model (PP2019
to PP2022). The register rows `SM-01` to `SM-04` and their tests in
[`crates/conformance/tests/self_model.rs`](crates/conformance/tests/self_model.rs)
hold the model to the route table and every facet to its records.

## The gate

`just vv` is the acceptance boundary, run in the digest-pinned SDK locally and
in CI. The repository follows the
[UOR Prism template](https://github.com/UOR-Foundation/template): every claim
is a row in [`model/ids.toml`](model/ids.toml), a scenario in
[`features/suites/`](features/suites) and a test named for its ID, and
[`CONFORMANCE.md`](CONFORMANCE.md) is generated from the register.

| Command | What it does |
| --- | --- |
| `just vv` | The whole gate: format, model and audits, the SDK's template and lock checks, Clippy, the tests, `cargo deny`, and the verified build of `app/`. |
| `just app` | Checks `app/` is canonical and locked, then builds and verifies its holo. |
| `just model-write` | Regenerates `CONFORMANCE.md` from `model/`. |
| `just self-model-write` | Regenerates the route table, its vectors and the facet modules of `app/` from `model/routes.toml` and `scripts/self-model/`, then locks, formats and rebinds the System release digest. `just self-model-check` (in `just vv`) fails when a committed file differs from what the generators write. |
| `just new-capability <ID> <level> <suite> "<statement>"` | Registers a capability with its scenario and failing test. |

For the Prism language, the facet metamodel and the SDK, read the
[PrismPM documentation](https://github.com/UOR-Foundation/PrismPM/tree/main/docs/site).

## Repository layout

| Path | Purpose |
| --- | --- |
| `crates/uorust/` | The engine: ingest, router, admission, lift and emitter, and the `uorust` binary. |
| `app/` | UORust's own Prism model, every facet, and its application, the route table. |
| `model/` | The route table and the claim, authority and ledger registers. |
| `features/suites/` | One Gherkin scenario per registered ID. |
| `site/` | The Pages site: `app.json` and the uor.foundation layer in `card.css` over the template frame. |
| `crates/model`, `crates/conformance`, `xtask/` | The template's register parser, meta-gate and repository gates. |
| `AGENTS.md`, `VERIFICATION.md`, `.devcontainer/`, `.github/workflows/bootstrap.yml`, `prismpm.lock`, `template.lock`, `template-contract.json` | Universal policy, byte-bound by `template.lock`. |

[TEMPLATE-CONTRACT.md](TEMPLATE-CONTRACT.md) defines the inherited policy and
the SDK and template update process, and [VERIFICATION.md](VERIFICATION.md)
the acceptance boundary.

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
