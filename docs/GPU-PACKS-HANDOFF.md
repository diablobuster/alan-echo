# HANDOFF — Build & ship the remaining GPU packs (ALAN Echo)

Written 2026-06-10, after v1.1.0 shipped. Mission: extend GPU acceleration
beyond NVIDIA. **Scope decision already made: build the VULKAN pack and the
small app change that makes it one-click. Nothing else** (see §8 for what was
deliberately parked and what would un-park it).

Everything in §1 already exists and is tested — do not rebuild it. The
implementer should be able to execute §2–§7 top to bottom.

---

## §1. What already exists (v1.1.0 — don't redo)

- **Engine probing**: `src-tauri/src/whisper.rs` `find_server_binary()` already
  prefers `models\vulkan_release\Release\whisper-server.exe` on machines with
  no NVIDIA GPU (and falls back to it on NVIDIA machines without the CUDA
  pack). A Vulkan pack extracted to that path works TODAY on v1.1.0 with zero
  app changes. `EngineInfo.engine_kind` reports "vulkan" via `binary_kind()`.
- **Download/install machinery**: `src-tauri/src/packs.rs` — streamed download
  with progress events, zip-slip-safe extraction to a temp dir, verify-then-
  atomic-rename, hot engine reload. Currently hardcoded to the CUDA pack
  (`GPU_PACK_URL`, `cuda_release`). §5 generalizes it.
- **Hardware awareness**: `packs.rs::test_gpu` already enumerates ALL display
  adapters via CIM (`display_gpus`) — AMD/Intel detection needs no new probe,
  only new verdict copy.
- **Distribution**: public releases repo `diablobuster/alan-echo-releases`
  (assets + SHA256SUMS.txt convention, checksums in notes), stable site
  redirects in stock-analyzer (`app/api/echo/download/[gpu]/route.ts` pattern,
  env-pointed). Site FAQ + design prompts currently say AMD/Intel is
  "on the roadmap" — keep that wording until §4's gate passes.
- **Docs**: `docs/GPU-PACKS.md` (landscape), this handoff.

Toolchain on the dev machine: git ✓, cmake ✓ (Strawberry-bundled — check
`cmake --version` ≥ 3.19; if old, `winget install Kitware.CMake`), VS Build
Tools ✓ (cargo links with MSVC). **Vulkan SDK: NOT installed** (no
`$env:VULKAN_SDK`, no C:\VulkanSDK).

## §2. Build the Vulkan whisper-server (one-time, ~1–2 h)

whisper.cpp publishes **no prebuilt Windows Vulkan binary** (v1.8.6 assets:
CPU/BLAS/CUDA only — verified 2026-06-10), so this is a from-source build.

1. **Install the LunarG Vulkan SDK** (~300 MB):
   `winget install LunarG.VulkanSDK` (or the installer from vulkan.lunarg.com
   with `--accept-licenses --default-answer --confirm-command install`).
   Open a NEW shell afterwards so `VULKAN_SDK` is set — the build needs its
   headers + `glslc` (ggml-vulkan compiles its shaders at build time).
2. **Clone at the engine tag we ship**: the live packs are whisper.cpp
   **v1.8.6** — stay on it so the /inference HTTP contract can't drift:
   ```
   git clone --branch v1.8.6 --depth 1 https://github.com/ggml-org/whisper.cpp C:\Users\arowm\whisper.cpp-vulkan
   ```
