# Nautilus extension

**Not yet written.** Lands with M4.

Adds "Send with PenguinSync" to the file-manager context menu, populated live with currently-connected devices.

## Route

`nautilus-python`, modeled on GSConnect's `nautilus-gsconnect.py`. It is what GSConnect, Packet and every comparable project uses, and it is packaged on Fedora 44 as `nautilus-python-4.1.0-2.fc44`.

A native C/Rust extension was considered and rejected: the only Rust bindings are GTK3-era and last saw a release in 2022.

## Two mandatory rules

Both stolen from GSConnect, both non-negotiable:

1. **Never call `gi.require_version("Nautilus", …)`** — just `from gi.repository import Nautilus`
2. **Accept `*args` and read the file list as `args[-1]`**

Nautilus 49 bumped its introspection namespace 4.0 → 4.1 and broke every extension that hardcoded the version. GSConnect sailed through because of these two lines.

## Menu behaviour

Devices come from the daemon's `org.freedesktop.DBus.ObjectManager` at `/org/penguinsync/Daemon` — initial `GetManagedObjects()`, then `InterfacesAdded` / `InterfacesRemoved` to stay current.

| Connected devices | Menu |
|---|---|
| 0 | item shown **greyed**, "No devices connected" |
| 1 | flat `Send to <name>` |
| ≥2 | `Send with PenguinSync ▸` submenu |

A menu item that vanishes is a menu item users think is broken. Grey it instead of hiding it.

## Packaging

Ships as a separate `penguinsync-nautilus` subpackage, so headless installs do not pull Python. Installed to `/usr/share/nautilus-python/extensions/`.

**Fallback:** `packaging/` also ships a `NoDisplay=true` + `MimeType=all/all` `.desktop` file, so *Open With ▸ PenguinSync* works where `nautilus-python` is absent. The app then shows its own device picker.
