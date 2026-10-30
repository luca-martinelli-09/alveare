//! Scheduler del pannello: un tick al minuto, ora di Roma. `due` decide cosa tocca adesso
//! (puro, testabile); `run` lo esegue. I siti sono sfalsati per non partire tutti insieme.
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Datelike, Timelike};
use chrono_tz::Tz;

use crate::api::App;
use crate::files::SiteFiles;
use crate::flows::{self, Host};
use crate::store::{SiteKind, SiteStatus};

const MB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct SiteRef {
    pub id: i64,
    pub wordpress: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    WpCron(i64),
    Backup(i64),
    PanelBackup,
    RotateLogs,
    Integrity(i64),
    /// Aggiornamenti di WordPress secondo la regola del sito, dopo il backup notturno.
    NightlyUpdate(i64),
    /// Raggiungibilità del sito, ogni 5 minuti.
    Uptime(i64),
    /// Spazio di file e database, ogni notte.
    Usage(i64),
    /// Errori 5xx e richieste lente dell'ultima ora, dall'access log.
    Traffic(i64),
    /// Avvisi nuovi o risolti verso Telegram.
    Notify,
    /// Cron personalizzato arrivato alla sua ora.
    CustomCron(i64),
    /// Blocchi IP scaduti: si tolgono dal Caddyfile.
    ExpireBlocks,
    /// Restore di prova mensile su un dominio interno: un backup mai ripristinato non conta.
    RestoreTest(i64),
    Cleanup,
}

pub const TRASH_DAYS: i64 = 7;

/// `<ts>-<nome>` (cestino dei file) o `<slug>-<ts>` (cestino dei siti). Prima il prefisso:
/// uno slug inizia sempre con una lettera, mentre un nome può finire con `-2024`.
fn stamp(name: &str) -> Option<i64> {
    name.split_once('-').and_then(|(ts, _)| ts.parse().ok()).or_else(|| name.rsplit_once('-').and_then(|(_, ts)| ts.parse().ok()))
}

/// Elimina le voci più vecchie di `max_age` secondi; restituisce quante.
pub fn purge_older(dir: &std::path::Path, now: i64, max_age: i64) -> std::io::Result<usize> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        let Some(ts) = stamp(&entry.file_name().to_string_lossy()) else { continue };
        if now - ts > max_age {
            let path = entry.path();
            if entry.file_type()?.is_dir() { std::fs::remove_dir_all(&path)? } else { std::fs::remove_file(&path)? }
            removed += 1;
        }
    }
    Ok(removed)
}

/// Problemi veri dall'output JSON di `wp plugin verify-checksums`: i plugin non su
/// wordpress.org (premium) non hanno checksum e non sono un problema.
pub fn plugin_problems(json: &str) -> Vec<String> {
    let start = json.find('[').unwrap_or(json.len());
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json[start..]).unwrap_or_default();
    rows.iter()
        .filter(|r| !r["message"].as_str().unwrap_or_default().starts_with("Could not retrieve"))
        .map(|r| format!("{}/{}: {}", r["plugin_name"].as_str().unwrap_or("?"), r["file"].as_str().unwrap_or("?"), r["message"].as_str().unwrap_or("?")))
        .collect()
}

pub fn due(now: DateTime<Tz>, sites: &[SiteRef]) -> Vec<Task> {
    let (h, m) = (now.hour() as i64, now.minute() as i64);
    let mut out = Vec::new();
    if m % 5 == 0 {
        out.push(Task::RotateLogs);
    }
    out.push(Task::ExpireBlocks);
    out.push(Task::Notify);
    if (h, m) == (4, 30) {
        out.push(Task::Cleanup);
    }
    if (h, m) == (1, 50) {
        out.push(Task::PanelBackup);
    }
    for s in sites.iter().filter(|s| s.active) {
        if m % 5 == (s.id + 2) % 5 {
            out.push(Task::Uptime(s.id));
        }
        if m == (s.id * 17) % 60 {
            out.push(Task::Traffic(s.id));
        }
        if (h, m) == (4, (s.id * 13) % 60) {
            out.push(Task::Usage(s.id));
        }
        if crate::backup::backup_slot(s.id) == (h as u32, m as u32) {
            out.push(Task::Backup(s.id));
        }
        // Un sito al giorno al massimo (fino a 28 siti), alle 6 dopo i backup.
        if now.day() as i64 == 1 + (s.id * 11).rem_euclid(28) && (h, m) == (6, (s.id * 7) % 60) {
            out.push(Task::RestoreTest(s.id));
        }
    }
    for s in sites.iter().filter(|s| s.active && s.wordpress) {
        if m % 5 == s.id % 5 {
            out.push(Task::WpCron(s.id));
        }
        if h == 5 && m == (s.id * 7) % 60 {
            out.push(Task::Integrity(s.id));
        }
        // 45 minuti dopo il backup del sito.
        let (bh, bm) = crate::backup::backup_slot(s.id);
        let after = bh * 60 + bm + 45;
        if (h as u32, m as u32) == (after / 60, after % 60) {
            out.push(Task::NightlyUpdate(s.id));
        }
    }
    out
}


