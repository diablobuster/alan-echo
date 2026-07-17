use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::activation;
use crate::settings::Settings;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
// Suppress the console window Windows would otherwise flash when `reg` runs
// (e.g. on every dictation when the trial counter is persisted).
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

type HmacSha256 = Hmac<Sha256>;

pub const DAILY_LIMIT: u32 = 5;
pub const LIFETIME_CAP: u32 = 50;

pub const SETTINGS_KEY: &str = "trial_state";
const SECONDARY_FILE: &str = ".trial";

const K0: [u8; 8] = [0x0e, 0x19, 0xc6, 0xbb, 0xf9, 0x43, 0x3a, 0xb3];
const K1: [u8; 8] = [0x12, 0x4c, 0x6a, 0x9d, 0x4e, 0x13, 0x26, 0x4a];
const K2: [u8; 8] = [0x9b, 0x44, 0x55, 0x25, 0x98, 0x14, 0xb7, 0xf1];
const K3: [u8; 8] = [0xec, 0x78, 0xf5, 0x55, 0x46, 0x02, 0x67, 0x19];

fn base_secret() -> [u8; 32] {
    let mut s = [0u8; 32];
    s[..8].copy_from_slice(&K0);
    s[8..16].copy_from_slice(&K1);
    s[16..24].copy_from_slice(&K2);
    s[24..32].copy_from_slice(&K3);
    s
}

fn derive_key(machine_hash: &str) -> Vec<u8> {
    let secret = base_secret();
    let mut key_material = secret.to_vec();
    key_material.extend_from_slice(machine_hash.as_bytes());
    Sha256::digest(&key_material).to_vec()
}

#[derive(Clone, Debug)]
pub struct TrialState {
    pub date: String,
    pub count: u32,
    pub max_date_seen: String,
    pub lifetime_total: u32,
    pub machine_hash: String,
}

#[derive(Debug, PartialEq)]
pub enum TrialStatus {
    Allowed,
    DailyLimitReached,
    LifetimeExpired,
}

impl TrialState {
    pub fn new_fresh(machine_hash: &str) -> Self {
        let today = today_str();
        Self {
            date: today.clone(),
            count: 0,
            max_date_seen: today,
            lifetime_total: 0,
            machine_hash: machine_hash.to_string(),
        }
    }

    fn to_json(&self) -> String {
        serde_json::json!({
            "d": self.date,
            "c": self.count,
            "m": self.max_date_seen,
            "lt": self.lifetime_total,
            "mh": self.machine_hash,
        })
        .to_string()
    }

    fn from_json(s: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        Some(Self {
            date: v.get("d")?.as_str()?.to_string(),
            count: v.get("c")?.as_u64()? as u32,
            max_date_seen: v.get("m")?.as_str()?.to_string(),
            lifetime_total: v.get("lt")?.as_u64()? as u32,
            machine_hash: v.get("mh")?.as_str()?.to_string(),
        })
    }

    fn sign(&self) -> String {
        let json = self.to_json();
        let key = derive_key(&self.machine_hash);
        let mut mac =
            HmacSha256::new_from_slice(&key).expect("HMAC accepts any key size");
        mac.update(json.as_bytes());
        let sig = mac.finalize().into_bytes();
        format!("{}.{}", B64.encode(json.as_bytes()), B64.encode(sig))
    }

    fn verify(blob: &str, current_machine: &str) -> Option<Self> {
        let parts: Vec<&str> = blob.splitn(2, '.').collect();
        if parts.len() != 2 {
            return None;
        }
        let json_bytes = B64.decode(parts[0]).ok()?;
        let sig_bytes = B64.decode(parts[1]).ok()?;
        let json_str = String::from_utf8(json_bytes.clone()).ok()?;
        let state = Self::from_json(&json_str)?;

        let key = derive_key(&state.machine_hash);
        let mut mac = HmacSha256::new_from_slice(&key).ok()?;
        mac.update(&json_bytes);
        mac.verify_slice(&sig_bytes).ok()?;

        if state.machine_hash != current_machine {
            return None;
        }
        Some(state)
    }
}

/// Small backwards clock jumps stay locked (someone gaming the daily reset);
/// anything larger is treated as clock damage, not tampering. Without this
/// grace window, one dictation while the system clock was transiently in the
/// future (dead CMOS battery, timezone mishap) ratcheted max_date_seen ahead
/// and bricked the trial with a false "resets tomorrow" until the real
/// calendar caught up.
const CLOCK_SKEW_GRACE_DAYS: i64 = 3;

