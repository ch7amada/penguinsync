# PenguinSync for Linux

Clipboard sync between this desktop and an Android phone, over your local
network. No cloud, no account, no server anyone else runs.

## Install

```sh
./install.sh
```

No root. Everything goes under your home directory:

| What | Where |
|---|---|
| `penguinsyncd`, `penguinsync` | `~/.local/bin` |
| systemd user unit | `~/.config/systemd/user` |
| GNOME Shell extension | `~/.local/share/gnome-shell/extensions` |
| Identity and paired devices | `~/.local/share/penguinsync` |

The installer enables and starts the daemon, then tells you the two steps it
cannot do for you: enabling the GNOME Shell extension, and pairing your phone.

## The GNOME Shell extension is not optional for clipboard

```sh
gnome-extensions enable penguinsync-clipboard@penguinsync.org
```

GNOME implements no clipboard-manager protocol — neither
`wlr-data-control-unstable-v1` nor `ext-data-control-v1` — so there is no way
to watch the clipboard from outside the shell. A small extension does it and
hands the bytes to the daemon over D-Bus. Without it the daemon runs fine and
reports clipboard as unavailable; sending *from the phone to this desktop*
still works.

On Wayland, a freshly-installed extension is only picked up once the shell
reloads its list of extensions. If `gnome-extensions enable` says the
extension is unknown, log out and back in.

## Pair a phone

Install the Android app (`penguinsync-<version>.apk` from the same release),
then:

```sh
penguinsync
```

Press `p`. Scan the QR code with the app's Pair screen. Compare the
fingerprint shown on both screens and press `y`. The code refreshes itself
every 60 seconds, so a stale one on screen is never the problem.

## Everyday use

The daemon runs in the background from login. The TUI is only needed for
pairing and for checking status.

```sh
systemctl --user status penguinsyncd     # is it running
journalctl --user -u penguinsyncd -f     # what is it doing
penguinsync                              # devices, pairing, unpairing
```

## Uninstall

```sh
./uninstall.sh            # keeps your identity and paired devices
./uninstall.sh --purge    # removes those too
```

## Requirements

- GNOME on Wayland, Shell 50 (the extension declares that version)
- Both devices on the same local network
- systemd, for the user service

## Support

Issues and questions: <https://github.com/ch7amada/penguinsync>
