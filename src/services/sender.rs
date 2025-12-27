//! AWS SES email sending service

use aws_config::{meta::region::RegionProviderChain, BehaviorVersion};
use aws_sdk_sesv2::{
    config::Region,
    types::{Body, Content, Destination, EmailContent, Message},
    Client,
};
use thiserror::Error;
use tokio::sync::OnceCell;

use crate::config;

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
}

/// Sends an email via AWS SES.
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

    let resp = client
        .send_email()
        .from_email_address(sender)
        .destination(Destination::builder().to_addresses(recipient).build())
        .content(EmailContent::builder().simple(message).build())
        .send()
        .await
        .map_err(|e| SendEmailError::Sdk(format!("{e:?}")))?;

    Ok(resp.message_id().unwrap_or_default().to_string())
}
