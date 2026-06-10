//! ALAN Echo — persistent whisper-server engine.
//!
//! Instead of shelling out to whisper-cli per transcription (which reloads the
//! model from disk every time, ~14s cold), we spawn whisper-server once at app
//! launch. It loads the model into (V)RAM and serves HTTP requests on
//! 127.0.0.1, so every transcription — including the first — costs only
//! inference time (~1-2s on GPU for short clips).

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Hide the console window of spawned children (whisper-server, nvidia-smi).
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const HOST: &str = "127.0.0.1";
const BASE_PORT: u16 = 8178;
/// large-v3 on a cold disk can take a while to map into VRAM.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
/// Long clips on a slow CPU need generous headroom.
const INFERENCE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub duration_seconds: f64,
    pub language: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineInfo {
    pub gpu_name: Option<String>,
    pub vram_mb: Option<u64>,
    /// An NVIDIA GPU is PRESENT (nvidia-smi answered). Says nothing about
    /// which engine build is actually running — see `engine_kind`.
    pub cuda: bool,
    pub cpu_cores: usize,
    pub model_file: Option<String>,
    pub model_label: Option<String>,
    /// "cuda" | "vulkan" | "cpu" — derived from the binary actually spawned,
    /// so the UI can tell "GPU present but CPU engine running" from "GPU
    /// acceleration active".
    pub engine_kind: Option<String>,
    pub ready: bool,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Status {
    Idle,
    Starting,
    Ready,
    Failed(String),
    Stopped,
}

struct Inner {
    child: Option<Child>,
    port: u16,
    status: Status,
    /// Bumped on every (re)start so stale init threads abandon themselves.
    generation: u64,
    model_file: Option<String>,
    engine_kind: Option<String>,
}

struct Hardware {
    gpu_name: Option<String>,
    vram_mb: Option<u64>,
    physical_cores: usize,
}

pub struct WhisperEngine {
    inner: Arc<Mutex<Inner>>,
    hw: Hardware,
    data_dir: PathBuf,
    language: String,
}

impl WhisperEngine {
    pub fn new(data_dir: &Path) -> Self {
        let hw = detect_hardware();
        match &hw.gpu_name {
            Some(name) => log::info!("GPU detected: {} ({} MB VRAM)", name, hw.vram_mb.unwrap_or(0)),
            None => log::info!("No NVIDIA GPU detected — using CPU build ({} physical cores)", hw.physical_cores),
        }
        Self {
            inner: Arc::new(Mutex::new(Inner {
                child: None,
                port: BASE_PORT,
                status: Status::Idle,
                generation: 0,
                model_file: None,
                engine_kind: None,
            })),
            hw,
            data_dir: data_dir.to_path_buf(),
            language: "en".to_string(),
        }
    }

    /// Spawn whisper-server in the background. Returns immediately; readiness
    /// is reflected by `is_ready()` / `info()`.
    pub fn start(&self, model_pref: Option<&str>) {
        let binary = match self.find_server_binary() {
            Ok(b) => b,
            Err(e) => {
                self.inner.lock().status = Status::Failed(e);
                return;
            }
        };
        let model = match self.resolve_model(model_pref) {
            Ok(m) => m,
            Err(e) => {
                self.inner.lock().status = Status::Failed(e);
                return;
            }
        };

        let generation;
        let port;
        {
            let mut inner = self.inner.lock();
            if inner.status == Status::Stopped {
                // shutdown() is final (app exit) — a transcribe() retry racing
                // the exit handler must never resurrect an orphan server.
                return;
            }
            if let Some(mut old) = inner.child.take() {
                old.kill().ok();
                old.wait().ok();
            }
            port = match free_port() {
                Some(p) => p,
                None => {
                    inner.status = Status::Failed("No free port found for whisper-server".into());
                    return;
                }
            };
            inner.generation += 1;
            generation = inner.generation;
            inner.port = port;
            inner.status = Status::Starting;
            inner.model_file = model.file_name().map(|n| n.to_string_lossy().to_string());
            inner.engine_kind = Some(binary_kind(&binary));
        }

        log::info!("Starting whisper-server: {} (model {}) on port {}", binary.display(), model.display(), port);

        let inner = Arc::clone(&self.inner);
        let threads = self.hw.physical_cores.max(1);
        let language = self.language.clone();

        std::thread::Builder::new()
            .name("whisper-server-init".into())
            .spawn(move || {
                let child = match spawn_server(&binary, &model, port, threads, &language) {
                    Ok(c) => c,
                    Err(e) => {
                        let mut guard = inner.lock();
                        if guard.generation == generation {
                            guard.status = Status::Failed(format!("Failed to launch whisper-server: {}", e));
                        }
                        return;
                    }
                };
                {
                    let mut guard = inner.lock();
                    if guard.generation != generation {
                        // A newer start superseded us — clean up our child.
                        let mut c = child;
                        c.kill().ok();
                        c.wait().ok();
                        return;
                    }
                    guard.child = Some(child);
                }

                // The server binds its socket only after the model is loaded,
                // so a successful TCP connect means it's ready to serve.
                let deadline = Instant::now() + STARTUP_TIMEOUT;
                loop {
                    if std::net::TcpStream::connect_timeout(
                        &format!("{}:{}", HOST, port).parse().expect("static addr"),
                        Duration::from_millis(500),
                    ).is_ok() {
                        let mut guard = inner.lock();
                        if guard.generation == generation {
                            guard.status = Status::Ready;
                            log::info!("whisper-server ready on port {}", port);
                        }
                        return;
                    }

                    let mut guard = inner.lock();
                    if guard.generation != generation {
                        return;
                    }
                    // Bail out if the process died (bad flag, OOM, missing DLL…).
                    if let Some(child) = guard.child.as_mut() {
                        if let Ok(Some(code)) = child.try_wait() {
                            guard.status = Status::Failed(format!("whisper-server exited during startup ({})", code));
                            guard.child = None;
                            return;
                        }
                    }
                    if Instant::now() > deadline {
                        guard.status = Status::Failed("whisper-server did not become ready in time".into());
                        if let Some(mut c) = guard.child.take() {
                            c.kill().ok();
                            c.wait().ok();
                        }
                        return;
                    }
                    drop(guard);
                    std::thread::sleep(Duration::from_millis(300));
                }
            })
            .ok();
    }

    /// Kill the current server and start a fresh one (e.g. after a model change).
    pub fn reload(&self, model_pref: Option<&str>) {
        self.start(model_pref);
    }

    pub fn shutdown(&self) {
        let mut inner = self.inner.lock();
        inner.generation += 1; // cancel any in-flight init thread
        inner.status = Status::Stopped;
        if let Some(mut child) = inner.child.take() {
            child.kill().ok();
            child.wait().ok();
        }
    }

    pub fn is_ready(&self) -> bool {
        let mut inner = self.inner.lock();
        if inner.status != Status::Ready {
            return false;
        }
        // Detect a crashed server so the UI doesn't claim readiness.
        if let Some(child) = inner.child.as_mut() {
            if let Ok(Some(_)) = child.try_wait() {
                inner.status = Status::Failed("whisper-server crashed".into());
                inner.child = None;
                return false;
            }
        }
        true
    }

    pub fn info(&self) -> EngineInfo {
        let inner = self.inner.lock();
        let (ready, status) = match &inner.status {
            Status::Idle => (false, "idle".to_string()),
            Status::Starting => (false, "loading model".to_string()),
            Status::Ready => (true, "ready".to_string()),
            Status::Failed(e) => (false, format!("failed: {}", e)),
            Status::Stopped => (false, "stopped".to_string()),
        };
        EngineInfo {
            gpu_name: self.hw.gpu_name.clone(),
            vram_mb: self.hw.vram_mb,
            cuda: self.hw.gpu_name.is_some(),
            cpu_cores: self.hw.physical_cores,
            model_file: inner.model_file.clone(),
            model_label: inner.model_file.as_deref().map(model_label),
            engine_kind: inner.engine_kind.clone(),
            ready,
            status,
        }
    }

    pub fn transcribe(&self, wav_path: &str) -> Result<TranscriptionResult, String> {
        let port = match self.wait_ready(Duration::from_secs(90)) {
            Ok(p) => p,
            Err(first_err) => {
                // Failed is otherwise sticky for the whole session — attempt
                // one restart before giving up (e.g. transient CUDA OOM or a
                // crash during startup). Permanent causes (missing binary or
                // model) re-fail fast inside start() without spawning.
                if !matches!(self.inner.lock().status, Status::Failed(_)) {
                    return Err(first_err);
                }
                log::warn!("Speech engine failed ({}), attempting restart", first_err);
                let model = self.inner.lock().model_file.clone();
                let pref = model.as_deref().and_then(model_name_from_file);
                self.start(pref.as_deref());
                self.wait_ready(Duration::from_secs(90))?
            }
        };

        let wav_bytes = std::fs::read(wav_path).map_err(|e| format!("Failed to read recording: {}", e))?;

        let text = match post_inference(port, &wav_bytes) {
            Ok(t) => t,
            Err(first_err) => {
                // The server may have crashed mid-flight — restart once and retry.
                log::warn!("Inference request failed ({}), restarting whisper-server", first_err);
                let model = self.inner.lock().model_file.clone();
                let pref = model.as_deref().and_then(model_name_from_file);
                self.start(pref.as_deref());
                let port = self.wait_ready(Duration::from_secs(90))?;
                post_inference(port, &wav_bytes)
                    .map_err(|e| format!("Transcription failed after retry: {}", e))?
            }
        };

        let duration = get_wav_duration(wav_path).unwrap_or(0.0);
        Ok(TranscriptionResult {
            text: text.trim().to_string(),
            duration_seconds: duration,
            language: self.language.clone(),
        })
    }

    fn wait_ready(&self, timeout: Duration) -> Result<u16, String> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let inner = self.inner.lock();
                match &inner.status {
                    Status::Ready => return Ok(inner.port),
                    Status::Failed(e) => return Err(format!("Speech engine unavailable: {}", e)),
                    Status::Stopped => return Err("Speech engine is shut down".into()),
                    Status::Idle => return Err("Speech engine was never started".into()),
                    Status::Starting => {}
                }
            }
            if Instant::now() > deadline {
                return Err("Speech engine is still loading — try again in a moment".into());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Pick the best engine build present: CUDA when an NVIDIA GPU is around,
    /// then Vulkan (covers AMD/Intel — pack shipped separately; the directory
    /// simply doesn't exist until installed), then CPU. Each build lives in
    /// its own directory with matching DLLs.
    fn find_server_binary(&self) -> Result<PathBuf, String> {
        let models = self.data_dir.join("models");
        let mut candidates: Vec<PathBuf> = Vec::new();

        let vulkan = models.join("vulkan_release").join("Release").join("whisper-server.exe");
        if self.hw.gpu_name.is_some() {
            candidates.push(models.join("cuda_release").join("Release").join("whisper-server.exe"));
            candidates.push(vulkan.clone());
            candidates.push(models.join("whisper-server.exe"));
            candidates.push(models.join("Release").join("whisper-server.exe"));
        } else {
            candidates.push(vulkan.clone());
            candidates.push(models.join("Release").join("whisper-server.exe"));
            candidates.push(models.join("whisper-server-cpu.exe"));
            candidates.push(models.join("whisper-server.exe"));
            candidates.push(models.join("cuda_release").join("Release").join("whisper-server.exe"));
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("whisper-server.exe"));
                candidates.push(dir.join("models").join("whisper-server.exe"));
            }
        }

        candidates
            .into_iter()
            .find(|p| p.exists())
            .ok_or_else(|| "whisper-server.exe not found in the models directory".to_string())
    }

    fn model_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.data_dir.join("models"), self.data_dir.clone()];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                dirs.push(dir.join("models"));
                dirs.push(dir.to_path_buf());
            }
        }
        dirs
    }

    /// True if a model file for `name` (any quantized or .en variant) exists on disk.
    pub fn model_available(&self, name: &str) -> bool {
        let dirs = self.model_dirs();
        model_file_candidates(name)
            .iter()
            .any(|v| dirs.iter().any(|d| d.join(v).exists()))
    }

    /// Honor the user's model preference; fall back to the best model that
    /// exists on disk for this hardware.
    fn resolve_model(&self, pref: Option<&str>) -> Result<PathBuf, String> {
        let default_order: &[&str] = if self.hw.gpu_name.is_some() {
            &["medium", "large-v3-turbo", "large-v3", "small", "base", "tiny"]
        } else if self.hw.physical_cores >= 8 {
            &["medium", "small", "base", "tiny", "large-v3-turbo", "large-v3"]
        } else if self.hw.physical_cores >= 4 {
            &["small", "base", "medium", "tiny", "large-v3-turbo", "large-v3"]
        } else {
            &["base", "tiny", "small", "medium", "large-v3-turbo", "large-v3"]
        };

        let mut order: Vec<&str> = Vec::new();
        if let Some(p) = pref {
            if !p.is_empty() && p != "auto" {
                order.push(p);
            }
        }
        for name in default_order {
            if !order.contains(name) {
                order.push(name);
            }
        }

        let dirs = self.model_dirs();

        for name in &order {
            for variant in model_file_candidates(name) {
                for dir in &dirs {
                    let p = dir.join(&variant);
                    if p.exists() {
                        return Ok(p);
                    }
                }
            }
        }

        Err("No Whisper model found — place a ggml model file in the models directory".into())
    }
}

