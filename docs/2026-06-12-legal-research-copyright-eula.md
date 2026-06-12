# ALAN Echo — Legal Research: US Software Copyright Registration & EULA Law

**Date:** 2026-06-12
**Prepared by:** Claude (deep-research workflow + model synthesis)
**Status:** Research deliverable — **legal information, not legal advice.** No attorney-client relationship is created by this document. Where a decision could cost real money, the document says so and recommends counsel.

---

## How to read this document

Two deep-research workflow runs (107 + 108 agents) decomposed each report into 5 search angles, fetched **34 authoritative sources** (23 for Report 1, 11 for Report 2 — 16 additional fetches failed on a session rate limit), and extracted **168 falsifiable claims**. The adversarial verification phase (3 independent verifiers per claim) was **aborted by the same rate limit** — every verifier abstained (0-0 votes), and the workflow mislabeled those abstentions as "refuted." **Nothing was actually refuted.** Treat the claims as single-sourced from the cited document, not cross-verified.

Every load-bearing claim below is tagged:

- **[S]** — extracted this session from the cited source document (mostly primary: copyright.gov, Federal Register, court opinions, GitHub policy). High confidence in what the source says; not independently cross-checked.
- **[K]** — from model legal knowledge (training cutoff January 2026). Settled doctrine is reliable; dollar figures and anything that changes administratively should be verified before you rely on it.
- **[K-approx]** — a ballpark from training; verify before budgeting around it.

Time-sensitive facts (fees, processing times, pending rulemaking) were captured **as of June 2026** by the workflow's fetches and are the most current data in this document.

---
---

# REPORT 1 — US Software Copyright Registration

## 1.1 TL;DR — What you should actually do, in order

1. **Register ALAN Echo now, this month.** Two clocks are running against you. (a) If your first public release was within the last 3 months, registering inside the 3-month window preserves statutory damages and attorney's fees retroactively to the date of first publication (17 U.S.C. § 412(2)) [S]. (b) The Copyright Office's **March 20, 2026 NPRM** proposes raising the Standard Application fee from $65 to $85, raising Special Handling from $800 to $1,100, and **eliminating the $45 Single Application entirely** (≈43% average increase across the schedule). It's not final yet — comments closed May 4, 2026, and under 17 U.S.C. § 708 the new schedule needs to sit before Congress for 120 days — so as of today the 2020 schedule ($65) still governs [S]. Register before that flips.
2. **Use the Standard Application ($65), not the Single Application ($45).** Your app bundles whisper.cpp and MIT/Apache crates — code by other authors. That disqualifies the Single Application (Compendium § 1405.3; 37 C.F.R. § 202.3(b)(2)(i)(B)(1)), and a refused Single Application must be **refiled with a new fee and a reset effective date** — which could cost you the § 412 window [S].
3. **Deposit the first 25 + last 25 pages of source code** (Compendium § 1509.1(F)(3)) [S]. Practical note: for a multi-file program there is no canonical "page 1" — applicants assemble the printout themselves, so you can order files such that license-verification and trial-state code (your real secret sauce: `activation.rs`, `trial.rs`) isn't in the deposited pages, avoiding redaction and the Rule of Doubt entirely [K — practitioner convention; confirm against current Circular 61 before filing].
4. **Exclude third-party material via Limitation of Claim.** In the application: Author Created = "computer program"; Material Excluded = "previously published and third-party computer code (open-source components)"; New Material = "computer program." The descriptions in New Material and Author Created must match exactly — mismatches are a top DIY error that triggers examiner correspondence (adds ~1.4 months) or a defective registration [S].
5. **Deal with the AI-generated-code question before filing.** Under the Office's March 2023 registration guidance, reaffirmed in the January 2025 Copyrightability Report, applicants must disclose AI-generated material that is **more than de minimis**; the registration then covers only the human-authored contribution. Purely AI-generated code is not copyrightable; AI-**assisted** work (you directing, selecting, arranging, editing) does not lose protection [S]. ALAN Echo was built with heavy AI assistance — **this is the single most important question to put to a copyright attorney before you file**, because the right characterization of your workflow determines what you can claim. Do not guess on a federal application you sign under penalty of accuracy (17 U.S.C. § 506(e) makes knowing false statements on an application a criminal offense [K]).
6. **Put a copyright notice everywhere now (free, takes an hour):** `© 2026 [your legal name]. All rights reserved.` in the app About dialog, installer/EULA screen, website footer, GitHub releases README, and source headers. Under 17 U.S.C. § 401(d), notice on published copies kills the innocent-infringer mitigation that can shrink statutory damages to $200 [S].
7. **Don't register the binary.** A source-code registration automatically covers the executable form and the UI/screen displays (Compendium § 1509.1(F)(6)) [S]. Depositing object code instead triggers the **Rule of Doubt** — the Office registers without examining and annotates the certificate, weakening the § 410(c) presumption of validity [S].
8. **Set a version cadence.** Each new version with new copyrightable code is a separate derivative work; a new registration covers only the new material [S]. Registering every patch release is pointless; register **v1.0 now, then each major version or annually**, whichever comes first [K — standard practitioner cadence].
9. **Know your enforcement venue in advance:** GitHub DMCA takedown (free) → Copyright Claims Board ($100 filing, $30k cap, designed for self-representation) → federal court (last resort). Details in § 1.4 below.
10. **Budget:** $65 registration + ~1–2 hours of your time, DIY. Add $500–$1,500 [K-approx] if you want an attorney to handle the AI-disclosure question and the filing (recommended once, for v1.0 — then you can copy the pattern for later versions yourself).

## 1.2 Basics (Questions 1–4)

### Q1. What copyright protects for software — and what it doesn't

Copyright protects the **expression** in your code — the literal source text (Rust, TypeScript), and to a limited extent non-literal structure, plus the screen displays it generates — but categorically **not** ideas, procedures, processes, systems, or methods of operation, "regardless of the form in which [they are] described" (17 U.S.C. § 102(b)) [S]. The House Report on the 1976 Act says this expressly for computer programs: only the programmer's expression is protected, never the underlying process [S]. Computer programs register as **literary works** under § 102(a)(1) [S].

What this means concretely for ALAN Echo [K]:

| Protected by your copyright | NOT protected by copyright |
|---|---|
| Your literal Rust/TS source and comments | The idea of "local voice-to-text with a hotkey" |
| The compiled binary (as a copy of the code) | Your activation **algorithm** (Ed25519-verify-a-JWT is a method) |
| Your UI screens/layouts as expressed | Functional UI behavior, keyboard shortcuts |
| Your installer scripts' expression | File formats, APIs, protocol shapes |
| Your website/docs text | Anything dictated by efficiency or the platform (merger doctrine / scènes à faire) [K] |

The complements [K]: **patents** protect functional inventions (expensive: $10k–$25k+ to prosecute; rarely sensible for an indie app). **Trade secrets** protect what you keep secret (your private signing key is a trade secret; source code in a private repo is too) — protection lasts as long as secrecy does, no registration exists. **Trademarks** protect the brand ("ALAN Echo" as a name/logo — a separate USPTO registration, ~$250–$350/class [K], worth doing once revenue justifies it). These four regimes stack; registration of copyright affects only the copyright layer.

### Q2. Automatic copyright vs. what registration adds

Copyright exists **automatically from fixation** — the moment code is saved, it's protected; registration is voluntary (17 U.S.C. §§ 102(a), 408(a)) [S]. Registration adds four things you cannot get otherwise:

1. **The right to sue at all.** For US works, you cannot file an infringement suit until the Office has **acted** on your application — registered it or refused it (§ 411(a), as held in *Fourth Estate Pub. Benefit Corp. v. Wall-Street.com*, 139 S. Ct. 881 (2019)) [S]. Merely applying is not enough; you wait the full processing time (or pay $800 Special Handling). Even a refusal preserves your right to sue if you then serve notice on the Register (§ 411(a), second sentence) [S].
2. **Statutory damages and attorney's fees** — only for infringement that began after registration (or within the § 412(2) grace window; see Q4) [S].
3. **Prima facie validity**: a certificate issued before or within 5 years of first publication is presumptive evidence of validity and of the facts stated (§ 410(c)) [S].
4. **A public record** of your claim, with the deposit as dated evidence of what you wrote [K].

### Q3. Registering before vs. after infringement — the dollar difference

If your registration is **timely** (before the infringement began, or within the grace period), you may elect **statutory damages: $750–$30,000 per work**, raised to **up to $150,000 per work for willful** infringement, plus discretionary **attorney's fees** under § 505 (*Fogerty v. Fantasy*, 510 U.S. 517 (1994) factors: frivolousness, motivation, objective unreasonableness, deterrence) [S]. The current figures date to the 1999 Digital Theft Deterrence Act and remain current in 2026 [S].

If your registration is **late**, you are limited to **actual damages plus the infringer's profits** (§ 504(b)) — and for an $89 app, actual damages are usually small and hard to prove, which often makes a lawsuit economically irrational. One pro-plaintiff feature survives: you need only prove the infringer's **gross revenue**; the burden shifts to them to prove deductible expenses [S].

