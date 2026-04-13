/// Zero-cost accessor for the `GLOBAL_TRANSPORT` poly lane layout (ADR 0031).
///
/// Provides named read/write methods over `[f32; 16]`, replacing bare
/// `TRANSPORT_*` lane-index constants with a single-point-of-definition
/// accessor layer.
///
/// # Lane layout
///
/// | Lane | Field          | Type   |
/// |------|----------------|--------|
/// | 0    | sample_count   | f32    |
/// | 1    | playing        | bool   |
/// | 2    | tempo          | f32    |
/// | 3    | beat           | f32    |
/// | 4    | bar            | f32    |
/// | 5    | beat_trigger   | bool   |
/// | 6    | bar_trigger    | bool   |
/// | 7    | tsig_num       | f32    |
/// | 8    | tsig_denom     | f32    |
pub struct TransportFrame;

impl TransportFrame {
    pub const SAMPLE_COUNT: usize = 0;
    pub const PLAYING: usize = 1;
    pub const TEMPO: usize = 2;
    pub const BEAT: usize = 3;
    pub const BAR: usize = 4;
    pub const BEAT_TRIGGER: usize = 5;
    pub const BAR_TRIGGER: usize = 6;
    pub const TSIG_NUM: usize = 7;
    pub const TSIG_DENOM: usize = 8;

    pub fn sample_count(frame: &[f32; 16]) -> f32 {
        frame[Self::SAMPLE_COUNT]
    }

    pub fn set_sample_count(frame: &mut [f32; 16], count: f32) {
        frame[Self::SAMPLE_COUNT] = count;
    }

    pub fn playing(frame: &[f32; 16]) -> bool {
        frame[Self::PLAYING] != 0.0
    }

    pub fn set_playing(frame: &mut [f32; 16], playing: bool) {
        frame[Self::PLAYING] = if playing { 1.0 } else { 0.0 };
    }

    pub fn playing_raw(frame: &[f32; 16]) -> f32 {
        frame[Self::PLAYING]
    }

    pub fn set_playing_raw(frame: &mut [f32; 16], value: f32) {
        frame[Self::PLAYING] = value;
    }

    pub fn tempo(frame: &[f32; 16]) -> f32 {
        frame[Self::TEMPO]
    }

    pub fn set_tempo(frame: &mut [f32; 16], tempo: f32) {
        frame[Self::TEMPO] = tempo;
    }

    pub fn beat(frame: &[f32; 16]) -> f32 {
        frame[Self::BEAT]
    }

    pub fn set_beat(frame: &mut [f32; 16], beat: f32) {
        frame[Self::BEAT] = beat;
    }

    pub fn bar(frame: &[f32; 16]) -> f32 {
        frame[Self::BAR]
    }

    pub fn set_bar(frame: &mut [f32; 16], bar: f32) {
        frame[Self::BAR] = bar;
    }

    pub fn beat_trigger(frame: &[f32; 16]) -> f32 {
        frame[Self::BEAT_TRIGGER]
    }

    pub fn set_beat_trigger(frame: &mut [f32; 16], trigger: f32) {
        frame[Self::BEAT_TRIGGER] = trigger;
    }

    pub fn bar_trigger(frame: &[f32; 16]) -> f32 {
        frame[Self::BAR_TRIGGER]
    }

    pub fn set_bar_trigger(frame: &mut [f32; 16], trigger: f32) {
        frame[Self::BAR_TRIGGER] = trigger;
    }

    pub fn tsig_num(frame: &[f32; 16]) -> f32 {
        frame[Self::TSIG_NUM]
    }

    pub fn set_tsig_num(frame: &mut [f32; 16], num: f32) {
        frame[Self::TSIG_NUM] = num;
    }

    pub fn tsig_denom(frame: &[f32; 16]) -> f32 {
        frame[Self::TSIG_DENOM]
    }

    pub fn set_tsig_denom(frame: &mut [f32; 16], denom: f32) {
        frame[Self::TSIG_DENOM] = denom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_count_round_trip() {
        let mut frame = [0.0f32; 16];
        TransportFrame::set_sample_count(&mut frame, 42.0);
        assert_eq!(TransportFrame::sample_count(&frame), 42.0);
    }

    #[test]
    fn playing_bool_round_trip() {
        let mut frame = [0.0f32; 16];
        assert!(!TransportFrame::playing(&frame));
        TransportFrame::set_playing(&mut frame, true);
        assert!(TransportFrame::playing(&frame));
        TransportFrame::set_playing(&mut frame, false);
        assert!(!TransportFrame::playing(&frame));
    }

    #[test]
    fn playing_raw_round_trip() {
        let mut frame = [0.0f32; 16];
        TransportFrame::set_playing_raw(&mut frame, 1.0);
        assert_eq!(TransportFrame::playing_raw(&frame), 1.0);
    }

    #[test]
    fn tempo_round_trip() {
        let mut frame = [0.0f32; 16];
        TransportFrame::set_tempo(&mut frame, 120.0);
        assert_eq!(TransportFrame::tempo(&frame), 120.0);
    }

    #[test]
    fn beat_round_trip() {
        let mut frame = [0.0f32; 16];
        TransportFrame::set_beat(&mut frame, 3.5);
        assert_eq!(TransportFrame::beat(&frame), 3.5);
    }

    #[test]
    fn bar_round_trip() {
        let mut frame = [0.0f32; 16];
        TransportFrame::set_bar(&mut frame, 7.0);
        assert_eq!(TransportFrame::bar(&frame), 7.0);
    }

    #[test]
    fn triggers_round_trip() {
        let mut frame = [0.0f32; 16];
        TransportFrame::set_beat_trigger(&mut frame, 1.0);
        TransportFrame::set_bar_trigger(&mut frame, 1.0);
        assert_eq!(TransportFrame::beat_trigger(&frame), 1.0);
        assert_eq!(TransportFrame::bar_trigger(&frame), 1.0);
    }

    #[test]
    fn time_signature_round_trip() {
        let mut frame = [0.0f32; 16];
        TransportFrame::set_tsig_num(&mut frame, 3.0);
        TransportFrame::set_tsig_denom(&mut frame, 4.0);
        assert_eq!(TransportFrame::tsig_num(&frame), 3.0);
        assert_eq!(TransportFrame::tsig_denom(&frame), 4.0);
    }

    #[test]
    fn lanes_do_not_overlap() {
        let lanes = [
            TransportFrame::SAMPLE_COUNT,
            TransportFrame::PLAYING,
            TransportFrame::TEMPO,
            TransportFrame::BEAT,
            TransportFrame::BAR,
            TransportFrame::BEAT_TRIGGER,
            TransportFrame::BAR_TRIGGER,
            TransportFrame::TSIG_NUM,
            TransportFrame::TSIG_DENOM,
        ];
        for (i, &a) in lanes.iter().enumerate() {
            for &b in &lanes[i + 1..] {
                assert_ne!(a, b, "lane indices must be unique");
            }
        }
    }

    #[test]
    fn all_lanes_within_bounds() {
        let lanes = [
            TransportFrame::SAMPLE_COUNT,
            TransportFrame::PLAYING,
            TransportFrame::TEMPO,
            TransportFrame::BEAT,
            TransportFrame::BAR,
            TransportFrame::BEAT_TRIGGER,
            TransportFrame::BAR_TRIGGER,
            TransportFrame::TSIG_NUM,
            TransportFrame::TSIG_DENOM,
        ];
        for &lane in &lanes {
            assert!(lane < 16, "lane {lane} out of bounds for [f32; 16]");
        }
    }
}
