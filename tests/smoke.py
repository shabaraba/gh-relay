"""Exercise real git worktrees with fake gh/agent CLIs; no network or AI usage."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import socket
import urllib.request
import urllib.error
import unittest

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/gh-relay'

class ReviewSmoke(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.repo = self.base / 'repo'
        self.repo.mkdir()
        self.git('init')
        self.git('config', 'user.email', 'test@example.invalid')
        self.git('config', 'user.name', 'Test')
        (self.repo / 'example.txt').write_text('base\n')
        self.git('add', '.')
        self.git('commit', '-m', 'base')
        (self.repo / 'example.txt').write_text('changed\n')
        self.git('commit', '-am', 'head')
        self.sha = self.git('rev-parse', 'HEAD').strip()
        self.origin = self.base / 'origin.git'
        subprocess.run(['git', 'clone', '--bare', str(self.repo), str(self.origin)], check=True, capture_output=True)
        subprocess.run(['git', '--git-dir', str(self.origin), 'update-ref', 'refs/pull/1/head', self.sha], check=True)
        self.git('remote', 'add', 'origin', str(self.origin))
        (self.repo / '.github').mkdir()
        (self.repo / '.github/gh-relay.yml').write_text('version: 1\nrepo: test/repo\nlabels:\n  codex:review:\n    agent: codex\n    prompt: Review\n')
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        self.fake('gh', '''
import json, os, sys
from pathlib import Path
a = sys.argv[1:]
if a[:2] == ['webhook','forward']:
    import time
    time.sleep(60)
elif a[0] == 'api' and 'permission' in a[-1]:
    print(json.dumps({'permission': 'write' if not Path(os.environ['TEST_BIN']).joinpath('reject').exists() else 'read'}))
elif a[0] == 'api' and '/events?' in a[-1]:
    print(json.dumps([[{'event':'labeled','label':{'name':'codex:review'},'actor':{'login':'test'}}]]))
elif a[0] == 'api' and '/pulls?' in a[-1]:
    print('[[]]')
elif a[0] == 'api':
    print(json.dumps({'state':'open','head':{'sha':os.environ['TEST_SHA']},'labels':[{'name':'codex:review'}]}))
elif a[:2] == ['pr','diff']:
    print('diff --git a/example.txt b/example.txt\\n-base\\n+changed')
elif a[:2] == ['pr','comment']:
    Path(os.environ['TEST_COMMENT']).write_text(Path(a[a.index('--body-file')+1]).read_text())
elif a[:2] == ['pr','edit']:
    Path(os.environ['TEST_REMOVE_LABEL']).write_text(json.dumps(a))
else:
    sys.exit(4)
''')
        self.fake('codex', '''
import os, sys
from pathlib import Path
assert '--sandbox' in sys.argv and 'read-only' in sys.argv
assert Path('example.txt').read_text() == 'changed\\n'
if os.environ.get('TEST_FAIL'):
    print('fake agent failure', file=sys.stderr)
    sys.exit(2)
if os.environ.get('TEST_STALE'):
    # Change only fake API's response for the final metadata read.
    gh = Path(os.environ['TEST_BIN']) / 'gh'
    text = gh.read_text().replace("os.environ['TEST_SHA']", "'0' * 40")
    gh.write_text(text)
print('Actionable test finding')
''')
        self.env = dict(os.environ, PATH=str(self.bin)+os.pathsep+os.environ['PATH'], TEST_SHA=self.sha, TEST_COMMENT=str(self.base/'comment.md'), TEST_BIN=str(self.bin), TEST_REMOVE_LABEL=str(self.base/'remove-label.json'))

    def fake(self, name, body):
        p = self.bin / name
        p.write_text('#!/usr/bin/env python3\n'+body)
        p.chmod(0o755)

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.repo, check=True, capture_output=True, text=True).stdout

    def run_review(self, *extra):
        return subprocess.run([str(BINARY), '--directory', str(self.repo), 'run', 'codex:review', '--pr', '1', *extra], env=self.env, capture_output=True, text=True)

    def test_preview_and_post(self):
        result = self.run_review()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Actionable test finding', result.stdout)
        self.assertFalse((self.base/'comment.md').exists())
        self.assertFalse((self.base/'remove-label.json').exists())
        result = self.run_review('--post')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(self.sha, (self.base/'comment.md').read_text())
        self.assertEqual(self.git('worktree', 'list').count('\n'), 1)
        remove_label = json.loads((self.base/'remove-label.json').read_text())
        self.assertEqual(remove_label[:3], ['pr', 'edit', '1'])
        self.assertIn('codex:review', remove_label)

    def test_failed_agent_does_not_post_and_cleans_up(self):
        self.env['TEST_FAIL'] = '1'
        result = self.run_review('--post')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.base/'comment.md').exists())
        self.assertEqual(self.git('worktree', 'list').count('\n'), 1)
        self.assertFalse(list((self.repo/'.git/gh-relay').glob('*.json')))
        self.assertFalse((self.base/'remove-label.json').exists())

    def test_changed_head_does_not_post(self):
        self.env['TEST_STALE'] = '1'
        result = self.run_review('--post')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('PR changed', result.stderr)
        self.assertFalse((self.base/'comment.md').exists())
        self.assertFalse((self.base/'remove-label.json').exists())

    def test_webhook_authorization_and_duplicate_delivery(self):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        process = subprocess.Popen([str(BINARY), '--directory', str(self.repo), 'start', '--port', str(port)], env=self.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(lambda: (process.terminate(), process.wait(timeout=5)) if process.poll() is None else None)
        deadline = time.monotonic() + 5
        while True:
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1):
                    break
            except OSError:
                if time.monotonic() > deadline:
                    self.fail('listener did not start')
                time.sleep(.02)
        def send(repo='test/repo'):
            body = json.dumps({'action':'labeled','repository':{'full_name':repo},'pull_request':{},'number':1,'label':{'name':'codex:review'}}).encode()
            req = urllib.request.Request(f'http://127.0.0.1:{port}/webhook', data=body, headers={'Content-Type':'application/json'})
            try:
                with urllib.request.urlopen(req, timeout=2) as result:
                    return result.status
            except urllib.error.HTTPError as error:
                return error.code
        deadline = time.monotonic()+5
        while send('other/repo') == 503:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        self.assertEqual(send('other/repo'), 204)
        (self.bin/'reject').touch()
        # Retry if the startup catch-up briefly owns the worker lock.
        deadline = time.monotonic()+5
        while send() == 503:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        time.sleep(.1)
        self.assertFalse((self.base/'comment.md').exists())
        (self.bin/'reject').unlink()
        deadline = time.monotonic()+5
        while send() == 503:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        while not list((self.repo/'.git/gh-relay').glob('*.json')):
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        comment = self.base/'comment.md'
        self.assertTrue(comment.exists())
        modified = comment.stat().st_mtime_ns
        self.assertEqual(send(), 202)
        time.sleep(.1)
        self.assertEqual(comment.stat().st_mtime_ns, modified)

if __name__ == '__main__':
    unittest.main()
