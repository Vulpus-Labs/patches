//! Ratatui frontend for `patch_player` (ticket 0704, ADR 0055 §5).
//!
//! Layout:
//! - Header: patch path / sample rate / oversampling / engine state.
//! - Meter pane: one peak+RMS bar pair per declared meter tap, dB-coloured.
//! - Event log pane: scrolling log; halt + reload outcomes routed here.
//! - Footer: keybindings.

mod render;
mod run;
mod state;

pub use run::{enter_terminal, leave_terminal, run};
pub use state::{format_diagnostic, taps_from_manifest, EngineState, HeaderInfo, RecordState, View};

#[cfg(test)]
mod tests {
    use super::render::{format_db, group_meter_rows, truncate_name, visible_rows, MeterRow, METRIC_W};
    use super::state::*;
    use std::time::{Duration, Instant};

    use patches_core::provenance::Provenance;
    use patches_core::Span;
    use patches_core::TapBlockFrame;
    use patches_dsl::manifest::{TapDescriptor, TapType};
    use patches_observation::subscribers::{Diagnostic, Subscribers};
    use patches_observation::tap_ring;

    fn header() -> HeaderInfo {
        HeaderInfo {
            patch_path: "x.patches".into(),
            sample_rate: 48_000,
            oversampling: 1,
        }
    }

    fn record() -> RecordState {
        RecordState { record_path: None, muted: None }
    }

    fn desc(slot: usize, name: &str, comp: TapType) -> TapDescriptor {
        TapDescriptor {
            slot,
            width: 1,
            name: name.into(),
            components: vec![comp],
            source: Provenance::root(Span::synthetic()),
        }
    }

    #[test]
    fn taps_from_manifest_sorts_by_slot() {
        let m = vec![
            desc(2, "b", TapType::Meter),
            desc(0, "a", TapType::Meter),
        ];
        let taps = taps_from_manifest(&m);
        assert_eq!(taps[0].slot, 0);
        assert_eq!(taps[0].name, "a");
        assert_eq!(taps[1].slot, 2);
    }

    #[test]
    fn format_diagnostic_renders_unsupported_component() {
        let d = Diagnostic::NotYetImplemented {
            slot: 3,
            tap_name: "scope".into(),
            component: TapType::Spectrum,
        };
        assert_eq!(
            format_diagnostic(&d),
            "tap `scope` (`spectrum`): not yet implemented"
        );
    }

