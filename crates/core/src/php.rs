use crate::site::Slug;

/// Percorsi identici in Caddy, PHP e pannello: FPM riceve path che devono esistere ovunque.
pub const SITES_DIR: &str = "/srv/sites";
pub const LOGS_DIR: &str = "/srv/logs";
pub const TMP_DIR: &str = "/srv/tmp";
pub const SOCKET_DIR: &str = "/run/php";
/// Gli UID dei siti partono da qui: sotto ci sono solo utenti di sistema.
pub const FIRST_SITE_UID: u32 = 20000;

const DISABLED_FUNCTIONS: &str = "exec,shell_exec,system,passthru,proc_open,popen";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpSettings {
    pub memory_limit_mb: u32,
    pub upload_max_mb: u32,
    pub max_execution_time_s: u32,
    pub max_children: u32,
}

impl Default for PhpSettings {
    fn default() -> Self {
        Self { memory_limit_mb: 256, upload_max_mb: 64, max_execution_time_s: 60, max_children: 5 }
    }
}

#[derive(Debug, Clone)]
pub struct SitePhp {
    pub slug: Slug,
    pub uid: u32,
    pub settings: PhpSettings,
    /// Direttive extra dell'admin, già validate da `parse_extra`.
    pub extra: Vec<String>,
}

/// Valori che reggono l'isolamento tra siti: nessun override li tocca.
const PROTECTED_INI: [&str; 9] =
    ["open_basedir", "disable_functions", "disable_classes", "error_log", "session.save_path", "upload_tmp_dir", "sys_temp_dir", "user_dir", "doc_root"];

/// Override globale `conf.d/zz-custom.ini`: solo `chiave = valore`, mai i valori che reggono
/// l'isolamento (nemmeno quelli di opcache che separano gli script dei siti).
pub fn parse_global_ini(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let (name, value) = line.split_once('=').ok_or_else(|| format!("riga non valida: {line}"))?;
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.') {
            return Err(format!("nome non valido: {name}"));
        }
        if PROTECTED_INI.contains(&name) || name.starts_with("opcache.validate_") || name == "opcache.restrict_api" {
            return Err(format!("{name} non si può cambiare: protegge l'isolamento tra i siti"));
        }
        if value.len() > 512 || value.contains('\0') {
            return Err(format!("valore non valido per {name}"));
        }
        out.push(format!("{name} = {value}"));
    }
    Ok(out)
}

/// Direttive extra del pool: solo `php_value`, `php_admin_value`, `php_flag`, `php_admin_flag`,
/// mai sui valori protetti. Commenti e righe vuote si scartano.
pub fn parse_extra(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let (head, value) = line.split_once('=').ok_or_else(|| format!("manca '=': {line}"))?;
        let head = head.trim();
        let (kind, name) = head.strip_suffix(']').and_then(|h| h.split_once('[')).ok_or_else(|| format!("direttiva non ammessa: {line}"))?;
        if !["php_value", "php_admin_value", "php_flag", "php_admin_flag"].contains(&kind) {
            return Err(format!("direttiva non ammessa: {line}"));
        }
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.') {
            return Err(format!("nome non valido: {name}"));
        }
        if PROTECTED_INI.contains(&name) {
            return Err(format!("{name} non si può cambiare: protegge l'isolamento tra i siti"));
        }
        let value = value.trim();
        if value.len() > 512 || value.contains(['\0', '\r']) {
            return Err(format!("valore non valido per {name}"));
        }
        out.push(format!("{kind}[{name}] = {value}"));
    }
    Ok(out)
}

pub fn socket_path(slug: &Slug) -> String {
    format!("{SOCKET_DIR}/{}.sock", slug.as_str())
}

