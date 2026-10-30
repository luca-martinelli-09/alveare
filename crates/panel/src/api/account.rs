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
}
