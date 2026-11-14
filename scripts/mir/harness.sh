#!/usr/bin/env bash
# Run the differential check's harness (`uorust embed --harness DIR`) and
# write what it prints to OUT: each sampled function of core called on its
# cases, with core built from the pinned rust-src exactly as
# scripts/mir/dump-lib.sh builds the core whose MIR is embedded. The codegen
# flags are read from dump-lib.sh's own build line, so the two builds cannot
# drift apart.
# usage: scripts/mir/harness.sh DIR OUT
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
dir=$(cd "$1" && pwd)
out=$2
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")
cache="$root/target/rust-src-$channel"
flags=$(sed -n 's/.*RUSTFLAGS="-Zdump-mir=[^ ]* -Zdump-mir-dir=[^ ]* \(-C[^"]*\)".*/\1/p' "$root/scripts/mir/dump-lib.sh")
if [ -z "$flags" ]; then
  echo "harness: scripts/mir/dump-lib.sh names no codegen flags to build core with" >&2
  exit 1
fi
host=$(rustc +"$channel" -vV | sed -n 's/^host: //p')
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
(
  cd "$dir"
  __CARGO_TESTS_ONLY_SRC_ROOT="$cache/library" RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
    CARGO_TARGET_DIR="$work/target" RUSTFLAGS="$flags" \
    cargo +"$channel" run -q -Zbuild-std=std,panic_unwind --target "$host"
) >"$out"