The fine print that matters most for you: statutory damages are **per work, not per copy**. ALAN Echo is one work; a pirate who distributes 10,000 copies yields **one** statutory award of $750–$150,000 in a single action — not 10,000 × $750 (§ 504(c)(1): "all infringements of any one work... a single award") [S]. Illustrative outcomes [K]: courts in software-counterfeiting cases routinely award mid-five to seven figures per work against commercial pirates (e.g., Microsoft's reseller cases produced willful-tier awards in the $100k–$1M+ range); two-party disputes over copied code more typically resolve in settlements after the statutory-damages leverage becomes clear. The leverage — "register timely, threaten the $150k ceiling plus fees" — is the entire practical point of registration for a small developer.

### Q4. The 3-month grace period (§ 412) — exact mechanics

§ 412 bars statutory damages and attorney's fees for: (1) infringement of an **unpublished** work commencing before registration — no grace period at all; and (2) infringement of a **published** work commencing after first publication and before registration **unless registration is made within 3 months after first publication** [S]. Register inside that window and the remedies reach back to cover infringement that began on day one after publication [S].

Two traps:

- **"Commenced" means the first act.** Courts hold that if infringement *began* before your (untimely) registration and continues after it, statutory damages remain barred for the whole continuing course of conduct — the post-registration continuation doesn't reset eligibility (*Derek Andrew, Inc. v. Poof Apparel Corp.*, 528 F.3d 696 (9th Cir. 2008)) [K]. So a late registration does **not** let you collect statutory damages from an infringer who started before it.
- **Effective date = receipt, not certificate.** Your registration is effective the day the Office receives an acceptable application + deposit + fee — the months of processing don't push your date back [S].

**Publication status for ALAN Echo** [K — verify with counsel]: distributing installers to the public via GitHub Releases is "publication" under § 101 (distribution of copies to the public). Your 3-month clock almost certainly started at your first public release. If that was less than 3 months ago, registering now is **maximally valuable**; if more, register anyway — it protects you against everyone who starts infringing after the effective date.

## 1.3 Process (Questions 5–9)

### Q5. Step-by-step: registering through the Copyright Office's electronic system

System status as of June 2026: the Office is mid-migration from the legacy **eCO** system to the new **Enterprise Copyright System (ECS)**. The ECS registration application had been deployed as a limited pilot covering eDeposit uploads and the **Standard Application** (the type you need), with eCO still operational otherwise [S]. Practically: go to copyright.gov → Registration → log in / create an account, and use whichever system the site routes you into for a Standard Application; the fields below exist in both [S/K].

1. **Account**: create one at copyright.gov (free).
2. **Start a Standard Application** ($65) [S]. Type of Work: **Literary Work** (computer programs register here) [S].
3. **Title**: "ALAN Echo" (add version, e.g., "ALAN Echo v1.0").
4. **Publication**: Yes; enter date of first publication (your first public GitHub release) and nation (US) [K].
5. **Author**: you, individually (citizenship, year of birth optional-ish; pseudonym possible). **Author Created**: check/enter **"computer program"** — this phrase automatically sweeps in the screen displays/UI [S]. If you operate through an LLC that owns the IP, the LLC is claimant via written transfer — and note an entity-owned work can never use the Single Application (Compendium §§ 1405.5–1405.6) [S].
6. **Claimant**: you (same as author), or your entity with a transfer statement.
7. **Limitation of Claim** (the critical screen for you): Material Excluded — "previously published and third-party owned computer code, including open-source components"; New Material Included — "computer program." The New Material description **must be identical** to the Author Created description [S]. Derivative works "almost always" require this per the Office's own guidance [S].
8. **Note to Copyright Office** field: if your printed source is ≤50 pages total, say you're depositing the entire program [S]; if you used any trade-secret deposit option, state it here [S].
9. **Deposit**: upload a PDF of the first 25 + last 25 pages of source (see Q6), including the page bearing the copyright notice [S].
10. **Certify, pay $65, submit.** Save the case number; your effective date is today if the package is acceptable [S].

**AI disclosure (do not skip):** if more-than-de-minimis portions of the code were AI-generated, current Office guidance requires disclosing it and limiting the claim to your human authorship [S]. The line between "AI-assisted" (no disclosure needed, full protection) and "AI-generated" (disclose, excluded from claim) is the unsettled question for a Claude-built app — see § 1.6 and get one hour of counsel on it.

### Q6. The deposit: 25-and-25, trade secrets, Rule of Doubt, special relief

Default rule (no trade secrets): **first 25 + last 25 pages of source code** for the version being registered, plus the page with the copyright notice; ≤50 pages total → deposit it all and say so (Compendium § 1509.1(F)(3), citing 37 C.F.R. § 202.20(c)(2)(vii)(A)(1)) [S].

If the source **contains trade secrets**, Compendium § 1509.1(F)(4)(b) gives exactly four options [S]:

| Option | What you deposit | Tradeoff |
|---|---|---|
| 1 | First 10 + last 10 pages of source, **nothing blocked out** | Smallest unredacted disclosure; full examination |
| 2 | First 25 + last 25 pages with trade-secret portions **blocked out**, provided redactions are proportionately less than what remains | Most common; keeps secrets; full examination if redactions are proportionate |
| 3 | First 25 + last 25 pages of **object code** + 10 or more consecutive pages of unredacted source | Largest secrecy; triggers Rule-of-Doubt-adjacent scrutiny on the object-code portion |
| 4 | Entire program (<50 pages) with trade-secret portions blocked out | For small programs |

**Rule of Doubt**: if you deposit object code (binary), the Office cannot examine it for copyrightable authorship. You must assert in writing that it contains copyrightable authorship; the Office registers under the Rule of Doubt, **annotates the certificate** ("Registration made under Rule of Doubt"), and that certificate "may not be entitled to a legal presumption" of validity — i.e., you lose much of the § 410(c) benefit you registered for [S]. Avoid: deposit source.

**Special relief** (37 C.F.R. § 202.20(d)): a written request to the Office to accept a non-conforming deposit when none of the four options work for you [K]. For an app your size, you'll never need it — option 2, or my recommendation in the TL;DR (assemble the page order so secret files aren't in the 50 deposited pages at all — no redaction, no annotations), covers you [K].

One more consideration specific to you: deposited material is examined by Office staff and the deposit can be inspected in limited circumstances (and produced in litigation), but it is not published [K]. Your real secrets (the Ed25519 **private key**, Stripe webhooks) live outside the source anyway; the activation *code* being seen is a modest exposure, and you can keep it out of the deposit pages entirely.

### Q7. Processing times and total costs (current as of June 2026)

- Average across all claims: **4.1 months** (cases closed Oct 1, 2025 – Mar 31, 2026) [S] — temporarily inflated by the Oct 1 – Nov 12, 2025 government shutdown, which halted processing while applications kept arriving; the Office expects the average to fall as the backlog clears [S].
- Fully electronic filing (your route; ~90% of applications): **3.6 months average without examiner correspondence** (range 2–5.3), **5.0 months with correspondence** (range 1.6–8.3). 27% of claims get correspondence; you must respond within 45 days [S].
- Paper filing: 6.3–8.1 months average and a $125 fee — don't [S].
- **Special Handling**: $800 (proposed to rise to $1,100 [S]), targets examination **within 5 working days**, granted only for pending/prospective litigation, customs matters, or contract/publishing deadlines; non-refundable even if refused or late [S]. You only ever buy this if you discover an infringer and need to sue *now* — and note the cheaper path: a pending application lets you file at the **CCB** without Special Handling, and CCB-related expedited registration costs only **$50** with a 10-business-day target [S].
- Realistic total for you, DIY: **$65 and a quiet afternoon.** With counsel: $565–$1,565 [K-approx].

### Q8. Binary, source, or both? And the UI?

