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

use sqlx::Row;

use crate::backup::{backup_args, forget_args, parse_summary, valid_snapshot, Retention, Snapshot};
use crate::database::TableInfo;
use crate::flows::{DbCreds, Dump, Host, Installed};
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
    pub adminer: PathBuf,
    pub backups: PathBuf,
    pub redis: PathBuf,
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
            adminer: root.join("adminer"),
            backups: root.join("backups"),
            redis: root.join("redis"),
        }
    }

    /// Percorsi nel container del pannello: identici a quelli di Caddy e PHP.
    /// `data/` è un unico mount su /srv, così cestino e upload si spostano con un rename.
    pub fn production() -> Self {
        Self {
            caddy: "/srv/caddy/config".into(),
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

/// Cartelle di Adminer: token, sessioni e tmp, solo per l'utente adminer.
pub fn create_adminer_dirs(p: &Paths, owner: Option<u32>) -> Result<()> {
    for sub in ["tokens", "sessions", "tmp"] {
        let dir = p.adminer.join(sub);
        fs::create_dir_all(&dir)?;
        set_mode(&dir, 0o700)?;
        chown(&dir, owner)?;
    }
    Ok(())
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

/// Copia ricorsiva per la clonazione: niente `.env` (credenziali della sorgente) né link simbolici.
pub fn copy_site_tree(src: &Path, dest: &Path, rel: &Path) -> Result<()> {
    fs::create_dir_all(dest.join(rel))?;
    for entry in fs::read_dir(src.join(rel))? {
        let entry = entry?;
        let child = rel.join(entry.file_name());
        if child == Path::new(".env") {
            continue;
        }
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_site_tree(src, dest, &child)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), dest.join(&child)).with_context(|| format!("copia di {}", child.display()))?;
        }
    }
    Ok(())
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

/// Gruppo dell'utente redis nell'immagine ufficiale: deve leggere il file ACL.
const REDIS_GID: u32 = 999;

/// users.acl nella cartella dati di Redis: contiene solo hash SHA-256 delle password.
pub fn write_redis_acl(p: &Paths, admin_sha256: &str, users: &[alveare_core::redis::RedisUser]) -> Result<()> {
    fs::create_dir_all(&p.redis)?;
    let path = p.redis.join("users.acl");
    write_atomic(&path, &alveare_core::redis::render_acl(admin_sha256, users))?;
    set_mode(&path, 0o640)?;
    if unsafe { libc::geteuid() } == 0 {
        lchown(&path, Some(0), Some(REDIS_GID))?;
    }
    Ok(())
}

/// AUTH come admin e ACL LOAD, in RESP: due comandi non valgono un client Redis.
pub async fn redis_acl_load(addr: &str, admin_password: &str) -> Result<()> {
    fn resp(args: &[&str]) -> String {
        args.iter().fold(format!("*{}\r\n", args.len()), |acc, a| acc + &format!("${}\r\n{a}\r\n", a.len()))
    }
    let stream = tokio::net::TcpStream::connect(addr).await.with_context(|| format!("connessione a Redis ({addr})"))?;
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    for cmd in [resp(&["AUTH", alveare_core::redis::ADMIN_USER, admin_password]), resp(&["ACL", "LOAD"])] {
        w.write_all(cmd.as_bytes()).await?;
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), r.read_line(&mut line)).await.context("Redis non risponde")??;
        if !line.starts_with('+') {
            bail!("Redis: {}", line.trim());
        }
    }
    Ok(())
}

pub struct SystemHost {
    pub paths: Paths,
    pub mysql: MySqlPool,
    pub panel_host: Option<Domain>,
    pub adminer_host: Option<Domain>,
    pub db_host: String,
    /// Passata a mariadb-dump e mariadb via MYSQL_PWD, mai sulla riga di comando.
    pub root_password: String,
    pub redis_addr: String,
    pub redis_admin_password: String,
}

impl SystemHost {
    async fn agent(&self, cmd: Command) -> Result<Response> {
        agent_call(&self.paths.agent, &cmd, AGENT_TIMEOUT).await
    }

