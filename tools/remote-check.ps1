param([string]$HostAlias = '2-titan')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
# Requires the isolated toolchain in docs/DEPLOYMENT.md and a SOCKS forward on 17898.
# Build/test in /tmp; do not install services or touch running miners.
ssh $HostAlias 'mkdir -p /tmp/gpu-pocket-build'
if ($LASTEXITCODE -ne 0) { throw 'Build directory setup failed' }
scp "$root/server/Cargo.toml" "$root/server/Cargo.lock" "$root/server/config.example.toml" "${HostAlias}:/tmp/gpu-pocket-build/"
if ($LASTEXITCODE -ne 0) { throw 'Manifest upload failed' }
scp -r "$root/server/src" "${HostAlias}:/tmp/gpu-pocket-build/"
if ($LASTEXITCODE -ne 0) { throw 'Source upload failed' }
ssh $HostAlias 'cd /tmp/gpu-pocket-build; export RUSTUP_HOME=/tmp/gpu-pocket-tools/rustup CARGO_HOME=/tmp/gpu-pocket-tools/cargo HTTPS_PROXY=socks5h://127.0.0.1:17898; ~/.cargo/bin/cargo test && ~/.cargo/bin/cargo build'
if ($LASTEXITCODE -ne 0) { throw 'Rust validation failed' }
