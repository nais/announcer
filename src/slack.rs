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
    pub fn new(config: SlackConfig, client: reqwest::Client) -> Self {
        Self { config, client }
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

#[cfg(test)]
mod tests {
    use super::format_slack_post;

    #[test]
    fn formats_single_markdown_link() {
        let input = "See [NAIS](https://nais.io) for more info";
        let expected = "See <https://nais.io|NAIS> for more info";
        assert_eq!(format_slack_post(input), expected);
    }

    #[test]
    fn formats_multiple_markdown_links() {
        let input = "[One](https://one.example) and [Two](https://two.example)";
        let expected = "<https://one.example|One> and <https://two.example|Two>";
        assert_eq!(format_slack_post(input), expected);
    }

    #[test]
    fn leaves_text_without_links_unchanged() {
        let input = "No links here, just text.";
        assert_eq!(format_slack_post(input), input);
    }
}
