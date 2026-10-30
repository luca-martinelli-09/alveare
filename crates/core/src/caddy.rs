use std::fmt::Write;

use crate::php::{socket_path, LOGS_DIR, SITES_DIR, SOCKET_DIR};
use crate::site::{Domain, Slug};

pub const ADMIN_SOCKET: &str = "/run/caddy/admin.sock";

#[derive(Debug, Clone)]
pub struct CaddySite {
    pub slug: Slug,
    pub primary: Domain,
    pub aliases: Vec<Domain>,
    pub redirects: Vec<Domain>,
    /// Pagina di manutenzione al posto del sito: file e database restano intatti.
    pub suspended: bool,
    /// Hash bcrypt della password dello staging (utente `staging`).
    pub staging_hash: Option<String>,
    /// Giorni di conservazione degli access log ruotati.
    pub log_days: u32,
    /// Override dell'admin dentro il blocco del sito (header, redirect, cache, blocchi di path).
    pub extra: Option<String>,
    /// IP o reti CIDR bloccati dal pannello (del sito e globali), già validati.
    pub blocked: Vec<String>,
}

const MAINTENANCE_PAGE: &str = "<!doctype html><html lang=\"it\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Sito in manutenzione</title><body style=\"font-family:system-ui;text-align:center;padding:4rem 1rem\"><h1>Sito in manutenzione</h1><p>Torniamo online a breve.</p></body></html>";

pub struct CaddyConfig {
    pub panel_host: Option<Domain>,
    /// Da proteggere con Cloudflare Access, come il pannello.
    pub adminer_host: Option<Domain>,
    pub sites: Vec<CaddySite>,
    /// Override globale: snippet riusabili dai siti, dopo le opzioni globali.
    pub global_extra: Option<String>,
}

/// Caddyfile completo dallo stato: nessuna modifica incrementale, così non può divergere.
pub fn render_caddyfile(cfg: &CaddyConfig) -> String {
    let mut out = format!(
        "{{
	admin unix/{ADMIN_SOCKET}
	auto_https off
	servers {{
		trusted_proxies static private_ranges
		client_ip_headers CF-Connecting-IP
	}}
}}
"
    );
    if let Some(extra) = cfg.global_extra.as_deref().filter(|e| !e.trim().is_empty()) {
        write!(out, "\n# override globale\n{}\n", extra.trim_end()).unwrap();
    }
    if let Some(host) = &cfg.panel_host {
        write!(out, "\nhttp://{} {{\n\treverse_proxy panel:8080 {{\n\t\theader_up X-Real-IP {{client_ip}}\n\t}}\n}}\n", host.as_str()).unwrap();
    }
    if let Some(host) = &cfg.adminer_host {
        // Solo il wrapper: adminer.php non si raggiunge direttamente.
        write!(
            out,
            "\nhttp://{} {{\n\troot * /usr/share/adminer\n\trewrite * /index.php\n\tphp_fastcgi unix/{SOCKET_DIR}/_adminer.sock {{\n\t\tenv REMOTE_ADDR {{client_ip}}\n\t}}\n}}\n",
            host.as_str()
        )
        .unwrap();
    }
    let mut sites: Vec<&CaddySite> = cfg.sites.iter().collect();
    sites.sort_by(|a, b| a.slug.as_str().cmp(b.slug.as_str()));
    for site in sites {
        render_site(&mut out, site);
    }
    out
}

