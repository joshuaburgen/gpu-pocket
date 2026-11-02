#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: update_srbminer.sh <miner-root-directory>" >&2
  exit 2
fi

miner_root=$1
release_api=https://api.github.com/repos/doktor83/SRBMiner-Multi/releases/latest
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

release_json=$(curl -fsSL --retry 5 --retry-delay 3 "$release_api")
version=$(printf '%s' "$release_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"].lstrip("v"))')
asset_info=$(printf '%s' "$release_json" | python3 -c 'import json,sys; d=json.load(sys.stdin); a=next(a for a in d["assets"] if a["name"].lower().endswith("linux.tar.gz")); print(a["browser_download_url"]+"\n"+a["url"])')
asset_url=$(printf '%s' "$asset_info" | sed -n '1p')
asset_api_url=$(printf '%s' "$asset_info" | sed -n '2p')

archive="$work_dir/srbminer.tar.gz"
curl -fL --retry 5 --retry-delay 5 --output "$archive" "$asset_url"
md5_url="${asset_url%.tar.gz}.tar.md5"
if curl -fsSL --retry 3 "$md5_url" -o "$work_dir/srbminer.tar.md5"; then
  expected=$(awk '{print $1}' "$work_dir/srbminer.tar.md5")
  actual=$(md5sum "$archive" | awk '{print $1}')
  [ "${expected,,}" = "${actual,,}" ] || { echo "SRBMiner MD5 mismatch" >&2; exit 1; }
fi

tar -xzf "$archive" -C "$work_dir"
binary=$(find "$work_dir" -type f -size +10M -print0 2>/dev/null | while IFS= read -r -d '' f; do file "$f" | grep -q 'ELF .* executable' && printf '%s\n' "$f"; done | head -n 1)
if [ -z "$binary" ]; then
  binary=$(find "$work_dir" -type f -exec file {} \; | awk -F: '/ELF .* executable/ {print $1}' | head -n 1)
fi
if [ -z "$binary" ]; then
  binary=$(find "$work_dir" -type f -iname '*srbminer*' | head -n 1 || true)
fi
if [ -z "$binary" ] || [ ! -f "$binary" ]; then
  echo "SRBMiner executable not found in release $version" >&2
  exit 1
fi
chmod 0755 "$binary"

if command -v timeout >/dev/null 2>&1; then
  timeout 10s "$binary" --help >/dev/null 2>&1 || true
else
  "$binary" --help >/dev/null 2>&1 || true
fi
if [ ! -x "$binary" ]; then
  echo "SRBMiner executable is not runnable for $version" >&2
  exit 1
fi

version_dir="$miner_root/research-srb-$version"
staging="$miner_root/.research-srb-$version.tmp"
rm -rf "$staging"
mkdir -p "$staging"
cp -a "$(dirname "$binary")/." "$staging/"
cp -a "$binary" "$staging/research-srb"
chmod 0755 "$staging/research-srb"
printf '%s\n' "$version" > "$staging/.gpu-pocket-version"

if [ ! -d "$version_dir" ]; then
  mv "$staging" "$version_dir"
else
  rm -rf "$staging"
fi

compat="$miner_root/research-srb-3-8-0"
backup=""
if [ -e "$compat" ] && [ ! -L "$compat" ]; then
  backup="$miner_root/research-srb-3-8-0.backup-$(date +%Y%m%d-%H%M%S)"
  mv "$compat" "$backup"
fi
rm -f "$compat"
ln -s "research-srb-$version" "$compat"

printf 'updated SRBMiner: %s\ncompatibility link: %s -> %s\nbackup: %s\n' "$version" "$compat" "research-srb-$version" "${backup:-none}"
