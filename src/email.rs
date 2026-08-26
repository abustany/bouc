use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

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

/// Clones share the captured messages, so a test can keep one to read what
/// the app sent.
#[derive(Clone, Default)]
pub struct CaptureSender {
    messages: Arc<Mutex<Vec<Message>>>,
}

impl CaptureSender {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn messages(&self) -> Vec<Message> {
        self.messages.lock().await.clone()
    }
}

#[async_trait]
impl Sender for CaptureSender {
    async fn send(&self, message: Message) -> anyhow::Result<()> {
        let mut messages = self.messages.lock().await;
        messages.push(message);
        Ok(())
    }
}
