//! API REST: la UI e gli script usano la stessa. Ogni handler verifica chi chiama e su quale sito.
use std::path::PathBuf;
use std::sync::Arc;

use alveare_core::site::{Domain, Slug};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{Auth, AuthError, Need, Role, Stage, User};
use crate::files::{FileError, SiteFiles, CHUNK, EDITOR_LIMIT};
use crate::flows::{self, CreateSite, Ctx, Host};
use crate::host::Paths;
use crate::jobs::Jobs;
use crate::store::StoreError;
use crate::template::Template;

const COOKIE: &str = "alveare_session";
const CSRF_HEADER: &str = "x-alveare-csrf";

pub struct App<H> {
    pub ctx: Arc<Ctx<H>>,
    pub auth: Auth,
    pub jobs: Jobs,
    pub templates: PathBuf,
    /// `Secure` sul cookie: sempre in produzione (HTTPS al bordo di Cloudflare).
    pub secure_cookie: bool,
    pub paths: Paths,
    /// I file scritti dal file manager tornano all'utente del sito (false solo nei test senza root).
    pub chown: bool,
    uploads: Arc<tokio::sync::Mutex<()>>,
}

impl<H> Clone for App<H> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            auth: self.auth.clone(),
            jobs: self.jobs.clone(),
            templates: self.templates.clone(),
            secure_cookie: self.secure_cookie,
            paths: self.paths.clone(),
            chown: self.chown,
            uploads: self.uploads.clone(),
        }
    }
}

impl<H: Host> App<H> {
    pub fn new(ctx: Ctx<H>, auth: Auth, templates: PathBuf, secure_cookie: bool) -> Self {
        let jobs = Jobs::new(ctx.store.pool().clone());
        Self { ctx: Arc::new(ctx), auth, jobs, templates, secure_cookie, paths: Paths::production(), chown: true, uploads: Default::default() }
    }

    pub fn with_files(self, paths: Paths, chown: bool) -> Self {
        Self { paths, chown, ..self }
    }
}

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl ApiError {
    fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self(status, msg.into())
    }
    fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
    fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "operazione non permessa")
    }
    fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "accesso richiesto")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::Invalid => Self::new(StatusCode::UNAUTHORIZED, e.to_string()),
            AuthError::Locked => Self::new(StatusCode::TOO_MANY_REQUESTS, e.to_string()),
            AuthError::WeakPassword => Self::bad_request(e.to_string()),
            AuthError::WrongStage => Self::new(StatusCode::CONFLICT, e.to_string()),
            AuthError::Db(_) | AuthError::Internal(_) => internal(e),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => Self::new(StatusCode::NOT_FOUND, e.to_string()),
            StoreError::HostnameTaken(_) => Self::new(StatusCode::CONFLICT, e.to_string()),
            StoreError::Db(_) => internal(e),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self::new(StatusCode::NOT_FOUND, "non trovato"),
            e => internal(e),
        }
    }
}

impl From<FileError> for ApiError {
    fn from(e: FileError) -> Self {
        match e {
            FileError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => Self::new(StatusCode::NOT_FOUND, "file non trovato"),
            // cap-std rifiuta gli accessi fuori dalla cartella con PermissionDenied.
            FileError::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => Self::bad_request("percorso non valido"),
            FileError::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists => Self::new(StatusCode::CONFLICT, "esiste già"),
            FileError::Io(io) => internal(io),
            e => Self::bad_request(e.to_string()),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        internal(e)
    }
}

/// Il dettaglio va nel log del server, non al client.
fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("errore interno: {e}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "errore interno")
}

type ApiResult<T> = Result<T, ApiError>;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// IP reale: Caddy lo risolve da CF-Connecting-IP e lo passa in X-Real-IP.
fn client_ip(h: &HeaderMap) -> String {
    h.get("x-real-ip").and_then(|v| v.to_str().ok()).unwrap_or("sconosciuto").to_string()
}

fn session_token(h: &HeaderMap) -> Option<String> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(COOKIE)?.strip_prefix('=').map(String::from))
}

