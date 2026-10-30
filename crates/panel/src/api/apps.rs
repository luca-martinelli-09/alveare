//! App (Laravel e simili): worker long-running e deploy da git.
use axum::routing::patch;

use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}/workers", get(list::<H>).post(create::<H>))
        .route("/api/sites/{id}/workers/{wid}", patch(update::<H>).delete(remove::<H>))
        .route("/api/sites/{id}/workers/{wid}/restart", post(restart::<H>))
        .route("/api/sites/{id}/deploy", get(deploy_config::<H>).put(set_deploy::<H>).post(deploy::<H>))
        .route("/api/sites/{id}/deploy/key", post(deploy_key::<H>))
}

async fn site_worker<H>(app: &App<H>, id: i64, wid: i64) -> ApiResult<crate::store::Worker> {
    app.ctx.store.workers(Some(id)).await?.into_iter().find(|w| w.id == wid).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "worker non trovato"))
}

async fn list<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    let site = app.ctx.store.site(id).await?;
    let status = app.ctx.host.workers_status().await.ok();
    let running = |name: &str| -> Vec<serde_json::Value> {
        status.as_ref().and_then(|s| s.as_array()).into_iter().flatten().filter(|p| p["slug"] == site.slug.as_str() && p["name"] == name).cloned().collect()
    };
    let out: Vec<_> = app.ctx.store.workers(Some(id)).await?.into_iter().map(|w| json!({ "worker": w, "processes": running(&w.name) })).collect();
    Ok(Json(json!({ "supervisor": status.is_some(), "workers": out })).into_response())
}

#[derive(Deserialize)]
struct NewWorker {
    name: String,
    command: String,
    #[serde(default = "one")]
    processes: u32,
    #[serde(default = "mem")]
    memory_mb: u32,
}

fn one() -> u32 {
    1
}

fn mem() -> u32 {
    256
}

async fn create<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<NewWorker>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let site = app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let args = flows::worker_args(&r.command).map_err(ApiError::bad_request)?;
    let spec = alveare_core::agent::WorkerSpec { slug: site.slug.clone(), uid: site.uid(), name: r.name.clone(), args, processes: r.processes, memory_mb: r.memory_mb };
    spec.validate().map_err(ApiError::bad_request)?;
    let wid = app.ctx.store.add_worker(id, &r.name, r.command.trim(), r.processes, r.memory_mb).await?;
    if let Err(e) = flows::sync_workers(&app.ctx).await {
        app.ctx.store.delete_worker(wid).await?;
        return Err(e.into());
    }
    app.auth.audit(Some(user.id), "worker.create", Some(id), &format!("{}: {} (×{}, {} MB)", r.name, r.command.trim(), r.processes, r.memory_mb)).await?;
    flows::budget_alert(&app.ctx, app.ram_mb).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": wid }))).into_response())
}

#[derive(Deserialize)]
struct WorkerUpdate {
    active: bool,
}

async fn update<H: Host>(State(app): State<App<H>>, Path((id, wid)): Path<(i64, i64)>, m: Method, h: HeaderMap, Json(r): Json<WorkerUpdate>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let w = site_worker(&app, id, wid).await?;
    app.ctx.store.set_worker_active(wid, r.active).await?;
    flows::sync_workers(&app.ctx).await?;
    app.auth.audit(Some(user.id), if r.active { "worker.start" } else { "worker.stop" }, Some(id), &w.name).await?;
    Ok(Json(json!({})).into_response())
}

async fn remove<H: Host>(State(app): State<App<H>>, Path((id, wid)): Path<(i64, i64)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let w = site_worker(&app, id, wid).await?;
    app.ctx.store.delete_worker(wid).await?;
    flows::sync_workers(&app.ctx).await?;
    app.auth.audit(Some(user.id), "worker.delete", Some(id), &w.name).await?;
    Ok(Json(json!({})).into_response())
}

