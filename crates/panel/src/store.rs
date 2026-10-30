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
text_enum!(OverrideKind { Php = "php", Caddy = "caddy" });
text_enum!(UpdateRule { Auto = "auto", Security = "security", Manual = "manual" });

#[derive(Debug, Clone, serde::Serialize)]
pub struct Override {
    pub id: i64,
    pub version: i64,
    pub content: String,
    pub author_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Alert {
    pub id: i64,
    pub site_id: Option<i64>,
    pub kind: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IpBlock {
    pub id: i64,
    pub site_id: Option<i64>,
    pub cidr: String,
    pub expires_at: Option<i64>,
    pub created_by: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Export {
    pub token: String,
    pub site_id: i64,
    pub file: String,
    pub size: i64,
    pub created_at: i64,
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
    pub staging: bool,
    pub update_rule: UpdateRule,
    pub log_retention_days: u32,
    pub settings: PhpSettingsRow,
}

/// Stessi default delle colonne di `sites` e di `PhpSettings`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PhpSettingsRow {
    pub memory_limit_mb: u32,
    pub upload_max_mb: u32,
    pub max_execution_time_s: u32,
    pub max_children: u32,
}

impl Default for PhpSettingsRow {
    fn default() -> Self {
        let d = PhpSettings::default();
        Self { memory_limit_mb: d.memory_limit_mb, upload_max_mb: d.upload_max_mb, max_execution_time_s: d.max_execution_time_s, max_children: d.max_children }
    }
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

const SITE_COLUMNS: &str = "id, slug, kind, template, status, staging, update_rule, log_retention_days, memory_limit_mb, upload_max_mb, max_execution_time_s, max_children";

fn site_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Site> {
    let decode = |e: alveare_core::site::InvalidSlug| StoreError::Db(sqlx::Error::Decode(e.to_string().into()));
    Ok(Site {
        id: r.try_get("id")?,
        slug: Slug::parse(r.try_get("slug")?).map_err(decode)?,
        kind: r.try_get::<&str, _>("kind")?.parse()?,
        template: r.try_get("template")?,
        status: r.try_get::<&str, _>("status")?.parse()?,
        staging: r.try_get("staging")?,
        update_rule: r.try_get::<&str, _>("update_rule")?.parse()?,
        log_retention_days: r.try_get("log_retention_days")?,
        settings: PhpSettingsRow {
            memory_limit_mb: r.try_get("memory_limit_mb")?,
            upload_max_mb: r.try_get("upload_max_mb")?,
            max_execution_time_s: r.try_get("max_execution_time_s")?,
            max_children: r.try_get("max_children")?,
        },
    })
}

const OVERRIDE_COLUMNS: &str = "id, version, content, author_id, created_at";

fn override_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Override> {
    Ok(Override { id: r.try_get("id")?, version: r.try_get("version")?, content: r.try_get("content")?, author_id: r.try_get("author_id")?, created_at: r.try_get("created_at")? })
}

fn export_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Export> {
    Ok(Export { token: r.try_get("token")?, site_id: r.try_get("site_id")?, file: r.try_get("file")?, size: r.try_get("size")?, created_at: r.try_get("created_at")? })
}

fn unique_violation(e: sqlx::Error, hostname: &Domain) -> StoreError {
    match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => StoreError::HostnameTaken(hostname.as_str().into()),
        _ => StoreError::Db(e),
    }
}

fn to_php(s: Site) -> SitePhp {
    SitePhp { uid: s.uid(), settings: (&s.settings).into(), slug: s.slug, extra: vec![] }
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

    /// `Some((password cifrata, hash bcrypt))` attiva la protezione dello staging, `None` la toglie.
    pub async fn set_staging(&self, id: i64, protection: Option<(&[u8], &str)>) -> Result<()> {
        let (enc, hash) = protection.unzip();
        sqlx::query("UPDATE sites SET staging = ?, staging_password_enc = ?, staging_hash = ? WHERE id = ?")
            .bind(enc.is_some())
            .bind(enc)
            .bind(hash)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_php_settings(&self, id: i64, p: &PhpSettingsRow) -> Result<()> {
        sqlx::query("UPDATE sites SET memory_limit_mb = ?, upload_max_mb = ?, max_execution_time_s = ?, max_children = ? WHERE id = ?")
            .bind(p.memory_limit_mb)
            .bind(p.upload_max_mb)
            .bind(p.max_execution_time_s)
            .bind(p.max_children)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_update_rule(&self, id: i64, rule: UpdateRule) -> Result<()> {
        sqlx::query("UPDATE sites SET update_rule = ? WHERE id = ?").bind(rule.as_str()).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_log_retention(&self, id: i64, days: u32) -> Result<()> {
        sqlx::query("UPDATE sites SET log_retention_days = ? WHERE id = ?").bind(days).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn staging_password(&self, id: i64) -> Result<Option<Vec<u8>>> {
        Ok(sqlx::query_scalar("SELECT staging_password_enc FROM sites WHERE id = ?").bind(id).fetch_one(&self.pool).await?)
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

    /// Toglie un alias o un redirect del sito; il dominio principale non si tocca.
    pub async fn remove_domain(&self, site_id: i64, hostname: &Domain) -> Result<()> {
        let done = sqlx::query("DELETE FROM domains WHERE site_id = ? AND hostname = ? AND kind != 'primary'")
            .bind(site_id)
            .bind(hostname.as_str())
            .execute(&self.pool)
            .await?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn hostname_taken(&self, hostname: &Domain) -> Result<bool> {
        Ok(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM domains WHERE hostname = ?").bind(hostname.as_str()).fetch_one(&self.pool).await? > 0)
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

    pub async fn add_override(&self, site_id: Option<i64>, kind: OverrideKind, content: &str, author: Option<i64>) -> Result<Override> {
        let mut tx = self.pool.begin().await?;
        let next: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) + 1 FROM config_overrides WHERE site_id IS ? AND kind = ?")
            .bind(site_id)
            .bind(kind.as_str())
            .fetch_one(&mut *tx)
            .await?;
        let id: i64 = sqlx::query_scalar("INSERT INTO config_overrides (site_id, kind, content, version, author_id) VALUES (?, ?, ?, ?, ?) RETURNING id")
            .bind(site_id)
            .bind(kind.as_str())
            .bind(content)
            .bind(next)
            .bind(author)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        self.override_by_id(id).await
    }

    async fn override_by_id(&self, id: i64) -> Result<Override> {
        let r = sqlx::query(&format!("SELECT {OVERRIDE_COLUMNS} FROM config_overrides WHERE id = ?")).bind(id).fetch_one(&self.pool).await?;
        override_from_row(&r)
    }

    pub async fn current_override(&self, site_id: Option<i64>, kind: OverrideKind) -> Result<Option<Override>> {
        Ok(self.override_history(site_id, kind).await?.into_iter().next())
    }

    /// Dalla più recente.
    pub async fn override_history(&self, site_id: Option<i64>, kind: OverrideKind) -> Result<Vec<Override>> {
        let rows = sqlx::query(&format!("SELECT {OVERRIDE_COLUMNS} FROM config_overrides WHERE site_id IS ? AND kind = ? ORDER BY version DESC"))
            .bind(site_id)
            .bind(kind.as_str())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(override_from_row).collect()
    }

    /// Solo per annullare una versione appena salvata che la validazione ha rifiutato.
    pub async fn remove_override(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM config_overrides WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    async fn php_extra(&self, site_id: i64) -> Result<Vec<String>> {
        Ok(self
            .current_override(Some(site_id), OverrideKind::Php)
            .await?
            .and_then(|o| alveare_core::php::parse_extra(&o.content).ok())
            .unwrap_or_default())
    }

    pub async fn add_block(&self, site_id: Option<i64>, cidr: &str, expires_at: Option<i64>, by: Option<i64>, now: i64) -> Result<i64> {
        Ok(sqlx::query_scalar("INSERT INTO ip_blocks (site_id, cidr, expires_at, created_by, created_at) VALUES (?, ?, ?, ?, ?) RETURNING id")
            .bind(site_id)
            .bind(cidr)
            .bind(expires_at)
            .bind(by)
            .bind(now)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Blocchi di un sito più quelli globali; `None`: tutti.
    pub async fn blocks(&self, site_id: Option<i64>) -> Result<Vec<IpBlock>> {
        let rows = match site_id {
            Some(id) => sqlx::query("SELECT * FROM ip_blocks WHERE site_id = ? OR site_id IS NULL ORDER BY id").bind(id).fetch_all(&self.pool).await?,
            None => sqlx::query("SELECT * FROM ip_blocks ORDER BY id").fetch_all(&self.pool).await?,
        };
        rows.iter()
            .map(|r| {
                Ok(IpBlock {
                    id: r.try_get("id")?,
                    site_id: r.try_get("site_id")?,
                    cidr: r.try_get("cidr")?,
                    expires_at: r.try_get("expires_at")?,
                    created_by: r.try_get("created_by")?,
                    created_at: r.try_get("created_at")?,
                })
            })
            .collect()
    }

    pub async fn block(&self, id: i64) -> Result<IpBlock> {
        self.blocks(None).await?.into_iter().find(|b| b.id == id).ok_or(StoreError::NotFound)
    }

    pub async fn remove_block(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM ip_blocks WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Toglie i blocchi scaduti; restituisce quanti (se > 0 va rigenerato il Caddyfile).
    pub async fn expire_blocks(&self, now: i64) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM ip_blocks WHERE expires_at IS NOT NULL AND expires_at <= ?").bind(now).execute(&self.pool).await?.rows_affected())
    }

    pub async fn add_export(&self, e: &Export) -> Result<()> {
        sqlx::query("INSERT INTO exports (token, site_id, file, size, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(&e.token)
            .bind(e.site_id)
            .bind(&e.file)
            .bind(e.size)
            .bind(e.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn export_by_token(&self, token: &str) -> Result<Option<Export>> {
        let r = sqlx::query("SELECT token, site_id, file, size, created_at FROM exports WHERE token = ?").bind(token).fetch_optional(&self.pool).await?;
        r.map(|r| export_from_row(&r)).transpose()
    }

    pub async fn exports(&self, site_id: i64) -> Result<Vec<Export>> {
        let rows = sqlx::query("SELECT token, site_id, file, size, created_at FROM exports WHERE site_id = ? ORDER BY created_at DESC").bind(site_id).fetch_all(&self.pool).await?;
        rows.iter().map(export_from_row).collect()
    }

    /// Toglie le esportazioni create prima di `before`; i file li cancella la pulizia notturna.
    pub async fn expire_exports(&self, before: i64) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM exports WHERE created_at < ?").bind(before).execute(&self.pool).await?.rows_affected())
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

    /// Siti serviti da Caddy: attivi, o sospesi con la pagina di manutenzione.
    pub async fn caddy_sites(&self) -> Result<Vec<CaddySite>> {
        let mut out = Vec::new();
        for site in self.sites().await?.into_iter().filter(|s| matches!(s.status, SiteStatus::Active | SiteStatus::Suspended)) {
            let domains = self.domains(site.id).await?;
            let of = |k: DomainKind| domains.iter().filter(|(_, dk)| *dk == k).map(|(d, _)| d.clone()).collect::<Vec<_>>();
            let primary = of(DomainKind::Primary).into_iter().next().ok_or(StoreError::NotFound)?;
            let staging_hash: Option<String> = sqlx::query_scalar("SELECT staging_hash FROM sites WHERE id = ?").bind(site.id).fetch_one(&self.pool).await?;
            let extra = self.current_override(Some(site.id), OverrideKind::Caddy).await?.map(|o| o.content);
            let blocked = self.blocks(Some(site.id)).await?.into_iter().map(|b| b.cidr).collect();
            out.push(CaddySite {
                slug: site.slug,
                primary,
                aliases: of(DomainKind::Alias),
                redirects: of(DomainKind::Redirect),
                suspended: site.status == SiteStatus::Suspended,
                staging_hash,
                log_days: site.log_retention_days,
                extra,
                blocked,
            });
        }
        Ok(out)
    }

    /// Tutti i siti con file su disco: servono a passwd e group.
    pub async fn php_sites(&self) -> Result<Vec<SitePhp>> {
        self.with_extra(self.sites().await?).await
    }

    /// Siti con un pool FPM attivo: non sospesi né in eliminazione.
    pub async fn pooled_sites(&self) -> Result<Vec<SitePhp>> {
        let sites = self.sites().await?.into_iter().filter(|s| !matches!(s.status, SiteStatus::Suspended | SiteStatus::Deleting)).collect();
        self.with_extra(sites).await
    }

    async fn with_extra(&self, sites: Vec<Site>) -> Result<Vec<SitePhp>> {
        let mut out = Vec::new();
        for s in sites {
            let extra = self.php_extra(s.id).await?;
            out.push(SitePhp { extra, ..to_php(s) });
        }
        Ok(out)
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
    async fn suspended_and_staging_sites_reach_caddy_with_their_mode() {
        let s = store().await;
        let a = s.create_site(&new_site("a.it")).await.unwrap();
        let b = s.create_site(&new_site("b.it")).await.unwrap();
        s.set_status(a.id, SiteStatus::Suspended).await.unwrap();
        s.set_status(b.id, SiteStatus::Active).await.unwrap();
        s.set_staging(b.id, Some((b"cifrata", "$2b$10$x"))).await.unwrap();
        let sites = s.caddy_sites().await.unwrap();
        assert_eq!(sites.len(), 2);
        assert!(sites[0].suspended && sites[0].staging_hash.is_none());
        assert!(!sites[1].suspended);
        assert_eq!(sites[1].staging_hash.as_deref(), Some("$2b$10$x"));
        assert!(s.site(b.id).await.unwrap().staging);
        assert_eq!(s.staging_password(b.id).await.unwrap().as_deref(), Some(&b"cifrata"[..]));
        s.set_staging(b.id, None).await.unwrap();
        assert!(!s.site(b.id).await.unwrap().staging);
        assert!(s.caddy_sites().await.unwrap()[1].staging_hash.is_none());
    }

    #[tokio::test]
    async fn overrides_are_versioned_per_site_and_global() {
        let s = store().await;
        let a = s.create_site(&new_site("a.it")).await.unwrap();
        assert!(s.current_override(Some(a.id), OverrideKind::Php).await.unwrap().is_none());
        assert_eq!(s.add_override(Some(a.id), OverrideKind::Php, "php_value[a] = 1", Some(7)).await.unwrap().version, 1);
        let v2 = s.add_override(Some(a.id), OverrideKind::Php, "php_value[a] = 2", Some(7)).await.unwrap();
        assert_eq!(v2.version, 2);
        assert_eq!(s.add_override(None, OverrideKind::Php, "memory_limit = 1G", None).await.unwrap().version, 1, "il globale ha la sua numerazione");
        let cur = s.current_override(Some(a.id), OverrideKind::Php).await.unwrap().unwrap();
        assert_eq!((cur.version, cur.content.as_str(), cur.author_id), (2, "php_value[a] = 2", Some(7)));
        let history = s.override_history(Some(a.id), OverrideKind::Php).await.unwrap();
        assert_eq!(history.iter().map(|o| o.version).collect::<Vec<_>>(), [2, 1]);
        assert!(s.current_override(Some(a.id), OverrideKind::Caddy).await.unwrap().is_none());
        // Le direttive attive finiscono nel pool.
        let php = s.php_sites().await.unwrap();
        assert_eq!(php[0].extra, ["php_value[a] = 2"]);
        s.remove_override(v2.id).await.unwrap();
        assert_eq!(s.current_override(Some(a.id), OverrideKind::Php).await.unwrap().unwrap().version, 1);
    }

    #[tokio::test]
    async fn ip_blocks_reach_caddy_until_they_expire() {
        let s = store().await;
        let a = s.create_site(&new_site("a.it")).await.unwrap();
        let b = s.create_site(&new_site("b.it")).await.unwrap();
        for id in [a.id, b.id] {
            s.set_status(id, SiteStatus::Active).await.unwrap();
        }
        s.add_block(Some(a.id), "198.51.100.9", Some(1000), Some(1), 10).await.unwrap();
        s.add_block(None, "203.0.113.0/24", None, Some(1), 10).await.unwrap();
        let caddy = s.caddy_sites().await.unwrap();
        assert_eq!(caddy[0].blocked, ["198.51.100.9", "203.0.113.0/24"]);
        assert_eq!(caddy[1].blocked, ["203.0.113.0/24"], "il blocco globale vale per tutti");
        assert_eq!(s.blocks(Some(a.id)).await.unwrap().len(), 2, "del sito e globali");
        assert_eq!(s.expire_blocks(999).await.unwrap(), 0);
        assert_eq!(s.expire_blocks(1000).await.unwrap(), 1);
        assert_eq!(s.caddy_sites().await.unwrap()[0].blocked, ["203.0.113.0/24"]);
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
