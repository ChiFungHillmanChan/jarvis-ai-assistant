//! Every AI model JARVIS knows, read from the model registry (ai/registry,
//! registry/registry.json): the default ("latest") model per provider and
//! feature, the cloud price list, the local Ollama catalog, and the one
//! function that turns token usage into cost. This module keeps the
//! functions callers used before the registry existed, as a thin layer over
//! `registry::current()`.
//!
//! To ship a new model or retire an old one, edit registry/registry.json and
//! publish it (registry/README.md). No model id is written anywhere else in
//! the app; the frontend reads the list through `ai_models`.
//!
//! Local models run on the user's Mac and have no cost: `usd_cost_micro`
//! returns None for them and callers report the tokens instead. A cloud model
//! the registry does not price has no price either: its usage is stored with
//! the cost unknown (`price_known`), never with a guessed rate.

use super::registry;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Defaults: what "Latest (recommended)" means for each provider.
// ---------------------------------------------------------------------------

/// OpenAI chat and mail.
pub fn openai_default() -> String {
    registry::current().default_model("chat", "openai")
}

/// Anthropic Claude.
pub fn claude_default() -> String {
    registry::current().default_model("chat", "claude")
}

/// Gemini for routine work.
pub fn gemini_flash() -> String {
    registry::current().default_model("chat", "gemini")
}

/// Gemini for an explicit premium request.
pub fn gemini_pro() -> String {
    registry::current().default_model("premium", "gemini")
}

/// AWS Bedrock, as a global cross-region inference profile.
pub fn bedrock_default() -> String {
    registry::current().default_model("chat", "bedrock")
}

/// TypeSafe Jev, the optional mail classifier.
pub fn jev() -> String {
    registry::current().default_model("mail_sort", "typesafe")
}

/// The model `provider` uses for `feature` when the user chose none.
pub fn default_for(feature: &str, provider: &str) -> String {
    registry::current().default_model(feature, provider)
}

/// The seeded defaults, for the tests written before the registry. They
/// equal the frozen test registry (`registry::fixture`), which the app's
/// tests run on; `fixtures_match_the_test_registry` checks it.
#[cfg(test)]
pub const OPENAI_DEFAULT: &str = "gpt-6-luna";
#[cfg(test)]
pub const CLAUDE_DEFAULT: &str = "claude-sonnet-5-5";
#[cfg(test)]
pub const GEMINI_FLASH: &str = "gemini-3.7-flash";
#[cfg(test)]
pub const GEMINI_PRO: &str = "gemini-3.1-pro-preview";
#[cfg(test)]
pub const BEDROCK_DEFAULT: &str = "global.anthropic.claude-sonnet-4-6";
#[cfg(test)]
pub const JEV: &str = "jev-latest";

// ---------------------------------------------------------------------------
// Request limits.
// ---------------------------------------------------------------------------

/// A provider's request limits (registry `providers[].limits`). The bundled
/// copy carries every limit this build reads (`registry::doc::REQUIRED_LIMITS`),
/// and a fetched copy that leaves one out keeps the bundled value, so the
/// zero a missing value would read as is never sent.
pub fn limits(provider: &str) -> registry::doc::Limits {
    registry::current().limits(provider)
}

/// The output cap of one chat request.
pub fn chat_max_output_tokens(provider: &str) -> u32 {
    limits(provider).chat_max_output_tokens.unwrap_or_default()
}

/// The output cap the two accessors below use only when neither the copy in
/// use nor the bundled copy carries their field (both were added in registry
/// version 3). The bundled copy carries both (`REQUIRED_LIMITS`,
/// `REQUIRED_FEATURE_LIMITS`) and a cached or fetched copy that leaves one
/// out takes the bundled value (`load::apply_floor`), so a release never
/// uses it; the app's tests do, on the frozen version 1 fixture. It is the
/// value the code held before version 3: never 0, never unlimited.
pub(crate) const OUTPUT_TOKENS_WHEN_UNSET: u32 = 4_096;

