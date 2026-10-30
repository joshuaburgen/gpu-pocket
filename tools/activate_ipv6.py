"""Activate an issued IPv6 certificate and schedule user-level ACME renewals."""
from pathlib import Path
import shutil
import subprocess

home = Path.home()
base = home / '.config/gpu-pocket/ipv6'
binary = Path('/opt/gpu-pocket-ipv6/caddy')
certbot = home / '.local/share/gpu-pocket/certbot-venv/bin/certbot'
certificate = base / 'acme/live/gpu-pocket-ipv6/fullchain.pem'
if not certificate.is_file():
    raise SystemExit('Public production certificate is missing; staging certificates are never activated')
config = base / 'Caddyfile'
backup = base / 'Caddyfile.http-backup'
if not backup.exists():
    shutil.copyfile(config, backup)
shutil.copyfile(base / 'Caddyfile.https', config)
try:
    subprocess.run([str(binary), 'validate', '--config', str(config), '--adapter', 'caddyfile'], check=True)
    subprocess.run([str(binary), 'reload', '--config', str(config), '--adapter', 'caddyfile',
                    '--address', 'unix/' + str(base / 'admin.sock'), '--force'], check=True)
except subprocess.CalledProcessError:
    shutil.copyfile(backup, config)
    raise
hook = base / 'reload-certificate.sh'
hook.write_text('#!/usr/bin/env bash\nset -e\nexec ' + str(binary) +
    ' reload --config ' + str(config) + ' --adapter caddyfile --address unix/' + str(base / 'admin.sock') + ' --force\n')
hook.chmod(0o700)
units = home / '.config/systemd/user'
service = '''[Unit]
Description=Renew GPU Pocket IPv6 IP certificate

[Service]
Type=oneshot
ExecStart={certbot} renew --config-dir {base}/acme --work-dir {base}/work --logs-dir {base}/logs --quiet --deploy-hook {base}/reload-certificate.sh
NoNewPrivileges=true
UMask=0077
'''.format(certbot=certbot, base=base)
timer = '''[Unit]
Description=Check GPU Pocket IPv6 certificate every six hours

[Timer]
OnCalendar=*-*-* 00,06,12,18:00:00
RandomizedDelaySec=15m
Persistent=true
Unit=gpu-pocket-ipv6-renew.service

[Install]
WantedBy=timers.target
'''
(units / 'gpu-pocket-ipv6-renew.service').write_text(service)
(units / 'gpu-pocket-ipv6-renew.timer').write_text(timer)
subprocess.run(['systemctl', '--user', 'daemon-reload'], check=True)
subprocess.run(['systemctl', '--user', 'enable', '--now', 'gpu-pocket-ipv6-renew.timer'], check=True)
print('Direct IPv6 HTTPS activated; certificate renewal checks scheduled every six hours.')
