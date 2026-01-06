use crate::{config::SlackConfig, rss::Post};
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    io::{Error, ErrorKind},
    sync::OnceLock,
};
use tracing::info;

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

#[async_trait]
pub trait SlackClient: Send + Sync {
    async fn post_message(&self, post: &Post) -> Result<Response, Error>;
    async fn update_message(&self, post: &Post, timestamp: &str) -> Result<Response, Error>;
}

#[derive(Debug, Clone)]
pub struct HttpSlackClient {
    config: SlackConfig,
    client: reqwest::Client,
}

impl HttpSlackClient {
    pub fn new(config: SlackConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    async fn send(&self, method: &str, payload: &Message) -> Result<Response, Error> {
        let slack_token = &self.config.token;

        let response = self
            .client
            .post(format!("https://slack.com/api/{method}"))
            .header("Authorization", format!("Bearer {slack_token}"))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(payload)
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
}

#[async_trait]
impl SlackClient for HttpSlackClient {
    async fn post_message(&self, post: &Post) -> Result<Response, Error> {
        let content = format_slack_post(&post.content);
        let payload = Message {
            channel: self.config.channel_id.clone(),
            ts: String::new(),
            text: format!("<{}|{}>\n{}", post.link, post.title, content),
        };

        self.send("chat.postMessage", &payload).await
    }

    async fn update_message(&self, post: &Post, timestamp: &str) -> Result<Response, Error> {
        let content = format_slack_post(&post.content);
        let payload = Message {
            channel: self.config.channel_id.clone(),
            ts: timestamp.to_string(),
            text: format!("<{}|{}>\n{}", post.link, post.title, content),
        };

        self.send("chat.update", &payload).await
    }
}

#[derive(Debug, Clone, Default)]
pub struct StdoutSlackClient;

#[async_trait]
impl SlackClient for StdoutSlackClient {
    async fn post_message(&self, post: &Post) -> Result<Response, Error> {
        let content = format_slack_post(&post.content);
        let text = format!("<{}|{}>\n{}", post.link, post.title, content);
        info!("DRY_RUN Slack post:\n{text}");

        Ok(Response {
            ok: true,
            ts: "dry-run".to_string(),
            error: String::new(),
        })
    }

    async fn update_message(&self, post: &Post, timestamp: &str) -> Result<Response, Error> {
        let content = format_slack_post(&post.content);
        let text = format!("<{}|{}>\n{}", post.link, post.title, content);
        info!("DRY_RUN Slack update (ts={timestamp}):\n{text}");

        Ok(Response {
            ok: true,
            ts: timestamp.to_string(),
            error: String::new(),
        })
    }
}
