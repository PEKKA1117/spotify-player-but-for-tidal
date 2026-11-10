use super::*;

use tidal_player_core::EntryId;
use tidal_player_core::library::DEFAULT_HIDDEN_VERSIONS;
use tidal_player_core::player::{
    self, EngineEvent, PlayerEffect, PlayerInput, PlayerState, Purpose, TrackDetails,
};
use tidal_player_core::protocol::Command;

use crate::player::runtime::fakes::track;

const SEED: u64 = 42;

#[test]
fn ac26_settings_precedence() {
    type Row = (
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        &'static [(&'static str, &'static str)],
        Result<(AudioQuality, &'static str), &'static str>,
    );
    let rows: &[Row] = &[
        (
            "defaults",
            None,
            None,
            &[],
            Ok((AudioQuality::HiResLossless, "default")),
        ),
        (
            "environment",
            None,
            None,
            &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
            Ok((AudioQuality::High, "hw:1,0")),
        ),
        (
            "flags beat environment",
            Some("lossless"),
            Some("hw:2,0"),
            &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
            Ok((AudioQuality::Lossless, "hw:2,0")),
        ),
        (
            "empty environment counts as unset",
            None,
            None,
            &[(QUALITY_VAR, ""), (DEVICE_VAR, "")],
            Ok((AudioQuality::HiResLossless, "default")),
        ),
        (
            "flag beats a bad environment value",
            Some("hi-res"),
            None,
            &[(QUALITY_VAR, "ultra")],
            Ok((AudioQuality::HiResLossless, "default")),
        ),
        ("unknown flag", Some("ultra"), None, &[], Err("--quality")),
        ("low flag", Some("low"), None, &[], Err("--quality")),
        (
            "unknown environment",
            None,
            None,
            &[(QUALITY_VAR, "ultra")],
            Err(QUALITY_VAR),
        ),
        (
            "low environment",
            None,
            None,
            &[(QUALITY_VAR, "low")],
            Err(QUALITY_VAR),
        ),
    ];
    for (name, quality, device, env, want) in rows {
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        };
        let got = resolve_settings(*quality, *device, lookup);
        match want {
            Ok((q, d)) => assert_eq!(
                got,
                Ok(Settings {
                    quality: *q,
                    device: (*d).into()
                }),
                "{name}"
            ),
            Err(setting) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, *setting, "{name}");
                if quality == &Some("low") || env.iter().any(|(_, v)| *v == "low") {
                    assert!(err.to_string().contains("HE-AAC"), "{name}: {err}");
                }
            }
        }
    }
    assert_eq!(configured_device(|_| None), "default");
    assert_eq!(
        configured_device(|k| (k == DEVICE_VAR).then(|| "hw:1,0".into())),
        "hw:1,0"
    );
}
/// AC25: the player settings from the environment, their ranges, and
/// `--autoplay` over the environment.
#[test]
fn ac25_player_config() {
    type Want = Result<(u8, u64, u64, bool), (&'static str, &'static str)>;
    type Env = &'static [(&'static str, &'static str)];
    let rows: &[(&str, Env, Want)] = &[
        ("defaults", &[], Ok((5, 5, 3, false))),
        (
            "empty counts as unset",
            &[
                (VOLUME_STEP_VAR, ""),
                (SEEK_STEP_VAR, ""),
                (PREVIOUS_RESTART_VAR, ""),
                (AUTOPLAY_VAR, ""),
            ],
            Ok((5, 5, 3, false)),
        ),
        (
            "lowest values",
            &[
                (VOLUME_STEP_VAR, "1"),
                (SEEK_STEP_VAR, "1"),
                (PREVIOUS_RESTART_VAR, "0"),
                (AUTOPLAY_VAR, "off"),
            ],
            Ok((1, 1, 0, false)),
        ),
        (
            "highest values",
            &[
                (VOLUME_STEP_VAR, "25"),
                (SEEK_STEP_VAR, "600"),
                (PREVIOUS_RESTART_VAR, "60"),
                (AUTOPLAY_VAR, "on"),
            ],
            Ok((25, 600, 60, true)),
        ),
        (
            "volume step 0",
            &[(VOLUME_STEP_VAR, "0")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "volume step 26",
            &[(VOLUME_STEP_VAR, "26")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "volume step text",
            &[(VOLUME_STEP_VAR, "five")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "volume step negative",
            &[(VOLUME_STEP_VAR, "-5")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "seek step 0",
            &[(SEEK_STEP_VAR, "0")],
            Err((SEEK_STEP_VAR, "1 to 600")),
        ),
        (
            "seek step 601",
            &[(SEEK_STEP_VAR, "601")],
            Err((SEEK_STEP_VAR, "1 to 600")),
        ),
        (
            "seek step fraction",
            &[(SEEK_STEP_VAR, "2.5")],
            Err((SEEK_STEP_VAR, "1 to 600")),
        ),
        (
            "previous 61",
            &[(PREVIOUS_RESTART_VAR, "61")],
            Err((PREVIOUS_RESTART_VAR, "0 to 60")),
        ),
        (
            "previous text",
            &[(PREVIOUS_RESTART_VAR, "3s")],
            Err((PREVIOUS_RESTART_VAR, "0 to 60")),
        ),
        (
            "autoplay yes",
            &[(AUTOPLAY_VAR, "yes")],
            Err((AUTOPLAY_VAR, "on or off")),
        ),
        (
            "autoplay 1",
            &[(AUTOPLAY_VAR, "1")],
            Err((AUTOPLAY_VAR, "on or off")),
        ),
    ];
    let lookup = |env: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    };
    for (name, env, want) in rows {
        let got = resolve_player_config(lookup(env));
        match want {
            Ok((volume, seek, previous, autoplay)) => {
                let got = got.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(
                    got.steps,
                    Steps {
                        volume: *volume,
                        seek: Duration::from_secs(*seek)
                    },
                    "{name}"
                );
                assert_eq!(
                    got.player,
                    PlayerConfig {
                        previous_restart: Duration::from_secs(*previous),
                        autoplay: *autoplay,
                        country: None,
                        device: "default".into(),
                    },
                    "{name}"
                );
            }
            Err((var, range)) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, *var, "{name}");
                let text = err.to_string();
                assert!(text.contains(var) && text.contains(range), "{name}: {text}");
            }
        }
    }

    // 0005 AC23: the release delay, 0–3600 s or `never`.
    // Ok(None): never; Err: refused.
    type ReleaseWant = Result<Option<u64>, ()>;
    let release: &[(&str, Option<&str>, ReleaseWant)] = &[
        ("unset", None, Ok(Some(10))),
        ("empty counts as unset", Some(""), Ok(Some(10))),
        ("at once", Some("0"), Ok(Some(0))),
        ("an hour", Some("3600"), Ok(Some(3600))),
        ("never", Some("never"), Ok(None)),
        ("over an hour", Some("3601"), Err(())),
        ("negative", Some("-1"), Err(())),
        ("text", Some("x"), Err(())),
    ];
    for (name, value, want) in release {
        let got = resolve_player_config(|key: &str| {
            (key == RELEASE_PAUSED_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        match want {
            Ok(secs) => assert_eq!(
                got.map(|s| s.release_paused).map_err(|e| e.to_string()),
                Ok(secs.map(Duration::from_secs)),
                "release delay: {name}"
            ),
            Err(()) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, RELEASE_PAUSED_VAR, "{name}");
                let text = err.to_string();
                assert!(
                    text.contains(RELEASE_PAUSED_VAR)
                        && text.contains("0 to 3600")
                        && text.contains("never"),
                    "{name}: {text}"
                );
            }
        }
    }

    // 0006 AC3: the page size (empty = unset) and the hidden words
    // (empty = hide nothing).
    let default_words: Vec<String> = DEFAULT_HIDDEN_VERSIONS
        .iter()
        .map(|w| (*w).to_owned())
        .collect();
    let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    type PageWant = Result<u32, ()>;
    let pages: &[(&str, Option<&str>, PageWant)] = &[
        ("unset", None, Ok(100)),
        ("empty counts as unset", Some(""), Ok(100)),
        ("lowest", Some("1"), Ok(1)),
        ("highest", Some("10000"), Ok(10000)),
        ("zero", Some("0"), Err(())),
        ("over", Some("10001"), Err(())),
        ("text", Some("x"), Err(())),
    ];
    for (name, value, want) in pages {
        let got = resolve_player_config(|key: &str| {
            (key == PAGE_SIZE_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        match want {
            Ok(n) => assert_eq!(
                got.map(|s| s.library.page_size).map_err(|e| e.to_string()),
                Ok(*n),
                "page size: {name}"
            ),
            Err(()) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, PAGE_SIZE_VAR, "{name}");
                let text = err.to_string();
                assert!(
                    text.contains(PAGE_SIZE_VAR) && text.contains("1 to 10000"),
                    "{name}: {text}"
                );
            }
        }
    }
    // 0007 AC1: the search page size (empty = unset).
    let search_pages: &[(&str, Option<&str>, PageWant)] = &[
        ("unset", None, Ok(20)),
        ("empty counts as unset", Some(""), Ok(20)),
        ("lowest", Some("1"), Ok(1)),
        ("highest", Some("1000"), Ok(1000)),
        ("zero", Some("0"), Err(())),
        ("over", Some("1001"), Err(())),
        ("text", Some("x"), Err(())),
    ];
    for (name, value, want) in search_pages {
        let got = resolve_player_config(|key: &str| {
            (key == SEARCH_PAGE_SIZE_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        match want {
            Ok(n) => assert_eq!(
                got.map(|s| s.library.search_page_size)
                    .map_err(|e| e.to_string()),
                Ok(*n),
                "search page size: {name}"
            ),
            Err(()) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, SEARCH_PAGE_SIZE_VAR, "{name}");
                let text = err.to_string();
                assert!(
                    text.contains(SEARCH_PAGE_SIZE_VAR) && text.contains("1 to 1000"),
                    "{name}: {text}"
                );
            }
        }
    }
    let hidden: &[(&str, Option<&str>, Vec<String>)] = &[
        ("unset", None, default_words),
        ("empty hides nothing", Some(""), Vec::new()),
        ("one word", Some("remix"), words(&["remix"])),
        (
            "trimmed list",
            Some(" live , demo,,  8d audio "),
            words(&["live", "demo", "8d audio"]),
        ),
    ];
    for (name, value, want) in hidden {
        let got = resolve_player_config(|key: &str| {
            (key == HIDE_VERSIONS_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        assert_eq!(
            got.map(|s| s.library.hidden_words)
                .map_err(|e| e.to_string()),
            Ok(want.clone()),
            "hidden words: {name}"
        );
    }

    // `play --autoplay` beats the environment, even a bad value.
    let precedence: &[(bool, Env, bool)] = &[
        (true, &[(AUTOPLAY_VAR, "off")], true),
        (true, &[(AUTOPLAY_VAR, "maybe")], true),
        (false, &[(AUTOPLAY_VAR, "on")], true),
        (false, &[(AUTOPLAY_VAR, "")], false),
    ];
    for (flag, env, want) in precedence {
        let got = resolve_play_config(*flag, lookup(env)).expect("valid");
        assert_eq!(got.player.autoplay, *want, "--autoplay {flag}, {env:?}");
    }

    // The player's previous uses the configured threshold: at 0:05,
    // 10 s goes back to the entry before, the default 3 s restarts.
    let previous_at_5s = |threshold: &'static str| {
        let settings = resolve_player_config(lookup(match threshold {
            "10" => &[(PREVIOUS_RESTART_VAR, "10")],
            _ => &[],
        }))
        .unwrap();
        let mut state = PlayerState::new(settings.player, SEED);
        let tracks = vec![track(1, Some(200)), track(2, Some(200))];
        let fx = player::update(
            &mut state,
            PlayerInput::Command(Command::LoadQueue { tracks, start: 1 }),
        );
        let Some(PlayerEffect::Resolve { tag, .. }) = fx.first().cloned() else {
            panic!("no resolve: {fx:?}");
        };
        player::update(
            &mut state,
            PlayerInput::Resolved {
                tag,
                result: Ok(AudioQuality::Lossless),
            },
        );
        let details = TrackDetails {
            source: "FLAC".into(),
            output: "hw:1,0".into(),
            bit_perfect: true,
            reason: None,
        };
        player::update(
            &mut state,
            PlayerInput::Engine(EngineEvent::Started { tag, details }),
        );
        player::update(
            &mut state,
            PlayerInput::Engine(EngineEvent::Position(Duration::from_secs(5))),
        );
        player::update(&mut state, PlayerInput::Command(Command::Previous))
    };
    let back = previous_at_5s("10");
    assert!(
        back.iter().any(|e| matches!(
            e,
            PlayerEffect::Resolve {
                entry: EntryId(1),
                purpose: Purpose::Play,
                ..
            }
        )),
        "10 s threshold: {back:?}"
    );
    let restart = previous_at_5s("default");
    assert!(
        restart.contains(&PlayerEffect::EngineSeek(Duration::ZERO)),
        "3 s threshold: {restart:?}"
    );
}

/// Spec 0009 AC12: the autoplay a player starts with: flag > environment
/// > remembered > `app.toml` > default.
#[test]
fn ac12_autoplay_precedence() {
    type Env = &'static [(&'static str, &'static str)];
    // name, app.toml, environment, --autoplay, remembered, want
    type Row = (&'static str, &'static str, Env, bool, Option<bool>, bool);
    let rows: &[Row] = &[
        ("default", "", &[], false, None, false),
        ("app.toml", "autoplay = true", &[], false, None, true),
        (
            "remembered over app.toml (off)",
            "autoplay = true",
            &[],
            false,
            Some(false),
            false,
        ),
        (
            "remembered over app.toml (on)",
            "autoplay = false",
            &[],
            false,
            Some(true),
            true,
        ),
        (
            "remembered over the default",
            "",
            &[],
            false,
            Some(true),
            true,
        ),
        (
            "environment over remembered (off)",
            "autoplay = true",
            &[(AUTOPLAY_VAR, "off")],
            false,
            Some(true),
            false,
        ),
        (
            "environment over remembered (on)",
            "",
            &[(AUTOPLAY_VAR, "on")],
            false,
            Some(false),
            true,
        ),
        (
            "empty environment falls through to remembered",
            "autoplay = true",
            &[(AUTOPLAY_VAR, "")],
            false,
            Some(false),
            false,
        ),
        (
            "--autoplay over all",
            "autoplay = false",
            &[(AUTOPLAY_VAR, "off")],
            true,
            Some(false),
            true,
        ),
    ];
    for (name, file, env, flag, remembered, want) in rows {
        let file = crate::config::parse_app_toml(std::path::Path::new("/c/app.toml"), file)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let env = |key: &str| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        };
        let settings =
            resolve_play_config_with(&file, *flag, env).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(start_autoplay(&settings, *remembered), *want, "{name}");
    }
}
