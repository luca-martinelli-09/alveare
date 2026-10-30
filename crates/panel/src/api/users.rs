//! Profilo di chi è collegato (/api/me) e gestione di un utente da parte degli admin (/api/users/{id}).
use axum::routing::{delete, patch};

use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/me", patch(update_me::<H>))
        .route("/api/me/password", post(change_password::<H>))
        .route("/api/me/totp", post(totp_start::<H>))
        .route("/api/me/totp/confirm", post(totp_confirm::<H>))
        .route("/api/users/{id}", get(detail::<H>).patch(update_user::<H>))
        .route("/api/users/{id}/password", post(temporary_password::<H>))
        .route("/api/users/{id}/totp/reset", post(reset_totp::<H>))
        .route("/api/users/{id}/sessions/revoke", post(revoke_all::<H>))
        .route("/api/users/{id}/sessions/{sid}", delete(revoke_one::<H>))
}

fn valid(name: &str, email: &str) -> ApiResult<()> {
    let email = email.trim();
    if name.trim().is_empty() || !email.contains('@') || email.len() > 254 {
        return Err(ApiError::bad_request("email o nome non validi"));
    }
    Ok(())
}

#[derive(Deserialize)]
struct ProfileReq {
    name: String,
    email: String,
    #[serde(default)]
    password: String,
}

async fn update_me<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<ProfileReq>) -> ApiResult<Response> {
    let mut user = caller(&app, &m, &h).await?;
    valid(&r.name, &r.email)?;
    // L'email è dove arriva il reset della password: cambiarla vale quanto una credenziale nuova.
    if !r.email.trim().eq_ignore_ascii_case(&user.email) {
        user = reauth(&app, &m, &h, &r.password).await?;
    }
    app.auth.update_profile(user.id, &r.name, &r.email).await?;
    app.auth.audit(Some(user.id), "profile.update", None, r.email.trim()).await?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct PasswordChangeReq {
    current: String,
    new: String,
}

async fn change_password<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<PasswordChangeReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let keep = session_token(&h).unwrap_or_default();
    app.auth.change_password(user.id, &r.current, &r.new, &keep, &client_ip(&h), now()).await.map_err(wrong_password)?;
    app.auth.audit(Some(user.id), "profile.password", None, "").await?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct PasswordReq {
    password: String,
}

async fn totp_start<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<PasswordReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    Ok(Json(app.auth.start_totp_change(user.id, &r.password, &client_ip(&h), now()).await.map_err(wrong_password)?).into_response())
}

#[derive(Deserialize)]
struct CodeReq {
    code: String,
}

async fn totp_confirm<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<CodeReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.auth.confirm_totp_change(user.id, &r.code, &client_ip(&h), now()).await?;
    app.auth.audit(Some(user.id), "profile.totp", None, "nuova app").await?;
    Ok(Json(json!({})).into_response())
}

async fn admin<H: Host>(app: &App<H>, m: &Method, h: &HeaderMap, id: i64) -> ApiResult<User> {
    let admin = caller(app, m, h).await?;
    require_admin(&admin)?;
    app.auth.user(id).await.map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "utente non trovato"))?;
    Ok(admin)
}

async fn detail<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    admin(&app, &m, &h, id).await?;
    let mut out = serde_json::to_value(app.auth.user_detail(id).await?).map_err(internal)?;
    out["sessions"] = json!(app.auth.sessions_of(id, None, now()).await?);
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
struct UserReq {
    name: String,
    email: String,
    is_admin: bool,
}

async fn update_user<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<UserReq>) -> ApiResult<Response> {
    let by = admin(&app, &m, &h, id).await?;
    valid(&r.name, &r.email)?;
    app.auth.update_profile(id, &r.name, &r.email).await?;
    app.auth.set_admin(id, r.is_admin).await?;
    app.auth.audit(Some(by.id), "user.update", None, &format!("{id}: {} {}", r.email.trim(), if r.is_admin { "admin" } else { "per sito" })).await?;
    Ok(Json(json!({})).into_response())
}

async fn temporary_password<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let by = admin(&app, &m, &h, id).await?;
    let temp = app.auth.temporary_password(id).await?;
    app.auth.audit(Some(by.id), "user.password_reset", None, &id.to_string()).await?;
    Ok(Json(json!({ "temporary_password": temp })).into_response())
}

