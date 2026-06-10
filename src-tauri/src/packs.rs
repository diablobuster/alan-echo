//! ALAN Echo — optional acceleration pack download + install.
//!
//! The retail installer ships the CPU engine only (NSIS hard-fails at 2 GB).
//! GPU packs live on the release host behind the site-owned stable URL
//! /api/echo/download/gpu; this module turns "extract a zip into %APPDATA%"
//! into one click: stream the zip with progress, extract to a temp dir,
//! verify, atomically swap into models/, and hot-restart the engine.
//!
//! Privacy note: this is the ONLY user-initiated network call in the app —
//! it downloads engine binaries and sends nothing. Dictation never touches
//! the network.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, State};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use crate::AppState;

const GPU_PACK_URL: &str = "https://alanglobalintelligence.com/api/echo/download/gpu";
/// Anything smaller is an error page or a truncated transfer, not the pack.
const MIN_PACK_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Default)]
pub struct PackProgress {
    /// idle | downloading | extracting | restarting | done | failed
    pub state: String,
    pub downloaded_mb: u64,
    pub total_mb: Option<u64>,
    pub error: Option<String>,
}

static PROGRESS: Lazy<Mutex<PackProgress>> = Lazy::new(|| {
    Mutex::new(PackProgress { state: "idle".into(), ..Default::default() })
});

fn set_progress(app: &tauri::AppHandle, p: PackProgress) {
    *PROGRESS.lock() = p.clone();
    // Global emit — reaches the webview even across hide/show cycles.
    app.emit("gpu-pack-progress", &p).ok();
}

#[tauri::command]
pub fn get_gpu_pack_status(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    let installed = state
        .data_dir
        .join("models")
        .join("cuda_release")
        .join("Release")
        .join("whisper-server.exe")
        .exists();
    let info = state.whisper.info();
    Ok(serde_json::json!({
        "gpu_name": info.gpu_name,
        "installed": installed,
        "engine_kind": info.engine_kind,
        "progress": PROGRESS.lock().clone(),
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuTestResult {
    pub tested_at: String,
    /// NVIDIA GPU per a FRESH nvidia-smi probe (not the cached launch-time
    /// detection — the user may have just installed a driver).
    pub nvidia_gpu: Option<String>,
    pub vram_mb: Option<u64>,
    /// Every display adapter Windows knows about — so AMD/Intel owners get a
    /// concrete answer instead of silence.
    pub display_gpus: Vec<String>,
    pub pack_installed: bool,
    pub engine_kind: Option<String>,
    pub cpu_cores: usize,
    /// cuda_ready | cuda_available | cpu_only — the consequence in one word.
    pub verdict: String,
}

/// Explicit, user-initiated hardware test. Probes fresh, persists the result
/// (settings key "gpu_test") so the verdict survives restarts, and returns
/// everything the UI needs to explain the consequences honestly.
#[tauri::command]
pub fn test_gpu(state: State<Arc<AppState>>) -> Result<GpuTestResult, String> {
    let (nvidia_gpu, vram_mb) = probe_nvidia();
    let display_gpus = probe_display_adapters();
    let pack_installed = state
        .data_dir
        .join("models")
        .join("cuda_release")
        .join("Release")
        .join("whisper-server.exe")
        .exists();
    let info = state.whisper.info();

    let verdict = if nvidia_gpu.is_some() && pack_installed {
        "cuda_ready"
    } else if nvidia_gpu.is_some() {
        "cuda_available"
    } else {
        "cpu_only"
    };

    let result = GpuTestResult {
        tested_at: chrono::Utc::now().to_rfc3339(),
        nvidia_gpu,
        vram_mb,
        display_gpus,
        pack_installed,
        engine_kind: info.engine_kind,
        cpu_cores: info.cpu_cores,
        verdict: verdict.to_string(),
    };

    // Persist so Settings can show "last tested …" on every visit. A save
    // failure degrades to a session-only result — never fail the test itself.
    {
        let mut s = state.settings.lock();
        if let Ok(v) = serde_json::to_value(&result) {
            s.set("gpu_test", v);
            if let Err(e) = s.save() {
                log::warn!("GPU test result could not be saved: {}", e);
            }
        }
    }

    Ok(result)
}

/// Fresh nvidia-smi probe (mirrors whisper.rs detect_hardware — duplicated on
/// purpose: that one is launch-time-cached engine state, this one is a live
/// user-initiated test).
fn probe_nvidia() -> (Option<String>, Option<u64>) {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]);
    cmd.stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            match stdout.lines().next() {
                Some(line) if line.contains(',') => {
                    let mut parts = line.rsplitn(2, ',');
                    let vram = parts.next().and_then(|v| v.trim().parse::<u64>().ok());
                    let name = parts.next().map(|n| n.trim().to_string());
                    (name, vram)
                }
                _ => (None, None),
            }
        }
        _ => (None, None),
    }
}

