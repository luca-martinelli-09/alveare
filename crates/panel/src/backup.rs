//! Backup con restic: un repository unico (la deduplica tra siti WordPress è il guadagno),
//! uno snapshot per sito con il tag `site=<slug>`. Qui le regole pure; i comandi nell'host.
use alveare_core::site::Slug;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Retention {
    pub daily: u32,
    pub weekly: u32,
    pub monthly: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Self { daily: 7, weekly: 4, monthly: 6 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Snapshot {
    pub snapshot_id: String,
    pub bytes_added: u64,
    pub total_bytes: u64,
}

/// Host fisso: restic raggruppa per host, e il container cambia nome a ogni ricreazione.
const RESTIC_HOST: &str = "alveare";

fn tag(slug: &Slug) -> String {
    format!("site={}", slug.as_str())
}

pub fn backup_args(slug: &Slug, site_dir: &str, dump: &str) -> Vec<String> {
    ["backup", "--json", "--host", RESTIC_HOST, "--tag", &tag(slug), site_dir, dump].map(String::from).to_vec()
}

pub fn forget_args(slug: &Slug, r: Retention) -> Vec<String> {
    let (d, w, m) = (r.daily.to_string(), r.weekly.to_string(), r.monthly.to_string());
    ["forget", "--json", "--host", RESTIC_HOST, "--tag", &tag(slug), "--keep-daily", &d, "--keep-weekly", &w, "--keep-monthly", &m, "--prune"]
        .map(String::from)
        .to_vec()
}

/// L'ultima riga `summary` dell'output JSON di `restic backup`.
pub fn parse_summary(out: &str) -> Option<Snapshot> {
    out.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).find(|v| v["message_type"] == "summary").and_then(|v| {
        Some(Snapshot {
            snapshot_id: v["snapshot_id"].as_str()?.to_string(),
            bytes_added: v["data_added"].as_u64().unwrap_or(0),
            total_bytes: v["total_bytes_processed"].as_u64().unwrap_or(0),
        })
    })
}

/// Id di snapshot (anche abbreviato): solo esadecimale minuscolo, 8–64 caratteri.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SnapEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
}

/// Figli diretti di `dir` dall'output di `restic ls --json` (una riga JSON per nodo).
pub fn parse_ls(out: &str, dir: &str) -> Vec<SnapEntry> {
    let dir = dir.trim_end_matches('/');
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["struct_type"] == "node")
        .filter(|v| v["path"].as_str().and_then(|p| p.rsplit_once('/')).is_some_and(|(parent, _)| parent == dir))
        .map(|v| SnapEntry {
            name: v["name"].as_str().unwrap_or_default().to_string(),
            is_dir: v["type"] == "dir",
            size: v["size"].as_u64().unwrap_or(0),
            mtime: v["mtime"].as_str().unwrap_or_default().to_string(),
        })
        .collect()
}

