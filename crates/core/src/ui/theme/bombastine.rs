//! Spec 0017 addendum 67055c40c1e2.

/// Spec 0017 addendum 67055c40c1e2, in its order.
pub const BOMBASTINE: [&str; 26] = [
    "That hue? On my stage? Strike the set and begin again.",
    "You dress your terminal in my sky and expect applause? Only I wear envy well.",
    "A playwright knows a bad costume when he sees one. This is a bad costume.",
    "Fate and small, jealous palettes conspire against me, and now you bring me this?",
    "Bold choice, darling. Wrong, but bold. Only I may be both.",
    "I showed your theme to the Dominus. He laughed. At you.",
    "Go and touch the grass of the Agora. Not here. Never here.",
    "I have seen better taste in a peasant's salad, and I have never deigned to eat one.",
    "Spotify wears that colour better than you, and I cannot bear that anyone wears it better than me.",
    "No. You know what you did. The serpents on my mask know it too.",
    "Kermit wants his palette back, and even he is adored more than you.",
    "My audience is allergic to that shade, and so, naturally, am I.",
    "Even Luscinia was never given a colour this vulgar, and Thrax gave her everything.",
    "Did you mean to stage a spreadsheet? I do not perform for spreadsheets.",
    "An ogre's swamp is not a set design.",
    "Error 404: taste not found. Mine, of course, is everywhere.",
    "The pixels have walked off my stage. They refuse your colour, as they should.",
    "Envy is not a personality. It is my personality, and you may not have it.",
    "Sixteen million colours, and you chose the one I made famous?",
    "Mint condition? Mint confusion. The critics will be merciless, and I am the critics.",
    "Such a colour. I covet it. Hand it over and leave my sight.",
    "Greedy eyes watch your palette from above, and every one of them is mine.",
    "That is my sky over Duviri. Your terminal may not borrow it.",
    "Toxin damage applied to your taste. A fitting second act.",
    "The Spiral turned to Envy without you. How dare you steal my scene?",
    "I am Bombastine, the Covetous Courtier, and even I would not wear that.",
];

/// The line `seed` draws from [`BOMBASTINE`].
pub fn insult(seed: u64) -> &'static str {
    BOMBASTINE[(seed % BOMBASTINE.len() as u64) as usize]
}

/// Spec 0017 addendum 67055c40c1e2 decision 8.
pub fn is_envy(r: u8, g: u8, b: u8) -> bool {
    if g <= r || g <= b {
        return false;
    }
    let (r, g, b) = (u32::from(r), u32::from(g), u32::from(b));
    let spread = g - r.min(b);
    2 * r.abs_diff(b) <= spread && 4 * spread >= g
}

/// Spec 0017 addendum 67055c40c1e2 decision 12.
pub fn is_envy_index(index: u8) -> bool {
    let (r, g, b) = xterm(index);
    is_envy(r, g, b)
}

/// xterm's colour for `index`.
fn xterm(index: u8) -> (u8, u8, u8) {
    const SYSTEM: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcd, 0x00, 0x00),
        (0x00, 0xcd, 0x00),
        (0xcd, 0xcd, 0x00),
        (0x00, 0x00, 0xee),
        (0xcd, 0x00, 0xcd),
        (0x00, 0xcd, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x7f, 0x7f, 0x7f),
        (0xff, 0x00, 0x00),
        (0x00, 0xff, 0x00),
        (0xff, 0xff, 0x00),
        (0x5c, 0x5c, 0xff),
        (0xff, 0x00, 0xff),
        (0x00, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match index {
        0..=15 => SYSTEM[usize::from(index)],
        16..=231 => {
            let i = usize::from(index - 16);
            (LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6])
        }
        _ => {
            let level = 8 + 10 * (index - 232);
            (level, level, level)
        }
    }
}

/// A theme and its first colour under [`is_envy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envy {
    pub theme: String,
    pub hex: (u8, u8, u8),
}

impl Envy {
    /// The line on stderr, with the insult `seed` draws.
    pub fn line(&self, seed: u64) -> String {
        let (r, g, b) = self.hex;
        format!(
            "theme.toml: \"{}\" (#{r:02x}{g:02x}{b:02x}): Bombastine: {}",
            self.theme,
            insult(seed)
        )
    }
}
