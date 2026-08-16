# Releasing

A release is two artifacts — a Linux tarball and an Android APK — published
together under one tag, built by `.github/workflows/release.yml`.

## Branch model

Two long-lived branches, since v0.1.0:

| Branch | What it is |
|---|---|
| `develop` | **Default.** Where all work lands. Feature branches merge here. |
| `main` | Released code only. Every commit on it is a version someone can download. |

Feature branches come off `develop` and go back into `develop`. `main` only
moves when a release is being cut, and every merge into it is immediately
tagged.

The point is that `main` answers one question without qualification: *what is
currently released?* Anyone landing on the repository sees `develop`, which is
where the project actually is; anyone cloning `main` gets something that was
built, signed and shipped.

CI runs on every branch, so `develop` is checked the same way `main` is.

## One-time setup: the Android signing key

Android identifies an app by its signing key, not by its package name. Every
update has to be signed with the same key the user already has installed;
signing 0.2.0 with a different key means every user must uninstall and lose
their pairing before they can update. **This key cannot be rotated. Losing it
is unrecoverable.**

Generate it once:

```sh
keytool -genkeypair -v \
  -keystore penguinsync-release.jks \
  -alias penguinsync \
  -keyalg RSA -keysize 4096 \
  -validity 10000 \
  -dname "CN=PenguinSync, O=PenguinSync, C=DE"
```

Keep the `.jks` and both passwords in a password manager, plus one offline
copy. `.gitignore` already excludes `*.jks` and `android/keystore.properties`;
that is a safety net, not the plan.

**Locally**, create `android/keystore.properties` (never committed):

```properties
storeFile=/absolute/path/to/penguinsync-release.jks
storePassword=…
keyAlias=penguinsync
keyPassword=…
```

**In CI**, add four repository secrets:

| Secret | Value |
|---|---|
| `PENGUINSYNC_KEYSTORE_BASE64` | `base64 -w0 penguinsync-release.jks` |
| `PENGUINSYNC_KEYSTORE_PASSWORD` | store password |
| `PENGUINSYNC_KEY_ALIAS` | `penguinsync` |
| `PENGUINSYNC_KEY_PASSWORD` | key password |

Without a keystore the release build still succeeds and produces
`app-release-unsigned.apk` — deliberately, so anyone can check that the app
compiles in release mode. `packaging/build-android-release.sh` refuses to
publish that.

## Cutting a release

Steps 1–3 happen on `develop`. Step 4 is the merge into `main`, and step 5 is
the tag.

1. **Bump the version in both places.** They are checked against each other
   and against the tag, and the build fails on a mismatch.
   - `Cargo.toml` → `[workspace.package] version`
   - `android/app/build.gradle.kts` → `versionName`, and **`versionCode`,
     which must increase** — Android refuses to install an APK whose
     `versionCode` is not greater than the installed one, regardless of what
     `versionName` says.

2. **Write the changelog entry.** Move `[Unreleased]` content into a new
   `## [x.y.z] - YYYY-MM-DD` section in `CHANGELOG.md` and update the link
   definitions at the bottom. This section becomes the GitHub release body
   verbatim, so write it for users, not for contributors.

3. **Check the release build by hand before tagging.** CI cannot do the part
   that matters:

   ```sh
   ./packaging/build-linux-bundle.sh
   ./packaging/build-android-release.sh
   ```

   Then install the APK on a real phone and pair it. R8 shrinking is on for
   release builds, and the UniFFI/JNA boundary it has to preserve is
   reflective — a wrong keep rule in `android/app/proguard-rules.pro` fails
   at the first native call, on a user's phone, not in any build log. The
   fingerprint on the Settings screen is a live call into Rust; if it renders,
   the boundary survived.

   Note that a release APK cannot be installed over a debug one — different
   signature. Uninstall first, which also wipes the phone's identity and
   pairings.

4. **Merge `develop` into `main`.** Nothing else should be going into `main`,
   so this is a fast, boring merge.

   ```sh
   git checkout develop && git push          # everything above is on develop
   git checkout main && git pull --ff-only
   git merge --no-ff develop -m "Release 0.1.0"
   git push
   ```

5. **Tag `main` and push the tag.** Tag the merge commit, not `develop` — the
   tag has to point at what was released.

   ```sh
   git tag -a v0.1.0 -m "PenguinSync 0.1.0"
   git push origin v0.1.0
   git checkout develop                      # go straight back; don't work on main
   ```

   The workflow builds both artifacts, checks that the tag agrees with both
   version numbers, and publishes a GitHub release with the tarball, the APK
   and their `.sha256` files.

6. **Install the published artifacts** on a machine that is not the one you
   built them on, following `README.md` exactly as a user would. Bad install
   instructions are the most common way a working release still fails.

`workflow_dispatch` runs the same build without publishing, which is how to
find out the Android toolchain still works without spending a tag on it.

## What the version number means here

- **Patch** — fixes only, no protocol change.
- **Minor** — new features. Non-GNOME compositors are 0.2.0.
- The wire protocol carries its own version (`docs/protocol.md`); a bump there
  is a breaking change for anyone who does not update both sides, and needs to
  be called out at the top of the changelog entry.
