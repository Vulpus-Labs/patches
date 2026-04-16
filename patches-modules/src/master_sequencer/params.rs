use patches_core::parameter_map::{ParameterMap, ParameterValue};

use super::{MasterSequencer, TransportState};

impl MasterSequencer {
    pub(super) fn apply_params(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("bpm") {
            self.bpm = *v;
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("rows_per_beat") {
            self.rows_per_beat = *v;
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("song") {
            self.song_index = if *v < 0 { None } else { Some(*v as usize) };
        }
        if let Some(ParameterValue::Bool(v)) = params.get_scalar("loop") {
            self.do_loop = *v;
        }
        if let Some(ParameterValue::Bool(v)) = params.get_scalar("autostart") {
            self.autostart = *v;
            if self.autostart && !self.use_host_transport {
                self.state = TransportState::Playing;
                self.reset_position();
            }
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("swing") {
            self.swing = *v;
        }
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("sync") {
            self.use_host_transport = match *v {
                "free" => false,
                "host" => true,
                _ /* auto */ => self.hosted,
            };
        }
    }
}
