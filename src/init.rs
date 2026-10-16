use crate::agent::Agent;
use crate::config::{Config, Kind, Workflow};
use crate::gh::{gh, output};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

pub async fn run(root: &Path) -> Result<()> {
    let git_dir = output("git", &["rev-parse", "--absolute-git-dir"], root).await?;
    if Path::new(git_dir.trim()) != root.join(".git") {
        bail!("v0.1 requires a primary git checkout (not a linked worktree)");
    }
    let path = root.join(".github/gh-relay.yml");
    if path.exists() {
        bail!("configuration already exists");
    }
    let repo: Value =
        serde_json::from_str(&gh(&["repo", "view", "--json", "nameWithOwner"], root).await?)?;
    let repo = repo["nameWithOwner"]
        .as_str()
        .context("missing repository")?
        .to_owned();
    let labels = [Agent::Codex, Agent::Claude, Agent::Copilot]
        .into_iter()
        .map(|agent| {
            (
                format!("{}:review", agent.executable()),
                Workflow {
                    agent,
                    model: None,
                    kind: Default::default(),
                    prompt: "Review for bugs, regressions and unnecessary complexity. Explain user-facing impact and cite file paths and lines. Report only actionable findings.".into(),
                },
            )
        })
        .collect();
    let config = Config {
        version: 1,
        repo: repo.clone(),
        labels,
    };
    fs::create_dir_all(root.join(".github"))?;
    fs::write(path, serde_yaml::to_string(&config)?)?;
    create_labels(root, &repo, &config.labels).await?;
    println!(
        "Created .github/gh-relay.yml and review labels. Inspect and commit the configuration."
    );
    Ok(())
}

/// Creates (or updates, via `--force`) one GitHub label per configured
/// workflow. Shared by `init` (review labels only, right after scaffolding)
/// and `setup` (every label in an already-committed configuration, including
/// hand-added `research`/`impl` ones).
pub async fn create_labels(
    root: &Path,
    repo: &str,
    labels: &BTreeMap<String, Workflow>,
) -> Result<()> {
    for (label, workflow) in labels {
        let description = match workflow.kind {
            Kind::Review => "Run a local gh-relay review",
            Kind::Research => "Run a local gh-relay issue research",
            Kind::Impl => "Run a local gh-relay issue implementation",
        };
        gh(
            &[
                "label",
                "create",
                label,
                "--repo",
                repo,
                "--color",
                "7057ff",
                "--description",
                description,
                "--force",
            ],
            root,
        )
        .await?;
    }
    Ok(())
}
