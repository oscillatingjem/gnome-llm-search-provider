//! Replays tests/golden/*.jsonl, recorded from the Python implementation.
//! Every case must match exactly.

use std::path::PathBuf;

use agent_search_core::config::{self, App, CommandItem, Config};
use agent_search_core::edit::{self, Edit, Scalar};
use agent_search_core::{catalog, launch, query, ranker};
use serde_json::{Value, json};

fn golden(name: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect()
}

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/config")
}

fn which(p: &str) -> Option<String> {
    if p.starts_with("missing-") { None } else { Some(format!("/usr/bin/{p}")) }
}

fn gateway_info() -> Vec<Value> {
    serde_json::from_value(json!([
        {"model_name": "smart-router", "model_info": {"display_name": "Auto"}},
        {"model_name": "coder", "model_info": {"display_name": "Qwen Coder", "tags": ["coding", "private"], "aliases": ["Coder", "qwen"]}},
        {"model_name": "big@low", "model_info": {"base": "big", "effort": "low", "display_name": "Big", "tags": ["reasoning"]}},
        {"model_name": "big@high", "model_info": {"base": "big", "effort": "high"}},
        {"model_name": "plain"}
    ]))
    .unwrap_or_default()
}

fn configs() -> Vec<(&'static str, Config, bool)> {
    let load = || config::load(Some(&examples())).unwrap_or_else(|e| panic!("{e}"));
    let base = load();
    let mut extra = load();
    let mut agent = App::new("agent", "Agent");
    agent.url = "x://new?q={prompt}&model={model}&effort={effort}".into();
    agent.tags = vec!["agent".into()];
    agent.models = vec!["opus".into(), "sonnet".into()];
    agent.efforts = vec!["low".into(), "high".into()];
    extra.apps.push(agent);
    let mut gone = App::new("gone", "Gone");
    gone.command = vec![CommandItem::Arg("missing-tool".into()), CommandItem::Arg("{prompt}".into())];
    gone.requires = vec!["missing-tool".into()];
    extra.apps.push(gone);
    let mut off = App::new("off", "Off");
    off.url = "y://{prompt}".into();
    off.enabled = false;
    extra.apps.push(off);
    extra.aliases.push(("opus".into(), "agent:opus".into()));
    extra.aliases.push(("big".into(), "model:big".into()));
    extra.chat_url = "http://127.0.0.1:3000/?models={model}&q={prompt}".into();
    let mut small = load();
    small.max_rows = 2;
    small.min_chars = 1;
    small.low_effort_max_words = 3;
    vec![("example", base, false), ("extended", extra, true), ("small", small, true)]
}

fn home_tilde(s: &str) -> String {
    let home = config::home_dir().to_string_lossy().into_owned();
    s.replace(&home, "~")
}

