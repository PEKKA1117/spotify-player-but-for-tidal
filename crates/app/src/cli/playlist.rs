//! `tidal-player playlist <command>` (spec 0023): playlists from a shell,
//! through the running player's library requests. Parsing and the flows
//! are pure over a [`Link`]; [`run`] finds the player.

use std::collections::HashSet;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tidal_player_core::item::parse_item;
use tidal_player_core::library::{
    LibraryRequest, LibraryResponse, ListItems, ListRef, PageData, PageRequest, PlaylistSummary,
};
use tidal_player_core::protocol::{ClientMessage, ServerMessage};
use tidal_player_core::ui::group;
use tidal_player_core::{Item, TrackId};

use crate::cli::oneshot::{NO_ANSWER, REPLY_TIMEOUT};
use crate::client::{Link, find};

/// The largest page of the playlists list and of a playlist's tracks.
const PLAYLISTS_PAGE: u32 = 50;
const TRACKS_PAGE: u32 = 100;
/// Tracks per `AddToPlaylist`: one request to Tidal, well within
/// [`REPLY_TIMEOUT`] (spec 0023 decision 4).
const ADD_BATCH: usize = 100;
/// What `delete` says with no terminal to ask on and no `--yes`.
const NEEDS_YES: &str = "Use --yes to delete without a terminal";
const UNEXPECTED: &str = "The player gave an unexpected answer";

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

/// A UUID as `list` prints it: 8-4-4-4-12 hexadecimal digits.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The playlist `text` names: a bare UUID, which items do not take, or a
/// link.
fn playlist_id(text: &str) -> Result<String, UsageError> {
    let text = text.trim();
    if is_uuid(text) {
        return Ok(text.to_ascii_lowercase());
    }
    match parse_item(text) {
        Ok(Item::Playlist(uuid)) => Ok(uuid.to_ascii_lowercase()),
        _ => Err(UsageError::NotAPlaylist(text.to_owned())),
    }
}

/// `name`, trimmed: a blank one is refused.
fn title(name: &str) -> Result<String, UsageError> {
    match name.trim() {
        "" => Err(UsageError::BlankName),
        name => Ok(name.to_owned()),
    }
}

/// The plan of `command`.
pub fn plan(command: &PlaylistCommand) -> Result<Plan, UsageError> {
    use PlaylistCommand as C;
    Ok(match command {
        C::List { json } => Plan::List { json: *json },
        C::New {
            name,
            description,
            public,
        } => Plan::New {
            title: title(name)?,
            description: description.as_deref().unwrap_or_default().trim().to_owned(),
            public: *public,
        },
        C::Edit {
            playlist,
            name,
            description,
            public,
            private,
        } => {
            let uuid = playlist_id(playlist)?;
            let title = name.as_deref().map(title).transpose()?;
            let description = description.as_deref().map(|d| d.trim().to_owned());
            let public = (*public || *private).then_some(*public);
            if title.is_none() && description.is_none() && public.is_none() {
                return Err(UsageError::NothingToChange);
            }
            Plan::Edit {
                uuid,
                title,
                description,
                public,
            }
        }
        C::Delete { playlist, yes } => Plan::Delete {
            uuid: playlist_id(playlist)?,
            yes: *yes,
        },
        C::Fork { playlist, name } => Plan::Fork {
            uuid: playlist_id(playlist)?,
            name: name.as_deref().map(title).transpose()?,
        },
        C::Import {
            from,
            to,
            duplicates,
        } => {
            let (from, to) = (playlist_id(from)?, playlist_id(to)?);
            if from == to {
                return Err(UsageError::SamePlaylist);
            }
            Plan::Import {
                from,
                to,
                duplicates: *duplicates,
            }
        }
    })
}

/// The requests of one command: each waits for its own answer.
struct Session<'a, L> {
    link: &'a mut L,
    timeout: Duration,
    next_id: u64,
}

