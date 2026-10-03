"""Build the glitch-hunt corpus: real prose (Wikipedia CC BY-SA, Wikinews CC BY 2.5,
Gutenberg PD), fetched into cache/, plus our own stress sentences from stress.txt.
Writes corpus.tsv (id, source, text) and corpus.txt (text only). The fetched prose,
the cache and the corpus files are git-ignored and never committed (share-alike
licences); only stress.txt and these scripts are.

  python build_corpus.py                 # build corpus.tsv (cached fetches: offline + stable)
  python build_corpus.py --regen-stress  # rewrite stress.txt from the templates below
"""
import json, os, random, re, sys, time
import requests

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, "cache")
os.makedirs(CACHE, exist_ok=True)
UA = {"User-Agent": "alan-echo-glitch-hunt/0.2 (dev tool)"}
random.seed(7)

WIKI = [
    # finance / business
    "Apple Inc.", "Microsoft", "Nvidia", "Tesla, Inc.", "OpenAI", "PayPal", "LinkedIn", "Federal Reserve",
    "Inflation", "S&P 500", "Bitcoin", "Berkshire Hathaway", "JPMorgan Chase", "Amazon (company)",
    "Alphabet Inc.", "Meta Platforms", "Netflix", "Goldman Sachs", "BlackRock", "Exxon Mobil",
    "Walmart", "Dow Jones Industrial Average", "2008 financial crisis", "Initial public offering",
    "Form 10-K", "Quantitative easing", "Silicon Valley Bank", "FTX", "Coinbase", "Robinhood Markets",
    "Salesforce", "Oracle Corporation", "Intel", "Advanced Micro Devices", "Qualcomm", "Broadcom",
    "Taiwan Semiconductor Manufacturing Company", "Samsung Electronics", "SoftBank Group",
    # tech
    "iPhone", "ChatGPT", "Large language model", "GitHub", "YouTube", "TikTok", "WhatsApp",
    "Android (operating system)", "Microsoft Windows", "Linux", "Wi-Fi", "Bluetooth", "5G",
    "Cloud computing", "Amazon Web Services", "Kubernetes", "JavaScript", "Python (programming language)",
    "USB-C", "PlayStation 5", "Xbox", "SpaceX", "Starlink", "Anthropic", "DeepMind", "Hugging Face",
    # science / health
    "James Webb Space Telescope", "CRISPR", "Climate change", "COVID-19 pandemic", "mRNA vaccine",
    "Large Hadron Collider", "Mars 2020", "Artemis program", "Ozempic", "Alzheimer's disease",
    "Black hole", "Higgs boson", "El Niño–Southern Oscillation",
    # politics / world
    "Russian invasion of Ukraine", "2024 United States presidential election", "European Union",
    "NATO", "United Nations Security Council", "Volodymyr Zelenskyy", "Xi Jinping", "Emmanuel Macron",
    "Narendra Modi", "Nancy Pelosi", "Mitch McConnell", "Kamala Harris", "Benjamin Netanyahu",
    "Brexit", "Supreme Court of the United States", "Gaza war", "Taiwan", "Tokyo", "São Paulo",
    "World Health Organization", "International Monetary Fund", "OPEC",
]

def get(url, params, key):
    p = os.path.join(CACHE, re.sub(r"[^A-Za-z0-9]+", "_", key)[:120] + ".json")
    if os.path.exists(p):
        with open(p, encoding="utf-8") as f:
            return json.load(f)
    for attempt in range(3):
        try:
            r = requests.get(url, params=params, headers=UA, timeout=30)
            r.raise_for_status()
            j = r.json()
            with open(p, "w", encoding="utf-8") as f:
                json.dump(j, f)
            time.sleep(0.2)
            return j
        except Exception as e:  # noqa
            print("fetch fail", key, e, file=sys.stderr)
            time.sleep(1 + attempt)
    return None

def extracts(api, titles, key):
    j = get(api, {"action": "query", "prop": "extracts", "explaintext": 1, "exsectionformat": "plain",
                  "titles": "|".join(titles), "format": "json", "redirects": 1}, key)
    out = []
    if not j:
        return out
    for pg in j.get("query", {}).get("pages", {}).values():
        out.append(pg.get("extract", "") or "")
    return out

