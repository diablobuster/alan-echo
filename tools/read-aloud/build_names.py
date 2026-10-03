"""Build src-tauri/data/cmudict.tsv.gz from CMUdict (BSD-2-Clause, cmusphinx).
Keeps the first pronunciation per word, lowercase words, drops comments and
entries with non a-z' characters. Run: py tools/read-aloud/build_names.py [local-cmudict.dict]"""
import gzip, io, re, sys, urllib.request, pathlib
URL = "https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict"
out = pathlib.Path(__file__).resolve().parents[2] / "src-tauri" / "data" / "cmudict.tsv.gz"
if len(sys.argv) > 1:
    raw = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
else:
    raw = urllib.request.urlopen(URL, timeout=60).read().decode("utf-8")
seen, rows = set(), []
for line in raw.splitlines():
    line = line.split("#", 1)[0].strip()
    if not line:
        continue
    word, *ph = line.split()
    word = re.sub(r"\(\d+\)$", "", word).lower()
    if word in seen or not re.fullmatch(r"[a-z][a-z']*", word):
        continue
    seen.add(word)
    rows.append(f"{word}\t{' '.join(ph)}")
out.parent.mkdir(parents=True, exist_ok=True)
with gzip.open(out, "wb", compresslevel=9) as f:
    f.write(("\n".join(rows) + "\n").encode("utf-8"))
print(len(rows), "words ->", out, out.stat().st_size, "bytes")
