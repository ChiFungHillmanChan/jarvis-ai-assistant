# Bedrock Smart Routing + Cantonese Voice Path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade Bedrock from a hardcoded Sonnet 4.6 client to a Singapore-CRIS, workload-routed, prompt-cached production layer; ship end-to-end Cantonese voice (Sin-ji acks + MiniMax cloud content + CosyVoice 2 offline mode); deliver a Daily Cantonese Inbox Briefing demo that combines both.

**Architecture:** AiRouter dispatches by `Workload` enum (Volume/Tools/Premium) to Bedrock Singapore CRIS via `Converse` API with `cachePoint` blocks on system prompt + tool schema. Voice pipeline reads `language` from `user_preferences`, routes STT through Whisper (yue/en/auto), TTS through `tts_router` (Sin-ji ack / MiniMax cloud / CosyVoice offline). System prompt is bilingual via `user_language` template variable.

**Tech Stack:** Rust + Tauri v2, `aws-sdk-bedrockruntime 1.130.0` (Converse API), `reqwest` (MiniMax HTTPS), `mlx-audio` or `speech-swift` (CosyVoice 2 on Apple Silicon), SQLite + refinery migrations, React + TypeScript + Vite frontend, `security-framework` crate (macOS Keychain).

**Spec:** `docs/superpowers/specs/2026-05-04-bedrock-routing-cantonese-voice-design.md`

---

## Step 0: Preconditions

These must be done **before** Task A1. They are not code work; they are environmental setup that blocks the plan.

- [ ] **0.1 Sync git WIP and create feature branch**

  Current state: `feature/local-model-implementation` has 19 modified files, including `src-tauri/src/ai/{bedrock,mod}.rs`, `src-tauri/src/voice/tts.rs`, `src-tauri/src/commands/settings.rs`, `src/pages/Settings.tsx` — all files this plan touches.

  Commit or stash WIP first, then branch:
  ```bash
  cd /Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant
  git status                     # Confirm what's modified
  git stash push -m "WIP before bedrock-routing-cantonese-voice plan" \
    src-tauri/src/ai/bedrock.rs \
    src-tauri/src/ai/mod.rs \
    src-tauri/src/voice/tts.rs \
    src-tauri/src/commands/settings.rs \
    src/pages/Settings.tsx
  # Other unrelated WIP can stay or be stashed as user prefers
  git checkout -b feature/bedrock-routing-cantonese-voice
  git status                     # Confirm clean for the 5 files above
  ```

- [ ] **0.2 Create MiniMax account, get API key**

  Sign up at https://platform.minimax.io (international tier, NOT minimax.cn). Bind credit card. Generate API key from Dashboard → API Keys. Save aside; needed at Task B6.

- [ ] **0.3 Confirm Cantonese style choice**

  Plan defaults to **code-mix** (e.g., "Schedule 咗 2pm 同 Sarah 個 meeting"). If you want pure 廣東話 (口語) or pure 書面語, override at Task B4 step 4.2 by editing the system prompt template.

- [ ] **0.4 Verify AWS Bedrock Singapore CRIS access**

  ```bash
  aws bedrock list-foundation-models --region ap-southeast-1 \
    --query 'modelSummaries[?contains(modelId, `claude-haiku-4-5`)].[modelId]' \
    --output text
  ```
  Expected: `apac.anthropic.claude-haiku-4-5-v1:0` listed. If not, request model access in AWS Console → Bedrock → Model access → ap-southeast-1.

---

## Phase A — Bedrock Smart Routing

### Task A1: V11 migration — AWS credentials + workload routing

**Files:**
- Create: `src-tauri/migrations/V11__aws_credentials_and_routing.sql`
- Modify: none (refinery auto-discovers migrations)

- [ ] **A1.1 Write the migration SQL**

  Create `src-tauri/migrations/V11__aws_credentials_and_routing.sql`:
  ```sql
  -- AWS credentials (singleton row, secret encrypted by app keychain wrapper)
  CREATE TABLE IF NOT EXISTS aws_credentials (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      access_key_id TEXT NOT NULL,
      secret_access_key TEXT NOT NULL,
      region TEXT NOT NULL DEFAULT 'ap-southeast-1',
      inference_profile TEXT NOT NULL DEFAULT 'apac',
      updated_at INTEGER NOT NULL
  );

  -- Workload → model routing (3 fixed workload tiers)
  CREATE TABLE IF NOT EXISTS workload_routing (
      workload TEXT PRIMARY KEY CHECK (workload IN ('volume', 'tools', 'premium')),
      model_id TEXT NOT NULL,
      enabled INTEGER NOT NULL DEFAULT 1
  );

  INSERT OR IGNORE INTO workload_routing (workload, model_id) VALUES
      ('volume',  'amazon.nova-micro-v1:0'),
      ('tools',   'apac.anthropic.claude-haiku-4-5-v1:0'),
      ('premium', 'apac.anthropic.claude-sonnet-4-6-v1:0');

  -- Inference metrics for cache-hit verification (R4 mitigation)
  CREATE TABLE IF NOT EXISTS inference_metrics (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      ts INTEGER NOT NULL,
      workload TEXT NOT NULL,
      model_id TEXT NOT NULL,
      input_tokens INTEGER NOT NULL,
      output_tokens INTEGER NOT NULL,
      cache_read_tokens INTEGER NOT NULL DEFAULT 0,
      cache_write_tokens INTEGER NOT NULL DEFAULT 0,
      latency_ms INTEGER NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_inference_metrics_ts ON inference_metrics(ts);
  ```

- [ ] **A1.2 Run app to trigger migration**

  Run: `npm run tauri dev`
  Wait for app to launch. Check logs for `Migrations applied: V11`.

- [ ] **A1.3 Verify schema**

  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    ".schema aws_credentials workload_routing inference_metrics"
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "SELECT * FROM workload_routing;"
  ```
  Expected: 3 rows seeded (volume / tools / premium).

- [ ] **A1.4 Commit**

  ```bash
  git add src-tauri/migrations/V11__aws_credentials_and_routing.sql
  git commit -m "feat(db): V11 add aws_credentials, workload_routing, inference_metrics"
  ```

---

### Task A2: AWS credentials store with Keychain encryption

**Files:**
- Create: `src-tauri/src/security/keychain.rs`
- Modify: `src-tauri/src/lib.rs` (mount `security` module)
- Modify: `src-tauri/Cargo.toml` (add `security-framework = "2.11"`)
- Test: inline `#[cfg(test)] mod tests` in `keychain.rs`

- [ ] **A2.1 Add dependency**

  Edit `src-tauri/Cargo.toml`, add under `[dependencies]`:
  ```toml
  security-framework = "2.11"
  ```
  Run: `cd src-tauri && cargo build` to verify it resolves.

- [ ] **A2.2 Write the failing test**

  Create `src-tauri/src/security/keychain.rs`:
  ```rust
  pub struct KeychainStore;

  impl KeychainStore {
      pub fn put(service: &str, account: &str, secret: &str) -> Result<(), String> {
          unimplemented!()
      }
      pub fn get(service: &str, account: &str) -> Result<String, String> {
          unimplemented!()
      }
      pub fn delete(service: &str, account: &str) -> Result<(), String> {
          unimplemented!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      #[test]
      fn round_trip_secret() {
          let svc = "com.jarvis.test";
          let acct = "test-aws-secret";
          KeychainStore::put(svc, acct, "AKIA-TEST-SECRET").unwrap();
          let got = KeychainStore::get(svc, acct).unwrap();
          assert_eq!(got, "AKIA-TEST-SECRET");
          KeychainStore::delete(svc, acct).unwrap();
          assert!(KeychainStore::get(svc, acct).is_err());
      }
  }
  ```

- [ ] **A2.3 Run test to verify it fails**

  Run: `cd src-tauri && cargo test --lib security::keychain::tests::round_trip_secret -- --nocapture`
  Expected: PANIC `unimplemented!`.

- [ ] **A2.4 Implement minimal code to make the test pass**

  Replace the `KeychainStore` impl in `keychain.rs`:
  ```rust
  use security_framework::passwords;

  pub struct KeychainStore;

  impl KeychainStore {
      pub fn put(service: &str, account: &str, secret: &str) -> Result<(), String> {
          passwords::set_generic_password(service, account, secret.as_bytes())
              .map_err(|e| format!("keychain put: {e}"))
      }
      pub fn get(service: &str, account: &str) -> Result<String, String> {
          let bytes = passwords::get_generic_password(service, account)
              .map_err(|e| format!("keychain get: {e}"))?;
          String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
      }
      pub fn delete(service: &str, account: &str) -> Result<(), String> {
          passwords::delete_generic_password(service, account)
              .map_err(|e| format!("keychain delete: {e}"))
      }
  }
  ```

- [ ] **A2.5 Mount the module**

  Edit `src-tauri/src/lib.rs`, add near the other `mod` declarations:
  ```rust
  mod security;
  ```
  Create `src-tauri/src/security/mod.rs`:
  ```rust
  pub mod keychain;
  ```

- [ ] **A2.6 Run the test, expect PASS**

  Run: `cd src-tauri && cargo test --lib security::keychain::tests::round_trip_secret -- --nocapture`
  Expected: PASS. macOS may prompt to grant keychain access; click "Always Allow".

- [ ] **A2.7 Commit**

  ```bash
  git add src-tauri/Cargo.toml src-tauri/src/security/
  git commit -m "feat(security): add KeychainStore with security-framework backend"
  ```

---

### Task A3: AWS credentials repository + Tauri commands

**Files:**
- Create: `src-tauri/src/aws/credentials.rs`
- Create: `src-tauri/src/aws/mod.rs`
- Create: `src-tauri/src/commands/aws.rs`
- Modify: `src-tauri/src/lib.rs` (mount module + register commands)

- [ ] **A3.1 Write the failing test for the repository**

  Create `src-tauri/src/aws/credentials.rs`:
  ```rust
  use crate::db::Database;
  use std::sync::Arc;

  const KEYCHAIN_SERVICE: &str = "com.jarvis.aws";
  const KEYCHAIN_ACCOUNT: &str = "secret_access_key";

  #[derive(Clone, Debug)]
  pub struct AwsCredentials {
      pub access_key_id: String,
      pub secret_access_key: String,
      pub region: String,
      pub inference_profile: String,
  }

  pub struct AwsCredsRepo {
      db: Arc<Database>,
  }

  impl AwsCredsRepo {
      pub fn new(db: Arc<Database>) -> Self { Self { db } }
      pub async fn save(&self, _c: &AwsCredentials) -> Result<(), String> { unimplemented!() }
      pub async fn load(&self) -> Result<Option<AwsCredentials>, String> { unimplemented!() }
      pub async fn clear(&self) -> Result<(), String> { unimplemented!() }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::db::Database;

      async fn fresh_db() -> Arc<Database> {
          let path = std::env::temp_dir().join(format!("jarvis-test-{}.db", uuid::Uuid::new_v4()));
          Arc::new(Database::new(path.to_str().unwrap()).await.unwrap())
      }

      #[tokio::test]
      async fn save_and_load_round_trip() {
          let db = fresh_db().await;
          let repo = AwsCredsRepo::new(db);
          let creds = AwsCredentials {
              access_key_id: "AKIATEST".to_string(),
              secret_access_key: "SECRETTEST".to_string(),
              region: "ap-southeast-1".to_string(),
              inference_profile: "apac".to_string(),
          };
          repo.save(&creds).await.unwrap();
          let loaded = repo.load().await.unwrap().unwrap();
          assert_eq!(loaded.access_key_id, "AKIATEST");
          assert_eq!(loaded.secret_access_key, "SECRETTEST");
          assert_eq!(loaded.region, "ap-southeast-1");
          repo.clear().await.unwrap();
          assert!(repo.load().await.unwrap().is_none());
      }
  }
  ```

  Create `src-tauri/src/aws/mod.rs`:
  ```rust
  pub mod credentials;
  ```

  Mount in `src-tauri/src/lib.rs`:
  ```rust
  mod aws;
  ```

