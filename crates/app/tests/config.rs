//! Spec 0008 AC12: a broken `app.toml` or `keymap.toml` stops `daemon`,
//! `play` and the TUI with exit 2 and the message on stderr, before the
//! player's socket or raw mode; a missing directory or file runs with the
//! defaults. Everything is isolated: temp state, runtime and config
//! directories, no keyring, and a proxy that goes nowhere.

use std::path::Path;
use std::time::Duration;

use assert_cmd::Command;

fn bin(tmp: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tidal-player"));
    cmd.env("TIDAL_PLAYER_STATE_DIR", tmp.join("state"))
        .env("TIDAL_PLAYER_RUNTIME_DIR", tmp.join("run"))
        .env("TIDAL_PLAYER_ASOUND_DIR", tmp.join("asound"))
        // Never the developer's cover cache, never the network (0016).
        .env("TIDAL_PLAYER_CACHE_DIR", tmp.join("cache"))
        .env("TIDAL_PLAYER_IMAGES_BASE", "http://127.0.0.1:9")
        .env("TIDAL_PLAYER_NO_KEYRING", "1")
        .env("TIDAL_PLAYER_API_BASE", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
        .env_remove("ALL_PROXY")
        .env_remove("TIDAL_PLAYER_CONFIG_DIR")
        .env_remove("TIDAL_PLAYER_PASSPHRASE_FILE")
        .env_remove("CREDENTIALS_DIRECTORY")
        .env_remove("NOTIFY_SOCKET");
    for var in [
        "QUALITY",
        "DEVICE",
        "VOLUME_STEP",
        "SEEK_STEP",
        "PREVIOUS_RESTART",
        "AUTOPLAY",
        "RELEASE_PAUSED",
        "PAGE_SIZE",
        "SEARCH_PAGE_SIZE",
        "HIDE_VERSIONS",
        "COVER_IMG",
        "COVER_IMG_ROWS",
        "COVER_IMG_PROTOCOL",
    ] {
        cmd.env_remove(format!("TIDAL_PLAYER_{var}"));
    }
    cmd.write_stdin("").timeout(Duration::from_secs(20));
    cmd
}

/// Runs `args` and returns (exit code, stdout, stderr).
fn run(mut cmd: Command) -> (i32, String, String) {
    let out = cmd.output().expect("the command ends by itself");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// AC12: a broken `app.toml` or `keymap.toml` exits 2 with `<path>: <message>` on stderr,
/// for `daemon`, `play` and the TUI, with the folder given by the flag or
/// by the variable; nothing was started (no runtime directory) and the
/// text has no escape sequences.
#[test]
fn ac12_broken_config_exits_2() {
    // (case, file, content, what stderr says after `<path>: `)
    let cases: &[(&str, &str, &[u8], &str)] = &[
        (
            "unknown key",
            "app.toml",
            b"volum_step = 3\n",
            "unknown setting \"volum_step\"",
        ),
        (
            "range",
            "app.toml",
            b"volume_step = 0\n",
            "invalid volume_step: expected an integer from 1 to 25, got 0",
        ),
        (
            "unknown layout key",
            "app.toml",
            b"[layout]\nlibary = { playlist_percent = 30, album_percent = 30 }\n",
            "unknown setting \"layout.libary\"",
        ),
        (
            "syntax",
            "app.toml",
            b"autoplay = true\nvolume_step = \n",
            "line 2, column 15",
        ),
        ("not UTF-8", "app.toml", &[0xff, 0xfe, 0x00], ""),
        // keymap.toml (0008 AC12, keymap half).
        (
            "unknown command",
            "keymap.toml",
            b"[[keymaps]]\ncommand = \"NextTrack\"\nkey_sequence = \"g n\"\n\n\
              [[keymaps]]\ncommand = \"NxtTrack\"\nkey_sequence = \"x\"\n",
            "keymaps[1]: unknown command \"NxtTrack\"",
        ),
        (
            "bad key",
            "keymap.toml",
            b"[[keymaps]]\ncommand = \"Shuffle\"\nkey_sequence = \"ctrl+s\"\n",
            "keymaps[0]: unknown key \"ctrl+s\" in \"ctrl+s\"",
        ),
        (
            "prefix conflict",
            "keymap.toml",
            b"[[keymaps]]\ncommand = \"NextTrack\"\nkey_sequence = \"s\"\n\n\
              [[keymaps]]\ncommand = \"LibraryPage\"\nkey_sequence = \"s l a\"\n",
            "\"s\" is bound to NextTrack and is the start of \"s l a\" (LibraryPage)",
        ),
        (
            "keymap syntax",
            "keymap.toml",
            b"[[keymaps]]\ncommand = \"NextTrack\nkey_sequence = \"n\"\n",
            "line 2, column",
        ),
        (
            "unknown field",
            "keymap.toml",
            b"[[keymaps]]\ncomand = \"NextTrack\"\nkey_sequence = \"n\"\n",
            "comand",
        ),
        (
            "no quit key",
            "keymap.toml",
            b"[[keymaps]]\ncommand = \"None\"\nkey_sequence = \"q\"\n\n\
              [[keymaps]]\ncommand = \"None\"\nkey_sequence = \"C-c\"\n",
            "Quit has no key left",
        ),
    ];
    // How each command gets the folder: flag after the command, or the
    // variable.
    type Invoke = fn(&mut Command, &Path);
    let invocations: &[(&str, Invoke)] = &[
        ("daemon -c", |cmd, dir| {
            cmd.args(["daemon", "-c"]).arg(dir);
        }),
        ("play -c", |cmd, dir| {
            cmd.args(["play", "-c"]).arg(dir).arg("1");
        }),
        ("tui -c", |cmd, dir| {
            cmd.arg("-c").arg(dir);
        }),
        ("daemon, variable", |cmd, dir| {
            cmd.arg("daemon").env("TIDAL_PLAYER_CONFIG_DIR", dir);
        }),
        ("play, variable", |cmd, dir| {
            cmd.args(["play", "1"]).env("TIDAL_PLAYER_CONFIG_DIR", dir);
        }),
        ("tui, variable", |cmd, dir| {
            cmd.env("TIDAL_PLAYER_CONFIG_DIR", dir);
        }),
    ];
    for (case, name, content, message) in cases {
        for (how, invoke) in invocations {
            let tmp = tempfile::tempdir().unwrap();
            let config = tmp.path().join("config");
            std::fs::create_dir(&config).unwrap();
            let file = config.join(name);
            std::fs::write(&file, content).unwrap();
            let mut cmd = bin(tmp.path());
            invoke(&mut cmd, &config);
            let (code, stdout, stderr) = run(cmd);
            let label = format!("{case} / {how}: {stdout:?} {stderr:?}");
            assert_eq!(code, 2, "{label}");
            assert!(
                stderr.starts_with(&format!("{}: ", file.display())),
                "{label}"
            );
            assert!(stderr.contains(message), "{label}");
            assert_eq!(stderr.lines().count(), 1, "{label}");
            assert!(
                !stderr.contains('\u{1b}') && !stdout.contains('\u{1b}'),
                "{label}"
            );
            assert!(!tmp.path().join("run").exists(), "nothing started: {label}");
        }
    }
    // A directory where the file should be.
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    std::fs::create_dir_all(config.join("app.toml")).unwrap();
    let mut cmd = bin(tmp.path());
    cmd.args(["daemon", "-c"]).arg(&config);
    let (code, _, stderr) = run(cmd);
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.starts_with(&format!("{}: ", config.join("app.toml").display())),
        "{stderr}"
    );
}

/// AC12: a missing directory, or a directory without the file, runs with
/// the defaults and creates nothing: `devices` lists, and `daemon` and
/// `play` get as far as the login check (exit 1, not 2).
#[test]
fn ac12_missing_config_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("no-such-dir");
    let empty = tmp.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    for dir in [&missing, &empty] {
        let mut cmd = bin(tmp.path());
        cmd.args(["devices", "-c"]).arg(dir);
        let (code, stdout, stderr) = run(cmd);
        assert_eq!(code, 0, "{stderr}");
        assert!(stdout.contains("* default"), "{stdout}");

        let mut cmd = bin(tmp.path());
        cmd.args(["daemon", "-c"]).arg(dir);
        let (code, _, stderr) = run(cmd);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "Not logged in: run \"tidal-player login\"\n")
        );

        let mut cmd = bin(tmp.path());
        cmd.args(["play", "-c"]).arg(dir).arg("1");
        let (code, _, stderr) = run(cmd);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "Not logged in: run \"tidal-player login\"\n")
        );
    }
    assert!(!missing.exists(), "nothing is created");
    assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);

    // A valid file is used: its output device is the one `devices` marks.
    let config = tmp.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(config.join("app.toml"), "output_device = \"hw:9,9\"\n").unwrap();
    let mut cmd = bin(tmp.path());
    cmd.args(["devices", "-c"]).arg(&config);
    let (code, stdout, stderr) = run(cmd);
    assert_eq!(code, 0, "{stderr}");
    assert!(!stdout.contains("* default"), "{stdout}");
}