/// The output cap of one local chat request (registry
/// `providers.ollama.limits.chat_max_output_tokens`), never more than half
/// the model's context window.
pub fn local_chat_max_output_tokens(context_length: u32) -> u32 {
    limits("ollama").chat_max_output_tokens.unwrap_or(OUTPUT_TOKENS_WHEN_UNSET).min(context_length / 2)
}

/// The output cap of one request a feature sends itself, whichever AI
/// answers it (registry `feature_limits.<feature>.max_output_tokens`): mail
/// analysis and mail sorting.
pub fn feature_max_output_tokens(feature: super::cost::Feature) -> u32 {
    registry::current().feature_limits(feature.as_str()).max_output_tokens.unwrap_or(OUTPUT_TOKENS_WHEN_UNSET)
}

/// Tool rounds in one chat turn before the final answer is forced.
pub fn max_tool_rounds(provider: &str) -> usize {
    limits(provider).max_tool_rounds.unwrap_or_default() as usize
}

/// The reasoning effort of a chat request to `model` on api.openai.com:
/// none for a model the registry says takes no effort
/// (`capabilities.reasoning_effort: false`), else the model's own
/// `chat_reasoning_effort`, else OpenAI's. A model the registry does not
/// list gets the provider's value.
pub fn openai_chat_effort(model: &str) -> Option<String> {
    registry::current().chat_reasoning_effort("openai", model)
}

/// The `anthropic-version` header.
pub fn anthropic_version() -> String {
    limits("claude").api_version.unwrap_or_default()
}

/// The registry provider whose limits an OpenAI-format row uses: OpenAI's
/// own API, or the compatible service at `base_url`.
pub fn openai_format_provider(base_url: Option<&str>) -> &'static str {
    match base_url {
        None => "openai",
        Some(base) if super::compat::is_openai(base) => "openai",
        Some(base) => super::compat::provider_for(base).unwrap_or("openai_compat"),
    }
}

// ---------------------------------------------------------------------------
// Cloud prices.
// ---------------------------------------------------------------------------

/// USD per 1M tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Price {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_write_per_million: f64,
}

/// The whole request is billed at `price` once its input exceeds `above`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LongContext {
    pub above: i64,
    pub price: Price,
}

/// One priced entry, as the frontend reads it (`src/lib/ai-models.ts`).
#[derive(Debug, Clone, Serialize)]
pub struct CloudModel {
    pub provider: String,
    /// Matched against the id a provider reports: the longest entry
    /// contained in that id wins.
    pub id: String,
    pub label: String,
    /// A family pattern ("gemini-flash") rather than a model a user can pick.
    pub family: bool,
    /// Only an active model is offered as a choice; the rest stay listed so
    /// their usage is still priced.
    pub status: registry::doc::Status,
    pub price: Price,
    pub long_context: Option<LongContext>,
    /// The most output tokens (thinking included) one request may ask for.
    pub max_output_tokens: Option<u32>,
}

/// The base price of a cloud model, or None when the registry has no entry
/// for it. There is no fallback rate: a guess either hides spend ($0) or
/// invents it.
pub fn price_for(model_id: &str) -> Option<Price> {
    let found = registry::current().price_for(model_id);
    if found.is_none() {
        log::warn!("cost: no price for model '{model_id}'; its usage is stored with the cost unknown. Add it to registry/registry.json.");
    }
    found
}

/// The registry's name for a model id, matched as `price_for` matches it
/// ("global.anthropic.claude-sonnet-4-6" is "Claude Sonnet 4.6"). None for
/// an id it does not list and for a family pattern.
pub fn label_for(model_id: &str) -> Option<String> {
    registry::current().label_for(model_id).map(str::to_string)
}

/// Whether usage of this model can be turned into money: always for a local
/// model (free), and for a cloud model only when the registry prices it.
pub fn price_known(provider: &str, model_id: &str) -> bool {
    registry::current().price_known(provider, model_id)
}

/// The output cap one request to this model may ask for, when recorded.
pub fn max_output_tokens(model_id: &str) -> Option<u32> {
    registry::current().max_output_tokens(model_id)
}

// ---------------------------------------------------------------------------
// Cost: the only place usage becomes money.
// ---------------------------------------------------------------------------

