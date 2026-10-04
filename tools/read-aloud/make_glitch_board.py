"""Listening board for the Read Aloud glitch hunt (phase 3 spec §6).

Builds one HTML page Alex can open and listen through:
  R2. Round 2: 25 finance / tech / politics / sports / weather / recipe lines at 2.2x,
     before round 2 (748873a) vs after, chunks back to back as the player plays them;
  0. Round 1: 20 acronym / emphasis / unit lines, before vs after round 1;
  1. 15 showcase lines, before (feat/read-aloud) vs after (this branch), with what
     Whisper heard for each;
  2. every residual round-trip glitch (audio, the sentence, the words, what
     Whisper heard);
  3. awkward pauses found by glitch-hunt/pauses.py;
  4. 20 random sentences with no glitch.

WAVs are copied to %TEMP%\\echo-glitch-board\\ and linked by absolute file:// paths
from tools/read-aloud/glitch_board.html. A self-contained copy (HTML + WAVs, relative
links) goes to tools/read-aloud/board/. Both outputs are git-ignored.

  python make_glitch_board.py [--hunt DIR] [--before DIR] [--after DIR] [--whisper-port 8199]

--hunt     glitch-hunt folder with roundtrip.csv, sample.tsv, transcripts.jsonl, wav/
--before   showcase WAVs synthesised by the old code (`<id>.wav`, ids from board_showcase.tsv)
--after    showcase WAVs synthesised by this branch
--whisper-port  our own whisper-server (never 8178); transcribes showcase WAVs once
                (cached in <dir>/heard.json). Without it the "heard" column is blank.
"""
import argparse, csv, html, json, os, random, shutil, wave

HERE = os.path.dirname(os.path.abspath(__file__))
TEMP = os.path.join(os.environ.get("TEMP", HERE), "echo-glitch-board")


def load_tsv(p):
    out = {}
    for line in open(p, encoding="utf-8"):
        parts = line.rstrip("\n").split("\t", 2)
        if len(parts) == 3:
            out[parts[0]] = parts[2]
    return out


