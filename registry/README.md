# 模型註冊表（Model registry）

`registry/registry.json` 是模型、價格、上限、能力標記、各功能的預設模型、停用開關、服務商資料條款及 JARVIS AI 商業設定的唯一來源。合併至 `main` 後，Registry workflow（`.github/workflows/registry.yml`）會驗證、簽署並發佈至 `GET /v1/registry`，已安裝的 App（P05）及 Bedrock 代理（P08）毋須發佈新版本即可取得。格式以 `registry/schema.json` 為準；讀取端點的契約見 `infra/README.md`。

**註冊表是公開資料：絕不可包含任何金鑰、密碼、憑證或使用者資料。** `validate.ts` 會拒絕看似 API 金鑰、私鑰、權杖或電郵地址的字串，但這只是最後防線，撰寫時仍須自行確認。

## 檔案

| 檔案 | 用途 |
| --- | --- |
| `registry.json` | 註冊表本身（人手編輯）。必須完全等於 `JSON.stringify(registry, null, 2)` 加換行，驗證器因此可拒絕重複的鍵 |
| `schema.json` | JSON Schema，P05、P08 依賴的契約 |
| `validate.ts` | 驗證格式、schema 及 schema 無法表達的規則；`--published <檔案>` 另檢查版本是否高於已發佈版本；`--signed` 用於簽署後的標準位元組 |
| `lookup.ts` | 模型 id 的比對規則（與 App 目前的價格查詢完全相同） |
| `signing.ts` | 標準 JSON（canonical JSON）位元組及 Ed25519 簽署、驗證 |
| `sign.ts` | 只在 workflow 的 sign job 執行，輸出 `registry.json` 及 `registry.json.sig` |
| `verify.ts` | 以已提交的公鑰驗證簽章；`publish.sh` 在任何 S3 操作之前執行 |
| `publish.sh` | 只在 workflow 的 publish job 執行，上載至該環境的 S3 bucket |
| `tsconfig.json` | `npm run registry:typecheck` 的設定（strict，不輸出檔案），涵蓋本資料夾的程式及 `tests/registry*.ts` |
| `signing-public-key.pem` | 註冊表的公鑰，由擁有人產生並提交（見「首次設定」）；P05 會把它編入 App |

## 測試及檢查在哪裡執行

| 檢查 | 在哪裡執行 |
| --- | --- |
| 驗證器 `validate.ts` | PR 檢查（`.github/workflows/registry-check.yml`）；Registry workflow 簽署前；`publish.sh` 上載前 |
| 型別檢查 `npm run registry:typecheck` | PR 檢查；Registry workflow 簽署前 |
| `tests/registry-publish.test.ts`：以替身 `aws` 指令測試 `publish.sh`，不連接 AWS | PR 檢查；Registry workflow 簽署前；`npm run test:frontend` |
| `tests/registry.test.ts`：與 `src-tauri/src/ai/models.rs` 等現有程式碼逐項對照、驗證器的拒絕情況、簽署格式 | PR 檢查；`npm run test:frontend`（本機，以及推送至 `main` 後的 CI）。**不會**在簽署前執行，因此不會阻止發佈 |

PR 檢查只讀取，沒有 environment、secret 或 AWS 權限。PR 改動了 `registry.json` 而 base 分支已有該檔案時，版本必須高於 base 分支的版本；`registry.json` 沒有改動的 PR（例如只改說明或測試）毋須提高版本。

P05 令 App 改讀註冊表之前，`tests/registry.test.ts` 會把註冊表與現時程式碼逐項比對：修改 App 目前寫死的數值（例如 `models.rs` 的價格）會令 PR 檢查及 `npm run test:frontend` 失敗，但不會阻止發佈。這是刻意的：在 P05 之前，改動註冊表不會改變 App 的行為。P05 令 App 改讀註冊表時，這些對照 `models.rs` 等程式碼的鎖定會從測試移除。JARVIS AI 的設定（`managed`）及各模型的 `status` 沒有被測試鎖定，可直接修改；它們的數值以擁有人的決定及審查為準，驗證器只檢查必須一直成立的規則。

