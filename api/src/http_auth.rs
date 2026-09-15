use crate::auth::{generate_nonce, verify_matches_async};
use crate::rate_limit::RateLimiter;
use dashmap::DashMap;
use rand::RngCore;
use sha2::{Digest, Sha256};
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationError {
    Missing,
    Invalid,
    Expired,
    WrongAccount,
}

pub struct HttpAuth {
    scope: &'static str,
    challenge_limiter: RateLimiter,
    challenges: DashMap<String, Challenge>,
    sessions: DashMap<[u8; 32], Session>,
}

impl HttpAuth {
    pub fn new(runtime_mode: RuntimeMode) -> Arc<Self> {
        Arc::new(Self {
            scope: runtime_mode.as_str(),
            challenge_limiter: RateLimiter::new(10, 60),
            challenges: DashMap::new(),
            sessions: DashMap::new(),
        })
    }

    pub fn challenge(&self, user: &UserId) -> Result<(String, String, u64), AuthError> {
        self.prune();
        let address = user.to_hex().to_ascii_lowercase();
        if !self.challenge_limiter.check(&address) {
            return Err(AuthError::RateLimited);
        }
        if self.challenges.len() >= MAX_CHALLENGES {
            return Err(AuthError::Capacity);
        }
        let nonce = generate_nonce();
        let expires_at_ms = now_ms().saturating_add(duration_ms(CHALLENGE_TTL));
        let message = signing_message(self.scope, &address, &nonce);
        self.challenges.insert(
            nonce.clone(),
            Challenge {
                address,
                expires_at_ms,
            },
        );
        Ok((nonce, message, expires_at_ms))
    }

    pub async fn create_session(
        &self,
        user: &UserId,
        nonce: &str,
        signature: &str,
    ) -> Result<(String, u64), AuthError> {
        self.prune();
        let address = user.to_hex().to_ascii_lowercase();
        let Some((_, challenge)) = self.challenges.remove(nonce) else {
            return Err(AuthError::MissingChallenge);
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
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(AuthError::Capacity);
        }

        let mut token_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let token = hex::encode(token_bytes);
        let token_hash = hash_token(&token);
        let expires_at_ms = now_ms().saturating_add(duration_ms(SESSION_TTL));
        self.sessions.insert(
            token_hash,
            Session {
                address,
                expires_at_ms,
            },
        );
        Ok((token, expires_at_ms))
    }

    pub fn authorize(
        &self,
        authorization: Option<&str>,
        user: &UserId,
    ) -> Result<(), AuthorizationError> {
        let token = bearer_token(authorization.ok_or(AuthorizationError::Missing)?)
            .ok_or(AuthorizationError::Invalid)?;
        let token_hash = hash_token(token);
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

    pub fn revoke(&self, authorization: Option<&str>) -> Result<(), AuthorizationError> {
        let token = bearer_token(authorization.ok_or(AuthorizationError::Missing)?)
            .ok_or(AuthorizationError::Invalid)?;
        if self.sessions.remove(&hash_token(token)).is_none() {
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
}
