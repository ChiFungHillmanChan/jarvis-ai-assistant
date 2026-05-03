# JARVIS Trial Launch Design

**Date:** 2026-05-03
**Status:** Draft (ready for review)
**Scope:** Ship a "fully functional" downloadable build of JARVIS to 5–10 trial users (friends, mentors, early waitlist). No accounts, no billing, no cross-device sync. Bug-fix iteration via auto-update.

---

## 1. Goals

- **Goal:** Hand a working DMG to 5–10 trial users so they can install, finish onboarding without help, and use the daily Gmail + Calendar + chat workflow without crashes.
- **Non-goal:** First paying user. That is Q2/Q3 work.
- **Non-goal:** Cloud account / Cognito / Stripe / cross-device sync / feature gating.
- **Roadmap alignment:** Roadmap T1.6 (v0.1.0 stability), T1.5 (in-app telemetry), Mentor "Distribution proposed forward to Q1" (Mentor Notes 第四部份).
- **Success criteria:**
  1. 5 trial users install the signed DMG and complete onboarding to the Dashboard without the founder doing setup work on their machine. (Phone-based moral support during Day-0 install watch is fine; touching their computer is not.)
  2. Each trial user reaches Day-7 with the app still installed and has had at least 3 chat or action sessions (counted via `conversations` table).
  3. The founder can ship a bug-fix release end-to-end in under 30 minutes via `scripts/release.sh`.
  4. At least 3 of 5 trial users submit feedback (in-app or email reply).

---

## 2. Decisions Locked

| Decision | Choice | Reason |
|---|---|---|
| AI inference for trial users | **Local LLM primary + BYOK fallback (A4)** | User already invested in `feature/local-model-implementation` branch. Privacy-first moat. No backend required. BYOK gives power users escape hatch. |
| Feature surface area | **Pragmatic triage (B3)** | Audit every feature, ship `pass` ones, hide `fail` ones. Aligns with mentor "Q1 = stability + onboarding only." |
| Distribution stack | **S3 + tauri-plugin-updater + manual feedback** | User has $200 AWS credits, no longer in free tier. No Sentry, no PostHog. In-app `LaunchPage` is the feedback channel. |
| S3 access mode | **Public-read bucket** | Notarized DMG already has signature for tamper detection. Q2 paid version migrates to private + presigned URLs once Cognito exists. |
| S3 region | **ap-east-1 (HK)** | ICP is HK/GBA. Latency negligible but narrative aligned with HKSTP. |
| Code signing | **Apple Developer ID (already owned)** | Required for macOS Gatekeeper. Separate from AWS credit. |
| Telemetry depth | **No external service** | LaunchPage SQLite + manual phone calls. Aligns with mentor "12 testers don't become 100 because of onboarding walls — solved by phone calls, not dashboards." |
| Release pipeline | **Local script, not GitHub Actions** | Apple cert lives in macOS Keychain. CI complexity not justified for solo dev. |
| Update prompt UX | **Opt-in modal**, not silent install | User control trumps speed for trial-stage software. |
| Feedback collection | **Local-only SQLite + manual outreach** | Privacy-first. No server. Founder calls each user. |

---

## 3. Architecture

```
┌──────────────────────────────────────────────────────────┐
│  jarvis-ai-assistant (Tauri v2 macOS app)                │
│                                                          │
│  NEW  First-Run Wizard                                   │
│   Step 1: Local LLM auto-setup (existing LocalAiSetup)   │
│   Step 2: BYOK cloud key (optional) — OpenAI/Claude/Gem  │
│   Step 3: Google OAuth (Gmail + Calendar)                │
│   Step 4: Land on Dashboard                              │
│                                                          │
│  EXISTING (uncommitted) LaunchPage                       │
│   - beta_testers + beta_feedback (SQLite)                │
│   - In-app feedback / waitlist capture                   │
│                                                          │
│  NEW  tauri-plugin-updater                               │
│   On startup: poll S3 manifest → prompt → apply          │
│                                                          │
│  EXISTING  Local LLM / Gmail / Calendar / Voice / Cron   │
│   (Subset gated by Phase 0 audit)                        │
└──────────────────────────────────────────────────────────┘
                       │ HTTPS GET
                       ↓
┌──────────────────────────────────────────────────────────┐
│  AWS S3: jarvis-releases (NEW, ap-east-1)                │
│   ├─ latest.json                ← updater manifest       │
│   ├─ JARVIS_x.y.z_aarch64.dmg   ← signed + notarized     │
│   └─ JARVIS_x.y.z_aarch64.dmg.sig                        │
│  Public-read on .dmg / .sig / latest.json                │
└──────────────────────────────────────────────────────────┘
                       ↑
                       │ direct download link
                       │
┌──────────────────────────────────────────────────────────┐
│  jarvis-official-website (Next.js, Vercel)               │
│   NEW: /[locale]/download                                │
│    - macOS / arch detection                              │
│    - Fetch latest.json → render version + notes          │
│    - Direct S3 download URL                              │
└──────────────────────────────────────────────────────────┘
                       ↑
                       │ download / email invite
                       │
┌──────────────────────────────────────────────────────────┐
│  5–10 Trial users                                        │
└──────────────────────────────────────────────────────────┘
```

