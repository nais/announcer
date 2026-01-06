use crate::{
    config,
    redis_client::{InMemoryRedis, RedisClient, RedisStore},
    slack::{self, HttpSlackClient, SlackClient, StdoutSlackClient},
};
use log::{error, info};
use serde::{Deserialize, Serialize};

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

pub async fn handle_feed(xml: &str, app_config: &config::AppConfig) {
    let doc: Rss = match quick_xml::de::from_str(xml) {
        Ok(d) => d,
        Err(e) => {
            error!("Parsing XML failed: {e}");
            return;
        }
    };
    info!(
        "Found {} posts in {}",
        doc.channel.posts.len(),
        doc.channel.title
    );

    let mut redis_client: Option<Box<dyn RedisClient>> = if app_config.mode.is_dry_run() {
        info!("DRY_RUN is set, using in-memory Redis");
        Some(Box::new(InMemoryRedis::new()))
    } else if let Some(redis_cfg) = app_config.redis_config() {
        RedisStore::connect(redis_cfg).map(|store| Box::new(store) as Box<dyn RedisClient>)
    } else {
        info!("No Redis configuration available, skipping Redis connectivity and persistence");
        None
    };

    let slack_client: Box<dyn SlackClient> = if app_config.mode.is_dry_run() {
        Box::new(StdoutSlackClient::default())
    } else {
        match app_config.slack_config() {
            Ok(cfg) => Box::new(HttpSlackClient::new(cfg.clone())),
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
                    let mut archive = serde_json::from_str::<Archive>(&raw).unwrap();
                    if archive.hash == hashed_post {
                        info!("No changes here");
                        return;
                    }

                    info!("Post has changed, updating Slack");
                    match slack_client.update_message(&item, &archive.timestamp).await {
                        Ok(_) => {
                            archive.hash = hashed_post;
                            let raw = serde_json::to_string(&archive).unwrap();
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
}
