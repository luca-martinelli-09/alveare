//! Cron personalizzati di un sito: elenco, creazione, pausa, esecuzione immediata, storico.
use axum::routing::patch;

use super::*;
use crate::crons::{parse_expression, prepare, CronKind};

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}/cron", get(list::<H>).post(create::<H>))
        .route("/api/sites/{id}/cron/{cid}", patch(update::<H>).delete(remove::<H>))
        .route("/api/sites/{id}/cron/{cid}/run", post(run_now::<H>))
        .route("/api/sites/{id}/cron/{cid}/runs", get(runs::<H>))
}

/// Il cron deve appartenere al sito dell'URL: i permessi si controllano sul sito.
async fn site_cron<H>(app: &App<H>, id: i64, cid: i64) -> ApiResult<crate::store::CronJob> {
    let cron = app.ctx.store.cron(cid).await?;
    if cron.site_id != id {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "cron non trovato"));
    }
    Ok(cron)
}

async fn list<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    // Comandi e output possono contenere segreti: solo per chi gestisce.
    require_site(&app, &user, id, Need::Manage).await?;
    let mut out = Vec::new();
    for c in app.ctx.store.crons(Some(id)).await? {
        let last = app.ctx.store.cron_runs(c.id, 1).await?.into_iter().next();
        out.push(json!({ "cron": c, "last": last }));
    }
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
struct NewCron {
    expression: String,
    kind: CronKind,
    command: String,
    #[serde(default = "default_timeout")]
    timeout_s: u32,
}

fn default_timeout() -> u32 {
    600
}

async fn create<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<NewCron>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    parse_expression(&r.expression).map_err(ApiError::bad_request)?;
    let max = if r.kind == CronKind::Wp { 600 } else { 3600 };
    if !(1..=max).contains(&r.timeout_s) {
        return Err(ApiError::bad_request(format!("timeout tra 1 e {max} secondi")));
    }
    let domains: Vec<Domain> = app.ctx.store.domains(id).await?.into_iter().map(|(d, _)| d).collect();
    prepare(r.kind, &r.command, &domains).map_err(ApiError::bad_request)?;
    let cid = app.ctx.store.add_cron(id, r.expression.trim(), r.kind.as_str(), r.command.trim(), r.timeout_s).await?;
    app.auth.audit(Some(user.id), "cron.create", Some(id), &format!("{} {}: {}", r.expression.trim(), r.kind.as_str(), r.command.trim())).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": cid }))).into_response())
}

#[derive(Deserialize)]
struct CronUpdate {
    active: bool,
}

async fn update<H: Host>(State(app): State<App<H>>, Path((id, cid)): Path<(i64, i64)>, m: Method, h: HeaderMap, Json(r): Json<CronUpdate>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let cron = site_cron(&app, id, cid).await?;
    app.ctx.store.set_cron_active(cid, r.active).await?;
    if !r.active {
        app.ctx.store.resolve_alerts(id, &format!("cron-{cid}")).await?;
    }
    app.auth.audit(Some(user.id), if r.active { "cron.resume" } else { "cron.pause" }, Some(id), &cron.command).await?;
    Ok(Json(json!({})).into_response())
}

async fn remove<H: Host>(State(app): State<App<H>>, Path((id, cid)): Path<(i64, i64)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let cron = site_cron(&app, id, cid).await?;
    app.ctx.store.delete_cron(cid).await?;
    app.ctx.store.resolve_alerts(id, &format!("cron-{cid}")).await?;
    app.auth.audit(Some(user.id), "cron.delete", Some(id), &cron.command).await?;
    Ok(Json(json!({})).into_response())
}

async fn run_now<H: Host>(State(app): State<App<H>>, Path((id, cid)): Path<(i64, i64)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let cron = site_cron(&app, id, cid).await?;
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("cron.run", Some(id), Some(user.id), move |mut log| async move {
            let ok = flows::run_cron(&ctx, cid).await?;
            let last = ctx.store.cron_runs(cid, 1).await?.into_iter().next();
            log.push(last.map(|r| r.output).unwrap_or_default());
            auth.audit(Some(uid), "cron.run", Some(id), &cron.command).await?;
            if !ok {
                anyhow::bail!("il comando è fallito");
            }
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

async fn runs<H: Host>(State(app): State<App<H>>, Path((id, cid)): Path<(i64, i64)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    site_cron(&app, id, cid).await?;
    Ok(Json(app.ctx.store.cron_runs(cid, 50).await?).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn managers_create_pause_and_run_crons_of_their_sites() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let other = create_site(&t, &admin, "upupa.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let url = format!("/api/sites/{id}/cron");
        let good = json!({"expression": "*/10 * * * *", "kind": "wp", "command": "cache flush"});
        let (s, _, _) = call(&t, req("POST", &url, Some(&reader), Some(good.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        for bad in [
            json!({"expression": "ogni tanto", "kind": "wp", "command": "cache flush"}),
            json!({"expression": "* * * * *", "kind": "http", "command": "https://upupa.it/"}),
            json!({"expression": "* * * * *", "kind": "php", "command": "/etc/passwd"}),
            json!({"expression": "* * * * *", "kind": "wp", "command": "cache flush", "timeout_s": 3600}),
        ] {
            let (s, _, _) = call(&t, req("POST", &url, Some(&manager), Some(bad.clone()))).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        }
        let (s, body, _) = call(&t, req("POST", &url, Some(&manager), Some(good))).await;
        assert_eq!(s, StatusCode::CREATED);
        let cid = body["id"].as_i64().unwrap();
        let (s, body, _) = call(&t, req("POST", &format!("{url}/{cid}/run"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        assert_eq!(wait_job(&t, body["job"].as_i64().unwrap()).await.status, "done");
        let (_, list, _) = call(&t, req("GET", &url, Some(&manager), None)).await;
        assert_eq!(list[0]["last"]["ok"], true);
        let (s, _, _) = call(&t, req("GET", &url, Some(&reader), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "comandi e output non sono per chi legge soltanto");
        let (s, _, _) = call(&t, req("PATCH", &format!("{url}/{cid}"), Some(&manager), Some(json!({"active": false})))).await;
        assert_eq!(s, StatusCode::OK);
        assert!(t.app.ctx.store.crons(None).await.unwrap().is_empty());
        // Un cron di un sito non si raggiunge passando da un altro sito.
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/sites/{other}/cron/{cid}"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        let (s, _, _) = call(&t, req("GET", &format!("{url}/{cid}/runs"), Some(&reader), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (_, runs, _) = call(&t, req("GET", &format!("{url}/{cid}/runs"), Some(&manager), None)).await;
        assert_eq!(runs.as_array().unwrap().len(), 1);
    }
}
