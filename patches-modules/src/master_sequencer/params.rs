use patches_core::param_frame::ParamView;
use patches_core::params_enum;

use super::MasterSequencer;

params_enum! {
    pub enum SyncMode {
        Auto => "auto",
        Free => "free",
        Host => "host",
    }
}

impl MasterSequencer {
    pub(super) fn apply_params(&mut self, params: &ParamView<'_>) {
        self.core.bpm = params.float("bpm");
        self.core.rows_per_beat = params.int("rows_per_beat");
        let song = params.int("song");
        self.core.song_index = if song < 0 { None } else { Some(song as usize) };
        self.core.do_loop = params.bool("loop");
        let autostart = params.bool("autostart");
        self.autostart = autostart;
        if autostart && !self.use_host_transport {
            self.core.start_playback();
        }
        self.core.swing = params.float("swing");
        let sync = params.enum_variant("sync");
        self.use_host_transport = match SyncMode::try_from(sync) {
            Ok(SyncMode::Free) => false,
            Ok(SyncMode::Host) => true,
            _ => self.hosted,
        };
    }
}
