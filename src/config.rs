//! Environment configuration module

use dotenv::dotenv;
use once_cell::sync::Lazy;
use std::env;

/// Application configuration loaded from environment variables.
#[derive(Debug)]
pub struct Environment {
    pub server_port: String,
    pub server_url: String,
    pub api_key: String,
    pub aws_region: String,
    pub aws_ses_from_email: String,
    pub max_send_per_second: i32,
    pub sentry_dsn: String,
}

#[allow(clippy::non_std_lazy_statics)]
static ENVIRONMENTS: Lazy<Environment> = Lazy::new(|| {
    dotenv().ok();

    Environment {
        server_port: env::var("SERVER_PORT").unwrap_or_else(|_| "8080".to_owned()),
        server_url: env::var("SERVER_URL").unwrap_or_default(),
        api_key: env::var("API_KEY").unwrap_or_default(),
        aws_region: env::var("AWS_REGION").unwrap_or_else(|_| "ap-northeast-2".to_owned()),
        aws_ses_from_email: env::var("AWS_SES_FROM_EMAIL").unwrap_or_default(),
        max_send_per_second: env::var("MAX_SEND_PER_SECOND")
            .unwrap_or_else(|_| "24".to_owned())
            .parse()
            .unwrap_or(24),
        sentry_dsn: env::var("SENTRY_DSN").unwrap_or_default(),
    }
});

#[must_use]
pub fn get_environments() -> &'static Environment {
    &ENVIRONMENTS
}
