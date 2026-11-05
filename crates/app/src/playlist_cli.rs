//! `tidal-player playlist <command>` (spec 0023): playlists from a shell,
//! through the running player's library requests. Parsing and the flows
//! are pure over a [`Link`]; [`run`] finds the player.

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;

use crate::client::Link;

/// The `playlist` subcommands.
#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand)]
pub enum PlaylistCommand {
    /// List your playlists and the ones you follow.
    List {
        /// One JSON array instead of tab-separated lines.
        #[arg(long)]
        json: bool,
    },
    /// Create a playlist and print its UUID.
    New {
        name: String,
        #[arg(short, long)]
        description: Option<String>,
        /// Make it public.
        #[arg(long)]
        public: bool,
    },
    /// Change one of your playlists: what is given, nothing else.
    Edit {
        /// A UUID or a playlist link.
        playlist: String,
        #[arg(long)]
        name: Option<String>,
        /// The new description; "" clears it.
        #[arg(long)]
        description: Option<String>,
        #[arg(long, conflicts_with = "private")]
        public: bool,
        #[arg(long)]
        private: bool,
    },
    /// Delete one of your playlists.
    Delete {
        /// A UUID or a playlist link.
        playlist: String,
        /// Do not ask.
        #[arg(long)]
        yes: bool,
    },
    /// Copy a playlist's tracks into a new private one and print its UUID.
    Fork {
        /// A UUID or a playlist link.
        playlist: String,
        /// The new name: "<title> (copy)" by default.
        #[arg(long)]
        name: Option<String>,
    },
    /// Add a playlist's tracks to one of yours.
    Import {
        /// A UUID or a playlist link.
        from: String,
        /// A UUID or a playlist link.
        to: String,
        /// Add tracks the playlist already has too.
        #[arg(long)]
        duplicates: bool,
    },
}

/// A bad argument (exit 2, nothing sent).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    #[error("Not a playlist: {0}")]
    NotAPlaylist(String),
    #[error("A playlist name cannot be blank")]
    BlankName,
    #[error("Nothing to change: give --name, --description, --public or --private")]
    NothingToChange,
    #[error("FROM and TO are the same playlist")]
    SamePlaylist,
}

/// What a command does, its arguments checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    List {
        json: bool,
    },
    New {
        title: String,
        description: String,
        public: bool,
    },
    Edit {
        uuid: String,
        title: Option<String>,
        description: Option<String>,
        public: Option<bool>,
    },
    Delete {
        uuid: String,
        yes: bool,
    },
    Fork {
        uuid: String,
        name: Option<String>,
    },
    Import {
        from: String,
        to: String,
        duplicates: bool,
    },
}

/// The plan of `command`.
pub fn plan(_command: &PlaylistCommand) -> Result<Plan, UsageError> {
    Err(UsageError::BlankName)
}

/// Runs `plan` over `link`; `terminal` answers `delete`'s question (`None`:
/// no terminal). The exit code.
pub fn execute<L: Link>(
    _link: &mut L,
    _plan: &Plan,
    _timeout: Duration,
    _out: &mut dyn Write,
    _err: &mut dyn Write,
    _terminal: Option<&mut dyn BufRead>,
) -> u8 {
    1
}

/// `tidal-player playlist <command>`.
pub fn run(_command: &PlaylistCommand) -> ExitCode {
    ExitCode::from(1)
}

#[cfg(test)]
mod tests;
