#!/usr/bin/env bash
# Dump the MIR of core's own bodies, one shard per module, for `uorust core`:
# scripts/mir/dump-lib.sh for core.
# usage: scripts/mir/dump-core.sh OUT [MODULE...]
exec "$(dirname "$0")/dump-lib.sh" core "$@"
