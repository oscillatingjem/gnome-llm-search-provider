//! `llm-search`: the GNOME search provider (`llm-search search-provider`) and the command line.
//! No GTK is linked, so this runs even where libadwaita is missing.

mod provider;

use agent_search_core::config;
use agent_search_core::launch;
use agent_search_core::{Identity, query, ranker};
use agent_search_platform as platform;

const USAGE: &str = "\
Usage: llm-search <command> [args]

  prompt [TEXT...]      Open the prompt box (with TEXT filled in)
  query WORDS...        Show the rows GNOME search would get for WORDS
  check                 Load the config and compile every routing pattern
  status                Show which config and IDs this build uses
  version               Print the version
  search-provider       Run the GNOME search provider (started by D-Bus)
";

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let command = if args.len() > 1 { args.remove(1) } else { String::new() };
    let code = match command.as_str() {
        // The subcommand is removed before GLib sees argv, so `--gapplication-*` options still work.
        "search-provider" => provider::run(args),
        "prompt" | "open" => prompt(&args[1..]),
        "query" => query_cmd(&args[1..]),
        "check" | "test" => check(),
        "status" => status(),
        "version" | "--version" => {
            println!("llm-search {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            0
        }
        _ => {
            eprint!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

fn prompt(words: &[String]) -> i32 {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        eprintln!("error: no D-Bus session bus");
        return 1;
    };
    let text = (!words.is_empty()).then(|| words.join(" "));
    platform::open_prompt_box(&conn, text.as_deref());
    0
}

fn query_cmd(words: &[String]) -> i32 {
    let cfg = match config::load(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let mut gateway = platform::Gateway::default();
    let models = gateway.models(&cfg);
    let cands = agent_search_core::catalog::candidates(&cfg, &models, &launch::which);
    match ranker::rank(&query::parse(words), &cands, &cfg) {
        Ok(rows) => {
            for r in rows {
                let sub = r.subtitle();
                println!("{}  {}{}", r.result_id(), r.title(), if sub.is_empty() { String::new() } else { format!(" · {sub}") });
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn check() -> i32 {
    let cfg = match config::load(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("agents.toml: {e}");
            return 1;
        }
    };
    let mut bad = 0;
    let patterns = cfg.high_effort_patterns.iter().chain(cfg.rules.iter().flat_map(|r| r.patterns.iter()));
    for p in patterns {
        if let Err(e) = agent_search_core::pyre::compile(p) {
            println!("pattern {p:?}: {e}");
            bad += 1;
        }
    }
    println!("config: {} agents, {} rules, {} aliases; {bad} pattern problem(s)", cfg.apps.len(), cfg.rules.len(), cfg.aliases.len());
    i32::from(bad > 0)
}

fn status() -> i32 {
    let id = Identity::current();
    println!("name:            {}", id.name);
    println!("prompt box id:   {}", id.app_id);
    println!("provider id:     {}", id.provider_id);
    println!("config:          {}", config::config_dir().display());
    let shortcut = platform::shortcut::get();
    println!("shortcut:        {}", if shortcut.is_empty() { "none".into() } else { shortcut });
    0
}
