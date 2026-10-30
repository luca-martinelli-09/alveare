//! Passkey (WebAuthn) come alternativa a password + TOTP: la verifica dell'utente sul
//! dispositivo (impronta, PIN) vale da secondo fattore. Le cerimonie in corso restano in
//! memoria per 5 minuti: il pannello è uno solo.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::Row;
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse, Url, Uuid, Webauthn, WebauthnBuilder,
};

use super::sessions::link_token;
use super::{Auth, AuthError, Result, Stage};

const CEREMONY_SECS: i64 = 300;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PasskeyInfo {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub last_used: Option<i64>,
}

enum Pending {
    Register { user_id: i64, state: PasskeyRegistration },
    Login { user_id: i64, email: String, state: PasskeyAuthentication },
}

#[derive(Clone)]
pub struct Passkeys {
    webauthn: Arc<Webauthn>,
    pending: Arc<Mutex<HashMap<String, (Pending, i64)>>>,
}

fn internal(e: impl std::fmt::Display) -> AuthError {
    AuthError::Internal(e.to_string())
}

impl Passkeys {
    /// `origin` è l'URL pubblico del pannello (https://pannello.esempio.it): l'rp id è il suo host.
    pub fn new(origin: &str) -> anyhow::Result<Self> {
        let url = Url::parse(origin)?;
        let rp_id = url.host_str().ok_or_else(|| anyhow::anyhow!("URL del pannello senza host"))?.to_string();
        let webauthn = WebauthnBuilder::new(&rp_id, &url)?.rp_name("Alveare").build()?;
        Ok(Self { webauthn: Arc::new(webauthn), pending: Default::default() })
    }

    fn put(&self, p: Pending, now: i64) -> String {
        let id = link_token();
        let mut map = self.pending.lock().expect("lock passkey");
        map.retain(|_, (_, exp)| *exp > now);
        map.insert(id.clone(), (p, now + CEREMONY_SECS));
        id
    }

    fn take(&self, id: &str, now: i64) -> Option<Pending> {
        let mut map = self.pending.lock().expect("lock passkey");
        map.remove(id).filter(|(_, exp)| *exp > now).map(|(p, _)| p)
    }

    pub async fn start_registration(&self, auth: &Auth, user_id: i64, now: i64) -> Result<(String, CreationChallengeResponse)> {
        let user = auth.user(user_id).await?;
        let uuid = auth.webauthn_id(user_id).await?;
        let exclude: Vec<_> = auth.passkeys_of(user_id).await?.into_iter().map(|(_, pk)| pk.cred_id().clone()).collect();
        let (ccr, state) = self.webauthn.start_passkey_registration(uuid, &user.email, &user.name, Some(exclude)).map_err(internal)?;
        Ok((self.put(Pending::Register { user_id, state }, now), ccr))
    }

    pub async fn finish_registration(&self, auth: &Auth, user_id: i64, id: &str, name: &str, cred: &RegisterPublicKeyCredential, now: i64) -> Result<i64> {
        let Some(Pending::Register { user_id: owner, state }) = self.take(id, now) else { return Err(AuthError::WrongStage) };
        if owner != user_id {
            return Err(AuthError::WrongStage);
        }
        let pk = self.webauthn.finish_passkey_registration(cred, &state).map_err(|_| AuthError::Invalid)?;
        auth.add_passkey(user_id, name, &pk, now).await
    }

    /// Accesso con l'email: le passkey di quell'utente. Senza passkey, `Invalid` come una password sbagliata.
    pub async fn start_login(&self, auth: &Auth, email: &str, ip: &str, now: i64) -> Result<(String, RequestChallengeResponse)> {
        let keys = Auth::failure_keys(email, ip);
        auth.check_not_locked(&keys, now).await?;
        let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE email = ? AND status = 'active'").bind(email.trim()).fetch_optional(&auth.pool).await?;
        let passkeys: Vec<Passkey> = match user_id {
            Some(id) => auth.passkeys_of(id).await?.into_iter().map(|(_, pk)| pk).collect(),
            None => vec![],
        };
        let (Some(user_id), false) = (user_id, passkeys.is_empty()) else {
            auth.record_failure(&keys, now).await?;
            return Err(AuthError::Invalid);
        };
        let (rcr, state) = self.webauthn.start_passkey_authentication(&passkeys).map_err(internal)?;
        Ok((self.put(Pending::Login { user_id, email: email.trim().to_string(), state }, now), rcr))
    }

    /// Sessione completa se la passkey è valida; il contatore della credenziale si aggiorna.
    pub async fn finish_login(&self, auth: &Auth, id: &str, cred: &PublicKeyCredential, ip: &str, ua: &str, now: i64) -> Result<String> {
        let Some(Pending::Login { user_id, email, state }) = self.take(id, now) else { return Err(AuthError::Invalid) };
        let keys = Auth::failure_keys(&email, ip);
        auth.check_not_locked(&keys, now).await?;
        let result = match self.webauthn.finish_passkey_authentication(cred, &state) {
            Ok(r) => r,
            Err(_) => {
                auth.record_failure(&keys, now).await?;
                return Err(AuthError::Invalid);
            }
        };
        auth.clear_failures(&keys).await?;
        for (info, mut pk) in auth.passkeys_of(user_id).await? {
            if pk.update_credential(&result).is_some() {
                auth.save_passkey(info.id, &pk, now).await?;
            }
        }
        auth.open_session(user_id, Stage::Full, ip, ua, now).await
    }
}

