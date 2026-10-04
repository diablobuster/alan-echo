"""Round-2 glitch-hunt corpus: the kind of text Alex reads (finance, tech, politics,
news). Fetches public-domain US-government text (Federal Reserve, SEC, BLS, BEA,
Treasury, NASA, White House) and SEC EDGAR 8-K earnings exhibits (company text:
local only) into cache_r2/, then adds our own sentences from stress_round2.txt.

Writes corpus_round2.tsv (id, source, text). The cache and the corpus are git-ignored;
only this script and stress_round2.txt are committed.

  python build_round2.py            # cached fetches: offline + identical after the first run
"""
import hashlib, json, os, random, re, sys, time
import requests
from bs4 import BeautifulSoup

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, "cache_r2")
os.makedirs(CACHE, exist_ok=True)
UA = {"User-Agent": "alan-echo-glitch-hunt/0.3 (dev tool)"}
random.seed(11)
TARGET_FETCHED = 1300
BLOCKED = set()  # hosts that answered 403 (www.sec.gov rate-limits unnamed tools): skipped


def get(url):
    key = hashlib.sha1(url.encode()).hexdigest()[:16]
    p = os.path.join(CACHE, key + ".html")
    if os.path.exists(p):
        with open(p, encoding="utf-8") as f:
            return f.read()
    host = url.split("/")[2]
    if host in BLOCKED:
        return ""
    for attempt in range(3):
        try:
            r = requests.get(url, headers=UA, timeout=30)
            if r.status_code == 404:
                return ""
            if r.status_code == 403:
                print("blocked", host, file=sys.stderr)
                BLOCKED.add(host)
                return ""
            r.raise_for_status()
            try:
                text = r.content.decode("utf-8")  # gov pages are UTF-8 even when the header omits it
            except UnicodeDecodeError:
                text = r.text
            with open(p, "w", encoding="utf-8") as f:
                f.write(text)
            time.sleep(0.25)
            return text
        except Exception as e:  # noqa
            print("fetch fail", url, e, file=sys.stderr)
            time.sleep(1 + attempt)
    return ""


def links(html, pat, base):
    out = []
    for m in re.finditer(r'href="([^"#]+)"', html):
        u = m.group(1)
        if u.startswith("/"):
            u = base + u
        if re.search(pat, u) and u not in out:
            out.append(u)
    return out


def paragraphs(html):
    soup = BeautifulSoup(html, "lxml")
    for t in soup(["script", "style", "nav", "header", "footer", "aside", "form"]):
        t.decompose()
    root = soup.find("article") or soup.find("main") or soup.find(id="article") or soup.body or soup
    out = []
    for el in root.find_all(["p", "li"]):
        t = " ".join(el.get_text(" ").split())
        if len(t) >= 40:
            out.append(t)
    return out


SENT_RE = re.compile(r"(?<=[.!?])[\"”']?\s+(?=[A-Z\"“($0-9])")


def sentences(paras):
    out = []
    for para in paras:
        for s in SENT_RE.split(para):
            s = s.strip()
            if 40 <= len(s) <= 320 and not s.lower().startswith(("click", "share", "follow us", "image credit")):
                out.append(s)
    return out


def fed():
    rows = []
    cal = get("https://www.federalreserve.gov/monetarypolicy/fomccalendars.htm")
    urls = links(cal, r"/newsevents/pressreleases/monetary20(2[2-6])\d+a\.htm$", "https://www.federalreserve.gov")
    feed = get("https://www.federalreserve.gov/feeds/press_all.xml")
    urls += [u for u in re.findall(r"https://www\.federalreserve\.gov/newsevents/pressreleases/[a-z]+\d+[a-z]\.htm", feed) if u not in urls]
    for u in urls[:40]:
        rows += [("gov:fed", s) for s in sentences(paragraphs(get(u)))]
    return rows


def sec():
    rows = []
    urls = []
    for page in range(0, 4):
        html = get(f"https://www.sec.gov/newsroom/press-releases?page={page}")
        urls += [u for u in links(html, r"/newsroom/press-releases/20\d\d-\d+", "https://www.sec.gov") if u not in urls]
    for u in urls[:50]:
        rows += [("gov:sec", s) for s in sentences(paragraphs(get(u)))]
    return rows


def bls():
    rows = []
    for r in ["empsit", "cpi", "ppi", "jolts", "eci", "prod2", "realer", "ximpim", "ecec", "laus", "metro", "cewqtr"]:
        rows += [("gov:bls", s) for s in sentences(paragraphs(get(f"https://www.bls.gov/news.release/{r}.nr0.htm")))]
    return rows


def bea():
    rows = []
    idx = get("https://www.bea.gov/news/current-releases")
    urls = links(idx, r"/news/20\d\d/[a-z0-9-]+$", "https://www.bea.gov")
    for u in urls[:25]:
        rows += [("gov:bea", s) for s in sentences(paragraphs(get(u)))]
    return rows


def treasury():
    rows = []
    urls = []
    for page in range(0, 3):
        html = get(f"https://home.treasury.gov/news/press-releases?page={page}")
        urls += [u for u in links(html, r"/news/press-releases/[a-z]{2}\d+$", "https://home.treasury.gov") if u not in urls]
    for u in urls[:40]:
        rows += [("gov:treasury", s) for s in sentences(paragraphs(get(u)))]
    return rows


