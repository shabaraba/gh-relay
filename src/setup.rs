use crate::config;
use crate::init::create_labels;
use anyhow::Result;
use std::path::Path;

/// Creates a GitHub label for every workflow already defined in
/// `.github/gh-relay.yml`, including `research`/`impl` labels added by hand
/// after `init` (which only scaffolds `review` labels).
pub async fn run(root: &Path) -> Result<()> {
    let config = config::load(root)?;
    create_labels(root, &config.repo, &config.labels).await?;
    println!("Created {} label(s).", config.labels.len());
    Ok(())
}
