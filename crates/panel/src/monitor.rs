//! Monitoraggio: raggiungibilità dei siti, spazio usato, errori 5xx e richieste lente.
//! Gli ultimi valori restano nelle impostazioni (`uptime:<id>`, `usage:<id>`, `traffic:<id>`).
use std::time::Instant;

use anyhow::Result;
use serde_json::json;

use crate::flows::{staging_password, Ctx, Host};
use crate::store::SiteStatus;

/// Fallimenti consecutivi prima dell'avviso: un timeout isolato non sveglia nessuno.
pub const DOWN_AFTER: u64 = 2;
/// Richieste oltre questa durata contano come lente.
pub const SLOW_MS: u64 = 5000;
/// Errori 5xx in un'ora oltre i quali parte l'avviso.
pub const ERRORS_PER_HOUR: u64 = 20;

#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Traffic {
    pub requests: u64,
    pub errors_5xx: u64,
    pub slow: u64,
}

/// Richieste dell'access log da `since` (timestamp) in poi.
pub fn traffic(lines: &[String], since: i64) -> Traffic {
    let mut t = Traffic::default();
    for e in lines.iter().filter_map(|l| crate::logs::parse_access(l)).filter(|e| e.ts >= since) {
        t.requests += 1;
        t.errors_5xx += u64::from(e.status >= 500);
        t.slow += u64::from(e.duration_ms >= SLOW_MS);
    }
    t
}

/// Byte occupati da una cartella, senza seguire i link.
pub fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_symlink() {
        return 0;
    }
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| dir_size(&e.path())).sum()
}

/// Un controllo di raggiungibilità: stato e tempo; avviso dopo `DOWN_AFTER` fallimenti di fila.
pub async fn uptime<H: Host>(c: &Ctx<H>, id: i64) -> Result<()> {
    let site = c.store.site(id).await?;
    if site.status != SiteStatus::Active {
        return Ok(());
    }
    let Some((domain, _)) = c.store.domains(id).await?.into_iter().next() else { return Ok(()) };
    let password = staging_password(c, id).await?;
    let started = Instant::now();
    let probe = c.host.probe(&domain, password.as_deref().map(|p| ("staging", p))).await;
    let ms = started.elapsed().as_millis() as u64;
    let key = format!("uptime:{id}");
    let previous: serde_json::Value = c.store.setting(&key).await?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    let (ok, status, error) = match &probe {
        Ok(s) => (*s < 500, *s, None),
        Err(e) => (false, 0, Some(format!("{e:#}"))),
    };
    let fails = if ok { 0 } else { previous["fails"].as_u64().unwrap_or(0) + 1 };
    let record = json!({ "ok": ok, "status": status, "ms": ms, "fails": fails, "error": error, "at": chrono::Utc::now().timestamp() });
    c.store.set_setting(&key, &record.to_string()).await?;
    c.store.record_uptime(id, chrono::Utc::now().timestamp(), ms, ok).await?;
    if ok {
        c.store.resolve_alerts(id, "down").await?;
    } else if fails >= DOWN_AFTER {
        let why = error.unwrap_or_else(|| format!("risponde {status}"));
        c.store.raise_alert(Some(id), "down", &format!("{} non risponde da {fails} controlli: {why}", domain.as_str())).await?;
    }
    Ok(())
}

/// Spazio di file e database; avviso oltre la soglia.
pub async fn usage<H: Host>(c: &Ctx<H>, id: i64, threshold_mb: u64) -> Result<()> {
    let site = c.store.site(id).await?;
    let files = c.host.site_disk_usage(&site.slug).await?;
    let db = c.db_creds(id).await?;
    let db_bytes: u64 = c.host.db_tables(&db.name).await?.iter().map(|t| t.bytes).sum();
    let record = json!({ "files_bytes": files, "db_bytes": db_bytes, "at": chrono::Utc::now().timestamp() });
    c.store.set_setting(&format!("usage:{id}"), &record.to_string()).await?;
    let total_mb = (files + db_bytes) / (1024 * 1024);
    if total_mb > threshold_mb {
        c.store.raise_alert(Some(id), "disk", &format!("il sito occupa {total_mb} MB (soglia {threshold_mb} MB)")).await?;
    } else {
        c.store.resolve_alerts(id, "disk").await?;
    }
    Ok(())
}

