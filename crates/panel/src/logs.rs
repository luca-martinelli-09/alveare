//! Log dei siti: tail, tail -f per gli SSE, parsing dell'access log JSON di Caddy, rotazione.
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;

const BLOCK: u64 = 64 * 1024;
const MAX_LINE: usize = 16 * 1024;

/// Al più MAX_LINE byte, senza spezzare un carattere.
fn clip(line: &str) -> &str {
    &line[..line.floor_char_boundary(MAX_LINE)]
}

/// Ultime `n` righe, leggendo a blocchi dalla fine.
pub fn tail(path: &Path, n: usize) -> std::io::Result<Vec<String>> {
    match fs::File::open(path) {
        Ok(f) => tail_file(f, n),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e),
    }
}

pub fn tail_file(mut f: fs::File, n: usize) -> std::io::Result<Vec<String>> {
    let len = f.metadata()?.len();
    let mut pos = len;
    let mut buf: Vec<u8> = Vec::new();
    while pos > 0 && buf.iter().filter(|&&b| b == b'\n').count() <= n {
        let step = BLOCK.min(pos);
        pos -= step;
        f.seek(SeekFrom::Start(pos))?;
        let mut chunk = vec![0; step as usize];
        f.read_exact(&mut chunk)?;
        chunk.extend_from_slice(&buf);
        buf = chunk;
    }
    let text = String::from_utf8_lossy(&buf);
    let all: Vec<&str> = text.lines().collect();
    // Se non siamo partiti dall'inizio, la prima riga è a metà.
    let skip = usize::from(pos > 0);
    let usable = &all[skip.min(all.len())..];
    Ok(usable[usable.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect())
}

/// Righe nuove man mano che arrivano; riapre il file se viene ruotato.
/// Il task si ferma quando il ricevente viene chiuso (client SSE disconnesso).
pub fn follow(path: PathBuf, every: Duration) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut pos = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut ino = fs::metadata(&path).map(|m| m.ino()).ok();
        let mut partial = String::new();
        loop {
            tokio::time::sleep(every).await;
            if tx.is_closed() {
                return;
            }
            let Ok(meta) = fs::metadata(&path) else { continue };
            if Some(meta.ino()) != ino || meta.len() < pos {
                ino = Some(meta.ino());
                pos = 0;
                partial.clear();
            }
            if meta.len() == pos {
                continue;
            }
            let Ok(mut f) = fs::File::open(&path) else { continue };
            if f.seek(SeekFrom::Start(pos)).is_err() {
                continue;
            }
            let mut buf = Vec::new();
            let Ok(read) = f.take(4 * 1024 * 1024).read_to_end(&mut buf) else { continue };
            pos += read as u64;
            partial.push_str(&String::from_utf8_lossy(&buf));
            while let Some(i) = partial.find('\n') {
                let line: String = partial.drain(..=i).collect();
                let line = clip(line.trim_end_matches(['\n', '\r'])).to_string();
                if tx.send(line).await.is_err() {
                    return;
                }
            }
        }
    });
    rx
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AccessEntry {
    pub ts: i64,
    pub ip: String,
    pub method: String,
    pub host: String,
    pub uri: String,
    pub status: u16,
    pub duration_ms: u64,
    pub size: u64,
    pub user_agent: String,
}

/// Una riga JSON dell'access log di Caddy; l'IP è quello reale (client_ip).
pub fn parse_access(line: &str) -> Option<AccessEntry> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let r = &v["request"];
    let s = |x: &serde_json::Value| x.as_str().unwrap_or_default().to_string();
    Some(AccessEntry {
        ts: v["ts"].as_f64()? as i64,
        ip: s(if r["client_ip"].is_string() { &r["client_ip"] } else { &r["remote_ip"] }),
        method: s(&r["method"]),
        host: s(&r["host"]),
        uri: s(&r["uri"]),
        status: v["status"].as_u64()? as u16,
        duration_ms: (v["duration"].as_f64().unwrap_or(0.0) * 1000.0) as u64,
        size: v["size"].as_u64().unwrap_or(0),
        user_agent: r["headers"]["User-Agent"][0].as_str().unwrap_or_default().to_string(),
    })
}

