//! API di Cloudflare: hostname pubblici sul tunnel (ingress + CNAME) e regole WAF e di rate
//! limiting da un modello unico. Le regole del pannello hanno la descrizione che inizia con
//! "alveare:" e sostituiscono solo sé stesse: quelle messe a mano nella zona restano.
use alveare_core::site::Domain;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub const SERVICE: &str = "http://caddy:80";
pub const RULE_PREFIX: &str = "alveare:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Etichetta scelta nel .env (`CLOUDFLARE_<ETICHETTA>_...`), mostrata nel pannello.
    pub label: String,
    pub account_id: String,
    pub tunnel_id: String,
    pub token: String,
}

/// Account dal .env: per ogni etichetta servono `_API_TOKEN`, `_ACCOUNT_ID` e `_TUNNEL_ID`.
pub fn accounts_from(vars: &[(String, String)]) -> Result<Vec<Account>> {
    let get = |k: String| vars.iter().rev().find(|(n, _)| *n == k).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
    let mut labels: Vec<String> = vars
        .iter()
        .filter_map(|(k, _)| k.strip_prefix("CLOUDFLARE_")?.strip_suffix("_API_TOKEN").map(String::from))
        .collect();
    labels.sort();
    labels.dedup();
    labels
        .into_iter()
        .filter(|l| get(format!("CLOUDFLARE_{l}_API_TOKEN")).is_some())
        .map(|l| {
            let need = |s: &str| get(format!("CLOUDFLARE_{l}_{s}")).with_context(|| format!("CLOUDFLARE_{l}_{s} mancante"));
            Ok(Account { label: l.to_ascii_lowercase(), account_id: need("ACCOUNT_ID")?, tunnel_id: need("TUNNEL_ID")?, token: need("API_TOKEN")? })
        })
        .collect()
}

/// Zone in cui può stare un hostname, dalla più specifica: "www.sona.it" → ["www.sona.it", "sona.it"].
pub fn zone_candidates(host: &Domain) -> Vec<String> {
    let labels: Vec<&str> = host.as_str().split('.').collect();
    (0..labels.len() - 1).map(|i| labels[i..].join(".")).collect()
}

/// Ingress del tunnel con l'hostname verso Caddy, prima della regola finale (senza hostname).
pub fn ingress_with(config: &Value, host: &str) -> Value {
    let mut config = config.clone();
    let mut rules: Vec<Value> = config["ingress"].as_array().cloned().unwrap_or_default();
    if !rules.iter().any(|r| r["hostname"] == host) {
        let at = rules.iter().position(|r| r.get("hostname").is_none()).unwrap_or(rules.len());
        rules.insert(at, json!({ "hostname": host, "service": SERVICE }));
    }
    if !rules.iter().any(|r| r.get("hostname").is_none()) {
        rules.push(json!({ "service": "http_status:404" }));
    }
    config["ingress"] = Value::Array(rules);
    config
}

pub fn ingress_without(config: &Value, host: &str) -> Value {
    let mut config = config.clone();
    let rules: Vec<Value> = config["ingress"].as_array().cloned().unwrap_or_default().into_iter().filter(|r| r["hostname"] != host).collect();
    config["ingress"] = Value::Array(rules);
    config
}

/// Regole della zona con le nostre al posto delle vecchie nostre; le altre restano prima.
pub fn merge_rules(existing: &[Value], ours: &[Value]) -> Vec<Value> {
    let keep = existing.iter().filter(|r| !r["description"].as_str().is_some_and(|d| d.starts_with(RULE_PREFIX))).map(|r| {
        // Nel PUT di un ruleset non vanno id, versione e date delle regole esistenti.
        let mut r = r.clone();
        if let Some(o) = r.as_object_mut() {
            for k in ["id", "version", "last_updated", "ref"] {
                o.remove(k);
            }
        }
        r
    });
    keep.chain(ours.iter().cloned()).collect()
}

