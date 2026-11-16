# 模型註冊表（Model registry）

`registry/registry.json` 是模型、價格、上限、能力標記、各功能的預設模型、停用開關、服務商資料條款及 JARVIS AI 商業設定的唯一來源。合併至 `main` 後，Registry workflow（`.github/workflows/registry.yml`）會驗證、簽署並發佈至 `GET /v1/registry`，已安裝的 App（P05）及 Bedrock 代理（P08）毋須發佈新版本即可取得。格式以 `registry/schema.json` 為準；讀取端點的契約見 `infra/README.md`。

**註冊表是公開資料：絕不可包含任何金鑰、密碼、憑證或使用者資料。** `validate.ts` 會拒絕看似 API 金鑰、私鑰、權杖或電郵地址的字串，但這只是最後防線，撰寫時仍須自行確認。

## 檔案

| 檔案 | 用途 |
| --- | --- |
| `registry.json` | 註冊表本身（人手編輯，縮排格式） |
| `schema.json` | JSON Schema，P05、P08 依賴的契約 |
| `validate.ts` | 驗證 schema 及 schema 無法表達的規則；`--published <檔案>` 另檢查版本是否高於已發佈版本 |
| `lookup.ts` | 模型 id 的比對規則（與 App 目前的價格查詢完全相同） |
| `signing.ts` | 標準 JSON（canonical JSON）位元組及 Ed25519 簽署、驗證 |
| `sign.ts` | 只在 workflow 的 sign job 執行，輸出 `registry.json` 及 `registry.json.sig` |
| `publish.sh` | 只在 workflow 的 publish job 執行，上載至該環境的 S3 bucket |
| `signing-public-key.pem` | 註冊表的公鑰，由擁有人產生並提交（見「首次設定」）；P05 會把它編入 App |

測試：`tests/registry.test.ts`（與 `src-tauri/src/ai/models.rs` 等現有程式碼逐項對照、驗證器的拒絕情況、簽署格式）及 `tests/registry-publish.test.ts`（以替身 `aws` 指令測試 `publish.sh`，不連接 AWS）。兩者均包含在 `npm run test:frontend`。

## 修改流程

1. 修改 `registry/registry.json`，把 `version` 加一（必須高於已發佈版本），並把 `issued_at` 改為當時的 UTC 時間。
2. 執行 `node --experimental-strip-types registry/validate.ts registry/registry.json` 及 `npm run test:frontend`。
3. 開 PR 並合併至 `main`。Registry workflow 依次執行 validate、sign、publish-dev。
4. 確認 dev 正常後，在 GitHub 的 Actions → Registry → Run workflow（分支 `main`）手動發佈至 prod。

P05 令 App 改讀註冊表之前，`tests/registry.test.ts` 會把註冊表與現時程式碼逐項比對；修改 App 目前寫死的數值會令測試失敗。這是刻意的：在 P05 之前，改動註冊表不會改變 App 的行為。

## prod 的人手批准

原本的要求是「prod 經 GitHub environment 人手批准」。GitHub 文件（https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments ，2026-11-13 閱讀）列明：使用 GitHub Free、Pro 或 Team 時，environment 的 required reviewer 只適用於公開 repository；本 repository 是私人的，因此沒有批准按鈕。擁有人於 2026-11-13 選定以下做法取代：

- 合併至 `main` 只會自動發佈至 dev。
- prod 只會在擁有人於 `main` 手動執行 workflow 時發佈。該次執行會重新簽署 `main` 的註冊表（Ed25519 簽章是確定性的，位元組及簽章與 dev 收到的相同），並先以 `REGISTRY_DEV_URL` 讀取 dev 正在提供的內容；位元組或簽章不同即拒絕發佈。
- prod job 仍使用 `registry-prod` environment；日後方案容許時，可再加上 required reviewer。

## 模型比對規則

