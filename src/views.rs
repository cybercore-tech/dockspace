//! Plain string-built HTML — no template engine dependency, matching the
//! minimal-deps approach used across the rest of this ecosystem. Every
//! function returns a complete HTML fragment; `layout()` wraps a body in
//! the full page shell.

use crate::dockercmd::StackStatus;
use crate::scanner::{encode_name, Stack};

pub fn layout(active_theme: &str, theme_options: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Dockspace</title>
<link rel="stylesheet" href="/vendor/tokens.css">
<style id="theme-vars"></style>
<link rel="stylesheet" href="/static/style.css">
<script src="/static/htmx.min.js" defer></script>
</head>
<body>
<header class="topbar">
  <span class="brand">DOCKSPACE<span class="brand-accent">//</span></span>
  <select id="theme-picker" onchange="applyTheme(this.value)">
    {theme_options}
  </select>
</header>
<main>
{body}
</main>
<script>
function applyTheme(name) {{
  fetch('/api/cybergrid/css/' + name)
    .then(r => r.text())
    .then(css => {{ document.getElementById('theme-vars').textContent = css; }});
  localStorage.setItem('dockspace-theme', name);
}}
window.addEventListener('DOMContentLoaded', () => {{
  const saved = localStorage.getItem('dockspace-theme') || '{active_theme}';
  document.getElementById('theme-picker').value = saved;
  applyTheme(saved);
}});
</script>
</body>
</html>"#
    )
}

fn status_class(status: &StackStatus) -> &'static str {
    match status {
        StackStatus::Running => "status-running",
        StackStatus::Partial => "status-partial",
        StackStatus::Stopped => "status-stopped",
        StackStatus::Unknown => "status-unknown",
    }
}

fn status_label(status: &StackStatus) -> &'static str {
    match status {
        StackStatus::Running => "RUNNING",
        StackStatus::Partial => "PARTIAL",
        StackStatus::Stopped => "STOPPED",
        StackStatus::Unknown => "UNKNOWN",
    }
}

pub fn dashboard(cards_html: &str, total: usize) -> String {
    format!(
        r#"<div class="dash-head">
  <h1>Container Fleet</h1>
  <p class="muted">{total} stacks under <code>~/Containers</code> — auto-refreshes every 4s.</p>
</div>
<div id="stack-grid" class="grid" hx-get="/partial/stacks" hx-trigger="every 4s" hx-swap="innerHTML">
{cards_html}
</div>"#
    )
}

pub fn stack_card(stack: &Stack, status: &StackStatus) -> String {
    let id = encode_name(&stack.name);
    let ports = if stack.ports.is_empty() {
        "<span class=\"muted\">none published</span>".to_string()
    } else {
        stack
            .ports
            .iter()
            .map(|p| format!("<code>{p}</code>"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let services = stack.services.join(", ");

    format!(
        r##"<div class="card" id="card-{id}">
  <div class="card-head">
    <span class="card-title">{name}</span>
    <span class="badge {status_class}">{status_label}</span>
  </div>
  <p class="card-services muted">{services}</p>
  <p class="card-ports">{ports}</p>
  <div class="card-actions">
    <button hx-post="/stacks/{id}/up" hx-target="#card-{id}" hx-swap="outerHTML">Start</button>
    <button hx-post="/stacks/{id}/down" hx-target="#card-{id}" hx-swap="outerHTML" class="btn-danger">Stop</button>
    <button hx-post="/stacks/{id}/restart" hx-target="#card-{id}" hx-swap="outerHTML">Restart</button>
    <a href="/stacks/{id}/logs" class="btn-link">Logs</a>
    <a href="/stacks/{id}/compose" class="btn-link">Compose</a>
  </div>
</div>"##,
        name = stack.name,
        status_class = status_class(status),
        status_label = status_label(status),
    )
}

pub fn logs_page(name: &str, id: &str, log_text: &str) -> String {
    format!(
        r##"<div class="page-head">
  <a href="/" class="back-link">&larr; back</a>
  <h1>{name} — logs</h1>
  <button hx-get="/stacks/{id}/logs/refresh" hx-target="#log-body" hx-swap="innerHTML">Refresh</button>
</div>
<pre id="log-body" class="log-view">{log_text}</pre>"##,
        log_text = escape(log_text)
    )
}

pub fn compose_page(name: &str, path: &str, content: &str) -> String {
    format!(
        r#"<div class="page-head">
  <a href="/" class="back-link">&larr; back</a>
  <h1>{name} — compose file</h1>
  <p class="muted"><code>{path}</code> — read-only for now.</p>
</div>
<pre class="log-view">{content}</pre>"#,
        content = escape(content)
    )
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
