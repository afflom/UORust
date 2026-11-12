#!/usr/bin/env bash
# Vendor std's own MIR for the items crates/uorust/mir/std.toml lifts into
# crates/uorust/mir/std.mir. It fetches rust-src of the toolchain
# rust-toolchain.toml pins (about 6 MB), checks it against the hash the
# toolchain's own channel manifest records, builds core and alloc from it
# with -Zbuild-std on the pinned stable compiler (RUSTC_BOOTSTRAP=1, as
# `uorust mir` does) and a -Zdump-mir filter that names only those items, and
# writes their bodies with every path of this machine resolved away. Nothing
# is installed into the toolchain, and the scratch build is removed.
# Run it after a toolchain bump or a change of the `lift` list; the
# `std_is_vendored_from_the_pinned_toolchain` test fails until it has run.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")
sysroot=$(rustc +"$channel" --print sysroot)
manifest="$sysroot/lib/rustlib/multirust-channel-manifest.toml"
section=$(sed -n '/^\[pkg\.rust-src\.target\."\*"\]/,/^\[/p' "$manifest")
url=$(sed -n 's/^xz_url = "\(.*\)"/\1/p' <<<"$section")
hash=$(sed -n 's/^xz_hash = "\(.*\)"/\1/p' <<<"$section")
grep -q "^rust_src_xz_sha256 = \"$hash\"" "$root/crates/uorust/mir/std.toml" || {
  echo "fetch-std: std.toml's rust_src_xz_sha256 is not the pinned toolchain's ($hash)" >&2
  exit 1
}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -sSfL -o "$work/rust-src.tar.xz" "$url"
echo "$hash  $work/rust-src.tar.xz" | sha256sum -c --quiet
tar -xJf "$work/rust-src.tar.xz" -C "$work"
library=$(echo "$work"/rust-src-*/rust-src/lib/rustlib/src/rust/library)
# The filter: each listed item's module and name; the writer keeps the exact
# items and drops whatever else the filter matched.
filter=$(sed -n '/^lift = \[/,/^\]/p' "$root/crates/uorust/mir/std.toml" |
  sed -n 's/^ *"\(.*\)",*$/\1/p' |
  awk -F'::' '{ printf "%sElaborateBoxDerefs&%s::&::%s", sep, $2, $NF; sep="|" }')
mkdir -p "$work/probe/src" "$work/mir"
printf '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n' >"$work/probe/Cargo.toml"
printf '#![no_std]\n' >"$work/probe/src/lib.rs"
(
  cd "$work/probe"
  __CARGO_TESTS_ONLY_SRC_ROOT="$library" RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
    CARGO_TARGET_DIR="$work/target" \
    RUSTFLAGS="-Zdump-mir=$filter -Zdump-mir-dir=$work/mir -Coverflow-checks=on -Cdebug-assertions=off" \
    cargo +"$channel" build -q -Zbuild-std=core,alloc --target "$(rustc +"$channel" -vV | sed -n 's/^host: //p')"
)
cd "$root"
UORUST_STD_DUMPS="$work/mir" cargo test -q --locked -p uorust --lib mir::generate::tests::std_is_vendored
UORUST_WRITE_SPEC=1 cargo test -q --locked -p uorust --lib mir::generate::tests::std_is_vendored
