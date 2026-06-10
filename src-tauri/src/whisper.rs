//! ALAN Echo — Whisper.cpp sidecar integration.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub duration_seconds: f64,
    pub language: String,
}

pub struct WhisperEngine {
    binary_path: PathBuf,
    model_path: PathBuf,
    language: String,
}

impl WhisperEngine {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let binary_path = find_whisper_binary(data_dir)?;
        let model_path = find_model(data_dir)?;
        log::info!("Whisper binary: {}", binary_path.display());
        log::info!("Whisper model: {}", model_path.display());
        Ok(Self { binary_path, model_path, language: "en".to_string() })
    }

    pub fn is_ready(&self) -> bool {
        self.binary_path.exists() && self.model_path.exists()
    }

    pub fn transcribe(&self, wav_path: &str) -> Result<TranscriptionResult, String> {
        if !self.is_ready() {
            return Err("Whisper engine not ready — model or binary missing".into());
        }

        let base_path = PathBuf::from(wav_path).with_extension("");

        let result = Command::new(&self.binary_path)
            .args([
                "-m", self.model_path.to_str().unwrap_or(""),
                "-f", wav_path,
                "-l", &self.language,
                "--no-timestamps",
                "-otxt",
                "-of", base_path.to_str().unwrap_or(""),
            ])
            .output()
            .map_err(|e| format!("Failed to run whisper: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("Whisper failed: {}", stderr));
        }

        let txt_path = base_path.with_extension("txt");
        let text = std::fs::read_to_string(&txt_path)
            .unwrap_or_default()
            .trim()
            .to_string();

        std::fs::remove_file(&txt_path).ok();

        let duration = get_wav_duration(wav_path).unwrap_or(0.0);

        Ok(TranscriptionResult { text, duration_seconds: duration, language: self.language.clone() })
    }
}

fn find_whisper_binary(data_dir: &Path) -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    const NAMES: &[&str] = &["whisper-cli.exe", "whisper.exe", "main.exe"];
    #[cfg(not(target_os = "windows"))]
    const NAMES: &[&str] = &["whisper-cli", "whisper", "main"];

    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().unwrap_or(Path::new("."));
        for name in NAMES {
            let p = dir.join(name);
            if p.exists() { return Ok(p); }
        }
    }

    for subdir in &["", "models", "bin"] {
        let dir = if subdir.is_empty() { data_dir.to_path_buf() } else { data_dir.join(subdir) };
        for name in NAMES {
            let p = dir.join(name);
            if p.exists() { return Ok(p); }
        }
    }

    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    if let Ok(output) = Command::new(which_cmd).arg(NAMES[0]).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(first) = path.lines().next() {
                if !first.is_empty() { return Ok(PathBuf::from(first)); }
            }
        }
    }

    Err("Whisper binary not found. Place whisper-cli in the app or models directory.".into())
}

fn find_model(data_dir: &Path) -> Result<PathBuf, String> {
    let model_names = [
        "ggml-large-v3-turbo.bin",
        "ggml-large-v3.bin",
        "ggml-medium.bin",
        "ggml-small.bin",
        "ggml-base.bin",
    ];

    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().unwrap_or(Path::new("."));
        for subdir in &["models", ""] {
            let d = if subdir.is_empty() { dir.to_path_buf() } else { dir.join(subdir) };
            for name in &model_names {
                let p = d.join(name);
                if p.exists() { return Ok(p); }
            }
        }
    }

    for subdir in &["models", ""] {
        let d = if subdir.is_empty() { data_dir.to_path_buf() } else { data_dir.join(subdir) };
        for name in &model_names {
            let p = d.join(name);
            if p.exists() { return Ok(p); }
        }
    }

    Err("Whisper model not found. Place a ggml model file in the models/ directory.".into())
}

fn get_wav_duration(path: &str) -> Option<f64> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let samples = reader.len() as f64;
    Some(samples / spec.sample_rate as f64 / spec.channels as f64)
}
