"""Move every file that names the workspace version to a new version.

    python3 -I scripts/release/bump.py ROOT NEW_VERSION

The version is decided by the release cadence (app/src/Engine.lex.tex,
nextRelease; scripts/self-model/cadence.py), never chosen here. This writes
the hand-kept places that name it: the workspace and its four path
dependencies in Cargo.toml (a member names an internal dependency with
`workspace = true`, never a version of its own), the app's lakefile, the product, component and
manifest versions of app/src/System.lex.tex, and the examples in README.md,
install.sh and install.ps1. Everything generated from the version (the lock,
the Release, Routes and Security facets, the System digest) is rewritten by
its own writer after this: `cargo update -w` and scripts/self-model/write.sh.
Each place must name the old version exactly as expected, or nothing is
written.
"""
import glob, re, sys

root, new = sys.argv[1], sys.argv[2]
assert re.fullmatch(r"\d+\.\d+\.\d+", new), new
cargo = open(f"{root}/Cargo.toml", encoding="utf-8").read()
old = re.search(r'\[workspace\.package\]\nversion = "([^"]+)"', cargo).group(1)
if old == new:
    sys.exit(f"bump: the workspace is already {new}")

EDITS = {  # file -> [(text naming the old version, how many times)]
    "Cargo.toml": [(f'version = "{old}"', 5)],
    "app/lakefile.toml": [(f'version = "{old}"', 1)],
    "app/src/System.lex.tex": [(f'version := "{old}"', 2), (f'productVersion := "{old}"', 1)],
    "README.md": [(f"--tag v{old} ", 1), (f"such as `{old}`", 1), (f"download v{old} ", 1)],
    "install.sh": [(f"e.g. {old} ", 1)],
    "install.ps1": [(f"e.g. {old} ", 1)],
}
# A member's manifest that names the version itself would be left behind,
# and `cargo update` then cannot resolve it.
for manifest in sorted(glob.glob(f"{root}/crates/*/Cargo.toml") + [f"{root}/xtask/Cargo.toml"]):
    assert f'"{old}"' not in open(manifest, encoding="utf-8").read(), (
        f"bump: {manifest} names {old}; name the dependency with `workspace = true`"
    )
texts = {}
for path, edits in EDITS.items():
    text = open(f"{root}/{path}", encoding="utf-8").read()
    for needle, count in edits:
        found = text.count(needle)
        assert found == count, f"bump: {path} names `{needle}` {found} times, expected {count}"
        text = text.replace(needle, needle.replace(old, new))
    texts[path] = text
for path, text in texts.items():
    open(f"{root}/{path}", "w", encoding="utf-8").write(text)
print(f"bump: {old} -> {new}")
