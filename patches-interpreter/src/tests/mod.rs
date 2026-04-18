//! Integration tests for the patches-interpreter crate. Split by category
//! from the original 717-line `tests.rs` per ticket 0536. Shared fixtures
//! and helpers live here; behaviour-specific tests live in sibling
//! submodules.

#![allow(unused_imports)]

pub(super) use super::*;

use patches_dsl::flat::{
    FlatConnection, FlatModule, FlatPatch, FlatPatternChannel, FlatPatternDef, FlatSongDef,
    FlatSongRow,
};
use patches_dsl::ast::{Ident, Scalar, SourceId, Span, Step, Value};
use patches_dsl::Provenance;

pub(super) fn span() -> Span {
    Span::synthetic()
}

pub(super) fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

pub(super) fn registry() -> Registry {
    patches_modules::default_registry()
}

pub(super) fn osc_module(id: &str) -> FlatModule {
    FlatModule {
        id: id.into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    }
}

pub(super) fn sum_module(id: &str, channels: i64) -> FlatModule {
    FlatModule {
        id: id.into(),
        type_name: "Sum".to_string(),
        shape: vec![("channels".to_string(), Scalar::Int(channels))],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    }
}

pub(super) fn connection(
    from_module: &str, from_port: &str, from_index: u32,
    to_module: &str, to_port: &str, to_index: u32,
) -> FlatConnection {
    let prov = Provenance::root(span());
    FlatConnection {
        from_module: from_module.into(),
        from_port: from_port.to_string(),
        from_index,
        to_module: to_module.into(),
        to_port: to_port.to_string(),
        to_index,
        scale: 1.0,
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }
}

pub(super) fn empty_flat() -> FlatPatch {
    FlatPatch::default()
}

pub(super) fn trigger_step() -> Step {
    Step { cv1: 0.0, cv2: 0.0, trigger: true, gate: true, cv1_end: None, cv2_end: None, repeat: 1 }
}

pub(super) fn rest_step() -> Step {
    Step { cv1: 0.0, cv2: 0.0, trigger: false, gate: false, cv1_end: None, cv2_end: None, repeat: 1 }
}

pub(super) fn ident(name: &str) -> Ident {
    Ident { name: name.into(), span: span() }
}

mod happy_path;
mod errors;
mod song_sequencer;
