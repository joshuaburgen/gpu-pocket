#!/usr/bin/env bash
# Restore only the active host interface's DNS from NetworkManager's DHCP data.
# Does not restart networking, edit resolv.conf, or enter research-tun.
set -Eeuo pipefail
if (( EUID != 0 )); then
  echo 'Run this script with sudo in a trusted terminal. Do not share the password.' >&2
  exit 1
fi
interface=${1:-wlx6c1ff738d2cf}
[[ $interface =~ ^[a-zA-Z0-9_.:-]+$ ]] || { echo 'Invalid interface' >&2; exit 1; }
for tool in nmcli resolvectl dig timeout getent; do command -v "$tool" >/dev/null; done
old_line=$(resolvectl dns "$interface")
read -r -a old_dns <<< "${old_line#*:}"
mapfile -t candidates < <(nmcli -g IP4.DNS device show "$interface" | tr ' |' '\n' | sed '/^$/d')
good_dns=()
for resolver in "${candidates[@]}"; do
  [[ $resolver =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
  if dig +time=2 +tries=1 +short "@$resolver" region1.v2.argotunnel.com A | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' >/dev/null; then
    good_dns+=("$resolver")
  fi
done
if (( ${#good_dns[@]} == 0 )); then
  echo 'No working DHCP DNS server; no changes made.' >&2
  exit 1
fi
backup=/var/backups/gpu-pocket-dns/$(date +%Y%m%d-%H%M%S)-$$
install -d -m 0700 "$backup"
resolvectl status > "$backup/resolved-before.txt"
nmcli -f IP4.DNS,IP6.DNS device show "$interface" > "$backup/dhcp-dns.txt"
{
  printf '#!/usr/bin/env bash\nset -e\nresolvectl dns %q' "$interface"
  printf ' %q' "${old_dns[@]}"
  printf '\nresolvectl flush-caches\n'
} > "$backup/rollback.sh"
chmod 0700 "$backup/rollback.sh"
restore_on_error() {
  echo "Validation failed; restoring previous DNS. Backup: $backup" >&2
  bash "$backup/rollback.sh"
}
trap restore_on_error ERR
printf 'Restoring %s DNS from DHCP: %s\n' "$interface" "${good_dns[*]}"
resolvectl dns "$interface" "${good_dns[@]}"
resolvectl flush-caches
for name in region1.v2.argotunnel.com api.trycloudflare.com; do
  timeout 12 getent ahostsv4 "$name" >/dev/null
  printf 'PASS host resolution: %s\n' "$name"
done
trap - ERR
resolvectl dns "$interface"
echo "Repair complete. Rollback: sudo bash $backup/rollback.sh"
echo 'NetworkManager profile was already set to automatic DNS; no profile or miner namespace was changed.'
