use anyhow::Result;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST, ORIGIN, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::memory::Brain;
use crate::app::memory::model::MemoryError;
use crate::app::tools::{self, Tool};
use crate::config::HttpConfig;

const MAX_BODY_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

// A bind failure is logged and skipped, so MCP keeps working.
pub async fn serve(
    brain: Arc<Brain>,
    config: HttpConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let listener = match tokio::net::TcpListener::bind(config.addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!("HTTP API not started, cannot bind {}: {error}", config.addr);
            return Ok(());
        }
    };
    tracing::info!("HTTP API listening on {}", config.addr);
    axum::serve(listener, router(brain, config))
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

fn router(brain: Arc<Brain>, config: HttpConfig) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/tools", get(list_tools))
        .route("/v1/tools/{name}", post(call_tool))
        .fallback(|| async { problem(StatusCode::NOT_FOUND, "no such resource") })
        .method_not_allowed_fallback(|| async {
            problem(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
        })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(Arc::new(config), guard))
        .with_state(brain)
}

async fn guard(State(config): State<Arc<HttpConfig>>, request: Request, next: Next) -> Response {
    if let Err(denied) = authorize(&config, request.headers()) {
        log_denial(&denied);
        return denied.into_response();
    }
    match tokio::time::timeout(REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => response,
        Err(_) => problem(StatusCode::SERVICE_UNAVAILABLE, "request timed out"),
    }
}

#[derive(Debug, PartialEq)]
enum Denied {
    ForeignHost,
    BrowserOrigin,
    BadToken,
}

impl Denied {
    fn reason(&self) -> &'static str {
        match self {
            Self::ForeignHost => "unexpected Host header",
            Self::BrowserOrigin => "cross-origin browser request",
            Self::BadToken => "missing or wrong bearer token",
        }
    }
}

impl IntoResponse for Denied {
    fn into_response(self) -> Response {
        match self {
            Self::ForeignHost | Self::BrowserOrigin => {
                problem(StatusCode::FORBIDDEN, self.reason())
            }
            Self::BadToken => {
                let mut response = problem(StatusCode::UNAUTHORIZED, self.reason());
                response
                    .headers_mut()
                    .insert(WWW_AUTHENTICATE, "Bearer".parse().expect("static header"));
                response
            }
        }
    }
}

// Logs at most one denial per second, to avoid log flooding.
fn log_denial(denied: &Denied) {
    static LAST_LOGGED: AtomicU64 = AtomicU64::new(0);
    let second = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    if LAST_LOGGED.swap(second, Ordering::Relaxed) != second {
        tracing::warn!(reason = denied.reason(), "HTTP request denied");
    }
}

// Only the literal address is accepted. A name like localhost may resolve
// elsewhere, where another process could collect tokens.
fn authorize(config: &HttpConfig, headers: &HeaderMap) -> Result<(), Denied> {
    let host = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if host != config.addr.to_string() {
        return Err(Denied::ForeignHost);
    }
    if headers.contains_key(ORIGIN) {
        return Err(Denied::BrowserOrigin);
    }
    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.as_bytes().strip_prefix(b"Bearer "))
        .ok_or(Denied::BadToken)?;
    if !config.token.matches(presented) {
        return Err(Denied::BadToken);
    }
    Ok(())
}

// RFC 9457 problem details.
fn problem(status: StatusCode, detail: &str) -> Response {
    let body = json!({
        "type": "about:blank",
        "title": status.canonical_reason().unwrap_or("Error"),
        "status": status.as_u16(),
        "detail": detail,
    });
    (
        status,
        [(CONTENT_TYPE, "application/problem+json")],
        body.to_string(),
    )
        .into_response()
}

