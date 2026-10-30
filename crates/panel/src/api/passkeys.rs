//! Passkey: registrazione dal profilo, elenco e rimozione, accesso dalla pagina di login.
use axum::routing::delete;
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

use super::*;
use crate::auth::passkeys::Passkeys;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/me/passkeys", get(list::<H>))
        .route("/api/me/passkeys/register/start", post(register_start::<H>))
        .route("/api/me/passkeys/register/finish", post(register_finish::<H>))
        .route("/api/me/passkeys/{pid}", delete(remove::<H>))
        .route("/api/auth/passkey/start", post(login_start::<H>))
        .route("/api/auth/passkey/finish", post(login_finish::<H>))
}

fn passkeys<H>(app: &App<H>) -> ApiResult<&Passkeys> {
    app.passkeys.as_ref().ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "passkey non disponibili: serve ALVEARE_PANEL_HOST"))
}

async fn list<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let list: Vec<_> = app.auth.passkeys_of(user.id).await?.into_iter().map(|(info, _)| info).collect();
    Ok(Json(json!({ "enabled": app.passkeys.is_some(), "passkeys": list })).into_response())
}

#[derive(Deserialize)]
struct RegisterStart {
    #[serde(default)]
    password: String,
}

async fn register_start<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<RegisterStart>) -> ApiResult<Response> {
    let user = reauth(&app, &m, &h, &r.password).await?;
    let (id, options) = passkeys(&app)?.start_registration(&app.auth, user.id, now()).await?;
    Ok(Json(json!({ "id": id, "options": options })).into_response())
}

#[derive(Deserialize)]
struct RegisterFinish {
    id: String,
    name: String,
    credential: RegisterPublicKeyCredential,
}

async fn register_finish<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<RegisterFinish>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let name = r.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::bad_request("dai un nome alla passkey (es. Portatile)"));
    }
    let pid = passkeys(&app)?.finish_registration(&app.auth, user.id, &r.id, name, &r.credential, now()).await?;
    app.auth.audit(Some(user.id), "passkey.add", None, name).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": pid }))).into_response())
}

async fn remove<H: Host>(State(app): State<App<H>>, Path(pid): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    if !app.auth.delete_passkey(user.id, pid).await? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "passkey non trovata"));
    }
    app.auth.audit(Some(user.id), "passkey.remove", None, &pid.to_string()).await?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct LoginStart {
    email: String,
}

async fn login_start<H: Host>(State(app): State<App<H>>, h: HeaderMap, Json(r): Json<LoginStart>) -> ApiResult<Response> {
    let (id, options) = passkeys(&app)?.start_login(&app.auth, &r.email, &client_ip(&h), now()).await?;
    Ok(Json(json!({ "id": id, "options": options })).into_response())
}

#[derive(Deserialize)]
struct LoginFinish {
    id: String,
    credential: PublicKeyCredential,
}

async fn login_finish<H: Host>(State(app): State<App<H>>, h: HeaderMap, Json(r): Json<LoginFinish>) -> ApiResult<Response> {
    let ua = h.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = passkeys(&app)?.finish_login(&app.auth, &r.id, &r.credential, &client_ip(&h), ua, now()).await?;
    Ok(([(header::SET_COOKIE, session_cookie(&token, app.secure_cookie))], Json(json!({ "stage": Stage::Full }))).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use axum::http::StatusCode;
    use serde_json::json;
    use webauthn_authenticator_rs::softpasskey::SoftPasskey;
    use webauthn_authenticator_rs::WebauthnAuthenticator;
    use webauthn_rs::prelude::Url;

    #[tokio::test]
    async fn passkey_registration_and_login_over_http() {
        let mut t = setup().await;
        let (_, alice) = browser_session(&t, "a@example.org").await;
        let pw = json!({"password": "una-password-lunga"});
        let (s, _, _) = call(&t, with_cookie("POST", "/api/me/passkeys/register/start", &alice, true, Some(pw.clone()))).await;
        assert_eq!(s, StatusCode::CONFLICT, "serve l'host del pannello");
        let origin = "https://pannello.example.org";
        t.app = t.app.clone().with_passkeys(Some(crate::auth::passkeys::Passkeys::new(origin).unwrap()));
        let mut device = WebauthnAuthenticator::new(SoftPasskey::new(true));
        let (_, start, _) = call(&t, with_cookie("POST", "/api/me/passkeys/register/start", &alice, true, Some(pw))).await;
        let cred = device.do_registration(Url::parse(origin).unwrap(), serde_json::from_value(start["options"].clone()).unwrap()).unwrap();
        let (s, _, _) = call(&t, with_cookie("POST", "/api/me/passkeys/register/finish", &alice, true, Some(json!({"id": start["id"], "name": "Telefono", "credential": cred})))).await;
        assert_eq!(s, StatusCode::CREATED);
        let (_, list, _) = call(&t, with_cookie("GET", "/api/me/passkeys", &alice, false, None)).await;
        assert_eq!(list["passkeys"][0]["name"], "Telefono");

        let (s, start, _) = call(&t, req("POST", "/api/auth/passkey/start", None, Some(json!({"email": "a@example.org"})))).await;
        assert_eq!(s, StatusCode::OK);
        let assertion = device.do_authentication(Url::parse(origin).unwrap(), serde_json::from_value(start["options"].clone()).unwrap()).unwrap();
        let (s, body, h) = call(&t, req("POST", "/api/auth/passkey/finish", None, Some(json!({"id": start["id"], "credential": assertion})))).await;
        assert_eq!((s, body["stage"].as_str()), (StatusCode::OK, Some("full")));
        let cookie = h["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
        let (s, me, _) = call(&t, with_cookie("GET", "/api/me", &cookie, false, None)).await;
        assert_eq!((s, me["user"]["email"].as_str()), (StatusCode::OK, Some("a@example.org")));
        let (s, _, _) = call(&t, req("POST", "/api/auth/passkey/start", None, Some(json!({"email": "nessuno@example.org"})))).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
}
