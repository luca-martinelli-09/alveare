//! Protocollo pannello ↔ agente: una riga JSON per richiesta, una per risposta.
//! L'enum è la whitelist: un comando che non sta qui non si può nemmeno esprimere.
use serde::{Deserialize, Serialize};

use crate::site::Slug;

pub const AGENT_SOCKET: &str = "/run/alveare/agent.sock";

/// Socket dell'agente di un runtime PHP: quello storico per il runtime di default.
pub fn agent_socket(runtime: &str) -> String {
    if runtime == crate::php::DEFAULT_RUNTIME { AGENT_SOCKET.into() } else { format!("/run/alveare/agent-{runtime}.sock") }
}

// Flag globali di WP-CLI che farebbero uscire il comando dal sito o dall'utente del sito.
const FORBIDDEN_WP_FLAGS: [&str; 6] = ["--path", "--allow-root", "--ssh", "--http", "--require", "--exec"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    PoolApply,
    FpmReopenLogs,
    CronRun { slug: Slug },
    Wp { slug: Slug, args: Vec<String> },
    /// Uno script PHP del sito (anche `artisan`) con la CLI, l'utente del sito e open_basedir.
    Php { slug: Slug, args: Vec<String>, timeout_s: u32 },
    /// Deploy di un'app: git fetch/reset, composer install, migrate e cache di artisan.
    Deploy { slug: Slug, repo: String, branch: String },
    /// Chiave SSH di deploy del sito (creata se manca): restituisce la parte pubblica.
    DeployKey { slug: Slug },
    /// Container php-workers: rilegge /srv/php/workers.json e allinea i processi.
    WorkersApply,
    /// Riavvia i processi di un worker (`name` vuoto: tutti quelli del sito).
    WorkerRestart { slug: Slug, name: String },
    WorkersStatus,
}

/// Socket dell'agente nel container php-workers.
pub const WORKERS_SOCKET: &str = "/run/alveare/workers.sock";

/// Un processo long-running di un sito (code, Horizon, Reverb), definito dal pannello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSpec {
    pub slug: Slug,
    pub uid: u32,
    pub name: String,
    /// Argomenti per php: il primo è lo script (es. artisan).
    pub args: Vec<String>,
    pub processes: u32,
    pub memory_mb: u32,
}

impl WorkerSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() || self.name.len() > 32 || !self.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
            return Err(format!("nome del worker non valido: {}", self.name));
        }
        if !(1..=8).contains(&self.processes) || !(32..=4096).contains(&self.memory_mb) {
            return Err("processi tra 1 e 8, memoria tra 32 e 4096 MB".into());
        }
        if self.uid < crate::php::FIRST_SITE_UID {
            return Err("utente non valido".into());
        }
        check_script(&self.args)
    }
}

/// Repository git: https, ssh o scp-like (git@host:percorso), mai un'opzione.
pub fn valid_repo(repo: &str) -> bool {
    let ok_chars = repo.bytes().all(|b| b.is_ascii_graphic()) && !repo.starts_with('-');
    let scp_like = repo.split_once('@').is_some_and(|(user, rest)| !user.is_empty() && !user.contains('/') && rest.contains(':'));
    ok_chars && repo.len() <= 500 && (repo.starts_with("https://") || repo.starts_with("ssh://") || scp_like)
}

pub fn valid_branch(b: &str) -> bool {
    !b.is_empty() && b.len() <= 200 && !b.starts_with(['-', '/']) && !b.contains("..") && b.bytes().all(|c| c.is_ascii_alphanumeric() || b"._/-".contains(&c))
}

/// Il primo argomento è lo script, relativo alla cartella del sito: niente opzioni di php,
/// percorsi assoluti o `..`.
fn check_script(args: &[String]) -> Result<(), String> {
    let script = args.first().ok_or("manca lo script")?;
    let ok = !script.starts_with('-')
        && !script.starts_with('/')
        && std::path::Path::new(script).components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if ok { Ok(()) } else { Err(format!("script non ammesso: {script}")) }
}