/// Providers that run on the user's own Mac. "ollama" and "本機 ai" are the
/// names a plain completion reports.
pub fn is_local(provider: &str) -> bool {
    matches!(provider.to_lowercase().as_str(), "local" | "ollama" | "本機 ai")
}

/// What one request cost in micro-USD at the registry in use. None when
/// there is no money figure: a local model, which is free (report its tokens
/// instead), or a cloud model without a price (`price_known` tells the two
/// apart).
pub fn usd_cost_micro(provider: &str, model_id: &str, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) -> Option<i64> {
    registry::current().usd_cost_micro(provider, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens)
}

/// One request at one price, in micro-USD. `input_tokens` includes the cache
/// reads and writes, which are billed at their own rates.
pub(crate) fn cost_micro_at(p: Price, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) -> i64 {
    let billable_input = (input_tokens - cache_read_tokens - cache_write_tokens).max(0);
    let usd = (billable_input as f64) * p.input_per_million / 1_000_000.0
        + (output_tokens as f64) * p.output_per_million / 1_000_000.0
        + (cache_read_tokens as f64) * p.cache_read_per_million / 1_000_000.0
        + (cache_write_tokens as f64) * p.cache_write_per_million / 1_000_000.0;
    (usd * 1_000_000.0).round() as i64
}

// ---------------------------------------------------------------------------
// Local models (Ollama). No cost.
// ---------------------------------------------------------------------------

/// Settings for the local models JARVIS can install in one click and trusts
/// with mail.
#[derive(Debug, Clone, Serialize)]
pub struct LocalSetup {
    /// Free disk needed to download and run it.
    pub disk_reserve_gb: f64,
    /// Largest context JARVIS asks for, whatever the model reports.
    pub context_cap: usize,
    /// Turn thinking off: these small models otherwise spend the output
    /// budget thinking and return no answer.
    pub think_off: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModel {
    pub id: String,
    pub label: String,
    pub param_size: String,
    pub download_gb: f64,
    pub min_ram_gb: u32,
    pub tool_capable: bool,
    pub description: String,
    /// Some for the models offered in setup and approved for mail.
    pub setup: Option<LocalSetup>,
}

fn local_from(model: &registry::Model) -> Option<LocalModel> {
    let local = model.local.as_ref()?;
    Some(LocalModel {
        id: model.id.clone(),
        label: model.display_name.en.clone(),
        param_size: local.param_size.clone(),
        download_gb: local.download_gb,
        min_ram_gb: local.min_ram_gb,
        tool_capable: model.capabilities.tools,
        description: local.description.clone(),
        setup: local.setup.as_ref().map(|s| LocalSetup { disk_reserve_gb: s.disk_reserve_gb, context_cap: s.context_cap, think_off: s.think_off }),
    })
}

/// The local catalog, in registry order.
pub fn local_models() -> Vec<LocalModel> {
    registry::current().local_models().filter_map(local_from).collect()
}

pub fn local_model(id: &str) -> Option<LocalModel> {
    registry::current().local_model(id).and_then(local_from)
}

/// The one-click setup model for this Mac: the largest approved model its RAM runs.
pub fn suggested_local_model(ram_gb: f64) -> Option<String> {
    registry::current().suggested_local_model(ram_gb).map(str::to_string)
}

/// Model families with native tool calling, for local models outside the
/// catalog.
pub fn tool_capable_families() -> Vec<String> {
    registry::current().tool_capable_families()
}

// ---------------------------------------------------------------------------
// For the frontend.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Defaults {
    pub openai: String,
    pub claude: String,
    pub gemini_flash: String,
    pub gemini_pro: String,
    /// What Gemini sorts mail with when no model is picked (Settings > Your AI).
    pub gemini_mail_sort: String,
    pub bedrock: String,
    /// The on-device speech model; its file is `<id>.bin` (voice/model_manager.rs).
    pub whisper: String,
}

/// A provider chain preset (Settings > Models), in registry order.
#[derive(Serialize)]
pub struct PresetChoice {
    pub id: String,
    pub label: registry::doc::Text,
}

