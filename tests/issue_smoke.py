"""Exercise the issue-triggered research/impl flows with fake gh/agent CLIs; no network or AI usage."""
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


class IssueSmoke(unittest.TestCase):
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
        self.origin = self.base / 'origin.git'
        subprocess.run(['git', 'clone', '--bare', str(self.repo), str(self.origin)], check=True, capture_output=True)
        self.git('remote', 'add', 'origin', str(self.origin))
        self.git('push', 'origin', 'main')
        (self.repo / '.github').mkdir()
        (self.repo / '.github/gh-relay.yml').write_text(
            'version: 1\nrepo: test/repo\nlabels:\n'
            '  codex:research:\n    agent: codex\n    kind: research\n    prompt: Research\n'
            '  codex:impl:\n    agent: codex\n    kind: impl\n    prompt: Implement\n'
        )
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        self.fake('gh', '''
import json, os, sys
from pathlib import Path

a = sys.argv[1:]

def issue_payload():
    return {
        "state": "open",
        "title": os.environ.get("TEST_TITLE", "Fix the bug"),
        "body": os.environ.get("TEST_BODY", "Please fix it"),
        "user": {"login": os.environ.get("TEST_AUTHOR", "author")},
        "labels": [{"name": os.environ["TEST_LABEL"]}],
        "pull_request": None,
    }

if a[:2] == ["webhook", "forward"]:
    import time
    time.sleep(60)
elif a[0] == "api" and "--paginate" in a and "/pulls?" in a[-1]:
    print("[[]]")
elif a[0] == "api" and "--paginate" in a and "/issues?" in a[-1]:
    print("[[]]")
elif a[0] == "api" and "/events?" in a[-1]:
    actor = os.environ.get("TEST_LABEL_ACTOR", "test")
    print(json.dumps([[{"event": "labeled", "label": {"name": os.environ["TEST_LABEL"]}, "actor": {"login": actor}}]]))
elif a[0] == "api" and "/collaborators/" in a[-1] and a[-1].endswith("/permission"):
    actor = a[-1].split("/collaborators/")[1].split("/")[0]
    reject_file = Path(os.environ["TEST_BIN"]) / f"reject_{actor}"
    print(json.dumps({"permission": "read" if reject_file.exists() else "write"}))
elif a[0] == "api" and a[-1] == "repos/test/repo":
    print(json.dumps({"default_branch": "main"}))
elif a[0] == "api" and a[-1].endswith(f"/issues/{os.environ['TEST_ISSUE']}"):
    print(json.dumps(issue_payload()))
elif a[:2] == ["issue", "comment"]:
    path = a[a.index("--body-file") + 1]
    Path(os.environ["TEST_COMMENT"]).write_text(Path(path).read_text())
elif a[:2] == ["pr", "create"]:
    path = a[a.index("--body-file") + 1]
    Path(os.environ["TEST_PR_BODY"]).write_text(Path(path).read_text())
    Path(os.environ["TEST_PR_FLAGS"]).write_text(json.dumps(a))
    print("https://github.test/test/repo/pull/99")
else:
    sys.exit(4)
''')
        self.fake('codex', '''
import sys
from pathlib import Path

argv = sys.argv[1:]
assert "--sandbox" in argv
if "workspace-write" in argv:
    Path("CHANGED.txt").write_text("implemented\\n")
    print("Implemented the fix")
else:
    assert "read-only" in argv
    print("Research finding: looks fine")
''')
        self.env = dict(
            os.environ,
            PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
            TEST_ISSUE='1',
            TEST_LABEL='codex:research',
            TEST_COMMENT=str(self.base / 'comment.md'),
            TEST_PR_BODY=str(self.base / 'pr-body.md'),
            TEST_PR_FLAGS=str(self.base / 'pr-flags.json'),
            TEST_BIN=str(self.bin),
        )

    def fake(self, name, body):
        p = self.bin / name
        p.write_text('#!/usr/bin/env python3\n' + body)
        p.chmod(0o755)

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.repo, check=True, capture_output=True, text=True).stdout

    def run_label(self, label, *extra):
        return subprocess.run(
            [str(BINARY), '--directory', str(self.repo), 'run', label, '--issue', '1', *extra],
            env=self.env, capture_output=True, text=True,
        )

    def test_research_preview_and_post(self):
        self.env['TEST_LABEL'] = 'codex:research'
        result = self.run_label('codex:research')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Research finding', result.stdout)
        self.assertFalse((self.base / 'comment.md').exists())
        result = self.run_label('codex:research', '--post')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Research finding', (self.base / 'comment.md').read_text())

    def test_impl_preview_does_not_push_or_open_pr(self):
        self.env['TEST_LABEL'] = 'codex:impl'
        result = self.run_label('codex:impl')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('preview, not pushed', result.stdout)
        self.assertFalse((self.base / 'pr-body.md').exists())
        remotes = subprocess.run(['git', 'ls-remote', 'origin'], cwd=self.repo, capture_output=True, text=True).stdout
        self.assertNotIn('gh-relay/issue-1', remotes)

    def test_impl_post_pushes_branch_and_opens_draft_pr(self):
        self.env['TEST_LABEL'] = 'codex:impl'
        result = self.run_label('codex:impl', '--post')
        self.assertEqual(result.returncode, 0, result.stderr)
        pr_body = (self.base / 'pr-body.md').read_text()
        self.assertTrue(pr_body.startswith('#1\n'))
        self.assertNotIn('Closes #', pr_body)
        flags = json.loads((self.base / 'pr-flags.json').read_text())
        self.assertIn('--draft', flags)
        remotes = subprocess.run(['git', 'ls-remote', 'origin'], cwd=self.repo, capture_output=True, text=True).stdout
        self.assertIn('refs/heads/gh-relay/issue-1', remotes)
        changed = subprocess.run(
            ['git', 'show', 'origin/gh-relay/issue-1:CHANGED.txt'],
            cwd=self.repo, capture_output=True, text=True,
        )
        self.assertEqual(changed.stdout, 'implemented\n')
        markers = list((self.repo / '.git/gh-relay').glob('issue-1-*.json'))
        self.assertEqual(len(markers), 1)

    def test_impl_rejects_untrusted_issue_author(self):
        self.env['TEST_LABEL'] = 'codex:impl'
        (self.bin / 'reject_author').touch()
        result = self.run_label('codex:impl', '--post')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('author lacks write permission', result.stderr)
        self.assertFalse((self.base / 'pr-body.md').exists())
        remotes = subprocess.run(['git', 'ls-remote', 'origin'], cwd=self.repo, capture_output=True, text=True).stdout
        self.assertNotIn('gh-relay/issue-1', remotes)

    def test_webhook_issue_event_dispatches_and_pr_event_ignores_issue_label(self):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        self.env['TEST_LABEL'] = 'codex:research'
        process = subprocess.Popen(
            [str(BINARY), '--directory', str(self.repo), 'start', '--port', str(port)],
            env=self.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
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

        def send_issue_event():
            body = json.dumps({
                'action': 'labeled',
                'repository': {'full_name': 'test/repo'},
                'issue': {'number': 1},
                'label': {'name': 'codex:research'},
            }).encode()
            req = urllib.request.Request(f'http://127.0.0.1:{port}/webhook', data=body, headers={'Content-Type': 'application/json'})
            try:
                with urllib.request.urlopen(req, timeout=2) as result:
                    return result.status
            except urllib.error.HTTPError as error:
                return error.code

        def send_pr_event_with_issue_label():
            # A pull_request-shaped event must not dispatch an issue-kind label.
            body = json.dumps({
                'action': 'labeled',
                'repository': {'full_name': 'test/repo'},
                'pull_request': {},
                'number': 1,
                'label': {'name': 'codex:research'},
            }).encode()
            req = urllib.request.Request(f'http://127.0.0.1:{port}/webhook', data=body, headers={'Content-Type': 'application/json'})
            try:
                with urllib.request.urlopen(req, timeout=2) as result:
                    return result.status
            except urllib.error.HTTPError as error:
                return error.code

        deadline = time.monotonic() + 5
        while send_pr_event_with_issue_label() == 503:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        self.assertEqual(send_pr_event_with_issue_label(), 204)

        deadline = time.monotonic() + 5
        while send_issue_event() == 503:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        deadline = time.monotonic() + 5
        while not (self.base / 'comment.md').exists():
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        self.assertIn('Research finding', (self.base / 'comment.md').read_text())


if __name__ == '__main__':
    unittest.main()
