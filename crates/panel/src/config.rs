//! Configurazione dal `.env` del pannello. Le variabili `SITE_ENV_*` finiscono, senza
//! prefisso, nel `.env` di ogni sito (credenziali SMTP di SES).
use alveare_core::site::Domain;
use anyhow::{bail, Context, Result};

use crate::secrets::SecretKey;
use crate::template::Vars;

pub struct Config {
    pub listen: String,
    pub key: SecretKey,
    pub db_host: String,
    pub mysql_url: String,
    pub root_password: String,
    pub panel_host: Option<Domain>,
    pub adminer_host: Option<Domain>,
    pub admin_email: Option<String>,
    pub secure_cookie: bool,
    pub site_env: Vars,
    /// restic legge da solo RESTIC_REPOSITORY, RESTIC_PASSWORD e AWS_* dall'ambiente.
    pub restic_repository: Option<String>,
    /// Valori per il kit di ripristino.
    pub recovery: Vec<(String, String)>,
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_vars(std::env::vars().collect())
    }

    pub fn from_vars(vars: Vec<(String, String)>) -> Result<Self> {
        let get = |k: &str| vars.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
        let key = SecretKey::from_base64(&get("ALVEARE_SECRET_KEY").context("ALVEARE_SECRET_KEY mancante")?)
            .context("ALVEARE_SECRET_KEY: servono 32 byte in base64 (openssl rand -base64 32)")?;
        let root = get("MARIADB_ROOT_PASSWORD").context("MARIADB_ROOT_PASSWORD mancante")?;
        let db_host = get("ALVEARE_DB_HOST").unwrap_or_else(|| "mariadb".into());
        let host = |key: &str| -> Result<Option<Domain>> {
            match get(key) {
                Some(h) => Ok(Some(Domain::parse(&h).map_err(|_| anyhow::anyhow!("{key} non è un dominio: {h}"))?)),
                None => Ok(None),
            }
        };
        let panel_host = host("ALVEARE_PANEL_HOST")?;
        let adminer_host = host("ALVEARE_ADMINER_HOST")?;
        let restic_repository = get("RESTIC_REPOSITORY");
        if restic_repository.is_some() && get("RESTIC_PASSWORD").is_none() {
            bail!("RESTIC_PASSWORD mancante: serve insieme a RESTIC_REPOSITORY");
        }
        let recovery = ["ALVEARE_SECRET_KEY", "RESTIC_REPOSITORY", "RESTIC_PASSWORD", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_DEFAULT_REGION"]
            .iter()
            .filter_map(|k| Some((k.to_string(), get(k)?)))
            .collect();
        let secure_cookie = match get("ALVEARE_INSECURE_COOKIE").as_deref() {
            None | Some("0") | Some("false") => true,
            Some("1") | Some("true") => false,
            Some(other) => bail!("ALVEARE_INSECURE_COOKIE: valore non valido {other}"),
        };
        Ok(Self {
            listen: get("ALVEARE_LISTEN").unwrap_or_else(|| "0.0.0.0:8080".into()),
            key,
            mysql_url: format!("mysql://root:{}@{db_host}:3306/mysql", url_encode(&root)),
            root_password: root,
            db_host,
            panel_host,
            adminer_host,
            admin_email: get("ALVEARE_ADMIN_EMAIL"),
            secure_cookie,
            restic_repository,
            recovery,
            site_env: vars.iter().filter_map(|(k, v)| Some((k.strip_prefix("SITE_ENV_")?.to_string(), v.clone()))).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

    fn vars(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        [("ALVEARE_SECRET_KEY", KEY), ("MARIADB_ROOT_PASSWORD", "root")]
            .iter()
            .chain(extra)
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_are_applied() {
        let c = Config::from_vars(vars(&[])).unwrap();
        assert_eq!(c.listen, "0.0.0.0:8080");
        assert_eq!(c.db_host, "mariadb");
        assert_eq!(c.mysql_url, "mysql://root:root@mariadb:3306/mysql");
        assert!(c.panel_host.is_none());
        assert!(c.secure_cookie);
    }

    #[test]
    fn secret_key_and_root_password_are_required() {
        assert!(Config::from_vars(vec![("MARIADB_ROOT_PASSWORD".into(), "x".into())]).is_err());
        assert!(Config::from_vars(vec![("ALVEARE_SECRET_KEY".into(), KEY.into())]).is_err());
        assert!(Config::from_vars(vars(&[("ALVEARE_SECRET_KEY", "corta")])).is_err());
    }

    #[test]
    fn site_env_prefix_is_copied_into_every_site() {
        let c = Config::from_vars(vars(&[("SITE_ENV_SMTP_HOST", "email-smtp.eu-west-1.amazonaws.com"), ("ALTRO", "x")])).unwrap();
        assert_eq!(c.site_env.get("SMTP_HOST").map(String::as_str), Some("email-smtp.eu-west-1.amazonaws.com"));
        assert_eq!(c.site_env.len(), 1);
    }

    #[test]
    fn panel_host_must_be_a_domain() {
        assert!(Config::from_vars(vars(&[("ALVEARE_PANEL_HOST", "pannello.example.org")])).unwrap().panel_host.is_some());
        assert!(Config::from_vars(vars(&[("ALVEARE_PANEL_HOST", "non valido")])).is_err());
    }

    #[test]
    fn adminer_host_is_optional_and_validated() {
        assert!(Config::from_vars(vars(&[])).unwrap().adminer_host.is_none());
        assert!(Config::from_vars(vars(&[("ALVEARE_ADMINER_HOST", "adminer.example.org")])).unwrap().adminer_host.is_some());
        assert!(Config::from_vars(vars(&[("ALVEARE_ADMINER_HOST", "x")])).is_err());
    }

    #[test]
    fn restic_needs_a_password_and_feeds_the_recovery_kit() {
        assert!(Config::from_vars(vars(&[("RESTIC_REPOSITORY", "s3:https://r2/x")])).is_err());
        let c = Config::from_vars(vars(&[("RESTIC_REPOSITORY", "s3:https://r2/x"), ("RESTIC_PASSWORD", "p")])).unwrap();
        assert_eq!(c.restic_repository.as_deref(), Some("s3:https://r2/x"));
        let keys: Vec<&str> = c.recovery.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["ALVEARE_SECRET_KEY", "RESTIC_REPOSITORY", "RESTIC_PASSWORD"]);
    }

    #[test]
    fn root_password_is_url_encoded() {
        let c = Config::from_vars(vars(&[("MARIADB_ROOT_PASSWORD", "a@b/c")])).unwrap();
        assert_eq!(c.mysql_url, "mysql://root:a%40b%2Fc@mariadb:3306/mysql");
    }
}