impl Command {
    pub fn parse(line: &str) -> Result<Self, String> {
        let cmd: Self = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if let Self::Php { args, timeout_s, .. } = &cmd {
            check_script(args)?;
            if !(1..=3600).contains(timeout_s) {
                return Err("timeout tra 1 e 3600 secondi".into());
            }
        }
        if let Self::Deploy { repo, branch, .. } = &cmd {
            if !valid_repo(repo) || !valid_branch(branch) {
                return Err("repository o branch non validi".into());
            }
        }
        if let Self::WorkerRestart { name, .. } = &cmd {
            if !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
                return Err("nome del worker non valido".into());
            }
        }
        if let Self::Wp { args, .. } = &cmd {
            let bad = args.iter().find(|a| {
                let name = a.split('=').next().unwrap_or_default().to_ascii_lowercase();
                FORBIDDEN_WP_FLAGS.contains(&name.as_str())
            });
            if let Some(flag) = bad {
                return Err(format!("flag non permesso: {flag}"));
            }
        }
        Ok(cmd)
    }

    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("serializzazione infallibile") + "\n"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wp_request_is_parsed() {
        let c = Command::parse(r#"{"cmd":"wp","slug":"sona_it","args":["plugin","list"]}"#).unwrap();
        assert_eq!(c, Command::Wp { slug: Slug::parse("sona_it").unwrap(), args: vec!["plugin".into(), "list".into()] });
    }

    #[test]
    fn commands_without_arguments_are_parsed() {
        assert_eq!(Command::parse(r#"{"cmd":"pool_apply"}"#).unwrap(), Command::PoolApply);
        assert_eq!(Command::parse(r#"{"cmd":"fpm_reopen_logs"}"#).unwrap(), Command::FpmReopenLogs);
        assert_eq!(
            Command::parse(r#"{"cmd":"cron_run","slug":"sona_it"}"#).unwrap(),
            Command::CronRun { slug: Slug::parse("sona_it").unwrap() }
        );
    }

    #[test]
    fn unknown_commands_are_rejected() {
        assert!(Command::parse(r#"{"cmd":"shell","line":"rm -rf /"}"#).is_err());
        assert!(Command::parse("not json").is_err());
    }

    #[test]
    fn invalid_slug_is_rejected() {
        assert!(Command::parse(r#"{"cmd":"cron_run","slug":"../root"}"#).is_err());
    }

    #[test]
    fn wp_flags_that_escape_the_site_are_rejected() {
        for flag in ["--path=/srv/sites/altro/public", "--allow-root", "--ssh=host", "--http=x", "--require=/tmp/x.php", "--exec=echo 1", "--PATH=/"] {
            let line = format!(r#"{{"cmd":"wp","slug":"sona_it","args":["plugin","list","{flag}"]}}"#);
            assert!(Command::parse(&line).is_err(), "accettato: {flag}");
        }
    }

    #[test]
    fn php_runs_only_a_script_inside_the_site() {
        let ok = Command::parse(r#"{"cmd":"php","slug":"sona_it","args":["artisan","schedule:run"],"timeout_s":600}"#).unwrap();
        assert_eq!(ok, Command::Php { slug: Slug::parse("sona_it").unwrap(), args: vec!["artisan".into(), "schedule:run".into()], timeout_s: 600 });
        for bad in [r#"["/etc/x.php"]"#, r#"["../altro/x.php"]"#, r#"["-d","x=1"]"#, r#"[]"#, r#"["public/../../x.php"]"#] {
            let line = format!(r#"{{"cmd":"php","slug":"sona_it","args":{bad},"timeout_s":60}}"#);
            assert!(Command::parse(&line).is_err(), "accettato: {bad}");
        }
        assert!(Command::parse(r#"{"cmd":"php","slug":"sona_it","args":["x.php"],"timeout_s":0}"#).is_err());
        assert!(Command::parse(r#"{"cmd":"php","slug":"sona_it","args":["x.php"],"timeout_s":7200}"#).is_err());
    }

    #[test]
    fn deploy_accepts_only_git_urls_and_plain_branches() {
        for ok in ["https://github.com/a/b.git", "git@github.com:a/b.git", "ssh://git@host:22/a/b"] {
            assert!(valid_repo(ok), "{ok}");
        }
        for bad in ["--upload-pack=x", "file:///etc", "/srv/sites/altro", "https://a b", "ext::sh -c x"] {
            assert!(!valid_repo(bad), "{bad}");
        }
        assert!(valid_branch("main") && valid_branch("release/1.2"));
        for bad in ["-x", "../x", "a b", "", "/main"] {
            assert!(!valid_branch(bad), "{bad}");
        }
        assert!(Command::parse(r#"{"cmd":"deploy","slug":"sona_it","repo":"--x","branch":"main"}"#).is_err());
    }

    #[test]
    fn worker_specs_are_validated() {
        let spec = WorkerSpec { slug: Slug::parse("sona_it").unwrap(), uid: 20001, name: "horizon".into(), args: vec!["artisan".into(), "horizon".into()], processes: 1, memory_mb: 256 };
        assert!(spec.validate().is_ok());
        for bad in [
            WorkerSpec { name: "Horizon!".into(), ..spec.clone() },
            WorkerSpec { processes: 0, ..spec.clone() },
            WorkerSpec { uid: 0, ..spec.clone() },
            WorkerSpec { args: vec!["/usr/bin/x".into()], ..spec.clone() },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn command_serializes_to_one_line() {
        let c = Command::CronRun { slug: Slug::parse("sona_it").unwrap() };
        let line = c.to_line();
        assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
        assert_eq!(Command::parse(&line).unwrap(), c);
    }
}
