# Packaging

**Not yet written.**

## Linux

| Artifact | Notes |
|---|---|
| `penguinsyncd.service` | systemd **user** unit, `WantedBy=default.target`, `Restart=on-failure`. Always running — not D-Bus-activated, not idle-exiting. |
| `org.penguinsync.Daemon1.service` | D-Bus name registration (for discovery by clients, not for activation) |
| `penguinsync.desktop` | `NoDisplay=true`, `MimeType=all/all` — the *Open With* fallback for when `nautilus-python` is absent. Also supplies the `desktop-entry` hint so mirrored notifications are attributed correctly by GNOME Shell. |
| RPM spec / COPR | Main package plus a `penguinsync-nautilus` subpackage, so headless installs don't pull Python |

**Runtime dependencies:** Avahi (used over D-Bus for mDNS). The GNOME Shell extension is required for clipboard only — the daemon runs without it.

**Firewall:** the Fedora Workstation zone already opens 1025–65535 TCP+UDP, so no configuration is needed there. Other distributions and zones will need documented guidance.

## Android

F-Droid metadata, plus the reproducible-build recipe. See `docs/reproducible-builds.md` — the checklist is applied from day one rather than retrofitted.

Users must be told that Play Protect blocks internet-sideloaded APKs declaring `NOTIFICATION_LISTENER`, and that installing through the F-Droid client avoids the classification.