const ROME: Tz = chrono_tz::Europe::Rome;

/// Un tick al minuto per sempre. Ogni compito gira in un task suo: uno lento non ferma gli altri.
pub async fn run<H: Host>(app: App<H>) {
    let busy: Arc<Mutex<HashSet<(i64, &'static str)>>> = Default::default();
    loop {
        let now = chrono::Utc::now();
        let wait = 60 - now.second() as u64;
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        let now = chrono::Utc::now().with_timezone(&ROME);
        let sites = match app.ctx.store.sites().await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("scheduler: lettura dei siti: {e}");
                continue;
            }
        };
        let refs: Vec<SiteRef> = sites
            .iter()
            .map(|s| SiteRef { id: s.id, wordpress: s.kind == SiteKind::Wordpress, active: s.status == SiteStatus::Active })
            .collect();
        let mut tasks = due(now, &refs);
        match app.ctx.store.crons(None).await {
            Ok(crons) => tasks.extend(
                crons
                    .iter()
                    .filter(|c| refs.iter().any(|s| s.id == c.site_id && s.active) && crate::crons::is_due(&c.expression, &now))
                    .map(|c| Task::CustomCron(c.id)),
            ),
            Err(e) => tracing::error!("scheduler: lettura dei cron: {e}"),
        }
        for task in tasks {
            let (app, busy) = (app.clone(), busy.clone());
            tokio::spawn(async move {
                let key = match task {
                    Task::WpCron(id) => (id, "cron"),
                    Task::Backup(id) => (id, "backup"),
                    Task::PanelBackup => (0, "panel-backup"),
                    Task::Integrity(id) => (id, "integrity"),
                    Task::RestoreTest(id) => (id, "restore-test"),
                    Task::ExpireBlocks => (0, "blocks"),
                    Task::NightlyUpdate(id) => (id, "update"),
                    // Il lock impedisce esecuzioni sovrapposte dello stesso cron.
                    Task::CustomCron(id) => (id, "custom-cron"),
                    Task::Uptime(id) => (id, "uptime"),
                    Task::Usage(id) => (id, "usage"),
                    Task::Traffic(id) => (id, "traffic"),
                    Task::Notify => (0, "notify"),
                    Task::RotateLogs => (0, "rotate"),
                    Task::Cleanup => (0, "cleanup"),
                };
                // Mai due esecuzioni sovrapposte dello stesso compito.
                if !busy.lock().expect("lock dello scheduler").insert(key) {
                    return;
                }
                if let Err(e) = execute(&app, task).await {
                    tracing::warn!("scheduler: {task:?}: {e:#}");
                }
                busy.lock().expect("lock dello scheduler").remove(&key);
            });
        }
    }
}

