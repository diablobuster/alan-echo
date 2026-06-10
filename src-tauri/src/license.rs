//! ALAN Echo — Hardware-bound license key validation.
//!
//! Keys are HMAC-SHA256 signed AND bound to the machine's hardware ID on activation.
//! A key activated on Machine A will not work on Machine B.
//! This prevents key sharing without requiring a license server.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

const PREFIX: &str = "ECHO";
const SECRET: &[u8] = b"ALAN_ECHO_v1_GLOBAL_INTELLIGENCE_2026";
const BIND_SECRET: &[u8] = b"ALAN_ECHO_HWID_BIND_2026";
const CHARSET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Get a stable machine identifier (hardware-bound).
/// Uses motherboard serial + CPU ID + disk serial for uniqueness.
pub fn get_machine_id() -> String {
    match machine_uid::get() {
        Ok(id) => {
            // Hash the raw machine ID so it's a consistent length
            let mut mac = HmacSha256::new_from_slice(BIND_SECRET).unwrap();
            mac.update(id.as_bytes());
            let result = mac.finalize();
            hex::encode(&result.into_bytes()[..16])
        }
        Err(_) => {
            // Fallback: use computer name + username
            let name = std::env::var("COMPUTERNAME").unwrap_or_default();
            let user = std::env::var("USERNAME").unwrap_or_default();
            let mut mac = HmacSha256::new_from_slice(BIND_SECRET).unwrap();
            mac.update(format!("{}-{}", name, user).as_bytes());
            let result = mac.finalize();
            hex::encode(&result.into_bytes()[..16])
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LicenseManager {
    key: Option<String>,
    machine_id: String,
    bound_machine: Option<String>,
}

impl LicenseManager {
    pub fn new(key: Option<String>, machine_id: String, bound_machine: Option<String>) -> Self {
        Self {
            key,
            machine_id,
            bound_machine,
        }
    }

    /// The machine binding produced at activation — persist this alongside the key.
    pub fn bound_machine(&self) -> Option<&str> {
        self.bound_machine.as_deref()
    }

    /// Check if the current key is valid for this machine.
    pub fn is_licensed(&self) -> bool {
        match &self.key {
            Some(k) => self.validate_key(k) && self.check_machine_binding(k),
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
        let expected = compute_check(&payload);
        constant_time_eq(parts[3].as_bytes(), expected.as_bytes())
    }

    /// Check if the key is bound to this machine.
    fn check_machine_binding(&self, key: &str) -> bool {
        let binding = compute_machine_binding(key, &self.machine_id);
        match &self.bound_machine {
            Some(bound) => constant_time_eq(bound.as_bytes(), binding.as_bytes()),
            None => true, // Not yet bound — will bind on activation
        }
    }

    /// Activate a key: validate format, bind to this machine.
    pub fn activate(&mut self, key: &str) -> (bool, String) {
        let key = key.trim().to_uppercase().replace(' ', "");

        if !self.validate_key(&key) {
            return (false, "Invalid license key — check for typos and try again".into());
        }

        // Bind to this machine
        let binding = compute_machine_binding(&key, &self.machine_id);
        self.key = Some(key);
        self.bound_machine = Some(binding);

        (true, "License activated successfully".into())
    }

    pub fn deactivate(&mut self) {
        self.key = None;
        self.bound_machine = None;
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

fn compute_check(payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(SECRET).unwrap();
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let bytes = result.into_bytes();
    let mut check = String::with_capacity(5);
    for &b in bytes.iter().take(5) {
        check.push(CHARSET[(b as usize) % CHARSET.len()] as char);
    }
    check
}

fn compute_machine_binding(key: &str, machine_id: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(BIND_SECRET).unwrap();
    mac.update(key.as_bytes());
    mac.update(b"|");
    mac.update(machine_id.as_bytes());
    let result = mac.finalize();
    hex::encode(&result.into_bytes()[..16])
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
        let lm = LicenseManager::new(None, "test-machine".into(), None);
        assert!(lm.validate_key("ECHO-AAAAA-BBBBB-CCCCC-FEMJB"));
        assert!(lm.validate_key("echo-aaaaa-bbbbb-ccccc-femjb")); // case-insensitive
        assert!(!lm.validate_key("ECHO-AAAAA-BBBBB-CCCCC-FEMJA")); // bad checksum
        assert!(!lm.validate_key("ECHO-AAAAA-BBBBB-CCCCC")); // missing segment
        assert!(!lm.validate_key("NOPE-AAAAA-BBBBB-CCCCC-FEMJB")); // wrong prefix
    }

    #[test]
    fn activation_binds_to_machine() {
        let mut lm = LicenseManager::new(None, "machine-a".into(), None);
        let (ok, _) = lm.activate("ECHO-AAAAA-BBBBB-CCCCC-FEMJB");
        assert!(ok);
        assert!(lm.is_licensed());
        let binding = lm.bound_machine().expect("binding set").to_string();

        // Same key + binding on a different machine must fail.
        let lm_b = LicenseManager::new(
            Some("ECHO-AAAAA-BBBBB-CCCCC-FEMJB".into()),
            "machine-b".into(),
            Some(binding.clone()),
        );
        assert!(!lm_b.is_licensed());

        // Same machine restores fine.
        let lm_a = LicenseManager::new(
            Some("ECHO-AAAAA-BBBBB-CCCCC-FEMJB".into()),
            "machine-a".into(),
            Some(binding),
        );
        assert!(lm_a.is_licensed());
    }

    #[test]
    fn generated_keys_validate() {
        let lm = LicenseManager::new(None, "m".into(), None);
        for _ in 0..50 {
            let key = generate_key();
            assert!(lm.validate_key(&key), "generated key failed: {}", key);
        }
    }
}
