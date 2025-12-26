use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use aws_sdk_sesv2::Client;

/// send_email
/// Send email using AWS SES
/// Environment variables AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION are required for sending
pub async fn send_email(
    client: &Client,
    sender: &str,
    recipient: &str,
    subject: &str,
    body: &str,
) -> Result<String, aws_sdk_sesv2::Error> {
    let message = Message::builder()
        .subject(
            Content::builder()
                .data(subject)
                .charset("UTF-8") // Using UTF-8 encoding
                .build()
                .expect("Failed to build subject content"),
        )
        .body(
            Body::builder()
                .html(
                    // Convert to HTML format
                    Content::builder()
                        .data(body)
                        .charset("UTF-8")
                        .build()
                        .expect("Failed to build body content"),
                )
                .build(),
        )
        .build();

    // Email send request
    let resp = client
        .send_email()
        .from_email_address(sender)
        .destination(Destination::builder().to_addresses(recipient).build())
        .content(EmailContent::builder().simple(message).build())
        .send()
        .await?;

    Ok(resp.message_id().unwrap_or_default().to_string()) // Return MessageId
}
