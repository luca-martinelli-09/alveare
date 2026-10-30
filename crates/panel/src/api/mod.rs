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
    pub adminer_host: Option<Domain>,
    /// Valori del kit di ripristino (chiave del pannello, repository e password di restic).
    pub recovery: Vec<(String, String)>,
    /// Hostname del pannello e di Adminer: nessun sito può usarli.
    pub reserved_hosts: Vec<Domain>,
    /// RAM del server per il budget di memoria (letta da /proc/meminfo).
    pub ram_mb: u64,
    /// Soglia di spazio per sito (MB) del monitoraggio.
    pub disk_alert_mb: u64,
    /// Email del pannello (inviti, reset) e URL pubblico per i link.
    pub mailer: crate::mailer::Mailer,
    pub panel_url: Option<String>,
    pub passkeys: Option<crate::auth::passkeys::Passkeys>,
    /// Token di Prometheus per /metrics (senza, /metrics non c'è).
    pub metrics_token: Option<String>,
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
            adminer_host: self.adminer_host.clone(),
            recovery: self.recovery.clone(),
            reserved_hosts: self.reserved_hosts.clone(),
            ram_mb: self.ram_mb,
            disk_alert_mb: self.disk_alert_mb,
            mailer: self.mailer.clone(),
            panel_url: self.panel_url.clone(),
            passkeys: self.passkeys.clone(),
            metrics_token: self.metrics_token.clone(),
        }
    }
}

impl<H: Host> App<H> {
    pub fn new(ctx: Ctx<H>, auth: Auth, templates: PathBuf, secure_cookie: bool) -> Self {
        let jobs = Jobs::new(ctx.store.pool().clone());
        Self { ctx: Arc::new(ctx), auth, jobs, templates, secure_cookie, paths: Paths::production(), chown: true, uploads: Default::default(), adminer_host: None, recovery: vec![], reserved_hosts: vec![], ram_mb: crate::budget::ram_mb(), disk_alert_mb: 10 * 1024, mailer: crate::mailer::Mailer::None, panel_url: None, passkeys: None, metrics_token: None }
    }

    pub fn with_recovery(self, recovery: Vec<(String, String)>) -> Self {
        Self { recovery, ..self }
    }

    pub fn recovery_kit(&self) -> String {
        let pairs: Vec<(&str, &str)> = self.recovery.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        crate::backup::recovery_kit(&pairs)
    }

    pub fn with_metrics_token(self, metrics_token: Option<String>) -> Self {
        Self { metrics_token, ..self }
    }

    pub fn with_passkeys(self, passkeys: Option<crate::auth::passkeys::Passkeys>) -> Self {
        Self { passkeys, ..self }
    }

    pub fn with_mailer(self, mailer: crate::mailer::Mailer, panel_url: Option<String>) -> Self {
        Self { mailer, panel_url, ..self }
    }

    /// Inviti e reset via email: servono SMTP e l'URL pubblico del pannello.
    fn email_links(&self) -> Option<&str> {
        self.panel_url.as_deref().filter(|_| self.mailer.enabled())
    }

    pub fn with_disk_alert(self, disk_alert_mb: u64) -> Self {
        Self { disk_alert_mb, ..self }
    }

    pub fn with_reserved_hosts(self, reserved_hosts: Vec<Domain>) -> Self {
        Self { reserved_hosts, ..self }
    }

    /// 409 se l'hostname è del pannello o di Adminer.
    fn check_not_reserved(&self, host: &Domain) -> ApiResult<()> {
        if self.reserved_hosts.contains(host) {
            return Err(ApiError::new(StatusCode::CONFLICT, format!("{} è riservato al pannello", host.as_str())));
        }
        Ok(())
    }