/// Statistiche dell'ultima ora dall'access log; avviso se gli errori 5xx sono troppi.
pub async fn record_traffic<H>(c: &Ctx<H>, id: i64, t: &Traffic) -> Result<()> {
    let record = json!({ "requests": t.requests, "errors_5xx": t.errors_5xx, "slow": t.slow, "at": chrono::Utc::now().timestamp() });
    c.store.set_setting(&format!("traffic:{id}"), &record.to_string()).await?;
    if t.errors_5xx >= ERRORS_PER_HOUR {
        c.store.raise_alert(Some(id), "errors", &format!("{} errori 5xx nell'ultima ora su {} richieste ({} lente)", t.errors_5xx, t.requests, t.slow)).await?;
    } else {
        c.store.resolve_alerts(id, "errors").await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flows::{create_site, CreateSite, JobLog};
    use crate::secrets::SecretKey;
    use crate::store::{SiteKind, Store};
    use crate::testing::FakeHost;
    use alveare_core::site::Domain;

    fn line(ts: i64, status: u16, secs: f64) -> String {
        format!("{{\"ts\":{ts}.0,\"request\":{{\"client_ip\":\"1.1.1.1\",\"method\":\"GET\",\"uri\":\"/\"}},\"status\":{status},\"duration\":{secs}}}")
    }

    #[test]
    fn traffic_counts_errors_and_slow_requests_in_the_window() {
        let lines = [line(100, 200, 0.1), line(200, 502, 0.2), line(300, 200, 6.0), line(400, 503, 7.5), "rotta".into()];
        assert_eq!(traffic(&lines, 150), Traffic { requests: 3, errors_5xx: 2, slow: 2 });
    }

    #[test]
    fn dir_size_does_not_follow_links() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("a/b")).unwrap();
        std::fs::write(t.path().join("a/b/x"), [0u8; 100]).unwrap();
        std::fs::write(t.path().join("a/y"), [0u8; 20]).unwrap();
        std::os::unix::fs::symlink("/usr", t.path().join("a/link")).unwrap();
        assert_eq!(dir_size(&t.path().join("a")), 120);
    }

    async fn site(host: FakeHost) -> (Ctx<FakeHost>, i64) {
        let c = Ctx {
            store: Store::open_memory().await.unwrap(),
            host,
            key: SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap(),
            db_host: "mariadb".into(),
            global_env: Default::default(),
            backups: false,
            site_url: crate::flows::DEFAULT_SITE_URL.into(),
        };
        let req = CreateSite { domain: Domain::parse("sona.it").unwrap(), kind: SiteKind::Wordpress, template: "wordpress".into(), title: "S".into(), admin_email: "a@b.it".into(), admin_user: "admin".into(), table_prefix: "wp_".into() };
        let id = create_site(&c, &req, &mut JobLog::default()).await.unwrap().id;
        (c, id)
    }

    #[tokio::test]
    async fn down_alert_after_two_failures_and_resolved_on_recovery() {
        let (c, id) = site(FakeHost { probe_status: Some(502), ..Default::default() }).await;
        uptime(&c, id).await.unwrap();
        assert!(c.store.open_alerts(None).await.unwrap().is_empty(), "un fallimento solo non basta");
        uptime(&c, id).await.unwrap();
        let alerts = c.store.open_alerts(None).await.unwrap();
        assert_eq!(alerts[0].kind, "down");
        assert!(alerts[0].message.contains("502"));
        let c = Ctx { host: FakeHost::default(), ..c };
        uptime(&c, id).await.unwrap();
        assert!(c.store.open_alerts(None).await.unwrap().is_empty());
        let rec: serde_json::Value = serde_json::from_str(&c.store.setting(&format!("uptime:{id}")).await.unwrap().unwrap()).unwrap();
        assert_eq!((rec["ok"].as_bool(), rec["status"].as_u64(), rec["fails"].as_u64()), (Some(true), Some(200), Some(0)));
    }

    #[tokio::test]
    async fn usage_over_threshold_alerts() {
        let (c, id) = site(FakeHost::default()).await;
        usage(&c, id, 0).await.unwrap();
        assert_eq!(c.store.open_alerts(None).await.unwrap()[0].kind, "disk");
        usage(&c, id, 10).await.unwrap();
        assert!(c.store.open_alerts(None).await.unwrap().is_empty());
        record_traffic(&c, id, &Traffic { requests: 100, errors_5xx: 25, slow: 3 }).await.unwrap();
        assert_eq!(c.store.open_alerts(None).await.unwrap()[0].kind, "errors");
    }
}
