"""Install a read-only API and Quick Tunnel as the current Linux user's services."""
import argparse
import hashlib
import os
from pathlib import Path
import secrets
import shutil
import subprocess

CLOUDFLARED_SHA256 = '1d356dac769d0dbf62ebe588c4034824bbff98d5790c2f844cc99e0e2f57462d'


def write_private(path, text):
    descriptor = os.open(str(path), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        file.write(text)
    path.chmod(0o600)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server-binary', required=True)
    parser.add_argument('--cloudflared', required=True)
    parser.add_argument('--template', required=True)
    args = parser.parse_args()
    if os.geteuid() == 0:
        parser.error('Run as the desktop/SSH user, not root')
    source = Path(args.cloudflared)
    if hashlib.sha256(source.read_bytes()).hexdigest() != CLOUDFLARED_SHA256:
        raise SystemExit('Official cloudflared 2026.10.6 SHA256 mismatch; nothing installed')
    home = Path.home()
    install = home / '.local/share/gpu-pocket'
    config = home / '.config/gpu-pocket'
    units = home / '.config/systemd/user'
    for directory in [install, install / 'state', config, units]:
        directory.mkdir(parents=True, exist_ok=True)
    config.chmod(0o700)
    (install / 'state').chmod(0o700)
    for binary, name in [(Path(args.server_binary), 'gpu-pocket-server'), (source, 'cloudflared')]:
        target = install / name
        if target.exists():
            raise SystemExit('Existing binary requires an explicit upgrade: ' + str(target))
        shutil.copyfile(binary, target)
        target.chmod(0o755)
    text = Path(args.template).read_text()
    if 'control_enabled = false' not in text:
        raise SystemExit('Template must explicitly disable control')
    text = text.replace('state_dir = "/var/lib/gpu-pocket"', 'state_dir = "' + str(install / 'state') + '"')
    write_private(config / 'server.toml', text)
    env = config / 'api.env'
    if not env.exists():
        write_private(env, 'GPU_POCKET_TOKEN=' + secrets.token_urlsafe(32) + '\n')
    api = '''[Unit]
Description=GPU Pocket read-only API

[Service]
Type=simple
EnvironmentFile={config}/api.env
Environment=RUST_LOG=info
ExecStart={install}/gpu-pocket-server {config}/server.toml
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
UMask=0077

[Install]
WantedBy=default.target
'''.format(config=config, install=install)
    tunnel = '''[Unit]
Description=GPU Pocket Cloudflare Quick Tunnel
Wants=gpu-pocket-api.service
After=gpu-pocket-api.service

[Service]
Type=simple
ExecStart={install}/cloudflared tunnel --no-autoupdate --protocol http2 --edge-ip-version 4 --metrics 127.0.0.1:18787 --url http://127.0.0.1:8787
Restart=on-failure
RestartSec=15
NoNewPrivileges=true
UMask=0077

[Install]
WantedBy=default.target
'''.format(install=install)
    for name, body in [('gpu-pocket-api.service', api), ('gpu-pocket-quick-tunnel.service', tunnel)]:
        path = units / name
        if path.exists():
            raise SystemExit('Existing user unit requires review: ' + str(path))
        path.write_text(body)
    subprocess.run(['systemctl', '--user', 'daemon-reload'], check=True)
    subprocess.run(['systemctl', '--user', 'enable', '--now', 'gpu-pocket-api.service', 'gpu-pocket-quick-tunnel.service'], check=True)
    print('Read-only API and Quick Tunnel started. Credentials remain in ' + str(env))


if __name__ == '__main__':
    main()
