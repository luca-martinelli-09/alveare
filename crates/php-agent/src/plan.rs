use std::path::PathBuf;
use std::time::Duration;

use alveare_core::php::{FIRST_SITE_UID, SITES_DIR, TMP_DIR};
use alveare_core::site::Slug;

const WP_TIMEOUT: Duration = Duration::from_secs(600);

/// Un processo da lanciare senza shell, con l'utente del sito.
#[derive(Debug)]
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
    fn cron_runs_due_wordpress_events() {
        let e = plan_cron(&slug("sona_it"), 20001);
        assert_eq!(e.args, ["--path=/srv/sites/sona_it/public", "cron", "event", "run", "--due-now"]);
        assert_eq!(e.uid, 20001);
    }
}
