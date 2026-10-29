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
use crate::store::{NewSite, Site, SiteKind, SiteStatus, Store};
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
    /// Copia il template, scrive il `.env` e restituisce i comandi WP-CLI da eseguire dopo.
    fn install_template(&self, slug: &Slug, uid: u32, template: &str, vars: &Vars, env: &Vars) -> impl Future<Output = Result<Vec<Vec<String>>>> + Send;
    /// Rigenera passwd, group e pool; restituisce i file di pool rifiutati.
    fn sync_php(&self, users: &[SitePhp], pools: &[SitePhp]) -> impl Future<Output = Result<Vec<String>>> + Send;
    fn sync_caddy(&self, sites: &[CaddySite]) -> impl Future<Output = Result<()>> + Send;
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
    /// Ripristina file e/o database dallo snapshot (dopo un backup dello stato attuale fatto dal flusso).
    fn restore(&self, slug: &Slug, snapshot: &str, files: bool, db: Option<&DbCreds>) -> impl Future<Output = Result<()>> + Send;
    /// panel.db, template e configurazione cifrata, con il tag `panel`.
    fn backup_panel(&self, store: &Store, env_enc: &[u8]) -> impl Future<Output = Result<Snapshot>> + Send;
    /// Riscrive l'ACL di Redis e la fa ricaricare.
    fn sync_redis(&self, users: &[RedisUser]) -> impl Future<Output = Result<()>> + Send;
    /// Token monouso per aprire Adminer già autenticato su questo database.
    fn adminer_grant(&self, db: &DbCreds) -> impl Future<Output = Result<String>> + Send;
    /// Riscrive una chiave del .env del sito.
    fn set_site_env(&self, slug: &Slug, uid: u32, key: &str, value: &str) -> impl Future<Output = Result<()>> + Send;
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

pub struct CreateSite {
    pub domain: Domain,
    pub kind: SiteKind,
    pub template: String,
    pub title: String,
    pub admin_email: String,
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
        self.host.sync_caddy(&self.store.caddy_sites().await?).await
    }

    async fn sync_redis(&self) -> Result<()> {
        self.host.sync_redis(&redis_users(&self.store, &self.key).await?).await
    }
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
        ("TABLE_PREFIX".into(), "wp_".into()),
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
    let db = DbCreds { name: slug.as_str().into(), user: slug.as_str().into(), password: random_password() };
    c.store.save_db_creds(site.id, &db.name, &db.user, &c.key.encrypt(&db.password)).await?;
    c.host.create_database(&db).await?;
    undo.push(Undo::Database { name: db.name.clone(), user: db.user.clone() });
    log.push("database e utente creati");

    c.host.create_site_dirs(slug, site.uid()).await?;
    undo.push(Undo::Dirs(slug.clone()));
    let redis_password = random_password();
    c.store.save_redis_password(site.id, &c.key.encrypt(&redis_password)).await?;
    let (vars, env) = site_vars(c, req, site, &db, &redis_password);
    let commands = c.host.install_template(slug, site.uid(), &req.template, &vars, &env).await?;
    log.push(format!("template {} installato", req.template));
    c.sync_redis().await?;
    log.push("utente Redis creato");

    let rejected = c.sync_php().await?;
    let own = format!("{}.conf", slug.as_str());
    if rejected.contains(&own) {
        bail!("il pool PHP del sito non è valido");
    }
    log.push("pool PHP attivo");

    for args in &commands {
        c.host.wp(slug, args).await.with_context(|| format!("wp {}", args.first().map_or("", String::as_str)))?;
    }
    if !commands.is_empty() {
        // shortcut: la password admin finisce nel log del job, visibile solo agli admin; da sostituire con l'invito via email (fase 3).
        log.push(format!("WordPress installato: utente {}, password {}", vars["ADMIN_USER"], vars["ADMIN_PASSWORD"]));
    }

    c.store.set_status(site.id, SiteStatus::Active).await?;
    c.sync_caddy().await?;
    log.push(format!("sito attivo: aggiungi {} al tunnel Cloudflare verso http://caddy:80", req.domain.as_str()));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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
    let files = mode != RestoreMode::Database;
    let with_db = (mode != RestoreMode::Files).then_some(&db);
    c.host.restore(&site.slug, snapshot, files, with_db).await?;
    log.push(format!("ripristinato {snapshot} ({})", match mode { RestoreMode::Full => "file e database", RestoreMode::Files => "solo file", RestoreMode::Database => "solo database" }));
    c.sync_php().await?;
    c.sync_caddy().await?;
    flush_object_cache(c, &site, log).await;
    Ok(())
}

/// Elimina un sito. Va solo avanti: se si ferma, il sito resta in `deleting` e si può rilanciare.
pub async fn delete_site<H: Host>(c: &Ctx<H>, id: i64, log: &mut JobLog) -> Result<()> {
    let site = c.store.site(id).await?;
    if c.backups {
        backup_site(c, id, log).await.context("backup finale obbligatorio")?;
    } else {
        log.push("ATTENZIONE: backup non configurati, nessun backup finale");
    }
    let db = c.store.db_creds(id).await.ok();
    c.store.set_status(id, SiteStatus::Deleting).await?;
    c.sync_caddy().await?;
    c.sync_php().await?;
    log.push("route e pool rimossi");
    c.host.trash_site_dirs(&site.slug).await?;
    log.push("file spostati nel cestino (7 giorni)");
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
        delete_site(&c, site.id, &mut JobLog::default()).await.unwrap();
        assert_eq!(c.host.ops()[..2], ["backup", "prune"]);
    }

    #[tokio::test]
    async fn failed_final_backup_stops_the_deletion_and_alerts() {
        let c = ctx_with_backups(FakeHost::failing("backup")).await;
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        assert!(delete_site(&c, site.id, &mut JobLog::default()).await.is_err());
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
                "restore sona_it abcdef12 files=false db=sona_it",
                "sync_php users=sona_it pools=sona_it",
                "sync_caddy sona_it",
                "wp sona_it cache flush",
            ]
        );
        let runs = c.store.backup_runs(site.id, 10).await.unwrap();
        assert_eq!((runs[0].status.as_str(), runs[0].snapshot_id.as_deref()), ("done", Some("abcdef12")));
    }

    #[tokio::test]
    async fn delete_site_removes_route_and_pool_before_files_and_database() {
        let c = ctx(FakeHost::default()).await;
        let keep = create_site(&c, &req("upupa.it"), &mut JobLog::default()).await.unwrap();
        let site = create_site(&c, &req("sona.it"), &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        delete_site(&c, site.id, &mut JobLog::default()).await.unwrap();
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
