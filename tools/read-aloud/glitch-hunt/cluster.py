"""Cluster static + round-trip glitches into cause categories; write current.json and
compare with baseline.json (the regression gate). `python cluster.py --set-baseline`
freezes the current numbers as the baseline. `--static-only` skips the round trip (no
audio): it compares the static numbers only."""
import csv, json, os, re, sys
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
NOT_GLITCH = ("GRU", "INFO_INITIALISM_SPELLED", "INFO_ALNUM_LETTERS", "INFO_EMOJI_SILENT", "INFO_QUOTE_UNK", "INFO_CASHTAG")

def cause_static(flag, cat, word, phon):
    f = flag.split("(")[0]
    if f == "UNK" and ("’" in word or "‘" in word):
        return "curly_apostrophe"
    if cat == "currency" or (f == "UNK" and re.search(r"[€£¥₿₪₹]", word)):
        return "currency"
    if cat in ("year", "decade") or f == "YEAR_AS_CARDINAL":
        return "years_decades"
    if f in ("DECIMAL_NO_POINT",) or (f == "EMPTY" and re.search(r"\d\.\d", word)):
        return "decimals_silent"
    if cat == "percent":
        return "percent"
    if f == "ORDINAL_SPLIT" or cat == "ordinal":
        return "ordinals"
    if cat in ("time", "date"):
        return "dates_times"
    if cat in ("url", "email", "handle", "hashtag"):
        return "url_email_handle"
    if f == "PAUSE_IN_NUMBER":
        return "number_word_pauses"
    if cat in ("alnum_code", "number+unit") or f == "ALNUM_LETTERS":
        return "alnum_codes"
    if cat in ("dotted_initialism",):
        return "dotted_initialisms"
    if cat == "abbrev_period":
        return "abbreviations"
    if f == "ROMAN_SPELLED" or cat == "roman":
        return "roman_numerals"
    if f == "WORD_ACRONYM_SPELLED":
        return "acronym_rules"
    if cat in ("slash", "math", "symbol/emoji", "ampersand") or f == "SYMBOL_DROPPED":
        return "symbols_math"
    if cat == "unit":
        return "units"
    if cat == "non_ascii_name":
        return "non_ascii_letters"
    if f == "UNK":
        return "unknown_char"
    if f == "SPELLED":
        return "mixed_case_spelled"
    if f == "PAUSE_IN_WORD":
        return "pause_in_word"
    if f == "EMPTY":
        return "silent_word"
    return "other_static"

def cause_rt(r):
    sf = set(filter(None, r["static_flags"].split("|")))
    cat = r["category"]
    hard = sf - set(NOT_GLITCH)
    if hard:
        return cause_static(sorted(hard)[0], cat, r["ref_words"], "")
    if cat in ("currency", "percent", "year", "decade", "decimal", "ordinal", "time", "date", "url", "email", "handle",
               "hashtag", "alnum_code", "number+unit", "dotted_initialism", "abbrev_period", "roman", "slash", "math",
               "symbol/emoji", "ampersand", "unit", "phone"):
        return cause_static("", cat, r["ref_words"], "")
    if "GRU" in sf:
        base = re.sub(r"['’]s$|s$", "", re.sub(r"^\W+|\W+$", "", r["ref_words"].split()[0] if r["ref_words"] else ""))
        if cat in ("allcaps_other", "acronym_spelled", "acronym_word") or (base.isupper() and len(base) >= 2):
            return "acronym_rules"  # caps acronym (or its 's / plural) sent to the GRU and read as a word
        if cat == "camelcase":
            return "camelcase"
        return "gru_mispronounced"
    if cat == "camelcase":
        return "camelcase"
    if cat in ("acronym_word", "allcaps_other", "acronym_spelled"):
        return "acronym_rules"
    if cat == "homograph":
        return "homographs"
    if cat == "non_ascii_name":
        return "non_ascii_letters"
    if r["ref_words"].lower().strip("().,;") in ("million", "billion", "trillion") or re.search(r"[$€£¥]", r["text"]) and set(r["heard"].split()) & {"us", "dollar", "euro", "pound"}:
        return "currency"
    if cat in ("capitalized",):
        return "names_brands_lexicon"
    if cat == "possessive/contraction":
        return "possessive_contraction"
    if cat == "hyphenated":
        return "hyphenated"
    if cat == "number":
        return "numbers_other"
    return "common_words_model"  # dictionary word misheard: model slur, function word, or whisper error

