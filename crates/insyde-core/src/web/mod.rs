//! InsyDE web access: serve a browser client that drives this machine's
//! projects, agent threads and terminals, from anywhere you can reach it.
//!
//! - One small HTTP server on std sockets (no async runtime): static files
//!   compiled into the binary, plus one WebSocket per browser tab.
//! - Pairing is a 256-bit token in the link (`/#t=…`), kept by the browser.
//!   Holding it is equivalent to a shell on this machine, so it never goes
//!   into server logs and can be rotated, which disconnects every client.
//! - Reachability: this machine only (default), the local network and
//!   Tailscale, or a temporary public https link through `cloudflared`.

mod assets;
pub mod http;
pub mod hub;
pub mod pty;

use crate::store::Store;
use anyhow::{Context as _, Result};
use http::Message;
use hub::{Hub, Out};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Largest message a browser may send (pasted text, long prompts).
const MAX_MESSAGE: usize = 8 << 20;

fn token_path() -> std::path::PathBuf {
    crate::store::data_dir().join("web-token")
}

/// The pairing token, created on first use and kept in the data folder (0600).
/// Read on every check so the app and `insy serve` always agree.
pub fn token() -> String {
    std::fs::read_to_string(token_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() >= 32)
        .unwrap_or_else(new_token)
}

/// Replace the pairing token. Existing links stop working.
pub fn rotate_token() -> String {
    let _ = std::fs::remove_file(token_path());
    token()
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))
        .is_ok();
    if !ok {
        // Fallback: mix time and addresses (only reached without /dev/urandom).
        let seed = format!("{:?}{:p}", std::time::SystemTime::now(), &bytes);
        let h = sha1_smol::Sha1::from(seed).digest().bytes();
        bytes[..20].copy_from_slice(&h);
    }
    let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let _ = std::fs::create_dir_all(crate::store::data_dir());
    let _ = std::fs::write(token_path(), &t);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(token_path(), std::fs::Permissions::from_mode(0o600));
    }
    t
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A way to open the web client, e.g. ("Tailscale", "http://100.64.0.3:7788/#t=…").
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub label: &'static str,
    pub url: String,
    /// Only reachable from this machine.
    pub local: bool,
}

pub struct WebServer {
    pub hub: Arc<Hub>,
    pub port: u16,
    pub network: bool,
    stop: Arc<AtomicBool>,
    accept: Option<std::thread::JoinHandle<()>>,
    tunnel: Arc<Mutex<Tunnel>>,
}

#[derive(Default)]
struct Tunnel {
    child: Option<Child>,
    url: Option<String>,
    error: Option<String>,
}

impl WebServer {
    /// Bind and start serving. `network` listens on all interfaces.
    pub fn start(store: Store, port: u16, network: bool, tunnel: bool) -> Result<Self> {
        Self::start_with(Hub::new(store), port, network, tunnel)
    }

    /// Serve an existing hub, so agent threads and terminals survive a
    /// restart (e.g. when the reach or port setting changes).
    pub fn start_with(hub: Arc<Hub>, port: u16, network: bool, tunnel: bool) -> Result<Self> {
        let ip = if network {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        };
        let listener = TcpListener::bind(SocketAddr::from((ip, port)))
            .with_context(|| format!("port {port} is in use (is another InsyDE serving?)"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (h, st) = (hub.clone(), stop.clone());
        let accept = std::thread::Builder::new()
            .name("web-accept".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if st.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = conn else { continue };
                    let h = h.clone();
                    let _ = std::thread::Builder::new()
                        .name("web-conn".into())
                        .spawn(move || serve_conn(h, stream));
                }
            })?;
        let server = Self {
            hub,
            port,
            network,
            stop,
            accept: Some(accept),
            tunnel: Arc::new(Mutex::new(Tunnel::default())),
        };
        if tunnel {
            server.start_tunnel();
        }
        tracing::info!(port, network, "web access on");
        Ok(server)
    }

    pub fn clients(&self) -> usize {
        self.hub.client_count()
    }

