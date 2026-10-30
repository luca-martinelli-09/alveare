//! Profilo (nome, email, password, TOTP) per sé stessi e gestione degli utenti per gli admin:
//! ruolo, password temporanea, azzeramento del TOTP.
use sqlx::Row;
use totp_rs::Secret;

use super::{hash_password, sha256_hex, totp, user_from_row, verify_password, Auth, AuthError, Result, TotpSetup, User, MIN_PASSWORD};
use crate::secrets::random_password;

#[derive(Debug, serde::Serialize)]
pub struct UserDetail {
    pub user: User,
    pub status: String,
    pub has_totp: bool,
    pub must_change_password: bool,
    pub passkeys: i64,
}

fn unique(e: sqlx::Error) -> AuthError {
    match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => AuthError::Taken,
        e => e.into(),
    }
}

impl Auth {
    pub async fn user_detail(&self, id: i64) -> Result<UserDetail> {
        let r = sqlx::query(
            "SELECT id, email, name, is_admin, status, totp_secret_enc IS NOT NULL AS has_totp, must_change_password,
                    (SELECT COUNT(*) FROM passkeys p WHERE p.user_id = users.id) AS passkeys FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(UserDetail {
            user: user_from_row(&r)?,
            status: r.try_get("status")?,
            has_totp: r.try_get("has_totp")?,
            must_change_password: r.try_get("must_change_password")?,
            passkeys: r.try_get("passkeys")?,
        })
    }

    pub async fn update_profile(&self, id: i64, name: &str, email: &str) -> Result<()> {
        sqlx::query("UPDATE users SET name = ?, email = ? WHERE id = ?").bind(name.trim()).bind(email.trim()).bind(id).execute(&self.pool).await.map_err(unique)?;
        Ok(())
    }

    async fn password_ok(&self, id: i64, password: &str) -> Result<bool> {
        let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?").bind(id).fetch_one(&self.pool).await?;
        Ok(verify_password(password, &hash))
    }

    /// Nuova password con quella attuale; le altre sessioni si chiudono, questa resta.
    pub async fn change_password(&self, id: i64, current: &str, new: &str, keep_token: &str) -> Result<()> {
        if !self.password_ok(id, current).await? {
            return Err(AuthError::Invalid);
        }
        if new.chars().count() < MIN_PASSWORD {
            return Err(AuthError::WeakPassword);
        }
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash_password(new)?).bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id != ?").bind(id).bind(sha256_hex(keep_token)).execute(&self.pool).await?;
        Ok(())
    }

    /// Nuovo segreto TOTP (per cambiare app), attivo solo dopo `confirm_totp_change`.
    pub async fn start_totp_change(&self, id: i64, password: &str) -> Result<TotpSetup> {
        if !self.password_ok(id, password).await? {
            return Err(AuthError::Invalid);
        }
        let user = self.user(id).await?;
        let secret = Secret::generate().to_base32();
        sqlx::query("UPDATE users SET totp_pending_enc = ? WHERE id = ?").bind(self.key.encrypt(&secret)).bind(id).execute(&self.pool).await?;
        let otpauth_url = totp(&secret, &user.email)?.to_url().map_err(|e| AuthError::Internal(format!("{e:?}")))?;
        Ok(TotpSetup { secret, otpauth_url })
    }

