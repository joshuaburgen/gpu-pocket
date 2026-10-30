"""Check the public Quick Tunnel using a private connection file; never print its token."""
import argparse
from datetime import datetime, timezone
import json
import ipaddress
from pathlib import Path
import time
import ssl
import urllib.error
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('connection')
    parser.add_argument('--output')
    parser.add_argument('--ca-file', help='Optional trusted PEM CA bundle; certificate validation remains enabled')
    args = parser.parse_args()
    settings = json.loads(Path(args.connection).read_text())
    url = urllib.parse.urlparse(settings['url'])
    try:
        address = ipaddress.IPv6Address(url.hostname or '')
        direct_ipv6 = address.is_global
    except ipaddress.AddressValueError:
        direct_ipv6 = False
    if url.scheme != 'https' or not ((url.hostname or '').endswith('.trycloudflare.com') or direct_ipv6) or url.username:
        raise SystemExit('Expected a Quick Tunnel or global IPv6 HTTPS URL')
    tls = ssl.create_default_context(cafile=args.ca_file)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect(),
                                          urllib.request.HTTPSHandler(context=tls))

    def call(path, authenticated, body=None):
        headers = {'Content-Type': 'application/json'}
        if authenticated:
            headers['Authorization'] = 'Bearer ' + settings['token']
        request = urllib.request.Request(settings['url'] + path, headers=headers,
            data=None if body is None else json.dumps(body).encode())
        try:
            response = opener.open(request, timeout=20)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.code, json.load(response)

    started = time.monotonic()
    assert call('/api/v1/status', False)[0] == 401, 'Anonymous access was not rejected'
    code, status = call('/api/v1/status', True)
    assert code == 200 and status['demo'] is False and status['control_enabled'] is False
    assert len(status['gpus']) == 2 and not status['errors'], 'GPU collection failed'
    assert call('/api/v1/miner', True, {'operation': 'stop', 'revision': 0})[0] == 403
    report = {'checked_at': datetime.now(timezone.utc).isoformat(), 'url': settings['url'],
              'anonymous_http': 401, 'authenticated_http': 200, 'control_http': 403,
              'gpu_count': len(status['gpus']), 'metric_errors': status['errors'],
              'duration_ms': round((time.monotonic()-started)*1000), 'result': 'PASS'}
    rendered = json.dumps(report, indent=2)
    if args.output:
        Path(args.output).write_text(rendered + '\n', encoding='utf-8')
    print(rendered)


if __name__ == '__main__':
    main()
