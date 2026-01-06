use color_eyre::eyre::{eyre, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    DryRun,
}

impl Mode {
    pub fn is_dry_run(self) -> bool {
        matches!(self, Mode::DryRun)
    }
}

#[derive(Debug, Clone)]
pub struct RedisConfig {
    pub uri: String,
}

#[derive(Debug, Clone)]
pub struct SlackConfig {
    pub token: String,
    pub channel_id: String,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mode: Mode,
    pub redis: Option<RedisConfig>,
    pub slack: Option<SlackConfig>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let mode = if std::env::var("DRY_RUN").is_ok() {
            Mode::DryRun
        } else {
            Mode::Normal
        };

        let slack = if mode.is_dry_run() {
            None
        } else {
            let token = std::env::var("SLACK_TOKEN")
                .wrap_err("Missing SLACK_TOKEN env; required in normal mode")?;
            let channel_id = std::env::var("SLACK_CHANNEL_ID")
                .wrap_err("Missing SLACK_CHANNEL_ID env; required in normal mode")?;
            Some(SlackConfig { token, channel_id })
        };

        let redis = if mode.is_dry_run() {
            None
        } else if std::env::var("NAIS_CLUSTER_NAME").is_ok() {
            let host = std::env::var("REDIS_HOST_RSS")
                .wrap_err("Missing REDIS_HOST_RSS env; required when running in NAIS")?;
            let username = std::env::var("REDIS_USERNAME_RSS")
                .wrap_err("Missing REDIS_USERNAME_RSS env; required when running in NAIS")?;
            let password = std::env::var("REDIS_PASSWORD_RSS")
                .wrap_err("Missing REDIS_PASSWORD_RSS env; required when running in NAIS")?;
            let port = std::env::var("REDIS_PORT_RSS")
                .wrap_err("Missing REDIS_PORT_RSS env; required when running in NAIS")?;

            let uri = format!("rediss://{username}:{password}@{host}:{port}");
            Some(RedisConfig { uri })
        } else {
            // Local development default; matches previous behaviour.
            Some(RedisConfig {
                uri: "redis://localhost:6379".to_string(),
            })
        };

        Ok(Self {
            mode,
            redis,
            slack,
        })
    }

    pub fn slack_config(&self) -> Result<&SlackConfig> {
        self.slack
            .as_ref()
            .ok_or_else(|| eyre!("Slack configuration missing"))
    }

    pub fn redis_config(&self) -> Option<&RedisConfig> {
        self.redis.as_ref()
    }
}

