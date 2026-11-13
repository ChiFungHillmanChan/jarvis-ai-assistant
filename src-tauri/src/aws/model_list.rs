//! The list behind the Bedrock model picker (Settings > Models > Provider
//! priority): the inference profiles and the text models AWS lists for the
//! saved region, or a short built-in list when that cannot be had.
//!
//! The two calls, as the Amazon Bedrock API Reference describes them (read
//! 2026-11-13):
//! - ListInferenceProfiles, `GET /inference-profiles`, "Returns a list of
//!   inference profiles that you can use"; paged with `nextToken`, filtered
//!   by `type` (SYSTEM_DEFINED or APPLICATION). Each summary has an id, a
//!   name, a status and the ARNs of the models it routes to; nothing in it
//!   says whether the account has been given access to those models.
//! - ListFoundationModels, `GET /foundation-models`, "Lists Amazon Bedrock
//!   foundation models that you can use"; no paging. Each summary has the
//!   id, the name, the provider, the input and output modalities, the
//!   inference types (ON_DEMAND, PROVISIONED) and the lifecycle status; again
//!   nothing about access granted to the account.
//!
//! So the picker shows what AWS lists for the region, and never says the
//! account may use a model. IAM actions (Service Authorization Reference):
//! `bedrock:ListInferenceProfiles`, `bedrock:ListFoundationModels`.
//!
//! The request carries the region and the signed credentials, nothing else:
//! no mail, prompt or usage. Nothing here is stored; the frontend keeps the
//! result while the section is open. Credentials and the SDK's own error text
//! are never logged or returned: the frontend gets a reason code and words
//! it chooses itself.

use crate::ai::models;
use crate::aws::credentials::{self, AwsCredentials};
use crate::db::Database;
use aws_config::BehaviorVersion;
use aws_sdk_bedrock::config::Region;
use aws_sdk_bedrock::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_bedrock::types::{
    FoundationModelLifecycleStatus, FoundationModelSummary, InferenceProfileStatus,
    InferenceProfileSummary, InferenceProfileType, InferenceType, ModelModality,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// The whole list, both calls and every page, must arrive within this.
pub const LIST_TIME_LIMIT: Duration = Duration::from_secs(10);

/// A guard against a service that keeps handing out new page tokens.
const MAX_PAGES: usize = 20;

/// The IAM actions the two calls need, as the Service Authorization
/// Reference names them.
pub const LIST_PROFILES_ACTION: &str = "bedrock:ListInferenceProfiles";
pub const LIST_MODELS_ACTION: &str = "bedrock:ListFoundationModels";

/// The two ids migration V13 seeds into `workload_routing`, and the default.
/// The built-in list holds these and nothing else: ids the app already ships.
const SEEDED_HAIKU: &str = "global.anthropic.claude-haiku-4-5-20251001-v1:0";
const SEEDED_NOVA_MICRO: &str = "apac.amazon.nova-micro-v1:0";
const BUILT_IN: [&str; 3] = [models::BEDROCK_DEFAULT, SEEDED_HAIKU, SEEDED_NOVA_MICRO];

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Profile,
    Model,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
    pub kind: Kind,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Aws,
    Fallback,
}

/// Why the built-in list is shown.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NoCredentials,
    UnreadableCredentials,
    CredentialsRefused,
    NotAllowed,
    Offline,
    TimedOut,
    Failed,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DefaultModel {
    pub id: String,
    pub name: Option<&'static str>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ModelList {
    pub source: Source,
    /// The region asked, or whose geography the built-in ids use. None when
    /// no region is known (no credentials, or unreadable ones).
    pub region: Option<String>,
    pub reason: Option<Reason>,
    /// For `NotAllowed`: the IAM actions AWS refused, in AWS's names.
    pub missing_permissions: Vec<&'static str>,
    /// What the empty choice runs (ai/mod.rs `bedrock_model`).
    pub default_model: DefaultModel,
    /// Inference profiles first, then plain model ids.
    pub models: Vec<ModelChoice>,
}

fn default_model() -> DefaultModel {
    let id = crate::ai::bedrock::default_model();
    let name = models::label_for(&id);
    DefaultModel { id, name }
}

/// The built-in list, with each geography-bound id pointed at the saved
/// region's geography as `apply_inference_profile` does when a request is
/// sent. With no region known the ids stay as shipped.
pub fn built_in(region: Option<&str>) -> Vec<ModelChoice> {
    BUILT_IN
        .iter()
        .map(|shipped| {
            let id = match region {
                Some(region) => crate::ai::apply_inference_profile(shipped, credentials::inference_profile_for_region(region)),
                None => shipped.to_string(),
            };
            let name = models::label_for(shipped).unwrap_or(shipped).to_string();
            ModelChoice { id, name, kind: Kind::Profile }
        })
        .collect()
}

