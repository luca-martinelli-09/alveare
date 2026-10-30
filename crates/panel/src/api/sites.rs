//! Stato del sito (sospensione, staging) e domini aggiuntivi.
use axum::routing::{delete, patch};

use super::*;
use crate::store::DomainKind;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}", patch(update_site::<H>))
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
