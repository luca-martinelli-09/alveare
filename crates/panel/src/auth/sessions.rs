//! Sessioni e token API dal profilo: elenco, revoca di una sessione o di tutte le altre.
use rand::distr::{Alphanumeric, SampleString};
use sqlx::Row;

use super::{sha256_hex, Auth, Result, IDLE_SECS};

/// L'id pubblico è un prefisso dell'hash del token: basta per revocarla, non per usarla.
const PUBLIC_ID_LEN: usize = 16;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: i64,
    pub last_used: i64,
    pub ip: String,
    pub user_agent: String,
    pub current: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TokenInfo {
    pub id: i64,
    pub name: String,
    pub expires_at: i64,
    pub last_used: Option<i64>,
}

impl Auth {
    /// Sessioni ancora valide dell'utente; `current` segna quella di chi chiede.
    pub async fn sessions_of(&self, user_id: i64, current_token: Option<&str>, now: i64) -> Result<Vec<SessionInfo>> {
        let current = current_token.map(sha256_hex);
        let rows = sqlx::query("SELECT id, created_at, last_used, ip, user_agent FROM sessions WHERE user_id = ? AND last_used > ? ORDER BY last_used DESC")
            .bind(user_id)
            .bind(now - IDLE_SECS)
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                let id: String = r.try_get("id")?;
                Ok(SessionInfo {
                    current: current.as_deref() == Some(id.as_str()),
                    id: id[..PUBLIC_ID_LEN].to_string(),
                    created_at: r.try_get("created_at")?,
                    last_used: r.try_get("last_used")?,
                    ip: r.try_get("ip")?,
                    user_agent: r.try_get("user_agent")?,
                })
            })
            .collect()
    }

    /// Revoca una sessione dell'utente (solo sue); `false` se non c'era.
    pub async fn revoke_session(&self, user_id: i64, public_id: &str) -> Result<bool> {
        if public_id.len() != PUBLIC_ID_LEN || !public_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(false);
        }
        let done = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND substr(id, 1, ?) = ?")
            .bind(user_id)
            .bind(PUBLIC_ID_LEN as i64)
            .bind(public_id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Esci ovunque tranne qui; restituisce quante sessioni sono state chiuse.
    pub async fn revoke_other_sessions(&self, user_id: i64, current_token: &str) -> Result<u64> {
        let done = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id != ?").bind(user_id).bind(sha256_hex(current_token)).execute(&self.pool).await?;
        Ok(done.rows_affected())
    }

    pub async fn tokens_of(&self, user_id: i64) -> Result<Vec<TokenInfo>> {
        let rows = sqlx::query("SELECT id, name, expires_at, last_used FROM api_tokens WHERE user_id = ? ORDER BY id DESC").bind(user_id).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| Ok(TokenInfo { id: r.try_get("id")?, name: r.try_get("name")?, expires_at: r.try_get("expires_at")?, last_used: r.try_get("last_used")? }))
            .collect()
    }

    pub async fn revoke_token(&self, user_id: i64, id: i64) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM api_tokens WHERE user_id = ? AND id = ?").bind(user_id).bind(id).execute(&self.pool).await?.rows_affected() > 0)
    }
}

/// Token monouso per i link via email (inviti, reset): in chiaro solo nel link.
pub fn link_token() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 43)
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
    async fn sessions_are_listed_and_revoked_only_by_their_owner() {
        let a = auth().await;
        let (alice, pw) = a.create_user("a@x.it", "A", false).await.unwrap();
        let (bob, pw_b) = a.create_user("b@x.it", "B", false).await.unwrap();
        let (here, _) = a.login("a@x.it", &pw, "1.1.1.1", "Firefox", 100).await.unwrap();
        let (there, _) = a.login("a@x.it", &pw, "2.2.2.2", "Telefono", 200).await.unwrap();
        a.login("b@x.it", &pw_b, "3.3.3.3", "x", 200).await.unwrap();
        let list = a.sessions_of(alice, Some(&here), 300).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].ip.as_str(), list[0].current), ("2.2.2.2", false));
        assert!(list[1].current);
        assert!(!a.revoke_session(bob, &list[0].id).await.unwrap(), "non è sua");
        assert!(a.revoke_session(alice, &list[0].id).await.unwrap());
        assert!(a.session(&there, 300).await.is_none());
        a.login("a@x.it", &pw, "4.4.4.4", "y", 300).await.unwrap();
        assert_eq!(a.revoke_other_sessions(alice, &here).await.unwrap(), 1);
        assert!(a.session(&here, 300).await.is_some());
        assert_eq!(a.sessions_of(bob, None, 300).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn api_tokens_are_listed_without_secret_and_revocable() {
        let a = auth().await;
        let (alice, _) = a.create_user("a@x.it", "A", false).await.unwrap();
        let token = a.create_api_token(alice, "portatile", 1000).await.unwrap();
        let list = a.tokens_of(alice).await.unwrap();
        assert_eq!(list[0].name, "portatile");
        assert!(a.revoke_token(alice, list[0].id).await.unwrap());
        assert!(a.token_user(&token, 10).await.is_none());
    }
}