## 修改流程

1. 修改 `registry/registry.json`，把 `version` 加一（必須高於已發佈版本），並把 `issued_at` 改為當時的 UTC 時間。
2. 執行 `node --experimental-strip-types registry/validate.ts registry/registry.json`、`npm run registry:typecheck` 及 `npm run test:frontend`。
3. 開 PR；Registry check 會自動執行上述檢查。合併至 `main` 後，Registry workflow 依次執行 validate、sign、publish-dev。
4. 確認 dev 正常後，在 GitHub 的 Actions → Registry → Run workflow（分支 `main`）手動發佈至 prod。

## prod 的人手批准

原本的要求是「prod 經 GitHub environment 人手批准」。GitHub 文件（https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments ，2026-11-13 閱讀）列明：使用 GitHub Free、Pro 或 Team 時，environment 的 required reviewer 只適用於公開 repository；本 repository 是私人的，因此沒有批准按鈕。擁有人於 2026-11-13 選定以下做法取代：

- 合併至 `main` 只會自動發佈至 dev。
- prod 只會在擁有人於 `main` 手動執行 workflow 時發佈。該次執行會重新簽署 `main` 的註冊表（Ed25519 簽章是確定性的，位元組及簽章與 dev 收到的相同），並先以 `REGISTRY_DEV_URL` 讀取 dev 正在提供的內容；位元組或簽章不同即拒絕發佈。
- prod job 仍使用 `registry-prod` environment；日後方案容許時，可再加上 required reviewer。
- 同一時間只有一次發佈在執行，另最多一次在等候；手動執行的 prod 發佈如仍在等候時又有新的推送至 `main`，GitHub 會取消它，須待該次推送發佈至 dev 後再手動執行一次。

## 模型比對規則

- 雲端服務商回報的模型 id，以「包含於該 id 之內的最長 entry id」比對（不分大小寫；長度相同時取較後的 entry），範圍只限 `prices` 不為 `null` 且 `managed` 為 `false` 的 entry。這組 entry 即 `models.rs` 的 `CLOUD_MODELS`，次序亦相同。
- `kind: "family"` 是一系列模型的價格樣式（例如 `gemini-flash`），不是可選擇的模型。
- 功能預設、preset 及 `replacement` 先以完全相同的 id 比對，找不到才用上述規則（例如 `gemini-3.1-pro-preview` 對應 `gemini-3.1-pro`）。
- 比對到的 entry 必須屬於同一服務商，而且不可完全等於 `kind: "family"` 的 entry；只包含 family id 的 id 則可以（例如 `jev-latest` 對應 `jev`）。`bedrock` 例外：它的 id 是經 AWS 使用的其他廠商模型，價格記在該廠商的 entry 之下。
- 仍包含真實 id 的錯字（例如 `gpt-6-luna-x` 會對應 `gpt-6-luna`）目前驗證器無法發現，要堵住須另設明確的別名清單。
- `managed: true` 的 entry 屬 JARVIS AI 自家代理，只以完全相同的 id 比對，永不用於 App 的價格查詢。
- 每個服務商在 `default_for` 列出的功能，都必須在 `features` 有預設模型，而且該模型必須為 `active`。

## JARVIS AI 模型的來源

- 價格：各模型的 `price_source`（AWS Price List，東京 ap-northeast-1）。驗證器要求每個有價格的 JARVIS AI 模型都註明網址及閱讀日期。
- `qwen.qwen3-32b-v1:0`（Qwen3 32B）及 `deepseek.v3.2`（DeepSeek V3.2）的 id 取自 AWS Bedrock User Guide 的模型卡 `model-card-qwen-qwen3-32b.html` 及 `model-card-deepseek-deepseek-v3-2.html`（https://docs.aws.amazon.com/bedrock/latest/userguide/ ，2026-11-13 閱讀）；兩張模型卡均列明東京支援 In-Region 呼叫。
- `apac.amazon.nova-micro-v1:0`（Nova Micro）：沒有任何已閱讀的頁面確認 ap-northeast-1 可使用這個 inference profile（Nova Micro 的模型卡只列出 `us.` 及 `eu.` 的 profile）。P08 使用前必須先確認。