SENT_RE = re.compile(r"(?<=[.!?])[\"”']?\s+(?=[A-Z\"“(])")

def sentences(text):
    out = []
    for para in text.split("\n"):
        para = para.strip()
        if len(para) < 40 or para.startswith("=="):
            continue
        for s in SENT_RE.split(para):
            s = s.strip()
            n = len(s.split())
            if 6 <= n <= 40 and not re.search(r"[{}\\|<>]|\^|displaystyle", s):
                out.append(s)
    return out

def real_prose():
    rows = []
    for t in WIKI:
        txt = (extracts("https://en.wikipedia.org/w/api.php", [t], "wp_" + t) or [""])[0]
        ss = sentences(txt)
        random.shuffle(ss)
        rows += [("wikipedia:" + t, s) for s in ss[:22]]
    # Wikinews: recent published articles
    j = get("https://en.wikinews.org/w/api.php", {"action": "query", "list": "categorymembers",
            "cmtitle": "Category:Published", "cmlimit": 200, "cmsort": "timestamp", "cmdir": "desc",
            "cmnamespace": 0, "format": "json"}, "wn_published")
    titles = [m["title"] for m in (j or {}).get("query", {}).get("categorymembers", [])]
    for i in range(0, min(len(titles), 120), 20):
        batch = titles[i:i + 20]
        j2 = get("https://en.wikinews.org/w/api.php", {"action": "query", "prop": "extracts", "explaintext": 1,
                 "exlimit": 20, "titles": "|".join(batch), "format": "json"}, f"wn_batch_{i}")
        for pg in (j2 or {}).get("query", {}).get("pages", {}).values():
            ss = sentences(pg.get("extract", "") or "")
            rows += [("wikinews:" + pg.get("title", ""), s) for s in ss[:8]]
    # Gutenberg plain prose (public domain)
    for gid in (1342, 11, 98):
        p = os.path.join(CACHE, f"pg{gid}.txt")
        if not os.path.exists(p):
            try:
                r = requests.get(f"https://www.gutenberg.org/cache/epub/{gid}/pg{gid}.txt", headers=UA, timeout=60)
                r.raise_for_status()
                open(p, "w", encoding="utf-8").write(r.text)
            except Exception as e:  # noqa
                print("gutenberg fail", gid, e, file=sys.stderr)
                continue
        txt = open(p, encoding="utf-8").read()
        a, b = txt.find("*** START"), txt.find("*** END")
        body = txt[a:b] if a > 0 and b > a else txt
        body = re.sub(r"\r?\n(?!\r?\n)", " ", body)
        ss = sentences(body)
        random.shuffle(ss)
        rows += [(f"gutenberg:{gid}", s) for s in ss[:120]]
    return rows

# ---------------------------------------------------------------- stress lists
BRANDS = """Apple Microsoft Nvidia Amazon Alphabet Google Meta Tesla Berkshire JPMorgan ExxonMobil Chevron Walmart
Costco UnitedHealth Pfizer Moderna AbbVie Merck Eli Lilly Novo Nordisk Comcast Verizon AT&T T-Mobile Disney
Netflix Spotify Uber Lyft Airbnb DoorDash Instacart Shopify Stripe Square Block PayPal Venmo Zelle Visa Mastercard
Salesforce Oracle SAP Adobe Intuit ServiceNow Snowflake Palantir Datadog CrowdStrike Cloudflare Okta Zscaler
MongoDB Atlassian HubSpot Zoom Slack Dropbox Box DocuSign Twilio Workday Autodesk Broadcom Qualcomm AMD Intel
Micron TSMC ASML Samsung Sony Nintendo Toyota Honda Hyundai Volkswagen BMW Mercedes-Benz Porsche Ferrari Stellantis
Ford GM Rivian Lucid Boeing Airbus Lockheed Raytheon Caterpillar Deere 3M Honeywell Siemens Philips Unilever Nestlé
PepsiCo Coca-Cola McDonald's Starbucks Chipotle Nike Adidas Lululemon Patagonia Target Kroger Walgreens CVS Humana
Cigna Anthem Aetna Goldman Morgan Stanley Citigroup Wells Fargo BlackRock Vanguard Fidelity Schwab Robinhood Coinbase
Binance Kraken OpenSea Anthropic OpenAI DeepMind Mistral Cohere Perplexity Midjourney ByteDance TikTok Tencent Alibaba
Baidu JD.com Xiaomi Huawei Lenovo Foxconn Infosys Wipro Reliance Aramco Gazprom Rosneft Shell BP TotalEnergies
Equinor Glencore Rio Tinto BHP Vale ArcelorMittal Maersk FedEx UPS DHL Delta JetBlue Ryanair Lufthansa Emirates
Etsy eBay Wayfair Chewy Peloton Roku Pinterest Snap Reddit Discord Twitch Hulu Paramount Warner Bros Lionsgate
Electronic Arts Activision Blizzard Ubisoft Roblox Unity Epic Games Valve SoftBank Rakuten LVMH Hermès Kering
Gucci Prada Burberry Zara H&M Uniqlo IKEA Lego Mattel Hasbro Kellogg's Kraft Heinz Mondelez Hershey Tyson Cargill""".split()

