//! The registry as the app reads it (registry/schema.json is the contract).
//! Only the fields the app uses are typed; anything else in a newer file is
//! ignored, so a field added for the proxy (P08) does not break older apps.
//! A field the app needs that a file lacks, or holds in a form this build
//! does not know, makes the whole file unusable ([`Registry::check`]): the
//! app then keeps the copy it already has.

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Text {
    pub en: String,
    #[serde(rename = "zh-HK")]
    pub zh_hk: String,
}

/// Ordered from least to most careful, so the stricter of two is the larger.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DataTerms {
    /// Email may be sent.
    Safe,
    /// Email may be sent once the user confirms paid billing.
    NeedsBillingConfirmation,
    /// Not checked: email is not sent (the consent card is P06).
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ConsentKeys {
    pub blocked: String,
    pub confirm: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Region {
    pub id: String,
    pub base_url: String,
    pub key_page: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SetupModel {
    /// None means the feature default.
    Optional,
    /// The user must pick one.
    Required,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Setup {
    pub order: u32,
    pub key_page: Option<String>,
    pub model: SetupModel,
    pub hint: Text,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Limits {
    pub chat_max_output_tokens: Option<u32>,
    pub default_max_output_tokens: Option<u32>,
    pub stt_max_output_tokens: Option<u32>,
    pub completion_context_tokens: Option<u32>,
    pub default_context_tokens: Option<u32>,
    pub max_tool_rounds: Option<u32>,
    pub chat_reasoning_effort: Option<String>,
    pub chat_temperature: Option<f32>,
    pub api_version: Option<String>,
}

impl Limits {
    /// Fill each limit this copy leaves out with `other`'s.
    pub(crate) fn fill_from(&mut self, other: &Limits) {
        fn fill<T: Clone>(mine: &mut Option<T>, theirs: &Option<T>) {
            if mine.is_none() {
                *mine = theirs.clone();
            }
        }
        fill(&mut self.chat_max_output_tokens, &other.chat_max_output_tokens);
        fill(&mut self.default_max_output_tokens, &other.default_max_output_tokens);
        fill(&mut self.stt_max_output_tokens, &other.stt_max_output_tokens);
        fill(&mut self.completion_context_tokens, &other.completion_context_tokens);
        fill(&mut self.default_context_tokens, &other.default_context_tokens);
        fill(&mut self.max_tool_rounds, &other.max_tool_rounds);
        fill(&mut self.chat_reasoning_effort, &other.chat_reasoning_effort);
        fill(&mut self.chat_temperature, &other.chat_temperature);
        fill(&mut self.api_version, &other.api_version);
    }

    /// Every limit this copy sets is inside its bound (registry/README.md,
    /// "Limits"). The error names the field, never its value.
    pub(crate) fn check(&self, provider: &str) -> Result<(), String> {
        let field = |name: &str| format!("providers.{provider}.limits.{name}");
        let counts = [
            ("chat_max_output_tokens", self.chat_max_output_tokens, MAX_OUTPUT_TOKENS),
            ("default_max_output_tokens", self.default_max_output_tokens, MAX_OUTPUT_TOKENS),
            ("stt_max_output_tokens", self.stt_max_output_tokens, MAX_OUTPUT_TOKENS),
            ("completion_context_tokens", self.completion_context_tokens, MAX_CONTEXT_TOKENS),
            ("default_context_tokens", self.default_context_tokens, MAX_CONTEXT_TOKENS),
            ("max_tool_rounds", self.max_tool_rounds, MAX_TOOL_ROUNDS),
        ];
        for (name, value, max) in counts {
            if value.is_some_and(|v| !(1..=max).contains(&v)) {
                return Err(format!("{} is outside 1 to {max}", field(name)));
            }
        }
        if self.chat_temperature.is_some_and(|t| !(0.0..=MAX_TEMPERATURE).contains(&t)) {
            return Err(format!("{} is outside 0 to {MAX_TEMPERATURE}", field("chat_temperature")));
        }
        if self.chat_reasoning_effort.as_deref().is_some_and(|effort| !REASONING_EFFORTS.contains(&effort)) {
            return Err(format!("{} is not one of {}", field("chat_reasoning_effort"), REASONING_EFFORTS.join(", ")));
        }
        if self.api_version.as_deref().is_some_and(|v| !is_date(v)) {
            return Err(format!("{} is not a date (YYYY-MM-DD)", field("api_version")));
        }
        Ok(())
    }
}

/// The limits of one feature's own requests, whichever provider answers them
/// (registry `feature_limits.<feature>`): mail analysis and mail sorting.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct FeatureLimits {
    pub max_output_tokens: Option<u32>,
}

impl FeatureLimits {
    /// Fill each limit this copy leaves out with `other`'s.
    pub(crate) fn fill_from(&mut self, other: &FeatureLimits) {
        if self.max_output_tokens.is_none() {
            self.max_output_tokens = other.max_output_tokens;
        }
    }

    /// Every limit this copy sets is inside its bound. The error names the
    /// field, never its value.
    pub(crate) fn check(&self, feature: &str) -> Result<(), String> {
        if self.max_output_tokens.is_some_and(|v| !(1..=MAX_OUTPUT_TOKENS).contains(&v)) {
            return Err(format!("feature_limits.{feature}.max_output_tokens is outside 1 to {MAX_OUTPUT_TOKENS}"));
        }
        Ok(())
    }
}

/// The bounds of the limits, inclusive, mirrored in registry/schema.json.
/// Version 1 holds at most 4,096 output tokens, 8,192 context tokens and 5
/// tool rounds; each maximum is at least ten times that. Output: 131,072
/// covers the largest output any listed model documents. Context: 1,048,576
/// covers a 1M-token context.
pub(crate) const MAX_OUTPUT_TOKENS: u32 = 131_072;
pub(crate) const MAX_CONTEXT_TOKENS: u32 = 1_048_576;
pub(crate) const MAX_TOOL_ROUNDS: u32 = 50;
/// A model's own output cap (`models[].max_output`): version 1's largest is
/// 65,536, so 16 times that. `models[].context` has no bound: the app does
/// not read it (the Model type has no such field).
pub(crate) const MAX_MODEL_OUTPUT_TOKENS: u32 = 1_048_576;
pub(crate) const MAX_TEMPERATURE: f32 = 2.0;
/// The values OpenAI lists for reasoning effort ("Supported values are
/// model-dependent and can include none, minimal, low, medium, high, xhigh,
/// and max": developers.openai.com/api/docs/guides/reasoning, read
/// 2026-11-17). ai/openai.rs sends the value as written and holds no list.
pub(crate) const REASONING_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// YYYY-MM-DD in shape (Anthropic's `anthropic-version` header).
fn is_date(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

/// A model id the app may put in a URL path or a request: letters, digits
/// and `. _ : / -`, with no `..` (mirrored in registry/schema.json).
pub(crate) fn is_safe_model_id(id: &str) -> bool {
    !id.is_empty() && !id.contains("..") && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(&c))
}

/// The limits this build reads, by provider. The bundled copy carries every
/// one (`the_bundled_registry_carries_every_limit_the_app_reads`); a fetched
/// copy that leaves one out keeps the bundled value (`load::apply_floor`).
#[cfg(test)]
pub(crate) const REQUIRED_LIMITS: &[(&str, &str)] = &[
    ("openai", "chat_max_output_tokens"),
    ("openai", "max_tool_rounds"),
    ("openai", "chat_reasoning_effort"),
    ("openai_compat", "chat_max_output_tokens"),
    ("openai_compat", "max_tool_rounds"),
    ("kimi", "chat_max_output_tokens"),
    ("kimi", "max_tool_rounds"),
    ("qwen", "chat_max_output_tokens"),
    ("qwen", "max_tool_rounds"),
    ("gemini_compat", "chat_max_output_tokens"),
    ("gemini_compat", "max_tool_rounds"),
    ("claude", "chat_max_output_tokens"),
    ("claude", "max_tool_rounds"),
    ("claude", "api_version"),
    ("gemini", "default_max_output_tokens"),
    ("gemini", "stt_max_output_tokens"),
    ("gemini", "max_tool_rounds"),
    ("bedrock", "chat_max_output_tokens"),
    ("bedrock", "chat_temperature"),
    ("bedrock", "max_tool_rounds"),
    ("ollama", "completion_context_tokens"),
    ("ollama", "default_context_tokens"),
    ("ollama", "max_tool_rounds"),
    ("ollama", "chat_max_output_tokens"),
];

/// The feature limits this build reads (`feature_limits.<feature>.max_output_tokens`).
/// The bundled copy carries each, and a fetched copy that leaves one out
/// keeps the bundled value, as for [`REQUIRED_LIMITS`].
#[cfg(test)]
pub(crate) const REQUIRED_FEATURE_LIMITS: &[&str] = &["mail_analysis", "mail_sort"];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Provider {
    pub id: String,
    pub wire: String,
    pub display_name: Text,
    pub base_url: Option<String>,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub host_suffixes: Vec<String>,
    #[serde(default)]
    pub model_id_suffixes: Vec<String>,
    pub data_terms: DataTerms,
    pub consent_copy_keys: Option<ConsentKeys>,
    pub setup: Option<Setup>,
    pub model_id_pattern: Option<String>,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub inference_geographies: Vec<String>,
    #[serde(default)]
    pub built_in_models: Vec<String>,
    #[serde(default)]
    pub tool_capable_families: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Model,
    /// A price pattern for a model family, not a model a user can pick.
    Family,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    /// Still works; the replacement is suggested.
    Deprecated,
    /// Never used. A saved choice of it waits for the user to confirm the
    /// replacement (DECISIONS.md section 5, "Failure handling").
    Disabled,
}

/// US$ per 1M tokens. A rate the provider does not list is null and bills
/// at zero, as registry/lookup.ts `costMicros` does.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Tier {
    pub above_input_tokens: i64,
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    /// Once a request's input exceeds `above_input_tokens`, the whole
    /// request is billed at that tier.
    pub tiers: Vec<Tier>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Capabilities {
    pub tools: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LocalSetup {
    pub disk_reserve_gb: f64,
    pub context_cap: usize,
    pub think_off: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Local {
    pub param_size: String,
    pub download_gb: f64,
    pub min_ram_gb: u32,
    pub description: String,
    pub setup: Option<LocalSetup>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Download {
    pub url: String,
    pub sha256: String,
    pub min_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Model {
    pub id: String,
    pub provider: String,
    pub kind: Kind,
    pub display_name: Text,
    pub status: Status,
    pub replacement: Option<String>,
    pub managed: bool,
    pub prices: Option<Prices>,
    pub max_output: Option<u32>,
    pub capabilities: Capabilities,
    pub local: Option<Local>,
    pub download: Option<Download>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ChatModelFilter {
    pub strip_prefix: String,
    pub exclude_words: Vec<String>,
    pub exclude_dated_snapshots: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct PresetEntry {
    pub provider: String,
    /// None: the workload picks it (the premium default for a premium
    /// request, the feature's default otherwise).
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Preset {
    pub label: Text,
    pub entries: Vec<PresetEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Presets {
    pub default: String,
    /// In file order, so Settings lists them as the file does.
    #[serde(deserialize_with = "in_file_order")]
    pub chains: Vec<(String, Preset)>,
}

fn in_file_order<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<(String, Preset)>, D::Error> {
    struct Chains;
    impl<'de> Visitor<'de> for Chains {
        type Value = Vec<(String, Preset)>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a map of preset chains")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut chains = Vec::new();
            while let Some(entry) = map.next_entry::<String, Preset>()? {
                chains.push(entry);
            }
            Ok(chains)
        }
    }
    deserializer.deserialize_map(Chains)
}

/// The parsed registry. Construct with [`Registry::parse`], which also runs
/// [`Registry::check`].
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Registry {
    pub version: u64,
    pub issued_at: String,
    pub min_app_version: String,
    pub providers: Vec<Provider>,
    pub models: Vec<Model>,
    pub features: BTreeMap<String, BTreeMap<String, String>>,
    /// Added in version 3; a copy without it keeps the bundled values
    /// (`load::apply_floor`). A feature this build does not read is never
    /// read, though its limits are still checked against their bounds.
    #[serde(default)]
    pub feature_limits: BTreeMap<String, FeatureLimits>,
    pub chat_model_filter: ChatModelFilter,
    pub presets: Presets,
}

/// The feature defaults and provider settings this build reads. A file that
/// lacks any of them is not used.
pub(crate) const REQUIRED_DEFAULTS: &[(&str, &str)] = &[
    ("chat", "openai"),
    ("chat", "claude"),
    ("chat", "gemini"),
    ("chat", "bedrock"),
    ("premium", "openai"),
    ("premium", "claude"),
    ("premium", "gemini"),
    ("premium", "bedrock"),
    ("mail_analysis", "openai"),
    ("mail_sort", "openai"),
    ("mail_sort", "typesafe"),
    ("stt", "gemini"),
    ("stt", "whisper"),
    ("tts", "minimax"),
];
pub(crate) const REQUIRED_PROVIDERS: &[&str] = &["openai", "claude", "gemini", "bedrock", "ollama", "ollama_cloud", "typesafe", "minimax", "whisper"];

impl Registry {
    /// Parse and check a registry. The error says what is wrong without
    /// quoting the file: a parse error gives its kind and position only.
    pub fn parse(bytes: &[u8]) -> Result<Registry, String> {
        let registry: Registry = serde_json::from_slice(bytes).map_err(|e| format!("not a registry this build can read ({:?} error at line {}, column {})", e.classify(), e.line(), e.column()))?;
        registry.check()?;
        Ok(registry)
    }

    /// Everything the app reads must be there and point at a real entry.
    pub fn check(&self) -> Result<(), String> {
        for id in REQUIRED_PROVIDERS {
            if self.provider(id).is_none() {
                return Err(format!("provider {id} is missing"));
            }
        }
        for (feature, provider) in REQUIRED_DEFAULTS {
            let id = self.feature_default(feature, provider).ok_or_else(|| format!("features.{feature}.{provider} is missing"))?;
            if self.resolve(provider, id).is_none() {
                return Err(format!("features.{feature}.{provider} names no model entry"));
            }
        }
        let whisper = self.feature_default("stt", "whisper").unwrap_or_default();
        if self.models.iter().find(|m| m.provider == "whisper" && m.id == whisper).and_then(|m| m.download.as_ref()).is_none() {
            return Err("the speech-to-text model on this Mac has no download".into());
        }
        if self.preset(&self.presets.default).is_none() {
            return Err("presets.default names no chain".into());
        }
        if let Some((name, _)) = self.presets.chains.iter().find(|(_, chain)| chain.entries.is_empty()) {
            return Err(format!("presets.chains.{name} is empty"));
        }
        if parse_version(&self.min_app_version).is_none() {
            return Err("min_app_version is not x.y.z".into());
        }
        for (feature, limits) in &self.feature_limits {
            limits.check(feature)?;
        }
        for provider in &self.providers {
            provider.limits.check(&provider.id)?;
            if !provider.built_in_models.iter().all(|id| is_safe_model_id(id)) {
                return Err(format!("providers.{}.built_in_models holds an id that is not a safe model id", provider.id));
            }
        }
        for model in &self.models {
            if !is_safe_model_id(&model.id) || !model.replacement.as_deref().is_none_or(is_safe_model_id) {
                return Err(format!("a {} model's id or replacement is not a safe model id", model.provider));
            }
            // Gemini sends it as maxOutputTokens (gemini.rs output_cap).
            if model.max_output.is_some_and(|v| !(1..=MAX_MODEL_OUTPUT_TOKENS).contains(&v)) {
                return Err(format!("models.{}.max_output is outside 1 to {MAX_MODEL_OUTPUT_TOKENS}", model.id));
            }
        }
        // A default and a preset entry are sent when the user saved no
        // model, so each must be safe to send and active.
        for (feature, defaults) in &self.features {
            for (provider, id) in defaults {
                if !is_safe_model_id(id) {
                    return Err(format!("features.{feature}.{provider} is not a safe model id"));
                }
                if self.model_state(provider, id) != super::lookup::ModelState::Active {
                    return Err(format!("features.{feature}.{provider} names a model that is not active"));
                }
            }
        }
        for (name, chain) in &self.presets.chains {
            for (i, entry) in chain.entries.iter().enumerate() {
                let Some(id) = entry.model.as_deref() else { continue };
                if !is_safe_model_id(id) {
                    return Err(format!("presets.chains.{name}[{i}] is not a safe model id"));
                }
                if self.model_state(&entry.provider, id) != super::lookup::ModelState::Active {
                    return Err(format!("presets.chains.{name}[{i}] names a model that is not active"));
                }
            }
        }
        Ok(())
    }

    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn feature_default(&self, feature: &str, provider: &str) -> Option<&str> {
        self.features.get(feature).and_then(|f| f.get(provider)).map(String::as_str)
    }

    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets.chains.iter().find(|(id, _)| id == name).map(|(_, chain)| chain)
    }

    /// The price entry for a model id a cloud provider reports: among the
    /// entries that carry a price and are not JARVIS AI's own, the longest
    /// id contained in it, compared in lower case; on a tie the later entry
    /// (registry/lookup.ts `findPriced`, and models.rs before P05).
    pub fn find_priced(&self, model_id: &str) -> Option<&Model> {
        let wanted = model_id.to_lowercase();
        self.models
            .iter()
            .filter(|m| !m.managed && m.prices.is_some() && wanted.contains(m.id.as_str()))
            .max_by_key(|m| m.id.len())
    }

    /// The entry a configured or saved id stands for: an unmanaged entry
    /// with exactly this id, else the price lookup (registry/lookup.ts
    /// `resolveModel`). `_provider` is the provider the id is used with;
    /// Bedrock ids are other vendors' models, so the entry's own provider
    /// may differ.
    pub fn resolve(&self, _provider: &str, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| !m.managed && m.id == id).or_else(|| self.find_priced(id))
    }
}

/// "x.y.z" as numbers; anything after a '-' or '+' is ignored.
pub fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}
