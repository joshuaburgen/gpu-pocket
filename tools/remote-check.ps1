param([string]$HostAlias = '2-titan')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
# Requires the isolated toolchain in docs/DEPLOYMENT.md. The server host DNS is
# now working, so Cargo uses its normal HTTPS connection without a proxy.
# Build/test in /tmp; do not install services or touch running miners.
ssh $HostAlias 'mkdir -p /tmp/gpu-pocket-build'
if ($LASTEXITCODE -ne 0) { throw 'Build directory setup failed' }
scp "$root/server/Cargo.toml" "$root/server/Cargo.lock" "$root/server/config.example.toml" "${HostAlias}:/tmp/gpu-pocket-build/"
if ($LASTEXITCODE -ne 0) { throw 'Manifest upload failed' }
scp -r "$root/server/src" "${HostAlias}:/tmp/gpu-pocket-build/"
if ($LASTEXITCODE -ne 0) { throw 'Source upload failed' }
ssh $HostAlias 'cd /tmp/gpu-pocket-build; export RUSTUP_HOME=/tmp/gpu-pocket-tools/rustup CARGO_HOME=/tmp/gpu-pocket-tools/cargo; ~/.cargo/bin/cargo fmt -- --check && ~/.cargo/bin/cargo test --locked && ~/.cargo/bin/cargo clippy --all-targets --locked -- -D warnings && ~/.cargo/bin/cargo build --locked'
if ($LASTEXITCODE -ne 0) { throw 'Rust validation failed' }