impl<L: Link> Session<'_, L> {
    /// Sends `request`: its answer, or the message to print.
    fn ask(&mut self, request: LibraryRequest) -> Result<LibraryResponse, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.link
            .send(&ClientMessage::Library { id, request })
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.link.recv(Some(left)) {
                Ok(Some(ServerMessage::LibraryReply { id: got, result })) if got == id => {
                    return result;
                }
                // Anything else (an event) is not the answer.
                Ok(Some(_)) => {}
                Ok(None) => return Err(NO_ANSWER.to_owned()),
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    /// The playlist's header.
    fn header(&mut self, uuid: &str) -> Result<PlaylistSummary, String> {
        let page = PageRequest::Playlist(uuid.to_owned());
        match self.ask(LibraryRequest::Page(page))? {
            LibraryResponse::Page(PageData::Playlist { playlist, .. }) => Ok(playlist),
            _ => Err(UNEXPECTED.to_owned()),
        }
    }

    /// The header of a playlist that must be the user's.
    fn own(&mut self, uuid: &str) -> Result<PlaylistSummary, String> {
        let playlist = self.header(uuid)?;
        if playlist.own {
            Ok(playlist)
        } else {
            Err(format!("{} is not your playlist", playlist.title))
        }
    }

    /// Every track of the playlist, and its total (videos counted).
    fn tracks(&mut self, uuid: &str) -> Result<(Vec<TrackId>, u32), String> {
        let (mut tracks, mut offset) = (Vec::new(), 0);
        loop {
            let more = LibraryRequest::More {
                list: ListRef::PlaylistTracks(uuid.to_owned()),
                offset,
                limit: TRACKS_PAGE,
            };
            let LibraryResponse::Items(ListItems::Tracks(page)) = self.ask(more)? else {
                return Err(UNEXPECTED.to_owned());
            };
            tracks.extend(page.items.iter().map(|t| t.id));
            offset += TRACKS_PAGE;
            if offset >= page.total {
                return Ok((tracks, page.total));
            }
        }
    }

    /// Every playlist of the library.
    fn playlists(&mut self) -> Result<Vec<PlaylistSummary>, String> {
        let (mut playlists, mut offset) = (Vec::new(), 0);
        loop {
            let more = LibraryRequest::More {
                list: ListRef::Playlists,
                offset,
                limit: PLAYLISTS_PAGE,
            };
            let LibraryResponse::Items(ListItems::Playlists(page)) = self.ask(more)? else {
                return Err(UNEXPECTED.to_owned());
            };
            playlists.extend(page.items);
            offset += PLAYLISTS_PAGE;
            if offset >= page.total {
                return Ok(playlists);
            }
        }
    }

    /// Creates a playlist: its UUID.
    fn create(&mut self, title: &str, description: &str) -> Result<String, String> {
        let request = LibraryRequest::CreatePlaylist {
            title: title.to_owned(),
            description: description.to_owned(),
        };
        match self.ask(request)? {
            LibraryResponse::Created(playlist) => Ok(playlist.uuid),
            _ => Err(UNEXPECTED.to_owned()),
        }
    }

    /// Adds `tracks` in batches; a failure says how far it got.
    fn add(&mut self, uuid: &str, title: &str, tracks: &[TrackId]) -> Result<(), String> {
        let mut added = 0;
        for batch in tracks.chunks(ADD_BATCH) {
            let request = LibraryRequest::AddToPlaylist {
                uuid: uuid.to_owned(),
                tracks: batch.to_vec(),
                allow_duplicates: true,
            };
            if let Err(reason) = self.ask(request) {
                return Err(format!(
                    "Added {} of {} tracks to {title}: {reason}",
                    group(added),
                    group(tracks.len() as u32),
                ));
            }
            added += batch.len() as u32;
        }
        Ok(())
    }
}

/// `1 track`, `12 tracks`.
fn count(n: usize, one: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{} {one}s", group(n as u32)),
    }
}

