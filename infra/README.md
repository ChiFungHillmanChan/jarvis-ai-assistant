# JARVIS Infra (AWS CDK)

Cloud control plane for JARVIS. Deploys to account `759626151650` in `ap-southeast-1`.

## Stacks

- `Jarvis-<env>-Auth` -- Cognito user pool (managed login, Google IdP, account-link trigger) + KMS CMK
- `Jarvis-<env>-Data` -- S3 sync bucket + DynamoDB user meta + plugin registry
- `Jarvis-<env>-Observability` -- CloudWatch dashboard + budget alarms + SNS
- `Jarvis-<env>-Payment` -- control-plane HTTP API: Stripe webhook, JWT-authorised `/checkout`, `/portal` and `/me`

Only Auth, Data and Payment are deployed in sandbox; Observability and Audit are not.

`<env>` is one of `sandbox` / `dev` / `prod`.

## First-time setup

Pre-req: `jarvis-cdk-deployer` IAM user with admin policy（root account 開）。Local profile `jarvis-deploy` configured.

Google sign-in needs a Secrets Manager secret before the Auth stack deploys:
`jarvis/<env>/google-login` = `{"clientId": "...", "clientSecret": "..."}` from a Google
Cloud **Web application** client whose redirect URI is
`https://jarvis-automation-<env>.auth.ap-southeast-1.amazoncognito.com/oauth2/idpresponse`.
Keep that client in a project with only `openid email profile` scopes, published to
Production -- not in the Gmail project, whose Testing status caps users at 100.

```bash
cd infra
npm install
npm run build

# Bootstrap once per account/region
npm run bootstrap

# Deploy sandbox first to test (multi-stack app: name the stack)
npx cdk deploy --context env=sandbox --profile jarvis-deploy Jarvis-sandbox-Payment
```

## 預估 monthly cost (sandbox / dev)

| Resource | Cost |
|----------|------|
| KMS CMK | $1 |
| S3 (5GB free tier) | $0 |
| DynamoDB pay-per-request (low traffic) | <$1 |
| Cognito (free tier 50k MAU) | $0 |
| SNS + budgets | $0 |
| CloudWatch dashboard | $3 |
| **Total** | **~$5/mo per env** |

Activate credits cover this 100x.

## Destroy sandbox

```bash
npm run destroy:sandbox
```

`prod` stack 唔可以 destroy（KMS + S3 + DynamoDB removalPolicy = RETAIN）。

## Outputs

After deploy, outputs 寫入 `cdk.out/<stack>.out.json`. Tauri build script reads:
- `UserPoolId`, `UserPoolClientId` -> Cognito SDK config
- `DataKeyArn` -> envelope encryption client
- `SyncBucketName` -> sync target

See `scripts/sync-cdk-outputs.sh`.

## Releases bucket (public read)

The `jarvis-releases` bucket (`ap-east-1`, account 759626151650, separate from
the CDK stacks above) holds the signed DMG + updater tarball. `scripts/release.sh`
uploads there via the `jarvis-release-bot` profile.

**This is already configured** — the bucket has a scoped public-read policy (the
one mirrored in `releases-bucket-policy.json`: public `s3:GetObject` on the DMG /
tarball / `.sig` / `latest.json` only, no listing or writes) and Block Public
Access allows it (`BlockPublicPolicy=false`). Nothing to do for a normal release;
`release.sh` just uploads and the objects are publicly downloadable.

Re-apply the policy only if it is ever lost (use the `jarvis-deploy` profile,
which has bucket-policy permissions; `jarvis-release-bot` is upload-only):

```bash
aws s3api put-bucket-policy --bucket jarvis-releases \
  --policy file://infra/releases-bucket-policy.json \
  --profile jarvis-deploy
```

The update *manifest* (`latest.json`) that the app reads is NOT here — it is
served publicly from the website at `https://jarvis-automation.com/latest.json`.
This bucket only serves the binaries (and an optional mirror of the manifest).

## 付款設定與 sandbox 驗證（2026-09-23；live 未啟用）

付款 handler 沿用 Stripe SDK 17 與 API `2025-02-24.acacia`。本輪 `Jarvis-sandbox-Payment` 已部署，CloudFormation 狀態為 `UPDATE_COMPLETE`；sandbox API 為 `https://k2jn2wsgee.execute-api.ap-southeast-1.amazonaws.com`。正式 live 付款未啟用。本輪已完成原生 QA/debug App 的 sandbox 付款與自動更新方案驗收；這不等同正式 DMG 或 live 收款驗收。

已對真正 sandbox endpoint 驗證以下結果；沒有建立 App 使用者或修改 DynamoDB：