    pub fn with_adminer(self, adminer_host: Option<Domain>) -> Self {
        Self { adminer_host, ..self }
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
        match e.downcast_ref::<flows::Invalid>() {
            Some(invalid) => Self::bad_request(invalid.0.clone()),
            None => internal(e),
        }
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
        .route("/api/auth/options", get(auth_options::<H>))
        .route("/api/auth/invite", post(accept_invite::<H>))
        .route("/api/auth/reset", post(request_reset::<H>))
        .route("/api/auth/reset/confirm", post(confirm_reset::<H>))
        .route("/api/users/{id}/invite", post(resend_invite::<H>))
        .route("/api/me", get(me::<H>))
        .route("/api/me/tokens", post(create_token::<H>))
        .route("/api/users", get(list_users::<H>).post(create_user::<H>))
        .route("/api/users/{id}/block", post(block_user::<H>))
        .route("/api/users/{id}/sites", get(user_sites::<H>))
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
        .route("/api/sites/{id}/database", get(database_overview::<H>))
        .route("/api/sites/{id}/database/export", get(database_export::<H>))
        .route("/api/sites/{id}/database/import", post(database_import::<H>))
        .route("/api/sites/{id}/database/password", post(database_password::<H>))
        .route("/api/sites/{id}/database/adminer", post(database_adminer::<H>))
        .route("/api/sites/{id}/backups", get(list_backups::<H>).post(backup_now::<H>))
        .route("/api/sites/{id}/backups/{snapshot}/restore", post(restore_backup::<H>))
        .route("/api/sites/{id}/backups/{snapshot}/files", get(snapshot_files::<H>))
        .route("/api/admin/recovery-kit", get(recovery_kit::<H>))
        .route("/api/alerts", get(list_alerts::<H>))
        .route("/api/jobs/{id}", get(get_job::<H>))
        .route("/metrics", get(metrics::<H>))
        .merge(sites::routes::<H>())
        .merge(imports::routes::<H>())
        .merge(logs::routes::<H>())
        .merge(config::routes::<H>())
        .merge(blocks::routes::<H>())
        .merge(wordpress::routes::<H>())
        .merge(crons::routes::<H>())
        .merge(cloudflare::routes::<H>())
        .merge(account::routes::<H>())
        .merge(passkeys::routes::<H>())
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
    let statuses = app.auth.statuses().await?;
    let users: Vec<_> = app
        .auth
        .users()
        .await?
        .into_iter()
        .map(|u| {
            let status = statuses.get(&u.id).cloned().unwrap_or_default();
            let mut v = serde_json::to_value(u).unwrap_or_default();
            v["status"] = json!(status);
            v
        })
        .collect();
    Ok(Json(users).into_response())
}

#[derive(Deserialize)]
struct NewUserReq {
    email: String,
    name: String,
    #[serde(default)]
    is_admin: bool,
    /// Invito via email invece della password temporanea.
    #[serde(default)]
    invite: bool,
}

async fn create_user<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<NewUserReq>) -> ApiResult<Response> {
    let admin = caller(&app, &m, &h).await?;
    require_admin(&admin)?;
    let email = r.email.trim();
    if !email.contains('@') || email.len() > 254 || r.name.trim().is_empty() {
        return Err(ApiError::bad_request("email o nome non validi"));
    }
    let dup = |e: AuthError| match e {
        AuthError::Db(sqlx::Error::Database(d)) if d.is_unique_violation() => ApiError::new(StatusCode::CONFLICT, "email già registrata"),
        e => e.into(),
    };
    if r.invite {
        let base = app.email_links().ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "inviti via email non configurati (SMTP del pannello e ALVEARE_PANEL_HOST)"))?;
        let (id, token) = app.auth.create_invited_user(email, r.name.trim(), r.is_admin, now()).await.map_err(dup)?;
        if let Err(e) = send_invite(&app, base, email, &token).await {
            app.auth.delete_user(id).await?;
            return Err(e);
        }
        app.auth.audit(Some(admin.id), "user.invite", None, email).await?;
        return Ok((StatusCode::CREATED, Json(json!({ "id": id, "invited": true }))).into_response());
    }
    let (id, temp) = app.auth.create_user(email, r.name.trim(), r.is_admin).await.map_err(dup)?;
    app.auth.audit(Some(admin.id), "user.create", None, email).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "temporary_password": temp }))).into_response())
}

async fn send_invite<H>(app: &App<H>, base: &str, email: &str, token: &str) -> ApiResult<()> {
    let body = format!(
        "Ciao,\n\nsei stato invitato nel pannello Alveare. Apri questo link entro 72 ore per scegliere la password e attivare il secondo fattore (TOTP):\n\n{base}/invito#{token}\n\nSe non aspettavi questo invito, ignora l'email.\n"
    );
    app.mailer.send(email, "Invito al pannello Alveare", &body).await.map_err(|e| {
        tracing::error!("invio dell'invito a {email}: {e:#}");
        ApiError::new(StatusCode::BAD_GATEWAY, "invio dell'email non riuscito")
    })
}

async fn resend_invite<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let admin = caller(&app, &m, &h).await?;
    require_admin(&admin)?;
    let base = app.email_links().ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "inviti via email non configurati"))?.to_string();
    let (email, token) = app.auth.reinvite(id, now()).await.map_err(|e| match e {
        AuthError::WrongStage => ApiError::new(StatusCode::CONFLICT, "l'utente ha già accettato l'invito"),
        e => e.into(),
    })?;
    send_invite(&app, &base, &email, &token).await?;
    app.auth.audit(Some(admin.id), "user.reinvite", None, &email).await?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct TokenOnly {
    token: String,
}

