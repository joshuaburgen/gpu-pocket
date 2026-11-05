import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import ai_worker


class WorkerTests(unittest.TestCase):
    def test_read_only_and_edit_tasks_select_sandbox_and_never_shell_interpolate(self):
        task = {'provider': 'codex', 'prompt': '$(echo unsafe) & echo unsafe', 'allow_edits': False}
        args = ai_worker.build_command(['codex.exe'], Path('repo'), task, Path('answer'))
        self.assertIn('read-only', args)
        self.assertNotIn(task['prompt'], args)
        task['allow_edits'] = True
        self.assertIn('workspace-write', ai_worker.build_command(['codex.exe'], Path('repo'), task, Path('answer')))

    def test_actual_child_returns_answer_and_cancel_stops_child(self):
        sleep = time.sleep
        task = {'id': 'test', 'provider': 'codex', 'prompt': 'question', 'allow_edits': False}
        with tempfile.TemporaryDirectory() as directory:
            def command(_command, _workspace, _task, output):
                return [sys.executable, '-c', 'import pathlib,sys;sys.stdin.read();pathlib.Path(sys.argv[1]).write_text("answer")', str(output)]
            with patch.object(ai_worker, 'build_command', command), patch.object(ai_worker.time, 'sleep', lambda _: sleep(.01)):
                status, output = ai_worker.execute([], Path(directory), task, lambda _: {'cancel': False})
            self.assertEqual((status, output), ('completed', 'answer'))
            def slow_command(*_):
                return [sys.executable, '-c', 'import time;time.sleep(60)']
            with patch.object(ai_worker, 'build_command', slow_command):
                status, _ = ai_worker.execute([], Path(directory), task, lambda _: {'cancel': True})
            self.assertEqual(status, 'cancelled')


if __name__ == '__main__':
    unittest.main()
