//! Flussi lunghi (crea, elimina sito). Ogni passo con effetti fuori dal pannello registra
//! come annullarlo; in caso di errore si annulla in ordine inverso e si rigenera tutto dallo stato.
use alveare_core::caddy::CaddySite;
use alveare_core::php::SitePhp;
use alveare_core::redis::{sha256_hex, RedisUser};
use alveare_core::site::{Domain, Slug};
use anyhow::{bail, Context, Result};
use rand::distr::{Alphanumeric, SampleString};

use std::pin::Pin;

use tokio::io::AsyncRead;

use crate::backup::{Retention, Snapshot};
use crate::database::TableInfo;
use crate::secrets::{random_password, SecretKey};
use crate::store::{NewSite, Override, OverrideKind, Site, SiteKind, SiteStatus, Store};
use crate::template::Vars;

const WP_SALT_KEYS: [&str; 8] =
    ["AUTH_KEY", "SECURE_AUTH_KEY", "LOGGED_IN_KEY", "NONCE_KEY", "AUTH_SALT", "SECURE_AUTH_SALT", "LOGGED_IN_SALT", "NONCE_SALT"];

pub struct DbCreds {
    pub name: String,
    pub user: String,
    pub password: String,
}

/// Effetti fuori dal pannello: database, file, PHP, Caddy.
pub trait Host: Send + Sync + 'static {
    fn create_database(&self, db: &DbCreds) -> impl Future<Output = Result<()>> + Send;
    fn drop_database(&self, name: &str, user: &str) -> impl Future<Output = Result<()>> + Send;
    fn create_site_dirs(&self, slug: &Slug, uid: u32) -> impl Future<Output = Result<()>> + Send;
    fn remove_site_dirs(&self, slug: &Slug) -> impl Future<Output = Result<()>> + Send;
    fn trash_site_dirs(&self, slug: &Slug) -> impl Future<Output = Result<()>> + Send;
    /// Copia il template, scrive il `.env` e dice cosa resta da fare (dump, comandi WP-CLI).
    fn install_template(&self, slug: &Slug, uid: u32, template: &str, vars: &Vars, env: &Vars) -> impl Future<Output = Result<Installed>> + Send;
    /// Rigenera passwd, group e pool; restituisce i file di pool rifiutati.
    fn sync_php(&self, users: &[SitePhp], pools: &[SitePhp]) -> impl Future<Output = Result<Vec<String>>> + Send;
    /// Carica il Caddyfile dei siti (con l'override globale) via Admin API: tutto o niente.
    fn sync_caddy(&self, sites: &[CaddySite], global: Option<&str>) -> impl Future<Output = Result<()>> + Send;
    /// `POST /adapt`: il JSON di un Caddyfile, senza caricarlo.
    fn caddy_adapt(&self, caddyfile: &str) -> impl Future<Output = Result<serde_json::Value>> + Send;
    /// Scrive l'override globale di PHP (conf.d/zz-custom.ini); il reload lo fa `sync_php`.
    fn write_php_global(&self, ini: &str) -> impl Future<Output = Result<()>> + Send;
    fn wp(&self, slug: &Slug, args: &[String]) -> impl Future<Output = Result<String>> + Send;
    /// Come `wp`, ma un'uscita con errore non è un errore: (riuscito, output).
    fn wp_output(&self, slug: &Slug, args: &[String]) -> impl Future<Output = Result<(bool, String)>> + Send;
    /// Fa riaprire a PHP-FPM i log dopo la rotazione.
    fn reopen_fpm_logs(&self) -> impl Future<Output = Result<()>> + Send;
    fn db_tables(&self, name: &str) -> impl Future<Output = Result<Vec<TableInfo>>> + Send;
    /// Dump SQL in streaming; `finished` dice se mariadb-dump è arrivato in fondo.
    fn db_dump(&self, name: &str, tables: &[String]) -> impl Future<Output = Result<Dump>> + Send;
    fn db_import(&self, name: &str, sql: Pin<Box<dyn AsyncRead + Send>>) -> impl Future<Output = Result<()>> + Send;
    fn set_db_password(&self, user: &str, password: &str) -> impl Future<Output = Result<()>> + Send;
    /// Dump del database + cartella del sito in uno snapshot restic.
    fn backup(&self, slug: &Slug, db_name: &str) -> impl Future<Output = Result<Snapshot>> + Send;
    fn prune(&self, slug: &Slug, keep: Retention) -> impl Future<Output = Result<()>> + Send;
    /// Snapshot di siti eliminati: restano 30 giorni.
    fn expire_deleted(&self, slug: &str) -> impl Future<Output = Result<()>> + Send;
    /// Ripristina file e/o database dello snapshot di `slug` nel sito `target` (lo stesso, o una
    /// copia su un nuovo dominio). Ciò che viene sostituito finisce nel cestino.
    fn restore(&self, slug: &Slug, snapshot: &str, target: &Slug, target_uid: u32, files: &FilesScope, db: Option<&DbCreds>) -> impl Future<Output = Result<()>> + Send;
    /// Contenuto di una cartella del sito (relativa) dentro lo snapshot.
    fn snapshot_ls(&self, slug: &Slug, snapshot: &str, rel: &str) -> impl Future<Output = Result<Vec<crate::backup::SnapEntry>>> + Send;
    /// panel.db, template e configurazione cifrata, con il tag `panel`.
    fn backup_panel(&self, store: &Store, env_enc: &[u8]) -> impl Future<Output = Result<Snapshot>> + Send;
    /// Riscrive l'ACL di Redis e la fa ricaricare.
    fn sync_redis(&self, users: &[RedisUser]) -> impl Future<Output = Result<()>> + Send;
    /// Token monouso per aprire Adminer già autenticato su questo database.
    fn adminer_grant(&self, db: &DbCreds) -> impl Future<Output = Result<String>> + Send;
    /// Copia i file di un sito in un altro (tranne `.env` e link), assegnandoli al nuovo utente.
    /// Estrae l'archivio di un sito esistente in `public/` e dice cosa contiene.
    fn import_files(&self, slug: &Slug, uid: u32, archive: &std::path::Path) -> impl Future<Output = Result<Imported>> + Send;
    fn copy_site_files(&self, from: &Slug, to: &Slug, uid: u32) -> impl Future<Output = Result<()>> + Send;
    fn read_site_env(&self, slug: &Slug) -> impl Future<Output = Result<Vars>> + Send;
    fn write_site_env(&self, slug: &Slug, uid: u32, env: &Vars) -> impl Future<Output = Result<()>> + Send;
    /// tar.gz con i file del sito (senza `.env`), `.env.example`, `database.sql` e `LEGGIMI.txt`;
    /// restituisce la dimensione.
    fn export_site(&self, slug: &Slug, db_name: &str, dest: &std::path::Path, env_example: &str, readme: &str) -> impl Future<Output = Result<u64>> + Send;
    /// Crea `templates/<name>/` con i file del sito (senza `.env`), il dump in `db.sql` e il manifest.
    /// `sql_file` è nella cartella tmp del sito, dove l'ha scritto WP-CLI.
    fn store_template(&self, slug: &Slug, name: &Slug, manifest: &crate::template::Manifest, sql_file: &str) -> impl Future<Output = Result<()>> + Send;
    /// Etichette degli account Cloudflare configurati (vuoto: hostname da aggiungere a mano).
    fn cloudflare_accounts(&self) -> Vec<String>;
    /// Hostname sul tunnel dell'account (ingress verso Caddy + CNAME proxato).
    fn tunnel_add(&self, account: &str, host: &Domain) -> impl Future<Output = Result<()>> + Send;
    fn tunnel_remove(&self, account: &str, host: &Domain) -> impl Future<Output = Result<()>> + Send;
    /// Applica il modello di regole WAF e rate limit alla zona dell'hostname; quante regole.
    fn cloudflare_rules(&self, account: &str, host: &Domain, template: &serde_json::Value) -> impl Future<Output = Result<usize>> + Send;
    /// Messaggio sul canale degli avvisi (Telegram); `Ok(false)` se il canale non è configurato.
    fn notify(&self, text: &str) -> impl Future<Output = Result<bool>> + Send;
    /// Byte occupati dalla cartella del sito.
    fn site_disk_usage(&self, slug: &Slug) -> impl Future<Output = Result<u64>> + Send;
    /// GET / di un sito attraverso Caddy (con la password dello staging se serve): lo stato HTTP.
    fn probe(&self, host: &Domain, auth: Option<(&str, &str)>) -> impl Future<Output = Result<u16>> + Send;
    /// GET di una pagina del sito attraverso Caddy: stato e inizio della risposta.
    fn http_get(&self, host: &Domain, path: &str, auth: Option<(&str, &str)>, timeout_s: u32) -> impl Future<Output = Result<(u16, String)>> + Send;
    /// Uno script PHP del sito con la CLI (anche artisan): (riuscito, output).
    fn php(&self, slug: &Slug, args: &[String], timeout_s: u32) -> impl Future<Output = Result<(bool, String)>> + Send;
    /// Riscrive una chiave del .env del sito.
    fn set_site_env(&self, slug: &Slug, uid: u32, key: &str, value: &str) -> impl Future<Output = Result<()>> + Send;
}

#[derive(Debug, Default, Clone)]
pub struct Imported {
    /// `$table_prefix` letto dal vecchio wp-config.php, se c'era.
    pub table_prefix: Option<String>,
    /// Se mancano wp-includes e wp-admin, il core va scaricato.
    pub has_core: bool,
}

/// Quali file ripristinare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesScope {
    None,
    All,
    /// Un file o una cartella, relativi alla cartella del sito.
    Path(String),
}

impl std::fmt::Display for FilesScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::All => f.write_str("all"),
            Self::Path(p) => f.write_str(p),
        }
    }
}

pub struct Installed {
    pub commands: Vec<Vec<String>>,
    /// Dump di un template snapshot, con il suo dominio fittizio.
    pub sql: Option<(std::path::PathBuf, String)>,
}

pub struct Dump {
    pub reader: Pin<Box<dyn AsyncRead + Send>>,
    pub finished: Pin<Box<dyn Future<Output = Result<()>> + Send>>,
}

pub struct Ctx<H> {
    pub store: Store,
    pub host: H,
    pub key: SecretKey,
    pub db_host: String,
    /// Valori comuni a tutti i siti (SMTP di SES).
    pub global_env: Vars,
    /// Repository restic configurato: senza, niente backup (e l'eliminazione lo dice nel log).
    pub backups: bool,
}

#[derive(Clone)]
pub struct CreateSite {
    pub domain: Domain,
    pub kind: SiteKind,
    pub template: String,
    pub title: String,
    pub admin_email: String,
    pub table_prefix: String,
}

