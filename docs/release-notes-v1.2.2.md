# ALAN Echo v1.2.2

**Legal & trust release.** No dictation-engine changes.

## What's new

- **License agreement on first launch** — Echo now shows the License Agreement the first time it runs (and after license updates). Existing users will see it once after updating; your settings, transcripts, and activation are untouched.
- **Installer license page** — the Windows installer now displays the license agreement before installation.
- **About & legal in Settings** — version, copyright, links to the License Agreement and Privacy Policy, and a full open-source license viewer (Settings → About → "Open-source licenses").
- **Trust hardening** — activation tokens now carry an expiry and refresh silently in the background; activation token writes are atomic; transcription temp audio is cleaned up immediately if transcription fails; export paths are validated; language selection is validated before reaching the speech engine.
- **Accessibility** — settings toggles and selectors now expose proper screen-reader roles.
- **macOS groundwork** — platform-correct hotkey display (Cmd vs Ctrl) and platform-neutral guidance text.

## Integrity

`SHA256SUMS.txt` is attached to this release. Verify on Windows:

```powershell
Get-FileHash ".\ALAN.Echo_1.2.2_x64-setup.exe" -Algorithm SHA256
```

---

Use of ALAN Echo is governed by the ALAN Echo License Agreement: https://www.alanglobalintelligence.com/legal/echo-license