CAMEL = """TechReview LinkedIn PayPal OpenAI iPhone iPad iPod iCloud iMessage macOS iOS watchOS tvOS YouTube
GitHub GitLab DoorDash FedEx PlayStation SoundCloud WordPress JavaScript TypeScript PowerPoint OneDrive SharePoint
BlackRock BlackBerry WeChat WhatsApp DeepMind DeepSeek SpaceX HubSpot MongoDB PostgreSQL MySQL NoSQL DevOps
eBay eToro ZoomInfo SolarWinds CrowdStrike ServiceNow DocuSign DreamWorks MasterCard SunPower FirstEnergy
JetBlue AirPods MacBook ChatGPT GPT-4o ThinkPad CloudFlare RingCentral QuickBooks TurboTax NerdWallet
WeWork SoftBank CoreWeave SuperMicro AppLovin MicroStrategy McKinsey McDonald MacArthur O'Brien DiCaprio
LeBron DeSantis McConnell VanEck SmartThings FaceTime AirDrop CarPlay HomePod GeForce RadeonPro TikTok
InstaCart BuzzFeed HuffPost TechCrunch FiveThirtyEight NewsGuard OnlyFans KickStarter IndieGoGo GoFundMe""".split()

TICKERS = "AAPL MSFT NVDA AMZN GOOGL META TSLA BRK.B JPM XOM UNH LLY V MA AVGO NFLX COST PEP KO WMT DIS INTC AMD QCOM PLTR SNOW CRM ORCL SPY QQQ VOO IWM TLT GLD BTC ETH".split()

LEADERS = """Joe Biden; Donald Trump; Kamala Harris; JD Vance; Nancy Pelosi; Chuck Schumer; Mitch McConnell; Mike Johnson;
Hakeem Jeffries; Alexandria Ocasio-Cortez; Ron DeSantis; Gavin Newsom; Pete Buttigieg; Marco Rubio; Elizabeth Warren;
Bernie Sanders; Volodymyr Zelenskyy; Vladimir Putin; Xi Jinping; Emmanuel Macron; Olaf Scholz; Keir Starmer; Rishi Sunak;
Giorgia Meloni; Narendra Modi; Benjamin Netanyahu; Recep Tayyip Erdoğan; Justin Trudeau; Mark Carney; Claudia Sheinbaum;
Luiz Inácio Lula da Silva; Javier Milei; Fumio Kishida; Shigeru Ishiba; Yoon Suk Yeol; Lee Jae-myung; Kim Jong Un;
Mohammed bin Salman; Ali Khamenei; Masoud Pezeshkian; Cyril Ramaphosa; Bola Tinubu; Ursula von der Leyen;
António Guterres; Jerome Powell; Janet Yellen; Scott Bessent; Christine Lagarde; Kristalina Georgieva; Jensen Huang;
Satya Nadella; Sundar Pichai; Tim Cook; Elon Musk; Mark Zuckerberg; Sam Altman; Dario Amodei; Jamie Dimon; Warren Buffett;
Lisa Su; Andy Jassy; Larry Fink; Ken Griffin; Cathie Wood; Masayoshi Son""".replace("\n", " ").split(";")

