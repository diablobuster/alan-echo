"""Static pass analysis: static.jsonl (from the Rust glitch_static test) -> static.csv
(one row per flagged word) + static_summary.json. No audio involved.

Pre-pass aware: a word whose fed text is "" sits inside a span ("billion" in
"$5 billion") and is spoken by the span's first word, so it is never flagged on its
own. An override word ("[PhD](/...)") says what the normaliser or hand list chose,
so a letter-by-letter reading there is intended. Phoneme checks ignore the length
mark (misaki-rs writes "dˈɑːlɚz" for "dollars")."""
import csv, json, os, re, sys, unicodedata
from collections import Counter, defaultdict
from categorize import category, EXPECTED_SPELLED, WORD_ACRONYMS, ROMAN

HERE = os.path.dirname(os.path.abspath(__file__))
STRESS = "ˈˌ"
PUNCT6 = set(";:,.!?")
QUOTES = set("\"'“”‘’()[]{}«»")
VOWELS = set("AIOWYaeiouæɑɒɔəɛɜɪʊʌᵻɐ")

def letters(s):
    return sum(c.isalpha() for c in s)

def looks_spelled(text, ph):
    """True when phonemes are a letter-by-letter reading: one stressed vowel group per letter."""
    n = letters(text)
    if n < 2 or not text.isalpha():
        return False
    st = sum(c in STRESS for c in ph)
    return st >= n

