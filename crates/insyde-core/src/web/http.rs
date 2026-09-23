//! The small slice of HTTP/1.1 and WebSocket (RFC 6455) the web client needs:
//! GET for embedded static files, and one upgraded socket per browser tab.
//! Hand-rolled on std sockets so reads and writes can live on separate
//! threads (a cloned `TcpStream` each) without an async runtime.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// A parsed request head.
#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// A query parameter (no percent-decoding needed for our tokens/ids).
    pub fn param(&self, key: &str) -> Option<&str> {
        self.query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }

    pub fn is_websocket(&self) -> bool {
        self.header("upgrade")
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
    }
}

const MAX_HEAD: usize = 16 * 1024;

/// Read one request head (the body is ignored: we only serve GETs).
pub fn read_request(s: &mut TcpStream) -> io::Result<Request> {
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > MAX_HEAD {
            return Err(io::Error::other("request head too large"));
        }
    }
    s.set_read_timeout(None)?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Ok(Request {
        method,
        path: path.to_string(),
        query: query.to_string(),
        headers,
    })
}

/// The page may only load its own scripts and talk to its own server.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: blob:; connect-src 'self' ws: wss:; frame-ancestors 'none'; base-uri 'none'";

pub fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> io::Result<()> {
    let csp = if ctype.starts_with("text/html") {
        format!("Content-Security-Policy: {CSP}\r\n")
    } else {
        String::new()
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n{csp}Connection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(body)?;
    s.flush()
}

/// Complete the WebSocket upgrade for `req`.
pub fn accept_websocket(s: &mut TcpStream, req: &Request) -> io::Result<()> {
    let key = req
        .header("sec-websocket-key")
        .ok_or_else(|| io::Error::other("missing Sec-WebSocket-Key"))?;
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(key)
    );
    s.write_all(head.as_bytes())?;
    s.flush()
}

pub fn accept_key(key: &str) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(key.trim().as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64(&h.digest().bytes())
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// A complete (reassembled) WebSocket message.
#[derive(Debug, PartialEq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close,
}

pub const OP_TEXT: u8 = 1;
pub const OP_BINARY: u8 = 2;
pub const OP_CLOSE: u8 = 8;
pub const OP_PING: u8 = 9;
pub const OP_PONG: u8 = 10;

/// Read one message, joining continuation frames. Client frames must be masked.
pub fn read_message(r: &mut impl Read, max: usize) -> io::Result<Message> {
    let mut data = Vec::new();
    let mut kind = 0u8;
    loop {
        let mut h = [0u8; 2];
        r.read_exact(&mut h)?;
        let fin = h[0] & 0x80 != 0;
        let op = h[0] & 0x0f;
        let masked = h[1] & 0x80 != 0;
        let mut len = (h[1] & 0x7f) as u64;
        if len == 126 {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            len = u16::from_be_bytes(b) as u64;
        } else if len == 127 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            len = u64::from_be_bytes(b);
        }
        if !masked {
            return Err(io::Error::other("unmasked client frame"));
        }
        if data.len() as u64 + len > max as u64 {
            return Err(io::Error::other("message too large"));
        }
        let mut mask = [0u8; 4];
        r.read_exact(&mut mask)?;
        let start = data.len();
        data.resize(start + len as usize, 0);
        r.read_exact(&mut data[start..])?;
        for (i, b) in data[start..].iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
        match op {
            OP_CLOSE => return Ok(Message::Close),
            OP_PING => return Ok(Message::Ping(data.split_off(start))),
            OP_PONG => {
                data.truncate(start);
                if kind == 0 {
                    return Ok(Message::Pong);
                }
                continue;
            }
            0 => {}
            op => kind = op,
        }
        if fin {
            return match kind {
                OP_TEXT => String::from_utf8(data)
                    .map(Message::Text)
                    .map_err(|_| io::Error::other("invalid utf-8")),
                OP_BINARY => Ok(Message::Binary(data)),
                _ => Err(io::Error::other("bad opcode")),
            };
        }
    }
}

/// Write one unmasked server frame.
pub fn write_frame(w: &mut impl Write, op: u8, payload: &[u8]) -> io::Result<()> {
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | op);
    match payload.len() {
        n if n < 126 => head.push(n as u8),
        n if n <= u16::MAX as usize => {
            head.push(126);
            head.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            head.push(127);
            head.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    w.write_all(&head)?;
    w.write_all(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_and_frames() {
        // RFC 6455 section 1.3 example.
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert_eq!(base64(b"ab"), "YWI=");
        // A masked "Hello" text frame, split into two fragments.
        let mask = [0x37, 0xfa, 0x21, 0x3d];
        let enc = |s: &[u8]| {
            s.iter()
                .enumerate()
                .map(|(i, b)| b ^ mask[i % 4])
                .collect::<Vec<_>>()
        };
        let mut wire = vec![0x01, 0x83];
        wire.extend(mask);
        wire.extend(enc(b"Hel"));
        wire.extend([0x80, 0x82]);
        wire.extend(mask);
        wire.extend(enc(b"lo"));
        let msg = read_message(&mut &wire[..], 1024).unwrap();
        assert_eq!(msg, Message::Text("Hello".into()));
        let mut out = Vec::new();
        write_frame(&mut out, OP_TEXT, &[0u8; 300]).unwrap();
        assert_eq!(&out[..4], &[0x81, 126, 1, 44]);
    }
}