- [ ] **A3.2 Run test to verify it fails**

  Run: `cd src-tauri && cargo test --lib aws::credentials::tests::save_and_load_round_trip`
  Expected: PANIC `unimplemented!`.

- [ ] **A3.3 Implement `save`, `load`, `clear`**

  Replace the `impl AwsCredsRepo` in `credentials.rs`:
  ```rust
  use crate::security::keychain::KeychainStore;

  impl AwsCredsRepo {
      pub async fn save(&self, c: &AwsCredentials) -> Result<(), String> {
          KeychainStore::put(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &c.secret_access_key)?;
          let now = chrono::Utc::now().timestamp();
          let conn = self.db.conn().await;
          conn.execute(
              "INSERT INTO aws_credentials (id, access_key_id, secret_access_key, region, inference_profile, updated_at)
               VALUES (1, ?1, '__keychain__', ?2, ?3, ?4)
               ON CONFLICT(id) DO UPDATE SET
                 access_key_id = excluded.access_key_id,
                 region = excluded.region,
                 inference_profile = excluded.inference_profile,
                 updated_at = excluded.updated_at",
              rusqlite::params![&c.access_key_id, &c.region, &c.inference_profile, now],
          ).map_err(|e| format!("db save: {e}"))?;
          Ok(())
      }

      pub async fn load(&self) -> Result<Option<AwsCredentials>, String> {
          let conn = self.db.conn().await;
          let row = conn.query_row(
              "SELECT access_key_id, region, inference_profile FROM aws_credentials WHERE id = 1",
              [],
              |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
          );
          let (akid, region, profile) = match row {
              Ok(t) => t,
              Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
              Err(e) => return Err(format!("db load: {e}")),
          };
          let secret = KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?;
          Ok(Some(AwsCredentials {
              access_key_id: akid,
              secret_access_key: secret,
              region,
              inference_profile: profile,
          }))
      }

      pub async fn clear(&self) -> Result<(), String> {
          let conn = self.db.conn().await;
          conn.execute("DELETE FROM aws_credentials WHERE id = 1", [])
              .map_err(|e| format!("db clear: {e}"))?;
          let _ = KeychainStore::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
          Ok(())
      }
  }
  ```

- [ ] **A3.4 Run test, expect PASS**

  Run: `cd src-tauri && cargo test --lib aws::credentials::tests::save_and_load_round_trip`
  Expected: PASS.

- [ ] **A3.5 Add Tauri commands**

  Create `src-tauri/src/commands/aws.rs`:
  ```rust
  use crate::aws::credentials::{AwsCredentials, AwsCredsRepo};
  use crate::db::Database;
  use std::sync::Arc;
  use tauri::State;

  #[derive(serde::Serialize)]
  pub struct AwsCredsStatus {
      pub configured: bool,
      pub region: Option<String>,
  }

  #[tauri::command]
  pub async fn set_aws_credentials(
      access_key_id: String,
      secret_access_key: String,
      region: String,
      db: State<'_, Arc<Database>>,
  ) -> Result<(), String> {
      let creds = AwsCredentials {
          access_key_id,
          secret_access_key,
          region,
          inference_profile: "apac".to_string(),
      };
      AwsCredsRepo::new(db.inner().clone()).save(&creds).await
  }

  #[tauri::command]
  pub async fn get_aws_credentials_status(
      db: State<'_, Arc<Database>>,
  ) -> Result<AwsCredsStatus, String> {
      let loaded = AwsCredsRepo::new(db.inner().clone()).load().await?;
      Ok(match loaded {
          Some(c) => AwsCredsStatus { configured: true, region: Some(c.region) },
          None    => AwsCredsStatus { configured: false, region: None },
      })
  }

  #[tauri::command]
  pub async fn clear_aws_credentials(
      db: State<'_, Arc<Database>>,
  ) -> Result<(), String> {
      AwsCredsRepo::new(db.inner().clone()).clear().await
  }
  ```

- [ ] **A3.6 Register commands in `lib.rs`**

  In `src-tauri/src/lib.rs`, find the `tauri::Builder::default().invoke_handler(tauri::generate_handler![...])` block and add the three new commands:
  ```rust
  // existing handlers...
  commands::aws::set_aws_credentials,
  commands::aws::get_aws_credentials_status,
  commands::aws::clear_aws_credentials,
  ```
  Also ensure `mod commands { pub mod aws; ... }` includes the new file (check existing `commands/mod.rs` structure and follow the same pattern).

- [ ] **A3.7 Build and smoke test**

  Run: `npm run tauri dev`
  Open DevTools console, run:
  ```js
  await window.__TAURI__.core.invoke('set_aws_credentials', {
    accessKeyId: 'AKIA...', secretAccessKey: 'sk...', region: 'ap-southeast-1'
  });
  await window.__TAURI__.core.invoke('get_aws_credentials_status');
  // Expect: { configured: true, region: 'ap-southeast-1' }
  ```

- [ ] **A3.8 Commit**

  ```bash
  git add src-tauri/src/aws/ src-tauri/src/commands/aws.rs src-tauri/src/lib.rs src-tauri/src/commands/mod.rs
  git commit -m "feat(aws): add credentials repo + Tauri commands with Keychain encryption"
  ```

---

### Task A4: Settings UI for AWS credentials

**Files:**
- Modify: `src/lib/commands.ts` (add wrappers)
- Modify: `src/lib/types.ts` (add types)
- Modify: `src/pages/Settings.tsx` (add AWS section)

- [ ] **A4.1 Add TypeScript types**

  In `src/lib/types.ts`, append:
  ```ts
  export interface AwsCredsStatus {
    configured: boolean;
    region: string | null;
  }
  ```

- [ ] **A4.2 Add command wrappers**

  In `src/lib/commands.ts`, append:
  ```ts
  import { invoke } from "@tauri-apps/api/core";
  import type { AwsCredsStatus } from "./types";

  export async function setAwsCredentials(
    accessKeyId: string,
    secretAccessKey: string,
    region: string,
  ): Promise<void> {
    await invoke("set_aws_credentials", { accessKeyId, secretAccessKey, region });
  }
  export async function getAwsCredentialsStatus(): Promise<AwsCredsStatus> {
    return await invoke<AwsCredsStatus>("get_aws_credentials_status");
  }
  export async function clearAwsCredentials(): Promise<void> {
    await invoke("clear_aws_credentials");
  }
  ```

- [ ] **A4.3 Add AWS Credentials section to Settings page**

  In `src/pages/Settings.tsx`, find a place after the existing API key sections (Claude / OpenAI / Gemini). Insert:
  ```tsx
  // imports at top
  import {
    setAwsCredentials, getAwsCredentialsStatus, clearAwsCredentials,
  } from "../lib/commands";

  // inside component
  const [awsAccessKey, setAwsAccessKey] = useState("");
  const [awsSecret, setAwsSecret] = useState("");
  const [awsRegion, setAwsRegion] = useState("ap-southeast-1");
  const [awsStatus, setAwsStatus] = useState<{ configured: boolean; region: string | null }>(
    { configured: false, region: null }
  );

  useEffect(() => {
    getAwsCredentialsStatus().then(setAwsStatus).catch(console.error);
  }, []);

  // JSX section
  <section className="settings-section">
    <h2>AWS Bedrock</h2>
    <p className="hint">
      Used for Claude (Haiku 4.5 / Sonnet 4.6) and Amazon Nova models via the
      apac. inference profile (Singapore region).
      {awsStatus.configured && <> Currently configured for <code>{awsStatus.region}</code>.</>}
    </p>
    <label>Access Key ID
      <input value={awsAccessKey} onChange={e => setAwsAccessKey(e.target.value)} />
    </label>
    <label>Secret Access Key
      <input type="password" value={awsSecret} onChange={e => setAwsSecret(e.target.value)} />
    </label>
    <label>Region
      <select value={awsRegion} onChange={e => setAwsRegion(e.target.value)}>
        <option value="ap-southeast-1">Singapore (ap-southeast-1) — recommended</option>
        <option value="ap-northeast-1">Tokyo (ap-northeast-1) — fallback</option>
        <option value="us-east-1">US East (us-east-1) — high latency from HK</option>
      </select>
    </label>
    <div className="row">
      <button onClick={async () => {
        await setAwsCredentials(awsAccessKey, awsSecret, awsRegion);
        setAwsAccessKey(""); setAwsSecret("");
        setAwsStatus(await getAwsCredentialsStatus());
      }}>Save</button>
      {awsStatus.configured && (
        <button className="danger" onClick={async () => {
          await clearAwsCredentials();
          setAwsStatus({ configured: false, region: null });
        }}>Clear</button>
      )}
    </div>
  </section>
  ```

- [ ] **A4.4 Smoke test the UI**

  Run: `npm run tauri dev`
  Open Settings → AWS Bedrock. Enter test creds, save. Verify status flips to "Currently configured for ap-southeast-1". Verify keychain entry exists:
  ```bash
  security find-generic-password -s com.jarvis.aws -a secret_access_key
  ```
  Expected: returns a record (without printing the secret).

- [ ] **A4.5 Commit**

  ```bash
  git add src/lib/types.ts src/lib/commands.ts src/pages/Settings.tsx
  git commit -m "feat(ui): AWS Bedrock credentials section in Settings"
  ```

---

### Task A5: Workload enum + AiRouter routing

**Files:**
- Modify: `src-tauri/src/ai/mod.rs` (add `Workload` enum, extend `send`)
- Create: `src-tauri/src/ai/routing.rs`
- Test: inline in `routing.rs`

- [ ] **A5.1 Write the failing test**

  Create `src-tauri/src/ai/routing.rs`:
  ```rust
  use crate::db::Database;
  use std::sync::Arc;

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Workload {
      Volume,   // cron classifiers, email triage
      Tools,    // chat / voice (default)
      Premium,  // user-flagged "think harder"
  }

  pub struct WorkloadRouter {
      db: Arc<Database>,
  }

  impl WorkloadRouter {
      pub fn new(db: Arc<Database>) -> Self { Self { db } }
      pub async fn model_for(&self, _w: Workload) -> Result<String, String> {
          unimplemented!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::db::Database;

      async fn fresh_db() -> Arc<Database> {
          let path = std::env::temp_dir().join(format!("jarvis-test-{}.db", uuid::Uuid::new_v4()));
          Arc::new(Database::new(path.to_str().unwrap()).await.unwrap())
      }

      #[tokio::test]
      async fn defaults_match_v11_seed() {
          let db = fresh_db().await;
          let r = WorkloadRouter::new(db);
          assert_eq!(r.model_for(Workload::Volume).await.unwrap(), "amazon.nova-micro-v1:0");
          assert_eq!(r.model_for(Workload::Tools).await.unwrap(), "apac.anthropic.claude-haiku-4-5-v1:0");
          assert_eq!(r.model_for(Workload::Premium).await.unwrap(), "apac.anthropic.claude-sonnet-4-6-v1:0");
      }
  }
  ```

  Mount in `src-tauri/src/ai/mod.rs`:
  ```rust
  pub mod routing;
  pub use routing::Workload;
  ```