def main():
    rows = [json.loads(l) for l in open(os.path.join(HERE, "static.jsonl"), encoding="utf-8")]
    out = []
    total_words = 0
    gru = Counter()
    sent_words = defaultdict(int)
    for r in rows:
        if "err" in r:
            out.append(dict(id=r["id"], src="", word="", context="", fed="", tokens="", phonemes="", flag="G2P_ERROR", category="error", detail=r["err"]))
            continue
        words = r["words"]
        toks = r["toks"]
        n = len(words)
        total_words += n
        sent_words[r["id"]] += n
        # assign tokens to words: owned tokens -> owner; unowned -> leading symbol of next word or trailing of prev
        by_word = defaultdict(list)
        last_owner = -1
        for k, (t, ph, o, fb, tag) in enumerate(toks):
            if o >= 0:
                by_word[o].append((k, t, ph, fb, True))
                last_owner = o
            else:
                nxt = next((tk[2] for tk in toks[k + 1:] if tk[2] >= 0), None)
                if nxt is not None and nxt != last_owner and words[nxt][1].lstrip().startswith(t):
                    by_word[nxt].append((k, t, ph, fb, False))
                elif last_owner >= 0:
                    by_word[last_owner].append((k, t, ph, fb, False))
        ctx_all = " ".join(w for w, _ in words)
        for i, (w, fed) in enumerate(words):
            if fed == "":
                continue  # inside a pre-pass span: spoken by the span's first word
            span_head = "](/" in fed  # a span's first word or a hand-list override
            tl = sorted(by_word.get(i, []))
            phs = "|".join(f"{t}={ph}" for _, t, ph, _, _ in tl)
            phs_n = phs.replace("ː", "")
            cat = category(w)
            flags = []
            core = re.sub(r"^\W+|\W+$", "", w)
            spoken = [(t, ph) for _, t, ph, _, own in tl if own]
            # (a) letter-by-letter
            for _, t, ph, fb, own in tl:
                if own and t.isalpha() and (fb == "spell" or looks_spelled(t, ph)) and len(t) >= 2:
                    if t.isupper() or span_head:
                        if ROMAN.match(t) and cat == "roman" and not span_head:
                            flags.append("ROMAN_SPELLED")
                        elif t in WORD_ACRONYMS and not span_head:
                            flags.append("WORD_ACRONYM_SPELLED")
                        else:
                            flags.append("INFO_INITIALISM_SPELLED")  # usually correct (FBI, EMI): not a glitch
                    else:
                        flags.append("SPELLED")
                    break
            # (b) GRU
            for _, t, ph, fb, own in tl:
                if fb == "gru":
                    flags.append("GRU")
                    gru[t] += 1
                    break
            # (c) empty / unknown
            if any(c.isalnum() for c in w):
                if not spoken or all(not ph.strip() or ph == "<NONE>" for _, ph in spoken):
                    flags.append("EMPTY")
            if any("❓" in ph or ph == "<NONE>" for _, _, ph, _, _ in tl):
                flags.append("UNK")
            # (d) symbols dropped / odd
            for k, t, ph, fb, own in tl:
                if len(t) == 1 and not t.isalnum() and t not in PUNCT6 and t not in QUOTES and t not in "-‐‑—–…":
                    if not ph.strip() or "❓" in ph:
                        flags.append(f"SYMBOL_DROPPED({t})")
            # (e) pause/punct phoneme inside a word (between two spoken parts of the same word)
            idx = [j for j, x in enumerate(tl) if x[4]]
            if len(idx) >= 2:
                for j in range(idx[0] + 1, idx[-1]):
                    _, t, ph, _, own = tl[j]
                    if not own and (ph.strip() in PUNCT6 or ph == " " or ph == ""):
                        if ph == "" or (t in "-‐‑" and not ph.strip()):
                            continue  # in-word hyphen: dropped on purpose by token_phonemes
                        flags.append(f"PAUSE_IN_WORD({t})")
                        break
            # (e2) multi-space pause inside one token's phonemes (misaki number words: "twenty   five")
            for _, t, ph, fb, own in tl:
                if own and "  " in ph.strip():
                    flags.append("PAUSE_IN_NUMBER")
                    break
            # number readings
            # a year said as a cardinal. "2007" -> "two thousand seven" and "1900" -> "nineteen
            # hundred" are the right year forms; before a count noun ("1500 workers") a
            # cardinal is right.
            nxt = re.sub(r"\W+$", "", words[i + 1][0]) if i + 1 < n and not re.search(r"[,;:.!?]$", w) else ""
            count_noun = bool(re.fullmatch(r"[a-z]+s", nxt))
            for _, t, ph, fb, own in tl:
                if own and re.fullmatch(r"\d{4}", t) and 1100 <= int(t) <= 2099 and not count_noun and "," not in core \
                        and cat in ("year", "decade", "date", "other", "number"):
                    y = int(t)
                    thousand_ok = 2000 <= y <= 2009 or y % 1000 == 0
                    hundred_ok = thousand_ok or y % 100 == 0
                    if (("θˈWz" in ph or "θWz" in ph) and not thousand_ok) or ("hˈʌndɹəd" in ph and not hundred_ok):
                        flags.append("YEAR_AS_CARDINAL")
                        break
            money_sub = any(x in phs_n for x in ("sˈɛnt", "pˈɛns", "pˈɛni"))  # "$7.99": dollars and cents
            if re.search(r"\d\.\d", w) and "pˈYnt" not in phs_n and "pYnt" not in phs_n and not money_sub:
                flags.append("DECIMAL_NO_POINT")
            if re.search(r"\d", w) and re.search(r"[A-Za-z]", w):
                # a span's first word carries the whole reading the normaliser chose
                if not span_head and any(own and t.isalpha() and ph.strip() and (looks_spelled(t, ph) or len(t) == 1)
                                         for _, t, ph, _, own in tl):
                    flags.append("ALNUM_LETTERS")
            # the ordinal suffix said on its own ("twenty one" + "st")
            if re.fullmatch(r"\W*\d+(st|nd|rd|th)\W*", w) and any(
                    own and t in ("st", "nd", "rd", "th") and ph.strip() for _, t, ph, _, own in tl):
                flags.append("ORDINAL_SPLIT")
            if cat == "currency":
                sym = re.search(r"[$€£¥]", w)
                if sym and not any(x in phs_n for x in ("dˈɑlɚ", "jˈʊɹO", "jˈʊɹɹO", "pˈWnd", "jˈɛn")) and not money_sub:
                    flags.append("CURRENCY_UNIT_MISSING")
            if flags:
                flags = list(dict.fromkeys(flags))
                # letters inside codes (10-K, H100, 3D) are read as letter names: correct
                flags = ["INFO_ALNUM_LETTERS" if f == "ALNUM_LETTERS" and cat == "alnum_code" else f for f in flags]
                # emoji have no reading; silence is acceptable
                if "UNK" in flags and all(unicodedata.category(t[0]).startswith("So") or t in ("️",)
                                          for _, t, ph, _, _ in tl if "❓" in ph):
                    flags = ["INFO_EMOJI_SILENT" if f == "UNK" else f for f in flags]
                # curly double quotes become ❓ (silent, only a lost prosody hint)
                if "UNK" in flags and all(t in "“”„«»" for _, t, ph, _, _ in tl if "❓" in ph):
                    flags = ["INFO_QUOTE_UNK" if f == "UNK" else f for f in flags]
                for f in flags:
                    out.append(dict(id=r["id"], src=r["src"], word=w, context=ctx_all[:160], fed=fed, tokens=" ".join(t for _, t, _, _, _ in tl),
                                    phonemes=phs, flag=f, category=cat, detail=""))
    with open(os.path.join(HERE, "static.csv"), "w", encoding="utf-8", newline="") as f:
        wr = csv.DictWriter(f, fieldnames=["id", "src", "word", "context", "fed", "tokens", "phonemes", "flag", "category", "detail"])
        wr.writeheader()
        wr.writerows(out)
    flag_base = Counter(re.sub(r"\(.*", "", o["flag"]) for o in out)
    NOT_GLITCH = ("GRU", "INFO_INITIALISM_SPELLED", "INFO_ALNUM_LETTERS", "INFO_EMOJI_SILENT", "INFO_QUOTE_UNK")
    glitch_words = {(o["id"], o["word"], o["context"]) for o in out if o["flag"] not in NOT_GLITCH}
    summ = dict(total_words=total_words, sentences=len(sent_words), flagged_rows=len(out), flags=flag_base,
                glitch_words=len(glitch_words), static_rate_per_1k=round(1000 * len(glitch_words) / total_words, 2),
                gru_unique=len(gru), gru_top=gru.most_common(60),
                by_category=Counter(o["category"] for o in out if o["flag"] not in NOT_GLITCH))
    json.dump(summ, open(os.path.join(HERE, "static_summary.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print(json.dumps({k: v for k, v in summ.items() if k not in ("gru_top",)}, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
