# gh-relay

Run local coding agents from GitHub PR and issue labels. Written in Rust; agent authentication stays with the agent CLI. No AI API client or self-hosted Actions runner is required.

**v0.1 supports PR review, PR fix, issue research and issue implementation with Codex, Claude Code and GitHub Copilot CLI.** LSP analysis, service installation and multiple repositories are future work.

## Install

Install Git, GitHub CLI and at least one agent CLI. Authenticate `gh` and your selected agent separately.

```sh
gh extension install shabaraba/gh-relay
gh extension install cli/gh-webhook
```

This downloads a precompiled binary for your platform (macOS or Linux, amd64 or arm64) from the latest release; no local Rust toolchain is needed. To build from source instead (e.g. to test an unreleased change):

```sh
git clone https://github.com/shabaraba/gh-relay.git
cd gh-relay
cargo build --release --locked
cp target/release/gh-relay ./gh-relay
gh extension install .
```

Then, in a **primary checkout** of the repository you want to review:

```sh
gh relay doctor
gh relay init
# Inspect .github/gh-relay.yml, then commit it.
gh relay start
```

`gh relay template list` shows bundled alternatives to `init`'s review-only scaffold (`fix`, `full`), covering the write-capable workflows described below; see [Templates](#templates).

Apply `codex:review`, `claude:review` or `copilot:review` to an open PR, or a `fix`-kind label such as `claude:fix` (see below) to an open PR, or `codex:research`/`codex:impl`-style labels (see below) to an open issue. The worker creates a detached worktree, runs the selected local CLI and delivers the result. GitHub's webhook forwarder is intended for development/testing, not a production delivery service.

`gh relay start` runs in the foreground; Ctrl-C stops the worker and webhook forwarder. Pass `--daemon`/`-d` to run it detached instead, so it keeps responding after the shell session that started it ends:

```sh
gh relay start -d                # prints the pid and log path, then returns
gh relay status                  # daemon: running (pid N), plus persisted markers
gh relay stop                    # sends the same signal as Ctrl-C and waits for it to exit
```

`stop` sends SIGINT rather than SIGTERM, since SIGINT is what the worker's Ctrl-C handler listens for; that handler is also what kills the `gh webhook forward` child cleanly, so a plain SIGTERM would leave it running as an orphan. Only one worker (foreground or daemon) may hold a checkout's lock at a time; starting a second daemon while one is already running is rejected before anything is spawned. If the spawned process dies immediately (bad config, port already in use), `start --daemon` reports that failure directly instead of silently leaving a dead pid file.

A recovery scan runs at startup and every five minutes, over both open PRs and open issues. It finds labels applied while the worker was disconnected. Only labels last applied by a collaborator with write/maintain/admin permission are eligible. A local lock prevents two workers sharing the same checkout; successful label/number/content combinations are persisted under `.git/gh-relay/`. The label itself is removed once a result is delivered, so an absent label means the job finished; to request another automatic run at the same content, re-apply the label, or stop the worker and remove the corresponding JSON marker after inspecting `gh relay status`, or run manually.

```sh
gh relay run codex:review --pr 123          # print result only
gh relay run claude:review --pr 123 --post
gh relay run codex:research --issue 45      # print result only
gh relay run codex:impl --issue 45 --post   # pushes a branch and opens a draft PR
gh relay status
```

`--directory /path/to/repo` works with every command. Run one process per repository, with distinct `start --port` values.

## Configuration

`.github/gh-relay.yml` is read from the local checkout when the process starts, never from the PR head or issue body. Fetch and inspect configuration changes before restarting.

```yaml
version: 1
repo: owner/repository
labels:
  codex:review:
    agent: codex
    kind: review # default; reads a PR diff, posts a PR comment
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
  claude:fix:
    agent: claude
    kind: fix # write-capable; addresses an open PR's own feedback, pushes to its existing branch
    prompt: Address the PR's existing review comments and discussion, then push a fix.
  claude:research:
    agent: claude
    kind: research # read-only; investigates an issue, posts an issue comment
    prompt: Investigate this issue against the current codebase and summarize findings.
  claude:impl:
    agent: claude
    kind: impl # write-capable; implements an issue, pushes a branch, opens a draft PR
    prompt: Implement what this issue asks for.
```

