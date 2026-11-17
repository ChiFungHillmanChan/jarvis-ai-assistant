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

PR 檢查只讀取，沒有 environment、secret 或 AWS 權限。除了 `registry/`、registry 的測試及 workflow，PR 檢查亦會在測試讀取的其他檔案（App 測試所用的凍結副本 `src-tauri/src/ai/registry/fixture.json` 及 `src-tauri/tauri.conf.json`）或 `package.json`、`package-lock.json` 有改動時執行；如測試讀取了 workflow 的 `paths` 沒有列出的檔案，`tests/registry.test.ts` 的最後一項測試會失敗。PR 改動了 `registry.json` 而 base 分支已有該檔案時，版本必須高於 base 分支的版本；`registry.json` 沒有改動的 PR（例如只改說明或測試）毋須提高版本。

自 P05 起，App 直接讀取註冊表，測試不再把註冊表與程式碼比對：修改價格、預設模型或上限毋須改動任何測試。App 的 Rust 測試及 `registry/lookup.ts` 的查詢測試使用凍結的副本 `src-tauri/src/ai/registry/fixture.json`（即首次發佈的第 1 版），因此改動 `registry.json` 不會令這些測試失敗；隨 App 發佈的副本則由 `the_bundled_registry_is_usable` 等 Rust 測試檢查是否可用，包括 App 讀取的每個上限。舊用量紀錄的一次性修正（`src-tauri/src/ai/cost.rs` 的 `correct_old_rows`）不讀取註冊表，而是使用凍結的價格表 `src-tauri/src/ai/old_rows_prices.frozen.json`（第 1 版的價格），令每個版本修正舊紀錄的結果相同；該檔案永不修改，新價格只應寫入 `registry.json`。JARVIS AI 的設定（`managed`）及各模型的 `status` 沒有被測試鎖定，可直接修改；它們的數值以擁有人的決定及審查為準，驗證器只檢查必須一直成立的規則。驗證器對 `managed.hkd_per_usd` 的檢查是港元聯繫匯率的兌換範圍 7.75 至 7.85；匯率如須設於這個範圍之外，必須在同一個 PR 一併修改驗證器。

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

App 的做法（P05，`src-tauri/src/ai/registry/`）：啟動時先用簽章有效、版本高於內置副本而且 `min_app_version` 符合的快取副本，否則用內置副本，啟動時不連網。之後在啟動時、每 6 小時及 Mac 喚醒後以 `If-None-Match` 取得遠端副本；只有簽章有效、版本高於目前使用及曾見過的最高版本、而且 `min_app_version` 符合的副本才會取代快取。被拒的副本只記錄原因，不記錄內容。資料條款以內置副本為下限，只可收緊；遠端副本新增的服務商一律視為 `unknown`，其 `base_url`、地區及主機名稱都不會被使用。被停用的模型不會自動改用替代模型：App 會提示使用者確認，確認前使用該模型的請求會停止，背景工作亦不會執行。正式版本的端點於編譯時以 `JARVIS_REGISTRY_URL` 指定；未指定時 App 只使用內置副本。

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

以下 AWS 指令使用部署腳本所用的 `jarvis` profile；每個指令都明確指定 profile，避免使用預設憑證。請在 repository 的主要 checkout 內開始，並在同一個終端機視窗內逐行完成全部步驟：`REPO`、`MAIN_CHECKOUT`、`KEYGEN`、`KEY_WORKTREE` 及 `REGISTRY_URL` 這些變數只存在於這個視窗。以 `&&` 連接的各行，任何一行失敗都會停止其後各行；停止時先處理該行的錯誤，再從該段的開頭重新執行。

0. **設定 `REPO` 及主要 checkout 的位置**（只需一次；在主要 checkout 內任何一個資料夾執行）：
   ```bash
   REPO=ChiFungHillmanChan/jarvis-ai-assistant
   MAIN_CHECKOUT="$(git rev-parse --show-toplevel)" && echo "$MAIN_CHECKOUT"
   ```
   檢查：顯示的應是主要 checkout 的頂層資料夾。
