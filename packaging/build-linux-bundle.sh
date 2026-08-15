#!/usr/bin/env bash
# Builds the Linux release tarball attached to a GitHub release.
#
# Output: dist/penguinsync-<version>-<arch>-linux.tar.gz, plus a .sha256 next
# to it. The tarball is self-contained — binaries, the systemd unit, the GNOME
# Shell extension, and an installer that needs no root.
#
# Usage: packaging/build-linux-bundle.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Single source of truth for the version: the workspace manifest. Deriving it
# anywhere else is how a tag and a tarball end up disagreeing.
version="$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$version" ]] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

arch="$(uname -m)"
name="penguinsync-$version-$arch-linux"
staging="dist/$name"

echo "==> Building penguinsyncd and penguinsync ($version, $arch)"
cargo build --workspace --release

echo "==> Staging $staging"
rm -rf "$staging"
mkdir -p "$staging/bin" "$staging/systemd" "$staging/gnome-extension"

install -m 755 target/release/penguinsyncd "$staging/bin/penguinsyncd"
install -m 755 target/release/penguinsync "$staging/bin/penguinsync"
install -m 644 packaging/systemd/penguinsyncd.service "$staging/systemd/"
install -m 644 gnome-extension/extension.js gnome-extension/metadata.json "$staging/gnome-extension/"
install -m 755 packaging/linux/install.sh packaging/linux/uninstall.sh "$staging/"
install -m 644 packaging/linux/BUNDLE-README.md "$staging/README.md"
install -m 644 LICENSE "$staging/LICENSE"

echo "==> Packing"
tar -C dist -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")

# The staging tree is an intermediate, and leaving it in dist/ is not
# harmless: the release job collects every file under dist/ and attaches it,
# so an unpacked bundle turns into a dozen stray release assets — LICENSE,
# install.sh, the raw binaries — sitting next to the tarball.
rm -rf "$staging"

echo
echo "dist/$name.tar.gz"
cat "dist/$name.tar.gz.sha256"
