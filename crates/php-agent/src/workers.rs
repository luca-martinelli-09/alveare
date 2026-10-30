//! Supervisore dei worker (container php-workers): avvia i processi definiti dal pannello con
//! l'utente del sito, li riavvia se terminano (attesa crescente), li ferma oltre la memoria.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use alveare_core::agent::WorkerSpec;

/// Chiave di un processo: sito, worker, indice.
pub type Key = (String, String, u32);

/// Attesa prima del riavvio: raddoppia a ogni uscita ravvicinata, si azzera dopo un minuto di vita.
pub fn next_backoff(current: Duration, lived: Duration) -> Duration {
    if lived >= Duration::from_secs(60) { Duration::from_secs(1) } else { (current * 2).clamp(Duration::from_secs(1), Duration::from_secs(60)) }
}

/// Processi voluti dalle specifiche, uno per indice.
pub fn wanted(specs: &[WorkerSpec]) -> BTreeMap<Key, WorkerSpec> {
    specs.iter().flat_map(|s| (0..s.processes).map(move |i| ((s.slug.as_str().to_string(), s.name.clone(), i), s.clone()))).collect()
}

/// (da fermare, da avviare) passando da `running` a `wanted`: un processo con la specifica cambiata
/// si ferma e riparte.
pub fn diff(running: &BTreeMap<Key, WorkerSpec>, wanted: &BTreeMap<Key, WorkerSpec>) -> (Vec<Key>, Vec<Key>) {
    // Il numero di processi non conta: aggiungerne uno non riavvia gli altri.
    let same = |a: &WorkerSpec, b: &WorkerSpec| WorkerSpec { processes: 0, ..a.clone() } == WorkerSpec { processes: 0, ..b.clone() };
    let stop = running.iter().filter(|(k, spec)| !wanted.get(*k).is_some_and(|w| same(w, spec))).map(|(k, _)| k.clone()).collect();
    let start = wanted.iter().filter(|(k, spec)| !running.get(*k).is_some_and(|r| same(r, spec))).map(|(k, _)| k.clone()).collect();
    (stop, start)
}

/// VmRSS da /proc/<pid>/status, in MB.
pub fn rss_mb(status: &str) -> Option<u64> {
    let kb: u64 = status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb / 1024)
}

/// Il limite di PHP (memory_limit) è il primo argine; questo prende chi lo supera con estensioni o fork.
pub fn over_memory(rss: u64, limit_mb: u32) -> bool {
    rss > u64::from(limit_mb) + u64::from(limit_mb) / 4
}

#[derive(Debug, serde::Serialize)]
pub struct Status {
    pub slug: String,
    pub name: String,
    pub index: u32,
    pub pid: Option<u32>,
    pub uptime_s: u64,
    pub restarts: u32,
    pub rss_mb: Option<u64>,
}

pub struct Proc {
    pub spec: WorkerSpec,
    pub child: Option<std::process::Child>,
    pub started: Instant,
    pub restarts: u32,
    pub backoff: Duration,
    pub next_start: Instant,
}

const CONFIG: &str = "/srv/php/workers.json";

pub struct Supervisor {
    procs: BTreeMap<Key, Proc>,
}

static SUPERVISOR: std::sync::Mutex<Supervisor> = std::sync::Mutex::new(Supervisor { procs: BTreeMap::new() });

impl Supervisor {
    fn running_specs(&self) -> BTreeMap<Key, WorkerSpec> {
        self.procs.iter().map(|(k, p)| (k.clone(), p.spec.clone())).collect()
    }

    /// Allinea i processi alle specifiche: ferma quelli tolti o cambiati, prepara i nuovi.
    fn apply(&mut self, specs: &[WorkerSpec]) {
        let wanted = wanted(specs);
        let (stop, start) = diff(&self.running_specs(), &wanted);
        for key in stop {
            if let Some(p) = self.procs.remove(&key) {
                stop_child(p.child);
            }
        }
        let now = Instant::now();
        for key in start {
            let spec = wanted[&key].clone();
            self.procs.insert(key, Proc { spec, child: None, started: now, restarts: 0, backoff: Duration::from_secs(1), next_start: now });
        }
    }

    /// Un giro del supervisore: chi è uscito riparte dopo l'attesa, chi supera la memoria si ferma.
    fn tick(&mut self) {
        let now = Instant::now();
        for (key, p) in self.procs.iter_mut() {
            if let Some(child) = &mut p.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        p.backoff = next_backoff(p.backoff, now - p.started);
                        p.next_start = now + p.backoff;
                        p.restarts += 1;
                        p.child = None;
                        eprintln!("php-workers: {key:?} uscito ({status}), riparte tra {:?}", p.backoff);
                    }
                    Ok(None) => {
                        let rss = std::fs::read_to_string(format!("/proc/{}/status", child.id())).ok().and_then(|s| rss_mb(&s));
                        if rss.is_some_and(|r| over_memory(r, p.spec.memory_mb)) {
                            eprintln!("php-workers: {key:?} oltre {} MB, fermato", p.spec.memory_mb);
                            let _ = child.kill();
                        }
                    }
                    Err(_) => {}
                }
            } else if now >= p.next_start {
                match spawn(&p.spec) {
                    Ok(c) => {
                        p.child = Some(c);
                        p.started = now;
                    }
                    Err(e) => {
                        eprintln!("php-workers: avvio di {key:?}: {e}");
                        p.backoff = next_backoff(p.backoff, Duration::ZERO);
                        p.next_start = now + p.backoff;
                    }
                }
            }
        }
    }

    fn restart(&mut self, slug: &str, name: &str) -> usize {
        let mut n = 0;
        for ((s, w, _), p) in self.procs.iter_mut() {
            if s == slug && (name.is_empty() || w == name) {
                stop_child(p.child.take());
                p.backoff = Duration::from_secs(1);
                p.next_start = Instant::now();
                n += 1;
            }
        }
        n
    }

    fn status(&self) -> Vec<Status> {
        self.procs
            .iter()
            .map(|((slug, name, index), p)| {
                let pid = p.child.as_ref().map(|c| c.id());
                Status {
                    slug: slug.clone(),
                    name: name.clone(),
                    index: *index,
                    pid,
                    uptime_s: if pid.is_some() { p.started.elapsed().as_secs() } else { 0 },
                    restarts: p.restarts,
                    rss_mb: pid.and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/status")).ok()).and_then(|s| rss_mb(&s)),
                }
            })
            .collect()
    }
}

