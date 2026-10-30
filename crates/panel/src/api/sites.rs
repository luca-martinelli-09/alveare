//! Stato del sito (sospensione, staging) e domini aggiuntivi.
use axum::routing::{delete, patch};

use super::*;
use crate::store::DomainKind;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}", patch(update_site::<H>))
        .route("/api/server", get(server::<H>))
        .route("/api/sites/{id}/monitor", get(monitor::<H>))
        .route("/api/sites/{id}/wp-admin", post(wp_admin::<H>))
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
    php: Option<crate::store::PhpSettingsRow>,
    /// Conferma esplicita quando il budget di memoria supera il 150% della RAM.
    #[serde(default)]
    confirm_budget: bool,
    suspended: Option<bool>,
    staging: Option<bool>,
    log_retention_days: Option<u32>,
    update_rule: Option<crate::store::UpdateRule>,
    /// Account Cloudflare del sito (solo admin); stringa vuota = il primo configurato.
    cloudflare_account: Option<String>,
}

async fn update_site<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<UpdateSite>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let mut warning = None;
    let mut budget = None;
    if let Some(php) = &r.php {
        crate::budget::validate(php).map_err(ApiError::bad_request)?;
        let b = flows::budget_with(&app.ctx, id, Some(php), app.ram_mb).await?;
        if b.needs_confirmation() && !r.confirm_budget {
            return Err(ApiError::new(StatusCode::CONFLICT, format!("i pool arriverebbero al {}% della RAM: conferma per applicare", b.percent)));
        }
        flows::set_php_settings(&app.ctx, id, php).await?;
        flows::budget_alert(&app.ctx, app.ram_mb).await?;
        if b.over_ram() {
            warning = Some(format!("Attenzione: il massimo teorico dei pool è il {}% della RAM ({} MB su {} MB).", b.percent, b.total_mb, b.ram_mb));
        }
        app.auth.audit(Some(user.id), "site.php", Some(id), &format!("memoria {} MB, upload {} MB, tempo {} s, processi {}", php.memory_limit_mb, php.upload_max_mb, php.max_execution_time_s, php.max_children)).await?;
        budget = Some(b);
    }
    if let Some(suspended) = r.suspended {
        flows::set_suspended(&app.ctx, id, suspended).await.map_err(|e| ApiError::new(StatusCode::CONFLICT, e.to_string()))?;
        app.auth.audit(Some(user.id), if suspended { "site.suspend" } else { "site.resume" }, Some(id), "").await?;
    }
    if let Some(on) = r.staging {
        flows::set_staging(&app.ctx, id, on).await?;
        app.auth.audit(Some(user.id), if on { "site.staging.on" } else { "site.staging.off" }, Some(id), "").await?;
    }
    if let Some(acc) = &r.cloudflare_account {
        require_admin(&user)?;
        let acc = Some(acc.trim()).filter(|a| !a.is_empty());
        if let Some(a) = acc {
            if !app.ctx.host.cloudflare_accounts().iter().any(|x| x == a) {
                return Err(ApiError::bad_request(format!("account Cloudflare sconosciuto: {a}")));
            }
        }
        app.ctx.store.set_cloudflare_account(id, acc).await?;
        app.auth.audit(Some(user.id), "site.cloudflare_account", Some(id), acc.unwrap_or("predefinito")).await?;
    }
    if let Some(rule) = r.update_rule {
        let site = app.ctx.store.site(id).await?;
        app.ctx.host.set_site_env(&site.slug, site.uid(), "ALVEARE_UPDATE_RULE", rule.as_str()).await?;
        app.ctx.store.set_update_rule(id, rule).await?;
        app.auth.audit(Some(user.id), "site.update_rule", Some(id), rule.as_str()).await?;
    }
    if let Some(days) = r.log_retention_days {
        if !(1..=365).contains(&days) {
            return Err(ApiError::bad_request("conservazione dei log tra 1 e 365 giorni"));
        }
        app.ctx.store.set_log_retention(id, days).await?;
        flows::sync_caddy(&app.ctx).await?;
        app.auth.audit(Some(user.id), "site.log_retention", Some(id), &format!("{days} giorni")).await?;
    }
    let site = app.ctx.store.site(id).await?;
    Ok(Json(json!({ "site": site, "staging": staging_json(flows::staging_password(&app.ctx, id).await?), "budget": budget, "warning": warning })).into_response())
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

