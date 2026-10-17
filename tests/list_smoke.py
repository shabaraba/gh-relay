"""Exercise `list` against a real detached process; no network or AI usage."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/gh-relay'


class ListSmoke(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.home = self.base / 'home'
        self.home.mkdir()
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
        # HOME is isolated to a tmp dir so this test's global instance
        # registry never touches (or collides with) the real user's
        # ~/.gh-relay/instances.
        self.env = dict(
            os.environ,
            PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
            HOME=str(self.home),
        )
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

    def test_list_without_any_running_relay(self):
        result = self.gh_relay('list')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('no gh-relay processes running', result.stdout)

    def test_list_shows_a_running_instance_and_drops_it_after_stop(self):
        result = self.gh_relay('start', '--daemon', '--port', '0')
        self.assertEqual(result.returncode, 0, result.stderr)
        pid = int((self.repo / '.git/gh-relay/daemon.pid').read_text().strip())

        deadline = time.monotonic() + 5
        while True:
            result = self.gh_relay('list')
            self.assertEqual(result.returncode, 0, result.stderr)
            if f'pid {pid}' in result.stdout:
                break
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        self.assertIn('test/repo', result.stdout)
        self.assertIn(os.path.realpath(self.repo), result.stdout)
        self.assertIn('port 0', result.stdout)

        self.gh_relay('stop')
        result = self.gh_relay('list')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(f'pid {pid}', result.stdout)


if __name__ == '__main__':
    unittest.main()
