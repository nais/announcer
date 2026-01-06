use crate::config::RedisConfig;
use async_trait::async_trait;
use log::error;
use redis::{Commands, Connection, RedisResult};
use std::collections::HashMap;

#[async_trait]
pub trait RedisClient: Send {
    async fn get(&mut self, key: &str) -> RedisResult<Option<String>>;
    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()>;
}

pub struct RedisStore {
    connection: Connection,
}

impl RedisStore {
    pub fn connect(config: &RedisConfig) -> Option<Self> {
        match redis::Client::open(config.uri.clone()) {
            Ok(client) => match client.get_connection() {
                Ok(connection) => Some(Self { connection }),
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
    }
}

#[async_trait]
impl RedisClient for RedisStore {
    async fn get(&mut self, key: &str) -> RedisResult<Option<String>> {
        self.connection.get(key)
    }

    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()> {
        self.connection.set(key, value)
    }
}

pub struct InMemoryRedis {
    store: HashMap<String, String>,
}

impl InMemoryRedis {
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }
}

#[async_trait]
impl RedisClient for InMemoryRedis {
    async fn get(&mut self, key: &str) -> RedisResult<Option<String>> {
        Ok(self.store.get(key).cloned())
    }

    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()> {
        self.store.insert(key.to_string(), value.to_string());
        Ok(())
    }
}
