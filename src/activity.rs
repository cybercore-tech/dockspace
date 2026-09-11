//! A tiny in-memory audit log — sencho's "read-only audit log of every
//! action" scaled down to what a single-machine, no-auth tool needs: no
//! user attribution (there's only one user), no persistence across a
//! restart, just a recent-activity feed so "wait, did that restart
//! actually happen, and when?" has an answer without going to `journalctl`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ENTRIES: usize = 200;

pub struct Entry {
    pub ts: u64,
    pub verb: &'static str,
    pub subject: String,
    pub ok: bool,
    pub detail: Option<String>,
}

pub struct Log(Mutex<VecDeque<Entry>>);

impl Log {
    pub fn new() -> Self {
        Log(Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)))
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
        let mut log = self.0.lock().unwrap();
        if log.len() >= MAX_ENTRIES {
            log.pop_front();
        }
        log.push_back(Entry {
            ts,
            verb,
            subject: subject.into(),
            ok,
            detail,
        });
    }

    /// Newest first.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let log = self.0.lock().unwrap();
        log.iter().rev().take(n).map(render_entry).collect()
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
        verb = crate::views::escape(e.verb),
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
