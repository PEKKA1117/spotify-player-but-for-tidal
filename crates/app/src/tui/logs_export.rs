//! Exporting the logs page's lines (spec 0027 "Exporting"): a new file in
//! the cache directory, one line per log line in the log file's format.
//! The TUI writes it, whatever process the player is.

use std::path::Path;
use std::time::SystemTime;

use tidal_player_core::log::LogLine;

use crate::logging::Zone;

/// The export's file name for local time `now` (`offset_secs` from UTC):
/// `tidal-player-logs-YYYYMMDD-HHMMSS.log`, and for the `n`th try after
/// the first (`n` from 2) `…-N.log`.
pub fn export_name(now: SystemTime, offset_secs: i32, n: u32) -> String {
    // Stub (red).
    let _ = (now, offset_secs, n);
    String::new()
}

/// Writes `lines` to a new file in `dir` (created when missing), named for
/// `now` in `zone`, with mode 0600; a name that is taken gets the next
/// `-N`. Returns how many lines it wrote and the file's path; the error is
/// the reason, and nothing is left under the final name.
pub fn export(
    lines: &[LogLine],
    dir: &Path,
    now: SystemTime,
    zone: &Zone,
) -> Result<(usize, String), String> {
    // Stub (red): writes nothing.
    let _ = (dir, now, zone);
    Ok((lines.len(), String::new()))
}

#[cfg(test)]
mod tests;
