# Security Policy

gh-relay runs local coding agent CLIs against your repositories, and its
`fix`/`impl` workflows grant those agents write access (pushing commits,
opening PRs). See
[Execution boundaries and limits](README.md#execution-boundaries-and-limits)
in the README for the trust model and known limits before reporting — some
behavior described there (e.g. "use v0.1 with repositories and contributors
you trust") is a documented constraint, not a vulnerability.

## Reporting a vulnerability

Please report security issues privately through
[GitHub Security Advisories](https://github.com/shabaraba/gh-relay/security/advisories/new)
rather than opening a public issue. Include steps to reproduce and the
version of gh-relay affected.

## Scope

In scope: anything that lets an attacker bypass the permission checks
described in the README (e.g. triggering `fix`/`impl` without the required
write access, escaping the worktree sandbox, exfiltrating credentials the
worker has access to) or that affects the webhook listener's validation of
repository/label/actor state.

Out of scope: behavior of the underlying agent CLIs (Claude Code, Codex,
Copilot CLI) themselves — report those upstream.
