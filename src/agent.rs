use crate::config::Workflow;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Stdio};
use tokio::{
    process::Command,
    time::{timeout, Duration},
};

/// Non-mutating `gh`/`git` commands agents may run themselves to pull extra
/// context (discussion history, commit history) by issue/PR number, instead
/// of gh-relay pre-fetching and inlining it into the prompt. Safe to grant
/// unconditionally: none of these can change GitHub or repository state.
const READ_ONLY_COMMANDS: &[&str] = &[
    "gh issue view",
    "gh issue list",
    "gh pr view",
    "gh pr list",
    "gh pr diff",
    "git log",
    "git show",
    "git diff",
    "git blame",
];

#[derive(Clone, Deserialize, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Claude,
    Copilot,
}

impl Agent {
    pub fn executable(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Copilot => "copilot",
        }
    }
    /// `write` grants file-edit (and, per adapter, shell) capability for `impl` workflows.
    /// Read-only workflows (`review`, `research`) must pass `write: false`.
    pub fn args(&self, prompt: &str, write: bool) -> Vec<String> {
        match self {
            Self::Codex => {
                // The sandbox governs filesystem writes only; READ_ONLY_COMMANDS
                // need no extra flag since nothing here writes to disk.
                let sandbox = if write {
                    "workspace-write"
                } else {
                    "read-only"
                };
                vec!["exec", "--sandbox", sandbox, "--color", "never", prompt]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            }
            Self::Claude => {
                let mut allowed: Vec<String> = READ_ONLY_COMMANDS
                    .iter()
                    .map(|c| format!("Bash({c}:*)"))
                    .collect();
                let tools = if write {
                    allowed.extend(["Edit".to_owned(), "Write".to_owned()]);
                    "Read,Grep,Glob,Bash,Edit,Write"
                } else {
                    allowed.extend(["Read".to_owned(), "Grep".to_owned(), "Glob".to_owned()]);
                    "Read,Grep,Glob,Bash"
                };
                let mut args = vec![
                    "-p".to_owned(),
                    prompt.to_owned(),
                    "--output-format".to_owned(),
                    "text".to_owned(),
                    "--tools".to_owned(),
                    tools.to_owned(),
                    "--allowedTools".to_owned(),
                    allowed.join(","),
                    "--permission-prompts".to_owned(),
                    "none".to_owned(),
                ];
                if write {
                    args.push("--setting-sources".to_owned());
                    args.push("project".to_owned());
                }
                args.push("--strict-mcp-config".to_owned());
                args.push("--mcp-config".to_owned());
                args.push("{\"mcpServers\":{}}".to_owned());
                args
            }
            Self::Copilot => {
                let mut available = vec!["view".to_owned(), "grep".to_owned(), "glob".to_owned()];
                let mut args = vec![
                    "-p".to_owned(),
                    prompt.to_owned(),
                    "--no-ask-user".to_owned(),
                ];
                let mut allow: Vec<String> = vec!["read".to_owned()];
                if write {
                    available.push("write".to_owned());
                    allow.push("write".to_owned());
                }
                available.push("bash".to_owned());
                allow.extend(READ_ONLY_COMMANDS.iter().map(|c| format!("bash({c}:*)")));
                args.push("--available-tools".to_owned());
                args.push(available.join(","));
                for a in allow {
                    args.push("--allow-tool".to_owned());
                    args.push(a);
                }
                args
            }
        }
    }
}

/// Runs `workflow`'s agent against `prompt` inside `cwd` and returns its stdout.
/// `write` must match the capability the prompt grants the agent (see [`Agent::args`]).
pub async fn invoke(workflow: &Workflow, prompt: &str, write: bool, cwd: &Path) -> Result<String> {
    let mut args = workflow.agent.args(prompt, write);
    if let Some(model) = &workflow.model {
        args.extend(["--model".into(), model.clone()]);
    }
    let result = timeout(
        Duration::from_secs(1800),
        Command::new(workflow.agent.executable())
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("agent timed out after 30 minutes")??;
    if !result.status.success() {
        bail!("agent failed: {}", String::from_utf8_lossy(&result.stderr));
    }
    let text = String::from_utf8(result.stdout)?;
    if text.trim().is_empty() {
        bail!("agent returned no output");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapters_do_not_bypass_permissions() {
        for agent in [Agent::Codex, Agent::Claude, Agent::Copilot] {
            for write in [false, true] {
                let args = agent.args("review", write);
                assert!(!args.iter().any(|s| {
                    s.contains("skip-permissions")
                        || s.contains("allow-all")
                        || s.contains("bypass-approvals-and-sandbox")
                        || s == "--yolo"
                }));
            }
        }
    }
    #[test]
    fn copilot_never_allow_lists_bare_bash() {
        for write in [false, true] {
            let args = Agent::Copilot.args("review", write);
            assert!(!args.iter().any(|s| s == "bash" || s == "shell"));
        }
    }
    #[test]
    fn claude_and_copilot_grant_read_only_gh_and_git_commands_in_both_modes() {
        for write in [false, true] {
            let claude = Agent::Claude.args("review", write).join(",");
            let copilot = Agent::Copilot.args("review", write).join(",");
            for command in READ_ONLY_COMMANDS {
                assert!(claude.contains(&format!("Bash({command}:*)")));
                assert!(copilot.contains(&format!("bash({command}:*)")));
            }
        }
    }
}
