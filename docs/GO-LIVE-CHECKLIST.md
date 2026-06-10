# ALAN Echo — Go-Live Checklist (written 2026-06-10, overnight build session)

What's already done is in `whispr-local/docs/2026-06-10-alan-echo-golive-session-log.md`.
This file is what's LEFT — the items only a human can do, in order.

## A. Morning tasks (do these before announcing anything)

1. **Stripe live-mode setup (~10 min):**
   - **Enable the new webhook events (REQUIRED — the refund/dispute/delayed-payment
     handlers shipped 2026-06-10 never fire without this):** Stripe dashboard →
     Developers → Webhooks → the alanglobalintelligence.com endpoint → add events:
     `checkout.session.async_payment_succeeded`, `checkout.session.async_payment_failed`,
     `charge.refunded`, `charge.dispute.created`, `charge.dispute.closed`.
     (The live STRIPE_SECRET_KEY is marked Sensitive in Vercel and can't be pulled,
     so this must be done in the dashboard.)
   - Create the live giveaway code with the LIVE key from the Stripe dashboard:
     `cd stock-analyzer; $env:STRIPE_SECRET_KEY="<live sk_live_ key>"; npx tsx scripts/echo-giveaway.ts ECHO-FRIENDS 25`
     The script now also creates the persistent "ALAN Echo" Stripe Product and prints
     TWO env vars to set in Vercel (Production) — `ECHO_STRIPE_PRODUCT_ID` (locks
     coupons to Echo via applies_to) and `ECHO_GIVEAWAY_PROMO_IDS` (authorizes the
     codes with the $0-session guard). Set both, then redeploy. For one-per-friend
     unguessable codes instead of a shared word: `npx tsx scripts/echo-giveaway.ts --codes 25`.
   - Stripe dashboard → Settings → Tax: confirm an origin (head-office) address is set in
     LIVE mode. If it's missing, checkout still works — the route falls back to an untaxed
     sale and logs `[echo-checkout] automatic tax unavailable` — but set it properly.
   - `checkout.session.completed` is already enabled (same endpoint the tier
     subscriptions use).
2. **Buy one real copy yourself** (live card, then refund it from the Stripe dashboard):
   proves payment → key on success page → email delivery → refund flow end-to-end.
3. **Unsigned-installer posture (DECIDED 2026-06-10: no code signing — owner declined
   Azure enrollment).** SmartScreen's "Windows protected your PC" prompt is permanent;
   every surface that hands out the installer must set the expectation honestly.
   Mitigations in place / to do:
   - SHA-256 checksums published in the GitHub release notes (done 2026-06-10):
     installer `3ba5081b…3297c1c`, GPU pack `f29d1903…743609`.
   - Upload the installer to VirusTotal once and keep the report link handy for
     support replies (~2 min, free).
   - If a customer reports Microsoft Defender quarantining the installer, submit a
     false-positive report at microsoft.com/wdsi/filesubmission (fast turnaround) —
     unsigned exes that register hotkeys + synthesize keystrokes are heuristic-flag
     candidates.
   - SmartScreen reputation builds with download volume even without signing; the
     warning may fade on its own over weeks.
4. **Support email:** make sure `support@alanglobalintelligence.com` exists and is
   monitored — it's printed on /echo, the EULA, the refund policy, and the license email.
5. **Counsel re-review (when convenient):** the refund-policy Echo carve-out (30-day
   money-back) and the new EULA at /legal/echo-license were drafted tonight; the rest of
   the refund policy was counsel-signed 2026-06-09 as "all fees non-refundable".

## B. The clean-machine end-to-end test (the single most important gate)

Run on a Windows 10/11 machine (or fresh VM) with NO dev tools, NO CUDA, NO existing
`%APPDATA%\ALAN Echo`. Windows 11 Home has no Hyper-V/Sandbox, so use a second PC, a
cloud Windows VM (Azure/Paperspace), or a friend's machine.

1. Visit alanglobalintelligence.com/echo → Get ALAN Echo → pay with a LIVE card
   (refund later) → confirm the success page shows the key.
2. Confirm the license email arrives (check spam! — first sends to a new address are the
   deliverability test).
3. Download the installer (129.3 MB). Expect SmartScreen's "Windows protected your PC" →
   More info → Run anyway (permanent — the installer is unsigned by decision).
4. Install (should require NO admin prompt) → launch → onboarding → pick mic →
   test dictation. THIS exercises the bundled CPU base.en model — the make-or-break step.
   Expect ~1s transcription on a modern laptop; usable on old dual-cores.
5. Enter the license key → activates (offline — try it with Wi-Fi off to prove the claim).
6. Open Notepad → Ctrl+Shift+Space → speak → release → text pastes into Notepad.
7. Close window (goes to tray) → dictate again from tray state → Quit from tray →
   Task Manager: confirm NO `whisper-server.exe` left running.
8. Reinstall / second machine with the SAME key → must activate (binding was removed).
9. Giveaway path: checkout with promo code ECHO-FRIENDS → $0 → key + email arrive.
10. /echo/recover with the purchase email → key re-arrives.
11. Refund the test purchase in Stripe; verify funds reverse.

Record anything that breaks; the likely failure modes are SmartScreen friction (permanent —
mitigated by honest instructions + checksums) and email-to-spam (check Resend domain
DKIM/SPF — site email is mature, should be fine).

## C. Known gaps / deliberate deferrals

- **Installer is unsigned permanently** (owner decision, 2026-06-10). Mitigations:
  published SHA-256 checksums, honest SmartScreen instructions on the success page,
  license email, and /echo FAQ (item A3).
- **GPU acceleration**: the CUDA engine is NOT in the installer (698 MB). It's uploaded as
  `ALAN-Echo-GPU-Pack-1.0.0.zip` on the GitHub release for power users (extract into
  `%APPDATA%\ALAN Echo\models\`). An in-app "download GPU pack" flow is the top 1.x item.
  The /echo page words this honestly ("free GPU acceleration pack planned").
- **No auto-updater**: updates = new installer download. tauri-plugin-updater is the
  second 1.x item.
- **Hosting**: installer + GPU pack live on the public GitHub repo
  `diablobuster/alan-echo-releases` (free bandwidth, 129 MB + 436 MB assets). Move to
  Cloudflare R2 with a download.alanglobalintelligence.com domain when convenient (~$0/mo).
- **EU/UK VAT**: selling worldwide via direct Stripe means VAT obligations technically
  accrue from the first EU/UK sale. Stripe Tax calculates where registered; you are not
  registered anywhere yet. Low practical risk at launch volume; revisit at real volume or
  switch to Paddle (merchant of record — they carry all VAT) if international sales grow.
- **Trademark**: confirm "ALAN" / "ALAN Echo" clearance at some point before heavy marketing.