1. **GitHub 方案**：私人 repository 的 environment 及 environment secret 需要 GitHub Pro（或 Team、Enterprise）。
2. **建立三個 GitHub environment**（`registry-signing`、`registry-dev`、`registry-prod`），每個都只容許 `main` 分支。這一步必須在第 5 步建立發佈角色之前完成：environment job 的 OIDC subject 不包含分支，未有分支規則時，任何分支上指名該 environment 的 workflow 都可取得角色接受的權杖。
   ```bash
   for ENV in registry-signing registry-dev registry-prod; do
     echo '{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}' \
       | gh api -X PUT "repos/$REPO/environments/$ENV" --input -
     gh api -X POST "repos/$REPO/environments/$ENV/deployment-branch-policies" -f name=main -f type=branch
   done
   ```
   重新執行時，如 POST 回覆 `main` 的規則已存在，可略過該錯誤。
   檢查：每個 environment 都應顯示 `{"custom_branch_policies":true,"protected_branches":false}` 及 `main`。
   ```bash
   for ENV in registry-signing registry-dev registry-prod; do
     echo "$ENV"
     gh api "repos/$REPO/environments/$ENV" --jq .deployment_branch_policy
     gh api "repos/$REPO/environments/$ENV/deployment-branch-policies" --jq '.branch_policies[].name'
   done
   ```
   亦可在 GitHub 網頁操作：Settings → Environments → New environment，輸入名稱後於 Deployment branches and tags 選 Selected branches and tags，加入 `main`。
3. **AWS 的 GitHub OIDC provider**（每個 AWS 帳戶只需一次）。先查看是否已有：
   ```bash
   aws iam list-open-id-connect-providers --profile jarvis \
     --query "OpenIDConnectProviderList[?ends_with(Arn, ':oidc-provider/token.actions.githubusercontent.com')].Arn" --output text
   ```
   沒有任何輸出時才建立：
   ```bash
   aws iam create-open-id-connect-provider --profile jarvis \
     --url https://token.actions.githubusercontent.com --client-id-list sts.amazonaws.com
   ```
   檢查：再執行第一個指令，應顯示一個以 `:oidc-provider/token.actions.githubusercontent.com` 結尾的 ARN。
4. **確認 dev 的 Payment stack 已部署**（路由加在它的 HTTP API 上）：
   ```bash
   aws cloudformation list-exports --profile jarvis --region ap-southeast-1 \
     --query "Exports[?Name=='Jarvis-dev-ApiBaseUrl'].Value" --output text
   ```
   檢查：應顯示一個以 `https://` 開頭的網址。沒有輸出即 dev 的 Payment stack 未部署，須先部署它才可繼續。
5. **部署 dev 的 Registry stack**。部署使用主要 checkout 的程式碼，因此第一段先確認它就是最新的 `main`（已合併 P04），而且 `infra/` 沒有未提交的改動，否則在建置之前停止；停止時先 `git switch main && git pull --ff-only`（或先處理 `infra/` 未提交的改動），再重新執行。部署的一行會再檢查一次這兩項：即使把整段一次貼上，檢查不通過時亦不會部署。
   ```bash
   cd "${MAIN_CHECKOUT:?先執行第 0 步}" &&
     git fetch origin main &&
     test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" &&
     test -z "$(git status --porcelain -- infra)" &&
     cd infra && npm ci && (cd lambda/registry && npm ci) && npm run build
   cd "${MAIN_CHECKOUT:?先執行第 0 步}/infra" && npx cdk diff --profile jarvis --context env=dev --exclusively Jarvis-dev-Registry
   cd "${MAIN_CHECKOUT:?先執行第 0 步}/infra" &&
     test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" &&
     test -z "$(git status --porcelain -- .)" &&
     npm run deploy:dev -- --exclusively Jarvis-dev-Registry
   ```
   **不可省略 `--exclusively`**：省略時，CDK 會先以本機的 checkout 部署 Registry 所依賴的 Auth、Data 及 Payment stack；prod 的 Payment stack 如部署時缺少三個 Stripe id，checkout 會回傳 503。
   檢查：應列出 `RegistryBucketName`、`RegistryPublisherRoleArn` 及 `RegistryUrl` 三項輸出（第 7 步用前兩項，第 9 步用 `RegistryUrl`）；以 `RegistryUrl` 執行的 `curl` 應顯示 `503`，即端點已存在、但未有發佈內容（第 9 步之後才會是 `200`）。
   ```bash
   aws cloudformation describe-stacks --profile jarvis --region ap-southeast-1 --stack-name Jarvis-dev-Registry \
     --query "Stacks[0].Outputs[].[OutputKey,OutputValue]" --output text
   curl -s -o /dev/null -w '%{http_code}\n' <RegistryUrl>
   ```
