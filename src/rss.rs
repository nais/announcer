use crate::redis::Commands;
use crate::slack;
use log::{error, info};
use redis::RedisResult;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Post {
    pub title: String,
    pub link: String,
    #[serde(rename = "pubDate")]
    pub_date: String,
    #[serde(rename = "encoded")]
    pub content: String,
}

#[derive(Deserialize)]
struct Feed {
    title: String,
    #[serde(rename = "item")]
    posts: Vec<Post>,
}

#[derive(Deserialize)]
struct RSS {
    feed: Feed,
}

#[derive(Deserialize, Serialize)]
pub struct Archive {
    pub hash: String,
    pub timestamp: String,
}

pub async fn handle_feed(xml: &str) {
    let doc: RSS = quick_xml::de::from_str(xml).unwrap();
    info!("Found {} posts in {}", doc.feed.posts.len(), doc.feed.title);

    let uri: String;

    if std::env::var("NAIS_CLUSTER_NAME").is_ok() {
        let host = std::env::var("REDIS_HOST_RSS").unwrap();
        let username = std::env::var("REDIS_USERNAME_RSS").unwrap();
        let password = std::env::var("REDIS_PASSWORD_RSS").unwrap();
        let port = std::env::var("REDIS_PORT_RSS").unwrap();
        uri = format!("rediss://{username}:{password}@{host}:{port}");
    } else {
        uri = "redis://localhost:6379".to_string();
    }

    let client = match redis::Client::open(uri) {
        Ok(c) => c,
        Err(err) => {
            error!("Connecting to Redis failed: {}", err);
            return;
        }
    };

    let mut con = match client.get_connection() {
        Ok(c) => c,
        Err(err) => {
            error!("Opening connection failed: {}", err);
            return;
        }
    };

    for item in doc.feed.posts {
        let key = item.link.split("#").collect::<Vec<&str>>()[1].to_owned();
        info!(
            "Handling '{}' (date: {}, key: {})",
            item.title, item.pub_date, key
        );

        let hashed_post = format!(
            "{:x}",
            md5::compute(format!("{}-{}", item.title, item.content))
        );

        match con.get::<_, Option<String>>(&key) {
            Ok(None) => {
                info!("New post, pushing to Slack");
                match slack::post_message(item).await {
                    Ok(response) => {
                        let archive = Archive {
                            hash: hashed_post,
                            timestamp: response.ts,
                        };
                        let raw = serde_json::to_string(&archive).unwrap();
                        let result: RedisResult<()> = con.set(key, raw);

                        match result {
                            Ok(_) => info!("Posted to Slack, and saved to Redis"),
                            Err(err) => error!("Failed saving to Redis: {}", err),
                        }
                    }
                    Err(err) => error!("Failed posting to Slack: {}", err),
                };
            }
            Ok(Some(raw)) => {
                let mut archive = serde_json::from_str::<Archive>(&raw).unwrap();
                if archive.hash != hashed_post {
                    info!("Post has changed, updating Slack");
                    match slack::update_message(item, &archive.timestamp).await {
                        Ok(_) => {
                            archive.hash = hashed_post;
                            let raw = serde_json::to_string(&archive).unwrap();
                            let result: RedisResult<()> = con.set(key, raw);

                            match result {
                                Ok(_) => info!("Finished updating Slack, and Redis"),
                                Err(err) => error!("Failed saving to Redis: {}", err),
                            }
                        }
                        Err(err) => error!("Failed posting to Slack: {}", err),
                    };
                } else {
                    info!("No changes here");
                }
            }
            Err(err) => error!("Failed getting {} from Redis: {}", key, err),
        }
    }
}
