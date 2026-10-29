//! Pagina database del sito: regole pure (nomi, .env, tipo di dump). Le query vere stanno nell'host.
use anyhow::{bail, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TableInfo {
    pub name: String,
    pub rows: u64,
    pub bytes: u64,
}

/// Nomi di tabella che finiscono come argomenti di mariadb-dump.
pub fn valid_table(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

/// `Some(true)` = gzip, `Some(false)` = SQL in chiaro, `None` = non è un dump.
pub fn dump_kind(name: &str) -> Option<bool> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".sql.gz") {
        Some(true)
    } else if lower.ends_with(".sql") {
        Some(false)
    } else {
        None
    }
}

/// Riscrive una chiave del .env del sito lasciando intatto il resto.
pub fn replace_env_value(env: &str, key: &str, value: &str) -> Result<String> {
    if value.contains(['\'', '\n', '\r']) {
        bail!("valore non scrivibile nel .env: {key}");
    }
    let line = format!("{key}='{value}'");
    let prefix = format!("{key}=");
    let mut found = false;
    let mut out: Vec<String> = env
        .lines()
        .map(|l| {
            if l.starts_with(&prefix) {
                found = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(line);
    }
    Ok(out.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_value_is_replaced_in_place() {
        let env = "DB_NAME='sona_it'\nDB_PASSWORD='vecchia'\nDB_HOST='mariadb'\n";
        assert_eq!(replace_env_value(env, "DB_PASSWORD", "nuova").unwrap(), "DB_NAME='sona_it'\nDB_PASSWORD='nuova'\nDB_HOST='mariadb'\n");
    }

    #[test]
    fn missing_key_is_appended() {
        assert_eq!(replace_env_value("A='1'\n", "B", "2").unwrap(), "A='1'\nB='2'\n");
    }

    #[test]
    fn value_breaking_quotes_is_rejected() {
        assert!(replace_env_value("", "A", "x'y").is_err());
    }

    #[test]
    fn table_names_are_validated() {
        assert!(valid_table("wp_posts"));
        assert!(valid_table("wp_wc$x"));
        for bad in ["", "a b", "a;DROP", "a`b", "../x"] {
            assert!(!valid_table(bad), "accettata: {bad:?}");
        }
    }

    #[test]
    fn dump_kind_is_detected_from_name() {
        assert_eq!(dump_kind("backup.sql"), Some(false));
        assert_eq!(dump_kind("backup.SQL.GZ"), Some(true));
        assert_eq!(dump_kind("foto.jpg"), None);
    }
}