## 簽署格式

- 標準 JSON：所有物件的鍵依 UTF-16 code unit 排序、沒有空白，數值及字串按 `JSON.stringify` 輸出；就註冊表所用的值而言，這與 RFC 8785（JCS）相同。
- 發佈的 `registry.json` 就是這些位元組；`registry.json.sig` 是以 base64 表示、針對這些位元組的 64 位元組 Ed25519 簽章。
- 讀取端必須先以編入的公鑰驗證**收到的原始位元組**，然後才解析，不可重新序列化後再驗證。
- 註冊表的金鑰對與 App 更新用的金鑰（`tauri.conf.json` 的 `plugins.updater.pubkey`）必須不同；`sign.ts` 會檢查。

## 發佈內容及讀取端點

每個環境（dev、prod）各有一個私人 bucket `jarvis-registry-<env>-<account>`：

- `registry/v<version>.json` 及 `.sig`：每個版本只寫入一次，永不覆寫。
- `registry/latest.json` 及 `.sig`：最新版本。`latest.json` 另以 object metadata（`signature`、`version`）保存簽章，令讀取端點一次讀取即取得互相對應的內容及簽章。

`publish.sh` 在任何 S3 操作之前，先以已提交的公鑰驗證簽章；不符即停止。讀取 S3 清單失敗時亦會停止，不會當作「未有發佈內容」。

`GET /v1/registry`（不需登入）回傳 `latest.json` 的原始位元組，附 `X-Registry-Signature`、強 `ETag` 及 `Cache-Control: public, max-age=300`；`If-None-Match` 相符時回 304。未有發佈內容或讀取失敗時回 503 `{"error":"registry_unavailable"}`，不附任何錯誤細節。

給讀取端（P05、P08）的規則：

- App 內置的副本是下限：版本低於內置副本、或低於曾見過的最新版本的註冊表一律不用。
- `min_app_version` 高於 App 版本時，繼續使用 App 內置的資料。
- 註冊表的 `base_url` 不可改變使用者金鑰的發送目的地；App 須把各服務商的主機名稱固定或列入允許清單。
- 給 P05：目前沒有任何欄位或機制令註冊表過期。所有已簽署的版本都保留在 bucket 內，而且永遠能通過驗證；能寫入 `registry/*` 的人可以把 `latest.json` 指回較舊的已簽署版本，只有上述下限會拒絕它。

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

只需停用個別模型或暫停 JARVIS AI 時毋須回滾：

- `managed.paused` 設為 `true` 即可單獨暫停 JARVIS AI，再按上述流程發佈。
- 把模型的 `status` 改為 `disabled`（或 `deprecated` 並填寫 `replacement`）時，如該模型是某個功能預設、preset 項目、`managed.workload_map` 的模型或 `managed.fallback`，必須在同一次改動中一併改用新的模型；否則驗證器會拒絕，不會發佈。其他模型可單獨停用。

## 首次設定（擁有人操作）

須依次完成。第 8 步（提交公鑰的 PR 之前的最後一步）把 repository 變數 `REGISTRY_PUBLISHING` 設為 `on` 之前，推送至 `main` 只會執行 validate：sign 及兩個 publish job 都會略過，workflow 顯示成功，亦不會建立任何 environment。設為 `on` 之後，欠缺 `REGISTRY_SIGNING_KEY` 或 `registry/signing-public-key.pem` 時，sign job 會失敗並指出欠缺哪一項；欠缺 `REGISTRY_ROLE_ARN` 時，publish job 會在取得 AWS 憑證的步驟失敗，訊息由該 action 提供；欠缺 `REGISTRY_BUCKET` 時，`publish.sh` 會失敗。以上情況都不會發佈任何內容。

以下 AWS 指令使用部署腳本所用的 `jarvis` profile；每個指令都明確指定 profile，避免使用預設憑證。所有 `gh` 指令使用同一個 `REPO` 變數，請在同一個終端機視窗內完成全部步驟。

