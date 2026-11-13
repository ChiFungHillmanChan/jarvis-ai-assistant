//! Every AI model JARVIS knows, in one place: the default ("latest") model
//! per provider, the cloud price list, the local Ollama catalog, and the one
//! function that turns token usage into cost.
//!
//! To ship a new model or retire an old one, edit this file only. Every other
//! module, and the frontend (through `ai_models`), reads from here.
//!
//! Local models run on the user's Mac and have no cost: `usd_cost_micro`
//! returns None for them and callers report the tokens instead. A cloud model
//! missing from CLOUD_MODELS has no price either: its usage is stored with the
//! cost unknown (`price_known`), never with a guessed rate.

use serde::Serialize;

// ---------------------------------------------------------------------------
// Defaults: what "Latest (recommended)" means for each provider.
// ---------------------------------------------------------------------------

/// OpenAI chat and mail.
pub const OPENAI_DEFAULT: &str = "gpt-6-luna";
/// Anthropic Claude.
pub const CLAUDE_DEFAULT: &str = "claude-sonnet-5-5";
/// Gemini for routine work, and for an explicit premium request.
pub const GEMINI_FLASH: &str = "gemini-3.7-flash";
pub const GEMINI_PRO: &str = "gemini-3.1-pro-preview";
/// AWS Bedrock, as a global cross-region inference profile.
pub const BEDROCK_DEFAULT: &str = "global.anthropic.claude-sonnet-4-6";
/// TypeSafe Jev, the optional mail classifier.
pub const JEV: &str = "jev-latest";

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

const fn price(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Price {
    Price { input_per_million: input, output_per_million: output, cache_read_per_million: cache_read, cache_write_per_million: cache_write }
}

/// The whole request is billed at `price` once its input exceeds `above`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LongContext {
    pub above: i64,
    pub price: Price,
}

#[derive(Debug, Serialize)]
pub struct CloudModel {
    pub provider: &'static str,
    /// Matched against the id a provider reports: the longest entry contained
    /// in that id wins, so "gpt-5.6-luna" beats "gpt-5" and a Bedrock profile
    /// such as "global.anthropic.claude-sonnet-4-6" finds "claude-sonnet-4-6".
    pub id: &'static str,
    pub label: &'static str,
    /// A family pattern ("gemini-flash") rather than a model a user can pick.
    pub family: bool,
    pub price: Price,
    pub long_context: Option<LongContext>,
    /// The most output tokens (thinking included) one request may ask for.
    /// None where it is not recorded; callers then keep their own cap.
    pub max_output_tokens: Option<u32>,
}

const fn model(provider: &'static str, id: &'static str, label: &'static str, price: Price) -> CloudModel {
    CloudModel { provider, id, label, family: false, price, long_context: None, max_output_tokens: None }
}

const fn family(provider: &'static str, id: &'static str, price: Price) -> CloudModel {
    CloudModel { provider, id, label: id, family: true, price, long_context: None, max_output_tokens: None }
}

/// The most output tokens (thinking included) one Gemini request may ask for.
/// Documented, not verified by a live call: on 2026-11-12 Google's model pages
/// (aistudio.google.com/docs/models/<id>) each stated "Output token limit
/// 65,536" for gemini-3.7-flash, gemini-3.1-pro-preview, gemini-3.6-flash,
/// gemini-3.5-flash, gemini-3.5-flash-lite and gemini-3.1-flash-lite. The
/// gemini-3-flash, gemini-2.5-pro and gemini-2.5-flash pages were not re-read
/// that day; 65,536 is their last published limit. The gemini-3.7-flash page
/// also says that id is deprecated and its requests are routed to
/// gemini-3.8-flash, whose page states the same 65,536; the ids and defaults
/// here are unchanged. The ignored test
/// `gemini_output_limits_match_what_google_serves` makes the live check.
const GEMINI_MAX_OUTPUT: Option<u32> = Some(65_536);