    /// Every address the client can be opened at, most private first.
    pub fn links(&self) -> Vec<Link> {
        let t = token();
        let url = |host: &str| format!("http://{host}:{}/#t={t}", self.port);
        let mut v = vec![Link {
            label: "This computer",
            url: url("127.0.0.1"),
            local: true,
        }];
        if self.network {
            if let Some(ip) = tailscale_ip() {
                v.push(Link {
                    label: "Tailscale",
                    url: url(&ip),
                    local: false,
                });
            }
            if let Some(ip) = lan_ip() {
                v.push(Link {
                    label: "Local network",
                    url: url(&ip),
                    local: false,
                });
            }
        }
        if let Some(u) = &self.tunnel.lock().url {
            v.push(Link {
                label: "Public link",
                url: format!("{u}/#t={t}"),
                local: false,
            });
        }
        v
    }

    /// Why the public link isn't available, if it was requested and failed.
    pub fn tunnel_error(&self) -> Option<String> {
        self.tunnel.lock().error.clone()
    }

    pub fn tunnel_pending(&self) -> bool {
        let t = self.tunnel.lock();
        t.child.is_some() && t.url.is_none() && t.error.is_none()
    }

    fn start_tunnel(&self) {
        let Some(bin) = crate::agents::which("cloudflared") else {
            self.tunnel.lock().error =
                Some("Install cloudflared for a public link (brew install cloudflared).".into());
            return;
        };
        let child = Command::new(bin)
            .args(["tunnel", "--no-autoupdate", "--url"])
            .arg(format!("http://127.0.0.1:{}", self.port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                self.tunnel.lock().error = Some(format!("cloudflared: {e}"));
                return;
            }
        };
        let stderr = child.stderr.take();
        self.tunnel.lock().child = Some(child);
        let tunnel = self.tunnel.clone();
        std::thread::spawn(move || {
            let Some(err) = stderr else { return };
            let re = regex::Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com").expect("regex");
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(m) = re.find(&line) {
                    tunnel.lock().url = Some(m.as_str().to_string());
                }
            }
            let mut t = tunnel.lock();
            if t.url.is_none() {
                t.error = Some("cloudflared exited before giving a link.".into());
            }
            t.url = None;
        });
    }
}

impl Drop for WebServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.hub.disconnect_all();
        if let Some(mut c) = self.tunnel.lock().child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        // Wake the accept loop so it sees the stop flag, then wait for it to
        // drop the listener: a restart may bind the same port right away.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)),
            Duration::from_millis(200),
        );
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
    }
}

fn serve_conn(hub: Arc<Hub>, mut stream: TcpStream) {
    let Ok(req) = http::read_request(&mut stream) else {
        return;
    };
    if req.path == "/ws" && req.is_websocket() {
        if !req.param("t").is_some_and(|t| same(t, &token())) {
            std::thread::sleep(Duration::from_millis(400));
            let _ = http::respond(
                &mut stream,
                "401 Unauthorized",
                "text/plain",
                b"pairing required",
            );
            return;
        }
        if http::accept_websocket(&mut stream, &req).is_err() {
            return;
        }
        run_socket(hub, stream);
        return;
    }
    if req.path == "/auth" {
        // Lets the client tell "wrong pairing" from "server unreachable".
        if req.param("t").is_some_and(|t| same(t, &token())) {
            let _ = http::respond(&mut stream, "200 OK", "text/plain", b"ok");
        } else {
            std::thread::sleep(Duration::from_millis(400));
            let _ = http::respond(
                &mut stream,
                "401 Unauthorized",
                "text/plain",
                b"pairing required",
            );
        }
        return;
    }
    if req.method != "GET" && req.method != "HEAD" {
        let _ = http::respond(&mut stream, "405 Method Not Allowed", "text/plain", b"");
        return;
    }
    match assets::get(&req.path) {
        Some((ctype, body)) => {
            let _ = http::respond(&mut stream, "200 OK", ctype, &body);
        }
        None => {
            let _ = http::respond(&mut stream, "404 Not Found", "text/plain", b"not found");
        }
    }
}

