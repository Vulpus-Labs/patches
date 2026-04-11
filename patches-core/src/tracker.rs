//! Runtime tracker data types for pattern sequencing.
//!
//! These types live inside `Arc<TrackerData>` and are read by modules on the
//! audio thread. All structures are optimised for read access: flat arrays,
//! integer indexing, no strings in the hot path.
//!
//! See ADR 0029 for the full design.

use std::collections::HashMap;
use std::sync::Arc;

/// A single step in a pattern channel.
///
/// Every step produces four values: `cv1`, `cv2`, `trigger`, and `gate`.
/// Optional `cv1_end` / `cv2_end` specify slide targets; `repeat` subdivides
/// the tick into multiple evenly-spaced triggers.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub cv1: f32,
    pub cv2: f32,
    pub trigger: bool,
    pub gate: bool,
    /// Slide target for cv1 (interpolates from `cv1` to `cv1_end` over the tick).
    pub cv1_end: Option<f32>,
    /// Slide target for cv2 (interpolates from `cv2` to `cv2_end` over the tick).
    pub cv2_end: Option<f32>,
    /// Repeat count: 1 = normal, >1 = subdivide the tick into `repeat` triggers.
    pub repeat: u8,
}

/// A multi-channel grid of step data.
///
/// Indexed by `[channel][step]`. The channel count and step count are stored
/// explicitly for bounds checking.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// Number of channels in this pattern.
    pub channels: usize,
    /// Number of steps per channel.
    pub steps: usize,
    /// Step data indexed as `[channel][step]`.
    pub data: Vec<Vec<Step>>,
}

/// A collection of patterns indexed by bank position.
///
/// Patterns are assigned bank indices by alphabetical sort on their names.
/// The name-to-index mapping is resolved at interpret time and encoded into
/// the `Song` order table.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternBank {
    pub patterns: Vec<Pattern>,
}

/// A song arrangement — which patterns play in which order across channels.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    /// Number of song-level channels.
    pub channels: usize,
    /// Order table: `[row][channel]` → pattern bank index.
    /// `None` indicates silence on that channel for that row.
    pub order: Vec<Vec<Option<usize>>>,
    /// Row index to loop back to (0 if no `@loop` annotation).
    pub loop_point: usize,
}

/// A named collection of songs.
#[derive(Debug, Clone, PartialEq)]
pub struct SongBank {
    pub songs: HashMap<String, Song>,
}

/// All pattern and song data for a patch, shared via `Arc`.
///
/// Distributed to modules at plan activation. The audio thread reads through
/// the `Arc` — no atomics, no contention on the read path.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackerData {
    pub patterns: PatternBank,
    pub songs: SongBank,
}

/// Opt-in trait for modules that receive tracker data (patterns and songs).
///
/// Modules that want tracker data implement this trait and override
/// [`Module::as_tracker_data_receiver`](crate::Module::as_tracker_data_receiver)
/// to return `Some(self)`. Modules that do not implement this trait pay zero
/// cost — the planner ignores them.
///
/// Called once per plan activation with `Arc::clone` (ref-count bump only).
/// Implementations must not allocate, block, or perform I/O.
pub trait ReceivesTrackerData {
    /// Receive tracker data at plan activation.
    ///
    /// The `Arc` is cloned (ref-count bump) once per module. The audio thread's
    /// read path is plain pointer dereference through the `Arc`.
    fn receive_tracker_data(&mut self, data: Arc<TrackerData>);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_data_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TrackerData>();
        assert_send_sync::<Arc<TrackerData>>();
    }

    #[test]
    fn empty_tracker_data() {
        let data = TrackerData {
            patterns: PatternBank { patterns: vec![] },
            songs: SongBank { songs: HashMap::new() },
        };
        assert_eq!(data.patterns.patterns.len(), 0);
        assert_eq!(data.songs.songs.len(), 0);
    }

    #[test]
    fn pattern_bank_indexing() {
        let step = Step {
            cv1: 1.0,
            cv2: 0.5,
            trigger: true,
            gate: true,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
        };
        let pattern = Pattern {
            channels: 1,
            steps: 2,
            data: vec![vec![step.clone(), step]],
        };
        let bank = PatternBank { patterns: vec![pattern] };
        assert_eq!(bank.patterns[0].channels, 1);
        assert_eq!(bank.patterns[0].steps, 2);
        assert_eq!(bank.patterns[0].data[0].len(), 2);
    }

    #[test]
    fn song_order_and_loop_point() {
        let song = Song {
            channels: 2,
            order: vec![
                vec![Some(0), Some(1)],
                vec![Some(0), Some(1)],
            ],
            loop_point: 1,
        };
        assert_eq!(song.order.len(), 2);
        assert_eq!(song.loop_point, 1);
        assert_eq!(song.order[0][0], Some(0));
    }

    #[test]
    fn song_silence_entries() {
        let song = Song {
            channels: 2,
            order: vec![vec![Some(0), None]],
            loop_point: 0,
        };
        assert_eq!(song.order[0][1], None);
    }

    #[test]
    fn step_slide_fields() {
        let step = Step {
            cv1: 0.0,
            cv2: 0.0,
            trigger: true,
            gate: true,
            cv1_end: Some(1.0),
            cv2_end: Some(0.8),
            repeat: 3,
        };
        assert_eq!(step.cv1_end, Some(1.0));
        assert_eq!(step.cv2_end, Some(0.8));
        assert_eq!(step.repeat, 3);
    }
}
