//! 환경 변수 설정 모듈.

use std::env;
use std::sync::{LazyLock, Once};

static INIT: Once = Once::new();

/// Initializes the environment by loading the .env file.
fn init_env() {
    INIT.call_once(|| {
        if let Err(e) = dotenvy::dotenv() {
            tracing::warn!("Warning: .env file not found or error loading: {e}");
        }
    });
}

/// Retrieves an environment variable by key.
///
/// If the variable is not set, returns the provided default value.
/// If no default is provided and the variable is not set, returns an empty string.
#[must_use]
pub fn get_env(key: &str, default: Option<&str>) -> String {
    init_env();
    env::var(key).unwrap_or_else(|_| default.unwrap_or("").to_string())
}

/// Retrieves an environment variable as a parsed type.
#[must_use]
pub fn get_env_parsed<T: std::str::FromStr>(key: &str, default: T) -> T {
    init_env();
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct AppConfig {
    // Server settings
    pub server_port: String,
    pub server_url: String,

    // API authentication
    pub api_key: String,

    // AWS settings
    pub aws_region: String,
    pub aws_ses_from_email: String,

    // Rate limiting
    pub max_send_per_second: i32,

    // Database settings
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout_secs: u64,
    pub db_idle_timeout_secs: u64,

    // Channel settings
    pub send_channel_buffer: usize,
    pub post_send_channel_buffer: usize,

    // Sentry settings
    pub sentry_dsn: String,
    pub sentry_traces_sample_rate: f32,
}

impl AppConfig {
    /// Creates a new `AppConfig` from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            server_port: get_env("SERVER_PORT", Some("8080")),
            server_url: get_env("SERVER_URL", None),

            api_key: get_env(
                "API_KEY",
                if cfg!(test) {
                    Some("test-api-key-12345")
                } else {
                    None
                },
            ),

            aws_region: get_env("AWS_REGION", Some("ap-northeast-2")),
            aws_ses_from_email: get_env("AWS_SES_FROM_EMAIL", None),

            max_send_per_second: get_env_parsed("MAX_SEND_PER_SECOND", 24),

            db_max_connections: get_env_parsed("DB_MAX_CONNECTIONS", 20),
            db_min_connections: get_env_parsed("DB_MIN_CONNECTIONS", 5),
            db_acquire_timeout_secs: get_env_parsed("DB_ACQUIRE_TIMEOUT_SECS", 30),
            db_idle_timeout_secs: get_env_parsed("DB_IDLE_TIMEOUT_SECS", 300),

            send_channel_buffer: get_env_parsed("SEND_CHANNEL_BUFFER", 10_000),
            post_send_channel_buffer: get_env_parsed("POST_SEND_CHANNEL_BUFFER", 1_000),

            sentry_dsn: get_env("SENTRY_DSN", None),
            sentry_traces_sample_rate: get_env_parsed("SENTRY_TRACES_SAMPLE_RATE", 0.1),
        }
    }
}

/// Global application configuration instance.
pub static APP_CONFIG: LazyLock<AppConfig> = LazyLock::new(AppConfig::from_env);

/// Legacy compatibility wrapper for existing code.
/// Returns a reference to the global configuration.
#[cfg(test)]
#[must_use]
pub fn get_environments() -> &'static AppConfig {
    &APP_CONFIG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_env_with_default() {
        let result = get_env("NON_EXISTENT_VAR_FOR_TEST_12345", Some("default_value"));
        assert_eq!(result, "default_value");
    }

    #[test]
    fn test_get_env_no_default() {
        let result = get_env("NON_EXISTENT_VAR_FOR_TEST_67890", None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_get_env_empty_default() {
        let result = get_env("NON_EXISTENT_VAR_99999", Some(""));
        assert_eq!(result, "");
    }

    #[test]
    fn test_get_env_parsed_default_u32() {
        let result: u32 = get_env_parsed("NON_EXISTENT_U32_VAR", 42);
        assert_eq!(result, 42);
    }

    #[test]
    fn test_get_env_parsed_default_i32() {
        let result: i32 = get_env_parsed("NON_EXISTENT_I32_VAR", 24);
        assert_eq!(result, 24);
    }

    #[test]
    fn test_get_env_parsed_default_usize() {
        let result: usize = get_env_parsed("NON_EXISTENT_USIZE_VAR", 10_000);
        assert_eq!(result, 10_000);
    }

    #[test]
    fn test_get_env_parsed_default_f32() {
        let result: f32 = get_env_parsed("NON_EXISTENT_F32_VAR", 0.5);
        assert!((result - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_app_config_from_env() {
        let config = AppConfig::from_env();

        assert!(!config.server_port.is_empty());
        assert!(config.db_max_connections > 0);
        assert!(config.db_min_connections > 0);
        assert!(config.max_send_per_second > 0);
    }

    #[test]
    fn test_app_config_default_values() {
        let config = AppConfig::from_env();

        assert_eq!(config.server_port, get_env("SERVER_PORT", Some("8080")));
        assert!(config.db_max_connections >= config.db_min_connections);
    }

    #[test]
    fn test_app_config_clone() {
        let config = AppConfig::from_env();
        let cloned = config.clone();

        assert_eq!(config.server_port, cloned.server_port);
        assert_eq!(config.db_max_connections, cloned.db_max_connections);
    }

    #[test]
    fn test_app_config_debug() {
        let config = AppConfig::from_env();
        let debug_str = format!("{config:?}");

        assert!(debug_str.contains("AppConfig"));
        assert!(debug_str.contains("server_port"));
        assert!(debug_str.contains("db_max_connections"));
    }

    #[test]
    fn test_get_environments_returns_config() {
        let config = get_environments();
        assert!(!config.server_port.is_empty());
    }

    #[test]
    fn test_app_config_global_same_instance() {
        let port1 = APP_CONFIG.server_port.clone();
        let port2 = APP_CONFIG.server_port.clone();
        assert_eq!(port1, port2);
    }

    #[test]
    fn test_get_env_special_characters_in_default() {
        let result = get_env("NON_EXISTENT_SPECIAL", Some("!@#$%^&*()"));
        assert_eq!(result, "!@#$%^&*()");
    }

    #[test]
    fn test_get_env_unicode_default() {
        let result = get_env("NON_EXISTENT_UNICODE", Some("한글테스트"));
        assert_eq!(result, "한글테스트");
    }
}
