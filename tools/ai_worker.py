"""Desktop AI worker: poll the authenticated GPU Pocket relay; expose no listener."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
OUTPUT_LIMIT = 24000


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Relay:
    def __init__(self, connection):
        self.connection = Path(connection)
        # An explicit OpenSSL CA bundle avoids malformed entries in the Windows
        # certificate store while retaining hostname and certificate validation.
        bundle = ssl.get_default_verify_paths().cafile
        context = ssl.create_default_context(cafile=bundle) if bundle else ssl.create_default_context()
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
            urllib.request.HTTPSHandler(context=context), NoRedirect())

    def request(self, path, body):
        settings = json.loads(self.connection.read_text(encoding='utf-8-sig'))
        url = urllib.parse.urlsplit(settings['url'])
        local = url.hostname in ('localhost', '127.0.0.1', '::1')
        if (url.scheme != 'https' and not (local and url.scheme == 'http')) or url.username or url.password or url.query or url.fragment or url.path not in ('', '/'):
            raise ValueError('Relay requires an HTTPS origin, or loopback HTTP')
        token = settings['token'].strip()
        if len(token) < 32:
            raise ValueError('Invalid relay credential')
        request = urllib.request.Request(settings['url'].rstrip('/') + path,
            data=json.dumps(body).encode(), headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
        with self.opener.open(request, timeout=15) as response:
            return json.load(response)


def codex_command():
    path = shutil.which('codex.exe') or shutil.which('codex')
    if not path:
        return None
    path = Path(path)
    if path.suffix.lower() in ('.cmd', '.ps1'):
        package = path.parent / 'node_modules/@openai/codex'
        matches = list(package.glob('node_modules/@openai/codex-win32-*/vendor/*/bin/codex.exe'))
        return [str(matches[0])] if matches else None
    return [str(path)]


def capabilities(command):
    available = False
    message = '尚未安装 Codex CLI'
    if command:
        try:
            result = subprocess.run(command + ['login', 'status'], capture_output=True, timeout=10,
                encoding='utf-8', errors='replace', **hidden())
            available = result.returncode == 0 and 'logged in' in (result.stdout + result.stderr).lower()
            message = '本机 Codex 已登录' if available else '请在电脑上执行 codex login 完成登录'
        except (OSError, subprocess.TimeoutExpired):
            message = '无法启动本机 Codex CLI'
    return [{'id': 'codex', 'available': available, 'message': message},
        {'id': 'cursor', 'available': False, 'message': '桌面 Cursor 的 Desktop Bridge 尚未启用；未使用 Cursor CLI'}]


def hidden():
    return {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}


def build_command(command, workspace, task, output):
    if task['provider'] != 'codex':
        raise ValueError('Desktop Cursor is not connected')
    sandbox = 'workspace-write' if task.get('allow_edits') else 'read-only'
    return command + ['-a', 'never', 'exec', '--sandbox', sandbox, '--cd', str(workspace),
        '--ephemeral', '--color', 'never', '--output-last-message', str(output), '-']


def stop_process(process):
    if process.poll() is not None:
        return
    if os.name == 'nt':
        subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], capture_output=True, **hidden())
    else:
        import signal
        os.killpg(process.pid, signal.SIGKILL)
    process.wait(timeout=10)


def tail(path):
    with Path(path).open('rb') as stream:
        stream.seek(max(0, Path(path).stat().st_size - OUTPUT_LIMIT))
        return stream.read(OUTPUT_LIMIT).decode('utf-8', errors='ignore')


def execute(command, workspace, task, heartbeat, timeout=1800):
    with tempfile.TemporaryDirectory(prefix='gpu-pocket-ai-') as directory:
        output = Path(directory) / 'answer.txt'
        log = Path(directory) / 'cli.log'
        env = os.environ.copy()
        env.pop('CODEX_THREAD_ID', None)
        prompt = ('这是来自 GPU Pocket 手机端的项目任务。工作目录是 GPU Pocket 仓库。'
            '不要读取或输出凭据、令牌、.env 或 .local 内的私人连接文件。'
            + ('允许按本次任务修改项目文件。' if task.get('allow_edits') else '本次仅问答和分析，不得修改文件或操作生产服务。')
            + '\n\n用户任务：\n' + task['prompt'])
        process = None
        status = 'failed'
        reason = ''
        started = last_contact = time.monotonic()
        try:
            with log.open('wb') as stream:
                process = subprocess.Popen(build_command(command, workspace, task, output), cwd=workspace,
                    stdin=subprocess.PIPE, stdout=stream, stderr=subprocess.STDOUT, env=env,
                    start_new_session=os.name != 'nt', **hidden())
                process.stdin.write(prompt.encode('utf-8'))
                process.stdin.close()
                while process.poll() is None:
                    try:
                        response = heartbeat(task['id'])
                        last_contact = time.monotonic()
                        if response.get('cancel'):
                            status, reason = 'cancelled', '任务已取消。'
                            break
                    except (OSError, ValueError):
                        if time.monotonic() - last_contact > 40:
                            reason = '与中转服务器断开，已停止本机任务。'
                            break
                    if time.monotonic() - started > timeout or log.stat().st_size > 8 * 1024 * 1024:
                        reason = '任务超时或输出超过限制，已停止。'
                        break
                    time.sleep(3)
                else:
                    status = 'completed' if process.returncode == 0 else 'failed'
        finally:
            if process is not None:
                stop_process(process)
        answer = tail(output) if output.exists() and output.stat().st_size else tail(log)
        return status, reason or answer or 'AI 未返回内容。'


def write_journal(path, value):
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(value, ensure_ascii=False), encoding='utf-8')
    temp.replace(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--connection', type=Path, default=ROOT / '.local/connection.json')
    parser.add_argument('--workspace', type=Path, default=ROOT)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    if workspace != ROOT or not (workspace / 'server/Cargo.toml').exists():
        raise SystemExit('This worker is restricted to the GPU Pocket checkout')
    command = codex_command()
    if args.check:
        print(json.dumps(capabilities(command), ensure_ascii=False))
        return
    state_dir = ROOT / '.local/ai-worker'
    state_dir.mkdir(parents=True, exist_ok=True)
    lock = (state_dir / 'worker.lock').open('a+b')
    if os.name == 'nt':
        import msvcrt
        lock.seek(0)
        lock.write(b'0')
        lock.flush()
        lock.seek(0)
        msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
    else:
        import fcntl
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    relay = Relay(args.connection)
    worker_id = socket.gethostname() + '-gpu-pocket'
    providers, last_check = [], 0
    journal = state_dir / 'pending-result.json'
    pending = json.loads(journal.read_text(encoding='utf-8')) if journal.exists() else None
    if pending and pending['status'] == 'running':
        pending.update(status='failed', output='本机工作进程重启，未自动重试可能已经修改文件的任务。')

    def heartbeat(task_id=None):
        return relay.request('/api/v1/ai/worker/poll', {'worker_id': worker_id,
            'providers': providers, 'running_task_id': task_id})

    print('GPU Pocket desktop worker started', flush=True)
    while True:
        try:
            if time.monotonic() - last_check > 60:
                providers = capabilities(command)
                last_check = time.monotonic()
            if pending:
                relay.request('/api/v1/ai/worker/result', pending)
                journal.unlink(missing_ok=True)
                pending = None
            response = heartbeat()
            task = response.get('task')
            if task:
                pending = {'worker_id': worker_id, 'id': task['id'], 'status': 'running', 'output': ''}
                write_journal(journal, pending)
                print('Task started: ' + task['id'], flush=True)
                if response.get('cancel'):
                    status, output = 'cancelled', '任务已取消。'
                else:
                    try:
                        status, output = execute(command, workspace, task, heartbeat)
                    except (OSError, ValueError) as error:
                        status, output = 'failed', '无法运行本机 AI：' + type(error).__name__
                pending.update(status=status, output=output.encode('utf-8')[-OUTPUT_LIMIT:].decode('utf-8', errors='ignore'))
                write_journal(journal, pending)
                print('Task finished: ' + task['id'] + ' ' + status, flush=True)
                continue
        except (OSError, ValueError) as error:
            # Never log request headers, credentials, prompts, or CLI account details.
            print('Relay unavailable: ' + type(error).__name__, flush=True)
        time.sleep(3)


if __name__ == '__main__':
    main()