async fn execute<H: Host>(app: &App<H>, task: Task) -> anyhow::Result<()> {
    match task {
        Task::WpCron(id) => {
            let site = app.ctx.store.site(id).await?;
            let args = ["cron", "event", "run", "--due-now"].map(String::from);
            app.ctx.host.wp(&site.slug, &args).await?;
        }
        Task::Backup(id) if app.ctx.backups => {
            // Come job: compare nella lista dei job del sito e non si sovrappone a crea/elimina.
            let ctx = app.ctx.clone();
            app.jobs
                .spawn("site.backup", Some(id), None, move |mut log| async move {
                    flows::backup_site(&ctx, id, &mut log).await?;
                    Ok(log)
                })
                .await?;
        }
        Task::PanelBackup if app.ctx.backups => {
            let env = crate::backup::panel_env(std::env::vars());
            let snap = app.ctx.host.backup_panel(&app.ctx.store, &app.ctx.key.encrypt(&env)).await;
            match snap {
                Ok(_) => app.ctx.store.resolve_global_alerts("panel-backup").await?,
                Err(e) => app.ctx.store.raise_alert(None, "panel-backup", &format!("backup del pannello fallito: {e:#}")).await?,
            }
        }
        Task::Backup(_) | Task::PanelBackup => {}
        Task::RotateLogs => rotate_all(app).await?,
        Task::Integrity(id) => integrity(app, id).await?,
        Task::RestoreTest(id) if app.ctx.backups => {
            let ctx = app.ctx.clone();
            app.jobs
                .spawn("site.restore_test", Some(id), None, move |mut log| async move {
                    flows::test_restore(&ctx, id, &mut log).await?;
                    Ok(log)
                })
                .await?;
        }
        Task::RestoreTest(_) => {}
        Task::Cleanup => cleanup(app).await?,
        Task::NightlyUpdate(id) => {
            let (ctx, auth, app2) = (app.ctx.clone(), app.auth.clone(), app.clone());
            app.jobs
                .spawn("wordpress.nightly", Some(id), None, move |mut log| async move {
                    for change in crate::wordpress::nightly(&ctx, id, &mut log).await? {
                        auth.audit(None, "wordpress.update", Some(id), &format!("automatico: {change}")).await?;
                    }
                    let site = ctx.store.site(id).await?;
                    let st = crate::wordpress::status(&ctx, &site.slug).await?;
                    crate::api::remember_updates(&app2, id, &st).await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
                    Ok(log)
                })
                .await?;
        }
        Task::Uptime(id) => crate::monitor::uptime(&app.ctx, id).await?,
        Task::Usage(id) => crate::monitor::usage(&app.ctx, id, app.disk_alert_mb).await?,
        Task::Traffic(id) => {
            let site = app.ctx.store.site(id).await?;
            let path = app.paths.logs.join(site.slug.as_str()).join("access.log");
            let lines = tokio::task::spawn_blocking(move || crate::logs::tail(&path, 50_000)).await??;
            let since = chrono::Utc::now().timestamp() - 3600;
            crate::monitor::record_traffic(&app.ctx, id, &crate::monitor::traffic(&lines, since)).await?;
        }
        Task::Notify => {
            flows::dispatch_alerts(&app.ctx).await?;
        }
        Task::CustomCron(id) => {
            flows::run_cron(&app.ctx, id).await?;
        }
        Task::ExpireBlocks => {
            if app.ctx.store.expire_blocks(chrono::Utc::now().timestamp()).await? > 0 {
                flows::sync_caddy(&app.ctx).await?;
            }
        }
    }
    Ok(())
}

async fn rotate_all<H: Host>(app: &App<H>) -> anyhow::Result<()> {
    let mut reopen = false;
    for site in app.ctx.store.sites().await? {
        let dir = app.paths.logs.join(site.slug.as_str());
        let owner = app.chown.then(|| site.uid());
        crate::logs::rotate(&dir.join("php-error.log"), 10 * MB, 5, owner)?;
        reopen |= crate::logs::rotate(&dir.join("php-slow.log"), 5 * MB, 3, None)?;
    }
    if reopen {
        app.ctx.host.reopen_fpm_logs().await?;
    }
    Ok(())
}

/// Checksum di core e plugin più PHP in uploads: il modo più economico di accorgersi di un sito compromesso.
async fn integrity<H: Host>(app: &App<H>, id: i64) -> anyhow::Result<()> {
    let site = app.ctx.store.site(id).await?;
    let mut problems = Vec::new();
    let core = ["core", "verify-checksums"].map(String::from);
    let (ok, out) = app.ctx.host.wp_output(&site.slug, &core).await?;
    if !ok {
        problems.extend(out.lines().filter(|l| l.contains("doesn't") || l.contains("should not")).map(String::from));
        if problems.is_empty() {
            problems.push(format!("core: {}", out.trim()));
        }
    }
    let plugins = ["plugin", "verify-checksums", "--all", "--format=json"].map(String::from);
    let (_, out) = app.ctx.host.wp_output(&site.slug, &plugins).await?;
    problems.extend(plugin_problems(&out));
    let (paths, slug) = (app.paths.clone(), site.slug.clone());
    let php = tokio::task::spawn_blocking(move || SiteFiles::open(&paths, &slug, None)?.php_in_uploads()).await??;
    problems.extend(php.into_iter().map(|p| format!("PHP in uploads: {p}")));
    if problems.is_empty() {
        app.ctx.store.resolve_alerts(id, "integrity").await?;
    } else {
        let shown: Vec<&str> = problems.iter().take(20).map(String::as_str).collect();
        let more = problems.len().saturating_sub(shown.len());
        let suffix = if more > 0 { format!("\n… e altri {more}") } else { String::new() };
        app.ctx.store.raise_alert(Some(id), "integrity", &format!("{}{suffix}", shown.join("\n"))).await?;
    }
    Ok(())
}

