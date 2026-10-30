"""Summarize tcpdump header text without publishing full client IPv6 addresses."""
import argparse
from collections import Counter, defaultdict
from datetime import datetime
import ipaddress
import json
from pathlib import Path
import re

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('capture')
parser.add_argument('--control', required=True, help='Known control client IPv6')
parser.add_argument('--external-start', required=True)
parser.add_argument('--external-end', required=True)
parser.add_argument('--output')
args = parser.parse_args()
server = '2001:da8:8002:6bd1:5d5d:9bea:1702:6bee'
start = datetime.fromisoformat(args.external_start)
end = datetime.fromisoformat(args.external_end)
peers = defaultdict(Counter)
flows = defaultdict(dict)
external_packets = 0
packet_count = 0
pattern = re.compile(r'^(\S+ \S+) IP6 (\S+) > (\S+): Flags \[([^]]+)\]')
for line in Path(args.capture).read_text().splitlines():
    match = pattern.search(line)
    if not match:
        continue
    time, source, target, flags = match.groups()
    moment = datetime.fromisoformat(time)
    source_ip, source_port = source.rsplit('.', 1)
    target_ip, target_port = target.rsplit('.', 1)
    outgoing = source_ip == server
    peer = target_ip if outgoing else source_ip
    port = target_port if outgoing else source_port
    peers[peer][('out:' if outgoing else 'in:') + flags] += 1
    packet_count += 1
    external_packets += int(start <= moment <= end)
    flow = flows[(peer, port)]
    if not outgoing and flags == 'S':
        flow.setdefault('syn', moment)
    if not outgoing and flags in ['.', 'P.']:
        flow.setdefault('ack', moment)
report = {'timezone': 'Asia/Shanghai', 'packet_count': packet_count,
          'external_probe_window': {'start': str(start), 'end': str(end), 'matching_packets': external_packets},
          'clients': []}
for number, (peer, counts) in enumerate(peers.items(), 1):
    delays = [round((flow['ack'] - flow['syn']).total_seconds() * 1000, 3)
              for (client, _), flow in flows.items() if client == peer and 'syn' in flow and 'ack' in flow]
    report['clients'].append({'label': 'computer-control' if peer == args.control else 'unconfirmed-client-' + str(number),
        'prefix': str(ipaddress.IPv6Network(peer + '/64', strict=False)),
        'flags': dict(counts), 'first_syn_to_first_ack_ms_by_connection': delays})
rendered = json.dumps(report, indent=2)
if args.output:
    Path(args.output).write_text(rendered + '\n', encoding='utf-8')
print(rendered)
