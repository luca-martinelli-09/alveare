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
use crate::flows::{DbCreds, Dump, FilesScope, Host, Installed};
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
    /// Override globale di PHP, letto da PHP_INI_SCAN_DIR.
    pub php_conf: PathBuf,
    pub staging: PathBuf,
    pub templates: PathBuf,
    pub caddy: PathBuf,
    pub caddy_admin: PathBuf,
    pub agent: PathBuf,
    pub adminer: PathBuf,
    pub backups: PathBuf,
    pub redis: PathBuf,
    /// Area import: archivi e dump caricati dall'admin (`files/`), blocchi in arrivo (`tmp/`).
    pub imports: PathBuf,
    /// Esportazioni dei siti, scaricabili per 7 giorni.
    pub exports: PathBuf,
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
            php_conf: root.join("php/conf.d"),
            staging: root.join("php/staging"),
            templates: root.join("templates"),
            caddy: root.join("caddy"),
            caddy_admin: root.join("run/caddy/admin.sock"),
            agent: root.join("run/alveare/agent.sock"),
            adminer: root.join("adminer"),
            backups: root.join("backups"),
            redis: root.join("redis"),
            imports: root.join("imports"),
            exports: root.join("exports"),
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

/// Legge un file direttamente nella cartella di un sito (che il sito possiede): mai attraverso un link.
fn read_nofollow(path: &Path) -> Result<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path).with_context(|| format!("apertura di {}", path.display()))?;
    if !f.metadata()?.is_file() {
        bail!("{} non è un file", path.display());
    }
    let mut out = String::new();
    f.read_to_string(&mut out)?;
    Ok(out)
}

/// Scrive `content` in `path` (nella cartella di un sito) passando da `<path>.new` creato da zero:
/// un link o un file messo lì dal sito non viene mai seguito né riusato.
fn replace_site_file(path: &Path, content: &str, mode: u32, owner: Option<u32>) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("new");
    match fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("pulizia di {}", tmp.display())),
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .with_context(|| format!("creazione di {}", tmp.display()))?;
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    if let Some(uid) = owner {
        std::os::unix::fs::fchown(&f, Some(uid), Some(uid))?;
    }
    drop(f);
    fs::rename(&tmp, path).with_context(|| format!("sostituzione di {}", path.display()))
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
    replace_site_file(&p.sites.join(slug.as_str()).join(".env"), &env_file(env)?, 0o400, owner)
}

/// Copia ricorsiva per la clonazione: niente `.env` (credenziali della sorgente) né link simbolici.
/// La sorgente si legge attraverso una Dir (cap-std): il sito non può far uscire la copia dalla sua cartella.
pub fn copy_site_tree(src: &Path, dest: &Path) -> Result<()> {
    let root = cap_std::fs::Dir::open_ambient_dir(src, cap_std::ambient_authority())?;
    copy_dir(&root, Path::new(""), dest)
}

