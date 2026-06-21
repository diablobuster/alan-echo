#!/bin/bash
# Stages the macOS retail build payload into src-tauri/resources/models/.
# Run before `npx tauri build` on a Mac. The staged files are gitignored.
#
# Payload:
#   - whisper-server (universal arm64+x86_64 build of whisper.cpp, Metal ON)
#   - ggml-base.en.bin (148 MB) — English-only base model
#
# Metal acceleration is ENABLED (the Metal framework ships on every Mac since
# 2014). On Apple Silicon whisper.cpp uses the GPU automatically; Intel Macs
# fall back to CPU inside whisper.cpp. The binary is universal so a single .app
# runs on both architectures (matches `tauri build --target universal-apple-darwin`).

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$REPO/src-tauri/resources/models"
MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
MODEL_FILE="$DEST/ggml-base.en.bin"
WHISPER_CPP_DIR="$REPO/.build/whisper.cpp"
# Pin a known-good whisper.cpp ref before a release build (reproducibility, and
# a guard against an upstream rename of the server target or a ggml CMake flag).
# Override with WHISPER_CPP_REF=<tag-or-branch>.
WHISPER_CPP_REF="${WHISPER_CPP_REF:-master}"

mkdir -p "$DEST"

# Metal shader compilation (GGML_METAL_EMBED_LIBRARY) needs the `metal` tool,
# which ships only with FULL Xcode — not the standalone Command Line Tools.
# Fail fast with an actionable message instead of a cryptic mid-build error.
if ! xcrun -sdk macosx -f metal >/dev/null 2>&1; then
    echo "ERROR: the Metal shader compiler ('metal') is missing."
    echo "Install full Xcode, then: sudo xcode-select -s /Applications/Xcode.app && sudo xcodebuild -license accept"
    exit 1
fi

# 1. Build whisper.cpp from source — universal (arm64 + x86_64), Metal ON.
echo "=== Building whisper-server from source (universal, Metal) — ref ${WHISPER_CPP_REF} ==="
if [ ! -d "$WHISPER_CPP_DIR" ]; then
    git clone --depth 1 --branch "$WHISPER_CPP_REF" https://github.com/ggerganov/whisper.cpp.git "$WHISPER_CPP_DIR"
fi

cd "$WHISPER_CPP_DIR"
git pull --ff-only 2>/dev/null || true

# Universal binary so the one bundled helper runs on both Apple Silicon and
# Intel. GGML_METAL_EMBED_LIBRARY bakes the Metal shaders into the binary, so
# the GPU path works from inside the .app bundle without an external metallib
# next to the executable (unknown cache vars are harmless across whisper.cpp
# versions; validate the produced binary actually uses Metal on hardware).
cmake -B build \
    -DCMAKE_BUILD_TYPE=Release \
    -DWHISPER_BUILD_SERVER=ON \
    -DCMAKE_OSX_ARCHITECTURES="arm64;x86_64" \
    -DGGML_METAL=ON \
    -DGGML_METAL_EMBED_LIBRARY=ON
cmake --build build --config Release -j "$(sysctl -n hw.ncpu)"

# The server binary name varies by whisper.cpp version
SERVER_BIN=""
for candidate in build/bin/whisper-server build/bin/server build/server; do
    if [ -x "$candidate" ]; then
        SERVER_BIN="$candidate"
        break
    fi
done

if [ -z "$SERVER_BIN" ]; then
    echo "ERROR: whisper-server binary not found after build"
    echo "Check build output above for errors"
    exit 1
fi

cp "$SERVER_BIN" "$DEST/whisper-server"
chmod +x "$DEST/whisper-server"

# Confirm the staged binary is actually universal (both slices present).
echo "=== whisper-server architectures ==="
lipo -info "$DEST/whisper-server" || file "$DEST/whisper-server"

# Belt-and-suspenders: -DGGML_METAL=ON already fails configure if Metal can't be
# enabled, but confirm the Metal backend is actually linked in. A release binary
# may be stripped (no symbol table), so treat a miss as a warning, not a failure.
if nm "$DEST/whisper-server" 2>/dev/null | grep -qi "ggml_metal"; then
    echo "Metal backend symbols confirmed in whisper-server."
else
    echo "WARNING: could not confirm Metal symbols via nm (binary may be stripped)."
    echo "Metal was requested via -DGGML_METAL=ON — verify GPU use on real hardware."
fi

# 2. Download the bundled model (skip if already present)
if [ ! -f "$MODEL_FILE" ] || [ "$(stat -f%z "$MODEL_FILE" 2>/dev/null || stat -c%s "$MODEL_FILE")" -lt 140000000 ]; then
    echo "=== Downloading ggml-base.en.bin (~148 MB) ==="
    curl -L -o "$MODEL_FILE" "$MODEL_URL" --progress-bar
fi

# 3. Summary
echo ""
echo "=== Staged payload ==="
ls -lh "$DEST/"
TOTAL=$(du -sh "$DEST" | cut -f1)
echo "Total: $TOTAL in $DEST"
echo ""
echo "Ready for: npx tauri build --target universal-apple-darwin"
