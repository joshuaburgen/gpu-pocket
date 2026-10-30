"""Prepare an IPv6-only Caddy endpoint and ACME webroot without changing the API."""
import argparse
import ipaddress
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('address')
args = parser.parse_args()
address = str(ipaddress.IPv6Address(args.address))
if not ipaddress.IPv6Address(address).is_global:
    parser.error('A global unicast IPv6 address is required')
home = Path.home()
base = home / '.config/gpu-pocket/ipv6'
data = home / '.local/share/gpu-pocket'
webroot = data / 'acme-webroot'
for path in [base, webroot, base / 'acme', base / 'work', base / 'logs', base / 'caddy-data', base / 'caddy-config']:
    path.mkdir(parents=True, exist_ok=True)
base.chmod(0o700)
common = '''{{
    admin unix/{base}/admin.sock
    auto_https off
}}
http://:80 {{
    bind {address}
    handle /ipv6-check {{
        respond "gpu-pocket-ipv6-ok" 200
    }}
    handle /.well-known/acme-challenge/* {{
        root * {webroot}
        file_server
    }}
    handle {{
        respond 404
    }}
}}
'''.format(base=base, address=address, webroot=webroot)
https = '''
https://[{address}]:443 {{
    bind {address}
    tls {base}/acme/live/gpu-pocket-ipv6/fullchain.pem {base}/acme/live/gpu-pocket-ipv6/privkey.pem
    handle /ipv6-check {{
        respond "gpu-pocket-ipv6-ok" 200
    }}
    handle {{
        reverse_proxy 127.0.0.1:8787
    }}
}}
'''.format(address=address, base=base)
config = base / 'Caddyfile'
if config.exists():
    raise SystemExit('Existing IPv6 config needs review; refusing to overwrite')
config.write_text(common)
(base / 'Caddyfile.https').write_text(common + https)
(base / 'address').write_text(address + '\n')
unit = '''[Unit]
Description=GPU Pocket direct IPv6 HTTPS endpoint
After=network-online.target
Wants=network-online.target

[Service]
User=joshua
Group=joshua
Environment=XDG_DATA_HOME={base}/caddy-data
Environment=XDG_CONFIG_HOME={base}/caddy-config
ExecStart=/opt/gpu-pocket-ipv6/caddy run --config {base}/Caddyfile --adapter caddyfile
ExecReload=/opt/gpu-pocket-ipv6/caddy reload --config {base}/Caddyfile --adapter caddyfile --address unix/{base}/admin.sock
Restart=on-failure
RestartSec=5
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
UMask=0077
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths={base}
PrivateTmp=true

[Install]
WantedBy=multi-user.target
'''.format(base=base)
(data / 'gpu-pocket-ipv6.service').write_text(unit)
print('Prepared IPv6 HTTP challenge listener; HTTPS requires a valid public certificate before activation.')