/// Log di un job; con `live` ogni riga è subito visibile a chi legge il job in corso.
#[derive(Debug, Default)]
pub struct JobLog {
    pub job_id: i64,
    pub lines: Vec<String>,
    pub live: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
}

impl JobLog {
    pub fn push(&mut self, line: impl Into<String>) {
        let line = line.into();
        if let Some(live) = &self.live {
            live.lock().expect("lock del log").push(line.clone());
        }
        self.lines.push(line);
    }
}

enum Undo {
    Row(i64),
    Database { name: String, user: String },
    Dirs(Slug),
}

impl<H: Host> Ctx<H> {
    pub async fn db_creds(&self, site_id: i64) -> Result<DbCreds> {
        let (name, user, enc) = self.store.db_creds(site_id).await?;
        Ok(DbCreds { name, user, password: self.key.decrypt(&enc)? })
    }

    async fn sync_php(&self) -> Result<Vec<String>> {
        self.host.sync_php(&self.store.php_sites().await?, &self.store.pooled_sites().await?).await
    }

    async fn sync_caddy(&self) -> Result<()> {
        sync_caddy(self).await
    }

    async fn sync_redis(&self) -> Result<()> {
        self.host.sync_redis(&redis_users(&self.store, &self.key).await?).await
    }
}

/// Account Cloudflare del sito: quello scelto o il primo configurato.
pub async fn cloudflare_account<H: Host>(c: &Ctx<H>, site_id: i64) -> Result<Option<String>> {
    let accounts = c.host.cloudflare_accounts();
    let chosen = c.store.cloudflare_account(site_id).await?;
    Ok(chosen.filter(|a| accounts.contains(a)).or_else(|| accounts.first().cloned()))
}

/// Pubblica (o toglie) gli hostname sul tunnel Cloudflare. Un errore non ferma il flusso:
/// finisce nel log e in un avviso, e l'hostname si sistema a mano o con "Sincronizza".
pub async fn tunnel_hostnames<H: Host>(c: &Ctx<H>, site_id: i64, hosts: &[Domain], add: bool, log: &mut JobLog) -> Result<()> {
    let Some(account) = cloudflare_account(c, site_id).await? else {
        if add {
            let list: Vec<&str> = hosts.iter().map(Domain::as_str).collect();
            log.push(format!("aggiungi al tunnel Cloudflare verso {}: {}", crate::cloudflare::SERVICE, list.join(", ")));
        }
        return Ok(());
    };
    let mut failed = Vec::new();
    for host in hosts {
        let done = if add { c.host.tunnel_add(&account, host).await } else { c.host.tunnel_remove(&account, host).await };
        match done {
            Ok(()) => log.push(format!("Cloudflare ({account}): {} {}", host.as_str(), if add { "pubblicato sul tunnel" } else { "tolto dal tunnel" })),
            Err(e) => {
                log.push(format!("Cloudflare ({account}): {} non sistemato: {e:#}", host.as_str()));
                failed.push(format!("{}: {e:#}", host.as_str()));
            }
        }
    }
    if failed.is_empty() {
        c.store.resolve_alerts(site_id, "cloudflare").await?;
    } else {
        c.store.raise_alert(Some(site_id), "cloudflare", &format!("hostname da sistemare sul tunnel:\n{}", failed.join("\n"))).await?;
    }
    Ok(())
}

pub async fn site_hostnames<H>(c: &Ctx<H>, site_id: i64) -> Result<Vec<Domain>> {
    Ok(c.store.domains(site_id).await?.into_iter().map(|(d, _)| d).collect())
}

/// Rigenera e ricarica il Caddyfile dallo stato.
pub async fn sync_caddy<H: Host>(c: &Ctx<H>) -> Result<()> {
    let global = c.store.current_override(None, OverrideKind::Caddy).await?.map(|o| o.content);
    c.host.sync_caddy(&c.store.caddy_sites().await?, global.as_deref()).await
}

/// Esegue un cron personalizzato e ne registra l'esito; un fallimento apre un avviso, il primo
/// successo lo chiude. `Ok(false)` vuol dire che il comando è fallito, non il pannello.
pub async fn run_cron<H: Host>(c: &Ctx<H>, cron_id: i64) -> Result<bool> {
    let cron = c.store.cron(cron_id).await?;
    let site = c.store.site(cron.site_id).await?;
    let domains: Vec<Domain> = c.store.domains(site.id).await?.into_iter().map(|(d, _)| d).collect();
    let run = c.store.cron_started(cron_id).await?;
    let kind = cron.kind.parse::<crate::crons::CronKind>().map_err(Invalid)?;
    let outcome = match crate::crons::prepare(kind, &cron.command, &domains) {
        Err(e) => Ok((false, e)),
        Ok(crate::crons::Action::Wp(args)) => c.host.wp_output(&site.slug, &args).await,
        Ok(crate::crons::Action::Php(args)) => c.host.php(&site.slug, &args, cron.timeout_s).await,
        Ok(crate::crons::Action::Http { host, path }) => {
            let password = staging_password(c, site.id).await?;
            let auth = password.as_deref().map(|p| ("staging", p));
            c.host.http_get(&host, &path, auth, cron.timeout_s).await.map(|(status, body)| ((200..400).contains(&status), format!("HTTP {status}\n{body}")))
        }
    };
    let (ok, output) = outcome.unwrap_or_else(|e| (false, format!("{e:#}")));
    c.store.cron_finished(run, ok, &output).await?;
    let alert = format!("cron-{cron_id}");
    if ok {
        c.store.resolve_alerts(site.id, &alert).await?;
    } else {
        let tail: String = output.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        c.store.raise_alert(Some(site.id), &alert, &format!("cron \"{}\" fallito:\n{tail}", cron.command)).await?;
    }
    Ok(ok)
}

/// Manda gli avvisi nuovi e le risoluzioni sul canale configurato, una volta sola ciascuno.
pub async fn dispatch_alerts<H: Host>(c: &Ctx<H>) -> Result<usize> {
    let mut sent = 0;
    for (alert, resolved) in c.store.alerts_to_notify().await? {
        let site = match alert.site_id {
            Some(id) => c.store.domains(id).await?.into_iter().next().map(|(d, _)| d.as_str().to_string()).unwrap_or_else(|| format!("sito {id}")),
            None => "pannello".into(),
        };
        let text = if resolved { format!("✅ {site} · {}: risolto", alert.kind) } else { format!("⚠️ {site} · {}\n{}", alert.kind, alert.message) };
        if c.host.notify(&text).await? {
            sent += 1;
        }
        c.store.mark_notified(alert.id, resolved).await?;
    }
    Ok(sent)
}

/// Una modifica rifiutata dalla validazione: la UI la mostra così com'è (400).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

/// Direttive PHP extra di un sito (admin): validate, provate con `php-fpm -t`, tolte se FPM le rifiuta.
pub async fn set_php_override<H: Host>(c: &Ctx<H>, site_id: i64, content: &str, author: Option<i64>) -> Result<Override> {
    let site = c.store.site(site_id).await?;
    alveare_core::php::parse_extra(content).map_err(Invalid)?;
    let saved = c.store.add_override(Some(site_id), OverrideKind::Php, content, author).await?;
    if let Err(e) = check_own_pool(c, &site).await {
        c.store.remove_override(saved.id).await?;
        c.sync_php().await?;
        return Err(Invalid(format!("php-fpm ha rifiutato le direttive: {e:#}")).into());
    }
    Ok(saved)
}

/// Impostazioni PHP dal form: se FPM rifiuta il pool, tornano quelle di prima.
pub async fn set_php_settings<H: Host>(c: &Ctx<H>, site_id: i64, settings: &crate::store::PhpSettingsRow) -> Result<()> {
    let site = c.store.site(site_id).await?;
    crate::budget::validate(settings).map_err(Invalid)?;
    c.store.set_php_settings(site_id, settings).await?;
    if let Err(e) = check_own_pool(c, &site).await {
        c.store.set_php_settings(site_id, &site.settings).await?;
        c.sync_php().await?;
        return Err(Invalid(format!("php-fpm ha rifiutato le impostazioni: {e:#}")).into());
    }
    Ok(())
}

/// Budget di memoria con le impostazioni di un sito sostituite da `proposed`.
/// `site_id = 0` e `proposed` aggiungono un sito nuovo con quelle impostazioni.
pub async fn budget_with<H>(c: &Ctx<H>, site_id: i64, proposed: Option<&crate::store::PhpSettingsRow>, ram_mb: u64) -> Result<crate::budget::Budget> {
    let sites = c.store.sites().await?;
    let new_site = (site_id == 0).then_some(proposed).flatten().cloned();
    let rows: Vec<crate::store::PhpSettingsRow> = sites
        .iter()
        .filter(|s| !matches!(s.status, SiteStatus::Suspended | SiteStatus::Deleting))
        .map(|s| if s.id == site_id { proposed.cloned().unwrap_or_else(|| s.settings.clone()) } else { s.settings.clone() })
        .chain(new_site)
        .collect();
    Ok(crate::budget::compute(&rows, ram_mb))
}

/// Avviso globale "budget" quando il massimo teorico dei pool supera la RAM.
pub async fn budget_alert<H>(c: &Ctx<H>, ram_mb: u64) -> Result<()> {
    let b = budget_with(c, -1, None, ram_mb).await?;
    if b.over_ram() {
        c.store.raise_alert(None, "budget", &format!("il massimo teorico dei pool è il {}% della RAM ({} MB su {} MB)", b.percent, b.total_mb, b.ram_mb)).await?;
    } else {
        c.store.resolve_global_alerts("budget").await?;
    }
    Ok(())
}

/// Override globale di PHP: scritto in conf.d e attivato con il reload di FPM.
pub async fn set_php_global<H: Host>(c: &Ctx<H>, content: &str, author: Option<i64>) -> Result<Override> {
    let lines = alveare_core::php::parse_global_ini(content).map_err(Invalid)?;
    let previous = c.store.current_override(None, OverrideKind::Php).await?;
    let saved = c.store.add_override(None, OverrideKind::Php, content, author).await?;
    let applied = async {
        c.host.write_php_global(&lines.join("\n")).await?;
        c.sync_php().await
    }
    .await;
    if let Err(e) = applied {
        c.store.remove_override(saved.id).await?;
        let old = previous.map(|o| alveare_core::php::parse_global_ini(&o.content).unwrap_or_default().join("\n")).unwrap_or_default();
        c.host.write_php_global(&old).await?;
        c.sync_php().await?;
        return Err(Invalid(format!("PHP non ha accettato la configurazione: {e:#}")).into());
    }
    Ok(saved)
}