/// Sources and dates are noted per group; re-check a rate before quoting it.
pub const CLOUD_MODELS: &[CloudModel] = &[
    // OpenAI. gpt-6-luna (developers.openai.com, 2026-09-25): $0.10 / $0.01
    // cached / $0.50; cache writes assumed at 1.25x input. Above 272K input
    // the whole request is 2x input / 1.5x output.
    CloudModel {
        long_context: Some(LongContext { above: 272_000, price: price(0.20, 0.75, 0.02, 0.25) }),
        ..model("openai", "gpt-6-luna", "GPT-6 Luna", price(0.10, 0.50, 0.01, 0.125))
    },
    // gpt-5.6-luna (2026-09-04): $0.20 / $0.02 cached / $1.20, writes $0.25.
    // Its long-context tier is not modelled.
    model("openai", "gpt-5.6-luna", "GPT-5.6 Luna", price(0.20, 1.20, 0.02, 0.25)),
    family("openai", "gpt-5", price(1.25, 10.0, 0.125, 1.25)),
    model("openai", "gpt-4o-mini", "GPT-4o mini", price(0.15, 0.60, 0.075, 0.15)),
    model("openai", "gpt-4o", "GPT-4o", price(2.50, 10.0, 1.25, 2.50)),
    // Anthropic model table (2026-10-06); cache writes at 1.25x input.
    model("claude", "claude-sonnet-5-5", "Claude Sonnet 5.5", price(2.0, 10.0, 0.20, 2.50)),
    model("claude", "claude-opus-5-5", "Claude Opus 5.5", price(4.0, 20.0, 0.20, 5.0)),
    // Haiku 5.5: base rate up to 100K input, $0.50 / $2.50 beyond.
    CloudModel {
        long_context: Some(LongContext { above: 100_000, price: price(0.50, 2.50, 0.05, 0.625) }),
        ..model("claude", "claude-haiku-5-5", "Claude Haiku 5.5", price(0.10, 0.50, 0.01, 0.125))
    },
    model("claude", "claude-sonnet-4-6", "Claude Sonnet 4.6", price(3.0, 15.0, 0.30, 3.75)),
    family("claude", "sonnet-4.6", price(3.0, 15.0, 0.30, 3.75)),
    model("claude", "claude-haiku-4-5", "Claude Haiku 4.5", price(1.0, 5.0, 0.10, 1.25)),
    family("claude", "haiku-4.5", price(1.0, 5.0, 0.10, 1.25)),
    // Gemini (ai.google.dev pricing, 2026-08-07 and 2026-09-04).
    // WARNING: gemini-3.7-flash is an INTRODUCTORY rate. It doubles to
    // $1.50 / $7.50 (cache read $0.15) on 2027-01-01; update it that day or
    // every blended-cost figure for the default chain reads half the truth.
    CloudModel { max_output_tokens: GEMINI_MAX_OUTPUT, ..model("gemini", "gemini-3.7-flash", "Gemini 3.7 Flash", price(0.75, 3.75, 0.075, 0.75)) },
    CloudModel {
        long_context: Some(LongContext { above: 200_000, price: price(4.0, 18.0, 1.0, 4.0) }),
        max_output_tokens: GEMINI_MAX_OUTPUT,
        ..model("gemini", "gemini-3.1-pro", "Gemini 3.1 Pro", price(2.0, 12.0, 0.50, 2.0))
    },
    CloudModel { max_output_tokens: GEMINI_MAX_OUTPUT, ..model("gemini", "gemini-3.6-flash", "Gemini 3.6 Flash", price(1.50, 7.50, 0.375, 1.50)) },
    CloudModel { max_output_tokens: GEMINI_MAX_OUTPUT, ..model("gemini", "gemini-3.5-flash", "Gemini 3.5 Flash", price(1.50, 9.0, 0.375, 1.50)) },
    CloudModel { max_output_tokens: GEMINI_MAX_OUTPUT, ..model("gemini", "gemini-3.5-flash-lite", "Gemini 3.5 Flash-Lite", price(0.30, 2.50, 0.075, 0.30)) },
    CloudModel { max_output_tokens: GEMINI_MAX_OUTPUT, ..model("gemini", "gemini-3.1-flash-lite", "Gemini 3.1 Flash-Lite", price(0.25, 1.50, 0.0625, 0.25)) },
    // Last known preview rate; no longer on Google's public page.
    CloudModel { max_output_tokens: GEMINI_MAX_OUTPUT, ..model("gemini", "gemini-3-flash", "Gemini 3 Flash", price(0.50, 3.0, 0.125, 0.50)) },
    CloudModel {
        long_context: Some(LongContext { above: 200_000, price: price(2.50, 15.0, 0.625, 2.50) }),
        max_output_tokens: GEMINI_MAX_OUTPUT,
        ..model("gemini", "gemini-2.5-pro", "Gemini 2.5 Pro", price(1.25, 10.0, 0.31, 1.25))
    },
    family("gemini", "gemini-pro", price(1.25, 10.0, 0.31, 1.25)),
    CloudModel { max_output_tokens: GEMINI_MAX_OUTPUT, ..model("gemini", "gemini-2.5-flash", "Gemini 2.5 Flash", price(0.30, 2.50, 0.075, 0.30)) },
    family("gemini", "gemini-flash", price(0.30, 2.50, 0.075, 0.30)),
    // Amazon Nova on Bedrock.
    model("bedrock", "nova-micro", "Amazon Nova Micro", price(0.035, 0.14, 0.00875, 0.04375)),
    model("bedrock", "nova-lite", "Amazon Nova Lite", price(0.06, 0.24, 0.015, 0.075)),
    model("bedrock", "nova-pro", "Amazon Nova Pro", price(0.80, 3.20, 0.20, 1.00)),
    // TypeSafe Jev (docs.typesafe.ai/models, 2026-09-25): $0.042 input, output free.
    family("typesafe", "jev", price(0.042, 0.0, 0.042, 0.042)),
];