async fn cleanup<H: Host>(app: &App<H>) -> anyhow::Result<()> {
    let now = chrono::Utc::now().timestamp();
    let week = TRASH_DAYS * 86400;
    let mut removed = purge_older(&app.paths.trash, now, week)?;
    removed += purge_older(&app.paths.exports, now, week)?;
    app.ctx.store.expire_exports(now - week).await?;
    removed += purge_stale_uploads(&app.paths.imports.join("tmp/uploads"), 86400)?;
    for site in app.ctx.store.sites().await? {
        let logs = app.paths.logs.join(site.slug.as_str());
        if logs.exists() {
            removed += crate::logs::purge_old_logs(&logs, site.log_retention_days, std::time::SystemTime::now())?;
        }
        let tmp = app.paths.tmp.join(site.slug.as_str());
        removed += purge_older(&tmp.join(".trash"), now, week)?;
        removed += purge_stale_uploads(&tmp.join("uploads"), 86400)?;
    }
    app.auth.purge_sessions(now).await?;
    if app.ctx.backups {
        for slug in app.ctx.store.orphan_backup_slugs().await? {
            app.ctx.host.expire_deleted(&slug).await?;
        }
    }
    tracing::info!("pulizia notturna: {removed} elementi eliminati");
    Ok(())
}

