use patches_core::params_enum;
use patches_core::parameter_map::{ParameterMap, ParameterValue};

use super::MasterSequencer;

params_enum! {
    pub enum SyncMode {
        Auto => "auto",
        Free => "free",
        Host => "host",
    }
}

impl MasterSequencer {
    pub(super) fn apply_params(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("bpm") {
            self.core.bpm = *v;
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("rows_per_beat") {
            self.core.rows_per_beat = *v;
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("song") {
            self.core.song_index = if *v < 0 { None } else { Some(*v as usize) };
        }
        if let Some(ParameterValue::Bool(v)) = params.get_scalar("loop") {
            self.core.do_loop = *v;
        }
        if let Some(ParameterValue::Bool(v)) = params.get_scalar("autostart") {
            self.autostart = *v;
            if self.autostart && !self.use_host_transport {
                self.core.start_playback();
            }
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("swing") {
            self.core.swing = *v;
        }
        if let Some(&ParameterValue::Enum(v)) = params.get_scalar("sync") {
            self.use_host_transport = match SyncMode::try_from(v) {
                Ok(SyncMode::Free) => false,
                Ok(SyncMode::Host) => true,
                _ => self.hosted,
            };
        }
    }
}
