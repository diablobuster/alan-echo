"""Writes stress_round2.txt: our own round-2 sentences in the styles Alex reads
(finance news, earnings calls, tech products, crypto, politics, sports, weather,
recipes). Hand-written lines plus seeded templates. Ours; safe to commit.

  python gen_stress_round2.py
"""
import os, random

HERE = os.path.dirname(os.path.abspath(__file__))
random.seed(23)

HAND = {
    "finance": [
        "Shares of Nvidia (NASDAQ: NVDA) rose 4.2% to $128.40 in premarket trading.",
        "Apple (NASDAQ:AAPL) slipped 1.1% after the report.",
        "JPMorgan Chase & Co. (NYSE: JPM) reported Q3 EPS of $4.37, beating estimates by $0.12.",
        "The S&P 500 closed up 0.8% at 5,712.44, while the Nasdaq Composite gained 1.3%.",
        "The 10-year Treasury yield fell 7 bps to 4.12%.",
        "The Fed cut rates by 50 bps, bringing the target range to 4.75%-5.00%.",
        "Core PCE rose 0.3% M/M and 2.7% Y/Y in August.",
        "Revenue grew 12% Y/Y and 3% Q/Q to $26.0B.",
        "Gross margin expanded 140 bps to 46.3%.",
        "Operating margin fell 2.1 pp to 18.4%.",
        "The company raised its FY25 guidance to $3.10-$3.20 per share.",
        "1H24 revenue came in at $412M, up 9% from 1H23.",
        "Adjusted EBITDA was $1.2bn, a margin of 22%.",
        "The stock trades at 25x forward earnings, versus 18x for the sector.",
        "Bitcoin jumped 6% to $67,250, its highest level since July.",
        "Ether (ETH) fell 3.4% to $2,410 while Solana (SOL) rose 2%.",
        "Tesla shares are up 10x since 2019.",
        "Netflix added 5.1M paid subscribers in Q2, beating the 4.1M consensus.",
        "Microsoft beat by $0.03 on EPS and guided Azure growth to 28%-29% in constant currency.",
        "Brent crude settled at $79.40 a barrel, down $1.12, or 1.4%.",
        "Gold hit a record $2,685/oz on Friday.",
        "The dollar index (DXY) slipped 0.3% to 100.9.",
        "Initial jobless claims fell by 12,000 to 218,000 in the week ended Sept. 28.",
        "Nonfarm payrolls rose by 254K in September, well above the 150K forecast.",
        "The unemployment rate ticked down to 4.1% from 4.2%.",
        "CPI rose 2.4% Y/Y, the smallest gain since Feb. 2021.",
        "Mortgage rates averaged 6.08% for a 30-year fixed loan, per Freddie Mac.",
        "The 2s10s spread turned positive at +5 bps.",
        "Fed funds futures price a 92% chance of a 25 bp cut in November.",
        "Q3 GDP grew at a 2.8% annualized rate (SAAR).",
        "The ECB lowered its deposit rate by 25 bps to 3.25%.",
        "The yen weakened past ¥150 per dollar.",
        "Berkshire Hathaway's cash pile hit a record $325.2 billion.",
        "Palantir (PLTR) surged 8% after it was added to the S&P 500.",
        "Shares fell -3.2% after hours.",
        "Guidance implies +/-2% growth next quarter.",
        "The stock has a P/E of 32 and a P/S of 9.8.",
        "Free cash flow (FCF) was $4.6B, up 21% YoY.",
        "ROE improved to 17.2% from 15.9% a year ago.",
        "The REIT yields 5.4% and trades at 0.9x book value.",
        "Q4 FY2024 revenue was $35.1 billion, up 94% from a year ago.",
        "The bank's CET1 ratio was 15.3%.",
        "AUM rose to $11.5 trillion at Sept. 30.",
        "Coinbase (COIN) shares gained 5.5% as BTC topped $70K.",
        "The ETF saw $1.2B of net inflows in Q3.",
        "It's the stock's best day since Mar. 2020.",
        "Analysts at Goldman Sachs raised their price target to $150 from $135.",
        "The deal values the company at roughly $14.1B, or $35 per share in cash.",
        "Short interest stands at 3.1% of the float.",
        "The yield on the 2-year note rose 4 basis points to 3.64%.",
    ],
    "earnings_call": [
        "Thank you, operator, and good afternoon, everyone.",
        "Revenue for the quarter was $94.9 billion, up 6% year over year.",
        "Services revenue reached an all-time high of $24.2 billion.",
        "We expect Q1 revenue of $37.5 billion, plus or minus 2%.",
        "GAAP gross margin is expected to be 73.0%, plus or minus 50 basis points.",
        "Opex was up 14% Q/Q, driven by compute and infrastructure.",
        "We returned $29 billion to shareholders, including $3.9B in dividends.",
        "Our board approved an additional $110 billion buyback.",
        "Diluted EPS was $1.64, up 12% Y/Y.",
        "Data center revenue was $30.8B, up 112% Y/Y and 17% Q/Q.",
        "Let me now turn the call over to Colette.",
        "Next question, please.",
        "As a reminder, today's call is being recorded.",
        "We're seeing strong demand for Blackwell, and supply is ramping through 1H25.",
        "On a constant-currency basis, revenue grew 9%.",
        "Capex for the year will be roughly $60B-$65B.",
        "Our installed base of active devices passed 2.2 billion.",
        "Average revenue per user (ARPU) rose 7% to $11.82.",
        "We exited the quarter with $67.1B in cash and marketable securities.",
        "Net retention was 115%, and RPO grew 28% to $41.1B.",
    ],
    "tech": [
        "OpenAI released GPT-4o mini, which costs $0.15 per 1M input tokens.",
        "Anthropic's Claude 3.5 Sonnet scored 49% on SWE-bench Verified.",
        "Apple shipped iOS 18.1 with Apple Intelligence on Monday.",
        "The RTX 4090 draws up to 450W and costs $1,599.",
        "Nvidia's H100 and B200 GPUs are sold out through 2025.",
        "Meta's Llama 3.1 405B was trained on 15T tokens.",
        "Google's Gemini 1.5 Pro has a 2M-token context window.",
        "Windows 11 24H2 rolls out to all users this month.",
        "The M4 Max has a 16-core CPU and a 40-core GPU.",
        "Samsung's Galaxy S24 Ultra has a 6.8-inch display and a 200MP camera.",
        "AMD's Ryzen 9 9950X boosts to 5.7GHz.",
        "Download the beta at https://example.com/beta (about 2.3 GB).",
        "Read the full post (https://blog.example.org/2024/ai-update) for details.",
        "The patch fixes CVE-2024-3094 in xz 5.6.1.",
        "macOS Sequoia 15.1 adds iPhone mirroring.",
        "Python 3.13 removes the GIL as an option (PEP 703).",
        "Rust 1.82 was released on Oct. 17.",
        "The Pixel 9 Pro XL starts at $1,099.",
        "Wi-Fi 7 routers offer up to 46 Gbps.",
        "The Vision Pro costs $3,499 in the U.S.",
        "Tesla's FSD v12.5 is rolling out to HW3 cars.",
        "Microsoft's Copilot+ PCs need a 40+ TOPS NPU.",
        "Intel's 18A process enters volume production in 2H25.",
        "TSMC's N2 node is slated for 2025, with A16 to follow in 2026.",
        "The Switch 2 has a 7.9-inch LCD screen.",
        "ChatGPT now has 200M weekly active users, per OpenAI.",
        "The app hit #1 on the App Store in 12 countries.",
        "PS5 Pro launches Nov. 7 for $699.99.",
        "Starlink now serves 4M+ customers in 100 countries.",
        "Llama-3-70B-Instruct runs at ~30 tokens/s on a single A100.",
    ],
    "crypto": [
        "BTC/USD broke above $64,000 overnight.",
        "ETH/BTC fell to 0.039, its lowest since 2021.",
        "Spot bitcoin ETFs saw $556M in inflows on Monday.",
        "The SEC approved 11 spot bitcoin ETFs in Jan. 2024.",
        "Solana's TVL climbed to $6.2B.",
        "USDC's market cap passed $35B.",
        "Dogecoin (DOGE) jumped 12% in 24h.",
        "The halving cut the block reward to 3.125 BTC.",
        "Gas fees on Ethereum fell below 2 gwei.",
        "MicroStrategy (MSTR) now holds 252,220 BTC.",
    ],
    "politics": [
        "The Senate passed the bill 68-32 on Tuesday.",
        "The House voted 218-214 to advance the measure.",
        "Sen. Elizabeth Warren (D-Mass.) called the deal a giveaway.",
        "Rep. Jim Jordan (R-OH) chairs the Judiciary Committee.",
        "The case is Loper Bright Enterprises v. Raimondo, 603 U.S. 369 (2024).",
        "Section 230 of the Communications Decency Act, 47 U.S.C. § 230, is at issue.",
        "The rule is codified at 17 C.F.R. § 240.10b-5.",
        "The court cited Fed. R. Civ. P. 12(b)(6).",
        "H.R. 4366 passed by a vote of 286-134.",
        "S. 2226, the NDAA for FY2024, authorizes $886 billion.",
        "The ruling was 6-3, with Justice Kagan writing the dissent.",
        "Polls show Harris at 48% and Trump at 46%, within the ±3.1-point margin of error.",
        "Turnout was 66.6%, the highest since 1900.",
        "Gov. Josh Shapiro (D) signed the budget on July 11.",
        "The CBO projects a $1.9 trillion deficit for FY2024.",
        "The debt ceiling was suspended until Jan. 1, 2025.",
        "Under 26 U.S.C. § 401(k), employees may defer up to $23,000 in 2024.",
        "The 14th Amendment, Section 3, was the core of the case.",
        "Trump v. United States, 603 U.S. 593 (2024), addressed immunity.",
        "The EU's AI Act enters into force Aug. 1, 2024.",
    ],
    "sports": [
        "The Dodgers beat the Yankees 7-6 in Game 1.",
        "Ohtani went 2-for-4 with a homer and 3 RBIs.",
        "Arsenal W 3-1 vs. Spurs; Chelsea D 1-1 at Villa.",
        "The Chiefs improved to 5-0 with a 26-13 win.",
        "Jokic finished with 32 points, 14 rebounds and 9 assists.",
        "Messi scored in the 87th minute to make it 2-1.",
        "Sinner won 6-3, 6-4, 7-6 (7-4).",
        "The Lakers are 3-1 ATS this season.",
        "He's batting .312 with a .945 OPS.",
        "Mahomes was 24/36 for 291 yds, 2 TD, 1 INT.",
        "The Celtics won 108-102 in OT.",
        "McIlroy shot a 5-under 67 to lead by two.",
        "Verstappen took P1 by 0.214s ahead of Norris.",
        "The Yankees lead the series 3-2.",
        "Final: Bills 31, Jets 20.",
    ],
    "weather": [
        "Lows tonight near -5°F with wind chills of -25°F.",
        "Highs in the mid-80s; 40% chance of storms after 2 p.m.",
        "Winds NW 15-25 mph, gusting to 40 mph.",
        "Expect 6-10 in. of snow above 5,000 ft.",
        "Hurricane Milton is a Cat. 5 storm with 180 mph winds.",
        "The pressure dropped to 897 mb.",
        "Today: sunny, high 72°F (22°C).",
        "A Flood Watch is in effect until 8 PM EDT.",
        "UV index: 9 (very high).",
        "Rainfall totals of 1-3\" are likely, with locally 5\".",
        "Temperatures will fall 20-30 degrees behind the front.",
        "The heat index could reach 110°.",
    ],
    "recipes": [
        "Whisk 1 ½ cups flour with ¾ tsp baking soda.",
        "Add 2 1/4 cups sugar and 1/2 cup butter, softened.",
        "Bake at 350°F (175°C) for 25-30 min.",
        "Season with 1 tbsp. kosher salt and ½ tsp. pepper.",
        "You'll need 3 large eggs, room temp.",
        "Simmer for 10-12 minutes, stirring occasionally.",
        "Makes 12 servings; 240 kcal per serving.",
        "Chill the dough for 1 hr. (or overnight).",
        "Use a 9x13-inch pan, lightly greased.",
        "Stir in 1/3 c. milk and 2 Tbsp honey.",
        "Preheat the oven to 425 degrees F.",
        "Cut into 1/2-inch cubes.",
        "Add 1 (14.5 oz) can diced tomatoes.",
        "Roast 45 min–1 hr, until golden.",
        "Total time: 1 hr 15 min.",
    ],
    "punct": [
        "The market—already shaky—fell another 2%.",
        "It's not a bubble — it's a mania, he said.",
        "\"We're not done,\" Powell said, \"but we're close.\"",
        "'Buy the dip' isn't a strategy; it's a slogan.",
        "Key points: • Revenue up 12% • Margins flat • Guidance raised",
        "Highlights:\n- Revenue: $4.2B\n- EPS: $1.12",
        "1) Rates; 2) earnings; 3) the election.",
        "The CEO's comments — and the CFO's silence — rattled investors.",
        "Q: What's next? A: More cuts.",
        "Sources say the deal is \"days away.\"",
        "The firm's 2024 outlook ... is cautious.",
        "Note: All figures are in USD unless stated otherwise.",
        "(Reporting by Jane Doe; Editing by John Smith)",
        "Updated 9:41 a.m. ET",
        "The call starts at 4:30 p.m. ET / 1:30 p.m. PT.",
        "Markets open at 9:30 am EST and close at 4 pm EST.",
        "The meeting is at 14:00 UTC (10:00 EDT).",
        "Tokyo opens at 9:00 JST.",
        "The figures (see Table 2) are preliminary.",
        "Apple's—and Google's—app stores face new rules.",
    ],
}