fn fallback(region: Option<&str>, reason: Reason, missing_permissions: Vec<&'static str>) -> ModelList {
    ModelList {
        source: Source::Fallback,
        region: region.map(str::to_string),
        reason: Some(reason),
        missing_permissions,
        default_model: default_model(),
        models: built_in(region),
    }
}

/// The picker's list for the saved credentials. Sends nothing when none are
/// saved or they cannot be read.
pub async fn list_for(db: &Database, limit: Duration) -> ModelList {
    match credentials::load_from(db) {
        Ok(Some(creds)) => list_with(&creds, limit).await,
        Ok(None) => fallback(None, Reason::NoCredentials, Vec::new()),
        Err(_) => {
            // The Keychain or database text stays out of the log as well.
            log::warn!("Bedrock model list: the saved AWS credentials could not be read; showing the built-in list");
            fallback(None, Reason::UnreadableCredentials, Vec::new())
        }
    }
}

pub(crate) async fn list_with(creds: &AwsCredentials, limit: Duration) -> ModelList {
    let region = creds.region.as_str();
    let client = client_for(creds).await;
    let both = async { tokio::join!(list_profiles(&client), list_foundation_models(&client)) };
    match tokio::time::timeout(limit, both).await {
        Ok((Ok(profiles), Ok(foundation))) => ModelList {
            source: Source::Aws,
            region: Some(region.to_string()),
            reason: None,
            missing_permissions: Vec::new(),
            default_model: default_model(),
            models: choices(&profiles, &foundation),
        },
        Ok((profiles, foundation)) => {
            let mut denied = Vec::new();
            if matches!(&profiles, Err(f) if f.reason == Reason::NotAllowed) {
                denied.push(LIST_PROFILES_ACTION);
            }
            if matches!(&foundation, Err(f) if f.reason == Reason::NotAllowed) {
                denied.push(LIST_MODELS_ACTION);
            }
            let failure = profiles.err().or(foundation.err()).expect("one of the two calls failed");
            if denied.is_empty() {
                fallback(Some(region), failure.reason, Vec::new())
            } else {
                fallback(Some(region), Reason::NotAllowed, denied)
            }
        }
        Err(_) => {
            log::warn!("Bedrock model list: AWS did not answer within {} s", limit.as_secs_f32());
            fallback(Some(region), Reason::TimedOut, Vec::new())
        }
    }
}

/// A control-plane client for the saved credentials and region, built as
/// `ai::bedrock::client_for` builds the runtime one. No automatic retries:
/// the user can load the list again.
async fn client_for(creds: &AwsCredentials) -> aws_sdk_bedrock::Client {
    let provider = aws_credential_types::Credentials::new(
        creds.access_key_id.clone(),
        creds.secret_access_key.clone(),
        None,
        None,
        "jarvis-aws-creds",
    );
    let loader = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(creds.region.clone()))
        .credentials_provider(provider)
        .retry_config(aws_smithy_types::retry::RetryConfig::disabled());
    #[cfg(test)]
    let loader = match crate::ai::bedrock::tests::LOOPBACK.with(|endpoint| endpoint.borrow().clone()) {
        Some(endpoint) => loader.endpoint_url(endpoint),
        None => loader,
    };
    aws_sdk_bedrock::Client::new(&loader.load().await)
}

#[derive(Debug)]
struct Failure {
    reason: Reason,
}

/// What a failed call means, from the service's error code or the kind of
/// failure. Only the code or the kind is logged, never the SDK's text.
fn failure<E: ProvideErrorMetadata, R>(call: &str, err: &SdkError<E, R>) -> Failure {
    let code = err.as_service_error().and_then(|e| e.code()).unwrap_or_default();
    let (reason, kind) = match err {
        SdkError::ServiceError(_) => match code {
            "AccessDeniedException" | "NotAuthorized" => (Reason::NotAllowed, format!("code {code}")),
            "UnrecognizedClientException" | "InvalidSignatureException" | "IncompleteSignature" | "ExpiredTokenException" => {
                (Reason::CredentialsRefused, format!("code {code}"))
            }
            "" => (Reason::Failed, "service error without a code".to_string()),
            _ => (Reason::Failed, format!("code {code}")),
        },
        SdkError::TimeoutError(_) => (Reason::TimedOut, "timed out".to_string()),
        SdkError::DispatchFailure(dispatch) if dispatch.is_timeout() => (Reason::TimedOut, "timed out connecting".to_string()),
        SdkError::DispatchFailure(_) => (Reason::Offline, "could not connect".to_string()),
        SdkError::ResponseError(_) => (Reason::Failed, "unreadable response".to_string()),
        SdkError::ConstructionFailure(_) => (Reason::Failed, "request not built".to_string()),
        _ => (Reason::Failed, "other failure".to_string()),
    };
    log::warn!("Bedrock model list: {call} failed: {kind}");
    Failure { reason }
}

