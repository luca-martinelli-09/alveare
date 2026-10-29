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

pub struct SitePhp {
    pub slug: Slug,
    pub uid: u32,
    pub settings: PhpSettings,
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
"
    )
}

// Il file sostituisce /etc/passwd dell'immagine PHP (Debian): gli utenti di sistema vanno ripetuti.
const BASE_PASSWD: &str = "root:x:0:0:root:/root:/bin/bash
www-data:x:33:33:www-data:/var/www:/usr/sbin/nologin
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
";
const BASE_GROUP: &str = "root:x:0:
www-data:x:33:
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
        SitePhp { slug: Slug::parse("sona_it").unwrap(), uid: 20001, settings: PhpSettings::default() }
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
        assert!(pw.contains("\nwww-data:x:33:33:"));
        assert!(pw.contains("\nsona_it:x:20001:20001::/srv/sites/sona_it:/usr/sbin/nologin\n"));
        assert!(pw.ends_with("upupa_it:x:20002:20002::/srv/sites/upupa_it:/usr/sbin/nologin\n"));
        let gr = render_group(&[&a, &b]);
        assert!(gr.starts_with("root:x:0:\n"));
        assert!(gr.contains("\nsona_it:x:20001:\n"));
    }
}
