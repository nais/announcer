use crate::{
    config,
    redis_client::{InMemoryRedis, RedisClient, RedisStore},
    slack::{self, HttpSlackClient, SlackClient, StdoutSlackClient},
};
use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

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

pub async fn handle_feed(xml: &str, app_state: &config::AppState) -> Result<()> {
    let doc: Rss = quick_xml::de::from_str(xml).wrap_err("Parsing RSS XML failed")?;
    info!(
        "Found {} posts in {}",
        doc.channel.posts.len(),
        doc.channel.title
    );

    let mut redis_client: Option<Box<dyn RedisClient>> = if app_state.config.mode.is_dry_run() {
        info!("DRY_RUN is set, using in-memory Redis");
        Some(Box::new(InMemoryRedis::new()))
    } else if let Some(redis_cfg) = app_state.config.redis_config() {
        RedisStore::connect(redis_cfg).map(|store| Box::new(store) as Box<dyn RedisClient>)
    } else {
        info!("No Redis configuration available, skipping Redis connectivity and persistence");
        None
    };

    let slack_client: Box<dyn SlackClient> = if app_state.config.mode.is_dry_run() {
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
        let key = item.link.split('#').collect::<Vec<&str>>()[1].to_owned();
        info!(
            "Handling '{}' (date: {}, key: {key})",
            item.title, item.pub_date
        );

        let hashed_post = format!(
            "{:x}",
            md5::compute(format!("{}-{}", item.title, item.content))
        );

        if let Some(store) = &mut redis_client {
            match store.get(&key).await {
                Ok(None) => {
                    info!("New post, pushing to Slack");
                    match slack_client.post_message(&item).await {
                        Ok(response) => {
                            let archive = Archive {
                                hash: hashed_post,
                                timestamp: response.ts,
                            };
                            let raw = serde_json::to_string(&archive).unwrap();
                            match store.set(&key, &raw).await {
                                Ok(()) => info!("Posted to Slack, and saved to Redis"),
                                Err(err) => error!("Failed saving to Redis: {err}"),
                            }
                        }
                        Err(err) => error!("Failed posting to Slack: {err}"),
                    };
                }
                Ok(Some(raw)) => {
                    let mut archive = serde_json::from_str::<Archive>(&raw)
                        .wrap_err_with(|| format!("Invalid archive JSON for key {key}"))?;
                    if archive.hash == hashed_post {
                        info!("No changes here");
                        // continue processing the rest of the feed. an older post
                        // might still have changed even if this one has not.
                        // Todo: ask kyrre about this guy actually. are we just optimizing this?
                        continue;
                    }

                    info!("Post has changed, updating Slack");
                    match slack_client.update_message(&item, &archive.timestamp).await {
                        Ok(_) => {
                            archive.hash = hashed_post;
                            let raw = serde_json::to_string(&archive)
                                .wrap_err_with(|| format!("Serializing archive for key {key}"))?;
                            match store.set(&key, &raw).await {
                                Ok(()) => info!("Finished updating Slack, and Redis"),
                                Err(err) => error!("Failed saving to Redis: {err}"),
                            }
                        }
                        Err(err) => error!("Failed posting to Slack: {err}"),
                    };
                }
                Err(err) => error!("Failed getting {key} from Redis: {err}"),
            }
        } else {
            let preview = format!(
                "<{}|{}>\n{}",
                item.link,
                item.title,
                slack::format_slack_post(&item.content)
            );
            info!(
                "No Redis connection available (DRY_RUN or connection error), \
would post Slack message and skip persistence:\n{}",
                preview
            );
        }
    }

    Ok(())
}