**Key architecture properties:**

- **No backend.** S3 is pure static hosting. Manifest signing happens at release time on the founder's Mac.
- **Two repos, two PRs.** Tauri app changes go in `jarvis-ai-assistant`. Website changes in `jarvis-official-website`. Per root `CLAUDE.md`, never combine.
- **Q2-ready migration path.** Adding Cognito + Stripe later is purely additive — auth gate sits in front of the existing app, S3 bucket switches to private + presigned URLs without changing the updater contract.

---

## 4. Components

### 4.1 First-Run Wizard (NEW)

**Purpose:** Guide a fresh user from DMG install to working chat in ~3 minutes without the founder physically present.

**Files:**
- `src/pages/OnboardingWizard.tsx`
- `src-tauri/src/commands/onboarding.rs` — `check_onboarding_status`, `mark_onboarding_complete`
- `src-tauri/migrations/V12__onboarding.sql` — seeds `user_preferences` with `onboarding_complete='false'`

**Steps:**
1. **Local LLM** — reuse existing `LocalAiSetup` (RAM check, Ollama install link, model selection, pull progress). On success, set `default_provider='local'` in `user_preferences`. Skip allowed.
2. **BYOK (optional)** — three input fields (OpenAI / Claude / Gemini). Stored in `user_preferences`. All optional.
3. **Google OAuth** — invoke existing `cmd_google_login`. Skip allowed.
4. **Welcome** — confirmation screen + "Start" button. Calls `mark_onboarding_complete`, routes to Dashboard.

**Routing:** `src/App.tsx` queries `check_onboarding_status` on mount. `false` → mount `<OnboardingWizard />`. `true` → existing routing.

### 4.2 Auto-Updater (NEW)

**Purpose:** Founder ships v0.1.1 → trial users' apps prompt to install on next launch.

**Files:**
- `src-tauri/Cargo.toml` — add `tauri-plugin-updater`
- `src-tauri/tauri.conf.json` — endpoint, public key
- `src-tauri/src/lib.rs` — register plugin, check on startup
- `src/components/UpdatePrompt.tsx` — modal with "Install / Later"

**Manifest format (`latest.json`):**
```json
{
  "version": "0.1.1",
  "notes": "Bug fixes for Gmail rules and Calendar tz",
  "pub_date": "2026-05-15T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<base64 ed25519>",
      "url": "https://jarvis-releases.s3.ap-east-1.amazonaws.com/JARVIS_0.1.1_aarch64.dmg"
    }
  }
}
```

**Two key types — must not be confused:**
- **Apple Developer ID cert** → codesign DMG so macOS Gatekeeper trusts it.
- **Tauri ed25519 signer key** → signs updater payload so the running app trusts the new DMG.

Public ed25519 key committed in `tauri.conf.json`. Private key + Apple cert live in macOS Keychain only, never in git.

### 4.3 Commit Existing Launch Code (CLEANUP)

**Already written, currently uncommitted on `feature/local-model-implementation`:**

