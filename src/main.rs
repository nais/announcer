#[macro_use]
extern crate lazy_static;
extern crate redis;

use crate::redis::Commands;
use axum::{routing::post, Router};
use log::{error, info};
use md5;
use redis::RedisResult;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::{Error, ErrorKind};

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
    println!("Hello, world!");

    let app = Router::new().route("/reconcile", post(reconcile));

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

    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let mut con = client.get_connection().unwrap(); // TODO: Denne kan feile!

    for item in doc.channel.items {
        let key = item.link.split("#").collect::<Vec<&str>>()[1].to_owned();
        info!("Handling {} ({}) - {}", item.title, item.pub_date, key);

        let hash = format!("{:x}", md5::compute(&item.content));

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
                println!("Hash: {} ({})", output.hash, output.timestamp);
                if output.hash != hash {
                    info!("Post has changed, updating the Slack message");
                    match update_message(item, &output.timestamp).await {
                        Ok(_) => {
                            output.hash = hash;
                            let raw = serde_json::to_string(&output).unwrap();
                            let output: RedisResult<()> = con.set(key, raw);

                            match output {
                                Ok(_) => {
                                    info!("Update Slack, and Redis");
                                }
                                Err(err) => error!("Failed saving to Redis: {}", err),
                            }
                        }
                        Err(err) => error!("Failed posting to Slack: {}", err),
                    };
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
    println!("Posting to Slack");
    let content = format_slack_post(post.content);
    let payload = SlackMessage {
        channel: "#test-rss-announcement".to_string(),
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
    println!("Updating message in Slack");
    let content = format_slack_post(post.content);
    let payload = SlackMessage {
        channel: "C082AH36ZTL".to_string(),
        ts: timestamp.to_string(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.update".to_string(), payload).await
}

async fn post_to_slack(method: String, payload: SlackMessage) -> Result<SlackResponse, Error> {
    let slack_token = std::env::var("SLACK_TOKEN").unwrap();

    match reqwest::Client::new()
        .post(format!("https://slack.com/api/{}", method))
        .header("Authorization", format!("Bearer {}", slack_token))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            match resp.json::<SlackResponse>().await {
                Ok(sr) => {
                    if sr.ok {
                        return Ok(sr);
                    } else {
                        return Err(Error::new(ErrorKind::Other, sr.error));
                    }
                }
                Err(err) => return Err(Error::new(ErrorKind::Other, err.to_string())),
            };
        }
        Err(err) => return Err(Error::new(ErrorKind::Other, err.to_string())),
    };
}