0. **設定 `REPO`**（只需一次）：
   ```bash
   REPO=ChiFungHillmanChan/jarvis-ai-assistant
   ```
1. **GitHub 方案**：私人 repository 的 environment 及 environment secret 需要 GitHub Pro（或 Team、Enterprise）。
2. **建立三個 GitHub environment**（`registry-signing`、`registry-dev`、`registry-prod`），每個都只容許 `main` 分支。這一步必須在第 5 步建立發佈角色之前完成：environment job 的 OIDC subject 不包含分支，未有分支規則時，任何分支上指名該 environment 的 workflow 都可取得角色接受的權杖。
   ```bash
   for ENV in registry-signing registry-dev registry-prod; do
     echo '{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}' \
       | gh api -X PUT "repos/$REPO/environments/$ENV" --input -
     gh api -X POST "repos/$REPO/environments/$ENV/deployment-branch-policies" -f name=main -f type=branch
   done
   ```
   檢查：每個 environment 都應顯示 `{"custom_branch_policies":true,"protected_branches":false}` 及 `main`。
   ```bash
   for ENV in registry-signing registry-dev registry-prod; do
     echo "$ENV"
     gh api "repos/$REPO/environments/$ENV" --jq .deployment_branch_policy
     gh api "repos/$REPO/environments/$ENV/deployment-branch-policies" --jq '.branch_policies[].name'
   done
   ```
   亦可在 GitHub 網頁操作：Settings → Environments → New environment，輸入名稱後於 Deployment branches and tags 選 Selected branches and tags，加入 `main`。
3. **AWS 的 GitHub OIDC provider**（每個 AWS 帳戶只需一次）：
   ```bash
   aws iam list-open-id-connect-providers --profile jarvis
   # 清單沒有 token.actions.githubusercontent.com 時：
   aws iam create-open-id-connect-provider --profile jarvis \
     --url https://token.actions.githubusercontent.com --client-id-list sts.amazonaws.com
   ```
4. **確認 dev 的 Payment stack 已部署**（路由加在它的 HTTP API 上）：
   ```bash
   aws cloudformation list-exports --profile jarvis --region ap-southeast-1 \
     --query "Exports[?Name=='Jarvis-dev-ApiBaseUrl'].Value" --output text
   ```
5. **部署 dev 的 Registry stack**：
   ```bash
   cd infra && npm ci && (cd lambda/registry && npm ci) && npm run build
   npx cdk diff --profile jarvis --context env=dev --exclusively Jarvis-dev-Registry
   npm run deploy:dev -- --exclusively Jarvis-dev-Registry
   ```
   **不可省略 `--exclusively`**：省略時，CDK 會先以本機的 checkout 部署 Registry 所依賴的 Auth、Data 及 Payment stack；prod 的 Payment stack 如部署時缺少三個 Stripe id，checkout 會回傳 503。記下輸出 `RegistryBucketName`、`RegistryPublisherRoleArn`、`RegistryUrl`。