```
NEW (untracked):
  src-tauri/migrations/V9__user_memory.sql
  src-tauri/migrations/V10__provider_chain_allow_bedrock.sql
  src-tauri/migrations/V11__beta_launch.sql
  src-tauri/src/ai/bedrock.rs
  src-tauri/src/commands/launch.rs
  src/pages/LaunchPage.tsx

MODIFIED:
  src-tauri/Cargo.{toml,lock}
  src-tauri/src/{lib.rs, commands/mod.rs, ai/{mod.rs, claude.rs, gemini.rs, openai.rs, tools.rs, local/mod.rs}}
  src-tauri/src/integrations/gmail.rs
  src/{App.tsx, lib/{commands.ts, types.ts}, pages/Settings.tsx, components/Sidebar.tsx, ChatMessage.tsx, chat/MessageRenderer.tsx}
```

**Action:** One commit `feat: beta launch dashboard + bedrock provider chain` on existing `feature/local-model-implementation` branch, before starting Phase 0 audit. Otherwise the audit + onboarding diff will tangle with existing in-flight work.

### 4.4 Release Script (NEW)

**File:** `scripts/release.sh` (root of `jarvis-ai-assistant` repo)

**Steps:**
1. Bump version in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` (sed in-place, three files in sync).
2. `npm run tauri build --target aarch64-apple-darwin`.
3. `codesign --deep --force --sign "Developer ID Application: <name>" --options runtime *.app`.
4. `xcrun notarytool submit *.dmg --apple-id $APPLE_ID --team-id $APPLE_TEAM_ID --password $APPLE_APP_PASSWORD --wait`.
5. `xcrun stapler staple *.dmg`.
6. `cargo tauri signer sign *.dmg` → produces `*.dmg.sig`.
7. **Confirm step** — open the DMG, drag `.app` to Applications, launch it, then `release.sh` prompts: "Did the local install start cleanly? [y/N]". Wording matches the data-flow diagram in §5.3.
8. `aws s3 cp *.dmg s3://jarvis-releases/ --profile jarvis`.
9. `aws s3 cp *.dmg.sig s3://jarvis-releases/`.
10. Generate `latest.json` from a `jq` template, then `aws s3 cp latest.json s3://jarvis-releases/ --cache-control "max-age=60"`.
11. Print download URL.

**Required env / files:**
- `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` — Apple App-specific password.
- `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — generated once via `npm run tauri signer generate`.
- AWS profile `jarvis` in `~/.aws/credentials`.

**Atomic guarantee:** `latest.json` always uploaded last. If any earlier step fails, existing trial users still see the previous valid manifest.

### 4.5 AWS S3 + IAM Setup Script (NEW)

**File:** `scripts/aws-setup.sh` (root of `jarvis-ai-assistant` repo). Idempotent. Not CDK — single bucket doesn't justify it.

**Resources:**
- S3 bucket `jarvis-releases` in `ap-east-1`.
- Bucket policy: public-read on `*.dmg`, `*.dmg.sig`, `latest.json`.
- CORS: allow GET from `https://jarvis-ai.com` and `http://localhost:3000`.
- IAM user `jarvis-release-bot` with `PutObject` on the bucket only. Access key stored in `~/.aws/credentials [jarvis]`.
- S3 Lifecycle rule: keep 5 latest versions, delete versions older than 90 days.

**Cost ceiling:** 30MB DMG × 100 downloads/month ≈ 3GB egress ≈ $0.27/month. $200 credit lasts 60+ years at this volume.

### 4.6 Website Download Page (NEW)

**Files (jarvis-official-website repo):**
- `app/[locale]/download/page.tsx`
- `lib/download.ts` — client-side fetch + parse `latest.json`
- `content/en/download.ts`, `content/zh-HK/download.ts`

**Behavior:**
- SSR renders hero + generic "Download for macOS" CTA.
- Client-side `useEffect` fetches `latest.json`, displays version + release notes.
- Platform detection — **simplified pragmatic approach** (`navigator.userAgent` cannot reliably distinguish Apple Silicon from Intel on macOS in Safari; User-Agent Client Hints only work in Chromium):
  - macOS detected → primary CTA shows the aarch64 DMG with explicit label "Requires macOS 12+ on Apple Silicon (M1 or later)".
  - Non-macOS → "macOS only — Windows / Linux on roadmap. Join the waitlist."
  - Intel users will read the label or hit Gatekeeper / Rosetta and email the founder. Acceptable for a 5-user trial; revisit in Q2.
- Static fallback if `latest.json` fetch fails: hardcoded link to last-known-good DMG version. Page never breaks.
- Add to `sitemap.ts`. Add OG image. Add footer link from homepage.