/// AC12: a `keymap.toml` with spotify-player commands this player does not
/// have is not an error: `daemon` and `play` get as far as the login check
/// (exit 1, not 2); the TUI shows the notice (the model's
/// `ac12_unsupported_notice` and `ac12_notice_after_welcome`).
#[test]
fn ac12_unsupported_keymap_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("keymap.toml"),
        "[[keymaps]]\ncommand = \"PlayRandom\"\nkey_sequence = \"x\"\n",
    )
    .unwrap();
    for args in [&["daemon", "-c"][..], &["play", "-c"][..]] {
        let mut cmd = bin(tmp.path());
        cmd.args(args).arg(&config);
        if args[0] == "play" {
            cmd.arg("1");
        }
        let (code, _, stderr) = run(cmd);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "Not logged in: run \"tidal-player login\"\n"),
            "{args:?}"
        );
    }
}

/// Spec 0019 AC9: a `keymap.toml` copied from the earlier example binds
/// `RemoveFromQueue` to `d`, which beats the new default `d d`: `daemon` and
/// `play` get as far as the login check (exit 1, not 2).
#[test]
fn ac9_old_keymap_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("keymap.toml"),
        "[[keymaps]]\ncommand = \"RemoveFromQueue\"\nkey_sequence = \"d\"\n",
    )
    .unwrap();
    for args in [&["daemon", "-c"][..], &["play", "-c"][..]] {
        let mut cmd = bin(tmp.path());
        cmd.args(args).arg(&config);
        if args[0] == "play" {
            cmd.arg("1");
        }
        let (code, _, stderr) = run(cmd);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "Not logged in: run \"tidal-player login\"\n"),
            "{args:?}"
        );
    }
}

