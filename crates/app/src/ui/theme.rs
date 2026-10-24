//! The UI model's colours, modifiers and styles (spec 0017, AC8) as
//! ratatui's: what every component is drawn in.

use ratatui::style::{Color, Modifier, Style};
use tidal_player_core::ui::theme::{self as model, Component, Theme};

/// The terminal colour of `color`.
pub fn color(color: model::Color) -> Color {
    use model::Color as C;
    match color {
        C::Reset => Color::Reset,
        C::Black => Color::Black,
        C::Red => Color::Red,
        C::Green => Color::Green,
        C::Yellow => Color::Yellow,
        C::Blue => Color::Blue,
        C::Magenta => Color::Magenta,
        C::Cyan => Color::Cyan,
        C::Gray => Color::Gray,
        C::DarkGray => Color::DarkGray,
        C::LightRed => Color::LightRed,
        C::LightGreen => Color::LightGreen,
        C::LightYellow => Color::LightYellow,
        C::LightBlue => Color::LightBlue,
        C::LightMagenta => Color::LightMagenta,
        C::LightCyan => Color::LightCyan,
        C::White => Color::White,
        C::Indexed(index) => Color::Indexed(index),
        C::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// The terminal modifiers of `modifiers`.
pub fn modifiers(modifiers: model::Modifiers) -> Modifier {
    use model::Modifiers as M;
    [
        (M::BOLD, Modifier::BOLD),
        (M::DIM, Modifier::DIM),
        (M::ITALIC, Modifier::ITALIC),
        (M::UNDERLINED, Modifier::UNDERLINED),
        (M::RAPID_BLINK, Modifier::RAPID_BLINK),
        (M::REVERSED, Modifier::REVERSED),
        (M::HIDDEN, Modifier::HIDDEN),
        (M::CROSSED_OUT, Modifier::CROSSED_OUT),
    ]
    .into_iter()
    .filter(|(m, _)| modifiers.contains(*m))
    .fold(Modifier::empty(), |all, (_, m)| all | m)
}

/// The terminal style of `style`: its colours where set, its modifiers
/// added.
pub fn style(style: model::Style) -> Style {
    let mut out = Style::new().add_modifier(modifiers(style.modifiers));
    if let Some(fg) = style.fg {
        out = out.fg(color(fg));
    }
    if let Some(bg) = style.bg {
        out = out.bg(color(bg));
    }
    out
}

/// `theme`'s style of `component`.
pub fn of(theme: &Theme, component: Component) -> Style {
    style(theme.style(component))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use tidal_player_core::ui::theme::{
        Modifiers, PALETTE_COLORS, parse_palette_color, parse_style_color,
    };

    /// AC1: the core's palette parsing gives what ratatui's
    /// `Color::from_str` gives, on every row of the core's table and on
    /// every spelling of every name.
    #[test]
    fn ac1_palette_matches_ratatui() {
        let mut rows: Vec<String> = [
            "reset",
            "Reset",
            "black",
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "gray",
            "darkgray",
            "lightred",
            "lightgreen",
            "lightyellow",
            "lightblue",
            "lightmagenta",
            "lightcyan",
            "white",
            "Red",
            "RED",
            "Bright Black",
            "bright_black",
            "BrightBlack",
            "light-red",
            "Light Red",
            "bright_red",
            "bright-magenta",
            "dark_gray",
            "Dark Gray",
            "grey",
            "dark-grey",
            "silver",
            "light gray",
            "light grey",
            "bright white",
            "bright_white",
            "light white",
            "light black",
            "light silver",
            "0",
            "7",
            "255",
            "+7",
            "#ff5555",
            "#FF5555",
            "#1e1F29",
            "#000000",
            "",
            "#fff",
            "#ggg000",
            "#ff55",
            "256",
            "-1",
            "orange",
            "Purpel",
            " red ",
            "re d",
            "#é0000",
        ]
        .map(str::to_owned)
        .to_vec();
        for color in PALETTE_COLORS {
            let key = color.key();
            rows.extend([
                key.to_owned(),
                key.to_uppercase(),
                key.replace('_', " "),
                key.replace('_', "-"),
                key.replace("bright", "light"),
            ]);
        }
        for text in rows {
            // Two of ratatui's names are refused (spec 0017 addendum
            // 67055c40c1e2).
            let want = Color::from_str(&text)
                .ok()
                .filter(|c| !matches!(c, Color::Green | Color::LightGreen));
            assert_eq!(
                parse_palette_color(&text).map(super::color),
                want,
                "{text:?}"
            );
        }
        // A style's hex colour is read the same way.
        for text in ["#50fa7b", "#50FA7B", "#fff", "#ggg000"] {
            let core = parse_style_color(text).map(|c| match c {
                model::StyleColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
                model::StyleColor::Palette(_) => Color::Reset,
            });
            assert_eq!(core, Color::from_str(text).ok(), "{text:?}");
        }
    }

    /// AC8: each colour (the sixteen, indexed, RGB, reset) and each
    /// modifier maps to ratatui's; a style's unset colours stay unset.
    #[test]
    fn ac8_style_mapping() {
        use model::Color as C;
        let colors = [
            (C::Reset, Color::Reset),
            (C::Black, Color::Black),
            (C::Red, Color::Red),
            (C::Green, Color::Green),
            (C::Yellow, Color::Yellow),
            (C::Blue, Color::Blue),
            (C::Magenta, Color::Magenta),
            (C::Cyan, Color::Cyan),
            (C::Gray, Color::Gray),
            (C::DarkGray, Color::DarkGray),
            (C::LightRed, Color::LightRed),
            (C::LightGreen, Color::LightGreen),
            (C::LightYellow, Color::LightYellow),
            (C::LightBlue, Color::LightBlue),
            (C::LightMagenta, Color::LightMagenta),
            (C::LightCyan, Color::LightCyan),
            (C::White, Color::White),
            (C::Indexed(0), Color::Indexed(0)),
            (C::Indexed(200), Color::Indexed(200)),
            (C::Rgb(0x1e, 0x1f, 0x29), Color::Rgb(0x1e, 0x1f, 0x29)),
        ];
        for (core, want) in colors {
            assert_eq!(super::color(core), want, "{core:?}");
        }
        let mods = [
            (Modifiers::BOLD, Modifier::BOLD),
            (Modifiers::DIM, Modifier::DIM),
            (Modifiers::ITALIC, Modifier::ITALIC),
            (Modifiers::UNDERLINED, Modifier::UNDERLINED),
            (Modifiers::RAPID_BLINK, Modifier::RAPID_BLINK),
            (Modifiers::REVERSED, Modifier::REVERSED),
            (Modifiers::HIDDEN, Modifier::HIDDEN),
            (Modifiers::CROSSED_OUT, Modifier::CROSSED_OUT),
            (Modifiers::NONE, Modifier::empty()),
            (
                Modifiers::BOLD | Modifiers::REVERSED,
                Modifier::BOLD | Modifier::REVERSED,
            ),
        ];
        for (core, want) in mods {
            assert_eq!(modifiers(core), want, "{core:?}");
        }
        assert_eq!(style(model::Style::new()), Style::new());
        assert_eq!(
            style(model::Style::new().fg(C::Cyan).add(Modifiers::BOLD)),
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            style(model::Style::new().bg(C::Rgb(1, 2, 3))),
            Style::new().bg(Color::Rgb(1, 2, 3))
        );
    }
}