### 4.7 Triage Audit (Phase 0 — workflow, not component)

**Output deliverable:** `jarvis-ai-assistant/docs/superpowers/specs/2026-05-04-trial-audit-results.md`. Committed before Phase 1 starts.

**Audit scope (one row per feature):**

| Feature group | Items |
|---|---|
| Chat providers | Local LLM (Ollama models), OpenAI, Claude, Gemini, Bedrock |
| Gmail | list, read, send, search, rules |
| Calendar | list, create, update, suggest_time |
| Notion | search, create, update |
| GitHub | PR list, issue list, basic actions |
| Obsidian | file ops |
| Voice | STT, TTS, wake-word, push-to-talk |
| Cron / scheduler | jobs, runs, retry |
| Smart assistant | action parser, function calling |
| 3D dashboard | render, interaction |

**Per-feature output columns:** `Feature | Status (pass/fix/hide) | Bug count | Action`.

**Audit protocol:** 5 minutes per feature, cold-start, single happy-path action. Document result, do not debug. Debugging happens in Phase 1.

**Estimated effort:** 1.5 working days end to end.

### 4.8 Email Invite Pipeline (USE EXISTING — no build)

The website already has Resend integration (per project `CLAUDE.md`). Manual workflow:
1. Existing waitlist form captures emails.
2. Founder picks 5–10 trial users (friends, mentors, hottest waitlist).
3. Sends Resend template: "JARVIS 0.1.0 beta — [download link]. Feedback via the in-app LaunchPage or reply to this email."

No automation. 5 users = manual is faster than build.

---

## 5. Data Flows

### 5.1 First-Run Experience

```
User opens DMG → Gatekeeper validates codesign + notarization → app launches
  → src/App.tsx mount → invoke('check_onboarding_status')
    → SELECT user_preferences WHERE key='onboarding_complete'
      → 'true'  → existing Dashboard route
      → 'false' / missing → mount <OnboardingWizard />
        → Step 1 (Local LLM): on success set default_provider='local'; on fail or skip, continue
        → Step 2 (BYOK): write keys to user_preferences if provided; skip OK
        → Step 3 (Google OAuth): trigger cmd_google_login; skip OK
        → Step 4 (Welcome) → mark_onboarding_complete → Dashboard
```

**Persistence:** All wizard state lives in `user_preferences`. Force-quitting the wizard leaves `onboarding_complete=false`, so next launch restarts at Step 1. This is the intended behavior.

### 5.2 Auto-Update Poll

```
App launch → useEffect → updater.check()
  → HTTPS GET https://jarvis-releases.s3.ap-east-1.amazonaws.com/latest.json
    → 200 + version > current → render <UpdatePrompt />
      → user clicks "Install" → updater.downloadAndInstall()
        → download DMG → verify ed25519 signature
          → pass → relaunch with new version
          → fail → abort install, log error, no fallback
      → user clicks "Later" → modal closes; re-prompts next launch
    → 200 + version == current → no-op
    → network fail / invalid JSON → silent log, app continues normally
```

**Cadence:** Once per app launch. No background polling.

### 5.3 Release Pipeline

```
Founder runs ./scripts/release.sh 0.1.1
  → bump version in 3 files
  → npm run tauri build --target aarch64-apple-darwin
  → codesign with Apple Developer ID
  → xcrun notarytool submit --wait (Apple server: 5–15 min)
    → rejected → exit, no S3 upload
    → accepted → continue
  → xcrun stapler staple
  → cargo tauri signer sign
  → CONFIRM: "Did the local install start cleanly?" [y/N]
    → N → exit, no S3 upload
    → y → continue
  → aws s3 cp DMG, .sig
  → aws s3 cp latest.json (LAST, with cache-control max-age=60)
  → print download URL
    → existing trial users see prompt on next launch
    → website /download page picks up new version on next visit
```

**Atomicity:** `latest.json` upload is the switch flag. Any failure before that step leaves all trial users on the previous valid release.

### 5.4 Feedback Loop

```
User opens LaunchPage in left sidebar → fills feedback form
  → invoke('submit_beta_feedback', input)
    → INSERT INTO beta_feedback (~/Library/Application Support/jarvis/jarvis.db)
      → STAYS LOCAL — founder cannot see remotely
```

