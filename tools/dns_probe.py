"""Read-only direct DNS probes; does not change system or namespace DNS."""
import concurrent.futures
import json
import secrets
import socket
import struct
import time
import argparse

HOST = 'region1.v2.argotunnel.com'


def probe(server):
    query_id = secrets.randbits(16)
    question = b''.join(bytes([len(label)]) + label.encode() for label in HOST.split('.')) + b'\0'
    query = struct.pack('!6H', query_id, 0x100, 1, 0, 0, 0) + question + struct.pack('!HH', 1, 1)
    start = time.monotonic()
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(2)
            sock.connect((server, 53))
            sock.send(query)
            data = sock.recv(4096)
        ident, flags, _, answers, _, _ = struct.unpack('!6H', data[:12])
        return dict(server=server, success=ident == query_id and flags & 15 == 0 and answers > 0,
                    rcode=flags & 15, answers=answers, ms=round((time.monotonic()-start)*1000))
    except OSError as error:
        return dict(server=server, success=False, error=type(error).__name__)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-system', action='store_true', help='Exit nonzero if the system DNS stub fails')
    args = parser.parse_args()
    servers = ['127.0.0.53', '198.18.0.2', '223.5.5.5', '119.29.29.29', '1.1.1.1', '8.8.8.8']
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(servers)) as pool:
        results = list(pool.map(probe, servers))
    print(json.dumps(results, indent=2))
    if args.require_system and not results[0]['success']:
        raise SystemExit(1)
