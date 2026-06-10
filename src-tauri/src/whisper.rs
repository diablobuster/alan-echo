//! ALAN Echo — Whisper.cpp sidecar integration.
//! Runs whisper.cpp CLI as a subprocess for transcription.
//! Falls back to a bundled binary, or a user-provided path.

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

        Ok(Self {
            binary_path,
            model_path,
            language: "en".to_string(),
        })
    }

    /// Check if the engine is ready (binary + model exist).
    pub fn is_ready(&self) -> bool {
        self.binary_path.exists() && self.model_path.exists()
    }

    /// Transcribe a WAV file and return the text.
    pub fn transcribe(&self, wav_path: &str) -> Result<TranscriptionResult, String> {
        if !self.is_ready() {
            return Err("Whisper engine not ready — model or binary missing".into());
        }

        let output_path = PathBuf::from(wav_path).with_extension("txt");

        let result = Command::new(&self.binary_path)
            .args([
                "-m", self.model_path.to_str().unwrap_or(""),
                "-f", wav_path,
                "-l", &self.language,
                "--no-timestamps",
                "--print-special", "false",
                "-otxt",
                "-of", output_path.to_str().unwrap_or(""),
            ])
            .output()
            .map_err(|e| format!("Failed to run whisper: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("Whisper failed: {}", stderr));
        }

        // Read the output text file
        let txt_path = format!("{}.txt", output_path.to_str().unwrap_or(""));
        let text = std::fs::read_to_string(&txt_path)
            .unwrap_or_else(|_| {
                // Try without double extension
                std::fs::read_to_string(&output_path).unwrap_or_default()
            })
            .trim()
            .to_string();

        // Clean up temp files
        std::fs::remove_file(&txt_path).ok();
        std::fs::remove_file(&output_path).ok();

        // Get duration from WAV file
        let duration = get_wav_duration(wav_path).unwrap_or(0.0);

        Ok(TranscriptionResult {
            text,
            duration_seconds: duration,
            language: self.language.clone(),
        })
    }
}

/// Find the whisper.cpp binary. Checks:
/// 1. Bundled alongside the app binary (dist/whisper.exe)
/// 2. In the data directory
/// 3. On PATH
fn find_whisper_binary(data_dir: &Path) -> Result<PathBuf, String> {
    // Check alongside app binary
    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().unwrap_or(Path::new("."));
        for name in &["whisper-cli.exe", "whisper.exe", "main.exe"] {
            let p = dir.join(name);
            if p.exists() { return Ok(p); }
        }
    }

    // Check data directory and its subdirectories
    for subdir in &["", "models", "bin"] {
        let dir = if subdir.is_empty() { data_dir.to_path_buf() } else { data_dir.join(subdir) };
        for name in &["whisper-cli.exe", "whisper.exe", "main.exe"] {
            let p = dir.join(name);
            if p.exists() { return Ok(p); }
        }
    }

    // Check PATH
    if let Ok(output) = Command::new("where").arg("whisper-cli.exe").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path.lines().next().unwrap_or("")));
            }
        }
    }

    Err("Whisper binary not found. Place whisper-cli.exe in the app directory or install whisper.cpp.".into())
}

/// Find the Whisper model file.
fn find_model(data_dir: &Path) -> Result<PathBuf, String> {
    let model_names = [
        "ggml-large-v3.bin",
        "ggml-large-v3-turbo.bin",
        "ggml-medium.bin",
        "ggml-base.bin",
        "ggml-small.bin",
    ];

    // Check alongside app binary
    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().unwrap_or(Path::new("."));
        let models_dir = dir.join("models");
        for name in &model_names {
            let p = models_dir.join(name);
            if p.exists() { return Ok(p); }
            let p = dir.join(name);
            if p.exists() { return Ok(p); }
        }
    }

    // Check data directory
    let models_dir = data_dir.join("models");
    for name in &model_names {
        let p = models_dir.join(name);
        if p.exists() { return Ok(p); }
        let p = data_dir.join(name);
        if p.exists() { return Ok(p); }
    }

    Err("Whisper model not found. Place a ggml model file in the models/ directory.".into())
}

fn get_wav_duration(path: &str) -> Option<f64> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let samples = reader.len() as f64;
    Some(samples / spec.sample_rate as f64 / spec.channels as f64)
}
