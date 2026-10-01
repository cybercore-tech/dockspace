//! Request guard for a no-auth, loopback-only dashboard that controls Docker.
//!
//! Binding to 127.0.0.1 keeps other machines out, but not web pages: any site
//! open in the user's browser can submit a form to `http://127.0.0.1:7070/…`
//! (cross-site request forgery), and DNS rebinding can make a hostile domain
//! resolve to 127.0.0.1. With Docker access, either one means saving a compose
//! file that mounts `/` and starting it — i.e. root. So:
//!
//! - every request must carry a loopback `Host` for our port (blocks DNS
//!   rebinding: a rebound request carries the attacker's hostname);
//! - state-changing requests (anything but GET/HEAD/OPTIONS) must not come
//!   from another site: `Sec-Fetch-Site` must be `same-origin` or `none`, and
//!   any `Origin` must be our own. Requests with neither header (curl, local
//!   scripts) are allowed — they aren't a browser acting on a page's behalf.

use axum::{
    extract::Request,
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::OnceLock;

static PORT: OnceLock<u16> = OnceLock::new();

/// Call once at startup, before serving.
pub fn init(port: u16) {
    let _ = PORT.set(port);
}

fn allowed_hosts(port: u16) -> [String; 3] {
    [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")]
}

/// Pure decision function (unit-tested below).
pub fn check(
    method: &Method,
    host: Option<&str>,
    origin: Option<&str>,
    sec_fetch_site: Option<&str>,
    port: u16,
) -> Result<(), &'static str> {
    let hosts = allowed_hosts(port);
    let Some(host) = host else { return Err("missing Host header") };
    if !hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        return Err("Host is not this loopback dashboard");
    }
    let safe = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    if safe {
        return Ok(());
    }
    if let Some(site) = sec_fetch_site {
        if !matches!(site, "same-origin" | "none") {
            return Err("cross-site request refused");
        }
    }
    if let Some(origin) = origin {
        if !hosts.iter().any(|h| origin.eq_ignore_ascii_case(&format!("http://{h}"))) {
            return Err("cross-origin request refused");
        }
    }
    Ok(())
}

pub async fn middleware(req: Request, next: Next) -> Response {
    let port = *PORT.get().unwrap_or(&7070);
    let h = req.headers();
    let get = |name: header::HeaderName| h.get(name).and_then(|v| v.to_str().ok());
    let verdict = check(
        req.method(),
        get(header::HOST),
        get(header::ORIGIN),
        h.get("sec-fetch-site").and_then(|v| v.to_str().ok()),
        port,
    );
    match verdict {
        Ok(()) => next.run(req).await,
        Err(reason) => {
            tracing::warn!("guard refused {} {}: {reason}", req.method(), req.uri().path());
            (StatusCode::FORBIDDEN, reason).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: u16 = 7070;

    #[test]
    fn same_origin_post_from_own_page_is_allowed() {
        assert!(check(&Method::POST, Some("127.0.0.1:7070"), Some("http://127.0.0.1:7070"), Some("same-origin"), P).is_ok());
        assert!(check(&Method::POST, Some("localhost:7070"), Some("http://localhost:7070"), Some("same-origin"), P).is_ok());
    }

    #[test]
    fn cross_site_form_post_is_refused() {
        assert!(check(&Method::POST, Some("127.0.0.1:7070"), Some("https://evil.example"), Some("cross-site"), P).is_err());
        // Even if a browser omitted Sec-Fetch-Site, a foreign Origin is refused.
        assert!(check(&Method::POST, Some("127.0.0.1:7070"), Some("https://evil.example"), None, P).is_err());
        // Same-site but different origin (e.g. another local port) is refused too.
        assert!(check(&Method::POST, Some("127.0.0.1:7070"), Some("http://127.0.0.1:8080"), Some("same-site"), P).is_err());
    }

    #[test]
    fn dns_rebinding_host_is_refused_even_for_get() {
        assert!(check(&Method::GET, Some("evil.example:7070"), None, None, P).is_err());
        assert!(check(&Method::GET, None, None, None, P).is_err());
    }

    #[test]
    fn plain_gets_and_header_less_local_clients_are_allowed() {
        assert!(check(&Method::GET, Some("127.0.0.1:7070"), None, Some("cross-site"), P).is_ok());
        assert!(check(&Method::POST, Some("127.0.0.1:7070"), None, None, P).is_ok()); // curl
    }

    #[test]
    fn port_must_match() {
        assert!(check(&Method::GET, Some("127.0.0.1:9999"), None, None, P).is_err());
    }
}
