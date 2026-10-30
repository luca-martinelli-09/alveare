//! Archivi zip e tar.gz: estrazione confinata in una cartella di cap-std (niente `..`, niente
//! percorsi assoluti, niente link) e creazione senza seguire i link simbolici.
use std::io::Read;
use std::os::unix::fs::fchown;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use cap_std::fs::Dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    TarGz,
}

impl Kind {
    pub fn of(name: &str) -> Option<Self> {
        let n = name.to_ascii_lowercase();
        if n.ends_with(".zip") {
            Some(Self::Zip)
        } else if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
            Some(Self::TarGz)
        } else {
            None
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub files: u64,
    pub bytes: u64,
}

/// Percorso relativo sicuro: solo componenti normali (`./` iniziale ammesso).
fn safe_rel(p: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            _ => bail!("percorso non ammesso nell'archivio: {}", p.display()),
        }
    }
    Ok(out)
}

struct Sink<'a> {
    dest: &'a Dir,
    owner: Option<u32>,
    limit: u64,
    report: Report,
}

impl Sink<'_> {
    fn own(&self, f: &std::fs::File) -> Result<()> {
        if let Some(uid) = self.owner {
            fchown(f, Some(uid), Some(uid))?;
        }
        Ok(())
    }

    fn dir(&mut self, rel: &Path) -> Result<()> {
        if rel.as_os_str().is_empty() {
            return Ok(());
        }
        let mut cur = PathBuf::new();
        for c in rel.components() {
            cur.push(c);
            if !self.dest.exists(&cur) {
                self.dest.create_dir(&cur).with_context(|| format!("cartella {}", cur.display()))?;
                // Non open_dir: il suo descrittore O_PATH non accetta fchown.
                self.own(&self.dest.open_with(&cur, cap_std::fs::OpenOptions::new().read(true))?.into_std())?;
            } else if !self.dest.symlink_metadata(&cur)?.is_dir() {
                bail!("{} esiste e non è una cartella", cur.display());
            }
        }
        Ok(())
    }

    fn file(&mut self, rel: &Path, data: &mut dyn Read) -> Result<()> {
        if let Some(parent) = rel.parent() {
            self.dir(parent)?;
        }
        if self.dest.symlink_metadata(rel).is_ok_and(|m| m.is_symlink()) {
            self.dest.remove_file(rel)?;
        }
        let mut f = self.dest.create(rel).with_context(|| format!("file {}", rel.display()))?.into_std();
        self.own(&f)?;
        let room = self.limit.saturating_sub(self.report.bytes);
        let n = std::io::copy(&mut data.take(room.saturating_add(1)), &mut f)?;
        if n > room {
            bail!("l'archivio supera il limite di {} byte", self.limit);
        }
        self.report.bytes += n;
        self.report.files += 1;
        Ok(())
    }
}

/// Estrae nella cartella `dest`; i link dell'archivio si saltano. `owner` riceve file e cartelle create.
pub fn extract(file: std::fs::File, kind: Kind, dest: &Dir, owner: Option<u32>, limit: u64) -> Result<Report> {
    let mut sink = Sink { dest, owner, limit, report: Report::default() };
    match kind {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(std::io::BufReader::new(file)).context("zip non valido")?;
            for i in 0..z.len() {
                let mut entry = z.by_index(i)?;
                let rel = safe_rel(Path::new(entry.name()?.as_ref()))?;
                if entry.is_symlink() {
                    continue;
                } else if entry.is_dir() {
                    sink.dir(&rel)?;
                } else {
                    sink.file(&rel, &mut entry)?;
                }
            }
        }
        Kind::TarGz => {
            let mut t = tar::Archive::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)));
            for entry in t.entries().context("tar.gz non valido")? {
                let mut entry = entry?;
                let rel = safe_rel(&entry.path()?)?;
                match entry.header().entry_type() {
                    tar::EntryType::Directory => sink.dir(&rel)?,
                    tar::EntryType::Regular | tar::EntryType::Continuous => sink.file(&rel, &mut entry)?,
                    _ => continue,
                }
            }
        }
    }
    Ok(sink.report)
}

