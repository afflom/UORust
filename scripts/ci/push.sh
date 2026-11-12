#!/usr/bin/env bash
# Commit what a writer changed under the named paths and push it to main as
# github-actions[bot], then dispatch the named workflows on the new head.
#
#     scripts/ci/push.sh "<commit subject>" "<path> ..." [workflow.yml ...]
#
# A push made with the workflow token starts no `push` workflow, so the gate
# and the page are dispatched explicitly for the commit this makes. Nothing is
# pushed when nothing changed, or when main moved past the commit the writers
# ran on: the newer push carries its own run. The checkout keeps no
# credentials (persist-credentials: false); the token is passed to this one
# push as a header and to `gh` through GH_TOKEN.
set -euo pipefail
subject=$1
read -r -a paths <<<"$2"
shift 2
: "${GH_TOKEN:?GH_TOKEN is required}"
base=$(git rev-parse HEAD)
git add -- "${paths[@]}"
if git diff --cached --quiet; then
  echo "push: nothing changed under ${paths[*]}"
  exit 0
fi
git diff --cached --stat
git -c user.name='github-actions[bot]' \
  -c user.email='41898282+github-actions[bot]@users.noreply.github.com' \
  commit -q -m "$subject" -m "Written by the ${GITHUB_WORKFLOW:-local} workflow from ${base}."
auth=$(printf 'x-access-token:%s' "$GH_TOKEN" | base64 -w0)
remote="https://github.com/${GITHUB_REPOSITORY:?}.git"
head=$(git -c "http.extraheader=AUTHORIZATION: basic $auth" ls-remote "$remote" refs/heads/main | cut -f1)
if [[ $head != "$base" ]]; then
  echo "push: main moved from $base to $head; its own run regenerates"
  exit 0
fi
if ! git -c "http.extraheader=AUTHORIZATION: basic $auth" push "$remote" HEAD:refs/heads/main; then
  echo "push: main moved while pushing; its own run regenerates"
  exit 0
fi
for workflow in "$@"; do
  gh workflow run "$workflow" --repo "$GITHUB_REPOSITORY" --ref main
done
{
  echo "## $subject"
  echo
  echo "Pushed \`$(git rev-parse HEAD)\` to main and dispatched: $*"
} >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
