mod api;
mod auth;
mod config;
mod flows;
mod host;
mod jobs;
mod secrets;
mod store;
mod template;
#[cfg(test)]
mod testing;


use std::time::Duration;

use alveare_core::agent::Command;
use alveare_core::caddy::{render_caddyfile, CaddyConfig};
use anyhow::{bail, Context, Result};
use axum::routing::get;
use sqlx::mysql::MySqlPoolOptions;

use crate::api::App;
use crate::auth::Auth;
use crate::config::Config;
use crate::flows::Ctx;
use crate::host::{agent_call, caddy_load, write_php_files, Paths, SystemHost};
use crate::store::Store;

const HEALTH_ADDR: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck().await;
    }
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
    let cfg = Config::from_env()?;
    // Crea cartelle e file di proprietà degli utenti dei siti: serve root.
    if unsafe { libc::geteuid() } != 0 {
        bail!("il pannello deve girare come root nel suo container");
    }
    let paths = Paths::production();
    std::fs::create_dir_all("/srv/panel")?;
    let store = Store::open(std::path::Path::new("/srv/panel/panel.db")).await?;
    let auth = Auth::new(store.pool().clone(), cfg.key.clone());
    bootstrap_admin(&auth, cfg.admin_email.as_deref()).await?;
    write_state_to_disk(&store, &paths, &cfg).await?;

    let mysql = MySqlPoolOptions::new().max_connections(4).connect_lazy(&cfg.mysql_url)?;
    let host = SystemHost { paths: paths.clone(), mysql, panel_host: cfg.panel_host.clone() };
    let ctx = Ctx { store: store.clone(), host, key: cfg.key.clone(), db_host: cfg.db_host.clone(), global_env: cfg.site_env.clone() };
    let app = App::new(ctx, auth, paths.templates.clone(), cfg.secure_cookie);
    tokio::spawn(apply_when_ready(app.clone()));

    let router = api::router(app).route("/healthz", get(|| async { "ok" }));
    let listener = tokio::net::TcpListener::bind(&cfg.listen).await?;
    tracing::info!("pannello in ascolto su {}", cfg.listen);
    axum::serve(listener, router).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

async fn shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("handler SIGTERM");
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

/// Primo avvio: crea l'admin da ALVEARE_ADMIN_EMAIL e stampa una volta la password temporanea.
async fn bootstrap_admin(auth: &Auth, email: Option<&str>) -> Result<()> {
    if auth.user_count().await? > 0 {
        return Ok(());
    }
    let Some(email) = email else {
        tracing::warn!("nessun utente: imposta ALVEARE_ADMIN_EMAIL e riavvia per creare il primo admin");
        return Ok(());
    };
    let (_, temp) = auth.create_user(email, "Amministratore", true).await?;
    tracing::warn!("creato l'admin {email} con password temporanea {temp} (va cambiata al primo accesso)");
    Ok(())
}

/// Tutto si rigenera dallo stato: passwd, group, pool in staging e Caddyfile.
/// PHP e Caddy partono dopo il pannello e trovano i file pronti.
async fn write_state_to_disk(store: &Store, paths: &Paths, cfg: &Config) -> Result<()> {
    write_php_files(paths, &store.php_sites().await?, &store.pooled_sites().await?)?;
    std::fs::create_dir_all(&paths.caddy)?;
    let caddyfile = render_caddyfile(&CaddyConfig { panel_host: cfg.panel_host.clone(), sites: store.caddy_sites().await? });
    std::fs::write(paths.caddy.join("Caddyfile"), caddyfile).context("scrittura del Caddyfile")
}

/// Dopo un riavvio del solo pannello, PHP e Caddy girano già: applica lo stato appena possibile.
async fn apply_when_ready(app: App<SystemHost>) {
    let paths = &app.ctx.host.paths;
    for attempt in 1..=60 {
        let pools = agent_call(&paths.agent, &Command::PoolApply, Duration::from_secs(60)).await;
        let caddy = match std::fs::read_to_string(paths.caddy.join("Caddyfile")) {
            Ok(text) => caddy_load(&paths.caddy_admin, &text).await,
            Err(e) => Err(e.into()),
        };
        match (pools, caddy) {
            (Ok(_), Ok(())) => {
                tracing::info!("stato applicato a PHP e Caddy");
                return;
            }
            (p, c) if attempt % 6 == 0 => tracing::warn!("PHP o Caddy non ancora pronti: {:?} / {:?}", p.err(), c.err()),
            _ => {}
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    tracing::error!("PHP e Caddy non raggiungibili dopo 5 minuti");
}

/// Per l'healthcheck del container, senza curl nell'immagine.
async fn healthcheck() -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(HEALTH_ADDR).await?;
    s.write_all(b"GET /healthz HTTP/1.0\r\n\r\n").await?;
    let mut resp = String::new();
    s.read_to_string(&mut resp).await?;
    if resp.starts_with("HTTP/1.0 200") || resp.starts_with("HTTP/1.1 200") { Ok(()) } else { bail!("healthcheck fallito") }
}
