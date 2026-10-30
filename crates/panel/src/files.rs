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
/// Sotto il limite di 100 MB per richiesta di Cloudflare Free e Pro.
pub const CHUNK: u64 = 8 * 1024 * 1024;
pub const UPLOAD_LIMIT: u64 = 10 * 1024 * 1024 * 1024;
const UPLOAD_ID_LEN: usize = 24;
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
    #[error("upload non valido: {0}")]
    BadUpload(&'static str),
    #[error("archivio: {0}")]
    Archive(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Voce del cestino: `origin` è il percorso da cui è stata eliminata (assente nelle voci vecchie).
#[derive(Debug, serde::Serialize)]
pub struct TrashItem {
    pub id: String,
    pub name: String,
    pub origin: Option<String>,
    pub deleted_at: i64,
    pub is_dir: bool,
    pub size: u64,
}

const TRASH: &str = ".trash";
/// Dentro ogni voce del cestino: il percorso originale.
const ORIGIN: &str = ".alveare-origine";

#[derive(Debug, serde::Serialize)]
pub struct UploadInfo {
    pub id: String,
    pub chunk_size: u64,
    pub chunks: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct UploadStatus {
    pub received: Vec<u64>,
    pub complete: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UploadMeta {
    dest: String,
    size: u64,
}

fn chunks_for(size: u64) -> u64 {
    size.div_ceil(CHUNK).max(1)
}

fn upload_dir(id: &str) -> Result<PathBuf> {
    if id.len() != UPLOAD_ID_LEN || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(FileError::BadUpload("id"));
    }
    Ok(Path::new("uploads").join(id))
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

/// Limite di estrazione nel file manager.
const EXTRACT_LIMIT: u64 = 5 * 1024 * 1024 * 1024;

fn archive_err(e: anyhow::Error) -> FileError {
    FileError::Archive(format!("{e:#}"))
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
        Self::open_dirs(&paths.sites.join(slug.as_str()), &paths.tmp.join(slug.as_str()), uid)
    }

    /// Stesse regole su un'altra coppia di cartelle (l'area import).
    pub fn open_dirs(root: &Path, tmp: &Path, uid: Option<u32>) -> Result<Self> {
        let root = Dir::open_ambient_dir(root, ambient_authority())?;
        let tmp = Dir::open_ambient_dir(tmp, ambient_authority())?;
        Ok(Self { root, tmp, uid })
    }

    /// Estrae uno zip o tar.gz del sito nella cartella `dest`; i percorsi dell'archivio non escono.
    pub fn extract(&self, archive: &str, dest: &str) -> Result<crate::archive::Report> {
        let kind = crate::archive::Kind::of(archive).ok_or(FileError::Archive("serve un .zip o un .tar.gz".into()))?;
        let file = self.open_read(archive)?;
        let dest_rel = rel(dest)?;
        self.mkdirs(&dest_rel)?;
        let dir = self.root.open_dir(&dest_rel)?;
        crate::archive::extract(file, kind, &dir, self.uid, EXTRACT_LIMIT).map_err(archive_err)
    }

    /// Crea `to` (.zip o .tar.gz) con i file e le cartelle indicati, ciascuno col suo nome.
    pub fn compress(&self, paths: &[String], to: &str) -> Result<()> {
        let kind = crate::archive::Kind::of(to).ok_or(FileError::Archive("serve un .zip o un .tar.gz".into()))?;
        // Percorsi relativi letti attraverso la Dir del sito: niente uscite con link messi a metà lavoro.
        let sources: Vec<(PathBuf, PathBuf)> = paths
            .iter()
            .map(|p| {
                let r = rel(p)?;
                let name = r.file_name().map(PathBuf::from).ok_or(FileError::BadPath)?;
                Ok((r, name))
            })
            .collect::<Result<_>>()?;
        let to_rel = rel(to)?;
        if let Some(parent) = to_rel.parent().filter(|x| !x.as_os_str().is_empty()) {
            self.mkdirs(parent)?;
        }
        let out = self.root.create(&to_rel)?.into_std();
        self.give_to_site(&out)?;
        let written = (|| match kind {
            crate::archive::Kind::Zip => {
                let mut w = crate::archive::ZipWriter::new(out);
                for (src, name) in &sources {
                    w.add_tree(&self.root, src, name)?;
                }
                w.finish()
            }
            crate::archive::Kind::TarGz => {
                let mut w = crate::archive::TarGzWriter::new(out);
                for (src, name) in &sources {
                    w.add_tree(&self.root, src, name)?;
                }
                w.finish()
            }
        })();
        if let Err(e) = written {
            let _ = self.root.remove_file(&to_rel);
            return Err(archive_err(e));
        }
        Ok(())
    }

    /// Cartella come zip per il download: file temporaneo già scollegato, sparisce alla chiusura.
    pub fn zip_for_download(&self, path: &str) -> Result<std::fs::File> {
        let r = rel(path)?;
        let name = r.file_name().map(PathBuf::from).ok_or(FileError::BadPath)?;
        let tmp_name = format!("download-{}.zip", rand::distr::SampleString::sample_string(&rand::distr::Alphanumeric, &mut rand::rng(), 16));
        let out = self.tmp.create(&tmp_name)?.into_std();
        let reader = self.tmp.open(&tmp_name)?.into_std();
        self.tmp.remove_file(&tmp_name)?;
        let mut w = crate::archive::ZipWriter::new(out);
        w.add_tree(&self.root, &r, &name).map_err(archive_err)?;
        w.finish().map_err(archive_err)?;
        Ok(reader)
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
                // open_dir dà un descrittore O_PATH, su cui fchown non funziona: si apre in lettura.
                self.give_to_site(&self.root.open_with(&cur, cap_std::fs::OpenOptions::new().read(true))?.into_std())?;
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
    pub fn is_dir(&self, path: &str) -> Result<bool> {
        Ok(self.root.symlink_metadata(rel(path)?)?.is_dir())
    }

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

    /// Nel cestino del sito (tmp/<slug>/.trash), svuotato dopo 7 giorni. Ogni voce è una cartella
    /// `<istante>-<casuale>` con dentro il file e il percorso da cui veniva, per rimetterlo lì.
    pub fn delete(&self, path: &str, now: i64) -> Result<()> {
        let p = rel(path)?;
        if is_protected(&p) {
            return Err(FileError::Protected);
        }
        let name = p.file_name().ok_or(FileError::BadPath)?;
        let entry = Path::new(TRASH).join(format!("{now}-{:08x}", rand::random::<u32>()));
        self.tmp.create_dir_all(&entry)?;
        self.tmp.write(entry.join(ORIGIN), p.to_string_lossy().as_bytes())?;
        if let Err(e) = self.root.rename(&p, &self.tmp, entry.join(name)) {
            self.tmp.remove_dir_all(&entry)?;
            return Err(e.into());
        }
        Ok(())
    }

    /// Voci del cestino, le più recenti prima.
    pub fn trash(&self) -> Result<Vec<TrashItem>> {
        let Ok(dir) = self.tmp.open_dir(TRASH) else { return Ok(vec![]) };
        let mut out = Vec::new();
        for entry in dir.entries()? {
            let id = entry?.file_name().to_string_lossy().into_owned();
            let Some((ts, rest)) = id.split_once('-') else { continue };
            let Ok(deleted_at) = ts.parse() else { continue };
            let (item, origin) = self.trash_entry(&id)?;
            let m = self.tmp.symlink_metadata(&item)?;
            let name = match (&origin, item.file_name()) {
                (Some(_), Some(n)) => n.to_string_lossy().into_owned(),
                _ => rest.to_string(),
            };
            out.push(TrashItem { origin, name, deleted_at, is_dir: m.is_dir(), size: if m.is_dir() { 0 } else { m.len() }, id });
        }
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    /// (file nel cestino, percorso originale). Le voci vecchie sono il file stesso, `<istante>-<nome>`.
    fn trash_entry(&self, id: &str) -> Result<(PathBuf, Option<String>)> {
        if id.is_empty() || id.contains('/') || id.starts_with('.') {
            return Err(FileError::BadPath);
        }
        let entry = Path::new(TRASH).join(id);
        let Ok(origin) = self.tmp.read_to_string(entry.join(ORIGIN)) else { return Ok((entry, None)) };
        let item = self.tmp.read_dir(&entry)?.flatten().map(|e| e.file_name()).find(|n| n != ORIGIN).ok_or(FileError::BadPath)?;
        Ok((entry.join(item), Some(origin)))
    }

    /// Rimette la voce dov'era (le vecchie nella radice); mai sopra un file che c'è già.
    pub fn restore(&self, id: &str) -> Result<String> {
        let (item, origin) = self.trash_entry(id)?;
        let name = id.split_once('-').map_or(id, |(_, n)| n);
        let dest = rel(origin.as_deref().unwrap_or(name))?;
        if is_protected(&dest) {
            return Err(FileError::Protected);
        }
        if self.root.symlink_metadata(&dest).is_ok() {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());
        }
        if let Some(parent) = dest.parent().filter(|x| !x.as_os_str().is_empty()) {
            self.mkdirs(parent)?;
        }
        self.tmp.rename(&item, &self.root, &dest)?;
        if origin.is_some() {
            self.tmp.remove_dir_all(Path::new(TRASH).join(id))?;
        }
        Ok(dest.to_string_lossy().into_owned())
    }

    /// Elimina definitivamente una voce del cestino.
    pub fn purge(&self, id: &str) -> Result<()> {
        self.trash_entry(id)?;
        let entry = Path::new(TRASH).join(id);
        if self.tmp.symlink_metadata(&entry)?.is_dir() { self.tmp.remove_dir_all(&entry)? } else { self.tmp.remove_file(&entry)? }
        Ok(())
    }

    pub fn chmod(&self, path: &str, mode: u32) -> Result<()> {
        if mode > 0o777 {
            return Err(FileError::BadMode);
        }
        let perms = cap_std::fs::Permissions::from_std(std::fs::Permissions::from_mode(mode));
        Ok(self.root.set_permissions(rel(path)?, perms)?)
    }

    /// Upload a blocchi: i pezzi si ricompongono in tmp/<slug>/uploads/<id> e il file finito
    /// si sposta al suo posto. I blocchi possono arrivare in qualunque ordine e ripetersi.
    pub fn upload_start(&self, dest: &str, size: u64) -> Result<UploadInfo> {
        let dest_rel = rel(dest)?;
        if is_protected(&dest_rel) {
            return Err(FileError::Protected);
        }
        if size > UPLOAD_LIMIT {
            return Err(FileError::BadUpload("file oltre 10 GB"));
        }
        let id = rand::distr::SampleString::sample_string(&rand::distr::Alphanumeric, &mut rand::rng(), UPLOAD_ID_LEN);
        let dir = upload_dir(&id)?;
        self.tmp.create_dir_all(&dir)?;
        let meta = UploadMeta { dest: dest_rel.to_string_lossy().into_owned(), size };
        self.tmp.write(dir.join("meta.json"), serde_json::to_vec(&meta).expect("serializzazione"))?;
        self.tmp.create(dir.join("data"))?.set_len(size)?;
        Ok(UploadInfo { id, chunk_size: CHUNK, chunks: chunks_for(size) })
    }

    fn upload_meta(&self, dir: &Path) -> Result<UploadMeta> {
        let raw = self.tmp.read(dir.join("meta.json")).map_err(|_| FileError::BadUpload("upload sconosciuto"))?;
        serde_json::from_slice(&raw).map_err(|_| FileError::BadUpload("metadati"))
    }

    pub fn upload_status(&self, id: &str) -> Result<UploadStatus> {
        let dir = upload_dir(id)?;
        let meta = self.upload_meta(&dir)?;
        let received: Vec<u64> = (0..chunks_for(meta.size)).filter(|n| self.tmp.exists(dir.join(format!("{n}.ok")))).collect();
        let complete = received.len() as u64 == chunks_for(meta.size);
        Ok(UploadStatus { received, complete })
    }

    pub fn upload_chunk(&self, id: &str, n: u64, bytes: &[u8]) -> Result<UploadStatus> {
        use std::io::{Seek, SeekFrom};
        let dir = upload_dir(id)?;
        let meta = self.upload_meta(&dir)?;
        if n >= chunks_for(meta.size) {
            return Err(FileError::BadUpload("blocco fuori intervallo"));
        }
        let expected = CHUNK.min(meta.size - n * CHUNK);
        if bytes.len() as u64 != expected {
            return Err(FileError::BadUpload("dimensione del blocco"));
        }
        let mut f = self.tmp.open_with(dir.join("data"), cap_std::fs::OpenOptions::new().write(true))?;
        f.seek(SeekFrom::Start(n * CHUNK))?;
        f.write_all(bytes)?;
        f.sync_data()?;
        self.tmp.write(dir.join(format!("{n}.ok")), b"")?;
        let status = self.upload_status(id)?;
        if status.complete {
            let dest = PathBuf::from(&meta.dest);
            if let Some(parent) = dest.parent().filter(|x| !x.as_os_str().is_empty()) {
                self.mkdirs(parent)?;
            }
            self.tmp.rename(dir.join("data"), &self.root, &dest)?;
            self.give_to_site(&self.root.open(&dest)?.into_std())?;
            self.tmp.remove_dir_all(&dir)?;
        }
        Ok(status)
    }

    /// File eseguibili da PHP dentro uploads: non dovrebbero mai esserci.
    pub fn php_in_uploads(&self) -> Result<Vec<String>> {
        let uploads = Path::new("public/wp-content/uploads");
        let dir = match self.root.open_dir(uploads) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut found = Vec::new();
        walk_php(&dir, PathBuf::new(), &mut found)?;
        found.sort();
        Ok(found)
    }
}

fn is_php_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".php", ".phtml", ".phar"].iter().any(|e| lower.ends_with(e)) || lower.rsplit('.').next().is_some_and(|x| x.len() == 4 && x.starts_with("php"))
}

fn walk_php(dir: &Dir, prefix: PathBuf, found: &mut Vec<String>) -> Result<()> {
    for entry in dir.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let path = prefix.join(&name);
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk_php(&entry.open_dir()?, path, found)?;
        } else if is_php_name(&name.to_string_lossy()) {
            found.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(())
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
    fn compress_then_extract_round_trip_in_both_formats() {
        let (_t, _p, f) = site();
        f.write("public/wp-content/themes/tema/style.css", b"body{}").unwrap();
        f.write("public/wp-content/themes/tema/index.php", b"<?php").unwrap();
        for (ts, archive) in [(1, "public/tema.zip"), (2, "public/tema.tar.gz")] {
            f.compress(&["public/wp-content/themes/tema".into()], archive).unwrap();
            f.mkdir("public/estratto").unwrap();
            let r = f.extract(archive, "public/estratto").unwrap();
            assert_eq!(r.files, 2, "{archive}");
            assert_eq!(f.read_text("public/estratto/tema/style.css").unwrap(), "body{}");
            f.delete("public/estratto", ts).unwrap();
        }
    }

    /// Con il proprietario impostato (come in produzione): le cartelle nuove vanno date al sito.
    /// Qui al proprio utente, che si può fare anche senza root.
    #[test]
    fn new_folders_get_the_site_owner() {
        let (_t, p, _) = site();
        let me = unsafe { libc::getuid() };
        let f = SiteFiles::open(&p, &Slug::parse("sona_it").unwrap(), Some(me)).unwrap();
        f.write("public/a/b/c.txt", b"x").unwrap();
        f.compress(&["public/a".into()], "a.zip").unwrap();
        f.extract("a.zip", "nuova/cartella").unwrap();
        assert_eq!(f.read_text("nuova/cartella/a/b/c.txt").unwrap(), "x");
    }

    #[test]
    fn compress_refuses_paths_through_symlinks_out_of_the_site() {
        let (t, p, f) = site();
        let outside = t.path().join("fuori");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("segreto.txt"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, p.sites.join("sona_it/public/link")).unwrap();
        assert!(f.compress(&["public/link".into()], "public/a.zip").is_err());
        assert!(f.compress(&[".env".into()], "public/a.zip").is_err());
        assert!(f.compress(&["/".into()], "public/a.zip").is_err(), "la radice contiene il .env");
        assert!(f.zip_for_download("").is_err());
        assert!(f.compress(&["public".into()], "public/a.rar").is_err());
    }

    #[test]
    fn folder_download_is_a_zip_without_leftovers() {
        let (_t, p, f) = site();
        f.write("public/a/b.txt", b"ciao").unwrap();
        let mut zip = f.zip_for_download("public/a").unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut zip, &mut bytes).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut z.by_name("a/b.txt").unwrap(), &mut text).unwrap();
        assert_eq!(text, "ciao");
        assert_eq!(std::fs::read_dir(p.tmp.join("sona_it")).unwrap().count(), 0, "il file temporaneo non resta");
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
    fn deleted_files_go_to_the_trash_and_come_back_where_they_were() {
        let (_t, _p, f) = site();
        f.write("public/tema/style.css", b"x").unwrap();
        f.delete("public/tema", 1_700_000_000).unwrap();
        assert!(f.list("public").unwrap().is_empty());
        let trash = f.trash().unwrap();
        assert_eq!(trash.len(), 1);
        let t = &trash[0];
        assert_eq!((t.name.as_str(), t.origin.as_deref(), t.deleted_at, t.is_dir), ("tema", Some("public/tema"), 1_700_000_000, true));
        // Al suo posto c'è altro: non si sovrascrive.
        f.write("public/tema/nuovo.css", b"y").unwrap();
        assert!(f.restore(&t.id).is_err());
        f.delete("public/tema", 1_700_000_000).unwrap();
        assert_eq!(f.trash().unwrap().len(), 2, "stesso nome nello stesso secondo: due voci");
        assert_eq!(f.restore(&t.id).unwrap(), "public/tema");
        assert_eq!(f.read_text("public/tema/style.css").unwrap(), "x");
        let left = f.trash().unwrap();
        assert_eq!(left.len(), 1);
        f.purge(&left[0].id).unwrap();
        assert!(f.trash().unwrap().is_empty());
        assert!(f.restore("..").is_err() && f.purge("../uploads").is_err());
    }

    #[test]
    fn old_trash_entries_without_a_path_go_back_to_the_root() {
        let (_t, p, f) = site();
        std::fs::create_dir_all(p.tmp.join("sona_it/.trash")).unwrap();
        std::fs::write(p.tmp.join("sona_it/.trash/1700000000-vecchio.txt"), "v").unwrap();
        let trash = f.trash().unwrap();
        assert_eq!((trash[0].name.as_str(), trash[0].origin.as_deref(), trash[0].deleted_at), ("vecchio.txt", None, 1_700_000_000));
        assert_eq!(f.restore(&trash[0].id).unwrap(), "vecchio.txt");
        assert_eq!(f.read_text("vecchio.txt").unwrap(), "v");
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

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn chunked_upload_in_any_order_lands_at_destination() {
        let (_t, p, f) = site();
        let content = data(CHUNK as usize + 1000);
        let up = f.upload_start("public/video/clip.mp4", content.len() as u64).unwrap();
        assert_eq!(up.chunks, 2);
        let s = f.upload_chunk(&up.id, 1, &content[CHUNK as usize..]).unwrap();
        assert_eq!((s.received.clone(), s.complete), (vec![1], false));
        let s = f.upload_chunk(&up.id, 0, &content[..CHUNK as usize]).unwrap();
        assert!(s.complete);
        assert_eq!(std::fs::read(p.sites.join("sona_it/public/video/clip.mp4")).unwrap(), content);
        assert!(std::fs::read_dir(p.tmp.join("sona_it/uploads")).unwrap().next().is_none());
    }

    #[test]
    fn upload_status_allows_resuming() {
        let (_t, _p, f) = site();
        let content = data(CHUNK as usize * 2 + 5);
        let up = f.upload_start("public/a.bin", content.len() as u64).unwrap();
        f.upload_chunk(&up.id, 0, &content[..CHUNK as usize]).unwrap();
        assert_eq!(f.upload_status(&up.id).unwrap().received, [0]);
    }

    #[test]
    fn upload_rejects_wrong_chunks_and_bad_ids() {
        let (_t, _p, f) = site();
        let up = f.upload_start("public/a.bin", 10).unwrap();
        assert!(f.upload_chunk(&up.id, 0, b"corto").is_err());
        assert!(f.upload_chunk(&up.id, 1, b"0123456789").is_err());
        assert!(f.upload_chunk("../../etc", 0, b"0123456789").is_err());
        assert!(f.upload_status("inesistente000000000000000").is_err());
    }

    #[test]
    fn upload_destination_must_stay_in_site() {
        let (_t, _p, f) = site();
        assert!(f.upload_start("../fuori.bin", 10).is_err());
        assert!(f.upload_start(".env", 10).is_err());
        assert!(f.upload_start("public/x", UPLOAD_LIMIT + 1).is_err());
    }

    #[test]
    fn php_files_in_uploads_are_found() {
        let (_t, _p, f) = site();
        f.write("public/wp-content/uploads/2026/11/foto.jpg", b"x").unwrap();
        f.write("public/wp-content/uploads/2026/11/shell.PHP", b"x").unwrap();
        f.write("public/wp-content/uploads/cache/x.phtml", b"x").unwrap();
        f.write("public/wp-content/plugins/ok.php", b"x").unwrap();
        assert_eq!(f.php_in_uploads().unwrap(), ["2026/11/shell.PHP", "cache/x.phtml"]);
    }

    #[test]
    fn php_scan_without_uploads_folder_is_empty() {
        let (_t, _p, f) = site();
        assert!(f.php_in_uploads().unwrap().is_empty());
    }

    #[test]
    fn env_file_is_hidden_from_the_file_manager() {
        let (_t, _p, f) = site();
        f.write(".env", b"DB_PASSWORD='x'").unwrap_err();
        assert!(f.read_text(".env").is_err());
    }
}
