//! Load and save user configuration in $LLM_SEARCH_HOME (default ~/.llm-search).
//!
//! agents.toml is the single source of settings. You edit it by hand, and the settings window edits
//! the same file in place (see `edit`, which keeps its comments). API keys live in secrets.env (mode
//! 600) and the keyboard shortcut in GNOME's settings.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::edit::{self, Edit, EditError, Scalar};
use crate::pystr;

pub const AGENTS_FILE: &str = "agents.toml";
pub const SECRETS_FILE: &str = "secrets.env";
pub const LEGACY_SETTINGS_FILE: &str = "settings.toml";

pub fn home() -> PathBuf {
    match std::env::var("LLM_SEARCH_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => home_dir().join(".llm-search"),
    }
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_dir() -> PathBuf {
    home().join("config")
}

pub fn state_dir() -> PathBuf {
    home().join("state")
}

/// An app agent, opened through a URL (e.g. claude://) or by running a command.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub url: String,
    /// argv template; nested arrays are optional groups.
    pub command: Vec<CommandItem>,
    pub cwd: String,
    pub requires: Vec<String>,
    pub description: String,
    pub icon: String,
    pub tags: Vec<String>,
    pub models: Vec<String>,
    pub efforts: Vec<String>,
    pub priority: f64,
    pub copy_prompt: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandItem {
    Arg(String),
    Group(Vec<String>),
}

impl App {
    pub fn new(id: &str, name: &str) -> Self {
        App {
            id: id.into(),
            name: name.into(),
            url: String::new(),
            command: Vec::new(),
            cwd: "~".into(),
            requires: Vec::new(),
            description: String::new(),
            icon: "system-search-symbolic".into(),
            tags: Vec::new(),
            models: Vec::new(),
            efforts: Vec::new(),
            priority: 0.0,
            copy_prompt: false,
            enabled: true,
        }
    }
}

/// Adds `weight` to candidates with `tag` when any pattern matches the prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub tag: String,
    pub patterns: Vec<String>,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub min_chars: i64,
    pub max_rows: i64,
    pub apps: Vec<App>,
    /// Lowercased alias → target, in file order.
    pub aliases: Vec<(String, String)>,
    pub rules: Vec<Rule>,
    pub high_effort_patterns: Vec<String>,
    pub low_effort_max_words: i64,
    /// Empty means the gateway is disabled.
    pub litellm_url: String,
    pub litellm_key_env: String,
    pub chat_url: String,
    pub auto_model: String,
    pub prompt_idle_minutes: f64,
    pub secrets: BTreeMap<String, String>,
    pub directory: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            min_chars: 3,
            max_rows: 5,
            apps: Vec::new(),
            aliases: Vec::new(),
            rules: Vec::new(),
            high_effort_patterns: Vec::new(),
            low_effort_max_words: 8,
            litellm_url: String::new(),
            litellm_key_env: "LITELLM_MASTER_KEY".into(),
            chat_url: String::new(),
            auto_model: "smart-router".into(),
            prompt_idle_minutes: 30.0,
            secrets: BTreeMap::new(),
            directory: None,
        }
    }
}

impl Config {
    pub fn alias(&self, name: &str) -> Option<&str> {
        // Python dict semantics: a later duplicate key wins.
        self.aliases.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parse a simple KEY=value file (comments and blank lines ignored).
pub fn read_env_file(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path).map(|t| parse_env(&t)).unwrap_or_default()
}

pub fn parse_env(text: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in pystr::splitlines(text) {
        let line = pystr::strip(line);
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let value = pystr::strip(value).trim_matches(|c| c == '\'' || c == '"');
        values.insert(pystr::strip(key).to_string(), value.to_string());
    }
    values
}

fn read_toml(path: &Path) -> Result<Table, LoadError> {
    match fs::read_to_string(path) {
        Ok(text) => text.parse::<Table>().map_err(|e| LoadError(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Table::new()),
        Err(e) => Err(LoadError(e.to_string())),
    }
}

fn num(v: Option<&Value>, default: f64) -> f64 {
    match v {
        Some(Value::Integer(i)) => *i as f64,
        Some(Value::Float(f)) => *f,
        Some(Value::Boolean(b)) => f64::from(u8::from(*b)),
        _ => default,
    }
}

fn int(v: Option<&Value>, default: i64) -> i64 {
    match v {
        Some(Value::Integer(i)) => *i,
        Some(Value::Float(f)) => *f as i64,
        Some(Value::Boolean(b)) => i64::from(*b),
        _ => default,
    }
}

fn string(v: Option<&Value>, default: &str) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        _ => default.to_string(),
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// Python truthiness for a TOML value used as a flag.
fn truthy(v: Option<&Value>, default: bool) -> bool {
    match v {
        None => default,
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(i)) => *i != 0,
        Some(Value::Float(f)) => *f != 0.0,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Table(t)) => !t.is_empty(),
        Some(Value::Datetime(_)) => true,
    }
}

