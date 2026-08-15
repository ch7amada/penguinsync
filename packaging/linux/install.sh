#!/usr/bin/env bash
# PenguinSync installer — user-scoped, no root.
#
# Everything lands under $HOME: binaries in ~/.local/bin, the systemd user
# unit in ~/.config/systemd/user, the GNOME Shell extension in
# ~/.local/share/gnome-shell/extensions. Nothing here needs sudo, and
# uninstall.sh removes exactly what this script wrote.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
ext_uuid="penguinsync-clipboard@penguinsync.org"
ext_dir="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$ext_uuid"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m  %s\n' "$*"; }

say "Installing binaries to $bin_dir"
mkdir -p "$bin_dir"
install -m 755 "$here/bin/penguinsyncd" "$bin_dir/penguinsyncd"
install -m 755 "$here/bin/penguinsync" "$bin_dir/penguinsync"

say "Installing the systemd user unit to $unit_dir"
mkdir -p "$unit_dir"
install -m 644 "$here/systemd/penguinsyncd.service" "$unit_dir/penguinsyncd.service"

say "Installing the GNOME Shell extension to $ext_dir"
mkdir -p "$ext_dir"
install -m 644 "$here/gnome-extension/extension.js" "$ext_dir/extension.js"
install -m 644 "$here/gnome-extension/metadata.json" "$ext_dir/metadata.json"

say "Enabling and starting penguinsyncd"
systemctl --user daemon-reload
systemctl --user enable --now penguinsyncd.service

# ~/.local/bin is on PATH by default on most distributions, but not all, and a
# working install whose command "isn't found" reads as a broken install.
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) warn "$bin_dir is not on your PATH — add it to your shell profile, or run $bin_dir/penguinsync directly." ;;
esac

cat <<EOF

Installed. Two things are left, and both need you:

  1. Enable the GNOME Shell extension — clipboard sync from Linux to the phone
     goes through it, because Mutter exposes no clipboard protocol of its own:

         gnome-extensions enable $ext_uuid

     On Wayland a newly-installed extension is only picked up after the shell
     reloads its extension list, so log out and back in if the command reports
     that the extension is unknown.

  2. Pair your phone:

         penguinsync

     Press 'p', scan the QR code with the PenguinSync app, and confirm the
     fingerprint on both screens.

Check on the daemon at any time with:

    systemctl --user status penguinsyncd
    journalctl --user -u penguinsyncd -f

EOF
