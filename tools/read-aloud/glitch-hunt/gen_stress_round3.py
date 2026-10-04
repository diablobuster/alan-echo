"""Writes stress_round3.txt and corpus_round3.tsv: our own round-3 sentences in text
styles the earlier corpora did not cover: legal and court opinions, medical / health
news, academic abstracts with citations and statistics, Reddit / forum style, product
reviews, and code-ish text inside articles. Hand-written lines plus seeded templates.
Ours; safe to commit. (corpus_round3.tsv is git-ignored like the other corpora.)

  python gen_stress_round3.py
"""
import os, random

HERE = os.path.dirname(os.path.abspath(__file__))
random.seed(31)

HAND = {
    "legal": [
        "See Brown v. Board of Education, 347 U.S. 483 (1954).",
        "The Court relied on Marbury v. Madison, 5 U.S. 137, 177 (1803).",
        "Plaintiff filed suit under 42 U.S.C. § 1983 in the U.S. District Court for the Southern District of New York.",
        "Under Fed. R. Civ. P. 12(b)(6), the complaint must state a plausible claim.",
        "The Ninth Circuit reversed. 912 F.3d 1201 (9th Cir. 2019).",
        "We review de novo. Id. at 1205.",
        "Defendants' motion for summary judgment is GRANTED in part and DENIED in part.",
        "The statute, 18 U.S.C. §§ 1341-1343, covers mail and wire fraud.",
        "Petitioner argues that the district court erred. Pet. Br. 14-16.",
        "Cf. Chevron U.S.A. Inc. v. NRDC, 467 U.S. 837 (1984), overruled by Loper Bright Enters. v. Raimondo, 603 U.S. 369 (2024).",
        "The jury returned a verdict of $4.2 million, plus pre-judgment interest at 9% per annum.",
        "J. Thomas, concurring in the judgment.",
        "Roberts, C.J., delivered the opinion of the Court, in which Alito, Gorsuch, and Kavanaugh, JJ., joined.",
        "The case is remanded for further proceedings consistent with this opinion.",
        "Section 2(a)(1) of the Act provides that no person shall be denied the right to vote.",
        "Pursuant to 28 U.S.C. § 1291, we have jurisdiction over this appeal.",
        "The parties stipulated to the facts in ¶¶ 3-17 of the Joint Statement.",
        "Appellant's counsel, Esq., moved for a continuance on Jan. 5, 2024.",
        "The Court of Appeals affirmed, 2d Cir. No. 22-1043, slip op. at 7.",
        "The defendant pleaded nolo contendere to the misdemeanor count.",
        "The order is stayed pending appeal, see Nken v. Holder, 556 U.S. 418, 434 (2009).",
        "Plaintiffs seek treble damages under 15 U.S.C. § 15(a).",
        "The Board's decision was arbitrary and capricious under 5 U.S.C. § 706(2)(A).",
        "The contract's force majeure clause (§ 14.2) excuses performance.",
        "Respondent v. Petitioner, No. 23-719, argued Feb. 8, 2024, decided Mar. 4, 2024.",
        "The ALJ found the claimant not disabled at step five.",
        "We decline to reach the First Amendment question.",
        "The trial court abused its discretion. See id. at 412 n.3.",
        "The settlement fund of $725 million will be distributed pro rata.",
        "Under Cal. Civ. Code § 1542, a general release does not extend to unknown claims.",
    ],
    "medical": [
        "Patients received 500 mg of amoxicillin twice daily for 7 days.",
        "The FDA approved the drug for adults with type 2 diabetes and a BMI of 30 kg/m2 or higher.",
        "Blood pressure fell from 142/91 mmHg to 128/82 mmHg after 12 weeks.",
        "The CDC recommends the updated COVID-19 vaccine for everyone 6 months and older.",
        "LDL cholesterol dropped by 38 mg/dL on average.",
        "HbA1c fell 1.4 percentage points, from 8.2% to 6.8%.",
        "Take 1 tablet by mouth every 4-6 hours as needed; do not exceed 4 g/day.",
        "The study enrolled 2,450 adults aged 50-75 at 31 sites in the U.S. and Canada.",
        "Side effects included nausea (12%), headache (8%), and fatigue (5%).",
        "The patient presented with a temperature of 101.3°F and an O2 sat of 91% on room air.",
        "An MRI showed a 2.3 cm lesion in the left temporal lobe.",
        "RSV hospitalizations rose 40% week over week, the NIH said.",
        "The WHO declared mpox a public health emergency of international concern.",
        "Ozempic (semaglutide) and Zepbound (tirzepatide) are GLP-1 drugs.",
        "The dose was titrated from 0.25 mg to 1 mg weekly.",
        "ICU admissions per 100,000 people fell to 2.1.",
        "Dr. Patel, an MD/PhD at Johns Hopkins, led the trial.",
        "Infants under 12 mo. should not be given honey.",
        "The EKG showed atrial fibrillation with a rate of 132 bpm.",
        "She was started on IV fluids at 125 mL/hr.",
        "Vitamin D levels below 20 ng/mL are considered deficient.",
        "The test has a sensitivity of 94.1% and a specificity of 98.7%.",
        "About 1 in 8 women will develop breast cancer, per the ACS.",
        "Ibuprofen 400-800 mg q6h PRN for pain.",
        "The CBC was normal except for a WBC of 13.4 x 10^9/L.",
        "Measles cases hit 1,274 in 2019, the most since 1992.",
        "The patient's BMI was 27.4 and her eGFR was 58 mL/min/1.73 m2.",
        "The vaccine was 87% effective against severe disease.",
        "Symptoms typically appear 2-14 days after exposure.",
        "Use SPF 30+ sunscreen and reapply every 2 hrs.",
    ],
    "academic": [
        "Prior work (Smith et al., 2021; Lee & Park, 2019) found similar effects.",
        "As Jones et al. (2021) showed, the effect persists across cohorts [12].",
        "The difference was significant (p < 0.05).",
        "We recruited n = 1,204 participants (mean age 34.2, SD = 9.1).",
        "The odds ratio was 2.1 (95% CI 1.2–3.4).",
        "Results held after adjustment (β = 0.32, SE = 0.08, p = .001).",
        "The model explained 41% of the variance (R² = 0.41).",
        "Effect sizes were moderate (Cohen's d = 0.54).",
        "See Table 2 and Fig. 3b for details.",
        "Inter-rater reliability was high (κ = 0.87).",
        "We used a mixed-effects model with random intercepts for site (Bates et al., 2015).",
        "Data and code are available at https://osf.io/abc12/.",
        "The hazard ratio was 0.76 (95% CI, 0.64 to 0.90; P = 0.002).",
        "Participants (N = 312) completed the survey online via MTurk.",
        "This replicates earlier findings [3, 7–9].",
        "The correlation was weak (r = .18, p = .03).",
        "A chi-square test showed no difference, χ²(2) = 1.84, p = .40.",
        "doi:10.1038/s41586-021-03819-2",
        "In a meta-analysis of 47 RCTs (k = 47), the pooled RR was 0.81.",
        "The intervention reduced symptoms by 23% vs. placebo (Wang, 2020, p. 114).",
        "Accuracy improved from 71.3% to 84.9% (Δ = 13.6 pp).",
        "We follow the approach of Vaswani et al. (2017).",
        "The sample was 52% female, 61% White, and 18% Hispanic or Latino.",
        "Results are reported as mean ± SD.",
        "The F-statistic was significant, F(3, 196) = 4.72, p = .003.",
        "All analyses were run in R 4.3.1 and Python 3.11.",
        "Funding: NSF grant DMS-2015392 and NIH R01-MH123456.",
        "Abstract: Background. Obesity affects over 40% of U.S. adults.",
        "Keywords: machine learning; NLP; large language models (LLMs).",
        "The AUC was 0.91 (0.88–0.94) on the held-out test set.",
    ],
    "forum": [
        "lol this is exactly what happened to me last week",
        "imo the first season is way better than the rest tbh",
        "anyone else's app crashing after the update? ios 18.1 here",
        "TIL you can just ask for a lower rate and they'll often say yes",
        "w/ the new patch my fps went from 60 to like 144",
        "b/c the landlord never fixed the heat we just moved out",
        "Mac &amp; cheese is not a side dish, fight me",
        "idk why ppl are mad, the u.s. version is fine",
        "Edit: thanks for the gold, kind stranger!",
        "ngl that ending made me cry",
        "smh people really be parking like that",
        "This. So much this.",
        "IIRC the deadline is the 15th, but double-check.",
        "OP, you're NTA. Your sister is TA here.",
        "fwiw i've had mine for 3 yrs and zero issues",
        "ELI5: why does the moon look bigger near the horizon?",
        "AFAIK it's not legal in the EU yet",
        "My 2c: just buy the cheaper one and save $$$",
        "brb gotta feed the cat",
        "update: it worked!!! thank u all sm",
        "rip my inbox lmao",
        "tl;dr don't buy from them",
        "Source: am a nurse, 10+ yrs in the ER",
        "/s obviously",
        "r/personalfinance would tell you to max your 401k first",
        "u/throwaway123 did you ever get a refund?",
        "can confirm, works on win 11 too",
        "honestly? 10/10 would recommend",
        "this sub has gone downhill since 2020 ngl",
        "hmu if u want the link",
    ],
    "reviews": [
        "5/5 stars. Would buy again!",
        "★★★★☆ Great sound, but the case feels cheap.",
        "Battery lasts about 9 hrs w/ ANC on, 12 hrs off.",
        "Pros: light, fast, quiet. Cons: only 2 USB-C ports.",
        "Returned it after 2 weeks. The left earbud kept disconnecting.",
        "For $49.99 you really can't beat it.",
        "UPDATE (3/12): Customer service replaced it, bumping to 4 stars.",
        "It's a solid 8/10 for the price.",
        "The 15.6\" screen is bright enough outdoors (around 400 nits).",
        "Fits true to size; I'm 5'10\" and 170 lbs and got a medium.",
        "Verified Purchase. Size: XL. Color: Navy.",
        "Charges from 0-100% in about 45 min.",
        "Weighs 2.8 lbs, so it's great for travel.",
        "Works with Alexa and Google Home, but not HomeKit.",
        "The 1TB SSD and 16GB RAM make it snappy.",
        "Don't waste your $$ on this.",
        "Arrived DOA. Zero stars if I could.",
        "Love it!!! My kids use it every day :)",
        "Holds 1.5 qt, which is perfect for two people.",
        "The 4K/60fps video is crisp, but low light is meh.",
        "Rated IPX7, so it survived a dunk in the pool.",
        "Bought on sale for $199 (reg. $279).",
        "Packaging was damaged but the item was fine.",
        "3 stars - does the job, nothing special.",
        "Wi-Fi 6E support is a nice touch.",
        "After 6 mos. of daily use, the hinge is still tight.",
        "The app (v2.3.1) is buggy on Android 14.",
        "Took 10 mins to set up. Easy peasy.",
        "Way louder than my old JBL.",
        "Bottom line: get the Pro if you can afford it.",
    ],
    "code": [
        "Run npm install and then npm run dev to start the server.",
        "Open file.txt in Notepad and save it as config.json.",
        "The app is written in C++ with a C# front end on .NET 8.",
        "Node.js 20 and Vue.js 3 are both supported.",
        "Most teams now deploy to k8s (Kubernetes) on AWS.",
        "Set the PATH variable to C:\\Program Files\\Git\\bin.",
        "Edit ~/.bashrc and add export EDITOR=vim.",
        "Install it with pip install requests==2.31.0.",
        "The bug is in utils.py, line 42.",
        "Call git commit -m \"fix typo\" and push to main.",
        "Use Ctrl+C to copy and Ctrl+V to paste.",
        "The API returns a 404 when the user ID is invalid.",
        "Visit localhost:3000 in your browser.",
        "The README.md explains how to build from source.",
        "It needs Python 3.10+ and at least 8 GB of RAM.",
        "TypeScript 5.4 adds better type narrowing.",
        "The index.html file loads main.js and styles.css.",
        "Rust's cargo build compiles the project in release mode with --release.",
        "Docker Compose v2 replaced docker-compose.",
        "Next.js 14 and React 18 power the dashboard.",
        "Run sudo apt-get update && sudo apt-get upgrade.",
        "The function returns null if the array is empty.",
        "Version 1.2.3-beta.4 fixes the memory leak.",
        "PostgreSQL 16 and MySQL 8.0 are supported; SQLite is used for tests.",
        "The JSON payload has a user_id and an access_token field.",
        "Go 1.22 and Java 21 LTS both ship with new GC options.",
        "Open the .env file and set API_KEY=your-key-here.",
        "The repo uses ESLint, Prettier and Jest.",
        "GPT-4o and Claude 3.5 Sonnet were tested via the API.",
        "The iOS 17 SDK requires Xcode 15.",
    ],
}

