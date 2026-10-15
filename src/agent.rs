use crate::config::Workflow;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Stdio};
use tokio::{
    process::Command,
    time::{timeout, Duration},
};

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
        let args: Vec<&str> = match (self, write) {
            (Self::Codex, false) => {
                vec!["exec", "--sandbox", "read-only", "--color", "never", prompt]
            }
            (Self::Codex, true) => vec![
                "exec",
                "--sandbox",
                "workspace-write",
                "--color",
                "never",
                prompt,
            ],
            (Self::Claude, false) => vec![
                "-p",
                prompt,
                "--output-format",
                "text",
                "--tools",
                "Read,Grep,Glob",
                "--allowedTools",
                "Read,Grep,Glob",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
            ],
            (Self::Claude, true) => vec![
                "-p",
                prompt,
                "--output-format",
                "text",
                "--tools",
                "Read,Grep,Glob,Edit,Write,Bash",
                "--allowedTools",
                "Edit,Write",
                "--permission-prompts",
                "none",
                "--setting-sources",
                "project",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
            ],
            (Self::Copilot, false) => vec![
                "-p",
                prompt,
                "--no-ask-user",
                "--available-tools",
                "view,grep,glob",
                "--allow-tool",
                "read",
                "--deny-tool",
                "shell",
                "--deny-tool",
                "write",
            ],
            (Self::Copilot, true) => vec![
                "-p",
                prompt,
                "--no-ask-user",
                "--available-tools",
                "view,grep,glob,write",
                "--allow-tool",
                "read",
                "--allow-tool",
                "write",
                "--deny-tool",
                "shell",
            ],
        };
        args.into_iter().map(str::to_owned).collect()
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
    fn copilot_write_mode_still_denies_shell() {
        assert!(Agent::Copilot
            .args("review", true)
            .windows(2)
            .any(|w| w == ["--deny-tool".to_string(), "shell".to_string()]));
    }
}