/// Override di Caddy, del sito o globale: `/adapt` per la sintassi, controllo del JSON per
/// l'isolamento, poi `/load`. Se qualcosa non va la versione salvata si toglie e non resta nulla.
pub async fn set_caddy_override<H: Host>(c: &Ctx<H>, site_id: Option<i64>, content: &str, author: Option<i64>) -> Result<Override> {
    let site = match site_id {
        Some(id) => Some(c.store.site(id).await?),
        None => None,
    };
    let saved = c.store.add_override(site_id, OverrideKind::Caddy, content, author).await?;
    let checked = async {
        let global = c.store.current_override(None, OverrideKind::Caddy).await?.map(|o| o.content);
        let sites: Vec<CaddySite> = match &site {
            Some(s) => c.store.caddy_sites().await?.into_iter().filter(|x| x.slug == s.slug).map(|x| CaddySite { suspended: false, ..x }).collect(),
            None => vec![],
        };
        let text = alveare_core::caddy::render_caddyfile(&alveare_core::caddy::CaddyConfig { panel_host: None, adminer_host: None, sites, global_extra: global });
        let json = c.host.caddy_adapt(&text).await.map_err(|e| Invalid(format!("Caddy: {e:#}")))?;
        let violations = alveare_core::caddy::adapted_violations(&json, site.as_ref().map(|s| &s.slug));
        if !violations.is_empty() {
            return Err(Invalid(format!("override rifiutato, romperebbe l'isolamento: {}", violations.join("; "))).into());
        }
        sync_caddy(c).await.map_err(|e| anyhow::Error::from(Invalid(format!("Caddy ha rifiutato la configurazione: {e:#}"))))
    }
    .await;
    if let Err(e) = checked {
        c.store.remove_override(saved.id).await?;
        let _ = sync_caddy(c).await;
        return Err(e);
    }
    Ok(saved)
}

pub async fn redis_users(store: &Store, key: &SecretKey) -> Result<Vec<RedisUser>> {
    let mut users = Vec::new();
    for (slug, enc) in store.redis_users().await? {
        users.push(RedisUser { slug, password_sha256: sha256_hex(&key.decrypt(&enc)?) });
    }
    Ok(users)
}

fn random_salt() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 64)
}

/// (variabili del template, contenuto del `.env`)
fn site_vars<H>(c: &Ctx<H>, req: &CreateSite, site: &Site, db: &DbCreds, redis_password: &str) -> (Vars, Vars) {
    let url = format!("https://{}", req.domain.as_str());
    let mut env = c.global_env.clone();
    env.extend([
        ("DB_NAME".into(), db.name.clone()),
        ("DB_USER".into(), db.user.clone()),
        ("DB_PASSWORD".into(), db.password.clone()),
        ("DB_HOST".into(), c.db_host.clone()),
        ("TABLE_PREFIX".into(), req.table_prefix.clone()),
        ("WP_HOME".into(), url.clone()),
        ("WP_SITEURL".into(), url.clone()),
        ("WP_ENVIRONMENT_TYPE".into(), "production".into()),
        ("ALVEARE_UPDATE_RULE".into(), "security".into()),
        ("REDIS_HOST".into(), "redis".into()),
        ("REDIS_PORT".into(), "6379".into()),
        ("REDIS_USERNAME".into(), site.slug.as_str().into()),
        ("REDIS_PASSWORD".into(), redis_password.into()),
        ("REDIS_PREFIX".into(), format!("{}:", site.slug.as_str())),
        ("SMTP_FROM".into(), format!("noreply@{}", req.domain.as_str())),
        // Il .env usa apici singoli: "Pro Loco d'Arco" diventa "Pro Loco d’Arco".
        ("SMTP_FROM_NAME".into(), req.title.replace('\'', "’").replace(['\n', '\r'], " ")),
    ]);
    env.extend(WP_SALT_KEYS.map(|k| (k.to_string(), random_salt())));
    let mut vars = env.clone();
    vars.extend([
        ("DOMAIN".into(), req.domain.as_str().to_string()),
        ("SITE_URL".into(), url),
        ("SLUG".into(), site.slug.as_str().to_string()),
        ("TITLE".into(), req.title.clone()),
        ("ADMIN_USER".into(), "gestore".into()),
        ("ADMIN_EMAIL".into(), req.admin_email.clone()),
        ("ADMIN_PASSWORD".into(), random_password()),
    ]);
    (vars, env)
}

pub async fn create_site<H: Host>(c: &Ctx<H>, req: &CreateSite, log: &mut JobLog) -> Result<Site> {
    let site = c
        .store
        .create_site(&NewSite { primary: req.domain.clone(), kind: req.kind, template: req.template.clone() })
        .await?;
    log.push(format!("sito {} registrato", site.slug.as_str()));
    let mut undo = vec![Undo::Row(site.id)];
    match create_steps(c, req, &site, &mut undo, log).await {
        Ok(()) => Ok(c.store.site(site.id).await?),
        Err(e) => {
            log.push(format!("errore: {e:#}"));
            rollback(c, undo, log).await;
            Err(e)
        }
    }
}

async fn create_steps<H: Host>(c: &Ctx<H>, req: &CreateSite, site: &Site, undo: &mut Vec<Undo>, log: &mut JobLog) -> Result<()> {
    let slug = &site.slug;
    let db = new_database(c, site, undo, log).await?;
    let redis_password = random_password();
    c.store.save_redis_password(site.id, &c.key.encrypt(&redis_password)).await?;
    let (vars, env) = site_vars(c, req, site, &db, &redis_password);
    let installed = c.host.install_template(slug, site.uid(), &req.template, &vars, &env).await?;
    let commands = installed.commands;
    log.push(format!("template {} installato", req.template));
    c.sync_redis().await?;
    log.push("utente Redis creato");

    check_own_pool(c, site).await?;
    log.push("pool PHP attivo");

    if let Some((sql, fake_domain)) = &installed.sql {
        c.host.db_import(&db.name, open_sql(sql).await?).await.context("import del dump del template")?;
        let args: Vec<String> = ["search-replace", fake_domain, req.domain.as_str(), "--all-tables", "--skip-columns=guid"].map(String::from).to_vec();
        c.host.wp(slug, &args).await?;
        log.push(format!("dump del template importato e portato su {}", req.domain.as_str()));
    }

    for args in &commands {
        c.host.wp(slug, args).await.with_context(|| format!("wp {}", args.first().map_or("", String::as_str)))?;
    }
    if !commands.is_empty() {
        // shortcut: la password admin finisce nel log del job, visibile solo agli admin; da sostituire con l'invito via email (fase 3).
        log.push(format!("WordPress installato: utente {}, password {}", vars["ADMIN_USER"], vars["ADMIN_PASSWORD"]));
    }

    c.store.set_status(site.id, SiteStatus::Active).await?;
    c.sync_caddy().await?;
    log.push("sito attivo");
    tunnel_hostnames(c, site.id, std::slice::from_ref(&req.domain), true, log).await
}

/// Copia un sito su un nuovo dominio: file, database e `wp search-replace` del dominio.
/// La copia nasce protetta come staging, con `WP_ENVIRONMENT_TYPE=staging`.
pub async fn clone_site<H: Host>(c: &Ctx<H>, src_id: i64, domain: &Domain, log: &mut JobLog) -> Result<Site> {
    let src = c.store.site(src_id).await?;
    if !matches!(src.status, SiteStatus::Active | SiteStatus::Suspended) {
        bail!("si clona solo un sito attivo o sospeso");
    }
    let src_domain = c.store.domains(src_id).await?.into_iter().next().map(|(d, _)| d).context("sito senza dominio")?;
    let src_env = c.host.read_site_env(&src.slug).await?;
    let site = c.store.create_site(&NewSite { primary: domain.clone(), kind: src.kind, template: src.template.clone() }).await?;
    log.push(format!("sito {} registrato come copia di {}", site.slug.as_str(), src.slug.as_str()));
    let mut undo = vec![Undo::Row(site.id)];
    let steps = async {
        let db = new_database(c, &site, &mut undo, log).await?;
        c.host.copy_site_files(&src.slug, &site.slug, site.uid()).await?;
        log.push("file copiati");
        configure_copy(c, &site, domain, &db, &src_env).await?;
        let src_db = c.db_creds(src_id).await?;
        let dump = c.host.db_dump(&src_db.name, &[]).await?;
        c.host.db_import(&db.name, dump.reader).await?;
        dump.finished.await?;
        log.push("database copiato");
        publish_copy(c, &site, &src_domain, domain, log).await
    };
    match steps.await {
        Ok(()) => Ok(c.store.site(site.id).await?),
        Err(e) => {
            log.push(format!("errore: {e:#}"));
            rollback(c, undo, log).await;
            Err(e)
        }
    }
}

pub struct ImportSite {
    pub site: CreateSite,
    /// Archivio dei file (zip o tar.gz) e dump SQL (.sql o .sql.gz), nell'area import.
    pub archive: std::path::PathBuf,
    pub dump: std::path::PathBuf,
}

/// Host di un URL ("https://www.x.it/blog" → "www.x.it").
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://")?.1;
    let host = rest.split(['/', ':', '?', '#']).next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Importa un sito esistente: i suoi file, il nostro wp-config.php (legge il .env), il suo dump.
/// Il prefisso delle tabelle viene dal vecchio wp-config.php; se il dominio cambia, search-replace.
pub async fn import_site<H: Host>(c: &Ctx<H>, req: &ImportSite, log: &mut JobLog) -> Result<Site> {
    let r = &req.site;
    let site = c.store.create_site(&NewSite { primary: r.domain.clone(), kind: r.kind, template: r.template.clone() }).await?;
    log.push(format!("sito {} registrato per l'import", site.slug.as_str()));
    let mut undo = vec![Undo::Row(site.id)];
    match import_steps(c, req, &site, &mut undo, log).await {
        Ok(()) => Ok(c.store.site(site.id).await?),
        Err(e) => {
            log.push(format!("errore: {e:#}"));
            rollback(c, undo, log).await;
            Err(e)
        }
    }
}