/// Modello di default, compatibile con il piano Free: blocco di xmlrpc.php e degli scanner
/// noti, rate limit sui POST a wp-login.php.
pub fn default_rules() -> Value {
    json!({
        "http_request_firewall_custom": [
            { "description": "alveare: blocca xmlrpc.php", "expression": "(http.request.uri.path eq \"/xmlrpc.php\")", "action": "block" },
            { "description": "alveare: scanner noti", "expression": "(lower(http.user_agent) contains \"sqlmap\") or (lower(http.user_agent) contains \"nikto\") or (lower(http.user_agent) contains \"wpscan\")", "action": "block" }
        ],
        "http_ratelimit": [
            {
                "description": "alveare: tentativi di login",
                "expression": "(http.request.uri.path eq \"/wp-login.php\" and http.request.method eq \"POST\")",
                "action": "block",
                "ratelimit": { "characteristics": ["ip.src", "cf.colo.id"], "period": 10, "requests_per_period": 5, "mitigation_timeout": 10 }
            }
        ]
    })
}

/// Il modello è un oggetto fase → regole; ogni regola ha descrizione "alveare:…", expression e action.
pub fn validate_rules(template: &Value) -> Result<()> {
    let phases = template.as_object().context("il modello deve essere un oggetto fase → regole")?;
    for (phase, rules) in phases {
        if !["http_request_firewall_custom", "http_ratelimit"].contains(&phase.as_str()) {
            bail!("fase non gestita: {phase}");
        }
        for r in rules.as_array().with_context(|| format!("{phase}: serve un elenco di regole"))? {
            if !r["description"].as_str().is_some_and(|d| d.starts_with(RULE_PREFIX)) {
                bail!("ogni regola deve avere una descrizione che inizia con \"{RULE_PREFIX}\"");
            }
            if !r["expression"].is_string() || !r["action"].is_string() {
                bail!("ogni regola deve avere expression e action");
            }
        }
    }
    Ok(())
}

pub const API: &str = "https://api.cloudflare.com/client/v4";