impl Drop for WhisperEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn spawn_server(binary: &Path, model: &Path, port: u16, threads: usize, language: &str) -> Result<Child, String> {
    let mut cmd = Command::new(binary);
    cmd.args([
        "-m", &model.to_string_lossy(),
        "--host", HOST,
        "--port", &port.to_string(),
        "-l", language,
        "-t", &threads.to_string(),
        "-sns", // suppress non-speech tokens — fewer [MUSIC]-style hallucinations
    ]);
    if let Some(dir) = binary.parent() {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map_err(|e| e.to_string())
}

/// POST the WAV to whisper-server's /inference endpoint as multipart form data.
fn post_inference(port: u16, wav_bytes: &[u8]) -> Result<String, String> {
    const BOUNDARY: &str = "----AlanEchoBoundary7MA4YWxkTrZu0gW";

    let mut body: Vec<u8> = Vec::with_capacity(wav_bytes.len() + 512);
    body.extend_from_slice(format!("--{}\r\n", BOUNDARY).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n");
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(wav_bytes);
    body.extend_from_slice(format!("\r\n--{}\r\n", BOUNDARY).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n");
    body.extend_from_slice(format!("--{}--\r\n", BOUNDARY).as_bytes());

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout(INFERENCE_TIMEOUT)
        .build();

    let resp = agent
        .post(&format!("http://{}:{}/inference", HOST, port))
        .set("Content-Type", &format!("multipart/form-data; boundary={}", BOUNDARY))
        .send_bytes(&body)
        .map_err(|e| format!("whisper-server request failed: {}", e))?;

    let mut text = String::new();
    resp.into_reader()
        .take(50 * 1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| format!("Failed to read whisper-server response: {}", e))?;

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid whisper-server response: {}", e))?;
    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
        return Err(format!("whisper-server error: {}", err));
    }
    Ok(json.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string())
}

fn free_port() -> Option<u16> {
    (BASE_PORT..BASE_PORT + 20).find(|p| std::net::TcpListener::bind((HOST, *p)).is_ok())
}

/// Which acceleration family a server binary belongs to, by its pack directory.
fn binary_kind(path: &Path) -> String {
    let p = path.to_string_lossy();
    if p.contains("cuda_release") {
        "cuda".to_string()
    } else if p.contains("vulkan_release") {
        "vulkan".to_string()
    } else {
        "cpu".to_string()
    }
}

fn detect_hardware() -> Hardware {
    let physical_cores = num_cpus::get_physical().max(1);

    let mut cmd = Command::new("nvidia-smi");
    cmd.args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]);
    cmd.stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let (gpu_name, vram_mb) = match cmd.output() {
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
    };

