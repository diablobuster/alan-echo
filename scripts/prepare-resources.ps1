# Stages the retail installer payload into src-tauri/resources/models/.
# Run before `npx tauri build`. The staged files are gitignored (148 MB model);
# this script is the source of truth for what ships inside the installer.
#
# Payload (CPU-only out-of-box experience, ~160 MB):
#   - whisper-server.exe + its DLLs (CPU build of whisper.cpp)
#   - msvcp140/vcruntime140 CRT DLLs (whisper.cpp links the VC++ runtime
#     dynamically; clean machines don't have it — verified via PE imports)
#   - ggml-base.en.bin (148 MB) — English-only base model, the f16 variant:
#     quantized q5 kernels are 3-5x slower on pre-AVX2 CPUs, so f16 is the
#     safe default for the oldest hardware.
# GPU/CUDA build (~700 MB) and larger models ship as optional downloads, never
# in the installer (NSIS hard-fails at 2 GB).

$ErrorActionPreference = 'Stop'

$repo = Split-Path $PSScriptRoot -Parent
$dest = Join-Path $repo 'src-tauri\resources\models'
$cpuBuild = Join-Path $env:APPDATA 'ALAN Echo\models\Release'
$modelUrl = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin'
$modelFile = Join-Path $dest 'ggml-base.en.bin'

New-Item -ItemType Directory -Force $dest | Out-Null

# 1. CPU whisper build (server + runtime DLLs only — no dev tools)
$serverFiles = @('whisper-server.exe', 'whisper.dll', 'ggml.dll', 'ggml-base.dll', 'ggml-cpu.dll', 'SDL2.dll')
foreach ($f in $serverFiles) {
    Copy-Item (Join-Path $cpuBuild $f) $dest -Force
}

# 2. VC++ CRT, app-local deployment (newest x64 redist on this machine)
$crt = Get-ChildItem 'C:\Program Files\Microsoft Visual Studio', 'C:\Program Files (x86)\Microsoft Visual Studio' `
        -Recurse -Filter 'vcruntime140_1.dll' -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\Redist\\MSVC\\[\d.]+\\x64\\Microsoft\.VC\d+\.CRT\\' } |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $crt) { throw 'VC++ x64 redist CRT folder not found — install VS Build Tools or copy msvcp140/vcruntime140 DLLs manually.' }
foreach ($f in @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')) {
    Copy-Item (Join-Path $crt.Directory.FullName $f) $dest -Force
}

# 3. Bundled model (skip if already present and plausibly complete)
if (-not (Test-Path $modelFile) -or (Get-Item $modelFile).Length -lt 140MB) {
    Write-Host "Downloading ggml-base.en.bin (~148 MB)..."
    curl.exe -L -o $modelFile $modelUrl --silent --show-error
}

Get-ChildItem $dest | Select-Object Name, @{N = 'MB'; E = { [math]::Round($_.Length / 1MB, 2) } } | Format-Table -AutoSize
$total = [math]::Round((Get-ChildItem $dest | Measure-Object Length -Sum).Sum / 1MB, 1)
Write-Host "Staged payload: $total MB in $dest"
