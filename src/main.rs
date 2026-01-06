extern crate redis;

mod config;
mod redis_client;
mod rss;
mod slack;

use axum::{
    extract::State,
    http,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use color_eyre::eyre;
use log::{error, info};
use structured_logger::{async_json::new_writer, Builder};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let app_config = config::AppConfig::from_env()?;

    Builder::with_level("info")
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    info!("Good morning, Nais!");

    if app_config.mode.is_dry_run() {
        info!("Running in DRY_RUN mode: Slack and Redis are disabled");
    }

    let app = Router::new()
        .route("/reconcile", post(reconcile))
        .route(
            "/",
            get(|| async { "Hello, check out https://nais.io/log/!" }),
        )
        .with_state(app_config);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await.map_err(eyre::Error::msg)
}

#[axum::debug_handler]
async fn reconcile(State(app_config): State<config::AppConfig>) -> Response {
    info!("Time to check the log");
    match reqwest::get("https://nais.io/log/rss.xml").await {
        Ok(resp) => {
            if !resp.status().is_success() {
                error!("Got a response, but no XML");
                return (
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    format!(
                        "https://nais.io/log/rss.xml answers with: {}",
                        resp.status()
                    ),
                )
                    .into_response();
            }
            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    error!("Unable to parse nais.io/log's rss: {e}");
                    return (
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Unable to decode nais log",
                    )
                        .into_response();
                }
            };
            rss::handle_feed(&body, &app_config).await;
        }
        Err(e) => {
            error!("Failed getting the feed: {e}");
            return (http::StatusCode::INTERNAL_SERVER_ERROR, "HTTP client error").into_response();
        }
    };
    (http::StatusCode::OK, "").into_response()
}
