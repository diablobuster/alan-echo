# Read Aloud Phase 3 Design: speaking real-world text correctly

Addendum to `2026-10-02-read-aloud-design.md` and `2026-10-02-read-aloud-phase2-design.md`. Those specs and both plans' Global Constraints still bind.

Branch: `feat/read-aloud-phase3`, cut from `feat/read-aloud-phase2`. The phase 2 optimisation (Task 8) is merged in before the integration tasks.

Evidence: the glitch-hunt baseline, a throwaway spike from 2026-10-03. The report and harness are in the session scratchpad `p3/`, and the harness gets committed as part of this phase (§6).

## TLDR
- Alex: "it doesn't know Microsoft, it spells TechReview… it needs a bigger vocab and needs to sound out unfamiliar words."
- **Baseline:** 36.2 glitches per 1,000 words in the static pass, and 31.5 per 1,000 audible in the Kokoro → Whisper round trip.
- About 75% of the glitches are **numbers**, not vocabulary.
- **The fix is a text-normalisation pre-pass** before G2P (G2P = grapheme-to-phoneme, the step that turns spelling into sounds). It covers numbers, currency, dates and times, ordinals, abbreviations, dotted initialisms, URLs, symbols, units, Roman numerals and curly apostrophes.
- **Also:** smarter acronym and CamelCase handling before the GRU guesser (the small neural model that sounds out unknown words), plus a brand list and a homograph table.
- **Target:** static ≤ 5 per 1,000 and round-trip controls ≤ 8 per 1,000, with no category above 1 per 1,000 in the static pass.

## 1. Goals and non-goals
**Goals**
- Read money, numbers, years, dates, times, percentages, ordinals, units and math the way a news reader would.
- Never spell a real word. Never read an acronym as a word.
- Never go silent on a token.
- Sound out unfamiliar brands and CamelCase by their parts.
- Keep one highlight per source word.
- Speed: the pre-pass adds ≤ 1 ms per 30-word chunk in a release build.

**Non-goals**
- Swapping the voice model. Kokoro-82M stays; the round-trip numbers show the voice isn't the problem.
- Replacing misaki-rs's lexicon. Its GB-style length marks are noted as a later question.
- Sentence-level prosody beyond pauses.
- Non-English text.

## 2. Architecture
A new pure pre-pass runs inside `ra_tts::g2p_words`, **before** the hand-list, caps and hyphen logic that already exists:

```
chunk words (source words, one highlight each)
  → ra_norm::spans(&words) -> Vec<Span { start, end, spoken: String }>   // non-overlapping
  → for each span: phonemise `spoken` with the same G2P (incl. NameFallback) → phonemes P
                    feed misaki the first word as an override  [word](/P/)
                    every other word in the span → zero-length (no audio, highlight jumps over it)
  → words not in any span: existing path (hand override → caps rule → misaki → NameFallback)
```

- **Why overrides:** misaki-rs 0.6 accepts `[text](/phonemes/)`, including multi-word phoneme strings. This was verified by the spike: the phonemes land on the first subtoken and the other subtokens come back empty. So timing alignment stays one token group per source word.
- **Pause guard:** empty subtokens currently add whitespace, which is a pause. `token_phonemes` must collapse whitespace contributed by empty-phoneme tokens owned by the same word, and must collapse runs of spaces inside one token's phonemes.

