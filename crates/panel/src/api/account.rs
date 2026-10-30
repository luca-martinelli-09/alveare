//! Profilo: sessioni aperte e token API dell'utente, con revoca.
use axum::routing::delete;

use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/me/sessions", get(sessions::<H>))
        .route("/api/me/sessions/others", post(revoke_others::<H>))
        .route("/api/me/sessions/{sid}", delete(revoke::<H>))
        .route("/api/me/tokens", get(tokens::<H>))
        .route("/api/me/tokens/{tid}", delete(revoke_token::<H>))
}

async fn sessions<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    Ok(Json(app.auth.sessions_of(user.id, session_token(&h).as_deref(), now()).await?).into_response())
}

async fn revoke<H: Host>(State(app): State<App<H>>, Path(sid): Path<String>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    if !app.auth.revoke_session(user.id, &sid).await? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "sessione non trovata"));
    }
    app.auth.audit(Some(user.id), "session.revoke", None, &sid).await?;
    Ok(Json(json!({})).into_response())
}

async fn revoke_others<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let token = session_token(&h).ok_or_else(|| ApiError::bad_request("serve una sessione del browser"))?;
    let n = app.auth.revoke_other_sessions(user.id, &token).await?;
    app.auth.audit(Some(user.id), "session.revoke_others", None, &format!("{n} sessioni")).await?;
    Ok(Json(json!({ "revoked": n })).into_response())
}

async fn tokens<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    Ok(Json(app.auth.tokens_of(user.id).await?).into_response())
}

async fn revoke_token<H: Host>(State(app): State<App<H>>, Path(tid): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    if !app.auth.revoke_token(user.id, tid).await? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "token non trovato"));
    }
    app.auth.audit(Some(user.id), "token.revoke", None, &tid.to_string()).await?;
    Ok(Json(json!({})).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn tokens_are_listed_and_revoked_by_their_owner() {
        let t = setup().await;
        let (_, alice) = token_for(&t, "a@example.org", false).await;
        let (_, bob) = token_for(&t, "b@example.org", false).await;
        let (_, list, _) = call(&t, req("GET", "/api/me/tokens", Some(&alice), None)).await;
        let id = list[0]["id"].as_i64().unwrap();
        assert!(list[0].get("token_hash").is_none());
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/me/tokens/{id}"), Some(&bob), None)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "non è suo");
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/me/tokens/{id}"), Some(&alice), None)).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, req("GET", "/api/me/tokens", Some(&alice), None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "il token revocato non vale più");
    }

    #[tokio::test]
    async fn new_credentials_need_the_password_and_a_browser_session() {
        let t = setup().await;
        let (uid, me) = browser_session(&t, "a@example.org").await;
        let api = t.app.auth.create_api_token(uid, "cli", i64::MAX).await.unwrap();
        let ok = serde_json::json!({"name": "ci", "days": 30, "password": "una-password-lunga"});
        let (s, _, _) = call(&t, req("POST", "/api/me/tokens", Some(&api), Some(ok.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "un token non ne crea altri");
        for bad in [serde_json::json!({"name": "ci", "days": 30}), serde_json::json!({"name": "ci", "days": 30, "password": "sbagliata"})] {
            let (s, _, _) = call(&t, with_cookie("POST", "/api/me/tokens", &me, true, Some(bad))).await;
            assert_eq!(s, StatusCode::FORBIDDEN);
        }
        let (s, body, _) = call(&t, with_cookie("POST", "/api/me/tokens", &me, true, Some(ok))).await;
        assert_eq!(s, StatusCode::CREATED, "{body}");
        let pw = serde_json::json!({"password": "una-password-lunga"});
        let (s, _, _) = call(&t, req("POST", "/api/me/passkeys/register/start", Some(&api), Some(pw.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "nemmeno una passkey");
        let (s, _, _) = call(&t, with_cookie("POST", "/api/me/passkeys/register/start", &me, true, Some(serde_json::json!({"password": "sbagliata"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, _) = call(&t, with_cookie("POST", "/api/me/passkeys/register/start", &me, true, Some(pw))).await;
        assert_eq!(s, StatusCode::CONFLICT, "password giusta: si arriva alle passkey (qui non configurate)");
    }

    #[tokio::test]
    async fn invite_by_email_then_first_access_from_the_link() {
        let mut t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let body = serde_json::json!({"email": "nuovo@example.org", "name": "Nuovo", "invite": true});
        let (s, _, _) = call(&t, req("POST", "/api/users", Some(&admin), Some(body.clone()))).await;
        assert_eq!(s, StatusCode::CONFLICT, "senza SMTP niente inviti");
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        t.app = t.app.clone().with_mailer(crate::mailer::Mailer::Memory(sent.clone()), Some("https://pannello.example.org".into()));
        let (_, opts, _) = call(&t, req("GET", "/api/auth/options", None, None)).await;
        assert_eq!(opts["email"], true);
        let (s, out, _) = call(&t, req("POST", "/api/users", Some(&admin), Some(body))).await;
        assert_eq!(s, StatusCode::CREATED, "{out}");
        assert!(out.get("temporary_password").is_none());
        let (to, _, text) = sent.lock().unwrap()[0].clone();
        assert_eq!(to, "nuovo@example.org");
        let token = text.split("/invito#").nth(1).unwrap().split_whitespace().next().unwrap().to_string();
        let (s, body, h) = call(&t, req("POST", "/api/auth/invite", None, Some(serde_json::json!({"token": token})))).await;
        assert_eq!((s, body["stage"].as_str()), (StatusCode::OK, Some("setup")));
        assert!(h["set-cookie"].to_str().unwrap().starts_with("alveare_session="));
        let (s, _, _) = call(&t, req("POST", "/api/auth/invite", None, Some(serde_json::json!({"token": token})))).await;
        assert_eq!(s, StatusCode::GONE, "monouso");
        let (s, out, _) = call(&t, req("POST", "/api/auth/reset", None, Some(serde_json::json!({"email": "nessuno@example.org"})))).await;
        assert_eq!((s, out["sent"].as_bool()), (StatusCode::OK, Some(true)), "stessa risposta anche se l'email non esiste");
        assert_eq!(sent.lock().unwrap().len(), 1);
    }
}