/// Ruota se il file supera `max`: file → .1 → … → .keep, il più vecchio sparisce.
/// Il nuovo file vuoto appartiene a `owner` (l'utente del sito per l'error log di PHP).
pub fn rotate(path: &Path, max: u64, keep: u32, owner: Option<u32>) -> std::io::Result<bool> {
    match fs::metadata(path) {
        Ok(m) if m.len() > max => {}
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    }
    let numbered = |n: u32| PathBuf::from(format!("{}.{n}", path.display()));
    let _ = fs::remove_file(numbered(keep));
    for n in (1..keep).rev() {
        if numbered(n).exists() {
            fs::rename(numbered(n), numbered(n + 1))?;
        }
    }
    fs::rename(path, numbered(1))?;
    fs::write(path, "")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o640))?;
    if let Some(uid) = owner {
        std::os::unix::fs::chown(path, Some(uid), Some(uid))?;
    }
    Ok(true)
}

/// Il log attuale e quelli ruotati, dal più recente: `php-error.log.1`, `.2`… (pannello) o
/// `access-<data>.log.gz` (Caddy).
pub fn log_files(dir: &Path, base: &str) -> Vec<PathBuf> {
    let stem = base.strip_suffix(".log").unwrap_or(base);
    let mut numbered: Vec<(u32, PathBuf)> = Vec::new();
    let mut dated: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(n) = name.strip_prefix(&format!("{base}.")).and_then(|n| n.parse().ok()) {
            numbered.push((n, entry.path()));
        } else if name.starts_with(&format!("{stem}-")) && (name.ends_with(".log") || name.ends_with(".log.gz")) {
            dated.push(entry.path());
        }
    }
    numbered.sort();
    dated.sort();
    dated.reverse();
    let current = dir.join(base);
    current.exists().then_some(current).into_iter().chain(numbered.into_iter().map(|(_, p)| p)).chain(dated).collect()
}

fn file_lines(path: &Path) -> std::io::Result<Vec<String>> {
    let f = fs::File::open(path)?;
    let mut text = Vec::new();
    if path.extension().is_some_and(|e| e == "gz") {
        flate2::read::GzDecoder::new(f).read_to_end(&mut text)?;
    } else {
        f.take(512 * 1024 * 1024).read_to_end(&mut text)?;
    }
    Ok(String::from_utf8_lossy(&text).lines().map(String::from).collect())
}

