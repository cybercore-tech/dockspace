//! Every real Docker interaction shells out to the actual `docker` CLI —
//! same philosophy as cyberfleet's `git_ops.rs`: get exact behavior parity
//! (auth, output, exit codes) with what you'd get running it yourself,
//! instead of reimplementing Docker Engine API semantics against a crate
//! that has to keep up with every daemon version.

use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, serde::Serialize, PartialEq)]
pub enum StackStatus {
    Running,
    Partial,
    Stopped,
    /// `docker compose ps` itself failed (daemon unreachable, permission
    /// denied, etc.) — distinct from "stopped" so the UI can say so
    /// instead of quietly lying about container state.
    Unknown,
}

pub struct ContainerState {
    /// Not read yet — kept for a future per-service status breakdown on
    /// the card (right now only the aggregate `derive_status()` is shown).
    #[allow(dead_code)]
    pub service: String,
    pub state: String,
}

/// `docker compose -f <path> ps --format json`. Handles both response
/// shapes seen across compose versions: one JSON array, or newline-
/// delimited JSON (one object per line).
pub async fn ps(compose_path: &Path) -> Result<Vec<ContainerState>, String> {
    let out = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_path)
        .arg("ps")
        .arg("--format")
        .arg("json")
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut result = Vec::new();

    let parsed_as_array: Option<Vec<serde_json::Value>> = serde_json::from_str(&text).ok();
    if let Some(arr) = parsed_as_array {
        for v in arr {
            result.push(container_from_value(&v));
        }
    } else {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                result.push(container_from_value(&v));
            }
        }
    }

    Ok(result)
}

fn container_from_value(v: &serde_json::Value) -> ContainerState {
    ContainerState {
        service: v
            .get("Service")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string(),
        state: v
            .get("State")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

pub fn derive_status(containers: &Result<Vec<ContainerState>, String>) -> StackStatus {
    match containers {
        Err(_) => StackStatus::Unknown,
        Ok(c) if c.is_empty() => StackStatus::Stopped,
        Ok(c) => {
            let running = c.iter().filter(|s| s.state == "running").count();
            if running == 0 {
                StackStatus::Stopped
            } else if running < c.len() {
                StackStatus::Partial
            } else {
                StackStatus::Running
            }
        }
    }
}

/// `docker compose -f <path> {up -d | down | restart}`. Runs async so an
/// axum handler awaiting this never blocks the rest of the server —
/// cassandra/nextcloud/solr can all take a while on first boot.
pub async fn action(compose_path: &Path, action: &str) -> (bool, String) {
    let mut cmd = Command::new("docker");
    cmd.arg("compose").arg("-f").arg(compose_path);
    match action {
        "up" => {
            cmd.arg("up").arg("-d");
        }
        "down" => {
            cmd.arg("down");
        }
        "restart" => {
            cmd.arg("restart");
        }
        _ => return (false, format!("unknown action '{action}'")),
    }
    cmd.stdin(Stdio::null());

    match cmd.output().await {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), text)
        }
        Err(e) => (false, e.to_string()),
    }
}

/// `docker compose -f <path> logs --tail <n>` — a snapshot, not a live
/// stream. Good enough for v1; a real tail (SSE) is a natural next step.
pub async fn logs(compose_path: &Path, tail: u32) -> String {
    let out = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_path)
        .arg("logs")
        .arg("--tail")
        .arg(tail.to_string())
        .arg("--no-color")
        .output()
        .await;
    match out {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            text
        }
        Err(e) => format!("error running docker compose logs: {e}"),
    }
}
