//! ALAN Echo — Audio capture via a dedicated recording thread.
//! cpal::Stream isn't Send, so we run it on its own thread and communicate via channels.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use parking_lot::Mutex;

const TARGET_SAMPLE_RATE: u32 = 16000;
/// Hard backstop just past the UI's 5-minute cap — even if the frontend loses
/// track of a recording, memory use stays bounded.
const MAX_RECORD_SECONDS: usize = 310;

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

    let devices: Vec<DeviceInfo> = host
        .input_devices()
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
        .collect();

    Ok(devices)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordingResult {
    pub wav_path: Option<String>,
    pub duration_seconds: f64,
    pub has_speech: bool,
}

enum RecCmd {
    Start { device_name: Option<String>, tx_result: mpsc::Sender<Result<(), String>> },
    Stop { tx_result: mpsc::Sender<Result<RecordingResult, String>> },
    IsRecording { tx_result: mpsc::Sender<bool> },
    Level { tx_result: mpsc::Sender<f32> },
}

/// Thread-safe recorder handle. The actual cpal stream lives on a background
/// thread; the sender is wrapped in a Mutex because std's mpsc::Sender is
/// !Sync and Tauri state is shared across threads.
pub struct RecorderHandle {
    cmd_tx: Mutex<mpsc::Sender<RecCmd>>,
}

impl RecorderHandle {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();

        let handle = std::thread::Builder::new()
            .name("audio-recorder".into())
            .spawn(move || {
                recorder_thread(cmd_rx);
            });

        match handle {
            Ok(_) => {},
            Err(e) => log::error!("Failed to spawn recorder thread: {}", e),
        }

        Self { cmd_tx: Mutex::new(cmd_tx) }
    }

    fn send(&self, cmd: RecCmd) -> Result<(), String> {
        self.cmd_tx.lock().send(cmd).map_err(|_| "Recorder thread dead".to_string())
    }

    pub fn start(&self, device_name: Option<&str>) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.send(RecCmd::Start {
            device_name: device_name.map(|s| s.to_string()),
            tx_result: tx,
        })?;
        rx.recv().map_err(|_| "Recorder thread dead".to_string())?
    }

    pub fn stop(&self) -> Result<RecordingResult, String> {
        let (tx, rx) = mpsc::channel();
        self.send(RecCmd::Stop { tx_result: tx })?;
        rx.recv().map_err(|_| "Recorder thread dead".to_string())?
    }

    pub fn is_recording(&self) -> bool {
        let (tx, rx) = mpsc::channel();
        if self.send(RecCmd::IsRecording { tx_result: tx }).is_ok() {
            rx.recv().unwrap_or(false)
        } else {
            false
        }
    }

    pub fn current_level(&self) -> f32 {
        let (tx, rx) = mpsc::channel();
        if self.send(RecCmd::Level { tx_result: tx }).is_ok() {
            rx.recv().unwrap_or(0.0)
        } else {
            0.0
        }
    }
}

