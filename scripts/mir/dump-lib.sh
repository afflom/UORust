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
# machine's paths resolved away. One build dumps every module asked for, so
# a census can link each module against all the others.
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
# crate root, and each module the root declares at a path of its own inside
# src (`#[path = "num/f32.rs"] pub mod f32;`); one at a path outside it (a
# submodule's crate, `../../stdarch/..`) is that crate's.
modules() {
  {
    find "$cache/library/$krate/src" -mindepth 1 -maxdepth 1 \( -type d -o -name '*.rs' \) -printf '%f\n' |
      sed 's/\.rs$//' | grep -vx 'lib\|lib\.miri'
    python3 -I -c '
import re, sys
path = None
for line in open(sys.argv[1], encoding="utf-8"):
    m = re.match(r"\s*#\[path\s*=\s*\"([^\"]*)\"\]", line)
    if m:
        path = m.group(1)
        continue
    m = re.match(r"\s*(pub(\([a-z]+\))?\s+)?mod\s+([a-z_0-9]+)\s*;", line)
    if m and path is not None and not path.startswith(".."):
        print(m.group(3))
    if not line.strip().startswith("#"):
        path = None
' "$cache/library/$krate/src/lib.rs"
  } | sort -u
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
# Its data layout, as its target specification states it: the memory's
# strides and alignments are read from it (crates/uorust/src/mir/layout.rs).
layout=$(RUSTC_BOOTSTRAP=1 rustc +"$channel" -Zunstable-options --print target-spec-json --target "$host" |
  python3 -I -c 'import json, sys; print(json.load(sys.stdin)["data-layout"])')
printf 'triple = "%s"\npointer_width = %s\ndata_layout = "%s"\n' "$host" "$width" "$layout" >"$out/target.toml"
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
# One build dumps every module asked for: a module's filter where there is
# one, the pass alone (every item) where there are several. Each module's
# bodies are its own `CRATE.MODULE-*` files.
if [ $# -eq 1 ]; then
  filter="ElaborateBoxDerefs&$1::"
else
  filter="ElaborateBoxDerefs"
fi
rm -rf "$work/mir" "$work/target"
mkdir -p "$work/mir"
(
  cd "$work/probe"
  __CARGO_TESTS_ONLY_SRC_ROOT="$cache/library" RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
    CARGO_TARGET_DIR="$work/target" \
    RUSTFLAGS="-Zdump-mir=$filter -Zdump-mir-dir=$work/mir -Coverflow-checks=on -Cdebug-assertions=off" \
    cargo +"$channel" build -q -Zbuild-std="$krate" --target "$host"
)
rm -rf "$work/target"
for module in "$@"; do
  dest="$out/$module"
  rm -rf "$dest"
  mkdir -p "$dest"
  # Only CRATE's bodies, before the pass; the paths of this machine resolved.
  find "$work/mir" -name "$krate.$module-*ElaborateBoxDerefs.before.mir" -print0 |
    xargs -0 -r sed -i -e "s#$cache/library/#library/#g" -e "s#$sysroot#<sysroot>#g"
  find "$work/mir" -name "$krate.$module-*ElaborateBoxDerefs.before.mir" -exec mv -t "$dest" {} +
  echo "$module: $(find "$dest" -type f | wc -l) bodies, $(du -sh "$dest" | cut -f1)"
done
# The layouts rustc chose for the aggregates the dumps print, which no flag
# prints whole and `repr(Rust)` leaves unspecified (crates/uorust/src/layouts.rs):
# in a copy of the library, an alias of each, `#[rustc_dump_layout(debug)]`,
# and of each of their fields, round by round, the copy checked each time.
cp -r "$cache/library" "$work/library"
check="cd '$work/probe' && __CARGO_TESTS_ONLY_SRC_ROOT='$work/library' RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR='$work/target' RUSTFLAGS='-Ztrim-diagnostic-paths=no -Coverflow-checks=on -Cdebug-assertions=off' \
  cargo +$channel check -q -Zbuild-std=$krate --target $host"
cargo run -q --locked --manifest-path "$root/Cargo.toml" -p uorust -- \
  layouts --crate "$krate" --mir "$out" --copy "$work/library" --check "$check"
rm -rf "$work/target" "$work/library"
