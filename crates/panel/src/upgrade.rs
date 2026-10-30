//! Aggiornamento del pannello: quando cambia versione, prima delle migrazioni si copia
//! panel.db accanto all'originale (le ultime 5 copie); dopo l'avvio parte un backup restic.
//! Per tornare indietro: immagine precedente + la copia al posto di panel.db.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const KEEP: usize = 5;

/// Se la versione registrata è diversa da `current`, copia il database e restituisce la copia.
pub fn before_migrations(dir: &Path, current: &str, now: i64) -> Result<Option<PathBuf>> {
    let db = dir.join("panel.db");
    let previous = std::fs::read_to_string(dir.join("VERSION")).ok().map(|v| v.trim().to_string());
    let Some(previous) = previous.filter(|p| p != current) else { return Ok(None) };
    if !db.exists() {
        return Ok(None);
    }
    let copy = dir.join(format!("panel-{previous}-{now}.db"));
    std::fs::copy(&db, &copy).with_context(|| format!("copia di sicurezza in {}", copy.display()))?;
    prune_copies(dir)?;
    Ok(Some(copy))
}

/// Da chiamare dopo un avvio riuscito (migrazioni applicate).
pub fn mark_started(dir: &Path, current: &str) -> Result<()> {
    std::fs::write(dir.join("VERSION"), format!("{current}\n")).context("scrittura di VERSION")
}

fn prune_copies(dir: &Path) -> Result<()> {
    let mut copies: Vec<(String, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stamp = name.strip_prefix("panel-")?.strip_suffix(".db")?.rsplit_once('-')?.1.to_string();
            Some((format!("{stamp:0>20}"), e.path()))
        })
        .collect();
    copies.sort();
    let excess = copies.len().saturating_sub(KEEP);
    for (_, p) in copies.into_iter().take(excess) {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_is_copied_only_when_the_version_changes() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("panel.db"), "dati").unwrap();
        assert_eq!(before_migrations(t.path(), "0.2.0", 1).unwrap(), None, "primo avvio: niente da copiare");
        mark_started(t.path(), "0.2.0").unwrap();
        assert_eq!(before_migrations(t.path(), "0.2.0", 2).unwrap(), None);
        let copy = before_migrations(t.path(), "0.3.0", 3).unwrap().unwrap();
        assert_eq!(copy.file_name().unwrap(), "panel-0.2.0-3.db");
        assert_eq!(std::fs::read_to_string(copy).unwrap(), "dati");
    }

    #[test]
    fn only_the_last_five_copies_are_kept() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("panel.db"), "x").unwrap();
        for i in 0..8 {
            std::fs::write(t.path().join("VERSION"), format!("0.{i}.0")).unwrap();
            before_migrations(t.path(), "9.9.9", 100 + i).unwrap();
        }
        let mut left: Vec<String> = std::fs::read_dir(t.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("panel-")).collect();
        left.sort();
        assert_eq!(left.len(), 5);
        assert_eq!(left[0], "panel-0.3.0-103.db");
    }

    /// Il rollback a un'immagine precedente funziona solo se le migrazioni aggiungono e basta.
    #[test]
    fn migrations_are_additive() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let sql = std::fs::read_to_string(entry.path()).unwrap().to_ascii_uppercase();
            for forbidden in ["DROP TABLE", "DROP COLUMN", "RENAME", "DELETE FROM"] {
                assert!(!sql.contains(forbidden), "{}: {forbidden} non è additiva", entry.file_name().to_string_lossy());
            }
        }
    }
}