/// 0016 AC9: the cover's settings are the TUI's: the daemon and `play`
/// accept them in `app.toml` and get as far as the login check; an
/// invalid value in the file or the environment stops them with exit 2,
/// naming the key or the variable.
#[test]
fn ac9_cover_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("app.toml"),
        "cover_img = false\ncover_img_rows = 8\ncover_img_protocol = \"kitty\"\n",
    )
    .unwrap();
    for args in [&["daemon"][..], &["play", "1"][..]] {
        let mut cmd = bin(tmp.path());
        cmd.args(args).arg("-c").arg(&config);
        let (code, _, stderr) = run(cmd);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "Not logged in: run \"tidal-player login\"\n"),
            "{args:?}"
        );
    }
    // (file, variable, value, what stderr names)
    type Bad<'a> = (&'a str, Option<(&'a str, &'a str)>, &'a str);
    let bad: &[Bad] = &[
        ("cover_img_rows = 3\n", None, "invalid cover_img_rows"),
        ("cover_img_rows = 21\n", None, "invalid cover_img_rows"),
        (
            "cover_img_protocol = \"ascii\"\n",
            None,
            "auto, sixel, kitty, iterm2 or halfblocks",
        ),
        (
            "",
            Some(("TIDAL_PLAYER_COVER_IMG_ROWS", "3")),
            "TIDAL_PLAYER_COVER_IMG_ROWS",
        ),
        (
            "",
            Some(("TIDAL_PLAYER_COVER_IMG", "yes")),
            "TIDAL_PLAYER_COVER_IMG",
        ),
        (
            "",
            Some(("TIDAL_PLAYER_COVER_IMG_PROTOCOL", "ascii")),
            "TIDAL_PLAYER_COVER_IMG_PROTOCOL",
        ),
    ];
    for (file, var, message) in bad {
        std::fs::write(config.join("app.toml"), file).unwrap();
        // The TUI exits before raw mode; the daemon reads the file before
        // the session, the variables after it (as every setting's).
        let commands: &[&[&str]] = if var.is_some() {
            &[&[]]
        } else {
            &[&[], &["daemon"]]
        };
        for args in commands {
            let mut cmd = bin(tmp.path());
            cmd.args(*args).arg("-c").arg(&config);
            if let Some((var, value)) = var {
                cmd.env(var, value);
            }
            let (code, stdout, stderr) = run(cmd);
            let label = format!("{file:?} {var:?} {args:?}: {stderr:?}");
            assert_eq!(code, 2, "{label}");
            assert!(stderr.contains(message), "{label}");
            assert!(
                !stdout.contains('\u{1b}') && !stderr.contains('\u{1b}'),
                "{label}"
            );
        }
    }
}