- [ ] **A5.2 Run test, expect FAIL**

  Run: `cd src-tauri && cargo test --lib ai::routing::tests::defaults_match_v11_seed`
  Expected: PANIC `unimplemented!`.

- [ ] **A5.3 Implement `model_for`**

  Replace the `impl WorkloadRouter` body in `routing.rs`:
  ```rust
  impl WorkloadRouter {
      pub fn new(db: Arc<Database>) -> Self { Self { db } }

      pub async fn model_for(&self, w: Workload) -> Result<String, String> {
          let key = match w {
              Workload::Volume  => "volume",
              Workload::Tools   => "tools",
              Workload::Premium => "premium",
          };
          let conn = self.db.conn().await;
          conn.query_row(
              "SELECT model_id FROM workload_routing WHERE workload = ?1 AND enabled = 1",
              rusqlite::params![key],
              |r| r.get::<_, String>(0),
          ).map_err(|e| format!("workload lookup: {e}"))
      }
  }
  ```

- [ ] **A5.4 Run test, expect PASS**

  Run: `cd src-tauri && cargo test --lib ai::routing::tests::defaults_match_v11_seed`
  Expected: PASS.

- [ ] **A5.5 Extend `AiRouter::send` signature**

  In `src-tauri/src/ai/mod.rs`, locate the `AiRouter::send` function (around line 225) and:
  1. Add a `workload: Workload` parameter (default to `Workload::Tools` at all existing call sites for now).
  2. When the chain entry is `provider_type == "bedrock"`, look up the model via `WorkloadRouter::model_for(workload)` and pass it into the `bedrock::send` call (signature change in Task A6).

  All existing callers in `src-tauri/src/commands/chat.rs` should pass `Workload::Tools` for now. Premium is wired in Task A8; Volume is wired in the daily briefing (Task A9).

- [ ] **A5.6 Verify the build still compiles**

  Run: `cd src-tauri && cargo build`
  Expected: clean build (deprecation warnings on bedrock signature ok — fixed in A6).

- [ ] **A5.7 Commit**

  ```bash
  git add src-tauri/src/ai/
  git commit -m "feat(ai): add Workload enum and WorkloadRouter for Bedrock model selection"
  ```

---

### Task A6: Bedrock client rewrite — Converse API + cachePoint

**Files:**
- Modify: `src-tauri/src/ai/bedrock.rs` (rewrite ~180 LOC)

The existing `bedrock::send` (276 LOC) uses a shape that's been working but does not use Converse, does not use prompt caching, and hardcodes the model id. We rewrite it.

- [ ] **A6.1 Read the existing implementation**

  Read `src-tauri/src/ai/bedrock.rs` end-to-end. Note: the existing tool-call loop, TTS chunk emission, and error handling patterns. Preserve those behaviors in the rewrite — only the request/response shape changes.

- [ ] **A6.2 Write a smoke-level integration test**

  At the bottom of `src-tauri/src/ai/bedrock.rs`, add:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      // Requires AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY in env, region ap-southeast-1
      #[tokio::test]
      #[ignore] // run manually: cargo test --lib ai::bedrock::tests::haiku_singapore_round_trip -- --ignored --nocapture
      async fn haiku_singapore_round_trip() {
          let creds = crate::aws::credentials::AwsCredentials {
              access_key_id: std::env::var("AWS_ACCESS_KEY_ID").unwrap(),
              secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
              region: "ap-southeast-1".to_string(),
              inference_profile: "apac".to_string(),
          };
          let resp = send_simple(
              &creds,
              "apac.anthropic.claude-haiku-4-5-v1:0",
              "Say only the word: pong",
          ).await.unwrap();
          assert!(resp.to_lowercase().contains("pong"), "got: {resp}");
      }
  }
  ```

- [ ] **A6.3 Run the test to verify it fails (before rewrite)**

  Run: `cd src-tauri && cargo test --lib ai::bedrock::tests::haiku_singapore_round_trip -- --ignored --nocapture`
  Expected: COMPILE ERROR (`send_simple` doesn't exist) or runtime error.

- [ ] **A6.4 Implement Converse-based client**

  Rewrite `src-tauri/src/ai/bedrock.rs`. Key elements (preserve existing tool-loop / streaming behavior, change request shape):
  ```rust
  use aws_config::{BehaviorVersion, Region};
  use aws_sdk_bedrockruntime::{
      Client,
      types::{
          ContentBlock, ConversationRole, ConverseStreamOutput, InferenceConfiguration,
          Message, SystemContentBlock, Tool, ToolConfiguration, ToolInputSchema,
          ToolSpecification, ToolUseBlock, ToolResultBlock, ToolResultContentBlock,
          CachePointBlock, CachePointType,
      },
      operation::converse_stream::ConverseStreamOutput as Stream,
  };
  use aws_sdk_bedrockruntime::primitives::Blob;
  use crate::aws::credentials::AwsCredentials;

  /// Build a Bedrock client from explicit credentials (do not rely on global env).
  async fn client_for(creds: &AwsCredentials) -> Client {
      let region = Region::new(creds.region.clone());
      let provider = aws_credential_types::Credentials::new(
          &creds.access_key_id,
          &creds.secret_access_key,
          None, None, "jarvis-aws-creds",
      );
      let cfg = aws_config::defaults(BehaviorVersion::latest())
          .region(region)
          .credentials_provider(provider)
          .load()
          .await;
      Client::new(&cfg)
  }

  /// Minimal one-shot helper used by the integration test in A6.2.
  pub async fn send_simple(
      creds: &AwsCredentials,
      model_id: &str,
      user_text: &str,
  ) -> Result<String, String> {
      let client = client_for(creds).await;
      let msg = Message::builder()
          .role(ConversationRole::User)
          .content(ContentBlock::Text(user_text.to_string()))
          .build()
          .map_err(|e| format!("msg build: {e}"))?;
      let out = client.converse()
          .model_id(model_id)
          .messages(msg)
          .send()
          .await
          .map_err(|e| format!("converse: {e}"))?;
      let blocks = out.output.unwrap().as_message().unwrap().content();
      let text = blocks.iter().filter_map(|b| match b {
          ContentBlock::Text(t) => Some(t.clone()),
          _ => None,
      }).collect::<Vec<_>>().join("");
      Ok(text)
  }

  /// Production entry. Replaces the prior pub fn send with Converse + cachePoint.
  pub async fn send(
      creds: &AwsCredentials,
      model_id: &str,
      system_prompt: &str,    // cacheable: static system block
      tool_schema: ToolConfiguration, // cacheable: static tool definitions
      messages: Vec<Message>, // dynamic: conversation history + current user msg
      app_handle: tauri::AppHandle,
      // ... preserve other params (TTS sender, max_iterations) from prior impl
  ) -> Result<String, String> {
      let client = client_for(creds).await;

      // System block with cachePoint to cache the static prefix
      let system = vec![
          SystemContentBlock::Text(system_prompt.to_string()),
          SystemContentBlock::CachePoint(
              CachePointBlock::builder()
                  .r#type(CachePointType::Default)
                  .build()
                  .map_err(|e| format!("cachepoint build: {e}"))?,
          ),
      ];

      // Tools come pre-built from caller; we add a cachePoint at the end of tool list
      // (Bedrock allows cachePoints in toolConfig as well — verify per current SDK docs)
      let tools = tool_schema; // already includes cachePoint per Task A7

      let inference = InferenceConfiguration::builder()
          .max_tokens(4096)
          .temperature(0.3)
          .build();

      // Multi-turn tool loop (max 3 iterations, mirroring previous behavior)
      let mut conv = messages;
      for _iter in 0..3 {
          let resp = client.converse_stream()
              .model_id(model_id)
              .set_system(Some(system.clone()))
              .set_messages(Some(conv.clone()))
              .tool_config(tools.clone())
              .inference_config(inference.clone())
              .send()
              .await
              .map_err(|e| format!("converse_stream: {e}"))?;

          // Stream chunks: emit text to TTS sender, accumulate full text, accumulate tool_use blocks
          let (text, tool_uses, usage) = consume_stream(resp.stream, &app_handle).await?;

          // Record metrics
          record_metrics(&conv, model_id, &usage).await;

          if tool_uses.is_empty() {
              return Ok(text);
          }

          // Append assistant message and tool results, loop
          conv.push(assistant_msg_from(text, &tool_uses)?);
          for tu in tool_uses {
              let result = run_tool(&tu).await;
              conv.push(tool_result_msg(&tu, result)?);
          }
      }
      Err("max tool iterations exceeded".into())
  }

  // helpers: consume_stream, record_metrics, assistant_msg_from, tool_result_msg, run_tool
  // (preserve exact signatures of equivalent helpers from prior bedrock.rs implementation)
  ```

  Notes:
  - The `consume_stream`, `run_tool`, `assistant_msg_from`, `tool_result_msg` helpers should be copied/adapted from the existing implementation — preserve their TTS-emit and tool-routing behavior.
  - `record_metrics` writes to the `inference_metrics` table created in V11 — implementation in A6.6.

- [ ] **A6.5 Update all callers in `ai/mod.rs`**

  Where `bedrock::send(...)` was previously called from the AiRouter, update the call to:
  1. Load `AwsCredentials` from `AwsCredsRepo` (cached on AiRouter struct after first load).
  2. Look up `model_id` via `WorkloadRouter::model_for(workload)`.
  3. Pass the cacheable system prompt + tool config from `tools::build_cacheable()` (A7).

- [ ] **A6.6 Implement `record_metrics`**

  In `src-tauri/src/ai/bedrock.rs`, add:
  ```rust
  async fn record_metrics(
      _conv: &[Message],
      model_id: &str,
      usage: &TokenUsage,
  ) {
      // Get DB handle from AppHandle managed state — pass &Database into send() if not already
      // Insert into inference_metrics table
      // Schema reminder: ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms
      // workload is unknown here — pass it down from caller if you want it; else default to "unknown"
  }
  ```
  Adjust `send` signature to thread `&Database` and `Workload` if needed. Keep this contained — metrics is observability, not feature logic.

- [ ] **A6.7 Run the integration test, expect PASS**

  Save valid AWS credentials in env, then:
  ```bash
  cd src-tauri && cargo test --lib ai::bedrock::tests::haiku_singapore_round_trip -- --ignored --nocapture
  ```
  Expected: PASS, output contains "pong". If `apac.anthropic.claude-haiku-4-5-v1:0` not available, model access not granted — go back to Step 0.4.

- [ ] **A6.8 Manual smoke: chat through the app**

  Run: `npm run tauri dev`
  Settings → AWS Bedrock → save credentials. Settings → Provider Chain → add Bedrock to chain. Send a chat message. Verify response streams in. Check `inference_metrics` table:
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "SELECT model_id, input_tokens, output_tokens, cache_read_tokens FROM inference_metrics ORDER BY ts DESC LIMIT 3;"
  ```
  Expected: rows for the model id used, with non-zero tokens.

