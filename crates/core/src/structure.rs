//! Structural edits to agents.toml for the full settings editor: lists, removing keys, adding,
//! deleting and renaming agents, ranking rules and aliases.
//!
//! Like `edit`, these work line by line so every comment outside the edited lines survives. When
//! a list is replaced, comments *inside* that list are lost; nothing else changes. After applying,
//! the whole config is validated (it must load, agent ids must be unique, every regex must
//! compile) and each operation is read back; if anything fails, nothing is written.

use toml::{Table, Value};

use crate::edit::{self, EditError, Scalar, headers, is_complete, key_line, span, split_value, to_lines};
use crate::pyre;
use crate::pystr;

/// Which table an operation targets.
#[derive(Debug, Clone, PartialEq)]
pub enum Sel {
    /// A plain [table].
    Table,
    /// The [[table]] entry whose `id` is this.
    Id(String),
    /// The n-th [[table]] entry (0-based), for entries without ids such as [[routing.rule]].
    Index(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ListItem {
    Str(String),
    Group(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Set { table: String, sel: Sel, key: String, value: Scalar },
    SetList { table: String, sel: Sel, key: String, items: Vec<ListItem> },
    Remove { table: String, sel: Sel, key: String },
    AddApp { id: String, name: String, url: Option<String>, command: Option<Vec<ListItem>> },
    DeleteApp { id: String },
    RenameApp { from: String, to: String },
    AddRule { tag: String, weight: f64, patterns: Vec<String> },
    DeleteRule { index: usize },
}

fn err(msg: impl Into<String>) -> EditError {
    EditError(msg.into())
}

pub fn format_list(items: &[ListItem]) -> String {
    let render = |i: &ListItem| match i {
        ListItem::Str(s) => pystr::json_dumps_str(s),
        ListItem::Group(g) => format!("[{}]", g.iter().map(|s| pystr::json_dumps_str(s)).collect::<Vec<_>>().join(", ")),
    };
    let parts: Vec<String> = items.iter().map(render).collect();
    let one_line = format!("[{}]", parts.join(", "));
    if one_line.chars().count() <= 96 || parts.len() < 2 {
        one_line
    } else {
        format!("[\n{}\n]", parts.iter().map(|p| format!("  {p},")).collect::<Vec<_>>().join("\n"))
    }
}

/// (header, end-exclusive, anchor) of the targeted table. The anchor is where new keys go after.
fn locate(lines: &[String], table: &str, sel: &Sel) -> Option<(usize, usize, usize)> {
    let hs = headers(lines);
    match sel {
        Sel::Table => hs.iter().find(|(_, n, arr)| n == table && !arr).map(|(i, _, _)| (*i, span(lines, *i), *i)),
        Sel::Index(n) => {
            hs.iter().filter(|(_, name, arr)| name == table && *arr).nth(*n).map(|(i, _, _)| (*i, span(lines, *i), *i))
        }
        Sel::Id(id) => {
            let id_re = key_line("id");
            for (i, name, arr) in &hs {
                if name != table || !arr {
                    continue;
                }
                let end = span(lines, *i);
                for j in i + 1..end {
                    let Some(rest) = id_re.captures(&lines[j]).map(|m| m[3].to_string()) else { continue };
                    let Some((v, _)) = split_value(&rest) else { continue };
                    let is_it = format!("x = {v}").parse::<Table>().ok().and_then(|t| t.get("x").and_then(|x| x.as_str().map(|s| s == id)));
                    if is_it == Some(true) {
                        return Some((*i, end, j));
                    }
                }
            }
            None
        }
    }
}

/// The lines a key's value occupies, from its key line `i`: (last line, trailing comment).
fn value_extent(lines: &[String], i: usize, end: usize, rest: &str) -> Option<(usize, String)> {
    if let Some((_, comment)) = split_value(rest) {
        return Some((i, comment));
    }
    let mut joined = rest.to_string();
    for j in i + 1..end {
        let tail = &lines[j];
        let mut cuts: Vec<usize> = tail.match_indices('#').map(|(k, _)| k).collect();
        cuts.push(tail.len());
        for cut in cuts {
            let candidate = format!("{joined}\n{}", pystr::rstrip(&tail[..cut]));
            if is_complete(&candidate) {
                let kept = pystr::rstrip(&tail[..cut]).len();
                return Some((j, tail[kept..].to_string()));
            }
        }
        joined = format!("{joined}\n{tail}");
    }
    None
}

/// Find `key` in the located table: (key line, last value line, indent, separator, comment).
fn find_key(lines: &[String], header: usize, end: usize, key: &str) -> Option<(usize, usize, String, String, String)> {
    let re = key_line(key);
    for i in header + 1..end {
        if let Some(m) = re.captures(&lines[i]) {
            let (indent, sep, rest) = (m[1].to_string(), m[2].to_string(), m[3].to_string());
            let (last, comment) = value_extent(lines, i, end, &rest)?;
            return Some((i, last, indent, sep, comment));
        }
    }
    None
}

fn set_rendered(lines: &mut Vec<String>, table: &str, sel: &Sel, key: &str, rendered: &str) -> Result<(), EditError> {
    match locate(lines, table, sel) {
        Some((header, end, anchor)) => match find_key(lines, header, end, key) {
            Some((i, last, indent, sep, comment)) => {
                lines.splice(i..=last, [format!("{indent}{key}{sep}{rendered}{comment}")]);
                Ok(())
            }
            None => {
                lines.insert(anchor + 1, format!("{key} = {rendered}"));
                Ok(())
            }
        },
        None if *sel == Sel::Table => {
            lines.push(String::new());
            lines.push(format!("[{table}]  # added by the settings window"));
            lines.push(format!("{key} = {rendered}"));
            Ok(())
        }
        None => Err(err(format!("no [[{table}]] entry {sel:?}"))),
    }
}

fn remove_key(lines: &mut Vec<String>, table: &str, sel: &Sel, key: &str) -> Result<(), EditError> {
    let Some((header, end, _)) = locate(lines, table, sel) else { return Ok(()) };
    if let Some((i, last, ..)) = find_key(lines, header, end, key) {
        lines.drain(i..=last);
    }
    Ok(())
}

fn is_content(line: &str) -> bool {
    let s = pystr::strip(line);
    !s.is_empty() && !s.starts_with('#')
}

/// Index of the last content line of the last [[table]] entry, if any.
fn last_entry_content(lines: &[String], table: &str) -> Option<usize> {
    let (header, ..) = headers(lines).into_iter().filter(|(_, n, arr)| n == table && *arr).last()?;
    let end = span(lines, header);
    (header..end).rev().find(|&i| is_content(&lines[i]))
}

fn insert_block(lines: &mut Vec<String>, table: &str, body: Vec<String>) {
    let mut block = vec![String::new(), format!("[[{table}]]")];
    block.extend(body);
    match last_entry_content(lines, table) {
        Some(after) => {
            lines.splice(after + 1..after + 1, block);
        }
        None => lines.extend(block),
    }
}

fn delete_block(lines: &mut Vec<String>, table: &str, sel: &Sel) -> Result<(), EditError> {
    let (header, end, _) = locate(lines, table, sel).ok_or_else(|| err(format!("no [[{table}]] entry {sel:?}")))?;
    let last = (header..end).rev().find(|&i| is_content(&lines[i])).unwrap_or(header);
    // Take the entry's own comment block (directly above, no blank line in between) with it.
    let mut start = header;
    while start > 0 && pystr::strip(&lines[start - 1]).starts_with('#') {
        start -= 1;
    }
    if start > 0 && pystr::strip(&lines[start - 1]).is_empty() {
        start -= 1;
    }
    lines.drain(start..=last);
    Ok(())
}

fn str_value(v: &str) -> String {
    pystr::json_dumps_str(v)
}

fn app_body(id: &str, name: &str, url: &Option<String>, command: &Option<Vec<ListItem>>) -> Vec<String> {
    let mut body = vec![format!("id = {}", str_value(id)), format!("name = {}", str_value(name))];
    if let Some(u) = url.as_ref().filter(|u| !u.is_empty()) {
        body.push(format!("url = {}", str_value(u)));
    }
    if let Some(c) = command.as_ref().filter(|c| !c.is_empty()) {
        body.push(format!("command = {}", format_list(c)));
    }
    body
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn app_ids(data: &Table) -> Vec<String> {
    data.get("app")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|t| t.get("id").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default()
}

fn rule_count(data: &Table) -> usize {
    data.get("routing").and_then(|r| r.get("rule")).and_then(Value::as_array).map_or(0, Vec::len)
}

fn lookup<'a>(data: &'a Table, table: &str, sel: &Sel, key: &str) -> Option<&'a Value> {
    let entries = || -> Option<&'a Vec<Value>> {
        match table.split_once('.') {
            Some((a, b)) => data.get(a)?.get(b)?.as_array(),
            None => data.get(table)?.as_array(),
        }
    };
    match sel {
        Sel::Table => data.get(table)?.as_table()?.get(key),
        Sel::Id(id) => entries()?.iter().find(|t| t.get("id").and_then(Value::as_str) == Some(id.as_str()))?.get(key),
        Sel::Index(n) => entries()?.get(*n)?.get(key),
    }
}

fn list_matches(v: Option<&Value>, items: &[ListItem]) -> bool {
    let Some(Value::Array(a)) = v else { return items.is_empty() && v.is_none() };
    a.len() == items.len()
        && a.iter().zip(items).all(|(x, i)| match (x, i) {
            (Value::String(s), ListItem::Str(t)) => s == t,
            (Value::Array(g), ListItem::Group(h)) => g.len() == h.len() && g.iter().zip(h).all(|(p, q)| p.as_str() == Some(q)),
            _ => false,
        })
}

/// Everything that must hold for the file to be saved.
pub fn validate(text: &str) -> Result<Table, EditError> {
    let data: Table = text.parse().map_err(|e| err(format!("the edit would break the file: {e}")))?;
    let config = crate::config::from_table(&data).map_err(|e| err(e.0))?;
    let mut seen = std::collections::HashSet::new();
    for app in &config.apps {
        if !valid_id(&app.id) {
            return Err(err(format!("agent id {} may only use letters, digits, - and _", pystr::repr_str(&app.id))));
        }
        if !seen.insert(app.id.clone()) {
            return Err(err(format!("two agents use the id {}", pystr::repr_str(&app.id))));
        }
    }
    for p in config.high_effort_patterns.iter().chain(config.rules.iter().flat_map(|r| r.patterns.iter())) {
        pyre::compile(p).map_err(|e| err(format!("pattern {}: {e}", pystr::repr_str(p))))?;
    }
    Ok(data)
}

/// Apply operations to the text of agents.toml, then validate and read every one back.
pub fn apply(text: &str, ops: &[Op]) -> Result<String, EditError> {
    let before: Table = text.parse().unwrap_or_default();
    let mut text = text.to_string();
    for op in ops {
        text = match op {
            Op::Set { table, sel: Sel::Table, key, value } => edit::set_table_value(&text, table, key, value)?,
            Op::Set { table, sel: Sel::Id(id), key, value } => edit::set_array_item_value(&text, table, id, key, value)?,
            Op::Set { table, sel, key, value } => {
                let mut lines = to_lines(&text);
                set_rendered(&mut lines, table, sel, key, &value.format())?;
                lines.join("\n") + "\n"
            }
            Op::SetList { table, sel, key, items } => {
                let mut lines = to_lines(&text);
                set_rendered(&mut lines, table, sel, key, &format_list(items))?;
                lines.join("\n") + "\n"
            }
            Op::Remove { table, sel, key } => {
                let mut lines = to_lines(&text);
                remove_key(&mut lines, table, sel, key)?;
                lines.join("\n") + "\n"
            }
            Op::AddApp { id, name, url, command } => {
                if !valid_id(id) {
                    return Err(err("an agent id may only use letters, digits, - and _"));
                }
                let mut lines = to_lines(&text);
                insert_block(&mut lines, "app", app_body(id, name, url, command));
                lines.join("\n") + "\n"
            }
            Op::DeleteApp { id } => {
                let mut lines = to_lines(&text);
                delete_block(&mut lines, "app", &Sel::Id(id.clone()))?;
                lines.join("\n") + "\n"
            }
            Op::RenameApp { from, to } => {
                if !valid_id(to) {
                    return Err(err("an agent id may only use letters, digits, - and _"));
                }
                let mut lines = to_lines(&text);
                let (_, _, anchor) = locate(&lines, "app", &Sel::Id(from.clone())).ok_or_else(|| err(format!("no agent {}", pystr::repr_str(from))))?;
                let re = key_line("id");
                let Some(m) = re.captures(&lines[anchor]).map(|m| (m[1].to_string(), m[2].to_string(), m[3].to_string())) else {
                    return Err(err("id line not found"));
                };
                let comment = split_value(&m.2).map(|(_, c)| c).unwrap_or_default();
                lines[anchor] = format!("{}id{}{}{}", m.0, m.1, str_value(to), comment);
                let mut out = lines.join("\n") + "\n";
                // Point aliases that targeted the old id at the new one.
                let data: Table = out.parse().unwrap_or_default();
                if let Some(Value::Table(aliases)) = data.get("aliases") {
                    for (name, target) in aliases {
                        let Some(target) = target.as_str() else { continue };
                        let renamed = if target == from {
                            Some(to.clone())
                        } else {
                            target.strip_prefix(&format!("{from}:")).map(|m| format!("{to}:{m}"))
                        };
                        if let Some(new_target) = renamed {
                            if valid_id(name) {
                                out = edit::set_table_value(&out, "aliases", name, &Scalar::Str(new_target))?;
                            }
                        }
                    }
                }
                out
            }
            Op::AddRule { tag, weight, patterns } => {
                let mut lines = to_lines(&text);
                let pats: Vec<ListItem> = patterns.iter().cloned().map(ListItem::Str).collect();
                let body = vec![
                    format!("tag = {}", str_value(tag)),
                    format!("weight = {}", Scalar::Float(*weight).format()),
                    format!("patterns = {}", format_list(&pats)),
                ];
                insert_block(&mut lines, "routing.rule", body);
                lines.join("\n") + "\n"
            }
            Op::DeleteRule { index } => {
                let mut lines = to_lines(&text);
                delete_block(&mut lines, "routing.rule", &Sel::Index(*index))?;
                lines.join("\n") + "\n"
            }
        };
    }

    let data = validate(&text)?;
    for op in ops {
        let ok = match op {
            Op::Set { table, sel, key, value } => {
                let got = lookup(&data, table, sel, key);
                Scalar::from_toml(got.unwrap_or(&Value::Boolean(false))).is_some_and(|g| match (&g, value) {
                    (Scalar::Str(a), Scalar::Str(b)) => a == b,
                    (a, b) => num(a) == num(b),
                }) && got.is_some()
            }
            Op::SetList { table, sel, key, items } => list_matches(lookup(&data, table, sel, key), items),
            Op::Remove { table, sel, key } => lookup(&data, table, sel, key).is_none(),
            Op::AddApp { id, .. } => app_ids(&data).contains(id),
            Op::DeleteApp { id } => !app_ids(&data).contains(id),
            Op::RenameApp { from, to } => app_ids(&data).contains(to) && (from == to || !app_ids(&data).contains(from)),
            Op::AddRule { .. } | Op::DeleteRule { .. } => true,
        };
        if !ok {
            return Err(err(format!("the change didn't read back as expected: {op:?}")));
        }
    }
    let delta = rule_count(&data) as i64 - rule_count(&before) as i64;
    let expected: i64 = ops.iter().map(|o| match o {
        Op::AddRule { .. } => 1,
        Op::DeleteRule { .. } => -1,
        _ => 0,
    }).sum();
    if delta != expected {
        return Err(err("the rule count didn't change as expected"));
    }
    Ok(text)
}

fn num(s: &Scalar) -> Option<f64> {
    match s {
        Scalar::Bool(b) => Some(f64::from(u8::from(*b))),
        Scalar::Int(i) => Some(*i as f64),
        Scalar::Float(f) => Some(*f),
        Scalar::Str(_) => None,
    }
}
