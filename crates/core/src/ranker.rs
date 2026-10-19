//! Turn a parsed query and the available agents/models into ranked search rows.

use blake2::Blake2sVar;
use blake2::digest::{Update, VariableOutput};

use crate::config::Config;
use crate::pyre;
use crate::pystr;
use crate::query::{EFFORTS, Query};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    App,
    Model,
    Auto,
}

/// Something a prompt can be sent to: an app agent, a gateway model, or the Auto router.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// "app:<id>", "model:<base id>", "auto"
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub tags: Vec<String>,
    pub efforts: Vec<String>,
    /// App agents only.
    pub models: Vec<String>,
    pub aliases: Vec<String>,
    pub icon: String,
    pub priority: f64,
}

impl Candidate {
    pub fn new(id: String, kind: Kind, name: String) -> Self {
        Candidate {
            id,
            kind,
            name,
            tags: Vec::new(),
            efforts: Vec::new(),
            models: Vec::new(),
            aliases: Vec::new(),
            icon: "system-search-symbolic".into(),
            priority: 0.0,
        }
    }

    fn short_id(&self) -> &str {
        self.id.split_once(':').map_or(self.id.as_str(), |(_, rest)| rest)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub candidate: Candidate,
    /// App agents: chosen model.
    pub model: Option<String>,
    pub effort: Option<String>,
    pub reasons: Vec<String>,
    /// Changes with the query; GNOME Shell caches rows by id.
    pub query_key: String,
}

impl Row {
    pub fn result_id(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.candidate.id,
            self.model.as_deref().unwrap_or(""),
            self.effort.as_deref().unwrap_or(""),
            self.query_key
        )
    }

    pub fn title(&self) -> String {
        match &self.model {
            Some(m) if !m.is_empty() => format!("{} · {}", self.candidate.name, pystr::capitalize(m)),
            _ => self.candidate.name.clone(),
        }
    }

    pub fn subtitle(&self) -> String {
        let mut parts = Vec::new();
        if let Some(e) = self.effort.as_deref().filter(|e| !e.is_empty()) {
            parts.push(format!("{e} effort"));
        }
        if !self.reasons.is_empty() {
            parts.push(self.reasons.join(", "));
        }
        parts.join(" · ")
    }
}

