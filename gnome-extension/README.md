# GNOME Shell extension

**Not yet written.** Lands with M1.

## Why this exists

Mutter implements **neither** `wlr-data-control-unstable-v1` **nor** `ext-data-control-v1` — verified by reading `src/meson.build` on `main` (post-50.4). Four requests between 2019 and 2025 were closed, most within a day. This is a settled position, not a backlog item.

Consequences:

- `wl-paste --watch` **fails on GNOME** — watch mode only exists over data-control.
- The `xdg-desktop-portal` clipboard route works, but a clipboard-only `RemoteDesktop` session still registers a remote-access handle, lighting up the permanent **"Stop Screen Sharing"** indicator in the top panel. Unshippable for a clipboard utility.

So the only viable route is what GSConnect, GPaste and Clipboard Indicator all do: a Shell extension watching the selection and exposing D-Bus.

## Rules for this directory

**Keep it minimal. All logic stays in Rust.** This code runs inside the `gnome-shell` process, where a bug hangs the user's desktop. It does selection access and byte transfer, and nothing else.

Model it on GSConnect's `src/shell/clipboard.js`:

- Watch: `global.display.get_selection()` → `owner-changed`, filtered to `Meta.SelectionType.SELECTION_CLIPBOARD`
- Read: `selection.transfer_async(…)`
- Write: `Meta.SelectionSourceMemory.new(mimetype, bytes)` → `selection.set_owner(…)`

## D-Bus interface

Consumed by `penguinsyncd` via `zbus`.

| | |
|---|---|
| Bus name | `org.penguinsync.Clipboard` |
| Object path | `/org/penguinsync/Clipboard` |

Methods: `GetMimetypes() → as`, `GetText() → s`, `SetText(s)`, `GetValue(s) → ay`, `SetValue(ay, s)`
Signal: `OwnerChange()`

## Maintenance cost, accepted openly

The GJS/Shell API breaks roughly every six months. `shell-version` must be revalidated each cycle. Distribution through extensions.gnome.org requires human review — clipboard extensions are routinely approved, so this is not a blocker.

**The daemon must run fine without this extension.** File transfer and notification mirroring do not need it; clipboard is simply reported unavailable. A daemon that refuses to start because an extension is missing is a support nightmare.

## Not doing

Subclassing `NotificationMessage` to add inline reply, the way GSConnect does. That broke at GNOME 45 and again at 48. This extension already carries a maintenance burden for clipboard, which is load-bearing; a second, more fragile monkey-patch for a convenience feature is how it becomes the thing that breaks every cycle.