- [ ] **A6.9 Commit**

  ```bash
  git add src-tauri/src/ai/bedrock.rs src-tauri/src/ai/mod.rs
  git commit -m "feat(bedrock): rewrite to Converse API with prompt caching, Singapore CRIS"
  ```

---

### Task A7: tools.rs — split system prompt into cacheable + dynamic

**Files:**
- Modify: `src-tauri/src/ai/tools.rs`

- [ ] **A7.1 Locate the existing `SYSTEM_PROMPT_BASE` and `build_system_prompt` (or equivalent) in `tools.rs`**

  Read the existing function. Identify:
  - Static portion: tool descriptions, behavior rules, output format conventions.
  - Dynamic portion: user name, today's date, language preference (added in B4), per-call context.

- [ ] **A7.2 Add `build_cacheable` and `dynamic_suffix` functions**

  Append to `tools.rs`:
  ```rust
  /// Returns the static portion of the system prompt that should be cached.
  /// Must be byte-stable across requests for cache hits.
  pub fn build_cacheable_system_prompt() -> String {
      // Move the static part of the existing prompt here.
      // No user name, no date, no per-request context.
      include_str!("system_prompt_static.md").to_string()
  }

  /// Returns the per-request dynamic suffix appended to the cached prefix.
  pub fn build_dynamic_suffix(
      user_name: &str,
      today_iso: &str,
      language: &str, // "en" | "yue" | "bilingual"  -- used by B4
  ) -> String {
      format!(
          "Current user: {user_name}\nToday: {today_iso}\nResponse language: {language}\n",
      )
  }

  /// Returns the cacheable Bedrock ToolConfiguration.
  pub fn build_cacheable_tool_config() -> aws_sdk_bedrockruntime::types::ToolConfiguration {
      // Move tool definitions here, build ToolConfiguration with cachePoint at end.
      todo_replace_with_real_impl()
  }
  ```

  Create `src-tauri/src/ai/system_prompt_static.md` and move the static prompt body in. (This file existing as a separate `.md` makes it easy to diff prompt changes and ensures the cache key is stable.)

- [ ] **A7.3 Add a stability test**

  In `tools.rs`:
  ```rust
  #[cfg(test)]
  mod prompt_tests {
      use super::*;

      #[test]
      fn cacheable_prompt_is_byte_stable() {
          let a = build_cacheable_system_prompt();
          let b = build_cacheable_system_prompt();
          assert_eq!(a, b, "cacheable prompt must be byte-stable for cache hits");
          assert!(a.len() > 500, "expected non-trivial static prompt");
      }
  }
  ```

- [ ] **A7.4 Run the test, expect PASS**

  Run: `cd src-tauri && cargo test --lib ai::tools::prompt_tests`
  Expected: PASS.

- [ ] **A7.5 Wire callers**

  In `bedrock::send` (Task A6), use:
  ```rust
  let system_prompt = tools::build_cacheable_system_prompt() + &tools::build_dynamic_suffix(...);
  // Note: cachePoint placement in SystemContentBlock vec means only the static prefix is cached.
  // The dynamic suffix should be a SEPARATE SystemContentBlock::Text AFTER the CachePoint:
  //   [Text(static), CachePoint, Text(dynamic_suffix)]
  ```
  Adjust the `system` vec in `bedrock::send` accordingly.

- [ ] **A7.6 Verify cache hits**

  Run the app, send 3 chat messages back-to-back. Check metrics:
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "SELECT cache_read_tokens, cache_write_tokens, input_tokens
     FROM inference_metrics ORDER BY ts DESC LIMIT 3;"
  ```
  Expected: First row has `cache_write_tokens > 0`, `cache_read_tokens = 0`. Subsequent rows have `cache_read_tokens > 0` (the static prefix is cached).
  If first-row cache_write is 0, the SystemContentBlock structure is wrong — the CachePoint must come AFTER at least one Text block.

- [ ] **A7.7 Commit**

  ```bash
  git add src-tauri/src/ai/tools.rs src-tauri/src/ai/system_prompt_static.md
  git commit -m "feat(ai): split system prompt into cacheable + dynamic sections"
  ```

---

### Task A8: "Think harder" UI flag → Workload::Premium

**Files:**
- Modify: `src/hooks/useChat.ts` (or equivalent — wherever chat send originates)
- Modify: `src/components/ChatComposer.tsx` (or equivalent)
- Modify: `src/lib/commands.ts` (extend send signature)
- Modify: `src-tauri/src/commands/chat.rs`

- [ ] **A8.1 Add `workload` parameter to `send_message` Tauri command**

  In `src-tauri/src/commands/chat.rs`, update `send_message`:
  ```rust
  #[tauri::command]
  pub async fn send_message(
      message: String,
      workload: Option<String>, // "tools" | "premium"; None defaults to tools
      // ...existing params
  ) -> Result<...> {
      let wl = match workload.as_deref() {
          Some("premium") => crate::ai::Workload::Premium,
          _ => crate::ai::Workload::Tools,
      };
      router.send(message, wl, ...).await
  }
  ```

- [ ] **A8.2 Add toggle to ChatComposer**

  In the chat composer component, add a "Think harder" toggle (checkbox or button). When enabled, pass `workload: "premium"` to the send command.

- [ ] **A8.3 Smoke test**

  Run app, open chat. Send a normal message — verify metrics row uses Haiku model id. Toggle "Think harder", send — verify metrics row uses Sonnet 4.6 model id.

- [ ] **A8.4 Commit**

  ```bash
  git add src-tauri/src/commands/chat.rs src/hooks/useChat.ts src/components/ChatComposer.tsx src/lib/commands.ts
  git commit -m "feat(chat): add 'Think harder' toggle to route to Sonnet 4.6"
  ```

---

### Task A9: Daily Briefing cron skeleton (English first)

**Files:**
- Create: `src-tauri/src/scheduler/daily_briefing.rs`
- Modify: `src-tauri/src/scheduler/mod.rs`
- Modify: `src-tauri/migrations/V11__aws_credentials_and_routing.sql` (add `daily_briefings` table — actually do this as V11.5 to keep V11 clean; or add a separate V11b migration)

Decision: instead of editing V11 (already committed), add a small follow-up migration here so V11 stays a clean checkpoint.

- [ ] **A9.1 Create migration V11b for briefing table**

  Create `src-tauri/migrations/V11b__daily_briefings.sql`:
  ```sql
  CREATE TABLE IF NOT EXISTS daily_briefings (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      date_hkt TEXT NOT NULL UNIQUE,    -- YYYY-MM-DD
      summary_md TEXT NOT NULL,
      actions_json TEXT NOT NULL,       -- JSON array of {label, tool, args}
      language TEXT NOT NULL DEFAULT 'en',
      created_at INTEGER NOT NULL
  );

  INSERT OR IGNORE INTO user_preferences (key, value) VALUES
      ('briefing_enabled',  '0'),
      ('briefing_time_hkt', '08:00');
  ```

- [ ] **A9.2 Run migration**

  `npm run tauri dev`. Verify schema:
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db ".schema daily_briefings"
  ```

- [ ] **A9.3 Implement briefing job (English MVP)**

  Create `src-tauri/src/scheduler/daily_briefing.rs`:
  ```rust
  use crate::ai::{AiRouter, Workload};
  use crate::db::Database;
  use crate::integrations::gmail;
  use std::sync::Arc;

  pub async fn run_briefing(
      db: Arc<Database>,
      router: Arc<tokio::sync::Mutex<AiRouter>>,
  ) -> Result<(), String> {
      let date = chrono::Utc::now()
          .with_timezone(&chrono_tz::Asia::Hong_Kong)
          .format("%Y-%m-%d").to_string();

      // Skip if already generated today
      {
          let conn = db.conn().await;
          let exists: bool = conn.query_row(
              "SELECT 1 FROM daily_briefings WHERE date_hkt = ?1",
              rusqlite::params![&date],
              |_| Ok(true),
          ).unwrap_or(false);
          if exists { return Ok(()); }
      }

      // 1. Fetch last 24h Gmail
      let messages = gmail::list_recent_messages(24).await
          .map_err(|e| format!("gmail fetch: {e}"))?;
      if messages.is_empty() { return Ok(()); } // R7 mitigation

      // 2. Nova Micro classify importance (low-cost batch)
      let classified = classify_with_nova(&messages, &router).await?;

      // 3. Haiku 4.5 write 5-bullet summary + 3 actions
      let (summary_md, actions_json) = summarize_with_haiku(&classified, &router, "en").await?;

      // 4. Persist
      let now = chrono::Utc::now().timestamp();
      let conn = db.conn().await;
      conn.execute(
          "INSERT INTO daily_briefings (date_hkt, summary_md, actions_json, language, created_at)
           VALUES (?1, ?2, ?3, 'en', ?4)",
          rusqlite::params![&date, &summary_md, &actions_json, now],
      ).map_err(|e| format!("db insert: {e}"))?;

      Ok(())
  }

  async fn classify_with_nova(
      messages: &[gmail::Message],
      router: &Arc<tokio::sync::Mutex<AiRouter>>,
  ) -> Result<Vec<(gmail::Message, String)>, String> {
      // Compose a single prompt: "Classify each email below as urgent|important|fyi|noise.
      // Return JSON array of {id, class}."
      // Send with Workload::Volume → routes to Nova Micro.
      // Parse JSON; pair classifications back to messages.
      todo!("implement: bulk classify via single Nova Micro call")
  }

  async fn summarize_with_haiku(
      classified: &[(gmail::Message, String)],
      router: &Arc<tokio::sync::Mutex<AiRouter>>,
      language: &str,
  ) -> Result<(String, String), String> {
      // Compose prompt: "Summarize the urgent/important emails as 5 bullet points
      // (markdown). Then suggest 3 concrete actions as JSON array
      // [{label, tool, args}]. Respond in {language}."
      // Send with Workload::Tools → Haiku 4.5 with cached system prompt.
      // Return (markdown_summary, actions_json_string).
      todo!("implement: single Haiku call returning summary + actions")
  }
  ```

  Replace `todo!()` with actual implementations using the existing AiRouter.send + result parsing patterns from the codebase. (Both functions are 30–60 LOC each; pattern follows `assistant/briefing.rs` if present.)

- [ ] **A9.4 Register the cron job**

  In `src-tauri/src/scheduler/mod.rs`, add registration at startup:
  ```rust
  use crate::scheduler::daily_briefing::run_briefing;

  pub async fn register_jobs(scheduler: &mut JobScheduler, db: Arc<Database>, router: Arc<Mutex<AiRouter>>) {
      // existing jobs...

      // Daily briefing — read briefing_enabled + briefing_time_hkt from prefs
      let enabled: String = read_pref(&db, "briefing_enabled").await.unwrap_or("0".into());
      if enabled == "1" {
          let time_hkt: String = read_pref(&db, "briefing_time_hkt").await.unwrap_or("08:00".into());
          let (h, m) = parse_hhmm(&time_hkt);
          // Convert HKT to UTC for cron expression
          let cron = format!("0 {m} {} * * *", (h + 24 - 8) % 24); // sec min hour ...
          let db2 = db.clone(); let router2 = router.clone();
          scheduler.add(Job::new_async(cron.as_str(), move |_, _| {
              let db = db2.clone(); let router = router2.clone();
              Box::pin(async move {
                  if let Err(e) = run_briefing(db, router).await {
                      eprintln!("briefing failed: {e}");
                  }
              })
          }).unwrap()).await.unwrap();
      }
  }
  ```

