//! ACL di Redis: un utente per sito, limitato alle chiavi e ai canali con prefisso `<slug>:`.
//! SCAN resta permesso (serve al flush selettivo di WordPress) e mostra i nomi delle chiavi
//! degli altri siti, non i valori: accettato.
use sha2::{Digest, Sha256};

use crate::site::Slug;

pub const ADMIN_USER: &str = "alveare";

pub struct RedisUser {
    pub slug: Slug,
    pub password_sha256: String,
}

pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

// Comandi che leggono o cancellano chiavi di tutti, o toccano il server.
const DENIED: &str = "-@admin -keys -flushdb -flushall -randomkey -swapdb -move -migrate -debug -monitor";

pub fn render_acl(admin_sha256: &str, users: &[RedisUser]) -> String {
    let mut out = format!("user default off\nuser {ADMIN_USER} on #{admin_sha256} ~* &* +@all\n");
    for u in users {
        let s = u.slug.as_str();
        out.push_str(&format!("user {s} on #{} resetkeys ~{s}:* resetchannels &{s}:* +@all {DENIED}\n", u.password_sha256));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::Slug;

    #[test]
    fn default_user_is_disabled_and_admin_is_first() {
        let acl = render_acl("adminhash", &[]);
        let lines: Vec<&str> = acl.lines().collect();
        assert_eq!(lines[0], "user default off");
        assert_eq!(lines[1], "user alveare on #adminhash ~* &* +@all");
    }

    #[test]
    fn site_user_is_confined_to_its_prefix() {
        let s = RedisUser { slug: Slug::parse("sona_it").unwrap(), password_sha256: "abc".into() };
        let acl = render_acl("h", &[s]);
        let line = acl.lines().find(|l| l.starts_with("user sona_it ")).unwrap();
        assert!(line.starts_with("user sona_it on #abc resetkeys ~sona_it:* resetchannels &sona_it:* "));
        for denied in ["-@admin", "-keys", "-flushdb", "-flushall", "-randomkey", "-swapdb"] {
            assert!(line.split(' ').any(|w| w == denied), "manca {denied}");
        }
    }

    #[test]
    fn password_hash_is_sha256_hex() {
        assert_eq!(sha256_hex("password"), "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8");
    }
}