fn days_ahead(from: &str, to: &str) -> Option<i64> {
    let f = chrono::NaiveDate::parse_from_str(from, "%Y-%m-%d").ok()?;
    let t = chrono::NaiveDate::parse_from_str(to, "%Y-%m-%d").ok()?;
    Some((t - f).num_days())
}

pub fn check_trial(state: &TrialState) -> TrialStatus {
    if state.lifetime_total >= LIFETIME_CAP {
        return TrialStatus::LifetimeExpired;
    }
    let today = today_str();
    if today < state.max_date_seen {
        let skew = days_ahead(&today, &state.max_date_seen).unwrap_or(i64::MAX);
        if skew <= CLOCK_SKEW_GRACE_DAYS {
            return TrialStatus::DailyLimitReached;
        }
        // max_date_seen is absurdly far ahead — clock anomaly; fall through.
    }
    if state.date == today && state.count >= DAILY_LIMIT {
        return TrialStatus::DailyLimitReached;
    }
    TrialStatus::Allowed
}

pub fn increment(state: &mut TrialState) {
    let today = today_str();
    if state.date != today {
        state.date = today.clone();
        state.count = 0;
    }
    state.count += 1;
    state.lifetime_total += 1;
    if today > state.max_date_seen {
        state.max_date_seen = today;
    } else if days_ahead(&today, &state.max_date_seen).unwrap_or(0) > CLOCK_SKEW_GRACE_DAYS {
        // Heal a ratchet left behind by a transiently-future clock.
        state.max_date_seen = today;
    }
}

pub fn get_status_json(state: &TrialState) -> serde_json::Value {
    let today = today_str();
    let used_today = if state.date == today { state.count } else { 0 };
    let daily_remaining = DAILY_LIMIT.saturating_sub(used_today);
    let lifetime_remaining = LIFETIME_CAP.saturating_sub(state.lifetime_total);

    serde_json::json!({
        "licensed": false,
        "trial": true,
        "used": used_today,
        "limit": DAILY_LIMIT,
        "remaining": daily_remaining,
        "lifetime_used": state.lifetime_total,
        "lifetime_limit": LIFETIME_CAP,
        "lifetime_remaining": lifetime_remaining,
    })
}

// ── Storage ──────────────────────────────────────────────────────────

pub fn load(settings: &Settings, data_dir: &Path) -> TrialState {
    let machine = activation::machine_fingerprint();
    let candidates = read_all_locations(settings, data_dir, &machine);

    if let Some(best) = candidates
        .into_iter()
        .max_by_key(|s| s.lifetime_total)
    {
        return best;
    }

    // Check for old-format migration
    if let Some(migrated) = migrate_old_format(settings, &machine) {
        return migrated;
    }

    TrialState::new_fresh(&machine)
}

pub fn save(state: &TrialState, settings: &mut Settings, data_dir: &Path) {
    let blob = state.sign();
    settings.set(SETTINGS_KEY, serde_json::Value::String(blob.clone()));
    settings.save().ok();

    let secondary = secondary_path(data_dir);
    if let Some(parent) = secondary.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&secondary, &blob).ok();

    #[cfg(target_os = "windows")]
    write_registry(&blob);
}

fn read_all_locations(settings: &Settings, data_dir: &Path, machine: &str) -> Vec<TrialState> {
    let mut results = Vec::new();

    if let Some(blob) = settings.get_str(SETTINGS_KEY) {
        if let Some(state) = TrialState::verify(&blob, machine) {
            results.push(state);
        }
    }

    if let Ok(blob) = std::fs::read_to_string(secondary_path(data_dir)) {
        if let Some(state) = TrialState::verify(blob.trim(), machine) {
            results.push(state);
        }
    }

    #[cfg(target_os = "windows")]
    if let Some(blob) = read_registry() {
        if let Some(state) = TrialState::verify(&blob, machine) {
            results.push(state);
        }
    }

    results
}

fn secondary_path(data_dir: &Path) -> PathBuf {
    let local_app_data = dirs::data_local_dir()
        .unwrap_or_else(|| data_dir.to_path_buf());
    local_app_data.join("com.alan.echo").join(SECONDARY_FILE)
}

fn migrate_old_format(settings: &Settings, machine: &str) -> Option<TrialState> {
    let old_date = settings.get_str("trial_date")?;
    let old_count = settings
        .get("trial_dictations_today")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let today = today_str();
    Some(TrialState {
        date: old_date.clone(),
        count: if old_date == today { old_count } else { 0 },
        max_date_seen: if old_date > today { old_date } else { today },
        lifetime_total: old_count,
        machine_hash: machine.to_string(),
    })
}

