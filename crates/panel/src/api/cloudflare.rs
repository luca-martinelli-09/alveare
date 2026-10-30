//! Cloudflare (solo admin): account configurati, modello delle regole, sincronizzazione degli
//! hostname di un sito e applicazione delle regole alle sue zone.
use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/cloudflare", get(overview::<H>))
        .route("/api/cloudflare/rules", put(set_rules::<H>))
        .route("/api/sites/{id}/cloudflare/sync", post(sync::<H>))
        .route("/api/sites/{id}/cloudflare/rules", post(apply_rules::<H>))
}

const RULES_KEY: &str = "cloudflare_rules";

pub(crate) async fn rules_template<H>(app: &App<H>) -> ApiResult<serde_json::Value> {
    Ok(app.ctx.store.setting(RULES_KEY).await?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_else(crate::cloudflare::default_rules))
}

async fn overview<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    Ok(Json(json!({ "accounts": app.ctx.host.cloudflare_accounts(), "rules": rules_template(&app).await?, "service": crate::cloudflare::SERVICE })).into_response())
}

#[derive(Deserialize)]
struct RulesReq {
    rules: serde_json::Value,
}

async fn set_rules<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<RulesReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    crate::cloudflare::validate_rules(&r.rules).map_err(|e| ApiError::bad_request(e.to_string()))?;
    app.ctx.store.set_setting(RULES_KEY, &r.rules.to_string()).await?;
    app.auth.audit(Some(user.id), "cloudflare.rules", None, "modello aggiornato").await?;
    Ok(Json(json!({})).into_response())
}

async fn sync<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    app.ctx.store.site(id).await?;
    let hosts = flows::site_hostnames(&app.ctx, id).await?;
    let mut log = flows::JobLog::default();
    flows::tunnel_hostnames(&app.ctx, id, &hosts, true, &mut log).await?;
    app.auth.audit(Some(user.id), "cloudflare.sync", Some(id), &log.lines.join("; ")).await?;
    Ok(Json(json!({ "log": log.lines })).into_response())
}

/// Una volta per zona: alias su domini diversi hanno zone diverse.
async fn apply_rules<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    app.ctx.store.site(id).await?;
    let account = flows::cloudflare_account(&app.ctx, id).await?.ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "nessun account Cloudflare configurato"))?;
    let template = rules_template(&app).await?;
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for host in flows::site_hostnames(&app.ctx, id).await? {
        let apex = crate::cloudflare::zone_candidates(&host).pop().unwrap_or_default();
        if seen.contains(&apex) {
            continue;
        }
        seen.push(apex.clone());
        match app.ctx.host.cloudflare_rules(&account, &host, &template).await {
            Ok(n) => out.push(json!({ "zone": apex, "rules": n })),
            Err(e) => out.push(json!({ "zone": apex, "error": format!("{e:#}") })),
        }
    }
    app.auth.audit(Some(user.id), "cloudflare.apply_rules", Some(id), &serde_json::to_string(&out).unwrap_or_default()).await?;
    Ok(Json(out).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn rules_template_sync_and_apply_per_zone() {
        let mut t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let (_, plain) = token_for(&t, "u@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", "/api/cloudflare", Some(&plain), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let id = create_site(&t, &admin, "sona.it").await;
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/cloudflare/rules"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::CONFLICT, "senza account");
        std::sync::Arc::get_mut(&mut t.app.ctx).unwrap().host.cf_accounts = vec!["banda".into()];
        call(&t, req("POST", &format!("/api/sites/{id}/domains"), Some(&admin), Some(json!({"hostname": "www.sona.it", "kind": "redirect"})))).await;
        call(&t, req("POST", &format!("/api/sites/{id}/domains"), Some(&admin), Some(json!({"hostname": "comune-sona.it", "kind": "alias"})))).await;
        assert!(t.app.ctx.host.calls().contains(&"tunnel_add banda www.sona.it".to_string()));
        let (s, body, _) = call(&t, req("POST", &format!("/api/sites/{id}/cloudflare/rules"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 2, "sona.it e comune-sona.it, una volta ciascuna: {body}");
        let (_, body, _) = call(&t, req("POST", &format!("/api/sites/{id}/cloudflare/sync"), Some(&admin), None)).await;
        assert_eq!(body["log"].as_array().unwrap().len(), 3);
        let (s, _, _) = call(&t, req("PUT", "/api/cloudflare/rules", Some(&admin), Some(json!({"rules": {"http_ratelimit": [{"description": "mia", "expression": "x", "action": "block"}]}})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let mine = json!({"http_request_firewall_custom": [{"description": "alveare: solo xmlrpc", "expression": "(http.request.uri.path eq \"/xmlrpc.php\")", "action": "block"}]});
        let (s, _, _) = call(&t, req("PUT", "/api/cloudflare/rules", Some(&admin), Some(json!({"rules": mine.clone()})))).await;
        assert_eq!(s, StatusCode::OK);
        let (_, body, _) = call(&t, req("GET", "/api/cloudflare", Some(&admin), None)).await;
        assert_eq!(body["rules"], mine);
        assert_eq!(body["accounts"], json!(["banda"]));
    }
}