/// All display adapters via CIM — names AMD/Intel hardware so the verdict can
/// say "we see your Radeon; Vulkan support is on the roadmap" instead of
/// pretending the machine has no GPU. (wmic is deprecated on Win11 24H2+.)
fn probe_display_adapters() -> Vec<String> {
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "(Get-CimInstance Win32_VideoController | Where-Object { $_.Name }).Name",
    ]);
    cmd.stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

#[tauri::command]
pub fn download_gpu_pack(app: tauri::AppHandle, state: State<Arc<AppState>>) -> Result<(), String> {
    {
        let mut prog = PROGRESS.lock();
        if matches!(prog.state.as_str(), "downloading" | "extracting" | "restarting") {
            return Err("The GPU pack is already downloading".into());
        }
        *prog = PackProgress { state: "downloading".into(), ..Default::default() };
    }

    let state = Arc::clone(state.inner());
    std::thread::Builder::new()
        .name("gpu-pack-install".into())
        .spawn(move || {
            if let Err(e) = run_install(&app, &state) {
                log::error!("GPU pack install failed: {}", e);
                set_progress(
                    &app,
                    PackProgress { state: "failed".into(), error: Some(e), ..Default::default() },
                );
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn run_install(app: &tauri::AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    let models = state.data_dir.join("models");
    std::fs::create_dir_all(&models).map_err(|e| format!("Couldn't open the models folder: {}", e))?;
    let zip_path = models.join("gpu-pack.zip.partial");
    let tmp_dir = models.join(".gpu-pack-tmp");

    download_to(app, GPU_PACK_URL, &zip_path)?;

    set_progress(app, PackProgress { state: "extracting".into(), ..Default::default() });
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)
            .map_err(|e| format!("Couldn't clear a previous attempt: {}", e))?;
    }
    let extract_result = extract_zip(&zip_path, &tmp_dir);
    std::fs::remove_file(&zip_path).ok();
    extract_result?;

    // The pack zip contains cuda_release/Release/whisper-server.exe — verify
    // BEFORE touching the live models/ contents, then swap via rename so a
    // failure can never leave a half-installed engine in place.
    let extracted = tmp_dir.join("cuda_release");
    if !extracted.join("Release").join("whisper-server.exe").exists() {
        std::fs::remove_dir_all(&tmp_dir).ok();
        return Err("The downloaded pack is incomplete — try again, or email support".into());
    }
    let dest = models.join("cuda_release");
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| format!("Couldn't replace the previous pack: {}", e))?;
    }
    std::fs::rename(&extracted, &dest)
        .map_err(|e| format!("Couldn't move the pack into place: {}", e))?;
    std::fs::remove_dir_all(&tmp_dir).ok();

    set_progress(app, PackProgress { state: "restarting".into(), ..Default::default() });
    let model_pref = state.settings.lock().get_str("whisper_model");
    state.whisper.reload(model_pref.as_deref());

    set_progress(app, PackProgress { state: "done".into(), ..Default::default() });
    log::info!("GPU pack installed; engine restarting on the CUDA build");
    Ok(())
}

fn download_to(app: &tauri::AppHandle, url: &str, dest: &Path) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        // Per-read timeout, NOT overall — a 440 MB download on a slow link is
        // legitimate; a 60s stall on a single read is a dead connection.
        .timeout_read(Duration::from_secs(60))
        .build();

    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("Download failed — check your connection and try again ({})", e))?;
    let total: Option<u64> = resp.header("Content-Length").and_then(|v| v.parse().ok());
    let total_mb = total.map(|t| t / (1024 * 1024));

    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("Couldn't create the download file: {}", e))?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_emit_mb: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download interrupted — try again ({})", e))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Disk write failed (out of space?): {}", e))?;
        downloaded += n as u64;
        let mb = downloaded / (1024 * 1024);
        if mb > last_emit_mb {
            last_emit_mb = mb;
            set_progress(
                app,
                PackProgress {
                    state: "downloading".into(),
                    downloaded_mb: mb,
                    total_mb,
                    error: None,
                },
            );
        }
    }
    drop(file);

    if downloaded < MIN_PACK_BYTES {
        std::fs::remove_file(dest).ok();
        return Err("The download was unexpectedly small — the server may be busy; try again in a minute".into());
    }
    if let Some(t) = total {
        if downloaded != t {
            std::fs::remove_file(dest).ok();
            return Err("The download ended early — try again".into());
        }
    }
    Ok(())
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("The pack archive is corrupt: {}", e))?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // Zip-slip guard: enclosed_name rejects absolute paths and `..`.
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut f = std::fs::File::create(&out)
                .map_err(|e| format!("Couldn't write {}: {}", out.display(), e))?;
            std::io::copy(&mut entry, &mut f)
                .map_err(|e| format!("Extraction failed (out of disk space?): {}", e))?;
        }
    }
    Ok(())
}
