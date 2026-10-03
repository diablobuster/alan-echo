# Read Aloud model prep

- **What:** `prepare_model.py` downloads Kokoro-82M (`onnx-community/Kokoro-82M-v1.0-ONNX-timestamped`), patches the q8 model's 3 grouped `ConvTranspose` nodes for DirectML, and copies the fp32 model, tokenizer and 3 voices into `%APPDATA%\ALAN Echo\models\kokoro\`.
- **Run:** `py -3.11 -m pip install huggingface_hub onnx && py -3.11 tools/read-aloud/prepare_model.py` (expect `patched 3 ConvTranspose nodes`, then `done: ...`).
- **Licence:** Kokoro-82M model weights and voices are Apache-2.0.
- **Re-run** after wiping the data dir. Never use the `q8f16` model.
- **Smoke test:** `cd src-tauri && cargo test ra_model::tests::ort_dml_smoke -- --ignored --nocapture`.

## ort 2.0.0-rc.13 notes (for the engine task)

Build config: `ort = { version = "=2.0.0-rc.13", features = ["directml"] }`. ort's default features
(`download-binaries`, `copy-dylibs`) stay on: ONNX Runtime is linked statically with the DirectML EP,
and `DirectML.dll` is copied next to the binary (`target/debug/DirectML.dll`). Ship that DLL with the app.

Working call shape (rc.13 differs from older sketches: EPs live in `ort::ep`, `DirectML` not
`DirectMLExecutionProvider`; `inputs()`/`outputs()` are methods and `name()`/`dtype()` are methods):

```rust
use ort::{ep, session::Session, value::Tensor};
let mut s = Session::builder()?
    .with_execution_providers([ep::DirectML::default().build().error_on_failure()])?
    .with_memory_pattern(false)?
    .with_parallel_execution(false)?
    .commit_from_file(&path)?;
let out = s.run(ort::inputs![
    "input_ids" => Tensor::from_array(([1usize, n], ids_i64))?,
    "style"     => Tensor::from_array(([1usize, 256usize], style_f32))?,
    "speed"     => Tensor::from_array(([1usize], vec![1.0f32]))?,
])?;
let (_shape, wav): (_, &[f32]) = out["waveform"].try_extract_tensor::<f32>()?;
let (_shape, dur): (_, &[f32]) = out["durations"].try_extract_tensor::<f32>()?;
```

Printed model I/O (model_q8_dml.onnx):

| name | dir | dtype | shape |
|---|---|---|---|
| `input_ids` | in | i64 | [1, sequence_length] |
| `style` | in | f32 | [1, 256] |
| `speed` | in | f32 | [1] |
| `waveform` | out | f32 | [1, num_samples] (24 kHz) |
| `durations` | out | **f32** (not i64) | [1, sequence_length], one per input id |

Smoke result (6 ids, zero style): session load 1.86 s, first run done at 2.43 s, 13 800 samples,
durations `[12.45, 1.90, 2.38, 2.10, 4.20, 1.26]`.