fn is_mutation(m: &Method) -> bool {
    !matches!(*m, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Le modifiche via cookie richiedono un header che un form di un altro sito non può impostare.
fn check_csrf(method: &Method, h: &HeaderMap) -> ApiResult<()> {
    if is_mutation(method) && h.get(CSRF_HEADER).is_none() {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "header anti-CSRF mancante"));
    }
    Ok(())
}

async fn caller<H>(app: &App<H>, method: &Method, h: &HeaderMap) -> ApiResult<User> {
    if let Some(bearer) = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")) {
        return app.auth.token_user(bearer.trim(), now()).await.ok_or_else(ApiError::unauthorized);
    }
    let token = session_token(h).ok_or_else(ApiError::unauthorized)?;
    let s = app.auth.session(&token, now()).await.filter(|s| s.stage == Stage::Full).ok_or_else(ApiError::unauthorized)?;
    check_csrf(method, h)?;
    Ok(s.user)
}

async fn require_site<H>(app: &App<H>, user: &User, site_id: i64, need: Need) -> ApiResult<()> {
    if app.auth.can(user, site_id, need).await? { Ok(()) } else { Err(ApiError::forbidden()) }
}

fn require_admin(user: &User) -> ApiResult<()> {
    if user.is_admin { Ok(()) } else { Err(ApiError::forbidden()) }
}

pub fn router<H: Host>(app: App<H>) -> Router {
    Router::new()
        .route("/api/auth/login", post(login::<H>))
        .route("/api/auth/setup", post(setup_password::<H>))
        .route("/api/auth/setup/totp", post(confirm_totp::<H>))
        .route("/api/auth/totp", post(verify_totp::<H>))
        .route("/api/auth/logout", post(logout::<H>))
        .route("/api/me", get(me::<H>))
        .route("/api/me/tokens", post(create_token::<H>))
        .route("/api/users", get(list_users::<H>).post(create_user::<H>))
        .route("/api/users/{id}/block", post(block_user::<H>))
        .route("/api/users/{id}/sites/{site}", put(assign_site::<H>).delete(unassign_site::<H>))
        .route("/api/templates", get(list_templates::<H>))
        .route("/api/sites", get(list_sites::<H>).post(create_site::<H>))
        .route("/api/sites/{id}", get(get_site::<H>).delete(delete_site::<H>))
        .route("/api/sites/{id}/audit", get(site_audit::<H>))
        .route("/api/sites/{id}/files", get(list_files::<H>))
        .route("/api/sites/{id}/files/content", get(read_file::<H>).put(write_file::<H>).layer(DefaultBodyLimit::max(EDITOR_LIMIT as usize + 1024)))
        .route("/api/sites/{id}/files/download", get(download_file::<H>))
        .route("/api/sites/{id}/files/ops", post(file_op::<H>))
        .route("/api/sites/{id}/files/uploads", post(upload_start::<H>))
        .route("/api/sites/{id}/files/uploads/{upload}", get(upload_status::<H>))
        .route("/api/sites/{id}/files/uploads/{upload}/{n}", put(upload_chunk::<H>).layer(DefaultBodyLimit::max(CHUNK as usize + 1024)))
        .route("/api/jobs/{id}", get(get_job::<H>))
        .with_state(app)
}

#[derive(Deserialize)]
struct LoginReq {
    email: String,
    password: String,
}

fn session_cookie(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/{secure}")
}

async fn login<H: Host>(State(app): State<App<H>>, h: HeaderMap, Json(r): Json<LoginReq>) -> ApiResult<Response> {
    let ua = h.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let (token, stage) = app.auth.login(&r.email, &r.password, &client_ip(&h), ua, now()).await?;
    Ok(([(header::SET_COOKIE, session_cookie(&token, app.secure_cookie))], Json(json!({ "stage": stage }))).into_response())
}

/// Passi del login dopo la password: sessione via cookie, sempre con header anti-CSRF.
async fn pending_token(method: &Method, h: &HeaderMap) -> ApiResult<String> {
    check_csrf(method, h)?;
    session_token(h).ok_or_else(ApiError::unauthorized)
}

