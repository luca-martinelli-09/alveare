//! Stato del sito (sospensione, staging) e domini aggiuntivi.
use axum::routing::{delete, patch};

use super::*;
use crate::store::DomainKind;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}", patch(update_site::<H>))
        .route("/api/sites/{id}/clone", post(clone_site::<H>))
        .route("/api/sites/{id}/export", post(export_site::<H>))
        .route("/api/sites/{id}/template", post(save_template::<H>))
        .route("/api/sites/{id}/exports", get(list_exports::<H>))
        .route("/api/exports/{token}", get(download_export::<H>))
        .route("/api/sites/{id}/domains", get(list_domains::<H>).post(add_domain::<H>))
        .route("/api/sites/{id}/domains/{hostname}", delete(remove_domain::<H>))
}

pub(super) fn staging_json(password: Option<String>) -> serde_json::Value {
    password.map_or(serde_json::Value::Null, |p| json!({ "user": "staging", "password": p }))
}

#[derive(Deserialize)]
struct UpdateSite {
    suspended: Option<bool>,
    staging: Option<bool>,
}

async fn update_site<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<UpdateSite>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    if let Some(suspended) = r.suspended {
        flows::set_suspended(&app.ctx, id, suspended).await.map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?;
        app.auth.audit(Some(user.id), if suspended { "site.suspend" } else { "site.resume" }, Some(id), "").await?;
    }
    if let Some(on) = r.staging {
        flows::set_staging(&app.ctx, id, on).await?;
        app.auth.audit(Some(user.id), if on { "site.staging.on" } else { "site.staging.off" }, Some(id), "").await?;
    }
    let site = app.ctx.store.site(id).await?;
    Ok(Json(json!({ "site": site, "staging": staging_json(flows::staging_password(&app.ctx, id).await?) })).into_response())
}

/// Dominio nuovo per un sito: valido, non riservato, non già usato.
pub(super) async fn free_domain<H: Host>(app: &App<H>, raw: &str) -> ApiResult<Domain> {
    let domain = Domain::parse(raw).map_err(|_| ApiError::bad_request("dominio non valido"))?;
    app.check_not_reserved(&domain)?;
    if app.ctx.store.hostname_taken(&domain).await? {
        return Err(StoreError::HostnameTaken(domain.as_str().into()).into());
    }
    Ok(domain)
}

#[derive(Deserialize)]
struct CloneReq {
    domain: String,
}

async fn clone_site<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<CloneReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let src = app.ctx.store.site(id).await?;
    let domain = free_domain(&app, &r.domain).await?;
    let (ctx, auth, jobs, uid) = (app.ctx.clone(), app.auth.clone(), app.jobs.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.clone", None, Some(user.id), move |mut log| async move {
            let site = flows::clone_site(&ctx, id, &domain, &mut log).await?;
            jobs.set_site(log.job_id, site.id).await?;
            auth.audit(Some(uid), "site.clone", Some(site.id), &format!("copia di {}", src.slug.as_str())).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

#[derive(Deserialize)]
struct TemplateReq {
    name: String,
    title: String,
}

async fn save_template<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<TemplateReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    app.ctx.store.site(id).await?;
    let name = Slug::parse(&r.name).map_err(|_| ApiError::bad_request("nome del template: solo minuscole, cifre e _"))?;
    if app.templates.join(name.as_str()).exists() {
        return Err(ApiError::new(StatusCode::CONFLICT, "esiste già un template con questo nome"));
    }
    let title = r.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::bad_request("titolo obbligatorio"));
    }
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.template", Some(id), Some(user.id), move |mut log| async move {
            flows::save_template(&ctx, id, &name, &title, now(), &mut log).await?;
            auth.audit(Some(uid), "template.create", Some(id), name.as_str()).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

const EXPORT_DAYS: i64 = 7;

async fn export_site<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let (ctx, auth, uid, dir) = (app.ctx.clone(), app.auth.clone(), user.id, app.paths.exports.clone());
    let job = app
        .jobs
        .spawn("site.export", Some(id), Some(user.id), move |mut log| async move {
            let export = flows::export_site(&ctx, id, &dir, now(), &mut log).await?;
            auth.audit(Some(uid), "site.export", Some(id), &export.file).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

async fn list_exports<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let cutoff = now() - EXPORT_DAYS * 86400;
    let out: Vec<_> = app
        .ctx
        .store
        .exports(id)
        .await?
        .into_iter()
        .filter(|e| e.created_at > cutoff)
        .map(|e| json!({ "token": e.token, "file": e.file, "size": e.size, "created_at": e.created_at, "expires_at": e.created_at + EXPORT_DAYS * 86400 }))
        .collect();
    Ok(Json(out).into_response())
}

/// Il link scade dopo 7 giorni e vale solo per chi gestisce il sito.
async fn download_export<H: Host>(State(app): State<App<H>>, Path(token): Path<String>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let export = app.ctx.store.export_by_token(&token).await?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "esportazione non trovata"))?;
    require_site(&app, &user, export.site_id, Need::Manage).await?;
    if export.created_at <= now() - EXPORT_DAYS * 86400 {
        return Err(ApiError::new(StatusCode::GONE, "esportazione scaduta"));
    }
    let file = tokio::fs::File::open(app.paths.exports.join(&export.file)).await.map_err(|e| ApiError::from(FileError::Io(e)))?;
    let len = file.metadata().await.map_err(internal)?.len();
    app.auth.audit(Some(user.id), "site.export.download", Some(export.site_id), &export.file).await?;
    let stream = tokio_util::io::ReaderStream::new(file);
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip".to_string()),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", export.file)),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}