/// Link d'invito: sessione di primo accesso, poi i soliti passi (password e TOTP).
async fn accept_invite<H: Host>(State(app): State<App<H>>, h: HeaderMap, Json(r): Json<TokenOnly>) -> ApiResult<Response> {
    let ua = h.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = app.auth.accept_invite(&r.token, &client_ip(&h), ua, now()).await.map_err(|_| ApiError::new(StatusCode::GONE, "invito non valido o scaduto"))?;
    Ok(([(header::SET_COOKIE, session_cookie(&token, app.secure_cookie))], Json(json!({ "stage": Stage::Setup }))).into_response())
}

#[derive(Deserialize)]
struct ResetReq {
    email: String,
}

/// Risponde sempre allo stesso modo: non dice se l'email esiste.
async fn request_reset<H: Host>(State(app): State<App<H>>, Json(r): Json<ResetReq>) -> ApiResult<Response> {
    let base = app.email_links().ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "reset via email non configurato: chiedi a un admin"))?.to_string();
    if let Some((email, token)) = app.auth.reset_link(&r.email, now()).await? {
        let body = format!(
            "Ciao,\n\nper scegliere una nuova password del pannello Alveare apri questo link entro un'ora. Ti servirà anche il codice dell'app TOTP.\n\n{base}/reset#{token}\n\nSe non hai chiesto tu il reset, ignora l'email: la password resta quella di prima.\n"
        );
        if let Err(e) = app.mailer.send(&email, "Nuova password del pannello Alveare", &body).await {
            tracing::error!("invio del reset a {email}: {e:#}");
        }
        app.auth.audit(None, "user.reset_requested", None, &email).await?;
    }
    Ok(Json(json!({ "sent": true })).into_response())
}

#[derive(Deserialize)]
struct ResetConfirm {
    token: String,
    password: String,
    #[serde(default)]
    code: String,
}

async fn confirm_reset<H: Host>(State(app): State<App<H>>, h: HeaderMap, Json(r): Json<ResetConfirm>) -> ApiResult<Response> {
    let ua = h.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let (token, stage) = app.auth.reset_password(&r.token, &r.password, &r.code, &client_ip(&h), ua, now()).await?;
    Ok(([(header::SET_COOKIE, session_cookie(&token, app.secure_cookie))], Json(json!({ "stage": stage }))).into_response())
}

async fn auth_options<H: Host>(State(app): State<App<H>>) -> ApiResult<Response> {
    Ok(Json(json!({ "email": app.email_links().is_some(), "passkeys": app.passkeys.is_some() })).into_response())
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

async fn user_sites<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    let rows: Vec<_> = app.auth.user_sites(id).await?.into_iter().map(|(site_id, role)| json!({ "site_id": site_id, "role": role })).collect();
    Ok(Json(rows).into_response())
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
            out.push(json!({ "id": name, "name": t.manifest.name, "kind": t.manifest.kind, "snapshot": t.manifest.sql.is_some() }));
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
            let uptime: Option<serde_json::Value> = app.ctx.store.setting(&format!("uptime:{}", site.id)).await?.and_then(|v| serde_json::from_str(&v).ok());
            let online = uptime.as_ref().and_then(|u| u["ok"].as_bool());
            out.push(json!({ "id": site.id, "slug": site.slug, "domain": primary, "kind": site.kind, "status": site.status, "online": online, "staging": site.staging }));
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
    /// Conferma esplicita se il budget di memoria supera il 150% della RAM.
    #[serde(default)]
    confirm_budget: bool,
}

