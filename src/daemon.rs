use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{process::Command, time::Duration};

fn state(root: &Path) -> PathBuf {
    root.join(".git/gh-relay")
}
fn pid_path(root: &Path) -> PathBuf {
    state(root).join("daemon.pid")
}
fn log_path(root: &Path) -> PathBuf {
    state(root).join("daemon.log")
}

/// The daemon's PID, if the process it names is still alive. Stale PID files
/// (process no longer running) report `None`.
pub fn status(root: &Path) -> Option<u32> {
    let pid: u32 = fs::read_to_string(pid_path(root))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    signal(pid, "-0")
        .map(|s| s.success())
        .unwrap_or(false)
        .then_some(pid)
}

/// Runs `kill <signal> <pid>`, suppressing its own stdout/stderr: callers
/// only care about the exit status, and "No such process" on a liveness or
/// already-exited check is expected, not an error to surface.
fn signal(pid: u32, signal: &str) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new("kill")
        .args([signal, &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

/// Spawns a detached `start --port` re-invocation of this same binary and
/// returns once it is confirmed alive. `nohup` only needs to ignore SIGHUP
/// here, since the child keeps the launcher's process group; that is enough
/// for it to outlive the shell session that started it.
pub async fn start(root: &Path, port: u16) -> Result<()> {
    if let Some(pid) = status(root) {
        bail!("daemon already running (pid {pid}); run `gh relay stop` first");
    }
    fs::create_dir_all(state(root))?;
    let log_file = log_path(root);
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;
    let exe = std::env::current_exe().context("resolving current executable")?;
    let mut child = Command::new("nohup")
        .arg(&exe)
        .arg("--directory")
        .arg(root)
        .arg("start")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
        .context("spawning daemon")?;
    let pid = child.id().context("daemon exited immediately")?;
    fs::write(pid_path(root), pid.to_string())?;
    // Poll for a couple of seconds to fail fast (missing config, port in
    // use, lock held) rather than reporting success for a process that is
    // already gone, without making every successful start wait that long.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let exit = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break Some(exit),
            Ok(None) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok(None) => break None,
            Err(_) => break None,
        }
    };
    if let Some(exit) = exit {
        let _ = fs::remove_file(pid_path(root));
        let log_tail = fs::read_to_string(&log_file).unwrap_or_default();
        bail!(
            "daemon exited immediately ({exit}); see {}\n{log_tail}",
            log_file.display()
        );
    }
    println!(
        "gh-relay daemon started (pid {pid}). Logs: {}",
        log_file.display()
    );
    Ok(())
}

/// Sends SIGINT (not SIGTERM, which has no handler and would skip cleanup
/// entirely): `start`'s `tokio::signal::ctrl_c()` branch is what kills the
/// `gh webhook forward` child on Ctrl-C, and SIGINT is the only signal that
/// reaches it. Waits for the process to actually exit before returning.
pub fn stop(root: &Path) -> Result<()> {
    let Some(pid) = status(root) else {
        let _ = fs::remove_file(pid_path(root));
        println!("no daemon running");
        return Ok(());
    };
    signal(pid, "-INT").context("sending SIGINT")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while status(root).is_some() {
        if std::time::Instant::now() > deadline {
            bail!(
                "daemon (pid {pid}) did not exit within 5s; check {}",
                log_path(root).display()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    fs::remove_file(pid_path(root))?;
    println!("stopped daemon (pid {pid})");
    Ok(())
}
