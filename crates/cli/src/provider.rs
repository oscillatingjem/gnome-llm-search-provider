//! GNOME Shell search provider (org.gnome.Shell.SearchProvider2) for LLMs and agents.
//!
//! Owns the provider's bus name, serves the interface, and exits after 60 s without calls.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use agent_search_core::config::{self, Config};
use agent_search_core::ranker::{self, Row};
use agent_search_core::{Identity, catalog, launch, query};
use agent_search_platform as platform;
use gio::prelude::*;
use glib::Variant;

const IDLE: Duration = Duration::from_secs(60);

const INTERFACE_XML: &str = r#"
<node>
  <interface name="org.gnome.Shell.SearchProvider2">
    <method name="GetInitialResultSet">
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetSubsearchResultSet">
      <arg type="as" name="previous_results" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetResultMetas">
      <arg type="as" name="identifiers" direction="in"/>
      <arg type="aa{sv}" name="metas" direction="out"/>
    </method>
    <method name="ActivateResult">
      <arg type="s" name="identifier" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
    <method name="LaunchSearch">
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
  </interface>
</node>
"#;

struct Provider {
    config: Config,
    gateway: platform::Gateway,
    signature: Vec<Option<(i128, u64)>>,
    rows: Vec<Row>,
    prompt: String,
}

impl Provider {
    fn new() -> Result<Provider, String> {
        Ok(Provider {
            config: config::load(None).map_err(|e| e.0)?,
            gateway: platform::Gateway::default(),
            signature: config::signature(None),
            rows: Vec::new(),
            prompt: String::new(),
        })
    }

    fn reload_if_changed(&mut self) -> Result<(), String> {
        let sig = config::signature(None);
        if sig != self.signature {
            self.config = config::load(None).map_err(|e| e.0)?;
            self.gateway = platform::Gateway::default();
            self.signature = sig;
        }
        Ok(())
    }

    fn search(&mut self, terms: &[String]) -> Result<Vec<String>, String> {
        self.reload_if_changed()?;
        let q = query::parse(terms);
        let models = self.gateway.models(&self.config);
        let cands = catalog::candidates(&self.config, &models, &launch::which);
        let rows = ranker::rank(&q, &cands, &self.config)?;
        self.prompt = q.prompt;
        // Python keeps rows in a dict keyed by id: a duplicate id keeps its first position.
        let mut unique: Vec<Row> = Vec::new();
        for r in rows {
            match unique.iter_mut().find(|u| u.result_id() == r.result_id()) {
                Some(slot) => *slot = r,
                None => unique.push(r),
            }
        }
        self.rows = unique;
        Ok(self.rows.iter().map(Row::result_id).collect())
    }

    fn metas(&self, ids: &[String]) -> Vec<HashMap<String, Variant>> {
        ids.iter()
            .filter_map(|rid| self.rows.iter().find(|r| &r.result_id() == rid))
            .map(|row| {
                let mut description = format!("“{}”", self.prompt);
                let sub = row.subtitle();
                if !sub.is_empty() {
                    description.push_str(&format!(" · {sub}"));
                }
                let icon = gio::ThemedIcon::new(&row.candidate.icon);
                let icon = IconExt::to_string(&icon).map(|s| s.to_string()).unwrap_or_else(|| row.candidate.icon.clone());
                HashMap::from([
                    ("id".to_string(), row.result_id().to_variant()),
                    ("name".to_string(), row.title().to_variant()),
                    ("description".to_string(), description.to_variant()),
                    ("gicon".to_string(), icon.to_variant()),
                ])
            })
            .collect()
    }

    fn activate(&mut self, rid: &str, terms: &[String]) -> Result<(), String> {
        let prompt = query::parse(terms).prompt;
        let activation = launch::activation(&self.config, rid, &prompt, &launch::which)?;
        platform::execute(&activation)
    }
}

