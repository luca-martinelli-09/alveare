//! Stampa pool, passwd o group per un sito: `cargo run --example pool -- <slug> <uid> pool|passwd|group`.
use alveare_core::php::{render_group, render_passwd, render_pool, PhpSettings, SitePhp};
use alveare_core::site::Slug;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let site = SitePhp { slug: Slug::parse(&a[1]).unwrap(), uid: a[2].parse().unwrap(), settings: PhpSettings::default(), extra: vec![], runtime: alveare_core::php::DEFAULT_RUNTIME.into() };
    match a[3].as_str() {
        "pool" => print!("{}", render_pool(&site)),
        "passwd" => print!("{}", render_passwd(&[&site])),
        _ => print!("{}", render_group(&[&site])),
    }
}
