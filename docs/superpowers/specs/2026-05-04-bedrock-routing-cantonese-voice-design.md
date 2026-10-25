# Bedrock Smart Routing + Cantonese Voice Path Design

**Date:** 2026-05-04
**Status:** Draft (ready for review)
**Scope:** Upgrade the AWS Bedrock provider from a hardcoded Sonnet 4.6 client to a workload-routed, prompt-cached, Singapore-CRIS production system. In parallel, unblock the voice pipeline for Cantonese end-to-end (STT, system-ack TTS, content TTS, optional offline TTS) using best-in-class May-2026 vendors.

---

## 1. Goals

- **Goal:** Make Bedrock the credible production inference layer the AWS Idea Launcher application promised — not a hardcoded fallback. Smart-route by workload (Nova Micro / Haiku 4.5 / Sonnet 4.6) so blended inference cost lands at US$1.20–1.50/user/month after caching.
- **Goal:** Make JARVIS the only Mac AI assistant that speaks production-grade Hong Kong Cantonese end-to-end. Three TTS tiers: free local (Sin-ji) for system acks, best-in-market cloud (MiniMax Speech-02-HD) for content, and Apple-Silicon offline (CosyVoice 2 via MLX) as a privacy-mode differentiator.
- **Goal:** Ship one demo-able feature combining both — "Daily Cantonese Inbox Briefing" — for AM check-in and Threads build-in-public.
- **Non-goal:** Custom Cantonese wake word. "Hey Jarvis" / "Hi Jarvis" English wake phrases stay as-is. Picovoice Porcupine (US$6K/year) is out of budget pre-revenue.
- **Non-goal:** AWS Polly Hiujin (May-2026 quality lags MiniMax / Chirp 3, would degrade demo).
- **Non-goal:** AWS Transcribe Cantonese (yue-HK WER ~17% vs Whisper ~8%; no custom vocabulary support).
- **Non-goal:** Replacing Whisper STT. Local Whisper stays primary, just unhardcoded.
- **Roadmap alignment:** T1.1 (AWS Cloud Control Plane), T1.3 (Local-First Inference confirmed), Mentor "Privacy moat = local-first by default, cloud opt-in only", Pitch Q9 commitment ("Bedrock for AI inference reduces APAC latency from 800ms to under 200ms — critical for voice").
- **Success criteria:**
  1. Bedrock provider routes by `Workload` enum: `Volume` → Nova Micro, `Tools` (default) → Haiku 4.5 Singapore CRIS, `Premium` → Sonnet 4.6.
  2. Prompt caching cuts repeat-turn input cost by ≥80% on the system-prompt + tool-schema block (verified via Bedrock CloudWatch metrics).
  3. End-to-end Cantonese voice loop: hotkey → Whisper (yue) STT → Haiku 4.5 (Cantonese reasoning) → MiniMax (Cantonese TTS) → audio first byte ≤ 800ms perceived from Hong Kong.
  4. Settings UI lets a non-technical user enter AWS access key + secret + region, MiniMax API key, and toggle Language = English / Cantonese / Bilingual.
  5. Daily Cantonese Inbox Briefing cron runs at user-set time (default 08:00 HKT), produces a 5-bullet Cantonese summary with 3 suggested tool actions; per-user-per-day cost ≤ US$0.01.
  6. CosyVoice 2 offline mode: when toggled, all TTS routes locally, no cloud egress for voice. Verified by network capture during a 60-second session.

---

## 2. Decisions Locked

