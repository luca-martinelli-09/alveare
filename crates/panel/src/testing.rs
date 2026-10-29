//! Doppioni per i test: un host che registra le chiamate invece di toccare il sistema.
use std::sync::Mutex;

use alveare_core::caddy::CaddySite;
use alveare_core::php::SitePhp;
use alveare_core::site::Slug;

use crate::flows::{DbCreds, Host};
use crate::template::Vars;

#[derive(Default)]
pub struct FakeHost {
    pub calls: Mutex<Vec<String>>,
    pub fail_on: Option<&'static str>,
    pub reject_pool: bool,
    pub env: Mutex<Vars>,
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
    async fn wp_output(&self, slug: &Slug, args: &[String]) -> anyhow::Result<(bool, String)> {
        self.record("wp", format!("{} {}", slug.as_str(), args.join(" "))).map(|_| (true, String::new()))
    }
    async fn reopen_fpm_logs(&self) -> anyhow::Result<()> {
        self.record("reopen_fpm_logs", String::new())
    }
}

