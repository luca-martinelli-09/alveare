//! Effetti reali del pannello: cartelle dei siti, MariaDB, agente PHP, Admin API di Caddy.
use std::fs;
use std::os::unix::fs::{lchown, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alveare_core::agent::{Command, Response};
use alveare_core::caddy::{render_caddyfile, CaddyConfig, CaddySite};
use alveare_core::php::{render_group, render_passwd, render_pool, SitePhp};
use alveare_core::site::{Domain, Slug};
use anyhow::{bail, Context, Result};
use sqlx::MySqlPool;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::flows::{DbCreds, Host};
use crate::template::{env_file, Template, Vars};

/// Il timeout dell'agente per WP-CLI è 600 s: il pannello aspetta un po' di più.
const AGENT_TIMEOUT: Duration = Duration::from_secs(660);

#[derive(Debug, Clone)]
pub struct Paths {
    pub sites: PathBuf,
    pub logs: PathBuf,
    pub tmp: PathBuf,
    pub trash: PathBuf,
    pub php_etc: PathBuf,
    pub pools: PathBuf,
    pub staging: PathBuf,
    pub templates: PathBuf,
    pub caddy: PathBuf,
    pub caddy_admin: PathBuf,
    pub agent: PathBuf,
}

impl Paths {
    pub fn under(root: &Path) -> Self {
        Self {
            sites: root.join("sites"),
            logs: root.join("logs"),
            tmp: root.join("tmp"),
            trash: root.join("trash"),
            php_etc: root.join("php/etc"),
            pools: root.join("php/pools"),
            staging: root.join("php/staging"),
            templates: root.join("templates"),
            caddy: root.join("caddy"),
            caddy_admin: root.join("run/caddy/admin.sock"),
            agent: root.join("run/alveare/agent.sock"),
        }
    }

    /// Percorsi nel container del pannello: identici a quelli di Caddy e PHP.
    pub fn production() -> Self {
        Self {
            caddy_admin: alveare_core::caddy::ADMIN_SOCKET.into(),
            agent: alveare_core::agent::AGENT_SOCKET.into(),
            ..Self::under(Path::new("/srv"))
        }
    }
}

fn set_mode(p: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).with_context(|| format!("chmod {}", p.display()))
}

fn chown(p: &Path, owner: Option<u32>) -> Result<()> {
    match owner {
        Some(uid) => lchown(p, Some(uid), Some(uid)).with_context(|| format!("chown {}", p.display())),
        None => Ok(()),
    }
}

/// chown ricorsivo che non segue i link simbolici.
fn chown_tree(p: &Path, owner: Option<u32>) -> Result<()> {
    chown(p, owner)?;
    if fs::symlink_metadata(p)?.is_dir() {
        for entry in fs::read_dir(p)? {
            chown_tree(&entry?.path(), owner)?;
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content).with_context(|| format!("scrittura di {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename su {}", path.display()))
}

/// `owner = None` lascia il proprietario attuale (test senza root).
pub fn create_dirs(p: &Paths, slug: &Slug, owner: Option<u32>) -> Result<()> {
    let s = slug.as_str();
    let site = p.sites.join(s);
    let public = site.join("public");
    let tmp = p.tmp.join(s);
    let logs = p.logs.join(s);
    for (dir, mode, own) in [(&site, 0o750, owner), (&public, 0o755, owner), (&tmp, 0o700, owner), (&logs, 0o755, None)] {
        fs::create_dir_all(dir).with_context(|| format!("mkdir {}", dir.display()))?;
        set_mode(dir, mode)?;
        chown(dir, own)?;
    }
    // L'error_log lo scrive il worker FPM, cioè l'utente del sito.
    let error_log = logs.join("php-error.log");
    fs::write(&error_log, "")?;
    set_mode(&error_log, 0o640)?;
    chown(&error_log, owner)
}

pub fn write_env(p: &Paths, slug: &Slug, env: &Vars, owner: Option<u32>) -> Result<()> {
    let path = p.sites.join(slug.as_str()).join(".env");
    let _ = fs::remove_file(&path);
    fs::write(&path, env_file(env)?)?;
    set_mode(&path, 0o400)?;
    chown(&path, owner)
}

pub fn trash_dirs(p: &Paths, slug: &Slug, now: u64) -> Result<()> {
    let s = slug.as_str();
    let dest = p.trash.join(format!("{s}-{now}"));
    fs::create_dir_all(&dest)?;
    for (from, name) in [(p.sites.join(s), "site"), (p.logs.join(s), "logs")] {
        if from.exists() {
            fs::rename(&from, dest.join(name)).with_context(|| format!("spostamento di {}", from.display()))?;
        }
    }
    remove_if_exists(&p.tmp.join(s))
}

fn remove_if_exists(dir: &Path) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("rm {}", dir.display())),
        _ => Ok(()),
    }
}