    pub async fn confirm_totp_change(&self, id: i64, code: &str, ip: &str, now: i64) -> Result<()> {
        let user = self.user(id).await?;
        let pending: Option<Vec<u8>> = sqlx::query_scalar("SELECT totp_pending_enc FROM users WHERE id = ?").bind(id).fetch_one(&self.pool).await?;
        let secret = self.key.decrypt(&pending.ok_or(AuthError::WrongStage)?).map_err(|e| AuthError::Internal(e.to_string()))?;
        self.check_code(&user, &secret, code, ip, now).await?;
        sqlx::query("UPDATE users SET totp_secret_enc = totp_pending_enc, totp_pending_enc = NULL WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Admin: password temporanea da cambiare al primo accesso; sessioni chiuse.
    pub async fn temporary_password(&self, id: i64) -> Result<String> {
        let temp = random_password();
        sqlx::query("UPDATE users SET password_hash = ?, must_change_password = 1 WHERE id = ?").bind(hash_password(&temp)?).bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&self.pool).await?;
        Ok(temp)
    }

    /// Admin: toglie il TOTP (telefono perso); al prossimo accesso lo riconfigura. Sessioni chiuse.
    pub async fn reset_totp(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE users SET totp_secret_enc = NULL, totp_pending_enc = NULL WHERE id = ?").bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Admin: ruolo di amministratore, senza mai restare senza admin attivi.
    pub async fn set_admin(&self, id: i64, is_admin: bool) -> Result<()> {
        if !is_admin {
            let others: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_admin = 1 AND status = 'active' AND id != ?").bind(id).fetch_one(&self.pool).await?;
            if others == 0 {
                return Err(AuthError::LastAdmin);
            }
        }
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?").bind(is_admin).bind(id).execute(&self.pool).await?;
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::store::Store;

    const T0: i64 = 1_800_000_000;

    async fn auth() -> Auth {
        let store = Store::open_memory().await.unwrap();
        Auth::new(store.pool().clone(), SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap())
    }

    fn code(secret: &str, now: i64) -> String {
        totp(secret, "x").unwrap().generate(now as u64).to_string()
    }

    /// Utente attivo con password "una-password-lunga" e TOTP; restituisce (id, segreto, token di sessione).
    async fn active(a: &Auth, email: &str, admin: bool) -> (i64, String, String) {
        let (id, temp) = a.create_user(email, "Prova", admin).await.unwrap();
        let (tok, _) = a.login(email, &temp, "1.1.1.1", "ua", T0).await.unwrap();
        let secret = a.setup_password(&tok, "una-password-lunga", T0).await.unwrap().unwrap().secret;
        a.confirm_totp(&tok, &code(&secret, T0), "1.1.1.1", T0).await.unwrap();
        (id, secret, tok)
    }

    #[tokio::test]
    async fn profile_name_and_email_change_and_email_stays_unique() {
        let a = auth().await;
        let (id, _, _) = active(&a, "a@x.it", false).await;
        active(&a, "b@x.it", false).await;
        a.update_profile(id, "Anna", "anna@x.it").await.unwrap();
        let u = a.user(id).await.unwrap();
        assert_eq!((u.name.as_str(), u.email.as_str()), ("Anna", "anna@x.it"));
        assert!(matches!(a.update_profile(id, "Anna", "b@x.it").await, Err(AuthError::Taken)));
    }

    #[tokio::test]
    async fn password_change_needs_the_current_one_and_closes_other_sessions() {
        let a = auth().await;
        let (id, secret, tok) = active(&a, "a@x.it", false).await;
        let (other, _) = a.login("a@x.it", "una-password-lunga", "2.2.2.2", "ua", T0 + 60).await.unwrap();
        a.verify_totp(&other, &code(&secret, T0 + 60), "2.2.2.2", T0 + 60).await.unwrap();
        assert!(matches!(a.change_password(id, "sbagliata", "nuova-password-lunga", &tok).await, Err(AuthError::Invalid)));
        assert!(matches!(a.change_password(id, "una-password-lunga", "corta", &tok).await, Err(AuthError::WeakPassword)));
        a.change_password(id, "una-password-lunga", "nuova-password-lunga", &tok).await.unwrap();
        assert!(a.session(&tok, T0 + 61).await.is_some(), "questa sessione resta");
        assert!(a.session(&other, T0 + 61).await.is_none(), "le altre si chiudono");
        assert!(a.login("a@x.it", "nuova-password-lunga", "1.1.1.1", "ua", T0 + 62).await.is_ok());
    }

    #[tokio::test]
    async fn totp_can_be_moved_to_a_new_app() {
        let a = auth().await;
        let (id, old, _) = active(&a, "a@x.it", false).await;
        assert!(matches!(a.start_totp_change(id, "sbagliata").await, Err(AuthError::Invalid)));
        let setup = a.start_totp_change(id, "una-password-lunga").await.unwrap();
        assert!(a.confirm_totp_change(id, &code(&old, T0 + 90), "1.1.1.1", T0 + 90).await.is_err(), "serve il codice della nuova app");
        a.confirm_totp_change(id, &code(&setup.secret, T0 + 120), "1.1.1.1", T0 + 120).await.unwrap();
        let (tok, _) = a.login("a@x.it", "una-password-lunga", "1.1.1.1", "ua", T0 + 200).await.unwrap();
        a.verify_totp(&tok, &code(&setup.secret, T0 + 210), "1.1.1.1", T0 + 210).await.unwrap();
    }

    #[tokio::test]
    async fn admins_reset_password_and_totp_and_the_last_admin_stays() {
        let a = auth().await;
        let (admin, _, _) = active(&a, "admin@x.it", true).await;
        let (id, _, tok) = active(&a, "a@x.it", false).await;
        let temp = a.temporary_password(id).await.unwrap();
        assert!(a.session(&tok, T0 + 1).await.is_none(), "sessioni chiuse");
        assert_eq!(a.login("a@x.it", &temp, "1.1.1.1", "ua", T0 + 2).await.unwrap().1, Stage::Setup, "al primo accesso la cambia");
        let (_, _, tok) = active(&a, "b@x.it", false).await;
        let b = a.user_detail(a.users().await.unwrap().iter().find(|u| u.email == "b@x.it").unwrap().id).await.unwrap();
        assert!(b.has_totp);
        a.reset_totp(b.user.id).await.unwrap();
        assert!(a.session(&tok, T0 + 3).await.is_none());
        assert!(!a.user_detail(b.user.id).await.unwrap().has_totp);
        assert!(matches!(a.set_admin(admin, false).await, Err(AuthError::LastAdmin)), "almeno un admin");
        a.set_admin(id, true).await.unwrap();
        a.set_admin(admin, false).await.unwrap();
        assert!(!a.user(admin).await.unwrap().is_admin);
    }
}
