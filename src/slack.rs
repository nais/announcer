use crate::rss::Post;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    io::{Error, ErrorKind},
    sync::OnceLock,
};

#[derive(Debug, Serialize)]
struct Message {
    channel: String,
    ts: String,
    text: String,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    ok: bool,
    #[serde(default)]
    pub ts: String,
    #[serde(default)]
    error: String,
}

static RE_PATTERN: OnceLock<Regex> = OnceLock::new();

pub(crate) fn format_slack_post(org: &str) -> String {
    RE_PATTERN
        .get_or_init(|| {
            Regex::new(r"\[(.*?)\]\((.*?)\)").expect("Hard-coded regex pattern should compile")
        })
        .replace_all(org, "<$2|$1>")
        .to_string()
}

pub async fn post_message(
    post: &Post,
    slack_config: &crate::config::SlackConfig,
) -> Result<Response, Error> {
    let content = format_slack_post(&post.content);
    let payload = Message {
        channel: slack_config.channel_id.clone(),
        ts: String::new(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.postMessage", payload, slack_config).await
}

pub async fn update_message(
    post: &Post,
    timestamp: &str,
    slack_config: &crate::config::SlackConfig,
) -> Result<Response, Error> {
    let content = format_slack_post(&post.content);
    let payload = Message {
        channel: slack_config.channel_id.clone(),
        ts: timestamp.to_string(),
        text: format!("<{}|{}>\n{}", post.link, post.title, content),
    };

    post_to_slack("chat.update", payload, slack_config).await
}

async fn post_to_slack(
    method: &str,
    payload: Message,
    slack_config: &crate::config::SlackConfig,
) -> Result<Response, Error> {
    let slack_token = &slack_config.token;

    let response = reqwest::Client::new()
        .post(format!("https://slack.com/api/{method}"))
        .header("Authorization", format!("Bearer {slack_token}"))
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
