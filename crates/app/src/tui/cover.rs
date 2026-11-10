//! The album cover in the playback window (spec 0016): which graphics
//! protocol draws it ([`protocol`]), what the TUI asks its loader for and
//! keeps ([`Slot`], pure), and the loader thread ([`Loader`]), which only
//! reads the player's cover cache: it never downloads, writes, touches or
//! evicts a file (the player does, [`crate::player::cover_cache`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::widgets::Widget;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize};

use crate::tui::render::CoverImage;

/// How long the loader waits before it looks again for a cover the player
/// has not cached yet.
pub const LOOK_AGAIN: Duration = Duration::from_secs(1);

/// `cover_img_protocol`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtocolSetting {
    /// The one the terminal answers to; none when it answers to none.
    #[default]
    Auto,
    Sixel,
    Kitty,
    Iterm2,
    Halfblocks,
}

/// The values `cover_img_protocol` accepts, for error messages.
pub const PROTOCOL_NAMES: &str = "auto, sixel, kitty, iterm2 or halfblocks";

impl ProtocolSetting {
    /// From its `app.toml` / environment name.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Self::Auto),
            "sixel" => Some(Self::Sixel),
            "kitty" => Some(Self::Kitty),
            "iterm2" => Some(Self::Iterm2),
            "halfblocks" => Some(Self::Halfblocks),
            _ => None,
        }
    }
}

/// A protocol the cover is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Graphics {
    Sixel,
    Kitty,
    Iterm2,
    /// Coloured `▀` cells.
    Halfblocks,
}

/// The terminal's answer to the query at start: the protocol it supports
/// (halfblocks: none of the three) and its cell size in pixels (width,
/// height), when known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub graphics: Graphics,
    pub cell: Option<(u16, u16)>,
}

/// The protocol the TUI draws the cover with and the cell size its layout
/// uses (spec 0016 "Protocol").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub graphics: Graphics,
    pub cell: Option<(u16, u16)>,
}

/// The protocol for `cover_img` and `setting`; `query` asks the terminal
/// and is only called when `cover_img` is on. `None`: no cover.
pub fn protocol(
    cover_img: bool,
    setting: ProtocolSetting,
    query: impl FnOnce() -> Option<Answer>,
) -> Option<Choice> {
    if !cover_img {
        return None;
    }
    let answer = query();
    let graphics = match setting {
        ProtocolSetting::Auto => match answer?.graphics {
            Graphics::Halfblocks => return None,
            graphics => graphics,
        },
        ProtocolSetting::Sixel => Graphics::Sixel,
        ProtocolSetting::Kitty => Graphics::Kitty,
        ProtocolSetting::Iterm2 => Graphics::Iterm2,
        ProtocolSetting::Halfblocks => Graphics::Halfblocks,
    };
    Some(Choice {
        graphics,
        cell: answer.and_then(|a| a.cell),
    })
}

/// The TUI's cover settings (`app.toml` under the environment, spec 0016
/// "Config") and where the player's cover cache is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `cover_img`.
    pub enabled: bool,
    /// `cover_img_rows`.
    pub rows: u16,
    /// `cover_img_protocol`.
    pub protocol: ProtocolSetting,
    /// `max_cover_arts > 0`: the player caches covers.
    pub cache_on: bool,
    /// The cache directory (`covers/` is inside).
    pub cache_dir: PathBuf,
}

impl Settings {
    /// From the resolved settings and this process's environment.
    pub fn from_player(player: &crate::settings::PlayerSettings) -> Self {
        Self {
            enabled: player.cover_img,
            rows: player.cover_img_rows,
            protocol: player.cover_img_protocol,
            cache_on: player.max_cover_arts > 0,
            cache_dir: crate::player::cover_cache::process_cache_dir(),
        }
    }
}

/// What the TUI asks its loader for: one cover, encoded for an area of
/// `size` cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub cover: String,
    pub size: Size,
}

/// What one look in the cache found.
#[derive(Debug)]
pub enum Outcome<I> {
    Image(I),
    /// Not cached (yet).
    Missing,
    /// Could not be read, decoded or encoded (logged).
    Failed,
}

/// The loader's answer to a [`Request`].
#[derive(Debug)]
pub struct Loaded<I> {
    pub request: Request,
    pub outcome: Outcome<I>,
}

/// The cover loader's decisions (spec 0016 "Where the image comes from"),
/// fed what the screen shows: the current entry's cover, the cover area's
/// size (`None` while the cover is not active), the loader's results and
/// the time. It asks for the current cover only, looks again every
/// [`LOOK_AGAIN`] while its file is missing, keeps only a result for the
/// current cover at the current size, and does not try a cover that failed
/// again until it becomes current again.
#[derive(Debug)]
pub struct Slot<I> {
    cover: Option<String>,
    size: Option<Size>,
    ready: Option<(Size, I)>,
    look_at: Option<Instant>,
    failed: bool,
}

