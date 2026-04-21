use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::params_enum;

use super::MasterSequencer;

module_params! {
    MasterSequencerParams {
        bpm:           Float,
        rows_per_beat: Int,
        autostart:     Bool,
        swing:         Float,
        sync:          Enum<SyncMode>,
    }
}
// Note: `loop` is a Rust keyword — cannot be used as ident in module_params!.
// The "loop" bool param remains as a string literal at its call site.

params_enum! {
    pub enum SyncMode {
        Auto => "auto",
        Free => "free",
        Host => "host",
    }
}

impl MasterSequencer {
    pub(super) fn apply_params(&mut self, p: &ParamView<'_>) {
        self.core.bpm = p.get(params::bpm);
        self.core.rows_per_beat = p.get(params::rows_per_beat);
        let song = p.int("song");
        self.core.song_index = if song < 0 { None } else { Some(song as usize) };
        self.core.do_loop = p.bool("loop");
        let autostart = p.get(params::autostart);
        self.autostart = autostart;
        if autostart && !self.use_host_transport {
            self.core.start_playback();
        }
        self.core.swing = p.get(params::swing);
        let sync: SyncMode = p.get(params::sync);
        self.use_host_transport = match sync {
            SyncMode::Free => false,
            SyncMode::Host => true,
            SyncMode::Auto => self.hosted,
        };
    }
}