async fn create_site<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<CreateSiteReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let domain = Domain::parse(&r.domain).map_err(|_| ApiError::bad_request("dominio non valido"))?;
    app.check_not_reserved(&domain)?;
    let template = load_template(&app.templates, &r.template)?;
    let req = CreateSite {
        domain,
        kind: template.manifest.kind,
        template: r.template.clone(),
        title: r.title.trim().to_string(),
        admin_email: r.admin_email.unwrap_or_else(|| user.email.clone()),
        table_prefix: template.manifest.table_prefix.clone().unwrap_or_else(|| "wp_".into()),
    };
    let budget = flows::budget_with(&app.ctx, 0, Some(&crate::store::PhpSettingsRow::default()), app.ram_mb).await?;
    if budget.needs_confirmation() && !r.confirm_budget {
        return Err(ApiError::new(StatusCode::CONFLICT, format!("con il nuovo sito i pool arriverebbero al {}% della RAM: conferma per creare", budget.percent)));
    }
    let (ctx, auth, jobs, uid, ram_mb) = (app.ctx.clone(), app.auth.clone(), app.jobs.clone(), user.id, app.ram_mb);
    let job = app
        .jobs
        .spawn("site.create", None, Some(user.id), move |mut log| async move {
            if budget.over_ram() {
                log.push(format!("ATTENZIONE: il massimo teorico dei pool sarà il {}% della RAM", budget.percent));
            }
            let site = flows::create_site(&ctx, &req, &mut log).await?;
            jobs.set_site(log.job_id, site.id).await?;
            flows::budget_alert(&ctx, ram_mb).await?;
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
    let staging = if can_manage { sites::staging_json(flows::staging_password(&app.ctx, id).await?) } else { serde_json::Value::Null };
    let wp_admin_pending = user.is_admin && app.ctx.store.setting(&format!("wp_admin:{id}")).await?.is_some();
    Ok(Json(json!({ "site": site, "domains": domains, "can_manage": can_manage, "staging": staging, "wp_admin_pending": wp_admin_pending })).into_response())
}

async fn delete_site<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let site = app.ctx.store.site(id).await?;
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.delete", Some(id), Some(user.id), move |mut log| async move {
            flows::delete_site(&ctx, id, flows::DeleteMode::Normal, &mut log).await?;
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
    // Una cartella si scarica come zip generato al momento.
    let (file, zipped) = blocking(move || if files.is_dir(&path)? { files.zip_for_download(&path).map(|f| (f, true)) } else { files.open_read(&path).map(|f| (f, false)) })
    .await?;
    let base = std::path::Path::new(&q.path).file_name().map(|n| n.to_string_lossy().replace('"', "")).unwrap_or_else(|| "sito".into());
    let name = if zipped { format!("{base}.zip") } else { base };
    if zipped {
        app.auth.audit(Some(user.id), "file.download_zip", Some(id), &q.path).await?;
    }
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
    /// Estrae un archivio del sito nella cartella `to`.
    Extract { path: String, to: String },
    /// Crea l'archivio `to` (.zip o .tar.gz) con `paths`.
    Compress { paths: Vec<String>, to: String },
}

async fn file_op<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(op): Json<FileOp>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    let (action, detail) = match &op {
        FileOp::Mkdir { path } => ("file.mkdir", path.clone()),
        FileOp::Rename { path, to } => ("file.rename", format!("{path} → {to}")),
        FileOp::Delete { path } => ("file.delete", path.clone()),
        FileOp::Chmod { path, mode } => ("file.chmod", format!("{path} {mode:o}")),
        FileOp::Extract { path, to } => ("file.extract", format!("{path} → {to}")),
        FileOp::Compress { paths, to } => ("file.compress", format!("{} → {to}", paths.join(", "))),
    };
    blocking(move || match op {
        FileOp::Mkdir { path } => files.mkdir(&path),
        FileOp::Rename { path, to } => files.rename(&path, &to),
        FileOp::Delete { path } => files.delete(&path, now()),
        FileOp::Chmod { path, mode } => files.chmod(&path, mode),
        FileOp::Extract { path, to } => files.extract(&path, &to).map(drop),
        FileOp::Compress { paths, to } => files.compress(&paths, &to),
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

async fn database_overview<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let db = app.ctx.db_creds(id).await?;
    let tables = app.ctx.host.db_tables(&db.name).await?;
    let total: u64 = tables.iter().map(|t| t.bytes).sum();
    Ok(Json(json!({ "name": db.name, "user": db.user, "host": app.ctx.db_host, "total_bytes": total, "tables": tables })).into_response())
}

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default)]
    tables: String,
}