    Hardware { gpu_name, vram_mb, physical_cores }
}

/// Filename candidates for a model name, in preference order. English-only
/// (.en) variants outrank multilingual at equal size — the app always dictates
/// with `-l en` and the retail installer bundles ggml-base.en.bin. Quantized
/// variants outrank f16 (smaller, ~equal accuracy). Nonexistent combinations
/// (e.g. large-v3.en) are harmless — they simply never match a file.
fn model_file_candidates(name: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(8);
    for stem in [format!("{}.en", name), name.to_string()] {
        for quant in ["-q5_0", "-q5_1", "-q8_0", ""] {
            out.push(format!("ggml-{}{}.bin", stem, quant));
        }
    }
    out
}

fn model_label(file: &str) -> String {
    if file.contains("large") {
        "Ultra".to_string()
    } else if file.contains("medium") {
        "Enhanced".to_string()
    } else if file.contains("small") {
        "Standard".to_string()
    } else if file.contains("base") {
        "Basic".to_string()
    } else if file.contains("tiny") {
        "Lite".to_string()
    } else {
        file.to_string()
    }
}

/// "ggml-medium-q5_0.bin" → "medium"; "ggml-base.en.bin" → "base".
/// Canonical names never carry the .en suffix — resolution re-adds it.
fn model_name_from_file(file: &str) -> Option<String> {
    let stem = file.strip_prefix("ggml-")?.strip_suffix(".bin")?;
    let stem = stem
        .strip_suffix("-q5_0")
        .or_else(|| stem.strip_suffix("-q5_1"))
        .or_else(|| stem.strip_suffix("-q8_0"))
        .unwrap_or(stem);
    let stem = stem.strip_suffix(".en").unwrap_or(stem);
    Some(stem.to_string())
}

fn get_wav_duration(path: &str) -> Option<f64> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let samples = reader.len() as f64;
    Some(samples / spec.sample_rate as f64 / spec.channels as f64)
}
