use crate::agent::Agent;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub repo: String,
    pub labels: BTreeMap<String, Workflow>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub agent: Agent,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub kind: Kind,
}

/// `Review` runs read-only against a PR diff and posts a PR comment.
/// `Fix` runs write-capable against an open PR already in this repository (not
/// a fork): it reads the PR's discussion and review feedback itself, pushes a
/// commit to the PR's own branch, and posts a PR comment describing the change.
/// `Research` runs read-only against an issue and posts an issue comment.
/// `Impl` runs write-capable against an issue, then pushes a branch and opens a draft PR.
#[derive(Clone, Copy, Default, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Review,
    Fix,
    Research,
    Impl,
}

impl Kind {
    /// `Review` and `Fix` target a pull request (`--pr`); `Research` and `Impl` target an issue (`--issue`).
    pub fn is_pr(self) -> bool {
        matches!(self, Kind::Review | Kind::Fix)
    }
}

pub fn load(root: &Path) -> Result<Config> {
    let c: Config = serde_yaml::from_str(&fs::read_to_string(root.join(".github/gh-relay.yml"))?)?;
    if c.version != 1 {
        bail!("unsupported configuration version");
    }
    let parts: Vec<_> = c.repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
    {
        bail!("invalid owner/repo");
    }
    if c.labels.is_empty() {
        bail!("configure at least one label");
    }
    Ok(c)
}
