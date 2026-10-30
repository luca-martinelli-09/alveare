//! Comandi della CLI: ognuno è una chiamata all'API del pannello.
use std::io::Write;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};

use crate::Api;

#[derive(Parser)]
#[command(name = "alveare", version = option_env!("ALVEARE_BUILD").unwrap_or(env!("CARGO_PKG_VERSION")), about = "CLI di Alveare: stessa API e stessi permessi del pannello")]
pub struct Cli {
    /// URL del pannello (es. https://pannello.esempio.it)
    #[arg(long, env = "ALVEARE_URL", global = true)]
    pub url: Option<String>,
    /// Token API personale (Account → Token API nel pannello)
    #[arg(long, env = "ALVEARE_TOKEN", hide_env_values = true, global = true)]
    pub token: Option<String>,
    /// Output JSON, per gli script
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Salva URL e token per i comandi successivi
    Login {
        #[arg(long)]
        url: String,
        #[arg(long)]
        token: String,
    },
    /// Siti: elenco, creazione, clonazione, esportazione
    #[command(subcommand)]
    Sites(Sites),
    /// Backup e ripristini
    #[command(subcommand)]
    Backup(Backup),
    /// Log in tempo reale e ricerca
    #[command(subcommand)]
    Logs(Logs),
    /// WP-CLI con l'utente del sito: alveare wp sona.it -- plugin list
    Wp {
        site: String,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
    /// Cron personalizzati
    #[command(subcommand)]
    Cron(Cron),
    /// Blocco degli IP
    #[command(subcommand)]
    Ip(Ip),
}

#[derive(Subcommand)]
pub enum Sites {
    /// Elenco dei siti visibili
    List,
    /// Nuovo sito da template (solo admin)
    Create {
        domain: String,
        #[arg(long, default_value = "wordpress")]
        template: String,
        #[arg(long)]
        title: Option<String>,
    },
    /// Copia un sito su un nuovo dominio, protetta come staging (solo admin)
    Clone { site: String, domain: String },
    /// Esportazione tar.gz (file, dump, LEGGIMI) scaricata in locale
    Export {
        site: String,
        #[arg(long, short)]
        output: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum Backup {
    /// Backup adesso
    Run { site: String },
    /// Backup del sito
    List { site: String },
    /// Ripristino da uno snapshot (prima viene fatto un backup dello stato attuale)
    Restore {
        site: String,
        snapshot: String,
        #[arg(long, value_enum, default_value_t = Mode::Full)]
        mode: Mode,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Mode {
    Full,
    Files,
    Database,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Kind {
    Access,
    PhpError,
    PhpSlow,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Access => "access",
            Kind::PhpError => "php-error",
            Kind::PhpSlow => "php-slow",
        }
    }
}

#[derive(Subcommand)]
pub enum Logs {
    /// Log in tempo reale (Ctrl-C per uscire)
    Tail {
        site: String,
        #[arg(long, value_enum, default_value_t = Kind::Access)]
        kind: Kind,
    },
    /// Cerca anche nei log ruotati
    Search {
        site: String,
        text: String,
        #[arg(long, value_enum, default_value_t = Kind::Access)]
        kind: Kind,
    },
}

#[derive(Subcommand)]
pub enum Cron {
    /// Cron personalizzati del sito
    List { site: String },
    /// Esegue subito un cron
    Run { site: String, id: i64 },
}

#[derive(Subcommand)]
pub enum Ip {
    /// Blocca un IP o una rete su un sito (o su tutti, solo admin)
    Block {
        site: String,
        ip: String,
        #[arg(long, default_value = "24h", value_parser = ["1h", "24h", "7d", "always"])]
        duration: String,
        #[arg(long)]
        all: bool,
    },
}

fn print(out: &mut impl Write, json_out: bool, v: &Value, human: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    if json_out {
        writeln!(out, "{}", serde_json::to_string_pretty(v)?)?;
        Ok(())
    } else {
        human(out)
    }
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    }
}

pub async fn run(api: &Api, cmd: Cmd, json_out: bool, out: &mut impl Write) -> Result<()> {
    match cmd {
        Cmd::Login { .. } => unreachable!("gestito prima di creare il client"),
        Cmd::Sites(Sites::List) => {
            let v = api.get("/api/sites").await?;
            print(out, json_out, &v, |o| {
                for site in v.as_array().into_iter().flatten() {
                    writeln!(o, "{:>4}  {:<32} {:<24} {:<10} {}", s(&site["id"]), s(&site["domain"]), s(&site["slug"]), s(&site["kind"]), s(&site["status"]))?;
                }
                Ok(())
            })
        }
        Cmd::Sites(Sites::Create { domain, template, title }) => {
            let title = title.unwrap_or_else(|| domain.clone());
            let j = api.run_job("/api/sites", json!({ "domain": domain, "template": template, "title": title }), out).await?;
            writeln!(out, "sito {} creato", s(&j["site_id"]))?;
            Ok(())
        }
        Cmd::Sites(Sites::Clone { site, domain }) => {
            let id = api.site_id(&site).await?;
            let j = api.run_job(&format!("/api/sites/{id}/clone"), json!({ "domain": domain }), out).await?;
            writeln!(out, "copia: sito {}", s(&j["site_id"]))?;
            Ok(())
        }
        Cmd::Sites(Sites::Export { site, output }) => {
            let id = api.site_id(&site).await?;
            api.run_job(&format!("/api/sites/{id}/export"), Value::Null, out).await?;
            let exports = api.get(&format!("/api/sites/{id}/exports")).await?;
            let latest = exports.as_array().and_then(|e| e.first()).context("esportazione non trovata")?;
            let file = s(&latest["file"]);
            let dest = output.unwrap_or_else(|| std::path::PathBuf::from(&file));
            let bytes = api.download(&format!("/api/exports/{}", s(&latest["token"])), &dest).await?;
            writeln!(out, "scaricato {} ({bytes} byte)", dest.display())?;
            Ok(())
        }
        Cmd::Backup(Backup::Run { site }) => {
            let id = api.site_id(&site).await?;
            api.run_job(&format!("/api/sites/{id}/backups"), Value::Null, out).await.map(drop)
        }
        Cmd::Backup(Backup::List { site }) => {
            let id = api.site_id(&site).await?;
            let v = api.get(&format!("/api/sites/{id}/backups")).await?;
            print(out, json_out, &v, |o| {
                for r in v["runs"].as_array().into_iter().flatten() {
                    writeln!(o, "{}  {:<7} {}", s(&r["started_at"]), s(&r["status"]), s(&r["snapshot_id"]).chars().take(8).collect::<String>())?;
                }
                Ok(())
            })
        }
        Cmd::Backup(Backup::Restore { site, snapshot, mode }) => {
            let id = api.site_id(&site).await?;
            let mode = match mode {
                Mode::Full => "full",
                Mode::Files => "files",
                Mode::Database => "database",
            };
            api.run_job(&format!("/api/sites/{id}/backups/{snapshot}/restore"), json!({ "mode": mode }), out).await.map(drop)
        }
        Cmd::Logs(Logs::Tail { site, kind }) => {
            let id = api.site_id(&site).await?;
            api.follow(&format!("/api/sites/{id}/logs/{}/stream", kind.as_str()), |v| {
                let line = if json_out || !v.is_object() { if v.is_string() { s(&v) } else { v.to_string() } } else { access_line(&v) };
                writeln!(out, "{line}")?;
                Ok(())
            })
            .await
        }
        Cmd::Logs(Logs::Search { site, text, kind }) => {
            let id = api.site_id(&site).await?;
            let q: String = text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
            let v = api.get(&format!("/api/sites/{id}/logs/{}?q={q}&lines=1000", kind.as_str())).await?;
            print(out, json_out, &v, |o| {
                for e in v["entries"].as_array().into_iter().flatten() {
                    writeln!(o, "{}", access_line(e))?;
                }
                for l in v["lines"].as_array().into_iter().flatten() {
                    writeln!(o, "{}", s(l))?;
                }
                Ok(())
            })
        }
        Cmd::Wp { site, args } => {
            let id = api.site_id(&site).await?;
            api.run_job(&format!("/api/sites/{id}/wp"), json!({ "args": args }), out).await.map(drop)
        }
        Cmd::Cron(Cron::List { site }) => {
            let id = api.site_id(&site).await?;
            let v = api.get(&format!("/api/sites/{id}/cron")).await?;
            print(out, json_out, &v, |o| {
                for c in v.as_array().into_iter().flatten() {
                    let cron = &c["cron"];
                    let state = if cron["active"] == true { "attivo" } else { "in pausa" };
                    writeln!(o, "{:>4}  {:<16} {:<8} {:<9} {}", s(&cron["id"]), s(&cron["expression"]), s(&cron["kind"]), state, s(&cron["command"]))?;
                }
                Ok(())
            })
        }
        Cmd::Cron(Cron::Run { site, id: cron }) => {
            let id = api.site_id(&site).await?;
            api.run_job(&format!("/api/sites/{id}/cron/{cron}/run"), Value::Null, out).await.map(drop)
        }
        Cmd::Ip(Ip::Block { site, ip, duration, all }) => {
            let id = api.site_id(&site).await?;
            let scope = if all { "all" } else { "site" };
            api.post(&format!("/api/sites/{id}/blocks"), json!({ "ip": ip, "duration": duration, "scope": scope })).await?;
            writeln!(out, "{ip} bloccato ({duration}, {})", if all { "tutti i siti" } else { "questo sito" })?;
            Ok(())
        }
    }
}

fn access_line(e: &Value) -> String {
    format!("{} {} {} {} {} {}ms", s(&e["ts"]), s(&e["ip"]), s(&e["method"]), s(&e["uri"]), s(&e["status"]), s(&e["duration_ms"]))
}
