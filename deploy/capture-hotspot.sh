#!/usr/bin/env bash
# Capture only headers for the known mobile /64 and the existing IPv6 SSH listener.
set -Eeuo pipefail
(( EUID == 0 )) || { echo 'Run with sudo' >&2; exit 1; }
umask 077
directory=/var/tmp/gpu-pocket-hotspot-$(date +%Y%m%d-%H%M%S)-$$
mkdir -m 0700 "$directory"
ip6tables-save -c > "$directory/firewall-before.rules"
ip -6 route show table all > "$directory/routes.txt"
ss -lnt > "$directory/listeners.txt"
echo "CAPTURE_READY $(date -Is) $directory"
set +e
timeout --signal=INT 180 tcpdump -i any -nn -tttt -l -s 80 'ip6 and host 2001:da8:8002:6bd1:5d5d:9bea:1702:6bee and net 240e:46e:8600:3a0::/64 and (icmp6 or tcp port 99)' 2>"$directory/capture-info.txt" | tee "$directory/headers.txt"
status=${PIPESTATUS[0]}
set -e
ip6tables-save -c > "$directory/firewall-after.rules"
echo "Capture finished: $directory"
[[ $status == 0 || $status == 124 || $status == 130 ]]
