# Sourced by the hooks: the lane's cargo flags (the brief's LANE_CFG) in
# `cfg`, and the shared target. The lane's N is $LANE_N when set, else one
# derived from the worktree's path; every workspace member is built with it,
# since repo-model reads its own CARGO_MANIFEST_DIR and a reused rlib could
# check another lane's tree.
root=$(git rev-parse --show-toplevel)
N=${LANE_N:-$(( $(printf '%s' "$root" | cksum | cut -d' ' -f1) % 200 + 30 ))}
cfg=()
for p in uorust repo-model repo-conformance xtask; do
  cfg+=(--config "profile.dev.package.$p.codegen-units=$N" --config "profile.dev.package.$p.debug=0")
done
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/home/user/uorust-engine/target} CARGO_INCREMENTAL=0