PLACES = """Kyiv; Kharkiv; Zaporizhzhia; Mariupol; Gaza; Rafah; Tel Aviv; Tehran; Riyadh; Doha; Beijing; Shenzhen; Shanghai;
Taipei; Seoul; Pyongyang; Tokyo; Osaka; Mumbai; Bengaluru; New Delhi; Karachi; Dhaka; Jakarta; Manila; Hanoi; Ho Chi Minh City;
Bangkok; Kuala Lumpur; Singapore; Sydney; Auckland; Nairobi; Lagos; Johannesburg; Cairo; Addis Ababa; Khartoum; Istanbul;
Ankara; Athens; Rome; Milan; Madrid; Barcelona; Lisbon; Paris; Marseille; Brussels; Amsterdam; Berlin; Munich; Frankfurt;
Warsaw; Prague; Budapest; Vienna; Zurich; Geneva; Stockholm; Oslo; Helsinki; Reykjavik; Dublin; Edinburgh; London; Montreal;
Toronto; Vancouver; Mexico City; Guadalajara; Bogotá; Caracas; Lima; Santiago; Buenos Aires; São Paulo; Rio de Janeiro;
Albuquerque; Tucson; Phoenix; Boise; Spokane; Poughkeepsie; Worcester; Schenectady; Sault Ste. Marie; Des Moines; Baton Rouge;
Chattanooga; Tallahassee; Pensacola; Kalamazoo; Ypsilanti; Wilkes-Barre; Cheyenne; Juneau; Honolulu; Kauai""".replace("\n", " ").split(";")

ACRONYMS = """FBI CIA NSA IRS SEC FDA CDC NIH EPA DOJ DOD DHS FAA FCC FTC NLRB GDP CPI PCE PPI ROI EBITDA EPS IPO ETF REIT
SPAC CEO CFO COO CTO NASA NATO OPEC UNICEF WHO IMF WTO ECB BOJ PBOC FOMC FDIC NYSE NASDAQ AI ML LLM GPU CPU RAM SSD
API SDK SaaS IoT VPN HTML CSS JSON SQL URL HTTP HTTPS PDF USB HDMI LED OLED LCD NFT DAO DeFi ESG DEI HR PR Q&A
FAQ ASAP FYI DIY TBD ETA RSVP AWOL NASCAR NCAA NFL NBA MLB NHL MLS FIFA UEFA BBC CNN NPR PBS ABC NBC CBS MSNBC HBO
ESPN AARP NAACP ACLU NRA AFL-CIO COVID-19 SARS HIV AIDS MRI CT ICU ER DNA RNA mRNA PTSD ADHD OCD BMI""".split()

ABBREV_SENTS = [
    "Dr. Lisa Cook met Mr. Powell and Mrs. Yellen on Main St. near the bank.",
    "Gen. Milley and Lt. Col. Vindman testified before Sen. Grassley and Rep. Jordan.",
    "The co-founder, Jr., and his father, Sr., both worked at Acme Corp. and Smith Bros. Inc.",
    "Prof. Chen published Fig. 3 and Table 2, i.e. the results, e.g. the margins, etc.",
    "The vs. matchup, Lakers vs. Celtics, drew 20 million viewers on Jan. 5.",
    "Mt. Everest and St. Louis are mentioned in vol. 2, ch. 7, p. 114 of the report.",
    "Approx. 40 pct. of respondents, incl. retirees, said no. 1 was cost.",
    "The U.S. and the U.K. signed a deal; the E.U. is reviewing it.",
    "Ft. Worth, Pt. Reyes and Ste. Genevieve are all approx. 3 hrs. away.",
    "Send it to Dept. of Commerce, Attn: Ms. Garcia, Ste. 400, Washington, D.C.",
    "Mon., Tues., Wed. and Thurs. are booked; Fri. is open until 5 p.m.",
    "He holds a Ph.D. from MIT, an M.B.A. from Wharton and a B.A. from Yale.",
    "The St. Patrick's Day parade on 5th Ave. starts at 11 a.m. sharp.",
    "Gov. Newsom, Sec. Blinken and Amb. Thomas-Greenfield spoke at the U.N.",
    "Capt. Sullenberger and Sgt. Pepper are not related, est. 1967.",
]