def heard_for(d, ids, port):
    """Whisper transcripts of <d>/<id>.wav, cached in <d>/heard.json."""
    cache_p = os.path.join(d, "heard.json")
    cache = json.load(open(cache_p, encoding="utf-8")) if os.path.exists(cache_p) else {}
    if port and port != 8178:
        import requests
        for i in ids:
            w = os.path.join(d, f"{i}.wav")
            if i in cache or not os.path.exists(w):
                continue
            with open(w, "rb") as fh:
                r = requests.post(f"http://127.0.0.1:{port}/inference", files={"file": ("a.wav", fh, "audio/wav")},
                                  data={"response_format": "json", "temperature": "0.0", "language": "en"}, timeout=300)
            r.raise_for_status()
            cache[i] = r.json().get("text", "").strip()
        json.dump(cache, open(cache_p, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    return cache


CSS = """
:root{--bg:#fbfaf7;--fg:#1d1d1b;--muted:#6b6a65;--line:#e4e1d9;--card:#fff;--bad:#b3261e;--good:#1e6b3a;--accent:#2b5bb5}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--bg:#151514;--fg:#ecebe6;--muted:#a19f97;--line:#2e2d2a;--card:#1d1c1a;--bad:#f2867d;--good:#79c995;--accent:#8fb0ff}}
:root[data-theme="dark"]{--bg:#151514;--fg:#ecebe6;--muted:#a19f97;--line:#2e2d2a;--card:#1d1c1a;--bad:#f2867d;--good:#79c995;--accent:#8fb0ff}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}
main{max-width:1100px;margin:0 auto;padding:24px 16px 64px}h1{font-size:26px;margin:0 0 4px}h2{font-size:19px;margin:36px 0 8px}
p.lede{color:var(--muted);margin:0 0 16px}table{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line)}
th,td{text-align:left;padding:8px 10px;border-bottom:1px solid var(--line);vertical-align:top}th{font-size:12px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted)}
td.num{font-variant-numeric:tabular-nums;white-space:nowrap}audio{width:220px;height:32px}.bad{color:var(--bad)}.good{color:var(--good)}
.muted{color:var(--muted)}code{font:13px ui-monospace,Consolas,monospace}.scroll{overflow-x:auto}
nav a{color:var(--accent);margin-right:14px}
"""


def seconds(d, i):
    """Length of <d>/<i>.wav in seconds, or None."""
    p = os.path.join(d, f"{i}.wav")
    if not os.path.exists(p):
        return None
    with wave.open(p, "rb") as w:
        return w.getnframes() / w.getframerate()


def audio(src):
    return f'<audio controls preload="none" src="{html.escape(src)}"></audio>'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--hunt", default=os.path.join(HERE, "glitch-hunt"))
    ap.add_argument("--before", default=os.path.join(TEMP, "before"))
    ap.add_argument("--after", default=os.path.join(TEMP, "after"))
    ap.add_argument("--whisper-port", type=int, default=0)
    ap.add_argument("--gate", default="", help="optional JSON with the gate table rows")
    ap.add_argument("--r1-before", default=os.path.join(TEMP, "r1-before"),
                    help="round 1 WAVs from the code before round 1 (ids from board_round1.tsv)")
    ap.add_argument("--r1-after", default=os.path.join(TEMP, "r1-after"), help="round 1 WAVs from this branch")
    ap.add_argument("--r2-before", default=os.path.join(TEMP, "r2-before"),
                    help="round 2 WAVs at 2.2x from the code before round 2 (ids from board_round2.tsv)")
    ap.add_argument("--r2-after", default=os.path.join(TEMP, "r2-after"), help="round 2 WAVs at 2.2x from this branch")
    a = ap.parse_args()
    random.seed(7)

    os.makedirs(os.path.join(TEMP, "rt"), exist_ok=True)
    show = load_tsv(os.path.join(HERE, "board_showcase.tsv"))
    hb = heard_for(a.before, list(show), a.whisper_port)
    ha = heard_for(a.after, list(show), a.whisper_port)
    r1 = load_tsv(os.path.join(HERE, "board_round1.tsv"))
    r1b = heard_for(a.r1_before, list(r1), a.whisper_port)
    r1a = heard_for(a.r1_after, list(r1), a.whisper_port)
    r2 = load_tsv(os.path.join(HERE, "board_round2.tsv"))
    r2b = heard_for(a.r2_before, list(r2), a.whisper_port)
    r2a = heard_for(a.r2_after, list(r2), a.whisper_port)

    sample = load_tsv(os.path.join(a.hunt, "sample.tsv"))
    trans = {}
    tp = os.path.join(a.hunt, "transcripts.jsonl")
    for line in open(tp, encoding="utf-8"):
        j = json.loads(line)
        trans[j["id"]] = j["text"]
    rows = list(csv.DictReader(open(os.path.join(a.hunt, "roundtrip.csv"), encoding="utf-8")))
    glitch = [r for r in rows if r["verdict"] == "GLITCH"]
    bad_ids = {r["id"] for r in glitch}
    good_ids = sorted(i for i in sample if i in trans and i not in bad_ids)
    good = random.sample(good_ids, min(20, len(good_ids)))
    pauses = []
    pp = os.path.join(a.hunt, "pauses.json")
    if os.path.exists(pp):
        pauses = json.load(open(pp, encoding="utf-8")).get("items", [])

    # copy WAVs into %TEMP%\echo-glitch-board\rt
    need = sorted(bad_ids | set(good) | {p["id"] for p in pauses})
    for i in need:
        src = os.path.join(a.hunt, "wav", f"{i}.wav")
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(TEMP, "rt", f"{i}.wav"))

    def page(link):
        """link(kind, id) -> the audio URL for this page."""
        o = [f"<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>"
             f"<title>Read Aloud Listening Board</title><style>{CSS}</style></head><body><main>",
             "<h1>Read Aloud listening board</h1>",
             f"<p class=lede>Glitch hunt, round 2 corpus (finance, tech, politics, government text). {len(glitch)} residual round-trip mismatches in {len(bad_ids)} sentences, "
             f"{len(pauses)} long pauses, 20 random clean sentences, and 15 before/after showcase lines. "
             "Voice: Kokoro af_heart on CPU at 1.0x. &ldquo;Heard&rdquo; is what Whisper large-v3 wrote down.</p>",
             "<nav><a href='#round2'>Round 2</a><a href='#round1'>Round 1</a><a href='#show'>Before / after</a><a href='#glitch'>Residual glitches</a><a href='#pause'>Pauses</a><a href='#good'>Clean sentences</a></nav>"]
        if a.gate and os.path.exists(a.gate):
            g = json.load(open(a.gate, encoding="utf-8"))
            o.append("<h2>Gate</h2><div class=scroll><table><tr><th>Measure</th><th>Baseline</th><th>After tasks 1-5</th><th>After fix rounds</th><th>Target</th></tr>")
            for r in g:
                o.append("<tr>" + "".join(f"<td class=num>{html.escape(str(x))}</td>" for x in r) + "</tr>")
            o.append("</table></div>")
        tb = sum(x for x in (seconds(a.r2_before, i) for i in r2) if x)
        ta = sum(x for x in (seconds(a.r2_after, i) for i in r2) if x)
        o.append("<h2 id=round2>Round 2: finance, tech, politics, sports, weather, recipes</h2><p class=lede>25 lines at "
                 "<b>2.2x</b>, before round 2 (748873a) vs after, with the chunks back to back as the player plays them. "
                 "Listen for: basis points, year over year, fiscal and half-year codes, tickers after an exchange, "
                 "RTX forty ninety, party tags, W/L scores, wind directions, recipe units, em dashes and bullets as pauses, "
                 "and the shorter gaps between sentences (the model's own lead-in and end pause are trimmed). "
                 f"Total audio {tb:.1f} s before, {ta:.1f} s after.</p>"
                 "<div class=scroll><table><tr><th>Line</th><th>Before (748873a)</th><th>After (round 2)</th></tr>")
        for i, t in r2.items():
            sb, sa = seconds(a.r2_before, i), seconds(a.r2_after, i)
            lb = f" <span class=muted>{sb:.1f} s</span>" if sb else ""
            la = f" <span class=muted>{sa:.1f} s</span>" if sa else ""
            o.append(f"<tr><td>{html.escape(t)}</td><td>{audio(link('r2-before', i))}{lb}<div class=muted>{html.escape(r2b.get(i, ''))}</div></td>"
                     f"<td>{audio(link('r2-after', i))}{la}<div class=muted>{html.escape(r2a.get(i, ''))}</div></td></tr>")
        o.append("</table></div>")
        o.append("<h2 id=round1>Round 1: acronyms, emphasis, units</h2><p class=lede>20 lines, before round 1 "
                 "(2703e6b) vs after. Dense acronyms stay spelled (SAT, ACT, ER, ETA), emphasis caps read as words, "
                 "units are plural after any number but 1, 60/40 is two numbers, a time zone stays in the phrase.</p>"
                 "<div class=scroll><table><tr><th>Line</th><th>Before (2703e6b)</th><th>After (round 1)</th></tr>")
        for i, t in r1.items():
            o.append(f"<tr><td>{html.escape(t)}</td><td>{audio(link('r1-before', i))}<div class=muted>{html.escape(r1b.get(i, ''))}</div></td>"
                     f"<td>{audio(link('r1-after', i))}<div class=muted>{html.escape(r1a.get(i, ''))}</div></td></tr>")
        o.append("</table></div>")
        o.append("<h2 id=show>Before and after: 15 showcase lines</h2><div class=scroll><table><tr><th>Line</th><th>Before (feat/read-aloud)</th><th>After (phase 3)</th></tr>")
        for i, t in show.items():
            o.append(f"<tr><td>{html.escape(t)}</td><td>{audio(link('before', i))}<div class=muted>{html.escape(hb.get(i, ''))}</div></td>"
                     f"<td>{audio(link('after', i))}<div class=muted>{html.escape(ha.get(i, ''))}</div></td></tr>")
        o.append("</table></div>")
        o.append("<h2 id=glitch>Residual round-trip glitches</h2><p class=lede>Every mismatch the scorer still calls a glitch. "
                 "Many are Whisper spelling a name differently; listen to judge.</p><div class=scroll><table>"
                 "<tr><th>Audio</th><th>Sentence</th><th>Written</th><th>Heard</th><th>Type</th></tr>")
        for r in sorted(glitch, key=lambda r: (r["stratum"], r["category"], r["id"])):
            o.append(f"<tr><td>{audio(link('rt', r['id']))}</td><td>{html.escape(sample.get(r['id'], r['text']))}"
                     f"<div class=muted>Whisper: {html.escape(trans.get(r['id'], ''))}</div></td>"
                     f"<td class=bad>{html.escape(r['ref_words'])}<div class=muted>{html.escape(r['ref_norm'])}</div></td>"
                     f"<td>{html.escape(r['heard'])}</td><td class=muted>{html.escape(r['category'])}<br>{html.escape(r['stratum'])}</td></tr>")
        o.append("</table></div>")
        o.append("<h2 id=pause>Long pauses</h2><p class=lede>A silence of 350 ms or more after a word with no punctuation, "
                 "mid-chunk (from the synth word timings). An A/B test showed the same pause when the number is typed out as words, "
                 "so these come from the voice model's phrasing, not the text rules.</p><div class=scroll><table><tr><th>Audio</th><th>Sentence</th><th>Word</th><th>Pause</th></tr>")
        if not pauses:
            o.append("<tr><td colspan=4 class=good>None found.</td></tr>")
        for p in pauses:
            o.append(f"<tr><td>{audio(link('rt', p['id']))}</td><td>{html.escape(sample.get(p['id'], ''))}</td>"
                     f"<td>{html.escape(p['word'])} | {html.escape(p.get('next', ''))}</td>"
                     f"<td class=num>{p['ms']} ms at {p['at_ms'] / 1000:.1f} s</td></tr>")
        o.append("</table></div>")
        o.append("<h2 id=good>20 random clean sentences</h2><div class=scroll><table><tr><th>Audio</th><th>Sentence</th><th>Heard</th></tr>")
        for i in good:
            o.append(f"<tr><td>{audio(link('rt', i))}</td><td>{html.escape(sample[i])}</td><td class=muted>{html.escape(trans.get(i, ''))}</td></tr>")
        o.append("</table></div></main></body></html>")
        return "\n".join(o)

    def file_url(kind, i):
        d = {"before": a.before, "after": a.after, "rt": os.path.join(TEMP, "rt"),
             "r1-before": a.r1_before, "r1-after": a.r1_after, "r2-before": a.r2_before, "r2-after": a.r2_after}[kind]
        return "file:///" + os.path.abspath(os.path.join(d, f"{i}.wav")).replace("\\", "/")

    abs_html = page(file_url)
    for p in (os.path.join(TEMP, "glitch_board.html"), os.path.join(HERE, "glitch_board.html")):
        open(p, "w", encoding="utf-8").write(abs_html)

    # self-contained copy with relative links
    board = os.path.join(HERE, "board")
    for kind, d in (("before", a.before), ("after", a.after), ("rt", os.path.join(TEMP, "rt")),
                    ("r1-before", a.r1_before), ("r1-after", a.r1_after),
                    ("r2-before", a.r2_before), ("r2-after", a.r2_after)):
        if not os.path.isdir(d):
            continue
        os.makedirs(os.path.join(board, kind), exist_ok=True)
        for f in os.listdir(d):
            if f.endswith(".wav"):
                shutil.copyfile(os.path.join(d, f), os.path.join(board, kind, f))
    open(os.path.join(board, "glitch_board.html"), "w", encoding="utf-8").write(page(lambda k, i: f"{k}/{i}.wav"))
    print(f"board: {len(glitch)} glitches, {len(pauses)} pauses, {len(good)} clean -> {os.path.join(board, 'glitch_board.html')}")


if __name__ == "__main__":
    main()
