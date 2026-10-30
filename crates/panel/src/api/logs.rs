//! Log dei siti: tail, ricerca nei file ruotati, errori PHP raggruppati, download, SSE.
use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}/logs/{kind}", get(read_log::<H>).delete(clear_log::<H>))
        .route("/api/sites/{id}/logs/{kind}/stream", get(stream_log::<H>))
        .route("/api/sites/{id}/logs/{kind}/groups", get(error_groups::<H>))
        .route("/api/sites/{id}/logs/{kind}/download", get(download_log::<H>))
}

/// `2xx`…`5xx` o un codice esatto.
fn status_matches(filter: &str, status: u16) -> bool {
    match filter.strip_suffix("xx").and_then(|c| c.parse::<u16>().ok()) {
        Some(class) => status / 100 == class,
        None => filter.parse::<u16>().is_ok_and(|s| s == status),
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum LogKind {
    Access,
    PhpError,
    PhpSlow,
    WpDebug,
}

const WP_DEBUG_LOG: &str = "public/wp-content/debug.log";

impl LogKind {
    fn file(self) -> &'static str {
        match self {
            LogKind::Access => "access.log",
            LogKind::PhpError => "php-error.log",
            LogKind::PhpSlow => "php-slow.log",
            LogKind::WpDebug => "debug.log",
        }
    }
}

#[derive(Deserialize)]
struct LinesQuery {
    lines: Option<usize>,
    /// Testo da cercare (IP, path, messaggio): attiva la ricerca anche nei file ruotati.
    q: Option<String>,
    /// Solo access log: `4xx` o `404`.
    status: Option<String>,
}

fn log_payload(kind: LogKind, lines: Vec<String>) -> serde_json::Value {
    if kind == LogKind::Access {
        let entries: Vec<_> = lines.iter().filter_map(|l| crate::logs::parse_access(l)).collect();
        json!({ "entries": entries })
    } else {
        json!({ "lines": lines })
    }
}

async fn read_log<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, LogKind)>, Query(q): Query<LinesQuery>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    let site = app.ctx.store.site(id).await?;
    let n = q.lines.unwrap_or(200).min(5000);
    let searching = q.q.as_deref().is_some_and(|x| !x.is_empty()) || q.status.as_deref().is_some_and(|x| !x.is_empty());
    let lines = if kind == LogKind::WpDebug {
        // debug.log sta nella cartella del sito: si legge con cap-std, mai seguendo link verso fuori.
        let files = SiteFiles::open(&app.paths, &site.slug, None)?;
        match files.open_read(WP_DEBUG_LOG) {
            Ok(f) => blocking(move || crate::logs::tail_file(f, n).map_err(FileError::Io)).await?,
            Err(FileError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e.into()),
        }
    } else if searching {
        let all = crate::logs::log_files(&app.paths.logs.join(site.slug.as_str()), kind.file());
        let (text, status) = (q.q.unwrap_or_default(), q.status.unwrap_or_default());
        let mut found = blocking(move || {
            crate::logs::search(
                &all,
                |l| {
                    (text.is_empty() || l.contains(&text))
                        && (status.is_empty() || crate::logs::parse_access(l).is_some_and(|e| status_matches(&status, e.status)))
                },
                n,
            )
            .map_err(FileError::Io)
        })
        .await?;
        found.reverse();
        found
    } else {
        let path = app.paths.logs.join(site.slug.as_str()).join(kind.file());
        blocking(move || crate::logs::tail(&path, n).map_err(FileError::Io)).await?
    };
    Ok(Json(log_payload(kind, lines)).into_response())
}

/// Solo il debug.log di WordPress si svuota dal pannello; gli altri ruotano da soli.
async fn clear_log<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, LogKind)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    if kind != LogKind::WpDebug {
        return Err(ApiError::bad_request("si può svuotare solo il debug.log di WordPress"));
    }
    let files = site_files(&app, &user, id).await?;
    blocking(move || files.write(WP_DEBUG_LOG, b"")).await?;
    app.auth.audit(Some(user.id), "log.clear", Some(id), "debug.log").await?;
    Ok(Json(json!({})).into_response())
}

