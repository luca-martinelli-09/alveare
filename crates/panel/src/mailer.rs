//! Email del pannello (inviti, reset della password) via SMTP, di solito AWS SES.
//! Senza SMTP gli utenti nuovi ricevono una password temporanea, come nell'MVP.
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub from: String,
}

#[derive(Clone)]
pub enum Mailer {
    None,
    Smtp(Smtp),
    /// Nei test: le email restano qui.
    Memory(Arc<Mutex<Vec<(String, String, String)>>>),
}

impl Mailer {
    pub fn enabled(&self) -> bool {
        !matches!(self, Mailer::None)
    }

    pub async fn send(&self, to: &str, subject: &str, body: &str) -> Result<()> {
        match self {
            Mailer::None => anyhow::bail!("SMTP del pannello non configurato"),
            Mailer::Memory(sent) => {
                sent.lock().expect("lock").push((to.into(), subject.into(), body.into()));
                Ok(())
            }
            Mailer::Smtp(cfg) => {
                use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
                let _ = rustls::crypto::ring::default_provider().install_default();
                let msg = Message::builder()
                    .from(cfg.from.parse().context("mittente non valido (PANEL_SMTP_FROM)")?)
                    .to(to.parse().context("destinatario non valido")?)
                    .subject(subject)
                    .header(lettre::message::header::ContentType::TEXT_PLAIN)
                    .body(body.to_string())?;
                let transport = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?
                    .port(cfg.port)
                    .credentials(lettre::transport::smtp::authentication::Credentials::new(cfg.user.clone(), cfg.password.clone()))
                    .timeout(Some(std::time::Duration::from_secs(30)))
                    .build();
                transport.send(msg).await.context("invio dell'email")?;
                Ok(())
            }
        }
    }
}
