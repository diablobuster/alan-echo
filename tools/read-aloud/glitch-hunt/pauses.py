"""Awkward-pause check for the round trip: long silences mid-phrase.

`glitch_synth` writes `wav/<id>.json` next to each WAV: one [word, start ms, end ms]
per source word (a word's span runs to the next word's start, so it holds the pause
after it). For each word this measures silent runs in the audio (10 ms frames whose
RMS is under 3% of the sentence's loud level):
  - INSIDE: a silence of >= 200 ms that ends before the word's span ends (a gap
    inside a word or a multi-word span: "four hundred | five");
  - AFTER: a trailing silence of >= 350 ms after a word with no punctuation
    (a pause mid-phrase), not at a chunk end.
Writes pauses.json and prints the rate per 1,000 words. Dev-only, stdlib only.
"""
import json, math, os, struct, sys, wave

HERE = os.path.dirname(os.path.abspath(__file__))
FRAME_MS = 10
INSIDE_MS = 200
AFTER_MS = 350
PUNCT = tuple(",.;:!?)]\"'”’—–-…")


def frames_rms(path):
    with wave.open(path, "rb") as w:
        sr, n = w.getframerate(), w.getnframes()
        raw = w.readframes(n)
    s = struct.unpack(f"<{len(raw) // 2}h", raw)
    step = sr * FRAME_MS // 1000
    out = []
    for i in range(0, len(s), step):
        seg = s[i:i + step]
        out.append(math.sqrt(sum(x * x for x in seg) / max(1, len(seg))) / 32768.0)
    return out


def silent_runs(rms, a, b, thr):
    """(start ms, end ms) of silent runs inside frames [a, b)."""
    runs, start = [], None
    for k in range(a, min(b, len(rms))):
        if rms[k] < thr:
            if start is None:
                start = k
        elif start is not None:
            runs.append((start * FRAME_MS, k * FRAME_MS))
            start = None
    if start is not None:
        runs.append((start * FRAME_MS, min(b, len(rms)) * FRAME_MS))
    return runs


def main(wavdir):
    flags, words_total = [], 0
    for name in sorted(os.listdir(wavdir)):
        if not name.endswith(".json"):
            continue
        sid = name[:-5]
        wav = os.path.join(wavdir, sid + ".wav")
        if not os.path.exists(wav):
            continue
        times = json.load(open(os.path.join(wavdir, name), encoding="utf-8"))
        rms = frames_rms(wav)
        loud = sorted(rms)[int(len(rms) * 0.95)] if rms else 0
        thr = max(0.003, loud * 0.03)
        words_total += len(times)
        for k, (word, s, e) in enumerate(times):
            nxt = times[k + 1][1] if k + 1 < len(times) else None
            chunk_end = nxt is None or nxt > e + 50  # the next chunk starts after a gap
            a, b = s // FRAME_MS, e // FRAME_MS
            for rs, re_ in silent_runs(rms, a, b, thr):
                dur = re_ - rs
                trailing = re_ >= b * FRAME_MS - FRAME_MS
                if not trailing and rs > s + 30 and dur >= INSIDE_MS:
                    flags.append(dict(id=sid, word=word, kind="INSIDE", at_ms=rs, ms=dur))
                elif trailing and dur >= AFTER_MS and not chunk_end and not word.endswith(PUNCT):
                    flags.append(dict(id=sid, word=word, kind="AFTER", at_ms=rs, ms=dur))
    rate = round(1000 * len(flags) / max(1, words_total), 2)
    out = dict(words=words_total, flags=len(flags), rate_per_1k=rate, items=flags)
    json.dump(out, open(os.path.join(HERE, "pauses.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print(f"pauses: {len(flags)} in {words_total} words = {rate}/1k")
    for f in flags[:40]:
        print(f"  {f['kind']:6} {f['ms']:4} ms  {f['id']}  {f['word']}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "wav"))