pub fn cleanup_old_keys(settings: &mut Settings) {
    settings.delete("trial_date");
    settings.delete("trial_dictations_today");
    settings.save().ok();
}

fn today_str() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ── Windows Registry ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn write_registry(blob: &str) {
    use std::process::{Command, Stdio};
    Command::new("reg")
        .args(["add", r"HKCU\Software\ALAN Echo", "/v", "TrialState", "/t", "REG_SZ", "/d", blob, "/f"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok();
}

#[cfg(target_os = "windows")]
fn read_registry() -> Option<String> {
    use std::process::{Command, Stdio};
    let output = Command::new("reg")
        .args(["query", r"HKCU\Software\ALAN Echo", "/v", "TrialState"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("TrialState") {
            let parts: Vec<&str> = trimmed.splitn(3, "    ").collect();
            if parts.len() == 3 {
                return Some(parts[2].trim().to_string());
            }
        }
    }
    None
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MACHINE: &str = "abc123def456";

    #[test]
    fn sign_verify_roundtrip() {
        let state = TrialState::new_fresh(TEST_MACHINE);
        let blob = state.sign();
        let recovered = TrialState::verify(&blob, TEST_MACHINE);
        assert!(recovered.is_some());
        let r = recovered.unwrap();
        assert_eq!(r.lifetime_total, 0);
        assert_eq!(r.machine_hash, TEST_MACHINE);
    }

    #[test]
    fn tampered_blob_rejected() {
        let state = TrialState::new_fresh(TEST_MACHINE);
        let blob = state.sign();
        let tampered = format!("{}X", blob);
        assert!(TrialState::verify(&tampered, TEST_MACHINE).is_none());
    }

    #[test]
    fn wrong_machine_rejected() {
        let state = TrialState::new_fresh(TEST_MACHINE);
        let blob = state.sign();
        assert!(TrialState::verify(&blob, "different_machine").is_none());
    }

    #[test]
    fn lifetime_cap_enforced() {
        let mut state = TrialState::new_fresh(TEST_MACHINE);
        state.lifetime_total = LIFETIME_CAP;
        assert_eq!(check_trial(&state), TrialStatus::LifetimeExpired);
    }

    #[test]
    fn daily_limit_enforced() {
        let today = today_str();
        let state = TrialState {
            date: today.clone(),
            count: DAILY_LIMIT,
            max_date_seen: today,
            lifetime_total: 10,
            machine_hash: TEST_MACHINE.to_string(),
        };
        assert_eq!(check_trial(&state), TrialStatus::DailyLimitReached);
    }

    #[test]
    fn clock_rollback_detected() {
        // A small rollback (yesterday's clock gaming the daily reset) stays
        // locked: max_date_seen one day ahead of today.
        let tomorrow = (chrono::Local::now().date_naive() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let state = TrialState {
            date: today_str(),
            count: 0,
            max_date_seen: tomorrow,
            lifetime_total: 10,
            machine_hash: TEST_MACHINE.to_string(),
        };
        assert_eq!(check_trial(&state), TrialStatus::DailyLimitReached);
    }

    #[test]
    fn far_future_ratchet_is_clock_damage_not_tamper() {
        // Regression: one dictation under a transiently-future clock (dead
        // CMOS battery) ratcheted max_date_seen years ahead and bricked the
        // trial with a false "resets tomorrow". Beyond the grace window the
        // trial must proceed, and the next increment must heal the ratchet.
        let mut state = TrialState {
            date: "2099-01-01".into(),
            count: 0,
            max_date_seen: "2099-01-01".into(),
            lifetime_total: 10,
            machine_hash: TEST_MACHINE.to_string(),
        };
        assert_eq!(check_trial(&state), TrialStatus::Allowed);
        increment(&mut state);
        assert_eq!(state.max_date_seen, today_str());
    }

    #[test]
    fn increment_advances_state() {
        let mut state = TrialState::new_fresh(TEST_MACHINE);
        assert_eq!(state.count, 0);
        assert_eq!(state.lifetime_total, 0);
        increment(&mut state);
        assert_eq!(state.count, 1);
        assert_eq!(state.lifetime_total, 1);
        increment(&mut state);
        assert_eq!(state.count, 2);
        assert_eq!(state.lifetime_total, 2);
    }

    #[test]
    fn new_day_resets_daily_count() {
        let mut state = TrialState {
            date: "2025-01-01".into(),
            count: 5,
            max_date_seen: "2025-01-01".into(),
            lifetime_total: 30,
            machine_hash: TEST_MACHINE.to_string(),
        };
        increment(&mut state);
        assert_eq!(state.count, 1);
        assert_eq!(state.lifetime_total, 31);
    }
}
