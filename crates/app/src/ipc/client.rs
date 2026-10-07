//! A client's end of the socket (spec 0005 "Messages"): connect, check the
//! greeting, then send [`ClientMessage`]s and receive [`ServerMessage`]s,
//! one JSON line each. Blocking, std only; the reconnecting TUI client and
//! the one-shot commands are built on it.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tidal_player_core::protocol::{ClientMessage, ServerMessage};

use super::codec::{DecodeError, GreetingError, LineBuffer, check_greeting, decode, encode};

/// How long a client waits for the greeting.
pub const GREETING_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a connection could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    Greeting(#[from] GreetingError),
    #[error("{}: {source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
    /// Closed, or silent past [`GREETING_TIMEOUT`], before a greeting.
    #[error("No greeting from {}", .0.display())]
    NoGreeting(PathBuf),
}

/// Why no message was received.
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// The player closed the connection.
    #[error("The player closed the connection")]
    Closed,
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A connection to a player, past its greeting.
#[derive(Debug)]
pub struct Connection {
    stream: UnixStream,
    lines: LineBuffer,
    /// Lines read but not returned yet.
    pending: VecDeque<Result<Vec<u8>, DecodeError>>,
}

impl Connection {
    /// Connects to the socket at `path` and checks the greeting.
    pub fn connect(path: &Path) -> Result<Self, ConnectError> {
        let stream = UnixStream::connect(path).map_err(|source| ConnectError::Io {
            path: path.to_owned(),
            source,
        })?;
        Self::handshake(stream, path)
    }

    /// Reads and checks the greeting on an open stream (`socket` names it
    /// in the messages).
    pub fn handshake(stream: UnixStream, socket: &Path) -> Result<Self, ConnectError> {
        let mut connection = Self {
            stream,
            lines: LineBuffer::new(),
            pending: VecDeque::new(),
        };
        let line = match connection.next_line(Some(GREETING_TIMEOUT)) {
            Ok(Some(Ok(line))) => line,
            Ok(Some(Err(_))) => return Err(GreetingError::NotOurs(socket.to_owned()).into()),
            Ok(None) | Err(RecvError::Closed) => {
                return Err(ConnectError::NoGreeting(socket.to_owned()));
            }
            Err(RecvError::Io(source)) => {
                return Err(ConnectError::Io {
                    path: socket.to_owned(),
                    source,
                });
            }
            Err(RecvError::Decode(_)) => {
                return Err(GreetingError::NotOurs(socket.to_owned()).into());
            }
        };
        check_greeting(&line, socket)?;
        Ok(connection)
    }

    /// Sends one message.
    pub fn send(&mut self, message: &ClientMessage) -> io::Result<()> {
        self.stream.write_all(&encode(message))
    }

    /// The next message, waiting at most `timeout` (`None`: no limit;
    /// zero: only what has arrived). `Ok(None)` when none came in time.
    pub fn recv(&mut self, timeout: Option<Duration>) -> Result<Option<ServerMessage>, RecvError> {
        match self.next_line(timeout)? {
            None => Ok(None),
            Some(line) => Ok(Some(decode(&line?)?)),
        }
    }

    /// Closes both directions (the player sees the client leave).
    pub fn close(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    /// The underlying stream (to clone it for a reader thread, say).
    pub fn stream(&self) -> &UnixStream {
        &self.stream
    }

    fn next_line(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Option<Result<Vec<u8>, DecodeError>>, RecvError> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut buf = [0u8; 64 * 1024];
        loop {
            if let Some(line) = self.pending.pop_front() {
                return Ok(Some(line));
            }
            let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            let read = match left {
                Some(left) if left.is_zero() => {
                    self.stream.set_nonblocking(true)?;
                    let read = self.stream.read(&mut buf);
                    self.stream.set_nonblocking(false)?;
                    read
                }
                left => {
                    self.stream.set_read_timeout(left)?;
                    self.stream.read(&mut buf)
                }
            };
            match read {
                Ok(0) => return Err(RecvError::Closed),
                Ok(n) => self.pending.extend(self.lines.push(&buf[..n])),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Ok(None);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {
                    return Err(RecvError::Closed);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}
