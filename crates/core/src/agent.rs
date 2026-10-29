//! Protocollo pannello ↔ agente: una riga JSON per richiesta, una per risposta.
//! L'enum è la whitelist: un comando che non sta qui non si può nemmeno esprimere.
use serde::{Deserialize, Serialize};

use crate::site::Slug;

pub const AGENT_SOCKET: &str = "/run/alveare/agent.sock";

// Flag globali di WP-CLI che farebbero uscire il comando dal sito o dall'utente del sito.
const FORBIDDEN_WP_FLAGS: [&str; 6] = ["--path", "--allow-root", "--ssh", "--http", "--require", "--exec"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    PoolApply,
    FpmReopenLogs,
    CronRun { slug: Slug },
    Wp { slug: Slug, args: Vec<String> },
}

impl Command {
    pub fn parse(line: &str) -> Result<Self, String> {
        let cmd: Self = serde_json::from_str(line).map_err(|e| e.to_string())?;
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
    fn command_serializes_to_one_line() {
        let c = Command::CronRun { slug: Slug::parse("sona_it").unwrap() };
        let line = c.to_line();
        assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
        assert_eq!(Command::parse(&line).unwrap(), c);
    }
}
