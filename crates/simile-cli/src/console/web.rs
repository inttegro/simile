use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::ConsoleSession;
use crate::error::Result;
use crate::terminal::Terminal;

const INDEX: &str = include_str!("../../../../web/index.html");
const STYLES: &str = include_str!("../../../../web/styles.css");
const APP: &str = include_str!("../../../../web/app.js");

#[derive(Clone)]
struct WebState {
    session: Arc<Mutex<ConsoleSession>>,
}

pub(crate) async fn run(session: ConsoleSession, terminal: &Terminal) -> Result<()> {
    let state = WebState {
        session: Arc::new(Mutex::new(session)),
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/styles.css", get(styles))
        .route("/app.js", get(app_script))
        .route("/service-data.js", get(service_data))
        .route("/api/catalog", get(catalog))
        .fallback(index)
        .with_state(state.clone());
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .map_err(|source| simile_server::Error::Bind {
            address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            source,
        })?;
    let address = listener
        .local_addr()
        .map_err(simile_server::Error::LocalAddress)?;
    let url = format!("http://{address}/");
    terminal.ui().banner()?;
    terminal.ui().note("console", &url)?;
    if let Err(error) = webbrowser::open(&url) {
        tracing::warn!(%error, %url, "could not open the browser; open the console URL manually");
    }

    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::warn!(%error, "could not install Ctrl-C handler for the web console");
            }
        })
        .await;
    state.session.lock().await.close().await;
    served.map_err(simile_server::Error::Serve)?;
    Ok(())
}

async fn index(
    State(state): State<WebState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if let Some(service) = query.get("service") {
        if let Err(error) = state.session.lock().await.select(service).await {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                error.to_string(),
            )
                .into_response();
        }
    }
    Html(INDEX).into_response()
}

async fn styles() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLES)
}

async fn app_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        APP,
    )
}

async fn service_data(State(state): State<WebState>) -> Response {
    let session = state.session.lock().await;
    let services = match serde_json::to_string(&session.catalog().services) {
        Ok(services) => services,
        Err(error) => return json_error(error),
    };
    let selected = match serde_json::to_string(session.selected()) {
        Ok(selected) => selected,
        Err(error) => return json_error(error),
    };
    let runtime = match serde_json::to_string(&format!("http://{}", session.runtime_address())) {
        Ok(runtime) => runtime,
        Err(error) => return json_error(error),
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        format!(
            "window.SIMILE_CONSOLE_SERVICES = Object.freeze({services});\nwindow.SIMILE_CONSOLE_RUNTIME = {runtime};\nwindow.SIMILE_SERVICE_DATA = Object.freeze({selected});\n"
        ),
    )
        .into_response()
}

async fn catalog(State(state): State<WebState>) -> Response {
    let session = state.session.lock().await;
    match serde_json::to_vec_pretty(session.catalog()) {
        Ok(body) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
            body,
        )
            .into_response(),
        Err(error) => json_error(error),
    }
}

fn json_error(error: serde_json::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        format!(
            r#"{{"error":{{"code":"console_serialization_failed","message":{}}}}}"#,
            serde_json::Value::String(error.to_string())
        ),
    )
        .into_response()
}