6. **產生註冊表的金鑰對，私鑰直接存入 secret**。私鑰只存在於 `registry-signing` 的 secret `REGISTRY_SIGNING_KEY`：不會寫入任何檔案、不會顯示在螢幕上，亦不另存副本（擁有人 2026-11-16 的決定）。在 repository 的任何一個現有 checkout 內，開一個由 `main` 開出的新 worktree 執行；主要 checkout 的檔案及未提交的改動不受影響：
   ```bash
   git fetch origin main
   git worktree add ../jarvis-registry-public-key -b chore/registry-public-key origin/main
   cd ../jarvis-registry-public-key
   node -e '
   if (process.stdout.isTTY) { console.error("pipe this into gh secret set; the private key never goes to the screen"); process.exit(1); }
   const { generateKeyPairSync } = require("node:crypto");
   const fs = require("node:fs");
   const { publicKey, privateKey } = generateKeyPairSync("ed25519");
   fs.writeFileSync("registry/signing-public-key.pem", publicKey.export({ type: "spki", format: "pem" }));
   process.stdout.write(privateKey.export({ type: "pkcs8", format: "pem" }));
   ' | gh secret set REGISTRY_SIGNING_KEY --env registry-signing --repo "$REPO"
   ```
   `node` 把公鑰寫入 `registry/signing-public-key.pem`，私鑰只寫到標準輸出，經管道直接交給 `gh secret set`（它從標準輸入讀取 secret 的值）；如沒有接上管道、標準輸出是終端機，`node` 會拒絕執行，不會產生金鑰。如任何一個指令失敗，只需在同一個 worktree 再執行一次整個 `node … | gh …` 指令：會產生一對新的金鑰並覆寫公鑰檔案。
   檢查：應列出 `REGISTRY_SIGNING_KEY` 及更新時間（不會顯示其值）。
   ```bash
   gh secret list --env registry-signing --repo "$REPO"
   ```
   **代價**：私鑰只存在於這個 secret。如 secret 遺失或被刪除，擁有人須重新產生金鑰對；公鑰編入 App（P05）之後，還要先發佈一個帶有新公鑰的 App 版本，使用者才能再收到註冊表更新。這對金鑰與 App 更新用的金鑰不同，`sign.ts` 每次簽署都會檢查。
7. **設定 dev 的變數**（第 5 步的輸出）：
   ```bash
   gh variable set REGISTRY_BUCKET   --env registry-dev --repo "$REPO" --body <RegistryBucketName>
   gh variable set REGISTRY_ROLE_ARN --env registry-dev --repo "$REPO" --body <RegistryPublisherRoleArn>
   ```
   檢查：應列出這兩個變數及其值。
   ```bash
   gh variable list --env registry-dev --repo "$REPO"
   ```
8. **開啟發佈**（提交公鑰的 PR 之前的最後一步）：
   ```bash
   gh variable set REGISTRY_PUBLISHING --repo "$REPO" --body on
   ```
   檢查：repository 變數中應有 `REGISTRY_PUBLISHING`，值為 `on`。
   ```bash
   gh variable list --repo "$REPO"
   ```
9. **提交公鑰**：在第 6 步的 worktree commit `registry/signing-public-key.pem`，push 該分支，開 PR 並合併至 `main`。合併會觸發 workflow，首次發佈至 dev。在同一個 worktree 檢查：
   ```bash
   curl -s -D /tmp/registry.headers -o /tmp/registry.json <RegistryUrl>
   SIG=$(grep -i '^x-registry-signature:' /tmp/registry.headers | cut -d' ' -f2 | tr -d '\r') \
   node --experimental-strip-types --input-type=module -e '
   import { readFileSync } from "node:fs";
   import { verifyBytes } from "./registry/signing.ts";
   console.log(verifyBytes(readFileSync("/tmp/registry.json"), process.env.SIG, readFileSync("registry/signing-public-key.pem", "utf8")) ? "signature ok" : "SIGNATURE BAD");'
   ```
   顯示 `signature ok` 後，可回到原本的 checkout，以 `git worktree remove ../jarvis-registry-public-key` 移除這個 worktree。
10. **prod**：以 `env=prod` 重複第 4、5 步（`Jarvis-prod-ApiBaseUrl`；部署用 `npm run deploy:prod -- --exclusively Jarvis-prod-Registry`，同樣不可省略 `--exclusively`，原因見第 5 步），然後設定 `registry-prod` 的變數並檢查：
    ```bash
    gh variable set REGISTRY_BUCKET   --env registry-prod --repo "$REPO" --body <prod 的 RegistryBucketName>
    gh variable set REGISTRY_ROLE_ARN --env registry-prod --repo "$REPO" --body <prod 的 RegistryPublisherRoleArn>
    gh variable set REGISTRY_DEV_URL  --env registry-prod --repo "$REPO" --body <dev 的 RegistryUrl>
    gh variable list --env registry-prod --repo "$REPO"
    ```
    最後手動發佈：
    ```bash
    gh workflow run registry.yml --ref main --repo "$REPO"
    ```
