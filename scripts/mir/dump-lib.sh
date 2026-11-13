#!/usr/bin/env bash
# Dump the MIR of a library crate of the pinned toolchain's rust-src (core or
# std), one shard per top-level module, for `uorust core` and `uorust std`.
# usage: scripts/mir/dump-lib.sh CRATE OUT [MODULE...]
#        scripts/mir/dump-lib.sh CRATE --modules
# With no MODULE, every top-level module of library/CRATE/src is a shard;
# --modules prints that list as a JSON array, the matrix CI shards over.
# It fetches rust-src of the toolchain rust-toolchain.toml pins (checked
# against the channel manifest's hash, as fetch-std.sh does) into a cache
# under target/, builds CRATE from it with -Zbuild-std on the pinned compiler
# and a -Zdump-mir filter naming the module, and keeps each of CRATE's own
# bodies' `ElaborateBoxDerefs.before` dump under OUT/MODULE with this
# machine's paths resolved away. The build of each shard is removed before
# the next, so the disk holds one shard's build at a time.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
krate=$1
out=$2
shift 2
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")
sysroot=$(rustc +"$channel" --print sysroot)
manifest="$sysroot/lib/rustlib/multirust-channel-manifest.toml"
section=$(sed -n '/^\[pkg\.rust-src\.target\."\*"\]/,/^\[/p' "$manifest")
url=$(sed -n 's/^xz_url = "\(.*\)"/\1/p' <<<"$section")
hash=$(sed -n 's/^xz_hash = "\(.*\)"/\1/p' <<<"$section")
cache="$root/target/rust-src-$channel"
if [ ! -d "$cache/library/$krate" ]; then
  mkdir -p "$cache"
  curl -sSfL -o "$cache/rust-src.tar.xz" "$url"
  echo "$hash  $cache/rust-src.tar.xz" | sha256sum -c --quiet
  tar -xJf "$cache/rust-src.tar.xz" -C "$cache"
  rm -rf "$cache/library"
  mv "$cache"/rust-src-*/rust-src/lib/rustlib/src/rust/library "$cache/library"
  rm -rf "$cache"/rust-src-* "$cache/rust-src.tar.xz"
fi
# The crate's top-level modules: each directory and file of its src but the
# crate root.
modules() {
  find "$cache/library/$krate/src" -mindepth 1 -maxdepth 1 \( -type d -o -name '*.rs' \) -printf '%f\n' |
    sed 's/\.rs$//' | grep -vx 'lib\|lib\.miri' | sort -u
}
if [ "$out" = --modules ]; then
  modules | python3 -I -c 'import json, sys; print(json.dumps([l.strip() for l in sys.stdin if l.strip()]))'
  exit 0
fi
if [ $# -eq 0 ]; then
  set -- $(modules)
fi
host=$(rustc +"$channel" -vV | sed -n 's/^host: //p')
# The target the bodies are built for, with its pointer width as rustc's
# own data layout states it: `usize` in these bodies is that wide.
mkdir -p "$out"
width=$(rustc +"$channel" --print cfg --target "$host" | sed -n 's/^target_pointer_width="\(.*\)"/\1/p')
printf 'triple = "%s"\npointer_width = %s\n' "$host" "$width" >"$out/target.toml"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/probe/src"
printf '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n' >"$work/probe/Cargo.toml"
# A probe that links CRATE and nothing more.
if [ "$krate" = core ]; then
  printf '#![no_std]\n' >"$work/probe/src/lib.rs"
else
  printf '\n' >"$work/probe/src/lib.rs"
fi
for module in "$@"; do
  rm -rf "$work/mir" "$work/target"
  mkdir -p "$work/mir"
  (
    cd "$work/probe"
    __CARGO_TESTS_ONLY_SRC_ROOT="$cache/library" RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
      CARGO_TARGET_DIR="$work/target" \
      RUSTFLAGS="-Zdump-mir=ElaborateBoxDerefs&$module:: -Zdump-mir-dir=$work/mir -Coverflow-checks=on -Cdebug-assertions=off" \
      cargo +"$channel" build -q -Zbuild-std="$krate" --target "$host"
  )
  dest="$out/$module"
  rm -rf "$dest"
  mkdir -p "$dest"
  # Only CRATE's bodies, before the pass; the paths of this machine resolved.
  find "$work/mir" -name "$krate.$module-*ElaborateBoxDerefs.before.mir" -print0 |
    xargs -0 -r sed -i -e "s#$cache/library/#library/#g" -e "s#$sysroot#<sysroot>#g"
  find "$work/mir" -name "$krate.$module-*ElaborateBoxDerefs.before.mir" -exec mv -t "$dest" {} +
  echo "$module: $(find "$dest" -type f | wc -l) bodies, $(du -sh "$dest" | cut -f1)"
done
