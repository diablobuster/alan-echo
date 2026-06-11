//! ALAN Echo — optional acceleration pack download + install.
//!
//! The retail installer ships the CPU engine only (NSIS hard-fails at 2 GB).
//! GPU packs live on the release host behind site-owned stable URLs —
//! /api/echo/download/gpu (CUDA, NVIDIA) and /api/echo/download/vulkan
//! (Vulkan, AMD/Intel, beta); this module turns "extract a zip into
//! %APPDATA%" into one click: stream the zip with progress, extract to a
//! temp dir, verify, atomically swap into models/, and hot-restart the
//! engine. The Vulkan pack additionally gets a post-install health watch
//! that disables it and falls back to CPU if the driver can't run it.
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
use std::time::{Duration, Instant};
use tauri::{Emitter, State};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use crate::AppState;

/// Acceleration pack kinds the app can install. CUDA is the proven default
/// for NVIDIA machines; Vulkan (beta) covers AMD/Intel and gets a post-install
/// health watch + rollback because consumer Vulkan driver quality varies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackKind {
    Cuda,
    Vulkan,
}

impl PackKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "cuda" => Some(Self::Cuda),
            "vulkan" => Some(Self::Vulkan),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
        }
    }

    /// Directory under models/ — must match what whisper.rs probes.
    fn dir_name(self) -> &'static str {
        match self {
            Self::Cuda => "cuda_release",
            Self::Vulkan => "vulkan_release",
        }
    }

    /// Site-owned stable URLs — the host behind them can move without an app
    /// update (same rule as the installer download).
    fn url(self) -> String {
        // Debug-only test seam (forced-failure rollback test §5.3): point an
        // install at a local server. Compiled out of retail builds so a
        // shipped binary can never be redirected.
        #[cfg(debug_assertions)]
        if let Ok(u) = std::env::var(match self {
            Self::Cuda => "ECHO_CUDA_PACK_URL",
            Self::Vulkan => "ECHO_VULKAN_PACK_URL",
        }) {
            return u;
        }
        match self {
            Self::Cuda => "https://alanglobalintelligence.com/api/echo/download/gpu".to_string(),
            Self::Vulkan => "https://alanglobalintelligence.com/api/echo/download/vulkan".to_string(),
        }
    }

    /// Anything smaller is an error page or a truncated transfer, not the
    /// pack (CUDA ships cuBLAS blobs at ~440 MB; Vulkan is lean).
    fn min_bytes(self) -> u64 {
        match self {
            Self::Cuda => 50 * 1024 * 1024,
            Self::Vulkan => 5 * 1024 * 1024,
        }
    }
}

#[cfg(target_os = "windows")]
const PACK_BINARY_NAME: &str = "whisper-server.exe";
#[cfg(not(target_os = "windows"))]
const PACK_BINARY_NAME: &str = "whisper-server";

fn pack_server_exe(data_dir: &Path, kind: PackKind) -> std::path::PathBuf {
    data_dir
        .join("models")
        .join(kind.dir_name())
        .join("Release")
        .join(PACK_BINARY_NAME)
}

fn pack_server_exe_relative(extracted: &Path) -> std::path::PathBuf {
    extracted.join("Release").join(PACK_BINARY_NAME)
}

/// Installed AND not disabled by a rollback — what "installed" means to the
/// UI (a disabled pack must re-offer the install button, not hide it).
fn pack_usable(data_dir: &Path, kind: PackKind) -> bool {
    if kind == PackKind::Vulkan
        && data_dir.join("models").join(kind.dir_name()).join("DISABLED").exists()
    {
        return false;
    }
    pack_server_exe(data_dir, kind).exists()
}