| 請求 | 實測結果 |
| --- | --- |
| 匿名 `GET /me` | HTTP 401 `Unauthorized` |
| 匿名 `POST /checkout` | HTTP 401 `Unauthorized` |
| 匿名 `POST /portal` | HTTP 401 `Unauthorized` |
| Webhook 缺少簽章 | HTTP 400 `missing_signature_or_body` |
| Webhook 錯誤簽章 | HTTP 400 `invalid_signature` |

AWS 唯讀核對確認三條帳戶路由均使用 JWT authorizer 及 `openid` scope；Webhook 不使用 JWT，由 handler 驗證 Stripe 簽章。`$default` Stage 的全域限流為每秒 10 次、burst 20；`POST /checkout` 與 `POST /portal` 各為每秒 2 次、burst 5。Stage 現已明確依賴這兩條路由，避免在路由尚未建立時套用 `RouteSettings`。

Checkout、Webhook、Portal 及 `/me` Lambda 均為 `Active`／`Successful`，使用 Node.js 24、256 MB；前三者 timeout 10 秒，`/me` 為 5 秒。已核對部署值：Pro `price_1TUsAuJCG4qg3rNcZVpOopcD`、Team `price_1TUsAwJCG4qg3rNcPY3YJiOB`；Checkout 與 Portal 使用專用 configuration `bpc_1UIu72JCG4qg3rNcRFPWhof9`。目前 sandbox Portal 提供發票、付款方式及期末取消管理，未開放方案切換。

新 restricted test key 已用合成客戶驗證 14 個 Stripe API 操作回傳 HTTP 200，涵蓋 Price／Product 讀取、Customer、未完成的 Checkout、Portal 及 Subscription／Invoice 列表。不存在合成 Subscription／Invoice ID 的 retrieve 回傳預期 HTTP 404；這項權限探測本身沒有完成付款。測試 Checkout 沒有完成付款，隨後已過期；這次建立的合成客戶亦已刪除。金鑰及原始 API 回應沒有寫入紀錄。


後續原生 QA/debug App 已使用既有 Google 帳戶登入，透過 Stripe hosted Checkout 以官方 `4242` 測試卡成功付款。Stripe Checkout 狀態為 `complete`／`paid`、`livemode=false`；官網中文成功頁正常顯示，App 自動由 Free 更新為 Pro 並顯示已確認付費方案。此流程使用 sandbox test mode，沒有真實扣款。

唯讀資料核對確認測試 customer `cus_VJXV69BBtwhrUT` 只對應一列 user-meta，`tier=pro`、`subscriptionStatus=active`，訂閱 `sub_1UIuRsJCG4qg3rNcvqIuFepL` 與該次 Checkout 完全相符。最後處理事件為 `evt_1UIuU5JCG4qg3rNc9glKRMn6`，資料更新時間為 `2026-09-23T18:01:35.455Z`。驗證沒有輸出 email、Cognito 身份或 JWT，亦沒有更改該測試訂閱。

`17:42–18:02 UTC` 的 Webhook Lambda 紀錄共 10 次完成執行，最長 1,904.41 ms，沒有 `ERROR`、timeout 或 `Billing operation failed` 紀錄；Lambda `Errors` 指標三個資料點總和為 0。目前沒有逐次 HTTP 回應碼日誌，因此這些數字不能代表每個 Webhook 都回傳 200；本次付款成功同步的證據是 Stripe 付款狀態、DynamoDB 對應資料及 App 自動更新三者一致。

