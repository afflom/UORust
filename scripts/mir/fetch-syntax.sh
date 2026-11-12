#!/usr/bin/env bash
# Fetch rustc's MIR definition (compiler/rustc_middle/src/mir/syntax.rs) at
# the toolchain rust-toolchain.toml pins into crates/uorust/mir/, without its
# comment lines, record its provenance in SOURCE.toml, and regenerate the
# translator's coverage tables (crates/uorust/src/mir/spec.rs) from it. Run it
# after a toolchain bump; the `mir` tests fail until it has run, and the
# translator fails to compile until every MIR kind the new definition adds is
# lowered or made a typed residue.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")
path=compiler/rustc_middle/src/mir/syntax.rs
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git -C "$work" init -q
git -C "$work" fetch -q --depth 1 --filter=blob:none https://github.com/rust-lang/rust "refs/tags/$channel"
git -C "$work" checkout -q FETCH_HEAD -- "$path"
commit=$(git -C "$work" rev-parse FETCH_HEAD)
upstream=$(sha256sum "$work/$path" | cut -d' ' -f1)
# Only the declarations are read: the comment lines are left out.
sed -E '/^\s*\/\//d' "$work/$path" >"$root/crates/uorust/mir/syntax.rs"
sha=$(sha256sum "$root/crates/uorust/mir/syntax.rs" | cut -d' ' -f1)
sed -i \
  -e "s/^toolchain = .*/toolchain = \"$channel\"/" \
  -e "s/^commit = .*/commit = \"$commit\"/" \
  -e "s/^upstream_sha256 = .*/upstream_sha256 = \"$upstream\"/" \
  -e "s/^sha256 = .*/sha256 = \"$sha\"/" \
  "$root/crates/uorust/mir/SOURCE.toml"
cd "$root"
UORUST_WRITE_SPEC=1 cargo test -q --locked -p uorust --lib mir::generate