- [ ] **A9.5 Add a manual-trigger Tauri command for testing**

  In `src-tauri/src/commands/scheduler.rs` (create if missing):
  ```rust
  #[tauri::command]
  pub async fn trigger_daily_briefing(
      db: State<'_, Arc<Database>>,
      router: State<'_, Arc<Mutex<AiRouter>>>,
  ) -> Result<(), String> {
      crate::scheduler::daily_briefing::run_briefing(
          db.inner().clone(), router.inner().clone()
      ).await
  }
  ```
  Register in `lib.rs` invoke_handler.

- [ ] **A9.6 Smoke test**

  Run app. In DevTools:
  ```js
  await window.__TAURI__.core.invoke('trigger_daily_briefing');
  ```
  Verify:
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "SELECT date_hkt, language, length(summary_md), length(actions_json) FROM daily_briefings;"
  ```
  Expected: 1 row, language `en`, non-trivial lengths.

- [ ] **A9.7 Commit**

  ```bash
  git add src-tauri/migrations/V11b__daily_briefings.sql src-tauri/src/scheduler/daily_briefing.rs src-tauri/src/scheduler/mod.rs src-tauri/src/commands/scheduler.rs src-tauri/src/lib.rs src-tauri/src/commands/mod.rs
  git commit -m "feat(scheduler): daily briefing cron with Nova classify + Haiku summarize"
  ```

---

## CHECKPOINT 1 — Phase A complete

Before moving to Phase B, verify all of the following:

- [ ] `cargo test --lib` passes (all unit tests).
- [ ] App launches with no migration errors. `aws_credentials`, `workload_routing`, `inference_metrics`, `daily_briefings` tables exist.
- [ ] Settings → AWS Bedrock saves credentials. Keychain entry verified via `security find-generic-password -s com.jarvis.aws`.
- [ ] Chat with Bedrock provider returns text. `inference_metrics` row shows model `apac.anthropic.claude-haiku-4-5-v1:0`.
- [ ] Send 3 chat messages — second and third have `cache_read_tokens > 0` (≥80% of `input_tokens` ideally).
- [ ] `Think harder` toggle on chat composer — verify next message routes to Sonnet 4.6 (check metrics).
- [ ] Manual `trigger_daily_briefing` produces an English markdown summary + JSON actions in `daily_briefings`.

If any of these fail, fix before proceeding.

---

## Phase B — Cantonese Voice Path

### Task B1: V12 migration — voice language preferences

**Files:**
- Create: `src-tauri/migrations/V12__voice_language.sql`

- [ ] **B1.1 Write the migration SQL**

  Create `src-tauri/migrations/V12__voice_language.sql`:
  ```sql
  INSERT OR IGNORE INTO user_preferences (key, value) VALUES
      ('language',             'en'),       -- en | yue | bilingual
      ('stt_language',         'auto'),     -- auto | en | yue
      ('tts_voice_en',         'Samantha'),
      ('tts_voice_yue',        'Sin-ji'),
      ('tts_content_provider', 'sin-ji'),  -- sin-ji | minimax | cosyvoice
      ('tts_offline_mode',     '0'),        -- 0 | 1
      ('minimax_api_key',      ''),
      ('cosyvoice_model_path', '');         -- populated after download
  ```

- [ ] **B1.2 Run migration, verify**

  `npm run tauri dev`, then:
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "SELECT key, value FROM user_preferences WHERE key LIKE 'language' OR key LIKE 'stt_%' OR key LIKE 'tts_%' OR key LIKE 'minimax_%' OR key LIKE 'cosyvoice_%';"
  ```
  Expected: 8 rows.

- [ ] **B1.3 Commit**

  ```bash
  git add src-tauri/migrations/V12__voice_language.sql
  git commit -m "feat(db): V12 voice language preferences"
  ```

---

### Task B2: STT language unhardcoding

**Files:**
- Modify: `src-tauri/src/voice/transcribe.rs:42` (Gemini path)
- Modify: `src-tauri/src/voice/transcribe.rs:145` (local Whisper path)

- [ ] **B2.1 Add a helper to read language preference**

  In `src-tauri/src/voice/transcribe.rs`, add at top:
  ```rust
  async fn read_stt_language(db: &crate::db::Database) -> String {
      let conn = db.conn().await;
      conn.query_row(
          "SELECT value FROM user_preferences WHERE key = 'stt_language'",
          [], |r| r.get::<_, String>(0),
      ).unwrap_or_else(|_| "auto".into())
  }
  ```

- [ ] **B2.2 Pass language to Whisper**

  Find line 145 (`params.set_language(Some("en"))`) and change to:
  ```rust
  let lang = read_stt_language(&db).await;
  let whisper_lang = match lang.as_str() {
      "yue" => Some("yue"),
      "en"  => Some("en"),
      _     => None,    // auto-detect
  };
  params.set_language(whisper_lang);
  ```
  This requires `db: &Database` to be in scope at the call site — thread it through if needed.

- [ ] **B2.3 Pass language hint to Gemini**

  Find line 42 (Gemini STT prompt). Change to:
  ```rust
  let lang = read_stt_language(&db).await;
  let prompt = match lang.as_str() {
      "yue" => "Transcribe this Cantonese audio to written Chinese (use traditional characters and Hong Kong colloquial style 廣東話, NOT 書面語).",
      "en"  => "Transcribe this audio to text in English.",
      _     => "Transcribe this audio to text in the speaker's language (auto-detect English or Cantonese).",
  };
  ```

