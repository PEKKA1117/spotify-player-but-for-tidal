//! The playback device list of `tidal-player devices` (spec 0003 "Commands",
//! AC25): a pure parser over the text of `/proc/asound/cards` and
//! `/proc/asound/pcm`, so it is tested with fixtures and no sound card.

/// One device `play --device` accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackDevice {
    /// The ALSA PCM name (`default`, `hw:1,0`).
    pub name: String,
    /// What it is, for the user (`E30 II: USB Audio`).
    pub description: String,
}

/// Description of the `default` PCM.
pub const DEFAULT_DESCRIPTION: &str = "shared, through the system mixer";

/// `default` first, then every `hw:C,D` that has a playback stream, in card
/// and device order, described by the card's name and the PCM's name.
pub fn parse_devices(cards: &str, pcm: &str) -> Vec<PlaybackDevice> {
    let _ = (cards, pcm);
    vec![PlaybackDevice {
        name: "default".into(),
        description: DEFAULT_DESCRIPTION.into(),
    }]
}

/// The listing as printed: one device per line, names aligned, the
/// `configured` device (the one `play` would use) marked with `*`.
pub fn format_devices(devices: &[PlaybackDevice], configured: &str) -> String {
    let _ = (devices, configured);
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! fixture {
        ($dir:literal) => {
            (
                include_str!(concat!("../tests/fixtures/asound/", $dir, "/cards")),
                include_str!(concat!("../tests/fixtures/asound/", $dir, "/pcm")),
            )
        };
    }

    fn dev(name: &str, description: &str) -> PlaybackDevice {
        PlaybackDevice {
            name: name.into(),
            description: description.into(),
        }
    }

    #[test]
    fn ac25_parse_devices() {
        let default = || dev("default", DEFAULT_DESCRIPTION);
        let rows = [
            ("no cards", fixture!("no_cards"), vec![default()]),
            (
                "one onboard card",
                fixture!("onboard"),
                vec![
                    default(),
                    dev("hw:0,0", "HDA Intel PCH: ALC892 Analog"),
                    dev("hw:0,1", "HDA Intel PCH: ALC892 Digital"),
                ],
            ),
            (
                "onboard + USB DAC",
                fixture!("onboard_usb"),
                vec![
                    default(),
                    dev("hw:0,0", "HDA Intel PCH: ALC892 Analog"),
                    dev("hw:0,1", "HDA Intel PCH: ALC892 Digital"),
                    dev("hw:1,0", "E30 II: USB Audio"),
                ],
            ),
            (
                "capture-only device not listed",
                fixture!("capture_only"),
                vec![default(), dev("hw:0,0", "HDA Intel PCH: ALC892 Analog")],
            ),
        ];
        for (name, (cards, pcm), want) in rows {
            assert_eq!(parse_devices(cards, pcm), want, "{name}");
        }
    }

    #[test]
    fn ac25_format_marks_configured() {
        let (cards, pcm) = fixture!("onboard_usb");
        let devices = parse_devices(cards, pcm);
        let rows = [
            (
                "default",
                "* default  shared, through the system mixer\n  \
                 hw:0,0   HDA Intel PCH: ALC892 Analog\n  \
                 hw:0,1   HDA Intel PCH: ALC892 Digital\n  \
                 hw:1,0   E30 II: USB Audio\n",
            ),
            (
                "hw:1,0",
                "  default  shared, through the system mixer\n  \
                 hw:0,0   HDA Intel PCH: ALC892 Analog\n  \
                 hw:0,1   HDA Intel PCH: ALC892 Digital\n\
                 * hw:1,0   E30 II: USB Audio\n",
            ),
            (
                // `hw:C` is the card's first device.
                "hw:1",
                "  default  shared, through the system mixer\n  \
                 hw:0,0   HDA Intel PCH: ALC892 Analog\n  \
                 hw:0,1   HDA Intel PCH: ALC892 Digital\n\
                 * hw:1,0   E30 II: USB Audio\n",
            ),
            (
                // Not in the list: nothing is marked.
                "plughw:1,0",
                "  default  shared, through the system mixer\n  \
                 hw:0,0   HDA Intel PCH: ALC892 Analog\n  \
                 hw:0,1   HDA Intel PCH: ALC892 Digital\n  \
                 hw:1,0   E30 II: USB Audio\n",
            ),
        ];
        for (configured, want) in rows {
            assert_eq!(format_devices(&devices, configured), want, "{configured}");
        }
    }
}