/// Righe che soddisfano `keep`, dalla più recente, al massimo `limit`.
pub fn search(files: &[PathBuf], keep: impl Fn(&str) -> bool, limit: usize) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    for f in files {
        for line in file_lines(f)?.into_iter().rev() {
            if keep(&line) {
                out.push(line);
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorGroup {
    pub message: String,
    pub count: u64,
    pub last: String,
}

/// Errori PHP raggruppati per messaggio (senza data), il più frequente prima. Gli stack trace si saltano.
pub fn php_error_groups(lines: &[String]) -> Vec<ErrorGroup> {
    let mut groups: Vec<ErrorGroup> = Vec::new();
    for line in lines {
        let (when, msg) = match line.strip_prefix('[').and_then(|r| r.split_once("] ")) {
            Some((w, m)) => (w.to_string(), m),
            None => (String::new(), line.as_str()),
        };
        let rest = msg.trim_start_matches("PHP").trim_start();
        if msg.starts_with("PHP Stack trace") || rest.split_once(". ").is_some_and(|(n, _)| n.parse::<u32>().is_ok()) || msg.trim().is_empty() {
            continue;
        }
        let message = msg.split_whitespace().collect::<Vec<_>>().join(" ");
        match groups.iter_mut().find(|g| g.message == message) {
            Some(g) => {
                g.count += 1;
                g.last = when;
            }
            None => groups.push(ErrorGroup { message, count: 1, last: when }),
        }
    }
    groups.sort_by(|a, b| b.count.cmp(&a.count));
    groups
}

/// Elimina i log ruotati più vecchi di `days` giorni (mai quelli attuali); restituisce quanti.
pub fn purge_old_logs(dir: &Path, days: u32, now: std::time::SystemTime) -> std::io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Ruotati: `<nome>.log.<n>` del pannello o `<nome>-<data>.log[.gz]` di Caddy.
        let rotated = name.rsplit_once(".log.").is_some_and(|(_, n)| n.parse::<u32>().is_ok() || n == "gz")
            || name.split_once('-').is_some_and(|(_, d)| d.starts_with(|c: char| c.is_ascii_digit()) && name.ends_with(".log"));
        let age = entry.metadata()?.modified().ok().and_then(|m| now.duration_since(m).ok()).unwrap_or_default();
        if rotated && age > Duration::from_secs(u64::from(days) * 86400) {
            fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("riga {i}\n")).collect()
    }

    #[test]
    fn long_lines_are_clipped_without_splitting_a_character() {
        let line = format!("a{}", "è".repeat(MAX_LINE));
        assert_eq!(clip(&line).len(), MAX_LINE - 1);
        assert_eq!(clip("corta"), "corta");
    }

    #[test]
    fn tail_returns_last_lines_of_big_files() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.log");
        fs::write(&p, lines(20_000)).unwrap();
        let last = tail(&p, 3).unwrap();
        assert_eq!(last, ["riga 19998", "riga 19999", "riga 20000"]);
        assert_eq!(tail(&p, 50_000).unwrap().len(), 20_000);
    }

    #[test]
    fn tail_of_missing_or_empty_file_is_empty() {
        let t = tempfile::tempdir().unwrap();
        assert!(tail(&t.path().join("nessuno.log"), 10).unwrap().is_empty());
        fs::write(t.path().join("vuoto.log"), "").unwrap();
        assert!(tail(&t.path().join("vuoto.log"), 10).unwrap().is_empty());
    }

    #[test]
    fn caddy_access_line_is_parsed() {
        let line = r#"{"level":"info","ts":1798000000.123,"logger":"http.log.access","msg":"handled request","request":{"remote_ip":"172.18.0.5","client_ip":"198.51.100.9","proto":"HTTP/1.1","method":"GET","host":"sona.it","uri":"/wp-login.php?x=1","headers":{"User-Agent":["Mozilla/5.0"]}},"bytes_read":0,"user_id":"","duration":0.0421,"size":5120,"status":404}"#;
        let e = parse_access(line).unwrap();
        assert_eq!(e.ip, "198.51.100.9");
        assert_eq!((e.method.as_str(), e.uri.as_str(), e.status), ("GET", "/wp-login.php?x=1", 404));
        assert_eq!(e.user_agent, "Mozilla/5.0");
        assert_eq!(e.duration_ms, 42);
        assert_eq!(e.ts, 1798000000);
        assert!(parse_access("non json").is_none());
    }

    #[test]
    fn rotation_shifts_files_and_drops_the_oldest() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("php-error.log");
        fs::write(&p, "x".repeat(100)).unwrap();
        fs::write(t.path().join("php-error.log.1"), "uno").unwrap();
        fs::write(t.path().join("php-error.log.2"), "due").unwrap();
        assert!(!rotate(&p, 1000, 2, None).unwrap(), "sotto soglia non ruota");
        assert!(rotate(&p, 50, 2, None).unwrap());
        assert_eq!(fs::read_to_string(&p).unwrap(), "");
        assert_eq!(fs::read_to_string(t.path().join("php-error.log.1")).unwrap(), "x".repeat(100));
        assert_eq!(fs::read_to_string(t.path().join("php-error.log.2")).unwrap(), "uno");
        assert!(!t.path().join("php-error.log.3").exists());
    }

    #[test]
    fn log_files_are_current_then_rotated_newest_first() {
        let t = tempfile::tempdir().unwrap();
        for f in ["php-error.log", "php-error.log.2", "php-error.log.1", "access.log", "access-2026-10-28T01-00-00.000.log.gz", "access-2026-10-29T01-00-00.000.log.gz", "altro.log"] {
            fs::write(t.path().join(f), "").unwrap();
        }
        let names = |base| log_files(t.path(), base).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names("php-error.log"), ["php-error.log", "php-error.log.1", "php-error.log.2"]);
        assert_eq!(names("access.log"), ["access.log", "access-2026-10-29T01-00-00.000.log.gz", "access-2026-10-28T01-00-00.000.log.gz"]);
    }

    #[test]
    fn search_reads_rotated_and_gzipped_files_newest_first() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("access.log"), "a 1.2.3.4 nuova\nb 5.6.7.8\n").unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"c 1.2.3.4 vecchia\n").unwrap();
        fs::write(t.path().join("access-2026-10-29T01-00-00.000.log.gz"), gz.finish().unwrap()).unwrap();
        let found = search(&log_files(t.path(), "access.log"), |l| l.contains("1.2.3.4"), 10).unwrap();
        assert_eq!(found, ["a 1.2.3.4 nuova", "c 1.2.3.4 vecchia"]);
        assert_eq!(search(&log_files(t.path(), "access.log"), |_| true, 2).unwrap(), ["b 5.6.7.8", "a 1.2.3.4 nuova"]);
    }

    #[test]
    fn php_errors_are_grouped_by_message() {
        let lines = [
            "[30-Oct-2026 06:00:01 Europe/Rome] PHP Warning:  Undefined variable $x in /srv/sites/a/public/x.php on line 3",
            "[30-Oct-2026 06:00:05 Europe/Rome] PHP Fatal error:  Uncaught Error: boom in /srv/sites/a/public/y.php:9",
            "[30-Oct-2026 06:00:05 Europe/Rome] PHP Stack trace:",
            "[30-Oct-2026 06:00:05 Europe/Rome] PHP   1. {main}() /srv/sites/a/public/y.php:0",
            "[30-Oct-2026 07:10:00 Europe/Rome] PHP Warning:  Undefined variable $x in /srv/sites/a/public/x.php on line 3",
        ]
        .map(String::from);
        let groups = php_error_groups(&lines);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].message, "PHP Warning: Undefined variable $x in /srv/sites/a/public/x.php on line 3");
        assert_eq!((groups[0].count, groups[0].last.as_str()), (2, "30-Oct-2026 07:10:00 Europe/Rome"));
        assert_eq!(groups[1].count, 1);
    }

    #[test]
    fn old_rotated_logs_are_purged_but_never_the_current_one() {
        let t = tempfile::tempdir().unwrap();
        for f in ["php-error.log", "php-error.log.1", "access-2026-01-01T00-00-00.000.log.gz"] {
            fs::write(t.path().join(f), "x").unwrap();
        }
        let now = std::time::SystemTime::now();
        assert_eq!(purge_old_logs(t.path(), 30, now).unwrap(), 0);
        let later = now + Duration::from_secs(31 * 86400);
        assert_eq!(purge_old_logs(t.path(), 30, later).unwrap(), 2);
        assert!(t.path().join("php-error.log").exists());
    }

    #[tokio::test]
    async fn follow_streams_appended_lines_and_survives_rotation() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("access.log");
        fs::write(&p, "vecchia\n").unwrap();
        let mut rx = follow(p.clone(), std::time::Duration::from_millis(20));
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"nuova 1\nnuova").unwrap();
        assert_eq!(rx.recv().await.unwrap(), "nuova 1");
        fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b" 2\n").unwrap();
        assert_eq!(rx.recv().await.unwrap(), "nuova 2");
        fs::rename(&p, t.path().join("access.log.1")).unwrap();
        fs::write(&p, "dopo rotazione\n").unwrap();
        assert_eq!(rx.recv().await.unwrap(), "dopo rotazione");
    }
}
