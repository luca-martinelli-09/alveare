//! Operazioni lunghe: rispondono subito con un id, girano in background una alla volta
//! (rigenerano pool e Caddyfile, non devono sovrapporsi) e lasciano il log nel database.
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use sqlx::{Row, SqlitePool};

use crate::flows::JobLog;

type LiveLog = Arc<Mutex<Vec<String>>>;

#[derive(Clone)]
pub struct Jobs {
    pool: SqlitePool,
    live: Arc<Mutex<HashMap<i64, LiveLog>>>,
    serial: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobView {
    pub id: i64,
    pub kind: String,
    pub site_id: Option<i64>,
    pub status: String,
    pub log: Vec<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// Riga dell'attività recente: un job con l'ultima riga del suo log.
#[derive(Debug, serde::Serialize)]
pub struct Activity {
    pub id: i64,
    pub kind: String,
    pub site_id: Option<i64>,
    pub status: String,
    pub detail: String,
    pub started_at: String,
}

impl Jobs {
    /// Ultimi job, solo dei siti in `visible` se c'è (senza i job del pannello).
    pub async fn recent(&self, visible: Option<&[i64]>, limit: i64) -> sqlx::Result<Vec<Activity>> {
        let rows = sqlx::query("SELECT id, kind, site_id, status, log, started_at FROM jobs ORDER BY id DESC LIMIT ?").bind(limit * 4).fetch_all(&self.pool).await?;
        let mut out = Vec::new();
        for r in rows {
            let site_id: Option<i64> = r.try_get("site_id")?;
            if visible.is_some_and(|v| !site_id.is_some_and(|s| v.contains(&s))) {
                continue;
            }
            let log: String = r.try_get("log")?;
            out.push(Activity {
                id: r.try_get("id")?,
                kind: r.try_get("kind")?,
                site_id,
                status: r.try_get("status")?,
                detail: log.lines().rev().find(|l| !l.is_empty()).unwrap_or_default().to_string(),
                started_at: r.try_get("started_at")?,
            });
            if out.len() as i64 == limit {
                break;
            }
        }
        Ok(out)
    }

    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, live: Default::default(), serial: Default::default() }
    }

    pub async fn spawn<F, Fut>(&self, kind: &str, site_id: Option<i64>, user_id: Option<i64>, work: F) -> anyhow::Result<i64>
    where
        F: FnOnce(JobLog) -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<JobLog>> + Send + 'static,
    {
        let id: i64 = sqlx::query_scalar("INSERT INTO jobs (kind, site_id, user_id) VALUES (?, ?, ?) RETURNING id")
            .bind(kind)
            .bind(site_id)
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        let live: LiveLog = Default::default();
        self.live.lock().expect("lock dei job").insert(id, live.clone());
        let this = self.clone();
        tokio::spawn(async move {
            let _turn = this.serial.lock().await;
            let log = JobLog { job_id: id, lines: Vec::new(), live: Some(live.clone()) };
            let (status, lines) = match work(log).await {
                Ok(log) => ("done", log.lines),
                Err(e) => {
                    let mut lines = live.lock().expect("lock del log").clone();
                    lines.push(format!("errore: {e:#}"));
                    ("failed", lines)
                }
            };
            if let Err(e) = this.finish(id, status, &lines).await {
                tracing::error!("salvataggio del job {id}: {e}");
            }
            this.live.lock().expect("lock dei job").remove(&id);
        });
        Ok(id)
    }

    /// Il sito di un job di creazione si conosce solo a metà job.
    pub async fn set_site(&self, id: i64, site_id: i64) -> sqlx::Result<()> {
        sqlx::query("UPDATE jobs SET site_id = ? WHERE id = ?").bind(site_id).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    async fn finish(&self, id: i64, status: &str, lines: &[String]) -> sqlx::Result<()> {
        sqlx::query("UPDATE jobs SET status = ?, log = ?, finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?")
            .bind(status)
            .bind(lines.join("\n"))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get(&self, id: i64) -> sqlx::Result<JobView> {
        let r = sqlx::query("SELECT id, kind, site_id, status, log, started_at, finished_at FROM jobs WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let live = self.live.lock().expect("lock dei job").get(&id).map(|l| l.lock().expect("lock del log").clone());
        let stored: String = r.try_get("log")?;
        let mut status: String = r.try_get("status")?;
        let log = match live {
            Some(lines) => lines,
            None if status == "running" => {
                // Job rimasto a metà per un riavvio del pannello.
                status = "failed".into();
                vec!["interrotto: il pannello è stato riavviato".into()]
            }
            None => stored.lines().map(String::from).filter(|l| !l.is_empty()).collect(),
        };
        Ok(JobView {
            id: r.try_get("id")?,
            kind: r.try_get("kind")?,
            site_id: r.try_get("site_id")?,
            status,
            log,
            started_at: r.try_get("started_at")?,
            finished_at: r.try_get("finished_at")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::time::Duration;

    async fn jobs() -> Jobs {
        Jobs::new(Store::open_memory().await.unwrap().pool().clone())
    }

    async fn wait_done(j: &Jobs, id: i64) -> JobView {
        for _ in 0..200 {
            let v = j.get(id).await.unwrap();
            if v.status != "running" {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("job {id} non terminato");
    }

    #[tokio::test]
    async fn successful_job_is_persisted_with_its_log() {
        let j = jobs().await;
        let id = j.spawn("create_site", None, Some(1), |mut log| async move {
            log.push("passo 1");
            Ok(log)
        }).await.unwrap();
        let v = wait_done(&j, id).await;
        assert_eq!(v.status, "done");
        assert_eq!(v.log, ["passo 1"]);
        assert!(v.finished_at.is_some());
    }

    #[tokio::test]
    async fn failed_job_records_the_error() {
        let j = jobs().await;
        let id = j.spawn("delete_site", Some(3), None, |_log| async move { Err(anyhow::anyhow!("boom")) }).await.unwrap();
        let v = wait_done(&j, id).await;
        assert_eq!(v.status, "failed");
        assert_eq!(v.site_id, Some(3));
        assert!(v.log.last().unwrap().contains("boom"));
    }

    #[tokio::test]
    async fn running_job_log_is_visible_live() {
        let j = jobs().await;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let id = j.spawn("x", None, None, |mut log| async move {
            log.push("in corso");
            let _ = rx.await;
            Ok(log)
        }).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let v = j.get(id).await.unwrap();
        assert_eq!((v.status.as_str(), v.log.clone()), ("running", vec!["in corso".to_string()]));
        tx.send(()).unwrap();
        wait_done(&j, id).await;
    }

    #[tokio::test]
    async fn jobs_run_one_at_a_time() {
        let j = jobs().await;
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut ids = Vec::new();
        for n in 0..3 {
            let order = order.clone();
            ids.push(j.spawn("x", None, None, move |log| async move {
                order.lock().unwrap().push(format!("inizio {n}"));
                tokio::time::sleep(Duration::from_millis(20)).await;
                order.lock().unwrap().push(format!("fine {n}"));
                Ok(log)
            }).await.unwrap());
        }
        for id in ids {
            wait_done(&j, id).await;
        }
        let o = order.lock().unwrap().clone();
        for pair in o.chunks(2) {
            assert_eq!(pair[0].replace("inizio", "fine"), pair[1], "job sovrapposti: {o:?}");
        }
    }
}
