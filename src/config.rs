use color_eyre::eyre::{eyre, Context, Result};
use reqwest::Client;

#[derive(Debug, Clone)]
pub struct ValkeyConfig {
    pub uri: String,
}

#[derive(Debug, Clone)]
pub struct SlackConfig {
    pub token: String,
    pub channel_id: String,
}

#[derive(Debug, Clone)]
pub enum AppConfig {
    DryRun,
    Normal {
        valkey: ValkeyConfig,
        slack: SlackConfig,
    },
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        if std::env::var("DRY_RUN").is_ok() {
            return Ok(AppConfig::DryRun);
        }

        let token = std::env::var("SLACK_TOKEN")
            .wrap_err("Missing SLACK_TOKEN env; required in normal mode")?;
        let channel_id = std::env::var("SLACK_CHANNEL_ID")
            .wrap_err("Missing SLACK_CHANNEL_ID env; required in normal mode")?;
        let slack = SlackConfig { token, channel_id };

        let valkey = if std::env::var("NAIS_CLUSTER_NAME").is_ok() {
            let host = std::env::var("REDIS_HOST_RSS")
                .wrap_err("Missing REDIS_HOST_RSS env; required when running in NAIS")?;
            let username = std::env::var("REDIS_USERNAME_RSS")
                .wrap_err("Missing REDIS_USERNAME_RSS env; required when running in NAIS")?;
            let password = std::env::var("REDIS_PASSWORD_RSS")
                .wrap_err("Missing REDIS_PASSWORD_RSS env; required when running in NAIS")?;
            let port = std::env::var("REDIS_PORT_RSS")
                .wrap_err("Missing REDIS_PORT_RSS env; required when running in NAIS")?;

            let uri = format!("rediss://{username}:{password}@{host}:{port}");
            ValkeyConfig { uri }
        } else {
            ValkeyConfig {
                uri: "redis://localhost:6379".to_string(),
            }
        };

        Ok(AppConfig::Normal { valkey, slack })
    }

    pub fn is_dry_run(&self) -> bool {
        matches!(self, AppConfig::DryRun)
    }

    pub fn slack_config(&self) -> Result<&SlackConfig> {
        match self {
            AppConfig::Normal { slack, .. } => Ok(slack),
            AppConfig::DryRun => Err(eyre!("Slack configuration missing in DryRun mode")),
        }
    }

    pub fn valkey_config(&self) -> Option<&ValkeyConfig> {
        match self {
            AppConfig::Normal { valkey, .. } => Some(valkey),
            AppConfig::DryRun => None,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub http_client: Client,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
        }
    }
}
