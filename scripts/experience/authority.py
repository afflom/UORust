"""Copy the brand authority, uor.foundation, into brand/foundation/ and lock it.

The brand and design facets are read from the UOR Foundation website
(UOR-Foundation/website), never restated: its page head, its design tokens and
its copy. This keeps an exact copy of the files they are read from, pinned by
commit and SHA-256 in brand/foundation.lock, the way brand.lock pins the
Hologram kit, so the facets regenerate and the gate checks them offline.

    python3 -I authority.py ROOT --check           the copies match the lock (no network)
    python3 -I authority.py ROOT --pin COMMIT      copy the files at COMMIT and lock them
    python3 -I authority.py ROOT --follow          pin the website's newest main commit
"""
import hashlib, json, pathlib, subprocess, sys, tempfile

REPOSITORY = "UOR-Foundation/website"
SPEC = "prism/brand-authority/1"
# What each facet reads, and nothing else: a file the facets do not read is
# not copied, so a change to it cannot move the pin's meaning.
FILES = ["index.html", "src/index.css", "src/site/copy.ts", "public/favicon.png"]

root = pathlib.Path(sys.argv[1])
lock_path = root / "brand/foundation.lock"
copies = root / "brand/foundation"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def git(*args, cwd=None):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def fetch(commit):
    """The website's tree at `commit` (or its main head), and the commit."""
    work = pathlib.Path(tempfile.mkdtemp())
    git("init", "-q", str(work))
    git("-C", str(work), "fetch", "-q", "--depth", "1", f"https://github.com/{REPOSITORY}.git", commit)
    git("-C", str(work), "checkout", "-q", "--detach", "FETCH_HEAD")
    return work, git("-C", str(work), "rev-parse", "HEAD")


def pin(commit):
    work, resolved = fetch(commit)
    files = []
    for path in FILES:
        data = (work / path).read_bytes()
        (copies / path).parent.mkdir(parents=True, exist_ok=True)
        (copies / path).write_bytes(data)
        files.append({"path": path, "sha256": sha256(data)})
    lock = {
        "spec": SPEC,
        "about": "uor.foundation, the brand authority of every brand and design facet: these files, copied byte for byte into brand/foundation/ (scripts/experience/authority.py).",
        "source": {"repository": REPOSITORY, "commit": resolved},
        "files": files,
    }
    lock_path.write_text(json.dumps(lock, indent=2) + "\n")
    print(f"authority: pinned {REPOSITORY}@{resolved[:12]}")


def check():
    lock = json.loads(lock_path.read_text())
    assert lock["spec"] == SPEC, f"{lock_path}: spec is not {SPEC}"
    assert [f["path"] for f in lock["files"]] == FILES, f"{lock_path} must list exactly {FILES}"
    bad = [f["path"] for f in lock["files"] if sha256((copies / f["path"]).read_bytes()) != f["sha256"]]
    if bad:
        raise SystemExit(f"authority: {', '.join(bad)} drifted from brand/foundation.lock; run authority.py --pin")
    print(f"authority: {len(FILES)} files match {REPOSITORY}@{lock['source']['commit'][:12]}")


if sys.argv[2] == "--check":
    check()
elif sys.argv[2] == "--pin":
    pin(sys.argv[3])
elif sys.argv[2] == "--follow":
    pin("main")
else:
    raise SystemExit(__doc__)