async fn list_domains<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    let mut domains = app.ctx.store.domains(id).await?;
    // Ordine stabile per la UI: principale, alias, redirect.
    domains.sort_by_key(|(d, k)| (*k as u8, d.as_str().to_string()));
    let out: Vec<_> = domains.into_iter().map(|(d, k)| json!({ "hostname": d.as_str(), "kind": k })).collect();
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
struct NewDomain {
    hostname: String,
    kind: DomainKind,
}

async fn add_domain<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<NewDomain>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    app.ctx.store.site(id).await?;
    if r.kind == DomainKind::Primary {
        return Err(ApiError::bad_request("si aggiungono solo alias e redirect"));
    }
    let host = Domain::parse(&r.hostname).map_err(|_| ApiError::bad_request("hostname non valido"))?;
    app.check_not_reserved(&host)?;
    app.ctx.store.add_domain(id, &host, r.kind).await?;
    flows::sync_caddy(&app.ctx).await?;
    app.auth.audit(Some(user.id), "domain.add", Some(id), &format!("{} ({})", host.as_str(), r.kind.as_str())).await?;
    Ok((StatusCode::CREATED, Json(json!({}))).into_response())
}

async fn remove_domain<H: Host>(State(app): State<App<H>>, Path((id, hostname)): Path<(i64, String)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let host = Domain::parse(&hostname).map_err(|_| ApiError::bad_request("hostname non valido"))?;
    let primary = app.ctx.store.domains(id).await?.into_iter().any(|(d, k)| d == host && k == DomainKind::Primary);
    if primary {
        return Err(ApiError::bad_request("il dominio principale non si elimina"));
    }
    app.ctx.store.remove_domain(id, &host).await?;
    flows::sync_caddy(&app.ctx).await?;
    app.auth.audit(Some(user.id), "domain.remove", Some(id), host.as_str()).await?;
    Ok(Json(json!({})).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn manager_suspends_and_resumes_and_it_is_audited() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let (s, body, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&manager), Some(json!({"suspended": true})))).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert_eq!(body["site"]["status"], "suspended");
        let (s, _, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&manager), Some(json!({"suspended": true})))).await;
        assert_eq!(s, StatusCode::CONFLICT, "già sospeso");
        let (_, body, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&manager), Some(json!({"suspended": false})))).await;
        assert_eq!(body["site"]["status"], "active");
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{id}/audit"), Some(&admin), None)).await;
        let actions: Vec<&str> = audit.as_array().unwrap().iter().map(|a| a["action"].as_str().unwrap()).collect();
        assert!(actions.contains(&"site.suspend") && actions.contains(&"site.resume"), "{actions:?}");
    }

    #[tokio::test]
    async fn readonly_users_cannot_change_the_site() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let (s, _, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&reader), Some(json!({"suspended": true})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn staging_password_is_returned_and_visible_only_to_managers() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let (s, body, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&manager), Some(json!({"staging": true})))).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert!(body["site"]["staging"].as_bool().unwrap());
        let password = body["staging"]["password"].as_str().unwrap().to_string();
        assert_eq!(body["staging"]["user"], "staging");
        let (_, body, _) = call(&t, req("GET", &format!("/api/sites/{id}"), Some(&manager), None)).await;
        assert_eq!(body["staging"]["password"], password.as_str());
        let (_, body, _) = call(&t, req("GET", &format!("/api/sites/{id}"), Some(&reader), None)).await;
        assert!(body["staging"].is_null());
        let (_, body, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&manager), Some(json!({"staging": false})))).await;
        assert!(!body["site"]["staging"].as_bool().unwrap());
        assert!(body["staging"].is_null());
    }

    #[tokio::test]
    async fn admin_adds_and_removes_aliases_and_redirects() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let other = create_site(&t, &admin, "upupa.it").await;
        let url = format!("/api/sites/{id}/domains");
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"hostname": "www.sona.it", "kind": "redirect"})))).await;
        assert_eq!(s, StatusCode::CREATED);
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"hostname": "Comune-Sona.it", "kind": "alias"})))).await;
        assert_eq!(s, StatusCode::CREATED);
        let (_, body, _) = call(&t, req("GET", &url, Some(&admin), None)).await;
        assert_eq!(body, json!([
            {"hostname": "sona.it", "kind": "primary"},
            {"hostname": "comune-sona.it", "kind": "alias"},
            {"hostname": "www.sona.it", "kind": "redirect"},
        ]));
        let caddy = t.app.ctx.store.caddy_sites().await.unwrap();
        assert_eq!(caddy.iter().find(|c| c.slug.as_str() == "sona_it").unwrap().redirects.len(), 1);
        // Un hostname di un altro sito o non valido viene rifiutato; il primario non si elimina.
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{other}/domains"), Some(&admin), Some(json!({"hostname": "www.sona.it", "kind": "alias"})))).await;
        assert_eq!(s, StatusCode::CONFLICT);
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"hostname": "non valido", "kind": "alias"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"hostname": "x.sona.it", "kind": "primary"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _, _) = call(&t, req("DELETE", &format!("{url}/sona.it"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/sites/{other}/domains/www.sona.it"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "solo i domini di quel sito");
        let (s, _, _) = call(&t, req("DELETE", &format!("{url}/www.sona.it"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        let (_, body, _) = call(&t, req("GET", &url, Some(&admin), None)).await;
        assert_eq!(body.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn managers_read_domains_but_only_admins_change_them() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let url = format!("/api/sites/{id}/domains");
        let (s, _, _) = call(&t, req("GET", &url, Some(&manager), None)).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"hostname": "www.sona.it", "kind": "redirect"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_clones_a_site_as_a_job() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let body = json!({"domain": "prova.sona.it"});
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/clone"), Some(&manager), Some(body.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/clone"), Some(&admin), Some(json!({"domain": "sona.it"})))).await;
        assert_eq!(s, StatusCode::CONFLICT, "hostname già usato");
        let (s, body, _) = call(&t, req("POST", &format!("/api/sites/{id}/clone"), Some(&admin), Some(body))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        let copy = job.site_id.unwrap();
        let (_, detail, _) = call(&t, req("GET", &format!("/api/sites/{copy}"), Some(&admin), None)).await;
        assert_eq!(detail["site"]["staging"], true);
        assert_eq!(detail["domains"][0]["hostname"], "prova.sona.it");
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{copy}/audit"), Some(&admin), None)).await;
        assert_eq!(audit[0]["action"], "site.clone");
    }

    #[tokio::test]
    async fn manager_exports_and_downloads_until_expiry() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let other = create_site(&t, &admin, "upupa.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let stranger = user_with_role(&t, "x@example.org", other, Role::Manager).await;
        let (s, body, _) = call(&t, req("POST", &format!("/api/sites/{id}/export"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::ACCEPTED, "{body}");
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        // FakeHost non scrive l'archivio: lo crea il test dove l'host reale l'avrebbe messo.
        let (_, list, _) = call(&t, req("GET", &format!("/api/sites/{id}/exports"), Some(&manager), None)).await;
        let token = list[0]["token"].as_str().unwrap().to_string();
        assert!(list[0]["expires_at"].as_i64().unwrap() > list[0]["created_at"].as_i64().unwrap());
        std::fs::create_dir_all(&t.app.paths.exports).unwrap();
        std::fs::write(t.app.paths.exports.join(list[0]["file"].as_str().unwrap()), b"archivio").unwrap();
        let (s, _, h) = call(&t, req("GET", &format!("/api/exports/{token}"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert!(h["content-disposition"].to_str().unwrap().contains("sona_it.tar.gz"));
        let (s, _, _) = call(&t, req("GET", &format!("/api/exports/{token}"), Some(&stranger), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        sqlx::query("UPDATE exports SET created_at = created_at - 8 * 86400").execute(t.app.ctx.store.pool()).await.unwrap();
        let (s, _, _) = call(&t, req("GET", &format!("/api/exports/{token}"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::GONE);
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{id}/audit"), Some(&admin), None)).await;
        let actions: Vec<&str> = audit.as_array().unwrap().iter().map(|a| a["action"].as_str().unwrap()).collect();
        assert!(actions.contains(&"site.export") && actions.contains(&"site.export.download"), "{actions:?}");
    }

    #[tokio::test]
    async fn admin_saves_a_site_as_template() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let url = format!("/api/sites/{id}/template");
        let (s, _, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"name": "negozio", "title": "Negozio"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"name": "../x", "title": "x"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"name": "wordpress", "title": "x"})))).await;
        assert_eq!(s, StatusCode::CONFLICT, "nome già usato");
        let (s, body, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"name": "negozio", "title": "Negozio"})))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        assert!(t.app.ctx.host.calls().iter().any(|c| c.starts_with("store_template sona_it negozio ")));
    }

    #[tokio::test]
    async fn panel_hostnames_are_reserved() {
        let mut t = setup().await;
        t.app = t.app.clone().with_reserved_hosts(vec![alveare_core::site::Domain::parse("pannello.sona.it").unwrap()]);
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/domains"), Some(&admin), Some(json!({"hostname": "pannello.sona.it", "kind": "alias"})))).await;
        assert_eq!(s, StatusCode::CONFLICT);
        let (s, _, _) = call(&t, req("POST", "/api/sites", Some(&admin), Some(json!({"domain": "pannello.sona.it", "template": "wordpress", "title": "x"})))).await;
        assert_eq!(s, StatusCode::CONFLICT);
    }
}
