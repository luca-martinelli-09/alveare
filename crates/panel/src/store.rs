//! Stato del pannello in SQLite: l'unica fonte di verità da cui si rigenera tutto il resto.
use std::str::FromStr;

use alveare_core::caddy::CaddySite;
use alveare_core::php::{PhpSettings, SitePhp, FIRST_SITE_UID};
use alveare_core::site::{slug_for, Domain, Slug};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("l'hostname {0} è già usato da un sito")]
    HostnameTaken(String),
    #[error("sito non trovato")]
    NotFound,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

macro_rules! text_enum {
    ($name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl FromStr for $name {
            type Err = sqlx::Error;
            fn from_str(s: &str) -> std::result::Result<Self, sqlx::Error> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    other => Err(sqlx::Error::Decode(format!("valore sconosciuto: {other}").into())),
                }
            }
        }
    };
}

text_enum!(SiteKind { Wordpress = "wordpress", Php = "php", Laravel = "laravel" });
text_enum!(SiteStatus { Creating = "creating", Active = "active", Suspended = "suspended", Error = "error", Deleting = "deleting" });
text_enum!(DomainKind { Primary = "primary", Alias = "alias", Redirect = "redirect" });

#[derive(Debug, Clone, serde::Serialize)]
pub struct Alert {
    pub id: i64,
    pub site_id: Option<i64>,
    pub kind: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupRun {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub snapshot_id: Option<String>,
    pub bytes_added: Option<i64>,
    pub total_bytes: Option<i64>,
    pub error: Option<String>,
}

pub struct NewSite {
    pub primary: Domain,
    pub kind: SiteKind,
    pub template: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Site {
    pub id: i64,
    pub slug: Slug,
    pub kind: SiteKind,
    pub template: String,
    pub status: SiteStatus,
    pub settings: PhpSettingsRow,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PhpSettingsRow {
    pub memory_limit_mb: u32,
    pub upload_max_mb: u32,
    pub max_execution_time_s: u32,
    pub max_children: u32,
}

impl From<&PhpSettingsRow> for PhpSettings {
    fn from(r: &PhpSettingsRow) -> Self {
        Self {
            memory_limit_mb: r.memory_limit_mb,
            upload_max_mb: r.upload_max_mb,
            max_execution_time_s: r.max_execution_time_s,
            max_children: r.max_children,
        }
    }
}

impl Site {
    pub fn uid(&self) -> u32 {
        FIRST_SITE_UID + self.id as u32
    }
}

const SITE_COLUMNS: &str = "id, slug, kind, template, status, memory_limit_mb, upload_max_mb, max_execution_time_s, max_children";

fn site_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Site> {
    let decode = |e: alveare_core::site::InvalidSlug| StoreError::Db(sqlx::Error::Decode(e.to_string().into()));
    Ok(Site {
        id: r.try_get("id")?,
        slug: Slug::parse(r.try_get("slug")?).map_err(decode)?,
        kind: r.try_get::<&str, _>("kind")?.parse()?,
        template: r.try_get("template")?,
        status: r.try_get::<&str, _>("status")?.parse()?,
        settings: PhpSettingsRow {
            memory_limit_mb: r.try_get("memory_limit_mb")?,
            upload_max_mb: r.try_get("upload_max_mb")?,
            max_execution_time_s: r.try_get("max_execution_time_s")?,
            max_children: r.try_get("max_children")?,
        },
    })
}

fn unique_violation(e: sqlx::Error, hostname: &Domain) -> StoreError {
    match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => StoreError::HostnameTaken(hostname.as_str().into()),
        _ => StoreError::Db(e),
    }
}

fn to_php(s: Site) -> SitePhp {
    SitePhp { uid: s.uid(), settings: (&s.settings).into(), slug: s.slug }
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &std::path::Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        Self::with_pool(SqlitePoolOptions::new().connect_with(opts).await?).await
    }

    #[cfg(test)]
    pub async fn open_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        // Una sola connessione: ogni connessione a :memory: è un database diverso.
        Self::with_pool(SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?).await
    }

