use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::{Error, ErrorKind};

use crate::rss;

#[derive(Serialize)]
struct SlackMessage {
    channel: String,
    ts: String,
    text: String,
}

#[derive(Deserialize)]
pub struct SlackResponse {
    ok: bool,
    #[serde(default)]
    pub ts: String,
    #[serde(default)]
    error: String,
}

#[derive(Deserialize, Serialize)]
pub struct SlackBlob {
    pub hash: String,
    pub timestamp: String,
}

fn format_slack_post(org: String) -> String {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\[(.*?)\]\((.*?)\)").unwrap();
    }

    RE.replace_all(&org, "<$2|$1>").to_string()
}

pub async fn post_message(post: Item) -> Result<SlackResponse, Error> {
    let content = format_slack_post(post.content);
    let payload = SlackMessage {
        channel: std::env::var("SLACK_CHANNEL_ID").unwrap(),
        ts: "".to_string(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.postMessage".to_string(), payload).await
}

pub async fn update_message(post: Item, timestamp: &String) -> Result<SlackResponse, Error> {
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
