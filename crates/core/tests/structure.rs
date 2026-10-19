//! The full settings editor's structural edits: comments survive, operations invert, rename
//! updates aliases, invalid results are refused.

use std::path::PathBuf;

use agent_search_core::config;
use agent_search_core::edit::Scalar;
use agent_search_core::structure::{ListItem, Op, Sel, apply};

fn example() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/config/agents.toml");
    std::fs::read_to_string(p).unwrap_or_default()
}

fn comments(s: &str) -> Vec<String> {
    s.lines().filter(|l| l.trim_start().starts_with('#')).map(str::to_string).collect()
}

fn load(text: &str) -> config::Config {
    config::from_table(&text.parse().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn add_then_delete_agent_restores_exact_bytes() {
    let original = example();
    let added = apply(&original, &[Op::AddApp {
        id: "perplexity".into(),
        name: "Perplexity".into(),
        url: Some("https://www.perplexity.ai/search?q={prompt}".into()),
        command: None,
    }])
    .unwrap_or_else(|e| panic!("{e}"));
    let c = load(&added);
    assert_eq!(c.apps.last().map(|a| a.id.as_str()), Some("perplexity"));
    assert_eq!(comments(&added), comments(&original));
    let back = apply(&added, &[Op::DeleteApp { id: "perplexity".into() }]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back, original);
}

#[test]
fn add_then_delete_rule_restores_exact_bytes() {
    let original = example();
    let added = apply(&original, &[Op::AddRule { tag: "research".into(), weight: 3.0, patterns: vec![r"\bsources?\b".into()] }])
        .unwrap_or_else(|e| panic!("{e}"));
    let c = load(&added);
    assert_eq!(c.rules.last().map(|r| r.tag.as_str()), Some("research"));
    let n = c.rules.len() - 1;
    let back = apply(&added, &[Op::DeleteRule { index: n }]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back, original);
}

#[test]
fn deleting_an_existing_agent_takes_its_comment_block() {
    let out = apply(&example(), &[Op::DeleteApp { id: "claude-code".into() }]).unwrap_or_else(|e| panic!("{e}"));
    let c = load(&out);
    assert!(c.apps.iter().all(|a| a.id != "claude-code"));
    assert!(!out.contains("# Claude Desktop, new Claude Code session."));
    assert!(out.contains("# Claude Code in a terminal window."));
}

#[test]
fn rename_updates_aliases() {
    let out = apply(&example(), &[Op::RenameApp { from: "claude-cli".into(), to: "terminal".into() }]).unwrap_or_else(|e| panic!("{e}"));
    let c = load(&out);
    assert!(c.apps.iter().any(|a| a.id == "terminal"));
    assert_eq!(c.alias("cli"), Some("terminal"));
    assert_eq!(c.alias("term"), Some("terminal"));
}

#[test]
fn lists_replace_multiline_values_and_keep_other_comments() {
    let original = example();
    let cmd = vec![
        ListItem::Str("kgx".into()), ListItem::Str("--".into()), ListItem::Str("claude".into()),
        ListItem::Group(vec!["--model".into(), "{model}".into()]), ListItem::Str("--".into()), ListItem::Str("{prompt}".into()),
    ];
    let out = apply(&original, &[
        Op::SetList { table: "app".into(), sel: Sel::Id("claude-cli".into()), key: "command".into(), items: cmd },
        Op::SetList { table: "app".into(), sel: Sel::Id("claude-cli".into()), key: "tags".into(), items: vec![ListItem::Str("coding".into())] },
        Op::SetList { table: "routing.rule".into(), sel: Sel::Index(0), key: "patterns".into(), items: vec![ListItem::Str(r"\bcode\b".into())] },
    ])
    .unwrap_or_else(|e| panic!("{e}"));
    let c = load(&out);
    let cli = c.apps.iter().find(|a| a.id == "claude-cli").unwrap_or_else(|| panic!("cli"));
    assert_eq!(cli.tags, vec!["coding".to_string()]);
    assert_eq!(c.rules[0].patterns, vec![r"\bcode\b".to_string()]);
    // Only the two comments inside the coding rule's old pattern list are gone.
    let lost: Vec<String> = comments(&original).into_iter().filter(|l| !comments(&out).contains(l)).collect();
    assert_eq!(lost.len(), 0, "{lost:?}");
}

#[test]
fn aliases_add_change_remove() {
    let original = example();
    let out = apply(&original, &[Op::Set { table: "aliases".into(), sel: Sel::Table, key: "deep".into(), value: Scalar::Str("claude-cli:opus".into()) }])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(load(&out).alias("deep"), Some("claude-cli:opus"));
    let back = apply(&out, &[Op::Remove { table: "aliases".into(), sel: Sel::Table, key: "deep".into() }]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back, original);
}

#[test]
fn rule_scalars_by_index() {
    let out = apply(&example(), &[
        Op::Set { table: "routing.rule".into(), sel: Sel::Index(1), key: "weight".into(), value: Scalar::Float(2.5) },
        Op::Set { table: "routing.rule".into(), sel: Sel::Index(1), key: "tag".into(), value: Scalar::Str("prose".into()) },
    ])
    .unwrap_or_else(|e| panic!("{e}"));
    let c = load(&out);
    assert_eq!((c.rules[1].tag.as_str(), c.rules[1].weight), ("prose", 2.5));
}

#[test]
fn invalid_results_are_refused() {
    let original = example();
    let bad_regex = apply(&original, &[Op::SetList { table: "routing".into(), sel: Sel::Table, key: "high_effort_patterns".into(), items: vec![ListItem::Str("(unclosed".into())] }]);
    assert!(bad_regex.is_err());
    let dup = apply(&original, &[Op::RenameApp { from: "claude-cli".into(), to: "claude-chat".into() }]);
    assert!(dup.is_err());
    let bad_id = apply(&original, &[Op::AddApp { id: "has space".into(), name: "X".into(), url: Some("x://".into()), command: None }]);
    assert!(bad_id.is_err());
    let missing = apply(&original, &[Op::DeleteApp { id: "nope".into() }]);
    assert!(missing.is_err());
}
