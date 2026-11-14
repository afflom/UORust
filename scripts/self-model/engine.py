"""Render the MIR path's CI workloads from the engine model, or refuse drift.

app/src/Engine.lex.tex holds, as LexLean data the kernel decides about, every
choice the core, std and crates workflows make: each workload's file, its
daily slot, its concurrency group, the stages a run goes through with their
budgets and which one writes, the runner, the one secret, the actions and
the commits they are pinned to, the paths whose change starts a run, and how
long each artifact is kept. Its theorems decide the invariants (only
GITHUB_TOKEN is read, the slots are distinct and in range, every budget fits
GitHub's job cap, each workload has exactly one writing stage). This script
reads those definitions and writes the workflows from them; nothing here
chooses a value. With --check it writes nothing and fails when a committed
workflow is not what the model renders.

    python3 -I scripts/self-model/engine.py ROOT [--check]
"""
import re, sys

root = sys.argv[1]
check = "--check" in sys.argv[2:]
MODEL = f"{root}/app/src/Engine.lex.tex"


# ---------------------------------------------------------------- the model
def definitions(text):
    """name -> body text of every definition with no parameters."""
    out = {}
    for m in re.finditer(r"definition\{\s*name: (\w+),\s*parameters: \[\],\s*result: ", text):
        at = text.index("body: ", m.end()) + len("body: ")
        depth, i = 0, at
        while True:
            c = text[i]
            if c == '"':
                i = text.index('"', i + 1)
            elif c in "({":
                depth += 1
            elif c in ")}":
                depth -= 1
            if depth == 0 and (c in ")}" or text[i + 1] in ",\n"):
                break
            i += 1
        out[m.group(1)] = text[at:i + 1].strip()
    return out


TOKEN = re.compile(r'\s*("(?:[^"\\]|\\.)*"|\(|\)|\{|\}|:=|,|[^\s(){},]+)')


def parse(src):
    """A LexLean data term: a string, a Nat, a Bool, a list or a record."""
    toks = [t for t in TOKEN.findall(src)]
    pos = 0

    def term():
        nonlocal pos
        t = toks[pos]
        pos += 1
        if t.startswith('"'):
            return t[1:-1].replace('\\"', '"').replace("\\\\", "\\")
        if t.isdigit():
            return int(t)
        if t in ("true", "false"):
            return t == "true"
        assert t == "(", f"unexpected {t}"
        head = toks[pos]
        pos += 1
        if head == "nil":
            pos += 1  # the type
            v = []
        elif head == "cons":
            first = term()
            v = [first] + term()
        elif head == "record":
            pos += 1  # the structure
            assert toks[pos] == "{"
            pos += 1
            v = {}
            while toks[pos] != "}":
                k = toks[pos]
                assert toks[pos + 1] == ":="
                pos += 2
                v[k] = term()
                if toks[pos] == ",":
                    pos += 1
            pos += 1
        else:
            raise AssertionError(f"not data: {head}")
        assert toks[pos] == ")", f"expected ) at {toks[pos]}"
        pos += 1
        return v

    return term()


defs = definitions(open(MODEL, encoding="utf-8").read())
data = {k: parse(v) for k, v in defs.items()
        if k in ("runner", "secrets", "workloads", "stages", "writeScopes", "pushPaths", "actions", "retentions",
                 "ledgerFixture", "idleAttemptsFixture", "movedAttemptsFixture")}

# The ledger's adapter (scripts/mir/ledger.py) applies Engine::staleModules;
# on the model's own fixtures it must answer what the model's theorems
# decide (anIdleTickRetriesNothing, aNewTranslatorRetriesEveryModule).
sys.path.insert(0, f"{root}/scripts/mir")
_argv, sys.argv = sys.argv, ["ledger.py", "--library"]
import ledger  # noqa: E402
sys.argv = _argv
def _attempts(name):
    return [(a["module"], a["translator"], a["toolchain"]) for a in data[name]]