Register the **source code once**. A computer-program registration covers the work — which includes the executable as a copy of that code [K — settled view, consistent with the Office's treatment of object code as the same work in unexamined form [S]] — and **automatically covers the copyrightable screen displays the program generates**, even if you never mention them and deposit no screenshots (Compendium § 1509.1(F)(6)) [S]. The converse is not true: an application claiming only "screen displays" does **not** cover the code [S]. Do not file a separate visual-arts claim for the UI; one literary-work registration with "computer program" in Author Created is the single-registration doctrine working as designed [S].

### Q9. Updates and new versions

The Office expressly classifies a new version of an existing program as a **derivative work** [S]. Each version with new copyrightable authorship is a separate work needing its own application, fee, and deposit; the new registration covers **only the new/revised material** — earlier registered code stays protected by the earlier registration, and third-party/public-domain code is never covered [S]. Deposit for a revised program: first 25 + last 25 pages if new material appears throughout; otherwise any 50 pages containing the new material (Compendium § 1509.1(F)(2)) [S]. List the prior registration number/year (up to the two most recent) in the application [S].

**Practical cadence for a solo dev** [K]: register v1.0 now; thereafter register (a) any version that ships a major new subsystem, or (b) annually, whichever comes first. The marginal $65 buys statutory-damages eligibility for the *new* code; your core codebase stays protected by the v1.0 registration indefinitely (life + 70 years, § 302(a) [K]).

## 1.4 Scope (Questions 10–12)

### Q10. One registration for backend + frontend + scripts + installer?

**One registration.** The Rust backend, React frontend, build scripts, and installer code that you ship together as ALAN Echo are one "computer program" / one work for registration purposes — claim "computer program" and deposit per Q6 [S/K]. The Office's **unit of publication** option (Compendium § 1402.3, § 1103.4; 37 C.F.R. § 202.3(b)(2)) exists for the edge case where genuinely *separate works* are first published physically bundled together — a single Standard Application can cover them if the same person owns all of them [S]. You don't need to invoke it: an integrated app is most naturally a single work, not a bundle of works [K]. (If you someday ship a separable user manual or a standalone model-manager tool, unit-of-publication or separate registrations become relevant.)

### Q11. Bundled MIT/Apache open-source code

- **It does not jeopardize your registration** — it just must be excluded. Bundled third-party code is "unclaimable material"; you disclaim it via Limitation of Claim (Q5 step 7), and your registration covers your original code sitting alongside it [S].
- **It does disqualify the cheap application**: third-party code in the deposit pushes you from the $45 Single Application to the $65 Standard Application (Compendium § 1405.3) [S]. (Moot soon anyway — the 2026 NPRM proposes eliminating the Single Application [S].)
- **License compliance protects your copyright**: protection does not extend to any part of a work in which preexisting material was used **unlawfully** (§ 103(a)) [S — via Circular 14]. Complying with MIT/Apache attribution terms (see Report 2, Q11) is therefore not just open-source etiquette; it removes an attack on your own derivative-work protection [S].
- You do **not** list every crate in the application; a general exclusion ("third-party and open-source computer code") is standard practice [K].

### Q12. The whisper.cpp model — code vs. weights

Two different objects:

- **whisper.cpp inference code** (MIT, by Georgi Gerganov et al.): ordinary third-party code — exclude from claim (Q11), preserve its license text in your distribution [S/K].
- **Whisper model weights** (released by OpenAI under MIT): whether trained weights are copyrightable **at all** is unsettled. The Copyright Office's January 2025 Copyrightability Report addresses AI *outputs*, not weights; it records that commenters proposed sui generis protection for weights and that the Office declined to recommend any, leaving the question open [S]. The doctrinal headwinds: purely machine-generated artifacts lack human authorship [S]; the counter-arguments (human curation of training choices) remain untested in court [S — marble.onl analysis; blog-quality source]. Practical consequence for you: **nothing to do beyond carrying the MIT notice.** Either the weights are copyrightable and you have a valid MIT license, or they aren't and no license is needed; both branches end with "ship the notice file" [S/K]. Your own registration neither covers nor needs to mention the weights — they're excluded third-party material either way [K].

## 1.5 Enforcement (Questions 13–15)

### Q13. What you can actually DO when someone copies your code or app

The realistic escalation ladder for a solo developer [S/K]:

1. **Preserve evidence** (screenshots, archive.org captures, downloaded copies, dates) — everything downstream depends on it [K].
2. **Cease-and-desist** — a letter from you is free; from a lawyer, $300–$1,500 [K-approx]. Often sufficient against identifiable small actors.
3. **DMCA § 512 takedown** to the hosting platform. GitHub specifics (where pirated builds of desktop apps usually surface): submit via the online form at github.com/contact/dmca (fastest) [S]; **forks are not automatically disabled — you must list each infringing fork**, except networks >100 repos can be disabled wholesale with a representative-review statement [S]; sworn under penalty of perjury, and § 512(f) creates liability for knowing material misrepresentation — be accurate [S]. If the user counter-notices, GitHub restores the content in 10–14 days **unless you file suit and tell GitHub** — a takedown alone cannot keep contested content down [S]. Google Search de-indexing and host-level notices work the same way for non-GitHub piracy [K].
4. **Copyright Claims Board** (the CASE Act small-claims court) — see Q14. Designed for exactly your size of dispute.
5. **Federal court** — requires the registration certificate (or refusal) per *Fourth Estate* [S]; realistic only for business-threatening infringement (a competitor shipping your code), ideally where willfulness + § 505 fees make economic sense [K].
6. **ITC § 337 exclusion orders** exist for infringing *imports* — practically irrelevant for a distributed-by-download app; mentioned only for completeness [K].

### Q14. What enforcement actually costs — and when each tier makes sense

| Tool | Out-of-pocket | Cap / outcome | When it makes sense for an $89 app |
|---|---|---|---|
| DMCA takedown | $0 (your time) | Content removed; reversible by counter-notice | Always the first move against distribution piracy |
| C&D letter | $0–$1,500 [K-approx] | Voluntary compliance | Identifiable infringer, pre-litigation posture |
| **CCB** | **$100 total** ($40 to file + $60 when the case activates) [S] | **$30,000/proceeding; $15,000/work statutory if timely registered** [S]; up to $7,500/work if not timely [K]; Smaller Claims track ≤$5,000 with a single officer [S] | Identifiable US infringer; you can file with a merely **pending** application (unlike federal court) [S]; designed for self-representation [S] |
| § 512(h) subpoena | ~$50–$60 court fee [S/K — $47 as of 2016, since increased] | Unmasks anonymous infringers via their host; clerk must issue if papers are in order, no judge needed [S] | Identifying who's behind a piracy site/repo; frequently produces settlement without suit [S] |
| Federal lawsuit | Five to six figures: AIPLA's biennial Economic Survey is the standard authority (2025 edition current; full tables paywalled at $495) [S]; prior editions put copyright suits with <$1M at risk at a **median ≈$200k–$550k through trial**, and realistically $50k–$150k just through early motions [K-approx] | $750–$150k statutory per work + discretionary fees | Only for existential threats, with counsel, ideally with fee-shifting prospects |

**The CCB's structural weakness — the opt-out problem**: respondents have 60 days to opt out, which kills the proceeding and leaves you with federal court or nothing [S]. (The two-installment fee means you only lose $40 to an opt-out [S].) Sophisticated/foreign infringers opt out; small US infringers often don't. There are no respondent fees to defend or counterclaim [S], and Register-review of a final determination costs $300 [S].

### Q15. The innocent-infringer defense and how you defeat it

Under § 504(c)(2), an infringer who proves they "[were] not aware and had no reason to believe" they were infringing can have statutory damages reduced to **as little as $200 per work** [S]. The antidote is cheap: under **§ 401(d)**, when a proper copyright notice appears on the published copies the defendant had access to, the court gives **no weight** to the innocent-infringement claim (narrow § 504(c)(2) exceptions aside) [S]. Courts have applied this strictly — notice on the publisher's copies defeated the defense even where the defendant copied from an unmarked stream (*Maverick Recording Co. v. Harper*, 598 F.3d 193 (5th Cir. 2010)) [K]. Hence TL;DR item 6: notice in the app, installer, site, and releases page. Registration itself also undercuts "no reason to believe" — it's a public record [K].

## 1.6 International (Questions 16–17)

### Q16. Berne Convention — what travels and what doesn't

There is **no international copyright**; protection abroad depends on each country's national law, knit together by treaties [S]. The Berne Convention has **182 parties as of December 2025** (the US joined March 1, 1989); your work is automatically protected in all of them, **without registration**, under national treatment — each country protects you the way it protects its own authors [S]. WTO/TRIPS (166 members) adds enforcement obligations; WIPO Copyright Treaty (118) and WPPT (114) extend digital rights [S]. The short list of places with effectively no US copyright relations: Eritrea and Iran ("None"), Palau, Somalia, South Sudan ("Unclear") [S — Circular 38a, Dec 2025].

What your **US registration** adds abroad: essentially evidence — a government certificate of authorship and date, useful in foreign proceedings and to platforms everywhere. What does **not** travel: US statutory damages, § 505 fees, and the § 410(c) presumption — those are creatures of US law for US litigation [S/K]. One subtlety: points of attachment (your nationality, place of first publication) are fixed at first publication, which is why the Office suggests confirming them before first publishing — yours are clean (US author, US publication) [S].

### Q17. When a foreign infringer copies the app — realistic options

In rough order of cost-effectiveness for a solo dev [S/K]:

1. **US-platform takedowns work regardless of where the infringer sits.** GitHub, Google, Cloudflare, YouTube, Discord, Stripe/PayPal — DMCA notices to US providers remove the distribution channel even when the human is in a non-enforcing jurisdiction [S/K].
2. **Payment rails**: report pirate sellers to Stripe/PayPal/credit-card abuse channels — cutting off payment is often more effective than legal process [K].
3. **Marketplace complaints** (Microsoft Store, app directories, download portals) — all have IP complaint forms [K].
4. **Local counsel in the infringer's country** for anything beyond takedowns — Berne guarantees you standing, but you litigate under local law with local remedies; the Copyright Office itself advises consulting counsel familiar with both systems and won't recommend attorneys [S]. Economically rational only against a revenue-generating commercial clone [K].
5. Accept that some jurisdictions are dead ends (Q16 list) and starve them at the platform layer instead [K].

## 1.7 Disagreements & uncertainties (Report 1)

1. **AI-generated code and registration scope** — the live issue for this app. The Office's disclosure guidance is clear in principle (more-than-de-minimis AI-generated material must be disclosed; claim limited to human authorship) [S], but where AI-assisted ends and AI-generated begins for agentic coding workflows is **not settled** by the Jan 2025 report and has not been tested in litigation. Only counsel should call this for your filing.
2. **Model weights copyrightability** — expressly unresolved at the Office level [S]; commentators split [S]. No action needed for you, but don't rely on the weights' MIT license meaning anything if the weights turn out to be uncopyrightable.
3. **AIPLA litigation medians** — the 2025 edition's actual tables are paywalled; the dollar figures in Q14 are from prior editions via training [K-approx]. If you ever budget litigation, buy the survey ($495) or have counsel quote it.
4. **eCO vs ECS** — mid-migration; the page describing the ECS pilot is undated, so which system you'll land in when you file can't be confirmed offline [S — flagged by the research agent itself]. Functionally irrelevant: same application, same fee.
5. **Verification caveat** — all [S] claims are single-sourced extractions; the adversarial cross-check was rate-limited out (see header). The fee/processing numbers come straight from copyright.gov pages fetched this session and are low-risk; re-verify them at copyright.gov/about/fees.html the day you file.

## 1.8 What a lawyer would tell you + costs (Report 1)

**Genuinely worth paying for:**
- **One hour on the AI-authorship disclosure question before filing v1.0** — this is the only part of your registration where a wrong answer creates real downside (a certificate procured with a knowingly inaccurate application can be challenged under § 411(b), and § 506(e) criminalizes knowing false statements [K]). Expect $300–$600 [K-approx]; some copyright boutiques will fold it into a flat-fee registration.
- **Flat-fee registration handling**: typical published ranges for software registrations are **$250–$1,500** plus the $65 fee [K-approx — verify against current published menus; the workflow's fetch of fee-menu sources was rate-limited]. Worth it once; copy the pattern yourself thereafter.
- **Enforcement letters** when the day comes: $300–$1,500 [K-approx].

**Safely DIY:**
- The registration itself (after the AI question is answered), using Circular 61 + this document.
- Copyright notices, version-cadence re-registrations, DMCA takedowns (follow GitHub's form precisely; you're swearing under penalty of perjury [S]), CCB filings (designed for pro se [S]).

**Common DIY mistakes to avoid** (each documented above): wrong application type (Single when ineligible — refiling resets your effective date [S]); missing/incomplete Limitation of Claim for OSS [S]; New Material ≠ Author Created text [S]; depositing object code and eating the Rule of Doubt [S]; blowing the 45-day examiner-correspondence window [S]; fixable-but-annoying errors cost $100 via supplementary registration [S].

---
---

# REPORT 2 — EULA Law for Commercial Desktop Software

## 2.1 TL;DR — What you should actually do, in order

Your current EULA's architecture (first-launch clickwrap + 30-day refund) sits on remarkably strong precedent — *Davidson v. Jung* upheld essentially your exact pattern (click-to-agree before use, full terms not visible pre-purchase, 30-day refund if you decline) [S]. The gaps are in specific clauses:

1. **Link the EULA on the purchase page** (Stripe checkout description + a "License terms" link near the buy button). First-launch assent is your binding moment [S], but pre-purchase availability cures the *Klocek*-line "money now, terms later" objection in unfriendly jurisdictions and is universal best practice [S]. One line of HTML; do it this week.
2. **Add an interoperability savings clause to the anti-reverse-engineering section.** Under EU Directive 2009/24/EC, decompilation for interoperability (Art. 6) and observation/study/testing (Art. 5(3)) **cannot be contracted away — contrary terms are "null and void" (Art. 8)** [K]. Without a savings clause your blanket ban is void as to EU users and reads as an unfair term. Example language for counsel to adapt: *"Nothing in this Section limits any right you may have under applicable law that cannot be lawfully excluded, including decompilation solely to achieve interoperability under Directive 2009/24/EC."*
3. **Rethink mandatory arbitration at your price point.** Arbitration clauses are enforceable post-*Concepcion* [K], but the **business pays the arbitrator**: AAA consumer rules cap the consumer's cost (~$225) while the business pays roughly $3,000–$4,000+ per case; JAMS similar or higher [K-approx — verify current schedules]. One disgruntled $89 customer costs you 35× the purchase price to arbitrate, and **mass arbitration** weaponizes this (the fee-gambit campaigns that pushed Amazon to drop its arbitration clause in 2021 [K]). Options, best-first for you: (a) drop arbitration; require informal resolution, then small claims or your home-state courts; (b) keep it but add a **small-claims carve-out**, a **30-day consumer opt-out** (bolsters enforceability [K]), an informal-negotiation prerequisite, and batching rules. This is a real drafting decision — counsel territory.
4. **Replace Delaware governing law with your home state** unless you actually have a Delaware entity. Choice-of-law needs a reasonable relationship to the parties or transaction (Restatement (Second) § 187; UCC § 1-301) [K]; Delaware's famous corporate-law advantages are about *entity governance*, not consumer contracts [K]. A sole proprietor with no Delaware nexus invites a challenge that voids the clause — and consumers keep their home state's mandatory protections regardless (US fundamental-policy doctrine; Rome I Art. 6 for EU buyers) [K]. Your home state is simpler, defensible, and where you'd actually litigate.
5. **Publish a privacy policy for the website** (if one isn't live). CalOPPA requires a conspicuous privacy policy from any online service available to Californians that collects PII — **no revenue threshold, applies extraterritorially**, and name/email/address collected by your Stripe checkout is squarely PII [S]. There's a 30-day cure after notice of noncompliance [S], with UCL penalties up to $2,500 per violation behind it [S]. The app's no-telemetry design is your headline asset — write it down: "the app processes all audio locally and transmits nothing."
6. **Ship third-party license notices.** MIT requires preserving the copyright + license text; Apache-2.0 additionally requires carrying the NOTICE file [K]. Generate a `THIRD-PARTY-NOTICES` file (cargo-about / cargo-license for crates + a script over package.json) and expose it in the installer directory and an About → "Open-source licenses" link [K]. This is a *license condition*, not politeness — and per Report 1 Q11, unlawful use of preexisting material can undermine your own protection [S].
7. **Patch the liability cap with standard carve-outs.** Caps tied to price paid are normal and generally enforced for economic loss [K], but blanket exculpation is void for fraud, willful injury, and (in California, Civ. Code § 1668) violations of law [K]; UCC § 2-719(3) makes consumer personal-injury exclusions prima facie unconscionable, and § 2-719(2) revives remedies when an exclusive remedy "fails of its essential purpose" [K]. Add: cap doesn't apply to gross negligence/willful misconduct/fraud; "some jurisdictions do not allow..." savings language; keep the 30-day refund as the baseline remedy (it's what makes AS IS + cap look fair).
8. **Add the missing boilerplate that does real work**: no-transfer/no-resale clause (locks in the *Vernor* "licensed, not sold" factors — your EULA needs all three: license grant ✓, transfer restrictions (add), use restrictions ✓ [S]); severability; entire agreement; assignment (you may assign — important if you ever sell the business); survival; export-compliance line; update/no-support disclaimer; termination effects (license ends → must delete copies).
9. **Disclose the registry trial-state storage in one sentence** (EULA or privacy page): "the software stores license and trial state locally, including in the Windows registry." Not strictly required, but undisclosed persistence is the kind of thing FTC deception analysis and state spyware statutes (e.g., California's Consumer Protection Against Computer Spyware Act) treat as material when surreptitious [K]; one sentence moots it. (Mac build: same sentence covers `defaults`/plist storage — per your dual-platform rule.)
10. **Trial terms**: add a short Trial section to the one EULA (no separate doc needed [K]): trial scope/limits may change or end at any time; no warranty/support; tampering with trial-state mechanisms is a material breach. Note: your HMAC-signed trial blob and Ed25519 activation are "technological measures controlling access" in DMCA § 1201 terms — the same theory that won in *Davidson* and *RealNetworks* [S] — so circumvention tooling against them is independently unlawful, beyond breach of contract.

## 2.2 Enforceability (Questions 1–5)

### Q1. Are click-wrap/browse-wrap EULAs actually enforceable?

**Clickwrap: yes, presumptively.** Courts presume clickwrap enforceable because the user performs an affirmative act of assent; browsewrap (terms merely posted) is where enforceability dies [S]. The empirical picture from a 2025 survey of 40 California state and federal cases: federal courts enforced **every** clickwrap/scrollwrap agreement surveyed for the seller, vs. 6 of 9 browsewrap; state courts 7/9 clickwrap vs. 3/8 browsewrap [S].

The doctrinal arc, case by case:

- ***ProCD v. Zeidenberg***, 86 F.3d 1447 (7th Cir. 1996): shrinkwrap terms inside the box enforceable; vendor is "master of the offer"; contract forms when the user uses the software after a chance to read and return; contract claims not preempted by the Copyright Act because contracts bind only the parties [S].
- ***Specht v. Netscape***, 306 F.3d 17 (2d Cir. 2002): terms below the download button, no required action = no assent; **reasonably conspicuous notice + unambiguous manifestation of assent** are the essentials of electronic contract formation [S].
- ***Feldman v. Google*** (E.D. Pa. 2007): scrollable clickwrap with "I agree" enforced (AdWords) — the standard cite that clicking binds even unread terms [K].
- ***Nguyen v. Barnes & Noble***, 763 F.3d 1171 (9th Cir. 2014): a conspicuous hyperlink alone — even adjacent to action buttons — is **not** constructive notice; browsewrap survives only where the setup resembles clickwrap or an explicit textual notice says continued use = assent; burden on the party enforcing [S].
- ***Meyer v. Uber***, 868 F.3d 66 (2d Cir. 2017): reasonably conspicuous hyperlinked terms + an uncluttered screen + a button whose context signals contractual significance = enforceable "sign-in-wrap" [K].
- ***Berman v. Freedom Financial Network*** (9th Cir. 2022) — the modern two-prong inquiry-notice test: (1) reasonably conspicuous notice, (2) action that unambiguously manifests assent [S]. As of the 2025 survey, no uniform bright-line standard has emerged; courts do fact-specific notice analysis [S].

**Your first-launch modal — terms displayed, affirmative click required before the app functions — is the strongest position in this entire case law** [S — the Seattle U. L. Rev. piece treats exactly this as the safe harbor]. Keep the button un-prechecked, keep the full text scrollable in-place, and log acceptance (timestamp + version of terms) locally with the license state [K — evidence practice the Kennedys client alert recommends [S]].

### Q2. Is first-launch acceptance enough? Pre-purchase availability?

First-launch acceptance is sufficient under the dominant line: *ProCD* (pay first, terms later, right to return) [S]; ***Hill v. Gateway 2000***, 105 F.3d 1147 (7th Cir. 1997) (terms in the box + 30-day keep period = acceptance, extending ProCD beyond software) [S]; and most on-point, ***Davidson & Associates v. Jung***, 422 F.3d 630 (8th Cir. 2005): Blizzard's click-before-install EULA — terms **not** printed on the packaging, with a **30-day full-refund return right** — held an enforceable contract [S]. That is your architecture, including the refund window.

The contrary authority you design around: ***Klocek v. Gateway***, 104 F. Supp. 2d 1332 (D. Kan. 2000), which rejected ProCD's premise — the *purchaser* is the offeror; terms shipped afterward are UCC § 2-207 proposals needing **express** consumer assent; keeping the product ≠ assent [S]. *Step-Saver* (3d Cir. 1991) is the older anchor of that camp [S/K]. Two saving graces for you: (1) courts "have generally favored the ProCD approach" [S]; (2) unlike Gateway's in-box terms, you *do* obtain an express affirmative click — Klocek's actual holding (silence ≠ assent) doesn't reach a mandatory click [K]. The belt-and-suspenders fix is TL;DR #1: link the EULA at checkout so no court ever has to pick a side — the *Klocek* footnote itself points to disclosure at or before sale as the cure [S].

### Q3. Anti-reverse-engineering clauses in the US

**Enforceable as contract terms, with real limits.**

- ***Bowers v. Baystate Technologies***, 320 F.3d 1317 (Fed. Cir. 2003): shrinkwrap ban on reverse engineering enforced; breach-of-contract claim **not preempted** by the Copyright Act; jury verdict for Bowers (≈$5.27M across contract and patent claims) affirmed in relevant part — over a dissent warning that contract was being used to protect unprotectable material [S/K].
- ***Davidson v. Jung*** (8th Cir. 2005): same holding — licensees **may contractually waive** their fair-use right to reverse engineer; state contract claims not preempted [S].
- The limits: (1) ***Vault v. Quaid***, 847 F.2d 255 (5th Cir. 1988) — the lone contrary appellate authority — found a Louisiana-statute-backed RE prohibition preempted [S]; (2) absent a contract, intermediate copying for reverse engineering is **fair use** when it's the only way to reach unprotected functional elements (*Sega v. Accolade*, 977 F.2d 1510 (9th Cir. 1992); *Sony v. Connectix*, 203 F.3d 596 (9th Cir. 2000)) [S] — your clause is what removes that default; (3) DMCA **§ 1201(f)** preserves a narrow interoperability exception to *circumvention* liability, though *Davidson* shows a EULA's scope can defeat it where the circumvention enables infringement [S]; (4) a California concurrence (*DVD CCA v. Bunner*) rejected the idea that a EULA ban makes reverse engineering "improper means" for trade-secret purposes — a flagged counterpoint, not a holding [S].

Net: keep the clause; it's your *Bowers/Davidson* contract hook against crackers who accepted the EULA. Just don't expect it to bind non-parties (someone who analyzes a binary they never licensed never accepted your terms — § 1201 and copyright are your tools there [K]).

### Q4. Anti-reverse-engineering clauses in the EU — your clause is partly void there

EU Software Directive 2009/24/EC [K throughout this answer — the EU-angle fetches were rate-limited; the Directive text and CJEU holdings are stable, settled law well within training]:

- **Art. 5(3)**: a licensed user may **observe, study, and test** the program's functioning to determine underlying ideas/principles while doing licensed acts — cannot be excluded by contract.
- **Art. 6**: **decompilation** is permitted without authorization when indispensable to achieve **interoperability** of an independently created program, subject to conditions (licensed user; info not otherwise readily available; limited to necessary parts; not for a competing substantially similar program).
- **Art. 8 (the teeth)**: contractual provisions contrary to Art. 6 or the Art. 5(2)–(3) exceptions are **null and void**.
- ***SAS Institute v. World Programming*** (CJEU C-406/10, 2012): functionality, programming languages, and data formats are not protected expression; license terms can't stop a licensee from observing/studying to reproduce functionality.
- ***Top System v. Belgian State*** (CJEU C-13/20, 2021): a lawful acquirer may even **decompile to correct errors** under Art. 5(1), notwithstanding contrary contract terms (contract may regulate modalities, not extinguish the right).

Consequence: a blanket "no reverse engineering, ever" clause is unenforceable to that extent against EU/EEA users, and in consumer-protection terms an unenforceable boilerplate prohibition risks unfair-term treatment (and in Germany, AGB-control problems) [K]. The fix is not to delete the clause but to add the savings sentence in TL;DR #2 — US courts still give you *Bowers/Davidson*, EU users keep what the Directive guarantees, and nobody can call the term deceptive.

### Q5. Binding arbitration for consumer software — enforceable, but is it wise for you?

Enforceability: the FAA (9 U.S.C. § 2) makes written arbitration provisions valid and enforceable except on general contract-law grounds [S]; *AT&T Mobility v. Concepcion*, 563 U.S. 333 (2011) lets class-action waivers ride along; *Epic Systems* (2018) reinforced it [K]. The recurring failure mode isn't arbitration law — it's **contract formation** (Q1): no valid assent, no arbitration clause (*Specht* itself was a failed motion to compel arbitration [S]).

The economics, which cut against you at $89 [K-approx — verify current fee schedules before deciding]:

- AAA Consumer Rules: consumer filing fee capped around $225; the **business** pays case-management + arbitrator compensation — realistically $3,000–$4,000+ per case even for a case you win. JAMS: consumer pays $250; business pays the rest, typically more.
- **Mass arbitration**: claimant firms file hundreds/thousands of individual demands; the per-case business fees become ruinous before any merits ruling (the *Abernathy v. DoorDash* fee-gambit era; Amazon dropped its consumer arbitration clause in 2021 in the aftermath) [K]. Providers have since added batching protocols, and drafters respond with batching clauses and bellwether procedures — drafting that genuinely needs counsel [K].
- Mitigations if you keep it: small-claims carve-out (both sides may elect small claims), 30-day opt-out right (courts repeatedly cite opt-outs in rejecting unconscionability [K]), informal-resolution prerequisite with a notice address, batching/bellwether terms, and a severability line saying if the class waiver fails the whole arbitration clause fails (prevents class *arbitration*, the worst outcome [K]).

My read for a sole developer: the clause protects against a class action you're unlikely to face and exposes you to fee asymmetry you can't absorb. Dropping it in favor of "informal resolution → small claims or [home state] courts" is the simpler, cheaper risk posture — but this is squarely a counsel decision (TL;DR #3).

## 2.3 Best practices (Questions 6–10)

### Q6. The clause checklist for a commercial desktop EULA

[K — assembled from standard practice; items marked ✓ you already have per your description]

1. License grant — personal, non-exclusive, non-transferable, **licensed not sold** ✓ (add the explicit "licensed, not sold" phrase + transfer restriction to nail the *Vernor* factors [S])
2. Scope/restrictions — machine count ✓ (5), no resale/rental/hosting, no circumvention of license/trial mechanisms (add — see Q17 tie-in)
3. Reverse-engineering clause ✓ + EU savings sentence (add)
4. Trial terms (add — TL;DR #10)
5. Updates — may provide, may discontinue, no obligation (add if absent)
6. Support — what you offer (email, best-effort), what you don't (add)
7. IP ownership — all rights reserved; feedback license (add)
8. Third-party/open-source components — pointer to notices file (add)
9. Privacy statement — "no telemetry; local processing; site privacy policy at <URL>" ✓/expand
10. Warranty disclaimer ✓ — keep "AS IS" conspicuous (caps), with "some jurisdictions..." savings [K — UCC § 2-316(3)(a) blesses "as is" for implied warranties; Magnuson-Moss only bites if you offer a *written warranty*, which you don't]
11. Limitation of liability ✓ — add carve-outs + failure-of-essential-purpose backstop (TL;DR #7)
12. Indemnity — for consumer EULAs, keep it one-way-light or omit; aggressive consumer indemnities read as unfair [K]
13. Termination — for breach; effect = delete copies; survival clause (add)
14. Export compliance — one line; authentication-only crypto (Ed25519 signature verification) is generally outside EAR encryption controls, so boilerplate suffices [K]
15. Governing law + dispute resolution ✓ — revise per TL;DR #3/#4
16. Severability, entire agreement, assignment (you may; user may not), no-waiver, force majeure, contact address (add as needed)
17. Amendment mechanics — new terms apply to **new versions/downloads**, not retroactively without notice (see Q7)

### Q7. Clauses that are commonly included but unenforceable or risky

[K unless noted]

- **"We may change these terms at any time without notice, continued use = acceptance"** — illusory-modification doctrine; unilateral silent amendment of an already-paid perpetual license is the classic unenforceable term. Tie changes to new versions + reasonable notice.
- **Total exculpation** ("not liable for anything, including our own gross negligence/willful acts") — void as against public policy (Cal. Civ. Code § 1668 and equivalents); can poison the whole limitation section in unfriendly courts.
- **Disclaiming non-disclaimable consumer rights without savings language** — UCC § 2-719(3) (consumer personal injury), state mini-UDAP statutes; always carry "to the maximum extent permitted by law" + jurisdictional savings.
- **EU-blind reverse-engineering bans** — null and void per Art. 8 (Q4).
- **"Void where prohibited" / deliberately overbroad boilerplate aimed at NJ consumers** — New Jersey's TCCWNA creates statutory claims for terms that violate clearly established rights without specifying applicability to NJ; post-*Spade v. Select Comfort* (NJ 2018) plaintiffs need actual aggrievement, but the cheap fix is jurisdiction-aware savings language rather than "void where prohibited" alone.
- **Fake liquidated damages** ("$10,000 per violation") — penalties, not enforceable; statutory damages via timely copyright registration are the legitimate version (Report 1).
- **Unconscionable venue/forum picks** (a forum with no party nexus) — same reasonable-relationship problem as the Delaware choice (Q9).
- **Browsewrap-style incorporation** ("by downloading you agree") with no click — exactly what *Specht/Nguyen* strike down [S]; you don't do this — don't start.

### Q8. The $89 liability cap

Caps limited to fees paid are the industry default and are routinely enforced in **B2B** deals; in **consumer** contracts they're generally enforceable for *economic loss* but attacked successfully where [K]:

- the conduct is **gross negligence, recklessness, willful misconduct, or fraud** (exculpation void — § 1668-type rules);
- **personal injury from a consumer product** is excluded (UCC § 2-719(3): prima facie unconscionable) — far-fetched for a dictation app, but the carve-out costs one sentence;
- **statutory claims** with non-waivable remedies are involved (consumer-protection statutes, BIPA-type laws);
- the **exclusive remedy fails of its essential purpose** (UCC § 2-719(2)) — if the cap+refund leaves a consumer with effectively nothing for a total failure, some courts revive ordinary remedies; keeping the 30-day refund honest is your defense;
- **unconscionability** in lopsided drafting (rare where price is $89 and a refund exists).

Verdict: $89 (or "amounts paid in the 12 months preceding the claim") is a reasonable, defensible cap **with** the carve-outs from TL;DR #7. Without them, the clause is brittle exactly when you'd need it.

### Q9. Delaware governing law — probably the wrong choice for you

Why companies pick Delaware: the Court of Chancery, deep corporate case law, predictable *entity* governance — reasons that concern incorporation, **not** consumer contract terms [K]. For choice-of-law in a contract: Restatement (Second) § 187 / UCC § 1-301 require the chosen state to have a **substantial relationship to the parties or transaction** (or another reasonable basis), and even then the choice yields to the **fundamental policy** of a state with a materially greater interest — which, for consumer protections, is typically the consumer's home state [K]. EU parallel: Rome I Art. 6 — choice of law cannot deprive an EU consumer of their home mandatory protections [K].

So: if you have a **Delaware LLC** that sells the licenses, Delaware has a party nexus and the clause is defensible (though consumers still keep home-state mandatory rights). If you're an unincorporated sole proprietor elsewhere, Delaware is an arbitrary pick that (a) invites the clause being disregarded, and (b) buys you law you don't know, in a state where you'd still have to litigate *somewhere else anyway*. Use your home state. (Separate business question, worth 30 minutes with counsel: forming an LLC for liability separation — at which point the LLC's state becomes the natural choice.) [K]

### Q10. Free trial vs. paid license terms

One EULA with a **Trial** section is the standard and sensible structure; a separate trial agreement adds maintenance burden without legal gain [K]. Trial-specific provisions that matter for you [K]:

- trial scope is whatever the software enforces (your 50-use lifetime cap), and you may **modify, limit, or terminate trial availability at any time**;
- no warranty/support obligations for trial use; liability cap applies a fortiori (consider $0/trial);
- **no auto-conversion or auto-charge** — you have no card on file; say so (it's a selling point);
- tampering with or circumventing trial-state mechanisms (including the signed trial blob and clock checks) is a **material breach and may violate law** — this sentence connects the EULA to your § 1201 position (Q17);
- trial users are licensees bound by the same EULA (acceptance still happens at first launch — which is exactly when trial users appear).

## 2.4 Specific risks for this app (Questions 11–14)

### Q11. Bundled MIT/Apache components — your actual obligations

- **MIT** (whisper.cpp, many crates/npm packages): the license text + copyright notice must be included "in all copies or substantial portions of the Software" — shipping binaries counts; carry the notices [K].
- **Apache-2.0** (some crates): include the license; if an upstream NOTICE file exists, you must carry it; state changes if you modified the code [K].
- **Mechanics**: generate `THIRD-PARTY-NOTICES.txt` with `cargo-about`/`cargo license` for Rust deps + a license-checker pass over npm deps; install it beside the binary; link it from About → "Open-source licenses" [K]. (Both platforms — per your dual-platform rule, wire the same file into the Mac bundle's About panel.)
- **Does the EULA have to disclose OSS?** No law requires the EULA itself to list components; the EULA should *point* to the notices file and clarify that third-party components are governed by their own licenses, which prevail over the EULA for those components [K].
- **IP-warranty angle**: you make no warranties (AS IS), so you're not warranting non-infringement of the OSS stack; if you ever sell B2B with an IP indemnity, the OSS inventory becomes diligence material — keep the notices file accurate from day one [K].
- Compliance also protects your own copyright claim (Report 1, Q11) [S].

### Q12. Microphone capture with purely local processing — the privacy map

The honest headline: **your architecture (no transmission, no developer possession of audio) eliminates most exposure, but not the obligation to say so in writing.**

- **Recording laws (CIPA § 632 etc.)**: California's two-party-consent statute targets surreptitious interception of *confidential communications*; it expressly excludes individuals **known by all parties** to be recording [S]. A user dictating into a visible transcription app is the openly-known recorder of their own speech; the *developer* of the tool is not the recording party [S/K]. Residual risk: a user covertly recording *others* with your app — that's the user's violation (and § 632(d) makes their recording inadmissible [S]); a "comply with recording laws" line in the EULA is standard prophylaxis [K].
- **Illinois BIPA (740 ILCS 14)**: regulates collecting/capturing/possessing "voiceprints" — speaker-**identifying** biometric data [K]. The live case to watch: the Apple/Siri BIPA class action, where (per the fetched reporting) a class was **certified in 2026** limited to users for whom "voiceprints or biometric feature vectors **capable of identifying them**" were computed — the class definition itself distinguishes identification vectors from mere transcription [S]; the claims also center on Apple's *disclosure of recordings to contractors* [S], an element structurally absent for you. Whisper-style speech-to-text decodes words; it does not compute or retain speaker-identification embeddings, and you never possess any audio or derived data [K — technical characterization]. Strong position; still **unsettled at the edges** (the theory survived a motion to dismiss in 2020 and certification in 2026 [S]), so: one-sentence disclosure ("processed locally, never transmitted, no voiceprints created or stored"), and have counsel sanity-check if you ever add speaker-ID or cloud features.
- **CCPA/CPRA**: applies only to businesses meeting thresholds — >$25M revenue, or 100k+ consumers' data bought/sold/shared, or 50%+ revenue from selling/sharing data [K]. You qualify for none. Inapplicable today; revisit at scale.
- **CalOPPA**: the one that **does** apply now — see TL;DR #5 [S].
- **GDPR**: selling to EU buyers makes your **website/Stripe flow** a controller of buyer data (Art. 3(2) targeting) — privacy notice, lawful basis, processor terms with Stripe (their standard DPA covers it) [K]. The **app** processes audio locally under the user's control; you never touch it, so you're not a controller of the audio at all [K]. Cover the website; the app's section of the privacy policy is one proud paragraph.
- **Is a privacy policy required when the app collects nothing?** For the *app* alone, arguably no — but CalOPPA attaches to the online service/website (which collects checkout PII) [S], GDPR attaches to the sales flow [K], and a published "we collect nothing in the app" policy is the cheapest trust asset you'll ever ship [K].

### Q13. Public GitHub Releases distribution

- **Does free downloadability weaken the EULA?** No — formation happens at first launch, after download; *who* downloaded is irrelevant to whether the runner clicked agree (Q1/Q2 law) [S/K]. The download being free is just your trial funnel.
- **GitHub ToS interplay** [K — fetches for ToS D.4–D.5 were rate-limited; verify text]: Section D licenses *GitHub* to host/serve your content, and D.5 grants other GitHub users the right to **view and fork public repositories** — that reaches repo *content*. Release binaries in a public repo are downloadable by design; none of this grants anyone a license to *use* the software contrary to your EULA, and your copyright is unaffected (you retain ownership; GitHub's license is for operating the service). Practical hygiene: **do not put an open-source LICENSE file in the releases repo** (it would signal an OSS grant); instead add a README/`LICENSE.md` stating "Proprietary. Use governed by the ALAN Echo EULA: <URL>" and repeat that line in each release's notes [K].
- **Forks**: a public repo can be forked (binaries copied with it) — your EULA + DMCA process handles misuse (Report 1, Q13: list forks explicitly in takedowns [S]).

### Q14. Installer writing trial state to the Windows registry

No statute requires disclosing registry writes as such [K]. The legal frame is **deception by material omission**: FTC Act § 5 and state UDAP analysis ask whether undisclosed software behavior would matter to a reasonable consumer and was hidden; state anti-spyware statutes (e.g., California's Consumer Protection Against Computer Spyware Act, Bus. & Prof. Code § 22947) target *deceptive* installation, surveillance, and resistance-to-removal behaviors [K]. Storing trial/license state locally is benign, industry-universal, and nowhere near those statutes' targets — **but** trial-state persistence that survives uninstall is exactly the kind of thing an annoyed power user writes a blog post about. One disclosure sentence (TL;DR #9) converts "hidden persistence" into "documented behavior," eliminating both the legal theory and the optics. Also offer manual cleanup instructions in your docs/uninstall notes [K — norms, not law].

## 2.5 Anti-piracy legal framework (Questions 15–17)

### Q15. The toolbox beyond the EULA

| Tool | What it gets you | Limits |
|---|---|---|
| Copyright infringement (17 U.S.C. § 501) | Damages/injunctions vs. copiers/distributors | Registration prerequisite to sue (*Fourth Estate*) [S]; see Report 1 |
| DMCA § 512 takedowns | Fast removal at the platform layer | Counter-notice → sue or it comes back [S] |
| **DMCA § 1201(a)(2), (b)** — anti-trafficking | Liability for **distributing** cracks/keygens/bypass tools, *independent of any copying* | The Ninth Circuit holds § 1201(a) needs **no infringement nexus** (*MDY*), the Federal Circuit (*Chamberlain*) disagrees — unresolved split [S] |
| DMCA § 1201(a)(1) — act of circumvention | The cracker's own act is unlawful | § 1201(f) interoperability exception exists but is narrow and fails where circumvention enables infringement (*Davidson*) [S] |
| CFAA 18 U.S.C. § 1030 | Unauthorized *access to computers* | Post-*Van Buren* (2021): "gates-up-or-down" — violating use restrictions on software you possess isn't CFAA "exceeding authorized access"; weak fit for license cracking; § 1201 is the right hook [K] |
| State computer-crime/UDAP statutes | Occasionally useful add-ons | Mostly duplicative for you [K] |
| Trademark (Lanham Act) | Against fake "official ALAN Echo" distributors/sites | Stronger once you register the mark [K] |
| Breach of contract (the EULA) | Against parties who accepted it | *MDY*'s covenant/condition rule: most EULA violations sound in contract only, **unless** the term is a license *condition* with a nexus to exclusive copyright rights [S]. Drafting note for counsel: making the grant expressly conditional on the machine-count limit ties over-installation (reproduction) to an exclusive right — converting it into copyright territory [S/K] |

### Q16. Realistic enforcement path against a keygen distributor

1. **GitHub-hosted keygens/cracks — use the Acceptable Use Policy route, not § 1201.** Sourced and important: GitHub's heightened expert-review process for § 1201 circumvention claims **does not apply to "unauthorized product licensing keys, key generators, or license-check bypass software" — those are handled under the AUP**, a simpler, faster complaint path [S]. File the AUP report; reserve the § 1201-style DMCA for tools that circumvent without being keygens.
2. If you do send a **§ 1201 takedown to GitHub**, know the post-youtube-dl posture: every credible claim is reviewed by technical + legal experts; you must concretely describe the TPM, how it controls access, and how the tool circumvents it; **ambiguity is resolved in favor of the accused developer**, backed by a $1M developer defense fund; such claims are <2% of GitHub's DMCA volume [S].
3. **Search/de-indexing**: Google's DMCA form for search results; same for Bing [K].
4. **Host/registrar notices** for standalone crack sites; CDN (Cloudflare) abuse reports reveal hosts [K].
5. **Unmasking anonymous distributors**: DMCA **§ 512(h) subpoena** — file a proposed subpoena + a copy of the takedown notice + a sworn declaration with a district-court clerk; the clerk **must** issue it without judicial review if the papers are in order; cost ≈ a $50–$60 filing fee [S — $47 as of 2016, since raised [K]]. Works against providers *hosting* the content (the conduit-ISP carve-out from the *RIAA v. Verizon* line limits it for mere ISPs) [S]. Identified infringers frequently fold and settle without suit [S].
6. **Suit** (federal or CCB where it fits): § 1203 remedies include **statutory damages of $200–$2,500 per act of circumvention or per device/product/service offered** — which scales per-download in trafficking cases, unlike per-work copyright statutory damages [K — § 1203(c)(3)].
7. **Cost-benefit honesty**: layers 1–4 cost time only and remove ~90% of casual piracy's distribution surface; layer 5 costs <$100; layer 6 starts at five figures with counsel. The whack-a-mole equilibrium for indie software is takedown-on-sight plus product velocity, with litigation reserved for a commercially significant, identifiable, US-reachable infringer [K].

### Q17. Has anyone won § 1201 cases over activation/license circumvention? Yes — repeatedly

- ***RealNetworks v. Streambox*** (W.D. Wash. 2000): the client-server "Secret Handshake" **authentication** held a technological measure effectively controlling access; preliminary injunction against the circumventing product under the anti-trafficking provisions; the **Sony Betamax "substantial noninfringing uses" defense rejected** as inapplicable to § 1201; but the companion Ripper tool (legitimate purposes, not primarily designed to circumvent) escaped — § 1201 is not strict liability for everything that touches protected software [S].
- ***Davidson & Associates v. Jung* (Blizzard v. BNETD)** (8th Cir. 2005): Blizzard's **CD-key "secret handshake"** validation = a TPM effectively controlling access; the bnetd emulator's bypass violated § 1201(a)(1), and distributing it (source *and* binaries) was § 1201(a)(2) trafficking — "no commercially significant purpose other than circumventing"; the § 1201(f) interoperability defense failed because the circumvention enabled infringement (pirated copies playable) [S]. **Directly analogous to a keygen for your Ed25519/JWT activation gate.**
- ***MDY Industries v. Blizzard*** (9th Cir. 2010): § 1201(a)(2) liability for selling the Glider bot that evaded Warden; § 1201(a) is a **standalone right requiring no infringement nexus** (expressly rejecting *Chamberlain*) — the open circuit split flagged in § 2.6; also the source of the covenant/condition rule and the *Vernor* licensed-not-sold factors used throughout this report [S].
- Plus the broader line: *Universal v. Corley/Reimerdes* (2d Cir. 2001, DeCSS) [K]; Microsoft's product-activation enforcement practice (largely settlements/defaults) [K]; *Synopsys v. Ubiquiti* (license-key circumvention claims surviving under § 1201, N.D. Cal. 2017–18) [K — verify details before citing].

**What this means for ALAN Echo**: your offline Ed25519-signed-JWT activation and HMAC-signed trial state are precisely the kind of access-control measures these cases protect. Registration of the copyright (Report 1) + these TPMs gives you both § 512 and § 1201 levers against crack distributors — with the GitHub AUP shortcut (Q16) as the everyday tool.

## 2.6 Disagreements & uncertainties (Report 2)

1. **"Money now, terms later" split**: *ProCD/Hill* (majority view, favored by courts [S]) vs. *Klocek/Step-Saver* (UCC § 2-207 view) [S]. Your mandatory click + checkout link strategy moots it.
2. **§ 1201 infringement-nexus circuit split**: *MDY* (9th Cir.: none required) vs. *Chamberlain* (Fed. Cir.: required); unresolved; *MDY* also notes a second split (siding with *Corley*) [S]. Against keygens you win under either reading (*Davidson* found enabled infringement anyway [S]).
3. **Reverse-engineering waivers**: *Bowers/Davidson* (enforceable) vs. *Vault* (preempted, 5th Cir.) + the *Bunner* concurrence (trade-secret skepticism) [S]; plus the US/EU divergence (Art. 8 nullity) [K]. Geographic savings language is the hedge.
4. **BIPA and local speech-to-text**: the Siri class certification (2026) shows voice-software BIPA theories have legs, while its own class definition (identifying feature vectors) and disclosure element distinguish pure local transcription [S]. Unsettled; low risk for your architecture; one-sentence disclosure recommended.
5. **Arbitration fee schedules and mass-arbitration protocols** are moving targets (AAA/JAMS revised schedules repeatedly in recent years) [K]; verify current numbers before finalizing the dispute-resolution clause.
6. **GitHub ToS D.4–D.5 exact text** for release binaries — characterized from training [K]; the fetch was rate-limited. Verify the current ToS wording before relying on the fine details in Q13.
7. **Verification caveat** — same as Report 1: [S] claims are single-sourced extractions; the 3-vote adversarial pass never ran (rate limit). Case holdings cited to court-hosted PDFs (8th Cir., 9th Cir. via govinfo) are the highest-confidence items in this report.

## 2.7 What a lawyer would tell you + costs (Report 2)

**Worth paying for (in order):**
1. **One revision pass on the EULA** implementing TL;DR #2–#4 and #7–#8 (EU savings clause, arbitration decision, governing law, cap carve-outs, missing boilerplate). Flat-fee review of an existing EULA from a tech/software attorney: **$500–$2,000**; full custom draft: **$1,500–$5,000** [K-approx — published indie-software flat-fee menus; the verifying fetches were rate-limited]. You need the review, not the custom draft — you have a working skeleton.
2. **The arbitration keep/drop decision** specifically — it's a judgment call about your risk posture with real fee consequences either way (often included in the review above).
3. **Privacy policy** covering site + app: **$500–$1,500** drafted, or $100–$300/yr from a reputable generator with attorney templates [K-approx]; given your unusually clean story, a generator + the one-paragraph local-processing statement is defensible to start.
4. **Entity formation consult** (LLC, ~$500–$1,500 + state fees [K-approx]) — not a EULA issue, but it changes the governing-law answer and your personal exposure; cheaper than any single hour of future litigation.

**Safely DIY:**
- Checkout-page EULA link; © notices; THIRD-PARTY-NOTICES generation; registry-disclosure sentence; trial section text (have counsel glance at it during the review); GitHub repo "Proprietary — see EULA" README; acceptance logging.

**Total sensible legal budget for where you are**: **$1,000–$2,500 one-time** (EULA review + privacy policy + the copyright AI-question hour from Report 1), then ~$0 ongoing until revenue justifies trademark registration (~$250–$350/class + $500–$1,000 attorney [K-approx]) and an entity.

---
---

# Combined priority roadmap

**This week (free, ~2 hours):**
1. © notices everywhere (app, installer, site, releases README)
2. EULA link on the checkout page
3. "Proprietary — governed by EULA <URL>" README in the releases repo
4. Generate + ship THIRD-PARTY-NOTICES (both platforms)

**This month (~$65–$200):**
5. Copyright registration: Standard Application, $65, deposit pages assembled to skip the secret-sauce files — after the AI-disclosure consult (below)
6. Publish/refresh privacy policy (CalOPPA) with the local-processing paragraph
7. Registry/trial-state disclosure sentence + trial section in the EULA

**Next 60–90 days (~$1,000–$2,500, one-time):**
8. Attorney: AI-authorship question for the registration (1 hr) — *before* item 5 if feasible
9. Attorney: EULA revision pass (EU savings clause, arbitration decision, governing law, cap carve-outs, transfer restriction, boilerplate)
10. Entity-formation consult

**Standing practice:**
- Re-register copyright each major version or annually
- Takedown-on-sight for piracy (GitHub AUP route for keygens; § 512 for copies; list forks)
- Log EULA acceptance (timestamp + terms version) with license state
- Re-verify fees at copyright.gov before filing — the 2026 NPRM will eventually change them

---

# Appendix A — Source bibliography

**Report 1 (23 fetched):** copyright.gov Compendium ch. 1400 & 1500 + comp3 index; Circulars 10, 14, 61(listed), 38a-equivalent international circular; copyright.gov/about/fees.html; processing-times FAQ PDF; continuous-development (ECS) page; rulemaking/feestudy2026; Federal Register doc. 2026-05529 (Mar. 20, 2026 NPRM); 17 U.S.C. §§ 102, 412, 504 (Cornell LII) + ch. 4 (copyright.gov); eCO help-limitation page; USCO *Copyright and AI, Part 2: Copyrightability* (Jan. 2025); CCB.gov FAQ; Copyright Alliance CCB fees FAQ; AIPLA Economic Survey page; GitHub DMCA submission guide; *Fourth Estate* opinion; marble.onl model-weights essay (blog-quality); Berkeley Tech. L.J. (Cox & Parra) article.

**Report 2 (11 fetched):** S. Cal. L. Rev. 2025 wrap-contract survey; Seattle U. L. Rev. online-contracts article; Duke CSPD EULA case compendium; Kennedys client alert (clickwrap arbitration); Termly CalOPPA guide; GitHub DMCA takedown policy; GitHub blog (youtube-dl reinstatement); *MDY v. Blizzard* (govinfo 9th Cir. PDF); *Davidson v. Jung* (8th Cir. PDF); *RealNetworks v. Streambox* (UH law mirror); Lutzker & Lutzker § 512(h) article; plus Cal. Penal Code § 632 text and Siri-BIPA case reporting via search agents.

**[K] backbone authorities cited from training** (verify pin cites before formal use): *Feldman v. Google*; *Meyer v. Uber*; *Berman v. Freedom Financial* (also [S]); *Sega v. Accolade*; *Sony v. Connectix*; *Vault v. Quaid*; *Vernor v. Autodesk*; *Concepcion*; *Epic Systems*; *Van Buren*; *Derek Andrew v. Poof Apparel*; *Maverick v. Harper*; *Fogerty v. Fantasy*; Directive 2009/24/EC; *SAS Institute* C-406/10; *Top System* C-13/20; Rome I Art. 6; UCC §§ 1-301, 2-207, 2-316, 2-719; Cal. Civ. Code § 1668; Cal. Bus. & Prof. Code §§ 22575 (CalOPPA), 22947 (spyware); 740 ILCS 14 (BIPA); 15 U.S.C. § 2308 (Magnuson-Moss); N.J. TCCWNA; 17 U.S.C. §§ 506(e), 1201, 1203; 18 U.S.C. § 1030.

# Appendix B — Re-running verification (optional)

The adversarial verification pass died on the session rate limit (resets 4:40am America/Denver). To re-run it with cached search/fetch results (only failed agents re-execute):

```
Workflow({scriptPath: "C:\\Users\\arowm\\.claude\\projects\\C--Users-arowm-alan-echo\\98a278a0-eda0-42ae-ab17-b87e95e899ab\\workflows\\scripts\\deep-research-wf_95dbcd24-3ac.js", resumeFromRunId: "wf_95dbcd24-3ac"})
Workflow({scriptPath: "C:\\Users\\arowm\\.claude\\projects\\C--Users-arowm-alan-echo\\98a278a0-eda0-42ae-ab17-b87e95e899ab\\workflows\\scripts\\deep-research-wf_ef466a2e-837.js", resumeFromRunId: "wf_ef466a2e-837"})
```

(Note: resume is same-session only; in a fresh session, the higher-value spot-check is simply re-fetching copyright.gov/about/fees.html, the processing-times FAQ, and the Federal Register NPRM before you file.)

The highest-value claims to spot-verify before acting: current fees ($65/$45/$800), the NPRM status (has the new schedule taken effect?), CCB fee structure, and GitHub's current AUP language on keygens.
