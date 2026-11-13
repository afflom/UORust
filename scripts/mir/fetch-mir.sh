#!/usr/bin/env bash
# Fetch the MIR CI dumped for a library crate (core or std) into OUT, one
# directory per module, so a census here reads the bytes CI's census reads.
# usage: scripts/mir/fetch-mir.sh CRATE OUT
#
# The shards of core.yml and std.yml keep each module's dump as the artifact
# CRATE-mir-KEY-MODULE, KEY being (toolchain κ, dump-script digest) as
# `scripts/mir/ledger.py mir-key` prints it. For every module the register
# crates/uorust/CRATE holds, this takes the newest unexpired artifact under
# the current KEY; a module CI has no dump of under that key is dumped here
# with scripts/mir/dump-lib.sh, and named on stderr, since a local dump may
# differ from CI's. rust-src of the pinned toolchain is fetched either way:
# the census reads its sources.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
krate=$1
out=$2
cd "$root"
key=$(python3 -I scripts/mir/ledger.py mir-key)
repo=$(git remote get-url origin | sed -E 's#^(https://github.com/|git@github.com:)##; s#\.git$##')
# rust-src, whose sources the census resolves impls against.
scripts/mir/dump-lib.sh "$krate" --modules >/dev/null
modules=$(find "crates/uorust/$krate" -maxdepth 1 -name '*.toml' ! -name frontier.toml -printf '%f\n' | sed 's/\.toml$//' | sort)
rm -rf "$out"
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
missing=()
fetched=0
for module in $modules; do
  name="$krate-mir-$key-$module"
  id=$(gh api "repos/$repo/actions/artifacts?name=$name&per_page=10" \
    --jq '[.artifacts[] | select(.expired | not)] | sort_by(.created_at) | last | .id // empty' 2>/dev/null || true)
  if [ -z "$id" ]; then
    missing+=("$module")
    continue
  fi
  gh api "repos/$repo/actions/artifacts/$id/zip" >"$work/$module.zip"
  rm -rf "$work/unzip" && mkdir -p "$work/unzip"
  unzip -q -o "$work/$module.zip" -d "$work/unzip"
  [ -f "$out/target.toml" ] || cp "$work/unzip/target.toml" "$out/target.toml"
  rm -rf "${out:?}/$module"
  # A module with no bodies (internal_macros) dumps an empty directory,
  # which the artifact's zip does not keep.
  if [ -d "$work/unzip/$module" ]; then
    mv "$work/unzip/$module" "$out/$module"
  else
    mkdir -p "$out/$module"
  fi
  fetched=$((fetched + 1))
  rm -f "$work/$module.zip"
done
echo "$krate-mir: $fetched modules fetched from CI under key $key"
if [ ${#missing[@]} -gt 0 ]; then
  echo "$krate-mir: CI has no dump under $key of ${#missing[*]} modules; dumping them here: ${missing[*]}" >&2
  ci_target=""
  [ -f "$out/target.toml" ] && ci_target=$(cat "$out/target.toml")
  scripts/mir/dump-lib.sh "$krate" "$out" "${missing[@]}"
  # The target the CI dumps were built for stands when there are any.
  # (An `if`: a bare `&&` would end the script with status 1 where CI has
  # no dump at all.)
  if [ -n "$ci_target" ]; then
    printf '%s\n' "$ci_target" >"$out/target.toml"
  fi
fi
