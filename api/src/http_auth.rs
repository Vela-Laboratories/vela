use crate::auth::{generate_nonce, verify_matches_async};
use crate::rate_limit::RateLimiter;
use dashmap::DashMap;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use types::RuntimeMode;
use types::UserId;

const CHALLENGE_TTL: Duration = Duration::from_secs(5 * 60);
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_CHALLENGES: usize = 10_000;
const MAX_SESSIONS: usize = 10_000;

#[derive(Debug, Clone)]
struct Challenge {
    address: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone)]
struct Session {
    address: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    Capacity,
    RateLimited,
    MissingChallenge,
    ExpiredChallenge,
    InvalidSignature,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationError {
    Missing,
    Invalid,
    Expired,
    WrongAccount,
    Storage,
}

pub struct HttpAuth {
    scope: &'static str,
    shared_required: bool,
    shared_pool: Option<PgPool>,
    challenge_limiter: RateLimiter,
    challenges: DashMap<String, Challenge>,
    sessions: DashMap<[u8; 32], Session>,
}

impl HttpAuth {
    pub fn new(runtime_mode: RuntimeMode, shared_pool: Option<PgPool>) -> Arc<Self> {
        Arc::new(Self {
            scope: runtime_mode.as_str(),
            shared_required: runtime_mode.is_production(),
            shared_pool,
            challenge_limiter: RateLimiter::new(10, 60),
            challenges: DashMap::new(),
            sessions: DashMap::new(),
        })
    }

    pub async fn challenge(&self, user: &UserId) -> Result<(String, String, u64), AuthError> {
        if self.shared_required && self.shared_pool.is_none() {
            return Err(AuthError::Storage);
        }
        if self.shared_pool.is_none() {
            self.prune();
        }
        let address = user.to_hex().to_ascii_lowercase();
        if !self.challenge_limiter.check(&address) {
            return Err(AuthError::RateLimited);
        }
        let nonce = generate_nonce();
        let expires_at_ms = now_ms().saturating_add(duration_ms(CHALLENGE_TTL));
        let message = signing_message(self.scope, &address, &nonce);
        if let Some(pool) = &self.shared_pool {
            prune_shared(pool).await.map_err(|_| AuthError::Storage)?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_http_auth_challenges")
                .fetch_one(pool)
                .await
                .map_err(|_| AuthError::Storage)?;
            if count >= MAX_CHALLENGES as i64 {
                return Err(AuthError::Capacity);
            }
            sqlx::query(
                "INSERT INTO api_http_auth_challenges (nonce, scope, address, expires_at_ms) VALUES ($1, $2, $3, $4)",
            )
            .bind(&nonce)
            .bind(self.scope)
            .bind(&address)
            .bind(expires_at_ms as i64)
            .execute(pool)
            .await
            .map_err(|_| AuthError::Storage)?;
        } else {
            if self.challenges.len() >= MAX_CHALLENGES {
                return Err(AuthError::Capacity);
            }
            self.challenges.insert(
                nonce.clone(),
                Challenge {
                    address,
                    expires_at_ms,
                },
            );
        }
        Ok((nonce, message, expires_at_ms))
    }

    pub async fn create_session(
        &self,
        user: &UserId,
        nonce: &str,
        signature: &str,
    ) -> Result<(String, u64), AuthError> {
        if self.shared_required && self.shared_pool.is_none() {
            return Err(AuthError::Storage);
        }
        if self.shared_pool.is_none() {
            self.prune();
        }
        let address = user.to_hex().to_ascii_lowercase();
        let challenge = if let Some(pool) = &self.shared_pool {
            let consumed = sqlx::query_as::<_, (String, i64)>(
                "DELETE FROM api_http_auth_challenges WHERE nonce = $1 AND scope = $2 RETURNING address, expires_at_ms",
            )
            .bind(nonce)
            .bind(self.scope)
            .fetch_optional(pool)
            .await
            .map_err(|_| AuthError::Storage)?;
            let Some((challenge_address, expires_at_ms)) = consumed else {
                return Err(AuthError::MissingChallenge);
            };
            Challenge {
                address: challenge_address,
                expires_at_ms: expires_at_ms as u64,
            }
        } else {
            let Some((_, challenge)) = self.challenges.remove(nonce) else {
                return Err(AuthError::MissingChallenge);
            };
            challenge
        };
        if challenge.expires_at_ms <= now_ms() {
            return Err(AuthError::ExpiredChallenge);
        }
        if challenge.address != address {
            return Err(AuthError::InvalidSignature);
        }
        let message = signing_message(self.scope, &address, nonce);
        verify_matches_async(message.into_bytes(), signature.to_owned(), address.clone())
            .await
            .map_err(|_| AuthError::InvalidSignature)?;
        let mut token_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let token = hex::encode(token_bytes);
        let token_hash = hash_token(&token);
        let expires_at_ms = now_ms().saturating_add(duration_ms(SESSION_TTL));
        if let Some(pool) = &self.shared_pool {
            prune_shared(pool).await.map_err(|_| AuthError::Storage)?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_http_auth_sessions")
                .fetch_one(pool)
                .await
                .map_err(|_| AuthError::Storage)?;
            if count >= MAX_SESSIONS as i64 {
                return Err(AuthError::Capacity);
            }
            sqlx::query(
                "INSERT INTO api_http_auth_sessions (token_hash, scope, address, expires_at_ms) VALUES ($1, $2, $3, $4)",
            )
            .bind(token_hash.as_slice())
            .bind(self.scope)
            .bind(&address)
            .bind(expires_at_ms as i64)
            .execute(pool)
            .await
            .map_err(|_| AuthError::Storage)?;
        } else {
            if self.sessions.len() >= MAX_SESSIONS {
                return Err(AuthError::Capacity);
            }
            self.sessions.insert(
                token_hash,
                Session {
                    address,
                    expires_at_ms,
                },
            );
        }
        Ok((token, expires_at_ms))
    }

