#!/usr/bin/env bash
# Builds the release APK attached to a GitHub release.
#
# Output: dist/penguinsync-<version>.apk plus a .sha256.
#
# Signing: android/keystore.properties, or the PENGUINSYNC_KEYSTORE_* /
# PENGUINSYNC_KEY_* environment variables (see docs/RELEASING.md). Without
# either, Gradle still produces an APK — an unsigned one, which cannot be
# installed. This script refuses to publish that by mistake.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$version" ]] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

# The Gradle project reads versionName independently; a mismatch means the
# tag, the tarball and the APK disagree about what this release is.
gradle_version="$(sed -n 's/^ *versionName = "\(.*\)"/\1/p' android/app/build.gradle.kts | head -1)"
if [[ "$gradle_version" != "$version" ]]; then
    echo "version mismatch: Cargo.toml says $version, android/app/build.gradle.kts says $gradle_version" >&2
    exit 1
fi

echo "==> Building the release APK ($version)"
# -Ppenguinsync.release=true is what selects the release-android cargo profile
# and drops x86_64 from the shipped ABIs. Do not omit it: :core's fallback
# heuristic reads task names, and a debug-profile .so in a release APK is
# several times larger and slower.
(cd android && ./gradlew --no-daemon :app:assembleRelease -Ppenguinsync.release=true)

apk=android/app/build/outputs/apk/release/app-release.apk
if [[ ! -f "$apk" ]]; then
    echo "no signed APK at $apk — the build produced app-release-unsigned.apk," >&2
    echo "which means no keystore was configured. See docs/RELEASING.md." >&2
    exit 1
fi

mkdir -p dist
install -m 644 "$apk" "dist/penguinsync-$version.apk"
(cd dist && sha256sum "penguinsync-$version.apk" > "penguinsync-$version.apk.sha256")

echo
echo "dist/penguinsync-$version.apk"
cat "dist/penguinsync-$version.apk.sha256"