async fn database_export<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, Query(q): Query<ExportQuery>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    use tokio_stream::StreamExt;
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let tables: Vec<String> = q.tables.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    if !tables.iter().all(|t| crate::database::valid_table(t)) {
        return Err(ApiError::bad_request("nome di tabella non valido"));
    }
    let site = app.ctx.store.site(id).await?;
    let db = app.ctx.db_creds(id).await?;
    let dump = app.ctx.host.db_dump(&db.name, &tables).await?;
    app.auth.audit(Some(user.id), "database.export", Some(id), &q.tables).await?;
    let flows::Dump { reader, finished } = dump;
    let gz = async_compression::tokio::bufread::GzipEncoder::new(tokio::io::BufReader::new(reader));
    // Se mariadb-dump fallisce a metà, lo stream finisce con un errore: il download risulta interrotto.
    let tail = futures_util::stream::once(async move {
        finished.await.map(|_| axum::body::Bytes::new()).map_err(|e| std::io::Error::other(e.to_string()))
    });
    let stream = tokio_util::io::ReaderStream::new(gz).chain(tail);
    let name = format!("{}-{}.sql.gz", site.slug.as_str(), chrono::Local::now().format("%Y%m%d-%H%M"));
    Ok((
        [(header::CONTENT_TYPE, "application/gzip".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}

#[derive(Deserialize)]
struct SearchReplace {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct ImportReq {
    /// Percorso di un dump già caricato nella cartella del sito (.sql o .sql.gz).
    path: String,
    search_replace: Option<SearchReplace>,
}

async fn database_import<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<ImportReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = site_files(&app, &user, id).await?;
    let gz = crate::database::dump_kind(&r.path).ok_or_else(|| ApiError::bad_request("serve un file .sql o .sql.gz"))?;
    let path = r.path.clone();
    let file = blocking(move || files.open_read(&path)).await?;
    let site = app.ctx.store.site(id).await?;
    let safety = app.paths.tmp.join(site.slug.as_str()).join(format!("db-before-import-{}.sql.gz", now()));
    let sr = r.search_replace.map(|s| (s.from, s.to));
    let (ctx, auth, uid, detail) = (app.ctx.clone(), app.auth.clone(), user.id, r.path.clone());
    let job = app
        .jobs
        .spawn("database.import", Some(id), Some(user.id), move |mut log| async move {
            let reader = tokio::io::BufReader::new(tokio::fs::File::from_std(file));
            let sql: std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>> =
                if gz { Box::pin(async_compression::tokio::bufread::GzipDecoder::new(reader)) } else { Box::pin(reader) };
            flows::import_database(&ctx, id, sql, &safety, sr, &mut log).await?;
            auth.audit(Some(uid), "database.import", Some(id), &detail).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

async fn database_password<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    flows::reset_db_password(&app.ctx, id).await?;
    app.auth.audit(Some(user.id), "database.password", Some(id), "").await?;
    Ok(Json(json!({})).into_response())
}

async fn database_adminer<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let host = app.adminer_host.clone().ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Adminer non configurato (ALVEARE_ADMINER_HOST)"))?;
    let db = app.ctx.db_creds(id).await?;
    let token = app.ctx.host.adminer_grant(&db).await?;
    app.auth.audit(Some(user.id), "database.adminer", Some(id), "").await?;
    Ok(Json(json!({ "url": format!("https://{}/?alveare={token}", host.as_str()) })).into_response())
}

async fn list_backups<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    let runs = app.ctx.store.backup_runs(id, 100).await?;
    let restore_test: Option<serde_json::Value> = app.ctx.store.setting(&format!("restore_test:{id}")).await?.and_then(|v| serde_json::from_str(&v).ok());
    Ok(Json(json!({ "enabled": app.ctx.backups, "runs": runs, "restore_test": restore_test })).into_response())
}

async fn backup_now<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    if !app.ctx.backups {
        return Err(ApiError::new(StatusCode::CONFLICT, "backup non configurati (RESTIC_REPOSITORY)"));
    }
    app.ctx.store.site(id).await?;
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.backup", Some(id), Some(user.id), move |mut log| async move {
            let snap = flows::backup_site(&ctx, id, &mut log).await?;
            auth.audit(Some(uid), "backup.run", Some(id), &snap.snapshot_id).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum RestoreReq {
    Full,
    Files,
    Database,
    /// Un file o una cartella scelti dalla vista dello snapshot.
    Path { path: String },
    /// Lo snapshot come sito nuovo (solo admin): verifica senza toccare l'originale.
    NewDomain { domain: String },
}

/// Solo snapshot di questo sito: l'id arriva dallo storico del sito, non da restic.
async fn known_snapshot<H>(app: &App<H>, id: i64, snapshot: &str) -> ApiResult<()> {
    if !crate::backup::valid_snapshot(snapshot) {
        return Err(ApiError::bad_request("snapshot non valido"));
    }
    let known = app.ctx.store.backup_runs(id, 10_000).await?.into_iter().any(|run| run.snapshot_id.as_deref().is_some_and(|s| s.starts_with(snapshot)));
    if !known {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "snapshot non trovato per questo sito"));
    }
    Ok(())
}

async fn restore_backup<H: Host>(State(app): State<App<H>>, Path((id, snapshot)): Path<(i64, String)>, m: Method, h: HeaderMap, Json(r): Json<RestoreReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    known_snapshot(&app, id, &snapshot).await?;
    let new_domain = match &r {
        RestoreReq::NewDomain { domain } => {
            require_admin(&user)?;
            Some(sites::free_domain(&app, domain).await?)
        }
        RestoreReq::Path { path } => {
            flows::site_rel_path(path).map_err(|e| ApiError::bad_request(e.to_string()))?;
            None
        }
        _ => None,
    };
    let (ctx, auth, jobs, uid) = (app.ctx.clone(), app.auth.clone(), app.jobs.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.restore", Some(id), Some(user.id), move |mut log| async move {
            let detail = match (r, new_domain) {
                (RestoreReq::Path { path }, _) => {
                    flows::restore_path(&ctx, id, &snapshot, &path, &mut log).await?;
                    format!("{snapshot}: {path}")
                }
                (RestoreReq::NewDomain { .. }, Some(domain)) => {
                    let site = flows::restore_as_new_site(&ctx, id, &snapshot, &domain, &mut log).await?;
                    jobs.set_site(log.job_id, site.id).await?;
                    format!("{snapshot} → {}", domain.as_str())
                }
                (mode, _) => {
                    let mode = match mode {
                        RestoreReq::Files => flows::RestoreMode::Files,
                        RestoreReq::Database => flows::RestoreMode::Database,
                        _ => flows::RestoreMode::Full,
                    };
                    flows::restore_site(&ctx, id, &snapshot, mode, &mut log).await?;
                    snapshot.clone()
                }
            };
            auth.audit(Some(uid), "backup.restore", Some(id), &detail).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

async fn snapshot_files<H: Host>(State(app): State<App<H>>, Path((id, snapshot)): Path<(i64, String)>, Query(q): Query<PathQuery>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    known_snapshot(&app, id, &snapshot).await?;
    if !q.path.trim_matches('/').is_empty() {
        flows::site_rel_path(&q.path).map_err(|e| ApiError::bad_request(e.to_string()))?;
    }
    let site = app.ctx.store.site(id).await?;
    Ok(Json(app.ctx.host.snapshot_ls(&site.slug, &snapshot, &q.path).await?).into_response())
}

/// Il kit contiene le chiavi in chiaro: solo admin, sempre nell'audit log.
async fn recovery_kit<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let kit = app.recovery_kit();
    app.ctx.store.set_setting("recovery_kit_fingerprint", &crate::backup::kit_fingerprint(&kit)).await?;
    app.ctx.store.resolve_global_alerts("recovery-kit").await?;
    app.auth.audit(Some(user.id), "recovery_kit.download", None, "").await?;
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"alveare-kit-ripristino.txt\"")],
        kit,
    )
        .into_response())
}

