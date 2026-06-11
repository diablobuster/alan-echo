//! ALAN Echo — license key format validation.
//!
//! Keys are validated for FORMAT only (prefix, groups, charset). Cryptographic
//! validation happens server-side via Ed25519 activation (see activation.rs).
//! No secrets are embedded in this binary.

use serde::{Deserialize, Serialize};

const PREFIX: &str = "ECHO";
const CHARSET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

#[derive(Debug, Serialize, Deserialize)]
pub struct LicenseManager {
    key: Option<String>,
}

impl LicenseManager {
    pub fn new(key: Option<String>) -> Self {
        Self { key }
    }

    /// Format-only check — cryptographic validation is via Ed25519 activation.
    pub fn is_licensed(&self) -> bool {
        false
    }

    /// Validate key format: ECHO-XXXXX-XXXXX-XXXXX-XXXXX with valid charset.
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
        for part in &parts {
            if !part.bytes().all(|b| CHARSET.contains(&b)) {
                return false;
            }
        }
        true
    }

    /// Store a format-valid key for use with online activation.
    pub fn activate(&mut self, key: &str) -> (bool, String) {
        let key = key.trim().to_uppercase().replace(' ', "");

        if !self.validate_key(&key) {
            return (false, "Invalid license key — check for typos and try again".into());
        }

        self.key = Some(key);
        (true, "Key accepted — activating...".into())
    }

    pub fn deactivate(&mut self) {
        self.key = None;
    }

    pub fn stored_key(&self) -> Option<&str> {
        self.key.as_deref()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_format_accepted() {
        let lm = LicenseManager::new(None);
        assert!(lm.validate_key("ECHO-ABCDE-FGHJK-MNPQR-STUVW"));
        assert!(lm.validate_key("echo-abcde-fghjk-mnpqr-stuvw")); // case-insensitive
    }

    #[test]
    fn bad_format_rejected() {
        let lm = LicenseManager::new(None);
        assert!(!lm.validate_key("ECHO-AAAAA-BBBBB-CCCCC")); // missing segment
        assert!(!lm.validate_key("NOPE-AAAAA-BBBBB-CCCCC-DDDDD")); // wrong prefix
        assert!(!lm.validate_key("ECHO-AAAA0-BBBBB-CCCCC-DDDDD")); // 0 not in charset
        assert!(!lm.validate_key("ECHO-AAAAL-BBBBB-CCCCC-DDDDD")); // L not in charset
    }

    #[test]
    fn activate_stores_key() {
        let mut lm = LicenseManager::new(None);
        let (ok, _) = lm.activate("ECHO-ABCDE-FGHJK-MNPQR-STUVW");
        assert!(ok);
        assert_eq!(lm.stored_key(), Some("ECHO-ABCDE-FGHJK-MNPQR-STUVW"));
        assert!(!lm.is_licensed()); // is_licensed always false — Ed25519 required
    }
}
