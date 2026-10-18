//! The GLib side shared by `llm-search` and `agent-search`: running activations, the clipboard,
//! the gateway HTTP call, D-Bus calls to the prompt box, and the keyboard shortcut.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use agent_search_core::catalog::GatewayCache;
use agent_search_core::config::Config;
use agent_search_core::launch::{Activation, Launch};
use agent_search_core::ranker::Candidate;
use agent_search_core::{Identity, launch};
use gio::prelude::*;
use glib::translate::IntoGlib;
use soup::prelude::*;

pub mod shortcut;

/// Log like the Python version: one line on stderr, prefixed "llm-search: ".
pub fn log(msg: &str) {
    eprintln!("llm-search: {msg}");
}

pub fn copy_to_clipboard(text: &str) {
    let run = |cmd: &mut Command| -> bool {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut child| {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
            })
            .is_ok()
    };
    if launch::which("wl-copy").is_some() {
        // Python passes the text as an argument ("wl-copy -- TEXT").
        let _ = Command::new("wl-copy").arg("--").arg(text).stdout(Stdio::null()).stderr(Stdio::null()).status();
    } else if launch::which("xclip").is_some() {
        run(Command::new("xclip").args(["-selection", "clipboard"]));
    }
}

/// Run a command detached from us (new session, no stdio), like `subprocess.Popen(start_new_session=True)`.
pub fn spawn(argv: &[String], cwd: &str) -> std::io::Result<()> {
    let (program, args) = argv.split_first().ok_or_else(|| std::io::Error::other("empty command"))?;
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid() is async-signal-safe and only affects the child.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    let pid = glib::Pid(child.id() as i32);
    // Reap the child when it exits so it never lingers as a zombie.
    glib::child_watch_add(pid, |_, _| {});
    std::mem::forget(child);
    Ok(())
}

pub fn open_uri(uri: &str) -> Result<(), glib::Error> {
    gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
}

/// Copy first (if asked), then run or open. Errors are returned for the D-Bus reply.
pub fn execute(activation: &Activation) -> Result<(), String> {
    if let Some(text) = &activation.copy {
        copy_to_clipboard(text);
    }
    match &activation.launch {
        Launch::Spawn { argv, cwd } => {
            log(&format!("running {} in {cwd}", argv.first().map(String::as_str).unwrap_or("")));
            spawn(argv, cwd).map_err(|e| e.to_string())
        }
        Launch::OpenUri(url) => {
            log(&format!("opening {}", url.split('?').next().unwrap_or(url)));
            open_uri(url).map_err(|e| e.to_string())
        }
        Launch::Nothing => Ok(()),
    }
}

/// Seconds since boot, like Python's time.monotonic().
pub fn monotonic_seconds() -> f64 {
    glib::monotonic_time() as f64 / 1_000_000.0
}

/// The LiteLLM gateway's model list, cached (60 s) with a retry back-off (30 s).
pub struct Gateway {
    cache: GatewayCache,
}

impl Default for Gateway {
    fn default() -> Self {
        Gateway { cache: GatewayCache::default() }
    }
}

impl Gateway {
    pub fn models(&mut self, config: &Config) -> Vec<Candidate> {
        if config.litellm_url.is_empty() {
            return Vec::new();
        }
        let now = monotonic_seconds();
        if self.cache.due(now) {
            let result = fetch_model_info(config);
            self.cache.store(now, result, &config.auto_model);
        }
        self.cache.models.clone()
    }
}

fn fetch_model_info(config: &Config) -> Result<Vec<serde_json::Value>, String> {
    let session = soup::Session::new();
    session.set_timeout(1);
    let url = format!("{}/model/info", config.litellm_url);
    let msg = soup::Message::new("GET", &url).map_err(|e| e.to_string())?;
    if let Some(key) = config.secrets.get(&config.litellm_key_env).filter(|k| !k.is_empty()) {
        if let Some(headers) = msg.request_headers() {
            headers.append("Authorization", &format!("Bearer {key}"));
        }
    }
    let body = session.send_and_read(&msg, None::<&gio::Cancellable>).map_err(|e| e.to_string())?;
    let code = msg.status().into_glib() as u32;
    if !(200..300).contains(&code) {
        return Err(format!("HTTP {code}"));
    }
    let data: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(data.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default())
}

/// Ask the prompt box (D-Bus activating it if needed) to open, optionally with text.
pub fn open_prompt_box(conn: &gio::DBusConnection, text: Option<&str>) {
    let id = Identity::current();
    let path = Identity::object_path(&id.app_id);
    let mut platform: HashMap<String, glib::Variant> = HashMap::new();
    if let Ok(token) = std::env::var("XDG_ACTIVATION_TOKEN") {
        platform.insert("activation-token".into(), token.to_variant());
    }
    if let Ok(id) = std::env::var("DESKTOP_STARTUP_ID") {
        platform.insert("desktop-startup-id".into(), id.to_variant());
    }
    let (method, params) = match text {
        Some(t) => ("ActivateAction", ("search", vec![t.to_variant()], platform).to_variant()),
        None => ("Activate", (platform,).to_variant()),
    };
    let _ = conn.call_sync(
        Some(&id.app_id),
        &path,
        "org.freedesktop.Application",
        method,
        Some(&params),
        None,
        gio::DBusCallFlags::NONE,
        5000,
        None::<&gio::Cancellable>,
    );
}
