//! Email di un sito: mittente e nome (SMTP_FROM e SMTP_FROM_NAME nel .env del sito) e invio di prova.
use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}/email", get(show::<H>).put(save::<H>))
        .route("/api/sites/{id}/email/test", post(send_test::<H>))
}

/// Indirizzo semplice (lettere, cifre, . _ % + -): finisce nel .env e, per la prova, nel codice PHP.
fn simple_email(s: &str) -> bool {
    let ok = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b"._%+-".contains(&b));
    s.len() <= 254 && s.split_once('@').is_some_and(|(local, domain)| ok(local) && ok(domain) && domain.contains('.'))
}

async fn show<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let site = app.ctx.store.site(id).await?;
    let env = app.ctx.host.read_site_env(&site.slug).await?;
    let get = |k: &str| env.get(k).cloned().unwrap_or_default();
    Ok(Json(json!({ "from": get("SMTP_FROM"), "from_name": get("SMTP_FROM_NAME"), "host": get("SMTP_HOST") })).into_response())
}

#[derive(Deserialize)]
struct Sender {
    from: String,
    from_name: String,
}

async fn save<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<Sender>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let from = r.from.trim();
    // Il .env usa apici singoli: l'apice diritto diventa tipografico, come alla creazione del sito.
    let name = r.from_name.trim().replace('\'', "’");
    if !simple_email(from) {
        return Err(ApiError::bad_request("mittente non valido"));
    }
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request("nome del mittente non valido"));
    }
    let site = app.ctx.store.site(id).await?;
    app.ctx.host.set_site_env(&site.slug, site.uid(), "SMTP_FROM", from).await?;
    app.ctx.host.set_site_env(&site.slug, site.uid(), "SMTP_FROM_NAME", &name).await?;
    app.auth.audit(Some(user.id), "site.email_sender", Some(id), &format!("{name} <{from}>")).await?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct TestReq {
    to: String,
}

/// Email di prova con wp_mail: restituisce l'esito e, se fallisce, l'errore di SMTP così com'è.
async fn send_test<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<TestReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let to = r.to.trim();
    if !simple_email(to) {
        return Err(ApiError::bad_request("destinatario non valido"));
    }
    let site = app.ctx.store.site(id).await?;
    let php = format!(
        "add_action('wp_mail_failed', function ($e) {{ echo 'ALVEARE_MAIL_ERR ' . $e->get_error_message(); }}); \
         if (wp_mail('{to}', 'Prova da Alveare', 'Email di prova inviata dal pannello Alveare.')) {{ echo 'ALVEARE_MAIL_OK'; }}"
    );
    let (_, out) = app.ctx.host.wp_output(&site.slug, &["eval".to_string(), php]).await?;
    app.auth.audit(Some(user.id), "site.email_test", Some(id), to).await?;
    let ok = out.contains("ALVEARE_MAIL_OK");
    let error = out.split_once("ALVEARE_MAIL_ERR ").map(|(_, e)| e.lines().next().unwrap_or_default().trim().to_string());
    Ok(Json(json!({ "ok": ok, "error": if ok { None } else { Some(error.unwrap_or_else(|| out.trim().chars().take(500).collect())) } })).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn managers_set_the_sender_and_send_a_test() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let url = format!("/api/sites/{id}/email");
        let (s, _, _) = call(&t, req("GET", &url, Some(&reader), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, body, _) = call(&t, req("GET", &url, Some(&manager), None)).await;
        assert_eq!((s, body["from"].as_str(), body["from_name"].as_str()), (StatusCode::OK, Some("noreply@sona.it"), Some("Prova")));
        for bad in [json!({"from": "non-email", "from_name": "X"}), json!({"from": "a@b.it\nBcc: x@y.it", "from_name": "X"}), json!({"from": "a@b.it", "from_name": "riga\nnuova"})] {
            let (s, _, _) = call(&t, req("PUT", &url, Some(&manager), Some(bad.clone()))).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        }
        let (s, _, _) = call(&t, req("PUT", &url, Some(&manager), Some(json!({"from": "info@sona.it", "from_name": "Pro Loco d'Arco"})))).await;
        assert_eq!(s, StatusCode::OK);
        let env = t.app.ctx.host.env.lock().unwrap().clone();
        assert_eq!((env["SITE_ENV SMTP_FROM"].as_str(), env["SITE_ENV SMTP_FROM_NAME"].as_str()), ("info@sona.it", "Pro Loco d’Arco"), "l'apice diritto romperebbe il .env");
        let (s, _, _) = call(&t, req("POST", &format!("{url}/test"), Some(&manager), Some(json!({"to": "x'); system('id"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "il destinatario finisce nel codice PHP: solo indirizzi semplici");
        let (s, body, _) = call(&t, req("POST", &format!("{url}/test"), Some(&manager), Some(json!({"to": "luca@esempio.it"})))).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert!(t.app.ctx.host.calls().iter().any(|c| c.starts_with("wp sona_it eval ") && c.contains("wp_mail('luca@esempio.it'")));
        assert_eq!(body["ok"], false, "FakeHost non risponde ALVEARE_MAIL_OK");
    }
}
