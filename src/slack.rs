use crate::rss::Post;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::{Error, ErrorKind};

#[derive(Serialize)]
struct Message {
    channel: String,
    ts: String,
    text: String,
}

#[derive(Deserialize)]
pub struct Response {
    ok: bool,
    #[serde(default)]
    pub ts: String,
    #[serde(default)]
    error: String,
}

fn format_slack_post(org: String) -> String {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\[(.*?)\]\((.*?)\)").unwrap();
    }

    RE.replace_all(&org, "<$2|$1>").to_string()
}

pub async fn post_message(post: Post) -> Result<Response, Error> {
    let content = format_slack_post(post.content);
    let payload = Message {
        channel: std::env::var("SLACK_CHANNEL_ID").unwrap(),
        ts: "".to_string(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.postMessage".to_string(), payload).await
}

pub async fn update_message(post: Post, timestamp: &String) -> Result<Response, Error> {
    let content = format_slack_post(post.content);
    let payload = Message {
        channel: std::env::var("SLACK_CHANNEL_ID").unwrap(),
        ts: timestamp.to_string(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.update".to_string(), payload).await
}

async fn post_to_slack(method: String, payload: Message) -> Result<Response, Error> {
    let slack_token = std::env::var("SLACK_TOKEN").unwrap();

    let response = reqwest::Client::new()
        .post(format!("https://slack.com/api/{}", method))
        .header("Authorization", format!("Bearer {}", slack_token))
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&payload)
        .send()
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?
        .json::<Response>()
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

    if response.ok {
        Ok(response)
    } else {
        Err(Error::new(ErrorKind::Other, response.error))
    }
}