#[derive(Deserialize)]
struct PasswordReq {
    password: String,
}

async fn setup_password<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<PasswordReq>) -> ApiResult<Response> {
    let token = pending_token(&m, &h).await?;
    Ok(Json(app.auth.setup_password(&token, &r.password, now()).await?).into_response())
}

#[derive(Deserialize)]
struct CodeReq {
    code: String,
}

async fn confirm_totp<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<CodeReq>) -> ApiResult<Response> {
    let token = pending_token(&m, &h).await?;
    app.auth.confirm_totp(&token, &r.code, &client_ip(&h), now()).await?;
    Ok(Json(json!({ "stage": Stage::Full })).into_response())
}

async fn verify_totp<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<CodeReq>) -> ApiResult<Response> {
    let token = pending_token(&m, &h).await?;
    app.auth.verify_totp(&token, &r.code, &client_ip(&h), now()).await?;
    Ok(Json(json!({ "stage": Stage::Full })).into_response())
}

async fn logout<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let token = pending_token(&m, &h).await?;
    app.auth.logout(&token).await?;
    let expired = format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    Ok(([(header::SET_COOKIE, expired)], Json(json!({}))).into_response())
}

async fn me<H: Host>(State(app): State<App<H>>, h: HeaderMap) -> ApiResult<Response> {
    if h.contains_key(header::AUTHORIZATION) {
        let user = caller(&app, &Method::GET, &h).await?;
        return Ok(Json(json!({ "user": user, "stage": Stage::Full })).into_response());
    }
    let token = session_token(&h).ok_or_else(ApiError::unauthorized)?;
    let s = app.auth.session(&token, now()).await.ok_or_else(ApiError::unauthorized)?;
    Ok(Json(json!({ "user": s.user, "stage": s.stage })).into_response())
}

#[derive(Deserialize)]
struct TokenReq {
    name: String,
    days: i64,
}

async fn create_token<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<TokenReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    if !(1..=365).contains(&r.days) || r.name.trim().is_empty() {
        return Err(ApiError::bad_request("nome obbligatorio e durata tra 1 e 365 giorni"));
    }
    let token = app.auth.create_api_token(user.id, r.name.trim(), now() + r.days * 86400).await?;
    app.auth.audit(Some(user.id), "token.create", None, r.name.trim()).await?;
    Ok((StatusCode::CREATED, Json(json!({ "token": token }))).into_response())
}

async fn list_users<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    Ok(Json(app.auth.users().await?).into_response())
}

#[derive(Deserialize)]
struct NewUserReq {
    email: String,
    name: String,
    #[serde(default)]
    is_admin: bool,
}

async fn create_user<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<NewUserReq>) -> ApiResult<Response> {
    let admin = caller(&app, &m, &h).await?;
    require_admin(&admin)?;
    let email = r.email.trim();
    if !email.contains('@') || email.len() > 254 || r.name.trim().is_empty() {
        return Err(ApiError::bad_request("email o nome non validi"));
    }
    let (id, temp) = app.auth.create_user(email, r.name.trim(), r.is_admin).await.map_err(|e| match e {
        AuthError::Db(sqlx::Error::Database(d)) if d.is_unique_violation() => ApiError::new(StatusCode::CONFLICT, "email già registrata"),
        e => e.into(),
    })?;
    app.auth.audit(Some(admin.id), "user.create", None, email).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "temporary_password": temp }))).into_response())
}

#[derive(Deserialize)]
struct BlockReq {
    blocked: bool,
}

