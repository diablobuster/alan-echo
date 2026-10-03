//! Read Aloud — streaming playback on the default output device.
//! cpal::Stream isn't Send, so it lives on its own thread (same pattern as audio.rs).
#![allow(dead_code)]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

#[derive(Default)]
pub struct Source {
    pub buf: VecDeque<f32>,
    pub frac: f64,
    pub paused: bool,
}

/// Fill `out` (interleaved, `channels`) from 24 kHz mono `src`, advancing `step` source samples per frame.
pub fn mix_into(out: &mut [f32], channels: usize, step: f64, src: &mut Source) -> u64 {
    if channels == 0 {
        return 0;
    }
    let mut used = 0u64;
    for frame in out.chunks_mut(channels) {
        let v = if src.paused || src.buf.is_empty() {
            0.0
        } else {
            let a = src.buf[0];
            let b = *src.buf.get(1).unwrap_or(&a);
            let v = a + (b - a) * src.frac as f32;
            src.frac += step;
            while src.frac >= 1.0 && !src.buf.is_empty() {
                src.buf.pop_front();
                src.frac -= 1.0;
                used += 1;
            }
            v
        };
        for s in frame.iter_mut() {
            *s = v;
        }
    }
    used
}

pub struct Player {
    src: Arc<Mutex<Source>>,
    pos: Arc<AtomicU64>,
    pushed: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
    stop_tx: mpsc::Sender<()>,
}

impl Player {
    pub fn start() -> Result<Player, String> {
        let src = Arc::new(Mutex::new(Source::default()));
        let pos = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ok_tx, ok_rx) = mpsc::channel::<Result<(), String>>();
        let (s2, p2, f2) = (Arc::clone(&src), Arc::clone(&pos), Arc::clone(&failed));
        std::thread::Builder::new()
            .name("ra-player".into())
            .spawn(move || {
                let run = || -> Result<cpal::Stream, String> {
                    let dev = cpal::default_host()
                        .default_output_device()
                        .ok_or("no output device")?;
                    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
                    if cfg.sample_format() != cpal::SampleFormat::F32 {
                        return Err(format!("unsupported output format {:?}", cfg.sample_format()));
                    }
                    let ch = cfg.channels() as usize;
                    let step = 24_000.0 / cfg.sample_rate().0 as f64;
                    let f3 = Arc::clone(&f2);
                    let stream = dev
                        .build_output_stream(
                            &cfg.config(),
                            move |out: &mut [f32], _| {
                                let used = mix_into(out, ch, step, &mut s2.lock());
                                p2.fetch_add(used, Ordering::Relaxed);
                            },
                            move |e| {
                                log::warn!("read-aloud: output stream error: {e}");
                                f3.store(true, Ordering::Relaxed);
                            },
                            None,
                        )
                        .map_err(|e| e.to_string())?;
                    stream.play().map_err(|e| e.to_string())?;
                    Ok(stream)
                };
                match run() {
                    Ok(stream) => {
                        ok_tx.send(Ok(())).ok();
                        let _ = stop_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        ok_tx.send(Err(e)).ok();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        ok_rx.recv().map_err(|e| e.to_string())??;
        Ok(Player { src, pos, pushed: Arc::new(AtomicU64::new(0)), failed, stop_tx })
    }

    pub fn push(&self, pcm: &[f32]) {
        self.src.lock().buf.extend(pcm.iter().copied());
        self.pushed.fetch_add(pcm.len() as u64, Ordering::Relaxed);
    }
    pub fn clear(&self) {
        let mut s = self.src.lock();
        s.buf.clear();
        s.frac = 0.0;
        self.pos.store(0, Ordering::Relaxed);
        self.pushed.store(0, Ordering::Relaxed);
    }
    pub fn set_paused(&self, p: bool) {
        self.src.lock().paused = p;
    }
    pub fn is_paused(&self) -> bool {
        self.src.lock().paused
    }
    pub fn position(&self) -> u64 {
        self.pos.load(Ordering::Relaxed)
    }
    pub fn queued_end(&self) -> u64 {
        self.pushed.load(Ordering::Relaxed)
    }
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumes_at_ratio_and_duplicates_channels() {
        let mut src = Source::default();
        src.buf.extend((0..100).map(|i| i as f32));
        let mut out = vec![0.0f32; 2 * 40]; // 40 stereo frames at 48k
        let used = mix_into(&mut out, 2, 0.5, &mut src); // 24k -> 48k
        assert_eq!(used, 20);
        assert_eq!(out[0], out[1]);
    }

    #[test]
    fn underrun_outputs_silence() {
        let mut src = Source::default();
        let mut out = vec![1.0f32; 8];
        assert_eq!(mix_into(&mut out, 2, 0.5, &mut src), 0);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn paused_outputs_silence_and_keeps_buffer() {
        let mut src = Source::default();
        src.paused = true;
        src.buf.extend([0.5f32; 10]);
        let mut out = vec![1.0f32; 8];
        assert_eq!(mix_into(&mut out, 2, 0.5, &mut src), 0);
        assert_eq!(src.buf.len(), 10);
    }

    #[test]
    #[ignore = "manual: needs an audio output device"]
    fn manual_plays_tone_and_position_advances() {
        let p = Player::start().expect("start player");
        let tone: Vec<f32> = (0..24_000)
            .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 24_000.0).sin())
            .collect();
        p.push(&tone);
        assert_eq!(p.queued_end(), 24_000);
        std::thread::sleep(std::time::Duration::from_millis(1300));
        let pos = p.position();
        println!("position after 1.3s = {pos}");
        assert!((21_600..=26_400).contains(&pos), "position {pos}");
        assert!(!p.failed());
    }
}
