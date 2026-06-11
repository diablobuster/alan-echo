use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use tauri::{Emitter, AppHandle};

const VERSION_URL: &str = "https://alanglobalintelligence.com/api/echo/version";

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
    pub size_mb: Option<String>,
    pub release_date: Option<String>,
}

pub fn check_for_update(current_version: &str) -> Result<UpdateInfo, String> {
    let resp: VersionResponse = ureq::get(VERSION_URL)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("Version check failed: {}", e))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("Version parse failed: {}", e))?;

    let available = version_gt(&resp.version, current_version);

    Ok(UpdateInfo {
        available,
        current_version: current_version.to_string(),
        latest_version: resp.version,
        download_url: resp.download_url,
        size_mb: resp.size_mb,
        release_date: resp.release_date,
    })
}

pub fn download_and_launch_update(
    app: &AppHandle,
    download_url: &str,
    data_dir: &std::path::Path,
) -> Result<(), String> {
    let installer_path = data_dir.join("ALAN-Echo-Update.exe");

    app.emit("update_progress", serde_json::json!({ "stage": "downloading", "percent": 0 }))
        .ok();

    let resp = ureq::get(download_url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let total_size: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut file = std::fs::File::create(&installer_path)
        .map_err(|e| format!("Could not create installer file: {}", e))?;

    let mut reader = resp.into_body().into_reader();
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

    app.emit("update_progress", serde_json::json!({ "stage": "launching" })).ok();

    // Launch the installer and exit the app so it can upgrade in place
    std::process::Command::new(&installer_path)
        .spawn()
        .map_err(|e| format!("Could not launch installer: {}", e))?;

    // Give the installer a moment to start, then exit
    std::thread::sleep(std::time::Duration::from_millis(500));
    std::process::exit(0);
}

fn version_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .filter_map(|p| p.trim().parse::<u32>().ok())
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
