use crate::config::{self, Config, Workflow};
use crate::gh::{current_repo, require_primary_checkout};
use crate::init::create_labels;
use anyhow::{bail, Context, Result};
use std::{collections::BTreeMap, fs, path::Path};

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

/// How to apply a template when `.github/gh-relay.yml` already exists.
/// `Refuse` is the default (matching `init`'s behavior); `--overwrite` and
/// `--merge` opt into replacing or extending an existing configuration.
#[derive(Clone, Copy)]
pub enum ApplyMode {
    Refuse,
    Overwrite,
    Merge,
}

/// Adds `incoming` labels that aren't already present in `existing`, leaving
/// any already-configured label (hand-edited or from a prior template)
/// untouched. Returns the labels actually added and those left unchanged.
fn merge_labels(
    existing: &mut BTreeMap<String, Workflow>,
    incoming: BTreeMap<String, Workflow>,
) -> (Vec<String>, Vec<String>) {
    let mut added = Vec::new();
    let mut skipped = Vec::new();
    for (label, workflow) in incoming {
        if existing.contains_key(&label) {
            skipped.push(label);
        } else {
            existing.insert(label.clone(), workflow);
            added.push(label);
        }
    }
    (added, skipped)
}

pub async fn apply(root: &Path, name: &str, mode: ApplyMode) -> Result<()> {
    let template = TEMPLATES
        .iter()
        .find(|t| t.name == name)
        .with_context(|| format!("unknown template '{name}'; run `gh relay template list`"))?;
    require_primary_checkout(root).await?;
    let path = root.join(".github/gh-relay.yml");
    let exists = path.exists();
    if exists && matches!(mode, ApplyMode::Refuse) {
        bail!(
            "configuration already exists; pass --overwrite to replace it, \
             or --merge to add this template's labels without disturbing existing ones"
        );
    }
    let repo = current_repo(root).await?;
    let rendered = template.yaml.replace("__REPO__", &repo);
    let template_config: Config =
        serde_yaml::from_str(&rendered).context("bundled template is invalid")?;

    let (config, summary) = match mode {
        ApplyMode::Refuse | ApplyMode::Overwrite if !exists => (
            template_config,
            format!("Created .github/gh-relay.yml from template '{name}' and its labels."),
        ),
        ApplyMode::Overwrite => (
            template_config,
            format!(
                "Replaced .github/gh-relay.yml with template '{name}' and (re)created its labels."
            ),
        ),
        ApplyMode::Merge => {
            let mut existing = config::load(root)?;
            let (added, skipped) = merge_labels(&mut existing.labels, template_config.labels);
            let summary = if added.is_empty() {
                format!("Template '{name}' added no new labels; already configured: {skipped:?}.")
            } else if skipped.is_empty() {
                format!("Merged template '{name}' into .github/gh-relay.yml: added {added:?}.")
            } else {
                format!(
                    "Merged template '{name}' into .github/gh-relay.yml: added {added:?}; left unchanged (already configured): {skipped:?}."
                )
            };
            (existing, summary)
        }
        ApplyMode::Refuse => {
            unreachable!("Refuse with an existing configuration already bailed above")
        }
    };

    fs::create_dir_all(root.join(".github"))?;
    fs::write(&path, serde_yaml::to_string(&config)?)?;
    create_labels(root, &repo, &config.labels).await?;
    println!("{summary} Inspect and commit the configuration.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;

    fn workflow(agent: Agent) -> Workflow {
        Workflow {
            agent,
            prompt: "prompt".into(),
            model: None,
            kind: Default::default(),
        }
    }

    #[test]
    fn every_bundled_template_parses_and_has_at_least_one_label() {
        for template in TEMPLATES {
            let rendered = template.yaml.replace("__REPO__", "owner/repo");
            let config: Config =
                serde_yaml::from_str(&rendered).expect("bundled template should parse");
            assert!(!config.labels.is_empty());
        }
    }

    #[test]
    fn merge_labels_adds_new_and_skips_existing() {
        let mut existing = BTreeMap::new();
        existing.insert("claude:review".to_string(), workflow(Agent::Claude));
        let mut incoming = BTreeMap::new();
        incoming.insert("claude:review".to_string(), workflow(Agent::Codex));
        incoming.insert("claude:fix".to_string(), workflow(Agent::Claude));

        let (added, skipped) = merge_labels(&mut existing, incoming);

        assert_eq!(added, vec!["claude:fix".to_string()]);
        assert_eq!(skipped, vec!["claude:review".to_string()]);
        assert_eq!(existing.len(), 2);
        assert!(matches!(existing["claude:review"].agent, Agent::Claude));
    }
}