/// Every page of the system-defined inference profiles.
async fn list_profiles(client: &aws_sdk_bedrock::Client) -> Result<Vec<InferenceProfileSummary>, Failure> {
    let mut summaries = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page = client
            .list_inference_profiles()
            .type_equals(InferenceProfileType::SystemDefined)
            .max_results(1000)
            .set_next_token(token.clone())
            .send()
            .await
            .map_err(|err| failure("ListInferenceProfiles", &err))?;
        summaries.extend(page.inference_profile_summaries().iter().cloned());
        match page.next_token().filter(|next| !next.is_empty()) {
            Some(next) if token.as_deref() != Some(next) => token = Some(next.to_string()),
            _ => return Ok(summaries),
        }
    }
    log::warn!("Bedrock model list: ListInferenceProfiles stopped after {MAX_PAGES} pages");
    Ok(summaries)
}

/// The region's foundation models, all of them: the output modalities also
/// decide which profiles are shown. The call has no paging.
async fn list_foundation_models(client: &aws_sdk_bedrock::Client) -> Result<Vec<FoundationModelSummary>, Failure> {
    let out = client
        .list_foundation_models()
        .send()
        .await
        .map_err(|err| failure("ListFoundationModels", &err))?;
    Ok(out.model_summaries().to_vec())
}

/// The foundation-model id in a model ARN
/// (`arn:aws:bedrock:<region>::foundation-model/<id>`).
fn model_id_in_arn(arn: &str) -> Option<&str> {
    arn.split_once(":foundation-model/").map(|(_, id)| id)
}

fn outputs_text(model: &FoundationModelSummary) -> bool {
    model.output_modalities().contains(&ModelModality::Text)
}

