# ALAN Echo

On-device voice-to-text dictation for Windows and macOS. Audio is captured,
transcribed, and stored entirely on your machine — your voice never leaves
your device.

© 2026 ALAN Global Intelligence. Proprietary — see [LICENSE](LICENSE).

Product page: https://www.alanglobalintelligence.com/echo

## Development

```bash
npm i
npm run tauri dev
```

Tauri 2 (Rust backend in `src-tauri/`, React frontend in `src/`).

## Release legal checklist

- [ ] EULA changed? Update src/legal/eula.md + bump EULA_VERSION + sync site page + `npm run gen:legal`
- [ ] Deps changed? `npm run gen:legal` (regenerates third-party notices)
- [ ] Installer shows license page (NSIS) / app shows EULA gate on fresh install
- [ ] Binaries signed (Windows) + notarized (macOS); SHA256SUMS attached to release (`.\scripts\release-checksums.ps1 <tag>`)
- [ ] Release notes end with: `Use is governed by the ALAN Echo License Agreement: https://www.alanglobalintelligence.com/legal/echo-license`
