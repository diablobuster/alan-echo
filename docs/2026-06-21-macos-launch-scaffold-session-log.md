# ALAN Echo — macOS Launch Scaffolding (session log)

**Date:** 2026-06-21
**Branch:** `feat/macos-launch-scaffold` (off `fix/echo-program-hardening`)
**Commit:** `42ea22c` — 11 files, +553 / −152
**Driver:** the macOS launch handoff (`docs/2026-06-21-macos-launch-handoff.md`).
**Decisions (user):** architecture = **universal2** (Intel + Apple Silicon); scope = **everything in-repo incl. the native paste rewrite (E)**.

> **Hard constraint:** authored on a Windows dev host. None of the macOS code is compiled or run here (resident-app rule + no Mac hardware). This branch lands for a **Mac CI build + on-device validation**. `cargo check` verified the *Windows/shared* build and that macOS deps resolve; everything macOS-only is gated on a real Mac build.

---

## 1. What shipped (handoff items B–F + a new ship-blocker)

| Item | Status this session |
|---|---|
| **NEW: macOS resource resolution** | **Fixed.** The engine couldn't find its binary/model on Mac at all (latent, never validated). |
| **B** signing + notarization + entitlements | **Scaffolded**, graceful no-op until Apple secrets exist. Verifiable only once the Apple identity exists. |
| **C** universal2 | **Done** (build target + fat whisper-server). |
| **D** Metal | **Done** (detection, engine label, pack-gating, UI copy). Perf must be confirmed on Apple Silicon. |
| **E** native paste | **Done** (osascript → NSWorkspace/CGEventPost). Highest compile/behavior risk; Mac-CI + on-device gated. |
| **F.2** site Mac download | **Deferred** (patch in §5) — stock-analyzer is on the parallel session's dirty tree; also dormant until go-live. |
| **A** Apple enrollment · **G** EULA grant · **F.1** ungate checkout | **Out of scope** (user action / counsel / go-live step). |

## 2. Files touched + intent

- **`src-tauri/src/whisper.rs`** — (a) **Resource fix:** `find_server_binary` + `model_dirs` now also probe `<App>.app/Contents/Resources/models` (Tauri stages bundled resources under `Contents/Resources`, but the code only looked next to the exe in `Contents/MacOS` and in an empty `data_dir/models`; on Mac the engine therefore found neither `whisper-server` nor the model). (b) **Metal:** `detect_apple_gpu()` (sysctl CPU-brand → Apple Silicon), `Hardware.metal` + `EngineInfo.metal`, `binary_kind(path, &hw)` returns `"metal"` on Apple Silicon, `cuda = gpu_name.is_some() && !metal` so Apple Silicon never reports CUDA, `detect_nvidia_gpu` gated to non-mac.
- **`src-tauri/src/packs.rs`** — gate the Windows CUDA/Vulkan **pack offers off macOS** (`get_gpu_pack_status`, `download_gpu_pack` hard-refuse via `cfg!`), and add a `"metal_ready"` verdict to `test_gpu` keyed on `info.metal`. Prevents offering a Windows pack to an Intel Mac (whose "Intel/Iris" adapter name would otherwise match `vulkan_candidate`).
- **`src-tauri/src/paste.rs`** — rewrote `mod mac`: `NSWorkspace.frontmostApplication` capture, `NSRunningApplication` refocus, `CGEventPost` Cmd+V; `AXIsProcessTrusted` preflight preserves the "enable Accessibility" guidance the osascript path returned; **held-Shift neutralization** (synthetic Shift-up before Cmd+V) to match the Windows path.
- **`src-tauri/Cargo.toml`** — `[target.'cfg(target_os = "macos")'.dependencies]`: `core-graphics 0.24`, `objc2-app-kit 0.2` with features `NSWorkspace, NSRunningApplication, NSApplication, libc`. **`libc` is required** — `processIdentifier` / `runningApplicationWithProcessIdentifier` are gated behind it (review blocker; fixed).
- **`src-tauri/tauri.conf.json`** — `bundle.macOS.entitlements = "entitlements.plist"` (kept `minimumSystemVersion 10.15` for Intel).
- **`src-tauri/entitlements.plist`** (new) — app: `com.apple.security.device.audio-input` (mic under hardened runtime). `Info.plist` already had `NSMicrophoneUsageDescription` — left as-is.
- **`src-tauri/entitlements-helper.plist`** (new) — whisper-server helper: `disable-library-validation` + `allow-jit` (minimal set TBD on hardware).
- **`scripts/prepare-resources-macos.sh`** — universal (`CMAKE_OSX_ARCHITECTURES="arm64;x86_64"`), Metal ON (`GGML_METAL=ON` + `GGML_METAL_EMBED_LIBRARY=ON` so the metallib is baked in for a bundled app), removed `-DWHISPER_NO_METAL=1`, full-Xcode **`metal` preflight**, `lipo`/`nm` post-build checks, pinnable `WHISPER_CPP_REF`.
- **`.github/workflows/build-macos.yml`** — build `--target universal-apple-darwin`; import cert → **sign the whisper-server helper** (hardened runtime) → `tauri build` (signs + notarizes + staples the app); all signing gated `if: env.APPLE_CERTIFICATE != ''` so **CI still emits an unsigned universal .dmg until enrollment**; verify the **`.app`** stapled ticket (not the flaky `.dmg`) + the helper's surviving signature; keychain timeout 6h.
- **`src/components/SettingsPanel.jsx`** — `engine_kind === 'metal'` → "GPU: <chip> · Metal active"; `metal_ready` verdict copy; "Apple Silicon GPU · starting…" transient label.

