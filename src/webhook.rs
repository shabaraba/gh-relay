use crate::gh::gh;
use crate::worker::Worker;
use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, Json};
use serde_json::Value;
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

/// The URL `gh webhook forward` registers on the repo. A daemon killed
/// without SIGINT skips that extension's own cleanup and leaves this
/// registration behind (see `main.rs`'s start/stop handling), which then
/// makes the next `start` fail with "Hook already exists".
const FORWARDER_URL: &str = "https://webhook-forwarder.github.com/hook";

/// Removes any stale `gh webhook forward` registrations from `repo`. Only
/// touches hooks pointing at [`FORWARDER_URL`], so hooks created for other
/// purposes are left alone. Returns the number removed.
pub async fn clean(repo: &str, root: &Path) -> Result<usize> {
    let hooks: Vec<Value> =
        serde_json::from_str(&gh(&["api", &format!("repos/{repo}/hooks")], root).await?)?;
    let mut removed = 0;
    for hook in hooks {
        if hook["config"]["url"].as_str() == Some(FORWARDER_URL) {
            let id = hook["id"].as_u64().context("missing hook id")?;
            gh(
                &["api", "-X", "DELETE", &format!("repos/{repo}/hooks/{id}")],
                root,
            )
            .await?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub async fn webhook(
    State(worker): State<Arc<Mutex<Worker>>>,
    Json(event): Json<Value>,
) -> StatusCode {
    if event["action"] != "labeled" {
        return StatusCode::NO_CONTENT;
    }
    let (number, expect_pr) = if !event["pull_request"].is_null() {
        let Some(number) = event["number"].as_u64() else {
            return StatusCode::BAD_REQUEST;
        };
        (number, true)
    } else if !event["issue"].is_null() {
        let Some(number) = event["issue"]["number"].as_u64() else {
            return StatusCode::BAD_REQUEST;
        };
        // Either Research or Impl; checked precisely once the label is resolved below.
        (number, false)
    } else {
        return StatusCode::NO_CONTENT;
    };
    let Ok(worker) = worker.try_lock_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if event["repository"]["full_name"] != worker.config.repo {
        return StatusCode::NO_CONTENT;
    }
    let Some(label) = event["label"]["name"].as_str() else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(workflow) = worker.config.labels.get(label) else {
        return StatusCode::NO_CONTENT;
    };
    if workflow.kind.is_pr() != expect_pr {
        return StatusCode::NO_CONTENT;
    }
    let label = label.to_owned();
    tokio::spawn(async move {
        match worker.authorized_label(number, &label).await {
            Ok(true) => {
                if let Err(e) = worker.dispatch(&label, number, true, true).await {
                    eprintln!("#{number} {label}: {e:#}");
                }
            }
            Ok(false) => eprintln!("#{number}: label actor lacks write permission"),
            Err(e) => eprintln!("webhook: {e:#}"),
        }
    });
    StatusCode::ACCEPTED
}
