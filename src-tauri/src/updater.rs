use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::io::{Read as _, Write};
use tauri::{Emitter, AppHandle};

const VERSION_URL: &str = "https://www.alanglobalintelligence.com/api/echo/version";

fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") { "mac" } else { "windows" }
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    version: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    sha256: Option<String>,
    #[serde(rename = "sizeMb")]
    size_mb: Option<String>,
    #[serde(rename = "releaseDate")]
    release_date: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub download_url: Option<String>,
    pub sha256: Option<String>,
    pub size_mb: Option<String>,
    pub release_date: Option<String>,
}

pub fn check_for_update(current_version: &str) -> Result<UpdateInfo, String> {
    let url = format!("{}?platform={}", VERSION_URL, current_platform());
    let resp: VersionResponse = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("Version check failed: {}", e))?
        .into_json()
        .map_err(|e| format!("Version parse failed: {}", e))?;

    let available = version_gt(&resp.version, current_version);

    Ok(UpdateInfo {
        available,
        current_version: current_version.to_string(),
        latest_version: resp.version,
        download_url: resp.download_url,
        sha256: resp.sha256,
        size_mb: resp.size_mb,
        release_date: resp.release_date,
    })
}

pub fn download_and_launch_update(
    app: &AppHandle,
    download_url: &str,
    expected_sha256: Option<&str>,
    data_dir: &std::path::Path,
) -> Result<(), String> {
    // The URL and hash both arrive from the webview. The SHA-256 check is
    // self-referential against a hostile caller (it can hash its own payload),
    // so pin the host: a compromised webview must not be able to point this
    // at an attacker server and escalate to native code execution.
    let allowed = download_url.starts_with("https://www.alanglobalintelligence.com/")
        || download_url.starts_with("https://alanglobalintelligence.com/");
    if !allowed {
        return Err("Update downloads are only allowed from alanglobalintelligence.com".into());
    }

    let installer_path = if cfg!(target_os = "macos") {
        data_dir.join("ALAN-Echo-Update.dmg")
    } else {
        data_dir.join("ALAN-Echo-Update.exe")
    };

    app.emit("update_progress", serde_json::json!({ "stage": "downloading", "percent": 0 }))
        .ok();

    let resp = ureq::get(download_url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let total_size: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut file = std::fs::File::create(&installer_path)
        .map_err(|e| format!("Could not create installer file: {}", e))?;

    let mut reader = resp.into_reader();
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download read error: {}", e))?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).map_err(|e| format!("Write error: {}", e))?;
        downloaded += n as u64;
        if total_size > 0 {
            let pct = (downloaded as f64 / total_size as f64 * 100.0) as u32;
            app.emit("update_progress", serde_json::json!({ "stage": "downloading", "percent": pct }))
                .ok();
        }
    }

    file.flush().map_err(|e| format!("Flush error: {}", e))?;
    drop(file);

    let expected = expected_sha256.ok_or_else(|| {
        let _ = std::fs::remove_file(&installer_path);
        "Update verification unavailable — download from the website instead".to_string()
    })?;

    app.emit("update_progress", serde_json::json!({ "stage": "verifying" })).ok();
    let mut hasher = Sha256::new();
    let mut f = std::fs::File::open(&installer_path)
        .map_err(|e| format!("Could not open installer for verification: {}", e))?;
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("Read error during verification: {}", e))?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    let hash = format!("{:x}", hasher.finalize());
    if !hash.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_file(&installer_path);
        return Err(format!("Download integrity check failed: expected {} got {}", expected, hash));
    }

    app.emit("update_progress", serde_json::json!({ "stage": "launching" })).ok();

    launch_installer(app, &installer_path)
}

#[cfg(target_os = "macos")]
fn launch_installer(app: &AppHandle, dmg_path: &std::path::Path) -> Result<(), String> {
    // Mount the DMG — the user drags ALAN Echo to Applications from the Finder
    // window that opens. We don't exit: the user keeps the current version
    // running until they relaunch.
    std::process::Command::new("open")
        .arg(dmg_path)
        .spawn()
        .map_err(|e| format!("Could not open update DMG: {}", e))?;

    app.emit(
        "update_progress",
        serde_json::json!({
            "stage": "mac_drag_install",
            "message": "Drag the new ALAN Echo to your Applications folder to complete the update, then relaunch."
        }),
    )
    .ok();

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn launch_installer(app: &AppHandle, exe_path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(exe_path)
        .spawn()
        .map_err(|e| format!("Could not launch installer: {}", e))?;

    // process::exit skips the RunEvent::Exit cleanup — shut the engine down
    // explicitly or every self-update orphans a whisper-server holding the
    // model in RAM/VRAM and open handles that can make NSIS fail on locked
    // files.
    use tauri::Manager;
    let state = app.state::<std::sync::Arc<crate::AppState>>();
    state.whisper.shutdown();

    std::thread::sleep(std::time::Duration::from_millis(500));
    std::process::exit(0);
}

fn version_gt(a: &str, b: &str) -> bool {
    // Take the leading numeric run of each dot-segment so a pre-release/hotfix
    // suffix (e.g. "1.2.4-beta") doesn't drop the whole segment and silently
    // suppress a real update. "1.2.4-beta" -> [1,2,4]; "1.3-hotfix" -> [1,3].
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .map(|p| {
                p.trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u32>()
                    .unwrap_or(0)
            })
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    for i in 0..va.len().max(vb.len()) {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        if x > y { return true; }
        if x < y { return false; }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::version_gt;

    #[test]
    fn plain_semver() {
        assert!(version_gt("1.2.3", "1.2.2"));
        assert!(version_gt("1.3.0", "1.2.9"));
        assert!(version_gt("2.0.0", "1.9.9"));
        assert!(!version_gt("1.2.3", "1.2.3"));
        assert!(!version_gt("1.2.2", "1.2.3"));
    }

    #[test]
    fn shorter_is_not_greater() {
        assert!(!version_gt("1.2", "1.2.0"));
        assert!(version_gt("1.2.1", "1.2"));
    }

    #[test]
    fn prerelease_suffix_does_not_suppress_a_real_update() {
        // Regression: filter_map(parse) previously dropped "4-beta" entirely,
        // leaving [1,2] which is NOT > [1,2,3] — so a real update was withheld.
        assert!(version_gt("1.2.4-beta", "1.2.3"));
        assert!(version_gt("1.3-hotfix", "1.2.9"));
        assert!(version_gt("1.2.10-rc1", "1.2.9"));
        // Same numeric core must still be treated as equal (no update).
        assert!(!version_gt("1.2.3-beta", "1.2.3"));
    }
}
