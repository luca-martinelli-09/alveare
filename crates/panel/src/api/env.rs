//! Il .env di un sito, modificabile a mano (solo admin: contiene anche le credenziali SMTP del server).
//! Database e Redis restano del pannello: li usa anche lui, cambiarli qui li disallineerebbe.
use super::*;
use crate::template::{env_file, parse_env_strict};

/// Valori che il pannello usa per parlare con il sito: si cambiano con le funzioni apposite.
const PROTECTED: [&str; 7] = ["DB_NAME", "DB_USER", "DB_PASSWORD", "DB_HOST", "REDIS_USERNAME", "REDIS_PASSWORD", "REDIS_PREFIX"];

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new().route("/api/sites/{id}/env", get(show::<H>).put(save::<H>))
}

async fn show<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    let site = app.ctx.store.site(id).await?;
    let content = env_file(&app.ctx.host.read_site_env(&site.slug).await?)?;
    Ok(Json(json!({ "content": content, "protected": PROTECTED })).into_response())
}

#[derive(Deserialize)]
struct EnvReq {
    content: String,
}

async fn save<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<EnvReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let site = app.ctx.store.site(id).await?;
    let new = parse_env_strict(&r.content).map_err(ApiError::bad_request)?;
    let current = app.ctx.host.read_site_env(&site.slug).await?;
    if let Some(k) = PROTECTED.iter().find(|k| new.get(**k) != current.get(**k)) {
        return Err(ApiError::bad_request(format!("{k} la usa anche il pannello e non si cambia qui (la password del database: Database → Cambia password)")));
    }
    app.ctx.host.write_site_env(&site.slug, site.uid(), &new).await?;
    // Nell'audit i nomi cambiati, mai i valori.
    let changed: Vec<&str> = new.keys().chain(current.keys()).filter(|k| new.get(*k) != current.get(*k)).map(String::as_str).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    app.auth.audit(Some(user.id), "site.env", Some(id), &changed.join(", ")).await?;
    Ok(Json(json!({ "changed": changed })).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn admins_edit_the_env_but_not_what_the_panel_uses() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let url = format!("/api/sites/{id}/env");
        let (s, _, _) = call(&t, req("GET", &url, Some(&manager), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "contiene le credenziali SMTP del server");
        let (s, body, _) = call(&t, req("GET", &url, Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        let content = body["content"].as_str().unwrap().to_string();
        assert!(content.contains("DB_NAME='sona_it'"), "{content}");
        assert_eq!(body["protected"][0], "DB_NAME");
        let with_key = format!("{content}# chiave del servizio\nAPI_KEY=\"abc 123\"\n");
        let (s, body, _) = call(&t, req("PUT", &url, Some(&admin), Some(json!({"content": with_key})))).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert_eq!(t.app.ctx.host.env.lock().unwrap()["API_KEY"], "abc 123");
        let (s, body, _) = call(&t, req("PUT", &url, Some(&admin), Some(json!({"content": "DB_NAME='altro'\n"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("DB_NAME"), "{body}");
        let (s, body, _) = call(&t, req("PUT", &url, Some(&admin), Some(json!({"content": format!("{content}SENZA_UGUALE\n")})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().starts_with("riga "), "{body}");
    }
}
