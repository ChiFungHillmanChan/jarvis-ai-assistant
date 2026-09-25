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
- JWT 驗證後的 `POST /checkout` 接受 `{tier, locale?}`，`POST /portal` 接受 `{locale?}`，身份只取已驗證的 `sub`。`locale` 只使用 `en` 或 `zh-HK`，其餘預設 `en`。成功回傳 `{url, session_id}`；已有訂閱的 Checkout 會回 Portal 並包含 `already_subscribed: true`。
- Webhook 必須訂閱 `checkout.session.completed`、`checkout.session.async_payment_succeeded`、`checkout.session.async_payment_failed`、`customer.subscription.created`、`customer.subscription.updated`、`customer.subscription.deleted`、`customer.subscription.paused`、`customer.subscription.resumed`、`invoice.paid`、`invoice.payment_failed`。請使用上述 API 版本的 endpoint，簽章 secret 必須與 endpoint 配對；事件的原始 body 不能先重新編碼。
- API key 需能讀取 customer、subscription、invoice、Checkout Session，並建立 customer／Checkout／Portal Session 及 expire Checkout Session。Lambda 的 Portal IAM 只讀取 user-meta 與 Stripe secret；Checkout／webhook 可更新 user-meta 的 billing 欄位及 lease。
- Checkout 以每帳戶 120 秒 lease 配合穩定 Stripe 冪等鍵串行處理；`billing_busy` 或 `billing_checkout_processing` 的 409 表示稍後重試。AWS 寫入失敗後會保存 attempt，優先回復已建立的 customer／session。未能確認的 attempt 超過 23 小時會回 503，必須先在 Stripe 核對及修復映射；不要直接刪除 attempt 再付款，以免越過 Stripe 冪等保證期限建立重複交易。已知 session 正常到期或訂閱取消後可重新付款。
- Webhook 不使用事件時間排序或事件中的舊 tier；每次重讀當前 Stripe 狀態並核對 customer／user。重送只重新寫入當前狀態，不建立付款；讀取、身份核對、鎖或寫入失敗回非 2xx，供 Stripe 重試。`past_due` 保留既有寬限政策；首次非同步付款未完成不授予付費 tier。tier 仍只是帳戶狀態，本輪沒有加入付費功能限制。

後續已部署 Stripe hosted locale 修正；新 Checkout／Portal session 使用 `en`／`zh-HK`。新付款嘗試持久保存 `checkoutStripeLocale`，舊嘗試恢復時維持省略該參數，避免改變 Stripe 冪等請求。部署只更新三個 billing Lambda 程式；部署後 CDK diff 為 0，Auth／Data 沒有部署。

最新 `npm test` 為 41 passed／0 failed，包含真正 handler 的本機 AWS／Stripe mock 與真實 CDK 合成範本的路由依賴測試；測試不載入憑證或發出 API 請求。TypeScript 建置及 `git diff --check` 通過。原生 App 的 Portal 期末取消、重啟後重新登入取得 Pro，以及新版 Portal 繁體中文介面均已實測。第一筆付款早於 locale 修正，新中文 Checkout session 尚待另行驗收。

Stripe 投遞紀錄補充：並行通知曾收到 HTTP 409 `billing_busy`；`invoice.paid` 及後續訂閱更新為 HTTP 200。手動重送同一 `checkout.session.completed` 及 `customer.subscription.created` 事件均回傳 HTTP 200。唯讀核對仍只有一筆測試訂閱，期末取消設定不變。歷史錯誤比例包含這些重試，不應把 Lambda Errors=0 說成所有通知首次都成功。正式 DMG、live 付款及其他異常流程仍需驗收。
