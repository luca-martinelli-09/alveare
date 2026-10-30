mod api;
mod archive;
mod auth;
mod backup;
mod budget;
mod cloudflare;
mod config;
mod crons;
mod database;
mod files;
mod flows;
mod host;
mod importer;
mod jobs;
mod logs;
mod mailer;
mod metrics;
mod monitor;
mod scheduler;
mod secrets;
mod store;
mod template;
mod ui;
mod upgrade;
mod wordpress;
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
use crate::flows::{Ctx, Host};
use crate::host::{agent_call, caddy_load, write_php_files, Paths, SystemHost};
use crate::store::{OverrideKind, Store};

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
    let panel_dir = std::path::Path::new("/srv/panel");
    std::fs::create_dir_all(panel_dir)?;
    let upgraded = upgrade::before_migrations(panel_dir, upgrade::VERSION, chrono::Utc::now().timestamp())?;
    if let Some(copy) = &upgraded {
        tracing::info!("nuova versione {}: copia di panel.db in {}", upgrade::VERSION, copy.display());
    }
    let store = Store::open(&panel_dir.join("panel.db")).await?;
    let auth = Auth::new(store.pool().clone(), cfg.key.clone());
    bootstrap_admin(&auth, cfg.admin_email.as_deref()).await?;
    write_state_to_disk(&store, &paths, &cfg).await?;

    let mysql = MySqlPoolOptions::new().max_connections(4).connect_lazy(&cfg.mysql_url)?;
    let host = SystemHost {
        paths: paths.clone(),
        mysql,
        panel_host: cfg.panel_host.clone(),
        adminer_host: cfg.adminer_host.clone(),
        db_host: cfg.db_host.clone(),
        root_password: cfg.root_password.clone(),
        redis_addr: cfg.redis_addr.clone(),
        redis_admin_password: cfg.redis_admin_password.clone(),
        caddy_http: "caddy:80".into(),
        telegram: cfg.telegram.clone(),
        http: cloudflare::http_client(),
        cloudflare: cfg.cloudflare.clone(),
        cloudflare_api: cloudflare::API.into(),
        php_runtimes: cfg.php_runtimes.clone(),
        site_runtimes: std::sync::Mutex::new(store.php_sites().await?.into_iter().map(|s| (s.slug.as_str().to_string(), s.runtime)).collect()),
    };
    let ctx = Ctx {
        store: store.clone(),
        host,
        key: cfg.key.clone(),
        db_host: cfg.db_host.clone(),
        global_env: cfg.site_env.clone(),
        backups: cfg.restic_repository.is_some(),
    };
    let app = App::new(ctx, auth, paths.templates.clone(), cfg.secure_cookie)
        .with_adminer(cfg.adminer_host.clone())
        .with_disk_alert(cfg.disk_alert_mb)
        .with_mailer(cfg.smtp.clone().map_or(mailer::Mailer::None, mailer::Mailer::Smtp), cfg.panel_url.clone())
        .with_metrics_token(cfg.metrics_token.clone())
        .with_php_runtimes(&cfg.php_runtimes)
        .with_passkeys(cfg.panel_url.as_deref().map(auth::passkeys::Passkeys::new).transpose()?)
        .with_reserved_hosts(cfg.panel_host.iter().chain(&cfg.adminer_host).cloned().collect())
        .with_recovery(cfg.recovery.clone());
    remind_recovery_kit(&app).await?;
    upgrade::mark_started(panel_dir, upgrade::VERSION)?;
    if upgraded.is_some() && app.ctx.backups {
        // Backup restic dello stato appena migrato: con la copia locale, il punto di ritorno.
        let ctx = app.ctx.clone();
        tokio::spawn(async move {
            let env = backup::panel_env(std::env::vars());
            match ctx.host.backup_panel(&ctx.store, &ctx.key.encrypt(&env)).await {
                Ok(s) => tracing::info!("backup del pannello dopo l'aggiornamento: {}", s.snapshot_id),
                Err(e) => tracing::error!("backup del pannello dopo l'aggiornamento: {e:#}"),
            }
        });
    }
    tokio::spawn(apply_when_ready(app.clone()));
    tokio::spawn(scheduler::run(app.clone()));

    let router = api::router(app).route("/healthz", get(|| async { "ok" })).fallback(ui::serve);
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

/// Il kit va riscaricato ogni volta che una chiave cambia.
async fn remind_recovery_kit(app: &App<SystemHost>) -> Result<()> {
    let current = backup::kit_fingerprint(&app.recovery_kit());
    if app.ctx.store.setting("recovery_kit_fingerprint").await?.as_deref() != Some(current.as_str()) {
        app.ctx.store.raise_alert(None, "recovery-kit", "Scarica il kit di ripristino e conservalo fuori dal server: le chiavi sono nuove o cambiate.").await?;
    }
    Ok(())
}

/// Tutto si rigenera dallo stato: passwd, group, pool in staging e Caddyfile.
/// PHP e Caddy partono dopo il pannello e trovano i file pronti.
async fn write_state_to_disk(store: &Store, paths: &Paths, cfg: &Config) -> Result<()> {
    write_php_files(paths, &store.php_sites().await?, &store.pooled_sites().await?, &cfg.php_runtimes)?;
    let redis_users = flows::redis_users(store, &cfg.key).await?;
    host::write_redis_acl(paths, &alveare_core::redis::sha256_hex(&cfg.redis_admin_password), &redis_users)?;
    std::fs::create_dir_all(&paths.caddy)?;
    host::create_adminer_dirs(paths, Some(alveare_core::php::ADMINER_UID))?;
    let caddyfile = render_caddyfile(&CaddyConfig {
        panel_host: cfg.panel_host.clone(),
        adminer_host: cfg.adminer_host.clone(),
        sites: store.caddy_sites().await?,
        global_extra: store.current_override(None, OverrideKind::Caddy).await?.map(|o| o.content),
    });
    let php_global = match store.current_override(None, OverrideKind::Php).await? {
        Some(o) => alveare_core::php::parse_global_ini(&o.content).unwrap_or_default().join("\n"),
        None => String::new(),
    };
    std::fs::create_dir_all(&paths.php_conf)?;
    std::fs::write(paths.php_conf.join("zz-custom.ini"), format!("; Override globale dal pannello Alveare: non modificare a mano.\n{php_global}\n"))?;
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