pub fn remove_dirs(p: &Paths, slug: &Slug) -> Result<()> {
    let s = slug.as_str();
    for dir in [p.sites.join(s), p.logs.join(s), p.tmp.join(s)] {
        remove_if_exists(&dir)?;
    }
    Ok(())
}

/// passwd/group sempre; in staging solo i pool cambiati; via i pool di siti che non ne hanno più.
pub fn write_php_files(p: &Paths, users: &[SitePhp], pools: &[SitePhp]) -> Result<()> {
    for dir in [&p.php_etc, &p.pools, &p.staging] {
        fs::create_dir_all(dir)?;
    }
    let users: Vec<&SitePhp> = users.iter().collect();
    write_atomic(&p.php_etc.join("passwd"), &render_passwd(&users))?;
    write_atomic(&p.php_etc.join("group"), &render_group(&users))?;
    let wanted: Vec<String> = pools.iter().map(|s| format!("{}.conf", s.slug.as_str())).collect();
    for (site, name) in pools.iter().zip(&wanted) {
        let conf = render_pool(site);
        if fs::read_to_string(p.pools.join(name)).ok().as_deref() != Some(conf.as_str()) {
            fs::write(p.staging.join(name), conf)?;
        }
    }
    for entry in fs::read_dir(&p.pools)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".conf") && !wanted.contains(&name) {
            fs::remove_file(p.pools.join(&name))?;
        }
    }
    Ok(())
}

pub async fn agent_call(socket: &Path, cmd: &Command, timeout: Duration) -> Result<Response> {
    let call = async {
        let stream = UnixStream::connect(socket).await.with_context(|| format!("connessione all'agente ({})", socket.display()))?;
        let (r, mut w) = stream.into_split();
        w.write_all(cmd.to_line().as_bytes()).await?;
        let mut line = String::new();
        BufReader::new(r).read_line(&mut line).await?;
        serde_json::from_str::<Response>(&line).context("risposta dell'agente non valida")
    };
    tokio::time::timeout(timeout, call).await.context("l'agente non ha risposto in tempo")?
}

/// POST /load all'Admin API di Caddy sul socket Unix: valida e applica, oppure rifiuta tutto.
pub async fn caddy_load(socket: &Path, caddyfile: &str) -> Result<()> {
    let mut stream = UnixStream::connect(socket).await.with_context(|| format!("connessione a Caddy ({})", socket.display()))?;
    let head = format!(
        "POST /load HTTP/1.1\r\nHost: caddy\r\nContent-Type: text/caddyfile\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        caddyfile.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(caddyfile.as_bytes()).await?;
    let mut resp = String::new();
    tokio::time::timeout(Duration::from_secs(30), stream.read_to_string(&mut resp)).await.context("Caddy non ha risposto")??;
    let status = resp.split(' ').nth(1).unwrap_or_default();
    if status != "200" {
        let body = resp.split_once("\r\n\r\n").map_or("", |(_, b)| b);
        bail!("Caddy ha rifiutato la configurazione ({status}): {body}");
    }
    Ok(())
}

fn sql_safe(s: &str) -> Result<&str> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        bail!("valore non ammesso in SQL: {s:?}");
    }
    Ok(s)
}

pub struct SystemHost {
    pub paths: Paths,
    pub mysql: MySqlPool,
    pub panel_host: Option<Domain>,
}

impl SystemHost {
    async fn agent(&self, cmd: Command) -> Result<Response> {
        agent_call(&self.paths.agent, &cmd, AGENT_TIMEOUT).await
    }
}

