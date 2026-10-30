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
            suspended: false,
            staging_hash: None,
            log_days: 30,
            extra: None,
            blocked: vec!["198.51.100.9".into(), "203.0.113.0/24".into()],
            runtime: "8.4".into(),
        }, CaddySite {
            slug: Slug::parse("prove_sona_it").unwrap(),
            primary: d("prove.sona.it"),
            aliases: vec![],
            redirects: vec![],
            suspended: false,
            staging_hash: Some("$2a$14$BkqIzVh37faSU/dIkEUdy.mS7peZY5xDpdqsI7SPhnPuQ0V223JNi".into()),
            log_days: 7,
            blocked: vec![],
            runtime: "8.5".into(),
            extra: Some("header Cache-Control \"max-age=60\"\nimport sicurezza".into()),
        }, CaddySite {
            slug: Slug::parse("chiuso_it").unwrap(),
            primary: d("chiuso.it"),
            aliases: vec![],
            redirects: vec![d("www.chiuso.it")],
            suspended: true,
            staging_hash: None,
            log_days: 30,
            extra: None,
            blocked: vec![],
            runtime: "8.5".into(),
        }],
        global_extra: Some("(sicurezza) {\n\theader X-Frame-Options DENY\n}".into()),
    };
    print!("{}", render_caddyfile(&cfg));
}