/// Client HTTPS del pannello (Cloudflare, Telegram): TLS con ring, niente toolchain C extra.
pub fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().user_agent("alveare").build().expect("client HTTP")
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    pub fn new(http: reqwest::Client, base: &str) -> Self {
        Self { http, base: base.trim_end_matches('/').to_string() }
    }

    /// Una chiamata all'API: il campo `result` se `success`, altrimenti gli errori di Cloudflare.
    /// `Ok(None)` per un 404 (es. una zona senza ruleset).
    async fn call(&self, acc: &Account, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Option<Value>> {
        let mut req = self.http.request(method.clone(), format!("{}{path}", self.base)).bearer_auth(&acc.token).timeout(std::time::Duration::from_secs(30));
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(|e| anyhow::anyhow!("Cloudflare non raggiungibile: {}", e.without_url()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let v: Value = resp.json().await.with_context(|| format!("risposta di Cloudflare non valida ({status})"))?;
        if v["success"] != true {
            let errors: Vec<String> = v["errors"].as_array().into_iter().flatten().map(|e| format!("{} {}", e["code"], e["message"].as_str().unwrap_or_default())).collect();
            bail!("Cloudflare {method} {path}: {}", errors.join("; "));
        }
        Ok(Some(v["result"].clone()))
    }

    async fn get(&self, acc: &Account, path: &str) -> Result<Value> {
        self.call(acc, reqwest::Method::GET, path, None).await?.with_context(|| format!("{path}: non trovato"))
    }

    /// La zona più specifica che contiene l'hostname.
    pub async fn zone_id(&self, acc: &Account, host: &Domain) -> Result<String> {
        for name in zone_candidates(host) {
            let found = self.get(acc, &format!("/zones?name={name}")).await?;
            if let Some(id) = found.as_array().and_then(|z| z.first()).and_then(|z| z["id"].as_str()) {
                return Ok(id.to_string());
            }
        }
        bail!("nessuna zona Cloudflare per {} in questo account", host.as_str())
    }

    fn tunnel_path(acc: &Account) -> String {
        format!("/accounts/{}/cfd_tunnel/{}/configurations", acc.account_id, acc.tunnel_id)
    }

    async fn put_ingress(&self, acc: &Account, change: impl Fn(&Value) -> Value) -> Result<()> {
        let current = self.get(acc, &Self::tunnel_path(acc)).await?;
        let config = change(&current["config"]);
        if config != current["config"] {
            self.call(acc, reqwest::Method::PUT, &Self::tunnel_path(acc), Some(json!({ "config": config }))).await?;
        }
        Ok(())
    }

    /// Hostname pubblico verso Caddy: regola di ingress del tunnel e CNAME proxato sul tunnel.
    pub async fn add_hostname(&self, acc: &Account, host: &Domain) -> Result<()> {
        self.put_ingress(acc, |c| ingress_with(c, host.as_str())).await?;
        let zone = self.zone_id(acc, host).await?;
        let target = format!("{}.cfargotunnel.com", acc.tunnel_id);
        let records = self.get(acc, &format!("/zones/{zone}/dns_records?name={}", host.as_str())).await?;
        let existing = records.as_array().and_then(|r| r.first()).cloned();
        let record = json!({ "type": "CNAME", "name": host.as_str(), "content": target, "proxied": true, "comment": "alveare" });
        match existing {
            Some(r) if r["type"] == "CNAME" && r["content"] == target.as_str() => {}
            Some(r) if r["type"] == "CNAME" => {
                let id = r["id"].as_str().context("record senza id")?;
                self.call(acc, reqwest::Method::PUT, &format!("/zones/{zone}/dns_records/{id}"), Some(record)).await?;
            }
            Some(r) => bail!("{} ha già un record {}: va tolto a mano", host.as_str(), r["type"].as_str().unwrap_or("?")),
            None => {
                self.call(acc, reqwest::Method::POST, &format!("/zones/{zone}/dns_records"), Some(record)).await?;
            }
        }
        Ok(())
    }

    /// Toglie l'hostname dal tunnel e il CNAME, solo se punta al nostro tunnel.
    pub async fn remove_hostname(&self, acc: &Account, host: &Domain) -> Result<()> {
        self.put_ingress(acc, |c| ingress_without(c, host.as_str())).await?;
        let zone = self.zone_id(acc, host).await?;
        let target = format!("{}.cfargotunnel.com", acc.tunnel_id);
        let records = self.get(acc, &format!("/zones/{zone}/dns_records?name={}", host.as_str())).await?;
        for r in records.as_array().into_iter().flatten().filter(|r| r["type"] == "CNAME" && r["content"] == target.as_str()) {
            let id = r["id"].as_str().context("record senza id")?;
            self.call(acc, reqwest::Method::DELETE, &format!("/zones/{zone}/dns_records/{id}"), None).await?;
        }
        Ok(())
    }

    /// Applica il modello di regole alla zona dell'hostname, fase per fase.
    pub async fn apply_rules(&self, acc: &Account, host: &Domain, template: &Value) -> Result<usize> {
        validate_rules(template)?;
        let zone = self.zone_id(acc, host).await?;
        let mut applied = 0;
        for (phase, ours) in template.as_object().into_iter().flatten() {
            let path = format!("/zones/{zone}/rulesets/phases/{phase}/entrypoint");
            let current = self.call(acc, reqwest::Method::GET, &path, None).await?;
            let existing: Vec<Value> = current.as_ref().and_then(|c| c["rules"].as_array().cloned()).unwrap_or_default();
            let ours = ours.as_array().cloned().unwrap_or_default();
            applied += ours.len();
            self.call(acc, reqwest::Method::PUT, &path, Some(json!({ "rules": merge_rules(&existing, &ours) }))).await?;
        }
        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn accounts_are_read_by_label() {
        let acc = accounts_from(&vars(&[
            ("CLOUDFLARE_BANDA_API_TOKEN", "t1"),
            ("CLOUDFLARE_BANDA_ACCOUNT_ID", "a1"),
            ("CLOUDFLARE_BANDA_TUNNEL_ID", "u1"),
            ("CLOUDFLARE_PROLOCO_API_TOKEN", "t2"),
            ("CLOUDFLARE_PROLOCO_ACCOUNT_ID", "a2"),
            ("CLOUDFLARE_PROLOCO_TUNNEL_ID", "u2"),
            ("CLOUDFLARE_VUOTO_API_TOKEN", ""),
        ]))
        .unwrap();
        assert_eq!(acc.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["banda", "proloco"]);
        assert_eq!(acc[1].tunnel_id, "u2");
        assert!(accounts_from(&vars(&[("CLOUDFLARE_X_API_TOKEN", "t")])).is_err(), "mancano account e tunnel");
    }

    #[test]
    fn zones_from_most_specific() {
        assert_eq!(zone_candidates(&Domain::parse("prove.www.sona.it").unwrap()), ["prove.www.sona.it", "www.sona.it", "sona.it"]);
    }

    #[test]
    fn ingress_gets_the_host_before_the_catch_all_once() {
        let cfg = json!({ "ingress": [{ "hostname": "a.it", "service": "http://caddy:80" }, { "service": "http_status:404" }], "warp-routing": {} });
        let added = ingress_with(&cfg, "b.it");
        assert_eq!(added["ingress"][1], json!({ "hostname": "b.it", "service": "http://caddy:80" }));
        assert_eq!(added["ingress"][2], json!({ "service": "http_status:404" }));
        assert_eq!(ingress_with(&added, "b.it"), added, "idempotente");
        assert!(added.get("warp-routing").is_some());
        assert_eq!(ingress_without(&added, "b.it"), cfg);
        let empty = ingress_with(&json!({}), "c.it");
        assert_eq!(empty["ingress"].as_array().unwrap().len(), 2, "serve sempre la regola finale");
    }

    #[test]
    fn our_rules_replace_only_our_rules() {
        let existing = [
            json!({ "id": "x1", "version": "3", "description": "regola a mano", "expression": "true", "action": "log" }),
            json!({ "id": "x2", "description": "alveare: vecchia", "expression": "true", "action": "block" }),
        ];
        let merged = merge_rules(&existing, default_rules()["http_request_firewall_custom"].as_array().unwrap());
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0], json!({ "description": "regola a mano", "expression": "true", "action": "log" }));
        assert!(merged[1..].iter().all(|r| r["description"].as_str().unwrap().starts_with("alveare:")));
    }

    #[test]
    fn templates_are_validated() {
        assert!(validate_rules(&default_rules()).is_ok());
        assert!(validate_rules(&json!({ "http_request_firewall_managed": [] })).is_err());
        assert!(validate_rules(&json!({ "http_ratelimit": [{ "description": "mia", "expression": "x", "action": "block" }] })).is_err());
        assert!(validate_rules(&json!([])).is_err());
    }

    /// Un Cloudflare finto: tunnel, zone, record DNS e ruleset in memoria.
    mod fake {
        use super::*;
        use axum::extract::{Path, Query, State};
        use axum::routing::{get, put};
        use axum::{Json, Router};
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        #[derive(Default)]
        pub struct Cf {
            pub ingress: Value,
            pub records: Vec<Value>,
            pub rulesets: HashMap<String, Vec<Value>>,
            pub calls: Vec<String>,
        }
        type S = Arc<Mutex<Cf>>;

        fn ok(v: Value) -> Json<Value> {
            Json(json!({ "success": true, "errors": [], "result": v }))
        }

        pub async fn start(cf: S) -> String {
            let app = Router::new()
                .route("/zones", get(|Query(q): Query<HashMap<String, String>>| async move {
                    ok(if q["name"] == "sona.it" { json!([{ "id": "z1", "name": "sona.it" }]) } else { json!([]) })
                }))
                .route("/accounts/{a}/cfd_tunnel/{t}/configurations", get(|State(s): State<S>| async move { ok(json!({ "config": s.lock().unwrap().ingress.clone() })) }).put(
                    |State(s): State<S>, Json(b): Json<Value>| async move {
                        let mut s = s.lock().unwrap();
                        s.calls.push("put ingress".into());
                        s.ingress = b["config"].clone();
                        ok(json!({}))
                    },
                ))
                .route("/zones/{z}/dns_records", get(|State(s): State<S>, Query(q): Query<HashMap<String, String>>| async move {
                    ok(Value::Array(s.lock().unwrap().records.iter().filter(|r| r["name"] == q["name"].as_str()).cloned().collect()))
                }).post(|State(s): State<S>, Json(b): Json<Value>| async move {
                    let mut s = s.lock().unwrap();
                    let mut r = b.clone();
                    r["id"] = json!(format!("r{}", s.records.len() + 1));
                    s.calls.push(format!("post dns {}", b["name"].as_str().unwrap()));
                    s.records.push(r);
                    ok(json!({}))
                }))
                .route("/zones/{z}/dns_records/{id}", axum::routing::delete(|State(s): State<S>, Path((_z, id)): Path<(String, String)>| async move {
                    let mut s = s.lock().unwrap();
                    s.calls.push(format!("delete dns {id}"));
                    s.records.retain(|r| r["id"] != id.as_str());
                    ok(json!({}))
                }))
                .route("/zones/{z}/rulesets/phases/{p}/entrypoint", get(|State(s): State<S>, Path((_z, p)): Path<(String, String)>| async move {
                    match s.lock().unwrap().rulesets.get(&p) {
                        Some(r) => Ok(ok(json!({ "rules": r }))),
                        None => Err(axum::http::StatusCode::NOT_FOUND),
                    }
                }))
                .route("/zones/{z}/rulesets/phases/{p}/entrypoint", put(|State(s): State<S>, Path((_z, p)): Path<(String, String)>, Json(b): Json<Value>| async move {
                    s.lock().unwrap().rulesets.insert(p, b["rules"].as_array().cloned().unwrap_or_default());
                    ok(json!({}))
                }))
                .with_state(cf);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            format!("http://{addr}")
        }
    }

    fn account() -> Account {
        Account { label: "banda".into(), account_id: "a1".into(), tunnel_id: "tun".into(), token: "t".into() }
    }

    #[tokio::test]
    async fn hostnames_are_added_and_removed_on_tunnel_and_dns() {
        let cf = std::sync::Arc::new(std::sync::Mutex::new(fake::Cf { ingress: json!({ "ingress": [{ "service": "http_status:404" }] }), ..Default::default() }));
        let client = Client::new(http_client(), &fake::start(cf.clone()).await);
        let host = Domain::parse("www.sona.it").unwrap();
        client.add_hostname(&account(), &host).await.unwrap();
        client.add_hostname(&account(), &host).await.unwrap();
        {
            let s = cf.lock().unwrap();
            assert_eq!(s.ingress["ingress"][0], json!({ "hostname": "www.sona.it", "service": "http://caddy:80" }));
            assert_eq!(s.records.len(), 1, "idempotente");
            assert_eq!((s.records[0]["content"].as_str(), s.records[0]["proxied"].as_bool()), (Some("tun.cfargotunnel.com"), Some(true)));
            assert_eq!(s.calls.iter().filter(|c| *c == "put ingress").count(), 1);
        }
        client.remove_hostname(&account(), &host).await.unwrap();
        let s = cf.lock().unwrap();
        assert!(s.records.is_empty());
        assert_eq!(s.ingress["ingress"], json!([{ "service": "http_status:404" }]));
        drop(s);
        let err = client.add_hostname(&account(), &Domain::parse("altro.org").unwrap()).await.unwrap_err();
        assert!(err.to_string().contains("nessuna zona"), "{err}");
    }

    #[tokio::test]
    async fn rules_are_merged_into_the_zone_phases() {
        let cf = std::sync::Arc::new(std::sync::Mutex::new(fake::Cf::default()));
        cf.lock().unwrap().rulesets.insert("http_request_firewall_custom".into(), vec![json!({ "id": "m", "description": "a mano", "expression": "true", "action": "log" })]);
        let client = Client::new(http_client(), &fake::start(cf.clone()).await);
        let n = client.apply_rules(&account(), &Domain::parse("sona.it").unwrap(), &default_rules()).await.unwrap();
        assert_eq!(n, 3);
        let s = cf.lock().unwrap();
        assert_eq!(s.rulesets["http_request_firewall_custom"].len(), 3, "la regola a mano resta");
        assert_eq!(s.rulesets["http_ratelimit"].len(), 1, "fase creata se mancava");
    }
}