pub fn render_pool(site: &SitePhp) -> String {
    let s = site.slug.as_str();
    let PhpSettings { memory_limit_mb, upload_max_mb, max_execution_time_s, max_children } = site.settings;
    let socket = socket_path(&site.slug);
    format!(
        "[{s}]
user = {s}
group = {s}
listen = {socket}
listen.owner = root
listen.group = root
listen.mode = 0660
pm = ondemand
pm.max_children = {max_children}
pm.process_idle_timeout = 10s
pm.max_requests = 500
chdir = {SITES_DIR}/{s}
slowlog = {LOGS_DIR}/{s}/php-slow.log
request_slowlog_timeout = 5s
php_admin_value[open_basedir] = {SITES_DIR}/{s}/:{TMP_DIR}/{s}/
php_admin_value[upload_tmp_dir] = {TMP_DIR}/{s}
php_admin_value[session.save_path] = {TMP_DIR}/{s}
php_admin_value[sys_temp_dir] = {TMP_DIR}/{s}
php_admin_value[disable_functions] = {DISABLED_FUNCTIONS}
php_admin_flag[log_errors] = on
php_admin_value[error_log] = {LOGS_DIR}/{s}/php-error.log
php_admin_value[memory_limit] = {memory_limit_mb}M
php_value[upload_max_filesize] = {upload_max_mb}M
php_value[post_max_size] = {upload_max_mb}M
php_value[max_execution_time] = {max_execution_time_s}
{extra}",
        extra = site.extra.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )
}

// Il file sostituisce /etc/passwd dell'immagine PHP (Debian): gli utenti di sistema vanno ripetuti.
// adminer: utente del pool di Adminer, sotto FIRST_SITE_UID così l'agente non lo tratta come un sito.
pub const ADMINER_UID: u32 = 19999;
const BASE_PASSWD: &str = "root:x:0:0:root:/root:/bin/bash
www-data:x:33:33:www-data:/var/www:/usr/sbin/nologin
adminer:x:19999:19999::/srv/adminer:/usr/sbin/nologin
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
";
const BASE_GROUP: &str = "root:x:0:
www-data:x:33:
adminer:x:19999:
nogroup:x:65534:
";

pub fn render_passwd(sites: &[&SitePhp]) -> String {
    sites.iter().fold(BASE_PASSWD.to_string(), |acc, x| {
        let s = x.slug.as_str();
        acc + &format!("{s}:x:{u}:{u}::{SITES_DIR}/{s}:/usr/sbin/nologin\n", u = x.uid)
    })
}