**Founder's path to feedback:**
- Day-7 1-on-1 calls (primary).
- Trial user replies to Resend invite email (likeliest in practice).
- Trial user voluntarily exports + sends DB row (rare).

This is a deliberate trade-off: zero server, zero PII risk, zero cost — at the price of mandating active outreach. Aligns with mentor: "12 testers don't become 100 because of onboarding walls — solved by phone calls, not dashboards."

---

## 6. Error Handling Rules

### Cross-cutting design rules

1. **Skip + degrade > block + retry.** Any failed step's default action is "let the user keep using the previous behavior."
2. **`latest.json` is always last.** Atomic release switch. Partial uploads never expose users to a half-shipped version.
3. **Dogfood every release.** `release.sh` step 7 is a manual confirm. No skip.
4. **Signed > unsigned, always.** Any signature failure aborts; never fall back to unsigned. Both Apple cert and Tauri ed25519.
5. **5 users do not need nation-state defenses.** No rate limiting, WAF, or MFA. Public S3 + signed DMG is the entire perimeter.

### Specific failure handling

| Class | Failure | Mitigation |
|---|---|---|
| Onboarding | Local LLM pull fails | Show error + skip button; do not block wizard |
| Onboarding | OAuth fails | Toast + skip; can complete later in Settings |
| Onboarding | User force-quits | `onboarding_complete=false` persists; next launch restarts at Step 1 |
| Onboarding | `mark_onboarding_complete` SQLite write fails | Retry once; second failure shows "restart app, file a bug" |
| Update | `latest.json` network fail | Silent log; do not block app startup |
| Update | Bad JSON / corrupted manifest | Silent log; existing version continues |
| Update | Signature verification fails | Abort install, log, surface error toast — never fall back to unsigned |
| Update | DMG download interrupted | Plugin retries once; second failure prompts user to retry later |
| Update | New version startup-crashes | No automated rollback. User manually re-downloads previous version from website fallback link |
| Release | Notarization rejected | `release.sh` exits non-zero before S3 upload |
| Release | AWS credentials expired | `aws s3 cp` 401, `release.sh` halts before `latest.json` flip |
| Release | DMG uploaded but `latest.json` upload fails | Trial users still see previous version (atomic guarantee preserved) |
| Release | Bad DMG accidentally shipped | Manual rollback: edit `latest.json` to point at previous DMG, re-upload `latest.json`. Updater will not downgrade running clients |
| Migration | V12 schema bug | Caught in `npm run tauri dev` cycle pre-release |
| Migration | Migration fails on existing user DB | refinery transactional rollback; user's app fails to start; user reports via Resend reply |
| Schema | Need to change V11 after ship | Forward-only — only add V13+ to extend; never edit a shipped migration |
| Website | `latest.json` fetch fails | Static fallback link to last-known-good DMG version |
| Website | S3 CORS misconfigured | Caught by `scripts/aws-setup.sh`; manual repro via curl with Origin header |
| Website | User on Intel Mac | UA detection shows secondary message; primary CTA hidden |
| Lifecycle | Apple Developer Program membership (1-year) lapses → cannot issue or renew certs | Calendar reminder 30 days before annual renewal. Existing Developer ID certificate (5-year validity) keeps signing usable past lapse, but new certs require active membership |
| Lifecycle | Apple App-Specific Password rotated / expires | Regenerate at appleid.apple.com; update Keychain entry referenced by `release.sh` |
| Lifecycle | AWS access key leaks to git | `release.sh` greps staged diff for `AKIA*` patterns before any commit. `.gitignore` excludes `.aws/` and `*.key` files |

---

## 7. Testing Strategy

### 7.1 Automated unit tests (Rust only, scope-limited)

| Module | Tests |
|---|---|
| `commands/onboarding.rs` | `check_onboarding_status` returns correct value for fresh DB / `false` / `true` / corrupt DB |
| `commands/launch.rs` | duplicate email is upsert (no dup row); rating < 1 or > 5 rejected; message < 8 chars rejected |
| Migration V12 | After running, `user_preferences` contains `onboarding_complete='false'` |

### 7.1.a Manual signature-tamper verification (one-off)

Once during Phase 3 setup: corrupt a copy of `*.dmg.sig` (flip a byte), point the dev `latest.json` at the corrupted sig, run the updater. Confirm install aborts and surfaces an error. Document outcome in the implementation plan; do not re-run per release.