for fixture, decided in [("idleAttemptsFixture", []), ("movedAttemptsFixture", ["bool", "char"])]:
    got = [a[0] for a in ledger.stale(_attempts(fixture), set(_attempts("ledgerFixture")))]
    assert got == decided, f"ledger.py's stale rule answers {got} on {fixture}; the model decides {decided}"
RUNNER = data["runner"]
USES = {a["name"]: f'{a["name"]}@{a["revision"]}' for a in data["actions"]}
KEEP = {r["artifact"]: r["days"] for r in data["retentions"]}
WRITES = data["writeScopes"]


def stage(workload, sid):
    rows = [s for s in data["stages"] if s["workload"] == workload and s["id"] == sid]
    assert len(rows) == 1, f"{workload}/{sid}: {len(rows)} stages"
    return rows[0]


def stage_ids(workload):
    return [s["id"] for s in data["stages"] if s["workload"] == workload]


def writer_permissions():
    return "".join(f"      {scope}: write\n" for scope in WRITES)


def checkout():
    return f"""      - name: Check out the exact event revision
        uses: {USES['actions/checkout']}
        with:
          fetch-depth: 0
          persist-credentials: false
"""


def triggers(w):
    out = "on:\n"
    if w["onPush"]:
        paths = data["pushPaths"] + [w["file"]]
        out += "  push:\n    branches: [main]\n    paths:\n" + "".join(f"      - '{p}'\n" for p in paths)
    out += f"  schedule:\n    - cron: '{w['cronMinute']} {w['cronHour']} * * *'\n"
    return out


HEADER = "# Rendered from app/src/Engine.lex.tex by scripts/self-model/engine.py; do not edit.\n"