impl Auth {
    /// Identificativo WebAuthn dell'utente: casuale, creato la prima volta.
    pub async fn webauthn_id(&self, user_id: i64) -> Result<Uuid> {
        let current: Option<String> = sqlx::query_scalar("SELECT webauthn_id FROM users WHERE id = ?").bind(user_id).fetch_one(&self.pool).await?;
        if let Some(id) = current.and_then(|s| Uuid::parse_str(&s).ok()) {
            return Ok(id);
        }
        let id = Uuid::new_v4();
        sqlx::query("UPDATE users SET webauthn_id = ? WHERE id = ?").bind(id.to_string()).bind(user_id).execute(&self.pool).await?;
        Ok(id)
    }

    pub async fn passkeys_of(&self, user_id: i64) -> Result<Vec<(PasskeyInfo, Passkey)>> {
        let rows = sqlx::query("SELECT id, name, data, created_at, last_used FROM passkeys WHERE user_id = ? ORDER BY id").bind(user_id).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| {
                let pk: Passkey = serde_json::from_str(r.try_get("data")?).map_err(internal)?;
                Ok((PasskeyInfo { id: r.try_get("id")?, name: r.try_get("name")?, created_at: r.try_get("created_at")?, last_used: r.try_get("last_used")? }, pk))
            })
            .collect()
    }

    async fn add_passkey(&self, user_id: i64, name: &str, pk: &Passkey, now: i64) -> Result<i64> {
        let cred_id = serde_json::to_string(pk.cred_id()).map_err(internal)?;
        Ok(sqlx::query_scalar("INSERT INTO passkeys (user_id, name, cred_id, data, created_at) VALUES (?, ?, ?, ?, ?) RETURNING id")
            .bind(user_id)
            .bind(name)
            .bind(cred_id)
            .bind(serde_json::to_string(pk).map_err(internal)?)
            .bind(now)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn save_passkey(&self, id: i64, pk: &Passkey, now: i64) -> Result<()> {
        sqlx::query("UPDATE passkeys SET data = ?, last_used = ? WHERE id = ?").bind(serde_json::to_string(pk).map_err(internal)?).bind(now).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn delete_passkey(&self, user_id: i64, id: i64) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM passkeys WHERE user_id = ? AND id = ?").bind(user_id).bind(id).execute(&self.pool).await?.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::SecretKey;
    use crate::store::Store;
    use webauthn_authenticator_rs::softpasskey::SoftPasskey;
    use webauthn_authenticator_rs::WebauthnAuthenticator;

    const ORIGIN: &str = "https://pannello.example.org";

    async fn auth() -> Auth {
        let store = Store::open_memory().await.unwrap();
        Auth::new(store.pool().clone(), SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap())
    }

    #[tokio::test]
    async fn register_then_log_in_with_a_soft_passkey() {
        let a = auth().await;
        let pk = Passkeys::new(ORIGIN).unwrap();
        let (user, _) = a.create_user("a@example.org", "A", false).await.unwrap();
        sqlx::query("UPDATE users SET must_change_password = 0").execute(&a.pool).await.unwrap();
        let mut device = WebauthnAuthenticator::new(SoftPasskey::new(true));
        let origin = Url::parse(ORIGIN).unwrap();

        let (rid, ccr) = pk.start_registration(&a, user, 100).await.unwrap();
        let reg = device.do_registration(origin.clone(), ccr).unwrap();
        pk.finish_registration(&a, user, &rid, "Portatile", &reg, 110).await.unwrap();
        assert_eq!(a.passkeys_of(user).await.unwrap()[0].0.name, "Portatile");

        let (lid, rcr) = pk.start_login(&a, "a@example.org", "1.1.1.1", 200).await.unwrap();
        let assertion = device.do_authentication(origin.clone(), rcr).unwrap();
        let session = pk.finish_login(&a, &lid, &assertion, "1.1.1.1", "x", 210).await.unwrap();
        assert_eq!(a.session(&session, 210).await.unwrap().stage, Stage::Full);
        assert!(a.passkeys_of(user).await.unwrap()[0].0.last_used.is_some());
        assert!(pk.finish_login(&a, &lid, &assertion, "1.1.1.1", "x", 220).await.is_err(), "cerimonia monouso");
    }

    #[tokio::test]
    async fn login_without_passkeys_or_with_the_wrong_one_fails_and_counts() {
        let a = auth().await;
        let pk = Passkeys::new(ORIGIN).unwrap();
        a.create_user("a@example.org", "A", false).await.unwrap();
        assert!(matches!(pk.start_login(&a, "a@example.org", "1.1.1.1", 100).await, Err(AuthError::Invalid)));
        assert!(matches!(pk.start_login(&a, "nessuno@example.org", "1.1.1.1", 100).await, Err(AuthError::Invalid)));
        // Una cerimonia di registrazione non vale per un altro utente.
        let (other, _) = a.create_user("b@example.org", "B", false).await.unwrap();
        let (alice, _) = (1, ());
        let (rid, _) = pk.start_registration(&a, alice, 100).await.unwrap();
        let mut device = WebauthnAuthenticator::new(SoftPasskey::new(true));
        let (_, ccr) = pk.start_registration(&a, other, 100).await.unwrap();
        let reg = device.do_registration(Url::parse(ORIGIN).unwrap(), ccr).unwrap();
        assert!(pk.finish_registration(&a, other, &rid, "x", &reg, 110).await.is_err());
    }
}
