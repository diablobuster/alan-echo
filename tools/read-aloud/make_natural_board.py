"""Adds the "Natural reading" section (first on the page) to the Read Aloud listening board.

Dev tooling only. Reads before/after render dirs (`<fixture>-<rate>.wav` + metrics.json),
the fixture texts and the style A/B folder, copies the WAVs to <board>/natural/ (relative
links) and splices the section between <!-- natural:start --> / <!-- natural:end --> right
after the <h1> of glitch_board.html. Re-running replaces the section. The html is backed up
once to glitch_board.before-natural.html.

  python make_natural_board.py [--before DIR] [--after DIR] [--after-plain DIR]
                               [--style DIR] [--board DIR] [--fixtures DIR]
"""
import argparse, csv, html, json, math, os, re, shutil

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = "C:/Users/arowm/alan-echo-nat-bench"
START, END = "<!-- natural:start -->", "<!-- natural:end -->"
CSS = """<style>
#natural .hl{background:var(--card);border:1px solid var(--line);padding:10px 14px;margin:0 0 14px}
#natural .hl p{margin:4px 0}#natural details{margin-top:6px}
#natural pre{white-space:pre-wrap;font:13px ui-monospace,Consolas,monospace;margin:6px 0}
#natural .box{background:var(--card);border:1px solid var(--line);padding:10px 14px;margin:0 0 12px}
#natural .cols{display:flex;flex-wrap:wrap;gap:14px}#natural .cols div{min-width:230px}
#natural h4{margin:0 0 4px;font-size:13px;color:var(--muted)}
</style>"""


def key(r):
    return (r["fixture"], round(r["rate"], 1))


def load(d):
    m = json.load(open(os.path.join(d, "metrics.json"), encoding="utf-8"))
    return {key(r): r for r in m["runs"]}


def wavname(fx, rate):
    return f"{fx}-{'3' if rate == 3.0 else '2.2'}.wav"


def kind_ms(r, kinds):
    n = t = 0
    for k, v in r["by_kind"].items():
        if k in kinds:
            n += v["n"]
            t += v["mean_ms"] * v["n"]
    return (t / n) if n else None


def change_line(b, a):
    parts = []
    hb, ha = kind_ms(b, ("Heading", "AfterHeading")), kind_ms(a, ("Heading", "AfterHeading"))
    if hb is not None and ha is not None:
        parts.append(f"headings now pause ~{ha:.0f} ms (was ~{hb:.0f} ms)")
    lb, la = kind_ms(b, ("ListItem",)), kind_ms(a, ("ListItem",))
    if lb is not None and la is not None:
        parts.append(f"list items ~{la:.0f} ms (was ~{lb:.0f} ms)")
    loud = f"loudness spread {b['loudness_spread_db']:.1f} dB to {a['loudness_spread_db']:.1f} dB"
    if not b["boundaries_n"]:
        return "no structure in this text; only loudness evening-out applies: " + loud
    return "; ".join(parts + [loud])


def player(rel):
    return f"<audio controls preload=none src='{html.escape(rel)}'></audio>"


def cls(n):
    return "good" if n == 0 else "bad"


def sdev(xs):
    m = sum(xs) / len(xs)
    return math.sqrt(sum((x - m) ** 2 for x in xs) / len(xs))


def style_rows(sd):
    rows = {}
    for r in csv.DictReader(open(os.path.join(sd, "metrics.csv"), encoding="utf-8")):
        rows.setdefault(r["variant"], []).append(r)
    out = []
    for v, rs in rows.items():
        db = [20 * math.log10(float(r["speech_rms"])) for r in rs if float(r["speech_rms"]) > 0]
        cen = [float(r["centroid_hz"]) for r in rs]
        f0 = [float(r["f0_hz"]) for r in rs if float(r["f0_hz"]) > 0]
        out.append((v, len(rs), max(db) - min(db), sdev(cen), sdev(f0) if f0 else 0.0))
    return out


