#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 ]]; then
    echo "Run with sudo: sudo bash $0 <staging-directory>" >&2
    exit 1
fi

staging_dir=${1:?Provide the directory containing the staged binary and unit files}
api_user=${GPU_POCKET_USER:-joshua}
user_record=$(getent passwd "$api_user")
api_home=$(cut -d: -f6 <<<"$user_record")
api_group=$(id -gn "$api_user")
api_dir="$api_home/.local/share/gpu-pocket"
config_dir="$api_home/.config/gpu-pocket"
api_config="$config_dir/server.toml"
api_env="$config_dir/api.env"
api_binary="$api_dir/gpu-pocket-server"
unit_source="$staging_dir/gpu-pocket-miner@.service"
sudoers_source="$staging_dir/gpu-pocket-gpu0.sudoers"
api_control_source="$staging_dir/gpu-pocket-api-control.conf"
binary_source="$staging_dir/gpu-pocket-server"
unit_target=/etc/systemd/system/gpu-pocket-miner@.service
sudoers_target=/etc/sudoers.d/gpu-pocket-gpu0
api_unit_dir="$api_home/.config/systemd/user/gpu-pocket-api.service.d"
api_unit_override="$api_unit_dir/50-gpu-control.conf"
backup_dir="/var/backups/gpu-pocket-control/$(date -u +%Y%m%dT%H%M%SZ)"

for file in "$unit_source" "$sudoers_source" "$api_control_source" "$binary_source" "$api_config" "$api_env" "$api_binary"; do
    if [[ ! -f $file ]]; then
        echo "Required file not found: $file" >&2
        exit 1
    fi
done

if [[ -e $unit_target || -e $sudoers_target ]]; then
    echo "GPU Pocket system unit or GPU0 sudoers file already exists; inspect before upgrading." >&2
    exit 1
fi

if ! grep -Fxq 'control_enabled = false' "$api_config"; then
    echo "Expected control_enabled = false in $api_config; refusing to overwrite unexpected config." >&2
    exit 1
fi

if ! systemctl is-active --quiet research-tun.service; then
    echo "research-tun.service is not active; refusing to install a miner unit." >&2
    exit 1
fi

if systemctl is-active --quiet gpu-pocket-miner@0.service; then
    echo "GPU Pocket GPU0 miner is already active; refusing to replace its binary." >&2
    exit 1
fi

visudo -cf "$sudoers_source"
install -d -m 0700 "$backup_dir"
cp -a "$api_config" "$backup_dir/server.toml"
cp -a "$api_binary" "$backup_dir/gpu-pocket-server"
if [[ -f $api_unit_dir/../gpu-pocket-api.service ]]; then
    cp -a "$api_unit_dir/../gpu-pocket-api.service" "$backup_dir/gpu-pocket-api.service"
fi

sed 's/^control_enabled = false$/control_enabled = true/' "$api_config" > "$backup_dir/server.toml.enabled"
if ! grep -Fxq 'control_enabled = true' "$backup_dir/server.toml.enabled"; then
    echo "Failed to prepare the enabled server config." >&2
    exit 1
fi

install -d -m 0755 /opt/gpu-pocket
install -d -m 0750 -o root -g "$api_group" /etc/gpu-pocket
install -o root -g root -m 0755 "$binary_source" /opt/gpu-pocket/gpu-pocket-server
install -o root -g "$api_group" -m 0640 "$backup_dir/server.toml.enabled" /etc/gpu-pocket/server.toml
install -o root -g root -m 0644 "$unit_source" "$unit_target"
install -o root -g root -m 0440 "$sudoers_source" "$sudoers_target"
visudo -c

install -d -o "$api_user" -g "$api_group" -m 0755 "$api_unit_dir"
install -o "$api_user" -g "$api_group" -m 0644 "$api_control_source" "$api_unit_override"

install -o "$api_user" -g "$api_group" -m 0755 "$binary_source" "$api_binary.next"
mv -f "$api_binary.next" "$api_binary"
install -o "$api_user" -g "$api_group" -m 0600 "$backup_dir/server.toml.enabled" "$api_config"
systemctl daemon-reload

api_uid=$(id -u "$api_user")
runuser -u "$api_user" -- env \
    XDG_RUNTIME_DIR="/run/user/$api_uid" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$api_uid/bus" \
    systemctl --user daemon-reload
runuser -u "$api_user" -- env \
    XDG_RUNTIME_DIR="/run/user/$api_uid" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$api_uid/bus" \
    systemctl --user restart gpu-pocket-api.service

sleep 2
if ! runuser -u "$api_user" -- env \
    XDG_RUNTIME_DIR="/run/user/$api_uid" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$api_uid/bus" \
    systemctl --user is-active --quiet gpu-pocket-api.service; then
    echo "GPU Pocket API did not restart; backups are in $backup_dir." >&2
    exit 1
fi

api_token=$(sed -n 's/^GPU_POCKET_TOKEN=//p' "$api_env" | head -n 1)
if [[ -z $api_token ]]; then
    echo "GPU_POCKET_TOKEN is missing from $api_env." >&2
    exit 1
fi
control_enabled=$(
    curl --fail --silent --show-error --max-time 10 \
        -H "Authorization: Bearer $api_token" \
        http://127.0.0.1:8787/api/v1/status |
        python3 -c 'import json,sys; print(json.load(sys.stdin).get("control_enabled"))'
)
unset api_token
if [[ $control_enabled != true ]]; then
    echo "The API did not report control_enabled=true; backups are in $backup_dir." >&2
    exit 1
fi

echo "GPU0 controls installed and enabled. Existing GPU processes were not stopped."
echo "GPU1 was not granted systemd control. Backups: $backup_dir"
echo "GPU0 start will be refused until its existing unmanaged miner is stopped."
