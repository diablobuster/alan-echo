// Single source of truth for which EULA revision this build embeds.
// MUST be bumped in lockstep with EFFECTIVE_DATE in
// stock-analyzer/app/legal/echo-license/page.tsx whenever the EULA changes —
// a mismatch re-prompts every user for acceptance on next launch (intended).
export const EULA_VERSION = '2026-06-10'
