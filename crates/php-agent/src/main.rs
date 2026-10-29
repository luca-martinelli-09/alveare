//! Agente del container PHP: supervisiona php-fpm ed esegue solo i comandi del protocollo
//! (`alveare_core::agent::Command`), con l'utente del sito. Gira come root.
mod plan;
mod pool;

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command as Proc, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};
use std::{fs, thread};

use alveare_core::agent::{Command, Response, AGENT_SOCKET};

use crate::plan::{plan_cron, plan_wp, site_uid, Exec};

const STAGING_DIR: &str = "/srv/php/staging";
const ACTIVE_DIR: &str = "/srv/php/pools";
const MAX_OUTPUT: usize = 64 * 1024;

static FPM_PID: AtomicI32 = AtomicI32::new(0);
static STOPPING: AtomicBool = AtomicBool::new(false);

extern "C" fn on_term(_: libc::c_int) {
    STOPPING.store(true, Ordering::SeqCst);
    let pid = FPM_PID.load(Ordering::SeqCst);
    if pid > 0 {
        // SIGQUIT = arresto graceful di FPM.
        unsafe { libc::kill(pid, libc::SIGQUIT) };
    }
}

fn main() {
    unsafe {
        libc::signal(libc::SIGTERM, on_term as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    thread::spawn(serve);
    supervise_fpm();
}

/// Tiene in vita php-fpm; esce quando il container viene fermato.
fn supervise_fpm() {
    let mut backoff = Duration::from_secs(1);
    while !STOPPING.load(Ordering::SeqCst) {
        let started = Instant::now();
        let status = Proc::new("php-fpm").arg("-F").spawn().and_then(|mut c| {
            FPM_PID.store(c.id() as i32, Ordering::SeqCst);
            c.wait()
        });
        FPM_PID.store(0, Ordering::SeqCst);
        if STOPPING.load(Ordering::SeqCst) {
            break;
        }
        eprintln!("php-agent: php-fpm terminato ({status:?}), riavvio tra {backoff:?}");
        backoff = if started.elapsed() > Duration::from_secs(60) { Duration::from_secs(1) } else { (backoff * 2).min(Duration::from_secs(30)) };
        thread::sleep(backoff);
    }
}

fn serve() {
    let path = Path::new(AGENT_SOCKET);
    fs::create_dir_all(path.parent().expect("socket in una cartella")).expect("cartella del socket");
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path).expect("bind del socket dell'agente");
    // Solo root (il pannello): gli utenti dei siti girano in questo container.
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("permessi del socket");
    for stream in listener.incoming().flatten() {
        thread::spawn(move || handle(stream));
    }
}

fn handle(stream: UnixStream) {
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let response = match Command::parse(&line) {
        Ok(cmd) => run(cmd),
        Err(e) => Response { ok: false, exit_code: None, output: format!("richiesta rifiutata: {e}") },
    };
    let out = serde_json::to_string(&response).expect("serializzazione") + "\n";
    let _ = (&stream).write_all(out.as_bytes());
}

fn run(cmd: Command) -> Response {
    match cmd {
        Command::PoolApply => apply_pools(),
        Command::FpmReopenLogs => signal_fpm(libc::SIGUSR1),
        Command::Wp { slug, args } => with_site_uid(&slug, |uid| exec(plan_wp(&slug, uid, &args))),
        Command::CronRun { slug } => with_site_uid(&slug, |uid| exec(plan_cron(&slug, uid))),
    }
}

fn fail(output: impl Into<String>) -> Response {
    Response { ok: false, exit_code: None, output: output.into() }
}

fn with_site_uid(slug: &alveare_core::site::Slug, f: impl FnOnce(u32) -> Response) -> Response {
    let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
    match site_uid(&passwd, slug) {
        Some(uid) => f(uid),
        None => fail(format!("sito sconosciuto: {}", slug.as_str())),
    }
}

fn signal_fpm(sig: libc::c_int) -> Response {
    let pid = FPM_PID.load(Ordering::SeqCst);
    if pid <= 0 || unsafe { libc::kill(pid, sig) } != 0 {
        return fail("php-fpm non in esecuzione");
    }
    Response { ok: true, exit_code: None, output: String::new() }
}

fn apply_pools() -> Response {
    let report = match pool::apply_pools(Path::new(STAGING_DIR), Path::new(ACTIVE_DIR), validate_pool) {
        Ok(r) => r,
        Err(e) => return fail(format!("applicazione dei pool: {e}")),
    };
    let reload = signal_fpm(libc::SIGUSR2);
    Response {
        ok: report.rejected.is_empty() && reload.ok,
        exit_code: None,
        output: serde_json::to_string(&report).expect("serializzazione"),
    }
}

/// `php-fpm -t` su una config che contiene solo questo pool.
fn validate_pool(pool: &Path) -> Result<(), String> {
    let conf = pool.with_extension("test");
    fs::write(&conf, format!("[global]\nerror_log = /proc/self/fd/2\ninclude={}\n", pool.display()))
        .map_err(|e| e.to_string())?;
    let out = Proc::new("php-fpm").args(["-t", "-y"]).arg(&conf).output();
    let _ = fs::remove_file(&conf);
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(e) => Err(e.to_string()),
    }
}

fn exec(e: Exec) -> Response {
    let child = Proc::new(&e.program)
        .args(&e.args)
        .current_dir(&e.cwd)
        .env_clear()
        .envs(e.env.iter().cloned())
        .uid(e.uid) // con uid impostato std azzera anche i gruppi supplementari
        .gid(e.gid)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    match child {
        Ok(child) => wait_with_timeout(child, e.timeout),
        Err(err) => fail(format!("avvio di {}: {err}", e.program)),
    }
}

fn wait_with_timeout(mut child: Child, timeout: Duration) -> Response {
    let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
        .into_iter()
        .flatten()
        .map(|mut r| thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            buf
        }))
        .collect();
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(_) => break None,
        }
    };
    let mut output: Vec<u8> = readers.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect();
    output.truncate(MAX_OUTPUT);
    let mut text = String::from_utf8_lossy(&output).into_owned();
    if status.is_none() {
        text.push_str("\n[interrotto: timeout]");
    }
    Response { ok: status.is_some_and(|s| s.success()), exit_code: status.and_then(|s| s.code()), output: text }
}
