"""Static pass analysis: static.jsonl (from the Rust glitch_static test) -> static.csv
(one row per flagged word) + static_summary.json. No audio involved.

Pre-pass aware: a word whose fed text is "" sits inside a span ("billion" in
"$5 billion") and is spoken by the span's first word, so it is never flagged on its
own. An override word ("[PhD](/...)") says what the normaliser or hand list chose,
so a letter-by-letter reading there is intended. Phoneme checks ignore the length
mark (misaki-rs writes "dˈɑːlɚz" for "dollars")."""
import csv, json, os, re, sys, unicodedata
from collections import Counter, defaultdict
from categorize import category, EXPECTED_SPELLED, WORD_ACRONYMS, ROMAN, UNITS, ABBREV

HERE = os.path.dirname(os.path.abspath(__file__))
STRESS = "ˈˌ"
PUNCT6 = set(";:,.!?")
QUOTES = set("\"'“”‘’()[]{}«»")
VOWELS = set("AIOWYaeiouæɑɒɔəɛɜɪʊʌᵻɐ")

# Valid Roman-numeral letters that are everyday acronyms, spelled on purpose (round 2)
ROMAN_ACRONYMS = {"CC", "CD", "CI", "CV", "DC", "DM", "IV", "MC", "MD", "MI", "VC", "XL"}
MODEL_NAMES = re.compile(r"\b(Llama|Mistral|Mixtral|Qwen\S*|Gemma|Falcon|Phi|DeepSeek\S*|Grok|Claude|GPT\S*)\b")

# Round 2: finance / sports / weather shorthand that must be expanded, not read as
# letters or left to misaki ("bps", "Y/Y", "FY25", "1H24", "mb", "W 3-1").
SHORTHAND = re.compile(r"^(bps?|pp|Y/Y|Q/Q|M/M|YoY|QoQ|MoM|FY\d{2,4}|[12]H\d{2,4}|[1-4]Q\d{2,4}|mb|kcal|Tbsp|NW|NE|SE|SW|"
                       r"(NASDAQ|NYSE|Nasdaq):[A-Z.]+|[DRI]-[A-Z][A-Za-z.]+|\+/-.*|\d+(\.\d+)?[KM]\+?)$")
SCORE = re.compile(r"^\d{1,3}[-\u2013]\d{1,3}[,.;]?$")

def letters(s):
    return sum(c.isalpha() for c in s)

def looks_spelled(text, ph):
    """True when phonemes are a letter-by-letter reading: one stressed vowel group per letter."""
    n = letters(text)
    if n < 2 or not text.isalpha():
        return False
    st = sum(c in STRESS for c in ph)
    return st >= n

# Plural count nouns: "1500 workers" is a cardinal, so a cardinal reading is right.
COUNT_NOUNS = set("""acres adults agents airports apps articles banks barrels beds birds boats books
buildings businesses cars cases cells centers children cities citizens civilians companies copies countries
customers days deaths delegates devices dogs dollars doctors drivers employees engineers euros events
families farms feet files firms games guns hectares homes hospitals hours households houses images islands
items jobs kids kilometers kilometres lines liters litres machines members men meters metres miles minutes
missiles months nurses officers pages participants patients people persons pieces planes players points
police pounds prisoners products projects records refugees residents rooms rows samples schools seats
seconds servers shares ships sites soldiers species staff stations stores students subscribers tanks
teachers teams tests times tonnes tons towns troops trucks units users vehicles videos villages visitors
votes voters websites weeks women words workers yards years""".split())