- Checkout 與 webhook 共用 `STRIPE_PRO_PRICE_ID`、`STRIPE_TEAM_PRICE_ID`。CDK 接受 `stripeProPriceId`、`stripeTeamPriceId` context，亦接受同名環境變數。只有明確 `ENV=sandbox` 可回退至既有測試 ID；`dev`／`prod` 缺少有效設定會拒絕建立 Checkout。切換 Stripe 帳戶必須更新兩個價格 ID、Secrets Manager 的 API key／webhook secret，以及帳戶對應的 customer 資料；不能把舊帳戶 customer ID 原樣用於新帳戶。
- `STRIPE_PORTAL_CONFIGURATION_ID`（CDK context：`stripePortalConfigurationId`）指定 JARVIS 專用 Portal 設定，避免共用帳戶的其他產品出現在方案管理中。只有 sandbox 可省略並使用帳戶預設；其他環境缺少此值會拒絕開啟 Portal。Portal 中可升降級的產品、取消方式及按比例收費規則須由擁有人在 Stripe 設定，本輪沒有替使用者決定。
- `TRIAL_DAYS`（CDK context：`trialDays`，預設 28，只接受 1 至 730 的整數）是首次訂閱者的免費試用日數；更改只需重新部署 Payment stack，毋須發佈 App。`/me` 以同一設定顯示試用日數。
- Stripe restricted key 另需 **Promotion Codes: Read**，App 內「我有優惠碼」才可查核優惠碼。
- JWT 驗證後的 `POST /checkout` 接受 `{tier, locale?, code?}`，`POST /portal` 接受 `{locale?}`，身份只取已驗證的 `sub`。`locale` 只使用 `en` 或 `zh-HK`，其餘預設 `en`。成功回傳 `{url, session_id}`；已有訂閱的 Checkout 會回 Portal 並包含 `already_subscribed: true`；未能識別或被拒絕的 `code` 回 `400 invalid_code`；`TRIAL_DAYS` 設定無效時回 `503 billing_trial_not_configured`。
- Webhook 必須訂閱 `checkout.session.completed`、`checkout.session.async_payment_succeeded`、`checkout.session.async_payment_failed`、`customer.subscription.created`、`customer.subscription.updated`、`customer.subscription.deleted`、`customer.subscription.paused`、`customer.subscription.resumed`、`invoice.paid`、`invoice.payment_failed`。請使用上述 API 版本的 endpoint，簽章 secret 必須與 endpoint 配對；事件的原始 body 不能先重新編碼。
- API key 需能讀取 customer、subscription、invoice、Checkout Session，並建立 customer／Checkout／Portal Session 及 expire Checkout Session。Lambda 的 Portal IAM 只讀取 user-meta 與 Stripe secret；Checkout／webhook 可更新 user-meta 的 billing 欄位及 lease。
- Checkout 以每帳戶 120 秒 lease 配合穩定 Stripe 冪等鍵串行處理；`billing_busy` 或 `billing_checkout_processing` 的 409 表示稍後重試。AWS 寫入失敗後會保存 attempt，優先回復已建立的 customer／session。未能確認的 attempt 超過 23 小時會回 503，必須先在 Stripe 核對及修復映射；不要直接刪除 attempt 再付款，以免越過 Stripe 冪等保證期限建立重複交易。已知 session 正常到期或訂閱取消後可重新付款。
- Webhook 不使用事件時間排序或事件中的舊 tier；每次重讀當前 Stripe 狀態並核對 customer／user。重送只重新寫入當前狀態，不建立付款；讀取、身份核對、鎖或寫入失敗回非 2xx，供 Stripe 重試。`past_due` 保留既有寬限政策；首次非同步付款未完成不授予付費 tier。付費版 App 現已加入付費牆：帳戶未達 Pro 或 Team（包含試用期）前會擋在牆前。

後續已部署 Stripe hosted locale 修正；新 Checkout／Portal session 使用 `en`／`zh-HK`。新付款嘗試持久保存 `checkoutStripeLocale`，舊嘗試恢復時維持省略該參數，避免改變 Stripe 冪等請求。部署只更新三個 billing Lambda 程式；部署後 CDK diff 為 0，Auth／Data 沒有部署。

最新 `npm test` 為 41 passed／0 failed，包含真正 handler 的本機 AWS／Stripe mock 與真實 CDK 合成範本的路由依賴測試；測試不載入憑證或發出 API 請求。TypeScript 建置及 `git diff --check` 通過。原生 App 的 Portal 期末取消、重啟後重新登入取得 Pro，以及新版 Portal 繁體中文介面均已實測。第一筆付款早於 locale 修正，新中文 Checkout session 尚待另行驗收。

Stripe 投遞紀錄補充：並行通知曾收到 HTTP 409 `billing_busy`；`invoice.paid` 及後續訂閱更新為 HTTP 200。手動重送同一 `checkout.session.completed` 及 `customer.subscription.created` 事件均回傳 HTTP 200。唯讀核對仍只有一筆測試訂閱，期末取消設定不變。歷史錯誤比例包含這些重試，不應把 Lambda Errors=0 說成所有通知首次都成功。正式 DMG、live 付款及其他異常流程仍需驗收。

## JARVIS AI 營運（kill switch、全域暫停、安全服務）

以下指令一律加 `--profile jarvis-deploy`，region 為 `ap-southeast-1`（Bedrock 與 Budgets 以外的資源都在這裡）。`<env>` 為 `sandbox` / `dev` / `prod`。

**解除 Bedrock kill switch。** Bedrock 月度預算 `jarvis-bedrock-monthly`（US$100）的實際花費到 150% 時，budget action 會自動把 managed policy `jarvis-bedrock-kill-switch`（Deny `bedrock:InvokeModel*`）掛到 role `jarvis-<env>-ai-proxy`，所有 AI 請求隨即失敗。只有 sandbox stack 擁有這個預算與 policy。確認超支原因並決定恢復後：

