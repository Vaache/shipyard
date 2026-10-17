use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::{secret, settings};

pub const MIN_PIN: usize = 4;
pub const MAX_PIN: usize = 6;
pub const FREE_ATTEMPTS: u32 = 5;
pub const MAX_ATTEMPTS: u32 = 10;
pub const BASE_DELAY: i64 = 60;
pub const MAX_DELAY: i64 = 15 * 60;
pub const ITERATIONS: u32 = 200_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LockRecord {
    pub salt: String,
    pub hash: String,
    pub iterations: u32,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub locked_until: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinCheck {
    Ok,
    Wrong(u32),
    Wait(i64),
    Blocked,
}

pub fn is_valid_pin(pin: &str) -> bool {
    (MIN_PIN..=MAX_PIN).contains(&pin.chars().count()) && pin.chars().all(|c| c.is_ascii_digit())
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn derive(pin: &str, salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    if openssl::pkcs5::pbkdf2_hmac(pin.as_bytes(), salt, iterations as usize, openssl::hash::MessageDigest::sha256(), &mut out).is_err() {
        out.clear();
    }
    out
}

pub fn new_record(pin: &str, iterations: u32) -> LockRecord {
    let mut salt = [0u8; 16];
    if openssl::rand::rand_bytes(&mut salt).is_err() {
        salt = *uuid::Uuid::new_v4().as_bytes();
    }
    LockRecord { salt: b64().encode(salt), hash: b64().encode(derive(pin, &salt, iterations)), iterations, failures: 0, locked_until: None }
}

pub fn state(r: &LockRecord, now: i64) -> Option<PinCheck> {
    if r.failures >= MAX_ATTEMPTS {
        return Some(PinCheck::Blocked);
    }
    r.locked_until.filter(|until| *until > now).map(PinCheck::Wait)
}

pub fn verify(r: &mut LockRecord, pin: &str, now: i64) -> PinCheck {
    if let Some(s) = state(r, now) {
        return s;
    }
    let salt = b64().decode(&r.salt).unwrap_or_default();
    let expected = b64().decode(&r.hash).unwrap_or_default();
    let actual = derive(pin, &salt, r.iterations);
    if !expected.is_empty() && actual.len() == expected.len() && openssl::memcmp::eq(&actual, &expected) {
        r.failures = 0;
        r.locked_until = None;
        return PinCheck::Ok;
    }
    r.failures += 1;
    if r.failures >= MAX_ATTEMPTS {
        return PinCheck::Blocked;
    }
    if r.failures >= FREE_ATTEMPTS {
        let delay = (BASE_DELAY << (r.failures - FREE_ATTEMPTS).min(10)).min(MAX_DELAY);
        let until = now + delay;
        r.locked_until = Some(until);
        return PinCheck::Wait(until);
    }
    PinCheck::Wrong(FREE_ATTEMPTS - r.failures)
}

fn key(account_id: &str) -> String {
    format!("lock-{account_id}")
}

pub async fn load(account_id: &str) -> Option<LockRecord> {
    let raw = crate::rt::run(secret::load(settings::config_dir(), key(account_id))).await?;
    serde_json::from_str(&raw).ok()
}

async fn save(account_id: &str, record: &LockRecord) -> bool {
    let Ok(raw) = serde_json::to_string(record) else { return false };
    crate::rt::run(secret::save(settings::config_dir(), key(account_id), raw)).await != secret::Saved::Failed
}

pub async fn is_protected(account_id: &str) -> bool {
    load(account_id).await.is_some()
}

pub async fn current_state(account_id: &str) -> Option<PinCheck> {
    let r = load(account_id).await?;
    state(&r, chrono::Utc::now().timestamp())
}

pub async fn set_pin(account_id: &str, pin: String) -> bool {
    let record = crate::rt::blocking(move || new_record(&pin, ITERATIONS)).await;
    save(account_id, &record).await
}

pub async fn remove(account_id: &str) {
    crate::rt::run(secret::delete(settings::config_dir(), key(account_id))).await;
}

pub async fn check(account_id: &str, pin: String) -> PinCheck {
    let Some(mut record) = load(account_id).await else { return PinCheck::Ok };
    let now = chrono::Utc::now().timestamp();
    let (result, record) = crate::rt::blocking(move || {
        let result = verify(&mut record, &pin, now);
        (result, record)
    })
    .await;
    save(account_id, &record).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: u32 = 1_000;

    #[test]
    fn validates_pin_format() {
        assert!(is_valid_pin("1234"));
        assert!(is_valid_pin("123456"));
        assert!(!is_valid_pin("123"));
        assert!(!is_valid_pin("1234567"));
        assert!(!is_valid_pin("12a4"));
        assert!(!is_valid_pin("١٢٣٤"));
    }

    #[test]
    fn stores_hash_not_pin() {
        let r = new_record("1234", FAST);
        let hash = b64().decode(&r.hash).unwrap();
        assert_eq!(hash.len(), 32);
        assert_ne!(hash, b"1234".to_vec());
        assert!(!r.hash.contains("1234"));
        assert_eq!(b64().decode(&r.salt).unwrap().len(), 16);
        assert_ne!(new_record("1234", FAST).salt, r.salt);
    }

    #[test]
    fn correct_pin_unlocks_and_resets_failures() {
        let mut r = new_record("1234", FAST);
        assert_eq!(verify(&mut r, "0000", 0), PinCheck::Wrong(4));
        assert_eq!(verify(&mut r, "1234", 0), PinCheck::Ok);
        assert_eq!(r.failures, 0);
    }

    #[test]
    fn pauses_after_five_failures_and_grows() {
        let mut r = new_record("1234", FAST);
        let clock = 1_000_000;
        for _ in 0..4 {
            verify(&mut r, "0000", clock);
        }
        assert_eq!(verify(&mut r, "0000", clock), PinCheck::Wait(clock + 60));
        assert_eq!(verify(&mut r, "1234", clock), PinCheck::Wait(clock + 60));
        let later = clock + 61;
        assert_eq!(verify(&mut r, "0000", later), PinCheck::Wait(later + 120));
    }

    #[test]
    fn pause_is_capped() {
        let mut r = new_record("1234", FAST);
        r.failures = 8;
        assert_eq!(verify(&mut r, "0000", 0), PinCheck::Wait(MAX_DELAY));
    }

    #[test]
    fn blocks_after_ten_failures() {
        let mut r = new_record("1234", FAST);
        let mut clock = 0;
        for _ in 0..10 {
            verify(&mut r, "0000", clock);
            clock += 3600;
        }
        assert_eq!(verify(&mut r, "1234", clock), PinCheck::Blocked);
        assert_eq!(state(&r, clock), Some(PinCheck::Blocked));
    }

    #[test]
    fn accounts_are_separate() {
        let mut a = new_record("1111", FAST);
        let mut b = new_record("2222", FAST);
        assert_ne!(verify(&mut b, "1111", 0), PinCheck::Ok);
        assert_eq!(verify(&mut b, "2222", 0), PinCheck::Ok);
        assert_eq!(verify(&mut a, "1111", 0), PinCheck::Ok);
        assert_eq!(key("a"), "lock-a");
    }

    #[test]
    fn record_round_trips() {
        let r = new_record("4321", FAST);
        let back: LockRecord = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
