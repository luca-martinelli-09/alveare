//! Comandi una tantum dal pannello: WP-CLI, artisan o uno script PHP del sito, eseguiti dall'agente
//! con l'utente del sito. Stesse regole dei cron personalizzati (`crons::prepare`), niente shell.
use super::*;
use crate::crons::{prepare, Action, CronKind};
use crate::store::SiteKind;

/// Limite di durata per artisan e script PHP (WP-CLI ha il suo nell'agente).
const PHP_TIMEOUT_S: u32 = 300;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new().route("/api/sites/{id}/commands", post(run::<H>))
}

#[derive(Deserialize)]
struct CommandReq {
    kind: CronKind,
    command: String,
}

async fn run<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<CommandReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let site = app.ctx.store.site(id).await?;
    let allowed = match r.kind {
        CronKind::Wp => site.kind == SiteKind::Wordpress,
        CronKind::Artisan => site.kind == SiteKind::Laravel,
        CronKind::Php => true,
        CronKind::Http => false,
    };
    if !allowed {
        return Err(ApiError::bad_request(format!("{} non si esegue su questo sito", r.kind.as_str())));
    }
    let action = prepare(r.kind, &r.command, &[]).map_err(ApiError::bad_request)?;
    // Nel log e nell'audit il comando intero ("wp plugin list"), con le password come ***.
    let shown = match &action {
        Action::Wp(args) => format!("wp {}", crate::host::shown_args(args)),
        Action::Php(args) => format!("php {}", crate::host::shown_args(args)),
        Action::Http { .. } => unreachable!("rifiutato sopra"),
    };
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("command", Some(id), Some(user.id), move |mut log| async move {
            auth.audit(Some(uid), "command", Some(id), &shown).await?;
            log.push(format!("$ {shown}"));
            let (ok, out) = match action {
                Action::Wp(args) => ctx.host.wp_output(&site.slug, &args).await?,
                Action::Php(args) => ctx.host.php(&site.slug, &args, PHP_TIMEOUT_S).await?,
                Action::Http { .. } => unreachable!("rifiutato sopra"),
            };
            for line in out.lines() {
                log.push(line);
            }
            if !ok {
                anyhow::bail!("il comando è terminato con errore");
            }
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn managers_run_one_off_commands_with_the_cron_rules() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let url = format!("/api/sites/{id}/commands");
        let wp = json!({"kind": "wp", "command": "wp option get blogname"});
        let (s, _, _) = call(&t, req("POST", &url, Some(&reader), Some(wp.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        for bad in [
            json!({"kind": "artisan", "command": "migrate"}),
            json!({"kind": "http", "command": "https://sona.it/"}),
            json!({"kind": "php", "command": "/etc/x.php"}),
            json!({"kind": "wp", "command": "option get 'blogname"}),
        ] {
            let (s, _, _) = call(&t, req("POST", &url, Some(&manager), Some(bad.clone()))).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        }
        let (s, body, _) = call(&t, req("POST", &url, Some(&manager), Some(wp))).await;
        assert_eq!(s, StatusCode::ACCEPTED, "{body}");
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!((job.status.as_str(), job.log.first().map(String::as_str)), ("done", Some("$ wp option get blogname")));
        assert!(t.app.ctx.host.calls().contains(&"wp sona_it option get blogname".to_string()));
        let (s, body, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"kind": "wp", "command": "user update anna --user_pass=Segreta123 --skip-email"})))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!(job.log.first().map(String::as_str), Some("$ wp user update anna --user_pass=*** --skip-email"), "password mai nel log");
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{id}/audit"), Some(&admin), None)).await;
        assert!(!audit.to_string().contains("Segreta123"), "né nell'audit");
        // Su più righe: dentro le virgolette l'a capo resta, fuori separa gli argomenti.
        let multi = "eval \"echo 1;\necho 2;\"\n--skip-plugins";
        let (s, body, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"kind": "wp", "command": multi})))).await;
        assert_eq!(s, StatusCode::ACCEPTED, "{body}");
        wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert!(t.app.ctx.host.calls().contains(&"wp sona_it eval echo 1;\necho 2; --skip-plugins".to_string()), "{:?}", t.app.ctx.host.calls());
        let (s, body, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"kind": "php", "command": "public/script.php --x"})))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!(job.log.first().map(String::as_str), Some("$ php public/script.php --x"), "il prefisso si aggiunge");
        assert!(t.app.ctx.host.calls().contains(&"php sona_it public/script.php --x (300 s)".to_string()));
    }
}
