"""Exercise `start --daemon`/`stop`/`status` against a real detached process; no network or AI usage."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/gh-relay'


class DaemonSmoke(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.repo = self.base / 'repo'
        self.repo.mkdir()
        self.git('init', '-b', 'main')
        self.git('config', 'user.email', 'test@example.invalid')
        self.git('config', 'user.name', 'Test')
        (self.repo / 'example.txt').write_text('base\n')
        self.git('add', '.')
        self.git('commit', '-m', 'base')
        (self.repo / '.github').mkdir()
        (self.repo / '.github/gh-relay.yml').write_text(
            'version: 1\nrepo: test/repo\nlabels:\n  codex:review:\n    agent: codex\n    prompt: Review\n'
        )
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        self.fake('gh', '''
import sys, time
a = sys.argv[1:]
if a[:2] == ["webhook", "forward"]:
    time.sleep(3600)
else:
    sys.exit(0)
''')
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'])
        self.addCleanup(self.stop)

    def fake(self, name, body):
        p = self.bin / name
        p.write_text('#!/usr/bin/env python3\n' + body)
        p.chmod(0o755)

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.repo, check=True, capture_output=True, text=True).stdout

    def gh_relay(self, *args):
        return subprocess.run(
            [str(BINARY), '--directory', str(self.repo), *args],
            env=self.env, capture_output=True, text=True,
        )

    def stop(self):
        self.gh_relay('stop')

    def pid_file(self):
        return self.repo / '.git/gh-relay/daemon.pid'

    def alive(self, pid):
        return subprocess.run(['kill', '-0', str(pid)], capture_output=True).returncode == 0

    def forwarder_pid(self):
        # Matches on this test's own fake gh path, so concurrent/unrelated
        # processes elsewhere on the machine can't produce a false match.
        out = subprocess.run(
            ['pgrep', '-f', f'{self.bin / "gh"} webhook forward'],
            capture_output=True, text=True,
        ).stdout.split()
        return int(out[0]) if out else None

    def test_status_without_a_daemon(self):
        result = self.gh_relay('status')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('daemon: not running', result.stdout)

    def test_stop_without_a_daemon_is_a_no_op(self):
        result = self.gh_relay('stop')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('no daemon running', result.stdout)

    def test_start_runs_detached_and_stop_ends_it(self):
        result = self.gh_relay('start', '--daemon', '--port', '0')
        self.assertEqual(result.returncode, 0, result.stderr)
        pid = int(self.pid_file().read_text().strip())
        self.assertTrue(self.alive(pid))

        result = self.gh_relay('status')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'daemon: running (pid {pid})', result.stdout)

        result = self.gh_relay('stop')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'stopped daemon (pid {pid})', result.stdout)
        self.assertFalse(self.pid_file().exists())
        deadline = time.monotonic() + 5
        while self.alive(pid):
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)

        result = self.gh_relay('status')
        self.assertIn('daemon: not running', result.stdout)

    def test_stop_also_ends_the_forwarder_child(self):
        # SIGTERM has no handler in the worker and would skip straight past
        # the Ctrl-C cleanup path, leaving `gh webhook forward` orphaned.
        self.gh_relay('start', '--daemon', '--port', '0')
        deadline = time.monotonic() + 5
        forwarder = self.forwarder_pid()
        while forwarder is None:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
            forwarder = self.forwarder_pid()
        self.gh_relay('stop')
        self.assertFalse(self.alive(forwarder))

    def test_start_twice_is_rejected(self):
        result = self.gh_relay('start', '--daemon', '--port', '0')
        self.assertEqual(result.returncode, 0, result.stderr)
        pid = int(self.pid_file().read_text().strip())
        try:
            result = self.gh_relay('start', '--daemon', '--port', '0')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('daemon already running', result.stderr)
        finally:
            self.gh_relay('stop')
        self.assertFalse(self.alive(pid))

    def test_start_surfaces_immediate_child_failure(self):
        (self.bin / 'gh').unlink()
        result = self.gh_relay('start', '--daemon', '--port', '0')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('daemon exited immediately', result.stderr)
        self.assertFalse(self.pid_file().exists())


if __name__ == '__main__':
    unittest.main()