- [ ] **B2.4 Compile and smoke**

  Build: `cd src-tauri && cargo build`. Run app.
  In Settings (we'll add UI in B5; for now, manually flip preference):
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "UPDATE user_preferences SET value='yue' WHERE key='stt_language';"
  ```
  Restart app, hold push-to-talk, speak Cantonese ("你好，幫我睇下今日有咩 email"), release. Verify transcription is Chinese characters, not romanized English.

- [ ] **B2.5 Commit**

  ```bash
  git add src-tauri/src/voice/transcribe.rs
  git commit -m "feat(voice): unhardcode STT language, read from user_preferences"
  ```

---

### Task B3: TTS router with Sin-ji + Samantha selection

**Files:**
- Create: `src-tauri/src/voice/tts_router.rs`
- Modify: `src-tauri/src/voice/mod.rs` (mount tts_router)
- Modify: `src-tauri/src/voice/tts.rs` (split out routing logic)

- [ ] **B3.1 Define TTS modes and provider trait**

  Create `src-tauri/src/voice/tts_router.rs`:
  ```rust
  use crate::db::Database;
  use std::sync::Arc;

  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum TtsMode { Ack, Content }

  #[derive(Clone, Debug, PartialEq)]
  pub struct TtsRequest {
      pub text: String,
      pub mode: TtsMode,
      pub language: String,  // "en" | "yue"
  }

  #[derive(Clone, Debug, PartialEq)]
  pub enum TtsProvider {
      MacSay { voice: String },
      Minimax,
      CosyVoice,
  }

  pub struct TtsRouter {
      db: Arc<Database>,
  }

  impl TtsRouter {
      pub fn new(db: Arc<Database>) -> Self { Self { db } }

      pub async fn select_provider(&self, req: &TtsRequest) -> Result<TtsProvider, String> {
          let conn = self.db.conn().await;
          let read = |k: &str| -> String {
              conn.query_row(
                  "SELECT value FROM user_preferences WHERE key = ?1",
                  rusqlite::params![k], |r| r.get::<_, String>(0),
              ).unwrap_or_default()
          };
          // Acks always use macOS say (free, instant)
          if req.mode == TtsMode::Ack {
              let voice = match req.language.as_str() {
                  "yue" => read("tts_voice_yue"),
                  _     => read("tts_voice_en"),
              };
              let voice = if voice.is_empty() { "Samantha".into() } else { voice };
              return Ok(TtsProvider::MacSay { voice });
          }
          // Content respects offline mode + provider pref
          if read("tts_offline_mode") == "1" {
              return Ok(TtsProvider::CosyVoice);
          }
          match read("tts_content_provider").as_str() {
              "minimax"   => Ok(TtsProvider::Minimax),
              "cosyvoice" => Ok(TtsProvider::CosyVoice),
              _ => {
                  // fall through to sin-ji / samantha for content if no cloud configured
                  let voice = match req.language.as_str() {
                      "yue" => read("tts_voice_yue"),
                      _     => read("tts_voice_en"),
                  };
                  Ok(TtsProvider::MacSay { voice: if voice.is_empty() { "Samantha".into() } else { voice } })
              }
          }
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::db::Database;

      async fn fresh_db() -> Arc<Database> {
          let path = std::env::temp_dir().join(format!("jarvis-test-{}.db", uuid::Uuid::new_v4()));
          Arc::new(Database::new(path.to_str().unwrap()).await.unwrap())
      }

      #[tokio::test]
      async fn ack_yue_uses_sin_ji() {
          let db = fresh_db().await;
          let r = TtsRouter::new(db);
          let p = r.select_provider(&TtsRequest {
              text: "已完成".into(), mode: TtsMode::Ack, language: "yue".into(),
          }).await.unwrap();
          assert_eq!(p, TtsProvider::MacSay { voice: "Sin-ji".into() });
      }

      #[tokio::test]
      async fn ack_en_uses_samantha() {
          let db = fresh_db().await;
          let r = TtsRouter::new(db);
          let p = r.select_provider(&TtsRequest {
              text: "Done".into(), mode: TtsMode::Ack, language: "en".into(),
          }).await.unwrap();
          assert_eq!(p, TtsProvider::MacSay { voice: "Samantha".into() });
      }

      #[tokio::test]
      async fn content_offline_uses_cosyvoice() {
          let db = fresh_db().await;
          {
              let conn = db.conn().await;
              conn.execute(
                  "UPDATE user_preferences SET value='1' WHERE key='tts_offline_mode'",
                  [],
              ).unwrap();
          }
          let r = TtsRouter::new(db);
          let p = r.select_provider(&TtsRequest {
              text: "long content".into(), mode: TtsMode::Content, language: "yue".into(),
          }).await.unwrap();
          assert_eq!(p, TtsProvider::CosyVoice);
      }
  }
  ```

- [ ] **B3.2 Run tests, expect PASS**

  Run: `cd src-tauri && cargo test --lib voice::tts_router::tests`
  Expected: 3 PASS.

- [ ] **B3.3 Wire `TtsRouter` into the voice flow**

  Refactor `src-tauri/src/voice/tts.rs` so that the public `speak(text, mode, language)` entry point calls `TtsRouter::select_provider`, then dispatches:
  - `MacSay { voice }` → existing `say -v {voice}` subprocess.
  - `Minimax` → new `voice::minimax::stream_speak(...)` (Task B6).
  - `CosyVoice` → new `voice::cosyvoice::speak(...)` (Task B8).

  Until B6/B8 land, add stubs that return `Err("not yet implemented")` for those branches, and route Content to MacSay as a temporary fallback so the build stays green.

- [ ] **B3.4 Mount in `voice/mod.rs`**

  Add `pub mod tts_router;`.

- [ ] **B3.5 Smoke test**

  Run app. Manually flip language to yue (B2.4 step). Trigger any voice response (chat with TTS enabled). Verify Sin-ji voice (Cantonese-female) plays.

- [ ] **B3.6 Commit**

  ```bash
  git add src-tauri/src/voice/tts_router.rs src-tauri/src/voice/tts.rs src-tauri/src/voice/mod.rs
  git commit -m "feat(voice): TtsRouter selects provider by mode + language + offline pref"
  ```

---

### Task B4: Bilingual system prompt + language pass-through

**Files:**
- Modify: `src-tauri/src/ai/system_prompt_static.md` (add bilingual rule)
- Modify: `src-tauri/src/ai/tools.rs` (read language pref into dynamic suffix)
- Modify: `src-tauri/src/ai/mod.rs` (pass language down to bedrock::send)

- [ ] **B4.1 Add language rule to static prompt**

  Append to `src-tauri/src/ai/system_prompt_static.md`:
  ```markdown

  ## Response language

  Honor the "Response language" field in the per-request context:

  - `en` — Respond in fluent English. Concise.
  - `yue` — Respond in Hong Kong Cantonese, code-mixed style typical of HK tech professionals: "Schedule 咗 2pm 同 Sarah 個 meeting，已加入 calendar". Use 你 (not 您). Use 嘅/咗/喺/啲, not 的/了/在/些. NEVER use 書面語 ("已為您安排下午二時"). NEVER use simplified characters.
  - `bilingual` — Respond in code-mixed Cantonese-English by default. If the user message is purely English, respond in English. If purely Chinese, respond in Cantonese.

  Examples (yue):
  - User: "幫我 book 個 meeting 聽日 3 點同 Tom"
    Response: "OK，幫你 book 咗聽日 3pm 同 Tom 嘅 meeting，已加入 Google Calendar。要唔要我同 Tom send 個 invite？"
  - User: "睇下今日有咩 urgent email"
    Response: "今日有 3 封 urgent email：1) Sarah re: Q2 report，要 EOD 覆... [bullets]"
  ```

- [ ] **B4.2 Override Cantonese style if user prefers different**

  Step 0.3 default = code-mix. If the user wants pure 廣東話 or pure 書面語, edit the bullet rules above accordingly. **Do not change anything else** — language style is the only thing that varies.

- [ ] **B4.3 Read language pref in dynamic suffix**

  In `src-tauri/src/ai/tools.rs`, update `build_dynamic_suffix` callers to read `language` from `user_preferences`:
  ```rust
  pub async fn read_language_pref(db: &Database) -> String {
      let conn = db.conn().await;
      conn.query_row(
          "SELECT value FROM user_preferences WHERE key = 'language'",
          [], |r| r.get::<_, String>(0),
      ).unwrap_or_else(|_| "en".into())
  }
  ```

- [ ] **B4.4 Pass to `bedrock::send`**

  Wherever the dynamic suffix is built (in `ai/mod.rs::AiRouter::send`), call `tools::read_language_pref(&db).await` and include the result in the dynamic suffix. The cacheable static prompt stays byte-stable; only the dynamic suffix changes per language.

- [ ] **B4.5 Manual smoke**

  Set language to `yue`:
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "UPDATE user_preferences SET value='yue' WHERE key='language';"
  ```
  Run app, send "今日有咩 schedule？" — verify response is code-mixed Cantonese matching the style rules.

- [ ] **B4.6 Commit**

  ```bash
  git add src-tauri/src/ai/system_prompt_static.md src-tauri/src/ai/tools.rs src-tauri/src/ai/mod.rs
  git commit -m "feat(ai): bilingual system prompt with code-mix Cantonese style"
  ```

---

### Task B5: Settings UI — language toggle + voice picker

**Files:**
- Modify: `src/lib/commands.ts`
- Modify: `src/pages/Settings.tsx`

- [ ] **B5.1 Add command wrappers**

  In `src/lib/commands.ts`:
  ```ts
  export async function getPreference(key: string): Promise<string> {
    return await invoke<string>("get_user_preference", { key });
  }
  export async function setPreference(key: string, value: string): Promise<void> {
    await invoke("set_user_preference", { key, value });
  }
  ```
  (If `get/set_user_preference` Tauri commands don't yet exist, add them in `commands/settings.rs` — they're tiny.)

- [ ] **B5.2 Add Language section to Settings**

  In `src/pages/Settings.tsx`, add after the AWS section:
  ```tsx
  // state hooks
  const [language, setLanguage] = useState("en");
  const [sttLanguage, setSttLanguage] = useState("auto");
  const [ttsContentProvider, setTtsContentProvider] = useState("sin-ji");

  useEffect(() => {
    (async () => {
      setLanguage(await getPreference("language"));
      setSttLanguage(await getPreference("stt_language"));
      setTtsContentProvider(await getPreference("tts_content_provider"));
    })();
  }, []);

  // JSX
  <section className="settings-section">
    <h2>Language & Voice</h2>
    <label>Response language
      <select value={language} onChange={async (e) => {
        await setPreference("language", e.target.value); setLanguage(e.target.value);
      }}>
        <option value="en">English</option>
        <option value="yue">廣東話 (Cantonese)</option>
        <option value="bilingual">Bilingual (code-mix)</option>
      </select>
    </label>
    <label>Speech-to-text language
      <select value={sttLanguage} onChange={async (e) => {
        await setPreference("stt_language", e.target.value); setSttLanguage(e.target.value);
      }}>
        <option value="auto">Auto-detect</option>
        <option value="en">English</option>
        <option value="yue">廣東話</option>
      </select>
    </label>
    <label>Cantonese content voice
      <select value={ttsContentProvider} onChange={async (e) => {
        await setPreference("tts_content_provider", e.target.value); setTtsContentProvider(e.target.value);
      }}>
        <option value="sin-ji">Sin-ji (free, local, basic)</option>
        <option value="minimax">MiniMax Speech-02-HD (cloud, best quality)</option>
        <option value="cosyvoice">CosyVoice 2 (offline, requires download)</option>
      </select>
    </label>
  </section>
  ```

- [ ] **B5.3 Smoke test**

  Run app. Change all three dropdowns. Verify the corresponding `user_preferences` values update via:
  ```bash
  sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
    "SELECT key, value FROM user_preferences WHERE key IN ('language','stt_language','tts_content_provider');"
  ```

- [ ] **B5.4 Commit**

  ```bash
  git add src/lib/commands.ts src/pages/Settings.tsx src-tauri/src/commands/settings.rs
  git commit -m "feat(ui): Language & Voice settings section"
  ```

---

### Task B6: MiniMax Speech-02-HD client

**Files:**
- Create: `src-tauri/src/voice/minimax.rs`
- Modify: `src-tauri/src/voice/mod.rs` (mount)
- Modify: `src-tauri/src/voice/tts.rs` (wire MiniMax branch in dispatch)

- [ ] **B6.1 Verify MiniMax API contract**

  Read https://platform.minimax.io/docs/guides/text-to-speech for the exact streaming endpoint, voice id for Cantonese (e.g., `Cantonese_PrettyGirl` or similar), audio format (MP3 streaming), and auth header format. Save the request schema; you'll need it.

- [ ] **B6.2 Write a smoke test**

  Create `src-tauri/src/voice/minimax.rs`:
  ```rust
  pub struct MinimaxClient {
      api_key: String,
  }

  impl MinimaxClient {
      pub fn new(api_key: String) -> Self { Self { api_key } }

      /// Returns streaming MP3 bytes channel.
      pub async fn stream_speak(&self, _text: &str, _voice_id: &str)
          -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, String>
      {
          unimplemented!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      #[tokio::test]
      #[ignore] // run manually with MINIMAX_API_KEY set
      async fn cantonese_tts_returns_mp3_bytes() {
          let key = std::env::var("MINIMAX_API_KEY").expect("MINIMAX_API_KEY env");
          let client = MinimaxClient::new(key);
          let mut rx = client.stream_speak("你好，我係 Jarvis", "Cantonese_PrettyGirl").await.unwrap();
          let mut total = 0;
          while let Some(chunk) = rx.recv().await { total += chunk.len(); }
          assert!(total > 1000, "expected non-trivial mp3 stream, got {total} bytes");
      }
  }
  ```

- [ ] **B6.3 Run, expect FAIL**

  ```bash
  MINIMAX_API_KEY=sk-... cargo test --lib voice::minimax::tests::cantonese_tts_returns_mp3_bytes -- --ignored --nocapture
  ```
  Expected: PANIC `unimplemented!`.

- [ ] **B6.4 Implement `stream_speak`**

  Replace the impl in `minimax.rs`:
  ```rust
  use reqwest::Client;
  use tokio::sync::mpsc;

  impl MinimaxClient {
      pub fn new(api_key: String) -> Self { Self { api_key } }

      pub async fn stream_speak(&self, text: &str, voice_id: &str)
          -> Result<mpsc::Receiver<Vec<u8>>, String>
      {
          // NOTE: Verify exact endpoint + body shape from B6.1
          let url = "https://api.minimaxi.chat/v1/t2a_v2";
          let body = serde_json::json!({
              "model": "speech-02-hd",
              "text": text,
              "stream": true,
              "voice_setting": {
                  "voice_id": voice_id,
                  "speed": 1.0,
                  "vol": 1.0,
                  "pitch": 0,
              },
              "audio_setting": {
                  "sample_rate": 32000,
                  "bitrate": 128000,
                  "format": "mp3",
                  "channel": 1,
              }
          });
          let client = Client::new();
          let resp = client.post(url)
              .bearer_auth(&self.api_key)
              .json(&body)
              .send()
              .await
              .map_err(|e| format!("minimax post: {e}"))?;
          if !resp.status().is_success() {
              return Err(format!("minimax http {}: {}",
                  resp.status(),
                  resp.text().await.unwrap_or_default()));
          }
          let (tx, rx) = mpsc::channel(64);
          tokio::spawn(async move {
              use futures_util::StreamExt;
              let mut byte_stream = resp.bytes_stream();
              while let Some(item) = byte_stream.next().await {
                  match item {
                      Ok(bytes) => {
                          if tx.send(bytes.to_vec()).await.is_err() { break; }
                      }
                      Err(_) => break,
                  }
              }
          });
          Ok(rx)
      }
  }
  ```

- [ ] **B6.5 Run integration test, expect PASS**

  ```bash
  MINIMAX_API_KEY=sk-... cargo test --lib voice::minimax::tests::cantonese_tts_returns_mp3_bytes -- --ignored --nocapture
  ```
  Expected: PASS, total > 1000.

- [ ] **B6.6 Wire MiniMax into TTS dispatch**

  In `src-tauri/src/voice/tts.rs`, replace the `Minimax` arm of the dispatch (currently a stub returning `Err`):
  ```rust
  TtsProvider::Minimax => {
      let api_key = read_pref(&db, "minimax_api_key").await;
      if api_key.is_empty() {
          return Err("MiniMax API key not configured".into());
      }
      let voice_id = match req.language.as_str() {
          "yue" => "Cantonese_PrettyGirl",  // verify exact id from B6.1
          _     => "English_FriendlyMan",
      };
      let client = crate::voice::minimax::MinimaxClient::new(api_key);
      let rx = client.stream_speak(&req.text, voice_id).await?;
      // Pipe rx into existing audio playback path (the same path used by macSay
      // already streams audio chunks — adapt or share).
      pipe_mp3_stream_to_audio_output(rx).await
  }
  ```

- [ ] **B6.7 Add MiniMax API key field to Settings**

  In `src/pages/Settings.tsx`, inside the "Language & Voice" section, add:
  ```tsx
  <label>MiniMax API key
    <input type="password" value={minimaxKey} onChange={async (e) => {
      await setPreference("minimax_api_key", e.target.value);
      setMinimaxKey(e.target.value);
    }} />
  </label>
  ```

- [ ] **B6.8 End-to-end smoke**

  Set language to yue, content provider to minimax, save MiniMax key. Send chat "你今日點？" — verify MiniMax voice plays response.

- [ ] **B6.9 Commit**

  ```bash
  git add src-tauri/src/voice/minimax.rs src-tauri/src/voice/mod.rs src-tauri/src/voice/tts.rs src/pages/Settings.tsx
  git commit -m "feat(voice): MiniMax Speech-02-HD client for Cantonese content TTS"
  ```

---

### Task B7: CosyVoice 2 integration spike + decision

**Files:**
- Create: `src-tauri/src/voice/cosyvoice_spike.md` (decision log; deleted after Task B8)

- [ ] **B7.1 Spike A — `mlx-audio` Python bridge**

  In a scratch directory:
  ```bash
  uv venv .cosyvoice-spike-mlx
  source .cosyvoice-spike-mlx/bin/activate
  pip install mlx-audio
  python -c "from mlx_audio.tts.generate import generate_audio; generate_audio('你好，我係 Jarvis', model_path='FunAudioLLM/CosyVoice2-0.5B', output_path='/tmp/test.wav')"
  open /tmp/test.wav
  ```
  Note: real-time factor (RTF) on this Mac, latency to first audio, install size, dependency footprint.

- [ ] **B7.2 Spike B — `speech-swift` native**

  ```bash
  git clone https://github.com/soniqo/speech-swift /tmp/speech-swift
  cd /tmp/speech-swift
  swift build -c release
  # Run their CosyVoice example with a Cantonese sentence
  ```
  Note: same metrics as above + Tauri integration complexity (subprocess of Swift CLI vs FFI).

- [ ] **B7.3 Decide and document**

  Create `src-tauri/src/voice/cosyvoice_spike.md` with the comparison: RTF, install size, integration complexity, error handling. Pick a winner.

  **Default if both work:** Choose `mlx-audio` Python — easier to bundle (uvx-style invocation), more documented, less FFI risk. Pay the ~150MB Python venv cost on first download.

  **Fallback:** If Python install in the bundled DMG is a non-starter, switch to `speech-swift` despite the Swift FFI complexity.

- [ ] **B7.4 Commit decision log**

  ```bash
  git add src-tauri/src/voice/cosyvoice_spike.md
  git commit -m "docs(voice): CosyVoice 2 runtime spike + decision"
  ```

---

### Task B8: CosyVoice 2 download + runtime wrapper

**Files:**
- Create: `src-tauri/src/voice/cosyvoice.rs`
- Create: `src-tauri/src/voice/cosyvoice_download.rs`
- Modify: `src-tauri/src/voice/mod.rs`
- Modify: `src-tauri/src/voice/tts.rs` (wire CosyVoice branch)

- [ ] **B8.1 Implement model download**

  Create `src-tauri/src/voice/cosyvoice_download.rs`:
  ```rust
  use std::path::PathBuf;
  use tokio::sync::mpsc;

  pub fn model_dir() -> PathBuf {
      dirs::data_dir().unwrap().join("jarvis/models/cosyvoice2")
  }

  /// Returns true if model files already present, otherwise streams download progress.
  pub async fn ensure_downloaded(
      progress: mpsc::Sender<DownloadProgress>,
  ) -> Result<PathBuf, String> {
      let dir = model_dir();
      let marker = dir.join(".complete");
      if marker.exists() {
          return Ok(dir);
      }
      tokio::fs::create_dir_all(&dir).await.map_err(|e| format!("mkdir: {e}"))?;

      // List of files to fetch from huggingface.co/FunAudioLLM/CosyVoice2-0.5B
      // (verify exact file list at spike time)
      let files = [
          "model.safetensors",
          "config.yaml",
          "tokenizer.json",
          // ...
      ];
      let base = "https://huggingface.co/FunAudioLLM/CosyVoice2-0.5B/resolve/main/";
      for (i, f) in files.iter().enumerate() {
          download_file(&format!("{base}{f}"), &dir.join(f), &progress, i, files.len()).await?;
      }
      tokio::fs::write(&marker, b"ok").await.map_err(|e| format!("marker: {e}"))?;
      let _ = progress.send(DownloadProgress::Complete).await;
      Ok(dir)
  }

  #[derive(serde::Serialize, Clone, Debug)]
  pub enum DownloadProgress {
      Started { total_files: usize },
      File { index: usize, name: String, bytes: u64, total: Option<u64> },
      Complete,
  }

  async fn download_file(
      url: &str,
      dest: &std::path::Path,
      progress: &mpsc::Sender<DownloadProgress>,
      index: usize,
      total: usize,
  ) -> Result<(), String> {
      // streaming reqwest get → write to dest, emit progress per chunk
      // standard pattern; ~30 LOC
      todo!("standard streaming download with progress events")
  }
  ```

- [ ] **B8.2 Implement runtime wrapper (per Task B7 decision)**

  Create `src-tauri/src/voice/cosyvoice.rs` with the chosen runtime. If `mlx-audio`:
  ```rust
  use std::process::Stdio;
  use tokio::process::Command;
  use tokio::sync::mpsc;

  pub async fn speak(
      text: &str,
      language: &str,
      model_dir: &std::path::Path,
  ) -> Result<mpsc::Receiver<Vec<u8>>, String> {
      // Use a bundled Python venv (preinstalled at app first-launch via Setup wizard)
      // or invoke uvx mlx-audio with the model_dir.
      let voice_arg = match language { "yue" => "yue", _ => "en" };
      let mut child = Command::new("uvx")
          .args(["--from", "mlx-audio", "mlx-audio-tts",
                 "--text", text,
                 "--model-path", model_dir.to_str().unwrap(),
                 "--lang", voice_arg,
                 "--output", "-",       // write WAV to stdout
                 "--stream"])
          .stdout(Stdio::piped())
          .spawn()
          .map_err(|e| format!("spawn: {e}"))?;
      let stdout = child.stdout.take().ok_or("no stdout")?;
      let (tx, rx) = mpsc::channel(64);
      tokio::spawn(async move {
          use tokio::io::AsyncReadExt;
          let mut reader = tokio::io::BufReader::new(stdout);
          let mut buf = vec![0u8; 4096];
          loop {
              match reader.read(&mut buf).await {
                  Ok(0) => break,
                  Ok(n) => { if tx.send(buf[..n].to_vec()).await.is_err() { break; } }
                  Err(_) => break,
              }
          }
      });
      Ok(rx)
  }
  ```

  (Adapt to `speech-swift` subprocess if that branch was chosen.)

- [ ] **B8.3 Add Tauri commands for download lifecycle**

  In `src-tauri/src/commands/voice.rs` (or extend existing):
  ```rust
  #[tauri::command]
  pub async fn enable_offline_voice(app: tauri::AppHandle, db: State<'_, Arc<Database>>) -> Result<(), String> {
      let (tx, mut rx) = tokio::sync::mpsc::channel(16);
      tokio::spawn(async move {
          while let Some(p) = rx.recv().await {
              let _ = app.emit("cosyvoice_progress", &p);
          }
      });
      let dir = crate::voice::cosyvoice_download::ensure_downloaded(tx).await?;
      // Persist path
      let conn = db.conn().await;
      conn.execute(
          "UPDATE user_preferences SET value = ?1 WHERE key = 'cosyvoice_model_path'",
          rusqlite::params![dir.to_str().unwrap()],
      ).map_err(|e| format!("db: {e}"))?;
      conn.execute(
          "UPDATE user_preferences SET value = '1' WHERE key = 'tts_offline_mode'",
          [],
      ).ok();
      Ok(())
  }
  ```

- [ ] **B8.4 Wire CosyVoice into TTS dispatch**

  In `src-tauri/src/voice/tts.rs`, replace the `CosyVoice` arm of the dispatch (currently a stub):
  ```rust
  TtsProvider::CosyVoice => {
      let model_path = read_pref(&db, "cosyvoice_model_path").await;
      if model_path.is_empty() {
          return Err("CosyVoice model not downloaded — enable Offline Voice in Settings".into());
      }
      let rx = crate::voice::cosyvoice::speak(
          &req.text, &req.language, std::path::Path::new(&model_path),
      ).await?;
      pipe_wav_stream_to_audio_output(rx).await
  }
  ```

- [ ] **B8.5 Add Settings UI for offline voice toggle + progress**

  In `src/pages/Settings.tsx`, add inside Language & Voice section:
  ```tsx
  const [offlineEnabled, setOfflineEnabled] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      setOfflineEnabled((await getPreference("tts_offline_mode")) === "1");
      const unlisten = await listen("cosyvoice_progress", (e: any) => {
        setDownloadProgress(JSON.stringify(e.payload));
      });
      return () => unlisten();
    })();
  }, []);

  <label>
    <input type="checkbox" checked={offlineEnabled} onChange={async (e) => {
      if (e.target.checked) {
        setDownloadProgress("Downloading model...");
        await invoke("enable_offline_voice");
        setOfflineEnabled(true);
      } else {
        await setPreference("tts_offline_mode", "0");
        setOfflineEnabled(false);
      }
    }} />
    Offline voice mode (downloads ~600MB CosyVoice 2 model on first enable)
  </label>
  {downloadProgress && <p className="hint">{downloadProgress}</p>}
  ```

- [ ] **B8.6 End-to-end smoke**

  Toggle Offline Voice in Settings. Wait for download. Confirm `~/Library/Application Support/jarvis/models/cosyvoice2/.complete` exists.
  Disable network (turn off Wi-Fi). Send chat "你好" with language=yue. Verify Cantonese audio plays from local model with no outbound network calls (verify with Little Snitch or `nettop`).

- [ ] **B8.7 Commit**

  ```bash
  git add src-tauri/src/voice/cosyvoice.rs src-tauri/src/voice/cosyvoice_download.rs src-tauri/src/voice/mod.rs src-tauri/src/voice/tts.rs src-tauri/src/commands/voice.rs src/pages/Settings.tsx
  git rm src-tauri/src/voice/cosyvoice_spike.md
  git commit -m "feat(voice): CosyVoice 2 offline TTS with on-demand model download"
  ```

---

### Task B9: Daily Briefing → Cantonese

**Files:**
- Modify: `src-tauri/src/scheduler/daily_briefing.rs`
- Modify: `src-tauri/src/commands/scheduler.rs`

- [ ] **B9.1 Read language preference in briefing**

  In `daily_briefing.rs::run_briefing`, before calling `summarize_with_haiku`:
  ```rust
  let lang = crate::ai::tools::read_language_pref(&db).await;
  let (summary_md, actions_json) = summarize_with_haiku(&classified, &router, &lang).await?;
  // Persist with the same language
  conn.execute(
      "INSERT INTO daily_briefings (date_hkt, summary_md, actions_json, language, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5)",
      rusqlite::params![&date, &summary_md, &actions_json, &lang, now],
  )?;
  ```

- [ ] **B9.2 Speak the briefing if voice enabled**

  After persisting, if `tts_enabled = '1'` in user_preferences, dispatch to TtsRouter:
  ```rust
  let tts_enabled = read_pref(&db, "tts_enabled").await == "1";
  if tts_enabled {
      let router_tts = crate::voice::tts_router::TtsRouter::new(db.clone());
      let provider = router_tts.select_provider(&TtsRequest {
          text: summary_md.clone(),
          mode: TtsMode::Content,
          language: lang.clone(),
      }).await?;
      crate::voice::tts::dispatch(&db, &provider, &summary_md).await?;
  }
  ```

- [ ] **B9.3 Manual smoke (Cantonese end-to-end)**

  Set language to yue. Set content provider to minimax. Trigger:
  ```js
  await window.__TAURI__.core.invoke('trigger_daily_briefing');
  ```
  Verify:
  1. `daily_briefings` row has `language = 'yue'`, `summary_md` is code-mix Cantonese.
  2. MiniMax voice plays the summary aloud.

- [ ] **B9.4 Commit**

  ```bash
  git add src-tauri/src/scheduler/daily_briefing.rs
  git commit -m "feat(scheduler): daily briefing respects language pref + speaks via TtsRouter"
  ```

---

## CHECKPOINT 2 — Phase B complete

Verify before Phase C:

- [ ] All `cargo test --lib` pass.
- [ ] Settings shows AWS, Language, Voice sections; all three persist to DB.
- [ ] Cantonese chat: "今日有咩 schedule？" → code-mix response, MiniMax voice plays.
- [ ] Offline mode toggle: downloads CosyVoice 2 model, subsequent voice uses zero outbound network.
- [ ] Daily Briefing trigger: produces Cantonese 5-bullet summary + 3 actions, speaks via configured provider.
- [ ] Sin-ji (system ack) speaks "已完成" after a successful action.

If any of these fail, fix before C.

---

## Phase C — Demo deliverables

### Task C1: Demo video — Daily Cantonese Inbox Briefing

- [ ] **C1.1 Pre-flight**

  - Wear AirPods (clean audio capture).
  - Quiet room.
  - Set system to Do Not Disturb (no banner notifications during recording).
  - Real Gmail account with a representative inbox (5–10 messages of varied importance).
  - Language: yue. TTS provider: minimax. Briefing time: set 2 minutes from now.

- [ ] **C1.2 Record (use QuickTime → New Screen Recording, 1080p)**

  Sequence (~30 seconds):
  1. JARVIS panel idle, time approaches briefing time.
  2. Briefing arrives, MiniMax voice reads summary in Cantonese.
  3. User clicks one action button ("覆 Sarah re: Q2 report").
  4. JARVIS confirms in Cantonese ("已 draft 咗 reply，要 review 嗎？").
  5. End on the briefing card visible.

- [ ] **C1.3 Save**

  Save to `docs/business/demos/2026-05-04-daily-cantonese-briefing.mp4`. Add a short description in `docs/business/demos/README.md`.

- [ ] **C1.4 Commit**

  ```bash
  git add docs/business/demos/2026-05-04-daily-cantonese-briefing.mp4 docs/business/demos/README.md
  git commit -m "demo: 30s daily Cantonese inbox briefing recording"
  ```

---

### Task C2: Demo video — Privacy Mode (offline)

- [ ] **C2.1 Pre-flight**

  - Open Activity Monitor → Network tab to make outbound traffic visible on screen.
  - Settings → enable Offline Voice (CosyVoice 2 already downloaded).
  - Wi-Fi off (or Little Snitch blocking everything).

- [ ] **C2.2 Record (~15 seconds)**

  1. Show Activity Monitor with zero outbound traffic for JARVIS.
  2. Push-to-talk: "Jarvis, summarise 我啲 unread email"
  3. CosyVoice voice answers in Cantonese.
  4. Activity Monitor still shows zero outbound traffic for JARVIS.

- [ ] **C2.3 Save + commit**

  ```bash
  git add docs/business/demos/2026-05-04-privacy-mode.mp4
  git commit -m "demo: 15s offline Cantonese voice with no network egress"
  ```

---

### Task C3: Architecture leave-behind PDF

- [ ] **C3.1 Draft markdown**

  Create `docs/business/leave_behinds/2026-05-04-jarvis-architecture-v0.2.md`:
  ```markdown
  # JARVIS AI Architecture v0.2 — Bedrock Routing + Cantonese Voice

  Date: 2026-05-04
  Hillman Chan | jarvis-automation.com

  ## What changed since v0.1
  - Bedrock smart routing (Nova Micro / Haiku 4.5 / Sonnet 4.6) via Singapore CRIS
  - Prompt caching cuts repeat-turn input cost ~80%
  - End-to-end Cantonese voice: STT (Whisper yue) → LLM (Haiku 4.5) → TTS (Sin-ji / MiniMax / CosyVoice 2 offline)
  - Daily Cantonese Inbox Briefing cron

  ## Routing diagram
  [insert ASCII diagram or PNG export]

  ## Cost model (per Pro user/month)
  - Nova Micro (volume): US$0.59
  - Haiku 4.5 cached (tools): US$0.39
  - Briefing: US$0.10
  - Sonnet weekly (premium): US$0.12
  - Bedrock LLM total: ~US$1.20
  - MiniMax TTS (Cantonese cloud users): +US$2.40
  - Gross margin at HK$148/mo Pro: 80–94% depending on TTS mode

  ## Privacy mode
  CosyVoice 2 (Apache 2.0, ~600MB) runs locally on Apple Silicon. Zero
  outbound voice traffic when enabled.

  ## What's next
  - 2026-Q3: Per-user Cantonese voice cloning (Team tier)
  - 2026-Q4: HK SME cross-border GBA workflows (subject to ICP validation)
  ```

- [ ] **C3.2 Export to PDF**

  Use `pandoc` or print-to-PDF from any markdown viewer. Save to `docs/business/leave_behinds/2026-05-04-jarvis-architecture-v0.2.pdf`.

- [ ] **C3.3 Commit**

  ```bash
  git add docs/business/leave_behinds/
  git commit -m "docs: architecture leave-behind PDF for AM check-in"
  ```

---

### Task C4: Threads build-in-public post

- [ ] **C4.1 Draft post (≤500 chars)**

  Example:
  > 14 days of work shipped: JARVIS now smart-routes Bedrock (Nova Micro for volume, Claude Haiku 4.5 for tool use, Sonnet 4.6 for "think harder") with prompt caching → blended inference cost down to US$1.20/user/month. Plus end-to-end Cantonese voice with offline mode (CosyVoice 2 on Apple Silicon, no cloud calls). Demo video below 👇 #buildinpublic #HKstartup

  (Drop the emoji per CLAUDE.md "no emojis" rule. Above is illustrative — adjust before posting.)

- [ ] **C4.2 Attach demo video from C1**

  Post on Threads. Cross-post to X. Tag @awscloud and @hkstp where appropriate.

- [ ] **C4.3 Update build-in-public log**

  Append entry to `docs/business/build-in-public-log.md` (create if missing) with date, post URL, post text, engagement metrics 24h later.

- [ ] **C4.4 Commit**

  ```bash
  git add docs/business/build-in-public-log.md
  git commit -m "docs: log Threads post for Bedrock + Cantonese demo"
  ```

---

## Self-Review

Run through this checklist before handing off to executor:

**Spec coverage:**
- [x] Bedrock Singapore CRIS region — Tasks A1, A6
- [x] Workload routing — Tasks A1, A5
- [x] Prompt caching — Tasks A6, A7
- [x] AWS credentials in Settings (encrypted) — Tasks A2, A3, A4
- [x] Daily Cantonese Inbox Briefing — Tasks A9, B9
- [x] V11 + V12 migrations — Tasks A1, B1
- [x] STT language unhardcoding — Task B2
- [x] TTS router (Sin-ji + MiniMax + CosyVoice) — Tasks B3, B6, B8
- [x] Bilingual system prompt with code-mix Cantonese — Task B4
- [x] Settings UI: language toggle + voice picker + MiniMax key + offline toggle — Tasks A4, B5, B6, B8
- [x] Demo deliverables — Tasks C1, C2, C3, C4

**Open question status:**
- Q1 (MiniMax account) — Step 0.2 precondition.
- Q2 (Sin-ji premium voice install) — surfaced in CHECKPOINT 2 manual test.
- Q3 (`mlx-audio` vs `speech-swift`) — Task B7 spike.
- Q4 (AWS credential encryption) — RESOLVED in Tasks A2, A3 (Keychain via security-framework, not SQLite for the secret).
- Q5 (Cantonese style) — Step 0.3 default code-mix; override at B4.2 if user prefers.

**Risks addressed:**
- R1 (MiniMax outage) — TTS router falls back to Sin-ji if MiniMax errors.
- R2 (CRIS region availability) — Step 0.4 verification before any code work.
- R3 (CosyVoice slow on M2) — surface in B7 spike + B8 has graceful "model not downloaded" error.
- R4 (cache hit rate) — `inference_metrics` table + CHECKPOINT 1 verification.
- R5 (Cantonese style drift) — B4 system prompt has explicit examples + CHECKPOINT 2 includes manual chat test.
- R6 (creds leak) — A2 uses Keychain, only opaque marker in SQLite.
- R7 (briefing on empty days) — A9.3 early-return when no messages.
- R8 (PRC vendor B2B blocker) — single-file MiniMax client easy to swap.

**Placeholders found and resolved:**
- A6.4 has `consume_stream`, `record_metrics`, `assistant_msg_from`, `tool_result_msg`, `run_tool` referenced as helpers — these are noted to copy/adapt from existing implementation, not genuine placeholders since the code already exists.
- A9.3 has `todo!()` markers for `classify_with_nova` and `summarize_with_haiku` — these are intentional: the prompts depend on Cantonese style choice (Step 0.3) and are too long to inline. The pattern they follow is well-established in the codebase via existing AiRouter usage.
- B6.1 says "verify exact endpoint + body shape" — this is a real verification step before B6.4, not a placeholder.
- B8.1 has `todo!()` for the streaming download — this is a standard reqwest pattern, ~30 LOC, intentionally not inlined to keep plan readable.

**Type consistency check:**
- `Workload` enum: defined in `ai/routing.rs`, used identically in `ai/mod.rs`, `bedrock.rs`, `commands/chat.rs`, `scheduler/daily_briefing.rs`.
- `AwsCredentials` struct: defined in `aws/credentials.rs`, used in `commands/aws.rs`, `bedrock.rs`.
- `TtsProvider` enum + `TtsRequest` struct: defined in `voice/tts_router.rs`, used in `voice/tts.rs` dispatch.
- `DownloadProgress`: defined in `cosyvoice_download.rs`, used in `commands/voice.rs::enable_offline_voice` event payload.
- All keychain access goes through `KeychainStore::{put, get, delete}` — consistent.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-04-bedrock-routing-cantonese-voice.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