fn find(model_id: &str) -> Option<&'static CloudModel> {
    let m = model_id.to_lowercase();
    CLOUD_MODELS.iter().filter(|entry| m.contains(entry.id)).max_by_key(|entry| entry.id.len())
}

/// The base price of a cloud model, or None when CLOUD_MODELS has no entry
/// for it (a model picked from a provider's live list, or a new id). There is
/// no fallback rate: a guess either hides spend ($0) or invents it.
pub fn price_for(model_id: &str) -> Option<Price> {
    let found = find(model_id).map(|entry| entry.price);
    if found.is_none() {
        log::warn!("cost: no price for model '{model_id}'; its usage is stored with the cost unknown. Add it to CLOUD_MODELS in ai/models.rs.");
    }
    found
}

/// Whether usage of this model can be turned into money: always for a local
/// model (free), and for a cloud model only when CLOUD_MODELS prices it.
pub fn price_known(provider: &str, model_id: &str) -> bool {
    is_local(provider) || find(model_id).is_some()
}

/// The output cap one request to this model may ask for, when recorded.
pub fn max_output_tokens(model_id: &str) -> Option<u32> {
    find(model_id).and_then(|entry| entry.max_output_tokens)
}

// ---------------------------------------------------------------------------
// Cost: the only place usage becomes money.
// ---------------------------------------------------------------------------

/// Providers that run on the user's own Mac. "ollama" and "本機 ai" are the
/// names a plain completion reports.
pub fn is_local(provider: &str) -> bool {
    matches!(provider.to_lowercase().as_str(), "local" | "ollama" | "本機 ai")
}