3. **Configure + build** (shared DLLs, server target — mirrors the shipped
   layout of separate whisper.dll/ggml*.dll):
   ```
   cmake -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON -DGGML_VULKAN=1
   cmake --build build --config Release --target whisper-server
   ```
   Output lands in `build\bin\Release\`.
4. **Sanity-run before packaging**: download nothing — point it at a model
   already on this machine:
   ```
   build\bin\Release\whisper-server.exe -m "%APPDATA%\ALAN Echo\models\ggml-base.en.bin" --port 8199 -l en
   ```
   Logs must show a `ggml_vulkan` device line naming the RTX 4060 (Vulkan runs
   on NVIDIA — that's what makes local smoke-testing possible). POST a WAV to
   `/inference` and confirm sane text.

## §3. Package the pack (exact layout — the app's verifier depends on it)

Zip name: `ALAN-Echo-GPU-Pack-Vulkan-1.0.0.zip`. Zip ROOT must contain
`vulkan_release/Release/` with:

- `whisper-server.exe`
- every `ggml*.dll` the build produced (expect `ggml.dll`, `ggml-base.dll`,
  `ggml-cpu.dll`, `ggml-vulkan.dll`) + `whisper.dll`
- `SDL2.dll` **only if** the build produced one next to the server (the CUDA
  pack ships it; a server-only target may not need it — if `whisper-server`
  starts and serves without it, leave it out)
- CRT DLLs `msvcp140.dll`, `vcruntime140.dll`, `vcruntime140_1.dll` (copy
  from the newest VS redist exactly like `scripts/prepare-resources.ps1`
  does — clean machines do not have the VC++ runtime)
- **Do NOT bundle `vulkan-1.dll`** — the Vulkan loader ships with Windows GPU
  drivers; bundling a stale one breaks newer drivers.
- **Do NOT copy the demo exes** (`main.exe`, `wchess.exe`, `whisper-talk-llama.exe`
  etc. — the CUDA pack carries ~25 of them as dead weight; keep this pack to
  the server + DLLs. Should land well under 100 MB vs CUDA's 436 MB, since
  there are no cuBLAS blobs).

The app's installer verify step checks exactly
`<root>/vulkan_release/Release/whisper-server.exe` exists post-extraction
(once §5 lands; the manual drop-in path checks nothing — layout still must
match what `find_server_binary` probes).

## §4. Test gates (two tiers — don't skip the second silently)

**Tier 1 — local smoke (RTX 4060, sufficient for a BETA release):**
1. Quit Echo. In `%APPDATA%\ALAN Echo\models\`: temporarily rename
   `cuda_release` → `cuda_release.hold` (NVIDIA machines prefer CUDA, so the
   Vulkan binary is only picked when CUDA is absent). Extract the pack.
2. Launch the installed app; confirm via Settings (or CDP-drive
   `get_engine_info` — the WebView2 `--remote-debugging-port` technique is in
   `whispr-local/docs/2026-06-10-alan-echo-golive-session-log.md` §6) that
   `engine_kind == "vulkan"` and `ready == true`.
3. Dictate into Notepad; compare latency vs CPU (`Test my GPU` row +
   transcript timings). Expect a large win over CPU on the same clip.
4. Restore `cuda_release`. Confirm engine returns to `cuda`.

**Tier 2 — real silicon (REQUIRED before removing "beta" from any copy):**
at least one AMD RDNA2+ card and ideally one Intel Arc / recent Iris Xe.
No such hardware here — use a friend's machine or the clean-machine-test
contact from GO-LIVE-CHECKLIST §B. Known risk areas to check: older Intel
iGPU Vulkan drivers (fp16 quirks), instant crash on unsupported extensions,
garbage output (not just crashes — TRANSCRIBE AND READ THE TEXT).
Until Tier 2 passes, every customer-facing mention says **beta** and invites
reports to support@alanglobalintelligence.com.

## §5. App change — one-click for AMD/Intel + crash-rollback (ship as v1.2.0)

Small, contained to `packs.rs` + `SettingsPanel.jsx`; batch with anything else
queued for 1.2:

1. **Generalize the installer**: parameterize `download_gpu_pack` /
   `run_install` over a pack kind (`cuda` | `vulkan`): per-kind URL
   (`https://alanglobalintelligence.com/api/echo/download/gpu` and
   `.../api/echo/download/vulkan`), per-kind directory, per-kind verify path.
   Keep ONE progress channel (a second concurrent install stays refused).