async fn block_user<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<BlockReq>) -> ApiResult<Response> {
    let admin = caller(&app, &m, &h).await?;
    require_admin(&admin)?;
    if admin.id == id {
        return Err(ApiError::bad_request("non puoi bloccare te stesso"));
    }
    app.auth.set_blocked(id, r.blocked).await?;
    app.auth.audit(Some(admin.id), if r.blocked { "user.block" } else { "user.unblock" }, None, &id.to_string()).await?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct RoleReq {
    role: Role,
}

async fn assign_site<H: Host>(State(app): State<App<H>>, Path((id, site)): Path<(i64, i64)>, m: Method, h: HeaderMap, Json(r): Json<RoleReq>) -> ApiResult<Response> {
    let admin = caller(&app, &m, &h).await?;
    require_admin(&admin)?;
    app.ctx.store.site(site).await?;
    app.auth.assign_site(id, site, r.role).await?;
    app.auth.audit(Some(admin.id), "user.assign", Some(site), &format!("utente {id}: {}", r.role.as_str())).await?;
    Ok(Json(json!({})).into_response())
}

async fn unassign_site<H: Host>(State(app): State<App<H>>, Path((id, site)): Path<(i64, i64)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let admin = caller(&app, &m, &h).await?;
    require_admin(&admin)?;
    app.auth.unassign_site(id, site).await?;
    app.auth.audit(Some(admin.id), "user.unassign", Some(site), &format!("utente {id}")).await?;
    Ok(Json(json!({})).into_response())
}

fn load_template(dir: &std::path::Path, name: &str) -> ApiResult<Template> {
    let name = Slug::parse(name).map_err(|_| ApiError::bad_request("nome di template non valido"))?;
    Template::load(&dir.join(name.as_str())).map_err(|_| ApiError::bad_request(format!("template {} non trovato", name.as_str())))
}

async fn list_templates<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    caller(&app, &m, &h).await?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&app.templates).map_err(internal)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Ok(t) = load_template(&app.templates, &name) {
            out.push(json!({ "id": name, "name": t.manifest.name, "kind": t.manifest.kind }));
        }
    }
    out.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok(Json(out).into_response())
}

async fn list_sites<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let visible = app.auth.visible_sites(&user).await?;
    let mut out = Vec::new();
    for site in app.ctx.store.sites().await? {
        if visible.as_ref().is_none_or(|v| v.contains(&site.id)) {
            let primary = app.ctx.store.domains(site.id).await?.into_iter().next().map(|(d, _)| d.as_str().to_string());
            out.push(json!({ "id": site.id, "slug": site.slug, "domain": primary, "kind": site.kind, "status": site.status }));
        }
    }
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
struct CreateSiteReq {
    domain: String,
    template: String,
    title: String,
    admin_email: Option<String>,
}

async fn create_site<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<CreateSiteReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let domain = Domain::parse(&r.domain).map_err(|_| ApiError::bad_request("dominio non valido"))?;
    let template = load_template(&app.templates, &r.template)?;
    let req = CreateSite {
        domain,
        kind: template.manifest.kind,
        template: r.template.clone(),
        title: r.title.trim().to_string(),
        admin_email: r.admin_email.unwrap_or_else(|| user.email.clone()),
    };
    let (ctx, auth, jobs, uid) = (app.ctx.clone(), app.auth.clone(), app.jobs.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.create", None, Some(user.id), move |mut log| async move {
            let site = flows::create_site(&ctx, &req, &mut log).await?;
            jobs.set_site(log.job_id, site.id).await?;
            auth.audit(Some(uid), "site.create", Some(site.id), req.domain.as_str()).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

async fn get_site<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Read).await?;
    let site = app.ctx.store.site(id).await?;
    let domains: Vec<_> = app.ctx.store.domains(id).await?.into_iter().map(|(d, k)| json!({ "hostname": d.as_str(), "kind": k })).collect();
    let can_manage = app.auth.can(&user, id, Need::Manage).await?;
    Ok(Json(json!({ "site": site, "domains": domains, "can_manage": can_manage })).into_response())
}

async fn delete_site<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let site = app.ctx.store.site(id).await?;
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.delete", Some(id), Some(user.id), move |mut log| async move {
            flows::delete_site(&ctx, id, &mut log).await?;
            auth.audit(Some(uid), "site.delete", Some(id), site.slug.as_str()).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

async fn site_audit<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    Ok(Json(app.auth.audit_for_site(id, 200).await?).into_response())
}

