//! Finds compose stacks under a root directory (default `~/Containers`) and
//! pulls each one's declared services/ports straight out of its YAML —
//! no hand-maintained registry to go stale, unlike a README table.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Serialize)]
pub struct Stack {
    /// Path relative to the scan root, e.g. "services/vault" — also this
    /// stack's URL-safe identifier once `/` is swapped for `--` (see
    /// `encode_name`/`decode_name`).
    pub name: String,
    pub abs_path: PathBuf,
    pub compose_file: String,
    pub services: Vec<String>,
    pub ports: Vec<String>,
}

pub fn default_root() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("Containers")
}

/// Walks `root` looking for `compose.yaml`/`compose.yml`/`docker-compose.yml`
/// — the first match per directory wins, mirroring `docker compose`'s own
/// file-discovery order. Skips `.git` and any dir starting with `.`.
pub fn scan(root: &Path) -> Vec<Stack> {
    const CANDIDATES: &[&str] = &[
        "compose.yaml",
        "compose.yml",
        "docker-compose.yml",
        "docker-compose.yaml",
    ];
    let mut stacks = Vec::new();

    for entry in WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|n| !n.starts_with('.'))
                .unwrap_or(true)
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
    {
        for candidate in CANDIDATES {
            let compose_path = entry.path().join(candidate);
            if compose_path.is_file() {
                let rel = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel.is_empty() {
                    continue; // don't treat the root itself as a stack
                }
                let (services, ports) = parse_compose(&compose_path);
                stacks.push(Stack {
                    name: rel,
                    abs_path: entry.path().to_path_buf(),
                    compose_file: candidate.to_string(),
                    services,
                    ports,
                });
                break;
            }
        }
    }

    stacks.sort_by(|a, b| a.name.cmp(&b.name));
    stacks
}

fn parse_compose(path: &Path) -> (Vec<String>, Vec<String>) {
    let mut services = Vec::new();
    let mut ports = Vec::new();

    let Ok(content) = fs::read_to_string(path) else {
        return (services, ports);
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&content) else {
        return (services, ports);
    };

    if let Some(map) = value.get("services").and_then(|s| s.as_mapping()) {
        for (key, svc) in map {
            let Some(name) = key.as_str() else { continue };
            services.push(name.to_string());

            let Some(port_list) = svc.get("ports").and_then(|p| p.as_sequence()) else {
                continue;
            };
            for p in port_list {
                match p {
                    serde_yaml::Value::String(s) => ports.push(s.clone()),
                    serde_yaml::Value::Number(n) => ports.push(n.to_string()),
                    serde_yaml::Value::Mapping(m) => {
                        let target = m
                            .get(serde_yaml::Value::String("target".into()))
                            .map(scalar_to_string);
                        let published = m
                            .get(serde_yaml::Value::String("published".into()))
                            .map(scalar_to_string);
                        if let (Some(t), Some(pu)) = (target, published) {
                            ports.push(format!("{pu}:{t}"));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    (services, ports)
}

fn scalar_to_string(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Stack names contain `/` (e.g. "services/vault") — axum path params can't
/// hold that cleanly, so URLs use this reversible `--` encoding instead of
/// a multi-segment wildcard route.
pub fn encode_name(name: &str) -> String {
    name.replace('/', "--")
}

pub fn decode_name(encoded: &str) -> String {
    encoded.replace("--", "/")
}
