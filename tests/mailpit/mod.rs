use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{free_port, wait_for_port};

/// A throwaway mail server: the app delivers notifications to its SMTP port,
/// and the test reads them back over its REST API.
pub struct Mailpit {
    process: Child,
    smtp_port: u16,
    api_port: u16,
    data_dir: PathBuf,
    api: reqwest::Client,
}

const MAILPIT_USERNAME: &str = "user";
const MAILPIT_PASSWORD: &str = "pass";

#[derive(Deserialize)]
struct MessagesSummary {
    messages: Vec<MessageSummary>,
}

#[derive(Deserialize)]
struct MessageSummary {
    #[serde(rename = "ID")]
    id: String,
}

#[derive(Deserialize)]
struct MessageText {
    #[serde(rename = "Text")]
    text: String,
}

impl Mailpit {
    pub async fn start() -> Result<Self> {
        let smtp_port = free_port()?;
        let api_port = free_port()?;
        // the default database is a temp file shared by every instance, and
        // concurrent tests each need their own mailbox
        let data_dir = std::env::temp_dir().join(format!("bouc-mailpit-{smtp_port}"));
        std::fs::create_dir_all(&data_dir).context("creating the mailpit data directory")?;
        let process = Command::new("mailpit")
            .arg(format!(
                "--database={}",
                data_dir.join("mailpit.db").display()
            ))
            .arg(format!("--smtp=127.0.0.1:{smtp_port}"))
            .arg(format!("--listen=127.0.0.1:{api_port}"))
            .arg("--disable-version-check")
            .arg("--quiet")
            .env(
                "MP_SMTP_AUTH",
                format!("{MAILPIT_USERNAME}:{MAILPIT_PASSWORD}"),
            )
            .env("MP_SMTP_AUTH_ALLOW_INSECURE", "true")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawning mailpit")?;
        let mailpit = Self {
            process,
            smtp_port,
            api_port,
            data_dir,
            api: reqwest::Client::new(),
        };

        wait_for_port(smtp_port, "the mailpit SMTP server").await?;
        wait_for_port(api_port, "the mailpit API").await?;

        Ok(mailpit)
    }

    pub fn smtp_url(&self) -> String {
        format!(
            "smtp://{MAILPIT_USERNAME}:{MAILPIT_PASSWORD}@127.0.0.1:{}",
            self.smtp_port
        )
    }

    pub async fn message_bodies(&self) -> Result<Vec<String>> {
        let base = format!("http://127.0.0.1:{}/api/v1", self.api_port);
        let summary: MessagesSummary = self
            .api
            .get(format!("{base}/messages"))
            .send()
            .await
            .context("listing messages")?
            .error_for_status()
            .context("listing messages")?
            .json()
            .await
            .context("decoding the message list")?;

        let mut bodies = Vec::with_capacity(summary.messages.len());

        for message in summary.messages {
            let body: MessageText = self
                .api
                .get(format!("{base}/message/{}", message.id))
                .send()
                .await
                .with_context(|| format!("getting message {}", message.id))?
                .error_for_status()
                .with_context(|| format!("getting message {}", message.id))?
                .json()
                .await
                .with_context(|| format!("decoding message {}", message.id))?;
            bodies.push(body.text);
        }

        Ok(bodies)
    }
}

impl Drop for Mailpit {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}