/// Upload a blocchi abbandonati: le cartelle non hanno data nel nome, conta l'ultima modifica.
fn purge_stale_uploads(dir: &std::path::Path, max_age: u64) -> std::io::Result<usize> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(0) };
    let mut removed = 0;
    for entry in entries.flatten() {
        let age = entry.metadata()?.modified()?.elapsed().map(|d| d.as_secs()).unwrap_or(0);
        if age > max_age {
            std::fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> chrono::DateTime<chrono_tz::Tz> {
        chrono_tz::Europe::Rome.with_ymd_and_hms(2026, 11, 2, h, m, 0).unwrap()
    }

    fn wp(id: i64) -> SiteRef {
        SiteRef { id, wordpress: true, active: true }
    }

    #[test]
    fn wordpress_cron_runs_every_five_minutes_staggered() {
        let sites = [wp(1), wp(2), SiteRef { id: 3, wordpress: false, active: true }, SiteRef { id: 6, wordpress: true, active: false }];
        let mut runs: Vec<(u32, i64)> = Vec::new();
        for m in 0..10 {
            for t in due(at(10, m), &sites) {
                if let Task::WpCron(id) = t {
                    runs.push((m, id));
                }
            }
        }
        assert_eq!(runs, [(1, 1), (2, 2), (6, 1), (7, 2)]);
    }

    #[test]
    fn nightly_backup_at_each_site_slot_and_panel_backup_before() {
        let sites = [wp(1), SiteRef { id: 2, wordpress: false, active: true }, SiteRef { id: 3, wordpress: true, active: false }];
        let all: Vec<(u32, u32, Task)> = (0..24 * 60).flat_map(|i| due(at(i / 60, i % 60), &sites).into_iter().map(move |t| (i / 60, i % 60, t))).collect();
        let backups: Vec<_> = all.iter().filter(|(_, _, t)| matches!(t, Task::Backup(_))).collect();
        assert_eq!(backups.len(), 2, "un backup per ogni sito attivo");
        let (h, m) = crate::backup::backup_slot(2);
        assert!(backups.contains(&&(h, m, Task::Backup(2))));
        assert_eq!(all.iter().filter(|(h, m, t)| *t == Task::PanelBackup && (*h, *m) == (1, 50)).count(), 1);
    }

    #[test]
    fn each_active_site_gets_one_test_restore_a_month() {
        let sites = [wp(1), SiteRef { id: 2, wordpress: false, active: true }, SiteRef { id: 3, wordpress: true, active: false }];
        let mut runs = Vec::new();
        for day in 1..=30 {
            for i in 0..24 * 60 {
                let t = chrono_tz::Europe::Rome.with_ymd_and_hms(2026, 11, day, i / 60, i % 60, 0).unwrap();
                runs.extend(due(t, &sites).into_iter().filter(|t| matches!(t, Task::RestoreTest(_))).map(|t| (day, i / 60, t)));
            }
        }
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert!(runs.iter().all(|(d, h, _)| *d <= 28 && *h == 6), "dopo i backup notturni, mai oltre il 28: {runs:?}");
        assert_ne!(runs[0].0, runs[1].0, "siti diversi in giorni diversi");
    }

    #[test]
    fn wordpress_updates_run_once_a_night_after_the_backup() {
        let sites = [wp(1), SiteRef { id: 2, wordpress: false, active: true }];
        let all: Vec<(u32, u32, Task)> = (0..24 * 60).flat_map(|i| due(at(i / 60, i % 60), &sites).into_iter().map(move |t| (i / 60, i % 60, t))).collect();
        let updates: Vec<_> = all.iter().filter(|(_, _, t)| matches!(t, Task::NightlyUpdate(_))).collect();
        assert_eq!(updates.len(), 1, "solo i siti WordPress");
        let (bh, bm) = crate::backup::backup_slot(1);
        let (h, m, _) = updates[0];
        assert!((h * 60 + m) > bh * 60 + bm, "dopo il backup");
    }

    #[test]
    fn monitoring_cadence() {
        let sites = [wp(1), SiteRef { id: 2, wordpress: false, active: false }];
        let all: Vec<Task> = (0..24 * 60).flat_map(|i| due(at(i / 60, i % 60), &sites)).collect();
        let count = |f: fn(&Task) -> bool| all.iter().filter(|t| f(t)).count();
        assert_eq!(count(|t| matches!(t, Task::Uptime(1))), 24 * 12, "ogni 5 minuti");
        assert_eq!(count(|t| matches!(t, Task::Traffic(1))), 24, "ogni ora");
        assert_eq!(count(|t| matches!(t, Task::Usage(1))), 1, "ogni notte");
        assert_eq!(count(|t| matches!(t, Task::Uptime(2))), 0, "solo i siti attivi");
        assert_eq!(count(|t| matches!(t, Task::Notify)), 24 * 60);
    }

    #[test]
    fn log_rotation_every_five_minutes() {
        assert!(due(at(10, 0), &[]).contains(&Task::RotateLogs));
        assert!(!due(at(10, 1), &[]).contains(&Task::RotateLogs));
    }

    #[test]
    fn nightly_tasks_run_once_at_their_time() {
        let sites = [wp(1)];
        let all: Vec<Task> = (0..24 * 60).flat_map(|i| due(at(i / 60, i % 60), &sites)).collect();
        assert_eq!(all.iter().filter(|t| **t == Task::Cleanup).count(), 1);
        assert_eq!(all.iter().filter(|t| **t == Task::Integrity(1)).count(), 1);
        assert!(due(at(4, 30), &sites).contains(&Task::Cleanup));
    }

    #[test]
    fn expired_trash_entries_are_removed() {
        let t = tempfile::tempdir().unwrap();
        let now = 1_800_000_000;
        for name in [format!("sona_it-{}", now - 8 * 86400), format!("{}-tema", now - 3600), format!("{}-foto-2024", now - 60), "senza-data".to_string()] {
            std::fs::create_dir_all(t.path().join(&name).join("x")).unwrap();
        }
        let removed = purge_older(t.path(), now, TRASH_DAYS * 86400).unwrap();
        assert_eq!(removed, 1);
        let mut left: Vec<String> = std::fs::read_dir(t.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, [format!("{}-tema", now - 3600), format!("{}-foto-2024", now - 60), "senza-data".to_string()]);
        assert_eq!(purge_older(&t.path().join("manca"), now, 1).unwrap(), 0);
    }

    #[test]
    fn plugin_checksum_failures_ignore_plugins_without_checksums() {
        let json = r#"[{"plugin_name":"akismet","file":"akismet.php","message":"Checksum does not match"},
                       {"plugin_name":"premium","file":"","message":"Could not retrieve the checksums for version 2.0 of the plugin premium, skipping."},
                       {"plugin_name":"akismet","file":"x.php","message":"File was added"}]"#;
        assert_eq!(plugin_problems(json), ["akismet/akismet.php: Checksum does not match", "akismet/x.php: File was added"]);
        assert!(plugin_problems("").is_empty());
    }

    #[test]
    fn integrity_checks_are_spread_between_5_and_6() {
        let sites: Vec<SiteRef> = (1..=40).map(wp).collect();
        let mut minutes = std::collections::BTreeSet::new();
        for i in 0..24 * 60 {
            for t in due(at(i / 60, i % 60), &sites) {
                if let Task::Integrity(_) = t {
                    assert_eq!(i / 60, 5);
                    minutes.insert(i % 60);
                }
            }
        }
        assert!(minutes.len() > 20, "controlli non sfalsati: {minutes:?}");
    }
}
