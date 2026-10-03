# gh-relay

Run local coding agents from GitHub PR labels. Written in Rust; agent authentication stays with the agent CLI. No AI API client or self-hosted Actions runner is required.

**v0.1 supports reviews with Codex, Claude Code and GitHub Copilot CLI.** Automatic edits, issue implementation, LSP analysis, service installation and multiple repositories are future work.

## Install from source

Install Rust, Git, GitHub CLI and at least one agent CLI. Authenticate `gh` and your selected agent separately.

```sh
git clone https://github.com/shabaraba/gh-relay.git
cd gh-relay
cargo build --release --locked
cp target/release/gh-relay ./gh-relay
gh extension install .
gh extension install cli/gh-webhook
```

Then, in a **primary checkout** of the repository you want to review:

```sh
gh relay doctor
gh relay init
# Inspect .github/gh-relay.yml, then commit it.
gh relay start
```

Apply `codex:review`, `claude:review` or `copilot:review` to an open PR. The foreground process creates a detached worktree at the exact PR head, runs the selected local CLI and posts a comment. Ctrl-C stops the worker and webhook forwarder. GitHub's webhook forwarder is intended for development/testing, not a production delivery service. Keep your Mac awake for immediate responses.

A recovery scan runs at startup and every five minutes. It finds labels applied while the worker was disconnected. Only labels last applied by a collaborator with write/maintain/admin permission are eligible. A local lock prevents two workers sharing the same checkout; successful label/PR/head combinations are persisted under `.git/gh-relay/`. To request another automatic run at the same SHA, stop the worker and remove the corresponding JSON marker after inspecting `gh relay status`, or run manually.

```sh
gh relay run codex:review --pr 123        # print result only
gh relay run claude:review --pr 123 --post
gh relay status
```

`--directory /path/to/repo` works with every command. Run one process per repository, with distinct `start --port` values. `status` requires the worker to be stopped in v0.1.

## Configuration

`.github/gh-relay.yml` is read from the local checkout when the process starts, never from the PR head. Fetch and inspect configuration changes before restarting.

```yaml
version: 1
repo: owner/repository
labels:
  codex:review:
    agent: codex
    prompt: |
      Review bugs, regressions and unnecessary complexity.
      Explain user-facing impact and cite paths and lines.
    # model: choose-a-model-supported-by-your-installed-cli
  claude:review:
    agent: claude
    prompt: Review actionable bugs and regressions.
  copilot:review:
    agent: copilot
    prompt: Review actionable bugs and regressions.
```

The label names are arbitrary configuration keys. Agent-specific arguments stay in the Rust adapters. No credentials are stored by gh-relay. Installed CLI versions must support the adapter flags; `doctor` prints versions but does not establish agent authentication or capability compatibility. A failed agent invocation fails the job without posting an empty review.

## Execution boundaries and limits

Use v0.1 with repositories and contributors you trust. Worktrees isolate files from your current checkout; they are not an OS security boundary. CLI/project settings and extensions can affect an agent's behavior. Codex requests its read-only sandbox. Claude gets Read/Grep/Glob only with an empty MCP configuration. Copilot gets view/grep/glob, with shell/write denied. Tool restrictions are not interchangeable with filesystem sandboxes.

No project builds, package installation or tests are run. The agent timeout is 30 minutes, GitHub/git command timeout is two minutes. Diffs above 60 KB and comments above 60 KB are rejected. The worker processes jobs serially. Busy webhook requests receive HTTP 503 and are recovered through the periodic scan. Failed jobs retry on the next scan. A crash after posting and before recording completion can produce a duplicate comment; v0.1 does not promise exactly-once delivery. The localhost listener validates repository, current labels and the historical label actor through GitHub API; it must not be exposed publicly.

Review worktrees are removed after execution. An interrupted run may leave a worktree: inspect `git worktree list`, remove the abandoned worktree with `git worktree remove`, then retry. A newer PR SHA prevents posting a stale review. Multiple machines do not share deduplication state.

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

MIT licensed. Rust makes startup and idle overhead small; review latency is mostly agent inference and GitHub/network operations. No performance measurements are claimed.
