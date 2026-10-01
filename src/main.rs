mod activity;
mod cybergrid;
mod dockercmd;
mod guard;
mod scanner;
mod views;

use axum::{
    extract::{Path as AxPath, State},
    http::header,
    response::sse::{Event, Sse},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    root: PathBuf,
    /// Stack names with an action (`up`/`down`/`restart`) currently running
    /// in the background. Checked before ever calling `docker compose ps`
    /// so a slow first-run image pull shows "WORKING…" instead of just
    /// looking dead for however many minutes the pull takes.
    pending: Arc<Mutex<HashSet<String>>>,
    /// Last failed action's output per stack, cleared the next time that
    /// stack's action succeeds. Surfaced as an error banner on its card —
    /// previously a failure was silently swallowed entirely.
    last_error: Arc<Mutex<HashMap<String, String>>>,
    /// Recent-activity feed — sencho's audit log, scaled down (see
    /// `activity.rs`).
    activity: Arc<activity::Log>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let root = std::env::var("DOCKSPACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| scanner::default_root());
    tracing::info!("scanning stacks under {}", root.display());

    let state = AppState {
        root,
        pending: Arc::new(Mutex::new(HashSet::new())),
        last_error: Arc::new(Mutex::new(HashMap::new())),
        activity: Arc::new(activity::Log::load()),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/partial/stacks", get(partial_stacks))
        .route("/stacks/new", get(new_stack_form).post(new_stack_create))
        .route("/stacks/:id/up", post(action_up))
        .route("/stacks/:id/down", post(action_down))
        .route("/stacks/:id/restart", post(action_restart))
        .route("/stacks/:id/logs", get(logs_view))
        .route("/stacks/:id/logs/refresh", get(logs_refresh))
        .route("/stacks/:id/logs/stream", get(logs_stream_sse))
        .route("/stacks/:id/compose", get(compose_view).post(compose_save))
        .route("/stacks/:id/compose/validate", post(compose_validate))
        .route("/resources", get(resources_view))
        .route("/resources/prune/:kind", post(resources_prune))
        .route("/activity", get(activity_view))
        .route("/api/summary", get(api_summary))
        .route("/api/activity", get(api_activity))
        .route("/api/telemetry", get(api_telemetry))
        // Separate literal routes rather than a `:action` wildcard segment —
        // axum's router can't mix a wildcard and literal segments (logs,
        // dockerfile) at the same path position even across HTTP methods.
        .route("/api/container/:name/start", post(api_container_start))
        .route("/api/container/:name/stop", post(api_container_stop))
        .route("/api/container/:name/restart", post(api_container_restart))
        .route("/api/container/:name/logs", get(api_container_logs))
        .route(
            "/api/container/:name/dockerfile",
            get(api_container_dockerfile),
        )
        .route("/api/cybergrid/themes", get(cybergrid::list_themes))
        .route("/api/cybergrid/css/:name", get(cybergrid::theme_css))
        .route("/vendor/tokens.css", get(tokens_css))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
        // Refuse cross-site / DNS-rebound requests (see guard.rs).
        .layer(axum::middleware::from_fn(guard::middleware));

    // Loopback-only by design: this controls docker containers with zero
    // auth in v1. Fine on a single-user box reached over an SSH tunnel or
    // locally; don't put this behind a public bind address as-is.
    let port: u16 = std::env::var("DOCKSPACE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7070);
    guard::init(port);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("dockspace listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("server error");
}

async fn tokens_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, cybercore::tokens::CSS_CONTENT_TYPE)],
        cybercore::tokens::CSS,
    )
}

/// Wraps a body in the full page shell, pulling the active CYBERGRID theme
/// name + the full theme list for the picker dropdown.
fn page(body: String) -> Html<String> {
    let schema = cybercore::schema::load();
    let active = schema.active.clone();
    let options: String = schema
        .theme_names()
        .map(|n| {
            let selected = if n == active { " selected" } else { "" };
            format!("<option value=\"{n}\"{selected}>{n}</option>")
        })
        .collect();
    Html(views::layout(&active, &options, &body))
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let cards = render_all_cards(&state).await;
    let count = scanner::scan(&state.root).len();
    page(views::dashboard(&cards, count))
}

async fn partial_stacks(State(state): State<AppState>) -> Html<String> {
    Html(render_all_cards(&state).await)
}

