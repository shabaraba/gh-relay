use crate::daemon;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
struct Entry {
    root: PathBuf,
    repo: String,
    port: u16,
}

/// `$HOME/.gh-relay/instances`, one JSON file per running `start` (foreground
/// or daemon), named by pid. A dedicated directory (rather than a single
/// shared file) means every process only ever writes its own file, so
/// concurrent `start`s across repositories never race on the same write.
fn registry_dir() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".gh-relay/instances"))
}

/// Registers one running `start` invocation in the global instance registry
/// so `gh relay list` can find it from any directory, and removes it again
/// on drop. Registration is best-effort: a failure (e.g. `$HOME` unset) is
/// logged but does not stop the webhook listener from serving.
pub struct Instance {
    path: PathBuf,
}

impl Instance {
    pub fn register(root: &Path, repo: &str, port: u16) -> Self {
        match Self::try_register(root, repo, port) {
            Ok(path) => Instance { path },
            Err(e) => {
                eprintln!("warning: failed to register in global instance list: {e:#}");
                Instance {
                    path: PathBuf::new(),
                }
            }
        }
    }
    fn try_register(root: &Path, repo: &str, port: u16) -> Result<PathBuf> {
        let dir = registry_dir()?;
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", std::process::id()));
        let entry = Entry {
            root: root.to_path_buf(),
            repo: repo.to_string(),
            port,
        };
        fs::write(&path, serde_json::to_string(&entry)?)?;
        Ok(path)
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Prints every repository with a `start` currently running on this machine,
/// across all directories. A registry file whose pid is no longer alive
/// (e.g. the process was killed outright, skipping its own cleanup) is
/// removed instead of printed.
pub fn run() -> Result<()> {
    let dir = registry_dir()?;
    let mut entries = Vec::new();
    if dir.is_dir() {
        for file in fs::read_dir(&dir)? {
            let path = file?.path();
            let Some(pid) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let alive = daemon::signal(pid, "-0")
                .map(|s| s.success())
                .unwrap_or(false);
            if !alive {
                let _ = fs::remove_file(&path);
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<Entry>(&fs::read_to_string(&path)?) {
                entries.push((pid, entry));
            }
        }
    }
    if entries.is_empty() {
        println!("no gh-relay processes running");
        return Ok(());
    }
    entries.sort_by_key(|(pid, _)| *pid);
    for (pid, entry) in entries {
        println!(
            "{}\tpid {pid}\tport {}\t{}",
            entry.repo,
            entry.port,
            entry.root.display()
        );
    }
    Ok(())
}
