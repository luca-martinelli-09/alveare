//! Cron personalizzati: espressione cron (ora di Roma) e un comando WP-CLI, artisan, uno script
//! PHP del sito o una chiamata HTTP a una pagina del sito stesso.
use alveare_core::site::Domain;
use chrono::Timelike;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CronKind {
    Wp,
    Artisan,
    Php,
    Http,
}

impl CronKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wp => "wp",
            Self::Artisan => "artisan",
            Self::Php => "php",
            Self::Http => "http",
        }
    }
}

impl FromStr for CronKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "wp" => Ok(Self::Wp),
            "artisan" => Ok(Self::Artisan),
            "php" => Ok(Self::Php),
            "http" => Ok(Self::Http),
            other => Err(format!("tipo di cron sconosciuto: {other}")),
        }
    }
}

/// Cosa eseguire, già controllato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Wp(Vec<String>),
    /// Argomenti per l'agente: il primo è lo script.
    Php(Vec<String>),
    /// GET a una pagina del sito, attraverso Caddy.
    Http { host: Domain, path: String },
}

/// Divide una riga di comando come la shell, con apici singoli e doppi (senza espansioni).
pub fn split_args(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("apice non chiuso".into());
    }
    if started {
        out.push(cur);
    }
    Ok(out)
}

/// Il comando di un cron, per tipo; l'URL di un cron HTTP deve essere su un dominio del sito.
pub fn prepare(kind: CronKind, command: &str, domains: &[Domain]) -> Result<Action, String> {
    let command = command.trim();
    if command.is_empty() || command.len() > 2000 || command.contains(['\n', '\r', '\0']) {
        return Err("comando vuoto o non valido".into());
    }
    match kind {
        CronKind::Wp => {
            let args = split_args(command.strip_prefix("wp ").unwrap_or(command))?;
            Ok(Action::Wp(args))
        }
        CronKind::Artisan => {
            let args = split_args(command.strip_prefix("php artisan ").or_else(|| command.strip_prefix("artisan ")).unwrap_or(command))?;
            Ok(Action::Php(std::iter::once("artisan".to_string()).chain(args).collect()))
        }
        CronKind::Php => {
            let args = split_args(command.strip_prefix("php ").unwrap_or(command))?;
            let script = args.first().ok_or("manca lo script")?;
            let ok = !script.starts_with(['-', '/']) && std::path::Path::new(script).components().all(|c| matches!(c, std::path::Component::Normal(_)));
            if !ok {
                return Err(format!("lo script deve essere un percorso dentro il sito: {script}"));
            }
            Ok(Action::Php(args))
        }
        CronKind::Http => {
            let rest = command.strip_prefix("https://").or_else(|| command.strip_prefix("http://")).ok_or("l'URL deve iniziare con https://")?;
            let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
            let host = Domain::parse(host).map_err(|_| format!("host non valido: {host}"))?;
            if !domains.contains(&host) {
                return Err(format!("{} non è un dominio di questo sito", host.as_str()));
            }
            if path.contains(char::is_whitespace) {
                return Err("percorso non valido".into());
            }
            Ok(Action::Http { host, path })
        }
    }
}

pub fn parse_expression(expr: &str) -> Result<croner::Cron, String> {
    croner::Cron::from_str(expr.trim()).map_err(|e| format!("espressione cron non valida: {e}"))
}

pub fn is_due(expr: &str, now: &chrono::DateTime<chrono_tz::Tz>) -> bool {
    // Le espressioni sono al minuto: il tick può svegliarsi qualche istante dopo lo :00.
    let minute = now.with_second(0).and_then(|t| t.with_nanosecond(0));
    minute.is_some_and(|t| parse_expression(expr).ok().and_then(|c| c.is_time_matching(&t).ok()).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn d(s: &str) -> Domain {
        Domain::parse(s).unwrap()
    }

    #[test]
    fn arguments_are_split_like_a_shell() {
        assert_eq!(split_args("option update blogname 'Pro Loco Sona'").unwrap(), ["option", "update", "blogname", "Pro Loco Sona"]);
        assert_eq!(split_args(r#"eval "echo 1;"  x"#).unwrap(), ["eval", "echo 1;", "x"]);
        assert_eq!(split_args("a ''").unwrap(), ["a", ""]);
        assert!(split_args("a 'b").is_err());
    }

    #[test]
    fn commands_are_prepared_per_kind() {
        let dom = [d("sona.it"), d("www.sona.it")];
        assert_eq!(prepare(CronKind::Wp, "wp cache flush", &dom).unwrap(), Action::Wp(vec!["cache".into(), "flush".into()]));
        assert_eq!(prepare(CronKind::Artisan, "php artisan queue:prune", &dom).unwrap(), Action::Php(vec!["artisan".into(), "queue:prune".into()]));
        assert_eq!(prepare(CronKind::Php, "public/cron.php --x", &dom).unwrap(), Action::Php(vec!["public/cron.php".into(), "--x".into()]));
        assert_eq!(prepare(CronKind::Http, "https://www.sona.it/wp-cron.php?x=1", &dom).unwrap(), Action::Http { host: d("www.sona.it"), path: "/wp-cron.php?x=1".into() });
        for (kind, bad) in [(CronKind::Php, "/etc/x.php"), (CronKind::Php, "../x.php"), (CronKind::Php, "-r 'echo 1;'"), (CronKind::Http, "https://altro.it/"), (CronKind::Http, "ftp://sona.it"), (CronKind::Wp, "")] {
            assert!(prepare(kind, bad, &dom).is_err(), "{bad}");
        }
    }

    #[test]
    fn due_in_rome_time() {
        let at = |h, m| chrono_tz::Europe::Rome.with_ymd_and_hms(2026, 11, 2, h, m, 0).unwrap();
        assert!(is_due("*/15 * * * *", &at(10, 30)));
        assert!(!is_due("*/15 * * * *", &at(10, 31)));
        assert!(is_due("0 3 * * 1", &at(3, 0)), "il 2 novembre 2026 è lunedì");
        assert!(!is_due("non valida", &at(3, 0)));
        let late = at(10, 30) + chrono::Duration::milliseconds(1500);
        assert!(is_due("*/15 * * * *", &late), "il tick può svegliarsi un secondo dopo");
        assert!(parse_expression("61 * * * *").is_err());
    }
}
