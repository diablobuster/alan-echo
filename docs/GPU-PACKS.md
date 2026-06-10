# ALAN Echo — GPU acceleration packs (landscape + roadmap)

Written 2026-06-10. Context: whisper.cpp supports several GPU backends; Echo
ships the CPU engine in the installer and delivers acceleration as optional
packs. The app (v1.1+) downloads/installs packs itself and probes pack
directories at engine start, so shipping a new pack = uploading a zip + (maybe)
one env var. No app rebuild needed.

## Shipped

- **CUDA pack (NVIDIA)** — `ALAN-Echo-GPU-Pack-1.0.0.zip`, repackaged from the
  official whisper.cpp `whisper-cublas-12.4.0-bin-x64.zip` release binaries
  (MIT). Installs to `models\cuda_release\Release\`. v1.1 installs it with one
  click (Settings → GPU acceleration → Enable); v1.0 users extract manually.
  Detection: `nvidia-smi`.

## Next: Vulkan pack (AMD + Intel Arc + NVIDIA fallback)

The highest-value second pack. One binary covers AMD Radeon, Intel Arc, and
recent integrated GPUs — the entire non-NVIDIA GPU audience.

- whisper.cpp publishes **no prebuilt Windows Vulkan binary** (v1.8.6 assets:
  CPU, BLAS, CUDA 11.8/12.4 only), so this is a one-time local build:
  1. Install the LunarG Vulkan SDK (~300 MB).
  2. `git clone https://github.com/ggml-org/whisper.cpp && cmake -B build -DGGML_VULKAN=1 -DCMAKE_BUILD_TYPE=Release && cmake --build build --config Release`
  3. Package `whisper-server.exe` + DLLs + CRT into a zip rooted at
     `vulkan_release/Release/` (the v1.1 engine already probes
     `models\vulkan_release\Release\whisper-server.exe` — drop-in).
- Smoke-testable on this machine (Vulkan runs on the RTX 4060), but should be
  validated on real AMD silicon before marketing it — ship as "beta" otherwise.
- App work later (not blocking): detect non-NVIDIA GPUs (e.g.
  `wmic path win32_VideoController get name`) so the in-app one-click installer
  can offer the Vulkan pack too; today the engine merely *uses* the directory
  if present.

## Considered and parked

- **ROCm/HIP (AMD)** — Windows support covers only a narrow RDNA card list;
  Vulkan reaches the same users with far less support burden.
- **OpenVINO / SYCL (Intel)** — real speedups on Intel NPUs/GPUs but a niche
  audience already covered acceptably by Vulkan; revisit if Intel users ask.
- **OpenBLAS CPU pack** — marginal gains over the shipped CPU build; not worth
  a 2nd download path.

## Invariants when adding any pack

- Zip root must be `<kind>_release/Release/whisper-server.exe` + its DLLs
  (+ CRT DLLs — clean machines lack the VC++ runtime).
- Engine preference order lives in `src-tauri/src/whisper.rs`
  `find_server_binary`: cuda → vulkan → cpu (GPU machines), vulkan → cpu
  (no NVIDIA).
- Upload as a release asset, publish its SHA-256 in the release notes and
  SHA256SUMS.txt, and point a stable site redirect at it
  (`/api/echo/download/<kind>` in stock-analyzer) before referencing it
  anywhere customer-facing.
