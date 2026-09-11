//! Every real Docker interaction shells out to the actual `docker` CLI —
//! same philosophy as cyberfleet's `git_ops.rs`: get exact behavior parity
//! (auth, output, exit codes) with what you'd get running it yourself,
//! instead of reimplementing Docker Engine API semantics against a crate
//! that has to keep up with every daemon version.

use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
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

/// Streams `docker compose -f <path> logs -f --tail <n>` line by line over
/// an mpsc channel, matching sencho's "aggregated log stream" — the live
/// counterpart to the manual-refresh `logs()` above. The child is killed
/// when the receiving end (the browser's SSE connection) goes away, since
/// dropping the `Child` in the spawned task's scope runs its `Drop`... in
/// practice we rely on the reader loop ending when the pipe closes, plus an
/// explicit `kill_on_drop` so a disconnected client doesn't leak a tailing
/// process forever.
pub fn logs_stream(compose_path: &Path, tail: u32) -> tokio::sync::mpsc::Receiver<String> {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(256);
    let compose_path = compose_path.to_path_buf();
    tokio::spawn(async move {
        let mut cmd = Command::new("docker");
        cmd.arg("compose")
            .arg("-f")
            .arg(&compose_path)
            .arg("logs")
            .arg("-f")
            .arg("--tail")
            .arg(tail.to_string())
            .arg("--no-color")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(format!("error starting log stream: {e}")).await;
                return;
            }
        };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let tx2 = tx.clone();
        let stderr_task = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx2.send(line).await.is_err() {
                        break;
                    }
                }
            }
        });

        if let Some(stdout) = stdout {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
        }
        let _ = stderr_task.await;
        let _ = child.wait().await;
    });
    rx
}

/// Compose Doctor — sencho's "preflight validation" before a save actually
/// lands. Writes the candidate content to a sibling temp file and asks
/// `docker compose config` to fully resolve it (interpolation, includes,
/// merges) without touching the real file or starting anything.
pub async fn validate_compose(compose_path: &Path, content: &str) -> Result<(), String> {
    let dir = compose_path.parent().unwrap_or_else(|| Path::new("."));
    let tmp_name = format!(".dockspace-validate-{}.yaml", std::process::id());
    let tmp_path = dir.join(&tmp_name);

    if let Err(e) = std::fs::write(&tmp_path, content) {
        return Err(format!("couldn't write temp file for validation: {e}"));
    }

    let out = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(&tmp_path)
        .arg("config")
        .arg("--quiet")
        .output()
        .await;

    let _ = std::fs::remove_file(&tmp_path);

    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(serde::Serialize)]
pub struct ImageInfo {
    pub id: String,
    pub repo_tag: String,
    pub size: String,
    pub created: String,
}

#[derive(serde::Serialize)]
pub struct VolumeInfo {
    pub name: String,
    pub driver: String,
}

#[derive(serde::Serialize)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub scope: String,
}

/// `docker images --format json` — one line of JSON per image, same
/// resources sencho's "Resources view for images, volumes, and networks"
/// lists, minus the per-image CVE scan (that needs a scanner like Trivy,
/// out of scope for this pass).
pub async fn list_images() -> Result<Vec<ImageInfo>, String> {
    let out = Command::new("docker")
        .arg("images")
        .arg("--format")
        .arg("{{json .}}")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut result = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let repo = v
                .get("Repository")
                .and_then(|s| s.as_str())
                .unwrap_or("<none>");
            let tag = v.get("Tag").and_then(|s| s.as_str()).unwrap_or("<none>");
            result.push(ImageInfo {
                id: v
                    .get("ID")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                repo_tag: format!("{repo}:{tag}"),
                size: v
                    .get("Size")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                created: v
                    .get("CreatedSince")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    Ok(result)
}

pub async fn list_volumes() -> Result<Vec<VolumeInfo>, String> {
    let out = Command::new("docker")
        .arg("volume")
        .arg("ls")
        .arg("--format")
        .arg("{{json .}}")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut result = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            result.push(VolumeInfo {
                name: v
                    .get("Name")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                driver: v
                    .get("Driver")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    Ok(result)
}

pub async fn list_networks() -> Result<Vec<NetworkInfo>, String> {
    let out = Command::new("docker")
        .arg("network")
        .arg("ls")
        .arg("--format")
        .arg("{{json .}}")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut result = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            result.push(NetworkInfo {
                name: v
                    .get("Name")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                driver: v
                    .get("Driver")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                scope: v
                    .get("Scope")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    Ok(result)
}

/// `docker {image,volume,network,system} prune -f` — sencho's "scoped
/// prune actions". `kind` is one of "images" / "volumes" / "networks" /
/// "all" (the last being a full `system prune -f`, unused volumes still
/// excluded unless the caller passes `--volumes` — deliberately not wired
/// here since dropping a volume is destructive and should stay an explicit,
/// separate click).
pub async fn prune(kind: &str) -> (bool, String) {
    let mut cmd = Command::new("docker");
    match kind {
        "images" => {
            cmd.arg("image").arg("prune").arg("-f").arg("-a");
        }
        "volumes" => {
            cmd.arg("volume").arg("prune").arg("-f");
        }
        "networks" => {
            cmd.arg("network").arg("prune").arg("-f");
        }
        "all" => {
            cmd.arg("system").arg("prune").arg("-f");
        }
        _ => return (false, format!("unknown prune target '{kind}'")),
    }
    match cmd.output().await {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), text)
        }
        Err(e) => (false, e.to_string()),
    }
}
