//! Candidates: app agents from agents.toml plus models listed by the LiteLLM gateway.

use serde_json::Value;

use crate::config::Config;
use crate::launch::{Which, app_available};
use crate::ranker::{Candidate, Kind};

pub const CACHE_SECONDS: f64 = 60.0;
pub const RETRY_SECONDS: f64 = 30.0;

pub fn app_candidates(config: &Config, which: &Which) -> Vec<Candidate> {
    config
        .apps
        .iter()
        .filter(|a| a.enabled && app_available(a, which))
        .map(|a| {
            let mut c = Candidate::new(format!("app:{}", a.id), Kind::App, a.name.clone());
            c.tags = a.tags.clone();
            c.efforts = a.efforts.clone();
            c.models = a.models.clone();
            c.icon = a.icon.clone();
            c.priority = a.priority;
            c
        })
        .collect()
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// Group LiteLLM /model/info entries (including generated effort variants) by base model.
pub fn models_from_info(data: &[Value], auto_model: &str) -> Vec<Candidate> {
    let mut by_base: Vec<(String, Candidate)> = Vec::new();
    for entry in data {
        let name = entry.get("model_name").and_then(Value::as_str).unwrap_or("");
        let empty = serde_json::Map::new();
        let info = entry.get("model_info").and_then(Value::as_object).unwrap_or(&empty);
        let text = |k: &str, default: &str| info.get(k).and_then(Value::as_str).unwrap_or(default).to_string();
        if name == auto_model {
            let mut c = Candidate::new("auto".into(), Kind::Auto, text("display_name", "Auto"));
            c.tags = str_list(info.get("tags"));
            c.icon = "emblem-system-symbolic".into();
            match by_base.iter_mut().find(|(b, _)| b == name) {
                Some(slot) => slot.1 = c,
                None => by_base.push((name.to_string(), c)),
            }
            continue;
        }
        let base = info.get("base").and_then(Value::as_str).filter(|b| !b.is_empty()).unwrap_or(name).to_string();
        let idx = match by_base.iter().position(|(b, _)| *b == base) {
            Some(i) => i,
            None => {
                let mut c = Candidate::new(format!("model:{base}"), Kind::Model, text("display_name", &base));
                c.tags = str_list(info.get("tags"));
                c.aliases = str_list(info.get("aliases")).iter().map(|a| a.to_lowercase()).collect();
                c.icon = text("icon", "user-available-symbolic");
                c.priority = info.get("priority").and_then(Value::as_f64).unwrap_or(0.0);
                by_base.push((base.clone(), c));
                by_base.len() - 1
            }
        };
        if let Some(effort) = info.get("effort").and_then(Value::as_str).filter(|e| !e.is_empty()) {
            let c = &mut by_base[idx].1;
            if !c.efforts.iter().any(|e| e == effort) {
                c.efforts.push(effort.to_string());
            }
        }
    }
    by_base.into_iter().map(|(_, c)| c).collect()
}

/// The gateway's cached model list. The HTTP fetch itself lives in the platform layer;
/// this keeps the cache and back-off timing (seconds on a monotonic clock).
#[derive(Debug, Default)]
pub struct GatewayCache {
    pub models: Vec<Candidate>,
    pub fetched: f64,
    pub failed: f64,
}

impl GatewayCache {
    /// Should we fetch now? (Python: skip while the cache is fresh or within the retry window.)
    pub fn due(&self, now: f64) -> bool {
        !(now - self.fetched < CACHE_SECONDS || now - self.failed < RETRY_SECONDS)
    }

    pub fn store(&mut self, now: f64, result: Result<Vec<Value>, String>, auto_model: &str) {
        match result {
            Ok(data) => {
                self.models = models_from_info(&data, auto_model);
                self.fetched = now;
            }
            Err(_) => {
                self.models.clear();
                self.failed = now;
            }
        }
    }
}

pub fn candidates(config: &Config, gateway_models: &[Candidate], which: &Which) -> Vec<Candidate> {
    let mut all = if config.litellm_url.is_empty() { Vec::new() } else { gateway_models.to_vec() };
    all.extend(app_candidates(config, which));
    all
}
