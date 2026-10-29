//! Interfaccia SvelteKit (build statica) incorporata nel binario: un solo artefatto da distribuire.
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

#[derive(rust_embed::Embed)]
#[folder = "../../ui/build"]
#[allow_missing = true]
struct Assets;

/// File statico se esiste, altrimenti index.html (le rotte sono della SPA).
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (file, path) = match Assets::get(path).filter(|_| !path.is_empty()) {
        Some(f) => (f, path),
        None => match Assets::get("index.html") {
            Some(f) => (f, "index.html"),
            None => return (StatusCode::NOT_FOUND, "interfaccia non inclusa in questa build").into_response(),
        },
    };
    let mime = file.metadata.mimetype().to_string();
    // I file in _app/immutable hanno l'hash nel nome: si possono tenere in cache per sempre.
    let cache = if path.starts_with("_app/immutable/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, cache.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::X_FRAME_OPTIONS, "DENY".to_string()),
            (header::REFERRER_POLICY, "same-origin".to_string()),
        ],
        file.data,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_routes_fall_back_to_the_spa() {
        let resp = serve("/sites/3".parse().unwrap()).await;
        if Assets::get("index.html").is_some() {
            assert_eq!(resp.status(), StatusCode::OK);
            assert!(resp.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
            assert_eq!(resp.headers()[header::X_FRAME_OPTIONS], "DENY");
        } else {
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        }
    }
}
