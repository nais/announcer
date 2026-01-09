use crate::config::ValkeyConfig;
use async_trait::async_trait;
use redis::{Commands, Connection, ErrorKind, RedisError, RedisResult};
use std::collections::HashMap;
use tokio::task;
use tracing::error;

#[async_trait]
pub trait ValkeyClient: Send {
    async fn get(&mut self, key: &str) -> RedisResult<Option<String>>;
    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()>;
}

pub struct ValkeyStore {
    connection: Option<Connection>,
}

impl ValkeyStore {
    pub fn connect(config: &ValkeyConfig) -> Option<Self> {
        match redis::Client::open(config.uri.clone()) {
            Ok(client) => match client.get_connection() {
                Ok(connection) => Some(Self {
                    connection: Some(connection),
                }),
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
        let key = key.to_owned();
        let mut conn = match self.connection.take() {
            Some(c) => c,
            None => {
                return Err(RedisError::from((
                    ErrorKind::IoError,
                    "Valkey connection not available",
                )))
            }
        };

        let result = task::spawn_blocking(move || {
            let res = conn.get(key);
            (conn, res)
        })
        .await;

        match result {
            Ok((conn, res)) => {
                self.connection = Some(conn);
                res
            }
            Err(e) => Err(RedisError::from((
                ErrorKind::IoError,
                "spawn_blocking join error",
                e.to_string(),
            ))),
        }
    }

    async fn set(&mut self, key: &str, value: &str) -> RedisResult<()> {
        let key = key.to_owned();
        let value = value.to_owned();
        let mut conn = match self.connection.take() {
            Some(c) => c,
            None => {
                return Err(RedisError::from((
                    ErrorKind::IoError,
                    "Valkey connection not available",
                )))
            }
        };

        let result = task::spawn_blocking(move || {
            let res = conn.set(key, value);
            (conn, res)
        })
        .await;

        match result {
            Ok((conn, res)) => {
                self.connection = Some(conn);
                res
            }
            Err(e) => Err(RedisError::from((
                ErrorKind::IoError,
                "spawn_blocking join error",
                e.to_string(),
            ))),
        }
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
