//! Aggiornamenti di WordPress: versioni lette con WP-CLI, aggiornamenti con backup prima e
//! l'elenco delle versioni prima e dopo per l'audit log.
use alveare_core::site::Slug;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::flows::{Ctx, Host, JobLog};
use crate::store::{SiteKind, UpdateRule};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    #[serde(default)]
    pub status: String,
    pub version: String,
    /// Versione disponibile, se c'è un aggiornamento.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub update_version: Option<String>,
}

fn empty_as_none<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Option::<String>::deserialize(d)?;
    Ok(v.filter(|s| !s.is_empty()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub core: String,
    pub core_update: Option<String>,
    /// Tutte le versioni proposte: la prima può essere una major, con una minore dopo.
    #[serde(default)]
    pub core_updates: Vec<String>,
    pub plugins: Vec<Item>,
    pub themes: Vec<Item>,
}

impl Status {
    pub fn pending(&self) -> usize {
        usize::from(self.core_update.is_some()) + self.plugins.iter().chain(&self.themes).filter(|i| i.update_version.is_some()).count()
    }
}

/// L'output JSON di WP-CLI può essere preceduto da avvisi PHP: si parte dal primo `[`.
pub fn parse_items(out: &str) -> Vec<Item> {
    let start = out.find('[').unwrap_or(out.len());
    serde_json::from_str(&out[start..]).unwrap_or_default()
}

/// `wp core check-update --format=json`: le versioni proposte, dalla più alta.
pub fn parse_core_updates(out: &str) -> Vec<String> {
    let start = out.find('[').unwrap_or(out.len());
    let v: Vec<serde_json::Value> = serde_json::from_str(&out[start..]).unwrap_or_default();
    v.iter().filter_map(|u| u["version"].as_str().map(String::from)).collect()
}

/// "plugin akismet: 5.3 → 5.4" per ogni versione cambiata; anche core e temi.
pub fn changes(before: &Status, after: &Status) -> Vec<String> {
    let mut out = Vec::new();
    if before.core != after.core {
        out.push(format!("core: {} → {}", before.core, after.core));
    }
    for (kind, b, a) in [("plugin", &before.plugins, &after.plugins), ("tema", &before.themes, &after.themes)] {
        for item in a {
            if let Some(old) = b.iter().find(|x| x.name == item.name).filter(|x| x.version != item.version) {
                out.push(format!("{kind} {}: {} → {}", item.name, old.version, item.version));
            }
        }
    }
    out
}

/// Nomi di plugin e temi: arrivano dalla UI e finiscono come argomenti di WP-CLI.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 100 && !n.starts_with('-') && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Cosa aggiornare di plugin o temi.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Selection {
    #[default]
    None,
    All,
    Names(Vec<String>),
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateReq {
    #[serde(default)]
    pub core: bool,
    /// Solo versioni minori del core (regola "solo sicurezza").
    #[serde(default)]
    pub minor_only: bool,
    #[serde(default)]
    pub plugins: Selection,
    #[serde(default)]
    pub themes: Selection,
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

pub async fn status<H: Host>(c: &Ctx<H>, slug: &Slug) -> Result<Status> {
    let fields = "--fields=name,status,version,update_version";
    let core = c.host.wp(slug, &args(&["core", "version"])).await?.trim().to_string();
    let (_, check) = c.host.wp_output(slug, &args(&["core", "check-update", "--format=json"])).await?;
    let plugins = parse_items(&c.host.wp(slug, &args(&["plugin", "list", "--format=json", fields])).await?);
    let themes = parse_items(&c.host.wp(slug, &args(&["theme", "list", "--format=json", fields])).await?);
    let core_updates = parse_core_updates(&check);
    Ok(Status { core, core_update: core_updates.first().cloned(), core_updates, plugins, themes })
}

fn selection_args(kind: &str, sel: &Selection) -> Result<Option<Vec<String>>> {
    Ok(match sel {
        Selection::None => None,
        Selection::All => Some(args(&[kind, "update", "--all"])),
        Selection::Names(names) => {
            if let Some(bad) = names.iter().find(|n| !valid_name(n)) {
                bail!(crate::flows::Invalid(format!("nome non valido: {bad}")));
            }
            if names.is_empty() {
                return Ok(None);
            }
            Some(args(&[kind, "update"]).into_iter().chain(names.iter().cloned()).collect())
        }
    })
}

/// Aggiornamento con backup prima (se configurati): restituisce le versioni cambiate.
pub async fn update<H: Host>(c: &Ctx<H>, id: i64, req: &UpdateReq, log: &mut JobLog) -> Result<Vec<String>> {
    let plugins = selection_args("plugin", &req.plugins)?;
    let themes = selection_args("theme", &req.themes)?;
    let site = c.store.site(id).await?;
    if site.kind != SiteKind::Wordpress {
        bail!(crate::flows::Invalid("il sito non è WordPress".into()));
    }
    if c.backups {
        crate::flows::backup_site(c, id, log).await.context("backup prima dell'aggiornamento")?;
    } else {
        log.push("ATTENZIONE: backup non configurati, aggiornamento senza backup");
    }
    let before = status(c, &site.slug).await?;
    if req.core {
        let mut cmd = args(&["core", "update"]);
        if req.minor_only {
            cmd.push("--minor".into());
        }
        log.push(c.host.wp(&site.slug, &cmd).await?.lines().last().unwrap_or_default().to_string());
        c.host.wp(&site.slug, &args(&["core", "update-db"])).await?;
    }
    for cmd in plugins.into_iter().chain(themes) {
        let (ok, out) = c.host.wp_output(&site.slug, &cmd).await?;
        log.push(out.lines().last().unwrap_or_default().to_string());
        if !ok {
            log.push(format!("wp {} non è riuscito del tutto", cmd.join(" ")));
        }
    }
    let after = status(c, &site.slug).await?;
    let changed = changes(&before, &after);
    if changed.is_empty() {
        log.push("nessuna versione cambiata");
    }
    for line in &changed {
        log.push(line.clone());
    }
    Ok(changed)
}

/// Aggiornamento notturno secondo la regola del sito, dopo il backup.
pub async fn nightly<H: Host>(c: &Ctx<H>, id: i64, log: &mut JobLog) -> Result<Vec<String>> {
    let site = c.store.site(id).await?;
    let req = match site.update_rule {
        UpdateRule::Auto => UpdateReq { core: true, minor_only: false, plugins: Selection::All, themes: Selection::All },
        UpdateRule::Security => UpdateReq { core: true, minor_only: true, ..Default::default() },
        UpdateRule::Manual => return Ok(vec![]),
    };
    let st = status(c, &site.slug).await?;
    let core_due = core_due(&st, req.minor_only);
    let others_due = req.plugins == Selection::All && st.pending() > usize::from(st.core_update.is_some());
    if !core_due && !others_due {
        log.push("niente da aggiornare");
        return Ok(vec![]);
    }
    update(c, id, &req, log).await
}

/// Con "solo minori" basta una versione proposta sullo stesso ramo, anche dopo una major.
pub fn core_due(st: &Status, minor_only: bool) -> bool {
    st.core_updates.iter().any(|v| !minor_only || same_branch(&st.core, v))
}

/// "6.8.1" e "6.8.2" sono sullo stesso ramo (aggiornamento minore); "6.9" no.
pub fn same_branch(current: &str, next: &str) -> bool {
    let branch = |v: &str| v.split('.').take(2).collect::<Vec<_>>().join(".");
    branch(current) == branch(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_list_with_php_warnings_is_parsed() {
        let out = "PHP Deprecated: x in y\n[{\"name\":\"akismet\",\"status\":\"active\",\"version\":\"5.3\",\"update_version\":\"5.4\"},{\"name\":\"hello\",\"status\":\"inactive\",\"version\":\"1.7\",\"update_version\":\"\"}]";
        let items = parse_items(out);
        assert_eq!(items[0].update_version.as_deref(), Some("5.4"));
        assert_eq!(items[1].update_version, None);
        assert!(parse_items("Error: niente").is_empty());
    }

    #[test]
    fn core_updates_are_all_offered_versions() {
        assert_eq!(parse_core_updates("[{\"version\":\"6.8.2\",\"update_type\":\"minor\",\"package_url\":\"x\"}]"), ["6.8.2"]);
        assert!(parse_core_updates("[]").is_empty());
        assert!(parse_core_updates("Success: WordPress is at the latest version.").is_empty());
    }

    #[test]
    fn a_minor_core_update_is_due_even_when_a_major_is_offered_first() {
        let st = |updates: &[&str]| Status { core: "6.8.2".into(), core_update: updates.first().map(|v| v.to_string()), core_updates: updates.iter().map(|v| v.to_string()).collect(), plugins: vec![], themes: vec![] };
        assert!(core_due(&st(&["6.9", "6.8.3"]), true), "la 6.8.3 è sicurezza");
        assert!(!core_due(&st(&["6.9"]), true));
        assert!(core_due(&st(&["6.9"]), false));
        assert!(!core_due(&st(&[]), false));
    }

    #[test]
    fn changes_list_versions_before_and_after() {
        let item = |n: &str, v: &str| Item { name: n.into(), status: "active".into(), version: v.into(), update_version: None };
        let before = Status { core: "6.8.1".into(), core_update: Some("6.8.2".into()), core_updates: vec!["6.8.2".into()], plugins: vec![item("akismet", "5.3"), item("hello", "1.7")], themes: vec![item("tt5", "1.0")] };
        let after = Status { core: "6.8.2".into(), core_update: None, core_updates: vec![], plugins: vec![item("akismet", "5.4"), item("hello", "1.7")], themes: vec![item("tt5", "1.1")] };
        assert_eq!(changes(&before, &after), ["core: 6.8.1 → 6.8.2", "plugin akismet: 5.3 → 5.4", "tema tt5: 1.0 → 1.1"]);
        assert_eq!(before.pending(), 1);
    }

    use crate::flows::{create_site, CreateSite};
    use crate::secrets::SecretKey;
    use crate::store::Store;
    use crate::testing::FakeHost;
    use alveare_core::site::Domain;

    const PLUGINS_OLD: &str = r#"[{"name":"akismet","status":"active","version":"5.3","update_version":"5.4"}]"#;

    async fn wp_site(host: FakeHost) -> (Ctx<FakeHost>, i64) {
        let c = Ctx {
            store: Store::open_memory().await.unwrap(),
            host,
            key: SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap(),
            db_host: "mariadb".into(),
            global_env: Default::default(),
            backups: true,
            site_url: crate::flows::DEFAULT_SITE_URL.into(),
        };
        let req = CreateSite { domain: Domain::parse("sona.it").unwrap(), kind: SiteKind::Wordpress, template: "wordpress".into(), title: "S".into(), admin_email: "a@b.it".into(), admin_user: "admin".into(), table_prefix: "wp_".into() };
        let site = create_site(&c, &req, &mut JobLog::default()).await.unwrap();
        c.host.calls.lock().unwrap().clear();
        (c, site.id)
    }

    #[tokio::test]
    async fn update_backs_up_first_and_runs_only_selected_parts() {
        let host = FakeHost { wp_replies: vec![("core version", "6.8.1\n"), ("plugin list --format=json --fields=name,status,version,update_version", PLUGINS_OLD)], ..Default::default() };
        let (c, id) = wp_site(host).await;
        let req = UpdateReq { plugins: Selection::Names(vec!["akismet".into()]), ..Default::default() };
        update(&c, id, &req, &mut JobLog::default()).await.unwrap();
        let calls = c.host.calls();
        assert_eq!(calls[0], "backup sona_it sona_it");
        assert!(calls.contains(&"wp sona_it plugin update akismet".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|x| x.contains("core update")), "{calls:?}");
        let bad = UpdateReq { plugins: Selection::Names(vec!["--all".into()]), ..Default::default() };
        assert!(update(&c, id, &bad, &mut JobLog::default()).await.unwrap_err().downcast_ref::<crate::flows::Invalid>().is_some());
    }

    #[tokio::test]
    async fn nightly_follows_the_site_rule() {
        let host = FakeHost {
            wp_replies: vec![("core version", "6.8.1"), ("core check-update --format=json", r#"[{"version":"6.9","update_type":"major"}]"#), ("plugin list --format=json --fields=name,status,version,update_version", PLUGINS_OLD)],
            ..Default::default()
        };
        let (c, id) = wp_site(host).await;
        // Solo sicurezza (default): 6.9 è un ramo nuovo e i plugin restano manuali.
        assert!(nightly(&c, id, &mut JobLog::default()).await.unwrap().is_empty());
        assert!(!c.host.ops().contains(&"backup".to_string()));
        c.store.set_update_rule(id, UpdateRule::Auto).await.unwrap();
        nightly(&c, id, &mut JobLog::default()).await.unwrap();
        let calls = c.host.calls();
        assert!(calls.contains(&"wp sona_it core update".to_string()) && calls.contains(&"wp sona_it plugin update --all".to_string()), "{calls:?}");
        c.host.calls.lock().unwrap().clear();
        c.store.set_update_rule(id, UpdateRule::Manual).await.unwrap();
        nightly(&c, id, &mut JobLog::default()).await.unwrap();
        assert!(c.host.calls().is_empty());
        assert!(same_branch("6.8.1", "6.8.3") && !same_branch("6.8.1", "6.9"));
    }

    #[test]
    fn names_cannot_become_options() {
        assert!(valid_name("redis-cache") && valid_name("twentytwentyfive"));
        for bad in ["--all", "a b", "../x", "", "a;b"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
