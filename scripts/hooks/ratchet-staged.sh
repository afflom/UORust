#!/usr/bin/env bash
# The ratchet before a commit, on what is staged: the index's register and
# certified manifest against HEAD's (`just ratchet-staged`, and the
# pre-commit hook). CI's `ratchet` decides on registers generated from both
# commits. Arguments go to cargo (a lane's `--config` flags).
# usage: scripts/hooks/ratchet-staged.sh [CARGO_ARG...]
set -euo pipefail
root=$(git rev-parse --show-toplevel)
cd "$root"
out=target/ratchet-staged
mkdir -p "$out"
git show :frontier.toml >"$out/head.toml"
git show HEAD:frontier.toml >"$out/base.toml"
git show :crates/certified/manifest.toml >"$out/head-manifest.toml" 2>/dev/null || rm -f "$out/head-manifest.toml"
git show HEAD:crates/certified/manifest.toml >"$out/base-manifest.toml" 2>/dev/null || rm -f "$out/base-manifest.toml"
cargo run -q --locked -p uorust "$@" -- certify ratchet --register "$out/head.toml" --manifest "$out/head-manifest.toml" \
  --base-register "$out/base.toml" --base-manifest "$out/base-manifest.toml"
