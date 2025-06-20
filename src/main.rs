extern crate redis;

mod rss;
mod slack;

use axum::{
    Router, http,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use color_eyre::eyre;
use log::{error, info};
use structured_logger::{Builder, async_json::new_writer};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    Builder::with_level("info")
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    info!("Good morning, Nais!");

    std::env::var("SLACK_TOKEN").expect("Missing SLACK_TOKEN env");
    std::env::var("SLACK_CHANNEL_ID").expect("Missing SLACK_CHANNEL_ID env");

    if std::env::var("NAIS_CLUSTER_NAME").is_ok() {
        std::env::var("REDIS_HOST_RSS").expect("Missing REDIS_HOST_RSS env");
        std::env::var("REDIS_USERNAME_RSS").expect("Missing REDIS_USERNAME_RSS env");
        std::env::var("REDIS_PASSWORD_RSS").expect("Missing REDIS_PASSWORD_RSS env");
        std::env::var("REDIS_PORT_RSS").expect("Missing REDIS_PORT_RSS env");
    }

    let app = Router::new().route("/reconcile", post(reconcile)).route(
        "/",
        get(|| async { "Hello, check out https://nais.io/log/!" }),
    );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await.map_err(eyre::Error::msg)
}

#[axum::debug_handler]
async fn reconcile() -> Response {
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
            rss::handle_feed(&body).await;
        }
        Err(e) => {
            error!("Failed getting the feed: {e}");
            return (http::StatusCode::INTERNAL_SERVER_ERROR, "HTTP client error").into_response();
        }
    };
    (http::StatusCode::OK, "").into_response()
}
