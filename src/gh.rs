use anyhow::{bail, Context, Result};
use std::{path::Path, process::Stdio};
use tokio::{
    process::Command,
    time::{timeout, Duration},
};

pub async fn output(program: &str, args: &[&str], cwd: &Path) -> Result<String> {
    let result = timeout(
        Duration::from_secs(120),
        Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("command timed out")??;
    if !result.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(String::from_utf8(result.stdout)?)
}
pub async fn gh(args: &[&str], cwd: &Path) -> Result<String> {
    output("gh", args, cwd).await
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Bails unless `root` is a primary checkout (not a linked git worktree):
/// commands that write to `.github/`, create branches or hold the worker
/// lock all assume this.
pub async fn require_primary_checkout(root: &Path) -> Result<()> {
    let git_dir = output("git", &["rev-parse", "--absolute-git-dir"], root).await?;
    if Path::new(git_dir.trim()) != root.join(".git") {
        bail!("v0.1 requires a primary git checkout (not a linked worktree)");
    }
    Ok(())
}

/// The current repository's `owner/name`, as `gh` resolves it from the checkout's remote.
pub async fn current_repo(root: &Path) -> Result<String> {
    let repo: serde_json::Value =
        serde_json::from_str(&gh(&["repo", "view", "--json", "nameWithOwner"], root).await?)?;
    Ok(repo["nameWithOwner"]
        .as_str()
        .context("missing repository")?
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn label_keys_cannot_escape_state_directory() {
        assert_eq!(hex(b"../a:b"), "2e2e2f613a62");
    }
}