# ---------------------------------------------------------------- core, std
def library(w):
    krate = w["id"]
    first, shard, merge = stage_ids(krate)
    return HEADER + f"""# Keeps the {krate} register current: every body of Rust's `{krate}`, at the
# rust-src of the pinned toolchain, translated by the MIR path and recorded
# by κ (crates/uorust/{krate}). The run is sharded by {krate}'s own top-level
# modules, read from library/{krate}/src by the first stage
# (scripts/mir/dump-lib.sh {krate} --modules), never listed here; the attempt
# ledger (scripts/mir/ledger.py, Engine::staleModules) keeps only the modules
# whose (module, translator κ, toolchain κ) it has not seen, so a tick with
# nothing new is idle and settles. The first stage dumps every module's MIR
# in one build; each shard keeps its module's dump as an artifact keyed by
# (toolchain κ, dump-script digest), which `just core-mir` fetches, translates
# its module with the uorust the first stage built, linked against every
# module's bodies as a whole census is (`--only`), and refuses a body the
# register records as translated that is now residue. The writing stage
# merges the shards' module files (`uorust merge`, the same monotonicity over
# the whole register), records the attempts, pushes the register and
# dispatches the generate workflow, so the Quality facet's cause queue
# follows.
name: {krate}

{triggers(w)}  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: {w['group']}
  cancel-in-progress: {'true' if w['cancel'] else 'false'}

env:
  CRATE: {krate}

jobs:
  {first}:
    name: build uorust and find {krate}'s stale modules
    runs-on: {RUNNER}
    timeout-minutes: {stage(krate, first)['budget']}
    outputs:
      modules: ${{{{ steps.modules.outputs.modules }}}}
    steps:
{checkout()}      - name: Build uorust and list the modules through the locked SDK
        shell: bash
        run: |
          # The SDK's shell sees none of this job's environment: the crate is
          # expanded here.
          scripts/ci/sdk.sh "cargo build --locked -p uorust && mkdir -p target/bin && cp target/debug/uorust target/bin/ && scripts/mir/dump-lib.sh $CRATE --modules >target/modules.json"
      - name: Keep the modules the ledger has not attempted
        id: modules
        shell: bash
        run: |
          stale=$(python3 -I scripts/mir/ledger.py stale "crates/uorust/$CRATE" target/modules.json)
          echo "modules=$stale" >>"$GITHUB_OUTPUT"
          echo "$CRATE: stale module shards $stale" >>"$GITHUB_STEP_SUMMARY"
      # Every module, in one build: a shard links its module against all of
      # them, so its rows are a whole census's.
      - name: Dump every module through the locked SDK
        if: ${{{{ steps.modules.outputs.modules != '[]' }}}}
        shell: bash
        run: scripts/ci/sdk.sh "scripts/mir/dump-lib.sh $CRATE target/mir"
      - name: Keep every module's MIR for the shards
        if: ${{{{ steps.modules.outputs.modules != '[]' }}}}
        uses: {USES['actions/upload-artifact']}
        with:
          name: ${{{{ env.CRATE }}}}-mir-all
          path: target/mir
          retention-days: {KEEP['uorust']}
      - name: Keep the built uorust for the shards
        uses: {USES['actions/upload-artifact']}
        with:
          name: uorust-${{{{ env.CRATE }}}}
          path: target/bin/uorust
          retention-days: {KEEP['uorust']}

  {shard}:
    name: translate ${{{{ matrix.module }}}}
    needs: {first}
    if: ${{{{ needs.{first}.outputs.modules != '[]' }}}}
    runs-on: {RUNNER}
    timeout-minutes: {stage(krate, shard)['budget']}
    strategy:
      fail-fast: false
      matrix:
        module: ${{{{ fromJSON(needs.{first}.outputs.modules) }}}}
    steps:
{checkout()}      - name: Fetch the built uorust
        uses: {USES['actions/download-artifact']}
        with:
          name: uorust-${{{{ env.CRATE }}}}
          path: target/bin
      - name: Fetch every module's MIR
        uses: {USES['actions/download-artifact']}
        with:
          name: ${{{{ env.CRATE }}}}-mir-all
          path: target/mir
      - name: Key the module's MIR
        id: dump
        shell: bash
        run: echo "key=$(python3 -I scripts/mir/ledger.py mir-key)" >>"$GITHUB_OUTPUT"
      # The dump is kept whether or not the translation refuses it: it is the
      # MIR `just core-mir` fetches, keyed by (toolchain κ, dump-script
      # digest), so a lane's census reads the bytes CI's reads.
      - name: Keep the module's MIR under its key
        uses: {USES['actions/upload-artifact']}
        with:
          name: ${{{{ env.CRATE }}}}-mir-${{{{ steps.dump.outputs.key }}}}-${{{{ matrix.module }}}}
          path: |
            target/mir/target.toml
            target/mir/layouts.json
            target/mir/${{{{ matrix.module }}}}
          retention-days: {KEEP['mir']}
      - name: Translate the module through the locked SDK
        shell: bash
        env:
          MODULE: ${{{{ matrix.module }}}}
        run: |
          chmod +x target/bin/uorust
          channel=$(sed -n 's/^channel = "\\(.*\\)"/\\1/p' rust-toolchain.toml)
          # The sources the declarations are read from: dump-lib.sh fetches
          # the pinned rust-src (hash-checked) before it lists the modules.
          scripts/ci/sdk.sh "scripts/mir/dump-lib.sh $CRATE --modules >/dev/null && target/bin/uorust $CRATE --mir target/mir --only '$MODULE' --library target/rust-src-$channel/library --register crates/uorust/$CRATE"
          mkdir -p target/shard
          cp "crates/uorust/$CRATE/$MODULE.toml" target/shard/
      # The deep embedding (UE-19): every body of the module as a value of
      # the MIR datatype, which `lexlean verify` elaborates with the
      # evaluator, and the differential check: the module's sampled
      # functions run by rustc's build of core (scripts/mir/harness.sh) and
      # by the evaluator on their embedded bodies, a theorem per case that
      # the two agree. A disagreement fails verify, naming the theorem,
      # which differential.toml maps to its body.
      - name: Embed the module and check the embedding through the locked SDK
        shell: bash
        env:
          MODULE: ${{{{ matrix.module }}}}
        run: |
          channel=$(sed -n 's/^channel = "\\(.*\\)"/\\1/p' rust-toolchain.toml)
          embed="target/bin/uorust embed --crate $CRATE --mir target/mir --only '$MODULE' --library target/rust-src-$channel/library --out target/embed"
          scripts/ci/sdk.sh "$embed --harness target/harness"
          if [ -d target/harness ]; then
            scripts/ci/sdk.sh "scripts/mir/harness.sh target/harness target/observed.txt && $embed --observed target/observed.txt"
          fi
          scripts/ci/sdk.sh "cd target/embed && lexlean lock && lexlean verify"
          sed -n '/^bodies/,/^$/p' target/embed/census.toml >>"$GITHUB_STEP_SUMMARY"
      - name: Keep the module's register file
        uses: {USES['actions/upload-artifact']}
        with:
          name: ${{{{ env.CRATE }}}}-register-${{{{ matrix.module }}}}
          path: target/shard
          retention-days: {KEEP['register']}

  {merge}:
    name: merge the {krate} register and push it
    needs: [{first}, {shard}]
    if: ${{{{ always() && needs.{first}.result == 'success' && needs.{first}.outputs.modules != '[]' }}}}
    runs-on: {RUNNER}
    timeout-minutes: {stage(krate, merge)['budget']}
    permissions:
{writer_permissions()}    steps:
{checkout()}      # A partial set of shards writes no row: a shard that failed may
      # have left others translating against what it lacked, so the
      # register is merged and pushed only when every shard succeeded.
      - name: Refuse a partial set of shards
        if: ${{{{ needs.{shard}.result != 'success' }}}}
        shell: bash
        run: |
          echo "a shard of $CRATE did not succeed; the register is left as it records" >&2
          exit 1
      - name: Fetch the built uorust
        uses: {USES['actions/download-artifact']}
        with:
          name: uorust-${{{{ env.CRATE }}}}
          path: target/bin
      - name: Fetch every shard's module file
        uses: {USES['actions/download-artifact']}
        with:
          pattern: ${{{{ env.CRATE }}}}-register-*
          merge-multiple: true
          path: target/shards
      - name: Merge the shards into the register through the locked SDK
        shell: bash
        run: |
          chmod +x target/bin/uorust
          mkdir -p target/shards
          scripts/ci/sdk.sh "target/bin/uorust merge --crate $CRATE --shards target/shards --register crates/uorust/$CRATE"
      - name: Record the attempts the shards made
        shell: bash
        run: python3 -I scripts/mir/ledger.py record "crates/uorust/$CRATE" target/shards
      - name: Publish the register's counts and ranked causes
        if: always()
        shell: bash
        run: |
          {{
            echo "## The $CRATE register"
            echo
            echo '```'
            cat "crates/uorust/$CRATE/frontier.toml" 2>/dev/null || echo "no register yet"
            echo '```'
          }} >>"$GITHUB_STEP_SUMMARY"
      - name: Push the merged register and regenerate the model from it
        shell: bash
        env:
          GH_TOKEN: ${{{{ github.token }}}}
          REBASE: '1'
        run: scripts/ci/push.sh "Regenerate the $CRATE register" "crates/uorust/$CRATE crates/uorust/ledger" generate.yml
"""


