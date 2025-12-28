//! AWS SES email sending service with retry logic

use std::time::Duration;

use aws_config::{meta::region::RegionProviderChain, BehaviorVersion};
use aws_sdk_sesv2::{
    config::Region,
    error::SdkError,
    types::{Body, Content, Destination, EmailContent, Message},
    Client,
};
use thiserror::Error;
use tokio::sync::OnceCell;
use tracing::warn;

use crate::config;

// Retry configuration
const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 100;

static SES_CLIENT: OnceCell<Client> = OnceCell::const_new();

async fn get_ses_client() -> &'static Client {
    SES_CLIENT
        .get_or_init(|| async {
            let envs = config::get_environments();
            let region = &envs.aws_region;

            let region_provider = RegionProviderChain::first_try(Region::new(region.clone()))
                .or_default_provider()
                .or_else(Region::new(region.clone()));

            let config = aws_config::defaults(BehaviorVersion::latest())
                .region(region_provider)
                .load()
                .await;

            Client::new(&config)
        })
        .await
}

#[derive(Debug, Error)]
pub enum SendEmailError {
    #[error("Failed to build email: {0}")]
    Build(String),

    #[error("SES SDK error: {0}")]
    Sdk(String),

    #[error("Max retries exceeded: {0}")]
    #[allow(dead_code)]
    MaxRetriesExceeded(String),
}

/// Checks if an SES error is retryable (throttling, transient network issues).
fn is_retryable_error<E: std::fmt::Debug>(err: &SdkError<E>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(e) if e.raw().status().as_u16() == 429
    ) || matches!(
        err,
        SdkError::TimeoutError(_) | SdkError::DispatchFailure(_)
    )
}

/// Sends an email via AWS SES with exponential backoff retry.
///
/// Returns the SES message ID on success.
pub async fn send_email(
    sender: &str,
    recipient: &str,
    subject: &str,
    body: &str,
) -> Result<String, SendEmailError> {
    let client = get_ses_client().await;

    let subject_content = Content::builder()
        .data(subject)
        .charset("UTF-8")
        .build()
        .map_err(|e| SendEmailError::Build(format!("subject: {e:?}")))?;

    let body_content = Content::builder()
        .data(body)
        .charset("UTF-8")
        .build()
        .map_err(|e| SendEmailError::Build(format!("body: {e:?}")))?;

    let message = Message::builder()
        .subject(subject_content)
        .body(Body::builder().html(body_content).build())
        .build();

    let email_content = EmailContent::builder().simple(message).build();
    let destination = Destination::builder().to_addresses(recipient).build();

    let mut attempts = 0;

    loop {
        match client
            .send_email()
            .from_email_address(sender)
            .destination(destination.clone())
            .content(email_content.clone())
            .send()
            .await
        {
            Ok(resp) => {
                return Ok(resp.message_id().unwrap_or_default().to_string());
            }
            Err(e) if is_retryable_error(&e) && attempts < MAX_RETRIES => {
                attempts += 1;
                let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2_u64.pow(attempts));
                warn!(
                    "SES retry {}/{} for {}: {:?}, waiting {:?}",
                    attempts, MAX_RETRIES, recipient, e, backoff
                );
                tokio::time::sleep(backoff).await;
            }
            Err(e) => {
                return Err(SendEmailError::Sdk(format!("{e:?}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_email_error_display() {
        let err = SendEmailError::Build("test".to_string());
        assert!(err.to_string().contains("Failed to build email"));

        let err = SendEmailError::Sdk("sdk error".to_string());
        assert!(err.to_string().contains("SES SDK error"));

        let err = SendEmailError::MaxRetriesExceeded("timeout".to_string());
        assert!(err.to_string().contains("Max retries exceeded"));
    }
}
