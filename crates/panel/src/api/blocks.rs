//! Blocco IP dalla vista dei log: per sito (gestore) o su tutti i siti (admin), con scadenza.
use axum::routing::delete;

use super::*;

pub(super) fn routes<H: Host>() -> Router<App<H>> {
    Router::new()
        .route("/api/sites/{id}/blocks", get(site_blocks::<H>).post(add_block::<H>))
        .route("/api/blocks", get(all_blocks::<H>))
        .route("/api/blocks/{block}", delete(remove_block::<H>))
}

/// Un IP o una rete CIDR, normalizzati; niente reti più larghe di /16 (IPv4) o /32 (IPv6).
pub fn parse_cidr(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let (addr, prefix) = match raw.split_once('/') {
        Some((a, p)) => (a, Some(p.parse::<u8>().ok()?)),
        None => (raw, None),
    };
    let ip: std::net::IpAddr = addr.parse().ok()?;
    match (ip, prefix) {
        (_, None) => Some(ip.to_string()),
        (std::net::IpAddr::V4(_), Some(p)) if (16..=32).contains(&p) => Some(format!("{ip}/{p}")),
        (std::net::IpAddr::V6(_), Some(p)) if (32..=128).contains(&p) => Some(format!("{ip}/{p}")),
        _ => None,
    }
}

#[derive(Deserialize, Clone, Copy)]
enum Duration {
    #[serde(rename = "1h")]
    Hour,
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "always")]
    Always,
}

impl Duration {
    fn seconds(self) -> Option<i64> {
        match self {
            Self::Hour => Some(3600),
            Self::Day => Some(86400),
            Self::Week => Some(7 * 86400),
            Self::Always => None,
        }
    }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Scope {
    Site,
    All,
}

#[derive(Deserialize)]
struct BlockReq {
    ip: String,
    duration: Duration,
    scope: Scope,
}

async fn site_blocks<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    require_site(&app, &user, id, Need::Read).await?;
    Ok(Json(app.ctx.store.blocks(Some(id)).await?).into_response())
}

async fn all_blocks<H: Host>(State(app): State<App<H>>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    require_admin(&caller(&app, &m, &h).await?)?;
    Ok(Json(app.ctx.store.blocks(None).await?).into_response())
}

async fn add_block<H: Host>(State(app): State<App<H>>, Path(id): Path<i64>, m: Method, h: HeaderMap, Json(r): Json<BlockReq>) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    app.ctx.store.site(id).await?;
    require_site(&app, &user, id, Need::Manage).await?;
    if r.scope == Scope::All {
        require_admin(&user)?;
    }
    let cidr = parse_cidr(&r.ip).ok_or_else(|| ApiError::bad_request("IP o rete non validi (reti al massimo /16 in IPv4, /32 in IPv6)"))?;
    let at = now();
    let site = (r.scope == Scope::Site).then_some(id);
    let block = app.ctx.store.add_block(site, &cidr, r.duration.seconds().map(|s| at + s), Some(user.id), at).await?;
    flows::sync_caddy(&app.ctx).await?;
    let scope = if site.is_some() { "questo sito" } else { "tutti i siti" };
    app.auth.audit(Some(user.id), "ip.block", Some(id), &format!("{cidr} su {scope}")).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": block }))).into_response())
}

async fn remove_block<H: Host>(State(app): State<App<H>>, Path(block): Path<i64>, m: Method, h: HeaderMap) -> ApiResult<Response> {
    let user = caller(&app, &m, &h).await?;
    let b = app.ctx.store.block(block).await?;
    match b.site_id {
        Some(site) => require_site(&app, &user, site, Need::Manage).await?,
        None => require_admin(&user)?,
    }
    app.ctx.store.remove_block(block).await?;
    flows::sync_caddy(&app.ctx).await?;
    app.auth.audit(Some(user.id), "ip.unblock", b.site_id, &b.cidr).await?;
    Ok(Json(json!({})).into_response())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::parse_cidr;
    use crate::auth::Role;
    use axum::http::StatusCode;
    use serde_json::json;

    #[test]
    fn ip_and_networks_are_validated() {
        assert_eq!(parse_cidr(" 198.51.100.9 ").as_deref(), Some("198.51.100.9"));
        assert_eq!(parse_cidr("203.0.113.0/24").as_deref(), Some("203.0.113.0/24"));
        assert_eq!(parse_cidr("2001:db8::1").as_deref(), Some("2001:db8::1"));
        for bad in ["0.0.0.0/0", "10.0.0.0/8", "nope", "1.2.3.4/33", "1.2.3.4 5.6.7.8"] {
            assert!(parse_cidr(bad).is_none(), "{bad}");
        }
    }

    #[tokio::test]
    async fn manager_blocks_on_own_site_admin_blocks_everywhere() {
        let t = setup().await;
        let (_, admin) = token_for(&t, "admin@example.org", true).await;
        let id = create_site(&t, &admin, "sona.it").await;
        let manager = user_with_role(&t, "g@example.org", id, Role::Manager).await;
        let reader = user_with_role(&t, "r@example.org", id, Role::Readonly).await;
        let url = format!("/api/sites/{id}/blocks");
        let (s, body, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"ip": "198.51.100.9", "duration": "24h", "scope": "site"})))).await;
        assert_eq!(s, StatusCode::CREATED, "{body}");
        let (s, _, _) = call(&t, req("POST", &url, Some(&manager), Some(json!({"ip": "203.0.113.5", "duration": "1h", "scope": "all"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "su tutti i siti solo l'admin");
        let (s, _, _) = call(&t, req("POST", &url, Some(&reader), Some(json!({"ip": "203.0.113.5", "duration": "1h", "scope": "site"})))).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"ip": "203.0.113.0/24", "duration": "always", "scope": "all"})))).await;
        assert_eq!(s, StatusCode::CREATED);
        let (s, _, _) = call(&t, req("POST", &url, Some(&admin), Some(json!({"ip": "10.0.0.0/8", "duration": "always", "scope": "all"})))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (_, list, _) = call(&t, req("GET", &url, Some(&reader), None)).await;
        assert_eq!(list.as_array().unwrap().len(), 2);
        assert!(list[0]["expires_at"].as_i64().unwrap() > list[0]["created_at"].as_i64().unwrap());
        assert!(list[1]["expires_at"].is_null());
        assert_eq!(t.app.ctx.store.caddy_sites().await.unwrap()[0].blocked, ["198.51.100.9", "203.0.113.0/24"]);
        let global = list[1]["id"].as_i64().unwrap();
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/blocks/{global}"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let own = list[0]["id"].as_i64().unwrap();
        let (s, _, _) = call(&t, req("DELETE", &format!("/api/blocks/{own}"), Some(&manager), None)).await;
        assert_eq!(s, StatusCode::OK);
        let (_, all, _) = call(&t, req("GET", "/api/blocks", Some(&admin), None)).await;
        assert_eq!(all.as_array().unwrap().len(), 1);
    }
}
