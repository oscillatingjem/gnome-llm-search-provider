//! Change single values in agents.toml without disturbing anything else.
//!
//! A line-for-line port of the Python editor (toml_edit.py), so both write identical files:
//!   - a key that exists is changed in place, keeping any comment after the value;
//!   - a missing key is added right below its table's header (below `id` in [[app]] entries);
//!   - a missing table is added at the end of the file.
//! Only single-line scalar values are written. Every edit is checked by parsing the result and
//! reading the value back; if that fails, nothing is changed.

use std::sync::OnceLock;

use regex::Regex;
use toml::{Table, Value};

use crate::pystr;

#[derive(Debug, Clone, PartialEq)]
pub struct EditError(pub String);

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A value the editor can write.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl Scalar {
    pub fn from_toml(v: &Value) -> Option<Scalar> {
        Some(match v {
            Value::Boolean(b) => Scalar::Bool(*b),
            Value::Integer(i) => Scalar::Int(*i),
            Value::Float(f) => Scalar::Float(*f),
            Value::String(s) => Scalar::Str(s.clone()),
            _ => return None,
        })
    }

    pub fn from_json(v: &serde_json::Value) -> Option<Scalar> {
        Some(match v {
            serde_json::Value::Bool(b) => Scalar::Bool(*b),
            serde_json::Value::Number(n) if n.is_i64() => Scalar::Int(n.as_i64()?),
            serde_json::Value::Number(n) => Scalar::Float(n.as_f64()?),
            serde_json::Value::String(s) => Scalar::Str(s.clone()),
            _ => return None,
        })
    }

    /// The TOML text written for this value (Python's format_value).
    pub fn format(&self) -> String {
        match self {
            Scalar::Bool(b) => if *b { "true".into() } else { "false".into() },
            Scalar::Int(i) => i.to_string(),
            Scalar::Float(f) => pystr::repr_float(*f),
            Scalar::Str(s) => pystr::json_dumps_str(s),
        }
    }

    fn repr(&self) -> String {
        match self {
            Scalar::Bool(b) => if *b { "True".into() } else { "False".into() },
            Scalar::Int(i) => i.to_string(),
            Scalar::Float(f) => pystr::repr_float(*f),
            Scalar::Str(s) => pystr::repr_str(s),
        }
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            Scalar::Bool(b) => Some(f64::from(u8::from(*b))),
            Scalar::Int(i) => Some(*i as f64),
            Scalar::Float(f) => Some(*f),
            Scalar::Str(_) => None,
        }
    }

    /// Python `==` between a value read back from TOML and this one.
    fn py_eq(&self, got: Option<&Value>) -> bool {
        let Some(got) = got else { return false };
        match (self, got) {
            (Scalar::Str(a), Value::String(b)) => a == b,
            (Scalar::Str(_), _) => false,
            (_, Value::Boolean(_) | Value::Integer(_) | Value::Float(_)) => {
                let g = match got {
                    Value::Boolean(b) => f64::from(u8::from(*b)),
                    Value::Integer(i) => *i as f64,
                    Value::Float(f) => *f,
                    _ => return false,
                };
                self.as_number() == Some(g)
            }
            _ => false,
        }
    }
}

/// One edit: `key` in [table], or in the [[table]] entry whose id is `item_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub table: String,
    pub key: String,
    pub item_id: Option<String>,
    pub value: Scalar,
}

impl Edit {
    pub fn table(table: &str, key: &str, value: Scalar) -> Edit {
        Edit { table: table.into(), key: key.into(), item_id: None, value }
    }

    pub fn item(table: &str, item_id: &str, key: &str, value: Scalar) -> Edit {
        Edit { table: table.into(), key: key.into(), item_id: Some(item_id.into()), value }
    }
}

fn header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r#"^\s*(\[\[?)\s*([A-Za-z0-9_."' -]+?)\s*(\]\]?)\s*(#.*)?$"#).expect("valid regex")
    })
}

/// (line index, table name, is array-of-tables) for every table header.
pub(crate) fn headers(lines: &[String]) -> Vec<(usize, String, bool)> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(m) = header_re().captures(line) {
            let open = &m[1] == "[[";
            let close = &m[3] == "]]";
            if open == close {
                out.push((i, pystr::strip(&m[2]).to_string(), open));
            }
        }
    }
    out
}

/// End (exclusive) of the table whose header is at `start`.
pub(crate) fn span(lines: &[String], start: usize) -> usize {
    headers(lines).into_iter().map(|(i, _, _)| i).find(|&i| i > start).unwrap_or(lines.len())
}

pub(crate) fn key_line(key: &str) -> Regex {
    #[allow(clippy::expect_used)]
    Regex::new(&format!(r"^(\s*){}(\s*=\s*)(.*)$", regex::escape(key))).expect("escaped key")
}

pub(crate) fn is_complete(value: &str) -> bool {
    format!("x = {value}").parse::<Table>().is_ok()
}