// --- spec 0017 AC7: theme.toml and `theme` ----------------------------------------

/// [`bin`] without the developer's `TIDAL_PLAYER_THEME`.
fn themed_bin(tmp: &Path) -> Command {
    let mut cmd = bin(tmp);
    cmd.env_remove("TIDAL_PLAYER_THEME");
    cmd
}

/// A config directory under `tmp` holding `files` (name, content).
fn config_dir(tmp: &Path, files: &[(&str, &[u8])]) -> std::path::PathBuf {
    let config = tmp.join("config");
    std::fs::create_dir_all(&config).unwrap();
    for (name, content) in files {
        std::fs::write(config.join(name), content).unwrap();
    }
    config
}

/// The TUI got past its config files to the session check: with no
/// session it asks to log in, which fails without the network (not 2).
fn assert_runs(code: i32, stdout: &str, stderr: &str, label: &str) {
    assert_ne!(code, 2, "{label}: {stdout:?} {stderr:?}");
    assert!(
        stdout.contains("Not logged in."),
        "{label}: {stdout:?} {stderr:?}"
    );
    assert!(!stderr.contains("theme"), "{label}: {stderr:?}");
}

/// AC7: a broken `theme.toml` makes the TUI (flag or variable for the
/// folder) exit 2 with `<path>: <message>` on one stderr line, before
/// anything starts and with no escape sequences.
#[test]
fn ac7_broken_theme_exits_2() {
    let cases: &[(&str, &[u8], &str)] = &[
        (
            "syntax",
            b"[[themes]]\nname = \"a\"\n[themes.palette]\nred = #ff5555\n",
            "line 4, column 7",
        ),
        (
            "unknown field",
            b"[[themes]]\nname = \"a\"\n\n[[themes]]\nname = \"b\"\ncolour = \"red\"\n",
            "themes[1]: unknown field \"colour\"",
        ),
        (
            "unknown palette colour",
            b"[[themes]]\nname = \"a\"\n[themes.palette]\norange = \"#ffaa00\"\n",
            "themes[0].palette: unknown colour \"orange\"",
        ),
        (
            "bad palette colour",
            b"[[themes]]\nname = \"a\"\n[themes.palette]\nred = \"#ff55\"\n",
            "themes[0].palette.red: invalid colour \"#ff55\"",
        ),
        (
            "unknown style",
            b"[[themes]]\nname = \"a\"\n[themes.component_style]\nblok_title = { fg = \"Red\" }\n",
            "themes[0].component_style: unknown style \"blok_title\"",
        ),
        (
            "unknown style field",
            b"[[themes]]\nname = \"a\"\n[themes.component_style]\nborder = { colour = \"Red\" }\n",
            "themes[0].component_style.border: unknown field \"colour\", expected fg, bg or modifiers",
        ),
        (
            "bad style colour",
            b"[[themes]]\nname = \"a\"\n[themes.component_style]\nborder = { fg = \"Purpel\" }\n",
            "themes[0].component_style.border.fg: invalid colour \"Purpel\" (a palette name such as BrightBlack, or #rrggbb)",
        ),
        (
            "bad modifier",
            b"[[themes]]\nname = \"a\"\n[themes.component_style]\nselection = { modifiers = [\"Blink\"] }\n",
            "themes[0].component_style.selection: unknown modifier \"Blink\"",
        ),
        (
            "missing name",
            b"[[themes]]\nname = \"a\"\n\n[[themes]]\n[themes.palette]\nred = \"red\"\n",
            "themes[1]: missing name",
        ),
        (
            "name taken",
            b"[[themes]]\nname = \"a\"\n\n[[themes]]\nname = \"a\"\n",
            "themes[1]: \"a\" is already themes[0]",
        ),
        ("not UTF-8", &[0xff, 0xfe, 0x00], ""),
    ];
    for (case, content, message) in cases {
        for how in ["-c", "variable"] {
            let tmp = tempfile::tempdir().unwrap();
            let config = config_dir(tmp.path(), &[("theme.toml", content)]);
            let mut cmd = themed_bin(tmp.path());
            if how == "-c" {
                cmd.arg("-c").arg(&config);
            } else {
                cmd.env("TIDAL_PLAYER_CONFIG_DIR", &config);
            }
            let (code, stdout, stderr) = run(cmd);
            let label = format!("{case} / {how}: {stdout:?} {stderr:?}");
            assert_eq!(code, 2, "{label}");
            assert!(
                stderr.starts_with(&format!("{}: ", config.join("theme.toml").display())),
                "{label}"
            );
            assert!(stderr.contains(message), "{label}");
            assert_eq!(stderr.lines().count(), 1, "{label}");
            assert!(
                !stderr.contains('\u{1b}') && !stdout.contains('\u{1b}'),
                "{label}"
            );
            assert!(!tmp.path().join("run").exists(), "nothing started: {label}");
        }
    }
}

