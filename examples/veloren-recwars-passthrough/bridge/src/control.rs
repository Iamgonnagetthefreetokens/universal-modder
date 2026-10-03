//! The control channel (docs/PROTOCOL.md §3): TCP on loopback, line-delimited ASCII.
//!
//! Small, ordered, human-readable, and easy to watch (`nc 127.0.0.1 47811`). Bulk data goes through
//! regions; this channel only carries camera, terrain revisions, input, focus and liveness.
//!
//! Both directions are non-blocking on purpose: a bridge must never stall a game frame, so a socket
//! that cannot take a line right now drops the line and counts it.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};

/// A parsed control message: a type plus `key=value` fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    /// Message type (`HELLO`, `CAM`, `IN`, …).
    pub kind: String,
    /// Fields in arrival order.
    pub fields: Vec<(String, String)>,
}

impl Msg {
    /// An empty message of the given type.
    pub fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            fields: Vec::new(),
        }
    }

    /// Add a field.
    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.fields.push((key.to_string(), value.to_string()));
        self
    }

    /// Field value, or `""`.
    pub fn get(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }

    /// Field as `f32` (a missing or unparsable field is `0.0`, so a broken sender cannot panic us).
    pub fn get_f32(&self, key: &str) -> f32 {
        self.get(key).parse().unwrap_or(0.0)
    }

    /// Field as `i64`.
    pub fn get_i64(&self, key: &str) -> i64 {
        self.get(key).parse().unwrap_or(0)
    }

    /// Field as `bool` (`1`/`true`/`yes`/`on`).
    pub fn get_bool(&self, key: &str) -> bool {
        matches!(self.get(key), "1" | "true" | "yes" | "on")
    }

    /// Serialise as one line, `\n`-terminated.
    pub fn encode(&self) -> String {
        let mut out = String::with_capacity(48);
        out.push_str(&self.kind);
        for (k, v) in &self.fields {
            out.push(' ');
            out.push_str(k);
            out.push('=');
            out.push_str(v);
        }
        out.push('\n');
        out
    }

    /// Parse one line. Unknown keys are kept (forward compatibility), unknown types are kept too:
    /// the caller decides what to ignore.
    pub fn parse(line: &str) -> Option<Self> {
        let mut parts = line.trim().split(' ');
        let kind = parts.next()?.to_string();
        if kind.is_empty() {
            return None;
        }
        let mut fields = Vec::new();
        for part in parts {
            if let Some((k, v)) = part.split_once('=') {
                fields.push((k.to_string(), v.to_string()));
            }
        }
        Some(Self { kind, fields })
    }
}

/// A non-blocking control peer.
#[derive(Debug)]
pub struct Peer {
    stream: TcpStream,
    buf: Vec<u8>,
    closed: bool,
    /// Lines written since the peer was created.
    pub sent: u64,
    /// Lines parsed since the peer was created.
    pub received: u64,
}

impl Peer {
    /// Wrap an accepted or connected stream.
    pub fn new(stream: TcpStream) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            buf: Vec::with_capacity(4096),
            closed: false,
            sent: 0,
            received: 0,
        })
    }

    /// Connect to a peer (the host does this; retry a few times at startup).
    pub fn connect(addr: SocketAddr, timeout_ms: u64) -> std::io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(timeout_ms))?;
        Self::new(stream)
    }

    /// True once the peer is gone (or a send failed hard).
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Send one message. Returns `false` if it was dropped: the socket buffer is full or the peer is
    /// gone. Dropping a control line is always safe — the state it described is republished.
    pub fn send(&self, kind: &str, fields: &[(&str, &str)]) -> bool {
        if self.closed {
            return false;
        }
        let mut msg = Msg::new(kind);
        for (k, v) in fields {
            msg.fields.push((k.to_string(), v.to_string()));
        }
        let line = msg.encode();
        let mut stream = &self.stream;
        match stream.write_all(line.as_bytes()) {
            Ok(()) => true,
            Err(e) if e.kind() == ErrorKind::WouldBlock => false,
            Err(_) => false,
        }
    }

    /// Read everything available and return the parsed messages. Never blocks.
    pub fn pump(&mut self) -> Vec<Msg> {
        if self.closed {
            return Vec::new();
        }
        let mut chunk = [0u8; 8192];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    self.closed = true;
                    break;
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.closed = true;
                    break;
                }
            }
        }
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]);
            if let Some(msg) = Msg::parse(&text) {
                self.received += 1;
                out.push(msg);
            }
        }
        out
    }

    /// The address of the peer.
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.stream.peer_addr()
    }
}

/// The guest's listening socket. Only loopback peers are accepted (docs/PROTOCOL.md §5).
#[derive(Debug)]
pub struct Listener {
    listener: TcpListener,
    /// Connections refused because they did not come from loopback.
    pub refused: u64,
}

impl Listener {
    /// Bind `127.0.0.1:port`.
    pub fn bind(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            refused: 0,
        })
    }

    /// Accept a loopback peer if one is waiting.
    pub fn try_accept(&mut self) -> std::io::Result<Option<Peer>> {
        match self.listener.accept() {
            Ok((stream, addr)) => {
                if !addr.ip().is_loopback() {
                    // Someone off-machine tried to feed the game: refuse, count it, keep serving.
                    drop(stream);
                    self.refused += 1;
                    return Err(std::io::Error::new(
                        ErrorKind::PermissionDenied,
                        "refused a non-loopback bridge connection",
                    ));
                }
                Ok(Some(Peer::new(stream)?))
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The port bound.
    pub fn port(&self) -> std::io::Result<u16> {
        Ok(self.listener.local_addr()?.port())
    }
}