6. **產生註冊表的金鑰對，私鑰直接存入 secret**。私鑰只存在於 `registry-signing` 的 secret `REGISTRY_SIGNING_KEY`：不會寫入任何檔案、不會顯示在螢幕上，亦不另存副本（擁有人 2026-11-16 的決定）。下面先把產生金鑰的程式存入變數 `KEYGEN`（只是程式，不含任何金鑰）；然後不論之前的步驟令終端機停在哪個資料夾，都先回到主要 checkout 的頂層，在它旁邊開一個由 `origin/main` 開出的新 worktree（`KEY_WORKTREE`），再在其中產生金鑰對。各行以 `&&` 連接，任何一行失敗都會在產生金鑰之前停止。主要 checkout 的檔案及未提交的改動不受影響。
   ```bash
   KEYGEN='
   if (process.stdout.isTTY) { console.error("pipe this into gh secret set; the private key never goes to the screen"); process.exit(1); }
   const { generateKeyPairSync } = require("node:crypto");
   const fs = require("node:fs");
   const { publicKey, privateKey } = generateKeyPairSync("ed25519");
   fs.writeFileSync("registry/signing-public-key.pem", publicKey.export({ type: "spki", format: "pem" }));
   process.stdout.write(privateKey.export({ type: "pkcs8", format: "pem" }));
   '
   cd "${MAIN_CHECKOUT:?先執行第 0 步}" &&
     KEY_WORKTREE="$(dirname "$MAIN_CHECKOUT")/jarvis-registry-public-key" &&
     git fetch origin main &&
     git worktree add -b chore/registry-public-key "$KEY_WORKTREE" origin/main &&
     cd "$KEY_WORKTREE" &&
     test -f registry/schema.json &&
     node -e "$KEYGEN" | gh secret set REGISTRY_SIGNING_KEY --env registry-signing --repo "$REPO"
   ```
   `node` 把公鑰寫入 `registry/signing-public-key.pem`，私鑰只寫到標準輸出，經管道直接交給 `gh secret set`（它從標準輸入讀取 secret 的值）；如沒有接上管道、標準輸出是終端機，`node` 會拒絕執行，不會產生金鑰。如在 `test -f registry/schema.json` 停止，即 `origin/main` 未有 `registry/`（P04 未合併）。
   如 `node` 或 `gh` 失敗（例如 environment 未建立），或之前的嘗試已建立這個 worktree（`git worktree add` 因此停止），在同一個視窗只執行下面一行重試：會產生一對新的金鑰並覆寫公鑰檔案。如分支 `chore/registry-public-key` 已存在而 worktree 不存在，先以 `git -C "$MAIN_CHECKOUT" branch -D chore/registry-public-key` 刪除舊分支，再執行上面整段。
   ```bash
   cd "${KEY_WORKTREE:?先執行上面整段}" && test -n "$KEYGEN" && test -f registry/schema.json && node -e "$KEYGEN" | gh secret set REGISTRY_SIGNING_KEY --env registry-signing --repo "$REPO"
   ```
   檢查：應列出 `REGISTRY_SIGNING_KEY` 及更新時間（不會顯示其值）。
   ```bash
   gh secret list --env registry-signing --repo "$REPO"
   ```
   這只證明 secret 存在，不證明它與 `registry/signing-public-key.pem` 是同一對金鑰；兩者相符與否，第一次由第 9 步 workflow 的 sign job 證明。
   **代價**：私鑰只存在於這個 secret。如 secret 遺失或被刪除，擁有人須重新產生金鑰對；公鑰編入 App（P05）之後，還要先發佈一個帶有新公鑰的 App 版本，使用者才能再收到註冊表更新。這對金鑰與 App 更新用的金鑰不同，`sign.ts` 每次簽署都會檢查。