/// File manager: serve il ruolo di gestore. Le operazioni sui file sono sincrone: girano fuori dal runtime.
async fn site_files<H: Host>(app: &App<H>, user: &User, id: i64) -> ApiResult<SiteFiles> {
    require_site(app, user, id, Need::Manage).await?;
    let site = app.ctx.store.site(id).await?;
    let (paths, uid) = (app.paths.clone(), app.chown.then(|| site.uid()));
    Ok(tokio::task::spawn_blocking(move || SiteFiles::open(&paths, &site.slug, uid)).await.map_err(internal)??)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, FileError> + Send + 'static) -> ApiResult<T> {
    Ok(tokio::task::spawn_blocking(f).await.map_err(internal)??)
}

#[derive(Deserialize)]
struct PathQuery {
    #[serde(default)]
    path: String,
}

async fn list_files<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, Query(q): Query<PathQuery>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    Ok(Json(blocking(move || files.list(&q.path)).await?).into_response())
}

async fn read_file<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, Query(q): Query<PathQuery>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    let content = blocking(move || files.read_text(&q.path)).await?;
    Ok(Json(json!({ "content": content })).into_response())
}

async fn write_file<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, Query(q): Query<PathQuery>, m: Method, h: HeaderMap, body: axum::body::Bytes) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    let path = q.path.clone();
    blocking(move || files.write(&path, &body)).await?;
    app.auth.audit(Some(user.id), "file.write", Some(id), &q.path).await?;
    Ok(Json(json!({})).into_response())
}

async fn download_file<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, Query(q): Query<PathQuery>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    let path = q.path.clone();
    let file = blocking(move || files.open_read(&path)).await?;
    let name = std::path::Path::new(&q.path).file_name().map(|n| n.to_string_lossy().replace('"', "")).unwrap_or_default();
    let stream = tokio_util::io::ReaderStream::new(tokio::fs::File::from_std(file));
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum FileOp {
    Mkdir { path: String },
    Rename { path: String, to: String },
    Delete { path: String },
    Chmod { path: String, mode: u32 },
}

async fn file_op<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(op): Json<FileOp>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    let (action, detail) = match &op {
        FileOp::Mkdir { path } => ("file.mkdir", path.clone()),
        FileOp::Rename { path, to } => ("file.rename", format!("{path} → {to}")),
        FileOp::Delete { path } => ("file.delete", path.clone()),
        FileOp::Chmod { path, mode } => ("file.chmod", format!("{path} {mode:o}")),
    };
    blocking(move || match op {
        FileOp::Mkdir { path } => files.mkdir(&path),
        FileOp::Rename { path, to } => files.rename(&path, &to),
        FileOp::Delete { path } => files.delete(&path, now()),
        FileOp::Chmod { path, mode } => files.chmod(&path, mode),
    })
    .await?;
    app.auth.audit(Some(user.id), action, Some(id), &detail).await?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct UploadReq {
    path: String,
    size: u64,
}

async fn upload_start<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<UploadReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    let path = r.path.clone();
    let info = blocking(move || files.upload_start(&path, r.size)).await?;
    app.auth.audit(Some(user.id), "file.upload", Some(id), &r.path).await?;
    Ok((StatusCode::CREATED, Json(info)).into_response())
}

async fn upload_status<H: Host>(State(app): State<App<H>>, Path((id, upload)): Path<(i64, String)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    Ok(Json(blocking(move || files.upload_status(&upload)).await?).into_response())
}

async fn upload_chunk<H: Host>(State(app): State<App<H>>, Path((id, upload, n)): Path<(i64, String, u64)>, m: Method, h: HeaderMap, body: axum::body::Bytes) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    // Un blocco alla volta: l'ultimo sposta il file, due "ultimi" in parallelo si pesterebbero.
    let _turn = app.uploads.lock().await;
    Ok(Json(blocking(move || files.upload_chunk(&upload, n, &body)).await?).into_response())
}