fn render_site(out: &mut String, site: &CaddySite) {
    let s = site.slug.as_str();
    let hosts: Vec<String> =
        std::iter::once(&site.primary).chain(&site.aliases).map(|d| format!("http://{}", d.as_str())).collect();
    let hosts = hosts.join(", ");
    if site.suspended {
        write!(
            out,
            "\n{hosts} {{\n\theader Retry-After 3600\n\theader Content-Type \"text/html; charset=utf-8\"\n\trespond `{MAINTENANCE_PAGE}` 503\n}}\n"
        )
        .unwrap();
    } else {
        let extra = site
            .extra
            .as_deref()
            .filter(|e| !e.trim().is_empty())
            .map(|e| format!("\t# override del sito\n{}", e.trim_end().lines().map(|l| format!("\t{l}\n")).collect::<String>()))
            .unwrap_or_default();
        let blocked = if site.blocked.is_empty() {
            String::new()
        } else {
            format!("\t@alveare_blocked client_ip {}\n\trespond @alveare_blocked 403\n", site.blocked.join(" "))
        };
        let staging = site
            .staging_hash
            .as_ref()
            .map(|h| format!("\tbasic_auth {{\n\t\tstaging {h}\n\t}}\n\theader X-Robots-Tag \"noindex, nofollow\"\n"))
            .unwrap_or_default();
        write!(
            out,
            "
{hosts} {{
	root * {SITES_DIR}/{s}/public
	log {{
		output file {LOGS_DIR}/{s}/access.log {{
			roll_size 10MiB
			roll_keep 5
			roll_keep_for {keep}h
		}}
		format json
	}}
{blocked}{staging}	@uploads_php path_regexp (?i)^/wp-content/uploads/.*\\.ph(p[0-9]?|tml|ar)$
	respond @uploads_php 403
{extra}	php_fastcgi unix/{socket} {{
		env REMOTE_ADDR {{client_ip}}
	}}
	file_server
}}
",
            socket = socket_path(&site.slug),
            keep = site.log_days * 24,
        )
        .unwrap();
    }
    for r in &site.redirects {
        write!(out, "\nhttp://{} {{\n\tredir https://{}{{uri}} permanent\n}}\n", r.as_str(), site.primary.as_str()).unwrap();
    }
}

/// Controlla il JSON di `/adapt`: radici, socket e file di log devono essere quelli del sito
/// (`slug`), o per l'override globale (`None`) mai i socket PHP dei siti né radici fuori da /srv/sites.
pub fn adapted_violations(json: &serde_json::Value, slug: Option<&Slug>) -> Vec<String> {
    let mut out = Vec::new();
    walk(json, slug, &mut out);
    out
}

fn walk(v: &serde_json::Value, slug: Option<&Slug>, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, child) in map {
                if let Some(text) = child.as_str() {
                    check_value(k, text, slug, out);
                }
                walk(child, slug, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, slug, out)),
        _ => {}
    }
}

