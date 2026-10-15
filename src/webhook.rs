use crate::config::Kind;
use crate::worker::Worker;
use axum::{extract::State, http::StatusCode, Json};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn webhook(
    State(worker): State<Arc<Mutex<Worker>>>,
    Json(event): Json<Value>,
) -> StatusCode {
    if event["action"] != "labeled" {
        return StatusCode::NO_CONTENT;
    }
    let (number, expected_kind) = if !event["pull_request"].is_null() {
        let Some(number) = event["number"].as_u64() else {
            return StatusCode::BAD_REQUEST;
        };
        (number, Kind::Review)
    } else if !event["issue"].is_null() {
        let Some(number) = event["issue"]["number"].as_u64() else {
            return StatusCode::BAD_REQUEST;
        };
        // Either Research or Impl; checked precisely once the label is resolved below.
        (number, Kind::Research)
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
    let is_issue_kind = workflow.kind != Kind::Review;
    if is_issue_kind != (expected_kind != Kind::Review) {
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