| Decision | Choice | Reason |
|---|---|---|
| Bedrock endpoint | **`ap-southeast-1` (Singapore) endpoint, model IDs use Cross-Region Inference profiles** | SDK calls go to ap-southeast-1 (~70ms RTT from HK vs ~220ms from us-east-1). Anthropic Claude 4.5/4.6 are only available as `global.` CRIS profiles (verified May 2026 via `aws bedrock list-inference-profiles --region ap-southeast-1`). Nova Micro available as `apac.` regional profile. |
| Default workload model | **Claude Haiku 4.5** (Bedrock Singapore CRIS) | Only sub-US$5-output-token model that reliably handles 4–6 step tool chains. Strong Cantonese quality (Anthropic frontier-tier). |
| Volume model | **Amazon Nova Micro** (US$0.035 in / US$0.14 out per 1M tokens) | ~28× cheaper than Haiku for English email triage / classification. Production-ready for extractive tasks. NOT used for Cantonese (quality drop). |
| Premium model | **Claude Sonnet 4.6** | Reserved for explicit user "think harder" flag (new UI toggle) or after Haiku tool-use fails twice in a row. |
| Workload assignment | **Explicit per call site, no auto-inference in v1** | Cron jobs hardcode `Workload::Volume`; chat/voice default to `Workload::Tools`; "think harder" UI toggle sets `Workload::Premium`. Predictable cost per call. Auto-inference deferred. |
| Bedrock API | **Converse / ConverseStream** | Provider-agnostic across Claude/Nova. Native tool use. Streaming tool support since SDK v1.50+. Avoid InvokeModel per-vendor JSON shapes. |
| Prompt caching | **`cachePoint` block on system prompt + tool schema (5-min TTL)** | 90% discount on cache reads (Anthropic Bedrock confirmed). Mandatory for unit economics. |
| AWS credential storage | **Encrypted at rest in `aws_credentials` table; in-memory after unlock** | Replaces .env-only. User-facing Settings form. Same pattern as Claude/OpenAI/Gemini API keys (see `reload_api_keys_from_db` ai/mod.rs:115–134). |
| STT primary | **whisper.cpp local, language hint from user pref (`yue` / `en` / auto)** | Already shipped local. Just unhardcode `set_language(Some("en"))` at transcribe.rs:145. Cantonese WER ~8% (FLEURS yue_hant_hk). |
| STT fallback | **Gemini API with language-aware prompt** | Already shipped. Update prompt at transcribe.rs:42 to include language hint. |
| Wake word | **No change** — "Hey Jarvis" / "Hi Jarvis" English fuzzy match | User confirmed. Skip Porcupine. Cantonese speakers can use push-to-talk hotkey (Cmd+Shift+Space) as alternative. |
| TTS — system ack | **macOS `say -v Sin-ji`** for Cantonese acks, `say -v Samantha` for English | Free, local, instant. "已完成" / "已安排" tier. |
| TTS — content (cloud) | **MiniMax Speech-02-HD** (US$50–100/1M chars, ~HK$0.02 per 200-char reply) | Best Cantonese TTS in May 2026. Native HK pronunciation, streaming, sub-300ms first audio from HK. PRC vendor — flagged in §10. |
| TTS — content (offline) | **CosyVoice 2 via MLX** (Apache 2.0, ~600MB model) | Best open-source HK Cantonese. Real-time on M3/M4. Genuine differentiator: "the only desktop AI assistant that speaks Cantonese fully offline." |
| CosyVoice model distribution | **Download-on-first-toggle**, not bundled | Keeps DMG small. User opts in via Settings → Privacy → "Offline voice mode". Background download with progress UI. |
| Bilingual response logic | **System prompt receives `user_language` variable; LLM responds in user's preferred language** | Single source of truth in `user_preferences.language`. No prompt branching in Rust. |
| Demo feature | **Daily Cantonese Inbox Briefing cron** | Combines: routing (Nova classify → Haiku summarize), caching (system prompt cached across 100s of users), Cantonese (LLM + TTS), tool use (action suggestions). One feature shows the entire Bedrock + voice stack. |

---

## 3. Architecture

### 3.1 Bedrock routing flow

```
user message
    │
    ▼
src-tauri/src/commands/chat.rs::send_message
    │
    ▼
AiRouter::send(message, workload: Workload)
    │  Workload assignment in v1 is EXPLICIT, not inferred:
    │    - Cron jobs (daily_briefing classifier, email triage) pass Workload::Volume
    │    - Default chat / voice path passes Workload::Tools
    │    - UI "think harder" toggle (new) passes Workload::Premium
    │    - Auto-inference from message complexity is Phase 2 — keeps v1 routing predictable
    │
    ├─ Workload::Volume   → bedrock::send(model = nova-micro, no tool use)
    ├─ Workload::Tools    → bedrock::send(model = haiku-4.5,  tool use, cached system+tools)
    └─ Workload::Premium  → bedrock::send(model = sonnet-4.6, tool use, cached system+tools)

bedrock::send():
    1. Load AWS creds from in-memory cache (populated from aws_credentials table at startup)
    2. Build Converse request:
       - system: [{ text: SYSTEM_PROMPT, cachePoint: { type: "default" } }]
       - tools:  [{ ...tool_schema, cachePoint: { type: "default" } }]
       - messages: [..conversation_history, current_user_msg]
    3. Stream via ConverseStream
    4. Emit TTS chunks as text streams in
    5. Parse tool_use blocks → run tool → loop (max 3 iterations)
```