TICKERS = [("Apple", "AAPL", "NASDAQ"), ("Microsoft", "MSFT", "NASDAQ"), ("Nvidia", "NVDA", "NASDAQ"),
           ("Amazon", "AMZN", "NASDAQ"), ("Alphabet", "GOOGL", "NASDAQ"), ("Meta", "META", "NASDAQ"),
           ("Tesla", "TSLA", "NASDAQ"), ("JPMorgan", "JPM", "NYSE"), ("Goldman Sachs", "GS", "NYSE"),
           ("Walmart", "WMT", "NYSE"), ("Exxon Mobil", "XOM", "NYSE"), ("Disney", "DIS", "NYSE"),
           ("Coca-Cola", "KO", "NYSE"), ("Boeing", "BA", "NYSE"), ("Intel", "INTC", "NASDAQ"),
           ("AMD", "AMD", "NASDAQ"), ("Netflix", "NFLX", "NASDAQ"), ("Palantir", "PLTR", "NYSE"),
           ("Shopify", "SHOP", "NYSE"), ("Costco", "COST", "NASDAQ"), ("Snowflake", "SNOW", "NYSE"),
           ("Ford", "F", "NYSE"), ("AT&T", "T", "NYSE"), ("Visa", "V", "NYSE"), ("Oracle", "ORCL", "NYSE"),
           ("Broadcom", "AVGO", "NASDAQ"), ("Super Micro", "SMCI", "NASDAQ"), ("Arm", "ARM", "NASDAQ"),
           ("Rivian", "RIVN", "NASDAQ"), ("Moderna", "MRNA", "NASDAQ")]
