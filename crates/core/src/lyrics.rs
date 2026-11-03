//! Lyrics (spec 0021): the types a lyrics page is made of and the pure
//! LRC parser the player uses to turn Tidal's `subtitles` into timed
//! lines. No I/O.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One timed line of synced lyrics.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LyricLine {
    /// When the line starts, in milliseconds on the wire.
    #[serde(with = "millis")]
    pub at: Duration,
    /// The line's text; empty for an instrumental gap.
    pub text: String,
}

// Spec 0021 red stub: writes nothing useful.
impl Serialize for LyricLine {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

/// The lyrics of a track (spec 0021 "Talking to the player").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lyrics {
    /// The timed lines, sorted by time; empty when Tidal has none.
    pub synced: Vec<LyricLine>,
    /// The plain text (LF line breaks, no trailing blank lines).
    pub plain: String,
    /// Whether Tidal marks the text right-to-left.
    pub right_to_left: bool,
}

/// `Duration` as whole milliseconds.
mod millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        u64::deserialize(d).map(Duration::from_millis)
    }
}

/// Parses LRC text into timed lines (spec 0021 "Parsing LRC").
pub fn parse_lrc(_text: &str) -> Vec<LyricLine> {
    Vec::new()
}

/// The index of the last line with `at <= position`; `None` before the
/// first line and for no lines.
pub fn current_line(_lines: &[LyricLine], _position: Duration) -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn line(at: u64, text: &str) -> LyricLine {
        LyricLine {
            at: ms(at),
            text: text.into(),
        }
    }

    /// AC2: LRC variations, one row each.
    #[test]
    fn ac2_parse_lrc() {
        let rows: Vec<(&str, &str, Vec<LyricLine>)> = vec![
            (
                "the probe's form",
                "[00:06.32] We hung the lanterns",
                vec![line(6_320, "We hung the lanterns")],
            ),
            (
                "no space after the stamp",
                "[00:06.32]We hung the lanterns",
                vec![line(6_320, "We hung the lanterns")],
            ),
            ("[mm:ss]", "[01:02] a", vec![line(62_000, "a")]),
            ("[m:ss.x]", "[1:02.5] a", vec![line(62_500, "a")]),
            ("[mm:ss.xx]", "[00:01.05] a", vec![line(1_050, "a")]),
            ("[mm:ss.xxx]", "[00:01.250] a", vec![line(1_250, "a")]),
            (
                "three-digit minutes",
                "[123:00.00] a",
                vec![line(123 * 60_000, "a")],
            ),
            (
                "two stamps on one line",
                "[00:10.00][00:20.00] chorus",
                vec![line(10_000, "chorus"), line(20_000, "chorus")],
            ),
            (
                "metadata tags and stamp-less lines are skipped",
                "[ar:Example Band]\n[offset:0]\njust words\n\n[00:01.00] a",
                vec![line(1_000, "a")],
            ),
            ("seconds of 60 or more are no stamp", "[00:75.00] a", vec![]),
            (
                "a fraction of four digits is no stamp",
                "[00:01.2345] a",
                vec![],
            ),
            (
                "CRLF",
                "[00:01.00] a\r\n[00:02.00] b\r\n",
                vec![line(1_000, "a"), line(2_000, "b")],
            ),
            (
                "a stamp with no text is an empty line",
                "[01:55.15] \n[01:56.00]",
                vec![line(115_150, ""), line(116_000, "")],
            ),
            (
                "text is trimmed",
                "[00:01.00]   padded  ",
                vec![line(1_000, "padded")],
            ),
            (
                "unsorted input is sorted",
                "[00:05.00] b\n[00:01.00] a",
                vec![line(1_000, "a"), line(5_000, "b")],
            ),
            (
                "equal times keep file order",
                "[00:05.00] x\n[00:01.00] a\n[00:05.00] y",
                vec![line(1_000, "a"), line(5_000, "x"), line(5_000, "y")],
            ),
            ("empty input", "", vec![]),
        ];
        for (name, input, want) in rows {
            assert_eq!(parse_lrc(input), want, "{name}");
        }
    }

    /// AC2: the current line.
    #[test]
    fn ac2_current_line() {
        let lines = vec![
            line(1_000, "a"),
            line(2_000, "b"),
            line(2_000, "c"),
            line(5_000, "d"),
        ];
        let rows: Vec<(&str, &[LyricLine], u64, Option<usize>)> = vec![
            ("no lines", &[], 3_000, None),
            ("before the first", &lines, 999, None),
            ("exactly on the first stamp", &lines, 1_000, Some(0)),
            ("between two", &lines, 1_500, Some(0)),
            ("equal stamps: the last of them", &lines, 2_000, Some(2)),
            ("just before the last", &lines, 4_999, Some(2)),
            ("after the last", &lines, 60_000, Some(3)),
        ];
        for (name, lines, position, want) in rows {
            assert_eq!(current_line(lines, ms(position)), want, "{name}");
        }
    }
}
