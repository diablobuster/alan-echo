//! Read Aloud — session state machine and controller.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Idle,
    Picking { reading: bool },
    Warming,
    Reading,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ev {
    Hotkey,
    Click,
    ClickRejected,
    FirstAudio,
    Esc,
    PlayPause,
    Stop,
    End,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    EnterPick,
    ExitPick,
    StartRead,
    ShowWarming,
    ShowReading,
    ShowPaused,
    Teardown,
    Toast(&'static str),
    BumpGen,
}

pub fn step(p: &Phase, ev: Ev) -> (Phase, Vec<Act>) {
    use Act::*;
    use Phase::*;
    match (p, ev) {
        (Idle, Ev::Error | Ev::Stop) => (Idle, vec![]),
        (_, Ev::Error | Ev::Stop) => (Idle, vec![Teardown]),
        (Idle, Ev::Hotkey) => (Picking { reading: false }, vec![EnterPick]),
        (Reading | Paused | Warming, Ev::Hotkey) => (Picking { reading: true }, vec![EnterPick]),
        (Picking { .. }, Ev::Hotkey) => (*p, vec![]),
        (Picking { .. }, Ev::Click) => (Warming, vec![ExitPick, BumpGen, StartRead, ShowWarming]),
        (Picking { .. }, Ev::ClickRejected) => (*p, vec![]),
        (Picking { reading: false }, Ev::Esc) => (Idle, vec![ExitPick, Teardown]),
        // reading carries on (playback never stopped during pick); the pill shows it again
        (Picking { reading: true }, Ev::Esc) => (Reading, vec![ExitPick, ShowReading]),
        (Warming, Ev::FirstAudio) => (Reading, vec![ShowReading]),
        (Reading, Ev::PlayPause) => (Paused, vec![ShowPaused]),
        (Paused, Ev::PlayPause) => (Reading, vec![ShowReading]),
        (Reading | Paused | Warming, Ev::End) => (Idle, vec![Teardown]),
        _ => (*p, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Phase::*;

    #[test]
    fn hotkey_from_idle_enters_pick() {
        let (p, a) = step(&Idle, Ev::Hotkey);
        assert_eq!(p, Picking { reading: false });
        assert!(a.contains(&Act::EnterPick));
    }

    #[test]
    fn picking_click_while_loading_goes_to_warming() {
        let (p, a) = step(&Picking { reading: false }, Ev::Click);
        assert_eq!(p, Warming);
        assert!(a.contains(&Act::StartRead) && a.contains(&Act::ShowWarming) && a.contains(&Act::ExitPick));
        let (p2, a2) = step(&p, Ev::FirstAudio);
        assert_eq!(p2, Reading);
        assert!(a2.contains(&Act::ShowReading));
    }

    #[test]
    fn rejected_click_stays_in_pick() {
        let (p, a) = step(&Picking { reading: false }, Ev::ClickRejected);
        assert_eq!(p, Picking { reading: false });
        assert!(a.is_empty());
    }

    #[test]
    fn esc_in_pick_returns_to_previous() {
        assert_eq!(step(&Picking { reading: false }, Ev::Esc).0, Idle);
        assert_eq!(step(&Picking { reading: true }, Ev::Esc).0, Reading);
    }

    #[test]
    fn jump_bumps_generation() {
        let (p, _) = step(&Reading, Ev::Hotkey);
        assert_eq!(p, Picking { reading: true });
        let (_, a) = step(&p, Ev::Click);
        assert!(a.contains(&Act::BumpGen));
    }

    #[test]
    fn play_pause_toggles() {
        assert_eq!(step(&Reading, Ev::PlayPause).0, Paused);
        assert_eq!(step(&Paused, Ev::PlayPause).0, Reading);
    }

    #[test]
    fn every_error_ends_idle() {
        for p in [Picking { reading: false }, Picking { reading: true }, Warming, Reading, Paused] {
            let (n, a) = step(&p, Ev::Error);
            assert_eq!(n, Idle, "{p:?}");
            assert!(a.contains(&Act::Teardown));
            let (n, a) = step(&p, Ev::Stop);
            assert_eq!(n, Idle);
            assert!(a.contains(&Act::Teardown));
        }
        assert_eq!(step(&Reading, Ev::End).0, Idle);
    }

    #[test]
    fn idle_ignores_controls() {
        for ev in [Ev::PlayPause, Ev::Esc, Ev::Click, Ev::End, Ev::FirstAudio] {
            assert_eq!(step(&Idle, ev).0, Idle);
        }
    }

    #[test]
    fn idle_stop_and_error_do_nothing() {
        assert!(step(&Idle, Ev::Stop).1.is_empty());
        assert!(step(&Idle, Ev::Error).1.is_empty());
    }

    #[test]
    fn esc_while_reading_resumes_and_shows_reading() {
        let (p, a) = step(&Picking { reading: true }, Ev::Esc);
        assert_eq!(p, Reading);
        assert_eq!(a, vec![Act::ExitPick, Act::ShowReading]);
        let (_, a) = step(&Picking { reading: false }, Ev::Esc);
        assert_eq!(a, vec![Act::ExitPick, Act::Teardown]);
    }

    #[test]
    fn hotkey_in_pick_is_ignored() {
        assert_eq!(step(&Picking { reading: true }, Ev::Hotkey), (Picking { reading: true }, vec![]));
    }
}
