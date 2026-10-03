"""Read Aloud brand review sheet.

Turns the glitch hunt's GRU-routed word list plus a curated tech / finance / news
brand list into tools/read-aloud/brand_review.tsv: word, corpus count, the reading
through the real Read Aloud path (hand list -> acronym / CamelCase / compound rules
-> misaki -> NameFallback), and what the GRU alone says. A reviewer then hand-writes
misaki phonemes into src-tauri/data/names_fix.tsv for every brand both get wrong.

  python tools/read-aloud/brand_candidates.py --static <glitch-hunt static.jsonl>

The static.jsonl is the glitch hunt's static-pass output (token rows with the
fallback branch that answered, "gru" for the letter-to-sound model). Use one from a
run BEFORE the CamelCase rules, so the list holds every word that used to reach the
GRU. Without --static only the curated list is reviewed. Local tool; Python 3, no
third-party packages. Runs `cargo test --release brand_review_dump -- --ignored`.
"""
import argparse
import json
import os
import re
import subprocess
import sys
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
SRC_TAURI = os.path.normpath(os.path.join(HERE, "..", "..", "src-tauri"))

# Tech / finance / news brands people meet in articles (about 200). Kept in the
# spelling the brands use themselves.
CURATED = """
Microsoft Apple Google Alphabet Amazon Meta Facebook Instagram WhatsApp Netflix Nvidia
Tesla SpaceX Starlink OpenAI ChatGPT Anthropic DeepMind DeepSeek Mistral Perplexity
xAI Grok Copilot LinkedIn PayPal Venmo Zelle Stripe Square Shopify Salesforce Oracle
IBM Intel AMD Qualcomm Broadcom TSMC Samsung Huawei Xiaomi Lenovo ByteDance TikTok
Tencent Alibaba Baidu WeChat Weibo Pinduoduo Temu Shein JD.com Rakuten SoftBank
Snapchat Pinterest Reddit Discord Telegram Signal Zoom Slack Dropbox Atlassian GitHub
GitLab Bitbucket Docker Kubernetes Vercel Netlify Cloudflare Akamai Fastly Snowflake
Databricks Palantir CrowdStrike SentinelOne Okta Twilio HubSpot Zendesk ServiceNow
Workday Adobe Autodesk Figma Canva Notion Airtable Asana Trello Miro Uber Lyft Airbnb
DoorDash Instacart Grubhub Postmates Robinhood Coinbase Binance Kraken Bitcoin Ethereum
Solana Dogecoin Tether Ripple BlackRock Vanguard Fidelity Schwab JPMorgan Citigroup
Citibank Goldman Sachs MorganStanley Barclays HSBC UBS Nomura Berkshire Hathaway
Moody's Nasdaq NYSE Bloomberg Reuters Axios Politico Vox BuzzFeed HuffPost TechCrunch
TechReview Wired Engadget Gizmodo Mashable ZDNet CNET Verge Forbes Fortune Barron's
MarketWatch CNBC MSNBC Fox CNN BBC NPR PBS ProPublica Substack Medium Patreon OnlyFans
YouTube Twitch Spotify SoundCloud Pandora iHeartMedia SiriusXM Roku Hulu Disney Paramount
Peacock Warner Comcast Verizon AT&T T-Mobile Sprint Vodafone Ericsson Nokia Motorola
Qualtrics Intuit TurboTax QuickBooks Equifax Experian TransUnion Mastercard Visa
AmEx Discover Klarna Affirm Afterpay Revolut Monzo Chime SoFi Wealthfront Betterment
NerdWallet Zillow Redfin Opendoor Compass Peloton Fitbit Garmin GoPro Sonos Bose
Logitech Razer Corsair AlienWare Asus Acer Dell HP Toshiba Sony Nintendo PlayStation
Xbox Steam Valve Epic Roblox Unity Unreal Activision Blizzard Ubisoft EA Rockstar
Moderna Pfizer BioNTech AstraZeneca Novavax Regeneron Gilead AbbVie Merck Novartis
Roche Sanofi GSK Bayer Medtronic UnitedHealth Humana Cigna Aetna Anthem Walgreens CVS
Walmart Costco Target Kroger Aldi Lidl IKEA Wayfair Etsy eBay Chewy Zalando ASOS
ExxonMobil Chevron Shell BP TotalEnergies Aramco Gazprom Rosneft Petrobras Equinor
Boeing Airbus Lockheed Raytheon Northrop Embraer Rivian Lucid Polestar NIO BYD Hyundai
Kia Toyota Honda Nissan Volkswagen Porsche Ferrari Stellantis Renault Peugeot Volvo
iPhone iPad iMac MacBook AirPods iCloud macOS iOS watchOS Android Pixel Galaxy Gemini
Claude Llama Midjourney DALL-E Sora Runway HuggingFace Nvidia's PostgreSQL MySQL MongoDB
Redis SQLite JavaScript TypeScript Python Rust Golang PyTorch TensorFlow NumPy pandas
"""


def curated():
    return [w for w in CURATED.split() if w]


def gru_words(static_path):
    """Tokens the static pass sent to the GRU, with how often."""
    c = Counter()
    with open(static_path, encoding="utf-8") as f:
        for line in f:
            r = json.loads(line)
            for tok in r.get("toks", []):
                text, _ph, _owner, via = tok[0], tok[1], tok[2], tok[3]
                if via == "gru":
                    c[text] += 1
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--static", help="glitch-hunt static.jsonl (a run before the CamelCase rules)")
    ap.add_argument("--top", type=int, default=300, help="GRU words to review, by corpus frequency")
    ap.add_argument("--out", default=os.path.join(HERE, "brand_review.tsv"))
    a = ap.parse_args()

    counts = gru_words(a.static) if a.static else Counter()
    rows = [(w, n) for w, n in counts.most_common(a.top)]
    seen = {w for w, _ in rows}
    for w in curated():
        if w not in seen:
            rows.append((w, counts.get(w, 0)))
            seen.add(w)
    # a word with a character the review TSV or a shell would mangle is skipped
    rows = [(w, n) for w, n in rows if re.fullmatch(r"[^\s\t]+", w)]

    tmp = os.path.join(HERE, ".brand_in.tsv")
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        for w, n in rows:
            f.write(f"{w}\t{n}\n")
    env = dict(os.environ, BRAND_IN=tmp, BRAND_OUT=os.path.abspath(a.out))
    cmd = ["cargo", "test", "--release", "brand_review_dump", "--", "--ignored"]
    r = subprocess.run(cmd, cwd=SRC_TAURI, env=env)
    os.remove(tmp)
    if r.returncode != 0:
        sys.exit(r.returncode)
    print(f"{len(rows)} words -> {a.out}")


if __name__ == "__main__":
    main()
