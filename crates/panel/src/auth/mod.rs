//! Utenti, sessioni, TOTP, token API e permessi per sito.
//! Il tempo arriva come parametro (`now`, secondi Unix) così le scadenze sono testabili.
pub mod invites;
pub mod sessions;

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use rand::distr::{Alphanumeric, SampleString};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use totp_rs::{Builder, Secret, Totp};

use crate::secrets::{random_password, SecretKey};

pub const IDLE_SECS: i64 = 12 * 3600;
pub const LOCK_SECS: i64 = 15 * 60;
const MAX_FAILURES: i64 = 5;
const MIN_PASSWORD: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("credenziali non valide")]
    Invalid,
    #[error("troppi tentativi falliti: riprova più tardi")]
    Locked,
    #[error("la password deve avere almeno 12 caratteri")]
    WeakPassword,
    #[error("passaggio non consentito in questo stato della sessione")]
    WrongStage,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("{0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, AuthError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Password temporanea: va cambiata e va attivato il TOTP.
    Setup,
    /// Password giusta, manca il codice TOTP.
    Totp,
    Full,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::Setup => "setup",
            Stage::Totp => "totp",
            Stage::Full => "full",
        }
    }
    fn parse(s: &str) -> Result<Self> {
        match s {
            "setup" => Ok(Stage::Setup),
            "totp" => Ok(Stage::Totp),
            "full" => Ok(Stage::Full),
            _ => Err(AuthError::Internal(format!("stadio di sessione sconosciuto: {s}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Manager,
    Readonly,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Manager => "manager",
            Role::Readonly => "readonly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Read,
    Manage,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub user: User,
    pub stage: Stage,
}

#[derive(Debug, serde::Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub site_id: Option<i64>,
    pub details: String,
    pub at: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TotpSetup {
    pub secret: String,
    pub otpauth_url: String,
}

pub fn totp(secret_b32: &str, account: &str) -> Result<Totp> {
    let secret = Secret::try_from_base32(secret_b32).map_err(|e| AuthError::Internal(format!("segreto TOTP: {e:?}")))?;
    Builder::new()
        .with_secret(secret)
        .with_account_name(account)
        .with_issuer(Some("Alveare"))
        .build()
        .map_err(|e| AuthError::Internal(format!("TOTP: {e:?}")))
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn hash_password(pw: &str) -> Result<String> {
    Argon2::default()
        .hash_password(pw.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Internal(format!("argon2: {e}")))
}

fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
}

fn user_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<User> {
    Ok(User { id: r.try_get("id")?, email: r.try_get("email")?, name: r.try_get("name")?, is_admin: r.try_get("is_admin")? })
}

#[derive(Clone)]
pub struct Auth {
    pub(crate) pool: SqlitePool,
    key: SecretKey,
}

impl Auth {
    pub fn new(pool: SqlitePool, key: SecretKey) -> Self {
        Self { pool, key }
    }

    /// Restituisce l'id e la password temporanea, da cambiare al primo accesso.
    pub async fn create_user(&self, email: &str, name: &str, is_admin: bool) -> Result<(i64, String)> {
        let temp = random_password();
        let id = sqlx::query_scalar("INSERT INTO users (email, name, is_admin, password_hash) VALUES (?, ?, ?, ?) RETURNING id")
            .bind(email.trim())
            .bind(name)
            .bind(is_admin)
            .bind(hash_password(&temp)?)
            .fetch_one(&self.pool)
            .await?;
        Ok((id, temp))
    }

    pub async fn user(&self, id: i64) -> Result<User> {
        let r = sqlx::query("SELECT id, email, name, is_admin FROM users WHERE id = ?").bind(id).fetch_one(&self.pool).await?;
        user_from_row(&r)
    }

    pub async fn users(&self) -> Result<Vec<User>> {
        sqlx::query("SELECT id, email, name, is_admin FROM users ORDER BY email")
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(user_from_row)
            .collect()
    }

    pub async fn user_count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&self.pool).await?)
    }

    pub async fn set_blocked(&self, id: i64, blocked: bool) -> Result<()> {
        sqlx::query("UPDATE users SET status = ? WHERE id = ?")
            .bind(if blocked { "blocked" } else { "active" })
            .bind(id)
            .execute(&self.pool)
            .await?;
        if blocked {
            sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&self.pool).await?;
        }
        Ok(())
    }

    async fn locked(&self, key: &str, now: i64) -> Result<bool> {
        let until: Option<i64> = sqlx::query_scalar("SELECT locked_until FROM login_failures WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(until.is_some_and(|u| u > now))
    }

    async fn record_failure(&self, keys: &[String], now: i64) -> Result<()> {
        for key in keys {
            sqlx::query(
                "INSERT INTO login_failures (key, count) VALUES (?, 1)
                 ON CONFLICT(key) DO UPDATE SET count = CASE WHEN locked_until > 0 AND locked_until <= ?2 THEN 1 ELSE count + 1 END,
                                                 locked_until = CASE WHEN locked_until > 0 AND locked_until <= ?2 THEN 0 ELSE locked_until END",
            )
            .bind(key)
            .bind(now)
            .execute(&self.pool)
            .await?;
            sqlx::query("UPDATE login_failures SET locked_until = ? WHERE key = ? AND count >= ? AND locked_until <= ?")
                .bind(now + LOCK_SECS)
                .bind(key)
                .bind(MAX_FAILURES)
                .bind(now)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn clear_failures(&self, keys: &[String]) -> Result<()> {
        for key in keys {
            sqlx::query("DELETE FROM login_failures WHERE key = ?").bind(key).execute(&self.pool).await?;
        }
        Ok(())
    }

    fn failure_keys(email: &str, ip: &str) -> Vec<String> {
        vec![format!("user:{}", email.trim().to_lowercase()), format!("ip:{ip}")]
    }

    async fn check_not_locked(&self, keys: &[String], now: i64) -> Result<()> {
        for k in keys {
            if self.locked(k, now).await? {
                return Err(AuthError::Locked);
            }
        }
        Ok(())
    }

    /// Restituisce il token del cookie (in chiaro, mai salvato) e lo stadio della sessione.
    pub async fn login(&self, email: &str, password: &str, ip: &str, ua: &str, now: i64) -> Result<(String, Stage)> {
        let keys = Self::failure_keys(email, ip);
        self.check_not_locked(&keys, now).await?;
        let row = sqlx::query("SELECT id, password_hash, must_change_password, totp_secret_enc IS NOT NULL AS has_totp, status FROM users WHERE email = ?")
            .bind(email.trim())
            .fetch_optional(&self.pool)
            .await?;
        let ok = row.as_ref().is_some_and(|r| {
            r.get::<&str, _>("status") != "blocked" && verify_password(password, r.get("password_hash"))
        });
        let Some(r) = row.filter(|_| ok) else {
            self.record_failure(&keys, now).await?;
            return Err(AuthError::Invalid);
        };
        self.clear_failures(&keys).await?;
        let stage = if r.get::<bool, _>("must_change_password") || !r.get::<bool, _>("has_totp") { Stage::Setup } else { Stage::Totp };
        let token = Alphanumeric.sample_string(&mut rand::rng(), 48);
        sqlx::query("INSERT INTO sessions (id, user_id, stage, created_at, last_used, ip, user_agent) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(sha256_hex(&token))
            .bind(r.get::<i64, _>("id"))
            .bind(stage.as_str())
            .bind(now)
            .bind(now)
            .bind(ip)
            .bind(ua)
            .execute(&self.pool)
            .await?;
        Ok((token, stage))
    }

    /// Sessione valida e non scaduta; l'uso ne rinnova la scadenza.
    pub async fn session(&self, token: &str, now: i64) -> Option<Session> {
        let id = sha256_hex(token);
        let r = sqlx::query(
            "SELECT u.id, u.email, u.name, u.is_admin, s.stage FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.id = ? AND s.last_used > ? AND u.status != 'blocked'",
        )
        .bind(&id)
        .bind(now - IDLE_SECS)
        .fetch_optional(&self.pool)
        .await
        .ok()??;
        sqlx::query("UPDATE sessions SET last_used = ? WHERE id = ?").bind(now).bind(&id).execute(&self.pool).await.ok()?;
        Some(Session { user: user_from_row(&r).ok()?, stage: Stage::parse(r.get("stage")).ok()? })
    }

    async fn set_stage(&self, token: &str, stage: Stage) -> Result<()> {
        sqlx::query("UPDATE sessions SET stage = ? WHERE id = ?").bind(stage.as_str()).bind(sha256_hex(token)).execute(&self.pool).await?;
        Ok(())
    }

    async fn session_in(&self, token: &str, stage: Stage, now: i64) -> Result<Session> {
        self.session(token, now).await.filter(|s| s.stage == stage).ok_or(AuthError::WrongStage)
    }

    /// Primo accesso: nuova password e segreto TOTP da confermare.
    pub async fn setup_password(&self, token: &str, new_password: &str, now: i64) -> Result<TotpSetup> {
        let s = self.session_in(token, Stage::Setup, now).await?;
        if new_password.chars().count() < MIN_PASSWORD {
            return Err(AuthError::WeakPassword);
        }
        let secret = Secret::generate().to_base32();
        sqlx::query("UPDATE users SET password_hash = ?, totp_pending_enc = ? WHERE id = ?")
            .bind(hash_password(new_password)?)
            .bind(self.key.encrypt(&secret))
            .bind(s.user.id)
            .execute(&self.pool)
            .await?;
        let otpauth_url = totp(&secret, &s.user.email)?.to_url().map_err(|e| AuthError::Internal(format!("{e:?}")))?;
        Ok(TotpSetup { secret, otpauth_url })
    }

    pub async fn confirm_totp(&self, token: &str, code: &str, ip: &str, now: i64) -> Result<()> {
        let s = self.session_in(token, Stage::Setup, now).await?;
        let pending: Option<Vec<u8>> = sqlx::query_scalar("SELECT totp_pending_enc FROM users WHERE id = ?").bind(s.user.id).fetch_one(&self.pool).await?;
        let secret = self.key.decrypt(&pending.ok_or(AuthError::WrongStage)?).map_err(|e| AuthError::Internal(e.to_string()))?;
        self.check_code(&s.user, &secret, code, ip, now).await?;
        sqlx::query("UPDATE users SET totp_secret_enc = totp_pending_enc, totp_pending_enc = NULL, must_change_password = 0 WHERE id = ?")
            .bind(s.user.id)
            .execute(&self.pool)
            .await?;
        self.set_stage(token, Stage::Full).await
    }

    pub async fn verify_totp(&self, token: &str, code: &str, ip: &str, now: i64) -> Result<()> {
        let s = self.session_in(token, Stage::Totp, now).await?;
        let enc: Vec<u8> = sqlx::query_scalar("SELECT totp_secret_enc FROM users WHERE id = ?").bind(s.user.id).fetch_one(&self.pool).await?;
        let secret = self.key.decrypt(&enc).map_err(|e| AuthError::Internal(e.to_string()))?;
        self.check_code(&s.user, &secret, code, ip, now).await?;
        self.set_stage(token, Stage::Full).await
    }

    async fn check_code(&self, user: &User, secret: &str, code: &str, ip: &str, now: i64) -> Result<()> {
        let keys = Self::failure_keys(&user.email, ip);
        self.check_not_locked(&keys, now).await?;
        let t = totp(secret, &user.email)?;
        // Finestra di ±30 s; il passo trovato deve essere successivo all'ultimo usato.
        let step = [now - 30, now, now + 30]
            .into_iter()
            .find(|ts| t.generate(*ts as u64).to_string() == code.trim())
            .map(|ts| ts.div_euclid(30));
        let fresh = match step {
            Some(step) => {
                sqlx::query("UPDATE users SET totp_last_step = ? WHERE id = ? AND totp_last_step < ?")
                    .bind(step)
                    .bind(user.id)
                    .bind(step)
                    .execute(&self.pool)
                    .await?
                    .rows_affected()
                    == 1
            }
            None => false,
        };
        if !fresh {
            self.record_failure(&keys, now).await?;
            return Err(AuthError::Invalid);
        }
        self.clear_failures(&keys).await
    }

    pub async fn purge_sessions(&self, now: i64) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE last_used <= ?").bind(now - IDLE_SECS).execute(&self.pool).await?;
        sqlx::query("DELETE FROM login_failures WHERE locked_until < ? AND locked_until > 0").bind(now).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn logout(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?").bind(sha256_hex(token)).execute(&self.pool).await?;
        Ok(())
    }

    /// Il token in chiaro si vede solo qui.
    pub async fn create_api_token(&self, user_id: i64, name: &str, expires_at: i64) -> Result<String> {
        let token = format!("alv_{}", Alphanumeric.sample_string(&mut rand::rng(), 40));
        sqlx::query("INSERT INTO api_tokens (user_id, name, token_hash, expires_at) VALUES (?, ?, ?, ?)")
            .bind(user_id)
            .bind(name)
            .bind(sha256_hex(&token))
            .bind(expires_at)
            .execute(&self.pool)
            .await?;
        Ok(token)
    }

    pub async fn token_user(&self, token: &str, now: i64) -> Option<User> {
        let hash = sha256_hex(token);
        let r = sqlx::query(
            "SELECT u.id, u.email, u.name, u.is_admin FROM api_tokens t JOIN users u ON u.id = t.user_id
             WHERE t.token_hash = ? AND t.expires_at > ? AND u.status != 'blocked'",
        )
        .bind(&hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .ok()??;
        let _ = sqlx::query("UPDATE api_tokens SET last_used = ? WHERE token_hash = ?").bind(now).bind(&hash).execute(&self.pool).await;
        user_from_row(&r).ok()
    }

    pub async fn assign_site(&self, user_id: i64, site_id: i64, role: Role) -> Result<()> {
        sqlx::query("INSERT INTO user_sites (user_id, site_id, role) VALUES (?, ?, ?) ON CONFLICT(user_id, site_id) DO UPDATE SET role = excluded.role")
            .bind(user_id)
            .bind(site_id)
            .bind(role.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn user_sites(&self, user_id: i64) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query("SELECT site_id, role FROM user_sites WHERE user_id = ? ORDER BY site_id").bind(user_id).fetch_all(&self.pool).await?;
        rows.iter().map(|r| Ok((r.try_get("site_id")?, r.try_get("role")?))).collect()
    }

    pub async fn unassign_site(&self, user_id: i64, site_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM user_sites WHERE user_id = ? AND site_id = ?").bind(user_id).bind(site_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn can(&self, user: &User, site_id: i64, need: Need) -> Result<bool> {
        if user.is_admin {
            return Ok(true);
        }
        let role: Option<String> = sqlx::query_scalar("SELECT role FROM user_sites WHERE user_id = ? AND site_id = ?")
            .bind(user.id)
            .bind(site_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match (role.as_deref(), need) {
            (Some("manager"), _) | (Some("readonly"), Need::Read) => true,
            _ => false,
        })
    }

    /// `None` = tutti i siti (admin).
    pub async fn visible_sites(&self, user: &User) -> Result<Option<Vec<i64>>> {
        if user.is_admin {
            return Ok(None);
        }
        let ids = sqlx::query_scalar("SELECT site_id FROM user_sites WHERE user_id = ? ORDER BY site_id").bind(user.id).fetch_all(&self.pool).await?;
        Ok(Some(ids))
    }

    pub async fn audit_for_site(&self, site_id: i64, limit: i64) -> Result<Vec<AuditEntry>> {
        let rows = sqlx::query("SELECT id, user_id, action, site_id, details, at FROM audit_log WHERE site_id = ? ORDER BY id DESC LIMIT ?")
            .bind(site_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(AuditEntry {
                    id: r.try_get("id")?,
                    user_id: r.try_get("user_id")?,
                    action: r.try_get("action")?,
                    site_id: r.try_get("site_id")?,
                    details: r.try_get("details")?,
                    at: r.try_get("at")?,
                })
            })
            .collect()
    }

    pub async fn audit(&self, user_id: Option<i64>, action: &str, site_id: Option<i64>, details: &str) -> Result<()> {
        sqlx::query("INSERT INTO audit_log (user_id, action, site_id, details) VALUES (?, ?, ?, ?)")
            .bind(user_id)
            .bind(action)
            .bind(site_id)
            .bind(details)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewSite, SiteKind, Store};
    use alveare_core::site::Domain;

    const T0: i64 = 1_800_000_000;

    async fn auth() -> Auth {
        let store = Store::open_memory().await.unwrap();
        Auth::new(store.pool().clone(), SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap())
    }

    fn code(secret: &str, now: i64) -> String {
        totp(secret, "x").unwrap().generate(now as u64).to_string()
    }

    /// Utente pronto: password cambiata e TOTP attivo. Restituisce (id, segreto TOTP).
    async fn active_user(a: &Auth, email: &str, admin: bool) -> (i64, String) {
        let (id, temp) = a.create_user(email, "Prova", admin).await.unwrap();
        let (tok, stage) = a.login(email, &temp, "1.1.1.1", "ua", T0).await.unwrap();
        assert_eq!(stage, Stage::Setup);
        let secret = a.setup_password(&tok, "una-password-lunga", T0).await.unwrap().secret;
        a.confirm_totp(&tok, &code(&secret, T0), "1.1.1.1", T0).await.unwrap();
        (id, secret)
    }

    #[tokio::test]
    async fn first_login_requires_password_change_and_totp() {
        let a = auth().await;
        let (_, secret) = active_user(&a, "luca@example.org", true).await;
        let (tok, stage) = a.login("LUCA@example.org", "una-password-lunga", "1.1.1.1", "ua", T0 + 40).await.unwrap();
        assert_eq!(stage, Stage::Totp);
        assert_eq!(a.session(&tok, T0 + 40).await.unwrap().stage, Stage::Totp);
        a.verify_totp(&tok, &code(&secret, T0 + 40), "1.1.1.1", T0 + 40).await.unwrap();
        assert_eq!(a.session(&tok, T0 + 40).await.unwrap().stage, Stage::Full);
    }

    #[tokio::test]
    async fn short_new_password_is_rejected() {
        let a = auth().await;
        let (_, temp) = a.create_user("a@example.org", "A", false).await.unwrap();
        let (tok, _) = a.login("a@example.org", &temp, "1.1.1.1", "ua", T0).await.unwrap();
        assert!(matches!(a.setup_password(&tok, "corta", T0).await, Err(AuthError::WeakPassword)));
    }

    #[tokio::test]
    async fn totp_code_cannot_be_reused() {
        let a = auth().await;
        let (_, secret) = active_user(&a, "a@example.org", false).await;
        let c = code(&secret, T0 + 60);
        let (t1, _) = a.login("a@example.org", "una-password-lunga", "1.1.1.1", "ua", T0 + 60).await.unwrap();
        a.verify_totp(&t1, &c, "1.1.1.1", T0 + 60).await.unwrap();
        let (t2, _) = a.login("a@example.org", "una-password-lunga", "1.1.1.1", "ua", T0 + 61).await.unwrap();
        assert!(a.verify_totp(&t2, &c, "1.1.1.1", T0 + 61).await.is_err(), "codice riusato accettato");
        a.verify_totp(&t2, &code(&secret, T0 + 90), "1.1.1.1", T0 + 90).await.unwrap();
    }

    #[tokio::test]
    async fn wrong_totp_does_not_promote_session() {
        let a = auth().await;
        active_user(&a, "a@example.org", false).await;
        let (tok, _) = a.login("a@example.org", "una-password-lunga", "1.1.1.1", "ua", T0).await.unwrap();
        assert!(a.verify_totp(&tok, "000000", "1.1.1.1", T0).await.is_err());
        assert_eq!(a.session(&tok, T0).await.unwrap().stage, Stage::Totp);
    }

    #[tokio::test]
    async fn five_failures_lock_the_account_even_with_right_password() {
        let a = auth().await;
        active_user(&a, "a@example.org", false).await;
        for i in 0..5 {
            assert!(matches!(a.login("a@example.org", "sbagliata", &format!("10.0.0.{i}"), "ua", T0).await, Err(AuthError::Invalid)));
        }
        assert!(matches!(a.login("a@example.org", "una-password-lunga", "9.9.9.9", "ua", T0).await, Err(AuthError::Locked)));
        assert!(a.login("a@example.org", "una-password-lunga", "9.9.9.9", "ua", T0 + LOCK_SECS + 1).await.is_ok());
    }

    #[tokio::test]
    async fn five_failures_from_one_ip_lock_that_ip() {
        let a = auth().await;
        active_user(&a, "b@example.org", false).await;
        for i in 0..5 {
            let _ = a.login(&format!("x{i}@example.org"), "sbagliata", "6.6.6.6", "ua", T0).await;
        }
        assert!(matches!(a.login("b@example.org", "una-password-lunga", "6.6.6.6", "ua", T0).await, Err(AuthError::Locked)));
        assert!(a.login("b@example.org", "una-password-lunga", "7.7.7.7", "ua", T0).await.is_ok());
    }

    #[tokio::test]
    async fn session_expires_after_12_hours_of_inactivity() {
        let a = auth().await;
        active_user(&a, "a@example.org", false).await;
        let (tok, _) = a.login("a@example.org", "una-password-lunga", "1.1.1.1", "ua", T0).await.unwrap();
        assert!(a.session(&tok, T0 + IDLE_SECS - 10).await.is_some());
        // l'uso rinnova la scadenza
        assert!(a.session(&tok, T0 + 2 * IDLE_SECS - 20).await.is_some());
        assert!(a.session(&tok, T0 + 3 * IDLE_SECS).await.is_none());
        a.logout(&tok).await.unwrap();
        assert!(a.session(&tok, T0).await.is_none());
    }

    #[tokio::test]
    async fn blocked_user_cannot_log_in() {
        let a = auth().await;
        let (id, _) = active_user(&a, "a@example.org", false).await;
        a.set_blocked(id, true).await.unwrap();
        assert!(matches!(a.login("a@example.org", "una-password-lunga", "1.1.1.1", "ua", T0).await, Err(AuthError::Invalid)));
    }

    #[tokio::test]
    async fn api_token_is_shown_once_and_expires() {
        let a = auth().await;
        let (id, _) = active_user(&a, "a@example.org", false).await;
        let tok = a.create_api_token(id, "script", T0 + 100).await.unwrap();
        assert!(tok.starts_with("alv_"));
        assert_eq!(a.token_user(&tok, T0).await.unwrap().id, id);
        assert!(a.token_user(&tok, T0 + 101).await.is_none());
        assert!(a.token_user("alv_inventato", T0).await.is_none());
    }

    #[tokio::test]
    async fn site_roles_decide_what_a_user_can_do() {
        let a = auth().await;
        let store = Store::from_pool(a.pool.clone());
        let new = |d: &str| NewSite { primary: Domain::parse(d).unwrap(), kind: SiteKind::Php, template: "php".into() };
        let x = store.create_site(&new("x.it")).await.unwrap().id;
        let y = store.create_site(&new("y.it")).await.unwrap().id;
        let (admin, _) = active_user(&a, "admin@example.org", true).await;
        let (gest, _) = active_user(&a, "g@example.org", false).await;
        a.assign_site(gest, x, Role::Manager).await.unwrap();
        a.assign_site(gest, y, Role::Readonly).await.unwrap();
        let admin = a.user(admin).await.unwrap();
        let gest = a.user(gest).await.unwrap();
        assert!(a.can(&admin, y, Need::Manage).await.unwrap());
        assert!(a.can(&gest, x, Need::Manage).await.unwrap());
        assert!(a.can(&gest, y, Need::Read).await.unwrap());
        assert!(!a.can(&gest, y, Need::Manage).await.unwrap());
        assert_eq!(a.visible_sites(&gest).await.unwrap(), Some(vec![x, y]));
        assert_eq!(a.visible_sites(&admin).await.unwrap(), None);
    }
}