fn copy_dir(root: &cap_std::fs::Dir, rel: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest.join(rel))?;
    let entries = if rel.as_os_str().is_empty() { root.entries()? } else { root.read_dir(rel)? };
    for entry in entries {
        let entry = entry?;
        let child = rel.join(entry.file_name());
        if child == Path::new(".env") {
            continue;
        }
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir(root, &child, dest)?;
        } else if ty.is_file() {
            let mut from = root.open(&child)?.into_std();
            let mut to = fs::File::create(dest.join(&child)).with_context(|| format!("copia di {}", child.display()))?;
            std::io::copy(&mut from, &mut to).with_context(|| format!("copia di {}", child.display()))?;
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
/// (attivi, in prova) dei pool di un runtime PHP.
pub fn runtime_dirs(p: &Paths, runtime: &str) -> (PathBuf, PathBuf) {
    if runtime == alveare_core::php::DEFAULT_RUNTIME {
        return (p.pools.clone(), p.staging.clone());
    }
    let (active, staging) = alveare_core::php::runtime_pool_dirs(runtime);
    let root = p.pools.parent().unwrap_or(&p.pools);
    (root.join(active), root.join(staging))
}

/// passwd/group sempre; in staging solo i pool cambiati, ognuno nel suo runtime; via i pool
/// di siti che non ne hanno più (o che hanno cambiato runtime).
pub fn write_php_files(p: &Paths, users: &[SitePhp], pools: &[SitePhp], extra_runtimes: &[String]) -> Result<()> {
    fs::create_dir_all(&p.php_etc)?;
    let users: Vec<&SitePhp> = users.iter().collect();
    write_atomic(&p.php_etc.join("passwd"), &render_passwd(&users))?;
    write_atomic(&p.php_etc.join("group"), &render_group(&users))?;
    let runtimes = std::iter::once(alveare_core::php::DEFAULT_RUNTIME.to_string()).chain(extra_runtimes.iter().cloned());
    for rt in runtimes {
        let (active, staging) = runtime_dirs(p, &rt);
        fs::create_dir_all(&active)?;
        fs::create_dir_all(&staging)?;
        let mine: Vec<&SitePhp> = pools.iter().filter(|s| s.runtime == rt).collect();
        let wanted: Vec<String> = mine.iter().map(|s| format!("{}.conf", s.slug.as_str())).collect();
        for (site, name) in mine.iter().zip(&wanted) {
            let conf = render_pool(site);
            if fs::read_to_string(active.join(name)).ok().as_deref() != Some(conf.as_str()) {
                fs::write(staging.join(name), conf)?;
            }
        }
        for entry in fs::read_dir(&active)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if name.ends_with(".conf") && !wanted.contains(&name) {
                fs::remove_file(active.join(&name))?;
            }
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

/// POST all'Admin API di Caddy sul socket Unix; restituisce il corpo se lo stato è 200.
async fn caddy_request(socket: &Path, path: &str, caddyfile: &str) -> Result<String> {
    caddy_admin(socket, "POST", path, caddyfile).await
}

/// Una richiesta all'Admin API (corpo vuoto = GET senza Content-Type).
async fn caddy_admin(socket: &Path, method: &str, path: &str, caddyfile: &str) -> Result<String> {
    let mut stream = UnixStream::connect(socket).await.with_context(|| format!("connessione a Caddy ({})", socket.display()))?;
    let ctype = if caddyfile.is_empty() { "" } else { "Content-Type: text/caddyfile\r\n" };
    let head = format!("{method} {path} HTTP/1.1\r\nHost: caddy\r\n{ctype}Content-Length: {}\r\nConnection: close\r\n\r\n", caddyfile.len());
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(caddyfile.as_bytes()).await?;
    let mut resp = Vec::new();
    tokio::time::timeout(Duration::from_secs(30), stream.read_to_end(&mut resp)).await.context("Caddy non ha risposto")??;
    let split = resp.windows(4).position(|w| w == b"\r\n\r\n").map_or(resp.len(), |i| i + 4);
    let head = String::from_utf8_lossy(&resp[..split]);
    let status = head.split(' ').nth(1).unwrap_or_default();
    let body = dechunk(&resp[split..]);
    if status != "200" {
        bail!("Caddy ha rifiutato la configurazione ({status}): {body}");
    }
    Ok(body)
}

/// Il corpo può arrivare in chunked encoding: si ricompone.
fn dechunk(body: &[u8]) -> String {
    // Le dimensioni sono in byte e un chunk può finire a metà di un carattere: si ricompone in byte.
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let Some(eol) = rest.windows(2).position(|w| w == b"\r\n") else { return String::from_utf8_lossy(body).into_owned() };
        let Some(n) = std::str::from_utf8(&rest[..eol]).ok().and_then(|s| usize::from_str_radix(s.trim(), 16).ok()) else {
            return String::from_utf8_lossy(body).into_owned();
        };
        let after = &rest[eol + 2..];
        if n == 0 || after.len() < n {
            return if out.is_empty() && n != 0 { String::from_utf8_lossy(body).into_owned() } else { String::from_utf8_lossy(&out).into_owned() };
        }
        out.extend_from_slice(&after[..n]);
        rest = after[n..].strip_prefix(b"\r\n").unwrap_or(&after[n..]);
    }
}

/// POST /load all'Admin API di Caddy sul socket Unix: valida e applica, oppure rifiuta tutto.
pub async fn caddy_load(socket: &Path, caddyfile: &str) -> Result<()> {
    caddy_request(socket, "/load", caddyfile).await.map(drop)
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

/// GET con `Host` e, se serve, Basic auth: stato HTTP e inizio del corpo (max 4 KB). Come dietro
/// Cloudflare, con X-Forwarded-Proto https, così WordPress non risponde con un redirect a https.
pub async fn http_get(addr: &str, host: &Domain, path: &str, auth: Option<(&str, &str)>, timeout: Duration) -> Result<(u16, String)> {
    use base64::Engine;
    let call = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.with_context(|| format!("connessione a {addr}"))?;
        let auth = auth
            .map(|(u, p)| format!("Authorization: Basic {}\r\n", base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"))))
            .unwrap_or_default();
        let req = format!("GET {path} HTTP/1.1\r\nHost: {}\r\nX-Forwarded-Proto: https\r\nUser-Agent: alveare\r\n{auth}Connection: close\r\n\r\n", host.as_str());
        stream.write_all(req.as_bytes()).await?;
        let mut buf = Vec::new();
        stream.take(64 * 1024).read_to_end(&mut buf).await?;
        let text = String::from_utf8_lossy(&buf);
        let status = text.split(' ').nth(1).and_then(|c| c.parse().ok()).with_context(|| format!("risposta non HTTP: {}", text.lines().next().unwrap_or_default()))?;
        let body: String = text.split_once("\r\n\r\n").map_or("", |(_, b)| b).chars().take(4096).collect();
        Ok((status, body))
    };
    tokio::time::timeout(timeout, call).await.context("il sito non ha risposto in tempo")?
}

pub async fn http_probe(addr: &str, host: &Domain, auth: Option<(&str, &str)>, timeout: Duration) -> Result<u16> {
    http_get(addr, host, "/", auth, timeout).await.map(|(s, _)| s)
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
    /// Caddy in HTTP sulla rete interna, per le sonde (restore di prova, monitoraggio).
    pub caddy_http: String,
    /// (token del bot, chat) per gli avvisi su Telegram.
    pub telegram: Option<(String, String)>,
    pub http: reqwest::Client,
    pub cloudflare: Vec<crate::cloudflare::Account>,
    /// Base dell'API di Cloudflare (sostituibile nei test).
    pub cloudflare_api: String,
    /// Runtime PHP aggiuntivi (container phpXX, canary), oltre a quello principale.
    pub php_runtimes: Vec<String>,
    /// Runtime di ogni sito, aggiornato a ogni sync_php: decide a quale agente parlare.
    pub site_runtimes: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SystemHost {
    fn cf_account(&self, label: &str) -> Result<&crate::cloudflare::Account> {
        self.cloudflare.iter().find(|a| a.label == label).with_context(|| format!("account Cloudflare sconosciuto: {label}"))
    }

    fn cf(&self) -> crate::cloudflare::Client {
        crate::cloudflare::Client::new(self.http.clone(), &self.cloudflare_api)
    }
}

impl SystemHost {
    /// Socket dell'agente di un runtime, accanto a quello principale.
    fn agent_path(&self, runtime: &str) -> PathBuf {
        if runtime == alveare_core::php::DEFAULT_RUNTIME {
            self.paths.agent.clone()
        } else {
            self.paths.agent.with_file_name(format!("agent-{runtime}.sock"))
        }
    }

    /// Il comando va all'agente del runtime del sito (WP-CLI e script girano con la sua versione di PHP).
    async fn agent_for(&self, slug: &Slug, cmd: Command, timeout: Duration) -> Result<Response> {
        let runtime = self.site_runtimes.lock().expect("lock runtime").get(slug.as_str()).cloned().unwrap_or_else(|| alveare_core::php::DEFAULT_RUNTIME.into());
        agent_call(&self.agent_path(&runtime), &cmd, timeout).await
    }

    fn workers_socket(&self) -> PathBuf {
        self.paths.agent.with_file_name("workers.sock")
    }

    fn all_runtimes(&self) -> Vec<String> {
        std::iter::once(alveare_core::php::DEFAULT_RUNTIME.to_string()).chain(self.php_runtimes.iter().cloned()).collect()
    }

    async fn restic(&self, args: &[String]) -> Result<String> {
        // Con un altro restic in corso (backup notturno e manuale) aspetta il lock invece di fallire.
        let out = tokio::process::Command::new("restic")
            .args(["--retry-lock", "30m"])
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
        *self.site_runtimes.lock().expect("lock runtime") = users.iter().map(|u| (u.slug.as_str().to_string(), u.runtime.clone())).collect();
        write_php_files(&self.paths, users, pools, &self.php_runtimes)?;
        #[derive(serde::Deserialize)]
        struct Report {
            rejected: Vec<(String, String)>,
        }
        let mut rejected = Vec::new();
        for rt in self.all_runtimes() {
            let used = pools.iter().any(|p| p.runtime == rt) || rt == alveare_core::php::DEFAULT_RUNTIME;
            let resp = match agent_call(&self.agent_path(&rt), &Command::PoolApply, AGENT_TIMEOUT).await {
                Ok(r) => r,
                // Un runtime senza siti può essere spento (profilo del compose non attivo).
                Err(_) if !used => continue,
                Err(e) => return Err(e.context(format!("runtime PHP {rt}"))),
            };
            let report: Report = serde_json::from_str(&resp.output).with_context(|| format!("pool.apply ({rt}): {}", resp.output))?;
            if !resp.ok && report.rejected.is_empty() {
                bail!("reload di PHP-FPM fallito (runtime {rt})");
            }
            rejected.extend(report.rejected.into_iter().map(|(name, _)| name));
        }
        Ok(rejected)
    }

    async fn sync_caddy(&self, sites: &[CaddySite], global: Option<&str>) -> Result<()> {
        let text = render_caddyfile(&CaddyConfig {
            panel_host: self.panel_host.clone(),
            adminer_host: self.adminer_host.clone(),
            sites: sites.to_vec(),
            global_extra: global.map(String::from),
        });
        fs::create_dir_all(&self.paths.caddy)?;
        caddy_load(&self.paths.caddy_admin, &text).await?;
        // Su disco solo una config che Caddy ha accettato: è quella usata al riavvio.
        write_atomic(&self.paths.caddy.join("Caddyfile"), &text)
    }

    async fn caddy_adapt(&self, caddyfile: &str) -> Result<serde_json::Value> {
        let body = caddy_request(&self.paths.caddy_admin, "/adapt", caddyfile).await?;
        let v: serde_json::Value = serde_json::from_str(&body).context("risposta di /adapt non valida")?;
        Ok(v.get("result").cloned().unwrap_or(v))
    }

    async fn write_php_global(&self, ini: &str) -> Result<()> {
        fs::create_dir_all(&self.paths.php_conf)?;
        write_atomic(&self.paths.php_conf.join("zz-custom.ini"), &format!("; Override globale dal pannello Alveare: non modificare a mano.\n{ini}\n"))
    }

    async fn wp(&self, slug: &Slug, args: &[String]) -> Result<String> {
        let (ok, output) = self.wp_output(slug, args).await?;
        if !ok {
            bail!("wp {}: {}", args.join(" "), output.trim());
        }
        Ok(output)
    }

    async fn wp_output(&self, slug: &Slug, args: &[String]) -> Result<(bool, String)> {
        let resp = self.agent_for(slug, Command::Wp { slug: slug.clone(), args: args.to_vec() }, AGENT_TIMEOUT).await?;
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

    async fn db_import(&self, db: &DbCreds, mut sql: std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>) -> Result<()> {
        // Utente del sito (solo il suo database) e --sandbox: niente comandi del client come \! o source.
        let mut child = tokio::process::Command::new("mariadb")
            .args(["--default-character-set=utf8mb4", "--sandbox", "--local-infile=0", "-h", &self.db_host, "-u"])
            .arg(sql_safe(&db.user)?)
            .arg("--")
            .arg(sql_safe(&db.name)?)
            .env("MYSQL_PWD", &db.password)
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

    async fn restore(&self, slug: &Slug, snapshot: &str, target: &Slug, target_uid: u32, files: &FilesScope, db: Option<&DbCreds>) -> Result<()> {
        if !valid_snapshot(snapshot) {
            bail!("snapshot non valido: {snapshot}");
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let staging = self.paths.backups.join(format!("restore-{}-{now}", target.as_str()));
        let (src_dir, target_dir) = (self.paths.sites.join(slug.as_str()), self.paths.sites.join(target.as_str()));
        let dump = self.paths.backups.join(slug.as_str()).join("db.sql");
        let wanted = match files {
            FilesScope::None => None,
            FilesScope::All => Some(PathBuf::new()),
            FilesScope::Path(rel) => Some(PathBuf::from(crate::flows::site_rel_path(rel)?)),
        };
        let mut args = vec!["restore".to_string(), snapshot.to_string(), "--target".into(), staging.to_string_lossy().into_owned()];
        if let Some(rel) = &wanted {
            args.extend(["--include".into(), src_dir.join(rel).to_string_lossy().into_owned()]);
        }
        if db.is_some() {
            args.extend(["--include".into(), dump.to_string_lossy().into_owned()]);
        }
        let result = async {
            self.restic(&args).await?;
            // restic ricrea i percorsi assoluti sotto la cartella di destinazione.
            let inside = |p: &Path| staging.join(p.strip_prefix("/").unwrap_or(p));
            if let Some(rel) = &wanted {
                let restored = inside(&src_dir.join(rel));
                if fs::symlink_metadata(&restored).is_err() {
                    bail!("lo snapshot non contiene {}", if rel.as_os_str().is_empty() { "i file del sito".into() } else { rel.display().to_string() });
                }
                // Spostamenti relativi a Dir (cap-std): un link nelle cartelle del sito non porta altrove.
                let ambient = cap_std::ambient_authority();
                let trash = self.paths.trash.join(format!("{}-{now}", target.as_str())).join("site");
                if rel.as_os_str().is_empty() {
                    // Tutto il sito: si sostituisce la cartella intera, il cui genitore è del pannello.
                    fs::create_dir_all(trash.parent().context("cestino")?)?;
                    if fs::symlink_metadata(&target_dir).is_ok() {
                        fs::rename(&target_dir, &trash)?;
                    }
                    fs::rename(&restored, &target_dir)?;
                } else {
                    fs::create_dir_all(&trash)?;
                    let (site, bin) = (cap_std::fs::Dir::open_ambient_dir(&target_dir, ambient)?, cap_std::fs::Dir::open_ambient_dir(&trash, ambient)?);
                    let from = cap_std::fs::Dir::open_ambient_dir(restored.parent().context("percorso senza cartella")?, ambient)?;
                    let restored_name = restored.file_name().context("percorso senza nome")?;
                    if site.symlink_metadata(rel).is_ok() {
                        if let Some(parent) = rel.parent().filter(|p| !p.as_os_str().is_empty()) {
                            bin.create_dir_all(parent)?;
                        }
                        site.rename(rel, &bin, rel)?;
                    }
                    if let Some(parent) = rel.parent().filter(|p| !p.as_os_str().is_empty()) {
                        site.create_dir_all(parent)?;
                    }
                    from.rename(restored_name, &site, rel)?;
                }
                let current = target_dir.join(rel);
                if target != slug {
                    chown_tree(&current, Some(target_uid))?;
                }
            }
            if let Some(db) = db {
                let restored = inside(&dump);
                let file = tokio::fs::File::open(&restored).await.context("lo snapshot non contiene il dump del database")?;
                // Il database attuale si salva prima di cancellarlo: se l'import fallisce torna com'era.
                let previous = staging.join("db-prima-del-ripristino.sql");
                let dump = self.db_dump(&db.name, &[]).await?;
                let mut out = tokio::fs::File::create(&previous).await?;
                let mut reader = dump.reader;
                tokio::io::copy(&mut reader, &mut out).await?;
                dump.finished.await?;
                self.drop_database(&db.name, &db.user).await?;
                self.create_database(db).await?;
                if let Err(e) = self.db_import(db, Box::pin(tokio::io::BufReader::new(file))).await {
                    self.drop_database(&db.name, &db.user).await?;
                    self.create_database(db).await?;
                    let old = tokio::fs::File::open(&previous).await?;
                    self.db_import(db, Box::pin(tokio::io::BufReader::new(old))).await.context("ripristino del database di prima")?;
                    return Err(e.context("import del dump dello snapshot: il database è tornato com'era"));
                }
            }
            Ok(())
        }
        .await;
        let _ = fs::remove_dir_all(&staging);
        result
    }

    async fn snapshot_ls(&self, slug: &Slug, snapshot: &str, rel: &str) -> Result<Vec<crate::backup::SnapEntry>> {
        if !valid_snapshot(snapshot) {
            bail!("snapshot non valido: {snapshot}");
        }
        let site_dir = self.paths.sites.join(slug.as_str());
        let dir = if rel.trim_matches('/').is_empty() { site_dir } else { site_dir.join(crate::flows::site_rel_path(rel)?) };
        let dir = dir.to_string_lossy().into_owned();
        let out = self.restic(&["ls".into(), "--json".into(), snapshot.into(), dir.clone()]).await?;
        Ok(crate::backup::parse_ls(&out, &dir))
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

    async fn import_files(&self, slug: &Slug, _uid: u32, archive: &Path) -> Result<crate::flows::Imported> {
        // Proprietario sistemato dopo, da install_template, insieme al resto del sito.
        let (public, archive) = (self.paths.sites.join(slug.as_str()).join("public"), archive.to_path_buf());
        tokio::task::spawn_blocking(move || crate::importer::import_files(&public, &archive)).await?
    }

    async fn export_site(&self, slug: &Slug, db_name: &str, dest: &Path, env_example: &str, readme: &str) -> Result<u64> {
        let work = self.paths.backups.join(slug.as_str());
        fs::create_dir_all(&work)?;
        set_mode(&work, 0o700)?;
        let dump = work.join("export.sql");
        self.dump_to_file(db_name, &dump).await.context("dump del database")?;
        let dir = dest.parent().context("destinazione senza cartella")?.to_path_buf();
        let (site_dir, dest, prefix) = (self.paths.sites.join(slug.as_str()), dest.to_path_buf(), PathBuf::from(slug.as_str()));
        let (env_example, readme, dump2) = (env_example.to_string(), readme.to_string(), dump.clone());
        let result = tokio::task::spawn_blocking(move || {
            fs::create_dir_all(&dir)?;
            set_mode(&dir, 0o700)?;
            let partial = dest.with_extension("partial");
            let mut w = crate::archive::TarGzWriter::new(fs::File::create(&partial)?);
            let site = cap_std::fs::Dir::open_ambient_dir(&site_dir, cap_std::ambient_authority())?;
            w.add_tree_except(&site, Path::new(""), &prefix, &[".env"])?;
            w.add_bytes(&prefix.join(".env.example"), env_example.as_bytes())?;
            w.add_file(&dump2, &prefix.join("database.sql"))?;
            w.add_bytes(&prefix.join("LEGGIMI.txt"), readme.as_bytes())?;
            w.finish()?;
            fs::rename(&partial, &dest)?;
            Ok::<_, anyhow::Error>(fs::metadata(&dest)?.len())
        })
        .await?;
        let _ = fs::remove_file(&dump);
        result
    }

    async fn store_template(&self, slug: &Slug, name: &Slug, manifest: &crate::template::Manifest, sql_file: &str) -> Result<()> {
        let (site_dir, sql) = (self.paths.sites.join(slug.as_str()), self.paths.tmp.join(slug.as_str()).join(sql_file));
        let dest = self.paths.templates.join(name.as_str());
        let toml = toml::to_string(manifest)?;
        tokio::task::spawn_blocking(move || {
            if dest.exists() {
                bail!("esiste già un template con questo nome");
            }
            let partial = dest.with_extension("partial");
            let _ = fs::remove_dir_all(&partial);
            let result = (|| {
                copy_site_tree(&site_dir, &partial.join("files"))?;
                // Il dump sta nel tmp del sito: si legge senza seguire link.
                fs::write(partial.join("db.sql"), read_nofollow(&sql).context("dump del template")?)?;
                fs::write(partial.join("template.toml"), &toml)?;
                fs::rename(&partial, &dest)?;
                Ok(())
            })();
            let _ = fs::remove_file(&sql);
            if result.is_err() {
                let _ = fs::remove_dir_all(&partial);
            }
            result
        })
        .await?
    }

    async fn probe(&self, host: &Domain, auth: Option<(&str, &str)>) -> Result<u16> {
        http_probe(&self.caddy_http, host, auth, Duration::from_secs(30)).await
    }

    async fn sync_workers(&self, specs: &[alveare_core::agent::WorkerSpec]) -> Result<()> {
        let path = self.paths.php_etc.parent().unwrap_or(&self.paths.php_etc).join("workers.json");
        write_atomic(&path, &serde_json::to_string_pretty(specs)?)?;
        match agent_call(&self.workers_socket(), &Command::WorkersApply, AGENT_TIMEOUT).await {
            Ok(r) if r.ok => Ok(()),
            Ok(r) => bail!("php-workers: {}", r.output),
            // Senza worker il container può essere spento (profilo workers non attivo).
            Err(_) if specs.is_empty() => Ok(()),
            Err(e) => Err(e.context("container php-workers non raggiungibile: docker compose --profile workers up -d")),
        }
    }

    async fn worker_restart(&self, slug: &Slug, name: &str) -> Result<()> {
        let r = agent_call(&self.workers_socket(), &Command::WorkerRestart { slug: slug.clone(), name: name.into() }, AGENT_TIMEOUT).await?;
        if !r.ok {
            bail!("riavvio dei worker: {}", r.output);
        }
        Ok(())
    }

    async fn workers_status(&self) -> Result<serde_json::Value> {
        let r = agent_call(&self.workers_socket(), &Command::WorkersStatus, Duration::from_secs(10)).await?;
        Ok(serde_json::from_str(&r.output)?)
    }

    async fn deploy(&self, slug: &Slug, repo: &str, branch: &str) -> Result<(bool, String)> {
        // Ogni passo ha il suo timeout nell'agente: il pannello aspetta il totale.
        let r = self.agent_for(slug, Command::Deploy { slug: slug.clone(), repo: repo.into(), branch: branch.into() }, Duration::from_secs(3600)).await?;
        Ok((r.ok, r.output))
    }

    async fn deploy_key(&self, slug: &Slug) -> Result<String> {
        let r = self.agent_for(slug, Command::DeployKey { slug: slug.clone() }, Duration::from_secs(60)).await?;
        if !r.ok {
            bail!("chiave di deploy: {}", r.output);
        }
        Ok(r.output)
    }

    async fn caddy_metrics(&self) -> Result<String> {
        caddy_admin(&self.paths.caddy_admin, "GET", "/metrics", "").await
    }

    fn cloudflare_accounts(&self) -> Vec<String> {
        self.cloudflare.iter().map(|a| a.label.clone()).collect()
    }

    async fn tunnel_add(&self, account: &str, host: &Domain) -> Result<()> {
        self.cf().add_hostname(self.cf_account(account)?, host).await
    }

    async fn tunnel_remove(&self, account: &str, host: &Domain) -> Result<()> {
        self.cf().remove_hostname(self.cf_account(account)?, host).await
    }

    async fn cloudflare_rules(&self, account: &str, host: &Domain, template: &serde_json::Value) -> Result<usize> {
        self.cf().apply_rules(self.cf_account(account)?, host, template).await
    }

    async fn notify(&self, text: &str) -> Result<bool> {
        let Some((token, chat)) = &self.telegram else { return Ok(false) };
        let resp = self
            .http
            .post(format!("https://api.telegram.org/bot{token}/sendMessage"))
            .json(&serde_json::json!({ "chat_id": chat, "text": text, "disable_web_page_preview": true }))
            .timeout(Duration::from_secs(20))
            .send()
            .await
            // L'URL contiene il token del bot: fuori dal messaggio d'errore.
            .map_err(|e| anyhow::anyhow!("Telegram non raggiungibile: {}", e.without_url()))?;
        if !resp.status().is_success() {
            bail!("Telegram ha risposto {}: {}", resp.status(), resp.text().await.unwrap_or_default());
        }
        Ok(true)
    }

    async fn site_disk_usage(&self, slug: &Slug) -> Result<u64> {
        let dir = self.paths.sites.join(slug.as_str());
        Ok(tokio::task::spawn_blocking(move || crate::monitor::dir_size(&dir)).await?)
    }

    async fn http_get(&self, host: &Domain, path: &str, auth: Option<(&str, &str)>, timeout_s: u32) -> Result<(u16, String)> {
        http_get(&self.caddy_http, host, path, auth, Duration::from_secs(u64::from(timeout_s))).await
    }

    async fn php(&self, slug: &Slug, args: &[String], timeout_s: u32) -> Result<(bool, String)> {
        // Il timeout lo applica l'agente; il pannello aspetta un minuto in più.
        let cmd = Command::Php { slug: slug.clone(), args: args.to_vec(), timeout_s };
        let resp = self.agent_for(slug, cmd, Duration::from_secs(u64::from(timeout_s) + 60)).await?;
        Ok((resp.ok, resp.output))
    }

    async fn copy_site_files(&self, from: &Slug, to: &Slug, uid: u32) -> Result<()> {
        let (src, dest) = (self.paths.sites.join(from.as_str()), self.paths.sites.join(to.as_str()));
        tokio::task::spawn_blocking(move || {
            copy_site_tree(&src, &dest)?;
            chown_tree(&dest, Some(uid))
        })
        .await?
    }

    async fn read_site_env(&self, slug: &Slug) -> Result<Vars> {
        let path = self.paths.sites.join(slug.as_str()).join(".env");
        Ok(crate::template::parse_env(&read_nofollow(&path).context("lettura del .env")?))
    }

    async fn write_site_env(&self, slug: &Slug, uid: u32, env: &Vars) -> Result<()> {
        write_env(&self.paths, slug, env, Some(uid))
    }

    async fn set_site_env(&self, slug: &Slug, uid: u32, key: &str, value: &str) -> Result<()> {
        let path = self.paths.sites.join(slug.as_str()).join(".env");
        let current = read_nofollow(&path).context("lettura del .env")?;
        let updated = crate::database::replace_env_value(&current, key, value)?;
        replace_site_file(&path, &updated, 0o400, Some(uid))
    }

    async fn reopen_fpm_logs(&self) -> Result<()> {
        for rt in self.all_runtimes() {
            match agent_call(&self.agent_path(&rt), &Command::FpmReopenLogs, AGENT_TIMEOUT).await {
                Ok(resp) if !resp.ok => bail!("riapertura dei log di FPM ({rt}): {}", resp.output),
                Ok(_) => {}
                Err(e) if rt == alveare_core::php::DEFAULT_RUNTIME => return Err(e),
                Err(_) => {}
            }
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
        SitePhp { slug: slug(s), uid, settings: PhpSettings::default(), extra: vec![], runtime: alveare_core::php::DEFAULT_RUNTIME.into() }
    }

    #[tokio::test]
    async fn probe_sends_host_and_basic_auth_and_reads_the_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 2048];
            let n = sock.read(&mut buf).await.unwrap();
            sock.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n").await.unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        let status = http_probe(&addr, &Domain::parse("sona.it").unwrap(), Some(("staging", "pw")), Duration::from_secs(5)).await.unwrap();
        let request = server.await.unwrap();
        assert_eq!(status, 503);
        assert!(request.contains("Host: sona.it\r\n") && request.contains("X-Forwarded-Proto: https\r\n"));
        assert!(request.contains("Authorization: Basic c3RhZ2luZzpwdw==\r\n"), "{request}");
    }

    #[test]
    fn site_files_are_never_written_or_read_through_links() {
        let (t, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let victim = other.path().join("index.php");
        fs::write(&victim, "originale").unwrap();
        let env = t.path().join(".env");
        fs::write(&env, "A='1'\n").unwrap();
        std::os::unix::fs::symlink(&victim, t.path().join(".new")).unwrap();
        std::os::unix::fs::symlink(&victim, env.with_extension("new")).unwrap();
        replace_site_file(&env, "A='2'\n", 0o400, None).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "originale", "il link non si segue");
        assert_eq!(fs::read_to_string(&env).unwrap(), "A='2'\n");
        let link = t.path().join("link.env");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(read_nofollow(&link).is_err(), "lettura attraverso un link rifiutata");
    }

    #[test]
    fn chunked_bodies_are_reassembled() {
        assert_eq!(dechunk(b"5\r\nciao \r\n5\r\nmondo\r\n0\r\n\r\n"), "ciao mondo");
        assert_eq!(dechunk(b"{\"a\":1}"), "{\"a\":1}");
        // Le dimensioni dei chunk sono in byte: il confine può cadere a metà di "à".
        assert_eq!(dechunk(b"3\r\nci\xc3\r\n2\r\n\xa0o\r\n0\r\n\r\n"), "ci\u{e0}o");
    }

    #[test]
    fn clone_copy_skips_env_and_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let (src, dest) = (t.path().join("a"), t.path().join("b"));
        fs::create_dir_all(src.join("public/wp-content")).unwrap();
        fs::write(src.join(".env"), "DB_PASSWORD='x'").unwrap();
        fs::write(src.join("public/wp-content/a.txt"), "ciao").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.join("public/link")).unwrap();
        copy_site_tree(&src, &dest).unwrap();
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
        write_php_files(&p, &[a.clone(), b.clone()], &[a, b], &[]).unwrap();
        assert!(!p.staging.join("a_it.conf").exists(), "pool invariato non va rivalidato");
        assert!(p.staging.join("b_it.conf").exists());
        assert!(!p.pools.join("old_it.conf").exists());
        let passwd = fs::read_to_string(p.php_etc.join("passwd")).unwrap();
        assert!(passwd.contains("\nb_it:x:20002:20002:"));
        assert!(p.php_etc.join("group").exists());
    }

    #[test]
    fn pools_move_between_runtimes() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(&t);
        let a = php("a_it", 20001);
        let runtimes = ["8.4".to_string()];
        write_php_files(&p, std::slice::from_ref(&a), std::slice::from_ref(&a), &runtimes).unwrap();
        fs::rename(p.staging.join("a_it.conf"), p.pools.join("a_it.conf")).unwrap();
        let moved = SitePhp { runtime: "8.4".into(), ..a };
        write_php_files(&p, std::slice::from_ref(&moved), std::slice::from_ref(&moved), &runtimes).unwrap();
        let (dir84, staging84) = runtime_dirs(&p, "8.4");
        assert!(staging84.join("a_it.conf").exists(), "nuovo pool nel runtime 8.4");
        assert!(fs::read_to_string(staging84.join("a_it.conf")).unwrap().contains("/run/php/a_it.8.4.sock"));
        assert!(!p.pools.join("a_it.conf").exists(), "tolto dal runtime di prima");
        assert!(dir84.is_dir());
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
