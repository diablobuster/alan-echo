"""Awkward-pause check for the round trip: long silences mid-phrase.

`glitch_synth` writes `wav/<id>.json` next to each WAV: one [word, start ms, end ms]
per source word. A word's span runs to the next word's start, so it holds the
pause after the word (and, because timings are spread over the tokens, sometimes
part of it lands mid-span). For each word this finds the longest silence in its
span (10 ms frames whose RMS is under 3% of the sentence's loud level) and flags
it when it is >= 350 ms and nothing explains it:
  - not the last word of a chunk (chunks are joined with a gap);
  - not a multi-word span whose last (zero-length) word ends in punctuation;
  - the word does not end in punctuation, and the next word does not open with a
    bracket, quote or dash (those pauses are natural).
Writes pauses.json and prints the rate per 1,000 words. Dev-only, stdlib only.
"""
import json, math, os, struct, sys, wave

HERE = os.path.dirname(os.path.abspath(__file__))
FRAME_MS = 10
PAUSE_MS = 350
PUNCT = tuple(",.;:!?)]\"'”’—–-…")
OPENERS = tuple("([{\"'“‘—–-")


def frames_rms(path):
    with wave.open(path, "rb") as w:
        n = w.getnframes()
        sr = w.getframerate()
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
        if not rms:
            continue
        loud = sorted(rms)[int(len(rms) * 0.95)]
        thr = max(0.003, loud * 0.03)
        words_total += len(times)
        for k, (word, s, e) in enumerate(times):
            nxt = times[k + 1] if k + 1 < len(times) else None
            if nxt is None or nxt[1] > e + 50:  # chunk end: the next chunk starts after a gap
                continue
            if word.endswith(PUNCT) or nxt[0].startswith(OPENERS):
                continue
            # a multi-word span ($16.5 billion,) is said on its first word; the inner
            # words have zero length, so their punctuation explains the pause
            j = k + 1
            while j < len(times) and times[j][1] == times[j][2]:
                j += 1
            if j > k + 1 and times[j - 1][0].endswith(PUNCT):
                continue
            if j < len(times) and times[j][0].startswith(OPENERS):
                continue
            runs = silent_runs(rms, s // FRAME_MS, e // FRAME_MS, thr)
            if not runs:
                continue
            rs, re_ = max(runs, key=lambda r: r[1] - r[0])
            if re_ - rs >= PAUSE_MS:
                flags.append(dict(id=sid, word=word, next=nxt[0], at_ms=rs, ms=re_ - rs))
    rate = round(1000 * len(flags) / max(1, words_total), 2)
    out = dict(words=words_total, flags=len(flags), rate_per_1k=rate, items=flags)
    json.dump(out, open(os.path.join(HERE, "pauses.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print(f"pauses: {len(flags)} in {words_total} words = {rate}/1k")
    for f in sorted(flags, key=lambda f: -f["ms"])[:30]:
        print(f"  {f['ms']:4} ms  {f['id']:>5}  {f['word']} | {f['next']}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "wav"))