impl<I> Default for Slot<I> {
    fn default() -> Self {
        Self {
            cover: None,
            size: None,
            ready: None,
            look_at: None,
            failed: false,
        }
    }
}

impl<I> Slot<I> {
    /// The current entry's cover changed (`None`: no cover, or nothing is
    /// current); what to ask the loader for.
    pub fn set_cover(&mut self, cover: Option<&str>) -> Option<Request> {
        if self.cover.as_deref() == cover {
            return None;
        }
        self.cover = cover.map(str::to_owned);
        self.ready = None;
        self.look_at = None;
        self.failed = false;
        self.request()
    }

    /// The cover area's size changed (`None`: the cover is not active);
    /// what to ask the loader for.
    pub fn set_size(&mut self, size: Option<Size>) -> Option<Request> {
        if self.size == size {
            return None;
        }
        self.size = size;
        self.ready = None;
        self.look_at = None;
        self.request()
    }

    /// A result from the loader, at `now`; dropped unless it is for the
    /// current cover at the current size.
    pub fn loaded(&mut self, loaded: Loaded<I>, now: Instant) {
        let Request { cover, size } = loaded.request;
        if self.cover.as_deref() != Some(cover.as_str()) || self.size != Some(size) {
            return;
        }
        match loaded.outcome {
            Outcome::Image(image) => self.ready = Some((size, image)),
            Outcome::Missing => self.look_at = Some(now + LOOK_AGAIN),
            Outcome::Failed => self.failed = true,
        }
    }

    /// The time passed: what to ask the loader for again.
    pub fn tick(&mut self, now: Instant) -> Option<Request> {
        if self.look_at.is_none_or(|at| now < at) {
            return None;
        }
        self.look_at = None;
        self.request()
    }

    /// The current cover at the current size, unless it failed.
    fn request(&self) -> Option<Request> {
        if self.failed {
            return None;
        }
        Some(Request {
            cover: self.cover.clone()?,
            size: self.size?,
        })
    }

    /// The image to draw and the area size it was encoded for; `None`:
    /// the area is blank.
    pub fn image(&self) -> Option<(Size, &I)> {
        self.ready.as_ref().map(|(size, image)| (*size, image))
    }

    /// When the loader looks again for the current cover's file.
    pub fn look_at(&self) -> Option<Instant> {
        self.look_at
    }

    /// Whether the current cover failed to load.
    pub fn failed(&self) -> bool {
        self.failed
    }
}

/// Encodes a decoded cover for the terminal at a size in cells: the seam
/// between the loader and `ratatui-image`, faked in tests.
pub trait Encode: Send + 'static {
    type Image: Send + 'static;
    fn encode(&self, image: DynamicImage, size: Size) -> Result<Self::Image, String>;
}

/// Where the loader's `warn` lines go.
pub type Warn = Arc<dyn Fn(&str) + Send + Sync>;

/// One look in the cache under `cache_dir`: the request's cover read,
/// decoded and encoded at its size. Read-only.
pub fn load<E: Encode>(
    cache_dir: &Path,
    request: &Request,
    encoder: &E,
    warn: &Warn,
) -> Outcome<E::Image> {
    let cover = &request.cover;
    let Some(path) = crate::player::cover_cache::cached_file(cache_dir, cover) else {
        warn(&format!("Not a cover ID: {cover:?}"));
        return Outcome::Failed;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Outcome::Missing,
        Err(e) => {
            warn(&format!("Cannot read the cover {}: {e}", path.display()));
            return Outcome::Failed;
        }
    };
    let image = match image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg) {
        Ok(image) => image,
        Err(e) => {
            warn(&format!("Cannot decode the cover {}: {e}", path.display()));
            return Outcome::Failed;
        }
    };
    match encoder.encode(image, request.size) {
        Ok(image) => Outcome::Image(image),
        Err(e) => {
            warn(&format!("Cannot draw the cover {cover}: {e}"));
            Outcome::Failed
        }
    }
}

/// The loader thread: takes [`Request`]s (the newest of those waiting),
/// answers each with a [`Loaded`].
pub struct Loader<I> {
    requests: Sender<Request>,
    results: Receiver<Loaded<I>>,
}

