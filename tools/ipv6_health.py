"""Temporary IPv6-only reachability endpoint. No API forwarding or credentials."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import socket
import threading


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != '/ipv6-check':
            self.send_error(404)
            return
        body = b'gpu-pocket-ipv6-ok\n'
        self.send_response(200)
        self.send_header('Content-Type', 'text/plain; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


class Server(ThreadingHTTPServer):
    address_family = socket.AF_INET6

    def server_bind(self):
        self.socket.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
        super().server_bind()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--address', required=True)
    parser.add_argument('--port', type=int, default=8766)
    parser.add_argument('--duration', type=int, default=600)
    args = parser.parse_args()
    ipaddress.IPv6Address(args.address)
    with Server((args.address, args.port), Handler) as server:
        timer = threading.Timer(args.duration, server.shutdown)
        timer.daemon = True
        timer.start()
        print('Temporary IPv6 health endpoint started', flush=True)
        server.serve_forever()
        timer.cancel()