### Modules (each pure, cfg(windows) like the other `ra_*`)
| File | Responsibility |
|---|---|
| `ra_numwords.rs` | Our own number→words (cardinal, ordinal, year, decimal digits). No num2words (LGPL). |
| `ra_norm_num.rs` | Number spans: years (1100–2099 standalone, `1990s`, `'90s`), decimals, cardinals (with commas), ordinals (`21st`), currency (`$ € £ ¥ ₹`, `US$ C$ A$`, scale suffixes `K M B bn T tn`, and a following `million/billion` word), percent, ranges (`2019–2021`, `24-17` scores → "to"), times (`7:30pm`, `22:37`), dates (`3/14`, `2017-09-25`, `Sept. 5`), fractions (`1/2`), phone numbers (digit groups), versions (`v2.1`), plus a guard: any token containing a digit never ends up empty. |
| `ra_norm_text.rs` | Non-number spans: abbreviation table (~80, with context rules: `St.`, `No.`, `Dr.`), dotted initialisms (`U.S.`, `Ph.D.`, `e.g.`, `i.e.`), domains, URLs, emails, @handles, #hashtags (split on case), symbols and math (`= × ÷ − ≈ ± √ § & / + < >`), units after a number (`km mph GB GHz kg mm °C °F sq ft`), Roman numerals in context (after War/Henry/Louis/Chapter/Part/Act/Super Bowl/Pope/King/Queen; `II–XXXIX`), glued ellipses. |
| `ra_norm.rs` | `spans(words)`: runs num, then text, and merges spans (numbers win on overlap). Plus `prepare(word)` character fixes: `’ ‘ ʼ` → `'`, U+2011 and U+2010 → `-`, NFKD accent fold available to callers. |
| `ra_names.rs` (modify) | **Acronyms:** an ALL-CAPS stem (≤ 5 letters, or no vowel) with an optional `s` or `'s` is spelled, with a plural or possessive suffix. A token with no vowel never goes to the GRU. Add a spelled list (SEC, AWS, ESG, GAAP, IPO, ETF, GPU, CPU, API, SaaS→"sass" is a brand entry). **CamelCase:** split on case boundaries and digit boundaries (`iHeart|Media`, `JP|Morgan`, `Postgre|SQL`, `USB|4`), phonemise each part (hand → misaki lexicon → CMU → spell caps → GRU), then join. **Fold:** a full Latin accent fold (ğ ă ș ț ł ő ø æ ß). **Never empty:** a token with digits or symbols reads them. |
| `data/names_fix.tsv` (extend) | Brand list: the ~300 most frequent GRU-routed proper nouns from the spike list, plus a curated tech/finance/news brand set (Microsoft, TechReview, LinkedIn, PayPal, OpenAI, iPhone, YouTube, Nvidia, …). Each is checked against the CamelCase+CMU result first and added only when that result is wrong. |
| `ra_homograph.rs` | POS-keyed table (~40: read, lead, live, wind, tear, record, object, project, present, content, minute, close, use, wound, bass, bow, row, desert, permit, refuse, invalid, produce, conduct, conflict, contract, convert, decrease, increase, insult, perfect, progress, rebel, subject, suspect, polish, entrance, august, moderate, separate, estimate, graduate). It picks the reading from misaki's token tag (`tk.tag`) plus one-word context rules ("to read", "have read", "will read", "lead to", "the lead", "live music"). |

### Raw-token reality
UIA/OCR tokens can split `$3.2B` into `$` `3` `.` `2B`, and they glue punctuation. `ra_text::normalize` must merge **glued** tokens (no whitespace between them) that form one written word: digits, `.`, `,`, `$`, `%`, `/`, `:`, and letters. This turns `$`,`3`,`.`,`2B` into `$3.2B`, so the pre-pass sees whole words. The fixtures are captured from real Chrome/Edge token streams, in the format the existing UIA tests use. Where a live capture isn't possible, they are written by hand to match `read_forward` output.

