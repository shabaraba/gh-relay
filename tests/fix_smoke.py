"""Exercise the PR-triggered fix flow (write-capable, pushes to the PR's own
branch rather than opening a new one) with fake gh/agent CLIs; no network or
AI usage."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/gh-relay'


class FixSmoke(unittest.TestCase):
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
        self.git('checkout', '-b', 'pr-branch')
        (self.repo / 'example.txt').write_text('changed\n')
        self.git('commit', '-am', 'head')
        self.sha = self.git('rev-parse', 'HEAD').strip()
        self.origin = self.base / 'origin.git'
        subprocess.run(['git', 'clone', '--bare', str(self.repo), str(self.origin)], check=True, capture_output=True)
        subprocess.run(['git', '--git-dir', str(self.origin), 'update-ref', 'refs/pull/1/head', self.sha], check=True)
        self.git('remote', 'add', 'origin', str(self.origin))
        (self.repo / '.github').mkdir()
        (self.repo / '.github/gh-relay.yml').write_text(
            'version: 1\nrepo: test/repo\nlabels:\n  codex:fix:\n    agent: codex\n    kind: fix\n    prompt: Fix\n'
        )
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        self.fake('gh', '''
import json, os, sys
from pathlib import Path
a = sys.argv[1:]
if a[:2] == ['webhook', 'forward']:
    import time
    time.sleep(60)
elif a[0] == 'api' and 'permission' in a[-1]:
    print(json.dumps({'permission': 'write' if not Path(os.environ['TEST_BIN']).joinpath('reject').exists() else 'read'}))
elif a[0] == 'api':
    print(json.dumps({
        'state': 'open',
        'head': {
            'sha': os.environ['TEST_SHA'],
            'ref': 'pr-branch',
            'repo': {'full_name': os.environ.get('TEST_HEAD_REPO', 'test/repo')},
        },
        'labels': [{'name': 'codex:fix'}],
        'user': {'login': 'author'},
    }))
elif a[:2] == ['pr', 'comment']:
    Path(os.environ['TEST_COMMENT']).write_text(Path(a[a.index('--body-file') + 1]).read_text())
elif a[:2] == ['pr', 'edit']:
    Path(os.environ['TEST_REMOVE_LABEL']).write_text(json.dumps(a))
else:
    sys.exit(4)
''')
        self.fake('codex', '''
import sys
from pathlib import Path
assert '--sandbox' in sys.argv and 'workspace-write' in sys.argv
Path('FIXED.txt').write_text('addressed\\n')
print('Addressed the feedback')
''')
        self.env = dict(
            os.environ,
            PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
            TEST_SHA=self.sha,
            TEST_COMMENT=str(self.base / 'comment.md'),
            TEST_BIN=str(self.bin),
            TEST_REMOVE_LABEL=str(self.base / 'remove-label.json'),
        )

    def fake(self, name, body):
        p = self.bin / name
        p.write_text('#!/usr/bin/env python3\n' + body)
        p.chmod(0o755)

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.repo, check=True, capture_output=True, text=True).stdout

    def run_fix(self, *extra):
        return subprocess.run(
            [str(BINARY), '--directory', str(self.repo), 'run', 'codex:fix', '--pr', '1', *extra],
            env=self.env, capture_output=True, text=True,
        )

    def test_preview_does_not_push(self):
        result = self.run_fix()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('preview, not pushed', result.stdout)
        self.assertFalse((self.base / 'comment.md').exists())
        remotes = self.git('ls-remote', 'origin')
        self.assertNotIn('FIXED.txt', remotes)

    def test_post_pushes_to_the_prs_own_branch_and_comments(self):
        result = self.run_fix('--post')
        self.assertEqual(result.returncode, 0, result.stderr)
        comment = (self.base / 'comment.md').read_text()
        self.assertIn('Pushed', comment)
        self.assertIn('Addressed the feedback', comment)
        remove_label = json.loads((self.base / 'remove-label.json').read_text())
        self.assertEqual(remove_label[:3], ['pr', 'edit', '1'])
        self.assertIn('codex:fix', remove_label)
        remotes = subprocess.run(['git', 'ls-remote', 'origin'], cwd=self.repo, capture_output=True, text=True).stdout
        self.assertIn('refs/heads/pr-branch', remotes)
        changed = subprocess.run(
            ['git', 'show', 'origin/pr-branch:FIXED.txt'],
            cwd=self.repo, capture_output=True, text=True,
        )
        self.assertEqual(changed.stdout, 'addressed\n')

    def test_rejects_untrusted_pr_author(self):
        (self.bin / 'reject').touch()
        result = self.run_fix('--post')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('author lacks write permission', result.stderr)
        self.assertFalse((self.base / 'comment.md').exists())

    def test_rejects_fork_pr(self):
        self.env['TEST_HEAD_REPO'] = 'someone-else/repo'
        result = self.run_fix('--post')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('fork', result.stderr)
        self.assertFalse((self.base / 'comment.md').exists())


if __name__ == '__main__':
    unittest.main()
