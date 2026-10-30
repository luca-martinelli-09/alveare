//! Template dei siti: una cartella con `template.toml` e `files/`.
//! I placeholder `{{NOME}}` si sostituiscono solo nei file elencati in `render`:
//! il core di WordPress contiene template JS con la stessa sintassi.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

pub type Vars = BTreeMap<String, String>;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub kind: crate::store::SiteKind,
    #[serde(default)]
    pub render: Vec<PathBuf>,
    #[serde(default)]
    pub post_create: Vec<Vec<String>>,
    /// Template snapshot: dump (.sql o .sql.gz) esportato con il dominio fittizio `sql_domain`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql: Option<PathBuf>,
    #[serde(default = "default_sql_domain")]
    pub sql_domain: String,
    /// Prefisso delle tabelle del dump, se non è `wp_`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_prefix: Option<String>,
}

fn default_sql_domain() -> String {
    "template.local".into()
}

/// Prefisso delle tabelle WordPress: finisce nel .env e nelle query, quindi solo [A-Za-z0-9_].
pub fn valid_table_prefix(p: &str) -> bool {
    !p.is_empty() && p.len() <= 20 && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub struct Template {
    dir: PathBuf,
    pub manifest: Manifest,
}

fn is_plain_relative(p: &Path) -> bool {
    p.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Sostituisce `{{NOME}}` (maiuscole, cifre, `_`); un nome sconosciuto è un errore.
pub fn render_str(s: &str, vars: &Vars) -> Result<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}");
        let name = end.map(|e| &after[..e]);
        match name {
            Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') => {
                let v = vars.get(n).with_context(|| format!("placeholder sconosciuto: {n}"))?;
                out.push_str(v);
                rest = &after[n.len() + 2..];
            }
            _ => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

impl Template {
    pub fn load(dir: &Path) -> Result<Self> {
        let text = fs::read_to_string(dir.join("template.toml")).context("lettura di template.toml")?;
        let manifest: Manifest = toml::from_str(&text).context("template.toml non valido")?;
        if let Some(p) = manifest.render.iter().chain(&manifest.sql).find(|p| !is_plain_relative(p)) {
            bail!("percorso non valido nel template: {}", p.display());
        }
        if manifest.table_prefix.as_deref().is_some_and(|p| !valid_table_prefix(p)) {
            bail!("prefisso delle tabelle non valido");
        }
        Ok(Self { dir: dir.to_path_buf(), manifest })
    }

    /// Copia `files/` in `dest` e renderizza i file elencati.
    pub fn install(&self, dest: &Path, vars: &Vars) -> Result<()> {
        copy_tree(&self.dir.join("files"), dest)?;
        for rel in &self.manifest.render {
            let path = dest.join(rel);
            let text = fs::read_to_string(&path).with_context(|| format!("lettura di {}", rel.display()))?;
            let rendered = render_str(&text, vars).with_context(|| format!("render di {}", rel.display()))?;
            fs::write(&path, rendered)?;
        }
        Ok(())
    }

    pub fn sql_path(&self) -> Option<PathBuf> {
        self.manifest.sql.as_ref().map(|p| self.dir.join(p))
    }

    pub fn post_create(&self, vars: &Vars) -> Result<Vec<Vec<String>>> {
        self.manifest.post_create.iter().map(|cmd| cmd.iter().map(|a| render_str(a, vars)).collect()).collect()
    }
}

pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            bail!("link simbolico nel template: {}", entry.path().display());
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Template dell'immagine: cartella da cui il primo avvio copia quelli predefiniti.
pub const IMAGE_TEMPLATES: &str = "/usr/share/alveare/templates";

/// Primo avvio con la cartella dei template vuota (bind mount creato da Docker): copia quelli
/// dell'immagine. Se c'è già un template non tocca niente, così quelli eliminati non tornano.
pub fn seed_defaults(dir: &Path, defaults: &Path) -> Result<bool> {
    let has_any = fs::read_dir(dir).is_ok_and(|d| d.flatten().any(|e| e.path().join("template.toml").is_file()));
    if has_any || !defaults.is_dir() {
        return Ok(false);
    }
    copy_tree(defaults, dir)?;
    Ok(true)
}

/// `.env` del sito, fuori dalla document root.
pub fn env_file(vars: &Vars) -> Result<String> {
    vars.iter()
        .map(|(k, v)| {
            if v.contains(['\'', '\n', '\r']) {
                bail!("valore non scrivibile nel .env: {k}");
            }
            Ok(format!("{k}='{v}'\n"))
        })
        .collect()
}

/// Legge un `.env` scritto da `env_file` (o a mano): `KEY='v'`, `KEY="v"` o `KEY=v`.
pub fn parse_env(text: &str) -> Vars {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            let v = v.trim();
            let unquoted = [('\'', '\''), ('"', '"')]
                .iter()
                .find_map(|(a, b)| v.strip_prefix(*a).and_then(|x| x.strip_suffix(*b)))
                .unwrap_or(v);
            (k.trim().to_string(), unquoted.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_templates_fill_only_an_empty_folder() {
        let defaults = tempfile::tempdir().unwrap();
        fs::create_dir_all(defaults.path().join("wordpress")).unwrap();
        fs::write(defaults.path().join("wordpress/template.toml"), "name = \"WordPress\"\nkind = \"wordpress\"\n").unwrap();
        let empty = tempfile::tempdir().unwrap();
        assert!(seed_defaults(empty.path(), defaults.path()).unwrap());
        assert!(empty.path().join("wordpress/template.toml").is_file());
        // Con un template già presente non si tocca niente: quelli eliminati non tornano.
        let used = tempfile::tempdir().unwrap();
        fs::create_dir_all(used.path().join("mio")).unwrap();
        fs::write(used.path().join("mio/template.toml"), "name = \"Mio\"\nkind = \"php\"\n").unwrap();
        assert!(!seed_defaults(used.path(), defaults.path()).unwrap());
        assert!(!used.path().join("wordpress").exists());
    }
    use std::fs;

    fn vars() -> Vars {
        Vars::from([("DOMAIN".to_string(), "sona.it".to_string()), ("DB_NAME".to_string(), "sona_it".to_string())])
    }

    fn template(manifest: &str) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("template.toml"), manifest).unwrap();
        fs::create_dir_all(t.path().join("files/public/js")).unwrap();
        fs::write(t.path().join("files/public/config.php"), "dom={{DOMAIN}} db={{DB_NAME}}").unwrap();
        fs::write(t.path().join("files/public/js/tpl.js"), "<b>{{DOMAIN}}</b> {{ data.x }}").unwrap();
        t
    }

    const MANIFEST: &str = r#"
name = "Prova"
kind = "wordpress"
render = ["public/config.php"]
post_create = [["option", "update", "home", "https://{{DOMAIN}}"]]
"#;

    #[test]
    fn placeholders_are_replaced_only_in_listed_files() {
        let t = template(MANIFEST);
        let dest = tempfile::tempdir().unwrap();
        let tpl = Template::load(t.path()).unwrap();
        tpl.install(dest.path(), &vars()).unwrap();
        assert_eq!(fs::read_to_string(dest.path().join("public/config.php")).unwrap(), "dom=sona.it db=sona_it");
        assert_eq!(fs::read_to_string(dest.path().join("public/js/tpl.js")).unwrap(), "<b>{{DOMAIN}}</b> {{ data.x }}");
    }

    #[test]
    fn post_create_commands_are_rendered() {
        let t = template(MANIFEST);
        let tpl = Template::load(t.path()).unwrap();
        assert_eq!(tpl.post_create(&vars()).unwrap(), vec![vec!["option", "update", "home", "https://sona.it"]]);
    }

    #[test]
    fn unknown_placeholder_is_an_error() {
        let t = template(MANIFEST);
        fs::write(t.path().join("files/public/config.php"), "{{DOMIAN}}").unwrap();
        let dest = tempfile::tempdir().unwrap();
        let err = Template::load(t.path()).unwrap().install(dest.path(), &vars()).unwrap_err();
        assert!(format!("{err:#}").contains("DOMIAN"), "{err:#}");
    }

    #[test]
    fn render_paths_outside_template_are_rejected() {
        let t = template(&MANIFEST.replace("public/config.php", "../../etc/passwd"));
        assert!(Template::load(t.path()).is_err());
        let t = template(&MANIFEST.replace("public/config.php", "/etc/passwd"));
        assert!(Template::load(t.path()).is_err());
    }

    #[test]
    fn symlinks_in_template_are_rejected() {
        let t = template(MANIFEST);
        std::os::unix::fs::symlink("/etc/passwd", t.path().join("files/public/link")).unwrap();
        let dest = tempfile::tempdir().unwrap();
        assert!(Template::load(t.path()).unwrap().install(dest.path(), &vars()).is_err());
    }

    #[test]
    fn snapshot_manifest_points_to_its_dump() {
        let t = template("name = \"Pro Loco\"\nkind = \"wordpress\"\nsql = \"db.sql.gz\"\ntable_prefix = \"pl_\"\n");
        let tpl = Template::load(t.path()).unwrap();
        assert_eq!(tpl.sql_path(), Some(t.path().join("db.sql.gz")));
        assert_eq!(tpl.manifest.sql_domain, "template.local");
        assert_eq!(tpl.manifest.table_prefix.as_deref(), Some("pl_"));
        assert_eq!(Template::load(template(MANIFEST).path()).unwrap().sql_path(), None);
        assert!(Template::load(template("name = \"x\"\nkind = \"wordpress\"\nsql = \"../db.sql\"\n").path()).is_err());
        assert!(Template::load(template("name = \"x\"\nkind = \"wordpress\"\ntable_prefix = \"x'; --\"\n").path()).is_err());
    }

    #[test]
    fn saved_manifest_loads_back() {
        let m = Manifest { name: "N".into(), kind: crate::store::SiteKind::Wordpress, render: vec![], post_create: vec![], sql: Some("db.sql".into()), sql_domain: "template.local".into(), table_prefix: None };
        let t = template(&toml::to_string(&m).unwrap());
        let tpl = Template::load(t.path()).unwrap();
        assert_eq!(tpl.sql_path(), Some(t.path().join("db.sql")));
    }

    #[test]
    fn env_file_single_quotes_every_value() {
        // Apici singoli: phpdotenv non espande `$` né interpreta `\\`.
        assert_eq!(env_file(&vars()).unwrap(), "DB_NAME='sona_it'\nDOMAIN='sona.it'\n");
    }

    #[test]
    fn env_file_parses_back_to_the_same_values() {
        let v = Vars::from([("A".to_string(), "x y".to_string()), ("B".to_string(), String::new())]);
        assert_eq!(parse_env(&env_file(&v).unwrap()), v);
        assert_eq!(parse_env("# commento\nC=nudo\nD=\"doppi\"\n\n"), Vars::from([("C".into(), "nudo".into()), ("D".into(), "doppi".into())]));
    }

    #[test]
    fn env_file_rejects_values_that_break_quoting() {
        for bad in ["a'b", "a\nb"] {
            let v = Vars::from([("X".to_string(), bad.to_string())]);
            assert!(env_file(&v).is_err(), "accettato: {bad:?}");
        }
    }
}
