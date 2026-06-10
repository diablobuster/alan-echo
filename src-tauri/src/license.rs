//! ALAN Echo — license key validation.
//!
//! Keys are HMAC-SHA256 checksummed and validate fully offline. A key works on
//! any machine: reinstalls, new laptops, and hardware changes never lock a
//! customer out (there is no activation server, so a hardware-bound key could
//! never be reset remotely). Sharing is deterred by the server-side issuance
//! record on alanglobalintelligence.com, not by machine binding.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

const PREFIX: &str = "ECHO";
/// Accepted HMAC secrets, newest first. The v1 secret is PERMANENT for all of
/// 1.x — rotating it would brick every sold key (see docs/V1.1-REBUILD-BATCH.md
/// runbook). If a v2 secret is ever needed, APPEND it here so old keys keep
/// validating; minting (server-side) switches to the newest entry.
const SECRETS: &[&[u8]] = &[b"ALAN_ECHO_v1_GLOBAL_INTELLIGENCE_2026"];
const CHARSET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

#[derive(Debug, Serialize, Deserialize)]
pub struct LicenseManager {
    key: Option<String>,
}

impl LicenseManager {
    pub fn new(key: Option<String>) -> Self {
        Self { key }
    }

    /// Check if the current key is valid.
    pub fn is_licensed(&self) -> bool {
        match &self.key {
            Some(k) => self.validate_key(k),
            None => false,
        }
    }

    /// Validate key format and HMAC checksum.
    pub fn validate_key(&self, key: &str) -> bool {
        let key = key.trim().to_uppercase().replace(' ', "");
        if !key.starts_with(&format!("{}-", PREFIX)) {
            return false;
        }
        let rest = &key[PREFIX.len() + 1..];
        let parts: Vec<&str> = rest.split('-').collect();
        if parts.len() != 4 || !parts.iter().all(|p| p.len() == 5) {
            return false;
        }
        // Validate all chars are in CHARSET
        for part in &parts {
            if !part.bytes().all(|b| CHARSET.contains(&b)) {
                return false;
            }
        }
        let payload = format!("{}-{}-{}", parts[0], parts[1], parts[2]);
        SECRETS.iter().any(|secret| {
            let expected = compute_check_with(secret, &payload);
            constant_time_eq(parts[3].as_bytes(), expected.as_bytes())
        })
    }

    /// Activate a key: validate format and checksum, then store it.
    pub fn activate(&mut self, key: &str) -> (bool, String) {
        let key = key.trim().to_uppercase().replace(' ', "");

        if !self.validate_key(&key) {
            return (false, "Invalid license key — check for typos and try again".into());
        }

        self.key = Some(key);
        (true, "License activated successfully".into())
    }

    pub fn deactivate(&mut self) {
        self.key = None;
    }

    /// Masked key for display.
    pub fn display_key(&self) -> Option<String> {
        self.key.as_ref().map(|k| {
            let parts: Vec<&str> = k.split('-').collect();
            if parts.len() >= 2 {
                format!("{}-{}-•••••-•••••-•••••", parts[0], parts[1])
            } else {
                "•••••".into()
            }
        })
    }
}

/// Checksum against the newest (minting) secret — used by tests and the
/// debug-only generator.
#[cfg(any(test, debug_assertions))]
fn compute_check(payload: &str) -> String {
    compute_check_with(SECRETS[0], payload)
}

fn compute_check_with(secret: &[u8], payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).unwrap();
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let bytes = result.into_bytes();
    let mut check = String::with_capacity(5);
    for &b in bytes.iter().take(5) {
        check.push(CHARSET[(b as usize) % CHARSET.len()] as char);
    }
    check
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Generate a valid license key (for sales backend / CLI tool).
/// Only available in debug builds — stripped from release binary.
#[cfg(debug_assertions)]
#[allow(dead_code)]
pub fn generate_key() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let segments: Vec<String> = (0..3)
        .map(|_| {
            (0..5)
                .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
                .collect()
        })
        .collect();
    let payload = segments.join("-");
    let check = compute_check(&payload);
    format!("{}-{}-{}", PREFIX, payload, check)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors computed with the Python keygen — proves checksum parity:
    /// hmac.new(b'ALAN_ECHO_v1_GLOBAL_INTELLIGENCE_2026', payload, sha256),
    /// CHARSET[b % 31] over the first 5 digest bytes.
    #[test]
    fn check_matches_python_keygen() {
        assert_eq!(compute_check("AAAAA-BBBBB-CCCCC"), "FEMJB");
        assert_eq!(compute_check("M7K2P-QRST3-UVWX9"), "GA6NJ");
    }

    #[test]
    fn python_generated_key_validates() {
        let lm = LicenseManager::new(None);
        assert!(lm.validate_key("ECHO-AAAAA-BBBBB-CCCCC-FEMJB"));
        assert!(lm.validate_key("echo-aaaaa-bbbbb-ccccc-femjb")); // case-insensitive
        assert!(!lm.validate_key("ECHO-AAAAA-BBBBB-CCCCC-FEMJA")); // bad checksum
        assert!(!lm.validate_key("ECHO-AAAAA-BBBBB-CCCCC")); // missing segment
        assert!(!lm.validate_key("NOPE-AAAAA-BBBBB-CCCCC-FEMJB")); // wrong prefix
    }

    #[test]
    fn key_works_after_activation_and_on_restore() {
        let mut lm = LicenseManager::new(None);
        let (ok, _) = lm.activate("ECHO-AAAAA-BBBBB-CCCCC-FEMJB");
        assert!(ok);
        assert!(lm.is_licensed());

        // A manager rebuilt from persisted settings (reinstall, new machine —
        // there is deliberately no hardware binding) stays licensed.
        let restored = LicenseManager::new(Some("ECHO-AAAAA-BBBBB-CCCCC-FEMJB".into()));
        assert!(restored.is_licensed());

        // A tampered persisted key does not validate.
        let tampered = LicenseManager::new(Some("ECHO-AAAAA-BBBBB-CCCCC-FEMJA".into()));
        assert!(!tampered.is_licensed());
    }

    #[test]
    fn generated_keys_validate() {
        let lm = LicenseManager::new(None);
        for _ in 0..50 {
            let key = generate_key();
            assert!(lm.validate_key(&key), "generated key failed: {}", key);
        }
    }
}