The label names are arbitrary configuration keys. Agent-specific arguments stay in the Rust adapters. No credentials are stored by gh-relay. Installed CLI versions must support the adapter flags; `doctor` prints versions but does not establish agent authentication or capability compatibility. A failed agent invocation fails the job without posting an empty review or comment. `gh relay init` only scaffolds `review` labels; add `fix`/`research`/`impl` labels by hand (or start from a [template](#templates)) and re-read the execution boundaries below first, since `fix` and `impl` grant write access. Run `gh relay setup` afterwards to create (or update) the GitHub label for every workflow currently in `.github/gh-relay.yml`, including the ones you added by hand.

## PR-triggered `fix`

`fix` addresses an already-open PR's own discussion and review feedback, then pushes a commit to the PR's **existing** branch — it never opens a new PR, since one already exists. It shares `review`'s dispatch path (checks out the PR's head SHA in a detached worktree) but grants write access the same way `impl` does, and tells the agent to read the PR's discussion itself (e.g. `gh pr view <pr> --comments` or `--json reviews,comments`) rather than having gh-relay inline it into the prompt, matching [Reading extra context yourself](#reading-extra-context-yourself) below.

Because the PR's diff and comments can come from anyone who can open one, `fix` requires **both** the collaborator who applied the label **and** the PR's author to have write/maintain/admin permission, matching `impl`'s trust requirement for issues. `fix` also refuses to run when the PR's head branch lives in a fork rather than this repository, since gh-relay only has push access here; use `review` for fork PRs instead. Nothing is pushed unless `--post` is passed; without it, you get a preview of the agent's summary and changed files only. Immediately before pushing, gh-relay re-checks that the PR is still open and its head SHA has not moved, aborting otherwise.

## Issue-triggered `research` and `impl`

`research` mirrors `review`'s read-only posture but targets an issue instead of a PR: it checks out the repository's default branch, gives the agent read-only access to investigate the issue against the current codebase, and posts the result as an issue comment.

`impl` grants the agent write access to implement the issue, then gh-relay itself (not the agent) commits the changes, pushes a `gh-relay/issue-<number>` branch and opens a **draft** PR referencing the issue (`#<number>`, never an auto-closing keyword — a human decides whether to merge and close it). Nothing is pushed unless `--post` is passed; without it, you get a preview of the agent's summary and changed files only.

Because an issue's body can be written by anyone who can open an issue, `impl` requires **both** the collaborator who applied the label **and** the issue's author to have write/maintain/admin permission on the repository, narrowing who can get write-capable agent execution to run over content they influence. `research` only requires the former, matching `review`.

Write-capable execution (shared by `impl` and `fix`) differs per adapter:

- **Claude** keeps Edit/Write pre-approved and Bash available, but Bash calls are only allowed when the target repository's own committed `.claude/settings.json` (`--setting-sources project`, so your personal/global settings are never consulted) already permits them, or when they match the read-only `gh`/`git` allowlist below; anything else is denied automatically (`--permission-prompts none`). MCP stays disabled, as in `review`.
- **Codex** runs with `--sandbox workspace-write` instead of `--sandbox read-only`.
- **Copilot** gets its `write` tool enabled in addition to the read-only allowlist below; anything else stays denied.

Issues have no SHA to pin to, so gh-relay hashes the title and body it read when the label was applied and re-checks that hash (and that the issue is still open) immediately before posting a comment or pushing a branch, aborting if either changed in the meantime.

### Reading extra context yourself

Rather than gh-relay pre-fetching and inlining issue/PR comments and commit history into the prompt, every workflow (`review`, `fix`, `research`, `impl`) tells the agent its PR/issue number and lets it pull more context on demand with a fixed set of non-mutating commands: `gh issue view`, `gh issue list`, `gh pr view`, `gh pr list`, `gh pr diff`, `git log`, `git show`, `git diff`, `git blame`. Claude and Copilot enforce this with per-command allowlists (`Bash(gh pr view:*)`, `bash(git log:*)`, etc., verified to auto-deny anything unmatched with no prompt); Codex's sandbox already permits running any command that doesn't write to disk, so no extra flag is needed there. Nothing on this list can change GitHub or repository state, so it is granted unconditionally, independent of the target repository's own `.claude/settings.json`. `fix` relies on this to see the PR's review comments and discussion itself, since gh-relay does not inline them into the prompt.

`impl` and `fix` additionally frame the work as a compressed, single-turn feature-development pass: understand existing conventions, design the smallest fitting change, implement, then self-review before finishing. Since no one is available to answer mid-task questions, the agent is told to default to the most conventional and minimal choice when something is ambiguous, and to state any point it genuinely can't resolve in its final summary — which becomes the pull request description (`impl`) or a PR comment (`fix`) — instead of leaving it silently unresolved.

A label is removed from the PR/issue after a result is successfully delivered (a posted comment; a pushed commit and comment for `fix`; or a pushed branch and opened PR for `impl`), so its absence signals that the job is done; failure to remove it is logged but does not fail the job. This only happens with `--post` (or automatically, which always implies `--post`); preview runs never touch labels.

## Execution boundaries and limits

Use v0.1 with repositories and contributors you trust — `fix` and `impl`'s write access makes this more important, not less. Worktrees isolate files from your current checkout; they are not an OS security boundary. CLI/project settings and extensions can affect an agent's behavior. For `review` and `research`, Codex requests its read-only sandbox, Claude gets Read/Grep/Glob plus the read-only `gh`/`git` allowlist with an empty MCP configuration, and Copilot gets view/grep/glob plus the same allowlist with write denied; `fix` and `impl`'s write-capable adapters are described above. Tool restrictions are not interchangeable with filesystem sandboxes. A repository's own `.claude/settings.json` governs what Claude's `fix`/`impl` runs can do with Bash beyond the read-only allowlist; review it like any other trusted configuration before enabling those labels.

No project builds, package installation or tests are run. The agent timeout is 30 minutes, GitHub/git command timeout is two minutes. Diffs, issue bodies, prompts, PR comments and issue comments above their documented size limits are rejected. The worker processes jobs serially. Busy webhook requests receive HTTP 503 and are recovered through the periodic scan. Failed jobs retry on the next scan. A crash after posting and before recording completion can produce a duplicate comment, a duplicate push attempt for `fix` (rejected as non-fast-forward once the first push lands), or for `impl`, a duplicate branch/PR attempt (rejected since the branch already exists); v0.1 does not promise exactly-once delivery. The localhost listener validates repository, current labels and the historical label actor (and, for `fix`/`impl`, the PR/issue author) through GitHub API; it must not be exposed publicly.

Worktrees live under `.gh-relay-worktrees/` in the primary checkout, not under `.git/`: Claude Code classifies any path inside `.git/` as sensitive and silently denies writes there, which would break every `fix`/`impl` run. Add `.gh-relay-worktrees/` to the target repository's own `.gitignore`. Worktrees are removed after execution. An interrupted run may leave one behind: inspect `git worktree list`, remove the abandoned worktree with `git worktree remove`, then retry. A newer PR SHA, or an issue edited after the label was applied, prevents delivering a stale result. Multiple machines do not share deduplication state.

## Templates

`gh relay template list` prints the bundled templates and `gh relay template use <name>` writes `.github/gh-relay.yml` from one and creates its labels, the same way `init` does for its review-only default. It refuses to run outside a primary checkout or over an existing configuration, matching `init`.

- `review`: `codex:review`, `claude:review`, `copilot:review` — the same labels `init` scaffolds.
- `fix`: `claude:review` and `claude:fix` — review a PR, then address that feedback and push a fix to it.
- `full`: every workflow above plus `claude:research` and `claude:impl`.

Edit the written file before committing it, same as any other `gh-relay.yml`.

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
python3 tests/smoke.py
python3 tests/issue_smoke.py
python3 tests/daemon_smoke.py
python3 tests/fix_smoke.py
python3 tests/template_smoke.py
```

Commits on `main` must follow [Semantic Commit Messages](https://gist.github.com/joshbuchea/6f47e86d2510bce28f8e7f42ae84c716) (`feat:`, `fix:`, ...): [release-please](https://github.com/googleapis/release-please) reads them to open a release PR, and merging that PR tags a release and triggers a build that attaches a precompiled binary per platform.

MIT licensed. Rust makes startup and idle overhead small; review latency is mostly agent inference and GitHub/network operations. No performance measurements are claimed.
