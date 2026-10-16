use crate::config::Kind;
use crate::gh::{gh, hex, output};
use crate::worker::Worker;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
};

/// Issues have no SHA to pin to; this stands in for one so dedup markers and
/// worktree names change whenever the title or body the agent saw changes.
pub(crate) fn content_version(title: &str, body: &str) -> String {
    let mut hasher = DefaultHasher::new();
    title.hash(&mut hasher);
    body.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The issue content an agent run operates on, bundled to keep adapter
/// function signatures short.
pub(crate) struct IssueContext<'a> {
    pub issue: u64,
    pub title: &'a str,
    pub body: &'a str,
    pub version: &'a str,
}

impl Worker {
    pub async fn run_issue(
        &self,
        label: &str,
        issue: u64,
        post_result: bool,
        deduplicate: bool,
    ) -> Result<()> {
        if issue == 0 {
            bail!("issue number must be positive");
        }
        let workflow = self.config.labels.get(label).context("unknown label")?;
        if workflow.kind.is_pr() {
            bail!("label targets a pull request; use --pr");
        }
        let endpoint = format!("repos/{}/issues/{issue}", self.config.repo);
        let info: Value = serde_json::from_str(&gh(&["api", &endpoint], &self.root).await?)?;
        if !info["pull_request"].is_null() {
            bail!("number is a pull request, not an issue");
        }
        if info["state"] != "open" {
            bail!("issue is not open");
        }
        if deduplicate
            && !info["labels"]
                .as_array()
                .context("missing labels")?
                .iter()
                .any(|l| l["name"] == label)
        {
            return Ok(());
        }
        if workflow.kind == Kind::Impl && !self.author_is_trusted(&info).await? {
            bail!("issue author lacks write permission; impl requires a trusted author");
        }
        let title = info["title"].as_str().context("missing title")?.to_owned();
        let body = info["body"].as_str().unwrap_or("").to_owned();
        let version = content_version(&title, &body);

        fs::create_dir_all(self.state())?;
        let marker = self.state().join(format!(
            "issue-{issue}-{version}-{}.json",
            hex(label.as_bytes())
        ));
        if deduplicate && marker.exists() {
            return Ok(());
        }
        let worktree = self
            .worktrees()
            .join(format!("work-issue-{issue}-{version}"));
        if worktree.exists() {
            bail!(
                "worktree already exists; inspect {} before retrying",
                worktree.display()
            );
        }
        fs::create_dir_all(self.worktrees())?;
        let repo_endpoint = format!("repos/{}", self.config.repo);
        let repo_info: Value =
            serde_json::from_str(&gh(&["api", &repo_endpoint], &self.root).await?)?;
        let default_branch = repo_info["default_branch"]
            .as_str()
            .context("missing default branch")?;
        output("git", &["fetch", "origin", default_branch], &self.root).await?;
        let sha = output("git", &["rev-parse", "FETCH_HEAD"], &self.root).await?;
        let sha = sha.trim();
        output(
            "git",
            &[
                "worktree",
                "add",
                "--detach",
                worktree.to_str().context("non-UTF8 path")?,
                sha,
            ],
            &self.root,
        )
        .await?;
        let context = IssueContext {
            issue,
            title: &title,
            body: &body,
            version: &version,
        };
        let result = match workflow.kind {
            Kind::Research => self.research(workflow, &context, &worktree).await,
            Kind::Impl => {
                self.implement(workflow, &context, &worktree, post_result)
                    .await
            }
            Kind::Review | Kind::Fix => unreachable!(),
        };
        let cleanup = output(
            "git",
            &["worktree", "remove", "--force", worktree.to_str().unwrap()],
            &self.root,
        )
        .await;
        let result = result?;
        cleanup?;

        if post_result {
            if workflow.kind == Kind::Research {
                self.post_issue_comment(issue, &version, &result).await?;
            }
            // Kind::Impl already delivered its result by pushing a branch and
            // opening a draft PR inside `implement` (gated on `post_result` there too);
            // nothing further to post.
            self.remove_label("issue", issue, label).await;
        } else {
            println!("{result}");
        }
        fs::write(
            marker,
            serde_json::to_vec_pretty(&json!({
                "issue": issue,
                "version": version,
                "label": label,
                "posted": post_result,
            }))?,
        )?;
        Ok(())
    }

    pub(crate) async fn author_is_trusted(&self, info: &Value) -> Result<bool> {
        let actor = info["user"]["login"]
            .as_str()
            .context("missing issue author")?;
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

    /// Bails if the issue's state, title or body moved since `version` was captured.
    pub(crate) async fn verify_unchanged(&self, issue: u64, version: &str) -> Result<Value> {
        let endpoint = format!("repos/{}/issues/{issue}", self.config.repo);
        let current: Value = serde_json::from_str(&gh(&["api", &endpoint], &self.root).await?)?;
        if current["state"] != "open" {
            bail!("issue changed; result not delivered");
        }
        let title = current["title"].as_str().context("missing title")?;
        let body = current["body"].as_str().unwrap_or("");
        if content_version(title, body) != version {
            bail!("issue changed; result not delivered");
        }
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_version_changes_with_body() {
        assert_ne!(content_version("t", "a"), content_version("t", "b"));
    }
}
