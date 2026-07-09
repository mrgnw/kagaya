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

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match UiAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => match UiAssets::get("index.html") {
            Some(content) => Html(content.data.into_owned()).into_response(),
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