async fn import_steps<H: Host>(c: &Ctx<H>, req: &ImportSite, site: &Site, undo: &mut Vec<Undo>, log: &mut JobLog) -> Result<()> {
    let slug = &site.slug;
    let db = new_database(c, site, undo, log).await?;
    let imported = c.host.import_files(slug, site.uid(), &req.archive).await?;
    log.push("file estratti");
    let create = CreateSite { table_prefix: imported.table_prefix.clone().unwrap_or_else(|| req.site.table_prefix.clone()), ..req.site.clone() };
    let redis_password = random_password();
    c.store.save_redis_password(site.id, &c.key.encrypt(&redis_password)).await?;
    let (vars, env) = site_vars(c, &create, site, &db, &redis_password);
    c.host.install_template(slug, site.uid(), &create.template, &vars, &env).await?;
    log.push(format!("wp-config.php e .env del pannello installati (prefisso {})", create.table_prefix));
    c.sync_redis().await?;
    check_own_pool(c, site).await?;
    let wordpress = site.kind == SiteKind::Wordpress;
    if wordpress && !imported.has_core {
        c.host.wp(slug, &["core", "download", "--skip-content", "--locale=it_IT"].map(String::from)).await?;
        log.push("core di WordPress scaricato");
    }
    c.host.db_import(&db.name, open_sql(&req.dump).await?).await.context("import del dump")?;
    log.push("dump importato");
    if wordpress {
        let home = c.host.wp(slug, &["option", "get", "home"].map(String::from)).await?;
        let new = create.domain.as_str();
        match url_host(&home) {
            Some(old) if old != new => {
                let (from, to) = (format!("//{old}"), format!("//{new}"));
                c.host.wp(slug, &["search-replace", &from, &to, "--all-tables", "--skip-columns=guid"].map(String::from)).await?;
                log.push(format!("search-replace {from} → {to}"));
            }
            _ => log.push("stesso dominio: nessun search-replace"),
        }
    }
    c.store.set_status(site.id, SiteStatus::Active).await?;
    c.sync_caddy().await?;
    flush_object_cache(c, site, log).await;
    log.push("sito importato");
    tunnel_hostnames(c, site.id, std::slice::from_ref(&create.domain), true, log).await
}

/// Valori del .env che non devono uscire dal server.
fn is_secret(key: &str) -> bool {
    ["PASSWORD", "KEY", "SALT", "SECRET", "TOKEN"].iter().any(|w| key.contains(w))
}

fn export_readme(domain: &str, slug: &str, kind: SiteKind) -> String {
    let wp = kind == SiteKind::Wordpress;
    format!(
        "Esportazione di {domain} dal pannello Alveare ({date})\n\n\
Contenuto\n\
- {slug}/public/      file del sito (document root)\n\
- {slug}/.env.example configurazione, con password e chiavi da impostare (DA_IMPOSTARE)\n\
- {slug}/database.sql dump del database (MariaDB/MySQL, utf8mb4)\n\n\
Requisiti\n\
- PHP 8.5 (va bene anche 8.3 o successivo) con le estensioni {ext}\n\
- MariaDB 10.6+ o MySQL 8+\n\
- un server web con la document root su public/\n\n\
Installazione altrove\n\
1. Crea un database e un utente con tutti i permessi su quel database.\n\
2. Importa il dump: mariadb -u UTENTE -p NOMEDB < database.sql\n\
3. Copia la cartella sul server e rinomina .env.example in .env, accanto a public/.\n\
4. Nel .env imposta DB_NAME, DB_USER, DB_PASSWORD, DB_HOST e i valori DA_IMPOSTARE{salts}.\n\
   Il .env deve stare fuori dalla document root.\n\
{wp_steps}",
        date = chrono::Local::now().format("%d/%m/%Y %H:%M"),
        ext = if wp { "gd o imagick, intl, zip, mysqli, opcache, exif (redis facoltativa)" } else { "richieste dall'applicazione" },
        salts = if wp { " (chiavi e salt nuovi: https://api.wordpress.org/secret-key/1.1/salt/)" } else { "" },
        wp_steps = if wp {
            "5. Se il dominio cambia: wp search-replace '//vecchio.dominio' '//nuovo.dominio' --all-tables --skip-columns=guid\n\
6. Senza Redis, togli wp-content/object-cache.php se presente.\n\
7. Il cron è disattivato nel wp-config.php (DISABLE_WP_CRON): pianifica `wp cron event run --due-now` ogni 5 minuti.\n"
        } else {
            ""
        },
    )
}

/// Esportazione portabile del sito in `dir`, registrata con un token per il download (7 giorni).
pub async fn export_site<H: Host>(c: &Ctx<H>, id: i64, dir: &std::path::Path, now: i64, log: &mut JobLog) -> Result<crate::store::Export> {
    let site = c.store.site(id).await?;
    let domain = c.store.domains(id).await?.into_iter().next().map(|(d, _)| d.as_str().to_string()).unwrap_or_default();
    let mut env = c.host.read_site_env(&site.slug).await?;
    for (k, v) in env.iter_mut() {
        if is_secret(k) {
            *v = "DA_IMPOSTARE".into();
        }
    }
    let env_example = crate::template::env_file(&env)?;
    let readme = export_readme(&domain, site.slug.as_str(), site.kind);
    let db = c.db_creds(id).await?;
    let file = format!("{now}-{}.tar.gz", site.slug.as_str());
    let size = c.host.export_site(&site.slug, &db.name, &dir.join(&file), &env_example, &readme).await?;
    let export = crate::store::Export { token: random_password(), site_id: id, file, size: size as i64, created_at: now };
    c.store.add_export(&export).await?;
    log.push(format!("esportazione pronta: {} ({} byte), scaricabile per 7 giorni", export.file, size));
    Ok(export)
}

/// Salva un sito come template snapshot: file e dump con il dominio portato su `template.local`
/// da `wp search-replace --export`, che rispetta i dati serializzati e non tocca il database vivo.
pub async fn save_template<H: Host>(c: &Ctx<H>, id: i64, name: &Slug, title: &str, now: i64, log: &mut JobLog) -> Result<()> {
    let site = c.store.site(id).await?;
    let domain = c.store.domains(id).await?.into_iter().next().map(|(d, _)| d).context("sito senza dominio")?;
    let env = c.host.read_site_env(&site.slug).await?;
    let sql_file = format!("template-{now}.sql");
    let export = format!("--export={}/{}/{sql_file}", alveare_core::php::TMP_DIR, site.slug.as_str());
    let manifest = crate::template::Manifest {
        name: title.to_string(),
        kind: site.kind,
        render: vec![],
        post_create: vec![],
        sql: Some("db.sql".into()),
        sql_domain: "template.local".into(),
        table_prefix: env.get("TABLE_PREFIX").filter(|p| *p != "wp_").cloned(),
    };
    c.host.wp(&site.slug, &["search-replace", domain.as_str(), &manifest.sql_domain, "--all-tables", "--skip-columns=guid", &export].map(String::from)).await?;
    log.push("dump esportato con il dominio template.local");
    c.host.store_template(&site.slug, name, &manifest, &sql_file).await?;
    log.push(format!("template {} salvato", name.as_str()));
    Ok(())
}

/// `.env` nuovo per una copia (clone o restore su nuovo dominio): credenziali e salt nuovi,
/// prefisso, nome e regola di aggiornamento della sorgente, ambiente `staging`. Poi Redis e pool.
async fn configure_copy<H: Host>(c: &Ctx<H>, site: &Site, domain: &Domain, db: &DbCreds, src_env: &Vars) -> Result<()> {
    let req = CreateSite {
        domain: domain.clone(),
        kind: site.kind,
        template: site.template.clone(),
        title: src_env.get("SMTP_FROM_NAME").cloned().unwrap_or_else(|| domain.as_str().into()),
        admin_email: String::new(),
        table_prefix: src_env.get("TABLE_PREFIX").cloned().unwrap_or_else(|| "wp_".into()),
    };
    let redis_password = random_password();
    c.store.save_redis_password(site.id, &c.key.encrypt(&redis_password)).await?;
    let (_, mut env) = site_vars(c, &req, site, db, &redis_password);
    env.insert("WP_ENVIRONMENT_TYPE".into(), "staging".into());
    if let Some(rule) = src_env.get("ALVEARE_UPDATE_RULE") {
        env.insert("ALVEARE_UPDATE_RULE".into(), rule.clone());
    }
    c.host.write_site_env(&site.slug, site.uid(), &env).await?;
    c.sync_redis().await?;
    check_own_pool(c, site).await
}

/// Porta il database della copia sul nuovo dominio, la protegge e la mette online.
async fn publish_copy<H: Host>(c: &Ctx<H>, site: &Site, src_domain: &Domain, domain: &Domain, log: &mut JobLog) -> Result<()> {
    if site.kind == SiteKind::Wordpress {
        let (from, to) = (format!("//{}", src_domain.as_str()), format!("//{}", domain.as_str()));
        c.host.wp(&site.slug, &["search-replace", &from, &to, "--all-tables", "--skip-columns=guid"].map(String::from)).await?;
        log.push(format!("search-replace {from} → {to}"));
    }
    let password = random_password();
    c.store.set_staging(site.id, Some((&c.key.encrypt(&password), &bcrypt::hash(&password, bcrypt::DEFAULT_COST)?))).await?;
    c.store.set_status(site.id, SiteStatus::Active).await?;
    c.sync_caddy().await?;
    log.push("copia attiva e protetta (utente staging)");
    tunnel_hostnames(c, site.id, std::slice::from_ref(domain), true, log).await
}

/// Database e cartelle di un sito appena registrato, con i passi per annullarli.
async fn new_database<H: Host>(c: &Ctx<H>, site: &Site, undo: &mut Vec<Undo>, log: &mut JobLog) -> Result<DbCreds> {
    let slug = &site.slug;
    let db = DbCreds { name: slug.as_str().into(), user: slug.as_str().into(), password: random_password() };
    c.store.save_db_creds(site.id, &db.name, &db.user, &c.key.encrypt(&db.password)).await?;
    c.host.create_database(&db).await?;
    undo.push(Undo::Database { name: db.name.clone(), user: db.user.clone() });
    log.push("database e utente creati");
    c.host.create_site_dirs(slug, site.uid()).await?;
    undo.push(Undo::Dirs(slug.clone()));
    Ok(db)
}

/// Rigenera i pool e fallisce se quello del sito è stato rifiutato.
async fn check_own_pool<H: Host>(c: &Ctx<H>, site: &Site) -> Result<()> {
    let rejected = c.sync_php().await?;
    if rejected.contains(&format!("{}.conf", site.slug.as_str())) {
        bail!("il pool PHP del sito non è valido");
    }
    Ok(())
}

/// Annulla i passi fatti in ordine inverso; continua anche se un passo fallisce.
async fn rollback<H: Host>(c: &Ctx<H>, undo: Vec<Undo>, log: &mut JobLog) {
    for step in undo.into_iter().rev() {
        let res = match &step {
            Undo::Row(id) => c.store.delete_site(*id).await.map_err(Into::into),
            Undo::Database { name, user } => c.host.drop_database(name, user).await,
            Undo::Dirs(slug) => c.host.remove_site_dirs(slug).await,
        };
        if let Err(e) = res {
            log.push(format!("rollback incompleto: {e:#}"));
        }
    }
    for res in [c.sync_php().await.map(drop), c.sync_redis().await, c.sync_caddy().await] {
        if let Err(e) = res {
            log.push(format!("rigenerazione dopo il rollback: {e:#}"));
        }
    }
}

