//! File manager confinato alla cartella del sito: ogni accesso passa da un `Dir` di cap-std,
//! che rifiuta `..` e link simbolici che escono dalla radice. Il pannello scrive come root,
//! quindi ogni file creato torna all'utente del sito (fchown sul descrittore, non sul path).
use std::io::{Read, Write};
use std::os::unix::fs::{fchown, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use alveare_core::site::Slug;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, MetadataExt};

use crate::host::Paths;

pub const EDITOR_LIMIT: u64 = 2 * 1024 * 1024;
/// Percorsi che il file manager non mostra né tocca: il .env ha le credenziali.
const HIDDEN: [&str; 1] = [".env"];
const PROTECTED: [&str; 1] = ["public"];

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("percorso non valido")]
    BadPath,
    #[error("file troppo grande per l'editor (max 2 MB)")]
    TooLarge,
    #[error("questa cartella non si può eliminare o spostare")]
    Protected,
    #[error("permessi non validi")]
    BadMode,
    #[error("il file non è testo UTF-8")]
    NotText,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, FileError>;

#[derive(Debug, serde::Serialize)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: i64,
    pub mode: u32,
    pub uid: u32,
}

pub struct SiteFiles {
    root: Dir,
    tmp: Dir,
    uid: Option<u32>,
}

/// "/public/../x" → errore; "/public/x" → "public/x"; "" → "."
fn rel(path: &str) -> Result<PathBuf> {
    let p = Path::new(path.trim_start_matches('/'));
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            _ => return Err(FileError::BadPath),
        }
    }
    if out.components().next().is_some_and(|c| HIDDEN.iter().any(|h| c.as_os_str() == *h)) {
        return Err(FileError::BadPath);
    }
    Ok(if out.as_os_str().is_empty() { PathBuf::from(".") } else { out })
}

fn is_protected(p: &Path) -> bool {
    p == Path::new(".") || PROTECTED.iter().any(|x| p == Path::new(x))
}

impl SiteFiles {
    /// `uid = None` lascia i proprietari come sono (test senza root).
    pub fn open(paths: &Paths, slug: &Slug, uid: Option<u32>) -> Result<Self> {
        let root = Dir::open_ambient_dir(paths.sites.join(slug.as_str()), ambient_authority())?;
        let tmp = Dir::open_ambient_dir(paths.tmp.join(slug.as_str()), ambient_authority())?;
        Ok(Self { root, tmp, uid })
    }

    fn give_to_site(&self, file: &std::fs::File) -> Result<()> {
        if let Some(uid) = self.uid {
            fchown(file, Some(uid), Some(uid))?;
        }
        Ok(())
    }

    fn mkdirs(&self, dir: &Path) -> Result<()> {
        let mut cur = PathBuf::new();
        for c in dir.components() {
            cur.push(c);
            if !self.root.exists(&cur) {
                self.root.create_dir(&cur)?;
                self.give_to_site(&self.root.open_dir(&cur)?.into_std_file())?;
            }
        }
        Ok(())
    }