    #[test]
    fn poll_drops_logs_advance_and_rate_limits_repeats() {
        let (mut tx, _rx) = tap_ring(1);
        let (subs, _diag) = Subscribers::new(tx.shared(), 8);
        let handle = subs.handle();

        let mut view = View::new(
            header(),
            vec![TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] }],
            record(),
        );
        let t0 = Instant::now();
        view.poll_drops(&handle, t0);
        assert!(view.log.is_empty(), "no advance yet, no log");

        let frame = TapBlockFrame::zeroed();
        assert!(tx.try_push_frame(&frame));
        assert!(!tx.try_push_frame(&frame));
        assert!(handle.dropped(0) > 0);

        view.poll_drops(&handle, t0);
        assert_eq!(view.log.lines.len(), 1, "first advance logs");

        assert!(!tx.try_push_frame(&frame));
        view.poll_drops(&handle, t0);
        assert_eq!(view.log.lines.len(), 1, "rate-limited, still one line");

        let later = t0 + DROP_LOG_INTERVAL + Duration::from_millis(1);
        assert!(!tx.try_push_frame(&frame));
        view.poll_drops(&handle, later);
        assert_eq!(view.log.lines.len(), 2, "second advance logs after interval");
    }

    #[test]
    fn set_taps_clamps_meter_scroll_and_keeps_baseline_for_surviving_names() {
        let mut view = View::new(
            header(),
            vec![
                TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] },
                TapEntry { name: "b".into(), slot: 1, components: vec![TapType::Meter] },
                TapEntry { name: "c".into(), slot: 2, components: vec![TapType::Meter] },
            ],
            record(),
        );
        view.meter_scroll = 2;
        view.drop_seen.insert("a".into(), 7);
        view.drop_seen.insert("b".into(), 11);

        view.set_taps(vec![TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] }]);
        assert_eq!(view.taps.len(), 1);
        assert_eq!(view.meter_scroll, 0);
        assert_eq!(view.drop_seen.get("a").copied(), Some(7));
        assert!(!view.drop_seen.contains_key("b"));
    }

    #[test]
    fn rename_under_same_slot_is_treated_as_fresh() {
        let (_tx, _rx) = tap_ring(4);
        let (subs, _diag) = Subscribers::new(_tx.shared(), 8);
        let handle = subs.handle();

        let mut view = View::new(
            header(),
            vec![TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] }],
            record(),
        );
        view.drop_seen.insert("a".into(), 7);

        view.set_taps(vec![TapEntry { name: "z".into(), slot: 0, components: vec![TapType::Meter] }]);
        view.seed_drop_baselines(&handle);
        view.poll_drops(&handle, Instant::now());
        assert!(view.log.is_empty(), "rename should not log spurious drops");
        assert!(!view.drop_seen.contains_key("a"));
    }

    #[test]
    fn truncate_name_short_pass_through() {
        assert_eq!(truncate_name("foo", 16), "foo");
    }

    #[test]
    fn truncate_name_exact_pass_through() {
        assert_eq!(truncate_name("0123456789abcdef", 16), "0123456789abcdef");
    }

    #[test]
    fn truncate_name_long_appends_ellipsis() {
        assert_eq!(truncate_name("0123456789abcdefghij", 16), "0123456789abcde…");
    }

    #[test]
    fn format_db_floor_is_inf() {
        let s = format_db(super::render::DB_FLOOR);
        assert_eq!(s.chars().count(), METRIC_W as usize);
        assert!(s.contains("-inf"));
    }

    #[test]
    fn format_db_normal_value_fits_metric_width() {
        let s = format_db(-12.3);
        assert_eq!(s.chars().count(), METRIC_W as usize);
    }

    #[test]
    fn visible_rows_at_least_one() {
        assert_eq!(visible_rows(0), 1);
        assert_eq!(visible_rows(1), 1);
        assert_eq!(visible_rows(20), 20);
    }

    #[test]
    fn meter_scroll_clamped_when_taps_shrink() {
        let mut view = View::new(
            header(),
            (0..8).map(|i| TapEntry { name: format!("t{i}"), slot: i, components: vec![TapType::Meter] }).collect(),
            record(),
        );
        view.meter_scroll = 7;
        view.set_taps(vec![TapEntry { name: "t0".into(), slot: 0, components: vec![TapType::Meter] }]);
        assert_eq!(view.meter_scroll, 0);
    }

    #[test]
    fn tab_cycles_events_first() {
        let mut t = Tab::Events;
        t = t.next();
        assert_eq!(t, Tab::Meters);
        t = t.next();
        assert_eq!(t, Tab::Spectrum);
        t = t.next();
        assert_eq!(t, Tab::Scope);
        t = t.next();
        assert_eq!(t, Tab::Cpu);
        t = t.next();
        assert_eq!(t, Tab::Events);
    }

    #[test]
    fn taps_from_manifest_carries_components() {
        let m = vec![
            desc(0, "a", TapType::Meter),
            desc(1, "b", TapType::Spectrum),
        ];
        let taps = taps_from_manifest(&m);
        assert!(taps[0].has(TapType::Meter));
        assert!(!taps[0].has(TapType::Spectrum));
        assert!(taps[1].has(TapType::Spectrum));
    }

    #[test]
    fn re_added_name_does_not_inherit_predecessors_count() {
        let (mut tx, _rx) = tap_ring(1);
        let (subs, _diag) = Subscribers::new(tx.shared(), 8);
        let handle = subs.handle();

        let frame = patches_core::TapBlockFrame::zeroed();
        assert!(tx.try_push_frame(&frame));
        for _ in 0..5 {
            assert!(!tx.try_push_frame(&frame));
        }
        assert!(handle.dropped(0) >= 5);

        let mut view = View::new(
            header(),
            vec![TapEntry { name: "b".into(), slot: 0, components: vec![TapType::Meter] }],
            record(),
        );
        view.seed_drop_baselines(&handle);
        view.poll_drops(&handle, Instant::now());
        assert!(view.log.is_empty(), "fresh name should not inherit predecessor drops");
    }

    #[test]
    fn group_meter_rows_pairs_stereo_meter_halves_by_stem() {
        let taps = [
            TapEntry { name: "kick".into(), slot: 0, components: vec![TapType::Meter] },
            TapEntry { name: "master/left".into(),  slot: 1, components: vec![TapType::StereoMeter] },
            TapEntry { name: "master/right".into(), slot: 2, components: vec![TapType::StereoMeter] },
            TapEntry { name: "snare".into(), slot: 3, components: vec![TapType::Meter] },
        ];
        let refs: Vec<&TapEntry> = taps.iter().collect();
        let rows = group_meter_rows(&refs);
        assert_eq!(rows.len(), 4, "no rows lost");
        assert!(matches!(rows[0], MeterRow::Mono(_)));
        assert!(matches!(rows[1], MeterRow::StereoLeft { stem: "master", .. }));
        assert!(matches!(rows[2], MeterRow::StereoRight(_)));
        assert!(matches!(rows[3], MeterRow::Mono(_)));
        // ▎L / ▎R ticks right-align to the bar edge so the two halves line up.
        assert_eq!(rows[1].label(16), "master        ▎L");
        assert_eq!(rows[2].label(16), "              ▎R");
    }

    #[test]
    fn group_meter_rows_does_not_pair_unrelated_left_right() {
        let taps = [
            TapEntry { name: "a/left".into(),  slot: 0, components: vec![TapType::Meter] },
            TapEntry { name: "a/right".into(), slot: 1, components: vec![TapType::Meter] },
        ];
        let refs: Vec<&TapEntry> = taps.iter().collect();
        let rows = group_meter_rows(&refs);
        assert_eq!(rows.len(), 2);
        for r in &rows {
            assert!(matches!(r, MeterRow::Mono(_)));
        }
    }

    #[test]
    fn format_hms_zero_and_wraparound() {
        assert_eq!(format_hms(0), "00:00:00");
        assert_eq!(format_hms(3661), "01:01:01");
        assert_eq!(format_hms(86_400), "00:00:00");
        assert_eq!(format_hms(86_399), "23:59:59");
    }

    #[test]
    fn wrap_with_prefix_short_message_one_line() {
        let lines = wrap_with_prefix("12:34:56 ", "hi", 40);
        assert_eq!(lines, vec!["12:34:56 hi"]);
    }

    #[test]
    fn wrap_with_prefix_wraps_and_indents_continuation() {
        let lines = wrap_with_prefix("12:34:56 ", "alpha bravo charlie delta", 20);
        assert!(lines.len() >= 2, "expected wrap, got {lines:?}");
        assert!(lines[0].starts_with("12:34:56 "));
        for cont in &lines[1..] {
            assert!(cont.starts_with("         "), "continuation not indented: {cont:?}");
        }
    }

    #[test]
    fn wrap_with_prefix_hard_splits_long_word() {
        let lines = wrap_with_prefix("> ", "abcdefghij", 6);
        assert_eq!(lines, vec!["> abcd", "  efgh", "  ij"]);
    }

    #[test]
    fn event_log_push_stamps_timestamp() {
        let mut log = EventLog::new(4);
        log.push_at(3661, "hello");
        let e = log.lines.front().unwrap();
        assert_eq!(e.epoch_secs, 3661);
        assert_eq!(e.msg, "hello");
    }
}
