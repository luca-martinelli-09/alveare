//! Import di un sito esistente: archivio dei file in `public/`, prefisso dal vecchio wp-config.php.
use std::path::Path;

use anyhow::{Context, Result};
use cap_std::ambient_authority;
use cap_std::fs::Dir;

use crate::archive::{self, Kind};
use crate::flows::Imported;
use crate::template::valid_table_prefix;

/// Oltre si usa una cartella già presente sul server, non un archivio.
pub const IMPORT_LIMIT: u64 = 20 * 1024 * 1024 * 1024;

/// `$table_prefix = 'ep_';` (apici singoli o doppi), solo se è un prefisso valido.
pub fn table_prefix(wp_config: &str) -> Option<String> {
    wp_config.lines().find_map(|l| {
        let rest = l.trim().strip_prefix("$table_prefix")?.trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next().filter(|q| *q == '\'' || *q == '"')?;
        let value = rest[1..].split(quote).next()?;
        valid_table_prefix(value).then(|| value.to_string())
    })
}

pub fn import_files(public: &Path, archive_path: &Path) -> Result<Imported> {
    let name = archive_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let kind = Kind::of(&name).context("serve un archivio .zip o .tar.gz")?;
    let dir = Dir::open_ambient_dir(public, ambient_authority())?;
    let file = std::fs::File::open(archive_path).with_context(|| format!("apertura di {name}"))?;
    archive::extract(file, kind, &dir, None, IMPORT_LIMIT)?;
    archive::flatten_single_dir(&dir)?;
    let prefix = dir.read_to_string("wp-config.php").ok().and_then(|c| table_prefix(&c));
    Ok(Imported { table_prefix: prefix, has_core: dir.is_dir("wp-includes") && dir.is_dir("wp-admin") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn prefix_is_read_from_wp_config() {
        assert_eq!(table_prefix("<?php\n$table_prefix = 'ep_';\n").as_deref(), Some("ep_"));
        assert_eq!(table_prefix("$table_prefix  =  \"pl2_\" ;").as_deref(), Some("pl2_"));
        assert_eq!(table_prefix("$table_prefix = 'x\\'; --';"), None);
        assert_eq!(table_prefix("// niente"), None);
    }

    #[test]
    fn archive_with_top_folder_lands_in_public_with_core_detected() {
        let public = tempfile::tempdir().unwrap();
        let zip_path = public.path().parent().unwrap().join(format!("import-{}.zip", std::process::id()));
        let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (name, data) in [("public_html/wp-config.php", "$table_prefix = 'ep_';"), ("public_html/wp-includes/version.php", "x"), ("public_html/wp-admin/index.php", "x")] {
            z.start_file(name, o).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let got = import_files(public.path(), &zip_path).unwrap();
        std::fs::remove_file(&zip_path).unwrap();
        assert_eq!(got.table_prefix.as_deref(), Some("ep_"));
        assert!(got.has_core);
        assert!(public.path().join("wp-includes/version.php").exists());
    }

    #[test]
    fn unknown_archive_type_is_refused() {
        let public = tempfile::tempdir().unwrap();
        assert!(import_files(public.path(), Path::new("/tmp/sito.rar")).is_err());
    }
}
