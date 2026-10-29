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
    pub panel_host: Option<Domain>,
    pub admin_email: Option<String>,
    pub secure_cookie: bool,
    pub site_env: Vars,
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
        let panel_host = match get("ALVEARE_PANEL_HOST") {
            Some(h) => Some(Domain::parse(&h).map_err(|_| anyhow::anyhow!("ALVEARE_PANEL_HOST non è un dominio: {h}"))?),
            None => None,
        };
        let secure_cookie = match get("ALVEARE_INSECURE_COOKIE").as_deref() {
            None | Some("0") | Some("false") => true,
            Some("1") | Some("true") => false,
            Some(other) => bail!("ALVEARE_INSECURE_COOKIE: valore non valido {other}"),
        };
        Ok(Self {
            listen: get("ALVEARE_LISTEN").unwrap_or_else(|| "0.0.0.0:8080".into()),
            key,
            mysql_url: format!("mysql://root:{}@{db_host}:3306/mysql", url_encode(&root)),
            db_host,
            panel_host,
            admin_email: get("ALVEARE_ADMIN_EMAIL"),
            secure_cookie,
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
    fn root_password_is_url_encoded() {
        let c = Config::from_vars(vars(&[("MARIADB_ROOT_PASSWORD", "a@b/c")])).unwrap();
        assert_eq!(c.mysql_url, "mysql://root:a%40b%2Fc@mariadb:3306/mysql");
    }
}
