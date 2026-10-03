#!/usr/bin/env bash
# One-command glitch hunt (static + round trip) and the comparison with baseline.json.
#   bash run_hunt.sh                      # full run (static, synth, Whisper), compare to baseline.json
#   GH_STATIC_ONLY=1 bash run_hunt.sh     # static pass only (seconds, no audio): the per-task gate
#   bash run_hunt.sh --set-baseline       # freeze this run as the new baseline
# Env:
#   GH_CORPUS        a frozen corpus.tsv to copy in (default: build one with build_corpus.py)
#   GH_SRC           src-tauri of the branch under test (default: this repo's)
#   GH_WHISPER_PORT  our own whisper-server port (default 8199; never 8178, the dev app's)
#   GH_RESAMPLE=1    re-pick sample.tsv (default: keep the existing sample)
# Everything this writes (corpus, cache, jsonl/csv/json outputs, wav/) is git-ignored.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="${GH_SRC:-$HERE/../../../src-tauri}"
PORT="${GH_WHISPER_PORT:-8199}"
[ "$PORT" = "8178" ] && { echo "port 8178 belongs to the dev app"; exit 1; }
WHERE="$(cygpath -w "$HERE" 2>/dev/null || echo "$HERE")"
SEP='\'
[ "$WHERE" = "$HERE" ] && SEP='/'
export PYTHONIOENCODING=utf-8
# a quick release build is enough for a test harness
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_PANIC=unwind

cd "$HERE"
if [ -n "${GH_CORPUS:-}" ]; then
  cp "$GH_CORPUS" corpus.tsv
fi
[ -f corpus.tsv ] || python build_corpus.py          # cached fetches; frozen once built
( cd "$SRC" && GH_CORPUS="$WHERE${SEP}corpus.tsv" GH_OUT="$WHERE${SEP}static.jsonl" \
    cargo test --release glitch_static -- --ignored --nocapture 2>&1 | grep -E "^static:|test result" )
python static_flags.py > /dev/null
python -c "import json;s=json.load(open('static_summary.json',encoding='utf-8'));print('static', s['static_rate_per_1k'], '/1k words')"
if [ -n "${GH_STATIC_ONLY:-}" ]; then
  python cluster.py --static-only "$@"
  exit 0
fi

{ [ -f sample.tsv ] && [ -z "${GH_RESAMPLE:-}" ]; } || python make_sample.py
rm -rf wav transcripts.jsonl                        # audio depends on the code under test
( cd "$SRC" && GH_SAMPLE="$WHERE${SEP}sample.tsv" GH_WAVDIR="$WHERE${SEP}wav" \
    cargo test --release glitch_synth -- --ignored --nocapture 2>&1 | grep -E "synth done|error|test result" )

# our own whisper-server (large-v3) on $PORT; stopped at the end if we started it
STARTED=""
if ! curl -s -m 2 "http://127.0.0.1:$PORT/" > /dev/null; then
  WDIR="$APPDATA/ALAN Echo/models/cuda_release/Release"
  ( cd "$WDIR" && ./whisper-server.exe -m "$APPDATA/ALAN Echo/models/ggml-large-v3.bin" --host 127.0.0.1 \
      --port "$PORT" -l en -t 4 -sns > "$HERE/whisper$PORT.log" 2>&1 & )
  STARTED=1
  until curl -s -m 2 "http://127.0.0.1:$PORT/" > /dev/null; do sleep 2; done
fi
GH_WHISPER_PORT="$PORT" python transcribe.py
if [ -n "$STARTED" ]; then
  # only the whisper-server we started on our own port (never the app or its server)
  WPID=$(netstat -ano | grep -E "127.0.0.1:$PORT .*LISTENING" | awk '{print $NF}' | head -1)
  [ -n "$WPID" ] && taskkill //PID "$WPID" //F > /dev/null && echo "stopped whisper-server pid $WPID (port $PORT)"
fi
python roundtrip.py > /dev/null
python cluster.py "$@"
