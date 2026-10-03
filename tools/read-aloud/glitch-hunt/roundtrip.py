"""Round-trip pass: align normalised reference vs Whisper transcript per sample sentence.
Writes roundtrip.csv (one row per mismatch, with FP verdicts) and roundtrip_summary.json."""
import csv, json, os, re
from collections import Counter, defaultdict
from difflib import SequenceMatcher
import jellyfish
from speechnorm import normalize_words
from categorize import category

HERE = os.path.dirname(os.path.abspath(__file__))
FUNC = {"the", "a", "an", "and", "of", "to", "in", "on", "is", "it", "that", "for", "at", "by", "as", "with", "or", "be", "this", "are", "was"}
HALLU = {"thank", "you", "thanks", "for", "watching", "bye"}
CAT_PRI = ["url", "email", "handle", "hashtag", "currency", "percent", "time", "date", "ordinal", "decade", "year", "phone",
           "decimal", "number+unit", "alnum_code", "slash", "math", "roman", "dotted_initialism", "abbrev_period", "unit",
           "ampersand", "symbol/emoji", "acronym_word", "allcaps_other", "acronym_spelled", "camelcase", "non_ascii_name",
           "hyphenated", "possessive/contraction", "homograph", "number", "capitalized", "word", "punct"]

def meta(s):
    return jellyfish.metaphone(re.sub(r"[^a-z]", "", s)) if re.search(r"[a-z]", s) else s

def verdict(ref, hyp, at_edge):
    r, h = "".join(ref), "".join(hyp)
    if r == h:
        return "FP_SPACING"  # compound split/join (data set / dataset)
    if not ref and set(hyp) <= HALLU and at_edge:
        return "FP_HALLUCINATION"
    if r and h and meta(r) == meta(h):
        return "FP_SPELLING"  # same sound, different spelling (Zelenskyy/Zelensky)
    if r and h and jellyfish.jaro_winkler_similarity(r, h) >= 0.88 and len(r) >= 5:
        return "LOW_NEAR"  # close: often a spelling variant, sometimes a mild mispronunciation
    if (set(ref) | set(hyp)) <= FUNC and len(ref) + len(hyp) == 1:
        return "LOW_FUNCWORD"  # single function word dropped/inserted: whisper or model, low confidence
    return "GLITCH"

def main():
    sample = {}
    for l in open(os.path.join(HERE, "sample.tsv"), encoding="utf-8"):
        i, st, text = l.rstrip("\n").split("\t", 2)
        sample[i] = (st, text)
    trans = {}
    for l in open(os.path.join(HERE, "transcripts.jsonl"), encoding="utf-8"):
        j = json.loads(l)
        trans[j["id"]] = j["text"]
    static = defaultdict(set)
    for r in csv.DictReader(open(os.path.join(HERE, "static.csv"), encoding="utf-8")):
        static[(r["id"], r["word"])].add(r["flag"].split("(")[0])
    rows = []
    words_total = Counter()
    glitch_words = Counter()
    for sid, (st, text) in sample.items():
        if sid not in trans:
            continue
        ref_words = text.replace("\\n", " ").split()
        stratum = "control" if st == "control" else "flagged"
        words_total[stratum] += len(ref_words)
        ref = normalize_words(text)
        hyp = normalize_words(trans[sid])
        rt, ht = [t for t, _ in ref], [t for t, _ in hyp]
        sm = SequenceMatcher(None, rt, ht, autojunk=False)
        bad_words = set()
        for op, a1, a2, b1, b2 in sm.get_opcodes():
            if op == "equal":
                continue
            if a1 == a2:  # insertion: belongs to one of the two neighbouring ref words
                widx = {ref[k][1] for k in (a1 - 1, a1) if 0 <= k < len(ref)}
            else:
                widx = {ref[k][1] for k in range(a1, a2)}
            v = verdict(rt[a1:a2], ht[b1:b2], at_edge=(a1 == len(rt) or a1 == 0))
            ws = [ref_words[k] for k in sorted(widx)]
            cats = [category(w) for w in ws]
            cat = min(cats, key=lambda c: CAT_PRI.index(c) if c in CAT_PRI else 99) if cats else "?"
            flags = sorted(set().union(*[static.get((sid, w), set()) for w in ws])) if ws else []
            rows.append(dict(id=sid, stratum=st, verdict=v, ref_words=" ".join(ws), ref_norm=" ".join(rt[a1:a2]),
                             heard=" ".join(ht[b1:b2]), category=cat, static_flags="|".join(flags), text=text[:200],
                             transcript=trans[sid][:200]))
            if v == "GLITCH":
                bad_words |= widx
        glitch_words[stratum] += len(bad_words)
    with open(os.path.join(HERE, "roundtrip.csv"), "w", encoding="utf-8", newline="") as f:
        wr = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        wr.writeheader()
        wr.writerows(rows)
    v = Counter(r["verdict"] for r in rows)
    summ = dict(sentences=len(trans), words=dict(words_total), glitch_words=dict(glitch_words),
                rate_per_1k={k: round(1000 * glitch_words[k] / max(1, words_total[k]), 1) for k in words_total},
                verdicts=dict(v),
                glitch_by_category=Counter(r["category"] for r in rows if r["verdict"] == "GLITCH").most_common(),
                control_glitch_by_category=Counter(r["category"] for r in rows if r["verdict"] == "GLITCH" and r["stratum"] == "control").most_common())
    json.dump(summ, open(os.path.join(HERE, "roundtrip_summary.json"), "w", encoding="utf-8"), indent=1)
    print(json.dumps(summ, indent=1))

if __name__ == "__main__":
    main()