/// Password admin di un WordPress appena creato: una lettura sola, solo admin, nell'audit.
async fn wp_admin<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let (wp_user, password) = flows::take_wp_admin(&app.ctx, id, now()).await?.ok_or_else(|| ApiError::new(StatusCode::GONE, "password già letta o scaduta: usa \"Password dimenticata\" di WordPress"))?;
    app.auth.audit(Some(user.id), "wordpress.admin_password", Some(id), "letta").await?;
    Ok(Json(json!({ "user": wp_user, "password": password })).into_response())
}

/// Ultimi valori del monitoraggio: raggiungibilità, spazio, traffico dell'ultima ora.
async fn monitor<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    let mut out = serde_json::Map::new();
    for key in ["uptime", "usage", "traffic"] {
        let v: serde_json::Value = app.ctx.store.setting(&format!("{key}:{id}")).await?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
        out.insert(key.into(), v);
    }
    out.insert("disk_alert_mb".into(), json!(app.disk_alert_mb));
    Ok(Json(out).into_response())
}

/// Pagina del server (admin): RAM e budget di memoria dei pool.
async fn server<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    let budget = flows::budget_with(&app.ctx, 0, None, app.ram_mb).await?;
    let mut sites = Vec::new();
    for s in app.ctx.store.sites().await? {
        let domain = app.ctx.store.domains(s.id).await?.into_iter().next().map(|(d, _)| d.as_str().to_string());
        sites.push(json!({ "id": s.id, "slug": s.slug, "domain": domain, "status": s.status, "pool_mb": crate::budget::pool_mb(&s.settings), "settings": s.settings }));
    }
    sites.sort_by_key(|s| std::cmp::Reverse(s["pool_mb"].as_u64()));
    Ok(Json(json!({ "budget": budget, "sites": sites, "version": crate::upgrade::VERSION })).into_response())
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
    let mut log = flows::JobLog::default();
    flows::tunnel_hostnames(&app.ctx, id, std::slice::from_ref(&host), true, &mut log).await?;
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
    let mut log = flows::JobLog::default();
    flows::tunnel_hostnames(&app.ctx, id, std::slice::from_ref(&host), false, &mut log).await?;
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
    async fn log_retention_is_set_per_site_within_bounds() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let (_, body, _) = call(&t, req("GET", &format!("/api/sites/{id}"), Some(&admin), None)).await;
        assert_eq!(body["site"]["log_retention_days"], 30);
        let (s, body, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&admin), Some(json!({"log_retention_days": 14})))).await;
        assert_eq!((s, body["site"]["log_retention_days"].as_u64()), (StatusCode::OK, Some(14)));
        assert_eq!(t.app.ctx.store.caddy_sites().await.unwrap()[0].log_days, 14);
        let (s, _, _) = call(&t, req("PATCH", &format!("/api/sites/{id}"), Some(&admin), Some(json!({"log_retention_days": 0})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn manager_changes_php_settings_within_the_memory_budget() {
        let mut t = setup().await;
        t.app.ram_mb = 4096;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let url = format!("/api/sites/{id}");
        let php = |mem: u32, children: u32| json!({"php": {"memory_limit_mb": mem, "upload_max_mb": 128, "max_execution_time_s": 120, "max_children": children}});
        let (s, body, _) = call(&t, req("PATCH", &url, Some(&manager), Some(php(512, 4)))).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert_eq!(body["site"]["settings"]["upload_max_mb"], 128);
        assert_eq!(body["budget"]["total_mb"], 512 * 4 + crate::budget::RESERVED_MB);
        assert!(body["warning"].is_null());
        // Oltre la RAM: applicato, ma con avviso.
        let (s, body, _) = call(&t, req("PATCH", &url, Some(&manager), Some(php(1024, 4)))).await;
        assert_eq!(s, StatusCode::OK);
        assert!(body["warning"].as_str().unwrap().contains("RAM"), "{body}");
        // Oltre il 150%: serve la conferma esplicita.
        let (s, body, _) = call(&t, req("PATCH", &url, Some(&manager), Some(php(2048, 4)))).await;
        assert_eq!(s, StatusCode::CONFLICT, "{body}");
        assert_eq!(t.app.ctx.store.site(id).await.unwrap().settings.memory_limit_mb, 1024, "non applicato");
        let mut confirmed = php(2048, 4);
        confirmed["confirm_budget"] = json!(true);
        let (s, _, _) = call(&t, req("PATCH", &url, Some(&manager), Some(confirmed))).await;
        assert_eq!(s, StatusCode::OK);
        let alerts = t.app.ctx.store.open_alerts(None).await.unwrap();
        assert!(alerts.iter().any(|a| a.kind == "budget" && a.site_id.is_none()), "{alerts:?}");
        let (s, _, _) = call(&t, req("PATCH", &url, Some(&manager), Some(php(16, 4)))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, body, _) = call(&t, req("GET", "/api/server", Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body["budget"]["ram_mb"], 4096);
        assert_eq!(body["sites"][0]["pool_mb"], 2048 * 4);
        let (s, _, _) = call(&t, req("GET", "/api/server", Some(&manager), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        // Un sito nuovo con i pool già al 200%: serve la conferma.
        let (s, body, _) = call(&t, req("POST", "/api/sites", Some(&admin), Some(json!({"domain": "nuovo.it", "template": "wordpress", "title": "x"})))).await;
        assert_eq!(s, StatusCode::CONFLICT, "{body}");
        let (s, _, _) = call(&t, req("POST", "/api/sites", Some(&admin), Some(json!({"domain": "nuovo.it", "template": "wordpress", "title": "x", "confirm_budget": true})))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn monitor_values_are_readable_by_readers() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        crate::monitor::uptime(&t.app.ctx, id).await.unwrap();
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/monitor"), Some(&reader), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body["uptime"]["status"], 200);
        assert!(body["usage"].is_null());
    }

    #[tokio::test]
    async fn metrics_need_their_token_and_include_caddy() {
        let mut t = setup().await;
        let (s, _, _) = call(&t, req("GET", "/metrics", None, None)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "spente senza token");
        t.app = t.app.clone().with_metrics_token(Some("prom".into()));
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        crate::monitor::uptime(&t.app.ctx, id).await.unwrap();
        let (s, _, _) = call(&t, req("GET", "/metrics", Some(&admin), None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "il token di un utente non basta");
        let resp = tower::ServiceExt::oneshot(crate::api::router(t.app.clone()), req("GET", "/metrics", Some("prom"), None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes().to_vec()).unwrap();
        assert!(body.contains("alveare_site_up{site=\"sona.it\"} 1") && body.contains("caddy_http_requests_total"), "{body}");
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
    async fn snapshot_browsing_and_partial_or_new_domain_restores() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let job = call(&t, req("POST", &format!("/api/sites/{id}/backups"), Some(&admin), None)).await.1["job"].as_i64().unwrap();
        wait_job(&t, job).await;
        let base = format!("/api/sites/{id}/backups/abcdef12");
        let (s, body, _) = call(&t, req("GET", &format!("{base}/files?path=public"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body[0]["name"], "wp-content");
        let (s, _, _) = call(&t, req("GET", &format!("{base}/files?path=../x"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, body, _) = call(&t, req("POST", &format!("{base}/restore"), Some(&manager), Some(json!({"mode": "path", "path": "public/wp-content/themes/x"})))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        assert_eq!(wait_job(&t, body["job"].as_i64().unwrap()).await.status, "done");
        let (s, _, _) = call(&t, req("POST", &format!("{base}/restore"), Some(&manager), Some(json!({"mode": "new_domain", "domain": "verifica.sona.it"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "un sito nuovo lo crea solo l'admin");
        let (s, body, _) = call(&t, req("POST", &format!("{base}/restore"), Some(&admin), Some(json!({"mode": "new_domain", "domain": "verifica.sona.it"})))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        assert_ne!(job.site_id, Some(id));
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
