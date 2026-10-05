//! Plain string-built HTML — no template engine dependency, matching the
//! minimal-deps approach used across the rest of this ecosystem. Every
//! function returns a complete HTML fragment; `layout()` wraps a body in
//! the full page shell.

use crate::dockercmd::{ContainerState, ImageInfo, NetworkInfo, StackStatus, VolumeInfo};
use crate::scanner::{encode_name, Stack};

pub fn layout(body: &str) -> String {
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
    <a href="/resources" class="btn-ghost">Resources</a>
    <a href="/activity" class="btn-ghost">Activity</a>
    <a href="/stacks/new" class="btn-ghost">+ New Stack</a>
    <select id="theme-picker" aria-label="Cybercore theme" onchange="applyTheme(this.value)">
      <option>Loading themes…</option>
    </select>
    <a href="http://127.0.0.1:8761/" class="btn-ghost" target="_blank" rel="noopener">Theme Studio ↗</a>
  </div>
</header>
<main>
{body}
</main>
<script>
let themeAppearance = 'dark';
async function applyTheme(id, persist = true) {{
  const picker = document.getElementById('theme-picker');
  if (persist) {{
    const selected = await fetch('/api/cybergrid/active/' + encodeURIComponent(id), {{ method: 'POST' }});
    if (!selected.ok) throw new Error('Could not save the shared Cybercore theme.');
  }}
  const response = await fetch('/api/cybergrid/css/' + encodeURIComponent(id) + '?appearance=' + themeAppearance);
  if (!response.ok) throw new Error('Could not load the selected Cybercore theme.');
  document.getElementById('theme-vars').textContent = await response.text();
  picker.value = id;
}}
window.addEventListener('DOMContentLoaded', async () => {{
  try {{
    const response = await fetch('/api/cybergrid/themes');
    if (!response.ok) throw new Error('Theme catalog request failed.');
    const catalog = await response.json();
    themeAppearance = catalog.appearance || 'dark';
    const picker = document.getElementById('theme-picker');
    const groups = new Map();
    for (const theme of catalog.themes) {{
      const family = theme.family || 'Other';
      if (!groups.has(family)) groups.set(family, []);
      groups.get(family).push(theme);
    }}
    picker.replaceChildren(...[...groups].map(([family, themes]) => {{
      const group = document.createElement('optgroup');
      group.label = family;
      for (const theme of themes) {{
        const option = document.createElement('option');
        option.value = theme.id;
        option.textContent = theme.name;
        group.append(option);
      }}
      return group;
    }}));
    await applyTheme(catalog.active, false);
  }} catch (error) {{
    console.error('Could not load Cybercore themes:', error);
  }}
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
  <input id="stack-search" class="text-input search-input" type="search"
         placeholder="Filter by stack or service name&hellip;" oninput="filterStacks(this.value)">
</div>
<div id="stack-grid" class="grid" hx-get="/partial/stacks" hx-trigger="every 4s" hx-swap="innerHTML" hx-on::after-swap="filterStacks(document.getElementById('stack-search').value)">
{cards_html}
</div>
<script>
function filterStacks(q) {{
  q = q.trim().toLowerCase();
  document.querySelectorAll('#stack-grid .card').forEach(card => {{
    const hay = (card.dataset.name + ' ' + card.dataset.services).toLowerCase();
    card.hidden = q.length > 0 && !hay.includes(q);
  }});
}}
</script>"#
    )
}

pub fn stack_card(
    stack: &Stack,
    status: &StackStatus,
    working: bool,
    error: Option<&str>,
    containers: Option<&[ContainerState]>,
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

    // Per-service breakdown — sencho shows each container's own state, not
    // just an aggregate rollup. Skipped while working (nothing meaningful
    // to report yet) or when the ps query itself failed.
    let service_rows = containers
        .filter(|c| !working && !c.is_empty())
        .map(|containers| {
            let rows: String = containers
                .iter()
                .map(|c| {
                    let dot_class = if c.state == "running" {
                        "svc-dot-up"
                    } else {
                        "svc-dot-down"
                    };
                    format!(
                        r##"<li><span class="svc-dot {dot_class}"></span>{svc} <span class="muted">({state})</span></li>"##,
                        svc = escape(&c.service),
                        state = escape(&c.state),
                    )
                })
                .collect();
            format!(r##"<ul class="service-list">{rows}</ul>"##)
        })
        .unwrap_or_default();

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
        r##"<div class="card" id="card-{id}" data-name="{name}" data-services="{services}">
  <div class="card-head">
    <span class="card-title">{name}</span>
    {badge}
  </div>
  <p class="card-services muted">{services}</p>
  <p class="card-ports">{ports}</p>
  {service_rows}
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
      <button hx-get="/stacks/{id}/logs/refresh" hx-target="#log-body" hx-swap="innerHTML">Refresh snapshot</button>
      <button type="button" id="live-tail-btn" onclick="toggleLiveTail('{id}')">&#9679; Live tail</button>
    </div>
    <pre id="log-body" class="log-view">{log_text}</pre>
  </div>
</div>
<script>
let liveTailSource = null;
function toggleLiveTail(id) {{
  const btn = document.getElementById('live-tail-btn');
  const body = document.getElementById('log-body');
  if (liveTailSource) {{
    liveTailSource.close();
    liveTailSource = null;
    btn.classList.remove('live-on');
    btn.textContent = '● Live tail';
    return;
  }}
  body.textContent = '';
  btn.classList.add('live-on');
  btn.textContent = '● Live (streaming…)';
  liveTailSource = new EventSource('/stacks/' + id + '/logs/stream');
  liveTailSource.onmessage = (ev) => {{
    body.textContent += ev.data + '\n';
    body.scrollTop = body.scrollHeight;
  }};
  liveTailSource.onerror = () => {{
    btn.classList.remove('live-on');
    btn.textContent = '● Live tail (disconnected)';
  }};
}}
window.addEventListener('beforeunload', () => {{ if (liveTailSource) liveTailSource.close(); }});
</script>"##,
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
    <div id="validate-msg"></div>
    <form method="post" action="/stacks/{id}/compose">
      <textarea name="content" class="editor" spellcheck="false" rows="24">{content}</textarea>
      <div class="page-actions">
        <button type="button" hx-post="/stacks/{id}/compose/validate" hx-include="closest form"
                hx-target="#validate-msg" hx-swap="innerHTML">Validate</button>
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

/// The little inline result banner for the Compose Doctor "Validate"
/// button — sencho's preflight check, minus a full diff-vs-running-state
/// comparison (that needs the drift-detection logic dockspace doesn't have
/// yet).
pub fn validate_result(ok: bool, message: &str) -> String {
    if ok {
        r#"<p class="save-msg">&#10003; valid — <code>docker compose config</code> resolved it cleanly</p>"#.to_string()
    } else {
        format!(
            r##"<p class="card-error" style="white-space:pre-wrap;overflow:visible;text-overflow:clip;">&#9888; {}</p>"##,
            escape(message)
        )
    }
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

/// The Resources page — sencho's "Resources view for images, volumes, and
/// networks with scoped prune actions", scaled to what a single machine
/// needs: no per-image CVE column (no scanner wired up), just what's
/// present and a button to reclaim disk with each.
pub fn resources_page(
    images: &Result<Vec<ImageInfo>, String>,
    volumes: &Result<Vec<VolumeInfo>, String>,
    networks: &Result<Vec<NetworkInfo>, String>,
) -> String {
    format!(
        r##"<div class="window">
  {titlebar}
  <div class="window-body" id="resources-body">
    {sections}
  </div>
</div>"##,
        titlebar = titlebar("resources", Some("/")),
        sections = resources_body(images, volumes, networks),
    )
}

/// Just the three prune-able sections, no window chrome — this is both
/// what `resources_page` wraps and what the htmx prune buttons swap back
/// into `#resources-body` in place, without a full page reload.
pub fn resources_body(
    images: &Result<Vec<ImageInfo>, String>,
    volumes: &Result<Vec<VolumeInfo>, String>,
    networks: &Result<Vec<NetworkInfo>, String>,
) -> String {
    let images_html = match images {
        Ok(rows) if rows.is_empty() => r#"<p class="muted">no images</p>"#.to_string(),
        Ok(rows) => {
            let body: String = rows
                .iter()
                .map(|i| {
                    format!(
                        r#"<tr><td><code>{tag}</code></td><td class="muted">{id}</td><td>{size}</td><td class="muted">{created}</td></tr>"#,
                        tag = escape(&i.repo_tag),
                        id = escape(&i.id),
                        size = escape(&i.size),
                        created = escape(&i.created),
                    )
                })
                .collect();
            format!(
                r#"<table class="res-table"><thead><tr><th>Image</th><th>ID</th><th>Size</th><th>Created</th></tr></thead><tbody>{body}</tbody></table>"#
            )
        }
        Err(e) => format!(r#"<p class="card-error">&#9888; {}</p>"#, escape(e)),
    };

    let volumes_html = match volumes {
        Ok(rows) if rows.is_empty() => r#"<p class="muted">no volumes</p>"#.to_string(),
        Ok(rows) => {
            let body: String = rows
                .iter()
                .map(|v| {
                    format!(
                        r#"<tr><td><code>{name}</code></td><td class="muted">{driver}</td></tr>"#,
                        name = escape(&v.name),
                        driver = escape(&v.driver),
                    )
                })
                .collect();
            format!(
                r#"<table class="res-table"><thead><tr><th>Volume</th><th>Driver</th></tr></thead><tbody>{body}</tbody></table>"#
            )
        }
        Err(e) => format!(r#"<p class="card-error">&#9888; {}</p>"#, escape(e)),
    };

    let networks_html = match networks {
        Ok(rows) if rows.is_empty() => r#"<p class="muted">no networks</p>"#.to_string(),
        Ok(rows) => {
            let body: String = rows
                .iter()
                .map(|n| {
                    format!(
                        r#"<tr><td><code>{name}</code></td><td class="muted">{driver}</td><td class="muted">{scope}</td></tr>"#,
                        name = escape(&n.name),
                        driver = escape(&n.driver),
                        scope = escape(&n.scope),
                    )
                })
                .collect();
            format!(
                r#"<table class="res-table"><thead><tr><th>Network</th><th>Driver</th><th>Scope</th></tr></thead><tbody>{body}</tbody></table>"#
            )
        }
        Err(e) => format!(r#"<p class="card-error">&#9888; {}</p>"#, escape(e)),
    };

    format!(
        r##"<section class="res-section">
      <div class="res-head"><h2>Images</h2>
        <button hx-post="/resources/prune/images" hx-target="#resources-body" hx-swap="innerHTML"
                hx-confirm="Remove every image not used by a running container?" class="btn-danger">Prune unused</button>
      </div>
      {images_html}
    </section>
    <section class="res-section">
      <div class="res-head"><h2>Volumes</h2>
        <button hx-post="/resources/prune/volumes" hx-target="#resources-body" hx-swap="innerHTML"
                hx-confirm="Remove every volume not attached to a container? This deletes data." class="btn-danger">Prune unused</button>
      </div>
      {volumes_html}
    </section>
    <section class="res-section">
      <div class="res-head"><h2>Networks</h2>
        <button hx-post="/resources/prune/networks" hx-target="#resources-body" hx-swap="innerHTML"
                hx-confirm="Remove every network not used by a container?" class="btn-danger">Prune unused</button>
      </div>
      {networks_html}
    </section>"##
    )
}

/// The Activity page — sencho's "read-only audit log of every action".
pub fn activity_page(rows: &[String]) -> String {
    let body = if rows.is_empty() {
        r#"<p class="muted">nothing logged yet — start/stop/restart/save/prune actions show up here.</p>"#.to_string()
    } else {
        format!(r#"<ul class="activity-list">{}</ul>"#, rows.join(""))
    };
    format!(
        r##"<div class="window">
  {titlebar}
  <div class="window-body">
    {body}
  </div>
</div>"##,
        titlebar = titlebar("activity", Some("/")),
    )
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
