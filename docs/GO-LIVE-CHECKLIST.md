# ALAN Echo — Go-Live Checklist (written 2026-06-10, overnight build session)

What's already done is in `whispr-local/docs/2026-06-10-alan-echo-golive-session-log.md`.
This file is what's LEFT — the items only a human can do, in order.

## A. Morning tasks (do these before announcing anything)

1. **Stripe live-mode setup (~5 min):**
   - The site creates the product inline at checkout (no dashboard product needed).
   - Create the live giveaway code: `cd stock-analyzer; vercel env pull .env.vercel.local`
     then `$env:STRIPE_SECRET_KEY="<live key>"; npx tsx scripts/echo-giveaway.ts ECHO-FRIENDS 25`
     (or create a 100%-off coupon + promo code `ECHO-FRIENDS` in the Stripe dashboard).
   - Stripe dashboard → Settings → Tax: confirm an origin (head-office) address is set in
     LIVE mode. If it's missing, checkout still works — the route falls back to an untaxed
     sale and logs `[echo-checkout] automatic tax unavailable` — but set it properly.
   - Confirm the live webhook endpoint includes `checkout.session.completed`
     (it already does — same endpoint the tier subscriptions use).
2. **Buy one real copy yourself** (live card, then refund it from the Stripe dashboard):
   proves payment → key on success page → email delivery → refund flow end-to-end.
3. **Azure Artifact Signing enrollment (START TODAY — identity validation takes 1 day to
   2 weeks):** portal.azure.com → create "Artifact Signing" (formerly Trusted Signing)
   resource → Basic tier ($9.99/mo) → Identity Validation (government ID; individuals in
   US/Canada eligible since Jan 2026 GA — no 3-year-history rule anymore). Note: the
   certificate CN will be your validated legal name unless you enroll an LLC.
   When approved: `cargo install trusted-signing-cli`, create an Entra app registration,
   grant it "Trusted Signing Certificate Profile Signer", then set in tauri.conf.json:
   `bundle.windows.signCommand = "trusted-signing-cli -e https://wus2.codesigning.azure.net -a <Account> -c <Profile> -d ALAN-Echo %1"`
   with AZURE_CLIENT_ID / AZURE_TENANT_ID / AZURE_CLIENT_SECRET in env. Rebuild, re-upload
   the installer to the GitHub release, and SmartScreen warnings stop.
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
3. Download the installer (129.3 MB). Note the SmartScreen behavior — until code signing
   lands, expect "Windows protected your PC" → More info → Run anyway.
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

Record anything that breaks; the likely failure modes are SmartScreen friction (fixed by
signing) and email-to-spam (check Resend domain DKIM/SPF — site email is mature, should
be fine).

## C. Known gaps / deliberate deferrals

- **Installer is unsigned** until Azure validation completes (item A3).
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