impl<I: Send + 'static> Loader<I> {
    /// Starts the thread, reading covers from the cache in `cache_dir`.
    pub fn spawn<E: Encode<Image = I>>(cache_dir: PathBuf, encoder: E, warn: Warn) -> Self {
        let (requests, asked) = std::sync::mpsc::channel::<Request>();
        let (answer, results) = std::sync::mpsc::channel();
        let thread_warn = Arc::clone(&warn);
        let spawned = std::thread::Builder::new()
            .name("cover-loader".into())
            .spawn(move || {
                // Until the TUI drops its end.
                while let Ok(mut request) = asked.recv() {
                    // Only the newest of the requests waiting.
                    while let Ok(newer) = asked.try_recv() {
                        request = newer;
                    }
                    let outcome = load(&cache_dir, &request, &encoder, &thread_warn);
                    if answer.send(Loaded { request, outcome }).is_err() {
                        break;
                    }
                }
            });
        if let Err(e) = spawned {
            warn(&format!("Cannot start the cover loader: {e}"));
        }
        Self { requests, results }
    }

    /// Asks for `request`; a later one replaces it if the thread has not
    /// started on it.
    pub fn ask(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// A result, when one is there.
    pub fn try_recv(&self) -> Option<Loaded<I>> {
        self.results.try_recv().ok()
    }
}

// --- ratatui-image ---------------------------------------------------------------

/// Asks the terminal which protocol it supports and its cell size; call it
/// once at start, on the alternate screen and before reading any event.
/// Waits at most `ratatui-image`'s timeout. `None`: no usable answer.
pub fn query_terminal() -> Option<(Picker, Answer)> {
    let picker = Picker::from_query_stdio().ok()?;
    let font = picker.font_size();
    let answer = Answer {
        graphics: match picker.protocol_type() {
            ProtocolType::Sixel => Graphics::Sixel,
            ProtocolType::Kitty => Graphics::Kitty,
            ProtocolType::Iterm2 => Graphics::Iterm2,
            ProtocolType::Halfblocks => Graphics::Halfblocks,
        },
        cell: Some((font.width, font.height)),
    };
    Some((picker, answer))
}

/// The encoder for `choice`: the queried picker (its cell size, tmux) or,
/// without an answer, one with a 10 x 20 px cell (2 columns per row),
/// drawing with the chosen protocol.
pub fn picker_for(choice: Choice, queried: Option<Picker>) -> Picker {
    let mut picker = queried.unwrap_or_else(Picker::halfblocks);
    picker.set_protocol_type(match choice.graphics {
        Graphics::Sixel => ProtocolType::Sixel,
        Graphics::Kitty => ProtocolType::Kitty,
        Graphics::Iterm2 => ProtocolType::Iterm2,
        Graphics::Halfblocks => ProtocolType::Halfblocks,
    });
    picker
}

/// Encodes with `ratatui-image`, scaled to fit the area keeping its aspect.
impl Encode for Picker {
    type Image = Protocol;
    fn encode(&self, image: DynamicImage, size: Size) -> Result<Protocol, String> {
        self.new_protocol(image, size, Resize::Fit(None))
            .map_err(|e| e.to_string())
    }
}

/// Drawn centred in the cover area.
impl CoverImage for Protocol {
    fn draw(&self, area: Rect, buf: &mut Buffer) {
        let size = self.size();
        let width = size.width.min(area.width);
        let height = size.height.min(area.height);
        let centred = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height - height) / 2,
            width,
            height,
        };
        Image::new(self).render(centred, buf);
    }
}

/// The TUI's cover: the [`Slot`] fed once per frame, its requests sent to
/// the [`Loader`] and its results taken from it. Never blocks.
pub struct Cover<I> {
    slot: Slot<I>,
    loader: Loader<I>,
    /// `max_cover_arts > 0`: the player caches covers. Off, nothing is
    /// looked for and the area stays blank.
    cache_on: bool,
}

impl<I: Send + 'static> Cover<I> {
    pub fn new(loader: Loader<I>, cache_on: bool) -> Self {
        Self {
            slot: Slot::default(),
            loader,
            cache_on,
        }
    }

    /// Once per frame: the current entry's cover and the cover area's size
    /// (`None`: not active).
    pub fn update(&mut self, cover: Option<&str>, size: Option<Size>, now: Instant) {
        while let Some(loaded) = self.loader.try_recv() {
            self.slot.loaded(loaded, now);
        }
        let cover = cover.filter(|_| self.cache_on);
        let requests = [
            self.slot.set_cover(cover),
            self.slot.set_size(size),
            self.slot.tick(now),
        ];
        // The last is the newest: the current cover at the current size.
        if let Some(request) = requests.into_iter().flatten().last() {
            self.loader.ask(request);
        }
    }

    /// The image to draw and the size it was encoded for.
    pub fn image(&self) -> Option<(Size, &I)> {
        self.slot.image()
    }

    /// The slot's state, for tests.
    pub fn slot(&self) -> &Slot<I> {
        &self.slot
    }
}

#[cfg(test)]
mod tests;