async fn list_alerts<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let visible = app.auth.visible_sites(&user).await?;
    Ok(Json(app.ctx.store.open_alerts(visible.as_deref()).await?).into_response())
}

/// Prometheus: metriche di Caddy e del pannello, con il token di ALVEARE_METRICS_TOKEN.
async fn metrics<H: Host>(State(app): State<App<H>>, h: HeaderMap) -> ApiResult<Response> {
    let expected = app.metrics_token.as_deref().ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "metriche non attive (ALVEARE_METRICS_TOKEN)"))?;
    if !crate::metrics::token_ok(expected, h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())) {
        return Err(ApiError::unauthorized());
    }
    let setting = |key: String| {
        let store = app.ctx.store.clone();
        async move { store.setting(&key).await.ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or(serde_json::Value::Null) }
    };
    let mut sites = Vec::new();
    for s in app.ctx.store.sites().await? {
        let Some((domain, _)) = app.ctx.store.domains(s.id).await?.into_iter().next() else { continue };
        let last_backup = app.ctx.store.backup_runs(s.id, 20).await?.into_iter().find(|r| r.status == "done").and_then(|r| r.finished_at).and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok()).map(|t| t.timestamp());
        sites.push(crate::metrics::SiteMetrics {
            domain: domain.as_str().to_string(),
            uptime: setting(format!("uptime:{}", s.id)).await,
            usage: setting(format!("usage:{}", s.id)).await,
            traffic: setting(format!("traffic:{}", s.id)).await,
            updates: setting(format!("updates:{}", s.id)).await,
            last_backup,
        });
    }
    let alerts = app.ctx.store.open_alerts(None).await?.len();
    let caddy = app.ctx.host.caddy_metrics().await.unwrap_or_else(|e| format!("# Caddy non raggiungibile: {e}\n"));
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], format!("{}{caddy}", crate::metrics::render(&sites, alerts))).into_response())
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

mod account;
mod blocks;
mod cloudflare;
mod config;
mod crons;
mod imports;
mod logs;
mod passkeys;
mod sites;
mod wordpress;
pub(crate) use wordpress::remember as remember_updates;

#[cfg(test)]
pub(crate) mod testkit;

