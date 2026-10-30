//! Strumenti comuni ai test dell'API.
use super::*;
use crate::secrets::SecretKey;
use crate::store::Store;
use crate::testing::FakeHost;
pub(crate) use axum::body::Body;
pub(crate) use axum::http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

pub(crate) struct T {
    pub app: App<FakeHost>,
    pub _templates: tempfile::TempDir,
}

pub(crate) async fn setup() -> T {
    let store = Store::open_memory().await.unwrap();
    let key = SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap();
    let templates = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(templates.path().join("wordpress")).unwrap();
    std::fs::write(templates.path().join("wordpress/template.toml"), "name = \"WordPress\"\nkind = \"wordpress\"\n").unwrap();
    let ctx = Ctx { store: store.clone(), host: FakeHost::default(), key: key.clone(), db_host: "mariadb".into(), global_env: Default::default(), backups: true, site_url: crate::flows::DEFAULT_SITE_URL.into() };
    let paths = crate::host::Paths::under(&templates.path().join("data"));
    let app = App::new(ctx, Auth::new(store.pool().clone(), key), templates.path().to_path_buf(), false).with_files(paths, false);
    T { app, _templates: templates }
}

pub(crate) async fn call(t: &T, req: Request<Body>) -> (StatusCode, serde_json::Value, axum::http::HeaderMap) {
    let resp = router(t.app.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null), headers)
}

pub(crate) fn req(method: &str, uri: &str, auth: Option<&str>, body: Option<serde_json::Value>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(tok) = auth {
        b = b.header("authorization", format!("Bearer {tok}"));
    }
    match body {
        Some(v) => b.header("content-type", "application/json").body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

pub(crate) async fn token_for(t: &T, email: &str, admin: bool) -> (i64, String) {
    let (id, _) = t.app.auth.create_user(email, "Prova", admin).await.unwrap();
    (id, t.app.auth.create_api_token(id, "test", i64::MAX).await.unwrap())
}

pub(crate) async fn create_site(t: &T, admin: &str, domain: &str) -> i64 {
    let (s, body, _) = call(t, req("POST", "/api/sites", Some(admin), Some(serde_json::json!({"domain": domain, "template": "wordpress", "title": "Prova"})))).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{body}");
    let job = body["job"].as_i64().unwrap();
    for _ in 0..200 {
        let v = t.app.jobs.get(job).await.unwrap();
        if v.status != "running" {
            assert_eq!(v.status, "done", "{:?}", v.log);
            return v.site_id.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("job non terminato");
}


pub(crate) async fn site_with_files(t: &T) -> (String, i64) {
    let (_, admin) = token_for(t, "admin@example.org", true).await;
    let id = create_site(t, &admin, "sona.it").await;
    // FakeHost non crea cartelle: le crea il test come farebbe l'host reale.
    crate::host::create_dirs(&t.app.paths, &alveare_core::site::Slug::parse("sona_it").unwrap(), None).unwrap();
    (admin, id)
}

pub(crate) fn raw(method: &str, uri: &str, auth: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder().method(method).uri(uri).header("authorization", format!("Bearer {auth}")).body(Body::from(body)).unwrap()
}

pub(crate) async fn wait_job(t: &T, job: i64) -> crate::jobs::JobView {
    for _ in 0..300 {
        let v = t.app.jobs.get(job).await.unwrap();
        if v.status != "running" {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("job non terminato");
}


pub(crate) fn cookie_of(headers: &axum::http::HeaderMap) -> String {
    let c = headers["set-cookie"].to_str().unwrap();
    c.split(';').next().unwrap().to_string()
}

pub(crate) fn with_cookie(method: &str, uri: &str, cookie: &str, csrf: bool, body: Option<serde_json::Value>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri).header("cookie", cookie).header("x-real-ip", "1.2.3.4");
    if csrf {
        b = b.header("x-alveare-csrf", "1");
    }
    match body {
        Some(v) => b.header("content-type", "application/json").body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}


/// Un utente non admin con quel ruolo sul sito.
pub(crate) async fn user_with_role(t: &T, email: &str, site: i64, role: crate::auth::Role) -> String {
    let (uid, token) = token_for(t, email, false).await;
    t.app.auth.assign_site(uid, site, role).await.unwrap();
    token
}