def section(a):
    B, M, P = load(a.before), load(a.after), load(a.after_plain)
    keys = sorted(B)
    tot = lambda D, f: sum(D[k][f] for k in keys)
    mean = lambda D, f: sum(D[k][f] for k in keys) / len(keys)
    fb, fm, fp = tot(B, "fused"), tot(M, "fused"), tot(P, "fused")
    nb = tot(B, "boundaries_n")
    sb, sm, sp = mean(B, "loudness_spread_db"), mean(M, "loudness_spread_db"), mean(P, "loudness_spread_db")
    h = ['<div id="natural">', START, CSS, "<h2>Natural reading (2026-10-04 overnight)</h2>",
         "<div class=hl>",
         f"<p><b>Pauses:</b> of {nb} headings and bullets, <b class=bad>{fb}</b> ran together (under 150 ms of silence) before; now <b class=good>{fm}</b> (page markup) and <b class=good>{fp}</b> (plain text).</p>",
         f"<p><b>Loudness:</b> chunk-to-chunk volume swing averaged <b class=bad>{sb:.1f} dB</b> before; now <b class=good>{sm:.1f} dB</b> (markup) and <b class=good>{sp:.1f} dB</b> (plain).</p>",
         "<p><b>How to read this:</b> play before, then after. Headings should breathe, bullets should be separate, and the voice should not jump louder or softer between sentences.</p></div>"]
    k0 = ("01-news", 2.2)
    if k0 in B:
        h.append("<h3>Listen first: news page with &ldquo;Key Takeaways&rdquo; headers and bullets, 2.2x</h3><div class='box cols'>")
        for lab, d in (("Before", "before"), ("After (page with markup)", "after-marked"), ("After (plain text, no markup)", "after-plain")):
            h.append(f"<div><h4>{lab}</h4>{player(f'natural/{d}/{wavname(*k0)}')}</div>")
        h.append("</div>")
    h.append("<h3>Every fixture, 2.2x and 3.0x</h3><div class=scroll><table><tr><th>Fixture</th><th>Rate</th><th>Before</th><th>After (markup)</th><th>After (plain)</th><th>Fused (before / markup / plain)</th><th>What changed</th></tr>")
    for k in keys:
        fx, rate = k
        txt = os.path.join(a.fixtures, fx + ".txt")
        body = html.escape(open(txt, encoding="utf-8").read()) if os.path.exists(txt) else "(text not found)"
        w = wavname(fx, rate)
        h.append(f"<tr><td><b>{fx}</b><details><summary class=muted>Text</summary><pre>{body}</pre></details></td><td class=num>{rate}x</td>"
                 f"<td>{player('natural/before/' + w)}</td><td>{player('natural/after-marked/' + w)}</td><td>{player('natural/after-plain/' + w)}</td>"
                 f"<td class=num><span class={cls(B[k]['fused'])}>{B[k]['fused']}</span> / <span class={cls(M[k]['fused'])}>{M[k]['fused']}</span> / <span class={cls(P[k]['fused'])}>{P[k]['fused']}</span></td>"
                 f"<td>{html.escape(change_line(B[k], M[k]))}</td></tr>")
    h.append("</table></div>")
    h.append("<h3>Style row A/B (your call)</h3><div class=box><p class=muted>The same short sentences read four ways. Lower numbers mean steadier.</p><table><tr><th>Variant</th><th>Listen</th><th>Notes</th></tr>")
    for v, n, vol, cen, f0 in style_rows(a.style):
        h.append(f"<tr><td><b>{html.escape(v)}</b>{' (today)' if v == 'today' else ''}</td><td>{player(f'natural/style/{v}_all.wav')}</td>"
                 f"<td class=muted>{n} chunks; volume swing {vol:.1f} dB; brightness spread {cen:.0f} Hz; pitch spread {f0:.0f} Hz</td></tr>")
    h.append("</table><p><b>We kept today's setting; tell us if a fixed row sounds steadier.</b></p></div>")
    h.append(END + "</div>")
    return "\n".join(h)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--before", default=f"{BENCH}/before")
    ap.add_argument("--after", default=f"{BENCH}/after-marked")
    ap.add_argument("--after-plain", default=f"{BENCH}/after-plain")
    ap.add_argument("--style", default=f"{BENCH}/style")
    ap.add_argument("--fixtures", default=os.path.join(HERE, "natural"))
    ap.add_argument("--board", default="C:/Users/arowm/alan-echo/tools/read-aloud/board")
    a = ap.parse_args()
    nat = os.path.join(a.board, "natural")
    for name, d in (("before", a.before), ("after-marked", a.after), ("after-plain", a.after_plain)):
        os.makedirs(os.path.join(nat, name), exist_ok=True)
        for f in os.listdir(d):
            if f.endswith(".wav"):
                shutil.copyfile(os.path.join(d, f), os.path.join(nat, name, f))
    os.makedirs(os.path.join(nat, "style"), exist_ok=True)
    for f in os.listdir(a.style):
        if f.endswith("_all.wav"):
            shutil.copyfile(os.path.join(a.style, f), os.path.join(nat, "style", f))
    page = os.path.join(a.board, "glitch_board.html")
    bak = os.path.join(a.board, "glitch_board.before-natural.html")
    src = open(page, encoding="utf-8").read()
    if not os.path.exists(bak):
        open(bak, "w", encoding="utf-8").write(src)
    src = re.sub(r'<div id="natural">\s*' + re.escape(START) + r".*?" + re.escape(END) + r"</div>\n?", "", src, flags=re.S)
    m = re.search(r"<h1[^>]*>.*?</h1>\n?", src, re.S)
    if not m:
        raise SystemExit("no <h1> in board html")
    src = src[:m.end()] + section(a) + "\n" + src[m.end():]
    open(page, "w", encoding="utf-8").write(src)
    print("wrote", page)


if __name__ == "__main__":
    main()