/// A plain-JSON counterpart to the dashboard — for anything that wants the
/// numbers without parsing HTML, e.g. a Quickshell bar widget or HUD panel.
async fn api_summary(State(state): State<AppState>) -> Json<serde_json::Value> {
    let stacks = scanner::scan(&state.root);
    let (mut running, mut partial, mut stopped, mut unknown, mut working) = (0, 0, 0, 0, 0);

    // Same concurrency fix as render_all_cards — see its comment. Stacks
    // already mid-action are resolved locally (no docker call needed) and
    // don't join the concurrent batch at all.
    let mut to_check = Vec::new();
    for stack in &stacks {
        if state.pending.lock().unwrap().contains(&stack.name) {
            working += 1;
        } else {
            to_check.push(stack);
        }
    }
    let futures = to_check.iter().map(|stack| {
        let compose_path = stack.abs_path.join(&stack.compose_file);
        async move { dockercmd::derive_status(&dockercmd::ps(&compose_path).await) }
    });
    for status in futures_util::future::join_all(futures).await {
        match status {
            dockercmd::StackStatus::Running => running += 1,
            dockercmd::StackStatus::Partial => partial += 1,
            dockercmd::StackStatus::Stopped => stopped += 1,
            dockercmd::StackStatus::Unknown => unknown += 1,
        }
    }

    Json(serde_json::json!({
        "total": stacks.len(),
        "running": running,
        "partial": partial,
        "stopped": stopped,
        "unknown": unknown,
        "working": working,
    }))
}

/// Every stack's status is an independent `docker compose ps` — awaiting
/// them one at a time made total latency scale with stack count (19 stacks
/// × ~150-300ms of subprocess overhead each easily passed a 2s client-side
/// timeout, e.g. Mission Control's HUD panel). Running them concurrently
/// bounds latency to the slowest single stack instead of the sum of all of
/// them; `join_all` preserves input order so the grid doesn't reshuffle.
async fn render_all_cards(state: &AppState) -> String {
    let stacks = scanner::scan(&state.root);
    let futures = stacks.iter().map(|stack| render_one_card(state, stack));
    futures_util::future::join_all(futures).await.concat()
}

async fn render_one_card(state: &AppState, stack: &scanner::Stack) -> String {
    let working = state.pending.lock().unwrap().contains(&stack.name);
    let error = state.last_error.lock().unwrap().get(&stack.name).cloned();

    if working {
        // Skip the docker ps round-trip entirely while an action's in
        // flight — the daemon can be fully occupied pulling a large image
        // and a status query would just queue up behind it.
        return views::stack_card(
            stack,
            &dockercmd::StackStatus::Unknown,
            true,
            error.as_deref(),
            None,
        );
    }

    let compose_path = stack.abs_path.join(&stack.compose_file);
    let ps_result = dockercmd::ps(&compose_path).await;
    let status = dockercmd::derive_status(&ps_result);
    let containers = ps_result.as_ref().ok().map(|v| v.as_slice());
    views::stack_card(stack, &status, false, error.as_deref(), containers)
}

fn find_stack(state: &AppState, id: &str) -> Option<scanner::Stack> {
    let name = scanner::decode_name(id);
    scanner::scan(&state.root)
        .into_iter()
        .find(|s| s.name == name)
}

/// Marks the stack pending, spawns the real (possibly slow) docker command
/// in the background instead of awaiting it, and returns immediately with
/// an optimistic "WORKING…" card. The next `/partial/stacks` poll (every
/// 4s) picks up the real outcome once the background task clears the
/// pending flag — success clears any stale error, failure records one.
async fn run_action(state: AppState, id: String, action: &'static str) -> Html<String> {
    let Some(stack) = find_stack(&state, &id) else {
        return Html("<div class=\"card\">stack not found — rescan?</div>".to_string());
    };
    let compose_path = stack.abs_path.join(&stack.compose_file);

    state.pending.lock().unwrap().insert(stack.name.clone());
    state.last_error.lock().unwrap().remove(&stack.name);

    let pending = state.pending.clone();
    let last_error = state.last_error.clone();
    let activity = state.activity.clone();
    let name = stack.name.clone();
    tokio::spawn(async move {
        let (ok, output) = dockercmd::action(&compose_path, action).await;
        if !ok {
            let tail: String = output.lines().rev().take(20).collect::<Vec<_>>().join("\n");
            last_error.lock().unwrap().insert(name.clone(), tail);
        }
        let detail = if ok { None } else { Some(output) };
        activity.record(action, name.clone(), ok, detail);
        pending.lock().unwrap().remove(&name);
    });

    Html(views::stack_card(
        &stack,
        &dockercmd::StackStatus::Unknown,
        true,
        None,
        None,
    ))
}

async fn action_up(State(state): State<AppState>, AxPath(id): AxPath<String>) -> Html<String> {
    run_action(state, id, "up").await
}
async fn action_down(State(state): State<AppState>, AxPath(id): AxPath<String>) -> Html<String> {
    run_action(state, id, "down").await
}
async fn action_restart(State(state): State<AppState>, AxPath(id): AxPath<String>) -> Html<String> {
    run_action(state, id, "restart").await
}

