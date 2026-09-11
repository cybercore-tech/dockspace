//! A tiny audit log — sencho's "read-only audit log of every action"
//! scaled down to what a single-machine, no-auth tool needs: no user
//! attribution (there's only one user), just a recent-activity feed so
//! "wait, did that restart actually happen, and when?" has an answer
//! without going to `journalctl`.
//!
//! Persisted to a small JSON file (`~/.cache/dockspace/activity.json`)
//! specifically so a service restart — which happens often during
//! development, and will keep happening — doesn't wipe it. A HUD reading
//! this over HTTP (Mission Control's "data flow" strip) going blank every
//! time dockspace gets redeployed was the actual bug this fixes.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ENTRIES: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub ts: u64,
    pub verb: String,
    pub subject: String,
    pub ok: bool,
    pub detail: Option<String>,
}

pub struct Log {
    entries: Mutex<VecDeque<Entry>>,
    path: PathBuf,
}

fn default_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".cache/dockspace/activity.json")
}

impl Log {
    /// Loads any existing log from disk; starts empty if there isn't one
    /// or it doesn't parse (a corrupt cache file is a reason to start
    /// fresh, not a reason to fail startup).
    pub fn load() -> Self {
        let path = default_path();
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<VecDeque<Entry>>(&s).ok())
            .unwrap_or_default();
        Log {
            entries: Mutex::new(entries),
            path,
        }
    }

    fn save(&self, entries: &VecDeque<Entry>) {
        let Some(parent) = self.path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        if let Ok(json) = serde_json::to_string(entries) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    pub fn record(
        &self,
        verb: &'static str,
        subject: impl Into<String>,
        ok: bool,
        detail: Option<String>,
    ) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut log = self.entries.lock().unwrap();
        if log.len() >= MAX_ENTRIES {
            log.pop_front();
        }
        log.push_back(Entry {
            ts,
            verb: verb.to_string(),
            subject: subject.into(),
            ok,
            detail,
        });
        self.save(&log);
    }

    /// Newest first.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let log = self.entries.lock().unwrap();
        log.iter().rev().take(n).map(render_entry).collect()
    }

    /// Same data as `recent`, structured — for anything that wants to
    /// render its own activity feed instead of parsing the HTML rows
    /// (e.g. a Quickshell HUD's live "data flow" strip).
    pub fn recent_json(&self, n: usize) -> Vec<serde_json::Value> {
        let log = self.entries.lock().unwrap();
        log.iter()
            .rev()
            .take(n)
            .map(|e| {
                serde_json::json!({
                    "ts": e.ts,
                    "verb": e.verb,
                    "subject": e.subject,
                    "ok": e.ok,
                })
            })
            .collect()
    }
}

fn render_entry(e: &Entry) -> String {
    let status = if e.ok { "ok" } else { "failed" };
    let when = humanize_age(e.ts);
    let detail = e
        .detail
        .as_deref()
        .map(|d| format!(r#" title="{}""#, crate::views::escape(d)))
        .unwrap_or_default();
    let cls = if e.ok { "activity-ok" } else { "activity-fail" };
    format!(
        r#"<li class="activity-row {cls}"{detail}><span class="activity-when">{when}</span><span class="activity-verb">{verb}</span><span class="activity-subject">{subject}</span><span class="activity-status">{status}</span></li>"#,
        verb = crate::views::escape(&e.verb),
        subject = crate::views::escape(&e.subject),
    )
}

fn humanize_age(ts: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(ts);
    let age = now.saturating_sub(ts);
    if age < 60 {
        format!("{age}s ago")
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else if age < 86400 {
        format!("{}h ago", age / 3600)
    } else {
        format!("{}d ago", age / 86400)
    }
}
