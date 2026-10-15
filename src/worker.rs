use crate::config::{Config, Kind};
use crate::gh::gh;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;

pub struct Worker {
    pub root: PathBuf,
    pub config: Config,
}
impl Worker {
    pub fn state(&self) -> PathBuf {
        self.root.join(".git/gh-relay")
    }
    /// Agent-visible worktrees must live outside `.git/`: Claude Code classifies
    /// any path under it as sensitive and silently denies writes there, which
    /// would otherwise break every `impl` run.
    pub fn worktrees(&self) -> PathBuf {
        self.root.join(".gh-relay-worktrees")
    }
    /// Routes to the PR-review flow or the issue-triggered flow based on the label's `kind`.
    pub async fn dispatch(
        &self,
        label: &str,
        number: u64,
        post_result: bool,
        deduplicate: bool,
    ) -> Result<()> {
        match self.config.labels.get(label).context("unknown label")?.kind {
            Kind::Review => self.run(label, number, post_result, deduplicate).await,
            Kind::Research | Kind::Impl => {
                self.run_issue(label, number, post_result, deduplicate)
                    .await
            }
        }
    }
    pub async fn catch_up(&self) -> Result<()> {
        // Enumerate all pages, then confirm that a collaborator applied each label.
        let endpoint = format!("repos/{}/pulls?state=open&per_page=100", self.config.repo);
        let pages = gh(&["api", "--paginate", "--slurp", &endpoint], &self.root).await?;
        let pages: Vec<Vec<Value>> = serde_json::from_str(&pages)?;
        for pr in pages.into_iter().flatten() {
            let number = pr["number"].as_u64().context("missing PR number")?;
            for (label, workflow) in &self.config.labels {
                if workflow.kind != Kind::Review {
                    continue;
                }
                if !pr["labels"]
                    .as_array()
                    .context("missing labels")?
                    .iter()
                    .any(|l| l["name"] == *label)
                {
                    continue;
                }
                if self.authorized_label(number, label).await? {
                    if let Err(e) = self.dispatch(label, number, true, true).await {
                        eprintln!("PR #{number} {label}: {e:#}");
                    }
                }
            }
        }
        let endpoint = format!("repos/{}/issues?state=open&per_page=100", self.config.repo);
        let pages = gh(&["api", "--paginate", "--slurp", &endpoint], &self.root).await?;
        let pages: Vec<Vec<Value>> = serde_json::from_str(&pages)?;
        for issue in pages.into_iter().flatten() {
            if !issue["pull_request"].is_null() {
                continue;
            }
            let number = issue["number"].as_u64().context("missing issue number")?;
            for (label, workflow) in &self.config.labels {
                if workflow.kind == Kind::Review {
                    continue;
                }
                if !issue["labels"]
                    .as_array()
                    .context("missing labels")?
                    .iter()
                    .any(|l| l["name"] == *label)
                {
                    continue;
                }
                if self.authorized_label(number, label).await? {
                    if let Err(e) = self.dispatch(label, number, true, true).await {
                        eprintln!("issue #{number} {label}: {e:#}");
                    }
                }
            }
        }
        Ok(())
    }
    pub async fn authorized_label(&self, number: u64, label: &str) -> Result<bool> {
        let endpoint = format!(
            "repos/{}/issues/{number}/events?per_page=100",
            self.config.repo
        );
        let pages: Vec<Vec<Value>> = serde_json::from_str(
            &gh(&["api", "--paginate", "--slurp", &endpoint], &self.root).await?,
        )?;
        let events: Vec<Value> = pages.into_iter().flatten().collect();
        let event = events.iter().rev().find(|e| {
            (e["event"] == "labeled" || e["event"] == "unlabeled") && e["label"]["name"] == label
        });
        let Some(event) = event else {
            return Ok(false);
        };
        if event["event"] != "labeled" {
            return Ok(false);
        }
        let actor = event["actor"]["login"]
            .as_str()
            .context("missing label actor")?;
        let endpoint = format!(
            "repos/{}/collaborators/{actor}/permission",
            self.config.repo
        );
        let permission: Value = serde_json::from_str(&gh(&["api", &endpoint], &self.root).await?)?;
        Ok(matches!(
            permission["permission"].as_str(),
            Some("admin" | "maintain" | "write")
        ))
    }
}
