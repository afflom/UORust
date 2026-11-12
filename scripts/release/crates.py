"""Print the workspace's publishable crates, each after the crates it depends on.

    cargo metadata --no-deps --format-version 1 | python3 -I scripts/release/crates.py

A crate is publishable unless its manifest says `publish = false`. The order is
the one `cargo publish` needs: a crate's workspace dependencies (normal and
build) are on crates.io before it. A publishable crate that depends on an
unpublishable one is refused here, before a registry would refuse it.
"""
import json, sys

meta = json.load(sys.stdin)
packages = {p["name"]: p for p in meta["packages"]}
publishable = {n for n, p in packages.items() if p.get("publish") is None or p["publish"]}

def needs(name):
    return sorted({d["name"] for d in packages[name]["dependencies"]
                   if d["name"] in packages and d.get("kind") in (None, "build")})

order, seen = [], set()
def visit(name, path=()):
    assert name not in path, f"dependency cycle: {' -> '.join(path + (name,))}"
    if name in seen:
        return
    for dep in needs(name):
        assert dep in publishable, f"{name} depends on {dep}, which is publish = false"
        visit(dep, path + (name,))
    seen.add(name)
    order.append(name)

for name in sorted(publishable):
    visit(name)
print("\n".join(order))
