"""Send a local script or stdin to remote bash, preserving Windows quoting and line endings."""
import argparse
import base64
from pathlib import Path
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('script', nargs='?', help='UTF-8 script path; omitted reads stdin')
parser.add_argument('--ssh', default='2-titan')
args = parser.parse_args()
if args.ssh.startswith('-'):
    parser.error('Invalid SSH alias')
source = Path(args.script).read_text(encoding='utf-8-sig') if args.script else sys.stdin.read()
payload = base64.b64encode(source.lstrip('\ufeff').replace('\r\n', '\n').encode()).decode()
command = 'python3 -c "import base64,subprocess;raise SystemExit(subprocess.run([\'bash\',\'-s\'],input=base64.b64decode(\'' + payload + '\')).returncode)"'
raise SystemExit(subprocess.run(['ssh', '-o', 'BatchMode=yes', args.ssh, command]).returncode)
