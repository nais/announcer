#[macro_use]
extern crate lazy_static;
extern crate redis;

use crate::redis::Commands;
use axum::{
    routing::{get, post},
    Router,
};
use log::{error, info};
use md5;
use redis::RedisResult;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::{Error, ErrorKind};
use structured_logger::{async_json::new_writer, Builder};

#[derive(Debug, Deserialize)]
struct Item {
    title: String,
    link: String,
    #[serde(rename = "pubDate")]
    pub_date: String,
    #[serde(rename = "encoded")]
    content: String,
}

#[derive(Debug, Deserialize)]
struct Channel {
    title: String,
    #[serde(rename = "item")]
    items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct RSS {
    channel: Channel,
}

#[derive(Deserialize, Serialize)]
struct SlackBlob {
    hash: String,
    timestamp: String,
}

#[tokio::main]
async fn main() {
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

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn reconcile() {
    info!("Time to check the log");
    match reqwest::get("https://nais.io/log/rss.xml").await {
        Ok(resp) => {
            if resp.status().is_success() {
                let body = resp.text().await;
                handle_feed(&body.unwrap()).await;
            } else {
                error!("Got a response, but no XML");
            }
        }
        Err(e) => error!("Failed getting the feed: {e}"),
    }
}

async fn handle_feed(xml: &str) {
    let doc: RSS = quick_xml::de::from_str(xml).unwrap();
    info!(
        "Found {} posts in {}",
        doc.channel.items.len(),
        doc.channel.title
    );

    let uri: String;

    if std::env::var("NAIS_CLUSTER_NAME").is_ok() {
        let host = std::env::var("REDIS_HOST_RSS").unwrap();
        let username = std::env::var("REDIS_USERNAME_RSS").unwrap();
        let password = std::env::var("REDIS_PASSWORD_RSS").unwrap();
        let port = std::env::var("REDIS_PORT_RSS").unwrap();
        uri = format!("rediss://{username}:{password}@{host}:{port}");
    } else {
        uri = "redis://localhost:6379".to_string();
    }

    let client = match redis::Client::open(uri) {
        Ok(c) => c,
        Err(err) => {
            error!("Connecting to Redis failed: {}", err);
            return;
        }
    };

    let mut con = match client.get_connection() {
        Ok(c) => c,
        Err(err) => {
            error!("Opening connection failed: {}", err);
            return;
        }
    };

    for item in doc.channel.items {
        let key = item.link.split("#").collect::<Vec<&str>>()[1].to_owned();
        info!(
            "Handling '{}' (date: {}, key: {})",
            item.title, item.pub_date, key
        );

        let hash = format!(
            "{:x}",
            md5::compute(format!("{}-{}", item.title, item.content))
        );

        match con.get::<_, Option<String>>(&key) {
            Ok(None) => {
                info!("New post, pushing to Slack");
                match post_message(item).await {
                    Ok(sr) => {
                        let sb = SlackBlob {
                            hash,
                            timestamp: sr.ts,
                        };
                        let raw = serde_json::to_string(&sb).unwrap();
                        let output: RedisResult<()> = con.set(key, raw);

                        match output {
                            Ok(_) => info!("Posted to Slack, and saved to Redis"),
                            Err(err) => error!("Failed saving to Redis: {}", err),
                        }
                    }
                    Err(err) => error!("Failed posting to Slack: {}", err),
                };
            }
            Ok(Some(raw)) => {
                let mut output = serde_json::from_str::<SlackBlob>(&raw).unwrap();
                if output.hash != hash {
                    info!("Post has changed, updating Slack");
                    match update_message(item, &output.timestamp).await {
                        Ok(_) => {
                            output.hash = hash;
                            let raw = serde_json::to_string(&output).unwrap();
                            let output: RedisResult<()> = con.set(key, raw);

                            match output {
                                Ok(_) => {
                                    info!("Finished updating Slack, and Redis");
                                }
                                Err(err) => error!("Failed saving to Redis: {}", err),
                            }
                        }
                        Err(err) => error!("Failed posting to Slack: {}", err),
                    };
                } else {
                    info!("No changes here");
                }
            }
            Err(err) => error!("Failed getting {} from Redis: {}", key, err),
        }
    }
}

#[derive(Serialize)]
struct SlackMessage {
    channel: String,
    ts: String,
    text: String,
}

#[derive(Deserialize)]
struct SlackResponse {
    ok: bool,
    #[serde(default)]
    ts: String,
    #[serde(default)]
    error: String,
}

async fn post_message(post: Item) -> Result<SlackResponse, Error> {
    let content = format_slack_post(post.content);
    let payload = SlackMessage {
        channel: std::env::var("SLACK_CHANNEL_ID").unwrap(),
        ts: "".to_string(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.postMessage".to_string(), payload).await
}

fn format_slack_post(org: String) -> String {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\[(.*)\]\((.*)\)").unwrap();
    }

    RE.replace_all(&org, "<$2|$1>").to_string()
}

async fn update_message(post: Item, timestamp: &String) -> Result<SlackResponse, Error> {
    let content = format_slack_post(post.content);
    let payload = SlackMessage {
        channel: std::env::var("SLACK_CHANNEL_ID").unwrap(),
        ts: timestamp.to_string(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.update".to_string(), payload).await
}

async fn post_to_slack(method: String, payload: SlackMessage) -> Result<SlackResponse, Error> {
    let slack_token = std::env::var("SLACK_TOKEN").unwrap();

    let response = reqwest::Client::new()
        .post(format!("https://slack.com/api/{}", method))
        .header("Authorization", format!("Bearer {}", slack_token))
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&payload)
        .send()
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?
        .json::<SlackResponse>()
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

    if response.ok {
        Ok(response)
    } else {
        Err(Error::new(ErrorKind::Other, response.error))
    }
}