#[test]
fn ranking_matches_python() {
    let configs = configs();
    let mut failures = Vec::new();
    let cases = golden("ranking.jsonl");
    for case in &cases {
        let (_, cfg, gateway) = configs.iter().find(|(n, _, _)| *n == case["config"]).unwrap_or_else(|| panic!("config"));
        let mut cands = Vec::new();
        if *gateway {
            cands.extend(catalog::models_from_info(&gateway_info(), &cfg.auto_model));
        }
        cands.extend(catalog::app_candidates(cfg, &which));
        let terms: Vec<String> = serde_json::from_value(case["terms"].clone()).unwrap_or_default();
        let q = query::parse(&terms);
        let parsed = json!({"prompt": q.prompt, "model": q.model, "effort": q.effort});
        let rows = ranker::rank(&q, &cands, cfg).unwrap_or_else(|e| panic!("{e}"));
        let mut got_rows = Vec::new();
        for r in &rows {
            let id = r.result_id();
            let mut row = json!({"id": id, "title": r.title(), "subtitle": r.subtitle(), "icon": r.candidate.icon});
            let (cid, model, effort) = ranker::parse_result_id(&id);
            let app = cfg.apps.iter().find(|a| format!("app:{}", a.id) == cid);
            match app.filter(|a| !a.command.is_empty()) {
                Some(app) => {
                    let (argv, cwd) = launch::command_for(app, model.as_deref(), effort.as_deref(), &q.prompt, &which);
                    row["command"] = json!({"argv": argv, "cwd": home_tilde(&cwd)});
                }
                None => match launch::url_for(cfg, &cid, model.as_deref(), effort.as_deref(), &q.prompt) {
                    Ok(url) => row["url"] = json!(url),
                    Err(e) => row["error"] = json!(e.0),
                },
            }
            got_rows.push(row);
        }
        let got = json!({"config": case["config"], "terms": terms, "parsed": parsed, "rows": got_rows});
        if &got != case {
            failures.push(format!("expected {case}\n     got {got}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} ranking cases differ:\n{}", failures.len(), cases.len(), failures[..failures.len().min(5)].join("\n"));
}

#[test]
fn units_match_python() {
    let cfg = config::load(Some(&examples())).unwrap_or_else(|e| panic!("{e}"));
    let mut failures = Vec::new();
    for case in golden("units.jsonl") {
        let (kind, input, want) = (&case["kind"], &case["in"], &case["out"]);
        let got: Value = match kind.as_str().unwrap_or("") {
            "suggest_effort" => json!(ranker::suggest_effort(input.as_str().unwrap_or(""), &cfg).unwrap_or_default()),
            "clamp_effort" => {
                let supported: Vec<String> = serde_json::from_value(input[1].clone()).unwrap_or_default();
                json!(ranker::clamp_effort(input[0].as_str(), &supported))
            }
            "fill_url" => {
                let v = &input[1];
                json!(launch::fill(input[0].as_str().unwrap_or(""), v["prompt"].as_str(), v["model"].as_str(), v["effort"].as_str()).map_err(|e| e.0).unwrap_or_default())
            }
            "fill_command" => {
                let tmpl: Vec<CommandItem> = input[0]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|x| match x {
                                Value::Array(g) => CommandItem::Group(g.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect()),
                                s => CommandItem::Arg(s.as_str().unwrap_or("").to_string()),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let v = &input[1];
                json!(launch::fill_command(&tmpl, v["prompt"].as_str(), v["model"].as_str(), v["effort"].as_str()))
            }
            "parse_result_id" => {
                let (c, m, e) = ranker::parse_result_id(input.as_str().unwrap_or(""));
                json!([c, m, e])
            }
            "query_key" => {
                let terms: Vec<&str> = agent_search_core::pystr::split(input.as_str().unwrap_or(""));
                json!(ranker::query_key(&query::parse(&terms)))
            }
            "models_from_info" => {
                let data: Vec<Value> = serde_json::from_value(input.clone()).unwrap_or_default();
                json!(catalog::models_from_info(&data, "smart-router")
                    .iter()
                    .map(|c| json!({
                        "id": c.id,
                        "kind": match c.kind { ranker::Kind::App => "app", ranker::Kind::Model => "model", ranker::Kind::Auto => "auto" },
                        "name": c.name, "tags": c.tags, "efforts": c.efforts, "models": c.models,
                        "aliases": c.aliases, "icon": c.icon, "priority": c.priority,
                    }))
                    .collect::<Vec<_>>())
            }
            other => panic!("unknown kind {other}"),
        };
        if &got != want {
            failures.push(format!("{kind} {input}: expected {want}, got {got}"));
        }
    }
    assert!(failures.is_empty(), "{} unit cases differ:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn config_and_edits_match_python() {
    let mut failures = Vec::new();
    let original = std::fs::read_to_string(examples().join("agents.toml")).unwrap_or_default();
    for case in golden("config.jsonl") {
        let (kind, input, want) = (case["kind"].as_str().unwrap_or(""), &case["in"], &case["out"]);
        match kind {
            "read_env_file" => {
                let got = json!(config::parse_env(input.as_str().unwrap_or("")));
                if &got != want {
                    failures.push(format!("read_env_file: expected {want}, got {got}"));
                }
            }
            "toml_apply" => {
                let edits: Vec<Edit> = input
                    .as_array()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .map(|e| Edit {
                        table: e[0].as_str().unwrap_or("").into(),
                        key: e[1].as_str().unwrap_or("").into(),
                        item_id: e[2].as_str().map(str::to_string),
                        value: Scalar::from_json(&e[3]).unwrap_or(Scalar::Str(String::new())),
                    })
                    .collect();
                let got = match edit::apply(&original, &edits) {
                    Ok(text) => json!({"text": text}),
                    Err(e) => json!({"error": e.0}),
                };
                if &got != want {
                    failures.push(format!("toml_apply {input}: expected {want}\n got {got}"));
                }
            }
            "load_example" => {
                let c = config::load(Some(&examples())).unwrap_or_else(|e| panic!("{e}"));
                for (key, value) in [
                    ("min_chars", json!(c.min_chars)),
                    ("max_rows", json!(c.max_rows)),
                    ("low_effort_max_words", json!(c.low_effort_max_words)),
                    ("litellm_url", json!(c.litellm_url)),
                    ("chat_url", json!(c.chat_url)),
                    ("auto_model", json!(c.auto_model)),
                    ("high_effort_patterns", json!(c.high_effort_patterns)),
                ] {
                    if want[key] != value {
                        failures.push(format!("load {key}: expected {}, got {value}", want[key]));
                    }
                }
                let ids: Vec<&str> = c.apps.iter().map(|a| a.id.as_str()).collect();
                let want_ids: Vec<&str> = want["apps"].as_array().map(|a| a.iter().filter_map(|x| x["id"].as_str()).collect()).unwrap_or_default();
                if ids != want_ids {
                    failures.push(format!("load apps: expected {want_ids:?}, got {ids:?}"));
                }
                if c.rules.len() != want["rules"].as_array().map_or(0, Vec::len) {
                    failures.push("load rules count differs".into());
                }
            }
            other => panic!("unknown kind {other}"),
        }
    }
    assert!(failures.is_empty(), "{} config cases differ:\n{}", failures.len(), failures.join("\n"));
}