## 3. Verification

- ✅ **`cargo check` (Windows) clean** — exit 0, no warnings. No regression to the resident Windows app / shared code (`Hardware`/`EngineInfo` field, `binary_kind` signature, `detect_hardware` cfg pattern, `packs.rs` `cfg!` gates all compile).
- ✅ **macOS deps resolve** — `core-graphics 0.24.0`, `objc2 0.5.2`, `objc2-app-kit 0.2.2` with the `libc` feature; feature names valid on crates.io.
- ✅ **5-lens adversarial review** (Rust x-plat / native-API / Tauri-signing-CI / Metal-semantics / integration) — **1 blocker** (the `libc` feature; fixed + re-checked), no other blockers or highs. Review independently confirmed: the Resources path-walk is correct, the Metal labelling is honest on both Apple Silicon and Intel, the pack-gating fully suppresses Windows packs on Mac, and the graceful-no-op CI path is correct.
- ⛔ **NOT done here (the Mac gate):** compile/run on macOS, signing+notarization (needs the Apple identity), Metal speed on Apple Silicon, native-paste behavior + Accessibility, universal binary on a real Intel Mac.

## 4. Follow-ups / known limitations (carry into the Mac session)

1. **Apple Developer enrollment (item A) is the critical path** — B cannot be *verified* until the identity exists; start the clock.
2. **paste.rs is compile-unverified on macOS** — first Mac CI build is the real check (objc2 surface verified against 0.2.2 by review, but not compiled). Then on-device: confirm Cmd+V (not Cmd+Shift+V) when the hotkey is held, and the Accessibility prompt.
3. **Helper entitlements are a starting set** — on first hardware run, try dropping `allow-jit` (the metallib is embedded); keep only what's needed to launch. If the helper won't launch, add `allow-unsigned-executable-memory`.
4. **Nested-signature survival** — the CI verify step asserts the bundled whisper-server keeps its hardened-runtime signature after Tauri's deep-sign; if Tauri clobbers it, re-sign the helper in-bundle then re-sign the `.app` (or promote it to an `externalBin` sidecar so Tauri owns its signing).
5. **Metal label is inferred from hardware**, guaranteed by `-DGGML_METAL=ON` (configure fails closed) — but if a future whisper.cpp ships CPU-only silently, the UI would claim Metal. The `nm` check warns; a stronger guard is to parse whisper-server's startup banner for the Metal backend.
6. **Pin `WHISPER_CPP_REF`** to a validated tag before the first release build (currently tracks `master`).
7. **EULA §1 grant** (item G, counsel) still says "Windows devices" — widen before taking Mac money.

## 5. F.2 — site Mac download section (DEFERRED patch)

Not applied: `stock-analyzer` is on the parallel session's `fix/echo-ship-blockers` with a dirty tree (coordination rule: don't race/deploy that tree), and the section is **dormant** anyway — it must only render once `ECHO_MAC_*` env is set at go-live. Apply during the coordinated Mac go-live, on a clean tree, in `app/echo/download/page.tsx`:

```tsx
// add alongside the existing windows version fetch
const [MAC, setMac] = useState<null | { version: string; sha256: string; sizeMb: string; releaseDate: string }>(null);
useEffect(() => {
  let live = true;
  fetch("/api/echo/version?platform=mac")
    .then((r) => (r.ok ? r.json() : null))
    .then((d) => { if (live && d?.sha256) setMac({ version: d.version, sha256: d.sha256, sizeMb: String(d.sizeMb ?? ""), releaseDate: d.releaseDate ?? "" }); })
    .catch(() => {});
  return () => { live = false; };
}, []);

// render ONLY when MAC is non-null (i.e. ECHO_MAC_* env is set → fulfillable):
{MAC && (
  <div style={{ marginTop: 24, border: "1px solid var(--border-primary)", borderTop: "2px solid var(--accent-green)", borderRadius: 5, background: "var(--bg-card)", padding: 28 }}>
    <a href="/api/echo/download-free?platform=mac" className="alan-btn alan-btn-primary alan-btn-lg" style={{ width: "100%" }}>Download ALAN Echo {MAC.version} for Mac</a>
    <div style={{ marginTop: 18, fontFamily: "var(--font-geist-mono), monospace", fontSize: 11, color: "var(--text-muted)" }}>
      <span>v{MAC.version}</span> · <span>{MAC.sizeMb} MB</span> · <span>macOS 10.15+ · Universal (Apple Silicon &amp; Intel)</span>
    </div>
    <p style={{ marginTop: 12, fontSize: 13 }}>
      First launch: the app is signed &amp; notarized, so it opens normally. If macOS ever says it can&apos;t verify the developer,
      right-click the app → <b>Open</b>. Verify the download: <code>shasum -a 256 ALAN-Echo.dmg</code> against the SHA-256 below.
    </p>
    <div style={{ marginTop: 10, fontFamily: "var(--font-geist-mono), monospace", fontSize: 11, wordBreak: "break-all" }}>{MAC.sha256}</div>
  </div>
)}
```

Also at go-live: F.1 (remove the `isMacBrowser` gate in `app/api/echo/checkout/route.ts`), flip the `/echo` FAQ "we intend…" line to present tense, set `ECHO_MAC_*` env (hash the **served** `.dmg`, not SHA256SUMS), redeploy, then verify live.
