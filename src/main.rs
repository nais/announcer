extern crate redis;

mod config;
mod redis_client;
mod rss;
mod slack;

use axum::{
    Router,
    extract::State,
    http,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use color_eyre::eyre;
use rss::FeedError;
use tracing::{error, info, instrument};
use tracing_subscriber::{EnvFilter, fmt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let app_config = config::AppConfig::from_env()?;

    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .finish()
        .init();

    let state = config::AppState::new(app_config);

    info!("Good morning, Nais!");

    if state.config.is_dry_run() {
        info!("Running in DRY_RUN mode: Slack and Redis are disabled");
    }

    let app = Router::new()
        .route("/reconcile", post(reconcile))
        .route("/internal/health", get(healthz))
        .route("/internal/ready", get(ready))
        .route(
            "/",
            get(|| async { "Hello, check out https://nais.io/log/!" }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await.map_err(eyre::Error::msg)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn ready() -> &'static str {
    "ok"
}

#[axum::debug_handler]
#[instrument(skip(state))]
async fn reconcile(State(state): State<config::AppState>) -> Response {
    info!(
        mode = %if state.config.is_dry_run() { "DryRun" } else { "Normal" },
        "Time to check the log"
    );
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
            if let Err(e) = rss::handle_feed(&body, &state).await {
                match e {
                    FeedError::RssParse(err) => {
                        error!("Failed to parse RSS feed: {err}");
                        return (
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to parse RSS feed",
                        )
                            .into_response();
                    }
                    FeedError::InvalidArchive { key, error } => {
                        error!("Invalid archive JSON for key {key}: {error}");
                        return (
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            "Corrupted archive data in Redis",
                        )
                            .into_response();
                    }
                    FeedError::SerializeArchive { key, error } => {
                        error!("Failed to serialize archive for key {key}: {error}");
                        return (
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to persist archive data",
                        )
                            .into_response();
                    }
                }
            }
        }
        Err(e) => {
            error!("Failed getting the feed: {e}");
            return (http::StatusCode::INTERNAL_SERVER_ERROR, "HTTP client error").into_response();
        }
    };
    (http::StatusCode::OK, "").into_response()
}
