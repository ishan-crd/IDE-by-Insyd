//! Runs the InsyDE web server inside the app while Settings › Web access is
//! on, restarting it when its settings change. The server itself lives in
//! `insyde_core::web` (also used headless by `insy serve`).

use insyde_core::store::Store;
use insyde_core::web::hub::Hub;
use insyde_core::web::{Link, WebServer};
use parking_lot::Mutex;
use std::sync::{Arc, LazyLock, OnceLock};

/// One hub for the app's lifetime: agent threads and terminals started from
/// a browser keep running when the server restarts with new settings.
static HUB: OnceLock<Arc<Hub>> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
struct Config {
    port: u16,
    network: bool,
    tunnel: bool,
}

#[derive(Default)]
struct State {
    running: Option<(Config, WebServer)>,
    error: Option<String>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Mutex::default);

/// Start, restart or stop the server to match settings. Cheap when nothing changed.
pub fn sync(store: &Store) {
    let s = insyde_core::settings::get();
    let want = s.web_access.then_some(Config {
        port: s.web_port,
        network: s.web_network,
        tunnel: s.web_tunnel,
    });
    let mut st = STATE.lock();
    if st.running.as_ref().map(|r| r.0) == want && (want.is_none() || st.error.is_none()) {
        return;
    }
    // Stop first: the new server may need the same port.
    st.running = None;
    st.error = None;
    let Some(cfg) = want else { return };
    let hub = HUB.get_or_init(|| Hub::new(store.clone())).clone();
    match WebServer::start_with(hub, cfg.port, cfg.network, cfg.tunnel) {
        Ok(server) => st.running = Some((cfg, server)),
        Err(e) => st.error = Some(format!("{e:#}")),
    }
}

/// What the settings screen shows.
pub struct Snapshot {
    pub links: Vec<Link>,
    pub clients: usize,
    pub error: Option<String>,
    pub tunnel_pending: bool,
    pub tunnel_error: Option<String>,
}

pub fn snapshot() -> Option<Snapshot> {
    let st = STATE.lock();
    if let Some(e) = &st.error {
        return Some(Snapshot {
            links: vec![],
            clients: 0,
            error: Some(e.clone()),
            tunnel_pending: false,
            tunnel_error: None,
        });
    }
    let (_, server) = st.running.as_ref()?;
    Some(Snapshot {
        links: server.links(),
        clients: server.clients(),
        error: None,
        tunnel_pending: server.tunnel_pending(),
        tunnel_error: server.tunnel_error(),
    })
}

/// New pairing link: every paired browser has to pair again.
pub fn new_link() {
    insyde_core::web::rotate_token();
    if let Some((_, server)) = &STATE.lock().running {
        server.hub.disconnect_all();
    }
}