HOMOGRAPHS = [
    "I read the report yesterday, and today I will read it again.",
    "The lead investigator found lead in the water supply.",
    "They live near the stadium where the band plays live every Friday.",
    "The wind was too strong to wind the sail back in.",
    "She will present the award to the present champion as a present.",
    "The bass player caught a striped bass on Sunday.",
    "We must polish the Polish furniture before the guests arrive.",
    "He wound the bandage around the wound carefully.",
    "The dove dove into the bushes when the hawk appeared.",
    "They refuse to move the refuse out of the alley.",
    "The farm used to produce produce for the city markets.",
    "Please close the door; the store is close to closing.",
    "A minute later, the minute details of the deal leaked.",
    "Officials object to the object found near the border.",
    "The record shows she will record a new album this year.",
    "The contract says the market will contract by 2 percent.",
    "Investors desert the bond market like soldiers desert a post in the desert.",
    "I tear up every time a tear runs down his face.",
    "Does the herd of does graze near the river?",
    "The project will project revenue into the next decade.",
    "Upset fans could upset the incumbent in the primary.",
    "The invalid argument rendered the invalid's claim invalid.",
    "We conduct audits; their conduct was beyond reproach.",
    "The rebels rebel against the new permit rules; the permit was denied.",
    "Tiny sow bugs crawled where farmers sow their seed.",
]

def money():
    out = []
    for _ in range(60):
        n = random.choice([random.randint(1, 999), round(random.uniform(1, 99), 1), round(random.uniform(1, 999), 2)])
        unit = random.choice(["", "", " million", " billion", "M", "B", "K", "bn", "T", " trillion"])
        sym = random.choice(["$", "$", "$", "€", "£", "¥", "US$", "C$"])
        out.append(f"{sym}{n}{unit}")
    tmpl = [
        "The company raised {m} in its latest funding round.",
        "Shares closed at {m}, up from {m2} a year earlier.",
        "Analysts expect revenue of {m} for the quarter, against {m2} last year.",
        "The deal values the startup at {m}, or roughly {p} above its last round.",
        "Microsoft paid {m} for the acquisition, its largest since LinkedIn.",
        "Gas prices hit {m} a gallon in California on Tuesday.",
        "Nvidia's market cap topped {m}, briefly passing Apple.",
        "The fine of {m} is the largest the SEC has imposed this year.",
    ]
    rows = []
    for i in range(70):
        rows.append(random.choice(tmpl).format(m=random.choice(out), m2=random.choice(out), p=pct()))
    return rows

def pct():
    return random.choice([f"{random.randint(1, 99)}%", f"{round(random.uniform(0, 20), 1)}%", f"{round(random.uniform(0, 5), 2)}%",
                          f"-{round(random.uniform(0, 9), 1)}%", f"{random.randint(1, 99)} percent", f"+{random.randint(1, 30)}%"])

