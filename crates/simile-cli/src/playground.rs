use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::response::Response;
use include_dir::{Dir, include_dir};
use tokio::net::TcpListener;

use crate::cli::PlaygroundArgs;
use crate::error::Result;
use crate::terminal::Terminal;

static PLAYGROUND: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../playground/dist");

pub(crate) async fn run(arguments: PlaygroundArgs, terminal: &Terminal) -> Result<()> {
    let requested = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), arguments.port);
    let listener =
        TcpListener::bind(requested)
            .await
            .map_err(|source| simile_server::Error::Bind {
                address: requested,
                source,
            })?;
    let address = listener
        .local_addr()
        .map_err(simile_server::Error::LocalAddress)?;
    let url = format!("http://{address}/");

    terminal.ui().banner()?;
    terminal.ui().note("playground", &url)?;
    if !arguments.no_open {
        if let Err(error) = webbrowser::open(&url) {
            tracing::warn!(%error, %url, "could not open the browser; open the playground URL manually");
        }
    }

    axum::serve(listener, Router::new().fallback(serve_asset))
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::warn!(%error, "could not install Ctrl-C handler for the playground");
            }
        })
        .await
        .map_err(simile_server::Error::Serve)?;
    Ok(())
}

async fn serve_asset(request: Request) -> Response {
    let requested = request.uri().path().trim_start_matches('/');
    let path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let Some(file) = PLAYGROUND.get_file(path) else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from("playground asset not found"))
            .unwrap_or_else(|_| Response::new(Body::empty()));
    };

    let cache_control = if path == "index.html" {
        "no-cache"
    } else if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type(path))
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(file.contents()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{PLAYGROUND, content_type};

    #[test]
    fn embeds_the_playground_entrypoint() {
        let index = PLAYGROUND
            .get_file("index.html")
            .unwrap_or_else(|| panic!("playground index should be embedded"));
        assert!(index.contents().starts_with(b"<!doctype html>"));
    }

    #[test]
    fn serves_webassembly_with_its_streaming_content_type() {
        assert_eq!(content_type("engine.wasm"), "application/wasm");
    }
}