/// SIGTERM, poi SIGKILL dopo 10 secondi: i worker di Laravel finiscono il job in corso.
fn stop_child(child: Option<std::process::Child>) {
    let Some(mut child) = child else { return };
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = child.kill();
        let _ = child.wait();
    });
}

fn spawn(spec: &WorkerSpec) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let e = crate::plan::plan_worker(spec);
    let log_path = format!("{}/{}/worker-{}.log", alveare_core::php::LOGS_DIR, spec.slug.as_str(), spec.name);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    std::process::Command::new(&e.program)
        .args(&e.args)
        .current_dir(&e.cwd)
        .env_clear()
        .envs(e.env.iter().cloned())
        .uid(e.uid)
        .gid(e.gid)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
}

fn load_config() -> Result<Vec<WorkerSpec>, String> {
    let text = match std::fs::read_to_string(CONFIG) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let specs: Vec<WorkerSpec> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    match specs.iter().find_map(|s| s.validate().err()) {
        Some(e) => Err(e),
        None => Ok(specs),
    }
}

pub fn command(cmd: alveare_core::agent::Command) -> alveare_core::agent::Response {
    use alveare_core::agent::{Command, Response};
    let mut sup = SUPERVISOR.lock().expect("lock del supervisore");
    match cmd {
        Command::WorkersApply => match load_config() {
            Ok(specs) => {
                sup.apply(&specs);
                Response { ok: true, exit_code: None, output: format!("{} processi", sup.procs.len()) }
            }
            Err(e) => crate::fail(format!("workers.json: {e}")),
        },
        Command::WorkerRestart { slug, name } => Response { ok: true, exit_code: None, output: format!("{} processi riavviati", sup.restart(slug.as_str(), &name)) },
        Command::WorkersStatus => Response { ok: true, exit_code: None, output: serde_json::to_string(&sup.status()).expect("serializzazione") },
        _ => crate::fail("comando non previsto nel container php-workers"),
    }
}

/// Ruolo php-workers: niente FPM, solo il supervisore e il suo socket.
pub fn run() {
    match load_config() {
        Ok(specs) => SUPERVISOR.lock().expect("lock").apply(&specs),
        Err(e) => eprintln!("php-workers: workers.json non valido all'avvio: {e}"),
    }
    std::thread::spawn(|| crate::serve_on(alveare_core::agent::WORKERS_SOCKET));
    loop {
        SUPERVISOR.lock().expect("lock").tick();
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alveare_core::site::Slug;

    fn spec(name: &str, processes: u32, mem: u32) -> WorkerSpec {
        WorkerSpec { slug: Slug::parse("sona_it").unwrap(), uid: 20001, name: name.into(), args: vec!["artisan".into(), "queue:work".into()], processes, memory_mb: mem }
    }

    #[test]
    fn backoff_grows_and_resets_after_a_stable_minute() {
        assert_eq!(next_backoff(Duration::from_secs(1), Duration::from_secs(2)), Duration::from_secs(2));
        assert_eq!(next_backoff(Duration::from_secs(40), Duration::from_secs(2)), Duration::from_secs(60));
        assert_eq!(next_backoff(Duration::from_secs(40), Duration::from_secs(90)), Duration::from_secs(1));
    }

    #[test]
    fn only_changed_processes_are_restarted() {
        let running = wanted(&[spec("coda", 2, 128), spec("horizon", 1, 256)]);
        let next = wanted(&[spec("coda", 3, 128), spec("horizon", 1, 512)]);
        let (stop, start) = diff(&running, &next);
        assert_eq!(stop, [("sona_it".to_string(), "horizon".to_string(), 0)]);
        assert_eq!(start, [("sona_it".to_string(), "coda".to_string(), 2), ("sona_it".to_string(), "horizon".to_string(), 0)]);
        let (stop, start) = diff(&next, &BTreeMap::new());
        assert_eq!((stop.len(), start.len()), (4, 0));
    }

    #[test]
    fn memory_is_read_from_proc_and_checked_with_a_margin() {
        assert_eq!(rss_mb("Name:\tphp\nVmRSS:\t  204800 kB\n"), Some(200));
        assert!(!over_memory(150, 128));
        assert!(over_memory(170, 128));
    }
}