def numbers():
    rows = []
    t = [
        "Inflation rose {p} in March, while unemployment held at {p2}.",
        "The 10-year Treasury yield climbed to {d}, its highest since {y}.",
        "The S&P 500 gained {p} on the day, closing at {big}.",
        "Turnout was {p}, with {big} ballots counted by {time}.",
        "The bill passed {v1}-{v2} in the Senate on {date}.",
        "Earnings per share came in at {d} versus the {d2} consensus.",
        "About {big} people were evacuated, according to officials.",
        "The rate cut of {bp} basis points was the first since {y}.",
        "The vote was {v1} to {v2}, with {small} abstentions.",
        "GDP grew at an annualized {p} pace in Q{q} {y}.",
        "The model has {params}B parameters and a {ctx}K context window.",
        "Its ratio of {r} beat the {r2} target set in FY{yy}.",
        "Population: {big}; area: {area} sq km; density: {small} per km².",
        "Version {ver} ships on {date}, replacing version {ver2}.",
        "Pi is roughly {pi}, and e is about 2.718.",
        "The {ord} amendment and the {ord2} district were both cited.",
        "She finished {ord} out of {small} runners in the {y} race.",
        "The {dec}s saw a boom, but the {dec2}s brought a bust.",
        "Q{q} results beat estimates; H{h} guidance was raised.",
        "The odds were 3:1, and the ratio of men to women was 2:3.",
        "Room 1204, Floor 12, Building 7B, Gate C23, Seat 14A.",
        "The score was 24-17 at halftime and 31-28 at the end.",
        "Temperatures hit 104°F (40°C) in Phoenix on {date}.",
        "Mortgage rates fell to 6.85% from 7.12% the week before.",
        "The fund returned 1,234.5% since 2009, or 23.4% a year.",
        "A 1/2 cup of flour, 3/4 tsp of salt and 2 1/2 cups of milk.",
        "Chapter XIV, Section IV and Louis XVI are covered in Part II.",
        "World War II ended in 1945; Henry VIII had six wives; Super Bowl LVIII was in Las Vegas.",
        "The 2nd, 3rd, 21st, 22nd, 101st and 1000th entries were flagged.",
        "Call 1-800-555-0199 or (212) 555-0147, ext. 42, before 9:30 a.m.",
        "Her number is +44 20 7946 0958 and his is 555-123-4567.",
        "Visit https://www.example.com/news?id=42 or email press@example.org for details.",
        "The docs are at github.com/openai/whisper and docs.python.org/3/library.",
        "Follow @elonmusk and @AOC, or search #Election2024 and #AI on X.",
        "Download speeds hit 1.2 Gbps, about 150 MB/s, on the 5G network.",
        "The car gets 35 mpg on the highway and tops out at 155 mph.",
        "The trail is 12.5 km long, roughly 7.8 mi, with 800 m of climbing.",
        "The phone has 8 GB of RAM, a 256GB SSD and a 4,500 mAh battery.",
        "The chip runs at 3.2 GHz and draws 65 W at full load.",
        "Mix 500 mg of the compound into 2 L of water at 25 °C.",
        "The patient weighed 72 kg, stood 1.8 m tall and ran 5k in 22 min.",
        "x + y = 10, x − y = 4, so x × y = 21 and x ÷ y = 2.33.",
        "If a > b and b ≥ c, then a > c; also 2^10 = 1024 and √16 = 4.",
        "The error was ±0.5%, and the result was ≈ 3.14.",
        "The 3 a.m. call came at 03:15 UTC, or 11:15 p.m. EST.",
        "Doors open at 7:30pm; the show starts at 8 PM sharp.",
        "The meeting is 9:00–10:30 AM on Mon, 3/14, and again 4/1.",
        "The filing is dated 2025-03-14 and the deadline is 12/31/2025.",
        "On January 6, 2021, and on 9/11, the nation stopped.",
        "Born 14 July 1789, died March 3rd, 1850.",
        "The 1990s, the '80s and the 2020s were all mentioned.",
        "In 2025, 1999, 1066, 2000, 2008, 1776, 1901 and 2010 the numbers differed.",
        "The firm filed its 10-K and 10-Q, plus an 8-K and a 13F, with the SEC.",
        "Form W-2, form 1099-NEC and Schedule K-1 are due by April 15.",
        "The F-35, the Su-57 and the MiG-29 flew over the B-52.",
        "Covid-19, H5N1 and SARS-CoV-2 were discussed alongside mpox.",
        "Flight UA 2471 departs from JFK Terminal 4 at gate B12.",
        "The S&P 500, the Nasdaq-100 and the FTSE 100 all fell.",
        "Its P/E ratio is 31x, its EV/EBITDA is 18.5x and its P/B is 4.2.",
        "The model hit 92.3% on MMLU and 87% on HumanEval at pass@1.",
        "The iPhone 16 Pro Max, Galaxy S24 Ultra and Pixel 9 Pro XL were tested.",
        "Windows 11 24H2, macOS 15.1 and iOS 18.2 shipped this fall.",
        "PS5, Xbox Series X and Switch 2 sales were compared.",
        "Python 3.12, Node.js 20 and Rust 1.80 were released.",
        "The Boeing 737 MAX 9 and Airbus A321neo were grounded.",
        "The B-2 bomber and the M1 Abrams tank were featured.",
        "The ISBN 978-0-13-468599-1 and DOI 10.1000/xyz123 were cited.",
    ]
    for line in t:
        for _ in range(2 if "{" in line else 1):
            rows.append(line.format(
                p=pct(), p2=pct(), d=f"{round(random.uniform(0, 9), 2)}%", d2=f"${round(random.uniform(0, 9), 2)}",
                y=random.choice([1987, 2000, 2007, 2008, 2019, 2020, 2023, 2024, 2025, 2026, 1999, 2010]),
                yy=random.choice([24, 25, 26]), big=f"{random.randint(1000, 9999999):,}", small=random.randint(2, 99),
                time=random.choice(["9 p.m.", "11:45 PM", "midnight", "6:30am"]),
                date=random.choice(["March 3", "Jan. 20, 2025", "3/14", "12/31", "July 4th", "Sept. 11", "2025-06-01"]),
                v1=random.randint(40, 70), v2=random.randint(20, 50), bp=random.choice([25, 50, 75]),
                q=random.randint(1, 4), h=random.randint(1, 2), params=random.choice([7, 13, 70, 405]),
                ctx=random.choice([8, 32, 128, 200]), r=round(random.uniform(0.5, 3), 2), r2=round(random.uniform(0.5, 3), 2),
                area=f"{random.randint(100, 99999):,}", ver=random.choice(["2.0", "3.1.4", "17.4.1", "1.80"]),
                ver2=random.choice(["1.9", "2.7.18", "16"]), pi="3.14159",
                ord=random.choice(["1st", "2nd", "3rd", "4th", "14th", "21st", "22nd", "23rd", "101st"]),
                ord2=random.choice(["5th", "11th", "12th", "13th", "32nd"]),
                dec=random.choice(["1920", "1960", "1980", "1990"]), dec2=random.choice(["1930", "1970", "2000", "2010"]),
            ))
    return rows

