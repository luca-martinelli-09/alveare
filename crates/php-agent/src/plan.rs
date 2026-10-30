use std::path::PathBuf;
use std::time::Duration;

use alveare_core::php::{FIRST_SITE_UID, LOGS_DIR, SITES_DIR, TMP_DIR};
use alveare_core::site::Slug;

const WP_TIMEOUT: Duration = Duration::from_secs(600);

/// Un processo da lanciare senza shell, con l'utente del sito.
#[derive(Debug, Clone)]
pub struct Exec {
    pub program: String,
    pub args: Vec<String>,
    pub uid: u32,
    pub gid: u32,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
}

/// UID del sito dal passwd generato dal pannello; mai un utente di sistema.
pub fn site_uid(passwd: &str, slug: &Slug) -> Option<u32> {
    passwd.lines().find_map(|l| {
        let mut f = l.split(':');
        (f.next()? == slug.as_str()).then_some(())?;
        let uid: u32 = f.nth(1)?.parse().ok()?;
        (uid >= FIRST_SITE_UID).then_some(uid)
    })
}

pub fn plan_wp(slug: &Slug, uid: u32, args: &[String]) -> Exec {
    let s = slug.as_str();
    Exec {
        program: "wp".into(),
        args: std::iter::once(format!("--path={SITES_DIR}/{s}/public")).chain(args.iter().cloned()).collect(),
        uid,
        gid: uid,
        cwd: PathBuf::from(format!("{SITES_DIR}/{s}")),
        env: vec![
            ("HOME".into(), format!("{TMP_DIR}/{s}")),
            ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
        ],
        timeout: WP_TIMEOUT,
    }
}

/// Script e artisan con la CLI: la CLI non legge il pool, quindi i confini si passano con `-d`.
/// proc_open resta abilitata: lo scheduler di Laravel la usa per lanciare i comandi.
pub fn plan_php(slug: &Slug, uid: u32, args: &[String], timeout_s: u32) -> Exec {
    let s = slug.as_str();
    let confine = [
        "-d".to_string(),
        format!("open_basedir={SITES_DIR}/{s}/:{TMP_DIR}/{s}/"),
        "-d".into(),
        "disable_functions=exec,shell_exec,system,passthru,popen".into(),
        "-d".into(),
        format!("error_log={LOGS_DIR}/{s}/php-error.log"),
    ];
    Exec {
        program: "php".into(),
        args: confine.into_iter().chain(args.iter().cloned()).collect(),
        uid,
        gid: uid,
        cwd: PathBuf::from(format!("{SITES_DIR}/{s}")),
        env: vec![("HOME".into(), format!("{TMP_DIR}/{s}")), ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())],
        timeout: Duration::from_secs(u64::from(timeout_s)),
    }
}

/// Un processo di un worker: come uno script PHP, senza timeout e con il suo memory_limit.
pub fn plan_worker(spec: &alveare_core::agent::WorkerSpec) -> Exec {
    let mut args = vec!["-d".to_string(), format!("memory_limit={}M", spec.memory_mb)];
    args.extend(spec.args.iter().cloned());
    Exec { timeout: Duration::ZERO, ..plan_php(&spec.slug, spec.uid, &args, 0) }
}

/// Cosa c'è nella cartella del sito: decide i passi del deploy.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppLayout {
    pub git: bool,
    pub composer: bool,
    pub artisan: bool,
}

/// Passi del deploy, ognuno con l'utente del sito; il secondo valore dice se è obbligatorio.
pub fn plan_deploy(slug: &Slug, uid: u32, repo: &str, branch: &str, app: AppLayout) -> Vec<(Exec, bool)> {
    let s = slug.as_str();
    let home = format!("{TMP_DIR}/{s}");
    let ssh = format!("ssh -i {home}/.ssh/id_ed25519 -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile={home}/.ssh/known_hosts");
    let base_env = vec![
        ("HOME".to_string(), home.clone()),
        ("PATH".to_string(), "/usr/local/bin:/usr/bin:/bin".to_string()),
        ("GIT_SSH_COMMAND".to_string(), ssh),
        ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
        ("COMPOSER_HOME".to_string(), format!("{home}/.composer")),
    ];
    let cmd = |program: &str, args: &[&str]| Exec {
        program: program.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        uid,
        gid: uid,
        cwd: PathBuf::from(format!("{SITES_DIR}/{s}")),
        env: base_env.clone(),
        timeout: WP_TIMEOUT,
    };
    let mut steps = Vec::new();
    if app.git {
        steps.push((cmd("git", &["remote", "set-url", "origin", repo]), true));
    } else {
        steps.push((cmd("git", &["init", "-q"]), true));
        steps.push((cmd("git", &["remote", "add", "origin", repo]), true));
    }
    steps.push((cmd("git", &["fetch", "--depth", "1", "origin", branch]), true));
    steps.push((cmd("git", &["reset", "--hard", "FETCH_HEAD"]), true));
    if app.composer {
        steps.push((cmd("composer", &["install", "--no-dev", "--no-interaction", "--prefer-dist", "--optimize-autoloader"]), true));
    }
    if app.artisan {
        let artisan = |a: &[&str]| plan_php(slug, uid, &std::iter::once("artisan").chain(a.iter().copied()).map(String::from).collect::<Vec<_>>(), 600);
        steps.push((artisan(&["migrate", "--force"]), true));
        for cache in ["config:cache", "route:cache", "view:cache"] {
            steps.push((artisan(&[cache]), true));
        }
        // Horizon e queue:work rileggono il codice nuovo: se non ci sono, non è un errore.
        steps.push((artisan(&["horizon:terminate"]), false));
        steps.push((artisan(&["queue:restart"]), false));
    }
    steps
}

