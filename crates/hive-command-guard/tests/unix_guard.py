"""Owned fixture processes only; all files live under the caller's temp directory."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest

COMMAND = r'''
import os,pathlib,subprocess,sys,time
root=pathlib.Path(sys.argv[1])
grandchild=subprocess.Popen([sys.executable,'-B','-c','import time; time.sleep(60)'])
(root/'grandchild').write_text(str(grandchild.pid))
time.sleep(60)
'''

def worker(guardian, root):
    child = subprocess.Popen([guardian, '--ready-file', str(Path(root)/'ready'), '--',
                              sys.executable, '-B', '-c', COMMAND, root],
                             stdin=subprocess.PIPE, start_new_session=True)
    # Retain the guardian's liveness writer until this worker dies.
    assert child.stdin
    time.sleep(60)

if len(sys.argv) > 1 and sys.argv[1] == '--worker':
    worker(sys.argv[2], sys.argv[3])
    raise SystemExit(0)

GUARDIAN = sys.argv.pop(1)

class GuardianTests(unittest.TestCase):
    def ready(self, root):
        deadline = time.monotonic()+5
        while time.monotonic() < deadline:
            try:
                receipt=json.loads((root/'ready').read_text())
                int((root/'grandchild').read_text())
                return receipt
            except (FileNotFoundError, ValueError):
                time.sleep(.02)
        self.fail('fixture did not become ready')

    def assert_output_closed(self, stream):
        done=threading.Event()
        def read():
            stream.read()
            done.set()
        thread=threading.Thread(target=read,daemon=True)
        thread.start()
        self.assertTrue(done.wait(5), 'a descendant survived and retained stdout')
        stream.close()

    def parent_death(self, whole_group):
        with tempfile.TemporaryDirectory(prefix='hive-guard-') as directory:
            root=Path(directory)
            parent=subprocess.Popen([sys.executable,'-B',__file__,'--worker',GUARDIAN,str(root)],
                                    stdout=subprocess.PIPE, start_new_session=True)
            receipt=self.ready(root)
            self.assertNotEqual(receipt['guardian'],receipt['command_group'])
            if whole_group: os.killpg(parent.pid,signal.SIGKILL)
            else: parent.kill()
            parent.wait(timeout=3)
            self.assert_output_closed(parent.stdout)

    def test_worker_sigkill_closes_command_and_grandchild_output(self):
        self.parent_death(False)

    def test_worker_group_sigkill_does_not_kill_guard_before_cleanup(self):
        self.parent_death(True)

    def test_explicit_liveness_close_cancels_command_tree(self):
        with tempfile.TemporaryDirectory(prefix='hive-guard-') as directory:
            root=Path(directory)
            guard=subprocess.Popen([GUARDIAN,'--ready-file',str(root/'ready'),'--',
                                    sys.executable,'-B','-c',COMMAND,str(root)],
                                   stdin=subprocess.PIPE,stdout=subprocess.PIPE,start_new_session=True)
            self.ready(root)
            guard.stdin.close()
            self.assertEqual(guard.wait(timeout=3),137)
            self.assert_output_closed(guard.stdout)

    def test_normal_exit_reaps_remaining_descendants_and_preserves_status(self):
        with tempfile.TemporaryDirectory(prefix='hive-guard-') as directory:
            script = "import subprocess,sys; subprocess.Popen([sys.executable,'-B','-c','import time; time.sleep(60)']); raise SystemExit(7)"
            guard=subprocess.Popen([GUARDIAN,'--ready-file',str(Path(directory)/'ready'),'--',
                                    sys.executable,'-B','-c',script],
                                   stdin=subprocess.PIPE,stdout=subprocess.PIPE,start_new_session=True)
            self.assertEqual(guard.wait(timeout=3),7)
            self.assert_output_closed(guard.stdout)
            guard.stdin.close()

    def test_failed_ready_handshake_still_cleans_up(self):
        with tempfile.TemporaryDirectory(prefix='hive-guard-') as directory:
            # An existing directory cannot be opened as the private ready receipt.
            guard=subprocess.Popen([GUARDIAN,'--ready-file',directory,'--',
                                    sys.executable,'-B','-c','import time; time.sleep(60)'],
                                   stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                                   start_new_session=True)
            self.assertNotEqual(guard.wait(timeout=3),0)
            self.assert_output_closed(guard.stdout)
            guard.stdin.close()
            guard.stderr.close()

    def test_command_exit_status_is_preserved(self):
        with tempfile.TemporaryDirectory(prefix='hive-guard-') as directory:
            guard=subprocess.Popen([GUARDIAN,'--ready-file',str(Path(directory)/'ready'),'--',
                                    sys.executable,'-B','-c','raise SystemExit(7)'],
                                   stdin=subprocess.PIPE,start_new_session=True)
            self.assertEqual(guard.wait(timeout=3),7)
            guard.stdin.close()

unittest.main()
