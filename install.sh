#!/bin/sh
# Install the uorust CLI from a GitHub release of afflom/UORust.
#
#   curl -fsSL https://afflom.github.io/UORust/install.sh | sh
#
# Picks the release archive for this machine, checks its SHA-256 against the
# release's SHA256SUMS, and installs the binary. Settings, all optional:
#
#   UORUST_VERSION      the release to install, e.g. 0.1.0 (default: the latest)
#   UORUST_INSTALL_DIR  where the binary goes (default: $HOME/.local/bin)
#   UORUST_REPOSITORY   the GitHub repository (default: afflom/UORust)
#   UORUST_ATTESTATION  1 to also verify the archive's build provenance with
#                       `gh attestation verify` (requires the GitHub CLI)
#
# The archives are the ones release.yml builds: uorust-<target>.tar.gz holding
# uorust-<target>/uorust. A target the release has no archive for is refused.
set -eu

repository=${UORUST_REPOSITORY:-afflom/UORust}
install_dir=${UORUST_INSTALL_DIR:-${HOME:?}/.local/bin}
version=${UORUST_VERSION:-}
version=${version#v}

say() { printf 'uorust-install: %s\n' "$*" >&2; }
die() {
  say "$*"
  exit 1
}

case $(uname -s) in
  Linux) os=unknown-linux-gnu ;;
  Darwin) os=apple-darwin ;;
  *) die "no release archive for $(uname -s); on Windows run install.ps1, elsewhere: cargo install --locked uorust" ;;
esac
case $(uname -m) in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) die "no release archive for $(uname -m); build from source: cargo install --locked uorust" ;;
esac
# A macOS shell under Rosetta reports x86_64 on Apple silicon: take the native build.
if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
  arch=aarch64
fi
target=$arch-$os
archive=uorust-$target.tar.gz

if [ -n "$version" ]; then
  base=https://github.com/$repository/releases/download/v$version
else
  base=https://github.com/$repository/releases/latest/download
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl --proto '=https' --tlsv1.2 -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
else
  die "needs curl or wget"
fi
if command -v sha256sum >/dev/null 2>&1; then
  digest() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  digest() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "needs sha256sum or shasum to check the download"
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

say "fetching SHA256SUMS from $base"
fetch "$base/SHA256SUMS" "$work/SHA256SUMS" || die "no SHA256SUMS at $base"
# A line is "<hex>  <name>" or "<hex> *<name>" (binary mode).
expected=$(awk -v name="$archive" '{ file = $2; sub(/^\*/, "", file); if (file == name) print $1 }' "$work/SHA256SUMS")
[ -n "$expected" ] || die "the release has no $archive; its archives: $(awk '{ sub(/^\*/, "", $2); printf "%s ", $2 }' "$work/SHA256SUMS")"

say "fetching $archive"
fetch "$base/$archive" "$work/$archive" || die "could not download $base/$archive"
actual=$(digest "$work/$archive")
[ "$actual" = "$expected" ] || die "SHA-256 mismatch for $archive: expected $expected, got $actual"
say "SHA-256 $actual matches SHA256SUMS"

if [ "${UORUST_ATTESTATION:-0}" = 1 ]; then
  command -v gh >/dev/null 2>&1 || die "UORUST_ATTESTATION=1 needs the GitHub CLI (gh)"
  gh attestation verify "$work/$archive" --repo "$repository" >/dev/null || die "build provenance of $archive does not verify"
  say "build provenance verified: signed by $repository's release workflow"
fi

tar -xzf "$work/$archive" -C "$work"
mkdir -p "$install_dir"
cp "$work/uorust-$target/uorust" "$install_dir/uorust.tmp"
chmod 755 "$install_dir/uorust.tmp"
mv -f "$install_dir/uorust.tmp" "$install_dir/uorust"
say "installed $("$install_dir/uorust" --version) to $install_dir/uorust"
case ":${PATH:-}:" in
  *":$install_dir:"*) ;;
  *) say "add $install_dir to PATH to run uorust by name" ;;
esac
