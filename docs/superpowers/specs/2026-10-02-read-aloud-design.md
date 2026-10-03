# ALAN Echo — Read Aloud (design spec)

- **Date:** 2026-10-02
- **Status:** Approved by Alex 2026-10-02. Spikes A and B complete.
- **Scope:** Windows only, personal use first, behind the "Read Aloud (beta)" setting (off by default).
- **Visual design:** Claude Design canvas at https://claude.ai/artifact/7VdxFzrRDRXjP9nbT4rLP2

## TLDR

Press `Ctrl+Shift+E`, then click any word in the active window. Echo reads aloud from that word at 2.2x speed, and a brass highlight follows each spoken word on the page. Reading keeps going when you switch to other apps. A small "Now Reading" pill and the media keys control it. When it's not in use it costs about 0 MB: the voice engine, overlay, pill and mouse hook exist only while they're needed.

---

## 1. Goals and non-goals

**Goals**
- Go from "I'm looking at a document" to "it's reading to me" in one shortcut and one click.
- Start reading from any word, and jump to any other word mid-read.
- Show a word-by-word highlight on the page itself, so you're reading along while you listen.
- Let you leave the window and keep listening, without needing a big window open.
- Use almost no resources at idle, and stay light while reading.

**Non-goals (for now)**
- macOS. Echo stays cross-platform, but Read Aloud is cfg-gated to Windows, and the setting is hidden on Mac.
- Shipping it to customers: no licence notices, store copy or onboarding until it's proven.
- A browser extension or PDF drop zone. Windows' accessibility layer (UIA, the interface screen readers use to read text out of apps) already covers browsers and Edge's PDF viewer, so revisit these only if testing shows a gap.
- Exporting audio files or batch-converting whole books.

---

## 2. User flow

| Step | You do | Echo does |
|---|---|---|
| 1 | Open a doc, web page, PDF or email | Nothing. Read Aloud isn't loaded. |
| 2 | Press `Ctrl+Shift+E` | Enters **pick mode**: dims the screen slightly, shows the cursor hint "Click a word to start", warms up the UIA connection to the foreground window, and starts loading the voice engine in the background. |
| 3 | Hover | The word under the cursor gets a soft outline. |
| 4 | Click a word | Starts reading from that word. The brass highlight follows the spoken word, and the page scrolls to keep it in view. The **Now Reading pill** appears. |
| 5 | Switch to another app | Audio continues. The highlight hides while the source window is hidden or minimized and comes back when you return. The pill stays on top. |
| 6 | Press `Ctrl+Shift+E` again and click another word | Jumps there immediately, cutting the current audio. |
| 7 | Press `Esc` in pick mode | Cancels pick mode. Reading carries on if it was already going. |
| 8 | Click ✕ on the pill, or reach the end of the text | Stops reading and tears down the overlay and pill. After 2 idle minutes the voice engine unloads. |

