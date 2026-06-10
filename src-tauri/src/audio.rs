//! ALAN Echo — Audio capture, recording, WAV generation.
//! Uses cpal for cross-platform audio input.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate};
use hound::{WavSpec, WavWriter};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

const TARGET_SAMPLE_RATE: u32 = 16000;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub index: usize,
    pub is_default: bool,
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn list_input_devices() -> Result<Vec<DeviceInfo>, String> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    host.input_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .enumerate()
        .filter_map(|(i, device)| {
            let name = device.name().ok()?;
            let config = device.default_input_config().ok()?;
            Some(DeviceInfo {
                is_default: name == default_name,
                name,
                index: i,
                sample_rate: config.sample_rate().0,
                channels: config.channels(),
            })
        })
        .collect::<Vec<_>>()
        .pipe_ok()
}

trait PipeOk: Sized {
    fn pipe_ok(self) -> Result<Self, String> { Ok(self) }
}
impl<T> PipeOk for T {}

/// Shared recording state.
pub struct Recorder {
    samples: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    recording: bool,
    device_sample_rate: u32,
    has_speech: Arc<Mutex<bool>>,
    rms_level: Arc<Mutex<f32>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            samples: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            recording: false,
            device_sample_rate: TARGET_SAMPLE_RATE,
            has_speech: Arc::new(Mutex::new(false)),
            rms_level: Arc::new(Mutex::new(0.0)),
        }
    }

    pub fn start(&mut self, device_name: Option<&str>) -> Result<(), String> {
        if self.recording {
            return Err("Already recording".into());
        }

        let host = cpal::default_host();
        let device = if let Some(name) = device_name {
            host.input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| format!("Device '{}' not found", name))?
        } else {
            host.default_input_device()
                .ok_or("No default input device")?
        };

        let config = device.default_input_config().map_err(|e| e.to_string())?;
        self.device_sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        self.samples.lock().clear();
        *self.has_speech.lock() = false;
        *self.rms_level.lock() = 0.0;

        let samples = Arc::clone(&self.samples);
        let has_speech = Arc::clone(&self.has_speech);
        let rms_level = Arc::clone(&self.rms_level);
        let silence_threshold = 0.005f32;

        let stream = match config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Downmix to mono if needed
                    let mono: Vec<f32> = if channels > 1 {
                        data.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect()
                    } else {
                        data.to_vec()
                    };
                    let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
                    *rms_level.lock() = rms;
                    if rms > silence_threshold {
                        *has_speech.lock() = true;
                    }
                    samples.lock().extend_from_slice(&mono);
                },
                |err| log::error!("Audio stream error: {}", err),
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mono: Vec<f32> = if channels > 1 {
                        data.chunks(channels).map(|c| {
                            c.iter().map(|s| *s as f32 / 32768.0).sum::<f32>() / channels as f32
                        }).collect()
                    } else {
                        data.iter().map(|s| *s as f32 / 32768.0).collect()
                    };
                    let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
                    *rms_level.lock() = rms;
                    if rms > silence_threshold {
                        *has_speech.lock() = true;
                    }
                    samples.lock().extend_from_slice(&mono);
                },
                |err| log::error!("Audio stream error: {}", err),
                None,
            ),
            _ => return Err("Unsupported sample format".into()),
        }.map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        self.recording = true;
        log::info!("Recording started (rate={}Hz, ch={})", self.device_sample_rate, channels);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<RecordingResult, String> {
        if !self.recording {
            return Err("Not recording".into());
        }
        // Drop the stream to stop recording
        self.stream.take();
        self.recording = false;

        let samples = self.samples.lock().clone();
        let has_speech = *self.has_speech.lock();

        if samples.is_empty() {
            return Ok(RecordingResult {
                wav_path: None,
                duration_seconds: 0.0,
                has_speech: false,
            });
        }

        // Resample to 16kHz if needed
        let resampled = if self.device_sample_rate != TARGET_SAMPLE_RATE {
            resample(&samples, self.device_sample_rate, TARGET_SAMPLE_RATE)
        } else {
            samples
        };

        let duration = resampled.len() as f64 / TARGET_SAMPLE_RATE as f64;

        // Write WAV file
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ALAN Echo");
        std::fs::create_dir_all(&data_dir).ok();
        let wav_path = data_dir.join("recording.wav");

        let spec = WavSpec {
            channels: 1,
            sample_rate: TARGET_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&wav_path, spec).map_err(|e| e.to_string())?;
        for &sample in &resampled {
            let val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;

        log::info!("Recording stopped: {:.1}s, speech={}", duration, has_speech);
        Ok(RecordingResult {
            wav_path: Some(wav_path.to_string_lossy().to_string()),
            duration_seconds: duration,
            has_speech,
        })
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn current_level(&self) -> f32 {
        (*self.rms_level.lock() * 5.0).min(1.0)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordingResult {
    pub wav_path: Option<String>,
    pub duration_seconds: f64,
    pub has_speech: bool,
}

/// Simple linear resampling.
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);
    for i in 0..output_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;
        let s0 = samples[idx.min(samples.len() - 1)];
        let s1 = samples[(idx + 1).min(samples.len() - 1)];
        output.push(s0 + (s1 - s0) * frac as f32);
    }
    output
}