pub fn render_group(sites: &[&SitePhp]) -> String {
    sites.iter().fold(BASE_GROUP.to_string(), |acc, x| acc + &format!("{}:x:{}:\n", x.slug.as_str(), x.uid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::Slug;

    fn site() -> SitePhp {
        SitePhp { slug: Slug::parse("sona_it").unwrap(), uid: 20001, settings: PhpSettings::default(), extra: vec![] }
    }

    #[test]
    fn global_ini_accepts_plain_settings_only() {
        assert_eq!(parse_global_ini("; nota\nmax_input_vars = 3000\nopcache.memory_consumption=256\n").unwrap(), ["max_input_vars = 3000", "opcache.memory_consumption = 256"]);
        for bad in ["disable_functions =", "open_basedir = /", "opcache.validate_root = 0", "opcache.validate_permission=0", "[sezione]", "senza uguale", "Max_Input = 1"] {
            assert!(parse_global_ini(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn extra_directives_come_last_so_they_win() {
        let mut s = site();
        s.extra = vec!["php_value[max_input_vars] = 5000".into()];
        let p = render_pool(&s);
        assert!(p.ends_with("php_value[max_input_vars] = 5000\n"), "{p}");
    }

    #[test]
    fn only_safe_php_directives_are_accepted() {
        for ok in ["php_value[max_input_vars] = 5000", "php_admin_flag[display_errors] = off", "php_admin_value[opcache.memory_consumption]=256", "  ; commento", ""] {
            assert!(parse_extra(ok).is_ok(), "{ok}");
        }
        for bad in [
            "php_admin_value[open_basedir] = /",
            "php_admin_value[disable_functions] = ",
            "PHP_VALUE[OPEN_BASEDIR] = /",
            "user = root",
            "listen = /run/php/altro.sock",
            "chroot = /",
            "php_value[error_log] = /srv/sites/altro/x",
            "php_value[x] = a\nuser = root",
            "php_value[a b] = 1",
        ] {
            assert!(parse_extra(bad).is_err(), "accettata: {bad:?}");
        }
        assert_eq!(parse_extra("; nota\nphp_value[a] = 1\n").unwrap(), vec!["php_value[a] = 1"]);
    }

    #[test]
    fn pool_runs_as_site_user_on_its_own_socket() {
        let p = render_pool(&site());
        assert!(p.starts_with("[sona_it]\n"));
        assert!(p.contains("\nuser = sona_it\n"));
        assert!(p.contains("\ngroup = sona_it\n"));
        assert!(p.contains("\nlisten = /run/php/sona_it.sock\n"));
    }

    #[test]
    fn pool_socket_is_not_reachable_by_other_site_users() {
        // Un sito che parla FastCGI col socket di un altro eseguirebbe codice come quell'utente.
        let p = render_pool(&site());
        assert!(p.contains("\nlisten.owner = root\n"));
        assert!(p.contains("\nlisten.group = root\n"));
        assert!(p.contains("\nlisten.mode = 0660\n"));
    }

    #[test]
    fn pool_confines_site_to_its_folders() {
        let p = render_pool(&site());
        assert!(p.contains("\nphp_admin_value[open_basedir] = /srv/sites/sona_it/:/srv/tmp/sona_it/\n"));
        assert!(p.contains("\nphp_admin_value[upload_tmp_dir] = /srv/tmp/sona_it\n"));
        assert!(p.contains("\nphp_admin_value[session.save_path] = /srv/tmp/sona_it\n"));
        assert!(p.contains("\nphp_admin_value[sys_temp_dir] = /srv/tmp/sona_it\n"));
        assert!(p.contains("\nphp_admin_value[disable_functions] = exec,shell_exec,system,passthru,proc_open,popen\n"));
    }

    #[test]
    fn pool_writes_logs_in_site_log_folder() {
        let p = render_pool(&site());
        assert!(p.contains("\nphp_admin_value[error_log] = /srv/logs/sona_it/php-error.log\n"));
        assert!(p.contains("\nslowlog = /srv/logs/sona_it/php-slow.log\n"));
        assert!(p.contains("\nrequest_slowlog_timeout = 5s\n"));
    }

    #[test]
    fn pool_uses_site_settings() {
        let mut s = site();
        s.settings = PhpSettings { memory_limit_mb: 512, upload_max_mb: 100, max_execution_time_s: 120, max_children: 8 };
        let p = render_pool(&s);
        assert!(p.contains("\npm = ondemand\n"));
        assert!(p.contains("\npm.max_children = 8\n"));
        assert!(p.contains("\nphp_admin_value[memory_limit] = 512M\n"));
        assert!(p.contains("\nphp_value[upload_max_filesize] = 100M\n"));
        assert!(p.contains("\nphp_value[post_max_size] = 100M\n"));
        assert!(p.contains("\nphp_value[max_execution_time] = 120\n"));
    }

    #[test]
    fn passwd_keeps_system_users_and_adds_sites() {
        let a = site();
        let mut b = site();
        b.slug = Slug::parse("upupa_it").unwrap();
        b.uid = 20002;
        let pw = render_passwd(&[&a, &b]);
        assert!(pw.starts_with("root:x:0:0:"));
        assert!(pw.contains("\nadminer:x:19999:19999::/srv/adminer:/usr/sbin/nologin\n"));
        assert!(pw.contains("\nwww-data:x:33:33:"));
        assert!(pw.contains("\nsona_it:x:20001:20001::/srv/sites/sona_it:/usr/sbin/nologin\n"));
        assert!(pw.ends_with("upupa_it:x:20002:20002::/srv/sites/upupa_it:/usr/sbin/nologin\n"));
        let gr = render_group(&[&a, &b]);
        assert!(gr.starts_with("root:x:0:\n"));
        assert!(gr.contains("\nsona_it:x:20001:\n"));
    }
}