```bash
# 1. 把 kill switch 從 proxy role 卸下，AI 即時恢復
aws iam detach-role-policy --profile jarvis-deploy \
  --role-name jarvis-<env>-ai-proxy \
  --policy-arn arn:aws:iam::759626151650:policy/jarvis-bedrock-kill-switch

# 2. action 已觸發後狀態不會自動回到待命；如需讓它能再次觸發，先查 action id 再 reset
aws budgets describe-budget-actions-for-budget --profile jarvis-deploy \
  --account-id 759626151650 --budget-name jarvis-bedrock-monthly
aws budgets execute-budget-action --profile jarvis-deploy \
  --account-id 759626151650 --budget-name jarvis-bedrock-monthly \
  --action-id <ACTION_ID> --execution-type RESET_BUDGET_ACTION
```

若本月花費仍高於 150%，reset 後 action 可能立即再次觸發；先處理花費來源，或接受它再次封鎖。不要為了繞過它而移除 policy 或預算。

**為所有人暫停 AI，或調整全域每日上限。** 兩者都在 DynamoDB 表 `jarvis-<env>-ai-config`，鍵為 `{"config": "current"}` 的那一列，設定以 JSON 字串存放在 `body` 欄位內：`globalPause`（`true` 即暫停，請求回 503 `paused`）及 `globalDailyCapMicros`（當日香港時間所有用戶合計的 billed 微美元上限，整數；shipped default 為 10000000，即 US$10）。proxy 最多快取 30 秒，`/me` 每次重讀，毋須部署；改動最遲約 30 秒生效。

```bash
# 1. 查看現有的列；沒有輸出代表尚未建立，此時改用 defaults（見第 2 步）
aws dynamodb get-item --profile jarvis-deploy --region ap-southeast-1 \
  --table-name jarvis-<env>-ai-config --key '{"config":{"S":"current"}}'

# 2a. 列已存在：以其 body 為底，只修改 globalPause / globalDailyCapMicros，其餘欄位原樣保留，整列寫成 ai-config-item.json
# 2b. 列不存在：在 infra/ 內執行以下指令，以 config/jarvis-ai.json 的完整 defaults 產生 ai-config-item.json
#     PAUSE 與 CAP 皆可省略；省略時沿用 defaults（不暫停、US$10）
PAUSE=true CAP=5000000 node -e 'const d=require("./config/jarvis-ai.json").defaults;if(process.env.PAUSE)d.globalPause=process.env.PAUSE==="true";if(process.env.CAP)d.globalDailyCapMicros=Number(process.env.CAP);if(!Number.isSafeInteger(d.globalDailyCapMicros)||d.globalDailyCapMicros<=0)throw new Error("CAP must be a positive integer");require("fs").writeFileSync("ai-config-item.json",JSON.stringify({config:{S:"current"},body:{S:JSON.stringify(d)}}))'

# 3. 整列寫入
aws dynamodb put-item --profile jarvis-deploy --region ap-southeast-1 \
  --table-name jarvis-<env>-ai-config --item file://ai-config-item.json
```

`body` 必須是完整、有效的設定，不可只寫 `globalPause` 或 `globalDailyCapMicros` 兩個欄位；內容無效時 proxy 會失效關閉（fail closed），等同全域暫停。列不存在時 proxy 使用 `config/jarvis-ai.json` 的 defaults，所以只有要暫停或改上限時才需建立這一列。`ai-config-item.json` 寫入後即可刪除，不要提交。

**GuardDuty 與 IAM Access Analyzer 不在 CDK 內，須用 CLI 在 `ap-southeast-1` 與 `ap-northeast-1` 各啟用一次**（每個 region 各自獨立；GuardDuty 未啟用時，finding 警報規則不會收到任何事件）：

```bash
for r in ap-southeast-1 ap-northeast-1; do
  aws guardduty create-detector --enable --region "$r" --profile jarvis-deploy
  aws accessanalyzer create-analyzer --type ACCOUNT \
    --analyzer-name jarvis-account-analyzer --region "$r" --profile jarvis-deploy
done
```

指令重複執行會因已存在而報錯，這是正常的；先用 `aws guardduty list-detectors` 與 `aws accessanalyzer list-analyzers` 查看是否已啟用。

警報涵蓋範圍有限：GuardDuty finding 的警報規則只存在於 `ap-southeast-1`；`ap-northeast-1` 的 GuardDuty finding 及所有 IAM Access Analyzer finding 只會出現在 console，不會發出電郵，須定期手動查看。
