use crate::config::ValkeyConfig;
use async_trait::async_trait;
use redis::{Commands, Connection, RedisResult};
use std::collections::HashMap;
use tracing::error;

#[async_trait]
pub trait ValkeyClient: Send {
    async fn get(&mut self, key: &str) -> RedisResult<Option<String>>;
    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()>;
}

pub struct ValkeyStore {
    connection: Connection,
}

impl ValkeyStore {
    pub fn connect(config: &ValkeyConfig) -> Option<Self> {
        match redis::Client::open(config.uri.clone()) {
            Ok(client) => match client.get_connection() {
                Ok(connection) => Some(Self { connection }),
                Err(err) => {
                    error!("Opening connection to Valkey failed: {err}");
                    None
                }
            },
            Err(err) => {
                error!("Connecting to Valkey failed: {err}");
                None
            }
        }
    }
}

#[async_trait]
impl ValkeyClient for ValkeyStore {
    async fn get(&mut self, key: &str) -> RedisResult<Option<String>> {
        self.connection.get(key)
    }

    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()> {
        self.connection.set(key, value)
    }
}

pub struct InMemoryValkey {
    store: HashMap<String, String>,
}

impl InMemoryValkey {
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }
}

#[async_trait]
impl ValkeyClient for InMemoryValkey {
    async fn get(&mut self, key: &str) -> RedisResult<Option<String>> {
        Ok(self.store.get(key).cloned())
    }

    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()> {
        self.store.insert(key.to_string(), value.to_string());
        Ok(())
    }
}