/// Archivi come "public_html/..." o "sito/...": se c'è una sola cartella, il suo contenuto sale di un livello.
pub fn flatten_single_dir(dir: &Dir) -> Result<bool> {
    let entries: Vec<_> = dir.entries()?.collect::<std::io::Result<_>>()?;
    let [only] = entries.as_slice() else { return Ok(false) };
    if !only.file_type()?.is_dir() {
        return Ok(false);
    }
    let tmp = ".alveare-flatten";
    dir.rename(only.file_name(), dir, tmp)?;
    for child in dir.read_dir(tmp)? {
        let name = child?.file_name();
        dir.rename(Path::new(tmp).join(&name), dir, &name)?;
    }
    dir.remove_dir(tmp)?;
    Ok(true)
}

/// File e cartelle sotto `rel` di `root`, in ordine, senza link simbolici. Si legge sempre attraverso
/// la `Dir` (cap-std): un link messo dal sito a metà lavoro non porta fuori dalla sua cartella.
fn walk(root: &Dir, rel: &Path, out: &mut Vec<(PathBuf, bool)>) -> Result<()> {
    let mut entries: Vec<_> = if rel.as_os_str().is_empty() { root.entries()? } else { root.read_dir(rel)? }.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let ty = e.file_type()?;
        let child = rel.join(e.file_name());
        if ty.is_dir() {
            out.push((child.clone(), true));
            walk(root, &child, out)?;
        } else if ty.is_file() {
            out.push((child, false));
        }
    }
    Ok(())
}

fn tree(root: &Dir, rel: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let mut out = Vec::new();
    walk(root, rel, &mut out)?;
    Ok(out.into_iter().map(|(p, d)| (p.strip_prefix(rel).map(Path::to_path_buf).unwrap_or(p), d)).collect())
}

fn is_dir(root: &Dir, rel: &Path) -> Result<bool> {
    Ok(rel.as_os_str().is_empty() || root.symlink_metadata(rel)?.is_dir())
}

pub struct TarGzWriter {
    b: tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
}

impl TarGzWriter {
    pub fn new(out: std::fs::File) -> Self {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(out, flate2::Compression::default()));
        b.follow_symlinks(false);
        Self { b }
    }

    /// Aggiunge `rel` di `root` come `prefix/...`; `skip` esclude percorsi relativi a `rel` (es. il `.env`).
    pub fn add_tree_except(&mut self, root: &Dir, rel: &Path, prefix: &Path, skip: &[&str]) -> Result<()> {
        if !is_dir(root, rel)? {
            return self.add_entry(root, rel, prefix);
        }
        for (child, is_dir) in tree(root, rel)? {
            if skip.iter().any(|s| child.starts_with(s)) {
                continue;
            }
            let name = prefix.join(&child);
            if is_dir {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(0o755);
                h.set_size(0);
                h.set_mtime(chrono::Utc::now().timestamp().max(0) as u64);
                self.b.append_data(&mut h, &name, std::io::empty())?;
            } else {
                self.add_entry(root, &rel.join(&child), &name)?;
            }
        }
        Ok(())
    }

    pub fn add_tree(&mut self, root: &Dir, rel: &Path, prefix: &Path) -> Result<()> {
        self.add_tree_except(root, rel, prefix, &[])
    }

    fn add_entry(&mut self, root: &Dir, rel: &Path, name: &Path) -> Result<()> {
        let f = root.open(rel)?.into_std();
        let meta = f.metadata()?;
        let mut h = tar::Header::new_gnu();
        h.set_metadata(&meta);
        self.b.append_data(&mut h, name, f)?;
        Ok(())
    }

    /// Un file del pannello (dump, ecc.), non del sito.
    pub fn add_file(&mut self, src: &Path, name: &Path) -> Result<()> {
        self.b.append_path_with_name(src, name)?;
        Ok(())
    }

    pub fn add_bytes(&mut self, name: &Path, data: &[u8]) -> Result<()> {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(chrono::Utc::now().timestamp().max(0) as u64);
        h.set_entry_type(tar::EntryType::Regular);
        self.b.append_data(&mut h, name, data)?;
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        self.b.into_inner()?.finish()?.sync_all()?;
        Ok(())
    }
}

