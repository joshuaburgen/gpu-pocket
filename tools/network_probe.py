"""Read-only SSH/network checks. No credentials, firewall edits or miner actions."""
import argparse
import base64
import concurrent.futures
import json
import socket
import subprocess
from pathlib import Path
from datetime import datetime, timezone

REMOTE = r'''
import concurrent.futures, json, socket, subprocess
targets = TARGETS
def check(target):
    host, port = target
    try:
        with socket.create_connection((host, port), 5):
            return {"host":host,"port":port,"tcp":"ok"}
    except OSError as e:
        return {"host":host,"port":port,"tcp":"failed","error":str(e)}
with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
    checks = list(pool.map(check, targets))
addresses = json.loads(subprocess.check_output(["ip","-j","-6","addr","show","scope","global"]))
print(json.dumps({"addresses":addresses,"outbound_checks":checks}, ensure_ascii=False))
'''

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ssh', default='2-titan')
    parser.add_argument('--output', help='Write report as UTF-8 JSON')
    args = parser.parse_args()
    if args.ssh.startswith('-'):
        parser.error('SSH alias must not start with -')
    targets = [('region1.v2.argotunnel.com', 7844), ('region2.v2.argotunnel.com', 7844),
               ('api.trycloudflare.com', 443), ('2606:4700:4700::1111', 443)]
    local_dns = {}
    for host in ['region1.v2.argotunnel.com', 'region2.v2.argotunnel.com']:
        try:
            ips = sorted({r[4][0] for r in socket.getaddrinfo(host, 7844, socket.AF_INET, socket.SOCK_STREAM)})
            local_dns[host] = ips
            targets.extend((ip, 7844) for ip in ips[:2])
        except OSError as error:
            local_dns[host] = str(error)
    source = REMOTE.replace('TARGETS', repr(targets))
    encoded = base64.b64encode(source.encode()).decode()
    command = f"python3 -c \"import base64;exec(base64.b64decode('{encoded}'))\""
    result = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=12', args.ssh, command],
                            check=True, capture_output=True, text=True, timeout=55)
    report = {'checked_at': datetime.now(timezone.utc).isoformat(), 'local_dns': local_dns, 'remote': json.loads(result.stdout)}
    # An SSH-port check is evidence for IPv6 reachability, not proof of API/TLS readiness.
    ipv6 = []
    for interface in report['remote']['addresses']:
        for address in interface.get('addr_info', []):
            ip = address.get('local', '')
            if not ip.startswith(('2', '3')) or address.get('temporary') or address.get('deprecated'):
                continue
            try:
                with socket.create_connection((ip, 99), 4):
                    ipv6.append({'address': ip, 'ssh_port99': 'ok'})
            except OSError as error:
                ipv6.append({'address': ip, 'ssh_port99': 'failed', 'errno': error.errno,
                             'winerror': getattr(error, 'winerror', None)})
    report['local_to_server_ipv6'] = ipv6
    rendered = json.dumps(report, indent=2, ensure_ascii=False)
    if args.output:
        Path(args.output).write_text(rendered + '\n', encoding='utf-8')
        print(f'Report written: {args.output}')
    else:
        print(rendered)

if __name__ == '__main__':
    main()
