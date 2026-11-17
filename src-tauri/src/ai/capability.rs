//! Which model a request uses, and whether it can do what the request needs
//! (P06). Every AI request asks [`resolve`] before anything is sent: chat and
//! its tools, mail analysis, mail sorting, the briefing, speech to text and
//! speech. The answer comes from the registry in use (capability flags,
//! feature defaults, the kill switch), never from a model id written here.
//!
//! JARVIS never moves a request to another model on its own (DECISIONS.md
//! section 5, "Failure handling"). `Unsupported` carries the models the user
//! could pick instead; the caller shows them and sends only after a click.

use super::cost::Feature;
use super::registry::{self, Model, ModelState, Registry};
use super::{compat, ProviderChainEntry, Workload};
use serde::{Deserialize, Serialize};

/// The registry's `features` key a request is resolved under. A chat turn
/// the user marked "Think harder" uses `premium`, whose defaults are the
/// models picked for that (Gemini's is Pro).
pub fn feature_key(feature: Feature, workload: Workload) -> &'static str {
    match feature {
        Feature::Chat | Feature::Tools | Feature::Other if workload == Workload::Premium => "premium",
        Feature::Chat | Feature::Tools | Feature::Other => "chat",
        Feature::MailAnalysis => "mail_analysis",
        Feature::MailSort => "mail_sort",
        Feature::Briefing => "briefing",
        Feature::Memory => "memory",
        Feature::Stt => "stt",
        Feature::Tts => "tts",
    }
}

/// The features that run on a provider only when the registry names a
/// model for it there, and the providers this build has code for. A chat
/// model is not a speech model, so these never fall back to chat's default
/// or to the model saved in the chain.
fn own_model_only(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "stt" => Some(&["gemini", "openai", "whisper"]),
        "tts" => Some(&["minimax"]),
        _ => None,
    }
}

/// The capability flag a feature needs (registry `models[].capabilities`).
/// Every chat request offers the tool list, so chat and the briefing need
/// tools; mail analysis and sorting ask for one JSON object. A local model
/// is best effort (DECISIONS.md section 5, "Parity bar"): it runs with tools
/// written into the prompt, so it is not refused here.
fn needs(key: &str, provider: &str) -> Option<&'static str> {
    if provider == "ollama" {
        return None;
    }
    match key {
        "chat" | "premium" | "briefing" => Some("tools"),
        "mail_analysis" | "mail_sort" => Some("json"),
        _ => None,
    }
}

fn has(model: &Model, capability: &str) -> bool {
    let c = &model.capabilities;
    match capability {
        "tools" => c.tools,
        "json" => c.json,
        "json_schema" => c.json_schema,
        "streaming" => c.streaming,
        "vision" => c.vision,
        "thinking" => c.thinking,
        "reasoning_effort" => c.reasoning_effort,
        _ => false,
    }
}

/// What the user chose, as a chain row holds it. The frontend sends one
/// back when the user picks a model in the error card; it is accepted only
/// when it is one of the alternatives offered ([`is_offered`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Pick {
    pub provider_type: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub endpoint_id: Option<String>,
    /// None: the registry's default for the feature.
    #[serde(default)]
    pub model_id: Option<String>,
}

impl Pick {
    pub fn of(entry: &ProviderChainEntry) -> Pick {
        Pick {
            provider_type: entry.provider_type.clone(),
            base_url: entry.base_url.clone(),
            endpoint_id: entry.endpoint_id.clone(),
            model_id: entry.model_id.clone().filter(|m| !m.trim().is_empty()),
        }
    }

    /// The one-row chain a picked request runs on.
    pub fn entry(&self) -> ProviderChainEntry {
        ProviderChainEntry {
            position: 0,
            provider_type: self.provider_type.clone(),
            endpoint_id: self.endpoint_id.clone(),
            model_id: self.model_id.clone(),
            enabled: true,
            base_url: self.base_url.clone(),
        }
    }
}

