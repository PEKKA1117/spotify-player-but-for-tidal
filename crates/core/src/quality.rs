//! Tidal's audio quality tiers (spec 0003, AC6).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A Tidal audio quality tier, ordered from lowest to highest.
///
/// Serialises as Tidal's names (`LOW`, `HIGH`, `LOSSLESS`,
/// `HI_RES_LOSSLESS`); [`FromStr`] parses the CLI names (`high`,
/// `lossless`, `hi-res`). `low` is rejected as a setting: Tidal serves it as
/// HE-AAC, which the decoder does not support (spec 0003, decision 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AudioQuality {
    /// HE-AAC (`mp4a.40.5`). Never asked for; may still be granted.
    Low,
    /// AAC-LC 320 kbit/s.
    High,
    /// FLAC 16-bit 44.1 kHz.
    Lossless,
    /// FLAC up to 24-bit 192 kHz.
    HiResLossless,
}

impl AudioQuality {
    /// Tidal's name, as sent in `audioquality=` and shown to the user.
    pub fn tidal_name(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::High => "HIGH",
            Self::Lossless => "LOSSLESS",
            Self::HiResLossless => "HI_RES_LOSSLESS",
        }
    }
}

impl fmt::Display for AudioQuality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tidal_name())
    }
}

/// A quality setting that is not one of the CLI names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseQualityError(String);

impl fmt::Display for ParseQualityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseQualityError {}

impl FromStr for AudioQuality {
    type Err = ParseQualityError;

    /// Parses a CLI name: `high`, `lossless` or `hi-res` (any case).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let _ = s;
        Err(ParseQualityError(String::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac6_audio_quality() {
        use AudioQuality::*;

        assert!(Low < High && High < Lossless && Lossless < HiResLossless);
        assert_eq!(
            [HiResLossless, Low, Lossless, High].iter().max(),
            Some(&HiResLossless)
        );

        for (quality, name) in [
            (Low, "LOW"),
            (High, "HIGH"),
            (Lossless, "LOSSLESS"),
            (HiResLossless, "HI_RES_LOSSLESS"),
        ] {
            let json = format!("\"{name}\"");
            assert_eq!(
                serde_json::to_string(&quality).unwrap(),
                json,
                "{quality:?}"
            );
            assert_eq!(
                serde_json::from_str::<AudioQuality>(&json).unwrap(),
                quality
            );
            assert_eq!(quality.to_string(), name);
        }

        for (input, expected) in [
            ("high", High),
            ("lossless", Lossless),
            ("hi-res", HiResLossless),
            ("HI-RES", HiResLossless),
        ] {
            assert_eq!(input.parse::<AudioQuality>(), Ok(expected), "{input}");
        }

        let low = "low".parse::<AudioQuality>().unwrap_err().to_string();
        assert!(
            low.contains("HE-AAC") && low.contains("not supported"),
            "{low}"
        );

        for bad in ["", "hires", "HI_RES_LOSSLESS", "max"] {
            let error = bad.parse::<AudioQuality>().unwrap_err().to_string();
            assert!(error.contains("hi-res, lossless or high"), "{bad}: {error}");
        }
    }
}
