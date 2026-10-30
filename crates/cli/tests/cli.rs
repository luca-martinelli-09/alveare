//! La CLI contro un finto pannello: stessi path e stesse risposte dell'API vera.
use std::sync::{Arc, Mutex};

use alveare::commands::{run, Cmd, Cron, Sites};
use alveare::Api;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

#[derive(Default)]
struct Fake {
    polls: usize,
    posted: Vec<Value>,
}
type S = Arc<Mutex<Fake>>;

fn authorized(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer alv_prova")
}

async fn start() -> (String, S) {
    let state = S::default();
    let app = Router::new()
        .route("/api/sites", get(|h: HeaderMap| async move {
            if !authorized(&h) {
                return Err((StatusCode::UNAUTHORIZED, Json(json!({ "error": "accesso richiesto" }))));
            }
            Ok(Json(json!([
                { "id": 1, "slug": "sona_it", "domain": "sona.it", "kind": "wordpress", "status": "active" },
                { "id": 2, "slug": "upupa_it", "domain": "upupa.it", "kind": "wordpress", "status": "suspended" }
            ])))
        }))
        .route("/api/sites/{id}/domains", get(|Path(id): Path<i64>| async move {
            Json(if id == 2 { json!([{ "hostname": "upupa.it", "kind": "primary" }, { "hostname": "www.upupa.it", "kind": "redirect" }]) } else { json!([]) })
        }))
        .route("/api/sites/{id}/wp", post(|State(s): State<S>, Path(id): Path<i64>, Json(b): Json<Value>| async move {
            s.lock().unwrap().posted.push(json!({ "site": id, "body": b }));
            Json(json!({ "job": 7 }))
        }))
        .route("/api/sites/{id}/cron/{cid}/run", post(|| async { (StatusCode::FORBIDDEN, Json(json!({ "error": "operazione non permessa" }))) }))
        .route("/api/jobs/{id}", get(|State(s): State<S>| async move {
            let mut s = s.lock().unwrap();
            s.polls += 1;
            Json(if s.polls < 2 {
                json!({ "id": 7, "status": "running", "log": ["akismet active 5.3"] })
            } else {
                json!({ "id": 7, "status": "done", "log": ["akismet active 5.3", "hello inactive 1.7"] })
            })
        }))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), state)
}

async fn output(api: &Api, cmd: Cmd, json_out: bool) -> anyhow::Result<String> {
    let mut out = Vec::new();
    run(api, cmd, json_out, &mut out).await?;
    Ok(String::from_utf8(out).unwrap())
}

#[tokio::test]
async fn sites_list_in_columns_or_json() {
    let (url, _) = start().await;
    let api = Api::new(&url, "alv_prova");
    let text = output(&api, Cmd::Sites(Sites::List), false).await.unwrap();
    assert!(text.lines().next().unwrap().contains("sona.it") && text.contains("suspended"), "{text}");
    let json: Value = serde_json::from_str(&output(&api, Cmd::Sites(Sites::List), true).await.unwrap()).unwrap();
    assert_eq!(json[1]["slug"], "upupa_it");
}

#[tokio::test]
async fn wp_resolves_the_site_by_alias_and_streams_the_job_log_once() {
    let (url, state) = start().await;
    let api = Api::new(&url, "alv_prova");
    let text = output(&api, Cmd::Wp { site: "www.upupa.it".into(), args: vec!["plugin".into(), "list".into()] }, false).await.unwrap();
    assert_eq!(text, "akismet active 5.3\nhello inactive 1.7\n", "ogni riga una volta sola");
    assert_eq!(state.lock().unwrap().posted[0], json!({ "site": 2, "body": { "args": ["plugin", "list"] } }));
}

#[tokio::test]
async fn panel_errors_are_reported_with_their_message() {
    let (url, _) = start().await;
    let err = output(&Api::new(&url, "alv_sbagliato"), Cmd::Sites(Sites::List), false).await.unwrap_err();
    assert_eq!(err.to_string(), "accesso richiesto (401)");
    let api = Api::new(&url, "alv_prova");
    let err = output(&api, Cmd::Cron(Cron::Run { site: "sona_it".into(), id: 3 }), false).await.unwrap_err();
    assert!(err.to_string().contains("operazione non permessa"), "{err}");
    let err = output(&api, Cmd::Wp { site: "nessuno.it".into(), args: vec!["x".into()] }, false).await.unwrap_err();
    assert!(err.to_string().contains("sito non trovato"));
}

#[test]
fn login_config_round_trip_is_private() {
    let dir = std::env::temp_dir().join(format!("alveare-cli-{}", std::process::id()));
    let p = dir.join("config.json");
    alveare::config::save_to(&p, "https://pannello.it", "alv_x").unwrap();
    assert_eq!(alveare::config::load_from(&p), Some(("https://pannello.it".into(), "alv_x".into())));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
