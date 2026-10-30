//! Cloudflare (solo admin): account configurati e modello delle regole. Hostname e regole
//! dei siti li pubblicano i flussi da soli (`flows::tunnel_hostnames`).
use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/cloudflare", get(overview::<H>))
        .route("/api/cloudflare/rules", put(set_rules::<H>))
}

async fn rules_template<H>(app: &App<H>) -> ApiResult<serde_json::Value> {
    Ok(flows::cloudflare_rules_template(&app.ctx).await?)
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
    app.ctx.store.set_setting(flows::CLOUDFLARE_RULES_KEY, &r.rules.to_string()).await?;
    app.auth.audit(Some(user.id), "cloudflare.rules", None, "modello aggiornato").await?;
    Ok(Json(json!({})).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn rules_template_is_admin_only_and_validated() {
        let mut t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let (_, plain) = token_for(&t, "u@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", "/api/cloudflare", Some(&plain), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        std::sync::Arc::get_mut(&mut t.app.ctx).unwrap().host.cf_accounts = vec!["banda".into()];
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
