use crate::{
    config,
    redis_client::{InMemoryValkey, ValkeyClient, ValkeyStore},
    slack::{self, HttpSlackClient, SlackClient, StdoutSlackClient},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, instrument};

#[derive(Debug)]
pub enum FeedError {
    RssParse(String),
    InvalidArchive { key: String, error: String },
    SerializeArchive { key: String, error: String },
}

#[derive(Debug, Deserialize)]
pub struct Post {
    pub title: String,
    pub link: String,
    #[serde(rename = "pubDate")]
    pub_date: String,
    #[serde(rename = "encoded")]
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct Feed {
    title: String,
    #[serde(rename = "item")]
    posts: Vec<Post>,
}

#[derive(Debug, Deserialize)]
struct Rss {
    channel: Feed,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Archive {
    pub hash: String,
    pub timestamp: String,
}

#[instrument(skip(xml, app_state))]
pub async fn handle_feed(xml: &str, app_state: &config::AppState) -> Result<(), FeedError> {
    let doc: Rss = quick_xml::de::from_str(xml).map_err(|e| FeedError::RssParse(e.to_string()))?;
    info!(
        "Found {} posts in {}",
        doc.channel.posts.len(),
        doc.channel.title
    );

    let mut redis_client: Option<Box<dyn ValkeyClient>> = if app_state.config.is_dry_run() {
        info!("DRY_RUN is set, using in-memory Valkey");
        Some(Box::new(InMemoryValkey::new()))
    } else if let Some(redis_cfg) = app_state.config.valkey_config() {
        ValkeyStore::connect(redis_cfg).map(|store| Box::new(store) as Box<dyn ValkeyClient>)
    } else {
        info!("No Valkey configuration available, skipping Valkey connectivity and persistence");
        None
    };

    let slack_client: Box<dyn SlackClient> = if app_state.config.is_dry_run() {
        Box::new(StdoutSlackClient::default())
    } else {
        match app_state.config.slack_config() {
            Ok(cfg) => Box::new(HttpSlackClient::new(
                cfg.clone(),
                app_state.http_client.clone(),
            )),
            Err(e) => {
                error!("Slack configuration missing when trying to post: {e}");
                Box::new(StdoutSlackClient::default())
            }
        }
    };

    for item in doc.channel.posts {
        let key = item
            .link
            .split('#')
            .collect::<Vec<&str>>()
            .get(1)
            .copied()
            .unwrap_or(&item.link);
        info!(
            post_key = %key,
            title = %item.title,
            pub_date = %item.pub_date,
            "Handling post"
        );

        let hashed_post = format!(
            "{:x}",
            md5::compute(format!("{}-{}", item.title, item.content))
        );

        if let Some(store) = &mut redis_client {
            match store.get(&key).await {
                Ok(None) => {
                    info!(post_key = %key, "New post, pushing to Slack");
                    match slack_client.post_message(&item).await {
                        Ok(response) => {
                            let archive = Archive {
                                hash: hashed_post,
                                timestamp: response.ts,
                            };
                            let raw = serde_json::to_string(&archive).unwrap();
                            match store.set(&key, &raw).await {
                                Ok(()) => {
                                    info!(post_key = %key, "Posted to Slack, and saved to Redis")
                                }
                                Err(err) => {
                                    error!(post_key = %key, error = %err, "Failed saving to Redis")
                                }
                            }
                        }
                        Err(err) => {
                            error!(post_key = %key, error = %err, "Failed posting to Slack")
                        }
                    };
                }
                Ok(Some(raw)) => {
                    let mut archive = serde_json::from_str::<Archive>(&raw).map_err(|e| {
                        FeedError::InvalidArchive {
                            key: key.to_string(),
                            error: e.to_string(),
                        }
                    })?;
                    if archive.hash == hashed_post {
                        info!(post_key = %key, "No changes here");
                        // Continue processing the rest of the feed; an older post
                        // might still have changed even if this one has not.
                        continue;
                    }

                    info!(post_key = %key, "Post has changed, updating Slack");
                    match slack_client.update_message(&item, &archive.timestamp).await {
                        Ok(_) => {
                            archive.hash = hashed_post;
                            let raw = serde_json::to_string(&archive).map_err(|e| {
                                FeedError::SerializeArchive {
                                    key: key.to_string(),
                                    error: e.to_string(),
                                }
                            })?;
                            match store.set(&key, &raw).await {
                                Ok(()) => {
                                    info!(post_key = %key, "Finished updating Slack, and Redis")
                                }
                                Err(err) => {
                                    error!(post_key = %key, error = %err, "Failed saving to Redis")
                                }
                            }
                        }
                        Err(err) => {
                            error!(post_key = %key, error = %err, "Failed posting to Slack")
                        }
                    };
                }
                Err(err) => error!(post_key = %key, error = %err, "Failed getting key from Redis"),
            }
        } else {
            let preview = format!(
                "<{}|{}>\n{}",
                item.link,
                item.title,
                slack::format_slack_post(&item.content)
            );
            info!(
                post_key = %key,
                title = %item.title,
                "No Redis connection available (DRY_RUN or connection error), would post Slack message and skip persistence"
            );
            tracing::debug!(post_key = %key, %preview, "DRY_RUN Slack preview body");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::handle_feed;
    use crate::config::{AppConfig, AppState};

    const SAMPLE_RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>NAIS Log</title>
    <item>
      <title>Test Post</title>
      <link>https://nais.io/log#test-post</link>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <encoded><![CDATA[This is **content** with a [link](https://example.com).]]></encoded>
    </item>
  </channel>
</rss>"#;

    #[tokio::test]
    async fn handle_feed_succeeds_in_dry_run() {
        let config = AppConfig::DryRun;
        let state = AppState::new(config);

        let result = handle_feed(SAMPLE_RSS, &state).await;
        assert!(result.is_ok());
    }
}