fn check_value(key: &str, v: &str, slug: Option<&Slug>, out: &mut Vec<String>) {
    let site_dir = slug.map(|s| format!("{SITES_DIR}/{}/", s.as_str()));
    let ok = match key {
        "root" => match &site_dir {
            Some(dir) => format!("{v}/").starts_with(dir.as_str()),
            None => v.starts_with(&format!("{SITES_DIR}/")),
        },
        "dial" => match slug {
            Some(s) => v == format!("unix/{}", socket_path(s)),
            None => !v.starts_with(&format!("unix/{SOCKET_DIR}/")),
        },
        "filename" => match slug {
            Some(s) => v.starts_with(&format!("{LOGS_DIR}/{}/", s.as_str())),
            None => v.starts_with(&format!("{LOGS_DIR}/")),
        },
        _ => true,
    };
    if !ok {
        out.push(format!("{key} non ammesso: {v}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::{Domain, Slug};

    fn d(s: &str) -> Domain {
        Domain::parse(s).unwrap()
    }

    fn sona() -> CaddySite {
        CaddySite {
            slug: Slug::parse("sona_it").unwrap(),
            primary: d("sona.it"),
            aliases: vec![d("comune-sona.it")],
            redirects: vec![d("www.sona.it")],
            suspended: false,
            staging_hash: None,
            log_days: 30,
            extra: None,
            blocked: vec![],
        }
    }

    #[test]
    fn blocked_ips_get_403_before_reaching_php() {
        let mut site = sona();
        site.blocked = vec!["198.51.100.9".into(), "203.0.113.0/24".into()];
        let c = render_caddyfile(&CaddyConfig { panel_host: None, adminer_host: None, global_extra: None, sites: vec![site] });
        let rule = c.find("\t@alveare_blocked client_ip 198.51.100.9 203.0.113.0/24\n\trespond @alveare_blocked 403\n").expect("blocco IP");
        assert!(rule < c.find("\tphp_fastcgi").unwrap());
        assert!(!render_caddyfile(&cfg()).contains("alveare_blocked"));
    }

    fn cfg() -> CaddyConfig {
        CaddyConfig { panel_host: Some(d("pannello.example.org")), adminer_host: None, sites: vec![sona()], global_extra: None }
    }

    #[test]
    fn site_and_global_overrides_land_in_their_blocks() {
        let mut site = sona();
        site.extra = Some("header Cache-Control \"max-age=3600\"\nimport sicurezza".into());
        let c = render_caddyfile(&CaddyConfig { panel_host: None, adminer_host: None, sites: vec![site], global_extra: Some("(sicurezza) {\n\theader X-Frame-Options DENY\n}".into()) });
        let global = c.find("\n(sicurezza) {\n\theader X-Frame-Options DENY\n}\n").expect("snippet globale");
        let block = c.find("http://sona.it, http://comune-sona.it {").unwrap();
        assert!(global < block, "gli snippet globali prima dei siti");
        let snippet = c.find("\t# override del sito\n\theader Cache-Control \"max-age=3600\"\n\timport sicurezza\n").expect("snippet del sito");
        assert!(block < snippet && snippet < c.find("\tphp_fastcgi").unwrap());
    }

    #[test]
    fn adapted_json_with_foreign_root_socket_or_log_is_flagged() {
        let json: serde_json::Value = serde_json::from_str(include_str!("../testdata/adapted_override.json")).unwrap();
        let sona = Slug::parse("sona_it").unwrap();
        let v = adapted_violations(&json, Some(&sona));
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().any(|x| x.contains("/etc")));
        assert!(v.iter().any(|x| x.contains("upupa_it.sock")));
        let clean = serde_json::json!({"apps": {"http": {"servers": {"s": {"routes": [{"handle": [{"handler": "vars", "root": "/srv/sites/sona_it/public"}, {"upstreams": [{"dial": "unix//run/php/sona_it.sock"}]}]}]}}}}, "logging": {"logs": {"l": {"writer": {"filename": "/srv/logs/sona_it/access.log"}}}}});
        assert!(adapted_violations(&clean, Some(&sona)).is_empty());
        let bad_log = serde_json::json!({"logging": {"logs": {"l": {"writer": {"filename": "/srv/sites/upupa_it/public/x.php"}}}}});
        assert_eq!(adapted_violations(&bad_log, Some(&sona)).len(), 1);
        // Globale: niente socket dei siti, radici solo sotto /srv/sites.
        let global = serde_json::json!({"upstreams": [{"dial": "unix//run/php/sona_it.sock"}, {"dial": "example.org:443"}], "root": "/srv/sites/x/public"});
        assert_eq!(adapted_violations(&global, None).len(), 1);
    }

    #[test]
    fn adminer_host_only_runs_the_wrapper_on_its_own_pool() {
        let mut c = cfg();
        c.adminer_host = Some(d("adminer.example.org"));
        let out = render_caddyfile(&c);
        assert!(out.contains("\nhttp://adminer.example.org {\n\troot * /usr/share/adminer\n\trewrite * /index.php\n\tphp_fastcgi unix//run/php/_adminer.sock {\n\t\tenv REMOTE_ADDR {client_ip}\n\t}\n}\n"));
        assert!(!render_caddyfile(&cfg()).contains("adminer"));
    }

    #[test]
    fn global_options_use_admin_socket_and_real_client_ip() {
        let c = render_caddyfile(&cfg());
        assert!(c.starts_with("{\n"));
        assert!(c.contains("\tadmin unix//run/caddy/admin.sock\n"));
        assert!(c.contains("\tauto_https off\n"));
        assert!(c.contains("\t\ttrusted_proxies static private_ranges\n"));
        assert!(c.contains("\t\tclient_ip_headers CF-Connecting-IP\n"));
    }

    #[test]
    fn site_block_serves_primary_and_aliases_from_site_root() {
        let c = render_caddyfile(&cfg());
        assert!(c.contains("\nhttp://sona.it, http://comune-sona.it {\n"));
        assert!(c.contains("\troot * /srv/sites/sona_it/public\n"));
        assert!(c.contains("\tfile_server\n"));
    }

    #[test]
    fn php_goes_to_site_socket_with_real_ip() {
        let c = render_caddyfile(&cfg());
        assert!(c.contains("\tphp_fastcgi unix//run/php/sona_it.sock {\n\t\tenv REMOTE_ADDR {client_ip}\n\t}\n"));
    }

    #[test]
    fn php_in_uploads_is_never_executed() {
        let c = render_caddyfile(&cfg());
        let block = c.find("\t@uploads_php path_regexp (?i)^/wp-content/uploads/.*\\.ph(p[0-9]?|tml|ar)$\n\trespond @uploads_php 403\n").unwrap();
        assert!(block < c.find("\tphp_fastcgi").unwrap(), "il blocco deve precedere php_fastcgi");
    }

    #[test]
    fn access_log_is_json_rotated_in_site_folder() {
        let c = render_caddyfile(&cfg());
        assert!(c.contains("\tlog {\n\t\toutput file /srv/logs/sona_it/access.log {\n\t\t\troll_size 10MiB\n\t\t\troll_keep 5\n\t\t\troll_keep_for 720h\n\t\t}\n\t\tformat json\n\t}\n"));
    }

    #[test]
    fn access_log_retention_follows_the_site_setting() {
        let mut site = sona();
        site.log_days = 7;
        let c = render_caddyfile(&CaddyConfig { panel_host: None, adminer_host: None, global_extra: None, sites: vec![site] });
        assert!(c.contains("\t\t\troll_keep_for 168h\n"), "{c}");
    }

    #[test]
    fn redirect_domains_point_to_primary_over_https() {
        let c = render_caddyfile(&cfg());
        assert!(c.contains("\nhttp://www.sona.it {\n\tredir https://sona.it{uri} permanent\n}\n"));
    }

    #[test]
    fn suspended_site_answers_503_and_never_reaches_php() {
        let mut site = sona();
        site.suspended = true;
        let c = render_caddyfile(&CaddyConfig { panel_host: None, adminer_host: None, global_extra: None, sites: vec![site] });
        let block = &c[c.find("http://sona.it, http://comune-sona.it {").unwrap()..];
        let block = &block[..block.find("\n}\n").unwrap()];
        assert!(block.contains("\theader Retry-After 3600\n"));
        assert!(block.contains("\trespond `"), "{block}");
        assert!(block.contains("` 503"));
        assert!(!block.contains("php_fastcgi") && !block.contains("file_server"));
        // Anche sospeso, il www continua a puntare al dominio principale.
        assert!(c.contains("\nhttp://www.sona.it {\n\tredir https://sona.it{uri} permanent\n}\n"));
    }

    #[test]
    fn staging_site_asks_for_password_and_is_not_indexed() {
        let mut site = sona();
        site.staging_hash = Some("$2b$10$abc".into());
        let c = render_caddyfile(&CaddyConfig { panel_host: None, adminer_host: None, global_extra: None, sites: vec![site] });
        assert!(c.contains("\tbasic_auth {\n\t\tstaging $2b$10$abc\n\t}\n"));
        assert!(c.contains("\theader X-Robots-Tag \"noindex, nofollow\"\n"));
        assert!(!render_caddyfile(&cfg()).contains("basic_auth"));
    }

    #[test]
    fn panel_host_proxies_to_panel() {
        let c = render_caddyfile(&cfg());
        // Il pannello riceve l'IP reale già risolto da Caddy (blocco tentativi per IP).
        assert!(c.contains("\nhttp://pannello.example.org {\n\treverse_proxy panel:8080 {\n\t\theader_up X-Real-IP {client_ip}\n\t}\n}\n"));
    }

    #[test]
    fn output_is_stable_regardless_of_input_order() {
        let mut other = sona();
        other.slug = Slug::parse("aaa_it").unwrap();
        other.primary = d("aaa.it");
        other.aliases.clear();
        other.redirects.clear();
        let a = CaddyConfig { panel_host: None, adminer_host: None, global_extra: None, sites: vec![sona(), other.clone()] };
        let b = CaddyConfig { panel_host: None, adminer_host: None, global_extra: None, sites: vec![other, sona()] };
        assert_eq!(render_caddyfile(&a), render_caddyfile(&b));
    }
}
