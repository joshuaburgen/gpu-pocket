"""Exercise a temporary API process. Real mode is strictly read-only."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--config', required=True)
    parser.add_argument('--real', action='store_true')
    args = parser.parse_args()
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    token = secrets.token_urlsafe(32)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(method, path, body=None, authenticated=True):
        headers = {'Content-Type': 'application/json'}
        if authenticated:
            headers['Authorization'] = 'Bearer ' + token
        req = urllib.request.Request('http://127.0.0.1:%s/api/v1/%s' % (port, path),
                                     data=None if body is None else json.dumps(body).encode(),
                                     headers=headers, method=method)
        try:
            response = opener.open(req, timeout=12)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            try:
                data = json.loads(raw)
            except ValueError:
                data = None
            return response.code, data

    with tempfile.TemporaryDirectory(prefix='gpu-pocket-smoke-') as directory:
        text = Path(args.config).read_text()
        for key, value in [('listen', json.dumps('127.0.0.1:%s' % port)),
                           ('state_dir', json.dumps(directory + '/state')),
                           ('control_enabled', 'false' if args.real else 'true')]:
            text = re.sub(r'^' + key + r'\s*=.*$', key + ' = ' + value, text, flags=re.M)
        config = Path(directory) / 'server.toml'
        config.write_text(text)
        env = dict(os.environ, GPU_POCKET_TOKEN=token, RUST_LOG='error')
        if args.real:
            env.pop('GPU_POCKET_MODE', None)
        else:
            env['GPU_POCKET_MODE'] = 'demo'
        process = subprocess.Popen([str(Path(args.binary).resolve()), str(config)], env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError(process.stderr.read().decode())
                try:
                    code, _ = request('GET', 'status', authenticated=False)
                    break
                except urllib.error.URLError:
                    time.sleep(0.1)
            else:
                raise RuntimeError('API startup timed out')
            assert code == 401
            code, status = request('GET', 'status')
            assert code == 200 and status['demo'] == (not args.real)
            assert status['gpus'], status['errors']
            assert request('GET', 'catalog')[0] == 200
            selected = dict(profile_id='pearl', pool_id='sg', wallet='smokeTest.worker', revision=0)
            if args.real:
                assert status['control_enabled'] is False
                assert request('PUT', 'selection', selected)[0] == 403
                assert request('POST', 'miner', dict(operation='restart', revision=0))[0] == 403
                assert request('PUT', 'proxy', dict(name='DIRECT'))[0] == 403
            else:
                assert request('PUT', 'selection', selected)[0] == 200
                assert request('PUT', 'selection', selected)[0] == 409
                assert request('POST', 'miner', dict(operation='restart', revision=1))[0] == 200
                selected.update(pool_id='hk', revision=1)
                assert request('PUT', 'selection', selected)[0] == 200
                latest = request('GET', 'status')[1]
                assert latest['active']['pool_id'] == 'sg'
                assert latest['desired']['pool_id'] == 'hk'
                assert request('PUT', 'proxy', dict(name='DEMO-SG'))[0] == 200
                assert request('GET', 'proxy')[1]['current'] == 'DEMO-SG'
                assert request('POST', 'miner', dict(operation='stop', revision=2))[0] == 200
            print(json.dumps({'mode': 'real-read-only' if args.real else 'demo',
                              'result': 'PASS', 'gpu_count': len(status['gpus']),
                              'gpu_names': [gpu['name'] for gpu in status['gpus']],
                              'metric_errors': status['errors']}, ensure_ascii=False))
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


if __name__ == '__main__':
    main()
