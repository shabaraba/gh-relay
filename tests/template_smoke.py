"""Exercise `template list`/`template use` with a fake gh CLI; no network usage."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/gh-relay'


class TemplateSmoke(unittest.TestCase):
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
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        self.fake('gh', '''
import json, os, sys
from pathlib import Path
a = sys.argv[1:]
if a[:2] == ['repo', 'view']:
    print(json.dumps({'nameWithOwner': 'test/repo'}))
elif a[:2] == ['label', 'create']:
    Path(os.environ['TEST_LABELS']).open('a').write(a[2] + '\\n')
else:
    sys.exit(4)
''')
        self.env = dict(
            os.environ,
            PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
            TEST_LABELS=str(self.base / 'labels.txt'),
        )

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

    def test_list_shows_every_bundled_template(self):
        result = self.gh_relay('template', 'list')
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in ('review', 'fix', 'full'):
            self.assertIn(name, result.stdout)

    def test_use_writes_config_with_resolved_repo_and_creates_its_labels(self):
        result = self.gh_relay('template', 'use', 'full')
        self.assertEqual(result.returncode, 0, result.stderr)
        config = (self.repo / '.github/gh-relay.yml').read_text()
        self.assertIn('repo: test/repo', config)
        self.assertNotIn('__REPO__', config)
        labels = (self.base / 'labels.txt').read_text().splitlines()
        self.assertIn('claude:fix', labels)
        self.assertIn('claude:impl', labels)

    def test_use_rejects_unknown_template(self):
        result = self.gh_relay('template', 'use', 'nonexistent')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('unknown template', result.stderr)
        self.assertFalse((self.repo / '.github/gh-relay.yml').exists())

    def test_use_refuses_to_overwrite_existing_config(self):
        (self.repo / '.github').mkdir()
        (self.repo / '.github/gh-relay.yml').write_text('version: 1\nrepo: test/repo\nlabels:\n  codex:review:\n    agent: codex\n    prompt: Review\n')
        result = self.gh_relay('template', 'use', 'review')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('already exists', result.stderr)

    def test_use_overwrite_replaces_existing_config(self):
        (self.repo / '.github').mkdir()
        (self.repo / '.github/gh-relay.yml').write_text('version: 1\nrepo: test/repo\nlabels:\n  custom:label:\n    agent: codex\n    prompt: Review\n')
        result = self.gh_relay('template', 'use', 'review', '--overwrite')
        self.assertEqual(result.returncode, 0, result.stderr)
        config = (self.repo / '.github/gh-relay.yml').read_text()
        self.assertNotIn('custom:label', config)
        self.assertIn('claude:review', config)

    def test_use_merge_keeps_existing_labels_and_adds_new_ones(self):
        (self.repo / '.github').mkdir()
        (self.repo / '.github/gh-relay.yml').write_text(
            'version: 1\nrepo: test/repo\nlabels:\n  claude:review:\n    agent: codex\n    prompt: Existing\n'
        )
        result = self.gh_relay('template', 'use', 'fix', '--merge')
        self.assertEqual(result.returncode, 0, result.stderr)
        config = (self.repo / '.github/gh-relay.yml').read_text()
        self.assertIn('Existing', config)
        self.assertIn('claude:fix', config)
        labels = (self.base / 'labels.txt').read_text().splitlines()
        self.assertIn('claude:fix', labels)

    def test_use_rejects_overwrite_and_merge_together(self):
        result = self.gh_relay('template', 'use', 'review', '--overwrite', '--merge')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.repo / '.github/gh-relay.yml').exists())


if __name__ == '__main__':
    unittest.main()