def span_spelled(core, cat, spoken, phs):
    """Letter-reading glitches on a span's first word, judged on what the span says:
    the spoken text spells exactly the uppercase single letters it lists ("U S"), so
    a stress mark per letter there is intended. More stress marks than those letters
    plus two per other word means some word came out letter by letter."""
    words = re.findall(r"[A-Za-z]+(?:'[A-Za-z]+)?", spoken)
    letters = [x for x in words if len(x) == 1 and x.isupper()]
    others = [x for x in words if not (len(x) == 1 and x.isupper())]
    flags = []
    plain = core.replace(".", "")
    if cat == "roman" and len(letters) >= 2 and "".join(letters) == plain and plain not in ROMAN_ACRONYMS:
        flags.append("ROMAN_SPELLED")
    if plain.upper() in WORD_ACRONYMS and "".join(letters) == plain.upper():
        flags.append("WORD_ACRONYM_SPELLED")
    head = phs.split("|")[0].split("=", 1)[-1] if phs else ""
    if sum(c in STRESS for c in head) > len(letters) + 2 * len(others) + 1:
        flags.append("SPELLED")
    return flags

def main():
    rows = [json.loads(l) for l in open(os.path.join(HERE, "static.jsonl"), encoding="utf-8")]
    out = []
    total_words = 0
    gru = Counter()
    sent_words = defaultdict(int)
    prev_chunk = {}
    for r in rows:
        if "err" in r:
            out.append(dict(id=r["id"], src="", word="", context="", fed="", tokens="", phonemes="", flag="G2P_ERROR", category="error", detail=r["err"]))
            continue
        words = r["words"]
        toks = r["toks"]
        n = len(words)
        # words inside a span are spoken by the span's first word: not counted twice
        n_spoken = sum(1 for _, f in words if f != "")
        total_words += n_spoken
        sent_words[r["id"]] += n_spoken
        spans = {a: (b, sp) for a, b, sp in r.get("spans", [])}
        # assign tokens to words: owned tokens -> owner; unowned -> leading symbol of next word or trailing of prev
        by_word = defaultdict(list)
        last_owner = -1
        for k, (t, ph, o, fb, tag) in enumerate(toks):
            if o >= 0:
                by_word[o].append((k, t, ph, fb, True))
                last_owner = o
            else:
                nxt = next((tk[2] for tk in toks[k + 1:] if tk[2] >= 0), None)
                # a symbol opening the next word, also inside its bracket ("($DIS)")
                if nxt is not None and nxt != last_owner and words[nxt][1].lstrip().lstrip("([{\"'“").startswith(t):
                    by_word[nxt].append((k, t, ph, fb, False))
                elif last_owner >= 0:
                    by_word[last_owner].append((k, t, ph, fb, False))
        ctx_all = " ".join(w for w, _ in words)
        # (round 2) a chunk cut after an abbreviation's period: the next chunk starts
        # lowercase, with a digit or with "§" ("tbsp. | kosher", "Cat. | 5", "U.S.C. | §")
        pw = prev_chunk.get(r["id"])
        # only after an abbreviation-shaped word (dotted, or 5 letters or fewer): "Corp.",
        # "a.m.", "tbsp."; an ordinary word ("containers.") ends a real sentence
        pw_letters = sum(c.isalpha() for c in (pw or ""))
        abbrev_like = bool(pw) and (re.search(r"[A-Za-z]\.[A-Za-z]", pw) is not None or pw_letters <= 4)
        if pw and abbrev_like and re.search(r"[A-Za-z]\.[\"')\]]*$", pw) and words:
            nxt0 = re.sub(r"^[\"'(\[]+", "", words[0][0])
            if nxt0[:1].islower() or nxt0[:1].isdigit() or nxt0[:1] == "§":
                out.append(dict(id=r["id"], src=r["src"], word=pw, context=ctx_all[:160], fed="", tokens="", phonemes="",
                                flag="FALSE_SENTENCE_BREAK", category=category(pw), detail=words[0][0]))
        prev_chunk[r["id"]] = words[-1][0] if words else ""
        # coverage: an inner word of a multi-word span that is a plain word ("billion",
        # "War", "Louis") must be in the span's spoken text. Inner words the span
        # rewrites on purpose are not checked: units and abbreviations ("km" ->
        # kilometers), ordinal suffixes ("4th"), and letters it spells ("PM" -> "P M").
        for a, (b, sp) in spans.items():
            said = set(re.findall(r"[a-z]+", sp.lower()))
            spelled = "".join(x for x in sp.split() if len(x) == 1 and x.isupper()).lower()
            for j in range(a + 1, min(b, n)):
                w = words[j][0]
                c = re.sub(r"^\W+|\W+$", "", w)
                plain_word = re.fullmatch(r"[A-Za-z][a-z]{2,}", c) and c.lower() not in UNITS and c.lower() not in ABBREV                     and c.lower() not in ("tbsp", "tsp")
                # a plural unit the span says in the singular ("3/4 cups" -> "of a cup")
                singular = c.lower().endswith("s") and c.lower()[:-1] in said
                # an abbreviation the span says in full ("Civ." -> "civil")
                singular = singular or any(x.startswith(c.lower()) and len(x) > len(c) for x in said)
                if plain_word and c.lower() not in said and c.lower() not in spelled and not singular:
                    out.append(dict(id=r["id"], src=r["src"], word=w, context=" ".join(x for x, _ in words)[:160], fed="",
                                    tokens="", phonemes="", flag="SPAN_COVERAGE", category=category(w), detail=sp))
        for i, (w, fed) in enumerate(words):
            if fed == "":
                continue  # inside a pre-pass span: spoken by the span's first word
            # a span's first word says the span's spoken text (`spoken`), not its own letters
            spoken_txt = spans[i][1] if i in spans else None
            tl = sorted(by_word.get(i, []))
            phs = "|".join(f"{t}={ph}" for _, t, ph, _, _ in tl)
            phs_n = phs.replace("ː", "")
            cat = category(w)
            flags = []
            core = re.sub(r"^\W+|\W+$", "", w)
            spoken = [(t, ph) for _, t, ph, _, own in tl if own]
            # (a) letter-by-letter
            if spoken_txt is not None:
                flags += span_spelled(core, cat, spoken_txt, phs)
            for _, t, ph, fb, own in tl:
                if spoken_txt is not None:
                    break  # checked above against what the span says
                if own and t.isalpha() and (fb == "spell" or looks_spelled(t, ph)) and len(t) >= 2:
                    if t.isupper():
                        if ROMAN.match(t) and cat == "roman" and t not in ROMAN_ACRONYMS:
                            flags.append("ROMAN_SPELLED")
                        elif t in WORD_ACRONYMS:
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
                        if t == "," and any(d in w for d in "—•"):
                            continue  # round 2: an em dash or bullet inside a word is a pause on purpose
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
            count_noun = nxt.lower() in COUNT_NOUNS
            for _, t, ph, fb, own in tl:
                if own and re.fullmatch(r"\d{4}", t) and 1100 <= int(t) <= 2099 and not count_noun and "," not in core \
                        and cat in ("year", "decade", "date", "other", "number"):
                    y = int(t)
                    thousand_ok = 2000 <= y <= 2009 or y % 1000 == 0
                    hundred_ok = thousand_ok or y % 100 == 0
                    if (("θˈWz" in ph or "θWz" in ph) and not thousand_ok) or ("hˈʌndɹəd" in ph and not hundred_ok):
                        flags.append("YEAR_AS_CARDINAL")
                        break
            # "$7.99": dollars and cents (only for money)
            # ("$1.00" -> "one dollar": zero cents are not said)
            money_sub = cat == "currency" and (any(x in phs_n for x in ("sˈɛnt", "pˈɛns", "pˈɛni"))
                                               or bool(re.search(r"\d\.00?(?!\d)", w)))
            if re.search(r"\d\.\d", w) and "pˈYnt" not in phs_n and "pYnt" not in phs_n and not money_sub:
                flags.append("DECIMAL_NO_POINT")
            if re.search(r"\d", w) and re.search(r"[A-Za-z]", w):
                if spoken_txt is not None:
                    # letter names the span says, minus the "U S" of "US dollars"
                    said = re.sub(r"\bU S (dollars?|cents?)\b", "", spoken_txt)
                    if any(len(x) == 1 and x.isalpha() for x in said.split()):
                        flags.append("ALNUM_LETTERS")
                elif any(own and t.isalpha() and ph.strip() and (looks_spelled(t, ph) or len(t) == 1) for _, t, ph, _, own in tl):
                    flags.append("ALNUM_LETTERS")
            # the ordinal suffix said on its own ("twenty one" + "st")
            if re.fullmatch(r"\W*\d+(st|nd|rd|th)\W*", w) and any(
                    own and t in ("st", "nd", "rd", "th") and ph.strip() for _, t, ph, _, own in tl):
                flags.append("ORDINAL_SPLIT")
            if cat == "currency":
                sym = re.search(r"[$€£¥]", w)
                if sym and not any(x in phs_n for x in ("dˈɑlɚ", "jˈʊɹO", "jˈʊɹɹO", "pˈWnd", "jˈɛn")) and not money_sub:
                    flags.append("CURRENCY_UNIT_MISSING")
            # (round 2) shorthand left as written (static: fed text unchanged or spelled)
            # (round 3) "SE = 0.08" is a standard error, not south-east: spelled is right
            stat = core in ("SE", "NE", "NW", "SW") and i + 1 < n and words[i + 1][0].startswith("=")
            if spoken_txt is None and SHORTHAND.match(core) and not stat and (fed.strip("()[],.;:") == core or "[" in fed):
                flags.append("SHORTHAND_UNEXPANDED")
            # a W/L/D result letter before a score ("Arsenal W 3-1")
            if spoken_txt is None and core in ("W", "L", "D") and i + 1 < n and SCORE.match(words[i + 1][0]):
                flags.append("SHORTHAND_UNEXPANDED")
            if flags:
                flags = list(dict.fromkeys(flags))
                # letters inside codes (10-K, H100, 3D) are read as letter names: correct
                flags = ["INFO_ALNUM_LETTERS" if f == "ALNUM_LETTERS" and cat == "alnum_code" else f for f in flags]
                # an AI model size after the model's name ("Llama 3.1 70B" -> "seventy B")
                if re.fullmatch(r"\W*\d+(\.\d+)?B\W*", w) and MODEL_NAMES.search(ctx_all):
                    flags = ["INFO_MODEL_SIZE" if f == "ALNUM_LETTERS" else f for f in flags]
                # a cashtag ($AAPL, $BTC) is the ticker: the $ is silent on purpose, no "dollars"
                if re.fullmatch(r"\W*\$[A-Z]{1,5}\W*", w):
                    flags = ["INFO_CASHTAG" if f in ("SYMBOL_DROPPED($)", "CURRENCY_UNIT_MISSING") else f for f in flags]
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
    NOT_GLITCH = ("GRU", "INFO_INITIALISM_SPELLED", "INFO_ALNUM_LETTERS", "INFO_EMOJI_SILENT", "INFO_QUOTE_UNK", "INFO_CASHTAG",
                  "INFO_MODEL_SIZE")
    glitch_words = {(o["id"], o["word"], o["context"]) for o in out if o["flag"] not in NOT_GLITCH}
    summ = dict(total_words=total_words, sentences=len(sent_words), flagged_rows=len(out), flags=flag_base,
                glitch_words=len(glitch_words), static_rate_per_1k=round(1000 * len(glitch_words) / total_words, 2),
                gru_unique=len(gru), gru_top=gru.most_common(60),
                by_category=Counter(o["category"] for o in out if o["flag"] not in NOT_GLITCH))
    json.dump(summ, open(os.path.join(HERE, "static_summary.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print(json.dumps({k: v for k, v in summ.items() if k not in ("gru_top",)}, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
