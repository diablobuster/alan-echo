"""POST every wav in wav/ to our own whisper-server (default port 8199) and cache
transcripts in transcripts.jsonl. Never touches port 8178 (the dev app's server)."""
import json, os, sys, time
import requests

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = int(os.environ.get("GH_WHISPER_PORT", "8199"))
assert PORT != 8178, "port 8178 belongs to the running dev app"
WAV = os.path.join(HERE, "wav")
OUT = os.path.join(HERE, "transcripts.jsonl")

def main():
    done = {}
    if os.path.exists(OUT):
        for l in open(OUT, encoding="utf-8"):
            j = json.loads(l)
            done[j["id"]] = j
    sample = [l.split("\t")[0] for l in open(os.path.join(HERE, "sample.tsv"), encoding="utf-8")]
    f = open(OUT, "a", encoding="utf-8")
    t = time.time()
    n = 0
    for sid in sample:
        if sid in done:
            continue
        p = os.path.join(WAV, f"{sid}.wav")
        if not os.path.exists(p):
            continue
        with open(p, "rb") as fh:
            r = requests.post(f"http://127.0.0.1:{PORT}/inference",
                              files={"file": ("a.wav", fh, "audio/wav")},
                              data={"response_format": "json", "temperature": "0.0", "language": "en"}, timeout=300)
        r.raise_for_status()
        text = r.json().get("text", "").strip()
        f.write(json.dumps({"id": sid, "text": text}) + "\n")
        f.flush()
        n += 1
        if n % 50 == 0:
            print(n, round(time.time() - t, 1), "s", flush=True)
    print("transcribed", n, "new; total", len(done) + n)

if __name__ == "__main__":
    main()