impl Host for SystemHost {
    async fn create_database(&self, db: &DbCreds) -> Result<()> {
        let (name, user, pw) = (sql_safe(&db.name)?, sql_safe(&db.user)?, sql_safe(&db.password)?);
        // Nomi e password solo [A-Za-z0-9_]: niente da escapare. CREATE USER non è preparabile.
        sqlx::raw_sql(&format!(
            "CREATE DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
             CREATE USER '{user}'@'%' IDENTIFIED BY '{pw}';
             GRANT ALL PRIVILEGES ON `{name}`.* TO '{user}'@'%';"
        ))
        .execute(&self.mysql)
        .await
        .context("creazione del database")?;
        Ok(())
    }

    async fn drop_database(&self, name: &str, user: &str) -> Result<()> {
        let (name, user) = (sql_safe(name)?, sql_safe(user)?);
        sqlx::raw_sql(&format!("DROP DATABASE IF EXISTS `{name}`; DROP USER IF EXISTS '{user}'@'%';"))
            .execute(&self.mysql)
            .await
            .context("eliminazione del database")?;
        Ok(())
    }

    async fn create_site_dirs(&self, slug: &Slug, uid: u32) -> Result<()> {
        create_dirs(&self.paths, slug, Some(uid))
    }

    async fn remove_site_dirs(&self, slug: &Slug) -> Result<()> {
        remove_dirs(&self.paths, slug)
    }