    async fn restic(&self, args: &[String]) -> Result<String> {
        let out = tokio::process::Command::new("restic")
            .args(args)
            .env("RESTIC_CACHE_DIR", self.paths.backups.join("cache"))
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .context("avvio di restic")?;
        if !out.status.success() {
            bail!("restic {}: {}", args.first().map_or("", String::as_str), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Il primo backup inizializza il repository.
    async fn ensure_repo(&self) -> Result<()> {
        if self.restic(&["cat".into(), "config".into()]).await.is_ok() {
            return Ok(());
        }
        self.restic(&["init".into()]).await.map(drop)
    }

    async fn dump_to_file(&self, db: &str, dest: &Path) -> Result<()> {
        let dump = self.db_dump(db, &[]).await?;
        let mut file = tokio::fs::File::create(dest).await?;
        let mut reader = dump.reader;
        tokio::io::copy(&mut reader, &mut file).await?;
        file.sync_all().await?;
        dump.finished.await
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

    async fn install_template(&self, slug: &Slug, uid: u32, template: &str, vars: &Vars, env: &Vars) -> Result<Installed> {
        let name = Slug::parse(template).map_err(|_| anyhow::anyhow!("nome di template non valido: {template}"))?;
        let tpl = Template::load(&self.paths.templates.join(name.as_str()))?;
        let dest = self.paths.sites.join(slug.as_str());
        tpl.install(&dest, vars)?;
        write_env(&self.paths, slug, env, None)?;
        chown_tree(&dest, Some(uid))?;
        Ok(Installed { commands: tpl.post_create(vars)?, sql: tpl.sql_path().map(|p| (p, tpl.manifest.sql_domain.clone())) })
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
        let text = render_caddyfile(&CaddyConfig { panel_host: self.panel_host.clone(), adminer_host: self.adminer_host.clone(), sites: sites.to_vec() });
        fs::create_dir_all(&self.paths.caddy)?;
        caddy_load(&self.paths.caddy_admin, &text).await?;
        // Su disco solo una config che Caddy ha accettato: è quella usata al riavvio.
        write_atomic(&self.paths.caddy.join("Caddyfile"), &text)
    }

    async fn wp(&self, slug: &Slug, args: &[String]) -> Result<String> {
        let (ok, output) = self.wp_output(slug, args).await?;
        if !ok {
            bail!("wp {}: {}", args.join(" "), output.trim());
        }
        Ok(output)
    }

    async fn wp_output(&self, slug: &Slug, args: &[String]) -> Result<(bool, String)> {
        let resp = self.agent(Command::Wp { slug: slug.clone(), args: args.to_vec() }).await?;
        Ok((resp.ok, resp.output))
    }

    async fn db_tables(&self, name: &str) -> Result<Vec<TableInfo>> {
        let rows = sqlx::query(
            "SELECT CAST(table_name AS CHAR) AS name, CAST(COALESCE(table_rows, 0) AS UNSIGNED) AS r,
                    CAST(COALESCE(data_length, 0) + COALESCE(index_length, 0) AS UNSIGNED) AS b
             FROM information_schema.tables WHERE table_schema = ? ORDER BY b DESC",
        )
        .bind(sql_safe(name)?)
        .fetch_all(&self.mysql)
        .await?;
        rows.iter()
            .map(|r| Ok(TableInfo { name: r.try_get("name")?, rows: r.try_get("r")?, bytes: r.try_get("b")? }))
            .collect()
    }

    async fn db_dump(&self, name: &str, tables: &[String]) -> Result<Dump> {
        let mut cmd = tokio::process::Command::new("mariadb-dump");
        cmd.args(["--single-transaction", "--quick", "--default-character-set=utf8mb4", "-h", &self.db_host, "-u", "root", "--"])
            .arg(sql_safe(name)?)
            .args(tables)
            .env("MYSQL_PWD", &self.root_password)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("avvio di mariadb-dump")?;
        let stdout = child.stdout.take().context("stdout di mariadb-dump")?;
        let finished = async move {
            let out = child.wait_with_output().await?;
            if !out.status.success() {
                bail!("mariadb-dump: {}", String::from_utf8_lossy(&out.stderr).trim());
            }
            Ok(())
        };
        Ok(Dump { reader: Box::pin(stdout), finished: Box::pin(finished) })
    }

    async fn db_import(&self, name: &str, mut sql: std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>) -> Result<()> {
        let mut child = tokio::process::Command::new("mariadb")
            .args(["--default-character-set=utf8mb4", "-h", &self.db_host, "-u", "root", "--"])
            .arg(sql_safe(name)?)
            .env("MYSQL_PWD", &self.root_password)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("avvio di mariadb")?;
        let mut stdin = child.stdin.take().context("stdin di mariadb")?;
        let copied = tokio::io::copy(&mut sql, &mut stdin).await;
        drop(stdin);
        let out = child.wait_with_output().await?;
        if !out.status.success() {
            bail!("import: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        copied.context("lettura del dump")?;
        Ok(())
    }

    async fn backup(&self, slug: &Slug, db_name: &str) -> Result<Snapshot> {
        let dir = self.paths.backups.join(slug.as_str());
        fs::create_dir_all(&dir)?;
        set_mode(&dir, 0o700)?;
        let dump = dir.join("db.sql");
        self.dump_to_file(db_name, &dump).await.context("dump del database")?;
        self.ensure_repo().await?;
        let site_dir = self.paths.sites.join(slug.as_str());
        let out = self.restic(&backup_args(slug, &site_dir.to_string_lossy(), &dump.to_string_lossy())).await;
        let _ = fs::remove_file(&dump);
        parse_summary(&out?).context("riepilogo di restic mancante")
    }

    async fn prune(&self, slug: &Slug, keep: Retention) -> Result<()> {
        self.restic(&forget_args(slug, keep)).await.map(drop)
    }

    async fn expire_deleted(&self, slug: &str) -> Result<()> {
        let slug = Slug::parse(slug).map_err(|_| anyhow::anyhow!("slug non valido: {slug}"))?;
        let args = ["forget", "--json", "--tag", &format!("site={}", slug.as_str()), "--keep-within", "30d", "--prune"].map(String::from);
        self.restic(&args).await.map(drop)
    }

    async fn restore(&self, slug: &Slug, snapshot: &str, files: bool, db: Option<&DbCreds>) -> Result<()> {
        if !valid_snapshot(snapshot) {
            bail!("snapshot non valido: {snapshot}");
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let staging = self.paths.backups.join(format!("restore-{}-{now}", slug.as_str()));
        let site_dir = self.paths.sites.join(slug.as_str());
        let dump = self.paths.backups.join(slug.as_str()).join("db.sql");
        let mut args = vec!["restore".to_string(), snapshot.to_string(), "--target".into(), staging.to_string_lossy().into_owned()];
        if files {
            args.extend(["--include".into(), site_dir.to_string_lossy().into_owned()]);
        }
        if db.is_some() {
            args.extend(["--include".into(), dump.to_string_lossy().into_owned()]);
        }
        let result = async {
            self.restic(&args).await?;
            // restic ricrea i percorsi assoluti sotto la cartella di destinazione.
            let inside = |p: &Path| staging.join(p.strip_prefix("/").unwrap_or(p));
            if files {
                let restored = inside(&site_dir);
                if !restored.is_dir() {
                    bail!("lo snapshot non contiene i file del sito");
                }
                let old = self.paths.trash.join(format!("{}-{now}", slug.as_str()));
                fs::create_dir_all(&old)?;
                if site_dir.exists() {
                    fs::rename(&site_dir, old.join("site"))?;
                }
                fs::rename(&restored, &site_dir)?;
            }
            if let Some(db) = db {
                let restored = inside(&dump);
                let file = tokio::fs::File::open(&restored).await.context("lo snapshot non contiene il dump del database")?;
                self.drop_database(&db.name, &db.user).await?;
                self.create_database(db).await?;
                self.db_import(&db.name, Box::pin(tokio::io::BufReader::new(file))).await?;
            }
            Ok(())
        }
        .await;
        let _ = fs::remove_dir_all(&staging);
        result
    }

    async fn backup_panel(&self, store: &crate::store::Store, env_enc: &[u8]) -> Result<Snapshot> {
        let dir = self.paths.backups.join("panel");
        fs::create_dir_all(&dir)?;
        set_mode(&dir, 0o700)?;
        store.snapshot_to(&dir.join("panel.db")).await?;
        fs::write(dir.join("env.enc"), env_enc)?;
        self.ensure_repo().await?;
        let args = ["backup", "--json", "--host", "alveare", "--tag", "panel", &dir.to_string_lossy(), &self.paths.templates.to_string_lossy()].map(String::from);
        let snap = parse_summary(&self.restic(&args).await?).context("riepilogo di restic mancante")?;
        let keep = ["forget", "--json", "--host", "alveare", "--tag", "panel", "--keep-daily", "7", "--keep-weekly", "4", "--keep-monthly", "6", "--prune"].map(String::from);
        self.restic(&keep).await?;
        Ok(snap)
    }

    async fn sync_redis(&self, users: &[alveare_core::redis::RedisUser]) -> Result<()> {
        write_redis_acl(&self.paths, &alveare_core::redis::sha256_hex(&self.redis_admin_password), users)?;
        redis_acl_load(&self.redis_addr, &self.redis_admin_password).await
    }

    async fn adminer_grant(&self, db: &DbCreds) -> Result<String> {
        let token = rand::distr::SampleString::sample_string(&rand::distr::Alphanumeric, &mut rand::rng(), 40);
        let exp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) + 300;
        let grant = serde_json::json!({ "host": self.db_host, "user": db.user, "password": db.password, "db": db.name, "exp": exp });
        let path = self.paths.adminer.join("tokens").join(format!("{token}.json"));
        fs::write(&path, grant.to_string())?;
        set_mode(&path, 0o400)?;
        chown(&path, Some(alveare_core::php::ADMINER_UID))?;
        Ok(token)
    }

    async fn set_db_password(&self, user: &str, password: &str) -> Result<()> {
        let (user, pw) = (sql_safe(user)?, sql_safe(password)?);
        sqlx::raw_sql(&format!("ALTER USER '{user}'@'%' IDENTIFIED BY '{pw}';")).execute(&self.mysql).await.context("cambio password")?;
        Ok(())
    }

    async fn copy_site_files(&self, from: &Slug, to: &Slug, uid: u32) -> Result<()> {
        let (src, dest) = (self.paths.sites.join(from.as_str()), self.paths.sites.join(to.as_str()));
        tokio::task::spawn_blocking(move || {
            copy_site_tree(&src, &dest, Path::new(""))?;
            chown_tree(&dest, Some(uid))
        })
        .await?
    }

    async fn read_site_env(&self, slug: &Slug) -> Result<Vars> {
        let path = self.paths.sites.join(slug.as_str()).join(".env");
        Ok(crate::template::parse_env(&fs::read_to_string(&path).context("lettura del .env")?))
    }

    async fn write_site_env(&self, slug: &Slug, uid: u32, env: &Vars) -> Result<()> {
        write_env(&self.paths, slug, env, Some(uid))
    }

    async fn set_site_env(&self, slug: &Slug, uid: u32, key: &str, value: &str) -> Result<()> {
        let path = self.paths.sites.join(slug.as_str()).join(".env");
        let current = fs::read_to_string(&path).context("lettura del .env")?;
        let updated = crate::database::replace_env_value(&current, key, value)?;
        let tmp = path.with_extension("new");
        fs::write(&tmp, updated)?;
        set_mode(&tmp, 0o400)?;
        chown(&tmp, Some(uid))?;
        fs::rename(&tmp, &path).context("sostituzione del .env")
    }

    async fn reopen_fpm_logs(&self) -> Result<()> {
        let resp = self.agent(Command::FpmReopenLogs).await?;
        if !resp.ok {
            bail!("riapertura dei log di FPM: {}", resp.output);
        }
        Ok(())
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
    fn clone_copy_skips_env_and_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let (src, dest) = (t.path().join("a"), t.path().join("b"));
        fs::create_dir_all(src.join("public/wp-content")).unwrap();
        fs::write(src.join(".env"), "DB_PASSWORD='x'").unwrap();
        fs::write(src.join("public/wp-content/a.txt"), "ciao").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.join("public/link")).unwrap();
        copy_site_tree(&src, &dest, Path::new("")).unwrap();
        assert_eq!(fs::read_to_string(dest.join("public/wp-content/a.txt")).unwrap(), "ciao");
        assert!(!dest.join(".env").exists());
        assert!(fs::symlink_metadata(dest.join("public/link")).is_err());
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
    fn adminer_dirs_are_private() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(&t);
        create_adminer_dirs(&p, None).unwrap();
        for sub in ["tokens", "sessions", "tmp"] {
            assert_eq!(mode(&p.adminer.join(sub)), 0o700);
        }
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
    async fn redis_acl_load_authenticates_then_reloads() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            let (r, mut w) = s.into_split();
            let mut r = BufReader::new(r);
            let mut seen = String::new();
            for _ in 0..2 {
                let mut header = String::new();
                r.read_line(&mut header).await.unwrap();
                let n: usize = header.trim()[1..].parse().unwrap();
                seen.push_str(&header);
                for _ in 0..n * 2 {
                    let mut l = String::new();
                    r.read_line(&mut l).await.unwrap();
                    seen.push_str(&l);
                }
                w.write_all(b"+OK\r\n").await.unwrap();
            }
            seen
        });
        redis_acl_load(&addr, "pw").await.unwrap();
        assert_eq!(server.await.unwrap(), "*3\r\n$4\r\nAUTH\r\n$7\r\nalveare\r\n$2\r\npw\r\n*2\r\n$3\r\nACL\r\n$4\r\nLOAD\r\n");
    }

    #[tokio::test]
    async fn redis_error_is_reported() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            s.write_all(b"-WRONGPASS invalid username-password pair\r\n").await.unwrap();
        });
        let err = redis_acl_load(&addr, "x").await.unwrap_err();
        assert!(err.to_string().contains("WRONGPASS"));
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
