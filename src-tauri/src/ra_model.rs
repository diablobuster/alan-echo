//! Read Aloud — Kokoro model file locations (data dir, not bundled).
use std::path::{Path, PathBuf};

pub struct Files {
    pub q8_dml: PathBuf,
    pub fp32: PathBuf,
    pub tokenizer: PathBuf,
    pub voices_dir: PathBuf,
}

pub fn model_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("kokoro")
}

pub fn files(data_dir: &Path) -> Files {
    let d = model_dir(data_dir);
    Files {
        q8_dml: d.join("model_q8_dml.onnx"),
        fp32: d.join("model_fp32.onnx"),
        tokenizer: d.join("tokenizer.json"),
        voices_dir: d.join("voices"),
    }
}

pub fn is_installed(data_dir: &Path) -> bool {
    let f = files(data_dir);
    f.q8_dml.exists() && f.tokenizer.exists() && f.voices_dir.join("af_heart.bin").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_live_under_models_kokoro() {
        let f = files(Path::new("C:/x"));
        assert!(f.q8_dml.ends_with("models/kokoro/model_q8_dml.onnx"));
        assert!(f.voices_dir.ends_with("models/kokoro/voices"));
    }

    #[test]
    fn not_installed_in_empty_dir() {
        let d = std::env::temp_dir().join(format!("ra-empty-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(!is_installed(&d));
    }

    /// Smoke: DirectML session loads the patched model and runs one call.
    /// Ignored by default (needs the prepared model). Run with:
    /// cargo test ra_model::tests::ort_dml_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn ort_dml_smoke() {
        use ort::{ep, session::Session, value::Tensor};
        let t0 = std::time::Instant::now();
        let data = dirs::data_dir().unwrap().join("ALAN Echo");
        let f = files(&data);
        let mut s = Session::builder()
            .unwrap()
            .with_execution_providers([ep::DirectML::default().build().error_on_failure()])
            .unwrap()
            .with_memory_pattern(false)
            .unwrap()
            .with_parallel_execution(false)
            .unwrap()
            .commit_from_file(&f.q8_dml)
            .unwrap();
        println!("session load: {:?}", t0.elapsed());
        for i in s.inputs().iter() {
            println!("input: {} {:?}", i.name(), i.dtype());
        }
        for o in s.outputs().iter() {
            println!("output: {} {:?}", o.name(), o.dtype());
        }
        let ids: Vec<i64> = vec![0, 50, 83, 54, 156, 0];
        let n = ids.len();
        let style = vec![0.0f32; 256];
        let out = s
            .run(ort::inputs![
                "input_ids" => Tensor::from_array(([1usize, n], ids)).unwrap(),
                "style" => Tensor::from_array(([1usize, 256usize], style)).unwrap(),
                "speed" => Tensor::from_array(([1usize], vec![1.0f32])).unwrap(),
            ])
            .unwrap();
        println!("first run: {:?}", t0.elapsed());
        assert!(out.len() >= 2, "expected waveform + durations");
        let (wshape, wav) = out["waveform"].try_extract_tensor::<f32>().unwrap();
        println!("waveform shape {:?}, {} samples", wshape, wav.len());
        assert!(!wav.is_empty());
        let (dshape, dur) = out["durations"].try_extract_tensor::<f32>().unwrap();
        println!("durations shape {:?}: {:?}", dshape, dur);
        assert_eq!(dur.len(), n, "one duration per input token");
    }
}
