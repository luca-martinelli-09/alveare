//! Log dei siti: tail, tail -f per gli SSE, parsing dell'access log JSON di Caddy, rotazione.
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;

const BLOCK: u64 = 64 * 1024;
const MAX_LINE: usize = 16 * 1024;

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
                let mut line = line.trim_end_matches(['\n', '\r']).to_string();
                line.truncate(MAX_LINE);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("riga {i}\n")).collect()
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