/// What one request cost in micro-USD. None when there is no money figure:
/// a local model, which is free (report its tokens instead), or a cloud model
/// without a price (`price_known` tells the two apart).
pub fn usd_cost_micro(provider: &str, model_id: &str, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) -> Option<i64> {
    if is_local(provider) {
        return None;
    }
    let p = match find(model_id).and_then(|entry| entry.long_context) {
        Some(long) if input_tokens > long.above => long.price,
        _ => price_for(model_id)?,
    };
    Some(cost_micro_at(p, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens))
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
#[derive(Debug, Serialize)]
pub struct LocalSetup {
    /// Free disk needed to download and run it.
    pub disk_reserve_gb: f64,
    /// Largest context JARVIS asks for, whatever the model reports.
    pub context_cap: usize,
    /// Turn thinking off: these small models otherwise spend the output
    /// budget thinking and return no answer.
    pub think_off: bool,
}

#[derive(Debug, Serialize)]
pub struct LocalModel {
    pub id: &'static str,
    pub label: &'static str,
    pub param_size: &'static str,
    pub download_gb: f64,
    pub min_ram_gb: u32,
    pub tool_capable: bool,
    pub description: &'static str,
    /// Some for the models offered in setup and approved for mail.
    pub setup: Option<LocalSetup>,
}

const fn local(id: &'static str, label: &'static str, param_size: &'static str, download_gb: f64, min_ram_gb: u32, tool_capable: bool, description: &'static str) -> LocalModel {
    LocalModel { id, label, param_size, download_gb, min_ram_gb, tool_capable, description, setup: None }
}

pub const LOCAL_MODELS: &[LocalModel] = &[
    LocalModel { setup: Some(LocalSetup { disk_reserve_gb: 8.0, context_cap: 4_096, think_off: true }), ..local("qwen3.5:2b", "Qwen 3.5 2B", "2B", 2.7, 8, true, "Lightweight local assistant") },
    local("llama3.2:3b", "Llama 3.2 3B", "3B", 2.0, 8, true, "Fast, good for chat and basic tasks"),
    local("qwen2.5:7b", "Qwen 2.5 7B", "7B", 4.7, 8, true, "Strong tool use, multilingual"),
    LocalModel { setup: Some(LocalSetup { disk_reserve_gb: 10.0, context_cap: 4_096, think_off: true }), ..local("qwen3.5:4b", "Qwen 3.5 4B", "4B", 3.4, 16, true, "Multilingual local assistant") },
    LocalModel { setup: Some(LocalSetup { disk_reserve_gb: 15.0, context_cap: 8_192, think_off: true }), ..local("qwen3.5:9b", "Qwen 3.5 9B", "9B", 6.6, 24, true, "Multilingual local assistant") },
    local("mistral:7b", "Mistral 7B", "7B", 4.1, 8, true, "Fast reasoning and tool use"),
    local("qwen3:8b", "Qwen 3 8B", "8B", 5.2, 8, true, "Qwen multilingual model"),
    local("deepseek-r1:7b", "DeepSeek R1 7B", "7B", 4.7, 8, false, "Deep reasoning, no tool support"),
    local("qwen2.5:14b", "Qwen 2.5 14B", "14B", 9.0, 16, true, "Higher quality tool use"),
    local("deepseek-r1:14b", "DeepSeek R1 14B", "14B", 9.0, 16, false, "Strong reasoning at 14B"),
    local("qwen2.5:32b", "Qwen 2.5 32B", "32B", 20.0, 32, true, "Near-cloud quality"),
    local("deepseek-r1:32b", "DeepSeek R1 32B", "32B", 20.0, 32, false, "Best local reasoning"),
    local("llama3.3:70b", "Llama 3.3 70B", "70B", 43.0, 64, true, "Large general-purpose model"),
];

/// Model families with native tool calling, for local models outside the
/// catalog.
pub const TOOL_CAPABLE_FAMILIES: &[&str] = &[
    "qwen2.5", "qwen2", "qwen3", "llama3.1", "llama3.2", "llama3.3", "llama4", "mistral", "mixtral",
    "deepseek-v3", "deepseek-r1", "command-r", "command-r-plus", "firefunction", "hermes", "nemotron",
];

pub fn local_model(id: &str) -> Option<&'static LocalModel> {
    LOCAL_MODELS.iter().find(|m| m.id == id)
}

/// The one-click setup model for this Mac: the largest approved model its RAM runs.
pub fn suggested_local_model(ram_gb: f64) -> Option<&'static str> {
    LOCAL_MODELS
        .iter()
        .filter(|m| m.setup.is_some() && m.min_ram_gb as f64 <= ram_gb)
        .max_by_key(|m| m.min_ram_gb)
        .map(|m| m.id)
}