### 3.2 Voice pipeline flow (Cantonese mode)

```
hotkey (Cmd+Shift+Space) OR wake-word "Hey Jarvis"
    │
    ▼
voice/audio_capture.rs (cpal 16kHz mono)
    │
    ▼
voice/transcribe.rs::transcribe(audio, language: yue)
    │  language read from user_preferences.stt_language at call site
    │
    ├─ primary:  whisper.cpp large-v3-q5 with set_language(Some("yue"))
    └─ fallback: Gemini API with prompt "Transcribe this Cantonese audio to written Chinese"
    │
    ▼
text → AiRouter::send (Workload::Tools, language = yue)
    │  System prompt: "Respond in Hong Kong Cantonese (written 廣東話, not 書面語)."
    │
    ▼
Haiku 4.5 streams Cantonese response + tool calls
    │
    ▼
voice/tts_router.rs::speak(text, mode)
    │  mode = ack | content
    │  user_preferences.tts_offline = true|false
    │
    ├─ ack + cantonese     → say -v Sin-ji
    ├─ ack + english       → say -v Samantha
    ├─ content + online    → MiniMax Speech-02-HD (yue) streaming MP3
    └─ content + offline   → CosyVoice 2 MLX (yue) streaming PCM
```

---

## 4. Migrations

### 4.1 V13: AWS credentials + workload routing

```sql
-- src-tauri/migrations/V13__aws_credentials_and_routing.sql

CREATE TABLE IF NOT EXISTS aws_credentials (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    access_key_id TEXT NOT NULL,
    secret_access_key TEXT NOT NULL,   -- encrypted with app keychain key
    region TEXT NOT NULL DEFAULT 'ap-southeast-1',
    inference_profile TEXT NOT NULL DEFAULT 'apac',  -- prepended to model id for CRIS
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workload_routing (
    workload TEXT PRIMARY KEY CHECK (workload IN ('volume', 'tools', 'premium')),
    model_id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1
);

-- Defaults
INSERT OR IGNORE INTO workload_routing (workload, model_id) VALUES
    ('volume',  'apac.amazon.nova-micro-v1:0'),
    ('tools',   'global.anthropic.claude-haiku-4-5-20251001-v1:0'),
    ('premium', 'global.anthropic.claude-sonnet-4-6');
```

### 4.2 V14: Voice language preferences

```sql
-- src-tauri/migrations/V14__voice_language.sql

INSERT OR IGNORE INTO user_preferences (key, value) VALUES
    ('language',          'en'),       -- en | yue | bilingual
    ('stt_language',      'auto'),     -- auto | en | yue
    ('tts_voice_en',      'Samantha'),
    ('tts_voice_yue',     'Sin-ji'),
    ('tts_content_provider', 'sin-ji'),  -- sin-ji | minimax | cosyvoice
    ('tts_offline_mode',  '0'),        -- 0 | 1
    ('minimax_api_key',   ''),
    ('cosyvoice_model_path', ''),       -- populated after download
    ('briefing_enabled',  '0'),
    ('briefing_time_hkt', '08:00');
```

---

## 5. File-by-file changes

### 5.1 Backend (Rust)

