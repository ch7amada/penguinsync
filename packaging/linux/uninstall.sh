#!/usr/bin/env bash
# Removes everything install.sh wrote.
#
# Leaves your data alone by default — the device identity keypair and the list
# of paired devices live in ~/.local/share/penguinsync, and deleting them means
# every paired phone has to be paired again. Pass --purge if that is what you
# actually want.
set -euo pipefail

purge=false
[[ "${1:-}" == "--purge" ]] && purge=true

bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
ext_uuid="penguinsync-clipboard@penguinsync.org"
ext_dir="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$ext_uuid"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/penguinsync"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/penguinsync"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }

say "Stopping and disabling penguinsyncd"
systemctl --user disable --now penguinsyncd.service 2>/dev/null || true

say "Removing files"
rm -f "$bin_dir/penguinsyncd" "$bin_dir/penguinsync"
rm -f "$unit_dir/penguinsyncd.service"
rm -rf "$ext_dir"
systemctl --user daemon-reload

gnome-extensions disable "$ext_uuid" 2>/dev/null || true

if $purge; then
    say "Purging state and config"
    rm -rf "$data_dir" "$config_dir"
else
    cat <<EOF

Left in place (pass --purge to remove):
  $data_dir     device identity and paired devices
  $config_dir   configuration
EOF
fi

say "Done. Uninstall the Android app from the phone separately."
