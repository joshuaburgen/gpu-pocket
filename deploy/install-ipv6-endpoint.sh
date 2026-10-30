#!/usr/bin/env bash
# Run with sudo after preparing and validating Caddy configuration as joshua.
set -Eeuo pipefail
(( EUID == 0 )) || { echo 'Run with sudo' >&2; exit 1; }
source_dir=/media/new_data3/.local/share/gpu-pocket
config=/media/new_data3/.config/gpu-pocket/ipv6/Caddyfile
[[ ! -e /etc/systemd/system/gpu-pocket-ipv6.service ]] || { echo 'Existing unit requires review' >&2; exit 1; }
caddy="$source_dir/caddy-extracted/usr/bin/caddy"
"$caddy" validate --config "$config" --adapter caddyfile
install -d -m 0755 /opt/gpu-pocket-ipv6
install -m 0755 -o root -g root "$caddy" /opt/gpu-pocket-ipv6/caddy
install -m 0644 -o root -g root "$source_dir/gpu-pocket-ipv6.service" /etc/systemd/system/gpu-pocket-ipv6.service
systemctl daemon-reload
systemctl enable --now gpu-pocket-ipv6.service
systemctl is-active gpu-pocket-ipv6.service