MODELS = ["GPT-4o", "GPT-4.1", "GPT-5", "o1-preview", "o3-mini", "Claude 3.5 Sonnet", "Claude 3 Opus",
          "Claude Opus 4.1", "Gemini 2.0 Flash", "Llama 3.1 70B", "Mistral 7B", "DeepSeek-V3", "Grok 2",
          "Phi-3", "Qwen2.5-72B", "iOS 18.1", "iOS 17.4", "Android 15", "macOS 14.5", "Windows 11",
          "RTX 4090", "RTX 5080", "RX 7900 XTX", "M3 Pro", "A18 Pro", "Snapdragon 8 Gen 3", "Xbox Series X",
          "iPhone 16 Pro Max", "Galaxy Z Fold6", "Pixel 9", "Core i9-14900K", "Ryzen 7 7800X3D", "H100", "GB200"]
CITIES = [("Columbus", "OH"), ("Portland", "OR"), ("Boise", "ID"), ("Honolulu", "HI"), ("Austin", "TX"),
          ("Phoenix", "AZ"), ("Denver", "CO"), ("Atlanta", "GA"), ("Nashville", "TN"), ("Detroit", "MI"),
          ("Seattle", "WA"), ("Miami", "FL"), ("Albany", "NY"), ("Raleigh", "NC"), ("Omaha", "NE")]