async fn logs_view(State(state): State<AppState>, AxPath(id): AxPath<String>) -> Html<String> {
    let Some(stack) = find_stack(&state, &id) else {
        return page("<p>stack not found</p>".to_string());
    };
    let compose_path = stack.abs_path.join(&stack.compose_file);
    let log_text = dockercmd::logs(&compose_path, 200).await;
    page(views::logs_page(&stack.name, &id, &log_text))
}

async fn logs_refresh(State(state): State<AppState>, AxPath(id): AxPath<String>) -> Html<String> {
    let Some(stack) = find_stack(&state, &id) else {
        return Html("stack not found".to_string());
    };
    let compose_path = stack.abs_path.join(&stack.compose_file);
    let log_text = dockercmd::logs(&compose_path, 200).await;
    Html(views::escape(&log_text))
}

async fn compose_view(State(state): State<AppState>, AxPath(id): AxPath<String>) -> Html<String> {
    let Some(stack) = find_stack(&state, &id) else {
        return page("<p>stack not found</p>".to_string());
    };
    let compose_path = stack.abs_path.join(&stack.compose_file);
    let content = std::fs::read_to_string(&compose_path).unwrap_or_default();
    page(views::compose_page(
        &stack.name,
        &id,
        &compose_path.display().to_string(),
        &content,
        None,
    ))
}

async fn compose_save(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
    body: String,
) -> Html<String> {
    let Some(stack) = find_stack(&state, &id) else {
        return page("<p>stack not found</p>".to_string());
    };
    let compose_path = stack.abs_path.join(&stack.compose_file);

    // axum's default form/body extraction would try to urlencode-decode
    // this as `content=...` — the client sends the raw textarea value as
    // `content=<urlencoded>`, so decode that one field by hand rather than
    // pull in a form extractor for a single field.
    let raw = body
        .strip_prefix("content=")
        .map(urlencoded_decode)
        .unwrap_or(body);

    let saved_msg = match std::fs::write(&compose_path, &raw) {
        Ok(()) => {
            state
                .activity
                .record("save", stack.name.clone(), true, None);
            None
        }
        Err(e) => {
            state
                .activity
                .record("save", stack.name.clone(), false, Some(e.to_string()));
            Some(format!("save failed: {e}"))
        }
    };

    page(views::compose_page(
        &stack.name,
        &id,
        &compose_path.display().to_string(),
        &raw,
        saved_msg.as_deref().or(Some("saved")),
    ))
}

fn urlencoded_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

async fn new_stack_form() -> Html<String> {
    page(views::new_stack_form(None))
}

async fn new_stack_create(State(state): State<AppState>, body: String) -> Html<String> {
    let name = body
        .strip_prefix("name=")
        .map(urlencoded_decode)
        .unwrap_or_default();
    let name = name.trim().trim_matches('/');

    if name.is_empty() || name.contains("..") {
        return page(views::new_stack_form(Some("invalid name")));
    }

    let dir = state.root.join(name);
    if dir.exists() {
        return page(views::new_stack_form(Some(
            "a stack with that name already exists",
        )));
    }
    if std::fs::create_dir_all(&dir).is_err() {
        return page(views::new_stack_form(Some(
            "couldn't create that directory",
        )));
    }

    let starter = "services:\n  app:\n    image: alpine:3.20\n    command: [\"tail\", \"-f\", \"/dev/null\"]\n";
    let compose_path = dir.join("compose.yaml");
    if std::fs::write(&compose_path, starter).is_err() {
        return page(views::new_stack_form(Some(
            "created the folder but couldn't write compose.yaml",
        )));
    }

    let id = scanner::encode_name(name);
    state
        .activity
        .record("create", name.to_string(), true, None);
    page(views::compose_page(
        name,
        &id,
        &compose_path.display().to_string(),
        starter,
        Some("stack created — edit the placeholder above, then Save"),
    ))
}

/// Compose Doctor — validates the *edited* textarea content (not what's on
/// disk yet) against `docker compose config`, matching sencho's preflight
/// check. Doesn't touch the real compose file either way.
async fn compose_validate(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
    body: String,
) -> Html<String> {
    let Some(stack) = find_stack(&state, &id) else {
        return Html(views::validate_result(false, "stack not found"));
    };
    let compose_path = stack.abs_path.join(&stack.compose_file);
    let raw = body
        .strip_prefix("content=")
        .map(urlencoded_decode)
        .unwrap_or(body);

    match dockercmd::validate_compose(&compose_path, &raw).await {
        Ok(()) => Html(views::validate_result(true, "")),
        Err(e) => Html(views::validate_result(false, &e)),
    }
}

