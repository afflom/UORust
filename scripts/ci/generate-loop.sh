#!/usr/bin/env bash
# Run every writer and push what changed, computed on the very tree it lands
# on, however main moves meanwhile. generate.yml's one step.
# usage: scripts/ci/generate-loop.sh SUBJECT "PATHS" [WORKFLOW...]
#
# A writer's output is only right for the tree it was computed on, so a
# commit of outputs is never rebased over newer source (push.sh pushes it
# only when main is still the tree it ran on). When main moved, this resets
# to the new main and runs the writers again in the same job: the SDK image,
# the cargo build and every verdict already decided carry over (a verdict is
# keyed by its translation's κ and digest, so it holds on any tree), and only
# the translations the newer commits added are verified. It loops until a
# push lands, nothing changed, or the time budget ($GENERATE_BUDGET seconds)
# is spent; then the newest push's own run takes over.
set -euo pipefail
subject=$1
paths=$2
shift 2
: "${GH_TOKEN:?GH_TOKEN is required}"
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
auth=$(printf 'x-access-token:%s' "$GH_TOKEN" | base64 -w0)
remote="https://github.com/${GITHUB_REPOSITORY:?}.git"
remote_main() {
  git -c "http.extraheader=AUTHORIZATION: basic $auth" ls-remote "$remote" refs/heads/main | cut -f1
}
verdicts=(lift-verdicts.toml crates/uorust/tests/fixtures/mir-path/lift-verdicts.toml)
keep=$(mktemp -d)
trap 'rm -rf "$keep"' EXIT
deadline=$(($(date +%s) + ${GENERATE_BUDGET:-4200}))
attempt=0
while true; do
  attempt=$((attempt + 1))
  base=$(git rev-parse HEAD)
  echo "generate: attempt $attempt on $base"
  # The brand authority, kept only while its brand meets WCAG 2.2.
  python3 -I scripts/experience/authority.py . --follow
  if ! python3 -I scripts/experience/experience.py "$(mktemp -d)" .; then
    git checkout -- brand
    echo "::warning::uor.foundation's newest main breaks a WCAG 2.2 contrast; the brand stays pinned"
  fi
  # Verdicts an earlier attempt decided.
  for v in "${verdicts[@]}"; do
    if [[ -f $keep/$v ]]; then cp "$keep/$v" "$v"; fi
  done
  scripts/ci/sdk.sh 'just generate'
  for v in "${verdicts[@]}"; do
    mkdir -p "$keep/$(dirname "$v")"
    cp "$v" "$keep/$v"
  done
  out=$(scripts/ci/push.sh "$subject" "$paths" "$@")
  printf '%s\n' "$out"
  if grep -q '^push: nothing changed' <<<"$out" || [[ $(remote_main) == "$(git rev-parse HEAD)" ]]; then
    exit 0
  fi
  if (($(date +%s) >= deadline)); then
    echo "generate: main kept moving past the time budget; the newest push's own run regenerates"
    exit 0
  fi
  git -c "http.extraheader=AUTHORIZATION: basic $auth" fetch -q "$remote" "+refs/heads/main:refs/remotes/upstream/main"
  git reset -q --hard upstream/main
  git clean -qfd -- $paths
done