/// Split 'value  # comment' into (value, comment), respecting '#' inside strings.
pub(crate) fn split_value(rest: &str) -> Option<(String, String)> {
    let mut cuts: Vec<usize> = rest.match_indices('#').map(|(i, _)| i).collect();
    cuts.push(rest.len());
    for cut in cuts {
        let candidate = pystr::rstrip(&rest[..cut]);
        if !candidate.is_empty() && is_complete(candidate) {
            return Some((candidate.to_string(), rest[candidate.len()..].to_string()));
        }
    }
    None
}

fn set_in_span(
    lines: &mut Vec<String>,
    header: usize,
    end: usize,
    key: &str,
    value: &Scalar,
    insert_after: Option<usize>,
) -> Result<(), EditError> {
    let pattern = key_line(key);
    for i in header + 1..end {
        let caps = pattern.captures(&lines[i]).map(|m| (m[1].to_string(), m[2].to_string(), m[3].to_string()));
        if let Some((indent, sep, rest)) = caps {
            let Some((_, comment)) = split_value(&rest) else {
                return Err(EditError(format!("{key} is a multi-line value; edit it in the file")));
            };
            lines[i] = format!("{indent}{key}{sep}{}{comment}", value.format());
            return Ok(());
        }
    }
    lines.insert(insert_after.unwrap_or(header) + 1, format!("{key} = {}", value.format()));
    Ok(())
}

pub(crate) fn to_lines(text: &str) -> Vec<String> {
    pystr::splitlines(text).into_iter().map(str::to_string).collect()
}

/// Set `key` in [table] (e.g. table = "search").
pub fn set_table_value(text: &str, table: &str, key: &str, value: &Scalar) -> Result<String, EditError> {
    let mut lines = to_lines(text);
    for (i, name, is_array) in headers(&lines) {
        if name == table && !is_array {
            let end = span(&lines, i);
            set_in_span(&mut lines, i, end, key, value, None)?;
            return Ok(lines.join("\n") + "\n");
        }
    }
    lines.push(String::new());
    lines.push(format!("[{table}]  # added by the settings window"));
    lines.push(format!("{key} = {}", value.format()));
    Ok(lines.join("\n") + "\n")
}

/// Set `key` in the [[array]] entry whose id = item_id (e.g. the [[app]] with id "claude-cli").
pub fn set_array_item_value(text: &str, array: &str, item_id: &str, key: &str, value: &Scalar) -> Result<String, EditError> {
    let mut lines = to_lines(text);
    let id_line = key_line("id");
    for (i, name, is_array) in headers(&lines) {
        if name != array || !is_array {
            continue;
        }
        let end = span(&lines, i);
        for j in i + 1..end {
            let rest = id_line.captures(&lines[j]).map(|m| m[3].to_string());
            let Some((v, _)) = rest.as_deref().and_then(split_value) else { continue };
            let matches = format!("x = {v}")
                .parse::<Table>()
                .ok()
                .and_then(|t| t.get("x").and_then(|x| x.as_str().map(|s| s == item_id)))
                .unwrap_or(false);
            if matches {
                set_in_span(&mut lines, i, end, key, value, Some(j))?;
                return Ok(lines.join("\n") + "\n");
            }
        }
    }
    Err(EditError(format!("no [[{array}]] with id {}", pystr::repr_str(item_id))))
}

/// Python repr of a value read back from TOML (for error messages).
fn repr_value(v: Option<&Value>) -> String {
    match v {
        None => "None".into(),
        Some(Value::String(s)) => pystr::repr_str(s),
        Some(Value::Integer(i)) => i.to_string(),
        Some(Value::Float(f)) => pystr::repr_float(*f),
        Some(Value::Boolean(b)) => if *b { "True".into() } else { "False".into() },
        Some(Value::Array(a)) => format!("[{}]", a.iter().map(|x| repr_value(Some(x))).collect::<Vec<_>>().join(", ")),
        Some(Value::Table(t)) => format!(
            "{{{}}}",
            t.iter().map(|(k, x)| format!("{}: {}", pystr::repr_str(k), repr_value(Some(x)))).collect::<Vec<_>>().join(", ")
        ),
        Some(other) => other.to_string(),
    }
}

/// Apply edits and verify them.
pub fn apply(text: &str, edits: &[Edit]) -> Result<String, EditError> {
    let mut text = text.to_string();
    for e in edits {
        text = match &e.item_id {
            None => set_table_value(&text, &e.table, &e.key, &e.value)?,
            Some(id) => set_array_item_value(&text, &e.table, id, &e.key, &e.value)?,
        };
    }
    let data: Table = text.parse().map_err(|err| EditError(format!("the edit would break the file: {err}")))?;
    for e in edits {
        let got = match &e.item_id {
            None => data.get(&e.table).and_then(Value::as_table).and_then(|t| t.get(&e.key)),
            Some(id) => data.get(&e.table).and_then(Value::as_array).and_then(|items| {
                items
                    .iter()
                    .filter_map(Value::as_table)
                    .find(|x| x.get("id").and_then(Value::as_str) == Some(id.as_str()))
                    .and_then(|x| x.get(&e.key))
            }),
        };
        if !e.value.py_eq(got) {
            return Err(EditError(format!(
                "{}.{} reads back as {}, not {}",
                e.table,
                e.key,
                repr_value(got),
                e.value.repr()
            )));
        }
    }
    Ok(text)
}