pub fn valid_snapshot(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Ora e minuto del backup notturno: tra le 3 e le 6, sfalsati per sito. Mai tra le 2 e le 3:
/// col cambio d'ora quell'ora salta (niente backup) o si ripete (due backup).
pub fn backup_slot(site_id: i64) -> (u32, u32) {
    let offset = (site_id * 37).rem_euclid(180) as u32;
    (3 + offset / 60, offset % 60)
}

/// Tutto ciò che serve per ricostruire il server da zero, da tenere fuori dal server.
pub fn recovery_kit(values: &[(&str, &str)]) -> String {
    let mut kit = String::from(
        "# Kit di ripristino di Alveare\n\
         # Conservalo fuori dal server (password manager). Senza queste chiavi i backup sono illeggibili.\n\n",
    );
    for (k, v) in values {
        kit.push_str(&format!("{k}={v}\n"));
    }
    kit.push_str(
        "\n# Ricostruzione:\n\
         # 1. Nuovo server con Docker; copia compose.yml e templates/ dal repository di Alveare.\n\
         # 2. Crea .env con i valori qui sopra (più MARIADB_ROOT_PASSWORD nuova).\n\
         # 3. Ripristina lo stato del pannello:\n\
         #    restic -r \"$RESTIC_REPOSITORY\" restore latest --tag panel --target /\n\
         # 4. docker compose up -d: il pannello rigenera Caddy, PHP e utenti dallo stato.\n\
         # 5. Per ogni sito: Ripristina dal pannello l'ultimo snapshot (file e database).\n",
    );
    kit
}

/// Configurazione del pannello da salvare cifrata insieme a panel.db.
pub fn panel_env(vars: impl Iterator<Item = (String, String)>) -> String {
    let mut lines: Vec<String> = vars
        .filter(|(k, _)| ["ALVEARE_", "MARIADB_", "RESTIC_", "AWS_", "SITE_ENV_"].iter().any(|p| k.starts_with(p)))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

/// Impronta del kit: se cambia, il pannello chiede di riscaricarlo.
pub fn kit_fingerprint(kit: &str) -> String {
    hex::encode(Sha256::digest(kit.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_listing_keeps_only_direct_children() {
        let out = r#"{"time":"2026-10-01T02:00:00Z","struct_type":"snapshot","id":"abc"}
{"name":"wp-content","type":"dir","path":"/srv/sites/sona_it/public/wp-content","mtime":"2026-09-30T10:00:00+02:00","struct_type":"node"}
{"name":"index.php","type":"file","path":"/srv/sites/sona_it/public/index.php","size":405,"mtime":"2026-09-30T10:00:00+02:00","struct_type":"node"}
{"name":"themes","type":"dir","path":"/srv/sites/sona_it/public/wp-content/themes","mtime":"2026-09-30T10:00:00+02:00","struct_type":"node"}
{"name":"public","type":"dir","path":"/srv/sites/sona_it/public","mtime":"2026-09-30T10:00:00+02:00","struct_type":"node"}
{"name":"link","type":"symlink","path":"/srv/sites/sona_it/public/link","struct_type":"node"}
"#;
        let entries = parse_ls(out, "/srv/sites/sona_it/public");
        assert_eq!(entries.len(), 3);
        assert_eq!((entries[0].name.as_str(), entries[0].is_dir), ("wp-content", true));
        assert_eq!((entries[1].name.as_str(), entries[1].size), ("index.php", 405));
        assert_eq!(entries[2].name, "link");
    }
    use alveare_core::site::Slug;

    fn slug() -> Slug {
        Slug::parse("sona_it").unwrap()
    }

    #[test]
    fn backup_args_tag_the_site_and_include_files_and_dump() {
        let a = backup_args(&slug(), "/srv/sites/sona_it", "/srv/backups/sona_it/db.sql");
        assert_eq!(a, ["backup", "--json", "--host", "alveare", "--tag", "site=sona_it", "/srv/sites/sona_it", "/srv/backups/sona_it/db.sql"]);
    }

    #[test]
    fn forget_args_apply_retention_per_site() {
        let r = Retention { daily: 7, weekly: 4, monthly: 6 };
        assert_eq!(
            forget_args(&slug(), r),
            ["forget", "--json", "--host", "alveare", "--tag", "site=sona_it", "--keep-daily", "7", "--keep-weekly", "4", "--keep-monthly", "6", "--prune"]
        );
    }

    #[test]
    fn summary_is_read_from_json_stream() {
        let out = r#"{"message_type":"status","percent_done":0.5}
{"message_type":"summary","files_new":3,"data_added":12345,"total_bytes_processed":999999,"snapshot_id":"4f1c2a9e8d7b6a5f"}
"#;
        let s = parse_summary(out).unwrap();
        assert_eq!(s.snapshot_id, "4f1c2a9e8d7b6a5f");
        assert_eq!((s.bytes_added, s.total_bytes), (12345, 999999));
        assert!(parse_summary("niente").is_none());
    }

    #[test]
    fn snapshot_ids_are_validated() {
        assert!(valid_snapshot("4f1c2a9e"));
        assert!(valid_snapshot("4f1c2a9e8d7b6a5f4f1c2a9e8d7b6a5f4f1c2a9e8d7b6a5f4f1c2a9e8d7b6a5f"));
        for bad in ["", "latest", "4f1c2a9", "../x", "4F1C2A9E"] {
            assert!(!valid_snapshot(bad), "accettato: {bad:?}");
        }
    }

    #[test]
    fn nightly_slot_is_between_3_and_6_away_from_the_dst_hour() {
        let mut seen = std::collections::BTreeSet::new();
        for id in 1..=200 {
            let (h, m) = backup_slot(id);
            // Tra le 2 e le 3 l'ora legale salta o si ripete: niente backup lì.
            assert!((3..6).contains(&h) && m < 60, "{id}: {h}:{m}");
            seen.insert((h, m));
        }
        assert!(seen.len() > 100, "siti non sfalsati");
    }

    #[test]
    fn panel_env_keeps_only_panel_settings() {
        let vars = [("PATH", "/bin"), ("RESTIC_PASSWORD", "p"), ("ALVEARE_SECRET_KEY", "k"), ("HOME", "/root")].map(|(k, v)| (k.to_string(), v.to_string()));
        assert_eq!(panel_env(vars.into_iter()), "ALVEARE_SECRET_KEY=k\nRESTIC_PASSWORD=p\n");
    }

    #[test]
    fn recovery_kit_contains_keys_and_instructions() {
        let kit = recovery_kit(&[("ALVEARE_SECRET_KEY", "k"), ("RESTIC_REPOSITORY", "s3:https://x/y"), ("RESTIC_PASSWORD", "p")]);
        assert!(kit.contains("ALVEARE_SECRET_KEY=k\n"));
        assert!(kit.contains("RESTIC_PASSWORD=p\n"));
        assert!(kit.contains("restic -r"));
        assert_eq!(kit_fingerprint(&kit), kit_fingerprint(&kit.clone()));
        assert_ne!(kit_fingerprint(&kit), kit_fingerprint("altro"));
    }
}