/// AC7: a `theme` no theme has, from `app.toml`, the variable or the flag,
/// makes the TUI exit 2 naming the source and the themes there are.
#[test]
fn ac7_unknown_theme_exits_2() {
    const THEMES: &[u8] = b"[[themes]]\nname = \"dracula\"\n";
    let known = "(themes: default, dracula)";
    // (case, app.toml, theme.toml, variable, flag, the message after the source)
    type Case<'a> = (
        &'a str,
        &'a [u8],
        Option<&'a [u8]>,
        Option<&'a str>,
        &'a [&'a str],
        String,
    );
    let cases: Vec<Case> = vec![
        (
            "app.toml",
            b"theme = \"draculla\"\n",
            Some(THEMES),
            None,
            &[],
            format!("app.toml: unknown theme \"draculla\" {known}"),
        ),
        (
            "app.toml, no theme.toml",
            b"theme = \"dracula\"\n",
            None,
            None,
            &[],
            "app.toml: unknown theme \"dracula\" (themes: default)".to_owned(),
        ),
        (
            "variable",
            b"theme = \"dracula\"\n",
            Some(THEMES),
            Some("nord"),
            &[],
            format!("TIDAL_PLAYER_THEME: unknown theme \"nord\" {known}"),
        ),
        (
            "--theme",
            b"",
            Some(THEMES),
            Some("dracula"),
            &["--theme", "nord"],
            format!("--theme: unknown theme \"nord\" {known}"),
        ),
        (
            "-t",
            b"",
            Some(THEMES),
            None,
            &["-t", "Dracula"],
            format!("--theme: unknown theme \"Dracula\" {known}"),
        ),
    ];
    for (case, app, themes, var, flag, message) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let mut files: Vec<(&str, &[u8])> = vec![("app.toml", app)];
        if let Some(themes) = themes {
            files.push(("theme.toml", themes));
        }
        let config = config_dir(tmp.path(), &files);
        let mut cmd = themed_bin(tmp.path());
        cmd.arg("-c").arg(&config).args(flag);
        if let Some(var) = var {
            cmd.env("TIDAL_PLAYER_THEME", var);
        }
        let (code, stdout, stderr) = run(cmd);
        let label = format!("{case}: {stdout:?} {stderr:?}");
        assert_eq!(code, 2, "{label}");
        assert!(stderr.ends_with(&format!("{message}\n")), "{label}");
        assert_eq!(stderr.lines().count(), 1, "{label}");
        if case.starts_with("app.toml") {
            assert!(
                stderr.starts_with(&format!("{}: ", config.join("app.toml").display())),
                "{label}"
            );
        }
        assert!(!tmp.path().join("run").exists(), "nothing started: {label}");
    }
}

