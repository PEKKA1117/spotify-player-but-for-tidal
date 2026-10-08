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
            b"[[keymaps]]\ncommand = \"NextTrack\"\nkey_sequence = \"g\"\n",
            "\"g\" is bound to NextTrack and is the start of \"g g\" (SelectFirstOrScrollToTop)",
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
