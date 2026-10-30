//! Override di PHP e Caddy, per sito e globali: solo admin, con storico e ripristino di una versione.
use super::*;
use crate::store::OverrideKind;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}/config/{kind}", get(get_site_config::<H>).put(put_site_config::<H>))
        .route("/api/sites/{id}/config/{kind}/history", get(site_history::<H>))
        .route("/api/config/{kind}", get(get_global::<H>).put(put_global::<H>))
        .route("/api/config/{kind}/history", get(global_history::<H>))
}

#[derive(Deserialize)]
struct ConfigReq {
    /// Nuovo contenuto, oppure la versione da ripristinare.
    content: Option<String>,
    restore: Option<i64>,
}

async fn wanted_content<H>(app: &App<H>, site: Option<i64>, kind: OverrideKind, r: ConfigReq) -> ApiResult<(String, String)> {
    match (r.content, r.restore) {
        (Some(c), None) => Ok((c, "salvataggio".into())),
        (None, Some(v)) => {
            let old = app.ctx.store.override_history(site, kind).await?.into_iter().find(|o| o.version == v);
            let old = old.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "versione non trovata"))?;
            Ok((old.content, format!("ripristino della versione {v}")))
        }
        _ => Err(ApiError::bad_request("serve content oppure restore")),
    }
}

async fn apply<H: Host>(app: &App<H>, site: Option<i64>, kind: OverrideKind, content: &str, author: i64) -> ApiResult<crate::store::Override> {
    if content.len() > 64 * 1024 {
        return Err(ApiError::bad_request("override troppo lungo (max 64 KB)"));
    }
    let saved = match (site, kind) {
        (Some(id), OverrideKind::Php) => flows::set_php_override(&app.ctx, id, content, Some(author)).await?,
        (None, OverrideKind::Php) => flows::set_php_global(&app.ctx, content, Some(author)).await?,
        (_, OverrideKind::Caddy) => flows::set_caddy_override(&app.ctx, site, content, Some(author)).await?,
    };
    Ok(saved)
}

fn current_json(o: Option<crate::store::Override>) -> serde_json::Value {
    match o {
        Some(o) => json!({ "content": o.content, "version": o.version, "author_id": o.author_id, "created_at": o.created_at }),
        None => json!({ "content": "", "version": 0 }),
    }
}

async fn get_site_config<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, OverrideKind)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    app.ctx.store.site(id).await?;
    Ok(Json(current_json(app.ctx.store.current_override(Some(id), kind).await?)).into_response())
}

async fn put_site_config<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, OverrideKind)>, m: Method, h: HeaderMap, Json(r): Json<ConfigReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    app.ctx.store.site(id).await?;
    let (content, what) = wanted_content(&app, Some(id), kind, r).await?;
    let saved = apply(&app, Some(id), kind, &content, user.id).await?;
    app.auth.audit(Some(user.id), &format!("config.{}", kind.as_str()), Some(id), &format!("{what}: versione {}", saved.version)).await?;
    Ok(Json(current_json(Some(saved))).into_response())
}

async fn site_history<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, OverrideKind)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    Ok(Json(app.ctx.store.override_history(Some(id), kind).await?).into_response())
}

async fn get_global<H: Host>(State(app): State<App<H>>, Path(kind): Path<OverrideKind>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    Ok(Json(current_json(app.ctx.store.current_override(None, kind).await?)).into_response())
}

async fn put_global<H: Host>(State(app): State<App<H>>, Path(kind): Path<OverrideKind>, m: Method, h: HeaderMap, Json(r): Json<ConfigReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let (content, what) = wanted_content(&app, None, kind, r).await?;
    let saved = apply(&app, None, kind, &content, user.id).await?;
    app.auth.audit(Some(user.id), &format!("config.global.{}", kind.as_str()), None, &format!("{what}: versione {}", saved.version)).await?;
    Ok(Json(current_json(Some(saved))).into_response())
}

async fn global_history<H: Host>(State(app): State<App<H>>, Path(kind): Path<OverrideKind>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    Ok(Json(app.ctx.store.override_history(None, kind).await?).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn admin_saves_restores_and_sees_history_of_site_overrides() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let url = format!("/api/sites/{id}/config/php");
        let (s, _, _) = call(&t, req("PUT", &url, Some(&manager), Some(json!({"content": "php_value[a] = 1"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, body, _) = call(&t, req("PUT", &url, Some(&admin), Some(json!({"content": "php_value[a] = 1"})))).await;
        assert_eq!((s, body["version"].as_i64()), (StatusCode::OK, Some(1)), "{body}");
        call(&t, req("PUT", &url, Some(&admin), Some(json!({"content": "php_value[a] = 2"})))).await;
        let (s, body, _) = call(&t, req("PUT", &url, Some(&admin), Some(json!({"content": "php_admin_value[open_basedir] = /"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("open_basedir"));
        let (_, body, _) = call(&t, req("PUT", &url, Some(&admin), Some(json!({"restore": 1})))).await;
        assert_eq!((body["version"].as_i64(), body["content"].as_str()), (Some(3), Some("php_value[a] = 1")));
        let (_, history, _) = call(&t, req("GET", &format!("{url}/history"), Some(&admin), None)).await;
        assert_eq!(history.as_array().unwrap().len(), 3);
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{id}/audit"), Some(&admin), None)).await;
        assert!(audit[0]["details"].as_str().unwrap().contains("ripristino della versione 1"), "{audit}");
    }

    #[tokio::test]
    async fn global_overrides_are_admin_only() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let (_, user) = token_for(&t, "u@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", "/api/config/caddy", Some(&user), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, body, _) = call(&t, req("PUT", "/api/config/php", Some(&admin), Some(json!({"content": "max_input_vars = 3000"})))).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        let (_, body, _) = call(&t, req("GET", "/api/config/php", Some(&admin), None)).await;
        assert_eq!(body["content"], "max_input_vars = 3000");
        let (s, _, _) = call(&t, req("PUT", "/api/config/xml", Some(&admin), Some(json!({"content": "x"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}