/// A model a request would run on: the row, the registry provider and the
/// model id that is sent.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Target {
    pub pick: Pick,
    /// The registry provider id ("gemini", "kimi", "openai_compat", ...).
    pub provider: String,
    /// The id sent; None when the row names none and the provider has no
    /// default for the feature.
    pub model: Option<String>,
    /// "Gemini 3.7 Flash (Google Gemini)", for the card.
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// The registry turned this saved model off (P05).
    Disabled { replacement: Option<String> },
    /// The model lacks a capability the feature needs.
    Missing { capability: String },
    /// The provider has no model for this feature (speech on Claude, a
    /// compatible service with no model chosen).
    NoModel,
    /// A row type this build does not know.
    UnknownProvider,
}

/// A model the user can pick in the card.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Alternative {
    pub pick: Pick,
    pub label: String,
    /// Set for the replacement of a disabled model: choosing it saves it at
    /// that chain position first (`disabled::confirm`), as P05's notice did.
    pub saves_at: Option<i32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Unsupported {
    pub target: Target,
    pub reason: Reason,
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    Ready(Target),
    Unsupported(Unsupported),
}

impl Resolved {
    pub fn ready(self) -> Option<Target> {
        match self {
            Resolved::Ready(target) => Some(target),
            Resolved::Unsupported(_) => None,
        }
    }
}

/// Whether `entry` can run the feature `key`, and on which model. `configured`
/// are the rows the user has credentials for (`AiRouter::configured`); the
/// alternatives are drawn from them.
pub fn resolve(key: &str, entry: &ProviderChainEntry, configured: &[ProviderChainEntry]) -> Resolved {
    let registry = registry::current();
    match check(&registry, key, entry) {
        Ok(target) => Resolved::Ready(target),
        Err((target, reason)) => {
            let mut alternatives = Vec::new();
            if let Reason::Disabled { replacement: Some(replacement) } = &reason {
                let pick = Pick { model_id: Some(replacement.clone()), ..Pick::of(entry) };
                if let Ok(ready) = check(&registry, key, &pick.entry()) {
                    alternatives.push(Alternative { pick, label: ready.label, saves_at: Some(entry.position) });
                }
            }
            for alternative in offered(&registry, key, configured) {
                if alternative.pick != Pick::of(entry) && !alternatives.iter().any(|a: &Alternative| a.pick == alternative.pick) {
                    alternatives.push(alternative);
                }
            }
            Resolved::Unsupported(Unsupported { target, reason, alternatives })
        }
    }
}

/// The models offered in the card for `key`: every configured row that is
/// ready for it, once each, in the order given.
pub fn offered_for(key: &str, configured: &[ProviderChainEntry]) -> Vec<Alternative> {
    offered(&registry::current(), key, configured)
}

fn offered(registry: &Registry, key: &str, configured: &[ProviderChainEntry]) -> Vec<Alternative> {
    let mut found: Vec<Alternative> = Vec::new();
    for row in configured {
        if let Ok(ready) = check(registry, key, row) {
            let pick = Pick::of(row);
            if !found.iter().any(|a| a.pick == pick) {
                found.push(Alternative { pick, label: ready.label, saves_at: None });
            }
        }
    }
    found
}

/// Whether a pick sent back by the frontend is one the card offered: a model
/// the user has credentials for that can run the feature. Anything else is
/// refused, so a pick can never name an address or a model of its own.
pub fn is_offered(key: &str, pick: &Pick, configured: &[ProviderChainEntry]) -> bool {
    offered_for(key, configured).iter().any(|a| a.pick == *pick)
}

