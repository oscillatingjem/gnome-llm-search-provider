//! User-written regexes (routing rules) are Python `re` patterns, searched case-insensitively.
//! They run on fancy-regex, after translating the differences that are cheap to fix.

use fancy_regex::{Regex, RegexBuilder};

/// Translate Python `re` syntax to fancy-regex syntax where they differ.
pub fn translate(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                let n = chars[i + 1];
                match n {
                    'Z' if !in_class => out.push_str("\\z"),
                    's' => out.push_str(if in_class { "\\s\\x1c-\\x1f" } else { "[\\s\\x1c-\\x1f]" }),
                    'S' if !in_class => out.push_str("[^\\s\\x1c-\\x1f]"),
                    'p' | 'P' | 'K' | 'G' => return Err(format!("bad escape \\{n} at position {i}")),
                    _ => {
                        out.push('\\');
                        out.push(n);
                    }
                }
                i += 2;
                continue;
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                // A leading ']' or '^]' is literal in both engines.
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                if chars.get(i + 1) == Some(&']') {
                    out.push_str("\\]");
                    i += 1;
                }
            }
            '[' if in_class => out.push_str("\\["),
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '&' | '~' | '-' if in_class && chars.get(i + 1) == Some(&c) => {
                out.push('\\');
                out.push(c);
                out.push('\\');
                out.push(c);
                i += 2;
                continue;
            }
            '{' if !in_class && !starts_quantifier(&chars[i..]) => out.push_str("\\{"),
            '{' if !in_class && chars.get(i + 1) == Some(&',') => out.push_str("{0"),
            '$' if !in_class => out.push_str("(?=\\n?\\z)"),
            '(' if !in_class && chars.get(i + 1) == Some(&'?') && chars.get(i + 2) == Some(&'<')
                && !matches!(chars.get(i + 3), Some('=') | Some('!')) =>
            {
                return Err(format!("unknown extension ?<{} at position {}", chars.get(i + 3).copied().unwrap_or(' '), i + 1));
            }
            _ => out.push(c),
        }
        i += 1;
    }
    Ok(out)
}

/// Is `{` at the start of `rest` a valid Python quantifier ({n}, {n,}, {,n}, {n,m})?
fn starts_quantifier(rest: &[char]) -> bool {
    let Some(end) = rest.iter().position(|&c| c == '}') else { return false };
    let body: String = rest[1..end].iter().collect();
    let (a, b) = match body.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (body.as_str(), None),
    };
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    match b {
        None => !a.is_empty() && digits(a),
        Some(b) => digits(a) && digits(b) && !(a.is_empty() && b.is_empty()),
    }
}

pub fn compile(pattern: &str) -> Result<Regex, String> {
    let translated = translate(pattern)?;
    RegexBuilder::new(&translated)
        .case_insensitive(true)
        .backtrack_limit(1_000_000)
        .build()
        .map_err(|e| e.to_string())
}

/// `re.search(pattern, text, re.IGNORECASE)` as a bool. A pattern error is returned, like Python raises.
pub fn search(pattern: &str, text: &str) -> Result<bool, String> {
    let re = compile(pattern)?;
    Ok(re.is_match(text).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_patterns_behave_like_python() {
        let fast = r"^\S+(\s+\S+){0,5}\??$";
        assert!(search(fast, "capital of peru?").unwrap_or(false));
        assert!(!search(fast, "one two three four five six seven eight").unwrap_or(true));
        assert!(search(r"\b(why|explain)\b", "WHY does this").unwrap_or(false));
        assert!(search(r"\w+\(\)", "call main() now").unwrap_or(false));
        assert!(search(r"a{,2}b", "aab").unwrap_or(false));
        assert!(search(r"x{y", "x{y").unwrap_or(false));
        assert!(search("end$", "the end\n").unwrap_or(false));
    }
}
