#!/usr/bin/env python3
"""Land the re-pin pull request template-update.yml opens, with no hand step.

    python3 -I scripts/ci/repin.py

Run by upstream.yml each hour, after upstream.py. Each run moves the open
re-pin pull request (branch automation/template-contract-update) one step:

1. The upstream.toml rows it resolves are dropped on its branch: a row is
   resolved when upstream.py opened its "Re-pin <id> to <image>" issue for the
   very SDK image the branch's prismpm.lock pins.
2. The branch's head has no bootstrap run yet: one is dispatched (a push or a
   pull request made with the workflow token starts no workflow).
3. Its bootstrap run passed: the pull request is squash-merged, its re-pin
   issues are closed, and generate, bootstrap and gated are dispatched on the
   merge commit, as push.sh does for a commit the workflow token pushed.
   Its run failed: nothing merges, and the failure is printed.

A step whose precondition does not hold yet waits for the next hour.
"""
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
BRANCH = "automation/template-contract-update"


def run(*args, check=True):
    return subprocess.run(args, check=check, capture_output=True, text=True).stdout.strip()


def gh_json(*args):
    return json.loads(run("gh", *args) or "null")


def resolved_rows(text, image, titles):
    """The ids of the [[patch]] rows whose re-pin issue names `image`."""
    ids = re.findall(r'^\[\[patch\]\]\nid = "([^"]+)"', text, re.M)
    return [i for i in ids if f"Re-pin {i} to {image}" in titles]


def drop_rows(text, ids):
    """upstream.toml without the [[patch]] blocks of `ids`."""
    blocks = re.split(r"(?m)^(?=\[\[)", text)
    keep = [b for b in blocks
            if not any(b.startswith(f'[[patch]]\nid = "{i}"\n') for i in ids)]
    return "".join(keep)


def main():
    prs = gh_json("pr", "list", "--head", BRANCH, "--state", "open",
                  "--json", "number,headRefOid,url")
    if not prs:
        print("repin: no open re-pin pull request")
        return
    pr = prs[0]
    run("git", "fetch", "-q", "origin", f"+refs/heads/{BRANCH}:refs/remotes/origin/{BRANCH}")
    lock = json.loads(run("git", "show", f"origin/{BRANCH}:prismpm.lock"))
    image = lock["sdk_image"]
    titles = {i["title"] for i in gh_json("issue", "list", "--state", "all", "--limit", "1000",
                                          "--json", "title")}
    table = run("git", "show", f"origin/{BRANCH}:upstream.toml")
    ids = resolved_rows(table + "\n", image, titles)
    if ids:
        run("git", "checkout", "-q", "-B", BRANCH, f"origin/{BRANCH}")
        (ROOT / "upstream.toml").write_text(drop_rows(table + "\n", ids))
        run("git", "add", "upstream.toml")
        run("git", "-c", "user.name=github-actions[bot]",
            "-c", "user.email=41898282+github-actions[bot]@users.noreply.github.com",
            "commit", "-q", "-m", "Drop the upstream rows this re-pin resolves",
            "-m", "Resolved: " + ", ".join(ids) + f". The SDK is now {image}.")
        run("git", "push", "-q", "origin", f"HEAD:refs/heads/{BRANCH}")
        print(f"repin: dropped {', '.join(ids)} on {pr['url']}")
        return
    head = run("git", "rev-parse", f"origin/{BRANCH}")
    runs = gh_json("run", "list", "--workflow", "bootstrap.yml", "--branch", BRANCH,
                   "--commit", head, "--json", "status,conclusion,url", "--limit", "1")
    if not runs:
        run("gh", "workflow", "run", "bootstrap.yml", "--ref", BRANCH)
        print(f"repin: dispatched bootstrap on {head[:12]} for {pr['url']}")
        return
    verdict = runs[0]
    if verdict["status"] != "completed":
        print(f"repin: bootstrap on {head[:12]} is {verdict['status']}: {verdict['url']}")
        return
    if verdict["conclusion"] != "success":
        print(f"repin: bootstrap on {head[:12]} {verdict['conclusion']}; not merged: {verdict['url']}")
        return
    run("gh", "pr", "merge", str(pr["number"]), "--squash", "--delete-branch",
        "--match-head-commit", head)
    merged = gh_json("pr", "view", str(pr["number"]), "--json", "mergeCommit")["mergeCommit"]["oid"]
    for issue in gh_json("issue", "list", "--state", "open", "--limit", "1000",
                         "--json", "number,title"):
        if issue["title"].endswith(f" to {image}") and issue["title"].startswith("Re-pin "):
            run("gh", "issue", "close", str(issue["number"]), "--reason", "completed",
                "--comment", f"Re-pinned by {pr['url']} ({merged[:12]}).")
    run("gh", "workflow", "run", "generate.yml", "--ref", "main")
    run("gh", "workflow", "run", "bootstrap.yml", "--ref", "main")
    run("gh", "workflow", "run", "gated.yml", "--ref", "main", "-f", f"sha={merged}")
    print(f"repin: merged {pr['url']} as {merged[:12]}; dispatched generate, bootstrap, gated")


if __name__ == "__main__":
    sys.exit(main())