PUNCT = [
    "Wait... what did he just say?",
    "The answer — and this matters — is no.",
    "The answer -- and this matters -- is no.",
    "Prices rose (sharply) in the spring [citation needed].",
    "“We will not back down,” she said. ‘Never.’",
    "\"It's over,\" he said, \"and it's not coming back.\"",
    "• First, cut costs. • Second, raise prices. • Third, wait.",
    "- Item one\n- Item two\n- Item three",
    "1. Open the app. 2. Tap Settings. 3. Choose Privacy.",
    "Step 1: plug it in; step 2: turn it on; step 3: profit!",
    "Note: this is a draft & subject to change; see p. 4.",
    "Rock & roll, R&D, AT&T, P&G, S&P and M&A all use an ampersand.",
    "The cost/benefit ratio and the and/or clause were debated.",
    "Read it here → then go back ← to the start.",
    "I ❤️ NY 🗽 and the 🚀 emoji means to the moon.",
    "The stock went 📈 then 📉 and investors were 😬.",
    "OMG, LOL, BRB and TBH are common in texts.",
    "© 2025 Apple Inc. All rights reserved. iPhone® is a trademark™.",
    "The so-called “Magnificent Seven” — Apple, Microsoft, Alphabet, Amazon, Nvidia, Meta and Tesla — led.",
    "Section § 230 and paragraph ¶ 4 were cited.",
    "Temperatures were in the mid-80s; it was a once-in-a-lifetime, state-of-the-art, well-known event.",
    "The year-over-year, quarter-on-quarter and month-to-date figures were up.",
    "Pre-tax, post-merger, non-GAAP, co-CEO and ex-president were all used.",
    "Mother-in-law, twenty-one, and a well-to-do, self-made, twenty-something founder.",
    "Don't, won't, can't, shouldn't've, y'all, it's, its, they're and o'clock.",
    "The company's, the companies', James's and Jesus' possessions were listed.",
    "Trump's tariffs, Biden's bill, Musk's X and Zuckerberg's Meta made news.",
    "AI's promise, the CEO's pay and NASA's budget were debated.",
    "She said — wait for it — “no.”",
    "Is it 50/50? Maybe 60/40. Either way, it's a toss-up.",
    "Hmm... uh, well, um, OK, I guess.",
    "THE MARKET CRASHED TODAY AS INVESTORS FLED TECH STOCKS.",
    "BREAKING: FED CUTS RATES BY 50 BASIS POINTS, POWELL SAYS MORE TO COME.",
    "NVIDIA, APPLE AND MICROSOFT LED THE NASDAQ HIGHER ON FRIDAY.",
    "WARNING: DO NOT OPERATE HEAVY MACHINERY WHILE TAKING THIS MEDICATION.",
    "The CEO said, in ALL CAPS, that the plan was GREAT and the critics were WRONG.",
]

