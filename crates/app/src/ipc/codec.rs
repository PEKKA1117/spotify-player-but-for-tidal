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
    let _ = message;
    Vec::new()
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
        let _ = (&mut bytes, self.discarding, self.limit, &self.buf);
        Vec::new()
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
    let _ = Greeting {
        tidal_player: VERSION.to_owned(),
    };
    Vec::new()
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
    let _ = (line, socket);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidal_player_core::protocol::{ClientMessage, Command, Event, ServerMessage};

    fn subscribe_line() -> Vec<u8> {
        b"\"Subscribe\"\n".to_vec()
    }

    fn request_line(id: u64) -> Vec<u8> {
        let mut line =
            serde_json::to_vec(&serde_json::json!({ "Request": { "id": id, "command": "Next" } }))
                .unwrap();
        line.push(b'\n');
        line
    }

    fn request(id: u64) -> ClientMessage {
        ClientMessage::Request {
            id,
            command: Command::Next,
        }
    }

    /// What one row expects of a decoded message.
    #[derive(Debug)]
    enum Want {
        Message(ClientMessage),
        /// An error whose text contains this.
        Error(&'static str),
    }

    fn check(rows: Vec<(&str, Vec<Vec<u8>>, Vec<Want>)>, limit: usize) {
        for (name, reads, want) in rows {
            let mut decoder = Decoder::<ClientMessage>::with_limit(limit);
            let got: Vec<_> = reads.iter().flat_map(|r| decoder.feed(r)).collect();
            assert_eq!(got.len(), want.len(), "{name}: {got:?}");
            for (got, want) in got.iter().zip(&want) {
                match (got, want) {
                    (Ok(m), Want::Message(w)) => assert_eq!(m, w, "{name}"),
                    (Err(e), Want::Error(text)) => {
                        assert!(e.to_string().contains(text), "{name}: {e}")
                    }
                    _ => panic!("{name}: got {got:?}, want {want:?}"),
                }
            }
        }
    }

    /// AC2: `encode` writes one line; the decoder assembles split
    /// messages, decodes several per read, refuses invalid JSON, unknown
    /// variants and long lines, naming the problem.
    #[test]
    fn ac2_framing() {
        // `encode`: one JSON line, which decodes back.
        let message = ServerMessage::Event(Event::ShuttingDown);
        let line = encode(&message);
        assert_eq!(line.last(), Some(&b'\n'), "{line:?}");
        assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
        let mut decoder = Decoder::<ServerMessage>::new();
        assert_eq!(decoder.feed(&line), vec![Ok(message)]);

        let both = [subscribe_line(), request_line(7)].concat();
        let rows: Vec<(&str, Vec<Vec<u8>>, Vec<Want>)> = vec![
            (
                "one per read",
                vec![subscribe_line(), request_line(1)],
                vec![
                    Want::Message(ClientMessage::Subscribe),
                    Want::Message(request(1)),
                ],
            ),
            (
                "split across reads",
                {
                    let line = request_line(2);
                    line.chunks(3).map(<[u8]>::to_vec).collect()
                },
                vec![Want::Message(request(2))],
            ),
            (
                "several in one read",
                vec![[both.clone(), request_line(8)].concat()],
                vec![
                    Want::Message(ClientMessage::Subscribe),
                    Want::Message(request(7)),
                    Want::Message(request(8)),
                ],
            ),
            (
                "a split right after a newline",
                vec![both[..12].to_vec(), both[12..].to_vec()],
                vec![
                    Want::Message(ClientMessage::Subscribe),
                    Want::Message(request(7)),
                ],
            ),
            (
                "unfinished line: nothing yet",
                vec![request_line(3)[..10].to_vec()],
                vec![],
            ),
            (
                "invalid JSON",
                vec![b"{\"Request\": \n".to_vec()],
                vec![Want::Error("invalid message: EOF while parsing")],
            ),
            (
                "unknown variant",
                vec![b"\"Unsubscribe\"\n".to_vec()],
                vec![Want::Error("unknown variant `Unsubscribe`")],
            ),
            (
                "unknown command",
                vec![b"{\"Request\":{\"id\":1,\"command\":\"Explode\"}}\n".to_vec()],
                vec![Want::Error("unknown variant `Explode`")],
            ),
            (
                "an invalid line, then a good one",
                vec![b"nope\n".to_vec(), subscribe_line()],
                vec![
                    Want::Error("invalid message"),
                    Want::Message(ClientMessage::Subscribe),
                ],
            ),
            (
                "a line at the limit",
                vec![format!("{:<64}\n", "\"Subscribe\"").into_bytes()],
                vec![Want::Message(ClientMessage::Subscribe)],
            ),
            (
                "a line over the limit, in one read",
                vec![format!("{:<65}\n", "\"Subscribe\"").into_bytes()],
                vec![Want::Error("message longer than 64 bytes")],
            ),
            (
                "a line over the limit, across reads, then a good one",
                vec![
                    vec![b' '; 40],
                    vec![b' '; 40],
                    vec![b' '; 40],
                    [b"\n".to_vec(), subscribe_line()].concat(),
                ],
                vec![
                    Want::Error("message longer than 64 bytes"),
                    Want::Message(ClientMessage::Subscribe),
                ],
            ),
        ];
        check(rows, 64);

        // The real limit: a 17 MiB line is refused once it passes 16 MiB,
        // and never held whole.
        let mut decoder = Decoder::<ClientMessage>::new();
        let chunk = vec![b'x'; 1024 * 1024];
        let mut errors = Vec::new();
        for _ in 0..17 {
            errors.extend(decoder.feed(&chunk));
            assert!(decoder.buffered() <= MAX_LINE, "{}", decoder.buffered());
        }
        assert_eq!(errors, vec![Err(DecodeError::TooLong { limit: MAX_LINE })]);
        assert_eq!(decoder.buffered(), 0);
        assert_eq!(
            decoder.feed(&[b"\n".to_vec(), subscribe_line()].concat()),
            vec![Ok(ClientMessage::Subscribe)]
        );
    }

    /// AC3: the greeting's exact text; `check_greeting` accepts this
    /// version, names both versions on a mismatch, refuses anything else.
    #[test]
    fn ac3_greeting() {
        assert_eq!(
            String::from_utf8(greeting()).unwrap(),
            format!("{{\"tidal_player\":\"{}\"}}\n", env!("CARGO_PKG_VERSION"))
        );
        let socket = Path::new("/run/user/1000/tidal-player/player.sock");
        let mismatch = |theirs: &str| {
            Err(GreetingError::Mismatch {
                theirs: theirs.into(),
                ours: VERSION.into(),
            })
        };
        let not_ours = Err(GreetingError::NotOurs(socket.into()));
        let ours = format!("{{\"tidal_player\":\"{VERSION}\"}}");
        let rows: Vec<(&str, String, Result<(), GreetingError>)> = vec![
            ("same version", ours.clone(), Ok(())),
            (
                "with spaces",
                format!("{{ \"tidal_player\" : \"{VERSION}\" }}"),
                Ok(()),
            ),
            (
                "older",
                "{\"tidal_player\":\"0.0.9\"}".into(),
                mismatch("0.0.9"),
            ),
            (
                "newer",
                "{\"tidal_player\":\"9.1.0-rc.1\"}".into(),
                mismatch("9.1.0-rc.1"),
            ),
            ("empty", String::new(), not_ours.clone()),
            ("not JSON", "SSH-2.0-OpenSSH_9.6".into(), not_ours.clone()),
            (
                "another key",
                "{\"spotify_player\":\"0.1.0\"}".into(),
                not_ours.clone(),
            ),
            ("a number", "{\"tidal_player\":1}".into(), not_ours.clone()),
            ("a message first", "\"Subscribe\"".into(), not_ours.clone()),
        ];
        for (name, line, want) in rows {
            assert_eq!(check_greeting(line.as_bytes(), socket), want, "{name}");
        }
        assert_eq!(
            mismatch("0.0.9").unwrap_err().to_string(),
            format!(
                "The running player is tidal-player 0.0.9, this is {VERSION}: restart it \
                 (\"tidal-player daemon stop\", or \"systemctl --user restart tidal-player\")"
            )
        );
        assert_eq!(
            not_ours.unwrap_err().to_string(),
            "Not a tidal-player socket: /run/user/1000/tidal-player/player.sock"
        );
    }
}
