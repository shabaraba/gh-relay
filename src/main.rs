mod agent;
mod config;
mod daemon;
mod fix;
mod gh;
mod implement;
mod init;
mod issue;
mod research;
mod review;
mod setup;
mod template;
mod webhook;
mod worker;

use anyhow::{bail, Context, Result};
use axum::{routing::post, Router};
use clap::{Parser, Subcommand};
use config::Kind;
use fs2::FileExt;
use gh::{gh, output};
use std::{fs, fs::OpenOptions, path::PathBuf, process::Stdio, sync::Arc};
use tokio::{process::Command, sync::Mutex, time::Duration};
use worker::Worker;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, default_value = ".", global = true)]
    directory: PathBuf,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Generate trusted local workflow configuration and GitHub labels.
    Init,
    /// Create a GitHub label for every workflow in .github/gh-relay.yml.
    Setup,
    /// Bundled configuration templates for .github/gh-relay.yml.
    Template {
        #[command(subcommand)]
        action: TemplateAction,
    },
    /// Listen locally and forward GitHub webhook events. Ctrl-C stops both.
    Start {
        #[arg(long, default_value_t = 38741)]
        port: u16,
        /// Run detached in the background; use `gh relay stop` to end it.
        #[arg(short, long)]
        daemon: bool,
    },
    /// Stop a daemon started with `start --daemon`.
    Stop,
    /// Remove a stale `gh webhook forward` registration left behind by a
    /// daemon that didn't shut down via `stop`/Ctrl-C.
    CleanWebhook,
    /// Run one workflow manually; posting is opt-in.
    Run {
        label: String,
        /// Required for `review` and `fix` labels.
        #[arg(long, conflicts_with = "issue")]
        pr: Option<u64>,
        /// Required for `research` and `impl` labels.
        #[arg(long, conflicts_with = "pr")]
        issue: Option<u64>,
        #[arg(long)]
        post: bool,
    },
    /// Inspect dependencies without executing an agent.
    Doctor,
    /// Show persisted successful runs.
    Status,
}

#[derive(Subcommand)]
enum TemplateAction {
    /// List bundled templates.
    List,
    /// Write .github/gh-relay.yml from a template and create its labels.
    Use { name: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.directory.canonicalize()?;
    match cli.command {
        Action::Doctor => {
            for executable in ["git", "gh", "codex", "claude", "copilot"] {
                match output(executable, &["--version"], &root).await {
                    Ok(v) => println!("{executable}: {}", v.trim()),
                    Err(e) => println!("{executable}: {e}"),
                }
            }
            gh(&["auth", "status"], &root).await?;
            println!("GitHub authenticated. Agent authentication remains managed by each CLI.");
        }
        Action::Init => init::run(&root).await?,
        Action::Setup => setup::run(&root).await?,
        Action::Template {
            action: TemplateAction::List,
        } => template::list(),
        Action::Template {
            action: TemplateAction::Use { name },
        } => template::apply(&root, &name).await?,
        Action::Start { port, daemon: true } => daemon::start(&root, port).await?,
        Action::Stop => daemon::stop(&root)?,
        Action::CleanWebhook => {
            let config = config::load(&root)?;
            let removed = webhook::clean(&config.repo, &root).await?;
            println!("removed {removed} stale webhook(s)");
        }
        Action::Status => {
            match daemon::status(&root) {
                Some(pid) => println!("daemon: running (pid {pid})"),
                None => println!("daemon: not running"),
            }
            let state_dir = root.join(".git/gh-relay");
            if state_dir.is_dir() {
                for entry in fs::read_dir(state_dir)? {
                    let entry = entry?;
                    if entry.path().extension().is_some_and(|e| e == "json") {
                        println!("{}", fs::read_to_string(entry.path())?);
                    }
                }
            }
        }
        action => {
            let config = config::load(&root)?;
            gh::require_primary_checkout(&root).await?;
            let worker = Worker { root, config };
            fs::create_dir_all(worker.state())?;
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(worker.state().join("lock"))?;
            lock.try_lock_exclusive()
                .context("another gh-relay process is running")?;
            match action {
                Action::Run {
                    label,
                    pr,
                    issue,
                    post,
                } => {
                    let kind = worker
                        .config
                        .labels
                        .get(&label)
                        .context("unknown label")?
                        .kind;
                    match (kind, pr, issue) {
                        (Kind::Review | Kind::Fix, Some(pr), None) => {
                            worker.run(&label, pr, post, false).await?
                        }
                        (Kind::Review | Kind::Fix, None, _) => {
                            bail!("label '{label}' targets a pull request; pass --pr")
                        }
                        (Kind::Research | Kind::Impl, None, Some(issue)) => {
                            worker.run_issue(&label, issue, post, false).await?
                        }
                        (Kind::Research | Kind::Impl, _, None) => {
                            bail!("label '{label}' targets an issue; pass --issue")
                        }
                        (_, Some(_), Some(_)) => unreachable!("clap rejects --pr with --issue"),
                    }
                }
                Action::Start { port, daemon: _ } => {
                    let listener =
                        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                            .await?;
                    let url = format!("http://127.0.0.1:{port}/webhook");
                    let mut forwarder = Command::new("gh")
                        .args([
                            "webhook",
                            "forward",
                            "--repo",
                            &worker.config.repo,
                            "--events",
                            "pull_request,issues",
                            "--url",
                            &url,
                        ])
                        .current_dir(&worker.root)
                        .stdin(Stdio::null())
                        .kill_on_drop(true)
                        .spawn()
                        .context("install cli/gh-webhook first")?;
                    let worker = Arc::new(Mutex::new(worker));
                    let recovery_worker = worker.clone();
                    let recovery = tokio::spawn(async move {
                        let mut interval = tokio::time::interval(Duration::from_secs(300));
                        loop {
                            interval.tick().await;
                            let worker = recovery_worker.lock().await;
                            if let Err(e) = worker.catch_up().await {
                                eprintln!("catch-up: {e:#}");
                            }
                        }
                    });
                    println!("Listening on {url}. Recovery every 5 minutes. Ctrl-C to stop.");
                    let app = Router::new()
                        .route("/webhook", post(webhook::webhook))
                        .with_state(worker);
                    tokio::select! {
                        result = axum::serve(listener, app) => { result?; },
                        result = forwarder.wait() => { recovery.abort(); bail!("webhook forwarder exited: {:?}", result?); },
                        _ = tokio::signal::ctrl_c() => {}
                    }
                    recovery.abort();
                    // SIGKILL (what `Child::kill` sends) skips `gh webhook
                    // forward`'s own cleanup, leaving its webhook
                    // registration on the repo and breaking the next
                    // `start`. Give it a chance to exit on SIGINT first.
                    if let Some(pid) = forwarder.id() {
                        let _ = std::process::Command::new("kill")
                            .args(["-INT", &pid.to_string()])
                            .status();
                        let _ =
                            tokio::time::timeout(Duration::from_secs(5), forwarder.wait()).await;
                    }
                    let _ = forwarder.kill().await;
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn unknown_workflow_fields_are_rejected() {
        assert!(serde_yaml::from_str::<crate::config::Workflow>(
            "agent: codex\nprompt: review\ncommand: rm\n"
        )
        .is_err());
    }
}