| File | Change | Est. LOC |
|---|---|---|
| `src-tauri/migrations/V13__aws_credentials_and_routing.sql` | New | 35 |
| `src-tauri/migrations/V14__voice_language.sql` | New | 15 |
| `src-tauri/src/ai/bedrock.rs` | Replace InvokeModel path with Converse / ConverseStream + cachePoint blocks. Accept `model_id` and `Workload` parameters. Read creds from in-memory cache, not env. | ~180 (rewrite of 276) |
| `src-tauri/src/ai/mod.rs` | Add `Workload` enum. Extend `AiRouter::send` signature. Routing logic: lookup `workload_routing` table for model id. Heuristic for inferring workload from message context. | ~80 |
| `src-tauri/src/ai/tools.rs` | Restructure system prompt build: split into `cacheable_block()` (static system + tools) and `dynamic_suffix(user_language)`. Add language-conditional response rule. | ~30 |
| `src-tauri/src/commands/aws.rs` | New. `set_aws_credentials`, `get_aws_credentials_status`, `clear_aws_credentials` Tauri commands. Encrypt secret with macOS keychain-derived key (use existing `app_secrets` pattern if present, else add minimal AES-GCM wrapper). | ~120 |
| `src-tauri/src/voice/transcribe.rs` | Lines 42 + 145: read language from `user_preferences.stt_language`. Pass to Whisper `set_language` and Gemini prompt. | ~15 |
| `src-tauri/src/voice/tts.rs` | Replace single-voice path with `select_voice(language, mode)`. Default to Sin-ji when language=yue and mode=ack. | ~25 |
| `src-tauri/src/voice/tts_router.rs` | New. Routes between Sin-ji (ack), MiniMax (content/online), CosyVoice (content/offline). Reads `tts_content_provider` + `tts_offline_mode` from prefs. | ~80 |
| `src-tauri/src/voice/minimax.rs` | New. `reqwest` POST `https://api.minimaxi.chat/v1/t2a_v2` with Cantonese voice id. Stream MP3 chunks to existing audio playback path. | ~120 |
| `src-tauri/src/voice/cosyvoice.rs` | New. Subprocess wrapper around bundled `mlx-audio` Python venv (or Swift binding via `speech-swift`). First-call check: if model not present, emit download progress event. | ~150 |
| `src-tauri/src/voice/cosyvoice_download.rs` | New. Background download of CosyVoice2-0.5B from HuggingFace to `~/Library/Application Support/jarvis/models/cosyvoice2/`. Progress events to frontend. | ~80 |
| `src-tauri/src/voice/wake_word.rs` | No code change. Comment update only — confirm wake phrases stay English. | ~5 |
| `src-tauri/src/scheduler/daily_briefing.rs` | New. Cron at user-set HKT time. Pulls last 24h Gmail. Nova Micro classifies importance. Haiku 4.5 writes 5-bullet Cantonese summary + 3 action suggestions. Stores result in `daily_briefings` table for UI display. Speaks via TTS router if user opted in. | ~180 |
| `src-tauri/src/scheduler/mod.rs` | Register `daily_briefing` job. | ~15 |
| `src-tauri/src/db.rs` | Add `daily_briefings` table for briefing history (id, date, summary_md, actions_json, created_at). | ~25 |

### 5.2 Frontend (React + TypeScript)

| File | Change | Est. LOC |
|---|---|---|
| `src/pages/Settings.tsx` | Add three sections: (1) AWS Credentials form (access key / secret / region dropdown), (2) Language toggle (English / Cantonese / Bilingual) + STT language radio + per-language TTS voice picker, (3) Cantonese Voice section: MiniMax API key field + "Enable offline voice mode" toggle (triggers CosyVoice download with progress bar) + Daily Briefing toggle + time picker. | ~220 |
| `src/lib/commands.ts` | Add wrappers: `setAwsCredentials`, `getAwsCredentialsStatus`, `setLanguagePreference`, `setMinimaxApiKey`, `enableOfflineVoice`, `getCosyVoiceDownloadProgress`, `enableDailyBriefing`. | ~60 |
| `src/components/DailyBriefing.tsx` | New. Dashboard card showing today's briefing (markdown rendered, action buttons inline). | ~120 |
| `src/pages/Dashboard.tsx` | Mount `DailyBriefing` card above existing widgets when `briefing_enabled = 1`. | ~10 |
| `src/hooks/useVoiceState.ts` | Pass `language` from preferences when triggering voice flow. | ~10 |
| `src/lib/types.ts` | `Workload`, `AwsCredentials`, `LanguagePreference`, `BriefingResult` types. | ~30 |

**Total estimate:** ~1,580 LOC across 18 files (5 new Rust modules, 2 migrations, 4 new TS files).

---

## 6. Cost model

### 6.1 Per-user per-month inference (Pro tier, 100 active users)

Assumed daily usage:
- 200 emails → Nova Micro classify → ~2K in / 200 out tokens each
- 5 voice/text queries → Haiku 4.5 with cached system prompt → ~1K cache-read in / 500 out
- 1 daily briefing → Nova Micro classify (above) + Haiku 4.5 5-bullet Cantonese summary → ~3K cache-read in / 600 out
- 1 "think harder" Sonnet call per week → ~5K in / 1K out

Costs per user per month:
- Nova Micro: 200 × 30 × (2K × $0.035 + 200 × $0.14) / 1M = **US$0.59**
- Haiku 4.5 (with caching, 90% discount on cache reads): 5 × 30 × (1K × $0.10 + 500 × $5) / 1M = **US$0.39**
- Briefing Haiku: 1 × 30 × (3K × $0.10 + 600 × $5) / 1M = **US$0.10**
- Sonnet 4.6 weekly: 4 × (5K × $3 + 1K × $15) / 1M = **US$0.12**
- **Bedrock LLM total: ~US$1.20/user/month**

