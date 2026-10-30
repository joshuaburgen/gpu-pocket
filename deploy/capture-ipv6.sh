#!/usr/bin/env bash
# Time-limited TCP header capture for the diagnostic HTTP endpoint only.
set -Eeuo pipefail
(( EUID == 0 )) || { echo 'Run with sudo' >&2; exit 1; }
address=2001:da8:8002:6bd1:5d5d:9bea:1702:6bee
duration=${1:-900}
[[ $duration =~ ^[0-9]+$ ]] && (( duration >= 30 && duration <= 1800 )) || exit 2
umask 077
directory=/var/tmp/gpu-pocket-ipv6-$(date +%Y%m%d-%H%M%S)-$$
mkdir -m 0700 "$directory"
was_active=false
if systemctl is-active --quiet gpu-pocket-ipv6.service; then was_active=true; fi
cleanup() {
  if [[ $was_active == false ]]; then systemctl stop gpu-pocket-ipv6.service; fi
  echo "Capture stopped. Private results: $directory"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP
ip6tables-save > "$directory/firewall-before.rules"
ip -6 route show table all > "$directory/routes-before.txt"
systemctl start gpu-pocket-ipv6.service
curl -g -6 --noproxy '*' --fail --silent --show-error --retry 5 --retry-connrefused --retry-delay 1 --retry-max-time 10 --max-time 5 "http://[$address]/ipv6-check" > "$directory/local-health.txt"
grep -q gpu-pocket-ipv6-ok "$directory/local-health.txt"
echo "CAPTURE_READY $(date -Is)"
echo "Results: $directory"
echo 'Capturing only IPv6 TCP port 80 headers; no API port, credentials or packet payload dumps.'
set +e
timeout --signal=INT "$duration" tcpdump -i any -nn -tttt -l -s 80 "ip6 and host $address and tcp port 80" 2>"$directory/capture-info.txt" | tee "$directory/tcp-headers.txt"
capture_status=${PIPESTATUS[0]}
set -e
if [[ $capture_status != 0 && $capture_status != 124 && $capture_status != 130 ]]; then
  echo "Capture failed: $capture_status" >&2
  cat "$directory/capture-info.txt" >&2
  exit "$capture_status"
fi