fn run_socket(hub: Arc<Hub>, stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let Ok(killer) = stream.try_clone() else {
        return;
    };
    let (tx, rx) = flume::bounded::<Out>(8192);
    let client = hub.connect(tx.clone(), killer);
    let write_thread = std::thread::Builder::new()
        .name("web-write".into())
        .spawn(move || {
            for out in rx.iter() {
                let r = match out {
                    Out::Text(s) => http::write_frame(&mut writer, http::OP_TEXT, s.as_bytes()),
                    Out::Binary(b) => http::write_frame(&mut writer, http::OP_BINARY, &b),
                    Out::Pong(b) => http::write_frame(&mut writer, http::OP_PONG, &b),
                    Out::Close => {
                        let _ = http::write_frame(&mut writer, http::OP_CLOSE, &[]);
                        let _ = writer.shutdown(std::net::Shutdown::Both);
                        break;
                    }
                };
                if r.is_err() {
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                    break;
                }
            }
        });
    let mut reader = stream;
    loop {
        match http::read_message(&mut reader, MAX_MESSAGE) {
            Ok(Message::Text(text)) => handle_text(&hub, client, &tx, text),
            Ok(Message::Ping(p)) => {
                let _ = tx.try_send(Out::Pong(p));
            }
            Ok(Message::Pong | Message::Binary(_)) => {}
            Ok(Message::Close) | Err(_) => break,
        }
    }
    hub.disconnect(client);
    let _ = tx.try_send(Out::Close);
    drop(tx);
    if let Ok(t) = write_thread {
        let _ = t.join();
    }
}

fn handle_text(hub: &Arc<Hub>, client: u64, tx: &flume::Sender<Out>, text: String) {
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let method = v.get("m").and_then(Value::as_str).unwrap_or("").to_string();
    let params = v.get("p").cloned().unwrap_or(Value::Null);
    let run = {
        let (hub, tx) = (hub.clone(), tx.clone());
        move || {
            let reply = match hub.handle(client, &method, &params) {
                Ok(r) => json!({ "id": id, "ok": r }),
                Err(e) => json!({ "id": id, "err": format!("{e:#}") }),
            };
            if !id.is_null() {
                let _ = tx.try_send(Out::Text(reply.to_string()));
            }
        }
    };
    // Keystrokes and other memory-only calls stay in order on this thread;
    // anything that may touch git or disk runs beside it.
    if Hub::is_quick(v.get("m").and_then(Value::as_str).unwrap_or("")) {
        run();
    } else {
        let _ = std::thread::Builder::new()
            .name("web-call".into())
            .spawn(run);
    }
}

/// This machine's address on the local network (no packets are sent).
pub fn lan_ip() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.255.255:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// This machine's Tailscale IPv4 address, if Tailscale is running.
pub fn tailscale_ip() -> Option<String> {
    let bin = crate::agents::which("tailscale").or_else(|| {
        let p = std::path::PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
        p.exists().then_some(p)
    })?;
    let out = Command::new(bin).args(["ip", "-4"]).output().ok()?;
    let ip = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    (out.status.success() && ip.starts_with("100.")).then_some(ip)
}

/// A QR code for `text` as (width, dark modules row by row).
pub fn qr(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let w = code.width();
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Some((w, dark))
}

/// The QR code drawn with half blocks for a terminal (two rows per line).
pub fn qr_text(text: &str) -> String {
    let Some((w, dark)) = qr(text) else {
        return String::new();
    };
    let at = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < w
            && dark[y as usize * w + x as usize]
    };
    let mut s = String::new();
    let q = 2isize; // quiet zone
    let mut y = -q;
    while y < w as isize + q {
        for x in -q..w as isize + q {
            // Light background, dark modules: invert for dark terminals.
            s.push(match (at(x, y), at(x, y + 1)) {
                (false, false) => '█',
                (true, false) => '▄',
                (false, true) => '▀',
                (true, true) => ' ',
            });
        }
        s.push('\n');
        y += 2;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_and_qr() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
        let (w, dark) = qr("http://127.0.0.1:7788/#t=abc").unwrap();
        assert_eq!(dark.len(), w * w);
        assert!(qr_text("x").lines().count() > 5);
    }
}