**Edge cases**
- **The click lands on browser chrome or between lines:** the click is rejected unless it falls inside the returned word's rectangle (see §5). The cursor hint shakes and pick mode stays active.
- **No text found (UIA returns nothing and there's no OCR yet):** a toast says "Nothing readable here", and pick mode exits.
- **Pressing the hotkey while the engine is still loading:** the click is accepted, and audio starts as soon as the first chunk is ready. The pill shows a "warming up" state.

---

## 3. Controls

| Action | Control |
|---|---|
| Start / jump | `Ctrl+Shift+E` → click a word |
| Cancel pick mode | `Esc` |
| Pause / resume | Media Play/Pause key (keyboard or headset), or the pill's ⏯ |
| Previous / next sentence | Media Prev / Next keys, or the pill's ⏮ / ⏭ |
| Speed − / + | Pill buttons, in 0.1x steps from 1.0x to 3.5x. The speed persists. |
| Stop | The pill's ✕ |

- The media keys are registered only while a read session is active, so they don't fight Spotify and the like at other times.
- `Ctrl+Shift+E` doesn't clash with Echo's existing hotkeys (`Ctrl+Shift+Space`, `X`, `H` and `V`). It becomes rebindable when Echo gets hotkey rebinding.

### Now Reading pill
- Frameless, always on top, draggable, and it remembers its position. It never takes focus away from your work.
- Shows the current sentence (one or two lines, ellipsized) with the live word highlighted, plus ⏮ ⏯ ⏭, the speed and ✕.
- States: **warming up**, **reading**, **paused**, **error** (for example "Voice engine failed — retrying on CPU").
- Created when reading starts and destroyed when it stops.

---

## 4. Architecture

```
Ctrl+Shift+E ─► read_aloud.rs (session state machine)
                   │
     ┌─────────────┼───────────────────────┬──────────────────┐
     ▼             ▼                       ▼                  ▼
 uia_text.rs    tts.rs (Kokoro ONNX,     overlay window      pill window
 (text +        DML → CPU, lazy,         (click-through,     (Now Reading,
  word rects,   idle unload)              highlight rect)     controls)
  scroll)          │
     ▲             ▼
 ocr.rs        playback (cpal, via audio.rs) ── word timeline ─► overlay + pill
 (phase 2)
```

### New Rust modules
These follow Echo's flat one-file-per-feature layout and are declared at the top of `main.rs`.

| File | Responsibility |
|---|---|
| `read_aloud.rs` | Session state machine (Idle → Picking → Reading ↔ Paused → Idle). Owns the hotkey handler, the media-key registration, and the wiring between the modules. Tauri commands: `ra_pause`, `ra_resume`, `ra_skip`, `ra_set_speed`, `ra_stop`. |
| `uia_text.rs` | Connects to UIA. Handles `ElementFromPoint` → TextPattern ancestor → `RangeFromPoint` → expand to word; reads text forward in batches of about 2000 words; returns word rectangles; scrolls a word into view. Also holds the text normalization (§5). |
| `tts.rs` | Owns the Kokoro session: lazy load, DirectML with CPU fallback, sentence chunking, synthesis, the word timeline, and the 2-minute idle unload. Follows `whisper.rs`'s lifecycle pattern (start on demand, health check, clean shutdown on exit), but runs in-process through `ort` rather than as a child server. |
| `overlay.rs` | Creates and destroys the click-through overlay window, maps screen rectangles to the window, and hides or shows it when the source window's visibility changes. |
| `ocr.rs` (phase 2) | `Windows.Media.Ocr` on a capture of the window: returns words plus rectangles in the same shape as `uia_text`. |

`audio.rs` gains a streaming playback path. It currently only plays beeps; this adds a queue of PCM chunks with a position callback that drives the word timeline.

### New frontend pieces
- `src/components/ReadAloudOverlay.jsx`: pick-mode dim, hover outline and the reading highlight. Rendered in the overlay window.
- `src/components/NowReadingPill.jsx`: the pill.
- A "Read Aloud (beta)" section in `SettingsPanel.jsx`.
- Styling uses `src/tokens.css` (warm paper and the brass accent), so no new design tokens are needed.

### Tauri windows and capabilities
- **`ra-overlay`:**
  - transparent, undecorated, always on top, skipped in the taskbar, never focused
  - sized to the monitor of the source window
  - click-through via `set_ignore_cursor_events(true)` while reading
  - **not** click-through during pick mode, so the click is captured
- **`ra-pill`:** small, undecorated, always on top, skipped in the taskbar, never focused, draggable.
- **`src-tauri/capabilities/`:** add a capability scoped to `["ra-overlay", "ra-pill"]` with only the events and commands these windows need. Leave `default.json`, which is scoped to `["main"]`, unchanged.
- Both windows are created when needed and destroyed when finished. Neither exists at startup.

### Hotkey path rule (from the debug journal)
The `Ctrl+Shift+E` handler only flips state and posts work to a thread. No UIA calls, no model loading and no process spawning happen in the handler. The UIA warm-up and the engine load run on worker threads that start at the same moment.

---

## 5. Getting the text: UIA (Spike A results)

Spike A (2026-10-02) was a throwaway test. Its scripts are in the session scratchpad folder `spikeA/` (`probe.py`, `acc.py`, `scroll2.py`, `acro3.py`).

### Which apps work

| Target | TextPattern | Clicked word correct | Text past the screen | Word rectangles (empty off-screen) | Scroll into view |
|---|---|---|---|---|---|
| Chrome (no flags) | Yes | 40/40 | Yes (2000 words) | Yes / Yes | Word range: no-op. **Paragraph: works** |
| Edge (HTML) | Yes | 40/40 | Yes | Yes / Yes | Same as Chrome |
| Edge PDF viewer | Yes | 40/40 | Yes, clean paragraphs | Yes / Yes | Yes |
| Word | Yes | 40/40 | Yes, across pages | Yes / Yes | Yes |
| Notepad (Win11) | Yes | 40/40 | Yes | Yes / Yes | Yes |
| **Acrobat** | **No** | No | MSAA text only | Paragraph-level only | No |

**Verdict:** UIA is the primary path. Acrobat, canvas apps and scanned or image content go through **OCR** (phase 2), triggered by a generic "no TextPattern found → OCR" branch.

### Latency

| Operation | Time |
|---|---|
| `ElementFromPoint` | about 2–15 ms |
| `RangeFromPoint` + expand to word | about 0.2–1.8 ms |
| Per word (move, text, rectangles) | about 0.6–1.2 ms |
| Next 2000 words in one call | 5–37 ms |
| Whole document text | 0.3–28 ms |
| **First contact with Chromium** (it builds its accessibility tree) | **about 1.7–1.9 s**, then about 12–29 ms |

### Rules from Spike A's gotchas
1. **Warm up on the hotkey press:** start the UIA connection to the foreground window as soon as `Ctrl+Shift+E` is pressed, not on the click. Chromium's first query can come back empty, so **retry once**.
2. **Validate the click:** accept it only if the click point is inside the returned word's bounding rectangle. UIA snaps clicks between lines, between PDF pages and under the browser toolbar to the nearest word.
3. **Reject clicks over browser chrome:** text partly hidden under the toolbar is still reported, so rule 2 handles it. Additionally, ignore points outside the TextPattern element's bounding rectangle.
4. **Scroll in Chromium by paragraph:** call `ScrollIntoView` on the enclosing paragraph range, not the word range. Word and the Edge PDF viewer honour the word range.
5. **Merge hyphen splits:** HTML, Word and Notepad split "hyphen-ated" into 3 tokens, and the PDF viewer gives "hyphen-" + "ated". Merge them into a single spoken word, while keeping both rectangles for the highlight.
6. **Normalize text:** treat `\r` (Word's paragraph separator) as a paragraph break. In PDFs the last word on a line has no trailing space, so insert word boundaries from the range structure rather than relying on spaces.
7. **One source per read:** browsers often return a paragraph-level TextPattern source. Its text still runs through the whole document, but its ranges can't be compared with document-level ranges (the call errors). Keep the single TextPattern source picked at click time for the entire read, and never cross-compare.
8. **Edge PDF after a programmatic scroll:** `GetVisibleRanges` returned ranges without rectangles. Always walk forward from the clicked word's range instead of re-querying the visible text.
9. **Re-fetch rectangles each time a word becomes current:** never cache them, so scrolling or resizing can't leave the highlight in the wrong place. The cost is about 1 ms per word, well within budget.

---

## 6. Voice engine (Spike B results)

> Spike B verdict: **feasible.** DirectML q8 has ~7x headroom at 2.2x and the highlight lands within ~20 ms. Two engineering items: patch the model, and handle unknown words. Spike files: scratchpad `spikeB/` (`kk.py`, `patch.py`, `bench2.jsonl`, `align_results.json`, WAV samples).

| Question | Answer |
|---|---|
| Model | `onnx-community/Kokoro-82M-v1.0-ONNX-timestamped`, `onnx/model_quantized.onnx` (8-bit, 92 MB, Apache-2.0). It outputs per-phoneme **durations** alongside the audio. |
| Required patch | DirectML fails on a `ConvTranspose` with `output_padding`. `patch.py` rewrites it as equivalent padding, and CPU output is bit-identical. **Ship the patched file.** |
| Avoid | `q8f16`: produces NaN output on DirectML. |
| CPU fallback model | fp32 (about 330 MB, downloaded only if the GPU fails). CPU q8 has RTF 0.75, which is too slow for 2.2x. |
| Word timing | Word start = sum of rounded phoneme durations × 600 samples. Apply a fixed **−36 ms** offset. Each word ends where the next one starts. Measured against forced alignment: **median 8 ms, p90 26 ms, p99 51 ms** error. |
| Reaching 2.2x | **Do not use Kokoro's `speed` setting.** Each phoneme has a 25 ms minimum, so it tops out near 1.95x, and Whisper's word error rate rises to 17.3%. Instead synthesize at 1.0x and **time-stretch each chunk with WSOLA** (a pitch-keeping speed-up method); word error rate 0.9%. Timestamps ÷ stretch factor. Stretch per chunk, not the whole passage, or timing drifts. |
| Chunking | DirectML costs ~0.2 s per call regardless of length. First chunk: one clause of 12 words or fewer (~0.2 s to first audio). Then 25–50-word sentences, kept 1–2 ahead. A 50-word chunk takes 0.55 s to make and plays ~7.5 s at 2.2x. |
| Phonemizer (turns spelling into speech sounds) | **No espeak-ng (GPL-3).** Use `misaki-rs` 0.6 (MIT) with `default-features = false`, plus misaki's Apache-2.0 US dictionaries (~6 MB). Add our own **unknown-word fallback** (CMUdict lookup, then letter-by-letter spelling). Without it, misaki silently drops the word and its timing. |
| Runtime | `ort` crate with DirectML. DirectML is in maintenance mode at Microsoft, so the CPU fallback stays mandatory. |

**Measured (Python spike, 318-word passage; RTF = compute time ÷ audio length at 1.0x; must be under 0.45 for 2.2x)**

| Config | Load | First audio (cold / warm) | RTF | Peak RAM (Python process) | VRAM |
|---|---|---|---|---|---|
| **DML q8 (chosen)** | 1.8 s | 0.64 / 0.12 s | **0.059** | 517 MB | ~0.6–1.0 GB |
| DML fp32 | 1.4 s | 1.08 / 0.13 s | 0.151 | 583 MB | ~0.6–1.0 GB |
| CPU fp32, 8 threads (fallback) | 1.1 s | 0.96 s | 0.21 | 634 MB | – |

The RAM figures include the Python runtime. The Rust build must be re-measured; the target is in section 7.

**Cold start without breaking "0 MB at rest":** don't warm up at app launch. Start loading the model **the instant `Ctrl+Shift+E` is pressed**, because the 1–2 s the user spends aiming the click hides most of the ~1.8 s load.

**Engine rules**
- Runs in-process through ONNX Runtime (`ort` crate) with the **DirectML** execution provider. If DirectML fails to start or errors, fall back to **CPU** automatically and say so on the pill.
- **Lazy load:** the session is created on the first `Ctrl+Shift+E`, never at app start.
- **Idle unload:** 2 minutes after the last audio finishes, drop the session to free RAM and VRAM. The timer resets on every read or jump.
- **Fast start:** synthesize the first sentence (or a shorter first clause) first, then stay 1–2 sentences ahead of playback.
- **Word timeline:** each chunk returns `[(word_index, start_ms, end_ms)]`. Playback position maps to the current word; the overlay and pill subscribe to it.

---

## 7. Resource rules

| State | What exists | Target |
|---|---|---|
| Idle (Read Aloud on, not in use) | One registered hotkey. Nothing else. | **About 0 MB added**, 0% CPU, 0% GPU |
| Pick mode | Overlay window, UIA connection, engine loading in the background | Short-lived |
| Reading | Engine session, audio stream, overlay, pill, media-key registrations | **Under 500 MB RAM added** (Python spike: 517 MB incl. runtime; Rust must beat it), under ~1 GB VRAM, GPU busy only while synthesizing |
| 2 minutes after the last read | Engine dropped, windows destroyed | Back to idle |

- No polling loops. Source-window visibility comes from WinEvent hooks (`EVENT_OBJECT_LOCATIONCHANGE`, `EVENT_SYSTEM_MINIMIZESTART`/`END`, foreground change), installed only while reading.
- The mouse hook or capture exists only during pick mode.
- Synthesis stays at most 2 sentences ahead, so a big document isn't synthesized all at once.

---

## 8. Settings

These live in the "Read Aloud (beta)" section of `SettingsPanel.jsx` and are stored in `settings.json` through `settings.rs`.

| Key | Default | Notes |
|---|---|---|
| `read_aloud_enabled` | `false` | When off, the hotkey isn't registered and nothing loads |
| `read_aloud_speed` | `2.2` | 1.0–3.5 |
| `read_aloud_voice` | `af_heart` (Heart, US) | Dropdown of the bundled voices |
| `read_aloud_device` | `auto` | `auto` (DirectML, then CPU) / `gpu` / `cpu` |
| `read_aloud_pill_pos` | none | Saved when you drag the pill |

The section is hidden on macOS.

---

## 9. Error handling

| Failure | Behaviour |
|---|---|
| DirectML session fails to start or errors mid-read | Rebuild on the CPU once; the pill shows "Using CPU"; reading continues |
| CPU session also fails | Stop the session and show the toast "Voice engine failed", with details in the log |
| No TextPattern under the click | Phase 1: toast "Nothing readable here". Phase 2: OCR fallback |
| The source window closes mid-read | Finish the current sentence, then stop |
| The UIA call errors (stale range, cross-source comparison) | Stop the read cleanly, toast, log. Never panic (`panic = "abort"` in release would kill all of Echo). |
| An audio device change (headset unplugged) | Pause, and resume on the new default device when you press play |

Every error path ends in Idle with the windows destroyed. No half-alive overlay may remain.

---

## 10. Phases

| Phase | Delivers | You can... |
|---|---|---|
| **1. Core loop** | Settings toggle, hotkey, pick mode, UIA text, Kokoro (DirectML/CPU), streaming playback, word timeline, on-page highlight, pill, media keys, idle unload | Read Chrome, Edge, Edge PDF, Word and Notepad daily |
| **2. OCR fallback** | `ocr.rs` via `Windows.Media.Ocr`: words plus rectangles for apps without a TextPattern | Read Acrobat, scans and images (visible screen only) |
| **3. Polish** | Voice picker, device switch, highlight style options, pill refinements from real use | — |

The browser extension and PDF drop zone stay out unless a gap shows up in phase 1 use.

---

## 11. Testing

**Unit tests (`cargo test`, inline `#[cfg(test)] mod tests`, following the existing pattern)**
- Text normalization: `\r`, missing trailing spaces, whitespace collapse.
- Hyphen merge: the HTML 3-token form and the PDF `"hyphen-" + "ated"` form both become one spoken word that keeps both rectangles.
- Sentence chunking: abbreviations (e.g., "Dr.", "U.S."), decimals, quotes, a short first chunk.
- Word-timing mapping: phoneme durations → word spans, scaled by speed; monotonic, with no gaps or overlaps.
- Idle unload: an injected clock reaches 2 minutes and the session is dropped; a jump resets the timer.
- The session state machine: every transition, and every error path ends in Idle.

**Manual matrix (phase 1 exit)**

| Target | Start from a word | Jump | Highlight lines up | Auto-scroll | Leave and return |
|---|---|---|---|---|---|
| Chrome article | | | | | |
| Edge article | | | | | |
| Edge PDF | | | | | |
| Word .docx | | | | | |
| Notepad | | | | | |
| Acrobat (phase 2) | | | | | |

**Resource check (Task Manager plus `nvidia-smi`)**
- Idle with Read Aloud on: Echo's memory within about 5 MB of baseline, no GPU memory held.
- While reading: under 500 MB extra RAM, under ~1 GB VRAM.
- 2 minutes after stopping: back to baseline RAM and VRAM.

---

## 12. Visual design

Claude Design canvas: **https://claude.ai/artifact/7VdxFzrRDRXjP9nbT4rLP2**

| Artboard | Shows |
|---|---|
| Pick mode | Screen dim, cursor hint, hover outline on a word |
| On-page highlight | The brass word highlight on a real page, current sentence tint |
| Now Reading pill: states | Warming up / reading / paused / error |
| Settings section | The "Read Aloud (beta)" block in Echo's settings |

Everything uses Echo's existing tokens (`src/tokens.css`: warm paper, brass accent, Instrument Serif / Geist / Geist Mono).

---

## 13. Risks

| Risk | Mitigation |
|---|---|
| Word timing drifts at 2.2x | Measured p90 26 ms after per-chunk WSOLA. Fall back to sentence tint only if real use shows drift. |
| GPL phonemizer | Resolved: misaki-rs (MIT) without espeak, plus our own unknown-word fallback |
| DirectML deprecation | Microsoft maintenance mode; CPU fp32 fallback always available |
| Model patch | Patched ONNX is built once by a script kept in-repo and verified bit-identical on CPU |
| Chromium's first-contact lag of about 1.8 s | Warm up on the hotkey press, retry once, show "warming up" on the pill |
| Overlay drawn over the wrong monitor or out of step with DPI scaling | Convert UIA physical-pixel rectangles to the overlay's coordinates per monitor; include a mixed-DPI case in the manual matrix |
| Media keys clash with other players | Register them only during an active session |
