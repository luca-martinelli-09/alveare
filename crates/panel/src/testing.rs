//! Doppioni per i test: un host che registra le chiamate invece di toccare il sistema.
use std::sync::Mutex;

use alveare_core::caddy::CaddySite;
use alveare_core::php::SitePhp;
use alveare_core::site::Slug;

use std::pin::Pin;

use tokio::io::AsyncRead;

use crate::backup::{Retention, Snapshot};
use crate::database::TableInfo;
use crate::flows::{DbCreds, Dump, FilesScope, Host, Imported, Installed};
use crate::template::Vars;

#[derive(Default)]
pub struct FakeHost {
    pub calls: Mutex<Vec<String>>,
    pub fail_on: Option<&'static str>,
    pub reject_pool: bool,
    pub env: Mutex<Vars>,
    pub snapshot_sql: Option<std::path::PathBuf>,
    pub imported: Option<Imported>,
    /// Risposte di `wp` per argomenti precisi ("option get home" → "https://...").
    pub wp_replies: Vec<(&'static str, &'static str)>,
    /// (.env.example, LEGGIMI.txt) dell'ultima esportazione.
    pub exported: Mutex<Option<(String, String)>>,
    pub stored_manifest: Mutex<Option<crate::template::Manifest>>,
    /// Stato HTTP restituito da `probe` (default 200).
    pub probe_status: Option<u16>,
    /// JSON restituito da `caddy_adapt` (default `{}`).
    pub adapted: Mutex<Option<serde_json::Value>>,
    /// Override globale dell'ultimo `sync_caddy`.
    pub caddy_global: Mutex<Option<String>>,
    /// Account Cloudflare configurati (default: nessuno, hostname a mano).
    pub cf_accounts: Vec<String>,
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
    async fn sync_caddy(&self, sites: &[CaddySite], global: Option<&str>) -> anyhow::Result<()> {
        *self.caddy_global.lock().unwrap() = global.map(String::from);
        self.record("sync_caddy", slugs(sites.iter().map(|s| &s.slug)))
    }
    async fn caddy_adapt(&self, _caddyfile: &str) -> anyhow::Result<serde_json::Value> {
        self.record("caddy_adapt", String::new())?;
        Ok(self.adapted.lock().unwrap().clone().unwrap_or_else(|| serde_json::json!({})))
    }
    async fn write_php_global(&self, ini: &str) -> anyhow::Result<()> {
        self.record("write_php_global", ini.to_string())
    }
    async fn wp(&self, slug: &Slug, args: &[String]) -> anyhow::Result<String> {
        self.record("wp", format!("{} {}", slug.as_str(), args.join(" ")))?;
        let joined = args.join(" ");
        Ok(self.wp_replies.iter().find(|(a, _)| *a == joined).map(|(_, r)| r.to_string()).unwrap_or_default())
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
    async fn restore(&self, slug: &Slug, snapshot: &str, target: &Slug, _uid: u32, files: &FilesScope, db: Option<&DbCreds>) -> anyhow::Result<()> {
        self.record("restore", format!("{} {snapshot} -> {} files={files} db={}", slug.as_str(), target.as_str(), db.map_or("-", |d| d.name.as_str())))
    }
    async fn snapshot_ls(&self, slug: &Slug, snapshot: &str, rel: &str) -> anyhow::Result<Vec<crate::backup::SnapEntry>> {
        self.record("snapshot_ls", format!("{} {snapshot} {rel}", slug.as_str()))?;
        Ok(vec![crate::backup::SnapEntry { name: "wp-content".into(), is_dir: true, size: 0, mtime: String::new() }])
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
    async fn import_files(&self, slug: &Slug, _uid: u32, archive: &std::path::Path) -> anyhow::Result<Imported> {
        self.record("import_files", format!("{} {}", slug.as_str(), archive.display()))?;
        Ok(self.imported.clone().unwrap_or_default())
    }
    async fn export_site(&self, slug: &Slug, db_name: &str, dest: &std::path::Path, env_example: &str, readme: &str) -> anyhow::Result<u64> {
        *self.exported.lock().unwrap() = Some((env_example.into(), readme.into()));
        self.record("export_site", format!("{} {db_name} {}", slug.as_str(), dest.display()))?;
        Ok(1234)
    }
    async fn store_template(&self, slug: &Slug, name: &Slug, manifest: &crate::template::Manifest, sql_file: &str) -> anyhow::Result<()> {
        *self.stored_manifest.lock().unwrap() = Some(manifest.clone());
        self.record("store_template", format!("{} {} {sql_file}", slug.as_str(), name.as_str()))
    }
    async fn probe(&self, host: &alveare_core::site::Domain, auth: Option<(&str, &str)>) -> anyhow::Result<u16> {
        self.record("probe", format!("{} {}", host.as_str(), auth.map_or("-".to_string(), |(u, _)| format!("{u}:"))))?;
        Ok(self.probe_status.unwrap_or(200))
    }
    async fn sync_workers(&self, specs: &[alveare_core::agent::WorkerSpec]) -> anyhow::Result<()> {
        let list: Vec<String> = specs.iter().map(|w| format!("{}/{}:{} x{} {}MB", w.slug.as_str(), w.name, w.args.join(" "), w.processes, w.memory_mb)).collect();
        self.record("sync_workers", list.join(","))
    }
    async fn worker_restart(&self, slug: &Slug, name: &str) -> anyhow::Result<()> {
        self.record("worker_restart", format!("{} {name}", slug.as_str()))
    }
    async fn workers_status(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!([{ "slug": "sona_it", "name": "horizon", "index": 0, "pid": 42, "uptime_s": 10, "restarts": 0, "rss_mb": 80 }]))
    }
    async fn deploy(&self, slug: &Slug, repo: &str, branch: &str) -> anyhow::Result<(bool, String)> {
        self.record("deploy", format!("{} {repo} {branch}", slug.as_str()))?;
        Ok((true, "$ git fetch\nfatto".into()))
    }
    async fn deploy_key(&self, slug: &Slug) -> anyhow::Result<String> {
        self.record("deploy_key", slug.as_str().into())?;
        Ok("ssh-ed25519 AAAAC3 alveare-sona_it".into())
    }
    async fn caddy_metrics(&self) -> anyhow::Result<String> {
        Ok("# TYPE caddy_http_requests_total counter\ncaddy_http_requests_total{host=\"sona.it\"} 7\n".into())
    }
    fn cloudflare_accounts(&self) -> Vec<String> {
        self.cf_accounts.clone()
    }
    async fn tunnel_add(&self, account: &str, host: &alveare_core::site::Domain) -> anyhow::Result<()> {
        self.record("tunnel_add", format!("{account} {}", host.as_str()))
    }
    async fn tunnel_remove(&self, account: &str, host: &alveare_core::site::Domain) -> anyhow::Result<()> {
        self.record("tunnel_remove", format!("{account} {}", host.as_str()))
    }
    async fn cloudflare_rules(&self, account: &str, host: &alveare_core::site::Domain, _template: &serde_json::Value) -> anyhow::Result<usize> {
        self.record("cloudflare_rules", format!("{account} {}", host.as_str()))?;
        Ok(3)
    }
    async fn notify(&self, text: &str) -> anyhow::Result<bool> {
        self.record("notify", text.into())?;
        Ok(true)
    }
    async fn site_disk_usage(&self, slug: &Slug) -> anyhow::Result<u64> {
        self.record("site_disk_usage", slug.as_str().into())?;
        Ok(5 * 1024 * 1024)
    }
    async fn http_get(&self, host: &alveare_core::site::Domain, path: &str, auth: Option<(&str, &str)>, _timeout_s: u32) -> anyhow::Result<(u16, String)> {
        self.record("http_get", format!("{} {path} {}", host.as_str(), auth.map_or("-".to_string(), |(u, _)| format!("{u}:"))))?;
        Ok((self.probe_status.unwrap_or(200), "ok".into()))
    }
    async fn php(&self, slug: &Slug, args: &[String], timeout_s: u32) -> anyhow::Result<(bool, String)> {
        self.record("php", format!("{} {} ({timeout_s} s)", slug.as_str(), args.join(" ")))?;
        Ok((true, String::new()))
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