BRAND_T = [
    "Shares of {b} rose after the earnings report.",
    "{b} announced a new partnership with {b2} on Monday.",
    "Analysts at {b} downgraded {b2} to neutral.",
    "The {b} app was the most downloaded this week.",
    "{b}'s chief executive spoke at the conference.",
    "Regulators are investigating {b} and {b2} over pricing.",
]

def stress():
    rows = []
    for b in BRANDS:
        rows.append(("stress:brand", random.choice(BRAND_T).format(b=b, b2=random.choice(BRANDS))))
    for c in CAMEL:
        rows.append(("stress:camel", random.choice(BRAND_T).format(b=c, b2=random.choice(CAMEL))))
    for i in range(0, len(TICKERS), 3):
        a = TICKERS[i:i + 3]
        rows.append(("stress:ticker", f"Traders bought {', '.join(a[:-1])} and {a[-1]} at the open." if len(a) > 1 else f"Traders bought {a[0]}."))
        rows.append(("stress:ticker", f"({a[0]}) fell 3% while NYSE: {a[-1]} gained."))
    for n in LEADERS:
        n = n.strip()
        rows.append(("stress:leader", random.choice([
            f"{n} said on Tuesday that talks would continue.",
            f"In a statement, {n} rejected the proposal.",
            f"Reporters asked {n} about the deal.",
            f"{n}'s office declined to comment.",
        ])))
    for p in PLACES:
        p = p.strip()
        rows.append(("stress:place", random.choice([
            f"Officials in {p} reported heavy rain overnight.",
            f"The summit will be held in {p} next spring.",
            f"Flights to {p} were cancelled on Sunday.",
        ])))
    for i in range(0, len(ACRONYMS), 3):
        a = ACRONYMS[i:i + 3]
        rows.append(("stress:acronym", f"The {a[0]} and the {a[-1]} both cited {a[1] if len(a) > 2 else a[0]} data." ))
    rows += [("stress:money", s) for s in money()]
    rows += [("stress:numbers", s) for s in numbers()]
    rows += [("stress:abbrev", s) for s in ABBREV_SENTS]
    rows += [("stress:homograph", s) for s in HOMOGRAPHS]
    rows += [("stress:punct", s) for s in PUNCT]
    return rows

def read_stress():
    rows = []
    for l in open(os.path.join(HERE, "stress.txt"), encoding="utf-8"):
        l = l.rstrip("\n")
        if l and not l.startswith("#"):
            src, text = l.split("\t", 1)
            rows.append((src, text))
    return rows

def main():
    if "--regen-stress" in sys.argv:
        real_prose()  # same random state as the frozen corpus was built with
        with open(os.path.join(HERE, "stress.txt"), "w", encoding="utf-8", newline="\n") as f:
            f.write("# Self-written stress sentences for the glitch hunt (source<TAB>text). Ours; safe to commit.\n")
            for src, s in stress():
                f.write(f"{src}\t{s.replace(chr(9), ' ').strip()}\n")
        print("stress.txt rewritten")
        return
    real = real_prose()
    st = read_stress()
    rows = real + st
    seen, out = set(), []
    for src, s in rows:
        s = s.replace("\t", " ").strip()
        if s and s not in seen:
            seen.add(s)
            out.append((src, s))
    with open(os.path.join(HERE, "corpus.tsv"), "w", encoding="utf-8", newline="\n") as f:
        for i, (src, s) in enumerate(out):
            f.write(f"{i}\t{src}\t{s.replace(chr(10), ' \\n ')}\n")
    with open(os.path.join(HERE, "corpus.txt"), "w", encoding="utf-8", newline="\n") as f:
        for _, s in out:
            f.write(s.replace("\n", " ") + "\n")
    words = sum(len(s.split()) for _, s in out)
    print(f"real {len(real)} stress {len(st)} -> {len(out)} sentences, {words} words")

if __name__ == "__main__":
    main()
