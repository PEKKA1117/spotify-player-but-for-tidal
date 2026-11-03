//! Lyrics (spec 0021): the types a lyrics page is made of and the pure
//! LRC parser the player uses to turn Tidal's `subtitles` into timed
//! lines. No I/O.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One timed line of synced lyrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    /// When the line starts, in milliseconds on the wire.
    #[serde(with = "millis")]
    pub at: Duration,
    /// The line's text; empty for an instrumental gap.
    pub text: String,
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

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(duration: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        u64::deserialize(d).map(Duration::from_millis)
    }
}

/// Parses LRC text into timed lines (spec 0021 "Parsing LRC").
///
/// A line is zero or more time stamps (`[mm:ss]`, `[mm:ss.x]`, `[mm:ss.xx]`
/// or `[mm:ss.xxx]`, minutes of any length, seconds under 60), then the
/// text, trimmed; several stamps give one line each. Lines that do not
/// start with a stamp (tags such as `[ar:…]`, plain text) are skipped.
/// The result is sorted by time, stably.
pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut rest = raw.trim_end_matches('\r');
        let mut stamps = Vec::new();
        while let Some((at, after)) = stamp(rest) {
            stamps.push(at);
            rest = after;
        }
        let body = rest.trim();
        lines.extend(stamps.into_iter().map(|at| LyricLine {
            at,
            text: body.to_owned(),
        }));
    }
    lines.sort_by_key(|line| line.at);
    lines
}

/// A time stamp at the start of `text` and what follows it.
fn stamp(text: &str) -> Option<(Duration, &str)> {
    let inner = text.strip_prefix('[')?;
    let (stamp, rest) = inner.split_once(']')?;
    let (minutes, time) = stamp.split_once(':')?;
    let (seconds, fraction) = match time.split_once('.') {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (time, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(minutes) || seconds.len() != 2 || !digits(seconds) {
        return None;
    }
    let millis = match fraction {
        None => 0,
        Some(f) if digits(f) && f.len() <= 3 => {
            // A decimal fraction of a second: `.5` is 500 ms, `.05` 50 ms.
            f.parse::<u64>().ok()? * 10u64.pow(3 - f.len() as u32)
        }
        Some(_) => return None,
    };
    let seconds: u64 = seconds.parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let minutes: u64 = minutes.parse().ok()?;
    let total = minutes
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(1000)?
        .checked_add(millis)?;
    Some((Duration::from_millis(total), rest))
}

/// The index of the last line with `at <= position`; `None` before the
/// first line and for no lines. `lines` is sorted by time.
pub fn current_line(lines: &[LyricLine], position: Duration) -> Option<usize> {
    lines
        .partition_point(|line| line.at <= position)
        .checked_sub(1)
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
