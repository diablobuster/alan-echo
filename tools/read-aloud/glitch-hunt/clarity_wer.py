"""Clarity at high speed: word error rate of Whisper on our audio at several reading
speeds and model-speed caps (round-2 review). Dev only.

  python clarity_wer.py sample <corpus.tsv> <out.tsv> [n=100]
      seeded random control sentences (6+ words)
  python clarity_wer.py score <sample.tsv> <wavdir> [port=8199]
      transcribes <wavdir>/<id>.wav with our own whisper-server (never 8178; cached in
      <wavdir>/heard.json) and prints the WER (word edit distance / reference words,
      after the same spoken-form normalisation the round trip uses)
"""
import json, os, random, sys
from speechnorm import normalize_words


def edit(a, b):
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]


def load(p):
    out = {}
    for l in open(p, encoding="utf-8"):
        parts = l.rstrip("\n").split("\t", 2)
        if len(parts) == 3:
            out[parts[0]] = parts[2]
    return out


def main():
    cmd = sys.argv[1]
    if cmd == "sample":
        rows = [r for r in load(sys.argv[2]).items() if len(r[1].split()) >= 6]
        n = int(sys.argv[4]) if len(sys.argv) > 4 else 100
        random.seed(31)
        pick = random.sample(rows, n)
        with open(sys.argv[3], "w", encoding="utf-8", newline="\n") as f:
            for i, t in pick:
                f.write(f"{i}\tcontrol\t{t}\n")
        return
    sample, d = load(sys.argv[2]), sys.argv[3]
    port = int(sys.argv[4]) if len(sys.argv) > 4 else 8199
    assert port != 8178, "8178 is the app's whisper-server"
    cache_p = os.path.join(d, "heard.json")
    heard = json.load(open(cache_p, encoding="utf-8")) if os.path.exists(cache_p) else {}
    import requests
    for i in sample:
        w = os.path.join(d, f"{i}.wav")
        if i in heard or not os.path.exists(w):
            continue
        with open(w, "rb") as fh:
            r = requests.post(f"http://127.0.0.1:{port}/inference", files={"file": ("a.wav", fh, "audio/wav")},
                              data={"response_format": "json", "temperature": "0.0", "language": "en"}, timeout=300)
        r.raise_for_status()
        heard[i] = r.json().get("text", "").strip()
    json.dump(heard, open(cache_p, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    errs = words = 0
    for i, t in sample.items():
        ref = [x for x, _ in normalize_words(t)]
        hyp = [x for x, _ in normalize_words(heard.get(i, ""))]
        errs += edit(ref, hyp)
        words += len(ref)
    print(json.dumps({"dir": d, "sentences": len(sample), "ref_words": words, "errors": errs,
                      "wer_pct": round(100 * errs / max(words, 1), 2)}))


if __name__ == "__main__":
    main()