async fn reset_totp<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let by = admin(&app, &m, &h, id).await?;
    app.auth.reset_totp(id).await?;
    app.auth.audit(Some(by.id), "user.totp_reset", None, &id.to_string()).await?;
    Ok(Json(json!({})).into_response())
}

async fn revoke_all<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let by = admin(&app, &m, &h, id).await?;
    let keep = if by.id == id { session_token(&h).unwrap_or_default() } else { String::new() };
    let n = app.auth.revoke_access(id, &keep, false).await?;
    app.auth.audit(Some(by.id), "user.sessions_revoke", None, &format!("{id}: {n}")).await?;
    Ok(Json(json!({ "closed": n })).into_response())
}

async fn revoke_one<H: Host>(State(app): State<App<H>>, Path((id, sid)): Path<(i64, String)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let by = admin(&app, &m, &h, id).await?;
    if !app.auth.revoke_session(id, &sid).await? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "sessione non trovata"));
    }
    app.auth.audit(Some(by.id), "user.session_revoke", None, &id.to_string()).await?;
    Ok(Json(json!({})).into_response())
}


#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn everyone_edits_their_own_profile() {
        let t = setup().await;
        let (uid, me) = browser_session(&t, "a@example.org").await;
        token_for(&t, "b@example.org", false).await;
        let patch = |body| with_cookie("PATCH", "/api/me", &me, true, Some(body));
        let (s, _, _) = call(&t, patch(json!({"name": "Anna", "email": "a@example.org"}))).await;
        assert_eq!(s, StatusCode::OK, "il nome si cambia senza password");
        let (_, body, _) = call(&t, with_cookie("GET", "/api/me", &me, false, None)).await;
        assert_eq!(body["user"]["name"], "Anna");
        for body in [json!({"name": "Anna", "email": "anna@example.org"}), json!({"name": "Anna", "email": "anna@example.org", "password": "sbagliata"})] {
            let (s, _, _) = call(&t, patch(body)).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "l'email (dove arriva il reset) vuole la password");
        }
        let (s, _, _) = call(&t, patch(json!({"name": "Anna", "email": "anna@example.org", "password": "una-password-lunga"}))).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, patch(json!({"name": "Anna", "email": "b@example.org", "password": "una-password-lunga"}))).await;
        assert_eq!(s, StatusCode::CONFLICT);
        let (s, _, _) = call(&t, patch(json!({"name": " ", "email": "x"}))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _, _) = call(&t, with_cookie("POST", "/api/me/password", &me, true, Some(json!({"current": "sbagliata", "new": "nuova-password-lunga"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "password sbagliata: 403, non 401 (che fa uscire)");
        let api = t.app.auth.create_api_token(uid, "cli", i64::MAX).await.unwrap();
        let (s, _, _) = call(&t, req("PATCH", "/api/me", Some(&api), Some(json!({"name": "Anna", "email": "c@example.org", "password": "una-password-lunga"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "con un token API l'email non si cambia");
    }

    #[tokio::test]
    async fn admins_manage_a_user_and_others_cannot() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let (uid, user) = token_for(&t, "u@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", &format!("/api/users/{uid}"), Some(&user), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, body, _) = call(&t, req("GET", &format!("/api/users/{uid}"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert_eq!((body["user"]["email"].as_str(), body["has_totp"].as_bool()), (Some("u@example.org"), Some(false)));
        assert!(body["sessions"].is_array());
        let (s, _, _) = call(&t, req("PATCH", &format!("/api/users/{uid}"), Some(&admin), Some(json!({"name": "Ugo", "email": "ugo@example.org", "is_admin": true})))).await;
        assert_eq!(s, StatusCode::OK);
        let (_, body, _) = call(&t, req("GET", &format!("/api/users/{uid}"), Some(&admin), None)).await;
        assert_eq!((body["user"]["name"].as_str(), body["user"]["is_admin"].as_bool()), (Some("Ugo"), Some(true)));
        let (s, body, _) = call(&t, req("POST", &format!("/api/users/{uid}/password"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert!(body["temporary_password"].as_str().is_some_and(|p| p.len() >= 12));
        let (s, _, _) = call(&t, req("POST", &format!("/api/users/{uid}/totp/reset"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        let tok = t.app.auth.create_api_token(uid, "cli", i64::MAX).await.unwrap();
        let (s, _, _) = call(&t, req("POST", &format!("/api/users/{uid}/sessions/revoke"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, req("GET", "/api/me", Some(&tok), None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "chiudere tutto revoca anche i token API");
    }
}
