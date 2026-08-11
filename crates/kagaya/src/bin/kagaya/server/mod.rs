mod api;

use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rust_embed::RustEmbed;
use std::net::SocketAddr;

#[derive(RustEmbed)]
#[folder = "../../ui/build"]
struct UiAssets;

pub fn router() -> Router {
    Router::new()
        .route("/api/version", get(api::version))
        .route("/api/services", get(api::list_services))
        .route("/api/services/{name}", get(api::service_detail))
        .route("/api/services/{name}/start", post(api::start))
        .route("/api/services/{name}/stop", post(api::stop))
        .route("/api/services/{name}/reload", post(api::reload))
        .route(
            "/api/services/{name}/processes/{process}/restart",
            post(api::restart_process),
        )
        .route(
            "/api/services/{name}/processes/{process}/kill",
            post(api::kill_process),
        )
        .route("/ws/echo/{name}", get(api::ws_echo))
        .route("/api/host-info", get(api::host_info))
        .route("/api/autostart", get(api::autostart_status))
        .route("/api/autostart/on", post(api::autostart_on))
        .route("/api/autostart/off", post(api::autostart_off))
        .fallback(static_handler)
}

pub async fn run() {
    let app = router();
    let port = crate::config::load_global_config().daemon.port;

    // Bound per-service log growth (issue #6): launchd has no native rotation, so
    // the daemon copytruncates oversized logs. `interval` fires once immediately,
    // so this also trims on startup, then every 10 minutes.
    tokio::spawn(async {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(600));
        loop {
            tick.tick().await;
            let _ = tokio::task::spawn_blocking(crate::logs::enforce_log_caps).await;
        }
    });

    // Listen on both loopback stacks (127.0.0.1 and ::1) so a local reverse
    // proxy reaches us whether it dials localhost via IPv4 or IPv6. Both are
    // loopback-only — never 0.0.0.0.
    let v4 = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .unwrap_or_else(|e| panic!("bind 127.0.0.1:{}: {}", port, e));
    match tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port))).await {
        Ok(v6) => {
            let app6 = app.clone();
            let s4 = tokio::spawn(async move { axum::serve(v4, app).await });
            let s6 = tokio::spawn(async move { axum::serve(v6, app6).await });
            let _ = tokio::try_join!(s4, s6);
        }
        Err(_) => {
            axum::serve(v4, app).await.expect("serve");
        }
    }
}

/// Asset filenames under `_app/immutable` carry a content hash, so they can be
/// cached forever. Everything else — index.html above all — must revalidate, or
/// a browser pins an old build and never sees a `ky self update`.
fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match UiAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, cache_control_for(path)),
                ],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => match UiAssets::get("index.html") {
            Some(content) => (
                [(header::CACHE_CONTROL, "no-cache")],
                Html(content.data.into_owned()),
            )
                .into_response(),
            None => (StatusCode::NOT_FOUND, "ui not built").into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn version_route_returns_pkg_version() {
        let res = router()
            .oneshot(
                Request::builder()
                    .uri("/api/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn start_unknown_service_is_404() {
        let res = router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/services/__nope__/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn unknown_service_detail_is_404() {
        let res = router()
            .oneshot(
                Request::builder()
                    .uri("/api/services/__nope__")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn html_revalidates_while_hashed_assets_are_immutable() {
        assert_eq!(cache_control_for("index.html"), "no-cache");
        assert_eq!(cache_control_for("settings.html"), "no-cache");
        assert_eq!(
            cache_control_for("_app/immutable/nodes/2.abc123.js"),
            "public, max-age=31536000, immutable"
        );
        // A path that merely mentions immutable must not be cached forever.
        assert_eq!(cache_control_for("docs/immutable/guide.html"), "no-cache");
    }

    #[tokio::test]
    async fn autostart_status_route_ok() {
        let res = router()
            .oneshot(
                Request::builder()
                    .uri("/api/autostart")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
