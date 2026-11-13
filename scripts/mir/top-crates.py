"""The crates of crates.io the crates workflow lifts, most downloaded first,
each at its newest stable version, as the JSON matrix the workflow shards
over: [{"name": ..., "version": ...}, ...].

    python3 -I scripts/mir/top-crates.py REGISTERS [COUNT]

COUNT is how many crates; when it is empty, one more than REGISTERS (the
directory of the crates' registers) holds, so that each run takes the next
crate of the ranking. The ranking is crates.io's own (`sort=downloads`), read
page by page as the API links them, at its own page size.
"""
import json, os, sys, urllib.request

registers = sys.argv[1]
given = sys.argv[2] if len(sys.argv) > 2 else ""
held = [d for d in (os.listdir(registers) if os.path.isdir(registers) else [])
        if os.path.isfile(os.path.join(registers, d, "frontier.toml"))]
count = int(given) if given.strip() else len(held) + 1
API = "https://crates.io/api/v1/crates"
# crates.io asks automated clients to name themselves.
HEADERS = {"User-Agent": "uorust-register", "Accept": "application/json"}

out = []
query = "?sort=downloads"
while query and len(out) < count:
    with urllib.request.urlopen(urllib.request.Request(API + query, headers=HEADERS)) as r:
        page = json.load(r)
    for c in page["crates"]:
        version = c.get("max_stable_version") or c.get("newest_version")
        if version and len(out) < count:
            out.append({"name": c["id"], "version": version})
    query = page.get("meta", {}).get("next_page")
print(json.dumps(out))