async fn restart<H: Host>(State(app): State<App<H>>, Path((id, wid)): Path<(i64, i64)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let w = site_worker(&app, id, wid).await?;
    let site = app.ctx.store.site(id).await?;
    app.ctx.host.worker_restart(&site.slug, &w.name).await?;
    app.auth.audit(Some(user.id), "worker.restart", Some(id), &w.name).await?;
    Ok(Json(json!({})).into_response())
}

async fn deploy_config<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let cfg = app.ctx.store.deploy_config(id).await?;
    Ok(Json(json!({ "repo": cfg.as_ref().map(|c| &c.0), "branch": cfg.as_ref().map(|c| &c.1) })).into_response())
}

#[derive(Deserialize)]
struct DeployConfig {
    repo: String,
    branch: String,
}

async fn set_deploy<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<DeployConfig>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let (repo, branch) = (r.repo.trim(), r.branch.trim());
    if !alveare_core::agent::valid_repo(repo) || !alveare_core::agent::valid_branch(branch) {
        return Err(ApiError::bad_request("repository (https:// o git@host:percorso) o branch non validi"));
    }
    app.ctx.store.set_deploy_config(id, repo, branch).await?;
    app.auth.audit(Some(user.id), "deploy.config", Some(id), &format!("{repo} {branch}")).await?;
    Ok(Json(json!({})).into_response())
}

async fn deploy<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    let (ctx, auth, uid) = (app.ctx.clone(), app.auth.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.deploy", Some(id), Some(user.id), move |mut log| async move {
            let result = flows::deploy_site(&ctx, id, &mut log).await;
            auth.audit(Some(uid), "site.deploy", Some(id), if result.is_ok() { "riuscito" } else { "fallito" }).await?;
            result?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

/// Chiave pubblica da aggiungere come deploy key (sola lettura) nel repository.
async fn deploy_key<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let site = app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    Ok(Json(json!({ "public_key": app.ctx.host.deploy_key(&site.slug).await? })).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn managers_define_workers_and_deploy_from_git() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let url = format!("/api/sites/{id}/workers");
        let (s, _, _) = call(&t, req("POST", &url, Some(&reader), Some(json!({"name": "coda", "command": "php artisan queue:work"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        for bad in [json!({"name": "Coda!", "command": "php artisan queue:work"}), json!({"name": "x", "command": "php /etc/x.php"}), json!({"name": "x", "command": "php artisan horizon", "processes": 50})] {
            let (s, _, _) = call(&t, req("POST", &url, Some(&manager), Some(bad.clone()))).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        }
        let (s, body, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"name": "horizon", "command": "php artisan horizon", "memory_mb": 384})))).await;
        assert_eq!(s, StatusCode::CREATED, "{body}");
        assert!(t.app.ctx.host.calls().iter().any(|c| c == "sync_workers sona_it/horizon:artisan horizon x1 384MB"));
        let (_, list, _) = call(&t, req("GET", &url, Some(&reader), None)).await;
        assert_eq!(list["workers"][0]["processes"][0]["pid"], 42);
        let wid = body["id"].as_i64().unwrap();
        let (s, _, _) = call(&t, req("POST", &format!("{url}/{wid}/restart"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::OK);

        let deploy = format!("/api/sites/{id}/deploy");
        let (s, _, _) = call(&t, req("PUT", &deploy, Some(&manager), Some(json!({"repo": "--upload-pack=x", "branch": "main"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        call(&t, req("PUT", &deploy, Some(&manager), Some(json!({"repo": "git@github.com:a/b.git", "branch": "main"})))).await;
        let (_, key, _) = call(&t, req("POST", &format!("{deploy}/key"), Some(&manager), None)).await;
        assert!(key["public_key"].as_str().unwrap().starts_with("ssh-ed25519"));
        let (s, body, _) = call(&t, req("POST", &deploy, Some(&manager), None)).await;
        assert_eq!(s, StatusCode::ACCEPTED);
        let job = wait_job(&t, body["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        assert!(t.app.ctx.host.calls().iter().any(|c| c == "worker_restart sona_it "));
    }
}