## 3. Reading rules (the contract the tests pin)
| Input | Spoken |
|---|---|
| `2007` (standalone) | two thousand seven |
| `1990`, `1683`, `2025` | nineteen ninety, sixteen eighty-three, twenty twenty-five |
| `1990s`, `'90s` | nineteen nineties, nineties |
| `2.5`, `0.35` | two point five, zero point three five |
| `7,428,973` | seven million four hundred twenty-eight thousand nine hundred seventy-three |
| `$480`, `$3.2B`, `$5 billion`, `US$32`, `€18.5 million`, `£149m` | four hundred eighty dollars · three point two billion dollars · five billion dollars · thirty-two US dollars · eighteen point five million euros · one hundred forty-nine million pounds |
| `8.6%` | eight point six percent |
| `21st`, `2nd`, `30th-largest` | twenty-first, second, thirtieth largest |
| `7:30pm`, `22:37` | seven thirty P M, twenty-two thirty-seven |
| `3/14` (no year) · `3/14/2025` · `2017-09-25` | March fourteenth · March fourteenth, twenty twenty-five · September twenty-fifth, twenty seventeen |
| `24-17` (score) · `2019–2021` | twenty-four to seventeen · twenty nineteen to twenty twenty-one |
| `U.S.`, `e.g.`, `i.e.`, `Ph.D.` | U S, for example, that is, P H D |
| `Gen.`, `Sen.`, `vs.`, `approx.`, `St. Louis`, `Main St.` | General, Senator, versus, approximately, Saint Louis, Main Street |
| `amazon.com`, `https://www.example.org/path` | amazon dot com · example dot org slash path (scheme and www dropped) |
| `press@example.org`, `@elonmusk`, `#MacronLeaks` | press at example dot org · at elon musk (CamelCase/CMU split) · hashtag macron leaks |
| `World War II`, `Henry VIII`, `Super Bowl LVIII` | World War two, Henry the eighth, Super Bowl fifty-eight |
| `5 km`, `8 TB`, `3 GHz`, `20°C` | five kilometers, eight terabytes, three gigahertz, twenty degrees Celsius |
| `a = b`, `3 × 4`, `±2` | a equals b, three times four, plus or minus two |
| `company’s` | company's |
| `SEC's`, `AWS`, `GPUs`, `EBITDA` | S E C's, A W S, G P Us, ee-bit-dah (brand-list entry) |
| `TechReview`, `LinkedIn`, `PayPal`, `OpenAI`, `iPhone`, `JPMorgan` | tech review, linked in, pay pal, open A I, eye phone, J P morgan |
| `I read it yesterday` / `I read every day` | red / reed |

When a rule is ambiguous (for example, `1500` as a year or a quantity), the context words decide: "in", "since", "by" or a month before it, or a following "s", means a year. A following unit or count noun means a cardinal. Otherwise: 1100–2099 reads as a year when it stands alone in a sentence that has no other digits; otherwise it reads as a cardinal.

## 4. Error handling
Everything is pure and never panics; the pre-pass returns `Vec<Span>`. On any internal inconsistency (overlapping spans, out-of-range indices), drop the span and log at debug. The word then falls back to the old path. G2P recursion is depth 1: phonemising `spoken` never runs the pre-pass again.

## 5. Performance
- Number words are built per span, and the tables are `static` sorted slices (binary search) or `match`. There is no regex crate unless it's already a dependency.
- Budget: ≤ 1 ms per 30-word chunk in release (`#[ignore]` perf test).
- 0 MB at rest: tables are static; nothing is loaded.

## 6. Testing and the regression gate
- **Unit tests:** every row of §3 plus edge cases (empty, emoji, CJK, 60-digit numbers, `$` alone, `.` alone, `1e10`, negative numbers `-5`, `−5`).
- **Alignment tests:** a chunk with spans keeps exactly one timing entry per source word. Words inside a multi-word span get zero-length entries at the span end. The highlight never moves backwards.
- **Glitch hunt committed:**
  - `tools/read-aloud/glitch-hunt/` holds the scripts and the self-written stress sentences only. Wikipedia text is CC BY-SA and is **not** committed; `build_corpus.py` fetches it locally into a git-ignored folder.
  - `src-tauri/src/ra_glitch_hunt.rs` is the `#[ignore]` static-pass test.
  - One command reruns everything. The static gate runs after every task; the round trip runs at the end.
- **Gate (final):** static ≤ 5 per 1,000 on the frozen corpus; round-trip random controls ≤ 8 per 1,000; no single category above 1 per 1,000 in the static pass. Homographs are tested by unit tests on targeted sentences, because Whisper can't hear them.
- **Listening board** of the residual glitches for Alex: `tools/read-aloud/glitch_board.html`, generated.

## 7. Risks
| Risk | Mitigation |
|---|---|
| Over-eager years ("1500 workers") | Context rules plus tests on both readings |
| Abbreviation false positives ("Sen." at a sentence end) | Only expand when the next word is capitalised or the abbreviation is unambiguous |
| CamelCase splitting real words ("McDonald", "iPhone") | Hand list and CMU lookup come first; split only on a CMU miss |
| misaki POS tagger quality limits homograph accuracy | Context rules first; measure on 50 targeted sentences; accept ≥ 85% |
| The overrides change prosody | Round-trip gate plus the listening board |
