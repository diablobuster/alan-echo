//! Glitch hunt (phase 3 spec §6): drives the real private Read Aloud pipeline
//! (`ra_text::normalize` -> `ra_text::chunk` -> `prepare_chunk` -> `synth_inner`) over
//! a corpus, for the scripts in `tools/read-aloud/glitch-hunt/`. A child module of
//! `ra_tts` so it can reach those private functions. Both tests are `#[ignore]`.
//!
//! Static pass (no audio, seconds):
//!   GH_CORPUS=<corpus.tsv> GH_OUT=<static.jsonl> cargo test --release glitch_static -- --ignored --nocapture
//! Synth pass (real CPU model, minutes):
//!   GH_SAMPLE=<sample.tsv> GH_WAVDIR=<dir> cargo test --release glitch_synth -- --ignored --nocapture
//! `tools/read-aloud/glitch-hunt/run_hunt.sh` runs both and the analysis.
use super::*;
use misaki_rs::fallback::{Fallback, FallbackError};
use std::io::Write as _;

/// NameFallback that logs which branch answered each out-of-vocabulary token.
struct Recorder {
    names: Arc<Names>,
    inner: NameFallback,
    log: Arc<Mutex<Vec<(String, &'static str)>>>,
}

impl Fallback for Recorder {
    fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
        let r = self.inner.phonemize(word)?;
        let src = if self.names.hand(word).is_some() {
            "hand"
        } else if self.names.lookup(word).filter(|p| !p.is_empty()).is_some() {
            "cmu"
        } else if r == crate::ra_names::spell(word) {
            "spell"
        } else {
            "gru"
        };
        self.log.lock().push((word.to_string(), src));
        Ok(r)
    }
}

/// UIA-like raw tokens: whitespace-separated words keeping their trailing space; a
/// literal `\n` in the corpus becomes a paragraph break.
fn raw_tokens(s: &str) -> Vec<String> {
    s.split(' ')
        .filter(|t| !t.is_empty())
        .map(|t| if t == "\\n" { "\r\n".to_string() } else { format!("{t} ") })
        .collect()
}

fn esc(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// (id, source, text) rows of a tab-separated file named by env var `var`.
fn read_tsv(var: &str) -> Vec<(String, String, String)> {
    let p = std::env::var(var).unwrap_or_else(|_| panic!("set {var}"));
    std::fs::read_to_string(&p)
        .unwrap()
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            Some((it.next()?.to_string(), it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

/// One JSON line per chunk: the words (source, fed), every misaki token
/// [text, phonemes as the model gets them (Kokoro spelling, space runs collapsed),
/// owner word or -1, fallback branch, tag],
/// and the phoneme string the model is fed. A word inside a span has fed "".
#[test]
#[ignore]
fn glitch_static() {
    let rows = read_tsv("GH_CORPUS");
    let out_p = std::env::var("GH_OUT").expect("set GH_OUT");
    let names = Arc::new(Names::load(G2pEn::load().map(|g| Box::new(g) as Box<dyn Lts>)));
    let log = Arc::new(Mutex::new(Vec::new()));
    let fallback = Recorder { names: Arc::clone(&names), inner: NameFallback(Arc::clone(&names)), log: Arc::clone(&log) };
    let g = G2P::with_fallback(Language::EnglishUS, Some(Box::new(fallback)));
    let mut out = std::io::BufWriter::new(std::fs::File::create(&out_p).unwrap());
    let t = Instant::now();
    let mut nwords = 0usize;
    for (id, src, text) in &rows {
        let words = ra_text::normalize(&raw_tokens(text));
        for (ci, r) in ra_text::chunk(&words, 12, 30).iter().enumerate() {
            let ws: Vec<&str> = words[r.clone()].iter().map(|w| w.text.as_str()).collect();
            nwords += ws.len();
            log.lock().clear();
            let p = match prepare_chunk(&g, &names, &ws) {
                Ok(p) => p,
                Err(e) => {
                    writeln!(out, "{{\"id\":{},\"err\":{}}}", esc(id), esc(&e)).unwrap();
                    continue;
                }
            };
            let fb: HashMap<String, &'static str> = log.lock().iter().map(|(w, s)| (w.clone(), *s)).collect();
            let fed_join: String =
                p.ph.iter().map(|(_, ph, ws)| format!("{}{}", kokoro_phonemes(ph), if *ws { " " } else { "" })).collect();
            let tj: Vec<String> = p
                .tokens
                .iter()
                .zip(&p.owner)
                .map(|(tk, o)| {
                    format!(
                        "[{},{},{},{},{}]",
                        esc(&tk.text),
                        esc(&tk.phonemes.as_deref().map(|p| kokoro_phonemes(&collapse_spaces(p))).unwrap_or_else(|| "<NONE>".into())),
                        o.map_or(-1, |o| o as i64),
                        esc(fb.get(&tk.text).copied().unwrap_or("")),
                        esc(&tk.tag),
                    )
                })
                .collect();
            let wj: Vec<String> = ws.iter().zip(&p.fed).map(|(w, f)| format!("[{},{}]", esc(w), esc(f))).collect();
            writeln!(
                out,
                "{{\"id\":{},\"src\":{},\"chunk\":{},\"words\":[{}],\"toks\":[{}],\"fed\":{}}}",
                esc(id),
                esc(src),
                ci,
                wj.join(","),
                tj.join(","),
                esc(&fed_join)
            )
            .unwrap();
        }
    }
    out.flush().unwrap();
    println!("static: {} sentences, {} words in {:?} -> {}", rows.len(), nwords, t.elapsed(), out_p);
}

fn write_wav(pcm: &[f32], p: &Path) {
    let spec = hound::WavSpec { channels: 1, sample_rate: ra_timing::SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(p, spec).unwrap();
    for s in pcm {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// Each sample sentence through the real CPU model at 1.0x, chunk by chunk (with a
/// 100 ms gap), as `<id>.wav`. Existing WAVs are kept (the runner deletes them).
#[test]
#[ignore]
fn glitch_synth() {
    let rows = read_tsv("GH_SAMPLE");
    let dir = PathBuf::from(std::env::var("GH_WAVDIR").expect("set GH_WAVDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
    e.preload("af_heart", "cpu");
    let eng = e.acquire().expect("engine");
    let mut l = eng.lock();
    assert_eq!(l.device, Device::Cpu);
    let t = Instant::now();
    let gap = vec![0.0f32; (ra_timing::SR as usize) / 10];
    for (n, (id, _src, text)) in rows.iter().enumerate() {
        let p = dir.join(format!("{id}.wav"));
        if p.is_file() {
            continue;
        }
        let words = ra_text::normalize(&raw_tokens(text));
        let mut pcm: Vec<f32> = Vec::new();
        for r in ra_text::chunk(&words, 12, 30) {
            let ws: Vec<&str> = words[r].iter().map(|w| w.text.as_str()).collect();
            let mut a = 0;
            while a < ws.len() {
                match Engine::synth_inner(&mut l, &ws[a..], 1.0, true) {
                    Ok(c) => {
                        pcm.extend_from_slice(&c.pcm);
                        pcm.extend_from_slice(&gap);
                        a += c.covered.max(1);
                    }
                    Err(err) => {
                        println!("synth error {id}: {err}");
                        a = ws.len();
                    }
                }
            }
        }
        write_wav(&pcm, &p);
        if n % 25 == 0 {
            println!("{n}/{} {:?}", rows.len(), t.elapsed());
        }
    }
    println!("synth done: {} in {:?}", rows.len(), t.elapsed());
}
