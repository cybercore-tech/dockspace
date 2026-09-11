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
<div class="bg-scanlines" aria-hidden="true"></div>
<div class="bg-glow" aria-hidden="true"></div>
<header class="topbar">
  <span class="brand">DOCKSPACE<span class="brand-accent">//</span></span>
  <div class="topbar-right">
    <a href="/stacks/new" class="btn-ghost">+ New Stack</a>
    <select id="theme-picker" onchange="applyTheme(this.value)">
      {theme_options}
    </select>
  </div>
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

/// A little terminal-window titlebar — three dots + a title, optionally a
/// real close button. Same motif as the cybercore system panel site.
fn titlebar(title: &str, close_href: Option<&str>) -> String {
    let close = close_href
        .map(|href| format!(r##"<a href="{href}" class="win-close" title="Close">&#10005;</a>"##))
        .unwrap_or_default();
    format!(
        r#"<div class="titlebar">
  <span class="dot dot-a"></span><span class="dot dot-b"></span><span class="dot dot-c"></span>
  <span class="titlebar-text">{title}</span>
  {close}
</div>"#
    )
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

pub fn stack_card(
    stack: &Stack,
    status: &StackStatus,
    working: bool,
    error: Option<&str>,
) -> String {
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

    let badge = if working {
        r#"<span class="badge status-working">WORKING&hellip;</span>"#.to_string()
    } else {
        format!(
            r#"<span class="badge {}">{}</span>"#,
            status_class(status),
            status_label(status)
        )
    };

    let error_banner = error
        .map(|e| {
            let short: String = e.lines().last().unwrap_or(e).chars().take(140).collect();
            format!(
                r##"<p class="card-error" title="{}">&#9888; {}</p>"##,
                escape(e),
                escape(&short)
            )
        })
        .unwrap_or_default();

    let disabled = if working { "disabled" } else { "" };

    format!(
        r##"<div class="card" id="card-{id}">
  <div class="card-head">
    <span class="card-title">{name}</span>
    {badge}
  </div>
  <p class="card-services muted">{services}</p>
  <p class="card-ports">{ports}</p>
  {error_banner}
  <div class="card-actions">
    <button hx-post="/stacks/{id}/up" hx-target="#card-{id}" hx-swap="outerHTML" {disabled}>Start</button>
    <button hx-post="/stacks/{id}/down" hx-target="#card-{id}" hx-swap="outerHTML" class="btn-danger" {disabled}>Stop</button>
    <button hx-post="/stacks/{id}/restart" hx-target="#card-{id}" hx-swap="outerHTML" {disabled}>Restart</button>
    <a href="/stacks/{id}/logs" class="btn-link">Logs</a>
    <a href="/stacks/{id}/compose" class="btn-link">Compose</a>
  </div>
</div>"##,
        name = stack.name,
    )
}

pub fn logs_page(name: &str, id: &str, log_text: &str) -> String {
    format!(
        r##"<div class="window">
  {titlebar}
  <div class="window-body">
    <div class="page-actions">
      <button hx-get="/stacks/{id}/logs/refresh" hx-target="#log-body" hx-swap="innerHTML">Refresh</button>
    </div>
    <pre id="log-body" class="log-view">{log_text}</pre>
  </div>
</div>"##,
        titlebar = titlebar(&format!("{name} — logs"), Some("/")),
        log_text = escape(log_text)
    )
}

pub fn compose_page(
    name: &str,
    id: &str,
    path: &str,
    content: &str,
    message: Option<&str>,
) -> String {
    let msg_html = message
        .map(|m| format!(r#"<p class="save-msg">{}</p>"#, escape(m)))
        .unwrap_or_default();
    format!(
        r##"<div class="window">
  {titlebar}
  <div class="window-body">
    <p class="muted"><code>{path}</code></p>
    {msg_html}
    <form method="post" action="/stacks/{id}/compose">
      <textarea name="content" class="editor" spellcheck="false" rows="24">{content}</textarea>
      <div class="page-actions">
        <button type="submit">Save</button>
        <a href="/" class="btn-ghost">Close without saving</a>
      </div>
    </form>
  </div>
</div>"##,
        titlebar = titlebar(&format!("{name} — compose file"), Some("/")),
        content = escape(content),
    )
}

pub fn new_stack_form(error: Option<&str>) -> String {
    let error_html = error
        .map(|e| format!(r#"<p class="card-error">&#9888; {}</p>"#, escape(e)))
        .unwrap_or_default();
    format!(
        r##"<div class="window">
  {titlebar}
  <div class="window-body">
    <p class="muted">Creates a new folder under <code>~/Containers</code> with a starter <code>compose.yaml</code> (a placeholder Alpine service) — edit it in the compose window that opens after.</p>
    {error_html}
    <form method="post" action="/stacks/new">
      <label class="field-label" for="name">Stack name (may include a category prefix, e.g. <code>services/myapp</code>)</label>
      <input type="text" name="name" id="name" class="text-input" placeholder="services/myapp" required>
      <div class="page-actions">
        <button type="submit">Create</button>
        <a href="/" class="btn-ghost">Cancel</a>
      </div>
    </form>
  </div>
</div>"##,
        titlebar = titlebar("new stack", Some("/")),
    )
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
