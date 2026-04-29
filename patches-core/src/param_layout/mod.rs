//! Fixed-layout scratch-buffer descriptor (ADR 0045 §3, Spike 1).
//!
//! Computes a deterministic [`ParamLayout`] from a
//! [`crate::ModuleDescriptor`]: ordered scalar slots with natural
//! alignment, an indexed buffer-slot table for Arc-handle parameters, and a
//! stable 64-bit `descriptor_hash` used by the host/plugin load-time drift
//! check (Spike 7).
//!
//! This module is a pure function surface. No runtime wiring, no audio-thread
//! entry points; later spikes consume the layout and build the packer / view
//! on top.

use crate::cables::CableKind;
use crate::modules::module_descriptor::{ModuleDescriptor, ParameterKind};
use crate::modules::parameter_map::ParameterKey;

mod hash;
#[cfg(test)]
mod tests;

pub use hash::descriptor_hash;

/// Fixed wire tag for a scalar parameter slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarTag {
    Float,
    Int,
    Bool,
    Enum,
}

impl ScalarTag {
    pub const fn size(self) -> u32 {
        match self {
            ScalarTag::Float => 4,
            ScalarTag::Int => 8,
            ScalarTag::Bool => 1,
            ScalarTag::Enum => 4,
        }
    }

    pub const fn align(self) -> u32 {
        match self {
            ScalarTag::Float => 4,
            ScalarTag::Int => 8,
            ScalarTag::Bool => 1,
            ScalarTag::Enum => 4,
        }
    }

}

/// A single scalar parameter's position in the packed scalar area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarSlot {
    pub key: ParameterKey,
    pub offset: u32,
    pub tag: ScalarTag,
}

/// Deterministic layout for a module's packed parameter frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamLayout {
    pub scalar_size: u32,
    pub scalars: Vec<ScalarSlot>,
    pub descriptor_hash: u64,
}

/// Compute the packed-frame layout for a module.
///
/// Order:
///   1. Partition parameters into scalar / buffer tables.
///   2. Sort each by `(name, index)` — canonical and independent of
///      declaration order in the descriptor.
///   3. Greedy-pack scalars with natural alignment.
///   4. `scalar_size` rounded up to the max scalar alignment observed, so
///      arrays of frames stay aligned.
///   5. Buffer slots indexed `0..` in canonical order.
///   6. Hash over a canonical byte encoding of shape (params + ports).
pub fn compute_layout(descriptor: &ModuleDescriptor) -> ParamLayout {
    let mut scalars_in: Vec<(ParameterKey, ScalarTag)> = Vec::new();

    for p in &descriptor.realtime_params {
        let key = ParameterKey::new(p.name, p.index);
        let tag = scalar_tag(&p.parameter_type);
        scalars_in.push((key, tag));
    }

    scalars_in.sort_by(|a, b| key_cmp(&a.0, &b.0));

    let mut offset: u32 = 0;
    let mut max_align: u32 = 1;
    let mut scalars: Vec<ScalarSlot> = Vec::with_capacity(scalars_in.len());
    for (key, tag) in scalars_in {
        let align = tag.align();
        if align > max_align {
            max_align = align;
        }
        offset = align_up(offset, align);
        scalars.push(ScalarSlot { key, offset, tag });
        offset += tag.size();
    }
    let scalar_size = align_up(offset, max_align);

    let descriptor_hash = hash::descriptor_hash(descriptor);

    ParamLayout { scalar_size, scalars, descriptor_hash }
}

/// Back-compat shim — delegates to [`ParameterMap::defaults`].
///
/// Callers should migrate to the associated constructor directly.
pub fn defaults_from_descriptor(
    desc: &crate::modules::module_descriptor::ModuleDescriptor,
) -> crate::modules::parameter_map::ParameterMap {
    crate::modules::parameter_map::ParameterMap::defaults(desc)
}

fn align_up(offset: u32, align: u32) -> u32 {
    debug_assert!(align.is_power_of_two());
    (offset + align - 1) & !(align - 1)
}

fn key_cmp(a: &ParameterKey, b: &ParameterKey) -> std::cmp::Ordering {
    a.name.cmp(&b.name).then_with(|| a.index.cmp(&b.index))
}

fn scalar_tag(kind: &ParameterKind) -> ScalarTag {
    match kind {
        ParameterKind::Float { .. } => ScalarTag::Float,
        ParameterKind::Int { .. } => ScalarTag::Int,
        ParameterKind::Bool { .. } => ScalarTag::Bool,
        ParameterKind::Enum { .. } => ScalarTag::Enum,
        ParameterKind::SongName => ScalarTag::Int,
        ParameterKind::File { .. } => unreachable!(
            "ParameterKind::File only valid in structural_params, not realtime_params"
        ),
    }
}

/// Canonical per-parameter kind tag for the hash encoding.
pub(crate) fn param_kind_tag(kind: &ParameterKind) -> u8 {
    match kind {
        ParameterKind::Float { .. } => 0,
        ParameterKind::Int { .. } => 1,
        ParameterKind::Bool { .. } => 2,
        ParameterKind::Enum { .. } => 3,
        ParameterKind::File { .. } => 4,
        ParameterKind::SongName => 5,
    }
}

pub(crate) fn port_kind_tag(kind: &CableKind) -> u8 {
    match kind {
        CableKind::Mono => 0,
        CableKind::Poly => 1,
        CableKind::Stereo => 2,
    }
}

pub(crate) fn mono_layout_tag(layout: crate::cables::MonoLayout) -> u8 {
    match layout {
        crate::cables::MonoLayout::Audio => 0,
        crate::cables::MonoLayout::Trigger => 1,
    }
}

pub(crate) fn poly_layout_tag(layout: crate::cables::PolyLayout) -> u8 {
    match layout {
        crate::cables::PolyLayout::Audio => 0,
        crate::cables::PolyLayout::Trigger => 1,
        crate::cables::PolyLayout::Transport => 2,
        crate::cables::PolyLayout::Midi => 3,
    }
}

