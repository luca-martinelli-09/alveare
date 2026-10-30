//! Vista aggiornamenti di WordPress: versioni, aggiornamento (job con backup prima), riepilogo.
use super::*;
use crate::wordpress::{self as wp, UpdateReq};

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}/wordpress", get(status::<H>))
        .route("/api/sites/{id}/wordpress/update", post(update::<H>))
        .route("/api/updates", get(summary::<H>))
        .route("/api/sites/{id}/wp", post(run_wp::<H>))
}

#[derive(Deserialize)]
struct WpReq {
    args: Vec<String>,
}

/// WP-CLI con l'utente del sito (per la CLI `alveare wp`): job con l'output nel log, nell'audit.
/// I flag che escono dal sito (--path, --ssh, --exec…) li rifiuta l'agente.
async fn run_wp<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<WpReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let site = app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    if r.args.is_empty() || r.args.len() > 100 || r.args.iter().any(|a| a.len() > 4096 || a.contains('\0')) {
        return Err(ApiError::bad_request("argomenti di WP-CLI non validi"));
    }
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("wp", Some(id), Some(user.id), move |mut log| async move {
            auth.audit(Some(uid), "wp", Some(id), &r.args.join(" ")).await?;
            let (ok, out) = ctx.host.wp_output(&site.slug, &r.args).await?;
            for line in out.lines() {
                log.push(line);
            }
            if !ok {
                anyhow::bail!("wp {} è terminato con errore", r.args.first().map_or("", String::as_str));
            }
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

async fn status<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    let site = app.ctx.store.site(id).await?;
    if site.kind != crate::store::SiteKind::Wordpress {
        return Err(ApiError::bad_request("il sito non è WordPress"));
    }
    let st = wp::status(&app.ctx, &site.slug).await?;
    remember(&app, id, &st).await?;
    Ok(Json(json!({ "status": st, "pending": st.pending(), "update_rule": site.update_rule })).into_response())
}

/// Il conteggio degli aggiornamenti in sospeso resta per il riepilogo di tutti i siti.
pub(crate) async fn remember<H>(app: &App<H>, id: i64, st: &wp::Status) -> ApiResult<()> {
    let v = json!({ "pending": st.pending(), "core": st.core, "checked_at": now() });
    app.ctx.store.set_setting(&format!("updates:{id}"), &v.to_string()).await?;
    Ok(())
}

async fn update<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<UpdateReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("wordpress.update", Some(id), Some(user.id), move |mut log| async move {
            for change in wp::update(&ctx, id, &r, &mut log).await? {
                auth.audit(Some(uid), "wordpress.update", Some(id), &change).await?;
            }
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

/// Aggiornamenti in sospeso per sito, dall'ultimo controllo (notturno o dalla vista).
async fn summary<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let visible = app.auth.visible_sites(&user).await?;
    let mut out = Vec::new();
    for site in app.ctx.store.sites().await? {
        if visible.as_ref().is_none_or(|v| v.contains(&site.id)) {
            let cached: Option<serde_json::Value> = app.ctx.store.setting(&format!("updates:{}", site.id)).await?.and_then(|v| serde_json::from_str(&v).ok());
            if let Some(c) = cached {
                out.push(json!({ "site_id": site.id, "update_rule": site.update_rule, "pending": c["pending"], "core": c["core"], "checked_at": c["checked_at"] }));
            }
        }
    }
    Ok(Json(out).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn readers_see_versions_managers_update_and_changes_are_audited() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/wordpress"), Some(&reader), None)).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert_eq!(body["update_rule"], "security");
        let (_, sum, _) = call(&t, req("GET", "/api/updates", Some(&reader), None)).await;
        assert_eq!(sum[0]["site_id"], id);
        let body = json!({"plugins": {"names": ["akismet"]}});
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/wordpress/update"), Some(&reader), Some(body.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, out, _) = call(&t, req("POST", &format!("/api/sites/{id}/wordpress/update"), Some(&manager), Some(body))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = wait_job(&t, out["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        assert!(t.app.ctx.host.calls().contains(&"wp sona_it plugin update akismet".to_string()));
    }

    #[tokio::test]
    async fn wp_cli_runs_as_a_job_for_managers_only() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let body = json!({"args": ["option", "get", "blogname"]});
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/wp"), Some(&reader), Some(body.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, out, _) = call(&t, req("POST", &format!("/api/sites/{id}/wp"), Some(&manager), Some(body))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = out["job"].as_i64().unwrap();
        assert_eq!(wait_job(&t, job).await.status, "done");
        let (s, _, _) = call(&t, req("GET", &format!("/api/jobs/{job}"), Some(&reader), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "l'output di WP-CLI può contenere segreti");
        let (s, _, _) = call(&t, req("GET", &format!("/api/jobs/{job}"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}/audit"), Some(&reader), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert!(t.app.ctx.host.calls().contains(&"wp sona_it option get blogname".to_string()));
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/wp"), Some(&manager), Some(json!({"args": []})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_rule_is_saved_and_written_to_env() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let (s, body, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&admin), Some(json!({"update_rule": "auto"})))).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert_eq!(body["site"]["update_rule"], "auto");
        assert_eq!(t.app.ctx.host.env.lock().unwrap()["SITE_ENV ALVEARE_UPDATE_RULE"], "auto");
    }
}
