//! Template (solo admin): nuovo (vuoto o copia di un altro), elimina (nel cestino dei template)
//! e i file con lo stesso file manager dei siti, `template.toml` compreso.
//! Upload temporanei e cestino stanno in `.tmp` e `.trash` dentro la cartella dei template:
//! è un mount a sé, e un rename fra mount diversi non funziona.
use axum::routing::delete;

use super::*;
use crate::store::SiteKind;
use crate::template::{Manifest, Template};

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/templates", get(list::<H>).post(create::<H>))
        .route("/api/templates/{name}", delete(remove::<H>))
        .route("/api/templates/{name}/files", get(list_files::<H>))
        .route("/api/templates/{name}/files/content", get(read_file::<H>).put(write_file::<H>).layer(DefaultBodyLimit::max(EDITOR_LIMIT as usize + 1024)))
        .route("/api/templates/{name}/files/download", get(download_file::<H>))
        .route("/api/templates/{name}/files/ops", post(file_op::<H>))
        .route("/api/templates/{name}/files/uploads", post(upload_start::<H>))
        .route("/api/templates/{name}/files/uploads/{upload}", get(upload_status::<H>))
        .route("/api/templates/{name}/files/uploads/{upload}/{n}", put(upload_chunk::<H>).layer(DefaultBodyLimit::max(CHUNK as usize + 1024)))
}

fn template_dir<H>(app: &App<H>, name: &str) -> ApiResult<std::path::PathBuf> {
    let name = Slug::parse(name).map_err(|_| ApiError::bad_request("nome di template non valido"))?;
    Ok(app.templates.join(name.as_str()))
}

pub(super) async fn files<H>(app: &App<H>, user: &User, name: &str) -> ApiResult<SiteFiles> {
    require_admin(user)?;
    let (root, tmp) = (template_dir(app, name)?, app.templates.join(".tmp"));
    if !root.join("template.toml").is_file() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "template non trovato"));
    }
    blocking(move || {
        std::fs::create_dir_all(&tmp)?;
        SiteFiles::open_dirs(&root, &tmp, None)
    })
    .await
}

async fn list<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    caller(&app, &m, &h).await?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&app.templates).map_err(internal)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Ok(t) = load_template(&app.templates, &name) {
            out.push(json!({ "id": name, "name": t.manifest.name, "kind": t.manifest.kind, "snapshot": t.manifest.sql.is_some(), "steps": t.manifest.post_create.len() }));
        }
    }
    out.sort_by_key(|t| t["name"].as_str().unwrap_or_default().to_lowercase());
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
struct NewTemplate {
    id: String,
    name: String,
    kind: Option<SiteKind>,
    /// Template da copiare; senza, un template vuoto del tipo `kind`.
    from: Option<String>,
}

/// Il nome nel manifest copiato, lasciando com'è il resto (commenti compresi).
fn rename_manifest(text: &str, name: &str) -> String {
    let quoted = toml::Value::String(name.into()).to_string();
    let mut done = false;
    text.lines()
        .map(|l| {
            if !done && l.split('=').next().is_some_and(|k| k.trim() == "name") {
                done = true;
                format!("name = {quoted}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

async fn create<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap, Json(r): Json<NewTemplate>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let dest = template_dir(&app, &r.id)?;
    let name = r.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::bad_request("serve un nome"));
    }
    if dest.exists() {
        return Err(ApiError::new(StatusCode::CONFLICT, "esiste già un template con questo identificativo"));
    }
    let source = match &r.from {
        Some(from) => {
            load_template(&app.templates, from)?;
            Some(template_dir(&app, from)?)
        }
        None => None,
    };
    let kind = r.kind;
    let written = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        match source {
            Some(src) => {
                crate::template::copy_tree(&src, &dest)?;
                let text = std::fs::read_to_string(dest.join("template.toml"))?;
                std::fs::write(dest.join("template.toml"), rename_manifest(&text, &name))?;
            }
            None => {
                let kind = kind.ok_or_else(|| flows::Invalid("serve il tipo (wordpress, php o laravel)".into()))?;
                let manifest = Manifest { name, kind, render: vec![], post_create: vec![], sql: None, sql_domain: "template.local".into(), table_prefix: None };
                std::fs::create_dir_all(dest.join("files/public"))?;
                std::fs::write(dest.join("template.toml"), toml::to_string(&manifest)?)?;
            }
        }
        Template::load(&dest)?;
        Ok(())
    })
    .await
    .map_err(internal)?;
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(template_dir(&app, &r.id)?);
        return Err(e.into());
    }
    app.auth.audit(Some(user.id), "template.create", None, &format!("{}{}", r.id, r.from.as_ref().map(|f| format!(" da {f}")).unwrap_or_default())).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": r.id }))).into_response())
}