/// A live tail of `docker compose logs -f`, pushed to the browser over
/// Server-Sent Events — the "aggregated log stream" sencho does, scoped to
/// one stack at a time here rather than the whole fleet at once.
async fn logs_stream_sse(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let compose_path = find_stack(&state, &id)
        .map(|s| s.abs_path.join(&s.compose_file))
        .unwrap_or_default();

    let rx = dockercmd::logs_stream(&compose_path, 200);
    let stream =
        tokio_stream::wrappers::ReceiverStream::new(rx).map(|line| Ok(Event::default().data(line)));

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

async fn resources_view() -> Html<String> {
    let (images, volumes, networks) = tokio::join!(
        dockercmd::list_images(),
        dockercmd::list_volumes(),
        dockercmd::list_networks()
    );
    page(views::resources_page(&images, &volumes, &networks))
}

async fn resources_prune(
    State(state): State<AppState>,
    AxPath(kind): AxPath<String>,
) -> Html<String> {
    let (ok, output) = dockercmd::prune(&kind).await;
    let detail = if ok { None } else { Some(output) };
    state.activity.record("prune", kind, ok, detail);

    let (images, volumes, networks) = tokio::join!(
        dockercmd::list_images(),
        dockercmd::list_volumes(),
        dockercmd::list_networks()
    );
    Html(views::resources_body(&images, &volumes, &networks))
}

async fn activity_view(State(state): State<AppState>) -> Html<String> {
    page(views::activity_page(&state.activity.recent(100)))
}

async fn api_activity(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "entries": state.activity.recent_json(30) }))
}

async fn container_action_handler(
    state: AppState,
    name: String,
    action: &'static str,
) -> Json<serde_json::Value> {
    let (ok, output) = dockercmd::container_action(&name, action).await;
    state.activity.record(
        action,
        name.clone(),
        ok,
        if ok { None } else { Some(output.clone()) },
    );
    Json(serde_json::json!({ "ok": ok, "output": output }))
}

async fn api_container_start(
    State(state): State<AppState>,
    AxPath(name): AxPath<String>,
) -> Json<serde_json::Value> {
    container_action_handler(state, name, "start").await
}
async fn api_container_stop(
    State(state): State<AppState>,
    AxPath(name): AxPath<String>,
) -> Json<serde_json::Value> {
    container_action_handler(state, name, "stop").await
}
async fn api_container_restart(
    State(state): State<AppState>,
    AxPath(name): AxPath<String>,
) -> Json<serde_json::Value> {
    container_action_handler(state, name, "restart").await
}

async fn api_container_logs(AxPath(name): AxPath<String>) -> Json<serde_json::Value> {
    let text = dockercmd::container_logs(&name, 200).await;
    Json(serde_json::json!({ "logs": text }))
}

/// Best-effort: does this container correspond to a service in one of the
/// compose stacks dockspace already scans, and does that stack's directory
/// have a Dockerfile? Most host containers are pulled images with no local
/// Dockerfile at all (vault, sencho) — this says so honestly rather than
/// guessing.
async fn api_container_dockerfile(
    State(state): State<AppState>,
    AxPath(name): AxPath<String>,
) -> Json<serde_json::Value> {
    let stacks = scanner::scan(&state.root);
    for stack in &stacks {
        let matches = stack.services.iter().any(|s| s == &name) || stack.name.ends_with(&name);
        if !matches {
            continue;
        }
        let df = stack.abs_path.join("Dockerfile");
        if df.is_file() {
            let content = std::fs::read_to_string(&df).unwrap_or_default();
            return Json(serde_json::json!({
                "found": true,
                "path": df.display().to_string(),
                "content": content,
            }));
        }
    }
    Json(serde_json::json!({ "found": false }))
}

/// Docker/host telemetry for Mission Control's "Docker & Container
/// Telemetry" section — bundled into one call since it's all cheap,
/// host-wide (not scoped to a compose stack), and always shown together.
async fn api_telemetry() -> Json<serde_json::Value> {
    let (containers, zombies, proxies) = tokio::join!(
        dockercmd::list_all_containers(),
        async { dockercmd::zombie_count() },
        dockercmd::docker_proxy_count()
    );

    let running_ports: usize = match &containers {
        Ok(cs) => cs.iter().filter(|c| c.state == "running").count(),
        Err(_) => 0,
    };

    Json(serde_json::json!({
        "containers": containers.as_ref().ok(),
        "containers_error": containers.err(),
        "zombie_processes": zombies,
        "docker_proxy_count": proxies,
        // A rough orphan signal: more proxy processes than 2x running
        // containers suggests some are left over from stacks no longer
        // up. Not exact (containers can publish >1 port each), but a
        // real, honest heuristic rather than a guess with no basis.
        "docker_proxy_expected_max": running_ports * 2,
    }))
}