fn check(registry: &Registry, key: &str, entry: &ProviderChainEntry) -> Result<Target, (Target, Reason)> {
    let pick = Pick::of(entry);
    // Speech providers are not chat rows: Whisper on this Mac and MiniMax
    // are asked for by name.
    let speech = match entry.provider_type.as_str() {
        "whisper" => Some("whisper"),
        "minimax" => Some("minimax"),
        _ => None,
    };
    let Some(provider) = speech.or_else(|| super::mail_policy::registry_provider(entry)) else {
        let target = Target { label: entry.provider_type.clone(), pick, provider: entry.provider_type.clone(), model: None };
        return Err((target, Reason::UnknownProvider));
    };
    let model = match own_model_only(key) {
        Some(wires) => wires.contains(&provider).then(|| registry.feature_default(key, provider).map(str::to_string)).flatten(),
        None => pick.model_id.clone().or_else(|| {
            let id = registry.default_model(key, provider);
            (!id.is_empty()).then_some(id)
        }),
    };
    let target = Target { label: label(registry, provider, entry, model.as_deref()), pick, provider: provider.to_string(), model: model.clone() };
    let Some(model) = model else { return Err((target, Reason::NoModel)) };
    if let ModelState::Disabled { replacement } = registry.model_state(provider, &model) {
        return Err((target, Reason::Disabled { replacement }));
    }
    if let (Some(capability), Some(entry)) = (needs(key, provider), describing(registry, provider, &model)) {
        if !has(entry, capability) {
            return Err((target, Reason::Missing { capability: capability.to_string() }));
        }
    }
    Ok(target)
}

/// The registry entry that describes `model` as used with `provider`: its
/// own entry, or for Bedrock (other vendors' models) the vendor's entry. A
/// model the registry does not list (a pick from a compatible service's
/// list, a local model outside the catalog) has none: its capabilities are
/// not known, and the request is sent.
fn describing<'a>(registry: &'a Registry, provider: &str, model: &str) -> Option<&'a Model> {
    registry.own_entry(provider, model).or_else(|| if provider == "bedrock" { registry.resolve(provider, model) } else { None })
}

/// "GPT-6 Luna (OpenAI (GPT))": the model's registry name, or its id, and
/// the service's.
fn label(registry: &Registry, provider: &str, entry: &ProviderChainEntry, model: Option<&str>) -> String {
    let service = match (entry.provider_type.as_str(), entry.base_url.as_deref()) {
        ("openai", Some(base)) if !compat::is_openai(base) => compat::label(base),
        _ => registry.provider(provider).map(|p| p.display_name.en.clone()).unwrap_or_else(|| provider.to_string()),
    };
    match model {
        Some(id) => {
            let name = describing(registry, provider, id).filter(|m| m.kind == registry::doc::Kind::Model).map(|m| m.display_name.en.clone()).unwrap_or_else(|| id.to_string());
            format!("{name} ({service})")
        }
        None => service,
    }
}