7. **設定 dev 的變數**（第 5 步檢查列出的輸出）：
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
9. **提交公鑰，首次發佈至 dev**。在第 6 步的 worktree commit 公鑰並 push：
   ```bash
   cd "${KEY_WORKTREE:?先執行第 6 步}" &&
     git add registry/signing-public-key.pem &&
     git commit -m "chore(registry): 提交註冊表的公鑰" &&
     git push -u origin chore/registry-public-key
   ```
   然後在 GitHub 開 PR（base 為 `main`）並合併。合併會觸發 Registry workflow。合併後約半分鐘，等候該次執行完成。下面只找由這次合併的 commit（`origin/main` 的最新 commit）觸發的執行，不會誤看較早的執行；如顯示未找到，稍候再執行本段：
   ```bash
   cd "${MAIN_CHECKOUT:?先執行第 0 步}" && git fetch origin main &&
     RUN_ID="$(gh run list --workflow registry.yml --repo "$REPO" --event push --commit "$(git rev-parse origin/main)" --limit 1 --json databaseId --jq '.[0].databaseId // empty')" &&
     gh run watch "${RUN_ID:?未找到這次合併觸發的執行，稍候再執行本段}" --repo "$REPO" --exit-status
   ```
   檢查：該次執行應以成功結束（validate、sign 及 publish-dev 都成功）。如 sign job 失敗並顯示 `the signing key does not match registry/signing-public-key.pem`，即 secret 與公鑰不是同一對：執行第 6 步重試的一行，再重複本步（commit 新的公鑰、push、開 PR 並合併）。
   然後確認 dev 提供的內容能以 `main` 上的公鑰驗證（`REGISTRY_URL` 設為第 5 步的 `RegistryUrl`）：
   ```bash
   REGISTRY_URL=<RegistryUrl>
   cd "${MAIN_CHECKOUT:?先執行第 0 步}" && git fetch origin main &&
     git show origin/main:registry/signing-public-key.pem > /tmp/registry-public-key.pem &&
     curl -fsS -D /tmp/registry.headers -o /tmp/registry.json "$REGISTRY_URL" &&
     SIG="$(grep -i '^x-registry-signature:' /tmp/registry.headers | cut -d' ' -f2 | tr -d '\r')" URL="$REGISTRY_URL" node -e '
     const { readFileSync } = require("node:fs");
     const { verify } = require("node:crypto");
     const body = readFileSync("/tmp/registry.json");
     const sig = Buffer.from(process.env.SIG ?? "", "base64");
     const ok = sig.length === 64 && verify(null, body, readFileSync("/tmp/registry-public-key.pem", "utf8"), sig);
     console.log(ok ? `signature ok, version ${JSON.parse(body).version}, from ${process.env.URL}` : `SIGNATURE BAD, from ${process.env.URL}`);'
   ```
   結果會顯示所檢查的網址：確認它是 dev 的 `RegistryUrl`。顯示 `signature ok` 後，移除第 6 步開的 worktree：
   ```bash
   cd "${MAIN_CHECKOUT:?先執行第 0 步}" && git worktree remove "${KEY_WORKTREE:?先執行第 6 步}"
   ```
10. **prod**。先確認 prod 的 Payment stack 已部署（檢查方法同第 4 步）：
    ```bash
    aws cloudformation list-exports --profile jarvis --region ap-southeast-1 \
      --query "Exports[?Name=='Jarvis-prod-ApiBaseUrl'].Value" --output text
    ```
    再部署 prod 的 Registry stack。與第 5 步一樣，第一段會確認主要 checkout 是最新的 `main`；第 9 步的合併之後須先 `git pull --ff-only`。同樣不可省略 `--exclusively`（原因見第 5 步）：
    ```bash
    cd "${MAIN_CHECKOUT:?先執行第 0 步}" &&
      git fetch origin main &&
      test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" &&
      test -z "$(git status --porcelain -- infra)" &&
      cd infra && npm ci && (cd lambda/registry && npm ci) && npm run build
    cd "${MAIN_CHECKOUT:?先執行第 0 步}/infra" && npx cdk diff --profile jarvis --context env=prod --exclusively Jarvis-prod-Registry
    cd "${MAIN_CHECKOUT:?先執行第 0 步}/infra" &&
      test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" &&
      test -z "$(git status --porcelain -- .)" &&
      npm run deploy:prod -- --exclusively Jarvis-prod-Registry
    ```
    檢查：應列出三項輸出，以 prod 的 `RegistryUrl` 執行的 `curl` 應顯示 `503`。
    ```bash
    aws cloudformation describe-stacks --profile jarvis --region ap-southeast-1 --stack-name Jarvis-prod-Registry \
      --query "Stacks[0].Outputs[].[OutputKey,OutputValue]" --output text
    curl -s -o /dev/null -w '%{http_code}\n' <prod 的 RegistryUrl>
    ```
    設定 `registry-prod` 的變數並檢查：
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
    約半分鐘後（新的執行要一點時間才會出現），找出最新一次手動執行：
    ```bash
    RUN="$(gh run list --workflow registry.yml --repo "$REPO" --event workflow_dispatch --limit 1 --json databaseId,createdAt --jq '.[] | "\(.databaseId) \(.createdAt)"')" && echo "${RUN:?未有手動執行的記錄，稍候再執行本行}"
    ```
    檢查：顯示的是執行編號及建立時間（UTC），時間應是剛才手動發佈的一刻；如仍是較早的一次執行，稍候再執行上面一行。確認後等候它完成：
    ```bash
    gh run watch "${RUN%% *}" --repo "$REPO" --exit-status
    ```
    檢查：該次執行應以成功結束（validate、sign 及 publish-prod 都成功）。然後把 `REGISTRY_URL` 設為 prod 的 `RegistryUrl`，再執行第 9 步確認 dev 內容的那一段（由 `cd "${MAIN_CHECKOUT…` 開始）；應顯示 `signature ok`，版本與 dev 相同，而且顯示的網址必須是 prod 的 `RegistryUrl`（如仍是 dev 的網址，即下面一行沒有生效，檢查的仍是 dev）。
    ```bash
    REGISTRY_URL=<prod 的 RegistryUrl>
    ```