pub fn run(_args: Vec<String>) -> i32 {
    let id = Identity::current();
    let main_loop = glib::MainLoop::new(None, false);
    let provider = match Provider::new() {
        Ok(p) => Rc::new(RefCell::new(p)),
        Err(e) => {
            platform::log(&format!("agents.toml: {e}"));
            // Still serve (empty results) so GNOME Shell doesn't wait on us.
            Rc::new(RefCell::new(Provider {
                config: Config::default(),
                gateway: platform::Gateway::default(),
                signature: Vec::new(),
                rows: Vec::new(),
                prompt: String::new(),
            }))
        }
    };
    let idle: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let restart_idle = {
        let idle = idle.clone();
        let main_loop = main_loop.clone();
        move || {
            if let Some(src) = idle.borrow_mut().take() {
                src.remove();
            }
            let ml = main_loop.clone();
            let idle_inner = idle.clone();
            let src = glib::timeout_add_local_once(IDLE, move || {
                idle_inner.borrow_mut().take();
                ml.quit();
            });
            *idle.borrow_mut() = Some(src);
        }
    };
    restart_idle();

    let Ok(node) = gio::DBusNodeInfo::for_xml(INTERFACE_XML) else { return 1 };
    let Some(iface) = node.lookup_interface("org.gnome.Shell.SearchProvider2") else { return 1 };
    let object_path = Identity::object_path(&id.provider_id);

    let ml_lost = main_loop.clone();
    let _owner = gio::bus_own_name(
        gio::BusType::Session,
        &id.provider_id,
        gio::BusNameOwnerFlags::NONE,
        move |conn, _name| {
            let provider = provider.clone();
            let restart_idle = restart_idle.clone();
            let call_conn = conn.clone();
            let registered = conn
                .register_object(&object_path, &iface)
                .method_call(move |_conn, _sender, _path, _iface, method, params, invocation| {
                    restart_idle();
                    let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        handle(&provider, &call_conn, method, &params)
                    }))
                    .unwrap_or_else(|_| Err(format!("{method} failed")));
                    match reply {
                        Ok(value) => invocation.return_value(value.as_ref()),
                        Err(e) if e == "UnknownMethod" => invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method),
                        Err(e) => {
                            platform::log(&format!("{method} failed: {e}"));
                            invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", &e);
                        }
                    }
                })
                .build();
            if let Err(e) = registered {
                platform::log(&format!("could not register the provider: {e}"));
            }
        },
        |_conn, _name| {},
        move |_conn, _name| ml_lost.quit(),
    );
    main_loop.run();
    0
}

fn handle(provider: &Rc<RefCell<Provider>>, conn: &gio::DBusConnection, method: &str, params: &Variant) -> Result<Option<Variant>, String> {
    let bad = || "invalid arguments".to_string();
    match method {
        "GetInitialResultSet" => {
            let (terms,) = params.get::<(Vec<String>,)>().ok_or_else(bad)?;
            Ok(Some((provider.borrow_mut().search(&terms)?,).to_variant()))
        }
        "GetSubsearchResultSet" => {
            let (_, terms) = params.get::<(Vec<String>, Vec<String>)>().ok_or_else(bad)?;
            Ok(Some((provider.borrow_mut().search(&terms)?,).to_variant()))
        }
        "GetResultMetas" => {
            let (ids,) = params.get::<(Vec<String>,)>().ok_or_else(bad)?;
            Ok(Some((provider.borrow().metas(&ids),).to_variant()))
        }
        "ActivateResult" => {
            let (rid, terms, _ts) = params.get::<(String, Vec<String>, u32)>().ok_or_else(bad)?;
            provider.borrow_mut().activate(&rid, &terms)?;
            Ok(None)
        }
        "LaunchSearch" => {
            // Clicking the category icon opens the prompt box with the query, to refine it there.
            let (terms, _ts) = params.get::<(Vec<String>, u32)>().ok_or_else(bad)?;
            platform::open_prompt_box(conn, Some(&terms.join(" ")));
            Ok(None)
        }
        _ => Err("UnknownMethod".into()),
    }
}
