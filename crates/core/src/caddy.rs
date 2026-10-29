use std::fmt::Write;

use crate::php::{socket_path, LOGS_DIR, SITES_DIR};
use crate::site::{Domain, Slug};

pub const ADMIN_SOCKET: &str = "/run/caddy/admin.sock";

#[derive(Debug, Clone)]
pub struct CaddySite {
    pub slug: Slug,
    pub primary: Domain,
    pub aliases: Vec<Domain>,
    pub redirects: Vec<Domain>,
}

pub struct CaddyConfig {
    pub panel_host: Option<Domain>,
    pub sites: Vec<CaddySite>,
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
    if let Some(host) = &cfg.panel_host {
        write!(out, "\nhttp://{} {{\n\treverse_proxy panel:8080\n}}\n", host.as_str()).unwrap();
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
    write!(
        out,
        "
{hosts} {{
	root * {SITES_DIR}/{s}/public
	log {{
		output file {LOGS_DIR}/{s}/access.log {{
			roll_size 10MiB
			roll_keep 5
			roll_keep_for 720h
		}}
		format json
	}}
	@uploads_php path_regexp (?i)^/wp-content/uploads/.*\\.ph(p[0-9]?|tml|ar)$
	respond @uploads_php 403
	php_fastcgi unix/{socket} {{
		env REMOTE_ADDR {{client_ip}}
	}}
	file_server
}}
",
        hosts = hosts.join(", "),
        socket = socket_path(&site.slug),
    )
    .unwrap();
    for r in &site.redirects {
        write!(out, "\nhttp://{} {{\n\tredir https://{}{{uri}} permanent\n}}\n", r.as_str(), site.primary.as_str()).unwrap();
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
        }
    }

    fn cfg() -> CaddyConfig {
        CaddyConfig { panel_host: Some(d("pannello.example.org")), sites: vec![sona()] }
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
    fn redirect_domains_point_to_primary_over_https() {
        let c = render_caddyfile(&cfg());
        assert!(c.contains("\nhttp://www.sona.it {\n\tredir https://sona.it{uri} permanent\n}\n"));
    }

    #[test]
    fn panel_host_proxies_to_panel() {
        let c = render_caddyfile(&cfg());
        assert!(c.contains("\nhttp://pannello.example.org {\n\treverse_proxy panel:8080\n}\n"));
    }

    #[test]
    fn output_is_stable_regardless_of_input_order() {
        let mut other = sona();
        other.slug = Slug::parse("aaa_it").unwrap();
        other.primary = d("aaa.it");
        other.aliases.clear();
        other.redirects.clear();
        let a = CaddyConfig { panel_host: None, sites: vec![sona(), other.clone()] };
        let b = CaddyConfig { panel_host: None, sites: vec![other, sona()] };
        assert_eq!(render_caddyfile(&a), render_caddyfile(&b));
    }
}
