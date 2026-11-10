//! The socket's framing (spec 0005 "Messages"): newline-delimited JSON, one
//! message per line, at most [`MAX_LINE`] bytes per line, and the greeting
//! the player sends first. Pure over byte buffers: the sockets are in
//! [`super::server`] and [`super::client`].

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The longest line accepted, newline excluded (16 MiB).
pub const MAX_LINE: usize = 16 * 1024 * 1024;

/// This build's version, as the greeting carries it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Why a line is not a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The line is longer than the limit (in bytes); it was not buffered.
    #[error("message longer than {limit} bytes")]
    TooLong { limit: usize },
    /// Not JSON, or not one of the messages (serde's words: `unknown
    /// variant`, `expected value`, …).
    #[error("invalid message: {0}")]
    Invalid(String),
}

/// `message` as one JSON line, newline included.
pub fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    // The protocol's types have string keys only: serialising cannot fail.
    let mut line = serde_json::to_vec(message).expect("a protocol message serialises");
    line.push(b'\n');
    line
}

/// Decodes one line (without its newline).
pub fn decode<T: DeserializeOwned>(line: &[u8]) -> Result<T, DecodeError> {
    serde_json::from_slice(line).map_err(|e| DecodeError::Invalid(e.to_string()))
}

/// Splits a byte stream into lines, holding at most `limit` bytes of an
/// unfinished one: a longer line is refused as soon as it is too long and
/// the rest of it is skipped, never buffered.
#[derive(Debug)]
pub struct LineBuffer {
    buf: Vec<u8>,
    limit: usize,
    /// Inside a line that was refused: skip to its newline.
    discarding: bool,
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::with_limit(MAX_LINE)
    }
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// A buffer refusing lines longer than `limit` bytes.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            buf: Vec::new(),
            limit,
            discarding: false,
        }
    }

    /// The bytes of the unfinished line held now.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Takes the next bytes read; returns every line they finished, in
    /// order (a refused one as its error).
    pub fn push(&mut self, mut bytes: &[u8]) -> Vec<Result<Vec<u8>, DecodeError>> {
        let mut lines = Vec::new();
        let too_long = DecodeError::TooLong { limit: self.limit };
        while !bytes.is_empty() {
            let end = bytes.iter().position(|b| *b == b'\n');
            let part = &bytes[..end.unwrap_or(bytes.len())];
            if !self.discarding {
                if self.buf.len() + part.len() > self.limit {
                    lines.push(Err(too_long.clone()));
                    self.buf = Vec::new();
                    self.discarding = true;
                } else {
                    self.buf.extend_from_slice(part);
                }
            }
            let Some(end) = end else {
                break;
            };
            if !std::mem::take(&mut self.discarding) {
                lines.push(Ok(std::mem::take(&mut self.buf)));
            }
            bytes = &bytes[end + 1..];
        }
        lines
    }
}

/// [`LineBuffer`] plus [`decode`]: the messages a stream of bytes carries.
#[derive(Debug)]
pub struct Decoder<T> {
    lines: LineBuffer,
    _message: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Default for Decoder<T> {
    fn default() -> Self {
        Self::with_limit(MAX_LINE)
    }
}

impl<T: DeserializeOwned> Decoder<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            lines: LineBuffer::with_limit(limit),
            _message: PhantomData,
        }
    }

    /// The bytes of the unfinished line held now.
    pub fn buffered(&self) -> usize {
        self.lines.buffered()
    }

    /// Takes the next bytes read; returns every message they finished.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Result<T, DecodeError>> {
        self.lines
            .push(bytes)
            .into_iter()
            .map(|line| line.and_then(|line| decode(&line)))
            .collect()
    }
}

/// The greeting's frozen shape: `{"tidal_player":"<version>"}`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Greeting {
    tidal_player: String,
}

/// The player's first line, newline included.
pub fn greeting() -> Vec<u8> {
    encode(&Greeting {
        tidal_player: VERSION.to_owned(),
    })
}

/// Why the first line from a socket is not a player this client can use.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GreetingError {
    #[error(
        "The running player is tidal-player {theirs}, this is {ours}: restart it \
         (\"tidal-player daemon stop\", or \"systemctl --user restart tidal-player\")"
    )]
    Mismatch { theirs: String, ours: String },
    #[error("Not a tidal-player socket: {}", .0.display())]
    NotOurs(PathBuf),
}

/// Checks the first line read from `socket` (without its newline).
pub fn check_greeting(line: &[u8], socket: &Path) -> Result<(), GreetingError> {
    match serde_json::from_slice::<Greeting>(line) {
        Ok(Greeting { tidal_player }) if tidal_player == VERSION => Ok(()),
        Ok(Greeting { tidal_player }) => Err(GreetingError::Mismatch {
            theirs: tidal_player,
            ours: VERSION.to_owned(),
        }),
        Err(_) => Err(GreetingError::NotOurs(socket.to_owned())),
    }
}

#[cfg(test)]
mod tests;
