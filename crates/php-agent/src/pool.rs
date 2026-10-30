use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, serde::Serialize)]
pub struct ApplyReport {
    pub applied: Vec<String>,
    pub rejected: Vec<(String, String)>,
}

/// Valida ogni pool in staging da solo e attiva solo quelli validi:
/// un pool sbagliato resta fuori senza bloccare gli altri, e il sito tiene la versione precedente.
pub fn apply_pools(
    staging: &Path,
    active: &Path,
    validate: impl Fn(&Path) -> Result<(), String>,
) -> io::Result<ApplyReport> {
    let mut files: Vec<PathBuf> = fs::read_dir(staging)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "conf"))
        .collect();
    files.sort();
    let mut report = ApplyReport::default();
    for path in files {
        let name = path.file_name().expect("file da read_dir").to_string_lossy().into_owned();
        match validate(&path) {
            Ok(()) => {
                fs::rename(&path, active.join(&name))?;
                report.applied.push(name);
            }
            Err(e) => {
                fs::remove_file(&path)?;
                report.rejected.push((name, e));
            }
        }
    }
    Ok(report)
}

/// Config FPM per un runtime aggiuntivo: i suoi pool, il suo pool interno, niente Adminer
/// (Adminer vive solo nel container principale).
pub fn runtime_fpm_conf(base: &str, runtime: &str) -> String {
    let (active, _) = alveare_core::php::runtime_pool_dirs(runtime);
    let without_adminer = base.split("\n; Adminer").next().unwrap_or(base);
    without_adminer
        .replace("include = /srv/php/pools/*.conf", &format!("include = /srv/php/{active}/*.conf"))
        .replace("/run/php/_alveare.sock", &format!("/run/php/_alveare.{runtime}.sock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let staging = t.path().join("staging");
        let active = t.path().join("pools");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&active).unwrap();
        (t, staging, active)
    }

    fn validator(p: &Path) -> Result<(), String> {
        let text = fs::read_to_string(p).unwrap();
        if text.contains("ROTTO") { Err("ERROR: riga 3".into()) } else { Ok(()) }
    }

    #[test]
    fn valid_pool_replaces_active_one() {
        let (_t, staging, active) = dirs();
        fs::write(active.join("a.conf"), "vecchio").unwrap();
        fs::write(staging.join("a.conf"), "nuovo").unwrap();
        let r = apply_pools(&staging, &active, validator).unwrap();
        assert_eq!(r.applied, ["a.conf"]);
        assert_eq!(fs::read_to_string(active.join("a.conf")).unwrap(), "nuovo");
        assert!(!staging.join("a.conf").exists());
    }

    #[test]
    fn invalid_pool_keeps_previous_and_does_not_block_others() {
        let (_t, staging, active) = dirs();
        fs::write(active.join("a.conf"), "vecchio").unwrap();
        fs::write(staging.join("a.conf"), "ROTTO").unwrap();
        fs::write(staging.join("b.conf"), "buono").unwrap();
        let r = apply_pools(&staging, &active, validator).unwrap();
        assert_eq!(r.applied, ["b.conf"]);
        assert_eq!(r.rejected, [("a.conf".to_string(), "ERROR: riga 3".to_string())]);
        assert_eq!(fs::read_to_string(active.join("a.conf")).unwrap(), "vecchio");
        assert!(active.join("b.conf").exists());
        assert!(!staging.join("a.conf").exists());
    }

    #[test]
    fn non_pool_files_are_ignored() {
        let (_t, staging, active) = dirs();
        fs::write(staging.join("note.txt"), "x").unwrap();
        let r = apply_pools(&staging, &active, validator).unwrap();
        assert!(r.applied.is_empty() && r.rejected.is_empty());
        assert!(!active.join("note.txt").exists());
    }

    #[test]
    fn runtime_config_has_its_pools_and_no_adminer() {
        let base = include_str!("../../../docker/php/fpm.conf");
        let conf = runtime_fpm_conf(base, "8.4");
        assert!(conf.contains("include = /srv/php/runtimes/8.4/pools/*.conf"));
        assert!(conf.contains("listen = /run/php/_alveare.8.4.sock"));
        assert!(!conf.contains("_adminer"), "{conf}");
        assert!(conf.contains("[_alveare]"));
    }
}
