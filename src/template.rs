use crate::config::Config;
use crate::gh::{current_repo, require_primary_checkout};
use crate::init::create_labels;
use anyhow::{bail, Context, Result};
use std::{fs, path::Path};

struct Template {
    name: &'static str,
    description: &'static str,
    yaml: &'static str,
}

/// Bundled starting points for `.github/gh-relay.yml`, embedded at compile
/// time so `template use` works from the published extension binary without
/// a checkout of this repository.
const TEMPLATES: &[Template] = &[
    Template {
        name: "review",
        description: "Read-only PR review across Codex, Claude Code and Copilot CLI",
        yaml: include_str!("../templates/review.yml"),
    },
    Template {
        name: "fix",
        description: "Claude Code reviews a PR, then addresses that feedback and pushes a fix",
        yaml: include_str!("../templates/fix.yml"),
    },
    Template {
        name: "full",
        description: "Every workflow (review, fix, research, impl), mostly on Claude Code",
        yaml: include_str!("../templates/full.yml"),
    },
];

pub fn list() {
    for template in TEMPLATES {
        println!("{:<8} {}", template.name, template.description);
    }
}

pub async fn apply(root: &Path, name: &str) -> Result<()> {
    let template = TEMPLATES
        .iter()
        .find(|t| t.name == name)
        .with_context(|| format!("unknown template '{name}'; run `gh relay template list`"))?;
    require_primary_checkout(root).await?;
    let path = root.join(".github/gh-relay.yml");
    if path.exists() {
        bail!("configuration already exists");
    }
    let repo = current_repo(root).await?;
    let rendered = template.yaml.replace("__REPO__", &repo);
    let config: Config = serde_yaml::from_str(&rendered).context("bundled template is invalid")?;
    fs::create_dir_all(root.join(".github"))?;
    fs::write(&path, &rendered)?;
    create_labels(root, &repo, &config.labels).await?;
    println!(
        "Created .github/gh-relay.yml from template '{name}' and its labels. Inspect and commit the configuration."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_bundled_template_parses_and_has_at_least_one_label() {
        for template in TEMPLATES {
            let rendered = template.yaml.replace("__REPO__", "owner/repo");
            let config: Config =
                serde_yaml::from_str(&rendered).expect("bundled template should parse");
            assert!(!config.labels.is_empty());
        }
    }
}