// ---------------------------------------------------------------------------
// For the frontend.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Defaults {
    pub openai: &'static str,
    pub claude: &'static str,
    pub gemini_flash: &'static str,
    pub gemini_pro: &'static str,
    pub bedrock: &'static str,
}

#[derive(Serialize)]
pub struct Catalog {
    pub defaults: Defaults,
    pub cloud: &'static [CloudModel],
    pub local: &'static [LocalModel],
}

/// The full model list, so the frontend never hardcodes a model id.
#[tauri::command]
pub fn ai_models() -> Catalog {
    Catalog {
        defaults: Defaults { openai: OPENAI_DEFAULT, claude: CLAUDE_DEFAULT, gemini_flash: GEMINI_FLASH, gemini_pro: GEMINI_PRO, bedrock: BEDROCK_DEFAULT },
        cloud: CLOUD_MODELS,
        local: LOCAL_MODELS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(model: &str, input: i64, output: i64, cache_read: i64) -> i64 {
        usd_cost_micro("cloud", model, input, output, cache_read, 0).unwrap()
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
    /// the family it contains, whatever the order of CLOUD_MODELS.
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
    /// most is stored with the cost unknown.
    #[test]
    fn every_default_model_has_a_real_price() {
        for id in [OPENAI_DEFAULT, CLAUDE_DEFAULT, GEMINI_FLASH, GEMINI_PRO, BEDROCK_DEFAULT, JEV] {
            assert!(find(id).is_some(), "{id} has no entry in CLOUD_MODELS");
        }
        assert_eq!(price_for(GEMINI_FLASH).unwrap().input_per_million, 0.75, "gemini-3.7-flash rate; it doubles on 2027-01-01");
    }

    /// The output limits above are documented, not checked live (see
    /// GEMINI_MAX_OUTPUT). Asking for more than a model allows is a 400, which
    /// stops the chain, so check them against what Google reports before
    /// relying on them. The default Flash and Pro ids must report a limit: a
    /// bad key or a retired id answers without one, and that proves nothing.
    /// Run deliberately:
    ///   GEMINI_API_KEY=... cargo test --manifest-path src-tauri/Cargo.toml \
    ///     gemini_output_limits_match_what_google_serves -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn gemini_output_limits_match_what_google_serves() {
        let key = std::env::var("GEMINI_API_KEY").expect("set GEMINI_API_KEY to run this test");
        let mut wrong = Vec::new();
        let mut unserved = Vec::new();
        let mut checked = Vec::new();
        for entry in CLOUD_MODELS.iter().filter(|m| m.provider == "gemini" && !m.family) {
            let id = if entry.id == "gemini-3.1-pro" { GEMINI_PRO } else { entry.id };
            let url = format!("{}/models/{id}", crate::ai::gemini::API_BASE);
            let response = reqwest::Client::new().get(url).header("x-goog-api-key", &key).send().await.unwrap();
            let status = response.status();
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            let served = body["outputTokenLimit"].as_u64().map(|n| n as u32);
            println!("{id}: HTTP {status}, served {served:?}, recorded {:?}", entry.max_output_tokens);
            checked.push(id);
            if served.is_none() && [GEMINI_FLASH, GEMINI_PRO].contains(&id) {
                unserved.push((id, status));
            }
            if served.is_some() && served != entry.max_output_tokens {
                wrong.push((id, served, entry.max_output_tokens));
            }
        }
        assert!(checked.contains(&GEMINI_FLASH) && checked.contains(&GEMINI_PRO), "the default ids were not checked: {checked:?}");
        assert!(unserved.is_empty(), "no outputTokenLimit for a default model, so nothing was checked (bad key or retired id?): {unserved:?}");
        assert!(wrong.is_empty(), "outputTokenLimit differs from models.rs: {wrong:?}");
    }

    #[test]
    fn setup_suggests_the_largest_approved_model_the_ram_runs() {
        assert_eq!(suggested_local_model(32.0), Some("qwen3.5:9b"));
        assert_eq!(suggested_local_model(16.0), Some("qwen3.5:4b"));
        assert_eq!(suggested_local_model(8.0), Some("qwen3.5:2b"));
        assert_eq!(suggested_local_model(4.0), None);
    }
}
