"""The attempt ledger of the MIR path's CI workloads: the adapter that hashes.

An attempt is (module, translator κ, toolchain κ), Engine::Attempt in
app/src/Engine.lex.tex; for a crate of crates.io the module is
`<name>@<version>`. A module's dump is fixed by the toolchain and its
translation by the translator, so an attempt the ledger holds is never made
again: Engine::staleModules keeps the attempts the ledger does not hold, and
a tick whose attempts are all held is idle and settles (Engine::settled).
The model decides those rules over its fixtures; this script computes the
two κ and applies the same rule to the ledger file:

- the translator κ is the SHA-256 of every file the translation reads from
  this repository (the translator's sources, its manifest, the lockfile and
  the dump script), each named by its path and its own SHA-256;
- the toolchain κ is the SHA-256 of rust-toolchain.toml and the SDK image
  prismpm.lock selects, which fix the compiler, rust-src and the build.

    python3 -I scripts/mir/ledger.py stale REGISTER MODULES.json
    python3 -I scripts/mir/ledger.py record REGISTER SHARDS_DIR
    python3 -I scripts/mir/ledger.py stale-crates REGISTERS RANKED.json
    python3 -I scripts/mir/ledger.py record-crates REGISTERS CRATES_JSON
    python3 -I scripts/mir/ledger.py mir-key

`mir-key` prints the key a library's MIR dump is kept under, as a CI
artifact: the toolchain κ and the digest of the dump script, the two inputs
that fix the dump's bytes. `just core-mir` fetches the dumps CI made under
the current key, so a lane's census reads the very MIR CI's does.

The ledger of a register is crates/uorust/ledger/<its directory's name>.toml:
core.toml, std.toml, and crates.toml for the crates of crates.io.
"""
import hashlib, json, os, subprocess, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TRANSLATOR = ["crates/uorust/src", "crates/uorust/Cargo.toml", "Cargo.lock", "scripts/mir/dump-lib.sh"]
TOOLCHAIN = ["rust-toolchain.toml"]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def tree_kappa(paths):
    files = subprocess.run(["git", "-C", ROOT, "ls-files", "--", *paths], check=True,
                           capture_output=True, text=True).stdout.split()
    lines = [f"{f}\0{sha(open(os.path.join(ROOT, f), 'rb').read())}" for f in sorted(files)]
    return sha("\n".join(lines).encode())


def toolchain_kappa():
    lock = open(os.path.join(ROOT, "prismpm.lock"), encoding="utf-8").read()
    image = lock.split('"sdk_image":"', 1)[1].split('"', 1)[0]
    return sha(f"{tree_kappa(TOOLCHAIN)}\0{image}".encode())


DUMP = ["scripts/mir/dump-lib.sh"]


def mir_key():
    """The key of a library's MIR dump: (toolchain κ, dump-script digest)."""
    return f"{toolchain_kappa()[:16]}-{tree_kappa(DUMP)[:16]}"


def ledger_path(register):
    """crates/uorust/ledger/<register>.toml: beside the registers, not in
    them, whose every other file is a module's."""
    return os.path.join(ROOT, "crates/uorust/ledger", os.path.basename(os.path.normpath(register)) + ".toml")


def read_ledger(register):
    """The attempts the ledger holds, as (module, translator, toolchain)."""
    path = ledger_path(register)
    if not os.path.exists(path):
        return set()
    import tomllib
    doc = tomllib.loads(open(path, encoding="utf-8").read())
    return {(a["module"], a["translator"], a["toolchain"]) for a in doc.get("attempt", [])}


def write_ledger(register, attempts):
    os.makedirs(os.path.dirname(ledger_path(register)), exist_ok=True)
    rows = "".join(f'[[attempt]]\nmodule = {json.dumps(m)}\ntranslator = "{t}"\ntoolchain = "{c}"\n\n'
                   for m, t, c in sorted(attempts))
    head = ("# The attempt ledger (written by scripts/mir/ledger.py; do not edit): each\n"
            "# (module, translator κ, toolchain κ) the workload translated. An attempt\n"
            "# held here is not made again (Engine::staleModules in app/src/Engine.lex.tex).\n\n")
    open(ledger_path(register), "w", encoding="utf-8").write((head + rows).rstrip("\n") + "\n")


def current(module):
    return (module, tree_kappa(TRANSLATOR), toolchain_kappa())


def stale(attempts, ledger):
    """Engine::staleModules: the attempts the ledger does not hold."""
    return [a for a in attempts if a not in ledger]


def record(register, modules):
    """The ledger with the current attempt of each module, the older
    attempts of those modules replaced."""
    ledger = read_ledger(register)
    done = {m for m in modules}
    kept = {a for a in ledger if a[0] not in done}
    write_ledger(register, kept | {current(m) for m in modules})


cmd = sys.argv[1]
if cmd == "--library":
    pass
elif cmd == "stale":
    register, modules = sys.argv[2], json.load(open(sys.argv[3]))
    print(json.dumps([a[0] for a in stale([current(m) for m in modules], read_ledger(register))]))
elif cmd == "record":
    register, shards = sys.argv[2], sys.argv[3]
    names = sorted(f[:-len(".toml")] for f in os.listdir(shards) if f.endswith(".toml")) if os.path.isdir(shards) else []
    record(register, names)
    print(f"ledger: {len(names)} attempts recorded in {ledger_path(register)}")
elif cmd == "stale-crates":
    registers, ranked = sys.argv[2], json.load(open(sys.argv[3]))
    ledger = read_ledger(registers)
    keep = {a[0] for a in stale([current(f"{c['name']}@{c['version']}") for c in ranked], ledger)}
    print(json.dumps([c for c in ranked if f"{c['name']}@{c['version']}" in keep]))
elif cmd == "record-crates":
    registers, crates = sys.argv[2], json.loads(sys.argv[3])
    lifted = [f"{c['name']}@{c['version']}" for c in crates
              if os.path.exists(os.path.join(registers, c["name"], "frontier.toml"))]
    record(registers, lifted)
    print(f"ledger: {len(lifted)} attempts recorded in {ledger_path(registers)}")
elif cmd == "mir-key":
    print(mir_key())
else:
    sys.exit(f"ledger: unknown command {cmd}")