/// Profiles, then plain model ids, each sorted by name.
/// - A profile is shown unless ListFoundationModels lists every model it
///   routes to and none of them answers in text (an image or embedding
///   profile). A profile whose models the region does not list is shown.
/// - A plain model id is shown when it answers in text, can be called on
///   demand (a model offered only with provisioned throughput cannot be
///   called by its id), and is not LEGACY.
fn choices(profiles: &[InferenceProfileSummary], foundation: &[FoundationModelSummary]) -> Vec<ModelChoice> {
    let text_by_id: HashMap<&str, bool> = foundation.iter().map(|m| (m.model_id(), outputs_text(m))).collect();
    let mut seen = HashSet::new();
    let mut listed: Vec<ModelChoice> = profiles
        .iter()
        .filter(|p| *p.status() == InferenceProfileStatus::Active)
        .filter(|p| {
            let known: Vec<bool> = p
                .models()
                .iter()
                .filter_map(|m| m.model_arn().and_then(model_id_in_arn))
                .filter_map(|id| text_by_id.get(id).copied())
                .collect();
            known.is_empty() || known.len() < p.models().len() || known.iter().any(|text| *text)
        })
        .map(|p| ModelChoice { id: p.inference_profile_id().to_string(), name: p.inference_profile_name().to_string(), kind: Kind::Profile })
        .collect();
    let mut plain: Vec<ModelChoice> = foundation
        .iter()
        .filter(|m| outputs_text(m))
        .filter(|m| m.inference_types_supported().contains(&InferenceType::OnDemand))
        .filter(|m| m.model_lifecycle().map(|l| *l.status() != FoundationModelLifecycleStatus::Legacy).unwrap_or(true))
        .map(|m| ModelChoice {
            id: m.model_id().to_string(),
            name: m.model_name().filter(|n| !n.trim().is_empty()).unwrap_or(m.model_id()).to_string(),
            kind: Kind::Model,
        })
        .collect();
    let by_name = |a: &ModelChoice, b: &ModelChoice| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id));
    listed.sort_by(by_name);
    plain.sort_by(by_name);
    listed.extend(plain);
    listed.retain(|choice| seen.insert(choice.id.clone()));
    listed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::bedrock::tests::{fixture_creds, LOOPBACK};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// What a stand-in for the Bedrock control plane received.
    #[derive(Clone, Debug)]
    struct Seen {
        /// Path and query, as sent.
        target: String,
        authorization: String,
    }

    struct Reply {
        status: u16,
        error_type: Option<&'static str>,
        body: Value,
    }

    fn ok(body: Value) -> Reply {
        Reply { status: 200, error_type: None, body }
    }

    fn refused(status: u16, code: &'static str) -> Reply {
        Reply { status, error_type: Some(code), body: json!({"message": "fixture refusal"}) }
    }

    /// Clears the endpoint when a test ends, so a later test on the same
    /// thread cannot reach a server that is gone.
    struct Endpoint;
    impl Drop for Endpoint {
        fn drop(&mut self) {
            LOOPBACK.with(|slot| *slot.borrow_mut() = None);
        }
    }

    fn point_at(endpoint: String) -> Endpoint {
        LOOPBACK.with(|slot| *slot.borrow_mut() = Some(endpoint));
        Endpoint
    }

    /// A stand-in for the Bedrock control plane on 127.0.0.1 that answers
    /// every request with `reply(path and query)` and records each request.
    async fn control_plane(reply: impl Fn(&str) -> Reply + Send + Sync + 'static) -> (Arc<Mutex<Vec<Seen>>>, Endpoint) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let reply = Arc::new(reply);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { return };
                let record = record.clone();
                let reply = reply.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0; 8192];
                    while !String::from_utf8_lossy(&request).contains("\r\n\r\n") {
                        let count = stream.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..count]);
                    }
                    let text = String::from_utf8_lossy(&request).to_string();
                    let target = text.lines().next().unwrap_or_default().split(' ').nth(1).unwrap_or_default().to_string();
                    let authorization = text
                        .lines()
                        .find_map(|line| line.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.trim().to_string()))
                        .unwrap_or_default();
                    record.lock().unwrap().push(Seen { target: target.clone(), authorization });
                    let answer = reply(&target);
                    let body = answer.body.to_string();
                    let error_header = answer.error_type.map(|code| format!("x-amzn-ErrorType: {code}\r\n")).unwrap_or_default();
                    let reason = if answer.status == 200 { "OK" } else { "Error" };
                    let head = format!(
                        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\n{error_header}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        answer.status,
                        body.len()
                    );
                    stream.write_all(head.as_bytes()).await.unwrap();
                    stream.write_all(body.as_bytes()).await.unwrap();
                });
            }
        });
        (seen, point_at(endpoint))
    }

    fn profile(id: &str, name: &str, model: &str) -> Value {
        json!({
            "inferenceProfileId": id,
            "inferenceProfileName": name,
            "inferenceProfileArn": format!("arn:aws:bedrock:ap-southeast-1:000000000000:inference-profile/{id}"),
            "models": [
                {"modelArn": format!("arn:aws:bedrock:ap-southeast-1::foundation-model/{model}")},
                {"modelArn": format!("arn:aws:bedrock:ap-northeast-1::foundation-model/{model}")}
            ],
            "status": "ACTIVE",
            "type": "SYSTEM_DEFINED"
        })
    }

    fn foundation(id: &str, name: &str, output: &str, inference: &str, lifecycle: &str) -> Value {
        json!({
            "modelArn": format!("arn:aws:bedrock:ap-southeast-1::foundation-model/{id}"),
            "modelId": id,
            "modelName": name,
            "providerName": "Fixture",
            "inputModalities": ["TEXT"],
            "outputModalities": [output],
            "responseStreamingSupported": true,
            "customizationsSupported": [],
            "inferenceTypesSupported": [inference],
            "modelLifecycle": {"status": lifecycle}
        })
    }

    /// The foundation models every test below lists: one of each kind the
    /// picker keeps or drops.
    fn region_models() -> Value {
        json!({"modelSummaries": [
            foundation("anthropic.claude-haiku-4-5-20251001-v1:0", "Claude Haiku 4.5", "TEXT", "ON_DEMAND", "ACTIVE"),
            foundation("amazon.nova-micro-v1:0", "Nova Micro", "TEXT", "ON_DEMAND", "ACTIVE"),
            foundation("fixture.image-v1:0", "Fixture Image", "IMAGE", "ON_DEMAND", "ACTIVE"),
            foundation("fixture.old-text-v1:0", "Fixture Old", "TEXT", "ON_DEMAND", "LEGACY"),
            foundation("fixture.provisioned-text-v1:0", "Fixture Provisioned", "TEXT", "PROVISIONED", "ACTIVE")
        ]})
    }

    fn choice(id: &str, name: &str, kind: Kind) -> ModelChoice {
        ModelChoice { id: id.into(), name: name.into(), kind }
    }

    /// Both calls read into names and ids: profiles first, then plain ids,
    /// each by name. Breaks if `choices` drops the profile ordering, keeps an
    /// image profile or model, or keeps a LEGACY or provisioned-only model.
    #[tokio::test]
    async fn both_list_calls_are_read_into_names_and_ids_profiles_first() {
        let (seen, _endpoint) = control_plane(|target| {
            if target.starts_with("/inference-profiles") {
                ok(json!({"inferenceProfileSummaries": [
                    profile("global.anthropic.claude-haiku-4-5-20251001-v1:0", "Global Claude Haiku 4.5", "anthropic.claude-haiku-4-5-20251001-v1:0"),
                    profile("apac.amazon.nova-micro-v1:0", "APAC Nova Micro", "amazon.nova-micro-v1:0"),
                    profile("apac.fixture.image-v1:0", "APAC Fixture Image", "fixture.image-v1:0"),
                    profile("apac.fixture.elsewhere-v1:0", "APAC Fixture Elsewhere", "fixture.elsewhere-v1:0")
                ]}))
            } else {
                ok(region_models())
            }
        })
        .await;
        let list = list_with(&fixture_creds("ap-southeast-1"), LIST_TIME_LIMIT).await;
        assert_eq!(list.source, Source::Aws);
        assert_eq!((list.region.as_deref(), list.reason), (Some("ap-southeast-1"), None));
        assert!(list.missing_permissions.is_empty());
        assert_eq!(
            list.models,
            vec![
                choice("apac.fixture.elsewhere-v1:0", "APAC Fixture Elsewhere", Kind::Profile),
                choice("apac.amazon.nova-micro-v1:0", "APAC Nova Micro", Kind::Profile),
                choice("global.anthropic.claude-haiku-4-5-20251001-v1:0", "Global Claude Haiku 4.5", Kind::Profile),
                choice("anthropic.claude-haiku-4-5-20251001-v1:0", "Claude Haiku 4.5", Kind::Model),
                choice("amazon.nova-micro-v1:0", "Nova Micro", Kind::Model),
            ],
            "the image profile and the image, LEGACY and provisioned-only models are left out"
        );
        let targets: Vec<String> = seen.lock().unwrap().iter().map(|s| s.target.clone()).collect();
        assert_eq!(targets.len(), 2, "one request per call: {targets:?}");
        assert!(targets.iter().any(|t| t.starts_with("/inference-profiles?") && t.contains("type=SYSTEM_DEFINED") && t.contains("maxResults=1000")), "{targets:?}");
        assert!(targets.iter().any(|t| t == "/foundation-models"), "no filter on the foundation models: {targets:?}");
    }

    /// The profiles call is followed through its pages. Breaks if
    /// `list_profiles` stops after the first page or does not send the token.
    #[tokio::test]
    async fn more_than_one_page_of_profiles_is_followed() {
        let (seen, _endpoint) = control_plane(|target| {
            if target.starts_with("/inference-profiles") && target.contains("nextToken=page-2") {
                ok(json!({"inferenceProfileSummaries": [profile("apac.amazon.nova-micro-v1:0", "APAC Nova Micro", "amazon.nova-micro-v1:0")]}))
            } else if target.starts_with("/inference-profiles") {
                ok(json!({"inferenceProfileSummaries": [profile("global.anthropic.claude-haiku-4-5-20251001-v1:0", "Global Claude Haiku 4.5", "anthropic.claude-haiku-4-5-20251001-v1:0")], "nextToken": "page-2"}))
            } else {
                ok(json!({"modelSummaries": []}))
            }
        })
        .await;
        let list = list_with(&fixture_creds("ap-southeast-1"), LIST_TIME_LIMIT).await;
        assert_eq!(list.source, Source::Aws);
        let ids: Vec<&str> = list.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["apac.amazon.nova-micro-v1:0", "global.anthropic.claude-haiku-4-5-20251001-v1:0"]);
        let pages: Vec<String> = seen.lock().unwrap().iter().filter(|s| s.target.starts_with("/inference-profiles")).map(|s| s.target.clone()).collect();
        assert_eq!(pages.len(), 2, "{pages:?}");
        assert!(pages.iter().all(|p| p.contains("type=SYSTEM_DEFINED")), "every page keeps the filter: {pages:?}");
        assert_eq!(pages.iter().filter(|p| p.contains("nextToken=page-2")).count(), 1, "{pages:?}");
    }

    /// A key that may not list gets the built-in list, naming the refused
    /// action as AWS names it. Breaks if the AccessDeniedException arm or the
    /// `denied` names are removed, or a partial AWS list is shown instead.
    #[tokio::test]
    async fn a_key_not_allowed_to_list_gets_the_built_in_list_naming_the_permission() {
        let (_seen, _endpoint) = control_plane(|target| {
            if target.starts_with("/inference-profiles") {
                refused(403, "AccessDeniedException")
            } else {
                ok(region_models())
            }
        })
        .await;
        let list = list_with(&fixture_creds("us-east-1"), LIST_TIME_LIMIT).await;
        assert_eq!((list.source, list.reason), (Source::Fallback, Some(Reason::NotAllowed)));
        assert_eq!(list.missing_permissions, vec!["bedrock:ListInferenceProfiles"]);
        assert_eq!(list.models, built_in(Some("us-east-1")));
        assert_eq!(list.region.as_deref(), Some("us-east-1"));
        drop(_endpoint);

        let (_seen, _endpoint) = control_plane(|_| refused(403, "AccessDeniedException")).await;
        let list = list_with(&fixture_creds("us-east-1"), LIST_TIME_LIMIT).await;
        assert_eq!(list.missing_permissions, vec!["bedrock:ListInferenceProfiles", "bedrock:ListFoundationModels"]);
    }

    /// Credentials AWS does not accept, and any other service error, also get
    /// the built-in list, each with its own reason. Breaks if the codes are
    /// read as "not allowed" or the error arm is dropped.
    #[tokio::test]
    async fn refused_credentials_and_other_errors_get_the_built_in_list() {
        let (_seen, _endpoint) = control_plane(|_| refused(403, "UnrecognizedClientException")).await;
        let list = list_with(&fixture_creds("ap-southeast-1"), LIST_TIME_LIMIT).await;
        assert_eq!((list.source, list.reason), (Source::Fallback, Some(Reason::CredentialsRefused)));
        assert!(list.missing_permissions.is_empty());
        drop(_endpoint);

        let (_seen, _endpoint) = control_plane(|_| refused(500, "InternalServerException")).await;
        let list = list_with(&fixture_creds("ap-southeast-1"), LIST_TIME_LIMIT).await;
        assert_eq!((list.source, list.reason), (Source::Fallback, Some(Reason::Failed)));
    }

    /// A control plane that never answers: the time limit gives the built-in
    /// list. Breaks if `tokio::time::timeout` is taken off the two calls.
    #[tokio::test]
    async fn a_list_that_does_not_arrive_in_time_gets_the_built_in_list() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let _endpoint = point_at(format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port()));
        let held = tokio::spawn(async move {
            let mut open = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                open.push(stream);
            }
        });
        let list = tokio::time::timeout(Duration::from_secs(5), list_with(&fixture_creds("ap-southeast-1"), Duration::from_millis(300)))
            .await
            .expect("the 300 ms limit ended the wait, not this test's 5 s");
        assert_eq!((list.source, list.reason), (Source::Fallback, Some(Reason::TimedOut)));
        assert_eq!(list.models, built_in(Some("ap-southeast-1")));
        held.abort();
    }

    /// Nothing listening: the built-in list, with "offline". Breaks if the
    /// DispatchFailure arm is removed (it would read as "failed").
    #[tokio::test]
    async fn a_refused_connection_gets_the_built_in_list() {
        let port = {
            let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        let _endpoint = point_at(format!("http://127.0.0.1:{port}"));
        let list = list_with(&fixture_creds("eu-west-1"), LIST_TIME_LIMIT).await;
        assert_eq!((list.source, list.reason), (Source::Fallback, Some(Reason::Offline)));
        assert_eq!(list.models, built_in(Some("eu-west-1")));
    }

    fn database(with_credentials_row: Option<&str>) -> Database {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE aws_credentials (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                access_key_id TEXT NOT NULL,
                secret_access_key TEXT NOT NULL,
                region TEXT NOT NULL DEFAULT 'ap-southeast-1',
                inference_profile TEXT NOT NULL DEFAULT 'apac',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE provider_chain (position INTEGER, provider_type TEXT, model_id TEXT);
            INSERT INTO provider_chain VALUES (0, 'bedrock', 'eu.fixture.saved-earlier-v1:0');",
        )
        .unwrap();
        if let Some(region) = with_credentials_row {
            conn.execute(
                "INSERT INTO aws_credentials (id, access_key_id, secret_access_key, region, inference_profile) VALUES (1, 'AKIAFIXTURENOTREAL', '__keychain__', ?1, 'apac')",
                [region],
            )
            .unwrap();
        }
        Database { conn: std::sync::Mutex::new(conn) }
    }

    fn saved_chain(db: &Database) -> Vec<(i64, String, String)> {
        let conn = db.conn.lock().unwrap();
        let mut statement = conn.prepare("SELECT position, provider_type, model_id FROM provider_chain ORDER BY position").unwrap();
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// No saved credentials: the built-in list, and no request at all.
    /// Breaks if `list_for` builds a client before it has credentials.
    #[tokio::test]
    async fn no_saved_credentials_gets_the_built_in_list_and_sends_nothing() {
        let (seen, _endpoint) = control_plane(|_| ok(region_models())).await;
        let list = list_for(&database(None), LIST_TIME_LIMIT).await;
        assert_eq!((list.source, list.reason, list.region.as_deref()), (Source::Fallback, Some(Reason::NoCredentials), None));
        assert_eq!(list.models, built_in(None));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(seen.lock().unwrap().is_empty(), "nothing was sent");
    }

    /// The saved region is the one asked: the client is built for it, so the
    /// signature's credential scope names it. Breaks if `client_for` takes
    /// the region from anywhere but the saved credentials.
    #[tokio::test]
    // KEYCHAIN_GUARD serialises the process-wide keychain stand-in across
    // tests, as in aws::credentials; this test's runtime has one thread, so
    // holding it over an await cannot deadlock.
    #[allow(clippy::await_holding_lock)]
    async fn the_saved_region_is_the_region_asked() {
        let _guard = crate::aws::credentials::tests::KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        crate::security::keychain::KeychainStore::put("com.jarvis.aws", "secret_access_key", "fixture-secret-not-real").unwrap();
        let (seen, _endpoint) = control_plane(|target| {
            if target.starts_with("/inference-profiles") {
                ok(json!({"inferenceProfileSummaries": []}))
            } else {
                ok(json!({"modelSummaries": []}))
            }
        })
        .await;
        let db = database(Some("ap-northeast-1"));
        let list = list_for(&db, LIST_TIME_LIMIT).await;
        let _ = crate::security::keychain::KeychainStore::delete("com.jarvis.aws", "secret_access_key");
        assert_eq!((list.source, list.region.as_deref()), (Source::Aws, Some("ap-northeast-1")));
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        for request in &seen {
            assert!(request.authorization.contains("Credential=AKIAFIXTURENOTREAL/"), "signed with the saved key id");
            assert!(request.authorization.contains("/ap-northeast-1/bedrock/aws4_request"), "scope names the saved region: {}", request.authorization);
            assert!(!request.authorization.contains("fixture-secret-not-real"), "the secret is never sent");
        }
    }

    /// Credentials that cannot be read (a row without its Keychain item):
    /// the built-in list, nothing sent. Breaks if the Err arm of `list_for`
    /// is folded into "no credentials" or tries a request.
    #[tokio::test]
    // KEYCHAIN_GUARD serialises the process-wide keychain stand-in across
    // tests, as in aws::credentials; this test's runtime has one thread, so
    // holding it over an await cannot deadlock.
    #[allow(clippy::await_holding_lock)]
    async fn unreadable_credentials_get_the_built_in_list_and_send_nothing() {
        let _guard = crate::aws::credentials::tests::KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = crate::security::keychain::KeychainStore::delete("com.jarvis.aws", "secret_access_key");
        let (seen, _endpoint) = control_plane(|_| ok(region_models())).await;
        let list = list_for(&database(Some("us-east-1")), LIST_TIME_LIMIT).await;
        assert_eq!((list.source, list.reason), (Source::Fallback, Some(Reason::UnreadableCredentials)));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(seen.lock().unwrap().is_empty());
    }

    /// An id saved earlier that AWS does not list stays saved: listing reads
    /// the credentials and nothing else, and writes nothing. Breaks if the
    /// list code ever rewrites the chain row to a listed id.
    #[tokio::test]
    // KEYCHAIN_GUARD serialises the process-wide keychain stand-in across
    // tests, as in aws::credentials; this test's runtime has one thread, so
    // holding it over an await cannot deadlock.
    #[allow(clippy::await_holding_lock)]
    async fn an_id_saved_earlier_that_is_not_listed_is_kept() {
        let _guard = crate::aws::credentials::tests::KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        crate::security::keychain::KeychainStore::put("com.jarvis.aws", "secret_access_key", "fixture-secret-not-real").unwrap();
        let (_seen, _endpoint) = control_plane(|target| {
            if target.starts_with("/inference-profiles") {
                ok(json!({"inferenceProfileSummaries": [profile("apac.amazon.nova-micro-v1:0", "APAC Nova Micro", "amazon.nova-micro-v1:0")]}))
            } else {
                ok(region_models())
            }
        })
        .await;
        let db = database(Some("ap-southeast-1"));
        let before = saved_chain(&db);
        let list = list_for(&db, LIST_TIME_LIMIT).await;
        let _ = crate::security::keychain::KeychainStore::delete("com.jarvis.aws", "secret_access_key");
        assert_eq!(list.source, Source::Aws);
        assert!(list.models.iter().all(|m| m.id != "eu.fixture.saved-earlier-v1:0"), "the saved id is not listed");
        assert_eq!(saved_chain(&db), before, "and is still the row's model");
        assert_eq!(before[0].2, "eu.fixture.saved-earlier-v1:0");
        // Saving it again goes through the same check as before, unchanged.
        assert_eq!(
            crate::ai::bedrock_model_to_save(Some("eu.fixture.saved-earlier-v1:0"), crate::ai::bedrock::UiLanguage::En),
            Ok(Some("eu.fixture.saved-earlier-v1:0".to_string()))
        );
    }

    /// The built-in list holds only ids the app ships (the default and the
    /// two V13 seeds), named as models.rs names them, pointed at the saved
    /// region's geography as `apply_inference_profile` would send them.
    /// Breaks if an id is added that the app does not ship, if a name is
    /// typed in by hand, or if the geography rewrite is dropped.
    #[test]
    fn the_built_in_list_holds_shipped_ids_for_the_saved_geography() {
        const V13: &str = include_str!("../../migrations/V13__aws_credentials_and_routing.sql");
        for id in BUILT_IN {
            assert!(id == models::BEDROCK_DEFAULT || V13.contains(&format!("'{id}'")), "{id} is not shipped");
            assert!(models::label_for(id).is_some(), "models.rs names {id}");
        }
        let names = |list: Vec<ModelChoice>| list.into_iter().map(|m| (m.id, m.name)).collect::<Vec<_>>();
        assert_eq!(
            names(built_in(None)),
            vec![
                ("global.anthropic.claude-sonnet-4-6".into(), "Claude Sonnet 4.6".into()),
                ("global.anthropic.claude-haiku-4-5-20251001-v1:0".into(), "Claude Haiku 4.5".into()),
                ("apac.amazon.nova-micro-v1:0".into(), "Amazon Nova Micro".into()),
            ]
        );
        let ids = |region: &str| built_in(Some(region)).into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids("us-east-1")[2], "us.amazon.nova-micro-v1:0");
        assert_eq!(ids("eu-west-1")[2], "eu.amazon.nova-micro-v1:0");
        assert_eq!(ids("ap-northeast-1")[2], "apac.amazon.nova-micro-v1:0");
        assert_eq!(ids("us-east-1")[..2], ids("ap-southeast-1")[..2], "global ids are not rewritten");
    }

    /// The empty choice names the model it runs.
    #[test]
    fn the_default_is_named() {
        if std::env::var("AWS_BEDROCK_MODEL_ID").is_err() {
            assert_eq!(default_model(), DefaultModel { id: models::BEDROCK_DEFAULT.into(), name: Some("Claude Sonnet 4.6") });
        }
    }

    /// What the frontend reads: snake_case codes, never SDK text.
    #[test]
    fn the_list_reaches_the_frontend_as_codes() {
        let value = serde_json::to_value(fallback(Some("us-east-1"), Reason::NotAllowed, vec![LIST_MODELS_ACTION])).unwrap();
        assert_eq!(value["source"], "fallback");
        assert_eq!(value["reason"], "not_allowed");
        assert_eq!(value["missing_permissions"], json!(["bedrock:ListFoundationModels"]));
        assert_eq!(value["models"][0]["kind"], "profile");
        assert_eq!(value["region"], "us-east-1");
    }
}