CASES = ["Smith", "Jones", "Garcia", "Doe", "United States", "Microsoft Corp.", "Apple Inc.", "Texas", "Roe", "Miller"]
COURTS = ["2d Cir.", "9th Cir.", "5th Cir.", "D.C. Cir.", "Fed. Cir.", "S.D.N.Y.", "N.D. Cal.", "D. Del.", "E.D. Va."]
REPORTERS = ["U.S.", "S. Ct.", "F.3d", "F.4th", "F. Supp. 3d", "L. Ed. 2d", "A.3d", "N.E.3d"]
DRUGS = ["metformin", "lisinopril", "atorvastatin", "amoxicillin", "ibuprofen", "sertraline", "semaglutide", "albuterol"]
UNITS_MED = ["mg", "mcg", "mL", "g", "IU", "mg/kg"]
AUTHORS = ["Smith", "Chen", "Garcia", "Kim", "Nguyen", "Müller", "O'Brien", "Okafor", "Patel", "Rossi"]
SUBS = ["r/AskReddit", "r/buildapc", "r/personalfinance", "r/cooking", "r/legaladvice", "r/nba"]
PRODUCTS = ["AirPods Pro 2", "Galaxy S24 Ultra", "Pixel 8a", "MacBook Air M3", "Kindle Paperwhite", "Instant Pot Duo 7-in-1",
            "Dyson V15", "Sony WH-1000XM5", "Roku Ultra", "iPad mini 7"]