- 雲端服務商回報的模型 id，以「包含於該 id 之內的最長 entry id」比對（不分大小寫；長度相同時取較後的 entry），範圍只限 `prices` 不為 `null` 且 `managed` 為 `false` 的 entry。這組 entry 即 `models.rs` 的 `CLOUD_MODELS`，次序亦相同。
- `kind: "family"` 是一系列模型的價格樣式（例如 `gemini-flash`），不是可選擇的模型。
- 功能預設、preset 及 `replacement` 先以完全相同的 id 比對，找不到才用上述規則（例如 `gemini-3.1-pro-preview` 對應 `gemini-3.1-pro`）。
- 比對到的 entry 必須屬於同一服務商，而且不可完全等於 `kind: "family"` 的 entry；只包含 family id 的 id 則可以（例如 `jev-latest` 對應 `jev`）。`bedrock` 例外：它的 id 是經 AWS 使用的其他廠商模型，價格記在該廠商的 entry 之下。
- 仍包含真實 id 的錯字（例如 `gpt-6-luna-x` 會對應 `gpt-6-luna`）目前驗證器無法發現，要堵住須另設明確的別名清單。
- `managed: true` 的 entry 屬 JARVIS AI 自家代理，只以完全相同的 id 比對，永不用於 App 的價格查詢。
- 每個服務商在 `default_for` 列出的功能，都必須在 `features` 有預設模型，而且該模型必須為 `active`。

## 簽署格式

- 標準 JSON：所有物件的鍵依 UTF-16 code unit 排序、沒有空白，數值及字串按 `JSON.stringify` 輸出；就註冊表所用的值而言，這與 RFC 8785（JCS）相同。
- 發佈的 `registry.json` 就是這些位元組；`registry.json.sig` 是以 base64 表示、針對這些位元組的 64 位元組 Ed25519 簽章。
- 讀取端必須先以編入的公鑰驗證**收到的原始位元組**，然後才解析，不可重新序列化後再驗證。
- 註冊表的金鑰對與 App 更新用的金鑰（`tauri.conf.json` 的 `plugins.updater.pubkey`）必須不同；`sign.ts` 會檢查。

## 發佈內容及讀取端點

每個環境（dev、prod）各有一個私人 bucket `jarvis-registry-<env>-<account>`：

- `registry/v<version>.json` 及 `.sig`：每個版本只寫入一次，永不覆寫。
- `registry/latest.json` 及 `.sig`：最新版本。`latest.json` 另以 object metadata（`signature`、`version`）保存簽章，令讀取端點一次讀取即取得互相對應的內容及簽章。

`GET /v1/registry`（不需登入）回傳 `latest.json` 的原始位元組，附 `X-Registry-Signature`、強 `ETag` 及 `Cache-Control: public, max-age=300`；`If-None-Match` 相符時回 304。未有發佈內容或讀取失敗時回 503 `{"error":"registry_unavailable"}`，不附任何錯誤細節。

給讀取端（P05、P08）的規則：

- 版本低於手上副本的註冊表一律不用。
- `min_app_version` 高於 App 版本時，繼續使用 App 內置的資料。
- 註冊表的 `base_url` 不可改變使用者金鑰的發送目的地；App 須把各服務商的主機名稱固定或列入允許清單。

## 回滾

已發佈的版本不會被覆寫；回滾方法是以舊內容發佈一個**更高的版本號**：

```bash
# 1. 查看已發佈的版本號（例如 7）
curl -s https://<api>/v1/registry | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).version))'
# 2. 取回要回復的內容，並把版本設為已發佈版本加一（此例為 8）
git show <good-commit>:registry/registry.json > registry/registry.json
node -e 'const fs=require("fs"),p="registry/registry.json",d=JSON.parse(fs.readFileSync(p,"utf8"));d.version=8;d.issued_at=new Date().toISOString().replace(/\.\d+Z$/,"Z");fs.writeFileSync(p,JSON.stringify(d,null,2)+"\n")'
# 3. 驗證後開 PR 並合併；dev 自動發佈，prod 照常手動執行 workflow
node --experimental-strip-types registry/validate.ts registry/registry.json
```

只需停用個別模型或暫停 JARVIS AI 時毋須回滾：把該模型的 `status` 改為 `disabled`（或 `deprecated` 並填寫 `replacement`），或把 `managed.paused` 設為 `true`，再按上述流程發佈。

## 首次設定（擁有人操作）

須依次完成。完成前，workflow 的 sign job 會失敗並指出欠缺的步驟，不會發佈任何內容。以下 AWS 指令使用部署腳本所用的 `jarvis` profile；每個指令都明確指定 profile，避免使用預設憑證。

1. **GitHub 方案**：私人 repository 的 environment 及 environment secret 需要 GitHub Pro（或 Team、Enterprise）。
2. **AWS 的 GitHub OIDC provider**（每個 AWS 帳戶只需一次）：
   ```bash
   aws iam list-open-id-connect-providers --profile jarvis
   # 清單沒有 token.actions.githubusercontent.com 時：
   aws iam create-open-id-connect-provider --profile jarvis \
     --url https://token.actions.githubusercontent.com --client-id-list sts.amazonaws.com
   ```
