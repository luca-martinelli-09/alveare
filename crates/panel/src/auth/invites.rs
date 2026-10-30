//! Inviti e reset della password via email. L'invito apre una sessione di primo accesso
//! (password + TOTP come per la password temporanea); il reset chiede anche il codice TOTP,
//! così chi legge l'email non prende l'account senza il secondo fattore.
use sqlx::Row;

use super::sessions::link_token;
use super::{hash_password, sha256_hex, AuthError, Auth, Result, Stage, MIN_PASSWORD};
use crate::secrets::random_password;

pub const INVITE_SECS: i64 = 72 * 3600;
pub const RESET_SECS: i64 = 3600;
/// Un reset ogni 5 minuti per utente: l'endpoint risponde sempre uguale, ma non spedisce a raffica.
const RESET_EVERY: i64 = 300;

impl Auth {
    async fn new_link(&self, user_id: i64, purpose: &str, ttl: i64, now: i64) -> Result<String> {
        sqlx::query("DELETE FROM user_links WHERE user_id = ? AND purpose = ?").bind(user_id).bind(purpose).execute(&self.pool).await?;
        let token = link_token();
        sqlx::query("INSERT INTO user_links (token_hash, user_id, purpose, expires_at, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(sha256_hex(&token))
            .bind(user_id)
            .bind(purpose)
            .bind(now + ttl)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(token)
    }

    /// Utente invitato: nessuna password utilizzabile finché non apre il link.
    pub async fn create_invited_user(&self, email: &str, name: &str, is_admin: bool, now: i64) -> Result<(i64, String)> {
        let id: i64 = sqlx::query_scalar("INSERT INTO users (email, name, is_admin, password_hash, status) VALUES (?, ?, ?, ?, 'invited') RETURNING id")
            .bind(email.trim())
            .bind(name)
            .bind(is_admin)
            .bind(hash_password(&random_password())?)
            .fetch_one(&self.pool)
            .await?;
        Ok((id, self.new_link(id, "invite", INVITE_SECS, now).await?))
    }

    /// Stato di ogni utente (invited, active, blocked), per l'elenco degli admin.
    pub async fn statuses(&self) -> Result<std::collections::HashMap<i64, String>> {
        let rows = sqlx::query("SELECT id, status FROM users").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|r| (r.get("id"), r.get("status"))).collect())
    }

    /// Solo per annullare un invito che non è partito.
    pub async fn delete_user(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE id = ? AND status = 'invited'").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Nuovo link d'invito (il precedente smette di valere); solo per chi non ha ancora accettato.
    pub async fn reinvite(&self, user_id: i64, now: i64) -> Result<(String, String)> {
        let r = sqlx::query("SELECT email, status FROM users WHERE id = ?").bind(user_id).fetch_optional(&self.pool).await?.ok_or(AuthError::Invalid)?;
        if r.get::<&str, _>("status") != "invited" {
            return Err(AuthError::WrongStage);
        }
        Ok((r.get("email"), self.new_link(user_id, "invite", INVITE_SECS, now).await?))
    }

    async fn take_link(&self, token: &str, purpose: &str, now: i64) -> Result<i64> {
        let hash = sha256_hex(token.trim());
        let user: Option<i64> = sqlx::query_scalar("SELECT user_id FROM user_links WHERE token_hash = ? AND purpose = ? AND expires_at > ?")
            .bind(&hash)
            .bind(purpose)
            .bind(now)
            .fetch_optional(&self.pool)
            .await?;
        let user = user.ok_or(AuthError::Invalid)?;
        sqlx::query("DELETE FROM user_links WHERE token_hash = ?").bind(&hash).execute(&self.pool).await?;
        Ok(user)
    }

    pub(super) async fn open_session(&self, user_id: i64, stage: Stage, ip: &str, ua: &str, now: i64) -> Result<String> {
        let token = link_token();
        sqlx::query("INSERT INTO sessions (id, user_id, stage, created_at, last_used, ip, user_agent) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(sha256_hex(&token))
            .bind(user_id)
            .bind(stage.as_str())
            .bind(now)
            .bind(now)
            .bind(ip)
            .bind(ua)
            .execute(&self.pool)
            .await?;
        Ok(token)
    }

    /// Link d'invito: sessione di primo accesso (nuova password e TOTP con i passi soliti).
    pub async fn accept_invite(&self, token: &str, ip: &str, ua: &str, now: i64) -> Result<String> {
        let user = self.take_link(token, "invite", now).await?;
        sqlx::query("UPDATE users SET status = 'active', must_change_password = 1 WHERE id = ? AND status = 'invited'").bind(user).execute(&self.pool).await?;
        self.open_session(user, Stage::Setup, ip, ua, now).await
    }

    /// (email, token) se l'utente esiste, è attivo e non ha chiesto un reset da poco.
    pub async fn reset_link(&self, email: &str, now: i64) -> Result<Option<(String, String)>> {
        let r = sqlx::query("SELECT id, email FROM users WHERE email = ? AND status = 'active'").bind(email.trim()).fetch_optional(&self.pool).await?;
        let Some(r) = r else { return Ok(None) };
        let id: i64 = r.get("id");
        let recent: Option<i64> = sqlx::query_scalar("SELECT created_at FROM user_links WHERE user_id = ? AND purpose = 'reset'").bind(id).fetch_optional(&self.pool).await?;
        if recent.is_some_and(|t| now - t < RESET_EVERY) {
            return Ok(None);
        }
        Ok(Some((r.get("email"), self.new_link(id, "reset", RESET_SECS, now).await?)))
    }

    /// Nuova password con il link e il codice TOTP: sessione completa. Senza TOTP configurato
    /// (primo accesso mai finito) si torna al primo accesso.
    pub async fn reset_password(&self, token: &str, password: &str, code: &str, ip: &str, ua: &str, now: i64) -> Result<(String, Stage)> {
        if password.chars().count() < MIN_PASSWORD {
            return Err(AuthError::WeakPassword);
        }
        let hash = sha256_hex(token.trim());
        let user_id: i64 = sqlx::query_scalar("SELECT user_id FROM user_links WHERE token_hash = ? AND purpose = 'reset' AND expires_at > ?")
            .bind(&hash)
            .bind(now)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(AuthError::Invalid)?;
        let user = self.user(user_id).await?;
        let secret: Option<Vec<u8>> = sqlx::query_scalar("SELECT totp_secret_enc FROM users WHERE id = ?").bind(user_id).fetch_one(&self.pool).await?;
        let stage = match secret {
            _ if !self.totp_required => Stage::Full,
            Some(enc) => {
                let secret = self.key.decrypt(&enc).map_err(|e| AuthError::Internal(e.to_string()))?;
                // Un codice sbagliato non consuma il link: si può riprovare (con il blocco dopo 5 errori).
                self.check_code(&user, &secret, code, ip, now).await?;
                Stage::Full
            }
            None => Stage::Setup,
        };
        self.take_link(token, "reset", now).await?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash_password(password)?).bind(user_id).execute(&self.pool).await?;
        // Sessioni e token API si chiudono: chi aveva la vecchia password non resta dentro.
        self.revoke_access(user_id, "", false).await?;
        Ok((self.open_session(user_id, stage, ip, ua, now).await?, stage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::SecretKey;
    use crate::store::Store;

    async fn auth() -> Auth {
        let store = Store::open_memory().await.unwrap();
        Auth::new(store.pool().clone(), SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap())
    }

    #[tokio::test]
    async fn without_totp_reset_needs_no_code() {
        let a = auth().await.without_totp();
        let (_, temp) = a.create_user("a@x.it", "A", false).await.unwrap();
        let (s, _) = a.login("a@x.it", &temp, "1.1.1.1", "x", 100).await.unwrap();
        a.setup_password(&s, "una-password-lunga", 100).await.unwrap();
        let (_, token) = a.reset_link("a@x.it", 1000).await.unwrap().unwrap();
        let (_, stage) = a.reset_password(&token, "nuova-password-lunga", "", "1.1.1.1", "x", 1000).await.unwrap();
        assert_eq!(stage, Stage::Full);
    }

    #[tokio::test]
    async fn invite_link_opens_first_access_once_and_expires() {
        let a = auth().await;
        let (id, token) = a.create_invited_user("nuovo@x.it", "Nuovo", false, 1000).await.unwrap();
        assert!(matches!(a.login("nuovo@x.it", "", "1.1.1.1", "x", 1000).await, Err(AuthError::Invalid)));
        let session = a.accept_invite(&token, "1.1.1.1", "x", 2000).await.unwrap();
        assert_eq!(a.session(&session, 2000).await.unwrap().stage, Stage::Setup);
        assert!(a.accept_invite(&token, "1.1.1.1", "x", 2000).await.is_err(), "monouso");
        assert!(a.reinvite(id, 2000).await.is_err(), "già accettato");
        let (_, late) = a.create_invited_user("tardi@x.it", "T", false, 0).await.unwrap();
        assert!(a.accept_invite(&late, "1.1.1.1", "x", INVITE_SECS + 1).await.is_err(), "scaduto");
    }

    #[tokio::test]
    async fn reset_needs_the_totp_code_and_closes_other_sessions() {
        let a = auth().await;
        let (id, temp) = a.create_user("a@x.it", "A", false).await.unwrap();
        let (s, _) = a.login("a@x.it", &temp, "1.1.1.1", "x", 100).await.unwrap();
        let setup = a.setup_password(&s, "una-password-lunga", 100).await.unwrap().unwrap();
        let code = |t: i64| super::super::totp(&setup.secret, "a@x.it").unwrap().generate(t as u64).to_string();
        a.confirm_totp(&s, &code(100), "1.1.1.1", 100).await.unwrap();
        let (_, link) = a.reset_link("a@x.it", 1000).await.unwrap().unwrap();
        assert!(a.reset_link("a@x.it", 1100).await.unwrap().is_none(), "non a raffica");
        assert!(a.reset_link("nessuno@x.it", 1000).await.unwrap().is_none());
        assert!(a.reset_password(&link, "nuova-password-lunga", "000000", "1.1.1.1", "x", 1000).await.is_err());
        let api = a.create_api_token(id, "cli", i64::MAX).await.unwrap();
        let (session, stage) = a.reset_password(&link, "nuova-password-lunga", &code(1000), "1.1.1.1", "x", 1000).await.unwrap();
        assert!(a.token_user(&api, 1000).await.is_none(), "i token API si revocano");
        assert_eq!(stage, Stage::Full);
        assert!(a.session(&s, 1000).await.is_none(), "la vecchia sessione è chiusa");
        assert!(a.session(&session, 1000).await.is_some());
        assert!(a.reset_password(&link, "altra-password-lunga", &code(1030), "1.1.1.1", "x", 1030).await.is_err(), "link usato");
        assert_eq!(a.login("a@x.it", "nuova-password-lunga", "1.1.1.1", "x", 1100).await.unwrap().1, Stage::Totp);
    }
}
