"""The release cadence: UORust's releases, each decided by the model.

model/cadence.toml lists every release since the cadence began: its version,
and the observation that cut it (main green, core's translated bodies, the
translator functions the self-register lifts, core's bodies). The rule is
Engine::nextRelease (app/src/Engine.lex.tex): cut only on a green main, only
when core's translated count or the translator's certified count grew since
the last release, a minor while core is incomplete and 1.0.0 when it is
complete, and one release per growth (its theorems aRedMainNeverCuts,
theVersionNeverFalls, theCountsNeverFall, oneReleasePerGrowth).

    python3 -I scripts/self-model/cadence.py ROOT [--check]
        write app/src/Cadence.lex.tex from model/cadence.toml (check.sh
        compares it, formatted, with the committed one), or with --check
        fail when the newest release is not the workspace version;
    python3 -I scripts/self-model/cadence.py ROOT --propose GREEN
        observe main (GREEN is 1 when its gate passed), apply the rule and,
        when it cuts, append the release to model/cadence.toml and print its
        version; print nothing when it does not.

This script only proposes. Cadence.lex.tex states each release as a theorem
decided by the kernel, `sameRelease (nextRelease previous observed) release`,
so a release this script appended that the rule does not make fails `lexlean
verify`, and nothing is cut.
"""
import re, sys, tomllib

root = sys.argv[1]
CADENCE = f"{root}/model/cadence.toml"
MODULE = f"{root}/app/src/Cadence.lex.tex"
TRANSLATOR = ("uorust::mir::", "uorust::lift::", "uorust::lex::")


def load():
    return tomllib.loads(open(CADENCE, encoding="utf-8").read())["release"]


def triple(version):
    return [int(x) for x in version.split(".")]


def state(row):
    major, minor, patch = triple(row["version"])
    return dict(major=major, minor=minor, patch=patch, translated=row["translated"], certified=row["certified"])


def observe(green):
    core = tomllib.loads(open(f"{root}/crates/uorust/core/frontier.toml", encoding="utf-8").read())
    own = tomllib.loads(open(f"{root}/frontier.toml", encoding="utf-8").read())
    certified = sum(1 for r in own.get("lifted", []) if r["function"].startswith(TRANSLATOR))
    return dict(green=green, translated=core["translated"], certified=certified, coreBodies=core["bodies"])


def next_release(last, now):
    """Engine::nextRelease, mirrored; the kernel decides each use of it."""
    grew = last["translated"] < now["translated"] or last["certified"] < now["certified"]
    if not (now["green"] == 1 and grew):
        return last
    promote = now["coreBodies"] <= now["translated"] and last["major"] == 0
    return dict(major=1 if promote else last["major"], minor=0 if promote else last["minor"] + 1, patch=0,
                translated=max(last["translated"], now["translated"]),
                certified=max(last["certified"], now["certified"]))


def rec_state(s):
    return ("(record Engine::ReleaseState {" + ", ".join(f"{k} := {s[k]}" for k in
            ("major", "minor", "patch", "translated", "certified")) + "})")


def rec_obs(o):
    return ("(record Engine::ReleaseObservation {" + ", ".join(f"{k} := {o[k]}" for k in
            ("green", "translated", "certified", "coreBodies")) + "})")


def render(rows):
    out = ("\\begin{lexlean}{Cadence}\n\\useglossary{lexlean.std.bool@1.2.0}\n\\useglossary{lexlean.std.nat@1.2.0}\n"
           "\\importmodule{Engine}\n\\title{Boolean}\n\\begin{semanticmodule}\n")
    names = []
    for i, row in enumerate(rows):
        name = "release" + row["version"].replace(".", "x")
        names.append(name)
        out += (f"definition{{\n  name: {name},\n  parameters: [],\n  result: Engine::ReleaseState,\n"
                f"  body: {rec_state(state(row))}\n}}\n\n")
        if i == 0:
            continue
        o = dict(green=row["green"], translated=row["observed_translated"],
                 certified=row["observed_certified"], coreBodies=row["core_bodies"])
        out += (f"theorem{{\n  name: {name}IsDecided,\n  parameters: [],\n"
                f"  statement: ((Engine::sameRelease (Engine::nextRelease ({names[i - 1]}) {rec_obs(o)}) ({name})) = true),\n"
                f"  proof: decide{{}}\n}}\n\n")
    lst = "(nil Engine::ReleaseState)"
    for name in reversed(names):
        lst = f"(cons ({name}) {lst})"
    out += f"definition{{\n  name: releases,\n  parameters: [],\n  result: (List Engine::ReleaseState),\n  body: {lst}\n}}\n"
    return out + "\\end{semanticmodule}\n\\end{lexlean}\n"


def workspace_version():
    cargo = open(f"{root}/Cargo.toml", encoding="utf-8").read()
    return re.search(r'\[workspace\.package\]\nversion = "([^"]+)"', cargo).group(1)


args = sys.argv[2:]
if args and args[0] == "--propose":
    rows = load()
    last = state(rows[-1])
    now = observe(int(args[1]))
    nxt = next_release(last, now)
    if nxt == last:
        sys.exit(0)
    version = f"{nxt['major']}.{nxt['minor']}.{nxt['patch']}"
    with open(CADENCE, "a", encoding="utf-8") as f:
        f.write(f'\n[[release]]\nversion = "{version}"\ntranslated = {nxt["translated"]}\n'
                f'certified = {nxt["certified"]}\ngreen = {now["green"]}\n'
                f'observed_translated = {now["translated"]}\nobserved_certified = {now["certified"]}\n'
                f'core_bodies = {now["coreBodies"]}\n')
    print(version)
    sys.exit(0)

rows = load()
text = render(rows)
if args and args[0] == "--check":
    # check.sh renders the module itself and compares it once formatted.
    bad = []
    if rows[-1]["version"] != workspace_version():
        bad.append(f"the newest release of model/cadence.toml is {rows[-1]['version']}, the workspace is {workspace_version()}")
    for b in bad:
        print(f"cadence: {b}; run just self-model-write", file=sys.stderr)
    sys.exit(1 if bad else 0)
open(MODULE, "w", encoding="utf-8").write(text)
print(f"cadence: {len(rows)} releases")
