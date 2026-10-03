"""Surface category of a written word (shared by static and round-trip passes)."""
import re

# Initialisms that SHOULD be read letter by letter (spelling them is correct).
EXPECTED_SPELLED = set("""FBI CIA NSA IRS SEC FDA CDC NIH EPA DOJ DOD DHS FAA FCC FTC NLRB GDP CPI PCE PPI ROI EPS IPO ETF
CEO CFO COO CTO WHO IMF WTO ECB BOJ PBOC FOMC FDIC NYSE AI ML LLM GPU CPU RAM SSD API SDK IoT VPN HTML CSS JSON SQL URL
HTTP HTTPS PDF USB HDMI LED OLED LCD NFT DAO DEI HR PR FAQ DIY TBD ETA RSVP NCAA NFL NBA MLB NHL MLS BBC CNN NPR PBS ABC
NBC CBS MSNBC HBO ESPN NAACP ACLU NRA HIV MRI CT ICU ER DNA RNA PTSD ADHD OCD BMI US USA UK EU UN TV PC ID IQ OK AM PM
JFK UTC EST PST GMT CCP TCM AMD IBM HP GE GM BP SAP TSMC ASML AWS LG BYD UAE DRC KGB FSB MI CDU SPD ANC BJP LGBT LGBTQ
GOP DNC RNC MAGA SNAP CBO OMB FEMA ATF DEA USPS NHS ECJ ICC ICJ IAEA NGO NGOs CEOs IPOs ETFs GPUs CPUs APIs PCs TVs""".split())

# Acronyms normally read as WORDS: spelling these is a glitch.
WORD_ACRONYMS = set("""NATO NASA OPEC UNICEF NASDAQ FIFA UEFA NASCAR AIDS SARS COVID AWOL SPAC REIT ASAP EBITDA ESG UNESCO
INTERPOL FEMA GAAP LIDAR RADAR SCOTUS POTUS FLOTUS DARPA OSHA MERCOSUR ASEAN BRICS""".split())

UNITS = set("km mi mph kg mg gb mb tb ghz mhz gbps mah w l m cm mm min ms sq tsp °c °f mpg lb lbs oz bps".split())
ROMAN = re.compile(r"^(?=[MDCLXVI])M{0,3}(CM|CD|D?C{0,3})(XC|XL|L?X{0,3})(IX|IV|V?I{0,3})$")
MONTHS = set("jan feb mar apr jun jul aug sep sept oct nov dec".split())
ABBREV = set("dr mr mrs ms st vs jr sr prof gen sen rep gov lt col capt sgt mt ft etc approx dept inc corp co no vol ch p fig est ext ave amb sec hrs pct incl attn ste pt bros".split())
HOMOGRAPHS = set("read lead live wind present bass polish wound dove refuse produce close minute object record contract desert tear does project upset invalid conduct rebel permit sow".split())

def category(w):
    s = w.strip()
    core = re.sub(r"^[\"'“”‘’(\[{•*]+|[\"'“”‘’)\]},;:!?.…]+$", "", s)
    low = core.lower()
    if re.search(r"https?://|www\.|\.(com|org|net|io|gov)\b|/\w+/", low):
        return "url"
    if re.search(r"\w@\w", core):
        return "email"
    if core.startswith("#") and len(core) > 1:
        return "hashtag"
    if core.startswith("@") and len(core) > 1:
        return "handle"
    if re.search(r"[$€£¥]", core):
        return "currency"
    if "%" in core:
        return "percent"
    if re.search(r"\d{1,2}:\d\d|^[ap]\.m\b|^\d+(am|pm)$", low):
        return "time"
    if re.fullmatch(r"\d{4}-\d\d-\d\d|\d{1,2}/\d{1,2}(/\d{2,4})?", core):
        return "date"
    if low.rstrip(".") in MONTHS and s.endswith("."):
        return "date"
    if re.fullmatch(r"\d+(st|nd|rd|th)", low):
        return "ordinal"
    if re.fullmatch(r"'?\d{2,4}s", low):
        return "decade"
    if re.fullmatch(r"(1[1-9]|20)\d\d", core):
        return "year"
    if re.fullmatch(r"\+?\(?\d[\d\-() ]{6,}\d", core) or re.fullmatch(r"1-\d{3}-\d{3}-\d{4}", core):
        return "phone"
    if re.fullmatch(r"-?\d+\.\d+", core):
        return "decimal"
    if re.fullmatch(r"\d{1,3}(,\d{3})+(\.\d+)?", core):
        return "number"
    if re.fullmatch(r"\d+", core):
        return "number"
    if re.fullmatch(r"\d+/\d+", core) or re.fullmatch(r"\w+/\w+", core):
        return "slash"
    if re.search(r"[=×÷−±≈≥≤√^<>]", core) or core in ("+", "-", "x", "="):
        return "math"
    if re.search(r"\d", core) and re.search(r"[A-Za-z]", core):
        if re.fullmatch(r"[\d.,]+[a-zA-Z]{1,4}", core) and re.sub(r"[\d.,]+", "", low) in UNITS | {"k", "x", "b", "m", "bn", "t"}:
            return "number+unit"
        return "alnum_code"
    if low in UNITS:
        return "unit"
    if re.fullmatch(r"([A-Za-z]\.){2,}[A-Za-z]?\.?", core + "."):
        return "dotted_initialism"
    if s.endswith(".") and low in ABBREV:
        return "abbrev_period"
    if "&" in core:
        return "ampersand"
    if not re.search(r"[A-Za-z0-9]", core):
        return "symbol/emoji" if core else "punct"
    if ROMAN.match(core) and len(core) >= 2:
        return "roman"
    if "-" in core.strip("-"):
        return "hyphenated"
    base = re.sub(r"['’]s$", "", core)
    if base.isupper() and len(base) >= 2:
        if base in WORD_ACRONYMS:
            return "acronym_word"
        if base in EXPECTED_SPELLED:
            return "acronym_spelled"
        return "allcaps_other"
    if re.search(r"[a-z][A-Z]", base) or re.fullmatch(r"[a-z]+[A-Z]\w*", base):
        return "camelcase"
    if re.search(r"['’]", core):
        return "possessive/contraction"
    if low in HOMOGRAPHS:
        return "homograph"
    if not core.isascii():
        return "non_ascii_name"
    if core[:1].isupper():
        return "capitalized"
    return "word"
