use ed25519_dalek::{Signature, VerifyingKey, Verifier};
use sha2::{Sha256, Digest};
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const PUB_KEY: [u8; 32] = [
    52,141,194,214,101,18,213,166,59,40,39,67,252,7,200,105,
    63,141,40,65,38,142,224,1,97,49,136,45,70,190,132,151
];

const ACTIVATE_URL: &str = "https://www.alanglobalintelligence.com/api/echo/activate";

pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("activation.jwt")
}

pub fn is_activated(data_dir: &Path) -> bool {
    let path = token_path(data_dir);
    if !path.exists() {
        return false;
    }
    let token = match std::fs::read_to_string(&path) {
        Ok(t) => t.trim().to_string(),
        Err(_) => return false,
    };
    let mfp = machine_fingerprint();
    verify_token(&token, &mfp).is_ok()
}

pub fn verify_token(token: &str, expected_mfp: &str) -> Result<serde_json::Value, String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("Malformed token".into());
    }
    let sig_input = format!("{}.{}", parts[0], parts[1]);
    let sig_bytes = base64_url_decode(parts[2])?;
    if sig_bytes.len() != 64 {
        return Err("Invalid signature length".into());
    }

    let verifying_key = VerifyingKey::from_bytes(&PUB_KEY)
        .map_err(|e| format!("Invalid public key: {}", e))?;
    let signature = Signature::from_bytes(&sig_bytes.try_into().map_err(|_| "bad sig")?);
    verifying_key.verify(sig_input.as_bytes(), &signature)
        .map_err(|_| "Signature verification failed")?;

    let payload_json = base64_url_decode(parts[1])?;
    let claims: serde_json::Value = serde_json::from_slice(&payload_json)
        .map_err(|e| format!("Invalid claims: {}", e))?;

    let token_mfp = claims.get("mfp").and_then(|v| v.as_str()).unwrap_or("");
    if !token_mfp.eq_ignore_ascii_case(expected_mfp) {
        return Err("Machine fingerprint mismatch".into());
    }

    // Tokens carry a 400-day `exp` since 2026-06; older tokens have none and
    // stay valid forever (existing customers must never be bricked by an
    // update). 7-day grace absorbs clock skew; an expired token just reads as
    // not-activated, and check_license silently re-activates with the saved key.
    if let Some(exp) = claims.get("exp").and_then(|v| v.as_i64()) {
        let now = chrono::Utc::now().timestamp();
        if now > exp + 7 * 86_400 {
            log::info!("Activation token expired; silent re-activation will refresh it");
            return Err("Activation token expired".into());
        }
    }

    Ok(claims)
}

fn base64_url_decode(input: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .map_err(|e| format!("Base64 decode error: {}", e))
}

pub fn activate_online(key: &str, data_dir: &Path) -> Result<String, String> {
    let mfp = machine_fingerprint();
    let body = serde_json::json!({ "key": key, "machineHash": mfp });

    let resp = ureq::post(ACTIVATE_URL)
        .timeout(std::time::Duration::from_secs(15))
        .send_json(&body)
        .map_err(|e| format!("Activation request failed: {}", e))?;

    let result: serde_json::Value = resp.into_json()
        .map_err(|e| format!("Invalid response: {}", e))?;

    if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }

    let token = result.get("token").and_then(|v| v.as_str())
        .ok_or("No token in response")?;

    verify_token(token, &mfp)?;

    let path = token_path(data_dir);
    std::fs::write(&path, token)
        .map_err(|e| format!("Could not save activation token: {}", e))?;

    Ok(token.to_string())
}

pub fn machine_fingerprint() -> String {
    let raw = raw_fingerprint_components();
    let input = raw.join("|");
    let hash = Sha256::digest(input.as_bytes());
    format!("{:x}", hash)
}

#[cfg(target_os = "windows")]
fn raw_fingerprint_components() -> Vec<String> {
    vec![
        wmi_value("Win32_Processor", "ProcessorId"),
        wmi_value_fallback("Win32_BaseBoard", "SerialNumber", "Product"),
        wmi_value("Win32_DiskDrive", "SerialNumber"),
    ]
}

#[cfg(target_os = "macos")]
fn raw_fingerprint_components() -> Vec<String> {
    vec![
        mac_ioreg_serial(),
        mac_hw_model(),
        mac_disk_serial(),
    ]
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn raw_fingerprint_components() -> Vec<String> {
    vec!["UNKNOWN".into(), "UNKNOWN".into(), "UNKNOWN".into()]
}

#[cfg(target_os = "windows")]
fn wmi_value(class: &str, prop: &str) -> String {
    use std::process::{Command, Stdio};
    let query = format!(
        "Get-CimInstance {} | Select-Object -ExpandProperty {} -ErrorAction SilentlyContinue",
        class, prop
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &query]);
    cmd.stdin(Stdio::null());
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    match cmd.output() {
        Ok(out) if out.status.success() => {
            let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if val.is_empty() || val == "To Be Filled By O.E.M." || val == "Default string" {
                "UNKNOWN".into()
            } else {
                val
            }
        }
        _ => "UNKNOWN".into(),
    }
}

#[cfg(target_os = "windows")]
fn wmi_value_fallback(class: &str, primary: &str, fallback: &str) -> String {
    let val = wmi_value(class, primary);
    if val == "UNKNOWN" { wmi_value(class, fallback) } else { val }
}

#[cfg(target_os = "macos")]
fn mac_ioreg_serial() -> String {
    use std::process::{Command, Stdio};
    match Command::new("ioreg").args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .stdin(Stdio::null()).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformSerialNumber") {
                    if let Some(val) = line.split('"').nth(3) {
                        if !val.is_empty() { return val.to_string(); }
                    }
                }
            }
            "UNKNOWN".into()
        }
        _ => "UNKNOWN".into(),
    }
}

#[cfg(target_os = "macos")]
fn mac_hw_model() -> String {
    use std::process::{Command, Stdio};
    match Command::new("sysctl").args(["-n", "hw.model"])
        .stdin(Stdio::null()).output() {
        Ok(out) if out.status.success() => {
            let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if val.is_empty() { "UNKNOWN".into() } else { val }
        }
        _ => "UNKNOWN".into(),
    }
}

#[cfg(target_os = "macos")]
fn mac_disk_serial() -> String {
    use std::process::{Command, Stdio};
    match Command::new("diskutil").args(["info", "disk0"])
        .stdin(Stdio::null()).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Volume UUID:") || trimmed.starts_with("Disk / Partition UUID:") {
                    if let Some(val) = trimmed.split(':').nth(1) {
                        let val = val.trim();
                        if !val.is_empty() { return val.to_string(); }
                    }
                }
            }
            "UNKNOWN".into()
        }
        _ => "UNKNOWN".into(),
    }
}