TTS cost (assuming 50% Cantonese voice users):
- MiniMax: 5 voice queries × 200 chars × 30 days = 30K chars/user/month × $0.08/1K = **US$2.40/user/month** (only for users on cloud Cantonese mode)
- Sin-ji + CosyVoice users: **US$0**

Combined:
- All-English Pro user: **~US$1.20/mo inference**
- Cantonese-cloud Pro user: **~US$3.60/mo inference**
- Cantonese-offline Pro user: **~US$1.20/mo inference**

Pro tier price: HK$148/mo ≈ US$19. Gross margin: **~80–94%** depending on TTS mode. Hits the unit economics target locked in BUSINESS_ROADMAP.md.

### 6.2 AWS Activate burn rate

US$5K credits ÷ US$1.20/user/month = ~4,200 user-months of LLM inference. At 100 active paying users, credits cover **~3.5 years** on paper. Reality: scale dilutes the cache hit rate and grows premium-tier usage. Realistic burn at 100–500 users: ~4–5 months. Short runway.

US$5K is the planning figure. As of 2026-10-25 no Activate credits have been granted; the account has only US$194 of Free Tier credit.

---

## 7. Demo deliverables

1. **30-second screen recording: "Daily Cantonese Inbox Briefing"** — JARVIS opens at 8am, reads 5-bullet summary in Cantonese (MiniMax voice), shows 3 action buttons. One click → action executes → JARVIS confirms in Cantonese ("已安排你 2 點同 Sarah 開會"). Posted to Threads as build-in-public.
2. **15-second screen recording: "Privacy Mode"** — Network monitor visible. User toggles "Offline voice mode" → CosyVoice downloads (progress bar) → user speaks Cantonese command → Cantonese response with zero outbound network traffic. Pinned tweet.
3. **One-page leave-behind PDF: "JARVIS AI Architecture v0.2"** — Bedrock routing diagram, prompt caching strategy, Cantonese stack, cost model. Send proactively to AM at first check-in.

---

## 8. Open questions

| # | Question | Owner |
|---|---|---|
| Q1 | MiniMax API key — does Hillman have an account? If not, sign-up + KYC time? | Hillman, Day 0 |
| Q2 | Sin-ji voice — does it require user to install via System Settings → Spoken Content → System Voice → Premium voices? Onboarding step needed? | Test on clean Mac, Day 8 |
| Q3 | CosyVoice 2 MLX runner — `mlx-audio` (Python) vs `speech-swift` (Swift)? Python easier to wire, Swift cleaner Tauri integration. | Spike Day 13 |
| Q4 | AWS credential encryption — is there an existing `app_secrets` pattern in the codebase, or do we need to add Keychain access via `security-framework` crate? | Audit Day 1 |
| Q5 | Cantonese system prompt phrasing — written 廣東話 (口語) or 書面語? Mentor preference? Hillman to provide example sentences. | Hillman, Day 11 |

---

## 9. Out of scope (Phase 2+)

- **Picovoice Porcupine custom Cantonese wake word** — defer until MRR > HK$15K, when US$6K/year is rounding error.
- **Per-user fine-tuned Cantonese voice cloning** (MiniMax 10s sample) — premium feature for Team tier.
- **AWS Polly Hiujin** — explicitly skipped; quality regression vs MiniMax.
- **AWS Transcribe Cantonese** — explicitly skipped; WER worse than Whisper.
- **HKGAI V1 / Votee Cantonese LLM** — no public API as of May 2026.
- **Mainland China region (`cn-north-1`)** — out of scope until cross-border GBA SME ICP is validated.
- **Mobile companion app** — full mobile is post-Q4.

---

## 10. Risks

