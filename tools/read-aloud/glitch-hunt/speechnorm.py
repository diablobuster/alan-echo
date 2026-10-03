"""Tiny spoken-form normaliser shared by the reference text and the Whisper transcript.
normalize_words(text) -> list of (spoken_token, source_word_index). Both sides go
through the same rules, so formatting differences (digits vs words, $ vs dollars)
cancel out and only real pronunciation differences survive.
"""
import re
from num2words import num2words

CURR = {"$": "dollars", "us$": "u s dollars", "c$": "canadian dollars", "a$": "australian dollars",
        "hk$": "hong kong dollars", "€": "euros", "£": "pounds", "¥": "yen"}
SCALE = {"k": "thousand", "m": "million", "mn": "million", "b": "billion", "bn": "billion", "t": "trillion",
         "thousand": "thousand", "million": "million", "billion": "billion", "trillion": "trillion"}
MONTHS = {"jan": "january", "feb": "february", "mar": "march", "apr": "april", "jun": "june", "jul": "july",
          "aug": "august", "sep": "september", "sept": "september", "oct": "october", "nov": "november", "dec": "december"}
ABBR = {"dr": "doctor", "mr": "mister", "mrs": "missus", "ms": "miss", "st": "saint", "vs": "versus", "jr": "junior",
        "sr": "senior", "prof": "professor", "gen": "general", "sen": "senator", "rep": "representative", "gov": "governor",
        "lt": "lieutenant", "col": "colonel", "capt": "captain", "sgt": "sergeant", "mt": "mount", "ft": "fort",
        "etc": "etcetera", "e.g": "for example", "i.e": "that is", "approx": "approximately", "dept": "department",
        "inc": "incorporated", "corp": "corporation", "co": "company", "no": "number", "vol": "volume", "ch": "chapter",
        "p": "page", "fig": "figure", "est": "established", "ext": "extension", "ave": "avenue", "amb": "ambassador",
        "sec": "secretary", "hrs": "hours", "pct": "percent", "incl": "including", "attn": "attention", "ste": "suite"}
UNITS = {"km": "kilometers", "mi": "miles", "mph": "miles per hour", "kg": "kilograms", "mg": "milligrams", "gb": "gigabytes",
         "mb": "megabytes", "tb": "terabytes", "ghz": "gigahertz", "mhz": "megahertz", "gbps": "gigabits per second",
         "mah": "milliamp hours", "w": "watts", "l": "liters", "m": "meters", "cm": "centimeters", "mm": "millimeters",
         "min": "minutes", "ms": "milliseconds", "sq": "square", "tsp": "teaspoon", "°c": "degrees celsius",
         "°f": "degrees fahrenheit", "mpg": "miles per gallon", "lb": "pounds", "lbs": "pounds", "oz": "ounces"}
EQUIV = {"okay": "ok", "o.k": "ok", "percent": "percent", "per cent": "percent", "&": "and", "+": "plus", "@": "at",
         "=": "equals", "×": "times", "÷": "divided by", "−": "minus", "%": "percent", "°": "degrees"}

def _n(x):
    try:
        s = num2words(x)
    except Exception:  # noqa
        return [str(x)]
    return [t for t in re.split(r"[\s\-,]+", s) if t and t != "and"]

def _num(tok):
    """'1,234.5' -> words. Years 1100-2099 read as years. None if not a number."""
    t = tok.replace(",", "")
    if not re.fullmatch(r"-?\d+(\.\d+)?", t):
        return None
    if "." in t:
        a, b = t.split(".")
        return _n(int(a)) + ["point"] + [_n(int(d))[0] for d in b]
    v = int(t)
    if "," not in tok and len(t) == 4 and 1100 <= v <= 2099:
        s = num2words(v, to="year")
        return [w for w in re.split(r"[\s\-]+", s) if w and w != "and"]
    return _n(v)

def _ordinal(tok):
    m = re.fullmatch(r"(\d+)(st|nd|rd|th)", tok)
    if not m:
        return None
    return [w for w in re.split(r"[\s\-]+", num2words(int(m.group(1)), to="ordinal")) if w and w != "and"]

def _stem(w):
    # plural-insensitive (both sides), so "dollar"/"dollars" and "x"/"x's" match
    if len(w) > 3 and w.endswith("'s"):
        w = w[:-2]
    if len(w) > 3 and w.endswith("s") and not w.endswith("ss"):
        w = w[:-1]
    return w

ROMAN = {"i": 1, "v": 5, "x": 10}
GREEK = {"α": " alpha ", "β": " beta ", "γ": " gamma ", "δ": " delta ", "ε": " epsilon ", "μ": " mu ",
         "π": " pi ", "σ": " sigma ", "θ": " theta ", "λ": " lambda ", "ω": " omega "}


def _roman(s):
    total, prev = 0, 0
    for c in reversed(s):
        v = ROMAN[c]
        total = total - v if v < prev else total + v
        prev = max(prev, v)
    return total if 2 <= total <= 39 else None