# ---------------------------------------------------------------- crates
def crates(w):
    wid = w["id"]
    first, shard, merge = stage_ids(wid)
    return HEADER + f"""# Keeps a register for each of crates.io's most-downloaded crates, so the MIR
# path grows against the code Rust developers depend on, as it does against
# core and std. The first stage reads the ranking from crates.io's own API
# (scripts/mir/top-crates.py, `sort=downloads`), each crate at its newest
# stable version; how many crates is the `count` input, and when it is not
# given (the daily run), one more than crates/uorust/crates holds registers
# for, so each run takes on the next crate of the ranking and none is chosen
# here. The attempt ledger keeps only the crates whose (crate@version,
# translator κ, toolchain κ) it has not seen. Each shard runs `uorust crate
# <name>@<version>`: it fetches the crate, checks it against crates.io's
# checksum, dumps its library's MIR on the pinned toolchain, translates every
# body and refuses a body the register records as translated that is now
# residue. The writing stage copies each crate's register into
# crates/uorust/crates/<name>, records the attempts, pushes and dispatches
# the generate workflow. A crate that does not build here (one that needs a
# system library, say) is reported in the summary and retried next tick.
name: {wid}

{triggers(w)}  workflow_dispatch:
    inputs:
      count:
        description: How many of crates.io's most-downloaded crates; empty for one more than the registers held
        required: false
        default: ''

permissions:
  contents: read

concurrency:
  group: {w['group']}
  cancel-in-progress: {'true' if w['cancel'] else 'false'}

jobs:
  {first}:
    name: build uorust and rank the crates
    runs-on: {RUNNER}
    timeout-minutes: {stage(wid, first)['budget']}
    outputs:
      crates: ${{{{ steps.crates.outputs.crates }}}}
    steps:
{checkout()}      - name: Rank the crates by downloads and keep those the ledger has not attempted
        id: crates
        shell: bash
        env:
          COUNT: ${{{{ inputs.count }}}}
        run: |
          python3 -I scripts/mir/top-crates.py crates/uorust/crates "$COUNT" >"$RUNNER_TEMP/ranked.json"
          crates=$(python3 -I scripts/mir/ledger.py stale-crates crates/uorust/crates "$RUNNER_TEMP/ranked.json")
          echo "crates=$crates" >>"$GITHUB_OUTPUT"
          echo "crates: $crates" >>"$GITHUB_STEP_SUMMARY"
      - name: Build uorust through the locked SDK
        shell: bash
        run: scripts/ci/sdk.sh 'cargo build --locked -p uorust && mkdir -p target/bin && cp target/debug/uorust target/bin/'
      - name: Keep the built uorust for the shards
        uses: {USES['actions/upload-artifact']}
        with:
          name: uorust-crates
          path: target/bin/uorust
          retention-days: {KEEP['uorust']}

  {shard}:
    name: lift ${{{{ matrix.crate.name }}}}@${{{{ matrix.crate.version }}}}
    needs: {first}
    if: ${{{{ needs.{first}.outputs.crates != '[]' }}}}
    runs-on: {RUNNER}
    timeout-minutes: {stage(wid, shard)['budget']}
    strategy:
      fail-fast: false
      matrix:
        crate: ${{{{ fromJSON(needs.{first}.outputs.crates) }}}}
    steps:
{checkout()}      - name: Fetch the built uorust
        uses: {USES['actions/download-artifact']}
        with:
          name: uorust-crates
          path: target/bin
      - name: Fetch, dump and translate the crate through the locked SDK
        shell: bash
        env:
          SPEC: ${{{{ matrix.crate.name }}}}@${{{{ matrix.crate.version }}}}
          NAME: ${{{{ matrix.crate.name }}}}
        run: |
          chmod +x target/bin/uorust
          mkdir -p target/tmp
          scripts/ci/sdk.sh "TMPDIR=$PWD/target/tmp target/bin/uorust crate '$SPEC'"
          mkdir -p "target/register/$NAME"
          cp -r "crates/uorust/crates/$NAME/." "target/register/$NAME/"
      - name: Keep the crate's register
        uses: {USES['actions/upload-artifact']}
        with:
          name: crate-register-${{{{ matrix.crate.name }}}}
          path: target/register
          retention-days: {KEEP['register']}

  {merge}:
    name: push the crates' registers
    needs: [{first}, {shard}]
    if: ${{{{ always() && needs.{first}.result == 'success' && needs.{first}.outputs.crates != '[]' }}}}
    runs-on: {RUNNER}
    timeout-minutes: {stage(wid, merge)['budget']}
    permissions:
{writer_permissions()}    steps:
{checkout()}      - name: Fetch every crate's register
        uses: {USES['actions/download-artifact']}
        with:
          pattern: crate-register-*
          merge-multiple: true
          path: target/registers
      - name: Replace each lifted crate's register and record its attempt
        shell: bash
        env:
          CRATES: ${{{{ needs.{first}.outputs.crates }}}}
        run: |
          mkdir -p crates/uorust/crates target/registers
          {{
            echo '## Crate registers'
            echo
          }} >>"$GITHUB_STEP_SUMMARY"
          for name in $(python3 -I -c 'import json, sys; print(" ".join(c["name"] for c in json.loads(sys.argv[1])))' "$CRATES"); do
            if [ -f "target/registers/$name/frontier.toml" ]; then
              rm -rf "crates/uorust/crates/$name"
              cp -r "target/registers/$name" "crates/uorust/crates/$name"
              echo "- $name: $(sed -n 's/^translated = //p' "crates/uorust/crates/$name/frontier.toml") of $(sed -n 's/^bodies = //p' "crates/uorust/crates/$name/frontier.toml") bodies translated" >>"$GITHUB_STEP_SUMMARY"
            else
              echo "- $name: not lifted in this run (see its shard)" >>"$GITHUB_STEP_SUMMARY"
            fi
          done
          python3 -I scripts/mir/ledger.py record-crates crates/uorust/crates "$CRATES"
      - name: Push the registers and regenerate the model from them
        shell: bash
        env:
          GH_TOKEN: ${{{{ github.token }}}}
          REBASE: '1'
        run: scripts/ci/push.sh "Regenerate the crates' registers" "crates/uorust/crates crates/uorust/ledger" generate.yml
"""