const APP_KEYS: [&str; 14] = [
    "id", "name", "url", "command", "cwd", "requires", "description", "icon", "tags", "models", "efforts",
    "priority", "copy_prompt", "enabled",
];

fn app_from(t: &Table) -> Result<App, LoadError> {
    if let Some(k) = t.keys().find(|k| !APP_KEYS.contains(&k.as_str())) {
        return Err(LoadError(format!("App.__init__() got an unexpected keyword argument '{k}'")));
    }
    for required in ["id", "name"] {
        if !t.contains_key(required) {
            return Err(LoadError(format!("App.__init__() missing 1 required positional argument: '{required}'")));
        }
    }
    let command = match t.get("command") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Array(group) => CommandItem::Group(group.iter().map(value_to_arg).collect()),
                other => CommandItem::Arg(value_to_arg(other)),
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut app = App::new(&string(t.get("id"), ""), &string(t.get("name"), ""));
    app.url = string(t.get("url"), "");
    app.command = command;
    app.cwd = string(t.get("cwd"), "~");
    app.requires = strings(t.get("requires"));
    app.description = string(t.get("description"), "");
    app.icon = string(t.get("icon"), "system-search-symbolic");
    app.tags = strings(t.get("tags"));
    app.models = strings(t.get("models"));
    app.efforts = strings(t.get("efforts"));
    app.priority = num(t.get("priority"), 0.0);
    app.copy_prompt = truthy(t.get("copy_prompt"), false);
    app.enabled = truthy(t.get("enabled"), true);
    Ok(app)
}

/// Python's `str(item)` for argv items that aren't strings.
fn value_to_arg(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => pystr::repr_float(*f),
        Value::Boolean(b) => if *b { "True".into() } else { "False".into() },
        other => other.to_string(),
    }
}

fn rule_from(t: &Table) -> Result<Rule, LoadError> {
    if let Some(k) = t.keys().find(|k| !["tag", "patterns", "weight"].contains(&k.as_str())) {
        return Err(LoadError(format!("Rule.__init__() got an unexpected keyword argument '{k}'")));
    }
    for required in ["tag", "patterns"] {
        if !t.contains_key(required) {
            return Err(LoadError(format!("Rule.__init__() missing 1 required positional argument: '{required}'")));
        }
    }
    Ok(Rule { tag: string(t.get("tag"), ""), patterns: strings(t.get("patterns")), weight: num(t.get("weight"), 1.0) })
}

pub fn from_table(data: &Table) -> Result<Config, LoadError> {
    let empty = Table::new();
    let section = |name: &str| data.get(name).and_then(Value::as_table).unwrap_or(&empty);
    let (search, routing, gateway, prompt) = (section("search"), section("routing"), section("gateway"), section("prompt"));
    let mut apps = Vec::new();
    if let Some(Value::Array(items)) = data.get("app") {
        for item in items {
            if let Value::Table(t) = item {
                apps.push(app_from(t)?);
            }
        }
    }
    let mut rules = Vec::new();
    if let Some(Value::Array(items)) = routing.get("rule") {
        for item in items {
            if let Value::Table(t) = item {
                rules.push(rule_from(t)?);
            }
        }
    }
    let aliases = section("aliases")
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|v| (k.to_lowercase(), v.to_string())))
        .collect();
    Ok(Config {
        min_chars: int(search.get("min_chars"), 3),
        max_rows: int(search.get("max_rows"), 5),
        apps,
        aliases,
        rules,
        high_effort_patterns: strings(routing.get("high_effort_patterns")),
        low_effort_max_words: int(routing.get("low_effort_max_words"), 8),
        litellm_url: string(gateway.get("url"), "").trim_end_matches('/').to_string(),
        litellm_key_env: string(gateway.get("key_env"), "LITELLM_MASTER_KEY"),
        chat_url: string(gateway.get("chat_url"), ""),
        auto_model: string(gateway.get("auto_model"), "smart-router"),
        prompt_idle_minutes: num(prompt.get("idle_minutes"), 30.0),
        secrets: BTreeMap::new(),
        directory: None,
    })
}

pub fn load(directory: Option<&Path>) -> Result<Config, LoadError> {
    let directory = directory.map(Path::to_path_buf).unwrap_or_else(config_dir);
    migrate_legacy_settings(&directory);
    let data = read_toml(&directory.join(AGENTS_FILE))?;
    let mut config = from_table(&data)?;
    config.secrets = read_env_file(&directory.join(SECRETS_FILE));
    config.directory = Some(directory);
    Ok(config)
}

