//! Exporting the logs page's lines (spec 0027 "Exporting"): a new file in
//! the cache directory, one line per log line in the log file's format.
//! The TUI writes it, whatever process the player is.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tidal_player_core::log::LogLine;

use crate::logging::{Zone, file_line, zoned};

/// The export's file name for local time `now` (`offset_secs` from UTC):
/// `tidal-player-logs-YYYYMMDD-HHMMSS.log`, and for the `n`th try after
/// the first (`n` from 2) `…-N.log`.
pub fn export_name(now: SystemTime, offset_secs: i32, n: u32) -> String {
    let at = zoned(now, offset_secs);
    let suffix = if n > 1 {
        format!("-{n}")
    } else {
        String::new()
    };
    format!(
        "tidal-player-logs-{:04}{:02}{:02}-{:02}{:02}{:02}{suffix}.log",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
    )
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
    let fail = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(dir).map_err(|e| fail(dir, e))?;
    let text: String = lines
        .iter()
        .map(|l| {
            let offset = zone.offset_at(l.at);
            format!("{}\n", file_line(l.at, offset, l.level, &l.target, &l.text))
        })
        .collect();
    // Written whole under a private name first, then linked to the final
    // one: a link never replaces a file, and a failure leaves nothing
    // under the final name.
    let offset = zone.offset_at(now);
    let temp = dir.join(format!(".{}.tmp", export_name(now, offset, 1)));
    let write = || -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&temp);
        return Err(fail(&temp, e));
    }
    let mut n = 1;
    let result = loop {
        let path: PathBuf = dir.join(export_name(now, offset, n));
        match std::fs::hard_link(&temp, &path) {
            Ok(()) => break Ok((lines.len(), path.display().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => break Err(fail(&path, e)),
        }
    };
    let _ = std::fs::remove_file(&temp);
    result
}

#[cfg(test)]
mod tests;