# ---------------------------------------------------------------- release
def release(w):
    wid = w["id"]
    (cut,) = stage_ids(wid)
    return HEADER + f"""# Cuts UORust's releases on the cadence the model decides. When the gate
# (bootstrap) passes on main, or daily, or on demand, this observes main:
# whether its gate passed, core's translated bodies (crates/uorust/core) and
# the translator functions the self-register lifts (frontier.toml). The rule
# is Engine::nextRelease in app/src/Engine.lex.tex: cut only on green, only
# when a count grew since the last release, a minor while core is
# incomplete, one release per growth. scripts/self-model/cadence.py applies
# it and appends the release to model/cadence.toml; then, through the locked
# SDK, scripts/release/bump.py moves every file that names the version, the
# lock and every generated file are rewritten (scripts/self-model/write.sh),
# and `lexlean verify` must decide the new release from the rule
# (app/src/Cadence.lex.tex) before anything is pushed. Green is observed on
# the commit the gate passed (the bootstrap run that triggered this one, or
# on a schedule the newest green main), never on main's head, which under a
# stream of pushes is almost always still being gated; the counts are read
# at that commit. The release commit (the version, the lock, cadence.toml) is
# rebased onto main as it moved and dispatches generate.yml, which rewrites
# the self model on top and dispatches the gate; when it passes on the new
# version, release-tag.yml tags it and release.yml builds it.
name: {wid}

on:
  workflow_run:
    workflows: [bootstrap]
    types: [completed]
    branches: [main]
  schedule:
    - cron: '{w['cronMinute']} {w['cronHour']} * * *'
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: {w['group']}
  cancel-in-progress: {'true' if w['cancel'] else 'false'}

jobs:
  {cut}:
    name: cut a release when the model decides one
    runs-on: {RUNNER}
    timeout-minutes: {stage(wid, cut)['budget']}
    permissions:
{writer_permissions()}    steps:
{checkout().replace("          persist-credentials: false", "          persist-credentials: false" + chr(10) + "          ref: main")}      - name: Observe main and apply the cadence
        id: cadence
        shell: bash
        env:
          GH_TOKEN: ${{{{ github.token }}}}
          REPOSITORY: ${{{{ github.repository }}}}
          EVENT: ${{{{ github.event_name }}}}
          RUN_SHA: ${{{{ github.event.workflow_run.head_sha }}}}
          RUN_CONCLUSION: ${{{{ github.event.workflow_run.conclusion }}}}
        run: |
          set -euo pipefail
          # Green is the gate's verdict on one exact commit, never a run's exit:
          # the commit the triggering bootstrap run gated, or the newest main
          # commit the gate passed.
          if [[ $EVENT == workflow_run ]]; then
            sha=$RUN_SHA
            conclusion=$RUN_CONCLUSION
          else
            sha=$(gh run list --repo "$REPOSITORY" --workflow bootstrap.yml --branch main --status success --limit 1 --json headSha --jq '.[0].headSha // ""')
            conclusion=success
            if [[ -z $sha ]]; then sha=$(git rev-parse HEAD); conclusion=""; fi
          fi
          green=0
          if [[ $conclusion == success ]] && git merge-base --is-ancestor "$sha" HEAD; then green=1; fi
          # The counts are those of the green commit; the release is appended
          # to main's cadence.
          observed="$RUNNER_TEMP/observed"
          git worktree add -q --detach "$observed" "$sha"
          cp model/cadence.toml "$observed/model/cadence.toml"
          version=$(python3 -I scripts/self-model/cadence.py "$observed" --propose "$green")
          cp "$observed/model/cadence.toml" model/cadence.toml
          git worktree remove --force "$observed"
          echo "version=$version" >>"$GITHUB_OUTPUT"
          echo "observed $sha: gate ${{conclusion:-pending}}; release ${{version:-none}}" | tee -a "$GITHUB_STEP_SUMMARY"
      - name: Move the version, rewrite what follows from it, and decide the release
        if: steps.cadence.outputs.version != ''
        shell: bash
        env:
          VERSION: ${{{{ steps.cadence.outputs.version }}}}
        run: scripts/ci/sdk.sh "python3 -I scripts/release/bump.py . $VERSION && cargo update -q -w --offline && bash scripts/self-model/write.sh && cd app && lexlean verify src/Engine.lex.tex src/Cadence.lex.tex"
      - name: Push the release and dispatch generate, which dispatches its gate
        if: steps.cadence.outputs.version != ''
        shell: bash
        env:
          GH_TOKEN: ${{{{ github.token }}}}
          VERSION: ${{{{ steps.cadence.outputs.version }}}}
          REBASE: any
        run: |
          set -euo pipefail
          paths="$(python3 -I scripts/release/bump.py --paths) Cargo.lock model/cadence.toml"
          # The self model the decision rewrote is generate's to commit on
          # top of main as it is; set it aside so the release rebases cleanly.
          git add -- $paths
          git stash push -q --keep-index --include-untracked || true
          scripts/ci/push.sh "Release $VERSION" "$paths" generate.yml
"""


RENDER = {"core": library, "std": library, "crates": crates, "release": release}
drift = []
for w in data["workloads"]:
    text = RENDER[w["id"]](w)
    path = f"{root}/{w['file']}"
    if check:
        try:
            now = open(path, encoding="utf-8").read()
        except FileNotFoundError:
            now = None
        if now != text:
            drift.append(w["file"])
    else:
        open(path, "w", encoding="utf-8").write(text)
if drift:
    for f in drift:
        print(f"engine: {f} is not what app/src/Engine.lex.tex renders; run just self-model-write", file=sys.stderr)
    sys.exit(1)
print(f"engine: {len(data['workloads'])} workloads, {len(data['stages'])} stages")
