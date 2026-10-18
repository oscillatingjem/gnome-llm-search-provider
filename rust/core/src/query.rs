//! Parse a search query into model/effort tokens and the prompt text.
//!
//! Tokens can appear anywhere in the query:
//!     @name   pick a model or agent by alias (prefix match)
//!     !level  pick an effort level (low, medium, high, xhigh, max)
//! Everything else is the prompt.

use crate::pystr;

pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub fn effort_alias(s: &str) -> Option<&'static str> {
    Some(match s {
        "l" | "lo" | "low" => "low",
        "m" | "med" | "medium" => "medium",
        "h" | "hi" | "high" => "high",
        "xh" | "xhigh" => "xhigh",
        "x" | "max" => "max",
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub prompt: String,
    /// Raw @alias text, lowercased, without the "@".
    pub model: Option<String>,
    /// One of `EFFORTS`.
    pub effort: Option<&'static str>,
}

/// Parse GNOME's search terms (the query split on whitespace).
pub fn parse<S: AsRef<str>>(terms: &[S]) -> Query {
    let mut words: Vec<&str> = Vec::new();
    let (mut model, mut effort) = (None, None);
    for term in terms {
        let term = term.as_ref();
        let mut chars = term.chars();
        let first = chars.next();
        let rest = chars.as_str();
        let long = term.chars().count() > 1;
        if long && first == Some('@') && model.is_none() {
            model = Some(rest.to_lowercase());
        } else if long && first == Some('!') && effort.is_none() && effort_alias(&rest.to_lowercase()).is_some() {
            effort = effort_alias(&rest.to_lowercase());
        } else {
            words.push(term);
        }
    }
    Query { prompt: pystr::strip(&words.join(" ")).to_string(), model, effort }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_anywhere() {
        let q = parse(&["refactor", "!med", "this", "@Son"]);
        assert_eq!(q, Query { prompt: "refactor this".into(), model: Some("son".into()), effort: Some("medium") });
        assert_eq!(parse(&["wow!", "!nope"]).prompt, "wow! !nope");
        assert_eq!(parse(&["@", "alone"]).prompt, "@ alone");
    }
}