def main():
    st = list(csv.DictReader(open(os.path.join(HERE, "static.csv"), encoding="utf-8")))
    ss = json.load(open(os.path.join(HERE, "static_summary.json"), encoding="utf-8"))
    static_only = "--static-only" in sys.argv
    if static_only:
        rt, rs = [], dict(words=0, glitch_words=0, rate_per_1k=None, verdicts={})
    else:
        rt = list(csv.DictReader(open(os.path.join(HERE, "roundtrip.csv"), encoding="utf-8")))
        rs = json.load(open(os.path.join(HERE, "roundtrip_summary.json"), encoding="utf-8"))
    s_causes = defaultdict(set)
    s_ex = defaultdict(list)
    for r in st:
        if r["flag"] in NOT_GLITCH:
            continue
        c = cause_static(r["flag"], r["category"], r["word"], r["phonemes"])
        key = (r["id"], r["word"])
        if key not in s_causes[c]:
            s_causes[c].add(key)
            if len(s_ex[c]) < 8 and r["word"] not in [e[0] for e in s_ex[c]]:
                s_ex[c].append((r["word"], r["phonemes"][:90]))
    r_causes = Counter()
    r_ctrl = Counter()
    r_ex = defaultdict(list)
    for r in rt:
        if r["verdict"] != "GLITCH":
            continue
        c = cause_rt(r)
        r_causes[c] += 1
        if r["stratum"] == "control":
            r_ctrl[c] += 1
        if len(r_ex[c]) < 8:
            r_ex[c].append((r["ref_words"], r["heard"]))
    cur = dict(
        static=dict(words=ss["total_words"], glitch_words=ss["glitch_words"], rate_per_1k=ss["static_rate_per_1k"],
                    by_cause={c: len(v) for c, v in sorted(s_causes.items(), key=lambda x: -len(x[1]))}),
        roundtrip=dict(words=rs["words"], glitch_words=rs["glitch_words"], rate_per_1k=rs["rate_per_1k"],
                       verdicts=rs["verdicts"], by_cause=dict(r_causes.most_common()), control_by_cause=dict(r_ctrl.most_common())),
        examples=dict(static={c: v for c, v in s_ex.items()}, roundtrip={c: v for c, v in r_ex.items()}),
    )
    n_words = cur["static"]["words"] or 1
    cur["static"]["per_1k_by_cause"] = {c: round(1000 * v / n_words, 2) for c, v in cur["static"]["by_cause"].items()}
    json.dump(cur, open(os.path.join(HERE, "current.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    bp = os.path.join(HERE, "baseline.json")
    if "--set-baseline" in sys.argv:
        json.dump(cur, open(bp, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
        print("baseline written")
    print(f"STATIC  {cur['static']['rate_per_1k']}/1k words ({cur['static']['glitch_words']} of {cur['static']['words']})")
    print(f"ROUNDTRIP {cur['roundtrip']['rate_per_1k']} /1k words")
    if os.path.exists(bp) and "--set-baseline" not in sys.argv:
        b = json.load(open(bp, encoding="utf-8"))
        print(f"BASELINE static {b['static']['rate_per_1k']}/1k, roundtrip {b['roundtrip']['rate_per_1k']}")
        print(f"{'cause':28} {'static base':>11} {'now':>6}   {'rt base':>7} {'now':>5}")
        keys = sorted(set(b["static"]["by_cause"]) | set(cur["static"]["by_cause"]) | set(b["roundtrip"]["by_cause"]) | set(cur["roundtrip"]["by_cause"]))
        for k in keys:
            print(f"{k:28} {b['static']['by_cause'].get(k, 0):>11} {cur['static']['by_cause'].get(k, 0):>6}   "
                  f"{b['roundtrip']['by_cause'].get(k, 0):>7} {cur['roundtrip']['by_cause'].get(k, 0):>5}")
    else:
        for k, v in cur["static"]["by_cause"].items():
            print(f"  static {k:28} {v}")
        for k, v in cur["roundtrip"]["by_cause"].items():
            print(f"  rt     {k:28} {v}  (controls {r_ctrl.get(k, 0)})")

if __name__ == "__main__":
    main()