async fn get_job<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let job = app.jobs.get(id).await?;
    let allowed = match job.site_id {
        _ if user.is_admin => true,
        Some(site) => app.auth.can(&user, site, Need::Read).await?,
        None => false,
    };
    if !allowed {
        return Err(ApiError::forbidden());
    }
    Ok(Json(job).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{totp, Role};
    use crate::secrets::SecretKey;
    use crate::store::Store;
    use crate::testing::FakeHost;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct T {
        app: App<FakeHost>,
        _templates: tempfile::TempDir,
    }

    async fn setup() -> T {
        let store = Store::open_memory().await.unwrap();
        let key = SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap();
        let templates = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(templates.path().join("wordpress")).unwrap();
        std::fs::write(templates.path().join("wordpress/template.toml"), "name = \"WordPress\"\nkind = \"wordpress\"\n").unwrap();
        let ctx = Ctx { store: store.clone(), host: FakeHost::default(), key: key.clone(), db_host: "mariadb".into(), global_env: Default::default() };
        let paths = crate::host::Paths::under(&templates.path().join("data"));
        let app = App::new(ctx, Auth::new(store.pool().clone(), key), templates.path().to_path_buf(), false).with_files(paths, false);
        T { app, _templates: templates }
    }

    async fn call(t: &T, req: Request<Body>) -> (StatusCode, serde_json::Value, axum::http::HeaderMap) {
        let resp = router(t.app.clone()).oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null), headers)
    }

    fn req(method: &str, uri: &str, auth: Option<&str>, body: Option<serde_json::Value>) -> Request<Body> {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(tok) = auth {
            b = b.header("authorization", format!("Bearer {tok}"));
        }
        match body {
            Some(v) => b.header("content-type", "application/json").body(Body::from(v.to_string())).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        }
    }

    async fn token_for(t: &T, email: &str, admin: bool) -> (i64, String) {
        let (id, _) = t.app.auth.create_user(email, "Prova", admin).await.unwrap();
        (id, t.app.auth.create_api_token(id, "test", i64::MAX).await.unwrap())
    }

    async fn create_site(t: &T, admin: &str, domain: &str) -> i64 {
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

    #[tokio::test]
    async fn requests_without_credentials_are_unauthorized() {
        let t = setup().await;
        let (s, body, _) = call(&t, req("GET", "/api/sites", None, None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert!(body["error"].is_string());
        let (s, _, _) = call(&t, req("GET", "/api/sites", Some("alv_falso"), None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_creates_site_as_job_and_it_gets_audited() {
        let t = setup().await;
        let (admin_id, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body["site"]["status"], "active");
        assert_eq!(body["domains"][0]["hostname"], "sona.it");
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{id}/audit"), Some(&admin), None)).await;
        assert_eq!(audit[0]["action"], "site.create");
        assert_eq!(audit[0]["user_id"], admin_id);
    }

    #[tokio::test]
    async fn invalid_domain_or_template_is_a_bad_request() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        for body in [
            serde_json::json!({"domain": "non valido", "template": "wordpress", "title": "x"}),
            serde_json::json!({"domain": "ok.it", "template": "../etc", "title": "x"}),
            serde_json::json!({"domain": "ok.it", "template": "inesistente", "title": "x"}),
        ] {
            let (s, _, _) = call(&t, req("POST", "/api/sites", Some(&admin), Some(body))).await;
            assert_eq!(s, StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn non_admin_cannot_create_or_delete_sites() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let (uid, user) = token_for(&t, "g@example.org", false).await;
        t.app.auth.assign_site(uid, id, Role::Manager).await.unwrap();
        let (s, _, _) = call(&t, req("POST", "/api/sites", Some(&user), Some(serde_json::json!({"domain": "x.it", "template": "wordpress", "title": "x"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/sites/{id}"), Some(&user), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn users_only_see_their_sites() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let a = create_site(&t, &admin, "a.it").await;
        let b = create_site(&t, &admin, "b.it").await;
        let (uid, user) = token_for(&t, "g@example.org", false).await;
        t.app.auth.assign_site(uid, a, Role::Readonly).await.unwrap();
        let (_, list, _) = call(&t, req("GET", "/api/sites", Some(&user), None)).await;
        let ids: Vec<i64> = list.as_array().unwrap().iter().map(|s| s["id"].as_i64().unwrap()).collect();
        assert_eq!(ids, [a]);
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{b}"), Some(&user), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (_, all, _) = call(&t, req("GET", "/api/sites", Some(&admin), None)).await;
        assert_eq!(all.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn admin_deletes_site_as_job() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let (s, body, _) = call(&t, req("DELETE", &format!("/api/sites/{id}"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = body["job"].as_i64().unwrap();
        for _ in 0..200 {
            if t.app.jobs.get(job).await.unwrap().status == "done" {
                let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}"), Some(&admin), None)).await;
                assert_eq!(s, StatusCode::NOT_FOUND);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("eliminazione non terminata");
    }

    async fn site_with_files(t: &T) -> (String, i64) {
        let (_, admin) = token_for(t, "admin@example.org", true).await;
        let id = create_site(t, &admin, "sona.it").await;
        // FakeHost non crea cartelle: le crea il test come farebbe l'host reale.
        crate::host::create_dirs(&t.app.paths, &alveare_core::site::Slug::parse("sona_it").unwrap(), None).unwrap();
        (admin, id)
    }

    fn raw(method: &str, uri: &str, auth: &str, body: Vec<u8>) -> Request<Body> {
        Request::builder().method(method).uri(uri).header("authorization", format!("Bearer {auth}")).body(Body::from(body)).unwrap()
    }

    #[tokio::test]
    async fn file_manager_edits_and_lists_site_files() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let (s, _, _) = call(&t, raw("PUT", &format!("/api/sites/{id}/files/content?path=public/index.php"), &admin, b"<?php echo 1;".to_vec())).await;
        assert_eq!(s, StatusCode::OK);
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/files/content?path=public/index.php"), Some(&admin), None)).await;
        assert_eq!((s, body["content"].as_str()), (StatusCode::OK, Some("<?php echo 1;")));
        let (_, list, _) = call(&t, req("GET", &format!("/api/sites/{id}/files?path=public"), Some(&admin), None)).await;
        assert_eq!(list[0]["name"], "index.php");
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/files/ops"), Some(&admin), Some(serde_json::json!({"op": "rename", "path": "public/index.php", "to": "public/home.php"})))).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/files/ops"), Some(&admin), Some(serde_json::json!({"op": "delete", "path": "public/home.php"})))).await;
        assert_eq!(s, StatusCode::OK);
        let (_, list, _) = call(&t, req("GET", &format!("/api/sites/{id}/files?path=public"), Some(&admin), None)).await;
        assert!(list.as_array().unwrap().is_empty());
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{id}/audit"), Some(&admin), None)).await;
        assert!(audit.as_array().unwrap().iter().any(|a| a["action"] == "file.delete"));
    }

    #[tokio::test]
    async fn file_manager_rejects_escapes_and_readonly_users() {
        let t = setup().await;
        let (_, id) = site_with_files(&t).await;
        let (uid, ro) = token_for(&t, "ro@example.org", false).await;
        t.app.auth.assign_site(uid, id, Role::Readonly).await.unwrap();
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}/files?path=public"), Some(&ro), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (_, admin) = token_for(&t, "admin2@example.org", true).await;
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}/files/content?path=../../etc/passwd"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn chunked_upload_over_http() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let size = crate::files::CHUNK as usize + 10;
        let (s, up, _) = call(&t, req("POST", &format!("/api/sites/{id}/files/uploads"), Some(&admin), Some(serde_json::json!({"path": "public/big.bin", "size": size})))).await;
        assert_eq!(s, StatusCode::CREATED);
        let uid = up["id"].as_str().unwrap().to_string();
        let (s, st, _) = call(&t, raw("PUT", &format!("/api/sites/{id}/files/uploads/{uid}/0"), &admin, vec![1u8; crate::files::CHUNK as usize])).await;
        assert_eq!((s, st["complete"].as_bool()), (StatusCode::OK, Some(false)));
        let (_, st, _) = call(&t, raw("PUT", &format!("/api/sites/{id}/files/uploads/{uid}/1"), &admin, vec![2u8; 10])).await;
        assert_eq!(st["complete"], true);
        let resp = router(t.app.clone()).oneshot(req("GET", &format!("/api/sites/{id}/files/download?path=public/big.bin"), Some(&admin), None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.into_body().collect().await.unwrap().to_bytes().len(), size);
    }

    fn cookie_of(headers: &axum::http::HeaderMap) -> String {
        let c = headers["set-cookie"].to_str().unwrap();
        c.split(';').next().unwrap().to_string()
    }

    fn with_cookie(method: &str, uri: &str, cookie: &str, csrf: bool, body: Option<serde_json::Value>) -> Request<Body> {
        let mut b = Request::builder().method(method).uri(uri).header("cookie", cookie).header("x-real-ip", "1.2.3.4");
        if csrf {
            b = b.header("x-alveare-csrf", "1");
        }
        match body {
            Some(v) => b.header("content-type", "application/json").body(Body::from(v.to_string())).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        }
    }

    #[tokio::test]
    async fn browser_login_flow_with_hardened_cookie() {
        let t = setup().await;
        let (_, temp) = t.app.auth.create_user("luca@example.org", "Luca", true).await.unwrap();
        let (s, _, _) = call(&t, req("POST", "/api/auth/login", None, Some(serde_json::json!({"email": "luca@example.org", "password": "sbagliata"})))).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, body, h) = call(&t, req("POST", "/api/auth/login", None, Some(serde_json::json!({"email": "luca@example.org", "password": temp})))).await;
        assert_eq!((s, body["stage"].as_str()), (StatusCode::OK, Some("setup")));
        let set_cookie = h["set-cookie"].to_str().unwrap();
        assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Strict") && set_cookie.contains("Path=/"));
        let cookie = cookie_of(&h);

        // a metà del login niente API
        let (s, _, _) = call(&t, with_cookie("GET", "/api/sites", &cookie, false, None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (_, me, _) = call(&t, with_cookie("GET", "/api/me", &cookie, false, None)).await;
        assert_eq!(me["stage"], "setup");

        let (s, setup, _) = call(&t, with_cookie("POST", "/api/auth/setup", &cookie, true, Some(serde_json::json!({"password": "una-password-lunga"})))).await;
        assert_eq!(s, StatusCode::OK);
        let secret = setup["secret"].as_str().unwrap().to_string();
        assert!(setup["otpauth_url"].as_str().unwrap().starts_with("otpauth://totp/"));
        let code = totp(&secret, "x").unwrap().generate(chrono::Utc::now().timestamp() as u64).to_string();
        let (s, _, _) = call(&t, with_cookie("POST", "/api/auth/setup/totp", &cookie, true, Some(serde_json::json!({"code": code})))).await;
        assert_eq!(s, StatusCode::OK);

        let (s, _, _) = call(&t, with_cookie("GET", "/api/sites", &cookie, false, None)).await;
        assert_eq!(s, StatusCode::OK);
        // modifiche via cookie senza header anti-CSRF: rifiutate
        let (s, _, _) = call(&t, with_cookie("POST", "/api/auth/logout", &cookie, false, None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, _) = call(&t, with_cookie("POST", "/api/auth/logout", &cookie, true, None)).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, with_cookie("GET", "/api/sites", &cookie, false, None)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_manages_users_and_assignments() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let site = create_site(&t, &admin, "sona.it").await;
        let (s, created, _) = call(&t, req("POST", "/api/users", Some(&admin), Some(serde_json::json!({"email": "n@example.org", "name": "Nuovo", "is_admin": false})))).await;
        assert_eq!(s, StatusCode::CREATED);
        assert_eq!(created["temporary_password"].as_str().unwrap().len(), 32);
        let uid = created["id"].as_i64().unwrap();
        let (s, _, _) = call(&t, req("PUT", &format!("/api/users/{uid}/sites/{site}"), Some(&admin), Some(serde_json::json!({"role": "readonly"})))).await;
        assert_eq!(s, StatusCode::OK);
        let user = t.app.auth.user(uid).await.unwrap();
        assert!(t.app.auth.can(&user, site, crate::auth::Need::Read).await.unwrap());
        let (_, other) = token_for(&t, "g@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", "/api/users", Some(&other), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }
}