async fn remove<H: Host>(State(app): State<App<H>>, Path(name): Path<String>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_admin(&user)?;
    let dir = template_dir(&app, &name)?;
    if !dir.join("template.toml").is_file() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "template non trovato"));
    }
    let trash = app.templates.join(".trash");
    std::fs::create_dir_all(&trash).map_err(internal)?;
    std::fs::rename(&dir, trash.join(format!("{}-{name}", now()))).map_err(internal)?;
    app.auth.audit(Some(user.id), "template.delete", None, &name).await?;
    Ok(Json(json!({})).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use axum::http::StatusCode;
    use serde_json::json;

    fn raw_put(uri: &str, auth: &str, body: &str) -> Request<Body> {
        raw("PUT", uri, auth, body.as_bytes().to_vec())
    }

    #[tokio::test]
    async fn admins_create_duplicate_and_delete_templates() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let (_, plain) = token_for(&t, "u@example.org", false).await;
        let new = json!({"id": "vetrina", "name": "Vetrina", "kind": "php"});
        let (s, _, _) = call(&t, req("POST", "/api/templates", Some(&plain), Some(new.clone()))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, body, _) = call(&t, req("POST", "/api/templates", Some(&admin), Some(new.clone()))).await;
        assert_eq!(s, StatusCode::CREATED, "{body}");
        let (s, _, _) = call(&t, req("POST", "/api/templates", Some(&admin), Some(new))).await;
        assert_eq!(s, StatusCode::CONFLICT, "nome già usato");
        let (s, _, _) = call(&t, req("POST", "/api/templates", Some(&admin), Some(json!({"id": "../fuori", "name": "x", "kind": "php"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        std::fs::create_dir_all(t.app.templates.join("wordpress/files/public")).unwrap();
        std::fs::write(t.app.templates.join("wordpress/files/public/index.php"), "<?php").unwrap();
        let (s, _, _) = call(&t, req("POST", "/api/templates", Some(&admin), Some(json!({"id": "wp_mio", "name": "WP mio", "from": "wordpress"})))).await;
        assert_eq!(s, StatusCode::CREATED);
        assert!(t.app.templates.join("wp_mio/files/public/index.php").exists());
        let (_, list, _) = call(&t, req("GET", "/api/templates", Some(&admin), None)).await;
        let names: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Vetrina", "WordPress", "WP mio"]);
        let (s, _, _) = call(&t, req("DELETE", "/api/templates/vetrina", Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert!(!t.app.templates.join("vetrina").exists());
        let (_, list, _) = call(&t, req("GET", "/api/templates", Some(&admin), None)).await;
        assert_eq!(list.as_array().unwrap().len(), 2, "le cartelle nascoste (cestino, tmp) non sono template");
        let (s, _, _) = call(&t, req("DELETE", "/api/templates/vetrina", Some(&admin), None)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn template_files_use_the_file_manager_and_the_manifest_is_validated() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let (_, plain) = token_for(&t, "u@example.org", false).await;
        call(&t, req("POST", "/api/templates", Some(&admin), Some(json!({"id": "vetrina", "name": "Vetrina", "kind": "php"})))).await;
        let (s, _, _) = call(&t, req("GET", "/api/templates/vetrina/files?path=", Some(&plain), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, body, _) = call(&t, req("GET", "/api/templates/vetrina/files?path=", Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        let names: Vec<&str> = body.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["files", "template.toml"]);
        let (s, _, _) = call(&t, raw_put("/api/templates/vetrina/files/content?path=files/index.php", &admin, "<?php echo 1;")).await;
        assert_eq!(s, StatusCode::OK);
        let (s, body, _) = call(&t, raw_put("/api/templates/vetrina/files/content?path=template.toml", &admin, "name = \"Rotto\"\nkind = \"boh\"\n")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
        let (s, _, _) = call(&t, raw_put("/api/templates/vetrina/files/content?path=template.toml", &admin, "name = \"Vetrina 2\"\nkind = \"php\"\n")).await;
        assert_eq!(s, StatusCode::OK);
        let (_, list, _) = call(&t, req("GET", "/api/templates", Some(&admin), None)).await;
        assert!(list.as_array().unwrap().iter().any(|t| t["name"] == "Vetrina 2"));
        let (s, _, _) = call(&t, req("POST", "/api/templates/vetrina/files/ops", Some(&admin), Some(json!({"op": "delete", "path": "template.toml"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "il manifest non si cancella né si rinomina");
    }
}