/// Change values in agents.toml in place. The file is rewritten atomically, only if every edit verifies.
pub fn update(edits: &[Edit], directory: Option<&Path>) -> Result<(), UpdateError> {
    let directory = directory.map(Path::to_path_buf).unwrap_or_else(config_dir);
    let path = directory.join(AGENTS_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(UpdateError::Io(e.to_string())),
    };
    let new = edit::apply(&text, edits).map_err(UpdateError::Edit)?;
    if new == text {
        return Ok(());
    }
    let tmp = directory.join(format!(".{AGENTS_FILE}.tmp"));
    fs::write(&tmp, &new).map_err(|e| UpdateError::Io(e.to_string()))?;
    if let Ok(meta) = fs::metadata(&path) {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(meta.permissions().mode() & 0o777));
    }
    fs::rename(&tmp, &path).map_err(|e| UpdateError::Io(e.to_string()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateError {
    Edit(EditError),
    Io(String),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::Edit(e) => write!(f, "{e}"),
            UpdateError::Io(e) => f.write_str(e),
        }
    }
}

/// Set (or, with an empty value, remove) KEY in secrets.env, keeping other lines and mode 600.
pub fn set_secret(key: &str, value: &str, directory: Option<&Path>) -> std::io::Result<()> {
    let directory = directory.map(Path::to_path_buf).unwrap_or_else(config_dir);
    let path = directory.join(SECRETS_FILE);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut kept: Vec<String> = pystr::splitlines(&text)
        .into_iter()
        .filter(|ln| {
            let k = pystr::strip(ln).split_once('=').map(|(k, _)| k).unwrap_or(pystr::strip(ln));
            pystr::strip(k) != key
        })
        .map(str::to_string)
        .collect();
    if !value.is_empty() {
        kept.push(format!("{key}={value}"));
    }
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path)?;
    f.write_all((kept.join("\n") + "\n").as_bytes())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
}

/// Changes whenever agents.toml or secrets.env changes; used to reload without restarting.
pub fn signature(directory: Option<&Path>) -> Vec<Option<(i128, u64)>> {
    let directory = directory.map(Path::to_path_buf).unwrap_or_else(config_dir);
    [AGENTS_FILE, SECRETS_FILE]
        .iter()
        .map(|name| {
            fs::metadata(directory.join(name)).ok().map(|m| {
                use std::os::unix::fs::MetadataExt;
                (i128::from(m.mtime()) * 1_000_000_000 + i128::from(m.mtime_nsec()), m.size())
            })
        })
        .collect()
}

/// General settings with factory values. A factory reset puts these back and leaves agents,
/// aliases, routing rules and the gateway alone.
pub fn factory_edits() -> Vec<Edit> {
    let d = Config::default();
    vec![
        Edit::table("search", "min_chars", Scalar::Int(d.min_chars)),
        Edit::table("search", "max_rows", Scalar::Int(d.max_rows)),
        Edit::table("prompt", "idle_minutes", Scalar::Float(d.prompt_idle_minutes)),
        Edit::table("routing", "low_effort_max_words", Scalar::Int(d.low_effort_max_words)),
    ]
}

pub fn restore_defaults(directory: Option<&Path>) -> Result<(), UpdateError> {
    update(&factory_edits(), directory)
}

/// Fold a 0.4.0 settings.toml into agents.toml once, then remove it.
pub fn migrate_legacy_settings(directory: &Path) {
    let legacy = directory.join(LEGACY_SETTINGS_FILE);
    if !legacy.exists() {
        return;
    }
    let Ok(data) = read_toml(&legacy) else { return };
    let mut edits = Vec::new();
    for table in ["search", "prompt", "routing", "gateway"] {
        if let Some(Value::Table(t)) = data.get(table) {
            for (k, v) in t {
                if let Some(s) = Scalar::from_toml(v) {
                    edits.push(Edit::table(table, k, s));
                }
            }
        }
    }
    if let Some(Value::Table(apps)) = data.get("app") {
        for (app_id, values) in apps {
            if let Value::Table(values) = values {
                for (k, v) in values {
                    if ["enabled", "priority", "cwd"].contains(&k.as_str()) {
                        if let Some(s) = Scalar::from_toml(v) {
                            edits.push(Edit::item("app", app_id, k, s));
                        }
                    }
                }
            }
        }
    }
    if update(&edits, Some(directory)).is_ok() {
        let _ = fs::rename(&legacy, directory.join(format!("{LEGACY_SETTINGS_FILE}.migrated")));
    }
}
