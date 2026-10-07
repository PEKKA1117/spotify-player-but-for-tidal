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
    let card_names: Vec<(u32, &str)> = cards.lines().filter_map(parse_card_line).collect();
    let mut playback: Vec<(u32, u32, &str)> = pcm.lines().filter_map(parse_pcm_line).collect();
    playback.sort_by_key(|&(card, device, _)| (card, device));
    let mut devices = vec![PlaybackDevice {
        name: "default".into(),
        description: DEFAULT_DESCRIPTION.into(),
    }];
    devices.extend(playback.into_iter().map(|(card, device, pcm_name)| {
        let card_name = card_names
            .iter()
            .find(|(n, _)| *n == card)
            .map(|(_, name)| *name);
        PlaybackDevice {
            name: format!("hw:{card},{device}"),
            description: match card_name {
                Some(card_name) => format!("{card_name}: {pcm_name}"),
                None => pcm_name.to_owned(),
            },
        }
    }));
    devices
}

/// ` 1 [DAC            ]: USB-Audio - E30 II` → `(1, "E30 II")`. The
/// second line of each card (its long name) does not start with a number.
fn parse_card_line(line: &str) -> Option<(u32, &str)> {
    let (number, rest) = line.trim_start().split_once(' ')?;
    let number = number.parse().ok()?;
    let (_, after_id) = rest.split_once("]: ")?;
    let name = after_id
        .split_once(" - ")
        .map_or(after_id, |(_, name)| name)
        .trim();
    Some((number, name))
}

/// `01-00: USB Audio : USB Audio : playback 1` → `(1, 0, "USB Audio")`;
/// `None` for a device without a playback stream.
fn parse_pcm_line(line: &str) -> Option<(u32, u32, &str)> {
    let (address, rest) = line.split_once(": ")?;
    let (card, device) = address.split_once('-')?;
    let fields: Vec<&str> = rest.split(" : ").map(str::trim).collect();
    if !fields.iter().any(|f| f.starts_with("playback")) {
        return None;
    }
    let name = fields
        .get(1)
        .or(fields.first())
        .copied()
        .unwrap_or_default();
    Some((card.parse().ok()?, device.parse().ok()?, name))
}

/// The listing as printed: one device per line, names aligned, the
/// `configured` device (the one `play` would use) marked with `*`.
pub fn format_devices(devices: &[PlaybackDevice], configured: &str) -> String {
    let configured = normalise(configured);
    let width = devices.iter().map(|d| d.name.len()).max().unwrap_or(0);
    devices
        .iter()
        .map(|d| {
            let mark = if d.name == configured { '*' } else { ' ' };
            format!("{mark} {:<width$}  {}\n", d.name, d.description)
        })
        .collect()
}

/// `hw:C` names the card's first device, `hw:C,0`.
fn normalise(device: &str) -> String {
    match device.strip_prefix("hw:") {
        Some(card) if !card.contains(',') => format!("hw:{card},0"),
        _ => device.to_owned(),
    }
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