**Out of scope:** React component tests, E2E (Playwright/Cypress), load tests.

### 7.2 Pre-release manual smoke checklist

**File:** `scripts/pre-release-checklist.md`. Run before every `release.sh` invocation. ~10 minutes.

```
[ ] 1. rm -rf ~/Library/Application\ Support/jarvis
[ ] 2. npm run tauri dev → onboarding wizard mounts at Step 1
[ ] 3. Step 1: pull a small model (Qwen 0.5B), confirm progress
[ ] 4. Step 2: enter an OpenAI key
[ ] 5. Step 3: complete Google OAuth
[ ] 6. Step 4: click Start → land on Dashboard
[ ] 7. Chat: "list 3 unread emails" — confirm Gmail action runs
[ ] 8. Calendar: "show today's events"
[ ] 9. LaunchPage: submit dummy feedback — confirm SQLite write
[ ] 10. Restart app → onboarding does not re-trigger
[ ] 11. Settings → OpenAI key still present
[ ] 12. Run all "pass" features from audit (~10 min)
```

All 12 must pass before `release.sh` runs.

### 7.3 Release pipeline test (using dev bucket `jarvis-releases-dev`)

| Test | Verification |
|---|---|
| Version bump syncs three files | grep returns matching version in `package.json`, `Cargo.toml`, `tauri.conf.json` |
| Build produces DMG | `ls bundle/dmg/JARVIS_*.dmg`, size > 10MB |
| Codesign valid | `codesign --verify --verbose *.app` returns "valid on disk" |
| Notarization stapled | `xcrun stapler validate *.dmg` returns OK |
| Tauri signature exists | `*.dmg.sig` non-empty, base64 |
| S3 upload | `aws s3 ls s3://jarvis-releases-dev/` shows three artifacts |
| `latest.json` valid | `jq` extracts version, url, signature |
| End-to-end install | Download from dev bucket, install, open, wizard mounts |

Switch to prod `jarvis-releases` bucket only after dev pipeline is green.

### 7.4 Cross-Mac testing

- Founder's Mac (Apple Silicon): full dogfood.
- One trusted friend's Apple Silicon Mac: install + wizard, before mass invite. This validates the cert chain works outside the founder's keychain.
- Intel Mac: skipped for v0.1.x. Trial recipients filtered to Apple Silicon owners.
- Older macOS: the supported floor is **whatever the founder can actually test**. If only macOS 14 / 15 are testable, the download page label and `tauri.conf.json` `minimumSystemVersion` both state macOS 14+. Raising the promise above the tested floor is the failure mode to avoid.

### 7.5 Friends-as-beta protocol

**Day 0 per user:**
- Founder watches install + onboarding via screen-share or in-person.
- Notes every "got stuck" moment verbatim.

**Day 1–7:**
- No founder contact unless user reaches out.

**Day 7 per user:**
- 30-minute call. Three questions:
  1. Which feature did you use most?
  2. What broke or frustrated you?
  3. Would you pay HK$148/month? Why or why not?

This is the primary data the Q2 Market Validation Report will rest on. More valuable than any dashboard.

### 7.6 Out-of-scope tests

| Skipped | Reason |
|---|---|
| E2E (Playwright/Cypress) | Setup cost > value at 5 users |
| Load test | Trial max 5 concurrent |
| Penetration test | No inbound port exposure |
| A/B test | Sample too small |
| External crash reporting (Sentry) | User-rejected; phone calls cover the gap |

---

## 8. Implementation Phases

| Phase | Deliverable | Gate |
|---|---|---|
| **Phase 0: Audit** | `2026-05-04-trial-audit-results.md` committed | All features have a status row (`pass` / `fix` / `hide`) |
| **Phase 1: Cleanup** | Existing uncommitted work landed in one commit | Clean working tree |
| **Phase 2: Onboarding Wizard** | OnboardingWizard.tsx, V12 migration, onboarding.rs, App.tsx routing | Pre-release checklist items 1–6 pass |
| **Phase 3: Auto-update + Release** | tauri-plugin-updater wired, ed25519 keys generated, `scripts/release.sh` written, `scripts/aws-setup.sh` written, S3 bucket created | Dev bucket end-to-end install passes |
| **Phase 4: Hide / Fix** | Feature flags or Settings hide for `hide` items; patches for `fix` items | Pre-release checklist item 12 passes |
| **Phase 5: Website /download** | `/[locale]/download` page, latest.json fetch, UA detection, fallback | Page renders correct version on prod |
| **Phase 6: First production release** | `./scripts/release.sh 0.1.0` against prod bucket | Trusted friend installs from website, wizard works |
| **Phase 7: Trial wave** | 5–10 invite emails via Resend; founder watches each install Day 0 | All 5 users complete onboarding without founder intervention |
| **Phase 8: Bug-fix cycle** | One or more `0.1.x` releases driven by trial feedback | All trial users on latest within 24h via auto-update |
| **Phase 9: Day-7 calls** | 1-on-1 30-min calls per user, notes filed | Notes inform Q2 Market Validation |

