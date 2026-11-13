#!/usr/bin/env bash
# Dump the MIR of core's own bodies, one shard per module, for `uorust core`.
# usage: scripts/mir/dump-core.sh OUT [MODULE...]
# With no MODULE, every top-level module of library/core/src is a shard.
# It fetches rust-src of the toolchain rust-toolchain.toml pins (checked
# against the channel manifest's hash, as fetch-std.sh does) into a cache
# under target/, builds core from it with -Zbuild-std on the pinned compiler
# and a -Zdump-mir filter naming the module, and keeps each body's
# `ElaborateBoxDerefs.before` dump under OUT/MODULE with this machine's paths
# resolved away. The build of each shard is removed before the next, so the
# disk holds one shard's build at a time.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
out=$1
shift
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")
sysroot=$(rustc +"$channel" --print sysroot)
manifest="$sysroot/lib/rustlib/multirust-channel-manifest.toml"
section=$(sed -n '/^\[pkg\.rust-src\.target\."\*"\]/,/^\[/p' "$manifest")
url=$(sed -n 's/^xz_url = "\(.*\)"/\1/p' <<<"$section")
hash=$(sed -n 's/^xz_hash = "\(.*\)"/\1/p' <<<"$section")
cache="$root/target/rust-src-$channel"
if [ ! -d "$cache/library/core" ]; then
  mkdir -p "$cache"
  curl -sSfL -o "$cache/rust-src.tar.xz" "$url"
  echo "$hash  $cache/rust-src.tar.xz" | sha256sum -c --quiet
  tar -xJf "$cache/rust-src.tar.xz" -C "$cache"
  mv "$cache"/rust-src-*/rust-src/lib/rustlib/src/rust/library "$cache/library"
  rm -rf "$cache"/rust-src-* "$cache/rust-src.tar.xz"
fi
if [ $# -eq 0 ]; then
  set -- $(find "$cache/library/core/src" -mindepth 1 -maxdepth 1 \( -type d -o -name '*.rs' \) -printf '%f\n' |
    sed 's/\.rs$//' | grep -vx 'lib\|lib\.miri' | sort -u)
fi
host=$(rustc +"$channel" -vV | sed -n 's/^host: //p')
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/probe/src"
printf '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n' >"$work/probe/Cargo.toml"
printf '#![no_std]\n' >"$work/probe/src/lib.rs"
for module in "$@"; do
  rm -rf "$work/mir" "$work/target"
  mkdir -p "$work/mir"
  (
    cd "$work/probe"
    __CARGO_TESTS_ONLY_SRC_ROOT="$cache/library" RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
      CARGO_TARGET_DIR="$work/target" \
      RUSTFLAGS="-Zdump-mir=ElaborateBoxDerefs&$module:: -Zdump-mir-dir=$work/mir -Coverflow-checks=on -Cdebug-assertions=off" \
      cargo +"$channel" build -q -Zbuild-std=core --target "$host"
  )
  dest="$out/$module"
  rm -rf "$dest"
  mkdir -p "$dest"
  # Only core's bodies, before the pass; the paths of this machine resolved.
  find "$work/mir" -name "core.$module-*ElaborateBoxDerefs.before.mir" -print0 |
    xargs -0 -r sed -i -e "s#$cache/library/#library/#g" -e "s#$sysroot#<sysroot>#g"
  find "$work/mir" -name "core.$module-*ElaborateBoxDerefs.before.mir" -exec mv -t "$dest" {} +
  echo "$module: $(find "$dest" -type f | wc -l) bodies, $(du -sh "$dest" | cut -f1)"
done