    pub fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let dir = self.root.open_dir(rel(path)?)?;
        let at_root = rel(path)? == Path::new(".");
        let mut out = Vec::new();
        for entry in dir.entries()? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if at_root && HIDDEN.contains(&name.as_str()) {
                continue;
            }
            let m = entry.metadata()?;
            out.push(Entry {
                is_dir: m.is_dir(),
                is_symlink: m.file_type().is_symlink(),
                size: m.len(),
                modified: m.mtime(),
                mode: m.mode() & 0o7777,
                uid: m.uid(),
                name,
            });
        }
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    pub fn read_text(&self, path: &str) -> Result<String> {
        let mut f = self.root.open(rel(path)?)?;
        if f.metadata()?.len() > EDITOR_LIMIT {
            return Err(FileError::TooLarge);
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        String::from_utf8(buf).map_err(|_| FileError::NotText)
    }

    /// Per il download in streaming.
    pub fn open_read(&self, path: &str) -> Result<std::fs::File> {
        let f = self.root.open(rel(path)?)?;
        if f.metadata()?.is_dir() {
            return Err(FileError::BadPath);
        }
        Ok(f.into_std())
    }

    pub fn write(&self, path: &str, content: &[u8]) -> Result<()> {
        let p = rel(path)?;
        if let Some(parent) = p.parent().filter(|x| !x.as_os_str().is_empty()) {
            self.mkdirs(parent)?;
        }
        let mut f: File = self.root.create(&p)?;
        f.write_all(content)?;
        self.give_to_site(&f.into_std())
    }

    pub fn mkdir(&self, path: &str) -> Result<()> {
        self.mkdirs(&rel(path)?)
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (from, to) = (rel(from)?, rel(to)?);
        if is_protected(&from) || is_protected(&to) {
            return Err(FileError::Protected);
        }
        if let Some(parent) = to.parent().filter(|x| !x.as_os_str().is_empty()) {
            self.mkdirs(parent)?;
        }
        Ok(self.root.rename(&from, &self.root, &to)?)
    }

    /// Nel cestino del sito (tmp/<slug>/.trash), svuotato dopo 7 giorni.
    pub fn delete(&self, path: &str, now: i64) -> Result<()> {
        let p = rel(path)?;
        if is_protected(&p) {
            return Err(FileError::Protected);
        }
        let name = p.file_name().ok_or(FileError::BadPath)?.to_string_lossy().into_owned();
        if !self.tmp.exists(".trash") {
            self.tmp.create_dir(".trash")?;
        }
        Ok(self.root.rename(&p, &self.tmp, Path::new(".trash").join(format!("{now}-{name}")))?)
    }

    pub fn chmod(&self, path: &str, mode: u32) -> Result<()> {
        if mode > 0o777 {
            return Err(FileError::BadMode);
        }
        let perms = cap_std::fs::Permissions::from_std(std::fs::Permissions::from_mode(mode));
        Ok(self.root.set_permissions(rel(path)?, perms)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{create_dirs, Paths};
    use alveare_core::site::Slug;
    use std::os::unix::fs::PermissionsExt;

    fn site() -> (tempfile::TempDir, Paths, SiteFiles) {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::under(t.path());
        let slug = Slug::parse("sona_it").unwrap();
        create_dirs(&p, &slug, None).unwrap();
        let f = SiteFiles::open(&p, &slug, None).unwrap();
        (t, p, f)
    }

    #[test]
    fn write_read_and_list() {
        let (_t, _p, f) = site();
        f.write("public/index.php", b"<?php echo 1;").unwrap();
        assert_eq!(f.read_text("/public/index.php").unwrap(), "<?php echo 1;");
        let names: Vec<String> = f.list("public").unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["index.php"]);
        let root: Vec<(String, bool)> = f.list("").unwrap().into_iter().map(|e| (e.name, e.is_dir)).collect();
        assert!(root.contains(&("public".into(), true)));
    }

    #[test]
    fn paths_cannot_escape_the_site() {
        let (_t, p, f) = site();
        std::fs::create_dir_all(p.sites.join("altro_it")).unwrap();
        std::fs::write(p.sites.join("altro_it/segreto"), "x").unwrap();
        for bad in ["../altro_it/segreto", "public/../../altro_it/segreto", "/../altro_it/segreto"] {
            assert!(f.read_text(bad).is_err(), "letto: {bad}");
            assert!(f.write(bad, b"x").is_err(), "scritto: {bad}");
        }
    }

    #[test]
    fn symlinks_pointing_outside_are_not_followed() {
        let (_t, p, f) = site();
        std::os::unix::fs::symlink("/etc/passwd", p.sites.join("sona_it/public/link")).unwrap();
        std::os::unix::fs::symlink(p.sites.as_path(), p.sites.join("sona_it/public/su")).unwrap();
        assert!(f.read_text("public/link").is_err());
        assert!(f.write("public/su/sona_it/x", b"x").is_err());
        assert!(f.list("public/su").is_err());
    }

    #[test]
    fn large_files_are_not_opened_in_the_editor() {
        let (_t, _p, f) = site();
        f.write("public/big.txt", &vec![b'a'; EDITOR_LIMIT as usize + 1]).unwrap();
        assert!(matches!(f.read_text("public/big.txt"), Err(FileError::TooLarge)));
    }

    #[test]
    fn delete_moves_to_site_trash() {
        let (_t, p, f) = site();
        f.write("public/tema/style.css", b"x").unwrap();
        f.delete("public/tema", 1_700_000_000).unwrap();
        assert!(f.list("public").unwrap().is_empty());
        assert!(p.tmp.join("sona_it/.trash/1700000000-tema/style.css").exists());
    }

    #[test]
    fn site_root_and_public_cannot_be_deleted_or_renamed() {
        let (_t, _p, f) = site();
        for path in ["", "/", ".", "public"] {
            assert!(f.delete(path, 1).is_err(), "eliminato: {path:?}");
        }
        assert!(f.rename("public", "pubblico").is_err());
    }

    #[test]
    fn rename_moves_within_the_site() {
        let (_t, _p, f) = site();
        f.write("public/a.txt", b"x").unwrap();
        f.mkdir("public/sub").unwrap();
        f.rename("public/a.txt", "public/sub/b.txt").unwrap();
        assert_eq!(f.read_text("public/sub/b.txt").unwrap(), "x");
        assert!(f.rename("public/sub/b.txt", "../fuori.txt").is_err());
    }

    #[test]
    fn chmod_rejects_special_bits() {
        let (_t, p, f) = site();
        f.write("public/a.php", b"x").unwrap();
        f.chmod("public/a.php", 0o640).unwrap();
        let mode = std::fs::metadata(p.sites.join("sona_it/public/a.php")).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o640);
        assert!(f.chmod("public/a.php", 0o4755).is_err());
    }

    #[test]
    fn env_file_is_hidden_from_the_file_manager() {
        let (_t, _p, f) = site();
        f.write(".env", b"DB_PASSWORD='x'").unwrap_err();
        assert!(f.read_text(".env").is_err());
    }
}
