//! Build the URL or command for a chosen row. Running it is the platform layer's job.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::config::{App, CommandItem, Config, home_dir};
use crate::ranker::parse_result_id;

/// Program lookup, injectable so tests don't depend on what's installed.
pub type Which = dyn Fn(&str) -> Option<String>;

/// D-Bus starts processes with the session's PATH, which often lacks per-user bin folders.
pub fn which(program: &str) -> Option<String> {
    if program.contains('/') {
        return is_executable(Path::new(program)).then(|| program.to_string());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.push(home_dir().join(".local/bin"));
    dirs.push(home_dir().join("bin"));
    dirs.into_iter()
        .map(|d| d.join(program))
        .find(|p| is_executable(p))
        .map(|p| p.to_string_lossy().into_owned())
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

pub fn app_available(app: &App, which: &Which) -> bool {
    app.requires.iter().all(|p| which(p).is_some())
}

/// Python `urllib.parse.quote(s, safe='')`.
pub fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Python `unquote_plus` as used by `parse_qsl` (UTF-8, invalid bytes replaced).
fn unquote_plus(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug, Clone, PartialEq)]
pub struct FillError(pub String);

/// Python `str.format(**values)` for `{name}` placeholders; `{{`/`}}` are literal braces.
fn format_template(template: &str, values: &[(&str, String)]) -> Result<String, FillError> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => name.push(ch),
                        None => return Err(FillError("ValueError".into())),
                    }
                }
                match values.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => return Err(FillError("KeyError".into())),
                }
            }
            '}' => return Err(FillError("ValueError".into())),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Fill {placeholders} (URL-encoded), then drop query parameters that ended up empty.
pub fn fill(template: &str, prompt: Option<&str>, model: Option<&str>, effort: Option<&str>) -> Result<String, FillError> {
    let enc = |v: Option<&str>| quote(v.unwrap_or(""));
    let values = [("prompt", enc(prompt)), ("model", enc(model)), ("effort", enc(effort))];
    let url = format_template(template, &values)?;
    // urlsplit: fragment first, then query.
    let (before_frag, fragment) = match url.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (url.as_str(), None),
    };
    let Some((base, query)) = before_frag.split_once('?') else { return Ok(url) };
    if query.is_empty() {
        return Ok(url);
    }
    let pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (unquote_plus(k), unquote_plus(v)),
            None => (unquote_plus(p), String::new()),
        })
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let encoded: Vec<String> = pairs.iter().map(|(k, v)| format!("{}={}", quote(k), quote(v))).collect();
    // urlunsplit lowercases the scheme and omits an empty query.
    let base = match base.split_once(':') {
        Some((scheme, rest)) if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) => {
            format!("{}:{rest}", scheme.to_ascii_lowercase())
        }
        _ => base.to_string(),
    };
    let mut out = base;
    if !encoded.is_empty() {
        out.push('?');
        out.push_str(&encoded.join("&"));
    }
    if let Some(f) = fragment.filter(|f| !f.is_empty()) {
        out.push('#');
        out.push_str(f);
    }
    Ok(out)
}

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    #[allow(clippy::expect_used)]
    RE.get_or_init(|| Regex::new(r"\{(\w+)\}").expect("valid regex"))
}

/// Fill an argv template. Items are never passed through a shell.
/// An item whose placeholder is empty is dropped; a group is dropped as a whole.
pub fn fill_command(template: &[CommandItem], prompt: Option<&str>, model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let lookup = |name: &str| -> Option<&str> {
        match name {
            "prompt" => prompt,
            "model" => model,
            "effort" => effort,
            _ => None,
        }
        .filter(|v| !v.is_empty())
    };
    let render = |item: &str| -> Option<String> {
        let re = placeholder_re();
        if re.captures_iter(item).any(|m| lookup(&m[1]).is_none()) {
            return None;
        }
        Some(re.replace_all(item, |m: &regex::Captures| lookup(&m[1]).unwrap_or("").to_string()).into_owned())
    };
    let mut argv = Vec::new();
    for item in template {
        let group: Vec<&str> = match item {
            CommandItem::Arg(a) => vec![a.as_str()],
            CommandItem::Group(g) => g.iter().map(String::as_str).collect(),
        };
        let rendered: Vec<Option<String>> = group.iter().map(|p| render(p)).collect();
        if rendered.iter().all(Option::is_some) {
            argv.extend(rendered.into_iter().flatten());
        }
    }
    argv
}

/// `str(Path(p).expanduser())`.
pub fn expand_path(p: &str) -> String {
    let expanded = if p == "~" {
        home_dir()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(p)
    };
    let mut out = PathBuf::new();
    for comp in expanded.components() {
        match comp {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    let s = out.to_string_lossy().into_owned();
    if s.is_empty() { ".".into() } else { s }
}

/// (argv, cwd) for a command app, with `requires` programs resolved to absolute paths.
pub fn command_for(app: &App, model: Option<&str>, effort: Option<&str>, prompt: &str, which: &Which) -> (Vec<String>, String) {
    let argv = fill_command(&app.command, Some(prompt), model, effort)
        .into_iter()
        .map(|a| if app.requires.contains(&a) { which(&a).unwrap_or(a) } else { a })
        .collect();
    (argv, expand_path(&app.cwd))
}

pub fn gateway_model_id(base: &str, effort: Option<&str>) -> String {
    match effort.filter(|e| !e.is_empty()) {
        Some(e) => format!("{base}@{e}"),
        None => base.to_string(),
    }
}

pub fn url_for(config: &Config, cid: &str, model: Option<&str>, effort: Option<&str>, prompt: &str) -> Result<Option<String>, FillError> {
    let (kind, name) = cid.split_once(':').unwrap_or((cid, ""));
    match kind {
        "app" => match config.apps.iter().find(|a| a.id == name) {
            Some(app) if !app.url.is_empty() => fill(&app.url, Some(prompt), model, effort).map(Some),
            _ => Ok(None),
        },
        _ if config.chat_url.is_empty() => Ok(None),
        "auto" => fill(&config.chat_url, Some(prompt), Some(&config.auto_model), None).map(Some),
        "model" => fill(&config.chat_url, Some(prompt), Some(&gateway_model_id(name, effort)), None).map(Some),
        _ => Ok(None),
    }
}

/// What activating a row should do. The caller copies first (if asked), then launches.
#[derive(Debug, Clone, PartialEq)]
pub struct Activation {
    pub copy: Option<String>,
    pub launch: Launch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    Spawn { argv: Vec<String>, cwd: String },
    OpenUri(String),
    Nothing,
}

pub fn activation(config: &Config, result_id: &str, prompt: &str, which: &Which) -> Result<Activation, String> {
    let (cid, model, effort) = parse_result_id(result_id);
    let app = config.apps.iter().find(|a| format!("app:{}", a.id) == cid);
    let copy = app.filter(|a| a.copy_prompt).map(|_| prompt.to_string());
    if let Some(app) = app.filter(|a| !a.command.is_empty()) {
        let (argv, cwd) = command_for(app, model.as_deref(), effort.as_deref(), prompt, which);
        if argv.is_empty() {
            return Err("IndexError: list index out of range".into());
        }
        return Ok(Activation { copy, launch: Launch::Spawn { argv, cwd } });
    }
    let url = url_for(config, &cid, model.as_deref(), effort.as_deref(), prompt).map_err(|e| e.0)?;
    Ok(Activation { copy, launch: url.map_or(Launch::Nothing, Launch::OpenUri) })
}