fn error_response(error: &MemoryError) -> Response {
    let status = match error {
        MemoryError::Invalid(_) => StatusCode::BAD_REQUEST,
        MemoryError::NotFound(_) => StatusCode::NOT_FOUND,
        MemoryError::Conflict(_) => StatusCode::CONFLICT,
        MemoryError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    problem(status, &error.to_string())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn list_tools() -> Json<Value> {
    Json(tools::definitions())
}

async fn call_tool(
    State(brain): State<Arc<Brain>>,
    name: Result<Path<String>, PathRejection>,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let Some(tool) = name.ok().and_then(|Path(name)| Tool::parse(&name)) else {
        return problem(StatusCode::NOT_FOUND, "unknown tool");
    };
    let Json(arguments) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return problem(
                rejection.status(),
                "body must be a JSON object of tool arguments",
            );
        }
    };
    match tools::call(&brain, tool, arguments).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => error_response(&error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tools::tests::brain;
    use crate::config::Token;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn config(addr: &str) -> HttpConfig {
        HttpConfig {
            addr: addr.parse().unwrap(),
            token: Token::try_from(TOKEN.to_string()).unwrap(),
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                name.parse::<axum::http::HeaderName>().unwrap(),
                value.parse().unwrap(),
            );
        }
        headers
    }

    fn bearer() -> String {
        format!("Bearer {TOKEN}")
    }

    #[test]
    fn guard_accepts_the_expected_host_with_the_token() {
        let config = config("127.0.0.1:8082");
        let request = headers(&[("host", "127.0.0.1:8082"), ("authorization", &bearer())]);
        assert_eq!(authorize(&config, &request), Ok(()));
    }

    #[test]
    fn guard_rejects_foreign_hosts_even_with_the_token() {
        let config = config("127.0.0.1:8082");
        for host in [
            "evil.example:8082",
            "127.0.0.1",
            "127.0.0.1:80",
            "localhost",
            "localhost:8082",
            "",
        ] {
            let request = headers(&[("host", host), ("authorization", &bearer())]);
            assert_eq!(
                authorize(&config, &request),
                Err(Denied::ForeignHost),
                "host: {host}"
            );
        }
        assert_eq!(
            authorize(&config, &headers(&[("authorization", &bearer())])),
            Err(Denied::ForeignHost)
        );
    }

    #[test]
    fn guard_rejects_browser_origins() {
        let config = config("127.0.0.1:8082");
        for origin in ["https://evil.example", "http://127.0.0.1:8082", "null"] {
            let request = headers(&[
                ("host", "127.0.0.1:8082"),
                ("origin", origin),
                ("authorization", &bearer()),
            ]);
            assert_eq!(authorize(&config, &request), Err(Denied::BrowserOrigin));
        }
    }

    #[test]
    fn guard_rejects_missing_or_wrong_tokens() {
        let config = config("127.0.0.1:8082");
        let wrong = format!("Bearer {}", "x".repeat(TOKEN.len()));
        let prefix = format!("Bearer {}", &TOKEN[..TOKEN.len() - 1]);
        let basic = format!("Basic {TOKEN}");
        for authorization in [
            None,
            Some(wrong.as_str()),
            Some(prefix.as_str()),
            Some(basic.as_str()),
            Some(TOKEN),
            Some("Bearer "),
        ] {
            let mut pairs = vec![("host", "127.0.0.1:8082")];
            if let Some(value) = authorization {
                pairs.push(("authorization", value));
            }
            assert_eq!(
                authorize(&config, &headers(&pairs)),
                Err(Denied::BadToken),
                "{authorization:?}"
            );
        }
    }

    async fn request(addr: std::net::SocketAddr, head: &str, body: &str) -> (u16, Value) {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let message = format!(
            "{head}\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(message.as_bytes()).await.unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).await.unwrap();
        let status = raw.split(' ').nth(1).unwrap().parse().unwrap();
        let payload = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        (status, serde_json::from_str(payload).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn tools_are_reachable_over_http_only_with_the_token() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (brain, _dir) = brain();
        let app = router(Arc::new(brain), config(&addr.to_string()));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let auth = format!("Authorization: Bearer {TOKEN}\r\nContent-Type: application/json");

        let (status, body) = request(addr, "GET /health HTTP/1.1", "").await;
        assert_eq!((status, body["status"].as_u64()), (401, Some(401)));

        let (status, _) = request(addr, &format!("GET /health HTTP/1.1\r\n{auth}"), "").await;
        assert_eq!(status, 200);

        for head in [
            "GET /nowhere HTTP/1.1",
            "GET /v1/tools/note HTTP/1.1",
            "OPTIONS /v1/tools/note HTTP/1.1",
        ] {
            assert_eq!(request(addr, head, "").await.0, 401, "{head}");
        }
        let (status, _) = request(
            addr,
            &format!("GET /health HTTP/1.1\r\n{auth}\r\nOrigin: https://evil.example"),
            "",
        )
        .await;
        assert_eq!(status, 403);
        let (status, problem) =
            request(addr, &format!("GET /v1/tools/note HTTP/1.1\r\n{auth}"), "").await;
        assert_eq!((status, problem["status"].as_u64()), (405, Some(405)));
        let (status, problem) = request(
            addr,
            &format!("POST /v1/tools/%FF HTTP/1.1\r\n{auth}"),
            "{}",
        )
        .await;
        assert_eq!((status, problem["status"].as_u64()), (404, Some(404)));
        let no_content_type =
            format!("POST /v1/tools/note HTTP/1.1\r\nAuthorization: Bearer {TOKEN}");
        assert_eq!(
            request(addr, &no_content_type, r#"{"kind":"goal","title":"x"}"#)
                .await
                .0,
            415
        );

        let (status, note) = request(
            addr,
            &format!("POST /v1/tools/note HTTP/1.1\r\n{auth}"),
            r#"{"kind":"goal","title":"Ship"}"#,
        )
        .await;
        assert_eq!((status, note["title"].as_str()), (200, Some("Ship")));

        let (status, problem) = request(
            addr,
            &format!("POST /v1/tools/get_note HTTP/1.1\r\n{auth}"),
            r#"{"id":999}"#,
        )
        .await;
        assert_eq!(
            (status, problem["detail"].as_str()),
            (404, Some("note 999 not found"))
        );

        let (status, _) = request(
            addr,
            &format!("POST /v1/tools/note HTTP/1.1\r\n{auth}"),
            r#"{"kind":"goal"}"#,
        )
        .await;
        assert_eq!(status, 400);
        let (status, _) = request(
            addr,
            &format!("POST /v1/tools/rm_rf HTTP/1.1\r\n{auth}"),
            "{}",
        )
        .await;
        assert_eq!(status, 404);
        let (status, _) = request(
            addr,
            &format!("POST /v1/tools/note HTTP/1.1\r\n{auth}"),
            "{broken",
        )
        .await;
        assert_eq!(status, 400);
        let (status, _) = request(addr, &format!("GET /nowhere HTTP/1.1\r\n{auth}"), "").await;
        assert_eq!(status, 404);

        let oversized = format!(
            r#"{{"kind":"fact","title":"t","body":"{}"}}"#,
            "x".repeat(MAX_BODY_BYTES)
        );
        let (status, _) = request(
            addr,
            &format!("POST /v1/tools/note HTTP/1.1\r\n{auth}"),
            &oversized,
        )
        .await;
        assert_eq!(status, 413);
    }
}
