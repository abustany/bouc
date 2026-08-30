use anyhow::{Context, Result};
use async_trait::async_trait;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor, message::header::ContentType};

pub enum SmtpTlsConfig {
    Disabled,
    TLS,
    StartTLS,
}

pub struct SmtpConfig {
    pub server_address: String,
    pub username: String,
    pub password: String,
    pub tls_config: SmtpTlsConfig,
}

#[derive(Clone)]
pub struct Message {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[async_trait]
pub trait Sender: Send + Sync {
    async fn send(&self, message: Message) -> anyhow::Result<()>;
}

pub struct ConsoleSender {}

impl Default for ConsoleSender {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleSender {
    pub fn new() -> Self {
        ConsoleSender {}
    }
}

#[async_trait]
impl Sender for ConsoleSender {
    async fn send(&self, message: Message) -> anyhow::Result<()> {
        println!("---- ✉️ New message ----");
        println!("From: {}", message.from);
        println!("To: {}", message.to);
        println!("Subject: {}", message.subject);
        println!("Body: {}", message.body);
        println!("-----------------------");
        Ok(())
    }
}

pub struct SmtpSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl SmtpSender {
    pub fn new(url: &str) -> Result<Self> {
        Ok(Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
                .context("configuring SMTP transport")?
                .build(),
        })
    }
}

#[async_trait]
impl Sender for SmtpSender {
    async fn send(&self, message: Message) -> anyhow::Result<()> {
        self.transport
            .send(
                lettre::Message::builder()
                    .from(message.from.parse().context("parsing from address")?)
                    .to(message.to.parse().context("parsing to address")?)
                    .subject(message.subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(message.body)
                    .context("building message")?,
            )
            .await?;
        Ok(())
    }
}
