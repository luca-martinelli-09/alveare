//! Doppioni per i test: un host che registra le chiamate invece di toccare il sistema.
use std::sync::Mutex;

use alveare_core::caddy::CaddySite;
use alveare_core::php::SitePhp;
use alveare_core::site::Slug;

use std::pin::Pin;

use tokio::io::AsyncRead;

use crate::backup::{Retention, Snapshot};
use crate::database::TableInfo;
use crate::flows::{DbCreds, Dump, Host, Installed};
use crate::template::Vars;

#[derive(Default)]
pub struct FakeHost {
    pub calls: Mutex<Vec<String>>,
    pub fail_on: Option<&'static str>,
    pub reject_pool: bool,
    pub env: Mutex<Vars>,
    pub snapshot_sql: Option<std::path::PathBuf>,
}

impl FakeHost {
    pub fn failing(op: &'static str) -> Self {
        Self { fail_on: Some(op), ..Default::default() }
    }
    fn record(&self, op: &str, detail: String) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(if detail.is_empty() { op.to_string() } else { format!("{op} {detail}") });
        if self.fail_on == Some(op) {
            anyhow::bail!("{op} fallito");
        }
        Ok(())
    }
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    pub fn ops(&self) -> Vec<String> {
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
    async fn install_template(&self, slug: &Slug, _uid: u32, template: &str, _vars: &Vars, env: &Vars) -> anyhow::Result<Installed> {
        *self.env.lock().unwrap() = env.clone();
        self.record("install_template", format!("{} {template}", slug.as_str()))?;
        let sql = self.snapshot_sql.clone().map(|p| (p, "template.local".to_string()));
        Ok(Installed { commands: vec![vec!["core".into(), "install".into()]], sql })
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
    async fn wp_output(&self, slug: &Slug, args: &[String]) -> anyhow::Result<(bool, String)> {
        self.record("wp", format!("{} {}", slug.as_str(), args.join(" "))).map(|_| (true, String::new()))
    }
    async fn reopen_fpm_logs(&self) -> anyhow::Result<()> {
        self.record("reopen_fpm_logs", String::new())
    }
    async fn db_tables(&self, name: &str) -> anyhow::Result<Vec<TableInfo>> {
        self.record("db_tables", name.to_string())?;
        Ok(vec![TableInfo { name: "wp_posts".into(), rows: 10, bytes: 4096 }])
    }
    async fn db_dump(&self, name: &str, tables: &[String]) -> anyhow::Result<Dump> {
        self.record("db_dump", format!("{name} {}", tables.join(",")))?;
        Ok(Dump { reader: Box::pin(std::io::Cursor::new(b"-- dump finto\n".to_vec())), finished: Box::pin(async { Ok(()) }) })
    }
    async fn db_import(&self, name: &str, mut sql: Pin<Box<dyn AsyncRead + Send>>) -> anyhow::Result<()> {
        let mut text = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut sql, &mut text).await?;
        self.record("db_import", format!("{name} {}", text.trim()))
    }
    async fn backup(&self, slug: &Slug, db_name: &str) -> anyhow::Result<Snapshot> {
        self.record("backup", format!("{} {db_name}", slug.as_str()))?;
        Ok(Snapshot { snapshot_id: "abcdef12".into(), bytes_added: 100, total_bytes: 1000 })
    }
    async fn prune(&self, slug: &Slug, keep: Retention) -> anyhow::Result<()> {
        self.record("prune", format!("{} {}", slug.as_str(), keep.daily))
    }
    async fn expire_deleted(&self, slug: &str) -> anyhow::Result<()> {
        self.record("expire_deleted", slug.to_string())
    }
    async fn restore(&self, slug: &Slug, snapshot: &str, files: bool, db: Option<&DbCreds>) -> anyhow::Result<()> {
        self.record("restore", format!("{} {snapshot} files={files} db={}", slug.as_str(), db.map_or("-", |d| d.name.as_str())))
    }
    async fn backup_panel(&self, _store: &crate::store::Store, _env: &[u8]) -> anyhow::Result<Snapshot> {
        self.record("backup_panel", String::new())?;
        Ok(Snapshot { snapshot_id: "panel123".into(), bytes_added: 1, total_bytes: 1 })
    }
    async fn sync_redis(&self, users: &[alveare_core::redis::RedisUser]) -> anyhow::Result<()> {
        self.record("sync_redis", slugs(users.iter().map(|u| &u.slug)))
    }
    async fn adminer_grant(&self, db: &DbCreds) -> anyhow::Result<String> {
        self.record("adminer_grant", db.name.clone())?;
        Ok("t".repeat(40))
    }
    async fn set_db_password(&self, user: &str, password: &str) -> anyhow::Result<()> {
        self.env.lock().unwrap().insert("DB_PASSWORD".into(), password.into());
        self.record("set_db_password", user.to_string())
    }
    async fn copy_site_files(&self, from: &Slug, to: &Slug, _uid: u32) -> anyhow::Result<()> {
        self.record("copy_site_files", format!("{} {}", from.as_str(), to.as_str()))
    }
    async fn read_site_env(&self, slug: &Slug) -> anyhow::Result<Vars> {
        self.record("read_site_env", slug.as_str().into())?;
        Ok(self.env.lock().unwrap().clone())
    }
    async fn write_site_env(&self, slug: &Slug, _uid: u32, env: &Vars) -> anyhow::Result<()> {
        *self.env.lock().unwrap() = env.clone();
        self.record("write_site_env", slug.as_str().into())
    }
    async fn set_site_env(&self, slug: &Slug, _uid: u32, key: &str, value: &str) -> anyhow::Result<()> {
        self.env.lock().unwrap().insert(format!("SITE_ENV {key}"), value.into());
        self.record("set_site_env", format!("{} {key}", slug.as_str()))
    }
}

