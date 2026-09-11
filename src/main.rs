mod cybergrid;
mod dockercmd;
mod scanner;
mod views;

use axum::{
    extract::{Path as AxPath, State},
    http::header,
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    root: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let root = std::env::var("DOCKSPACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| scanner::default_root());
    tracing::info!("scanning stacks under {}", root.display());

    let state = AppState { root };

    let app = Router::new()
        .route("/", get(index))
        .route("/partial/stacks", get(partial_stacks))
        .route("/stacks/:id/up", post(action_up))
        .route("/stacks/:id/down", post(action_down))
        .route("/stacks/:id/restart", post(action_restart))
        .route("/stacks/:id/logs", get(logs_view))
        .route("/stacks/:id/logs/refresh", get(logs_refresh))
        .route("/stacks/:id/compose", get(compose_view))
        .route("/api/cybergrid/themes", get(cybergrid::list_themes))
        .route("/api/cybergrid/css/:name", get(cybergrid::theme_css))
        .route("/vendor/tokens.css", get(tokens_css))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);

    // Loopback-only by design: this controls docker containers with zero
    // auth in v1. Fine on a single-user box reached over an SSH tunnel or
    // locally; don't put this behind a public bind address as-is.
    let addr = SocketAddr::from(([127, 0, 0, 1], 7070));
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

async fn render_all_cards(state: &AppState) -> String {
    let stacks = scanner::scan(&state.root);
    let mut out = String::new();
    for stack in &stacks {
        let compose_path = stack.abs_path.join(&stack.compose_file);
        let ps_result = dockercmd::ps(&compose_path).await;
        let status = dockercmd::derive_status(&ps_result);
        out.push_str(&views::stack_card(stack, &status));
    }
    out
}

fn find_stack(state: &AppState, id: &str) -> Option<scanner::Stack> {
    let name = scanner::decode_name(id);
    scanner::scan(&state.root)
        .into_iter()
        .find(|s| s.name == name)
}

async fn run_action(state: AppState, id: String, action: &'static str) -> Html<String> {
    let Some(stack) = find_stack(&state, &id) else {
        return Html("<div class=\"card\">stack not found — rescan?</div>".to_string());
    };
    let compose_path = stack.abs_path.join(&stack.compose_file);
    let _ = dockercmd::action(&compose_path, action).await;
    let ps_result = dockercmd::ps(&compose_path).await;
    let status = dockercmd::derive_status(&ps_result);
    Html(views::stack_card(&stack, &status))
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
        &compose_path.display().to_string(),
        &content,
    ))
}
