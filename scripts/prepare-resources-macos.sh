#!/bin/bash
# Stages the macOS retail build payload into src-tauri/resources/models/.
# Run before `npx tauri build` on a Mac. The staged files are gitignored.
#
# Payload (CPU-only out-of-box experience):
#   - whisper-server (CPU build of whisper.cpp for macOS)
#   - ggml-base.en.bin (148 MB) — English-only base model
#
# Metal/CoreML GPU acceleration is deferred to a future release.
# For now, the app ships CPU-only on Mac.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$REPO/src-tauri/resources/models"
MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
MODEL_FILE="$DEST/ggml-base.en.bin"
WHISPER_CPP_DIR="$REPO/.build/whisper.cpp"

mkdir -p "$DEST"

# 1. Build whisper.cpp from source (CPU-only, no Metal for v1)
echo "=== Building whisper-server from source ==="
if [ ! -d "$WHISPER_CPP_DIR" ]; then
    git clone --depth 1 https://github.com/ggerganov/whisper.cpp.git "$WHISPER_CPP_DIR"
fi

cd "$WHISPER_CPP_DIR"
git pull --ff-only 2>/dev/null || true

# Build the HTTP server. WHISPER_NO_METAL=1 for CPU-only v1 release.
# Remove this flag in a future release to enable Metal acceleration.
cmake -B build \
    -DCMAKE_BUILD_TYPE=Release \
    -DWHISPER_BUILD_SERVER=ON \
    -DWHISPER_NO_METAL=1
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
echo "Ready for: npx tauri build"
