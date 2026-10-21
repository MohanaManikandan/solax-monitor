use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::clock;
use crate::config::Config;
use crate::db::Store;
use crate::metrics::{fault_code_label, run_mode_label, Registry};

const COOKIE_NAME: &str = "solax_token";

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub registry: Arc<Registry>,
    pub store: Store,
    pub updates: broadcast::Sender<Arc<solax_api::LiveEvent>>,
    pub curves: crate::projection::CurveCache,
}

/// The built screens. Missing until the UI has been built once, in which
/// case the page answers 404 and the API still works.
#[derive(rust_embed::Embed)]
#[folder = "../ui/dist/"]
#[allow_missing = true]
struct Assets;

pub fn router(state: AppState) -> Router {
    // The page and its assets. The API lives under /api/v1.
    let page = Router::new()
        .route("/", get(index))
        .route("/assets/{*path}", get(asset))
        .layer(tower_http::compression::CompressionLayer::new());

    let protected = page
        .nest("/api/v1", crate::api_v1::routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_token,
        ));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(protected)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

/// Token gate. A browser can present the token three ways, and all three
/// matter: a header for scripted calls, a query parameter for the first visit
/// from a phone, and a cookie afterwards.
///
/// The cookie branch is load-bearing rather than a convenience: EventSource
/// cannot set request headers, so /api/live can only ever authenticate with
/// the cookie that the query parameter plants.
async fn require_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let expected = state.config.server.token.as_str();

    if let Some(header) = request.headers().get("x-auth-token") {
        if header.as_bytes() == expected.as_bytes() {
            return next.run(request).await;
        }
    }

    if let Some(token) = query_token(request.uri().query().unwrap_or_default()) {
        if token == expected {
            // Plant the cookie and bounce to the clean URL so the token stops
            // travelling in links, history and logs. Everything else in the
            // query survives: dropping it would turn a shared
            // /api/export?date=...&token=... into an export of today.
            let rest = strip_token(request.uri().query().unwrap_or_default());
            let location = if rest.is_empty() {
                request.uri().path().to_string()
            } else {
                format!("{}?{}", request.uri().path(), rest)
            };

            return Response::builder()
                .status(StatusCode::SEE_OTHER)
                .header(header::LOCATION, location)
                .header(
                    header::SET_COOKIE,
                    format!(
                        "{COOKIE_NAME}={token}; Path=/; Max-Age=31536000; HttpOnly; SameSite=Lax"
                    ),
                )
                .body(Body::empty())
                // Config validation rejects tokens that cannot sit in a
                // header, so this is unreachable. Answering rather than
                // panicking keeps a bad build from taking the task down.
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "bad redirect").into_response()
                });
        }
    }

    if cookie_token(request.headers()).as_deref() == Some(expected) {
        return next.run(request).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        "This monitor needs a token. Open it as http://host:8080/?token=YOUR_TOKEN once \
         and the browser remembers it.",
    )
        .into_response()
}

fn query_token(query: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == "token").then(|| percent_decode(value))
    })
}

/// The query string with the token pair removed, order otherwise intact.
fn strip_token(query: &str) -> String {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| pair.split_once('=').map(|(name, _)| name) != Some("token"))
        .collect::<Vec<_>>()
        .join("&")
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == COOKIE_NAME).then(|| value.to_string())
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn index() -> Response {
    serve_asset("index.html")
}

async fn asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    serve_asset(&format!("assets/{path}"))
}

fn serve_asset(path: &str) -> Response {
    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// The codes listed in a stored Error object, in the order the dongle gave
/// them. A malformed or unexpected object yields none rather than failing.
pub(crate) fn error_codes(raw: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    parsed
        .get("ErrorCode")
        .and_then(|value| value.as_str())
        .map(|list| {
            list.split(',')
                .map(|code| code.trim().to_string())
                .filter(|code| !code.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub(crate) struct RangeQuery {
    from: Option<i64>,
    to: Option<i64>,
    date: Option<String>,
}

impl RangeQuery {
    /// Defaults to the whole of today in the configured zone, which is what
    /// makes the chart a day rather than a rolling window.
    fn resolve(&self, tz: Tz) -> (i64, i64) {
        if let (Some(from), Some(to)) = (self.from, self.to) {
            return (from, to);
        }
        let date = self
            .date
            .as_deref()
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
            .unwrap_or_else(|| clock::today(tz));
        let (start, end) = clock::day_bounds(tz, date);
        (self.from.unwrap_or(start), self.to.unwrap_or(end))
    }
}

/// Quote a CSV field when it carries a comma, a quote or a newline. A fault
/// row lists several codes in one cell, so this is load bearing rather than
/// precautionary.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub(crate) async fn export_handler(
    State(state): State<AppState>,
    Query(range): Query<RangeQuery>,
) -> Result<Response, ApiError> {
    let (from, to) = range.resolve(state.config.storage.timezone);
    let snapshots = state.store.history(from, to).await?;

    let mut out = String::from("timestamp");
    for metric in state.registry.metrics() {
        out.push(',');
        out.push_str(&metric.key);
    }
    // Faults sit outside Data, so they need columns of their own. Both the
    // numbers and their names go out: the numbers survive a rename here, the
    // names are what a reader can act on.
    out.push_str(",error_codes,error_names\n");

    for snapshot in &snapshots {
        let moment = chrono::DateTime::from_timestamp(snapshot.ts, 0)
            .map(|utc| utc.with_timezone(&state.config.storage.timezone))
            .map(|local| local.format("%Y-%m-%d %H:%M:%S %Z").to_string())
            .unwrap_or_default();
        out.push_str(&csv_field(&moment));
        for metric in state.registry.metrics() {
            out.push(',');
            if let Some(&raw) = snapshot.raw.get(&metric.key) {
                if state.registry.is_run_mode(metric) {
                    out.push_str(&csv_field(&run_mode_label(raw)));
                } else {
                    out.push_str(&format!(
                        "{:.*}",
                        metric.decimals as usize,
                        metric.scaled(raw)
                    ));
                }
            }
        }

        let codes = snapshot
            .error
            .as_deref()
            .map(error_codes)
            .unwrap_or_default();
        let names: Vec<String> = codes.iter().map(|code| fault_code_label(code)).collect();
        out.push(',');
        out.push_str(&csv_field(&codes.join(",")));
        out.push(',');
        out.push_str(&csv_field(&names.join(", ")));
        out.push('\n');
    }

    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"solax-{from}-{to}.csv\""),
            ),
        ],
        out,
    )
        .into_response())
}

pub enum ApiError {
    /// The request asked for something malformed; the message says what.
    BadRequest(String),
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self::Internal(error.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            Self::Internal(error) => {
                tracing::error!(error = %error, "request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
        }
    }
}