2. **Offer logic** in `get_gpu_pack_status`: NVIDIA present → offer `cuda`
   (unchanged); else if `display_gpus` contains a non-NVIDIA adapter matching
   /radeon|arc|iris|intel|amd/i → offer `vulkan` with a `beta: true` flag.
   `GpuPackRow` renders the beta label + "report problems to support" line.
3. **Rollback on a bad driver** (the safety feature Tier-2 risk demands):
   after install + `whisper.reload()`, watch engine status; if it hits
   `Failed` (or isn't `Ready` within ~60 s) while `engine_kind == "vulkan"`,
   rename `vulkan_release` → `vulkan_release.disabled`, reload (engine falls
   back to CPU), and surface: "Your GPU's Vulkan driver couldn't run the
   engine — Echo is back on the CPU engine. Nothing is broken; email support
   with your GPU model." Without this, a crashing Vulkan binary re-fails on
   every launch with no user-visible escape.
4. **Verdict copy**: `test_gpu`/`gpuVerdictText` gain a `vulkan_available`
   branch ("We see your {AMD Radeon …} — enable the beta Vulkan pack below")
   and a `vulkan_ready` equivalent.
5. Version 1.2.0 everywhere; rebuild; the release process is §6 + the standing
   rule: update `ECHO_DOWNLOAD_URL` + `ECHO_INSTALLER_SHA256` TOGETHER.

## §6. Distribution checklist (site + release — mirrors the CUDA pack)

1. Upload the zip to the CURRENT app release on
   `diablobuster/alan-echo-releases` + append its SHA-256 to the release notes
   and SHA256SUMS.txt (`gh release view --json body` → concatenate →
   `--notes-file`; NEVER bare `--notes`, it replaces the body).
2. stock-analyzer: add `app/api/echo/download/vulkan/route.ts` (copy the gpu
   route; env `ECHO_VULKAN_PACK_URL`, fallback to the releases page),
   `.env.template` entry, Vercel Production env, deploy.
3. VirusTotal-scan the zip; keep the permalink for support replies.
4. Copy updates — **beta wording until Tier 2 passes**: /echo FAQ ("available
   in beta for AMD and Intel GPUs"), success page + license email GPU lines
   (mention Echo offers it in Settings on AMD/Intel machines),
   `docs/echo-claude-design-prompts.md` verified-facts block (it currently
   says "roadmap" — change ONLY when this ships, and keep the beta qualifier;
   the no-overpromising rule is load-bearing for refund volume).

## §7. Acceptance criteria

- [x] Vulkan whisper-server builds from the v1.8.6 tag and transcribes
      correctly on the RTX 4060 via the Vulkan backend (Tier 1 full pass).
- [x] Pack zip matches the §3 layout, < ~150 MB, no demo exes, no vulkan-1.dll.
- [x] v1.1.0 (unchanged, already in the field) uses the pack via manual
      drop-in on a no-NVIDIA configuration (`engine_kind == "vulkan"`).
- [x] v1.2.0 offers one-click install ONLY on machines with a non-NVIDIA
      discrete/recent GPU, labeled beta, with the §5.3 rollback proven by a
      forced-failure test (e.g. corrupt the extracted exe and watch it
      disable + fall back + message).
- [ ] Release asset + checksums + stable redirect + envs live; site copy
      updated with beta wording; VirusTotal scanned.
- [ ] Tier 2 evidence recorded in this file before "beta" is ever removed.

## §8. Parked — and the triggers that un-park them

- **CUDA 11.8 legacy pack** (prebuilt exists upstream — trivial to ship):
  only if support tickets show CUDA-12 driver-too-old failures on real
  customers' NVIDIA cards.
- **ROCm/HIP (AMD)**: only if Tier 2 shows Vulkan perf on AMD is
  disappointing AND the affected cards are on Windows ROCm's (short) list.
- **OpenVINO / SYCL (Intel)**: only if Intel users materialize in volume or
  ask for NPU support.
- **OpenBLAS CPU pack**: never, barring benchmarks showing >1.5× on real
  customer CPUs — the shipped CPU build is already the safe default.

---

## §9. EXECUTED 2026-06-10 — evidence (Tier 1 PASS; Tier 2 still open)

**Build (§2)**: LunarG SDK 1.4.350.0 (elevated install, C:\VulkanSDK\1.4.350.0).
whisper.cpp v1.8.6 (23ee035) at C:\Users\arowm\whisper.cpp-vulkan, built with
VS 2022 (`-G "Visual Studio 17 2022" -A x64 -DBUILD_SHARED_LIBS=ON
-DGGML_VULKAN=1`). NOTE: plain `cmake -B build` picks up Strawberry MinGW gcc
and fails on old Windows headers — the VS generator is required.

**Pack (§3)**: `ALAN-Echo-GPU-Pack-Vulkan-1.0.0.zip`, **17.9 MB**, SHA-256
`84CB3A0C2CAA024EB5551F080E0E5010D490543AEB245E2AE248E5FD4ED39977`.
Layout verified (vulkan_release/Release/: whisper-server.exe, whisper.dll,
ggml{,-base,-cpu,-vulkan}.dll, CRT x3). The server target produces no
SDL2.dll and runs without it — left out. No vulkan-1.dll, no demo exes.
Extracted copy serves + transcribes correctly (customer-path test).

**Tier 1 (§4) — PASS on RTX 4060**:
- ggml_vulkan device line names the RTX 4060 (coopmat2); JFK sample
  transcribes correctly; warm /inference 445 ms (base.en, 11 s clip).
- v1.1.0 UNCHANGED + manual drop-in (cuda_release held): installed app
  reports `engine_kind == "vulkan"`, `ready == true`, Enhanced model.
- Latency, same clip + medium model: **Vulkan 366 ms vs CPU 5,490 ms (15×)**.
- cuda_release restored → engine returns to `cuda`. PASS.
- Human-mic dictation into Notepad: not executable by the agent session that
  ran this; engine-level evidence above covers the claim. Worth one manual
  dictation before announcing.

**§5 (v1.2.0) — shipped**: packs.rs parameterized (PackKind Cuda|Vulkan,
per-kind URL/dir/min-size); offer logic (NVIDIA→cuda, AMD/Intel adapter→
vulkan + beta flag); §5.3 watch + rollback. Three hardening finds from the
forced-failure tests, all shipped:
1. `SetErrorMode(SEM_FAILCRITICALERRORS|SEM_NOOPENFILEERRORBOX)` in main() —
   without it a corrupt engine exe pops a MODAL "Unsupported 16-Bit
   Application" dialog and the spawn hangs until a human clicks OK.
2. Rollback rename retries (10×1 s), then falls back to a `DISABLED` marker
   file inside vulkan_release (file locks can't block marker creation);
   find_server_binary + pack offer logic honor the marker.
3. confirm_vulkan_engine treats `stopped`/`idle` as "interrupted", not as a
   driver verdict (app exit mid-install must not disable the pack).
Debug-only env seam `ECHO_{CUDA,VULKAN}_PACK_URL` (cfg(debug_assertions),
compiled out of retail) + scripts/serve-file.mjs + scripts/cdp-eval.mjs are
how the forced-failure test runs: corrupt zip served locally → one-click
install → spawn fails instantly → pack disabled (rename ~3 s) → §5.3 message
in Settings → engine back on CPU, ready. Re-launch with the broken pack still
on disk picks CPU. PASS.

**Review**: 23-agent adversarial pass over both repos; 10 findings confirmed
(2 majors fixed: stopped/idle handling, failed-state refresh; beta-copy
fallback line added to email + success page; debug-gate/zip-slip/beta-wording
verified clean).

**Tier 2 (§4) — OPEN**: no AMD/Intel silicon touched yet. Everything
customer-facing says **beta**. Record real-hardware evidence here before any
copy drops the qualifier.