    pub async fn authorize(
        &self,
        authorization: Option<&str>,
        user: &UserId,
    ) -> Result<(), AuthorizationError> {
        if self.shared_required && self.shared_pool.is_none() {
            return Err(AuthorizationError::Storage);
        }
        let token = bearer_token(authorization.ok_or(AuthorizationError::Missing)?)
            .ok_or(AuthorizationError::Invalid)?;
        let token_hash = hash_token(token);
        if let Some(pool) = &self.shared_pool {
            let session = sqlx::query_as::<_, (String, i64, Option<i64>)>(
                "SELECT address, expires_at_ms, revoked_at_ms FROM api_http_auth_sessions WHERE token_hash = $1 AND scope = $2",
            )
            .bind(token_hash.as_slice())
            .bind(self.scope)
            .fetch_optional(pool)
            .await
            .map_err(|_| AuthorizationError::Storage)?
            .ok_or(AuthorizationError::Invalid)?;
            if session.2.is_some() {
                return Err(AuthorizationError::Invalid);
            }
            if session.1 <= now_ms() as i64 {
                return Err(AuthorizationError::Expired);
            }
            if session.0 != user.to_hex().to_ascii_lowercase() {
                return Err(AuthorizationError::WrongAccount);
            }
            return Ok(());
        }
        let Some(session) = self.sessions.get(&token_hash) else {
            return Err(AuthorizationError::Invalid);
        };
        if session.expires_at_ms <= now_ms() {
            drop(session);
            self.sessions.remove(&token_hash);
            return Err(AuthorizationError::Expired);
        }
        if session.address != user.to_hex().to_ascii_lowercase() {
            return Err(AuthorizationError::WrongAccount);
        }
        Ok(())
    }

    pub async fn revoke(&self, authorization: Option<&str>) -> Result<(), AuthorizationError> {
        if self.shared_required && self.shared_pool.is_none() {
            return Err(AuthorizationError::Storage);
        }
        let token = bearer_token(authorization.ok_or(AuthorizationError::Missing)?)
            .ok_or(AuthorizationError::Invalid)?;
        let token_hash = hash_token(token);
        if let Some(pool) = &self.shared_pool {
            let result = sqlx::query(
                "UPDATE api_http_auth_sessions SET revoked_at_ms = $3 WHERE token_hash = $1 AND scope = $2 AND revoked_at_ms IS NULL AND expires_at_ms > $3",
            )
            .bind(token_hash.as_slice())
            .bind(self.scope)
            .bind(now_ms() as i64)
            .execute(pool)
            .await
            .map_err(|_| AuthorizationError::Storage)?;
            if result.rows_affected() != 1 {
                return Err(AuthorizationError::Invalid);
            }
        } else if self.sessions.remove(&token_hash).is_none() {
            return Err(AuthorizationError::Invalid);
        }
        Ok(())
    }

    fn prune(&self) {
        let now = now_ms();
        self.challenges
            .retain(|_, challenge| challenge.expires_at_ms > now);
        self.sessions
            .retain(|_, session| session.expires_at_ms > now);
    }
}

async fn prune_shared(pool: &PgPool) -> Result<(), sqlx::Error> {
    let now = now_ms() as i64;
    sqlx::query("DELETE FROM api_http_auth_challenges WHERE expires_at_ms <= $1")
        .bind(now)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM api_http_auth_sessions WHERE expires_at_ms <= $1")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn signing_message(scope: &str, address: &str, nonce: &str) -> String {
    format!(
        "vela:http-auth:v1:{}:{}:{}",
        scope.to_ascii_lowercase(),
        address.to_ascii_lowercase(),
        nonce
    )
}

fn bearer_token(value: &str) -> Option<&str> {
    let token = value.strip_prefix("Bearer ")?;
    (token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(token)
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_tokens_are_strictly_parsed() {
        let token = "ab".repeat(32);
        assert_eq!(
            bearer_token(&format!("Bearer {token}")),
            Some(token.as_str())
        );
        assert!(bearer_token(&format!("bearer {token}")).is_none());
        assert!(bearer_token("Bearer short").is_none());
    }

    #[test]
    fn signing_message_is_versioned_and_account_bound() {
        assert_eq!(
            signing_message("Production", "0xAbC", "nonce"),
            "vela:http-auth:v1:production:0xabc:nonce"
        );
    }

    #[tokio::test]
    async fn production_without_shared_store_fails_closed() {
        let auth = HttpAuth::new(RuntimeMode::Production, None);
        let user = UserId::from_hex("0x1111111111111111111111111111111111111111").unwrap();
        assert_eq!(auth.challenge(&user).await, Err(AuthError::Storage));
        assert_eq!(
            auth.authorize(None, &user).await,
            Err(AuthorizationError::Storage)
        );
    }
}
