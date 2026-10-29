//! Flussi lunghi (crea, elimina sito). Ogni passo con effetti fuori dal pannello registra
//! come annullarlo; in caso di errore si annulla in ordine inverso e si rigenera tutto dallo stato.
#![allow(async_fn_in_trait)]

use alveare_core::caddy::CaddySite;
use alveare_core::php::SitePhp;
use alveare_core::site::{Domain, Slug};
use anyhow::{bail, Context, Result};
use rand::distr::{Alphanumeric, SampleString};

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
pub trait Host {
    async fn create_database(&self, db: &DbCreds) -> Result<()>;
    async fn drop_database(&self, name: &str, user: &str) -> Result<()>;
    async fn create_site_dirs(&self, slug: &Slug, uid: u32) -> Result<()>;
    async fn remove_site_dirs(&self, slug: &Slug) -> Result<()>;
    async fn trash_site_dirs(&self, slug: &Slug) -> Result<()>;
    /// Copia il template, scrive il `.env` e restituisce i comandi WP-CLI da eseguire dopo.
    async fn install_template(&self, slug: &Slug, uid: u32, template: &str, vars: &Vars, env: &Vars) -> Result<Vec<Vec<String>>>;
    /// Rigenera passwd, group e pool; restituisce i file di pool rifiutati.
    async fn sync_php(&self, users: &[SitePhp], pools: &[SitePhp]) -> Result<Vec<String>>;
    async fn sync_caddy(&self, sites: &[CaddySite]) -> Result<()>;
    async fn wp(&self, slug: &Slug, args: &[String]) -> Result<String>;
}

pub struct Ctx<H> {
    pub store: Store,
    pub host: H,
    pub key: SecretKey,
    pub db_host: String,
    /// Valori comuni a tutti i siti (SMTP di SES).
    pub global_env: Vars,
}

pub struct CreateSite {
    pub domain: Domain,
    pub kind: SiteKind,
    pub template: String,
    pub title: String,
    pub admin_email: String,
}

#[derive(Debug, Default)]
pub struct JobLog {
    pub lines: Vec<String>,
}

impl JobLog {
    pub fn push(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
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
}

fn random_salt() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 64)
}

/// (variabili del template, contenuto del `.env`)
fn site_vars<H>(c: &Ctx<H>, req: &CreateSite, site: &Site, db: &DbCreds) -> (Vars, Vars) {
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
    let (vars, env) = site_vars(c, req, site, &db);
    let commands = c.host.install_template(slug, site.uid(), &req.template, &vars, &env).await?;
    log.push(format!("template {} installato", req.template));

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
    for res in [c.sync_php().await.map(drop), c.sync_caddy().await] {
        if let Err(e) = res {
            log.push(format!("rigenerazione dopo il rollback: {e:#}"));
        }
    }
}

/// Elimina un sito. Va solo avanti: se si ferma, il sito resta in `deleting` e si può rilanciare.
pub async fn delete_site<H: Host>(c: &Ctx<H>, id: i64, log: &mut JobLog) -> Result<()> {
    let site = c.store.site(id).await?;
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
    log.push(format!("sito {} eliminato", site.slug.as_str()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::SecretKey;
    use crate::store::{SiteStatus, Store};
    use alveare_core::site::Domain;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeHost {
        calls: Mutex<Vec<String>>,
        fail_on: Option<&'static str>,
        reject_pool: bool,
        env: Mutex<Vars>,
    }

    impl FakeHost {
        fn failing(op: &'static str) -> Self {
            Self { fail_on: Some(op), ..Default::default() }
        }
        fn record(&self, op: &str, detail: String) -> anyhow::Result<()> {
            self.calls.lock().unwrap().push(if detail.is_empty() { op.to_string() } else { format!("{op} {detail}") });
            if self.fail_on == Some(op) {
                anyhow::bail!("{op} fallito");
            }
            Ok(())
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn ops(&self) -> Vec<String> {
            self.calls().iter().map(|c| c.split(' ').next().unwrap().to_string()).collect()
        }
    }

    fn slugs<'a>(it: impl Iterator<Item = &'a Slug>) -> String {
        it.map(|s| s.as_str()).collect::<Vec<_>>().join(",")
    }

    impl Host for FakeHost {
        async fn create_database(&self, db: &DbCreds) -> anyhow::Result<()> {
            self.record("create_database", db.name.clone())
        }
        async fn drop_database(&self, name: &str, _user: &str) -> anyhow::Result<()> {
            self.record("drop_database", name.to_string())
        }
        async fn create_site_dirs(&self, slug: &Slug, _uid: u32) -> anyhow::Result<()> {
            self.record("create_site_dirs", slug.as_str().into())
        }
        async fn remove_site_dirs(&self, slug: &Slug) -> anyhow::Result<()> {
            self.record("remove_site_dirs", slug.as_str().into())
        }
        async fn trash_site_dirs(&self, slug: &Slug) -> anyhow::Result<()> {
            self.record("trash_site_dirs", slug.as_str().into())
        }
        async fn install_template(&self, slug: &Slug, _uid: u32, template: &str, _vars: &Vars, env: &Vars) -> anyhow::Result<Vec<Vec<String>>> {
            *self.env.lock().unwrap() = env.clone();
            self.record("install_template", format!("{} {template}", slug.as_str()))?;
            Ok(vec![vec!["core".into(), "install".into()]])
        }
        async fn sync_php(&self, users: &[SitePhp], pools: &[SitePhp]) -> anyhow::Result<Vec<String>> {
            self.record("sync_php", format!("users={} pools={}", slugs(users.iter().map(|u| &u.slug)), slugs(pools.iter().map(|p| &p.slug))))?;
            Ok(if self.reject_pool { pools.iter().map(|p| format!("{}.conf", p.slug.as_str())).collect() } else { vec![] })
        }
        async fn sync_caddy(&self, sites: &[CaddySite]) -> anyhow::Result<()> {
            self.record("sync_caddy", slugs(sites.iter().map(|s| &s.slug)))
        }
        async fn wp(&self, slug: &Slug, args: &[String]) -> anyhow::Result<String> {
            self.record("wp", format!("{} {}", slug.as_str(), args.join(" "))).map(|_| String::new())
        }
    }

    async fn ctx(host: FakeHost) -> Ctx<FakeHost> {
        Ctx {
            store: Store::open_memory().await.unwrap(),
            host,
            key: SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap(),
            db_host: "mariadb".into(),
            global_env: Vars::from([("SMTP_HOST".into(), "email-smtp.eu-west-1.amazonaws.com".into())]),
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
    }

    #[tokio::test]
    async fn failure_undoes_completed_steps_in_reverse_and_resyncs() {
        let c = ctx(FakeHost::failing("install_template")).await;
        let mut log = JobLog::default();
        assert!(create_site(&c, &req("sona.it"), &mut log).await.is_err());
        assert_eq!(
            c.host.ops(),
            ["create_database", "create_site_dirs", "install_template", "remove_site_dirs", "drop_database", "sync_php", "sync_caddy"]
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
            ]
        );
        assert!(c.store.site(site.id).await.is_err());
        assert!(c.store.site(keep.id).await.is_ok());
    }
}
