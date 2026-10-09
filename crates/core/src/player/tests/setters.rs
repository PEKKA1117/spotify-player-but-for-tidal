//! Spec 0010 AC1 and AC2: the setter commands `Play`, `Pause`, `Stop`,
//! `SetShuffle`, `SetRepeat` and `SetPosition`, against the toggles and seeks
//! of spec 0004.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum St {
    Empty,
    Stopped,
    StoppedAtStart,
    Loading,
    LoadingHeld,
    LoadingReady,
    Opening,
    OpeningHeld,
    Playing,
    Buffering,
    Paused,
    PausedReleased,
}

const ALL: [St; 12] = [
    St::Empty,
    St::Stopped,
    St::StoppedAtStart,
    St::Loading,
    St::LoadingHeld,
    St::LoadingReady,
    St::Opening,
    St::OpeningHeld,
    St::Playing,
    St::Buffering,
    St::Paused,
    St::PausedReleased,
];

/// Queue 1, 2, 3 with track 2 current, in `state`.
fn build(state: St) -> PlayerState {
    let mut st = player();
    if state == St::Empty {
        return st;
    }
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&[1, 2, 3]),
            start: 1,
        },
    );
    let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
    match state {
        St::Empty => unreachable!(),
        St::Stopped | St::StoppedAtStart => {
            let fx = resolved(&mut st, tag);
            assert!(fx.contains(&PlayerEffect::EnginePlay {
                tag,
                start_at: Duration::ZERO
            }));
            started(&mut st, tag);
            position(&mut st, secs(30));
            // A transient failure stops the player where it was.
            engine(
                &mut st,
                EngineEvent::Error {
                    tag,
                    failure: failure(FailureKind::Transient, "Network error"),
                },
            );
            if state == St::StoppedAtStart {
                cmd(&mut st, C::SeekTo(Duration::ZERO));
                st.position = Duration::ZERO;
            }
        }
        St::Loading => {}
        St::LoadingHeld => {
            cmd(&mut st, C::TogglePause);
        }
        St::LoadingReady => {
            cmd(&mut st, C::TogglePause);
            resolved(&mut st, tag);
        }
        St::Opening => {
            resolved(&mut st, tag);
        }
        St::OpeningHeld => {
            resolved(&mut st, tag);
            cmd(&mut st, C::TogglePause);
        }
        St::Playing | St::Buffering | St::Paused | St::PausedReleased => {
            resolved(&mut st, tag);
            started(&mut st, tag);
            position(&mut st, secs(30));
            if state == St::Buffering {
                engine(&mut st, EngineEvent::Buffering);
            }
            if matches!(state, St::Paused | St::PausedReleased) {
                cmd(&mut st, C::TogglePause);
            }
            if state == St::PausedReleased {
                engine(&mut st, EngineEvent::Released);
            }
        }
    }
    st
}

/// Whether `TogglePause` in `state` starts or resumes (else it pauses or
/// does nothing).
fn toggle_starts(state: St) -> bool {
    matches!(
        state,
        St::Stopped
            | St::StoppedAtStart
            | St::LoadingHeld
            | St::LoadingReady
            | St::OpeningHeld
            | St::Paused
            | St::PausedReleased
    )
}

fn toggle_pauses(state: St) -> bool {
    matches!(
        state,
        St::Loading | St::Opening | St::Playing | St::Buffering
    )
}

#[test]
fn ac1_play_pause_stop() {
    for state in ALL {
        let ctx = format!("{state:?}");
        let mut toggled = build(state);
        let toggle_fx = cmd(&mut toggled, C::TogglePause);

        // Play: TogglePause's effects where it starts or resumes.
        let mut st = build(state);
        let before = st.snapshot();
        let fx = cmd(&mut st, C::Play);
        if toggle_starts(state) {
            assert_eq!(fx, toggle_fx, "Play as TogglePause in {ctx}");
            assert_eq!(st.snapshot(), toggled.snapshot(), "Play snapshot in {ctx}");
        } else {
            assert!(fx.is_empty(), "Play in {ctx} must do nothing: {fx:?}");
            assert_eq!(st.snapshot(), before, "Play changed {ctx}");
        }

        // Pause: the reverse.
        let mut st = build(state);
        let fx = cmd(&mut st, C::Pause);
        if toggle_pauses(state) {
            assert_eq!(fx, toggle_fx, "Pause as TogglePause in {ctx}");
            assert_eq!(st.snapshot(), toggled.snapshot(), "Pause snapshot in {ctx}");
        } else {
            assert!(fx.is_empty(), "Pause in {ctx} must do nothing: {fx:?}");
            assert_eq!(st.snapshot(), before, "Pause changed {ctx}");
        }
    }
}

