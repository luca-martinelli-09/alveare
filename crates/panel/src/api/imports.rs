//! Area import (solo admin): archivi e dump caricati a blocchi, poi "Importa sito".
use std::os::unix::fs::PermissionsExt;

use axum::routing::delete;

use super::*;
use crate::store::SiteKind;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/imports", get(list::<H>))
        .route("/api/imports/{name}", delete(remove::<H>))
        .route("/api/imports/uploads", post(upload_start::<H>))
        .route("/api/imports/uploads/{upload}", get(upload_status::<H>))
        .route("/api/imports/uploads/{upload}/{n}", put(upload_chunk::<H>).layer(DefaultBodyLimit::max(CHUNK as usize + 1024)))
        .route("/api/sites/import", post(import_site::<H>))
}

/// Area import, creata al primo uso e leggibile solo dal pannello.
async fn area<H: Host>(app: &App<H>, user: &User) -> ApiResult<SiteFiles> {
    require_admin(user)?;
    let (files, tmp) = (app.paths.imports.join("files"), app.paths.imports.join("tmp"));
    blocking(move || {
        for d in [&files, &tmp] {
            std::fs::create_dir_all(d)?;
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
        }
        SiteFiles::open_dirs(&files, &tmp, None)
    })
    .await
}

/// Nome di un file dell'area: niente cartelle né file nascosti.
fn flat_name(name: &str) -> ApiResult<&str> {
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
        return Err(ApiError::bad_request("nome di file non valido"));
    }
    Ok(name)
}

async fn list<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = area(&app, &user).await?;
    let entries = blocking(move || files.list("")).await?;
    let out: Vec<_> = entries.into_iter().filter(|e| !e.is_dir).map(|e| json!({ "name": e.name, "size": e.size, "modified": e.modified })).collect();
    Ok(Json(out).into_response())
}

async fn remove<H: Host>(State(app): State<App<H>>, Path(name): Path<String>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    flat_name(&name)?;
    let path = app.paths.imports.join("files").join(&name);
    area(&app, &user).await?;
    tokio::fs::remove_file(&path).await.map_err(|e| ApiError::from(FileError::Io(e)))?;
    app.auth.audit(Some(user.id), "import.delete", None, &name).await?;
    Ok(Json(json!({})).into_response())
}

async fn upload_start<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<UploadReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let name = flat_name(&r.path)?.to_string();
    let files = area(&app, &user).await?;
    let info = blocking(move || files.upload_start(&name, r.size, true)).await?;
    app.auth.audit(Some(user.id), "import.upload", None, &r.path).await?;
    Ok((StatusCode::CREATED, Json(info)).into_response())
}

async fn upload_status<H: Host>(State(app): State<App<H>>, Path(upload): Path<String>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = area(&app, &user).await?;
    Ok(Json(blocking(move || files.upload_status(&upload)).await?).into_response())
}

async fn upload_chunk<H: Host>(State(app): State<App<H>>, Path((upload, n)): Path<(String, u64)>, m: Method, h: HeaderMap, body: axum::body::Bytes) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let files = area(&app, &user).await?;
    let _turn = app.uploads.lock().await;
    Ok(Json(blocking(move || files.upload_chunk(&upload, n, &body)).await?).into_response())
}

#[derive(Deserialize)]
struct ImportReq {
    domain: String,
    title: String,
    archive: String,
    dump: String,
    #[serde(default = "default_template")]
    template: String,
}

fn default_template() -> String {
    "wordpress".into()
}

