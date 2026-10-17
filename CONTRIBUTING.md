# Contributing

## Issues

Use the standard GitHub labels to triage: `bug`, `enhancement`, `documentation`,
`question`, `good first issue`, `help wanted`, `duplicate`, `invalid`, `wontfix`.
Search existing issues before opening a new one.

This repository also carries gh-relay's own workflow labels —
`claude:review`, `codex:review`, `copilot:review`, `claude:research`,
`claude:impl` — used to dogfood gh-relay on its own development. These are
driven by a maintainer's local `gh relay start` worker; applying one yourself
does nothing unless that worker happens to be running, and `impl`/`research`
additionally require the issue author to have write access (see
[Execution boundaries and limits](README.md#execution-boundaries-and-limits)
in the README). Don't rely on them to get an issue worked on — open a regular
issue and a maintainer will pick it up.

## Pull requests

- Keep changes focused; unrelated cleanup belongs in its own PR.
- Run the full check suite locally before opening a PR:

  ```sh
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test --locked
  python3 tests/smoke.py
  python3 tests/issue_smoke.py
  python3 tests/daemon_smoke.py
  python3 tests/fix_smoke.py
  python3 tests/template_smoke.py
  python3 tests/list_smoke.py
  ```

- Commits on `main` must follow [Semantic Commit Messages](https://gist.github.com/joshbuchea/6f47e86d2510bce28f8e7f42ae84c716)
  (`feat:`, `fix:`, `chore:`, ...): [release-please](https://github.com/googleapis/release-please)
  reads them to cut releases, so a PR with a malformed title/commits won't be
  versioned correctly once merged.
- Describe the user-facing behavior change, not just the implementation.

## Security

Do not open a public issue for a security vulnerability — see
[SECURITY.md](SECURITY.md).