#[test]
fn ac1_stop() {
    for state in ALL {
        let ctx = format!("{state:?}");
        let mut st = build(state);
        let before = st.snapshot();
        let engine_holds = matches!(
            state,
            St::Opening
                | St::OpeningHeld
                | St::Playing
                | St::Buffering
                | St::Paused
                | St::PausedReleased
        );
        let fx = cmd(&mut st, C::Stop);
        let after = st.snapshot();
        assert_eq!(
            fx.contains(&PlayerEffect::EngineStop),
            engine_holds,
            "EngineStop in {ctx}: {fx:?}"
        );
        assert_eq!(after.state, S::Stopped, "state in {ctx}");
        assert_eq!(after.position, Duration::ZERO, "position in {ctx}");
        assert_eq!(after.current, before.current, "current entry in {ctx}");
        assert_eq!(after.queue, before.queue, "queue in {ctx}");
        assert!(after.now_playing.is_none(), "now playing in {ctx}");
        if matches!(state, St::Empty | St::StoppedAtStart) {
            assert!(fx.is_empty(), "Stop in {ctx} is a no-op: {fx:?}");
            assert_eq!(after, before, "Stop changed {ctx}");
        }
        // A following Play starts the entry at 0:00.
        let fx = cmd(&mut st, C::Play);
        if state == St::Empty {
            assert!(fx.is_empty(), "Play with nothing current");
            continue;
        }
        let (entry, _, _) = expect_resolve(&fx, Purpose::Play);
        assert_eq!(Some(entry), before.current, "Play after Stop in {ctx}");
        assert_eq!(st.snapshot().position, Duration::ZERO, "restart in {ctx}");
        let tag = st.current_tag().unwrap();
        let fx = resolved(&mut st, tag);
        assert!(
            fx.contains(&PlayerEffect::EnginePlay {
                tag,
                start_at: Duration::ZERO
            }),
            "Play after Stop starts at 0:00 in {ctx}: {fx:?}"
        );
    }
}

#[test]
fn ac2_set_shuffle_repeat_position() {
    for state in ALL {
        for target in [false, true] {
            let ctx = format!("SetShuffle({target}) in {state:?}");
            let mut a = build(state);
            let mut b = build(state);
            let current = a.snapshot().shuffle;
            let fx_a = cmd(&mut a, C::SetShuffle(target));
            if current != target {
                let fx_b = cmd(&mut b, C::ToggleShuffle);
                assert_eq!(fx_a, fx_b, "{ctx}: effects");
                assert_eq!(a.snapshot(), b.snapshot(), "{ctx}: snapshot");
                assert_eq!(a.snapshot().shuffle, target, "{ctx}");
            } else {
                assert!(fx_a.is_empty(), "{ctx}: must do nothing: {fx_a:?}");
                assert_eq!(a.snapshot(), b.snapshot(), "{ctx}: snapshot");
            }
        }
        // From shuffled too.
        let mut a = build(state);
        cmd(&mut a, C::ToggleShuffle);
        let mut b = a.clone();
        assert!(cmd(&mut a, C::SetShuffle(true)).is_empty());
        let fx_a = cmd(&mut a, C::SetShuffle(false));
        let fx_b = cmd(&mut b, C::ToggleShuffle);
        assert_eq!(fx_a, fx_b, "unshuffle in {state:?}");
        assert_eq!(a.snapshot(), b.snapshot());

        for mode in [RepeatMode::Off, RepeatMode::Queue, RepeatMode::Track] {
            let ctx = format!("SetRepeat({mode:?}) in {state:?}");
            let mut a = build(state);
            let fx_a = cmd(&mut a, C::SetRepeat(mode));
            assert_eq!(a.snapshot().repeat, mode, "{ctx}");
            // The cycle that reaches `mode` gives the same effects and state.
            let mut b = build(state);
            let mut fx_b = Vec::new();
            while b.snapshot().repeat != mode {
                fx_b = cmd(&mut b, C::CycleRepeat);
            }
            assert_eq!(fx_a, fx_b, "{ctx}: effects");
            assert_eq!(a.snapshot(), b.snapshot(), "{ctx}: snapshot");
            // Setting it again does nothing.
            assert!(cmd(&mut a, C::SetRepeat(mode)).is_empty(), "{ctx}: again");
        }
    }
}

#[test]
fn ac2_set_position() {
    for state in ALL {
        let current = build(state).snapshot().current;
        let other = if state == St::Empty {
            EntryId(99)
        } else {
            entry_of(&build(state), 3)
        };
        // (entry, position, same as SeekTo?)
        let rows: Vec<(Option<EntryId>, Duration, bool)> = vec![
            (current, secs(10), current.is_some()),
            (current, Duration::ZERO, current.is_some()),
            (current, LEN, current.is_some()),
            (current, LEN + Duration::from_millis(1), false),
            (Some(other), secs(10), false),
            (Some(EntryId(12345)), secs(10), false),
        ];
        for (entry, position, seeks) in rows {
            let Some(entry) = entry else { continue };
            let ctx = format!("SetPosition({entry:?}, {position:?}) in {state:?}");
            let mut a = build(state);
            let before = a.snapshot();
            let fx = cmd(&mut a, C::SetPosition { entry, position });
            if seeks {
                let mut b = build(state);
                let fx_b = cmd(&mut b, C::SeekTo(position));
                assert_eq!(fx, fx_b, "{ctx}: effects");
                assert_eq!(a.snapshot(), b.snapshot(), "{ctx}: snapshot");
            } else {
                assert!(fx.is_empty(), "{ctx}: must do nothing: {fx:?}");
                assert_eq!(a.snapshot(), before, "{ctx}");
            }
        }
    }
    // The seek really happens: playing, 10 s.
    let mut st = build(St::Playing);
    let entry = st.snapshot().current.unwrap();
    let fx = cmd(
        &mut st,
        C::SetPosition {
            entry,
            position: secs(10),
        },
    );
    assert!(fx.contains(&PlayerEffect::EngineSeek(secs(10))), "{fx:?}");
    assert_eq!(st.snapshot().position, secs(10));
}