pub struct ZipWriter {
    z: zip::ZipWriter<std::fs::File>,
}

impl ZipWriter {
    pub fn new(out: std::fs::File) -> Self {
        Self { z: zip::ZipWriter::new(out) }
    }

    pub fn add_tree(&mut self, root: &Dir, rel: &Path, prefix: &Path) -> Result<()> {
        let opts = zip::write::SimpleFileOptions::default();
        if !is_dir(root, rel)? {
            return self.add_file(root, rel, prefix);
        }
        self.z.add_directory(format!("{}/", prefix.display()), opts)?;
        for (child, is_dir) in tree(root, rel)? {
            let name = prefix.join(&child);
            if is_dir {
                self.z.add_directory(format!("{}/", name.display()), opts)?;
            } else {
                self.add_file(root, &rel.join(&child), &name)?;
            }
        }
        Ok(())
    }

    fn add_file(&mut self, root: &Dir, rel: &Path, name: &Path) -> Result<()> {
        let mut f = root.open(rel)?.into_std();
        let big = f.metadata()?.len() > u32::MAX as u64;
        let opts = zip::write::SimpleFileOptions::default().large_file(big);
        self.z.start_file(name.to_string_lossy(), opts)?;
        std::io::copy(&mut f, &mut self.z)?;
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        self.z.finish()?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    use std::io::Write;

    fn dir(t: &tempfile::TempDir) -> Dir {
        Dir::open_ambient_dir(t.path(), ambient_authority()).unwrap()
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let f = tempfile::NamedTempFile::new().unwrap();
        let mut z = zip::ZipWriter::new(f.reopen().unwrap());
        for (name, data) in entries {
            if name.ends_with('/') {
                z.add_directory(*name, zip::write::SimpleFileOptions::default()).unwrap();
            } else {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(data).unwrap();
            }
        }
        z.finish().unwrap();
        f
    }

    #[test]
    fn kind_from_extension() {
        assert_eq!(Kind::of("sito.ZIP"), Some(Kind::Zip));
        assert_eq!(Kind::of("a.tar.gz"), Some(Kind::TarGz));
        assert_eq!(Kind::of("a.tgz"), Some(Kind::TarGz));
        assert_eq!(Kind::of("a.rar"), None);
    }

    #[test]
    fn zip_extracts_nested_files() {
        let t = tempfile::tempdir().unwrap();
        let z = zip_of(&[("wp-content/", b""), ("wp-content/themes/a/style.css", b"body{}"), ("index.php", b"<?php")]);
        let r = extract(std::fs::File::open(z.path()).unwrap(), Kind::Zip, &dir(&t), None, u64::MAX).unwrap();
        assert_eq!(r, Report { files: 2, bytes: 11 });
        assert_eq!(std::fs::read(t.path().join("wp-content/themes/a/style.css")).unwrap(), b"body{}");
    }

    #[test]
    fn zip_slip_and_absolute_paths_are_rejected() {
        for bad in ["../fuori.php", "a/../../fuori.php", "/etc/passwd"] {
            let t = tempfile::tempdir().unwrap();
            let z = zip_of(&[(bad, b"x")]);
            assert!(extract(std::fs::File::open(z.path()).unwrap(), Kind::Zip, &dir(&t), None, u64::MAX).is_err(), "{bad}");
        }
    }

    #[test]
    fn existing_symlink_in_destination_is_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        let t = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), t.path().join("link")).unwrap();
        let z = zip_of(&[("link/x.php", b"x")]);
        assert!(extract(std::fs::File::open(z.path()).unwrap(), Kind::Zip, &dir(&t), None, u64::MAX).is_err());
        assert!(!outside.path().join("x.php").exists());
    }

    #[test]
    fn size_limit_stops_archive_bombs() {
        let t = tempfile::tempdir().unwrap();
        let z = zip_of(&[("grande.bin", &[0u8; 4096])]);
        assert!(extract(std::fs::File::open(z.path()).unwrap(), Kind::Zip, &dir(&t), None, 1000).is_err());
    }

    #[test]
    fn tar_gz_round_trip_skips_symlinks() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("public/wp-content")).unwrap();
        std::fs::write(src.path().join("public/index.php"), b"<?php").unwrap();
        std::fs::write(src.path().join("public/wp-content/a.txt"), b"ciao").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.path().join("public/passwd")).unwrap();
        let out = tempfile::NamedTempFile::new().unwrap();
        let mut w = TarGzWriter::new(out.reopen().unwrap());
        w.add_tree(&dir(&src), Path::new(""), Path::new("sito")).unwrap();
        w.add_bytes(Path::new("LEGGIMI.txt"), b"leggi").unwrap();
        w.finish().unwrap();
        let t = tempfile::tempdir().unwrap();
        let r = extract(std::fs::File::open(out.path()).unwrap(), Kind::TarGz, &dir(&t), None, u64::MAX).unwrap();
        assert_eq!(r.files, 3);
        assert_eq!(std::fs::read(t.path().join("sito/public/wp-content/a.txt")).unwrap(), b"ciao");
        assert_eq!(std::fs::read(t.path().join("LEGGIMI.txt")).unwrap(), b"leggi");
        assert!(!t.path().join("sito/public/passwd").exists());
    }

    #[test]
    fn zip_writer_round_trip() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("tema/css")).unwrap();
        std::fs::write(src.path().join("tema/css/a.css"), b"a{}").unwrap();
        let out = tempfile::NamedTempFile::new().unwrap();
        let mut w = ZipWriter::new(out.reopen().unwrap());
        w.add_tree(&dir(&src), Path::new("tema"), Path::new("tema")).unwrap();
        w.finish().unwrap();
        let t = tempfile::tempdir().unwrap();
        extract(std::fs::File::open(out.path()).unwrap(), Kind::Zip, &dir(&t), None, u64::MAX).unwrap();
        assert_eq!(std::fs::read(t.path().join("tema/css/a.css")).unwrap(), b"a{}");
    }

    #[test]
    fn archives_never_follow_links_out_of_the_site() {
        let (outside, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::write(outside.path().join("segreto.txt"), "chiave").unwrap();
        std::os::unix::fs::symlink(outside.path(), src.path().join("altro")).unwrap();
        std::fs::write(src.path().join("a.txt"), "a").unwrap();
        let out = tempfile::NamedTempFile::new().unwrap();
        let mut w = ZipWriter::new(out.reopen().unwrap());
        assert!(w.add_tree(&dir(&src), Path::new("altro/segreto.txt"), Path::new("x")).is_err(), "un link verso fuori non si apre");
        w.add_tree(&dir(&src), Path::new(""), Path::new("sito")).unwrap();
        w.finish().unwrap();
        let names: Vec<String> = zip::ZipArchive::new(std::fs::File::open(out.path()).unwrap()).unwrap().file_names().map(|n| n.unwrap().to_string()).collect();
        assert!(names.iter().all(|n| !n.contains("segreto")), "{names:?}");
    }

    #[test]
    fn single_top_folder_is_flattened() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("public_html/wp-content")).unwrap();
        std::fs::write(t.path().join("public_html/index.php"), b"x").unwrap();
        assert!(flatten_single_dir(&dir(&t)).unwrap());
        assert!(t.path().join("index.php").exists() && t.path().join("wp-content").is_dir());
        assert!(!t.path().join("public_html").exists());
        // Con due voci non si tocca nulla.
        assert!(!flatten_single_dir(&dir(&t)).unwrap());
    }
}