pub fn query_key(q: &Query) -> String {
    let text = format!("{}\0{}\0{}", q.model.as_deref().unwrap_or(""), q.effort.unwrap_or(""), q.prompt);
    let mut hasher = Blake2sVar::new(6).unwrap_or_else(|_| unreachable!("6 is a valid BLAKE2s length"));
    hasher.update(text.as_bytes());
    let mut out = [0u8; 6];
    let _ = hasher.finalize_variable(&mut out);
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// (candidate id, model, effort) from a result id; empty parts become None.
pub fn parse_result_id(result_id: &str) -> (String, Option<String>, Option<String>) {
    let mut parts = result_id.split('|');
    let cid = parts.next().unwrap_or("").to_string();
    let some = |s: Option<&str>| s.filter(|s| !s.is_empty()).map(str::to_string);
    let model = some(parts.next());
    let effort = some(parts.next());
    (cid, model, effort)
}

pub fn suggest_effort(prompt: &str, config: &Config) -> Result<String, String> {
    for p in &config.high_effort_patterns {
        if pyre::search(p, prompt)? {
            return Ok("high".into());
        }
    }
    let words = pystr::split(prompt).len() as i64;
    Ok(if words <= config.low_effort_max_words {
        "low"
    } else if words > 60 {
        "high"
    } else {
        "medium"
    }
    .into())
}

/// Pick the supported effort closest to the requested one.
pub fn clamp_effort(effort: Option<&str>, supported: &[String]) -> Option<String> {
    let effort = effort.filter(|e| !e.is_empty())?;
    if supported.is_empty() {
        return None;
    }
    if supported.iter().any(|s| s == effort) {
        return Some(effort.to_string());
    }
    let want = EFFORTS.iter().position(|e| *e == effort)? as i64;
    let distance = |e: &str| EFFORTS.iter().position(|x| *x == e).map_or(99, |i| (i as i64 - want).abs());
    // Python's min() keeps the first of equal keys.
    let mut best = &supported[0];
    for s in &supported[1..] {
        if distance(s) < distance(best) {
            best = s;
        }
    }
    Some(best.clone())
}

/// Tag → summed weight, in the order tags first match (Python dict order).
pub fn matched_tags(prompt: &str, config: &Config) -> Result<Vec<(String, f64)>, String> {
    let mut weights: Vec<(String, f64)> = Vec::new();
    for rule in &config.rules {
        let mut hit = false;
        for p in &rule.patterns {
            if pyre::search(p, prompt)? {
                hit = true;
                break;
            }
        }
        if hit {
            match weights.iter_mut().find(|(t, _)| *t == rule.tag) {
                Some((_, w)) => *w += rule.weight,
                None => weights.push((rule.tag.clone(), rule.weight)),
            }
        }
    }
    Ok(weights)
}

/// Resolve an @alias to a candidate (and, for app agents, one of its models).
pub fn resolve<'a>(name: &str, candidates: &'a [Candidate], config: &Config) -> Option<(&'a Candidate, Option<String>)> {
    if let Some(target) = config.alias(name).filter(|t| !t.is_empty()) {
        let (cid, model) = target.split_once(':').unwrap_or((target, ""));
        for c in candidates {
            if c.id == cid || c.id == format!("app:{cid}") || c.id == format!("model:{cid}") {
                return Some((c, Some(model.to_string()).filter(|m| !m.is_empty())));
            }
        }
    }
    let keys = |c: &Candidate| -> Vec<String> {
        let mut k = vec![c.short_id().to_string(), c.name.to_lowercase()];
        k.extend(c.aliases.iter().cloned());
        k
    };
    for c in candidates {
        if keys(c).iter().any(|k| k == name) {
            return Some((c, None));
        }
    }
    for c in candidates {
        if c.kind == Kind::App {
            if let Some(m) = c.models.iter().find(|m| m.starts_with(name)) {
                return Some((c, Some(m.clone())));
            }
        }
    }
    for c in candidates {
        if keys(c).iter().any(|k| k.starts_with(name)) {
            return Some((c, None));
        }
    }
    None
}

pub fn rank(q: &Query, candidates: &[Candidate], config: &Config) -> Result<Vec<Row>, String> {
    if (q.prompt.chars().count() as i64) < config.min_chars {
        return Ok(Vec::new());
    }
    let effort = match q.effort {
        Some(e) => e.to_string(),
        None => suggest_effort(&q.prompt, config)?,
    };
    let tags = matched_tags(&q.prompt, config)?;
    let key = query_key(q);

    let make_row = |c: &Candidate, model: Option<String>| Row {
        candidate: c.clone(),
        model,
        effort: clamp_effort(Some(&effort), &c.efforts),
        reasons: tags.iter().filter(|(t, _)| c.tags.contains(t)).map(|(t, _)| t.clone()).collect(),
        query_key: key.clone(),
    };

    let mut rows: Vec<Row> = Vec::new();
    if let Some(name) = &q.model {
        if let Some((c, model)) = resolve(name, candidates, config) {
            rows.push(make_row(c, model));
        }
    }

    let score = |c: &Candidate| c.priority + tags.iter().filter(|(t, _)| c.tags.contains(t)).map(|(_, w)| w).sum::<f64>();
    let mut ranked: Vec<&Candidate> = candidates.iter().filter(|c| c.kind != Kind::Auto).collect();
    // Stable, descending: equal scores keep their original order, like Python's sorted(reverse=True).
    ranked.sort_by(|a, b| score(b).partial_cmp(&score(a)).unwrap_or(std::cmp::Ordering::Equal));
    let auto = candidates.iter().filter(|c| c.kind == Kind::Auto);

    for c in auto.chain(ranked.iter().copied()) {
        if rows.len() as i64 >= config.max_rows {
            break;
        }
        if !rows.iter().any(|r| r.candidate.id == c.id) {
            rows.push(make_row(c, None));
        }
    }

    // Always keep the best app agent visible, even when many gateway models outrank it.
    if let Some(best) = ranked.iter().find(|c| c.kind == Kind::App) {
        if !rows.iter().any(|r| r.candidate.kind == Kind::App) {
            rows.pop();
            rows.push(make_row(best, None));
        }
    }
    Ok(rows)
}
