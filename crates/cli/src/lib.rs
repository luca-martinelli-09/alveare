//! CLI di Alveare: parla con l'API del pannello con un token personale, quindi ha esattamente
//! i permessi dell'utente. Output leggibile o JSON (`--json`) per gli script.
use std::io::Write;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub mod commands;

pub struct Api {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Api {
    pub fn new(base: &str, token: &str) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder().user_agent(concat!("alveare-cli/", env!("CARGO_PKG_VERSION"))).build().expect("client HTTP");
        Self { http, base: base.trim_end_matches('/').to_string(), token: token.to_string() }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}{path}", self.base)).bearer_auth(&self.token)
    }

    async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body: Value = resp.json().await.unwrap_or_default();
        let msg = body["error"].as_str().unwrap_or("errore sconosciuto").to_string();
        bail!("{msg} ({})", status.as_u16())
    }

    pub async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut req = self.request(method, path);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = Self::check(req.send().await.map_err(|e| anyhow::anyhow!("pannello non raggiungibile: {}", e.without_url()))?).await?;
        let text = resp.text().await?;
        Ok(if text.is_empty() { Value::Null } else { serde_json::from_str(&text).context("risposta non JSON")? })
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.call(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    /// Un sito per id, dominio (anche alias) o slug.
    pub async fn site_id(&self, which: &str) -> Result<i64> {
        let sites = self.get("/api/sites").await?;
        let list = sites.as_array().context("elenco dei siti non valido")?;
        let found = list.iter().find(|s| s["id"].as_i64().map(|i| i.to_string()).as_deref() == Some(which) || s["domain"] == which || s["slug"] == which);
        if let Some(id) = found.and_then(|s| s["id"].as_i64()) {
            return Ok(id);
        }
        for s in list {
            let id = s["id"].as_i64().unwrap_or_default();
            let domains = self.get(&format!("/api/sites/{id}/domains")).await.unwrap_or_default();
            if domains.as_array().into_iter().flatten().any(|d| d["hostname"] == which) {
                return Ok(id);
            }
        }
        bail!("sito non trovato: {which}")
    }

    /// Segue un job stampando le righe nuove del log; errore se il job fallisce.
    pub async fn wait_job(&self, job: i64, out: &mut impl Write, every: std::time::Duration) -> Result<Value> {
        let mut shown = 0;
        loop {
            let j = self.get(&format!("/api/jobs/{job}")).await?;
            let log = j["log"].as_array().cloned().unwrap_or_default();
            for line in log.iter().skip(shown) {
                writeln!(out, "{}", line.as_str().unwrap_or_default())?;
            }
            shown = log.len();
            match j["status"].as_str() {
                Some("done") => return Ok(j),
                Some("failed") => bail!("job {job} fallito"),
                _ => tokio::time::sleep(every).await,
            }
        }
    }

    /// Avvia un job (risposta `{"job": id}`) e lo segue fino alla fine.
    pub async fn run_job(&self, path: &str, body: Value, out: &mut impl Write) -> Result<Value> {
        let started = self.post(path, body).await?;
        let job = started["job"].as_i64().context("risposta senza job")?;
        self.wait_job(job, out, std::time::Duration::from_secs(1)).await
    }

    pub async fn download(&self, path: &str, dest: &std::path::Path) -> Result<u64> {
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let resp = Self::check(self.request(reqwest::Method::GET, path).send().await.map_err(|e| anyhow::anyhow!("{}", e.without_url()))?).await?;
        let mut file = tokio::fs::File::create(dest).await.with_context(|| format!("creazione di {}", dest.display()))?;
        let mut stream = resp.bytes_stream();
        let mut total = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            total += chunk.len() as u64;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(total)
    }

    /// Righe del log in tempo reale (Server-Sent Events), finché il pannello chiude.
    pub async fn follow(&self, path: &str, mut each: impl FnMut(Value) -> Result<()>) -> Result<()> {
        use futures_util::StreamExt;
        let resp = Self::check(self.request(reqwest::Method::GET, path).send().await.map_err(|e| anyhow::anyhow!("{}", e.without_url()))?).await?;
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(i) = buf.find('\n') {
                let line: String = buf.drain(..=i).collect();
                if let Some(data) = line.trim_end().strip_prefix("data:") {
                    let v: Value = serde_json::from_str(data.trim()).unwrap_or(Value::Null);
                    if !v.is_null() {
                        each(v)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Configurazione salvata da `alveare login`: URL del pannello e token.
pub mod config {
    use super::*;
    use std::path::PathBuf;

    pub fn path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("ALVEARE_CONFIG") {
            return Some(PathBuf::from(p));
        }
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.map(|b| b.join("alveare").join("config.json"))
    }

    pub fn load() -> Option<(String, String)> {
        load_from(&path()?)
    }

    pub fn load_from(p: &std::path::Path) -> Option<(String, String)> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
        Some((v["url"].as_str()?.to_string(), v["token"].as_str()?.to_string()))
    }

    pub fn save(url: &str, token: &str) -> Result<PathBuf> {
        let p = path().context("cartella di configurazione non trovata (imposta ALVEARE_CONFIG)")?;
        save_to(&p, url, token)?;
        Ok(p)
    }

    /// Il token resta leggibile solo dall'utente.
    pub fn save_to(p: &std::path::Path, url: &str, token: &str) -> Result<()> {
        std::fs::create_dir_all(p.parent().context("percorso senza cartella")?)?;
        std::fs::write(p, json!({ "url": url, "token": token }).to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}
