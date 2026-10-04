"""Pick the round-trip sample: stratified over static (flag, category) groups + all
stress sources + random controls. Deterministic (seeded). Writes sample.tsv
(id, stratum, text); controls are marked stratum=control."""
import csv, os, random
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
random.seed(11)
N_CONTROL = int(os.environ.get("GH_CONTROLS", "150"))
PER_GROUP = int(os.environ.get("GH_PER_GROUP", "8"))
CAP = int(os.environ.get("GH_SAMPLE_MAX", "560"))

corpus = {}
for l in open(os.path.join(HERE, "corpus.tsv"), encoding="utf-8"):
    i, src, text = l.rstrip("\n").split("\t", 2)
    corpus[i] = (src, text)
groups = defaultdict(set)
for r in csv.DictReader(open(os.path.join(HERE, "static.csv"), encoding="utf-8")):
    groups[(r["flag"].split("(")[0], r["category"])].add(r["id"])
for i, (src, _) in corpus.items():
    if src.startswith(("stress:", "own:")):
        groups[("STRESS", src)].add(i)

ids = list(corpus)
controls = random.sample(ids, N_CONTROL)
chosen = {i: "control" for i in controls}
pools = {}
for key in sorted(groups):
    pool = sorted(groups[key])
    random.shuffle(pool)
    pools[key] = pool
taken = defaultdict(int)
for rnd in range(PER_GROUP):  # round-robin so every group gets coverage before the cap
    for key, pool in pools.items():
        if len(chosen) >= CAP or taken[key] >= (PER_GROUP if key[0] != "STRESS" else 6):
            continue
        while pool:
            i = pool.pop()
            if i not in chosen:
                chosen[i] = f"{key[0]}/{key[1]}"
                taken[key] += 1
                break
with open(os.path.join(HERE, "sample.tsv"), "w", encoding="utf-8", newline="\n") as f:
    for i, st in chosen.items():
        f.write(f"{i}\t{st}\t{corpus[i][1]}\n")
print(len(chosen), "sentences;", sum(1 for v in chosen.values() if v == "control"), "controls;",
      sum(len(corpus[i][1].split()) for i in chosen), "words")
