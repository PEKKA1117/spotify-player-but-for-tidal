//! The album cover in the playback window (spec 0016): which graphics
//! protocol draws it ([`protocol`]), what the TUI asks its loader for and
//! keeps ([`Slot`], pure), and the loader thread ([`Loader`]), which only
//! reads the player's cover cache: it never downloads, writes, touches or
//! evicts a file (the player does, [`crate::covers`]).

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

use crate::ui::CoverImage;

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
    pub fn from_player(player: &crate::play::PlayerSettings) -> Self {
        Self {
            enabled: player.cover_img,
            rows: player.cover_img_rows,
            protocol: player.cover_img_protocol,
            cache_on: player.max_cover_arts > 0,
            cache_dir: crate::covers::process_cache_dir(),
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
    let Some(path) = crate::covers::cached_file(cache_dir, cover) else {
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
mod tests {
    use std::cell::Cell;
    use std::sync::Mutex;

    use super::*;

    const A: &str = "2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec";
    const B: &str = "8b0c1a3e-1111-4c3a-a0ba-000000000002";
    const C: &str = "8b0c1a3e-2222-4c3a-a0ba-000000000003";

    fn size(width: u16, height: u16) -> Size {
        Size { width, height }
    }

    /// AC3: the protocol per `cover_img`, `cover_img_protocol` and the
    /// query's answer, and whether the terminal is asked at all.
    #[test]
    fn ac3_protocol() {
        use Graphics::*;
        use ProtocolSetting as P;
        let cell = Some((8, 16));
        let answer = |graphics, cell| Some(Answer { graphics, cell });
        // (name, cover_img, setting, answer, expected, queried)
        type Row = (
            &'static str,
            bool,
            ProtocolSetting,
            Option<Answer>,
            Option<Choice>,
            bool,
        );
        let choice = |graphics, cell| Some(Choice { graphics, cell });
        let rows: &[Row] = &[
            (
                "auto sixel",
                true,
                P::Auto,
                answer(Sixel, cell),
                choice(Sixel, cell),
                true,
            ),
            (
                "auto kitty",
                true,
                P::Auto,
                answer(Kitty, cell),
                choice(Kitty, cell),
                true,
            ),
            (
                "auto iterm2",
                true,
                P::Auto,
                answer(Iterm2, cell),
                choice(Iterm2, cell),
                true,
            ),
            (
                "auto halfblocks only",
                true,
                P::Auto,
                answer(Halfblocks, cell),
                None,
                true,
            ),
            ("auto no answer", true, P::Auto, None, None, true),
            (
                "forced sixel over kitty",
                true,
                P::Sixel,
                answer(Kitty, cell),
                choice(Sixel, cell),
                true,
            ),
            (
                "forced kitty over halfblocks",
                true,
                P::Kitty,
                answer(Halfblocks, cell),
                choice(Kitty, cell),
                true,
            ),
            (
                "forced iterm2, no answer",
                true,
                P::Iterm2,
                None,
                choice(Iterm2, None),
                true,
            ),
            (
                "forced sixel, no cell size",
                true,
                P::Sixel,
                answer(Sixel, None),
                choice(Sixel, None),
                true,
            ),
            (
                "halfblocks over sixel",
                true,
                P::Halfblocks,
                answer(Sixel, cell),
                choice(Halfblocks, cell),
                true,
            ),
            (
                "halfblocks, no answer",
                true,
                P::Halfblocks,
                None,
                choice(Halfblocks, None),
                true,
            ),
            (
                "off, auto",
                false,
                P::Auto,
                answer(Kitty, cell),
                None,
                false,
            ),
            (
                "off, forced",
                false,
                P::Kitty,
                answer(Kitty, cell),
                None,
                false,
            ),
            ("off, halfblocks", false, P::Halfblocks, None, None, false),
        ];
        for (name, on, setting, answer, want, queried) in rows {
            let asked = Cell::new(false);
            let got = protocol(*on, *setting, || {
                asked.set(true);
                *answer
            });
            assert_eq!(got, *want, "{name}");
            assert_eq!(asked.get(), *queried, "{name}: queried");
        }
    }

    #[test]
    fn ac3_protocol_names() {
        for (name, want) in [
            ("auto", Some(ProtocolSetting::Auto)),
            ("sixel", Some(ProtocolSetting::Sixel)),
            ("kitty", Some(ProtocolSetting::Kitty)),
            ("iterm2", Some(ProtocolSetting::Iterm2)),
            ("halfblocks", Some(ProtocolSetting::Halfblocks)),
            ("ascii", None),
            ("Kitty", None),
            ("", None),
        ] {
            assert_eq!(ProtocolSetting::parse(name), want, "{name:?}");
        }
    }

    /// One step of a slot's life and what it should ask for.
    #[derive(Debug, Clone, Copy)]
    enum Step {
        /// The current cover (`None`: no cover, or the queue emptied).
        Cover(Option<&'static str>, Ask),
        /// The cover area (`None`: not active).
        Size(Option<(u16, u16)>, Ask),
        /// A result: cover, size, what (`Some(label)`: an image).
        Result(&'static str, (u16, u16), Got),
        /// Seconds since the start.
        Tick(u64, Ask),
        /// What is drawn: `Some((label, size))` or blank.
        Shows(Option<(&'static str, (u16, u16))>),
    }

    #[derive(Debug, Clone, Copy)]
    enum Got {
        Image(&'static str),
        Missing,
        Failed,
    }

    /// The request expected: `Some((cover, size))` or nothing.
    type Ask = Option<(&'static str, (u16, u16))>;

    /// AC4: the slot's decisions over sequences of events.
    #[test]
    fn ac4_slot() {
        use Step::*;
        const S: (u16, u16) = (12, 6);
        const T: (u16, u16) = (16, 8);
        let rows: &[(&str, &[Step])] = &[
            (
                "asks for the current cover, shows its image",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Shows(None),
                    Result(A, S, Got::Image("a")),
                    Shows(Some(("a", S))),
                    Tick(5, None),
                ],
            ),
            (
                "nothing asked while not active",
                &[
                    Cover(Some(A), None),
                    Tick(2, None),
                    Size(Some(S), Some((A, S))),
                ],
            ),
            (
                "the newest cover only; a stale result is dropped",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Cover(Some(B), Some((B, S))),
                    Result(A, S, Got::Image("a")),
                    Shows(None),
                    Result(B, S, Got::Image("b")),
                    Shows(Some(("b", S))),
                ],
            ),
            (
                "missing: looks again every second while current",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Missing),
                    Tick(0, None),
                    Shows(None),
                    Tick(1, Some((A, S))),
                    Tick(1, None),
                    Result(A, S, Got::Missing),
                    Tick(2, Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Shows(Some(("a", S))),
                    Tick(3, None),
                    Tick(10, None),
                ],
            ),
            (
                "missing: never again once no longer current",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Missing),
                    Cover(Some(B), Some((B, S))),
                    Result(B, S, Got::Image("b")),
                    Tick(1, None),
                    Tick(5, None),
                ],
            ),
            (
                "the queue empties: blank, nothing asked",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Cover(None, None),
                    Shows(None),
                    Tick(5, None),
                    Result(A, S, Got::Image("a")),
                    Shows(None),
                ],
            ),
            (
                "missing, then the queue empties: no more looks",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Missing),
                    Cover(None, None),
                    Tick(1, None),
                    Tick(9, None),
                ],
            ),
            (
                "resize: encoded again, blank until then; stale size dropped",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Size(Some(T), Some((A, T))),
                    Shows(None),
                    Result(A, S, Got::Image("a-old")),
                    Shows(None),
                    Result(A, T, Got::Image("a-new")),
                    Shows(Some(("a-new", T))),
                ],
            ),
            (
                "resize to the same size asks nothing",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Size(Some(S), None),
                    Shows(Some(("a", S))),
                ],
            ),
            (
                "inactive: blank; active again: asked again",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Size(None, None),
                    Shows(None),
                    Result(A, S, Got::Image("a")),
                    Shows(None),
                    Size(Some(S), Some((A, S))),
                ],
            ),
            (
                "the same cover again asks nothing",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Cover(Some(A), None),
                    Shows(Some(("a", S))),
                ],
            ),
            (
                "failed: blank, not tried again, not even on resize",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Failed),
                    Shows(None),
                    Tick(1, None),
                    Tick(30, None),
                    Size(Some(T), None),
                    Cover(Some(A), None),
                ],
            ),
            (
                "failed: tried again once current again",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Failed),
                    Cover(Some(B), Some((B, S))),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Shows(Some(("a", S))),
                ],
            ),
            (
                "a track without a cover: blank",
                &[
                    Size(Some(S), None),
                    Cover(Some(A), Some((A, S))),
                    Result(A, S, Got::Image("a")),
                    Cover(None, None),
                    Shows(None),
                    Cover(Some(C), Some((C, S))),
                ],
            ),
        ];
        let start = Instant::now();
        let req = |ask: Ask| {
            ask.map(|(cover, (w, h))| Request {
                cover: cover.to_owned(),
                size: size(w, h),
            })
        };
        for (name, steps) in rows {
            let mut slot: Slot<&'static str> = Slot::default();
            let mut now = start;
            for (i, step) in steps.iter().enumerate() {
                let at = format!("{name}: step {i} {step:?}");
                match *step {
                    Cover(cover, ask) => assert_eq!(slot.set_cover(cover), req(ask), "{at}"),
                    Size(area, ask) => assert_eq!(
                        slot.set_size(area.map(|(w, h)| size(w, h))),
                        req(ask),
                        "{at}"
                    ),
                    Result(cover, (w, h), got) => slot.loaded(
                        Loaded {
                            request: Request {
                                cover: cover.to_owned(),
                                size: size(w, h),
                            },
                            outcome: match got {
                                Got::Image(label) => Outcome::Image(label),
                                Got::Missing => Outcome::Missing,
                                Got::Failed => Outcome::Failed,
                            },
                        },
                        now,
                    ),
                    Tick(secs, ask) => {
                        now = start + Duration::from_secs(secs);
                        assert_eq!(slot.tick(now), req(ask), "{at}");
                    }
                    Shows(want) => assert_eq!(
                        slot.image().map(|(s, label)| (*label, (s.width, s.height))),
                        want,
                        "{at}"
                    ),
                }
            }
        }
    }

    // --- AC8: the loader, on a temp cache directory -------------------------------

    /// What the fake encoder made: the decoded image's pixel size and the
    /// area it was encoded for.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Fake {
        pixels: (u32, u32),
        size: Size,
    }

    struct FakeEncoder;

    impl Encode for FakeEncoder {
        type Image = Fake;
        fn encode(&self, image: DynamicImage, size: Size) -> Result<Fake, String> {
            Ok(Fake {
                pixels: (image.width(), image.height()),
                size,
            })
        }
    }

    /// A JPEG of `w` × `h` pixels.
    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 40, 40])))
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }

    /// The cache's `covers/` with `files` in it, renamed into place as the
    /// player does.
    fn cache(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let covers = dir.path().join("covers");
        std::fs::create_dir_all(&covers).unwrap();
        for (cover, bytes) in files {
            put(dir.path(), cover, bytes);
        }
        dir
    }

    fn put(cache_dir: &Path, cover: &str, bytes: &[u8]) {
        let covers = cache_dir.join("covers");
        let tmp = covers.join(format!("{cover}.jpg.tmp"));
        std::fs::write(&tmp, bytes).unwrap();
        std::fs::rename(&tmp, covers.join(format!("{cover}.jpg"))).unwrap();
    }

    /// The `warn` lines, kept.
    fn warns() -> (Warn, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&lines);
        (
            Arc::new(move |line: &str| kept.lock().unwrap().push(line.to_owned())),
            lines,
        )
    }

    /// An image server that must see no request: the TUI never downloads.
    struct NoDownloads {
        runtime: tokio::runtime::Runtime,
        server: wiremock::MockServer,
    }

    impl NoDownloads {
        fn start() -> Self {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let server = runtime.block_on(async {
                let server = wiremock::MockServer::start().await;
                wiremock::Mock::given(wiremock::matchers::any())
                    .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(jpeg(4, 4)))
                    .expect(0)
                    .mount(&server)
                    .await;
                server
            });
            Self { runtime, server }
        }

        fn verify(self) {
            self.runtime.block_on(self.server.verify());
        }
    }

    const AREA: Size = Size {
        width: 12,
        height: 6,
    };

    /// Updates `cover` at `now` until `done` or 3 s of real time pass.
    fn until(
        cover: &mut Cover<Fake>,
        current: Option<&str>,
        now: Instant,
        done: impl Fn(&Cover<Fake>) -> bool,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            cover.update(current, Some(AREA), now);
            if done(cover) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn shown(cover: &Cover<Fake>) -> Option<Fake> {
        cover.image().map(|(_, fake)| fake.clone())
    }

    /// AC8: a cached cover is read, decoded and encoded at the area's size.
    #[test]
    fn ac8_cached() {
        let images = NoDownloads::start();
        let dir = cache(&[(A, &jpeg(6, 6))]);
        let (warn, lines) = warns();
        let mut cover = Cover::new(
            Loader::spawn(dir.path().to_owned(), FakeEncoder, warn),
            true,
        );
        let t0 = Instant::now();
        assert!(
            until(&mut cover, Some(A), t0, |c| c.image().is_some()),
            "no image for a cached cover"
        );
        assert_eq!(
            shown(&cover),
            Some(Fake {
                pixels: (6, 6),
                size: AREA
            })
        );
        assert_eq!(cover.image().map(|(s, _)| s), Some(AREA));
        assert!(lines.lock().unwrap().is_empty(), "{lines:?}");
        images.verify();
    }

    /// AC8: `max_cover_arts = 0`: nothing is looked for, even a cached file.
    #[test]
    fn ac8_cache_off() {
        let dir = cache(&[(A, &jpeg(6, 6))]);
        let (warn, _) = warns();
        let mut cover = Cover::new(
            Loader::spawn(dir.path().to_owned(), FakeEncoder, warn),
            false,
        );
        let t0 = Instant::now();
        assert!(!until(&mut cover, Some(A), t0, |c| c.image().is_some()
            || c.slot().look_at().is_some()));
    }

    /// AC8: a missing cover yields nothing, then yields once the player
    /// renamed it into place and the one-second look comes.
    #[test]
    fn ac8_appears_later() {
        let images = NoDownloads::start();
        let dir = cache(&[]);
        let (warn, lines) = warns();
        let mut cover = Cover::new(
            Loader::spawn(dir.path().to_owned(), FakeEncoder, warn),
            true,
        );
        let t0 = Instant::now();
        assert!(
            until(&mut cover, Some(A), t0, |c| c.slot().look_at().is_some()),
            "the loader never said the file is missing"
        );
        assert_eq!(shown(&cover), None);
        put(dir.path(), A, &jpeg(5, 5));
        // Before the second is up: not looked at again.
        let early = t0 + Duration::from_millis(500);
        for _ in 0..20 {
            cover.update(Some(A), Some(AREA), early);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(shown(&cover), None, "looked again before the second");
        assert!(
            until(&mut cover, Some(A), t0 + LOOK_AGAIN, |c| c
                .image()
                .is_some()),
            "no image after the file appeared"
        );
        assert_eq!(shown(&cover).map(|f| f.pixels), Some((5, 5)));
        assert!(lines.lock().unwrap().is_empty(), "{lines:?}");
        images.verify();
    }

    /// Every file in the cache: name, length, modification time.
    fn listing(dir: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
        let mut files: Vec<_> = std::fs::read_dir(dir.join("covers"))
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                let meta = e.metadata().unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    meta.len(),
                    meta.modified().unwrap(),
                )
            })
            .collect();
        files.sort();
        files
    }

    /// AC8: the loader never writes, touches or deletes: the cache's
    /// files, names and modification times are the same after it ran on a
    /// cached, a missing and a broken cover; no request reached the image
    /// server.
    #[test]
    fn ac8_read_only() {
        let images = NoDownloads::start();
        let dir = cache(&[(A, &jpeg(4, 4)), (C, b"not a jpeg")]);
        // A stale `.tmp` and a foreign file stay too (the player cleans).
        std::fs::write(dir.path().join("covers/x.jpg.tmp"), b"half").unwrap();
        std::fs::write(dir.path().join("covers/notes.txt"), b"mine").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        for name in std::fs::read_dir(dir.path().join("covers")).unwrap() {
            let file = std::fs::File::options()
                .write(true)
                .open(name.unwrap().path())
                .unwrap();
            file.set_modified(old).unwrap();
        }
        let before = listing(dir.path());
        let (warn, _) = warns();
        let mut cover = Cover::new(
            Loader::spawn(dir.path().to_owned(), FakeEncoder, warn),
            true,
        );
        let t0 = Instant::now();
        until(&mut cover, Some(A), t0, |c| c.image().is_some());
        until(&mut cover, Some(B), t0, |c| c.slot().look_at().is_some());
        until(&mut cover, Some(B), t0 + LOOK_AGAIN, |c| {
            c.slot().look_at() > Some(t0 + LOOK_AGAIN)
        });
        until(&mut cover, Some(C), t0, |c| c.slot().failed());
        // The direct look too.
        let (warn, _) = warns();
        for cover in [A, B, C, "../escape"] {
            let request = Request {
                cover: cover.to_owned(),
                size: AREA,
            };
            let _ = load(dir.path(), &request, &FakeEncoder, &warn);
        }
        assert_eq!(listing(dir.path()), before);
        assert!(!dir.path().join("covers").join(format!("{B}.jpg")).exists());
        images.verify();
    }

    /// AC8: a file that does not decode: blank, one `warn`, not tried
    /// again while it stays current.
    #[test]
    fn ac8_decode_failure() {
        let images = NoDownloads::start();
        let dir = cache(&[(C, b"\xff\xd8 not really a jpeg")]);
        let (warn, lines) = warns();
        let mut cover = Cover::new(
            Loader::spawn(dir.path().to_owned(), FakeEncoder, warn),
            true,
        );
        let t0 = Instant::now();
        assert!(
            until(&mut cover, Some(C), t0, |c| c.slot().failed()),
            "the broken file did not fail"
        );
        for secs in [1, 2, 5, 30] {
            cover.update(Some(C), Some(AREA), t0 + Duration::from_secs(secs));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(shown(&cover), None);
        let lines = lines.lock().unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains(C), "{lines:?}");
        images.verify();
    }

    /// AC8: a newer cover while one is read: the older result is never
    /// shown, the newer one is.
    #[test]
    fn ac8_newest_wins() {
        let images = NoDownloads::start();
        let dir = cache(&[(A, &jpeg(2, 2)), (B, &jpeg(3, 3))]);
        let (warn, _) = warns();
        let mut cover = Cover::new(
            Loader::spawn(dir.path().to_owned(), FakeEncoder, warn),
            true,
        );
        let t0 = Instant::now();
        cover.update(Some(A), Some(AREA), t0);
        let mut seen = Vec::new();
        assert!(until(&mut cover, Some(B), t0, |c| {
            match shown(c) {
                Some(fake) => fake.pixels == (3, 3),
                None => false,
            }
        }));
        // Keep looking a while: A never comes back.
        for _ in 0..20 {
            cover.update(Some(B), Some(AREA), t0);
            seen.extend(shown(&cover).map(|f| f.pixels));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(seen.iter().all(|p| *p == (3, 3)), "{seen:?}");
        images.verify();
    }
}
