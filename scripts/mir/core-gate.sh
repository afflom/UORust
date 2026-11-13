#!/usr/bin/env bash
# The core register's ratchet on CI's own MIR (`just core-gate`, and the
# pre-push hook when a push touches crates/uorust/src/mir): fetch the dump CI
# made under the current key (scripts/mir/fetch-mir.sh), translate every
# module of it against a copy of the committed register, and fail when a body
# the register records as translated is residue. The committed register is
# left as it is. Arguments go to cargo (a lane's `--config` flags).
# usage: scripts/mir/core-gate.sh [CARGO_ARG...]
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
scripts/mir/fetch-mir.sh core target/core-mir || {
  echo "core-gate: fetching CI's core MIR failed, so nothing was checked" >&2
  exit 1
}
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)
rm -rf target/core-gate
cp -r crates/uorust/core target/core-gate
cargo run -q --locked -p uorust "$@" -- core --mir target/core-mir \
  --library "target/rust-src-$channel/library" --register target/core-gate