def word_tokens(w, nxt="", prev_cap=False):
    """Spoken tokens for one written word (best effort). Returns (tokens, pending_suffix)."""
    lw = w.lower().strip()
    lw = lw.replace("’", "'").replace("‘", "'").replace("“", "").replace("”", "").replace('"', "")
    lw = lw.strip("()[]{}<>*•…:;,!?—–")
    lw = lw.rstrip(".")
    if any(g in lw for g in GREEK):  # Aβ -> "a beta" (stays one source word)
        for g, name in GREEK.items():
            lw = lw.replace(g, name)
        return [t for t in re.findall(r"[a-z]+|\d+", lw) for t in (_num(t) or [t])], None
    if not lw:
        return [], None
    if lw in EQUIV:
        return EQUIV[lw].split(), None
    # Task 6 scorer fixes (applied to both sides): Roman numerals II-XXXIX after a
    # capitalised word, versions (v2.1), squared units (km²), Greek letters (below)
    if re.fullmatch(r"x{0,3}(ix|iv|v?i{0,3})", lw) and len(lw) >= 2 and prev_cap and w.strip("()[],.;:!?").isupper():
        v = _roman(lw)
        if v:
            return _n(v), None
    m = re.fullmatch(r"v(\d+(?:\.\d+)*)", lw)
    if m:
        out = ["version"]
        for k, part in enumerate(m.group(1).split(".")):
            out += (["point"] if k else []) + _n(int(part))
        return out, None
    m = re.fullmatch(r"(km|m|cm|mm|mi|ft)(²|2)", lw)
    if m:
        return ["square"] + UNITS.get(m.group(1), m.group(1)).split(), None
    # currency
    m = re.fullmatch(r"(us\$|c\$|a\$|hk\$|[$€£¥])(-?[\d,]+(?:\.\d+)?)(k|m|mn|b|bn|t|thousand|million|billion|trillion)?", lw)
    if m:
        sym, amt, sc = m.groups()
        out = _num(amt) or [amt]
        if sc:
            return out + [SCALE[sc]] + CURR[sym].split(), None
        if SCALE.get(nxt.lower().strip(".,")):
            return out, CURR[sym]  # "$5 billion" -> five billion dollars
        return out + CURR[sym].split(), None
    m = re.fullmatch(r"([+\-]?[\d,]+(?:\.\d+)?)%", lw)
    if m:
        v = m.group(1)
        pre = ["plus"] if v.startswith("+") else (["minus"] if v.startswith("-") else [])
        return pre + (_num(v.lstrip("+-")) or [v]) + ["percent"], None
    o = _ordinal(lw)
    if o:
        return o, None
    n = _num(lw)
    if n:
        return n, None
    m = re.fullmatch(r"'?(\d{2,4})s", lw)
    if m:
        base = _num(m.group(1)) or []
        return base[:-1] + [base[-1] + "s"] if base else [], None
    m = re.fullmatch(r"(\d{1,2}):(\d\d)(am|pm|a\.m|p\.m)?", lw)
    if m:
        h, mi, ap = m.groups()
        out = _n(int(h)) + ([] if mi == "00" else (["oh"] + _n(int(mi)) if mi.startswith("0") else _n(int(mi))))
        if ap:
            out += ["a", "m"] if ap.startswith("a") else ["p", "m"]
        return out, None
    if lw in ("a.m", "am") and nxt == "":
        pass
    if lw in ("a.m", "p.m"):
        return [lw[0], "m"], None
    m = re.fullmatch(r"(\d+)(am|pm)", lw)
    if m:
        return _n(int(m.group(1))) + [m.group(2)[0], "m"], None
    if lw in ABBR and w.rstrip(",;:").endswith("."):
        return ABBR[lw].split(), None
    if lw in MONTHS and w.rstrip(",;:").endswith("."):
        return [MONTHS[lw]], None
    if lw in UNITS and len(lw) >= 2 and lw not in ("ms", "mi", "min", "mb", "lb"):
        return UNITS[lw].split(), None  # single letters (m, l, w) are too ambiguous standalone
    m = re.fullmatch(r"([\d.,]+)(km|mi|mph|kg|mg|gb|mb|tb|ghz|gbps|mah|w|l|m|cm|mm|k|x)", lw)
    if m:
        unit = {"k": ["k"], "x": ["x"]}.get(m.group(2), UNITS.get(m.group(2), m.group(2)).split())
        return (_num(m.group(1)) or [m.group(1)]) + unit, None
    # dotted initialisms: u.s -> u s ; ph.d -> p h d
    if re.fullmatch(r"([a-z]\.)+[a-z]?", lw + ".") or re.fullmatch(r"[a-z](\.[a-z])+", lw):
        return [c for c in lw if c.isalpha()], None
    # split on internal separators, digits <-> letters boundaries
    parts = re.findall(r"\d+(?:[.,]\d+)*|[a-zÀ-ɏ]+(?:'[a-z]+)?|[&%+@=]", lw)
    out = []
    for p in parts:
        if p[0].isdigit():
            out += _num(p) or [p]
        elif p in EQUIV:
            out += EQUIV[p].split()
        else:
            out.append(p)
    return out, None

def _fold(s):
    import unicodedata
    return "".join(c for c in unicodedata.normalize("NFKD", s) if not unicodedata.combining(c))

def normalize_words(text):
    """[(token, word_index)] for a text; word_index = index in text.split()."""
    words = text.replace("\\n", " ").split()
    out = []
    pending = None  # (suffix, origin word index): "$5 billion" -> five billion dollars
    for i, w in enumerate(words):
        pw = words[i - 1].strip("\"'“”‘’([") if i > 0 else ""
        prev_cap = pw[:1].isupper() and pw[-1:].isalpha()
        toks, pend = word_tokens(w, words[i + 1] if i + 1 < len(words) else "", prev_cap)
        for t in toks:
            out.append((_stem(_fold(t)), i))
        if pending:
            out += [(_stem(t), pending[1]) for t in pending[0].split()]
            pending = None
        if pend:
            pending = (pend, i)
    if pending:
        out += [(_stem(t), pending[1]) for t in pending[0].split()]
    return [(t, i) for t, i in out if t]

if __name__ == "__main__":
    import sys
    for s in sys.argv[1:]:
        print([t for t, _ in normalize_words(s)])