async fn import_site<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<ImportReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let domain = sites::free_domain(&app, &r.domain).await?;
    let template = load_template(&app.templates, &r.template)?;
    let dir = app.paths.imports.join("files");
    let (archive, dump) = (dir.join(flat_name(&r.archive)?), dir.join(flat_name(&r.dump)?));
    if crate::database::dump_kind(&r.dump).is_none() {
        return Err(ApiError::bad_request("il dump deve essere .sql o .sql.gz"));
    }
    for p in [&archive, &dump] {
        if !p.is_file() {
            return Err(ApiError::new(StatusCode::NOT_FOUND, format!("{} non è nell'area import", p.file_name().unwrap_or_default().to_string_lossy())));
        }
    }
    let kind: SiteKind = template.manifest.kind;
    let req = flows::ImportSite {
        site: CreateSite {
            domain,
            kind,
            template: r.template.clone(),
            title: r.title.trim().to_string(),
            admin_email: user.email.clone(),
            admin_user: crate::flows::DEFAULT_ADMIN_USER.into(),
            table_prefix: template.manifest.table_prefix.clone().unwrap_or_else(|| "wp_".into()),
            space: false,
        },
        archive,
        dump,
    };
    let (ctx, auth, jobs, uid) = (app.ctx.clone(), app.auth.clone(), app.jobs.clone(), user.id);
    let job = app
        .jobs
        .spawn("site.import", None, Some(user.id), move |mut log| async move {
            let site = flows::import_site(&ctx, &req, &mut log).await?;
            jobs.set_site(log.job_id, site.id).await?;
            auth.audit(Some(uid), "site.import", Some(site.id), &format!("{} + {}", r.archive, r.dump)).await?;
            Ok(log)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use axum::http::StatusCode;
    use serde_json::json;
    use std::io::Write;

    fn small_zip() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        z.start_file("wp-content/index.php", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"<?php").unwrap();
        z.finish().unwrap();
        buf.into_inner()
    }

    async fn upload(t: &T, admin: &str, name: &str, data: Vec<u8>) {
        let (s, up, _) = call(t, req("POST", "/api/imports/uploads", Some(admin), Some(json!({"path": name, "size": data.len()})))).await;
        assert_eq!(s, StatusCode::CREATED, "{up}");
        let id = up["id"].as_str().unwrap().to_string();
        let (s, st, _) = call(t, raw("PUT", &format!("/api/imports/uploads/{id}/0"), admin, data)).await;
        assert_eq!((s, st["complete"].as_bool()), (StatusCode::OK, Some(true)));
    }

    #[tokio::test]
    async fn admin_uploads_lists_and_deletes_import_files() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        upload(&t, &admin, "epops.zip", small_zip()).await;
        let (s, list, _) = call(&t, req("GET", "/api/imports", Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(list[0]["name"], "epops.zip");
        let (s, _, _) = call(&t, req("DELETE", "/api/imports/epops.zip", Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        let (_, list, _) = call(&t, req("GET", "/api/imports", Some(&admin), None)).await;
        assert_eq!(list, json!([]));
    }

    #[tokio::test]
    async fn import_area_is_admin_only_and_flat() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let (_, user) = token_for(&t, "u@example.org", false).await;
        let (s, _, _) = call(&t, req("GET", "/api/imports", Some(&user), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        for bad in ["../x.zip", "a/b.zip", ".nascosto"] {
            let (s, _, _) = call(&t, req("POST", "/api/imports/uploads", Some(&admin), Some(json!({"path": bad, "size": 3})))).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        }
    }

    #[tokio::test]
    async fn import_site_runs_as_job_from_uploaded_files() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        upload(&t, &admin, "epops.zip", small_zip()).await;
        upload(&t, &admin, "epops.sql", b"-- dump".to_vec()).await;
        let body = json!({"domain": "epops.it", "title": "Epops", "archive": "epops.zip", "dump": "epops.sql"});
        let (s, _, _) = call(&t, req("POST", "/api/sites/import", Some(&admin), Some(json!({"domain": "epops.it", "title": "x", "archive": "manca.zip", "dump": "epops.sql"})))).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        let (s, out, _) = call(&t, req("POST", "/api/sites/import", Some(&admin), Some(body))).await;
        assert_eq!(s, StatusCode::ACCEPTED, "{out}");
        let job = wait_job(&t, out["job"].as_i64().unwrap()).await;
        assert_eq!(job.status, "done", "{:?}", job.log);
        let calls = t.app.ctx.host.calls();
        assert!(calls.iter().any(|c| c.starts_with("import_files epops_it ") && c.ends_with("imports/files/epops.zip")), "{calls:?}");
        assert!(calls.contains(&"db_import epops_it as epops_it -- dump".to_string()));
        let (_, audit, _) = call(&t, req("GET", &format!("/api/sites/{}/audit", job.site_id.unwrap()), Some(&admin), None)).await;
        assert_eq!(audit[0]["action"], "site.import");
    }
}
