//! CYBERGRID palette bridge — same pattern as cyberdeck's `core` branch.
//! Exposes the shared `cybercore` crate's named colour themes over HTTP so
//! the dashboard can list and apply them.
//!
//! - `GET /api/cybergrid/themes`      — every theme + its 11 colour roles
//! - `GET /api/cybergrid/css/:name`   — a ready `:root{ --bg:#… }` block

use axum::{extract::Path, http::header, response::IntoResponse, Json};
use serde_json::{json, Value};

fn role_map(name: &str, p: &cybercore::schema::Palette) -> Value {
    json!({
        "name": name,
        "bg": p.bg,
        "white": p.white,
        "acid_green": p.acid_green,
        "hot_pink": p.hot_pink,
        "purple": p.purple,
        "cyan": p.cyan,
        "orange": p.orange,
        "red": p.red,
        "panel": p.panel,
        "line": p.line,
        "muted": p.muted,
    })
}

pub async fn list_themes() -> impl IntoResponse {
    let s = cybercore::schema::load();
    let themes: Vec<Value> = s
        .theme_names()
        .filter_map(|n| s.theme(n).map(|p| role_map(n, p)))
        .collect();
    Json(json!({ "active": s.active, "themes": themes }))
}

pub async fn theme_css(Path(name): Path<String>) -> impl IntoResponse {
    let s = cybercore::schema::load();
    let p = s.theme(&name).unwrap_or_else(|| s.active_theme());
    let css = format!(
        ":root{{\
--bg:#{bg};--fg:#{white};\
--acid:#{acid};--pink:#{pink};--purple:#{purple};--cyan:#{cyan};\
--orange:#{orange};--red:#{red};\
--panel:#{panel};--line:#{line};--muted:#{muted};\
}}\n",
        bg = p.bg,
        white = p.white,
        acid = p.acid_green,
        pink = p.hot_pink,
        purple = p.purple,
        cyan = p.cyan,
        orange = p.orange,
        red = p.red,
        panel = p.panel,
        line = p.line,
        muted = p.muted,
    );
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], css)
}