/// Dopo import e restore la cache oggetti in Redis ha i dati vecchi. `wp cache flush` usa il
/// flush selettivo del drop-in (solo le chiavi del sito); se non riesce, il sito resta valido.
async fn flush_object_cache<H: Host>(c: &Ctx<H>, site: &Site, log: &mut JobLog) {
    if site.kind != SiteKind::Wordpress {
        return;
    }
    let args = ["cache", "flush"].map(String::from);
    match c.host.wp(&site.slug, &args).await {
        Ok(_) => log.push("cache oggetti svuotata"),
        Err(e) => log.push(format!("cache oggetti non svuotata: {e:#}")),
    }
}

/// Import di un dump: prima un dump di sicurezza in tmp/<slug>, poi l'import e,
/// se richiesto, `wp search-replace` (rispetta i dati serializzati di WordPress).
pub async fn import_database<H: Host>(
    c: &Ctx<H>,
    site_id: i64,
    sql: Pin<Box<dyn AsyncRead + Send>>,
    safety: &std::path::Path,
    search_replace: Option<(String, String)>,
    log: &mut JobLog,
) -> Result<()> {
    let site = c.store.site(site_id).await?;
    let db = c.db_creds(site_id).await?;
    save_dump(c, &db.name, safety).await.context("dump di sicurezza")?;
    log.push(format!("dump di sicurezza salvato in {}", safety.display()));
    c.host.db_import(&db.name, sql).await?;
    log.push("dump importato");
    if let Some((from, to)) = search_replace {
        let args: Vec<String> = ["search-replace", &from, &to, "--all-tables", "--skip-columns=guid"].map(String::from).to_vec();
        let out = c.host.wp(&site.slug, &args).await?;
        log.push(format!("search-replace {from} → {to}: {}", out.lines().last().unwrap_or_default()));
    }
    flush_object_cache(c, &site, log).await;
    Ok(())
}

/// Un dump su file, compresso (.gz) o no, come flusso per `db_import`.
pub async fn open_sql(path: &std::path::Path) -> Result<Pin<Box<dyn AsyncRead + Send>>> {
    let file = tokio::fs::File::open(path).await.with_context(|| format!("apertura di {}", path.display()))?;
    let reader = tokio::io::BufReader::new(file);
    Ok(if path.extension().is_some_and(|e| e == "gz") {
        Box::pin(async_compression::tokio::bufread::GzipDecoder::new(reader))
    } else {
        Box::pin(reader)
    })
}

/// Dump compresso su file; fallisce se mariadb-dump non arriva in fondo.
pub async fn save_dump<H: Host>(c: &Ctx<H>, db: &str, dest: &std::path::Path) -> Result<()> {
    use async_compression::tokio::write::GzipEncoder;
    use tokio::io::AsyncWriteExt;
    let dump = c.host.db_dump(db, &[]).await?;
    let file = tokio::fs::File::create(dest).await.with_context(|| format!("creazione di {}", dest.display()))?;
    let mut gz = GzipEncoder::new(file);
    let mut reader = dump.reader;
    tokio::io::copy(&mut reader, &mut gz).await?;
    gz.shutdown().await?;
    dump.finished.await
}

pub async fn reset_db_password<H: Host>(c: &Ctx<H>, site_id: i64) -> Result<()> {
    let site = c.store.site(site_id).await?;
    let db = c.db_creds(site_id).await?;
    let password = random_password();
    c.host.set_db_password(&db.user, &password).await?;
    c.store.update_db_password(site_id, &c.key.encrypt(&password)).await?;
    c.host.set_site_env(&site.slug, site.uid(), "DB_PASSWORD", &password).await
}