def nasa():
    rows = []
    for page in range(1, 4):
        feed = get(f"https://www.nasa.gov/news-release/feed/?paged={page}")
        for item in re.findall(r"<content:encoded><!\[CDATA\[(.*?)\]\]></content:encoded>", feed, re.S):
            rows += [("gov:nasa", s) for s in sentences(paragraphs(item))]
    return rows


def whitehouse():
    rows = []
    urls = []
    for page in range(1, 5):
        html = get(f"https://www.whitehouse.gov/briefings-statements/page/{page}/")
        urls += [u for u in links(html, r"whitehouse\.gov/(briefings-statements|presidential-actions|fact-sheets)/20\d\d/\d\d/[^/]+/$", "https://www.whitehouse.gov") if u not in urls]
    for page in range(1, 3):
        html = get(f"https://www.whitehouse.gov/fact-sheets/page/{page}/")
        urls += [u for u in links(html, r"whitehouse\.gov/fact-sheets/20\d\d/\d\d/[^/]+/$", "https://www.whitehouse.gov") if u not in urls]
    for u in urls[:50]:
        rows += [("gov:whitehouse", s) for s in sentences(paragraphs(get(u)))]
    return rows


def fedreg():
    """Federal Register abstracts (public domain): SEC, Fed, Treasury, CFPB, CFTC, FDIC rules."""
    rows = []
    for ag in ["securities-and-exchange-commission", "federal-reserve-system", "treasury-department",
               "consumer-financial-protection-bureau", "commodity-futures-trading-commission",
               "federal-deposit-insurance-corporation"]:
        for page in (1, 2):
            j = get("https://www.federalregister.gov/api/v1/documents.json?per_page=100&page="
                    f"{page}&conditions%5Bagencies%5D%5B%5D={ag}&fields%5B%5D=abstract")
            if not j:
                continue
            for d in json.loads(j).get("results", []):
                if d.get("abstract"):
                    rows += [(f"gov:fedreg", s) for s in sentences([" ".join(d["abstract"].split())])]
    return rows


# SEC EDGAR 8-K earnings press releases (EX-99.1). Company-authored: local only.
EDGAR_CIKS = {
    "AAPL": 320193, "MSFT": 789019, "NVDA": 1045810, "AMZN": 1018724, "GOOGL": 1652044, "META": 1326801,
    "TSLA": 1318605, "JPM": 19617, "GS": 886982, "NFLX": 1065280, "AMD": 2488, "INTC": 50863,
    "COIN": 1679788, "PLTR": 1321655, "DIS": 1744489, "WMT": 104169,
}


def edgar():
    rows = []
    for tick, cik in EDGAR_CIKS.items():
        j = get(f"https://data.sec.gov/submissions/CIK{cik:010d}.json")
        if not j:
            continue
        rec = json.loads(j)["filings"]["recent"]
        n = 0
        for form, acc, doc in zip(rec["form"], rec["accessionNumber"], rec["primaryDocument"]):
            if form != "8-K" or n >= 2:
                continue
            base = f"https://www.sec.gov/Archives/edgar/data/{cik}/{acc.replace('-', '')}/"
            idx = get(base + f"{acc}-index.htm")
            ex = [u for u in links(idx, r"/Archives/edgar/data/\d+/\d+/[^\"]+\.htm$", "https://www.sec.gov")
                  if re.search(r"(ex99|ex-99|exhibit99|ex991|q\d.*(press|earn|release))", u, re.I)]
            if not ex:
                continue
            n += 1
            rows += [(f"edgar:{tick}", s) for s in sentences(paragraphs(get(ex[0])))]
    return rows


def main():
    fetched = []
    for name, fn in [("fed", fed), ("sec", sec), ("bls", bls), ("bea", bea), ("treasury", treasury),
                     ("nasa", nasa), ("whitehouse", whitehouse), ("fedreg", fedreg), ("edgar", edgar)]:
        rows = fn()
        seen = set()
        rows = [r for r in rows if not (r[1] in seen or seen.add(r[1]))]
        print(f"{name}: {len(rows)} sentences", file=sys.stderr)
        fetched.append(rows)
    # an even share per source family (capped), the rest from whatever has more
    per = TARGET_FETCHED // len(fetched)
    picked, rest = [], []
    for rows in fetched:
        random.shuffle(rows)
        picked += rows[:per]
        rest += rows[per:]
    random.shuffle(rest)
    picked += rest[: max(0, TARGET_FETCHED - len(picked))]
    own = []
    with open(os.path.join(HERE, "stress_round2.txt"), encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if line and not line.startswith("#") and "\t" in line:
                own.append(tuple(line.split("\t", 1)))
    out = picked + own
    with open(os.path.join(HERE, "corpus_round2.tsv"), "w", encoding="utf-8", newline="\n") as f:
        for i, (src, s) in enumerate(out):
            f.write(f"{i}\t{src}\t{s}\n")
    words = sum(len(s.split()) for _, s in out)
    print(f"fetched {len(picked)} + own {len(own)} -> {len(out)} sentences, {words} words")


if __name__ == "__main__":
    main()
