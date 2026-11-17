//! What the app asks the registry: prices, labels, limits, defaults, the
//! local catalog and whether a saved model may still be used. The rules are
//! those of registry/lookup.ts and of models.rs before P05.

use super::doc::{Limits, Model, Registry, Status};
use crate::ai::models::{cost_micro_at, is_local, Price};

/// Whether a saved model may be used, and what replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
    Active,
    /// Still works; `replacement` is suggested.
    Deprecated { replacement: Option<String> },
    /// Never used. The saved choice stays until the user confirms
    /// `replacement` (DECISIONS.md section 5, "Failure handling").
    Disabled { replacement: Option<String> },
}

impl Registry {
    /// The base price, or None when no entry prices the id (a model picked
    /// from a provider's live list, or a new id). There is no fallback rate.
    pub fn price_for(&self, model_id: &str) -> Option<Price> {
        self.find_priced(model_id).and_then(|m| m.prices.as_ref()).map(|p| price(p.input, p.output, p.cache_read, p.cache_write))
    }

    /// The registry's English name for a model id, matched as `price_for`
    /// matches it. None for an id it does not list and for a family.
    pub fn label_for(&self, model_id: &str) -> Option<&str> {
        self.find_priced(model_id).filter(|m| m.kind == super::doc::Kind::Model).map(|m| m.display_name.en.as_str())
    }

    /// Always for a local model (free); for a cloud model only when priced.
    pub fn price_known(&self, provider: &str, model_id: &str) -> bool {
        is_local(provider) || self.find_priced(model_id).is_some()
    }

    pub fn max_output_tokens(&self, model_id: &str) -> Option<u32> {
        self.find_priced(model_id).and_then(|m| m.max_output)
    }

    /// What one request cost in micro-US$. None for a local model (free) and
    /// for a cloud model without a price (`price_known` tells them apart).
    /// The highest tier the input exceeds bills the whole request.
    pub fn usd_cost_micro(&self, provider: &str, model_id: &str, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) -> Option<i64> {
        if is_local(provider) {
            return None;
        }
        let Some(prices) = self.find_priced(model_id).and_then(|m| m.prices.as_ref()) else {
            log::warn!("cost: no price for model '{model_id}' in registry version {}; its usage is stored with the cost unknown", self.version);
            return None;
        };
        let p = match prices.tiers.iter().rev().find(|t| input_tokens > t.above_input_tokens) {
            Some(t) => price(t.input, t.output, t.cache_read, t.cache_write),
            None => price(prices.input, prices.output, prices.cache_read, prices.cache_write),
        };
        Some(cost_micro_at(p, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens))
    }

    /// The model `provider` uses for `feature` when the user chose none.
    /// Every default this build reads is required by [`Registry::check`];
    /// a feature without its own default for the provider uses chat's.
    pub fn default_model(&self, feature: &str, provider: &str) -> String {
        self.feature_default(feature, provider)
            .or_else(|| self.feature_default("chat", provider))
            .unwrap_or_default()
            .to_string()
    }

    pub fn limits(&self, provider: &str) -> Limits {
        self.provider(provider).map(|p| p.limits.clone()).unwrap_or_default()
    }

    /// The Ollama catalog, in file order.
    pub fn local_models(&self) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(|m| m.provider == "ollama" && m.local.is_some())
    }

    pub fn local_model(&self, id: &str) -> Option<&Model> {
        self.local_models().find(|m| m.id == id)
    }

    /// The one-click setup model for this Mac: the largest approved model
    /// its RAM runs (the last of equals, as `max_by_key` keeps).
    pub fn suggested_local_model(&self, ram_gb: f64) -> Option<&str> {
        self.local_models()
            .filter(|m| m.local.as_ref().is_some_and(|l| l.setup.is_some() && l.min_ram_gb as f64 <= ram_gb))
            .max_by_key(|m| m.local.as_ref().map(|l| l.min_ram_gb))
            .map(|m| m.id.as_str())
    }

    /// Local model families with native tool calling, for models outside
    /// the catalog.
    pub fn tool_capable_families(&self) -> Vec<String> {
        self.provider("ollama").map(|p| p.tool_capable_families.clone()).unwrap_or_default()
    }

    /// Whether `model_id`, saved for `provider` (a registry provider id),
    /// may be used. Ollama and OpenAI-compatible ids are the user's own
    /// picks and match by exact id only; an id the registry does not list is
    /// active. A disabled entry's replacement is used when it is for the
    /// same provider and active; otherwise, as for a Bedrock id whose entry
    /// is another vendor's, the provider's chat default is suggested.
    pub fn model_state(&self, provider: &str, model_id: &str) -> ModelState {
        let model_id = model_id.trim();
        let entry = if matches!(provider, "ollama" | "openai_compat" | "kimi" | "qwen" | "gemini_compat") {
            self.models.iter().find(|m| m.provider == provider && m.id == model_id)
        } else {
            self.resolve(provider, model_id)
        };
        let Some(entry) = entry else { return ModelState::Active };
        let replacement = || {
            entry
                .replacement
                .as_deref()
                .filter(|r| entry.provider == provider && self.resolve(provider, r).is_some_and(|m| m.status == Status::Active))
                .map(str::to_string)
                .or_else(|| self.feature_default("chat", provider).filter(|d| *d != model_id).map(str::to_string))
        };
        match entry.status {
            Status::Active => ModelState::Active,
            Status::Deprecated => ModelState::Deprecated { replacement: replacement() },
            Status::Disabled => ModelState::Disabled { replacement: replacement() },
        }
    }
}

fn price(input: f64, output: f64, cache_read: Option<f64>, cache_write: Option<f64>) -> Price {
    Price { input_per_million: input, output_per_million: output, cache_read_per_million: cache_read.unwrap_or(0.0), cache_write_per_million: cache_write.unwrap_or(0.0) }
}
