//! Local control API: newline-delimited JSON-RPC 2.0 over a Unix socket.
//!
//! The app serves it; the `insy` CLI (and agents running inside InsyDE
//! terminals) are clients. The socket is created with 0600 permissions in the
//! user's data directory, so only the same user can connect. Requests are
//! forwarded to the UI thread, which owns all state, through a channel.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn err(id: Value, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcError {
                code: -32000,
                message: message.into(),
            }),
        }
    }
}

/// A request plus where to send its response (the handler may answer later).
pub struct Call {
    pub request: Request,
    pub reply: flume::Sender<Response>,
}

impl Call {
    pub fn ok(self, v: Value) {
        let _ = self.reply.send(Response::ok(self.request.id, v));
    }
    pub fn err(self, m: impl Into<String>) {
        let _ = self.reply.send(Response::err(self.request.id, m));
    }
    pub fn param_str(&self, key: &str) -> Option<String> {
        self.request
            .params
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
    pub fn param_u64(&self, key: &str) -> Option<u64> {
        self.request.params.get(key).and_then(|v| v.as_u64())
    }
}

pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("INSYDE_SOCKET") {
        return PathBuf::from(p);
    }
    crate::store::data_dir().join("insyde.sock")
}

/// Start serving on `path`. Each accepted request is sent to `calls`.
#[cfg(unix)]
pub fn serve(path: &Path, calls: flume::Sender<Call>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    // A stale socket from a crashed run blocks bind; remove it if nothing answers.
    if path.exists() && std::os::unix::net::UnixStream::connect(path).is_err() {
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new()
        .name("rpc-accept".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let calls = calls.clone();
                let _ = std::thread::Builder::new()
                    .name("rpc-conn".into())
                    .spawn(move || {
                        let mut writer = match stream.try_clone() {
                            Ok(w) => w,
                            Err(_) => return,
                        };
                        for line in BufReader::new(stream).lines() {
                            let Ok(line) = line else { break };
                            if line.trim().is_empty() {
                                continue;
                            }
                            let resp = match serde_json::from_str::<Request>(&line) {
                                Ok(request) => {
                                    let (tx, rx) = flume::bounded(1);
                                    if calls.send(Call { request, reply: tx }).is_err() {
                                        break;
                                    }
                                    rx.recv().unwrap_or_else(|_| {
                                        Response::err(Value::Null, "app is shutting down")
                                    })
                                }
                                Err(e) => {
                                    Response::err(Value::Null, format!("invalid request: {e}"))
                                }
                            };
                            let Ok(mut s) = serde_json::to_string(&resp) else {
                                break;
                            };
                            s.push('\n');
                            if writer.write_all(s.as_bytes()).is_err() {
                                break;
                            }
                        }
                    });
            }
        })?;
    Ok(())
}

/// Blocking client: one request, one response.
#[cfg(unix)]
pub fn call(method: &str, params: Value) -> Result<Value> {
    let path = socket_path();
    let mut stream = std::os::unix::net::UnixStream::connect(&path)
        .with_context(|| format!("InsyDE isn't running (no socket at {})", path.display()))?;
    let req = Request {
        jsonrpc: "2.0".into(),
        id: Value::from(1),
        method: method.into(),
        params,
    };
    let mut s = serde_json::to_string(&req)?;
    s.push('\n');
    stream.write_all(s.as_bytes())?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let resp: Response = serde_json::from_str(&line).context("bad response")?;
    match (resp.result, resp.error) {
        (Some(v), _) => Ok(v),
        (_, Some(e)) => Err(anyhow!(e.message)),
        _ => Ok(Value::Null),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("insyde-rpc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sock");
        let (tx, rx) = flume::unbounded::<Call>();
        serve(&path, tx).unwrap();
        std::thread::spawn(move || {
            while let Ok(c) = rx.recv() {
                let m = c.request.method.clone();
                c.ok(serde_json::json!({ "echo": m }));
            }
        });
        // SAFETY: test-only, single-threaded use of the env var before any reader.
        unsafe { std::env::set_var("INSYDE_SOCKET", &path) };
        let v = call("ping", Value::Null).unwrap();
        assert_eq!(v["echo"], "ping");
    }
}