/// `text` on one line: a tab or line break would break the columns.
fn one_line(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// `list`'s output.
fn list_text(playlists: &[PlaylistSummary], json: bool) -> String {
    if json {
        let rows: Vec<serde_json::Value> = playlists
            .iter()
            .map(|p| {
                serde_json::json!({
                    "uuid": p.uuid,
                    "title": p.title,
                    "own": p.own,
                    "public": p.public,
                    "tracks": p.tracks,
                    "description": p.description,
                })
            })
            .collect();
        return format!("{}\n", serde_json::Value::Array(rows));
    }
    playlists
        .iter()
        .map(|p| {
            let kind = if p.own { "own" } else { "followed" };
            let tracks = p.tracks.map_or("-".to_owned(), |n| n.to_string());
            format!("{}\t{kind}\t{tracks}\t{}\n", p.uuid, one_line(&p.title))
        })
        .collect()
}

/// Says on stderr that `total` held more than `tracks`: videos.
fn note_videos(err: &mut dyn Write, tracks: usize, total: u32) {
    let videos = (total as usize).saturating_sub(tracks);
    if videos > 0 {
        let _ = writeln!(err, "Skipped {}", count(videos, "video"));
    }
}

/// `delete`'s question: whether the answer is `y`.
fn confirmed(err: &mut dyn Write, terminal: &mut dyn BufRead, title: &str) -> bool {
    let _ = write!(err, "Delete {title}? (y/n) ");
    let _ = err.flush();
    let mut line = String::new();
    terminal.read_line(&mut line).is_ok() && line.trim() == "y"
}

/// `new`: the UUID is printed before anything else can fail.
fn new<L: Link>(
    session: &mut Session<'_, L>,
    out: &mut dyn Write,
    title: &str,
    description: &str,
    public: bool,
) -> Result<(), String> {
    let uuid = session.create(title, description)?;
    let _ = writeln!(out, "{uuid}");
    if public {
        session.ask(LibraryRequest::SetPlaylistPublic { uuid, public })?;
    }
    Ok(())
}

/// `edit`: what is given, nothing else.
fn edit<L: Link>(
    session: &mut Session<'_, L>,
    uuid: &str,
    title: &Option<String>,
    description: &Option<String>,
    public: Option<bool>,
) -> Result<(), String> {
    session.own(uuid)?;
    if title.is_some() || description.is_some() {
        session.ask(LibraryRequest::EditPlaylist {
            uuid: uuid.to_owned(),
            title: title.clone(),
            description: description.clone(),
        })?;
    }
    if let Some(public) = public {
        session.ask(LibraryRequest::SetPlaylistPublic {
            uuid: uuid.to_owned(),
            public,
        })?;
    }
    Ok(())
}

/// `fork`: the UUID is printed even when adding fails, so that a script can
/// find what was left.
fn fork<L: Link>(
    session: &mut Session<'_, L>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    uuid: &str,
    name: Option<&str>,
) -> Result<(), String> {
    let source = session.header(uuid)?;
    let (tracks, total) = session.tracks(uuid)?;
    note_videos(err, tracks.len(), total);
    let title = name.map_or_else(|| format!("{} (copy)", source.title), str::to_owned);
    let description = source.description.unwrap_or_default();
    let new = session.create(&title, &description)?;
    let added = session.add(&new, &title, &tracks);
    let _ = writeln!(out, "{new}");
    added
}

/// `import`: `from`'s tracks at the end of `to`, in order.
fn import<L: Link>(
    session: &mut Session<'_, L>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    from: &str,
    to: &str,
    duplicates: bool,
) -> Result<(), String> {
    let target = session.own(to)?;
    let (tracks, total) = session.tracks(from)?;
    note_videos(err, tracks.len(), total);
    let new: Vec<TrackId> = if duplicates {
        tracks.clone()
    } else {
        let (present, _) = session.tracks(to)?;
        let mut seen: HashSet<TrackId> = present.into_iter().collect();
        tracks.iter().copied().filter(|t| seen.insert(*t)).collect()
    };
    session.add(to, &target.title, &new)?;
    let _ = writeln!(
        out,
        "Added {} to {}, skipped {}",
        count(new.len(), "track"),
        target.title,
        group((tracks.len() - new.len()) as u32),
    );
    Ok(())
}

/// Runs `plan` over `link`; `terminal` answers `delete`'s question (`None`:
/// no terminal). The exit code.
pub fn execute<L: Link>(
    link: &mut L,
    plan: &Plan,
    timeout: Duration,
    out: &mut dyn Write,
    err: &mut dyn Write,
    terminal: Option<&mut dyn BufRead>,
) -> u8 {
    if matches!(plan, Plan::Delete { yes: false, .. }) && terminal.is_none() {
        let _ = writeln!(err, "{NEEDS_YES}");
        return 2;
    }
    let mut session = Session {
        link,
        timeout,
        next_id: 0,
    };
    let done = match plan {
        Plan::List { json } => session.playlists().map(|playlists| {
            let _ = out.write_all(list_text(&playlists, *json).as_bytes());
        }),
        Plan::New {
            title,
            description,
            public,
        } => new(&mut session, out, title, description, *public),
        Plan::Edit {
            uuid,
            title,
            description,
            public,
        } => edit(&mut session, uuid, title, description, *public),
        Plan::Delete { uuid, yes } => session.own(uuid).and_then(|playlist| {
            let go = *yes || terminal.is_some_and(|t| confirmed(err, t, &playlist.title));
            if go {
                let uuid = uuid.clone();
                session.ask(LibraryRequest::DeletePlaylist { uuid })?;
            }
            Ok(())
        }),
        Plan::Fork { uuid, name } => fork(&mut session, out, err, uuid, name.as_deref()),
        Plan::Import {
            from,
            to,
            duplicates,
        } => import(&mut session, out, err, from, to, *duplicates),
    };
    let _ = out.flush();
    match done {
        Ok(()) => 0,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            1
        }
    }
}

/// `tidal-player playlist <command>`.
pub fn run(command: &PlaylistCommand) -> ExitCode {
    let plan = match plan(command) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut link = match find(|key| std::env::var(key).ok()) {
        Ok((link, _)) => link,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    let terminal: Option<&mut dyn BufRead> = if io::stdin().is_terminal() {
        Some(&mut lock)
    } else {
        None
    };
    let code = execute(
        &mut link,
        &plan,
        REPLY_TIMEOUT,
        &mut io::stdout(),
        &mut io::stderr(),
        terminal,
    );
    Link::close(&mut link);
    ExitCode::from(code)
}

#[cfg(test)]
mod tests;