#[cfg(test)]
mod tests {
    use super::testkit::{call, cookie_of, create_site, raw, req, setup, site_with_files, token_for, wait_job, with_cookie};
    use super::*;
    use crate::auth::{totp, Role};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

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
    async fn file_manager_compresses_extracts_and_zips_folders() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let ops = format!("/api/sites/{id}/files/ops");
        call(&t, raw("PUT", &format!("/api/sites/{id}/files/content?path=public/tema/style.css"), &admin, b"a{}".to_vec())).await;
        let (s, _, _) = call(&t, req("POST", &ops, Some(&admin), Some(serde_json::json!({"op": "compress", "paths": ["public/tema"], "to": "public/tema.zip"})))).await;
        assert_eq!(s, StatusCode::OK);
        let (s, _, _) = call(&t, req("POST", &ops, Some(&admin), Some(serde_json::json!({"op": "extract", "path": "public/tema.zip", "to": "public/copia"})))).await;
        assert_eq!(s, StatusCode::OK);
        let (_, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/files/content?path=public/copia/tema/style.css"), Some(&admin), None)).await;
        assert_eq!(body["content"], "a{}");
        let (s, _, h) = call(&t, req("GET", &format!("/api/sites/{id}/files/download?path=public/tema"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert!(h["content-disposition"].to_str().unwrap().contains("tema.zip"));
        let (s, _, _) = call(&t, req("POST", &ops, Some(&admin), Some(serde_json::json!({"op": "extract", "path": "public/tema/style.css", "to": "public"})))).await;
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

    #[tokio::test]
    async fn logs_are_readable_by_readonly_users_and_access_is_parsed() {
        let t = setup().await;
        let (_, id) = site_with_files(&t).await;
        let logs = t.app.paths.logs.join("sona_it");
        std::fs::write(logs.join("access.log"), "{\"ts\":1.0,\"request\":{\"client_ip\":\"198.51.100.9\",\"method\":\"GET\",\"uri\":\"/\"},\"status\":200}\nrotta\n").unwrap();
        std::fs::write(logs.join("php-error.log"), "PHP Warning: x\n").unwrap();
        let (uid, ro) = token_for(&t, "ro@example.org", false).await;
        t.app.auth.assign_site(uid, id, Role::Readonly).await.unwrap();
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/access"), Some(&ro), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body["entries"].as_array().unwrap().len(), 1);
        assert_eq!(body["entries"][0]["ip"], "198.51.100.9");
        let (_, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/php-error"), Some(&ro), None)).await;
        assert_eq!(body["lines"][0], "PHP Warning: x");
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/sites/{id}/logs/wp-debug"), Some(&ro), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/sconosciuto"), Some(&ro), None)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn wp_debug_log_symlink_outside_site_is_not_read() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let wpc = t.app.paths.sites.join("sona_it/public/wp-content");
        std::fs::create_dir_all(&wpc).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", wpc.join("debug.log")).unwrap();
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/wp-debug"), Some(&admin), None)).await;
        assert_ne!(s, StatusCode::OK, "{body}");
        assert!(!body.to_string().contains("root:"));
    }

    #[tokio::test]
    async fn alerts_are_filtered_by_visible_sites() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let a = create_site(&t, &admin, "a.it").await;
        let b = create_site(&t, &admin, "b.it").await;
        t.app.ctx.store.raise_alert(Some(a), "integrity", "x").await.unwrap();
        t.app.ctx.store.raise_alert(Some(b), "integrity", "y").await.unwrap();
        let (uid, user) = token_for(&t, "g@example.org", false).await;
        t.app.auth.assign_site(uid, a, Role::Readonly).await.unwrap();
        let (_, list, _) = call(&t, req("GET", "/api/alerts", Some(&user), None)).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["site_id"], a);
        let (_, all, _) = call(&t, req("GET", "/api/alerts", Some(&admin), None)).await;
        assert_eq!(all.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn database_overview_and_compressed_export() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let (uid, ro) = token_for(&t, "ro@example.org", false).await;
        t.app.auth.assign_site(uid, id, Role::Readonly).await.unwrap();
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}/database"), Some(&ro), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/database"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body["tables"][0]["name"], "wp_posts");
        assert_eq!(body["total_bytes"], 4096);

        let resp = router(t.app.clone()).oneshot(req("GET", &format!("/api/sites/{id}/database/export?tables=wp_posts"), Some(&admin), None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers()["content-disposition"].to_str().unwrap().contains("sona_it"));
        let gz = resp.into_body().collect().await.unwrap().to_bytes();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&gz[..]), &mut text).unwrap();
        assert_eq!(text, "-- dump finto\n");
        assert!(t.app.ctx.host.calls().contains(&"db_dump sona_it wp_posts".to_string()));
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}/database/export?tables=a;DROP"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn database_password_reset_updates_db_store_and_env() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let before = t.app.ctx.db_creds(id).await.unwrap().password;
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/database/password"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        let after = t.app.ctx.db_creds(id).await.unwrap().password;
        assert_ne!(before, after);
        let env = t.app.ctx.host.env.lock().unwrap().clone();
        assert_eq!(env["DB_PASSWORD"], after);
        assert_eq!(env["SITE_ENV DB_PASSWORD"], after);
    }

    #[tokio::test]
    async fn database_import_runs_as_job_with_safety_dump() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let files = crate::files::SiteFiles::open(&t.app.paths, &alveare_core::site::Slug::parse("sona_it").unwrap(), None).unwrap();
        files.write("dump.sql", b"INSERT INTO x VALUES (1);").unwrap();
        let body = serde_json::json!({"path": "dump.sql", "search_replace": {"from": "vecchio.it", "to": "sona.it"}});
        let (s, resp, _) = call(&t, req("POST", &format!("/api/sites/{id}/database/import"), Some(&admin), Some(body))).await;
        assert_eq!(s, StatusCode::ACCEPTED, "{resp}");
        let job = wait_job(&t, resp["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        let calls = t.app.ctx.host.calls();
        let pos = |p: &str| calls.iter().position(|c| c.starts_with(p)).unwrap_or_else(|| panic!("manca {p}: {calls:?}"));
        assert!(pos("db_dump sona_it") < pos("db_import sona_it INSERT INTO x VALUES (1);"));
        assert!(pos("db_import") < pos("wp sona_it search-replace vecchio.it sona.it --all-tables"));
        assert!(pos("wp sona_it search-replace") < pos("wp sona_it cache flush"), "la cache va svuotata dopo l'import");
        let safety: Vec<_> = std::fs::read_dir(t.app.paths.tmp.join("sona_it")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("db-before-import-")).collect();
        assert_eq!(safety.len(), 1);
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/database/import"), Some(&admin), Some(serde_json::json!({"path": "foto.jpg"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn adminer_link_needs_configuration_and_manager_role() {
        let mut t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/database/adminer"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        t.app = t.app.clone().with_adminer(Some(Domain::parse("adminer.example.org").unwrap()));
        let (s, body, _) = call(&t, req("POST", &format!("/api/sites/{id}/database/adminer"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(body["url"], format!("https://adminer.example.org/?alveare={}", "t".repeat(40)));
        let (uid, ro) = token_for(&t, "ro@example.org", false).await;
        t.app.auth.assign_site(uid, id, Role::Readonly).await.unwrap();
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/database/adminer"), Some(&ro), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn backups_run_as_jobs_and_restore_only_known_snapshots() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let (s, resp, _) = call(&t, req("POST", &format!("/api/sites/{id}/backups"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        assert_eq!(wait_job(&t, resp["job"].as_i64().unwrap()).await.status, "done");
        let (_, list, _) = call(&t, req("GET", &format!("/api/sites/{id}/backups"), Some(&admin), None)).await;
        assert_eq!(list["runs"][0]["snapshot_id"], "abcdef12");
        let (s, _, _) = call(&t, req("POST", &format!("/api/sites/{id}/backups/deadbeef/restore"), Some(&admin), Some(serde_json::json!({"mode": "full"})))).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        let (s, resp, _) = call(&t, req("POST", &format!("/api/sites/{id}/backups/abcdef12/restore"), Some(&admin), Some(serde_json::json!({"mode": "files"})))).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        assert_eq!(wait_job(&t, resp["job"].as_i64().unwrap()).await.status, "done");
        assert!(t.app.ctx.host.calls().iter().any(|c| c == "restore sona_it abcdef12 -> sona_it files=all db=-"));
    }

    #[tokio::test]
    async fn recovery_kit_is_admin_only_and_resolves_the_reminder() {
        let mut t = setup().await;
        t.app = t.app.clone().with_recovery(vec![("RESTIC_PASSWORD".into(), "segreta".into())]);
        t.app.ctx.store.raise_alert(None, "recovery-kit", "scarica il kit").await.unwrap();
        let (_, user) = token_for(&t, "g@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", "/api/admin/recovery-kit", Some(&user), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let resp = router(t.app.clone()).oneshot(req("GET", "/api/admin/recovery-kit", Some(&admin), None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let text = String::from_utf8(resp.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
        assert!(text.contains("RESTIC_PASSWORD=segreta"));
        assert!(t.app.ctx.store.open_alerts(None).await.unwrap().is_empty());
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
        let (_, sites, _) = call(&t, req("GET", &format!("/api/users/{uid}/sites"), Some(&admin), None)).await;
        assert_eq!(sites, serde_json::json!([{"site_id": site, "role": "readonly"}]));
        let (_, other) = token_for(&t, "g@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", "/api/users", Some(&other), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }
}