/// First non-NVIDIA adapter that plausibly runs Vulkan (§5.2: all AMD/Intel
/// names match on purpose — the beta label and post-install rollback are the
/// guardrails, not a curated allowlist).
fn vulkan_candidate(adapters: &[String]) -> Option<String> {
    adapters
        .iter()
        .find(|n| {
            let l = n.to_lowercase();
            !l.contains("nvidia")
                && ["radeon", "arc", "iris", "intel", "amd"].iter().any(|k| l.contains(k))
        })
        .cloned()
}

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
    let info = state.whisper.info();
    // §5.2 offer logic: NVIDIA → CUDA pack (proven); otherwise any AMD/Intel
    // adapter → Vulkan pack, beta-labeled. No matching hardware → no offer.
    let (offer, offer_gpu, beta) = if info.gpu_name.is_some() {
        (Some(PackKind::Cuda), info.gpu_name.clone(), false)
    } else if let Some(name) = vulkan_candidate(&probe_display_adapters()) {
        (Some(PackKind::Vulkan), Some(name), true)
    } else {
        (None, None, false)
    };
    let installed = offer
        .map(|k| pack_usable(&state.data_dir, k))
        .unwrap_or(false);
    Ok(serde_json::json!({
        "gpu_name": info.gpu_name,
        "installed": installed,
        "engine_kind": info.engine_kind,
        "offer": offer.map(PackKind::as_str),
        "offer_gpu": offer_gpu,
        "beta": beta,
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
    pub vulkan_pack_installed: bool,
    pub engine_kind: Option<String>,
    pub cpu_cores: usize,
    /// cuda_ready | cuda_available | vulkan_ready | vulkan_available |
    /// cpu_only — the consequence in one word.
    pub verdict: String,
}

/// Explicit, user-initiated hardware test. Probes fresh, persists the result
/// (settings key "gpu_test") so the verdict survives restarts, and returns
/// everything the UI needs to explain the consequences honestly.
#[tauri::command]
pub fn test_gpu(state: State<Arc<AppState>>) -> Result<GpuTestResult, String> {
    let (nvidia_gpu, vram_mb) = probe_nvidia();
    let display_gpus = probe_display_adapters();
    let pack_installed = pack_usable(&state.data_dir, PackKind::Cuda);
    let vulkan_pack_installed = pack_usable(&state.data_dir, PackKind::Vulkan);
    let info = state.whisper.info();

    let vulkan_gpu = vulkan_candidate(&display_gpus);
    let verdict = if nvidia_gpu.is_some() && pack_installed {
        "cuda_ready"
    } else if nvidia_gpu.is_some() {
        "cuda_available"
    } else if vulkan_gpu.is_some() && vulkan_pack_installed {
        "vulkan_ready"
    } else if vulkan_gpu.is_some() {
        "vulkan_available"
    } else {
        "cpu_only"
    };

    let result = GpuTestResult {
        tested_at: chrono::Utc::now().to_rfc3339(),
        nvidia_gpu,
        vram_mb,
        display_gpus,
        pack_installed,
        vulkan_pack_installed,
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
    #[cfg(not(target_os = "windows"))]
    {
        return (None, None);
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("nvidia-smi");
        cmd.args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]);
        cmd.stdin(Stdio::null());
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
}

/// All display adapters reported by the OS. On Windows uses CIM; on macOS
/// uses system_profiler. The names feed the Vulkan offer logic and the
/// test_gpu result.
fn probe_display_adapters() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_VideoController | Where-Object { $_.Name }).Name",
        ]);
        cmd.stdin(Stdio::null());
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
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("system_profiler");
        cmd.args(["SPDisplaysDataType", "-detailLevel", "mini"]);
        cmd.stdin(Stdio::null());

        match cmd.output() {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout
                    .lines()
                    .filter_map(|l| {
                        let trimmed = l.trim();
                        if trimmed.starts_with("Chipset Model:") {
                            Some(trimmed.trim_start_matches("Chipset Model:").trim().to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Vec::new()
    }
}

#[tauri::command]
pub fn download_gpu_pack(
    app: tauri::AppHandle,
    state: State<Arc<AppState>>,
    kind: Option<String>,
) -> Result<(), String> {
    // No kind = CUDA, the original pack (pre-1.2 UI sent no argument).
    let kind = PackKind::parse(kind.as_deref().unwrap_or("cuda"))
        .ok_or_else(|| "Unknown acceleration pack".to_string())?;
    {
        let mut prog = PROGRESS.lock();
        // ONE progress channel — a second concurrent install stays refused.
        if matches!(prog.state.as_str(), "downloading" | "extracting" | "restarting") {
            return Err("The GPU pack is already downloading".into());
        }
        *prog = PackProgress { state: "downloading".into(), ..Default::default() };
    }

    let state = Arc::clone(state.inner());
    std::thread::Builder::new()
        .name("gpu-pack-install".into())
        .spawn(move || {
            if let Err(e) = run_install(&app, &state, kind) {
                log::error!("{} pack install failed: {}", kind.as_str(), e);
                set_progress(
                    &app,
                    PackProgress { state: "failed".into(), error: Some(e), ..Default::default() },
                );
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn run_install(app: &tauri::AppHandle, state: &Arc<AppState>, kind: PackKind) -> Result<(), String> {
    let models = state.data_dir.join("models");
    std::fs::create_dir_all(&models).map_err(|e| format!("Couldn't open the models folder: {}", e))?;
    let zip_path = models.join("gpu-pack.zip.partial");
    let tmp_dir = models.join(".gpu-pack-tmp");

    download_to(app, &kind.url(), &zip_path, kind.min_bytes())?;

    set_progress(app, PackProgress { state: "extracting".into(), ..Default::default() });
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)
            .map_err(|e| format!("Couldn't clear a previous attempt: {}", e))?;
    }
    let extract_result = extract_zip(&zip_path, &tmp_dir);
    std::fs::remove_file(&zip_path).ok();
    extract_result?;

    let extracted = tmp_dir.join(kind.dir_name());
    if !pack_server_exe_relative(&extracted).exists() {
        std::fs::remove_dir_all(&tmp_dir).ok();
        return Err("The downloaded pack is incomplete — try again, or email support".into());
    }
    let dest = models.join(kind.dir_name());
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| format!("Couldn't replace the previous pack: {}", e))?;
    }
    std::fs::rename(&extracted, &dest)
        .map_err(|e| format!("Couldn't move the pack into place: {}", e))?;
    std::fs::remove_dir_all(&tmp_dir).ok();
    // A fresh install supersedes any pack disabled by an earlier rollback.
    std::fs::remove_dir_all(models.join(format!("{}.disabled", kind.dir_name()))).ok();

    set_progress(app, PackProgress { state: "restarting".into(), ..Default::default() });
    let model_pref = state.settings.lock().get_str("whisper_model");
    state.whisper.reload(model_pref.as_deref());

    if kind == PackKind::Vulkan {
        // §5.3: consumer Vulkan drivers can crash the engine outright. Watch
        // the restart; a failure here must not re-fail on every launch with
        // no user-visible escape.
        confirm_vulkan_engine(state, &models)?;
    }

    set_progress(app, PackProgress { state: "done".into(), ..Default::default() });
    log::info!("{} pack installed; engine restarted", kind.as_str());
    Ok(())
}

const VULKAN_ROLLBACK_MSG: &str = "Your GPU's Vulkan driver couldn't run the engine — Echo is back on the CPU engine. Nothing is broken; email support@alanglobalintelligence.com with your GPU model.";

/// Wait for the engine restarted on the freshly installed Vulkan pack to come
/// up. Success = Ready on ANY binary (on a CUDA machine the new pack is
/// simply not the chosen engine — that's fine). If the engine fails (or never
/// becomes ready) while running the Vulkan binary, disable the pack and fall
/// back to CPU.
fn confirm_vulkan_engine(state: &Arc<AppState>, models: &Path) -> Result<(), String> {
    // whisper.rs kills a non-starting server at 120 s — 150 s covers that
    // with margin, so the child is dead (and the exe renameable) by then.
    let deadline = Instant::now() + Duration::from_secs(150);
    loop {
        let info = state.whisper.info();
        if info.ready {
            return Ok(());
        }
        if info.status == "stopped" || info.status == "idle" {
            // App shutdown raced the install (or the engine never started) —
            // that's not a driver verdict; leave the pack alone.
            return Err("The engine restart was interrupted — reopen Echo and try again".into());
        }
        let failed = info.status.starts_with("failed");
        if failed || Instant::now() > deadline {
            if info.engine_kind.as_deref() == Some("vulkan") {
                disable_vulkan_pack(state, models);
                return Err(VULKAN_ROLLBACK_MSG.into());
            }
            return Err(format!("The engine didn't restart cleanly ({})", info.status));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Rename the pack aside (NOT delete — support may want it) and restart the
/// engine, which falls back to the CPU build.
fn disable_vulkan_pack(state: &Arc<AppState>, models: &Path) {
    let live = models.join("vulkan_release");
    let disabled = models.join("vulkan_release.disabled");
    std::fs::remove_dir_all(&disabled).ok();
    // A failed spawn can hold the exe open for a surprising while (Defender
    // scans unknown binaries on first execution) — retry briefly, then fall
    // back to a DISABLED marker inside the pack. find_server_binary honors
    // the marker, and creating a new file can't be blocked by the exe lock,
    // so the broken pack can never re-fail on every launch.
    let mut renamed = false;
    for _ in 0..10 {
        match std::fs::rename(&live, &disabled) {
            Ok(()) => {
                renamed = true;
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        }
    }
    if !renamed {
        if let Err(e) = std::fs::write(
            live.join("DISABLED"),
            b"Disabled by ALAN Echo after the engine failed on this Vulkan driver.",
        ) {
            log::error!("Couldn't disable the failed Vulkan pack: {}", e);
        }
    }
    let model_pref = state.settings.lock().get_str("whisper_model");
    state.whisper.reload(model_pref.as_deref());
    log::warn!("Vulkan engine failed on this driver — pack disabled, engine back on CPU");
}

fn download_to(app: &tauri::AppHandle, url: &str, dest: &Path, min_bytes: u64) -> Result<(), String> {
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

    if downloaded < min_bytes {
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
