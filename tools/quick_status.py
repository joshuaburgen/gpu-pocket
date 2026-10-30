"""Inspect installed user services and export connection data without printing tokens."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export', action='store_true', help='Write private connection.json, never print its token')
    args = parser.parse_args()
    home = Path.home()
    unit = 'gpu-pocket-quick-tunnel.service'
    invocation = subprocess.check_output(['systemctl', '--user', 'show', unit, '-p', 'InvocationID', '--value'], text=True).strip()
    if not invocation:
        raise SystemExit('Tunnel service has no current invocation')
    logs = subprocess.check_output(['journalctl', '--user', '_SYSTEMD_INVOCATION_ID=' + invocation,
                                    '-n', '150', '--no-pager', '-o', 'cat'], text=True)
    addresses = re.findall(r'https://[a-z0-9-]+\.trycloudflare\.com', logs)
    if not addresses:
        raise SystemExit('No Quick Tunnel address in current invocation; inspect journal')
    address = addresses[-1]
    env = (home / '.config/gpu-pocket/api.env').read_text()
    token = next(line.split('=', 1)[1] for line in env.splitlines() if line.startswith('GPU_POCKET_TOKEN='))
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    request = urllib.request.Request('http://127.0.0.1:8787/api/v1/status',
                                     headers={'Authorization': 'Bearer ' + token})
    with opener.open(request, timeout=10) as response:
        status = json.load(response)
    result = {'url': address, 'demo': status['demo'], 'control_enabled': status['control_enabled'],
              'gpu_count': len(status['gpus']), 'metric_errors': status['errors']}
    if status['demo'] or status['control_enabled'] or not status['gpus']:
        raise SystemExit('Read-only real API readiness check failed')
    if args.export:
        output = home / '.config/gpu-pocket/connection.json'
        descriptor = os.open(str(output), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(descriptor, 'w') as file:
            json.dump({'url': address, 'token': token}, file, indent=2)
            file.write('\n')
        output.chmod(0o600)
        result['private_connection_file'] = str(output)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