/// Backup di un sito con registrazione dell'esito e avviso se fallisce.
pub async fn backup_site<H: Host>(c: &Ctx<H>, site_id: i64, log: &mut JobLog) -> Result<Snapshot> {
    if !c.backups {
        bail!("backup non configurati (RESTIC_REPOSITORY)");
    }
    let site = c.store.site(site_id).await?;
    let db = c.db_creds(site_id).await?;
    let run = c.store.backup_started(site_id, &site.slug).await?;
    let result = async {
        let snap = c.host.backup(&site.slug, &db.name).await?;
        c.host.prune(&site.slug, c.store.retention(site_id).await?).await?;
        Ok::<_, anyhow::Error>(snap)
    }
    .await;
    match &result {
        Ok(snap) => {
            c.store.backup_finished(run, Ok(snap)).await?;
            c.store.resolve_alerts(site_id, "backup").await?;
            log.push(format!("backup {} ({} byte nuovi)", snap.snapshot_id, snap.bytes_added));
        }
        Err(e) => {
            c.store.backup_finished(run, Err(format!("{e:#}"))).await?;
            c.store.raise_alert(Some(site_id), "backup", &format!("backup fallito: {e:#}")).await?;
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    Full,
    Files,
    Database,
}

/// Prima un backup dello stato attuale, poi il ripristino: si può sempre tornare indietro.
pub async fn restore_site<H: Host>(c: &Ctx<H>, site_id: i64, snapshot: &str, mode: RestoreMode, log: &mut JobLog) -> Result<()> {
    let site = c.store.site(site_id).await?;
    backup_site(c, site_id, log).await.context("backup prima del ripristino")?;
    let db = c.db_creds(site_id).await?;
    let files = if mode == RestoreMode::Database { FilesScope::None } else { FilesScope::All };
    let with_db = (mode != RestoreMode::Files).then_some(&db);
    c.host.restore(&site.slug, snapshot, &site.slug, site.uid(), &files, with_db).await?;
    log.push(format!("ripristinato {snapshot} ({})", match mode { RestoreMode::Full => "file e database", RestoreMode::Files => "solo file", RestoreMode::Database => "solo database" }));
    c.sync_php().await?;
    c.sync_caddy().await?;
    flush_object_cache(c, &site, log).await;
    Ok(())
}

/// Sospende o riattiva: alla sospensione prima la pagina di manutenzione, poi via il pool;
/// alla riattivazione prima il pool, così Caddy non manda mai richieste a un socket assente.
pub async fn set_suspended<H: Host>(c: &Ctx<H>, id: i64, suspended: bool) -> Result<()> {
    let site = c.store.site(id).await?;
    let (from, to) = if suspended { (SiteStatus::Active, SiteStatus::Suspended) } else { (SiteStatus::Suspended, SiteStatus::Active) };
    if site.status != from {
        bail!("il sito è {:?}, non {:?}", site.status, from);
    }
    c.store.set_status(id, to).await?;
    if suspended {
        c.sync_caddy().await?;
        c.sync_php().await?;
    } else {
        c.sync_php().await?;
        c.sync_caddy().await?;
    }
    Ok(())
}

/// Attiva (restituendo la password, utente `staging`) o toglie la protezione dello staging.
pub async fn set_staging<H: Host>(c: &Ctx<H>, id: i64, on: bool) -> Result<Option<String>> {
    c.store.site(id).await?;
    let password = on.then(random_password);
    match &password {
        Some(p) => {
            let hash = bcrypt::hash(p, bcrypt::DEFAULT_COST)?;
            c.store.set_staging(id, Some((&c.key.encrypt(p), &hash))).await?;
        }
        None => c.store.set_staging(id, None).await?,
    }
    c.sync_caddy().await?;
    Ok(password)
}

pub async fn staging_password<H>(c: &Ctx<H>, id: i64) -> Result<Option<String>> {
    Ok(c.store.staging_password(id).await?.map(|enc| c.key.decrypt(&enc)).transpose()?)
}

/// Percorso relativo alla cartella del sito, senza `..` né il `.env`.
pub fn site_rel_path(path: &str) -> Result<String> {
    let p = std::path::Path::new(path.trim_matches('/'));
    let ok = p.components().all(|c| matches!(c, std::path::Component::Normal(_)))
        && p.components().next().is_some_and(|c| c.as_os_str() != ".env");
    if !ok {
        bail!("percorso non valido: {path:?}");
    }
    Ok(p.to_string_lossy().into_owned())
}

/// Ripristina un solo file o cartella (un tema cancellato, un'immagine): la versione attuale va nel cestino.
pub async fn restore_path<H: Host>(c: &Ctx<H>, id: i64, snapshot: &str, path: &str, log: &mut JobLog) -> Result<()> {
    let site = c.store.site(id).await?;
    let rel = site_rel_path(path)?;
    c.host.restore(&site.slug, snapshot, &site.slug, site.uid(), &FilesScope::Path(rel.clone()), None).await?;
    log.push(format!("ripristinato {rel} da {snapshot}; la versione sostituita è nel cestino"));
    Ok(())
}

/// Ripristina uno snapshot come sito nuovo, protetto come staging: verifica un backup senza toccare l'originale.
pub async fn restore_as_new_site<H: Host>(c: &Ctx<H>, src_id: i64, snapshot: &str, domain: &Domain, log: &mut JobLog) -> Result<Site> {
    let src = c.store.site(src_id).await?;
    let src_domain = c.store.domains(src_id).await?.into_iter().next().map(|(d, _)| d).context("sito senza dominio")?;
    let site = c.store.create_site(&NewSite { primary: domain.clone(), kind: src.kind, template: src.template.clone() }).await?;
    log.push(format!("sito {} registrato per il ripristino di {snapshot}", site.slug.as_str()));
    let mut undo = vec![Undo::Row(site.id)];
    let steps = async {
        let db = new_database(c, &site, &mut undo, log).await?;
        c.host.restore(&src.slug, snapshot, &site.slug, site.uid(), &FilesScope::All, Some(&db)).await?;
        log.push("file e database ripristinati");
        let old_env = c.host.read_site_env(&site.slug).await?;
        configure_copy(c, &site, domain, &db, &old_env).await?;
        publish_copy(c, &site, &src_domain, domain, log).await?;
        flush_object_cache(c, &site, log).await;
        Ok::<_, anyhow::Error>(())
    };
    match steps.await {
        Ok(()) => Ok(c.store.site(site.id).await?),
        Err(e) => {
            log.push(format!("errore: {e:#}"));
            rollback(c, undo, log).await;
            Err(e)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    /// Backup finale obbligatorio e file nel cestino per 7 giorni.
    Normal,
    /// Copie usa e getta (restore di prova): niente backup, file eliminati subito.
    Discard,
}

/// Elimina un sito. Va solo avanti: se si ferma, il sito resta in `deleting` e si può rilanciare.
pub async fn delete_site<H: Host>(c: &Ctx<H>, id: i64, mode: DeleteMode, log: &mut JobLog) -> Result<()> {
    let site = c.store.site(id).await?;
    if mode == DeleteMode::Discard {
        log.push("copia usa e getta: nessun backup finale");
    } else if c.backups {
        backup_site(c, id, log).await.context("backup finale obbligatorio")?;
    } else {
        log.push("ATTENZIONE: backup non configurati, nessun backup finale");
    }
    let db = c.store.db_creds(id).await.ok();
    if mode == DeleteMode::Normal {
        let hosts = site_hostnames(c, id).await?;
        tunnel_hostnames(c, id, &hosts, false, log).await?;
    }
    c.store.set_status(id, SiteStatus::Deleting).await?;
    c.sync_caddy().await?;
    c.sync_php().await?;
    log.push("route e pool rimossi");
    if mode == DeleteMode::Discard {
        c.host.remove_site_dirs(&site.slug).await?;
        log.push("file eliminati");
    } else {
        c.host.trash_site_dirs(&site.slug).await?;
        log.push("file spostati nel cestino (7 giorni)");
    }
    if let Some((name, user, _)) = db {
        c.host.drop_database(&name, &user).await?;
        log.push("database eliminato");
    }
    c.store.delete_site(id).await?;
    c.sync_php().await?;
    c.sync_redis().await?;
    log.push(format!("sito {} eliminato", site.slug.as_str()));
    Ok(())
}

/// Restore di prova: l'ultimo snapshot diventa un sito su un dominio interno, si controlla che
/// risponda, poi si elimina senza lasciare tracce. L'esito resta in un avviso e nelle impostazioni.
pub async fn test_restore<H: Host>(c: &Ctx<H>, id: i64, log: &mut JobLog) -> Result<()> {
    let site = c.store.site(id).await?;
    let result = test_restore_steps(c, &site, log).await;
    let (ok, detail) = match &result {
        Ok(snap) => (true, format!("snapshot {snap} ripristinato e raggiungibile")),
        Err(e) => (false, format!("{e:#}")),
    };
    let record = serde_json::json!({ "at": chrono::Utc::now().to_rfc3339(), "ok": ok, "detail": detail });
    c.store.set_setting(&format!("restore_test:{id}"), &record.to_string()).await?;
    if ok {
        c.store.resolve_alerts(id, "restore-test").await?;
    } else {
        c.store.raise_alert(Some(id), "restore-test", &format!("restore di prova fallito: {detail}")).await?;
    }
    log.push(detail);
    result.map(drop)
}

async fn test_restore_steps<H: Host>(c: &Ctx<H>, site: &Site, log: &mut JobLog) -> Result<String> {
    let snapshot = c
        .store
        .backup_runs(site.id, 100)
        .await?
        .into_iter()
        .find_map(|r| r.snapshot_id.filter(|_| r.status == "done"))
        .context("nessun backup riuscito da verificare")?;
    let domain = Domain::parse(&format!("verifica-{}.alveare.invalid", site.slug.as_str().replace('_', "-")))
        .map_err(|_| anyhow::anyhow!("dominio di verifica non valido"))?;
    let copy = restore_as_new_site(c, site.id, &snapshot, &domain, log).await?;
    let password = staging_password(c, copy.id).await?.unwrap_or_default();
    let probe = c.host.probe(&domain, Some(("staging", &password))).await;
    delete_site(c, copy.id, DeleteMode::Discard, log).await.context("eliminazione della copia di prova")?;
    match probe? {
        s if (200..400).contains(&s) => Ok(snapshot),
        s => bail!("la copia ripristinata risponde {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::SecretKey;
    use crate::store::{SiteStatus, Store};
    use crate::testing::FakeHost;
    use alveare_core::site::Domain;

    async fn ctx(host: FakeHost) -> Ctx<FakeHost> {
        Ctx {
            store: Store::open_memory().await.unwrap(),
            host,
            key: SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap(),
            db_host: "mariadb".into(),
            global_env: Vars::from([("SMTP_HOST".into(), "email-smtp.eu-west-1.amazonaws.com".into())]),
            backups: false,
        }
    }

    fn req(domain: &str) -> CreateSite {
        CreateSite {
            domain: Domain::parse(domain).unwrap(),
            kind: SiteKind::Wordpress,
            template: "wordpress".into(),
            title: "Sona".into(),
            admin_email: "admin@sona.it".into(),
            table_prefix: "wp_".into(),
        }
    }

    #[tokio::test]
    async fn create_site_runs_steps_in_order_and_activates_site() {
        let c = ctx(FakeHost::default()).await;
        let mut log = JobLog::default();
        let site = create_site(&c, &req("sona.it"), &mut log).await.unwrap();
        assert_eq!(
            c.host.calls(),
            [
                "create_database sona_it",
                "create_site_dirs sona_it",
                "install_template sona_it wordpress",
                "sync_redis sona_it",
                "sync_php users=sona_it pools=sona_it",
                "wp sona_it core install",
                "sync_caddy sona_it",
            ]
        );
        assert_eq!(c.store.site(site.id).await.unwrap().status, SiteStatus::Active);
    }

    #[tokio::test]
    async fn snapshot_template_imports_its_dump_and_moves_it_to_the_new_domain() {
        let sql = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(sql.path(), "-- snapshot").unwrap();
        let c = ctx(FakeHost { snapshot_sql: Some(sql.path().to_path_buf()), ..Default::default() }).await;
        let mut r = req("sona.it");
        r.table_prefix = "pl_".into();
        create_site(&c, &r, &mut JobLog::default()).await.unwrap();
        let calls = c.host.calls();
        let at = |prefix: &str| calls.iter().position(|x| x.starts_with(prefix)).unwrap_or_else(|| panic!("manca {prefix}: {calls:?}"));
        assert!(at("sync_php") < at("db_import sona_it -- snapshot"));
        assert!(at("db_import") < at("wp sona_it search-replace template.local sona.it --all-tables --skip-columns=guid"));
        assert!(at("wp sona_it search-replace") < at("wp sona_it core install"));
        assert_eq!(c.host.env.lock().unwrap()["TABLE_PREFIX"], "pl_");
    }

    #[tokio::test]
    async fn clone_copies_files_and_database_then_protects_the_copy() {
        let c = ctx(FakeHost::default()).await;
        let src = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.env.lock().unwrap().insert("TABLE_PREFIX".into(), "pl_".into());
        c.host.calls.lock().unwrap().clear();
        let copy = clone_site(&c, src.id, &Domain::parse("prova.sona.it").unwrap(), &mut JobLog::default()).await.unwrap();
        assert_eq!(
            c.host.calls(),
            [
                "read_site_env sona_it",
                "create_database prova_sona_it",
                "create_site_dirs prova_sona_it",
                "copy_site_files sona_it prova_sona_it",
                "write_site_env prova_sona_it",
                "sync_redis prova_sona_it,sona_it",
                "sync_php users=prova_sona_it,sona_it pools=prova_sona_it,sona_it",
                "db_dump sona_it ",
                "db_import prova_sona_it -- dump finto",
                "wp prova_sona_it search-replace //sona.it //prova.sona.it --all-tables --skip-columns=guid",
                "sync_caddy prova_sona_it,sona_it",
            ]
        );
        let copy = c.store.site(copy.id).await.unwrap();
        assert_eq!((copy.status, copy.staging, copy.kind), (SiteStatus::Active, true, SiteKind::Wordpress));
        let env = c.host.env.lock().unwrap().clone();
        assert_eq!(env["TABLE_PREFIX"], "pl_", "stesso prefisso della sorgente");
        assert_eq!(env["WP_HOME"], "https://prova.sona.it");
        assert_eq!(env["WP_ENVIRONMENT_TYPE"], "staging");
        assert_eq!(env["DB_NAME"], "prova_sona_it");
    }

    #[tokio::test]
    async fn failed_clone_leaves_the_source_untouched_and_rolls_back() {
        let c = ctx(FakeHost::failing("db_import")).await;
        let src = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        assert!(clone_site(&c, src.id, &Domain::parse("prova.sona.it").unwrap(), &mut JobLog::default()).await.is_err());
        let ops = c.host.ops();
        assert!(ops.ends_with(&["remove_site_dirs", "drop_database", "sync_php", "sync_redis", "sync_caddy"].map(String::from)), "{ops:?}");
        assert_eq!(c.store.sites().await.unwrap().len(), 1);
    }

    fn import_req(dump: &std::path::Path) -> ImportSite {
        ImportSite { site: CreateSite { domain: Domain::parse("epops.it").unwrap(), ..req("epops.it") }, archive: "/srv/imports/files/epops.zip".into(), dump: dump.to_path_buf() }
    }

    #[tokio::test]
    async fn import_keeps_the_old_files_and_prefix_and_moves_the_domain() {
        let dump = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(dump.path(), "-- vecchio dump").unwrap();
        let host = FakeHost {
            imported: Some(Imported { table_prefix: Some("ep_".into()), has_core: false }),
            wp_replies: vec![("option get home", "https://vecchio.epops.it\n")],
            ..Default::default()
        };
        let c = ctx(host).await;
        let site = import_site(&c, &import_req(dump.path()), &mut JobLog::default()).await.unwrap();
        assert_eq!(
            c.host.calls(),
            [
                "create_database epops_it",
                "create_site_dirs epops_it",
                "import_files epops_it /srv/imports/files/epops.zip",
                "install_template epops_it wordpress",
                "sync_redis epops_it",
                "sync_php users=epops_it pools=epops_it",
                "wp epops_it core download --skip-content --locale=it_IT",
                "db_import epops_it -- vecchio dump",
                "wp epops_it option get home",
                "wp epops_it search-replace //vecchio.epops.it //epops.it --all-tables --skip-columns=guid",
                "sync_caddy epops_it",
                "wp epops_it cache flush",
            ]
        );
        assert_eq!(c.host.env.lock().unwrap()["TABLE_PREFIX"], "ep_");
        assert_eq!(c.store.site(site.id).await.unwrap().status, SiteStatus::Active);
    }

    #[tokio::test]
    async fn import_on_the_same_domain_skips_search_replace_and_core_download() {
        let dump = tempfile::NamedTempFile::new().unwrap();
        let host = FakeHost {
            imported: Some(Imported { table_prefix: None, has_core: true }),
            wp_replies: vec![("option get home", "https://epops.it")],
            ..Default::default()
        };
        let c = ctx(host).await;
        import_site(&c, &import_req(dump.path()), &mut JobLog::default()).await.unwrap();
        let calls = c.host.calls();
        assert!(!calls.iter().any(|x| x.contains("search-replace") || x.contains("core download")), "{calls:?}");
        assert_eq!(c.host.env.lock().unwrap()["TABLE_PREFIX"], "wp_");
    }

    #[tokio::test]
    async fn failed_import_rolls_back() {
        let dump = tempfile::NamedTempFile::new().unwrap();
        let c = ctx(FakeHost { fail_on: Some("db_import"), imported: Some(Imported::default()), ..Default::default() }).await;
        assert!(import_site(&c, &import_req(dump.path()), &mut JobLog::default()).await.is_err());
        assert!(c.store.sites().await.unwrap().is_empty());
        assert!(c.host.ops().contains(&"drop_database".to_string()));
    }

    #[test]
    fn host_of_url() {
        assert_eq!(url_host("https://www.epops.it/blog\n").as_deref(), Some("www.epops.it"));
        assert_eq!(url_host("http://localhost:8080").as_deref(), Some("localhost"));
        assert_eq!(url_host("niente"), None);
    }

    #[tokio::test]
    async fn export_strips_credentials_and_writes_a_readme() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        let dir = tempfile::tempdir().unwrap();
        let export = export_site(&c, site.id, dir.path(), 1_800_000_000, &mut JobLog::default()).await.unwrap();
        assert_eq!(export.token.len(), 32);
        assert_eq!(export.file, "1800000000-sona_it.tar.gz");
        assert_eq!(c.host.calls(), ["read_site_env sona_it", format!("export_site sona_it sona_it {}", dir.path().join(&export.file).display()).as_str()]);
        let (env, readme) = c.host.exported.lock().unwrap().clone().unwrap();
        assert!(env.contains("DB_PASSWORD='DA_IMPOSTARE'") && env.contains("AUTH_KEY='DA_IMPOSTARE'") && env.contains("REDIS_PASSWORD='DA_IMPOSTARE'"), "{env}");
        assert!(env.contains("DB_NAME='sona_it'") && env.contains("WP_HOME='https://sona.it'"));
        assert!(readme.contains("PHP 8.5") && readme.contains("database.sql") && readme.contains("sona.it"));
        let found = c.store.export_by_token(&export.token).await.unwrap().unwrap();
        assert_eq!((found.site_id, found.size), (site.id, 1234));
    }

    #[tokio::test]
    async fn save_as_template_exports_a_serialization_safe_dump() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.env.lock().unwrap().insert("TABLE_PREFIX".into(), "pl_".into());
        c.host.calls.lock().unwrap().clear();
        let name = Slug::parse("negozio").unwrap();
        save_template(&c, site.id, &name, "Negozio pronto", 1_800_000_000, &mut JobLog::default()).await.unwrap();
        assert_eq!(
            c.host.calls(),
            [
                "read_site_env sona_it",
                "wp sona_it search-replace sona.it template.local --all-tables --skip-columns=guid --export=/srv/tmp/sona_it/template-1800000000.sql",
                "store_template sona_it negozio template-1800000000.sql",
            ]
        );
        let manifest = c.host.stored_manifest.lock().unwrap().take().unwrap();
        assert_eq!(manifest.name, "Negozio pronto");
        assert_eq!(manifest.sql.as_deref(), Some(std::path::Path::new("db.sql")));
        assert_eq!(manifest.sql_domain, "template.local");
        assert_eq!(manifest.table_prefix.as_deref(), Some("pl_"));
        assert!(manifest.post_create.is_empty());
    }

    #[tokio::test]
    async fn db_password_is_stored_encrypted_and_written_to_env() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        let creds = c.db_creds(site.id).await.unwrap();
        let env = c.host.env.lock().unwrap().clone();
        assert_eq!(creds.name, "sona_it");
        assert_eq!(env["DB_PASSWORD"], creds.password);
        assert_eq!(env["DB_HOST"], "mariadb");
        assert_eq!(env["SMTP_HOST"], "email-smtp.eu-west-1.amazonaws.com");
        assert_eq!(env["WP_HOME"], "https://sona.it");
        assert_eq!(env["AUTH_KEY"].len(), 64);
        assert_ne!(env["AUTH_KEY"], env["AUTH_SALT"]);
        assert!(!env.contains_key("ADMIN_PASSWORD"), "la password admin non va nel .env");
        assert_eq!(env["SMTP_FROM"], "noreply@sona.it");
        assert_eq!(env["SMTP_FROM_NAME"], "Sona");
        assert_eq!(env["REDIS_USERNAME"], "sona_it");
        assert_eq!(env["REDIS_PREFIX"], "sona_it:");
        assert_eq!(env["REDIS_PASSWORD"].len(), 32);
        assert_ne!(env["REDIS_PASSWORD"], env["DB_PASSWORD"]);
    }

    #[tokio::test]
    async fn failure_undoes_completed_steps_in_reverse_and_resyncs() {
        let c = ctx(FakeHost::failing("install_template")).await;
        let mut log = JobLog::default();
        assert!(create_site(&c, &req("sona.it"), &mut log).await.is_err());
        assert_eq!(
            c.host.ops(),
            ["create_database", "create_site_dirs", "install_template", "remove_site_dirs", "drop_database", "sync_php", "sync_redis", "sync_caddy"]
        );
        assert!(c.store.sites().await.unwrap().is_empty());
        assert!(log.lines.iter().any(|l| l.contains("install_template fallito")));
    }

    #[tokio::test]
    async fn rejected_pool_fails_the_creation() {
        let c = ctx(FakeHost { reject_pool: true, ..Default::default() }).await;
        assert!(create_site(&c, &req("sona.it"), &mut JobLog::default()).await.is_err());
        assert!(c.store.sites().await.unwrap().is_empty());
        assert!(c.host.ops().contains(&"drop_database".to_string()));
    }

    #[tokio::test]
    async fn duplicate_hostname_touches_nothing() {
        let c = ctx(FakeHost::default()).await;
        create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        let before = c.host.calls().len();
        assert!(create_site(&c, &req("sona.it"), &mut JobLog::default()).await.is_err());
        assert_eq!(c.host.calls().len(), before);
    }

    async fn ctx_with_backups(host: FakeHost) -> Ctx<FakeHost> {
        Ctx { backups: true, ..ctx(host).await }
    }

    #[tokio::test]
    async fn delete_takes_a_final_backup_first() {
        let c = ctx_with_backups(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        delete_site(&c, site.id, DeleteMode::Normal, &mut JobLog::default()).await.unwrap();
        assert_eq!(c.host.ops()[..2], ["backup", "prune"]);
    }

    #[tokio::test]
    async fn failed_final_backup_stops_the_deletion_and_alerts() {
        let c = ctx_with_backups(FakeHost::failing("backup")).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        assert!(delete_site(&c, site.id, DeleteMode::Normal, &mut JobLog::default()).await.is_err());
        assert_eq!(c.store.site(site.id).await.unwrap().status, SiteStatus::Active);
        let alerts = c.store.open_alerts(None).await.unwrap();
        assert_eq!(alerts[0].kind, "backup");
        assert_eq!(c.store.backup_runs(site.id, 10).await.unwrap()[0].status, "failed");
    }

    #[tokio::test]
    async fn restore_backs_up_current_state_then_restores_and_resyncs() {
        let c = ctx_with_backups(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        restore_site(&c, site.id, "abcdef12", RestoreMode::Database, &mut JobLog::default()).await.unwrap();
        assert_eq!(
            c.host.calls(),
            [
                "backup sona_it sona_it",
                "prune sona_it 7",
                "restore sona_it abcdef12 -> sona_it files=none db=sona_it",
                "sync_php users=sona_it pools=sona_it",
                "sync_caddy sona_it",
                "wp sona_it cache flush",
            ]
        );
        let runs = c.store.backup_runs(site.id, 10).await.unwrap();
        assert_eq!((runs[0].status.as_str(), runs[0].snapshot_id.as_deref()), ("done", Some("abcdef12")));
    }

    #[tokio::test]
    async fn suspend_swaps_route_for_maintenance_and_drops_the_pool() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        set_suspended(&c, site.id, true).await.unwrap();
        assert_eq!(c.host.calls(), ["sync_caddy sona_it", "sync_php users=sona_it pools="]);
        assert_eq!(c.store.site(site.id).await.unwrap().status, SiteStatus::Suspended);
        assert!(c.store.caddy_sites().await.unwrap()[0].suspended);
        c.host.calls.lock().unwrap().clear();
        set_suspended(&c, site.id, false).await.unwrap();
        // Prima il pool, poi la route: alla riattivazione PHP deve già rispondere.
        assert_eq!(c.host.calls(), ["sync_php users=sona_it pools=sona_it", "sync_caddy sona_it"]);
        assert_eq!(c.store.site(site.id).await.unwrap().status, SiteStatus::Active);
    }

    #[tokio::test]
    async fn only_active_or_suspended_sites_change_state() {
        let c = ctx(FakeHost::default()).await;
        let site = c.store.create_site(&NewSite { primary: Domain::parse("x.it").unwrap(), kind: SiteKind::Wordpress, template: "wordpress".into() }).await.unwrap();
        assert!(set_suspended(&c, site.id, true).await.is_err());
        assert_eq!(c.store.site(site.id).await.unwrap().status, SiteStatus::Creating);
    }

    #[tokio::test]
    async fn staging_protection_uses_a_bcrypt_password_shown_to_managers() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        let password = set_staging(&c, site.id, true).await.unwrap().unwrap();
        assert_eq!(password.len(), 32);
        let hash = c.store.caddy_sites().await.unwrap()[0].staging_hash.clone().unwrap();
        assert!(bcrypt::verify(&password, &hash).unwrap());
        assert_eq!(staging_password(&c, site.id).await.unwrap().as_deref(), Some(password.as_str()));
        assert_eq!(c.host.calls(), ["sync_caddy sona_it"]);
        assert!(set_staging(&c, site.id, false).await.unwrap().is_none());
        assert!(c.store.caddy_sites().await.unwrap()[0].staging_hash.is_none());
        assert!(staging_password(&c, site.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn single_path_restore_touches_only_that_path() {
        let c = ctx_with_backups(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        restore_path(&c, site.id, "abcdef12", "public/wp-content/themes/tema", &mut JobLog::default()).await.unwrap();
        assert_eq!(c.host.calls(), ["restore sona_it abcdef12 -> sona_it files=public/wp-content/themes/tema db=-"]);
        for bad in ["../altro", "public/../../etc", "", ".env"] {
            assert!(restore_path(&c, site.id, "abcdef12", bad, &mut JobLog::default()).await.is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn restore_on_new_domain_builds_a_protected_copy_from_the_snapshot() {
        let c = ctx_with_backups(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        let copy = restore_as_new_site(&c, site.id, "abcdef12", &Domain::parse("verifica.sona.it").unwrap(), &mut JobLog::default()).await.unwrap();
        assert_eq!(
            c.host.calls(),
            [
                "create_database verifica_sona_it",
                "create_site_dirs verifica_sona_it",
                "restore sona_it abcdef12 -> verifica_sona_it files=all db=verifica_sona_it",
                "read_site_env verifica_sona_it",
                "write_site_env verifica_sona_it",
                "sync_redis sona_it,verifica_sona_it",
                "sync_php users=sona_it,verifica_sona_it pools=sona_it,verifica_sona_it",
                "wp verifica_sona_it search-replace //sona.it //verifica.sona.it --all-tables --skip-columns=guid",
                "sync_caddy sona_it,verifica_sona_it",
                "wp verifica_sona_it cache flush",
            ]
        );
        let copy = c.store.site(copy.id).await.unwrap();
        assert!(copy.staging && copy.status == SiteStatus::Active);
        // Nessun backup: lo snapshot esiste già e il sito di origine non si tocca.
        assert!(!c.host.ops().contains(&"backup".to_string()));
    }

    #[tokio::test]
    async fn test_restore_checks_the_copy_answers_then_discards_it() {
        let c = ctx_with_backups(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        backup_site(&c, site.id, &mut JobLog::default()).await.unwrap();
        c.store.raise_alert(Some(site.id), "restore-test", "vecchio").await.unwrap();
        c.host.calls.lock().unwrap().clear();
        test_restore(&c, site.id, &mut JobLog::default()).await.unwrap();
        let calls = c.host.calls();
        assert!(calls.contains(&"restore sona_it abcdef12 -> verifica_sona_it_alveare files=all db=verifica_sona_it_alveare".to_string()), "{calls:?}");
        let probe = calls.iter().position(|x| x.starts_with("probe verifica-sona-it.alveare.invalid staging:")).unwrap();
        let removed = calls.iter().position(|x| x == "remove_site_dirs verifica_sona_it_alveare").unwrap();
        assert!(probe < removed);
        assert!(!calls.iter().any(|x| x.starts_with("backup ") || x.starts_with("trash_site_dirs")), "la copia di prova non si salva: {calls:?}");
        assert_eq!(c.store.sites().await.unwrap().len(), 1);
        assert!(c.store.open_alerts(None).await.unwrap().is_empty());
        assert!(c.store.setting(&format!("restore_test:{}", site.id)).await.unwrap().unwrap().contains("\"ok\":true"));
    }

    #[tokio::test]
    async fn failed_test_restore_alerts_and_still_removes_the_copy() {
        let c = ctx_with_backups(FakeHost { probe_status: Some(500), ..Default::default() }).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        backup_site(&c, site.id, &mut JobLog::default()).await.unwrap();
        assert!(test_restore(&c, site.id, &mut JobLog::default()).await.is_err());
        assert_eq!(c.store.sites().await.unwrap().len(), 1);
        let alerts = c.store.open_alerts(None).await.unwrap();
        assert_eq!(alerts[0].kind, "restore-test");
        assert!(alerts[0].message.contains("500"), "{}", alerts[0].message);
    }

    #[tokio::test]
    async fn test_restore_needs_a_backup() {
        let c = ctx_with_backups(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        assert!(test_restore(&c, site.id, &mut JobLog::default()).await.is_err());
        assert_eq!(c.store.open_alerts(None).await.unwrap()[0].kind, "restore-test");
    }

    #[tokio::test]
    async fn php_override_is_validated_applied_and_reverted_if_fpm_refuses() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        let err = set_php_override(&c, site.id, "php_admin_value[open_basedir] = /", Some(1)).await.unwrap_err();
        assert!(err.downcast_ref::<Invalid>().is_some(), "{err:#}");
        assert!(c.store.current_override(Some(site.id), OverrideKind::Php).await.unwrap().is_none());
        set_php_override(&c, site.id, "php_value[max_input_vars] = 5000", Some(1)).await.unwrap();
        assert_eq!(c.store.php_sites().await.unwrap()[0].extra, ["php_value[max_input_vars] = 5000"]);

        let c = ctx(FakeHost { reject_pool: true, ..Default::default() }).await;
        let site = c.store.create_site(&NewSite { primary: Domain::parse("x.it").unwrap(), kind: SiteKind::Wordpress, template: "wordpress".into() }).await.unwrap();
        let err = set_php_override(&c, site.id, "php_value[memory_limit] = tanto", Some(1)).await.unwrap_err();
        assert!(err.downcast_ref::<Invalid>().is_some());
        assert!(c.store.current_override(Some(site.id), OverrideKind::Php).await.unwrap().is_none(), "versione rifiutata tolta");
    }

    #[tokio::test]
    async fn caddy_override_is_checked_on_the_adapted_json_and_reverted() {
        let bad = serde_json::json!({"routes": [{"handle": [{"handler": "vars", "root": "/etc"}]}]});
        let c = ctx(FakeHost { adapted: std::sync::Mutex::new(Some(bad)), ..Default::default() }).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        let err = set_caddy_override(&c, Some(site.id), "root * /etc", Some(1)).await.unwrap_err();
        assert!(err.to_string().contains("/etc"), "{err}");
        assert!(c.store.current_override(Some(site.id), OverrideKind::Caddy).await.unwrap().is_none());
        *c.host.adapted.lock().unwrap() = None;
        c.host.calls.lock().unwrap().clear();
        set_caddy_override(&c, Some(site.id), "header X-Prova 1", Some(1)).await.unwrap();
        assert_eq!(c.host.calls(), ["caddy_adapt", "sync_caddy sona_it"]);
        assert_eq!(c.store.caddy_sites().await.unwrap()[0].extra.as_deref(), Some("header X-Prova 1"));
        set_caddy_override(&c, None, "(sicurezza) {\n\theader X-Frame-Options DENY\n}", Some(1)).await.unwrap();
        assert!(c.host.caddy_global.lock().unwrap().as_deref().unwrap().contains("sicurezza"));
    }

    #[tokio::test]
    async fn caddy_refusing_the_config_reverts_the_override() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        let c = Ctx { host: FakeHost::failing("sync_caddy"), ..c };
        assert!(set_caddy_override(&c, Some(site.id), "header X 1", Some(1)).await.is_err());
        assert!(c.store.current_override(Some(site.id), OverrideKind::Caddy).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn global_php_ini_is_validated_written_and_reloaded() {
        let c = ctx(FakeHost::default()).await;
        assert!(set_php_global(&c, "disable_functions =", Some(1)).await.unwrap_err().downcast_ref::<Invalid>().is_some());
        set_php_global(&c, "; note\nmax_input_vars = 3000\n", Some(1)).await.unwrap();
        assert_eq!(c.host.calls(), ["write_php_global max_input_vars = 3000", "sync_php users= pools="]);
    }

    #[tokio::test]
    async fn custom_crons_run_per_kind_and_alert_on_failure() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.store.set_staging(site.id, Some((&c.key.encrypt("pw"), "$2b$x"))).await.unwrap();
        let wp = c.store.add_cron(site.id, "* * * * *", "wp", "cache flush", 600).await.unwrap();
        let php = c.store.add_cron(site.id, "* * * * *", "artisan", "php artisan schedule:run", 120).await.unwrap();
        let http = c.store.add_cron(site.id, "* * * * *", "http", "https://sona.it/wp-cron.php", 30).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        for id in [wp, php, http] {
            assert!(run_cron(&c, id).await.unwrap());
        }
        assert_eq!(
            c.host.calls(),
            ["wp sona_it cache flush", "php sona_it artisan schedule:run (120 s)", "http_get sona.it /wp-cron.php staging:"]
        );
        assert_eq!(c.store.cron_runs(http, 5).await.unwrap()[0].ok, Some(true));

        let c = Ctx { host: FakeHost { probe_status: Some(500), ..Default::default() }, ..c };
        assert!(!run_cron(&c, http).await.unwrap());
        let alerts = c.store.open_alerts(None).await.unwrap();
        assert_eq!(alerts[0].kind, format!("cron-{http}"));
        assert!(c.store.cron_runs(http, 1).await.unwrap()[0].output.contains("500"));
        let c = Ctx { host: FakeHost::default(), ..c };
        assert!(run_cron(&c, http).await.unwrap());
        assert!(c.store.open_alerts(None).await.unwrap().is_empty(), "risolto al primo successo");
    }

    #[tokio::test]
    async fn alerts_go_to_the_channel_with_the_site_name() {
        let c = ctx(FakeHost::default()).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.store.raise_alert(Some(site.id), "backup", "backup fallito: rete").await.unwrap();
        c.store.raise_alert(None, "panel-backup", "x").await.unwrap();
        c.host.calls.lock().unwrap().clear();
        assert_eq!(dispatch_alerts(&c).await.unwrap(), 2);
        assert_eq!(c.host.calls(), ["notify ⚠️ sona.it · backup\nbackup fallito: rete", "notify ⚠️ pannello · panel-backup\nx"]);
        c.store.resolve_alerts(site.id, "backup").await.unwrap();
        dispatch_alerts(&c).await.unwrap();
        assert_eq!(c.host.calls().last().unwrap(), "notify ✅ sona.it · backup: risolto");
        assert_eq!(dispatch_alerts(&c).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn hostnames_follow_the_site_on_the_cloudflare_tunnel() {
        let c = ctx(FakeHost { cf_accounts: vec!["banda".into(), "proloco".into()], ..Default::default() }).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        assert_eq!(c.host.calls().last().unwrap(), "tunnel_add banda sona.it", "il primo account per default");
        c.store.add_domain(site.id, &Domain::parse("www.sona.it").unwrap(), crate::store::DomainKind::Redirect).await.unwrap();
        c.store.set_cloudflare_account(site.id, Some("proloco")).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        delete_site(&c, site.id, DeleteMode::Normal, &mut JobLog::default()).await.unwrap();
        let calls = c.host.calls();
        assert_eq!(calls[..2], ["tunnel_remove proloco sona.it", "tunnel_remove proloco www.sona.it"]);
    }

    #[tokio::test]
    async fn tunnel_errors_do_not_stop_the_site_but_raise_an_alert() {
        let c = ctx(FakeHost { cf_accounts: vec!["banda".into()], fail_on: Some("tunnel_add"), ..Default::default() }).await;
        let mut log = JobLog::default();
        let site = create_site(&c, &req("sona.it"), &mut log).await.unwrap();
        assert_eq!(c.store.site(site.id).await.unwrap().status, SiteStatus::Active);
        assert_eq!(c.store.open_alerts(None).await.unwrap()[0].kind, "cloudflare");
        assert!(log.lines.iter().any(|l| l.contains("non sistemato")));
    }

    #[tokio::test]
    async fn delete_site_removes_route_and_pool_before_files_and_database() {
        let c = ctx(FakeHost::default()).await;
        let keep = create_site(&c, &req("upupa.it"), &mut JobLog::default()).await.unwrap();
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        delete_site(&c, site.id, DeleteMode::Normal, &mut JobLog::default()).await.unwrap();
        assert_eq!(
            c.host.calls(),
            [
                "sync_caddy upupa_it",
                "sync_php users=sona_it,upupa_it pools=upupa_it",
                "trash_site_dirs sona_it",
                "drop_database sona_it",
                "sync_php users=upupa_it pools=upupa_it",
                "sync_redis upupa_it",
            ]
        );
        assert!(c.store.site(site.id).await.is_err());
        assert!(c.store.site(keep.id).await.is_ok());
    }
}