Phase ordering respects the root `CLAUDE.md` git boundary rule: every Phase 5 commit lands in `jarvis-official-website`; everything else lands in `jarvis-ai-assistant`.

---

## 9. Out of Scope (intentionally deferred)

| Item | Deferred to | Reason |
|---|---|---|
| AWS Cognito user accounts | Q2 (after Closed Beta validates WTP) | No paying users yet means no auth needed |
| Stripe Checkout / Customer Portal | Q2 | Same |
| Feature gating (Free / Pro / Team) | Q2 | Same |
| S3 + KMS encrypted cross-device sync | Q2/Q3 | Privacy moat, but optional and complex |
| Intel Mac builds | Q2 stable wave | Bandwidth: ship one arch first |
| GitHub Actions CI | Post-stable | Solo dev + Keychain-bound cert makes local script faster |
| Sentry crash reporting | Q2 if data shows phone calls miss things | User-rejected for now |
| PostHog product analytics | Never (or far future) | 5 users → call them; 50 users → reconsider |
| Auto-rollback on crash | If trial reveals it's needed | Manual rollback path documented |
| Server-side feedback collection | Post-trial | Mandates founder to call, which is the right behavior at this stage |

---

## 10. Open Questions / Risks

1. **Audit may surface a feature that is broken but core to the demo.** If e.g. Gmail send is broken, Phase 4 patch could blow Phase 1 timeline. Mitigation: Phase 0 audit completes before any Phase 4 commitment; if a `pass`-rate falls below ~60%, scope narrows further (e.g., disable Calendar entirely, ship Gmail-only).
2. **Apple notarization can take longer than 15 min during peak hours.** Phase 6 first release should not be on a Friday afternoon HK time.
3. **One trial user per Mac architecture.** If the trusted friend's Apple Silicon test passes but a different friend has a corp-managed machine that blocks Gatekeeper exceptions, that user is unreachable for v0.1.x. Acceptable trade-off; note in invite email "personal Macs only."
4. **Trial user friction with Local LLM.** If 4 of 5 users have <16GB RAM Macs, Local LLM step is unusable, BYOK becomes mandatory, and the privacy moat narrative is undermined for the trial. Mitigation: pre-select trial users with adequate hardware. Long-term: smaller-footprint models (Qwen 0.5B already in audit list).
5. **Founder bandwidth in May 2026.** HKSTP cohort starts 2026-06-01. Phase 0–9 must complete before then or roll into Q1 Silver Badge timeline. Implementation plan will set explicit calendar gates.

---

## 11. Source Documents Referenced

- [`docs/business/BUSINESS_ROADMAP.md`](../../business/BUSINESS_ROADMAP.md) — Q1 priorities (T1.1, T1.5, T1.6).
- [`docs/business/MENTORSHIP_NOTES.md`](../../business/MENTORSHIP_NOTES.md) — "Q1 完全停低 Ship Feature", "Distribution 提早到 Q1", "唔好 over-engineer".
- [`CLAUDE.md`](../../../CLAUDE.md) — privacy-first principle, AWS architecture decisions, unit economics constraints.
- Root workspace `CLAUDE.md` (at `/Users/hillmanchan/Desktop/jarvis-projects/CLAUDE.md`, not under version control) — git boundary rules between the two repos.

---

## 12. Next Step

Once this design is approved, the writing-plans skill produces a phase-by-phase implementation plan with concrete commits, test gates, and review checkpoints. The plan lives at `docs/superpowers/plans/2026-05-03-jarvis-trial-launch.md`.