fn recorder_thread(cmd_rx: mpsc::Receiver<RecCmd>) {
    let mut stream: Option<cpal::Stream> = None;
    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let has_speech: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let rms_level: Arc<Mutex<f32>> = Arc::new(Mutex::new(0.0));
    let mut device_rate: u32 = TARGET_SAMPLE_RATE;
    let mut recording = false;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            RecCmd::Start { device_name, tx_result } => {
                if recording {
                    let _ = tx_result.send(Err("Already recording".into()));
                    continue;
                }

                let host = cpal::default_host();
                let device = if let Some(ref name) = device_name {
                    host.input_devices()
                        .ok()
                        .and_then(|mut devs| devs.find(|d| d.name().map(|n| n == *name).unwrap_or(false)))
                        .or_else(|| host.default_input_device())
                } else {
                    host.default_input_device()
                };

                let device = match device {
                    Some(d) => d,
                    None => { let _ = tx_result.send(Err("No input device".into())); continue; }
                };

                let config = match device.default_input_config() {
                    Ok(c) => c,
                    Err(e) => { let _ = tx_result.send(Err(e.to_string())); continue; }
                };

                device_rate = config.sample_rate().0;
                let channels = config.channels() as usize;

                samples.lock().clear();
                *has_speech.lock() = false;
                *rms_level.lock() = 0.0;

                let threshold = 0.001f32;
                let max_samples = device_rate as usize * MAX_RECORD_SECONDS;
                let sample_format = config.sample_format();
                let stream_config: cpal::StreamConfig = config.into();

                log::info!("Audio device: channels={}, rate={}, format={:?}", channels, device_rate, sample_format);

                let built = match sample_format {
                    cpal::SampleFormat::F32 => {
                        let smp = Arc::clone(&samples);
                        let hs = Arc::clone(&has_speech);
                        let rl = Arc::clone(&rms_level);
                        device.build_input_stream(
                            &stream_config,
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                let mono: Vec<f32> = if channels > 1 {
                                    data.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect()
                                } else {
                                    data.to_vec()
                                };
                                let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len().max(1) as f32).sqrt();
                                *rl.lock() = rms;
                                if rms > threshold { *hs.lock() = true; }
                                let mut buf = smp.lock();
                                if buf.len() < max_samples {
                                    let take = mono.len().min(max_samples - buf.len());
                                    buf.extend_from_slice(&mono[..take]);
                                }
                            },
                            |err| log::error!("Audio error (f32): {}", err),
                            None,
                        )
                    }
                    cpal::SampleFormat::I16 => {
                        let smp = Arc::clone(&samples);
                        let hs = Arc::clone(&has_speech);
                        let rl = Arc::clone(&rms_level);
                        device.build_input_stream(
                            &stream_config,
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                let mono: Vec<f32> = if channels > 1 {
                                    data.chunks(channels).map(|c| {
                                        c.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32
                                    }).collect()
                                } else {
                                    data.iter().map(|&s| s as f32 / 32768.0).collect()
                                };
                                let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len().max(1) as f32).sqrt();
                                *rl.lock() = rms;
                                if rms > threshold { *hs.lock() = true; }
                                let mut buf = smp.lock();
                                if buf.len() < max_samples {
                                    let take = mono.len().min(max_samples - buf.len());
                                    buf.extend_from_slice(&mono[..take]);
                                }
                            },
                            |err| log::error!("Audio error (i16): {}", err),
                            None,
                        )
                    }
                    cpal::SampleFormat::U16 => {
                        let smp = Arc::clone(&samples);
                        let hs = Arc::clone(&has_speech);
                        let rl = Arc::clone(&rms_level);
                        device.build_input_stream(
                            &stream_config,
                            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                                let mono: Vec<f32> = if channels > 1 {
                                    data.chunks(channels).map(|c| {
                                        c.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).sum::<f32>() / channels as f32
                                    }).collect()
                                } else {
                                    data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect()
                                };
                                let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len().max(1) as f32).sqrt();
                                *rl.lock() = rms;
                                if rms > threshold { *hs.lock() = true; }
                                let mut buf = smp.lock();
                                if buf.len() < max_samples {
                                    let take = mono.len().min(max_samples - buf.len());
                                    buf.extend_from_slice(&mono[..take]);
                                }
                            },
                            |err| log::error!("Audio error (u16): {}", err),
                            None,
                        )
                    }
                    _ => {
                        let _ = tx_result.send(Err(format!("Unsupported audio format: {:?}", sample_format)));
                        continue;
                    }
                };

                match built {
                    Ok(s) => {
                        if let Err(e) = s.play() {
                            let _ = tx_result.send(Err(e.to_string()));
                            continue;
                        }
                        stream = Some(s);
                        recording = true;
                        let _ = tx_result.send(Ok(()));
                    }
                    Err(e) => { let _ = tx_result.send(Err(e.to_string())); }
                }
            }

            RecCmd::Stop { tx_result } => {
                if !recording {
                    let _ = tx_result.send(Err("Not recording".into()));
                    continue;
                }
                drop(stream.take());
                recording = false;

                let raw = std::mem::take(&mut *samples.lock());
                let speech = *has_speech.lock();

                if raw.is_empty() {
                    let _ = tx_result.send(Ok(RecordingResult {
                        wav_path: None, duration_seconds: 0.0, has_speech: false,
                    }));
                    continue;
                }

                let resampled = if device_rate != TARGET_SAMPLE_RATE {
                    resample(&raw, device_rate, TARGET_SAMPLE_RATE)
                } else {
                    raw
                };

                let duration = resampled.len() as f64 / TARGET_SAMPLE_RATE as f64;

                let data_dir = dirs::data_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("ALAN Echo");
                std::fs::create_dir_all(&data_dir).ok();
                let wav_path = data_dir.join(format!("recording_{}.wav", uuid::Uuid::new_v4()));

                let spec = WavSpec {
                    channels: 1, sample_rate: TARGET_SAMPLE_RATE,
                    bits_per_sample: 16, sample_format: hound::SampleFormat::Int,
                };

                match WavWriter::create(&wav_path, spec) {
                    Ok(mut writer) => {
                        let mut write_err = None;
                        for &s in &resampled {
                            if let Err(e) = writer.write_sample((s * 32767.0).clamp(-32768.0, 32767.0) as i16) {
                                write_err = Some(e);
                                break;
                            }
                        }
                        if let Some(e) = write_err {
                            let _ = tx_result.send(Err(format!("WAV write error: {}", e)));
                        } else if let Err(e) = writer.finalize() {
                            let _ = tx_result.send(Err(format!("WAV finalize error: {}", e)));
                        } else {
                            let _ = tx_result.send(Ok(RecordingResult {
                                wav_path: Some(wav_path.to_string_lossy().to_string()),
                                duration_seconds: duration,
                                has_speech: speech,
                            }));
                        }
                    }
                    Err(e) => { let _ = tx_result.send(Err(e.to_string())); }
                }
            }

            RecCmd::IsRecording { tx_result } => {
                let _ = tx_result.send(recording);
            }

            RecCmd::Level { tx_result } => {
                let level = if recording { (*rms_level.lock() * 5.0).min(1.0) } else { 0.0 };
                let _ = tx_result.send(level);
            }
        }
    }
}

fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    (0..output_len).map(|i| {
        let src = i as f64 * ratio;
        let idx = src as usize;
        let frac = (src - idx as f64) as f32;
        let s0 = samples[idx.min(samples.len() - 1)];
        let s1 = samples[(idx + 1).min(samples.len() - 1)];
        s0 + (s1 - s0) * frac
    }).collect()
}