TECH = ["C++", "C#", ".NET", "Node.js", "Vue.js", "k8s", "React", "Next.js", "PyTorch", "TensorFlow", "F#", "Objective-C",
        "ASP.NET", "Express.js", "D3.js", "Three.js"]
FILES = ["file.txt", "config.yaml", "main.py", "index.js", "package.json", "Dockerfile", "app.tsx", "setup.cfg", "data.csv",
         "README.md", "server.go", "lib.rs"]
CMDS = ["npm install", "npm run build", "pip install -r requirements.txt", "git pull", "cargo test", "docker build .",
        "yarn add lodash", "brew install node", "kubectl get pods", "make clean"]


def n(lo, hi):
    return random.randint(lo, hi)


def p():
    return random.choice(["< 0.05", "< .001", "= .03", "= 0.012", "< 0.01", "= .21", "> .05"])


TEMPLATES = {
    "legal": [
        lambda: f"See {random.choice(CASES)} v. {random.choice(CASES)}, {n(100, 600)} {random.choice(REPORTERS)} {n(1, 999)}, {n(1, 999)} ({random.choice(COURTS)} {n(1990, 2024)}).",
        lambda: f"Id. at {n(10, 900)}.",
        lambda: f"Under {n(5, 50)} U.S.C. § {n(100, 9999)}({random.choice('abcde')}), the agency must act within {n(30, 180)} days.",
        lambda: f"Fed. R. Evid. {n(401, 806)} bars the testimony.",
        lambda: f"The court granted the motion on {random.choice(['Jan.', 'Feb.', 'Mar.', 'Apr.', 'Aug.', 'Sept.', 'Oct.', 'Nov.', 'Dec.'])} {n(1, 28)}, {n(2015, 2024)}.",
        lambda: f"{random.choice(CASES)}, No. {n(19, 24)}-cv-{n(100, 9999):04d} (D. {random.choice(['Mass.', 'Md.', 'Minn.', 'Colo.'])} {n(2018, 2024)}).",
        lambda: f"Section {n(1, 30)}({random.choice('abc')})({n(1, 4)}) does not apply here.",
        lambda: f"The parties dispute ¶ {n(1, 60)} of the complaint.",
        lambda: f"{random.choice(['Kagan', 'Sotomayor', 'Barrett', 'Jackson'])}, J., filed a dissenting opinion.",
        lambda: f"The jury awarded ${n(1, 99)}.{n(1, 9)} million in compensatory damages.",
    ],
    "medical": [
        lambda: f"Patients took {n(1, 1000)} {random.choice(UNITS_MED)} of {random.choice(DRUGS)} {random.choice(['once daily', 'twice daily', 'every 8 hours', 'q12h', 'BID', 'TID'])}.",
        lambda: f"Blood pressure was {n(110, 170)}/{n(65, 105)} mmHg.",
        lambda: f"Her A1C was {n(5, 11)}.{n(0, 9)}% and her LDL was {n(60, 220)} mg/dL.",
        lambda: f"The trial enrolled {n(100, 9999):,} patients aged {n(18, 50)}-{n(51, 85)}.",
        lambda: f"Cases rose {n(5, 90)}% in {random.choice(['the U.S.', 'the U.K.', 'NYC', 'L.A. County'])}, the {random.choice(['CDC', 'WHO', 'NIH', 'FDA'])} said.",
        lambda: f"The {random.choice(['MRI', 'CT scan', 'X-ray', 'ultrasound'])} showed a {n(1, 9)}.{n(0, 9)} cm mass.",
        lambda: f"Heart rate was {n(50, 150)} bpm and temp {n(97, 104)}.{n(0, 9)}°F.",
        lambda: f"About {n(1, 9)} in {n(10, 100)} adults have the condition.",
        lambda: f"Give {n(1, 20)} mg/kg IV every {n(4, 12)} hrs.",
        lambda: f"The vaccine was {n(50, 97)}% effective (95% CI {n(30, 49)}-{n(98, 99)}%).",
    ],
    "academic": [
        lambda: f"As {random.choice(AUTHORS)} et al. ({n(1998, 2024)}) found, the effect replicates [{n(1, 60)}].",
        lambda: f"({random.choice(AUTHORS)} & {random.choice(AUTHORS)}, {n(1990, 2024)}; {random.choice(AUTHORS)}, {n(1990, 2024)})",
        lambda: f"The difference was significant (p {p()}).",
        lambda: f"We surveyed n = {n(100, 9999):,} respondents.",
        lambda: f"OR = {n(1, 4)}.{n(0, 9)} (95% CI {n(1, 2)}.{n(0, 9)}–{n(3, 6)}.{n(0, 9)}).",
        lambda: f"β = {random.choice(['0.', '-0.'])}{n(10, 99)}, SE = 0.{n(1, 30):02d}, p {p()}.",
        lambda: f"The scale was reliable (α = .{n(70, 95)}).",
        lambda: f"See Fig. {n(1, 9)}{random.choice(['', 'a', 'b', 'c'])} and Table {n(1, 6)}.",
        lambda: f"Mean age was {n(20, 70)}.{n(0, 9)} years (SD = {n(3, 15)}.{n(0, 9)}).",
        lambda: f"t({n(20, 400)}) = {n(1, 6)}.{n(10, 99)}, p {p()}, d = 0.{n(10, 99)}.",
        lambda: f"Results are shown in refs. [{n(1, 20)}–{n(21, 40)}].",
    ],
    "forum": [
        lambda: f"{random.choice(['lol', 'lmao', 'ngl', 'tbh', 'imo', 'honestly'])} {random.choice(['same', 'this is so true', 'i had no idea', 'that is wild', 'big mood'])}",
        lambda: f"{random.choice(['w/', 'w/o', 'b/c', 'bc'])} {random.choice(['the update', 'my ex', 'a 401k', 'the u.s. version', 'ads'])} it's {random.choice(['fine', 'worse', 'way better', 'pointless'])}",
        lambda: f"posted this in {random.choice(SUBS)} too but no answers yet",
        lambda: f"{random.choice(['Mac', 'Salt', 'Rock', 'Black', 'R'])} &amp; {random.choice(['cheese', 'pepper', 'roll', 'white', 'B'])} {random.choice(['forever', 'is underrated', 'ftw'])}",
        lambda: f"edit: {random.choice(['typo', 'formatting', 'words', 'thanks everyone!!', 'nvm figured it out'])}",
        lambda: f"{random.choice(['AITA', 'WIBTA', 'TIFU', 'PSA', 'ELI5', 'LPT'])}: {random.choice(['i told my roommate no', 'the bus pass trick', 'how do vaccines work', 'check your tire pressure'])}",
        lambda: f"been using it for {n(2, 11)} {random.choice(['yrs', 'mos', 'wks'])} and {random.choice(['no complaints', 'its meh', 'love it', 'zero regrets'])}",
        lambda: f"{random.choice(['afaik', 'iirc', 'fwiw', 'idk'])} {random.choice(['it ships in may', 'the fee is $5', 'u can cancel anytime', 'the u.s. gets it first'])}",
    ],
    "reviews": [
        lambda: f"{random.choice(PRODUCTS)}: {n(1, 5)}/5. {random.choice(['Worth it.', 'Meh.', 'Love it!', 'Overpriced.', 'Solid buy.'])}",
        lambda: f"Battery lasts {n(4, 30)} hrs {random.choice(['w/', 'with'])} {random.choice(['ANC', 'Wi-Fi', 'GPS', 'the screen'])} on.",
        lambda: f"Got it for ${n(19, 999)}.{random.choice(['99', '00', '49'])} on sale.",
        lambda: f"The {n(13, 17)}.{n(1, 9)}\" display is {random.choice(['bright', 'dim', 'sharp', 'washed out'])}.",
        lambda: f"{random.choice(['★★★★★', '★★★★☆', '★★★☆☆', '★★☆☆☆', '★☆☆☆☆'])} {random.choice(['Great', 'Okay', 'Not bad', 'Disappointing'])}.",
        lambda: f"{n(8, 64)}GB RAM, {random.choice(['256GB', '512GB', '1TB', '2TB'])} SSD, {random.choice(['Wi-Fi 6E', 'Wi-Fi 7', 'BT 5.3'])}.",
        lambda: f"I'm {n(5, 6)}'{n(1, 11)}\" and {n(120, 230)} lbs; the {random.choice(['M', 'L', 'XL', 'S'])} fits well.",
        lambda: f"UPDATE ({n(1, 12)}/{n(1, 28)}): {random.choice(['still going strong', 'it died', 'support sent a new one'])}.",
    ],
    "code": [
        lambda: f"Run {random.choice(CMDS)} in the project folder.",
        lambda: f"Open {random.choice(FILES)} and change line {n(1, 300)}.",
        lambda: f"The backend uses {random.choice(TECH)} and the frontend uses {random.choice(TECH)}.",
        lambda: f"{random.choice(TECH)} {n(1, 22)}.{n(0, 9)} {random.choice(['adds', 'drops', 'deprecates', 'fixes'])} {random.choice(['async support', 'the old API', 'a memory leak', 'Windows on ARM'])}.",
        lambda: f"Set {random.choice(['NODE_ENV', 'DEBUG', 'API_URL', 'PORT'])}={random.choice(['production', '1', 'http://localhost:8080', '3000'])} in .env.",
        lambda: f"The {random.choice(['v1', 'v2', 'v3'])} endpoint {random.choice(['/api/users', '/v1/orders', '/auth/login'])} returns HTTP {random.choice(['200', '201', '401', '404', '500'])}.",
        lambda: f"Press {random.choice(['Ctrl+S', 'Cmd+K', 'Alt+F4', 'Ctrl+Shift+P', 'F5'])} to {random.choice(['save', 'search', 'quit', 'open the palette', 'run'])}.",
        lambda: f"It runs on {random.choice(['k8s', 'EC2', 'Lambda', 'GKE', 'Azure'])} with {n(2, 64)} vCPUs and {n(4, 256)} GB RAM.",
    ],
}


def main():
    rows = []
    for cat, lines in HAND.items():
        rows += [(f"own:{cat}", l.replace("\n", " ")) for l in lines]
    seen = {r[1] for r in rows}
    want = 1500
    cats = list(TEMPLATES)
    tries = 0
    while len(rows) < want and tries < 200000:
        tries += 1
        cat = random.choice(cats)
        s = random.choice(TEMPLATES[cat])()
        if s not in seen:
            seen.add(s)
            rows.append((f"own:{cat}", s))
    with open(os.path.join(HERE, "stress_round3.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# Self-written round-3 sentences (source<TAB>text). Ours; safe to commit.\n")
        f.write("# Regenerate with gen_stress_round3.py.\n")
        for src, s in rows:
            f.write(f"{src}\t{s}\n")
    with open(os.path.join(HERE, "corpus_round3.tsv"), "w", encoding="utf-8", newline="\n") as f:
        for i, (src, s) in enumerate(rows):
            f.write(f"{i}\t{src}\t{s}\n")
    words = sum(len(s.split()) for _, s in rows)
    print(len(rows), "own sentences,", words, "words")


if __name__ == "__main__":
    main()
