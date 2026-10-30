//! Metriche in formato Prometheus per Grafana: quelle di Caddy (dall'Admin API) più quelle del
//! pannello per sito, dai valori del monitoraggio. `/metrics` richiede ALVEARE_METRICS_TOKEN.
use std::fmt::Write;

use serde_json::Value;

/// Un sito con i suoi ultimi valori (impostazioni `uptime:`, `usage:`, `traffic:`, `updates:`).
pub struct SiteMetrics {
    pub domain: String,
    pub uptime: Value,
    pub usage: Value,
    pub traffic: Value,
    pub updates: Value,
    /// Timestamp dell'ultimo backup riuscito.
    pub last_backup: Option<i64>,
}

fn escape(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn gauge(out: &mut String, name: &str, help: &str, rows: &[(String, f64)]) {
    if rows.is_empty() {
        return;
    }
    writeln!(out, "# HELP {name} {help}\n# TYPE {name} gauge").unwrap();
    for (site, v) in rows {
        writeln!(out, "{name}{{site=\"{}\"}} {v}", escape(site)).unwrap();
    }
}

pub fn render(sites: &[SiteMetrics], open_alerts: usize) -> String {
    let mut out = String::new();
    let rows = |f: &dyn Fn(&SiteMetrics) -> Option<f64>| sites.iter().filter_map(|s| f(s).map(|v| (s.domain.clone(), v))).collect::<Vec<_>>();
    gauge(&mut out, "alveare_site_up", "1 se l'ultimo controllo ha risposto", &rows(&|s| s.uptime["ok"].as_bool().map(|b| f64::from(u8::from(b)))));
    gauge(&mut out, "alveare_site_response_seconds", "Tempo di risposta dell'ultimo controllo", &rows(&|s| s.uptime["ms"].as_f64().map(|ms| ms / 1000.0)));
    gauge(&mut out, "alveare_site_files_bytes", "Spazio dei file del sito", &rows(&|s| s.usage["files_bytes"].as_f64()));
    gauge(&mut out, "alveare_site_database_bytes", "Dimensione del database del sito", &rows(&|s| s.usage["db_bytes"].as_f64()));
    gauge(&mut out, "alveare_site_requests_hour", "Richieste nell'ultima ora", &rows(&|s| s.traffic["requests"].as_f64()));
    gauge(&mut out, "alveare_site_errors_5xx_hour", "Risposte 5xx nell'ultima ora", &rows(&|s| s.traffic["errors_5xx"].as_f64()));
    gauge(&mut out, "alveare_site_slow_requests_hour", "Richieste oltre 5 secondi nell'ultima ora", &rows(&|s| s.traffic["slow"].as_f64()));
    gauge(&mut out, "alveare_site_pending_updates", "Aggiornamenti WordPress in sospeso", &rows(&|s| s.updates["pending"].as_f64()));
    gauge(&mut out, "alveare_site_last_backup_timestamp", "Ultimo backup riuscito (secondi Unix)", &rows(&|s| s.last_backup.map(|t| t as f64)));
    writeln!(out, "# HELP alveare_open_alerts Avvisi aperti nel pannello\n# TYPE alveare_open_alerts gauge\nalveare_open_alerts {open_alerts}").unwrap();
    out
}

/// Confronto a tempo costante del token di Prometheus.
pub fn token_ok(expected: &str, header: Option<&str>) -> bool {
    let Some(given) = header.and_then(|h| h.strip_prefix("Bearer ")) else { return false };
    let (a, b) = (expected.as_bytes(), given.trim().as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn site_values_become_labeled_gauges() {
        let sites = [
            SiteMetrics { domain: "sona.it".into(), uptime: json!({ "ok": true, "ms": 120 }), usage: json!({ "files_bytes": 1000, "db_bytes": 50 }), traffic: json!({ "requests": 10, "errors_5xx": 1, "slow": 0 }), updates: json!({ "pending": 3 }), last_backup: Some(1_800_000_000) },
            SiteMetrics { domain: "x\"y.it".into(), uptime: json!({ "ok": false }), usage: Value::Null, traffic: Value::Null, updates: Value::Null, last_backup: None },
        ];
        let out = render(&sites, 2);
        assert!(out.contains("# TYPE alveare_site_up gauge\nalveare_site_up{site=\"sona.it\"} 1\nalveare_site_up{site=\"x\\\"y.it\"} 0\n"), "{out}");
        assert!(out.contains("alveare_site_response_seconds{site=\"sona.it\"} 0.12\n"));
        assert!(out.contains("alveare_site_last_backup_timestamp{site=\"sona.it\"} 1800000000\n"));
        assert!(!out.contains("alveare_site_files_bytes{site=\"x"), "niente valori inventati");
        assert!(out.ends_with("alveare_open_alerts 2\n"));
    }

    #[test]
    fn metrics_token_is_checked() {
        assert!(token_ok("segreto", Some("Bearer segreto")));
        assert!(!token_ok("segreto", Some("Bearer segreta")));
        assert!(!token_ok("segreto", Some("segreto")));
        assert!(!token_ok("segreto", None));
    }
}