/// Crea la chiave di deploy del sito se manca (nella HOME del sito, fuori dalla document root).
pub fn plan_deploy_key(slug: &Slug, uid: u32) -> Exec {
    let s = slug.as_str();
    let key = format!("{TMP_DIR}/{s}/.ssh/id_ed25519");
    Exec {
        program: "ssh-keygen".into(),
        args: ["-q", "-t", "ed25519", "-N", "", "-C", &format!("alveare-{s}"), "-f", &key].map(String::from).to_vec(),
        uid,
        gid: uid,
        cwd: PathBuf::from(format!("{TMP_DIR}/{s}")),
        env: vec![("HOME".into(), format!("{TMP_DIR}/{s}"))],
        timeout: Duration::from_secs(30),
    }
}

pub fn plan_cron(slug: &Slug, uid: u32) -> Exec {
    let args = ["cron", "event", "run", "--due-now"].map(String::from);
    plan_wp(slug, uid, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\nsona_it:x:20001:20001::/srv/sites/sona_it:/usr/sbin/nologin\nevil:x:0:0::/srv/sites/evil:/bin/sh\n";

    fn slug(s: &str) -> Slug {
        Slug::parse(s).unwrap()
    }

    #[test]
    fn uid_is_read_from_generated_passwd() {
        assert_eq!(site_uid(PASSWD, &slug("sona_it")), Some(20001));
        assert_eq!(site_uid(PASSWD, &slug("altro_it")), None);
    }

    #[test]
    fn system_uids_are_never_used_for_sites() {
        assert_eq!(site_uid(PASSWD, &slug("evil")), None);
        assert_eq!(site_uid(PASSWD, &slug("root")), None);
    }

    #[test]
    fn wp_runs_as_site_user_inside_site_root() {
        let e = plan_wp(&slug("sona_it"), 20001, &["plugin".into(), "list".into()]);
        assert_eq!(e.program, "wp");
        assert_eq!(e.args, ["--path=/srv/sites/sona_it/public", "plugin", "list"]);
        assert_eq!((e.uid, e.gid), (20001, 20001));
        assert_eq!(e.cwd, PathBuf::from("/srv/sites/sona_it"));
        assert!(e.env.contains(&("HOME".into(), "/srv/tmp/sona_it".into())));
    }

    #[test]
    fn php_scripts_run_confined_to_the_site() {
        let e = plan_php(&slug("sona_it"), 20001, &["artisan".into(), "schedule:run".into()], 120);
        assert_eq!(e.program, "php");
        assert_eq!(
            e.args,
            [
                "-d", "open_basedir=/srv/sites/sona_it/:/srv/tmp/sona_it/",
                "-d", "disable_functions=exec,shell_exec,system,passthru,popen",
                "-d", "error_log=/srv/logs/sona_it/php-error.log",
                "artisan", "schedule:run",
            ]
        );
        assert_eq!((e.uid, e.cwd.clone(), e.timeout), (20001, PathBuf::from("/srv/sites/sona_it"), Duration::from_secs(120)));
    }

    #[test]
    fn worker_runs_php_confined_with_its_memory_limit() {
        let spec = alveare_core::agent::WorkerSpec { slug: slug("sona_it"), uid: 20001, name: "coda".into(), args: vec!["artisan".into(), "queue:work".into()], processes: 2, memory_mb: 192 };
        let e = plan_worker(&spec);
        assert_eq!(e.program, "php");
        assert!(e.args.windows(2).any(|w| w == ["-d", "memory_limit=192M"]));
        assert!(e.args.iter().any(|a| a.starts_with("open_basedir=/srv/sites/sona_it/")));
        assert_eq!(e.args[e.args.len() - 2..], ["artisan", "queue:work"]);
        assert_eq!(e.uid, 20001);
    }

    #[test]
    fn deploy_steps_follow_what_the_app_has() {
        let all = AppLayout { git: false, composer: true, artisan: true };
        let steps = plan_deploy(&slug("sona_it"), 20001, "git@github.com:a/b.git", "main", all);
        let names: Vec<String> = steps.iter().map(|(e, _)| format!("{} {}", e.program, e.args.iter().filter(|a| !a.starts_with("open_basedir") && !a.starts_with("disable_functions") && !a.starts_with("error_log") && *a != "-d").cloned().collect::<Vec<_>>().join(" "))).collect();
        assert_eq!(names[..4], ["git init -q", "git remote add origin git@github.com:a/b.git", "git fetch --depth 1 origin main", "git reset --hard FETCH_HEAD"]);
        assert!(names.contains(&"composer install --no-dev --no-interaction --prefer-dist --optimize-autoloader".to_string()));
        assert!(names.contains(&"php artisan migrate --force".to_string()));
        assert!(!steps.iter().find(|(e, _)| e.args.contains(&"horizon:terminate".to_string())).unwrap().1, "facoltativo");
        assert!(steps.iter().all(|(e, _)| e.uid == 20001));
        let git_env = &steps[0].0.env;
        assert!(git_env.iter().any(|(k, v)| k == "GIT_SSH_COMMAND" && v.contains("/srv/tmp/sona_it/.ssh/id_ed25519")));
        let plain = plan_deploy(&slug("sona_it"), 20001, "https://x/y.git", "main", AppLayout { git: true, ..Default::default() });
        assert_eq!(plain.len(), 3, "solo git: set-url, fetch, reset");
    }

    #[test]
    fn cron_runs_due_wordpress_events() {
        let e = plan_cron(&slug("sona_it"), 20001);
        assert_eq!(e.args, ["--path=/srv/sites/sona_it/public", "cron", "event", "run", "--due-now"]);
        assert_eq!(e.uid, 20001);
    }
}