/// Whether "Think harder" can do anything on `entry` for a chat turn: the
/// registry says the premium model thinks or takes a reasoning effort, and
/// this build can send that on the provider's chat wire with tools.
///
/// OpenAI's Chat Completions refuses function tools when reasoning is on
/// for gpt-6-luna (ai/openai.rs `reasoning_effort`, verified live
/// 2026-09-25), and every chat turn sends tools, so api.openai.com is not
/// offered it until chat moves to the Responses API. Bedrock is left as it
/// was before P06 (the premium workload is recorded, nothing else changes),
/// so it is not offered either.
pub fn think_harder(entry: &ProviderChainEntry) -> bool {
    let registry = registry::current();
    let Ok(target) = check(&registry, "premium", entry) else { return false };
    let Some(model) = target.model.as_deref() else { return false };
    let Some(described) = describing(&registry, &target.provider, model) else { return false };
    let thinks = described.capabilities.thinking || described.capabilities.reasoning_effort;
    thinks && matches!(target.provider.as_str(), "gemini" | "claude")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::registry::test_override;

    fn row(provider_type: &str, model: Option<&str>, base_url: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: None, model_id: model.map(str::to_string), enabled: true, base_url: base_url.map(str::to_string) }
    }

    fn local(model: &str) -> ProviderChainEntry {
        ProviderChainEntry { endpoint_id: Some("mac".into()), ..row("local", Some(model), None) }
    }

    const KIMI: &str = "https://api.moonshot.ai/v1";

    fn ready(key: &str, entry: &ProviderChainEntry) -> Target {
        match resolve(key, entry, &[]) {
            Resolved::Ready(target) => target,
            other => panic!("{key} on {entry:?}: {other:?}"),
        }
    }

    fn reason(key: &str, entry: &ProviderChainEntry) -> Reason {
        match resolve(key, entry, &[]) {
            Resolved::Unsupported(u) => u.reason,
            other => panic!("{key} on {entry:?} should be unsupported: {other:?}"),
        }
    }

    /// The feature key a request is resolved under.
    #[test]
    fn each_request_resolves_under_its_registry_feature() {
        assert_eq!(feature_key(Feature::Chat, Workload::Tools), "chat");
        assert_eq!(feature_key(Feature::Chat, Workload::Premium), "premium");
        assert_eq!(feature_key(Feature::Other, Workload::Volume), "chat");
        assert_eq!(feature_key(Feature::MailAnalysis, Workload::Volume), "mail_analysis");
        assert_eq!(feature_key(Feature::MailSort, Workload::Volume), "mail_sort");
        assert_eq!(feature_key(Feature::Briefing, Workload::Tools), "briefing");
        assert_eq!(feature_key(Feature::Memory, Workload::Volume), "memory");
        assert_eq!(feature_key(Feature::Stt, Workload::Volume), "stt");
        assert_eq!(feature_key(Feature::Tts, Workload::Volume), "tts");
    }

    /// Every cloud provider is ready for every text feature with the
    /// registry's defaults (the parity bar), and the model sent is the
    /// feature's own default.
    #[test]
    fn every_cloud_provider_runs_every_text_feature_on_its_default() {
        let registry = registry::current();
        for key in ["chat", "premium", "mail_analysis", "mail_sort", "briefing", "memory"] {
            for provider in ["openai", "claude", "gemini", "bedrock"] {
                let target = ready(key, &row(provider, None, None));
                assert_eq!(target.model.as_deref(), Some(registry.default_model(key, provider).as_str()), "{key} {provider}");
            }
        }
        // Gemini's premium default is not its chat default.
        assert_ne!(ready("premium", &row("gemini", None, None)).model, ready("chat", &row("gemini", None, None)).model);
        // A saved model is the one sent.
        assert_eq!(ready("chat", &row("openai", Some("gpt-5.6-luna"), None)).model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(ready("chat", &row("openai", Some("gpt-5.6-luna"), None)).label, "GPT-5.6 Luna (OpenAI (GPT))");
    }

    /// A compatible service and a local model run on the model the user
    /// picked; without one there is nothing to send.
    #[test]
    fn a_service_without_defaults_needs_a_chosen_model() {
        assert_eq!(ready("chat", &row("openai", Some("kimi-k2"), Some(KIMI))).label, "kimi-k2 (Kimi)");
        assert_eq!(ready("mail_analysis", &row("openai", Some("kimi-k2"), Some(KIMI))).provider, "kimi");
        assert_eq!(reason("chat", &row("openai", None, Some(KIMI))), Reason::NoModel);
        assert_eq!(ready("chat", &local("qwen3:8b")).model.as_deref(), Some("qwen3:8b"));
        assert_eq!(reason("chat", &row("local", None, None)), Reason::NoModel);
        assert_eq!(reason("chat", &row("mystery", Some("m"), None)), Reason::UnknownProvider);
    }

    /// Speech runs only where the registry names a speech model, never on a
    /// chat model, whatever the chain row saved.
    #[test]
    fn speech_to_text_and_speech_run_only_on_the_registrys_own_models() {
        let registry = registry::current();
        assert_eq!(ready("stt", &row("gemini", Some("gemini-3.1-pro-preview"), None)).model.as_deref(), registry.feature_default("stt", "gemini"));
        assert_eq!(ready("stt", &row("whisper", None, None)).model.as_deref(), registry.feature_default("stt", "whisper"));
        for provider in ["openai", "claude", "bedrock"] {
            assert_eq!(reason("stt", &row(provider, None, None)), Reason::NoModel, "{provider}: registry version 1 names no speech model");
        }
        assert_eq!(reason("stt", &row("openai", Some("kimi-k2"), Some(KIMI))), Reason::NoModel);
        assert_eq!(ready("tts", &row("minimax", None, None)).model.as_deref(), registry.feature_default("tts", "minimax"));
        assert_eq!(reason("tts", &row("gemini", None, None)), Reason::NoModel);

        // A registry copy that adds an OpenAI speech model turns it on.
        let _registry = test_override::with(|r| {
            let mut transcribe = r.models.iter().find(|m| m.id == "gpt-4o-mini").unwrap().clone();
            transcribe.id = "transcribe-test".into();
            r.models.push(transcribe);
            r.features.get_mut("stt").unwrap().insert("openai".into(), "transcribe-test".into());
            // Not for a provider this build has no speech code for.
            r.features.get_mut("stt").unwrap().insert("claude".into(), "claude-haiku-4-5".into());
        });
        assert_eq!(ready("stt", &row("openai", None, None)).model.as_deref(), Some("transcribe-test"));
        assert_eq!(reason("stt", &row("claude", None, None)), Reason::NoModel);
    }

    /// The capability matrix: a model the registry says cannot call tools is
    /// refused for chat and the briefing, one without JSON for mail; a local
    /// model is best effort; a model the registry does not list is sent.
    #[test]
    fn the_capability_each_feature_needs_comes_from_the_registry() {
        let _registry = test_override::with(|r| {
            let entry = r.models.iter_mut().find(|m| m.id == "gpt-4o").unwrap();
            entry.capabilities.tools = false;
            let entry = r.models.iter_mut().find(|m| m.id == "claude-haiku-4-5").unwrap();
            entry.capabilities.json = false;
            let entry = r.models.iter_mut().find(|m| m.id == "nova-pro").unwrap();
            entry.capabilities.tools = false;
        });
        let no_tools = row("openai", Some("gpt-4o"), None);
        for key in ["chat", "premium", "briefing"] {
            assert_eq!(reason(key, &no_tools), Reason::Missing { capability: "tools".into() }, "{key}");
        }
        ready("mail_analysis", &no_tools);
        ready("memory", &no_tools);
        let no_json = row("claude", Some("claude-haiku-4-5"), None);
        for key in ["mail_analysis", "mail_sort"] {
            assert_eq!(reason(key, &no_json), Reason::Missing { capability: "json".into() }, "{key}");
        }
        ready("chat", &no_json);
        // A Bedrock id is described by its own entry, or by the vendor's.
        assert_eq!(reason("chat", &row("bedrock", Some("apac.amazon.nova-pro-v1:0"), None)), Reason::Missing { capability: "tools".into() });
        let haiku_on_bedrock = row("bedrock", Some("us.anthropic.claude-haiku-4-5-20251001-v1:0"), None);
        assert_eq!(reason("mail_sort", &haiku_on_bedrock), Reason::Missing { capability: "json".into() });
        ready("chat", &haiku_on_bedrock);
        // The registry lists deepseek-r1 without tools; a local model still runs.
        assert!(!registry::current().local_model("deepseek-r1:7b").unwrap().capabilities.tools);
        ready("chat", &local("deepseek-r1:7b"));
        // Unlisted: capabilities unknown, sent.
        ready("chat", &row("openai", Some("gpt-7-unlisted"), None));
        ready("mail_sort", &row("openai", Some("vendor-model"), Some(KIMI)));
    }

    /// A disabled saved model is unsupported, and its replacement is offered
    /// first and saved only when chosen. The registry default is never
    /// disabled (Registry::check), so an empty choice is ready.
    #[test]
    fn a_disabled_model_is_unsupported_and_offers_its_replacement_first() {
        let _registry = test_override::with(|r| {
            let entry = r.models.iter_mut().find(|m| m.id == "gpt-5.6-luna").unwrap();
            entry.status = registry::Status::Disabled;
            entry.replacement = Some("gpt-6-luna".into());
        });
        let saved = ProviderChainEntry { position: 2, ..row("openai", Some("gpt-5.6-luna"), None) };
        let configured = [saved.clone(), row("claude", None, None)];
        let Resolved::Unsupported(u) = resolve("chat", &saved, &configured) else { panic!("disabled") };
        assert_eq!(u.reason, Reason::Disabled { replacement: Some("gpt-6-luna".into()) });
        assert_eq!(u.alternatives[0], Alternative { pick: Pick { model_id: Some("gpt-6-luna".into()), ..Pick::of(&saved) }, label: "GPT-6 Luna (OpenAI (GPT))".into(), saves_at: Some(2) });
        assert_eq!(u.alternatives[1].pick, Pick::of(&row("claude", None, None)));
        assert_eq!(u.alternatives[1].saves_at, None);
        assert_eq!(u.alternatives.len(), 2, "the disabled model itself is not offered");
        ready("chat", &row("openai", None, None));
    }

    /// Alternatives are the configured rows that can run the feature, once
    /// each, never the model that failed. A pick is accepted only when it is
    /// one of them.
    #[test]
    fn alternatives_are_configured_models_that_support_the_feature() {
        let configured = [row("gemini", None, None), row("openai", None, None), row("openai", None, None), row("openai", None, Some(KIMI)), row("openai", Some("kimi-k2"), Some(KIMI))];
        let labels: Vec<String> = offered_for("chat", &configured).into_iter().map(|a| a.label).collect();
        assert_eq!(labels, ["Gemini 3.7 Flash (Google Gemini)", "GPT-6 Luna (OpenAI (GPT))", "kimi-k2 (Kimi)"]);
        let stt: Vec<String> = offered_for("stt", &configured).into_iter().map(|a| a.pick.provider_type).collect();
        assert_eq!(stt, ["gemini"], "only Gemini has a speech model in registry version 1");

        assert!(is_offered("chat", &Pick::of(&row("openai", None, None)), &configured));
        assert!(!is_offered("chat", &Pick::of(&row("claude", None, None)), &configured), "no Claude key");
        assert!(!is_offered("chat", &Pick::of(&row("openai", Some("kimi-k2"), Some("https://attacker.example/v1"))), &configured), "an address the user never saved");
        assert!(!is_offered("chat", &Pick::of(&row("openai", None, Some(KIMI))), &configured), "no model chosen for Kimi");
    }

    /// "Think harder" is offered where the registry says the premium model
    /// thinks and this build sends it with tools: Gemini (thinking config),
    /// Claude once the registry marks its models as thinking. Not OpenAI's
    /// Chat Completions, not Bedrock, not a model the registry does not list.
    #[test]
    fn think_harder_follows_the_registry_and_the_chat_wire() {
        assert!(think_harder(&row("gemini", None, None)));
        assert!(!think_harder(&row("claude", None, None)), "registry version 1 marks Claude as not thinking");
        assert!(!think_harder(&row("openai", None, None)), "Chat Completions refuses tools with reasoning on");
        assert!(!think_harder(&row("bedrock", None, None)));
        assert!(!think_harder(&row("openai", Some("kimi-k2"), Some(KIMI))));
        assert!(!think_harder(&local("qwen3.5:4b")));
        let _registry = test_override::with(|r| {
            for m in r.models.iter_mut().filter(|m| m.provider == "claude") {
                m.capabilities.thinking = true;
            }
            for m in r.models.iter_mut().filter(|m| m.provider == "gemini") {
                m.capabilities.thinking = false;
            }
        });
        assert!(think_harder(&row("claude", None, None)));
        assert!(!think_harder(&row("gemini", None, None)));
    }
}