def pct():
    return f"{random.choice([0.4, 1.2, 2.5, 3.8, 6.1, 12, 18.3, 27])}%"


def money():
    return random.choice(["$1.2B", "$450M", "$3.4 billion", "$87.5 million", "$12.7bn", "$2.3T", "$0.42", "$1.08",
                          "$118.50", "$6.1B", "€2.4bn", "£1.1 billion", "$15K", "$999"])


TEMPLATES = {
    "ticker": [
        lambda: (lambda c: f"Shares of {c[0]} ({c[2]}: {c[1]}) {random.choice(['rose', 'fell', 'jumped', 'slid'])} {pct()} on {random.choice(['Monday', 'Tuesday', 'Friday'])}.")(random.choice(TICKERS)),
        lambda: (lambda c: f"{c[0]} ({c[2]}:{c[1]}) reports after the bell.")(random.choice(TICKERS)),
        lambda: (lambda c: f"${c[1]} is up {pct()} premarket.")(random.choice(TICKERS)),
        lambda: (lambda c: f"{c[0]} (${c[1]}) hit a 52-week high.")(random.choice(TICKERS)),
    ],
    "earnings": [
        lambda: f"{random.choice(TICKERS)[0]} posted EPS of ${random.choice(['0.87', '1.12', '2.45', '3.01'])} vs. ${random.choice(['0.84', '1.10', '2.40', '2.95'])} expected.",
        lambda: f"Revenue rose {pct()} Y/Y and {pct()} Q/Q.",
        lambda: f"Margins widened {random.choice([30, 75, 120, 210])} bps to {pct()}.",
        lambda: f"The company beat by ${random.choice(['0.03', '0.05', '0.11', '0.25'])} and raised FY{random.choice(['25', '26', '2025'])} guidance.",
        lambda: f"{random.choice(['1H24', '2H24', '1H25', '2H25', 'Q1 FY25', 'Q4 FY24', '3Q24', 'FY24'])} sales were {money()}.",
        lambda: f"Share of revenue rose {random.choice([0.5, 1.5, 2, 3.4])} pp to {pct()}.",
        lambda: f"The stock trades at {random.choice(['12x', '25x', '40x', '1.5x', '0.8x'])} {random.choice(['earnings', 'sales', 'book', 'EBITDA'])}.",
        lambda: f"Volume was {random.choice(['2x', '3x', '10x', '1.5x'])} the 30-day average.",
        lambda: f"Guidance calls for {random.choice(['+/-', '±'])}{random.choice([1, 2, 3])}% around {money()}.",
        lambda: f"EBITDA came in at {money()}, {random.choice(['up', 'down'])} {pct()} YoY.",
    ],
    "versions": [
        lambda: f"{random.choice(MODELS)} beats {random.choice(MODELS)} on most benchmarks.",
        lambda: f"The update brings {random.choice(MODELS)} support to {random.choice(MODELS)} devices.",
        lambda: f"Benchmarks put the {random.choice(MODELS)} {pct()} ahead of the {random.choice(MODELS)}.",
    ],
    "cities": [
        lambda: (lambda c: f"The plant in {c[0]}, {c[1]}, employs {random.choice([450, 1200, 3000])} people.")(random.choice(CITIES)),
        lambda: (lambda c: f"{c[0]}, {c[1]} (AP) — Officials said Tuesday the vote will go ahead.")(random.choice(CITIES)),
    ],
    "sports": [
        lambda: f"{random.choice(['Yankees', 'Dodgers', 'Mets', 'Cubs', 'Astros'])} {random.choice(['W', 'L'])} {random.randint(1, 9)}-{random.randint(0, 8)}",
        lambda: f"{random.choice(['Judge', 'Soto', 'Betts', 'Alvarez'])} went {random.randint(1, 3)}-for-{random.randint(3, 5)} with {random.randint(1, 4)} RBIs.",
        lambda: f"The {random.choice(['Eagles', 'Ravens', 'Lions', '49ers'])} won {random.randint(20, 38)}-{random.randint(10, 19)} to move to {random.randint(3, 8)}-{random.randint(0, 3)}.",
    ],
    "weather": [
        lambda: f"Low {random.choice(['-5', '-12', '3', '18'])}°F, high {random.choice(['14', '27', '33'])}°F.",
        lambda: f"Wind chill near {random.choice(['-20', '-35', '-8'])}°F overnight.",
        lambda: f"{random.choice(['Snow', 'Rain'])} totals {random.choice(['2-4', '4-8', '1-2'])} in. by {random.choice(['Tuesday', '7 a.m.', 'noon'])}.",
    ],
    "recipes": [
        lambda: f"Add {random.choice(['1 ½', '2 ¼', '1 1/2', '3/4', '1/3', '2 2/3', '½', '¼'])} {random.choice(['cups', 'cup', 'tsp', 'tbsp', 'teaspoons', 'oz'])} {random.choice(['flour', 'sugar', 'milk', 'salt', 'butter'])}.",
        lambda: f"Bake {random.choice(['20-25', '35-40', '12'])} min at {random.choice(['350', '375', '400'])}°F.",
    ],
    "uk_dates": [
        lambda: f"The deadline is {random.choice(['25/12', '31/10', '15/03/2025', '28/02/24', '13/7'])}.",
    ],
    "fractions": [
        lambda: f"Use a {random.choice(['13/16', '5/8', '3/8', '7/16', '1/4'])} inch {random.choice(['bolt', 'drill bit', 'socket'])}.",
        lambda: f"About {random.choice(['2/3', '3/4', '1/5', '7/8'])} of {random.choice(['voters', 'the S&P 500', 'analysts'])} agreed.",
    ],
}


def main():
    rows = []
    for cat, lines in HAND.items():
        rows += [(f"own:{cat}", l.replace("\n", " ")) for l in lines]
    seen = {r[1] for r in rows}
    want = 700
    cats = list(TEMPLATES)
    while len(rows) < want:
        cat = random.choice(cats)
        s = random.choice(TEMPLATES[cat])()
        if s not in seen:
            seen.add(s)
            rows.append((f"own:{cat}", s))
    with open(os.path.join(HERE, "stress_round2.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# Self-written round-2 sentences (source<TAB>text). Ours; safe to commit.\n")
        f.write("# Regenerate with gen_stress_round2.py.\n")
        for src, s in rows:
            f.write(f"{src}\t{s}\n")
    print(len(rows), "own sentences")


if __name__ == "__main__":
    main()