async fn error_groups<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, LogKind)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    if kind != LogKind::PhpError {
        return Err(ApiError::bad_request("si raggruppano solo gli errori PHP"));
    }
    let site = app.ctx.store.site(id).await?;
    let path = app.paths.logs.join(site.slug.as_str()).join(kind.file());
    let lines = blocking(move || crate::logs::tail(&path, 20_000).map_err(FileError::Io)).await?;
    Ok(Json(crate::logs::php_error_groups(&lines)).into_response())
}

async fn download_log<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, LogKind)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    if kind == LogKind::WpDebug {
        return Err(ApiError::bad_request("il debug.log si scarica dal file manager"));
    }
    let site = app.ctx.store.site(id).await?;
    let file = tokio::fs::File::open(app.paths.logs.join(site.slug.as_str()).join(kind.file())).await.map_err(|e| ApiError::from(FileError::Io(e)))?;
    let name = format!("{}-{}", site.slug.as_str(), kind.file());
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))],
        axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file)),
    )
        .into_response())
}

async fn stream_log<H: Host>(State(app): State<App<H>>, Path((id, kind)): Path<(i64, LogKind)>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use tokio_stream::StreamExt;
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    if kind == LogKind::WpDebug {
        return Err(ApiError::bad_request("il debug.log non si segue in tempo reale"));
    }
    let site = app.ctx.store.site(id).await?;
    let path = app.paths.logs.join(site.slug.as_str()).join(kind.file());
    let rx = crate::logs::follow(path, std::time::Duration::from_secs(1));
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(move |line| {
        let data = match kind {
            LogKind::Access => crate::logs::parse_access(&line).map(|e| serde_json::to_string(&e).expect("serializzazione")),
            _ => Some(serde_json::to_string(&line).expect("serializzazione")),
        };
        Ok::<_, std::convert::Infallible>(Event::default().data(data.unwrap_or_else(|| "null".into())))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}


#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use crate::auth::Role;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn search_spans_rotated_files_and_filters_access_by_status() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let logs = t.app.paths.logs.join("sona_it");
        let line = |ip: &str, uri: &str, status: u16| format!("{{\"ts\":1.0,\"request\":{{\"client_ip\":\"{ip}\",\"method\":\"GET\",\"uri\":\"{uri}\"}},\"status\":{status}}}\n");
        std::fs::write(logs.join("access.log"), line("1.1.1.1", "/", 200) + &line("2.2.2.2", "/wp-login.php", 404)).unwrap();
        std::fs::write(logs.join("access-2026-10-29T01-00-00.000.log"), line("2.2.2.2", "/xmlrpc.php", 403)).unwrap();
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/access?q=2.2.2.2"), Some(&reader), None)).await;
        assert_eq!(s, StatusCode::OK);
        let uris: Vec<&str> = body["entries"].as_array().unwrap().iter().map(|e| e["uri"].as_str().unwrap()).collect();
        assert_eq!(uris, ["/xmlrpc.php", "/wp-login.php"], "in ordine di tempo, come il tail");
        let (_, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/access?status=4xx"), Some(&reader), None)).await;
        assert_eq!(body["entries"].as_array().unwrap().len(), 2);
        let (_, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/access?status=200"), Some(&reader), None)).await;
        assert_eq!(body["entries"][0]["ip"], "1.1.1.1");
        drop(admin);
    }

    #[tokio::test]
    async fn php_errors_grouped_and_logs_downloadable() {
        let t = setup().await;
        let (admin, id) = site_with_files(&t).await;
        let logs = t.app.paths.logs.join("sona_it");
        std::fs::write(logs.join("php-error.log"), "[x] PHP Warning:  a\n[y] PHP Warning:  a\n[z] PHP Notice:  b\n").unwrap();
        let (s, body, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/php-error/groups"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!((body[0]["message"].as_str(), body[0]["count"].as_u64()), (Some("PHP Warning: a"), Some(2)));
        let (s, _, h) = call(&t, req("GET", &format!("/api/sites/{id}/logs/php-error/download"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::OK);
        assert!(h["content-disposition"].to_str().unwrap().contains("sona_it-php-error.log"));
        let (s, _, _) = call(&t, req("GET", &format!("/api/sites/{id}/logs/access/groups"), Some(&admin), None)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}