/// AC7: only the TUI reads `theme.toml` and checks the name (decision 5):
/// `daemon` and `play` start with a broken `theme.toml` and an unknown
/// `theme`, and get as far as the login check.
#[test]
fn ac7_daemon_ignores_theme_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_dir(
        tmp.path(),
        &[
            ("app.toml", b"theme = \"draculla\"\n"),
            ("theme.toml", b"[[themes]]\nname = \"a\"\ncolour = 1\n"),
        ],
    );
    for args in [&["daemon"][..], &["play", "1"][..]] {
        let mut cmd = themed_bin(tmp.path());
        cmd.args(args).arg("-c").arg(&config);
        cmd.env("TIDAL_PLAYER_THEME", "nord");
        let (code, _, stderr) = run(cmd);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "Not logged in: run \"tidal-player login\"\n"),
            "{args:?}"
        );
    }
}

/// AC7: `theme` in `app.toml` is a string, for every process.
#[test]
fn ac7_theme_wrong_type_exits_2() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_dir(tmp.path(), &[("app.toml", b"theme = 5\n")]);
    let message = format!(
        "{}: invalid theme: expected a string, got 5\n",
        config.join("app.toml").display()
    );
    for args in [&[][..], &["daemon"][..], &["play", "1"][..]] {
        let mut cmd = themed_bin(tmp.path());
        cmd.args(args).arg("-c").arg(&config);
        let (code, _, stderr) = run(cmd);
        assert_eq!((code, stderr.as_str()), (2, message.as_str()), "{args:?}");
        assert!(
            !tmp.path().join("run").exists(),
            "nothing started: {args:?}"
        );
    }
}

/// AC7: no `theme.toml` runs with `default`; a valid one with `theme`, the
/// variable or the flag naming one of its themes runs too.
#[test]
fn ac7_missing_theme_toml_runs() {
    const THEMES: &[u8] = b"\xef\xbb\xbf[[themes]]\r\nname = \"dracula\"\r\n[themes.component_style]\r\nlyrics_played = { fg = \"Red\" }\r\n";
    // (case, files, the variable, the flag)
    type Case<'a> = (
        &'a str,
        Vec<(&'a str, &'a [u8])>,
        Option<&'a str>,
        &'a [&'a str],
    );
    let cases: Vec<Case> = vec![
        ("no files", vec![], None, &[]),
        (
            "no theme.toml, theme default",
            vec![("app.toml", b"theme = \"default\"\n")],
            None,
            &[],
        ),
        (
            "empty theme.toml",
            vec![("theme.toml", b"# nothing\n")],
            None,
            &[],
        ),
        (
            "theme.toml and theme",
            vec![
                ("app.toml", b"theme = \"dracula\"\n"),
                ("theme.toml", THEMES),
            ],
            None,
            &[],
        ),
        (
            "the variable",
            vec![("theme.toml", THEMES)],
            Some("dracula"),
            &[],
        ),
        (
            "the flag",
            vec![("theme.toml", THEMES)],
            Some("nord"),
            &["-t", "dracula"],
        ),
    ];
    for (case, files, var, flag) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let config = config_dir(tmp.path(), &files);
        let mut cmd = themed_bin(tmp.path());
        cmd.arg("-c").arg(&config).args(flag);
        if let Some(var) = var {
            cmd.env("TIDAL_PLAYER_THEME", var);
        }
        let (code, stdout, stderr) = run(cmd);
        assert_runs(code, &stdout, &stderr, case);
    }
    // A missing directory too.
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = themed_bin(tmp.path());
    cmd.arg("-c").arg(tmp.path().join("no-such-dir"));
    let (code, stdout, stderr) = run(cmd);
    assert_runs(code, &stdout, &stderr, "missing directory");
}