/// A service setup offers (ChooseAi), in the registry's `setup.order`.
#[derive(Serialize)]
pub struct SetupChoice {
    /// The registry provider id; `openai_compat` is "another service".
    pub id: String,
    pub name: registry::doc::Text,
    pub hint: registry::doc::Text,
    pub key_page: Option<String>,
    pub model: registry::doc::SetupModel,
}

/// An OpenAI-compatible service known by name: its regions and the hosts
/// that are it. Always as this build shipped them (`load::apply_floor`).
#[derive(Serialize)]
pub struct CompatibleService {
    pub id: String,
    pub name: String,
    pub regions: Vec<registry::doc::Region>,
    pub host_suffixes: Vec<String>,
}

#[derive(Serialize)]
pub struct Catalog {
    pub defaults: Defaults,
    pub cloud: Vec<CloudModel>,
    pub local: Vec<LocalModel>,
    /// The registry these lists come from (Settings > About).
    pub registry: registry::Info,
    /// Whether this build fetches newer registries (it has an endpoint).
    pub registry_updates: bool,
    pub app_version: String,
    pub presets: Vec<PresetChoice>,
    pub default_preset: String,
    pub setup: Vec<SetupChoice>,
    pub compatible: Vec<CompatibleService>,
}

fn cloud_models(registry: &registry::Registry) -> Vec<CloudModel> {
    registry
        .models
        .iter()
        .filter(|m| !m.managed)
        .filter_map(|m| {
            let p = m.prices.as_ref()?;
            let price = |input: f64, output: f64, cache_read: Option<f64>, cache_write: Option<f64>| Price {
                input_per_million: input,
                output_per_million: output,
                cache_read_per_million: cache_read.unwrap_or(0.0),
                cache_write_per_million: cache_write.unwrap_or(0.0),
            };
            Some(CloudModel {
                provider: m.provider.clone(),
                id: m.id.clone(),
                label: if m.kind == registry::doc::Kind::Family { m.id.clone() } else { m.display_name.en.clone() },
                family: m.kind == registry::doc::Kind::Family,
                status: m.status,
                price: price(p.input, p.output, p.cache_read, p.cache_write),
                long_context: p.tiers.first().map(|t| LongContext { above: t.above_input_tokens, price: price(t.input, t.output, t.cache_read, t.cache_write) }),
                max_output_tokens: m.max_output,
            })
        })
        .collect()
}