| # | Risk | Likelihood | Mitigation |
|---|---|---|---|
| R1 | MiniMax API outage or PRC network throttling from HK | Low–Medium | Sin-ji + CosyVoice both available as fallback. TTS router catches MiniMax error and degrades gracefully. |
| R2 | Singapore CRIS profile model availability changes | Low | Cross-check `apac.anthropic.*` model id against [Bedrock region docs](https://docs.aws.amazon.com/bedrock/latest/userguide/models-region-compatibility.html) at start of work; pin model id in V11 migration; document fallback to Tokyo CRIS (`apne1.`). |
| R3 | CosyVoice 2 quality on M2 base too slow for streaming | Medium | Detect chip family at runtime; show "Offline mode requires M3 or later" if M2 base detected; fall back to Sin-ji + warn user. Acceptable degradation for Phase 1. |
| R4 | Prompt caching not actually saving 90% (cache hit rate < 50%) | Medium | Instrument Bedrock response metadata (`cacheReadInputTokens` / `cacheWriteInputTokens` in Converse response) → log to `inference_metrics` table → review weekly. If hit rate poor, restructure cache block boundaries. |
| R5 | Cantonese system prompt produces 書面語 (literary Chinese) instead of 廣東話 (spoken HK Cantonese) | Medium | Few-shot examples in system prompt with explicit "respond like a HK friend texting, not a Mainland news anchor". Validate with Hillman + 2 native HK testers in Day 13 demo. |
| R6 | AWS credentials in encrypted SQLite leak via backup / iCloud Drive sync | Low | Store in macOS Keychain via `security-framework` crate, not in SQLite. Only store opaque keychain reference in DB. (Decision pending Q4.) |
| R7 | Daily briefing cron generates briefings even when user is on holiday → email feels stale | Low | Skip days where Gmail returned 0 unread emails since last briefing. Resume on next inbox activity. |
| R8 | MiniMax PRC vendor surfaces in B2B / compliance pivot later | Medium (deferred) | Document in §9. If Q4 pivot to compliance ICP, swap to Google Chirp 3 HD `yue-HK` (US$30/1M chars). MiniMax client is isolated in `voice/minimax.rs` — single-file replacement. |

---

## 11. Sequencing (reference, not contractual)

User indicated speed will be faster than nominal 14-day estimate. Sequence is logical, not calendar-locked.

1. V11 migration + AWS credentials Settings form + encrypted storage
2. Bedrock client rewrite (Converse API + Singapore CRIS + cachePoint)
3. AiRouter Workload enum + routing logic + Nova Micro wired
4. Prompt caching instrumentation (verify 80%+ hit rate)
5. Daily briefing cron (skeleton, English first)
6. V12 migration + language preference + Settings language toggle
7. Whisper + Gemini language unhardcoding
8. TTS router with Sin-ji content path (system acks both languages)
9. Bilingual system prompt + frontend language pass-through
10. MiniMax client (cloud Cantonese content TTS)
11. CosyVoice 2 integration spike → choose `mlx-audio` vs `speech-swift`
12. CosyVoice download flow + offline-mode toggle
13. Daily briefing converted to Cantonese, end-to-end test
14. Demo recordings + leave-behind PDF + Threads post

---

## 12. References

- Pre-existing implementation audit: `src-tauri/src/ai/bedrock.rs:1-276`, `src-tauri/src/ai/mod.rs:225-440`, `src-tauri/migrations/V10__provider_chain_allow_bedrock.sql`, `src-tauri/src/voice/transcribe.rs:42,145`, `src-tauri/src/voice/tts.rs:195`, `src-tauri/src/voice/wake_word.rs:539-565`.
- Business roadmap unit economics: `docs/business/BUSINESS_ROADMAP.md` AWS Architecture section.
- Pitch commitment: `aws_startup_launcher/ptich_deck/JARVIS_Pitch_Preparation.pdf` Q9 ("Bedrock for AI inference reduces APAC latency from 800ms to under 200ms").
- AWS Bedrock Singapore CRIS: https://aws.amazon.com/blogs/machine-learning/global-cross-region-inference-for-latest-anthropic-claude-opus-sonnet-and-haiku-models-on-amazon-bedrock-in-thailand-malaysia-singapore-indonesia-and-taiwan/
- Bedrock prompt caching: https://docs.aws.amazon.com/bedrock/latest/userguide/prompt-caching.html
- Activate credits on Anthropic via Bedrock: https://aws.amazon.com/blogs/startups/aws-activate-credits-now-accepted-for-third-party-models-on-amazon-bedrock/
- MiniMax Speech-02-HD: https://www.minimax.io/news/speech-02-series, https://platform.minimax.io/docs/guides/pricing-speech
- CosyVoice 2: https://funaudiollm.github.io/cosyvoice2/, https://huggingface.co/FunAudioLLM/CosyVoice2-0.5B
- mlx-audio (Apple Silicon TTS runtime): https://github.com/Blaizzy/mlx-audio
- Whisper-large-v3 Cantonese model card: https://huggingface.co/openai/whisper-large-v3
