# Packaging

What ships today, and how it is built. Cutting a release is
[`docs/RELEASING.md`](../docs/RELEASING.md).

## What's here

| Path | Purpose |
|---|---|
| `build-linux-bundle.sh` | Builds `dist/penguinsync-<version>-<arch>-linux.tar.gz` + `.sha256` |
| `build-android-release.sh` | Builds `dist/penguinsync-<version>.apk` + `.sha256` |
| `linux/install.sh` | User-scoped installer shipped inside the tarball. No root. |
| `linux/uninstall.sh` | Removes what the installer wrote; `--purge` also removes state |
| `linux/BUNDLE-README.md` | Becomes `README.md` inside the tarball |
| `systemd/penguinsyncd.service` | systemd **user** unit, `WantedBy=default.target`, `Restart=on-failure` |

Both build scripts read the version from `Cargo.toml`'s
`[workspace.package]`, and the Android one refuses to run if
`android/app/build.gradle.kts` disagrees.

## Linux: distribution by tarball

Deliberately, for now. The tarball installs entirely under `$HOME` — binaries
in `~/.local/bin`, the user unit in `~/.config/systemd/user`, the GNOME Shell
extension in `~/.local/share/gnome-shell/extensions` — so there is nothing to
undo as root and no distribution-specific packaging to keep correct while the
project is still moving this fast.

**Runtime dependencies:** none beyond systemd and GNOME. The daemon speaks
mDNS itself and talks to the Shell extension over the session bus.

**Firewall:** the Fedora Workstation zone already opens 1025–65535 TCP+UDP,
so nothing is needed there. Other distributions and zones will need
documented guidance.

### Deliberately absent

- **No D-Bus `.service` file.** A `.service` file in `/usr/share/dbus-1` means
  *activation*, and this daemon is explicitly not D-Bus-activated and does not
  idle-exit (`docs/design.md` §4.3). It claims `org.penguinsync.Daemon1`
  itself, at startup, from the systemd user unit. Adding the file would
  quietly reintroduce the behaviour that was rejected.
- **No `.desktop` file yet.** The `NoDisplay=true` + `MimeType=all/all` entry
  described in `docs/design.md` is the *Open With* fallback for sending files
  where `nautilus-python` is absent. There is no file transfer in 0.1.0, so
  it would be an entry that does nothing.

### Later

RPM spec and COPR, with a `penguinsync-nautilus` subpackage so headless
installs don't pull Python. Waits until file transfer exists and the install
layout has stopped changing.

## Android: distribution by APK

The release APK is signed with a self-signed key held by the maintainer
(`docs/RELEASING.md`), attached to the GitHub release with its SHA-256.
Users will see Android's warning about installing from outside a store.

Release builds shrink with R8, which is not optional — unshrunk, the dex
alone is around 55 MB, almost all of it unused Material icons. The keep rules
in `android/app/proguard-rules.pro` protect the reflective UniFFI/JNA
boundary, and a release APK must be run on a real device before shipping,
because a wrong rule fails at the first native call rather than at build time.

Shipped ABIs are `arm64-v8a` and `armeabi-v7a`; `x86_64` is added in debug
builds only, so the emulator works without a phone.

### F-Droid

Out of scope until M4. The reproducible-build hygiene it needs is being
applied from day one anyway — see
[`docs/reproducible-builds.md`](../docs/reproducible-builds.md) — so this is
a matter of writing metadata, not of retrofitting the build.

When it lands, users must be told that Play Protect blocks
internet-sideloaded APKs declaring `NOTIFICATION_LISTENER`, and that
installing through the F-Droid client avoids the classification.
