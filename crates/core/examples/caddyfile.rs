//! Stampa un Caddyfile di esempio, per validarlo con `caddy adapt`.
use alveare_core::caddy::{render_caddyfile, CaddyConfig, CaddySite};
use alveare_core::site::{Domain, Slug};

fn main() {
    let d = |s: &str| Domain::parse(s).unwrap();
    let cfg = CaddyConfig {
        panel_host: Some(d("pannello.example.org")),
        adminer_host: Some(d("adminer.example.org")),
        sites: vec![CaddySite {
            slug: Slug::parse("sona_it").unwrap(),
            primary: d("sona.it"),
            aliases: vec![d("comune-sona.it")],
            redirects: vec![d("www.sona.it")],
        }],
    };
    print!("{}", render_caddyfile(&cfg));
}