3. **確認 dev 的 Payment stack 已部署**（路由加在它的 HTTP API 上）：
   ```bash
   aws cloudformation list-exports --profile jarvis --region ap-southeast-1 \
     --query "Exports[?Name=='Jarvis-dev-ApiBaseUrl'].Value" --output text
   ```
4. **部署 dev 的 Registry stack**：
   ```bash
   cd infra && npm ci && (cd lambda/registry && npm ci) && npm run build
   npx cdk diff --profile jarvis --context env=dev --exclusively Jarvis-dev-Registry
   npm run deploy:dev -- --exclusively Jarvis-dev-Registry
   ```
   記下輸出 `RegistryBucketName`、`RegistryPublisherRoleArn`、`RegistryUrl`。
5. **建立三個 GitHub environment**（`registry-signing`、`registry-dev`、`registry-prod`），每個都只容許 `main` 分支：
   ```bash
   REPO=ChiFungHillmanChan/jarvis-ai-assistant
   for ENV in registry-signing registry-dev registry-prod; do
     echo '{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}' \
       | gh api -X PUT "repos/$REPO/environments/$ENV" --input -
     gh api -X POST "repos/$REPO/environments/$ENV/deployment-branch-policies" -f name=main -f type=branch
   done
   ```
   亦可在 GitHub 網頁操作：Settings → Environments → New environment，輸入名稱後於 Deployment branches and tags 選 Selected branches and tags，加入 `main`。
6. **產生註冊表的金鑰對**（在本機的 repository 根目錄、於一個由 `main` 開出的新分支執行；私鑰不會顯示在螢幕上）：
   ```bash
   git switch -c chore/registry-public-key origin/main
   umask 077
   node -e '
   const { generateKeyPairSync } = require("node:crypto");
   const fs = require("node:fs");
   const { publicKey, privateKey } = generateKeyPairSync("ed25519");
   fs.writeFileSync("registry/signing-public-key.pem", publicKey.export({ type: "spki", format: "pem" }));
   fs.writeFileSync(process.env.HOME + "/jarvis-registry-private.pem", privateKey.export({ type: "pkcs8", format: "pem" }), { mode: 0o600 });
   '
   gh secret set REGISTRY_SIGNING_KEY --env registry-signing --repo "$REPO" < ~/jarvis-registry-private.pem
   ```
   把 `~/jarvis-registry-private.pem` 存入密碼管理器或離線備份，然後刪除（`rm ~/jarvis-registry-private.pem`）。公鑰一旦編入 App（P05），遺失註冊表的私鑰便要發佈新版 App 才能更換。
7. **設定 dev 的變數**（第 4 步的輸出）：
   ```bash
   gh variable set REGISTRY_BUCKET   --env registry-dev --repo "$REPO" --body <RegistryBucketName>
   gh variable set REGISTRY_ROLE_ARN --env registry-dev --repo "$REPO" --body <RegistryPublisherRoleArn>
   ```
8. **提交公鑰**：commit `registry/signing-public-key.pem`，開 PR 並合併至 `main`。合併會觸發 workflow，首次發佈至 dev。檢查（在 repository 根目錄執行）：
   ```bash
   curl -s -D /tmp/registry.headers -o /tmp/registry.json <RegistryUrl>
   SIG=$(grep -i '^x-registry-signature:' /tmp/registry.headers | cut -d' ' -f2 | tr -d '\r') \
   node --experimental-strip-types --input-type=module -e '
   import { readFileSync } from "node:fs";
   import { verifyBytes } from "./registry/signing.ts";
   console.log(verifyBytes(readFileSync("/tmp/registry.json"), process.env.SIG, readFileSync("registry/signing-public-key.pem", "utf8")) ? "signature ok" : "SIGNATURE BAD");'
   ```
9. **prod**：以 `env=prod` 重複第 3、4 步（`Jarvis-prod-ApiBaseUrl`；部署用 `npm run deploy:prod -- --exclusively Jarvis-prod-Registry`），設定 `registry-prod` 的 `REGISTRY_BUCKET`、`REGISTRY_ROLE_ARN`（prod 的輸出）及 `REGISTRY_DEV_URL`（dev 的 `RegistryUrl`），然後執行：
   ```bash
   gh workflow run registry.yml --ref main --repo "$REPO"
   ```