// --- spec 0017 addendum 67055c40c1e2 AC18 -----------------------------------------

/// A `theme.toml` whose second theme has a counted hex in its palette.
const ENVY_THEMES: &[u8] = b"[[themes]]\nname = \"calm\"\n[themes.palette]\nbackground = \"#1e1f29\"\n\n[[themes]]\nname = \"dracula\"\n[themes.palette]\nred = \"#ff5555\"\nyellow = \"#50FA7B\"\n";

/// AC18: the chosen theme with a counted hex ends the TUI at start, from
/// each source: exit 2, one line, before anything starts.
#[test]
fn ac18_envy_theme_exits_2() {
    use tidal_player_core::ui::theme::bombastine::BOMBASTINE;
    // (case, app.toml, the variable, the flag)
    type Case<'a> = (&'a str, &'a [u8], Option<&'a str>, &'a [&'a str]);
    let cases: [Case; 3] = [
        ("app.toml", b"theme = \"dracula\"\n", None, &[]),
        ("the variable", b"", Some("dracula"), &[]),
        ("the flag", b"theme = \"calm\"\n", None, &["-t", "dracula"]),
    ];
    let prefix = "theme.toml: \"dracula\" (#50fa7b): Bombastine: ";
    for (case, app, var, flag) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let config = config_dir(
            tmp.path(),
            &[("app.toml", app), ("theme.toml", ENVY_THEMES)],
        );
        let mut cmd = themed_bin(tmp.path());
        cmd.arg("-c").arg(&config).args(flag);
        if let Some(var) = var {
            cmd.env("TIDAL_PLAYER_THEME", var);
        }
        let (code, stdout, stderr) = run(cmd);
        let label = format!("{case}: {stdout:?} {stderr:?}");
        assert_eq!(code, 2, "{label}");
        assert_eq!(stderr.lines().count(), 1, "{label}");
        let insult = stderr
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix('\n'))
            .unwrap_or_else(|| panic!("{label}"));
        assert!(BOMBASTINE.contains(&insult), "{label}");
        assert!(
            !stderr.contains("panicked") && !stderr.contains("backtrace"),
            "{label}"
        );
        assert!(
            !stdout.contains('\u{1b}') && !stderr.contains('\u{1b}'),
            "{label}"
        );
        assert!(!stdout.contains("Not logged in."), "{label}");
        assert!(!tmp.path().join("run").exists(), "nothing started: {label}");
    }
}

/// AC18: a theme with a counted hex that is not chosen does nothing.
#[test]
fn ac18_unchosen_envy_theme_starts() {
    for app in [&b""[..], b"theme = \"calm\"\n"] {
        let tmp = tempfile::tempdir().unwrap();
        let config = config_dir(
            tmp.path(),
            &[("app.toml", app), ("theme.toml", ENVY_THEMES)],
        );
        let mut cmd = themed_bin(tmp.path());
        cmd.arg("-c").arg(&config);
        let (code, stdout, stderr) = run(cmd);
        assert_runs(code, &stdout, &stderr, &String::from_utf8_lossy(app));
    }
}

/// AC18: `daemon` and `play` do not read `theme.toml`, so the chosen
/// theme's colours change nothing for them.
#[test]
fn ac18_daemon_ignores_envy() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_dir(
        tmp.path(),
        &[
            ("app.toml", b"theme = \"dracula\"\n"),
            ("theme.toml", ENVY_THEMES),
        ],
    );
    for args in [&["daemon"][..], &["play", "1"][..]] {
        let mut cmd = themed_bin(tmp.path());
        cmd.args(args).arg("-c").arg(&config);
        let (code, _, stderr) = run(cmd);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "Not logged in: run \"tidal-player login\"\n"),
            "{args:?}"
        );
    }
}