    async fn trash_site_dirs(&self, slug: &Slug) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        trash_dirs(&self.paths, slug, now)
    }

    async fn install_template(&self, slug: &Slug, uid: u32, template: &str, vars: &Vars, env: &Vars) -> Result<Vec<Vec<String>>> {
        let name = Slug::parse(template).map_err(|_| anyhow::anyhow!("nome di template non valido: {template}"))?;
        let tpl = Template::load(&self.paths.templates.join(name.as_str()))?;
        let dest = self.paths.sites.join(slug.as_str());
        tpl.install(&dest, vars)?;
        write_env(&self.paths, slug, env, None)?;
        chown_tree(&dest, Some(uid))?;
        tpl.post_create(vars)
    }

    async fn sync_php(&self, users: &[SitePhp], pools: &[SitePhp]) -> Result<Vec<String>> {
        write_php_files(&self.paths, users, pools)?;
        let resp = self.agent(Command::PoolApply).await?;
        #[derive(serde::Deserialize)]
        struct Report {
            rejected: Vec<(String, String)>,
        }
        let report: Report = serde_json::from_str(&resp.output).with_context(|| format!("pool.apply: {}", resp.output))?;
        if !resp.ok && report.rejected.is_empty() {
            bail!("reload di PHP-FPM fallito");
        }
        Ok(report.rejected.into_iter().map(|(name, _)| name).collect())
    }

    async fn sync_caddy(&self, sites: &[CaddySite]) -> Result<()> {
        let text = render_caddyfile(&CaddyConfig { panel_host: self.panel_host.clone(), sites: sites.to_vec() });
        fs::create_dir_all(&self.paths.caddy)?;
        caddy_load(&self.paths.caddy_admin, &text).await?;
        // Su disco solo una config che Caddy ha accettato: è quella usata al riavvio.
        write_atomic(&self.paths.caddy.join("Caddyfile"), &text)
    }

    async fn wp(&self, slug: &Slug, args: &[String]) -> Result<String> {
        let resp = self.agent(Command::Wp { slug: slug.clone(), args: args.to_vec() }).await?;
        if !resp.ok {
            bail!("wp {}: {}", args.join(" "), resp.output.trim());
        }
        Ok(resp.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alveare_core::php::PhpSettings;
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    fn slug(s: &str) -> Slug {
        Slug::parse(s).unwrap()
    }

    fn paths(t: &tempfile::TempDir) -> Paths {
        Paths::under(t.path())
    }

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    fn php(s: &str, uid: u32) -> SitePhp {
        SitePhp { slug: slug(s), uid, settings: PhpSettings::default() }
    }

    #[test]
    fn site_dirs_are_private_to_the_site() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(&t);
        create_dirs(&p, &slug("sona_it"), None).unwrap();
        assert_eq!(mode(&p.sites.join("sona_it")), 0o750);
        assert!(p.sites.join("sona_it/public").is_dir());
        assert_eq!(mode(&p.tmp.join("sona_it")), 0o700);
        assert_eq!(mode(&p.logs.join("sona_it")), 0o755);
        assert!(p.logs.join("sona_it/php-error.log").is_file());
    }

    #[test]
    fn trash_moves_site_folder_and_removes_tmp() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(&t);
        create_dirs(&p, &slug("sona_it"), None).unwrap();
        fs::write(p.sites.join("sona_it/public/index.php"), "x").unwrap();
        trash_dirs(&p, &slug("sona_it"), 1_700_000_000).unwrap();
        assert!(!p.sites.join("sona_it").exists());
        assert!(!p.tmp.join("sona_it").exists());
        assert!(p.trash.join("sona_it-1700000000/site/public/index.php").exists());
        assert!(p.trash.join("sona_it-1700000000/logs/php-error.log").exists());
    }

    #[test]
    fn remove_dirs_deletes_everything_and_tolerates_missing() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(&t);
        create_dirs(&p, &slug("sona_it"), None).unwrap();
        remove_dirs(&p, &slug("sona_it")).unwrap();
        remove_dirs(&p, &slug("sona_it")).unwrap();
        assert!(!p.sites.join("sona_it").exists() && !p.logs.join("sona_it").exists());
    }

    #[test]
    fn env_file_is_readable_only_by_owner() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(&t);
        create_dirs(&p, &slug("sona_it"), None).unwrap();
        write_env(&p, &slug("sona_it"), &Vars::from([("A".into(), "b".into())]), None).unwrap();
        let env = p.sites.join("sona_it/.env");
        assert_eq!(fs::read_to_string(&env).unwrap(), "A='b'\n");
        assert_eq!(mode(&env), 0o400);
    }

    #[test]
    fn php_files_stage_only_changed_pools_and_drop_stale_ones() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(&t);
        let (a, b) = (php("a_it", 20001), php("b_it", 20002));
        fs::create_dir_all(&p.pools).unwrap();
        fs::write(p.pools.join("a_it.conf"), render_pool(&a)).unwrap();
        fs::write(p.pools.join("old_it.conf"), "vecchio").unwrap();
        write_php_files(&p, &[a.clone(), b.clone()], &[a, b]).unwrap();
        assert!(!p.staging.join("a_it.conf").exists(), "pool invariato non va rivalidato");
        assert!(p.staging.join("b_it.conf").exists());
        assert!(!p.pools.join("old_it.conf").exists());
        let passwd = fs::read_to_string(p.php_etc.join("passwd")).unwrap();
        assert!(passwd.contains("\nb_it:x:20002:20002:"));
        assert!(p.php_etc.join("group").exists());
    }

    #[tokio::test]
    async fn agent_client_sends_one_line_and_reads_response() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("agent.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            let (r, mut w) = s.into_split();
            let mut line = String::new();
            BufReader::new(r).read_line(&mut line).await.unwrap();
            w.write_all(b"{\"ok\":true,\"exit_code\":0,\"output\":\"fatto\"}\n").await.unwrap();
            line
        });
        let resp = agent_call(&sock, &Command::CronRun { slug: slug("sona_it") }, Duration::from_secs(5)).await.unwrap();
        assert_eq!(resp.output, "fatto");
        assert_eq!(server.await.unwrap(), "{\"cmd\":\"cron_run\",\"slug\":\"sona_it\"}\n");
    }

    async fn fake_caddy(sock: &Path, status: &'static str, body: &'static str) -> tokio::task::JoinHandle<String> {
        let listener = tokio::net::UnixListener::bind(sock).unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap();
            let resp = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            s.write_all(resp.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        })
    }

    #[tokio::test]
    async fn caddy_load_posts_caddyfile() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("admin.sock");
        let server = fake_caddy(&sock, "200 OK", "").await;
        caddy_load(&sock, "{\n}\n").await.unwrap();
        let req = server.await.unwrap();
        assert!(req.starts_with("POST /load HTTP/1.1\r\n"));
        assert!(req.contains("Content-Type: text/caddyfile\r\n"));
        assert!(req.ends_with("\r\n\r\n{\n}\n"));
    }

    #[tokio::test]
    async fn caddy_load_error_carries_caddy_message() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("admin.sock");
        let _server = fake_caddy(&sock, "400 Bad Request", "{\"error\":\"unrecognized directive\"}").await;
        let err = caddy_load(&sock, "x").await.unwrap_err();
        assert!(format!("{err:#}").contains("unrecognized directive"), "{err:#}");
    }
}
