use crate::{config, slack};
use log::{error, info};
use redis::{Commands, RedisResult};
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

    let mut connection = if app_config.mode.is_dry_run() {
        info!("DRY_RUN is set, skipping Redis connectivity and persistence");
        None
    } else if let Some(redis_cfg) = app_config.redis_config() {
        match redis::Client::open(redis_cfg.uri.clone()) {
            Ok(client) => match client.get_connection() {
                Ok(conn) => Some(conn),
                Err(err) => {
                    error!("Opening connection to Redis failed: {err}");
                    None
                }
            },
            Err(err) => {
                error!("Connecting to Redis failed: {err}");
                None
            }
        }
    } else {
        info!("No Redis configuration available, skipping Redis connectivity and persistence");
        None
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

        if let Some(con) = &mut connection {
            match con.get::<_, Option<String>>(&key) {
                Ok(None) => {
                    info!("New post, pushing to Slack");
                    let slack_cfg = match app_config.slack_config() {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            error!("Slack configuration missing when trying to post: {e}");
                            continue;
                        }
                    };
                    match slack::post_message(&item, slack_cfg).await {
                        Ok(response) => {
                            let archive = Archive {
                                hash: hashed_post,
                                timestamp: response.ts,
                            };
                            let raw = serde_json::to_string(&archive).unwrap();
                            let result: RedisResult<()> = con.set(key, raw);

                            match result {
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
                    let slack_cfg = match app_config.slack_config() {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            error!("Slack configuration missing when trying to update: {e}");
                            continue;
                        }
                    };
                    match slack::update_message(&item, &archive.timestamp, slack_cfg).await {
                        Ok(_) => {
                            archive.hash = hashed_post;
                            let raw = serde_json::to_string(&archive).unwrap();
                            let result: RedisResult<()> = con.set(key, raw);

                            match result {
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