/// The full model list, so the frontend never hardcodes a model id.
#[tauri::command]
pub fn ai_models() -> Catalog {
    let registry = registry::current();
    Catalog {
        defaults: Defaults {
            openai: registry.default_model("chat", "openai"),
            claude: registry.default_model("chat", "claude"),
            gemini_flash: registry.default_model("chat", "gemini"),
            gemini_pro: registry.default_model("premium", "gemini"),
            gemini_mail_sort: registry.default_model("mail_sort", "gemini"),
            bedrock: registry.default_model("chat", "bedrock"),
            whisper: registry.default_model("stt", "whisper"),
        },
        cloud: cloud_models(&registry),
        local: registry.local_models().filter_map(local_from).collect(),
        registry: registry::Info { version: registry.version, issued_at: registry.issued_at.clone(), source: registry::holder().get().source },
        registry_updates: registry::fetch::endpoint().is_some(),
        app_version: registry::load::app_version().to_string(),
        presets: registry.presets.chains.iter().map(|(id, preset)| PresetChoice { id: id.clone(), label: preset.label.clone() }).collect(),
        default_preset: registry.presets.default.clone(),
        setup: {
            let mut offered: Vec<(u32, SetupChoice)> = registry
                .providers
                .iter()
                .filter_map(|p| {
                    let setup = p.setup.as_ref()?;
                    Some((setup.order, SetupChoice { id: p.id.clone(), name: p.display_name.clone(), hint: setup.hint.clone(), key_page: setup.key_page.clone(), model: setup.model.clone() }))
                })
                .collect();
            offered.sort_by_key(|(order, _)| *order);
            offered.into_iter().map(|(_, choice)| choice).collect()
        },
        compatible: registry
            .providers
            .iter()
            .filter(|p| p.wire == "openai_compat" && !p.host_suffixes.is_empty())
            .map(|p| CompatibleService { id: p.id.clone(), name: p.display_name.en.clone(), regions: p.regions.clone(), host_suffixes: p.host_suffixes.clone() })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(model: &str, input: i64, output: i64, cache_read: i64) -> i64 {
        usd_cost_micro("cloud", model, input, output, cache_read, 0).unwrap()
    }

    /// The constants the older tests use are the test registry's defaults.
    #[test]
    fn fixtures_match_the_test_registry() {
        assert_eq!(openai_default(), OPENAI_DEFAULT);
        assert_eq!(claude_default(), CLAUDE_DEFAULT);
        assert_eq!(gemini_flash(), GEMINI_FLASH);
        assert_eq!(gemini_pro(), GEMINI_PRO);
        assert_eq!(bedrock_default(), BEDROCK_DEFAULT);
        assert_eq!(jev(), JEV);
    }

    /// The accessors read the registry in use, not a copy made at start-up.
    #[test]
    fn a_registry_change_reaches_every_accessor() {
        let _registry = registry::test_override::with(|r| {
            r.features.get_mut("chat").unwrap().insert("openai".into(), "gpt-5.6-luna".into());
            let luna = r.models.iter_mut().find(|m| m.id == "gpt-5.6-luna").unwrap();
            luna.display_name.en = "Renamed".into();
            luna.prices.as_mut().unwrap().input = 9.0;
        });
        assert_eq!(openai_default(), "gpt-5.6-luna");
        assert_eq!(label_for("gpt-5.6-luna").as_deref(), Some("Renamed"));
        assert_eq!(price_for("gpt-5.6-luna").unwrap().input_per_million, 9.0);
        assert_eq!(cost("gpt-5.6-luna", 1_000_000, 0, 0), 9_000_000);
        let catalog = ai_models();
        assert_eq!(catalog.defaults.openai, "gpt-5.6-luna");
        assert!(catalog.cloud.iter().any(|m| m.id == "gpt-5.6-luna" && m.label == "Renamed"));
    }

    /// Settings > Your AI preselects Gemini's mail sort default, which the
    /// catalog reads from the registry's mail_sort feature, not chat's.
    #[test]
    fn the_catalog_carries_the_gemini_mail_sort_default() {
        let _registry = registry::test_override::with(|r| {
            r.features.get_mut("mail_sort").unwrap().insert("gemini".into(), "gemini-3.6-flash".into());
        });
        let catalog = ai_models();
        assert_eq!(catalog.defaults.gemini_mail_sort, "gemini-3.6-flash");
        assert_eq!(catalog.defaults.gemini_flash, registry::current().default_model("chat", "gemini"));
    }

    /// What the frontend lists (presets, setup, compatible services and the
    /// registry version) comes from the registry, in its order.
    #[test]
    fn the_frontend_catalog_lists_what_the_registry_holds() {
        let _registry = registry::test_override::with(|r| {
            r.version = 42;
            r.presets.chains.reverse();
            r.providers.iter_mut().find(|p| p.id == "claude").unwrap().setup.as_mut().unwrap().order = 0;
        });
        let catalog = ai_models();
        assert_eq!(catalog.registry.version, 42);
        let wire = serde_json::to_value(&catalog).unwrap();
        assert_eq!(wire["registry"]["source"], "bundled");
        let presets: Vec<&str> = catalog.presets.iter().map(|p| p.id.as_str()).collect();
        let mut expected: Vec<String> = registry::fixture().presets.chains.iter().map(|(id, _)| id.clone()).collect();
        expected.reverse();
        assert_eq!(presets, expected);
        assert_eq!(catalog.setup[0].id, "claude", "setup follows setup.order");
        assert!(catalog.setup.iter().any(|s| s.id == "openai_compat"));
        let services: Vec<&str> = catalog.compatible.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(services, ["kimi", "qwen", "gemini_compat"]);
        assert_eq!(wire["setup"][0]["name"]["zh-HK"], "Anthropic Claude");
        assert_eq!(wire["compatible"][0]["regions"][0]["key_page"], "https://platform.moonshot.ai/console/api-keys");
    }

    #[test]
    fn sonnet_cost_matches_known_pricing() {
        // 1000*3/1e6 + 500*15/1e6 = 0.0105 USD
        assert_eq!(cost("claude-sonnet-4-6", 1000, 500, 0), 10_500);
    }

    #[test]
    fn cache_read_subtracts_from_billable_input() {
        // billable 200*3/1e6 + 800*0.30/1e6 = 0.00084
        assert_eq!(cost("claude-sonnet-4-6", 1000, 0, 800), 840);
    }

    #[test]
    fn local_models_have_no_cost() {
        for provider in ["local", "Ollama", "本機 AI"] {
            assert_eq!(usd_cost_micro(provider, "qwen3.5:4b", 5_000, 500, 0, 0), None, "{provider}");
        }
    }

    /// A model without a price has no cost figure at all: not a silent $0,
    /// and not the $3/$15 guess that used to make it look expensive.
    #[test]
    fn unknown_model_has_no_price_instead_of_a_guess() {
        assert_eq!(usd_cost_micro("openai", "gpt-7-mystery", 1000, 1000, 0, 0), None);
        assert_eq!(price_for("gpt-7-mystery"), None);
        assert!(!price_known("openai", "gpt-7-mystery"));
        assert!(price_known("openai", "gpt-6-luna"));
        assert!(price_known("local", "anything:7b"), "a local model is known to be free");
    }

    #[test]
    fn gemini_models_carry_their_output_limit() {
        assert_eq!(max_output_tokens(GEMINI_FLASH), Some(65_536));
        assert_eq!(max_output_tokens(GEMINI_PRO), Some(65_536));
        assert_eq!(max_output_tokens("gemini-2.5-flash"), Some(65_536));
        assert_eq!(max_output_tokens("gemini-flash-latest"), None, "a family pattern does not guess a limit");
        assert_eq!(max_output_tokens("gemini-9-mystery"), None);
    }

    /// The longest matching entry wins, so a specific id is never priced as
    /// the family it contains, whatever the order of the registry.
    #[test]
    fn specific_ids_beat_the_families_they_contain() {
        assert_eq!(price_for("gpt-5.6-luna").unwrap().input_per_million, 0.20);
        assert_eq!(price_for("gpt-5").unwrap().input_per_million, 1.25);
        assert_eq!(price_for("gemini-3.5-flash-lite").unwrap().input_per_million, 0.30);
        assert_eq!(price_for("gemini-3.5-flash").unwrap().input_per_million, 1.50);
        assert_eq!(price_for("gemini-3.6-flash").unwrap().input_per_million, 1.50);
        assert_eq!(price_for("gemini-3.1-flash-lite").unwrap().input_per_million, 0.25);
        assert_eq!(price_for("global.anthropic.claude-sonnet-4-6").unwrap().input_per_million, 3.0);
        assert_eq!(price_for("amazon.nova-micro-v1:0").unwrap().input_per_million, 0.035);
    }

    #[test]
    fn gpt_6_luna_and_jev_have_their_own_prices() {
        let luna = price_for("gpt-6-luna").unwrap();
        assert_eq!((luna.input_per_million, luna.output_per_million), (0.10, 0.50));
        let jev = price_for("jev-1.13.0").unwrap();
        assert_eq!((jev.input_per_million, jev.output_per_million), (0.042, 0.0));
        assert_eq!(cost("gpt-6-luna", 200_000, 100_000, 0), 70_000);
        // Above 272K the whole request moves to 2x input.
        assert_eq!(cost("gpt-6-luna", 300_000, 0, 0), 60_000);
        assert_eq!(cost("gpt-6-luna", 250_000, 0, 0), 25_000);
    }

    #[test]
    fn gemini_pro_long_context_requests_bill_at_the_higher_tier() {
        let base = cost("gemini-3.1-pro-preview", 100_000, 1_000, 0);
        let long = cost("gemini-3.1-pro-preview", 250_000, 1_000, 0);
        assert!(long > base * 2, "long-context input must use the $4 tier, got {long} vs {base}");
        assert_eq!(cost("gemini-3.5-flash", 250_000, 0, 0), 375_000);
    }

    #[test]
    fn haiku_5_5_bills_above_100k_at_the_higher_tier() {
        // 50K in + 10K out at $0.10 / $0.50: $0.005 + $0.005.
        assert_eq!(cost("claude-haiku-5-5", 50_000, 10_000, 0), 10_000);
        // 200K in at $0.50: $0.10.
        assert_eq!(cost("claude-haiku-5-5", 200_000, 0, 0), 100_000);
        assert_eq!(price_for("claude-haiku-5-5").unwrap().input_per_million, 0.10, "not priced as haiku-4-5");
    }

    /// Every default must have its own price, or the traffic that matters
    /// most is stored with the cost unknown. Checked on the registry this
    /// build ships, not only on the test registry.
    #[test]
    fn every_default_model_has_a_real_price() {
        for registry in [registry::fixture(), registry::bundled()] {
            for (feature, provider) in [("chat", "openai"), ("chat", "claude"), ("chat", "gemini"), ("premium", "gemini"), ("mail_sort", "gemini"), ("chat", "bedrock"), ("mail_sort", "typesafe")] {
                let id = registry.default_model(feature, provider);
                assert!(registry.find_priced(&id).is_some(), "{id} has no price in registry version {}", registry.version);
            }
        }
        assert_eq!(price_for(GEMINI_FLASH).unwrap().input_per_million, 0.75, "gemini-3.7-flash rate; it doubles on 2027-01-01");
    }

    /// The output limits above are documented, not checked live (see the
    /// registry's gemini entries). Asking for more than a model allows is a
    /// 400, which stops the chain, so check them against what Google reports
    /// before relying on them. The default Flash and Pro ids must report a
    /// limit: a bad key or a retired id answers without one, and that proves
    /// nothing. Run deliberately:
    ///   GEMINI_API_KEY=... cargo test --manifest-path src-tauri/Cargo.toml \
    ///     gemini_output_limits_match_what_google_serves -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn gemini_output_limits_match_what_google_serves() {
        let key = std::env::var("GEMINI_API_KEY").expect("set GEMINI_API_KEY to run this test");
        let registry = registry::bundled();
        let (flash, pro) = (registry.default_model("chat", "gemini"), registry.default_model("premium", "gemini"));
        let mut wrong = Vec::new();
        let mut unserved = Vec::new();
        let mut checked = Vec::new();
        for entry in registry.models.iter().filter(|m| m.provider == "gemini" && m.kind == registry::doc::Kind::Model && m.prices.is_some()) {
            // The entry that prices the premium default is named for the
            // family; ask Google for the id that is sent.
            let id = if registry.find_priced(&pro).is_some_and(|m| m.id == entry.id) { pro.clone() } else { entry.id.clone() };
            let url = format!("{}/models/{id}", crate::ai::gemini::API_BASE);
            let response = reqwest::Client::new().get(url).header("x-goog-api-key", &key).send().await.unwrap();
            let status = response.status();
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            let served = body["outputTokenLimit"].as_u64().map(|n| n as u32);
            println!("{id}: HTTP {status}, served {served:?}, recorded {:?}", entry.max_output);
            if served.is_none() && (id == flash || id == pro) {
                unserved.push((id.clone(), status));
            }
            if served.is_some() && served != entry.max_output {
                wrong.push((id.clone(), served, entry.max_output));
            }
            checked.push(id);
        }
        assert!(checked.contains(&flash) && checked.contains(&pro), "the default ids were not checked: {checked:?}");
        assert!(unserved.is_empty(), "no outputTokenLimit for a default model, so nothing was checked (bad key or retired id?): {unserved:?}");
        assert!(wrong.is_empty(), "outputTokenLimit differs from the registry: {wrong:?}");
    }

    #[test]
    fn setup_suggests_the_largest_approved_model_the_ram_runs() {
        assert_eq!(suggested_local_model(32.0).as_deref(), Some("qwen3.5:9b"));
        assert_eq!(suggested_local_model(16.0).as_deref(), Some("qwen3.5:4b"));
        assert_eq!(suggested_local_model(8.0).as_deref(), Some("qwen3.5:2b"));
        assert_eq!(suggested_local_model(4.0), None);
    }
}