    async fn with_pool(pool: SqlitePool) -> Result<Self> {
        sqlx::migrate!().run(&pool).await.map_err(|e| StoreError::Db(e.into()))?;
        Ok(Self { pool })
    }

    /// Su un pool già migrato.
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn create_site(&self, new: &NewSite) -> Result<Site> {
        let mut tx = self.pool.begin().await?;
        let taken: Vec<String> = sqlx::query_scalar("SELECT slug FROM sites").fetch_all(&mut *tx).await?;
        let slug = slug_for(&new.primary, |c| taken.iter().any(|t| t == c));
        let id: i64 = sqlx::query_scalar("INSERT INTO sites (slug, kind, template) VALUES (?, ?, ?) RETURNING id")
            .bind(slug.as_str())
            .bind(new.kind.as_str())
            .bind(&new.template)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO domains (site_id, hostname, kind) VALUES (?, ?, 'primary')")
            .bind(id)
            .bind(new.primary.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| unique_violation(e, &new.primary))?;
        tx.commit().await?;
        self.site(id).await
    }

    pub async fn site(&self, id: i64) -> Result<Site> {
        let row = sqlx::query(&format!("SELECT {SITE_COLUMNS} FROM sites WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::NotFound)?;
        site_from_row(&row)
    }

    pub async fn sites(&self) -> Result<Vec<Site>> {
        sqlx::query(&format!("SELECT {SITE_COLUMNS} FROM sites ORDER BY slug"))
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(site_from_row)
            .collect()
    }

    pub async fn delete_site(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM sites WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_status(&self, id: i64, status: SiteStatus) -> Result<()> {
        sqlx::query("UPDATE sites SET status = ? WHERE id = ?").bind(status.as_str()).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn add_domain(&self, site_id: i64, hostname: &Domain, kind: DomainKind) -> Result<()> {
        sqlx::query("INSERT INTO domains (site_id, hostname, kind) VALUES (?, ?, ?)")
            .bind(site_id)
            .bind(hostname.as_str())
            .bind(kind.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| unique_violation(e, hostname))?;
        Ok(())
    }

    pub async fn domains(&self, site_id: i64) -> Result<Vec<(Domain, DomainKind)>> {
        let rows = sqlx::query("SELECT hostname, kind FROM domains WHERE site_id = ? ORDER BY kind = 'primary' DESC, hostname")
            .bind(site_id)
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                let host = Domain::parse(r.try_get("hostname")?)
                    .map_err(|_| sqlx::Error::Decode("hostname non valido nel database".into()))?;
                Ok((host, r.try_get::<&str, _>("kind")?.parse()?))
            })
            .collect()
    }

    pub async fn save_db_creds(&self, site_id: i64, name: &str, user: &str, password_enc: &[u8]) -> Result<()> {
        sqlx::query("INSERT INTO site_databases (site_id, db_name, db_user, password_enc) VALUES (?, ?, ?, ?)")
            .bind(site_id)
            .bind(name)
            .bind(user)
            .bind(password_enc)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_db_password(&self, site_id: i64, password_enc: &[u8]) -> Result<()> {
        sqlx::query("UPDATE site_databases SET password_enc = ? WHERE site_id = ?").bind(password_enc).bind(site_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save_redis_password(&self, site_id: i64, enc: &[u8]) -> Result<()> {
        sqlx::query("UPDATE site_databases SET redis_password_enc = ? WHERE site_id = ?").bind(enc).bind(site_id).execute(&self.pool).await?;
        Ok(())
    }

    /// (slug, password Redis cifrata) dei siti che ne hanno una.
    pub async fn redis_users(&self) -> Result<Vec<(Slug, Vec<u8>)>> {
        let rows = sqlx::query("SELECT s.slug, d.redis_password_enc FROM sites s JOIN site_databases d ON d.site_id = s.id WHERE d.redis_password_enc IS NOT NULL ORDER BY s.slug")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                let slug = Slug::parse(r.try_get("slug")?).map_err(|e| StoreError::Db(sqlx::Error::Decode(e.to_string().into())))?;
                Ok((slug, r.try_get("redis_password_enc")?))
            })
            .collect()
    }

    /// (nome DB, utente DB, password cifrata)
    pub async fn db_creds(&self, site_id: i64) -> Result<(String, String, Vec<u8>)> {
        let r = sqlx::query("SELECT db_name, db_user, password_enc FROM site_databases WHERE site_id = ?")
            .bind(site_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::NotFound)?;
        Ok((r.try_get("db_name")?, r.try_get("db_user")?, r.try_get("password_enc")?))
    }

    /// Un solo avviso aperto per sito e tipo: il nuovo messaggio sostituisce il vecchio.
    pub async fn raise_alert(&self, site_id: Option<i64>, kind: &str, message: &str) -> Result<()> {
        let updated = sqlx::query("UPDATE alerts SET message = ?, created_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE site_id IS ? AND kind = ? AND resolved_at IS NULL")
            .bind(message)
            .bind(site_id)
            .bind(kind)
            .execute(&self.pool)
            .await?;
        if updated.rows_affected() == 0 {
            sqlx::query("INSERT INTO alerts (site_id, kind, message) VALUES (?, ?, ?)").bind(site_id).bind(kind).bind(message).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn resolve_alerts(&self, site_id: i64, kind: &str) -> Result<()> {
        sqlx::query("UPDATE alerts SET resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE site_id = ? AND kind = ? AND resolved_at IS NULL")
            .bind(site_id)
            .bind(kind)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn resolve_global_alerts(&self, kind: &str) -> Result<()> {
        sqlx::query("UPDATE alerts SET resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE site_id IS NULL AND kind = ? AND resolved_at IS NULL")
            .bind(kind)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// `visible = None`: tutti (admin), compresi gli avvisi globali.
    pub async fn open_alerts(&self, visible: Option<&[i64]>) -> Result<Vec<Alert>> {
        let rows = sqlx::query("SELECT id, site_id, kind, message, created_at FROM alerts WHERE resolved_at IS NULL ORDER BY id DESC")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::new();
        for r in rows {
            let site_id: Option<i64> = r.try_get("site_id")?;
            if visible.is_none_or(|v| site_id.is_some_and(|s| v.contains(&s))) {
                out.push(Alert { id: r.try_get("id")?, site_id, kind: r.try_get("kind")?, message: r.try_get("message")?, created_at: r.try_get("created_at")? });
            }
        }
        Ok(out)
    }

    pub async fn retention(&self, site_id: i64) -> Result<crate::backup::Retention> {
        let r = sqlx::query("SELECT keep_daily, keep_weekly, keep_monthly FROM backup_policies WHERE site_id = ?")
            .bind(site_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match r {
            Some(r) => crate::backup::Retention { daily: r.try_get("keep_daily")?, weekly: r.try_get("keep_weekly")?, monthly: r.try_get("keep_monthly")? },
            None => crate::backup::Retention::default(),
        })
    }

    pub async fn backup_started(&self, site_id: i64, slug: &Slug) -> Result<i64> {
        Ok(sqlx::query_scalar("INSERT INTO backup_runs (site_id, slug) VALUES (?, ?) RETURNING id").bind(site_id).bind(slug.as_str()).fetch_one(&self.pool).await?)
    }

    pub async fn backup_finished(&self, run: i64, outcome: std::result::Result<&crate::backup::Snapshot, String>) -> Result<()> {
        let q = match outcome {
            Ok(s) => sqlx::query("UPDATE backup_runs SET status = 'done', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), snapshot_id = ?, bytes_added = ?, total_bytes = ? WHERE id = ?")
                .bind(s.snapshot_id.clone())
                .bind(s.bytes_added as i64)
                .bind(s.total_bytes as i64)
                .bind(run),
            Err(e) => sqlx::query("UPDATE backup_runs SET status = 'failed', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), error = ? WHERE id = ?").bind(e).bind(run),
        };
        q.execute(&self.pool).await?;
        Ok(())
    }

    pub async fn backup_runs(&self, site_id: i64, limit: i64) -> Result<Vec<BackupRun>> {
        let rows = sqlx::query("SELECT id, started_at, finished_at, status, snapshot_id, bytes_added, total_bytes, error FROM backup_runs WHERE site_id = ? ORDER BY id DESC LIMIT ?")
            .bind(site_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(BackupRun {
                    id: r.try_get("id")?,
                    started_at: r.try_get("started_at")?,
                    finished_at: r.try_get("finished_at")?,
                    status: r.try_get("status")?,
                    snapshot_id: r.try_get("snapshot_id")?,
                    bytes_added: r.try_get("bytes_added")?,
                    total_bytes: r.try_get("total_bytes")?,
                    error: r.try_get("error")?,
                })
            })
            .collect()
    }

    /// Slug con backup ma senza più un sito: i loro snapshot scadono dopo 30 giorni.
    pub async fn orphan_backup_slugs(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar("SELECT DISTINCT slug FROM backup_runs WHERE slug NOT IN (SELECT slug FROM sites)").fetch_all(&self.pool).await?)
    }

    pub async fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(&self.pool).await?)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Copia coerente del database del pannello, anche mentre è in uso.
    pub async fn snapshot_to(&self, dest: &std::path::Path) -> Result<()> {
        let _ = std::fs::remove_file(dest);
        sqlx::query("VACUUM INTO ?").bind(dest.to_string_lossy().into_owned()).execute(&self.pool).await?;
        Ok(())
    }

    /// Siti serviti da Caddy: solo quelli attivi.
    pub async fn caddy_sites(&self) -> Result<Vec<CaddySite>> {
        let mut out = Vec::new();
        for site in self.sites().await?.into_iter().filter(|s| s.status == SiteStatus::Active) {
            let domains = self.domains(site.id).await?;
            let of = |k: DomainKind| domains.iter().filter(|(_, dk)| *dk == k).map(|(d, _)| d.clone()).collect::<Vec<_>>();
            let primary = of(DomainKind::Primary).into_iter().next().ok_or(StoreError::NotFound)?;
            out.push(CaddySite { slug: site.slug, primary, aliases: of(DomainKind::Alias), redirects: of(DomainKind::Redirect) });
        }
        Ok(out)
    }

    /// Tutti i siti con file su disco: servono a passwd e group.
    pub async fn php_sites(&self) -> Result<Vec<SitePhp>> {
        Ok(self.sites().await?.into_iter().map(to_php).collect())
    }

    /// Siti con un pool FPM attivo: non sospesi né in eliminazione.
    pub async fn pooled_sites(&self) -> Result<Vec<SitePhp>> {
        Ok(self
            .sites()
            .await?
            .into_iter()
            .filter(|s| !matches!(s.status, SiteStatus::Suspended | SiteStatus::Deleting))
            .map(to_php)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alveare_core::site::Domain;

    async fn store() -> Store {
        Store::open_memory().await.unwrap()
    }

    fn d(s: &str) -> Domain {
        Domain::parse(s).unwrap()
    }

    fn new_site(domain: &str) -> NewSite {
        NewSite { primary: d(domain), kind: SiteKind::Wordpress, template: "wordpress".into() }
    }

    #[tokio::test]
    async fn created_site_gets_slug_and_uid_from_id() {
        let s = store().await;
        let a = s.create_site(&new_site("sona.it")).await.unwrap();
        let b = s.create_site(&new_site("upupa.it")).await.unwrap();
        assert_eq!(a.slug.as_str(), "sona_it");
        assert_eq!(a.uid(), 20001);
        assert_eq!(b.uid(), 20002);
        assert_eq!(a.status, SiteStatus::Creating);
        assert_eq!(s.domains(a.id).await.unwrap(), vec![(d("sona.it"), DomainKind::Primary)]);
    }

    #[tokio::test]
    async fn uid_is_never_reused_after_delete() {
        let s = store().await;
        let a = s.create_site(&new_site("sona.it")).await.unwrap();
        s.delete_site(a.id).await.unwrap();
        let b = s.create_site(&new_site("sona.it")).await.unwrap();
        assert_eq!(b.uid(), 20002);
    }

    #[tokio::test]
    async fn slug_collision_gets_suffix() {
        let s = store().await;
        s.create_site(&new_site("sona.it")).await.unwrap();
        let b = s.create_site(&new_site("www.sona-it.com")).await.unwrap();
        assert_eq!(b.slug.as_str(), "sona_it_com");
        let f = s.create_site(&new_site("sona-it.com")).await.unwrap();
        assert_eq!(f.slug.as_str(), "sona_it_com_2");
    }

    #[tokio::test]
    async fn hostname_cannot_belong_to_two_sites() {
        let s = store().await;
        let a = s.create_site(&new_site("sona.it")).await.unwrap();
        s.add_domain(a.id, &d("www.sona.it"), DomainKind::Redirect).await.unwrap();
        assert!(matches!(s.create_site(&new_site("www.sona.it")).await, Err(StoreError::HostnameTaken(_))));
        assert!(matches!(s.add_domain(a.id, &d("sona.it"), DomainKind::Alias).await, Err(StoreError::HostnameTaken(_))));
    }

    #[tokio::test]
    async fn caddy_config_lists_active_sites_with_their_domains() {
        let s = store().await;
        let a = s.create_site(&new_site("sona.it")).await.unwrap();
        s.add_domain(a.id, &d("www.sona.it"), DomainKind::Redirect).await.unwrap();
        s.add_domain(a.id, &d("comune-sona.it"), DomainKind::Alias).await.unwrap();
        s.set_status(a.id, SiteStatus::Active).await.unwrap();
        s.create_site(&new_site("in-creazione.it")).await.unwrap();
        let sites = s.caddy_sites().await.unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].primary, d("sona.it"));
        assert_eq!(sites[0].aliases, vec![d("comune-sona.it")]);
        assert_eq!(sites[0].redirects, vec![d("www.sona.it")]);
    }

    #[tokio::test]
    async fn pooled_sites_exclude_suspended_and_deleting() {
        let s = store().await;
        for (domain, status) in [("a.it", SiteStatus::Active), ("b.it", SiteStatus::Suspended), ("c.it", SiteStatus::Deleting), ("d.it", SiteStatus::Creating)] {
            let site = s.create_site(&new_site(domain)).await.unwrap();
            s.set_status(site.id, status).await.unwrap();
        }
        let slugs: Vec<String> = s.pooled_sites().await.unwrap().into_iter().map(|p| p.slug.as_str().to_string()).collect();
        assert_eq!(slugs, ["a_it", "d_it"]);
    }

    #[tokio::test]
    async fn alerts_are_deduplicated_resolved_and_filtered() {
        let s = store().await;
        let a = s.create_site(&new_site("a.it")).await.unwrap();
        let b = s.create_site(&new_site("b.it")).await.unwrap();
        s.raise_alert(Some(a.id), "integrity", "file modificati: 1").await.unwrap();
        s.raise_alert(Some(a.id), "integrity", "file modificati: 2").await.unwrap();
        s.raise_alert(Some(b.id), "integrity", "x").await.unwrap();
        let open = s.open_alerts(None).await.unwrap();
        assert_eq!(open.len(), 2);
        let mine = s.open_alerts(Some(&[a.id])).await.unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].message, "file modificati: 2");
        s.resolve_alerts(a.id, "integrity").await.unwrap();
        assert!(s.open_alerts(Some(&[a.id])).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn php_sites_include_every_site_with_files_on_disk() {
        let s = store().await;
        let a = s.create_site(&new_site("sona.it")).await.unwrap();
        s.set_status(a.id, SiteStatus::Active).await.unwrap();
        s.create_site(&new_site("nuovo.it")).await.unwrap();
        let php = s.php_sites().await.unwrap();
        assert_eq!(php.len(), 2);
        let sona = php.iter().find(|p| p.slug.as_str() == "sona_it").unwrap();
        assert_eq!(sona.uid, 20001);
        assert_eq!(sona.settings, alveare_core::php::PhpSettings::default());
    }
}
