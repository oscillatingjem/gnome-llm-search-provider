//! Agent Search core: query parsing, ranking, config loading and editing. No GTK, no GLib.

pub mod catalog;
pub mod config;
pub mod edit;
pub mod launch;
pub mod pyre;
pub mod pystr;
pub mod query;
pub mod ranker;

/// App identity. `AGENT_SEARCH_VARIANT=rs` gives the side-by-side Rust build its own IDs, name,
/// shortcut path and data folder, so it can be installed next to the Python version.
#[derive(Debug, Clone)]
pub struct Identity {
    pub app_id: String,
    pub provider_id: String,
    pub name: String,
    pub cli_name: String,
    pub shortcut_slug: String,
}

impl Identity {
    pub fn current() -> Identity {
        // Set in the side-by-side .service files; `llm-search-rs` (the side-by-side command) implies it.
        let side_by_side = std::env::var("AGENT_SEARCH_VARIANT").map(|v| v == "rs").unwrap_or(false)
            || std::env::args()
                .next()
                .map(|a| std::path::Path::new(&a).file_name().is_some_and(|n| n.to_string_lossy().ends_with("-rs")))
                .unwrap_or(false);
        let (suffix, name, cli) = if side_by_side {
            ("Rs", "Agent Search (Rust)", "llm-search-rs")
        } else {
            ("", "Agent Search", "llm-search")
        };
        let app_id = format!("io.github.oscillatingjem.AgentSearch{suffix}");
        Identity {
            provider_id: format!("{app_id}.SearchProvider"),
            app_id,
            name: name.into(),
            cli_name: cli.into(),
            shortcut_slug: cli.into(),
        }
    }

    pub fn object_path(id: &str) -> String {
        format!("/{}", id.replace('.', "/"))
    }
}
